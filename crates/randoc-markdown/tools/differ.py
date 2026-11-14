#!/usr/bin/env python3
"""Differential runs of the Markdown reader against the oracle.

Usage:
  differ.py matrix raw|blocktag|lazy|closer|abbrev OUT_DIR
  differ.py random COUNT SEED OUT_DIR [wide]
  differ.py rerun OUT_DIR
  differ.py toml OUT_FILE MATRIX...

Each command writes OUT_DIR/cases.json, runs the oracle once for all cases
(`pandoc lua oracle.lua`), runs `target/debug/randoc -t json` for each case,
and prints a signature table: the cases grouped by the first node where the
two trees differ. OUT_DIR/diffs.json holds each differing case.
Environment: RANDOC_ORACLE_PANDOC, RANDOC (default target/debug/randoc), and
RANDOC_ARGS (more arguments for Randoc, in shell syntax).
"""
import concurrent.futures
import json
import os
import random
import shlex
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
RANDOC = os.environ.get("RANDOC", os.path.join(ROOT, "target", "debug", "randoc"))

EXT_SETS = [
    "markdown",
    "markdown-markdown_in_html_blocks",
    "markdown-native_divs",
    "markdown-raw_html",
    "markdown-markdown_in_html_blocks-native_divs",
    "markdown_phpextra",
    "markdown_strict",
    "markdown_github",
    "markdown_mmd",
]


def raw_matrix():
    forms = [
        "<?php x ?>",
        "<?xml version=\"1.0\"?>",
        "<? a b ?>",
        "<?x\ny ?>",
        "<?x",
        "<!DOCTYPE html>",
        "<!doctype html>",
        "<!ELEMENT a b>",
        "<!X\ny>",
        "<!x>",
        "<!>",
        "<![CDATA[ a < b ]]>",
        "<![CDATA[a\n\nb]]>",
        "<![CDATA[x",
        "<!-- c -->",
        "<!---->",
        "<!--->",
        "<!-->",
        "<!-- a -- b -->",
        "<!--a\n\nb-->",
        "<!-- x",
        "<?",
        "<!",
    ]
    places = {
        "alone": "{f}\n",
        "after": "x {f} y\n",
        "line-end": "x {f}\n",
        "then-text": "{f} y\n",
        "in-para": "a\n{f}\nb\n",
        "item": "- {f}\n",
        "item-text": "- a {f} b\n",
        "quote": "> {f}\n",
        "emph": "*a {f} b*\n",
        "link": "[a {f} b](u)\n",
        "code": "`a {f} b`\n",
        "indent1": " {f}\n",
        "heading": "# a {f} b\n",
        "table": "| a | b |\n|---|---|\n| {f} | c |\n",
        "def": "Term\n\n:   {f}\n",
        "div": "::: d\n{f}\n:::\n",
        "html-div": "<div>\n{f}\n</div>\n",
    }
    cases = []
    for fi, form in enumerate(forms):
        for place, tpl in places.items():
            for ext in EXT_SETS:
                cases.append(
                    {
                        "id": f"raw-{fi}-{place}-{ext}",
                        "from": ext,
                        "input": tpl.replace("{f}", form),
                    }
                )
    return cases


BLOCK_NAMES = [
    "address", "article", "aside", "blockquote", "center", "dd", "details",
    "dialog", "div", "dl", "dt", "fieldset", "figure", "footer", "form", "h1",
    "h2", "header", "hr", "li", "main", "nav", "ol", "p", "pre", "script",
    "section", "style", "table", "td", "textarea", "tr", "ul", "video",
    "audio", "button", "del", "ins", "svg", "area", "col", "embed", "source",
    "track", "meta", "iframe", "noscript", "canvas", "map", "object",
    "summary", "menu", "isindex", "frameset", "colgroup",
]
INLINE_NAMES = ["span", "a", "em", "b", "img", "input", "label", "x-y"]


def blocktag_matrix():
    tags = []
    for name in BLOCK_NAMES + INLINE_NAMES:
        tags += [
            (f"{name}-pair", f"<{name}>y</{name}>"),
            (f"{name}-open", f"<{name}>"),
            (f"{name}-close", f"</{name}>"),
        ]
    tags += [
        ("div-attr", '<div class="c">y</div>'),
        ("div-md", '<div markdown="1">*y*</div>'),
        ("p-self", "<p/>"),
        ("hr-self", "<hr/>"),
        ("div-unclosed-tag", "<div class=x"),
    ]
    places = {
        "end": "x {t}\n",
        "mid": "x {t} z\n",
        "start": "{t} z\n",
        "second-line": "a\nx {t} z\n",
        "next-line": "x {t}\nz\n",
        "code": "`x {t}` z\n",
        "link": "[x {t}](u) z\n",
        "emph": "*x {t}* z\n",
        "quote": "> x {t} z\n",
        "item": "- x {t} z\n",
        "two": "x {t} z {t} w\n",
        "heading": "# x {t} z\n",
        "autolink": "<http://a> {t}\n",
        "span-open": "<span>x {t}</span>\n",
        "math": "$x {t}$\n",
    }
    exts = [
        "markdown",
        "markdown-markdown_in_html_blocks",
        "markdown-native_divs",
        "markdown-raw_html",
        "markdown-markdown_in_html_blocks+markdown_attribute",
        "markdown-markdown_in_html_blocks-native_divs",
        "markdown_strict",
        "markdown_phpextra",
    ]
    cases = []
    for tid, tag in tags:
        for place, tpl in places.items():
            for ext in exts:
                cases.append(
                    {
                        "id": f"bt-{tid}-{place}-{ext}",
                        "from": ext,
                        "input": tpl.replace("{t}", tag),
                    }
                )
    return cases


