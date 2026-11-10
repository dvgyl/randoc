#!/usr/bin/env python3
"""Writes crates/randoc-core/tests/shared_oracle.json with results of the oracle.

Usage: shared_oracle.py RESEARCH_DIR UNICODE_DATA_TXT

The arguments are the same as for shared_tables.py. RANDOC_ORACLE_PANDOC is
the path of Pandoc 3.13. The inputs come from a random generator with a fixed
seed, so a second run gives the same file.
"""
import json
import os
import random
import subprocess
import sys

from shared_tables import categories, emojis

HERE = os.path.dirname(os.path.abspath(__file__))
OUTPUT = os.path.join(HERE, "..", "tests", "shared_oracle.json")
PANDOC = os.environ["RANDOC_ORACLE_PANDOC"]
API = [1, 23, 1, 2]

STRINGIFY_LUA = """
function Pandoc(doc)
  local blocks, meta = {}, {}
  for _, b in ipairs(doc.blocks) do blocks[#blocks + 1] = pandoc.utils.stringify(b) end
  for k, v in pairs(doc.meta) do meta[k] = pandoc.utils.stringify(v) end
  io.stdout:write(pandoc.json.encode({blocks = blocks, meta = meta}))
  return pandoc.Pandoc({})
end
"""

# The Markdown reader makes the identifier of a heading before it resolves an
# implicit heading reference, so such a link gives other text (I-SHARED-05).
# The modes turn the extension off to test the shared functions alone.
PANDOC_MODES = [
    "markdown-implicit_header_references+emoji",
    "markdown-implicit_header_references+emoji+gfm_auto_identifiers",
    "markdown-implicit_header_references+emoji+ascii_identifiers",
    "markdown-implicit_header_references+emoji+gfm_auto_identifiers+ascii_identifiers",
    "markdown-implicit_header_references-smart+emoji+gfm_auto_identifiers",
    "markdown-implicit_header_references+gfm_auto_identifiers",
]
COMMONMARK_MODES = [
    "gfm",
    "gfm+ascii_identifiers",
    "commonmark+gfm_auto_identifiers",
    "commonmark_x-implicit_header_references",
    "commonmark_x-implicit_header_references+ascii_identifiers",
    "commonmark_x-implicit_header_references-smart-emoji",
]
ATTRIBUTE_MODES = ("markdown", "commonmark_x")


def pandoc(args, text):
    result = subprocess.run([PANDOC] + args, input=text.encode(), capture_output=True, check=True)
    return result.stdout.decode()


