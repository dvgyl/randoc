"""Check the input and comparison rules of the AST suite."""

import hashlib
import json
import signal
import subprocess
import sys
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing, redirect_stdout
from io import StringIO
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

TOOLS = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(TOOLS / "ast"))
import roundtrip
import capture as conversion
import sources
from observe import NoResult, Observation, run


class AstSuiteTests(unittest.TestCase):
    def test_defaults_command_uses_the_standard_library_ast_module(self):
        command = TOOLS / "measure" / "suites" / "defaults.py"
        result = subprocess.run([sys.executable, "-E", "-B", str(command), "--help"],
                                cwd=TOOLS.parent, capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--harness", result.stdout)

    def test_exact_bytes_determine_distinct_trees(self):
        first, second = b'{"blocks":[]}', b'{"blocks": []}'
        with redirect_stdout(StringIO()):
            trees, problems = roundtrip.collect((
                [("pivot/a", first), ("reader/b", first), ("reader/c", second),
                 ("reader/d", "no oracle result")],
            ))
        digest = hashlib.sha256(first).hexdigest()
        self.assertEqual(set(trees), {digest, hashlib.sha256(second).hexdigest()})
        self.assertEqual(trees[digest].sources, ["pivot/a", "reader/b"])
        self.assertEqual(problems, {"reader/d": "no oracle result"})

    def test_version_cases_cover_each_form_with_distinct_bytes(self):
        cases = dict(roundtrip.version_cases())
        self.assertEqual(len(cases), 10)
        self.assertEqual(len(set(cases.values())), 10)
        self.assertNotIn("pandoc-api-version", json.loads(cases["version/missing"]))
        self.assertIsNone(json.loads(cases["version/null"])["pandoc-api-version"])

    def check_tree(self, json_result, native_result, read_result, actual_json, actual_native):
        data = b'{"meta":{},"blocks":[]}'
        results = {
            ("json", "json"): (json_result, actual_json),
            ("json", "native"): (native_result, actual_native),
            ("native", "json"): (read_result, read_result),
        }
        native_inputs = []

        def capture(binary, command, unit, oracle=False):
            input_format, target = command[1], command[3]
            if input_format == "native":
                native_inputs.append((binary, unit.data))
            return results[input_format, target][not oracle]

        tree = roundtrip.Tree(data, ["version/example"])
        args = SimpleNamespace(oracle="oracle", randoc="randoc", verify_oracle=False)
        with closing(roundtrip.open_cache(Path(":memory:"))) as database, ThreadPoolExecutor(2) as pool, \
                patch("roundtrip.time.sleep"), redirect_stdout(StringIO()):
            [row] = roundtrip.compare_trees(args, {"document": tree}, database, b"test", (pool, pool),
                                            capture, None)
        self.assertEqual(native_inputs, [("oracle", native_result.stdout)] * 2
                         + [("randoc", native_result.stdout)])
        return row

    def test_roundtrip_uses_bytes_and_oracle_native_input(self):
        expected = Observation(0, b'{"blocks":[]}', b"", {})
        different_json = Observation(0, b'{"blocks": []}', b"", {})
        native = Observation(0, b"[HorizontalRule]", b"", {})
        different_native = Observation(0, b"[ ]", b"", {})
        row = self.check_tree(expected, native, expected, different_json, different_native)
        self.assertEqual(row["status"], "fail")
        self.assertEqual(row["commands"]["json-to-json"]["differences"], ["stdout"])
        self.assertEqual(row["commands"]["json-to-native"]["differences"], ["stdout"])
        self.assertEqual(row["commands"]["native-to-json"]["status"], "pass")

    def test_ordinary_oracle_errors_stay_in_the_measurement(self):
        error = Observation(64, b"", b"invalid input\n", {})
        result = Observation(0, b'{"blocks":[]}', b"", {})
        row = self.check_tree(error, error, result, error, error)
        self.assertEqual(row["status"], "pass")
        self.assertEqual(len(row["commands"]), 3)


@unittest.skipUnless(sys.platform == "darwin", "The measurement uses macOS process limits.")
class CaptureTests(unittest.TestCase):
    def test_capture_matches_the_writer_runner(self):
        data = b"sample input\n" * 16_384
        command = ["-c", 'printf note >&2\nprintf file > written\n/bin/cat\nexit 4']
        expected = run("/bin/sh", command, data)
        actual = conversion.Capture().run("/bin/sh", command, data, oracle=False)
        self.assertEqual(actual, expected)
        self.assertEqual(actual.stdout, data)
        self.assertEqual(actual.stderr, b"note")
        self.assertEqual(actual.exit, 4)
        self.assertEqual(set(actual.files), {"written"})

    def test_capture_stops_a_conversion_at_its_wall_limit(self):
        with self.assertRaisesRegex(NoResult, "wall time limit"):
            conversion.Capture().run("/bin/sh", ["-c", "/bin/sleep 30"], b"",
                                     oracle=False, wall_seconds=0.1)

    def test_capture_stops_a_conversion_at_its_memory_limit(self):
        with patch.object(conversion, "MEMORY_BYTES", 0):
            with self.assertRaisesRegex(NoResult, "memory limit"):
                conversion.Capture().run("/bin/sleep", ["30"], b"", oracle=True)

    def test_capture_enforces_cpu_time_limit(self):
        launch = conversion.LAUNCH.replace("ulimit -t 60", "ulimit -t 1", 1)
        signals = f"{-signal.SIGXCPU}|{-signal.SIGKILL}"
        with patch.object(conversion, "LAUNCH", launch):
            with self.assertRaisesRegex(NoResult, f"exit code ({signals})$"):
                conversion.Capture().run("/bin/sh", ["-c", "while :; do :; done"], b"",
                                         oracle=True, wall_seconds=30)

    def oracle_script(self, folder, script):
        oracle = Path(folder) / "oracle"
        oracle.write_text("#!/bin/sh\n" + script, encoding="utf-8")
        oracle.chmod(0o700)
        return oracle

    def cpu_probe(self, folder):
        return self.oracle_script(folder, 'ulimit -t\nif [ "$#" -eq 5 ]; then\n'
                                  '    /bin/cat "$5"\nelse\n    /bin/cat\nfi\n')

    def test_commonmark_inputs_have_the_cpu_limit(self):
        tool = sources.load_tool("ast_commonmark", "tools/commonmark-spec/run.py")
        data = "CommonMark sample\n"
        with tempfile.TemporaryDirectory(dir=roundtrip.ROOT) as folder, ThreadPoolExecutor(1) as pool:
            spec_dir = Path(folder) / "commonmark-spec"
            spec_dir.mkdir()
            (spec_dir / tool.SPEC_FILE).write_text(
                f"{tool.FENCE} example\n{data}.\n<p>Sample</p>\n{tool.FENCE}\n", encoding="utf-8")
            args = SimpleNamespace(oracle=self.cpu_probe(folder))
            with patch.dict(sources.os.environ, RANDOC_ENV=folder), patch.object(
                    sources, "load_tool", return_value=tool), patch.object(tool, "check_hash"):
                rows = list(sources.commonmark_trees(args, pool, conversion.Capture()))
        self.assertEqual(rows, [("commonmark/1", b"60\n" + data.encode())])

    def test_pandoc_inputs_and_extension_lookup_have_the_cpu_limit(self):
        tool = sources.load_tool("ast_pandoc_suite", "tools/pandoc-testsuite/run.py")
        case = tool.testsuite.AstCase("sample", "reader", "markdown", "input.md", "native",
                                      "expected.native", frozenset({"probe"}))
        with tempfile.TemporaryDirectory(dir=roundtrip.ROOT) as folder, ThreadPoolExecutor(1) as pool:
            location = Path(folder)
            (location / case.input_path).write_bytes(b"Reader input\n")
            (location / case.expected_path).write_bytes(b"Expected tree\n")
            oracle = self.oracle_script(folder, 'if [ "$#" -eq 1 ]; then\n'
                                        '    [ "$(ulimit -t)" = 60 ] || exit 1\n'
                                        "    printf '+default\\n'\n"
                                        'else\n    ulimit -t\n    printf "%s\\n" "$2"\n'
                                        '    /bin/cat "$5"\nfi\n')
            args = SimpleNamespace(oracle=oracle, pandoc_tests=location)
            with patch.object(sources, "load_tool", return_value=tool), patch.object(
                    tool.testsuite, "discover", return_value=SimpleNamespace(cases=[case])), patch.dict(
                    tool.testsuite._EXTENSION_CACHE, clear=True):
                rows = list(sources.suite_trees(args, pool, conversion.Capture()))
        self.assertEqual(rows, [
            ("pandoc/sample/expected", b"60\nnative\nExpected tree\n"),
            ("pandoc/sample/input", b"60\nmarkdown-default+probe\nReader input\n"),
        ])

    def test_uncached_corpus_input_has_the_cpu_limit(self):
        data = b"Visible corpus sample\n"
        document = sources.corpuslib.Document("sample", "markdown", "", "", "",
                                              hashlib.sha256(data).hexdigest(), len(data))
        with tempfile.TemporaryDirectory(dir=roundtrip.ROOT) as folder:
            corpus = Path(folder)
            (corpus / document.file_name).write_bytes(data)
            args = SimpleNamespace(oracle=self.cpu_probe(folder), corpus=corpus)
            with patch.object(sources.corpuslib, "load_manifest", return_value=[document]):
                rows = list(sources.corpus_trees(args, "probe", conversion.Capture()))
        self.assertEqual(rows, [("corpus/sample", b"60\n" + data)])

    def test_generated_inputs_have_the_cpu_limit(self):
        data = b"Generated sample\n"
        command = ["-f", "markdown", "-t", "json", "input.md"]
        record = {"args": command, "id": "sample", "input": "input.md"}

        def generate(seed, _count, _extensions, oracle_json):
            if seed != 1:
                return {}
            self.assertEqual(oracle_json("markdown", data), b"60\n" + data)
            return {"sample.case.json": json.dumps(record), "input.md": data}

        with tempfile.TemporaryDirectory(dir=roundtrip.ROOT) as folder, ThreadPoolExecutor(1) as pool:
            args = SimpleNamespace(oracle=self.cpu_probe(folder))
            with patch.object(sources.casegen, "generate", side_effect=generate):
                rows = list(sources.generated_trees(args, pool, conversion.Capture()))
        self.assertEqual(rows, [("casegen/1/sample", b"60\n" + data)])
