Extensions = {smart = true, hard_line_breaks = false}

function Writer(doc, options)
  local active = {}
  for _, name in ipairs({'smart', 'hard_line_breaks'}) do
    if options.extensions:includes(name) then
      active[#active + 1] = name
    end
  end
  return string.format('%d blocks with extensions [%s]', #doc.blocks, table.concat(active, ','))
end
