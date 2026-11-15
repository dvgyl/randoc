-- The fields of `PANDOC_READER_OPTIONS` and `PANDOC_WRITER_OPTIONS`, for the
-- matrix of `tools/options-matrix.py`.
local here = pandoc.system.get_working_directory()
local function show(value, depth)
  depth = depth or 0
  local kind = type(value)
  if kind == 'string' then
    return '"' .. value:gsub(here:gsub('%p', '%%%0'), 'CWD') .. '"'
  end
  if kind == 'table' and depth < 4 then
    local keys = {}
    for key in pairs(value) do keys[#keys + 1] = key end
    table.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
    local parts = {}
    for _, key in ipairs(keys) do parts[#parts + 1] = tostring(key) .. '=' .. show(value[key], depth + 1) end
    local mt = getmetatable(value)
    local name = type(mt) == 'table' and rawget(mt, '__name')
    return (name and (name .. ' ') or '') .. '{' .. table.concat(parts, ', ') .. '}'
  end
  if kind == 'userdata' then return show(tostring(value), depth) end
  return kind .. ':' .. tostring(value)
end
function Pandoc(document)
  local lines = {}
  for _, name in ipairs { 'PANDOC_READER_OPTIONS', 'PANDOC_WRITER_OPTIONS' } do
    local options = _G[name]
    lines[#lines + 1] = name .. ' ' .. pandoc.utils.type(options)
    for key, value in pairs(options) do
      if key == 'abbreviations' then
        local count = 0
        for _ in pairs(value) do count = count + 1 end
        value = 'count ' .. count .. ', Mr. ' .. tostring(value['Mr.'])
      elseif key == 'template' and value ~= nil then
        value = 'a template'
      elseif key == 'highlight_method' and type(value) ~= 'string' then
        value = 'a style'
      end
      lines[#lines + 1] = name:sub(8, 8) .. ' ' .. key .. ' = ' .. show(value)
    end
  end
  local text = table.concat(lines, '\n')
  local file = os.getenv('OPTIONS_DUMP') and io.open(os.getenv('OPTIONS_DUMP'), 'w')
  if file then file:write(text) file:close() end
  document.meta.options = text
  return document
end
