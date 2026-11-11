-- Reads a JSON list of YAML texts from arg[1]. Writes a JSON list to arg[2]:
-- for each text, "ok " and the JSON document, or "err " and the error text.
-- Each text is the content of one metadata block.
local inputs = pandoc.json.decode(io.open(arg[1]):read("a"))
local results = {}
for i, text in ipairs(inputs) do
  local ok, doc = pcall(pandoc.read, "---\n" .. text .. "---\n", "commonmark+yaml_metadata_block")
  if ok then
    results[i] = "ok " .. pandoc.write(doc, "json")
  else
    results[i] = "err " .. tostring(doc)
  end
end
io.open(arg[2], "w"):write(pandoc.json.encode(results))
