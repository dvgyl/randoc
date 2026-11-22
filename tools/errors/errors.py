"""Compare exit code probes and conversion logs with Pandoc 3.13."""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor
from contextlib import nullcontext
from dataclasses import dataclass
from pathlib import Path

from triggers import CODES, TRIGGERS, UNCONFIRMED, Command, charset_server, convert, http

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / "tools/pivot"), str(ROOT / "tools/readers")]
from inputs import corpuslib, document_data
from observe import LAUNCH, LIMIT_RUN, MAX_OUTPUT_BYTES, NoResult, snapshot
from writers import file_hash, prepare

REPORT = "errors-report.json"
TARGETS = ("html", "latex", "markdown")
PARTS = ("exit", "stdout", "stderr", "files")
LOG_OPTIONS = ("--verbose", "--log=log.json")


@dataclass(frozen=True)
class Unit:
    id: str
    command: Command


def digest_stream(stream):
    stream.seek(0)
    return snapshot.ByteBudget().digest(stream)


def file_changes(before, after):
    return {path: after.get(path) for path in sorted(before.keys() | after.keys())
            if before.get(path) != after.get(path)}


def capture(binary, command, *, oracle=False):
    """Run one command in a new work folder. Keep hashes of output bytes."""
    try:
        program = binary.resolve(strict=True)
    except OSError as error:
        raise NoResult(f"The program is missing: {error}") from error
    scratch = (tempfile.TemporaryDirectory(prefix="randoc-errors-scratch-")
               if command.local_temp else nullcontext())
    with scratch as temp, tempfile.TemporaryDirectory(prefix="randoc-errors-") as folder, \
         tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        work = Path(folder)
        for name, data in command.files:
            path = work / name
            path.write_bytes(data)
            if name == command.executable:
                path.chmod(0o700)
        before = snapshot.snapshot(folder, set()) if command.files else {}
        env = {
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "HOME": str(work / ".home"),
            "LANG": "en_US.UTF-8",
            "LC_ALL": "en_US.UTF-8",
            "TZ": "UTC",
            "SOURCE_DATE_EPOCH": "1700000000",
            "GHCRTS": "-M3g",
            "RANDOC_DATA_DIR": os.environ.get("RANDOC_DATA_DIR")
            or str(Path(os.environ.get("RANDOC_ENV", ROOT.parent / "env")) / "datapack"),
        }
        if temp is not None:
            env["TMPDIR"] = temp
        env.update(command.env)
        argv = [sys.executable, str(LIMIT_RUN), "3000", "600", "/bin/sh", "-c",
                LAUNCH, "errors", str(program), *command.args]
        result = subprocess.run(argv, input=command.stdin, stdout=stdout, stderr=stderr, cwd=work, env=env)
        code = result.returncode
        if code == 125:
            stderr.seek(0)
            reason = stderr.read(4096).decode("utf-8", errors="replace").strip()
            raise RuntimeError(f"The limit tool failed to start {program.name}: {reason}")
        if code < 0 or 128 <= code < 192 or (oracle and code == 251):
            raise NoResult(f"{program.name} stopped with exit code {code}")
        if max(stdout.tell(), stderr.tell()) > MAX_OUTPUT_BYTES:
            raise NoResult(f"{program.name} exceeded the output byte limit")
        try:
            after = snapshot.snapshot(folder, set())
            return {"exit": code, "stdout": digest_stream(stdout), "stderr": digest_stream(stderr),
                    "files": file_changes(before, after)}
        except (OSError, ValueError) as error:
            raise NoResult(f"Cannot capture output from {program.name}: {error}") from error


def attempt(binary, unit, *, oracle=False):
    try:
        return capture(binary, unit.command, oracle=oracle)
    except NoResult as error:
        return str(error)


def oracle_pair(oracle, unit):
    first = attempt(oracle, unit, oracle=True)
    time.sleep(1)
    return first, attempt(oracle, unit, oracle=True)


def differences(first, second):
    return [part for part in PARTS if first[part] != second[part]]


def classify(unit, first, second, actual):
    row = {"id": unit.id, "oracle": first}
    if isinstance(first, str) or isinstance(second, str):
        return {**row, "status": "oracle_limit", "oracle_second": second,
                "reason": "An oracle run gave no result."}
    changed = differences(first, second)
    if changed:
        return {**row, "status": "unstable", "oracle_second": second,
                "unstable_parts": changed, "reason": "The two oracle runs differ.",
                "randoc": actual, "unstable_parts_ok": isinstance(actual, dict)
                and set(differences(first, actual)) <= set(changed)}
    if isinstance(actual, str):
        return {**row, "status": "no_result", "randoc": actual}
    changed = differences(first, actual)
    return {**row, "status": "fail" if changed else "pass", "randoc": actual,
            "differences": changed}


