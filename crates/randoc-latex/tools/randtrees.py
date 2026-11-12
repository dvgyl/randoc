#!/usr/bin/env python3
"""Compares Randoc with the oracle on random document trees.

Usage (from the repository root):

    RANDOC_ORACLE_PANDOC=... python3 crates/randoc-latex/tools/randtrees.py \
        --seed 7 --count 200 --out DIR [--depth 3] [--formats latex beamer] \
        [--highlight off|only] [--option --wrap=none] [--reduce]

`--highlight off` (the default) leaves out the constructs whose output needs
the syntax highlighter (I-LATEX-01, I-LATEX-02, I-LATEX-08): inline code with
`numberLines`. `--highlight only` makes trees of only these constructs.
`--option` passes one writer option to both programs. Then Randoc runs as the
example program `target/debug/examples/latex_write` (build it with
`cargo build --example latex_write -p randoc-latex`). `--reduce` makes the
smallest tree that still differs, for the first tree of each class of
difference, and writes it as CLASS-N.min.json.
"""
import json
import os
import random
import re
import subprocess
import sys

ORACLE = os.environ["RANDOC_ORACLE_PANDOC"]
RANDOC = "target/debug/randoc"

WORDS = ["a", "b-c", "x_y", "50%", "#1", "{z}", "it's", "“q”", "—", "eé", "~", "[k]", "\\v"]


HIGHLIGHT = "off"


def attr(rng, code=False):
    ident = rng.choice(["", "", "", "id1", "a b"])
    names = ["c", "smallcaps", "underline", "mark", "unnumbered", "numberLines",
             "fragile", "allowframebreaks", "incremental", "columns", "column", "notes"]
    if code and HIGHLIGHT == "off":
        names.remove("numberLines")
    classes = rng.sample(names, rng.randint(0, 2))
    attrs = rng.sample([["lang", "fr"], ["dir", "rtl"], ["width", "50%"], ["startFrom", "3"],
                        ["height", "2cm"]], rng.randint(0, 1))
    return [ident, classes, attrs]


def inlines(rng, depth):
    return [inline(rng, depth) for _ in range(rng.randint(1, 4))]


def inline(rng, depth):
    leaf = [
        lambda: {"t": "Str", "c": rng.choice(WORDS)},
        lambda: {"t": "Space"},
        lambda: {"t": "SoftBreak"},
        lambda: {"t": "LineBreak"},
        lambda: {"t": "Code", "c": [attr(rng, code=True), rng.choice(["x", "a b", "{%}", "p`q"])]},
        lambda: {"t": "Math", "c": [{"t": rng.choice(["InlineMath", "DisplayMath"])}, "x^2"]},
        lambda: {"t": "RawInline", "c": [rng.choice(["latex", "html", "tex"]), "\\raw"]},
    ]
    if depth <= 0:
        return rng.choice(leaf)()
    nested = [
        lambda: {"t": rng.choice(["Emph", "Strong", "Underline", "Strikeout", "Superscript", "Subscript",
                                   "SmallCaps"]), "c": inlines(rng, depth - 1)},
        lambda: {"t": "Quoted", "c": [{"t": rng.choice(["SingleQuote", "DoubleQuote"])}, inlines(rng, depth - 1)]},
        lambda: {"t": "Link", "c": [attr(rng), inlines(rng, depth - 1), [rng.choice(["http://x.org/a_b", "#id1", "mailto:m@x.org"]), ""]]},
        lambda: {"t": "Image", "c": [attr(rng), inlines(rng, depth - 1), [rng.choice(["a.png", "b.svg"]), ""]]},
        lambda: {"t": "Span", "c": [attr(rng), inlines(rng, depth - 1)]},
        lambda: {"t": "Note", "c": blocks(rng, depth - 1)},
    ]
    return rng.choice(leaf + nested)()


def blocks(rng, depth):
    return [block(rng, depth) for _ in range(rng.randint(1, 3))]


def cell(rng, depth):
    return [["", [], []], {"t": "AlignDefault"}, 1, 1, blocks(rng, depth)]


