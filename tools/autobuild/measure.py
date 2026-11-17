#!/usr/bin/env python3
"""Measure the metrics of tools/autobuild/metrics.toml and append rows to the scoreboard.

Usage: measure.py [--suite NAME ...] [--quick] [--item ID] [--stream NAME]
                  [--filter TEXT] [--no-build]

Steps: build randoc-cli and randoc-harness (unless --no-build); run the
command of each selected suite; write the evidence of each suite to
$RANDOC_RUNS/measure/<utc>-<sha12>/<suite>.json (fields rows, failing,
no_result, unstable, oracle_limit, no_input, duration_s; the classes of
plan/SPEC.md 3.11); append one row for each metric to
plan/scoreboard/<stream>.tsv; print each row with its change against the
previous row of the same metric.

Default suites: each suite with an active metric. --quick: the harness suite
(harness.* and caps.*) and the suite of the metric of the --item in
plan/ITEMS.tsv, so that each experiment measures its own metric. Default stream: `build` on gpt/build, ID on
gpt/w/ID. A run with --filter writes evidence but no row, because its TOTAL
is not the TOTAL of the metric.

Exit code: 0; 1 when a metric fell below the best earlier clean row of the
same stream with the same TOTAL; 2 when a tool failed (the rows of the other
suites are still written).
"""

from __future__ import annotations

import argparse
import contextlib
import json
import os
import re
import shlex
import shutil
import subprocess
import tempfile
import sys
import time
import tomllib
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable, Iterator

ROOT = Path(__file__).resolve().parents[2]
METRICS_FILE = Path(__file__).resolve().with_name("metrics.toml")
SCOREBOARD = ROOT / "plan" / "scoreboard"
HEADER = ("utc", "commit", "dirty", "item", "metric", "pass", "total", "tool", "evidence")
STATUSES = ("active", "to-build", "external")
ENV_PLACEHOLDERS = {
    "oracle": "RANDOC_ORACLE_PANDOC",
    "pandoc_src": "RANDOC_PANDOC_SRC",
    "corpus": "RANDOC_CORPUS",
}


class ToolError(Exception):
    """A measurement tool failed, so its suite has no rows."""


@dataclass(frozen=True)
class Metric:
    key: str
    suite: str
    status: str
    command: str
    pass_rule: str
    total_rule: str


@dataclass(frozen=True)
class Row:
    utc: str
    commit: str
    dirty: str
    item: str
    metric: str
    passed: int
    total: int
    tool: str
    evidence: str

    def line(self) -> str:
        fields = (self.utc, self.commit, self.dirty, self.item, self.metric,
                  str(self.passed), str(self.total), self.tool, self.evidence)
        return "\t".join(fields) + "\n"


# The classes of a unit (plan/SPEC.md, 3.11). PASS counts the class pass;
# TOTAL counts pass and fail. The other classes are reported apart.
PASS, FAIL, UNSTABLE, ORACLE_LIMIT, NO_INPUT = "pass", "fail", "unstable", "oracle_limit", "no_input"


@dataclass(frozen=True)
class Unit:
    """One unit of a suite run: its class and the metrics that it counts in."""

    id: str
    cls: str
    metrics: tuple[str, ...]
    no_result: bool = False


@dataclass
class Counts:
    """The result of one suite run: PASS and TOTAL of each metric, and the unit lists of each class.

    `no_result` lists the units of class fail where Randoc gave no result.
    """

    rows: dict[str, tuple[int, int]] = field(default_factory=dict)
    failing: list[str] = field(default_factory=list)
    no_result: list[str] = field(default_factory=list)
    unstable: list[str] = field(default_factory=list)
    oracle_limit: list[str] = field(default_factory=list)
    no_input: list[str] = field(default_factory=list)

    @classmethod
    def of(cls, units: list[Unit], metrics: tuple[str, ...]) -> Counts:
        counts = cls(rows={key: (0, 0) for key in metrics})
        lists = {FAIL: counts.failing, UNSTABLE: counts.unstable,
                 ORACLE_LIMIT: counts.oracle_limit, NO_INPUT: counts.no_input}
        for unit in units:
            if unit.cls in (PASS, FAIL):
                for key in unit.metrics:
                    passed, total = counts.rows[key]
                    counts.rows[key] = (passed + (unit.cls == PASS), total + 1)
            if unit.cls != PASS:
                lists[unit.cls].append(unit.id)
            if unit.no_result:
                counts.no_result.append(unit.id)
        return counts


@dataclass(frozen=True)
class SuiteResult:
    suite: str
    counts: Counts
    tool: str
    evidence: Path
    duration_s: float


