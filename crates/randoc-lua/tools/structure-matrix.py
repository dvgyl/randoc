#!/usr/bin/env python3
"""Write probes for pandoc.structure."""

import os
import subprocess
from pathlib import Path
from runpy import run_path

TOOLS = Path(__file__).resolve().parent
REPLAY = TOOLS.parent / "tests" / "replay"
matrix = run_path(str(TOOLS / "wave8b_matrix.py"))
calls = matrix["calls"]

matrix["write_matrix"]("structure", {
    "sections": (r"""local S=P.structure
local blocks={P.Para'intro', H(1,'First'), P.HorizontalRule(), H(3,'Leaf'), P.HorizontalRule()}""", [
        *calls("S.make_sections", ["", "nil", "false", "'intro'", "blocks", "P.Pandoc(blocks)",
                                   "blocks,false", "blocks,'opts'", "blocks,{number_sections=true}",
                                   "blocks,{base_level=2}", "blocks,{base_level='2'}",
                                   "blocks,{base_level=1.5}", "blocks,{slide_level='2'}",
                                   "blocks,{slide_level=1.5}", "blocks,{slide_level=0}",
                                   "blocks,{slide_level=-1}", "blocks,{slide_level=2}",
                                   "blocks,{slide_level=2,base_level=2,number_sections=true}",
                                   "{},{slide_level=2}", "{P.Para'intro'},{slide_level=3,number_sections=true}",
                                   "{P.Para'intro',H(1,'First')},{slide_level=3,number_sections=true}",
                                   "{H(3,'Leaf')},{slide_level=2}", "{P.HorizontalRule()},{slide_level=2}",
                                   "{P.Div{H(1,'Inside')}},{slide_level=2}"]),
        *calls("S.slide_level", ["blocks", "P.Pandoc(blocks)", "{}", "false"]),
        *calls("S.unique_identifier", ["'Some Heading'", "'Some Heading',{'some-heading'}"]),
    ]),
    "split": (r"""local S=P.structure
local doc=P.Pandoc({
  P.Para{P.Link('inside','#leaf'),P.Space(),P.Link('missing','#absent')},
  H(1,'First',P.Attr('first',{'unlisted'})), H(2,'Child'),
  H(3,'Leaf'), H(2,'Child'), H(1,'Second'), H(3,'Deep'),
})
local function summary(cd)
  local result={}
  for i,c in ipairs(cd.chunks) do
    result[i]={c.id,c.level,c.number,c.path,c.unlisted,c.section_number,
      up=c.up and c.up.id,heading=c.heading,contents=c.contents}
  end
  return result
end
local function split(opts) return summary(S.split_into_chunks(doc,opts)) end""", [
        *calls("S.split_into_chunks", ["", "nil", "{}", "doc,false", "doc,'opts'",
            "doc,{chunk_level='bad'}", "doc,{chunk_level=1.5}",
            "doc,{base_heading_level='bad'}", "doc,{path_template=false}"]),
        *calls("split", ["", "{chunk_level=0}", "{chunk_level=-2}",
                         "{chunk_level=2,number_sections=true,path_template='%n-%s-%i-%h-%%-%x-%'}",
                         "{chunk_level=3,number_sections=0}", "{base_heading_level=2}",
                         "{base_heading_level=2,chunk_level=2}", "{path_template=123}"]),
        ("empty", "return summary(S.split_into_chunks(P.Pandoc{}))"),
        ("empty with level zero", "return summary(S.split_into_chunks(P.Pandoc{},{chunk_level=0}))"),
        ("intro title metadata", "return summary(S.split_into_chunks(P.Pandoc({P.Para'plain'},{title='Probe'})))"),
        ("no headings", "return summary(S.split_into_chunks(P.Pandoc{P.Para'plain'}))"),
        ("explicit identifiers", "return summary(S.split_into_chunks(P.Pandoc{H(1,'Same',P.Attr('same')),H(1,'Same'),H(1,'Same')}))"),
    ]),
    "userdata": (r"""local S=P.structure
local cd=S.split_into_chunks(P.Pandoc(
  {H(1,'One'),H(2,'Two'),H(3,'Three'),H(2,'Four'),H(1,'Five')},{title='Probe'}),
  {chunk_level=3,number_sections=true})
local a,b,c,d,e=table.unpack(cd.chunks)
local function tree(t)
  local e=t[0]
  local result={e.id,e.level,e.path,e.number,title=e.title}
  for i,child in ipairs(t) do result['child'..i]=tree(child) end
  return result
end""", [
        ("types", "return P.utils.type(cd),P.utils.type(a),P.utils.type(cd.meta),P.utils.type(cd.toc),getmetatable(cd.chunks),getmetatable(cd.toc),P.utils.type(a.heading),P.utils.type(a.contents)"),
        ("pairs", "return keys(cd),keys(a),keys(c)"),
        ("unknown keys", "return cd.unknown,a.unknown,cd.t,a.t"),
        ("chunked document string", "return tostring(cd)"),
        ("chunk record string", "return tostring(a)"),
        ("toc", "return tree(cd.toc)"),
        ("toc shape", "local t=cd.toc; return t[0].id,t[0].path,#t,t[1][0].title,t[1][1][1][0].path,getmetatable(t[1][0])"),
        ("first links", "return a.up,a.prev,a.next.id,a.next.up.id,a.next.prev.id,a.next.next"),
        ("nested links", "return c.up.id,c.up.up.id,c.up.prev,c.up.next,c.prev.id,c.prev.up.id,c.prev.prev,c.prev.next,c.next.id,c.next.prev.id,c.next.prev.prev,c.next.next"),
        ("last links", "return e.up,e.next,e.prev.id,d.up"),
        ("link identity", "return a.next==b,a.next==a.next,b.prev==a"),
        ("cached fields", "return cd.chunks==cd.chunks,cd.meta==cd.meta,cd.toc==cd.toc,rawequal(a.heading,a.heading),rawequal(a.contents,a.contents)"),
        ("property writes", "local copy=a.next; copy.id='new'; copy.heading=P.Inlines'title'; copy.contents=P.Blocks'body'; return copy.id,copy.heading,copy.contents,b.id"),
        ("nil property", "a.id=nil; return a.id"),
        ("write link", "a.next=e; return a.next.id"),
        ("write chunked property", "cd.meta={description='changed'}; return cd.meta.description"),
        ("write unknown chunk key", "a.extra=1"),
        ("write unknown doc key", "cd.extra=1"),
    ]),
    "toc": (r"""local S=P.structure
local blocks={H(1,'First',P.Attr('first')),H(2,'Sub',P.Attr('sub')),
  H(3,'Leaf',P.Attr('leaf')),H(4,'Deep',P.Attr('deep')),
  H(1,'Hidden',P.Attr('hidden',{'unlisted'})),H(2,'Child',P.Attr('child')),
  H(1,'Unnumbered',P.Attr('un',{'unlisted','unnumbered'})),H(2,'Child',P.Attr('un-child'))}
local doc=P.Pandoc(blocks)
local sectioned=S.make_sections(blocks,{number_sections=true})
local cd=S.split_into_chunks(doc,{chunk_level=2,number_sections=true,path_template='%i.html'})""", [
        *calls("S.table_of_contents", ["", "nil", "42", "false", "'text'", "{}", "blocks",
            "doc", "cd", "blocks,{toc_depth=0}", "blocks,{toc_depth=-1}",
            "doc,{toc_depth=4}", "doc,{number_sections=true}",
            "doc,P.WriterOptions{toc_depth=1,number_sections=true}",
            "doc,{identifier_prefix='prefix-',toc_depth=1}", "doc,'opts'",
            "doc,{toc_depth='bad'}", "doc,{unknown=1}",
            "cd,{number_sections=true,toc_depth=4}", "sectioned,{toc_depth=1}",
            "{H(1,'No identifier')}", "P.Para'x'", "H(1,'Heading')",
            "{P.Div({P.Para'x'},P.Attr('',{'section'}))}"]),
        ("cached TOC writes", "cd.toc[1][0].title=P.Inlines'new'; return S.table_of_contents(cd,{toc_depth=1})"),
    ]),
}, known=[
    # The Wave 8B contract fixes nonpositive slide levels at one and ignores strings.
    ("sections", "S.make_sections(blocks,{slide_level='2'})"),
    ("sections", "S.make_sections(blocks,{slide_level=0})"),
    ("sections", "S.make_sections(blocks,{slide_level=-1})"),
    # The Wave 8B contract uses the same intro chunk with or without title metadata.
    ("split", "intro title metadata"),
    # Each link read creates a new view. The string displays the userdata address.
    ("userdata", "link identity"),
    ("userdata", "chunk record string"),
    # Row I-LUA-03: the constructor pandoc.WriterOptions is not available yet.
    ("toc", "S.table_of_contents(doc,P.WriterOptions{toc_depth=1,number_sections=true})"),
])


docs, filters = run_path(str(TOOLS / "record-replay.py"))["read_cases"](REPLAY / "structure.txt")
oracle = os.environ["RANDOC_ORACLE_PANDOC"]
records = []
for name, doc_names, source in filters:
    for doc_name in doc_names:
        document = matrix["lua_string"](docs[doc_name])
        script = source + f"\nio.write(P.write(Pandoc(P.read({document}, 'json')), 'json'), '\\n')"
        result = subprocess.run(
            [oracle, "lua", "-e", script], capture_output=True, check=True, timeout=60
        )
        records.append(f"@@@ {name} {doc_name} 0\n" + result.stdout.decode("utf-8"))
(REPLAY / "structure-expected.txt").write_text("".join(records), encoding="utf-8")
