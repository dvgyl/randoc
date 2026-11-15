#!/usr/bin/env python3
"""Writes and records the oracle matrix of the script entrances: custom
readers (`pandoc -f s.lua`), custom writers (`pandoc -t s.lua`) and the
interpreter (`pandoc lua s.lua`).

Usage (from the root of the worktree, after `. env.sh`):

    python3 -I crates/randoc-lua/tools/script-matrix.py

The tool writes `crates/randoc-lua/tests/replay/scripts.txt` and runs each
case with Pandoc 3.13 (`$RANDOC_ORACLE_PANDOC`) in a temporary folder, one
process at a time with a limit of 60 s. It writes the exit code and the
output to `scripts-expected.txt`: the standard output for the exit code 0
and the standard error for each other code. An interpreter case writes its
result to the file that its first argument names, and the record holds
that file in place of the standard output.

The format of `scripts.txt`: a line `@@@ case NAME KIND ARG...` starts a
case (KIND is `reader`, `writer` or `lua`), a line `--- script` starts the
Lua file `s.lua`, a line `--- input` starts the input file `in.txt` (a
document in the native form for a writer). The commands:

    reader: pandoc -f s.lua -t json in.txt
    writer: pandoc -f native -t s.lua in.txt
    lua:    pandoc lua s.lua out.txt ARG...
"""

import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPLAY = os.path.join(HERE, "..", "tests", "replay")
HEADER = "@@@ "

SHOW_SOURCES = """function Reader(input, opts)
  local parts = {}
  for _, s in ipairs(input) do parts[#parts + 1] = s.name .. '=' .. s.text end
  return pandoc.Pandoc({pandoc.Para(tostring(input)), pandoc.Para(table.concat(parts, '|')),
    pandoc.Para(table.concat({tostring(opts.columns), tostring(FORMAT), #input, type(input),
      pandoc.utils.type(input), tostring(PANDOC_SCRIPT_FILE), pandoc.utils.type(opts)}, ' '))})
end
"""

DOC = '[Header 1 ("h",[],[]) [Str "Head"],Para [Emph [Str "a"],Space,Str "b"],BulletList [[Plain [Str "c"]]]]\n'

CASES = [
    ("reader-sources", "reader", [], SHOW_SOURCES, "one\ntwo\n"),
    ("reader-markdown", "reader", [], "function Reader(input) return pandoc.read(tostring(input), 'markdown') end\n",
     "# Title\n\n*a* and \"b\"\n"),
    ("reader-bytes", "reader", [],
     "function ByteStringReader(input, opts) return pandoc.Pandoc{pandoc.Para(type(input) .. ' ' .. #input .. ' ' .. pandoc.utils.type(opts))} end\n",
     "one\ntwo\n"),
    ("reader-both", "reader", [],
     "function Reader() return pandoc.Pandoc{pandoc.Para'text'} end\nfunction ByteStringReader() return pandoc.Pandoc{pandoc.Para'bytes'} end\n",
     "x\n"),
    ("reader-none", "reader", [], "x = 1\n", "x\n"),
    ("reader-reader-table", "reader", [], "Reader = {}\n", "x\n"),
    ("reader-error", "reader", [], "function Reader() error('boom') end\n", "x\n"),
    ("reader-number", "reader", [], "function Reader() return 5 end\n", "x\n"),
    ("reader-blocks", "reader", [], "function Reader() return pandoc.Blocks{pandoc.Para'x'} end\n", "x\n"),
    ("reader-nil", "reader", [], "function Reader() end\n", "x\n"),
    ("reader-syntax", "reader", [], "function Reader(\n", "x\n"),
    ("reader-load-error", "reader", [], "error('at load')\n", "x\n"),
    ("reader-globals", "reader", [],
     "function Reader() return pandoc.Pandoc{pandoc.Para(table.concat({type(PANDOC_READER_OPTIONS), type(PANDOC_WRITER_OPTIONS), type(PANDOC_STATE), tostring(FORMAT), type(pandoc.read)}, ' '))} end\n",
     "x\n"),
    ("writer-basic", "writer", [],
     "function Writer(doc, opts) return 'w:' .. #doc.blocks .. ' ' .. tostring(FORMAT) .. ' ' .. pandoc.utils.type(opts) .. ' ' .. pandoc.utils.stringify(doc) end\n",
     DOC),
    ("writer-newline", "writer", [], "function Writer() return 'a\\n' end\n", DOC),
    ("writer-two-newlines", "writer", [], "function Writer() return 'a\\n\\n' end\n", DOC),
    ("writer-empty", "writer", [], "function Writer() return '' end\n", DOC),
    ("writer-number", "writer", [], "function Writer() return 12 end\n", DOC),
    ("writer-bytes", "writer", [], "function ByteStringWriter(doc) return 'b' .. #doc.blocks .. '\\n' end\n", DOC),
    ("writer-both", "writer", [],
     "function Writer() return 'text' end\nfunction ByteStringWriter() return 'bytes' end\n", DOC),
    ("writer-pandoc-write", "writer", [],
     "function Writer(doc, opts) return pandoc.write(doc:walk{Str = function(s) return pandoc.Str(s.text:upper()) end}, 'html', opts) end\n",
     DOC),
    ("writer-none", "writer", [], "x = 1\n", DOC),
    ("writer-error", "writer", [], "function Writer() error('bad') end\n", DOC),
    ("writer-table", "writer", [], "function Writer() return {} end\n", DOC),
    ("writer-nil", "writer", [], "function Writer() end\n", DOC),
    ("writer-syntax", "writer", [], "function Writer(\n", DOC),
    ("writer-globals", "writer", [],
     "function Writer() return table.concat({type(PANDOC_READER_OPTIONS), type(PANDOC_WRITER_OPTIONS), tostring(PANDOC_DOCUMENT), type(PANDOC_STATE), tostring(FORMAT)}, ' ') end\n",
     DOC),
    ("writer-classic", "writer", [],
     "Para = function(text) return '[' .. text .. ']' end\nStr = function(text) return text:upper() end\n"
     "Space = function() return '_' end\nBlocksep = function() return '\\n' end\nDoc = function(body) return body end\n",
     '[Para [Str "a",Space,Str "b"]]\n'),
    ("lua-arguments", "lua", ["b", "c-d"],
     "local out = {}\nfor i = -1, #arg do out[#out + 1] = i .. '=' .. (i == 1 and 'OUT' or tostring(arg[i])) end\n"
     "local rest = table.pack(...)\nout[#out + 1] = rest.n .. ' ' .. tostring(rest[2]) .. ' ' .. tostring(rest[3])\n"
     "local f = io.open(arg[1], 'w') f:write(table.concat(out, '\\n'), '\\n') f:close()\n",
     ""),
    ("lua-globals", "lua", [],
     "local f = io.open(arg[1], 'w')\n"
     "f:write(table.concat({tostring(PANDOC_SCRIPT_FILE), tostring(FORMAT), type(PANDOC_STATE), tostring(PANDOC_READER_OPTIONS), tostring(PANDOC_WRITER_OPTIONS), type(pandoc.read), type(pandoc.write), type(lpeg), pandoc.write(pandoc.read('*a*'), 'html')}, ' '), '\\n') f:close()\n",
     ""),
    ("lua-error", "lua", [], "error('x')\n", ""),
    ("lua-error-in-function", "lua", [], "local function f() error('deep') end\nf()\n", ""),
    ("lua-syntax", "lua", [], "x = = 1\n", ""),
    ("lua-return", "lua", [], "local f = io.open(arg[1], 'w') f:write('ran\\n') f:close()\nreturn 1, 2\n", ""),
]


