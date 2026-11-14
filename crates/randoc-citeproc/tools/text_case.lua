-- Writes tests/replay/text-case.txt: for each text case and each title of
-- tests/replay/text-case-titles.txt, the title inlines and the citation
-- inlines of the oracle, as JSON documents with one Plain block.
-- Usage (from the crate folder): pandoc lua tools/text_case.lua SCRATCH_DIR
-- SCRATCH_DIR gets one small style for each text case.
local dir = arg[1]
local cases = {'lowercase', 'uppercase', 'capitalize-first', 'capitalize-all', 'sentence', 'title'}
for _, case in ipairs(cases) do
  local style = io.open(dir .. '/' .. case .. '.csl', 'w')
  style:write('<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="en-US">',
    '<citation><layout><text variable="title" text-case="', case, '"/></layout></citation></style>')
  style:close()
end
local out = io.open('tests/replay/text-case.txt', 'w')
out:write('# Oracle results of the text cases. Fields: CASE, TITLE, CITATION (JSON).\n')
out:write('# Written by tools/text_case.lua. The titles are new; none is copied.\n')
for title in io.lines('tests/replay/text-case-titles.txt') do
  local inlines = pandoc.read(title, 'markdown').blocks[1].content
  for _, case in ipairs(cases) do
    local doc = pandoc.read('[@a]', 'markdown')
    doc.meta.references = { pandoc.MetaMap({ id = 'a', type = 'book', title = pandoc.MetaInlines(inlines) }) }
    doc.meta.csl = dir .. '/' .. case .. '.csl'
    doc.meta['suppress-bibliography'] = true
    local cite = pandoc.utils.citeproc(doc).blocks[1].content[1].content
    out:write(case, '\t', pandoc.write(pandoc.Pandoc({ pandoc.Plain(inlines) }), 'json'), '\t',
      pandoc.write(pandoc.Pandoc({ pandoc.Plain(cite) }), 'json'), '\n')
  end
end
out:close()
