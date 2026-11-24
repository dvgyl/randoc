//! Checks XML conversion, limits, and format registration.

#![allow(clippy::unwrap_used)]

use randoc_ast::*;
use randoc_core::{Context, Error, Reader, ReaderOptions, Sources, WrapOption, WriterOptions};
use randoc_xmlformats::{reader, writer};

fn read(format: &str, text: &str, context: &mut Context) -> randoc_core::Result<Pandoc> {
    let Some(Reader::Text(reader)) = reader(format) else {
        return Err(Error::App("Missing reader.".into()));
    };
    reader.read(
        &Sources::from_text(text),
        &ReaderOptions::default(),
        context,
    )
}

fn write(format: &str, doc: &Pandoc, context: &mut Context) -> randoc_core::Result<String> {
    let mut options = WriterOptions::default();
    options.wrap_text = WrapOption::WrapNone;
    let output = writer(format)
        .ok_or_else(|| Error::App("Missing writer.".into()))?
        .write(doc, &options, context)?;
    Ok(String::from_utf8(output.into_bytes()).unwrap())
}

#[test]
fn registers_supported_names() {
    for name in ["docbook", "jats", "bits", "fb2"] {
        assert!(matches!(reader(name), Some(Reader::Text(_))));
    }
    for name in [
        "tei",
        "docbook4",
        "docbook5",
        "jats_archiving",
        "jats_articleauthoring",
        "jats_publishing",
    ] {
        assert!(reader(name).is_none());
    }
    for name in [
        "docbook",
        "docbook4",
        "docbook5",
        "jats",
        "jats_archiving",
        "jats_articleauthoring",
        "jats_publishing",
        "tei",
    ] {
        assert!(writer(name).unwrap().renders_template());
    }
    assert!(!writer("fb2").unwrap().renders_template());
    assert!(writer("bits").is_none());
}

#[test]
fn jats_divs_choose_the_wrapper_from_the_child_blocks() {
    let plain = || Block::Plain(vec![Inline::Str("Plain".into())]);
    let para = || Block::Para(vec![Inline::Str("Paragraph".into())]);
    let div = |id, children| {
        Block::Div(
            Attr::new(
                id,
                vec!["note".into()],
                vec![("custom".into(), "value".into())],
            ),
            children,
        )
    };
    let doc = Pandoc {
        blocks: vec![
            div("empty", vec![]),
            div("1plain&", vec![plain(), plain()]),
            div("para", vec![para()]),
            div("Ⅳé·1", vec![]),
            div("outer", vec![plain(), div("inner", vec![para()])]),
            div("", vec![para()]),
        ],
        ..Pandoc::default()
    };
    let expected = concat!(
        "<p id=\"empty\">\n</p>\n",
        "<p id=\"U0031plainU0026\">\n  <p>Plain</p>\n  <p>Plain</p>\n</p>\n",
        "<boxed-text id=\"para\">\n  <p>Paragraph</p>\n</boxed-text>\n",
        "<p id=\"U2163é·1\">\n</p>\n",
        "<boxed-text id=\"outer\">\n  <p>Plain</p>\n",
        "  <boxed-text id=\"inner\">\n    <p>Paragraph</p>\n  </boxed-text>\n</boxed-text>\n",
        "<p>Paragraph</p>",
    );
    for format in [
        "jats",
        "jats_archiving",
        "jats_articleauthoring",
        "jats_publishing",
    ] {
        assert_eq!(
            write(format, &doc, &mut Context::pure()).unwrap(),
            expected,
            "{format}"
        );
    }
}

#[test]
fn fragment_boundaries() {
    let mut context = Context::pure();
    for format in ["docbook", "jats", "bits"] {
        assert_eq!(
            read(format, "<!--x-->", &mut context).unwrap(),
            Pandoc::default()
        );
        assert_eq!(
            read(format, "one<x>two</x>three", &mut context)
                .unwrap()
                .blocks
                .len(),
            3
        );
    }
    assert!(read("fb2", "<FictionBook/><FictionBook/>", &mut context).is_err());
}

#[test]
fn xml_close_tag_span() {
    for format in ["docbook", "jats", "bits", "fb2"] {
        let error = read(format, "<broken><p>X</broken>", &mut Context::pure()).unwrap_err();
        assert!(matches!(error, Error::Xml { file, message }
            if file.is_empty() && message == "1:13 (12)-1:22 (21): Unexpected close tag broken"));
    }
}

#[test]
fn xml_entity_span() {
    for format in ["jats", "bits"] {
        let error = read(format, "<p>&hellip;</p>", &mut Context::pure()).unwrap_err();
        assert!(matches!(error, Error::Xml { file, message }
            if file.is_empty() && message == "1:4 (3)-1:12 (11): Unresolved entity &hellip;"));
    }
}

