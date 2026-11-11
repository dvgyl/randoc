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


def toml_string(text):
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'


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
    slug = re.sub(r"[^A-Za-z0-9]+", "-", name).strip("-") or "sym"
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
    p = commands.add_parser("probe")
    p.add_argument("name")
    p.add_argument("--env", action="store_true")
    p.add_argument("--args", default="{word}")
    a = parser.parse_args()
    if a.command == "freq":
        freq(a.top)
    elif a.command == "cases":
        write_cases(a.count, a.seed)
    elif a.command == "diff":
        diff(a.count, a.seed, a.max_items)
    else:
        probe(a.name, a.env, a.args)


if __name__ == "__main__":
    main()
