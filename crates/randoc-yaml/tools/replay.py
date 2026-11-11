#!/usr/bin/env python3
"""Makes the replay data of randoc-yaml: YAML texts and the oracle result for each.

Usage:
    replay.py OUT.json [--seed N] [--block N] [--file N] [--no-fixed]
    replay.py --exact DATA.json      (makes the exact data of each case again)
    replay.py --append DATA.json     (adds the fixed texts that DATA.json does not have)
    replay.py --changed DATA.json OUT.json BLOCK FILE
                                     (changed texts of the accepted cases of DATA.json)

The default writes crates/randoc-yaml/tests/replay.json: the fixed cases and
the generated cases. A larger run with another seed is the differential test:
    replay.py "$TMPDIR/yaml-diff.json" --seed 7 --block 3000 --file 500 --no-fixed
    RANDOC_YAML_REPLAY="$TMPDIR/yaml-diff.json" cargo test -p randoc-yaml --test replay

The oracle is $RANDOC_ORACLE_PANDOC. Metadata blocks go through `pandoc lua`
(tools/oracle.lua) in batches. Metadata files need one process for each text.
"""

import itertools
import json
import os
import random
import re
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
PANDOC = os.environ["RANDOC_ORACLE_PANDOC"]
FILE_NAME = "m.yaml"


def all_text(x):
    """The text of each Str, Code and Space in a JSON tree, in order."""
    if isinstance(x, list):
        return "".join(all_text(y) for y in x)
    if isinstance(x, dict):
        t = x.get("t")
        if t == "Str":
            return x["c"]
        if t in ("Space", "SoftBreak", "LineBreak"):
            return " "
        if t == "Code":
            return x["c"][1]
        return all_text(x.get("c", []))
    return ""


def inline_text(inlines):
    out = []
    for x in inlines:
        if x["t"] == "Str":
            out.append(x["c"])
        elif x["t"] == "Space":
            out.append(" ")
        elif x["t"] == "SoftBreak":
            out.append("\n")
        else:
            return None
    return "".join(out)


def compact(v):
    t, c = v["t"], v.get("c")
    if t == "MetaMap":
        return {"m": {k: compact(x) for k, x in c.items()}}
    if t == "MetaList":
        return {"l": [compact(x) for x in c]}
    if t == "MetaBool":
        return {"b": c}
    if t == "MetaString":
        return {"s": c}
    if t == "MetaInlines":
        text = inline_text(c)
        return {"i": text} if text is not None else {"i?": all_text(c)}
    if t == "MetaBlocks":
        paras = []
        for b in c:
            text = inline_text(b["c"]) if b["t"] in ("Para", "Plain") else None
            if text is None:
                return {"k?": all_text(c)}
            paras.append(text)
        return {"k": paras}
    raise ValueError(t)


def meta_of(doc_json):
    doc = json.loads(doc_json)
    meta = doc["meta"]
    return {"meta": {k: compact(v) for k, v in meta.items()}, "blocks": len(doc["blocks"])}


def run_blocks(texts):
    results = []
    for start in range(0, len(texts), 400):
        batch = texts[start:start + 400]
        with tempfile.TemporaryDirectory(prefix="yaml-replay-") as d:
            src, out = os.path.join(d, "in.json"), os.path.join(d, "out.json")
            with open(src, "w") as f:
                json.dump(batch, f)
            subprocess.run([PANDOC, "lua", os.path.join(HERE, "oracle.lua"), src, out],
                           check=True, timeout=600)
            with open(out) as f:
                raw = json.load(f)
        for r in raw:
            if r.startswith("err "):
                results.append({"err": r[4:]})
            else:
                results.append({"ok": meta_of(r[3:])})
        print("block", start + len(batch), "of", len(texts), file=sys.stderr)
    return results


LINE_BREAK = re.compile(r"\r\n|\r|\n")


def is_marker(line, marker):
    return line.startswith(marker) and line[3:4] in ("", " ", "\t")


def defaults_file(text, key):
    """A defaults file with the first document of `text` below `key`. The directives
    stay before the document, and each line of the document is indented by two spaces."""
    bom = "\ufeff" if text.startswith("\ufeff") else ""
    lines = LINE_BREAK.split(text[len(bom):])
    if lines and lines[-1] == "":
        lines.pop()
    head = 0
    while head < len(lines) and (lines[head].startswith("%") or lines[head].strip(" \t") == ""
                                 or lines[head].lstrip(" \t").startswith("#")):
        head += 1
    directives, body = [], lines
    if head < len(lines) and is_marker(lines[head], "---"):
        directives = [line for line in lines[:head] if line.startswith("%")]
        rest = lines[head][3:].strip(" \t")
        body = ([rest] if rest else []) + lines[head + 1:]
    for end, line in enumerate(body):
        if is_marker(line, "---") or is_marker(line, "..."):
            body = body[:end]
            break
    out = bom + "".join(d + "\n" for d in directives) + ("---\n" if directives else "")
    return out + key + ":\n" + "".join(("  " + line if line else "") + "\n" for line in body)


def run_literal(texts, key="variables"):
    """The value of each text below the key `key` of a defaults file, or None."""
    results = []
    for start in range(0, len(texts), 400):
        batch = [defaults_file(t, key) for t in texts[start:start + 400]]
        with tempfile.TemporaryDirectory(prefix="yaml-replay-") as d:
            src, out = os.path.join(d, "in.json"), os.path.join(d, "out.json")
            with open(src, "w") as f:
                json.dump(batch, f)
            subprocess.run([PANDOC, "lua", os.path.join(HERE, "literal.lua"), src, out,
                            os.path.join(d, "defaults.yaml"), key], check=True, timeout=600)
            with open(out) as f:
                raw = json.load(f)
        results += [json.loads(r[3:]) if r.startswith("ok ") else None for r in raw]
        print("literal", start + len(batch), "of", len(texts), file=sys.stderr)
    return results


def same_shape(literal, compact_value):
    """True when the literal value has the keys, the lists and the booleans of the
    metadata value: then the indentation did not change the meaning of the text."""
    if "m" in compact_value:
        return (isinstance(literal, dict) and literal.keys() == compact_value["m"].keys()
                and all(same_shape(literal[k], v) for k, v in compact_value["m"].items()))
    if "l" in compact_value:
        return (isinstance(literal, list) and len(literal) == len(compact_value["l"])
                and all(same_shape(a, b) for a, b in zip(literal, compact_value["l"])))
    if "b" in compact_value:
        return literal is compact_value["b"]
    return literal is None or isinstance(literal, str)


def add_literal(cases):
    """Adds the exact strings of each accepted text: the key `literal` from the
    `variables` of a defaults file, which keep leading spaces, and the key `strings`
    from its `metadata`, which keep each line break at the end."""
    accepted = [c for c in cases if "ok" in c]
    texts = [c["yaml"] for c in accepted]
    for c, lit, strings in zip(accepted, run_literal(texts), run_literal(texts, "metadata")):
        if lit is not None and same_shape(lit, {"m": c["ok"]["meta"]}):
            c["literal"] = lit
            if strings is not None:
                c["strings"] = strings


def run_file(text):
    with tempfile.TemporaryDirectory(prefix="yaml-replay-") as d:
        with open(os.path.join(d, FILE_NAME), "wb") as f:
            f.write(text.encode("utf-8"))
        try:
            r = subprocess.run([PANDOC, "--metadata-file", FILE_NAME, "-t", "json"], cwd=d,
                               input=b"", capture_output=True, timeout=30)
        except subprocess.TimeoutExpired:
            return {"timeout": True}
    if r.returncode == 0:
        return {"ok": meta_of(r.stdout.decode("utf-8"))}
    err = r.stderr.decode("utf-8")
    return {"err": err[:-1] if err.endswith("\n") else err, "exit": r.returncode}


