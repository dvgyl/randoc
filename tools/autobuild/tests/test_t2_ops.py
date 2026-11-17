"""Tests of ops: milestone check on one candidate run, corrections, next by metric, the merge re-measure.
Run: python3 -m unittest discover -s tools/autobuild/tests -p 'test_t2_*.py'"""

from __future__ import annotations

import json
import os
import unittest
from pathlib import Path
from unittest import mock

from fixture import ROWS_HEADER, TempRepo, row

from opslib import cli, core, holdout, items
from opslib.corrections import experiments

REGISTRY = """
[[metric]]
key = "harness.units"
suite = "harness"
status = "active"
command = "{harness} run"
pass = "p"
total = "t"

[[metric]]
key = "lua.api"
suite = "lua"
status = "active"
command = "{python} lua.py"
pass = "p"
total = "t"

[[metric]]
key = "caps.extensions"
suite = "harness"
status = "to-build"
command = ""
pass = "p"
total = "t"
"""

MILESTONES = """
[[milestone]]
id = "M1"
title = "First"
[[milestone.require]]
metric = "harness.units"
min_pass = 5
[[milestone.require]]
metric = "lua.api"
exists = true
[[milestone.require]]
metric = "lua.api"
min_total = 80
[[milestone.require]]
metric = "harness.units"
exact_total = 9
"""

FAKE_MEASURE = """import json, os, subprocess, sys
args = sys.argv[1:]
with open(os.environ["FAKE_MEASURE_LOG"], "a") as log:
    log.write(json.dumps(args) + "\\n")
code = int(os.environ.get("FAKE_MEASURE_EXIT", "0"))
if code != 2:
    head = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    item = args[args.index("--item") + 1]
    with open("plan/scoreboard/build.tsv", "a") as board:
        board.write(f"2026-11-17T12:00:00Z\\t{head}\\tno\\t{item}\\tharness.units\\t{os.environ['FAKE_PASS']}\\t9\\tt\\t/e\\n")
sys.exit(code)
"""


