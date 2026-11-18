import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import measure


def unit(fmt, index, status="pass"):
    return {"id": f"writer/{fmt}/{index}", "format": fmt, "status": status}


class WriterSuiteTests(unittest.TestCase):
    def parse(self, units, *, selection=None, pdf=None):
        measure.load_metrics()
        with tempfile.TemporaryDirectory() as folder, \
             patch.object(measure, "inventory_names", return_value=["html", "plain", "pdf"]):
            path = Path(folder)
            report = {"units": units, "summary": {"units": len(units), "selected": selection}}
            (path / "writers-report.json").write_text(json.dumps(report), encoding="utf-8")
            matrix = {"output_formats": {"html": {"write": {"passing": 1, "total": 1}},
                                         "plain": {"end_to_end": {"passing": 1, "total": 2}}}}
            (path / "capabilities.json").write_text(json.dumps(matrix), encoding="utf-8")
            if pdf:
                (path / "pdf.json").write_text(json.dumps({"rows": [pdf]}), encoding="utf-8")
            return measure.RUNNERS["writers"].parse(path, "")

    def test_capabilities_need_the_pivot_and_harness_units(self):
        rows = [unit(fmt, index) for fmt in ("html", "plain") for index in range(400)]
        rows[0]["status"] = "unstable"
        result = self.parse(rows, pdf={"metric": "pdf.units", "pass": 1, "total": 1})
        self.assertEqual(result.rows, {"fmt.write": (799, 799), "fmt.write.html": (399, 399),
                                      "fmt.write.plain": (400, 400), "caps.writers": (2, 3)})
        self.assertEqual(result.unstable, ["writer/html/0"])
        self.assertEqual(result.failing_by_metric["caps.writers"], ["capability/writer/plain"])

    def test_a_full_report_needs_every_tree(self):
        with self.assertRaisesRegex(measure.ToolError, "needs 400 trees per writer"):
            self.parse([unit("html", 0)])

    def test_a_filtered_report_counts_no_result_but_excludes_oracle_limit(self):
        result = self.parse([unit("html", 0, "no_result"), unit("html", 1, "oracle_limit")],
                            selection="html")
        self.assertEqual(result.rows, {"fmt.write": (0, 1), "fmt.write.html": (0, 1)})
        self.assertEqual(result.no_result, ["writer/html/0"])
        self.assertEqual(result.oracle_limit, ["writer/html/1"])
