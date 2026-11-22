"""Supply original documents that exercise each extension."""

import io
import json
from xml.sax.saxutils import escape, quoteattr
from zipfile import ZipFile, ZipInfo

from generate import document, json_bytes
from inputs import Input
from samples import EMPTY_ATTR, inline_text, node, one, plain, table_forms


MARKDOWN = {
    "abbreviations": "The VSO is ready.\n\n*[VSO]: Valve shutoff",
    "alerts": "> [!WARNING]\n> Use the west stair.",
    "all_symbols_escapable": r"Flow \? pressure \@ noon.",
    "angle_brackets_escapable": r"Mark \<angle\> on the drawing.",
    "ascii_identifiers": "# Débit mesuré",
    "attributes": "# Flow {#station-x .reading}",
    "auto_identifiers": "# 7. Débit & flow",
    "autolink_bare_uris": "Read https://example.com/flow.",
    "backtick_code_blocks": "``` text\nflow = 24\n```",
    "blank_before_blockquote": "Read the note.\n> Flow is low.",
    "blank_before_header": "Read the note.\n# Flow check",
    "bracketed_spans": "[Low flow]{.caution}",
    "citations": "Repeat the steps in [@flow-survey].",
    "definition_lists": "Cutoff\n:   The minimum reading.",
    "east_asian_line_breaks": "流量\n測定",
    "emoji": "The sun is up :sunny:.",
    "escaped_line_breaks": "Upstream\\\nDownstream",
    "example_lists": "(@valve) Check the valve.\n(@pump) Check the pump.\n\nSee (@valve).",
    "fancy_lists": "C) Check flow.\nD) Check pressure.",
    "fenced_code_attributes": "``` {.text #valve-reading}\nflow = 24\n```",
    "fenced_code_blocks": "~~~ text\nflow = 24\n~~~",
    "fenced_divs": "::: {.caution}\nUse the west stair.\n:::",
    "footnotes": "Check flow.[^valve]\n\n[^valve]: Read valve 7.",
    "four_space_rule": "- Check flow.\n\n  Check pressure.",
    "gfm_auto_identifiers": "# 7. Débit ~ flow",
    "grid_tables": "+---------+---------+\n| Valve 7 | Open    |\n+---------+---------+",
    "gutenberg": "Gutenberg punctuation: _flow_ and =pressure=.",
    "hard_line_breaks": "Check flow.\nCheck pressure.",
    "header_attributes": "# Flow {#station-x .reading}",
    "ignore_line_breaks": "Check flow.\nCheck pressure.",
    "implicit_figures": "![Valve cross section](valve.png)",
    "implicit_header_references": "# East circuit\n\nOpen the [East circuit] page.",
    "inline_code_attributes": "Read `flow`{#current .reading}.",
    "inline_notes": "Check pressure.^[Use gauge 7.]",
    "intraword_underscores": "Read the north_flow_sensor.",
    "latex_macros": r"\newcommand{\reading}{24}  Flow is $\reading$.",
    "line_blocks": "| East station\n|   West station",
    "link_attributes": "[Flow](flow.txt){.reference width=40%}",
    "lists_without_preceding_blankline": "Read the meters.\n- Check flow.\n- Check pressure.",
    "literate_haskell": "> volume = 24\n> reading = volume + 3",
    "mark": "The reading is ==above the limit==.",
    "markdown_attribute": '<div markdown="1">\n*Check flow.*\n</div>',
    "markdown_in_html_blocks": "<section>\n\n*Check flow.*\n\n</section>",
    "mmd_header_identifiers": "## Valve number [service-valve]",
    "mmd_link_attributes": "[meter]: meter.png \"Valve meter\" height=44px\n\n![Gauge][meter]",
    "mmd_title_block": "Title: Valve survey\nAuthor: River Lab\n\nMeasure flow.",
    "multiline_tables": "------------------------------\nMeter          Reading\n-------------  ---------------\nNorth          24 litres\n               per minute\n\nSouth          18 litres\n               per minute\n------------------------------",
    "native_divs": '<div id="north">\n\nMeasure flow.\n\n</div>',
    "native_spans": 'Read <span class="meter">24</span> litres.',
    "old_dashes": "Use the east-west pipe at 7-9 -- inspect it first.",
    "pandoc_title_block": "% Valve survey\n% River Lab\n% 2024-02-03\n\nMeasure flow.",
    "pipe_tables": "| Circuit | Flow |\n|---------|------|\n| North   | 24   |",
    "raw_attribute": "```{=html}\n<aside>Check flow.</aside>\n```",
    "raw_html": "Inspect the <meter value=\"24\">flow meter</meter>.",
    "raw_markdown": "**Valve 7** is open.",
    "raw_tex": r"Use the \textsf{flow} meter.",
    "rebase_relative_paths": "[Flow](data/flow.csv)",
    "short_subsuperscripts": "Q~1~ and m^2^",
    "shortcut_reference_links": "Read the [meter].\n\n[meter]: /measurements",
    "simple_tables": "Circuit  Flow\n-------  ----\nNorth    24\nSouth    18",
    "smart": '"East valve" -- flow is low...',
    "sourcepos": "## Flow\n\nRead valve *7*.",
    "space_in_atx_header": "##Valve reading",
    "spaced_reference_links": "Read [the meter] [meter].\n\n[meter]: /measurements",
    "startnum": "7. Inspect the valve.\n8. Inspect the pump.",
    "strikeout": "The valve is ~~closed~~ open.",
    "subscript": "Use Q~1~ for the north valve.",
    "superscript": "Flow is measured over 4 m^2^.",
    "table_attributes": "| Circuit | Flow |\n|---------|------|\n| North   | 24   |\n\n: Results {#readings .sample}",
    "table_captions": "| Circuit | Flow |\n|---------|------|\n| North   | 24   |\n\n: Readings from the north circuit",
    "task_lists": "- [x] Check flow.\n- [ ] Check pressure.",
    "tex_math_dollars": "Flow is $q_{7}$.",
    "tex_math_double_backslash": r"Flow is \\(q_{7}\\).",
    "tex_math_gfm": "Flow is $`q_{7}`$.\n\n```math\nq_{7} = 24\n```",
    "tex_math_single_backslash": r"Flow is \(q_{7}\).",
    "wikilinks_title_after_pipe": "See [[flow|Flow measurements]].",
    "wikilinks_title_before_pipe": "See [[Flow measurements|flow]].",
    "yaml_metadata_block": "---\ntitle: Valve survey\n---\n\nMeasure flow.",
}

