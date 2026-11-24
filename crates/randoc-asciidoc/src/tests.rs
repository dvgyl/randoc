use randoc_ast::{Attr, Block, Inline, MathType, Pandoc, QuoteType};
use randoc_core::{Context, PureHost, ReaderOptions, Source, Sources, WriterOptions};

use super::{reader, writer};

fn read(text: &str, context: &mut Context) -> randoc_core::Result<Pandoc> {
    reader::read(
        &Sources::from_text(text),
        &ReaderOptions::default(),
        context,
    )
}

#[test]
fn format_names() {
    assert!(super::reader("asciidoc").is_some());
    assert!(super::reader("asciidoctor").is_none());
    assert!(super::reader("asciidoc_legacy").is_none());
    assert!(super::writer("adoc").is_none());
    for format in ["asciidoc", "asciidoctor", "asciidoc_legacy"] {
        assert!(super::writer(format).is_some_and(|writer| writer.renders_template()));
    }
}

#[test]
fn headings_and_forward_links() -> randoc_core::Result<()> {
    let doc = read(
        "See <<_hello,_élan!_2,Parent>>.\n\n== Hello, Élan!\n\n=== Hello, Élan!\n\n[discrete]\n==== A discrete heading\n",
        &mut Context::pure(),
    )?;
    assert!(matches!(&doc.blocks[1], Block::Header(1, attr, _) if attr.id == "_hello,_élan!_2"));
    assert!(matches!(&doc.blocks[2], Block::Header(2, attr, _) if attr.id == "_hello,_élan!"));
    assert!(matches!(&doc.blocks[3], Block::Header(_, attr, _) if attr.id.is_empty()));
    Ok(())
}

#[test]
fn literal_attribute_value() -> randoc_core::Result<()> {
    let doc = read(
        ":value: a *b*\n:reference: {value}\n\n{value}\n\n{reference}",
        &mut Context::pure(),
    )?;
    assert_eq!(
        doc.blocks,
        [
            Block::Para(vec![
                Inline::Str("a".into()),
                Inline::Space,
                Inline::Str("*b*".into())
            ]),
            Block::Para(vec![Inline::Str("{value}".into())]),
        ]
    );
    Ok(())
}

#[test]
fn hard_breaks_and_inline_markup() -> randoc_core::Result<()> {
    let doc = read(
        "[%hardbreaks]\nThis is *bold*\nand _italic_.",
        &mut Context::pure(),
    )?;
    let [Block::Div(attr, blocks)] = doc.blocks.as_slice() else {
        panic!("Missing paragraph wrapper");
    };
    assert_eq!(attr.get("options"), Some("hardbreaks"));
    let [Block::Para(inlines)] = blocks.as_slice() else {
        panic!("Missing paragraph");
    };
    assert!(inlines.contains(&Inline::LineBreak));
    assert!(
        inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Strong(_)))
    );
    assert!(
        inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Emph(_)))
    );
    Ok(())
}

#[test]
fn html_passthrough() -> randoc_core::Result<()> {
    let doc = read(
        "Text +++<b>bold</b>+++ and pass:[<i>italic</i>].",
        &mut Context::pure(),
    )?;
    assert!(matches!(&doc.blocks[0], Block::Para(inlines)
        if inlines.iter().any(|inline| matches!(inline, Inline::Strong(_))) &&
           inlines.iter().any(|inline| matches!(inline, Inline::Emph(_)))));
    Ok(())
}

#[test]
fn relative_nested_includes() -> randoc_core::Result<()> {
    let mut host = PureHost::default();
    host.files.insert(
        "sub/chapter.adoc".into(),
        b"include::parts/p.adoc[]".to_vec(),
    );
    host.files
        .insert("sub/parts/p.adoc".into(), b"Nested.".to_vec());
    let mut context = Context::new(Box::new(host));
    context.state.resource_path = vec!["ignored".into()];
    let doc = reader::read(
        &Sources(vec![Source {
            name: "sub/main.adoc".into(),
            text: "include::chapter.adoc[]".into(),
        }]),
        &ReaderOptions::default(),
        &mut context,
    )?;
    let [Block::Div(outer, blocks)] = doc.blocks.as_slice() else {
        panic!("Missing include");
    };
    assert_eq!(outer.get("path"), Some("sub/chapter.adoc"));
    assert!(
        matches!(blocks.as_slice(), [Block::Div(inner, _)] if inner.get("path") == Some("sub/parts/p.adoc"))
    );
    Ok(())
}

