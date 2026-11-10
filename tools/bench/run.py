#!/usr/bin/env python3
"""Measures the CPU time of a program on the benchmarks and compares two programs.

    python3 tools/bench/run.py --program PATH [--compare PATH] [--filter TEXT]
                               [--runs N] [--max-runs M] [--alpha A]
                               [--timeout S] [--benchmarks FILE] --out FILE

`--program` is the program under test. `--compare` is the reference, for
example a release build of `main`. The exit code is 1 when a benchmark is
`slower` or `lost-support`, 2 when a run failed, 3 when a comparison is
`inconclusive`, and else 0 (see `exit_code`). docs/BENCHMARKS.md describes the
method.
"""

import argparse
import dataclasses
import datetime
import functools
import hashlib
import json
import math
import os
import pathlib
import platform
import shutil
import statistics
import subprocess
import sys
import signal
import tempfile
import threading
import time
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]
DEFAULT_BENCHMARKS = ROOT / "benchmarks" / "benchmarks.toml"

THRESHOLD = 0.05
ALPHA = 0.01
# The resolution of the CPU time from `wait4`. A time of 0 counts as this value.
CPU_RESOLUTION = 1e-6
# Pandoc exits with 21 for an unknown input format and 22 for an unknown output format.
NOT_SUPPORTED_EXIT_CODES = (21, 22)
MAXRSS_UNIT = 1 if sys.platform == "darwin" else 1024


class ConfigError(Exception):
    """A problem in the benchmarks file."""


@dataclasses.dataclass(frozen=True)
class Benchmark:
    """One command line and one standard input."""

    name: str
    args: tuple
    input: pathlib.Path | None = None
    stdin: str | None = None
    series: str | None = None
    scale: int | None = None
    startup: str | None = None

    def stdin_label(self, base):
        """Returns the standard input as the JSON file records it."""
        if self.input is not None:
            return {"file": os.path.relpath(self.input, base)}
        return {"text": self.stdin or ""}


@dataclasses.dataclass(frozen=True)
class Sample:
    """The resource use of one run. Times are in seconds, memory in bytes."""

    cpu: float
    user: float
    sys: float
    wall: float
    maxrss: int


def _string_list(table, key, where):
    value = table.get(key)
    if not isinstance(value, list) or not all(isinstance(v, str) for v in value):
        raise ConfigError(f"{where}: `{key}` must be a list of strings")
    return tuple(value)


def _name(table, where, names):
    name = table.get("name")
    if not isinstance(name, str) or not name:
        raise ConfigError(f"{where}: `name` must be a non-empty string")
    if name in names:
        raise ConfigError(f"{where}: the name `{name}` is used two times")
    names.add(name)
    return name


def parse_benchmarks(text, base):
    """Parses the text of a benchmarks file. Paths are relative to `base`."""
    data = tomllib.loads(text)
    unknown = set(data) - {"benchmark", "series"}
    if unknown:
        raise ConfigError(f"unknown tables: {', '.join(sorted(unknown))}")
    names = set()
    benchmarks = []
    for index, table in enumerate(data.get("benchmark", [])):
        where = f"benchmark {index + 1}"
        extra = set(table) - {"name", "args", "input", "stdin"}
        if extra:
            raise ConfigError(f"{where}: unknown keys: {', '.join(sorted(extra))}")
        if "input" in table and "stdin" in table:
            raise ConfigError(f"{where}: use `input` or `stdin`, not both")
        name = _name(table, where, names)
        stdin = table.get("stdin")
        if stdin is not None and not isinstance(stdin, str):
            raise ConfigError(f"{where}: `stdin` must be a string")
        benchmarks.append(
            Benchmark(
                name=name,
                args=_string_list(table, "args", where),
                input=base / table["input"] if "input" in table else None,
                stdin=stdin,
            )
        )
    for index, table in enumerate(data.get("series", [])):
        where = f"series {index + 1}"
        extra = set(table) - {"name", "args", "input", "scales", "startup_stdin"}
        if extra:
            raise ConfigError(f"{where}: unknown keys: {', '.join(sorted(extra))}")
        series = _name(table, where, set())
        pattern = table.get("input")
        if not isinstance(pattern, str) or "{scale}" not in pattern:
            raise ConfigError(f"{where}: `input` must be a string that contains {{scale}}")
        scales = table.get("scales")
        if not isinstance(scales, list) or not scales or not all(
            isinstance(s, int) and s > 0 for s in scales
        ):
            raise ConfigError(f"{where}: `scales` must be a list of positive integers")
        startup_stdin = table.get("startup_stdin")
        if not isinstance(startup_stdin, str):
            raise ConfigError(f"{where}: `startup_stdin` must be a string")
        args = _string_list(table, "args", where)
        startup = _name({"name": f"{series}-startup"}, where, names)
        benchmarks.append(Benchmark(name=startup, args=args, stdin=startup_stdin))
        for scale in scales:
            benchmarks.append(
                Benchmark(
                    name=_name({"name": f"{series}-x{scale}"}, where, names),
                    args=args,
                    input=base / pattern.replace("{scale}", str(scale)),
                    series=series,
                    scale=scale,
                    startup=startup,
                )
            )
    return benchmarks


