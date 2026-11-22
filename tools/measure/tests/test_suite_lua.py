import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import measure

sys.path.insert(0, str(measure.ROOT / "tools/pivot"))
from observe import run

source = measure.ROOT / "tools/measure/suites/lua.py"
spec = importlib.util.spec_from_file_location("lua_suite", source)
lua = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lua)


class LuaSuiteTests(unittest.TestCase):
    def test_the_report_counts_a_name_only_if_both_modes_match(self):
        common = (0, b"", b"", {})
        oracle_rows = {"pandoc.Str": 'function\t"copper"', "pandoc.Space": "function"}
        oracle = lua.Run(oracle_rows, {}, common, False)
        changed_rows = {**oracle_rows, "pandoc.Str": 'function\t"blue"'}
        changed = lua.Run(changed_rows, {}, common, False)
        references = {"filter": oracle, "standalone": oracle}
        candidates = {"filter": oracle, "standalone": changed}
        units = [lua.compare_name(references, candidates, name) for name in oracle_rows]
        counts = self.parse({"names": list(oracle_rows), "units": units})
        self.assertEqual(counts.rows, {"lua.api": (1, 2)})
        self.assertEqual(counts.failing, ["pandoc.Str"])
        self.assertEqual(counts.passing, ["pandoc.Space"])

    def test_a_limit_keeps_the_name_in_total(self):
        oracle = lua.Run({"pandoc.Str": "function"}, {}, (0, b"", b"", {}), False)
        stopped = lua.Run({}, {}, (137, b"", b"", {}), True)
        unit = lua.compare_name({"filter": oracle, "standalone": oracle},
                                {"filter": stopped, "standalone": oracle}, "pandoc.Str")
        counts = self.parse({"names": ["pandoc.Str"], "units": [unit]})
        self.assertEqual(counts.rows, {"lua.api": (0, 1)})
        self.assertEqual(counts.no_result, ["pandoc.Str"])

    def test_side_effects_are_compared_for_the_name_that_produced_them(self):
        rows = b'lua-api\tprint\tfunction\nlua-api\tpandoc.Str\tfunction\n'
        message = b"lua-api-effect print copper\n"
        expected, expected_effects, expected_other = lua.read_rows(message, rows)
        actual, actual_effects, actual_other = lua.read_rows(b"", rows + message)
        oracle = lua.Run(expected, expected_effects, (0, *expected_other, {}), False)
        candidate = lua.Run(actual, actual_effects, (0, *actual_other, {}), False)
        self.assertEqual(lua.compare_mode(oracle, candidate, "print"), "fail")
        self.assertEqual(lua.compare_mode(oracle, candidate, "pandoc.Str"), "pass")

    def test_output_during_a_probe_only_changes_that_probe(self):
        rows = b'lua-api\tpandoc.cli.repl\tfunction\nlua-api\tpandoc.Str\tfunction\n'
        frame = (b"\x1eLuaProbe\tbegin\tpandoc.cli.repl\x1f"
                 b"Interactive prompt>\x1eLuaProbe\tend\tpandoc.cli.repl\x1f")
        expected, expected_effects, expected_other = lua.read_rows(frame, rows)
        actual, actual_effects, actual_other = lua.read_rows(b"", rows)
        oracle = lua.Run(expected, expected_effects, (0, *expected_other, {}), False)
        candidate = lua.Run(actual, actual_effects, (0, *actual_other, {}), False)
        self.assertEqual(lua.compare_mode(oracle, candidate, "pandoc.cli.repl"), "fail")
        self.assertEqual(lua.compare_mode(oracle, candidate, "pandoc.Str"), "pass")

    @unittest.skipUnless(os.environ.get("RANDOC_ORACLE_PANDOC"), "Pandoc 3.13 is required")
    def test_a_no_op_bold_function_fails_the_styling_probe(self):
        probe = source.with_name("lua_probe.lua").read_bytes()
        setup = (b'local names = {"pandoc.layout.bold", "pandoc.layout.literal"}\n'
                 b"local standalone = false\nlocal discover = false\n")
        command = ["-f", "markdown", "-t", "plain", "-L", "/dev/stdin", "/dev/null"]
        observed = []
        for override in (b"", b"pandoc.layout.bold = function(doc) return doc end\n"):
            result = run(os.environ["RANDOC_ORACLE_PANDOC"], command, setup + override + probe,
                         oracle=True)
            self.assertEqual(result.exit, 0, result.stderr)
            rows, effects, other = lua.read_rows(result.stdout, result.stderr)
            observed.append(lua.Run(rows, effects, (result.exit, *other, result.files), False))
        self.assertEqual(lua.compare_mode(*observed, "pandoc.layout.bold"), "fail")
        self.assertEqual(lua.compare_mode(*observed, "pandoc.layout.literal"), "pass")

    def test_repeated_rows_and_missing_oracle_names_fail(self):
        row = b"lua-api\tpandoc.Str\tfunction\n"
        with self.assertRaisesRegex(ValueError, "repeated Lua probe name"):
            lua.read_rows(b"", row * 2)
        oracle = lua.Run({"pandoc.Str": "function"}, {}, (0, b"", b"", {}), False)
        with self.assertRaisesRegex(ValueError, "incomplete oracle probes"):
            lua.require_oracle(oracle, {"pandoc.Str", "pandoc.Space"}, "filter")
        with self.assertRaisesRegex(measure.ToolError, "one result for each selected name"):
            self.parse({"names": ["pandoc.Str"], "units": []})

    def test_an_oracle_difference_stops_the_measurement(self):
        common = (0, b"", b"", {})
        before = lua.Run({"pandoc.Str": 'function\t"copper"'}, {}, common, False)
        after = lua.Run({"pandoc.Str": 'function\t"blue"'}, {}, common, False)
        with self.assertRaisesRegex(ValueError, r"oracle runs differ.*pandoc\.Str"):
            lua.require_stable(before, after, "filter")

    def parse(self, report):
        with tempfile.TemporaryDirectory() as folder:
            evidence = Path(folder)
            (evidence / lua.REPORT).write_text(json.dumps(report), encoding="utf-8")
            return lua.make_runner(measure).parse(evidence, "")


if __name__ == "__main__":
    unittest.main()
