#!/usr/bin/env python3
"""Writes the oracle matrix of `pandoc.read` and `pandoc.write`.

Usage (from the root of the worktree, after `. env.sh`):

    python3 -I crates/randoc-lua/tools/convert-matrix.py
    python3 -I crates/randoc-lua/tools/record-replay.py convert

The script writes `crates/randoc-lua/tests/replay/convert.txt` in the format
of `modules.txt`. The matrix has each reader and each writer of the
registry of the program (`crates/randoc/src/formats.rs`): format strings
with extensions, reader and writer options as a table and as the globals,
and the errors for a wrong format name, a wrong option name and a wrong
argument type. The test `the_convert_matrix_has_each_registered_format`
compares the two name lines at the top of the file with the registry.
The replay test runs the matrix in full mode and in restricted mode.
"""

import importlib.util
import io
import os
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
OUTPUT = os.path.join(HERE, "..", "tests", "replay", "convert.txt")

_spec = importlib.util.spec_from_file_location("module_matrix", os.path.join(HERE, "module-matrix.py"))
_matrix = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_matrix)
DOCS, HELPER, FOOTER, lua_string = _matrix.DOCS, _matrix.HELPER, _matrix.FOOTER, _matrix.lua_string

READERS = [
    "commonmark", "commonmark_x", "creole", "csljson", "docx", "dokuwiki", "fodt", "gfm", "html",
    "jira", "json", "latex", "markdown",
    "markdown_github", "markdown_mmd", "markdown_phpextra", "markdown_strict", "mediawiki",
    "native", "odt", "org", "rst", "rtf",
]
WRITERS = [
    "beamer", "commonmark", "commonmark_x", "csljson", "dokuwiki", "fodt", "gfm", "html", "html4",
    "html5", "jira", "json",
    "latex", "markdown", "markdown_github", "markdown_mmd", "markdown_phpextra",
    "markdown_strict", "markua", "mediawiki", "native", "odt", "opendocument", "org", "plain", "rst", "rtf",
]

MARKDOWN = (
    "# A head {#top}\n\nSome *emph*, **strong**, \"quotes\" -- and --- dashes...\n"
    "a_b_c ~~gone~~ <b>raw</b> https://example.org and $x^2$ and Mr. Smith.\n\n"
    "    indented code\n\n"
    "| a | b |\n|---|---|\n| 1 | 2 |\n\n"
    "- [ ] task\n- item[^1]\n\n[^1]: The note.\n\n<!-- hidden -->\n\n"
    "![image](picture)\n"
)
HTML = (
    '<h1 id="x">Head</h1><p>Some <em>emph</em> &amp; "q" -- d</p><!-- c -->'
    '<div class="c">in <span>div</span></div><table><tr><td>1</td></tr></table>'
)
LATEX = (
    "\\section{Head}\\label{h}\nSome \\emph{emph} ``q'' -- dash \\foo{x} $a$\n\n"
    "\\begin{itemize}\\item one\\end{itemize}\n% comment\n"
)
JSON = DOCS["small"]
RST = (
    "A heading\n=========\n\n"
    "Some *emphasis*, **strong text**, and `a link <https://example.org>`__.\n\n"
    "::\n\n    sample\n\n"
    ".. note::\n\n   A note.\n\n"
    ">>> print(1)\n"
)
NATIVE = '[Header 1 ("h",[],[]) [Str "H"],Para [Emph [Str "a"],Space,Str "b"]]'
CSLJSON = (
    '[{"id":"k","type":"book","title":"The title","author":[{"family":"Fam","given":"Giv"}],'
    '"issued":{"date-parts":[[2020,3]]}}]'
)


