#!/usr/bin/env python3
"""Writes the oracle matrix of the tree walk of randoc-lua.

Usage (from the root of the worktree, after `. env.sh`):

    python3 -I crates/randoc-lua/tools/walk-matrix.py
    python3 -I crates/randoc-lua/tools/record-replay.py walk

The script writes `crates/randoc-lua/tests/replay/walk.txt` in the format of
`cases.txt`. The oracle (`pandoc lua`) makes the two documents from the Lua
programs below. Each filter of the matrix writes each visit to a log, and a
second filter puts the log into the metadata field `log`. Thus the recorded
output shows the result of the walk and the order of the calls.

The axes of the matrix: the traversal (`typewise`, `topdown`); the return
value of each filter function (nil, the same element, a changed element, a
list, an empty list, `false` as the second value); the set of filter
functions (each constructor name, the base names `Inline` and `Block`, a mix
of the two on one element); the entry point (a filter of the document, and
the methods `walk` of `Pandoc`, `Blocks`, `Block`, `Inlines` and `Inline`).
Some filters give errors: a return value of a wrong type.
"""

import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
OUTPUT = os.path.join(HERE, "..", "tests", "replay", "walk.txt")

ALLKINDS = r"""
local P = pandoc
local function t(s) return P.Str(s) end
local cell = function(s) return P.Cell({P.Plain{t(s)}}) end
local row = function(a, b) return P.Row{cell(a), cell(b)} end
local tbl = P.Table(
  P.Caption({P.Plain{t'cap'}}, {t'short'}),
  {{P.AlignLeft, 0.5}, {P.AlignDefault, P.ColWidthDefault}},
  P.TableHead{row('h1', 'h2')},
  {{attr = P.Attr(), body = {row('b1', 'b2')}, head = {row('i1', 'i2')}, row_head_columns = 1}},
  P.TableFoot{row('f1', 'f2')})
local doc = P.Pandoc({
  P.Header(1, {t'Head', P.Space(), P.Emph{t'em'}}, P.Attr('h')),
  P.Para{t'a', P.Space(), P.Strong{t'b'}, P.SoftBreak(), P.Strikeout{t'c'}, P.LineBreak(),
    P.Superscript{t'd'}, P.Subscript{t'e'}, P.SmallCaps{t'f'}, P.Underline{t'g'},
    P.Quoted('DoubleQuote', {t'h'}), P.Code('i'), P.Math('InlineMath', 'j'),
    P.RawInline('html', '<k>'), P.Link({t'l'}, 'u'), P.Image({t'm'}, 'v'),
    P.Note{P.Para{t'n'}}, P.Span({t'o'}),
    P.Cite({t'p'}, {P.Citation('key', 'NormalCitation', {t'pre'}, {t'suf'})})},
  P.Plain{t'q'},
  P.LineBlock{{t'r1'}, {t'r2'}},
  P.CodeBlock('code'),
  P.RawBlock('tex', '\\x'),
  P.BlockQuote{P.Para{t's'}},
  P.OrderedList{{P.Plain{t'o1'}}, {P.Plain{t'o2'}}},
  P.BulletList{{P.Plain{t'u1'}, P.BulletList{{P.Plain{t'u2'}}}}},
  P.DefinitionList{{{t'term'}, {{P.Plain{t'def'}}}}},
  P.HorizontalRule(),
  tbl,
  P.Figure({P.Plain{P.Image({t'w'}, 'x')}}, P.Caption({P.Plain{t'fc'}}, {t'fs'})),
  P.Div{P.Para{t'y'}},
}, {
  mb = true,
  ms = P.MetaString'z',
  mi = P.MetaInlines{t'mi'},
  mbl = P.MetaBlocks{P.Para{t'mb'}},
  ml = P.MetaList{P.MetaInlines{t'ml'}, P.MetaBlocks{P.Plain{t'ml2'}}},
  mm = P.MetaMap{k = P.MetaInlines{t'mm'}, j = P.MetaList{P.MetaInlines{t'mj'}}},
})
io.write(pandoc.write(doc, 'json'), '\n')
"""

DEEP = r"""
local P = pandoc
local inner = P.Emph{P.Str'x', P.Strong{P.Str'y', P.Span{P.Str'z'}}, P.Str'w'}
local doc = P.Pandoc({
  P.Div{P.BlockQuote{P.Para{P.Str'one', inner}, P.Div{P.Plain{P.Str'two'}}}, P.Para{P.Str'three'}},
  P.Para{P.Note{P.Para{P.Emph{P.Str'four'}}, P.BlockQuote{P.Plain{P.Str'five'}}}, P.Str'six'},
}, {title = P.MetaInlines{P.Emph{P.Str'seven'}}})
io.write(pandoc.write(doc, 'json'), '\n')
"""

