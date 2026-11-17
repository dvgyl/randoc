"""Tests of tools/autobuild/metrics.toml and its loader. Run: python3 -m unittest discover -s tools/autobuild/tests"""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import measure  # noqa: E402

ENTRY = """
[[metric]]
key = "{key}"
suite = "{suite}"
status = "{status}"
command = "{command}"
pass = "p"
total = "t"
"""


def registry(*entries: dict) -> dict:
    with tempfile.TemporaryDirectory() as folder:
        path = Path(folder) / "metrics.toml"
        path.write_text("".join(ENTRY.format(**e) for e in entries), encoding="utf-8")
        return measure.load_metrics(path)


def entry(key="harness.units", suite="harness", status="active", command="{harness} run"):
    return dict(key=key, suite=suite, status=status, command=command)


class CommittedRegistry(unittest.TestCase):
    def setUp(self):
        self.metrics = measure.load_metrics()

    def test_each_key_of_the_existing_tools_is_active(self):
        active = {k for k, m in self.metrics.items() if m.status == "active"}
        self.assertEqual(active, {
            "harness.units", "harness.read", "harness.write", "harness.e2e", "harness.cmd",
            "suite.pandoc", "suite.pandoc.command", "suite.pandoc.old", "suite.pandoc.readers",
            "corpus.units", "corpus.json", "corpus.html", "corpus.markdown",
            "caps.readers", "caps.writers", "caps.options",
        })

    def test_the_areas_with_no_measurement_are_to_build_and_the_held_out_keys_are_external(self):
        to_build = {
            "caps.extensions", "casegen.visible", "ast.roundtrip", "lua.api", "lua.filters",
            "custom.readers", "custom.writers", "templates.units", "citeproc.units", "defaults.units",
            "errors.units", "files.units", "zip.units", "pdf.units", "perf.startup", "perf.throughput",
            "perf.memory", "downstream.quarto", "downstream.rmarkdown",
        }
        for key in to_build:
            self.assertEqual(self.metrics[key].status, "to-build", key)
        for key in ("holdout.corpus", "holdout.generated"):
            self.assertEqual(self.metrics[key].status, "external", key)

    def test_each_active_suite_has_a_runner(self):
        self.assertEqual(measure.active_suites(self.metrics), sorted(measure.RUNNERS))


class Loader(unittest.TestCase):
    def test_a_valid_registry_loads(self):
        metrics = registry(entry(), entry(key="lua.api", suite="lua", status="to-build", command=""))
        self.assertEqual(metrics["lua.api"].status, "to-build")

    def test_each_defect_of_the_registry_is_refused(self):
        defects = {
            "listed two times": (entry(), entry()),
            "not one of": (entry(status="done"),),
            "no runner": (entry(suite="nothing"),),
            "different commands": (entry(), entry(key="harness.read", command="{harness} other")),
        }
        for message, entries in defects.items():
            with self.subTest(message=message), self.assertRaisesRegex(measure.ToolError, message):
                registry(*entries)


class QuickRuns(unittest.TestCase):
    def test_a_quick_run_measures_the_harness_and_the_suite_of_the_metric_of_the_item(self):
        from unittest import mock
        items = ("id\tmilestone\tstatus\tmetric\tbaseline\ttarget\ttitle\n"
                 "M1-02\tM1\tactive\tsuite.pandoc\t646/2231\t+2\tt\n"
                 "M1-04\tM1\tqueued\tharness.write\t-\t+5\tt\n"
                 "M1-12\tM1\tqueued\tlua.api\t-\t+1\tt\n")
        metrics = measure.load_metrics()
        with tempfile.TemporaryDirectory() as folder:
            (Path(folder) / "plan").mkdir()
            (Path(folder) / "plan" / "ITEMS.tsv").write_text(items, encoding="utf-8")
            with mock.patch.object(measure, "ROOT", Path(folder)):
                chosen = {item: measure.quick_suites(item, metrics) for item in ("M1-02", "M1-04", "M1-12", "-")}
        self.assertEqual(chosen, {"M1-02": ["harness", "pandoc"], "M1-04": ["harness"],
                                  "M1-12": ["harness"], "-": ["harness"]})


if __name__ == "__main__":
    unittest.main()