# The parsers: each reads the raw output of its suite command from the evidence folder.

HARNESS_KINDS = {"read": "harness.read", "write": "harness.write", "e2e": "harness.e2e", "cmd": "harness.cmd"}
HARNESS_CLASSES = {"pass": PASS, "fail": FAIL, "no_result": FAIL, "not_recorded": FAIL, "skipped": NO_INPUT}
CAPS = {
    "caps.readers": ("input_formats", "input-formats.json"),
    "caps.writers": ("output_formats", "output-formats.json"),
    "caps.options": ("options", "options.json"),
}


def read_json(path: Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise ToolError(f"cannot read {path}: {error}") from error


def inventory_names(path: Path) -> list[str]:
    """The names of an inventory list; an option is named by its first long name."""
    return [item["long"][0] if isinstance(item, dict) else item for item in read_json(path)]


def check_sum(suite: str, units: list[Unit], expected: int) -> None:
    if len(units) != expected:
        raise ToolError(f"{suite}: the classes hold {len(units)} units, but the report has {expected}")


def harness_counts(evidence: Path, log: str) -> Counts:
    report = read_json(evidence / "harness-report.json")
    units = []
    for unit in report["units"]:
        ident, status = unit["id"], unit["status"]
        kind = ident.rpartition("#")[2].partition(":")[0]
        units.append(Unit(ident, HARNESS_CLASSES[status], ("harness.units", HARNESS_KINDS[kind]),
                          status == "no_result"))
    check_sum("harness", units, report["summary"]["units"])
    counts = Counts.of(units, ("harness.units", *HARNESS_KINDS.values()))
    matrix_path = evidence / "capabilities.json"
    if matrix_path.exists():
        matrix = read_json(matrix_path)
        for key, (section, inventory) in CAPS.items():
            names = inventory_names(ROOT / "spec" / "inventory" / inventory)
            passing = sum(matrix[section].get(name, {}).get("status") == "passing" for name in names)
            counts.rows[key] = (passing, len(names))
    return counts


PANDOC_GROUPS = {"command": "suite.pandoc.command", "old": "suite.pandoc.old", "readers": "suite.pandoc.readers"}
# Failure texts of tools/pandoc-testsuite/run.py where the oracle gave no result.
PANDOC_ORACLE_LIMIT = ("no result: the oracle", "timeout (expected)", "timeout (--list-extensions")


def pandoc_counts(evidence: Path, log: str) -> Counts:
    report = read_json(evidence / "pandoc-report.json")

    def unit(ident: str, cls: str, no_result: bool = False) -> Unit:
        return Unit(ident, cls, ("suite.pandoc", PANDOC_GROUPS[ident.partition("/")[0]]), no_result)

    units = [unit(ident, PASS) for ident in report["passing"]]
    lines = log.splitlines()
    for index, line in enumerate(lines):
        if line.startswith("FAIL "):
            reason = lines[index + 1].strip() if index + 1 < len(lines) else ""
            if reason.startswith(PANDOC_ORACLE_LIMIT):
                units.append(unit(line[5:], ORACLE_LIMIT))
            else:
                units.append(unit(line[5:], FAIL, reason.startswith("timeout")))
    check_sum("pandoc", units, report["totals"]["run"])
    return Counts.of(units, ("suite.pandoc", *PANDOC_GROUPS.values()))


CORPUS_TARGETS = ("json", "html", "markdown")
CORPUS_ORACLE_LIMIT = ("oracle timeout", "error oracle")
CORPUS_NO_RESULT = ("timeout", "error program")


def corpus_counts(evidence: Path, log: str) -> Counts:
    report = read_json(evidence / "corpus-report.json")
    units = []
    for ident, status in sorted(report["results"].items()):
        cls = PASS if status == "pass" else ORACLE_LIMIT if status in CORPUS_ORACLE_LIMIT else FAIL
        target = ident.rpartition(":")[2]
        units.append(Unit(ident, cls, ("corpus.units", f"corpus.{target}"), status in CORPUS_NO_RESULT))
    check_sum("corpus", units, report["totals"]["units"])
    return Counts.of(units, ("corpus.units", *(f"corpus.{target}" for target in CORPUS_TARGETS)))


@dataclass(frozen=True)
class Runner:
    parse: Callable[[Path, str], Counts]
    filter_option: str
    tool_dir: str


RUNNERS = {
    "harness": Runner(harness_counts, "--filter", "crates/randoc-harness"),
    "pandoc": Runner(pandoc_counts, "--filter", "tools/pandoc-testsuite"),
    "corpus": Runner(corpus_counts, "--only", "tools/corpus"),
}


def load_metrics(path: Path | None = None) -> dict[str, Metric]:
    """Reads and checks the registry: unique keys, known status, a runner and one command per suite."""
    path = path or METRICS_FILE
    try:
        entries = tomllib.loads(path.read_text(encoding="utf-8"))["metric"]
    except (OSError, ValueError, KeyError) as error:
        raise ToolError(f"{path}: {error}") from error
    metrics: dict[str, Metric] = {}
    commands: dict[str, str] = {}
    for entry in entries:
        metric = Metric(entry["key"], entry["suite"], entry["status"], entry["command"],
                        entry["pass"], entry["total"])
        if metric.key in metrics:
            raise ToolError(f"{path}: the key {metric.key} is listed two times")
        if metric.status not in STATUSES:
            raise ToolError(f"{path}: {metric.key} has the status {metric.status!r}, not one of {STATUSES}")
        if metric.status == "active":
            if metric.suite not in RUNNERS:
                raise ToolError(f"{path}: {metric.key} is active, but measure.py has no runner for suite {metric.suite}")
            if commands.setdefault(metric.suite, metric.command) != metric.command:
                raise ToolError(f"{path}: the active metrics of suite {metric.suite} have different commands")
        metrics[metric.key] = metric
    return metrics


def active_suites(metrics: dict[str, Metric]) -> list[str]:
    return sorted({m.suite for m in metrics.values() if m.status == "active"})


def git(*args: str) -> str:
    done = subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True)
    if done.returncode != 0:
        raise ToolError(f"git {' '.join(args)}: {done.stderr.strip()}")
    return done.stdout.strip()


