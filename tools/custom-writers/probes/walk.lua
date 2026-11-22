function Writer(doc)
  local lengths = {}
  doc:walk({
    Str = function(element)
      lengths[#lengths + 1] = utf8.len(element.text)
    end
  })
  return 'Word lengths: ' .. table.concat(lengths, ' ')
end
