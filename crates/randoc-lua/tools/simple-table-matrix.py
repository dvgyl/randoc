#!/usr/bin/env python3
"""Write the SimpleTable replay matrix."""

from pathlib import Path
from runpy import run_path

matrix = run_path(str(Path(__file__).with_name("wave8b_matrix.py")))
calls = matrix["calls"]

matrix["write_matrix"]("simple-table", {
    "constructor": (r"""local S=P.SimpleTable
local st=S('a caption',{'AlignLeft','AlignRight'},{.5,0},{{P.Plain'h1'},{P.Plain'h2'}},{{{P.Plain'x'},{P.Plain'y'}}})""", [
        *calls("S", ["", "nil", "{}", "123", "'cap'", "'cap',{}", "'cap',{},{}",
                      "'cap',{},{},{}", "'cap',{}, {},{},{}", "'cap',{'Bogus'}, {},{},{}",
                      "'cap',{}, {'bad'}, {},{}", "'cap',{}, {},false,{}",
                      "'cap',{}, {},{},false", "'cap',{}, {},{false},{}",
                      "'cap',{}, {},{},{false}"]),
        ("fields", "return st.t, keys(st), P.utils.type(st),getmetatable(st), st.unknown"),
        ("field types", "return P.utils.type(st.caption), P.utils.type(st.aligns), P.utils.type(st.widths), P.utils.type(st.headers), P.utils.type(st.headers[1]), P.utils.type(st.rows), P.utils.type(st.rows[1]), P.utils.type(st.rows[1][1])"),
        ("record", "return tostring(st)"),
        ("equal", "return S('x',{},{},{},{}) == S('x',{},{},{},{})"),
        ("not equal", "return st == S('',{},{},{},{}),st == P.Str'a'"),
        ("write cached property", "local old = st.aligns; st.aligns[1]='AlignCenter'; return st.aligns[1],rawequal(old,st.aligns)"),
        ("write caption", "st.caption='new'; return st.caption, tostring(st)"),
        ("reset caption", "st.caption=nil; return st.caption"),
        ("write tag", "st.t='Table'"),
        ("write unknown", "st.extra=1"),
    ]),
    "conversion": (r"""local S,U=P.SimpleTable,P.utils
local h=P.TableHead{P.Row{P.Cell({P.Plain'h'},'AlignDefault',1,2)}}
local body={attr=P.Attr(),row_head_columns=0,
  head={P.Row{P.Cell'h1',P.Cell'h2'}},
  body={P.Row{P.Cell({P.Plain'a'},'AlignDefault',2,1),P.Cell'b'},P.Row{P.Cell'c'}}}
local foot=P.TableFoot{P.Row{P.Cell'd',P.Cell'e'}}
local tbl=P.Table({long={P.Plain'Cap',P.Plain'Second'}},
  {{'AlignLeft',0},{'AlignRight',0.5}},h,{body},foot)""", [
        *calls("U.to_simple_table", ["", "nil", "{}", "P.Para'x'", "tbl"]),
        *calls("U.from_simple_table", ["", "nil", "{}", "tbl", "S('',{},{},{},{})",
                                             "S('cap',{'AlignLeft'},{-1},{},{})"]),
        ("complex table", "local st=U.to_simple_table(tbl); return st.caption,st.headers,st.rows"),
        ("with unequal widths", "return U.from_simple_table(S('cap',{'AlignLeft','AlignRight'},{.5},{},{}))"),
        ("round trip", "local t=U.from_simple_table(S('x',{'AlignLeft'},{0},{{P.Plain'h'}},{{{P.Plain'b'}}})); return t==U.from_simple_table(U.to_simple_table(t))"),
    ]),
})