def lazy_matrix():
    """Lines after a block quote line or a term: lazy lines and bare markers."""
    lines = [
        "b", "# h", "## h ##", "- b", "* b", "1. b", "1) b", "a. b", "(@) b",
        "#. b", ": d", ":", "~ d", "~", ":   ", "> b", ">", "```", "~~~",
        "---", "***", "<div>", "<div>b</div>", "<!-- c -->", "| b",
        "    b", "  : d", "::: d", "[x]: u", "[^x]: n", "+ b", "B) b",
        "```\nq\n```", "~~~\nq\n~~~", "``` {.c}\nq\n```", "```{=html}\n<b>q</b>\n```",
    ]
    heads = {
        "quote": "> a\n{l}\nz\n",
        "empty-quote": ">\n{l}\nz\n",
        "quote-blank": "> a\n>\n{l}\nz\n",
        "nested": "> > a\n{l}\nz\n",
        "item-quote": "- > a\n{l}\nz\n",
        "term": "a\n{l}\n    z\n",
        "term-loose": "a\n\n{l}\n    z\n",
        "para": "a\nb\n{l}\n",
        "item": "- a\n{l}\n  z\n",
    }
    exts = [
        "markdown", "markdown_github", "markdown_phpextra", "markdown_mmd",
        "markdown_strict", "markdown+lists_without_preceding_blankline",
        "markdown-blank_before_header", "markdown-blank_before_blockquote",
    ]
    cases = []
    for li, line in enumerate(lines):
        for head, tpl in heads.items():
            for ext in exts:
                cases.append(
                    {
                        "id": f"lazy-{li}-{head}-{ext}",
                        "from": ext,
                        "input": tpl.replace("{l}", line),
                    }
                )
    return cases


def closer_matrix():
    """Closing tags in tokens of the content, and elements next to each other."""
    tokens = {
        "fence": "```\n{c}\n```",
        "tilde": "~~~\n{c}\n~~~",
        "open-fence": "```\n{c}",
        "item-fence": "- ```\n  {c}\n  ```",
        "quote-fence": "> ```\n> {c}\n> ```",
        "code": "a `{c}` b",
        "code2": "a ``x {c} ` y`` b",
        "open-code": "a `{c}",
        "math": "${c}$",
        "space-math": "$ {c}$",
        "escape": "a \\{c} b",
        "comment": "<!-- {c} -->",
        "comment-mid": "a <!-- {c} --> b",
        "comment-lines": "<!--\n{c}\n-->",
        "open-comment": "<!-- {c}",
        "attr": "<b title=\"{c}\">t</b>",
        "indented": "    {c}",
        "blank-indented": "\n    {c}\n",
        "text": "a {c} b",
    }
    elements = ["div", "section", "p", "span"]
    cases = []
    for el in elements:
        for tid, tok in tokens.items():
            body = tok.replace("{c}", f"</{el}>")
            forms = {
                "lines": f"<{el}>\n{body}\n</{el}>\n",
                "inline": f"<{el}>{body}</{el}>\n",
                "after": f"<{el}>\n{body}\n</{el}>\nz\n",
            }
            for fid, text in forms.items():
                for ext in ["markdown", "markdown-markdown_in_html_blocks", "markdown_phpextra"]:
                    cases.append({"id": f"cl-{el}-{tid}-{fid}-{ext}", "from": ext, "input": text})
    names = ["div", "p", "section", "span", "em", "blockquote", "ul", "pre"]
    shapes = {
        "empty": "<{a}></{a}><{b}>x</{b}>\n",
        "text": "<{a}>w</{a}><{b}>x</{b}>\n",
        "space": "<{a}></{a}> <{b}>x</{b}>\n",
        "after": "<{a}></{a}><{b}>x</{b}>y\n",
        "lines": "<{a}></{a}><{b}>\nx\n</{b}>\n",
    }
    for a in names:
        for b in names:
            for sid, shape in shapes.items():
                cases.append(
                    {"id": f"adj-{a}-{b}-{sid}", "from": "markdown", "input": shape.format(a=a, b=b)}
                )
    return cases