class T2Ops(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = TempRepo()
        self.repo.write("tools/autobuild/metrics.toml", REGISTRY)
        self.repo.write("plan/milestones.toml", MILESTONES)
        self.repo.must_commit("chore(tools): registry and milestones [ops]")

    def tearDown(self) -> None:
        self.repo.cleanup()

    def ok(self, *args: str, **env: str) -> str:
        code, out, err = self.repo.ops(*args, **env)
        self.assertEqual(code, 0, f"ops {' '.join(args)} failed:\n{out}\n{err}")
        return out

    def refused(self, words: str, *args: str) -> str:
        code, out, err = self.repo.ops(*args)
        self.assertEqual(code, 1, f"ops {' '.join(args)} did not refuse:\n{out}")
        self.assertIn(words, err)
        return err

    def rows(self, *lines: str) -> None:
        path = self.repo.root / "plan/scoreboard/build.tsv"
        self.repo.write("plan/scoreboard/build.tsv", (path.read_text() if path.exists() else ROWS_HEADER) + "".join(lines))

    def start(self, metric: str = "harness.units", baseline: tuple[int, int] = (5, 9)) -> None:
        self.rows(row("2026-11-17T09:00:00Z", self.repo.head(), "-", metric, *baseline))
        self.ok("item", "add", "M1-01", "--milestone", "M1", "--metric", metric, "--target", "+2", "--title", "cases")
        self.ok("item", "start", "M1-01")

    def check(self, held: str | None = None) -> tuple[int, str]:
        result = (Path("r.json"), {"commit": held}) if held else None
        with mock.patch.object(holdout, "newest", lambda ctx: result):
            code, out, _ = self.repo.ops("milestone", "check", "M1")
        return code, out

    # -- milestone check -------------------------------------------------------

    def test_the_check_uses_the_newest_clean_full_run_and_not_mixed_rows(self) -> None:
        head = self.repo.head()
        self.rows(row("2026-11-17T10:00:00Z", head, "-", "harness.units", 6, 9),
                  row("2026-11-17T10:00:00Z", head, "-", "lua.api", 0, 0),
                  row("2026-11-17T11:00:00Z", head, "-", "lua.api", 90, 100),
                  row("2026-11-17T12:00:00Z", head, "-", "harness.units", 9, 9, dirty="yes"),
                  row("2026-11-17T12:00:00Z", head, "-", "lua.api", 90, 100, dirty="yes"),
                  row("2026-11-17T13:00:00Z", head, "M1-05", "harness.units", 9, 9),
                  row("2026-11-17T13:00:00Z", head, "M1-05", "lua.api", 90, 100))
        code, out = self.check(head)
        self.assertEqual(code, 1)
        self.assertIn(f"candidate run: {head[:12]} at 2026-11-17T10:00:00Z (2 rows)", out)
        self.assertIn("FAIL  lua.api has a row with TOTAL above 0 (0/0)", out)
        self.assertIn("FAIL  lua.api TOTAL >= 80 (0/0)", out)
        self.assertIn("pass  harness.units PASS >= 5 (6/9)", out)
        self.assertIn("pass  harness.units TOTAL = 9 (6/9)", out)
        self.rows(row("2026-11-17T14:00:00Z", head, "-", "harness.units", 6, 9),
                  row("2026-11-17T14:00:00Z", head, "-", "lua.api", 95, 100))
        code, out = self.check(head)
        self.assertEqual(code, 0, out)
        self.assertIn("2026-11-17T14:00:00Z", out)

    def test_no_candidate_fails_each_requirement(self) -> None:
        self.rows(row("2026-11-17T10:00:00Z", self.repo.head(), "-", "harness.units", 6, 9))
        code, out = self.check(self.repo.head())
        self.assertEqual(code, 1)
        self.assertIn("candidate run: none", out)
        self.assertIn("FAIL  harness.units PASS >= 5 (no row)", out)

    def test_invalid_counts_and_a_fall_fail_unless_a_correction_made_the_old_row_obsolete(self) -> None:
        head = self.repo.head()
        self.rows(row("2026-11-17T08:00:00Z", head, "-", "harness.units", 8, 9),
                  row("2026-11-17T08:00:00Z", head, "-", "lua.api", 90, 100),
                  row("2026-11-17T10:00:00Z", head, "-", "harness.units", 6, 9),
                  row("2026-11-17T10:00:00Z", head, "-", "lua.api", 120, 100))
        code, out = self.check(head)
        self.assertEqual(code, 1)
        self.assertIn("FAIL  each row of the candidate run has 0 <= PASS <= TOTAL: lua.api 120/100", out)
        self.assertIn("FAIL  lua.api TOTAL >= 80 (120/100, not 0 <= PASS <= TOTAL)", out)
        self.assertIn("FAIL  no metric of the candidate run is below its best earlier value with the same TOTAL: "
                      "harness.units 6/9 < 8", out)
        self.repo.write("plan/corrections.tsv", "utc\tmetric\tcommit\titem\treason\n"
                        f"2026-11-17T09:00:00Z\tharness.units\t{head}\tM1-01\tstricter\n")
        self.assertIn("pass  no metric of the candidate run is below", self.check(head)[1])

    def test_a_row_older_than_a_correction_of_its_metric_does_not_count(self) -> None:
        head = self.repo.head()
        self.rows(row("2026-11-17T10:00:00Z", head, "-", "harness.units", 6, 9),
                  row("2026-11-17T10:00:00Z", head, "-", "lua.api", 80, 100))
        self.assertEqual(self.check(head)[0], 0)
        self.repo.write("plan/corrections.tsv", "utc\tmetric\tcommit\titem\treason\n"
                        f"2026-11-17T11:00:00Z\tlua.api\t{head}\tM1-01\tstricter\n")
        code, out = self.check(head)
        self.assertEqual(code, 1)
        self.assertIn("FAIL  lua.api has a row with TOTAL above 0 (80/100, older than the correction of "
                      "2026-11-17T11:00:00Z)", out)

    def test_the_held_out_result_is_for_the_candidate_or_a_later_plan_only_commit(self) -> None:
        candidate = self.repo.head()
        self.rows(row("2026-11-17T10:00:00Z", candidate, "-", "harness.units", 6, 9),
                  row("2026-11-17T10:00:00Z", candidate, "-", "lua.api", 80, 100))
        plan_only = self.repo.must_commit("chore(plan): rows [ops]", ["plan"])
        self.assertEqual(self.check(plan_only)[0], 0)
        self.assertEqual(self.check(candidate)[0], 0)
        self.repo.write("tools/x.py", "x = 1\n")
        code_change = self.repo.must_commit("chore(tools): x [ops]")
        code, out = self.check(code_change)
        self.assertEqual(code, 1)
        self.assertIn("FAIL  the newest held-out result measures the candidate commit", out)
        self.assertEqual(self.check(None)[0], 1)

    # -- corrections ---------------------------------------------------------------

    def test_a_correction_is_recorded_and_resets_the_baseline_of_its_item(self) -> None:
        self.start("harness.units", (5, 9))
        self.refused("is not a metric key", "correction", "add", "bad key", "--item", "M1-01", "--reason", "r")
        self.ok("item", "add", "M1-02", "--milestone", "M1", "--metric", "lua.api", "--target", "+1", "--title", "t")
        self.refused("item M1-02 is queued", "correction", "add", "lua.api", "--item", "M1-02", "--reason", "r")
        out = self.ok("correction", "add", "harness.units", "--item", "M1-01", "--reason", "the probe rule is stricter")
        self.assertIn("The baseline of M1-01 is now 0/0", out)
        self.assertEqual(self.repo.out("log", "-1", "--format=%s"), "chore(plan): correct the metric harness.units [ops]")
        [correction] = experiments.read_tsv(self.repo.root / "plan/corrections.tsv")
        self.assertEqual((correction["metric"], correction["item"], correction["reason"]),
                         ("harness.units", "M1-01", "the probe rule is stricter"))
        self.assertIn("M1-01\tM1\tactive\tharness.units\t0/0\t", self.repo.read("plan/ITEMS.tsv"))
        self.assertIn("Tool correction of harness.units by M1-01", self.repo.out("show", "HEAD:plan/REVIEW_LOG.md"))
        self.assertEqual(self.repo.out("status", "--porcelain", "--untracked-files=no"), "")

    # -- next ----------------------------------------------------------------------

    def test_next_lists_each_failing_unit_under_its_most_specific_metric(self) -> None:
        run = self.repo.gpt / "runs" / "measure" / "20261117T100000Z-abc"
        run.mkdir(parents=True)
        write = [f"w/table-{n}#write:html" for n in range(5)]
        read = [f"r/list-{n}#read" for n in range(4)]
        (run / "harness.json").write_text(json.dumps({
            "rows": [{"metric": "harness.units", "pass": 1, "total": 10}, {"metric": "harness.write", "pass": 0, "total": 5},
                     {"metric": "harness.read", "pass": 1, "total": 5}, {"metric": "caps.options", "pass": 1, "total": 109}],
            "failing": write + read, "failing_by_metric": {"harness.units": write + read, "harness.write": write,
                                                           "harness.read": read},
            "unstable": [], "duration_s": 1.0}))
        (run / "corpus.json").write_text(json.dumps({"rows": [{"metric": "corpus.units", "pass": 0, "total": 3}],
                                                      "failing": ["a:json", "b:json", "c:json"]}))
        filtered = self.repo.gpt / "runs" / "measure" / "20261117T110000Z-abc"
        filtered.mkdir()
        (filtered / "corpus.json").write_text(json.dumps({"rows": [], "failing": ["z:json"], "filter": "z"}))
        out = self.ok("next")
        lines = [line for line in out.splitlines() if " units (metric " in line]
        self.assertEqual(len(lines), 3, out)
        self.assertTrue(any("5 units (metric harness.write;" in line for line in lines), out)
        self.assertTrue(any("4 units (metric harness.read;" in line for line in lines), out)
        self.assertTrue(any("3 units (metric unknown;" in line for line in lines), out)
        self.assertIn("note: the evidence of corpus has no failing_by_metric", out)
        self.assertNotIn("caps.options", out)
        self.assertNotIn("z:json", out)

    # -- the re-measure of a worker merge -----------------------------------------

    def remeasure(self, passed: int, exit_code: int = 0) -> bool:
        env = {**self.repo.env(), "FAKE_PASS": str(passed), "FAKE_MEASURE_EXIT": str(exit_code),
               "FAKE_MEASURE_LOG": str(self.repo.tmp / "measure.log")}
        with mock.patch.dict(os.environ, env):
            return items.remeasure_item(core.load_context(self.repo.root), "M1-01")

    def test_remeasure_runs_the_suite_of_the_metric_and_judges_the_newest_row(self) -> None:
        self.repo.write("tools/autobuild/measure.py", FAKE_MEASURE)
        self.repo.must_commit("chore(tools): fake measure [ops]")
        self.start("harness.units", (5, 9))
        self.assertTrue(self.remeasure(7))
        call = json.loads((self.repo.tmp / "measure.log").read_text().splitlines()[-1])
        self.assertEqual(call, ["--suite", "harness", "--item", "M1-01", "--stream", "build"])
        self.assertFalse(self.remeasure(5))
        self.assertFalse(self.remeasure(9, exit_code=2))

    def test_remeasure_of_a_metric_that_is_not_active_shows_no_gain(self) -> None:
        self.repo.write("tools/autobuild/measure.py", FAKE_MEASURE)
        self.repo.must_commit("chore(tools): fake measure [ops]")
        self.start("caps.extensions", (0, 0))
        self.assertFalse(self.remeasure(7))
        self.assertFalse((self.repo.tmp / "measure.log").exists())

    # -- parser lines for the other tool agent --------------------------------------

    def test_the_parser_has_the_new_options(self) -> None:
        parser = cli.parser()
        self.assertEqual(parser.parse_args(["worker", "run", "M1-01", "--effort", "low"]).effort, "low")
        self.assertIsNone(parser.parse_args(["worker", "run", "M1-01"]).effort)
        self.assertEqual(parser.parse_args(["ask", "--reason", "no-next-step", "--evidence", "log", "q"]).evidence, "log")
        self.assertIs(parser.parse_args(["push"]).func, cli.push)


if __name__ == "__main__":
    unittest.main()
