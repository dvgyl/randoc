-- Reads one JSON request per line: {"doc":..., "fmt":"html", "opts":{...}}; writes one JSON line {"out":...}.
local json = pandoc.json
for line in io.lines() do
  local req = json.decode(line, false)
  local o = req.opts or {}
  local wo = {}
  if o.section_divs then wo.section_divs = true end
  if o.number_sections then wo.number_sections = true end
  if o.number_offset then wo.number_offset = o.number_offset; wo.number_sections = true end
  if o.refloc then wo.reference_location = ({block="end-of-block", section="end-of-section", document="end-of-document"})[o.refloc] end
  if o.wrap then wo.wrap_text = ({none="wrap-none", preserve="wrap-preserve", auto="wrap-auto"})[o.wrap] end
  if o.columns then wo.columns = o.columns end
  if o.id_prefix then wo.identifier_prefix = o.id_prefix end
  if o.q_tags then wo.html_q_tags = true end
  if o.incremental then wo.incremental = true end
  if o.ascii then wo.prefer_ascii = true end
  if o.email then wo.email_obfuscation = o.email end
  if o.math then wo.html_math_method = o.math end
  local ok, res = pcall(function()
    local doc = pandoc.read(json.encode(req.doc), "json")
    return pandoc.write(doc, req.fmt, pandoc.WriterOptions(wo))
  end)
  if ok then io.write(json.encode({out = res .. "\n"}), "\n") else io.write(json.encode({error = tostring(res)}), "\n") end
end
