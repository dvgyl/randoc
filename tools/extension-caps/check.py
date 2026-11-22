#!/usr/bin/env python3
"""Measure the supported reader and writer extension pairs."""

import argparse
import hashlib
import json
import os
import sqlite3
import sys
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from itertools import groupby, islice
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / "tools/readers"), str(ROOT / "tools/measure/suites")]

from extension_caps import inventory
from inputs import Input, corpuslib, document_inputs, harness_inputs, pivot_inputs
from normalize import notebook_rule
from reference import open_cache
from writers import prepare

from ext_capture import Captures, Conversion, compare, open_captures
from probes import reader_probe, writer_probe


SETTINGS = ("on", "off", "default")
BINARY_READERS = {"docx", "epub", "odt", "pptx"}


def commands(pair, sample):
    target = "json" if pair.direction == "reader" else pair.format
    return tuple(Conversion(args, sample.data, target) for args in pair.commands())


def read_json(path):
    return json.loads(path.read_bytes())


def nonstable(oracle, conversions, sample, settings=SETTINGS):
    results = {}
    for setting, conversion in zip(settings, conversions):
        status, detail = oracle[conversion]
        if status != "stable":
            results[setting] = {"status": status, "detail": detail}
    return {"input": sample.id, "oracle": results} if results else None


def result(pair, status, method, **details):
    return {"id": pair.id, "status": status, "method": method, **details}


def matches(oracle, randoc, conversions, sample, settings=SETTINGS):
    differences = {}
    for setting, conversion in zip(settings, conversions):
        reference = oracle[conversion][1]
        actual = randoc[conversion]
        parts = compare(reference, actual)
        if parts:
            differences[setting] = {"parts": parts}
            if isinstance(actual, str):
                differences[setting]["reason"] = actual
    return {"input": sample.id, "differences": differences} if differences else None


def failed(pair, method, issue):
    no_result = any("no_result" in entry["parts"] for entry in issue.get("differences", {}).values())
    return result(pair, "no_result" if no_result else "fail", method, **issue)


def authored(pairs, probes, captures, *, retry=False):
    units = [(pair, probes[pair], commands(pair, probes[pair])) for pair in pairs if pair in probes]
    oracle = captures.oracle(request for _, _, conversions in units for request in conversions)
    rows = {}
    sensitive = []
    for pair, sample, conversions in units:
        issue = nonstable(oracle, conversions, sample)
        if issue:
            rows[pair] = failed(pair, "unconfirmed", issue)
            continue
        observations = [oracle[request][1] for request in conversions]
        if observations[0] == observations[1] == observations[2]:
            continue
        exits = [observation[0] for observation in observations]
        if 0 not in exits:
            rows[pair] = result(pair, "fail", "error_only", input=sample.id, oracle_exits=exits)
            continue
        sensitive.append((pair, sample, conversions, exits))
    randoc = captures.randoc((request for _, _, conversions, _ in sensitive for request in conversions),
                             retry=retry)
    for pair, sample, conversions, exits in sensitive:
        issue = matches(oracle, randoc, conversions, sample)
        rows[pair] = (failed(pair, "sensitive", issue) if issue else
                      result(pair, "pass", "sensitive", input=sample.id))
        rows[pair]["oracle_exits"] = exits
    return rows


def inert(pairs, samples, captures, *, retry=False):
    pending = list(pairs)
    rows = {}
    checked = 0
    while pending and (group := list(islice(samples, 1 if checked == 0 else 12))):
        # A default result that differs already prevents an inert pass.
        todo = {pair: [(sample, commands(pair, sample)) for sample in group] for pair in pending}
        defaults = [requests[2] for _, requests in todo[pending[0]]]
        oracle = captures.oracle(defaults)
        randoc = captures.randoc((request for request in defaults if oracle[request][0] == "stable"),
                                 retry=retry)
        # Check earlier inputs first so that a later failure does not hide a witness.
        for index, (sample, request) in enumerate(zip(group, defaults)):
            issue = (nonstable(oracle, (request,), sample, ("default",)) if oracle[request][0] != "stable" else
                     matches(oracle, randoc, (request,), sample, ("default",)))
            if issue:
                for pair in pending:
                    rows[pair] = failed(pair, "inert_check_failed", issue)
                    todo[pair] = todo[pair][:index]
                break
        oracle.update(captures.oracle(request for units in todo.values() for _, requests in units
                                      for request in requests[:2]))
        ready = {}
        for pair, units in todo.items():
            ready[pair] = []
            for sample, requests in units:
                issue = nonstable(oracle, requests, sample)
                if issue:
                    rows[pair] = failed(pair, "unconfirmed", issue)
                    break
                on, off, default = (oracle[request][1] for request in requests)
                if on != default or off != default:
                    rows[pair] = result(pair, "fail", "uncovered", input=sample.id,
                                        oracle_exits=[on[0], off[0], default[0]])
                    break
                ready[pair].append((sample, requests[:2]))
        randoc = captures.randoc((request for units in ready.values() for _, requests in units
                                  for request in requests), retry=retry)
        for pair, units in ready.items():
            for sample, requests in units:
                issue = matches(oracle, randoc, requests, sample, SETTINGS[:2])
                if issue:
                    rows[pair] = failed(pair, "inert_check_failed", issue)
                    break
        pending = [pair for pair in pending if pair not in rows]
        checked += len(group)
    for pair in pending:
        if checked:
            rows[pair] = result(pair, "pass", "inert", inputs_checked=checked)
        else:
            rows[pair] = result(pair, "fail", "unconfirmed", reason="no inputs for the inert check")
    return rows


