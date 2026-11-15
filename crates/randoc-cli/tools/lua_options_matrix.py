#!/usr/bin/env python3
"""Records the options that a Lua filter sees through the program.

Usage: lua_options_matrix.py PANDOC OUT.json

For each command line of `CASES` the script runs
`PANDOC -f json CASE --lua-filter dump.lua -o out` on an empty document in a
new folder, with the filter `tests/data/lua-options-dump.lua`. The filter
writes one line for each field of `PANDOC_READER_OPTIONS` and
`PANDOC_WRITER_OPTIONS` to the file `$OPTIONS_DUMP`. The data file keeps the
arguments, the exit code and the sorted lines. `tests/lua_options.rs`
replays each case against Randoc.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
FILTER = os.path.join(HERE, "..", "tests", "data", "lua-options-dump.lua")
DOC = '{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}'

CASES = [
    "-t json",
    "-t html --columns 50 --tab-stop 2",
    "-t markdown --toc --toc-depth 2 -N",
    "-t html --number-offset 2,3 --section-divs --id-prefix x- --email-obfuscation javascript",
    "-t html --mathjax",
    "-t html --katex",
    "-t html --mathml",
    "-t html --webtex",
    "-t html --gladtex",
    "-t latex --natbib --top-level-division chapter",
    "-t html --wrap none --dpi 120 --html-q-tags --ascii",
    "-t markdown --reference-links --reference-location block --markdown-headings setext --list-tables",
    "-t markdown --markdown-headings atx --wrap auto --columns 60",
    "-t html --slide-level 2 -i",
    "-t html -V foo=bar -V foo=baz -V k --variable n=1",
    "-t html --no-highlight",
    "-t html --syntax-highlighting kate",
    "-t html --lof --lot --link-images",
    "-t html --figure-caption-position above --table-caption-position below",
    "-t json --track-changes all --strip-comments --indented-code-classes a,b --default-image-extension png",
    "-t html --chunk-template x.html --reference-doc foo.docx",
    "-t html --biblatex",
    "-t html --wrap preserve --columns 100 --split-level 3",
    "-t latex --top-level-division part",
    "-t latex --reference-location section",
    "-t gfm",
    "-t commonmark_x",
    "-t html --email-obfuscation references",
    "-t markdown --track-changes reject",
    "-t native",
    "-t plain --columns 30",
    "-t html -s",
    "-t html --template t.html",
    "-t html --number-sections",
    "-t html --toc --toc-depth 4",
    "-t html --shift-heading-level-by 1",
    "-t html --incremental",
    "-t html --resource-path a:b",
    "-t markdown --abbreviations ab",
    "-t html --indented-code-classes x",
    "-t plain",
    "-t html --metadata title=x",
    "-t html --tab-stop 8 --preserve-tabs",
    "-t markdown-smart",
]


def run(pandoc, case):
    folder = tempfile.mkdtemp(prefix="cli-lua-options-")
    try:
        shutil.copyfile(FILTER, os.path.join(folder, "dump.lua"))
        with open(os.path.join(folder, "t.html"), "w", encoding="utf-8") as handle:
            handle.write("$body$\n")
        with open(os.path.join(folder, "ab"), "w", encoding="utf-8") as handle:
            handle.write("Zz.\n")
        dump = os.path.join(folder, "dump.txt")
        args = ["-f", "json"] + case.split() + ["--lua-filter", "dump.lua", "-o", "out"]
        env = {"PATH": os.environ.get("PATH", ""), "HOME": folder, "GHCRTS": "-M512m",
               "OPTIONS_DUMP": dump, "LANG": "C.UTF-8"}
        done = subprocess.run([pandoc] + args, cwd=folder, input=DOC.encode(), env=env,
                              capture_output=True, timeout=60)
        lines = []
        if os.path.isfile(dump):
            with open(dump, encoding="utf-8") as handle:
                lines = sorted(handle.read().splitlines())
        return {"args": args, "exit": done.returncode, "lines": lines}
    finally:
        shutil.rmtree(folder)


def main():
    pandoc, out = sys.argv[1], sys.argv[2]
    records = [run(pandoc, case) for case in CASES]
    with open(out, "w", encoding="utf-8") as handle:
        handle.write("[\n")
        handle.write(",\n".join(json.dumps(r, ensure_ascii=True) for r in records))
        handle.write("\n]\n")
    print(f"{len(records)} cases")


if __name__ == "__main__":
    main()
