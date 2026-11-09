"""Unit tests for the parsers of the test tool. The examples are written for these tests."""

import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import suite  # noqa: E402
from suite import Outcome  # noqa: E402


class CodeBlocks(unittest.TestCase):
    def test_backtick_and_tilde_fences(self):
        text = "intro\n\n```\none\n```\n\ntext\n\n~~~~ {.x}\ntwo\n~~~~\n"
        self.assertEqual(suite.code_blocks(text), ["one", "two"])

    def test_closing_fence_must_be_as_long_as_the_opening_fence(self):
        text = "````\na\n```\nb\n`````\nafter\n"
        self.assertEqual(suite.code_blocks(text), ["a\n```\nb"])

    def test_other_fence_character_does_not_close(self):
        self.assertEqual(suite.code_blocks("~~~\n```\nx\n~~~\n"), ["```\nx"])

    def test_html_comment_hides_a_block(self):
        text = "<!-- note\n\n```\nhidden\n```\n-->\n\n```\nshown\n```\n"
        self.assertEqual(suite.code_blocks(text), ["shown"])

    def test_one_line_comment(self):
        self.assertEqual(suite.code_blocks("<!-- x -->\n```\ny\n```\n"), ["y"])

    def test_tabs_stay(self):
        self.assertEqual(suite.code_blocks("```\n\tz\n```\n"), ["\tz"])

    def test_unclosed_fence_is_not_a_block(self):
        self.assertEqual(suite.code_blocks("```\nopen\n"), [])

    def test_backtick_in_info_string_is_not_a_fence(self):
        self.assertEqual(suite.code_blocks("``` a`b\nc\n```\n"), [])

    def test_indented_fence_removes_its_indent(self):
        self.assertEqual(suite.code_blocks("  ```\n  a\n b\n  ```\n"), ["a\nb"])

    def test_empty_block(self):
        self.assertEqual(suite.code_blocks("```\n```\n"), [""])


class CommandBlock(unittest.TestCase):
    def test_parts(self):
        block = "% pandoc -t html\n*x*\n^D\n<p><em>x</em></p>"
        self.assertEqual(
            suite.parse_command_block(block), ("pandoc -t html", "*x*\n", "<p><em>x</em></p>\n")
        )

    def test_continuation_lines_join_with_spaces(self):
        block = "% pandoc \\\n-t \\\nhtml\n^D\nout"
        self.assertEqual(suite.parse_command_block(block)[0], "pandoc  -t  html")

    def test_expected_output_stops_at_dot_line(self):
        block = "% pandoc\nin\n^D\nline\n.\nignored"
        self.assertEqual(suite.parse_command_block(block)[2], "line\n")

    def test_no_end_of_input_marker(self):
        self.assertEqual(suite.parse_command_block("% pandoc\na\nb"), ("pandoc", "a\nb\n", ""))

    def test_block_without_command(self):
        self.assertIsNone(suite.parse_command_block("just code"))


class CommandOutput(unittest.TestCase):
    def test_success(self):
        self.assertEqual(suite.command_output(Outcome(0, b"a\r\nb\n", b"")), "a\nb\n")

    def test_failure_with_stderr(self):
        out = Outcome(3, b"x\n", b"first\nsecond")
        self.assertEqual(suite.command_output(out), "2> first\n2> second\nx\n=> 3\n")

    def test_case_compares_the_text(self):
        case = suite.CommandCase("c#1", "command", "pandoc", "in\n", "2> w\n=> 1\n")
        calls = []

        def execute(command, stdin):
            calls.append((command, stdin))
            return Outcome(1, b"", b"w\n")

        self.assertIsNone(case.evaluate(execute, "."))
        self.assertEqual(calls, [("pandoc", b"in\n")])
        self.assertIsNotNone(case.evaluate(lambda c, s: Outcome(0, b"", b""), "."))

    def test_timeout_fails(self):
        case = suite.CommandCase("c#1", "command", "pandoc", "", "")
        self.assertEqual(case.evaluate(lambda c, s: Outcome(-1, b"", b"", True), "."), "timeout")