#[test]
fn docbook_entities() {
    let doc = read(
        "docbook",
        "<para>&nbsp;&copy;&b.alpha;</para>",
        &mut Context::pure(),
    )
    .unwrap();
    assert_eq!(
        doc.blocks,
        vec![Block::Para(vec![Inline::Str("\u{a0}©𝛂".into())])]
    );
}

#[test]
fn jats_prefers_tex_to_mathml() {
    let text = "<p><inline-formula><alternatives><math><msup><mi>x</mi><mn>2</mn></msup></math><tex-math><![CDATA[$x^3$]]></tex-math></alternatives></inline-formula></p>";
    let doc = read("jats", text, &mut Context::pure()).unwrap();
    assert_eq!(
        doc.blocks,
        vec![Block::Para(vec![Inline::Math(
            MathType::InlineMath,
            "x^3".into()
        )])]
    );
}

#[test]
fn jats_writes_math_for_each_variant() {
    let doc = Pandoc {
        blocks: vec![Block::Para(vec![Inline::Math(
            MathType::InlineMath,
            "x^2".into(),
        )])],
        ..Pandoc::default()
    };
    let mut context = Context::pure();
    for format in ["jats", "jats_archiving", "jats_publishing"] {
        let text = write(format, &doc, &mut context).unwrap();
        assert!(text.contains("<alternatives>\n<tex-math><![CDATA[x^2]]></tex-math>\n<mml:math"));
    }
    let text = write("jats_articleauthoring", &doc, &mut context).unwrap();
    assert!(text.contains("<inline-formula><mml:math"));
}

#[test]
fn docbook_normalizes_heading_levels() {
    let doc = Pandoc {
        blocks: [4, 2, 3]
            .into_iter()
            .map(|level| {
                Block::Header(level, Attr::default(), vec![Inline::Str(level.to_string())])
            })
            .collect(),
        ..Pandoc::default()
    };
    let text = write("docbook4", &doc, &mut Context::pure()).unwrap();
    let tags: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("<sect"))
        .collect();
    assert_eq!(tags, ["<sect3>", "<sect1>", "<sect2>"]);
}

#[test]
fn docbook_omits_titles_and_ids_on_procedures_and_substeps() {
    let text = r#"<procedure id="p"><title>A</title><step><para>B</para>
        <substeps id="s"><title>C</title><step><para>D</para></step></substeps>
        </step></procedure>
        <orderedlist id="o"><title>E</title><listitem><para>F</para></listitem></orderedlist>"#;
    let doc = read("docbook", text, &mut Context::pure()).unwrap();
    let [Block::OrderedList(_, steps), Block::Div(attrs, _)] = doc.blocks.as_slice() else {
        panic!("expected a procedure and a titled ordered list");
    };
    assert_eq!(attrs.id, "o");
    let [step] = steps.as_slice() else {
        panic!("expected one step");
    };
    let [Block::Para(_), Block::OrderedList(_, substeps)] = step.as_slice() else {
        panic!("expected a paragraph and a substep list");
    };
    assert_eq!(
        substeps,
        &[vec![Block::Para(vec![Inline::Str("D".into())])]]
    );
}

#[test]
fn fb2_decodes_and_stages_media() {
    let mut context = Context::pure();
    let text = "<FictionBook><binary id='i' content-type='text/plain'>!!aG???k=!!</binary><binary id='i' content-type='image/png'>QQ</binary><binary id='../i' content-type='text/plain'>!!!</binary></FictionBook>";
    read("fb2", text, &mut context).unwrap();
    let item = context.media.get("i").unwrap();
    assert_eq!(item.contents, b"A");
    assert_eq!(item.mime_type, "image/png");
    assert!(
        context
            .media
            .get("da39a3ee5e6b4b0d3255bfef95601890afd80709.txt")
            .is_some()
    );
}

#[test]
fn input_size_is_bounded() {
    let mut context = Context::pure();
    context.limits.max_input_size = 8;
    assert!(read("jats", "<p>hello</p>", &mut context).is_err());
    let doc = Pandoc {
        blocks: vec![Block::Para(vec![Inline::Str("abcdefgh".into())])],
        ..Pandoc::default()
    };
    assert!(write("tei", &doc, &mut context).is_err());
}

#[test]
fn table_size_is_checked_before_padding() {
    let mut context = Context::pure();
    context.limits.max_table_cells = 15;
    let row = format!("<row>{}</row>", "<entry/>".repeat(8));
    let input = format!("<table><tgroup><tbody>{row}{row}</tbody></tgroup></table>");
    assert!(read("docbook", &input, &mut context).is_err());
    let row = Row {
        cells: vec![Cell {
            row_span: i64::MAX,
            col_span: i64::MAX,
            ..Cell::default()
        }],
        ..Row::default()
    };
    let doc = Pandoc {
        blocks: vec![Block::Table(Box::new(Table {
            head: TableHead {
                rows: vec![row],
                ..TableHead::default()
            },
            ..Table::default()
        }))],
        ..Pandoc::default()
    };
    assert!(write("docbook5", &doc, &mut context).is_err());
}

