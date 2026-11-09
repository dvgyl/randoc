#!/usr/bin/env python3
"""Converts each corpus document with a program and with the oracle, and compares the results.

Each document goes from its format to each target format. A unit passes when
the exit code, the standard error output and the output are equal. The
program compares the `json` output (the document tree) by JSON equality and
the other outputs by bytes.

The program caches the oracle results in $RANDOC_CORPUS/.oracle-cache, an
SQLite database, so that a second run is fast.
"""

import argparse
import collections
import concurrent.futures
import dataclasses
import hashlib
import json
import os
import signal
import sqlite3
import subprocess
import sys
import threading
from pathlib import Path

import corpuslib

TARGETS = ("json", "html", "markdown")
SOURCE_DATE_EPOCH = "1700000000"
CACHE_NAME = ".oracle-cache"
# A fixed HOME that does not depend on the run, because Pandoc prints the
# path of its user data directory in some messages and in `--version`.
HOME = Path("/var/empty")
MAX_OUTPUT_BYTES = 64_000_000
CLEANUP_SECONDS = 5


@dataclasses.dataclass(frozen=True)
class Outcome:
    """The result of one program run."""

    exit: int
    stdout: bytes
    stderr: bytes
    timed_out: bool = False


def environment(home):
    """Returns the fixed environment of each run, as the conformance harness uses it."""
    return {
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
        "HOME": str(home),
        "LANG": "en_US.UTF-8",
        "LC_ALL": "en_US.UTF-8",
        "TZ": "UTC",
        "SOURCE_DATE_EPOCH": SOURCE_DATE_EPOCH,
    }


class _PipeReader(threading.Thread):
    """Reads a pipe to its end and keeps at most `limit` bytes."""

    def __init__(self, pipe, limit):
        super().__init__(daemon=True)
        self.pipe = pipe
        self.limit = limit
        self.data = bytearray()
        self.overflow = False

    def run(self):
        try:
            while chunk := self.pipe.read(65536):
                room = self.limit - len(self.data)
                self.data += chunk[:max(room, 0)]
                self.overflow = self.overflow or len(chunk) > room
        except (OSError, ValueError):
            pass


def execute(binary, args, cwd, home, timeout, limit=MAX_OUTPUT_BYTES):
    """Runs `binary` under the name `pandoc` and returns the outcome.

    The program runs in its own session. At the time limit, the function
    stops the whole process group, so that a child process cannot keep the
    pipes open. Each pipe keeps at most `limit` bytes.
    """
    try:
        process = subprocess.Popen(
            ["pandoc", *args], executable=str(binary), cwd=cwd, env=environment(home),
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            start_new_session=True)
    except OSError as error:
        return Outcome(-1, b"", f"[corpus] {binary}: {error}\n".encode())
    readers = [_PipeReader(process.stdout, limit), _PipeReader(process.stderr, limit)]
    for reader in readers:
        reader.start()
    timed_out = False
    try:
        process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass
    process.wait()
    for reader in readers:
        reader.join(CLEANUP_SECONDS)
    for pipe in (process.stdout, process.stderr):
        pipe.close()
    stdout, stderr = (bytes(reader.data) for reader in readers)
    if timed_out:
        stderr += f"[corpus] no result after {timeout} s\n".encode()
        return Outcome(-1, stdout, stderr, timed_out=True)
    if any(reader.overflow for reader in readers):
        stderr += f"[corpus] output larger than {limit} bytes\n".encode()
        return Outcome(-1, stdout, stderr)
    return Outcome(process.returncode, stdout, stderr)


def arguments(document, target):
    """Returns the command-line arguments that convert `document` to `target`."""
    return ["-f", document.format, "-t", target, document.file_name]


def json_equal(left, right):
    """Equality of decoded JSON values that keeps Booleans, integers, floats and strings apart.

    In Python, `True == 1` and `1 == 1.0`. The comparison uses a stack, so
    deep nesting cannot exhaust the Python stack.
    """
    stack = [(left, right)]
    while stack:
        a, b = stack.pop()
        if type(a) is not type(b):
            return False
        if isinstance(a, dict):
            if a.keys() != b.keys():
                return False
            stack.extend((a[key], b[key]) for key in a)
        elif isinstance(a, list):
            if len(a) != len(b):
                return False
            stack.extend(zip(a, b))
        elif a != b:
            return False
    return True