def abbrev_matrix():
    """Words of the default abbreviation list and other words before a period."""
    words = ["Dr", "Mr", "e.g", "Fig", "St", "Zq", "Word"]
    forms = {
        "period": "See {w}. Next", "escaped": "See {w}\\. Next", "two-spaces": "See {w}.  Next",
        "line-end": "See {w}.\nNext", "emph": "See *{w}.* Next", "bracket": "See [{w}.] Next",
        "strong": "See **{w}.** Next", "comma": "See {w}., Next",
    }
    cases = []
    for w in words:
        for fid, form in forms.items():
            for ext in ["markdown", "markdown_strict", "markdown-smart", "markdown_phpextra"]:
                cases.append({"id": f"ab-{w}-{fid}-{ext}", "from": ext, "input": form.format(w=w) + "\n"})
    return cases


WORDS = ["alpha", "beta", "x", "Gamma", "d", "east", "foo", "bar", "q", "zeta"]


def random_doc(rng, wide):
    def w():
        return rng.choice(WORDS)

    def inline():
        forms = [
            lambda: w(),
            lambda: f"*{w()}*",
            lambda: f"**{w()} {w()}**",
            lambda: f"_{w()}_",
            lambda: f"`{w()}`",
            lambda: f"[{w()}](http://{w()}.org)",
            lambda: f"[{w()}][{w()}]",
            lambda: f"![{w()}]({w()}.png)",
            lambda: f"<http://{w()}.org>",
            lambda: f"${w()}^2$",
            lambda: f"$${w()}$$",
            lambda: f"[@{w()}]",
            lambda: f"@{w()}",
            lambda: f"[@{w()}, p. 3]",
            lambda: f"[^{rng.randint(1, 3)}]",
            lambda: f"^[{w()} {w()}]",
            lambda: f"[{w()}]{{.c}}",
            lambda: f"<span>{w()}</span>",
            lambda: f"<em>{w()}</em>",
            lambda: f"<!-- {w()} -->",
            lambda: f'"{w()}"',
            lambda: f"'{w()}'",
            lambda: f"{w()}--{w()}",
            lambda: f"{w()}...",
            lambda: f"~~{w()}~~",
            lambda: f"H~2~O",
            lambda: f"{w()}^2^",
            lambda: f"`{w()}`{{.c}}",
            lambda: f"\\*{w()}",
            lambda: f"{w()}\\",
            lambda: f"[{w()}]",
        ]
        if wide:
            forms += [
                lambda: f"`{w()}`{{=html}}",
                lambda: f"\\emph{{{w()}}}",
                lambda: f"\\cite{{{w()}}} {w()}",
                lambda: f"&amp; &copy; &#x41;",
                lambda: f"{w()}\\ {w()}",
                lambda: f"{w()}_{w()}_{w()}",
                lambda: f"{w()}*{w()}*{w()}",
                lambda: f"***{w()}*** **{w()} *{w()}* {w()}**",
                lambda: f"[{w()}](http://{w()}.org \"{w()}\")",
                lambda: f"[{w()}](<u {w()}>)",
                lambda: f"<{w()}@{w()}.org>",
                lambda: f"[{w()}]({w()}){{#i .c k=v}}",
                lambda: f"[Heading {w()}]",
                lambda: f"@{w()} [p. 4]",
                lambda: f"[-@{w()}; @{w()}]",
                lambda: f"{w()}---{w()} 'q'",
                lambda: f"<b>{w()}</b>",
                lambda: f"<abbr title=\"t\">{w()}</abbr>",
                lambda: f"${w()} < 2$",
                lambda: f"\\${w()}",
            ]
        return " ".join(rng.choice(forms)() for _ in range(rng.randint(1, 5)))

    def block(depth):
        forms = [
            lambda: inline(),
            lambda: f"{inline()}\n{inline()}",
            lambda: f"# {inline()}",
            lambda: f"## {inline()} {{#{w()}}}",
            lambda: f"{inline()}\n{'=' * rng.randint(1, 5)}",
            lambda: f"{inline()}\n---",
            lambda: "\n".join(f"- {inline()}" for _ in range(rng.randint(1, 3))),
            lambda: "\n".join(f"{i + 1}. {inline()}" for i in range(rng.randint(1, 3))),
            lambda: "\n".join(f"#. {inline()}" for _ in range(rng.randint(1, 2))),
            lambda: "\n".join(f"(@{w()}) {inline()}" for _ in range(rng.randint(1, 2))),
            lambda: f"a) {inline()}\nb) {inline()}",
            lambda: f"i. {inline()}\nii. {inline()}",
            lambda: f"- [ ] {inline()}\n- [x] {inline()}",
            lambda: f"* {inline()}\n\n    {inline()}",
            lambda: f"> {inline()}\n> {inline()}",
            lambda: f"> {inline()}\n{inline()}",
            lambda: f"    {w()} {w()}\n    {w()}",
            lambda: f"```\n{w()}\n```",
            lambda: f"~~~ {{.haskell #id}}\n{w()} = 1\n~~~",
            lambda: f"```python\n{w()}\n\n{w()}\n```",
            lambda: f"| {w()} | {w()} |\n|---|:--:|\n| {inline()} | {w()} |",
            lambda: f"  {w()}   {w()}\n------ ------\n  {w()}   {w()}\n\n",
            lambda: f"+-----+-----+\n| {w()[:3]:3} | b   |\n+=====+=====+\n| c   | d   |\n+-----+-----+",
            lambda: f"-----------\n{w()} {w()}\n\n{w()}\n-----------",
            lambda: f"{w()}\n:   {inline()}",
            lambda: f"{w()}\n\n:   {inline()}\n\n    {w()}",
            lambda: f"{w()}\n~ {inline()}",
            lambda: f"[^{rng.randint(1, 3)}]: {inline()}",
            lambda: f"[^{rng.randint(1, 3)}]:\n    {inline()}\n\n    {w()}",
            lambda: f"[{w()}]: http://{w()}.org \"{w()}\"",
            lambda: f"<div>\n{inline()}\n</div>",
            lambda: f"<div class=\"{w()}\">\n\n{inline()}\n\n</div>",
            lambda: f"<table><tr><td>{w()}</td></tr></table>",
            lambda: f"::: {w()}\n{inline()}\n:::",
            lambda: f"::: {{.{w()} #{w()}}}\n{block(depth + 1) if depth < 2 else w()}\n:::",
            lambda: f"| {inline()}\n|   {w()}",
            lambda: f"$${w()} = 1$$",
            lambda: f"\\begin{{equation}}\n{w()}\n\\end{{equation}}",
            lambda: f"```{{=html}}\n<b>{w()}</b>\n```",
            lambda: "* * *",
            lambda: f"<!-- {w()} -->",
            lambda: f"{inline()}  \n{inline()}",
            lambda: f"- {inline()}\n\n  > {inline()}",
            lambda: f"1. {w()}\n   - {inline()}\n   - {w()}",
            lambda: f"![{w()}]({w()}.png){{width=50%}}",
            lambda: f"Table: {w()}\n\n| a |\n|---|\n| b |",
        ]
        if wide:
            forms += [
                lambda: f"# Heading {w()}\n\n{inline()}",
                lambda: f"{w()}\n{'-' * rng.randint(3, 6)}\n\n{inline()}",
                lambda: f"A. {inline()}\nB. {inline()}",
                lambda: f"I. {inline()}\nII. {inline()}",
                lambda: f"3) {inline()}\n4) {inline()}",
                lambda: f"(a) {inline()}\n(b) {inline()}",
                lambda: f"- {w()}\n\n      {w()} code\n\n- {inline()}",
                lambda: f"- {w()}\n    - {inline()}\n        - {w()}",
                lambda: f"> > {inline()}\n> {w()}",
                lambda: f"> - {inline()}\n> - {w()}",
                lambda: f"> ```\n> {w()}\n> ```",
                lambda: "___",
                lambda: "- - -",
                lambda: f"``` {{.python .numberLines startFrom=3}}\n{w()}\n```",
                lambda: f"~~~~\n~~~\n{w()}\n~~~~",
                lambda: f"{w()}  {w()}\n----  ----\n{w()}  {w()}\n\n: {w()}",
                lambda: f"-------  -----\n  {w()}   {w()}\n-------  -----",
                lambda: f"+---+---+\n| a | b |\n+:==+==:+\n| {w()[:1]} | c |\n+---+---+",
                lambda: f"| a | b |\n|--:|:--|\n| {w()} | {inline()} |\n\n: {w()}",
                lambda: f"{w()}\n\n:   {inline()}\n\n:   {inline()}",
                lambda: f"{w()}\n  ~ {inline()}\n  ~ {w()}",
                lambda: f"(@a) {inline()}\n\nAs (@a) shows.",
                lambda: f"| {w()}\n| {w()}\n\n{inline()}",
                lambda: f"::: {w()}\n::: {w()}\n{inline()}\n:::\n:::",
                lambda: f"<div class=\"{w()}\" markdown=\"1\">\n*{w()}*\n</div>",
                lambda: f"<p>{w()}</p>\n\n{inline()}",
                lambda: f"\\begin{{itemize}}\n\\item {w()}\n\\end{{itemize}}",
                lambda: f"\\newcommand{{\\{w()}}}{{x}}",
                lambda: f"[{w()}]: http://{w()}.org {{.c}}\n\n[{w()}]",
                lambda: f"[^n]: {inline()}\n\nA note[^n] and again.",
                lambda: f"{inline()}\n\n[{w()}]: <http://{w()}.org> '{w()}'",
                lambda: f"![{w()}]({w()}.png \"t\")",
                lambda: f"![{w()}]({w()}.png)\\\n{w()}",
                lambda: f"    {w()}\n\n    {w()}",
                lambda: f"\t{w()}",
                lambda: f"- [ ] {w()}\n    - [x] {inline()}",
                lambda: f"`````\n```\n{w()}\n`````",
                lambda: f"{w()}\n#. {inline()}",
            ]
        return rng.choice(forms)()

    parts = []
    if rng.random() < 0.15:
        parts.append(f"---\ntitle: {w()} *{w()}*\nauthor: [{w()}, {w()}]\n---")
    if rng.random() < 0.05:
        parts.append(f"% {w()}\n% {w()}\n% 2020")
    for _ in range(rng.randint(1, 4)):
        parts.append(block(0))
    sep = rng.choice(["\n\n", "\n\n", "\n"])
    return sep.join(parts) + "\n"


