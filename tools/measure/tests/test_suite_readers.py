"""Tests of reader inputs and the reader measurement."""

import hashlib
import json
import sys
import tempfile
import unittest
from contextlib import closing
from pathlib import Path
from unittest.mock import mock_open, patch

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools/readers"))
sys.path.insert(0, str(ROOT / "tools/measure"))

import inputs
import measure
import readers
import results
from corpuslib import Document
from observe import Observation
from reference import Reference, cache_key, open_cache, read_reference, write_references


class ReaderInputsTest(unittest.TestCase):
    def test_harness_inputs_deduplicate_content_per_base_format(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            cases = root / "conformance/cases"
            fixtures = root / "conformance/fixtures"
            cases.mkdir(parents=True)
            fixtures.mkdir()
            (fixtures / "a.txt").write_bytes(b"Silk")
            (cases / "formats.toml").write_text(
                'from = "markdown+smart"\n'
                '[[case]]\nid = "inline"\ninput = "Silk"\n'
                '[[case]]\nid = "fixture"\ninput_file = "a.txt"\n'
                '[[case]]\nid = "different"\nfrom = "org"\ninput = "Silk"\n'
                '[[case]]\nid = "command"\ncmd = ["-f", "markdown", "-t", "json"]\ninput = "Wool"\n',
                encoding="utf-8")
            groups = inputs.harness_inputs(root, ["markdown", "org"])
            self.assertEqual([unit.data for unit in groups["markdown"]], [b"Silk"])
            self.assertEqual([unit.data for unit in groups["org"]], [b"Silk"])
            self.assertNotEqual(groups["markdown"][0].id, groups["org"][0].id)

    def test_corpus_input_checks_size_and_hash_before_use(self):
        data = b"One document"
        digest = hashlib.sha256(data).hexdigest()
        document = Document("test-book", "xlsx", "https://example.org/test.xlsx",
                            "Apache-2.0", "Example", digest, len(data))
        with tempfile.TemporaryDirectory() as folder:
            directory = Path(folder)
            with patch.object(inputs, "download", return_value=data) as download:
                self.assertEqual(inputs.document_data(document, directory, fetch=True), data)
                self.assertEqual(inputs.document_data(document, directory), data)
                download.assert_called_once_with(document.url)
            (directory / document.file_name).write_bytes(b"Bad document")
            with self.assertRaisesRegex(ValueError, "pinned size or SHA-256"):
                inputs.document_data(document, directory)

    def test_corpus_input_reads_at_most_one_byte_past_the_pinned_size(self):
        document = Document("test-book", "xlsx", "https://example.org/test.xlsx",
                            "Apache-2.0", "Example", hashlib.sha256(b"a").hexdigest(), 1)
        opener = mock_open(read_data=b"ab")
        with patch.object(Path, "is_file", return_value=True), patch.object(Path, "open", opener):
            with self.assertRaisesRegex(ValueError, "pinned size or SHA-256"):
                inputs.document_data(document, Path("corpus"))
        opener().read.assert_called_once_with(2)

    def test_pivot_uses_pinned_raw_oracle_output_only_after_exit_zero(self):
        trees = [("forms/heading", b"first tree"), ("forms/line", b"second tree")]
        version = b"oracle version"
        args = ["-f", "json", "-t", "html"]
        with tempfile.TemporaryDirectory() as folder, \
                closing(open_cache(Path(folder) / "writers.sqlite")) as database:
            write_references(database, [
                (cache_key(version, args, trees[0][1]),
                 Reference("stable", Observation(0, b"<h1>Title</h1>\n", b"", {}))),
                (cache_key(version, args, trees[1][1]),
                 Reference("stable", Observation(1, b"error", b"", {}))),
            ])
            units = list(inputs.pivot_inputs("html", trees, database, version,
                                            None, None, None, None))
            self.assertEqual(units, [inputs.Input("reader/html/pivot/forms/heading", b"<h1>Title</h1>\n")])


class ReaderResultsTest(unittest.TestCase):
    def test_json_value_comparison_checks_numbers_types_and_keys(self):
        self.assertTrue(results.json_values_match(b'{"a":2,"b":[1.5]}', b'{"b":[1.50], "a":2}'))
        for left, right in (
            (b"1", b"1.0"), (b"1", b"true"), (b"1e1000", b"2e1000"),
            (b'{"a":1,"a":1}', b'{"a":1,"a":1}'), (b"NaN", b"NaN"),
            (b"[0,1]", b"[1,0]"), (b"invalid", b"invalid"),
        ):
            with self.subTest(left=left, right=right):
                self.assertFalse(results.json_values_match(left, right))

    def test_successful_result_checks_files_and_records_value_only(self):
        expected = Observation(0, b'{"a":1}', b"", {})
        actual = Observation(0, b'{ "a":1 }', b"", {})
        reference = results.oracle_reference(expected, actual)
        row = results.verdict("reader/json/a", "json", reference, actual)
        self.assertEqual(row["status"], "pass")
        self.assertTrue(row["value_only"])
        changed = Observation(0, actual.stdout, b"warning", {"out": "changed"})
        self.assertEqual(results.differences(expected, changed), ["stderr", "files"])

    def test_oracle_error_remains_a_comparable_result(self):
        expected = Observation(64, b"", b"invalid input", {})
        reference = results.oracle_reference(expected, expected)
        self.assertEqual(reference.status, "stable")
        self.assertEqual(results.verdict("case", "html", reference, expected)["status"], "pass")
        self.assertEqual(results.verdict("case", "html", reference, "no result")["status"], "no_result")

    def test_recheck_limit_preserves_original_oracle_result_and_differences(self):
        first = Observation(0, b"[]", b"first", {})
        second = Observation(0, b"[]", b"second", {})
        saved = results.oracle_reference(first, second)
        limited = results.oracle_reference(first, "limit", saved)
        self.assertEqual(limited.status, "oracle_limit")
        self.assertEqual(limited.first, first)
        self.assertEqual(limited.unstable_parts, ("stderr",))
        checked = results.oracle_reference(first, first, limited)
        self.assertEqual(checked.status, "unstable")
        changed = Observation(0, b"[1]", b"second", {})
        self.assertFalse(results.verdict("case", "html", checked, changed)["unstable_parts_ok"])

    def test_recheck_preserves_a_difference_recorded_during_the_oracle_call(self):
        key = b"reader"
        first = Observation(0, b"[]", b"", {})
        changed = Observation(0, b"[1]", b"", {})
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "readers.sqlite"
            with closing(open_cache(path)) as database, closing(open_cache(path)) as other:
                write_references(database, [(key, results.oracle_reference(first, first))])

                def capture(_unit):
                    write_references(other, [(key, results.oracle_reference(first, changed))], refresh=True)
                    return first

                pool = type("Pool", (), {"map": staticmethod(map)})()
                readers.recorded_references(database, {key: None}, capture, pool, True)
                stored = read_reference(database, key)
                self.assertEqual((stored.status, stored.unstable_parts), ("unstable", ("stdout",)))

    def test_a_timeout_keeps_a_difference_from_the_other_observation(self):
        first = Observation(0, b"[]", b"", {})
        changed = Observation(0, b"[1]", b"", {})
        saved = results.oracle_reference(first, first)
        limited = results.oracle_reference(first, "timeout", saved)
        checked = results.oracle_reference("timeout", changed, limited)
        self.assertEqual((checked.status, checked.unstable_parts), ("oracle_limit", ("stdout",)))
        recovered = results.oracle_reference(first, first, checked)
        self.assertEqual(results.verdict("case", "html", recovered, first)["status"], "unstable")

    def test_a_new_pair_keeps_its_completed_observation_after_a_timeout(self):
        first = Observation(0, b"[]", b"", {})
        changed = Observation(0, b"[1]", b"", {})
        for pair in ((first, "timeout"), ("timeout", first)):
            with self.subTest(pair=pair):
                limited = results.oracle_reference(*pair)
                self.assertEqual((limited.status, limited.first), ("oracle_limit", first))
                checked = results.oracle_reference(changed, changed, limited)
                self.assertEqual((checked.status, checked.unstable_parts), ("unstable", ("stdout",)))


class ReaderSuiteTest(unittest.TestCase):
    def parse(self, rows, *, selected=None, known=(), formats=("html", "markdown")):
        measure.load_metrics()
        read = measure.read_json

        def read_report(path):
            return {"units": known} if path.name == "unstable.json" else read(path)

        counts = {fmt: {"pivot": 0, "harness": 0, "corpus": 0} for fmt in formats}
        for row in rows:
            kind = row["id"].split("/")[2]
            counts[row["format"]][kind] += 1
        report = {"units": rows, "input_counts": counts,
                  "summary": {"units": len(rows), "selected": selected}}
        matrix = {"input_formats": {
            "html": {"read": {"passing": 1, "total": 1}},
            "markdown": {"read": {"passing": 1, "total": 1},
                         "end_to_end": {"passing": 1, "total": 2}},
        }}
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(measure, "inventory_names", return_value=formats), \
                patch.object(measure, "read_json", side_effect=read_report):
            folder = Path(directory)
            (folder / "readers-report.json").write_text(json.dumps(report), encoding="utf-8")
            (folder / "capabilities.json").write_text(json.dumps(matrix), encoding="utf-8")
            return measure.RUNNERS["readers"].parse(folder, "")

    def test_capabilities_need_all_reader_and_harness_results(self):
        rows = [{"id": f"reader/{fmt}/pivot/forms/title", "format": fmt, "status": "pass"}
                for fmt in ("html", "markdown")]
        parsed = self.parse(rows)
        self.assertEqual(parsed.rows, {"fmt.read": (2, 2), "fmt.read.html": (1, 1),
                                      "fmt.read.markdown": (1, 1), "caps.readers": (1, 2)})
        self.assertEqual(parsed.failing_by_metric["caps.readers"], ["capability/reader/markdown"])

    def test_a_full_report_requires_each_exclusion_in_the_register(self):
        rows = [{"id": "reader/html/pivot/forms/title", "format": "html", "status": "oracle_limit"}]
        with self.assertRaisesRegex(measure.ToolError, "record the oracle class"):
            self.parse(rows, formats=("html",))
        known = [{"suite": "readers", "id": rows[0]["id"], "status": "oracle_limit",
                  "reason": "limit", "evidence": "reader report"}]
        parsed = self.parse(rows, formats=("html",), known=known)
        self.assertEqual(parsed.oracle_limit, [rows[0]["id"]])
        self.assertEqual(parsed.rows["caps.readers"], (0, 1))

    def test_a_filtered_report_counts_a_randoc_no_result_as_failed(self):
        rows = [{"id": "reader/html/pivot/forms/title", "format": "html", "status": "no_result"}]
        parsed = self.parse(rows, selected="html")
        self.assertEqual(parsed.rows, {"fmt.read": (0, 1), "fmt.read.html": (0, 1)})
        self.assertEqual(parsed.no_result, [rows[0]["id"]])

    def test_a_full_report_requires_ten_xlsx_documents(self):
        rows = [{"id": "reader/xlsx/corpus/workbook", "format": "xlsx", "status": "pass"}]
        with self.assertRaisesRegex(measure.ToolError, "xlsx needs 10 or more"):
            self.parse(rows, formats=("xlsx",))
