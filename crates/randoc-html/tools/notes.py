#!/usr/bin/env python3
"""Compares the placement of notes with the oracle for `--reference-location`
section and block, with and without `--section-divs`: a heading at the top
level, in one div and in nested divs; with and without a block that is not a
heading; with one and two notes; a note in the heading, before, after or outside
the section (the last one is the case of I-HTML-19).

The container matrix (`containers`) puts a note in each container kind: a
plain div, a div with the class `section`, a block quote, an item of a bullet
list, of an ordered list and of a definition list, a table cell and a figure;
alone and inside each other kind; at the top and after a level-1 heading; with
a heading before or after the note; with `--reference-location` block and
section; with and without `--section-divs`. `record` writes the oracle output
to `src/notes.jsonl`. The unit test
`html::tests::the_oracle_matrices_replay_with_no_difference` replays the file.

Usage:
  python3 tools/notes.py [show]         # the heading matrix against the oracle
  python3 tools/notes.py record         # runs the oracle on the container matrix
  python3 tools/notes.py check          # compares the writer with the data file
  python3 tools/notes.py check show     # as check, and prints 12 differences
"""
import itertools, json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from differential import compare, doc_of, oracle_batch, replay_check, S, A

DATA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src", "notes.jsonl")


def note(text):
    return {"t": "Note", "c": [{"t": "Para", "c": [S(text)]}]}


def cases():
    out = []
    for refloc, divs, place, content, notes, where in itertools.product(
            ["section", "block"], [False, True], ["top", "div", "nested"], [False, True], [1, 2],
            ["heading", "before", "after", "outside"]):
        heading = {"t": "Header", "c": [2, A, [S("h")] + ([note("hn")] if where == "heading" else [])]}
        body = [heading]
        if content:
            body.append({"t": "Para", "c": [S("p")] + ([note("pn")] if where != "before" else [])})
        if notes == 2:
            body.append({"t": "Para", "c": [S("q"), note("qn")]})
        if place == "div":
            body = [{"t": "Div", "c": [["d", [], []], body]}]
        elif place == "nested":
            body = [{"t": "Div", "c": [A, [{"t": "Div", "c": [["d", [], []], body]}]]}]
        blocks = ([{"t": "Para", "c": [S("a"), note("an")]}] if where == "before" else []) + body
        if where == "outside":
            blocks.append({"t": "BulletList", "c": [[{"t": "Para", "c": [S("o"), note("on")]}]]})
        opts = {"refloc": refloc}
        if divs:
            opts["section_divs"] = True
        out.append((f"{refloc} divs={divs} {place} content={content} notes={notes} {where}", doc_of(blocks), "html", opts))
    return out


def containers():
    para = lambda text, *more: {"t": "Para", "c": [S(text), *more]}
    head = lambda level, *more: {"t": "Header", "c": [level, A, [S("h"), *more]]}
    cell = lambda blocks: [A, {"t": "AlignDefault"}, 1, 1, blocks]
    row = lambda blocks: [A, [cell(blocks)]]
    kinds = {
        "div": lambda b: {"t": "Div", "c": [A, b]},
        "div-id": lambda b: {"t": "Div", "c": [["d", ["c"], []], b]},
        "section": lambda b: {"t": "Div", "c": [["", ["section"], []], b]},
        "quote": lambda b: {"t": "BlockQuote", "c": b},
        "bullet": lambda b: {"t": "BulletList", "c": [b]},
        "ordered": lambda b: {"t": "OrderedList", "c": [[1, {"t": "Decimal"}, {"t": "Period"}], [b]]},
        "definition": lambda b: {"t": "DefinitionList", "c": [[[S("t")], [b]]]},
        "cell": lambda b: {"t": "Table", "c": [A, [None, []], [[{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]],
                                               [A, []], [[A, 0, [], [row(b)]]], [A, []]]},
        "figure": lambda b: {"t": "Figure", "c": [A, [None, []], b]},
    }
    contents = {
        "note": [para("q", note("n"))],
        "note-para": [para("q", note("n")), para("p")],
        "two-notes": [para("q", note("n")), para("r", note("m"))],
        "heading-note": [head(2), para("q", note("n"))],
        "note-heading": [para("q", note("n")), head(1)],
        "note-in-heading": [head(1, note("n")), para("p")],
        "heading-only": [head(2)],
    }
    wraps = {name: (lambda b, f=f: [f(b)]) for name, f in kinds.items()}
    for (outer, f), (inner, g) in itertools.product(kinds.items(), kinds.items()):
        wraps[f"{outer}>{inner}"] = lambda b, f=f, g=g: [f([g(b)])]
    places = {
        "top": lambda b: b,
        "after-h1": lambda b: [head(1)] + b + [para("z")],
        "note-before": lambda b: [para("a", note("an"))] + b,
    }
    options = [{"refloc": r, **d} for r in ("block", "section") for d in ({}, {"section_divs": True})]
    for (wn, wrap), (cn, content), (pn, place), opts in itertools.product(
            wraps.items(), contents.items(), places.items(), options):
        formats = ["html", "html4"] if ">" not in wn else ["html"]
        for fmt in formats:
            yield f"notes {wn} {cn} {pn}", doc_of(place(wrap(content))), fmt, opts


def record():
    all_cases = list(containers())
    with open(DATA, "w") as f:
        for (name, doc, fmt, opts), out in zip(all_cases, oracle_batch(all_cases)):
            f.write(json.dumps([name, fmt, opts, doc["blocks"], out], ensure_ascii=False) + "\n")
    print(f"recorded {len(all_cases)} cases")


def main():
    if sys.argv[1:2] == ["record"]:
        return record()
    if sys.argv[1:2] == ["check"]:
        return replay_check(DATA, "show" in sys.argv)
    all_cases = cases()
    diffs = compare(all_cases)
    for (name, doc, fmt, opts), mine, other in diffs[: 10 if len(sys.argv) > 1 else 0]:
        print("===", name)
        print("R:", repr(mine.get("out", mine.get("error"))))
        print("O:", repr(other["out"]))
    print(f"{len(diffs)} differences in {len(all_cases)} cases")


if __name__ == "__main__":
    main()