def docx():
    """A small DOCX package with fixed dates, so the bytes never change:
    a paragraph with italic and bold text, an insertion and a deletion."""
    w = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'
    change = 'w:author="A" w:date="2020-01-01T00:00:00Z"'
    parts = {
        "[Content_Types].xml":
            '<?xml version="1.0" encoding="UTF-8"?>'
            '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
            '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
            '<Default Extension="xml" ContentType="application/xml"/>'
            '<Override PartName="/word/document.xml" ContentType='
            '"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
            '</Types>',
        "_rels/.rels":
            '<?xml version="1.0" encoding="UTF-8"?>'
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" Type='
            '"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument"'
            ' Target="word/document.xml"/></Relationships>',
        "word/document.xml":
            f'<?xml version="1.0" encoding="UTF-8"?><w:document {w}><w:body>'
            '<w:p><w:r><w:t xml:space="preserve">Some </w:t></w:r>'
            '<w:r><w:rPr><w:i/></w:rPr><w:t>emph</w:t></w:r>'
            '<w:r><w:t xml:space="preserve"> and </w:t></w:r>'
            '<w:r><w:rPr><w:b/></w:rPr><w:t>strong</w:t></w:r>'
            f'<w:ins w:id="1" {change}><w:r><w:t xml:space="preserve"> new</w:t></w:r></w:ins>'
            f'<w:del w:id="2" {change}><w:r><w:delText xml:space="preserve"> old</w:delText></w:r></w:del>'
            '<w:r><w:t>.</w:t></w:r></w:p>'
            '<w:p><w:r><w:t>Second paragraph.</w:t></w:r></w:p>'
            '</w:body></w:document>',
    }
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", zipfile.ZIP_STORED) as archive:
        for name, text in parts.items():
            archive.writestr(zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0)), text)
    return buffer.getvalue()


OFFICE_NS = 'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"'
TEXT_NS = 'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"'
ODF_BODY = '<office:body><office:text><text:p>OpenDocument</text:p></office:text></office:body>'
FODT = f'<office:document {OFFICE_NS} {TEXT_NS}><office:styles/>{ODF_BODY}</office:document>'


def odt():
    """A small ODT with a stored MIME type and fixed dates."""
    parts = {
        "mimetype": "application/vnd.oasis.opendocument.text",
        "content.xml":
            f'<office:document-content {OFFICE_NS} {TEXT_NS}>{ODF_BODY}</office:document-content>',
        "styles.xml": f'<office:document-styles {OFFICE_NS}><office:styles/></office:document-styles>',
    }
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", zipfile.ZIP_STORED) as archive:
        for name, content in parts.items():
            archive.writestr(zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0)), content)
    return buffer.getvalue()


INPUT = {"csljson": CSLJSON, "docx": docx(), "html": HTML, "json": JSON, "latex": LATEX,
         "native": NATIVE, "fodt": FODT, "odt": odt(), "rst": RST,
         "creole": "= Head =\n\nSome //emph// and **strong**.\n\n* item\n",
         "dokuwiki": "====== Head ======\n\nSome //emph// and **strong**.\n\n  * item\n",
         "jira": "h1. Head\n\nSome _emph_ and *strong*.\n\n* item\n",
         "mediawiki": "== Head ==\n\nSome ''emph'' and '''strong'''.\n\n* item\n",
         "org": "* Head\n\nSome /emph/ and *strong*.\n\n- item\n",
         "rtf": r"{\rtf1\ansi Some {\i emph} and {\b strong}.\par}"}

# Extension changes for each reader, each one supported by its format.
READER_SPECS = {
    "creole": ["+east_asian_line_breaks"],
    "dokuwiki": ["-smart", "+raw_html", "+tex_math_dollars"],
    "jira": ["+east_asian_line_breaks"],
    "mediawiki": ["-auto_identifiers", "+smart"],
    "org": ["-auto_identifiers", "+smart", "-task_lists"],
    "rtf": ["+east_asian_line_breaks"],
    "commonmark": ["+smart", "+pipe_tables+strikeout", "+autolink_bare_uris", "-raw_html"],
    "commonmark_x": ["-smart", "-pipe_tables", "-task_lists"],
    "gfm": ["-autolink_bare_uris", "+smart", "-raw_html", "-strikeout"],
    "html": ["-native_divs", "-native_spans", "+raw_html", "-line_blocks", "+smart"],
    "latex": ["+raw_tex", "-smart", "-latex_macros"],
    "markdown": ["-smart", "-pipe_tables", "-raw_html", "-footnotes", "+autolink_bare_uris",
                 "-tex_math_dollars", "-task_lists", "-strikeout", "+hard_line_breaks"],
    "markdown_github": ["-autolink_bare_uris"],
    "markdown_mmd": ["+smart", "-pipe_tables"],
    "markdown_phpextra": ["+smart", "-footnotes"],
    "markdown_strict": ["+smart", "+pipe_tables", "+footnotes"],
    "native": [],
    "json": [],
    "csljson": [],
    "docx": ["+empty_paragraphs", "+native_numbering"],
    "fodt": ["+empty_paragraphs", "-auto_identifiers"],
    "odt": ["+empty_paragraphs", "-auto_identifiers"],
    "rst": ["-auto_identifiers", "+literate_haskell", "+smart"],
}

