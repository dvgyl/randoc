#!/usr/bin/env python3
"""The oracle matrix of math in e-mail links and of raw formula source.

Each formula is a paragraph with the formula in a `mailto:` link and the
formula alone, for each math method, with `--email-obfuscation=references`
and without obfuscation, in `html` and `html4`, through
`pandoc lua tools/write_json.lua`. `record` writes `src/math_email.jsonl`:
`[formula, format, options, document JSON, output]`. The unit test
`html::tests::the_math_email_matrix_has_no_difference` replays it.

Usage:
  python3 tools/math_email.py record
"""
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "src", "math_email.jsonl")
P = os.environ["RANDOC_ORACLE_PANDOC"]

FORMULAS = ["x", "\\text{a@b}", "a\"b'c\u21a9<d&", "a < b > c", "\\frac{1}{2}", "\u21a9", "\\text{\u21a9}",
            "x^2 + y_i", "\\mathrm{d}x", "\\z", "a&b", "\\{ q \\}"]
METHODS = ["mathjax", "katex", "mathml", "plain", "gladtex", "webtex"]


def doc(tex, display):
    kind = {"t": "DisplayMath"} if display else {"t": "InlineMath"}
    math = {"t": "Math", "c": [kind, tex]}
    link = {"t": "Link", "c": [["", [], []], [math], ["mailto:a@b.test", ""]]}
    return json.dumps({"pandoc-api-version": [1, 23, 1, 2], "meta": {},
                       "blocks": [{"t": "Para", "c": [link, {"t": "Space"}, math]}]})


def cases():
    for tex in FORMULAS:
        for display in [False, True]:
            for method in METHODS:
                for email in ["references", "none"]:
                    for fmt in ["html", "html4"]:
                        yield tex, fmt, {"math": method, "email": email}, doc(tex, display)


def record():
    todo = list(cases())
    requests = "".join(json.dumps({"doc": d, "fmt": f, "opts": o}) + "\n" for _, f, o, d in todo)
    run = subprocess.run([P, "lua", os.path.join(HERE, "write_json.lua")], input=requests.encode(),
                         capture_output=True, timeout=60, check=True)
    lines = run.stdout.decode().splitlines()
    assert len(lines) == len(todo), run.stderr.decode()
    with open(DATA, "w") as f:
        for (tex, fmt, opts, d), line in zip(todo, lines):
            result = json.loads(line)
            assert "out" in result, (tex, result)
            f.write(json.dumps([tex, fmt, opts, d, result["out"]], ensure_ascii=False) + "\n")
    print(f"{len(todo)} cases")


if __name__ == "__main__":
    {"record": record}[sys.argv[1]]()
