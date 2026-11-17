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


def row(passed: int, commit: str, dirty: str = "no", total: int = 10) -> measure.Row:
    return measure.Row("2026-11-17T00:00:00Z", commit, dirty, "-", "corpus.units", passed, total, "t", "/e")


class Decisions(unittest.TestCase):
    def test_a_baseline_unit_that_fails_or_is_gone_is_lost(self):
        units = [{"id": "a", "status": "pass"}, {"id": "b", "status": "fail"}, {"id": "d", "status": "pass"}]
        self.assertEqual(gate.lost_units(["a", "b", "c"], units), ["b", "c"])

    def test_only_clean_rows_of_contained_commits_with_the_same_total_count(self):
        history = [row(7, "x"), row(9, "y"), row(8, "x", dirty="yes"), row(30, "x", total=40)]
        contains = {"x": True, "y": False}.__getitem__
        self.assertEqual(gate.fallen_metrics([row(7, "")], history, contains), [])
        self.assertEqual(gate.fallen_metrics([row(6, "")], history, contains),
                         ["corpus.units 6/10 is below 7/10 of x"])


class Steps(unittest.TestCase):
    def setUp(self):
        self.runs = Path(tempfile.mkdtemp(prefix="randoc-gate-test-"))
        self.addCleanup(shutil.rmtree, self.runs, True)
        patch = mock.patch.dict(os.environ, {"RANDOC_RUNS": str(self.runs)})
        patch.start()
        self.addCleanup(patch.stop)
        self.ran: list[str] = []

    def step(self, name: str, reason: str | None) -> gate.Step:
        def run(_gate, log):
            self.ran.append(name)
            log.write_text(f"log of {name}\n")
            return reason
        return gate.Step(name, run)

    def run_gate(self, steps: list[gate.Step]) -> tuple[int, str, str, dict]:
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(gate, "STEPS", steps), contextlib.redirect_stdout(out), \
                contextlib.redirect_stderr(err):
            code = gate.main([])
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
        self.assertEqual([(s["name"], s["status"]) for s in record["steps"]], [("one", "pass"), ("two", "fail")])

    def test_a_gate_with_each_step_passing_exits_0(self):
        code, out, _, record = self.run_gate([self.step("one", None), self.step("two", None)])
        self.assertEqual(code, 0)
        self.assertRegex(out, r"^gate: PASS in \d+s \(one \d+s, two \d+s\); evidence ")
        self.assertEqual(record["result"], "pass")

    def test_the_real_gate_has_each_step_of_the_contract(self):
        self.assertEqual([s.name for s in gate.STEPS],
                         ["format", "licence", "lints", "docs", "tests", "tool-tests", "harness", "measurement"])


if __name__ == "__main__":
    unittest.main()
