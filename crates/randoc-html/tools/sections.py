#!/usr/bin/env python3
"""The oracle matrix of section containers: a heading, or a div that starts
with a heading, in each combination of the attributes and options that take
part in the decision.

Axes of the full product (a div at the top, level 1, followed by a paragraph):
the format (`html`, `html4`); `--section-divs` and `--number-sections`; the
div identifier (none, its own, the identifier of the heading); the div
classes (none, `section`, other, both); the div pairs (none, a key, `role`);
the first block (a heading or a paragraph); the heading identifier, classes
(none, other, `section`) and pairs (none, the same key and value as the div,
the same key with a different value, `role`).

Axes of the nesting product, on a smaller set of attributes: heading levels
1 and 2; the div at the top, after a level-1 heading, inside a plain div
after a level-1 heading, with a second heading of the same level inside it,
and with a later div that starts with a heading of the same level.

The note cases place a note before, in and after sections in divs, with
`--reference-location=block`.

The quote cases put a div with the class `section` in a block quote and in
a list item, with a heading first or not.

The axes cases of review round 4 combine repeated heading classes, heading
roles, own `number` pairs, the key `id`, the heading identifier and a block
quote around the div.

The earlier axes of round 2 (headings with the class `section` in quotes,
list items and divs, before and after other blocks) are part of the matrix,
with and without `--section-divs`.

Usage:
  python3 tools/sections.py record   # runs the oracle and writes the data file
  python3 tools/sections.py record --only notes  # records only the note cases
  python3 tools/sections.py record --only quote  # records only the cases in quotes and items
  python3 tools/sections.py record --only axes   # records only the axes of review round 4
  python3 tools/sections.py check    # compares the writer with the data file
  python3 tools/sections.py show     # as check, and prints 12 differences

The data file `src/sections.jsonl` has one case on each line:
`[name, format, options, blocks, oracle output]`. The unit test
`html::tests::the_oracle_matrices_replay_with_no_difference` reads it.
"""
import itertools, json, os, subprocess, sys
from concurrent.futures import ThreadPoolExecutor
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from differential import doc_of, oracle, S, A

DATA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src", "sections.jsonl")
PARA = {"t": "Para", "c": [S("p")]}
OPTIONS = [{}, {"section_divs": True}, {"number_sections": True}, {"section_divs": True, "number_sections": True}]
FORMATS = ["html", "html4"]


def header(level, attr, text="h"):
    return {"t": "Header", "c": [level, attr, [S(text)]]}


def div(attr, blocks):
    return {"t": "Div", "c": [attr, blocks]}


def full_product():
    div_ids = ["", "d", "h"]
    div_classes = [[], ["section"], ["c"], ["section", "c"]]
    div_pairs = {"none": [], "kv": [["k", "v"]], "role": [["role", "r"]]}
    head_ids = ["", "h"]
    head_classes = [[], ["c"], ["section"]]
    head_pairs = {"none": [], "kv": [["k", "v"]], "kw": [["k", "w"]], "role": [["role", "r"]]}
    for did, dcl, (dpn, dp) in itertools.product(div_ids, div_classes, div_pairs.items()):
        dattr = [did, dcl, dp]
        dname = f"div({did}|{'.'.join(dcl)}|{dpn})"
        yield f"full {dname} para", [div(dattr, [PARA, PARA]), PARA]
        for hid, hcl, (hpn, hp) in itertools.product(head_ids, head_classes, head_pairs.items()):
            hname = f"h1({hid}|{'.'.join(hcl)}|{hpn})"
            yield f"full {dname} {hname}", [div(dattr, [header(1, [hid, hcl, hp]), PARA]), PARA]


