#![allow(clippy::unwrap_used)]

use randoc_ast::*;
use randoc_core::{Context, Reader, ReaderOptions, Sources, WrapOption, WriterOptions};
use randoc_templates::Template;

fn read(format: &str, input: &str) -> Pandoc {
    let Reader::Text(reader) = crate::reader(format).unwrap() else {
        unreachable!();
    };
    reader
        .read(
            &Sources::from_text(input),
            &ReaderOptions::default(),
            &mut Context::pure(),
        )
        .unwrap()
}

fn write(format: &str, doc: &Pandoc, options: &WriterOptions) -> String {
    let output = crate::writer(format)
        .unwrap()
        .write(doc, options, &mut Context::pure())
        .unwrap();
    String::from_utf8(output.into_bytes()).unwrap()
}

fn string(text: &str) -> Inline {
    Inline::Str(text.into())
}

fn para(text: &str) -> Block {
    Block::Para(vec![string(text)])
}

fn doc(blocks: Vec<Block>) -> Pandoc {
    Pandoc {
        blocks,
        ..Pandoc::default()
    }
}

fn options() -> WriterOptions {
    let mut options = WriterOptions::default();
    options.wrap_text = WrapOption::WrapNone;
    options
}

fn template(source: &str) -> WriterOptions {
    let mut options = options();
    options.template = Some(
        Template::compile("test", source, 128, &mut |_| {
            Ok::<_, std::convert::Infallible>(String::new())
        })
        .unwrap(),
    );
    options
}

#[test]
fn docbook_list_title() {
    let parsed = read(
        "docbook",
        r#"<orderedlist id="steps" role="task"><title>Steps</title><listitem><para>Open.</para></listitem></orderedlist>"#,
    );
    assert_eq!(
        parsed.blocks,
        [Block::Div(
            Attr::new("steps", vec![], vec![("role".into(), "task".into())]),
            vec![
                Block::Div(
                    Attr::new("", vec!["title".into()], vec![]),
                    vec![Block::Plain(vec![string("Steps")])],
                ),
                Block::OrderedList(
                    ListAttributes {
                        style: ListNumberStyle::Decimal,
                        ..ListAttributes::default()
                    },
                    vec![vec![para("Open.")]],
                )
            ]
        )]
    );
}

#[test]
fn docbook_mathphrase_preserves_spaces() {
    let parsed = read(
        "docbook",
        "<equation><mathphrase>\n x &lt; y\n</mathphrase></equation><informalequation><mathphrase> z </mathphrase></informalequation>",
    );
    let math = |tex: &str| Block::Para(vec![Inline::Math(MathType::DisplayMath, tex.into())]);
    assert_eq!(
        parsed.blocks,
        [
            math("\n x < y\n"),
            Block::Div(
                Attr::new("", vec!["informalequation".into()], vec![]),
                vec![math(" z ")]
            )
        ]
    );
}

#[test]
fn docbook_release_info() {
    let parsed = read(
        "docbook",
        "<book><bookinfo><releaseinfo>2.4</releaseinfo></bookinfo></book>",
    );
    assert_eq!(
        parsed.meta.get("releaseinfo"),
        Some(&MetaValue::MetaInlines(vec![string("2.4")]))
    );
}

#[test]
fn docbook_emphasis_preserves_edge_spaces() {
    let parsed = read(
        "docbook",
        "<para>\n<emphasis> Ready </emphasis>now\n</para>",
    );
    assert_eq!(
        parsed.blocks,
        [Block::Para(vec![
            Inline::Emph(vec![Inline::Space, string("Ready"), Inline::Space]),
            string("now")
        ])]
    );
}

#[test]
fn jats_italic_preserves_edge_spaces() {
    let parsed = read("jats", "<p>\n<italic> Ready  </italic>now\n</p>");
    assert_eq!(
        parsed.blocks,
        [Block::Para(vec![
            Inline::Emph(vec![Inline::Space, string("Ready"), Inline::Space]),
            string("now")
        ])]
    );
}

#[test]
fn docbook_processing_instruction_scales_cals_widths() {
    let table = r#"<tgroup cols="2"><colspec colwidth="1*"/><colspec colwidth="3*"/><tbody><row><entry>A</entry><entry>B</entry></row></tbody></tgroup>"#;
    let parsed = read(
        "docbook",
        &format!(
            r#"<informaltable><!-- <?dbfo table-width="90%"?> --><?dbfo table-width='40%'?>{table}</informaltable><informaltable>{table}</informaltable>"#
        ),
    );
    for (block, widths) in parsed.blocks.iter().zip([[0.1, 0.3], [0.25, 0.75]]) {
        let Block::Table(table) = block else {
            unreachable!();
        };
        assert_eq!(table.col_specs.len(), 2);
        for (column, expected) in table.col_specs.iter().zip(widths) {
            let ColWidth::ColWidth(width) = column.width else {
                unreachable!();
            };
            assert!((width - expected).abs() < f64::EPSILON);
        }
    }
    assert_eq!(parsed.blocks.len(), 2);
}