def run_files(texts):
    with ThreadPoolExecutor(max_workers=2) as pool:
        return list(pool.map(run_file, texts))


# The scalar forms. Each is a text that can stand as a value.
SCALARS = [
    "word", "two words", "  spaced  ", "~", "null", "Null", "NULL", "nULL", "", "true", "True",
    "TRUE", "tRUE", "false", "False", "FALSE", "yes", "no", "on", "off", "y", "n", "Yes", "NO",
    "0", "1", "-1", "+1", "-0", "+0", "007", "-007", "12345678901234567890",
    "9223372036854775807", "9223372036854775808", "-9223372036854775808",
    "-9223372036854775809", "123456789012345678901234567890", "0x1F", "0x1f", "0X1F", "0x",
    "0xg", "-0x1", "0o17", "0o8", "0o", "0b101", "1_000", "1,000", "1.0", "1.50", "-1.50",
    "+1.5", ".5", "5.", "-.5", "1e3", "1E3", "1e+3", "1e-3", "1.5e3", "1.5e-3", "1e", "e3",
    "1.2.3", "0.0", "-0.0", "0.0001", "0.00001", "1e-8", "1234567.0", "12345678.0",
    "0.1", "0.12345678901234567890", "100", "1e1000", "1e1001", "1e-1000", "1e-1001",
    "10e1000", "0.1e1001", "1.5e1001", "12e-1001", "100e-1002", ".inf", ".Inf", ".INF",
    "-.inf", "+.inf", ".nan", ".NaN", ".NAN", "inf", "nan", "2024-01-01", "2024-01-01T10:00:00Z",
    "12:30", "1:2:3", "a:b", "a :b", "http://x.org/a", "a#b", "a #b", "-a", "--a", "?a", "a?",
    "é", "日本語", "a\u00a0b", "x" * 300, "'single'", "\"double\"", "'it''s'",
    "\"a\\\"b\"", "\"tab\\tx\"", "\"\\u00e9\"", "\"\\x41\"", "\"\\U0001F600\"", "\"\\n\"",
    "\"\\q\"", "\"\\u12\"", "\"\\uD800\"", "\"a\\\\b\"", "\"\\/\"", "\"\\0\"", "\"\\_\"",
    "'a\\nb'", "\"\"", "''", "!!str 1", "!!str", "!!int 12", "!!int \"12\"", "!!int x",
    "!!int 1.5", "!!float 1", "!!float x", "!!float \"1.5\"", "!!bool true", "!!bool yes",
    "!!bool x", "!!null ~", "!!null x", "!!null \"\"", "!!binary aGk=", "!!timestamp 2024-01-01",
    "!foo x", "!foo 1", "! 1", "!<tag:yaml.org,2002:str> 1", "!!map x", "!!seq x", "!e!x 1",
    "&a x", "&a 1", "*b", "@x", "`x", "%x", "a, b", "[a, b]", "{a: b}", "[]", "{}", "[[]]",
    "[a, [b, [c]]]", "{a: {b: c}}", "[a: b]", "[a: b, c]", "{a, b}", "{a: }", "[a, ]",
    "[, a]", "[a b]", "{a: b, a: c}", "[*b]", "{? a: b}", "[a]x", "a]", "a}", "#x", "a: b",
]

CONTEXTS = [
    "a: {}\n",
    "- {}\n",
    "a: [{}]\n",
    "a: {{k: {}}}\n",
    "a:\n  {}\n",
    "a:\n  - {}\n",
    "{}: v\n",
    "b: &b q\na: {}\n",
]

BLOCK_BODIES = [
    "  line\n",
    "  one\n  two\n",
    "  one\n\n  two\n",
    "  one\n    more\n  two\n",
    "  one\n\n\n",
    "\n  after empty\n",
    "",
    "   three\n  two\n",
    "  # not a comment\n",
    "  tab\there\n",
]

FIXED = [
    # Mappings and indentation.
    "a: b\nc: d\n", "a:\n  b: c\n  d: e\nf: g\n", "a:\n    b: c\n    d: e\n", "a:\n  b:\n    c: d\n",
    "a:\n  b: c\n d: e\n", "a:\n  b: c\n   d: e\n", "a: b\n  c: d\n", " a: b\n c: d\n",
    "  a: b\nc: d\n", "a:\n- x\n- y\n", "a:\n  - x\n  - y\nb: z\n", "- a\n- b\n", "- - a\n  - b\n- c\n",
    "- a: b\n  c: d\n- e\n", "-  a: b\n   c: d\n", "-\n  a: b\n", "- a\nb: c\n", "a: b\n- c\n",
    "a:\n  - b\n  c: d\n", "a: - b\n", "a: b: c\n", "a: 'b': c\n", "'a': b\n\"c\": d\n",
    "a b: c\n", "a: b # c\n", "a: b# c\n", "a: b #c\n# d\ne: f\n", "# only\n", "\n\n", " \n",
    "a: |\n  x\nb: >\n  y\n", "a:\n\n  b: c\n", "a: b\n\n\nc: d\n", "a: x\n  y\n  z\n",
    "a: x\n\n  y\n", "a: x\n  # c\n  y\n", "a: x # c\n  y\n", "a: x\n  y: z\n", "a: x\ny\n",
    "a: x\n y\n", "- x\n  y\n", "a:\n  x\n  y\n", "a: [x,\n  y]\n", "a: [x,\ny]\n", "a: {x: 1,\n  y: 2}\n",
    "a: [\n  x\n  ]\n", "a: \"x\n  y\"\n", "a: \"x\n\n  y\"\n", "a: 'x\n  y'\n", "a: \"x \\\n  y\"\n",
    "a: \"unterminated\n", "a: 'unterminated\n", "a: [x\n", "a: {x: 1\n", "a: [x]]\n", "a: {x: 1}}\n",
    "a: [x] y\n", "a: \"x\" y\n", "a: 'x' y\n", "a: \"x\"y\n", "a: 'x'y\n", "a: \"x\": y\n",
    "[a]: b\n", "{a: b}: c\n", "? a\n: b\n", "a: ? b\n", "a: b\n...\n", "a: b\n---\nc: d\n",
    "a: &x 1\nb: *x\n", "a: &x [1, 2]\nb: *x\n", "a: &x {k: v}\nb: *x\nc: *x\n", "a: *x\n",
    "a: &x\n  k: v\nb: *x\n", "a: &x &y 1\n", "a: & 1\n", "a: * \n", "a: &x [*x]\n",
    "a: &x 1\na2: &x 2\nb: *x\n", "*x : 1\n", "a: [&x 1, *x]\n", "a: {&x k: v, b: *x}\n",
    "<<: {a: 1}\n", "a: &m {x: 1}\nb:\n  <<: *m\n  y: 2\n", "a:\n  <<: [{x: 1}, {y: 2}]\n",
    "a: b\na: c\n", "a: 1\nb: 2\na: 3\n", "{1: a, '1': b}\n", "{1: a, 1.0: b}\n", "{1: a, 0x1: b}\n",
    "{1.50: a, 1.5: b}\n", "{~: a, null: b}\n", "{~: a, '': b}\n", "{true: a, 'true': b}\n",
    "{true: a, True: b}\n", "{a: 1, \"a\": 2}\n", "{\"a\\\"\": 1, 'a\"': 2}\n", "{é: 1, \"\\u00e9\": 2}\n",
    "a:\n  b: 1\n  b: 2\n", "a: [{b: 1, b: 2}]\n", "{b: 1, b: 2, c: 1e1001}\n",
    "b: 1\nb: 2\na: 1e1001\n", "a: 1e1001\nb: *x\n", "{a: {b: 1, b: 2}, c: 1e1001}\n",
    "{1e1001: a}\n", "a: [1e1001]\n", "a: {b: [1, {c: 1e1001}]}\n",
    "\ta: b\n", "a:\n\tb: c\n", "a:\n  \tb: c\n", "a: [\n\tx]\n", "- \tx\n", "a:\t\tb\n", "a: \"\tx\"\n",
    "a: b\t# c\n", "a:\tb\t\n", "a: |\n\tx\n", "a: |\n  x\n\ty\n",
    "--- a: b\n", "--- [x]\n", "--- x\n", "--- |\n  x\n", "---\na: b\n", "--- # c\na: b\n",
    "a: b\n--- \nc: [\n", "a: b\n---\nc: *x\n", "a: &x b\n---\nc: *x\n", "a: b\n... x\n",
    "a: b\n...\nc: [\n", "...\n", "---\n", "--- \n---\n", "%YAML 1.2\n---\na: b\n", "a: b\n%x\n",
    "a: 'é'\n", "é: ü\n", "日本: 語\n", "a: é[\n", "é: [\n", "\"é\": [x\n", "a: \"é\\q\"\n",
    "a: x\u2028y\n", "a: \u00a0x\n", "a: x\u3000y\n",
    "a: 1\nb: !!str 2\nc: !!int \"3\"\n", "a: !!str\n", "a: !!int\n", "a: !!null\n", "a: !!bool\n",
    "a: !!float\n", "a: !!seq [x]\n", "a: !!map {x: 1}\n", "a: !!seq {x: 1}\n", "a: !!str [x]\n",
    "a: !x\n  - y\n", "a: !!str\n  - y\n", "a: !!int 0x1F\n", "a: !!int 0o17\n", "a: !!int -0\n",
    "a: !!float 1e3\n", "a: !!float .inf\n", "a: !!float .nan\n", "a: !!bool True\n",
    "a: !!bool 'true'\n", "a: !!null 'null'\n", "a: !!str yes\n", "! a: b\n", "a: !\n",
]


