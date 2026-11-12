#!/usr/bin/env python3
"""The differential run: many formulas through the oracle and through Randoc.

Usage: diffrun.py record DIR      write DIR/formulas.txt and DIR/oracle.tsv
       diffrun.py pairs DIR       the same for the pair run (see `pairs`);
                                  RANDOC_MATH_SEED selects the atoms (3)
       diffrun.py count           print the sizes of both runs
       diffrun.py table DIR       group DIR/diff.tsv by cause, print the table

Between the two steps, run the Randoc side:
  RANDOC_MATH_TSV=DIR/oracle.tsv RANDOC_MATH_OUT=DIR/diff.tsv \\
    cargo test -p randoc-math --test replay -- --ignored replay_other_file

The formulas put each command of the symbol table (`src/symbols.rs`) and a
list of constructs in each argument place of `CONTEXTS`, in inline and in
display math. `probe.py record` records the oracle results in batches.
Source env.sh first: it sets the heap limit of the oracle.
"""

import collections
import itertools
import os
import random
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import probe  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))

# Each argument place. X is the place.
CONTEXTS = [
    "X", "a X b", "X_i", "X^2", "x_X", "x^{X}", "X'",
    "\\sum_{X}", "\\sum_{X}^{n}", "\\int_{X}", "\\lim_{X}", "\\sum_i X",
    "\\frac{X}{b}", "\\frac{a}{X}", "\\sqrt{X}", "\\sqrt[X]{a}",
    "\\left(X\\right)", "\\left( a X \\right)",
    "\\begin{matrix}X & a\\end{matrix}", "\\begin{cases}X & a\\\\b\\end{cases}",
    "\\mathbf{X}", "\\mathrm{a X}", "\\mathcal{X}",
    "\\displaystyle X", "\\displaystyle 12 X",
    "\\text{a $X$ b}", "\\overset{X}{=}", "\\hat{X}", "X + b", "a + X",
    "\\substack{X\\\\a}",
]

# Constructs that are not in the symbol table.
CONSTRUCTS = [
    "x", "1", "12.5", "+", "-", "=", "(", ")", "[", "|", ",", ";", ".", "!", "'",
    "\\hat{x}", "\\bar{x}", "\\vec{v}", "\\tilde{a}", "\\overline{ab}", "\\underbrace{ab}",
    "\\frac{a}{b}", "\\sqrt{x}", "\\sqrt[3]{x}", "\\binom{n}{k}", "\\dfrac{1}{2}",
    "\\mathbf{x}", "\\mathbb{R}", "\\mathcal{L}", "\\mathrm{d}", "\\boldsymbol{\\alpha}",
    "\\text{ab}", "\\text{a b}", "\\textbf{t}", "\\mbox{m}",
    "\\left(x\\right)", "\\left[x\\right]", "\\left|x\\right|", "\\big(", "\\Big)",
    "x_1", "x^2", "x_i^2", "\\sin x", "\\log", "\\max", "\\operatorname{rank}",
    "\\quad", "\\,", "\\;", "\\!", "\\ ",
    "\\{", "\\}", "\\|", "\\langle", "\\rangle",
    "\\begin{pmatrix}a\\end{pmatrix}", "\\color{red}{x}", "\\boxed{x}", "\\phantom{x}",
    "\\not=", "\\not\\in", "\\mathop{T}", "\\mathbin{\\#}", "\\mathrel{R}",
    "\\overset{a}{b}", "\\underset{a}{b}", "\\stackrel{a}{=}", "\\xrightarrow{f}",
    "\\sum", "\\int", "\\prod_{i}", "\\lim_{n}",
]


# The places of the pair run: the top level and 6 containers. X is the pair.
PAIR_PLACES = [
    "X", "\\left(X\\right)", "\\begin{matrix}X\\end{matrix}", "\\begin{cases}X\\end{cases}",
    "x^{X}", "\\frac{X}{b}", "\\text{a $X$ b}",
]