def load_benchmarks(path):
    """Reads and parses a benchmarks file."""
    path = pathlib.Path(path)
    return parse_benchmarks(path.read_text(encoding="utf-8"), path.parent)


def select(benchmarks, text):
    """Returns the benchmarks whose name contains `text`, with their startup benchmarks."""
    if not text:
        return list(benchmarks)
    chosen = {b.name for b in benchmarks if text in b.name}
    chosen |= {b.startup for b in benchmarks if b.name in chosen and b.startup}
    return [b for b in benchmarks if b.name in chosen]


def run_once(argv, stdin_path, stdout_path=os.devnull, stderr_path=os.devnull, timeout=None):
    """Runs a command one time and returns (exit code, Sample).

    The child runs in its own process group. At the time limit, or when the
    wait stops with an exception, the tool kills the group, reaps the child and
    raises the exception (`subprocess.TimeoutExpired` for the time limit).
    """
    with open(stdin_path, "rb") as fin, open(stdout_path, "wb") as fout, open(
        stderr_path, "wb"
    ) as ferr:
        actions = [
            (os.POSIX_SPAWN_DUP2, fin.fileno(), 0),
            (os.POSIX_SPAWN_DUP2, fout.fileno(), 1),
            (os.POSIX_SPAWN_DUP2, ferr.fileno(), 2),
        ]
        start = time.perf_counter()
        pid = os.posix_spawn(
            argv[0], list(argv), os.environ, file_actions=actions, setpgroup=0
        )
        status, usage, timed_out = _wait(pid, timeout)
        wall = time.perf_counter() - start
    if timed_out:
        raise subprocess.TimeoutExpired(list(argv), timeout)
    sample = Sample(
        cpu=usage.ru_utime + usage.ru_stime,
        user=usage.ru_utime,
        sys=usage.ru_stime,
        wall=wall,
        maxrss=usage.ru_maxrss * MAXRSS_UNIT,
    )
    return os.waitstatus_to_exitcode(status), sample


def _wait(pid, timeout):
    """Waits for a child and returns (status, rusage, timed_out)."""
    lock = threading.Lock()
    state = {"exited": False, "killed": False}

    def kill():
        with lock:
            # The child is not reaped before `exited` is true, so `pid` still
            # names the child and its process group.
            if state["exited"]:
                return
            state["killed"] = True
            try:
                os.killpg(pid, signal.SIGKILL)
            except ProcessLookupError:
                # The child moved to another process group.
                os.kill(pid, signal.SIGKILL)

    timer = threading.Timer(timeout, kill) if timeout else None
    if timer:
        timer.start()
    try:
        os.waitid(os.P_PID, pid, os.WEXITED | os.WNOWAIT)
    except BaseException:
        kill()
        os.wait4(pid, 0)
        raise
    finally:
        with lock:
            state["exited"] = True
        if timer:
            timer.cancel()
    _, status, usage = os.wait4(pid, 0)
    return status, usage, timed_out(state["killed"], status)


