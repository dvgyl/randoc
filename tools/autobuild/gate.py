#!/usr/bin/env python3
"""The gate of the build. It stops at the first step that fails.

Usage: gate.py [--quick] [--base REV] [--report FILE] [--filter TEXT]

Steps:
  format       cargo fmt --all -- --check
  licence      python3 -I tools/autobuild/licence_scan.py scan [--base REV]
  lints        cargo clippy --workspace --all-targets --quiet -- -D warnings
  docs         RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --quiet
  tests        cargo test --workspace --quiet --no-fail-fast, without the
               conformance test (the harness step runs the same units)
  tool-tests   python3 -m unittest discover in each tools/ folder with tests
  harness      build randoc-cli and randoc-harness, take a measurement slot
               and one read-only copy of the programs for the harness and
               measurement steps, run the harness suite of measure.py; each
               unit of spec/baseline.json must pass
  measurement  run each other active suite of measure.py (not with --quick),
               so that a new suite is checked too, except the suite of
               gate.steps (that suite runs this gate, so the gate never runs
               it); then two checks against the clean `build` rows (item -)
               of commits that HEAD contains, without the rows that a
               correction of their metric (plan/corrections.tsv) made
               obsolete:
               - no metric of this run is below the best such row with the
                 same TOTAL;
               - each unit of a suite that passed in the newest such run of
                 the suite (its evidence file <suite>.json, field passing)
                 passes now. A unit that fails now, that is excluded now or
                 that is gone is lost, unless a correction newer than that
                 run is of a metric of the unit: for a failing unit, a metric
                 that it fails now (failing_by_metric); for an excluded or
                 gone unit, a metric that it passed then (passing_by_metric
                 of that run). A unit with no metric list is lost. A suite
                 with no such evidence passes this check with a note in the
                 log.

The gate refuses to start when the drive has less than 15 GB free.
Output: one summary line; for a failed step also the end of its log on
standard error. Evidence: $RANDOC_RUNS/gate/<utc>-<sha12>/ (a log for each
step, gate.json: commit, base, quick, changed_outside_plan (the tracked
files outside plan/ with changes at the start), steps (each step of the
contract in order, with status pass, fail or not-run), result). --report
FILE writes the same report to FILE: the suite of gate.steps reads it.
--filter runs through the last step whose name contains TEXT. The report
marks this run as quick, so it cannot close an item.
Exit code: 0 when each step passes, else 1.
"""

from __future__ import annotations

import argparse
import contextlib
import json
import os
import re
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
HARNESS_SUITE = "harness"
GATE_METRIC = "gate.steps"
LOG_TAIL_LINES = 40
ORACLE_SHA256 = "8f17378a70e46eed7b63b5433fff6f5a0802f92659aa5782418548e0ca4a8e44"


def gate_suites(metrics: dict[str, measure.Metric]) -> list[str]:
    """The active suites that the gate runs: each one except the suite of gate.steps, which runs the gate."""
    own = metrics.get(GATE_METRIC)
    skip = own.suite if own is not None and own.status == "active" else None
    return [suite for suite in measure.active_suites(metrics) if suite != skip]


@dataclass
class Gate:
    evidence: Path
    base: str | None
    quick: bool
    metrics: dict[str, measure.Metric]
    stack: contextlib.ExitStack = field(default_factory=contextlib.ExitStack)
    programs: measure.Programs | None = None
    results: list[measure.SuiteResult] = field(default_factory=list)

    def snapshot(self) -> measure.Programs:
        """The measurement slot and the program copies of the gate, taken once for its suites."""
        if self.programs is None:
            self.stack.enter_context(measure.measurement_slot())
            commands = [measure.suite_command(s, self.metrics) for s in gate_suites(self.metrics)]
            self.programs = self.stack.enter_context(measure.program_snapshot(measure.programs_named(commands)))
        return self.programs


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


def oracle_sha256(_gate: Gate, log: Path) -> str | None:
    path = os.environ.get("RANDOC_ORACLE_PANDOC")
    if not path:
        return "set RANDOC_ORACLE_PANDOC"
    try:
        digest = measure.file_sha256(Path(path))
    except OSError as error:
        return f"cannot read the oracle: {error}"
    log.write_text(f"{digest}  {path}\n", encoding="utf-8")
    return None if digest == ORACLE_SHA256 else "the oracle SHA-256 differs from plan/SPEC.md"


