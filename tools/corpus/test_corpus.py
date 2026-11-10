"""Unit tests for the corpus tools. Run: python3 -m unittest discover -s tools/corpus"""

import contextlib
import http.client
import io
import json
import os
import shutil
import socket
import sqlite3
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request
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
        with contextlib.closing(sqlite3.connect(self.corpus / run.CACHE_NAME)) as connection:
            rows = connection.execute("SELECT COUNT(*) FROM result").fetchone()[0]
        self.assertEqual(rows, 6)

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


class ReviewCorrectionTests(unittest.TestCase):
    """Tests for the findings of the first review (COR-01 to COR-10)."""

    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="randoc-corpus-review-"))

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def test_cor01_entries_use_the_rights_statement_inside_the_file(self):
        docs = {d.name: d for d in corpuslib.load_manifest()}
        removed = {"tika-testepub-xml-ext-epub", "tika-test-fb2", "tika-testword-3imgs-docx", "tika-testrtfjapanese-rtf",
                   "tika-testrtfword2010czechcharacters-rtf"}
        self.assertFalse(removed & set(docs))
        ebooks = [d for d in docs.values() if d.name.startswith("se-")]
        ebooks.append(docs["tika-testepub-multi-metadata-vals-epub"])
        self.assertEqual(len(ebooks), 6)
        for d in ebooks:
            self.assertEqual(d.license, "LicenseRef-Public-Domain-US AND CC0-1.0", d.name)

    def test_cor08_peps_record_the_declaration_of_each_file(self):
        docs = {d.name: d.license for d in corpuslib.load_manifest() if d.name.startswith("pep-")}
        for name in ("pep-0008", "pep-0020", "pep-0257", "pep-0287", "pep-0484", "pep-0572"):
            self.assertEqual(docs[name], "LicenseRef-Public-Domain", name)
        self.assertEqual(docs["pep-0001"], "LicenseRef-Public-Domain OR CC0-1.0")

    def test_cor02_the_fingerprint_is_the_hash_of_the_program_file(self):
        first, second = self.dir / "a", self.dir / "b"
        first.write_bytes(b"program one")
        second.write_bytes(b"program two")
        self.assertNotEqual(run.fingerprint(first), run.fingerprint(second))
        self.assertEqual(run.fingerprint(first), corpuslib.sha256_of(b"program one"))
        self.assertEqual(run.HOME, Path("/var/empty"))

    def test_cor03_a_changed_document_file_stops_the_run(self):
        corpus = self.dir / "corpus"
        corpus.mkdir()
        doc = document("one")
        manifest = self.dir / "manifest.toml"
        manifest.write_text(corpuslib.format_manifest([doc]))
        (corpus / "one.md").write_bytes(b"alphb")
        self.assertEqual(run.verify_documents([doc], corpus), ["one"])
        (corpus / "one.md").write_bytes(b"alpha")
        self.assertEqual(run.verify_documents([doc], corpus), [])
        self.assertEqual(run.verify_documents([document("one", size=4)], corpus), ["one"])
        (corpus / "one.md").write_bytes(b"changed")
        with contextlib.redirect_stderr(io.StringIO()) as err:
            code = run.main(["--pandoc", "/usr/bin/true", "--oracle", "/usr/bin/true",
                             "--manifest", str(manifest), "--corpus", str(corpus)])
        self.assertEqual(code, 2)
        self.assertIn("changed documents", err.getvalue())

    def test_cor04_json_comparison_keeps_types_apart(self):
        self.assertEqual(run.status(outcome(stdout=b'{"x":true}'), outcome(stdout=b'{"x":1}'), "json"),
                         "fail output")
        self.assertEqual(run.status(outcome(stdout=b"[1]"), outcome(stdout=b"[1.0]"), "json"), "fail output")
        self.assertEqual(run.status(outcome(stdout=b'{"a":[1,{"b":null}]}'),
                                    outcome(stdout=b'{ "a": [1, {"b": null}] }'), "json"), "pass")
        self.assertFalse(run.json_equal({"a": 1}, {"b": 1}))
        self.assertFalse(run.json_equal([1], [1, 2]))

    def test_cor05_the_time_limit_stops_child_processes(self):
        start = time.monotonic()
        result = run.execute("/bin/sh", ["-c", "sleep 3; :"], self.dir, self.dir, 0.2)
        self.assertLess(time.monotonic() - start, 2)
        self.assertTrue(result.timed_out)

    def test_cor05_pipe_reads_are_limited(self):
        result = run.execute("/bin/sh", ["-c", "printf 0123456789abcdef"], self.dir, self.dir, 10, limit=10)
        self.assertEqual((result.exit, result.stdout), (-1, b"0123456789"))
        self.assertIn(b"output larger than 10 bytes", result.stderr)

    def test_cor06_deep_json_and_unit_errors_become_failed_units(self):
        depth = 100000
        expected = outcome(stdout=b"[" * depth + b"0" + b"]" * depth)
        actual = outcome(stdout=b"[" * depth + b"1" + b"]" * depth)
        self.assertEqual(run.status(expected, actual, "json"), "fail output")
        cache = run.OracleCache(":memory:", "f")
        with mock.patch.object(run, "execute", side_effect=RuntimeError("boom")):
            units = run.run_units([document("a")], ("json",), "p", "o", self.dir, cache, 1, 1, self.dir)
        cache.close()
        self.assertEqual([u[2] for u in units], ["error RuntimeError: boom"])

    def test_cor07_fetch_checks_the_size_and_the_total_limit(self):
        status, _ = fetch.fetch_one(document("a", size=1), self.dir, lambda url: b"alpha")
        self.assertEqual(status, "size mismatch: got 5 bytes")
        self.assertFalse((self.dir / "a.md").exists())
        manifest = self.dir / "manifest.toml"
        docs = [document(name, sha256="", size=0) for name in ("a", "b", "c")]
        manifest.write_text(corpuslib.format_manifest(docs))
        with mock.patch.object(corpuslib, "MAX_TOTAL_BYTES", 5), contextlib.redirect_stdout(io.StringIO()) as out:
            code = fetch.main(["--manifest", str(manifest), "--corpus", str(self.dir), "--pin"],
                              lambda url: b"abc", 0)
        self.assertEqual(code, 1)
        self.assertIn("b: over the total limit: 3 bytes, 2 left", out.getvalue())
        pinned = corpuslib.parse_manifest(manifest.read_text(), require_hashes=False)
        self.assertEqual(sum(d.size for d in pinned), 3)

    def test_cor09_the_documentation_separates_the_rejection_causes(self):
        text = (Path(__file__).parents[2] / "docs" / "CORPUS.md").read_text(encoding="utf-8")
        self.assertIn("Pandoc 3.13 rejects 15 documents", text)
        self.assertIn("code 64 (a parse error) for 13", text)

    def test_cor10_a_redirect_to_http_is_refused(self):
        handler = fetch.HttpsOnlyRedirects()
        with self.assertRaises(urllib.error.HTTPError) as refused:
            handler.redirect_request(urllib.request.Request("https://example.org/a"), None, 302,
                                     "Found", {}, "http://example.org/a")
        refused.exception.close()
        allowed = handler.redirect_request(urllib.request.Request("https://example.org/a"), None, 302,
                                           "Found", {}, "https://example.org/b")
        self.assertEqual(allowed.full_url, "https://example.org/b")


