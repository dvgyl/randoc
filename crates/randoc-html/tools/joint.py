#!/usr/bin/env python3
"""The joint oracle matrix of divs, headings, notes and the special names.

One case is a unit (a heading, or a div with 0, 1 or 2 headings, or nested
section divs) in a container, with heading attributes, div attributes, notes
and options. The axes:

- container: top level, block quote, bullet item, ordered item, definition,
  note, plain div, section div, table cell, figure;
- unit: `h`, `div0` (one paragraph), `div0hr`, `div0list`, `div1`, `div1h`
  (a heading alone), `div2`
  (two headings of one level), `div2sub` (a subsection), `nested`
  (a section div inside the div), `nested2` (a heading, then a section div);
- heading: level, classes, identifier, pairs;
- div: classes (each special name), identifier, pairs;
- notes: none, in the text, in the heading, a note in a note, after the unit;
- options: `--section-divs`, `--number-sections`, `--reference-location`,
  `html` and `html4`.

The matrix is a pairwise design over all axes, the full product of the axes
that the round 5 findings name together (`PRODUCTS`), and the inputs of the
round 5 reviews (`reviews`). `record` writes `src/joint.jsonl`; the unit
test `html::tests::the_oracle_matrices_replay_with_no_difference` replays it.

Round 6 adds `round6_cases`: chains of divs with a div first (R6-01) and
section numbers with quotes (R6-L1). The Opus round 6 files are lists of
`{id, doc, fmt, opts}` objects.

Usage:
  python3 tools/joint.py record [REVIEW_DIR ...]  # oracle run, writes the data file
  python3 tools/joint.py add [REVIEW_DIR ...]     # oracle run for the cases that the data file does not have
  python3 tools/joint.py check                    # compares the writer with the data file
  python3 tools/joint.py show                     # as check, and prints 12 differences
"""
import itertools, json, os, random, re, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from differential import doc_of, oracle_batch, replay_check, S, A

DATA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src", "joint.jsonl")

AXES = {
    "container": ["top", "bq", "bullet", "ordered", "def", "note", "div", "secdiv", "cell", "figure"],
    "unit": ["h", "div0", "div0hr", "div0list", "div1", "div2", "div2sub", "nested", "nested2"],
    "level": [1, 2],
    "hclasses": [[], ["unnumbered"], ["unlisted"], ["c"], ["c", "c"], ["section"]],
    "hid": ["", "h", "d"],
    "hpairs": [[], [["number", "3"]], [["role", "doc-x"]], [["role", "heading"]], [["k", "v"]]],
    "dclasses": [[], ["section"], ["section", "section"], ["section", "c"], ["c", "section"], ["c"],
                 ["column"], ["section", "column"], ["columns"], ["section", "columns"], ["cell"],
                 ["fragment"], ["csl-bib-body"], ["csl-entry"], ["section", "csl-entry"],
                 ["incremental"], ["nonincremental"], ["unnumbered"]],
    "did": ["", "refs", "d"],
    "dpairs": [[], [["width", "40%"]], [["wrapper", "1"]], [["number", "7"]], [["style", "s"]],
               [["wrapper", "1"], ["width", "1%"]], [["style", "1"], ["k", "v"], ["style", "2"]]],
    "notes": ["none", "text", "heading", "nested", "after"],
    "section_divs": [False, True],
    "number_sections": [False, True],
    "refloc": ["document", "block", "section", None],
    "fmt": ["html", "html4"],
}

DEFAULT = {"container": "top", "unit": "div1", "level": 1, "hclasses": [], "hid": "", "hpairs": [],
           "dclasses": ["section"], "did": "", "dpairs": [], "notes": "none", "section_divs": False,
           "number_sections": False, "refloc": None, "fmt": "html"}