ORG = {
    "citations": "Use the procedure in [cite:@valve-survey].",
    "fancy_lists": "c. Read valve 7.\nd. Read valve 8.",
    "smart": '"East valve" -- flow is low...',
    "smart_quotes": '"The east valve" needs a new seal.',
    "special_strings": r"East---west, 20--24, cut\-off, wait...",
    "task_lists": "- [X] Read valve 7.\n- [ ] Read valve 8.",
}

HTML = {
    "empty_paragraphs": "<p>Check flow.</p><p></p><p>Check pressure.</p>",
    "epub_html_exts": '<p>Flow<a epub:type="noteref" href="#note">1</a>.</p>'
                     '<aside epub:type="footnote" id="note"><p>Read gauge 7.</p></aside>',
    "line_blocks": '<div class="line-block">East station<br />West station</div>',
    "literate_haskell": '<pre class="haskell literate">volume = 24</pre>',
    "raw_html": "<p>Pressure is <meter value=\"24\">24</meter>.</p>",
    "raw_tex": r'<span class="math inline">\(q_{7}\)</span>',
    "task_lists": '<ul class="task-list"><li><input type="checkbox" checked=""/>Read valve 7.</li></ul>',
}

NATIVE = {
    "asciidoc": "= Débit mesuré\n\n流量\n測定",
    "biblatex": "@book{valve, author={Eva Rios}, title={Flow rates}, date={2024}}",
    "bibtex": "@book{valve, author={Eva Rios}, title={Flow rates}, year={2024}}",
    "csljson": '[{"id":"valve","type":"book","title":"流量 測定","author":[{"family":"Rios"}]}]',
    "docbook": "<article><title>Flow rates</title><para>流量\n測定</para></article>",
    "dokuwiki": "====== Débit mesuré ======\n\n< html>flow</html> $$q_7$$",
    "fb2": '<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0">'
           "<body><section><p>流量\n測定</p></section></body></FictionBook>",
    "haddock": "= Flow rates\n\n流量\n測定",
    "jira": "h1. Flow rates\n\n流量\n測定",
    "latex": r"\section{Débit mesuré}\newcommand{\valve}{q_7} Flow is $\valve$.",
    "man": ".TH FLOW 1\n.SH Débit mesuré\n.PP\n流量\n測定",
    "mediawiki": "== Débit mesuré ==\n\n\"East valve\" -- flow is low...",
    "muse": ";; service note\n#title Valve survey\n\n* Débit mesuré\n\n; shutoff note\n\n#end",
    "org": "* Débit mesuré\n\n流量\n測定",
    "rst": "Débit mesuré\n============\n\n> volume = 24\n\n\"East valve\" -- flow is low...",
    "rtf": r"{\rtf1\ansi\uc1 Flow \u27969?\u37327?\line \u28204?\u23450?\par}",
    "t2t": "Flow rates\nRiver Lab\n2024-03-05\n\n流量\n測定",
    "textile": "h1. Débit mesuré\n\n\"East valve\" -- flow is low...\\textsf{flow}",
    "twiki": '---+ Débit mesuré\n\n"East valve" -- flow is low...',
    "typst": "= Débit mesuré\n\nSee @valve-survey.\n\n\"East valve\" -- flow is low...",
    "vimwiki": "= Débit mesuré =\n\n流量\n測定",
    "xml": "<document><title>Flow rates</title><p>流量\n測定</p></document>",
}


