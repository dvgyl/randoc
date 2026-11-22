function Writer(doc)
  error(string.format('Block index unavailable for %d blocks', #doc.blocks), 0)
end
