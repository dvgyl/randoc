#!/usr/bin/env python3
"""Writes crates/randoc-core/src/shared/tables.rs.

Usage: shared_tables.py RESEARCH_DIR UNICODE_DATA_TXT

RESEARCH_DIR holds the BSD sources `commonmark-hs` (commonmark 0.3.1) and
`emojis` (0.1.5). UNICODE_DATA_TXT is `UnicodeData.txt` of Unicode 15.1.0,
the version of the GHC tables in Pandoc 3.13:
https://www.unicode.org/Public/15.1.0/ucd/UnicodeData.txt
(SHA-256 2fc713e6a31a87c4850a37fe2caffa4218180fadb5de86b43a143ddb4581fb86).
"""
import os
import re
import sys
import unicodedata

HERE = os.path.dirname(os.path.abspath(__file__))
OUTPUT = os.path.join(HERE, "..", "src", "shared", "tables.rs")


def haskell_string(body):
    out, i = [], 0
    while i < len(body):
        c = body[i]
        if c != "\\":
            out.append(c)
            i += 1
            continue
        nxt = body[i + 1]
        if nxt == "&":
            i += 2
        elif nxt == "x":
            m = re.match(r"[0-9A-Fa-f]+", body[i + 2:])
            out.append(chr(int(m.group(0), 16)))
            i += 2 + len(m.group(0))
        elif nxt.isdigit():
            m = re.match(r"[0-9]+", body[i + 1:])
            out.append(chr(int(m.group(0))))
            i += 1 + len(m.group(0))
        else:
            out.append({"n": "\n", "t": "\t", '"': '"', "\\": "\\"}[nxt])
            i += 2
    return "".join(out)


PAIR = re.compile(r'\(\s*"((?:[^"\\]|\\.)*)"\s*,\s*"((?:[^"\\]|\\.)*)"\s*\)')


def entities(research):
    path = os.path.join(research, "commonmark-hs", "commonmark", "src", "Commonmark", "Entity.hs")
    text = open(path, encoding="utf-8").read()
    text = text[text.index("htmlEntities =") :]
    pairs = [(haskell_string(a), haskell_string(b)) for a, b in PAIR.findall(text)]
    return sorted(dict(pairs).items(), key=lambda p: p[0].encode())


def emojis(research):
    path = os.path.join(research, "emojis", "src", "Text", "emojis.inc")
    text = open(path, encoding="utf-8").read()
    text = text[text.index("emojis =") : text.index("baseEmojis =")]
    pairs = [(haskell_string(a), haskell_string(b)) for a, b in PAIR.findall(text)]
    by_alias = dict(pairs)
    by_emoji = {}
    for alias, emoji in pairs:
        by_emoji.setdefault(emoji, alias)
    return (
        sorted(by_alias.items(), key=lambda p: p[0].encode()),
        sorted(by_emoji.items(), key=lambda p: p[0].encode()),
    )


def categories(path):
    table = {}
    first = None
    for line in open(path, encoding="utf-8"):
        fields = line.split(";")
        code, name, category = int(fields[0], 16), fields[1], fields[2]
        if name.endswith(", First>"):
            first = code
            continue
        span = range(first, code + 1) if name.endswith(", Last>") else range(code, code + 1)
        first = None
        for c in span:
            table[c] = category
    return table


def ranges(codes):
    out = []
    for code in sorted(codes):
        if out and out[-1][1] + 1 == code:
            out[-1][1] = code
        else:
            out.append([code, code])
    return out


def rust_str(text):
    out = []
    for c in text:
        if c in '"\\':
            out.append("\\" + c)
        elif 0x20 <= ord(c) < 0x7F:
            out.append(c)
        else:
            out.append("\\u{%X}" % ord(c))
    return '"' + "".join(out) + '"'


def write_pairs(out, name, doc, pairs):
    out.write("/// %s\n" % doc)
    out.write("pub(super) static %s: [(&str, &str); %d] = [\n" % (name, len(pairs)))
    for a, b in pairs:
        out.write("    (%s, %s),\n" % (rust_str(a), rust_str(b)))
    out.write("];\n\n")