class Inputs:
    def __init__(self, args, writers, trees, cache, writer_version, writer_cache, pool):
        self.args = args
        self.writers = set(writers)
        self.trees = trees
        self.cache = cache
        self.writer_version = writer_version
        self.writer_cache = writer_cache
        self.pool = pool
        self.rule = notebook_rule()
        self.harness = harness_inputs(ROOT, read_json(ROOT / "spec/inventory/input-formats.json"))
        self.corpus = corpuslib.load_manifest()
        self.workbooks = corpuslib.load_manifest(ROOT / "tools/readers/workbooks.toml")

    def probes(self, pairs):
        probes = {}
        binary = {}
        for pair in pairs:
            if pair.direction == "writer":
                probes[pair] = writer_probe(pair.extension)
            elif pair.format in BINARY_READERS:
                binary.setdefault(pair.format, []).append(pair)
            else:
                sample = reader_probe(pair.format, pair.extension)
                if sample is None:
                    cases = self.harness[pair.format]
                    if not cases:
                        raise ValueError(f"{pair.id}: no targeted reader input")
                    sample = cases[0]
                probes[pair] = sample
        for fmt, group in binary.items():
            trees = [(pair.extension, writer_probe(pair.extension).data) for pair in group]
            files = {sample.id.rsplit("/", 1)[1]: sample.data
                     for sample in pivot_inputs(fmt, trees, self.writer_cache, self.writer_version,
                                                self.args.oracle, self.pool, self.rule, None)}
            for pair in group:
                if pair.extension not in files:
                    raise ValueError(f"{pair.id}: the oracle could not write the targeted reader input")
                probes[pair] = Input("authored", files[pair.extension])
        return probes

    def fallback(self, direction, fmt):
        if direction == "writer":
            yield from (Input(f"writer/{fmt}/{name}", data) for name, data in self.trees)
            return
        # Use the same three input sources as fmt.read.F.
        yield from self.harness[fmt]
        yield from document_inputs((doc for doc in self.corpus if doc.format == fmt), self.args.corpus)
        yield from document_inputs((doc for doc in self.workbooks if doc.format == fmt),
                                   self.cache / "fmt/workbooks", fetch=True)
        if fmt in self.writers:
            yield from pivot_inputs(fmt, self.trees, self.writer_cache, self.writer_version,
                                    self.args.oracle, self.pool, self.rule, None)


def measure(args, pairs, captures, inputs):
    probes = inputs.probes(pairs)
    rows = authored(pairs, probes, captures)
    for retry in (False, True):
        remaining = [pair for pair in pairs if pair not in rows
                     or (retry and rows[pair]["status"] == "no_result")]
        if retry:
            rows.update(authored(remaining, probes, captures, retry=True))
            remaining = [pair for pair in remaining if pair not in rows
                         or (rows[pair]["status"] == "no_result" and rows[pair]["method"] != "sensitive")]
        for (direction, fmt), group in groupby(remaining, lambda pair: pair[:2]):
            measured = inert(list(group), inputs.fallback(direction, fmt), captures, retry=retry)
            rows.update(measured)
            print(f"{direction}/{fmt}: checked {len(measured)} pairs for inert behaviour", flush=True)
    return [rows[pair] for pair in pairs]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle", required=True, type=Path)
    parser.add_argument("--randoc", required=True, type=Path)
    parser.add_argument("--evidence", required=True, type=Path)
    parser.add_argument("--corpus", required=True, type=Path)
    parser.add_argument("--filter")
    parser.add_argument("--verify-oracle", action="store_true")
    args = parser.parse_args()
    try:
        pairs = [pair for pair in inventory(ROOT, read_json)
                 if args.filter is None or args.filter in pair.id]
        writers, trees, writer_version = prepare(args)
        version = hashlib.sha256(writer_version + Path(__file__).with_name("ext_capture.py").read_bytes()).digest()
        cache = (Path(os.environ["RANDOC_CACHE"]) if os.environ.get("RANDOC_CACHE") else
                 Path(os.environ.get("RANDOC_ENV", ROOT.parent / "env")) / "cache")
        with closing(open_cache(cache / "fmt/writers.sqlite")) as writer_cache, \
                closing(open_captures(cache / "extension_caps/oracle.sqlite")) as database, \
                ThreadPoolExecutor(max_workers=2) as pool:
            captures = Captures(args, pool, database, version)
            inputs = Inputs(args, writers, trees, cache, writer_version, writer_cache, pool)
            rows = measure(args, pairs, captures, inputs)
        report = {"units": rows, "selected": args.filter,
                  "inert": sorted(row["id"] for row in rows if row["method"] == "inert")}
        (args.evidence / "extension-caps-report.json").write_text(json.dumps(report) + "\n", encoding="utf-8")
    except (KeyError, OSError, ValueError, RuntimeError, sqlite3.Error, corpuslib.ManifestError) as error:
        parser.exit(2, f"extension capabilities: {error}\n")


if __name__ == "__main__":
    main()