# Texts from the round 1 reviews of the crate, each a case of a finding.
REVIEW = [
    "a: \"\n\t\n \"\n", "a: '\n\t\n '\n", "a: \"x\\\n\t\n y\"\n", "a: [\"x\n\t\n y\"]\n", "a: \"x\n \t\n y\"\n",
    "a: !%21bool true\n", "a: !<!!bool> true\n", "a: !%21int 0x10\n", "a: !<!!int> 0x10\n", "a: !%21float word\n",
    "a: !%21map word\n", "a: !<!!str> 1\n", "a: !<!!seq> x\n",
    "a: !<tag:yaml.org,2002:bool> true\n", "a: !<tag:yaml.org,2002:int> 0x1F\n",
    "a: !<tag:yaml.org,2002:null> ~\n", "a: !<tag:yaml.org,2002:int> word\n", "a: !<x> 1\n",
    "a: !<!> c\n", "a: !<!bar> c\n", "a: !<a:b> c\n", "a: |\n  \nb: c\n", "a: |\n   \nb: c\n",
    "a: >\n  \nb: c\n", "a: |+\n  \nb: c\n", "a: \"x\\\n\n  y\"\n", "a: \"x\\\n\n\n  y\"\n",
    "a: \"x\\\n  \n  y\"\n", "a: 1" + "0" * 1001 + "\n", "a: 0x" + "f" * 832 + "\n",
    "a: 0o1" + "0" * 5000 + "\n", "a: !!int 1" + "0" * 1001 + "\n", "a: 1" + "0" * 1001 + ".0\n",
    "a: !!float 1" + "0" * 1001 + "\n", "a: !!float 0x10\n", "a: !!float 0o17\n",
    "a: !!float 1e1001\n", "&k \"a\": v\nb: w\n", "!!str \"a\": v\nb: w\n", "&k 'a': v\nb: w\n",
    "!!int \"1\": v\nb: w\n", "x:\n  !t \"a\": v\n  b: w\n", "a: &a b\nc: {*a : d}\n",
    "a: &a [b]\nc: {*a : d}\n", "a: &x b\nc: &e *x\n", "a: &x b\nc: !t *x\n", "? [a, b]\n: c\n",
    "? - a\n  - b\n: c\n", "? {a: b}\n: c\n", "? a: b\n", "? a\n\t: b\n", "k0: {k:{}}\n",
    "k0: {k0:[yes, N]}\n", "a: {k:[x]}\n", "a: !<a,b> word\n", "a: \"b\"#c\n", "a: b\nc:[]\n",
]
REVIEW_FILES = [
    "%TAG !e! !!\n---\na: !e!bool true\n", "%TAG !e! !\n---\na: !e!!int 0x10\n",
    " #\n%YAML 1.2\n---\na: b\n", "\t#\n%YAML 1.2\n---\na: b\n", "  # c\n%TAG !e! tag:yaml.org,2002:\n---\na: !e!int 0x10\n",
    "%YAML 1.2\t# c\n---\na: b\n", "%TAG !e! tag:yaml.org,2002:\t# c\n---\na: b\n", "%YAML 01.2\n---\na: b\n",
    "%YAML 001.2\n---\na: b\n", "%YAML 0001.0002\n---\na: b\n", "%YAML 1.2#c\n---\na: b\n",
    "a: b\r\nc: d\r\n", "a: |\r\n  x\r\n  y\r\n", "a: true\r\n", "a: [word]\r\n", "a: b\rc: d\r",
    "%TAG !e! tag:yaml.org,2002:\n---\na: !e!int 0x10\n", "%YAML 9.9\n---\na: word\n",
    "%YAML 1.2\n%YAML 1.2\n---\na: b\n", "%TAG !e! a:\n%TAG !e! b:\n---\na: b\n", "%YAML\n---\na: b\n",
    "%TAG !e!\n---\na: b\n", "%TAG ! tag:yaml.org,2002:\n---\na: !int 0x10\n", "%FOO bar\n---\na: b\n",
    "a: |\t-\n word\n", "r\teview: \"word\\u004\"\n",
]


# Texts of the round 3 low rows: NaN keys, explicit keys in a flow sequence, a second
# line of properties.
REVIEW_LOW = [".nan: a\n.nan: b\n", ".nan: a\n.NaN: b\n", ".NaN: a\n.nan: b\n", ".nan: a\n.NAN: b\n", ".NaN: a\n.NaN: b\n", ".nan: a\n!!float .nan: b\n", ".nan: a\n~: b\n", ".nan: a\n'.NaN': b\n", ".nan: a\n.inf: b\n", "!!float .nan: a\n!!float .nan: b\n", ".NAN: a\n.NAN: b\n", ".nan: a\n'.nan': b\n", "{.nan: a, .NaN: b}\n", "{.nan: a, .nan: b}\n", "[{.nan: a}, {.nan: b}]\n", ".nan: a\nx: 1\n.NaN: b\n", "x: [? a]\n", "x: [? a : b]\n", "x: [?  a: b]\n", "x: [? a b]\n", "x: [?]\n", "x: [? ]\n", "x: [? , a]\n", "x: [? [a]]\n", "x: [? a, b]\n", "x: [? a:]\n", "x: [? : b]\n", "x: [?a]\n", "x: [? &x a, *x]\n", "x: [? 'a': b]\n", "x: [? {a: b}]\n", "x: [?\n  a]\n", "x: [? a\n  : b]\n", "x: [? ? a]\n", "x: [? a: b: c]\n", "x: [? !t a]\n", "&:\n  &x\n  b: c\n", "&a\n  &x\n  b: c\n", "&a:\n  &x\n  b: c\n", "&::\n  &x\n  b: c\n", "&:\n  b: c\n", "&:\n", "a: &: x\n", "&: a\n", "x:\n  &:\n    &y\n    b: c\n", "&:x\n  &y\n  b: c\n", "&a: b\n"]