#[test]
fn jats_reference_id_removes_one_prefix() {
    let parsed = read(
        "jats",
        r#"<ref-list><ref id="ref-ref-key"/><ref id="plain"/></ref-list>"#,
    );
    assert_eq!(
        parsed.meta.get("references"),
        Some(&MetaValue::MetaList(
            ["ref-key", "plain"]
                .map(|id| MetaValue::MetaMap(
                    [("id".into(), MetaValue::MetaString(id.into()))].into()
                ))
                .into()
        ))
    );
}

#[test]
fn docbook_image_title_from_object_info() {
    let parsed = read(
        "docbook",
        r#"<para><inlinemediaobject><imageobject><objectinfo><title>Diagram</title></objectinfo><imagedata fileref="figure.png"/></imageobject><textobject><phrase>Alt</phrase></textobject></inlinemediaobject></para>"#,
    );
    assert_eq!(
        parsed.blocks,
        [Block::Para(vec![Inline::Image(
            Attr::default(),
            vec![string("Alt")],
            Target::new("figure.png", "Diagram")
        )])]
    );
}

#[test]
fn jats_article_meta_root_reads_permissions() {
    let parsed = read(
        "jats",
        r#"<article-meta xmlns:ali="http://www.niso.org/schemas/ali/1.0/"><title-group><article-title/></title-group><permissions><copyright-holder>A</copyright-holder><copyright-year>2025</copyright-year><copyright-statement>© A</copyright-statement><license license-type="open"><ali:license_ref>https://example.org/license</ali:license_ref><license-p>Free to share.</license-p></license></permissions></article-meta>"#,
    );
    let map = |pairs: &[(&str, &str)]| {
        MetaValue::MetaMap(
            pairs
                .iter()
                .map(|(key, value)| ((*key).into(), MetaValue::MetaString((*value).into())))
                .collect(),
        )
    };
    assert!(parsed.blocks.is_empty());
    assert_eq!(parsed.meta.0.len(), 2);
    assert_eq!(
        parsed.meta.get("copyright"),
        Some(&map(&[
            ("holder", "A"),
            ("year", "2025"),
            ("statement", "© A")
        ]))
    );
    assert_eq!(
        parsed.meta.get("license"),
        Some(&map(&[
            ("type", "open"),
            ("link", "https://example.org/license"),
            ("text", "Free to share.")
        ]))
    );
}

#[test]
fn bits_book_back_adds_heading_level() {
    let parsed = read(
        "bits",
        r#"<book><book-back><ack id="thanks"><title>Thanks</title><p>Help.</p></ack></book-back></book>"#,
    );
    assert_eq!(
        parsed.blocks,
        [
            Block::Header(
                2,
                Attr::new("thanks", vec![], vec![]),
                vec![string("Thanks")]
            ),
            para("Help."),
        ]
    );
}

#[test]
fn jats_credit_roles_fill_template_names() {
    let mut doc = doc(vec![]);
    let roles = [
        "software",
        "writing-original-draft",
        "writing-review-editing",
        "unknown",
    ]
    .map(|role| MetaValue::MetaMap([("credit".into(), MetaValue::MetaString(role.into()))].into()));
    doc.meta.insert(
        "author",
        MetaValue::MetaList(vec![MetaValue::MetaMap(
            [("roles".into(), MetaValue::MetaList(roles.into()))].into(),
        )]),
    );
    let options = template("$for(author)$$for(author.roles)$[$it.credit-name$]$endfor$$endfor$");
    assert_eq!(
        write("jats", &doc, &options),
        "[Software][Writing – original draft][Writing – review &amp; editing][]"
    );
}

#[test]
fn jats_nested_figures_use_fig_group() {
    let figure =
        |id, blocks| Block::Figure(Attr::new(id, vec![], vec![]), Caption::default(), blocks);
    let doc = doc(vec![figure(
        "fig:group",
        vec![figure("fig:item", vec![para("A")])],
    )]);
    assert_eq!(
        write("jats", &doc, &options()),
        "<fig-group id=\"figU003Agroup\">\n  <fig id=\"figU003Aitem\">\n    <p>A</p>\n  </fig>\n</fig-group>"
    );
}

