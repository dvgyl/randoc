#!/usr/bin/env python3
"""Writes the NFC tables and the NFC oracle data of randoc_core::shared.

Usage: shared_nfc.py UNICODE_DATA_TXT COMPOSITION_EXCLUSIONS_TXT
       shared_nfc.py --measure UCD_DIR...

The files come from the Unicode Character Database 18.0.0. The measurement
(`shared_nfc.py --measure DIR...`) chose this version: see I-SHARED-04.
  https://www.unicode.org/Public/18.0.0/ucd/UnicodeData.txt
  (SHA-256 0736451de439ae7baf1425136617da495e09ee5afbe6e394374db7009ea08950)
  https://www.unicode.org/Public/18.0.0/ucd/CompositionExclusions.txt
  (SHA-256 c759b100e9ae8960ae6b50fc5765950a7581fd9f30d4ef95f68f25a6d97e818e)

Outputs:
  - crates/randoc-core/src/shared/nfc_tables.rs;
  - crates/randoc-core/tests/nfc_oracle.json: inputs and the `Str` text that
    `pandoc -f commonmark -t json` gives for them. RANDOC_ORACLE_PANDOC is
    the path of Pandoc 3.13. The inputs come from a random generator with a
    fixed seed.
"""
import json
import os
import random
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TABLES = os.path.join(HERE, "..", "src", "shared", "nfc_tables.rs")
DATA = os.path.join(HERE, "..", "tests", "nfc_oracle.json")
PANDOC = os.environ.get("RANDOC_ORACLE_PANDOC")
HANGUL = range(0xAC00, 0xAC00 + 11172)


def read_unicode_data(path):
    classes, decompositions = {}, {}
    first = None
    for line in open(path, encoding="utf-8"):
        fields = line.split(";")
        code, name = int(fields[0], 16), fields[1]
        if name.endswith(", First>"):
            first = code
            continue
        span = range(first, code + 1) if name.endswith(", Last>") else range(code, code + 1)
        first = None
        for c in span:
            if int(fields[3]):
                classes[c] = int(fields[3])
        if fields[5] and not fields[5].startswith("<"):
            decompositions[code] = [int(x, 16) for x in fields[5].split()]
    return classes, decompositions


def read_exclusions(path):
    out = set()
    for line in open(path, encoding="utf-8"):
        line = line.split("#")[0].strip()
        if line:
            out.add(int(line, 16))
    return out


def full_decomposition(code, decompositions):
    parts = decompositions.get(code)
    if parts is None:
        return [code]
    return [c for part in parts for c in full_decomposition(part, decompositions)]


def compositions(classes, decompositions, exclusions):
    pairs = []
    for code, parts in decompositions.items():
        if len(parts) != 2 or code in exclusions:
            continue
        if classes.get(code, 0) or classes.get(parts[0], 0):
            continue
        pairs.append((parts[0], parts[1], code))
    return sorted(pairs)


def class_ranges(classes):
    rows = []
    for code in sorted(classes):
        if rows and rows[-1][1] + 1 == code and rows[-1][2] == classes[code]:
            rows[-1][1] = code
        else:
            rows.append([code, code, classes[code]])
    return rows


def code_ranges(codes):
    out = []
    for code in sorted(codes):
        if out and out[-1][1] + 1 == code:
            out[-1][1] = code
        else:
            out.append([code, code])
    return out


def rust_char(code):
    return "'\\u{%X}'" % code


def write_tables(classes, decompositions, pairs):
    full = {c: full_decomposition(c, decompositions) for c in decompositions}
    unsafe = set(classes) | set(full) | {second for _, second, _ in pairs}
    unsafe |= set(range(0x1161, 0x1176)) | set(range(0x11A8, 0x11C3))
    with open(TABLES, "w", encoding="utf-8") as out:
        out.write("//! Tables for NFC from the Unicode Character Database 18.0.0.\n")
        out.write("//! `tools/shared_nfc.py` writes this file. Do not edit it.\n\n")
        rows = class_ranges(classes)
        out.write("/// Ranges of code points with a canonical combining class that is not zero.\n")
        out.write("pub(super) static COMBINING_CLASSES: [(u32, u32, u8); %d] = [\n" % len(rows))
        for low, high, value in rows:
            out.write("    (0x%04X, 0x%04X, %d),\n" % (low, high, value))
        out.write("];\n\n")
        out.write("/// The full canonical decomposition of each character that has one, except Hangul syllables.\n")
        out.write("pub(super) static DECOMPOSITIONS: [(char, &str); %d] = [\n" % len(full))
        for code in sorted(full):
            text = "".join("\\u{%X}" % c for c in full[code])
            out.write('    (%s, "%s"),\n' % (rust_char(code), text))
        out.write("];\n\n")
        out.write("/// The canonical compositions of two characters, without the full composition exclusions.\n")
        out.write("pub(super) static COMPOSITIONS: [(char, char, char); %d] = [\n" % len(pairs))
        for first, second, code in pairs:
            out.write("    (%s, %s, %s),\n" % (rust_char(first), rust_char(second), rust_char(code)))
        out.write("];\n\n")
        rows = code_ranges(unsafe)
        out.write("/// Ranges of the characters that can change in NFC: with a decomposition, a\n")
        out.write("/// combining class, or a place as the second character of a composition.\n")
        out.write("pub(super) static UNSAFE: [(u32, u32); %d] = [\n" % len(rows))
        for low, high in rows:
            out.write("    (0x%04X, 0x%04X),\n" % (low, high))
        out.write("];\n")