def table(rng, depth):
    columns = rng.randint(1, 2)
    width = rng.choice([{"t": "ColWidthDefault"}, {"t": "ColWidth", "c": 0.4}])
    specs = [[{"t": rng.choice(["AlignDefault", "AlignLeft", "AlignRight", "AlignCenter"])}, width]
             for _ in range(columns)]
    row = lambda: [["", [], []], [cell(rng, depth) for _ in range(columns)]]
    caption = rng.choice([[], [{"t": "Plain", "c": inlines(rng, depth)}]])
    head = rng.choice([[], [row()]])
    return {"t": "Table", "c": [attr(rng), [None, caption], specs, [["", [], []], head],
                                [[["", [], []], 0, [], [row()]]], [["", [], []], []]]}


def block(rng, depth):
    leaf = [
        lambda: {"t": "Para", "c": inlines(rng, depth)},
        lambda: {"t": "Plain", "c": inlines(rng, depth)},
        lambda: {"t": "Header", "c": [rng.randint(1, 4), attr(rng), inlines(rng, depth)]},
        lambda: {"t": "CodeBlock", "c": [attr(rng), rng.choice(["x", "a\nb\n", "\\end{verbatim}"])]},
        lambda: {"t": "RawBlock", "c": [rng.choice(["latex", "html"]), "\\raw"]},
        lambda: {"t": "HorizontalRule"},
        lambda: {"t": "LineBlock", "c": [inlines(rng, depth), inlines(rng, depth)]},
    ]
    if depth <= 0:
        return rng.choice(leaf)()
    nested = [
        lambda: {"t": "BlockQuote", "c": blocks(rng, depth - 1)},
        lambda: {"t": "BulletList", "c": [blocks(rng, depth - 1) for _ in range(rng.randint(1, 2))]},
        lambda: {"t": "OrderedList", "c": [[rng.randint(1, 3), {"t": rng.choice(["Decimal", "LowerAlpha", "DefaultStyle"])},
                                            {"t": rng.choice(["Period", "OneParen", "DefaultDelim"])}],
                                           [blocks(rng, depth - 1) for _ in range(rng.randint(1, 2))]]},
        lambda: {"t": "DefinitionList", "c": [[inlines(rng, depth - 1), [blocks(rng, depth - 1)]]]},
        lambda: {"t": "Div", "c": [attr(rng), blocks(rng, depth - 1)]},
        lambda: {"t": "Figure", "c": [attr(rng), [None, [{"t": "Plain", "c": inlines(rng, depth - 1)}]], blocks(rng, depth - 1)]},
        lambda: table(rng, depth - 1),
    ]
    return rng.choice(leaf + nested)()


# The realistic generator: the weights follow what a reader produces. Most
# attributes are empty, table cells and captions hold short inline text, a
# figure holds an image, and notes hold paragraphs.
ONLY = None


def allowed(kind):
    return ONLY is None or kind in ONLY or kind in ("Str", "Space", "Para", "Plain")


def pick(rng, options):
    options = [(weight, kind, make) for weight, kind, make in options if allowed(kind)]
    total = sum(weight for weight, _, _ in options)
    point = rng.random() * total
    for weight, _, make in options:
        point -= weight
        if point <= 0:
            return make()
    return options[-1][2]()


def real_attr(rng, kinds=("c",)):
    if rng.random() < 0.75:
        return ["", [], []]
    ident = rng.choice(["", "sec-a", "fig1"])
    classes = rng.sample(list(kinds), rng.randint(0, min(1, len(kinds))))
    return [ident, classes, []]


def real_inlines(rng, depth, budget):
    out = []
    for _ in range(rng.randint(1, 5)):
        if budget[0] <= 0:
            break
        out.append(real_inline(rng, depth, budget))
        if rng.random() < 0.6:
            out.append({"t": "Space"})
    return out or [{"t": "Str", "c": "w"}]


