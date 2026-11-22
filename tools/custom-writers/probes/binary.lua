function ByteStringWriter(doc)
  return string.char(0, 255) .. string.pack('>I4', #doc.blocks)
end
