#!/usr/bin/env python3
"""The lever of the LaTeX reader: frequency list, differential run, probes.

Commands:

  freq [--top N]
      Count the LaTeX commands and environments in the LaTeX documents of
      the corpus ($RANDOC_CORPUS/*.tex). The list is the order of the work.

  diff [--count N] [--seed S] [--max-items K]
      Generate N documents from a grammar of the constructs that the reader
      supports, convert each one with the oracle and with Randoc
      (`-f latex -t native`), and print a table of difference classes with
      the smallest input of each class.

  cases [--count N] [--seed S]
      Write N documents of the grammar as conformance cases to
      conformance/cases/latex-reader/generated.toml (the file is replaced).

  matrix
      Write the rule matrices (each construct in each context) as
      conformance cases to conformance/cases/latex-reader/rules.toml (the
      file is replaced). Then run `randoc-harness record`.

  probe-list
      Probe each name of tools/names.tsv (the work list, most common first)
      as `probe` does. Then run `randoc-harness record`.

  table
      Read the harness report (`randoc-harness run --filter
      latex-reader/probes/` writes target/harness/report.json) and write the
      state of each name of tools/names.tsv to COMMANDS.md in the crate:
      equal (each probed context passes), not started (not equal, and no
      string of src/parse.rs names it), partial (some pass), different
      (none pass).

  probe NAME [--env] [--args ARGS]
      Put the command (or the environment) NAME in a set of contexts and add
      one conformance case for each context to
      conformance/cases/latex-reader/probes.toml. Then run
      `randoc-harness record`: the harness stores the oracle result as
      replay data.

Memory and time: at most 2 oracle processes run at the same time, each with
a limit of 60 seconds. `env.sh` limits the heap of each oracle process.
"""

import argparse
import collections
import concurrent.futures
import json
import os
import random
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
PROBES = ROOT / "conformance/cases/latex-reader/probes.toml"
RANDOC = ROOT / "target/debug/randoc"
TIME_LIMIT = 60
ORACLE_PROCESSES = 2


def corpus_files():
    corpus = Path(os.environ["RANDOC_CORPUS"])
    return sorted(p for p in corpus.glob("*.tex") if not p.name.startswith("._"))


def freq(top):
    commands = collections.Counter()
    environments = collections.Counter()
    for path in corpus_files():
        text = path.read_text(encoding="utf-8", errors="replace")
        text = re.sub(r"(?<!\\)%.*", "", text)
        for name in re.findall(r"\\begin\s*\{([^}]*)\}", text):
            environments[name] += 1
        for name in re.findall(r"\\([A-Za-z@]+|.)", text):
            if name not in ("begin", "end"):
                commands[name] += 1
    rows = [("\\" + n, c) for n, c in commands.items()]
    rows += [("{" + n + "}", c) for n, c in environments.items()]
    rows.sort(key=lambda row: (-row[1], row[0]))
    for rank, (name, count) in enumerate(rows[:top], 1):
        print(f"{rank}\t{count}\t{name}")


WORDS = ["alpha", "beta", "gamma", "delta", "x", "y2", "end."]

# Each rule gives a LaTeX fragment. `{}` is a place for a nested fragment.
INLINE_RULES = [
    "{w}", "{w} {w}", "\\emph{{{i}}}", "\\textbf{{{i}}}", "\\textit{{{i}}}",
    "\\texttt{{{w}}}", "{{\\bf {i}}}", "{{\\em {i}}}", "\\underline{{{i}}}",
    "\\textsc{{{i}}}", "{w}~{w}", "{w} -- {w}", "{w}---{w}", "``{i}''",
    "`{i}'", "\\%", "\\&", "\\$", "\\#", "\\_", "\\{{", "\\}}", "{w}\\\\{w}",
    "\\footnote{{{i}}}", "\\href{{http://e.org/{w}}}{{{i}}}",
    "\\url{{http://e.org/{w}}}", "${w}+1$", "\\({w}^2\\)", "\\verb|{w} {w}|",
    "\\label{{{w}}}", "\\ref{{{w}}}", "\\cite{{{w}}}", "\\citep[p.~1]{{{w}}}",
    "\\'e", "\\\"o", "\\ldots", "\\LaTeX", "\\TeX{{}}", "{w}%comment\n{w}",
    "\\includegraphics{{{w}.png}}", "\\unknown{{{w}}}", "{{{i}}}",
    "\\textsuperscript{{{w}}}", "\\textsubscript{{{w}}}", "\\mbox{{{i}}}",
]
BLOCK_RULES = [
    "{p}", "{p}\n\n{p}", "\\section{{{i}}}", "\\subsection*{{{i}}}",
    "\\paragraph{{{i}}}", "\\begin{{itemize}}\n\\item {p}\n\\item {p}\n\\end{{itemize}}",
    "\\begin{{enumerate}}\n\\item {p}\n\\end{{enumerate}}",
    "\\begin{{description}}\n\\item[{w}] {p}\n\\end{{description}}",
    "\\begin{{quote}}\n{p}\n\\end{{quote}}", "\\begin{{center}}\n{p}\n\\end{{center}}",
    "\\begin{{verbatim}}\n{w} {w}\n\\end{{verbatim}}",
    "\\begin{{equation}}\n{w}=1\n\\end{{equation}}", "\\[{w}\\]",
    "\\begin{{tabular}}{{lc}}\n{w} & {w} \\\\\n\\hline\n{w} & {w}\n\\end{{tabular}}",
    "\\newcommand{{\\mac}}[1]{{#1 {w}}}\n\\mac{{{w}}}",
]


def generate(rng, depth, kind):
    rules = BLOCK_RULES if kind == "block" else INLINE_RULES
    if depth <= 0:
        rules = INLINE_RULES[:2]
    rule = rng.choice(rules)
    nested = {}
    if "{i}" in rule:
        nested["i"] = generate(rng, depth - 1, "inline")
    if "{p}" in rule:
        nested["p"] = " ".join(
            generate(rng, depth - 1, "inline") for _ in range(rng.randint(1, 3))
        )
    return rule.format(w=rng.choice(WORDS), i=nested.get("i", ""), p=nested.get("p", ""))


def run(command, text):
    try:
        done = subprocess.run(
            command, input=text.encode(), capture_output=True, timeout=TIME_LIMIT
        )
    except subprocess.TimeoutExpired:
        return ("timeout", "", "")
    return (done.returncode, done.stdout.decode(errors="replace"), done.stderr.decode(errors="replace"))