def heading_texts(rng, cats, research):
    texts = []
    printable = [chr(c) for c in range(0x21, 0x7F) if chr(c) not in "{}\\#"]
    for c in printable:
        texts += [c, "a%sb" % c, "%s x" % c]
    for _ in range(300):
        texts.append("".join(rng.choice(printable + [" "] * 8) for _ in range(rng.randint(1, 12))).strip() or "x")
    by_category = {}
    for code, category in cats.items():
        by_category.setdefault(category, []).append(code)
    by_category["Cn"] = [c for c in range(0x80, 0x40000) if c not in cats and not 0xD800 <= c < 0xE000][::997]
    for category in sorted(by_category):
        if category in ("Cc", "Cs"):
            continue
        codes = by_category[category]
        for code in rng.sample(codes, min(25, len(codes))):
            c = chr(code)
            texts += [c, "a%sb" % c, "a %s b" % c, "%sx" % c]
    marks = [chr(c) for c, k in cats.items() if k.startswith("M")]
    for _ in range(150):
        base = rng.choice("aeiouAEIOUncकα")
        texts.append(base + "".join(rng.choice(marks) for _ in range(rng.randint(1, 3))) + " w")
    jamo = ([chr(c) for c in range(0x1100, 0x1113)], [chr(c) for c in range(0x1161, 0x1176)],
            [chr(c) for c in range(0x11A8, 0x11C3)])
    for _ in range(100):
        lead, vowel, tail = (rng.choice(part) for part in jamo)
        texts.append(lead + vowel + rng.choice(["", tail]) + " h")
    composed = [chr(c) for c in range(0xC0, 0x250) if cats.get(c, "").startswith("L")]
    for _ in range(100):
        texts.append(rng.choice(composed) + rng.choice(marks) + " d")
    _, by_emoji = emojis(research)
    tones = ["\U0001F3FB", "\U0001F3FC", "\U0001F3FD", "\U0001F3FE", "\U0001F3FF"]
    for emoji, _ in by_emoji:
        texts.append("x%sy" % emoji)
    for emoji, _ in rng.sample(by_emoji, 150):
        texts.append("%s%s z" % (emoji, rng.choice(tones)))
        texts.append("%s‍%s" % (emoji, rng.choice(by_emoji)[0]))
    texts += [
        "1. Intro", "33", "...", "!!!", "-x-", "_x_", "x.y.z", "3.14 pi", "١٢ x",
        "section", "section", "Section", "x", "x", "x-1", "x", "a  b", "a\tb", " lead",
        "*emph* **strong** `co de` $x^2$ ~~gone~~ x^sup^ x~sub~",
        "\"quoted\" 'single' it's", "[link](u) ![img](i.png) [span]{.c}",
        "<b>raw</b> @cite [@a; @b, p. 3]", ":smile: :+1: :nosuch:",
        "Crème brûlée à côté", "Why?--so *many* dashes?", "[A], [B2], and [C]?",
        "İstanbul K ΣΟΣ", "\U0001F1EB\U0001F1F7 flag", "1️⃣ key",
    ]
    texts += texts[-12:]
    return texts


def inline_pieces(rng, aliases):
    """Markdown source for each inline type and each attribute form, with a random word."""
    words = ["alpha", "Beta", "\u00e9t\u00e9", "x1", "3d", "\u00c4\u00d6", "\u0130z", "x", "foo"]
    w = lambda: rng.choice(words)
    makers = [
        w, w, w,
        lambda: "*%s*" % w(), lambda: "**%s**" % w(), lambda: "~~%s~~" % w(),
        lambda: "^%s^" % w(), lambda: "~%s~" % w(), lambda: "`%s %s`" % (w(), w()),
        lambda: "$x^2$", lambda: "$$%s$$" % w(),
        lambda: rng.choice(["<br>", "<br/>", "<br />", "<BR>", "<br class=x>", "<b>%s</b>" % w(), "<span>%s</span>" % w()]),
        lambda: "`%s`{=%s}" % (rng.choice(["<br>", "<b>", "\\\\"]), rng.choice(["html", "html5", "latex", "tex"])),
        lambda: "\\LaTeX{}",
        lambda: "[%s](u)" % w(), lambda: "![%s](i.png)" % w(), lambda: "<http://a.b/%s>" % w(),
        lambda: "^[note %s]" % w(), lambda: "@%s" % w(), lambda: "[@%s, p. 3]" % w(),
        lambda: "[%s]{.c}" % w(), lambda: "[%s]{#s%d .c k=v}" % (w(), rng.randrange(1000)),
        lambda: "[%s]{.emoji data-emoji=%s}" % (w(), rng.choice(aliases)),
        lambda: "[%s]{.emoji .b data-emoji=%s}" % (w(), rng.choice(aliases)),
        lambda: ":%s:" % rng.choice(aliases), lambda: ":%s:" % rng.choice(aliases),
        lambda: '"%s"' % w(), lambda: "'%s'" % w(), lambda: "%s's" % w(),
        lambda: rng.choice(["...", "--", "---", "&amp;", "&copy;", "&#x41;", "\\*", "\\_", "\U0001F44D", "\U0001F606"]),
        lambda: rng.choice(["`\U0001F44D`", "`a \U0001F606`", "$\U0001F44D$", "[x]{.emoji data-emoji=\U0001F606}",
                            "[x]{.emoji data-emoji=thumbs\U0001F606up}", "[\U0001F44D](u)", "![\U0001F44D](i.png)",
                            "^[\U0001F44D note]", "`\U0001F44D`{=html}", "*\U0001F44D*"]),
    ]
    return [rng.choice(makers)() for _ in range(rng.randint(1, 5))]


