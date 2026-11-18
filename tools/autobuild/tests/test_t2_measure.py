"""Tests of the experiment limit, the attempt records, the slots and the evidence of measure.py.
Run: python3 -m unittest discover -s tools/autobuild/tests -p 'test_t2_*.py'"""

from __future__ import annotations

import contextlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import measure  # noqa: E402

FAKE_TOML = """
[[metric]]
key = "fake.units"
suite = "fake"
status = "active"
command = "{python} -c \\"import json,os,sys; json.dump(dict(passed=int(os.environ['FAKE_PASS'])), open(sys.argv[1] + '/fake.json', 'w')); sys.exit(int(os.environ.get('FAKE_EXIT', '0')))\\" {evidence}"
pass = "p"
total = "t"

[[metric]]
key = "fake.part"
suite = "fake"
status = "active"
command = "{python} -c \\"import json,os,sys; json.dump(dict(passed=int(os.environ['FAKE_PASS'])), open(sys.argv[1] + '/fake.json', 'w')); sys.exit(int(os.environ.get('FAKE_EXIT', '0')))\\" {evidence}"
pass = "p"
total = "t"
"""
ITEMS = "id\tmilestone\tstatus\tmetric\tbaseline\ttarget\ttitle\nM1-01\tM1\tactive\tfake.units\t5/10\t+1\tt\n"


def fake_counts(evidence: Path, log: str) -> measure.Counts:
    passed = json.loads((evidence / "fake.json").read_text())["passed"]
    units = [measure.Unit(f"u{n}", measure.PASS if n < passed else measure.FAIL,
                          ("fake.units",) + (("fake.part",) if n % 2 else ())) for n in range(10)]
    return measure.Counts.of(units, ("fake.units", "fake.part"))


class Flow(unittest.TestCase):
    def setUp(self):
        self.folder = Path(tempfile.mkdtemp(prefix="randoc-t2-measure-"))
        (self.folder / "metrics.toml").write_text(FAKE_TOML, encoding="utf-8")
        (self.folder / "plan").mkdir()
        (self.folder / "plan" / "ITEMS.tsv").write_text(ITEMS, encoding="utf-8")
        self.runs = self.folder / "runs"
        patches = [
            mock.patch.object(measure, "METRICS_FILE", self.folder / "metrics.toml"),
            mock.patch.object(measure, "METRICS_DIR", self.folder / "metrics.d"),
            mock.patch.object(measure, "PLAN", self.folder / "plan"),
            mock.patch.object(measure, "SCOREBOARD", self.folder / "plan" / "scoreboard"),
            mock.patch.object(measure, "MIN_FREE_BYTES", 0),
            mock.patch.dict(measure.RUNNERS, {"fake": measure.Runner(fake_counts, "--only", "tools/autobuild")}),
            mock.patch.dict(os.environ, {"RANDOC_RUNS": str(self.runs), "RANDOC_ENV": str(self.folder / "env")}),
        ]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)
        os.environ.pop(measure.SLOT_ENV, None)

    def tearDown(self):
        shutil.rmtree(self.folder, ignore_errors=True)

    def measure(self, passed: int, *extra: str, exit_code: int = 0) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        env = {"FAKE_PASS": str(passed), "FAKE_EXIT": str(exit_code)}
        with mock.patch.dict(os.environ, env), contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = measure.main(["--no-build", "--stream", "test", *extra])
        return code, out.getvalue(), err.getvalue()

    def rows(self) -> list[measure.Row]:
        return measure.read_rows(self.folder / "plan" / "scoreboard" / "test.tsv")

    def attempts(self) -> list[dict[str, str]]:
        return measure.experiments.read_tsv(self.runs / "attempts.tsv")


class ExperimentLimit(Flow):
    def test_a_fourth_experiment_with_no_gain_is_refused(self):
        for _ in range(3):
            self.assertEqual(self.measure(5, "--item", "M1-01")[0], 0)
        self.assertEqual(len(self.attempts()), 3)
        code, _, err = self.measure(9, "--item", "M1-01")
        self.assertEqual(code, 2)
        self.assertIn("refused: item M1-01 used 3 experiments with no gain on fake.units", err)
        self.assertIn("ops item cut M1-01", err)
        self.assertEqual(len(self.rows()), 6)
        self.assertEqual(len(self.attempts()), 3)

    def test_rows_below_the_coverage_rule_of_the_item_exhaust_it(self):
        items = self.folder / "plan" / "ITEMS.tsv"
        items.write_text(ITEMS.replace("\ttitle\n", "\ttitle\taccept\n").replace("\tt\n", "\tt\tall-pass\n"))
        for _ in range(3):
            self.assertEqual(self.measure(7, "--item", "M1-01")[0], 0)
        code, _, err = self.measure(10, "--item", "M1-01")
        self.assertEqual(code, 2, "7/10 gains over 5/10 but does not meet all-pass")
        self.assertIn("refused: item M1-01 used 3 experiments with no gain", err)

    def test_a_failed_run_counts_as_an_experiment(self):
        code, _, _ = self.measure(5, "--item", "M1-01", exit_code=3)
        self.assertEqual(code, 2)
        self.assertEqual(self.rows(), [])
        [record] = self.attempts()
        self.assertEqual((record["item"], record["suites"], record["commit"]),
                         ("M1-01", "fake", measure.git("rev-parse", "HEAD")))
        self.measure(5, "--item", "M1-01", exit_code=3)
        self.measure(5, "--item", "M1-01", exit_code=3)
        self.assertEqual(self.measure(9, "--item", "M1-01")[0], 2)

    def test_an_item_whose_latest_row_gains_may_measure_again(self):
        self.measure(5, "--item", "M1-01")
        self.measure(5, "--item", "M1-01")
        self.measure(7, "--item", "M1-01")
        self.assertEqual(self.measure(7, "--item", "M1-01")[0], 0)

    def test_a_filtered_run_is_a_diagnostic_with_no_attempt(self):
        for _ in range(3):
            self.measure(5, "--item", "M1-01")
        code, out, _ = self.measure(5, "--item", "M1-01", "--filter", "u1")
        self.assertEqual(code, 0, out)
        self.assertEqual(len(self.attempts()), 3)

    def test_a_run_with_no_item_writes_no_attempt(self):
        self.assertEqual(self.measure(5)[0], 0)
        self.assertEqual(self.attempts(), [])


