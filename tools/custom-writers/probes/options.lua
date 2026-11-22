function Writer(doc, options)
  return string.format('Blocks=%d, columns=%d, wrap=%s',
    #doc.blocks, options.columns, options.wrap_text)
end