def structured_texts(rng, research):
    by_alias, _ = emojis(research)
    aliases = [alias for alias, _ in by_alias]
    texts = []
    for _ in range(1600):
        pieces = inline_pieces(rng, aliases)
        texts.append(rng.choice([" ", ""]).join(pieces))
    explicit = ["x", "x-1", "x-2", "section", "section-1", "alpha", "alpha-1", "foo", "beta", "s1"]
    for _ in range(300):
        texts.append("%s {#%s}" % (rng.choice(["x", "alpha", "foo", "Beta", "y"]), rng.choice(explicit)))
        texts.append(rng.choice(["x", "alpha", "foo", "Beta", "", "!"]))
    texts += ["x", "x", "y {#x}", "z {#x-1}", "x", "x", "y {#x}", "x"]
    # Identifiers of spans do not count as explicit heading identifiers.
    for _ in range(100):
        texts.append("[y]{#%s} %s" % (rng.choice(explicit), rng.choice(["x", "alpha", "foo"])))
        texts.append(rng.choice(["x", "alpha", "foo", "section"]))
    return texts


def explicit_ids(mode, texts):
    if not mode.startswith(ATTRIBUTE_MODES):
        return [None] * len(texts)
    out = []
    for text in texts:
        start = text.rfind("{#")
        out.append(text[start + 2 : -1] if start >= 0 and text.endswith("}") and " " not in text[start:] else None)
    return out


def identifier_docs(rng, cats, research):
    texts = heading_texts(rng, cats, research) + structured_texts(rng, research)
    source = "\n\n".join("# " + t for t in texts) + "\n"
    docs = []
    for mode in PANDOC_MODES + COMMONMARK_MODES:
        doc = json.loads(pandoc(["-f", mode, "-t", "json"], source))
        headers = [b for b in doc["blocks"] if b["t"] == "Header"]
        if len(headers) != len(doc["blocks"]) or len(headers) != len(texts):
            raise SystemExit("%s: %d headers for %d texts" % (mode, len(headers), len(texts)))
        docs.append({"from": mode, "doc": doc, "explicit": explicit_ids(mode, texts)})
    return docs, len(texts)


def attr(rng):
    return ["", rng.choice([[], ["c"]]), []]


def words(rng):
    pool = ["a", "B", "été", "x y", "Σ", "1", "--", "’", "&", "\U0001F604", ""]
    return rng.choice(pool)


def inline(rng, depth):
    leaves = [
        lambda: {"t": "Str", "c": words(rng) or "s"},
        lambda: {"t": "Space"},
        lambda: {"t": "SoftBreak"},
        lambda: {"t": "LineBreak"},
        lambda: {"t": "Code", "c": [attr(rng), "co de"]},
        lambda: {"t": "Math", "c": [{"t": rng.choice(["InlineMath", "DisplayMath"])}, "x^2"]},
        lambda: {"t": "RawInline", "c": [rng.choice(["html", "html5", "HTML", "tex", "latex"]),
                                         rng.choice(["<b>", "<br>", "<br/>", "<BR>", " <br>", "<br", "<bra>", "\\\\"])]},
    ]
    if depth <= 0:
        return rng.choice(leaves)()
    nested = [
        lambda t: {"t": t, "c": inlines(rng, depth - 1)},
    ]
    kind = rng.randint(0, 14)
    if kind < 6:
        return rng.choice(leaves)()
    if kind < 9:
        tag = rng.choice(["Emph", "Underline", "Strong", "Strikeout", "Superscript", "Subscript", "SmallCaps"])
        return nested[0](tag)
    if kind == 9:
        quote = rng.choice(["SingleQuote", "DoubleQuote"])
        return {"t": "Quoted", "c": [{"t": quote}, inlines(rng, depth - 1)]}
    if kind == 10:
        citation = {
            "citationId": "k",
            "citationPrefix": inlines(rng, depth - 1),
            "citationSuffix": inlines(rng, depth - 1),
            "citationMode": {"t": "NormalCitation"},
            "citationNoteNum": 0,
            "citationHash": 0,
        }
        return {"t": "Cite", "c": [[citation], inlines(rng, depth - 1)]}
    if kind == 11:
        return {"t": rng.choice(["Link", "Image"]), "c": [attr(rng), inlines(rng, depth - 1), ["u", "title"]]}
    if kind == 12:
        return {"t": "Note", "c": [{"t": "Para", "c": inlines(rng, depth - 1)}]}
    return {"t": "Span", "c": [attr(rng), inlines(rng, depth - 1)]}


