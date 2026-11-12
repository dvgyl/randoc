#!/usr/bin/env python3
"""Oracle matrices for the LaTeX and Beamer writers.

Usage:
  matrix.py toml NAME            print the cases of matrix NAME as TOML
  matrix.py diff NAME RANDOC     run the oracle and RANDOC on each case and
                                 print each case that differs

Each case is Markdown input and the command-line options. The oracle is
$RANDOC_ORACLE_PANDOC. At most one oracle process runs at a time.
"""
import json
import os
import subprocess
import sys

def beamer_cases():
    """Headings in containers against the slide level."""
    containers = {
        "plain": ("", ""),
        "div": ("::: d\n", ":::\n"),
        "id-div": ("::: {#i}\n", ":::\n"),
        "column": (":::: columns\n::: column\n", ":::\n::::\n"),
        "notes": ("::: notes\n", ":::\n"),
        "nested": (":::: d\n::: e\n", ":::\n::::\n"),
    }
    before = {
        "none": "",
        "slide": "## Slide\n\ntext\n\n",
        "section": "# Sec\n\n",
    }
    levels = {"auto": [], "1": ["--slide-level=1"], "2": ["--slide-level=2"], "3": ["--slide-level=3"]}
    for cname, (open_, close) in containers.items():
        for bname, prefix in before.items():
            for heading in (1, 2, 3):
                for lname, args in levels.items():
                    body = f"{'#' * heading} H\n\nin\n"
                    text = f"{prefix}{open_}{body}{close}\nafter\n"
                    yield f"m-{cname}-{bname}-h{heading}-{lname}", text, args

def beamer_attr_cases():
    """Attributes of section divs, frames and columns against the slide level."""
    attrs = {
        "frame-classes": "{#i .t .shrink .fragile}",
        "frame-keys": "{.c frameoptions=squeeze label=x}",
        "lang": "{lang=fr}",
        "dir": "{dir=rtl}",
        "lang-dir": "{#d lang=de dir=ltr}",
        "two-ids": "{#d .t}",
    }
    for aname, attr in attrs.items():
        for heading in (1, 2, 3):
            for lname, args in {"auto": [], "2": ["--slide-level=2"]}.items():
                head = f"{'#' * heading} H" + (" {#h}" if aname == "two-ids" else "")
                text = f"::: {attr}\n{head}\n\nin\n:::\n\nafter\n"
                yield f"a-{aname}-h{heading}-{lname}", text, args
                text = f"{'#' * heading} H {attr}\n\nin\n"
                yield f"a-heading-{aname}-h{heading}-{lname}", text, args
    columns = {
        "plain": ".columns",
        "text-width": ".columns .onlytextwidth",
        "all": ".columns .onlytextwidth align=bottom totalwidth=8cm",
        "center": ".columns align=center",
        "unknown": ".columns align=x .other",
    }
    column = {
        "plain": ".column",
        "width": ".column width=30%",
        "align": ".column align=top-baseline width=2cm",
    }
    for cname, outer in columns.items():
        for kname, inner in column.items():
            for body in ("text", "### Block\n\ntext", "## Slide\n\ntext"):
                text = f"## S\n\n:::: {{{outer}}}\n::: {{{inner}}}\n{body}\n:::\n::::\n"
                tag = body.split()[0].count("#")
                yield f"c-{cname}-{kname}-h{tag}", text, []

def bib_cases():
    """Bibliography forms with each citation method."""
    forms = {
        "empty-refs": "x\n\n::: {#refs}\n:::\n",
        "full-refs": "x\n\n::: {#refs}\ny\n:::\n",
        "hanging-refs": "x\n\n::: {#refs .hanging-indent}\n:::\n",
        "last-section": "a\n\n# References\n",
        "last-section-text": "a\n\n# References\n\nb\n",
        "section-refs": "a\n\n# Other\n\n::: {#refs}\n:::\n",
        "middle-refs": "# Refs\n\n::: {#refs}\n:::\n\n# After\n\ny\n",
        "last-subsection": "a\n\n## R\n",
        "refs-heading": "a\n\n# R {#refs}\n",
        "nested-refs": "::: d\n::: {#refs}\n:::\n:::\n",
        "quoted-refs": "> ::: {#refs}\n> y\n> :::\n",
        "unnumbered": "x\n\n# R {.unnumbered}\n",
        "div-section": "x\n\n::: d\n# R\n:::\n",
    }
    for name, text in forms.items():
        for method, args in (("plain", []), ("natbib", ["--natbib"]), ("biblatex", ["--biblatex"])):
            yield f"bib-{name}-{method}", text, args

MATRICES = {
    "beamer": (beamer_cases, "beamer"),
    "beamer-attrs": (beamer_attr_cases, "beamer"),
    "bib": (bib_cases, "latex"),
}

def run(program, fmt, text, args):
    result = subprocess.run([program, "-f", "markdown", "-t", fmt, *args], input=text,
                            capture_output=True, text=True, timeout=60)
    return result.stdout

def main():
    mode, name = sys.argv[1], sys.argv[2]
    cases, fmt = MATRICES[name]
    if mode == "toml":
        for ident, text, args in cases():
            print(f'\n[[case]]\nid = "{ident}"')
            if args:
                print(f"args = {args!r}".replace("'", '"'))
            print(f"input = '''\n{text}'''")
        return
    oracle = os.environ["RANDOC_ORACLE_PANDOC"]
    randoc = sys.argv[3]
    # MATRIX_CACHE names a file that keeps the oracle results between runs.
    cache_path = os.environ.get("MATRIX_CACHE")
    cache = {}
    if cache_path and os.path.exists(cache_path):
        with open(cache_path) as handle:
            cache = json.load(handle)
    total = differ = 0
    for ident, text, args in cases():
        total += 1
        key = json.dumps([fmt, text, args])
        if key not in cache:
            cache[key] = run(oracle, fmt, text, args)
        expected = cache[key]
        actual = run(randoc, fmt, text, args)
        if expected != actual:
            differ += 1
            print(f"== {ident} {args}\n{text}-- oracle\n{expected}-- randoc\n{actual}")
    if cache_path:
        with open(cache_path, "w") as handle:
            json.dump(cache, handle)
    print(f"{differ} of {total} cases differ")

main()
