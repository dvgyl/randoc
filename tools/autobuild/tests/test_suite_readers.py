"""Tests of reader inputs and the reader measurement."""

import hashlib
import sys
import tempfile
import unittest
from contextlib import closing
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools/readers"))

import inputs
from corpuslib import Document
from observe import Observation
from reference import Reference, cache_key, open_cache, write_references


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
