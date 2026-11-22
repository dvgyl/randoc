"""Check extension qualification, comparison and report coverage."""

import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch


AUTOBUILD = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(AUTOBUILD), str(AUTOBUILD.parent / "extension-caps")]

import measure
from check import Input, Inputs, authored, inert, harness_inputs, read_json
from ext_capture import classify, signature
from extension_caps import Pair, inventory, make_runner
from observe import Observation
from probes import workbook


def observed(exit=0, stdout=b"flow", stderr=b""):
    return signature(Observation(exit, stdout, stderr, {}), "json")


class FakeCaptures:
    def __init__(self, oracle=None, randoc=None):
        self.oracle_overrides = oracle or {}
        self.randoc_overrides = randoc or {}
        self.actual = []

    def oracle(self, requests):
        return {request: ("stable", self.oracle_overrides.get((request.args[1], request.data), observed()))
                for request in requests}

    def randoc(self, requests, *, retry=False):
        results = {request: self.randoc_overrides.get((request.args[1], request.data), observed())
                   for request in requests}
        self.actual.extend(results)
        return results


class ExtensionCaps(unittest.TestCase):
    def setUp(self):
        self.pair = Pair("reader", "markdown", "footnotes")
        self.sample = Input("valve", b"valve")

    def authored(self, captures):
        return authored([self.pair], {self.pair: self.sample}, captures)

    def test_inventory_is_exact_and_includes_both_directions(self):
        pairs = inventory(measure.ROOT, measure.read_json)
        self.assertEqual(len(pairs), 1719)
        self.assertEqual(sum(pair.direction == "reader" for pair in pairs), 771)
        self.assertIn(Pair("reader", "docx", "styles"), pairs)
        self.assertIn(Pair("writer", "context", "tagging"), pairs)
        self.assertEqual(list(Pair("writer", "gfm", "alerts").commands()),
                         [("-f", "json", "-t", fmt) for fmt in ["gfm+alerts", "gfm-alerts", "gfm"]])

    def test_each_pair_has_a_targeted_input(self):
        root = measure.ROOT
        pairs = inventory(root, read_json)
        readers = read_json(root / "spec/inventory/input-formats.json")
        inputs = SimpleNamespace(harness=harness_inputs(root, readers), args=SimpleNamespace(oracle=None),
                                 writer_cache=None, writer_version=None, pool=None, rule=None)

        def render(fmt, trees, *_):
            return [Input(f"reader/{fmt}/pivot/{extension}", b"archive") for extension, _ in trees]

        with patch("check.pivot_inputs", side_effect=render):
            probes = Inputs.probes(inputs, pairs)
        self.assertEqual(probes.keys(), set(pairs))
        self.assertTrue(all(probe.data for probe in probes.values()))
        self.assertEqual(workbook(), workbook())

    def test_insensitive_authored_input_needs_an_inert_check(self):
        captures = FakeCaptures()
        self.assertEqual(self.authored(captures), {})
        self.assertEqual(captures.actual, [])

    def test_error_only_change_cannot_pass(self):
        captures = FakeCaptures({
            ("markdown+footnotes", b"valve"): observed(exit=3),
            ("markdown-footnotes", b"valve"): observed(exit=4),
            ("markdown", b"valve"): observed(exit=3),
        })
        self.assertEqual(self.authored(captures)[self.pair]["method"], "error_only")
        self.assertEqual(captures.actual, [])

    def test_sensitive_input_compares_all_three_settings(self):
        change = {("markdown-footnotes", b"valve"): observed(stdout=b"pressure")}
        captures = FakeCaptures(change, {**change, ("markdown", b"valve"): observed(stderr=b"warning")})
        row = self.authored(captures)[self.pair]
        self.assertEqual(row["method"], "sensitive")
        self.assertEqual(row["differences"], {"default": {"parts": ["stderr"]}})
        self.assertEqual(len(captures.actual), 3)
        self.assertEqual(self.authored(FakeCaptures(change, change))[self.pair]["status"], "pass")

    def test_inert_pass_checks_all_inputs_in_all_three_settings(self):
        captures = FakeCaptures()
        samples = [self.sample, Input("gauge", b"gauge")]
        row = inert([self.pair], iter(samples), captures)[self.pair]
        self.assertEqual(row, {"id": self.pair.id, "status": "pass", "method": "inert", "inputs_checked": 2})
        self.assertEqual({request.data for request in captures.actual}, {b"valve", b"gauge"})
        self.assertEqual(len(captures.actual), 6)
        change = {("markdown-footnotes", b"gauge"): observed(stdout=b"pressure")}
        row = inert([self.pair], iter(samples), FakeCaptures(change))[self.pair]
        self.assertEqual(row["method"], "uncovered")
        self.assertEqual(row["input"], "gauge")

    def test_missing_randoc_result_fails_a_pair(self):
        captures = FakeCaptures(randoc={("markdown", b"valve"): "process timed out"})
        row = inert([self.pair], iter([self.sample]), captures)[self.pair]
        self.assertEqual(row["status"], "no_result")
        self.assertEqual(row["differences"]["default"]["reason"], "process timed out")

    def test_json_order_is_a_byte_difference_for_this_metric(self):
        a = observed(stdout=b'{"flow":24,"pressure":36}')
        b = observed(stdout=b'{"pressure":36,"flow":24}')
        self.assertEqual(classify(a, b), ("unstable", ["stdout"]))

    def test_report_cannot_omit_or_duplicate_pairs(self):
        with tempfile.TemporaryDirectory() as folder:
            evidence = Path(folder)
            path = evidence / "extension-caps-report.json"
            parse = make_runner(measure).parse
            unit = {"id": self.pair.id, "method": "unconfirmed", "status": "fail"}
            report = {"units": [unit], "selected": self.pair.id, "inert": []}
            path.write_text(json.dumps(report), encoding="utf-8")
            self.assertEqual(parse(evidence, "").rows, {"caps.extensions": (0, 1)})
            for units in ([], [unit, unit]):
                path.write_text(json.dumps({**report, "units": units}), encoding="utf-8")
                with self.assertRaises(measure.ToolError):
                    parse(evidence, "")
