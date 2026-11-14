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
