#!/usr/bin/env python3
"""Writes the oracle matrix of the functions of the `pandoc` Lua module.

Usage (from the root of the worktree, after `. env.sh`):

    python3 -I crates/randoc-lua/tools/module-matrix.py
    python3 -I crates/randoc-lua/tools/record-replay.py modules

The script writes `crates/randoc-lua/tests/replay/modules.txt` in the format
of `cases.txt`. Each filter is one group of probes. A probe calls one
function with `pcall` and writes the results, or the error text, to a log.
The function `Pandoc` of the filter returns a document with the log in one
code block. Thus the recorded output shows each result and each error text.
"""

import os
import random

HERE = os.path.dirname(os.path.abspath(__file__))
OUTPUT = os.path.join(HERE, "..", "tests", "replay", "modules.txt")

DOCS = {
    "empty": '{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}',
    "small": '{"pandoc-api-version":[1,23,1,2],"meta":{"title":{"t":"MetaInlines",'
    '"c":[{"t":"Str","c":"Probe"}]}},"blocks":[{"t":"Para","c":[{"t":"Str","c":"one"}]}]}',
}

HELPER = r"""
local P = pandoc
local log = {}
local function t(s) return P.Str(s) end
local function H(level, text, attr) return P.Header(level, {t(text)}, attr or P.Attr()) end
local function cell(text) return P.Cell{P.Plain{t(text)}} end
local function show(value, depth)
  depth = depth or 0
  local kind = type(value)
  -- Not `%q`: its escapes depend on the C locale (row I-LUA-13).
  if kind == 'string' then
    return '"' .. value:gsub('[\0-\31\127"\\]', function(c) return '\\' .. c:byte() end) .. '"'
  end
  if kind == 'number' then return math.type(value) .. ':' .. tostring(value) end
  if kind == 'table' and depth < 6 then
    local mt = getmetatable(value)
    local name = mt and rawget(mt, '__name')
    if name == 'Inlines' or name == 'Blocks' then return name .. ' ' .. tostring(value) end
    local keys = {}
    for key in pairs(value) do keys[#keys + 1] = key end
    table.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
    local parts = {}
    for _, key in ipairs(keys) do
      parts[#parts + 1] = show(key, depth + 1) .. '=' .. show(value[key], depth + 1)
    end
    return (name and (tostring(name) .. ' ') or '') .. '{' .. table.concat(parts, ', ') .. '}'
  end
  if kind == 'userdata' then return P.utils.type(value) .. ' ' .. tostring(value) end
  if kind == 'function' then return 'function' end
  return tostring(value)
end
local function probe(label, fn)
  local results = table.pack(pcall(fn))
  local parts = {}
  for i = 2, results.n do parts[#parts + 1] = show(results[i]) end
  -- Pandoc raises some errors as `PandocError` objects (row I-LUA-10):
  -- the text of an error is compared, not its type.
  if not results[1] then parts = {show(tostring(results[2]))} end
  local line = label .. (results[1] and ' => ' or ' !! ') .. table.concat(parts, ' | ')
  -- Addresses differ from run to run.
  log[#log + 1] = (line:gsub('0x%x%x%x%x+', 'ADDR'))
end
"""

FOOTER = r"""
function Pandoc(document)
  run(document)
  return P.Pandoc({P.CodeBlock(table.concat(log, '\n'))})
end
"""


def lua_string(text):
    out = []
    for byte in text.encode("utf-8"):
        if 32 <= byte < 127 and chr(byte) not in "\\'":
            out.append(chr(byte))
        else:
            out.append(f"\\{byte:03d}")
    return "'" + "".join(out) + "'"


def calls(function, arguments):
    """One probe for each argument list (Lua source text)."""
    return [(f"{function}({args})", f"return {function}({args})") for args in arguments]


GROUPS = {}

GROUPS["utils-roman"] = calls("P.utils.to_roman_numeral", [
    "0", "1", "3", "4", "9", "14", "40", "49", "90", "400", "944", "1994", "2024",
    "3999", "4000", "5000", "-1", "-4000", "2.0", "1.5", "'12'", "' 7 '", "'x'",
    "'0x10'", "nil", "", "{}", "true", "math.maxinteger", "math.mininteger", "2^53",
    "1e300", "0/0",
])

GROUPS["utils-sha1"] = calls("P.utils.sha1", [
    "''", "'abc'", lua_string("é"), "12", "1.5", "string.rep('a', 55)",
    "string.rep('a', 56)", "string.rep('a', 63)", "string.rep('a', 64)",
    "string.rep('ab', 500)", "'\\0'", "", "nil", "{}", "true", "P.Str'x'",
]) + calls("P.sha1", ["'abc'", ""])

DATES = [
    "2023-01-05", "2023/01/05", "01/05/2023", "Jan 5, 2023", "5 January 2023",
    "2023", "2023-01", "1500-01-01", "1601-01-01", "9999-12-31", "10000-01-01",
    "", "x", "2023-13-01", "2023-02-30", "20230105", "January 2023",
    "Thu, 05 Jan 2023", "05.01.2023", "2023-1-5", " 2023-01-05", "2023-01-05 ",
    "1/5/23", "Jan 2023", "5 Jan 2023", "01/05/23", "1/5/2023", "01/5/2023",
    "12/31/99", "12/31/68", "12/31/69", "01/05/1600", "02023-01-05", "05 Jan 2023",
    "05 January 2023", "05 jan 2023", "05 JAN 2023", "5 january 2023",
    "05 Sept 2023", "05 Sep 2023", "Jan. 05, 2023", "Jan. 5, 2023", "Jan 05, 2023",
    "January 05, 2023", "January 5, 2023", "January  5, 2023", "Jan.  5, 2023",
    "202301", "999", "0999", "1600", "9999", "12345678", "20231301", "2023-02-29",
    "2024-02-29", "1900-02-29", "2000-02-29", "23", "230105", "2023-01-05T10:00",
    "05/01/2023", "13/01/2023", "2023.01.05", "Jan 5 2023", "2023-W01",
    "\t2023\n", "05  Jan 2023", "05Jan2023", "05\tJan 2023", "01/05/ 23",
    "January. 5, 2023", "Jan. 5,2023", "Jan.5, 2023", "January5, 2023",
    "16011201", "160101", "00002023", "2023.0", "+2023", "-2023", "5 Janu 2023",
    "005 Jan 2023", "05 Jan 02023", "05 Jan 23", "Feb. 29, 2023", "May. 5, 2023",
    "June 5, 2023", "00/10/2023", "01/01/00", "٢٠٢٣",
    "２０２３", "31 February 2023", "5 January 2023",
    "5 January 2023", "5\u0085January 2023", "5 January 2023",
    "01/05/23x", "January 005, 2023", "1 Jan. 2023", "99999999999999999999999",
    "Dec. 31, 9999", "Mar 1, 2023", "Oct 10 2023", "  \n ",
    "January 0005, 2023", "0005 January 2023", "January 105, 2023",
    "Jan. 0031, 2023", "01/005/2023", "2023-001-05",
]
GROUPS["utils-date"] = calls(
    "P.utils.normalize_date", [lua_string(d) for d in DATES]
    + ["2023", "2023.0", "20230105", "", "nil", "{}", "true"]
)

BLOCKS = r"""
local function blocks()
  return {
    P.Plain{t'plain'}, P.Para{t'para'}, P.LineBlock{{t'l1'}, {t'l2'}},
    P.CodeBlock('code', P.Attr('i', {'c'})), P.RawBlock('html', '<b>'),
    P.BlockQuote{P.Para{t'q1'}, P.Para{t'q2'}},
    P.OrderedList{{P.Plain{t'o1'}}, {P.Para{t'o2'}, P.Para{t'o3'}}},
    P.BulletList{{P.Plain{t'b1'}}, {P.Plain{t'b2'}}},
    P.DefinitionList{{{t'term'}, {{P.Plain{t'd1'}}, {P.Para{t'd2'}}}}},
    P.Header(2, {t'head'}, P.Attr('hid')), P.HorizontalRule(),
    P.Table(P.Caption({P.Plain{t'cap'}}), {{P.AlignLeft, 0.5}},
      P.TableHead{P.Row{cell'h'}},
      {{attr = P.Attr(), body = {P.Row{cell'c'}}, head = {}, row_head_columns = 0}},
      P.TableFoot()),
    P.Figure({P.Plain{t'fig'}}, P.Caption({P.Plain{t'fc'}})),
    P.Div({P.Para{t'div1'}, P.Para{t'div2'}}, P.Attr('d')),
  }
end
"""

