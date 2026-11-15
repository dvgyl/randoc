#!/usr/bin/env python3
"""The oracle matrix of the text fields that the writer takes from metadata.

The fields `pagetitle` (from `title`), `author-meta` (from `author`),
`date-meta` (from `date`) and `description-meta` (from `description`) of
standalone output. Each metadata field gets each value kind, in `html` and
`html4`, through `pandoc lua tools/write_json.lua` with an own probe template
that holds only these variables. `record` writes `src/meta_fields.jsonl`: one
header line with the probe, then `[name, format, document JSON, output]`.
The unit test `standalone::tests::the_meta_field_matrix_has_no_difference`
replays it.

Usage:
  python3 tools/meta_fields.py record
"""
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "src", "meta_fields.jsonl")
P = os.environ["RANDOC_ORACLE_PANDOC"]

PROBE = ("pagetitle=[$pagetitle$]\nauthor-meta=$for(author-meta)$<$author-meta$>$endfor$\n"
         "date-meta=[$date-meta$]\ndescription-meta=[$description-meta$]")
FIELDS = ["title", "author", "date", "description"]


def s(text):
    return {"t": "Str", "c": text}


SP = {"t": "Space"}


def words(text):
    out = []
    for i, w in enumerate(text.split(" ")):
        if i:
            out.append(SP)
        out.append(s(w))
    return out


def para(text):
    return {"t": "Para", "c": words(text)}


def plain(text):
    return {"t": "Plain", "c": words(text)}


def blocks(*bs):
    return {"t": "MetaBlocks", "c": list(bs)}


def inl(text):
    return {"t": "MetaInlines", "c": words(text)}


KINDS = {
    "string": {"t": "MetaString", "c": "Lid & <b> \"q\""},
    "string-date": {"t": "MetaString", "c": "2021-03-05"},
    "inlines": {"t": "MetaInlines", "c": words("Red") + [SP, {"t": "Emph", "c": words("pine")}]},
    "inlines-empty": {"t": "MetaInlines", "c": []},
    "inlines-break": {"t": "MetaInlines", "c": [s("up"), {"t": "LineBreak"}, s("down")]},
    "inlines-note": {"t": "MetaInlines", "c": [s("Tea"), {"t": "Note", "c": [para("cup")]}]},
    "inlines-code": {"t": "MetaInlines", "c": [{"t": "Code", "c": [["", [], []], "a<b"]}, SP, s("end")]},
    "blocks-empty": blocks(),
    "para": blocks(para("Only one")),
    "plain": blocks(plain("Plain one")),
    "para-date": blocks(para("March 5, 2021")),
    "two-paras": blocks(para("One"), para("Two")),
    "three-paras": blocks(para("Uno dos"), para("tres"), para("cuatro")),
    "para-plain": blocks(para("Lead"), plain("tail")),
    "two-dates": blocks(para("2020-01-02"), para("2021-03-04")),
    "quote": blocks({"t": "BlockQuote", "c": [para("Cited words")]}),
    "quote-two": blocks({"t": "BlockQuote", "c": [para("a"), para("b")]}),
    "bullets": blocks({"t": "BulletList", "c": [[plain("ant")], [plain("bee")]]}),
    "ordered": blocks({"t": "OrderedList", "c": [[1, {"t": "Decimal"}, {"t": "Period"}],
                                                   [[para("first")], [para("second")]]]}),
    "code": blocks({"t": "CodeBlock", "c": [["", [], []], "let x = 1;\nx < 2"]}),
    "code-para": blocks(para("Before"), {"t": "CodeBlock", "c": [["", [], []], "mid"]}),
    "header-para": blocks({"t": "Header", "c": [1, ["", [], []], words("Head")]}, para("body")),
    "div": blocks({"t": "Div", "c": [["", [], []], [para("in div")]]}),
    "raw": blocks({"t": "RawBlock", "c": ["html", "<i>raw</i>"]}),
    "rule": blocks(para("x"), {"t": "HorizontalRule"}, para("y")),
    "lines": blocks({"t": "LineBlock", "c": [words("line one"), words("line two")]}),
    "deflist": blocks({"t": "DefinitionList", "c": [[words("term"), [[plain("def")]]]]}),
    "map": {"t": "MetaMap", "c": {"name": inl("Mapped"), "x": inl("y")}},
    "list-inlines": {"t": "MetaList", "c": [inl("Ann"), inl("Bob")]},
    "list-blocks": {"t": "MetaList", "c": [blocks(para("P1"), para("P2")), inl("Cy")]},
    "list-nested": {"t": "MetaList", "c": [{"t": "MetaList", "c": [inl("deep")]}, inl("flat")]},
    "list-map": {"t": "MetaList", "c": [{"t": "MetaMap", "c": {"name": inl("N")}}]},
    "list-empty": {"t": "MetaList", "c": []},
    "list-para": {"t": "MetaList", "c": [blocks(para("Solo")), blocks(plain("Duo")), inl("Dee")]},
    "inlines-quoted": {"t": "MetaInlines", "c": [{"t": "Quoted", "c": [{"t": "DoubleQuote"}, words("say")]}]},
    "bool": {"t": "MetaBool", "c": True},
    "bool-false": {"t": "MetaBool", "c": False},
}


def doc(meta):
    return json.dumps({"pandoc-api-version": [1, 23, 1, 2], "meta": meta, "blocks": []})


def cases():
    for field in FIELDS:
        for kind, value in KINDS.items():
            for fmt in ["html", "html4"]:
                yield f"{field} {kind}", fmt, doc({field: value})
    # Two fields together, so that one rule does not hide another.
    for kind in ["two-paras", "quote", "list-blocks", "code"]:
        for fmt in ["html", "html4"]:
            yield f"all {kind}", fmt, doc({f: KINDS[kind] for f in FIELDS})


def record():
    todo = list(cases())
    requests = "".join(json.dumps({"doc": d, "fmt": f, "template": PROBE}) + "\n" for _, f, d in todo)
    run = subprocess.run([P, "lua", os.path.join(HERE, "write_json.lua")], input=requests.encode(),
                         capture_output=True, timeout=60, check=True)
    lines = run.stdout.decode().splitlines()
    assert len(lines) == len(todo), run.stderr.decode()
    with open(DATA, "w") as f:
        f.write(json.dumps({"oracle": "pandoc 3.13", "probe": PROBE}) + "\n")
        for (name, fmt, d), line in zip(todo, lines):
            result = json.loads(line)
            assert "out" in result, (name, result)
            f.write(json.dumps([name, fmt, d, result["out"]], ensure_ascii=False) + "\n")
    print(f"{len(todo)} cases")


if __name__ == "__main__":
    {"record": record}[sys.argv[1]]()