def tool_version(name: str, directory: str) -> str:
    try:
        tree = git("rev-parse", f"HEAD:{directory}")[:12]
    except ToolError:
        return f"{name}@unversioned"
    changed = git("status", "--porcelain", "--", directory)
    return f"{name}@{tree}" + ("+dirty" if changed else "")


def target_dir() -> Path:
    return Path(os.environ.get("CARGO_TARGET_DIR") or ROOT / "target")


PROGRAMS = {"randoc": "randoc", "harness": "randoc-harness"}


@contextlib.contextmanager
def program_copies(command: str) -> Iterator[dict[str, str]]:
    """Copies the debug programs that `command` names into a new folder of $TMPDIR for one suite run.

    A cargo command in the same target folder rewrites the programs in place
    (exFAT has no hard links), and a run of a half-written program gives a
    false result. The copies make the run independent of other builds.
    """
    folder = Path(tempfile.mkdtemp(prefix="randoc-measure-"))
    try:
        copies = {}
        for name, program in PROGRAMS.items():
            if f"{{{name}}}" not in command:
                continue
            source = target_dir() / "debug" / program
            try:
                shutil.copy2(source, folder / program)
            except OSError as error:
                raise ToolError(f"cannot copy {source}: {error}. Build it: cargo build -p randoc-cli -p randoc-harness")
            copies[name] = str(folder / program)
        yield copies
    finally:
        shutil.rmtree(folder, ignore_errors=True)


def placeholders(evidence: Path, programs: dict[str, str]) -> dict[str, str]:
    values = {"python": sys.executable, "evidence": str(evidence), **programs}
    for name, variable in ENV_PLACEHOLDERS.items():
        if os.environ.get(variable):
            values[name] = os.environ[variable]
    return values


def suite_argv(command: str, evidence: Path, programs: dict[str, str], runner: Runner,
               text: str | None) -> list[str]:
    values = placeholders(evidence, programs)
    argv = []
    for token in shlex.split(command):
        try:
            argv.append(token.format(**values))
        except KeyError as missing:
            raise ToolError(f"set {ENV_PLACEHOLDERS.get(missing.args[0], missing.args[0])}") from None
    if text is not None:
        if "--matrix" in argv:
            # The harness writes the matrix only for the whole suite.
            at = argv.index("--matrix")
            del argv[at:at + 2]
        argv += [runner.filter_option, text]
    return argv


