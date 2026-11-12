#!/usr/bin/env python3
"""Writes the oracle matrix of `pandoc.system`, `pandoc.path.exists` and
`pandoc.pipe`.

Usage (from the root of the worktree, after `. env.sh`):

    python3 -I crates/randoc-lua/tools/system-matrix.py
    TMPDIR="$TMPDIR/lua-s4" python3 -I crates/randoc-lua/tools/record-replay.py system

The script writes `crates/randoc-lua/tests/replay/system.txt`. Each filter
makes a temporary directory under `$TMPDIR/lua-s4` with
`with_temporary_directory`, works only inside it with relative paths, and
starts only the programs `echo`, `cat`, `true`, `false` and `sh -c` with
one of them. The directory path in a result is replaced by `TMP`.
"""

import os

HERE = os.path.dirname(os.path.abspath(__file__))
OUTPUT = os.path.join(HERE, "..", "tests", "replay", "system.txt")

DOCS = {
    "empty": '{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}',
    "small": '{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"one"}]}]}',
}

HELPER = r"""
local P, S = pandoc, pandoc.system
local log = {}
local here = ''
local function show(value, depth)
  depth = depth or 0
  local kind = type(value)
  if kind == 'string' then
    local text = here ~= '' and value:gsub(here:gsub('%p', '%%%0'), 'TMP') or value
    return '"' .. text:gsub('[\0-\31\127"\\]', function(c) return '\\' .. c:byte() end) .. '"'
  end
  if kind == 'number' then return math.type(value) .. ':' .. tostring(value) end
  if kind == 'table' and depth < 4 then
    local keys = {}
    for key in pairs(value) do keys[#keys + 1] = key end
    table.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
    local parts = {}
    for _, key in ipairs(keys) do
      parts[#parts + 1] = show(key, depth + 1) .. '=' .. show(value[key], depth + 1)
    end
    local mt = getmetatable(value)
    local name = type(mt) == 'table' and rawget(mt, '__name')
    return (name and (tostring(name) .. ' ') or '') .. '{' .. table.concat(parts, ', ') .. '}'
  end
  return tostring(value)
end
local function probe(label, fn)
  local results = table.pack(pcall(fn))
  local parts = {}
  for i = 2, results.n do parts[#parts + 1] = show(results[i]) end
  -- Pandoc raises `PandocError` objects (row I-LUA-10): the text is compared.
  if not results[1] then parts = {show(tostring(results[2]))} end
  log[#log + 1] = label .. (results[1] and ' => ' or ' !! ') .. table.concat(parts, ' | ')
end
local function sorted(list) table.sort(list) return list end
"""

FOOTER = r"""
function Pandoc(document)
  local parent = (os.getenv('TMPDIR') or '/tmp'):gsub('/$', '') .. '/lua-s4'
  S.make_directory(parent, true)
  S.with_temporary_directory(parent, 'matrix', function(directory)
    S.with_working_directory(directory, function()
      here = S.get_working_directory()
      run()
    end)
  end)
  return P.Pandoc({P.CodeBlock(table.concat(log, '\n'))})
end
"""

GROUPS = {}