def timed_out(killed, status):
    """Returns True when the time limit stopped the child.

    The child can exit just before the timer sends SIGKILL. Then the status
    shows a normal exit, and the run is not a time-out.
    """
    return killed and os.WIFSIGNALED(status) and os.WTERMSIG(status) == signal.SIGKILL


def probe(argv, stdin_path, scratch, timeout=None):
    """Runs a command one time with its output kept, and classifies the result."""
    out = scratch / "probe.out"
    err = scratch / "probe.err"
    try:
        code, _ = run_once(argv, stdin_path, out, err, timeout=timeout)
    except subprocess.TimeoutExpired:
        return {"status": "error", "exit_code": None, "stderr": f"time limit of {timeout} s"}
    stdout = out.read_bytes()
    stderr = err.read_bytes().decode("utf-8", "replace")
    if code == 0:
        status = "ok"
    elif code in NOT_SUPPORTED_EXIT_CODES:
        status = "not-supported"
    else:
        status = "error"
    return {
        "status": status,
        "exit_code": code,
        "stderr": public_text(stderr[:2000]),
        "stdout_bytes": len(stdout),
        "stdout_sha256": hashlib.sha256(stdout).hexdigest(),
    }


def summarize(samples):
    """Returns the minimum and the median of each measure."""
    summary = {}
    for field in ("cpu", "user", "sys", "wall"):
        values = [s[field] for s in samples]
        summary[field] = {"min": min(values), "median": statistics.median(values)}
    rss = [s["maxrss"] for s in samples]
    summary["maxrss"] = {"median": statistics.median(rss), "max": max(rss)}
    return summary


@functools.lru_cache(maxsize=None)
def sign_cutoff(n, alpha):
    """Returns the largest k with P(B <= k - 1) <= alpha for B ~ Binomial(n, 1/2), or 0.

    The value is exact. 0 means that n pairs cannot give a bound with this
    confidence.
    """
    # Integer arithmetic: alpha * 2**n is too large for a float above 1023 pairs.
    numerator, denominator = alpha.as_integer_ratio()
    limit = numerator * 2**n
    cumulative = 0
    cutoff = 0
    for k in range(1, n + 1):
        cumulative += math.comb(n, k - 1)
        if cumulative * denominator > limit:
            break
        cutoff = k
    return cutoff


def paired_bounds(base, candidate, alpha=ALPHA):
    """Returns (lower, estimate, upper) for the median of the paired ratios candidate / base.

    Pair i is the run of round i of each program. The estimate is the median
    of the ratios. The bounds come from the sign test: with the sorted ratios
    r(1) <= ... <= r(n) and k = sign_cutoff(n, alpha), r(k) is a lower bound
    and r(n + 1 - k) an upper bound of the median ratio, each with confidence
    1 - alpha. The sign test needs independent pairs, but no symmetry and no
    other form of the distribution. A bound is 0 or infinity when there are too
    few pairs for the confidence.
    """
    ratios = sorted(max(c, CPU_RESOLUTION) / max(b, CPU_RESOLUTION) for b, c in zip(base, candidate))
    n = len(ratios)
    estimate = statistics.median(ratios)
    cutoff = sign_cutoff(n, alpha)
    if cutoff == 0:
        return 0.0, estimate, math.inf
    return ratios[cutoff - 1], estimate, ratios[n - cutoff]


def verdict(base, candidate, threshold=THRESHOLD, alpha=ALPHA):
    """Compares the paired CPU times of two programs and returns the comparison row.

    The data must prove the result. `slower`: the lower bound of the ratio is
    above 1 + threshold. `same`: the upper bound is at or below 1 + threshold.
    `faster`: the upper bound is below 1 / (1 + threshold). Else the result is
    `inconclusive`, and only more runs can decide.
    """
    lower, estimate, upper = paired_bounds(base, candidate, alpha)
    limit = 1 + threshold
    if lower > limit:
        result = "slower"
    elif upper < 1 / limit:
        result = "faster"
    elif upper <= limit:
        result = "same"
    else:
        result = "inconclusive"
    return {
        "ratio": estimate,
        "ratio_lower": lower,
        "ratio_upper": upper,
        "ratio_min": max(min(candidate), CPU_RESOLUTION) / max(min(base), CPU_RESOLUTION),
        "ratio_median": max(statistics.median(candidate), CPU_RESOLUTION)
        / max(statistics.median(base), CPU_RESOLUTION),
        "verdict": result,
    }


