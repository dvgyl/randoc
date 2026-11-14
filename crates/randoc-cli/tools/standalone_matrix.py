#!/usr/bin/env python3
"""Writes the matrix of standalone command lines for `html` and `html4`.

Usage: standalone_matrix.py ROOT

The cases replace the part of `conformance/cases/cli/standalone.toml` after
the line MARK. Each case is `-s` (or a template) with a seeded random choice
of the options and variables that the HTML template uses, on one of three
JSON documents. The matrix has no `--katex`: its output holds a script of
the Pandoc HTML writer (GPL), which a stored result must not hold.
"""

import json
import random
import sys

MARK = "# The matrix of standalone command lines for html and html4 (tools/standalone_matrix.py).\n"

EMPTY = '{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}'
TITLED = ('{"pandoc-api-version":[1,23,1,2],"meta":{"author":{"t":"MetaList","c":[{"t":"MetaInlines","c":[{"t":"Str","c":"Ann"}]},'
          '{"t":"MetaInlines","c":[{"t":"Str","c":"Bob"}]}]},"date":{"t":"MetaInlines","c":[{"t":"Str","c":"2026-01-02"}]},'
          '"title":{"t":"MetaInlines","c":[{"t":"Str","c":"The"},{"t":"Space"},{"t":"Emph","c":[{"t":"Str","c":"title"}]}]}},'
          '"blocks":[{"t":"Header","c":[1,["one",[],[]],[{"t":"Str","c":"One"}]]},{"t":"Para","c":[{"t":"Str","c":"Text"},'
          '{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"link"}],["http://e.org",""]]},{"t":"Space"},'
          '{"t":"Code","c":[["",[],[]],"code"]}]},{"t":"Header","c":[2,["two",[],[]],[{"t":"Str","c":"Two"}]]},'
          '{"t":"BulletList","c":[[{"t":"Plain","c":[{"t":"Str","c":"a"}]}],[{"t":"Plain","c":[{"t":"Str","c":"b"}]}]]}]}')
MATH = ('{"pandoc-api-version":[1,23,1,2],"meta":{"abstract":{"t":"MetaInlines","c":[{"t":"Str","c":"Short."}]},'
        '"keywords":{"t":"MetaList","c":[{"t":"MetaInlines","c":[{"t":"Str","c":"k1"}]},{"t":"MetaInlines","c":[{"t":"Str","c":"k2"}]}]},'
        '"lang":{"t":"MetaInlines","c":[{"t":"Str","c":"en-GB"}]}},"blocks":[{"t":"Header","c":[1,["math",[],[]],[{"t":"Str","c":"Math"}]]},'
        '{"t":"Para","c":[{"t":"Math","c":[{"t":"InlineMath"},"x^2"]},{"t":"Space"},{"t":"Math","c":[{"t":"DisplayMath"},"\\\\sum_i i"]}]},'
        '{"t":"Para","c":[{"t":"Str","c":"A"},{"t":"Space"},{"t":"Str","c":"note."},{"t":"Note","c":[{"t":"Para","c":[{"t":"Str","c":"N."}]}]}]},'
        '{"t":"CodeBlock","c":[["",[],[]],"print(1)"]}]}')
DOCS = [("empty", EMPTY), ("titled", TITLED), ("math", MATH)]

OPTIONS = [
    ["--toc"], ["--toc", "--toc-depth=1"], ["-N"], ["--section-divs"], ["-T", "Pre"], ["-c", "a.css"],
    ["-c", "a.css", "-c", "b.css"], ["-H", "h.html"], ["-B", "b.html"], ["-A", "a.html"], ["-V", "pagetitle=PT"],
    ["-M", "title=MT"], ["-M", "lang=de"], ["-V", "lang=fr"], ["-V", "document-css=false"], ["--mathjax"],
    ["--mathml"], ["--webtex"], ["--html-q-tags"], ["-V", "mainfont=Serif"], ["-V", "maxwidth=30em"],
    ["-M", "subtitle=Sub"], ["-M", "author=Al"], ["-M", "date=Today"], ["-V", "header-includes=<meta x>"],
    ["-V", "toc-title=Inhalt"], ["-M", "toc-title=Contents"], ["-V", "dir=rtl"], ["-V", "fontsize=12pt"],
    ["-V", "linestretch=1.5"], ["-V", "backgroundcolor=#eee"], ["-V", "include-before=IB"], ["--css=b.css"],
    ["--variable-json=author=[\"J1\",\"J2\"]"], ["-M", "description-meta=D"], ["-V", "quotes=true"],
    ["--number-offset=2"], ["--id-prefix=p-"], ["--wrap=none"], ["--columns=20"], ["--eol=crlf"],
    ["--ascii"], ["-M", "pagetitle=MP"], ["-V", "classoption=x"], ["--metadata=keywords:kx"],
]
FILES = {"a.css": "p{}\n", "b.css": "h1{}\n", "h.html": "<meta name=\"h\">\n", "b.html": "<p>before</p>\n",
         "a.html": "<p>after</p>\n", "t.html": "<t>$pagetitle$|$title-prefix$|$lang$|$for(css)$$css$,$endfor$</t>\n$body$\n"}


def cases():
    rng = random.Random(20261112)
    out = []
    for writer in ("html", "html4"):
        for index in range(150):
            name, doc = DOCS[index % len(DOCS)]
            extra = []
            for option in rng.sample(OPTIONS, rng.randint(1, 4)):
                extra += option
            start = ["--template=t.html"] if index % 10 == 9 else ["-s"]
            out.append((f"matrix-{writer}-{index:03d}-{name}", ["-f", "json", "-t", writer] + start + extra, doc))
    return out


def main():
    root = sys.argv[1]
    path = f"{root}/conformance/cases/cli/standalone.toml"
    text = open(path, encoding="utf-8").read()
    text = text.split(MARK)[0].rstrip("\n") + "\n\n" + MARK
    files = "files = { " + ", ".join(f"{json.dumps(k)} = {json.dumps(v)}" for k, v in FILES.items()) + " }\n"
    for ident, cmd, doc in cases():
        text += f'\n[[case]]\nid = "{ident}"\ncmd = {json.dumps(cmd)}\ninput = {json.dumps(doc)}\n' + files
    open(path, "w", encoding="utf-8").write(text)
    print(len(cases()))


if __name__ == "__main__":
    main()
