#!/usr/bin/env python3
"""Measure each Pandoc reader on pinned inputs."""

import argparse
import hashlib
import inspect
import json
import os
import sqlite3
import subprocess
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from pathlib import Path

from inputs import ROOT, corpuslib, document_inputs, harness_inputs, pivot_inputs, reader_formats
from normalize import notebook_rule
from reference import cache_key, open_cache, read_reference, read_status, write_references
from results import json_equal, oracle_reference, verdict
from writers import attempt, batches, file_hash, first_run, prepare


def recorded_references(database, inputs, capture, pool, verify):
    statuses = {key: read_status(database, key) for key in inputs}
    pairs = [key for key, status in statuses.items() if status in (None, "oracle_limit")]
    if pairs:
        with tempfile.TemporaryDirectory(prefix="randoc-reader-pair-") as folder, \
                closing(open_cache(Path(folder) / "first.sqlite")) as first:
            for group in batches(pairs, 2):
                captures = pool.map(capture, (inputs[key] for key in group))
                write_references(first, [(key, first_run(value)) for key, value in zip(group, captures)])
            time.sleep(1)
            for group in batches(pairs, 2):
                captures = pool.map(capture, (inputs[key] for key in group))
                results = []
                for key, second in zip(group, captures):
                    initial = read_reference(first, key)
                    earlier = read_reference(database, key)
                    reference = oracle_reference(initial.first if initial.first is not None else initial.reason,
                                                 second, earlier)
                    results.append((key, reference))
                write_references(database, results, refresh=True)
    if verify:
        saved = [key for key, status in statuses.items() if status in ("stable", "unstable")]
        for group in batches(saved, 2):
            originals = {key: read_reference(database, key) for key in group}
            captures = pool.map(capture, (inputs[key] for key in group))
            checked = [(key, oracle_reference(originals[key].first, value, originals[key]))
                       for key, value in zip(group, captures)]
            write_references(database, checked, refresh=True)


def compare_format(args, fmt, units, database, version, oracle_pool, randoc_pool, rows, retries):
    command = ["-f", fmt, "-t", "json"]
    keys = [cache_key(version, command, unit.data) for unit in units]

    def oracle(unit):
        return attempt(args.oracle, command, unit, oracle=True)

    def randoc(unit):
        return attempt(args.randoc, command, unit)

    recorded_references(database, dict(zip(keys, units)), oracle, oracle_pool, args.verify_oracle)
    for group in batches(list(zip(keys, units)), 4):
        expected = {key: read_reference(database, key) for key, _ in group}
        ready = [unit for key, unit in group if expected[key].status != "oracle_limit"]
        actual = dict(zip((unit.id for unit in ready), randoc_pool.map(randoc, ready)))
        for key, unit in group:
            value = actual.get(unit.id)
            if isinstance(value, str):
                retries.append((len(rows), key, fmt, unit))
            rows.append(verdict(unit.id, fmt, expected[key], value))


def measure(args, formats, writers, trees, writer_version, cache):
    digest = hashlib.sha256(b"randoc reader results" + writer_version)
    digest.update((ROOT / "tools/readers/results.py").read_bytes())
    digest.update(inspect.getsource(json_equal).encode("utf-8"))
    version = digest.digest()
    own = harness_inputs(ROOT, formats)
    corpus = corpuslib.load_manifest()
    workbooks = corpuslib.load_manifest(ROOT / "tools/readers/workbooks.toml")
    rule = notebook_rule()
    rows, retries = [], []
    counts = {}
    with closing(open_cache(cache / "writers.sqlite")) as writer_cache, \
            closing(open_cache(cache / "readers.sqlite")) as reader_cache, \
            ThreadPoolExecutor(max_workers=2) as oracle_pool, \
            ThreadPoolExecutor(max_workers=4) as randoc_pool:
        for fmt in formats:
            selected = args.filter

            def matches(unit):
                return selected is None or selected in unit.id

            def documents(source):
                return [doc for doc in source if doc.format == fmt
                        and (selected is None or selected in f"reader/{fmt}/corpus/{doc.name}")]

            pivot = list(pivot_inputs(fmt, trees, writer_cache, writer_version, args.oracle,
                                      oracle_pool, rule, selected)) if fmt in writers else []
            cases = list(filter(matches, own[fmt]))
            visible = list(document_inputs(documents(corpus), args.corpus))
            visible.extend(document_inputs(documents(workbooks), cache / "workbooks", fetch=True))
            units = pivot + cases + visible
            counts[fmt] = {"pivot": len(pivot), "harness": len(cases), "corpus": len(visible)}
            if units:
                compare_format(args, fmt, units, reader_cache, version, oracle_pool, randoc_pool, rows, retries)
                print(f"{fmt}: checked {len(units)} inputs", flush=True)
        for index, key, fmt, unit in retries:
            expected = read_reference(reader_cache, key)
            actual = attempt(args.randoc, ["-f", fmt, "-t", "json"], unit)
            rows[index] = verdict(unit.id, fmt, expected, actual)
    return rows, counts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--randoc", type=Path, required=True)
    parser.add_argument("--harness", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--filter")
    parser.add_argument("--verify-oracle", action="store_true")
    args = parser.parse_args()
    try:
        writers, trees, version = prepare(args)
        cache = Path(os.environ["RANDOC_GPT"]) / "cache/fmt"
        rows, counts = measure(args, reader_formats(ROOT), set(writers), trees, version, cache)
        matrix = args.evidence / "capabilities.json"
        if args.filter is None and not matrix.exists():
            subprocess.run([args.harness, "run", "--randoc", args.randoc, "--jobs", "4",
                            "--report", args.evidence / "readers-harness.json", "--matrix", matrix], check=True)
        report = {
            "units": rows,
            "input_counts": counts,
            "summary": {"units": len(rows), "selected": args.filter},
            "oracle_sha256": file_hash(args.oracle),
            "value_only": [row["id"] for row in rows if row.get("value_only")],
            "unstable_parts_ok": [row["id"] for row in rows if row.get("unstable_parts_ok")],
        }
        (args.evidence / "readers-report.json").write_text(json.dumps(report) + "\n", encoding="utf-8")
    except (KeyError, OSError, ValueError, RuntimeError, sqlite3.Error,
            corpuslib.ManifestError, subprocess.CalledProcessError) as error:
        parser.exit(2, f"readers: {error}\n")


if __name__ == "__main__":
    main()