B2I = "P.utils.blocks_to_inlines"
GROUPS["utils-blocks-to-inlines"] = [
    (f"kind {i}", f"return {B2I}{{blocks()[{i}]}}") for i in range(1, 15)
] + [
    (f"kind {i} then para", f"return {B2I}({{blocks()[{i}], P.Para{{t'after'}}}})")
    for i in range(1, 15)
] + [
    ("all with sep", f"return {B2I}(blocks(), {{t'|', P.Space()}})"),
    ("empty", f"return {B2I}{{}}"),
    ("sep", f"return {B2I}({{P.Para{{t'a'}}, P.Para{{t'b'}}, P.Para{{t'c'}}}}, {{t'|', P.Space()}})"),
    ("sep string", f"return {B2I}({{P.Para{{t'a'}}, P.Para{{t'b'}}}}, 'xx')"),
    ("sep empty", f"return {B2I}({{P.Para{{t'a'}}, P.Para{{t'b'}}}}, {{}})"),
    ("sep nil", f"return {B2I}({{P.Para{{t'a'}}, P.Para{{t'b'}}}}, nil)"),
    ("sep inline", f"return {B2I}({{P.Para{{t'a'}}, P.Para{{t'b'}}}}, P.Space())"),
    ("sep Inlines", f"return {B2I}({{P.Para{{t'a'}}, P.Para{{t'b'}}}}, P.Inlines{{t'-'}})"),
    ("sep number", f"return {B2I}({{P.Para{{t'a'}}}}, 5)"),
    ("sep bad item", f"return {B2I}({{P.Para{{t'a'}}}}, {{'x'}})"),
    ("block argument", f"return {B2I}(P.Para{{t'a'}})"),
    ("Blocks argument", f"return {B2I}(P.Blocks{{P.Para{{t'a'}}, P.Plain{{t'b'}}}})"),
    ("string argument", f"return {B2I}('text')"),
    ("no argument", f"return {B2I}()"),
    ("inline item", f"return {B2I}{{t'x'}}"),
    ("string item", f"return {B2I}{{'x'}}"),
    ("type", f"return P.utils.type({B2I}{{P.Para{{t'a'}}}})"),
    ("nested", f"return {B2I}{{P.Div{{P.Div{{P.Para{{t'x'}}}}, P.BulletList{{{{P.Div{{P.Plain{{t'y'}}, P.Plain{{t'z'}}}}}}}}}}}}"),
    ("empty blocks", f"return {B2I}{{P.Plain{{}}, P.Para{{}}, P.Div{{}}}}"),
    ("no merge inside", f"return {B2I}{{P.Para{{t'a', t'b'}}}}"),
    ("space line break", f"return {B2I}{{P.Para{{t'a', P.Space()}}, P.Para{{P.Space(), t'b'}}}}"),
    ("emph merge", f"return {B2I}({{P.Para{{P.Emph{{t'a'}}}}, P.Para{{P.Emph{{t'b'}}}}}}, {{}})"),
    ("smallcaps", f"return {B2I}({{P.Para{{P.SmallCaps{{t'a'}}}}, P.Para{{P.SmallCaps{{t'b'}}}}}}, {{}})"),
    ("each merge", f"return {B2I}({{P.Plain{{P.Underline{{t'a'}}, P.Subscript{{t'b'}}, P.Superscript{{t'c'}}, P.Strikeout{{t'd'}}, P.Strong{{t'e'}}}}, P.Plain{{P.Underline{{t'a'}}}}, P.Plain{{P.Subscript{{t'b'}}}}, P.Plain{{P.Superscript{{t'c'}}}}, P.Plain{{P.Strikeout{{t'd'}}}}}}, {{}})"),
    ("strong merge", f"return {B2I}({{P.Plain{{P.Strong{{t'e'}}}}, P.Plain{{P.Strong{{t'f'}}}}}}, {{}})"),
    ("space space", f"return {B2I}({{P.Para{{t'a', P.Space()}}, P.Para{{P.Space(), t'b'}}}}, {{P.Space()}})"),
    ("soft breaks", f"return {B2I}({{P.Para{{t'a', P.SoftBreak()}}, P.Para{{P.Space(), t'b'}}, P.Para{{P.SoftBreak()}}, P.Para{{P.SoftBreak()}}}}, {{}})"),
    ("space soft", f"return {B2I}({{P.Para{{t'a', P.Space()}}, P.Para{{P.SoftBreak(), t'b'}}}}, {{}})"),
    ("break pairs", f"return {B2I}({{P.Para{{t'a', P.LineBreak()}}, P.Para{{P.SoftBreak(), t'b', P.SoftBreak()}}, P.Para{{P.LineBreak()}}, P.Para{{P.Space()}}}}, {{}})"),
    ("break break", f"return {B2I}({{P.Para{{t'a', P.LineBreak()}}, P.Para{{P.SoftBreak(), t'b'}}}})"),
    ("nested sep", f"return {B2I}({{P.BlockQuote{{P.Para{{t'q1'}}, P.Para{{t'q2'}}}}, P.Para{{t'x'}}}}, {{t'|'}})"),
    ("div sep", f"return {B2I}({{P.Div{{P.Para{{t'q1'}}, P.Para{{t'q2'}}}}}}, {{t'|'}})"),
    ("list sep", f"return {B2I}({{P.BulletList{{{{P.Para{{t'a1'}}, P.Para{{t'a2'}}}}, {{P.Para{{t'b'}}}}}}}}, {{t'|'}})"),
    ("line block sep", f"return {B2I}({{P.LineBlock{{{{t'l1'}}, {{t'l2'}}}}}}, {{t'|'}})"),
    ("line block spaces", f"return {B2I}{{P.LineBlock{{{{t'l1', P.Space()}}, {{P.Space(), t'l2'}}, {{}}, {{t'l3'}}}}}}"),
    ("line block strs", f"return {B2I}{{P.LineBlock{{{{t'a'}}, {{}}}}, P.LineBlock{{{{}}, {{t'b'}}}}}}"),
    ("definitions", f"return {B2I}({{P.DefinitionList{{{{{{t'term'}}, {{{{P.Para{{t'd1'}}, P.Para{{t'd1b'}}}}, {{P.Para{{t'd2'}}}}}}}}, {{{{t'two', P.Space()}}, {{}}}}}}}}, {{t'|'}})"),
    ("table full", f"""return {B2I}({{P.Table(P.Caption({{P.Plain{{t'cap'}}}}, {{t'sh'}}), {{{{P.AlignLeft, 0.5}}, {{P.AlignLeft, 0.5}}}},
      P.TableHead{{P.Row{{cell'h1', cell'h2'}}, P.Row{{cell'h3', cell'h4'}}}},
      {{{{attr = P.Attr(), body = {{P.Row{{cell'c1', cell'c2'}}, P.Row{{cell'c3', cell'c4'}}}}, head = {{P.Row{{cell'i1', cell'i2'}}}}, row_head_columns = 0}},
       {{attr = P.Attr(), body = {{P.Row{{cell'd1', cell'd2'}}}}, head = {{}}, row_head_columns = 0}}}},
      P.TableFoot{{P.Row{{cell'f1', cell'f2'}}}})}}, {{t'|'}})"""),
    ("table empty", f"return {B2I}({{P.Table(P.Caption(), {{{{P.AlignLeft, 0.5}}}}, P.TableHead(), {{}}, P.TableFoot())}}, {{t'|'}})"),
    ("table empty rows", f"return {B2I}{{P.Table(P.Caption(), {{{{P.AlignLeft, 0.5}}}}, P.TableHead{{P.Row{{}}, P.Row{{}}}}, {{}}, P.TableFoot{{P.Row{{cell'f'}}}})}}"),
    ("table cell paras", f"return {B2I}({{P.Table(P.Caption(), {{{{P.AlignLeft, 0.5}}}}, P.TableHead(), {{{{attr = P.Attr(), body = {{P.Row{{P.Cell{{P.Para{{t'p1'}}, P.Para{{t'p2'}}}}}}}}, head = {{}}, row_head_columns = 0}}}}, P.TableFoot())}}, {{t'|'}})"),
    ("table spaces", f"return {B2I}{{P.Table(P.Caption(), {{{{P.AlignLeft, 0.5}}}}, P.TableHead{{P.Row{{P.Cell{{P.Plain{{t'a', P.Space()}}}}}}, P.Row{{P.Cell{{P.Plain{{P.Space(), t'b'}}}}}}}}, {{}}, P.TableFoot())}}"),
    ("figure", f"return {B2I}({{P.Figure({{P.Para{{t'f1'}}, P.Para{{t'f2'}}}}, P.Caption({{P.Plain{{t'fc'}}}}))}}, {{t'|'}})"),
    ("empty item", f"return {B2I}({{P.OrderedList{{{{P.Plain{{t'a'}}}}, {{}}, {{P.Plain{{t'b'}}}}}}}}, {{t'|'}})"),
    ("one block sep", f"return {B2I}({{P.Para{{t'a'}}}}, {{P.Space(), t'x'}})"),
    ("sep spaces", f"return {B2I}({{P.Para{{t'a'}}, P.Para{{t'b'}}}}, {{P.Space(), t'x', P.Space()}})"),
    ("sep soft", f"return {B2I}({{P.Para{{t'a', P.Space()}}, P.Para{{t'b'}}}}, {{P.Space(), t'x', P.SoftBreak()}})"),
    ("rule middle", f"return {B2I}({{P.Plain{{t'a'}}, P.HorizontalRule(), P.Plain{{t'b'}}}}, {{}})"),
    ("emph lists", f"return {B2I}({{P.Plain{{P.Emph{{t'a', t'b'}}}}, P.Plain{{P.Emph{{t'c'}}}}}}, {{}})"),
    ("code merge", f"return {B2I}({{P.CodeBlock('a'), P.CodeBlock('b')}}, {{}})"),
    ("note kept", f"return {B2I}{{P.Para{{P.Note{{P.Para{{t'n'}}}}}}}}"),
    ("pandoc argument", f"return {B2I}(P.Pandoc{{P.Para{{t'a'}}}})"),
]

