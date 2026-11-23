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
