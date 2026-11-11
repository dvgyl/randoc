-- Reads a JSON list of Markdown documents from arg[1]. Writes a JSON list to
-- arg[2]: for each document, "ok " and the JSON document, or "err " and the
-- error text.
local inputs = pandoc.json.decode(io.open(arg[1]):read("a"))
local results = {}
for i, text in ipairs(inputs) do
  local ok, doc = pcall(pandoc.read, text, "markdown")
  if ok then
    results[i] = "ok " .. pandoc.write(doc, "json")
  else
    results[i] = "err " .. tostring(doc)
  end
end
io.open(arg[2], "w"):write(pandoc.json.encode(results))
