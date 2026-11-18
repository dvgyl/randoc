"""Tests of tools/autobuild/gate.py. Run: python3 -m unittest discover -s tools/autobuild/tests"""

from __future__ import annotations

import contextlib
import io
import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import gate  # noqa: E402
import measure  # noqa: E402


def row(passed: int, commit: str, dirty: str = "no", total: int = 10, item: str = "-",
        utc: str = "2026-11-17T00:00:00Z") -> measure.Row:
    return measure.Row(utc, commit, dirty, item, "corpus.units", passed, total, "t", "/e")


class Decisions(unittest.TestCase):
    def test_a_baseline_unit_that_fails_or_is_gone_is_lost(self):
        units = [{"id": "a", "status": "pass"}, {"id": "b", "status": "fail"}, {"id": "d", "status": "pass"}]
        self.assertEqual(gate.lost_units(["a", "b", "c"], units), ["b", "c"])

    def test_only_clean_full_rows_of_contained_commits_with_the_same_total_count(self):
        history = [row(7, "x"), row(9, "y"), row(8, "x", dirty="yes"), row(9, "x", item="M1-01"),
                   row(30, "x", total=40)]
        contains = {"x": True, "y": False}.__getitem__
        clean = gate.clean_history(history, contains, [])
        self.assertEqual(gate.fallen_metrics([row(7, "")], clean), [])
        self.assertEqual(gate.fallen_metrics([row(6, "")], clean), ["corpus.units 6/10 is below 7/10 of x"])

    def test_a_correction_makes_the_older_rows_of_its_metric_obsolete(self):
        history = [row(9, "x", utc="2026-11-17T01:00:00Z"), row(5, "x", utc="2026-11-17T03:00:00Z")]
        correction = [{"utc": "2026-11-17T02:00:00Z", "metric": "corpus.units"}]
        clean = gate.clean_history(history, lambda commit: True, correction)
        self.assertEqual(gate.fallen_metrics([row(5, "")], clean), [])
        self.assertEqual(gate.fallen_metrics([row(4, "")], clean), ["corpus.units 4/10 is below 5/10 of x"])


class Steps(unittest.TestCase):
    def setUp(self):
        self.runs = Path(tempfile.mkdtemp(prefix="randoc-gate-test-"))
        self.addCleanup(shutil.rmtree, self.runs, True)
        for patch in (mock.patch.dict(os.environ, {"RANDOC_RUNS": str(self.runs)}),
                      mock.patch.object(measure, "MIN_FREE_BYTES", 0)):
            patch.start()
            self.addCleanup(patch.stop)
        self.ran: list[str] = []

    def step(self, name: str, reason: str | None) -> gate.Step:
        def run(_gate, log):
            self.ran.append(name)
            log.write_text(f"log of {name}\n")
            return reason
        return gate.Step(name, run)

    def run_gate(self, steps: list[gate.Step], *argv: str) -> tuple[int, str, str, dict]:
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(gate, "STEPS", steps), contextlib.redirect_stdout(out), \
                contextlib.redirect_stderr(err):
            code = gate.main(list(argv))
        [record] = list(self.runs.glob("gate/*/gate.json"))
        return code, out.getvalue(), err.getvalue(), json.loads(record.read_text())

    def test_the_gate_stops_at_the_first_failed_step_and_prints_one_line(self):
        code, out, err, record = self.run_gate(
            [self.step("one", None), self.step("two", "it broke"), self.step("three", None)])
        self.assertEqual(code, 1)
        self.assertEqual(self.ran, ["one", "two"])
        self.assertEqual(len(out.splitlines()), 1)
        self.assertRegex(out, r"^gate: FAIL at two after \d+s: it broke \(one \d+s, two \d+s\); log ")
        self.assertIn("log of two", err)
        self.assertEqual([(s["name"], s["status"]) for s in record["steps"]],
                         [("one", "pass"), ("two", "fail"), ("three", "not-run")])
        self.assertIsInstance(record["changed_outside_plan"], list)

    def test_the_report_of_a_failed_gate_names_each_step_for_the_suite_of_gate_steps(self):
        report = self.runs / "suite-evidence" / "gate-report.json"
        report.parent.mkdir()
        code, _, _, record = self.run_gate([self.step("one", "it broke"), self.step("two", None)],
                                           "--report", str(report))
        self.assertEqual(code, 1, "the gate keeps its exit code; the runner of the suite reads the report")
        self.assertEqual(json.loads(report.read_text()), record)
        self.assertEqual([(s["name"], s["status"]) for s in record["steps"]], [("one", "fail"), ("two", "not-run")])

    def test_a_gate_with_each_step_passing_exits_0(self):
        code, out, _, record = self.run_gate([self.step("one", None), self.step("two", None)])
        self.assertEqual(code, 0)
        self.assertRegex(out, r"^gate: PASS in \d+s \(one \d+s, two \d+s\); evidence ")
        self.assertEqual(record["result"], "pass")

    def test_a_filtered_gate_stops_at_the_matching_step_and_marks_the_report_quick(self):
        steps = [self.step(name, None) for name in ("one", "two", "three")]
        code, _, _, record = self.run_gate(steps, "--filter", "two")
        self.assertEqual(code, 0)
        self.assertEqual(self.ran, ["one", "two"])
        self.assertTrue(record["quick"])
        self.assertEqual(record["filter"], "two")
        self.assertEqual(record["steps"][-1]["status"], "not-run")

    def test_the_real_gate_has_each_step_of_the_contract(self):
        self.assertEqual([s.name for s in gate.STEPS],
                         ["format", "licence", "oracle-sha256", "no-oracle-call", "lints", "docs", "tests",
                          "tool-tests", "oracle-heap-harness", "oracle-heap-pandoc", "oracle-heap-corpus",
                          "oracle-heap-commonmark", "harness", "harness-verdicts", "measurement"])

    def test_the_oracle_check_rejects_a_different_executable(self):
        path = self.runs / "oracle"
        path.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        log = self.runs / "oracle.log"
        with mock.patch.dict(os.environ, {"RANDOC_ORACLE_PANDOC": str(path)}):
            self.assertIn("SHA-256 differs", gate.oracle_sha256(None, log))
        self.assertIn(measure.file_sha256(path), log.read_text())

    def test_the_source_check_rejects_a_direct_call_in_a_crate(self):
        code = Path("crates/converter/src/lib.rs")
        path = self.runs / code
        path.parent.mkdir(parents=True)
        log = self.runs / "source.log"
        with mock.patch.object(gate, "ROOT", self.runs), mock.patch.object(measure, "git", return_value=f"{code}\0"):
            for argument in ['"pandoc"', 'r##"pandoc"##', 'r###"/usr/bin/pandoc"###']:
                with self.subTest(argument=argument):
                    path.write_text(f'fn converter() {{}}\nCommand::new({argument});\n', encoding="utf-8")
                    self.assertIn(f"{code}:2", gate.no_oracle_call(None, log))

    def test_the_source_check_reports_an_unreadable_file_as_a_failed_step(self):
        code = "crates/converter/src/lib.rs"
        with mock.patch.object(gate, "ROOT", self.runs), mock.patch.object(measure, "git", return_value=f"{code}\0"):
            self.assertIn(f"cannot read {code}", gate.no_oracle_call(None, self.runs / "source.log"))


if __name__ == "__main__":
    unittest.main()
