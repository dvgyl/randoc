-- The Lua part of the `pandoc` module of Randoc.
--
-- The chunk gets one argument: the table of Rust functions. Each Rust
-- function of `native.call` returns `true` and its results, or `false` and
-- an error message. The Lua wrapper raises the message as a string, because
-- a script must get a string and `mlua` raises a userdata value.
--
-- The list functions are a port of `cbits/listmod.c` of `hslua-list` 1.1.4
-- (MIT). The index functions of the objects are a port of `cbits/hslobj.c`
-- of `hslua-objectorientation` 2.5.0 (MIT).
local native = ...
local call, typename, cache = native.call, native.typename, native.cache
local userdata_name = native.userdata_name
local error, type, rawget, rawset, rawequal, select, tostring, setmetatable, getmetatable =
  error, type, rawget, rawset, rawequal, select, tostring, setmetatable, getmetatable
local xpcall, next, pairs, ipairs = xpcall, next, pairs, ipairs
local math_type, string_format = math.type, string.format

local function check(ok, ...)
  if ok then return ... end
  error((...), 0)
end

-- Returns a Lua function that runs the Rust operation `op`.
local function operation(op)
  return function(...) return check(call(op, ...)) end
end

-- The type names of the proxies whose metatable is hidden (`__metatable`).
local proxy_names = setmetatable({}, { __mode = 'k' })
local protected_names = {}

local function typed_name(value)
  if type(value) == 'userdata' then
    local name = userdata_name(value)
    if protected_names[name] then return name end
  end
end

-- The name of the type of a value, as `luaL_typeerror` shows it.
local function described(value)
  local name = typename(value) or proxy_names[value] or typed_name(value)
  if name then return name end
  local mt = getmetatable(value)
  if type(mt) == 'table' and type(rawget(mt, '__name')) == 'string' then
    return rawget(mt, '__name')
  end
  return type(value)
end

local function argerror(position, name, message)
  error(string_format("bad argument #%d to '%s' (%s)", position, name, message), 0)
end

local function checktable(value, position, name)
  if type(value) ~= 'table' then
    argerror(position, name, 'table expected, got ' .. described(value))
  end
end

local function checkcallable(value, position, name)
  if type(value) == 'function' then return end
  local mt = getmetatable(value)
  if type(mt) == 'table' and rawget(mt, '__call') ~= nil then return end
  argerror(position, name, 'function expected, got ' .. described(value))
end

local function optinteger(value, default, position, name)
  if value == nil then return default end
  if math_type(value) == 'integer' then return value end
  local number = math.tointeger(value)
  if number == nil then
    if type(value) == 'number' then
      argerror(position, name, 'number has no integer representation')
    end
    argerror(position, name, 'number expected, got ' .. described(value))
  end
  return number
end

local function posrelat(pos, len)
  if pos >= 0 then return pos end
  if -pos > len then return 0 end
  return len + pos + 1
end

