#!/usr/bin/env python3
"""Write the nested filter and reference replay matrix."""

from pathlib import Path
from runpy import run_path

matrix = run_path(str(Path(__file__).with_name("wave8b_matrix.py")))
calls = matrix["calls"]

matrix["write_matrix"]("run-filter", {
    "lua": (r"""local U=P.utils
local function run_filter(source, env)
  return with_files(function(dir)
    local path=dir..'/filter.lua'
    write(path,source)
    return U.run_lua_filter(document,path,env)
  end)
end""", [
        *calls("U.run_lua_filter", ["", "{}", "document", "document, {}",
                                         "document, 'missing.lua', true", "document, 'wave8b-missing.lua'"]),
        ("return list", """local d=run_filter("return {{Pandoc=function(d) d.blocks:insert(pandoc.Para'one');return d end},{Str=function(s) s.text=s.text:upper();return s end}}"); return d.blocks,document.blocks"""),
        ("global functions", """return run_filter("function Pandoc(d) d.blocks:insert(pandoc.Para(FORMAT..':'..PANDOC_SCRIPT_FILE)); return d end").blocks"""),
        ("explicit environment", """local env={P=P,token='local'}
          local out=run_filter("return {Pandoc=function(d) d.blocks:insert(P.Para(token)); return d end}",env)
          return out.blocks,env.pandoc,env._G"""),
        ("environment writes", """local env={token='before'}
          run_filter("token = 'after'; return {}",env)
          return env.token"""),
        ("no global fallback", """return run_filter("return {Pandoc=function(d) if tostring then d.blocks={} end; return d end}",{}).blocks"""),
        ("bad result", "return run_filter('return 42')"),
        ("runtime error", """local ok,err=pcall(run_filter,"error('nested failure')")
          local text=tostring(err)
          return ok,text:find('nested failure',1,true)~=nil,text:find('stack traceback',1,true)~=nil"""),
    ]),
    "json": ("local U=P.utils", [
        *calls("U.run_json_filter", ["", "{}", "document", "document,{}", "document,'cat',false",
                                          "document,'cat',{false}", "document,'cat',{}"]),
        ("nil FORMAT", "local saved=FORMAT; FORMAT=nil; local ok,err=pcall(U.run_json_filter,document,'cat'); FORMAT=saved; return ok,tostring(err)"),
        ("default FORMAT", "local ok,err=pcall(U.run_json_filter,document,'echo'); return ok,tostring(err):find(FORMAT,1,true)~=nil"),
        ("stringified number arguments", "local ok,err=pcall(U.run_json_filter,document,'echo',{'tag',123}); return ok,tostring(err):find('tag 123',1,true)~=nil"),
    ]),
    "references": (r"""local U=P.utils
local ref={id='a', type='book', volume='12', title=P.Inlines{P.Str'A',P.Space(),P.Emph'Title'},
  author={{family='Dijk',given='Jan',['dropping-particle']='de',['non-dropping-particle']='van',
    suffix='Sr.',['comma-suffix']=true,['static-ordering']=true},{literal='Team'}},
  issued={literal='Spring',circa=true,season='2'},
  accessed={['date-parts']={{'2000','1'},{'2002','3'}},circa=true},
  ['custom-field']='custom'}
local refs={ref,{id='b',type='article-journal',title='Other'},{title='No id'}}
local reference_doc=P.Pandoc({P.Para{P.Cite('a',{P.Citation('a','NormalCitation')})}},
  {references=refs})
local uncited=P.Pandoc({}, {references=refs})
local all=P.Pandoc({}, {references=refs,nocite=P.Inlines{P.Cite('@*',{P.Citation('*','NormalCitation')})}})""", [
        *calls("U.references", ["", "nil", "{}", "document", "reference_doc", "uncited", "all"]),
        ("shape", "local r=U.references(reference_doc)[1]; return type(r.id),type(r.type),P.utils.type(r.volume),P.utils.type(r.author),P.utils.type(r.issued['date-parts']),P.utils.type(r.accessed['date-parts'][1])"),
        ("CSL JSON bibliography", """return with_files(function(dir)
          local path=dir..'/references.json'
          write(path,'[{"id":"a","type":"book","title":"A Book","volume":12,"URL":"https://example.com"}]')
          return U.references(P.Pandoc(reference_doc.blocks,{bibliography=path}))
        end)"""),
        ("missing bibliography", "return U.references(P.Pandoc(reference_doc.blocks,{bibliography='wave8b-missing.bib'}))"),
    ]),
})
