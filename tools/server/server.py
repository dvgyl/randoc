#!/usr/bin/env python3
"""Compare Pandoc and Randoc server responses."""

import argparse
import json
import time
from collections import Counter
from pathlib import Path

from cases import conformance_requests, fixed_requests
from transport import cgi_run, http_run, option_run

ROOT = Path(__file__).resolve().parents[2]
BATCH_SIZE = 256
OPTIONS = ("--help", "--version")


def groups(requests):
    ordinary = [request for request in requests if request.timeout is None]
    for start in range(0, len(ordinary), BATCH_SIZE):
        yield ordinary[start:start + BATCH_SIZE]
    for request in requests:
        if request.timeout is not None:
            yield [request]


def verdict(unit, first, second, actual):
    row = {"id": unit, "oracle": first, "randoc": actual}
    if "no_result" in first or "no_result" in second:
        row.update(status="oracle_limit", repeat=second)
    elif first != second:
        parts = [part for part in first if first[part] != second[part]]
        stable_parts = set(first) - set(parts)
        row.update(status="unstable", repeat=second, different=parts,
                   unstable_parts_ok=bool(actual.keys() == first.keys()
                                          and all(first[part] == actual[part] for part in stable_parts)))
    elif "no_result" in actual:
        row.update(status="no_result")
    elif first == actual:
        row.update(status="pass")
    else:
        parts = [part for part in first if first[part] != actual[part]]
        row.update(status="fail", different=parts)
    return row


def observations(run, program, requests):
    results = {}
    for batch in groups(requests):
        results.update(run(program, batch))
    return results


def request_results(mode, run, args, requests):
    if not requests:
        return
    first = observations(run, args.oracle, requests)
    print(f"{mode}: first oracle pass finished", flush=True)
    time.sleep(1)
    second = observations(run, args.oracle, requests)
    print(f"{mode}: second oracle pass finished", flush=True)
    actual = observations(run, args.randoc, requests)
    retry = [request for request in requests if "no_result" in actual[request.id]]
    if retry:
        print(f"{mode}: retrying {len(retry)} Randoc requests", flush=True)
        for request in retry:
            actual.update(run(args.randoc, [request]))
    for request in requests:
        key = request.id
        if request.timeout is not None:
            for result in (first[key], second[key]):
                if "no_result" not in result and result["status"] != 503:
                    raise ValueError(f"{mode}/{key}: the oracle did not return a timeout response")
        yield verdict(f"{mode}/{key}", first[key], second[key], actual[key])


def option_result(args, option):
    first = option_run(args.oracle, option)
    time.sleep(1)
    second = option_run(args.oracle, option)
    actual = option_run(args.randoc, option)
    if "no_result" in actual:
        actual = option_run(args.randoc, option)
    return verdict(f"cli/{option}", first, second, actual)


def measure(args):
    requests, skipped = conformance_requests(ROOT)
    requests.extend(fixed_requests())
    planned = set()
    rows = []
    for mode, run in (("http", http_run), ("cgi", cgi_run)):
        selected = [request for request in requests
                    if args.filter is None or args.filter in f"{mode}/{request.id}"]
        planned.update(f"{mode}/{request.id}" for request in selected)
        rows.extend(request_results(mode, run, args, selected))
        print(f"{mode}: measured {len(selected)} requests", flush=True)
    for option in OPTIONS:
        key = f"cli/{option}"
        if args.filter is None or args.filter in key:
            planned.add(key)
            rows.append(option_result(args, option))
    counts = dict(Counter(row["status"] for row in rows))
    report = {"selected": args.filter, "planned": sorted(planned), "counts": counts, "units": rows,
              "e2e_not_expressible": skipped,
              "unstable_parts_ok": [row["id"] for row in rows if row.get("unstable_parts_ok")]}
    args.evidence.mkdir(parents=True, exist_ok=True)
    with (args.evidence / "server-report.json").open("w", encoding="utf-8") as stream:
        json.dump(report, stream, ensure_ascii=False, separators=(",", ":"))
    print(f"{len(rows)} units: {counts}. {len(skipped)} conversion units need other command options.")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle", required=True, type=Path)
    parser.add_argument("--randoc", required=True, type=Path)
    parser.add_argument("--evidence", required=True, type=Path)
    parser.add_argument("--filter")
    args = parser.parse_args()
    args.oracle = args.oracle.resolve(strict=True)
    args.randoc = args.randoc.resolve(strict=True)
    return measure(args)


if __name__ == "__main__":
    raise SystemExit(main())
