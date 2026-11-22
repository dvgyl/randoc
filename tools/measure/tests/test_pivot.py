import hashlib
import io
import json
import os
import re
import sys
import tempfile
import unittest
import zipfile
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools/pivot"))
import generate as pivot
from normalize import notebook_output, notebook_positions, notebook_rule
from observe import NoResult, Observation, differences, run
from reference import cache_key, classify, open_cache, read_reference, write_references
from writers import Unit, references


def measure_tree(value):
    """Count tagged nodes and their nesting depth."""
    stack = [(value, 0)]
    count, deepest = 0, 0
    while stack:
        part, depth = stack.pop()
        if isinstance(part, dict):
            if "t" in part:
                count += 1
                depth += 1
                deepest = max(deepest, depth)
            stack.extend((child, depth) for child in part.values())
        elif isinstance(part, list):
            stack.extend((child, depth) for child in part)
    return count, deepest


class PivotTests(unittest.TestCase):
    def test_fixed_list_hash_and_coverage(self):
        data = pivot.generate(ROOT, lambda _tree: True)
        self.assertEqual(hashlib.sha256(data).hexdigest(),
                         (ROOT / "spec/pivot.sha256").read_text().strip())
        rows = [json.loads(line) for line in data.splitlines()]
        dictionaries = {row["id"]: row["tree"] for row in rows}
        self.assertEqual(len(dictionaries), 400)
        self.assertEqual(pivot.json_bytes(rows[0]["tree"]),
                         (ROOT / "conformance/fixtures/files/sample-cell.json").read_bytes())
        counts = Counter(name.split("/")[0] for name in dictionaries)
        self.assertEqual(counts["highlight"], 194)
        self.assertEqual(counts["math"], 60)
        for language in json.loads((ROOT / "spec/inventory/highlight-languages.json").read_text()):
            code = dictionaries[f"highlight/{language}"]["blocks"][0]
            self.assertEqual(code["t"], "CodeBlock")
            self.assertEqual(code["c"][0][1], [language])
            self.assertGreaterEqual(len(code["c"][1].splitlines()), 15)

        commands = set()
        for name, tree in dictionaries.items():
            if name.startswith("math/commands-"):
                for paragraph in tree["blocks"]:
                    expression = paragraph["c"][0]["c"][1]
                    commands.update(re.findall(r"\\([a-zA-Z]+)", expression))
        names = (ROOT / "crates/randoc-math/tools/katex-names.txt").read_text().splitlines()
        self.assertEqual(commands, {name for name in names if name and not name.startswith("#")})
        malformed = {tree["blocks"][0]["c"][0]["c"][1] for name, tree in dictionaries.items()
                     if name.startswith("math/invalid-")}
        self.assertEqual(len(malformed), 20)

    def test_rejected_random_seed_uses_the_next_seed(self):
        def accept(tree):
            return tree["meta"]["identifier"]["c"] != "urn:randoc:pivot:seed/0001"

        rows = [json.loads(line) for line in pivot.generate(ROOT, accept).splitlines()]
        seeds = [row["id"] for row in rows if row["id"].startswith("seed/")]
        self.assertEqual(seeds[0], "seed/0002")
        self.assertEqual(len(rows), 400)

    def test_generated_random_trees_stay_within_limits(self):
        for seed in range(1, 1001):
            count, depth = measure_tree(pivot.random_tree(seed))
            self.assertLessEqual(count, 200)
            self.assertLessEqual(depth, 6)


class ObservationTests(unittest.TestCase):
    def test_output_and_files_come_from_the_same_run(self):
        with tempfile.TemporaryDirectory() as folder:
            program = Path(folder) / "program"
            program.write_text('#!/bin/sh\ncat\nmkdir "$HOME"\nprintf "note" > "$HOME/mark"\n')
            program.chmod(0o755)
            result = run(program, [], b"input")
        self.assertEqual((result.exit, result.stdout, result.stderr), (0, b"input", b""))
        self.assertEqual(result.files, {".home": {"kind": "directory"}, ".home/mark": {
            "kind": "file", "sha256": hashlib.sha256(b"note").hexdigest()}})

    def test_absent_program_has_no_result(self):
        with tempfile.TemporaryDirectory() as folder:
            with self.assertRaises(NoResult):
                run(Path(folder) / "missing", [], b"")

    def test_both_programs_get_the_specified_environment(self):
        expected = {
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8",
            "TZ": "UTC", "SOURCE_DATE_EPOCH": "1700000000", "GHCRTS": "-M3g",
            "RANDOC_DATA_DIR": os.environ.get("RANDOC_DATA_DIR")
            or str(Path(os.environ.get("RANDOC_ENV", ROOT.parent / "env")) / "datapack"),
        }
        for oracle in (False, True):
            result = run("/usr/bin/env", [], b"", oracle=oracle)
            observed = dict(line.split("=", 1) for line in result.stdout.decode().splitlines())
            self.assertTrue(observed.pop("HOME").endswith("/.home"))
            self.assertEqual(observed, expected)

    def test_zip_ignores_directory_entries_and_checks_the_first_entry(self):
        def output(names, method):
            stream = io.BytesIO()
            with zipfile.ZipFile(stream, "w", compression=method) as archive:
                for name in names:
                    archive.writestr(name, b"" if name.endswith("/") else name.encode())
            return Observation(0, stream.getvalue(), b"", {})

        expected = output(["mimetype", "a/file"], zipfile.ZIP_STORED)
        same = output(["mimetype", "a/", "a/file"], zipfile.ZIP_STORED)
        wrong = output(["a/file", "mimetype"], zipfile.ZIP_DEFLATED)
        invalid = Observation(0, b"broken archive", b"", {})
        self.assertEqual(differences("epub", expected, same), [])
        self.assertEqual(differences("epub", expected, wrong), ["stdout"])
        self.assertEqual(differences("epub", invalid, invalid), ["stdout"])
        self.assertEqual(differences("html", expected, same), ["stdout"])

    def test_damaged_compressed_member_is_an_output_mismatch(self):
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            archive.writestr("x", b"ordinary document content")
        data = stream.getvalue()
        damaged = data[:31] + bytes([255]) + data[32:]
        first = Observation(0, data, b"", {})
        actual = Observation(0, damaged, b"", {})
        self.assertEqual(differences("docx", first, actual), ["stdout"])


