#!/usr/bin/env python3
"""Writes a wrapper program that adds a fixed CPU cost to a program.

The wrapper runs a loop and then replaces itself with the program (`execv`),
so the benchmark tool measures the loop and the program in one process. It
needs a C compiler (`cc`). The calibration sets the loop so that the median
CPU time of one benchmark grows by the factor. The cost is fixed, so a
shorter benchmark grows by more and a longer benchmark by less. Use the
wrapper to check that `run.py --compare` finds a slowdown of a real program:

    python3 tools/bench/make_slow.py --program target/release/randoc \\
        --benchmark json-to-json-x100 --factor 1.2 --out "$TMPDIR/slow-randoc"
"""

import argparse
import json
import os
import pathlib
import statistics
import subprocess
import sys
import tempfile

import run

PROBE_ITERATIONS = 5_000_000


def write_wrapper(path, program, iterations):
    """Compiles a wrapper that loops `iterations` times and then runs `program`.

    A shell script costs about 5 ms of CPU time on the reference machine. That
    is more than 20% of the json-to-json-x100 benchmark. A C wrapper costs less.
    """
    source = (
        "#include <unistd.h>\n"
        "int main(int argc, char **argv) {\n"
        "    volatile unsigned long i;\n"
        f"    for (i = 0; i < {int(iterations)}UL; i++) {{}}\n"
        f"    argv[0] = {json.dumps(program)};\n"
        "    execv(argv[0], argv);\n"
        "    return 127;\n"
        "}\n"
    )
    with tempfile.TemporaryDirectory(prefix="randoc-wrapper-") as scratch:
        c_file = pathlib.Path(scratch) / "wrapper.c"
        c_file.write_text(source, encoding="utf-8")
        subprocess.run(["cc", "-O0", "-o", str(path), str(c_file)], check=True)


def calibrate(base_argv, zero_argv, probe_argv, stdin_path, runs):
    """Returns (base, wrapper cost, cost of PROBE_ITERATIONS) as medians of CPU time.

    The three commands run in interleaved rounds, and the costs are medians of
    the differences in each round, so a change of load affects each term equally.
    """
    rows = [
        [run.run_once(argv, stdin_path)[1].cpu for argv in (base_argv, zero_argv, probe_argv)]
        for _ in range(runs)
    ]
    return (
        statistics.median(r[0] for r in rows),
        statistics.median(r[1] - r[0] for r in rows),
        statistics.median(r[2] - r[1] for r in rows),
    )


def iterations_for(base, wrapper_cost, probe_cost, factor):
    """Returns the loop count that adds (factor - 1) * base of CPU time, or None.

    None means that the calibration is not usable: a cost is not positive, or
    the wrapper alone costs more than the factor permits.
    """
    if wrapper_cost <= 0 or probe_cost <= 0:
        return None
    count = ((factor - 1) * base - wrapper_cost) / (probe_cost / PROBE_ITERATIONS)
    return round(count) if count >= 0 else None


def main(argv=None):
    """Calibrates and writes the wrapper. Returns the exit code."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--program", required=True)
    parser.add_argument("--benchmark", default="json-to-json-x100")
    parser.add_argument("--factor", type=float, default=1.2)
    parser.add_argument("--runs", type=int, default=60)
    parser.add_argument("--benchmarks", default=str(run.DEFAULT_BENCHMARKS))
    parser.add_argument("--out", required=True)
    args = parser.parse_args(argv)
    program = run.resolve_program(args.program)
    if program is None:
        parser.error(f"{args.program}: not an executable program")
    found = [b for b in run.load_benchmarks(args.benchmarks) if b.name == args.benchmark]
    if not found:
        parser.error(f"no benchmark has the name {args.benchmark}")
    bench = found[0]
    out = os.path.abspath(args.out)
    with tempfile.TemporaryDirectory(prefix="randoc-slow-") as scratch:
        stdin_path = str(bench.input) if bench.input else os.path.join(scratch, "stdin")
        if bench.input is None:
            pathlib.Path(stdin_path).write_text(bench.stdin or "", encoding="utf-8")
        zero = os.path.join(scratch, "zero")
        probe = os.path.join(scratch, "probe")
        write_wrapper(zero, program, 0)
        write_wrapper(probe, program, PROBE_ITERATIONS)
        base, wrapper_cost, probe_cost = calibrate(
            [program, *bench.args], [zero, *bench.args], [probe, *bench.args], stdin_path, args.runs
        )
    count = iterations_for(base, wrapper_cost, probe_cost, args.factor)
    summary = (
        f"program {base * 1000:.2f} ms, wrapper {wrapper_cost * 1000:.2f} ms, "
        f"{PROBE_ITERATIONS} iterations {probe_cost * 1000:.2f} ms (medians)"
    )
    if count is None:
        print(f"make_slow: calibration not usable: {summary}", file=sys.stderr)
        return 1
    write_wrapper(out, program, count)
    print(f"{summary}; loop {count} iterations for the factor {args.factor}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