ORACLE_REFERENCE = re.compile(
    r'RANDOC_ORACLE_PANDOC|pandoc-3\.13-arm64|'
    r'Command\s*::\s*new\s*\(\s*(?:b?r#*|b)?"(?:[^"\n]*/)?pandoc"'
)


def no_oracle_call(_gate: Gate, log: Path) -> str | None:
    files = measure.git("ls-files", "-z", "--", "crates").split("\0")
    matches = []
    for name in files:
        path = Path(name)
        if path.suffix != ".rs" or any(part in {"randoc-harness", "tests", "fuzz", "benches"}
                                       for part in path.parts):
            continue
        try:
            source = (ROOT / path).read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            return f"cannot read {name}: {error}"
        if match := ORACLE_REFERENCE.search(source):
            line = source.count("\n", 0, match.start()) + 1
            matches.append(f"{name}:{line}: {match.group()}")
    log.write_text("".join(f"{match}\n" for match in matches), encoding="utf-8")
    return f"found an oracle reference in {matches[0]}" if matches else None


def python_check(tool: str, test: str) -> Callable[[Gate, Path], str | None]:
    return lambda _gate, log: command([sys.executable, "-m", "unittest", test], log,
                                      {"PYTHONPATH": str(ROOT / "tools" / tool)})


def harness_heap(_gate: Gate, log: Path) -> str | None:
    test = "exec::tests::only_the_oracle_gets_the_heap_limit_and_its_exit_251_is_no_result"
    reason = command(["cargo", "test", "--quiet", "-p", "randoc-harness", "--lib", test, "--", "--exact"], log)
    if reason is None and "1 passed; 0 failed" not in log.read_text(encoding="utf-8"):
        return "the harness heap-limit test did not run"
    return reason


def lost_units(baseline: list[str], report_units: list[dict]) -> list[str]:
    """The units of the baseline that do not pass. A unit that the suite no longer has is lost too."""
    passing = {unit["id"] for unit in report_units if unit["status"] == "pass"}
    return sorted(set(baseline) - passing)


def harness(gate: Gate, log: Path) -> str | None:
    try:
        measure.build()
        result = measure.run_suite(HARNESS_SUITE, gate.metrics, gate.evidence, gate.snapshot())
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


def harness_verdicts(gate: Gate, log: Path) -> str | None:
    programs = gate.snapshot().paths
    reports = [gate.evidence / f"harness-jobs8-{index}.json" for index in (1, 2)]
    for report in reports:
        reason = command([programs["harness"], "run", "--randoc", programs["randoc"],
                          "--jobs", "8", "--report", str(report)], log)
        if reason:
            return reason
    return command([sys.executable, "tools/harness-verdicts/compare.py", *map(str, reports)], log)


def clean_history(history: list[measure.Row], contains: Callable[[str], bool],
                  corrections: list[dict[str, str]]) -> list[measure.Row]:
    """The clean `build` rows of full measurements (item -) of commits that HEAD contains,
    without held-out rows and without the rows that a newer correction made obsolete."""
    cut = measure.experiments.cutoffs(corrections)
    return [r for r in history if r.dirty == "no" and r.item == "-" and not r.metric.startswith("holdout.")
            and measure.experiments.counts(r.as_dict(), cut) and contains(r.commit)]


def fallen_metrics(rows: list[measure.Row], history: list[measure.Row]) -> list[str]:
    """The metrics of `rows` below the best row of `history` with the same TOTAL."""
    fallen = []
    for row in rows:
        earlier = [r for r in history if r.metric == row.metric and r.total == row.total]
        best = max(earlier, key=lambda r: r.passed, default=None)
        if best is not None and row.passed < best.passed:
            fallen.append(f"{row.metric} {row.passed}/{row.total} is below {best.passed}/{best.total} "
                          f"of {best.commit[:12]}")
    return fallen


def earlier_evidence(history: list[measure.Row], suite: str) -> measure.Row | None:
    """The newest row of `history` whose evidence file is <suite>.json and exists."""
    for row in sorted(history, key=lambda r: r.utc, reverse=True):
        path = Path(row.evidence)
        if path.name == f"{suite}.json" and path.is_file():
            return row
    return None


def metrics_by_unit(units_by_metric: dict | None) -> dict[str, set[str]]:
    found: dict[str, set[str]] = {}
    for metric, units in (units_by_metric or {}).items():
        for unit in units:
            found.setdefault(unit, set()).add(metric)
    return found


