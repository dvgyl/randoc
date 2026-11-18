"""Tests of the unit-level regression check and the suites of the gate's measurement step.
Run: python3 -m unittest discover -s tools/autobuild/tests -p 'test_t2_*.py'"""

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

ENTRY = """
[[metric]]
key = "{suite}.units"
suite = "{suite}"
status = "active"
command = "{{python}} -c \\"import os,sys; open(sys.argv[1] + '/raw-{suite}.txt', 'w').write(os.environ.get('FAKE_FAIL', ''))\\" {{evidence}}"
pass = "p"
total = "t"
"""
UNITS = ("u0", "u1", "u2", "u3")


def fake_runner(suite: str) -> measure.Runner:
    def parse(evidence: Path, log: str) -> measure.Counts:
        failing = (evidence / f"raw-{suite}.txt").read_text().split(",")
        units = [measure.Unit(u, measure.FAIL if u in failing else measure.PASS, (f"{suite}.units",)) for u in UNITS]
        return measure.Counts.of(units, (f"{suite}.units",))
    return measure.Runner(parse, "--only", "tools/autobuild")


class LostUnits(unittest.TestCase):
    def test_a_unit_that_passed_and_fails_now_is_lost_unless_its_metric_is_corrected(self):
        before = {"passing": ["a", "b", "c"]}
        now = {"passing": ["b"], "failing": ["a", "c", "new"],
               "failing_by_metric": {"s.units": ["a", "c", "new"], "s.part": ["c"]}}
        self.assertEqual(gate.lost_by_suite(now, before, set()), ["a", "c"])
        self.assertEqual(gate.lost_by_suite(now, before, {"s.part"}), ["a"])
        self.assertEqual(gate.lost_by_suite(now, before, {"s.units"}), [])
        before["passing_by_metric"] = {"s.units": ["a", "b", "c"], "s.part": ["a", "c"]}
        self.assertEqual(gate.lost_by_suite(now, before, {"s.part"}), ["a"], "a metric that a now passes is no cover")

    def test_a_unit_that_passed_and_is_gone_or_excluded_now_is_lost_unless_a_metric_that_it_passed_is_corrected(self):
        before = {"passing": ["a", "gone", "flaky"],
                  "passing_by_metric": {"s.units": ["a", "gone", "flaky"], "s.part": ["gone"]}}
        now = {"passing": ["a"], "failing": [], "unstable": ["flaky"],
               "failing_by_metric": {"s.units": [], "s.part": []}}
        self.assertEqual(gate.lost_by_suite(now, before, set()), ["flaky", "gone"])
        self.assertEqual(gate.lost_by_suite(now, before, {"other.units"}), ["flaky", "gone"])
        self.assertEqual(gate.lost_by_suite(now, before, {"s.part"}), ["flaky"])
        self.assertEqual(gate.lost_by_suite(now, before, {"s.units"}), [])

    def test_a_correction_never_covers_a_gone_unit_with_no_metric_list(self):
        before = {"passing": ["a", "gone"], "rows": [{"metric": "s.units"}, {"metric": "s.part"}]}
        now = {"passing": ["a"], "failing_by_metric": {"s.units": [], "s.part": []},
               "rows": [{"metric": "s.units"}, {"metric": "s.part"}]}
        self.assertEqual(gate.lost_by_suite(now, before, {"s.units", "s.part"}), ["gone"])