def real_inline(rng, depth, budget):
    budget[0] -= 1
    word = lambda: {"t": "Str", "c": rng.choice(["word", "text", "x_y", "50%", "it's", "\u201cq\u201d", "a.", "#1"])}
    options = [
        (40, "Str", word),
        (4, "SoftBreak", lambda: {"t": "SoftBreak"}),
        (2, "LineBreak", lambda: {"t": "LineBreak"}),
        (5, "Code", lambda: {"t": "Code", "c": [["", [], []], rng.choice(["x", "a b", "{%}"])]}),
        (4, "Math", lambda: {"t": "Math", "c": [{"t": rng.choice(["InlineMath", "InlineMath", "DisplayMath"])}, "x^2"]}),
        (1, "RawInline", lambda: {"t": "RawInline", "c": [rng.choice(["latex", "html"]), "\\raw"]}),
    ]
    if depth > 0:
        sub = lambda: real_inlines(rng, depth - 1, budget)
        options += [
            (8, "Emph", lambda: {"t": rng.choice(["Emph", "Strong"]), "c": sub()}),
            (2, "Strikeout", lambda: {"t": rng.choice(["Strikeout", "Superscript", "Subscript", "SmallCaps", "Underline"]), "c": sub()}),
            (4, "Quoted", lambda: {"t": "Quoted", "c": [{"t": rng.choice(["SingleQuote", "DoubleQuote"])}, sub()]}),
            (5, "Link", lambda: {"t": "Link", "c": [["", [], []], sub(), [rng.choice(["http://x.org/a_b", "#sec-a", "mailto:m@x.org"]), ""]]}),
            (2, "Image", lambda: {"t": "Image", "c": [["", [], []], sub(), ["a.png", ""]]}),
            (2, "Span", lambda: {"t": "Span", "c": [real_attr(rng, ("smallcaps", "mark", "c")), sub()]}),
            (3, "Note", lambda: {"t": "Note", "c": [{"t": "Para", "c": sub()}]}),
        ]
    return pick(rng, options)


def real_blocks(rng, depth, budget, count=3):
    out = []
    for _ in range(rng.randint(1, count)):
        if budget[0] <= 0:
            break
        out.append(real_block(rng, depth, budget))
    return out or [{"t": "Para", "c": [{"t": "Str", "c": "w"}]}]


def real_block(rng, depth, budget):
    budget[0] -= 1
    para = lambda: {"t": "Para", "c": real_inlines(rng, depth, budget)}
    options = [
        (35, "Para", para),
        (8, "Header", lambda: {"t": "Header", "c": [rng.choice([1, 2, 2, 3]), real_attr(rng, ("unnumbered",)), real_inlines(rng, min(depth, 1), budget)]}),
        (5, "CodeBlock", lambda: {"t": "CodeBlock", "c": [["", [], []], rng.choice(["x", "a\nb"])]}),
        (2, "HorizontalRule", lambda: {"t": "HorizontalRule"}),
        (2, "RawBlock", lambda: {"t": "RawBlock", "c": [rng.choice(["latex", "html"]), "\\raw"]}),
        (2, "LineBlock", lambda: {"t": "LineBlock", "c": [real_inlines(rng, 0, budget), real_inlines(rng, 0, budget)]}),
        (3, "Figure", lambda: {"t": "Figure", "c": [real_attr(rng), [None, [{"t": "Plain", "c": real_inlines(rng, min(depth, 1), budget)}]],
                                                   [{"t": "Plain", "c": [{"t": "Image", "c": [["", [], []], [{"t": "Str", "c": "w"}], ["a.png", ""]]}]}]]}),
        (3, "Table", lambda: real_table(rng, budget)),
    ]
    if depth > 0:
        sub = lambda: real_blocks(rng, depth - 1, budget, 2)
        tight = lambda: [{"t": "Plain", "c": real_inlines(rng, depth - 1, budget)}]
        options += [
            (5, "BlockQuote", lambda: {"t": "BlockQuote", "c": sub()}),
            (6, "BulletList", lambda: {"t": "BulletList", "c": [rng.choice([tight, sub])() for _ in range(rng.randint(1, 3))]}),
            (4, "OrderedList", lambda: {"t": "OrderedList", "c": [[1, {"t": rng.choice(["Decimal", "LowerAlpha"])}, {"t": rng.choice(["Period", "OneParen"])}],
                                                                  [rng.choice([tight, sub])() for _ in range(rng.randint(1, 3))]]}),
            (3, "DefinitionList", lambda: {"t": "DefinitionList", "c": [[real_inlines(rng, 0, budget), [rng.choice([tight, sub])()]]]}),
            (3, "Div", lambda: {"t": "Div", "c": [real_attr(rng, ("c", "notes")), sub()]}),
        ]
    return pick(rng, options)


