local function label(name, text)
  return name .. '(' .. text .. ')'
end

function Doc(body)
  return label('document', body)
end

function Blocksep()
  return '|'
end

function Str(text)
  return text
end

function Space()
  return ' '
end

function Header(level, text)
  return label('heading ' .. level, text)
end

function Div(body)
  return label('division', body)
end

function CodeBlock(text)
  return label('code block', text)
end

function RawBlock(format, text)
  return label('raw block ' .. format, text)
end

function RawInline(format, text)
  return label('raw inline ' .. format, text)
end

function Link(text, target)
  return label('link ' .. target, text)
end

function Image(text, target)
  return label('image ' .. target, text)
end

CaptionedImage = Image

function InlineMath(text)
  return label('math', text)
end

function DisplayMath(text)
  return label('display math', text)
end

function BulletList(items)
  return label('bullets', table.concat(items, '|'))
end

function OrderedList(items)
  return label('numbers', table.concat(items, '|'))
end

function DefinitionList(items)
  local terms = {}
  for _, item in ipairs(items) do
    for term in pairs(item) do
      terms[#terms + 1] = term
    end
  end
  table.sort(terms)
  return label('definitions', table.concat(terms, '|'))
end

function Table(caption)
  return label('table', caption)
end

function Figure(caption)
  return label('figure', caption)
end

function HorizontalRule()
  return 'rule'
end

function LineBreak()
  return '\n'
end

SoftBreak = Space

for _, name in ipairs({
  'BlockQuote', 'Cite', 'Code', 'DoubleQuoted', 'Emph', 'LineBlock', 'Note',
  'Para', 'Plain', 'SingleQuoted', 'SmallCaps', 'Span', 'Strikeout', 'Strong',
  'Subscript', 'Superscript', 'Underline'
}) do
  _G[name] = function(text)
    if type(text) == 'table' then
      text = table.concat(text, '|')
    end
    return label(name, text)
  end
end
