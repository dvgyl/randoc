# The Lua interface of Pandoc in Randoc

`tools/interface-table.py` writes this file from a run of
`tools/interface-probe.lua` with Pandoc 3.13 and with Randoc. Do not
edit it. A name is equal when Randoc has it with the same kind (the
Lua type, and the value of a short scalar), different when the kind
differs, and absent when Randoc does not have it or raises
"is not implemented in Randoc". The matrices of `tests/replay`
compare the behaviour.

Names of Pandoc: 468. Equal 279, different 2, absent 187.
Names only in Randoc: 48.

| name | state |
| --- | --- |
| `FORMAT` | equal |
| `PANDOC_API_VERSION` | equal |
| `PANDOC_READER_OPTIONS` | different |
| `PANDOC_SCRIPT_FILE` | equal |
| `PANDOC_STATE` | equal |
| `PANDOC_VERSION` | equal |
| `PANDOC_WRITER_OPTIONS` | different |
| `_G` | equal |
| `_VERSION` | equal |
| `assert` | equal |
| `collectgarbage` | equal |
| `coroutine` | equal |
| `debug` | equal |
| `dofile` | equal |
| `error` | equal |
| `getmetatable` | equal |
| `io` | equal |
| `ipairs` | equal |
| `load` | equal |
| `loadfile` | equal |
| `lpeg` | equal |
| `lpeg.B` | equal |
| `lpeg.C` | equal |
| `lpeg.Carg` | equal |
| `lpeg.Cb` | equal |
| `lpeg.Cc` | equal |
| `lpeg.Cf` | equal |
| `lpeg.Cg` | equal |
| `lpeg.Cmt` | equal |
| `lpeg.Cp` | equal |
| `lpeg.Cs` | equal |
| `lpeg.Ct` | equal |
| `lpeg.P` | equal |
| `lpeg.R` | equal |
| `lpeg.S` | equal |
| `lpeg.V` | equal |
| `lpeg.locale` | equal |
| `lpeg.match` | equal |
| `lpeg.pcode` | equal |
| `lpeg.ptree` | equal |
| `lpeg.setmaxstack` | equal |
| `lpeg.type` | equal |
| `lpeg.utfR` | equal |
| `lpeg.version` | equal |
| `math` | equal |
| `next` | equal |
| `os` | equal |
| `package` | equal |
| `pairs` | equal |
| `pandoc` | equal |
| `pandoc.AlignCenter` | equal |
| `pandoc.AlignDefault` | equal |
| `pandoc.AlignLeft` | equal |
| `pandoc.AlignRight` | equal |
| `pandoc.Attr` | equal |
| `pandoc.AttributeList` | equal |
| `pandoc.AuthorInText` | equal |
| `pandoc.Block` | equal |
| `pandoc.Block.constructor` | equal |
| `pandoc.BlockQuote` | equal |
| `pandoc.Blocks` | equal |
| `pandoc.BulletList` | equal |
| `pandoc.Caption` | equal |
| `pandoc.Cell` | equal |
| `pandoc.Citation` | equal |
| `pandoc.Cite` | equal |
| `pandoc.Code` | equal |
| `pandoc.CodeBlock` | equal |
| `pandoc.Decimal` | equal |
| `pandoc.DefaultDelim` | equal |
| `pandoc.DefaultStyle` | equal |
| `pandoc.DefinitionList` | equal |
| `pandoc.DisplayMath` | equal |
| `pandoc.Div` | equal |
| `pandoc.DoubleQuote` | equal |
| `pandoc.Emph` | equal |
| `pandoc.Example` | equal |
| `pandoc.Figure` | equal |
| `pandoc.Header` | equal |
| `pandoc.HorizontalRule` | equal |
| `pandoc.Image` | equal |
| `pandoc.Inline` | equal |
| `pandoc.Inline.constructor` | equal |
| `pandoc.InlineMath` | equal |
| `pandoc.Inlines` | equal |
| `pandoc.LineBlock` | equal |
| `pandoc.LineBreak` | equal |
| `pandoc.Link` | equal |
| `pandoc.List` | equal |
| `pandoc.List.__concat` | equal |
| `pandoc.List.__eq` | equal |
| `pandoc.List.__index` | equal |
| `pandoc.List.__name` | equal |
| `pandoc.List.__tostring` | equal |
| `pandoc.List.at` | equal |
| `pandoc.List.clone` | equal |
| `pandoc.List.extend` | equal |
| `pandoc.List.filter` | equal |
| `pandoc.List.find` | equal |
| `pandoc.List.find_if` | equal |
| `pandoc.List.includes` | equal |
| `pandoc.List.insert` | equal |
| `pandoc.List.iter` | equal |
| `pandoc.List.map` | equal |
| `pandoc.List.new` | equal |
| `pandoc.List.remove` | equal |
| `pandoc.List.sort` | equal |
| `pandoc.ListAttributes` | equal |
| `pandoc.LowerAlpha` | equal |
| `pandoc.LowerRoman` | equal |
| `pandoc.Math` | equal |
| `pandoc.Meta` | equal |
| `pandoc.MetaBlocks` | equal |
| `pandoc.MetaBool` | equal |
| `pandoc.MetaInlines` | equal |
| `pandoc.MetaList` | equal |
| `pandoc.MetaMap` | equal |
| `pandoc.MetaString` | equal |
| `pandoc.NormalCitation` | equal |
| `pandoc.Note` | equal |
| `pandoc.OneParen` | equal |
| `pandoc.OrderedList` | equal |
| `pandoc.Pandoc` | equal |
| `pandoc.Para` | equal |
| `pandoc.Period` | equal |
| `pandoc.Plain` | equal |
| `pandoc.Quoted` | equal |
| `pandoc.RawBlock` | equal |
| `pandoc.RawInline` | equal |
| `pandoc.ReaderOptions` | absent |
| `pandoc.Row` | equal |
| `pandoc.SimpleTable` | absent |
| `pandoc.SingleQuote` | equal |
| `pandoc.SmallCaps` | equal |
| `pandoc.SoftBreak` | equal |
| `pandoc.Space` | equal |
| `pandoc.Span` | equal |
| `pandoc.Str` | equal |
| `pandoc.Strikeout` | equal |
| `pandoc.Strong` | equal |
| `pandoc.Subscript` | equal |
| `pandoc.Superscript` | equal |
| `pandoc.SuppressAuthor` | equal |
| `pandoc.Table` | equal |
| `pandoc.TableBody` | equal |
| `pandoc.TableFoot` | equal |
| `pandoc.TableHead` | equal |
| `pandoc.TwoParens` | equal |
| `pandoc.Underline` | equal |
| `pandoc.UpperAlpha` | equal |
| `pandoc.UpperRoman` | equal |
| `pandoc.WriterOptions` | absent |
| `pandoc.cli` | absent |
| `pandoc.cli.default_options` | absent |
| `pandoc.cli.parse_options` | absent |
| `pandoc.cli.repl` | absent |
| `pandoc.format` | equal |
| `pandoc.format.all_extensions` | equal |
| `pandoc.format.default_extensions` | equal |
| `pandoc.format.extensions` | equal |
| `pandoc.format.from_path` | equal |
| `pandoc.image` | absent |
| `pandoc.image.format` | absent |
| `pandoc.image.size` | absent |
| `pandoc.json` | equal |
| `pandoc.json.decode` | equal |
| `pandoc.json.encode` | equal |
| `pandoc.json.null` | equal |
| `pandoc.layout` | absent |
| `pandoc.layout.after_break` | absent |
| `pandoc.layout.before_non_blank` | absent |
| `pandoc.layout.bg` | absent |
| `pandoc.layout.blankline` | absent |
| `pandoc.layout.blanklines` | absent |
| `pandoc.layout.bold` | absent |
| `pandoc.layout.braces` | absent |
| `pandoc.layout.brackets` | absent |
| `pandoc.layout.cblock` | absent |
| `pandoc.layout.chomp` | absent |
| `pandoc.layout.concat` | absent |
| `pandoc.layout.cr` | absent |
| `pandoc.layout.double_quotes` | absent |
| `pandoc.layout.empty` | absent |
| `pandoc.layout.fg` | absent |
| `pandoc.layout.flush` | absent |
| `pandoc.layout.hang` | absent |
| `pandoc.layout.height` | absent |
| `pandoc.layout.inside` | absent |
| `pandoc.layout.is_empty` | absent |
| `pandoc.layout.italic` | absent |
| `pandoc.layout.lblock` | absent |
| `pandoc.layout.literal` | absent |
| `pandoc.layout.min_offset` | absent |
| `pandoc.layout.nest` | absent |
| `pandoc.layout.nestle` | absent |
| `pandoc.layout.nowrap` | absent |
| `pandoc.layout.offset` | absent |
| `pandoc.layout.parens` | absent |
| `pandoc.layout.prefixed` | absent |
| `pandoc.layout.quotes` | absent |
| `pandoc.layout.rblock` | absent |
| `pandoc.layout.real_length` | absent |
| `pandoc.layout.render` | absent |
| `pandoc.layout.space` | absent |
| `pandoc.layout.strikeout` | absent |
| `pandoc.layout.underlined` | absent |
| `pandoc.layout.update_column` | absent |
| `pandoc.layout.vfill` | absent |
| `pandoc.log` | absent |
| `pandoc.log.info` | absent |
| `pandoc.log.silence` | absent |
| `pandoc.log.warn` | absent |
| `pandoc.mediabag` | absent |
| `pandoc.mediabag.delete` | absent |
| `pandoc.mediabag.empty` | absent |
| `pandoc.mediabag.fetch` | absent |
| `pandoc.mediabag.fill` | absent |
| `pandoc.mediabag.insert` | absent |
| `pandoc.mediabag.items` | absent |
| `pandoc.mediabag.list` | absent |
| `pandoc.mediabag.lookup` | absent |
| `pandoc.mediabag.make_data_uri` | absent |
| `pandoc.mediabag.write` | absent |
| `pandoc.path` | equal |
| `pandoc.path.directory` | equal |
| `pandoc.path.exists` | equal |
| `pandoc.path.filename` | equal |
| `pandoc.path.is_absolute` | equal |
| `pandoc.path.is_relative` | equal |
| `pandoc.path.join` | equal |
| `pandoc.path.make_relative` | equal |
| `pandoc.path.normalize` | equal |
| `pandoc.path.search_path_separator` | equal |
| `pandoc.path.separator` | equal |
| `pandoc.path.split` | equal |
| `pandoc.path.split_extension` | equal |
| `pandoc.path.split_search_path` | equal |
| `pandoc.path.treat_strings_as_paths` | equal |
| `pandoc.pipe` | equal |
| `pandoc.read` | equal |
| `pandoc.readers` | equal |
| `pandoc.readers.asciidoc` | absent |
| `pandoc.readers.biblatex` | absent |
| `pandoc.readers.bibtex` | absent |
| `pandoc.readers.bits` | absent |
| `pandoc.readers.commonmark` | equal |
| `pandoc.readers.commonmark_x` | equal |
| `pandoc.readers.creole` | absent |
| `pandoc.readers.csljson` | equal |
| `pandoc.readers.csv` | absent |
| `pandoc.readers.djot` | absent |
| `pandoc.readers.docbook` | absent |
| `pandoc.readers.docx` | absent |
| `pandoc.readers.dokuwiki` | absent |
| `pandoc.readers.endnotexml` | absent |
| `pandoc.readers.epub` | absent |
| `pandoc.readers.fb2` | absent |
| `pandoc.readers.fodt` | absent |
| `pandoc.readers.gfm` | equal |
| `pandoc.readers.haddock` | absent |
| `pandoc.readers.html` | equal |
| `pandoc.readers.ipynb` | absent |
| `pandoc.readers.jats` | absent |
| `pandoc.readers.jira` | absent |
| `pandoc.readers.json` | equal |
| `pandoc.readers.latex` | equal |
| `pandoc.readers.man` | absent |
| `pandoc.readers.markdown` | equal |
| `pandoc.readers.markdown_github` | equal |
| `pandoc.readers.markdown_mmd` | equal |
| `pandoc.readers.markdown_phpextra` | equal |
| `pandoc.readers.markdown_strict` | equal |
| `pandoc.readers.mdoc` | absent |
| `pandoc.readers.mediawiki` | absent |
| `pandoc.readers.muse` | absent |
| `pandoc.readers.native` | equal |
| `pandoc.readers.odt` | absent |
| `pandoc.readers.opml` | absent |
| `pandoc.readers.org` | absent |
| `pandoc.readers.pod` | absent |
| `pandoc.readers.pptx` | absent |
| `pandoc.readers.ris` | absent |
| `pandoc.readers.rst` | absent |
| `pandoc.readers.rtf` | absent |
| `pandoc.readers.t2t` | absent |
| `pandoc.readers.textile` | absent |
| `pandoc.readers.tikiwiki` | absent |
| `pandoc.readers.tsv` | absent |
| `pandoc.readers.twiki` | absent |
| `pandoc.readers.typst` | absent |
| `pandoc.readers.vimwiki` | absent |
| `pandoc.readers.xlsx` | absent |
| `pandoc.readers.xml` | absent |
| `pandoc.scaffolding` | absent |
| `pandoc.scaffolding.Writer` | absent |
| `pandoc.sha1` | equal |
| `pandoc.structure` | equal |
| `pandoc.structure.make_sections` | equal |
| `pandoc.structure.slide_level` | equal |
| `pandoc.structure.split_into_chunks` | absent |
| `pandoc.structure.table_of_contents` | absent |
| `pandoc.structure.unique_identifier` | equal |
| `pandoc.system` | equal |
| `pandoc.system.arch` | equal |
| `pandoc.system.command` | equal |
| `pandoc.system.copy` | equal |
| `pandoc.system.cputime` | equal |
| `pandoc.system.environment` | equal |
| `pandoc.system.get_working_directory` | equal |
| `pandoc.system.list_directory` | equal |
| `pandoc.system.make_directory` | equal |
| `pandoc.system.os` | equal |
| `pandoc.system.read_file` | equal |
| `pandoc.system.remove` | equal |
| `pandoc.system.remove_directory` | equal |
| `pandoc.system.rename` | equal |
| `pandoc.system.times` | equal |
| `pandoc.system.with_environment` | equal |
| `pandoc.system.with_temporary_directory` | equal |
| `pandoc.system.with_working_directory` | equal |
| `pandoc.system.write_file` | equal |
| `pandoc.system.xdg` | equal |
| `pandoc.template` | absent |
| `pandoc.template.apply` | absent |
| `pandoc.template.compile` | absent |
| `pandoc.template.default` | absent |
| `pandoc.template.get` | absent |
| `pandoc.template.meta_to_context` | absent |
| `pandoc.text` | equal |
| `pandoc.text.fromencoding` | equal |
| `pandoc.text.len` | equal |
| `pandoc.text.lower` | equal |
| `pandoc.text.reverse` | equal |
| `pandoc.text.sub` | equal |
| `pandoc.text.subscript` | equal |
| `pandoc.text.superscript` | equal |
| `pandoc.text.toencoding` | equal |
| `pandoc.text.upper` | equal |
| `pandoc.types` | equal |
| `pandoc.types.Sources` | absent |
| `pandoc.types.Version` | equal |
| `pandoc.utils` | equal |
| `pandoc.utils.Version` | equal |
| `pandoc.utils.blocks_to_inlines` | equal |
| `pandoc.utils.citeproc` | absent |
| `pandoc.utils.documentation` | absent |
| `pandoc.utils.equals` | equal |
| `pandoc.utils.from_simple_table` | absent |
| `pandoc.utils.make_sections` | equal |
| `pandoc.utils.normalize_date` | equal |
| `pandoc.utils.references` | absent |
| `pandoc.utils.run_json_filter` | absent |
| `pandoc.utils.run_lua_filter` | absent |
| `pandoc.utils.sha1` | equal |
| `pandoc.utils.stringify` | equal |
| `pandoc.utils.to_roman_numeral` | equal |
| `pandoc.utils.to_simple_table` | absent |
| `pandoc.utils.type` | equal |
| `pandoc.walk_block` | equal |
| `pandoc.walk_inline` | equal |
| `pandoc.with_state` | absent |
| `pandoc.write` | equal |
| `pandoc.write_classic` | absent |
| `pandoc.writers` | equal |
| `pandoc.writers.ansi` | absent |
| `pandoc.writers.asciidoc` | absent |
| `pandoc.writers.asciidoc_legacy` | absent |
| `pandoc.writers.asciidoctor` | absent |
| `pandoc.writers.bbcode` | absent |
| `pandoc.writers.bbcode_fluxbb` | absent |
| `pandoc.writers.bbcode_hubzilla` | absent |
| `pandoc.writers.bbcode_phpbb` | absent |
| `pandoc.writers.bbcode_steam` | absent |
| `pandoc.writers.bbcode_xenforo` | absent |
| `pandoc.writers.beamer` | equal |
| `pandoc.writers.biblatex` | absent |
| `pandoc.writers.bibtex` | absent |
| `pandoc.writers.chunkedhtml` | absent |
| `pandoc.writers.commonmark` | equal |
| `pandoc.writers.commonmark_x` | equal |
| `pandoc.writers.context` | absent |
| `pandoc.writers.csljson` | equal |
| `pandoc.writers.djot` | absent |
| `pandoc.writers.docbook` | absent |
| `pandoc.writers.docbook4` | absent |
| `pandoc.writers.docbook5` | absent |
| `pandoc.writers.docx` | absent |
| `pandoc.writers.dokuwiki` | absent |
| `pandoc.writers.dzslides` | absent |
| `pandoc.writers.epub` | absent |
| `pandoc.writers.epub2` | absent |
| `pandoc.writers.epub3` | absent |
| `pandoc.writers.fb2` | absent |
| `pandoc.writers.fodt` | absent |
| `pandoc.writers.gfm` | equal |
| `pandoc.writers.haddock` | absent |
| `pandoc.writers.html` | equal |
| `pandoc.writers.html4` | equal |
| `pandoc.writers.html5` | equal |
| `pandoc.writers.icml` | absent |
| `pandoc.writers.ipynb` | absent |
| `pandoc.writers.jats` | absent |
| `pandoc.writers.jats_archiving` | absent |
| `pandoc.writers.jats_articleauthoring` | absent |
| `pandoc.writers.jats_publishing` | absent |
| `pandoc.writers.jira` | absent |
| `pandoc.writers.json` | equal |
| `pandoc.writers.latex` | equal |
| `pandoc.writers.man` | absent |
| `pandoc.writers.markdown` | equal |
| `pandoc.writers.markdown_github` | equal |
| `pandoc.writers.markdown_mmd` | equal |
| `pandoc.writers.markdown_phpextra` | equal |
| `pandoc.writers.markdown_strict` | equal |
| `pandoc.writers.markua` | equal |
| `pandoc.writers.mediawiki` | absent |
| `pandoc.writers.ms` | absent |
| `pandoc.writers.muse` | absent |
| `pandoc.writers.native` | equal |
| `pandoc.writers.odt` | absent |
| `pandoc.writers.opendocument` | absent |
| `pandoc.writers.opml` | absent |
| `pandoc.writers.org` | absent |
| `pandoc.writers.plain` | equal |
| `pandoc.writers.pptx` | absent |
| `pandoc.writers.revealjs` | absent |
| `pandoc.writers.rst` | absent |
| `pandoc.writers.rtf` | absent |
| `pandoc.writers.s5` | absent |
| `pandoc.writers.slideous` | absent |
| `pandoc.writers.slidy` | absent |
| `pandoc.writers.t2t` | absent |
| `pandoc.writers.tei` | absent |
| `pandoc.writers.texinfo` | absent |
| `pandoc.writers.textile` | absent |
| `pandoc.writers.typst` | absent |
| `pandoc.writers.vimdoc` | absent |
| `pandoc.writers.xml` | absent |
| `pandoc.writers.xwiki` | absent |
| `pandoc.writers.zimwiki` | absent |
| `pandoc.zip` | absent |
| `pandoc.zip.Archive` | absent |
| `pandoc.zip.Entry` | absent |
| `pandoc.zip.read_entry` | absent |
| `pandoc.zip.zip` | absent |
| `pcall` | equal |
| `print` | equal |
| `rawequal` | equal |
| `rawget` | equal |
| `rawlen` | equal |
| `rawset` | equal |
| `re` | equal |
| `re.compile` | equal |
| `re.find` | equal |
| `re.gsub` | equal |
| `re.match` | equal |
| `re.updatelocale` | equal |
| `require` | equal |
| `select` | equal |
| `setmetatable` | equal |
| `string` | equal |
| `table` | equal |
| `tonumber` | equal |
| `tostring` | equal |
| `type` | equal |
| `utf8` | equal |
| `warn` | equal |
| `xpcall` | equal |