class Fb2Normalize(unittest.TestCase):
    def test_breaks_between_tags_and_drops_binary(self):
        text = '<a><b>t</b><binary id="x">AAA</binary></a>'
        self.assertEqual(suite.fb2_normalize(text), "<a>\n<b>t</b>\n</a>\n")

    def test_short_text(self):
        self.assertEqual(suite.fb2_normalize("x"), "")

    def test_drops_empty_lines(self):
        self.assertEqual(suite.fb2_normalize("<a></a>\n\n"), "<a>\n</a>\n")

    def test_drops_a_line_that_is_a_prefix_of_the_tag(self):
        self.assertEqual(suite.fb2_normalize("<a>\n<bi\n<b>x</b>\n"), "<a>\n<b>x</b>\n")


OLD_SAMPLE = '''
module Tests.Old (tests) where

tests :: FilePath -> [TestTree]
tests pandocPath =
  [ testGroup "fmt"
    [ testGroup "writer" $ writerTests' "fmt" ++ lhsWriterTests' "fmt"
    , test' "reader" ["-r", "fmt", "-w", "native"] "in.fmt" "out.native"
    , test' "reader" ["-r", "fmt"] ("in" <.> "x") ("dir" </> "out")
    -- a comment
    ]
  , testGroup "more" $ map (\\f -> testGroup f $ writerTests' f) [ "p", "q" ]
  , testGroup "m" $ mconcat [ extWriterTests' "z" ]
  , testGroup "fb" [ fb2WriterTest' "basic" [] "a.md" "a.fb2" ]
  , s5WriterTest' "basic" ["-s"] "s5"
  , lhsReaderTest' "markdown+lhs"
  ]
 where
    test' = test pandocPath
'''


class OldParser(unittest.TestCase):
    def setUp(self):
        self.tests = dict(suite.parse_old(OLD_SAMPLE))

    def test_identifiers(self):
        self.assertEqual(
            list(self.tests),
            [
                "old/fmt/writer/basic", "old/fmt/writer/tables",
                "old/fmt/writer/lhs to normal", "old/fmt/writer/lhs to lhs",
                "old/fmt/reader", "old/fmt/reader#2",
                "old/more/p/basic", "old/more/p/tables", "old/more/q/basic", "old/more/q/tables",
                "old/m/basic", "old/m/tables", "old/m/planets table",
                "old/m/nordics table", "old/m/students table",
                "old/fb/basic", "old/s5 writer (basic)", "old/lhs",
            ],
        )

    def test_writer_test_arguments(self):
        golden = self.tests["old/fmt/writer/basic"]
        self.assertEqual(golden.input, "testsuite.native")
        self.assertEqual(golden.norm, "writer.fmt")
        self.assertEqual(
            golden.args,
            ("-r", "native", "-w", "fmt", "--columns=80", "--variable", "pandoc-version=", "-s"),
        )

    def test_path_operators(self):
        golden = self.tests["old/fmt/reader#2"]
        self.assertEqual((golden.input, golden.norm), ("in.x", "dir/out"))

    def test_lhs_and_fb2(self):
        self.assertEqual(self.tests["old/fmt/writer/lhs to lhs"].norm, "lhs-test.fmt+lhs")
        self.assertEqual(self.tests["old/lhs"].norm, "lhs-test-markdown.native")
        self.assertEqual(self.tests["old/fb/basic"].normalize, "fb2")
        self.assertEqual(self.tests["old/m/planets table"].input, "tables/planets.native")

    def test_unknown_function(self):
        source = "tests p =\n  [ strange \"x\" ]\n where\n"
        with self.assertRaises(suite.HaskellError):
            suite.parse_old(source)