def span_name_matrix():
    """Codex R3-08: tag names that start with `span`."""
    names = ["span", "span-box", "spanx", "span.x", "span_y", "SPAN", "Span-a", "span:a"]
    cases = []
    for ext in ["markdown", "markdown-native_spans", "markdown_phpextra"]:
        for a in names:
            for b in names:
                cases.append({"id": f"sn-{a}-{b}-{ext}", "from": ext, "input": f"<{a}>x</{b}>\n"})
            cases.append({"id": f"sn-{a}-attr-{ext}", "from": ext, "input": f'<{a} class="c">x</{a}>\n'})
            cases.append({"id": f"sn-{a}-nest-{ext}", "from": ext, "input": f"a <{a}>x <span>y</span></{a}> b\n"})
    return cases


ATTR_FORMS = {
    "bare": "a", "sq-empty": "a=''", "dq-empty": 'a=""', "sq": "a='b'", "dq": 'a="b"',
    "unq": "a=b", "upper": 'A="b"', "sq-amp": "a='&amp;'", "dq-lt": 'a="&lt;"',
    "sq-dq": "a='x\"y'", "dq-sq": 'a="x\'y"', "spaced": 'a  =  "b"', "two": "a='b' c",
    "unq-amp": "a=&quot;", "num": "a='&#65;'", "gt": "a='>'", "tab": "a='x\ty'",
}


