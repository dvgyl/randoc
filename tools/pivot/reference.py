"""Classify and cache the paired oracle runs of the writer suite."""

import hashlib
import json
import sqlite3
from dataclasses import dataclass

from generate import ROOT, json_bytes
from normalize import notebook_output, notebook_positions
from observe import Observation, differences


@dataclass(frozen=True)
class Reference:
    status: str
    first: Observation | None
    cell_positions: tuple[int, ...] = ()
    unstable_parts: tuple[str, ...] = ()
    reason: str = ""


def compare(fmt, reference, actual, rule):
    parts = differences(fmt, reference.first, actual)
    if reference.cell_positions and "stdout" in parts:
        first = notebook_output(reference.first.stdout, reference.cell_positions, rule)
        other = notebook_output(actual.stdout, reference.cell_positions, rule)
        if first is not None and first == other:
            parts.remove("stdout")
    return parts


def classify(fmt, first, second, rule):
    for number, result in enumerate((first, second), start=1):
        if isinstance(result, str):
            return Reference("oracle_limit", None, reason=f"oracle run {number}: {result}")
    positions = ()
    if fmt == "ipynb" and first.exit == second.exit == 0:
        positions = notebook_positions(first.stdout, second.stdout, rule)
    recorded = Reference("stable", first, positions)
    changed = compare(fmt, recorded, second, rule)
    if changed:
        return Reference("unstable", first, positions, tuple(changed),
                         "the two oracle results differ")
    return recorded


def cache_version(oracle_sha256):
    digest = hashlib.sha256(json_bytes(["writer", oracle_sha256]))
    for path in ("tools/pivot/observe.py", "tools/pivot/normalize.py", "tools/pivot/reference.py",
                 "tools/autobuild/bin/limit-run",
                 "crates/randoc-harness/src/snapshot.py", "spec/normalizations.toml"):
        digest.update(hashlib.sha256((ROOT / path).read_bytes()).digest())
    return digest.digest()


def cache_key(version, args, data):
    key = hashlib.sha256(version)
    key.update(hashlib.sha256(json_bytes(args)).digest())
    key.update(data)
    return key.digest()


def open_cache(path):
    path.parent.mkdir(parents=True, exist_ok=True)
    database = sqlite3.connect(path, timeout=30)
    database.execute("""CREATE TABLE IF NOT EXISTS writer_refs (
        key BLOB PRIMARY KEY, status TEXT NOT NULL, result_exit INTEGER,
        stdout BLOB, stderr BLOB, files TEXT, cells TEXT, parts TEXT, reason TEXT
    )""")
    return database


def read_reference(database, key):
    row = database.execute("""SELECT status, result_exit, stdout, stderr, files, cells, parts, reason
        FROM writer_refs WHERE key = ?""", (key,)).fetchone()
    if row is None:
        return None
    status, code, stdout, stderr, files, cells, parts, reason = row
    first = None if code is None else Observation(code, stdout, stderr, json.loads(files))
    return Reference(status, first, tuple(json.loads(cells)), tuple(json.loads(parts)), reason)


def write_references(database, pairs):
    rows = []
    for key, recorded in pairs:
        first = recorded.first
        result = (None, None, None, None) if first is None else (
            first.exit, first.stdout, first.stderr, json.dumps(first.files, sort_keys=True))
        rows.append((key, recorded.status, *result, json.dumps(recorded.cell_positions),
                     json.dumps(recorded.unstable_parts), recorded.reason))
    with database:
        database.executemany("""INSERT OR IGNORE INTO writer_refs
            (key, status, result_exit, stdout, stderr, files, cells, parts, reason)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)""", rows)
