"""Unit tests for the CommonMark specification tool. Run: python3 -m unittest discover -s tools/commonmark-spec"""

import tempfile
import unittest
from pathlib import Path

import run

FENCE = "`" * 32

SAMPLE = "\n".join([
    "# A made-up document",
    "",
    "Some prose that is not an example.",
    "",
    FENCE + " example",
    "one→two",
    "",
    "three",
    ".",
    "<p>ignored</p>",
    FENCE,
    "",
    FENCE + " example",
    ".",
    "",
    FENCE,
    "",
    "    " + FENCE + " example (indented, so not an example)",
    "",
])


def outcome(exit=0, stdout=b"{}", stderr=b"", timed_out=False):
    return run.Outcome(exit, stdout, stderr, timed_out)


class ExtractTests(unittest.TestCase):
    def test_examples_keep_lines_and_turn_arrows_into_tabs(self):
        self.assertEqual(run.extract_examples(SAMPLE), [(1, "one\ttwo\n\nthree\n"), (2, "")])

    def test_text_without_examples_gives_nothing(self):
        self.assertEqual(run.extract_examples("no examples\n"), [])


class CompareTests(unittest.TestCase):
    def test_json_is_compared_as_data(self):
        self.assertTrue(run.same_result(outcome(stdout=b'{"a": 1}'), outcome(stdout=b'{"a":1}\n')))
        self.assertFalse(run.same_result(outcome(stdout=b'{"a": 1}'), outcome(stdout=b'{"a": 2}')))

    def test_exit_code_stderr_and_time_limit_count(self):
        self.assertFalse(run.same_result(outcome(), outcome(exit=1)))
        self.assertFalse(run.same_result(outcome(), outcome(stderr=b"warning\n")))
        self.assertFalse(run.same_result(outcome(), outcome(timed_out=True)))
        self.assertTrue(run.same_result(outcome(stdout=b"x"), outcome(stdout=b"x")))

    def test_summary_has_numbers_and_no_text(self):
        results = {
            ("oracle", "a\n"): outcome(stdout=b"[1]"), ("randoc", "a\n"): outcome(stdout=b"[1]"),
            ("oracle", "b\n"): outcome(stdout=b"[1]"), ("randoc", "b\n"): outcome(stdout=b"[2]"),
            ("oracle", "c\n"): outcome(), ("randoc", "c\n"): outcome(timed_out=True),
            ("oracle", "d\n"): outcome(timed_out=True), ("randoc", "d\n"): outcome(),
        }
        summary = run.compare([(1, "a\n"), (2, "b\n"), (3, "c\n"), (4, "d\n")], "oracle", "randoc",
                              1.0, 2, run=lambda binary, source, timeout: results[(binary, source)])
        self.assertEqual(summary["examples"], 4)
        self.assertEqual(summary["pass"], 1)
        self.assertEqual(summary["failing_examples"], [2])
        self.assertEqual(summary["timeout_examples"], [3])
        self.assertEqual(summary["oracle_problem_examples"], [4])
        self.assertNotIn("a\n", str(summary))


class FetchTests(unittest.TestCase):
    def test_wrong_hash_is_refused_and_not_stored(self):
        with tempfile.TemporaryDirectory() as folder:
            with self.assertRaises(ValueError):
                run.fetch(folder, getter=lambda: b"not the specification")
            self.assertFalse((Path(folder) / run.SPEC_FILE).exists())

    def test_a_stored_file_with_the_right_hash_is_used(self):
        data = b"stored text"
        with tempfile.TemporaryDirectory() as folder:
            (Path(folder) / run.SPEC_FILE).write_bytes(data)
            original = run.SPEC_SHA256
            run.SPEC_SHA256 = run.sha256_of(data)
            try:
                text = run.fetch(folder, getter=lambda: self.fail("no download expected"))
            finally:
                run.SPEC_SHA256 = original
            self.assertEqual(text, "stored text")

    def test_check_hash(self):
        run.check_hash(b"x", run.sha256_of(b"x"))
        with self.assertRaises(ValueError):
            run.check_hash(b"x", run.sha256_of(b"y"))


if __name__ == "__main__":
    unittest.main()
