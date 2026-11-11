#!/usr/bin/env python3
"""The oracle probe of names with a special meaning on a div, a span or a
heading: classes, identifiers and keys.

`screen` takes each lowercase word of the Pandoc manual, each word of the
default HTML templates and style sheets in `crates/randoc-data`, and the CSL
names. It puts the word on a div, a span and a heading as a class, as an
identifier and as a key, in each shape of `shapes`. A word is special when
the oracle output differs from the output for the neutral name `zq`. The
command prints the special words and each difference between the writer and
the oracle.

`record` writes the oracle output of the full matrix for the special names
(`SPECIAL`, the result of `screen`) to `src/classes.jsonl`. The unit test
`html::tests::the_oracle_matrices_replay_with_no_difference` replays the file.

Usage:
  python3 tools/classes.py screen   # finds the special names (about 2 minutes)
  python3 tools/classes.py record   # runs the oracle and writes the data file
  python3 tools/classes.py check    # compares the writer with the data file
  python3 tools/classes.py show     # as check, and prints 12 differences
"""
import glob, itertools, json, os, re, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from differential import ROOT, doc_of, oracle_batch, runner_batch, replay_check, S, A

DATA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src", "classes.jsonl")
NEUTRAL = "zq"
PARA = {"t": "Para", "c": [S("p")]}
OPTIONS = [{}, {"section_divs": True}, {"number_sections": True}, {"section_divs": True, "number_sections": True}]
FORMATS = ["html", "html4"]
# A long name must not change the line breaks of the screening.
WIDE = {"wrap": "none"}
# The special span classes of review round 3.
SPANS = ["smallcaps", "underline", "mark", "kbd"]
CSL = ["refs", "references", "csl-entry", "csl-bib-body", "csl-block", "csl-left-margin", "csl-right-inline",
       "csl-indent", "csl-no-emph", "csl-no-strong", "csl-no-smallcaps", "hanging-indent", "nocase", "citation"]

# The result of `screen` on Pandoc 3.13: each name whose oracle output differs
# from the neutral name, by the place of the name.
SPECIAL = {
    "div.class": ["cell", "column", "columns", "csl-bib-body", "csl-entry", "fragment", "section", "unnumbered"],
    "div.id": ["refs"],
    "div.key": ["wrapper", "number", "id", "class", "role"],
    "span.class": ["smallcaps", "underline", "mark", "kbd", "csl-block", "csl-left-margin", "csl-right-inline",
                   "csl-indent", "csl-no-emph", "csl-no-strong", "csl-no-smallcaps"],
    "span.key": [],
    "h.class": ["section", "unnumbered"],
    "h.key": ["number", "id", "class", "role"],
}


def header(level, attr, text="h"):
    return {"t": "Header", "c": [level, attr, [S(text)]]}


def div(attr, blocks):
    return {"t": "Div", "c": [attr, blocks]}


def span(attr, text="x"):
    return {"t": "Para", "c": [S("a"), {"t": "Span", "c": [attr, [S(text)]]}]}


def raw(text):
    return {"t": "RawBlock", "c": ["html", text]}