# The atom classes of the pair run that are not classes of the symbol table.
# Each class holds atoms that the code treats in the same way.
PAIR_CLASSES = {
    "letter": ["x", "a", "B"],
    "number": ["1", "12.5", "0"],
    "number-separator": ["1,5", "1{,}5", "12\\,345"],
    "ascii-bin": ["+", "-", "*"],
    "ascii-rel": ["=", "<", ">"],
    "open": ["(", "[", "\\{", "\\langle"],
    "close": [")", "]", "\\}", "\\rangle"],
    "bar": ["|", "\\|", "\\vert"],
    "punctuation": [",", ";", "\\colon", "!", "?", ":", "."],
    "prime": ["'", "''", "\\prime"],
    "accent": ["\\hat{x}", "\\bar{x}", "\\vec{v}", "\\tilde{a}", "\\overline{ab}", "\\underbrace{ab}"],
    "fraction": ["\\frac{a}{b}", "\\dfrac{1}{2}", "\\binom{n}{k}", "{a \\over b}"],
    "root": ["\\sqrt{x}", "\\sqrt[3]{x}", "\\sqrt2"],
    "alphabet": ["\\mathbf{x}", "\\mathbb{R}", "\\mathcal{L}", "\\mathrm{d}", "\\boldsymbol{\\alpha}"],
    "text": ["\\text{ab}", "\\text{a b}", "\\textbf{t}", "\\mbox{m}"],
    "text-edge": ["\\text{ a}", "\\text{a }", "\\text{ }", "\\textbf{ b }", "\\mbox{ c}", "\\text{a { b}}"],
    "fenced": ["\\left(x\\right)", "\\left[x\\right]", "\\left|x\\right|", "\\left.x\\right\\}"],
    "sized": ["\\big(", "\\Big)", "\\bigl[", "\\bigg|"],
    "scripts": ["x_1", "x^2", "x_i^2", "x'_1", "x^{a'}_b"],
    "function": ["\\sin x", "\\log", "\\max", "\\operatorname{rank}", "\\operatorname{{ker}x}"],
    "space-command": ["\\quad", "\\,", "\\;", "\\!", "\\ ", "~", "\\enspace", "\\qquad"],
    "white-space": [" ", "\t", "\n", "\n\n", " \n "],
    "comment": ["%c\n", " %c\n", "%\n"],
    "table": ["\\begin{pmatrix}a\\end{pmatrix}", "\\begin{cases}a&b\\end{cases}", "\\substack{a\\\\b}"],
    "box": ["\\color{red}{x}", "\\boxed{x}", "\\phantom{x}"],
    "negation": ["\\not=", "\\not\\in", "\\not<"],
    "class-command": ["\\mathop{T}", "\\mathbin{\\#}", "\\mathrel{R}", "\\mathpunct{x}"],
    "stack": ["\\overset{a}{b}", "\\underset{a}{b}", "\\stackrel{a}{=}", "\\xrightarrow{f}"],
    "large-operator": ["\\sum", "\\int", "\\prod_{i}", "\\lim_{n}", "\\sum\\limits_i"],
    "row-end": ["\\\\", "\\\\[2pt]", "\\\\ ", "&"],
    "bracket-group": ["[a]", "[2pt]", "[A, B]"],
    "style": ["\\displaystyle x", "\\textstyle a", "\\scriptstyle 1"],
    "control": ["\x0b", "\u200b", "\x85"],
}


def pair_classes():
    """Each class of the pair run: the classes above and one class for each
    class of the symbol table, with its commands."""
    classes = {name: list(atoms) for name, atoms in PAIR_CLASSES.items()}
    with open(os.path.join(HERE, "..", "src", "symbols.rs"), encoding="utf-8") as f:
        text = f.read()
    body = text.split("pub(crate) static SYMBOLS")[1].split("];")[0]
    for name, cls in re.findall(r'\(\s*"([^"]+)",\s*Class::(\w+)', body):
        classes.setdefault("symbol-" + cls, []).append("\\" + name)
    return classes


def pairs():
    """Each ordered pair of two classes, with 3 atoms of each class (9
    formulas), in each place of `PAIR_PLACES`, in inline and display math."""
    rng = random.Random(int(os.environ.get("RANDOC_MATH_SEED", "3")))
    classes = pair_classes()
    names = sorted(classes)
    seen = set()
    for a in names:
        for b in names:
            left = rng.sample(classes[a], min(3, len(classes[a])))
            right = rng.sample(classes[b], min(3, len(classes[b])))
            for x in left:
                for y in right:
                    for place in PAIR_PLACES:
                        tex = place.replace("X", x + y)
                        for kind in ("I", "D"):
                            if (kind, tex) not in seen:
                                seen.add((kind, tex))
                                yield (kind, tex, place, a + "+" + b)


def symbol_names():
    with open(os.path.join(HERE, "..", "src", "symbols.rs"), encoding="utf-8") as f:
        text = f.read()
    body = text.split("pub(crate) static SYMBOLS")[1].split("];")[0]
    return re.findall(r'\(\s*"([^"]+)",', body)