def matching_unit(identifier, selected):
    return selected is None or selected in identifier


def log_units(args):
    if args.filter is not None and args.filter.startswith("exit/"):
        return
    if args.filter is None or not args.filter.startswith("log/pivot/"):
        documents = corpuslib.load_manifest()
        for document in documents:
            identifier = f"log/corpus/{document.name}"
            if matching_unit(identifier, args.filter):
                data = document_data(document, args.corpus)
                yield Unit(identifier, convert(*LOG_OPTIONS, document.file_name, source=document.format,
                                               stdin=b"", files=((document.file_name, data),)))
    if args.filter is not None and args.filter.startswith("log/corpus/"):
        return
    _, trees, _ = prepare(args)
    for name, data in trees:
        for target in TARGETS:
            identifier = f"log/pivot/{target}/{name}"
            if matching_unit(identifier, args.filter):
                yield Unit(identifier, convert(*LOG_OPTIONS, source="json", target=target, stdin=data))


def confirm_triggers(oracle, selected, charset_url):
    confirmed, evidence = [], {}
    for code in CODES:
        if not matching_unit(f"exit/{code}", selected):
            continue
        probes = []
        entry = evidence[str(code)] = {"probes": probes}
        candidates = ({"unknown-http-charset": http(charset_url)} if code == 94 else TRIGGERS[code])
        for name, command in candidates.items():
            unit = Unit(f"exit/{code}", command)
            first, second = oracle_pair(oracle, unit)
            probes.append({"name": name, "args": command.args, "runs": [first, second]})
            if all(isinstance(run, dict) and run["exit"] == code for run in (first, second)):
                confirmed.append((unit, first, second))
                entry["confirmed"] = name
                break
        if "confirmed" not in entry:
            entry["reason"] = UNCONFIRMED.get(code, f"No probe returned exit code {code} twice.")
    return confirmed, evidence


def compare_units(args, confirmed, units):
    rows, retries = [], []

    def compare(unit, first, second):
        actual = None
        if isinstance(first, dict) and isinstance(second, dict):
            actual = attempt(args.randoc, unit)
        return classify(unit, first, second, actual), actual

    def capture_oracle(unit):
        return attempt(args.oracle, unit, oracle=True)

    def record(unit, row, actual):
        if isinstance(actual, str):
            retries.append((len(rows), unit, row["oracle"], row.get("oracle_second", row["oracle"])))
        rows.append(row)

    for unit, first, second in confirmed:
        record(unit, *compare(unit, first, second))
    with ThreadPoolExecutor(max_workers=2) as pool:
        first_runs = list(pool.map(capture_oracle, units))
        if units:
            time.sleep(1)
        second_runs = list(pool.map(capture_oracle, units))
        for unit, (row, actual) in zip(units, pool.map(compare, units, first_runs, second_runs)):
            record(unit, row, actual)
    for index, unit, first, second in retries:
        rows[index], _ = compare(unit, first, second)
    return rows


def measure(args):
    expected = json.loads((ROOT / "spec/inventory/oracle.json").read_bytes())["sha256"]
    if file_hash(args.oracle) != expected:
        raise ValueError("The oracle SHA-256 differs from spec/inventory/oracle.json.")
    units = list(log_units(args))
    server = charset_server() if matching_unit("exit/94", args.filter) else nullcontext()
    with server as charset_url:
        confirmed, coverage = confirm_triggers(args.oracle, args.filter, charset_url)
        rows = compare_units(args, confirmed, units)
    return {"units": rows, "summary": {"units": len(rows), "selected": args.filter},
            "unstable_parts_ok": sum(row.get("unstable_parts_ok", False) for row in rows),
            "exit_codes": coverage, "oracle_sha256": expected}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--randoc", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--filter")
    args = parser.parse_args()
    try:
        report = measure(args)
        (args.evidence / REPORT).write_text(json.dumps(report) + "\n", encoding="utf-8")
    except (KeyError, OSError, RuntimeError, ValueError, corpuslib.ManifestError) as error:
        parser.exit(2, f"errors: {error}\n")


if __name__ == "__main__":
    main()