#[test]
fn include_cycle_and_cumulative_size() {
    let mut host = PureHost::default();
    host.files
        .insert("a.adoc".into(), b"include::./a.adoc[]".to_vec());
    host.files.insert("b.adoc".into(), b"1234567890".to_vec());
    let mut context = Context::new(Box::new(host));
    let cycle = read("include::a.adoc[]", &mut context).expect("Expected an include block");
    assert!(matches!(cycle.blocks.as_slice(), [Block::Div(_, blocks)] if blocks.is_empty()));
    assert!(matches!(
        context.log(),
        [randoc_core::LogMessage::CouldNotLoadIncludeFile(path, pos)]
            if path == "./a.adoc" && pos.line == 0 && pos.column == 0
    ));
    let input = "include::b.adoc[]\n\ninclude::b.adoc[]";
    context.limits.max_input_size = input.len() as u64 + 15;
    let error = read(input, &mut context).expect_err("Expected cumulative input size limit");
    assert!(error.to_string().contains("input size"));
    context.limits.max_input_size = 100;
    context.limits.max_resource_size = 5;
    let error = read("include::b.adoc[]", &mut context).expect_err("Expected resource size limit");
    assert!(error.to_string().contains("input size"));
}

#[test]
fn include_cycle_in_listing() -> randoc_core::Result<()> {
    let source = Source {
        name: "listing.adoc".into(),
        text: "----\nfirst\ninclude::listing.adoc[]\nlast\n----".into(),
    };
    let mut context = Context::pure();
    let doc = reader::read(
        &Sources(vec![source]),
        &ReaderOptions::default(),
        &mut context,
    )?;
    assert!(matches!(&doc.blocks[0], Block::CodeBlock(_, text) if text == "first\nlast"));
    assert!(matches!(
        context.log(),
        [randoc_core::LogMessage::CouldNotLoadIncludeFile(path, pos)]
            if path == "listing.adoc" && pos.line == 0 && pos.column == 0
    ));
    Ok(())
}

#[test]
fn table_limits_before_allocation() {
    let mut context = Context::pure();
    context.limits.max_table_cells = 32;
    for input in [
        "[cols=\"100000*\"]\n|===\n|x\n|===",
        "|===\n.9999+|x\n|===",
        "|===\n9999+|x\n|===",
        "|===\n8.8+|x\n|===",
        "|===\n9999*|x\n|===",
    ] {
        let error = read(input, &mut context).expect_err("Expected table limit");
        assert!(error.to_string().contains("table cells"), "{error}");
    }
}

#[test]
fn named_notes_resolve_and_charge_each_copy() -> randoc_core::Result<()> {
    let input = "First footnote:note[]. Then footnote:note[*A note*], footnote:note[Other]. Finally footnoteref:[note].";
    let doc = read(input, &mut Context::pure())?;
    let Block::Para(inlines) = &doc.blocks[0] else {
        panic!("Missing paragraph");
    };
    let notes: Vec<_> = inlines
        .iter()
        .filter(|inline| matches!(inline, Inline::Note(_)))
        .collect();
    let note = Inline::Note(vec![Block::Para(vec![Inline::Strong(vec![
        Inline::Str("A".into()),
        Inline::Space,
        Inline::Str("note".into()),
    ])])]);
    assert_eq!(notes, vec![&note; 4]);
    let note = "long text ".repeat(180);
    let text = format!("footnote:a[{note}] {}", "footnote:a[] ".repeat(90));
    let mut context = Context::pure();
    context.limits.max_input_size = 20_000;
    let error = read(&text, &mut context).expect_err("Expected reference expansion limit");
    assert!(error.to_string().contains("reference expansion"));
    Ok(())
}