READER_OPTIONS = [
    "{columns = 20}",
    "{tab_stop = 2}",
    "{tab_stop = 8}",
    "{indented_code_classes = {'lua', 'x'}}",
    "{default_image_extension = 'png'}",
    "{strip_comments = true}",
    "{standalone = true}",
    "{abbreviations = {['Mr.'] = true}}",
    "{abbreviations = {}}",
    "{track_changes = 'all-changes'}",
    "PANDOC_READER_OPTIONS",
]

DOC = (
    "P.Pandoc({H(1, 'Head one', P.Attr('top')), "
    "P.Para{P.Emph't', P.Space(), P.Str'\"q\"', P.Space(), P.Link('link', 'https://example.org', 'T'), "
    "P.Space(), P.Link('mail', 'mailto:me@example.org'), P.Space(), P.Code('c'), P.Space(), "
    "P.Note{P.Para'note'}, P.Space(), P.Math('InlineMath', 'x^2'), P.Space(), "
    "P.Quoted('DoubleQuote', 'qq'), P.Space(), P.Strikeout's', P.Space(), "
    "P.Str'a long line of words that the writer wraps at the column limit of the options'}, "
    "H(2, 'Head two'), P.BulletList{{P.Plain'one'}, {P.Plain'two'}}, "
    "P.CodeBlock('code', P.Attr('', {'nolang'})), P.HorizontalRule(), "
    "P.Div(P.Para'in div', P.Attr('d', {'c'})), "
    "P.Table(P.Caption{P.Plain'cap'}, {{P.AlignDefault, P.ColWidthDefault}}, P.TableHead{}, "
    "{{attr = P.Attr(), body = {P.Row{P.Cell'cell'}}, head = {}, row_head_columns = 0}}, P.TableFoot{})}, "
    "{title = P.Inlines'The title', references = {{id = 'k', type = 'book', title = 'T'}}})"
)

# Use a common wiki document for options probes. The full document still
# tests known differences in links, divisions and code blocks.
WIKI_DOC = (
    "P.Pandoc({H(1, 'Head one', P.Attr('top')), "
    "P.Para{P.Emph't', P.Space(), P.Str'\"q\"', P.Space(), P.Link('link', 'https://example.org', 'T'), "
    "P.Space(), P.Note{P.Para'note'}, P.Space(), P.Math('InlineMath', 'x^2'), P.Space(), "
    "P.Str'a long line of words that the writer wraps at the column limit of the options'}, "
    "H(2, 'Head two'), P.BulletList{{P.Plain'one'}, {P.Plain'two'}}}, "
    "{title = P.Inlines'The title'})"
)


# Extension changes for each writer, each one supported by its format.
WRITER_SPECS = {
    "dokuwiki": ["-smart", "+raw_html", "+tex_math_dollars"],
    "jira": ["+east_asian_line_breaks"],
    "mediawiki": ["-auto_identifiers", "+smart"],
    "org": ["-auto_identifiers", "+smart", "-task_lists"],
    "rtf": ["+east_asian_line_breaks"],
    "beamer": ["-smart", "+raw_tex"],
    "commonmark": ["+smart", "-raw_html", "+pipe_tables"],
    "commonmark_x": ["-smart", "-footnotes", "-pipe_tables"],
    "csljson": [],
    "gfm": ["-pipe_tables", "+smart", "-strikeout"],
    "html": ["-smart", "+ascii_identifiers", "-native_divs", "-auto_identifiers"],
    "html4": ["+smart", "-native_divs"],
    "html5": ["-smart", "+east_asian_line_breaks"],
    "json": [],
    "latex": ["-smart", "+raw_tex", "-auto_identifiers"],
    "markdown": ["-smart", "-pipe_tables", "-footnotes", "-tex_math_dollars", "-strikeout",
                 "-fenced_divs", "+hard_line_breaks", "-header_attributes"],
    "markdown_github": ["-pipe_tables"],
    "markdown_mmd": ["-pipe_tables", "+smart"],
    "markdown_phpextra": ["-footnotes", "+smart"],
    "markdown_strict": ["+pipe_tables", "+footnotes", "+smart"],
    "markua": ["-smart"],
    "native": [],
    "plain": ["-smart", "+gutenberg"],
    "fodt": ["+empty_paragraphs", "+native_numbering"],
    "odt": ["+empty_paragraphs", "+native_numbering"],
    "opendocument": ["+empty_paragraphs", "+native_numbering", "+xrefs_name"],
    "rst": ["-auto_identifiers", "+literate_haskell", "+smart"],
}

