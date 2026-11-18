#!/usr/bin/env python3
"""Measure every non-PDF writer on the fixed pivot trees."""

import argparse
import hashlib
import json
import os
import subprocess
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from dataclasses import dataclass
from pathlib import Path

from generate import ROOT, generate, json_bytes, oracle_accepts
from normalize import RULE_ID, notebook_rule
from observe import NoResult, run
from reference import (Reference, cache_key, cache_version, classify, compare, open_cache,
                       read_reference, read_status, verify, write_references)


@dataclass(frozen=True)
class Unit:
    name: str
    data: bytes
    key: bytes


def file_hash(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def attempt(binary, args, unit, oracle=False):
    try:
        return run(binary, args, unit.data, oracle=oracle)
    except NoResult as error:
        return str(error)


def batches(items, size):
    for start in range(0, len(items), size):
        yield items[start:start + size]


def first_run(result):
    if isinstance(result, str):
        return Reference("oracle_limit", None, reason=result)
    return Reference("first", result)


def record_pairs(database, units, capture, fmt, pool, rule):
    with tempfile.TemporaryDirectory(prefix="randoc-oracle-pair-") as folder, \
         closing(open_cache(Path(folder) / "first.sqlite")) as first:
        for group in batches(units, 2):
            write_references(first, [(unit.key, first_run(result))
                                     for unit, result in zip(group, pool.map(capture, group))])
        time.sleep(1)
        for group in batches(units, 2):
            fresh = []
            for unit, second in zip(group, pool.map(capture, group)):
                saved = read_reference(first, unit.key)
                initial = saved.reason if saved.first is None else saved.first
                fresh.append((unit.key, classify(fmt, initial, second, rule)))
            write_references(database, fresh)


def references(database, units, oracle, args, pool, rule, verify_saved=False):
    def capture(unit):
        return attempt(oracle, args, unit, oracle=True)

    fmt = args[-1]
    statuses = {unit.key: read_status(database, unit.key) for unit in units}
    pending = [unit for unit in units if statuses[unit.key] in (None, "oracle_limit")]
    if pending:
        record_pairs(database, pending, capture, fmt, pool, rule)
    if verify_saved:
        saved = [unit for unit in units if statuses[unit.key] in ("stable", "unstable")]
        for group in batches(saved, 2):
            originals = {unit.key: read_reference(database, unit.key) for unit in group}
            ready = [unit for unit in group if originals[unit.key].first is not None]
            checked = [(unit.key, verify(fmt, originals[unit.key], result, rule))
                       for unit, result in zip(ready, pool.map(capture, ready))]
            write_references(database, checked, refresh=True)


def verdict(unit, fmt, reference, actual, rule):
    status = reference.status
    row = {"id": unit.name, "format": fmt, "status": status}
    if reference.first is None:
        row["reason"] = reference.reason
        return row
    if reference.cell_positions:
        row["normalization"] = RULE_ID
        row["normalized_cell_positions"] = list(reference.cell_positions)
    row["oracle_stdout_sha256"] = hashlib.sha256(reference.first.stdout).hexdigest()
    if status == "unstable":
        row["unstable_parts"] = list(reference.unstable_parts)
        row["reason"] = reference.reason
    if isinstance(actual, str):
        row["reason_randoc"] = actual
        if status == "stable":
            row["status"] = "no_result"
        return row
    parts = compare(fmt, reference, actual, rule)
    row["differences"] = parts
    if status == "unstable":
        row["unstable_parts_ok"] = all(part in reference.unstable_parts for part in parts)
    else:
        row["status"] = "fail" if parts else "pass"
    return row


def measure(args, writers, trees, database, version, rule):
    rows, retries = [], []
    with ThreadPoolExecutor(max_workers=2) as oracle_pool, ThreadPoolExecutor(max_workers=4) as randoc_pool:
        for fmt in writers:
            command = ["-f", "json", "-t", fmt]
            units = []
            for identifier, data in trees:
                name = f"writer/{fmt}/{identifier}"
                if args.filter is None or args.filter in name:
                    units.append(Unit(name, data, cache_key(version, command, data)))
            if not units:
                continue
            references(database, units, args.oracle, command, oracle_pool, rule, args.verify_oracle)

            def capture(unit):
                return attempt(args.randoc, command, unit)

            for group in batches(units, 4):
                expected = {unit.key: read_reference(database, unit.key) for unit in group}
                ready = [unit for unit in group if expected[unit.key].first is not None]
                actual = dict(zip((unit.name for unit in ready), randoc_pool.map(capture, ready)))
                for unit in group:
                    value = actual.get(unit.name)
                    if isinstance(value, str):
                        retries.append((len(rows), unit, command))
                    rows.append(verdict(unit, fmt, expected[unit.key], value, rule))
            print(f"{fmt}: checked {len(units)} trees", flush=True)
    for index, unit, command in retries:
        recorded = read_reference(database, unit.key)
        rows[index] = verdict(unit, command[-1], recorded, attempt(args.randoc, command, unit), rule)
    return rows


def prepare(args):
    inventory = ROOT / "spec/inventory"
    expected = json.loads((inventory / "oracle.json").read_text(encoding="utf-8"))["sha256"]
    if file_hash(args.oracle) != expected:
        raise ValueError("the oracle SHA-256 differs from spec/inventory/oracle.json")
    data = generate(ROOT, lambda tree: oracle_accepts(str(args.oracle), tree))
    score = (ROOT / "spec/pivot.sha256").read_text(encoding="ascii").strip()
    if hashlib.sha256(data).hexdigest() != score:
        raise ValueError("the pivot SHA-256 differs from spec/pivot.sha256")
    trees = [(entry["id"], json_bytes(entry["tree"]))
             for line in data.splitlines() if (entry := json.loads(line))]
    writers = json.loads((inventory / "output-formats.json").read_text(encoding="utf-8"))
    return writers, trees, cache_version(expected)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--randoc", type=Path, required=True)
    parser.add_argument("--harness", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--filter")
    parser.add_argument("--verify-oracle", action="store_true",
                        help="run the oracle again for each saved result")
    args = parser.parse_args()
    try:
        writers, trees, version = prepare(args)
        gpt_dir = Path(os.environ["RANDOC_GPT"])
        with closing(open_cache(gpt_dir / "cache/fmt/writers.sqlite")) as database:
            rows = measure(args, [fmt for fmt in writers if fmt != "pdf"], trees, database, version, notebook_rule())
        matrix = args.evidence / "capabilities.json"
        if args.filter is None and not matrix.exists():
            subprocess.run([args.harness, "run", "--randoc", args.randoc, "--jobs", "4",
                            "--report", args.evidence / "writers-harness.json", "--matrix", matrix], check=True)
        report = {"units": rows, "summary": {"units": len(rows), "selected": args.filter},
                  "oracle_sha256": file_hash(args.oracle)}
        (args.evidence / "writers-report.json").write_text(json.dumps(report) + "\n", encoding="utf-8")
    except (KeyError, OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        parser.exit(2, f"writers: {error}\n")


if __name__ == "__main__":
    main()