Names only in Randoc:

- `PANDOC_READER_OPTIONS.abbreviations`
- `PANDOC_READER_OPTIONS.columns`
- `PANDOC_READER_OPTIONS.default_image_extension`
- `PANDOC_READER_OPTIONS.extensions`
- `PANDOC_READER_OPTIONS.indented_code_classes`
- `PANDOC_READER_OPTIONS.standalone`
- `PANDOC_READER_OPTIONS.strip_comments`
- `PANDOC_READER_OPTIONS.tab_stop`
- `PANDOC_READER_OPTIONS.track_changes`
- `PANDOC_WRITER_OPTIONS.chunk_template`
- `PANDOC_WRITER_OPTIONS.cite_method`
- `PANDOC_WRITER_OPTIONS.columns`
- `PANDOC_WRITER_OPTIONS.dpi`
- `PANDOC_WRITER_OPTIONS.email_obfuscation`
- `PANDOC_WRITER_OPTIONS.epub_chapter_level`
- `PANDOC_WRITER_OPTIONS.epub_fonts`
- `PANDOC_WRITER_OPTIONS.epub_metadata`
- `PANDOC_WRITER_OPTIONS.epub_subdirectory`
- `PANDOC_WRITER_OPTIONS.epub_title_page`
- `PANDOC_WRITER_OPTIONS.extensions`
- `PANDOC_WRITER_OPTIONS.figure_caption_position`
- `PANDOC_WRITER_OPTIONS.highlight_method`
- `PANDOC_WRITER_OPTIONS.html_math_method`
- `PANDOC_WRITER_OPTIONS.html_q_tags`
- `PANDOC_WRITER_OPTIONS.identifier_prefix`
- `PANDOC_WRITER_OPTIONS.incremental`
- `PANDOC_WRITER_OPTIONS.link_images`
- `PANDOC_WRITER_OPTIONS.list_of_figures`
- `PANDOC_WRITER_OPTIONS.list_of_tables`
- `PANDOC_WRITER_OPTIONS.list_tables`
- `PANDOC_WRITER_OPTIONS.number_offset`
- `PANDOC_WRITER_OPTIONS.number_sections`
- `PANDOC_WRITER_OPTIONS.prefer_ascii`
- `PANDOC_WRITER_OPTIONS.reference_doc`
- `PANDOC_WRITER_OPTIONS.reference_links`
- `PANDOC_WRITER_OPTIONS.reference_location`
- `PANDOC_WRITER_OPTIONS.section_divs`
- `PANDOC_WRITER_OPTIONS.setext_headers`
- `PANDOC_WRITER_OPTIONS.slide_level`
- `PANDOC_WRITER_OPTIONS.split_level`
- `PANDOC_WRITER_OPTIONS.tab_stop`
- `PANDOC_WRITER_OPTIONS.table_caption_position`
- `PANDOC_WRITER_OPTIONS.table_of_contents`
- `PANDOC_WRITER_OPTIONS.template`
- `PANDOC_WRITER_OPTIONS.toc_depth`
- `PANDOC_WRITER_OPTIONS.top_level_division`
- `PANDOC_WRITER_OPTIONS.variables`
- `PANDOC_WRITER_OPTIONS.wrap_text`
