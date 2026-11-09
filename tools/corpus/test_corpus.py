"""Unit tests for the corpus tools. Run: python3 -m unittest discover -s tools/corpus"""

import contextlib
import io
import json
import shutil
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import corpuslib
import fetch
import run

HASH_A = corpuslib.sha256_of(b"alpha")


def document(name="doc", fmt="markdown", data=b"alpha", **changes):
    fields = dict(name=name, format=fmt, url=f"https://example.org/{name}", license="MIT",
                  source="example", sha256=corpuslib.sha256_of(data), size=len(data))
    fields.update(changes)
    return corpuslib.Document(**fields)


class ManifestTests(unittest.TestCase):
    def test_format_and_parse_give_the_same_documents_and_keep_the_header(self):
        docs = [document("a-1"), document("b", "docx", b"zip", license='A "quoted" \\ name')]
        text = corpuslib.format_manifest(docs, "# head\n# two")
        self.assertTrue(text.startswith("# head\n# two\n\n[[document]]\n"))
        self.assertEqual(corpuslib.parse_manifest(text), docs)
        self.assertEqual(corpuslib.manifest_header(text), "# head\n# two")

    def test_file_name_uses_the_extension_of_the_format(self):
        self.assertEqual(document("x", "latex").file_name, "x.tex")
        self.assertEqual(document("x", "mdoc").file_name, "x.1")

    def test_each_rule_rejects_a_bad_document(self):
        bad = {
            "lowercase words": [document("Bad_Name")],
            "not unique": [document("a"), document("a")],
            "unknown format": [document("a", "nosuch")],
            "https": [document("a", url="http://example.org/a")],
            "must not be empty": [document("a", license=" ")],
            "both be set": [document("a", sha256="", size=0)],
            "more than 2000000": [document("a", size=2_000_001)],
            "more than 300000000": [document(f"d{i}", size=2_000_000) for i in range(151)],
        }
        for message, docs in bad.items():
            with self.subTest(message), self.assertRaisesRegex(corpuslib.ManifestError, message):
                corpuslib.check(docs)

    def test_an_unpinned_document_is_valid_only_when_hashes_are_not_required(self):
        unpinned = [document("a", sha256="", size=0)]
        corpuslib.check(unpinned, require_hashes=False)
        with self.assertRaisesRegex(corpuslib.ManifestError, "both be set"):
            corpuslib.check([document("a", sha256="", size=5)], require_hashes=False)

    def test_missing_unknown_fields_and_bad_toml_are_errors(self):
        with self.assertRaisesRegex(corpuslib.ManifestError, "missing size"):
            corpuslib.parse_manifest('[[document]]\nname="a"\nformat="org"\nurl="https://x"\n'
                                     'license="MIT"\nsource="s"\nsha256=""\n')
        text = corpuslib.format_manifest([document("a")]) + 'extra = "x"\n'
        with self.assertRaisesRegex(corpuslib.ManifestError, "unknown field extra"):
            corpuslib.parse_manifest(text)
        with self.assertRaisesRegex(corpuslib.ManifestError, "manifest:"):
            corpuslib.parse_manifest("[[document]\n")

    def test_the_committed_manifest_is_valid_and_in_the_limits(self):
        docs = corpuslib.load_manifest()
        self.assertGreaterEqual(len(docs), 150)

    def test_the_tables_in_the_corpus_documentation_match_the_manifest(self):
        docs = corpuslib.load_manifest()
        text = (Path(__file__).parents[2] / "docs" / "CORPUS.md").read_text(encoding="utf-8")
        self.assertIn(corpuslib.source_table(docs), text)
        self.assertIn(corpuslib.count_table(docs), text)

    def test_tables_group_documents_by_source_and_format(self):
        docs = [document("a", source="x/y@123"), document("b", "org", data=b"abc", source="x/y@456",
                                                           license="ISC")]
        self.assertIn("| x/y | ISC; MIT | markdown, org | 2 |", corpuslib.source_table(docs))
        self.assertTrue(corpuslib.count_table(docs).endswith("| org | 1 | 3 |\n| all | 2 | 8 |"))