def block_scalars():
    out = []
    for kind in "|>":
        for chomp in ["", "-", "+"]:
            for ind in ["", "1", "2", "3"]:
                for i, body in enumerate(BLOCK_BODIES):
                    if ind and i % 3:
                        continue
                    out.append("a: " + kind + chomp + ind + "\n" + body + "b: c\n")
                    out.append("a: " + kind + ind + chomp + " # c\n" + body)
    out += ["- |\n  x\n- >\n  y\n", "a: |x\n", "a: |0\n  x\n", "a: |10\n  x\n", "a: |--\n  x\n",
            "a: >\n  a\n  b\n\n  c\n   d\n  e\n", "a: >-\n\n  x\n\n", "a: |+\n\n", "a: >+\n  x\n\n\n",
            "a: |\n x\n", "a:\n  |\n  x\n", "a: |\n  x\n y\n", "a: |\n   x\n  y\n"]
    return out


SCALAR_END_CONTEXTS = [("", 0), ("m:\n  ", 2), ("s:\n- ", 0), ("s:\n  - ", 2)]


def scalar_end_rows(entrance):
    """The matrix of the end of a block scalar: each header, context and body, then
    one next line of each kind at each indentation from 0 to the content indentation
    plus 1, then a tail."""
    out = []
    for kind in "|>":
        for chomp in ["", "-", "+"]:
            for ind in ["", "1", "2", "7"]:
                for prefix, parent in SCALAR_END_CONTEXTS:
                    head = prefix + "a: " + kind + chomp + ind + "\n"
                    key = parent + 2 if prefix.endswith("- ") else parent
                    content = key + int(ind) if ind else key + 2
                    for body in [" " * content + "x\n", ""]:
                        for i in range(content + 2):
                            pad = " " * i
                            deep = [" " * content + "y\n", " " * (content - 1) + "y\n"]
                            nexts = {"block": [(pad + "#c\n", ["", "z: 1\n"] + deep),
                                               (pad + "b: c\n", [""]), (pad + "- y\n", [""]),
                                               (pad + "\n", ["", "z: 1\n"] + deep)],
                                     "file": [(pad + "#c\n", ["z: 1\n"]), (pad + "...\n", ["", "z: 1\n"])]}[entrance]
                            for line, tails in nexts:
                                out += [head + body + line + tail for tail in tails]
    return list(dict.fromkeys(out))


def scalar_end_cases():
    return scalar_end_rows("block")


# Nested containers, each at two depths: the text before the header and the column of
# the innermost collection.
NESTED_CONTEXTS = [("s:\n- - ", 2), ("s:\n  - - ", 4), ("s:\n- - a: ", 4), ("s:\n  - - a: ", 6),
                   ("s:\n- a:\n  - ", 2), ("s:\n  - a:\n    - ", 4), ("? a\n: - ", 2), ("s:\n  ? a\n  : - ", 4),
                   ("s:\n- - - ", 4), ("s:\n- ? a\n  : - ", 4)]


def nested_end_rows(entrance):
    """The block scalar end matrix in nested containers: a list item, a key, a comment
    and a blank line at each indentation from 0 to the content indentation plus 1."""
    out = []
    for kind in "|>":
        for chomp in ["", "-"]:
            for ind in ["", "1", "2"]:
                for prefix, inner in NESTED_CONTEXTS:
                    head = prefix + kind + chomp + ind + "\n"
                    content = inner + int(ind) if ind else inner + 2
                    for body in [" " * content + "x\n", ""]:
                        for i in range(content + 2):
                            pad = " " * i
                            deep = [" " * content + "y\n", " " * (content - 1) + "- y\n"]
                            nexts = {"block": [(pad + "#c\n", ["", "z: 1\n"] + deep), (pad + "b: c\n", [""]),
                                               (pad + "- y\n", ["", "z: 1\n"]), (pad + "\n", [""] + deep)],
                                     "file": [(pad + "- y\n", ["z: 1\n"]), (pad + "#c\n", ["- y\n"])]}[entrance]
                            for line, tails in nexts:
                                out += [head + body + line + tail for tail in tails]
    return list(dict.fromkeys(out))


# The texts of the review finding Opus yaml-R9-01.
REVIEW_R9 = ["s:\n  - - |\n      x\n    - y\n", "a:\n  - - >\n      text\n    - other\n",
             "s:\n  - - - |\n        x\n      - y\n", "? a\n: - |\n     x\n  - y\n", "- - a: |\n        x\n  - y\n",
             "s:\n  - - |2\n    - y\n", "s:\n  - - |\n      x\n    #c\n    - y\n", "s:\n  - - |\n      x\n\n    - y\n",
             "s:\n  - - !!str |\n      x\n    - y\n", "s:\n  - - &a |\n      x\n    - y\n", "s:\n  - - |\n      x\n    -\n",
             "s:\n  - - | #h\n    - y\n     &n y\nz: 1\n", "- - >\n    x\n    x y\n      - y\n  -\n- t\n",
             "- - |\n     x\n  - y\n", "- - >-\n     x\n  - y\n", "- - a: >-\n        x\n  - y\n",
             "a:\n  - - |\n       x\n    - y\n", "a:\n  - - >-\n       x\n    - y\n", "? a\n: - >-\n     x\n  - y\n"]


def window_cases():
    out = []
    for length in [79, 80, 81, 82, 85, 100, 120, 150, 200]:
        for caret in [5, 40, 41, 42, 60, 75, 79, 80, 81, 82, 90, 120, 160, 190]:
            if caret + 1 <= length:
                out.append("a" + "b" * (caret - 4) + ": [" + "x" * (length - caret) + "\n")
    out.append("a: {k0: 1, " + ", ".join("x%d: 1" % i for i in range(10)) + ", k0: 2}\n")
    out.append("a: [\u00e9" + "\u00e9" * 100 + "\n")
    out.append("a\tb: \"x\\u004\"\n")
    return out


def quote_cases():
    """Continuation lines of quoted scalars: each quote, context, indentation and escape."""
    out = []
    for q in ['"', "'"]:
        for ctx in ["a: %s%s\n%s%s\n", "a: [%s%s\n%s%s]\n", "b:\n  a: %s%s\n%s%s\n", "b:\n  - %s%s\n%s%s\n"]:
            for cont in ["y", " y", "  y", "   y", "\ty", " \ty", "  \ty", "", "  "]:
                for esc in ["", "\\"]:
                    if esc and q == "'":
                        continue
                    out.append(ctx % (q, "x" + esc, cont, q))
    return out + ["a:\n  \t''\n", "a:\n  \t{}\n", "a:\n  - {?}\n", "a:\n  ?]\n"]


def explicit_cases():
    """Explicit keys in nested mappings: key indentation, next line and depth."""
    out = ["x:\n  ? a\n  c: d\n", "x:\n  ? a\n  ? b\n", "x:\n  ? a\n  ? \n", "x:\n  ? a\n  ? b\n  : 2\n",
           "x:\n    ? a\n    ? b\n    c: d\ny: 2\n", "x:\n  ?\n  ?   a\n  :\n    b\n",
           "a:\n  - ? b\n    : c\n    ? d\n    ? d\n", "x:\n  ? a\n c: d\n", "a:\n  ? .IN\n  ? .IN\n",
           "a:\n  ? b\n : c\n", "a:\n ? b\n ? c\n", "a:\n ? b\n ? c\n ? d\n", "? x\n? y\n",
           "x:\n  ? a\n  : b\n  c: d\n"]
    for ind in [1, 2, 4]:
        for nxt in ["? k", "? k\n%s: v", "k: v", "- s"]:
            for d in [-1, 0, 1]:
                ns = " " * max(0, ind + d)
                out.append("a:\n%s? q\n%s%s\n" % (" " * ind, ns, nxt.replace("%s", ns)))
                ns = "  " + " " * max(0, d)
                out.append("s:\n- ? q\n%s%s\n" % (ns, nxt.replace("%s", ns)))
    return list(dict.fromkeys(out))