def slope(points):
    """Returns the least-squares slope of (x, y) points, or None for fewer than 2 x values."""
    xs = [x for x, _ in points]
    if len(set(xs)) < 2:
        return None
    mean_x = statistics.fmean(xs)
    mean_y = statistics.fmean(y for _, y in points)
    num = sum((x - mean_x) * (y - mean_y) for x, y in points)
    den = sum((x - mean_x) ** 2 for x in xs)
    return num / den


def derive(benchmarks, results):
    """Returns the conversion time of each benchmark and the slope of each series."""
    min_cpu = {
        name: r["summary"]["cpu"]["min"] for name, r in results.items() if r["status"] == "ok"
    }
    conversion = {
        b.name: min_cpu[b.name] - min_cpu[b.startup]
        for b in benchmarks
        if b.startup and b.name in min_cpu and b.startup in min_cpu
    }
    points = {}
    for b in benchmarks:
        if b.series and b.name in min_cpu:
            points.setdefault(b.series, []).append((b.scale, min_cpu[b.name]))
    slopes = {series: slope(p) for series, p in points.items()}
    return {
        "conversion_cpu_seconds": conversion,
        "slope_cpu_seconds_per_section": {s: v for s, v in slopes.items() if v is not None},
    }


def _command_output(argv):
    try:
        done = subprocess.run(argv, capture_output=True, text=True, timeout=10, check=False)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return done.stdout.strip() if done.returncode == 0 else None


def public_text(text):
    """Replaces the repository root with `<repo>` and the home folder with `~`."""
    return text.replace(str(ROOT), "<repo>").replace(os.path.expanduser("~"), "~")


def public_version(text):
    """Returns the version lines without lines that name a local folder.

    Pandoc prints `User data directory: PATH`, and PATH contains the home folder.
    """
    return [
        public_text(line)
        for line in text.splitlines()
        if not line.startswith("User data directory:")
    ]


def _utc(timestamp):
    return datetime.datetime.fromtimestamp(timestamp, datetime.timezone.utc).isoformat(
        timespec="seconds"
    )


def program_info(path):
    """Returns the name, the version lines and the Git state of a program.

    The record keeps no local path. `worktree_commit` is the HEAD of the Git
    work tree that contains the program when the tool runs, not a proven build
    commit. A program that is older than `worktree_commit_time`, or a work
    tree with changes, can come from a different commit.
    """
    real = os.path.realpath(path)
    version = _command_output([real, "--version"])
    info = {
        "name": os.path.basename(real),
        "version": public_version(version) if version else None,
        "modified": _utc(os.path.getmtime(real)),
        "worktree_commit": None,
        "worktree_commit_time": None,
        "worktree_dirty": None,
    }
    folder = os.path.dirname(real)
    commit = _command_output(["git", "-C", folder, "log", "-1", "--format=%H %ct"])
    if commit:
        sha, seconds = commit.split()
        info["worktree_commit"] = sha
        info["worktree_commit_time"] = _utc(int(seconds))
        changes = _command_output(["git", "-C", folder, "status", "--porcelain"])
        info["worktree_dirty"] = bool(changes)
    return info


def public_path(path):
    """Returns `path` relative to the repository root, or its file name when it is outside."""
    absolute = pathlib.Path(path).resolve()
    try:
        return str(absolute.relative_to(ROOT))
    except ValueError:
        return absolute.name