--
-- List
--
-- `prefix` is the start of each function name in the message of a bad
-- argument: Lua finds the functions of `pandoc.List` in the table of
-- loaded modules and shows the full name.
local function new_list_metatable(name, prefix)
  local P = prefix or ''
  local List = { __name = name }
  List.__index = List

  function List.new(mt, items, state, control)
    if items == nil then
      items = {}
    elseif type(items) == 'function' then
      local result, i = {}, 0
      repeat
        control = items(state, control)
        i = i + 1
        rawset(result, i, control)
      until control == nil
      items = result
    else
      checktable(items, 2, P .. 'new')
    end
    return setmetatable(items, mt)
  end

  function List.at(list, index, default)
    index = optinteger(index, nil, 2, P .. 'at')
    if index == nil then argerror(2, P .. 'at', 'number expected, got no value') end
    local len = #list
    if index < -len or index > len then return default end
    if index < 0 then index = len + index + 1 end
    local value = rawget(list, index)
    if value == nil then return default end
    return value
  end

  function List.clone(list)
    checktable(list, 1, P .. 'clone')
    local result = setmetatable({}, getmetatable(list))
    for i = 1, #list do result[i] = list[i] end
    return result
  end

  function List.__concat(first, second)
    checktable(first, 1, P .. '__concat')
    checktable(second, 2, P .. '__concat')
    local len = #first
    local result = setmetatable({}, getmetatable(first))
    for i = 1, len do result[i] = first[i] end
    for i = 1, #second do result[len + i] = second[i] end
    return result
  end

  function List.__eq(first, second)
    local mt1, mt2 = getmetatable(first), getmetatable(second)
    if not (mt1 and mt2 and rawequal(mt1, mt2)) then return false end
    local len = #first
    if len ~= #second then return false end
    for i = 1, len do
      if first[i] ~= second[i] then return false end
    end
    return true
  end

  function List.extend(list, other)
    checktable(list, 1, P .. 'extend')
    checktable(other, 2, P .. 'extend')
    local len = #list
    for i = 1, #other do list[len + i] = other[i] end
    return list
  end

  function List.filter(list, predicate)
    checktable(list, 1, P .. 'filter')
    checkcallable(predicate, 2, P .. 'filter')
    local result = setmetatable({}, getmetatable(list) or native.metatables.List)
    local count = 0
    for i = 1, #list do
      if predicate(list[i], i) then
        count = count + 1
        result[count] = list[i]
      end
    end
    return result
  end

  function List.find(list, needle, init)
    checktable(list, 1, P .. 'find')
    local len = #list
    for i = posrelat(optinteger(init, 1, 3, P .. 'find'), len), len do
      local item = list[i]
      if needle == item then return item, i end
    end
    return nil
  end

  function List.find_if(list, predicate, init)
    checktable(list, 1, P .. 'find_if')
    checkcallable(predicate, 2, P .. 'find_if')
    local len = #list
    for i = posrelat(optinteger(init, 1, 3, P .. 'find_if'), len), len do
      if predicate(list[i], i) then return list[i], i end
    end
    return nil
  end

  function List.includes(list, needle, init)
    return (List.find(list, needle, init)) and true or false
  end

  function List.iter(list, step, start)
    checktable(list, 1, P .. 'iter')
    step = optinteger(step, 1, 2, P .. 'iter')
    local len = #list
    local i = optinteger(start, (step > 0 or len <= 0) and 1 or len, 3, P .. 'iter')
    if step == 0 then error('List.iter: step size must not be 0', 0) end
    return function()
      local item = list[i]
      i = i + step
      return item
    end
  end

  function List.map(list, fn)
    checktable(list, 1, P .. 'map')
    checkcallable(fn, 2, P .. 'map')
    local result = setmetatable({}, native.metatables.List)
    for i = 1, #list do result[i] = fn(list[i], i) end
    return result
  end

  function List.__tostring(list)
    local parts = {}
    for i = 1, #list do parts[i] = tostring(list[i]) end
    local mt = getmetatable(list)
    local prefix = (mt and rawget(mt, '__name') ~= nil) and (tostring(rawget(mt, '__name')) .. ' ') or ''
    return prefix .. '{' .. table.concat(parts, ', ') .. '}'
  end

  List.insert = table.insert
  List.remove = table.remove
  List.sort = table.sort
  return List
end

local List = new_list_metatable('List', 'pandoc.List.')
setmetatable(List, { __call = List.new })

local function new_element_list_metatable(name)
  local mt = new_list_metatable(name)
  local walk, clone, show = operation('list_walk'), operation('list_clone'), operation('list_show')
  mt.walk = function(list, filter) return walk(name, list, filter) end
  mt.clone = function(list) return clone(name, list) end
  mt.__tostring = function(list) return show(name, list) end
  local json = operation('json')
  mt.__tojson = function(list) return json('list_tojson', name, list) end
  return mt
end

native.metatables.List = List
native.metatables.Inlines = new_element_list_metatable('Inlines')
native.metatables.Blocks = new_element_list_metatable('Blocks')
native.metatables.Meta = { __name = 'Meta' }
native.metatables.CommonStateInterface = { __name = 'CommonStateInterface' }

--
-- Objects
--
local TYPES = native.types
local method = operation('method')
for type_name, spec in pairs(TYPES) do
  local methods = {}
  for _, name in ipairs(spec.method_names) do
    methods[name] = function(...) return method(type_name, name, ...) end
  end
  spec.methods = methods
