#!/usr/bin/env python3
"""The gate of the build. It stops at the first step that fails.

Usage: gate.py [--quick] [--base REV]

Steps:
  format       cargo fmt --all -- --check
  licence      python3 -I tools/autobuild/licence_scan.py scan [--base REV]
  lints        cargo clippy --workspace --all-targets --quiet -- -D warnings
  docs         RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --quiet
  tests        cargo test --workspace --quiet --no-fail-fast, without the
               conformance test (the harness step runs the same units with a
               wider time limit); each failing test binary is in the log
  tool-tests   python3 -m unittest discover in each tools/ folder with tests
  harness      build randoc-cli and randoc-harness, run the harness suite of
               measure.py; each unit of spec/baseline.json must pass
  measurement  run the pandoc and corpus suites of measure.py (not with
               --quick); no metric of this run may be below the best clean
               `build` row with the same TOTAL of a commit that HEAD contains

Output: one summary line; for a failed step also the end of its log on
standard error. Evidence: $RANDOC_RUNS/gate/<utc>-<sha12>/ (a log for each
step, gate.json). Exit code: 0 when each step passes, else 1.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

sys.path.insert(0, str(Path(__file__).resolve().parent))
import measure  # noqa: E402

ROOT = measure.ROOT
BUILD_SCOREBOARD = measure.SCOREBOARD / "build.tsv"
CONFORMANCE_TEST = "each_unit_of_the_baseline_passes"
EXTERNAL_SUITES = ("pandoc", "corpus")
LOG_TAIL_LINES = 40


@dataclass
class Gate:
    evidence: Path
    base: str | None
    quick: bool
    metrics: dict[str, measure.Metric]
    results: list[measure.SuiteResult] = field(default_factory=list)


@dataclass(frozen=True)
class Step:
    """`run` gets the gate and the log file of the step; it returns None on a pass, else the reason."""

    name: str
    run: Callable[[Gate, Path], str | None]


def command(argv: list[str], log: Path, env: dict[str, str] | None = None) -> str | None:
    with open(log, "a", encoding="utf-8") as stream:
        stream.write(f"$ {' '.join(argv)}\n")
        stream.flush()
        code = subprocess.run(argv, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT,
                              env={**os.environ, **(env or {})}).returncode
    return None if code == 0 else f"exit code {code} of {' '.join(argv)[:160]}"


def licence(gate: Gate, log: Path) -> str | None:
    base = ["--base", gate.base] if gate.base else []
    return command([sys.executable, "-I", "tools/autobuild/licence_scan.py", "scan", *base], log)


def tool_tests(gate: Gate, log: Path) -> str | None:
    folders = sorted({path.parent for path in ROOT.glob("tools/*/test_*.py")}
                     | {path.parent for path in ROOT.glob("tools/*/tests/test_*.py")})
    failed = [str(folder.relative_to(ROOT)) for folder in folders
              if command([sys.executable, "-m", "unittest", "discover", "-s", str(folder)], log)]
    return f"tests failed in {', '.join(failed)}" if failed else None


def lost_units(baseline: list[str], report_units: list[dict]) -> list[str]:
    """The units of the baseline that do not pass. A unit that the suite no longer has is lost too."""
    passing = {unit["id"] for unit in report_units if unit["status"] == "pass"}
    return sorted(set(baseline) - passing)


def harness(gate: Gate, log: Path) -> str | None:
    try:
        measure.build()
        result = measure.run_suite("harness", gate.metrics, gate.evidence)
    except measure.ToolError as error:
        log.write_text(f"{error}\n", encoding="utf-8")
        return str(error).splitlines()[0]
    gate.results.append(result)
    baseline = json.loads((ROOT / "spec" / "baseline.json").read_text(encoding="utf-8"))["passing"]
    report = json.loads((gate.evidence / "harness-report.json").read_text(encoding="utf-8"))
    lost = lost_units(baseline, report["units"])
    log.write_text("".join(f"REGRESSION {unit}\n" for unit in lost), encoding="utf-8")
    if lost:
        return f"{len(lost)} units of spec/baseline.json do not pass, the first is {lost[0]}"
    return None


def fallen_metrics(rows: list[measure.Row], history: list[measure.Row],
                   contains: Callable[[str], bool]) -> list[str]:
    """The metrics of `rows` below the best clean history row with the same TOTAL of a commit that HEAD contains."""
    fallen = []
    for row in rows:
        earlier = [r for r in history if r.metric == row.metric and r.total == row.total
                   and r.dirty == "no" and contains(r.commit)]
        best = max(earlier, key=lambda r: r.passed, default=None)
        if best is not None and row.passed < best.passed:
            fallen.append(f"{row.metric} {row.passed}/{row.total} is below {best.passed}/{best.total} "
                          f"of {best.commit[:12]}")
    return fallen


def head_contains(commit: str) -> bool:
    return subprocess.run(["git", "merge-base", "--is-ancestor", commit, "HEAD"], cwd=ROOT,
                          capture_output=True).returncode == 0


def measurement(gate: Gate, log: Path) -> str | None:
    for suite in () if gate.quick else EXTERNAL_SUITES:
        try:
            gate.results.append(measure.run_suite(suite, gate.metrics, gate.evidence))
        except measure.ToolError as error:
            log.write_text(f"{error}\n", encoding="utf-8")
            return str(error).splitlines()[0]
    rows = [measure.Row("", "", "", "-", key, passed, total, result.tool, str(result.evidence))
            for result in gate.results for key, (passed, total) in sorted(result.counts.rows.items())]
    known: dict[str, bool] = {}
    fallen = fallen_metrics(rows, measure.read_rows(BUILD_SCOREBOARD),
                            lambda commit: known.setdefault(commit, head_contains(commit)))
    lines = [f"{row.metric} {row.passed}/{row.total}" for row in rows] + [f"FELL {text}" for text in fallen]
    log.write_text("".join(f"{line}\n" for line in lines), encoding="utf-8")
    return f"{len(fallen)} metrics fell: {fallen[0]}" if fallen else None


def cargo(*args: str, env: dict[str, str] | None = None) -> Callable[[Gate, Path], str | None]:
    return lambda gate, log: command(["cargo", *args], log, env)


STEPS = [
    Step("format", cargo("fmt", "--all", "--", "--check")),
    Step("licence", licence),
    Step("lints", cargo("clippy", "--workspace", "--all-targets", "--quiet", "--", "-D", "warnings")),
    Step("docs", cargo("doc", "--workspace", "--no-deps", "--quiet", env={"RUSTDOCFLAGS": "-D warnings"})),
    Step("tests", cargo("test", "--workspace", "--quiet", "--no-fail-fast", "--", "--skip", CONFORMANCE_TEST)),
    Step("tool-tests", tool_tests),
    Step("harness", harness),
    Step("measurement", measurement),
]


def duration(seconds: float) -> str:
    whole = round(seconds)
    return f"{whole // 60}m{whole % 60:02d}s" if whole >= 60 else f"{whole}s"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--quick", action="store_true", help="skip the pandoc and corpus suites")
    parser.add_argument("--base", help="the base of the licence scan (default: the default of licence_scan.py)")
    args = parser.parse_args(argv)
    try:
        metrics = measure.load_metrics()
        commit = measure.git("rev-parse", "HEAD")
        _, evidence = measure.new_evidence_dir("gate", commit)
    except measure.ToolError as error:
        print(f"gate: FAIL before the first step: {error}")
        return 1
    gate = Gate(evidence, args.base, args.quick, metrics)
    done = []
    failure = None
    started = time.monotonic()
    for step in STEPS:
        log = evidence / f"{step.name}.log"
        log.touch()
        begun = time.monotonic()
        reason = step.run(gate, log)
        done.append({"name": step.name, "seconds": round(time.monotonic() - begun, 1),
                     "status": "pass" if reason is None else "fail", "reason": reason, "log": str(log)})
        if reason is not None:
            failure = (step.name, reason, log)
            break
    total = duration(time.monotonic() - started)
    (evidence / "gate.json").write_text(json.dumps(
        {"commit": commit, "base": args.base, "quick": args.quick, "steps": done,
         "result": "pass" if failure is None else "fail"}, indent=1) + "\n", encoding="utf-8")
    times = ", ".join(f"{s['name']} {duration(s['seconds'])}" for s in done)
    if failure is None:
        print(f"gate: PASS in {total} ({times}); evidence {evidence}")
        return 0
    name, reason, log = failure
    tail = log.read_text(encoding="utf-8", errors="replace").splitlines()[-LOG_TAIL_LINES:]
    print("\n".join(tail), file=sys.stderr)
    print(f"gate: FAIL at {name} after {total}: {reason} ({times}); log {log}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
