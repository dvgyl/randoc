Writer = pandoc.scaffolding.Writer

local function kind(element)
  return '{' .. element.tag .. '}'
end

for _, name in ipairs({
  'BlockQuote', 'BulletList', 'CodeBlock', 'DefinitionList', 'Div', 'Figure',
  'Header', 'HorizontalRule', 'LineBlock', 'OrderedList', 'RawBlock', 'Table'
}) do
  Writer.Block[name] = kind
end

for _, name in ipairs({'Plain', 'Para'}) do
  Writer.Block[name] = function(element)
    return {'[', Writer.Inlines(element.content), ']'}
  end
end

for _, name in ipairs({
  'Cite', 'Code', 'Emph', 'Image', 'LineBreak', 'Link', 'Math', 'Note', 'Quoted',
  'RawInline', 'SmallCaps', 'SoftBreak', 'Span', 'Strikeout', 'Strong',
  'Subscript', 'Superscript', 'Underline'
}) do
  Writer.Inline[name] = kind
end

Writer.Inline.Str = function(element)
  return element.text
end

Writer.Inline.Space = ' '
