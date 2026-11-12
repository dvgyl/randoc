#!/usr/bin/env python3
"""Differential run: random documents from block and inline parts, read by
the oracle and by target/debug/randoc with `-t json`.

Each document has 1 to 6 blocks; each block has a container prefix, a block
form and 1 to 4 inline parts. The parts are written for Randoc. The run
prints a table of difference classes: the format and the first top-level
block where the two trees differ (oracle type, Randoc type).

Run: python3 tools/commonmark-cases/differential.py COUNT SEED [FORMAT ...]
"""

import json
import os
import random
import subprocess
import sys
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FORMATS = ["commonmark", "gfm", "commonmark_x"]

INLINES = [
    "word", "two words", "*em*", "**strong**", "_u_", "~~del~~", "~sub~", "^sup^", "`code`",
    "[link](/u \"t\")", "[ref][r]", "[r]", "![img](i.png)", "<http://a.b>", "http://a.b/c.",
    "www.a.b", "a@b.cd", ":smile:", "$x$", "$$y$$", "[span]{.c}", "`r`{=html}", "<b>x</b>",
    "<!-- c -->", "&amp;", "&copy;", "\\*", "'quote'", "\"dq\"", "a--b", "x...", "[^n]",
    "[[wiki]]", "[x](u){#i}", "é", "é", "a\\\nb", "a  \nb", "[]", "[a][]'b'",
]
BLOCKS = [
    "{}", "# {}", "## {} {{#h}}", "{}\n===", "> {}", "- {}", "1. {}", "a. {}", "- [ ] {}",
    "- [x] {}", "```\n{}\n```", "~~~ py\n{}\n~~~", "    {}", "***", "| a | b |\n|---|---|\n| {} | x |",
    "term\n: {}", "::: d\n{}\n:::", "> [!NOTE]\n> {}", "[^n]: {}", "[r]: /r", "<div>\n{}\n</div>",
    "{{.c}}\n{}", "$$\n{}\n$$", "{}\n\n    {}", "- {}\n\n  {}", "1) {}\n2) {}", "- a\n  - {}",
]
PREFIXES = ["", "", "", "> ", "- ", "  ", "> - "]
SEPARATORS = ["\n\n", "\n", "\n\n\n"]


def document(rng):
    blocks = []
    for _ in range(rng.randint(1, 6)):
        text = " ".join(rng.choice(INLINES) for _ in range(rng.randint(1, 4)))
        form = rng.choice(BLOCKS)
        block = form.replace("{}", text, form.count("{}")).replace("{{", "{").replace("}}", "}")
        prefix = rng.choice(PREFIXES)
        more = prefix.replace("- ", "  ")
        lines = block.split("\n")
        blocks.append("\n".join([prefix + lines[0]] + [more + line for line in lines[1:]]))
    return "".join(b + rng.choice(SEPARATORS) for b in blocks)


def run(program, fmt, text):
    try:
        done = subprocess.run([program, "-f", fmt, "-t", "json"], input=text.encode(),
                              capture_output=True, timeout=60)
    except subprocess.TimeoutExpired:
        return None
    return done.returncode, done.stdout.decode(), done.stderr.decode()


def first_difference(a, b):
    try:
        ta, tb = json.loads(a[1]), json.loads(b[1])
    except (ValueError, TypeError):
        return f"exit {a[0]} / {b[0]}"
    if ta["meta"] != tb["meta"]:
        return "meta"
    for x, y in zip(ta["blocks"], tb["blocks"]):
        if x != y:
            return f"{x['t']} / {y['t']}"
    return f"block count {len(ta['blocks'])} / {len(tb['blocks'])}"


def main():
    count, seed = int(sys.argv[1]), int(sys.argv[2])
    formats = sys.argv[3:] or FORMATS
    oracle = os.environ["RANDOC_ORACLE_PANDOC"]
    randoc = str(ROOT / "target/debug/randoc")
    rng = random.Random(seed)
    docs = [document(rng) for _ in range(count)]
    classes = Counter()
    examples = {}
    for fmt in formats:
        with ThreadPoolExecutor(2) as pool:
            expected = list(pool.map(lambda d: run(oracle, fmt, d), docs))
        same = 0
        for doc, want in zip(docs, expected):
            got = run(randoc, fmt, doc)
            if want is None:
                key = (fmt, "oracle did not end")
            elif got == want:
                same += 1
                continue
            elif got is None:
                key = (fmt, "randoc did not end")
            else:
                key = (fmt, first_difference(want, got))
            classes[key] += 1
            if key not in examples or len(doc) < len(examples[key]):
                examples[key] = doc
        print(f"{fmt}: {count} documents, {same} same")
    print("| Format | Class (oracle / Randoc) | Documents | Shortest example |")
    print("| --- | --- | ---: | --- |")
    for key, n in sorted(classes.items()):
        print(f"| {key[0]} | {key[1]} | {n} | {examples[key][:120]!r} |")


if __name__ == "__main__":
    main()
