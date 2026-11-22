"""Run conversions and keep compact records of their results."""

import hashlib
import json
import sqlite3
import time
from typing import NamedTuple

from observe import NoResult, ZIP_FORMATS, run, zip_content
from reference import cache_key
from writers import batches


class Conversion(NamedTuple):
    args: tuple[str, ...]
    data: bytes
    target: str


def signature(result, target):
    stdout = hashlib.sha256(result.stdout).hexdigest()
    stored_mimetype = False
    if target in ZIP_FORMATS and result.exit == 0:
        archive = zip_content(result.stdout)
        if archive is None:
            return "invalid ZIP output"
        stdout = archive["sha256"]
        stored_mimetype = archive["first_stored_mimetype"]
    files = json.dumps(result.files, sort_keys=True).encode("utf-8")
    return [result.exit, stdout, hashlib.sha256(result.stderr).hexdigest(),
            hashlib.sha256(files).hexdigest(), stored_mimetype]


def capture(binary, conversion, *, oracle=False):
    try:
        result = run(binary, conversion.args, conversion.data, oracle=oracle)
    except NoResult as error:
        return str(error)
    return signature(result, conversion.target)


def compare(expected, actual):
    if isinstance(actual, str):
        return ["no_result"]
    parts = [name for index, name in enumerate(("exit", "stdout", "stderr", "files"))
             if expected[index] != actual[index]]
    if expected[4] and not actual[4] and "stdout" not in parts:
        parts.append("stdout")
    return parts


def classify(first, second):
    if isinstance(first, str) or isinstance(second, str):
        reasons = [f"oracle run {index}: {value}" for index, value in enumerate((first, second), 1)
                   if isinstance(value, str)]
        return "oracle_limit", reasons
    if first != second:
        return "unstable", sorted(set(compare(first, second) + compare(second, first)))
    return "stable", first


class Captures:
    def __init__(self, args, pool, database, version):
        self.args = args
        self.pool = pool
        self.database = database
        self.version = version
        self.verified = set()
        self.latest = {}
        self.retried = set()
        database.execute("CREATE TABLE IF NOT EXISTS observations (key BLOB PRIMARY KEY, result TEXT)")

    def oracle(self, requests):
        result = {}
        pending = {}
        for request in dict.fromkeys(requests):
            key = cache_key(self.version, request.args, request.data)
            row = self.database.execute("SELECT result FROM observations WHERE key = ?", (key,)).fetchone()
            saved = tuple(json.loads(row[0])) if row else None
            if saved and (not self.args.verify_oracle or key in self.verified):
                result[request] = saved
            else:
                pending[request] = (key, saved)

        def convert(request):
            return capture(self.args.oracle, request, oracle=True)

        for group in batches(list(pending), 96):
            first = list(self.pool.map(convert, group))
            time.sleep(1)
            second = list(self.pool.map(convert, group))
            for request, a, b in zip(group, first, second):
                key, saved = pending[request]
                observed = classify(a, b)
                if saved and observed != saved and saved[0] == "stable" and observed[0] == "stable":
                    observed = ("unstable", sorted(set(compare(saved[1], a) + compare(a, saved[1]))))
                result[request] = observed
                self.database.execute("INSERT OR IGNORE INTO observations VALUES (?, ?)",
                                      (key, json.dumps(observed)))
                if saved is not None and saved != observed:
                    self.database.execute("UPDATE observations SET result = ? WHERE key = ? AND result = ?",
                                          (json.dumps(observed), key, json.dumps(saved)))
                self.verified.add(key)
            self.database.commit()
        return result

    def randoc(self, requests, *, retry=False):
        requests = list(dict.fromkeys(requests))
        pending = [request for request in requests if request not in self.latest]

        def convert(request):
            return capture(self.args.randoc, request)

        self.latest.update(zip(pending, map(convert, pending) if retry else self.pool.map(convert, pending)))
        if retry:
            for request in requests:
                if isinstance(self.latest[request], str) and request not in self.retried:
                    self.latest[request] = convert(request)
                    self.retried.add(request)
        return {request: self.latest[request] for request in requests}


def open_captures(path):
    path.parent.mkdir(parents=True, exist_ok=True)
    return sqlite3.connect(path, timeout=30)