def run_suite(suite: str, metrics: dict[str, Metric], evidence: Path, text: str | None = None) -> SuiteResult:
    """Runs the command of `suite`, parses its output, and writes `<suite>.json` into `evidence`."""
    runner = RUNNERS[suite]
    active = sorted(k for k, m in metrics.items() if m.suite == suite and m.status == "active")
    if not active:
        raise ToolError(f"suite {suite} has no active metric in {METRICS_FILE.name}")
    log_path = evidence / f"{suite}.log"
    started = time.monotonic()
    command = metrics[active[0]].command
    with program_copies(command) as programs, open(log_path, "w", encoding="utf-8") as log:
        argv = suite_argv(command, evidence, programs, runner, text)
        try:
            code = subprocess.run(argv, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT).returncode
        except OSError as error:
            raise ToolError(f"{suite}: {argv[0]}: {error}") from error
    duration = time.monotonic() - started
    log_text = log_path.read_text(encoding="utf-8", errors="replace")
    if code != 0:
        tail = "\n".join(log_text.splitlines()[-5:])
        raise ToolError(f"{suite}: exit code {code} (log {log_path}):\n{tail}")
    try:
        counts = runner.parse(evidence, log_text)
    except (KeyError, TypeError, IndexError, AttributeError) as error:
        raise ToolError(f"{suite}: unexpected output ({error!r}); see {evidence}") from error
    unknown = set(counts.rows) - set(active)
    missing = set(active) - set(counts.rows)
    if unknown or (missing and text is None):
        raise ToolError(f"{suite}: rows {sorted(unknown)} are not active in metrics.toml, "
                        f"and active metrics {sorted(missing)} have no row")
    tool = tool_version(suite, runner.tool_dir)
    record = {
        "suite": suite, "command": argv, "filter": text, "tool": tool,
        "rows": [{"metric": k, "pass": p, "total": t} for k, (p, t) in sorted(counts.rows.items())],
        "failing": counts.failing, "no_result": counts.no_result, "unstable": counts.unstable,
        "oracle_limit": counts.oracle_limit, "no_input": counts.no_input, "duration_s": round(duration, 1),
    }
    path = evidence / f"{suite}.json"
    path.write_text(json.dumps(record, indent=1) + "\n", encoding="utf-8")
    return SuiteResult(suite, counts, tool, path, duration)


def build() -> None:
    done = subprocess.run(["cargo", "build", "--quiet", "-p", "randoc-cli", "-p", "randoc-harness"],
                          cwd=ROOT, capture_output=True, text=True)
    if done.returncode != 0:
        raise ToolError(f"cargo build failed:\n{done.stderr[-2000:]}")


def read_rows(path: Path) -> list[Row]:
    if not path.exists():
        return []
    lines = [line for line in path.read_text(encoding="utf-8").splitlines()
             if line.strip() and not line.startswith("#")]
    if not lines:
        return []
    header = lines[0].split("\t")
    rows = []
    for line in lines[1:]:
        value = dict(zip(header, line.split("\t")))
        try:
            rows.append(Row(value["utc"], value["commit"], value["dirty"], value["item"], value["metric"],
                            int(value["pass"]), int(value["total"]), value["tool"], value["evidence"]))
        except (KeyError, ValueError):
            continue
    return rows


def append_rows(path: Path, rows: list[Row]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    text = "".join(row.line() for row in rows)
    if not path.exists() or path.stat().st_size == 0:
        text = "\t".join(HEADER) + "\n" + text
    # One write on an append-only descriptor, so that two writers do not mix lines.
    descriptor = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o644)
    try:
        os.write(descriptor, text.encode("utf-8"))
    finally:
        os.close(descriptor)


def best_pass(history: list[Row], metric: str, total: int) -> int | None:
    """The best PASS of the clean rows of `metric` with the same TOTAL. A dirty row is an experiment."""
    values = [r.passed for r in history if r.metric == metric and r.total == total and r.dirty == "no"]
    return max(values) if values else None


def describe(row: Row, history: list[Row]) -> tuple[str, bool]:
    """The printed line of a new row, and True when it fell below the best earlier clean row."""
    same = [r for r in history if r.metric == row.metric]
    text = f"{row.metric:24} {row.passed}/{row.total}"
    if not same:
        text += "  (first row)"
    elif same[-1].total == row.total:
        text += f"  ({row.passed - same[-1].passed:+d} against {same[-1].passed}/{same[-1].total})"
    else:
        text += f"  (TOTAL changed: previous row {same[-1].passed}/{same[-1].total})"
    best = best_pass(history, row.metric, row.total)
    fell = best is not None and row.passed < best
    if fell:
        text += f"  FELL below the best clean row {best}/{row.total}"
    return text, fell


def default_stream(branch: str) -> str | None:
    if branch == "gpt/build":
        return "build"
    match = re.fullmatch(r"gpt/w/([A-Za-z0-9][A-Za-z0-9._-]*)", branch)
    return match.group(1) if match else None


def new_evidence_dir(kind: str, sha: str) -> tuple[str, Path]:
    runs = os.environ.get("RANDOC_RUNS")
    if not runs:
        raise ToolError("set RANDOC_RUNS (source tools/autobuild/env.sh)")
    while True:
        now = datetime.now(timezone.utc)
        path = Path(runs) / kind / f"{now:%Y%m%dT%H%M%SZ}-{sha[:12]}"
        try:
            path.mkdir(parents=True)
            return now.strftime("%Y-%m-%dT%H:%M:%SZ"), path
        except FileExistsError:
            time.sleep(1)


