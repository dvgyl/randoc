#!/usr/bin/env python3
"""Writes the oracle matrices of tools/commonmark-cases/matrices.json.

Each matrix holds one input for each combination of its axes. The inputs are
written for Randoc. The file keeps the oracle result of each input as replay
data. The crate test `case_tests::oracle_matrices_match` reads it, so that
the matrices also run with reader options that the program does not accept
(for example `strip_comments`).

Run: python3 tools/commonmark-cases/matrices.py            (record with the oracle)
     python3 tools/commonmark-cases/matrices.py --only A,B  (record only the matrices A and B)
     python3 tools/commonmark-cases/matrices.py --compare [A,B]  (compare target/debug/randoc)
"""

import json
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "tools/commonmark-cases/matrices.json"
LIMIT = int(os.environ.get("MATRIX_SHOW", "20"))
FORMATS = ["commonmark", "gfm", "commonmark_x"]

COMMENTS = [
    "<!--b-->", "<!---->", "<!-->", "<!--->", "<!--a--->", "<!-- a -- b -->",
    "<!--a\nb-->", "<!--b", "<!-<!--a-->-b-->", "<!--a--><!--b-->", "<!--a-->-->",
]
COMMENT_PLACES = [
    "{C}", "{C} x", "{C}\n\nx", "a{C}c", "x {C} y", "<div>{C}c</div>", "<div>\n{C}\n</div>",
    "x <a title=\"{C}\">", "`{C}`{{=html}}", "```{{=html}}\n{C}\n```", "`{C}`{{=latex}}",
    "> {C}", "- {C}", "- a\n\n  {C}", "1. x {C}", "| a |\n|---|\n| x{C} |",
    "x[^1]\n\n[^1]: y {C}", "t\n\n: d {C}", "::: d\n{C}\n:::", "# h {C}", "[l{C}](u)",
    "`{C}`", "---\ntitle: a{C}c\n---\n\nx {C}",
]


def strip_comments():
    for comment in COMMENTS:
        for place in COMMENT_PLACES:
            text = place.format(C=comment) + "\n"
            for fmt in FORMATS:
                yield {"from": fmt, "strip_comments": True, "input": text}


LIST_BLOCKS = [
    "x", "[g]: /u", "[^1]: n", "{.a}", "{.a}\nx", "<div>x</div>", "<!-- c -->", "    code",
    "```\ncode\n```", "- n", "- n\n\n  m", "t\n\n: d", "t\n: d", "> q", "# h", "[g]: /u\n[h]: /v",
]
LIST_SHAPES = [
    # (marker of the first item, text after the block)
    ("- ", ""), ("- ", "- b\n"), ("1. ", ""), ("- ", "\nx\n"), ("- ", "\n\n- b\n"),
]


def indent(text, width):
    return "\n".join((" " * width + line) if line else "" for line in text.split("\n"))


def lists():
    for block in LIST_BLOCKS:
        for marker, after in LIST_SHAPES:
            for blanks in ("", "\n", "\n\n"):
                body = f"{marker}a\n{blanks}{indent(block, len(marker))}\n{after}"
                for quote in (False, True):
                    text = indent_quote(body) if quote else body
                    for fmt in FORMATS:
                        yield {"from": fmt, "input": text}
    for fmt in FORMATS:
        for text in LIST_EXTRA:
            yield {"from": fmt, "input": text}


def indent_quote(text):
    return "".join("> " + line + "\n" for line in text.rstrip("\n").split("\n"))


# The inputs of the round 6 findings and of I-CM-15, and the continuation of a
# definition inside a loose list.
LIST_EXTRA = [
    "- a\n\n  [g]: /u\n", "- a\n- b\n\n  [g]: /u\n", "1. a\n2. b\n\n   [g]: /u\n\nx [g]\n",
    "- {.a}\n\n  {.b}\n  x\n", "- a\n\n  {.a}\n", "- a\n  [g]: /u\n\n- b\n",
    ">- {.a}\n>   {.b\n>   }\n>   #\n", "- {.a}\n  {.b\n  }\n  # h\n", "- {.a}\n  ```\n  c\n  ```\n",
    "- t\n\n  : d\n  : e\n", "- t\n\n  : d\n\n    e\n", "- t\n\n  : d\n    e\n",
    "- t\n\n  : d\n\n      e\n", "- x\n- t\n\n  : d\n  : e\n", "1. t\n\n   : d\n   : e\n",
    "> - t\n>\n>   : d\n>   : e\n", "- t\n\n  : d\n\n  : e\n", "- t\n  : d\n  : e\n",
    "- a\n\n\n  b\n", "- a\n  - b\n\n  c\n", "- a\n  - b\n\n- c\n", "- a\n\n  - b\n",
    "- a\n  - b\n\n    c\n", "-\n\n  a\n", "- a\n  ```\n\n  ```\n",
    "-\n  a\n", "-\n- x\n-\n", "- [ ]\n- [ ] b\n", "- [x]\n  * b\n", "-\n\n\n- b\n", "1.\n   a\n\n2. b\n",
]


