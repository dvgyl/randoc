"""Unit tests of the case generator. Run: python3 -m unittest discover -s tools/casegen"""

import contextlib
import hashlib
import io
import json
import os
import re
import sys
import tempfile
import unittest
from pathlib import Path

import casegen

INVENTORY = Path(__file__).resolve().parents[2] / "spec" / "inventory"
EXTENSIONS = json.loads((INVENTORY / "extensions.json").read_text(encoding="utf-8"))
NO_ORACLE = ("ext-matrix", "options-cli")


def fake_oracle_json(reader, content):
    return json.dumps({"reader": reader, "size": len(content)}).encode()


def cases_of(files):
    return [json.loads(data) for name, data in sorted(files.items()) if name.endswith(".case.json")]


def format_and_toggles(spec):
    name = re.match(r"[a-z0-9_]+", spec).group(0)
    return name, re.findall(r"([+-])([a-z0-9_]+)", spec[len(name):])


class GenerateTest(unittest.TestCase):
    def test_same_seed_gives_same_bytes(self):
        first = casegen.generate(42, 80, EXTENSIONS, casegen.FAMILIES, fake_oracle_json)
        second = casegen.generate(42, 80, EXTENSIONS, casegen.FAMILIES, fake_oracle_json)
        self.assertEqual(first, second)

    def test_other_seed_gives_other_cases(self):
        first = casegen.generate(1, 30, EXTENSIONS, NO_ORACLE)
        second = casegen.generate(2, 30, EXTENSIONS, NO_ORACLE)
        self.assertNotEqual(first, second)

    def test_shorter_run_is_prefix_of_longer_run(self):
        short = casegen.generate(9, 20, EXTENSIONS, casegen.FAMILIES, fake_oracle_json)
        long = casegen.generate(9, 70, EXTENSIONS, casegen.FAMILIES, fake_oracle_json)
        short_cases = cases_of(short)
        self.assertEqual(len(short_cases), 20)
        for case in short_cases:
            self.assertEqual(short[f"{case['id']}.case.json"], long[f"{case['id']}.case.json"])
            self.assertEqual(short[case["input"]], long[case["input"]])

    def test_count_is_exact_and_each_case_has_its_input(self):
        files = casegen.generate(3, 57, EXTENSIONS, casegen.FAMILIES, fake_oracle_json)
        cases = cases_of(files)
        self.assertEqual(len(cases), 57)
        self.assertEqual(len(files), 2 * 57)
        for case in cases:
            self.assertIn(case["input"], files)
            self.assertIn(case["family"], casegen.FAMILIES)

    def test_families_and_readers_are_covered(self):
        cases = cases_of(casegen.generate(11, 400, EXTENSIONS, casegen.FAMILIES, fake_oracle_json))
        self.assertEqual({case["family"] for case in cases}, set(casegen.FAMILIES))
        readers = {format_and_toggles(case["args"][1])[0] for case in cases if case["family"] == "ext-matrix"}
        self.assertTrue({"markdown", "commonmark_x", "gfm", "html", "latex", "rst", "org"} <= readers)

    def test_extension_toggles_are_supported_by_the_format(self):
        cases = cases_of(casegen.generate(5, 300, EXTENSIONS, NO_ORACLE))
        toggled = 0
        for case in (c for c in cases if c["family"] == "ext-matrix"):
            for flag in ("-f", "-t"):
                fmt, toggles = format_and_toggles(case["args"][case["args"].index(flag) + 1])
                supported = set(EXTENSIONS["formats"][fmt]["supported"])
                default = set(EXTENSIONS["formats"][fmt]["default"])
                for sign, name in toggles:
                    toggled += 1
                    self.assertIn(name, supported, case)
                    self.assertEqual(sign == "-", name in default, case)
        self.assertGreater(toggled, 100)

    def test_ext_matrix_uses_only_safe_options(self):
        cases = cases_of(casegen.generate(6, 300, EXTENSIONS, NO_ORACLE))
        for case in (c for c in cases if c["family"] == "ext-matrix"):
            args = case["args"]
            self.assertEqual(args[-1], case["input"])
            rest = iter(args[4:-1])
            for arg in rest:
                self.assertIn(arg.split("=", 1)[0], casegen.SAFE_OPTION_NAMES, case)
                if arg == "-M":
                    next(rest)

    def test_writers_and_readers_exist_in_the_inventory(self):
        outputs = set(json.loads((INVENTORY / "output-formats.json").read_text(encoding="utf-8")))
        inputs = set(json.loads((INVENTORY / "input-formats.json").read_text(encoding="utf-8")))
        self.assertLessEqual(set(casegen.WRITERS), outputs)
        self.assertLessEqual(set(casegen.READERS), inputs)

    def test_inputs_have_no_tokens_left(self):
        files = casegen.generate(8, 400, EXTENSIONS, NO_ORACLE)
        for name, data in files.items():
            if not name.endswith(".case.json"):
                text = data.decode("utf-8")
                self.assertIsNone(casegen.LEFTOVER.search(text), (name, casegen.LEFTOVER.search(text)))

    def test_every_template_fills_without_tokens_left(self):
        r = casegen.Rng(1)
        for snippets, headers in casegen.bank.BANKS.values():
            for template in snippets + headers:
                text = casegen.fill(r, template, "ref1")
                self.assertIsNone(casegen.LEFTOVER.search(text), template[:40])

    def test_fixed_tokens_repeat_inside_a_snippet_and_vary_between_snippets(self):
        r = casegen.Rng(3)
        values = [casegen.fill(r, "@id@ @id@ @v@ @V@ @ref@ @ref@", "doc7").split() for _ in range(30)]
        for first, second, word, capital, ref, ref2 in values:
            self.assertEqual(first, second)
            self.assertEqual(capital, word.capitalize())
            self.assertEqual((ref, ref2), ("doc7", "doc7"))
        self.assertGreater(len({v[0] for v in values}), 10)

    def test_golden_output_of_a_visible_seed(self):
        # This hash changes when the generator or the extension inventory changes.
        # Then raise GENERATOR in casegen.py and write the new hash here.
        files = casegen.generate(1, 40, EXTENSIONS, NO_ORACLE)
        digest = hashlib.sha256()
        for name in sorted(files):
            digest.update(name.encode() + b"\0" + files[name] + b"\0")
        self.assertEqual(digest.hexdigest(), "b19d4216b202344d7708ca793a40e8dc8e567be829ee01a8b654bf5e5825288e")

    def test_underline_and_overline_match_the_title(self):
        r = casegen.Rng(2)
        lines = casegen.fill(r, "@u:#@\n@W@ @w@ @w@\n@u:#@\n\n@W@\n@u:-@", "x1").split("\n")
        self.assertEqual(len(lines[0]), max(len(lines[1]), 3))
        self.assertEqual(lines[0], lines[2])
        self.assertEqual(set(lines[0]), {"#"})
        self.assertEqual(len(lines[5]), max(len(lines[4]), 3))

    def test_tables_keep_columns_aligned(self):
        r = casegen.Rng(4)
        for _ in range(20):
            lines = casegen.table(r, "grid").split("\n")
            self.assertEqual(len({len(line) for line in lines}), 1, lines)
            plus = [i for i, ch in enumerate(lines[0]) if ch == "+"]
            for line in lines[1::2]:
                self.assertEqual([i for i, ch in enumerate(line) if ch == "|"], plus)

    def test_json_roundtrip_writes_the_oracle_json_as_input(self):
        files = casegen.generate(13, 200, EXTENSIONS, ("json-roundtrip",), fake_oracle_json)
        cases = cases_of(files)
        derived = [case for case in cases if "source" in case]
        self.assertTrue(derived)
        by_id = {case["id"]: case for case in cases}
        for case in derived:
            self.assertEqual(case["args"][:2], ["-f", "json"])
            source = by_id[case["source"]]
            self.assertEqual(source["args"][2:4], ["-t", "json"])
            reader = source["args"][1]
            self.assertEqual(json.loads(files[case["input"]]),
                             {"reader": reader, "size": len(files[source["input"]])})

    def test_json_roundtrip_without_oracle_result_keeps_only_the_read_case(self):
        cases = cases_of(casegen.generate(13, 25, EXTENSIONS, ("json-roundtrip",), lambda reader, content: None))
        self.assertEqual(len(cases), 25)
        self.assertTrue(all(case["args"][2:4] == ["-t", "json"] for case in cases))

    def test_json_roundtrip_needs_an_oracle(self):
        with self.assertRaises(ValueError):
            casegen.generate(1, 5, EXTENSIONS, casegen.FAMILIES, None)

    def test_options_cli_unsupported_extension_is_really_unsupported(self):
        cases = cases_of(casegen.generate(21, 300, EXTENSIONS, ("options-cli",)))
        checked = 0
        for case in cases:
            args = case["args"]
            if "-f" in args:
                fmt, toggles = format_and_toggles(args[args.index("-f") + 1])
                for sign, name in toggles:
                    if fmt in EXTENSIONS["formats"]:
                        checked += 1
                        self.assertNotIn(name, EXTENSIONS["formats"][fmt]["supported"], case)
        self.assertGreater(checked, 10)