class FetchTests(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="randoc-corpus-test-"))
        self.calls = []

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def get(self, data):
        def fake(url):
            self.calls.append(url)
            return data
        return fake

    def test_downloads_a_missing_document_and_then_skips_it(self):
        doc = document("a")
        self.assertEqual(fetch.fetch_one(doc, self.dir, self.get(b"alpha"))[0], "downloaded")
        self.assertEqual((self.dir / "a.md").read_bytes(), b"alpha")
        self.assertEqual(fetch.fetch_one(doc, self.dir, self.get(b"alpha"))[0], "present")
        self.assertEqual(len(self.calls), 1)
        self.assertEqual([p.name for p in self.dir.iterdir() if not p.name.startswith("._")], ["a.md"])

    def test_a_file_with_a_wrong_hash_is_downloaded_again(self):
        (self.dir / "a.md").write_bytes(b"old")
        self.assertEqual(fetch.fetch_one(document("a"), self.dir, self.get(b"alpha"))[0], "downloaded")
        self.assertEqual((self.dir / "a.md").read_bytes(), b"alpha")

    def test_a_hash_mismatch_writes_no_file(self):
        result, _ = fetch.fetch_one(document("a"), self.dir, self.get(b"changed"))
        self.assertEqual(result, f"hash mismatch: got {corpuslib.sha256_of(b'changed')}")
        self.assertFalse((self.dir / "a.md").exists())

    def test_pin_fills_the_hash_and_the_size(self):
        doc = document("a", sha256="", size=0)
        self.assertEqual(fetch.fetch_one(doc, self.dir, self.get(b"alpha"))[0], "unpinned: run with --pin")
        result, pinned = fetch.fetch_one(doc, self.dir, self.get(b"alpha"), pin=True)
        self.assertEqual((result, pinned.sha256, pinned.size), ("downloaded", HASH_A, 5))

    def test_download_rejects_a_document_that_is_too_large(self):
        source = self.dir / "big"
        source.write_bytes(b"x" * 11)
        with mock.patch.object(corpuslib, "MAX_DOCUMENT_BYTES", 10):
            with self.assertRaisesRegex(ValueError, "more than 10 bytes"):
                fetch.download(source.as_uri())
            source.write_bytes(b"x" * 10)
            self.assertEqual(fetch.download(source.as_uri()), b"x" * 10)

    def test_main_pins_the_manifest_and_reports_failures(self):
        manifest = self.dir / "manifest.toml"
        docs = [document("a", sha256="", size=0), document("b", sha256="", size=0)]
        manifest.write_text(corpuslib.format_manifest(docs, "# header"))

        def get(url):
            if url.endswith("/b"):
                raise OSError("unreachable")
            return b"alpha"
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = fetch.main(["--manifest", str(manifest), "--corpus", str(self.dir), "--pin"], get, 0)
        self.assertEqual(code, 1)
        self.assertIn("b: error: unreachable", out.getvalue())
        pinned = corpuslib.parse_manifest(manifest.read_text(), require_hashes=False)
        self.assertEqual([(d.sha256, d.size) for d in pinned], [(HASH_A, 5), ("", 0)])
        self.assertTrue(manifest.read_text().startswith("# header\n"))

    def test_main_without_pin_needs_hashes_and_honours_only(self):
        manifest = self.dir / "manifest.toml"
        manifest.write_text(corpuslib.format_manifest([document("a", sha256="", size=0)]))
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(fetch.main(["--manifest", str(manifest), "--corpus", str(self.dir)]), 2)
        manifest.write_text(corpuslib.format_manifest([document("a"), document("b")]))
        with contextlib.redirect_stdout(io.StringIO()):
            code = fetch.main(["--manifest", str(manifest), "--corpus", str(self.dir), "--only", "a"],
                              self.get(b"alpha"), 0)
        self.assertEqual((code, self.calls), (0, ["https://example.org/a"]))


def outcome(exit=0, stdout=b"", stderr=b"", timed_out=False):
    return run.Outcome(exit, stdout, stderr, timed_out)


class CompareTests(unittest.TestCase):
    def test_json_output_compares_as_a_tree_and_text_output_as_bytes(self):
        a, b = outcome(stdout=b'{"x": [1, 2]}'), outcome(stdout=b'{"x":[1,2]}')
        self.assertEqual(run.status(a, b, "json"), "pass")
        self.assertEqual(run.status(a, b, "html"), "fail output")
        self.assertEqual(run.status(a, outcome(stdout=b"not json"), "json"), "fail output")

    def test_exit_code_and_stderr_are_part_of_the_status(self):
        expected = outcome(0, b"o", b"warn")
        self.assertEqual(run.status(expected, outcome(1, b"o", b"warn"), "html"), "fail exit")
        self.assertEqual(run.status(expected, outcome(2, b"x", b""), "html"), "fail exit,stderr,output")

    def test_timeouts_have_their_own_status(self):
        self.assertEqual(run.status(outcome(), outcome(timed_out=True), "html"), "timeout")
        self.assertEqual(run.status(outcome(timed_out=True), outcome(), "html"), "oracle timeout")

    def test_summary_counts_units_by_target_and_format(self):
        docs = [document("b", "org"), document("a", "rst")]
        units = [(docs[0], "html", "pass"), (docs[0], "json", "fail output"), (docs[1], "html", "pass")]
        report = run.summarize(units, "prog 1", "pandoc 3.13", ("json", "html"), 9)
        self.assertEqual(report["totals"], {"units": 3, "pass": 2, "fail": 1})
        self.assertEqual(report["by_target"]["json"], {"units": 1, "pass": 0, "fail": 1})
        self.assertEqual(report["by_format"]["org"], {"units": 2, "pass": 1, "fail": 1})
        self.assertEqual(list(report["results"]), ["a:html", "b:html", "b:json"])
        self.assertEqual(report["documents"], 2)