class SecondReviewTests(unittest.TestCase):
    """Tests for the findings of the second review (C-01 to C-10)."""

    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="randoc-corpus-review2-"))
        self.corpus = self.dir / "corpus"
        self.corpus.mkdir()
        self.docs = [document("one", data=b'{"a": 1}'), document("two", "org", data=b"[1]")]
        (self.corpus / "one.md").write_bytes(b'{"a": 1}')
        (self.corpus / "two.org").write_bytes(b"[1]")
        self.manifest = self.dir / "manifest.toml"
        self.manifest.write_text(corpuslib.format_manifest(self.docs))

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def script(self, name, body):
        path = self.dir / name
        path.write_text("#!/bin/sh\n" + body)
        path.chmod(0o755)
        return path

    def main(self, program, oracle, *extra):
        out = self.dir / "report.json"
        argv = ["--pandoc", str(program), "--oracle", str(oracle), "--out", str(out),
                "--manifest", str(self.manifest), "--corpus", str(self.corpus), *extra]
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            code = run.main(argv)
        return code, (json.loads(out.read_text()) if out.exists() else None)

    def test_c01_a_second_run_uses_the_cache_when_version_text_contains_home(self):
        oracle = self.script("pandoc", 'if [ "$1" = --version ]; then echo "pandoc 1.0"; echo "dir $HOME"; exit 0; fi\n'
                                       'echo "$5 $4" >> "$(dirname "$0")/pandoc.log"\ncat "$5"\n')
        self.main(oracle, oracle)
        self.main(oracle, oracle)
        self.assertEqual(len((self.dir / "pandoc.log").read_text().splitlines()), 12 + 6)

    def test_c03_each_run_sees_the_same_fixed_home(self):
        program = self.script("pandoc", 'echo "$HOME" >> "$(dirname "$0")/home.log"\ncat "$5"\n')
        self.main(program, program, "--target", "json")
        self.main(program, program, "--target", "json")
        homes = set((self.dir / "home.log").read_text().splitlines())
        self.assertEqual(homes, {str(run.HOME)})

    @unittest.skipUnless(os.environ.get("RANDOC_CORPUS"), "needs the corpus")
    def test_c03_no_typst_document_imports_a_package(self):
        corpus = Path(os.environ["RANDOC_CORPUS"])
        for d in corpuslib.load_manifest():
            path = corpus / d.file_name
            if d.format == "typst" and path.exists():
                self.assertNotIn(b'#import "@', path.read_bytes(), d.name)

    def test_c04_a_hash_mismatch_removes_the_old_file(self):
        (self.corpus / "x.md").write_bytes(b"old stale text")
        status, _ = fetch.fetch_one(document("x"), self.corpus, lambda url: b"other")
        self.assertTrue(status.startswith("hash mismatch"))
        self.assertFalse((self.corpus / "x.md").exists())

    def test_c05_a_start_failure_or_a_signal_is_not_cached(self):
        cache = run.OracleCache(":memory:", "f")
        cache.put("start", run.Outcome(-1, b"", b"[corpus] cannot start\n"))
        cache.put("signal", run.Outcome(-9, b"", b""))
        cache.put("ok", run.Outcome(64, b"", b"parse error\n"))
        self.assertEqual(cache.keys(), {"ok"})
        cache.close()

    def test_c07_an_empty_selection_is_an_error(self):
        program = self.script("pandoc", 'cat "$5"\n')
        code, report = self.main(program, program, "--only", "no-such-document", "--expect-all-pass")
        self.assertEqual((code, report), (2, None))

    def test_c08_the_report_identifies_the_tested_program(self):
        program = self.script("randoc", 'if [ "$1" = --version ]; then echo "pandoc 3.13"; exit 0; fi\ncat "$5"\n')
        oracle = self.script("pandoc", 'if [ "$1" = --version ]; then echo "pandoc 3.13"; exit 0; fi\ncat "$5"\n')
        _, report = self.main(program, oracle)
        self.assertEqual(report["program_path"], str(program))
        self.assertEqual(report["program_sha256"], corpuslib.sha256_of(program.read_bytes()))
        self.assertEqual(report["oracle_sha256"], corpuslib.sha256_of(oracle.read_bytes()))
        self.assertNotIn(str(run.HOME), json.dumps(report))

    def test_c09_licence_corrections(self):
        docs = {d.name: d for d in corpuslib.load_manifest()}
        self.assertNotIn("csv-population", docs)
        for name in ("concord-opml-worknotes", "concord-example1-source"):
            self.assertEqual(docs[name].license, "GPL-3.0-or-later")

    def test_c10_a_manifest_entry_sets_its_own_time_limit(self):
        slow = document("one", data=b'{"a": 1}', timeout=3)
        self.manifest.write_text(corpuslib.format_manifest([slow, self.docs[1]]))
        self.assertIn("timeout = 3\n", self.manifest.read_text())
        self.assertEqual(corpuslib.parse_manifest(self.manifest.read_text())[0].timeout, 3)
        program = self.script("pandoc", 'sleep 2\ncat "$5"\n')
        _, report = self.main(program, program, "--timeout", "1", "--target", "json")
        self.assertEqual(report["results"], {"one:json": "pass", "two:json": "timeout"})
        self.assertEqual(report["timeout_seconds"], 1)
        for value in ("-1", "0", "true", "1.5", '"9"'):
            entry = corpuslib.format_manifest([document("x")]) + f"timeout = {value}\n"
            with self.subTest(value), self.assertRaisesRegex(corpuslib.ManifestError, "greater than 0"):
                corpuslib.parse_manifest(entry)
        with self.assertRaisesRegex(corpuslib.ManifestError, "whole number"):
            corpuslib.check([document("x", timeout=True)])
        timeouts = {d.name: d.timeout for d in corpuslib.load_manifest()}
        self.assertEqual(timeouts["jupyter-running-code"], 300)
        self.assertEqual(sum(1 for t in timeouts.values() if t), 1)


