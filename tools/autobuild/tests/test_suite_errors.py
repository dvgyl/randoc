import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

AUTOBUILD = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(AUTOBUILD), str(AUTOBUILD.parent / "errors")]
import errors
import measure


class ErrorSuiteTests(unittest.TestCase):
    def parse(self, rows, *, codes=None, selected=None):
        measure.load_metrics()
        report = {
            "units": rows,
            "summary": {"units": len(rows), "selected": selected},
            "exit_codes": codes,
        }
        with tempfile.TemporaryDirectory() as folder:
            evidence = Path(folder)
            (evidence / errors.REPORT).write_text(json.dumps(report), encoding="utf-8")
            return measure.RUNNERS["errors"].parse(evidence, "")

    def test_trigger_requires_both_oracle_codes_to_match_the_requested_code(self):
        first = {"exit": 6}
        second = {"exit": 1}
        with patch.object(errors, "oracle_pair", return_value=(first, second)):
            confirmed, evidence = errors.confirm_triggers(Path("oracle"), "exit/6", None)
        self.assertEqual(confirmed, [])
        self.assertEqual(evidence["6"]["probes"][0]["runs"], [first, second])
        with patch.object(errors, "oracle_pair", return_value=(first, first)):
            confirmed, evidence = errors.confirm_triggers(Path("oracle"), "exit/6", None)
        self.assertEqual(confirmed[0][0].id, "exit/6")
        self.assertEqual(evidence["6"]["confirmed"], "invalid-columns")

    def test_log_bytes_count_as_a_separate_part(self):
        unit = errors.Unit("log/example", errors.Command(()))
        oracle = {"exit": 0, "stdout": "same", "stderr": "same", "files": {"log.json": "old"}}
        randoc = {**oracle, "files": {"log.json": "new"}}
        row = errors.classify(unit, oracle, oracle, randoc)
        self.assertEqual(row["status"], "fail")
        self.assertEqual(row["differences"], ["files"])
        self.assertEqual(errors.classify(unit, oracle, oracle, oracle)["status"], "pass")

    def test_capture_records_log_bytes_and_omits_unchanged_inputs(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "stub"
            path.write_text(
                '#!/bin/sh\n'
                'test "$GHCRTS" = "-M3g" || exit 42\n'
                'test -f input.md || exit 43\n'
                'test ! -e log.json || exit 44\n'
                'printf "%s" "[]" > log.json\n',
                encoding="utf-8",
            )
            path.chmod(0o700)
            command = errors.Command((), files=(("input.md", b"Check the seal."),))
            first = errors.capture(path, command, oracle=True)
            second = errors.capture(path, command, oracle=True)
        self.assertEqual(first["exit"], 0)
        self.assertEqual(first["files"], {
            "log.json": {"kind": "file", "sha256": hashlib.sha256(b"[]").hexdigest()},
        })
        self.assertEqual(second, first)

    def test_unstable_oracle_result_checks_the_parts_that_agree(self):
        unit = errors.Unit("log/example", errors.Command(()))
        first = {"exit": 0, "stdout": "same", "stderr": "old", "files": {}}
        second = {**first, "stderr": "new"}
        result = errors.classify(unit, first, second, second)
        self.assertEqual(result["status"], "unstable")
        self.assertEqual(result["unstable_parts"], ["stderr"])
        self.assertTrue(result["unstable_parts_ok"])
        result = errors.classify(unit, first, second, {**second, "files": {"log.json": "new"}})
        self.assertFalse(result["unstable_parts_ok"])

    def test_missing_randoc_result_is_retried_and_counted_as_fail(self):
        args = type("Args", (), {"randoc": Path("missing"), "oracle": Path("oracle")})
        unit = errors.Unit("exit/6", errors.Command(()))
        oracle = {"exit": 6, "stdout": "same", "stderr": "same", "files": {}}
        with patch.object(errors, "attempt", side_effect=("Stopped once", "Stopped again")) as run:
            rows = errors.compare_units(args, [(unit, oracle, oracle)], [])
        self.assertEqual(run.call_count, 2)
        self.assertEqual(rows[0]["randoc"], "Stopped again")
        counts = self.parse(rows, selected="exit/6")
        self.assertEqual(counts.rows, {"errors.units": (0, 1)})
        self.assertEqual(counts.no_result, ["exit/6"])

    def test_full_report_requires_every_corpus_unit_and_pivot_target(self):
        rows = [{"id": f"log/corpus/{i}", "status": "pass"} for i in range(230)]
        rows += [{"id": f"log/pivot/{target}/{i}", "status": "pass"}
                 for target in errors.TARGETS for i in range(400)]
        codes = {str(code): {"reason": "No candidate matched."} for code in errors.CODES}
        self.assertEqual(self.parse(rows, codes=codes).rows, {"errors.units": (1430, 1430)})
        with self.assertRaisesRegex(measure.ToolError, "same 400 trees"):
            self.parse(rows[:-1], codes=codes)
        with self.assertRaisesRegex(measure.ToolError, "230 corpus logs"):
            self.parse(rows[1:], codes=codes)
        with self.assertRaisesRegex(measure.ToolError, "all 31 exit codes"):
            self.parse(rows, codes={})

    def test_filtered_report_keeps_oracle_exclusions_with_their_unit_ids(self):
        rows = [{"id": "limit", "status": "oracle_limit", "reason": "Time limit"},
                {"id": "unstable", "status": "unstable", "reason": "Log bytes differ"}]
        result = self.parse(rows, selected="log")
        self.assertEqual(result.rows, {"errors.units": (0, 0)})
        self.assertEqual(result.oracle_limit, ["limit"])
        self.assertEqual(result.unstable, ["unstable"])


if __name__ == "__main__":
    unittest.main()
