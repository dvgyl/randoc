function Template()
  return 'Document $identifier$\n===\n$body$\n===\nEnd of document'
end

function Writer(doc, options)
  local fields = {
    body = string.format('%d top-level blocks', #doc.blocks),
    identifier = pandoc.utils.stringify(doc.meta.identifier or 'unlabelled')
  }
  return pandoc.layout.render(pandoc.template.apply(options.template, fields))
end
