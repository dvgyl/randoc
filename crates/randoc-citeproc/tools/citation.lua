-- Writes tests/replay/citation.txt: for the cases FIRST to LAST of
-- tests/replay/citation-cases.txt (Markdown citation, citation items,
-- layout attributes, layout content), the case fields and the citation
-- inlines of the oracle (JSON). The style is the template below with the
-- locale tests/replay/citation-locale.xml; the bibliography is
-- tests/replay/citation-refs.json. Case 1 starts the file; later cases are
-- appended, so that each oracle process stays short.
-- Usage (from the crate folder): pandoc lua tools/citation.lua SCRATCH_DIR FIRST LAST
local dir, first, last = arg[1], tonumber(arg[2]), tonumber(arg[3])
local locale_file = io.open('tests/replay/citation-locale.xml')
local locale = locale_file:read('l')
locale_file:close()
local out = io.open('tests/replay/citation.txt', first == 1 and 'w' or 'a')
if first == 1 then
  out:write('# Oracle results of citations. Fields: CITATION, ITEMS, LAYOUT ATTRIBUTES, LAYOUT, INLINES (JSON).\n')
  out:write('# Written by tools/citation.lua. The styles and references are new; none is copied.\n')
end
local n = 0
for line in io.lines('tests/replay/citation-cases.txt') do
  n = n + 1
  if n >= first and n <= last then
    local form, items, attrs, inner = line:match('^(.-)\t(.-)\t(.-)\t(.*)$')
    local style = '<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="en-US">'
      .. locale
      .. '<macro name="m"><text variable="publisher" font-weight="bold"/></macro>'
      .. '<citation><layout ' .. attrs .. '>' .. inner .. '</layout></citation></style>'
    local f = io.open(dir .. '/c.csl', 'w'); f:write(style); f:close()
    local doc = pandoc.read(form, 'markdown')
    doc.meta.bibliography = 'tests/replay/citation-refs.json'
    doc.meta.csl = dir .. '/c.csl'
    doc.meta['suppress-bibliography'] = true
    local cite = pandoc.utils.citeproc(doc).blocks[1].content[1].content
    out:write(form, '\t', items, '\t', attrs, '\t', inner, '\t',
      pandoc.write(pandoc.Pandoc({ pandoc.Plain(cite) }), 'json'), '\n')
  end
end
out:close()