def real_table(rng, budget):
    columns = rng.randint(1, 3)
    width = rng.choice([{"t": "ColWidthDefault"}, {"t": "ColWidthDefault"}, {"t": "ColWidth", "c": round(0.9 / columns, 4)}])
    specs = [[{"t": rng.choice(["AlignDefault", "AlignLeft", "AlignRight", "AlignCenter"])}, width] for _ in range(columns)]
    cell = lambda: [["", [], []], {"t": "AlignDefault"}, 1, 1, [{"t": "Plain", "c": real_inlines(rng, 1, budget)}]]
    row = lambda: [["", [], []], [cell() for _ in range(columns)]]
    caption = rng.choice([[], [{"t": "Plain", "c": real_inlines(rng, 1, budget)}]])
    head = rng.choice([[], [row()]])
    return {"t": "Table", "c": [["", [], []], [None, caption], specs, [["", [], []], head],
                                [[["", [], []], 0, [], [row() for _ in range(rng.randint(1, 2))]]], [["", [], []], []]]}


def tree_words(node, out):
    if isinstance(node, dict):
        if node.get("t") in ("Str", "Code", "Math") and isinstance(node.get("c"), (str, list)):
            text = node["c"] if isinstance(node["c"], str) else node["c"][-1]
            out.update(re.findall(r"[A-Za-z]+", text))
        for value in node.values():
            tree_words(value, out)
    elif isinstance(node, list):
        if len(node) == 3 and isinstance(node[0], str) and node[0]:
            out.update(re.findall(r"[A-Za-z]+", node[0]))
        for value in node:
            tree_words(value, out)


def signature(line, words):
    line = re.sub(r"[A-Za-z]+", lambda m: "W" if m.group(0) in words else m.group(0), line)
    return re.sub(r"[0-9]+", "N", line)


def first_pair(fmt, doc, options, randoc):
    a = run(ORACLE, fmt, doc, options)[1].split("\n")
    b = run(randoc, fmt, doc, options)[1].split("\n")
    for line_a, line_b in zip(a + [""] * len(b), b + [""] * len(a)):
        if line_a != line_b:
            return line_a.strip(), line_b.strip()
    return "", ""


def highlight_blocks(rng):
    languages = ["haskell", "python", "c", "zzzz", "numberLines"]
    out = []
    for _ in range(rng.randint(1, 3)):
        classes = rng.sample(languages, rng.randint(1, 2))
        code = rng.choice(["x = 1", "a `b` c", "main = print 1", "{%}"])
        if rng.random() < 0.5:
            out.append({"t": "Para", "c": [{"t": "Code", "c": [["", classes, []], code]}]})
        else:
            out.append({"t": "CodeBlock", "c": [["", classes, []], code]})
    return out


