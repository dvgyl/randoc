"""Tests of the scoreboard rows and the main flow of tools/autobuild/measure.py.
Run: python3 -m unittest discover -s tools/autobuild/tests"""

from __future__ import annotations

import contextlib
import hashlib
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import measure  # noqa: E402

# A fake suite: the command writes PASS from $FAKE_PASS (and exits with $FAKE_EXIT); the parser reads it.
FAKE_TOML = """
[[metric]]
key = "fake.units"
suite = "fake"
status = "active"
command = "{python} -c \\"import json,os,sys; json.dump(dict(passed=int(os.environ['FAKE_PASS']), total=10), open(sys.argv[1] + '/fake.json', 'w')); sys.exit(int(os.environ.get('FAKE_EXIT', '0')))\\" {evidence}"
pass = "p"
total = "t"
"""


def fake_counts(evidence: Path, log: str) -> measure.Counts:
    value = json.loads((evidence / "fake.json").read_text())
    return measure.Counts(rows={"fake.units": (value["passed"], value["total"])}, failing=["u1"])


def row(metric="m", passed=5, total=10, dirty="no", commit="c" * 40) -> measure.Row:
    return measure.Row("2026-11-17T00:00:00Z", commit, dirty, "-", metric, passed, total, "t@1", "/e")


class Rows(unittest.TestCase):
    def test_the_header_is_written_once_and_rows_read_back(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "plan" / "scoreboard" / "build.tsv"
            measure.append_rows(path, [row(passed=1)])
            measure.append_rows(path, [row(passed=2), row(metric="n")])
            lines = path.read_text().splitlines()
            self.assertEqual(lines[0], "\t".join(measure.HEADER))
            self.assertEqual(len(lines), 4)
            self.assertEqual([r.passed for r in measure.read_rows(path)], [1, 2, 5])

    def test_a_fall_is_judged_against_the_best_clean_row_with_the_same_total(self):
        history = [row(passed=7), row(passed=9, dirty="yes"), row(passed=6)]
        self.assertEqual(measure.describe(row(passed=7), history)[1], False)
        text, fell = measure.describe(row(passed=6), history)
        self.assertTrue(fell)
        self.assertIn("+0 against 6/10", text)
        self.assertIn("FELL below the best clean row 7/10", text)
        text, fell = measure.describe(row(passed=1, total=30), history)
        self.assertFalse(fell)
        self.assertIn("TOTAL changed", text)
        self.assertIn("(first row)", measure.describe(row(metric="new"), history)[0])

    def test_the_stream_follows_the_branch(self):
        self.assertEqual(measure.default_stream("gpt/build"), "build")
        self.assertEqual(measure.default_stream("gpt/w/M1-07"), "M1-07")
        self.assertIsNone(measure.default_stream("main"))


class ProgramSnapshot(unittest.TestCase):
    def test_each_suite_of_a_run_uses_one_read_only_copy_that_a_later_build_does_not_change(self):
        with tempfile.TemporaryDirectory() as folder:
            debug = Path(folder) / "debug"
            debug.mkdir()
            (debug / "randoc").write_bytes(b"old program")
            with mock.patch.object(measure, "target_dir", lambda: Path(folder)):
                with measure.program_snapshot(measure.programs_named(["{randoc} -t json"])) as programs:
                    (debug / "randoc").write_bytes(b"new program")
                    copy = Path(programs.paths["randoc"])
                    self.assertEqual(copy.read_bytes(), b"old program")
                    self.assertEqual(programs.sha256["randoc"], hashlib.sha256(b"old program").hexdigest())
                    self.assertNotIn("harness", programs.paths)
                self.assertFalse(copy.exists())
                with self.assertRaisesRegex(measure.ToolError, "randoc changed during the run"):
                    with measure.program_snapshot(["randoc"]) as programs:
                        Path(programs.paths["randoc"]).chmod(0o755)
                        Path(programs.paths["randoc"]).write_bytes(b"changed")
                with self.assertRaisesRegex(measure.ToolError, "cannot copy"):
                    with measure.program_snapshot(["harness"]):
                        pass


class MainFlow(unittest.TestCase):
    def setUp(self):
        self.folder = Path(tempfile.mkdtemp(prefix="randoc-measure-test-"))
        (self.folder / "metrics.toml").write_text(FAKE_TOML, encoding="utf-8")
        self.scoreboard = self.folder / "scoreboard"
        patches = [
            mock.patch.object(measure, "METRICS_FILE", self.folder / "metrics.toml"),
            mock.patch.object(measure, "METRICS_DIR", self.folder / "metrics.d"),
            mock.patch.object(measure, "SCOREBOARD", self.scoreboard),
            mock.patch.object(measure, "PLAN", self.folder / "plan"),
            mock.patch.object(measure, "MIN_FREE_BYTES", 0),
            mock.patch.dict(measure.RUNNERS, {"fake": measure.Runner(fake_counts, "--only", "tools/autobuild")}),
            mock.patch.dict(os.environ, {"RANDOC_RUNS": str(self.folder / "runs"), "RANDOC_ENV": str(self.folder / "env")}),
        ]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)

    def tearDown(self):
        import shutil
        shutil.rmtree(self.folder, ignore_errors=True)

    def measure(self, passed: int, *extra: str, exit_code: int = 0) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        env = {"FAKE_PASS": str(passed), "FAKE_EXIT": str(exit_code)}
        with mock.patch.dict(os.environ, env), contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = measure.main(["--no-build", "--stream", "test", *extra])
        return code, out.getvalue(), err.getvalue()

    def rows(self) -> list[measure.Row]:
        return measure.read_rows(self.scoreboard / "test.tsv")

    def test_a_run_appends_one_row_with_its_evidence(self):
        code, out, _ = self.measure(5, "--item", "M1-01")
        self.assertEqual(code, 0, out)
        [written] = self.rows()
        self.assertEqual((written.item, written.metric, written.passed, written.total), ("M1-01", "fake.units", 5, 10))
        self.assertEqual(written.commit, measure.git("rev-parse", "HEAD"))
        self.assertTrue(written.tool.startswith("fake@"))
        evidence = json.loads(Path(written.evidence).read_text())
        self.assertEqual(evidence["rows"], [{"metric": "fake.units", "pass": 5, "total": 10}])
        self.assertEqual((evidence["failing"], evidence["unstable"]), (["u1"], []))
        self.assertIn("duration_s", evidence)
        self.assertEqual(Path(written.evidence).parent.parent, self.folder / "runs" / "measure")

    def test_a_fall_below_the_best_clean_row_exits_1_and_still_writes_the_row(self):
        measure.append_rows(self.scoreboard / "test.tsv", [row(metric="fake.units", passed=7)])
        code, out, _ = self.measure(6)
        self.assertEqual(code, 1)
        self.assertIn("FELL below the best clean row 7/10", out)
        self.assertEqual([r.passed for r in self.rows()], [7, 6])

    def test_a_filtered_run_writes_no_row(self):
        code, out, _ = self.measure(5, "--filter", "x")
        self.assertEqual(code, 0)
        self.assertIn("filtered run", out)
        self.assertEqual(self.rows(), [])

    def test_a_failed_tool_exits_2_and_writes_no_row(self):
        code, _, err = self.measure(5, exit_code=3)
        self.assertEqual(code, 2)
        self.assertIn("fake: exit code 3", err)
        self.assertEqual(self.rows(), [])


if __name__ == "__main__":
    unittest.main()