# The axes that the findings of round 5 name together; each product is full.
PRODUCTS = [
    # R5-03, R5-06, R5-07, R5-10, L3: the element of a section div outside the section path.
    {"container": ["top", "bq", "bullet", "div", "secdiv", "note"], "unit": ["div1", "div2", "nested"],
     "dclasses": [["section"], ["section", "section"], ["section", "c"], ["c", "section"]],
     "did": ["", "d"], "hclasses": [[], ["unnumbered"], ["c"], ["c", "c"]], "hid": ["", "h", "d"],
     "section_divs": [False, True], "fmt": ["html", "html4"]},
    # R5-01, R5-05 (Opus), R5-09: the place of the notes.
    {"container": ["top", "bq", "bullet", "def", "note", "div", "secdiv", "cell"],
     "unit": ["h", "div1", "div2", "div2sub", "nested"], "dclasses": [["section"], [], ["section", "c"]],
     "notes": ["text", "heading", "nested", "after"], "refloc": ["block", "section", "document"],
     "section_divs": [False, True], "fmt": ["html", "html4"]},
    # R5-04 (both), R5-05 (Codex), L1: the special names with `section`, wrappers and roles.
    {"dclasses": AXES["dclasses"], "did": ["", "refs", "d"], "dpairs": AXES["dpairs"],
     "unit": ["div0", "div0hr", "div1", "div1h"], "container": ["top", "bq"], "fmt": ["html", "html4"]},
    # R5-02 (Codex), L4, R5-03 (Codex), R5-11: numbers.
    {"container": ["top", "bq", "div", "secdiv", "bullet"], "unit": ["h", "div1", "div0"],
     "hclasses": [[], ["unnumbered"]], "hpairs": [[], [["number", "7"]], [["number", "<"]], [["number", "a b"]]],
     "dpairs": [[], [["number", "7"]]], "number_sections": [False, True], "section_divs": [False, True],
     "fmt": ["html", "html4"]},
    # R5-08 (Codex): columns with a section heading.
    {"dclasses": [["section", "columns"], ["columns"], ["columns", "section"], ["section", "column"]],
     "unit": ["div0", "div1", "div2", "div2sub"], "hpairs": [[], [["style", "display:inline"]]],
     "container": ["top", "bq", "div"], "section_divs": [False, True], "fmt": ["html", "html4"]},
]


def note(text, inner=False):
    body = [S(text)] + ([{"t": "Note", "c": [{"t": "Para", "c": [S("n2")]}]}] if inner else [])
    return {"t": "Note", "c": [{"t": "Para", "c": body}]}


def unit_blocks(c):
    notes = c["notes"]
    level = c["level"]
    head_inl = [S("h")] + ([note("hn")] if notes == "heading" else [])
    text_inl = [S("t")] + ([note("tn", inner=notes == "nested")] if notes in ("text", "nested") else [])
    heading = {"t": "Header", "c": [level, [c["hid"], c["hclasses"], c["hpairs"]], head_inl]}
    para = {"t": "Para", "c": text_inl}
    second = lambda lv: {"t": "Header", "c": [lv, ["", [], []], [S("s")]]}
    attr = [c["did"], c["dclasses"], c["dpairs"]]
    unit = c["unit"]
    if unit == "h":
        blocks = [heading, para]
    elif unit == "div0":
        blocks = [{"t": "Div", "c": [attr, [para]]}]
    elif unit == "div0hr":
        blocks = [{"t": "Div", "c": [attr, [{"t": "HorizontalRule"}]]}]
    elif unit == "div0list":
        blocks = [{"t": "Div", "c": [attr, [{"t": "BulletList", "c": [[{"t": "Plain", "c": text_inl}]]}]]}]
    elif unit == "div1h":
        blocks = [{"t": "Div", "c": [attr, [heading]]}]
    elif unit == "div1":
        blocks = [{"t": "Div", "c": [attr, [heading, para]]}]
    elif unit == "div2":
        blocks = [{"t": "Div", "c": [attr, [heading, para, second(level), {"t": "Para", "c": [S("u")]}]]}]
    elif unit == "div2sub":
        blocks = [{"t": "Div", "c": [attr, [heading, para, second(level + 1), {"t": "Para", "c": [S("u")]}]]}]
    elif unit == "nested":
        blocks = [{"t": "Div", "c": [attr, [{"t": "Div", "c": [["", ["section"], []], [heading, para]]}]]}]
    else:  # nested2
        inner = {"t": "Div", "c": [["", ["section"], []], [second(level + 1), para]]}
        blocks = [{"t": "Div", "c": [attr, [heading, inner]]}]
    if notes == "after":
        blocks.append({"t": "Para", "c": [S("a"), note("an")]})
    return blocks