#[test]
fn writer_dialects() -> randoc_core::Result<()> {
    let document = Pandoc {
        blocks: vec![Block::Para(vec![
            Inline::Quoted(QuoteType::DoubleQuote, vec![Inline::Str("q".into())]),
            Inline::Space,
            Inline::Math(MathType::InlineMath, "x".into()),
        ])],
        ..Pandoc::default()
    };
    let mut context = Context::pure();
    let options = WriterOptions::default();
    let modern = writer::write(&document, &options, &mut context, writer::Dialect::Modern)?;
    assert_eq!(modern, "\"\x60q\x60\" latexmath:[x]");
    assert_eq!(
        writer::write(
            &document,
            &options,
            &mut context,
            writer::Dialect::Asciidoctor
        )?,
        modern
    );
    assert!(context.log().is_empty());
    assert_eq!(
        writer::write(&document, &options, &mut context, writer::Dialect::Legacy)?,
        "\x60\x60q'' latexmath:[$x$]"
    );
    Ok(())
}

#[test]
fn reader_on_two_mib_stack() -> std::io::Result<()> {
    let thread = std::thread::Builder::new().stack_size(2 << 20).spawn(|| {
        let mut context = Context::pure();
        context.limits.max_nesting_depth = 160;
        let mut input = String::new();
        for level in 1..=145 {
            input.push_str(&"*".repeat(level));
            input.push_str(" item\n");
        }
        assert!(read(&input, &mut context).is_ok());
        context.limits.max_nesting_depth = 64;
        assert!(read(&input, &mut context).is_err());
        assert!(read(&"{".repeat(100_000), &mut context).is_ok());
        let mut blocks = String::new();
        for depth in 4..64 {
            blocks.push_str(&"=".repeat(depth));
            blocks.push('\n');
        }
        let close = blocks.lines().rev().collect::<Vec<_>>().join("\n");
        let nested_blocks = format!("{blocks}leaf\n{close}\n");
        let nested_tables = format!("|===\na|!===\na!{nested_blocks}!===\n|===\n");
        let notes = format!("{}leaf{}", "footnote:[".repeat(100), "]".repeat(100));
        let labels = format!("{}leaf{}", "<<id,".repeat(100), ">>".repeat(100));
        let delimiters = "*_#^~leaf~^#_*";
        let html = format!(
            "{blocks}++++\n{}leaf{}\n++++\n{close}\n",
            "<b>".repeat(30),
            "</b>".repeat(30)
        );
        context.limits.max_nesting_depth = 128;
        for input in [
            &nested_blocks,
            &nested_tables,
            &notes,
            &html,
            &labels,
            delimiters,
        ] {
            let result = read(input, &mut context);
            assert!(result.is_ok(), "{result:?}");
        }
        let nested = read(&nested_tables, &mut context);
        assert!(matches!(nested, Ok(doc) if matches!(doc.blocks.as_slice(),
            [Block::Table(outer)] if matches!(outer.bodies[0].body[0].cells[0].content.as_slice(),
                [Block::Table(_)]))));
        for input in [&nested_blocks, &nested_tables, &notes, delimiters] {
            context.limits.max_nesting_depth = 3;
            assert!(read(input, &mut context).is_err());
        }
        context.limits.max_nesting_depth = 75;
        assert!(read(&html, &mut context).is_err());
        assert_eq!(context.limits.max_nesting_depth, 75);
        let inline_html = format!(
            "{blocks}+++{}leaf{}+++\n{close}\n",
            "<i>".repeat(30),
            "</i>".repeat(30)
        );
        assert!(read(&inline_html, &mut context).is_err());
        assert_eq!(context.limits.max_nesting_depth, 75);
        let mut host = PureHost::default();
        for index in 1..=132 {
            let next = if index == 132 {
                "leaf".into()
            } else {
                format!("include::{}.adoc[]", index + 1)
            };
            host.files
                .insert(format!("{index}.adoc"), next.into_bytes());
        }
        let mut context = Context::new(Box::new(host));
        context.limits.max_nesting_depth = 144;
        assert!(read("include::1.adoc[]", &mut context).is_ok());
        context.limits.max_nesting_depth = 32;
        assert!(read("include::1.adoc[]", &mut context).is_err());
    })?;
    thread
        .join()
        .map_err(|_| std::io::Error::other("Reader stack test failed"))
}