# Not `email_obfuscation = 'javascript'`: its output is a script of Pandoc
# (rule 10 of the session 5 rules), so the matrix does not store it.
WRITER_OPTIONS = [
    "{columns = 30}",
    "{wrap_text = 'wrap-none'}",
    "{wrap_text = 'wrap-preserve'}",
    "{reference_links = true}",
    "{reference_location = 'end-of-section'}",
    "{setext_headers = true}",
    "{number_sections = true}",
    "{section_divs = true}",
    "{identifier_prefix = 'p-'}",
    "{html_q_tags = true}",
    "{email_obfuscation = 'references'}",
    "{top_level_division = 'top-level-chapter'}",
    "{tab_stop = 2}",
    "{prefer_ascii = true}",
    "{slide_level = 1}",
    "{incremental = true}",
    "{list_tables = true}",
    "{table_of_contents = true}",
    "{number_offset = {3, 1}}",
    "{dpi = 300}",
    "PANDOC_WRITER_OPTIONS",
]


def read_probes(name):
    text = lua_string(INPUT.get(name, MARKDOWN))
    probes = [(f"read {name}", f"return P.read({text}, {lua_string(name)})")]
    for spec in READER_SPECS[name]:
        probes.append((f"read {name}{spec}", f"return P.read({text}, {lua_string(name + spec)})"))
    return probes


def read_option_probes(name):
    text = lua_string(INPUT.get(name, MARKDOWN))
    return [
        (f"read {name} {options}", f"return P.read({text}, {lua_string(name)}, {options})")
        for options in READER_OPTIONS
    ]


def written(arguments):
    """A write probe without the final line ends: the group
    `convert-final-newline` compares them (row I-LUA-30)."""
    return f"return (P.write({arguments}):gsub('\\n+$', ''))"


ODT_CONTENT = """local function odt_content(bytes)
  local ok, archive = pcall(function() return P.zip.Archive end)
  if not ok then return P.read(bytes, 'odtcontentxml').blocks[1].text end
  for _, entry in ipairs(archive(bytes).entries) do
    if entry.path == 'content.xml' then return entry:contents() end
  end
  error('Missing content.xml')
end"""

FODT_BODY = """local function fodt_body(xml)
  local body = assert(xml:match('(<office:body>.-</office:body>)'))
  -- Match equivalent XML text and attribute order. Skip the formula
  -- annotation because Pandoc and Randoc use different encodings (I-ODT-03).
  body = body:gsub('>([^<]+)<', function(text)
    return '>' .. text:gsub('&quot;', '"') .. '<'
  end)
  body = body:gsub('(<math) (display="[^"]+") (xmlns="[^"]+")>', '%1 %3 %2>')
  return (body:gsub('<annotation encoding="StarMath 5%.0">.-</annotation>', '')
              :gsub('<annotation encoding="application/x%-tex">.-</annotation>', ''))
end"""


def write_probe(name, spec, options=""):
    doc = WIKI_DOC if name in ("dokuwiki", "jira", "mediawiki") and (options or spec != name) else DOC
    arguments = f"{doc}, {lua_string(spec)}{options}"
    if name == "odt":
        return f"return odt_content(P.write({arguments}))"
    if name == "fodt":
        return f"return fodt_body(P.write({arguments}))"
    return written(arguments)


def write_probes(name):
    probes = [(f"write {name}", write_probe(name, name))]
    for spec in WRITER_SPECS[name]:
        probes.append((f"write {name}{spec}", write_probe(name, name + spec)))
    return probes


def write_option_probes(name):
    return [
        (f"write {name} {options}", write_probe(name, name, f", {options}"))
        for options in WRITER_OPTIONS
    ]