def single_quotes():
    defs = "\n\n[r]: /r\n[a]: /a\n"
    forms = [
        "[a](u)", "[a](u){#i}", "[a](u){.c}", "[a][r]", "[a][r]{.c}", "[a][]", "[r][]", "[r][]{.c}",
        "[a]", "[r]", "[r]{.c}", "[a][x]", "![a](u)", "![a](u){.c}", "![r]", "![a][]", "[a]{.c}",
        "[a]{#i}", "[[a]](u)", "[a](u \"t\")", "[a]()", "[a](<u>)", "[a] [r]", "[^1]",
    ]
    tails = ["'b'", "'s dog'", "'", "'b", "\"b\"", "' b'", "'.", "'b' c"]
    for form in forms:
        for tail in tails:
            for prefix in ("", "'x ", "x "):
                for fmt in ("commonmark_x", "commonmark+smart", "gfm+smart"):
                    yield {"from": fmt, "input": f"{prefix}{form}{tail}{defs}"}


META_FORMATS = ["commonmark+yaml_metadata_block", "gfm+yaml_metadata_block", "commonmark_x"]
# Each value form of a YAML scalar. A line break in a form continues the
# value on the next line; the place indents that line.
META_SCALARS = [
    "x", "two words", "*e*", "# h", "- l", "[l](u)", "`c`", "<b>x</b>", "a\\b", "It's \"q\"",
    "'single'", "\"double\"", "\"p1\\n\"", "\"p1\\n\\np2\"", "\"\"", "''", "\"  x  \"", "\"a  \\nb\"",
    "\"x\\ty\"", "é", "|\n  a\n\n  b", ">-\n  a\n  b", "|+\n  a\n", "|-\n  *e*", ">\n  # h",
    "12", "-0", "0x1F", "0o17", "1e3", "1.0", ".5", "1.5e-7", ".inf", "-.inf", ".nan",
    "12345678901234567890", "007", "+12", "true", "False", "yes", "off", "null", "~", "", "Null",
    "!!str 12", "!!str true", "!!int \"7\"", "!!float 3", "!!bool \"true\"", "!!null \"\"", "! 12",
    "!x y", "!!timestamp 2001-01-01", "!!binary aGk=", "!!x ~", "&a x", "!!str", "! ~",
]
META_PLACES = [
    ("a: {V}\n", ""), ("a:\n  - {V}\n", "    "), ("a:\n  b: {V}\n", "    "),
    ("a: [{V}]\n", None), ("a: {{b: {V}}}\n", None),
]
META_KEYS = [
    "a", "a_", "_a", "1", "1.0", "0x10", "true", "null", "~", "\"q k\"", "'s'", "!!str 3", "a b",
    "é", "-1", ".nan", "\"\"", "\"a_\"",
]


def metadata_values():
    for value in META_SCALARS:
        for place, more in META_PLACES:
            if more is None:
                if "\n" in value or value.startswith(("|", ">", "&")):
                    continue
                text = place.format(V=value)
            else:
                text = place.format(V=value.replace("\n", "\n" + more))
            for fmt in META_FORMATS:
                yield {"from": fmt, "input": f"---\n{text}---\n\nx\n"}
    for key in META_KEYS:
        for text in (f"{key}: x\n", f"m:\n  {key}: x\n  b: y\n"):
            for fmt in META_FORMATS:
                yield {"from": fmt, "input": f"---\n{text}---\n\nx\n"}