READER_SAMPLE = '''
tests = [ testCompare "one" "d/a.docx" "d/a.native"
        , testCompareWithOpts def{readerTrackChanges=AllChanges}
            "two" "d/b.docx" "d/b.native"
        , testCompareWithOpts def{readerExtensions =
              extensionsFromList [Ext_styles]}
            "three" "d/c.docx" "d/c.native"
        , testCompareWithOpts def{readerAbbreviations=x}
            "four" "d/e.docx" "d/e.native"
        , testCompare "one" "d/f.docx" "d/f.native"
        ]
'''


class ReaderTests(unittest.TestCase):
    def test_compare_entries(self):
        cases, skipped = suite.parse_compare_tests(READER_SAMPLE, "docx", "readers/docx")
        self.assertEqual(skipped, 1)
        self.assertEqual(
            [c.id for c in cases],
            ["readers/docx/one", "readers/docx/two", "readers/docx/three", "readers/docx/one#2"],
        )
        self.assertIsNone(cases[0].extensions)
        self.assertEqual(cases[1].extensions, frozenset())
        self.assertEqual(cases[1].extra_args, ("--track-changes=all",))
        self.assertEqual(cases[2].extensions, frozenset({"styles"}))
        self.assertEqual((cases[2].input_path, cases[2].expected_path), ("d/c.docx", "d/c.native"))

    def test_options(self):
        self.assertEqual(
            suite.parse_reader_options("def{readerStandalone=True}"), (frozenset(), ("--standalone",))
        )
        self.assertIsNone(suite.parse_reader_options("def{readerColumns=3}"))

    def test_string_list_skips_comments(self):
        source = 'names :: [String]\nnames = [ "a"\n--      , "b"\n        , "c"\n        ]\n'
        self.assertEqual(suite.string_list(source, "names"), ["a", "c"])

    def test_exact_extensions(self):
        suite._EXTENSION_CACHE.clear()

        def execute(args, stdin):
            self.assertEqual(args, ("--list-extensions=fmtx",))
            return Outcome(0, b"+alpha\n-beta\n+gamma\n", b"")

        spec = suite.format_with_exact_extensions(execute, "fmtx", frozenset({"gamma", "beta"}))
        self.assertEqual(spec, "fmtx-alpha+beta")

    def test_ast_case_compares_json(self):
        doc = json.dumps({"blocks": [], "meta": {}}).encode()

        def execute(args, stdin):
            return Outcome(0, doc if "in.x" in args else b'{"blocks": [1], "meta": {}}', b"")

        case = suite.AstCase("r/1", "r", "x", "in.x", "native", "out.native")
        self.assertIsNotNone(case.evaluate(execute, "."))
        same = suite.AstCase("r/1", "r", "x", "in.x", "native", "in.x")
        self.assertIsNone(same.evaluate(execute, "."))


class GoldenFiles(unittest.TestCase):
    def test_golden_case(self):
        with tempfile.TemporaryDirectory() as directory:
            with open(os.path.join(directory, "want.txt"), "wb") as handle:
                handle.write(b"a\r\nb\n")
            case = suite.GoldenCase("g", "old", ("x",), "want.txt")
            self.assertIsNone(case.evaluate(lambda a, s: Outcome(0, b"a\nb\n", b""), directory))
            self.assertIsNotNone(case.evaluate(lambda a, s: Outcome(0, b"a\n", b""), directory))
            failed = case.evaluate(lambda a, s: Outcome(2, b"a\nb\n", b"bad"), directory)
            self.assertTrue(failed.startswith("exit code 2"))


class Counting(unittest.TestCase):
    def test_count(self):
        source = (
            "testCase :: String -> Assertion\n"
            '  [ "a" =: "x" =?> para "x"\n'
            "  -- \"b\" =?> skipped\n"
            '  , testCase "c" $ assert True\n'
            '  , testProperty "d" prop ]\n'
        )
        self.assertEqual(suite.count_tests(source), 3)


if __name__ == "__main__":
    unittest.main()
