#!/usr/bin/env python3
"""The oracle matrix of the table of contents for headings in divs.

Which headings of a div go into the table of contents, and which classes
remove a heading from it: divs that are a section and divs that are not,
the classes `unlisted` and `unnumbered` on the div and on the heading,
wrapper divs, and the special classes of slide divs. Each tree runs with 3
option sets in `html` and `html4` through `pandoc lua tools/write_json.lua`
with the own template `$toc$|$body$`. `record` writes `src/toc.jsonl`:
`[name, format, options, document JSON, output]`. The unit test
`standalone::tests::the_toc_matrix_has_no_difference` replays it.

Usage:
  python3 tools/toc_matrix.py record
"""
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "src", "toc.jsonl")
P = os.environ["RANDOC_ORACLE_PANDOC"]
TEMPLATE = "$toc$|$body$"


def s(t):
    return {"t": "Str", "c": t}


def h(level, text, id="", cls=(), kv=()):
    return {"t": "Header", "c": [level, [id, list(cls), list(kv)], [s(text)]]}


def p(text):
    return {"t": "Para", "c": [s(text)]}


def div(cls, blocks, id="", kv=()):
    return {"t": "Div", "c": [[id, list(cls), [list(x) for x in kv]], blocks]}


TREES = {
    "null para h1": [div([], [p("x"), h(1, "A", "a")])],
    "null h1 h1": [div([], [h(1, "A", "a"), h(1, "B", "b")])],
    "null h1 para h1": [div([], [h(1, "A", "a"), p("p"), h(1, "B", "b")])],
    "null h1": [div([], [h(1, "A", "a")])],
    "null h1 h2": [div([], [h(1, "A", "a"), h(2, "B", "b")])],
    "section para h1": [div(["section"], [p("x"), h(1, "A", "a")])],
    "section h1 h1": [div(["section"], [h(1, "A", "a"), h(1, "B", "b")])],
    "section h1": [div(["section"], [h(1, "A", "a")])],
    "columns h1": [div(["columns"], [h(1, "A", "a")])],
    "columns h1 h2": [div(["columns"], [h(1, "A", "a"), h(2, "B", "b")])],
    "columns para h1": [div(["columns"], [p("x"), h(1, "A", "a")])],
    "columns of columns": [div(["columns"], [div(["column"], [h(1, "A", "a")]),
                                             div(["column"], [h(1, "B", "b")])])],
    "class para h1 h2": [div(["k"], [p("x"), h(1, "A", "a"), h(2, "B", "b")])],
    "nested null": [div([], [div([], [p("x"), h(1, "A", "a")])])],
    "h1 then div": [h(1, "T", "t"), div([], [p("x"), h(2, "A", "a")])],
    "div h1 nested div": [div(["c"], [h(1, "A", "a"), div([], [p("x"), h(2, "B", "b")])])],
    "h1 div h2 first": [h(1, "T", "t"), div(["c"], [h(2, "A", "a"), p("x")]), h(1, "U", "u")],
    "unlisted div": [div(["unlisted"], [h(1, "A", "a")])],
    "unlisted div id": [div(["unlisted"], [h(1, "A", "a")], id="d")],
    "unlisted heading in div": [div(["c"], [h(1, "A", "a", ["unlisted"])])],
    "unlisted both": [div(["unlisted"], [h(1, "A", "a", ["unlisted"])])],
    "unlisted unnumbered div": [div(["unlisted", "unnumbered"], [h(1, "A", "a")])],
    "unnumbered div": [div(["unnumbered"], [h(1, "A", "a"), h(2, "B", "b")])],
    "unlisted heading unnumbered div": [div(["unnumbered"], [h(1, "A", "a", ["unlisted"])])],
    "unlisted div pair": [div(["unlisted"], [h(1, "A", "a")], kv=[("k", "v")])],
    "unlisted heading child": [h(1, "A", "a", ["unlisted"]), h(2, "B", "b"), h(1, "C", "c")],
    "wrapper one section": [div([], [h(1, "A", "a"), p("x")], kv=[("wrapper", "1")])],
    "wrapper two sections": [div([], [h(1, "A", "a"), h(1, "B", "b")], kv=[("wrapper", "1")])],
    "wrapper para h1": [div([], [p("x"), h(1, "A", "a")], kv=[("wrapper", "1")])],
    "quote h1": [{"t": "BlockQuote", "c": [h(1, "A", "a")]}],
    "list h1": [{"t": "BulletList", "c": [[h(1, "A", "a")]]}],
    "deep divs": [div([], [div([], [div(["c"], [h(1, "A", "a")])])])],
    "div first div": [div(["o"], [div(["i"], [h(1, "A", "a")]), p("x")])],
    "one div of a section div": [div([], [div(["c"], [h(1, "A", "a")])])],
    "one div of a heading div": [div([], [div([], [h(1, "A", "a")])])],
    "one class div of a section div": [div(["o"], [div(["i"], [h(1, "A", "a")])])],
    "section div and para": [div([], [div(["c"], [h(1, "A", "a")]), p("x")])],
    "two section divs": [div([], [div(["c"], [h(1, "A", "a")]), div(["d"], [h(1, "B", "b")])])],
    "two section divs h1 h2": [div([], [div(["c"], [h(1, "A", "a")]), div(["d"], [h(2, "B", "b")])])],
    "one div h1 h2": [div([], [div(["c"], [h(1, "A", "a"), h(2, "B", "b")])])],
    "one div h1 para": [div([], [div(["c"], [h(1, "A", "a"), p("x")])])],
    "class h1 para": [div(["k"], [h(1, "A", "a"), p("p")])],
    "class h1 h1": [div(["k"], [h(1, "A", "a"), h(1, "B", "b")])],
    "one div of class h1 h1": [div([], [div(["k"], [h(1, "A", "a"), h(1, "B", "b")])])],
    "para then section div": [div([], [p("x"), div(["k"], [h(1, "A", "a")])])],
    "section of one div": [div(["section"], [div(["k"], [h(1, "A", "a")])])],
}
OPTS = [{"toc": True}, {"toc": True, "number_sections": True}, {"toc": True, "section_divs": True}]


def cases():
    for name, blocks in TREES.items():
        d = json.dumps({"pandoc-api-version": [1, 23, 1, 2], "meta": {}, "blocks": blocks})
        for opts in OPTS:
            for fmt in ["html", "html4"]:
                yield name, fmt, opts, d


def record():
    todo = list(cases())
    requests = "".join(json.dumps({"doc": d, "fmt": f, "template": TEMPLATE,
                                   "opts": dict(o, variables={"pagetitle": "Fixed"})}) + "\n"
                       for _, f, o, d in todo)
    run = subprocess.run([P, "lua", os.path.join(HERE, "write_json.lua")], input=requests.encode(),
                         capture_output=True, timeout=60, check=True)
    lines = run.stdout.decode().splitlines()
    assert len(lines) == len(todo), run.stderr.decode()
    with open(DATA, "w") as f:
        for (name, fmt, opts, d), line in zip(todo, lines):
            result = json.loads(line)
            assert "out" in result, (name, result)
            f.write(json.dumps([name, fmt, opts, d, result["out"]], ensure_ascii=False) + "\n")
    print(f"{len(todo)} cases")


if __name__ == "__main__":
    {"record": record}[sys.argv[1]]()