def differences(expected, actual, target):
    """Returns the parts of `actual` that differ from `expected`: exit, stderr, output."""
    parts = []
    if expected.exit != actual.exit:
        parts.append("exit")
    if expected.stderr != actual.stderr:
        parts.append("stderr")
    same = expected.stdout == actual.stdout
    if not same and target == "json":
        try:
            same = json_equal(json.loads(expected.stdout), json.loads(actual.stdout))
        except (ValueError, RecursionError):
            same = False
    if not same:
        parts.append("output")
    return parts


def status(expected, actual, target):
    """Returns the one-line status of a unit."""
    if actual.timed_out:
        return "timeout"
    if expected.timed_out:
        return "oracle timeout"
    parts = differences(expected, actual, target)
    return "pass" if not parts else "fail " + ",".join(parts)


class OracleCache:
    """The oracle results, stored in one SQLite file."""

    def __init__(self, path, fingerprint):
        self.fingerprint = fingerprint
        self.connection = sqlite3.connect(path)
        self.connection.execute(
            "CREATE TABLE IF NOT EXISTS result "
            "(key TEXT PRIMARY KEY, exit INTEGER, stdout BLOB, stderr BLOB)")

    def key(self, content_hash, args):
        """Returns the cache key: the oracle, the hash of the document file and the arguments."""
        material = json.dumps([self.fingerprint, content_hash, args])
        return corpuslib.sha256_of(material.encode())

    def keys(self):
        """Returns the set of the stored keys."""
        return {row[0] for row in self.connection.execute("SELECT key FROM result")}

    def get(self, key):
        """Returns the stored outcome for `key`, or None."""
        row = self.connection.execute(
            "SELECT exit, stdout, stderr FROM result WHERE key = ?", (key,)).fetchone()
        return None if row is None else Outcome(row[0], bytes(row[1]), bytes(row[2]))

    def put(self, key, outcome):
        """Stores `outcome`. An outcome after a timeout is not stored."""
        if outcome.timed_out:
            return
        self.connection.execute(
            "INSERT OR REPLACE INTO result VALUES (?, ?, ?, ?)",
            (key, outcome.exit, outcome.stdout, outcome.stderr))
        self.connection.commit()

    def close(self):
        """Closes the database."""
        self.connection.close()


def version_line(binary, home):
    """Returns the first line of `binary --version`."""
    outcome = execute(binary, ["--version"], home, home, 60)
    lines = outcome.stdout.decode("utf-8", "replace").splitlines()
    return lines[0] if lines else f"unknown (exit {outcome.exit})"


def fingerprint(binary):
    """Identifies the oracle program by the SHA-256 hash of its file."""
    digest = hashlib.sha256()
    with open(binary, "rb") as file:
        while chunk := file.read(1 << 20):
            digest.update(chunk)
    return digest.hexdigest()


def verify_documents(documents, corpus):
    """Returns the names of the documents whose file is missing or differs from the manifest."""
    bad = []
    for document in documents:
        path = corpus / document.file_name
        try:
            data = path.read_bytes()
        except OSError:
            bad.append(document.name)
            continue
        if len(data) != document.size or corpuslib.sha256_of(data) != document.sha256:
            bad.append(document.name)
    return bad


def summarize(units, program, oracle, targets, timeout):
    """Returns the report: totals and one status line for each document and target.

    `units` is a list of (document, target, status) triples.
    """
    totals = collections.Counter()
    by_target = collections.defaultdict(collections.Counter)
    by_format = collections.defaultdict(collections.Counter)
    results = {}
    for document, target, unit_status in sorted(units, key=lambda u: (u[0].name, u[1])):
        outcome = "pass" if unit_status == "pass" else "fail"
        for counter in (totals, by_target[target], by_format[document.format]):
            counter["units"] += 1
            counter[outcome] += 1
        results[f"{document.name}:{target}"] = unit_status

    def counts(counter):
        return {"units": counter["units"], "pass": counter["pass"], "fail": counter["fail"]}

    return {
        "program": program,
        "oracle": oracle,
        "targets": list(targets),
        "timeout_seconds": timeout,
        "documents": len({document.name for document, _, _ in units}),
        "totals": counts(totals),
        "by_target": {name: counts(c) for name, c in sorted(by_target.items())},
        "by_format": {name: counts(c) for name, c in sorted(by_format.items())},
        "results": results,
    }


