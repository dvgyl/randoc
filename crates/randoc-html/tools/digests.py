#!/usr/bin/env python3
"""Digests of oracle output that must not be stored as text.

Some options make Pandoc 3.13 write a script of its own: the script of
`--email-obfuscation=javascript` and the loader script of `--katex`. A
stored result must not hold such a script. `record` runs each case through
`pandoc lua tools/write_json.lua` and writes `src/digests.json`: the case, the
SHA-256 of the output and its length in bytes, never the output text. The
unit test `digests::the_writer_output_has_the_oracle_digests`
compares the Randoc output with these values.

Usage:
  python3 tools/digests.py record
"""
import hashlib, json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "src", "digests.json")
P = os.environ["RANDOC_ORACLE_PANDOC"]

def doc(blocks, meta=None):
    return json.dumps({"pandoc-api-version": [1, 23, 1, 2], "meta": meta or {}, "blocks": blocks})


def s(text):
    return {"t": "Str", "c": text}


MAIL = [{"t": "Para", "c": [s("Write"), {"t": "Space"}, s("to"), {"t": "Space"},
                            {"t": "Link", "c": [["", [], []], [s("box@site.test")], ["mailto:box@site.test", ""]]},
                            {"t": "Space"}, {"t": "Link", "c": [["", [], []], [s("the"), {"t": "Space"}, s("desk")],
                                                                ["mailto:desk@site.test?subject=Hi", ""]]}]}]
MATH = [{"t": "Para", "c": [s("Then"), {"t": "Space"}, {"t": "Math", "c": [{"t": "InlineMath"}, "a+b"]}]},
        {"t": "Para", "c": [{"t": "Math", "c": [{"t": "DisplayMath"}, "\\sum_i x_i"]}]}]
# An own probe template: only the variable that holds the loader script.
PROBE = "[$math$]"


def email_link(label, address, classes=None, identifier=""):
    return {"t": "Link", "c": [[identifier, classes or [], []], label, ["mailto:" + address, ""]]}


EMAIL_CASES = {
    "autolink": [{"t": "Para", "c": [email_link([s("foo@bar.com")], "foo@bar.com", ["email"])]}],
    "mixed case": [{"t": "Para", "c": [email_link([s("A.B+9@x-y.ORG")], "A.B+9@x-y.ORG", ["email"])]}],
    "label": [{"t": "Para", "c": [email_link([s("mail"), {"t": "Space"}, s("me")], "a@b.c")]}],
    "markup and subject": [{"t": "Para", "c": [email_link(
        [{"t": "Emph", "c": [s("mail")]}, {"t": "Space"}, s("me")], "a@b.c?subject=hi", ["red"], "m"
    )]}],
    "heading": [{"t": "Header", "c": [1, ["foobar.com", [], []], [
        email_link([s("foo@bar.com")], "foo@bar.com", ["email"])
    ]]}],
}


def cases():
    for fmt in ["html", "html4"]:
        yield f"email {fmt}", fmt, {"email": "javascript"}, doc(MAIL), None
        for name, blocks in EMAIL_CASES.items():
            yield f"email {name} {fmt}", fmt, {"email": "javascript"}, doc(blocks), None
        for url in ["https://cdn.jsdelivr.net/npm/katex@0.16.21/dist/", "https://files.test/k/"]:
            opts = {"katex": url, "variables": {"pagetitle": "Fixed"}}
            yield f"katex {url} {fmt}", fmt, opts, doc(MATH), PROBE


def record():
    todo = list(cases())
    requests = "".join(json.dumps({"doc": d, "fmt": f, "opts": o, "template": t}) + "\n"
                       for _, f, o, d, t in todo)
    run = subprocess.run([P, "lua", os.path.join(HERE, "write_json.lua")], input=requests.encode(),
                         capture_output=True, timeout=60, check=True)
    lines = run.stdout.decode().splitlines()
    assert len(lines) == len(todo), run.stderr.decode()
    records = []
    for (name, fmt, opts, d, template), line in zip(todo, lines):
        result = json.loads(line)
        assert "out" in result, result
        out = result["out"].encode()
        record = {"id": name, "format": fmt, "opts": opts, "doc": d, "template": template,
                  "sha256": hashlib.sha256(out).hexdigest(), "length": len(out)}
        records.append(record)
    with open(DATA, "w") as f:
        json.dump({"oracle": "pandoc 3.13", "cases": records}, f, indent=1)
        f.write("\n")
    print(f"{len(records)} cases")


if __name__ == "__main__":
    {"record": record}[sys.argv[1]]()
