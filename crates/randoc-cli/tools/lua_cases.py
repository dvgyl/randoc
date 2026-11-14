import json
DOC = '{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"a"}]}]}'
HDOC = '{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Header","c":[1,["h",[],[]],[{"t":"Str","c":"h"}]]},{"t":"Para","c":[{"t":"Str","c":"a"}]}]}'

def suffix(old, new):
    return f"function Str(s)\n  if s.text == '{old}' then return pandoc.Str('{new}') end\nend\n"

def show(expr):
    return ("function Pandoc(doc)\n  doc.blocks:insert(pandoc.Para({pandoc.Str(" + expr + ")}))\n  return doc\nend\n")

SH = lambda old, new: ("#!/bin/sh\nwhile IFS= read -r l || [ -n \"$l\" ]; do printf '%s' \"${l//\\\"c\\\":\\\"" + old + "\\\"/\\\"c\\\":\\\"" + new + "\\\"}\"; done\n")

STATE = ("table.concat(PANDOC_STATE.input_files, ',') .. ';' .. tostring(PANDOC_STATE.output_file) .. ';' .. "
         "PANDOC_STATE.verbosity .. ';' .. tostring(PANDOC_STATE.trace) .. ';' .. "
         "table.concat(PANDOC_STATE.resource_path, ',') .. ';' .. tostring(PANDOC_STATE.source_url)")
HEADERS = ("(function() local t = {} for _, p in ipairs(PANDOC_STATE.request_headers) do "
           "t[#t + 1] = p[1] .. '=' .. p[2] end return table.concat(t, ',') end)()")

cases = []
def case(id, cmd, files, input=DOC, comment=None):
    cases.append((id, cmd, files, input, comment))