def raw_tag_matrix():
    """Codex R3-09: the text of a raw start tag with each attribute form."""
    cases = []
    for name in ["p", "div", "x-y"]:
        for aid, attr in ATTR_FORMS.items():
            forms = {
                "empty": f"<{name} {attr}></{name}>\n",
                "text": f"<{name} {attr}>x</{name}>\n",
                "lines": f"<{name} {attr}>\nx\n</{name}>\n",
            }
            for fid, text in forms.items():
                for ext in ["markdown", "markdown-markdown_in_html_blocks", "markdown-native_divs", "markdown_phpextra"]:
                    cases.append({"id": f"rt-{name}-{aid}-{fid}-{ext}", "from": ext, "input": text})
    return cases


def caption_matrix():
    """Codex R3-10: line breaks and spaces in table captions."""
    tables = {
        "pipe": "x|y\n-|-\n1|2",
        "simple": "  x   y\n ---  ---\n  1   2",
        "grid": "+---+---+\n| x | y |\n+===+===+\n| 1 | 2 |\n+---+---+",
        "multi": "-------\n x   y\n--- ---\n 1   2\n-------",
    }
    captions = {
        "two-spaces": "a  \nb", "three-spaces": "a   \nb", "one-space": "a \nb", "backslash": "a\\\nb",
        "end-spaces": "a b  ", "mid-spaces": "a  b", "tab": "a\t\nb", "plain": "a\nb",
        "lines": "a  \nb  \nc", "emph": "*a*  \nb",
    }
    cases = []
    for tid, table in tables.items():
        for cid, cap in captions.items():
            for prefix in [": ", "Table: ", "table: "]:
                pid = prefix.strip(": ") or "colon"
                cases.append({"id": f"cap-{tid}-{cid}-{pid}-before", "from": "markdown", "input": f"{prefix}{cap}\n\n{table}\n"})
                cases.append({"id": f"cap-{tid}-{cid}-{pid}-after", "from": "markdown", "input": f"{table}\n\n{prefix}{cap}\n"})
    return cases


def div_comment_matrix():
    """Codex R3-11: fence lines inside comments and processing instructions of a div."""
    raws = {
        "comment-open": "<!--\n{f}\n-->", "comment-one": "<!-- {f} -->", "comment-tail": "<!-- a\n{f} -->",
        "pi": "<?x\n{f}\n?>", "comment-after-text": "t <!--\n{f}\n-->", "comment-indented": "  <!--\n{f}\n-->",
        "two-comments": "<!--\n{f}\n-->\n<!--\n{f}\n-->", "open-no-end": "<!--\n{f}",
    }
    fences = {"open": "::: b", "close": ":::", "long": "::::", "attr": "::: {.c}"}
    cases = []
    for rid, raw in raws.items():
        for fid, fence in fences.items():
            body = raw.replace("{f}", fence)
            forms = {"plain": f"::: a\n{body}\n:::\n", "after": f"::: a\n{body}\n:::\n\nz\n", "nested": f"::: a\n::: n\n{body}\n:::\n:::\n"}
            for form, text in forms.items():
                for ext in ["markdown", "markdown-raw_html"]:
                    cases.append({"id": f"dc-{rid}-{fid}-{form}-{ext}", "from": ext, "input": text})
    return cases