def environment():
    """Returns a description of the machine."""
    env = {
        "os": platform.platform(),
        "machine": platform.machine(),
        "python": platform.python_version(),
        "cpu_count": os.cpu_count(),
        "cpu_model": None,
        "memory_bytes": None,
        "load_average": list(os.getloadavg()),
    }
    if sys.platform == "darwin":
        env["cpu_model"] = _command_output(["sysctl", "-n", "machdep.cpu.brand_string"])
        memory = _command_output(["sysctl", "-n", "hw.memsize"])
        env["memory_bytes"] = int(memory) if memory else None
        for level in ("perflevel0", "perflevel1"):
            cores = _command_output(["sysctl", "-n", f"hw.{level}.physicalcpu"])
            if cores:
                env[f"{level}_cores"] = int(cores)
    else:
        try:
            cpuinfo = pathlib.Path("/proc/cpuinfo").read_text(encoding="utf-8")
            meminfo = pathlib.Path("/proc/meminfo").read_text(encoding="utf-8")
        except OSError:
            return env
        for line in cpuinfo.splitlines():
            if line.startswith("model name"):
                env["cpu_model"] = line.split(":", 1)[1].strip()
                break
        for line in meminfo.splitlines():
            if line.startswith("MemTotal:"):
                env["memory_bytes"] = int(line.split()[1]) * 1024
                break
    return env


def resolve_program(path):
    """Returns the absolute path of a program, or None when it does not exist."""
    found = shutil.which(path) if os.sep not in path else path
    if found is None or not os.access(found, os.X_OK):
        return None
    return os.path.abspath(found)


class Measurement:
    """The probe results and the timed runs of each program on each benchmark."""

    def __init__(self, programs, benchmarks, scratch, timeout=None):
        self.programs = programs
        self.timeout = timeout
        self.stdin_paths = {}
        for index, bench in enumerate(benchmarks):
            if bench.input is not None:
                self.stdin_paths[bench.name] = str(bench.input)
            else:
                path = scratch / f"stdin-{index}"
                path.write_text(bench.stdin or "", encoding="utf-8")
                self.stdin_paths[bench.name] = str(path)
        self.results = {role: {} for role in programs}
        for role, path in programs.items():
            for bench in benchmarks:
                result = probe([path, *bench.args], self.stdin_paths[bench.name], scratch, timeout)
                result["argv"] = [role.upper(), *bench.args]
                result["samples"] = []
                self.results[role][bench.name] = result
        self.rounds = 0

    def run_rounds(self, benchmarks, count, progress=None):
        """Runs `count` more rounds of the given benchmarks.

        In each round, each program runs each benchmark one time. The order of
        the programs changes in each round, so that a change of machine load
        affects each program equally. Sample i of each program is from round i.
        """
        order = list(self.programs.items())
        for _ in range(count):
            self.rounds += 1
            if progress:
                progress(f"round {self.rounds}")
            for bench in benchmarks:
                for role, path in order if self.rounds % 2 else order[::-1]:
                    result = self.results[role][bench.name]
                    if result["status"] == "ok":
                        self._timed_run(result, [path, *bench.args], self.stdin_paths[bench.name])

    def _timed_run(self, result, argv, stdin_path):
        try:
            code, sample = run_once(argv, stdin_path, timeout=self.timeout)
        except subprocess.TimeoutExpired:
            code, sample = None, None
        if code != 0:
            result["status"] = "error"
            result["exit_code"] = code
            result["stderr"] = (
                f"time limit of {self.timeout} s in a timed run"
                if code is None
                else f"exit code {code} in a timed run"
            )
            return
        result["samples"].append(dataclasses.asdict(sample))

    def finish(self):
        """Adds the summary of each successful result and returns the results."""
        for role_results in self.results.values():
            for result in role_results.values():
                if result["status"] == "ok":
                    result["summary"] = summarize(result["samples"])
        return self.results


def look_sizes(runs, max_runs):
    """Returns the round counts after which the comparison looks at the data.

    The count doubles from `runs` up to `max_runs`.
    """
    sizes = [runs]
    while sizes[-1] < max_runs:
        sizes.append(min(sizes[-1] * 2, max_runs))
    return sizes


