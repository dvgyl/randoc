#!/usr/bin/env python3
"""Writes `crates/randoc-lua/MODULES.md`: each name of the Lua interface of
Pandoc 3.13 and its state in Randoc.

Usage (from the root of the worktree, after `. env.sh` and
`cargo build -p randoc-lua --examples`):

    python3 -I crates/randoc-lua/tools/interface-table.py

The tool runs `tools/interface-probe.lua` as a Lua filter with Pandoc 3.13
(`$RANDOC_ORACLE_PANDOC`) and with `target/debug/examples/randoc-lua
filter`. The probe lists each global, each field of the module `pandoc`,
each field of a table in it, and each field of another global table, with
its kind: the Lua type, and the value of a short scalar. The tool compares
the two lists and writes one row for each name of the reference:

- equal: Randoc has the name with the same kind;
- different: Randoc has the name with another kind;
- absent: Randoc does not have the name, or has a stub that raises
  "is not implemented in Randoc".

The probe compares the interface, not the behaviour; the matrices of
`tests/replay` compare the behaviour. The file holds names and states
only, so it stores no text of Pandoc.
"""

import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
CRATE = os.path.join(HERE, "..")
PROBE = os.path.join(HERE, "interface-probe.lua")
OUTPUT = os.path.join(CRATE, "MODULES.md")
RANDOC = os.path.join(CRATE, "..", "..", "target", "debug", "examples", "randoc-lua")
DOCUMENT = '{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}'
STATES = ["equal", "different", "absent"]


def run(command):
    """The probe lines of one program, by name."""
    with tempfile.TemporaryDirectory(prefix="lua-interface-") as folder:
        probe = os.path.join(folder, "probe.lua")
        with open(PROBE, encoding="utf-8") as source, open(probe, "w", encoding="utf-8") as target:
            target.write(source.read())
        done = subprocess.run(command + ["probe.lua"], cwd=folder, input=DOCUMENT.encode(), capture_output=True, timeout=60)
    if done.returncode != 0:
        sys.exit(f"{command[0]} failed: {done.stderr.decode(errors='replace')}")
    names = {}
    for line in done.stderr.decode().splitlines():
        if "\t" in line:
            path, kind = line.split("\t", 1)
            names[path] = kind
    return names


def state(reference, ours):
    if ours is None or ours == "stub":
        return "absent"
    return "equal" if ours == reference else "different"


def main():
    pandoc = os.environ.get("RANDOC_ORACLE_PANDOC")
    if not pandoc or "GHCRTS" not in os.environ:
        sys.exit("source env.sh first: RANDOC_ORACLE_PANDOC and GHCRTS must be set")
    reference = run([pandoc, "-f", "json", "-t", "json", "--lua-filter"])
    randoc = run([RANDOC, "filter"])
    result = {path: state(kind, randoc.get(path)) for path, kind in reference.items()}
    counts = {name: sum(value == name for value in result.values()) for name in STATES}
    extra = sorted(path for path in randoc if path not in reference)
    summary = ", ".join(f"{name} {counts[name]}" for name in STATES)
    lines = [
        "# The Lua interface of Pandoc in Randoc",
        "",
        "`tools/interface-table.py` writes this file from a run of",
        "`tools/interface-probe.lua` with Pandoc 3.13 and with Randoc. Do not",
        "edit it. A name is equal when Randoc has it with the same kind (the",
        "Lua type, and the value of a short scalar), different when the kind",
        "differs, and absent when Randoc does not have it or raises",
        "\"is not implemented in Randoc\". The matrices of `tests/replay`",
        "compare the behaviour.",
        "",
        f"Names of Pandoc: {len(reference)}. {summary[0].upper()}{summary[1:]}.",
        f"Names only in Randoc: {len(extra)}.",
        "",
        "| name | state |",
        "| --- | --- |",
    ]
    lines += [f"| `{path}` | {result[path]} |" for path in sorted(reference)]
    if extra:
        lines += ["", "Names only in Randoc:", ""]
        lines += [f"- `{path}`" for path in extra]
    with open(OUTPUT, "w", encoding="utf-8") as handle:
        handle.write("\n".join(lines) + "\n")
    print(f"{len(reference)} names: {summary}, only in Randoc {len(extra)}")


if __name__ == "__main__":
    main()