def formulas():
    atoms = ["\\" + name for name in symbol_names()] + CONSTRUCTS
    out = []
    seen = set()
    for context in CONTEXTS:
        for atom in atoms:
            tex = context.replace("X", atom)
            for kind in ("I", "D"):
                if (kind, tex) not in seen:
                    seen.add((kind, tex))
                    out.append((kind, tex, context, atom))
    return out


def record(folder, items):
    """Records `items` in batches: one batch of formulas and results is in
    memory at a time."""
    with open(os.path.join(folder, "formulas.txt"), "w", encoding="utf-8", newline="\n") as f, \
            open(os.path.join(folder, "oracle.tsv"), "w", encoding="utf-8", newline="\n") as o:
        for batch in itertools.batched(items, probe.BATCH):
            for kind, tex, context, atom in batch:
                f.write("%s\t%s\t%s\t%s\n" % (kind, probe.esc(tex), probe.esc(context), probe.esc(atom)))
            for row in probe.record([(kind, tex) for kind, tex, _, _ in batch]):
                o.write(row + "\n")


def shape(text):
    """The element structure of a MathML body, with each text replaced by `_`."""
    text = re.sub(r'^"?(Ok\(")?<math display="(inline|block)" xmlns="[^"]*"><semantics>', "", text)
    text = re.sub(r"<annotation.*", "", text, flags=re.S)
    text = text.replace('\\"', '"')
    return re.sub(r">[^<]+<", ">_<", text)


def cause(output, want, got):
    """The signature of one difference: what kind of difference it is."""
    if output.endswith("warning"):
        if not got:
            return "warning: Randoc converts, the oracle fails"
        return "warning text (I-MATH-02)"
    if got.startswith("Err(Parse"):
        message = re.search(r'message: "([^"\\]*)', got)
        return "scope gap: Randoc error (%s)" % (message.group(1)[:50] if message else "?")
    if got.startswith("Err(Unsupported"):
        return "inline: Randoc gives up, the oracle converts"
    if output == "mathml" and not want.startswith("<math"):
        return "Randoc converts, the oracle fails"
    if output == "inline" and want == "None":
        return "Randoc converts, the oracle fails"
    if output == "mathml":
        a, b = shape(want), shape(got)
        if a == b:
            return "mathml: other text"
        i = 0
        while i < min(len(a), len(b)) and a[i] == b[i]:
            i += 1
        start = a.rfind("<", 0, i + 1)
        return "mathml: %s | %s" % (a[start:start + 40], b[start:start + 40])
    return "inline: other list"


def table(folder):
    contexts = {}
    with open(os.path.join(folder, "formulas.txt"), encoding="utf-8") as f:
        for line in f:
            kind, tex, context, atom = line.rstrip("\n").split("\t")
            contexts[(kind, probe.unesc(tex))] = (probe.unesc(context), probe.unesc(atom))
    groups = collections.defaultdict(list)
    with open(os.path.join(folder, "diff.tsv"), encoding="utf-8") as f:
        for line in f:
            output, kind, tex, want, got = [probe.unesc(x) for x in line.rstrip("\n").split("\t")]
            context = contexts.get((kind, tex), ("?", "?"))[0]
            groups[(output.split("-")[0] if output.endswith("warning") else output,
                    cause(output, want, got))].append((kind, tex, context))
    total = sum(1 for _ in open(os.path.join(folder, "oracle.tsv"), encoding="utf-8"))
    print("formulas: %d" % total)
    print("| Output | Cause | Count | Contexts | Example |")
    print("| --- | --- | ---: | ---: | --- |")
    for (output, sig), items in sorted(groups.items(), key=lambda kv: -len(kv[1])):
        example = min(items, key=lambda item: len(item[1]))
        print("| %s | %s | %d | %d | `%s` (%s) |" % (output, sig.replace("|", "\\|"), len(items),
                                                 len({c for _, _, c in items}), example[1], example[0]))


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "record":
        record(sys.argv[2], formulas())
    elif len(sys.argv) == 3 and sys.argv[1] == "pairs":
        record(sys.argv[2], pairs())
    elif len(sys.argv) == 2 and sys.argv[1] == "count":
        classes = pair_classes()
        print("single run: %d formulas" % len(formulas()))
        print("pair run: %d classes, %d atoms, %d formulas" % (
            len(classes), sum(len(v) for v in classes.values()), sum(1 for _ in pairs())))
    elif len(sys.argv) == 3 and sys.argv[1] == "table":
        table(sys.argv[2])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