def verbatim_cases():
    """The scheme before ':' of a verbatim tag: each kind of first and later character."""
    out = []
    for scheme in ["", "x", "X", "x1", "x+y", "x-y", "x.y", "-x", "+x", ".x", "1x", "x,y", "x!",
                   "x]", "x*", "x&", "x%41", "%41x", "tag", "ta[g"]:
        out.append("a: !<%s:yaml.org,2002:str> 1\n" % scheme)
    return out + ["a: !<:> x\n", "a: !<:x> true\n", "a: !<x:> true\n", "a: !<!:x> true\n",
                  "a: !<x::y> true\n", "a: !<tag!:yaml.org,2002:float> 1.0\n"]


def properties_cases():
    """Properties alone on a line below a key, then a list, a key or a scalar."""
    out = []
    for props in ["&x", "!t", "!!seq", "&x !!seq", "!!map", "!!str", "&x !t"]:
        for k in [0, 2]:
            for p in [k, k + 1, k + 2]:
                for nxt in ["- b", "c: d", "b"]:
                    for n in [k - 1, k, k + 1]:
                        if n >= 0:
                            out.append(("x:\n" if k else "") + " " * k + "a:\n" + " " * p + props
                                       + "\n" + " " * n + nxt + "\n")
        out += ["- a:\n   %s\n  - b\n" % props, "a:\n %s\n- b\n- c\nd: e\n" % props,
                "a:\n %s\n- b\nd: e\n" % props]
    return list(dict.fromkeys(out))


def property_lines_cases():
    """Properties on their own lines: 1 or 2 property lines, then each kind of next line."""
    nexts = ["k: v", "&b k: v", "!u k: v", "- x", "word", "[x]", "{c: d}", "&b word", "!u word"]
    out = []
    for p1 in ["&x", "!t"]:
        for p2 in [None, "&y", "!u", "&y !u"]:
            for nxt in nexts:
                for ind in [(0, 0, 0), (0, 1, 1), (0, 1, 2), (0, 0, 1)]:
                    lines = [p1] + ([p2] if p2 else []) + [nxt]
                    ind = ind if p2 else (ind[0], ind[2])
                    out.append("".join(" " * i + l + "\n" for i, l in zip(ind, lines)))
                for a in [1, 2]:
                    for b in ([1, 2, 3] if p2 else [None]):
                        for n in [0, 1, 2, 3]:
                            lines = [(a, p1)] + ([(b, p2)] if p2 else []) + [(n, nxt)]
                            out.append("a:\n" + "".join(" " * i + l + "\n" for i, l in lines))
    return list(dict.fromkeys(out))


def key_identity_cases():
    """Pairs of mapping keys: each scalar kind and form, direct and through an alias."""
    forms = ["0", "-0", "+0", "00", "0x0", "0o0", "1", "0x1", "0o1", "01", "1.0", "1e0", "0.0", "-0.0", "+0.0",
             "0e0", "-0e0", ".0", "-.0", "0.00", ".nan", ".NaN", "!!float .nan", ".inf", "-.inf", ".Inf",
             "true", "True", "false", "~", "null", "Null", "''", "'0'", "'0.0'", "'.nan'", "'true'", "'~'",
             "\"1\"", "!!str 1", "!!int 1", "!!float 1", "a", "'a'"]
    out = []
    for i, a in enumerate(forms):
        for b in forms[i:]:
            out.append("%s: x\n%s: y\n" % (a, b))
            out.append("m: {%s: x, %s: y}\n" % (a, b))
            out.append("a: &a %s\n*a : x\n%s: y\n" % (a, b))
            out.append("a: &a %s\nb: {%s: x, *a : y}\n" % (b, a))
    return list(dict.fromkeys(out))


def flow_properties_cases():
    """Properties in a flow collection: each property form, separator and next token, in
    each place of a flow sequence and a flow mapping."""
    props = ["&a", "!t", "!", "!<!v>", "&a !t", "!t &a"]
    nexts = ["x", "'x'", "\"x\"", "*z", "[y]", "{y: w}", ",", "]", "}", ":", "!", "&b", "!u x", "&b x"]
    seps = ["", " ", "\n ", "\n\n  ", " #c\n "]
    places = ["a: [%s]\n", "a: [%s: v]\n", "a: [k: %s]\n", "a: {%s: v}\n", "a: {k: %s}\n"]
    out = []
    for p in props:
        for n in nexts:
            for s in seps:
                for place in places:
                    out.append("z: &z q\n" + place % (p + s + n))
    return list(dict.fromkeys(out))


def alias_properties_cases():
    """Properties in block context, then an alias or a scalar on a later line."""
    out = []
    for p in ["&a", "!t", "!", "!!str", "&a !t", "!t &a"]:
        for place in ["b: %s\n %s\n", "b: %s\n  %s\n", "- %s\n  %s\n", "b:\n  %s\n  %s\n", "%s\n%s\n",
                      "b: [%s\n  %s]\n", "b: {k: %s\n  %s}\n"]:
            for n in ["*z", "x", "*z : v"]:
                body = place % (p, n)
                out.append(("z: &z q\n" if place[0] != "-" and place[0] != "%" else "") + body)
                if place[0] == "-":
                    out.append("- &z q\n" + body)
    return list(dict.fromkeys(out))


ALIAS_KEY_NEXTS = ["", " ", "\t", " #c", "#c", "x", " x", " xv", " ,", " ]", ":", ": v", " : v", " :", "\t: v",
                   " :x", ":x", " \n  v", "\n: v"]


def alias_key_cases():
    """An alias at the column of block mapping keys, then each next character, in each
    place, with and without properties before the alias. `end` is False for the end of
    the input right after the alias line (the metadata file entrance only)."""
    places = [
        ("z: &z q\n", ""),
        ("z: &z q\nb:\n  c: d\n", "  "),
        ("z: &z q\nb:\n    c: d\n", "    "),
        ("z: &z q\nb:\n  c:\n    d: e\n", "    "),
        ("- &z q\n- c: d\n", "  "),
        ("z: &z q\nb:\n  - c: d\n", "    "),
        ("z: &z q\nb:\n", "  "),
        ("z: &z q\nb:\n", ""),
        ("z: &z q\n? c\n: d\n", ""),
    ]
    out = []
    for head, indent in places:
        for props in ["", "&p ", "!t ", "&p !t "]:
            for n in ALIAS_KEY_NEXTS:
                out.append(head + indent + props + "*z" + n + "\n")
    return list(dict.fromkeys(out))


def alias_value_cases():
    """An alias with ':' in or after its name, in each place of a value."""
    places = ["z: &z q\nb:\n  %s\n", "z: &z q\nb: %s\n", "z: &z q\nb:\n  - %s\n", "- &z q\n- %s\n",
              "z: &z q\nb:\n  %s\n  c: d\n", "z: &z q\n? %s\n", "z: &z q\nb: [%s]\n", "%s\n",
              "z: &z q\nb: {%s}\n", "z: &z q\nb: [k: %s]\n", "z: &z q\nb: [%s, y]\n", "z: &z q\nb: {%s, y: w}\n"]
    nexts = ["*z:", "*z: ", "*z: v", "*z: #c", "*z:\t", "*z:x", "*z :x", "*z :", "*z ::", "*z:x: v", "*z:x : v", "*z :,", "*z :]", "*z :}", "*z :[a]", "*z :\"a\""]
    return [place % n for place in places for n in nexts]