def run(program, fmt, doc, options):
    args = [program, "-f", "json", "-t", fmt, *options] if program == ORACLE else (
        [program, "-t", fmt, *options] if options else [program, "-f", "json", "-t", fmt])
    try:
        result = subprocess.run(args, input=doc, capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired:
        return None
    return (result.returncode, result.stdout.rstrip("\n"))


def differs(fmt, doc, options, randoc):
    expected = run(ORACLE, fmt, doc, options)
    if expected is None:
        return False, None
    return run(randoc, fmt, doc, options) != expected, expected


def first_difference(fmt, doc, options, randoc):
    a = run(ORACLE, fmt, doc, options)[1].split("\n")
    b = run(randoc, fmt, doc, options)[1].split("\n")
    for line_a, line_b in zip(a + [""] * len(b), b + [""] * len(a)):
        if line_a != line_b:
            return line_a
    return ""


def candidates(node):
    """Smaller trees: one list element removed, or a node replaced by a child."""
    if isinstance(node, list):
        for index in range(len(node)):
            yield node[:index] + node[index + 1:]
        for index, item in enumerate(node):
            for smaller in candidates(item):
                yield node[:index] + [smaller] + node[index + 1:]
    elif isinstance(node, dict):
        content = node.get("c")
        if isinstance(content, list):
            for part in content:
                if isinstance(part, list) and part and isinstance(part[0], dict) and "t" in part[0]:
                    for child in part:
                        if child.get("t") in BLOCKS if node.get("t") in BLOCKS else child.get("t") not in BLOCKS:
                            yield child
            for smaller in candidates(content):
                yield {"t": node["t"], "c": smaller}


BLOCKS = {"Plain", "Para", "LineBlock", "CodeBlock", "RawBlock", "BlockQuote", "OrderedList",
          "BulletList", "DefinitionList", "Header", "HorizontalRule", "Table", "Figure", "Div"}


def wrap(blocks_):
    return json.dumps({"pandoc-api-version": [1, 23, 1, 2], "meta": {}, "blocks": blocks_})


def reduce(fmt, blocks_, options, randoc):
    """The smallest tree that keeps the first differing line of the oracle output."""
    key = first_difference(fmt, wrap(blocks_), options, randoc).strip()

    def bad(candidate):
        doc = wrap(candidate)
        bad_, expected = differs(fmt, doc, options, randoc)
        # A candidate that the oracle rejects is not a valid document tree.
        if not bad_ or expected is None or expected[0] != 0:
            return False
        return first_difference(fmt, doc, options, randoc).strip() == key
    current = blocks_
    changed = True
    while changed:
        changed = False
        for smaller in candidates(current):
            if isinstance(smaller, list) and smaller != current and len(json.dumps(smaller)) < len(json.dumps(current)) and bad(smaller):
                current = smaller
                changed = True
                break
    return current


def main():
    global HIGHLIGHT
    import argparse
    from concurrent.futures import ThreadPoolExecutor
    parser = argparse.ArgumentParser()
    parser.add_argument("--seed", type=int, required=True)
    parser.add_argument("--count", type=int, required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--depth", type=int, default=3)
    parser.add_argument("--formats", nargs="+", default=["latex", "beamer"])
    parser.add_argument("--highlight", choices=["off", "only"], default="off")
    parser.add_argument("--option", action="append", default=[])
    parser.add_argument("--reduce", action="store_true")
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--mode", choices=["real", "exotic"], default="real")
    parser.add_argument("--small", action="store_true", help="depth 2 and at most 6 nodes")
    parser.add_argument("--only", default="", help="node types, separated by commas")
    parser.add_argument("--top", type=int, default=30)
    args = parser.parse_args()
    HIGHLIGHT = args.highlight
    global ONLY
    ONLY = set(args.only.split(",")) if args.only else None
    randoc = "target/debug/examples/latex_write" if args.option else RANDOC
    os.makedirs(args.out, exist_ok=True)
    rng = random.Random(args.seed)
    trees = []
    for _ in range(args.count):
        if HIGHLIGHT == "only":
            trees.append(highlight_blocks(rng))
        elif args.mode == "exotic":
            trees.append(blocks(rng, args.depth))
        elif args.small:
            trees.append(real_blocks(rng, 2, [6]))
        else:
            trees.append(real_blocks(rng, args.depth, [40], 6))
    jobs = [(index, fmt) for index in range(len(trees)) for fmt in args.formats]

    def check(job):
        index, fmt = job
        doc = json.dumps({"pandoc-api-version": [1, 23, 1, 2], "meta": {}, "blocks": trees[index]})
        bad, _ = differs(fmt, doc, args.option, randoc)
        return index, fmt, bad, doc

    differ = {fmt: 0 for fmt in args.formats}
    classes = {}
    with ThreadPoolExecutor(args.jobs) as pool:
        for index, fmt, bad, doc in pool.map(check, jobs):
            if not bad:
                continue
            differ[fmt] += 1
            with open(os.path.join(args.out, f"{args.seed}-{index}-{fmt}.json"), "w") as file:
                file.write(doc)
            words = set()
            tree_words(trees[index], words)
            pair = first_pair(fmt, doc, args.option, randoc)
            key = (fmt, signature(pair[0], words), signature(pair[1], words))
            classes.setdefault(key, []).append(index)
    print(f"{args.count} trees, mode {args.mode}, small {args.small}, options {args.option}: differences {differ}")
    ranked = sorted(classes.items(), key=lambda kv: -len(kv[1]))
    for number, ((fmt, oracle_line, randoc_line), indexes) in enumerate(ranked[:args.top]):
        smallest = min(indexes, key=lambda i: len(json.dumps(trees[i])))
        print(f"  {len(indexes):3} {fmt:6} oracle {oracle_line[:70]!r}")
        print(f"             randoc {randoc_line[:70]!r}  smallest tree {smallest}: {json.dumps(trees[smallest])[:240]}")
    return
    for number, ((fmt, line), indexes) in enumerate(sorted(classes.items(), key=lambda kv: -len(kv[1]))):
        print(f"  class {number}: {len(indexes)} {fmt} trees {indexes[:5]}, first oracle line {line!r}")
        if args.reduce:
            smallest = reduce(fmt, trees[indexes[0]], args.option, randoc)
            name = os.path.join(args.out, f"class-{number}-{fmt}.min.json")
            with open(name, "w") as file:
                file.write(json.dumps(smallest))
            print(f"    smallest: {json.dumps(smallest)[:300]}")


if __name__ == "__main__":
    main()
