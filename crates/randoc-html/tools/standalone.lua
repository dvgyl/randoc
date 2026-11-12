-- Reads one JSON request per line: {"markdown":..., "fmt":..., "opts":{...}, "template":...};
-- writes one JSON line {"doc": JSON text of the document, "out": pandoc.write output}.
local json = pandoc.json
local templates = json.decode(io.read("l"), false).templates
for line in io.lines() do
  local req = json.decode(line, false)
  local o = req.opts or {}
  local ok, res = pcall(function()
    local doc = pandoc.read(req.markdown, "markdown")
    local source = templates[req.template] or pandoc.template.default(req.fmt)
    local wo = {template = pandoc.template.compile(source)}
    if o.toc then wo.table_of_contents = true end
    if o.toc_depth then wo.toc_depth = o.toc_depth end
    if o.section_divs then wo.section_divs = true end
    if o.number_sections then wo.number_sections = true end
    if o.id_prefix then wo.identifier_prefix = o.id_prefix end
    if o.q_tags then wo.html_q_tags = true end
    if o.ascii then wo.prefer_ascii = true end
    if o.wrap then wo.wrap_text = ({none="wrap-none", preserve="wrap-preserve", auto="wrap-auto"})[o.wrap] end
    if o.columns then wo.columns = o.columns end
    if o.mathjax then wo.html_math_method = "mathjax" end
    if o.caption_below then wo.table_caption_position = "below" end
    local variables = o.variables or {}
    -- The command line owns the fallback of `pagetitle`: each case without a
    -- title gets a fixed one, and the writer test does the same.
    local title = doc.meta.title
    if pandoc.utils.type(title) == "Blocks" then
      title = (#title == 1 and (title[1].t == "Para" or title[1].t == "Plain")) and title[1].content or ""
    end
    if not variables.pagetitle and pandoc.utils.stringify(title or "") == "" then
      variables.pagetitle = "Fixed"
    end
    wo.variables = variables
    return {doc = pandoc.write(doc, "json"), out = pandoc.write(doc, req.fmt, pandoc.WriterOptions(wo))}
  end)
  if ok then io.write(json.encode(res), "\n") else io.write(json.encode({error = tostring(res)}), "\n") end
end