def nesting_product():
    div_attrs = {"plain": A, "id": ["d", [], []], "class": ["", ["c"], []], "section": ["", ["section"], []],
                 "kv": ["", [], [["k", "v"]]], "same": ["h", ["c"], [["k", "v"]]]}
    head_attrs = {"plain": A, "id": ["h", [], []], "class-kv": ["h", ["c"], [["k", "v"]]],
                  "role": ["", [], [["role", "r"]]]}
    for (dn, dattr), (hn, hattr), level in itertools.product(div_attrs.items(), head_attrs.items(), [1, 2]):
        case = div(dattr, [header(level, hattr), PARA])
        name = f"nest div({dn}) h{level}({hn})"
        yield f"{name} after-h1", [header(1, A, "a"), case]
        yield f"{name} in-div after-h1", [header(1, A, "a"), div(A, [case])]
        yield f"{name} in-div first", [div(["o", [], []], [case, PARA])]
        yield f"{name} two-sections", [div(dattr, [header(level, hattr), PARA, header(level, A, "b")])]
        yield f"{name} nested-two", [div(dattr, [header(level, hattr), div(A, [div(A, [header(level, A, "b")])])])]
        yield f"{name} deeper", [div(dattr, [header(level, hattr), header(level + 1, A, "s"), PARA])]
        yield f"{name} next-div", [case, div(A, [header(level, A, "b")]), PARA]


def round2_product():
    attrs = {"plain": ["x", [], []], "section": ["", ["section"], []],
             "section-id": ["x", ["section", "c"], []], "section-kv": ["x", ["section"], [["k", "v"]]]}
    for (aname, attr), level, text, before, after, place in itertools.product(
            attrs.items(), [1, 2], [True, False], ["none", "para", "heading"], ["none", "para", "lower", "equal"],
            ["top", "div", "div-attr", "quote", "item"]):
        blocks = []
        if before == "para":
            blocks.append(PARA)
        elif before == "heading":
            blocks.append(header(1, A, "b"))
        blocks.append({"t": "Header", "c": [level, attr, [S("h")] if text else []]})
        follow = {"none": None, "para": PARA, "lower": level + 1, "equal": level}[after]
        if isinstance(follow, int):
            blocks.append(header(follow, A, "f"))
        elif follow:
            blocks.append(follow)
        wrap = {"top": lambda b: b, "div": lambda b: [div(A, b)], "div-attr": lambda b: [div(["d", ["e"], []], b)],
                "quote": lambda b: [{"t": "BlockQuote", "c": b}], "item": lambda b: [{"t": "BulletList", "c": [b]}]}
        yield f"r2 {aname} l{level} text={text} before={before} after={after} {place}", wrap[place](blocks)


def notes_product():
    note = {"t": "Note", "c": [{"t": "Para", "c": [S("n")]}]}
    with_note = {"t": "Para", "c": [S("q"), note]}
    section = ["", ["section"], []]
    shapes = {
        "before-section": [div(A, [with_note, header(1, A), PARA])],
        "two-sections": [div(A, [header(1, A), with_note, header(1, A, "b")])],
        "section-class": [div(section, [with_note, PARA])],
        "after-inner-section": [div(A, [div(["d8", [], []], [header(1, A)]), with_note])],
        "plain": [div(A, [with_note]), PARA],
        "div-section": [div(A, [header(1, A), with_note]), PARA],
        "section-class-inner": [div(section, [div(A, [with_note]), PARA])],
        "after-plain": [header(1, A), div(A, [with_note, PARA]), PARA],
        "nested-section": [div(A, [div(A, [header(1, A), with_note]), PARA])],
        "heading-note": [div(["", ["c"], []], [{"t": "Header", "c": [1, A, [note]]}])],
        "nested-two-notes": [div(A, [div(["d", [], []], [header(2, A), with_note, with_note])])],
    }
    for name, blocks in shapes.items():
        yield f"notes {name}", blocks


def quote_product():
    quote = lambda blocks: {"t": "BlockQuote", "c": blocks}
    item = lambda blocks: {"t": "BulletList", "c": [blocks]}
    attrs = {"section": ["", ["section"], []], "section-class": ["", ["section", "q"], []],
             "class-section": ["d", ["q", "section"], []],
             "section-id": ["d", ["section", "q"], []], "section-kv": ["", ["section"], [["k", "v"]]],
             "class": ["", ["q"], []]}
    firsts = {"heading": [header(1, A), PARA], "heading-7": [header(7, A), PARA], "para": [PARA],
              "two": [header(1, A), header(1, A, "b")]}
    for (an, attr), (fn, blocks), (wn, wrap) in itertools.product(
            attrs.items(), firsts.items(), {"quote": quote, "item": item}.items()):
        yield f"quote {wn} div({an}) {fn}", [wrap([div(attr, blocks)])]