#[test]
fn writer_on_two_mib_stack() -> std::io::Result<()> {
    let thread = std::thread::Builder::new().stack_size(2 << 20).spawn(|| {
        let mut block = Block::Plain(vec![Inline::Str("leaf".into())]);
        for _ in 0..145 {
            block = Block::Div(Attr::default(), vec![block]);
        }
        let doc = Pandoc {
            blocks: vec![block],
            ..Pandoc::default()
        };
        let mut context = Context::pure();
        context.limits.max_nesting_depth = 160;
        let options = WriterOptions::default();
        let result = writer::write(&doc, &options, &mut context, writer::Dialect::Modern);
        assert!(matches!(result, Ok(text) if text == "leaf"));
        context.limits.max_nesting_depth = 64;
        assert!(writer::write(&doc, &options, &mut context, writer::Dialect::Modern).is_err());
    })?;
    thread
        .join()
        .map_err(|_| std::io::Error::other("Writer stack test failed"))
}

#[test]
fn caption_and_table_occupancy() -> randoc_core::Result<()> {
    let doc = read(
        ".Caption\n[cols=\"2,>1\",%header]\n|===\n|Left |Right\n.2+|Both rows |First\n|Second\n|===",
        &mut Context::pure(),
    )?;
    let [Block::Table(table)] = doc.blocks.as_slice() else {
        panic!("Missing table");
    };
    assert_eq!(
        table.col_specs[0].width,
        randoc_ast::ColWidth::ColWidth(2.0 / 3.0)
    );
    assert_eq!(table.col_specs[1].align, randoc_ast::Alignment::AlignRight);
    assert_eq!(
        table.caption.long,
        [Block::Plain(vec![Inline::Str("Caption".into())])]
    );
    assert_eq!(table.head.rows.len(), 1);
    assert_eq!(table.bodies[0].body[0].cells[0].row_span, 2);
    assert_eq!(table.bodies[0].body[1].cells.len(), 1);
    Ok(())
}

#[test]
fn code_callouts_and_admonitions() -> randoc_core::Result<()> {
    let doc = read(
        "[source,python]\n----\nf() <1>\ng() <.>\n----\n\n<1> First\n<2> Second\n\nNOTE: See *this*.",
        &mut Context::pure(),
    )?;
    assert!(matches!(&doc.blocks[0], Block::CodeBlock(attr, text)
        if attr.has_class("python") && text == "f() ①\ng() ②"));
    assert!(matches!(&doc.blocks[1], Block::Div(attr, body)
        if attr.has_class("callout-list") && matches!(body.as_slice(), [Block::OrderedList(_, items)] if items.len() == 2)));
    assert!(matches!(&doc.blocks[2], Block::Div(attr, body)
        if attr.has_class("note") && matches!(body.as_slice(), [Block::Div(title, _), Block::Para(_)] if title.has_class("title"))));
    Ok(())
}

#[test]
fn checklists_and_display_math() -> randoc_core::Result<()> {
    let doc = read(
        "* [x] done\n* [ ] open\n\n[stem]\n++++\nx^2\n++++",
        &mut Context::pure(),
    )?;
    let output = |dialect| {
        writer::write(
            &doc,
            &WriterOptions::default(),
            &mut Context::pure(),
            dialect,
        )
    };
    assert!(output(writer::Dialect::Modern)?.starts_with("* [x] done\n* [ ] open"));
    assert!(output(writer::Dialect::Legacy)?.starts_with("* ☒ done\n* ☐ open"));
    assert!(
        matches!(&doc.blocks[1], Block::Para(inlines) if inlines == &[Inline::Math(MathType::DisplayMath, "x^2\n".into())])
    );
    Ok(())
}

#[test]
fn wrap_modes_for_soft_breaks() -> randoc_core::Result<()> {
    let doc = read("One line\nthen another.", &mut Context::pure())?;
    let mut options = WriterOptions::default();
    options.wrap_text = randoc_core::WrapOption::WrapPreserve;
    assert_eq!(
        writer::write(
            &doc,
            &options,
            &mut Context::pure(),
            writer::Dialect::Modern
        )?,
        "One line\nthen another."
    );
    options.wrap_text = randoc_core::WrapOption::WrapNone;
    assert_eq!(
        writer::write(
            &doc,
            &options,
            &mut Context::pure(),
            writer::Dialect::Modern
        )?,
        "One line then another."
    );
    Ok(())
}