GROUPS = {}
for _name in READERS:
    GROUPS[f"convert-read-{_name}"] = read_probes(_name) + read_option_probes(_name)
for _name in WRITERS:
    GROUPS[f"convert-write-{_name}"] = write_probes(_name) + write_option_probes(_name)

GROUPS["convert-final-newline"] = [
    (f"end of {name}", f"return P.write({DOC}, {lua_string(name)}):sub(-2)") for name in WRITERS if name != "odt"
]

_DOC = "P.Pandoc{P.Para{P.Emph't', P.Space(), P.Str'\"q\"'}, P.Header(2, 'h')}"
GROUPS["convert-arguments"] = [
    ("readers type", "return type(P.readers), P.readers.markdown, P.readers.native"),
    ("writers type", "return type(P.writers), P.writers.html, P.writers.native"),
    ("read native document", "return P.read('Pandoc (Meta {unMeta = fromList []}) [Plain [Str \"b\"]]', 'native')"),
    ("read native error", "return P.read('[Para [Str \"a\"]', 'native')"),
    ("read native words", "return P.read('[a](b)', 'native')"),
    ("read default format", "return P.read('*a* \"b\"')"),
    ("read number", "return P.read(12, 'native')"),
    ("read number markdown", "return P.read(1.5)"),
    ("read list", "return P.read({'[Para [Str \"a\"]', ']'}, 'native')"),
    ("read list markdown", "return P.read({'*a', 'b*'})"),
    ("read empty list", "return P.read({}, 'native')"),
    ("read list latex", "return P.read({'\\\\emph{a', 'b}'}, 'latex')"),
    ("read list html", "return P.read({'<em>a', 'b</em>'}, 'html')"),
    ("read list line ends", "return P.read({'a\\n', '\\nb'})"),
    ("read list heading", "return P.read({'# a', 'b'}, 'commonmark')"),
    ("read list number item", "return P.read({'a', 5})"),
    ("read list table item", "return P.read({'a', {}})"),
    ("read list boolean item", "return P.read({true}, 'native')"),
    ("read spec table extensions string", "return P.read('\"a\"', {format = 'markdown', extensions = 'smart'})"),
    ("read spec table extensions number list", "return P.read('\"a\"', {format = 'markdown', extensions = {5}})"),
    ("read spec table extensions unknown map", "return P.read('\"a\"', {format = 'markdown', extensions = {nosuch = false}})"),
    ("read spec table extensions unsupported map", "return P.read('[]', {format = 'native', extensions = {smart = false}})"),
    ("read invalid utf8", "return P.read('\\255', 'native')"),
    ("read invalid utf8 later", "return P.read('[Para [Str \"\\195\\40\"]]', 'native')"),
    ("read nil", "return P.read(nil)"),
    ("read boolean", "return P.read(true, 'native')"),
    ("read no argument", "return P.read()"),
    ("read unknown", "return P.read('x', 'nosuch')"),
    ("read writer name", "return P.read('x', 'plain')"),
    ("read upper", "return P.read('[]', 'NATIVE')"),
    ("read upper extension", "return P.read('\"a\"', 'Markdown-Smart')"),
    ("read spec plus", "return P.read('x', '+smart')"),
    ("read spec empty", "return P.read('x', '')"),
    ("read spec minus", "return P.read('x', '-x')"),
    ("read bad extension", "return P.read('x', 'markdown+nosuch')"),
    ("read unsupported extension", "return P.read('x', 'native+smart')"),
    ("read empty extension", "return P.read('x', 'markdown++smart')"),
    ("read spec space", "return P.read('x', 'mark down')"),
    ("read spec number", "return P.read('x', 12)"),
    ("read spec table", "return P.read('[]', {format = 'native'})"),
    ("read spec table markdown list", "return P.read('\"a\" ~~b~~', {format = 'markdown', extensions = {'strikeout'}})"),
    ("read spec table markdown map", "return P.read('\"a\" ~~b~~', {format = 'markdown', extensions = {smart = false, strikeout = 'enable'}})"),
    ("read spec table markdown map disable", "return P.read('\"a\" ~~b~~', {format = 'markdown', extensions = {strikeout = 'disable'}})"),
    ("read spec table no format", "return P.read('[]', {})"),
    ("read spec table number", "return P.read('[]', {format = 1})"),
    ("read spec table bad list", "return P.read('[]', {format = 'native', extensions = {'nosuch'}})"),
    ("read spec table bad map", "return P.read('[]', {format = 'native', extensions = {smart = true}})"),
    ("read options several", "return P.read('[]', 'native', {columns = 3, standalone = true, tab_stop = '2'})"),
    ("read options unknown", "return P.read('[]', 'native', {nosuch = 3})"),
    ("read options number", "return P.read('[]', 'native', 5)"),
    ("read options string", "return P.read('[]', 'native', 'x')"),
    ("read options columns", "return P.read('[]', 'native', {columns = 'x'})"),
    ("read options tab", "return P.read('[]', 'native', {tab_stop = {}})"),
    ("read options classes", "return P.read('[]', 'native', {indented_code_classes = 'x'})"),
    ("read options abbreviations", "return P.read('[]', 'native', {abbreviations = 'x'})"),
    ("read options track", "return P.read('[]', 'native', {track_changes = 'bad'})"),
    ("read options key", "return P.read('[]', 'native', {[1] = 2})"),
    ("read options extensions", "return P.read('\"a\"', 'markdown', {extensions = {'smart'}})"),
    ("read options object changed", "PANDOC_READER_OPTIONS.columns = 4 return P.read('[]', 'native', PANDOC_READER_OPTIONS)"),
    ("read env", "return P.read('[]', 'native', nil, {})"),
    ("read env number", "return P.read('[]', 'native', nil, 5)"),
    ("write default format", f"return P.write({_DOC})"),
    ("write native columns text", f"return P.write({_DOC}, 'native', {{columns = '12'}})"),
    ("write options object changed", f"PANDOC_WRITER_OPTIONS.columns = 4 local r = P.write({_DOC}, 'native', PANDOC_WRITER_OPTIONS) PANDOC_WRITER_OPTIONS.columns = 72 return r"),
    ("write spec table", f"return P.write({_DOC}, {{format = 'native'}})"),
    ("write spec table html map", f"return P.write({_DOC}, {{format = 'html', extensions = {{smart = false}}}})"),
    ("write spec table markdown list", f"return P.write({_DOC}, {{format = 'markdown', extensions = {{'smart'}}}})"),
    ("write unknown", f"return P.write({_DOC}, 'nosuch')"),
    ("write reader name", f"return P.write({_DOC}, 'csv')"),
    ("write upper", f"return P.write({_DOC}, 'HTML')"),
    ("write string", "return P.write('x')"),
    ("write block", "return P.write(P.Para'x', 'native')"),
    ("write inlines", "return P.write(P.Inlines'x', 'native')"),
    ("write table", "return P.write({}, 'native')"),
    ("write nothing", "return P.write()"),
    ("write bad extension", f"return P.write({_DOC}, 'html+nosuch')"),
    ("write unsupported extension", f"return P.write({_DOC}, 'native+smart')"),
    ("write spec plus", f"return P.write({_DOC}, '+smart')"),
    ("write spec empty", f"return P.write({_DOC}, '')"),
    ("write spec number", f"return P.write({_DOC}, 7)"),
    ("write spec table no format", f"return P.write({_DOC}, {{extensions = {{}}}})"),
    ("write spec table number", f"return P.write({_DOC}, {{format = 1}})"),
    ("write options number", f"return P.write({_DOC}, 'native', 7)"),
    ("write options string", f"return P.write({_DOC}, 'native', 'x')"),
    ("write options unknown", f"return P.write({_DOC}, 'native', {{nosuch = 1}})"),
    ("write options key", f"return P.write({_DOC}, 'native', {{[1] = 2}})"),
    ("write options columns", f"return P.write({_DOC}, 'native', {{columns = 'x'}})"),
    ("write options columns float", f"return P.write({_DOC}, 'native', {{columns = 2.5}})"),
    ("write options many", f"return P.write({_DOC}, 'native', {{slide_level = 2, number_offset = {{1, 2}}, epub_fonts = {{'a'}}, "
     f"incremental = 'x', dpi = 72, toc_depth = '2', tab_stop = 3, wrap_text = 'wrap-none', cite_method = 'natbib', "
     f"email_obfuscation = 'none', reference_location = 'end-of-block', top_level_division = 'top-level-part', "
     f"figure_caption_position = 'above', table_caption_position = 'below', chunk_template = 'x', identifier_prefix = 'p'}})"),
] + [
    (f"write option {field} = {value}", f"return P.write({_DOC}, 'native', {{{field} = {value}}})")
    for field in ["cite_method", "email_obfuscation", "reference_location", "top_level_division",
                  "wrap_text", "figure_caption_position", "table_caption_position", "chunk_template",
                  "identifier_prefix", "epub_fonts", "number_offset"]
    for value in ["'bad'", "5", "true"]
]


