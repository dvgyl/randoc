#!/usr/bin/env python3
"""Write the media bag replay matrix."""

from pathlib import Path
from runpy import run_path

matrix = run_path(str(Path(__file__).with_name("wave8b_matrix.py")))
calls = matrix["calls"]

matrix["write_matrix"]("mediabag", {
    "entries": ("local M = P.mediabag", [
        ("insert and lookup", "M.insert('z.png', nil, 'a\\0b'); return M.lookup('z.png')"),
        ("numeric arguments", "M.insert(12, 3, 4); return M.lookup(12)"),
        ("sorted list", "return M.list()"),
        ("iterator", "local f,s,i = M.items(); return type(f), f(s,i), f(s,i)"),
        ("snapshot", "local f,s,i = M.items(); M.delete('12'); return f(s,i)"),
        ("empty", "M.empty(); return #M.list(), M.lookup('z.png')"),
        *calls("M.insert", ["", "'x'", "'x', nil", "'x', {}, ''", "false, nil, ''", "'x', nil, {}"]),
        *calls("M.lookup", ["", "nil", "{}"]),
        *calls("M.delete", ["", "true", "'missing'"]),
    ]),
    "mime": ("local M = P.mediabag", [
        (ext, f"M.insert('file.{ext}', nil, ''); return M.lookup('file.{ext}')")
        for ext in ("md", "png", "gif", "jpeg", "svg", "svgz", "pdf", "eps", "webp", "tiff",
                    "bmp", "mp3", "mp4", "wav", "css", "js", "json", "csv", "woff2", "zip",
                    "PNG", "unknown")
    ]),
    "data": ("local M = P.mediabag", [
        *calls("M.make_data_uri", ["", "'text/plain'", "nil, 'x'", "'text/plain', {}",
                                   "'text/plain', 'a b%&é\\n'", "'text/plain;x=1', '\\0'",
                                   "'TEXT/plain', 'x'", "'', ''", "'application/json', 4"]),
        *calls("M.fetch", ["", "nil", "{}", "'data:,a+b'", "'data:image/png;base64,YWI='",
                           "'wave8b-missing.png'"]),
        *calls("M.fill", ["", "P.Plain('x')", "document"]),
        ("fill images", "return M.fill(P.Pandoc{P.Para{P.Image('a','absent.png','title',P.Attr('id',{'existing'},{k='v'})), P.Image('b','data:image/png;base64,YWI=')}}).blocks, M.list()"),
    ]),
    "files": ("local M = P.mediabag", [
        ("fetch file", "return with_files(function(dir) local f=dir..'/a.txt'; write(f,'abc'); return M.fetch(f) end)"),
        ("loaded message", """return with_files(function(dir)
          local path=dir..'/loaded.txt'; write(path,'data')
          local logs=P.log.silence(function() M.fetch(path) end)
          return P.json.encode(logs[1]):gsub(dir:gsub('(%W)','%%%1'),'DIR')
        end)"""),
        ("write files", """return with_files(function(dir)
          M.insert('sub/a.txt', nil, 'abc'); M.insert('../unsafe.txt', nil, 'a');
          M.write(dir)
          local f=assert(io.open(dir..'/sub/a.txt','rb')); local data=f:read('*a'); f:close()
          local safe=P.utils.sha1('a')..'.txt'
          f=assert(io.open(dir..'/'..safe,'rb')); local hashed=f:read('*a'); f:close()
          return data, hashed
        end)"""),
        ("extract message", """return with_files(function(dir)
          local logs=P.log.silence(function() M.write(dir,'sub/a.txt') end)
          return P.json.encode(logs[1]):gsub(dir:gsub('(%W)','%%%1'),'DIR')
        end)"""),
        *calls("M.write", ["", "{}", "'.', {}", "'.', 'absent'"]),
    ]),
})
