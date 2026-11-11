#!/usr/bin/env python3
"""Writes the delimiter run matrix of conformance/cases/commonmark/runs.toml.

Each case is one document. Each paragraph of the document holds two or three
runs of one delimiter character. The matrix has each pair of run lengths from
1 to 8 in each flanking position, and each triple of run lengths from 1 to 5.
The inputs are written for Randoc.

Run: python3 tools/commonmark-cases/runs.py
Compare the two programs without the harness (the oracle runs once for each case):
     python3 tools/commonmark-cases/runs.py --compare
"""

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

# R and S are the runs. A paragraph starts with a letter, so that a run never starts a block.
PAIR_FORMS = [
    "a{R}b{S}c",      # both runs are inside a word
    "x {R}b{S} y",    # R can open, S can close
    "a{R} b{S}c",     # R can close only
    "a{R}b {S}c",     # S can open only
    "x {R}b {S}c",    # two openers
    "a{R} b{S} y",    # two closers
    "x .{R}b.{S}. y",  # punctuation before each run
    "x {R}.b.{S} y",  # punctuation inside
]
TRIPLE_FORMS = [
    "a{R}b{S}c{T}d",
    "x {R}b{S}c{T} y",
]

# (case name, delimiter character, format)
SETS = [
    ("star", "*", "commonmark"),
    ("under", "_", "commonmark"),
    ("caret", "^", "commonmark_x"),
    ("tilde", "~", "commonmark_x"),
    ("tilde-gfm", "~", "gfm"),
]


def pairs(ch):
    return [form.format(R=ch * i, S=ch * j)
            for form in PAIR_FORMS for i in range(1, 9) for j in range(1, 9)]


def triples(ch):
    return [form.format(R=ch * i, S=ch * j, T=ch * k)
            for form in TRIPLE_FORMS
            for i in range(1, 6) for j in range(1, 6) for k in range(1, 6)]


def cases():
    for name, ch, fmt in SETS:
        yield f"pairs-{name}", fmt, pairs(ch)
        yield f"triples-{name}", fmt, triples(ch)


def document(paragraphs):
    return "\n\n".join(paragraphs) + "\n"


def write():
    out = ["# Delimiter runs: each pair of lengths 1 to 8 and each triple of lengths 1 to 5.",
           "# The inputs are written for Randoc. tools/commonmark-cases/runs.py writes this file.",
           "to = []", ""]
    for cid, fmt, paragraphs in cases():
        out += ["[[case]]", f"id = \"{cid}\"", f"from = \"{fmt}\"",
                f"input = {json.dumps(document(paragraphs))}", ""]
    path = ROOT / "conformance/cases/commonmark/runs.toml"
    path.write_text("\n".join(out), encoding="utf-8")


def blocks(program, fmt, text):
    done = subprocess.run([program, "-f", fmt, "-t", "json"], input=text.encode(),
                          capture_output=True, timeout=60, check=True)
    return json.loads(done.stdout)["blocks"]


def compare():
    oracle = os.environ["RANDOC_ORACLE_PANDOC"]
    randoc = str(ROOT / "target/debug/randoc")
    total = 0
    different = 0
    for cid, fmt, paragraphs in cases():
        text = document(paragraphs)
        want = blocks(oracle, fmt, text)
        got = blocks(randoc, fmt, text)
        if len(want) != len(paragraphs) or len(got) != len(paragraphs):
            print(f"{cid}: {len(paragraphs)} paragraphs, oracle {len(want)} blocks, Randoc {len(got)}")
            different += 1
            continue
        for source, a, b in zip(paragraphs, want, got):
            total += 1
            if a != b:
                different += 1
                if different <= 20:
                    print(f"{cid} ({fmt}): {source}\n  oracle {json.dumps(a['c'])}\n  randoc {json.dumps(b['c'])}")
    print(f"{total} paragraphs compared, {different} different")
    return 1 if different else 0


if __name__ == "__main__":
    if sys.argv[1:] == ["--compare"]:
        sys.exit(compare())
    write()
