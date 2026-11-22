function Writer(doc)
  local tags = {}
  for _, block in ipairs(doc.blocks) do
    tags[#tags + 1] = block.tag
  end
  return string.format('%d blocks [%s]', #tags, table.concat(tags, ','))
end