def command(pandoc, kind, args):
    if kind == "reader":
        return [pandoc, "-f", "s.lua", "-t", "json", "in.txt"]
    if kind == "writer":
        return [pandoc, "-f", "native", "-t", "s.lua", "in.txt"]
    return [pandoc, "lua", "s.lua", "out.txt"] + args


def record(pandoc, case):
    name, kind, args, script, text = case
    with tempfile.TemporaryDirectory(prefix="lua-scripts-") as folder:
        with open(os.path.join(folder, "s.lua"), "w", encoding="utf-8") as handle:
            handle.write(script)
        with open(os.path.join(folder, "in.txt"), "w", encoding="utf-8") as handle:
            handle.write(text)
        try:
            done = subprocess.run(command(pandoc, kind, args), cwd=folder, capture_output=True, timeout=60)
        except subprocess.TimeoutExpired:
            sys.exit(f"the oracle did not end in 60 s: {name}")
        if done.returncode == 251 and b"Heap exhausted" in done.stderr:
            sys.exit(f"the oracle reached its heap limit: {name}")
        output = done.stdout if done.returncode == 0 else done.stderr
        if kind == "lua" and done.returncode == 0:
            with open(os.path.join(folder, "out.txt"), "rb") as handle:
                output = handle.read()
    text = output.decode("utf-8")
    if not text.endswith("\n"):
        sys.exit(f"output with no final newline: {name}")
    if any(line.startswith(HEADER) for line in text.split("\n")):
        sys.exit(f"output with a header line: {name}")
    return f"{HEADER}{name} {done.returncode}\n{text}"


def main():
    pandoc = os.environ.get("RANDOC_ORACLE_PANDOC")
    if not pandoc or "GHCRTS" not in os.environ:
        sys.exit("source env.sh first: RANDOC_ORACLE_PANDOC and GHCRTS must be set")
    parts = [
        "# The oracle matrix of the script entrances.\n",
        "# `tools/script-matrix.py` writes this file. Do not edit it.\n",
    ]
    for name, kind, args, script, text in CASES:
        if "\n--- " in "\n" + script or "\n--- " in "\n" + text:
            sys.exit(f"a part with a marker line: {name}")
        parts.append(f"{HEADER}case {name} {kind} {' '.join(args)}".rstrip() + "\n")
        parts.append(f"--- script\n{script}--- input\n{text}")
    with open(os.path.join(REPLAY, "scripts.txt"), "w", encoding="utf-8") as handle:
        handle.write("".join(parts))
    results = [record(pandoc, case) for case in CASES]
    with open(os.path.join(REPLAY, "scripts-expected.txt"), "w", encoding="utf-8") as handle:
        handle.write("".join(results))
    failures = sum(1 for result in results if not result.split("\n", 1)[0].endswith(" 0"))
    print(f"{len(CASES)} cases, {failures} with an error exit")


if __name__ == "__main__":
    main()