class MeasurementStep(unittest.TestCase):
    def setUp(self):
        self.folder = Path(tempfile.mkdtemp(prefix="randoc-t2-gate-"))
        registry = self.folder / "metrics.toml"
        registry.write_text("".join(ENTRY.format(suite=s) for s in ("alpha", "beta")), encoding="utf-8")
        (self.folder / "plan").mkdir()
        self.board = self.folder / "plan" / "build.tsv"
        self.head = measure.git("rev-parse", "HEAD")
        patches = [
            mock.patch.object(measure, "METRICS_FILE", registry),
            mock.patch.object(measure, "METRICS_DIR", self.folder / "none"),
            mock.patch.object(measure, "PLAN", self.folder / "plan"),
            mock.patch.object(gate, "BUILD_SCOREBOARD", self.board),
            mock.patch.dict(measure.RUNNERS, {"alpha": fake_runner("alpha"), "beta": fake_runner("beta")}),
            mock.patch.dict(os.environ, {"RANDOC_RUNS": str(self.folder / "runs"), "RANDOC_ENV": str(self.folder / "env")}),
        ]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)
        os.environ.pop(measure.SLOT_ENV, None)
        self.metrics = {k: m for k, m in measure.load_metrics().items() if m.suite in ("alpha", "beta")}

    def tearDown(self):
        shutil.rmtree(self.folder, ignore_errors=True)

    def run_step(self, failing: str) -> tuple[str | None, str]:
        _, evidence = measure.new_evidence_dir("gate", self.head)
        state = gate.Gate(evidence, None, False, self.metrics)
        log = evidence / "measurement.log"
        with mock.patch.dict(os.environ, {"FAKE_FAIL": failing}), state.stack:
            reason = gate.measurement(state, log)
        return reason, log.read_text()

    def earlier_run(self, utc: str, passing: list[str], suite: str = "alpha") -> None:
        folder = self.folder / "runs" / "measure" / utc.replace(":", "")
        folder.mkdir(parents=True)
        (folder / f"{suite}.json").write_text(json.dumps({"passing": passing, "failing": []}))
        measure.append_rows(self.board, [measure.Row(utc, self.head, "no", "-", f"{suite}.units", len(passing),
                                                     len(UNITS), "t", str(folder / f"{suite}.json"))])

    def test_each_active_suite_runs_and_a_suite_with_no_earlier_evidence_passes_with_a_note(self):
        reason, log = self.run_step("u1")
        self.assertIsNone(reason, log)
        self.assertIn("alpha.units 3/4", log)
        self.assertIn("beta.units 3/4", log)
        self.assertIn("NOTE alpha: no earlier clean evidence with a unit list", log)

    def test_a_unit_that_passed_in_the_earlier_clean_run_and_fails_now_fails_the_gate(self):
        self.earlier_run("2026-11-17T01:00:00Z", ["u0", "u2", "u3"])
        reason, log = self.run_step("u1")
        self.assertIsNone(reason, log)
        reason, log = self.run_step("u2")
        self.assertEqual(reason, "1 units passed in the earlier clean run and fail now, the first is alpha u2")
        self.assertIn("LOST alpha u2", log)

    def test_the_gate_never_runs_the_suite_of_gate_steps(self):
        registry = self.folder / "metrics.toml"
        registry.write_text(registry.read_text() + ENTRY.format(suite="gate").replace("gate.units", "gate.steps"))
        with mock.patch.dict(measure.RUNNERS, {"gate": fake_runner("gate")}):
            self.metrics = {k: m for k, m in measure.load_metrics().items() if m.suite in ("alpha", "beta", "gate")}
            self.assertEqual(gate.gate_suites(self.metrics), ["alpha", "beta"])
            reason, log = self.run_step("")
        self.assertIsNone(reason, log)
        self.assertIn("alpha.units 4/4", log)
        self.assertNotIn("gate", log)
        self.assertEqual(list((self.folder / "runs").glob("gate/*/raw-gate.txt")), [])

    def test_a_newer_correction_of_the_metric_covers_the_unit(self):
        self.earlier_run("2026-11-17T01:00:00Z", ["u0", "u1", "u2"])
        (self.folder / "plan" / "corrections.tsv").write_text(
            "utc\tmetric\tcommit\titem\treason\n2026-11-17T02:00:00Z\talpha.units\tc\tM1-07\tstricter\n")
        reason, log = self.run_step("u2")
        self.assertIsNone(reason, log)
        self.assertIn("NOTE alpha: no earlier clean evidence", log)
        self.earlier_run("2026-11-17T03:00:00Z", ["u0", "u1", "u2"])
        reason, _ = self.run_step("u2")
        self.assertIsNotNone(reason)


class ProgramCopies(unittest.TestCase):
    def test_a_copy_that_changes_during_the_gate_fails_the_gate(self):
        folder = Path(tempfile.mkdtemp(prefix="randoc-t2-gate-"))
        self.addCleanup(shutil.rmtree, folder, True)
        (folder / "debug").mkdir()
        (folder / "debug" / "randoc").write_bytes(b"program")
        registry = folder / "metrics.toml"
        registry.write_text(ENTRY.format(suite="alpha").replace("{python} -c", "{randoc} -c"), encoding="utf-8")

        def change(state: gate.Gate, log: Path) -> None:
            copy = Path(state.snapshot().paths["randoc"])
            copy.chmod(0o755)
            copy.write_bytes(b"other")

        patches = [
            mock.patch.object(measure, "METRICS_FILE", registry),
            mock.patch.object(measure, "METRICS_DIR", folder / "none"),
            mock.patch.object(measure, "MIN_FREE_BYTES", 0),
            mock.patch.object(measure, "target_dir", lambda: folder),
            mock.patch.dict(measure.RUNNERS, {"alpha": fake_runner("alpha")}),
            mock.patch.object(gate, "STEPS", [gate.Step("one", change)]),
            mock.patch.dict(os.environ, {"RANDOC_RUNS": str(folder / "runs"), "RANDOC_ENV": str(folder / "env")}),
        ]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)
        os.environ.pop(measure.SLOT_ENV, None)
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(gate.main([]), 1)
        self.assertIn("gate: FAIL at programs", out.getvalue())
        self.assertIn("randoc changed during the run", out.getvalue())


if __name__ == "__main__":
    unittest.main()