def test_inputs(classes, decompositions, exclusions):
    rng = random.Random(20261110)
    full = {c: full_decomposition(c, decompositions) for c in decompositions}
    marks = sorted(classes)
    texts = []
    for code in sorted(full):
        texts.append(chr(code))
        texts.append("".join(map(chr, full[code])))
    for code in rng.sample(list(HANGUL), 300):
        index = code - 0xAC00
        lead, vowel, tail = 0x1100 + index // 588, 0x1161 + (index % 588) // 28, 0x11A7 + index % 28
        jamo = [lead, vowel] + ([tail] if tail != 0x11A7 else [])
        texts.append("".join(map(chr, jamo)))
        texts.append(chr(code) + chr(rng.randrange(0x11A8, 0x11C3)))
    for _ in range(300):
        seq = [rng.choice([0x1100 + rng.randrange(19), 0x1161 + rng.randrange(21), 0x11A8 + rng.randrange(27)]) for _ in range(6)]
        texts.append("".join(map(chr, seq)))
    bases = [c for c in range(0x41, 0x7B) if chr(c).isalpha()] + [0x3B1, 0x3C9, 0x430, 0x915, 0x5D0, 0x627, 0xE9, 0x1EA1]
    same = {}
    for mark in marks:
        same.setdefault(classes[mark], []).append(mark)
    for _ in range(600):
        seq = [rng.choice(bases)] + [rng.choice(marks) for _ in range(rng.randint(1, 5))]
        texts.append("".join(map(chr, seq)))
    for _ in range(200):
        group = same[rng.choice([230, 220, 202, 216, 1])]
        texts.append(chr(rng.choice(bases)) + "".join(chr(rng.choice(group)) for _ in range(3)))
    for code in sorted(exclusions):
        texts.append(chr(code) + "x")
        texts.append("".join(map(chr, full.get(code, [code]))))
    singletons = [c for c, parts in decompositions.items() if len(parts) == 1]
    texts += [chr(c) + chr(rng.choice(marks)) for c in singletons]
    texts += ["plain", "été", "日本語", "العربية", "\U0001F604"]
    # Each mark against an old mark on each side: this shows the class of each mark.
    for mark in marks:
        texts.append("a%s\u0323" % chr(mark))
        texts.append("e%s\u0301" % chr(mark))
    texts.append("a" + "".join(chr(rng.choice(marks)) for _ in range(500)))
    texts.append("́" * 300 + "e")
    return texts


def oracle(texts):
    source = "".join("q%s\n\n" % text for text in texts)
    result = subprocess.run([PANDOC, "-f", "commonmark", "-t", "json"], input=source.encode(),
                            capture_output=True, check=True, timeout=600)
    blocks = json.loads(result.stdout)["blocks"]
    if len(blocks) != len(texts):
        raise SystemExit("the oracle gave %d blocks for %d inputs" % (len(blocks), len(texts)))
    out, skipped = [], 0
    for text, block in zip(texts, blocks):
        # A space or a Markdown character in the result makes more than one inline: skip it.
        if block["t"] != "Para" or any(inline["t"] != "Str" for inline in block["c"]):
            skipped += 1
            continue
        out.append(["q" + text, "".join(inline["c"] for inline in block["c"])])
    print("skipped %d inputs that give more than text" % skipped)
    return out