def axes_product():
    """The axes of review round 4: repeated heading classes, the roles that
    stay on a heading, own `number` pairs, the keys `id` and `data-number`,
    and a div with the class `section` in a block quote."""
    head_classes = {"none": [], "twice": ["c", "c"], "aba": ["a", "b", "a"]}
    head_pairs = {"none": [], "role-heading": [["role", "heading"]], "role-r": [["role", "r"]],
                  "number": [["number", "7"]], "id": [["id", "x"]]}
    div_classes = {"none": [], "c": ["c"], "section": ["section"]}
    div_pairs = {"none": [], "number": [["number", "3"]], "role-tab": [["role", "tab"]]}
    head_ids = ["", "d", "h"]
    places = {"top": lambda b: b, "quote": lambda b: [{"t": "BlockQuote", "c": b}]}
    for (hcn, hc), (hpn, hp), (dcn, dc), (dpn, dp), hid, (pn, place) in itertools.product(
            head_classes.items(), head_pairs.items(), div_classes.items(), div_pairs.items(), head_ids,
            places.items()):
        blocks = [div(["d", dc, dp], [header(1, [hid, hc, hp]), PARA])]
        yield f"axes div({dcn}|{dpn}) h1({hid}|{hcn}|{hpn}) {pn}", place(blocks)


def cases():
    out = []
    notes_options = [dict(o, refloc="block") for o in OPTIONS[:2]]
    products = [(full_product(), OPTIONS), (nesting_product(), OPTIONS), (round2_product(), OPTIONS[:2]),
                (notes_product(), notes_options), (quote_product(), OPTIONS), (axes_product(), OPTIONS)]
    if "--only" in sys.argv:
        wanted = sys.argv[sys.argv.index("--only") + 1]
        products = [products[{"notes": 3, "quote": 4, "axes": 5}[wanted]]]
    for product, options in products:
        for name, blocks in product:
            for fmt, opts in itertools.product(FORMATS, options):
                out.append((name, doc_of(blocks), fmt, opts))
    return out


def record():
    all_cases = cases()
    with ThreadPoolExecutor(2) as pool:
        results = list(pool.map(lambda c: oracle(c[1], c[2], c[3]), all_cases))
    names = {name for name, _, _, _ in all_cases}
    kept = [line for line in open(DATA) if json.loads(line)[0] not in names] if os.path.exists(DATA) else []
    with open(DATA, "w") as f:
        f.writelines(kept)
        for (name, doc, fmt, opts), result in zip(all_cases, results):
            if result is None or result["err"]:
                print("no result:", name, fmt, opts, result)
                continue
            f.write(json.dumps([name, fmt, opts, doc["blocks"], result["out"]], ensure_ascii=False) + "\n")
    print(f"recorded {len(all_cases)} cases")


def check(show):
    recorded = [json.loads(line) for line in open(DATA)]
    subprocess.run(["cargo", "build", "-q", "-p", "randoc-html", "--example", "runner"],
                   cwd=os.path.join(os.path.dirname(DATA), "..", "..", ".."), check=True)
    from differential import RUNNER
    runner = subprocess.Popen([RUNNER], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    diffs = []
    for name, fmt, opts, blocks, expected in recorded:
        runner.stdin.write(json.dumps({"doc": doc_of(blocks), "fmt": fmt, "opts": opts}) + "\n")
        runner.stdin.flush()
        mine = json.loads(runner.stdout.readline())
        if mine.get("out") != expected or mine.get("err"):
            diffs.append((name, fmt, opts, mine, expected))
    runner.stdin.close()
    runner.wait()
    if os.environ.get("DUMP"):
        with open(os.environ["DUMP"], "w") as f:
            json.dump(diffs, f)
    for name, fmt, opts, mine, expected in diffs[: 12 if show else 0]:
        print("===", name, fmt, opts)
        print("R:", repr(mine.get("out", mine.get("error"))), repr(mine.get("err", "")))
        print("O:", repr(expected))
    print(f"{len(diffs)} differences in {len(recorded)} cases")


if __name__ == "__main__":
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    if mode == "record":
        record()
    elif mode in ("check", "show"):
        check(mode == "show")
    else:
        print(__doc__)
