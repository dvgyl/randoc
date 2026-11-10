#!/usr/bin/env python3
"""Differential test of the JSON reader and writer against the oracle.

The tool generates random documents and mutated documents. It runs
`randoc -f json -t json` and `pandoc -f json -t json` on each and compares
the exit code, the standard output and the standard error.

Usage: json_diff.py RANDOC PANDOC [COUNT] [SEED]
"""

import concurrent.futures
import json
import random
import struct
import subprocess
import sys

A_TAGS = ["AlignLeft", "AlignRight", "AlignCenter", "AlignDefault"]
STYLES = ["DefaultStyle", "Example", "Decimal", "LowerRoman", "UpperRoman", "LowerAlpha", "UpperAlpha"]
DELIMS = ["DefaultDelim", "Period", "OneParen", "TwoParens"]
CHARS = "ab xyz09\"\\/é中\U0001f600\n\r\u0001\u007f <&'"


class Gen:
    def __init__(self, rng):
        self.r = rng

    def text(self):
        return "".join(self.r.choice(CHARS) for _ in range(self.r.randint(0, 6)))

    def int(self):
        return self.r.choice([0, 1, 2, -1, 7, 2**31, -(2**40), 9223372036854775807])

    def double(self):
        k = self.r.random()
        if k < 0.3:
            d = struct.unpack("<d", struct.pack("<Q", self.r.getrandbits(64)))[0]
            return d if d == d and abs(d) != float("inf") else 0.5
        if k < 0.6:
            return self.r.random()
        return self.r.choice([0.0, 1.0, 1e23, 5e-324, 0.1, 1e7, 9999999.0, 1e-3, 123.456])

    def attr(self):
        return [self.text(), [self.text() for _ in range(self.r.randint(0, 2))],
                [[self.text(), self.text()] for _ in range(self.r.randint(0, 2))]]

    def tag(self, t, c=None):
        return {"t": t} if c is None else {"t": t, "c": c}

    def inlines(self, d):
        return [self.inline(d) for _ in range(self.r.randint(0, 3))]

    def blocks(self, d):
        return [self.block(d) for _ in range(self.r.randint(0, 3))]

    def citation(self, d):
        return {"citationId": self.text(), "citationPrefix": self.inlines(d), "citationSuffix": self.inlines(d),
                "citationMode": self.tag(self.r.choice(["AuthorInText", "SuppressAuthor", "NormalCitation"])),
                "citationNoteNum": self.int(), "citationHash": self.int()}

    def inline(self, d):
        if d <= 0:
            return self.tag("Str", self.text())
        d -= 1
        r = self.r
        k = r.randrange(20)
        simple = ["Emph", "Underline", "Strong", "Strikeout", "Superscript", "Subscript", "SmallCaps"]
        if k < 7:
            return self.tag(simple[k], self.inlines(d))
        return [
            lambda: self.tag("Str", self.text()),
            lambda: self.tag("Quoted", [self.tag(r.choice(["SingleQuote", "DoubleQuote"])), self.inlines(d)]),
            lambda: self.tag("Cite", [[self.citation(d) for _ in range(r.randint(0, 2))], self.inlines(d)]),
            lambda: self.tag("Code", [self.attr(), self.text()]),
            lambda: self.tag("Space"),
            lambda: self.tag("SoftBreak"),
            lambda: self.tag("LineBreak"),
            lambda: self.tag("Math", [self.tag(r.choice(["DisplayMath", "InlineMath"])), self.text()]),
            lambda: self.tag("RawInline", [self.text(), self.text()]),
            lambda: self.tag("Link", [self.attr(), self.inlines(d), [self.text(), self.text()]]),
            lambda: self.tag("Image", [self.attr(), self.inlines(d), [self.text(), self.text()]]),
            lambda: self.tag("Note", self.blocks(d)),
            lambda: self.tag("Span", [self.attr(), self.inlines(d)]),
        ][k - 7]()

    def cell(self, d):
        return [self.attr(), self.tag(self.r.choice(A_TAGS)), self.int(), self.int(), self.blocks(d)]

    def rows(self, d):
        return [[self.attr(), [self.cell(d) for _ in range(self.r.randint(0, 2))]] for _ in range(self.r.randint(0, 2))]

    def block(self, d):
        if d <= 0:
            return self.tag("HorizontalRule")
        d -= 1
        r = self.r
        k = r.randrange(14)
        return [
            lambda: self.tag("Plain", self.inlines(d)),
            lambda: self.tag("Para", self.inlines(d)),
            lambda: self.tag("LineBlock", [self.inlines(d) for _ in range(r.randint(0, 2))]),
            lambda: self.tag("CodeBlock", [self.attr(), self.text()]),
            lambda: self.tag("RawBlock", [self.text(), self.text()]),
            lambda: self.tag("BlockQuote", self.blocks(d)),
            lambda: self.tag("OrderedList", [[self.int(), self.tag(r.choice(STYLES)), self.tag(r.choice(DELIMS))],
                                             [self.blocks(d) for _ in range(r.randint(0, 2))]]),
            lambda: self.tag("BulletList", [self.blocks(d) for _ in range(r.randint(0, 2))]),
            lambda: self.tag("DefinitionList", [[self.inlines(d), [self.blocks(d)]] for _ in range(r.randint(0, 2))]),
            lambda: self.tag("Header", [self.int(), self.attr(), self.inlines(d)]),
            lambda: self.tag("HorizontalRule"),
            lambda: self.tag("Table", [self.attr(), [r.choice([None, self.inlines(d)]), self.blocks(d)],
                                       [[self.tag(r.choice(A_TAGS)), r.choice([self.tag("ColWidthDefault"), self.tag("ColWidth", self.double())])]
                                        for _ in range(r.randint(0, 3))],
                                       [self.attr(), self.rows(d)], [[self.attr(), self.int(), self.rows(d), self.rows(d)]],
                                       [self.attr(), self.rows(d)]]),
            lambda: self.tag("Figure", [self.attr(), [r.choice([None, self.inlines(d)]), self.blocks(d)], self.blocks(d)]),
            lambda: self.tag("Div", [self.attr(), self.blocks(d)]),
        ][k]()

    def meta_value(self, d):
        r = self.r
        if d <= 0:
            return self.tag("MetaBool", r.random() < 0.5)
        d -= 1
        return r.choice([
            lambda: self.tag("MetaMap", {self.text(): self.meta_value(d) for _ in range(r.randint(0, 2))}),
            lambda: self.tag("MetaList", [self.meta_value(d) for _ in range(r.randint(0, 2))]),
            lambda: self.tag("MetaBool", r.random() < 0.5),
            lambda: self.tag("MetaString", self.text()),
            lambda: self.tag("MetaInlines", self.inlines(d)),
            lambda: self.tag("MetaBlocks", self.blocks(d)),
        ])()

    def document(self):
        return {"pandoc-api-version": [1, 23, 1, 2],
                "meta": {self.text(): self.meta_value(2) for _ in range(self.r.randint(0, 3))},
                "blocks": self.blocks(3)}


