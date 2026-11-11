#!/usr/bin/env python3
"""Compares the HTML writer with the Pandoc oracle on generated trees.

The script builds the example runner (`cargo build -p randoc-html --example
runner`), sends each tree to it, converts the same tree with the oracle and
the matching command-line options, and prints each difference.

Usage:
  python3 tools/differential.py random 500          # random trees, default options
  python3 tools/differential.py random-options 300  # random trees and random options
  python3 tools/differential.py wrap                # the matrix of spaces and breaks
  python3 tools/differential.py special 2000        # random trees with the special names, half with
                                                    # random options, through the batch oracle
Each mode takes an optional seed as the last argument.
"""
import json, os, random, subprocess, sys
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..")
P = os.environ["RANDOC_ORACLE_PANDOC"]
TARGET = os.environ.get("CARGO_TARGET_DIR", os.path.join(ROOT, "target"))
RUNNER = os.path.join(TARGET, "debug", "examples", "runner")


def flags(opts):
    out = []
    for key, value in opts.items():
        if key == "wrap":
            out.append(f"--wrap={value}")
        elif key == "columns":
            out.append(f"--columns={value}")
        elif key in ("section_divs", "number_sections", "ascii", "incremental") and value:
            out.append("--" + key.replace("_", "-"))
        elif key == "number_offset":
            out.append("--number-offset=" + ",".join(map(str, value)))
        elif key == "id_prefix":
            out.append(f"--id-prefix={value}")
        elif key == "q_tags" and value:
            out.append("--html-q-tags")
        elif key == "email":
            out.append(f"--email-obfuscation={value}")
        elif key == "refloc":
            out.append(f"--reference-location={value}")
        elif key == "math":
            out.append(f"--math-method={value}")
    return out


def oracle(doc, fmt, opts):
    try:
        r = subprocess.run([P, "-f", "json", "-t", fmt, *flags(opts)], input=json.dumps(doc).encode(),
                           capture_output=True, timeout=60)
    except subprocess.TimeoutExpired:
        return None
    err = "".join(l + "\n" for l in r.stderr.decode().splitlines() if l.startswith("[WARNING]"))
    return {"out": r.stdout.decode(), "err": err}


def compare(cases):
    """cases: list of (name, doc, fmt, opts). Returns the list of differences."""
    subprocess.run(["cargo", "build", "-q", "-p", "randoc-html", "--example", "runner"], cwd=ROOT, check=True)
    runner = subprocess.Popen([RUNNER], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    ours = []
    for _, doc, fmt, opts in cases:
        runner.stdin.write(json.dumps({"doc": doc, "fmt": fmt, "opts": opts}) + "\n")
        runner.stdin.flush()
        ours.append(json.loads(runner.stdout.readline()))
    runner.stdin.close()
    runner.wait()
    # Rule 18a of the standing orders: at most 2 oracle processes at the same time.
    with ThreadPoolExecutor(2) as pool:
        theirs = list(pool.map(lambda c: oracle(c[1], c[2], c[3]), cases))
    diffs = []
    for case, mine, other in zip(cases, ours, theirs):
        if other is None:
            continue
        if mine.get("out") != other["out"] or mine.get("err", "") != other["err"]:
            diffs.append((case, mine, other))
    return diffs


BATCH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "oracle_batch.lua")


def requests(cases):
    return [json.dumps({"doc": doc, "fmt": fmt, "opts": opts}) + "\n" for _, doc, fmt, opts in cases]


def oracle_batch(cases, chunk=10000):
    """Returns the oracle output of each case. One oracle process converts a
    chunk of cases (`pandoc lua oracle_batch.lua`), so a matrix needs seconds.
    The outputs are the bytes of the command line: the 14092 recorded cases
    of `sections.jsonl` are equal on both paths. Warnings are not captured."""
    lines = requests(cases)

    def run(start):
        r = subprocess.run([P, "lua", BATCH], input="".join(lines[start:start + chunk]).encode(),
                           capture_output=True, timeout=60)
        outs = [json.loads(line) for line in r.stdout.decode().splitlines()]
        if r.returncode != 0 or len(outs) != len(lines[start:start + chunk]):
            raise SystemExit(f"the oracle did not end: exit {r.returncode}, {r.stderr.decode()[:300]}")
        return [o.get("out") for o in outs]

    # Rule 18a of the standing orders: at most 2 oracle processes at the same time.
    with ThreadPoolExecutor(2) as pool:
        return [out for part in pool.map(run, range(0, len(lines), chunk)) for out in part]


