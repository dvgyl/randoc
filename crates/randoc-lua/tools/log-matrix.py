#!/usr/bin/env python3
"""Write the log replay matrix."""

from pathlib import Path
from runpy import run_path

matrix = run_path(str(Path(__file__).with_name("wave8b_matrix.py")))
calls = matrix["calls"]

matrix["write_matrix"]("log", {
    "messages": ("local L = P.log", [
        *calls("L.info", ["", "{}", "nil", "42", "'info'"]),
        *calls("L.warn", ["", "{}", "nil", "42", "'warning'"]),
        ("live state", """local old=#PANDOC_STATE.log; L.warn('first'); L.info('second')
          local log=PANDOC_STATE.log
          return #log-old, P.utils.type(log), log[old+1].message, log[old+2].message"""),
        ("json", "local log = L.silence(function() L.info('info'); L.warn('warning') end); return P.json.encode(log[1]), P.json.encode(log[2])"),
        ("typed message", """local log=L.silence(function() L.info('hidden') end)
          local m=log[1]
          return P.utils.type(m), tostring(m), getmetatable(m), keys(m), m[0], m.unknown"""),
        ("assign", "local log=L.silence(function() L.warn('hidden') end); log[1].message='x'"),
        ("length", "local log=L.silence(function() L.warn('hidden') end); return #log[1]"),
    ]),
    "silence": ("local L = P.log", [
        *calls("L.silence", ["", "nil", "function() end"]),
        ("arguments and results", """local msgs,a,b,c=L.silence(function(x) L.warn(x); return false,nil,x end,'x')
          return #msgs,a,b,c"""),
        ("nested", """local old=#PANDOC_STATE.log
          local outer,inner=L.silence(function()
            L.warn('before')
            local messages=L.silence(function() warn('inside') end)
            L.info('after')
            return messages
          end)
          return outer[1].message,outer[2].message,inner[1].message,#PANDOC_STATE.log-old"""),
        ("error identity", "local sentinel={}; local ok,v=pcall(L.silence,function() error(sentinel) end); return ok, rawequal(v,sentinel)"),
        ("hold ends on error", "pcall(L.silence,function() L.warn('hidden'); error('abort') end); local old=#PANDOC_STATE.log; L.warn('visible'); return #PANDOC_STATE.log-old"),
    ]),
}, known=[("silence", "error identity")])
