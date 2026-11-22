local P = pandoc
local unpack = table.unpack
local functions = {}

local function pack(...)
  return table.pack(...)
end

local function args(entries, prefix)
  for name, values in pairs(entries) do
    functions[(prefix or '') .. name] = function(call)
      local parameters = values()
      return call(unpack(parameters, 1, parameters.n or #parameters))
    end
  end
end

local function word() return P.Str('copper') end
local function para() return P.Para{word()} end
local function doc() return P.Pandoc{para()} end
local function heading() return P.Header(1, {word()}) end
local function cell() return P.Cell{para()} end
local function row() return P.Row{cell()} end
local function identity(value) return value end
local function empty() return {} end
local function layout() return P.layout.literal('é x') end
local function sample() return {'sample.txt'} end
local function values() return P.List{3, 1, 2} end
local function first(value) return value == 3 end

args({
  Attr = function() return {'label', {'mark'}, {shade = 'blue'}} end,
  AttributeList = function() return {{shade = 'blue'}} end,
  BlockQuote = function() return {{para()}} end,
  Blocks = function() return {{para()}} end,
  BulletList = function() return {{{para()}}} end,
  Caption = function() return {{para()}, {word()}} end,
  Cell = function() return {{para()}, 'AlignLeft', 1, 2} end,
  Citation = function() return {'sample', 'NormalCitation'} end,
  Cite = function() return {{word()}, {P.Citation('sample', 'NormalCitation')}} end,
  Code = function() return {'x = 1', P.Attr('label')} end,
  CodeBlock = function() return {'x = 1', P.Attr('label')} end,
  DefinitionList = function() return {{{{word()}, {{para()}}}}} end,
  Div = function() return {{para()}, P.Attr('label')} end,
  Emph = function() return {{word()}} end,
  Figure = function() return {{para()}, P.Caption{para()}} end,
  Header = function() return {2, {word()}, P.Attr('label')} end,
  HorizontalRule = empty,
  Image = function() return {{word()}, 'tile.png', 'A tile'} end,
  Inlines = function() return {{word()}} end,
  LineBlock = function() return {{{word()}, {P.Str('button')}}} end,
  LineBreak = empty,
  Link = function() return {{word()}, '#label', 'A button'} end,
  List = function() return {{3, 1, 2}} end,
  ListAttributes = function() return {3, 'LowerRoman', 'OneParen'} end,
  Math = function() return {'InlineMath', 'x+1'} end,
  Meta = function() return {{label = 'copper'}} end,
  MetaBlocks = function() return {{para()}} end,
  MetaBool = function() return {true} end,
  MetaInlines = function() return {{word()}} end,
  MetaList = function() return {{'copper', true}} end,
  MetaMap = function() return {{label = 'copper'}} end,
  MetaString = function() return {'copper'} end,
  Note = function() return {{para()}} end,
  OrderedList = function() return {{{para()}}, P.ListAttributes(3)} end,
  Pandoc = function() return {{para()}, {label = 'copper'}} end,
  Para = function() return {{word()}} end,
  Plain = function() return {{word()}} end,
  Quoted = function() return {'SingleQuote', {word()}} end,
  RawBlock = function() return {'html', '<hr>'} end,
  RawInline = function() return {'html', '<wbr>'} end,
  Row = function() return {{cell()}, P.Attr('label')} end,
  SimpleTable = function()
    return pack({word()}, {'AlignLeft'}, {0.5}, {{para()}}, {{{para()}}})
  end,
  SmallCaps = function() return {{word()}} end,
  SoftBreak = empty,
  Space = empty,
  Span = function() return {{word()}, P.Attr('label')} end,
  Str = function() return {'copper'} end,
  Strikeout = function() return {{word()}} end,
  Strong = function() return {{word()}} end,
  Subscript = function() return {{word()}} end,
  Superscript = function() return {{word()}} end,
  Table = function()
    return {P.Caption{para()}, {{'AlignLeft', 0.5}}, P.TableHead{row()},
            {P.TableBody({row()}, {}, 0, P.Attr())}, P.TableFoot{row()}}
  end,
  TableBody = function() return {{row()}, {}, 0, P.Attr('label')} end,
  TableFoot = function() return {{row()}, P.Attr('label')} end,
  TableHead = function() return {{row()}, P.Attr('label')} end,
  Underline = function() return {{word()}} end,
  pipe = function() return {'/bin/cat', {}, 'copper'} end,
  read = function() return {'*copper*', 'markdown'} end,
  sha1 = function() return {'copper'} end,
  walk_block = function() return {para(), {Str = function() return P.Str('blue') end}} end,
  walk_inline = function() return {P.Emph{word()}, {Str = function() return P.Str('blue') end}} end,
  with_state = function() return {{}, function() return 'copper', false end} end,
  write = function() return {doc(), 'plain'} end,
}, 'pandoc.')

functions['pandoc.ReaderOptions'] = function(call)
  local value = call{tab_stop = 3}
  return type(value), value.tab_stop
end
functions['pandoc.WriterOptions'] = function(call)
  local value = call{columns = 41}
  return type(value), value.columns
end

functions['pandoc.write_classic'] = function(call)
  local callbacks = {Str = identity, Para = identity, Doc = identity,
                     Blocksep = function() return '\n' end}
  local previous = {}
  for name, callback in pairs(callbacks) do
    previous[name], _G[name] = _G[name], callback
  end
  local result = pack(pcall(call, doc(), {}))
  for name in pairs(callbacks) do _G[name] = previous[name] end
  if not result[1] then error(result[2], 0) end
  return unpack(result, 2, result.n)
end

args({
  __concat = function() return {values(), P.List{4}} end,
  __eq = function() return {values(), values()} end,
  __index = function() return {values(), 'at'} end,
  __tostring = function() return {values()} end,
  at = function() return {values(), -1} end,
  extend = function() return {values(), {4, 5}} end,
  filter = function() return {values(), first} end,
  find = function() return {values(), 1} end,
  find_if = function() return {values(), first} end,
  includes = function() return {values(), 1} end,
  insert = function() return {values(), 2, 4} end,
  map = function() return {values(), function(value) return value * 2 end} end,
  new = function() return {{4, 5}} end,
  remove = function() return {values(), 2} end,
  sort = function() return {values()} end,
}, 'pandoc.List.')

for _, name in ipairs{'extend', 'insert', 'remove', 'sort'} do
  local key = 'pandoc.List.' .. name
  local probe = functions[key]
  functions[key] = function(call)
    return probe(function(list, ...)
      local result = pack(call(list, ...))
      result.n = result.n + 1
      result[result.n] = list
      return unpack(result, 1, result.n)
    end)
  end
end
functions['pandoc.List.iter'] = function(call)
  local items = {}
  for value in call(values()) do items[#items + 1] = value end
  return items
end
functions['pandoc.List.clone'] = function(call) return call(values()) end

args({
  parse_options = function() return {{'--columns=41', '--to=plain', 'sample.txt'}} end,
  repl = empty,
}, 'pandoc.cli.')
args({
  all_extensions = function() return {'markdown'} end,
  default_extensions = function() return {'markdown'} end,
  extensions = function() return {'markdown+smart'} end,
  from_path = function() return {'sample.md'} end,
}, 'pandoc.format.')
args({
  decode = function() return {'{"a":[false,1.25],"b":"copper"}'} end,
  encode = function() return {{true, 2.25, 'é'}} end,
}, 'pandoc.json.')

local gif = 'GIF89a\3\0\2\0\128\0\0\0\0\0\255\255\255' ..
  '\44\0\0\0\0\3\0\2\0\0\2\2\132\95\0\59'
args({format = function() return {gif} end, size = function() return {gif} end}, 'pandoc.image.')

args({
  after_break = function() return {'>'} end,
  before_non_blank = function() return {layout()} end,
  bg = function() return {layout(), 'red'} end,
  blanklines = function() return {2} end,
  bold = function() return {layout()} end,
  braces = function() return {layout()} end,
  brackets = function() return {layout()} end,
  cblock = function() return {layout(), 8} end,
  chomp = function() return {layout() .. P.layout.cr} end,
  concat = function() return {{layout(), layout()}, P.layout.literal(':')} end,
  double_quotes = function() return {layout()} end,
  fg = function() return {layout(), 'blue'} end,
  flush = function() return {layout()} end,
  hang = function() return {layout(), 2, P.layout.literal('> ')} end,
  height = function() return {layout() .. P.layout.cr .. layout()} end,
  inside = function() return {layout(), P.layout.literal('<'), P.layout.literal('>')} end,
  is_empty = function() return {layout()} end,
  italic = function() return {layout()} end,
  lblock = function() return {layout(), 8} end,
  literal = function() return {'copper'} end,
  min_offset = function() return {layout()} end,
  nest = function() return {layout(), 2} end,
  nestle = function() return {P.layout.cr .. layout()} end,
  nowrap = function() return {layout()} end,
  offset = function() return {layout()} end,
  parens = function() return {layout()} end,
  prefixed = function() return {layout(), '> '} end,
  quotes = function() return {layout()} end,
  rblock = function() return {layout(), 8} end,
  real_length = function() return {'é界'} end,
  render = function() return {layout(), 9} end,
  strikeout = function() return {layout()} end,
  underlined = function() return {layout()} end,
  update_column = function() return {layout(), 3} end,
  vfill = function() return {'|'} end,
}, 'pandoc.layout.')

local function effect(name) return 'lua-api-effect ' .. name .. ' copper' end
args({
  info = function() return {effect('pandoc.log.info')} end,
  silence = function() return {function() P.log.warn(effect('pandoc.log.silence')) return 'copper' end} end,
  warn = function() return {effect('pandoc.log.warn')} end,
}, 'pandoc.log.')

local function with_item(call)
  P.mediabag.insert('sample.bin', 'application/octet-stream', 'copper')
  return call('sample.bin')
end
functions['pandoc.mediabag.delete'] = function(call)
  with_item(call)
  return P.mediabag.lookup('sample.bin')
end
functions['pandoc.mediabag.empty'] = function(call)
  P.mediabag.insert('sample.bin', 'application/octet-stream', 'copper')
  call()
  return P.mediabag.list()
end
functions['pandoc.mediabag.insert'] = function(call)
  call('sample.bin', 'application/octet-stream', 'copper')
  return P.mediabag.lookup('sample.bin')
end
functions['pandoc.mediabag.items'] = function(call)
  P.mediabag.insert('sample.bin', 'application/octet-stream', 'copper')
  local entries = {}
  for path, mime, contents in call() do entries[path] = {mime, contents} end
  return entries
end
for _, name in ipairs{'list', 'lookup', 'write'} do
  functions['pandoc.mediabag.' .. name] = function(call)
    P.mediabag.insert('sample.bin', 'application/octet-stream', 'copper')
    if name == 'write' then
      call('media', 'sample.bin')
      local file = assert(io.open('media/sample.bin', 'rb'))
      local content = file:read('a')
      file:close()
      return content
    end
    if name == 'lookup' then return call('sample.bin') end
    return call()
  end
end
args({
  fetch = sample,
  fill = function() return {P.Pandoc{P.Para{P.Image({word()}, 'sample.txt')}}} end,
  make_data_uri = function() return {'application/octet-stream', 'copper'} end,
}, 'pandoc.mediabag.')

args({
  directory = function() return {'a/b.txt'} end,
  exists = sample,
  filename = function() return {'a/b.txt'} end,
  is_absolute = function() return {'/a/b'} end,
  is_relative = function() return {'a/b'} end,
  join = function() return {{'a', 'b.txt'}} end,
  make_relative = function() return {'/a/b.txt', '/a', true} end,
  normalize = function() return {'a//b/./c/../'} end,
  split = function() return {'a/b.txt'} end,
  split_extension = function() return {'a/b.tar.gz'} end,
  split_search_path = function() return {'a:b::c'} end,
  treat_strings_as_paths = empty,
}, 'pandoc.path.')

args({
  make_sections = function() return {{heading(), para()}, {number_sections = true}} end,
  slide_level = function() return {{heading(), para()}} end,
  split_into_chunks = function() return {P.Pandoc{heading(), para()}, {chunk_level = 1}} end,
  table_of_contents = function() return {P.Pandoc{heading(), para()}} end,
  unique_identifier = function() return {{P.Str('Copper button!')}} end,
}, 'pandoc.structure.')

local function cwd() return P.system.get_working_directory() end
args({
  command = function() return {'/bin/cat', {}, 'copper'} end,
  copy = function() return {'sample.txt', 'copied.txt'} end,
  list_directory = function() return {'listed'} end,
  make_directory = function() return {'made/nested', true} end,
  read_file = sample,
  remove = function() return {'remove.txt'} end,
  remove_directory = function() return {'remove-dir', true} end,
  rename = function() return {'rename.txt', 'renamed.txt'} end,
  with_environment = function()
    return {{COPPER = 'blue'}, function() return os.getenv('COPPER') end}
  end,
  with_temporary_directory = function()
    return {'.', 'lua-probe', function(path) return type(path), #P.system.list_directory(path) end}
  end,
  with_working_directory = function()
    return {'.home', function() return cwd():sub(-6) end}
  end,
  write_file = function() return {'written.txt', 'copper'} end,
}, 'pandoc.system.')

functions['pandoc.system.cputime'] = function(call)
  local value = call()
  return type(value), value >= 0
end
functions['pandoc.system.environment'] = function(call)
  local env = call()
  return env.LANG, env.TZ, env.HOME:sub(-6)
end
functions['pandoc.system.get_working_directory'] = function(call)
  return call():sub(-5)
end
functions['pandoc.system.xdg'] = function(call)
  return call('config', 'probe'):sub(-14)
end
functions['pandoc.system.times'] = function(call)
  return (call('timed.txt'))
end

args({
  apply = function() return {P.template.compile('[$label$]'), {label = 'copper'}} end,
  get = sample,
  meta_to_context = function()
    return {{label = P.MetaInlines{word()}}, P.utils.stringify, P.utils.stringify}
  end,
}, 'pandoc.template.')

args({
  fromencoding = function() return {'caf\233', 'latin1'} end,
  len = function() return {'é界'} end,
  lower = function() return {'ÉCOLE'} end,
  reverse = function() return {'é界'} end,
  sub = function() return {'aé界b', 2, 3} end,
  subscript = function() return {'H2O'} end,
  superscript = function() return {'x2'} end,
  toencoding = function() return {'café', 'latin1'} end,
  upper = function() return {'école'} end,
}, 'pandoc.text.')
args({
  Sources = function() return {{{'sample.txt', 'copper'}}} end,
  Version = function() return {'3.13.2'} end,
}, 'pandoc.types.')

args({
  Version = function() return {{3, 13, 2}} end,
  blocks_to_inlines = function() return {{para(), para()}, {P.Str('|')}} end,
  citeproc = function() return {doc()} end,
  equals = function() return {word(), word()} end,
  from_simple_table = function()
    return {P.SimpleTable({word()}, {'AlignLeft'}, {0.5}, {{para()}}, {{{para()}}})}
  end,
  make_sections = function() return pack(true, 1, {heading(), para()}) end,
  normalize_date = function() return {'July 2, 2020'} end,
  references = function() return {doc()} end,
  run_json_filter = function() return {doc(), './identity', {}} end,
  run_lua_filter = function() return {doc(), 'inner.lua'} end,
  sha1 = function() return {'copper'} end,
  stringify = function() return {{P.Emph{word()}, P.Space(), P.Str('button')}} end,
  to_roman_numeral = function() return {49} end,
  to_simple_table = function()
    return {P.Table(P.Caption{para()}, {{'AlignLeft', 0.5}}, P.TableHead{}, {}, P.TableFoot{})}
  end,
  type = function() return {P.Inlines{word()}} end,
}, 'pandoc.utils.')

functions['pandoc.utils.documentation'] = function(call)
  local result = call(P.Str)
  return type(result), #result > 0
end
functions['pandoc.template.compile'] = function(call)
  return P.template.apply(call('[$label$]'), {label = 'copper'})
end
functions['pandoc.template.default'] = function(call)
  local result = call('plain')
  return type(result), result:find('$body$', 1, true) ~= nil
end

args({
  Archive = function() return {{P.zip.Entry('tile.txt', 'copper', 1700000000)}} end,
  Entry = function() return {'tile.txt', 'copper', 1700000000} end,
  read_entry = sample,
  zip = function() return {{'sample.txt'}} end,
}, 'pandoc.zip.')

functions['pandoc.zip'] = functions['pandoc.zip.zip']
functions['pandoc.scaffolding.Writer'] = function(call)
  local writer = P.scaffolding.Writer
  local inline, block = rawget(writer.Inline, 'Str'), rawget(writer.Block, 'Para')
  writer.Inline.Str = function(node) return node.text end
  writer.Block.Para = function(node) return node.content[1].text .. '!' end
  local result = pack(pcall(call, doc()))
  writer.Inline.Str, writer.Block.Para = inline, block
  if not result[1] then error(result[2], 0) end
  return unpack(result, 2, result.n)
end

args({
  assert = function() return {true, 'copper'} end,
  collectgarbage = function() return {'isrunning'} end,
  error = function() return {'copper', 0} end,
  getmetatable = function() return {setmetatable({}, {sample = true})} end,
  next = function() return {{copper = true}} end,
  pcall = function() return {identity, 'copper'} end,
  print = function() return {effect('print')} end,
  rawequal = function() return {'copper', 'copper'} end,
  rawget = function() return {{copper = true}, 'copper'} end,
  rawlen = function() return {{3, 4}} end,
  rawset = function() return {{}, 'copper', true} end,
  require = function() return {'pandoc.path'} end,
  select = function() return {2, 'blue', 'copper'} end,
  setmetatable = function() return {{copper = true}, {}} end,
  tonumber = function() return {'ff', 16} end,
  tostring = function() return {2.25} end,
  type = function() return {true} end,
  warn = function() return {effect('warn')} end,
  xpcall = function() return {function() error('copper', 0) end, identity} end,
})

for _, name in ipairs{'pairs', 'ipairs'} do
  functions[name] = function(call)
    local entries = {}
    for key, value in call({'copper', 'blue'}) do entries[key] = value end
    return entries
  end
end
functions.load = function(call) return assert(call('return 9'))() end
functions.loadfile = function(call) return assert(call('inner.lua'))()[1].Str(word()).text end
functions.dofile = function(call) return call('inner.lua')[1].Str(word()).text end

local function match(pattern) return lpeg.match(pattern, 'copper') end
args({
  match = function() return {lpeg.C(lpeg.P('cop')), 'copper'} end,
  pcode = function() return {lpeg.P('cop')} end,
  ptree = function() return {lpeg.P('cop')} end,
  setmaxstack = function() return {400} end,
  type = function() return {lpeg.P('cop')} end,
  version = empty,
}, 'lpeg.')

local patterns = {
  B = function(call) return lpeg.P('cop') * call(lpeg.P('cop')) * lpeg.C(lpeg.P('per')) end,
  C = function(call) return call(lpeg.P('cop')) end,
  Carg = function(call) return call(1) end,
  Cb = function(call) return lpeg.Cg(lpeg.C('cop'), 'stem') * call('stem') end,
  Cc = function(call) return call('blue', 3) end,
  Cf = function(call) return call(lpeg.Cc(2) * lpeg.Cc(3), function(a, b) return a + b end) end,
  Cg = function(call) return lpeg.Ct(call(lpeg.C('cop'), 'stem')) end,
  Cmt = function(call) return call(lpeg.P('cop'), function(_, pos) return pos, 'blue' end) end,
  Cp = function(call) return lpeg.P('cop') * call() end,
  Cs = function(call) return call(lpeg.P('cop') * (lpeg.P('per') / 'y')) end,
  Ct = function(call) return call(lpeg.C('cop') * lpeg.C('per')) end,
  P = function(call) return call('cop') end,
  R = function(call) return call('az') end,
  S = function(call) return call('bc') end,
  V = function(call) return lpeg.P{call('token'), token = lpeg.C('cop')} end,
  utfR = function(call) return call(97, 122) end,
}
for name, pattern in pairs(patterns) do
  functions['lpeg.' .. name] = function(call)
    return lpeg.match(pattern(call), 'copper', 1, 'blue')
  end
end
functions['lpeg.locale'] = function(call) return match(call().alpha) end
functions['re.compile'] = function(call) return match(call("{ 'cop' }")) end
args({
  find = function() return {'copper', "'per'"} end,
  gsub = function() return {'copper', "'per'", 'y'} end,
  match = function() return {'copper', "{ 'cop' }"} end,
  updatelocale = empty,
}, 're.')

local function name_type(value, mt)
  local kind = type(value)
  local name = type(mt) == 'table' and rawget(mt, '__name')
  return name and (kind .. ':' .. name) or kind
end

local function quote(value)
  return '"' .. value:gsub('\\', '\\\\'):gsub('[^ -~]', function(byte)
    return ('\\%03d'):format(byte:byte())
  end):gsub('"', '\\"') .. '"'
end

local function show(value, depth, active)
  local kind = type(value)
  if kind == 'string' then
    return quote(value)
  elseif kind == 'number' then
    return math.type(value) .. ':' .. ('%.17g'):format(value)
  elseif kind == 'nil' or kind == 'boolean' then
    return tostring(value)
  end
  local mt = debug.getmetatable(value)
  local label = name_type(value, mt)
  if active[value] or depth == 0 then return label end
  if kind == 'userdata' and mt then
    if rawget(mt, '__tostring') then
      local plain = tostring(value)
      local render = label == 'userdata:Doc' and P.layout.render
      if type(render) == 'function' then
        local ok, ansi = pcall(render, value, nil, 'ansi')
        if ok and type(ansi) == 'string' and ansi ~= plain then
          return label .. ':' .. quote(plain) .. ':' .. quote(ansi)
        end
      end
      return label .. ':' .. quote(plain)
    end
    if label == 'userdata:zip.Entry' then
      return label .. ':' .. show({value.path, value.modtime, value:contents()}, depth - 1, active)
    end
    if not rawget(mt, '__pairs') then return label end
  elseif kind ~= 'table' then
    return label
  end
  active[value] = true
  local fields = {}
  for key, item in pairs(value) do
    fields[#fields + 1] = show(key, depth - 1, active) .. '=' .. show(item, depth - 1, active)
  end
  table.sort(fields)
  active[value] = nil
  return label .. '{' .. table.concat(fields, ',') .. '}'
end

local function resolve(path)
  local value = _G
  for part in path:gmatch('[^.]+') do
    if value == nil then return nil end
    value = value[part]
  end
  return value
end

local function callable(value)
  if type(value) == 'function' then return true end
  local mt = getmetatable(value)
  return type(mt) == 'table' and rawget(mt, '__call') ~= nil
end

local library = {_G = true, coroutine = true, debug = true, io = true, math = true,
                 os = true, package = true, string = true, table = true, utf8 = true}

local function enumerate(selected, value, parent, depth)
  for key, field in pairs(value) do
    if type(key) == 'string' and key:match('^[%a_][%w_]*$') then
      local path = parent and (parent .. '.' .. key) or key
      selected[path] = true
      local next_depth = path == 'pandoc' and 1 or depth - 1
      if next_depth >= 0 and type(field) == 'table' and not library[path] then
        enumerate(selected, field, path, next_depth)
      end
    end
  end
end

local reader_samples = {
  csv = 'label,color\ncopper,blue\n',
  csljson = '[]',
  html = '<p>Copper</p>',
  ipynb = '{"cells":[],"metadata":{},"nbformat":4,"nbformat_minor":5}',
  json = '{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}',
  native = '[Para [Str "copper"]]',
  tsv = 'label\tcolor\ncopper\tblue\n',
}

local function writer_document(format)
  if format == 'ipynb' then
    return P.Pandoc{P.Div({P.Plain{word()}}, P.Attr('copper-cell', {'cell', 'markdown'}))}
  end
  if format == 'epub' or format == 'epub2' or format == 'epub3' then
    return P.Pandoc({para()}, {identifier = 'urn:uuid:2b04dd43-4203-477c-973a-ba5de0ad04a1'})
  end
  return doc()
end

local function call_probe(name, value)
  local probe = functions[name]
  if probe then
    return probe(value)
  elseif name:match('^pandoc%.readers%.') then
    local format = name:match('[^.]+$')
    local source = reader_samples[format] or 'copper'
    if type(value) == 'string' then return P.read(source, format) end
    return value(source)
  elseif name:match('^pandoc%.writers%.') then
    local format = name:match('[^.]+$')
    local document = writer_document(format)
    local result
    if type(value) == 'string' then result = P.write(document, format) else result = value(document) end
    if value == 'bytestring' then
      local file = assert(io.open('writer-probe/' .. format, 'wb'))
      assert(file:write(result))
      assert(file:close())
      return type(result)
    end
    return result
  end
  return value()
end

local function observe(name)
  local value = resolve(name)
  local kind = type(value)
  local registry = name:match('^pandoc%.readers%.') or name:match('^pandoc%.writers%.')
  if callable(value) or (registry and kind == 'string') then
    local results = pack(pcall(call_probe, name, value))
    local parts = {kind == 'string' and show(value, 1, {}) or kind}
    if not results[1] then return parts[1] .. '\terror\t' .. show(tostring(results[2]), 1, {}) end
    for index = 2, results.n do parts[#parts + 1] = show(results[index], 8, {}) end
    return table.concat(parts, '\t')
  end
  if kind == 'table' or kind == 'userdata' then
    if name == 'PANDOC_VERSION' or name == 'PANDOC_API_VERSION' then
      return kind .. '\t' .. quote(tostring(value))
    elseif name == 'PANDOC_STATE' then
      return kind .. '\t' .. show(value.input_files, 2, {})
    elseif name == 'PANDOC_READER_OPTIONS' then
      return kind .. '\t' .. show(value.tab_stop, 1, {})
    elseif name == 'PANDOC_WRITER_OPTIONS' then
      return kind .. '\t' .. show(value.columns, 1, {})
    elseif name:match('^pandoc%.layout%.') or name == 'pandoc.cli.default_options' then
      return kind .. '\t' .. show(value, 8, {})
    elseif name == 'pandoc.Block.constructor' or name == 'pandoc.Inline.constructor' then
      return kind .. '\t' .. show(value, 2, {})
    end
    return kind
  end
  return show(value, 1, {})
end

local function frame(action, name)
  local marker = '\030LuaProbe\t' .. action .. '\t' .. name .. '\031'
  for _, stream in ipairs{io.stdout, io.stderr} do
    stream:write(marker)
    stream:flush()
  end
end

local function run()
  local selected = {}
  for _, name in ipairs(names) do selected[name] = true end
  if discover then
    enumerate(selected, _G, nil, 1)
    selected['pandoc.cli.repl'] = nil
  end
  local ordered = {}
  for name in pairs(selected) do ordered[#ordered + 1] = name end
  table.sort(ordered)
  for _, name in ipairs(ordered) do
    frame('begin', name)
    local ok, result = pcall(observe, name)
    frame('end', name)
    if not ok then result = 'error\t' .. show(tostring(result), 1, {}) end
    io.stderr:write('lua-api\t', name, '\t', result, '\n')
  end
end

if standalone then
  run()
else
  return {{Pandoc = run}}
end