#[test]
fn xml_writers_drop_disallowed_characters() {
    let doc = doc(vec![para("h\u{4}i\u{fffe}\u{ffff}")]);
    for (format, expected) in [
        ("docbook", "<para>\n  hi\n</para>"),
        ("jats", "<p>hi</p>"),
        ("tei", "<p>hi</p>"),
    ] {
        assert_eq!(write(format, &doc, &options()), expected);
    }
}

fn one_cell(blocks: Vec<Block>) -> Block {
    Block::Table(Box::new(Table {
        col_specs: vec![ColSpec::default()],
        bodies: vec![TableBody {
            body: vec![Row {
                cells: vec![Cell {
                    content: blocks,
                    ..Cell::default()
                }],
                ..Row::default()
            }],
            ..TableBody::default()
        }],
        ..Table::default()
    }))
}

#[test]
fn jats_table_wraps_quotes_but_keeps_lists_and_code() {
    let doc = doc(vec![one_cell(vec![
        Block::BlockQuote(vec![para("Quoted.")]),
        Block::BulletList(vec![vec![para("Item.")]]),
        Block::CodeBlock(Attr::default(), "code".into()),
    ])]);
    assert_eq!(
        write("jats", &doc, &options()),
        concat!(
            "<table-wrap>\n  <table>\n    <tbody>\n      <tr>\n",
            "        <td><p specific-use=\"wrapper\">\n",
            "          <disp-quote>\n            <p>Quoted.</p>\n          </disp-quote>\n        </p>\n",
            "        <list list-type=\"bullet\">\n          <list-item>\n",
            "            <p>Item.</p>\n          </list-item>\n        </list>\n",
            "        <preformat>code</preformat></td>\n      </tr>\n    </tbody>\n  </table>\n</table-wrap>"
        )
    );
}

#[test]
fn jats_line_breaks_follow_the_xml_context() {
    let lines = vec![string("A"), Inline::LineBreak, string("B")];
    let mut doc = doc(vec![
        Block::Header(1, Attr::new("s", vec![], vec![]), lines.clone()),
        Block::Para(lines.clone()),
        one_cell(vec![Block::Plain(
            [lines.clone(), vec![Inline::Strong(lines.clone())]].concat(),
        )]),
    ]);
    doc.meta.insert("title", MetaValue::MetaInlines(lines));
    let rendered = write(
        "jats",
        &doc,
        &template("<article-title>$title$</article-title>\n$body$"),
    );
    assert!(rendered.starts_with("<article-title>A<break/>B</article-title>\n<sec id=\"s\">\n  <title>A<break/>B</title>\n  <p>A\n  B</p>"));
    assert!(rendered.contains("<td>A<break/>B<bold>A\n          B</bold></td>"));
}

#[test]
fn docbook_ascii_uses_uppercase_hex() {
    let mut options = options();
    options.prefer_ascii = true;
    assert_eq!(
        write("docbook", &doc(vec![para("åé漢")]), &options),
        "<para>\n  &#xE5;&#xE9;&#x6F22;\n</para>"
    );
}

#[test]
fn jats_ascii_uses_uppercase_hex() {
    let mut options = options();
    options.prefer_ascii = true;
    assert_eq!(
        write("jats", &doc(vec![para("åé漢")]), &options),
        "<p>&#xE5;&#xE9;&#x6F22;</p>"
    );
}

#[test]
fn jats_escapes_ids_and_cross_references() {
    let attr = |id| Attr::new(id, vec![], vec![]);
    let doc = doc(vec![
        Block::Header(1, attr("a:b"), vec![string("Title")]),
        Block::Para(vec![
            Inline::Span(attr("c.d"), vec![string("Label")]),
            Inline::Link(attr("e:f"), vec![string("See")], Target::new("#a:b", "")),
            Inline::Image(attr("g:h"), vec![], Target::new("a.png", "")),
        ]),
    ]);
    for format in ["jats_archiving", "jats_publishing", "jats_articleauthoring"] {
        assert_eq!(
            write(format, &doc, &options()),
            concat!(
                "<sec id=\"aU003Ab\">\n  <title>Title</title>\n",
                "  <p><styled-content id=\"c.d\">Label</styled-content>",
                "<xref id=\"eU003Af\" alt=\"See\" rid=\"aU003Ab\">See</xref>",
                "<inline-graphic id=\"gU003Ah\" mimetype=\"image\" mime-subtype=\"png\" xlink:href=\"a.png\" /></p>\n</sec>"
            )
        );
    }
}
