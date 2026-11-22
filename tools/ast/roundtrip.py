#!/usr/bin/env python3
"""Measure JSON and native conversion on each distinct visible oracle tree."""

import argparse
import hashlib
import json
import os
import sqlite3
import time
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from dataclasses import dataclass, field
from pathlib import Path

from sources import (ROOT, commonmark_trees, corpus_trees, generated_trees, harness_trees,
                     reader_trees, suite_trees, version_cases)
from capture import Capture
from normalize import notebook_rule
from reference import (cache_key, classify, open_cache, read_reference, read_status, recover,
                       verify, write_references)
from writers import Unit, batches, file_hash, prepare, verdict

COMMANDS = (("json", "json"), ("json", "native"), ("native", "json"))


@dataclass
class Tree:
    data: bytes
    sources: list[str] = field(default_factory=list)


def collect(groups):
    trees, problems = {}, {}
    for group in groups:
        counts = Counter()
        for source, output in group:
            if isinstance(output, str):
                problems[source] = output
                continue
            digest = hashlib.sha256(output).hexdigest()
            trees.setdefault(digest, Tree(output)).sources.append(source)
            counts[source.partition("/")[0]] += 1
        print(f"Collected sources: {dict(counts)}", flush=True)
    return trees, problems


def inputs(args, writers, pivot, version, oracle_sha, pool, capture):
    return collect((
        ((f"pivot/{identifier}", data) for identifier, data in pivot),
        version_cases(),
        harness_trees(),
        corpus_trees(args, oracle_sha, capture),
        reader_trees(args, set(writers), pivot, version, pool),
        suite_trees(args, pool, capture),
        commonmark_trees(args, pool, capture),
        generated_trees(args, pool, capture),
    ))


def record_references(database, units, capture, target, rule, pool, verify_saved):
    statuses = {unit.key: read_status(database, unit.key) for unit in units}
    pending = [unit for unit in units if statuses[unit.key] in (None, "oracle_limit")]
    if pending:
        first = list(pool.map(capture, pending))
        time.sleep(1)
        second = list(pool.map(capture, pending))
        with database:
            database.execute("BEGIN IMMEDIATE")
            recorded = []
            for unit, one, two in zip(pending, first, second):
                saved = read_reference(database, unit.key)
                result = (recover(target, saved, one, two, rule) if saved and saved.first
                          else classify(target, one, two, rule))
                recorded.append((unit.key, result))
            write_references(database, recorded, refresh=True)
    if verify_saved:
        existing = [unit for unit in units if statuses[unit.key] in ("stable", "unstable")]
        observations = list(pool.map(capture, existing))
        with database:
            database.execute("BEGIN IMMEDIATE")
            write_references(database, [
                (unit.key, verify(target, read_reference(database, unit.key), observation, rule))
                for unit, observation in zip(existing, observations)
            ], refresh=True)


def command_results(args, database, version, rule, pools, attempt, input_format, target, data):
    oracle_pool, randoc_pool = pools
    command = ["-f", input_format, "-t", target]
    units = [Unit(identifier, content, cache_key(version, command, content))
             for identifier, content in data.items()]
    distinct = list({unit.key: unit for unit in units}.values())
    record_references(database, distinct, lambda unit: attempt(args.oracle, command, unit, oracle=True),
                      target, rule, oracle_pool, args.verify_oracle)
    expected = {unit.key: read_reference(database, unit.key) for unit in distinct}

    def check(unit):
        ref = expected[unit.key]
        actual = attempt(args.randoc, command, unit) if ref.status != "oracle_limit" else None
        return verdict(unit, target, ref, actual, rule)

    checked = dict(zip((unit.key for unit in distinct), randoc_pool.map(check, distinct)))
    rows = {unit.name: dict(checked[unit.key], id=unit.name) for unit in units}
    retries = [(unit, command, target, expected[unit.key]) for unit in distinct
               if "reason_randoc" in checked[unit.key]]
    rows_by_key = {}
    for unit in units:
        if "reason_randoc" in checked[unit.key]:
            rows_by_key.setdefault(unit.key, []).append(rows[unit.name])
    outputs = {unit.name: expected[unit.key].first.stdout for unit in units
               if expected[unit.key].status != "oracle_limit"}
    return rows, outputs, [(*retry, rows_by_key[retry[0].key]) for retry in retries]