MS = "P.utils.make_sections"
SECTION_CASES = [
    "false, nil, doc()", "true, nil, doc()", "true, 2, doc()",
    "false, 1, {H(3,'x'), H(5,'y'), H(4, 'z'), H(2,'w')}",
    "true, nil, {H(3,'x'), H(5,'y'), H(4, 'z'), H(2,'w')}",
    "true, nil, {H(1,'x', P.Attr('', {'unnumbered'})), H(2,'y'), H(1, 'z'), H(1,'q', P.Attr('', {'unlisted'}))}",
    "true, nil, {P.Div({H(1,'in'), P.Para{t'p'}}, P.Attr('dv', {'x'})), H(1,'after')}",
    "true, nil, {P.Div({H(1,'in'), P.Para{t'p'}}, P.Attr('dv', {'section'}))}",
    "true, nil, {H(1,'x', P.Attr('', {}, {number='9'})), H(1,'y')}",
    "true, 0, {H(1,'x'), H(2,'y')}", "true, -1, {H(1,'x'), H(2,'y')}",
    "'yes', nil, {}", "true, 'x', {}", "true, nil", "", "nil, nil, {H(1,'x')}",
    "1, nil, {H(1,'x')}", "true, nil, {P.BlockQuote{H(1,'bq')}, H(1, 'y')}",
    "true, 1.0, {H(2,'x')}", "true, 1.5, {}", "true, '2', {H(1,'x')}",
    "true, nil, {H(0,'zero'), H(1,'one')}",
    "true, nil, {H(2,'a'), H(1,'b'), H(3, 'c'), H(3, 'd'), H(2, 'e'), H(1, 'f')}",
    "true, nil, {P.Div({H(2,'in')}, P.Attr('', {'section'})), H(1,'after'), P.Div({H(2,'in2')}, P.Attr('', {'section'}))}",
    "true, nil, {P.Div({H(1,'in', P.Attr('hid', {'hc'}, {a='1'}))}, P.Attr('dv', {'dc'}, {b='2'}))}",
    "true, nil, {P.Div({P.Para{t'x'}, H(1,'in'), P.Para{t'y'}, H(2, 'sub')}, P.Attr('dv'))}",
    "true, nil, {H(1,'a'), P.Div({H(2,'in')}), H(3,'after-div'), H(2, 'two')}",
    "true, 1, {H(3,'top'), P.Div({H(2,'deep')}), P.BlockQuote{H(1,'bq')}}",
    "true, 1, {H(3,'top'), P.Div({P.Para{}, H(2,'deep')})}",
    "true, nil, {H(3,'top'), P.Div({P.Para{}, H(2,'deep')})}",
    "true, nil, {H(3,'top'), P.BlockQuote{H(1,'bq')}, H(4, 'x')}",
    "true, nil, {H(1,'a', P.Attr('', {'unnumbered'})), H(1, 'b'), H(2, 'c', P.Attr('', {'unnumbered'})), H(3, 'd'), H(2, 'e')}",
    "true, nil, {H(1,'a'), H(1, 'b', P.Attr('', {'-'})), H(1, 'c')}",
    "false, nil, {H(1,'a', P.Attr('x', {'section'}))}",
    "true, nil, {P.Div({H(1,'in')}, P.Attr('', {'section'}, {number='7'})), H(1, 'n')}",
    "false, nil, {P.Div({H(1,'in')}, P.Attr('', {'section'}, {number='7'})), H(1, 'n')}",
    "true, nil, {P.Div({H(1,'in', P.Attr('', {}, {number='8'}))}), H(1, 'n')}",
    "true, 2, {}",
    "true, nil, {P.Div({H(2,'a')}), P.Div({H(1,'b')}), H(2, 'c')}",
    "true, nil, {H(1, 'x'), P.Div({P.Div({H(2, 'deep')})}), H(2, 'y')}",
    "true, nil, {H(1, 'x', P.Attr('', {}, {number=''})), H(1, 'y')}",
    "true, nil, {P.Div({H(1, 'x')}, P.Attr('', {'unnumbered'})), H(1, 'y')}",
    "true, nil, {H(2, 'x'), P.Div({H(1, 'div1')}), H(2, 'y')}",
    "true, nil, {P.Div({H(1,'in', P.Attr('hid'))})}",
    "true, 1, {H(3,'top'), P.BlockQuote{H(2,'bq')}, P.Div({H(4, 'd')}), P.Div({P.Para{}, H(5,'e')})}",
    "true, nil, {P.Div({H(1,'in', P.Attr('', {'unnumbered'})), H(2, 'x'), H(1, 'y'), P.Para{}})}",
    "true, nil, {P.Div({H(1,'in', P.Attr('h', {'c'}, {{'number', '5'}, {'z', '1'}}))}, P.Attr('', {'d', 'c', 'section'}, {{'z', '2'}, {'y', '3'}}))}",
    "false, nil, {P.Div({H(1,'in', P.Attr('h'))}, P.Attr('d'))}",
    "true, nil, {H(1, 'a'), P.Para{t'p'}, P.Div({H(2, 'in'), P.Para{t'q'}}), P.Para{t'r'}, H(2, 'b')}",
    "true, nil, {P.Div({H(2, 'x')}), P.Div({H(1, 'y')})}",
    "true, 1, {P.Div({H(3, 'x')}), H(2, 'y'), P.OrderedList{{H(1, 'li')}}}",
    "true, nil, {P.Figure({H(1,'f')}), P.Div({P.Div({H(1, 'dd')})}, P.Attr('outer'))}",
    "true, nil, {P.Div({H(1, 'x'), P.Div({H(1, 'y')})})}",
    "true, nil, {P.Div({H(1,'x'), P.BlockQuote{H(1, 'q')}}, P.Attr('d'))}",
    "true, nil, {H(3,'x'), P.Para{P.Note{H(1, 'n')}}}",
    "true, 1, {H(3,'x'), P.Para{P.Note{H(2, 'n')}}}",
    "true, nil, {P.Div({H(2,'x'), H(3, 'y'), P.Div({H(2, 'z')})}, P.Attr('d'))}",
    "true, nil, {P.Div({H(2,'x'), H(3, 'y'), P.Div({H(3, 'z')})}, P.Attr('d'))}",
    "true, 3, {H(1,'x', P.Attr('', {'unnumbered'})), H(2, 'y')}",
    "true, nil, {P.Div({H(1,'x')}, P.Attr('', {}, {number='3'})), H(1, 'y')}",
    "true, nil, {H(2,'x'), P.Table(P.Caption({H(1, 'cap')}), {{P.AlignLeft, 0.5}}, P.TableHead(), {}, P.TableFoot())}",
    "true, nil, {H(2,'x'), P.Para{P.Emph{P.Note{H(1, 'n')}}}}",
    "true, nil, {H(2,'x'), P.DefinitionList{{{t'term'}, {{H(1, 'def')}}}}}",
    "true, nil, {H(1, 'x'), P.Div({}), P.Div({P.Para{}}), H(1, 'y')}",
    "true, nil, {P.Para{t'a'}, P.Para{t'b'}}",
    "true, nil, P.Blocks{H(1, 'x')}",
    "true, nil, H(1, 'x')",
    "true, nil, {t'x'}",
    "true, nil, {H(1, 'x', P.Attr('i', {'unnumbered', 'unnumbered'}, {number='1'}))}",
    "true, 5, {H(1, 'x'), H(2, 'y'), H(9, 'z')}",
    "true, math.maxinteger, {H(1, 'x')}",
    "true, nil, {H(1, 'a'), H(4, 'b'), H(2, 'c'), H(4, 'd'), H(3, 'e'), H(1, 'f'), H(3, 'g')}",
    "false, nil, {P.Div({H(1, 'a'), P.Para{t'p'}}, P.Attr('', {}, {number='4'}))}",
]
GROUPS["utils-sections"] = [
    (f"make_sections({args})", f"return {MS}({args})") for args in SECTION_CASES
]