ESCAPED_CHILD = (
    "import subprocess,sys,time;"
    "subprocess.Popen([sys.executable,'-c','import time;time.sleep(3)'],start_new_session=True);"
    "time.sleep(30)"
)


class ThirdReviewTests(unittest.TestCase):
    """Tests for the findings of the Codex round 2 review (COR-R2-01, COR-R2-02)."""

    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="randoc-corpus-review3-"))

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def test_r2_01_a_child_in_a_new_session_cannot_hold_the_time_limit(self):
        with mock.patch.object(run, "CLEANUP_SECONDS", 0.05):
            started = time.monotonic()
            result = run.execute(sys.executable, ["-c", ESCAPED_CHILD], self.dir, run.HOME, 0.1)
            elapsed = time.monotonic() - started
        self.assertTrue(result.timed_out)
        self.assertLess(elapsed, 1)

    def test_r2_01_open_pipes_after_a_normal_end_are_a_tool_error(self):
        script = ("import subprocess,sys;"
                  "subprocess.Popen([sys.executable,'-c','import time;time.sleep(3)'],start_new_session=True)")
        with mock.patch.object(run, "CLEANUP_SECONDS", 0.1):
            started = time.monotonic()
            result = run.execute(sys.executable, ["-c", script], self.dir, run.HOME, 10)
            elapsed = time.monotonic() - started
        self.assertLess(elapsed, 1)
        self.assertEqual(result.exit, -1)
        self.assertIn(b"kept the output open", result.stderr)

    def test_r2_02_matching_overflow_outcomes_are_not_a_pass(self):
        expected = run.execute("/bin/sh", ["-c", "printf 0123456789A"], self.dir, run.HOME, 2, limit=10)
        actual = run.execute("/bin/sh", ["-c", "printf 0123456789B"], self.dir, run.HOME, 2, limit=10)
        for target in run.TARGETS:
            self.assertEqual(run.status(expected, actual, target), "error program")

    def test_r2_02_start_failures_and_signals_are_errors(self):
        not_a_program = self.dir / "data"
        not_a_program.write_text("x")
        not_a_program.chmod(0o644)
        failed = run.execute(not_a_program, [], self.dir, run.HOME, 2)
        killed = run.execute("/bin/sh", ["-c", "kill -9 $$"], self.dir, run.HOME, 2)
        self.assertEqual(killed.exit, -9)
        good = outcome(64, b"", b"parse error\n")
        self.assertEqual(run.status(failed, failed, "html"), "error program")
        self.assertEqual(run.status(good, killed, "html"), "error program")
        self.assertEqual(run.status(killed, good, "html"), "error oracle")
        self.assertEqual(run.status(good, outcome(64, b"", b"parse error\n"), "html"), "pass")

    def test_r2_02_expect_all_pass_fails_when_no_program_can_start(self):
        corpus = self.dir / "corpus"
        corpus.mkdir()
        (corpus / "one.md").write_bytes(b"alpha")
        manifest = self.dir / "manifest.toml"
        manifest.write_text(corpuslib.format_manifest([document("one")]))
        program = self.dir / "data"
        program.write_text("x")
        program.chmod(0o644)
        out = self.dir / "report.json"
        with contextlib.redirect_stdout(io.StringIO()):
            code = run.main(["--pandoc", str(program), "--oracle", str(program), "--manifest", str(manifest),
                             "--corpus", str(corpus), "--out", str(out), "--expect-all-pass"])
        self.assertEqual(code, 1)
        self.assertEqual(set(json.loads(out.read_text())["results"].values()), {"error program"})

    def test_fetch_a_download_has_a_total_time_limit(self):
        source = self.dir / "slow"
        source.write_bytes(b"x" * 100)
        with self.assertRaisesRegex(TimeoutError, "no complete download after -1 s"):
            fetch.download(source.as_uri(), total=-1)
        self.assertEqual(fetch.download(source.as_uri()), b"x" * 100)