class Evidence(Flow):
    def test_the_evidence_lists_passing_units_and_the_failing_units_of_each_metric(self):
        self.assertEqual(self.measure(4)[0], 0)
        [first, _] = self.rows()
        evidence = json.loads(Path(first.evidence).read_text())
        self.assertEqual(evidence["passing"], ["u0", "u1", "u2", "u3"])
        self.assertEqual(evidence["failing_by_metric"], {
            "fake.units": ["u4", "u5", "u6", "u7", "u8", "u9"], "fake.part": ["u5", "u7", "u9"]})
        self.assertEqual(evidence["passing_by_metric"], {"fake.units": ["u0", "u1", "u2", "u3"], "fake.part": ["u1", "u3"]})
        self.assertEqual(evidence["programs"], {})

    def test_a_completed_run_writes_its_manifest_and_a_failed_run_does_not(self):
        self.assertEqual(self.measure(4)[0], 0)
        [first, _] = self.rows()
        manifest = json.loads((Path(first.evidence).parent / "run.json").read_text())
        self.assertEqual({k: manifest[k] for k in ("utc", "commit", "item", "stream", "suites", "rows")},
                         {"utc": first.utc, "commit": first.commit, "item": "-", "stream": "test",
                          "suites": ["fake"], "rows": 2})
        self.assertTrue(manifest["end"])
        self.assertEqual(self.measure(4, exit_code=3)[0], 2)
        folders = sorted(p for p in (self.runs / "measure").iterdir() if not p.name.startswith("._"))
        self.assertFalse((folders[-1] / "run.json").exists())

    def test_a_runner_reads_the_report_after_the_exit_codes_that_it_lists(self):
        reporting = measure.Runner(fake_counts, "--only", "tools/autobuild", exits=(0, 3))
        with mock.patch.dict(measure.RUNNERS, {"fake": reporting}):
            code, out, err = self.measure(6, exit_code=3)
        self.assertEqual(code, 0, err)
        self.assertEqual([(r.metric, r.passed) for r in self.rows()], [("fake.part", 3), ("fake.units", 6)])
        code, _, err = self.measure(6, exit_code=1)
        self.assertEqual(code, 2)
        self.assertIn("fake: exit code 1", err)

    def test_a_correction_makes_older_rows_no_reference_for_a_fall(self):
        board = self.folder / "plan" / "scoreboard" / "test.tsv"
        measure.append_rows(board, [measure.Row("2000-01-01T01:00:00Z", "c", "no", "-", "fake.units", 9, 10, "t", "/e")])
        self.assertEqual(self.measure(4)[0], 1)
        (self.folder / "plan" / "corrections.tsv").write_text(
            "utc\tmetric\tcommit\titem\treason\n2000-01-01T02:00:00Z\tfake.units\tc\tM1-01\tstricter\n")
        code, out, _ = self.measure(4)
        self.assertEqual(code, 0, out)


class Machine(Flow):
    def test_a_drive_with_too_little_free_space_is_refused_before_any_attempt(self):
        with mock.patch.object(measure, "MIN_FREE_BYTES", 10**18):
            code, _, err = self.measure(5, "--item", "M1-01")
        self.assertEqual(code, 2)
        self.assertIn("GB free on the drive (a measurement needs", err)
        self.assertEqual(self.attempts(), [])

    def test_two_slots_for_the_machine_and_a_stale_slot_is_taken_again(self):
        first = measure.take_slot()
        second = measure.take_slot()
        self.assertEqual((first.name, second.name), ("measure-1", "measure-2"))
        self.assertIsNone(measure.take_slot())
        ended = subprocess.Popen([sys.executable, "-c", "pass"])
        ended.wait()
        (second / "pid").write_text(f"{ended.pid}\n")
        self.assertIsNone(measure.take_slot())
        self.assertEqual(measure.take_slot().name, "measure-2")

    def test_a_child_of_a_slot_holder_uses_the_slot_of_its_parent(self):
        with measure.measurement_slot() as name:
            self.assertEqual(os.environ[measure.SLOT_ENV], name)
            with measure.measurement_slot() as inner:
                self.assertEqual(inner, name)
            self.assertEqual(sorted(p.name for p in measure.slots_folder().iterdir()
                                    if not p.name.startswith("._")), [name])
        self.assertNotIn(measure.SLOT_ENV, os.environ)
        self.assertEqual([p for p in measure.slots_folder().iterdir() if not p.name.startswith("._")], [])

    def test_a_run_frees_its_slot(self):
        self.assertEqual(self.measure(5)[0], 0)
        self.assertEqual([p for p in measure.slots_folder().iterdir() if not p.name.startswith("._")], [])


if __name__ == "__main__":
    unittest.main()