# The inputs of the YAML findings of round 6 (Codex R6-02, R6-03, Opus R6-01)
# and the forms of row I-CM-04.
YAML_FINDINGS = [
    "a: !!x ~\n", "a: !!timestamp ~\n", "a: !!x true\n", "a: !!x -1\n", "a: !!x 0x1\n",
    "a: !!seq\n", "a: !!map x\n", "a: !!seq\n  b: c\n", "a: !!map\n  - b\n", "a: &x x\nb: ! *x\n",
    "a: &x x\nb: &y *x\n", "title: \"A long\ntitle\"\n", "a: 'x\ny'\n", "b:\n  a: \"x\n  y\"\n",
    "a:\n  - \"x\n  y\"\n", "a: \"x\n\ny\"\n", "a: \"a\\\nb\"\n", "title: \"A long\n title\"\n",
    "a: [1,\n2]\n", "a: [-1, - 1]\n", "? a\n: b\n", "x: {? b : c}\n", "?a: b\n", "a: ?x\n",
    "a: !<tag:yaml.org,2002:str> 1\n", "&k a: b\n", "a: &x b\n*x : c\n", "%YAML 1.2\n",
    "{a: 1}: x\n", "a: !!str\n  - x\n", "a: !<x> 1\n", "a: |0\n",
]


def yaml_findings():
    for text in YAML_FINDINGS:
        for fmt in META_FORMATS:
            yield {"from": fmt, "input": f"---\n{text}---\n\nx\n"}


# `linkPath` of Autolink.hs: each character after a link path, then each
# follower. One document holds one paragraph for each character.
LINK_BASES = ["http://a.b", "www.a.b", "x http://a.b/p"]
LINK_OPENS = ["", "(q", "[q", "&amp;"]
LINK_CHARS = list("!\"')*,.:;?_~<&()[]x") + [" ", " ", "\f", "&amp;", "&a;b", "..", ";<", "?)"]
LINK_FOLLOWERS = ["", " z", "x", "<b", ")", "]", "&amp;", "&amp; ", "(", "]x", "&x;.", "*"]


def link_paths():
    for base in LINK_BASES:
        for opened in LINK_OPENS:
            for follower in LINK_FOLLOWERS:
                paras = [f"{base}{opened}{c}{follower}" for c in LINK_CHARS]
                for fmt in ("gfm", "commonmark+autolink_bare_uris"):
                    yield {"from": fmt, "input": "\n\n".join(paras) + "\n"}


# `pLinkDestination`, `pLinkTitle` and `pLinkLabel` of Inlines.hs, and the
# reference definition scanners: each destination, title and label form.
def nested(n):
    return "(" * n + "x" + ")" * n


LINK_DESTS = [
    "u", "a\u00a0b", "a\u2003b", "a\x01b", "a\x7fb", "a\x1fb", "a\fb", nested(31), nested(32),
    nested(33), "a(b", "a)b", "(a", "a\\(b", "a\\b", "a\\\\b", "a\\é", "a&amp;b", "a&#x41;b", "a&nope;b",
    "a%20b", "é", "<a b>", "<a\nb>", "<a\\>b>", "<>", "<a<b>", "<a&amp;>", "<a\\\nb>", "a<b", "a>b",
    "*a*", "a`b", "a\"b",
]
LINK_TITLES = [
    "\"t\"", "'t'", "(t)", "(t(x))", "(t\\(x)", "\"t\\\"q\"", "\"a\nb\"", "\"&amp;\"", "\"\\*\"",
    "\"a\\\nb\"", "'a\"b'", "\"\"", "\"t\" x", "(t", "\"é&#xe9;\"",
]
LINK_LABELS = [
    "r", "R", "r ", " r", "r\nx", "a\\]b", "a[b", "ẞ", "ss", "a  b", "*r*", "`r`", "x" * 998, "x" * 999,
    "x" * 1000, "\\", "a\\\nb", "\u00a0r", "é" * 500, "é" * 999, "é" * 1000, "r\u2028", "r\f", "\u3000r",
]