CLASSES = {"L": "Letter", "N": "Number", "M": "Mark", "Pc": "Connector", "Zs": "Space"}


def write_classes(out, cats):
    rows = []
    for code in sorted(cats):
        category = cats[code]
        name = CLASSES.get(category) or CLASSES.get(category[0])
        if name is None:
            continue
        if rows and rows[-1][1] + 1 == code and rows[-1][2] == name:
            rows[-1][1] = code
        else:
            rows.append([code, code, name])
    out.write("/// Ranges of code points and their class, from the general categories.\n")
    out.write("pub(super) static CLASSES: [(u32, u32, Class); %d] = [\n" % len(rows))
    for low, high, name in rows:
        out.write("    (0x%04X, 0x%04X, Class::%s),\n" % (low, high, name))
    out.write("];\n\n")


def write_chars(out, name, doc, pairs):
    out.write("/// %s\n" % doc)
    out.write("pub(super) static %s: [(char, char); %d] = [\n" % (name, len(pairs)))
    for a, b in pairs:
        out.write("    ('\\u{%X}', '%s'),\n" % (ord(a), b.replace("\\", "\\\\").replace("'", "\\'")))
    out.write("];\n\n")


def pandoc_ascii(cats):
    # The oracle maps U+0131 to `i`. It has no canonical decomposition.
    pairs = {"ı": "i"}
    for code in cats:
        if code < 0x80:
            continue
        first = unicodedata.normalize("NFD", chr(code))[0]
        if ord(first) < 0x80:
            pairs[chr(code)] = first
    return sorted(pairs.items())


def commonmark_ascii(research):
    path = os.path.join(
        research, "commonmark-hs", "commonmark-extensions", "src", "Commonmark", "Extensions", "AutoIdentifiers.hs"
    )
    text = open(path, encoding="utf-8").read()
    text = text[text.index("asciiMap =") :]
    pairs = re.findall(r"\('((?:[^'\\]|\\[0-9]+)+)','(.)'\)", text)
    return sorted((haskell_string(a), b) for a, b in pairs)


def main():
    research, unicode_data = sys.argv[1], sys.argv[2]
    if unicodedata.unidata_version != "16.0.0":
        raise SystemExit("PANDOC_ASCII needs the decompositions of Unicode 16.0.0, Python has %s" % unicodedata.unidata_version)
    cats = categories(unicode_data)
    by_alias, by_emoji = emojis(research)
    with open(OUTPUT, "w", encoding="utf-8") as out:
        out.write("//! Tables for `randoc_core::shared`.\n")
        out.write("//! `tools/shared_tables.py` writes this file. Do not edit it.\n\n")
        write_pairs(out, "ENTITIES", "HTML named character references of `commonmark` 0.3.1, sorted by name.", entities(research))
        write_pairs(out, "EMOJI_BY_ALIAS", "Emoji aliases of `emojis` 0.1.5 and their emoji, sorted by alias.", by_alias)
        write_pairs(out, "ALIAS_BY_EMOJI", "Each emoji of `emojis` 0.1.5 and its first alias, sorted by emoji.", by_emoji)
        starts = sorted({emoji[0] for emoji, _ in by_emoji})
        out.write("/// The first character of each emoji in `ALIAS_BY_EMOJI`, sorted.\n")
        out.write("pub(super) static EMOJI_STARTS: [char; %d] = [\n" % len(starts))
        for c in starts:
            out.write("    '\\u{%X}',\n" % ord(c))
        out.write("];\n\n")
        out.write("use super::identifiers::Class;\n\n")
        write_classes(out, cats)
        write_chars(out, "PANDOC_ASCII", "The ASCII form of a character: the first character of its canonical decomposition.", pandoc_ascii(cats))
        write_chars(out, "COMMONMARK_ASCII", "The `asciiMap` of `commonmark-extensions` 0.2.7.3.", commonmark_ascii(research))


if __name__ == "__main__":
    main()
