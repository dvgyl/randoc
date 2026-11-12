#!/usr/bin/env python3
"""The oracle replay of standalone output (`pandoc.write` with a template).

Each case is a Markdown document, a format, writer options and a template:
`default` (the default template of the format) or `probe`, which prints each
variable that the writer owns. `record` runs `tools/standalone.lua` through
`pandoc lua` and writes `src/standalone.jsonl`: one header line with the
probe template, then `[name, format, options, document JSON, template,
output]`. The unit test `standalone::tests::the_standalone_replay_has_no_difference`
replays it.

Usage:
  python3 tools/standalone.py record
"""
import itertools, json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "src", "standalone.jsonl")
P = os.environ["RANDOC_ORACLE_PANDOC"]

NAMES = ["body", "toc", "table-of-contents", "title", "pagetitle", "author", "date", "date-meta",
         "lang", "dir", "math", "mathjax", "quotes", "abstract", "abstract-title", "keywords",
         "description-meta", "document-css", "idprefix", "html5", "table-caption-below", "meta-json",
         "subtitle", "css", "x", "toc-title", "displaymath-css", "highlighting-css"]
PROBE = "\n".join(f"{n}=[$if({n})$T$else$F$endif$|${n}$]" for n in NAMES) + \
    "\nauthor-meta=$for(author-meta)$<$author-meta$>$endfor$\nkw=$for(keywords)$<$keywords$>$sep$,$endfor$\n"

METAS = {
    "none": "",
    "plain-title": "title: Plain title",
    "rich-title": "title: The *best* \"title\" & <x> 's",
    "empty-title": "title: ''",
    "full": "title: T\nsubtitle: S\nauthor: [A One, B *Two*]\ndate: March 5, 2021",
    "single-author": "author: Single Author\ndate: 2020-01-02",
    "map-author": "author:\n- name: Mapped\n- Plain & <i>x</i>\n- true\ndate: [2020-01-02]",
    "abstract": "abstract: |\n  Para one.\n\n  Para two.\nkeywords: [k1, k*2*]\nlang: en\ndir: rtl",
    "description": "description: Desc *x* & \"y\"\nlang: en-US\ndate: 5 March 2021",
    "kinds": "title: T\ndate: 03/05/21\nflag: true\nn: 3\nnested:\n  a: [1, {b: c}]\nempty: []",
    "css": "title: T\nheader-includes: '<meta x>'\ncss: [a.css, b.css]",
    "note": "title: Title with a note^[The note.]\ndate: next week",
    "override": "title: T\npagetitle: PT\nauthor-meta: AM\ndate-meta: DM\nbody: B\ntoc: TT\nquotes: Q\nhtml5: H\ndocument-css: false",
    "long": "title: A long title with many words, longer than the line width of seventy-two columns here",
    "blocks-title": "title: |\n  Block title one.\n\n  Block two.\nauthor: |\n  Block author.",
}

BODIES = {
    "quotes": "Para with \"quotes\" and 'single'.",
    "toc": "# One\n\nText.\n\n## Sub {.unnumbered}\n\n### Deep\n\n## Hidden {.unlisted}\n\n### Under hidden\n\n"
           "## Numbered hidden {.unlisted .unnumbered}\n\n# Two {#custom .cls}\n\n::: {.section}\n## In div\n:::\n\n"
           "#### Skip\n\n# Three\n\n##### Five",
    "links": "# A [link](u) and note[^n]\n\n[^n]: The note.\n\n## B *em* `c`\n\n> # In quote\n\n- # In list\n\n"
             "::: d\n# In plain div\n\nText.\n:::",
    "empty": "",
    "table": "| a | b |\n|---|---|\n| 1 | 2 |\n\n: Caption",
    "text": "# É ü & <b>raw</b>\n\nText & `code`.",
    "odd": "#\n\n## {#only-id}\n\n# Long heading with many words that is longer than the line width of the text\n\n"
           "## Level two\n\n# Last",
    "levels": "## Starts at two\n\n### Three\n\n## Two again\n\n#### Four",
}

OPTS = [{}, {"toc": True}, {"toc": True, "toc_depth": 1}, {"toc": True, "toc_depth": 6},
        {"toc": True, "number_sections": True}, {"toc": True, "section_divs": True, "id_prefix": "p-"},
        {"toc": True, "number_sections": True, "section_divs": True}, {"q_tags": True},
        {"ascii": True, "toc": True}, {"wrap": "none", "toc": True}, {"columns": 30, "toc": True},
        {"wrap": "preserve"}, {"mathjax": True}, {"caption_below": True},
        {"variables": {"title": "VT", "lang": "fr", "css": "v.css", "x": "vx", "toc": "VTOC"}}]


def document(meta, body):
    return (f"---\n{meta}\n---\n\n{body}\n" if meta else body + "\n")


def cases():
    bodies = list(BODIES.items())
    metas = list(METAS.items())
    # Each metadata form with each option set, in the probe template.
    for i, ((mk, meta), opts, fmt) in enumerate(itertools.product(metas, OPTS, ["html", "html4"])):
        bk, body = bodies[i % len(bodies)]
        yield f"probe {mk} {bk} {json.dumps(opts, sort_keys=True)}", fmt, opts, document(meta, body), "probe"
    # Each body with each option set, in the default template.
    for i, ((bk, body), opts, fmt) in enumerate(itertools.product(bodies, OPTS, ["html", "html4"])):
        mk, meta = metas[i % len(metas)]
        yield f"default {mk} {bk} {json.dumps(opts, sort_keys=True)}", fmt, opts, document(meta, body), "default"


def record():
    rows = list(cases())
    header = json.dumps({"templates": {"probe": PROBE}}, ensure_ascii=False)
    lines = [header] + [json.dumps({"markdown": md, "fmt": fmt, "opts": opts, "template": t})
                        for _, fmt, opts, md, t in rows]
    r = subprocess.run([P, "lua", os.path.join(HERE, "standalone.lua")], input="\n".join(lines).encode() + b"\n",
                       capture_output=True, timeout=60)
    outs = [json.loads(line) for line in r.stdout.decode().splitlines()]
    if r.returncode != 0 or len(outs) != len(rows):
        raise SystemExit(f"the oracle did not end: exit {r.returncode}, {r.stderr.decode()[:300]}")
    errors = 0
    with open(DATA, "w") as f:
        f.write(header + "\n")
        for (name, fmt, opts, _, template), out in zip(rows, outs):
            if "error" in out:
                errors += 1
                print("error:", name, out["error"][:200])
                continue
            f.write(json.dumps([name, fmt, opts, out["doc"], template, out["out"]], ensure_ascii=False,
                               separators=(",", ":")) + "\n")
    print(f"{len(rows) - errors} cases recorded, {errors} errors")


if __name__ == "__main__":
    record()