def link_scanners():
    for dest in LINK_DESTS:
        paras = [f"[a]({dest})", f"[a]({dest} \"t\")", f"![a]({dest})", f"[a]( {dest} )",
                 f"[a]({dest}\u00a0\"t\")", f"x [r]\n\n[r]: {dest}\n\ny [r]\n"]
        for fmt in ("commonmark", "gfm"):
            for para in paras:
                yield {"from": fmt, "input": para + "\n"}
    for title in LINK_TITLES:
        for para in (f"[a](u {title})", f"[a](<u> {title})", f"[a](u\n{title})",
                     f"[r]\n\n[r]: u {title}\n", f"[r]\n\n[r]: u\n{title}\n", f"[a](u{title})"):
            for fmt in ("commonmark", "gfm"):
                yield {"from": fmt, "input": para + "\n"}
    for label in LINK_LABELS:
        for para in (f"[x][{label}]\n\n[{label}]: /u\n", f"[{label}]\n\n[{label}]: /u\n",
                     f"[{label}][]\n\n[R]: /u\n[ss]: /v\n", f"[x][{label}]\n\n[r]: /u\n[ẞ]: /v\n"):
            for fmt in ("commonmark", "gfm"):
                yield {"from": fmt, "input": para}


# `processBs` of Inlines.hs: random sequences of brackets, link suffixes,
# image and note openers and words, with the definitions of `a` and `b`.
BRACKET_PARTS = ["[", "]", "![", "(u)", "a", "b", " ", "[]", "[a]", "{.c}", "^", "*", "\\[", "'", "(", ")", "[^n]"]


def brackets():
    import random
    rng = random.Random(3)
    for _ in range(60):
        paras = ["".join(rng.choice(BRACKET_PARTS) for _ in range(rng.randint(2, 9))) for _ in range(30)]
        text = "\n\n".join(p.strip() or "x" for p in paras) + "\n\n[a]: /a\n[b]: /b 't'\n[^n]: note\n"
        for fmt in ("commonmark", "gfm", "commonmark_x"):
            yield {"from": fmt, "input": text}