def meta_image_matrix():
    """Codex R3-14: images alone in a paragraph of a metadata value."""
    values = {
        "dq": '"![x](u)\\n"', "dq-no-nl": '"![x](u)"', "block": "|\n  ![x](u)\n", "block-two": "|\n  ![x](u)\n\n  ![y](v)\n",
        "attr": '"![x](u){.c}\\n"', "text": '"t ![x](u)\\n"', "list": '\n  - "![x](u)\\n"\n  - b', "map": '\n  k: "![x](u)\\n"',
        "empty-alt": '"![](u)\\n"', "two-para": '"a\\n\\n![x](u)\\n"',
    }
    cases = []
    for vid, value in values.items():
        for key in ["a", "title", "abstract"]:
            text = f"---\n{key}: {value}\n---\n"
            cases.append({"id": f"mi-{vid}-{key}", "from": "markdown", "input": text})
            cases.append({"id": f"mi-{vid}-{key}-body", "from": "markdown", "input": text + "\n![b](w)\n"})
    return cases


def html_attr_case_matrix():
    """Codex R3-15: the case of attribute names of native divs and spans."""
    attrs = ["ID=x", "Id=x", "iD=x", "CLASS=c", "Class='a b'", "ID=x CLASS=c", "DATA-X=1", "Style=s", "id=x ID=y", "class=a CLASS=b", "TITLE=t"]
    cases = []
    for attr in attrs:
        aid = attr.replace("=", "-").replace(" ", "_").replace("'", "")
        for ext in ["markdown", "markdown_phpextra"]:
            cases.append({"id": f"ac-div-{aid}-{ext}", "from": ext, "input": f"<div {attr}></div>\n"})
            cases.append({"id": f"ac-divx-{aid}-{ext}", "from": ext, "input": f"<div {attr}>\nx\n</div>\n"})
            cases.append({"id": f"ac-span-{aid}-{ext}", "from": ext, "input": f"<span {attr}>x</span>\n"})
            cases.append({"id": f"ac-p-{aid}-{ext}", "from": ext, "input": f"<p {attr}>x</p>\n"})
    return cases


def attr_value_matrix():
    """Codex R3-20: character references and escapes in attribute values."""
    values = {
        "dq-lt": '"&lt;"', "dq-amp": '"&amp;"', "dq-num": '"&#65;"', "dq-hex": '"&#x41;"', "dq-bad": '"&zz;"',
        "dq-bs-a": '"\\a"', "dq-bs-q": '"\\""', "dq-bs-bs": '"\\\\"', "dq-bs-star": '"\\*"', "dq-bs-brace": '"\\}"',
        "sq-lt": "'&lt;'", "sq-bs-a": "'\\a'", "sq-bs-q": "'\\''",
        "unq-lt": "&lt;", "unq-bs-star": "\\*", "unq-bs-a": "\\a", "unq-amp": "&amp;", "unq-bs-bs": "\\\\",
    }
    hosts = {
        "span": "[x]{{a={v}}}", "link": "[x](u){{a={v}}}", "image": "![x](u){{a={v}}}", "code": "`c`{{a={v}}}",
        "heading": "# h {{a={v}}}", "div": "::: {{a={v}}}\nx\n:::", "fence": "```{{a={v}}}\nc\n```",
        "id": "[x]{{#{v}}}", "class": "[x]{{.c a={v} b=1}}",
    }
    cases = []
    for vid, v in values.items():
        for hid, host in hosts.items():
            cases.append({"id": f"av-{hid}-{vid}", "from": "markdown", "input": host.format(v=v) + "\n"})
    return cases


def quote_in_item_matrix():
    """Opus R3-01: a list marker after a block quote line in a list item."""
    cases = []
    for outer in ["-", "*", "1."]:
        pad = " " * (len(outer) + 1)
        for inner in ["-", "*", "+", "1.", "2)", "a.", "#."]:
            for indent in sorted({len(outer) + 1, len(outer) + 2, 4}):
                for quote in ["> a", "> a\n" + pad + "> b"]:
                    qid = "two" if "\n" in quote else "one"
                    sp = " " * indent
                    text = f"{outer} {quote}\n{sp}{inner} sub\n"
                    for ext in ["markdown", "markdown_phpextra", "markdown_strict", "markdown_github"]:
                        cases.append({"id": f"qi-{outer}-{inner}-{indent}-{qid}-{ext}", "from": ext, "input": text})
            cases.append({"id": f"qi-{outer}-{inner}-blank", "from": "markdown", "input": f"{outer} > a\n\n{pad}{inner} sub\n"})
            cases.append({"id": f"qi-{outer}-{inner}-rule", "from": "markdown", "input": f"{outer} > a\n{pad}- - -\n"})
    return cases


