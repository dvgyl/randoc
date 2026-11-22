#!/usr/bin/env python3
"""Writes src/collate/ducet.bin: the collation table of the Unicode
Collation Algorithm (DUCET, allkeys.txt 13.0.0), the canonical
decompositions and the canonical combining classes (UnicodeData.txt
13.0.0). The citeproc library of the oracle (unicode-collation 0.1.3.7)
uses these two files of Unicode 13.0.0.

Usage (from the crate folder):
    python3 -I tools/collation_tables.py ALLKEYS UNICODEDATA

Get the two files from unicode.org:
    https://www.unicode.org/Public/UCA/13.0.0/allkeys.txt
    https://www.unicode.org/Public/13.0.0/ucd/UnicodeData.txt
The script checks their SHA-256 sums. The files are under the Unicode
License (see THIRD_PARTY_NOTICES.md); the output holds their data only.

Format (little-endian):
    b"RDUCET1\\0"
    u32 N, then N entries: u8 count of code points, the code points
        (3 bytes each), u8 count of elements, the elements (u16 primary,
        u16 secondary, u8 tertiary with 0x80 for a variable element)
    u32 N, then N decompositions: code point (3 bytes), u8 count, the
        code points (3 bytes each)
    u32 N, then N combining classes: code point (3 bytes), u8 class
"""

import hashlib
import struct
import sys

ALLKEYS_SHA256 = "a3255d45b7af97f4dc14fb8364d7573b434425e5c58cacf00d16901ce081c78d"
UNICODEDATA_SHA256 = "bdbffbbfc8ad4d3a6d01b5891510458f3d36f7170422af4ea2bed3211a73e8bb"
OUT = "src/collate/ducet.bin"


def read_checked(path, digest):
    data = open(path, "rb").read()
    found = hashlib.sha256(data).hexdigest()
    if found != digest:
        sys.exit(f"{path}: SHA-256 {found}, expected {digest}")
    return data.decode("utf-8")


def cp3(value):
    return struct.pack("<I", value)[:3]


def read_code_points(text):
    """readCodePoints: hexadecimal numbers separated by spaces."""
    cps = []
    rest = text
    while True:
        rest = rest.lstrip(" ")
        digits = ""
        while rest and rest[0] in "0123456789abcdefABCDEF":
            digits += rest[0]
            rest = rest[1:]
        if not digits:
            return cps, rest
        cps.append(int(digits, 16))


def collation(text):
    entries = []
    for line in text.splitlines():
        cps, rest = read_code_points(line)
        if not cps:
            continue
        elements = []
        while True:
            start = rest.find("[")
            if start < 0:
                break
            end = rest.find("]", start)
            if end < 0:
                break
            contents = rest[start + 1 : end]
            rest = rest[end + 1 :]
            variable = contents.startswith("*")
            weights = [w for w in contents.replace("*", ".").split(".") if w]
            l1, l2, l3 = (int(w, 16) for w in weights)
            assert l1 < 0x10000 and l2 < 0x10000 and l3 < 0x80, line
            elements.append((l1, l2, l3, variable))
        entries.append((cps, elements))
    return entries


def unicode_data(text):
    decompositions = []
    classes = []
    for line in text.splitlines():
        fields = line.split(";")
        assert len(fields) == 15, line
        cp = int(fields[0], 16)
        ccc = int(fields[3]) if fields[3].isdigit() else 0
        if ccc > 0:
            classes.append((cp, ccc))
        if fields[5] and not fields[5].startswith("<"):
            cps, _ = read_code_points(fields[5])
            if cps:
                decompositions.append((cp, cps))
    return decompositions, classes


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    entries = collation(read_checked(sys.argv[1], ALLKEYS_SHA256))
    decompositions, classes = unicode_data(read_checked(sys.argv[2], UNICODEDATA_SHA256))
    out = bytearray(b"RDUCET1\0")
    out += struct.pack("<I", len(entries))
    for cps, elements in entries:
        out += bytes([len(cps)]) + b"".join(cp3(c) for c in cps)
        out += bytes([len(elements)])
        for l1, l2, l3, variable in elements:
            out += struct.pack("<HHB", l1, l2, l3 | (0x80 if variable else 0))
    out += struct.pack("<I", len(decompositions))
    for cp, cps in decompositions:
        out += cp3(cp) + bytes([len(cps)]) + b"".join(cp3(c) for c in cps)
    out += struct.pack("<I", len(classes))
    for cp, ccc in classes:
        out += cp3(cp) + bytes([ccc])
    with open(OUT, "wb") as f:
        f.write(out)
    print(f"{OUT}: {len(entries)} entries, {len(decompositions)} decompositions, "
          f"{len(classes)} combining classes, {len(out)} bytes")


if __name__ == "__main__":
    main()