J = ["-f", "json", "-t", "native"]
# The search of the file.
case("lua-path", J + ["-L", "a.lua"], {"a.lua": suffix("a", "aL")}, comment="The search of the filter file: the path, then `filters/` of the user data folder.")
case("lua-path-long-option", J + ["--lua-filter=a.lua"], {"a.lua": suffix("a", "aL")})
case("lua-path-in-a-folder", J + ["-L", "sub/a.lua"], {"sub/a.lua": suffix("a", "aS")})
case("lua-path-dot-slash", J + ["-L", "./a.lua"], {"a.lua": show("PANDOC_SCRIPT_FILE")})
case("lua-data-dir", J + ["--data-dir=dd", "-L", "a.lua"], {"dd/filters/a.lua": show("PANDOC_SCRIPT_FILE")})
case("lua-data-dir-folder", J + ["--data-dir=dd", "-L", "s/a.lua"], {"dd/filters/s/a.lua": show("PANDOC_SCRIPT_FILE")})
case("lua-path-before-data-dir", J + ["--data-dir=dd", "-L", "a.lua"], {"a.lua": suffix("a", "cwd"), "dd/filters/a.lua": suffix("a", "dd")})
case("lua-missing-in-both", J + ["--data-dir=dd", "-L", "b.lua"], {"dd/filters/a.lua": suffix("a", "dd")})
case("lua-missing-verbose", J + ["--verbose", "-L", "nosuch.lua"], {})
case("lua-directory", J + ["-L", "sub"], {"sub/keep.lua": ""})
case("lua-extension-is-not-needed", J + ["-L", "a.txt"], {"a.txt": suffix("a", "txt")})
# Order.
case("lua-two-filters-in-order", J + ["-L", "one.lua", "-L", "two.lua"], {"one.lua": suffix("a", "a1"), "two.lua": suffix("a1", "a12")}, comment="The order of Lua filters, JSON filters and the filters of a defaults file.")
case("lua-two-filters-reversed", J + ["-L", "two.lua", "-L", "one.lua"], {"one.lua": suffix("a", "a1"), "two.lua": suffix("a1", "a12")})
case("lua-then-json", J + ["-L", "one.lua", "-F", "./two.sh"], {"one.lua": suffix("a", "a1"), "two.sh": SH("a1", "a12")})
case("json-then-lua", J + ["-F", "./one.sh", "-L", "two.lua"], {"one.sh": SH("a", "a1"), "two.lua": suffix("a1", "a12")})
case("lua-defaults-string", J + ["-d", "d.yaml"], {"d.yaml": "filters:\n- one.lua\n- two.lua\n", "one.lua": suffix("a", "a1"), "two.lua": suffix("a1", "a12")})
case("lua-defaults-map", J + ["-d", "d.yaml"], {"d.yaml": "filters:\n- type: lua\n  path: one.flt\n- type: json\n  path: ./two.sh\n", "one.flt": suffix("a", "a1"), "two.sh": SH("a1", "a12")})
case("lua-defaults-map-without-type", J + ["-d", "d.yaml"], {"d.yaml": "filters:\n- path: one.lua\n", "one.lua": suffix("a", "a1")})
case("lua-defaults-then-option", J + ["-L", "two.lua", "-d", "d.yaml"], {"d.yaml": "filters: [one.lua]\n", "one.lua": suffix("a", "a1"), "two.lua": suffix("a1", "a12")})
case("lua-option-then-defaults", J + ["-d", "d.yaml", "-L", "two.lua"], {"d.yaml": "filters: [one.lua]\n", "one.lua": suffix("a", "a1"), "two.lua": suffix("a1", "a12")})
case("lua-defaults-data-dir", J + ["-d", "d.yaml"], {"d.yaml": "data-dir: dd\nfilters: [a.lua]\n", "dd/filters/a.lua": show("PANDOC_SCRIPT_FILE")})
case("lua-defaults-json-filter-named-lua", J + ["-d", "d.yaml"], {"d.yaml": "filters:\n- type: json\n  path: ./x.lua\n", "x.lua": SH("a", "j")})
# Globals.
case("lua-format", ["-f", "json", "-t", "native", "-L", "g.lua"], {"g.lua": show("FORMAT")}, comment="The globals of a filter.")
case("lua-format-without-extensions", ["-f", "json", "-t", "html5+smart-raw_html", "-L", "g.lua"], {"g.lua": show("FORMAT")})
case("lua-format-from-output-file", ["-f", "json", "-o", "-", "-t", "markdown_strict", "-L", "g.lua"], {"g.lua": show("FORMAT")})
case("lua-versions", J + ["-L", "g.lua"], {"g.lua": show("tostring(PANDOC_VERSION) .. ' ' .. tostring(PANDOC_API_VERSION) .. ' ' .. type(PANDOC_VERSION)")})
case("lua-script-file", J + ["-L", "sub/../g.lua"], {"g.lua": show("PANDOC_SCRIPT_FILE"), "sub/keep.lua": ""})
case("lua-state-standard-input", J + ["-L", "g.lua"], {"g.lua": show(STATE)})
case("lua-state-files", ["-f", "json", "-t", "native", "-o", "-", "--resource-path=x:y", "-L", "g.lua", "in.json", "in.json"], {"g.lua": show(STATE), "in.json": DOC})
case("lua-state-quiet-trace", J + ["--quiet", "--trace", "-L", "g.lua"], {"g.lua": show(STATE)})
case("lua-state-request-headers", J + ["--request-header=A:b", "-L", "g.lua"], {"g.lua": show(HEADERS)})
case("lua-state-user-agent", J + ["--request-header=User-Agent:me", "-L", "g.lua"], {"g.lua": show(HEADERS)})
case("lua-state-data-dir", J + ["--data-dir=dd", "-L", "g.lua"], {"g.lua": show("tostring(PANDOC_STATE.user_data_dir)"), "dd/keep.lua": ""})
case("lua-state-missing-data-dir", J + ["--data-dir=nodir", "-L", "g.lua"], {"g.lua": show("tostring(PANDOC_STATE.user_data_dir)")})
# Errors.
case("lua-syntax-error", J + ["-L", "e.lua"], {"e.lua": "function Para(\n"}, comment="Error texts and exit codes.")
case("lua-run-time-error", J + ["-L", "e.lua"], {"e.lua": "function Para(p)\n  error('stop here')\nend\n"})
case("lua-run-time-error-at-load", J + ["-L", "e.lua"], {"e.lua": "local t = nil\nprint(t.x)\n"})
case("lua-wrong-return-value", J + ["-L", "e.lua"], {"e.lua": "function Para(p)\n  return 42\nend\n"})
case("lua-wrong-return-value-inline", J + ["-L", "e.lua"], {"e.lua": "function Str(s)\n  return {}, 1\nend\n"})
case("lua-error-stops-the-next-filter", J + ["-L", "e.lua", "-L", "one.lua"], {"e.lua": "error('first')\n", "one.lua": "io.stderr:write('second ran\\n')\n"})
# Standard error of a filter.
case("lua-standard-error", J + ["-L", "w.lua"], {"w.lua": "io.stderr:write('from the filter\\n')\nfunction Str(s)\n  io.stderr:write(s.text .. '\\n')\nend\n"}, comment="The standard error and the standard output of a filter.")
case("lua-print", ["-f", "json", "-t", "native", "-o", "out.txt", "-L", "w.lua"], {"w.lua": "print('printed')\n"})
case("lua-warn-quiet", J + ["--quiet", "-L", "w.lua"], {"w.lua": "io.stderr:write('still shown\\n')\n"})
# The time of the filters.
SEE = ("function Pandoc(doc)\n  local levels = {}\n  for _, b in ipairs(doc.blocks) do\n    if b.t == 'Header' then levels[#levels + 1] = tostring(b.level) end\n  end\n"
       "  local keys = {}\n  for k in pairs(doc.meta) do keys[#keys + 1] = k end\n  table.sort(keys)\n"
       "  doc.blocks:insert(pandoc.Para({pandoc.Str(table.concat(keys, ',') .. ';' .. table.concat(levels, ','))}))\n  return doc\nend\n")