def difference_class(expected, actual):
    if expected[0] != actual[0]:
        return f"exit {expected[0]} != {actual[0]}"
    if expected[2] != actual[2]:
        return "stderr: " + re.sub(r"\d+", "N", (expected[2] or actual[2]).splitlines()[0])[:80]
    for left, right in zip(expected[1].splitlines(), actual[1].splitlines()):
        if left != right:
            shape = re.sub(r'"(?:[^"\\]|\\.)*"', '"_"', left.strip())
            return "tree: " + shape[:80]
    return "tree: length"


def diff(count, seed, max_items):
    if not RANDOC.exists():
        sys.exit(f"build the program first: {RANDOC}")
    oracle = os.environ["RANDOC_ORACLE_PANDOC"]
    rng = random.Random(seed)
    inputs = []
    for _ in range(count):
        items = rng.randint(1, max_items)
        inputs.append("\n\n".join(generate(rng, 2, "block") for _ in range(items)) + "\n")
    command = ["-f", "latex", "-t", "native"]
    with concurrent.futures.ThreadPoolExecutor(ORACLE_PROCESSES) as pool:
        expected = list(pool.map(lambda text: run([oracle, *command], text), inputs))
    classes = {}
    equal = 0
    for text, want in zip(inputs, expected):
        got = run([str(RANDOC), *command], text)
        if want == got:
            equal += 1
            continue
        key = difference_class(want, got)
        count_so_far, smallest = classes.get(key, (0, text))
        classes[key] = (count_so_far + 1, min(smallest, text, key=len))
    print(f"inputs {len(inputs)}, equal {equal}, different {len(inputs) - equal}")
    print("| count | class | smallest input |")
    print("| --- | --- | --- |")
    for key, (n, smallest) in sorted(classes.items(), key=lambda item: -item[1][0]):
        shown = smallest.strip().replace("\n", "\\n").replace("|", "\\|")
        print(f"| {n} | `{key}` | `{shown[:160]}` |")


def write_cases(count, seed):
    rng = random.Random(seed)
    out = (
        "# Documents of the grammar of `latex_diff.py cases`. Do not edit: run the tool.\n"
        'from = "latex"\nto = []\n'
    )
    for number in range(1, count + 1):
        items = rng.randint(1, 3)
        text = "\n\n".join(generate(rng, 2, "block") for _ in range(items)) + "\n"
        out += f'\n[[case]]\nid = "generated-{number:03}"\ninput = {toml_string(text)}\n'
    target = ROOT / "conformance/cases/latex-reader/generated.toml"
    target.write_text(out)
    print(f"wrote {count} cases to {target.relative_to(ROOT)}; run randoc-harness record")


# Each matrix: a name, the forms of the construct, and the contexts. `{}` in
# a context is the place of the form.
MATRICES = [
    (
        "group",
        ["\\begin{quote}q\\end{quote}", "\\begin{itemize}\\item i\\end{itemize}",
         "\\section{s}", "p\n\np2", "x y", "\\begin{equation}e\\end{equation}",
         "\\begin{verbatim}\nv\n\\end{verbatim}"],
        ["{{{}}}", "a {{{}}} b", "a {{y {{{}}} z}} w", "{{\\bf {}}}",
         "\\begin{{itemize}}\\item {{{}}} x\\end{{itemize}}", "``a {{{}}} b''"],
    ),
    (
        "whole",
        ["{{x}}", "{ {x}}", "{{x} }", "{{{x}}}", "{{x}{y}}", "{ x }", "{x }", "{{\\bf x}}"],
        ["\\emph{}", "\\textbf{}", "\\section{}", "\\subsection*{}", "\\footnote{}",
         "\\href{{u}}{}", "\\textsc{}", "\\texttt{}"],
    ),
    (
        "star",
        ["emph", "textbf", "textit", "texttt", "textsc", "underline", "footnote", "url",
         "mbox", "label", "ref", "eqref", "cite", "citep", "citet", "LaTeX", "ldots",
         "includegraphics", "parencite", "Parencite", "autocite", "Autocite", "textcite",
         "Textcite", "citeauthor", "citeyear", "citealp", "citealt", "footcite", "Cite"],
        ["\\{}*{{x}} y", "a \\{}* {{x}}"],
    ),
    (
        "mbox",
        ["x y", "\\emph{x y}", "a\\\\b", "x~y", "\\textbf{a \\emph{b c}}"],
        ["\\mbox{{{}}}", "a \\hbox{{{}}} b"],
    ),
    (
        "cites",
        ["{a}{b}", "[p][s]{a}[q]{b} x", "(pre)(post)[p][s]{a}[q][r]{b}", "(pre){a}{b}",
         "{a} {b} x", "{a}\n{b}", "{a}[x]", "{a,b}{c}", "[p]{a}"],
        ["\\cites{}", "\\Cites{}", "\\textcites{}", "\\Textcites{}", "\\parencites{}",
         "\\autocites{}", "\\footcites{}", "\\supercites{}"],
    ),
    (
        "accent",
        ["\\^\\i x", "\\'\\i  x", "\\\"\\i{}x", "\\'{\\i} x", "\\`\\j x", "\\~\\o x",
         "\\c\\i x", "\\'e x"],
        ["{}", "a {} b"],
    ),
    (
        "url",
        ["{x}", "{{x}y}", "{a{b}c}"],
        ["\\url{}", "\\href{} {{t}}"],
    ),
    ("today", ["\\today", "\\today x"], ["{}", "\\date{{{}}}"]),
    (
        "cond",
        ["\\iftrue a\\else b\\fi", "\\iffalse a\\else b\\fi", "\\iftrue a\\iffalse b\\fi c\\fi",
         "\\iffalse a\\ifx b\\else q\\fi c\\else d\\fi", "\\iffalse a\\else b",
         "\\newif\\iffoo \\footrue\\iffoo yes\\else no\\fi",
         "\\newif\\iffoo \\iffoo yes\\else no\\fi", "\\iftrue a\n\nb\\else c\\fi",
         "\\fi x \\else y"],
        ["{}", "p {} q", "\\emph{{{}}}"],
    ),
    (
        "enquote",
        ["\\enquote{x}", "\\enquote*{x}", "\\enquote{a \\enquote{b} c}",
         "\\enquote*{a \\enquote{b}}", "\\enquote{ x }"],
        ["{}", "p {} q"],
    ),
    (
        "misc",
        ["\\texorpdfstring{\\emph{a}}{b}", "\\char65 b", "\\char65x", "\\noindent{x} y",
         "\\centering {x} y", "\\maketitle", "\\maketitle{x} y"],
        ["{}", "p {} q", "p\n\n{}\n\nq"],
    ),
    (
        "headlabel",
        ["\\section{a}", "\\section{\\}}", "\\subsection*{a}"],
        ["{}\n\n\\label{{b}} x", "{}\n\n%c\n\n\\label{{b}}", "{}\n\nx \\label{{b}}", "{}\n\\label{{b}}"],
    ),
    (
        "braced",
        ["section", "subsection", "chapter", "part", "paragraph", "subparagraph", "label",
         "ref", "eqref", "autoref", "cref", "Cref", "cite", "citep", "citet", "citealp",
         "citeauthor", "citeyear", "parencite", "Parencite", "textcite", "autocite",
         "footcite", "cites", "nocite", "begin", "usepackage", "includegraphics", "footnote",
         "footnotetext", "documentclass", "url", "href", "nolinkurl", "hyperlink",
         "hypertarget", "hyperref", "title", "author", "thanks", "bibliography",
         "addbibresource", "input", "include", "newenvironment", "newtheorem", "theoremstyle",
         "textcolor", "colorbox", "foreignquote", "SI", "num", "qty", "ang", "ensuremath",
         "gls", "ac", "parbox", "graphicspath", "newcommand", "renewcommand",
         "providecommand", "DeclareMathOperator", "def", "let",
         "emph", "textbf", "texttt", "caption", "mbox", "item", "date", "hspace", "enquote",
         "multicolumn", "textsc", "underline", "marginpar", "newcounter", "color"],
        ["a \\{}| b", "a \\{} x y", "a \\{}*| b", "a \\{}[o]| b", "a \\{}\\foo  b",
         "\\emph{{\\{}|}}", "\\begin{{itemize}}\\item \\{}|\\end{{itemize}}"],
    ),
    (
        "unclosed",
        ["document", "abstract", "itemize", "enumerate", "description", "quote", "center",
         "minipage", "verbatim", "lstlisting", "figure", "table", "tabular", "longtable",
         "equation", "align*", "unknownenv"],
        ["\\begin{{{}}}a", "x\n\n\\begin{{{}}}a\n\nb\n", "\\begin{{{}}}a}}b\\end{{{}}}",
         "\\emph{{\\begin{{{}}}a}} b"],
    ),
    (
        "lead",
        ["itemize", "enumerate", "description"],
        ["\\begin{{{}}}\nlead\n\\end{{{}}}", "\\begin{{{}}}\nlead\n\\item a\n\\end{{{}}}"],
    ),
]