class ExecuteCaseTests(unittest.TestCase):
    """One test for each case that `run.execute` must handle (Codex round 3)."""

    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="randoc-corpus-exec-"))

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def python(self, script, timeout, **kwargs):
        started = time.monotonic()
        result = run.execute(sys.executable, ["-c", script], self.dir, run.HOME, timeout, **kwargs)
        return result, time.monotonic() - started

    def test_1_normal_end(self):
        result, _ = self.python("import sys;sys.stdout.write('out');sys.stderr.write('err')", 10)
        self.assertEqual(result, run.Outcome(0, b"out", b"err"))

    def test_2_non_zero_exit_code(self):
        result, _ = self.python("import sys;print('x');sys.exit(64)", 10)
        self.assertEqual(result, run.Outcome(64, b"x\n", b""))

    def test_3_end_by_a_signal(self):
        result, _ = self.python("import os,signal;os.kill(os.getpid(),signal.SIGTERM)", 10)
        self.assertEqual((result.exit, result.timed_out), (-15, False))
        self.assertEqual(run.status(result, result, "html"), "error program")

    def test_4_start_failure(self):
        result = run.execute(self.dir / "missing", [], self.dir, run.HOME, 10)
        self.assertEqual((result.exit, result.timed_out), (-1, False))
        self.assertIn(b"[corpus]", result.stderr)

    def test_5_time_limit_with_the_program_alone(self):
        result, elapsed = self.python("import time;time.sleep(30)", 0.3)
        self.assertTrue(result.timed_out)
        self.assertLess(elapsed, 2)

    def test_6_time_limit_with_a_child_in_the_same_group(self):
        script = "import subprocess,sys,time;subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)']);time.sleep(30)"
        result, elapsed = self.python(script, 0.3)
        self.assertTrue(result.timed_out)
        self.assertLess(elapsed, 2)

    def test_7_a_child_in_a_new_session_keeps_the_pipes(self):
        with mock.patch.object(run, "CLEANUP_SECONDS", 0.05):
            result, elapsed = self.python(ESCAPED_CHILD, 0.1)
        self.assertTrue(result.timed_out)
        self.assertLess(elapsed, 1)

    def test_8_a_program_that_closes_its_outputs_early_and_ends_later(self):
        result, _ = self.python("import os,time;os.close(1);os.close(2);time.sleep(0.4);os._exit(64)", 2)
        self.assertEqual(result, run.Outcome(64, b"", b""))
        result, elapsed = self.python("import os,time;os.close(1);os.close(2);time.sleep(30)", 0.3)
        self.assertTrue(result.timed_out)
        self.assertLess(elapsed, 2)

    def test_9_output_above_the_size_limit(self):
        result, _ = self.python("import sys;sys.stdout.write('x'*100000)", 10, limit=1000)
        self.assertEqual((result.exit, len(result.stdout)), (-1, 1000))
        self.assertIn(b"output larger than 1000 bytes", result.stderr)

    def test_10_input_that_the_program_does_not_read(self):
        # The program gets /dev/null as standard input, so a reader sees the end at once.
        result, elapsed = self.python("import sys;print(len(sys.stdin.read()))", 10)
        self.assertEqual(result, run.Outcome(0, b"0\n", b""))
        self.assertLess(elapsed, 5)