# The probes that differ from the oracle for a reason that a row of
# `docs/ISSUES.md` records. The replay test fails when one of them becomes
# equal, so that the list stays exact.
_MARKDOWN_READERS = ["markdown", "markdown_github", "markdown_mmd", "markdown_phpextra",
                     "markdown_strict"]
KNOWN = (
    [("I-LUA-30", "convert-final-newline", f"end of {name}")
     for name in ["commonmark", "commonmark_x", "gfm", "markdown", "markdown_github",
                  "markdown_mmd", "markdown_phpextra", "markdown_strict", "markua", "plain"]]
    + [("I-LUA-30", "convert-arguments", label)
       for label in ["write default format", "write spec table markdown list"]]
    + [("I-LUA-31", f"convert-read-{name}", f"read {name} {options}")
       for name in _MARKDOWN_READERS
       for options in ["{tab_stop = 2}", "{tab_stop = 8}", "{default_image_extension = 'png'}",
                       "{strip_comments = true}"]]
    + [("I-LUA-31", f"convert-write-{name}", f"write {name} {{prefer_ascii = true}}")
       for name in ["commonmark", "gfm", "markdown_github", "markdown_mmd", "markdown_phpextra",
                    "markdown_strict", "plain"]]
    + [("I-LUA-31", f"convert-write-{name}", f"write {name} {{columns = 30}}")
       for name in ["markdown_mmd", "markdown_phpextra"]]
    + [("I-LUA-31", "convert-write-markdown", "write markdown+hard_line_breaks"),
       ("I-LUA-31", "convert-write-markdown_strict",
        "write markdown_strict {reference_location = 'end-of-section'}"),
       ("I-LUA-31", "convert-write-plain", "write plain+gutenberg")]
    + [("I-LUA-31", "convert-write-markua", label)
       for label, _ in [("write markua", None)] + write_option_probes("markua")]
    + [("I-ORG-01", "convert-final-newline", "end of org"),
       ("I-WIKI-03", "convert-write-dokuwiki", "write dokuwiki"),
       ("I-WIKI-04", "convert-write-jira", "write jira"),
       ("I-WIKI-05", "convert-write-mediawiki", "write mediawiki"),
       ("I-WIKI-05", "convert-final-newline", "end of mediawiki")]
    + [("I-LUA-32", "convert-arguments", "read spec table bad list"),
       ("I-LUA-32", "convert-arguments", "read list heading")]
)