def inlines(rng, depth):
    return [inline(rng, depth) for _ in range(rng.randint(0, 4))]


def caption(rng):
    short = inlines(rng, 1) if rng.random() < 0.5 else None
    return [short, [{"t": "Plain", "c": inlines(rng, 1)}]]


def cell(rng, depth):
    return [attr(rng), {"t": "AlignDefault"}, 1, 1, blocks(rng, depth - 1)]


def rows(rng, depth):
    return [[attr(rng), [cell(rng, depth) for _ in range(2)]] for _ in range(rng.randint(0, 2))]


def block(rng, depth):
    para = lambda: {"t": rng.choice(["Para", "Plain"]), "c": inlines(rng, 2)}
    if depth <= 0:
        return para()
    choices = [
        para,
        lambda: {"t": "Header", "c": [2, attr(rng), inlines(rng, 2)]},
        lambda: {"t": "LineBlock", "c": [inlines(rng, 1) for _ in range(2)]},
        lambda: {"t": "CodeBlock", "c": [attr(rng), "code"]},
        lambda: {"t": "RawBlock", "c": ["html", "<x>"]},
        lambda: {"t": "HorizontalRule"},
        lambda: {"t": "BlockQuote", "c": blocks(rng, depth - 1)},
        lambda: {"t": "Div", "c": [attr(rng), blocks(rng, depth - 1)]},
        lambda: {"t": "BulletList", "c": [blocks(rng, depth - 1) for _ in range(2)]},
        lambda: {
            "t": "OrderedList",
            "c": [[1, {"t": "Decimal"}, {"t": "Period"}], [blocks(rng, depth - 1) for _ in range(2)]],
        },
        lambda: {"t": "DefinitionList", "c": [[inlines(rng, 1), [blocks(rng, depth - 1)]] for _ in range(2)]},
        lambda: {"t": "Figure", "c": [attr(rng), caption(rng), blocks(rng, depth - 1)]},
        lambda: {
            "t": "Table",
            "c": [
                attr(rng),
                caption(rng),
                [[{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]] * 2,
                [attr(rng), rows(rng, depth)],
                [[attr(rng), 0, rows(rng, depth), rows(rng, depth)]],
                [attr(rng), rows(rng, depth)],
            ],
        },
    ]
    return rng.choice(choices)()


def blocks(rng, depth):
    return [block(rng, depth) for _ in range(rng.randint(0, 2))]


def meta_value(rng, depth):
    kinds = ["MetaBool", "MetaString", "MetaInlines", "MetaBlocks"]
    if depth > 0:
        kinds += ["MetaList", "MetaMap"]
    kind = rng.choice(kinds)
    if kind == "MetaBool":
        return {"t": kind, "c": rng.random() < 0.5}
    if kind == "MetaString":
        return {"t": kind, "c": words(rng)}
    if kind == "MetaInlines":
        return {"t": kind, "c": inlines(rng, 2)}
    if kind == "MetaBlocks":
        return {"t": kind, "c": blocks(rng, 1)}
    if kind == "MetaList":
        return {"t": kind, "c": [meta_value(rng, depth - 1) for _ in range(rng.randint(0, 3))]}
    return {"t": kind, "c": {"k%d" % i: meta_value(rng, depth - 1) for i in rng.sample(range(20), 3)}}


def stringify_case(rng):
    body = [{"t": "Para", "c": inlines(rng, 3)} for _ in range(600)]
    body += [block(rng, 3) for _ in range(200)]
    meta = {"m%03d" % i: meta_value(rng, 2) for i in range(60)}
    doc = {"pandoc-api-version": API, "meta": meta, "blocks": body}
    lua = os.path.join(os.environ.get("TMPDIR", "/tmp"), "shared-stringify.lua")
    with open(lua, "w", encoding="utf-8") as out:
        out.write(STRINGIFY_LUA)
    try:
        expected = json.loads(pandoc(["-f", "json", "-t", "json", "-L", lua, "-o", "/dev/null"], json.dumps(doc)))
    finally:
        os.remove(lua)
    return {"doc": doc, "blocks": expected["blocks"], "meta": expected["meta"]}


def entity_cases(rng, research):
    from shared_tables import entities

    refs = ["&" + name for name, _ in entities(research)]
    pieces = ["#", "#x", "#X", "x", "amp", "AMP", "Amp", "1", "41", "FFFD", "0", ";", ";;", "D800", "110000"]
    malformed = ["&#;", "&#x;", "&#xG;", "&#12a;", "&# 1;", "& amp;", "&#000065;", "&#x0;", "&#0;",
                 "&#99999999999999999999;", "&#xFFFFFFFFFF;", "&#xD800;", "&#xDFFF;", "&#1114111;",
                 "&#1114112;", "&unknown;", "&amp;amp;", "&;", "&", "&&amp;", "&#65", "&#x41"]
    while len(malformed) < 320:
        malformed.append("&" + "".join(rng.choice(pieces) for _ in range(rng.randint(1, 4))))
    inputs = refs + malformed
    source = "\n\n".join("X%sY" % ref for ref in inputs) + "\n"
    lua = os.path.join(os.environ.get("TMPDIR", "/tmp"), "shared-entities.lua")
    with open(lua, "w", encoding="utf-8") as out:
        out.write(STRINGIFY_LUA)
    try:
        expected = json.loads(pandoc(["-f", "markdown", "-t", "json", "-L", lua, "-o", "/dev/null"], source))
    finally:
        os.remove(lua)
    return [[ref, text] for ref, text in zip(inputs, expected["blocks"])]


def emoji_cases(research):
    by_alias, _ = emojis(research)
    aliases = [alias for alias, _ in by_alias] + ["nosuch", "Smile", "smile_", ""]
    source = "\n\n".join("x :%s: y" % alias for alias in aliases) + "\n"
    doc = json.loads(pandoc(["-f", "markdown+emoji", "-t", "json"], source))
    cases = []
    for alias, para in zip(aliases, doc["blocks"]):
        spans = [i for i in para["c"] if i["t"] == "Span"]
        cases.append([alias, spans[0]["c"][1][0]["c"] if spans else None])
    return cases


def main():
    research, unicode_data = sys.argv[1], sys.argv[2]
    rng = random.Random(20261110)
    cats = categories(unicode_data)
    identifiers, count = identifier_docs(rng, cats, research)
    stringify = stringify_case(rng)
    entity = entity_cases(rng, research)
    emoji = emoji_cases(research)
    # An array of arrays, so that the test can read it into tuples with no extra dependency.
    data = [
        [[case["from"], case["doc"], case["explicit"]] for case in identifiers],
        [stringify["doc"], stringify["blocks"], stringify["meta"]],
        entity,
        emoji,
    ]
    with open(OUTPUT, "w", encoding="utf-8") as out:
        json.dump(data, out, ensure_ascii=False, separators=(",", ":"))
        out.write("\n")
    print("headings per mode:", count, "entities:", len(entity), "emoji:", len(emoji))


if __name__ == "__main__":
    main()