def workbook():
    types = "http://schemas.openxmlformats.org/package/2006/content-types"
    relations = "http://schemas.openxmlformats.org/package/2006/relationships"
    office = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
    sheet = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
    content = "application/vnd.openxmlformats-officedocument.spreadsheetml"
    parts = {
        "[Content_Types].xml":
            f'<Types xmlns="{types}"><Default Extension="rels" '
            'ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
            f'<Override PartName="/xl/workbook.xml" ContentType="{content}.sheet.main+xml"/>'
            f'<Override PartName="/xl/worksheets/sheet1.xml" ContentType="{content}.worksheet+xml"/></Types>',
        "_rels/.rels":
            f'<Relationships xmlns="{relations}"><Relationship Id="rId1" '
            f'Type="{office}/officeDocument" Target="xl/workbook.xml"/></Relationships>',
        "xl/workbook.xml":
            f'<workbook xmlns="{sheet}" xmlns:r="{office}"><sheets>'
            '<sheet name="Flow" sheetId="1" r:id="rId1"/></sheets></workbook>',
        "xl/_rels/workbook.xml.rels":
            f'<Relationships xmlns="{relations}"><Relationship Id="rId1" '
            f'Type="{office}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>',
        "xl/worksheets/sheet1.xml":
            f'<worksheet xmlns="{sheet}"><sheetData><row r="1"><c r="A1" t="inlineStr">'
            "<is><t>流量&#10;測定</t></is></c></row></sheetData></worksheet>",
    }
    output = io.BytesIO()
    with ZipFile(output, "w") as archive:
        for path, data in parts.items():
            archive.writestr(ZipInfo(path), data.encode("utf-8"))
    return Input("authored", output.getvalue())


def reader_probe(fmt, extension):
    markdown = MARKDOWN.get(extension, "## Débit mesuré\n\nCheck flow.")
    if fmt.startswith(("markdown", "commonmark")) or fmt in {"gfm", "djot"}:
        data = markdown
    elif fmt == "ipynb":
        data = json.dumps({"cells": [{"cell_type": "markdown", "metadata": {}, "source": markdown}],
                           "metadata": {}, "nbformat": 4, "nbformat_minor": 4})
    elif fmt == "opml":
        note = quoteattr(markdown, {"\n": "&#10;"})
        data = f'<opml version="2.0"><head/><body><outline text="Flow" _note={note}/></body></opml>'
    elif fmt == "html":
        sample = HTML.get(extension, f"<p>{escape(markdown)}</p>")
        data = '<html xmlns:epub="http://www.idpf.org/2007/ops"><body>' + sample + "</body></html>"
    elif fmt == "org":
        data = ORG.get(extension, NATIVE[fmt])
    elif fmt == "jats":
        data = ('<article><body><p><element-citation publication-type="journal">'
                '<person-group person-group-type="author"><name><surname>Rios</surname>'
                '<given-names>Eva</given-names></name></person-group><article-title>Valve survey</article-title>'
                '<year>2024</year></element-citation></p></body></article>')
    elif fmt == "fodt":
        data = ('<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
                'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">'
                '<office:body><office:text><text:h text:outline-level="1">Valve survey</text:h>'
                '<text:p/><text:p>流量\n測定</text:p></office:text></office:body></office:document>')
    elif fmt in NATIVE:
        data = NATIVE[fmt]
    elif fmt == "json":
        return writer_probe(extension)
    elif fmt == "xlsx":
        return workbook()
    else:
        return None
    return Input("authored", (data + "\n").encode("utf-8"))