# Environments with the arguments that they need, for the error matrices.
ERROR_ENVIRONMENTS = [
    ("itemize", ""), ("enumerate", ""), ("description", ""), ("quote", ""), ("quotation", ""),
    ("verse", ""), ("center", ""), ("flushleft", ""), ("minipage", "{3cm}"), ("figure", ""),
    ("table", ""), ("abstract", ""), ("unknownenv", ""), ("tabular", "{l}"),
    ("longtable", "{l}"), ("tabularx", "{3cm}{l}"), ("multicols", "{2}"), ("small", ""),
    ("theorem", ""), ("proof", ""), ("sloppypar", ""), ("otherlanguage", "{german}"),
    ("table*", ""), ("thebibliography", "{9}"),
]
ERRORS = ["\\end{word}", "}", "\\label|", "\\textbf{a"]


def error_forms():
    forms = []
    for name, args in ERROR_ENVIRONMENTS:
        item = "\\item " if name in ("itemize", "enumerate", "description",
                                     "thebibliography") else ""
        for error in ERRORS:
            forms.append(f"\\begin{{{name}}}{args}\n{item}x {error} y\n\\end{{{name}}}")
    return forms


# Matrices of parse errors and of table cells, in `errors.toml`. The fourth
# element is the input format of the cases.
ERROR_MATRICES = [
    (
        "fallback",
        error_forms(),
        ["a\n\n{}\n\nb", "\\begin{{quote}}\n{}\n\\end{{quote}}",
         "\\begin{{itemize}}\n\\item {}\n\\end{{itemize}}\nz"],
        "latex",
    ),
    ("fallback-raw", error_forms(), ["a\n\n{}\n\nb"], "latex+raw_tex"),
    (
        "cell",
        ["p \\end{word} q", "a } b", "\\textbf{a & b}", "{a & b}", "\\emph{a \\\\ b}",
         "a \\label| b", "a \\begin{quote} b", "\\begin{quote} a \\end{quote}",
         "\\begin{tabular}{l} a & b \\end{tabular}", "\\begin{itemize}\\item x\\end{itemize}",
         "a\n\nb", "\\multicolumn{2}{c}{a}", "\\multicolumn{2}{c}{a} & b", "a & b & c",
         "", "\\section{h}", "\\textbf{a", "\\begin{word} x", "\\end{word}", "a & b",
         "\\itshape a ", "\\emph{a} \\textbf{b}", "f \\begin{quote} a \\end{quote}"],
        ["\\begin{{tabular}}{{l}}\n{} \\\\\nd \\\\\n\\end{{tabular}}",
         "\\begin{{tabular}}{{ll}}\n{} & z \\\\\nd & e \\\\\n\\end{{tabular}}",
         "\\begin{{tabular}}{{lll}}\n\\hline\n{} \\\\\n\\hline\nd & e & f\n\\end{{tabular}}"],
        "latex",
    ),
    (
        "cell-raw",
        ["a & b & c", "\\textbf{a & b}", "p \\end{word} q"],
        ["\\begin{{tabular}}{{ll}}\n{} \\\\\n\\end{{tabular}}"],
        "latex+raw_tex",
    ),
    (
        "stray",
        ["\\end{word}", "\\end*{word}", "\\end[o]{word}", "}", "{a", "\\textbf{a",
         "\\begin{word}", "\\emph{\\begin{quote}x\\end{quote}}", "\\emph{a \\end{word}}",
         "{a \\end{word} b}", "\\emph{\\begin{word}}", "\\footnote{a \\end{word}}",
         "\\emph{\\begin{equation}x\\end{equation}}"],
        ["{}", "a {}", "x\n\n{}", "  {}", "\\begin{{quote}}\n{}\n\\end{{quote}}",
         "\\begin{{center}}\nx {} y\n\\end{{center}}",
         "\\begin{{itemize}}\n\\item {}\n\\end{{itemize}}\nz",
         "\\begin{{document}}\n{}\n\\end{{document}}"],
        "latex",
    ),
    (
        "figure",
        ["\\includegraphics{a.png}", "\\includegraphics{a.png} x", "text",
         "\\includegraphics{a.png}\n\\includegraphics{b.png}",
         "\\includegraphics{a.png}\n\ny",
         "\\begin{center}\n\\includegraphics{a.png}\n\\end{center}",
         "\\begin{quote}\n\\includegraphics{a.png}\n\\end{quote}",
         "\\emph{\\includegraphics{a.png}}", "\\centering\n\\includegraphics[width=3cm]{a.png}",
         "one\n\ntwo"],
        ["\\begin{{figure}}\n{}\n\\end{{figure}}",
         "\\begin{{figure}}[h]\n{}\n\\caption{{c}}\\label{{f}}\n\\end{{figure}}",
         "\\begin{{itemize}}\n\\item \\begin{{figure}}\n{}\n\\end{{figure}}\n\\end{{itemize}}"],
        "latex",
    ),
    (
        "par",
        ["a\\par b", "a \\par\nb", "\\par a", "a\\par\n\nb", "a\\par\\par b"],
        ["{}", "\\begin{{itemize}}\\item {}\\end{{itemize}}",
         "\\begin{{tabular}}{{l}}{} \\\\\\end{{tabular}}", "x {{{}}} y",
         "\\begin{{quote}}{}\\end{{quote}}", "\\emph{{{}}}", "\\section{{{}}}",
         "\\newcommand{{\\x}}[1]{{#1}}\\x{{{}}}"],
        "latex",
    ),
    (
        "mathmacro",
        ["equation", "equation*", "align", "align*", "gather", "multline", "eqnarray",
         "displaymath", "math"],
        ["\\newcommand{{\\R}}{{\\mathbb{{R}}}}\\begin{{{}}}a\\R\\end{{{}}}",
         "\\newcommand{{\\be}}{{\\begin{{{}}}}}\\newcommand{{\\ee}}{{\\end{{{}}}}}\nT.\n\\be\nx\n\\ee\nM.",
         "\\newcommand{{\\ee}}{{\\end{{{}}}}}\\begin{{{}}}x\\ee",
         "\\newcommand{{\\v}}[1]{{\\mathbf{{#1}}}}\\begin{{{}}}\\v{{a}}\\end{{{}}}",
         "\\newcommand{{\\x}}{{\\iftrue a\\fi}}\\begin{{{}}}\\x\\end{{{}}}"],
        "latex",
    ),
    (
        "endarg",
        ["\\end*{word}", "\\end[o]{word}", "\\end {word}", "\\end x", "\\end{word}"],
        ["{}", "\\begin{{quote}}\n{}\n\\end{{quote}}", "\\begin{{center}}x {}\\end{{center}}",
         "\\begin{{itemize}}\\item {}\\end{{itemize}}"],
        "latex",
    ),
    (
        "span",
        ["\\multicolumn{2}{c}{a}", "\\multicolumn{3}{c}{a}", "\\multicolumn{3}{c}{a} & b",
         "\\multicolumn{1}{r}{a} & b", "a & \\multicolumn{2}{l}{b}"],
        ["\\begin{{tabular}}{{l}}\n{} \\\\\n\\end{{tabular}}",
         "\\begin{{tabular}}{{ll}}\n{} \\\\\n\\end{{tabular}}",
         "\\begin{{tabular}}{{lll}}\n{} \\\\\nx \\\\\n\\end{{tabular}}"],
        "latex",
    ),
    (
        "global",
        ["\\global\\def\\x{a}\\x", "\\global\\let\\a\\b x", "\\global\\edef\\x{a}\\x",
         "\\global\\gdef\\x{a}\\x", "\\global\\xdef\\x{a}\\x", "\\gdef\\x{a}\\x",
         "\\edef\\x{a}\\x", "\\global\\newcommand{\\x}{a}\\x", "\\global x",
         "\\global{a}", "\\global\\relax a", "\\global\\global\\def\\x{a}\\x",
         "\\global\\advance\\count by1", "\\global\\setlength\\x{2}", "\\global"],
        ["{}", "a {}", "\\begin{{quote}}{}\\end{{quote}}"],
        "latex",
    ),
    (
        "lines",
        ["abc\\\nde }", "abc\\\nde\nf }", "ab\\\\\nde }", "abc\\\n  de }",
         "a\\\nb\\\nc\n\\def<{x}", "x\\\n\n\\section|"],
        ["{}", "p\n\n{}"],
        "latex",
    ),
    (
        "accent",
        ["\\~{}", "\\'{}", "\\`{}", "\\^{}", "\\\"{}", "\\={}", "\\.{}", "\\c{}", "\\v{}",
         "\\u{}", "\\H{}", "\\r{}", "\\k{}", "\\d{}", "\\b{}", "\\~{ }", "\\'{e}", "\\~ n"],
        ["{}", "a{}b", "\\emph{{{}}}", "\\section{{{}}}"],
        "latex",
    ),
    (
        "thanks",
        ["A\\thanks{x}", "A\\thanks{x \\emph{y}}", "A\\thanks{x}\\thanks{y}"],
        ["\\title{{{}}}\\begin{{document}}\\maketitle\\end{{document}}",
         "\\author{{{} \\and B}}\\begin{{document}}\\maketitle\\end{{document}}",
         "\\date{{{}}}\\begin{{document}}\\maketitle\\end{{document}}", "{}"],
        "latex",
    ),
    (
        "thanks-raw",
        ["A\\thanks{x}"],
        ["{}", "\\title{{{}}}\\begin{{document}}\\maketitle\\end{{document}}"],
        "latex+raw_tex",
    ),
    (
        "envparam",
        ["\\newenvironment{e}{}{}\\begin{e}\\newcommand\\x[1]{#1}\\x a\\end{e}",
         "\\newenvironment{e}[1]{<#1>}{>}\\begin{e}{p}q\\end{e}",
         "\\newenvironment{e}[2][d]{(#1,#2)}{.}\\begin{e}{p}q\\end{e}",
         "\\newenvironment{e}[2][d]{(#1,#2)}{.}\\begin{e}[o]{p}q\\end{e}",
         "\\newenvironment{e}{}{}\\begin{e}\\def\\y#1{[#1]}\\y z\\end{e}"],
        ["{}", "x {} y", "\\begin{{quote}}{}\\end{{quote}}"],
        "latex",
    ),
    (
        "verbspace",
        ["verbatim", "lstlisting", "Verbatim", "comment"],
        ["\\begin {{{}}}\n% n\n\\foo{{x}} y\n\\end{{{}}}", "\\begin  {{{}}}%\n\\end{{{}}}",
         "a\n\n\\begin {{{}}}\nb\n\\end{{{}}}\n\nc"],
        "latex",
    ),
    (
        "argmacro",
        ["\\label{\\x}\\ref{\\x}", "\\cite{\\x}", "\\cite[\\x]{k}", "\\citep[p]{\\x}",
         "\\textcite{\\x}", "\\eqref{\\x}", "\\autoref{\\x}", "\\section{T}\\label{s\\x}",
         "\\includegraphics{\\x}", "\\url{\\x}", "\\href{\\x}{y}", "\\foo{\\x}"],
        ["\\newcommand{{\\x}}{{k1,k2}}{}", "\\newcommand{{\\x}}{{X}}{}", "\\def\\x{{a.png}}{}"],
        "latex",
    ),
    (
        "argmacro-raw",
        ["\\cite[\\x]{k}", "\\foo{\\x}", "\\eqref{\\x}", "\\label{\\x}"],
        ["\\newcommand{{\\x}}{{X}}{}"],
        "latex+raw_tex",
    ),
    (
        "head",
        ["a & b \\\\\n\\hline\nc & d \\\\\n", "a & b \\\\\nc & d \\\\\n\\hline\ne & f \\\\\n",
         "\\hline\na & b \\\\\n\\hline\nc & d \\\\\n", "a & b \\\\\n\\hline\n",
         "\\toprule\na & b \\\\\n\\midrule\nc & d \\\\\n\\bottomrule\n",
         "a & b \\\\\nc & d \\\\\ne & f \\\\\n\\hline\n", "\\hline\n\\hline\na & b \\\\\n\\hline\nc & d\\\\\n"],
        ["\\begin{{tabular}}{{ll}}\n{}\\end{{tabular}}",
         "\\begin{{table}}\\caption{{c}}\\begin{{tabular}}{{ll}}\n{}\\end{{tabular}}\\end{{table}}"],
        "latex",
    ),
    (
        "ctrlsym",
        ["\\def\\!{x}", "\\newcommand{\\!}{x}", "\\def\\-{x}", "\\def\\,{x}", "\\let\\!\\relax",
         "\\newcommand{\\;}[1]{<#1>}"],
        ["{}\\!\\-\\,\\;{{a}}", "{}a\\! b", "{}\\emph{{\\!}}"],
        "latex",
    ),
    (
        "ctrlspace",
        ["\\  x", "a\\ \\ b", "x\n\\ \ny", "\\ \n\nb", "a\\\tb", "a\\  \n  b"],
        ["{}", "\\emph{{{}}}"],
        "latex",
    ),
    (
        "dimension",
        [".5\\linewidth", "0.5\\linewidth", "1.\\linewidth", "1\\linewidth", "-.5\\linewidth",
         "0.25\\textwidth", "2\\columnwidth", "50", "1.5cm", "0.5 \\linewidth", "+0.5\\linewidth"],
        ["\\includegraphics[width={}]{{x}}", "\\includegraphics[height={}]{{x}}"],
        "latex",
    ),
    (
        "delimdef",
        ["\\def\\x#1.{(#1)}\\x ab. c", "\\def\\x#1.{(#1)}\\x {ab}. c", "\\def\\x#1.{(#1)}\\x {a}b. c",
         "\\def\\x#1.{(#1)}\\x ab c", "\\def\\x#1.{(#1)}\\x a{.}b. c", "\\def\\x[#1]{<#1>}\\x q r",
         "\\def\\x[#1]{<#1>}\\x [q] r", "\\def\\x#1.{(#1)}\\x ab\n\ncd. e",
         "\\def\\x(#1,#2){<#1|#2>}\\x(a,b) c", "\\def\\x#1#2.{#2#1}\\x ab.",
         "\\def\\x#1.#2{[#1][#2]}\\x a.b c", "\\def\\x#1,{#1#1}\\x ab,", "\\def\\x#1.#2!{(#1)(#2)}\\x a.b! c",
         "\\def\\x#1..{(#1)}\\x a.b.. c", "\\def\\x.#1{(#1)}\\x .ab"],
        ["{}", "\\emph{{{}}}", "\\begin{{itemize}}\\item {}\\end{{itemize}}"],
        "latex",
    ),
    (
        "expand",
        # Each macro kind gives the text `kx`: \m is the use of the macro.
        ["\\newcommand{\\m}{kx}", "\\def\\m{kx}", "\\newcommand{\\n}[1]{k#1}\\def\\m{\\n{x}}",
         "\\newcommand{\\n}[1][x]{k#1}\\def\\m{\\n}", "\\def\\n{kx}\\let\\m\\n",
         "\\def\\!{kx}\\def\\m{\\!}", "\\def\\n#1.{k#1}\\def\\m{\\n x.}"],
        ["{}\\label{{\\m}}\\ref{{\\m}}", "{}\\cite{{\\m}}", "{}\\cite[\\m]{{c}}",
         "{}\\url{{\\m}}", "{}\\href{{\\m}}{{t}}", "{}\\includegraphics{{\\m}}",
         "{}\\section{{\\m}}", "{}\\emph{{\\m}}", "{}\\footnote{{\\m}}", "{}\\texttt{{\\m}}",
         "{}\\verb|\\m|", "{}\\begin{{equation}}\\m\\end{{equation}}",
         "{}\\begin{{align}}a&\\m\\end{{align}}", "{}$\\m$", "{}\\[\\m\\]",
         "{}\\begin{{verbatim}}\n\\m\n\\end{{verbatim}}", "{}\\begin{{quote}}\\m\\end{{quote}}",
         "{}\\begin{{tabular}}{{l}}\\m\\\\\\end{{tabular}}",
         "{}\\begin{{itemize}}\\item \\m\\end{{itemize}}", "{}\\'\\m", "{}\\caption{{\\m}}",
         "{}\\title{{\\m}}\\begin{{document}}\\maketitle\\end{{document}}"],
        "latex",
    ),
    (
        "expand-raw",
        ["\\newcommand{\\m}{kx}", "\\def\\!{kx}\\def\\m{\\!}"],
        ["{}\\foo{{\\m}}", "{}\\cite[\\m]{{c}}", "{}\\label{{\\m}}", "{}\\begin{{zz}}\\m\\end{{zz}}"],
        "latex+raw_tex",
    ),
    (
        "counter",
        ["\\setcounter{enumi}{1}", "\\setcounter{enumi}{0}", "\\setcounter{enumi}{-3}",
         "\\setcounter{enumi}{a}", "\\setcounter{enumi}{ 2 }", "\\setcounter {enumi} {2}",
         "\\setcounter{enumii}{2}", "\\setcounter{enumv}{3}", "\\setcounter{enum}{3}",
         "\\setcounter{enumx}{3}", "\\setcounter{enumI}{3}", "\\setcounter{enumi }{3}",
         "\\setcounter{page}{3}", "\\setcounter{enumi}{+3}", "\\setcounter{enumi}{1+1}",
         "\\setcounter{enumi}{}", "\\setcounter{enumi}3", "\\setcounter{enumi}{007}",
         "\\setcounter{enumi}{99999999999999999999}", "\\setcounter{enumi}{1}\\setcounter{enumi}{5}",
         "\\setcounter{enumi}{a}\\setcounter{enumi}{3}", "\\setcounter{page}{1}\\setcounter{enumi}{3}",
         "\\setcounter{enumi}{3}x", "x\\setcounter{enumi}{3}", "{\\setcounter{enumi}{3}}",
         "%c\n\\setcounter{enumi}{3}\n\n", "\\addtocounter{enumi}{2}", "\\setcounter*{enumi}{3}",
         "\\s", "\\setcounter{enumi}{\\x}", ""],
        ["\\def\\s{{\\setcounter{{enumi}}{{4}}}}\\def\\x{{6}}\\begin{{enumerate}}{}\\item y\\end{{enumerate}}",
         "\\begin{{enumerate}}[(i)]{}\\item y\\item z\\end{{enumerate}}",
         "\\begin{{enumerate}}[a]{}\\item y\\end{{enumerate}}",
         "\\begin{{enumerate}}{}\\end{{enumerate}}",
         "\\begin{{enumerate}}\\item a\\begin{{enumerate}}{}\\item x\\end{{enumerate}}\\end{{enumerate}}",
         "\\begin{{itemize}}{}\\item y\\end{{itemize}}"],
        "latex",
    ),
    (
        "label",
        ["a", "a.", "A", "i", "I", "1", "(a)", "a)", "(1)", "i.", "1.", "x", "(i", "a:", " a. ",
         "Step 1.", "\\alph*."],
        ["\\begin{{enumerate}}[{}]\\item y\\end{{enumerate}}",
         "\\begin{{enumerate}} [{}]\\item y\\end{{enumerate}}"],
        "latex",
    ),
    (
        "scope",
        ["{\\footrue}", "\\footrue{\\foofalse}", "{\\footrue\\iffoo a\\else b\\fi}", "{{\\footrue}}",
         "{\\global\\footrue}", "\\begin{quote}\\footrue\\end{quote}", "\\emph{\\footrue}",
         "\\footnote{\\footrue}", "\\begin{itemize}\\item\\footrue\\end{itemize}",
         "\\begin{tabular}{l}\\footrue\\\\\\end{tabular}", "\\def\\y{\\footrue}{\\y}",
         "\\newcommand{\\y}[1]{#1}\\y{\\footrue}", "\\textbf{\\footrue}", "\\section{\\footrue}",
         "\\x{\\footrue}", "{\\newif\\iffoo}", "{\\newif\\iffoo\\footrue}", "\n\n{\\footrue}\n\n",
         "{\\footrue}{\\foofalse\\global\\footrue}", "\\begingroup\\footrue\\endgroup"],
        ["\\newif\\iffoo{}\\iffoo a\\else b\\fi", "\\newif\\iffoo{}{{\\iffoo a\\else b\\fi}}",
         "\\newif\\iffoo\\texttt{{{}\\iffoo a\\else b\\fi}}"],
        "latex",
    ),
    (
        "macroscope",
        ["{\\def\\x{a}}", "{\\newcommand{\\x}{a}}", "\\emph{\\def\\x{a}}", "\\begin{quote}\\def\\x{a}\\end{quote}",
         "{\\renewcommand{\\x}{a}}", "{\\gdef\\x{a}}", "{\\global\\def\\x{a}}", "{\\xdef\\x{a}}",
         "{\\let\\x\\relax}", "{\\def\\x{a}\\x}", "{\\newenvironment{e}{(}{)}}\\begin{e}e\\end{e}",
         "{\\def\\x{a}{\\gdef\\x{c}}\\x}", "\\begin{itemize}\\item\\def\\x{a}\\end{itemize}",
         "\\section{\\def\\x{a}}", "\\footnote{\\def\\x{a}}"],
        ["\\def\\x{{b}}{}\\x", "{}\\x"],
        "latex",
    ),
    (
        "optspace",
        ["", " ", "  ", "\n", " \n ", "\n\n"],
        ["\\footnote{}[0]{{x}}",
         "\\cite{}[p]{}[q]{}{{k}}",
         "\\cite{}[p]{}{{k}}",
         "\\citep{}[p]{}{{k}}",
         "\\textcite{}[p]{}[q]{}{{k}}",
         "\\includegraphics{}[width=1cm]{}{{x}}",
         "\\section{}[s]{}{{x}}",
         "\\section*{}[s]{}{{x}}",
         "\\begin{{figure}}{}[h]\\includegraphics{{a}}\\caption{}[s]{}{{c}}\\end{{figure}}",
         "\\begin{{itemize}}\\item{}[a] x\\end{{itemize}}",
         "\\begin{{description}}\\item{}[a] x\\end{{description}}",
         "a\\\\{}[2pt] b",
         "\\newcommand{{\\x}}[1][d]{{(#1)}}\\x{}[o]",
         "\\newcommand{{\\x}}[2][d]{{(#1,#2)}}\\x{}[o]{}{{y}}",
         "\\begin{{enumerate}}{}[a.]\\item x\\end{{enumerate}}",
         "\\begin{{minipage}}{}[t]{}{{3cm}}x\\end{{minipage}}",
         "\\footnote{}[0]{}{{x}}",
         "\\autocite{}[p]{}[q]{}{{k}}",
         "\\cites{}[a]{}{{k}}{}[b]{}{{l}}",
         "\\begin{{lstlisting}}{}[language=C]\nx\n\\end{{lstlisting}}",
         "\\caption{}[s]{}{{x}}",
         "\\parencite{}[p]{}{{k}}",
         "\\href{}{{u}}{}{{t}}",
         "\\begin{{tabular}}{}{{l}}a\\end{{tabular}}",
         "\\newenvironment{{e}}[1][d]{{(#1)}}{{.}}\\begin{{e}}{}[o]x\\end{{e}}",
         "\\cite{}{{k}}",
         "\\textbf{}[x]{{y}}",
         "\\footnote{}%c\n[0]{{x}}"],
        "latex",
    ),
    (
        "mathspace",
        [" x", "x ", " x ", "\nx\n", "  x  ", "\n x \n", "%c\nx", "x\n\n"],
        ["\\begin{{math}}{}\\end{{math}}", "\\begin{{displaymath}}{}\\end{{displaymath}}",
         "\\begin{{equation}}{}\\end{{equation}}", "\\begin{{align*}}{}\\end{{align*}}",
         "a \\begin{{math}}{}\\end{{math}} b"],
        "latex",
    ),
    (
        "code",
        ["\\texttt{--}", "\\texttt{---}", "\\texttt{a--b c}", "\\texttt{``q''}", "\\texttt{`q'}", "\\texttt{a~b}", "\\texttt{\\url{x}}", "\\texttt{\\emph{a--b}}", "\\texttt{\\textsc{ab}}", "\\texttt{\\underline{ab}}", "\\texttt{\\textsuperscript{ab}}", "\\texttt{\\textsubscript{ab}}", "\\texttt{\\sout{ab}}", "\\texttt{a\\footnote{b--c}}", "\\texttt{\\cite{k}}", "\\texttt{\\enquote{ab}}", "\\texttt{\\href{u}{ab}}", "\\texttt{\\includegraphics{x}}", "\\texttt{$x$}", "\\texttt{a\\\\b}", "\\texttt{a\\textbf{b}c}", "\\texttt{a\\ b}", "\\texttt{\\ldots}", "\\texttt{?`}", "\\texttt{a\nb}", "\\texttt{\\emph{x} y}", "\\texttt{\\textbf{--}}", "\\texttt{x\\verb|y|}", "\\texttt{\\LaTeX}", "\\texttt{\\$}", "x\\enquote{a}y", "\\enquote*{a}", "\\enquote{\\emph{a}}", "!`", "a'b", "\\'e"],
        ["{}", "a {} b", "\\emph{{{}}}"],
        "latex",
    ),
    (
        "plaingroup",
        ["{{x}}", "{{x}y}", "{{{x}}}", "{ {x}}", "{{x} }", "{{x}{y}}", "a{{x}}b", "{{}}",
         "{{\\emph{x}}}", "{x{y}}", "{{x\n\ny}}"],
        ["{}", "\\emph{{{}}}", "\\section{{{}}}", "\\begin{{itemize}}\\item {}\\end{{itemize}}"],
        "latex",
    ),
    (
        "metagroup",
        ["{\\title{x}}", "{\\author{a}}", "{\\date{d}}", "{\\title{x} y}", "{a \\title{x}}",
         "{{\\title{x}}}", "\\emph{\\title{x}}"],
        ["{}", "{}\\begin{{document}}\\maketitle\\end{{document}}"],
        "latex",
    ),
    (
        "eofarg",
        ["\\emph", "\\textbf", "\\section", "\\footnote", "\\texttt", "\\mbox", "\\url", "\\label",
         "\\cite", "\\href{u}", "\\textsc", "\\emph ", "\\emph\n", "\\underline", "\\textsuperscript",
         "\\enquote", "\\ref", "\\includegraphics", "\\title", "\\verb"],
        ["{}", "a {}", "\\begin{{quote}}x\\end{{quote}}{}"],
        "latex",
    ),
    (
        "tikz",
        ["\\begin{tikzpicture}\n\\draw (0,0);\n\\end{tikzpicture}",
         "\\begin{tikzpicture}[s]{w}\nb\n\\end{tikzpicture}"],
        ["{}", "a {} b", "\\begin{{tabular}}{{ll}}\n{} & x\\\\\n\\end{{tabular}}",
         "\\begin{{itemize}}\\item {}\\end{{itemize}}"],
        "latex",
    ),
    (
        "emptylist",
        ["", "\n", " ", "%c\n", "\n\n", "{}", "\\relax", " x", "\\label{a}", "[a]", "[a]\n",
         "[a] ", "\\setcounter{enumi}{3}", "\\setcounter{enumi}{3}\n", "\n\\setcounter{enumi}{3}",
         "\\setcounter{enumi}{3}x", "\\setcounter{enumx}{3}", "\\setcounter{page}{3}",
         "\\setcounter{enumi}{a}", "\\setcounter{enumi}3", "\\addtocounter{enumi}{2}",
         "[a]\\setcounter{enumi}{3}", "[a]\\item[b] x", "\\setcounter{enumx}{3}\\item y"],
        ["\\begin{{itemize}}{}\\end{{itemize}}", "\\begin{{enumerate}}{}\\end{{enumerate}}",
         "\\begin{{description}}{}\\end{{description}}"],
        "latex",
    ),
    (
        "manyopts",
        ["[a][b]", "[a] [b]", "[a][b][c]", "[a]\n[b]"],
        ["\\footnote{}{{x}}", "\\thanks{}{{x}}", "\\section{}{{x}}", "\\section*{}{{x}}",
         "\\paragraph{}{{x}}", "\\begin{{minipage}}{}{{1cm}}x\\end{{minipage}}",
         "\\begin{{table}}{}x\\end{{table}}", "\\begin{{figure}}{}\\includegraphics{{x}}\\end{{figure}}",
         "\\begin{{zzz}}{}x\\end{{zzz}}", "\\begin{{center}}{}x\\end{{center}}",
         "\\begin{{flushleft}}{}x\\end{{flushleft}}", "\\begin{{tabular}}{}{{l}}x\\end{{tabular}}",
         "\\begin{{itemize}}\\item{} x\\end{{itemize}}",
         "\\begin{{enumerate}}\\item{} x\\end{{enumerate}}",
         "\\begin{{description}}\\item{} x\\end{{description}}",
         "\\caption{}{{x}}", "\\includegraphics{}{{x}}"],
        "latex",
    ),
    (
        "newline",
        ["a\\newline b", "a\\newline [a] b", "a\\newline{x} b", "a\\newline\n\nb", "a \\newline b",
         "\\newline a", "a\\newline", "a\\\\ \\newline b"],
        ["{}", "\\emph{{{}}}", "\\begin{{itemize}}\\item {}\\end{{itemize}}",
         "\\begin{{tabular}}{{l}}{}\\\\\\end{{tabular}}"],
        "latex",
    ),
    (
        "newline-raw",
        ["a\\newline b", "a\\newline [a] b"],
        ["{}"],
        "latex+raw_tex",
    ),
]


