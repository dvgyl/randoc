#!/usr/bin/env python3
"""Compares section numbers with the oracle: heading levels -1 to 9 in short
sequences, offsets of 0 to 7 items with the values 0, 5 and the 64-bit
limits, with and without section divs, in `html` and `html4`.

Usage: python3 tools/numbers.py [show]
"""
import itertools, os, random, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from differential import compare, doc_of, S

MAX, MIN = 9223372036854775807, -9223372036854775808


def cases():
    rng = random.Random(7)
    sequences = [[l] for l in range(-1, 10)] + [[a, b] for a, b in itertools.product([-1, 0, 1, 2, 3, 7], repeat=2)]
    sequences += [[rng.randint(-1, 9) for _ in range(rng.randint(3, 6))] for _ in range(20)]
    offsets = [[], [0], [5], [0, 5], [5, 0, 5], [MAX], [MAX, MIN], [MIN, MAX, 0, 1], [0] * 7, [5] * 7, [1, 2, 3, 4, 5, 6, 7]]
    out = []
    for seq, off, fmt, divs in itertools.product(sequences, offsets, ["html", "html4"], [False, True]):
        blocks = [{"t": "Header", "c": [level, ["", ["c"] if i == 0 else [], [["k", "v"]] if i == 0 else []], [S(f"h{level}")]]}
                  for i, level in enumerate(seq)]
        opts = {"number_sections": True, "wrap": "none"}
        if off:
            opts["number_offset"] = off
        if divs:
            opts["section_divs"] = True
        out.append((f"{seq} {off} {divs}", doc_of(blocks), fmt, opts))
    return out


def main():
    diffs = compare(cases())
    for (name, doc, fmt, opts), mine, other in diffs[: 10 if len(sys.argv) > 1 else 0]:
        print("===", name, fmt)
        print("R:", repr(mine.get("out", mine.get("error"))))
        print("O:", repr(other["out"]))
    print(f"{len(diffs)} differences in {len(cases())} cases")


if __name__ == "__main__":
    main()
