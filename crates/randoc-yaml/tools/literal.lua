-- Reads a JSON list of defaults files from arg[1]. Writes a JSON list to
-- arg[2]: for each file, "ok " and the JSON of the value below the key arg[4]
-- (`variables` or `metadata`), or "err " and the error text. arg[3] is the
-- path of the defaults file. Pandoc does not read these strings as Markdown,
-- so their characters stay literal.
local inputs = pandoc.json.decode(io.open(arg[1]):read("a"))
local results = {}
for i, text in ipairs(inputs) do
  local f = io.open(arg[3], "wb")
  f:write(text)
  f:close()
  local ok, opts = pcall(pandoc.cli.parse_options, {"-d", arg[3]})
  if ok then
    results[i] = "ok " .. pandoc.json.encode(opts[arg[4]] or {})
  else
    results[i] = "err " .. tostring(opts)
  end
end
io.open(arg[2], "w"):write(pandoc.json.encode(results))