class NotebookTests(unittest.TestCase):
    FIRST = "ab75f436-30f3-4aa0-9ae0-44837c5bbbdb"
    SECOND = "fd8c456b-0cd0-47c3-9628-c689bf301f57"
    STABLE = "bc303b57-7acb-47bf-afac-e888a17fad55"

    def notebook(self, first_id, stable_id=STABLE, metadata="survey"):
        return pivot.json_bytes({"cells": [{"id": first_id}, {"id": stable_id}],
                                 "metadata": {"id": metadata}})

    def test_only_changed_cell_positions_are_normalized(self):
        rule = notebook_rule()
        first, second = self.notebook(self.FIRST), self.notebook(self.SECOND)
        positions = notebook_positions(first, second, rule)
        self.assertEqual(positions, (0,))
        normalized = notebook_output(first, positions, rule)
        self.assertEqual(normalized, self.notebook("<ID1>"))
        self.assertEqual(notebook_output(second, positions, rule), normalized)
        other = self.notebook(self.SECOND, stable_id=self.FIRST)
        self.assertNotEqual(notebook_output(other, positions, rule), normalized)

    def test_duplicate_identifier_and_normalized_literal_keep_no_match(self):
        rule = notebook_rule()
        duplicate = self.notebook(self.FIRST, metadata=self.FIRST)
        self.assertIsNone(notebook_output(duplicate, (0,), rule))
        self.assertIsNone(notebook_output(self.notebook("<ID1>"), (0,), rule))


class ReferenceTests(unittest.TestCase):
    def test_a_cached_reference_keeps_the_original_result(self):
        first = Observation(0, b"a", b"", {})
        second = Observation(0, b"a", b"warning\n", {})
        stable = classify("html", first, first, None)
        unstable = classify("html", first, second, None)
        stopped = classify("html", first, "the oracle stopped", None)
        self.assertEqual(stable.status, "stable")
        self.assertEqual(unstable.unstable_parts, ("stderr",))
        self.assertEqual(stopped.status, "oracle_limit")
        with tempfile.TemporaryDirectory() as folder:
            database = open_cache(Path(folder) / "cache.sqlite")
            try:
                key = cache_key(b"version", ["-t", "html"], b"test")
                self.assertIsNone(read_reference(database, key))
                write_references(database, [(key, unstable)])
                write_references(database, [(key, stable)])
                self.assertEqual(read_reference(database, key), unstable)
            finally:
                database.close()

    def test_references_retry_a_limit_and_recheck_saved_results(self):
        good = Observation(0, b"first", b"", {})
        changed = Observation(0, b"later", b"", {})
        unit = Unit("one", b"data", b"key")
        cache_args = ["-f", "json", "-t", "html"]
        limited = classify("html", "oracle timeout", good, None)
        with tempfile.TemporaryDirectory() as folder, ThreadPoolExecutor(max_workers=2) as pool:
            database = open_cache(Path(folder) / "cache.sqlite")
            try:
                write_references(database, [(unit.key, limited)])
                with patch("writers.attempt", side_effect=[good, good, changed]) as oracle, \
                     patch("writers.time.sleep"):
                    references(database, [unit], "oracle", cache_args, pool, None)
                    self.assertEqual(oracle.call_count, 2)
                    self.assertEqual(read_reference(database, unit.key).status, "stable")
                    references(database, [unit], "oracle", cache_args, pool, None, verify_saved=True)
                    updated = read_reference(database, unit.key)
                    self.assertEqual(oracle.call_count, 3)
                    self.assertEqual(updated.status, "unstable")
                    self.assertEqual(updated.unstable_parts, ("stdout",))
                    self.assertEqual(updated.first, good)
            finally:
                database.close()

    def test_limited_recheck_preserves_the_original_and_its_differences(self):
        first = Observation(0, b"first", b"", {})
        other = Observation(0, b"other", b"", {})
        unit = Unit("one", b"data", b"key")
        command = ["-f", "json", "-t", "html"]
        with tempfile.TemporaryDirectory() as folder, ThreadPoolExecutor(max_workers=2) as pool:
            database = open_cache(Path(folder) / "cache.sqlite")
            try:
                write_references(database, [(unit.key, classify("html", first, other, None))])
                with patch("writers.attempt", side_effect=["timeout", other, other]), patch("writers.time.sleep"):
                    references(database, [unit], "oracle", command, pool, None, verify_saved=True)
                    stopped = read_reference(database, unit.key)
                    self.assertEqual(stopped.status, "oracle_limit")
                    self.assertEqual(stopped.first, first)
                    self.assertEqual(stopped.unstable_parts, ("stdout",))
                    references(database, [unit], "oracle", command, pool, None)
                    retried = read_reference(database, unit.key)
                    self.assertEqual(retried.status, "unstable")
                    self.assertEqual(retried.first, first)
                    self.assertEqual(retried.unstable_parts, ("stdout",))
            finally:
                database.close()


if __name__ == "__main__":
    unittest.main()