NORMALIZE_CASES = [
    "P.Para{t'a', t'b'}", "P.Para{P.Emph{t'a', t'b'}}", "P.Para{P.Link({t'a', t'b'}, 'u')}",
    "P.Para{P.Note{P.Para{t'a', t'b'}}}", "P.Para{P.Space(), P.Space(), t'a'}",
    "P.Para{P.Emph{P.Space(), P.Space(), t'a'}}", "P.Para{P.SoftBreak(), P.SoftBreak(), t'a'}",
    "P.Para{P.Emph{P.SoftBreak(), P.SoftBreak(), t'a'}}", "P.Para{t'a', t''}", "P.Para{t''}",
    "P.Para{t'', t''}", "P.Para{P.Emph{t'a'}, P.Emph{t'b'}}", "P.Para{P.Emph{t'a'}, P.Emph{t'b', t'c'}}",
    "P.Para{P.Strong{t'a'}, P.Strong{t'b'}, P.SmallCaps{t'c'}, P.SmallCaps{t'd'}}",
    "P.Para{P.Underline{t'a'}, P.Underline{t'b'}, P.Strikeout{t'c'}, P.Strikeout{t'd'}, P.Subscript{t'e'}, P.Subscript{t'f'}, P.Superscript{t'g'}, P.Superscript{t'h'}}",
    "P.Para{P.Space(), P.LineBreak(), P.Space(), P.SoftBreak(), P.LineBreak(), P.LineBreak(), P.SoftBreak(), P.Space()}",
    "P.Para{P.Span({t'a'}), P.Span({t'b'})}", "P.Para{P.Quoted('DoubleQuote', {t'a', t'b'})}",
    "P.Para{P.Cite({t'a', t'b'}, {P.Citation('k', 'NormalCitation', {t'p', t'q'}, {P.Space(), P.Space()})})}",
    "P.Para{P.Image({t'a', t'b'}, 'u')}", "P.Header(1, {t'a', t'b'})", "P.Plain{t'a', t'b'}",
    "P.LineBlock{{t'a', t'b'}, {P.Space(), P.Space()}}", "P.DefinitionList{{{t'a', t'b'}, {{P.Plain{t'c', t'd'}}}}}",
    "P.Div{P.BlockQuote{P.Para{t'a', t'b'}}}", "P.BulletList{{P.Plain{t'a', t'b'}}}",
    "P.Figure({P.Plain{t'a', t'b'}}, P.Caption({P.Plain{t'c', t'd'}}, {t'e', t'f'}))",
    "P.Para{P.Emph{P.Emph{t'a', t'b'}, P.Emph{t'c'}}}", "P.Para{P.Code('a'), P.Code('b'), P.Math('InlineMath', 'x')}",
    "P.Para{P.Str'a', P.Emph{}, P.Str'b'}", "P.Para{P.Emph{}, P.Emph{}}",
]
TABLE_CASES = [
    "{{P.AlignLeft, 0.5}, {P.AlignRight, 0.5}}, P.TableHead{P.Row{cell'a'}}, {}, P.TableFoot()",
    "{{P.AlignLeft, 0.5}}, P.TableHead{P.Row{cell'a', cell'b', cell'c'}}, {}, P.TableFoot()",
    "{{P.AlignLeft, 0.5}, {P.AlignLeft, 0.5}, {P.AlignLeft, 0.5}}, P.TableHead{P.Row{P.Cell({P.Plain'x'}, 'AlignDefault', 2, 2), cell'b'}, P.Row{cell'c', cell'd'}, P.Row{cell'e'}}, {}, P.TableFoot()",
    "{{P.AlignLeft, 0.5}, {P.AlignLeft, 0.5}}, P.TableHead{P.Row{P.Cell({}, 'AlignDefault', 5, 0)}, P.Row{P.Cell({}, 'AlignDefault', -2, 7)}}, {}, P.TableFoot()",
    "{{P.AlignLeft, 0.5}, {P.AlignLeft, 0.5}, {P.AlignLeft, 0.5}}, P.TableHead(), {{attr = P.Attr(), row_head_columns = 2, head = {P.Row{cell'h'}}, body = {P.Row{P.Cell({}, 'AlignDefault', 1, 3), cell'x'}, P.Row{cell'a', cell'b', cell'c', cell'd'}}}}, P.TableFoot()",
    "{{P.AlignLeft, 0.5}, {P.AlignLeft, 0.5}}, P.TableHead(), {{attr = P.Attr(), row_head_columns = 5, head = {}, body = {P.Row{cell'a'}}}}, P.TableFoot{P.Row{cell'f', cell'g', cell'h'}}",
    "{{P.AlignLeft, 0.5}, {P.AlignLeft, 0.5}}, P.TableHead(), {{attr = P.Attr(), row_head_columns = -1, head = {}, body = {P.Row{P.Cell({}, 'AlignDefault', 3, 1), cell'b'}, P.Row{cell'c', cell'd'}, P.Row{cell'e', cell'f'}}}}, P.TableFoot()",
    "{}, P.TableHead{P.Row{cell'a'}}, {{attr = P.Attr(), row_head_columns = 1, head = {}, body = {P.Row{cell'b'}}}}, P.TableFoot{P.Row{cell'c'}}",
    "{{P.AlignLeft, 0.5}, {P.AlignLeft, 0.5}, {P.AlignLeft, 0.5}}, P.TableHead{P.Row{cell'a', P.Cell({}, 'AlignDefault', 2, 1), cell'c'}, P.Row{P.Cell({}, 'AlignDefault', 1, 3)}}, {}, P.TableFoot()",
    "{{P.AlignLeft, 0.5}, {P.AlignLeft, 0.5}, {P.AlignLeft, 0.5}}, P.TableHead{P.Row{}}, {{attr = P.Attr(), row_head_columns = 1, head = {}, body = {P.Row{P.Cell({}, 'AlignDefault', 2, 2), cell'b'}, P.Row{cell'c', cell'd', cell'e'}}}}, P.TableFoot()",
]
GROUPS["pandoc-normalize"] = [
    (f"normalize {case}", f"return P.Pandoc{{{case}}}:normalize()") for case in NORMALIZE_CASES
] + [
    (f"normalize table {case}", f"return P.Pandoc{{P.Table(P.Caption({{P.Plain{{t'a', t'b'}}}}), {case})}}:normalize()")
    for case in TABLE_CASES
] + [
    ("normalize meta", "return P.Pandoc({}, {m = P.MetaInlines{t'a', t'b'}, l = P.MetaList{P.MetaBlocks{P.Para{t'c', t'd'}}}}):normalize()"),
    ("normalize keeps original", "local d = P.Pandoc{P.Para{t'a', t'b'}} local n = d:normalize() return d, n"),
    ("normalize no self", "return P.Pandoc.normalize()"),
    ("normalize block", "return P.Pandoc.normalize(P.Para{})"),
]


PAIRS_VALUES = {
    "Str": "P.Str'a'", "Emph": "P.Emph{'a'}", "Code": "P.Code('c')", "Link": "P.Link('a', 'u')",
    "Image": "P.Image('a', 's')", "Cite": "P.Cite('a', {P.Citation('k', 'NormalCitation')})",
    "Math": "P.Math('InlineMath', 'x')", "Note": "P.Note{P.Para'x'}", "Quoted": "P.Quoted('DoubleQuote', 'a')",
    "RawInline": "P.RawInline('html', 'x')", "Span": "P.Span('a')", "Space": "P.Space()",
    "Para": "P.Para'a'", "Header": "P.Header(1, 'a')", "CodeBlock": "P.CodeBlock('x')", "Div": "P.Div{}",
    "BulletList": "P.BulletList{}", "OrderedList": "P.OrderedList{}", "DefinitionList": "P.DefinitionList{}",
    "Table": "P.Table(P.Caption(), {}, P.TableHead(), {}, P.TableFoot())", "Figure": "P.Figure{}",
    "LineBlock": "P.LineBlock{}", "RawBlock": "P.RawBlock('html', 'x')", "HorizontalRule": "P.HorizontalRule()",
    "BlockQuote": "P.BlockQuote{}", "Attr": "P.Attr('i', {'c'}, {k='v'})",
    "Citation": "P.Citation('k', 'NormalCitation')", "ListAttributes": "P.ListAttributes()",
    "Cell": "P.Cell{P.Plain'x'}", "Row": "P.Row{P.Cell{}}", "TableHead": "P.TableHead()",
    "TableFoot": "P.TableFoot()", "Caption": "P.Caption()", "Pandoc": "P.Pandoc{}",
    "Version": "P.types.Version'1.2'", "AttributeList": "P.Attr('', {}, {{'a', '1'}, {'b', '2'}}).attributes",
    "changed Str": "(function() local s = P.Str'a' s.text = 'b' return s end)()",
    "Caption with short": "P.Caption({P.Plain'x'}, {P.Str'y'})",
}
GROUPS["object-pairs"] = [
    (f"pairs {name}", "local keys = {} for k, v in pairs(" + value + ") do keys[#keys + 1] = tostring(k) .. ':' .. type(v) end return table.concat(keys, ' ')")
    for name, value in PAIRS_VALUES.items()
] + [
    ("pairs missing utils function", "return #P.utils.citeproc"),
    ("state copy", "PANDOC_STATE.resource_path:insert('new') return table.concat(PANDOC_STATE.resource_path, '|')"),
    ("state headers copy", "local h = PANDOC_STATE.request_headers h[1][1] = 'X' return PANDOC_STATE.request_headers[1][1]"),
    ("state equal lists", "return PANDOC_STATE.resource_path == PANDOC_STATE.resource_path, rawequal(PANDOC_STATE.resource_path, PANDOC_STATE.resource_path)"),
    ("state unknown key", "return PANDOC_STATE.nothing"),
    ("state set", "PANDOC_STATE.trace = true return PANDOC_STATE.trace, type(PANDOC_STATE), getmetatable(PANDOC_STATE).__name"),
    ("state pairs", "local n = 0 for _ in pairs(PANDOC_STATE) do n = n + 1 end return n"),
] + [
    (f"attribute index {i} {v}", f"local x = P.AttributeList{{{{'a','b'}},{{'c','d'}}}} x[{i}] = {v} return x")
    for i in ["math.mininteger", "0", "-1", "1", "2", "3", "math.maxinteger"]
    for v in ["'z'", "{'q','z'}", "nil"]
] + [
    (f"integer {text}", f"return P.Header({lua_string(text)}, 'h')")
    for text in ["(3)", "- 3", "((3))", "( 3 )", "(- 3)", "-(3)", "(3", "3)", " -  3 ", "()", "(0x10)", "--3", "-\t3", "(-3)"]
]

GROUPS["object-errors"] = [
    (f"{op} {name}", f"local v = {value} return {expr}")
    for name, value in PAIRS_VALUES.items()
    for op, expr in [
        ("call", "v()"), ("concat", "v .. 'x'"), ("concat left", "'x' .. v"), ("length", "#v"),
        ("less", "v < v"), ("less equal", "v <= v"), ("add", "v + 1"), ("minus", "-v"),
        ("getmetatable", "getmetatable(v)"), ("index number", "v[1]"),
    ]
] + [
    ("getmetatable nil", "return getmetatable(nil)"),
    ("getmetatable string", "return getmetatable('x').__index == string"),
    ("getmetatable list", "return getmetatable(P.List{}) == P.List"),
]