GROUPS["system-files"] = [
    ("mkdir a", "return S.make_directory('a')"),
    ("mkdir a again", "return S.make_directory('a')"),
    ("mkdir b/c", "return S.make_directory('b/c')"),
    ("mkdir b/c parents", "return S.make_directory('b/c', true)"),
    ("mkdir b/c parents again", "return S.make_directory('b/c', true)"),
    ("mkdir parents false", "return S.make_directory('d/e', false)"),
    ("mkdir results", "return select('#', S.make_directory('m'))"),
    ("write a/f", "return S.write_file('a/f', 'xyz')"),
    ("write results", "return select('#', S.write_file('a/w', ''))"),
    ("write missing folder", "return S.write_file('nope/x', 'x')"),
    ("write to folder", "return S.write_file('a', 'x')"),
    ("read a/f", "return S.read_file('a/f')"),
    ("read missing", "return S.read_file('nope')"),
    ("read folder", "return S.read_file('a')"),
    ("list a", "return sorted(S.list_directory('a'))"),
    ("list type", "local l = S.list_directory('a') return type(l), rawget(getmetatable(l) or {}, '__name')"),
    ("list missing", "return S.list_directory('nope')"),
    ("list file", "return S.list_directory('a/f')"),
    ("copy", "return S.copy('a/f', 'a/g')"),
    ("copy read", "return S.read_file('a/g')"),
    ("copy over", "S.write_file('a/o', 'old') S.copy('a/f', 'a/o') return S.read_file('a/o')"),
    ("copy missing", "return S.copy('nope', 'a/h')"),
    ("copy to folder", "return S.copy('a/f', 'b')"),
    ("rename", "return S.rename('a/g', 'a/h')"),
    ("rename read", "return S.read_file('a/h')"),
    ("rename missing", "return S.rename('a/g', 'a/i')"),
    ("rename file over folder", "return S.rename('a/h', 'b')"),
    ("rename folder", "S.make_directory('r1') return S.rename('r1', 'r2'), P.path.exists('r2', 'directory')"),
    ("times types", "local m, a = S.times('a/f') return type(m), type(a), select('#', S.times('a/f'))"),
    ("times form", "local m = S.times('a/f') return m:match('^%d%d%d%d%-%d%d%-%d%dT%d%d:%d%d:%d%d[.%d]*Z$') ~= nil"),
    ("times missing", "return S.times('nope')"),
    ("remove", "return S.remove('a/h')"),
    ("remove missing", "return S.remove('a/h')"),
    ("remove folder", "return S.remove('b')"),
    ("rmdir nonempty", "return S.remove_directory('a')"),
    ("rmdir missing", "return S.remove_directory('nope')"),
    ("rmdir file", "return S.remove_directory('a/f')"),
    ("rmdir empty", "S.make_directory('e') return S.remove_directory('e'), P.path.exists('e')"),
    ("rmdir empty recursive", "S.make_directory('e2') return S.remove_directory('e2', true), P.path.exists('e2')"),
    ("exists", "return P.path.exists('a'), P.path.exists('a', 'directory'), P.path.exists('a', 'file'), "
     "P.path.exists('a/f', 'file'), P.path.exists('a/f', 'directory'), P.path.exists('nope'), "
     "P.path.exists('a', 'symlink'), P.path.exists('nope', 'file')"),
    ("exists bad type", "return P.path.exists('a', 'pipe')"),
    ("exists number", "return P.path.exists(5)"),
]

GROUPS["system-directories"] = [
    ("with_wd", "return S.with_working_directory('.', function() return S.get_working_directory() end)"),
    ("with_wd sub", "S.make_directory('w') S.write_file('w/f', 'in w') "
     "return S.with_working_directory('w', function() return S.get_working_directory(), S.read_file('f') end)"),
    ("with_wd after", "return S.get_working_directory()"),
    ("with_wd error", "return S.with_working_directory('w', function() error('in w', 0) end)"),
    ("with_wd error after", "return S.get_working_directory()"),
    ("with_wd missing", "return S.with_working_directory('nope', function() return 1 end)"),
    ("with_wd results", "return select('#', S.with_working_directory('.', function() return nil, nil end))"),
    ("with_wd values", "return S.with_working_directory('.', function() return 1, 'two', {3} end)"),
    ("with_tmp", "local seen return S.with_temporary_directory('.', 'tt', function(d) seen = d "
     "return d:match('^%./tt%-%x+$') ~= nil, P.path.exists(d, 'directory') end), P.path.exists(seen)"),
    ("with_tmp error", "return S.with_temporary_directory('.', 'tt', function() error('x', 0) end)"),
    ("with_tmp results", "return select('#', S.with_temporary_directory('.', 'tt', function() end))"),
    ("list after", "local l = {} for _, n in ipairs(S.list_directory('.')) do "
     "if n:sub(1, 2) ~= '._' then l[#l + 1] = n end end return sorted(l)"),
]