case("lua-before-shift-heading-level", ["-f", "json", "-t", "native", "--shift-heading-level-by=1", "-L", "see.lua"], {"see.lua": SEE}, input=HDOC, comment="The time of the filters: after the metadata options, before the shift of the headings.")
case("lua-after-metadata", ["-f", "json", "-t", "native", "-s", "-M", "k=v", "--metadata-file=m.yaml", "-L", "see.lua"], {"see.lua": SEE, "m.yaml": "f: 1\n"}, input=HDOC)
case("lua-heading-shift-sees-filter-headings", ["-f", "json", "-t", "native", "--shift-heading-level-by=1", "-L", "add.lua"],
     {"add.lua": "function Pandoc(doc)\n  doc.blocks:insert(pandoc.Header(2, 'n'))\n  return doc\nend\n"}, input=HDOC)
case("lua-before-citeproc-error", J + ["-L", "e.lua", "--citeproc"], {"e.lua": "error('before citeproc')\n"})
case("lua-metadata-change-reaches-the-writer", ["-f", "json", "-t", "native", "-s", "-L", "m.lua"], {"m.lua": "function Meta(m)\n  m.added = pandoc.MetaString('yes')\n  return m\nend\n"})
# Sandbox: full mode, as Pandoc.
case("lua-sandbox-reads-a-file", J + ["--sandbox", "-L", "r.lua"], {"r.lua": show("io.open('data.txt'):read('l')"), "data.txt": "secret\n"}, comment="Under `--sandbox` the filter runs in full mode (ADR 11).")
case("lua-sandbox-data-dir", J + ["--sandbox", "--data-dir=dd", "-L", "a.lua"], {"dd/filters/a.lua": suffix("a", "dd")})
case("lua-sandbox-libraries", J + ["--sandbox", "-L", "l.lua"], {"l.lua": show("type(io) .. type(os) .. type(debug) .. type(package)")})
case("lua-sandbox-missing", J + ["--sandbox", "-L", "nosuch.lua"], {})

def q(s):
    return json.dumps(s, ensure_ascii=False)

out = ["# Lua filters (`--lua-filter`, `-L`, a Lua item of `filters:`) with own small filters.\n"]
for id, cmd, files, input, comment in cases:
    if comment:
        out.append(f"\n# {comment}\n")
    out.append("\n[[case]]\n")
    out.append(f"id = {q(id)}\n")
    out.append("cmd = [" + ", ".join(q(c) for c in cmd) + "]\n")
    out.append(f"input = {q(input)}\n")
    out.append('covers = ["opt:lua-filter"]\n')
    if files:
        out.append("[case.files]\n")
        for name, text in files.items():
            out.append(f"{q(name)} = {q(text)}\n")
import sys
open(sys.argv[1], "w").write("".join(out))
print(len(cases))