PATHS = [
    "", ".", "..", "/", "//", "///", "a", "a/", "a//", "/a", "//a", "a/b", "a/b/", "/a/b/c",
    "a/./b", "./a", "a/.", "a/..", "../a", "a/../b", "a//b///c", "/./a", "a.txt", "a.b.c",
    ".bashrc", "a/.bashrc", "a.d/b", "a.d/b.e", "a/b.", "a.", "./", "a/./", "/a/./.",
    "x y/z.tar.gz", "\u00e9/\u00fc.md", "a:b", "C:\\x\\y.z",
]
GROUPS["path"] = [
    (f"{fn} {path!r}", f"return P.path.{fn}({lua_string(path)})")
    for fn in ["directory", "filename", "is_absolute", "is_relative", "normalize", "split",
               "split_extension", "split_search_path"]
    for path in PATHS
] + [
    (f"join {args}", f"return P.path.join({args})")
    for args in ["{}", "{'a'}", "{'a', 'b'}", "{'a/', 'b'}", "{'a', '/b'}", "{'', 'b'}", "{'a', ''}",
                 "{'/', 'a', 'b/', 'c'}", "{'a', 'b', '/c', 'd'}", "'a'", "{1, 2}", "{'a', {}}", ""]
] + [
    (f"make_relative {args}", f"return P.path.make_relative({args})")
    for args in ["'/a/b/c', '/a'", "'/a/b/c', '/a/'", "'/a', '/a'", "'/a/', '/a'", "'a/b', 'a'",
                 "'/a/b', '/c'", "'/a/b', '/c', true", "'/a/b', '/a/c/d', true", "'a/b', '/a'",
                 "'/a/b', 'a'", "'a/b', 'a/b/c', true", "'a/./b', 'a', false", "'//a/b', '/a'",
                 "'a/b', 'c'", "'a/b', 'c', 1", "'./a', '.'", "'a', '.'", "'/a/b/', '/a/b/c/', true",
                 "'/x', '/x', true", "'a', nil", "nil, 'a'", "'a'", "'a', 'b', 'yes'"]
] + [
    ("separators", "return P.path.separator, P.path.search_path_separator"),
    ("directory number", "return P.path.directory(12)"),
    ("directory table", "return P.path.directory({})"),
    ("directory none", "return P.path.directory()"),
    ("split_search_path none", "return P.path.split_search_path()"),
    ("module names", "local n = {} for k in pairs(P.path) do n[#n + 1] = k end table.sort(n) return table.concat(n, ' ')"),
    ("require", "return require('pandoc.path') == P.path"),
    ("treat strings", "P.path.treat_strings_as_paths() return ('a' / 'b') + 'c', ('x.y'):split_extension(), ('/a/b'):directory(), ('a' + '.md')"),
    ("treat strings error", "P.path.treat_strings_as_paths() return 'a' / {}"),
]

TEXTS = ["", "abc", "ÄÖÜ äöü", "straße", "ΣΑΣ σας", "İı", "ǅǆǄ", "ﬃ", "日本語", "😀a", "a\u0301",
         "ŉ", "ΐ", "ǰ", "ﬀ", "ᾳ", "Ꭰꭰ", "ꞵ", "Ⓐⓐ"]
GROUPS["text"] = [
    (f"{fn} {text!r}", f"return P.text.{fn}({lua_string(text)})")
    for fn in ["len", "lower", "upper", "reverse", "subscript", "superscript"]
    for text in TEXTS
] + [
    (f"sub {args}", f"return P.text.sub({args})")
    for args in ["'häßlich', 2", "'häßlich', 2, 4", "'häßlich', -3", "'häßlich', -3, -2", "'häßlich', 0",
                 "'häßlich', 0, 0", "'häßlich', 10", "'häßlich', 3, 1", "'häßlich', -100, 100",
                 "'häßlich', math.mininteger", "'häßlich', 1, math.mininteger", "'häßlich', math.maxinteger",
                 "'häßlich', 2.0", "'häßlich', 1.5", "'häßlich', '2'", "'häßlich'", "'häßlich', 1, nil",
                 "'', 1", "12345, 2, 3", "nil, 1"]
] + [
    (f"script {fn} {text!r}", f"return P.text.{fn}({lua_string(text)})")
    for fn in ["subscript", "superscript"]
    for text in ["(1+2)=3", "x2", "−1", "\t1\n", "\u00a01\u3000", "\u00852", "\u20282", "i", "n", " ", "2²", "₂", "-+", "0123456789"]
] + [
    (f"toencoding {enc}", f"local r = P.text.toencoding('aé€', {enc}) return (r:gsub('.', function(c) return string.format('%02x', c:byte()) end))")
    for enc in ["'UTF-8'", "'utf8'", "'UTF-16LE'", "'UTF-16'", "'UTF-16BE'", "'UTF-32'", "'UTF-32LE'",
                "'UTF-32BE'", "'latin1'", "'ISO-8859-1'", "'ASCII'", "'nonsense'", "'UTF-8//IGNORE'",
                "'utf-16be'", "nil", "5"]
] + [
    (f"toencoding latin {enc}", f"local r = P.text.toencoding('aé', {enc}) return (r:gsub('.', function(c) return string.format('%02x', c:byte()) end))")
    for enc in ["'latin1'", "'ASCII'", "'UTF-16LE'"]
] + [
    (f"fromencoding {args}", f"return P.text.fromencoding({args})")
    for args in ["'\\255\\254', 'UTF-8'", "'a\\0', 'UTF-16LE'", "'\\0a', 'UTF-16'", "'a\\195\\169'",
                 "'a\\255b'", "'a\\195b'", "'\\233', 'latin1'", "'\\233', 'ASCII'", "'a', 'nonsense'",
                 "12", "", "{}", "'a', 5", "'\\0\\0\\0a', 'UTF-32'", "'a\\0\\0\\0', 'UTF-32LE'",
                 "'\\0\\216', 'UTF-16LE'", "'a', 'UTF-16LE'", "'\\0\\0\\17\\0', 'UTF-32'",
                 "'a\\195\\169', 'utf8'", "'\\240\\159\\152\\128x', 'UTF-8'"]
] + [
    ("text module names", "local n = {} for k in pairs(P.text) do n[#n + 1] = k end table.sort(n) return table.concat(n, ' ')"),
    ("require text", "return require('pandoc.text') == P.text, require('text') == P.text"),
    ("len none", "return P.text.len()"),
    ("upper table", "return P.text.upper({})"),
]

JSON_ENCODE = [
    "nil", "", "true", "false", "42", "-7", "0", "-0.0", "1.5", "3.0", "-2.5", "1e300", "1e-7", "1e-6",
    "0.1", "1/3", "1e21", "1e7", "1e8", "12345678.5", "1234567.5", "123.456", "0.000123",
    "math.maxinteger", "math.mininteger", "2^53", "2^53 + 1", "1/0", "-1/0", "0/0", "5e-324",
    "'a\"b\\\\c\\n\\t\\1\\8\\12\\13\\31/<>&\\127'", lua_string("é€😀\u2028"), "'\\255'", "''",
    "{}", "{1, 'a', true}", "{1, nil, 3}", "{b = 1, a = 2, c = {d = 3}}", "{1, 2, x = 3}",
    "{[2] = 1, [3] = 2}", "{[1.5] = 1}", "{[true] = 1}", "{[{}] = 1}", "{['b'] = 1, [2] = 2}",
    "{['B'] = 1, ['a'] = 2, ['é'] = 3, ['Z'] = 4}", "P.json.null", "{1, P.json.null}",
    "{a = P.json.null}", "P.Str'a'", "P.Para{'a'}", "P.Inlines{'a b'}", "P.Blocks{P.Para'x'}",
    "P.Pandoc({P.Para'x'}, {a = 'b', n = 1, l = {true, 'x'}})", "P.Attr('i', {'c'}, {k = 'v'})",
    "P.Meta{a = 'b'}", "P.List{1, 2}", "P.types.Version'1.2'", "print", "coroutine.create(print)",
    "setmetatable({}, {__tojson = function() return '\"X\"' end})",
    "{setmetatable({}, {__tojson = function() return '\"X\"' end})}",
    "setmetatable({}, {__tojson = function() return 5 end})",
    "setmetatable({}, {__tojson = function() error('boom') end})",
    "P.Citation('k', 'NormalCitation')", "P.Caption()", "P.Cell{}", "P.Row{}", "P.TableHead()",
    "P.TableFoot()", "P.ListAttributes()", "P.Attr().attributes",
    "P.Table(P.Caption(), {{P.AlignLeft, 0.5}, {P.AlignRight, P.ColWidthDefault}}, P.TableHead(), {}, P.TableFoot())",
    "P.Image('a', 's', 't')", "P.Math('InlineMath', 'x')", "P.Header(2, 'h', P.Attr('i'))",
    "P.OrderedList({P.Plain'a'}, P.ListAttributes(3, 'LowerRoman', 'Period'))",
    "(function() local s = P.Str'a' s.text = 'changed' return s end)()",
    "{P.Str'a', 1}", "{x = P.Para'p'}", "P.MetaInlines{'a'}", "P.MetaBool(true)",
]
JSON_DECODE = [
    "'{\"a\":1,\"b\":[1,2.5,null,true],\"c\":{}}'", "'{\"t\":\"Str\",\"c\":\"a\"}'",
    "'{\"t\":\"Str\",\"c\":\"a\"}', false", "'{\"t\":\"Str\",\"c\":\"a\"}', true",
    "'{\"t\":\"Str\",\"c\":\"a\"}', nil", "'{\"t\":\"Str\",\"c\":\"a\"}', 0",
    "'[{\"t\":\"Str\",\"c\":\"a\"}]'", "'[{\"t\":\"Para\",\"c\":[]}]'", "'[]'", "'[]', false",
    "'{\"pandoc-api-version\":[1,23,1,2],\"meta\":{},\"blocks\":[]}'", "'{'", "'[1,]'", "'12'",
    "'1.0'", "'1e2'", "'-0'", "'123456789012345678901234567890'", "'1e400'", "'01'", "'.5'", "'1.'",
    "'\"a\\\\u00e9\\\\ud83d\\\\ude00\"'", "'\"\\\\ud83d\"'", "'\"\\\\ud83dx\"'", "'\"\\\\x\"'",
    "''", "'  '", "", "{}", "12", "'1 2'", "'  [1]  '", "'{\"a\":1,\"a\":2}'", "'null'", "'true'",
    "'\"s\"'", "'[[[]]]'", "'{\"t\":\"Str\"}'", "'{\"t\":\"Str\",\"c\":\"a\",\"x\":1}'",
    "'{\"t\":\"Space\"}'", "'[{\"t\":\"Space\"}, {\"t\":\"Para\",\"c\":[]}]'",
    "'{\"t\":\"MetaString\",\"c\":\"x\"}'", "'[\"i\",[],[]]'", "'\"\\t\"'", "'\"\\\\/\"'",
    "'{\"b\":1,\"a\":{\"c\":[1,{\"d\":null}]}}'", "'[1,null,3]'", "'{\"1\":\"x\",\"2\":\"y\"}'",
]
GROUPS["json"] = [
    (f"encode {value}", f"return P.json.encode({value})") for value in JSON_ENCODE
] + [
    (f"decode {args}", f"return P.json.decode({args})") for args in JSON_DECODE
] + [
    ("null", "return type(P.json.null), P.json.null == P.json.null, tostring(P.json.null):gsub('0x%x+', 'ADDR')"),
    ("decode null identity", "return P.json.decode('null') == P.json.null, P.json.decode('[null]')[1] == P.json.null"),
    ("decode list metatable", "local t = P.json.decode('[1]') return getmetatable(t) == P.List, getmetatable(P.json.decode('{}'))"),
    ("roundtrip", "return P.json.encode(P.json.decode('{\"x\":[1,2,{\"y\":\"z\"}],\"a\":false}'))"),
    ("tojson Inlines", "return getmetatable(P.Inlines{}).__tojson(P.Inlines{'x y'})"),
    ("module names json", "local n = {} for k in pairs(P.json) do n[#n + 1] = k end table.sort(n) return table.concat(n, ' ')"),
    ("require json", "return require('pandoc.json') == P.json"),
    ("encode deep", "local t = {} local x = t for i = 1, 100 do x[1] = {} x = x[1] end return #P.json.encode(t)"),
]