class ExecuteTests(unittest.TestCase):
    def test_the_program_runs_as_pandoc_in_a_fixed_environment(self):
        with tempfile.TemporaryDirectory() as home:
            script = 'echo "$0 $SOURCE_DATE_EPOCH $TZ $LC_ALL $HOME"; echo e >&2; exit 3'
            result = run.execute("/bin/sh", ["-c", script], home, home, 10)
        self.assertEqual(result.stdout.decode(), f"pandoc 1700000000 UTC en_US.UTF-8 {home}\n")
        self.assertEqual((result.exit, result.stderr, result.timed_out), (3, b"e\n", False))

    def test_a_slow_program_is_stopped(self):
        with tempfile.TemporaryDirectory() as home:
            result = run.execute("/bin/sh", ["-c", "sleep 5"], home, home, 1)
        self.assertEqual((result.exit, result.timed_out), (-1, True))
        self.assertEqual(result.stderr, b"[corpus] no result after 1 s\n")

    def test_a_missing_program_is_a_failed_outcome(self):
        with tempfile.TemporaryDirectory() as home:
            result = run.execute(Path(home) / "none", [], home, home, 1)
        self.assertEqual(result.exit, -1)
        self.assertIn(b"[corpus]", result.stderr)


FAKE = """#!/bin/sh
if [ "$1" = --version ]; then echo "{name} 1.0"; exit 0; fi
echo "$5 $4" >> "$(dirname "$0")/{name}.log"
if [ "$4" = html ] && [ -n "{html_extra}" ]; then printf '{html_extra}'; fi
cat "$5"
"""


class RunTests(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="randoc-corpus-run-"))
        self.corpus = self.dir / "corpus"
        self.corpus.mkdir()
        docs = [document("one", data=b'{"a": 1}'), document("two", "org", data=b"[1]")]
        (self.corpus / "one.md").write_bytes(b'{"a": 1}')
        (self.corpus / "two.org").write_bytes(b"[1]")
        self.manifest = self.dir / "manifest.toml"
        self.manifest.write_text(corpuslib.format_manifest(docs))

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def program(self, name, html_extra=""):
        path = self.dir / name
        path.write_text(FAKE.format(name=name, html_extra=html_extra))
        path.chmod(0o755)
        return path

    def main(self, program, oracle, *extra):
        out = self.dir / "report.json"
        argv = ["--pandoc", str(program), "--oracle", str(oracle), "--out", str(out),
                "--manifest", str(self.manifest), "--corpus", str(self.corpus), "--jobs", "2", *extra]
        with contextlib.redirect_stdout(io.StringIO()):
            code = run.main(argv)
        return code, json.loads(out.read_text())

    def test_the_oracle_against_itself_passes_and_fills_the_cache(self):
        oracle = self.program("pandoc")
        code, report = self.main(oracle, oracle, "--expect-all-pass")
        self.assertEqual(code, 0)
        self.assertEqual(report["totals"], {"units": 6, "pass": 6, "fail": 0})
        self.assertEqual(report["program"], "pandoc 1.0")
        rows = sqlite3.connect(self.corpus / run.CACHE_NAME).execute("SELECT COUNT(*) FROM result")
        self.assertEqual(rows.fetchone()[0], 6)

    def test_a_second_run_uses_the_cached_oracle_results(self):
        oracle = self.program("pandoc")
        self.main(oracle, oracle)
        log = self.dir / "pandoc.log"
        self.assertEqual(len(log.read_text().splitlines()), 12)
        _, report = self.main(self.program("other"), oracle)
        self.assertEqual(report["totals"]["fail"], 0)
        self.assertEqual(len(log.read_text().splitlines()), 12)

    def test_a_different_program_fails_where_its_output_differs(self):
        code, report = self.main(self.program("randoc", "<x>"), self.program("pandoc"), "--expect-all-pass")
        self.assertEqual(code, 1)
        self.assertEqual(report["results"]["one:html"], "fail output")
        self.assertEqual(report["results"]["one:json"], "pass")
        self.assertEqual(report["by_target"]["html"], {"units": 2, "pass": 0, "fail": 2})

    def test_target_and_only_select_units_and_missing_documents_stop_the_run(self):
        oracle = self.program("pandoc")
        _, report = self.main(oracle, oracle, "--target", "json", "--only", "two")
        self.assertEqual(list(report["results"]), ["two:json"])
        (self.corpus / "two.org").unlink()
        with contextlib.redirect_stderr(io.StringIO()) as err, contextlib.redirect_stdout(io.StringIO()):
            code = run.main(["--pandoc", str(oracle), "--oracle", str(oracle), "--manifest",
                             str(self.manifest), "--corpus", str(self.corpus)])
        self.assertEqual(code, 2)
        self.assertIn("two", err.getvalue())


if __name__ == "__main__":
    unittest.main()