# The extension modules of commonmark-extensions: the forms of each rule.
EXTENSION_FORMS = {
    "smart": (["commonmark+smart", "commonmark_x", "gfm+smart"], [
        *[f"a{'-' * n}b" for n in range(1, 14)], "-- a", "a --", "--- a ---",
        *[f"a{'.' * n}b" for n in range(1, 10)], "'a'", "\"a\"", "a'b", "'a", "\"a", "''a''", "\"'a'\"",
        "'\"a\"'", "a\"b\"c", "5'6\"", "'90s", "('a')", "[\"a\"]", "*'a'*", "'*a*'", "é'é", "1'2",
        "'a'.", "\"a\",", "' a '", "\" a \"", "a' 'b", "'a' 'b'", "\"a 'b' c\"", "'a \"b\" c'",
        "`'a'`", "'a\nb'", "\\'a'", "&quot;a&quot;", "a--b--c", "'" * 3 + "a" + "'" * 3,
    ]),
    "emoji": (["gfm", "commonmark+emoji"], [
        ":smile:", ":+1:", ":-1:", ":a b:", "::", ":smile", "a:smile:b", ":SMILE:", ":smile::smile:",
        ":100:", ":e-mail:", ":x_y:", "\\:smile:", "1:smile:", ":smile:1", "*:smile:*", ":heart: x",
    ]),
    "math": (["commonmark+tex_math_dollars", "commonmark_x", "gfm"], [
        "$a$", "$$a$$", "$ a$", "$a $", "$a$b", "1$a$", "$a\\$b$", "$$a\n$$", "$$", "$a", "$`a`$",
        "$$\na\n$$", "a $b$ c", "$a$$b$", "$$a$ b", "$\\$$", "$a\nb$", "$*a*$", "$$ $$", "\\$a$",
        "```math\nx\n```", "$1 and $2", "$a$.", "($a$)", "$a}b$", "${a}$", "${a$}$", "$a{$ b$", "$$a$b$$",
        "$a$5", "$a$x", "$\\{$", "$a\\\nb$", "$a${.c}", "$ $", "$$a{$$}$$", "$" + "{" * 1001 + "$",
        "$a$$", "$$a$", "$a$_b",
    ]),
    "task-list": (["gfm", "commonmark+task_lists"], [
        "- [ ] a", "- [x] a", "- [X] a", "- []", "- [ ]", "- [ ]a", "* [x]\ta", "1. [x] a", "- [ ] ",
        "- [x] [x] a", "- [-] a", "- [ ] a\n\n- [x] b", "- [ ] a\n  - [x] b", "- [x]\n  a", "+ [x] a",
        "- [ ]  a", "- [x]\n\n  b", "> - [x] a", "-  [ ] a", "- [x]\n\n\n  b", "- [x]\n\n  b\n- [ ] c",
        "> - [x]\n>\n>   b", "- [x]\n- [ ]\n\n  b", "- [x]  \n\n  b", "- [ ]\n\n  - b", "- [x]\n\n  b\n\n  c",
    ]),
    "alerts": (["gfm", "commonmark+alerts"], [
        "> [!NOTE]\n> a", "> [!note]\n> a", "> [!TIP]\n> a", "> [!IMPORTANT]\n> a",
        "> [!WARNING]\n> a", "> [!CAUTION]\n> a", "> [!NOTE] x\n> a", "> [!NOTE]\n>\n> a",
        "> [!FOO]\n> a", ">[!NOTE]\n>a", "> [!NOTE]", "> [!NOTE]\na", "> [!NOTE]  \n> a", "> a\n> [!NOTE]",
        "- > [!NOTE]\n  > a", "> [! NOTE]\n> a", "> [!NOTE]\n> - a",
    ]),
    "wikilinks": (["commonmark+wikilinks_title_after_pipe", "commonmark+wikilinks_title_before_pipe",
                   "gfm+wikilinks_title_after_pipe"], [
        "[[a]]", "[[a|b]]", "[[a b]]", "[[a|]]", "[[|b]]", "[[a]]b", "[[a\\|b]]", "[[*a*]]", "[[a]]{.c}",
        "[[a|b|c]]", "[[ a ]]", "[[a\nb]]", "[[a]", "[[]]", "[[a]]]", "[[[a]]]", "![[a]]", "[[a](u)]]",
        "[[a|*b*]]", "[[http://x.y|z]]", "[[a#b]]", "[[a&amp;b]]", "[[ a | b ]]", "[[a\\]b]]", "[[a||b]]",
        "[[a|b]]{.c}", "[[a`b`]]", "[[a\\|b|c]]",
    ]),
    "fancy-lists": (["commonmark+fancy_lists", "commonmark_x"], [
        "i. a", "ii. a\niii. b", "a) a", "A. a", "(a) a", "#. a", "I. a\nII. b", "v. a", "c. a\nd. b",
        "1) a\n2. b", "a. a\nb. b", "B. Russell", "(1) a", "1.a", "i. a\nj. b", "x. a", "C. a\nD. b",
        "(i) a\n(ii) b", "#. a\n#. b", "IV. a", "a. a\n\nb. b", "10. a", "a.\tb", "Z. a", "ab. a",
        "p\na. b", "p\ni. b", "1. a\na. b",
    ]),
    "auto-identifiers": (["gfm", "commonmark+auto_identifiers", "commonmark_x", "gfm+ascii_identifiers"], [
        "# Hello World", "# Héllo", "# a_b-c.d", "# 1 a", "# *em* text", "# [l](u)", "# a  b", "# --",
        "# `x`", "# Ünïcödé", "# a\n# a\n# a-1", "# :smile: x", "# a!b?c", "# 中文", "#", "# a & b",
        "# A-B", "# a b", "# <b>x</b>", "# a $x$", "# a\n\nb\n-", "# α β", "# x\n# X",
    ]),
    "attributes-inline": (["commonmark_x", "commonmark+attributes"], [
        "`c`{.a}", "[s]{#i .c k=v}", "*a*{.c}", "![i](u){width=1}", "[s]{.a .b}", "[s]{k=\"v w\"}",
        "[s]{k='v'}", "[s]{#i#j}", "`r`{=html}", "[s]{ .a }", "[s]{.a\n.b}", "[s]{-}", "[s]{.}",
        "[s]{#}", "[s]{k=}", "[s]{k}", "[s]{%}", "[s]{.a=b}", "a{.c}", "[l](u){.c}", "[s]{.a}{.b}",
        "[s]{k=\"a\\\"b\"}", "[s]{#i .c #j}", "[s]{.é}", "[s]{data-x=1}", "[s] {.c}", "**a**{.c}",
        "[s]{k=v k=w}", "[s]{.c k=v .d}", "`c` {.a}",
    ]),
}


def extension_forms():
    for name, (formats, forms) in EXTENSION_FORMS.items():
        for form in forms:
            for fmt in formats:
                yield {"from": fmt, "input": form + "\n", "module": name}