def runner_batch(cases):
    """Returns the writer output of each case, or the error. The batch oracle
    gives no warnings, so the warnings of the writer are not compared here."""
    subprocess.run(["cargo", "build", "-q", "-p", "randoc-html", "--example", "runner"], cwd=ROOT, check=True)
    r = subprocess.run([RUNNER], input="".join(requests(cases)), capture_output=True, text=True, check=True)
    return [o["out"] if "out" in o else o for o in map(json.loads, r.stdout.splitlines())]


def replay_check(data, show):
    """Compares the writer with a data file of `[name, format, options, blocks, oracle output]` lines."""
    recorded = [json.loads(line) for line in open(data)]
    ours = runner_batch([(name, doc_of(blocks), fmt, opts) for name, fmt, opts, blocks, _ in recorded])
    diffs = [(case, mine) for case, mine in zip(recorded, ours) if mine != case[4]]
    if os.environ.get("DUMP"):
        with open(os.environ["DUMP"], "w") as f:
            json.dump(diffs, f)
    for (name, fmt, opts, blocks, expected), mine in diffs[: int(os.environ.get("SHOW", "12")) if show else 0]:
        print("===", name, fmt, opts)
        print(json.dumps(blocks, ensure_ascii=False)[:500])
        print("R:", repr(mine))
        print("O:", repr(expected))
    print(f"{len(diffs)} differences in {len(recorded)} cases")


def doc_of(blocks):
    return {"pandoc-api-version": [1, 23, 1, 2], "meta": {}, "blocks": blocks}


S = lambda t: {"t": "Str", "c": t}
SP = {"t": "Space"}
SB = {"t": "SoftBreak"}
LB = {"t": "LineBreak"}
A = ["", [], []]


def wrap_cases():
    """Each sequence of up to three separators between and around words, in
    inline elements and at the block edges, in each wrap mode."""
    seps = [[SP], [SP, SP], [SB], [SP, SB], [SB, SP], [SB, SB], [LB], [LB, SP], [SP, LB], [LB, SB], [SB, LB],
            [S("  ")], [S(" ")], [S("​"), SP], [SP, S(" "), SP]]
    cases = []
    for mode in ["none", "preserve", "auto"]:
        for i, sep in enumerate(seps):
            shapes = {
                "middle": [S("a"), *sep, S("b")],
                "start": [*sep, S("b")],
                "end": [S("a"), *sep],
                "in-emph": [S("a"), {"t": "Emph", "c": [*sep, S("b"), *sep]}, S("c")],
                "in-span": [{"t": "Span", "c": [A, [*sep, S("b")]]}, *sep, S("c")],
            }
            for shape, inlines in shapes.items():
                for block in ("Para", "Plain"):
                    blocks = [{"t": block, "c": inlines}]
                    if block == "Plain":
                        blocks = [{"t": "BulletList", "c": [[{"t": "Plain", "c": inlines}]]}]
                    opts = {"wrap": mode} if mode != "auto" else {"columns": 4}
                    cases.append((f"{mode}/{i}/{shape}/{block}", doc_of(blocks), "html", opts))
        cases.append((f"{mode}/str-runs", doc_of([{"t": "Para", "c": [S("a  b   c "), SP, S(" d"), LB, S(" e")]}]), "html",
                      {"wrap": mode} if mode != "auto" else {}))
        cases.append((f"{mode}/linebreak-end", doc_of([{"t": "Para", "c": [S("a"), LB]}, {"t": "Para", "c": [LB, S("b")]}]), "html",
                      {"wrap": mode} if mode != "auto" else {}))
    return cases


def random_inlines(rng, depth):
    out = []
    for _ in range(rng.randint(0, 6)):
        r = rng.random()
        if r < 0.3:
            out.append(S(rng.choice(["a", "b c", "word", "x  y", " ", "<&>", "é", "↩", "日本", "​"])))
        elif r < 0.45:
            out.append(rng.choice([SP, SB, LB]))
        elif depth > 0 and r < 0.6:
            kind = rng.choice(["Emph", "Strong", "Strikeout", "Superscript", "Subscript", "SmallCaps", "Underline"])
            out.append({"t": kind, "c": random_inlines(rng, depth - 1)})
        elif depth > 0 and r < 0.68:
            out.append({"t": "Span", "c": [random_attr(rng), random_inlines(rng, depth - 1)]})
        elif depth > 0 and r < 0.74:
            out.append({"t": "Link", "c": [random_attr(rng), random_inlines(rng, depth - 1),
                                            [rng.choice(["u", "#x", "mailto:a@b", ""]), rng.choice(["", "t"])]]})
        elif depth > 0 and r < 0.78:
            out.append({"t": "Image", "c": [random_attr(rng, image=True), random_inlines(rng, depth - 1), ["i.png", ""]]})
        elif r < 0.84:
            out.append({"t": "Code", "c": [random_attr(rng), rng.choice(["c", "a b", "<x>"])]})
        elif depth > 0 and r < 0.88:
            out.append({"t": "Note", "c": random_blocks(rng, depth - 1)})
        elif depth > 0 and r < 0.92:
            out.append({"t": "Quoted", "c": [{"t": rng.choice(["SingleQuote", "DoubleQuote"])}, random_inlines(rng, depth - 1)]})
        elif r < 0.96:
            out.append({"t": "RawInline", "c": [rng.choice(["html", "latex", "html4"]), "<b>"]})
        else:
            out.append({"t": "Cite", "c": [[{"citationId": "k", "citationPrefix": [], "citationSuffix": [],
                                             "citationMode": {"t": "NormalCitation"}, "citationNoteNum": 1,
                                             "citationHash": 0}], [S("[@k]")]]})
    return out


