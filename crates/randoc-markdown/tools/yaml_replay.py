#!/usr/bin/env python3
"""Makes the YAML replay data of the Markdown reader.

Usage: yaml_replay.py OUT.json

The texts are the metadata block texts of the replay data of randoc-yaml
(crates/randoc-yaml/tests/replay.json, entrance "block") and the metadata
blocks of conformance/cases/markdown. For each text, the oracle reads
"---\\nTEXT---\\n" as `markdown` (tools/yaml_replay.lua, `pandoc lua` in
batches of 400). The tabs of the document are expanded first with a tab
stop of 4, as the program does before it reads. Each record is {"yaml", "err"} or {"yaml", "meta", "blocks"}:
"meta" is the JSON metadata of the document and "blocks" the number of blocks.
The test `yaml_replay_matches_the_oracle` of the crate reads OUT.json from
crates/randoc-markdown/tests/yaml_replay.json.
"""
import json
import os
import subprocess
import sys
import tempfile
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
PANDOC = os.environ["RANDOC_ORACLE_PANDOC"]


def is_end(line):
    return line.rstrip(" \t\r") in ("---", "...")


def case_texts():
    """The text of each metadata block at the start of a case input."""
    folder = os.path.join(ROOT, "conformance", "cases", "markdown")
    out = []
    for name in sorted(os.listdir(folder)):
        if name.startswith("._") or not name.endswith(".toml"):
            continue
        with open(os.path.join(folder, name), "rb") as f:
            data = tomllib.load(f)
        for case in data.get("case", []):
            lines = case.get("input", "").split("\n")
            if not lines or lines[0].rstrip() != "---" or len(lines) < 2 or not lines[1].strip():
                continue
            body = []
            for line in lines[1:]:
                if is_end(line):
                    out.append("".join(l + "\n" for l in body))
                    break
                body.append(line)
    return out


def yaml_texts():
    with open(os.path.join(ROOT, "crates", "randoc-yaml", "tests", "replay.json")) as f:
        return [c["yaml"] for c in json.load(f) if c["entrance"] == "block"]


def expand_tabs(text):
    lines = []
    for line in text.split("\n"):
        out = ""
        for c in line:
            out += " " * (4 - len(out) % 4) if c == "\t" else c
        lines.append(out)
    return "\n".join(lines)


def run(texts):
    results = []
    for start in range(0, len(texts), 400):
        batch = texts[start:start + 400]
        with tempfile.TemporaryDirectory(prefix="markdown-yaml-replay-") as d:
            src, out = os.path.join(d, "in.json"), os.path.join(d, "out.json")
            with open(src, "w") as f:
                json.dump([expand_tabs("---\n" + t + "---\n") for t in batch], f)
            subprocess.run([PANDOC, "lua", os.path.join(HERE, "yaml_replay.lua"), src, out],
                           check=True, timeout=600)
            with open(out) as f:
                raw = json.load(f)
        for text, r in zip(batch, raw):
            if r.startswith("err "):
                results.append({"yaml": text, "err": r[4:]})
            else:
                doc = json.loads(r[3:])
                results.append({"yaml": text, "meta": doc["meta"], "blocks": len(doc["blocks"])})
        print("block", start + len(batch), "of", len(texts), file=sys.stderr)
    return results


def main():
    texts = list(dict.fromkeys(case_texts() + yaml_texts()))
    results = run(texts)
    with open(sys.argv[1], "w") as f:
        f.write("[\n")
        f.write(",\n".join(json.dumps(r, ensure_ascii=False, separators=(",", ":")) for r in results))
        f.write("\n]\n")


if __name__ == "__main__":
    main()
