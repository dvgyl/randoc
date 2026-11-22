"""Tests of tools/measure/metrics.toml and its loader. Run: python3 -m unittest discover -s tools/measure/tests"""

from __future__ import annotations

import string
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
    """The registry agrees with the parsers and the placeholders, whatever the number of active keys."""

    def setUp(self):
        self.metrics = measure.load_metrics()

    def active(self, suite: str) -> set[str]:
        return {k for k, m in self.metrics.items() if m.suite == suite and m.status == "active"}

    def test_each_row_key_of_a_built_in_parser_is_an_active_key_of_its_suite(self):
        emitted = {
            "harness": {"harness.units", *measure.HARNESS_KINDS.values(), *measure.CAPS},
            "pandoc": {"suite.pandoc", *measure.PANDOC_GROUPS.values()},
            "corpus": {"corpus.units", *(f"corpus.{t}" for t in measure.CORPUS_TARGETS)},
        }
        for suite, keys in emitted.items():
            with self.subTest(suite=suite):
                self.assertEqual(self.active(suite), keys)

    def test_each_active_command_uses_only_known_placeholders(self):
        known = {"python", "evidence", *measure.PROGRAMS, *measure.ENV_PLACEHOLDERS}
        for key, metric in self.metrics.items():
            if metric.status != "active":
                continue
            names = {name for _, name, _, _ in string.Formatter().parse(metric.command) if name}
            with self.subTest(key=key):
                self.assertTrue(metric.command.strip())
                self.assertLessEqual(names, known)

    def test_the_areas_of_the_spec_have_keys_and_the_held_out_keys_are_external(self):
        areas = {
            "caps.extensions", "casegen.visible", "ast.roundtrip", "lua.api", "lua.filters",
            "custom.readers", "custom.writers", "templates.units", "citeproc.units", "defaults.units",
            "errors.units", "files.units", "zip.units", "pdf.units", "perf.startup", "perf.throughput",
            "perf.memory", "downstream.quarto", "downstream.rmarkdown",
        }
        self.assertLessEqual(areas, set(self.metrics))
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
    def test_a_quick_run_measures_the_harness(self):
        metrics = measure.load_metrics()
        self.assertEqual(measure.quick_suites("-", metrics), ["harness"])


if __name__ == "__main__":
    unittest.main()