#[test]
fn table_copy_budget_and_real_cells() -> randoc_core::Result<()> {
    let mut context = Context::pure();
    let doc = read(
        "[cols=\"3*\"]\n|===\n|x 500000*|y\n|z|w|u\n|===",
        &mut context,
    )?;
    let [Block::Table(table)] = doc.blocks.as_slice() else {
        panic!("Missing table");
    };
    let rows = &table.bodies[0].body;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].cells[1], rows[0].cells[2]);
    assert_eq!(rows[1].cells.len(), 3);

    let input = format!("|===\n1000*|{}\n|===", "x".repeat(100_000));
    let error = read(&input, &mut context).expect_err("Expected copy expansion limit");
    assert!(error.to_string().contains("reference expansion"));

    context.limits.max_table_cells = 1;
    let input = format!("|===\n{}|===", "|x\n".repeat(200_001));
    let doc = read(&input, &mut context)?;
    assert!(matches!(&doc.blocks[0], Block::Table(table) if table.bodies[0].body.len() == 200_001));
    let doc = read("|===\n|a|b|c\n|===", &mut context)?;
    assert!(matches!(&doc.blocks[0], Block::Table(table) if table.col_specs.len() == 3));
    assert!(read("|===\n3*|x\n|===", &mut context).is_err());
    assert!(read("|===\n.2+|x\n\n.2+|y\n|===", &mut context).is_err());
    Ok(())
}

#[test]
fn table_copies_parse_includes_once() -> randoc_core::Result<()> {
    let path = "loop.adoc";
    let mut host = PureHost::default();
    host.files
        .insert(path.into(), b"include::loop.adoc[]".to_vec());
    let mut context = Context::new(Box::new(host));
    let doc = read(
        "[cols=\"2*a\"]\n|===\n2*|include::loop.adoc[]\n|===",
        &mut context,
    )?;
    let [Block::Table(table)] = doc.blocks.as_slice() else {
        panic!("Missing table");
    };
    let cells = &table.bodies[0].body[0].cells;
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0], cells[1]);
    assert_eq!(context.log().len(), 1);
    Ok(())
}

#[test]
fn description_list_paragraphs_scale_linearly() -> randoc_core::Result<()> {
    fn run(count: usize) -> randoc_core::Result<usize> {
        let text = format!("a:: x\ny\n\n{}b:: z", "body\n\n".repeat(count));
        reader::DEFINITION_CHECKS.with(|checks| checks.set(0));
        let doc = read(&text, &mut Context::pure())?;
        let checks = reader::DEFINITION_CHECKS.with(std::cell::Cell::get);
        assert_eq!(doc.blocks.len(), count + 2);
        let Block::DefinitionList(first) = &doc.blocks[0] else {
            panic!("Missing first description list");
        };
        assert_eq!(first[0].1[0].len(), 1);
        assert!(matches!(doc.blocks[count + 1], Block::DefinitionList(_)));
        assert!(
            doc.blocks[1..=count]
                .iter()
                .all(|block| matches!(block, Block::Para(_)))
        );
        assert!(checks >= count);
        Ok(checks)
    }

    let small = run(2_000)?;
    let large = run(16_000)?;
    assert!(
        large <= small * 9,
        "2,000 paragraphs needed {small} definition checks. 16,000 needed {large}."
    );
    Ok(())
}

#[test]
fn metadata_rendering_reports_messages() -> randoc_core::Result<()> {
    use randoc_ast::{Alignment, Cell, ColSpec, ColWidth, MetaValue, Row, Table, TableBody};

    fn table(content: Vec<Block>) -> Block {
        Block::Table(Box::new(Table {
            col_specs: vec![ColSpec {
                align: Alignment::AlignDefault,
                width: ColWidth::ColWidthDefault,
            }],
            bodies: vec![TableBody {
                body: vec![Row {
                    cells: vec![Cell {
                        content,
                        ..Cell::default()
                    }],
                    ..Row::default()
                }],
                ..TableBody::default()
            }],
            ..Table::default()
        }))
    }
    let mut document = Pandoc::default();
    document.meta.insert(
        "nested",
        MetaValue::MetaBlocks(vec![table(vec![table(vec![table(vec![])])])]),
    );
    let mut options = WriterOptions::default();
    options.template = Some(
        randoc_templates::Template::compile("test", "$nested$", 128, &mut |_| {
            Err::<String, _>("Unexpected partial")
        })
        .expect("Expected valid template"),
    );
    let mut context = Context::pure();
    writer::write(&document, &options, &mut context, writer::Dialect::Modern)?;
    assert!(matches!(
        context.log(),
        [randoc_core::LogMessage::BlockNotRendered(_)]
    ));
    Ok(())
}