# The continuation rule of each container: each kind of second line after
# the first line of the container.
CONTAINERS = ["> a", "- a", "1. a", "x[^1]\n\n[^1]: a", "t\n: a", "::: d\na", "> [!NOTE]\n> a", "-\n  a"]
SECOND_LINES = [
    "b", " b", "  b", "   b", "    b", "     b", "\tb", "- b", "> b", "# b", "```\nb\n```", "\n b", "\n  b",
    "\n   b", "\n    b", "\n     b", "\n\tb", "\n\n  b", "***", "  - b", "    - b", ":::", "\n:::",
]


def continuation():
    for container in CONTAINERS:
        for line in SECOND_LINES:
            for fmt in ("commonmark_x", "gfm+alerts"):
                yield {"from": fmt, "input": f"{container}\n{line}\n"}


# The tokenizer of Tokens.hs: NUL, line endings, NFC, word characters with
# marks, tabs.
TOKEN_FORMS = [
    "a\0b", "*a\0*", "a\r\nb", "a\rb", "a\r\n\r\nb", "é*x*", "*é*", "á_b_", "\tcode",
    " \tcode", "-\tb", ">\t\tb", "a *b*", "_à_", "**́a**", "a_́_b", "1٣_x_",
    "x​*y*", "*a*́", "Å", "ﬁ*x*", "a b", "a\u0085b", " # h", "# h", "- a",
    "a\fb", "\f# h", "  \t- a", "1.\tx", "- a\n\t- b", "`\0`", "<a\0>",
]


def tokenizer():
    for form in TOKEN_FORMS:
        for fmt in ("commonmark", "gfm"):
            yield {"from": fmt, "input": form + "\n"}


# Quotes after an unknown collapsed reference behind another opener: the
# inputs that the mutants of `close_bracket` (session 3 unit c) need.
def quote_after_pair():
    for form in ["[[a][]'b'", "[[a][]'b' c", "x[[a][]'b'", "[x[a][]'b'", "![[a][]'b'", "[[r][]'b'", "[[a][]\"b\""]:
        for fmt in ("commonmark+smart", "commonmark_x", "gfm+smart"):
            yield {"from": fmt, "input": form + "\n\n[r]: /r\n"}


# Task items (review round 7: Opus R7-01, R7-04; Codex R7-03): an item
# after a paragraph line, white space after the box, the lines after the
# item, inside a block quote and inside an ordinary list item.
TASK_PREFIXES = ["", "x\n"]
TASK_ITEMS = ["- [ ]", "- [x]", "* [X]", "- [ ] b", "- [ ]  ", "- [ ] \u00a0", "1. [ ]"]
TASK_TAILS = ["", "\n\n- [ ] b", "\n\n\n- b", "\n- [ ]", "\n\n  x", "\nb"]
TASK_WRAPPERS = ["", "> ", "- ", "> - "]


def wrap(text, wrapper):
    """Puts the text in the containers of the wrapper: the first line gets the
    wrapper, each other line its continuation (a list marker becomes spaces)."""
    lines = text.split("\n")
    more = wrapper.replace("- ", "  ")
    return "\n".join([wrapper + lines[0]] + [(more + line).rstrip(" ") for line in lines[1:]])


def tasks():
    for prefix in TASK_PREFIXES:
        for item in TASK_ITEMS:
            for tail in TASK_TAILS:
                for wrapper in TASK_WRAPPERS:
                    for fmt in ("gfm", "commonmark_x"):
                        yield {"from": fmt, "input": wrap(prefix + item + tail, wrapper) + "\n"}


# The bottom of processBs (review round 7: Codex R7-06, R7-07; Opus R7-03,
# R7-06): a pair that makes nothing, with or without a chunk between the
# bottom and its opener, before `[]`.
BOTTOM_PREFIXES = ["", "x", "x ", "[b]x", "[l](u)x", "[x", "\\[", "![b]", "*e*", "`c`", "'", "[r]"]
BOTTOM_CORES = ["[a][]'b'", "[[][](u)", "[a][]{.c}", "![a][](u)", "[a][][x](u)", "[a][]", "[a][]'b' [r][]'c'"]


def bracket_bottom():
    for prefix in BOTTOM_PREFIXES:
        for core in BOTTOM_CORES:
            for fmt in ("commonmark+smart", "commonmark_x", "gfm+smart"):
                yield {"from": fmt, "input": f"{prefix}{core}\n\n[r]: /r\n"}