def main():
    classes, decompositions = read_unicode_data(sys.argv[1])
    exclusions = read_exclusions(sys.argv[2])
    pairs = compositions(classes, decompositions, exclusions)
    write_tables(classes, decompositions, pairs)
    cases = oracle(test_inputs(classes, decompositions, exclusions))
    with open(DATA, "w", encoding="utf-8") as out:
        json.dump(cases, out, ensure_ascii=False, separators=(",", ":"))
        out.write("\n")
    print("compositions %d, cases %d" % (len(pairs), len(cases)))




def nfc_model(unicode_data, exclusions_file):
    """Returns an NFC function in Python that uses the tables of one Unicode version."""
    classes, decompositions = read_unicode_data(unicode_data)
    pairs = {(a, b): c for a, b, c in compositions(classes, decompositions, read_exclusions(exclusions_file))}
    full = {c: full_decomposition(c, decompositions) for c in decompositions}

    def decompose(code):
        index = code - 0xAC00
        if 0 <= index < 11172:
            jamo = [0x1100 + index // 588, 0x1161 + (index % 588) // 28]
            return jamo + ([0x11A7 + index % 28] if index % 28 else [])
        return full.get(code, [code])

    def compose_pair(a, b):
        if 0 <= a - 0x1100 < 19 and 0 <= b - 0x1161 < 21:
            return 0xAC00 + ((a - 0x1100) * 21 + (b - 0x1161)) * 28
        if 0 <= a - 0xAC00 < 11172 and (a - 0xAC00) % 28 == 0 and 0 < b - 0x11A7 < 28:
            return a + b - 0x11A7
        return pairs.get((a, b))

    def nfc(text):
        chars = [c for ch in text for c in decompose(ord(ch))]
        out, run = [], []
        for c in chars + [None]:
            if c is not None and classes.get(c, 0):
                run.append(c)
                continue
            out += sorted(run, key=lambda x: classes[x])
            run = []
            if c is not None:
                out.append(c)
        result, starter, last = [], None, 0
        for c in out:
            klass = classes.get(c, 0)
            if starter is not None and (starter + 1 == len(result) or (last and last < klass)):
                composite = compose_pair(result[starter], c)
                if composite is not None:
                    result[starter] = composite
                    continue
            if klass == 0:
                starter = len(result)
            last = klass
            result.append(c)
        return "".join(map(chr, result))

    return nfc


def measure(versions):
    """Compares the oracle with the NFC of each Unicode version, for probes of each code point.

    The probes for a code point X are X, `a X U+0323` and `a X U+0334`. Planes 0 to 3
    and U+E0000 to U+E0FFF are complete; the other code points are a sample of
    each 97th one.
    """
    models = {name: nfc_model(os.path.join(path, "UnicodeData.txt"), os.path.join(path, "CompositionExclusions.txt"))
              for name, path in versions}
    codes = [c for c in range(0x110000) if not 0xD800 <= c < 0xE000 and (c < 0x40000 or 0xE0000 <= c < 0xE1000 or c % 97 == 0)]
    codes = [c for c in codes if c >= 0xA0 or chr(c).isalnum()]
    probes = [p for c in codes for p in (chr(c), "a%ṣ" % chr(c), "a%s̴" % chr(c))]
    differences = {name: [] for name, _ in versions}
    skipped = 0
    for start in range(0, len(probes), 100000):
        batch = probes[start : start + 100000]
        source = "".join("q%s\n\n" % text for text in batch)
        result = subprocess.run([PANDOC, "-f", "commonmark", "-t", "json"], input=source.encode(),
                                capture_output=True, check=True, timeout=1200)
        blocks = json.loads(result.stdout)["blocks"]
        if len(blocks) != len(batch):
            raise SystemExit("the oracle gave %d blocks for %d probes" % (len(blocks), len(batch)))
        for text, block in zip(batch, blocks):
            if block["t"] != "Para" or any(inline["t"] != "Str" for inline in block["c"]):
                skipped += 1
                continue
            actual = "".join(inline["c"] for inline in block["c"])
            for name, model in models.items():
                if model("q" + text) != actual:
                    differences[name].append(text)
        print("probes %d" % (start + len(batch)), {n: len(d) for n, d in differences.items()}, flush=True)
    print("code points %d, probes %d, skipped %d" % (len(codes), len(probes), skipped))
    for name, found in differences.items():
        print(name, len(found), " ".join("+".join("%04X" % ord(c) for c in t) for t in found[:12]))


if __name__ == "__main__":
    if sys.argv[1] == "--measure":
        measure([(os.path.basename(path.rstrip("/")), path) for path in sys.argv[2:]])
    else:
        main()