def alias_below_properties_cases():
    """Properties alone on a line, then an alias at each column, with and without text after it."""
    heads = ["a: &a x\nb:\n  %s\n", "a: &a x\nb:\n    %s\n", "a: &a x\nb:\n  - %s\n", "a: &a x\nb: %s\n",
             "- &a x\n- %s\n"]
    out = []
    for p in ["&c", "!t", "&c !t"]:
        for head in heads:
            for indent in range(5):
                for n in ["", " y", " #c", " y z"]:
                    out.append(head % p + " " * indent + "*a" + n + "\n")
    return out


# Texts of the round 6 reviews: an alias at the column of keys without ':'.
REVIEW_R6 = ["a: &a x\n*a\n", "author: &me Ann\neditor:\n*me\n", "a: &a x\nb:\n  c: d\n  *a\n", "z: &z q\n*z #c\n",
             "z: &z q\n*z xv\n", "z: &z q\n*z ,\n", "a: &a x\nb:\n  &c\n *a\n", "a: &a x\nb:\n  - &c\n   *a\n",
             "a: &a x\nb: &c\n *a y\n"]


ANCHOR_NAMES = [":", "::", "a:", "a::", ":a", "a:b", "a:b:", "a.b", "a-1", "x_y", "a/b", "1", "\u65e5\u672c", "a#b", "a*", "a&",
                "a!", "a|", "a?", "a'", "a\"", "a%", "a@", "a`", "a>", "a="]
ANCHOR_NEXTS = ["", " ", ":", " :", ": v", " : v", ":x", " :x", "::", " ::", " #c", "#c", "x", " x"]
ANCHOR_FLOW_NEXTS = ANCHOR_NEXTS + [",", " ,", ":,", " :,", ": ,"]


def anchor_name_cases():
    """An anchor name with ':' or another character, then an alias of it in a key place
    and in a value place, in block and flow context, before each next text."""
    block = ["z: &%s q\n*%s%s\n", "z: &%s q\nb:\n  *%s%s\n", "z: &%s q\nb: *%s%s\n", "z: &%s q\nb:\n  - *%s%s\n",
             "z: &%s q\n? *%s%s\n"]
    flow = ["z: &%s q\nb: [*%s%s]\n", "z: &%s q\nb: [*%s%s, y]\n", "z: &%s q\nb: {*%s%s}\n", "z: &%s q\nb: {*%s%s, y: w}\n",
            "z: &%s q\nb: [k: *%s%s]\n", "z: [&%s q, *%s%s]\n"]
    out = []
    for name in ANCHOR_NAMES:
        out += [p % (name, name, n) for p in block for n in ANCHOR_NEXTS]
        out += [p % (name, name, n) for p in flow for n in ANCHOR_FLOW_NEXTS]
    return list(dict.fromkeys(out))


# Texts of the round 7 reviews: anchor names with ':', a ':' after an alias key, and
# strings with a bare carriage return or a last line of spaces.
REVIEW_R7 = ["a: [&:,*: :]\n", "a: &a: x\nb: [*a: : y]\n", "a:\n*::x\n", "a: \"\\r\"\n", "a: \"\\n \"\n",
             "z: &x: q\nb: [*x: :]\n", "z: &x: q\nb: {*x: :}\n", "z: &x: q\nb:\n  *x: :\n",
             "k: &x: q\nb:\n  c: d\n  *x: :\t\n  e: f\n", "a: &a: x\n*a: : y\n", "a: \"\\r\\n\"\nb: \"x\\r\"\nc: \"\\n\\t\"\n"]
# The low findings of round 7 (Opus L02, L03, L04) and the two classes of I-YAML-10
# from round 6 (Opus L03).
REVIEW_R7_LOW = ["z: &z q\n? &a\n  *z\n", "z: &z q\n? k\n: !t\n  *z\n", "z: &a-1 q\nb: [*a-1:}, x]\n",
                 "z: &z q\nb:\n  *z :#c\n", "z: &1 q\nb:\n  *1 :-\n", "- &z q\n- *z:\n  - li\n", "k: &z q\n? *z ::\n",
                 "k: &x_y q\n*x_y ::\n", "k: &a:b [1, 2]\n*a:b:{v: w}\nc: d\n", "k: &z\n  m: n\nb:\n  c: d\n  *z: : v\n  e: f\n",
                 "x: [\n  a,\n  y\n]\n", "x: {\n  a: b,\n  y: c\n}\n", "a: [&a\n !t x, !u\n\t!v y]\n", "- [!t &a\nx: v]\n"]
REVIEW_R7_FILES = ["a: b\n...\n", "a: b*\n...\n", "a: [&:,*: :]\n", "a: &a: x\n*a: : y\n", "a: \"\\r\"\n"]


def whitespace_string_cases():
    """Double-quoted strings of up to 3 characters of a letter and each kind of white
    space: the rule of the line breaks and the leading white space of `strings`."""
    esc = {"x": "x", " ": " ", "\t": "\\t", "\r": "\\r", "\n": "\\n", "\x0b": "\\v", "\x0c": "\\f", "\u00a0": "\\_"}
    out = []
    for n in range(1, 4):
        out += ['a: "%s"\n' % "".join(esc[c] for c in p) for p in itertools.product(esc, repeat=n)]
    return out


def alias_key_end_cases():
    return [t[:-1] for t in alias_key_cases() if not t.endswith("\n\n")]


# Texts of the round 5 reviews: properties on separate lines in a flow collection, and
# properties before an alias.
REVIEW_R5 = ["a: [&a\n !]\n", "a: [!\n &a []]\n", "a: [!\n !]\n", "a: [&a\n !t x]\n", "a: {&a\n !t key: value}\n",
             "a: [&a,!\n *a]\n", "a: &a\nb: !\n *a\n", "a: &a x\nb: !t\n *a\n", "a: &a x\nb: ! *a\n",
             "a: &a x\nb: &b *a\n", "bibliography: my_file.bib\n", "title: \"It's a test\"\n"]


# Strings with each class of character that Markdown reads, and the whitespace rule of
# the variables of a defaults file (one of two line breaks at the end is lost).
LITERAL = ["a: my_file.bib\nb: a*b\nc: \\usepackage{x}\nd: \"It's a test\"\ne: x &amp; z\nf: <br>\n",
           "a: \"`c` [l](u) # h ~x~ ^y^ $m$ @c | > ! _u_\"\nb: \"x &copy; &#65; <i>y</i>\"\n",
           "a: \"\\n\"\nb: \"\\n\\n\"\nc: \"x\\n\"\nd: \"x\\n\\n\\n\"\ne: |+\n  x\n\nf: \" x \"\ng: \"\\tx\\r\\n\"\n",
           "a: [1, 1.50, ~, true, {}, [], 0x1F, 1e3, -0.0, .inf]\n"]


# Texts of the round 4 reviews: a verbatim tag before ':', the base of a block scalar.
REVIEW_R4 = ["a: [!<!t>: x]\n", "a: {!<!t>: x}\n", "a: {? !<!t>: x}\n", "a: [!<tag:x.org,2026:a>: x]\n", "!<!t>: x\n",
             "a: [!<!t>\n  : x]\n", "a: [!<!t> : x]\n", "a: [!!str : x]\n", "x: !<!t>: y\n", "- !<!t>: x\n",
             "  |\n    x\n  y\n", "- |\n    x\n  y\n", "-\t|\n    x\n  y\n",
             "&x\n&b k: v\n", "a:\n  &x\n  &b k: v\n", "a:\n  !t\n  !!str k: v\n", "a:\n &x \n !t-b\n - c\n",
             "&a\n&b c:\n", "&a\n!b\nc: d\n", "&a41k0:\n&a41 k0:\n  - 0x10\n", "root:\n  &a\n  &b k: v\n",
             "0.0: x\n-0.0: y\n", "a: &a .nan\n*a :\n~:\n", "a: &a .nan\n*a :\n.NaN:\n",
             "a: {-0.0: first, 0e2: second}\n", "title:\"Report #4\"\n", "nocite:|\n"]


