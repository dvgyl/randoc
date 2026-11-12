#!/usr/bin/env python3
"""Compares the verdict of each unit in two harness report files.

Usage: compare.py OLD_REPORT NEW_REPORT

A report file is the file that `randoc-harness run --report FILE` writes.
The verdict of a unit is its status and its failure kind (the flags of the
comparison). The program prints a table of difference classes and exits
with 1 when one unit or more has a different verdict or is in one file only.
"""

import collections
import json
import sys


def verdicts(path):
    with open(path, encoding="utf-8") as handle:
        units = json.load(handle)["units"]
    result = {}
    for unit in units:
        verdict = unit.get("verdict") or {}
        kind = ",".join(
            name for name in ("exit_ok", "stderr_ok", "bytes_ok", "content_ok")
            if verdict and not verdict[name]
        )
        result[unit["id"]] = (unit["status"], kind)
    return result


def main(argv):
    if len(argv) != 3:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    old, new = verdicts(argv[1]), verdicts(argv[2])
    classes = collections.Counter()
    examples = {}
    for unit_id in sorted(old.keys() | new.keys()):
        before = old.get(unit_id, ("missing", ""))
        after = new.get(unit_id, ("missing", ""))
        if before != after:
            key = f"{before[0]}[{before[1]}] -> {after[0]}[{after[1]}]"
            classes[key] += 1
            examples.setdefault(key, unit_id)
    print(f"verdicts: {len(old)} old units, {len(new)} new units, "
          f"{sum(classes.values())} differences")
    for key, count in classes.most_common():
        print(f"  {count:6}  {key}  (for example {examples[key]})")
    return 1 if classes else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