CELL = [A, {"t": "AlignDefault"}, 1, 1, [{"t": "Plain", "c": [S("p")]}]]
# Each block kind as the one child of a div.
CHILDREN = {
    "plain": {"t": "Plain", "c": [S("p")]},
    "empty-para": {"t": "Para", "c": []},
    "raw": raw("<x>"),
    "rule": {"t": "HorizontalRule"},
    "quote": {"t": "BlockQuote", "c": [PARA]},
    "bullet": {"t": "BulletList", "c": [[PARA]]},
    "tasks": {"t": "BulletList", "c": [[{"t": "Plain", "c": [S("\u2610"), {"t": "Space"}, S("p")]}]]},
    "ordered": {"t": "OrderedList", "c": [[3, {"t": "Decimal"}, {"t": "Period"}], [[PARA]]]},
    "definition": {"t": "DefinitionList", "c": [[[S("t")], [[PARA]]]]},
    "figure": {"t": "Figure", "c": [["f", [], []], [None, [PARA]], [PARA]]},
    "table": {"t": "Table", "c": [["t", [], []], [None, []], [[{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]],
                                  [A, []], [[A, 0, [], [[A, [CELL]]]]], [A, []]]},
    "line-block": {"t": "LineBlock", "c": [[S("l")]]},
    "section-div": div(["", ["section"], []], [PARA]),
    "column": div(["", ["column"], [["width", "5"]]], [PARA]),
}


def shapes(w):
    """Each place of a name, with the trees that show its rule."""
    width = [["width", "50%"]]
    head = header(1, A)
    return {
        "div.class": {
            "alone": [div(["", [w], []], [PARA, PARA])],
            "empty": [div(["", [w], []], [])],
            "second-class": [div(["", [w, "c"], []], [PARA])],
            "after-class": [div(["", ["c", w], []], [PARA])],
            "twice": [div(["", [w, w], []], [PARA])],
            "with-section": [div(["", [w, "section"], []], [head, PARA])],
            "id-and-key": [div(["i", [w], [["k", "v"]]], [PARA])],
            "heading": [div(["", [w], []], [head, PARA])],
            "heading-id": [div(["d", [w], []], [header(1, ["h", [], []]), PARA]), PARA],
            "heading-same-class": [div(["", [w], []], [header(1, ["", [w], []]), PARA])],
            "heading-later": [div(["", [w], []], [PARA, header(2, A), PARA])],
            "child-divs": [div(["", [w], []], [div(A, [PARA]), div(A, [PARA])])],
            "child-same": [div(["", [w], []], [div(["", [w], width], [PARA]), div(["", [w], width], [PARA])])],
            "child-raw": [div(["", [w], []], [raw("a"), raw("b")])],
            "child-heading-divs": [div(["", [w], []], [div(A, [head, PARA]), div(A, [header(1, A, "b")])])],
            "width": [div(["", [w], width], [PARA])],
            "width-style": [div(["", [w], [["style", "color:red"], ["width", "50%"], ["k", "v"]]], [PARA])],
            "width-twice": [div(["", [w], [["width", "50%"], ["width", "20%"]]], [PARA])],
            "width-middle": [div(["", [w], [["k", "v"], ["width", "5"], ["m", "n"]]], [PARA])],
            "width-before-style": [div(["", [w], [["width", "5"], ["style", "s"]]], [PARA])],
            "plain": [div(["", [w], []], [{"t": "Plain", "c": [S("p")]}, PARA])],
            "role": [div(["", [w], [["role", "x"]]], [PARA])],
            "deep": [div(["", [w], []], [CHILDREN["quote"], CHILDREN["bullet"], CHILDREN["figure"],
                                         {"t": "Para", "c": [S("a"), {"t": "Note", "c": [PARA]}]}])],
            "in-quote": [{"t": "BlockQuote", "c": [div(["", [w], width], [head, PARA, PARA])]}],
            "in-item": [{"t": "BulletList", "c": [[div(["", [w], []], [PARA, PARA])]]}],
            "after-heading": [header(1, A, "a"), div(["", [w], []], [header(2, A), PARA]), PARA],
        },
        "div.id": {
            "alone": [div([w, [], []], [PARA])],
            "empty": [div([w, [], []], [])],
            "class": [div([w, ["c"], [["k", "v"]]], [PARA, PARA])],
            "heading": [div([w, [], []], [head, PARA])],
            "role": [div([w, [], [["role", "r"]]], [PARA])],
            "in-quote": [{"t": "BlockQuote", "c": [div([w, [], []], [PARA])]}],
        },
        "div.key": {
            "one-child": [div(["", [], [[w, "1"]]], [{"t": "HorizontalRule"}])],
            "one-para": [div(["", [], [[w, "1"]]], [PARA])],
            "other-value": [div(["", [], [[w, "0"]]], [PARA])],
            "two-children": [div(["", [], [[w, "1"]]], [PARA, PARA])],
            "empty": [div(["", [], [[w, "1"]]], [])],
            "attributes": [div(["i", ["c"], [["k", "v"], [w, "1"]]], [PARA])],
            "attributes-two": [div(["i", ["c"], [[w, "1"], ["k", "v"]]], [PARA, PARA])],
            "child-div": [div(["i", ["c"], [[w, "1"]]], [div(["j", ["e"], [["m", "n"]]], [PARA])])],
            "child-code": [div(["i", ["c"], [[w, "1"]]], [{"t": "CodeBlock", "c": [["j", [], []], "x"]}])],
            "heading": [div(["", [], [[w, "1"]]], [head, PARA])],
            "heading-only": [div(["i", [], [[w, "1"]]], [head])],
            "heading-key": [div(["", [], [[w, "7"]]], [header(1, ["", [], [[w, "8"]]]), PARA])],
            "in-quote": [{"t": "BlockQuote", "c": [div(["i", [], [[w, "1"]]], [PARA])]}],
            "second-pair": [div(["", [], [["k", "v"], [w, "1"], [w, "2"]]], [PARA])],
            "first-pair-other": [div(["", [], [[w, "2"], [w, "1"]]], [{"t": "HorizontalRule"}])],
            **{f"child-{kind}": [div(["i", ["c"], [[w, "1"], ["k", "v"]]], [block])] for kind, block in CHILDREN.items()},
        },
        "span.class": {
            "alone": [span(["", [w], []])],
            "empty": [span(["", [w], []], "")],
            "second-class": [span(["", [w, "c"], []])],
            "after-class": [span(["", ["c", w], []])],
            "twice": [span(["", [w, w], []])],
            "id-and-key": [span(["i", [w], [["k", "v"]]])],
            "width": [span(["", [w], width])],
            "style": [span(["", [w], [["style", "color:red"]]])],
            "csl-style": [span(["", [w, "csl-no-emph"], []])],
            "csl-style-first": [span(["", ["csl-no-strong", w], []])],
            "smallcaps": [span(["", [w, "smallcaps"], []])],
            "smallcaps-first": [span(["", ["smallcaps", w], []])],
        },
        "span.key": {
            "alone": [span(["", [], [[w, "1"]]])],
            "attributes": [span(["i", ["c"], [["k", "v"], [w, "1"]]])],
        },
        "h.class": {
            "alone": [header(1, ["", [w], []]), PARA],
            "twice": [header(1, ["", [w, w], []]), PARA],
            "id": [header(1, ["h", [w, "c"], []]), PARA, header(2, A, "s"), PARA],
            "in-div": [div(A, [header(1, ["", [w], []]), PARA])],
            "in-div-class": [div(["", ["c"], []], [header(1, ["", [w], []]), PARA])],
            "in-quote": [{"t": "BlockQuote", "c": [header(1, ["", [w], []]), PARA]}],
        },
        "h.key": {
            "alone": [header(1, ["", [], [[w, "7"]]]), PARA],
            "attributes": [header(1, ["h", ["c"], [["k", "v"], [w, "7"]]]), PARA],
            "subsection": [header(1, A, "a"), header(2, ["", [], [[w, "7"]]]), PARA, header(2, A, "b")],
            "in-div": [div(A, [header(1, ["", [], [[w, "7"]]]), PARA])],
            "in-div-id": [div(["d", [], []], [header(1, ["", [], [[w, "7"]]]), PARA])],
            "in-quote": [{"t": "BlockQuote", "c": [header(1, ["", [], [[w, "7"]]]), PARA]}],
        },
    }


def candidates():
    words = set(CSL + SPANS)
    manual = os.path.join(os.environ["RANDOC_PANDOC_SRC"], "MANUAL.txt")
    data = os.path.join(ROOT, "crates", "randoc-data", "data")
    files = [manual] + glob.glob(os.path.join(data, "templates", "*.html*")) + glob.glob(
        os.path.join(data, "*", "template.html"))
    for path in files:
        if os.path.basename(path).startswith("._"):
            continue
        text = open(path, encoding="utf-8").read()
        words.update(re.findall(r"(?<![A-Za-z0-9])[a-z][a-z0-9_:-]*[a-z0-9]", text))
    words.discard(NEUTRAL)
    return sorted(words)


def screen():
    """Prints each name whose oracle output differs from the neutral name."""
    words = candidates()
    cases = []
    for word in [NEUTRAL] + words:
        for place, trees in shapes(word).items():
            for (shape, blocks), (fmt, opts) in itertools.product(
                    trees.items(), [("html", WIDE), ("html4", WIDE), ("html", dict(OPTIONS[3], **WIDE))]):
                cases.append((f"{place} {word} {shape}", doc_of(blocks), fmt, opts))
    ours = runner_batch(cases)
    theirs = oracle_batch(cases)
    per_word = len(cases) // (len(words) + 1)
    neutral = theirs[:per_word]
    special, wrong = {}, {}
    for index, word in enumerate(words):
        start = (index + 1) * per_word
        for offset in range(per_word):
            case, out, mine, base = cases[start + offset], theirs[start + offset], ours[start + offset], neutral[offset]
            place = case[0].split(" ")[0]
            # A key that HTML5 knows has no `data-` prefix: `tools/attributes.py` owns that list.
            same = [base.replace(NEUTRAL, word), base.replace("data-" + NEUTRAL, word)]
            if out not in same:
                special.setdefault(place, set()).add(word)
            if mine != out:
                wrong.setdefault(place, set()).add(word)
    print(f"{len(words)} words, {len(cases)} cases")
    for place in sorted(special):
        print("special", place, json.dumps(sorted(special[place])))
    for place in sorted(wrong):
        print("writer differs", place, json.dumps(sorted(wrong[place])))


def matrix():
    for place, names in SPECIAL.items():
        for word in names + [NEUTRAL]:
            for shape, blocks in shapes(word)[place].items():
                for fmt, opts in itertools.product(FORMATS, OPTIONS):
                    yield f"{place} {word} {shape}", doc_of(blocks), fmt, opts


def record():
    cases = list(matrix())
    with open(DATA, "w") as f:
        for (name, doc, fmt, opts), out in zip(cases, oracle_batch(cases)):
            f.write(json.dumps([name, fmt, opts, doc["blocks"], out], ensure_ascii=False) + "\n")
    print(f"recorded {len(cases)} cases")


if __name__ == "__main__":
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    if mode == "screen":
        screen()
    elif mode == "record":
        record()
    elif mode in ("check", "show"):
        replay_check(DATA, mode == "show")
    else:
        print(__doc__)