#[test]
fn readers_on_two_mib_stack() {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            let mut context = Context::pure();
            let nested = |tag: &str, depth: usize| {
                format!(
                    "{}x{}",
                    format!("<{tag}>").repeat(depth),
                    format!("</{tag}>").repeat(depth)
                )
            };
            for format in ["jats", "bits", "docbook"] {
                assert!(read(format, &nested("sec", 126), &mut context).is_ok());
                assert!(read(format, &nested("sec", 8192), &mut context).is_err());
            }
            for tag in ["blockquote", "warning", "figure", "div"] {
                assert!(read("docbook", &nested(tag, 128), &mut context).is_ok());
            }
            let list = "<list><list-item>".repeat(64) + "x" + &"</list-item></list>".repeat(64);
            assert!(read("jats", &list, &mut context).is_ok());
            let mixed = "<sec><disp-quote><list><list-item>".repeat(32)
                + "x"
                + &"</list-item></list></disp-quote></sec>".repeat(32);
            assert!(read("jats", &mixed, &mut context).is_ok());
            let fb2 = format!(
                "<FictionBook><body>{}</body></FictionBook>",
                nested("section", 124)
            );
            assert!(read("fb2", &fb2, &mut context).is_ok());
            assert!(
                read(
                    "jats",
                    &format!("<p>{}</p>", nested("italic", 126)),
                    &mut context
                )
                .is_ok()
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn writers_on_two_mib_stack() {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            let mut context = Context::pure();
            let nested = |depth| Pandoc {
                blocks: vec![
                    (0..depth).fold(Block::Para(vec![Inline::Str("x".into())]), |b, _| {
                        Block::BlockQuote(vec![b])
                    }),
                ],
                ..Pandoc::default()
            };
            let valid = nested(126);
            let too_deep = nested(512);
            for format in ["jats", "docbook", "tei", "fb2"] {
                assert!(write(format, &valid, &mut context).is_ok());
                assert!(write(format, &too_deep, &mut context).is_err());
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn cyclic_notes_stop() {
    let doc = read("jats", "<p><xref ref-type='fn' rid='n'>1</xref></p><fn-group><fn id='n'><p><xref ref-type='fn' rid='n'>1</xref></p></fn></fn-group>", &mut Context::pure()).unwrap();
    let [Block::Para(body)] = doc.blocks.as_slice() else {
        unreachable!()
    };
    let [Inline::Note(note)] = body.as_slice() else {
        unreachable!()
    };
    let [Block::Para(inlines)] = note.as_slice() else {
        unreachable!()
    };
    assert!(matches!(inlines.as_slice(), [Inline::Link(..)]));
}

#[test]
fn fb2_requires_one_root() {
    let mut context = Context::pure();
    for text in ["", "only text", "<!--comment-->"] {
        assert!(
            matches!(read("fb2", text, &mut context), Err(Error::Xml { message, .. })
            if message == "XML document has no root element")
        );
    }
    assert!(
        matches!(read("fb2", "<FictionBook/><FictionBook/>", &mut context),
        Err(Error::Xml { message, .. }) if message == "unexpected content after the root XML element")
    );
}

#[test]
fn fb2_writer_budgets_decoded_resources() {
    let mut context = Context::pure();
    context.limits.max_resource_size = 3;
    context.media.insert(
        "image",
        randoc_core::MediaItem {
            mime_type: "image/png".into(),
            contents: vec![1, 2],
        },
    );
    let image = Inline::Image(Attr::default(), vec![], Target::new("image", ""));
    let doc = Pandoc {
        blocks: vec![Block::Para(vec![image.clone(), image])],
        ..Pandoc::default()
    };
    assert!(write("fb2", &doc, &mut context).is_err());
}

#[test]
fn bits_toc_preserves_heading_levels() {
    let text = "<toc-group><toc-title-group><title>Contents</title></toc-title-group><toc><toc-title-group><title>Book</title></toc-title-group><toc-div><toc-title-group><title>Part</title></toc-title-group><toc-entry><label>1</label><title>Chapter</title><nav-pointer rid='c'/></toc-entry></toc-div></toc></toc-group>";
    let doc = read("bits", text, &mut Context::pure()).unwrap();
    let headings: Vec<_> = doc
        .blocks
        .iter()
        .map(|block| match block {
            Block::Header(level, _, content) => (*level, randoc_core::shared::stringify(content)),
            _ => (0, String::new()),
        })
        .collect();
    assert_eq!(
        headings,
        [
            (1, "Contents".into()),
            (2, "Book".into()),
            (3, "Part".into()),
            (4, "Chapter".into())
        ]
    );
}

#[test]
fn docbook_cals_keeps_cell_spans() {
    let text = "<table><tgroup cols='3'><colspec colname='a' colwidth='1*'/><colspec colname='b' colwidth='2*'/><colspec colname='c'/><tbody><row><entry namest='a' nameend='b' morerows='1'>span</entry><entry>first</entry></row><row><entry>second</entry></row></tbody></tgroup></table>";
    let doc = read("docbook", text, &mut Context::pure()).unwrap();
    let [Block::Table(table)] = doc.blocks.as_slice() else {
        unreachable!()
    };
    let rows = &table.bodies[0].body;
    assert_eq!(
        (rows[0].cells[0].row_span, rows[0].cells[0].col_span),
        (2, 2)
    );
    assert_eq!(rows[1].cells.len(), 1);
}

#[test]
fn writers_escape_xml_text() {
    let doc = Pandoc {
        blocks: vec![Block::Para(vec![Inline::Str("'\"<&>".into())])],
        ..Pandoc::default()
    };
    let mut context = Context::pure();
    for format in [
        "docbook4",
        "docbook5",
        "jats",
        "jats_articleauthoring",
        "tei",
    ] {
        assert!(
            write(format, &doc, &mut context)
                .unwrap()
                .contains("'&quot;&lt;&amp;&gt;")
        );
    }
    assert!(
        write("fb2", &doc, &mut context)
            .unwrap()
            .contains("&#39;&quot;&lt;&amp;&gt;")
    );
}

#[test]
fn docbook_widest_row_sets_column_count() {
    for (group, expected) in [
        (
            "<tgroup cols='2'><row><entry/><entry/><entry>kept</entry></row></tgroup>",
            3,
        ),
        ("<tgroup cols='4'><row><entry/><entry/></row></tgroup>", 2),
        (
            "<tgroup cols='2'><colspec colwidth='1*'/><colspec colwidth='1*'/><colspec colwidth='2*'/><row><entry/></row></tgroup>",
            3,
        ),
        ("<tgroup cols='2'><tbody/></tgroup>", 0),
        (
            "<thead><tr><th/><th/><th/></tr></thead><tbody><tr><td/></tr></tbody>",
            3,
        ),
        ("<tgroup cols='2'><colspec/><row><entry/></row></tgroup>", 1),
        (
            "<tgroup cols='3'><colspec align='left'/><colspec align='right'/><row><entry/></row></tgroup>",
            1,
        ),
    ] {
        let doc = read(
            "docbook",
            &format!("<table>{group}</table>"),
            &mut Context::pure(),
        )
        .unwrap();
        let Block::Table(table) = &doc.blocks[0] else {
            unreachable!()
        };
        assert_eq!(table.col_specs.len(), expected, "{group}");
        if group.contains("kept") {
            assert_eq!(
                table.bodies[0].body[0].cells[2].content,
                vec![plain("kept")]
            );
        }
        if group.contains("2*") {
            assert_eq!(
                table
                    .col_specs
                    .iter()
                    .map(|col| col.width)
                    .collect::<Vec<_>>(),
                [0.25, 0.25, 0.5].map(ColWidth::ColWidth)
            );
        }
    }
}

fn plain(value: &str) -> Block {
    Block::Plain(vec![Inline::Str(value.into())])
}

fn para(value: &str) -> Block {
    Block::Para(vec![Inline::Str(value.into())])
}

#[test]
fn jats_back_sections_and_references() {
    for (xml, expected) in [
        (
            "<back><ack><title>Ack</title><p>x</p></ack></back>",
            vec![2],
        ),
        ("<back><sec><title>S</title></sec></back>", vec![2]),
        ("<ack><sec><title>S</title></sec></ack>", vec![2]),
        (
            "<back><app-group><app><title>A</title></app></app-group></back>",
            vec![3],
        ),
        (
            "<article><body><sec><title>S</title></sec></body><back><sec><title>B</title></sec></back></article>",
            vec![1, 2],
        ),
        (
            "<back><ref-list><title>R</title></ref-list><sec><ref-list><title>S</title></ref-list></sec></back>",
            vec![1, 1],
        ),
    ] {
        let doc = read("jats", xml, &mut Context::pure()).unwrap();
        let levels = doc
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Header(level, _, _) => Some(*level),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(levels, expected, "{xml}");
    }
}

#[test]
fn jats_table_bodies_and_clamped_spans() {
    let doc = read("jats", "<table><tr><td>a</td></tr></table><table><tbody><tr><td rowspan='300000' colspan='300000'>b</td></tr></tbody></table>", &mut Context::pure()).unwrap();
    let [Block::Table(no_body), Block::Table(spanning)] = doc.blocks.as_slice() else {
        unreachable!()
    };
    assert!(no_body.bodies.is_empty());
    let cell = &spanning.bodies[0].body[0].cells[0];
    assert_eq!((cell.row_span, cell.col_span), (1, 1));
    assert_eq!(cell.content, vec![plain("b")]);
}

#[test]
fn restricted_xml_node_limit() {
    let mut context = Context::pure();
    context.limits = randoc_core::Limits::restricted();
    let input = "<p/>".repeat(context.limits.max_table_cells * 5 + 1);
    assert!(
        matches!(read("jats", &input, &mut context), Err(Error::Xml { message, .. }) if message == "XML DOM allocation exceeded limit")
    );
}

#[test]
fn jats_boxed_text_skips_title() {
    let doc = read(
        "jats",
        "<boxed-text><title>B</title><p>x</p></boxed-text>",
        &mut Context::pure(),
    )
    .unwrap();
    assert_eq!(doc.blocks, vec![para("x")]);
}

#[test]
fn xml_link_target_fallbacks() {
    for (format, xml, urls) in [
        (
            "docbook",
            "<para><link>x</link><ulink>y</ulink><link href=''>z</link></para>",
            vec!["#", "", ""],
        ),
        (
            "jats",
            "<p xmlns:z='http://www.w3.org/1999/xlink'><ext-link>x</ext-link><ext-link href='y'>y</ext-link><ext-link xlink:href='u'>u</ext-link><ext-link z:href='z' z:title='Z'>z</ext-link><ext-link rid='id'/></p>",
            vec!["#", "#", "#", "z", "#id"],
        ),
    ] {
        let doc = read(format, xml, &mut Context::pure()).unwrap();
        let Block::Para(lines) = &doc.blocks[0] else {
            unreachable!()
        };
        let targets = lines
            .iter()
            .filter_map(|inline| match inline {
                Inline::Link(_, _, target) => Some(target.url.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(targets, urls);
        if format == "jats" {
            assert!(matches!(&lines[3], Inline::Link(_, _, target) if target.title == "Z"));
            assert!(
                matches!(&lines[4], Inline::Link(_, label, _) if label == &[Inline::Str("#id".into())])
            );
        }
    }
}

#[test]
fn docbook_rejected_entities() {
    for entity in ["Ugr", "Uacgr", "Udigr", "b.Upsi"] {
        let input = format!("<para>&{entity};</para>");
        let expected = format!(
            "1:7 (6)-1:{} ({}): Unresolved entity &{entity};",
            9 + entity.len(),
            8 + entity.len()
        );
        let error = read("docbook", &input, &mut Context::pure()).unwrap_err();
        assert!(matches!(error, Error::Xml { message, .. } if message == expected));
    }
}

#[test]
fn docbook_xref_labels() {
    let doc = read("docbook", "<sect1 id='s'/><figure id='f'><title>Figure title</title></figure><para id='p'>P</para><note id='n'/><step id='z'/><para><anchor id='a'/><xref linkend='missing'/><xref linkend='s'/><xref linkend='f'/><xref linkend='p'/><xref linkend='n'/><xref linkend='z'/><xref linkend='a'/></para>", &mut Context::pure()).unwrap();
    let Some(Block::Para(lines)) = doc.blocks.last() else {
        unreachable!()
    };
    let labels = lines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link(_, label, _) => Some(label.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let str_ = |value: &str| Inline::Str(value.into());
    assert_eq!(
        labels,
        vec![
            vec![str_("???")],
            vec![str_("???")],
            vec![str_("Figure"), Inline::Space, str_("title")],
            vec![str_("para_title")],
            vec![str_("note_title")],
            vec![str_("step_title")],
            vec![str_("anchor_title")],
        ]
    );
}

#[test]
fn docbook_author_metadata_shapes() {
    let name = |text: &str| MetaValue::MetaInlines(vec![Inline::Str(text.into())]);
    for (xml, expected) in [
        (
            "<info><author><firstname>J</firstname></author><author><firstname>K</firstname></author></info>",
            name("K"),
        ),
        (
            "<info><authorgroup><author><firstname>J</firstname></author></authorgroup></info>",
            MetaValue::MetaList(vec![name("J")]),
        ),
        (
            "<article><author><firstname>J</firstname><surname>D</surname></author></article>",
            MetaValue::MetaInlines(vec![
                Inline::Str("J".into()),
                Inline::Space,
                Inline::Str("D".into()),
            ]),
        ),
        (
            "<info><author><personname><firstname>J</firstname><surname>D</surname></personname></author></info>",
            name("JD"),
        ),
    ] {
        let doc = read("docbook", xml, &mut Context::pure()).unwrap();
        assert_eq!(doc.meta.get("author"), Some(&expected), "{xml}");
    }
}

#[test]
fn docbook_block_and_emphasis_elements() {
    let input = "<para>a<emphasis role='bold'>b</emphasis><emphasis role='underline'>u</emphasis><note><para>n</para></note><bridgehead>T</bridgehead><literallayout>x\n  y</literallayout>z</para><formalpara><title>F</title><para>f</para></formalpara><sidebar><title>S</title><para>s</para></sidebar><abstract><para>p</para></abstract><glosslist><glossentry><glossterm>t</glossterm><glossdef><para>d</para></glossdef></glossentry></glosslist><qandaset><qandaentry><question><para>q</para></question><answer><para>a</para></answer></qandaentry></qandaset>";
    let doc = read("docbook", input, &mut Context::pure()).unwrap();
    let str_ = |value: &str| Inline::Str(value.into());
    let strong = |value| Inline::Strong(vec![str_(value)]);
    let div = |class: &str, blocks| Block::Div(Attr::new("", vec![class.into()], vec![]), blocks);
    assert_eq!(
        doc.blocks,
        vec![
            Block::Para(vec![
                str_("a"),
                strong("b"),
                Inline::Underline(vec![str_("u")])
            ]),
            div("note", vec![div("title", vec![]), para("n")]),
            Block::Para(vec![strong("T")]),
            Block::LineBlock(vec![vec![str_("x")], vec![str_("\u{a0}\u{a0}y")]]),
            para("z"),
            div(
                "formalpara",
                vec![div("title", vec![plain("F")]), para("f")]
            ),
            div("sidebar", vec![div("title", vec![plain("S")]), para("s")]),
            Block::BlockQuote(vec![para("p")]),
            Block::DefinitionList(vec![(vec![str_("t")], vec![vec![para("d")]])]),
            Block::Para(vec![strong("Q:"), str_(" q")]),
            Block::Para(vec![strong("A:"), str_(" a")]),
        ]
    );
}

#[test]
fn xml_depth_uses_context_limit() {
    std::thread::Builder::new()
        .stack_size(32 << 20)
        .spawn(|| {
            let mut context = Context::pure();
            context.limits.max_nesting_depth = 16_384;
            let quotes = "<para>".to_owned()
                + &"<blockquote>".repeat(1_000)
                + "<para>x</para>"
                + &"</blockquote>".repeat(1_000)
                + "</para>";
            let doc = read("docbook", &quotes, &mut context).unwrap();
            let mut blocks = doc.blocks.as_slice();
            for _ in 0..1_000 {
                let [Block::BlockQuote(inner)] = blocks else {
                    unreachable!()
                };
                blocks = inner;
            }
            assert_eq!(blocks, &[para("x")]);
            for (format, tag) in [
                ("docbook", "<blockquote>"),
                ("jats", "<disp-quote>"),
                ("tei", "<quote>"),
                ("fb2", "<cite>"),
            ] {
                assert_eq!(
                    write(format, &doc, &mut context)
                        .unwrap()
                        .matches(tag)
                        .count(),
                    1_000
                );
            }
            let lists =
                "<list><list-item>".repeat(60) + "<p>x</p>" + &"</list-item></list>".repeat(60);
            let doc = read("jats", &lists, &mut context).unwrap();
            let mut blocks = doc.blocks.as_slice();
            for _ in 0..60 {
                let [Block::BulletList(items)] = blocks else {
                    unreachable!()
                };
                assert_eq!(items.len(), 1);
                blocks = &items[0];
            }
            assert_eq!(blocks, &[para("x")]);
            let sections = "<sec><title>S</title>".repeat(300) + &"</sec>".repeat(300);
            let doc = read("jats", &sections, &mut context).unwrap();
            assert_eq!(doc.blocks.len(), 300);
            assert!(matches!(doc.blocks[299], Block::Header(300, ..)));
            let info = "<info>".repeat(1_000) + "<title>T</title>" + &"</info>".repeat(1_000);
            let doc = read("docbook", &info, &mut context).unwrap();
            assert_eq!(
                doc.meta.get("title"),
                Some(&MetaValue::MetaInlines(vec![Inline::Str("T".into())]))
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn fb2_writer_budgets_generated_prefixes() {
    let mut context = Context::pure();
    context.limits.max_input_size = 200;
    let nested = |wrap: fn(Vec<Block>) -> Block| {
        (0..12).fold(para("x"), |block, _| wrap(vec![para("x"), block]))
    };
    for block in [
        nested(|blocks| Block::BulletList(vec![blocks])),
        nested(|blocks| Block::DefinitionList(vec![(vec![], vec![blocks])])),
    ] {
        let doc = Pandoc {
            blocks: vec![block],
            ..Pandoc::default()
        };
        assert!(
            matches!(write("fb2", &doc, &mut context), Err(Error::App(message)) if message.contains("FB2 list and definition prefix size"))
        );
    }
}

fn write_with(format: &str, doc: &Pandoc, options: &WriterOptions) -> String {
    let output = writer(format)
        .unwrap()
        .write(doc, options, &mut Context::pure())
        .unwrap();
    String::from_utf8(output.into_bytes()).unwrap()
}

#[test]
fn tei_code_blocks_use_the_first_language_class() {
    for (classes, expected) in [
        (vec!["numberLines", "python"], "python"),
        (vec!["Python"], "Python"),
        (vec!["py", "rust"], "python"),
        (vec!["sh"], "bash"),
        (vec!["foo"], ""),
    ] {
        let classes = classes.into_iter().map(Text::from).collect();
        let doc = Pandoc {
            meta: Meta::default(),
            blocks: vec![Block::CodeBlock(
                Attr::new("", classes, Vec::new()),
                "x".into(),
            )],
        };
        assert_eq!(
            write_with("tei", &doc, &WriterOptions::default()),
            format!("<ab type='codeblock {expected}'>\nx\n</ab>")
        );
    }
}

#[test]
fn jats_element_citations_list_cited_references_in_metadata_order() {
    let reference = |id: &str| {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("id".to_owned(), MetaValue::MetaString(id.into()));
        fields.insert(
            "type".to_owned(),
            MetaValue::MetaString("article-journal".into()),
        );
        fields.insert("title".to_owned(), MetaValue::MetaString(id.to_uppercase()));
        MetaValue::MetaMap(fields)
    };
    let cite = |id: &str| Citation {
        id: id.into(),
        prefix: Vec::new(),
        suffix: Vec::new(),
        mode: CitationMode::NormalCitation,
        note_num: 0,
        hash: 0,
    };
    let mut meta = Meta::default();
    meta.insert(
        "references",
        MetaValue::MetaList(vec![reference("c"), reference("b"), reference("a")]),
    );
    let doc = Pandoc {
        meta,
        blocks: vec![
            Block::Para(vec![Inline::Cite(vec![cite("b"), cite("c")], Vec::new())]),
            Block::Div(
                Attr::new("refs", Vec::new(), Vec::new()),
                vec![Block::Div(
                    Attr::new("ref-b", Vec::new(), Vec::new()),
                    vec![para("B")],
                )],
            ),
        ],
    };
    let mut options = WriterOptions::default();
    options
        .extensions
        .enable(randoc_core::Extension::ElementCitations);
    options.template = Some(
        randoc_templates::Template::compile::<String>("t", "$back$", 8, &mut |path| {
            Err(path.to_owned())
        })
        .unwrap(),
    );
    let output = write_with("jats", &doc, &options);
    let ids = output
        .match_indices("<ref id=\"")
        .map(|(start, _)| &output[start + 9..start + 14])
        .collect::<Vec<_>>();
    assert_eq!(ids, ["ref-c", "ref-b"], "{output}");
    assert!(
        output.contains("<article-title>C</article-title>"),
        "{output}"
    );
    assert!(!output.contains("mixed-citation"), "{output}");
}

#[test]
fn docbook_gives_the_title_only_to_inline_images() {
    let image = Inline::Image(
        Attr::default(),
        vec![Inline::Str("Alt".into())],
        Target::new("a.jpg", "Title"),
    );
    let inline = Pandoc {
        meta: Meta::default(),
        blocks: vec![Block::Para(vec![image.clone()])],
    };
    assert!(
        write("docbook5", &inline, &mut Context::pure())
            .unwrap()
            .contains("<objectinfo>")
    );
    let figure = Pandoc {
        meta: Meta::default(),
        blocks: vec![Block::Figure(
            Attr::default(),
            Caption::default(),
            vec![Block::Plain(vec![image])],
        )],
    };
    assert!(
        !write("docbook5", &figure, &mut Context::pure())
            .unwrap()
            .contains("<objectinfo>")
    );
}

#[test]
fn prefixes_ids_and_numbers_jats_sections() {
    let header = |level, id: &str, classes| {
        Block::Header(
            level,
            Attr::new(id, classes, Vec::new()),
            vec![Inline::Str(id.into())],
        )
    };
    let doc = Pandoc {
        blocks: vec![
            header(1, "a", vec![]),
            header(2, "b", vec![]),
            Block::Para(vec![Inline::Link(
                Attr::default(),
                vec![Inline::Str("Link".into())],
                Target::new("#a", ""),
            )]),
            header(1, "c", vec!["unnumbered".into()]),
            header(1, "d", vec![]),
            header(3, "e", vec![]),
        ],
        ..Pandoc::default()
    };
    let mut options = WriterOptions::default();
    options.identifier_prefix = "p-".into();
    options.number_sections = true;
    for dialect in [
        "jats",
        "jats_archiving",
        "jats_publishing",
        "jats_articleauthoring",
    ] {
        let output = write_with(dialect, &doc, &options);
        for (id, number) in [("a", "1"), ("b", "1.1"), ("d", "2"), ("e", "2.0.1")] {
            assert!(
                output
                    .split_once(&format!("id=\"p-{id}\">"))
                    .unwrap()
                    .1
                    .trim_start()
                    .starts_with(&format!("<label>{number}</label>")),
                "{dialect}\n{output}",
            );
        }
        assert!(output.contains("<sec id=\"p-c\">\n  <title>c"));
        assert!(output.contains("rid=\"p-a\""));
    }
    for (dialect, target) in [
        ("docbook4", "linkend=\"p-a\""),
        ("docbook5", "linkend=\"p-a\""),
        ("tei", "target=\"#p-a\""),
    ] {
        let output = write_with(dialect, &doc, &options);
        assert!(output.contains("id=\"p-a\""));
        assert!(output.contains(target));
    }
}

#[test]
fn jats_places_bibliography_title_and_reference_fields_in_back() {
    let text = |value: &str| MetaValue::MetaString(value.into());
    let map = |fields: &[(&str, &str)]| {
        MetaValue::MetaMap(
            fields
                .iter()
                .map(|(key, value)| ((*key).into(), text(value)))
                .collect(),
        )
    };
    let MetaValue::MetaMap(mut reference) = map(&[
        ("id", "b"),
        ("type", "book"),
        ("title", "Book"),
        ("issued", "2016-2"),
        ("accessed", "2026-11-24"),
        ("pmid", "42"),
    ]) else {
        unreachable!()
    };
    reference.insert(
        "author".into(),
        MetaValue::MetaList(vec![
            map(&[
                ("family", "Bray"),
                ("given", "Jan"),
                ("non-dropping-particle", "de"),
            ]),
            map(&[
                ("family", "Beethoven"),
                ("given", "Ludwig"),
                ("dropping-particle", "van"),
            ]),
            text("宮水 三葉"),
            map(&[("literal", "others")]),
        ]),
    );
    let mut meta = Meta::default();
    meta.insert(
        "references",
        MetaValue::MetaList(vec![MetaValue::MetaMap(reference)]),
    );
    let doc = Pandoc {
        meta,
        blocks: vec![
            Block::Para(vec![Inline::Cite(
                vec![Citation {
                    id: "b".into(),
                    prefix: vec![],
                    suffix: vec![],
                    mode: CitationMode::NormalCitation,
                    note_num: 0,
                    hash: 0,
                }],
                vec![Inline::Str("Citation".into())],
            )]),
            Block::Header(
                1,
                Attr::new("bibliography", vec![], vec![]),
                vec![Inline::Str("Works".into())],
            ),
            Block::Div(Attr::new("refs", vec![], vec![]), vec![]),
        ],
    };
    let mut options = WriterOptions::default();
    options.template = Some(
        randoc_templates::Template::compile::<String>("t", "$body$\n$back$", 8, &mut |path| {
            Err(path.to_owned())
        })
        .unwrap(),
    );
    options
        .extensions
        .enable(randoc_core::Extension::ElementCitations);
    let output = write_with("jats", &doc, &options);
    assert!(!output.contains("<sec"));
    for expected in [
        "<ref-list>\n  <title>Works</title>",
        "<name><surname>de Bray</surname><given-names>Jan</given-names></name>",
        "<name><surname>Beethoven</surname><given-names>Ludwig van</given-names></name>",
        "<string-name>宮水 三葉</string-name>",
        "<etal/>",
        "<source>Book</source>",
        "<year iso-8601-date=\"2016-02\">2016</year><month>02</month>",
        "<date-in-citation content-type=\"access-date\"><year iso-8601-date=\"2026-11-24\">2026</year><month>11</month><day>24</day></date-in-citation>",
        "<pub-id pub-id-type=\"pmid\">42</pub-id>",
    ] {
        assert!(output.contains(expected), "{expected}\n{output}");
    }
}

#[test]
fn jats_keeps_nested_references_in_their_section() {
    let doc = Pandoc {
        blocks: vec![
            Block::Header(
                1,
                Attr::new("research", vec![], vec![]),
                vec![Inline::Str("Research".into())],
            ),
            Block::Header(
                2,
                Attr::new("sources", vec![], vec![]),
                vec![Inline::Str("Sources".into())],
            ),
            Block::Div(
                Attr::new("refs", vec![], vec![]),
                vec![Block::Div(
                    Attr::new("ref-a", vec![], vec![]),
                    vec![para("Entry.")],
                )],
            ),
        ],
        ..Pandoc::default()
    };
    let output = write_with("jats", &doc, &WriterOptions::default());
    assert!(output.contains("<title>Research</title>"));
    assert!(output.contains("<title>Sources</title>\n    <ref-list>"));
    assert!(output.contains("<mixed-citation>Entry.</mixed-citation>"));
}