def write_matrix():
    out = (
        "# Rule matrices of `latex_diff.py matrix`. Do not edit: run the tool.\n"
        'from = "latex"\nto = []\n'
    )
    count = 0
    for name, forms, contexts in MATRICES:
        for f, form in enumerate(forms, 1):
            for c, context in enumerate(contexts, 1):
                text = context.replace("{}", "\0").format().replace("\0", form) + "\n"
                out += f'\n[[case]]\nid = "{name}-{f:02}-{c:02}"\ninput = {toml_string(text)}\n'
                count += 1
    target = ROOT / "conformance/cases/latex-reader/rules.toml"
    target.write_text(out)
    print(f"wrote {count} cases to {target.relative_to(ROOT)}; run randoc-harness record")
    out = (
        "# Error and cell matrices of `latex_diff.py matrix`. Do not edit: run the tool.\n"
        'from = "latex"\nto = []\n'
    )
    count = 0
    for name, forms, contexts, source in ERROR_MATRICES:
        for f, form in enumerate(forms, 1):
            for c, context in enumerate(contexts, 1):
                text = context.replace("{}", "\0").format().replace("\0", form) + "\n"
                out += f'\n[[case]]\nid = "{name}-{f:02}-{c:02}"\ninput = {toml_string(text)}\n'
                if source != "latex":
                    out += f'from = "{source}"\n'
                count += 1
    target = ROOT / "conformance/cases/latex-reader/errors.toml"
    target.write_text(out)
    print(f"wrote {count} cases to {target.relative_to(ROOT)}; run randoc-harness record")