GROUPS["system"] = [
    ("system names", "local n = {} for k in pairs(P.system) do n[#n + 1] = k end table.sort(n) return table.concat(n, ' ')"),
    ("system field types", "return type(P.system.os), type(P.system.arch)"),
    ("read_file none", "return P.system.read_file()"),
    ("read_file table", "return P.system.read_file({})"),
    ("write_file one", "return P.system.write_file('x.txt')"),
    ("write_file table", "return P.system.write_file({}, 'a')"),
    ("write_file contents table", "return P.system.write_file('x.txt', {})"),
    ("require system", "return require('pandoc.system') == P.system"),
]

FORMATS = ("ansi asciidoc asciidoc_legacy asciidoctor bbcode bbcode_fluxbb bbcode_hubzilla bbcode_phpbb "
    "bbcode_steam bbcode_xenforo beamer biblatex bibtex bits chunkedhtml commonmark commonmark_x context creole "
    "csljson csv djot docbook docbook4 docbook5 docx dokuwiki dzslides endnotexml epub epub2 epub3 fb2 fodt gfm "
    "haddock html html4 html5 icml ipynb jats jats_archiving jats_articleauthoring jats_publishing jira json latex "
    "man markdown markdown_github markdown_mmd markdown_phpextra markdown_strict markua mdoc mediawiki ms muse "
    "native odt opendocument opml org pdf plain pod pptx revealjs ris rst rtf s5 slideous slidy t2t tei texinfo "
    "textile tikiwiki tsv twiki typst vimdoc vimwiki xlsx xml xwiki zimwiki nonsense MARKDOWN markdown+smart "
    "gfm-smart texinfo2 docbook6 doc odp ods zip djot2").split()
EXTENSIONS = ("adoc asciidoc asc bib biblatex bibtex cbz commonmark context creole csl csv ctx db dbk djot dj doc "
    "docbook docx dokuwiki dw epub fb2 htm html xhtml icml ipynb jats jira json latex ltx lhs md markdown mkd mkdn "
    "mdwn mdown mdtxt mdtext text txt man ms muse native odt opml org pdf pod pptx xlsx rst rtf s5 t2t tei tex texi "
    "texinfo textile tsv typ typst wiki xml 1 2 3 4 5 6 7 8 9 0 10 1p 3pm roff mdoc bits rmd qmd yaml yml markua "
    "mediawiki haddock vimdoc xwiki zim tikiwiki twiki ris endnotexml enl gz zip png svg pptm docm odp ods ansi "
    "chunkedhtml beamer slidy revealjs dzslides slideous").split()
GROUPS["format"] = [
    (f"{fn} {name}", f"local r = P.format.{fn}({lua_string(name)}) "
     "if getmetatable(r) then return table.concat(r, ' ') end "
     "local k = {} for e, on in pairs(r) do k[#k + 1] = e .. '=' .. tostring(on) end table.sort(k) return table.concat(k, ' ')")
    for name in FORMATS for fn in ["default_extensions", "all_extensions", "extensions"]
] + [
    ("default list type", "return getmetatable(P.format.default_extensions('html')) == P.List"),
    ("from_path extensions", "local out = {} for _, e in ipairs{" + ", ".join(lua_string(e) for e in EXTENSIONS)
     + "} do out[#out + 1] = e .. '=' .. tostring(P.format.from_path('x.' .. e)) end return table.concat(out, ' ')"),
] + [
    (f"from_path {path!r}", f"return P.format.from_path({lua_string(path)})")
    for path in ["x.MD", "x.Md", "X.HTML", "dir.md/x", "x.md/", ".md", "x..md", "x.md.gz", "x.tar.gz", "README",
                 "x.1.gz", "x.md ", "x. md", "x.md?y", "x.md#a", "https://h/x.html?y=1", "https://h/x.html#f",
                 "x.LaTeX", "x.TXT", "", ".", "a/b/c.rst", "C:\\x\\y.md", "ftp://h/y.tex"]
] + [
    ("from_path list", "return P.format.from_path({'x.zzz', 'y.md'}), P.format.from_path({'y.md', 'x.zzz'}), P.format.from_path({'x.zzz'}), P.format.from_path({})"),
    ("from_path number", "return P.format.from_path(5)"),
    ("from_path none", "return P.format.from_path()"),
    ("from_path table bad", "return P.format.from_path({{}})"),
    ("default none", "return P.format.default_extensions()"),
    ("require format", "return require('pandoc.format') == P.format"),
    ("constructor tables", "local n = {} for k, v in pairs(P.Inline.constructor) do n[#n + 1] = k .. tostring(v == P[k]) end "
     "for k, v in pairs(P.Block.constructor) do n[#n + 1] = k .. tostring(v == P[k]) end table.sort(n) return table.concat(n, ' ')"),
    ("constructor call", "return P.Inline.constructor.Str('a'), P.Block.constructor.Para{'x'}, P.utils.type(P.Block)"),
    ("constructor error", "return P.Inline.constructor.Str()"),
    ("Block keys", "local n = {} for k in pairs(P.Block) do n[#n + 1] = k end return table.concat(n, ' '), getmetatable(P.Block)"),
]

