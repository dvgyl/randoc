#!/usr/bin/env python3
"""Writes src/json/categories.rs from UnicodeData.txt.

Usage: unicode_tables.py UnicodeData.txt OUTPUT.rs

Pandoc 3.13 uses the Unicode 15.1 tables of GHC. Use
https://www.unicode.org/Public/15.1.0/ucd/UnicodeData.txt.
"""
import sys


def ranges(codes):
    out = []
    for code in sorted(codes):
        if out and out[-1][1] + 1 == code:
            out[-1][1] = code
        else:
            out.append([code, code])
    return out


def main():
    letters, numbers = set(), set()
    first = None
    for line in open(sys.argv[1], encoding="utf-8"):
        fields = line.split(";")
        code, name, category = int(fields[0], 16), fields[1], fields[2]
        if name.endswith(", First>"):
            first = code
            continue
        span = range(first, code + 1) if name.endswith(", Last>") else range(code, code + 1)
        first = None
        if category.startswith("L"):
            letters.update(span)
        elif category.startswith("N"):
            numbers.update(span)
    with open(sys.argv[2], "w", encoding="utf-8") as out:
        out.write("//! Unicode general categories, from `UnicodeData.txt` of Unicode 15.1.0.\n")
        out.write("//! `tools/unicode_tables.py` writes this file. Do not edit it.\n\n")
        for name, codes, doc in [("LETTERS", letters, "letter (L)"), ("NUMBERS", numbers, "number (N)")]:
            table = ranges(codes)
            out.write("/// The ranges of the general category %s.\n" % doc)
            out.write("pub const %s: [(u32, u32); %d] = [\n" % (name, len(table)))
            for low, high in table:
                out.write("    (0x%04X, 0x%04X),\n" % (low, high))
            out.write("];\n\n")


main()