def lost_by_suite(now: dict, before: dict, corrected: set[str]) -> list[str]:
    """The units that passed in `before` and do not pass in `now` (they fail, are excluded or are gone),
    except the units with a corrected metric in their list. The list of a failing unit is its metrics
    in `failing_by_metric` of `now`; the list of an excluded or gone unit is its metrics in
    `passing_by_metric` of `before`. A correction never covers a unit with no list."""
    failing = metrics_by_unit(now.get("failing_by_metric"))
    passed = metrics_by_unit(before.get("passing_by_metric"))
    lost = set(before.get("passing") or []) - set(now.get("passing") or [])
    return sorted(unit for unit in lost if not failing.get(unit, passed.get(unit, set())) & corrected)


def unit_regressions(results: list[measure.SuiteResult], history: list[measure.Row],
                     corrections: list[dict[str, str]]) -> tuple[list[str], list[str]]:
    """The lost units of each suite of this gate (as SUITE UNIT), and the notes of the suites
    that have no earlier evidence with a unit list."""
    cut = measure.experiments.cutoffs(corrections)
    lost, notes = [], []
    for result in results:
        earlier = earlier_evidence(history, result.suite)
        before = json.loads(Path(earlier.evidence).read_text(encoding="utf-8")) if earlier else {}
        if not isinstance(before.get("passing"), list):
            notes.append(f"{result.suite}: no earlier clean evidence with a unit list; the unit check passes")
            continue
        now = json.loads(result.evidence.read_text(encoding="utf-8"))
        if not isinstance(now.get("passing"), list):
            notes.append(f"{result.suite}: this run has no unit list; the unit check passes")
            continue
        corrected = {metric for metric, utc in cut.items() if utc > earlier.utc}
        lost += [f"{result.suite} {unit}" for unit in lost_by_suite(now, before, corrected)]
    return lost, notes


def head_contains(commit: str) -> bool:
    return subprocess.run(["git", "merge-base", "--is-ancestor", commit, "HEAD"], cwd=ROOT,
                          capture_output=True).returncode == 0


def measurement(gate: Gate, log: Path) -> str | None:
    suites = [] if gate.quick else [s for s in gate_suites(gate.metrics) if s != HARNESS_SUITE]
    for suite in suites:
        try:
            gate.results.append(measure.run_suite(suite, gate.metrics, gate.evidence, gate.snapshot()))
        except measure.ToolError as error:
            log.write_text(f"{error}\n", encoding="utf-8")
            return str(error).splitlines()[0]
    rows = [measure.Row("", "", "", "-", key, passed, total, result.tool, str(result.evidence))
            for result in gate.results for key, (passed, total) in sorted(result.counts.rows.items())]
    known: dict[str, bool] = {}
    corrections = measure.corrections()
    history = clean_history(measure.read_rows(BUILD_SCOREBOARD),
                            lambda commit: known.setdefault(commit, head_contains(commit)), corrections)
    fallen = fallen_metrics(rows, history)
    lost, notes = unit_regressions(gate.results, history, corrections)
    lines = ([f"{row.metric} {row.passed}/{row.total}" for row in rows] + [f"NOTE {text}" for text in notes]
             + [f"FELL {text}" for text in fallen] + [f"LOST {unit}" for unit in lost])
    log.write_text("".join(f"{line}\n" for line in lines), encoding="utf-8")
    if fallen:
        return f"{len(fallen)} metrics fell: {fallen[0]}"
    if lost:
        return f"{len(lost)} units passed in the earlier clean run and fail now, the first is {lost[0]}"
    return None


def cargo(*args: str, env: dict[str, str] | None = None) -> Callable[[Gate, Path], str | None]:
    return lambda gate, log: command(["cargo", *args], log, env)