def item_metric(item: str) -> str | None:
    """The metric of `item` in plan/ITEMS.tsv, or None."""
    path = ROOT / "plan" / "ITEMS.tsv"
    lines = path.read_text(encoding="utf-8").splitlines() if path.exists() else []
    lines = [line for line in lines if line.strip() and not line.startswith("#")]
    header = lines[0].split("\t") if lines else []
    for line in lines[1:]:
        value = dict(zip(header, line.split("\t")))
        if value.get("id") == item:
            return value.get("metric")
    return None


def quick_suites(item: str, metrics: dict[str, Metric]) -> list[str]:
    metric = metrics.get(item_metric(item) or "")
    extra = [metric.suite] if metric and metric.status == "active" and metric.suite != "harness" else []
    return ["harness", *extra]


def parse_args(argv: list[str] | None, metrics: dict[str, Metric]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    choice = parser.add_mutually_exclusive_group()
    choice.add_argument("--suite", action="extend", nargs="+", choices=active_suites(metrics),
                        help="the suites to run (default: each suite with an active metric)")
    choice.add_argument("--quick", action="store_true",
                        help="the harness suite and the suite of the metric of --item")
    parser.add_argument("--item", help="the item id that this run measures (default: the ID of gpt/w/ID, else -)")
    parser.add_argument("--stream", help="the scoreboard file plan/scoreboard/STREAM.tsv")
    parser.add_argument("--filter", help="only the units whose identifier holds TEXT; writes no row")
    parser.add_argument("--no-build", action="store_true", help="do not build randoc-cli and randoc-harness")
    args = parser.parse_args(argv)
    for name in ("item", "stream"):
        value = getattr(args, name)
        if value is not None and not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", value):
            parser.error(f"--{name} {value!r} must be letters, digits, '.', '_' or '-'")
    if args.filter is not None and not args.filter.strip():
        parser.error("--filter needs a text")
    return args


def main(argv: list[str] | None = None) -> int:
    try:
        metrics = load_metrics()
    except ToolError as error:
        print(f"measure: {error}", file=sys.stderr)
        return 2
    args = parse_args(argv, metrics)
    try:
        branch = git("rev-parse", "--abbrev-ref", "HEAD")
        stream = args.stream or default_stream(branch)
        if stream is None:
            raise ToolError(f"branch {branch} is not gpt/build or gpt/w/ID: pass --stream NAME")
        item = args.item or (stream if stream != "build" and args.stream is None else "-")
        commit = git("rev-parse", "HEAD")
        dirty = "yes" if git("status", "--porcelain", "--untracked-files=no") else "no"
        suites = quick_suites(item, metrics) if args.quick else (args.suite or active_suites(metrics))
        if not args.no_build:
            build()
        utc, evidence = new_evidence_dir("measure", commit)
    except ToolError as error:
        print(f"measure: {error}", file=sys.stderr)
        return 2

    scoreboard = SCOREBOARD / f"{stream}.tsv"
    history = read_rows(scoreboard)
    print(f"measure: {commit[:12]}{' (dirty)' if dirty == 'yes' else ''}, stream {stream}, item {item}, "
          f"evidence {evidence}")
    failed = False
    fell = False
    new_rows: list[Row] = []
    for suite in suites:
        try:
            result = run_suite(suite, metrics, evidence, args.filter)
        except ToolError as error:
            print(f"measure: {error}", file=sys.stderr)
            failed = True
            continue
        c = result.counts
        print(f"{suite}: {result.duration_s:.0f} s, {result.tool}; {len(c.failing)} fail "
              f"({len(c.no_result)} with no result); not in TOTAL: {len(c.unstable)} unstable, "
              f"{len(c.oracle_limit)} oracle-limit, {len(c.no_input)} no input")
        for key, (passed, total) in sorted(result.counts.rows.items()):
            row = Row(utc, commit, dirty, item, key, passed, total, result.tool, str(result.evidence))
            text, below = describe(row, history)
            print(f"  {text}")
            fell = fell or below
            new_rows.append(row)
    if args.filter is not None:
        print(f"measure: filtered run, so no row went to {scoreboard}")
    elif new_rows:
        append_rows(scoreboard, new_rows)
        print(f"measure: appended {len(new_rows)} rows to {scoreboard}")
    return 2 if failed else 1 if fell else 0


if __name__ == "__main__":
    sys.exit(main())