def run_units(documents, targets, program, oracle, corpus, cache, jobs, timeout, home):
    """Runs each unit and returns the list of (document, target, status) triples."""
    stored = cache.keys()
    tasks = []
    for document in documents:
        for target in targets:
            args = arguments(document, target)
            tasks.append((document, target, args, cache.key(document.sha256, args)))

    def work(task):
        _, target, args, key = task
        try:
            actual = execute(program, args, corpus, home, timeout)
            expected = None if key in stored else execute(oracle, args, corpus, home, timeout)
            unit_status = None if expected is None else status(expected, actual, target)
            return actual, expected, unit_status
        except Exception as error:  # one unit must not stop the run
            return None, None, f"error {type(error).__name__}: {error}"

    units = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        for task, (actual, expected, unit_status) in zip(tasks, pool.map(work, tasks)):
            document, target, _, key = task
            if actual is not None and expected is None:
                try:
                    unit_status = status(cache.get(key), actual, target)
                except Exception as error:  # one unit must not stop the run
                    unit_status = f"error {type(error).__name__}: {error}"
            elif expected is not None:
                cache.put(key, expected)
            units.append((document, target, unit_status))
    return units


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--pandoc", type=Path, required=True, help="the program to test")
    parser.add_argument("--oracle", type=Path, required=True, help="Pandoc 3.13")
    parser.add_argument("--out", type=Path, help="write the report to this JSON file")
    parser.add_argument("--manifest", type=Path, default=corpuslib.MANIFEST)
    parser.add_argument("--corpus", type=Path, default=os.environ.get("RANDOC_CORPUS"),
                        help="the corpus directory (default: $RANDOC_CORPUS)")
    parser.add_argument("--jobs", type=int, default=4, help="the number of parallel runs")
    parser.add_argument("--timeout", type=int, default=180, help="seconds for one run")
    parser.add_argument("--target", action="append", choices=TARGETS,
                        help="a target format (default: all)")
    parser.add_argument("--only", action="append", default=[],
                        help="use only the documents whose name contains this text")
    parser.add_argument("--expect-all-pass", action="store_true",
                        help="exit with status 1 when a unit does not pass")
    args = parser.parse_args(argv)
    if args.corpus is None:
        parser.error("set RANDOC_CORPUS or use --corpus")

    try:
        documents = corpuslib.load_manifest(args.manifest)
    except corpuslib.ManifestError as error:
        print(error, file=sys.stderr)
        return 2
    if args.only:
        documents = [d for d in documents if any(part in d.name for part in args.only)]
    bad = verify_documents(documents, args.corpus)
    if bad:
        print(f"missing or changed documents, run fetch.py: {', '.join(bad)}", file=sys.stderr)
        return 2

    targets = tuple(args.target or TARGETS)
    program = args.pandoc.resolve()
    oracle = args.oracle.resolve()
    cache = OracleCache(args.corpus / CACHE_NAME, fingerprint(oracle))
    try:
        units = run_units(documents, targets, program, oracle, args.corpus.resolve(),
                          cache, args.jobs, args.timeout, HOME)
    finally:
        cache.close()
    report = summarize(units, version_line(program, HOME), version_line(oracle, HOME),
                       targets, args.timeout)

    text = json.dumps(report, indent=1, ensure_ascii=False) + "\n"
    if args.out:
        args.out.write_text(text, encoding="utf-8")
    totals = report["totals"]
    print(f"{report['documents']} documents, {totals['units']} units: "
          f"{totals['pass']} pass, {totals['fail']} fail")
    for target, counts in report["by_target"].items():
        print(f"  {target}: {counts['pass']}/{counts['units']}")
    return 1 if args.expect_all_pass and totals["fail"] else 0


if __name__ == "__main__":
    sys.exit(main())
