function Writer(doc)
  local layout = pandoc.layout
  local rows = {}
  for index, block in ipairs(doc.blocks) do
    rows[#rows + 1] = index .. '. ' .. block.tag
  end
  return layout.render(layout.concat(rows, layout.cr))
end