def measure_and_compare(measurement, benchmarks, runs, max_runs, alpha=ALPHA, progress=None):
    """Runs rounds until each comparison is decided or `max_runs` rounds are done.

    The comparison looks at the data after each size of `look_sizes`. Only the
    inconclusive benchmarks get more rounds. Each look uses `alpha` divided by
    the number of looks, so the error rate of the whole invocation stays at
    `alpha` or below, for each benchmark and each direction.
    """
    sizes = look_sizes(runs, max_runs)
    alpha = alpha / len(sizes)
    pending = list(benchmarks)
    rows = {}
    for size in sizes:
        measurement.run_rounds(pending, size - measurement.rounds, progress)
        for row in compare(pending, measurement.results, alpha):
            rows[row["name"]] = row
        pending = [b for b in pending if rows[b.name]["verdict"] == "inconclusive"]
        if not pending:
            break
    return [rows[b.name] for b in benchmarks], alpha


def compare(benchmarks, results, alpha=ALPHA):
    """Returns one comparison row for each benchmark."""
    rows = []
    for bench in benchmarks:
        tested = results["program"][bench.name]
        reference = results["reference"][bench.name]
        row = {"name": bench.name}
        statuses = {tested["status"], reference["status"]}
        if statuses == {"ok"}:
            row.update(
                verdict(
                    [s["cpu"] for s in reference["samples"]],
                    [s["cpu"] for s in tested["samples"]],
                    alpha=alpha,
                )
            )
            row["runs"] = len(tested["samples"])
            row["reference_min"] = min(s["cpu"] for s in reference["samples"])
            row["program_min"] = min(s["cpu"] for s in tested["samples"])
            row["same_output"] = tested["stdout_sha256"] == reference["stdout_sha256"]
        elif "error" in statuses:
            row["verdict"] = "error"
        elif reference["status"] == "ok":
            row["verdict"] = "lost-support"
        elif tested["status"] == "ok":
            row["verdict"] = "new"
        else:
            row["verdict"] = "not-supported"
        rows.append(row)
    return rows


def _ms(seconds):
    return f"{seconds * 1000:9.2f}"


def format_table(benchmarks, results, rows=None):
    """Returns the text table that the tool prints."""
    lines = []
    if rows is None:
        lines.append(f"{'benchmark':28} {'cpu min ms':>10} {'cpu med ms':>10} {'wall med ms':>11} {'rss MiB':>8}")
        for bench in benchmarks:
            result = results["program"][bench.name]
            if result["status"] != "ok":
                lines.append(f"{bench.name:28} {result['status']}")
                continue
            s = result["summary"]
            lines.append(
                f"{bench.name:28} {_ms(s['cpu']['min']):>10} {_ms(s['cpu']['median']):>10} "
                f"{_ms(s['wall']['median']):>11} {s['maxrss']['median'] / 2**20:8.1f}"
            )
        return "\n".join(lines)
    lines.append(
        f"{'benchmark':28} {'ref min ms':>10} {'prog min ms':>11} {'ratio':>6} {'99% bounds':>15}  verdict"
    )
    for row in rows:
        if "ratio" not in row:
            lines.append(f"{row['name']:28} {'':>10} {'':>11} {'':>6} {'':>15}  {row['verdict']}")
            continue
        note = "" if row["same_output"] else " (output differs)"
        bounds = f"{row['ratio_lower']:.3f} to {row['ratio_upper']:.3f}"
        lines.append(
            f"{row['name']:28} {_ms(row['reference_min']):>10} {_ms(row['program_min']):>11} "
            f"{row['ratio']:6.3f} {bounds:>15}  {row['verdict']}{note}"
        )
    return "\n".join(lines)


def exit_code(results, rows):
    """Returns the exit code of the tool.

    1: a benchmark is slower, or the program under test lost the support of a
    benchmark that the reference supports. 2: a benchmark failed.
    3: a comparison is inconclusive. 0: none of these.
    """
    verdicts = {row["verdict"] for row in rows or []}
    if verdicts & {"slower", "lost-support"}:
        return 1
    if any(r["status"] == "error" for role in results.values() for r in role.values()):
        return 2
    if "inconclusive" in verdicts:
        return 3
    return 0


