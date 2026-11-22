import errno
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


AUTOBUILD = Path(__file__).resolve().parents[1]
SERVER = AUTOBUILD.parent / "server"
sys.path.insert(0, str(SERVER))
try:
    import cases
    import server
    import transport
finally:
    sys.path.remove(str(SERVER))

SPEC = importlib.util.spec_from_file_location("server_suite", AUTOBUILD / "suites/server.py")
suite = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(suite)


class ServerSuiteTests(unittest.TestCase):
    def test_listener_retries_temporary_connection_errors(self):
        process = SimpleNamespace(poll=lambda: None)
        with patch.object(transport.socket, "create_connection",
                          side_effect=[TimeoutError(), OSError(errno.EADDRNOTAVAIL, "address unavailable"),
                                       io.BytesIO()]) as connect, \
             patch.object(transport.time, "sleep"):
            transport.wait_for_listener(process, 12345)
        self.assertEqual(connect.call_count, 3)

    def test_http_batch_reuses_the_connection(self):
        requests = cases.fixed_requests()[:2]
        with patch.object(transport, "local_port", return_value=12345), \
             patch.object(transport.subprocess, "Popen"), \
             patch.object(transport, "wait_for_listener"), \
             patch.object(transport, "stop"), \
             patch.object(transport.http.client, "HTTPConnection") as connection, \
             patch.object(transport, "http_response", side_effect=[{"status": 200}, {"status": 400}]) as response:
            results = transport.http_run(Path("pandoc"), requests)
        self.assertEqual(connection.call_count, 1)
        self.assertEqual([call.args for call in response.call_args_list],
                         [(connection.return_value, request) for request in requests])
        self.assertEqual(results, {requests[0].id: {"status": 200}, requests[1].id: {"status": 400}})

    def test_requests_keep_harness_overrides_and_put_resources_in_the_body(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            cases_dir = root / "conformance/cases/own"
            cases_dir.mkdir(parents=True)
            (cases_dir / "test.toml").write_text(
                'from = "markdown-smart"\nto = ["html"]\nargs = ["--wrap=none"]\n'
                '[[case]]\nid = "table"\ninput = "Grid"\nto = ["latex", "html"]\n'
                'args = ["-s", "--columns", "40", "--metadata=title:Ferry"]\n'
                '[case.files]\n"notes.txt" = "reef"\n'
                '[[case]]\nid = "external"\nargs = ["--lua-filter", "filter.lua"]\n'
                '[[case]]\nid = "command"\ncmd = ["--version"]\n',
                encoding="utf-8",
            )
            requests, skipped = cases.conformance_requests(root)
        self.assertEqual([request.id for request in requests], [
            "e2e/own/test/table#e2e:latex/json", "e2e/own/test/table#e2e:latex/text",
            "e2e/own/test/table#e2e:html/json", "e2e/own/test/table#e2e:html/text",
        ])
        self.assertEqual(json.loads(requests[0].content), {
            "text": "Grid", "from": "markdown-smart", "to": "latex", "standalone": True,
            "columns": 40, "metadata": {"title": "Ferry"}, "files": {"notes.txt": "cmVlZg=="},
        })
        self.assertEqual(skipped, {"own/test/external#e2e:html": "no request field for --lua-filter"})

    def test_writer_flags_select_math_and_heading_styles(self):
        for method in ("gladtex", "katex", "mathjax", "mathml", "webtex"):
            with self.subTest(method=method):
                self.assertEqual(cases.options([f"--{method}"]), {"html-math-method": method})
        self.assertEqual(cases.options(["--mathjax", "--math-method=plain"]),
                         {"html-math-method": "plain"})
        self.assertEqual(cases.options(["--markdown-headings=setext"]), {"setext-headers": True})
        self.assertEqual(cases.options(["--markdown-headings", "atx"]), {"setext-headers": False})

    def test_batch_and_timeout_requests_have_the_required_inputs(self):
        requests = {request.id: request for request in cases.fixed_requests()}
        self.assertEqual(len(json.loads(requests["batch"].content)), 10)
        self.assertEqual(requests["option/timeout"].timeout, 1)
        self.assertEqual(requests["version"].method, "GET")

    def test_cgi_result_contains_status_type_and_body_digest(self):
        body = b"\x00report\r\n"
        cgi = io.BytesIO(b"Status: 406 Not Acceptable\r\nContent-Type: text/plain\r\n\r\n" + body)
        self.assertEqual(transport.cgi_response(cgi), {
            "status": 406, "content_type": "text/plain",
            "body": transport.hash_body(io.BytesIO(body)),
        })
        for response in (b"<p>ordinary CLI output</p>\n", b"Content-Type: text/plain\n"):
            with self.subTest(response=response), self.assertRaises(transport.NoResult):
                transport.cgi_response(io.BytesIO(response))

    def test_error_responses_count_and_non_results_do_not_pass(self):
        error = {"status": 400, "content_type": "text/plain", "body": "body"}
        other = {**error, "body": "other"}
        missing = transport.missing("timeout")
        self.assertEqual(server.verdict("unit", error, error, error)["status"], "pass")
        self.assertEqual(server.verdict("unit", error, error, other)["different"], ["body"])
        self.assertEqual(server.verdict("unit", error, error, missing)["status"], "no_result")
        self.assertEqual(server.verdict("unit", missing, error, error)["status"], "oracle_limit")
        unstable = server.verdict("unit", error, other, other)
        self.assertEqual(unstable["status"], "unstable")
        self.assertTrue(unstable["unstable_parts_ok"])

    def test_requests_with_no_result_are_retried_separately(self):
        requests = [SimpleNamespace(id=key, timeout=None) for key in ("first", "second")]
        args = SimpleNamespace(oracle="oracle", randoc="randoc")
        response = {"status": 200, "content_type": "text/plain", "body": "body"}

        for mode in ("http", "cgi"):
            with self.subTest(mode=mode), patch.object(server.time, "sleep"):
                calls = []

                def run(program, batch, *, workers=None):
                    calls.append((program, [request.id for request in batch]))
                    result = response
                    if program == args.randoc and len(batch) > 1:
                        result = transport.missing("timeout")
                    return {request.id: result for request in batch}

                rows = list(server.request_results(mode, run, args, requests))
                self.assertEqual(calls, [
                    ("oracle", ["first", "second"]),
                    ("oracle", ["first", "second"]),
                    ("randoc", ["first", "second"]),
                    ("randoc", ["first"]),
                    ("randoc", ["second"]),
                ])
                self.assertEqual([row["status"] for row in rows], ["pass", "pass"])

    def test_full_report_requires_conversion_requests_in_both_modes_and_types(self):
        measure = SimpleNamespace(ToolError=ValueError)
        required = {f"{mode}/{unit}" for mode in suite.MODES for unit in suite.FIXED}
        required.update(("cli/--help", "cli/--version"))
        with self.assertRaisesRegex(ValueError, "has no conversion requests"):
            suite.check_coverage(measure, required)
        units = {f"{mode}/e2e/demo/row#e2e:html/{kind}"
                 for mode in suite.MODES for kind in ("text", "json")}
        suite.check_coverage(measure, required | units)
        units.remove("cgi/e2e/demo/row#e2e:html/text")
        with self.assertRaisesRegex(ValueError, "missing conversion unit cgi/"):
            suite.check_coverage(measure, required | units)


if __name__ == "__main__":
    unittest.main()
