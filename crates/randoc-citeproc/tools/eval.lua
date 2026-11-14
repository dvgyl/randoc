-- Writes tests/replay/eval.txt: for each case of tests/replay/eval-cases.txt
-- (item, layout attributes, layout content), the item, the style and the
-- citation inlines of the oracle (JSON), with the bibliography
-- tests/replay/eval-refs.json.
-- Usage (from the crate folder): pandoc lua tools/eval.lua SCRATCH_DIR
local dir = arg[1]
local out = io.open('tests/replay/eval.txt', 'w')
out:write('# Oracle results of citations with text, group and choose. Fields: ITEM, STYLE, CITATION (JSON).\n')
out:write('# Written by tools/eval.lua. The styles and references are new; none is copied.\n')
for line in io.lines('tests/replay/eval-cases.txt') do
  local id, fmt, inner = line:match('^(.-)\t(.-)\t(.*)$')
  local style = '<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="en-US">'
    .. '<locale><terms><term name="test-term">long form</term><term name="test-term" form="short">sh.</term>'
    .. '<term name="page-range-delimiter">\u{2013}</term></terms></locale>'
    .. '<macro name="m"><text variable="publisher" font-weight="bold"/></macro>'
    .. '<citation><layout ' .. fmt .. '>' .. inner .. '</layout></citation></style>'
  local f = io.open(dir .. '/s.csl', 'w'); f:write(style); f:close()
  local doc = pandoc.read('[@' .. id .. ']', 'markdown')
  doc.meta.bibliography = 'tests/replay/eval-refs.json'
  doc.meta.csl = dir .. '/s.csl'
  doc.meta['suppress-bibliography'] = true
  local cite = pandoc.utils.citeproc(doc).blocks[1].content[1].content
  out:write(id, '\t', style, '\t', pandoc.write(pandoc.Pandoc({ pandoc.Plain(cite) }), 'json'), '\n')
end
out:close()
