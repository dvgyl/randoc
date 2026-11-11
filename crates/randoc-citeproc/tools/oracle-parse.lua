-- Prints the oracle result of each style: PATH<TAB>ok or PATH<TAB>ERROR.
-- Usage: pandoc lua oracle-parse.lua LIST_FILE (one style path on each line).
local ref = pandoc.read('---\nreferences:\n- id: a\n  type: book\n  title: T\n  author: [{family: Doe, given: J}]\n  issued: {date-parts: [[2001]]}\n...\n', 'markdown').meta.references
for path in io.lines(arg[1]) do
  if path ~= '' then
    local doc = pandoc.read('[@a]', 'markdown')
    doc.meta.references = ref
    doc.meta.csl = path
    local ok, err = pcall(pandoc.utils.citeproc, doc)
    local text = ok and 'ok' or tostring(err)
    io.write(path, '\t', (text:gsub('\n', '\\n')), '\n')
    io.flush()
  end
end