def heading_span_matrix():
    """Codex R3-17: a heading line that opens an inline construct with its end on the next line."""
    forms = {
        "code": "`a\nb`", "code2": "``a\nb``", "math": "$a\nb$", "dmath": "$$a\nb$$", "link": "[a\nb](u)",
        "emph": "*a\nb*", "span": "[a\nb]{.c}", "html": "<span>a\nb</span>", "note": "^[a\nb]", "quote": "\"a\nb\"",
        "code-open": "`a\nb", "tag": "<b\nc>", "comment": "<!--a\nb-->", "code-end": "a `b\nc` d",
        "code-heading": "`a\n# b`", "code-hashes": "`a\nb` #", "code-three": "`a\nb\nc`", "code-other-run": "`a\n``b``\nc`",
    }
    cases = []
    for fid, form in forms.items():
        for lead in ["# ", "## ", "# x "]:
            lid = str(len(lead))
            for after in ["", "\n\nz"]:
                aid = "z" if after else "end"
                for ext in ["markdown", "markdown_strict", "markdown_phpextra"]:
                    cases.append({"id": f"hs-{fid}-{lid}-{aid}-{ext}", "from": ext, "input": f"{lead}{form}{after}\n"})
    return cases


def fallback_attr_matrix():
    """Codex R3-19: attributes after an undefined reference."""
    attrs = ['{a="\\"}b"}', '{a="}"}', "{a='}'}", '{.c}', '{#i}', '{a="x"}b"}', '{a=\\}}', '{a="b\\\\"}c}']
    labels = ["[x]", "[x][y]", "[x] [y]", "[x][]", "![x]"]
    cases = []
    for ai, attr in enumerate(attrs):
        for li, label in enumerate(labels):
            for ext in ["markdown", "markdown_phpextra", "markdown_mmd", "markdown-bracketed_spans", "markdown-link_attributes"]:
                cases.append({"id": f"fa-{ai}-{li}-{ext}", "from": ext, "input": f"{label}{attr} t\n"})
    return cases


def foreign_marker_matrix():
    """Opus R3-02: a marker that the flavor does not read, at the content indentation of an item."""
    cases = []
    for outer in ["-", "1."]:
        for marker in ["a.", "A)", "(1)", "i.", "(@)", "#.", "1)", "b)"]:
            for indent in [2, 3]:
                for tail in ["", "\n\n" + ("2." if outer == "1." else "-") + " next"]:
                    tid = "next" if tail else "end"
                    text = f"{outer} item\n{' ' * indent}{marker} b{tail}\n"
                    for ext in ["markdown_strict", "markdown_phpextra", "markdown_mmd", "markdown_github", "markdown"]:
                        cases.append({"id": f"fm-{outer}-{marker}-{indent}-{tid}-{ext}", "from": ext, "input": text})
    return cases


def github_fence_matrix():
    """Opus R3-03: fences with attributes in a flavor without fenced code attributes."""
    openers = ["```{.c}", "```{.c .d}", "``` {.c}", "```{#i}", "~~~{.c}", "```py {.c}"]
    afters = {"para": "\n\ny\n", "fence": "\n\n```\nz\n```\n", "lang": "\n\n```py\nz\n```\n", "end": "\n"}
    cases = []
    for oi, opener in enumerate(openers):
        close = opener[:3]
        for aid, after in afters.items():
            text = f"{opener}\nx\n{close}{after}"
            for ext in ["markdown_github", "markdown_strict", "markdown_phpextra", "markdown-fenced_code_attributes"]:
                cases.append({"id": f"gf-{oi}-{aid}-{ext}", "from": ext, "input": text})
    return cases


def nested_bracket_matrix():
    """Opus R3-05: a link or a span inside a superscript, a note or a link."""
    inner = {"link": "[1](#r1)", "span": "[x]{.c}", "small": "[s t]{.smallcaps}", "ref": "[r]", "cite": "[@k]", "img": "![i](u)"}
    outer = {"sup": "^{i}^", "sub": "~{i}~", "link": "[{i}](u)", "note": "^[{i}]", "emph": "*{i}*", "span": "[{i}]{{.o}}", "strong": "**{i}**"}
    cases = []
    for iid, i in inner.items():
        for oid, o in outer.items():
            text = "a " + o.format(i=i) + " b\n\n[r]: v\n"
            cases.append({"id": f"nb-{oid}-{iid}", "from": "markdown", "input": text})
    return cases


S5_MATRICES = {
    "span-name": span_name_matrix,
    "raw-tag": raw_tag_matrix,
    "caption": caption_matrix,
    "div-comment": div_comment_matrix,
    "meta-image": meta_image_matrix,
    "attr-case": html_attr_case_matrix,
    "attr-value": attr_value_matrix,
    "quote-item": quote_in_item_matrix,
    "heading-span": heading_span_matrix,
    "fallback-attr": fallback_attr_matrix,
    "foreign-marker": foreign_marker_matrix,
    "github-fence": github_fence_matrix,
    "nested-bracket": nested_bracket_matrix,
}


def toml_string(text):
    return json.dumps(text, ensure_ascii=False)


def write_toml(names, path):
    """Writes the cases of the matrices `names` as one harness case file."""
    lines = [
        "# Replay data of the session 5 oracle matrices (tools/differ.py toml).",
        'from = "markdown"',
        "to = []",
        "",
    ]
    for name in names:
        for case in S5_MATRICES[name]():
            lines += ["[[case]]", f"id = {toml_string(case['id'])}", f"from = {toml_string(case['from'])}", f"input = {toml_string(case['input'])}", ""]
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))