def writer_probe(extension):
    heading = node("Header", [1, ["north-valve", [], []], inline_text("Débit mesuré")])
    blocks = [
        heading,
        node("Para", [node("Str", "流量"), node("SoftBreak"), node("Str", "測定")]),
        node("Para", [node("Emph", inline_text("Check")), node("Space"),
                      node("Quoted", [node("DoubleQuote"), inline_text("pressure")]),
                      node("Space"), node("Str", "—…"), node("Space"),
                      node("Link", [EMPTY_ATTR, [], ["#north-valve", ""]])]),
        node("Para", []),
    ]
    if extension in {"grid_tables", "multiline_tables", "ntb", "native_numbering", "pipe_tables",
                     "simple_tables", "table_attributes", "table_captions", "xrefs_number", "xrefs_name"}:
        table = next(table_forms())[1]
        table["c"][0] = ["readings", [], []]
        blocks.extend([table, node("Para", [node("Link", [EMPTY_ATTR, [], ["#readings", ""]])])])
    elif extension == "element_citations":
        ref = node("MetaMap", {
            "id": node("MetaString", "valve-survey"), "type": node("MetaString", "article-journal"),
            "title": node("MetaString", "Valve survey"),
            "author": node("MetaList", [node("MetaMap", {
                "given": node("MetaString", "Eva"), "family": node("MetaString", "Rios")})]),
        })
        tree = {"meta": {"references": node("MetaList", [ref])}, "blocks": blocks}
        return Input("authored", json_bytes(document("extension-probe", tree)))
    elif extension in {"amuse", "styles", "native_divs", "native_spans", "bracketed_spans", "attributes",
                       "fenced_divs", "link_attributes", "inline_code_attributes"}:
        span = node("Span", [["valve", ["reading"], [["custom-style", "Valve reading"]]], inline_text("24")])
        blocks.append(node("Div", [["north", ["station"], [["custom-style", "Station note"]]],
                                   [node("Para", [span])]]))
    elif extension in {"fancy_lists", "example_lists", "startnum", "task_lists",
                       "lists_without_preceding_blankline", "four_space_rule"}:
        style = "Example" if extension == "example_lists" else "UpperAlpha"
        blocks.extend([node("OrderedList", [[3, node(style), node("OneParen")],
                                            [[plain("Valve 7")], [plain("Valve 8")]]]),
                       node("BulletList", [[plain("☑ Read gauge 7.")], [plain("☐ Read gauge 8.")]])])
    elif extension in {"literate_haskell", "backtick_code_blocks", "fenced_code_blocks",
                       "fenced_code_attributes"}:
        blocks.append(node("CodeBlock", [["valve", ["haskell", "literate"], []], "volume = 24"]))
    elif extension in {"raw_html", "raw_tex", "raw_markdown", "raw_attribute", "epub_html_exts"}:
        blocks.extend([node("RawBlock", [fmt, data]) for fmt, data in (
            ("html", "<aside>Read gauge 7.</aside>"), ("tex", r"\textsf{Valve}"),
            ("markdown", "**Read gauge 7.**"))])
    elif extension in {"strikeout", "subscript", "superscript", "mark", "line_blocks", "footnotes",
                       "inline_notes", "hard_line_breaks", "short_subsuperscripts", "escaped_line_breaks"}:
        blocks.extend([node("LineBlock", [inline_text("East valve"), inline_text("West valve")]),
                       node("Para", [node("Strikeout", inline_text("closed")), node("LineBreak"),
                                     node("Subscript", inline_text("7")), node("Superscript", inline_text("2")),
                                     node("Note", [plain("Read gauge 7.")])])])
    elif extension.startswith("tex_math") or extension == "latex_macros":
        blocks.append(node("Para", [node("Math", [node("InlineMath"), "q_{7}"])]))
    tree = one(heading)
    tree["blocks"] = blocks
    return Input("authored", json_bytes(document("extension-probe", tree)))