# Letters of FancyList.hs beyond ASCII (session 4): pLowerAlpha and
# pUpperAlpha take one letter of the Haskell classes isLower and isUpper.
FANCY_LETTERS = list("éαＢǅªⓐĸɕÄΩßıİǈʰ中٣ⅳ") + ["e\u0301"]
FANCY_DELIMS = [". x", ") x", "(c) x", ".  x"]
FANCY_CONTEXTS = ["", "x\n", "a. y\n", "- y\n"]


def fancy_letters():
    for letter in FANCY_LETTERS:
        for delim in FANCY_DELIMS:
            marker = delim.replace("c", letter) if delim.startswith("(") else letter + delim
            for context in FANCY_CONTEXTS:
                for fmt in ("commonmark_x", "commonmark+fancy_lists"):
                    yield {"from": fmt, "input": f"{context}{marker}\n"}


# Alert names of Alerts.hs (session 4): the word equals the name after the
# Unicode upper-case mapping of Data.Text.
ALERT_NAMES = ["NOTE", "note", "tıp", "cautıon", "ımportant", "WARNİNG", "Tip", "NOTES", "NO TE", "ＮＯＴＥ", "caution"]


def alert_names():
    for name in ALERT_NAMES:
        for tail in ["\n> x", "", "\nx", " \n> x"]:
            for fmt in ("gfm+alerts", "commonmark_x+alerts"):
                yield {"from": fmt, "input": f"> [!{name}]{tail}\n"}


# Metadata findings of review round 7: decoded text and NFC (Codex R7-01),
# later YAML documents (Codex R7-02, Opus R7-02), a block that ends on the
# open last line of an inline value (Codex R7-04).
META_R7_VALUES = [
    '"[e\\u0301]\\n\\n[\u00e9]: u"', '"[\u00e9]\\n\\n[e\\u0301]: u"', '"![e\\u0301]\\n\\n[\u00e9]: u"',
    '"[e\\u0301]\\n\\n[\u00e9]: u\\n"', '"e\\u0301"', '"<!--\\n-->"', '"<!-- -->"', '"<!--\\n-->x"',
    '"<!--\\n-->\\nx"', '"x\\n\\n<!--\\n-->"', '"<script>\\nx\\n</script>"', '"<div>\\nx"',
    '"<pre>\\n</pre>"', '"<?\\n?>"', '"```\\nx\\n```"', '"> ```\\n> x\\n> ```"', '"- ```\\n  x\\n  ```"',
    '"::: d\\nx\\n:::"', '"<!--\\n--> "', '"> <!--\\n> -->"', '"- <!--\\n  -->"', '"<![CDATA[\\nx]]>"',
]
META_R7_STREAMS = [
    "a: 1\n--- x\n---\n", "a: 1\n--- # c\n---\nbody\n", "a: 1\n--- !x\n---\nb\n", "--- x\n---\nb\n",
    "--- {a: 1}\n---\nb\n", "a: 1\n--- [1]\n---\nb\n", "a: 1\n--- \nb: 2\n---\nb\n", "- 1\n--- \nb: 2\n---\nb\n",
    "a: 1\n--- x\n...\nb\n", "a: 1\n--- {b: 2}\n---\n", "a: 1\n--- |\n  x\n---\n", "a: 1\n--- &x y\n---\n",
    "a: 1\n---\n", "# c\n--- x\n---\nb\n", "~\n--- {a: 1}\n---\nb\n", "a: 1\n--- [\n---\nb\n",
    "a: 1\n--- x\n--- y\n---\n", "a: &r 1\n--- *r\n---\n", "a: 1\n...\n--- x\n---\n",
]


def metadata_r7():
    for value in META_R7_VALUES:
        for fmt in META_FORMATS:
            yield {"from": fmt, "input": f"---\na: {value}\n---\n"}
    for stream in META_R7_STREAMS:
        for fmt in META_FORMATS:
            yield {"from": fmt, "input": f"---\n{stream}"}


# A reference definition as the first block of an item (the differential
# run of session 4): the paragraph of definitions stays a child upstream,
# so the item continues after a blank line.
REF_ITEM_HEADS = ["- [r]: /u", "1. [r]: /u", "- [r]: /u\n  [s]: /v", "* [^n]: <http://a.b>", "- [r]:\n  /u"]
REF_ITEM_TAILS = ["\n\n  x", "\n  x", "\n\n\n  x", "\n\n- b", "\n\nx", "\n\n  [t]: /w\n\n  x", "\n\n  - c"]