def random_cases(count, seed, wide):
    rng = random.Random(seed)
    cases = []
    for i in range(count):
        cases.append(
            {
                "id": f"rnd-{seed}-{i}",
                "from": rng.choice(["markdown"] * 6 + ["markdown_phpextra", "markdown_mmd", "markdown_github", "markdown_strict"]),
                "input": random_doc(rng, wide),
            }
        )
    return cases


def run_oracle(cases_path, results_path):
    oracle = os.environ["RANDOC_ORACLE_PANDOC"]
    subprocess.run(
        [oracle, "lua", os.path.join(HERE, "oracle.lua"), cases_path, results_path],
        check=True,
        timeout=600,
    )
    with open(results_path, encoding="utf-8") as f:
        return json.load(f)


def run_randoc(case):
    try:
        p = subprocess.run(
            [RANDOC, "-f", case["from"], "-t", "json", *shlex.split(os.environ.get("RANDOC_ARGS", ""))],
            input=case["input"].encode("utf-8"),
            capture_output=True,
            timeout=60,
        )
    except subprocess.TimeoutExpired:
        return {"error": "timeout"}
    if p.returncode != 0:
        return {"error": p.stderr.decode("utf-8", "replace").strip()}
    return json.loads(p.stdout)


def first_diff(a, b, path):
    if type(a) is not type(b):
        return path, kind(a), kind(b)
    if isinstance(a, dict):
        if a.get("t") != b.get("t"):
            return path, kind(a), kind(b)
        p = path + [a["t"]] if "t" in a else path
        for k in sorted(set(a) | set(b)):
            if k not in a or k not in b:
                return p, f"key {k}", "missing"
            d = first_diff(a[k], b[k], p)
            if d:
                return d
        return None
    if isinstance(a, list):
        for x, y in zip(a, b):
            d = first_diff(x, y, path)
            if d:
                return d
        if len(a) != len(b):
            extra_a = kind(a[len(b)]) if len(a) > len(b) else "-"
            extra_b = kind(b[len(a)]) if len(b) > len(a) else "-"
            return path, f"more {extra_a}", f"more {extra_b}"
        return None
    if a != b:
        return path, "value", "value"
    return None


def kind(x):
    if isinstance(x, dict):
        return x.get("t", "obj")
    if isinstance(x, list):
        return "list"
    return type(x).__name__


def run(cases, out_dir):
    os.makedirs(out_dir, exist_ok=True)
    cases_path = os.path.join(out_dir, "cases.json")
    with open(cases_path, "w", encoding="utf-8") as f:
        json.dump(cases, f)
    oracle = run_oracle(cases_path, os.path.join(out_dir, "oracle.json"))
    table(cases, oracle, out_dir)


def table(cases, oracle, out_dir):
    os.makedirs(out_dir, exist_ok=True)
    with concurrent.futures.ThreadPoolExecutor(4) as pool:
        mine = list(pool.map(run_randoc, cases))
    groups = {}
    diffs = []
    for case, got in zip(cases, mine):
        want = oracle.get(case["id"])
        d = first_diff(want, got, [])
        if d is None:
            continue
        path, w, g = d
        sig = f"{'/'.join(path[-3:]) or 'top'}: {w} -> {g}"
        groups.setdefault(sig, []).append(case)
        diffs.append({**case, "sig": sig})
    with open(os.path.join(out_dir, "diffs.json"), "w", encoding="utf-8") as f:
        json.dump(diffs, f, indent=1)
    print(f"cases {len(cases)}, equal {len(cases) - len(diffs)}, differ {len(diffs)}")
    for sig, members in sorted(groups.items(), key=lambda kv: -len(kv[1])):
        small = min(members, key=lambda c: len(c["input"]))
        print(f"{len(members):5}  {sig}\n       {small['from']} {small['input']!r}")


def rerun(out_dir):
    """Runs Randoc again on the differing cases of OUT_DIR and prints the new table."""
    with open(os.path.join(out_dir, "diffs.json"), encoding="utf-8") as f:
        cases = [{k: c[k] for k in ("id", "from", "input")} for c in json.load(f)]
    with open(os.path.join(out_dir, "oracle.json"), encoding="utf-8") as f:
        oracle = json.load(f)
    table(cases, oracle, os.path.join(out_dir, "rerun"))


def main():
    cmd = sys.argv[1]
    if cmd == "rerun":
        rerun(sys.argv[2])
        return
    if cmd == "matrix":
        cases = {
            "raw": raw_matrix,
            "blocktag": blocktag_matrix,
            "lazy": lazy_matrix,
            "closer": closer_matrix,
            "abbrev": abbrev_matrix,
            **S5_MATRICES,
        }[sys.argv[2]]()
        run(cases, sys.argv[3])
    elif cmd == "toml":
        write_toml(sys.argv[3:], sys.argv[2])
    elif cmd == "random":
        wide = sys.argv[5:] == ["wide"]
        run(random_cases(int(sys.argv[2]), int(sys.argv[3]), wide), sys.argv[4])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
