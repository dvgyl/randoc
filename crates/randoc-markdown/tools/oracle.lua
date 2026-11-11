-- Reads many Markdown inputs in one oracle process.
-- Usage: pandoc lua oracle.lua CASES.json RESULTS.json
-- CASES.json is a list of {"id", "from", "input"}. RESULTS.json maps each id
-- to the JSON output of the document, or to {"error": TEXT}.
local cases_path, out_path = arg[1], arg[2]
local f = assert(io.open(cases_path, 'r'))
local cases = pandoc.json.decode(f:read('a'), false)
f:close()
local results = {}
for _, case in ipairs(cases) do
  local ok, doc = pcall(pandoc.read, case.input, case.from, { columns = 72 })
  if ok then
    results[case.id] = pandoc.write(doc, 'json')
  else
    results[case.id] = pandoc.json.encode({ error = tostring(doc) })
  end
end
local parts = {}
for id, json in pairs(results) do
  parts[#parts + 1] = pandoc.json.encode(id) .. ':' .. json
end
local out = assert(io.open(out_path, 'w'))
out:write('{' .. table.concat(parts, ',') .. '}')
out:close()