class SlowServer:
    """A local HTTP server for one request that sends `head` and then `body` one byte at a time."""

    def __init__(self, head, body, pause=0.05):
        self.listener = socket.create_server(("127.0.0.1", 0))
        self.url = f"http://127.0.0.1:{self.listener.getsockname()[1]}/slow"
        self.head, self.body, self.pause = head, body, pause
        self.thread = threading.Thread(target=self.serve)
        self.thread.start()

    def serve(self):
        self.listener.settimeout(10)
        try:
            connection, _ = self.listener.accept()
        except OSError:
            return
        with connection:
            try:
                connection.recv(65536)
                connection.sendall(self.head)
                for byte in self.body:
                    time.sleep(self.pause)
                    connection.sendall(bytes([byte]))
            except OSError:
                pass

    def close(self):
        self.listener.close()
        self.thread.join()


class SlowServerTests(unittest.TestCase):
    """Each download through a slow server must end near the total time limit (COR-R3-02, COR-R4-01)."""

    TOTAL = 0.5

    def assert_stops_near_the_limit(self, head, body):
        server = SlowServer(head, body)
        started = time.monotonic()
        try:
            with self.assertRaisesRegex(TimeoutError, "no complete download after 0.5 s"):
                fetch.download(server.url, timeout=0.2, total=self.TOTAL)
            elapsed = time.monotonic() - started
        finally:
            server.close()
        self.assertLess(elapsed, self.TOTAL + 1.0)

    def test_a_server_that_sends_single_bytes_cannot_pass_the_total_limit(self):
        self.assert_stops_near_the_limit(b"HTTP/1.1 200 OK\r\nContent-Length: 200\r\n\r\n", b"x" * 200)

    def test_a_slow_chunk_size_line_cannot_pass_the_total_limit(self):
        self.assert_stops_near_the_limit(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
                                         b"0" * 200 + b"1\r\nx\r\n0\r\n\r\n")

    def test_slow_response_headers_cannot_pass_the_total_limit(self):
        self.assert_stops_near_the_limit(b"", b"HTTP/1.1 200 OK\r\n" + b"X-Slow: " + b"y" * 200)

    def test_a_normal_server_gives_the_document_and_leaves_no_temporary_file(self):
        before = set(Path(tempfile.gettempdir()).glob("randoc-corpus-download-*"))
        server = SlowServer(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nalpha", b"", pause=0)
        try:
            self.assertEqual(fetch.download(server.url, total=10), b"alpha")
        finally:
            server.close()
        self.assertEqual(set(Path(tempfile.gettempdir()).glob("randoc-corpus-download-*")), before)

    def test_a_timeout_removes_the_temporary_file(self):
        before = set(Path(tempfile.gettempdir()).glob("randoc-corpus-download-*"))
        self.assert_stops_near_the_limit(b"HTTP/1.1 200 OK\r\nContent-Length: 200\r\n\r\n", b"x" * 200)
        self.assertEqual(set(Path(tempfile.gettempdir()).glob("randoc-corpus-download-*")), before)

    def assert_cut_download_fails(self, head, body):
        before = set(Path(tempfile.gettempdir()).glob("randoc-corpus-download-*"))
        server = SlowServer(head, body, pause=0)
        try:
            with self.assertRaisesRegex(OSError, "IncompleteRead"):
                fetch.download(server.url, total=10)
        finally:
            server.close()
        self.assertEqual(set(Path(tempfile.gettempdir()).glob("randoc-corpus-download-*")), before)

    def test_r5_01_a_download_cut_before_content_length_fails(self):
        self.assert_cut_download_fails(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n", b"x" * 10)

    def test_r5_01_a_chunked_download_cut_before_the_last_chunk_fails(self):
        self.assert_cut_download_fails(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n", b"5\r\nalpha\r\n")
        self.assert_cut_download_fails(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n", b"a\r\nalp")

    def test_no_manifest_url_uses_a_rendering_api_that_changes(self):
        for d in corpuslib.load_manifest():
            self.assertNotIn("/api/rest_v1/", d.url, d.name)

    def test_r5_01_pin_does_not_write_a_cut_download(self):
        corpus = Path(tempfile.mkdtemp(prefix="randoc-corpus-cut-"))
        try:
            manifest = corpus / "manifest.toml"
            text = corpuslib.format_manifest([document("a", sha256="", size=0)])
            manifest.write_text(text)
            for _ in range(2):
                server = SlowServer(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n", b"x" * 10, pause=0)
                try:
                    if _ == 0:
                        with self.assertRaises(OSError):
                            fetch.fetch_one(document("a", sha256="", size=0), corpus,
                                            lambda url: fetch.download(server.url, total=10), pin=True)
                    else:
                        with contextlib.redirect_stdout(io.StringIO()) as out:
                            code = fetch.main(["--manifest", str(manifest), "--corpus", str(corpus), "--pin"],
                                              lambda url: fetch.download(server.url, total=10), 0)
                        self.assertEqual(code, 1)
                        self.assertIn("a: error:", out.getvalue())
                finally:
                    server.close()
                self.assertFalse((corpus / "a.md").exists())
            self.assertEqual(manifest.read_text(), text)
        finally:
            shutil.rmtree(corpus, ignore_errors=True)

    def test_a_server_error_is_an_os_error(self):
        server = SlowServer(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n", b"", pause=0)
        try:
            with self.assertRaisesRegex(OSError, "HTTP Error 404"):
                fetch.download(server.url, total=10)
        finally:
            server.close()