INLINES = [
    "Cite", "Code", "Emph", "Image", "LineBreak", "Link", "Math", "Note", "Quoted",
    "RawInline", "SmallCaps", "SoftBreak", "Space", "Span", "Str", "Strikeout", "Strong",
    "Subscript", "Superscript", "Underline",
]
BLOCKS = [
    "BlockQuote", "BulletList", "CodeBlock", "DefinitionList", "Div", "Figure", "Header",
    "HorizontalRule", "LineBlock", "OrderedList", "Para", "Plain", "RawBlock", "Table",
]

# The logger: `element` makes the function for one element name, `list`
# the function for `Inlines` or `Blocks`.
LOGGER = r"""
local LOG = {}
local MODE = '%(mode)s'
local function note(s) LOG[#LOG + 1] = s end
local function id(el)
  if el.t == 'Str' then return 'Str:' .. el.text end
  return el.t
end
local function element(name, kind)
  return function(el)
    note(name .. '>' .. id(el))
    if MODE == 'same' then return el end
    if MODE == 'edit' then
      if el.t == 'Str' then return pandoc.Str(el.text .. '!') end
      if el.t == 'Header' then el.level = el.level + 1 end
      return el
    end
    if MODE == 'list' then
      if kind == 'inline' then return {el, pandoc.Str('+')} end
      return {el, pandoc.HorizontalRule()}
    end
    if MODE == 'empty' then return {} end
    if MODE == 'stop' then return el, false end
    if MODE == 'nilstop' then return nil, false end
  end
end
local function list(name)
  return function(xs)
    note(name .. '#' .. #xs)
    if MODE == 'same' or MODE == 'edit' or MODE == 'list' then return xs end
    if MODE == 'empty' then return {} end
    if MODE == 'stop' then return xs, false end
    if MODE == 'nilstop' then return nil, false end
  end
end
local L = {traverse = '%(traverse)s'}
"""

FINISH = r"""
local function save(doc)
  local items = {}
  for i, s in ipairs(LOG) do items[i] = pandoc.MetaString(s) end
  doc.meta.log = pandoc.MetaList(items)
  return doc
end
"""

SETS = {
    "all": [(n, "inline") for n in INLINES] + [(n, "block") for n in BLOCKS]
    + [("Inline", "inline"), ("Block", "block")],
    "base": [("Inline", "inline"), ("Block", "block")],
    "mix": [("Str", "inline"), ("Inline", "inline"), ("Para", "block"), ("Block", "block")],
}

MODES = {
    "typewise": ["nil", "same", "edit", "list", "empty"],
    "topdown": ["nil", "same", "edit", "list", "empty", "stop", "nilstop"],
}


def logger(mode, traverse, functions, lists=True, document=True):
    lines = [LOGGER % {"mode": mode, "traverse": traverse}]
    for name, kind in functions:
        lines.append(f"L.{name} = element('{name}', '{kind}')\n")
    if lists:
        lines.append("L.Inlines = list('Inlines')\nL.Blocks = list('Blocks')\n")
    if document:
        lines.append(
            "L.Meta = function(m) note('Meta')\n"
            "  if MODE == 'edit' then m.edited = true end\n"
            "  if MODE == 'nil' or MODE == 'empty' then return nil end\n"
            "  if MODE == 'stop' then return m, false end\n"
            "  return m end\n"
            "L.Pandoc = function(d) note('Pandoc#' .. #d.blocks)\n"
            "  if MODE == 'nil' or MODE == 'empty' then return nil end\n"
            "  if MODE == 'stop' then return d, false end\n"
            "  return d end\n"
        )
    lines.append(FINISH)
    return "".join(lines)


METHODS = {
    "doc": "doc = doc:walk(L)",
    "blocks": "doc.blocks = doc.blocks:walk(L)",
    "block": "for i, b in ipairs(doc.blocks) do doc.blocks[i] = b:walk(L) end",
    "inlines": (
        "for _, b in ipairs(doc.blocks) do\n"
        "  if b.t == 'Para' or b.t == 'Plain' or b.t == 'Header' then b.content = b.content:walk(L) end\n"
        "end"
    ),
    "inline": (
        "for _, b in ipairs(doc.blocks) do\n"
        "  if b.t == 'Para' or b.t == 'Plain' or b.t == 'Header' then\n"
        "    for i, x in ipairs(b.content) do b.content[i] = x:walk(L) end\n"
        "  end\n"
        "end"
    ),
}