def filter_source(name, probes):
    lines = [HELPER]
    if name == "convert-write-odt":
        lines.append(ODT_CONTENT)
    if name == "convert-write-fodt":
        lines.append(FODT_BODY)
    lines.append("local function run(document)")
    for label, body in probes:
        lines.append(f"  probe({lua_string(label)}, function() {body} end)")
    lines.append("end")
    lines.append(FOOTER)
    return "\n".join(lines)


def main():
    parts = [
        "# The oracle matrix of pandoc.read and pandoc.write.\n",
        "# `tools/convert-matrix.py` writes this file. Do not edit it.\n",
        f"# readers: {' '.join(READERS)}\n",
        f"# writers: {' '.join(WRITERS)}\n",
    ] + [f"# known {issue} {name} {label}\n" for issue, name, label in KNOWN]
    for name, text in DOCS.items():
        parts.append(f"@@@ doc {name}\n{text}\n")
    for name, probes in GROUPS.items():
        parts.append(f"@@@ filter {name} empty small\n{filter_source(name, probes)}\n")
    with open(OUTPUT, "w", encoding="utf-8") as handle:
        handle.write("".join(parts))
    print(f"{len(GROUPS)} filters, {sum(len(p) for p in GROUPS.values())} probes")


if __name__ == "__main__":
    main()
