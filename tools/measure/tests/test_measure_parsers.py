"""Tests of the suite parsers of tools/measure/measure.py. Run: python3 -m unittest discover -s tools/measure/tests"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import measure  # noqa: E402


def counts(parser, files: dict, log: str = "") -> measure.Counts:
    with tempfile.TemporaryDirectory() as folder:
        for name, value in files.items():
            (Path(folder) / name).write_text(json.dumps(value), encoding="utf-8")
        return parser(Path(folder), log)


class Harness(unittest.TestCase):
    UNITS = [
        {"id": "s/f/a#read", "status": "pass"},
        {"id": "s/f/a#write:html", "status": "fail"},
        {"id": "s/f/a#e2e:html", "status": "no_result"},
        {"id": "s/f/b#write:json", "status": "skipped"},
        {"id": "s/f/c#cmd", "status": "not_recorded"},
        {"id": "s/f/d#e2e:markdown", "status": "pass"},
    ]
    REPORT = {"summary": {"units": 6}, "units": UNITS}

    def test_no_input_units_are_not_in_the_total_and_no_result_units_fail(self):
        found = counts(measure.harness_counts, {"harness-report.json": self.REPORT})
        self.assertEqual(found.rows, {
            "harness.units": (2, 5), "harness.read": (1, 1), "harness.write": (0, 1),
            "harness.e2e": (1, 2), "harness.cmd": (0, 1),
        })
        self.assertEqual(found.failing, ["s/f/a#write:html", "s/f/a#e2e:html", "s/f/c#cmd"])
        self.assertEqual(found.no_result, ["s/f/a#e2e:html"])
        self.assertEqual(found.no_input, ["s/f/b#write:json"])
        self.assertEqual((found.unstable, found.oracle_limit), ([], []))

    def test_classes_that_do_not_add_up_to_the_report_are_a_tool_error(self):
        report = {"summary": {"units": 7}, "units": self.UNITS}
        with self.assertRaisesRegex(measure.ToolError, "the classes hold 6 units, but the report has 7"):
            counts(measure.harness_counts, {"harness-report.json": report})

    def test_capability_totals_come_from_the_inventory_and_not_from_the_matrix(self):
        inventory = measure.ROOT / "spec" / "inventory"
        readers = json.loads((inventory / "input-formats.json").read_text())
        options = json.loads((inventory / "options.json").read_text())
        matrix = {
            "input_formats": {readers[0]: {"status": "passing"}, readers[1]: {"status": "partial"}},
            "output_formats": {},
            "options": {options[0]["long"][0]: {"status": "passing"}, "not-an-option": {"status": "passing"}},
        }
        found = counts(measure.harness_counts, {"harness-report.json": self.REPORT, "capabilities.json": matrix})
        self.assertNotIn("caps.readers", found.rows)
        self.assertNotIn("caps.writers", found.rows)
        self.assertEqual(found.rows["caps.options"], (1, 109))


class Pandoc(unittest.TestCase):
    REPORT = {"totals": {"run": 6}, "passing": ["command/2.md#1", "old/a/b", "command/3.md#2"]}
    LOG = "\n".join([
        "FAIL command/1.md#1", "    timeout",
        "FAIL readers/docx/heap", "    no result: the oracle reached its heap limit (GHCRTS=-M3g)",
        "FAIL old/c", "    exit code 2",
        "total: 3/6 pass",
    ])

    def test_oracle_limit_tests_leave_the_total_and_randoc_timeouts_fail(self):
        found = counts(measure.pandoc_counts, {"pandoc-report.json": self.REPORT}, self.LOG)
        self.assertEqual(found.rows, {
            "suite.pandoc": (3, 5), "suite.pandoc.command": (2, 3),
            "suite.pandoc.old": (1, 2), "suite.pandoc.readers": (0, 0),
        })
        self.assertEqual(found.failing, ["command/1.md#1", "old/c"])
        self.assertEqual(found.no_result, ["command/1.md#1"])
        self.assertEqual(found.oracle_limit, ["readers/docx/heap"])

    def test_a_missing_failure_line_is_a_tool_error(self):
        with self.assertRaises(measure.ToolError):
            counts(measure.pandoc_counts, {"pandoc-report.json": self.REPORT}, self.LOG.replace("FAIL old/c", ""))


class Corpus(unittest.TestCase):
    def test_targets_classes_and_unit_lists(self):
        report = {
            "totals": {"units": 6},
            "results": {"a:json": "pass", "b:json": "pass", "a:html": "fail output",
                        "b:html": "error oracle", "a:markdown": "timeout", "b:markdown": "pass"},
        }
        found = counts(measure.corpus_counts, {"corpus-report.json": report})
        self.assertEqual(found.rows, {"corpus.units": (3, 5), "corpus.json": (2, 2),
                                      "corpus.html": (0, 1), "corpus.markdown": (1, 2)})
        self.assertEqual(found.failing, ["a:html", "a:markdown"])
        self.assertEqual(found.no_result, ["a:markdown"])
        self.assertEqual(found.oracle_limit, ["b:html"])


if __name__ == "__main__":
    unittest.main()
