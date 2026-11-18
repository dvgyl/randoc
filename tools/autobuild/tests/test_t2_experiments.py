"""Tests of tools/autobuild/hooks/experiments.py: experiments, gain and corrections.
Run: python3 -m unittest discover -s tools/autobuild/tests -p 'test_t2_*.py'"""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from measure import experiments as rules  # noqa: E402


def row(utc: str, passed: int, total: int, item: str = "M1-01", metric: str = "m", commit: str = "c") -> dict[str, str]:
    return {"utc": utc, "commit": commit, "dirty": "no", "item": item, "metric": metric,
            "pass": str(passed), "total": str(total)}


def attempt(item: str = "M1-01") -> dict[str, str]:
    return {"utc": "2026-11-17T00:00:00Z", "commit": "c", "item": item, "suites": "harness", "pid": "1"}


class Experiments(unittest.TestCase):
    def test_the_count_is_the_larger_of_runs_and_attempts(self):
        rows = [row("T1", 5, 9), row("T1", 5, 9, metric="other"), row("T2", 6, 9), row("T3", 1, 1, item="M1-02")]
        self.assertEqual(rules.experiments(rows, [], "M1-01"), 2)
        self.assertEqual(rules.experiments(rows, [attempt()] * 3 + [attempt("M1-02")], "M1-01"), 3)

    def test_a_run_that_failed_before_its_rows_still_exhausts_the_item(self):
        rows = [row("T1", 5, 9)]
        self.assertFalse(rules.exhausted(rows, [attempt()] * 2, "M1-01", "m", (5, 9)))
        self.assertTrue(rules.exhausted(rows, [attempt()] * 3, "M1-01", "m", (5, 9)))
        self.assertFalse(rules.exhausted(rows + [row("T2", 6, 9)], [attempt()] * 3, "M1-01", "m", (5, 9)))

    def test_attempt_records_append_under_one_header(self):
        with tempfile.TemporaryDirectory() as folder:
            path = rules.attempts_file(folder)
            rules.record_attempt(path, "T1", "c1", "M1-01", ["harness", "pandoc"], 11)
            rules.record_attempt(path, "T2", "c2", "M1-01", [], 12)
            self.assertEqual(path.read_text().splitlines(), [
                "utc\tcommit\titem\tsuites\tpid", "T1\tc1\tM1-01\tharness,pandoc\t11", "T2\tc2\tM1-01\t-\t12"])
            self.assertEqual(rules.experiments([], rules.read_tsv(path), "M1-01"), 2)


class Gain(unittest.TestCase):
    def test_the_latest_row_decides_not_the_best_row(self):
        self.assertTrue(rules.gained([row("T1", 6, 9)], "M1-01", "m", (5, 9)))
        self.assertFalse(rules.gained([row("T1", 8, 9), row("T2", 5, 9)], "M1-01", "m", (5, 9)))

    def test_a_higher_pass_with_more_failing_units_is_no_gain(self):
        self.assertFalse(rules.gained([row("T1", 6, 12)], "M1-01", "m", (5, 9)))
        self.assertTrue(rules.gained([row("T1", 6, 10)], "M1-01", "m", (5, 9)))

    def test_a_zero_baseline_gains_with_the_first_row_with_a_total(self):
        self.assertFalse(rules.gained([row("T1", 0, 0)], "M1-01", "m", (0, 0)))
        self.assertTrue(rules.gained([row("T1", 0, 80)], "M1-01", "m", (0, 0)))

    def test_rows_of_other_items_metrics_and_invalid_rows_do_not_count(self):
        rows = [row("T1", 9, 9, item="M1-02"), row("T2", 9, 9, metric="n"), row("T3", 10, 9)]
        self.assertFalse(rules.gained(rows, "M1-01", "m", (5, 9)))
        self.assertFalse(rules.gained([row("T1", 6, 9)], "M1-01", "m", None))

    def test_a_correction_makes_older_rows_obsolete(self):
        rows = [row("2026-11-17T01:00:00Z", 0, 50)]
        correction = [{"utc": "2026-11-17T02:00:00Z", "metric": "m"}]
        self.assertTrue(rules.gained(rows, "M1-01", "m", (0, 0)))
        self.assertFalse(rules.gained(rows, "M1-01", "m", (0, 0), correction))
        later = rows + [row("2026-11-17T03:00:00Z", 30, 50)]
        self.assertTrue(rules.gained(later, "M1-01", "m", (0, 0), correction))

    def test_a_withdrawn_correction_restores_the_older_rows_and_voids_the_rows_of_its_time(self):
        old, during, after = (row("2026-11-17T01:00:00Z", 8, 9), row("2026-11-17T03:00:00Z", 2, 9),
                              row("2026-11-17T05:00:00Z", 7, 9))
        correction = {"utc": "2026-11-17T02:00:00Z", "metric": "m", "item": "M1-01", "withdraws": "-"}
        withdrawal = {"utc": "2026-11-17T04:00:00Z", "metric": "m", "item": "M1-01", "withdraws": correction["utc"]}
        in_force = rules.cutoffs([correction])
        self.assertEqual([rules.counts(r, in_force) for r in (old, during, after)], [False, True, True])
        withdrawn = rules.cutoffs([correction, withdrawal])
        self.assertEqual(withdrawn, {})
        self.assertEqual([rules.counts(r, withdrawn) for r in (old, during, after)], [True, False, True])
        self.assertFalse(rules.gained([during], "M1-01", "m", (0, 0), [correction, withdrawal]))

    def test_the_newest_correction_of_each_metric_is_the_cutoff(self):
        corrections = [{"utc": "T2", "metric": "m"}, {"utc": "T5", "metric": "m"}, {"utc": "T3", "metric": "n"}]
        self.assertEqual(rules.cutoffs(corrections), {"m": "T5", "n": "T3"})
        self.assertEqual(rules.baseline("94/109"), (94, 109))
        self.assertIsNone(rules.baseline("-"))


if __name__ == "__main__":
    unittest.main()