def toml_string(text):
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'


def probe_slug(name):
    if re.fullmatch(r"[A-Za-z0-9]+", name):
        return name
    if re.fullmatch(r"[A-Za-z0-9]+\*", name):
        return name[:-1] + "-star"
    return "sym-" + "-".join(f"{ord(c):x}" for c in name)


NAMES = ROOT / "crates/randoc-latex-reader/tools/names.tsv"
CONTEXTS = ["para", "alone", "item", "cell", "argument", "optional", "star", "rawtex"]


def work_list():
    rows = []
    for line in NAMES.read_text().split("\n"):
        if not line or line.startswith("#"):
            continue
        name, kind, args = line.split("\t")
        rows.append((name, kind == "env", "{word}" if args == "-" else args))
    return rows


def probe_list():
    for name, environment, args in work_list():
        probe(name, environment, args)


def write_table():
    report = json.loads((ROOT / "target/harness/report.json").read_text())
    status = {unit["id"]: unit["status"] for unit in report["units"]}
    source = (ROOT / "crates/randoc-latex-reader/src/parse.rs").read_text()
    literals = set(re.findall(r'"((?:[^"\\]|\\.)*)"', source))
    literals = {text.replace("\\\\", "\\") for text in literals}
    counts = collections.Counter()
    lines = []
    for rank, (name, environment, _) in enumerate(work_list(), 1):
        prefix = "env" if environment else "cmd"
        ids = [f"latex-reader/probes/{prefix}-{probe_slug(name)}-{c}#read" for c in CONTEXTS]
        results = [status.get(i) for i in ids]
        passed = sum(r == "pass" for r in results)
        known = name in literals or ("\\" + name) in literals
        if None in results:
            state = "not probed"
        elif passed == len(ids):
            state = "equal"
        elif not known:
            state = "not started"
        elif passed:
            state = "partial"
        else:
            state = "different"
        counts[state] += 1
        shown = ("{" + name + "}") if environment else ("\\" + name)
        lines.append(f"| {rank} | `{shown}` | {state} | {passed}/{len(ids)} |")
    out = [
        "# Work list of the LaTeX reader",
        "",
        "Made by `tools/latex_diff.py table` from the probe results. Do not edit.",
        "The order is the estimated frequency in real documents. A state",
        "counts the probe contexts that give the bytes of Pandoc 3.13:",
        "equal (all), not started (not all, and no string of src/parse.rs",
        "names it), partial (some), different (none).",
        "",
        "Counts: " + ", ".join(f"{state} {counts[state]}" for state in
                               ["equal", "partial", "different", "not started", "not probed"]),
        "",
        "| rank | name | state | contexts equal |",
        "| --- | --- | --- | --- |",
        *lines,
    ]
    target = ROOT / "crates/randoc-latex-reader/COMMANDS.md"
    target.write_text("\n".join(out) + "\n")
    print(", ".join(f"{state} {counts[state]}" for state in counts))


