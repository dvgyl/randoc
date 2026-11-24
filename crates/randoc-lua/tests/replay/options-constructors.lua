local P = pandoc
local layout = P.layout
assert(require 'pandoc.template' == P.template)
assert(require 'pandoc.scaffolding' == P.scaffolding)
assert(require 'pandoc.cli' == P.cli)

local reader = P.ReaderOptions { tab_stop = 2 }
assert(type(reader) == 'userdata' and P.utils.type(reader) == 'ReaderOptions')
assert(reader.columns == 80 and reader.tab_stop == 2 and reader.abbreviations['Mr.'])
reader.tab_stop = 8
assert(P.ReaderOptions(reader).tab_stop == 8)
assert(not pcall(function() reader.unknown = true end))

local writer = P.WriterOptions { toc_depth = 1, variables = { title = layout.literal('Title') } }
assert(type(writer) == 'userdata' and P.utils.type(writer) == 'WriterOptions')
assert(writer.dpi == 96 and writer.toc_depth == 1 and #writer.number_offset == 6)
assert(P.utils.type(writer.variables.title) == 'Doc')
assert(tostring(writer.variables.title) == 'Title')
writer.epub_chapter_level = 4
assert(writer.split_level == 4)

local inputs = P.types.Sources {'one', { name = 'second', text = 'two' }}
assert(P.utils.type(inputs) == 'Sources' and #inputs == 2)
assert(P.utils.type(inputs[2]) == 'Source' and inputs[2].name == 'second')
assert(inputs[2].text == 'two\n' and tostring(inputs) == 'onetwo\n')
assert(tostring(inputs:clone()) == tostring(inputs))

assert(P.template.default('json') == '')
assert(P.template.default('html') == P.template.get('default.html5'))
assert(not pcall(P.template.get, 'missing-template-for-lua-replay'))

local context = P.template.meta_to_context(P.Meta {
  name = P.Inlines 'Ada',
  bio = P.Blocks { P.Para 'Writer' },
  active = true,
}, P.utils.stringify, P.utils.stringify)
assert(P.utils.type(context) == 'pandoc Context')
assert(P.utils.type(context.name) == 'Doc' and tostring(context.bio) == 'Writer')
assert(tostring(P.template.apply('$name$/$bio$', context)) == 'Ada/Writer')

local parsed = P.cli.parse_options { '-f', 'markdown', '--toc', '-V', 'key=value', 'a.md' }
assert(P.cli.default_options.columns == 72 and parsed.from == 'markdown')
assert(parsed['table-of-contents'] and parsed['input-files'][1] == 'a.md')
assert(parsed.variables.key == 'value')

local first = PANDOC_STATE.resource_path[1]
local result, gap, last = P.with_state({ resource_path = { 'test-resources' } }, function()
  assert(PANDOC_STATE.resource_path[1] == 'test-resources')
  P.with_state({ resource_path = { 'nested' } }, function()
    assert(PANDOC_STATE.resource_path[1] == 'nested')
  end)
  assert(PANDOC_STATE.resource_path[1] == 'test-resources')
  return 'first', nil, 'last'
end)
assert(result == 'first' and gap == nil and last == 'last')
assert(PANDOC_STATE.resource_path[1] == first)

local W = P.scaffolding.Writer
assert(P.scaffolding.Reader == nil)
W.Inline.Str = function(str) return str.text end
W.Block.Para = function(para) return W.Inlines(para.content) end
local doc = P.Pandoc { P.Para 'Text', P.Para 'Next' }
writer.template = P.template.compile '$title$\n$body$'
assert(W(doc, writer) == 'Title\nText\n\nNext')

assert(not pcall(P.with_state, { resource_path = { 'test-resources' } }, function()
  error('callback failed')
end))