def summarize(identifier, tree, commands):
    status = {row["status"] for row in commands.values()}
    row = {"id": identifier, "sources": tree.sources, "commands": commands}
    for label in ("oracle_limit", "unstable", "no_result", "fail", "pass"):
        if label in status:
            row["status"] = label
            break
    if row["status"] == "unstable":
        row["unstable_parts_ok"] = all(
            command.get("unstable_parts_ok", command["status"] == "pass")
            for command in commands.values())
    return row


def compare_trees(args, trees, database, version, pools, attempt, rule):
    commands = {identifier: {} for identifier in trees}
    retries = []
    for group in batches(list(trees), 256):
        raw = {identifier: trees[identifier].data for identifier in group}
        native = {}
        for input_format, target in COMMANDS:
            data = native if input_format == "native" else raw
            results, outputs, pending = command_results(
                args, database, version, rule, pools, attempt, input_format, target, data)
            if target == "native":
                # The oracle supplies the same native input to both programs.
                # An ordinary error still has standard output, even when it is empty.
                native = outputs
            for identifier, row in results.items():
                commands[identifier][f"{input_format}-to-{target}"] = row
            retries.extend(pending)
        if group:
            print(f"Checked {group[-1]} and the preceding group ({len(group)} trees)", flush=True)

    for unit, command, target, expected, rows in retries:
        result = verdict(unit, target, expected, attempt(args.randoc, command, unit), rule)
        for row in rows:
            identifier = row["id"]
            row.clear()
            row.update(result, id=identifier)
    return [summarize(identifier, tree, commands[identifier]) for identifier, tree in trees.items()]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle", required=True, type=Path)
    parser.add_argument("--randoc", required=True, type=Path)
    parser.add_argument("--corpus", required=True, type=Path)
    parser.add_argument("--pandoc-tests", required=True, type=Path)
    parser.add_argument("--evidence", required=True, type=Path)
    parser.add_argument("--filter")
    parser.add_argument("--verify-oracle", action="store_true")
    args = parser.parse_args()
    try:
        writers, pivot, version = prepare(args)
        oracle_sha = file_hash(args.oracle)
        cache = (Path(os.environ["RANDOC_CACHE"]) if os.environ.get("RANDOC_CACHE") else
                 Path(os.environ.get("RANDOC_ENV", ROOT.parent / "env")) / "cache") / "ast/references.sqlite"
        capture = Capture()
        with ThreadPoolExecutor(max_workers=2) as oracle_pool, ThreadPoolExecutor(max_workers=4) as randoc_pool, \
                closing(open_cache(cache)) as database:
            trees, problems = inputs(args, writers, pivot, version, oracle_sha, oracle_pool, capture)
            selected = {key: trees[key] for key in sorted(trees)
                        if args.filter is None or args.filter in key
                        or any(args.filter in source for source in trees[key].sources)}
            print(f"Checking {len(selected)} distinct inputs", flush=True)
            ast_version = hashlib.sha256(version + Path(__file__).with_name("capture.py").read_bytes()).digest()
            rows = compare_trees(args, selected, database, ast_version, (oracle_pool, randoc_pool),
                                 capture.attempt, notebook_rule())
        report = {"summary": {"units": len(selected), "filter": args.filter},
                  "units": rows, "source_problems": problems, "oracle_sha256": oracle_sha}
        (args.evidence / "ast-report.json").write_text(json.dumps(report) + "\n", encoding="utf-8")
    except (OSError, ValueError, RuntimeError, sqlite3.Error) as error:
        parser.exit(2, f"ast: {error}\n")


if __name__ == "__main__":
    main()
