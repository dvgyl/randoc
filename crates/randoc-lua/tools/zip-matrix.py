#!/usr/bin/env python3
"""Write the ZIP replay matrix."""

from pathlib import Path
from runpy import run_path

matrix = run_path(str(Path(__file__).with_name("wave8b_matrix.py")))
calls = matrix["calls"]

matrix["write_matrix"]("zip", {
    "entry": ("local Z = P.zip; local e=Z.Entry('a.txt','abc',1700000001)", [
        *calls("Z.Entry", ["", "'a'", "'a',{}", "{},'b'", "'a','b',1.5", "'a','b','x'",
                          "12,34,0"]),
        ("entry fields", "return P.utils.type(e), tostring(e), getmetatable(e), keys(e),e.path,e.modtime,e.unknown"),
        ("entry operations", "return e:contents(), e:contents('pw'), e:symlink()"),
        ("password type", "return e:contents({})"),
        ("change entry", "e.path='b'; e.modtime=9; return e.path,e.modtime"),
        ("restore entry", "e.path=nil; e.modtime=nil; return e.path,e.modtime"),
        ("unknown entry field", "e.contents='x'"),
        ("default timestamp", "return math.abs(Z.Entry('a','').modtime-os.time()) <= 2"),
    ]),
    "archive": ("local Z=P.zip; local e=Z.Entry('a','abc',1700000001); local a=Z.Archive{e}", [
        *calls("Z.Archive", ["false", "{1}", "123", "'bad'", "'not a zip'"]),
        ("archive fields", "return P.utils.type(a), tostring(a), getmetatable(a), keys(a), P.utils.type(a.entries),a.unknown"),
        ("empty archive", "return #Z.Archive():bytestring(), #Z.Archive(nil).entries"),
        ("zip round trip", "local entry=Z.Archive(a:bytestring()).entries[1]; return entry.path,entry.modtime,entry:contents()"),
        ("zip time floor", "local entry=Z.Archive(Z.Archive{Z.Entry('a','a',0)}:bytestring()).entries[1]; return entry.modtime"),
        ("stored or compressed", "local bytes=string.rep('a',10000); local archive=Z.Archive{Z.Entry('large',bytes,1700000000)}; local zip=archive:bytestring(); return #zip<10000,Z.Archive(zip).entries[1]:contents()==bytes"),
        ("archive snapshot", "e.path='x'; return a.entries[1].path,rawequal(a.entries,a.entries)"),
        ("insert", "a.entries:insert(Z.Entry('b','b',0)); return #a.entries,Z.Archive(a:bytestring()).entries[2].path"),
        ("assign entries", "a.entries={e}; return a.entries[1].path,P.utils.type(a.entries)"),
        ("restore entries", "a.entries=nil; return #a.entries,P.utils.type(a.entries),a.entries[1].path"),
        ("unknown archive field", "a.contents='x'"),
        ("extract options", "return a:extract(false)"),
    ]),
    "files": ("local Z=P.zip", [
        *calls("Z.read_entry", ["", "{}"]),
        *calls("Z.zip", ["", "'x'", "{false}", "{},false", "{}"]),
        ("read entry", """return with_files(function(dir)
          local path=dir..'/f';write(path,'zip')
          local e=Z.read_entry(path)
          return e.path:sub(1,2),e.path:sub(-2),e:contents()
        end)"""),
        ("read location", """return with_files(function(dir)
          local path=dir..'/f';write(path,'zip')
          return Z.read_entry(path,{location='pre'}).path:sub(1,2)

        end)"""),
        ("zip recursive", """return with_files(function(dir)
          write(dir..'/f','fixture')
          local a=Z.zip({dir},{recursive=true})
          local entries=a.entries:filter(function(e) return not e.path:match('/%._[^/]+$') end)
          return #entries,entries[1].path:sub(-1),entries[2].path:sub(-2),entries[2]:contents()
        end)"""),
        ("zip directory only", """return with_files(function(dir)
          write(dir..'/f','fixture')
          local a=Z.zip{dir}
          return #a.entries,a.entries[1].path:sub(-1)
        end)"""),
        ("symbolic links", """return with_files(function(dir)
          write(dir..'/file','target')
          local link=dir..'/link'
          assert(os.execute('ln -s file '..string.format('%q',link)))
          local a=Z.zip({link},{preserve_symlinks=false}).entries[1]
          local b=Z.zip({link},{preserve_symlinks=true}).entries[1]
          return a:contents(),a:symlink(),b:contents(),b:symlink()
        end)"""),
        ("extract", """return with_files(function(dir)
          local a=Z.Archive{Z.Entry('sub/','',0), Z.Entry('sub/file','abc',0)}
          a:extract({destination=dir})
          local f=assert(io.open(dir..'/sub/file','rb')); local content=f:read('*a'); f:close()
          return content
        end)"""),
    ]),
}, known=[("files", "symbolic links")])