SHOW_VALUES = [
    "P.Attr('i', {'c', 'd'}, {{'k', 'v'}, {'q', 'a\"b'}})", "P.Attr().attributes", "P.Attr('', {}, {{'x', 'y'}}).attributes",
    "P.Citation('k', 'AuthorInText', {P.Str'p'}, {P.Str's'}, 3, 7)", "P.Citation('k', 'SuppressAuthor')",
    "P.Caption()", "P.Caption({P.Para'x'})", "P.Caption({P.Para'x'}, {P.Str'short'})",
    "P.Cell{P.Plain'x'}", "P.Cell({P.Plain'x'}, 'AlignRight', 2, 3, P.Attr('c'))",
    "P.Row{P.Cell{}}", "P.Row({P.Cell{}}, P.Attr('r'))", "P.TableHead()", "P.TableHead({P.Row{}}, P.Attr('h'))",
    "P.TableFoot()", "P.TableFoot({P.Row{}}, P.Attr('f'))",
    "P.Table(P.Caption({P.Plain'c'}, {P.Str's'}), {{P.AlignLeft, 0.5}, {P.AlignCenter, P.ColWidthDefault}}, P.TableHead{P.Row{P.Cell{}}}, {{attr = P.Attr('b'), row_head_columns = 1, head = {P.Row{}}, body = {P.Row{P.Cell{}, P.Cell{}}}}}, P.TableFoot{P.Row{}}, P.Attr('t'))",
    "P.Table(P.Caption(), {{P.AlignLeft, 0.5}, {P.AlignCenter, P.ColWidthDefault}}, P.TableHead{P.Row{P.Cell{}}}, {{attr = P.Attr('b'), row_head_columns = 1, head = {P.Row{}}, body = {P.Row{P.Cell{}, P.Cell{}}}}}, P.TableFoot{P.Row{}}, P.Attr('t')).bodies[1]",
    "P.Pandoc({P.Para'x'}, {a = 'b', n = true, f = false, l = {'x', {P.Para'y'}}, m = {k = P.MetaInlines{'i'}}, e = {}, s = P.MetaString'str', bl = P.MetaBlocks{P.Para'b'}})",
    "P.Meta{a = 'b'}", "P.types.Version'1.2.3'", "P.types.Version{2, 0}", "P.ListAttributes(2, 'UpperAlpha', 'TwoParens')",
    "P.Figure({P.Plain'x'}, P.Caption({P.Plain'c'}), P.Attr('fig'))", "P.Image({P.Str'a'}, 'src', 'title', P.Attr('img'))",
    "P.Cite({P.Str'c'}, {P.Citation('k', 'NormalCitation')})", "P.Math('DisplayMath', 'x^2')",
    "P.Quoted('SingleQuote', {P.Str'q'})", "P.Link({P.Str'l'}, 'u', 't', P.Attr('a'))",
    "P.OrderedList({{P.Plain'a'}}, P.ListAttributes(3, 'LowerRoman', 'OneParen'))",
    "P.DefinitionList{{{P.Str't'}, {{P.Plain'd'}}}}", "P.LineBlock{{P.Str'a'}, {P.Str'b'}}",
    "P.RawBlock('tex', '\\\\x')", "P.CodeBlock('c\\n\"q\"', P.Attr('', {'lua'}))", "P.Header(-3, {P.Str'h'})",
    "P.Str'\\1\\127 \\195\\169 \\226\\130\\172 \\240\\159\\152\\128'", "P.Str'tab\\there'", "P.Str'\\\\back\"'",
    "P.Note{P.Para'n'}", "P.Span({P.Str's'}, P.Attr('', {}, {{'k', 'v'}}))", "P.Inlines{P.Str'a', P.Space()}",
    "P.Blocks{P.HorizontalRule(), P.Div({}, P.Attr('d'))}",
]
GROUPS["show"] = [
    (f"tostring {value}", f"return tostring({value})") for value in SHOW_VALUES
]

GROUPS["structure"] = [
    (f"make_sections {args}", f"return P.structure.make_sections({args})")
    for args in ["doc()", "doc(), {number_sections = true}", "doc(), {number_sections = true, base_level = 2}",
                 "P.Pandoc(doc())", "", "doc(), 5", "doc(), 'x'", "doc(), {base_level = 'x'}", "doc(), {base_level = 1.5}",
                 "P.Para'x'", "{}", "doc(), {number_sections = 1}", "doc(), nil", "{H(1, 'x')}, {base_level = 3, number_sections = true}"]
] + [
    (f"slide_level {args}", f"return P.structure.slide_level({args})")
    for args in ["doc()", "{H(1,'a'), H(2,'b'), P.Para'x'}", "{P.Para'x'}", "{H(1,'a'), P.HorizontalRule(), H(3, 'x'), P.Para'p'}",
                 "P.Pandoc{H(2,'a'), P.Para'x'}", "{H(2, 'a')}", "{}", "{P.Div{H(1, 'a'), P.Para'x'}, H(3, 'b'), P.Para'y'}",
                 "{H(4, 'a'), P.Div{}, H(2, 'b'), H(1, 'c')}", "{H(9, 'a'), P.Para'x'}", "{H(0, 'a'), P.Para'x'}", "", "5"]
] + [
    ("unique identifiers", "local used, out = {}, {} for _, t in ipairs{'Hello World', 'Hello World', '', '123 abc', "
     "'Ünïcödé', 'a.b_c-d e', '  ', '!!!', 'Hello World'} do local id = P.structure.unique_identifier({P.Str(t)}, used) "
     "used[id] = true out[#out + 1] = id end return table.concat(out, ' ')"),
] + [
    (f"unique_identifier {args}", f"return P.structure.unique_identifier({args})")
    for args in ["{P.Str'Hello', P.Space(), P.Emph{P.Str'World'}}", "{P.Str'Ünï'}, {}, {'ascii_identifiers'}",
                 "{P.Str'A B'}, {}, {'gfm_auto_identifiers'}", "{P.Str'A B'}, {}, {'gfm_auto_identifiers', 'ascii_identifiers'}",
                 "", "'some text'", "P.Str'x'", "{P.Str'x'}, {x = true, ['x-1'] = true}", "{P.Str'x'}, {x = false}",
                 "{P.Str'x'}, {'x'}", "{P.Str'x'}, nil, {'nonsense'}", "{P.Str'x'}, 5", "{P.Str'3. Čas'}, {}, {}",
                 "{P.Code('c d'), P.Math('InlineMath', 'x')}", "{P.Note{P.Para'n'}, P.Str'x'}"]
] + [
    ("structure names", "local n = {} for k in pairs(P.structure) do n[#n + 1] = k end table.sort(n) return table.concat(n, ' ')"),
]

RANDOM = random.Random(20261112)

GROUPS["state-fields"] = [
    (f"PANDOC_STATE.{key}", f"return PANDOC_STATE.{key}")
    for key in ["output_file", "source_url", "user_data_dir", "trace", "verbosity", "log",
                "resource_path", "request_headers", "nokey", "Output_file", "input"]
] + [
    ("state key 1", "return PANDOC_STATE[1]"),
    ("state key true", "return PANDOC_STATE[true]"),
    ("state output test", "if PANDOC_STATE.output_file then return 'file' end return 'stdout'"),
]

GROUPS["json-empty-lists"] = [
    (f"encode {value}", f"return P.json.encode({value})")
    for value in ["P.List{}", "P.Inlines{}", "P.Blocks{}", "{}", "setmetatable({}, {})",
                  "setmetatable({}, {__name = 'List'})", "setmetatable({}, P.List)",
                  "{a = P.List{}}", "P.List{P.List{}}", "{P.List{}, {}}", "P.Attr().classes",
                  "P.Attr().attributes", "P.AttributeList{}", "P.MetaList{}", "P.Meta{}",
                  "P.Div{}.content", "P.Para{}.content", "P.Cell{}.contents", "P.Table(P.Caption(), {}, P.TableHead(), {}, P.TableFoot()).bodies",
                  "P.json.decode('[]')", "P.json.decode('{\"a\":[]}')", "P.json.decode('[[],{}]')",
                  "P.json.decode('{\"a\":{}}')", "P.json.decode('{\"a\":[[]]}')", "P.Pandoc{}",
                  "P.List{1}:filter(function() return false end)", "P.List{1}:map(function(x) return x end)",
                  "P.Inlines{}:clone()", "pandoc.List{}:clone()"]
]

NUMBERS = [2.0 ** 54 + 8, -(2.0 ** 54 + 8), 2.0 ** 55, 2.0 ** 53 + 2, 2.0 ** 60 + 2 ** 8, 1e22, 1e23,
           1e21 + 2 ** 20, 0.1, 0.3, 1 / 3, 123.456, 5e-324, 2.2250738585072014e-308, 1.7976931348623157e308,
           9007199254740993.0, 4.35, 1e7, 1e8, 12345678.9, 0.000123, 1e-7, 100.0, -0.5]
for _ in range(220):
    kind = RANDOM.randrange(4)
    if kind == 0:
        NUMBERS.append(float(RANDOM.randrange(2 ** 53, 2 ** 62)))
    elif kind == 1:
        NUMBERS.append(RANDOM.uniform(-1e6, 1e6))
    elif kind == 2:
        NUMBERS.append(RANDOM.uniform(1, 10) * 10.0 ** RANDOM.randrange(-30, 30))
    else:
        NUMBERS.append(float(RANDOM.randrange(1, 2 ** 20)) * 2.0 ** RANDOM.randrange(30, 70))
GROUPS["json-numbers"] = [
    (f"encode {number!r}", f"return P.json.encode({number.hex()})") for number in NUMBERS
] + [
    (f"encode integer {value}", f"return P.json.encode({value})")
    for value in [18014398509481992, -18014398509481992, 2 ** 62, 2 ** 63 - 1, -(2 ** 63), 12345, 0]
] + [
    ("encode list of numbers", "return P.json.encode({0x1.0000000000002p+54, 1/3, 1e23})"),
]

ENCODINGS = ["UTF-16", "UTF-16LE", "UTF-16BE", "UTF-32", "UTF-32LE", "UTF-32BE", "UTF-8", "UTF-8BOM",
             "utf16", "UTF16LE//IGNORE"]
BOM_INPUTS = ["'\\255\\254a\\0'", "'\\254\\255\\0a'", "'\\255\\254\\0\\0a\\0\\0\\0'", "'\\0\\0\\254\\255\\0\\0\\0a'",
              "'\\239\\187\\191a'", "'a\\0'", "'\\0a'", "'\\255\\254'", "'\\254\\255'", "''",
              "'\\255\\254\\255\\254a\\0'", "'\\254\\255\\254\\255\\0a'", "'\\255\\254\\0'"]
GROUPS["text-bom"] = [
    (f"fromencoding {text} {name}",
     f"local s = P.text.fromencoding({text}, {lua_string(name)}) return (s:gsub('.', function(c) return c:byte() .. ' ' end))")
    for name in ENCODINGS for text in BOM_INPUTS
] + [
    (f"toencoding {name}",
     f"return (P.text.toencoding('a\\226\\128\\147', {lua_string(name)}):gsub('.', function(c) return c:byte() .. ' ' end))")
    for name in ENCODINGS
]


