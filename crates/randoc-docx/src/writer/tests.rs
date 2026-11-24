#![allow(clippy::unwrap_used)]

use randoc_ast::Pandoc;
use randoc_core::{BinaryWriter, Context, WriterOptions};

#[test]
fn writer_limits_work_on_a_two_mib_stack() {
    use randoc_ast::{Alignment, Block, Cell, ColSpec, ColWidth, Inline, Row, Table};
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut context = Context::pure();
            let options = WriterOptions::default();
            let writer = super::DocxWriter;
            let mut inline = Inline::Str("text".into());
            for _ in 0..110 {
                inline = Inline::Emph(vec![inline]);
            }
            let mut doc = Pandoc {
                blocks: vec![Block::Para(vec![inline])],
                ..Pandoc::default()
            };
            assert!(writer.write(&doc, &options, &mut context).is_ok());
            let mut block = doc.blocks.pop().unwrap();
            for _ in 0..50 {
                block = Block::BlockQuote(vec![block]);
            }
            doc.blocks.push(block);
            assert!(writer.write(&doc, &options, &mut context).is_err());
            let mut blocks = vec![Block::Para(vec![Inline::Str("Nested note".into())])];
            for _ in 0..50 {
                blocks = vec![Block::Para(vec![Inline::Note(blocks)])];
            }
            doc.blocks = blocks;
            assert!(writer.write(&doc, &options, &mut context).is_ok());

            let mut meta = randoc_ast::MetaValue::MetaBool(false);
            for _ in 0..400 {
                meta = randoc_ast::MetaValue::MetaList(vec![meta]);
            }
            doc.meta.0.insert("nested".into(), meta);
            assert!(writer.write(&doc, &options, &mut context).is_err());
            doc.meta.0.clear();

            let mut table = Table {
                col_specs: vec![
                    ColSpec {
                        align: Alignment::AlignDefault,
                        width: ColWidth::ColWidthDefault
                    };
                    500
                ],
                head: randoc_ast::TableHead {
                    rows: vec![Row::default(); 100],
                    ..Default::default()
                },
                ..Default::default()
            };
            doc.blocks = vec![Block::Table(Box::new(table.clone()))];
            context.limits.max_table_cells = 20_000;
            assert!(writer.write(&doc, &options, &mut context).is_err());
            table.col_specs.truncate(1);
            table.head.rows.truncate(1);
            table.head.rows[0].cells.push(Cell {
                col_span: i64::MAX,
                ..Cell::default()
            });
            doc.blocks = vec![Block::Table(Box::new(table))];
            assert!(writer.write(&doc, &options, &mut context).is_ok());
            context.limits.max_archive_size = 100;
            assert!(
                writer
                    .write(&Pandoc::default(), &options, &mut context)
                    .is_err()
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn writes_sixteen_default_members_in_order() {
    use randoc_archive::xml::{XmlLimits, XmlOptions, parse};
    use randoc_archive::zip::{Archive, ArchiveLimits};

    let output = super::DocxWriter
        .write(
            &Pandoc::default(),
            &WriterOptions::default(),
            &mut Context::pure(),
        )
        .unwrap();
    let mut archive = Archive::open(&output, ArchiveLimits::default()).unwrap();
    let expected = [
        "[Content_Types].xml",
        "_rels/.rels",
        "word/document.xml",
        "word/_rels/document.xml.rels",
        "word/_rels/footnotes.xml.rels",
        "word/numbering.xml",
        "word/styles.xml",
        "word/footnotes.xml",
        "word/comments.xml",
        "docProps/core.xml",
        "docProps/custom.xml",
        "word/settings.xml",
        "docProps/app.xml",
        "word/theme/theme1.xml",
        "word/fontTable.xml",
        "word/webSettings.xml",
    ];
    assert_eq!(archive.entry_names(), expected);
    for name in expected {
        let bytes = archive.read(name).unwrap().unwrap();
        let xml = std::str::from_utf8(&bytes).unwrap();
        if name == "word/settings.xml" {
            assert!(!xml.contains("updateFields"));
        }
        assert!(
            parse(xml, &XmlLimits::default(), XmlOptions::default()).is_ok(),
            "{name}"
        );
    }
}

#[test]
fn uses_custom_template_and_context_terms() {
    use randoc_archive::zip::{Archive, ArchiveLimits};
    use randoc_ast::{Block, Inline, MetaValue};
    use randoc_templates::Template;

    let mut doc = Pandoc {
        blocks: vec![Block::Para(vec![Inline::Str("Body & text".into())])],
        ..Pandoc::default()
    };
    doc.meta
        .0
        .insert("title".into(), MetaValue::MetaString("My title".into()));
    doc.meta.0.insert(
        "abstract".into(),
        MetaValue::MetaString("Summary text".into()),
    );
    let mut ctx = Context::pure();
    ctx.state.translations = Some(
        [
            ("Abstract".into(), "Summary & scope".into()),
            ("Contents".into(), "Index".into()),
            ("ListOfFigures".into(), "Figure index".into()),
            ("ListOfTables".into(), "Table index".into()),
        ]
        .into_iter()
        .collect(),
    );
    let mut options = WriterOptions::default();
    options.table_of_contents = true;
    options.list_of_figures = true;
    options.list_of_tables = true;
    options.template = Some(Template::compile(
        "test",
        concat!(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
            r#"<w:p>$title$<w:r><w:t>$abstract-title$</w:t></w:r></w:p>"#,
            "$toc$$lof$$lot$$abstract$$body$$sectpr$</w:body></w:document>"
        ),
        8,
        &mut |_| Err::<String, &str>("Unexpected partial"),
    ).unwrap());
    let output = super::DocxWriter.write(&doc, &options, &mut ctx).unwrap();
    let mut archive = Archive::open(&output, ArchiveLimits::default()).unwrap();
    let xml = String::from_utf8(archive.read("word/document.xml").unwrap().unwrap()).unwrap();
    for value in [
        "My title",
        "Summary &amp; scope",
        r#"<w:docPartGallery w:val="Table of Contents" />"#,
        ">Table of Contents</w:t>",
        r#"<w:docPartGallery w:val="List of Figures" />"#,
        ">Figure index</w:t>",
        r#"<w:docPartGallery w:val="List of Tables" />"#,
        ">Table index</w:t>",
        "Summary text",
        "Body &amp; text",
    ] {
        assert!(xml.contains(value), "{value}");
    }
    doc.meta.0.insert(
        "toc-title".into(),
        MetaValue::MetaString("Outline & index".into()),
    );
    let output = super::DocxWriter.write(&doc, &options, &mut ctx).unwrap();
    let xml = document_xml(&output, "word/document.xml");
    assert!(xml.contains(r#"<w:docPartGallery w:val="Table of Contents" />"#));
    assert!(xml.contains(">Outline &amp; index</w:t>"));
}

fn document_xml(bytes: &[u8], part: &str) -> String {
    use randoc_archive::zip::{Archive, ArchiveLimits};

    let mut archive = Archive::open(bytes, ArchiveLimits::default()).unwrap();
    String::from_utf8(archive.read(part).unwrap().unwrap()).unwrap()
}

fn on_two_mib_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}

fn write_blocks(blocks: Vec<randoc_ast::Block>) -> String {
    let doc = Pandoc {
        blocks,
        ..Pandoc::default()
    };
    let bytes = super::DocxWriter
        .write(&doc, &WriterOptions::default(), &mut Context::pure())
        .unwrap();
    document_xml(&bytes, "word/document.xml")
}

#[test]
fn nested_tables_fit_on_a_two_mib_stack() {
    use randoc_ast::{Alignment, Block, Cell, ColSpec, ColWidth, Inline, Row, Table, TableBody};

    on_two_mib_stack(|| {
        let levels = Context::pure().limits.max_nesting_depth - 2;
        let mut block = Block::Plain(vec![Inline::Str("Innermost cell".into())]);
        for _ in 0..levels {
            block = Block::Table(Box::new(Table {
                col_specs: vec![ColSpec {
                    align: Alignment::AlignDefault,
                    width: ColWidth::ColWidthDefault,
                }],
                bodies: vec![TableBody {
                    body: vec![Row {
                        cells: vec![Cell {
                            content: vec![block],
                            ..Cell::default()
                        }],
                        ..Row::default()
                    }],
                    ..TableBody::default()
                }],
                ..Table::default()
            }));
        }
        let xml = write_blocks(vec![block]);
        assert_eq!(xml.matches("<w:tbl>").count(), levels);
        assert!(xml.contains("Innermost cell"));
    });
}

#[test]
fn figures_in_lists_and_divs_fit_on_a_two_mib_stack() {
    use randoc_ast::{Attr, Block, Caption, Inline};

    on_two_mib_stack(|| {
        let levels = (Context::pure().limits.max_nesting_depth - 2) / 3;
        let mut block = Block::Plain(vec![Inline::Str("Innermost figure".into())]);
        for _ in 0..levels {
            block = Block::Div(
                Attr::default(),
                vec![Block::BulletList(vec![vec![Block::Figure(
                    Attr::default(),
                    Caption::default(),
                    vec![block],
                )]])],
            );
        }
        let xml = write_blocks(vec![block]);
        assert_eq!(xml.matches("<w:tbl>").count(), levels);
        assert!(xml.contains("Innermost figure"));
    });
}

#[test]
fn deep_display_math_fits_on_a_two_mib_stack() {
    use randoc_ast::{Block, Inline, MathType};

    on_two_mib_stack(|| {
        let groups = (Context::pure().limits.max_nesting_depth - 4) / 2;
        let formula = format!("{}x{}", "x^{".repeat(groups), "}".repeat(groups));
        let xml = write_blocks(vec![Block::Para(vec![Inline::Math(
            MathType::DisplayMath,
            formula,
        )])]);
        assert!(xml.contains("<m:oMathPara>"));
        assert_eq!(xml.matches("<m:sSup>").count(), groups);
    });
}

#[test]
fn uses_the_configured_nesting_limit() {
    use randoc_ast::{Block, Inline, MetaValue};

    let mut context = Context::pure();
    context.limits.max_nesting_depth = 512;
    let mut quote = Block::Para(vec![Inline::Str("Deep quote".into())]);
    for _ in 0..200 {
        quote = Block::BlockQuote(vec![quote]);
    }
    let mut list = Block::Plain(vec![Inline::Str("Deep list".into())]);
    for _ in 0..150 {
        list = Block::BulletList(vec![vec![list]]);
    }
    let mut nested = MetaValue::MetaBool(false);
    for _ in 0..400 {
        nested = MetaValue::MetaList(vec![nested]);
    }
    let mut doc = Pandoc {
        blocks: vec![quote, list],
        ..Pandoc::default()
    };
    doc.meta.0.insert("nested".into(), nested);
    let bytes = super::DocxWriter
        .write(&doc, &WriterOptions::default(), &mut context)
        .unwrap();
    let xml = document_xml(&bytes, "word/document.xml");
    assert!(xml.contains("Deep quote"));
    assert!(xml.contains("Deep list"));
}

#[test]
fn allocates_unique_comment_ids_for_explicit_and_generated_ids() {
    use randoc_ast::{Attr, Block, Inline};

    let mut spans = Vec::new();
    for key in ["", "9", "11", "", "9"] {
        for class in ["comment-start", "comment-end"] {
            spans.push(Inline::Span(
                Attr {
                    id: key.into(),
                    classes: vec![class.into()],
                    ..Attr::default()
                },
                vec![],
            ));
        }
    }
    let doc = Pandoc {
        blocks: vec![Block::Para(spans)],
        ..Pandoc::default()
    };
    let bytes = super::DocxWriter
        .write(&doc, &WriterOptions::default(), &mut Context::pure())
        .unwrap();
    let comments = document_xml(&bytes, "word/comments.xml");
    let document = document_xml(&bytes, "word/document.xml");
    for id in 9..14 {
        assert_eq!(
            comments
                .matches(&format!("<w:comment w:id=\"{id}\""))
                .count(),
            1
        );
        for tag in ["commentRangeStart", "commentRangeEnd", "commentReference"] {
            assert_eq!(
                document.matches(&format!("<w:{tag} w:id=\"{id}\"")).count(),
                1
            );
        }
    }
}

#[test]
fn writes_custom_style_ids_and_code_background() {
    use randoc_ast::{Attr, Block, Inline};

    let doc = Pandoc {
        blocks: vec![Block::Div(
            Attr {
                attributes: vec![("custom-style".into(), "My Style!".into())],
                ..Attr::default()
            },
            vec![Block::Para(vec![Inline::Str("Example".into())])],
        )],
        ..Pandoc::default()
    };
    let mut options = WriterOptions::default();
    options.highlight_style = Some("zenburn".into());
    let bytes = super::DocxWriter
        .write(&doc, &options, &mut Context::pure())
        .unwrap();
    let xml = document_xml(&bytes, "word/styles.xml");
    assert!(xml.contains(r#"<w:wordWrap w:val="off" /><w:shd w:val="clear" w:fill="303030" />"#));
    assert!(xml.contains(r#"w:customStyle="1" w:styleId="MyStyle!"><w:name w:val="My Style!" /><w:basedOn w:val="BodyText" /><w:qFormat />"#));
}

#[test]
fn template_rendering_uses_the_remaining_byte_budget() {
    use randoc_ast::{Block, Inline};
    use randoc_templates::Template;

    let doc = Pandoc {
        blocks: vec![Block::Para(vec![Inline::Str("x".repeat(40_000))])],
        ..Pandoc::default()
    };
    let mut context = Context::pure();
    context.limits.max_archive_size = 120_000;
    let mut options = WriterOptions::default();
    let template = |source| {
        Template::compile("test", source, 8, &mut |_| {
            Err::<String, &str>("Unexpected partial")
        })
        .unwrap()
    };
    options.template = Some(template("<doc>$body$</doc>"));
    super::DocxWriter
        .write(&doc, &options, &mut context)
        .unwrap();
    options.template = Some(template("<doc>$body$$body$</doc>"));
    let error = super::DocxWriter
        .write(&doc, &options, &mut context)
        .unwrap_err();
    assert_eq!(error.to_string(), "DOCX output exceeds the byte limit.");
}

/// Returns the paragraphs that are open at byte `at` of `xml`.
fn open_paragraphs(xml: &str, at: usize) -> usize {
    let before = &xml[..at];
    before.matches("<w:p>").count() - before.matches("</w:p>").count()
}

#[test]
fn code_block_bookmark_surrounds_the_paragraph_at_body_level() {
    use randoc_ast::{Attr, Block, Inline};

    let code = |id: &str| Block::CodeBlock(Attr::new(id, vec![], vec![]), "x = 1".into());
    let xml = write_blocks(vec![
        code("top"),
        Block::BulletList(vec![vec![
            Block::Plain(vec![Inline::Str("item".into())]),
            code("in-list"),
        ]]),
        Block::BlockQuote(vec![code("quoted")]),
    ]);
    let mut starts = 0;
    for (at, _) in xml.match_indices("<w:bookmarkStart ") {
        assert_eq!(open_paragraphs(&xml, at), 0, "{xml}");
        let paragraph = xml[at..].find("<w:p>").unwrap();
        let code = xml[at..].find("SourceCode").unwrap();
        assert!(paragraph < code, "{xml}");
        starts += 1;
    }
    let mut ends = 0;
    for (at, _) in xml.match_indices("<w:bookmarkEnd ") {
        assert_eq!(open_paragraphs(&xml, at), 0, "{xml}");
        let previous = xml[..at].rfind("</w:p>").unwrap();
        assert!(
            xml[previous..at]
                .trim_start_matches("</w:p>")
                .trim()
                .is_empty()
        );
        ends += 1;
    }
    assert_eq!(starts, 3, "{xml}");
    assert_eq!(ends, 3, "{xml}");
}

/// Writes an empty document with `reference` as the reference document and
/// returns its settings part.
fn settings_with_reference(reference: Vec<u8>) -> String {
    let host = randoc_core::PureHost {
        files: std::collections::BTreeMap::from([("ref.docx".into(), reference)]),
        ..randoc_core::PureHost::default()
    };
    let mut options = WriterOptions::default();
    options.reference_doc = Some("ref.docx".into());
    let bytes = super::DocxWriter
        .write(
            &Pandoc::default(),
            &options,
            &mut Context::new(Box::new(host)),
        )
        .unwrap();
    document_xml(&bytes, "word/settings.xml")
}

/// Returns `archive` with the entry `name` replaced by `xml`.
fn replace_entry(archive: &[u8], name: &str, xml: &str) -> Vec<u8> {
    use randoc_archive::zip::{Archive, ArchiveLimits, ArchiveWriter};

    let mut source = Archive::open(archive, ArchiveLimits::default()).unwrap();
    let mut output = ArchiveWriter::new();
    let names: Vec<String> = source
        .entry_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    for entry in names {
        let bytes = if entry == name {
            xml.as_bytes().to_vec()
        } else {
            source.read(&entry).unwrap().unwrap()
        };
        output.write_deflated(&entry, &bytes).unwrap();
    }
    output.finish().unwrap()
}

#[test]
fn settings_have_an_empty_rsids_element_when_the_reference_has_none() {
    let base = super::DocxWriter
        .write(
            &Pandoc::default(),
            &WriterOptions::default(),
            &mut Context::pure(),
        )
        .unwrap();
    let w = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let without = replace_entry(
        &base,
        "word/settings.xml",
        &format!(
            r#"<w:settings xmlns:w="{w}"><w:listSeparator w:val=";"/><w:zoom w:percent="90"/></w:settings>"#
        ),
    );
    let settings = settings_with_reference(without);
    assert!(
        settings.ends_with(
            "<w:zoom w:percent=\"90\" /><w:rsids>\n  </w:rsids><w:listSeparator w:val=\";\" /></w:settings>"
        ),
        "{settings}"
    );

    let with = replace_entry(
        &base,
        "word/settings.xml",
        &format!(
            r#"<w:settings xmlns:w="{w}"><w:rsids><w:rsidRoot w:val="00A1"/></w:rsids><w:zoom w:percent="90"/></w:settings>"#
        ),
    );
    let settings = settings_with_reference(with);
    assert!(
        settings.ends_with(
            "<w:zoom w:percent=\"90\" /><w:rsids><w:rsidRoot w:val=\"00A1\" /></w:rsids></w:settings>"
        ),
        "{settings}"
    );
}

#[test]
fn reports_each_local_image_once_as_a_loaded_resource() {
    use randoc_ast::{Attr, Block, Inline, Target};
    use randoc_core::LogMessage;

    const GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff\x21\xf9\x04\x01\x00\x00\x00\x00\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x01\x44\x00\x3b";
    let image = |url: &str| {
        Inline::Image(
            Attr::default(),
            vec![Inline::Str("alt".into())],
            Target {
                url: url.into(),
                title: String::new(),
            },
        )
    };
    let doc = Pandoc {
        blocks: vec![Block::Para(vec![
            image("chap1/spider.gif"),
            image("chap1/spider.gif"),
            image("moon.gif"),
            image("data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7"),
        ])],
        ..Pandoc::default()
    };
    let host = randoc_core::PureHost {
        files: std::collections::BTreeMap::from([
            ("chap1/spider.gif".into(), GIF.to_vec()),
            ("images/moon.gif".into(), GIF.to_vec()),
        ]),
        ..randoc_core::PureHost::default()
    };
    let mut context = Context::new(Box::new(host));
    context.state.resource_path = vec![".".into(), "images".into()];
    super::DocxWriter
        .write(&doc, &WriterOptions::default(), &mut context)
        .unwrap();
    let loaded: Vec<_> = context
        .log()
        .iter()
        .filter(|message| matches!(message, LogMessage::LoadedResource(..)))
        .cloned()
        .collect();
    assert_eq!(
        loaded,
        [
            LogMessage::LoadedResource("chap1/spider.gif".into(), "chap1/spider.gif".into()),
            LogMessage::LoadedResource("moon.gif".into(), "images/moon.gif".into()),
        ]
    );
}
