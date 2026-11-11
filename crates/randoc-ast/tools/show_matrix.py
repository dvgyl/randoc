#!/usr/bin/env python3
"""Write tests/show_matrix.tsv: strings and their Haskell `show` from the oracle.

Usage: tools/show_matrix.py  (from crates/randoc-ast, after sourcing env.sh)

Each line has the UTF-8 bytes of one string in hexadecimal and the text that
`pandoc -f json -t native` shows for `Str` with that string. One oracle
process reads all strings.
"""

import json
import os
import random
import re
import subprocess
import sys

CLASSES = {
    "control": [chr(c) for c in range(0, 32)],
    "del": ["\x7f"],
    "digit": list("0123456789"),
    "ascii": list("aHhZ\"\\' ~&"),
    "latin1": [chr(c) for c in range(0x80, 0x100)],
    "bmp": ["Ā", " ", "あ", "﻿", "￿", "", "​"],
    "astral": ["\U00010000", "\U0001F600", "\U0010FFFF", "\U00020000"],
}
FOLLOWERS = ["H", "h", "0", "9", "a", "\x0e", "\x7f", '"', "\\", "é", "\U0001F600"]
COUNT = 2600


def strings():
    rng = random.Random(11)
    names = list(CLASSES)
    members = [c for name in names for c in CLASSES[name]]
    found = {""}
    found.update(members)
    found.update(a + b for a in members for b in FOLLOWERS)
    found.update(["\x0eHH", "\x0e\x0eH", "x\x0eH1", "\x0e\x7fH", "\x0eH\x0eH"])
    while len(found) < COUNT:
        size = rng.randint(1, 8)
        found.add("".join(rng.choice(CLASSES[rng.choice(names)]) for _ in range(size)))
    return sorted(found)


def main():
    table = strings()
    doc = {
        "pandoc-api-version": [1, 23, 1],
        "meta": {},
        "blocks": [{"t": "Plain", "c": [{"t": "Str", "c": s}]} for s in table],
    }
    result = subprocess.run(
        [os.environ["RANDOC_ORACLE_PANDOC"], "-f", "json", "-t", "native"],
        input=json.dumps(doc).encode(),
        capture_output=True,
        timeout=60,
        check=True,
    )
    shown = []
    for line in result.stdout.decode().splitlines():
        match = re.fullmatch(r'[\[,] Plain \[ Str (".*") \]', line)
        if match:
            shown.append(match.group(1))
    if len(shown) != len(table):
        sys.exit(f"the oracle gave {len(shown)} strings for {len(table)}")
    here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    with open(os.path.join(here, "tests", "show_matrix.tsv"), "w", encoding="utf-8") as out:
        for text, show in zip(table, shown):
            out.write(f"{text.encode().hex()}\t{show}\n")


if __name__ == "__main__":
    main()