def slide_blocks(depth):
    blocks = []
    for _ in range(RANDOM.randrange(0, 6)):
        kind = RANDOM.randrange(10)
        if kind < 5:
            blocks.append(f"H({RANDOM.randrange(1, 7)}, 'h')")
        elif kind < 7:
            blocks.append("P.Para'x'")
        elif kind == 7:
            blocks.append("P.HorizontalRule()")
        elif depth < 2:
            blocks.append("P.Div{" + ", ".join(slide_blocks(depth + 1)) + "}")
        else:
            blocks.append("P.BlockQuote{H(1, 'q'), P.Para'y'}")
    return blocks


SLIDE_LISTS = ["H(2, 'a'), P.Div{H(1, 'b'), P.Para'c'}", "H(2, 'a'), P.Para'x', P.Div{H(1, 'b'), P.Para'c'}",
               "P.Div{H(1, 'b'), P.Para'c'}", "H(2, 'a'), P.HorizontalRule(), P.Para'x'", "H(3, 'a'), H(2, 'b'), P.Para'x'",
               "H(1, 'a'), P.Div{}, H(2, 'b'), P.Para'x'", "P.Div{H(3, 'a')}, P.Para'x'", "H(2, 'a'), P.Div{P.Div{H(1, 'b'), P.Para'c'}}",
               "H(4, 'a'), P.Para'x', H(4, 'b'), P.Div{H(2, 'c'), P.Para'y'}", "H(1, 'a')", "P.Para'x', H(1, 'a')",
               "P.Div{H(2, 'a'), P.Para'x'}, H(3, 'b'), P.Div{H(1, 'c'), P.Para'y'}",
               "P.Div{H(3, 'a'), P.Para'x'}, H(2, 'b'), P.Div{H(1, 'c'), P.Para'y'}",
               "P.Div{H(2, 'a'), P.Para'x'}, H(2, 'b'), P.Div{H(1, 'c'), P.Para'y'}",
               "P.Div{P.Div{H(2, 'a'), P.Para'x'}, H(3, 'b'), P.Div{H(1, 'c'), P.Para'y'}}",
               "P.Div{H(4, 'a'), P.Para'x'}, P.Div{H(5, 'b'), P.Div{H(1, 'c'), P.Para'y'}}",
               "H(3, 'a'), P.BlockQuote{H(1, 'q'), P.Para'y'}, H(2, 'b'), P.Para'x'"]
SLIDE_LISTS += [", ".join(slide_blocks(0)) for _ in range(160)]
GROUPS["slide-level"] = [
    (f"slide_level {{{blocks}}}", f"return P.structure.slide_level{{{blocks}}}") for blocks in SLIDE_LISTS
]

GROUPS["from-path-uri"] = [
    (f"from_path {path!r}", f"return P.format.from_path({lua_string(path)})")
    for path in ["file:/a.md#x", "file:///a.md#x", "file:a.md?q", "https://h/é.md#x", "https://h/a.md#x",
                 "https://h/a.md?x=1#y", "http://h:80/a.tex", "http://h:x/a.tex", "http://[::1]/a.rst",
                 "http://u@h/a.rst?z", "mailto:a.md", "urn:x:a.md", "x:a.md#f", "1x:a.md#f", "a b:c.md#f",
                 "https://h/a%20b.md#x", "https://h/a%zz.md#x", "https://h/a b.md#x", "https://h/a.md#x#y",
                 "https://h/a.md#é", "https://h/a.md?é", "https://h/a{b}.md#x", "https://h/a|b.md#x",
                 "https://h/a.md#x?y", "https://h/a.md?", "https://h/a.md#", "https:/a.md#x", "https:a.md#x",
                 "c:/a.md#x", "C:\\a.md#x", "-x:a.md#f", "x+y.z-w:a.md#f", "x_y:a.md#f", "://a.md#x",
                 ":a.md#x", "https://h/dir.md/#x", "https://h#x.md", "https://h?x.md", "https://h/.md#x",
                 "https://h/a.MD#x", "https://user:pw@h:8080/p/a.html?x=1&y=2#frag", "https://[v1.x]/a.md#f",
                 "https://[zz]/a.md#f", "https://h/a.md#[x]", "https://h/a[x].md#f", "data:text/plain,a.md#x",
                 "https://h/a.md%23x", "https://h/~a/!$&'()*+,;=:@.md#x", "http://h:x/a.tex#f",
                 "http://h:80x/a.tex#f", "http://u[@h/a.tex#f", "http://u:p:q@h/a.tex#f", "http://h%41/a.tex#f",
                 "http://1.2.3.4/a.tex#f", "http://[1:2::3]/a.tex#f", "http://[1:2::3]:9/a.tex#f",
                 "http://[1:2::3]x/a.tex#f", "http://[::ffff:1.2.3.4]/a.tex#f", "http://[1:2:3]/a.tex#f",
                 "https://h/", "https://h", "https://h/#x", "https://h?q", "mailto:", "x:#f", "file://h",
                 "file:///", "x:", "x:/", "x:?q.md", "https://h/a.md/..", "https://h/a/./b.rst#x", "x://a.md",
                 "http://h/a.tex%", "http://h/a.tex%4", "HTTP://H/A.TEX#F", "http://h/a.tex#f%20"]
]

GROUPS["changed-chains"] = [
    (f"insert chain {n}", "local e = P.Plain{P.Str'x'} for i = 1, " + str(n)
     + " do local d = P.Div{} d.content:insert(e) e = d end return #tostring(e), #P.json.encode(e)")
    for n in [1, 2, 24, 25, 30, 64, 100, 126]
] + [
    (f"list chain {n}", "local l = P.BulletList{} local cur = l for i = 1, " + str(n)
     + " do local n = P.BulletList{} cur.content = {{n}} cur = n end return #tostring(l), #P.json.encode(l)")
    for n in [1, 24, 25, 30, 60]
] + [
    (f"span chain {n}", "local e = P.Str'x' for i = 1, " + str(n)
     + " do local s = P.Span{} s.content = {e, P.Space()} s.attr.classes:insert('c' .. i) e = s end "
     "local p = P.Para{e} return #tostring(p), #P.utils.stringify(p)")
    for n in [10, 40, 100]
] + [
    ("shared child", "local c = P.Div{} c.content:insert(P.Para'x') local d = P.Div{} d.content = {c, c, c} "
     "local e = P.Div{} e.content = {d, d} return #tostring(e), P.json.encode(e)"),
    ("changed after read", "local c = P.Div{} c.content:insert(P.Para'x') local d = P.Div{c} local s1 = tostring(d) "
     "c.content:insert(P.Para'y') d.content = {c} return s1 == tostring(d), #tostring(d)"),
]

RAW_FILTERS = {
    "raw-bom": "﻿function Pandoc(d) d.meta.r = 'ok' return d end\n",
    "raw-bom-shebang": "﻿#!/usr/bin/env pandoc lua\nfunction Pandoc(d) d.meta.r = 'ok' return d end\n",
    "raw-bom-twice": "﻿﻿function Pandoc(d) return d end\n",
    "raw-lookup-order": "local log = {}\nlocal f = setmetatable({}, {__index = function(_, k) log[#log + 1] = k return nil end})\n"
        "return {f, {Pandoc = function(d) d.meta.r = table.concat(log, '|') return d end}}\n",
    "raw-lookup-order-topdown": "local log = {}\nlocal f = setmetatable({traverse = 'topdown'}, {__index = function(_, k) log[#log + 1] = k return nil end})\n"
        "return {f, {Pandoc = function(d) d.meta.r = table.concat(log, '|') return d end}}\n",
    "raw-lookup-order-walk": "function Pandoc(d) local log = {}\n"
        "d:walk(setmetatable({}, {__index = function(_, k) log[#log + 1] = k return nil end}))\n"
        "d.meta.r = table.concat(log, '|') return d end\n",
}

PRELUDES = {
    "structure": r'''
local function doc()
  return {P.Para'intro', H(1,'a', P.Attr('ida')), P.Para'pa', H(2,'b'), H(1,'c', P.Attr('', {'unnumbered'})), H(2, 'd')}
end
''',
    "utils-blocks-to-inlines": BLOCKS,
    "utils-sections": r"""
local function doc()
  return {P.Para{t'intro'}, H(1,'a', P.Attr('ida', {'c1'}, {k='v'})), P.Para{t'pa'},
    H(2,'b'), P.Para{t'pb'}, H(3,'c'), H(2,'d'), H(1,'e'), P.Para{t'pe'}}
end
""",
}


def filter_source(name, probes):
    lines = [HELPER, PRELUDES.get(name, ""), "local function run(document)"]
    for label, body in probes:
        lines.append(f"  probe({lua_string(label)}, function() {body} end)")
    lines.append("end")
    lines.append(FOOTER)
    return "\n".join(lines)


def main():
    parts = [
        "# The oracle matrix of the functions of the `pandoc` Lua module.\n",
        "# `tools/module-matrix.py` writes this file. Do not edit it.\n",
    ]
    for name, text in DOCS.items():
        parts.append(f"@@@ doc {name}\n{text}\n")
    for name, probes in GROUPS.items():
        parts.append(f"@@@ filter {name} empty small\n{filter_source(name, probes)}\n")
    for name, source in RAW_FILTERS.items():
        parts.append(f"@@@ filter {name} empty small\n{source}")
    with open(OUTPUT, "w", encoding="utf-8") as handle:
        handle.write("".join(parts))
    print(f"{len(GROUPS)} filters, {sum(len(p) for p in GROUPS.values())} probes")


if __name__ == "__main__":
    main()