def ref_items():
    for head in REF_ITEM_HEADS:
        for tail in REF_ITEM_TAILS:
            for wrapper in ("", "> "):
                for fmt in ("commonmark", "commonmark_x"):
                    yield {"from": fmt, "input": wrap(head + tail, wrapper) + "\n\n[r]\n"}


MATRICES = {
    "strip-comments": strip_comments,
    "lists": lists,
    "single-quotes": single_quotes,
    "metadata-values": metadata_values,
    "yaml-findings": yaml_findings,
    "link-paths": link_paths,
    "link-scanners": link_scanners,
    "brackets": brackets,
    "extension-forms": extension_forms,
    "continuation": continuation,
    "tokenizer": tokenizer,
    "quote-after-pair": quote_after_pair,
    "tasks": tasks,
    "bracket-bottom": bracket_bottom,
    "fancy-letters": fancy_letters,
    "alert-names": alert_names,
    "metadata-r7": metadata_r7,
    "ref-items": ref_items,
}


def run(program, entry):
    args = [program, "-f", entry["from"], "-t", "json"]
    if entry.get("strip_comments"):
        args.insert(1, "--strip-comments")
    done = subprocess.run(args, input=entry["input"].encode(), capture_output=True, timeout=60)
    return done.returncode, done.stdout.decode().strip(), done.stderr.decode().rstrip("\n")


# Known differences (rows of docs/ISSUES.md): an input that holds the text
# differs from the oracle. The crate test and --compare require that it
# still differs, so that a correction removes the row.
KNOWN = {}


def entries(names):
    for name in names:
        for entry in MATRICES[name]():
            known = [issue for text, issue in KNOWN.items() if text in entry["input"]]
            yield {"matrix": name, **entry, **({"known": known[0]} if known else {})}


def record(names):
    """Records the named matrices and keeps the stored entries of the others.
    The error text of a refused input is kept too."""
    oracle = os.environ["RANDOC_ORACLE_PANDOC"]
    kept = []
    if OUT.exists() and set(names) != set(MATRICES):
        kept = [e for e in json.loads(OUT.read_text(encoding="utf-8")) if e["matrix"] not in names]
    todo = list(entries(names))
    with ThreadPoolExecutor(2) as pool:
        results = list(pool.map(lambda e: run(oracle, e), todo))
    for entry, (code, out, err) in zip(todo, results):
        entry["exit"] = code
        entry["stdout"] = out if code == 0 else ""
        if code != 0:
            entry["stderr"] = err
    stored = [e for name in MATRICES for e in kept + todo if e["matrix"] == name]
    OUT.write_text(json.dumps(stored, indent=0, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"{len(todo)} inputs recorded, {len(stored)} stored")


def compare(names=None):
    randoc = str(ROOT / "target/debug/randoc")
    stored = json.loads(OUT.read_text(encoding="utf-8"))
    if names:
        stored = [e for e in stored if e["matrix"] in names]
    different = 0
    known = 0
    by_matrix = {}
    for entry in stored:
        if entry.get("strip_comments"):
            continue  # the program does not accept the option; the crate test checks these
        code, out, err = run(randoc, entry)
        differs = (code != entry["exit"] or (code == 0 and out != entry["stdout"])
                   or ("stderr" in entry and err != entry["stderr"]))
        if "known" in entry:
            known += 1
            if not differs:
                print(f"{entry['matrix']} ({entry['from']}): {entry['input']!r} equals the oracle now: "
                      f"close {entry['known']}")
                different += 1
            continue
        if differs:
            different += 1
            by_matrix[entry["matrix"]] = by_matrix.get(entry["matrix"], 0) + 1
            if different <= LIMIT:
                print(f"{entry['matrix']} ({entry['from']}): {entry['input']!r}\n"
                      f"  oracle {entry['exit']} {entry['stdout'][:300]}\n  randoc {code} {out[:300]}")
    print(f"{len(stored)} inputs, {different} different {by_matrix}, {known} known differences")
    return 1 if different else 0


if __name__ == "__main__":
    if sys.argv[1:2] == ["--compare"]:
        sys.exit(compare(sys.argv[2].split(",") if len(sys.argv) > 2 else None))
    if sys.argv[1:2] == ["--only"]:
        record(sys.argv[2].split(","))
    else:
        record(list(MATRICES))
