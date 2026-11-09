#!/usr/bin/env python3
"""Runs the Pandoc test suite against a program with the Pandoc command line.

See docs/PANDOC_TESTSUITE.md.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from collections import Counter
from concurrent.futures import ThreadPoolExecutor

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import suite as testsuite  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
EXCLUSIONS = os.path.join(HERE, "exclusions.json")
CLEANUP_SECONDS = 5.0


class Runner:
    """Runs the program as `pandoc` in the suite directory with a fixed environment."""

    def __init__(self, program: str, suite: str, timeout: float, bin_dir: str):
        self.suite = suite
        self.timeout = timeout
        self.pandoc = os.path.join(bin_dir, "pandoc")
        os.symlink(os.path.abspath(program), self.pandoc)
        # Tests/Helpers.hs sets TMP and HOME to the test directory. TZ and
        # LC_ALL come from the conformance harness. SOURCE_DATE_EPOCH is
        # absent, because it adds a PDF trailer ID to LaTeX output.
        self.env = {
            "PATH": f"{bin_dir}:/usr/bin:/bin:/usr/sbin:/sbin",
            "TMP": ".",
            "HOME": "./",
            "LANG": "en_US.UTF-8",
            "LC_ALL": "en_US.UTF-8",
            "TZ": "UTC",
        }

    def execute(self, command, stdin: bytes) -> testsuite.Outcome:
        if isinstance(command, str):
            argv = ["/bin/sh", "-c", command]
        else:
            argv = [self.pandoc, *command]
        # A new session puts the program and all its children in one process
        # group, so that a timeout can stop all of them.
        process = subprocess.Popen(
            argv,
            cwd=self.suite,
            env=self.env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
        try:
            out, err = process.communicate(stdin, timeout=self.timeout)
        except subprocess.TimeoutExpired:
            return self.stop(process)
        return testsuite.Outcome(process.returncode, out, err)

    def stop(self, process) -> testsuite.Outcome:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        try:
            out, err = process.communicate(timeout=CLEANUP_SECONDS)
        except subprocess.TimeoutExpired:
            # A process outside the group still holds a pipe. Give up on the output.
            for pipe in (process.stdout, process.stderr):
                pipe.close()
            process.wait(timeout=CLEANUP_SECONDS)
            out, err = b"", b""
        return testsuite.Outcome(-1, out or b"", err or b"", timed_out=True)

    def check(self, case) -> tuple:
        try:
            return case.id, case.evaluate(self.execute, self.suite)
        except OSError as error:
            return case.id, f"[tool] {error}"


def load_exclusions(path: str) -> dict:
    if not os.path.exists(path):
        return {}
    with open(path, encoding="utf-8") as handle:
        return {item["id"]: item["reason"] for item in json.load(handle)["exclusions"]}


def load_baseline(path: str) -> list:
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)["passing"]


def summarize(found, results: dict, excluded: dict, program: str, text: str = "") -> dict:
    groups: dict = {}
    for case in found.cases:
        if text not in case.id:
            continue
        totals = groups.setdefault(case.group, Counter())
        if case.id in excluded:
            totals["excluded"] += 1
        elif case.id in results:
            totals["run"] += 1
            totals["pass" if results[case.id] is None else "fail"] += 1
    unsupported: dict = {}
    for item in found.unsupported:
        unsupported.setdefault(item.group, []).append({"count": item.count, "reason": item.reason})
    run = sum(t["run"] for t in groups.values())
    passed = sum(t["pass"] for t in groups.values())
    return {
        "program": program,
        "totals": {
            "run": run,
            "pass": passed,
            "fail": run - passed,
            "excluded": sum(t["excluded"] for t in groups.values()),
            "unsupported": sum(item.count for item in found.unsupported),
        },
        "groups": {
            name: {key: totals[key] for key in ("run", "pass", "fail", "excluded")}
            for name, totals in sorted(groups.items())
        },
        "unsupported": dict(sorted(unsupported.items())),
        "passing": sorted(ident for ident, failure in results.items() if failure is None),
    }


def markdown_tables(report: dict) -> str:
    """The tables of docs/PANDOC_TESTSUITE.md for one run."""
    rows = ["| Group | Tests | Run | Pass | Excluded |", "|---|---:|---:|---:|---:|"]
    for name, t in report["groups"].items():
        rows.append(f"| `{name}` | {t['run'] + t['excluded']} | {t['run']} | {t['pass']} | {t['excluded']} |")
    rows += ["", "| Group | Tests | Reason |", "|---|---:|---|"]
    for name, items in report["unsupported"].items():
        for item in items:
            rows.append(f"| `{name}` | {item['count']} | {item['reason']} |")
    return "\n".join(rows)


def write_json(path: str, value) -> None:
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(value, handle, indent=1, ensure_ascii=False)
        handle.write("\n")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pandoc", required=True, help="the program to test")
    parser.add_argument("--suite", required=True, help="the test/ directory of the Pandoc source tree")
    parser.add_argument("--out", help="write the results as JSON to this file")
    parser.add_argument("--label", help="store the results under this key of the --out file and keep the other keys")
    parser.add_argument("--filter", default="", help="run only the tests whose identifier contains this text")
    parser.add_argument("--baseline", help="fail when a test of this baseline file does not pass")
    parser.add_argument("--write-baseline", help="write the identifiers of the passing tests to this file")
    parser.add_argument("--exclusions", default=EXCLUSIONS, help="the file of excluded tests and reasons")
    parser.add_argument("--timeout", type=float, default=60.0, help="seconds for one test")
    parser.add_argument("--jobs", type=int, default=4, help="number of threads")
    parser.add_argument("--show-failures", action="store_true", help="print the difference for each failure")
    parser.add_argument("--tables", action="store_true", help="print the group tables in Markdown")
    args = parser.parse_args(argv)

    suite = os.path.abspath(args.suite)
    found = testsuite.discover(suite)
    excluded = load_exclusions(args.exclusions)
    selected = [c for c in found.cases if args.filter in c.id and c.id not in excluded]

    bin_dir = tempfile.mkdtemp(prefix="pandoc-testsuite-")
    started = time.monotonic()
    try:
        runner = Runner(args.pandoc, suite, args.timeout, bin_dir)
        with ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
            results = dict(pool.map(runner.check, selected))
    finally:
        shutil.rmtree(bin_dir, ignore_errors=True)

    report = summarize(found, results, excluded, os.path.basename(args.pandoc), args.filter)
    for ident, failure in sorted(results.items()):
        if failure is not None:
            print(f"FAIL {ident}")
            if args.show_failures:
                print("    " + failure.replace("\n", "\n    "))
    for name, totals in report["groups"].items():
        print(f"{name}: {totals['pass']}/{totals['run']} pass, {totals['excluded']} excluded")
    totals = report["totals"]
    print(
        f"total: {totals['pass']}/{totals['run']} pass, {totals['excluded']} excluded, "
        f"{totals['unsupported']} unsupported ({time.monotonic() - started:.0f} s)"
    )

    if args.tables:
        print(markdown_tables(report))
    if args.out:
        if args.label:
            stored = {}
            if os.path.exists(args.out):
                with open(args.out, encoding="utf-8") as handle:
                    stored = json.load(handle)
            stored[args.label] = report
            write_json(args.out, dict(sorted(stored.items())))
        else:
            write_json(args.out, report)
    if args.write_baseline:
        write_json(args.write_baseline, {"program": report["program"], "passing": report["passing"]})

    if args.baseline:
        missing = [i for i in load_baseline(args.baseline) if args.filter in i and i not in excluded and results.get(i, "") is not None]
        for ident in missing:
            print(f"REGRESSION {ident}")
        if missing:
            print(f"{len(missing)} tests of the baseline do not pass")
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