def _now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")


def main(argv=None):
    """Runs the tool and returns the exit code."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--program", required=True, help="the program under test")
    parser.add_argument("--compare", help="the reference program, for example a build of main")
    parser.add_argument("--filter", default="", help="run only benchmarks whose name contains TEXT")
    parser.add_argument("--runs", type=int, default=30, help="timed runs for each benchmark (8 or more, 30 or more to decide)")
    parser.add_argument(
        "--max-runs", type=int, help="with --compare: more runs for inconclusive benchmarks, up to this count"
    )
    parser.add_argument(
        "--alpha",
        type=float,
        default=ALPHA,
        help="with --compare: the error allowance of this invocation (see docs/BENCHMARKS.md)",
    )
    parser.add_argument("--timeout", type=float, default=60.0, help="time limit of one run in seconds")
    parser.add_argument("--benchmarks", default=str(DEFAULT_BENCHMARKS), help="the benchmarks file")
    parser.add_argument("--out", required=True, help="the JSON file for the raw data")
    args = parser.parse_args(argv)
    if args.runs < 8:
        parser.error("--runs must be 8 or more. Fewer pairs give no sign-test bound at small alpha values")
    if not 0 < args.alpha < 1:
        parser.error("--alpha must be between 0 and 1")
    max_runs = args.max_runs or args.runs
    if max_runs < args.runs:
        parser.error("--max-runs must be equal to or more than --runs")
    programs = {}
    for role, given in (("program", args.program), ("reference", args.compare)):
        if given is None:
            continue
        path = resolve_program(given)
        if path is None:
            parser.error(f"{given}: not an executable program")
        programs[role] = path
    try:
        benchmarks = select(load_benchmarks(args.benchmarks), args.filter)
    except (OSError, tomllib.TOMLDecodeError, ConfigError) as error:
        parser.error(f"{args.benchmarks}: {error}")
    if not benchmarks:
        parser.error(f"no benchmark name contains {args.filter!r}")
    for bench in benchmarks:
        if bench.input is not None and not bench.input.is_file():
            parser.error(f"{bench.name}: the input {bench.input} does not exist")

    record = {
        "schema": 1,
        "started": _now(),
        "settings": {
            "runs": args.runs,
            "max_runs": max_runs,
            "timeout": args.timeout,
            "filter": args.filter,
            "benchmarks_file": public_path(args.benchmarks),
            "threshold": THRESHOLD,
            "alpha": args.alpha,
        },
        "environment": environment(),
        "programs": {role: program_info(path) for role, path in programs.items()},
        "benchmarks": [
            {
                "name": b.name,
                "args": list(b.args),
                "stdin": b.stdin_label(pathlib.Path(args.benchmarks).parent),
                "series": b.series,
                "scale": b.scale,
                "startup": b.startup,
            }
            for b in benchmarks
        ],
    }
    def progress(text):
        print(text, file=sys.stderr, flush=True)

    rows = None
    with tempfile.TemporaryDirectory(prefix="randoc-bench-") as scratch:
        measurement = Measurement(programs, benchmarks, pathlib.Path(scratch), args.timeout)
        if "reference" in programs:
            rows, alpha = measure_and_compare(
                measurement, benchmarks, args.runs, max_runs, args.alpha, progress
            )
            record["settings"]["method"] = "sign test on paired ratios"
            record["settings"]["alpha_per_look"] = alpha
        else:
            measurement.run_rounds(benchmarks, args.runs, progress)
        results = measurement.finish()
    record["finished"] = _now()
    record["environment"]["load_average_end"] = list(os.getloadavg())
    record["results"] = results
    record["derived"] = {role: derive(benchmarks, r) for role, r in results.items()}
    record["comparison"] = rows
    pathlib.Path(args.out).write_text(json.dumps(record, indent=1) + "\n", encoding="utf-8")
    print(format_table(benchmarks, results, rows))
    return exit_code(results, rows)


if __name__ == "__main__":
    sys.exit(main())
