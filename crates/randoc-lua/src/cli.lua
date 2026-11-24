-- Option values for scripts that use the Pandoc command line interface.
local json = pandoc.json
local function default_options(array)
  local options = {}
  for _, key in ipairs {
    "bibliography",
    "css",
    "epub-fonts",
    "filters",
    "include-after-body",
    "include-before-body",
    "include-in-header",
    "indented-code-classes",
    "metadata-files",
    "number-offset",
    "pdf-engine-opts",
    "request-headers",
    "syntax-definitions",
    "typst-inputs",
  } do options[key] = array {} end
  options["resource-path"] = array { "." }
  options.metadata, options.variables = {}, {}
  options["ascii"] = false
  options["dump-args"] = false
  options["embed-resources"] = false
  options["fail-if-warnings"] = false
  options["file-scope"] = false
  options["html-q-tags"] = false
  options["ignore-args"] = false
  options["incremental"] = false
  options["link-images"] = false
  options["list-of-figures"] = false
  options["list-of-tables"] = false
  options["list-tables"] = false
  options["no-check-certificate"] = false
  options["number-sections"] = false
  options["preserve-tabs"] = false
  options["reference-links"] = false
  options["sandbox"] = false
  options["section-divs"] = false
  options["self-contained"] = false
  options["setext-headers"] = false
  options["standalone"] = false
  options["strip-comments"] = false
  options["table-of-contents"] = false
  options["trace"] = false
  options["cite-method"] = "citeproc"
  options["columns"] = 72.0
  options["default-image-extension"] = ""
  options["identifier-prefix"] = ""
  options["dpi"] = 96.0
  options["email-obfuscation"] = "none"
  options["eol"] = "native"
  options["epub-subdirectory"] = "EPUB"
  options["epub-title-page"] = true
  options["figure-caption-position"] = "below"
  options["ipynb-output"] = "IpynbOutputBest"
  options["math-method"] = "mathml"
  options["reference-location"] = "end-of-document"
  options["shift-heading-level-by"] = 0.0
  options["split-level"] = 1.0
  options["syntax-highlighting"] = "default"
  options["tab-stop"] = 4.0
  options["table-caption-position"] = "above"
  options["toc-depth"] = 3.0
  options["top-level-division"] = "top-level-default"
  options["track-changes"] = "accept-changes"
  options["verbosity"] = "WARNING"
  options["wrap"] = "wrap-auto"
  return options
end

local aliases = {
  f='from', r='from', reader='from', t='to', w='to', writer='to', o='output',
  s='standalone', N='number-sections', i='incremental', V='variable', M='metadata',
  H='include-in-header', B='include-before-body', A='include-after-body',
  F='filter', L='lua-filter', c='css', p='preserve-tabs', C='citeproc',
  toc='table-of-contents', lof='list-of-figures', lot='list-of-tables',
  ['epub-chapter-level']='split-level', ['id-prefix']='identifier-prefix',
}
local names = {
  output='output-file', ['reference-doc']='reference-doc', template='template',
  from='from', to='to', ['metadata-file']='metadata-files', ['pdf-engine-opt']='pdf-engine-opts',
  ['epub-embed-font']='epub-fonts', ['syntax-definition']='syntax-definitions',
}
local choices = {
  wrap={auto='wrap-auto', none='wrap-none', preserve='wrap-preserve'},
  ['track-changes']={accept='accept-changes', reject='reject-changes', all='all-changes'},
  ['reference-location']={block='end-of-block', section='end-of-section', document='end-of-document'},
  ['top-level-division']={part='top-level-part', chapter='top-level-chapter',
    section='top-level-section', default='top-level-default'},
  ['ipynb-output']={all='IpynbOutputAll', best='IpynbOutputBest', none='IpynbOutputNone'},
}

local function parse_error(message)
  error('Cannot process info option: OptError (PandocOptionError ' .. json.encode(message) .. ')', 0)
end

