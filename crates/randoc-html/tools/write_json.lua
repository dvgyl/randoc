-- Reads one JSON request per line: {"doc": JSON text, "fmt":..., "opts":{...}, "template": text or null};
-- writes one JSON line {"out": pandoc.write output}.
local json = pandoc.json
for line in io.lines() do
  local req = json.decode(line, false)
  local o = req.opts or {}
  local ok, res = pcall(function()
    local doc = pandoc.read(req.doc, "json")
    local wo = {}
    if type(req.template) == "string" then wo.template = pandoc.template.compile(req.template) end
    if o.email then wo.email_obfuscation = o.email end
    if o.katex then wo.html_math_method = {method = "katex", url = o.katex} end
    if o.math then wo.html_math_method = o.math end
    if o.toc then wo.table_of_contents = true end
    if o.number_sections then wo.number_sections = true end
    if o.section_divs then wo.section_divs = true end
    wo.variables = o.variables or {}
    return {out = pandoc.write(doc, req.fmt, pandoc.WriterOptions(wo))}
  end)
  if ok then io.write(json.encode(res), "\n") else io.write(json.encode({error = tostring(res)}), "\n") end
end