# The special names of `tools/classes.py` and `tools/joint.py`, for the mode `special`.
SPECIAL_CLASSES = ["section", "section", "column", "columns", "cell", "fragment", "csl-bib-body", "csl-entry",
                   "incremental", "nonincremental", "unnumbered", "unlisted", "csl-block", "csl-no-emph"]
SPECIAL_IDS = ["refs", "d"]
SPECIAL_PAIRS = [["wrapper", "1"], ["number", "7"], ["width", "40%"], ["role", "doc-x"], ["style", "s"]]
SPECIAL = False


def random_attr(rng, image=False):
    if rng.random() < (0.4 if SPECIAL else 0.6):
        return A
    ident = rng.choice(["", "i", "1a", "x y"] + (SPECIAL_IDS if SPECIAL else []))
    names = ["q1", "smallcaps", "mark", "underline", "kbd", "section", "unnumbered", "csl-no-emph"]
    classes = rng.sample(names + (SPECIAL_CLASSES if SPECIAL else []), rng.randint(0, 2))
    keys = ["k", "title", "role", "style", "lang", "data-x"] + (["width", "height", "alt"] if image else [])
    values = ["v", "50%", "2in", "10", "1.5cm"]
    pairs = [[rng.choice(keys), rng.choice(values)] for _ in range(rng.randint(0, 2))]
    if SPECIAL and rng.random() < 0.4:
        pairs.insert(rng.randint(0, len(pairs)), rng.choice(SPECIAL_PAIRS))
    return [ident, classes, pairs]


def random_cell(rng, depth):
    return [random_attr(rng) if rng.random() < 0.2 else A,
            {"t": rng.choice(["AlignDefault", "AlignLeft", "AlignRight", "AlignCenter"])},
            rng.choice([1, 1, 1, 2]), rng.choice([1, 1, 1, 2]),
            random_blocks(rng, depth - 1) if rng.random() < 0.7 else []]


def random_table(rng, depth):
    ncols = rng.randint(1, 3)
    specs = [[{"t": rng.choice(["AlignDefault", "AlignLeft", "AlignRight", "AlignCenter"])},
              rng.choice([{"t": "ColWidthDefault"}, {"t": "ColWidth", "c": rng.choice([0.25, 0.5, 0.3])}])]
             for _ in range(ncols)]
    rows = lambda n: [[A, [random_cell(rng, depth) for _ in range(rng.randint(0, ncols))]] for _ in range(n)]
    caption = [random_blocks(rng, 0)[0]] if rng.random() < 0.3 else []
    return {"t": "Table", "c": [A, [None, caption], specs, [A, rows(rng.randint(0, 1))],
                                [[A, rng.randint(0, 1), rows(rng.randint(0, 1)), rows(rng.randint(0, 2))]
                                 for _ in range(rng.randint(0, 2))], [A, rows(rng.randint(0, 1))]]}


