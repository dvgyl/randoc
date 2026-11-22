"""Compare ten Lua writers on the pinned set of 400 document trees."""

import argparse
import hashlib
import json
import os
import sqlite3
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from functools import partial
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/errors"))

from errors import Command, Unit, attempt, classify, differences
from generate import json_bytes
from writers import file_hash, prepare

REPORT = "custom-writers-report.json"
PROBES = {
    "binary": ("", ()),
    "blocks": ("", ()),
    "classic": ("", ()),
    "extensions": ("-smart+hard_line_breaks", ()),
    "layout": ("", ()),
    "options": ("", ("--columns=44", "--wrap=none")),
    "reject": ("", ()),
    "scaffold": ("", ()),
    "template": ("", ("-s",)),
    "walk": ("", ()),
}


def writer_units(name, suffix, options, script, trees, selected):
    filename = f"{name}.lua"
    args = ("-f", "json", "-t", filename + suffix, *options)
    for tree_id, tree in trees:
        identifier = f"writer/{name}/{tree_id}"
        if selected is None or selected in identifier:
            yield Unit(identifier, Command(args, tree, ((filename, script),)))


def reference_key(version, unit):
    command = unit.command
    digest = hashlib.sha256(version)
    digest.update(json_bytes(command.args))
    for name, data in command.files:
        digest.update(json_bytes(name))
        digest.update(hashlib.sha256(data).digest())
    digest.update(command.stdin)
    return digest.digest()


def changed_parts(first, observations):
    return {part for observed in observations if isinstance(observed, dict)
            for part in differences(first, observed)}


def merge_pairs(database, captures, pairs):
    with database:
        # Another connection can save observations while the oracle runs.
        database.execute("BEGIN IMMEDIATE")
        for key, results in captures:
            record = database.execute("SELECT pair FROM refs WHERE key = ?", (key,)).fetchone()
            if record is None:
                if len(results) != 2:
                    raise ValueError("The saved oracle pair disappeared during the check.")
                if all(isinstance(result, dict) for result in results):
                    database.execute("INSERT INTO refs VALUES (?, ?)", (key, json.dumps(results)))
                pairs[key] = results
                continue

            observations = json.loads(record[0])
            first, *earlier = observations
            changed = changed_parts(first, earlier)
            saved_count = len(observations)
            limits = []
            for result in results:
                if isinstance(result, str):
                    limits.append(result)
                    continue
                parts = set(differences(first, result))
                if parts - changed:
                    observations.append(result)
                    changed.update(parts)
            if len(observations) > saved_count:
                database.execute("UPDATE refs SET pair = ? WHERE key = ?", (json.dumps(observations), key))
            pairs[key] = observations + limits


def oracle_pairs(database, units, version, capture, pool, verify):
    keys = [reference_key(version, unit) for unit in units]
    pairs = {}
    missing = []
    for key, unit in zip(keys, units, strict=True):
        saved = database.execute("SELECT pair FROM refs WHERE key = ?", (key,)).fetchone()
        if saved is None:
            missing.append((key, unit))
        else:
            pairs[key] = json.loads(saved[0])
    captures = []
    if missing:
        inputs = [unit for _, unit in missing]
        first = list(pool.map(capture, inputs))
        time.sleep(1)
        for (key, _), left, right in zip(missing, first, pool.map(capture, inputs), strict=True):
            captures.append((key, [left, right]))
    if verify:
        saved = [(key, unit) for key, unit in zip(keys, units, strict=True) if key in pairs]
        checked = pool.map(capture, (unit for _, unit in saved))
        for (key, _), result in zip(saved, checked, strict=True):
            captures.append((key, [result]))
    if captures:
        merge_pairs(database, captures, pairs)
    return [pairs[key] for key in keys]


def verdict(unit, observations, actual):
    first, second, *rechecks = observations
    if not rechecks:
        return classify(unit, first, second, actual)
    if isinstance(rechecks[-1], str):
        row = classify(unit, first, rechecks[-1], actual)
        row["oracle_second"] = second
    else:
        changed = changed_parts(first, [second, *rechecks])
        row = {"id": unit.id, "oracle": first, "oracle_second": second, "randoc": actual,
               "status": "unstable", "unstable_parts": sorted(changed),
               "reason": "The saved and checked oracle results differ.",
               "unstable_parts_ok": isinstance(actual, dict)
               and set(differences(first, actual)) <= changed}
    row["oracle_rechecks"] = rechecks
    return row


def measure(args, trees, database, version):
    rows, retries = [], []
    oracle = partial(attempt, args.oracle, oracle=True)
    randoc = partial(attempt, args.randoc)
    with ThreadPoolExecutor(max_workers=4) as pool:
        for name, (suffix, options) in PROBES.items():
            script = (Path(__file__).with_name("probes") / f"{name}.lua").read_bytes()
            units = list(writer_units(name, suffix, options, script, trees, args.filter))
            if not units:
                continue
            pairs = oracle_pairs(database, units, version, oracle, pool, args.verify_oracle)
            for unit, pair, actual in zip(units, pairs, pool.map(randoc, units), strict=True):
                if isinstance(actual, str):
                    retries.append((len(rows), unit, pair))
                rows.append(verdict(unit, pair, actual))
            print(f"{name}: checked {len(units)} trees", flush=True)
    for index, unit, pair in retries:
        rows[index] = verdict(unit, pair, randoc(unit))
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--randoc", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--filter")
    parser.add_argument("--verify-oracle", action="store_true", help="check each saved oracle result")
    args = parser.parse_args()
    try:
        _, trees, pivot_version = prepare(args)
        env_dir = Path(os.environ.get("RANDOC_ENV", ROOT.parent / "env"))
        datapack = os.environ.get("RANDOC_DATA_DIR") or str(env_dir / "datapack")
        version = json_bytes([pivot_version.hex(), datapack, *(
            file_hash(ROOT / path) for path in ("tools/errors/errors.py", "tools/custom-writers/run.py")
        )])
        cache_root = Path(os.environ["RANDOC_CACHE"]) if os.environ.get("RANDOC_CACHE") else env_dir / "cache"
        cache = cache_root / "custom/writers.sqlite"
        cache.parent.mkdir(parents=True, exist_ok=True)
        with closing(sqlite3.connect(cache, timeout=30)) as database:
            database.execute("CREATE TABLE IF NOT EXISTS refs (key BLOB PRIMARY KEY, pair TEXT NOT NULL)")
            rows = measure(args, trees, database, version)
        report = {
            "units": rows,
            "summary": {"units": len(rows), "selected": args.filter},
            "oracle_sha256": file_hash(args.oracle),
        }
        (args.evidence / REPORT).write_text(json.dumps(report) + "\n", encoding="utf-8")
    except (KeyError, OSError, ValueError, RuntimeError, sqlite3.Error) as error:
        parser.exit(2, f"custom writers: {error}\n")


if __name__ == "__main__":
    main()
