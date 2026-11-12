#!/usr/bin/env python3
"""The oracle matrix of math in the HTML writer.

Axes: the math method (`plain`, `mathml`, `mathjax`, `katex`, `webtex`,
`gladtex`), inline and display math, formulas of each class of
`randoc-math` (also formulas that it cannot convert), the place (a
paragraph, a heading, a table cell) and `html`/`html4`. `record` writes
`src/math.jsonl`; the unit test
`html::tests::the_oracle_matrices_replay_with_no_difference` replays it.

Usage:
  python3 tools/math_matrix.py record
  python3 tools/math_matrix.py check
"""
import itertools, json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from differential import doc_of, oracle_batch, replay_check, S, A

DATA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src", "math.jsonl")

FORMULAS = [
    "x^2", "x_i", "x_i^2", r"\frac{a}{b}", r"\sqrt{x}", r"\sqrt[3]{x}", r"\alpha+\beta", r"\sum_{i=1}^n i",
    r"\int_0^1 f(x)\,dx", r"\mathbf{v}", r"\mathrm{d}x", r"\left(\frac{a}{b}\right)", "a < b", r"a \le b",
    r"\{x\} & y", r"\hat{x}", r"\bar{y}", r"\vec{v}", r"\text{if } x", r"\lim_{x\to 0}", r"\sin x", r"\log_2 n",
    "1.5", "x'", r"\infty", r"\pm 1", r"a \cdot b", r"\begin{matrix}a&b\\c&d\end{matrix}", r"\overline{ab}",
    r"\mathbb{R}", r"\operatorname{tr}A", "x^", r"\frac{", r"\unknowncmd", "", "a \"b\" 'c'", r"\mathit{ab} cd",
    "é ü", r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}",
]
METHODS = ["plain", "mathml", "mathjax", "katex", "webtex", "gladtex"]


def place(where, inline):
    if where == "para":
        return [{"t": "Para", "c": [S("a"), {"t": "Space"}, inline, {"t": "Space"}, S("b")]}]
    if where == "heading":
        return [{"t": "Header", "c": [1, ["h", [], []], [S("h"), {"t": "Space"}, inline]]}]
    cell = [A, {"t": "AlignDefault"}, 1, 1, [{"t": "Plain", "c": [inline]}]]
    return [{"t": "Table", "c": [A, [None, []], [[{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]],
                                 [A, []], [[A, 0, [], [[A, [cell]]]]], [A, []]]}]


def cases():
    for formula, kind, where, method, fmt in itertools.product(
            FORMULAS, ["InlineMath", "DisplayMath"], ["para", "heading", "cell"], METHODS, ["html", "html4"]):
        inline = {"t": "Math", "c": [{"t": kind}, formula]}
        yield f"math {method} {kind} {where} {formula!r}", fmt, {"math": method}, place(where, inline)


def record():
    rows = list(cases())
    outs = oracle_batch([(n, doc_of(b), f, o) for n, f, o, b in rows], chunk=2000)
    with open(DATA, "w") as f:
        for (name, fmt, opts, blocks), out in zip(rows, outs):
            if out is not None:
                f.write(json.dumps([name, fmt, opts, blocks, out], ensure_ascii=False, separators=(",", ":")) + "\n")
    print(f"{len(rows)} cases recorded")


if __name__ == "__main__":
    record() if sys.argv[1:] == ["record"] else replay_check(DATA, sys.argv[1:] == ["show"])
