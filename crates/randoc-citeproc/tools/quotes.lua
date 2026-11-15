-- Writes tests/replay/quotes.txt: for each case of tests/replay/quotes-cases.txt
-- (locale, item, layout attributes, layout content), the item, the style and
-- the citation inlines of the oracle (JSON), with the bibliography
-- tests/replay/quotes-refs.json.
-- Usage (from the crate folder): pandoc lua tools/quotes.lua SCRATCH_DIR
-- [CASES OUT]: CASES and OUT give a part of the cases and the file of its
-- results (without the header lines).
local dir = arg[1]
local out = io.open(arg[3] or 'tests/replay/quotes.txt', 'w')
if not arg[3] then
  out:write('# Oracle results of citations with the quote terms of a locale. Fields: ITEM, STYLE, CITATION (JSON).\n')
  out:write('# Written by tools/quotes.lua. The styles and references are new; none is copied.\n')
end
for line in io.lines(arg[2] or 'tests/replay/quotes-cases.txt') do
  local locale, id, fmt, inner = line:match('^(.-)\t(.-)\t(.-)\t(.*)$')
  local style = '<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="en-US">'
    .. locale .. '<citation><layout ' .. fmt .. '>' .. inner .. '</layout></citation></style>'
  local f = io.open(dir .. '/s.csl', 'w'); f:write(style); f:close()
  local doc = pandoc.read('[@' .. id .. ']', 'markdown')
  doc.meta.bibliography = 'tests/replay/quotes-refs.json'
  doc.meta.csl = dir .. '/s.csl'
  doc.meta['suppress-bibliography'] = true
  local cite = pandoc.utils.citeproc(doc).blocks[1].content[1].content
  out:write(id, '\t', style, '\t', pandoc.write(pandoc.Pandoc({ pandoc.Plain(cite) }), 'json'), '\n')
end
out:close()