#[test]
fn expansion_allowance_depends_on_input_size() {
    let value = "x".repeat(200);
    let definitions = format!(":a: {value}\n\n");
    let input = format!("{definitions}{}", "{a} ".repeat(100));
    let error = read(&input, &mut Context::pure()).expect_err("Expected input-scaled limit");
    assert!(error.to_string().contains("reference expansion"));

    let padded = format!("{input}\n\n// {}", "x".repeat(200));
    assert!(read(&padded, &mut Context::pure()).is_ok());
    let mut context = Context::pure();
    context.limits.max_input_size = 2_000;
    let error = read(&padded, &mut context).expect_err("Expected capped limit");
    assert!(error.to_string().contains("reference expansion"));
}

#[test]
fn revision_lines_need_a_space_after_the_comma() -> randoc_core::Result<()> {
    let meta = |revision: &str| -> randoc_core::Result<Vec<(String, String)>> {
        let doc = read(
            &format!("= T\nAnn Author\n{revision}\n\nbody\n"),
            &mut Context::pure(),
        )?;
        Ok(["version", "date", "remark"]
            .into_iter()
            .filter_map(|key| {
                doc.meta
                    .get(key)
                    .map(|value| (key.to_owned(), randoc_core::shared::stringify_meta(value)))
            })
            .collect())
    };
    let pairs = |items: &[(&str, &str)]| -> Vec<(String, String)> {
        items
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    };
    assert_eq!(meta("a,b")?, pairs(&[]));
    assert_eq!(meta("v1.0,2024")?, pairs(&[]));
    assert_eq!(meta("v1, 2024:x")?, pairs(&[]));
    assert_eq!(
        meta("x: y, z")?,
        pairs(&[("version", "x: y"), ("date", "z")])
    );
    assert_eq!(
        meta("v1.0, 2024: re, mark")?,
        pairs(&[("version", "1.0"), ("date", "2024"), ("remark", "re, mark")])
    );
    assert_eq!(
        meta("v1, 2024:")?,
        pairs(&[("version", "1"), ("date", "2024"), ("remark", "")])
    );
    assert_eq!(meta("v2: remark")?, pairs(&[("version", "2: remark")]));
    Ok(())
}

#[test]
fn attribute_lines_end_paragraphs() -> randoc_core::Result<()> {
    let doc = read("text\n[foo]\nmore\n", &mut Context::pure())?;
    assert_eq!(
        doc.blocks,
        [
            Block::Para(vec![Inline::Str("text".into())]),
            Block::Para(vec![Inline::Str("more".into())]),
        ]
    );
    Ok(())
}

#[test]
fn image_alt_text_keeps_commas_and_inline_markup() -> randoc_core::Result<()> {
    let image = |alt: Vec<Inline>| {
        Inline::Image(Attr::default(), alt, randoc_ast::Target::new("x.png", ""))
    };
    let document = Pandoc {
        meta: randoc_ast::Meta::default(),
        blocks: vec![Block::Para(vec![
            image(vec![
                Inline::Str("a,".into()),
                Inline::Space,
                Inline::Emph(vec![Inline::Str("b".into())]),
            ]),
            Inline::Space,
            Inline::Link(
                Attr::default(),
                vec![image(vec![
                    Inline::Str("c,".into()),
                    Inline::Space,
                    Inline::Str("d".into()),
                ])],
                randoc_ast::Target::new("u", ""),
            ),
        ])],
    };
    let output = writer::write(
        &document,
        &WriterOptions::default(),
        &mut Context::pure(),
        writer::Dialect::Modern,
    )?;
    assert_eq!(output, "image:x.png[a, _b_] link:u[image:x.png[c&#44; d]]");
    Ok(())
}