class FilesTest(unittest.TestCase):
    def test_directory_and_tar_hold_the_same_cases(self):
        files = casegen.generate(17, 15, EXTENSIONS, NO_ORACLE)
        with tempfile.TemporaryDirectory() as tmp:
            directory, archive = Path(tmp, "cases"), Path(tmp, "cases.tar")
            casegen.write_directory(files, directory)
            casegen.write_tar(files, archive)
            self.assertEqual(casegen.read_cases(directory), casegen.read_cases(archive))
            self.assertEqual(len(casegen.read_cases(archive)), 15)
            first = archive.read_bytes()
            casegen.write_tar(files, archive)
            self.assertEqual(first, archive.read_bytes())

    def test_directory_must_be_empty(self):
        with tempfile.TemporaryDirectory() as tmp:
            Path(tmp, "old.txt").write_text("x")
            with self.assertRaises(SystemExit):
                casegen.write_directory({"a": b"b"}, Path(tmp))

    def test_command_line_end_to_end_with_a_fake_oracle(self):
        with tempfile.TemporaryDirectory() as tmp:
            oracle = Path(tmp, "oracle")
            oracle.write_text(f"#!{sys.executable}\nimport sys\nsys.stdin.buffer.read()\nprint('{{}}')\n")
            os.chmod(oracle, 0o755)
            out = Path(tmp, "out.tar")
            with contextlib.redirect_stdout(io.StringIO()):
                code = casegen.main(["--seed", "3", "--count", "30", "--out", str(out), "--oracle", str(oracle),
                                     "--extensions", str(INVENTORY / "extensions.json")])
            self.assertEqual(code, 0)
            cases = casegen.read_cases(out)
            self.assertEqual(len(cases), 30)
            derived = [data for record, data in cases if "source" in record]
            self.assertTrue(all(data == b"{}\n" for data in derived))

    def test_rejects_a_bad_family(self):
        with tempfile.TemporaryDirectory() as tmp, self.assertRaises(SystemExit), \
                contextlib.redirect_stderr(io.StringIO()):
            casegen.main(["--seed", "1", "--count", "1", "--out", str(Path(tmp, "x")), "--families", "nope"])


if __name__ == "__main__":
    unittest.main()
