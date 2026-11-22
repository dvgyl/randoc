"""Check the custom writer measurement and report."""

import importlib.util
import json
import sqlite3
import sys
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

AUTOBUILD = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(AUTOBUILD))
import measure

spec = importlib.util.spec_from_file_location(
    "custom_writers_check", AUTOBUILD.parent / "custom-writers/run.py")
custom = importlib.util.module_from_spec(spec)
spec.loader.exec_module(custom)

ORACLE = {"exit": 84, "stdout": "empty", "stderr": "error", "files": {}}


class CustomWriterTests(unittest.TestCase):
    def parse(self, rows, selected=None):
        measure.load_metrics()
        report = {"units": rows, "summary": {"units": len(rows), "selected": selected}}
        with tempfile.TemporaryDirectory() as folder:
            evidence = Path(folder)
            (evidence / custom.REPORT).write_text(json.dumps(report), encoding="utf-8")
            return measure.RUNNERS["custom"].parse(evidence, "")

    def test_full_report_requires_each_writer_to_use_the_same_400_trees(self):
        rows = [{"id": f"writer/{name}/seed/{index}", "status": "pass"}
                for name in custom.PROBES for index in range(400)]
        self.assertEqual(self.parse(rows).rows, {"custom.writers": (4000, 4000)})
        rows[-1]["id"] = "writer/walk/seed/400"
        with self.assertRaisesRegex(measure.ToolError, "same 400 trees"):
            self.parse(rows)

    def test_oracle_errors_count_and_randoc_no_result_fails(self):
        first, second = list(custom.writer_units(
            "reject", "", (), b"script", [("forms/a", b"a"), ("forms/b", b"b")], None))
        rows = [custom.verdict(first, [ORACLE, ORACLE], ORACLE),
                custom.verdict(second, [ORACLE, ORACLE], "Randoc timed out.")]
        counts = self.parse(rows, selected="writer/reject/")
        self.assertEqual(counts.rows, {"custom.writers": (1, 2)})
        self.assertEqual(counts.no_result, [second.id])

    def test_cache_uses_the_script_bytes_and_checks_saved_results(self):
        def unit(script):
            return next(custom.writer_units("binary", "", (), script, [("forms/a", b"a")], None))

        first = unit(b"first script")
        changed = unit(b"changed script")
        with closing(sqlite3.connect(":memory:")) as database, ThreadPoolExecutor(max_workers=1) as pool:
            database.execute("CREATE TABLE refs (key BLOB PRIMARY KEY, pair TEXT NOT NULL)")
            with patch.object(custom.time, "sleep") as pause:
                self.assertEqual(custom.oracle_pairs(database, [first], b"oracle", lambda _: ORACLE,
                                                     pool, False), [[ORACLE, ORACLE]])
                pause.assert_called_once_with(1)
            current = {**ORACLE, "stderr": "different"}
            checked, = custom.oracle_pairs(database, [first], b"oracle", lambda _: current, pool, True)
            self.assertEqual(custom.verdict(first, checked, ORACLE)["unstable_parts"], ["stderr"])
            cached, = custom.oracle_pairs(database, [first], b"oracle", self.fail, pool, False)
            self.assertEqual(cached, checked)
            with patch.object(custom.time, "sleep"):
                pairs = custom.oracle_pairs(database, [changed], b"oracle", lambda _: current, pool, False)
            self.assertEqual(pairs, [[current, current]])

    def overlapping_captures(self, committed, pending):
        unit = next(custom.writer_units("binary", "", (), b"script", [("forms/a", b"a")], None))
        version = b"oracle"
        key = custom.reference_key(version, unit)
        results = iter(pending)
        pool = SimpleNamespace(map=map)
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "writers.sqlite"
            with closing(sqlite3.connect(path)) as database, closing(sqlite3.connect(path, timeout=0)) as other:
                database.execute("CREATE TABLE refs (key BLOB PRIMARY KEY, pair TEXT NOT NULL)")

                def save_during_capture(_seconds):
                    with other:
                        other.execute("INSERT INTO refs VALUES (?, ?)", (key, json.dumps(committed)))

                with patch.object(custom.time, "sleep", side_effect=save_during_capture):
                    checked, = custom.oracle_pairs(database, [unit], version, lambda _: next(results), pool, False)
                cached, = custom.oracle_pairs(database, [unit], version, self.fail, pool, False)
        return unit, checked, cached

    def test_cache_merges_overlapping_captures_in_both_orders(self):
        stdout = {**ORACLE, "stdout": "different output"}
        stderr = {**ORACLE, "stderr": "different error"}
        for left, right, parts in (
            ((ORACLE, ORACLE), (ORACLE, stdout), ["stdout"]),
            ((ORACLE, stderr), (ORACLE, stdout), ["stderr", "stdout"]),
        ):
            for committed, pending in ((left, right), (right, left)):
                with self.subTest(committed=committed, pending=pending):
                    unit, checked, cached = self.overlapping_captures(committed, pending)
                    self.assertEqual(checked, cached)
                    row = custom.verdict(unit, checked, ORACLE)
                    self.assertEqual(row["status"], "unstable")
                    self.assertEqual(row["unstable_parts"], parts)

    def test_cache_keeps_completed_observations_when_the_other_capture_times_out(self):
        stdout = {**ORACLE, "stdout": "different output"}
        stderr = {**ORACLE, "stderr": "different error"}
        timeout = "The oracle timed out."
        for pending in ((stdout, timeout), (timeout, stdout)):
            with self.subTest(pending=pending):
                unit, checked, cached = self.overlapping_captures((ORACLE, stderr), pending)
                row = custom.verdict(unit, checked, ORACLE)
                self.assertEqual(row["status"], "oracle_limit")
                self.assertEqual(row["oracle"], ORACLE)
                self.assertEqual(row["oracle_second"], stderr)
                self.assertEqual(row["oracle_rechecks"], [stdout, timeout])
                recovered = custom.verdict(unit, cached, ORACLE)
                self.assertEqual(recovered["status"], "unstable")
                self.assertEqual(recovered["unstable_parts"], ["stderr", "stdout"])

    def test_randoc_no_result_gets_one_retry_after_all_first_runs(self):
        args = SimpleNamespace(oracle="oracle", randoc="randoc", filter="writer/reject/", verify_oracle=False)
        calls = []

        def run(program, unit, *, oracle=False):
            if oracle:
                return ORACLE
            calls.append(unit.id)
            return "Randoc timed out." if calls == ["writer/reject/forms/a"] else ORACLE

        with patch.object(custom, "attempt", side_effect=run), patch.object(custom.time, "sleep"), \
             patch.object(custom, "ThreadPoolExecutor", return_value=ThreadPoolExecutor(max_workers=1)), \
             closing(sqlite3.connect(":memory:")) as database:
            database.execute("CREATE TABLE refs (key BLOB PRIMARY KEY, pair TEXT NOT NULL)")
            rows = custom.measure(args, [("forms/a", b"a"), ("forms/b", b"b")], database, b"oracle")
        self.assertEqual(calls, ["writer/reject/forms/a", "writer/reject/forms/b", "writer/reject/forms/a"])
        self.assertEqual([row["status"] for row in rows], ["pass", "pass"])


if __name__ == "__main__":
    unittest.main()
