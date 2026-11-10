"""Tests of run.py and make_slow.py. Run: python3 -m unittest discover -s tools/bench"""

import collections
import contextlib
import fractions
import io
import itertools
import json
import math
import os
import pathlib
import random
import signal
import subprocess
import tempfile
import time
import unittest

import make_slow
import run

FAKE_PROGRAM = """#!/bin/sh
case "$2" in
  nosuch) echo "Unknown input format 'nosuch'" >&2; exit 21 ;;
  broken) echo "failed" >&2; exit 3 ;;
esac
cat
"""


class TempDirTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="randoc-bench-test-")
        self.dir = pathlib.Path(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()

    def write(self, name, text, mode=None):
        path = self.dir / name
        path.write_text(text, encoding="utf-8")
        if mode:
            path.chmod(mode)
        return path


class ParseTest(unittest.TestCase):
    BASE = pathlib.Path("/bench")

    def test_series_expands_to_one_benchmark_for_each_scale(self):
        text = """
[[benchmark]]
name = "version"
args = ["--version"]

[[series]]
name = "conv"
args = ["-f", "json", "-t", "json"]
input = "inputs/doc-x{scale}.json"
scales = [1, 10]
startup_stdin = "{}"
"""
        benches = run.parse_benchmarks(text, self.BASE)
        self.assertEqual([b.name for b in benches], ["version", "conv-startup", "conv-x1", "conv-x10"])
        startup = benches[1]
        self.assertEqual((startup.stdin, startup.input, startup.args), ("{}", None, ("-f", "json", "-t", "json")))
        self.assertEqual(benches[3].input, self.BASE / "inputs/doc-x10.json")
        self.assertEqual((benches[3].series, benches[3].scale, benches[3].startup), ("conv", 10, "conv-startup"))
        self.assertEqual(benches[2].args, ("-f", "json", "-t", "json"))

    def test_invalid_files_are_rejected(self):
        cases = {
            "both inputs": '[[benchmark]]\nname="a"\nargs=[]\ninput="x"\nstdin="y"\n',
            "unknown key": '[[benchmark]]\nname="a"\nargs=[]\nfoo=1\n',
            "duplicate": '[[benchmark]]\nname="a"\nargs=[]\n[[benchmark]]\nname="a"\nargs=[]\n',
            "args type": '[[benchmark]]\nname="a"\nargs="x"\n',
            "no scale in pattern": '[[series]]\nname="s"\nargs=[]\ninput="x"\nscales=[1]\n',
            "bad scales": '[[series]]\nname="s"\nargs=[]\ninput="x{scale}"\nscales=[0]\n',
            "missing startup_stdin": '[[series]]\nname="s"\nargs=[]\ninput="x{scale}"\nscales=[1]\n',
            "old startup key": '[[series]]\nname="s"\nargs=[]\ninput="x{scale}"\nscales=[1]\nstartup_stdin=""\nstartup="a"\n',
            "unknown table": '[[bench]]\nname="a"\n',
            "series name clash": '[[benchmark]]\nname="s-x1"\nargs=[]\n'
            '[[series]]\nname="s"\nargs=[]\ninput="x{scale}"\nscales=[1]\nstartup_stdin=""\n',
        }
        for label, text in cases.items():
            with self.subTest(label), self.assertRaises(run.ConfigError):
                run.parse_benchmarks(text, self.BASE)

    def test_filter_keeps_the_startup_benchmark(self):
        benches = [
            run.Benchmark("empty", ()),
            run.Benchmark("conv-x1", (), startup="empty"),
            run.Benchmark("other", ()),
        ]
        self.assertEqual([b.name for b in run.select(benches, "conv")], ["empty", "conv-x1"])
        self.assertEqual(len(run.select(benches, "")), 3)
        self.assertEqual(run.select(benches, "none"), [])

    def test_repository_benchmarks_file_is_valid_and_inputs_exist(self):
        benches = run.load_benchmarks(run.DEFAULT_BENCHMARKS)
        self.assertIn("json-to-json-x100", [b.name for b in benches])
        for bench in benches:
            if bench.input is not None:
                self.assertTrue(bench.input.is_file(), bench.input)

    def test_each_series_and_its_startup_benchmark_have_equal_arguments(self):
        benches = run.load_benchmarks(run.DEFAULT_BENCHMARKS)
        by_name = {b.name: b for b in benches}
        series = [b for b in benches if b.series]
        self.assertTrue(series)
        for bench in series:
            self.assertEqual(bench.args, by_name[bench.startup].args, bench.name)


class StatisticsTest(unittest.TestCase):
    def test_sign_cutoff_uses_exact_binomial_tails(self):
        # n = 8: P(B <= 0) = 1/256 <= 0.01 < P(B <= 1) = 9/256.
        self.assertEqual(run.sign_cutoff(8, 0.01), 1)
        # n = 6: P(B <= 0) = 1/64 > 0.01, so no bound exists.
        self.assertEqual(run.sign_cutoff(6, 0.01), 0)
        # n = 10, alpha 0.05: P(B <= 1) = 11/1024, P(B <= 2) = 56/1024.
        self.assertEqual(run.sign_cutoff(10, 0.05), 2)
        # n = 20, alpha 0.01: P(B <= 4) = 6196/2^20, P(B <= 5) = 21700/2^20.
        self.assertEqual(run.sign_cutoff(20, 0.01), 5)

    def test_sign_bounds_support_1024_pairs(self):
        # Codex R3-01: alpha * 2**n overflowed a float at 1024 pairs.
        self.assertEqual(run.sign_cutoff(1024, 0.01), 475)
        self.assertEqual(run.paired_bounds([1.0] * 1024, [1.0] * 1024), (1.0, 1.0, 1.0))

    def test_sign_cutoff_is_exact_for_1920_pairs(self):
        n, alpha = 1920, fractions.Fraction(1, 100)
        k = run.sign_cutoff(n, 0.01)
        below = sum(math.comb(n, i) for i in range(k))
        self.assertLessEqual(fractions.Fraction(below, 2**n), fractions.Fraction(*(0.01).as_integer_ratio()))
        self.assertGreater(fractions.Fraction(below + math.comb(n, k), 2**n), alpha)
        row = run.verdict([1.0] * n, [1.0] * n)
        self.assertEqual((row["ratio_lower"], row["ratio_upper"], row["verdict"]), (1.0, 1.0, "same"))

    def test_cpu_time_of_zero_does_not_stop_the_tool(self):
        # Opus R2-2.
        row = run.verdict([0.0] + [1.0] * 9, [1.0] * 10)
        self.assertIn(row["verdict"], ("same", "inconclusive", "slower", "faster"))
        self.assertEqual(run.verdict([0.0] * 10, [0.0] * 10)["ratio_min"], 1.0)

    def test_timed_out_needs_a_kill_by_the_limit(self):
        # Opus R2-3: a child that exits just before the timer is not a time-out.
        self.assertTrue(run.timed_out(True, signal.SIGKILL))
        self.assertFalse(run.timed_out(True, 0))
        self.assertFalse(run.timed_out(False, signal.SIGKILL))
        self.assertFalse(run.timed_out(True, 3 << 8))

    def test_bounds_keep_the_confidence_for_an_asymmetric_distribution(self):
        # Codex R2-02: candidate 1.06 with probability 0.708, else 0.80. The median ratio is 1.06.
        for n in (30, 60, 120):
            false_pass = sum(
                math.comb(n, high) * 0.708**high * 0.292 ** (n - high)
                for high in range(n + 1)
                if run.verdict([1.0] * n, [1.06] * high + [0.8] * (n - high))["verdict"] in ("same", "faster")
            )
            with self.subTest(n=n):
                self.assertLessEqual(false_pass, run.ALPHA)

    def test_declared_split_keeps_the_allowance_over_tries(self):
        # Codex R2-01: log ratios symmetric around ln(1.06), retried only while inconclusive.
        for tries in (1, 3, 10):
            counts = collections.Counter(
                run.verdict(
                    [1.0] * 8, [0.9 if s < 0 else 1.06**2 / 0.9 for s in signs], alpha=run.ALPHA / tries
                )["verdict"]
                for signs in itertools.product((-1, 1), repeat=8)
            )
            p = (counts["same"] + counts["faster"]) / 256
            q = counts["slower"] / 256
            false_pass = p * (1 - (1 - p - q) ** tries) / (p + q) if p + q else 0.0
            with self.subTest(tries=tries):
                self.assertLessEqual(false_pass, run.ALPHA)

    def test_paired_bounds(self):
        base = [1.0 + 0.05 * i for i in range(30)]
        lower, estimate, upper = run.paired_bounds(base, [1.1 * v for v in base])
        self.assertAlmostEqual((lower, estimate, upper), (1.1, 1.1, 1.1))
        lower, _, upper = run.paired_bounds(base[:6], base[:6])
        self.assertEqual((lower, upper), (0.0, float("inf")))

    def test_verdict_finds_a_clear_slowdown(self):
        base = [1.0 + 0.01 * i for i in range(20)]
        row = run.verdict(base, [v * 1.2 for v in base])
        self.assertEqual(row["verdict"], "slower")
        self.assertAlmostEqual(row["ratio_min"], 1.2)
        self.assertAlmostEqual(row["ratio_lower"], 1.2)

    def test_verdict_passes_only_a_proven_small_ratio(self):
        base = [1.0 + 0.01 * i for i in range(20)]
        self.assertEqual(run.verdict(base, [v * 1.04 for v in base])["verdict"], "same")
        self.assertEqual(run.verdict(base, list(base))["verdict"], "same")
        self.assertEqual(run.verdict(base, [v * 0.8 for v in base])["verdict"], "faster")

    def test_unproven_slowdown_is_inconclusive_and_fails_the_gate(self):
        # The minimum is 6% above the reference, but the pairs are noisy.
        base = [1.0 + 0.05 * i for i in range(30)]
        candidate = [1.06 * v for v in reversed(base)]
        row = run.verdict(base, candidate)
        self.assertAlmostEqual(row["ratio_min"], 1.06)
        self.assertEqual(row["verdict"], "inconclusive")
        self.assertEqual(run.exit_code({}, [dict(row, name="b")]), 3)

    def test_ten_percent_slowdown_with_realistic_noise_is_not_a_pass(self):
        data = json.loads((run.ROOT / "benchmarks/results/main.json").read_text(encoding="utf-8"))
        pool = [s["cpu"] for s in data["results"]["program"]["json-to-json-x100"]["samples"]]
        rng = random.Random(1)
        passes = 0
        for _ in range(200):
            base = rng.choices(pool, k=30)
            cand = [x * 1.10 for x in rng.choices(pool, k=30)]
            passes += run.verdict(base, cand)["verdict"] == "same"
        # A 10% slowdown must not pass the 5% gate in most trials.
        self.assertLess(passes / 200, 0.2)

    def test_resampled_detection_rates(self):
        data = json.loads((run.ROOT / "benchmarks/results/main.json").read_text(encoding="utf-8"))
        pool = [s["cpu"] for s in data["results"]["program"]["json-to-json-x100"]["samples"]]
        rng = random.Random(2)
        for factor, verdict, most in ((1.0, "slower", 0.02), (1.10, "same", 0.0), (1.30, "same", 0.0)):
            hits = sum(
                run.verdict(rng.choices(pool, k=60), [x * factor for x in rng.choices(pool, k=60)])[
                    "verdict"
                ]
                == verdict
                for _ in range(100)
            )
            with self.subTest(factor=factor, verdict=verdict):
                self.assertLessEqual(hits / 100, most)

    def test_lost_support_fails_the_gate(self):
        benches = [run.Benchmark("b", ())]
        ok = {"status": "ok", "samples": [{"cpu": 1.0}] * 8, "summary": {"cpu": {"min": 1.0}}, "stdout_sha256": "x"}
        results = {"program": {"b": {"status": "not-supported"}}, "reference": {"b": ok}}
        rows = run.compare(benches, results)
        self.assertEqual(rows[0]["verdict"], "lost-support")
        self.assertEqual(run.exit_code(results, rows), 1)
        results = {"program": {"b": ok}, "reference": {"b": {"status": "not-supported"}}}
        rows = run.compare(benches, results)
        self.assertEqual((rows[0]["verdict"], run.exit_code(results, rows)), ("new", 0))
        results = {"program": {"b": {"status": "not-supported"}}, "reference": {"b": {"status": "not-supported"}}}
        rows = run.compare(benches, results)
        self.assertEqual((rows[0]["verdict"], run.exit_code(results, rows)), ("not-supported", 0))

    def test_public_version_removes_local_folders(self):
        home = os.path.expanduser("~")
        text = f"pandoc 3.13\nUser data directory: {home}/.local/share/pandoc\nconfig {home}/x"
        self.assertEqual(run.public_version(text), ["pandoc 3.13", "config ~/x"])
        # Opus R2-4: the standard error of a probe uses the same filter.
        self.assertEqual(run.public_text(f"{run.ROOT}/a and {home}/b"), "<repo>/a and ~/b")

    def test_slope_and_conversion(self):
        self.assertAlmostEqual(run.slope([(1, 3.0), (10, 12.0), (100, 102.0)]), 1.0)
        self.assertIsNone(run.slope([(1, 3.0)]))
        benches = [
            run.Benchmark("empty", ()),
            run.Benchmark("c-x1", (), series="c", scale=1, startup="empty"),
            run.Benchmark("c-x10", (), series="c", scale=10, startup="empty"),
            run.Benchmark("u-x1", (), series="u", scale=1, startup="empty"),
        ]

        def ok(value):
            return {"status": "ok", "summary": {"cpu": {"min": value}}}

        results = {"empty": ok(2.0), "c-x1": ok(3.0), "c-x10": ok(12.0), "u-x1": {"status": "not-supported"}}
        derived = run.derive(benches, results)
        self.assertEqual(derived["conversion_cpu_seconds"], {"c-x1": 1.0, "c-x10": 10.0})
        self.assertEqual(derived["slope_cpu_seconds_per_section"], {"c": 1.0})


class ProcessTest(TempDirTest):
    def test_run_once_reports_exit_code_and_resources(self):
        program = self.write("prog", FAKE_PROGRAM, 0o755)
        stdin = self.write("in", "hello")
        out = self.dir / "out"
        code, sample = run.run_once([str(program), "-f", "json"], str(stdin), str(out))
        self.assertEqual(code, 0)
        self.assertEqual(out.read_text(encoding="utf-8"), "hello")
        self.assertGreater(sample.cpu, 0)
        self.assertAlmostEqual(sample.cpu, sample.user + sample.sys)
        self.assertGreater(sample.wall, 0)
        self.assertGreater(sample.maxrss, 2**16)

    def test_probe_classifies_exit_codes(self):
        program = str(self.write("prog", FAKE_PROGRAM, 0o755))
        stdin = str(self.write("in", "x"))
        self.assertEqual(run.probe([program, "-f", "json"], stdin, self.dir)["status"], "ok")
        unsupported = run.probe([program, "-f", "nosuch"], stdin, self.dir)
        self.assertEqual((unsupported["status"], unsupported["exit_code"]), ("not-supported", 21))
        self.assertIn("Unknown input format", unsupported["stderr"])
        self.assertEqual(run.probe([program, "-f", "broken"], stdin, self.dir)["status"], "error")
        noisy = str(self.write("noisy", '#!/bin/sh\necho "cannot read $HOME/x" >&2\nexit 3\n', 0o755))
        self.assertEqual(run.probe([noisy], stdin, self.dir)["stderr"], "cannot read ~/x\n")

    def test_time_limit_stops_the_process_group(self):
        marker = self.dir / "grandchild"
        script = self.write("hang", f'#!/bin/sh\nsleep 60 &\necho $! > "{marker}"\nsleep 60\n', 0o755)
        start = time.monotonic()
        with self.assertRaises(subprocess.TimeoutExpired):
            run.run_once([str(script)], os.devnull, timeout=0.5)
        self.assertLess(time.monotonic() - start, 10)
        grandchild = int(marker.read_text(encoding="utf-8"))
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            try:
                os.kill(grandchild, 0)
            except ProcessLookupError:
                break
            time.sleep(0.05)
        else:
            self.fail("the grandchild still runs after the time limit")
        result = run.probe(["/bin/sleep", "60"], os.devnull, self.dir, timeout=0.2)
        self.assertEqual((result["status"], result["exit_code"]), ("error", None))


class EndToEndTest(TempDirTest):
    def setUp(self):
        super().setUp()
        self.program = str(self.write("prog", FAKE_PROGRAM, 0o755))
        self.write("doc-x1.txt", "a\n")
        self.write("doc-x2.txt", "a\na\n")
        self.benchmarks = str(
            self.write(
                "benchmarks.toml",
                '[[benchmark]]\nname = "empty"\nargs = ["-f", "json"]\n'
                '[[benchmark]]\nname = "unknown"\nargs = ["-f", "nosuch"]\n'
                '[[series]]\nname = "conv"\nargs = ["-f", "json"]\ninput = "doc-x{scale}.txt"\n'
                'scales = [1, 2]\nstartup_stdin = ""\n',
            )
        )

    def tool(self, *args):
        out = self.dir / "out.json"
        stdout = io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(io.StringIO()):
            code = run.main(["--benchmarks", self.benchmarks, "--runs", "10", "--out", str(out), *args])
        return code, json.loads(out.read_text(encoding="utf-8")), stdout.getvalue()

    def test_single_program_writes_raw_data(self):
        code, data, table = self.tool("--program", self.program)
        self.assertEqual(code, 0)
        self.assertEqual(data["programs"]["program"]["name"], "prog")
        self.assertEqual(data["results"]["program"]["conv-x1"]["argv"], ["PROGRAM", "-f", "json"])
        self.assertEqual(len(data["results"]["program"]["conv-x2"]["samples"]), 10)
        self.assertEqual(data["results"]["program"]["unknown"]["status"], "not-supported")
        self.assertEqual(data["results"]["program"]["unknown"]["samples"], [])
        self.assertEqual(data["benchmarks"][3]["stdin"], {"file": "doc-x1.txt"})
        self.assertEqual(data["benchmarks"][2]["stdin"], {"text": ""})
        self.assertIn("conv", data["derived"]["program"]["slope_cpu_seconds_per_section"])
        self.assertIn("cpu_count", data["environment"])
        self.assertIsNone(data["comparison"])
        self.assertIn("not-supported", table)

    def test_record_has_no_home_or_absolute_path(self):
        _, data, _ = self.tool("--program", self.program, "--compare", self.program)
        text = json.dumps(data)
        self.assertNotIn(str(pathlib.Path.home()), text)
        self.assertNotIn(str(self.dir), text)
        self.assertNotIn(os.path.realpath(self.dir), text)

    def test_compare_with_itself_and_with_a_slower_program(self):
        code, data, _ = self.tool("--program", self.program, "--compare", self.program)
        self.assertIn(code, (0, 3))
        verdicts = {row["name"]: row["verdict"] for row in data["comparison"]}
        self.assertEqual(verdicts["unknown"], "not-supported")
        self.assertNotIn("slower", verdicts.values())

        slow = self.dir / "slow"
        make_slow.write_wrapper(slow, self.program, 20_000_000)
        code, data, table = self.tool(
            "--program", str(slow), "--compare", self.program, "--filter", "conv", "--max-runs", "40"
        )
        self.assertEqual(code, 1)
        # A decided benchmark gets no more rounds.
        self.assertEqual({r["runs"] for r in data["comparison"]}, {10})
        self.assertEqual(data["settings"]["alpha_per_look"], run.ALPHA / 3)
        self.assertEqual([r["name"] for r in data["comparison"]], ["conv-startup", "conv-x1", "conv-x2"])
        self.assertTrue(all(r["verdict"] == "slower" for r in data["comparison"]))
        self.assertTrue(all(r["same_output"] for r in data["comparison"]))
        self.assertIn("slower", table)

    def test_failing_benchmark_gives_exit_code_2(self):
        self.write("benchmarks.toml", '[[benchmark]]\nname = "bad"\nargs = ["-f", "broken"]\n')
        code, data, _ = self.tool("--program", self.program)
        self.assertEqual(code, 2)
        self.assertEqual(data["results"]["program"]["bad"]["exit_code"], 3)


class LookTest(unittest.TestCase):
    def test_look_sizes_double_up_to_the_maximum(self):
        self.assertEqual(run.look_sizes(30, 30), [30])
        self.assertEqual(run.look_sizes(30, 200), [30, 60, 120, 200])
        self.assertEqual(run.look_sizes(10, 40), [10, 20, 40])


class MakeSlowTest(unittest.TestCase):
    def test_iterations_for(self):
        per_probe = 0.01 * make_slow.PROBE_ITERATIONS
        self.assertEqual(make_slow.iterations_for(10.0, 1.0, per_probe, 1.2), 100)
        self.assertIsNone(make_slow.iterations_for(10.0, 3.0, per_probe, 1.2))
        self.assertIsNone(make_slow.iterations_for(10.0, -1.0, per_probe, 1.2))
        self.assertIsNone(make_slow.iterations_for(10.0, 1.0, 0.0, 1.2))


if __name__ == "__main__":
    unittest.main()