def long_cases():
    return [
        "a: " + "x" * 20000 + "\n",
        "".join("k%d: v%d\n" % (i, i) for i in range(2000)),
        "a: " + "[" * 50 + "x" + "]" * 50 + "\n",
        "a:\n" + "".join("  " * (i + 1) + "k%d:\n" % i for i in range(60)) + "  " * 61 + "x\n",
        "a: [" + ", ".join(str(i) for i in range(3000)) + "]\n",
        "a: &a " + "x" * 9999 + "\nb: [" + ",".join(["*a"] * 10) + "]\n",
        "a: &a " + "x" * 10000 + "\nb: [" + ",".join(["*a"] * 10) + "]\n",
        "a0: &a0 [x, x]\n" + "".join("a%d: &a%d [*a%d, *a%d]\n" % (i, i, i - 1, i - 1) for i in range(1, 16)),
        "a0: &a0 [x, x]\n" + "".join("a%d: &a%d [*a%d, *a%d]\n" % (i, i, i - 1, i - 1) for i in range(1, 12)),
        "p: &p " + "x" * 50000 + "\nb: [*p, *p]\n",
        "p: &p " + "x" * 50000 + "\nq: " + "y" * 49989 + "\nb: [*p, *p]\n",
        "p: &p " + "x" * 50000 + "\nq: " + "y" * 49988 + "\nb: [*p, *p]\n",
        "p: &p " + "é" * 50000 + "\nb: [*p, *p]\n",
        "p: &p {" + ", ".join("k%d: v" % i for i in range(5000)) + "}\nb: [*p, *p, *p]\n",
        "p: &" + "n" * 30 + " " + "x" * 50000 + "\nb: [*" + "n" * 30 + ", *" + "n" * 30 + "]\n",
    ]


def generated(rng):
    out = []
    for ctx in CONTEXTS:
        for s in SCALARS:
            out.append(ctx.replace("{}", s, 1).replace("{{", "{").replace("}}", "}"))
    out += block_scalars()
    return out


MUTATION_CHARS = list(" \n\t:-#[]{},'\"&*!|>?%@`\\.0e~xé") + ["  ", ": ", "- ", "\n  "]


def mutate(rng, text):
    t = text
    for _ in range(rng.randint(1, 3)):
        if not t:
            t = rng.choice(MUTATION_CHARS)
            continue
        i = rng.randrange(len(t) + 1)
        op = rng.randrange(4)
        if op == 0 and i < len(t):
            t = t[:i] + t[i + 1:]
        elif op == 1:
            t = t[:i] + rng.choice(MUTATION_CHARS) + t[i:]
        elif op == 2 and i < len(t):
            t = t[:i] + rng.choice(MUTATION_CHARS) + t[i + 1:]
        else:
            lines = t.split("\n")
            j = rng.randrange(len(lines))
            lines.insert(j, lines[j])
            t = "\n".join(lines)
    if not t.endswith("\n"):
        t += "\n"
    return t


def safe_for_block(text):
    # A line with only "---" or "..." ends the metadata block early.
    return not any(line.rstrip() in ("---", "...") for line in text.split("\n"))


def fixed_cases(base):
    """The fixed texts of the metadata block entrance and of the metadata file entrance."""
    block, files = [], []
    block += [t for t in base + long_cases() + window_cases() + quote_cases() + explicit_cases() + verbatim_cases()
              + properties_cases() + property_lines_cases() + key_identity_cases()
              + flow_properties_cases() + alias_properties_cases() + alias_key_cases() + alias_value_cases() + alias_below_properties_cases()
              + anchor_name_cases() + whitespace_string_cases()
              + LITERAL
              + REVIEW + REVIEW_LOW + REVIEW_R4 + REVIEW_R5 + REVIEW_R6 if safe_for_block(t)]
    files += [t for t in REVIEW_FILES] + ["a: !<:> x\n", "a:\n &x\n- b\n", "a:\n !!seq\n- b\nd: e\n", ".nan: a\n.NaN: b\n", "x: [? a]\n",
                                            "&:\n  &x\n  b: c\n",
                                            "&a\n&b c:\n", "0.0:\n-0.0:\n", "a: &a .nan\n*a :\n~:\n", "bibliography: my_file.bib\n",
                                            "title: \"It's a test\"\n", "a: [&a\n !t x]\n", "a: &a\nb: !\n *a\n"]
    files += [t for t in base if not safe_for_block(t)] + alias_key_end_cases()
    files += ["", "x\n", "[a]\n", "~\n", "\ufeffa: b\n", "a: b\n---\nc: [\n", "a: b\n---\nc: *x\n",
              "--- a: b\n", "a: 1e1001\n", "- a\n", "'a'\n", "a: b\n...\n---\nc: d\n"]
    block += REVIEW_R7 + REVIEW_R7_LOW
    files += REVIEW_R7_FILES
    block += scalar_end_cases() + marker_key_cases()
    files += scalar_end_rows("file")[::4]
    block += nested_end_rows("block")[::24] + REVIEW_R9
    files += nested_end_rows("file")[::48] + REVIEW_R9
    return list(dict.fromkeys(block)), list(dict.fromkeys(files))


def append_fixed(path):
    """Adds each fixed text that a data file does not have yet, with its oracle result."""
    with open(path) as f:
        cases = json.load(f)
    have = {(c["entrance"], c["yaml"]) for c in cases}
    block, files = fixed_cases(FIXED + generated(random.Random(1)))
    block = [t for t in block if ("block", t) not in have]
    files = [t for t in files if ("file", t) not in have]
    new = [{"entrance": "block", "yaml": t, **r} for t, r in zip(block, run_blocks(block))]
    new += [{"entrance": "file", "yaml": t, **r} for t, r in zip(files, run_files(files))]
    add_literal(new)
    write_cases(path, cases + new)


# One character of each class: a letter, a digit, punctuation, a Markdown character,
# white space and a line break.
CHANGE_CHARS = {"letter": "q", "digit": "7", "punctuation": ";", "markdown": "*", "space": " ", "break": "\n"}


def place(text, i):
    """The place of the character at `i`: in a list, in a key or in a value."""
    start = text.rfind("\n", 0, i) + 1
    line = text[start:text.find("\n", i) if "\n" in text[i:] else len(text)]
    if line.lstrip().startswith("- ") or text.count("[", 0, i) > text.count("]", 0, i):
        return "list"
    colon = line.find(": ")
    return "key" if colon < 0 or i - start < colon else "value"


def style(text):
    for mark, name in (("\"", "double"), ("'", "single"), ("|", "literal"), (">", "folded")):
        if mark in text:
            return name
    return "plain"


def changed_texts(rng, text):
    """For each place of the text, one changed, one added and one removed character of
    each class."""
    by_place = {}
    for i in range(len(text)):
        by_place.setdefault(place(text, i), []).append(i)
    out = []
    for positions in by_place.values():
        i = rng.choice(positions)
        for c in CHANGE_CHARS.values():
            out += [text[:i] + c + text[i + 1:], text[:i] + c + text[i:]]
        out.append(text[:i] + text[i + 1:])
    return out


MARKER_KEYS = ["---", "...", "---x", "...x", "----", "....", "--- x", "... x"]


def marker_key_cases():
    """Keys that start with a document marker, each with values of several kinds."""
    out = []
    for k in MARKER_KEYS:
        out += [k + ":\n", k + ": x\n", k + ": 0.1\n", k + ": 12\n", "a: b\n" + k + ": [x, y]\n",
                k + ":\n  - 1\n", "m:\n  " + k + ": x\n", k + ": |\n  t\n", k + ": {c: d}\n"]
    return out


