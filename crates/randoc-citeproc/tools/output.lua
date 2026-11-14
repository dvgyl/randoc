-- Writes tests/replay/output.txt: for each layout of
-- tests/replay/output-layouts.txt (layout attributes, a tab, the layout
-- content), the style and the citation inlines of the oracle as a JSON
-- document with one Plain block.
-- Usage (from the crate folder): pandoc lua tools/output.lua SCRATCH_DIR
local dir = arg[1]
local out = io.open('tests/replay/output.txt', 'w')
out:write('# Oracle results of layouts with text values and groups. Fields: STYLE, CITATION (JSON).\n')
out:write('# Written by tools/output.lua. The layouts are new; none is copied.\n')
for line in io.lines('tests/replay/output-layouts.txt') do
  local attrs, inner = line:match('^(.-)\t(.*)$')
  local style = '<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="en-US">'
    .. '<citation><layout ' .. attrs .. '>' .. inner .. '</layout></citation></style>'
  local file = io.open(dir .. '/s.csl', 'w')
  file:write(style)
  file:close()
  local doc = pandoc.read('[@a]', 'markdown')
  doc.meta.references = { pandoc.MetaMap({ id = 'a', type = 'book', title = pandoc.MetaInlines({ pandoc.Str('T') }) }) }
  doc.meta.csl = dir .. '/s.csl'
  doc.meta['suppress-bibliography'] = true
  local cite = pandoc.utils.citeproc(doc).blocks[1].content[1].content
  out:write(style, '\t', pandoc.write(pandoc.Pandoc({ pandoc.Plain(cite) }), 'json'), '\n')
end
out:close()