ERRORS = {
    "inline-gives-block": "function Str(s) return pandoc.Para{s} end",
    "block-gives-inline": "function Para(p) return pandoc.Str'x' end",
    "inline-list-with-block": "function Str(s) return {s, pandoc.Para{}} end",
    "inlines-gives-element": "function Inlines(xs) return pandoc.Str'x' end",
    "blocks-gives-element": "function Blocks(xs) return pandoc.Para{} end",
    "inlines-gives-strings": "function Inlines(xs) return {'a', 'b'} end",
    "blocks-gives-string": "function Blocks(xs) return 'text' end",
    "inline-gives-string": "function Emph(e) return 'two words' end",
    "meta-gives-string": "function Meta(m) return 'x' end",
    "pandoc-gives-number": "function Pandoc(d) return 5 end",
    "pandoc-gives-meta": "function Pandoc(d) return d.meta end",
    "not-functions": (
        "return {{Str = 5, Para = 'x', Inline = {}, traverse = 'topdown',"
        " Space = function() return pandoc.Str'_' end}}"
    ),
    "traverse-unknown": (
        "return {{traverse = 'bottomup', Str = function(s) return pandoc.Str(s.text .. '.') end}}"
    ),
    "traverse-number": (
        "return {{traverse = 7, Emph = function(e) return e.content end}}"
    ),
    "typewise-false": "function Str(s) return pandoc.Str'k', false end",
    "topdown-true": (
        "return {{traverse = 'topdown', Emph = function(e) return pandoc.Strong(e.content), true end,"
        " Str = function(s) return pandoc.Str(s.text .. '?') end}}"
    ),
    "topdown-meta-stop": (
        "return {{traverse = 'topdown', Meta = function(m) m.x = true; return m, false end,"
        " Str = function(s) return pandoc.Str(s.text .. '?') end}}"
    ),
    "topdown-pandoc-stop": (
        "return {{traverse = 'topdown', Pandoc = function(d) return d, false end,"
        " Str = function(s) return pandoc.Str(s.text .. '?') end}}"
    ),
    "error-in-walk": (
        "function Pandoc(d)\n  return d:walk{Str = function(s) error('stop here') end}\nend"
    ),
    "inline-walk-self": (
        "function Pandoc(d)\n"
        "  local e = pandoc.Emph{pandoc.Str'a'}\n"
        "  d.blocks:insert(pandoc.Para{e:walk{Emph = function() return pandoc.Str'gone' end,"
        " Str = function() return pandoc.Str'b' end}})\n"
        "  return d\nend"
    ),
    "block-walk-self": (
        "function Pandoc(d)\n"
        "  local q = pandoc.BlockQuote{pandoc.Para{'a'}}\n"
        "  d.blocks:insert(q:walk{BlockQuote = function() return {} end,"
        " Para = function() return pandoc.Plain'b' end})\n"
        "  return d\nend"
    ),
}


def oracle_json(program):
    pandoc = os.environ.get("RANDOC_ORACLE_PANDOC")
    if not pandoc or "GHCRTS" not in os.environ:
        sys.exit("source env.sh first: RANDOC_ORACLE_PANDOC and GHCRTS must be set")
    done = subprocess.run(
        [pandoc, "lua", "-"], input=program.encode(), capture_output=True, timeout=60, check=True
    )
    return done.stdout.decode("utf-8")


def main():
    docs = {"walk-allkinds": oracle_json(ALLKINDS), "walk-deep": oracle_json(DEEP)}
    doc_names = " ".join(docs)
    out = [
        "# The oracle matrix of the tree walk. `tools/walk-matrix.py` writes this file;\n"
        "# do not edit it by hand.\n"
    ]
    for name, text in docs.items():
        out.append(f"@@@ doc {name}\n{text}")

    def add(name, source):
        out.append(f"@@@ filter {name} {doc_names}\n{source.rstrip()}\n")

    for traverse, modes in MODES.items():
        for mode in modes:
            for set_name, functions in SETS.items():
                source = logger(mode, traverse, functions)
                source += "return {L, {Pandoc = save}}\n"
                add(f"walk-{traverse}-{mode}-{set_name}", source)
    for traverse in MODES:
        for mode in ["same", "list", "stop"]:
            if traverse == "typewise" and mode == "stop":
                continue
            for method, code in METHODS.items():
                source = logger(mode, traverse, SETS["all"], document=(method == "doc"))
                source += f"function Pandoc(doc)\n{code}\nreturn save(doc)\nend\n"
                add(f"walk-method-{method}-{traverse}-{mode}", source)
    for name, source in ERRORS.items():
        add(f"walk-error-{name}", source)
    with open(OUTPUT, "w", encoding="utf-8") as handle:
        handle.write("".join(out))
    print(f"{sum(1 for part in out if part.startswith('@@@ filter'))} filters")


if __name__ == "__main__":
    main()