GROUPS["system-environment"] = [
    ("env type", "local e = S.environment() return type(e), getmetatable(e), type(e.PATH)"),
    ("with_env", "return S.with_environment({X_PROBE = 'v'}, function() "
     "return os.getenv('X_PROBE'), S.environment().X_PROBE, os.getenv('PATH') == nil end)"),
    ("with_env after", "return os.getenv('X_PROBE'), S.environment().X_PROBE, os.getenv('PATH') ~= nil"),
    ("with_env error", "return S.with_environment({X_PROBE = 'v'}, function() error('boom', 0) end)"),
    ("with_env error after", "return os.getenv('X_PROBE'), os.getenv('PATH') ~= nil"),
    ("with_env number", "return S.with_environment({X_PROBE = 1}, function() return S.environment().X_PROBE end)"),
    ("with_env count", "return S.with_environment({A1 = 'a', B1 = 'b'}, function() "
     "local n = 0 for _ in pairs(S.environment()) do n = n + 1 end return n end)"),
    ("with_env empty", "return S.with_environment({}, function() return next(S.environment()) end)"),
    ("with_env results", "return select('#', S.with_environment({}, function() end))"),
    ("with_env command", "return S.with_environment({X_PROBE = 'seen'}, function() "
     "return S.command('sh', {'-c', 'echo $X_PROBE'}) end)"),
    ("getenv missing", "return os.getenv('X_PROBE_NEVER_SET')"),
    ("xdg config", "return S.with_environment({HOME = '/h'}, function() return S.xdg('config'), S.xdg('data', 'x/y'), "
     "S.xdg('datadirs'), S.xdg('configdirs'), S.xdg('cache'), S.xdg('state') end)"),
    ("xdg variables", "return S.with_environment({HOME = '/h', XDG_CONFIG_HOME = 'rel', XDG_DATA_DIRS = '/a::/b', "
     "XDG_CACHE_HOME = '/c'}, function() return S.xdg('config'), S.xdg('datadirs'), S.xdg('cache', '/abs'), "
     "S.xdg('CONFIG_DIRS'), S.xdg('XDG_CACHE'), S.xdg('cache', 'f') end)"),
    ("xdg names", "return S.with_environment({HOME = '/h'}, function() return S.xdg('Config'), S.xdg('XDGDATA'), "
     "S.xdg('xdg_state_') end)"),
    ("xdg bad", "return S.xdg('nope')"),
    ("xdg home", "return S.xdg('Xdg_Config_Home')"),
    ("xdg none", "return S.xdg()"),
]

GROUPS["system-processes"] = [
    ("command echo", "return S.command('echo', {'a', 'b'})"),
    ("command cat", "return S.command('cat', {}, 'in put')"),
    ("command cat number", "return S.command('cat', {}, 12)"),
    ("command false", "return S.command('false', {})"),
    ("command true", "return S.command('true', {})"),
    ("command sh", "return S.command('sh', {'-c', 'echo err 1>&2; exit 3'})"),
    ("command signal", "return S.command('sh', {'-c', 'kill -9 $$'})"),
    ("command opts", "return S.command('echo', {'x'}, '', {})"),
    ("command noargs", "return S.command('true')"),
    ("command args string", "return S.command('true', 'x')"),
    ("command none", "return S.command()"),
    ("command in folder", "S.make_directory('cw') return S.with_working_directory('cw', function() "
     "return S.command('sh', {'-c', 'pwd'}) end)"),
    ("pipe cat", "return P.pipe('cat', {}, 'pi')"),
    ("pipe echo", "return P.pipe('echo', {'x', 'y'}, '')"),
    ("pipe false", "return P.pipe('false', {}, '')"),
    ("pipe error fields", "local ok, e = pcall(P.pipe, 'sh', {'-c', 'echo o; exit 2'}, '') "
     "return ok, e.command, e.error_code, e.output, P.utils.type(e)"),
    ("pipe error text", "local ok, e = pcall(P.pipe, 'sh', {'-c', 'echo o; exit 2'}, '') return tostring(e)"),
    ("pipe no input", "return P.pipe('true', {})"),
    ("pipe none", "return P.pipe()"),
]

GROUPS["system-cputime"] = [
    ("cputime", "local t = S.cputime() return math.type(t), t > 0"),
]


def lua_string(text):
    out = []
    for byte in text.encode("utf-8"):
        if 32 <= byte < 127 and chr(byte) not in "\\'":
            out.append(chr(byte))
        else:
            out.append(f"\\{byte:03d}")
    return "'" + "".join(out) + "'"


def filter_source(probes):
    lines = [HELPER, "local function run()"]
    for label, body in probes:
        lines.append(f"  probe({lua_string(label)}, function() {body} end)")
    lines.append("end")
    lines.append(FOOTER)
    return "\n".join(lines)


def main():
    parts = [
        "# The oracle matrix of `pandoc.system`, `pandoc.path.exists` and `pandoc.pipe`.\n",
        "# `tools/system-matrix.py` writes this file. Do not edit it.\n",
    ]
    for name, text in DOCS.items():
        parts.append(f"@@@ doc {name}\n{text}\n")
    for name, probes in GROUPS.items():
        parts.append(f"@@@ filter {name} empty small\n{filter_source(probes)}\n")
    with open(OUTPUT, "w", encoding="utf-8") as handle:
        handle.write("".join(parts))
    print(f"{len(GROUPS)} filters, {sum(len(p) for p in GROUPS.values())} probes")


if __name__ == "__main__":
    main()