STEPS = [
    Step("format", cargo("fmt", "--all", "--", "--check")),
    Step("licence", licence),
    Step("oracle-sha256", oracle_sha256),
    Step("no-oracle-call", no_oracle_call),
    Step("lints", cargo("clippy", "--workspace", "--all-targets", "--quiet", "--", "-D", "warnings")),
    Step("docs", cargo("doc", "--workspace", "--no-deps", "--quiet", env={"RUSTDOCFLAGS": "-D warnings"})),
    Step("tests", cargo("test", "--workspace", "--quiet", "--no-fail-fast", "--", "--skip", CONFORMANCE_TEST)),
    Step("tool-tests", tool_tests),
    Step("oracle-heap-harness", harness_heap),
    Step("oracle-heap-pandoc", python_check("pandoc-testsuite",
         "test_suite.OracleHeapLimit.test_only_the_oracle_gets_the_limit_and_its_exit_251_is_no_result")),
    Step("oracle-heap-corpus", python_check("corpus",
         "test_corpus.RunTests.test_each_run_of_the_oracle_program_gets_the_heap_limit")),
    Step("oracle-heap-commonmark", python_check("commonmark-spec",
         "test_run.HeapLimitTests.test_only_the_oracle_gets_the_limit_and_its_exit_251_is_an_oracle_problem")),
    Step("harness", harness),
    Step("harness-verdicts", harness_verdicts),
    Step("measurement", measurement),
]


def changed_outside_plan() -> list[str]:
    """The tracked files outside plan/ with changes: a gate of such a tree is not a gate of its commit."""
    out = subprocess.run(["git", "status", "--porcelain=v1", "-z", "--untracked-files=no"], cwd=ROOT,
                         capture_output=True, text=True)
    if out.returncode != 0:
        raise measure.ToolError(f"git status: {out.stderr.strip()}")
    return [entry[3:] for entry in out.stdout.split("\0") if len(entry) > 3 and not entry[3:].startswith("plan/")]


def duration(seconds: float) -> str:
    whole = round(seconds)
    return f"{whole // 60}m{whole % 60:02d}s" if whole >= 60 else f"{whole}s"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--quick", action="store_true", help="skip the pandoc and corpus suites")
    parser.add_argument("--base", help="the base of the licence scan (default: the default of licence_scan.py)")
    parser.add_argument("--report", type=Path, help="also write the report (gate.json) to FILE, for the suite of gate.steps")
    parser.add_argument("--filter", help="run through the last step whose name contains TEXT", metavar="TEXT")
    args = parser.parse_args(argv)
    steps_to_run = STEPS
    if args.filter is not None:
        matches = [index for index, step in enumerate(STEPS) if args.filter in step.name]
        if not args.filter or not matches:
            parser.error(f"no gate step matches {args.filter!r}")
        steps_to_run = STEPS[:matches[-1] + 1]
    try:
        measure.check_free_disk(measure.runs_folder())
        metrics = measure.load_metrics()
        commit = measure.git("rev-parse", "HEAD")
        changed = changed_outside_plan()
        _, evidence = measure.new_evidence_dir("gate", commit)
    except measure.ToolError as error:
        print(f"gate: FAIL before the first step: {error}")
        return 1
    gate = Gate(evidence, args.base, args.quick, metrics)
    done = []
    failure = None
    started = time.monotonic()
    try:
        with gate.stack:
            for step in steps_to_run:
                log = evidence / f"{step.name}.log"
                log.touch()
                begun = time.monotonic()
                reason = step.run(gate, log)
                done.append({"name": step.name, "seconds": round(time.monotonic() - begun, 1),
                             "status": "pass" if reason is None else "fail", "reason": reason, "log": str(log)})
                if reason is not None:
                    failure = (step.name, reason, log)
                    break
    except measure.ToolError as error:
        # The end of the program copies: a copy changed during the gate.
        log = evidence / "programs.log"
        log.write_text(f"{error}\n", encoding="utf-8")
        done.append({"name": "programs", "seconds": 0, "status": "fail", "reason": str(error), "log": str(log)})
        failure = failure or ("programs", str(error), log)
    total = duration(time.monotonic() - started)
    ran = {s["name"] for s in done}
    programs = [step for step in done if step["name"] == "programs"]
    steps = [step for step in done if step["name"] != "programs"]
    steps += [{"name": step.name, "status": "not-run"} for step in STEPS if step.name not in ran]
    steps += programs
    report = json.dumps({"commit": commit, "base": args.base, "quick": args.quick or args.filter is not None,
                         "filter": args.filter, "changed_outside_plan": changed, "steps": steps,
                         "result": "pass" if failure is None else "fail"}, indent=1) + "\n"
    (evidence / "gate.json").write_text(report, encoding="utf-8")
    if args.report:
        args.report.write_text(report, encoding="utf-8")
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
