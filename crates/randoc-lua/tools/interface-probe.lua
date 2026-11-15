-- Lists the Lua interface of a filter: each global, each field of the
-- module `pandoc` and each field of a table in it. One line for each name:
-- PATH, TAB, KIND. KIND is the Lua type, `stub` for a function or table
-- that only raises "is not implemented in Randoc", or a scalar value.
-- `interface-table.py` runs it with both programs.
local lines = {}
local seen = {}

local function is_stub(value)
  if type(value) == 'function' then
    local info = debug.getinfo(value, 'S')
    if info.source ~= '=[randoc]' then return false end
    for i = 1, 8 do
      local name = debug.getupvalue(value, i)
      if name == nil then return false end
      if name == 'missing' then return true end
    end
    return false
  end
  if type(value) == 'table' then
    local ok, message = pcall(pairs, value)
    return not ok and tostring(message):find('is not implemented in Randoc', 1, true) ~= nil
  end
  return false
end

local function kind(value)
  if is_stub(value) then return 'stub' end
  local t = type(value)
  if t == 'string' then
    return 'string ' .. (#value <= 40 and value:gsub('[%c\\]', '?') or ('length ' .. #value))
  elseif t == 'boolean' then
    return 'boolean ' .. tostring(value)
  elseif t == 'number' then
    return math.type(value) .. ' ' .. tostring(value)
  end
  return t
end

local function keys(tbl)
  local result = {}
  local ok = pcall(function()
    for key in pairs(tbl) do
      if type(key) == 'string' then result[#result + 1] = key end
    end
  end)
  if not ok then return {} end
  table.sort(result)
  return result
end

local function add(path, value)
  lines[#lines + 1] = path .. '\t' .. kind(value)
end

local function walk(path, tbl, depth)
  if seen[tbl] then return end
  seen[tbl] = true
  for _, key in ipairs(keys(tbl)) do
    local value = rawget(tbl, key)
    if value == nil then value = tbl[key] end
    local child = path .. '.' .. key
    add(child, value)
    if type(value) == 'table' and depth > 1 and not is_stub(value) then
      walk(child, value, depth - 1)
    end
  end
end

local libraries = {
  coroutine = true, debug = true, io = true, math = true, os = true,
  package = true, string = true, table = true, utf8 = true, _G = true,
}
for _, name in ipairs(keys(_G)) do
  local value = _G[name]
  add(name, value)
  if type(value) == 'table' and not libraries[name] and not is_stub(value) then
    walk(name, value, name == 'pandoc' and 2 or 1)
  end
end

function Pandoc(doc)
  io.stderr:write(table.concat(lines, '\n'), '\n')
  return doc
end
