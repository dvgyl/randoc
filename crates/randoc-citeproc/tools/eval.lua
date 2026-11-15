-- Writes tests/replay/SUITE.txt: for each case of tests/replay/SUITE-cases.txt
-- (item, layout attributes, layout content), the item, the style and the
-- citation inlines of the oracle (JSON). Layout attributes that start with
-- '@' go on the citation element instead. The bibliography is
-- tests/replay/SUITE-refs.json and the style locale is
-- tests/replay/SUITE-locale.xml (one line).
-- Usage (from the crate folder): pandoc lua tools/eval.lua SCRATCH_DIR SUITE
local dir = arg[1]
local suite = arg[2]
local locale_file = io.open('tests/replay/' .. suite .. '-locale.xml')
local locale = locale_file:read('l')
locale_file:close()
local out = io.open('tests/replay/' .. suite .. '.txt', 'w')
out:write('# Oracle results of citations. Fields: ITEM, STYLE, CITATION (JSON).\n')
out:write('# Written by tools/eval.lua. The styles and references are new; none is copied.\n')
for line in io.lines('tests/replay/' .. suite .. '-cases.txt') do
  local id, fmt, inner = line:match('^(.-)\t(.-)\t(.*)$')
  local cite_attrs = ''
  if fmt:sub(1, 1) == '@' then cite_attrs, fmt = ' ' .. fmt:sub(2), '' end
  local style = '<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="en-US">'
    .. locale
    .. '<macro name="m"><text variable="publisher" font-weight="bold"/></macro>'
    .. '<citation' .. cite_attrs .. '><layout ' .. fmt .. '>' .. inner .. '</layout></citation></style>'
  local f = io.open(dir .. '/s.csl', 'w'); f:write(style); f:close()
  local doc = pandoc.read('[@' .. id .. ']', 'markdown')
  doc.meta.bibliography = 'tests/replay/' .. suite .. '-refs.json'
  doc.meta.csl = dir .. '/s.csl'
  doc.meta['suppress-bibliography'] = true
  local cite = pandoc.utils.citeproc(doc).blocks[1].content[1].content
  out:write(id, '\t', style, '\t', pandoc.write(pandoc.Pandoc({ pandoc.Plain(cite) }), 'json'), '\n')
end
out:close()