def contain(container, blocks):
    if container == "top":
        return blocks
    if container == "bq":
        return [{"t": "BlockQuote", "c": blocks}]
    if container == "bullet":
        return [{"t": "BulletList", "c": [blocks]}]
    if container == "ordered":
        return [{"t": "OrderedList", "c": [[1, {"t": "DefaultStyle"}, {"t": "DefaultDelim"}], [blocks]]}]
    if container == "def":
        return [{"t": "DefinitionList", "c": [[[S("term")], [blocks]]]}]
    if container == "note":
        return [{"t": "Para", "c": [S("o"), {"t": "Note", "c": blocks}]}]
    if container == "div":
        return [{"t": "Div", "c": [A, blocks]}]
    if container == "secdiv":
        return [{"t": "Div", "c": [["", ["section"], []], blocks]}]
    if container == "cell":
        cell = [A, {"t": "AlignDefault"}, 1, 1, blocks]
        return [{"t": "Table", "c": [A, [None, []], [[{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]],
                                     [A, []], [[A, 0, [], [[A, [cell]]]]], [A, []]]}]
    return [{"t": "Figure", "c": [A, [None, []], blocks]}]


def case_of(c):
    opts = {}
    for key in ("section_divs", "number_sections"):
        if c[key]:
            opts[key] = True
    if c["refloc"]:
        opts["refloc"] = c["refloc"]
    name = "joint " + json.dumps({k: c[k] for k in AXES if c[k] != DEFAULT[k]}, separators=(",", ":"))
    return name, c["fmt"], opts, contain(c["container"], unit_blocks(c))


def pairwise(seed=5):
    """A greedy pairwise design: each pair of values of two axes is in a case."""
    rng = random.Random(seed)
    names = list(AXES)
    sizes = [len(AXES[n]) for n in names]
    todo = {(i, a, j, b) for i, j in itertools.combinations(range(len(names)), 2)
            for a in range(sizes[i]) for b in range(sizes[j])}
    rows = []
    while todo:
        i, a, j, b = min(todo)
        best, gain = None, -1
        for _ in range(60):
            row = [rng.randrange(s) for s in sizes]
            row[i], row[j] = a, b
            g = sum((x, row[x], y, row[y]) in todo for x, y in itertools.combinations(range(len(names)), 2))
            if g > gain:
                best, gain = row, g
        todo -= {(x, best[x], y, best[y]) for x, y in itertools.combinations(range(len(names)), 2)}
        rows.append({n: AXES[n][v] for n, v in zip(names, best)})
    return rows


def incremental_cases():
    """R5-02 (Opus): lists in a div with the class `incremental` or
    `nonincremental`, each list kind, nested lists, with and without
    `--incremental`."""
    plain = lambda t: {"t": "Plain", "c": [S(t)]}
    bullet = lambda *items: {"t": "BulletList", "c": [list(i) for i in items] or [[plain("a")]]}
    lists = {
        "bullet": [bullet()],
        "ordered": [{"t": "OrderedList", "c": [[1, {"t": "DefaultStyle"}, {"t": "DefaultDelim"}], [[plain("a")]]]}],
        "ordered-start": [{"t": "OrderedList", "c": [[2, {"t": "Decimal"}, {"t": "Period"}], [[plain("a")]]]}],
        "example": [{"t": "OrderedList", "c": [[1, {"t": "Example"}, {"t": "TwoParens"}], [[plain("a")]]]}],
        "definition": [{"t": "DefinitionList", "c": [[[S("t")], [[plain("d")]]]]}],
        "task": [bullet([{"t": "Plain", "c": [S("\u2610"), {"t": "Space"}, S("x")]}])],
        "nested": [bullet([plain("a"), bullet()])],
        "in-quote": [{"t": "BlockQuote", "c": [bullet()]}],
        "after-para": [{"t": "Para", "c": [S("p")]}, bullet()],
        "after-heading": [{"t": "Header", "c": [1, A, [S("h")]]}, bullet()],
        "in-note": [{"t": "Para", "c": [S("p"), {"t": "Note", "c": [bullet()]}]}],
        "inner-div": [{"t": "Div", "c": [["", ["nonincremental"], []], [bullet()]]}],
        "inner-div-incremental": [{"t": "Div", "c": [["", ["incremental"], []], [bullet()]]}],
    }
    classes = [["incremental"], ["nonincremental"], ["incremental", "nonincremental"],
               ["nonincremental", "incremental"], ["section", "incremental"], ["incremental", "section"], []]
    for (kind, blocks), cls, wrapper, container, inc, fmt in itertools.product(
            lists.items(), classes, [False, True], ["top", "bq", "bullet"], [False, True], ["html", "html4"]):
        pairs = [["wrapper", "1"]] if wrapper else []
        unit = [{"t": "Div", "c": [["", cls, pairs], blocks]}]
        opts = {"incremental": True} if inc else {}
        yield f"incremental {kind} {cls} wrapper={wrapper} {container}", fmt, opts, contain(container, unit)


def round6_cases():
    """R6-01: chains of divs with a div first, with the heading or paragraph at
    the bottom and a second block at each level. R6-L1: section numbers with
    `"` and `'`, also long ones that wrap."""
    para = lambda t: {"t": "Para", "c": [S(t)]}
    head = lambda lv: {"t": "Header", "c": [lv, A, [S("h")]]}
    bottoms = {"h1": head(1), "h2": head(2), "para": para("x")}
    seconds = {"para": para("p"), "h1": head(1), "h2": head(2)}
    for depth, (bk, bottom), (sk, second), cls, container, secdivs, fmt in itertools.product(
            [1, 2, 3, 5], bottoms.items(), seconds.items(), [[], ["section"]], ["top", "bq"], [False, True],
            ["html", "html4"]):
        block = bottom
        for _ in range(depth):
            block = {"t": "Div", "c": [["", cls, []], [block, second]]}
        opts = {"section_divs": True} if secdivs else {}
        yield f"round6 chain {depth} {bk} {sk} {cls} {container}", fmt, opts, contain(container, [block])
    numbers = ['a"b', "a'b", "'", '"7"', "x\"y z", "it's a number text that is long enough to wrap the line here"]
    for number, place, numbered, fmt in itertools.product(numbers, ["top", "bq", "div"], [False, True],
                                                         ["html", "html4"]):
        heading = {"t": "Header", "c": [1, ["", [], [["number", number]]], [S("Heading"), {"t": "Space"}, S("text")]]}
        opts = {"number_sections": True} if numbered else {}
        yield f"round6 number {number!r} {place}", fmt, opts, contain(place, [heading, para("t")])


def products():
    for product in PRODUCTS:
        keys = list(product)
        for values in itertools.product(*(product[k] for k in keys)):
            yield {**DEFAULT, **dict(zip(keys, values))}


def reviews(dirs):
    """The inputs of the round 5 reviews: Codex `evidence.jsonl` and `witnesses.json`, Opus `diffs-*.json`."""
    for d in dirs:
        path = os.path.join(d, "evidence.jsonl")
        if os.path.exists(path):
            for line in open(path):
                r = json.loads(line)
                yield "codex " + r["name"], r["fmt"], r["opts"], r["doc"]["blocks"]
        path = os.path.join(d, "witnesses.json")
        if os.path.exists(path):
            for name, w in json.load(open(path)).items():
                doc = w.get("doc") or w.get("input")
                if isinstance(doc, dict) and "blocks" in doc:
                    yield "codex " + name, w.get("fmt", "html"), w.get("opts", {}), doc["blocks"]
        for f in sorted(os.listdir(d)) if os.path.isdir(d) else []:
            if f.startswith("diffs-") and f.endswith(".json"):
                data = json.load(open(os.path.join(d, f)))
                if isinstance(data, list):
                    for item in data:
                        if isinstance(item.get("doc"), dict):
                            yield f"opus6 {f[:-5]} {item['id']}", item["fmt"], item["opts"], item["doc"]["blocks"]
                    continue
                for item in data.get("diffs", []) + data.get("wdiffs", []):
                    yield "opus " + item[0], item[2], item[3], item[1]


SUPPORTED = {"incremental", "section_divs", "number_sections", "number_offset", "refloc", "wrap", "columns", "id_prefix",
             "q_tags", "ascii", "email"}


# Review inputs that show a recorded row of `docs/ISSUES.md`, not a rule of this matrix.
KNOWN = {"opus wctx-entry-notepara": "I-HTML-29", "opus6 diffs-t9 ": "I-HTML-31", "opus6 diffs-t10 ": "I-HTML-31",
         "opus6 diffs-t6-2 rand2-40": "I-HTML-31", "opus6 diffs-t6-2 rand2-110": "I-HTML-29",
         "opus6 diffs-t6-2 rand2-526": "I-HTML-29", "opus6 diffs-t6-3 ": "I-HTML-32",
         "opus6 diffs-t3 ": "randoc-cli: --number-offset alone"}
SCOPE_GAP = re.compile(r'"Math"|"t": "Code(Block)?", "c": \[\["(?:[^"\\]|\\.)*", \["')  # I-HTML-01, I-HTML-02


def matrix(dirs):
    seen = set()
    rows = ([case_of(c) for c in pairwise()] + [case_of(c) for c in products()] + list(incremental_cases())
            + list(round6_cases()) + list(reviews(dirs)))
    for name, fmt, opts, blocks in rows:
        if not set(opts) <= SUPPORTED or SCOPE_GAP.search(json.dumps(blocks)):
            continue
        if any(name.startswith(prefix) for prefix in KNOWN):
            continue
        key = json.dumps([fmt, opts, blocks], sort_keys=True)
        if key not in seen:
            seen.add(key)
            yield name, fmt, opts, blocks


def record(dirs):
    cases = list(matrix(dirs))
    outs = oracle_batch([(n, doc_of(b), f, o) for n, f, o, b in cases], chunk=4000)
    with open(DATA, "w") as f:
        for (name, fmt, opts, blocks), out in zip(cases, outs):
            if out is not None:
                f.write(json.dumps([name, fmt, opts, blocks, out], ensure_ascii=False, separators=(",", ":")) + "\n")
    print(f"{len(cases)} cases recorded")


def add(dirs):
    known = {json.dumps(json.loads(line)[1:4], sort_keys=True) for line in open(DATA)}
    cases = [c for c in matrix(dirs) if json.dumps([c[1], c[2], c[3]], sort_keys=True) not in known]
    outs = oracle_batch([(n, doc_of(b), f, o) for n, f, o, b in cases], chunk=4000)
    with open(DATA, "a") as f:
        for (name, fmt, opts, blocks), out in zip(cases, outs):
            if out is not None:
                f.write(json.dumps([name, fmt, opts, blocks, out], ensure_ascii=False, separators=(",", ":")) + "\n")
    print(f"{len(cases)} cases added")


if __name__ == "__main__":
    mode = sys.argv[1] if len(sys.argv) > 1 else "check"
    if mode == "record":
        record(sys.argv[2:])
    elif mode == "add":
        add(sys.argv[2:])
    else:
        replay_check(DATA, mode == "show")