def random_blocks(rng, depth):
    out = []
    for _ in range(rng.randint(1, 4)):
        r = rng.random()
        if r < 0.3:
            out.append({"t": "Para", "c": random_inlines(rng, depth)})
        elif r < 0.38:
            out.append({"t": "Plain", "c": random_inlines(rng, depth)})
        elif r < 0.5:
            out.append({"t": "Header", "c": [rng.choice([1, 1, 2, 2, 3, 0, 7]), random_attr(rng), random_inlines(rng, 0)]})
        elif depth > 0 and r < 0.58:
            out.append({"t": "Div", "c": [random_attr(rng), random_blocks(rng, depth - 1)]})
        elif depth > 0 and r < 0.63:
            out.append({"t": "BlockQuote", "c": random_blocks(rng, depth - 1)})
        elif depth > 0 and r < 0.69:
            out.append({"t": rng.choice(["BulletList", "OrderedList"]), "c": None})
            items = [random_blocks(rng, depth - 1) for _ in range(rng.randint(0, 2))]
            if out[-1]["t"] == "BulletList":
                out[-1]["c"] = items
            else:
                out[-1]["c"] = [[rng.choice([1, 3, 0]), {"t": rng.choice(["Decimal", "LowerAlpha", "DefaultStyle", "Example", "UpperRoman"])},
                                 {"t": rng.choice(["Period", "OneParen", "DefaultDelim"])}], items]
        elif depth > 0 and r < 0.73:
            out.append({"t": "DefinitionList", "c": [[random_inlines(rng, 0), [random_blocks(rng, depth - 1)]]]})
        elif depth > 0 and r < 0.79:
            out.append(random_table(rng, depth))
        elif depth > 0 and r < 0.83:
            out.append({"t": "Figure", "c": [random_attr(rng), [None, random_blocks(rng, 0) if rng.random() < 0.6 else []],
                                             random_blocks(rng, depth - 1)]})
        elif r < 0.87:
            out.append({"t": "CodeBlock", "c": [random_attr(rng), "a < b\n\n c"]})
        elif r < 0.91:
            out.append({"t": "RawBlock", "c": [rng.choice(["html", "latex"]), rng.choice(["<x>", "x\n", ""])]})
        elif r < 0.95:
            out.append({"t": "LineBlock", "c": [random_inlines(rng, 0) for _ in range(rng.randint(0, 2))]})
        else:
            out.append({"t": "HorizontalRule"})
    return out


def random_options(rng):
    opts = {}
    if rng.random() < 0.5:
        opts["wrap"] = rng.choice(["none", "preserve"])
    elif rng.random() < 0.5:
        opts["columns"] = rng.choice([10, 20, 40])
    for key in ["section_divs", "number_sections", "ascii", "q_tags"]:
        if rng.random() < 0.25:
            opts[key] = True
    if rng.random() < 0.2:
        opts["number_offset"] = [rng.choice([0, 5, 1]) for _ in range(rng.randint(1, 3))]
    if rng.random() < 0.2:
        opts["id_prefix"] = rng.choice(["p-", "1"])
    if rng.random() < 0.3:
        opts["refloc"] = rng.choice(["block", "section", "document"])
    if rng.random() < 0.3:
        opts["email"] = rng.choice(["references", "none"])
    if rng.random() < 0.3 and not SPECIAL:
        opts["math"] = "mathjax"
    if SPECIAL and rng.random() < 0.2:
        opts["incremental"] = True
    return opts


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "random"
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 100
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    rng = random.Random(seed)
    if mode == "special":
        global SPECIAL
        SPECIAL = True
        cases = [(f"special-{i}", doc_of(random_blocks(rng, 3)), rng.choice(["html", "html4"]),
                  random_options(rng) if i % 2 else {}) for i in range(count)]
        ours = runner_batch(cases)
        theirs = oracle_batch(cases, chunk=500)
        # The batch path can keep a final line end that the command line drops,
        # so each batch difference runs again through the command line.
        suspects = [c for c, m, o in zip(cases, ours, theirs) if o is not None and m != o]
        diffs = compare(suspects) if suspects else []
    elif mode == "wrap":
        cases = wrap_cases()
    else:
        cases = []
        for i in range(count):
            blocks = random_blocks(rng, 2)
            fmt = rng.choice(["html", "html4"]) if mode == "random-options" else rng.choice(["html", "html4"])
            opts = random_options(rng) if mode == "random-options" else {}
            cases.append((f"{mode}-{i}", doc_of(blocks), fmt, opts))
    # Math needs a TeX converter (I-HTML-01); leave math out of the trees.
    if mode != "special":
        diffs = compare(cases)
    if os.environ.get("DUMP"):
        with open(os.environ["DUMP"], "w") as f:
            json.dump([{"name": c[0], "doc": c[1], "fmt": c[2], "opts": c[3], "mine": m, "oracle": o} for c, m, o in diffs], f)
    for (name, doc, fmt, opts), mine, other in diffs[: int(os.environ.get("SHOW", "20"))]:
        print("===", name, fmt, opts)
        print(json.dumps(doc["blocks"], ensure_ascii=False)[:600])
        print("R:", repr(mine.get("out", mine.get("error")))[:900], repr(mine.get("err", "")))
        print("O:", repr(other["out"])[:900], repr(other["err"]))
    print(f"{len(diffs)} differences in {len(cases)} cases")


if __name__ == "__main__":
    main()