def probe(name, environment, args):
    if environment:
        use = f"\\begin{{{name}}}{args}\nbody text\n\\end{{{name}}}"
        star = f"\\begin{{{name}*}}{args}\nbody text\n\\end{{{name}*}}"
    else:
        use = f"\\{name}{args}"
        star = f"\\{name}*{args}"
    contexts = {
        "para": f"Before {use} after.\n",
        "alone": f"{use}\n",
        "item": f"\\begin{{itemize}}\n\\item {use}\n\\end{{itemize}}\n",
        "cell": f"\\begin{{tabular}}{{ll}}\n{use} & x\\\\\n\\end{{tabular}}\n",
        "argument": f"\\emph{{{use}}}\n",
        "optional": f"\\{name}[opt]{args}\n" if not environment else f"\\begin{{{name}}}[opt]{args}\nb\n\\end{{{name}}}\n",
        "star": f"{star}\n",
        "rawtex": f"Before {use} after.\n",
    }
    existing = PROBES.read_text() if PROBES.exists() else (
        "# Probes of `latex_diff.py probe`: one command or environment in each context.\n"
        'from = "latex"\nto = []\n'
    )
    slug = probe_slug(name)
    added = 0
    for context, text in contexts.items():
        case_id = f"{'env' if environment else 'cmd'}-{slug}-{context}"
        if f'id = "{case_id}"' in existing:
            continue
        existing += f"\n[[case]]\nid = {toml_string(case_id)}\ninput = {toml_string(text)}\n"
        if context == "rawtex":
            existing += 'from = "latex+raw_tex"\n'
        added += 1
    PROBES.write_text(existing)
    print(f"added {added} cases to {PROBES.relative_to(ROOT)}; run randoc-harness record")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    f = commands.add_parser("freq")
    f.add_argument("--top", type=int, default=100)
    d = commands.add_parser("diff")
    d.add_argument("--count", type=int, default=100)
    d.add_argument("--seed", type=int, default=1)
    d.add_argument("--max-items", type=int, default=3)
    c = commands.add_parser("cases")
    c.add_argument("--count", type=int, default=200)
    c.add_argument("--seed", type=int, default=7)
    commands.add_parser("matrix")
    commands.add_parser("probe-list")
    commands.add_parser("table")
    p = commands.add_parser("probe")
    p.add_argument("name")
    p.add_argument("--env", action="store_true")
    p.add_argument("--args", default="{word}")
    a = parser.parse_args()
    if a.command == "freq":
        freq(a.top)
    elif a.command == "cases":
        write_cases(a.count, a.seed)
    elif a.command == "probe-list":
        probe_list()
    elif a.command == "table":
        write_table()
    elif a.command == "matrix":
        write_matrix()
    elif a.command == "diff":
        diff(a.count, a.seed, a.max_items)
    else:
        probe(a.name, a.env, a.args)


if __name__ == "__main__":
    main()