end

local get = operation('get')
local attributes = operation('attributes')
local version = operation('version')

local function version_must_be_at_least(self, minimum, message)
  if version('compare', self, minimum) < 0 then
    message = message or 'expected version %s or newer, got %s'
    error(string_format(message, tostring(version('new', minimum)), tostring(self)), 0)
  end
end

local metamethods = {}

function metamethods.__index(self, key)
  local type_name = typename(self)
  if type_name == 'AttributeList' then return attributes('index', self, key) end
  if type_name == 'Version' then
    if key == 'must_be_at_least' then return version_must_be_at_least end
    return version('index', self, key)
  end
  local cached = cache(self)
  local value = rawget(cached, key)
  if value ~= nil then return value end
  local spec = TYPES[type_name]
  if spec.getters[key] then
    value = get(self, key)
    if value ~= nil then rawset(cached, key, value) end
    return value
  end
  local alias = spec.aliases[key]
  if alias then
    local object = self
    for i = 1, #alias do
      object = object[alias[i]]
      if object == nil then break end
    end
    return object
  end
  return spec.methods[key]
end

function metamethods.__newindex(self, key, value)
  local type_name = typename(self)
  if type_name == 'AttributeList' then return attributes('newindex', self, key, value) end
  if type_name == 'Version' then error('Cannot modify read-only object.', 0) end
  local spec = TYPES[type_name]
  local alias = spec.aliases[key]
  if alias then
    local object = self
    for i = 1, #alias - 1 do object = object[alias[i]] end
    if object == nil then error('attempt to index a nil value', 0) end
    object[alias[#alias]] = value
    return
  end
  if spec.setters[key] then
    rawset(cache(self), key, value)
    return
  end
  if spec.readonly[key] then
    error("'" .. key .. "' is a read-only property.", 0)
  end
  error('Cannot set unknown property.', 0)
end

local eq, show, concat = operation('eq'), operation('tostring'), operation('concat')

function metamethods.__eq(first, second) return eq(first, second) end

function metamethods.__tostring(self)
  if typename(self) == 'ListAttributes' then return string_format('ListAttributes: %p', self) end
  return show(self)
end

function metamethods.__concat(first, second)
  if typename(first) ~= 'Pandoc' and typename(second) ~= 'Pandoc' then
    local bad = (type(first) == 'string' or type(first) == 'number') and second or first
    error('attempt to concatenate a ' .. described(bad) .. ' value', 2)
  end
  return concat(first, second)
end

function metamethods.__len(self)
  local type_name = typename(self)
  if type_name == 'AttributeList' then return attributes('len', self) end
  if type_name == 'Version' then return version('len', self) end
  error('attempt to get length of a ' .. type_name .. ' value', 2)
end

function metamethods.__pairs(self)
  local type_name = typename(self)
  local items
  if type_name == 'AttributeList' then
    items = attributes('pairs', self)
    local i = 0
    return function()
      i = i + 1
      local item = items[i]
      if item then return item[1], item[2] end
    end
  elseif type_name == 'Version' then
    items = version('list', self)
    local i = 0
    return function()
      i = i + 1
      if items[i] ~= nil then return i, items[i] end
    end
  end
  -- The properties in the order of their names, then the methods. An
  -- element lists only the properties of its tag.
  local spec = TYPES[type_name]
  local element = type_name == 'Inline' or type_name == 'Block'
  local names, keys, values = {}, {}, {}
  for name in pairs(spec.getters) do names[#names + 1] = name end
  table.sort(names)
  for _, name in ipairs(names) do
    local value = self[name]
    if value ~= nil or not element then
      keys[#keys + 1] = name
      values[#keys] = value
    end
  end
  local methods = {}
  for _, name in ipairs(spec.method_names) do methods[#methods + 1] = name end
  table.sort(methods)
  for _, name in ipairs(methods) do
    keys[#keys + 1] = name
    values[#keys] = spec.methods[name]
  end
  local i = 0
  return function()
    i = i + 1
    if keys[i] ~= nil then return keys[i], values[i] end
  end, self, nil
end

local function compare(first, second, accept)
  if typename(first) ~= 'Version' and typename(second) ~= 'Version' then
    local a, b = described(first), described(second)
    if a == b then
      error('attempt to compare two ' .. a .. ' values', 3)
    end
    error('attempt to compare ' .. a .. ' with ' .. b, 3)
  end
  return accept(version('compare', first, second))
end

function metamethods.__lt(first, second)
  return compare(first, second, function(order) return order < 0 end)
end

function metamethods.__le(first, second)
  return compare(first, second, function(order) return order <= 0 end)
end

native.metamethods = metamethods

-- Calls `fn` and adds a traceback to the message of its error.
function native.call_traced(fn, ...)
  return xpcall(fn, native.traceback, ...)
end

--
-- The module `pandoc`
--
local pandoc = {}
local construct = operation('construct')
for _, name in ipairs(native.constructors) do
  pandoc[name] = function(...) return construct(name, ...) end
end
for _, name in ipairs(native.constants) do
  pandoc[name] = name
end
pandoc.List = List

-- The tables `pandoc.Block` and `pandoc.Inline`: each has a table of the
-- constructors of its kind, with new functions, as in Pandoc.
local INLINE_KINDS = {
  'Cite', 'Code', 'Emph', 'Image', 'LineBreak', 'Link', 'Math', 'Note', 'Quoted', 'RawInline',
  'SmallCaps', 'SoftBreak', 'Space', 'Span', 'Str', 'Strikeout', 'Strong', 'Subscript',
  'Superscript', 'Underline',
}
local BLOCK_KINDS = {
  'BlockQuote', 'BulletList', 'CodeBlock', 'DefinitionList', 'Div', 'Figure', 'Header',
  'HorizontalRule', 'LineBlock', 'OrderedList', 'Para', 'Plain', 'RawBlock', 'Table',
}
for kind, names in pairs { Inline = INLINE_KINDS, Block = BLOCK_KINDS } do
  local constructors = {}
  for _, name in ipairs(names) do
    constructors[name] = function(...) return construct(name, ...) end
  end
  pandoc[kind] = { constructor = constructors }
end

local function type_of(value)
  local name = typename(value) or proxy_names[value] or typed_name(value)
  if name then return name end
  local mt = getmetatable(value)
  if type(mt) == 'table' then
    local mt_name = rawget(mt, '__name')
    if type(mt_name) == 'string' then return mt_name end
  end
  return type(value)
end

local walk = operation('walk')
pandoc.walk_block = function(element, filter) return walk('Block', element, filter) end
pandoc.walk_inline = function(element, filter) return walk('Inline', element, filter) end

local new_version = function(specifier) return version('construct', specifier) end

pandoc.utils = {
  stringify = operation('stringify'),
  type = type_of,
  equals = function(first, second) return first == second end,
  Version = new_version,
}
pandoc.types = { Version = new_version }

local function missing(name)
  return name .. ' is not implemented in Randoc'
end

local function missing_function(name)
  return function() error(missing(name), 2) end
end

local utils_call = operation('utils')
for _, name in ipairs { 'blocks_to_inlines', 'make_sections', 'normalize_date', 'sha1', 'to_roman_numeral' } do
  pandoc.utils[name] = function(...) return utils_call(name, ...) end
end
pandoc.sha1 = pandoc.utils.sha1

-- `pandoc.path` and `pandoc.text`: ports of `hslua-module-path` 1.2.0 and
-- `hslua-module-text` 1.2.0 (MIT).
local function module_of(op, names)
  local run = operation(op)
  local module = {}
  for _, name in ipairs(names) do
    module[name] = function(...) return run(name, ...) end
  end
  return module, run
end
local path_run
pandoc.path, path_run = module_of('path', {
  'directory', 'filename', 'is_absolute', 'is_relative', 'join', 'make_relative', 'normalize',
  'split', 'split_extension', 'split_search_path',
})
pandoc.path.separator = '/'
pandoc.path.search_path_separator = ':'
function pandoc.path.treat_strings_as_paths()
  local string_mt = getmetatable('')
  string_mt.__add = function(...) return path_run('add_extension', ...) end
  string_mt.__div = function(...) return path_run('combine', ...) end
  for _, name in ipairs {
    'directory', 'filename', 'is_absolute', 'is_relative', 'normalize', 'split',
    'split_extension', 'split_search_path',
  } do
    string[name] = pandoc.path[name]
  end
end
-- `pandoc.system`: restricted mode has only the fields. In full mode Rust
-- calls `native.system_functions` with the function that runs each
-- operation of `system.rs` with the host.
local function system_os()
  local os_name, arch = native.os, native.arch
  return os_name, arch
end
pandoc.system = {}
pandoc.system.os, pandoc.system.arch = system_os()
local function refused(name)
  return function() error(name .. ' needs the system functions of full mode', 2) end
end
pandoc.pipe = refused('pandoc.pipe')
pandoc.path.exists = refused('pandoc.path.exists')
-- Mutable slots for scoped Rust callbacks updated on each `in_scope` call.
-- Defining the Lua wrappers once ensures scripts that cache `local read = pandoc.read`,
-- `local write = pandoc.write`, `local w = warn`, or `pandoc.system.*` at load time
-- never invoke a destructed scoped callback on a later `Reader`/`Writer`/`Template` call.
local active_report
local active_read
local active_citeproc
local active_compile
local document_installed = false
local active_write
local active_system
local active_host
local warn_installed = false
local convert_installed = false
local system_installed = false

-- `warn` of Lua, which Pandoc also reports as a scripting warning at the
-- position of the caller. `@on` and `@off` turn the text `Lua warning: ...`
-- of Lua itself on and off; a control message gives no report.
function native.install_warn(report)
  active_report = report
  if warn_installed then return end
  warn_installed = true
  local select, type, concat, on = select, type, table.concat, false
  function warn(...)
    local count = select('#', ...)
    for index = 1, math.max(count, 1) do
      local kind = index <= count and type((select(index, ...))) or 'no value'
      if kind ~= 'string' and kind ~= 'number' then
        error(("bad argument #%d to 'warn' (string expected, got %s)"):format(index, kind), 2)
      end
    end
    local message = concat({ ... })
    if count == 1 and message:sub(1, 1) == '@' then
      if message == '@on' then on = true elseif message == '@off' then on = false end
    else
      active_report(message, on)
    end
  end
end
-- `PANDOC_READER_OPTIONS` and `PANDOC_WRITER_OPTIONS`: a proxy with the
-- fields `names` in this order. Only a writable object takes a new value,
-- and only for a known field.
function native.options_object(name, values, names, writable)
  local known = {}
  for _, key in ipairs(names) do known[key] = true end
  local metatable = { __name = name, __metatable = true }
  function metatable.__index(_, key) return values[key] end
  function metatable.__newindex(_, key, value)
    if not writable then error('This ReaderOptions value is read-only.', 0) end
    if not known[key] then error('Cannot set unknown property.', 0) end
    values[key] = value
  end
  function metatable.__pairs()
    local index = 0
    return function()
      index = index + 1
      local key = names[index]
      if key ~= nil then return key, values[key] end
    end
  end
  if not writable then
    function metatable.__tostring()
      error('ReaderOptions expected, got ReaderOptions (read-only)\n' ..
        '\twhile retrieving function argument opts\n\twhile retrieving arguments for function ', 0)
    end
  end
  local proxy = setmetatable({}, metatable)
  proxy_names[proxy] = name
  return proxy
end
function native.convert_functions(read, write, readers, writers)
  active_read = read
  active_write = write
  pandoc.readers = readers
  pandoc.writers = writers
  if convert_installed then return end
  convert_installed = true
  pandoc.read = function(...) return check(active_read(...)) end
  pandoc.write = function(...) return check(active_write(...)) end
end
function native.document_functions(citeproc, compile_template)
  active_citeproc = citeproc
  active_compile = compile_template
  if document_installed then return end
  document_installed = true
  pandoc.utils.citeproc = function(...) return check(active_citeproc(...)) end
  pandoc.template.compile = function(...) return check(active_compile(...)) end
end
function native.system_functions(system_call)
  active_system = system_call
  if system_installed then return end
  system_installed = true
  local system = pandoc.system
  local pack, unpack, select, type, setmetatable = table.pack, table.unpack, select, type, setmetatable
  local function op(name) return function(...) return check(active_system(name, ...)) end end
  local filepath_list = {}
  for key, value in pairs(pandoc.List) do filepath_list[key] = value end
  filepath_list.__name = 'FilePath list'
  filepath_list.__index = filepath_list
  for _, name in ipairs {
    'command', 'copy', 'cputime', 'environment', 'get_working_directory', 'make_directory',
    'read_file', 'remove', 'remove_directory', 'rename', 'times', 'write_file',
  } do
    system[name] = op(name)
  end
  local list_directory, xdg = op('list_directory'), op('xdg')
  function system.list_directory(...) return setmetatable(list_directory(...), filepath_list) end
  function system.xdg(...)
    local result = xdg(...)
    if type(result) == 'table' then setmetatable(result, filepath_list) end
    return result
  end
  local set_directory, remove_tree = op('set_working_directory'), op('remove_tree')
  local enter_directory, enter_environment = op('with_working_directory'), op('with_environment')
  local leave_environment, make_temporary = op('leave_environment'), op('with_temporary_directory')
  -- Runs `callback` and then `restore`, also after an error of the callback.
  local function finally(restore, callback, ...)
    local results = pack(pcall(callback, ...))
    restore()
    if not results[1] then error(results[2], 0) end
    return unpack(results, 2, results.n)
  end
  function system.with_working_directory(...)
    local old = enter_directory(...)
    local directory, callback = ...
    return finally(function() set_directory(old) end, callback, directory)
  end
  -- Pandoc changes the environment of its process; Randoc keeps the
  -- replacement in Rust (`os.rs`). While it is active, `os.getenv` reads it,
  -- and `io.popen` and `os.execute` give it to the shell.
  local getenv_of = op('getenv')
  local shell_command = op('shell_command')
  local stock_getenv, stock_popen, stock_execute = os.getenv, io.popen, os.execute
  local function replaced_getenv(...)
    local name = ...
    if type(name) ~= 'string' and type(name) ~= 'number' then return stock_getenv(...) end
    return getenv_of(name)
  end
  local function replaced_popen(command, ...)
    if type(command) ~= 'string' then return stock_popen(command, ...) end
    return stock_popen(shell_command(command), ...)
  end
  local function replaced_execute(...)
    local command = ...
    if type(command) ~= 'string' then return stock_execute(...) end
    return stock_execute(shell_command(command))
  end
  function system.with_environment(...)
    enter_environment(...)
    local _, callback = ...
    local outer_getenv, outer_popen, outer_execute = os.getenv, io.popen, os.execute
    os.getenv, io.popen, os.execute = replaced_getenv, replaced_popen, replaced_execute
    return finally(function()
      leave_environment()
      os.getenv, io.popen, os.execute = outer_getenv, outer_popen, outer_execute
    end, callback)
  end
  function system.with_temporary_directory(...)
    local directory = make_temporary(...)
    local callback = select(select('#', ...) < 3 and 2 or 3, ...)
    return finally(function() remove_tree(directory) end, callback, directory)
  end
  pandoc.path.exists = op('exists')
  local pipe_error = {
    __name = 'pandoc pipe error',
    __tostring = function(e)
      local output = e.output == '' and '<no output>' or e.output
      return 'Error running ' .. e.command .. ' (error code ' .. e.error_code .. '): ' .. output
    end,
  }
  function pandoc.pipe(...)
    local command = ...
    local code, output = check(active_system('pipe', ...))
    if code then
      error(setmetatable({ command = command, error_code = code, output = output }, pipe_error), 0)
    end
    return output
  end
end
function native.host_functions(host)
  active_host = host
end

local function host_op(module, name)
  return function(...) return check(active_host(module, name, ...)) end
end

local function pure_op(module, name)
  return function(...) return check(call(module, name, ...)) end
end

local function operations(op, module, names)
  local result = {}
  for _, name in ipairs(names) do result[name] = op(module, name) end
  return result
end

local function typed(name, spec)
  local op = spec.host and host_op or pure_op
  local module = spec.module
  local methods = operations(op, module, spec.methods or {})
  local get = op(module, 'get')
  local mt = {
    __index = function(self, key) return methods[key] or get(self, key) end,
    __newindex = op(module, 'set'),
    __tostring = op(module, 'tostring'),
  }
  function mt.__pairs(self)
    local index = 0
    return function()
      index = index + 1
      local key = spec.pairs_order and spec.pairs_order[index]
      if key ~= nil then return key, mt.__index(self, key) end
    end
  end
  for _, key in ipairs(spec.ops or {}) do mt[key] = op(module, key) end
  native.typed[name] = mt
  protected_names[name] = true
end

local doc_methods = {
  'before_non_blank', 'braces', 'brackets', 'cblock', 'chomp', 'double_quotes',
  'flush', 'hang', 'height', 'inside', 'is_empty', 'lblock', 'min_offset', 'nest',
  'nestle', 'nowrap', 'offset', 'parens', 'prefixed', 'quotes', 'rblock', 'render',
  'update_column', 'vfill',
}
typed('ChunkedDoc', { module = 'structure', pairs_order = { 'chunks', 'meta', 'toc' } })
typed('Chunk', {
  module = 'structure',
  pairs_order = { 'contents', 'heading', 'id', 'level', 'next', 'number', 'path', 'prev',
    'section_number', 'unlisted', 'up' },
})
typed('Doc', {
  module = 'layout', methods = doc_methods, pairs_order = doc_methods,
  ops = { '__concat', '__add', '__div', '__idiv', '__eq' },
})
typed('LogMessage', { module = 'log', host = true, ops = { '__tojson' } })
typed('zip.Archive', {
  module = 'zip', host = true, methods = { 'bytestring', 'extract' },
  pairs_order = { 'entries', 'bytestring', 'extract' },
})
typed('zip.Entry', {
  module = 'zip', host = true, methods = { 'contents', 'symlink' },
  pairs_order = { 'modtime', 'path', 'contents', 'symlink' },
})
typed('SimpleTable', {
  module = 'simple_table', ops = { '__eq' },
  pairs_order = { 'aligns', 'caption', 'headers', 'rows', 't', 'widths' },
})

pandoc.structure = operations(pure_op, 'structure', {
  'make_sections', 'slide_level', 'unique_identifier', 'split_into_chunks', 'table_of_contents',
})
pandoc.mediabag = operations(host_op, 'mediabag', {
  'insert', 'lookup', 'list', 'delete', 'empty', 'make_data_uri', 'fetch', 'fill', 'write',
})
do
  local items = host_op('mediabag', 'items')
  function pandoc.mediabag.items()
    local snapshot = items()
    local function step(state)
      state.i = state.i + 1
      local entry = snapshot[state.i]
      if entry then return entry[1], entry[2], entry[3] end
    end
    return step, { i = 0 }, nil
  end
end
pandoc.log = operations(host_op, 'log', { 'info', 'warn' })
native.state_log = host_op('log', 'list')
do
  local hold, release = host_op('log', 'hold'), host_op('log', 'release')
  local pack, unpack, pcall = table.pack, table.unpack, pcall
  function pandoc.log.silence(fn, ...)
    if fn == nil then error('attempt to index a nil value', 0) end
    hold()
    local results = pack(pcall(fn, ...))
    local messages = release()
    if not results[1] then error(results[2], 0) end
    return messages, unpack(results, 2, results.n)
  end
end
pandoc.layout = operations(pure_op, 'layout', doc_methods)
for _, name in ipairs {
  'literal', 'after_break', 'blanklines', 'bold', 'italic', 'underlined', 'strikeout',
  'fg', 'bg', 'concat', 'real_length',
} do
  pandoc.layout[name] = pure_op('layout', name)
end
do
  local constants = operations(pure_op, 'layout', { 'empty', 'space', 'cr', 'blankline' })
  setmetatable(pandoc.layout, { __index = function(_, key)
    local constant = constants[key]
    if constant then return constant() end
  end })
end
pandoc.zip = operations(host_op, 'zip', { 'Archive', 'Entry', 'read_entry', 'zip' })
pandoc.image = operations(pure_op, 'image', { 'format', 'size' })
pandoc.SimpleTable = pure_op('simple_table', 'SimpleTable')
pandoc.utils.to_simple_table = pure_op('simple_table', 'to_simple_table')
pandoc.utils.from_simple_table = pure_op('simple_table', 'from_simple_table')
pandoc.utils.run_lua_filter = host_op('filters', 'run_lua_filter')
pandoc.utils.run_json_filter = host_op('filters', 'run_json_filter')
pandoc.utils.references = host_op('references', 'references')

pandoc.json = module_of('json', { 'decode', 'encode' })
pandoc.format = module_of('format', { 'all_extensions', 'default_extensions', 'extensions', 'from_path' })
pandoc.json.null = native.json_null
pandoc.text = module_of('text', {
  'fromencoding', 'len', 'lower', 'reverse', 'sub', 'subscript', 'superscript', 'toencoding',
  'upper',
})

--
-- The parts of the interface that this unit does not implement. A call
-- gives a Lua error, never a wrong result.
--
local function missing_module(name)
  local index = function(_, key) error(missing(name .. '.' .. tostring(key)), 2) end
  local iterate = function() error(missing(name), 2) end
  return setmetatable({}, { __index = index, __newindex = index, __call = index, __pairs = iterate })
end

for _, name in ipairs {
  'read', 'write', 'write_classic', 'with_state', 'ReaderOptions',
  'WriterOptions',
} do
  pandoc[name] = missing_function('pandoc.' .. name)
end
pandoc.utils.documentation = missing_function('pandoc.utils.documentation')
pandoc.types.Sources = missing_function('pandoc.types.Sources')
-- `pandoc.template`: `compile` and `apply` (row I-LUA-27). Compiled
-- templates are opaque `userdata` (`LuaTemplate`) preserving resolved partials.
local template_call = operation('template')
local doc_metatable = { __name = 'Doc' }
doc_metatable.__index = { render = function(doc) return rawget(doc, 'text') end }
doc_metatable.__tostring = function(doc) return rawget(doc, 'text') end
doc_metatable.__concat = function(a, b) return tostring(a) .. tostring(b) end
pandoc.template = {
  apply = function(template, context)
    return setmetatable({ text = template_call('apply', template, context) }, doc_metatable)
  end,
  default = missing_function('pandoc.template.default'),
  get = missing_function('pandoc.template.get'),
  meta_to_context = missing_function('pandoc.template.meta_to_context'),
}

local missing_modules = {
  'cli', 'scaffolding', 'readers', 'writers',
}
for _, name in ipairs(missing_modules) do
  pandoc[name] = missing_module('pandoc.' .. name)
end

local globals = _G
globals.pandoc = pandoc
-- Pandoc protects the metatable of each object with `__metatable = true`.
-- `mlua` does not permit that field, so the global function gives the
-- same result.
globals.getmetatable = function(...)
  if select('#', ...) == 0 then
    error("bad argument #1 to 'getmetatable' (value expected)", 2)
  end
  local value = ...
  if typename(value) or typed_name(value) then
    return true
  end
  return getmetatable(...)
end
globals.PANDOC_READER_OPTIONS = missing_module('PANDOC_READER_OPTIONS')
globals.PANDOC_WRITER_OPTIONS = missing_module('PANDOC_WRITER_OPTIONS')

local package = rawget(globals, 'package')
if type(package) == 'table' and type(package.loaded) == 'table' then
  local loaded = package.loaded
  loaded.pandoc = pandoc
  loaded['pandoc.List'] = List
  loaded['pandoc.utils'] = pandoc.utils
  loaded['pandoc.types'] = pandoc.types
  loaded['pandoc.path'] = pandoc.path
  loaded['pandoc.text'] = pandoc.text
  loaded['pandoc.json'] = pandoc.json
  loaded['pandoc.structure'] = pandoc.structure
  loaded['pandoc.format'] = pandoc.format
  loaded['pandoc.system'] = pandoc.system
  for _, name in ipairs { 'mediabag', 'log', 'layout', 'zip', 'image' } do
    loaded['pandoc.' .. name] = pandoc[name]
  end
  for _, name in ipairs(missing_modules) do
    if name ~= 'readers' and name ~= 'writers' then
      loaded['pandoc.' .. name] = pandoc[name]
    end
  end
  loaded.text = pandoc.text
end