def kind_changes(rng, text):
    """Changed texts of five kinds: the digits of a number, a key, the value under a
    key that starts with a document marker, a removed line, the indentation of a line."""
    out = []
    for m in re.finditer(r"\d+", text):
        a, b = m.span()
        i = rng.randrange(a, b)
        d = str((int(text[i]) + rng.randrange(1, 10)) % 10)
        out += [("digits", text[:i] + d + text[i + 1:]), ("digits", text[:b] + d + text[b:]),
                ("digits", text[:i] + text[i + 1:])]
        if "." in text[max(0, a - 1):a]:
            out.append(("digits", text[:b] + "0000000000000001" + text[b:]))
    lines = text.split("\n")
    body = [j for j, line in enumerate(lines) if line.strip()]
    for j in body:
        line = lines[j]
        m = re.match(r"(\s*(?:- )?)([^\s:#][^:#]*?)(:(?: |$).*)", line)
        if m:
            new = rng.choice(["b", "a1", "A", "a ", "---", "...", "--- x", "aa"])
            if new != m.group(2):
                out.append(("key", "\n".join(lines[:j] + [m.group(1) + new + m.group(3)] + lines[j + 1:])))
            if m.group(2).startswith(("---", "...")):
                for value in [": q", ": x y", ": 0.10000000000000001", ":", ": [x]", ": ~"]:
                    if value != m.group(3):
                        out.append(("marker value", "\n".join(lines[:j] + [m.group(1) + m.group(2) + value]
                                                               + lines[j + 1:])))
    if len(body) > 1:
        j = rng.choice(body)
        out.append(("line", "\n".join(lines[:j] + lines[j + 1:])))
    for j in rng.sample(body, min(2, len(body))):
        out.append(("indentation", "\n".join(lines[:j] + [" " + lines[j]] + lines[j + 1:])))
        if lines[j].startswith(" "):
            out.append(("indentation", "\n".join(lines[:j] + [lines[j][1:]] + lines[j + 1:])))
    return out


def kinds_campaign(data_path, out_path, n_block, n_file):
    """Adds changed texts of the five kinds of `kind_changes` to the campaign file."""
    with open(data_path) as f:
        cases = json.load(f)
    with open(out_path) as f:
        done = json.load(f)
    rng = random.Random(9)
    seen = {(c["entrance"], c["yaml"]) for c in done}
    pairs = {"block": [], "file": []}
    for entrance, limit in (("block", n_block), ("file", n_file)):
        fixtures = [c["yaml"] for c in cases if c["entrance"] == entrance and "ok" in c and c["ok"]["meta"]
                    and "literal" in c and len(c["yaml"]) < 200]
        marked = [t for t in fixtures if re.search(r"(^|\n)\s*(- )?(---|\.\.\.)", t)]
        rng.shuffle(fixtures)
        for original in marked + fixtures:
            for kind, t in kind_changes(rng, original):
                ok = t != original and t.split("\n")[0].strip() != "" and (
                    entrance == "file" or (safe_for_block(t) and t.endswith("\n")))
                if ok and (entrance, t) not in seen and len(pairs[entrance]) < limit:
                    seen.add((entrance, t))
                    pairs[entrance].append((kind, original, t))
    out = []
    texts = [t for _, _, t in pairs["block"]]
    out += [{"entrance": "block", "kind": k, "from": o, "yaml": t, **r}
            for (k, o, t), r in zip(pairs["block"], run_blocks(texts))]
    texts = [t for _, _, t in pairs["file"]]
    out += [{"entrance": "file", "kind": k, "from": o, "yaml": t, **r}
            for (k, o, t), r in zip(pairs["file"], run_files(texts))]
    add_literal(out)
    write_cases(out_path, done + out)


def changed_campaign(data_path, out_path, n_block, n_file):
    """Changed texts of accepted fixtures with their oracle results. Each case names its
    original in `from`."""
    with open(data_path) as f:
        cases = json.load(f)
    rng = random.Random(8)
    fixtures = {e: [c for c in cases if c["entrance"] == e and "ok" in c and c["ok"]["meta"]
                    and len(c["yaml"]) < 200] for e in ("block", "file")}
    by_style = {}
    for c in fixtures["block"]:
        by_style.setdefault(style(c["yaml"]), []).append(c)
    pairs, seen = {"block": [], "file": []}, set()

    def add(entrance, original, limit):
        for t in changed_texts(rng, original):
            ok = t != original and t.split("\n")[0].strip() != "" and (entrance == "file" or (safe_for_block(t) and t.endswith("\n")))
            if ok and (entrance, t) not in seen and len(pairs[entrance]) < limit:
                seen.add((entrance, t))
                pairs[entrance].append((original, t))

    styles = sorted(by_style)
    while len(pairs["block"]) < n_block:
        group = by_style[styles[len(pairs["block"]) % len(styles)]]
        add("block", rng.choice(group)["yaml"], n_block)
    files = [c["yaml"] for c in fixtures["file"] if "literal" in c]
    while len(pairs["file"]) < n_file:
        add("file", rng.choice(files), n_file)
    out = []
    block_texts = [t for _, t in pairs["block"]]
    out += [{"entrance": "block", "from": o, "yaml": t, **r} for (o, t), r in zip(pairs["block"], run_blocks(block_texts))]
    file_texts = [t for _, t in pairs["file"]]
    out += [{"entrance": "file", "from": o, "yaml": t, **r} for (o, t), r in zip(pairs["file"], run_files(file_texts))]
    add_literal(out)
    write_cases(out_path, out)


def main():
    args = sys.argv[1:]
    if args[0] == "--exact":
        return exact_again(args[1])
    if args[0] == "--cases":
        texts = [t for t in globals()[args[1]]() if safe_for_block(t)]
        cases = [{"entrance": "block", "yaml": t, **r} for t, r in zip(texts, run_blocks(texts))]
        add_literal(cases)
        return write_cases(args[2], cases)
    if args[0] == "--append":
        return append_fixed(args[1])
    if args[0] == "--changed":
        return changed_campaign(args[1], args[2], int(args[3]), int(args[4]))
    if args[0] == "--kinds":
        return kinds_campaign(args[1], args[2], int(args[3]), int(args[4]))
    out_path = args[0]
    seed = int(args[args.index("--seed") + 1]) if "--seed" in args else 1
    n_block = int(args[args.index("--block") + 1]) if "--block" in args else 400
    n_file = int(args[args.index("--file") + 1]) if "--file" in args else 250
    rng = random.Random(seed)
    base = FIXED + generated(rng)
    pool = base + [s for s in SCALARS]
    block, files = [], []
    if "--no-fixed" not in args:
        block, files = fixed_cases(base)
    while len(block) < (len(base) if "--no-fixed" not in args else 0) + n_block:
        t = mutate(rng, rng.choice(pool))
        if safe_for_block(t):
            block.append(t)
    target = len(files) + n_file
    while len(files) < target:
        files.append(mutate(rng, rng.choice(pool)))
    cases = []
    for t, r in zip(block, run_blocks(block)):
        cases.append({"entrance": "block", "yaml": t, **r})
    for t, r in zip(files, run_files(files)):
        cases.append({"entrance": "file", "yaml": t, **r})
    add_literal(cases)
    write_cases(out_path, cases)


def write_cases(path, cases):
    with open(path, "w") as f:
        f.write("[\n" + ",\n".join(json.dumps(c, ensure_ascii=False) for c in cases) + "\n]\n")
    print("wrote", len(cases), "cases to", path, file=sys.stderr)


def exact_again(path):
    """Makes the exact data (`literal` and `strings`) of each case of a data file again."""
    with open(path) as f:
        cases = json.load(f)
    for c in cases:
        c.pop("literal", None)
        c.pop("strings", None)
    add_literal(cases)
    write_cases(path, cases)


if __name__ == "__main__":
    main()