def dump(value, rng):
    """Writes JSON with random number forms and spacing."""
    if isinstance(value, bool) or value is None:
        return json.dumps(value)
    if isinstance(value, int):
        return rng.choice([str(value), str(value), "%d.0" % value, "%de0" % value, "%d0e-1" % value])
    if isinstance(value, float):
        return repr(value)
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=rng.random() < 0.3)
    space = rng.choice(["", "", " ", "\n"])
    if isinstance(value, list):
        return "[" + ("," + space).join(dump(v, rng) for v in value) + "]"
    items = list(value.items())
    if rng.random() < 0.2:
        rng.shuffle(items)
    return "{" + ("," + space).join(json.dumps(k) + ":" + dump(v, rng) for k, v in items) + "}"


MUTATIONS = [
    lambda t, r, i: t[:i] + t[i + 1:],
    lambda t, r, i: t[:i] + r.choice('[]{}",:0-.e\\ntu ') + t[i:],
    lambda t, r, i: t[:i] + r.choice('[]{}",:0-.e\\5x') + t[i + 1:],
    lambda t, r, i: t[:i],
    lambda t, r, i: t.replace('"c":', '"c":5,"c":', 1),
    lambda t, r, i: t.replace('"t":"', '"t":"X', 1),
    lambda t, r, i: t.replace("[1,23,1,2]", r.choice(["[1,22]", "[1]", "null", "[1,23.0]", '"x"']), 1),
    lambda t, r, i: t.replace(":1", r.choice([":1.5", ":1e400", ":-0", ":null", ':"1"', ":1e19"]), 1),
    lambda t, r, i: t + r.choice([" x", "]", "\n", " ", "{}"]),
    lambda t, r, i: t.replace("[", "[5,", 1) if i % 2 else t.replace("]", ",5]", 1),
]


def make_case(seed):
    rng = random.Random(seed)
    text = dump(Gen(rng).document(), rng)
    if seed % 2:
        for _ in range(rng.randint(1, 3)):
            text = rng.choice(MUTATIONS)(text, rng, rng.randrange(max(1, len(text))))
    return text.replace("\t", " ").replace("\r", "")


def run(program, text):
    result = subprocess.run([program, "-f", "json", "-t", "json"], input=text.encode(), capture_output=True, timeout=60)
    return result.returncode, result.stdout, result.stderr


def compare(args):
    randoc, pandoc, seed = args
    text = make_case(seed)
    mine, theirs = run(randoc, text), run(pandoc, text)
    return seed, text, mine, theirs


def main():
    randoc, pandoc = sys.argv[1], sys.argv[2]
    count = int(sys.argv[3]) if len(sys.argv) > 3 else 2000
    first = int(sys.argv[4]) if len(sys.argv) > 4 else 0
    stats = {"total": 0, "same": 0, "accepted": 0, "different": 0}
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
        for seed, text, mine, theirs in pool.map(compare, [(randoc, pandoc, s) for s in range(first, first + count)]):
            stats["total"] += 1
            stats["accepted"] += theirs[0] == 0
            if mine == theirs:
                stats["same"] += 1
            else:
                stats["different"] += 1
                if stats["different"] <= 10:
                    print("seed %d differs\n input: %r\n randoc: %r\n pandoc: %r" % (seed, text[:300], mine, theirs))
    print(json.dumps(stats))
    return 0 if stats["different"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