local function add(values, value)
  values[#values + 1] = value
end

local function pair(text, missing)
  local key, value = text:match('^([^:=]+)[:=](.*)$')
  return key or text, value or missing
end

local cli = {}
cli.default_options = default_options(function(items) return items end)

function cli.parse_options(args)
  if type(args) ~= 'table' then error('table expected, got ' .. type(args), 0) end
  local options = default_options(pandoc.List)
  local inputs = pandoc.List()
  local index, positional = 1, false
  while index <= #args do
    local arg = args[index]
    if type(arg) == 'number' then arg = tostring(arg) end
    if type(arg) ~= 'string' then error('string expected, got ' .. type(arg), 0) end
    if positional or arg == '-' or arg:sub(1, 1) ~= '-' then
      add(inputs, arg)
    elseif arg == '--' then
      positional = true
    else
      local flag, attached
      if arg:sub(1, 2) == '--' then
        flag, attached = arg:match('^%-%-([^=]+)=(.*)$')
        flag = flag or arg:sub(3)
      else
        flag = arg:sub(2, 2)
        if #arg > 2 then attached = arg:sub(3) end
      end
      local original = flag
      flag = aliases[flag] or flag
      local key = names[flag] or flag
      local current = options[key]
      local function value()
        if attached ~= nil then return attached end
        index = index + 1
        if type(args[index]) ~= 'string' then
          parse_error('option `' .. arg .. "' requires an argument\nTry  --help for more information.")
        end
        return args[index]
      end
      if flag == 'help' or flag == 'h' then
        error('Cannot process info option: Help', 0)
      elseif flag == 'version' or flag == 'v' then
        error('Cannot process info option: Version', 0)
      elseif flag == 'quiet' or flag == 'verbose' then
        options.verbosity = flag == 'quiet' and 'ERROR' or 'INFO'
      elseif flag == 'filter' or flag == 'lua-filter' then
        local kind = flag == 'filter' and 'json' or 'lua'
        add(options.filters, { path=value(), type=kind })
      elseif flag == 'variable' or flag == 'metadata' then
        local field, text = pair(value(), 'true')
        if flag == 'variable' then
          local previous = options.variables[field]
          if type(previous) == 'table' then
            add(previous, text)
          elseif previous ~= nil then
            local list = pandoc.List()
            add(list, previous)
            add(list, text)
            options.variables[field] = list
          else
            options.variables[field] = text
          end
        else
          local kind, item = 'MetaString', text
          if text == 'true' or text == 'false' then kind, item = 'MetaBool', text == 'true' end
          options.metadata[field] = { t=kind, c=item }
        end
      elseif flag == 'resource-path' then
        local paths = pandoc.List()
        local sep = pandoc.path.search_path_separator
        for path in (value() .. sep):gmatch('(.-)' .. sep) do add(paths, path == '' and '.' or path) end
        for _, path in ipairs(current) do add(paths, path) end
        options[key] = paths
      elseif flag == 'markdown-headings' then
        options['setext-headers'] = value() == 'setext'
      elseif flag == 'number-offset' or flag == 'indented-code-classes' then
        for item in value():gmatch('[^,]+') do
          if flag == 'number-offset' then
            local number = math.tointeger(tonumber(item))
            if not number then parse_error('Argument of --number-offset must be a list of integers') end
            options['number-sections'] = true
            item = number
          end
          add(current, item)
        end
      elseif flag == 'natbib' or flag == 'biblatex' then
        options['cite-method'] = flag
      elseif flag == 'citeproc' then
        add(options.filters, {type='citeproc'})
      elseif flag == 'no-highlight' then
        options['syntax-highlighting'] = 'none'
      elseif type(current) == 'boolean' and attached == nil then
        options[key] = true
      elseif type(current) == 'number' then
        local number = math.tointeger(tonumber(value()))
        if not number or (number < 1 and key ~= 'shift-heading-level-by') then
          parse_error('Argument of --' .. key .. ' must be a number greater than 0')
        end
        options[key] = number + 0.0
      elseif choices[key] then
        local choice = choices[key][value()]
        if not choice then parse_error('Invalid argument for --' .. key) end
        options[key] = choice
      elseif type(current) == 'table' and key ~= 'variables' and key ~= 'filters' then
        add(current, value())
      elseif names[flag] or type(current) == 'string' then
        options[key] = value()
      else
        local prefix = arg:sub(1, 2) == '--' and '--' or '-'
        parse_error('Unknown option ' .. prefix .. original .. '.\nTry  --help for more information.')
      end
    end
    index = index + 1
  end
  if #inputs ~= 0 then options['input-files'] = inputs end
  return options
end
return cli
