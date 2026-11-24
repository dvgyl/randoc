use super::*;
use randoc_ast::{Cell, ColSpec, ColWidth, Row, Table, TableBody, Target};

fn paragraph(value: &str) -> Block {
    Block::Para(vec![Inline::Str(value.into())])
}

fn render_test(
    blocks: Vec<Block>,
    metadata: &[(&str, MetaValue)],
    template: Option<&str>,
    extensions: &[Extension],
) -> Result<String> {
    let mut doc = Pandoc {
        blocks,
        ..Pandoc::default()
    };
    doc.meta.0.extend(
        metadata
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone())),
    );
    let mut options = WriterOptions::default();
    options.extensions = randoc_core::Extensions::from_list(extensions);
    options.wrap_text = WrapOption::WrapNone;
    if let Some(template) = template {
        options.template = Some(
            Template::compile("unit", template, 128, &mut |path| {
                Err(Error::App(format!("Missing test template partial {path}.")))
            })
            .map_err(|error| Error::App(error.to_string()))?,
        );
    }
    write_opendocument(&doc, &options, &mut Context::pure())
}

fn cell(text: &str, row_span: i64, col_span: i64) -> Cell {
    Cell {
        content: vec![paragraph(text)],
        row_span,
        col_span,
        ..Cell::default()
    }
}

fn row(cells: Vec<Cell>) -> Row {
    Row {
        cells,
        ..Row::default()
    }
}

#[test]
fn table_head_spans_keep_the_cell_style_and_omit_covered_cells() -> Result<()> {
    let mut table = Table {
        col_specs: vec![ColSpec::default(); 3],
        ..Table::default()
    };
    table.head.rows = vec![
        row(vec![cell("Top left", 2, 2), cell("Top right", 1, 1)]),
        row(vec![cell("Bottom right", 1, 1)]),
    ];
    let xml = render_test(vec![Block::Table(Box::new(table))], &[], None, &[])?;
    assert!(xml.contains("<table:table-header-rows>"));
    assert!(xml.contains(r#"<table:table-cell table:style-name="TableHeaderRowCell" office:value-type="string" table:number-columns-spanned="2" table:number-rows-spanned="2">"#));
    assert!(xml.contains(r#"<text:p text:style-name="Table_20_Heading">Bottom right</text:p>"#));
    assert_eq!(xml.matches("<table:table-cell ").count(), 3);
    assert!(!xml.contains("covered-table-cell"));
    Ok(())
}

#[test]
fn table_body_spans_keep_regular_cell_markup_for_body_headers() -> Result<()> {
    let table = Table {
        col_specs: vec![ColSpec::default(); 4],
        bodies: vec![TableBody {
            row_head_columns: 1,
            head: vec![row(vec![cell("Group", 1, 4)])],
            body: vec![row(vec![cell("Label", 1, 1), cell("Value", 1, 3)])],
            ..TableBody::default()
        }],
        ..Table::default()
    };
    let xml = render_test(vec![Block::Table(Box::new(table))], &[], None, &[])?;
    assert!(xml.contains(r#"<table:table-cell table:style-name="TableRowCell" office:value-type="string" table:number-columns-spanned="4">"#));
    for text in ["Group", "Label"] {
        assert!(xml.contains(&format!(
            r#"<text:p text:style-name="Table_20_Heading">{text}</text:p>"#
        )));
    }
    assert!(xml.contains(r#"<text:p text:style-name="Table_20_Contents">Value</text:p>"#));
    assert!(!xml.contains("TableHeaderRowCell"));
    assert!(!xml.contains("covered-table-cell"));
    Ok(())
}

#[test]
fn table_relative_width_is_the_rounded_sum_within_one_page_width() -> Result<()> {
    for (widths, expected) in [
        ([0.32, 0.18], Some("50%")),
        ([0.162, 0.161], Some("32%")),
        ([0.5, 0.4999], Some("100%")),
        ([0.5, 0.51], None),
        ([0.0, 0.0], None),
    ] {
        let table = Table {
            col_specs: widths
                .into_iter()
                .map(|width| ColSpec {
                    width: ColWidth::ColWidth(width),
                    ..ColSpec::default()
                })
                .collect(),
            ..Table::default()
        };
        let xml = render_test(
            vec![Block::Table(Box::new(table))],
            &[],
            Some("$automatic-styles$\n$body$"),
            &[],
        )?;
        let props = "<style:table-properties table:align=\"center\"";
        let relative = expected
            .map(|percent| format!(" style:rel-width=\"{percent}\""))
            .unwrap_or_default();
        assert!(xml.contains(&format!("{props}{relative} />")));
    }
    Ok(())
}

#[test]
fn document_direction_styles_the_heading_paragraph_and_quote() -> Result<()> {
    let heading = Block::Header(2, Attr::default(), vec![Inline::Str("Heading".into())]);
    let document = vec![
        heading,
        paragraph("First paragraph"),
        Block::BlockQuote(vec![paragraph("Quote")]),
    ];
    let xml = render_test(
        document,
        &[("dir", MetaValue::MetaString("rtl".into()))],
        Some("$automatic-styles$\n$body$"),
        &[],
    )?;
    for (index, parent) in ["Heading_20_2", FIRST, "Quotations"]
        .into_iter()
        .enumerate()
    {
        assert!(xml.contains(&format!(
            "style:name=\"P{}\" style:family=\"paragraph\" style:parent-style-name=\"{parent}\"",
            index + 1
        )));
    }
    assert!(
        xml.contains(r#"<text:h text:style-name="P1" text:outline-level="2">Heading</text:h>"#)
    );
    assert!(xml.contains(r#"<text:p text:style-name="P2">First paragraph</text:p>"#));
    assert!(xml.contains(r#"<text:p text:style-name="P3">Quote</text:p>"#));
    assert_eq!(xml.matches(RTL).count(), 3);
    Ok(())
}

#[test]
fn document_language_supplies_the_direction() -> Result<()> {
    let xml = render_test(
        vec![paragraph("Body text"), paragraph("More text")],
        &[(
            "lang",
            MetaValue::MetaInlines(vec![Inline::Str("he-IL".into())]),
        )],
        Some("$automatic-styles$\n$body$"),
        &[],
    )?;
    assert!(xml.contains(r#"style:parent-style-name="Text_20_body""#));
    assert_eq!(xml.matches(RTL).count(), 1);
    assert_eq!(xml.matches(r#"text:p text:style-name="P1""#).count(), 2);
    Ok(())
}

#[test]
fn direction_uses_central_kurdish_and_the_template_language() -> Result<()> {
    let mut options = WriterOptions::default();
    options.template = Some(
        Template::compile("unit", "$automatic-styles$\n$body$", 128, &mut |path| {
            Err(Error::App(format!("Missing test template partial {path}.")))
        })
        .map_err(|error| Error::App(error.to_string()))?,
    );
    let mut doc = Pandoc {
        blocks: vec![paragraph("Body text")],
        ..Pandoc::default()
    };
    let render = |doc: &Pandoc, options: &WriterOptions| {
        write_opendocument(doc, options, &mut Context::pure())
    };
    doc.meta
        .insert("lang", MetaValue::MetaString("ckb-IQ".into()));
    assert!(render(&doc, &options)?.contains(RTL));

    options.variables.insert("lang", Val::text("en"));
    options.variables.insert("dir", Val::text("rtl"));
    assert!(!render(&doc, &options)?.contains(RTL));

    doc.meta.insert("lang", MetaValue::MetaString("en".into()));
    for language in ["AR", "he_IL", "ckb"] {
        options.variables.insert("lang", Val::text(language));
        assert!(render(&doc, &options)?.contains(RTL), "{language}");
    }
    doc.meta.insert("dir", MetaValue::MetaString("ltr".into()));
    assert!(!render(&doc, &options)?.contains(RTL));
    Ok(())
}

#[test]
fn div_direction_is_local_to_its_content() -> Result<()> {
    let rtl = Attr::new("", vec![], vec![("dir".into(), "rtl".into())]);
    let ltr = Attr::new("", vec![], vec![("dir".into(), "ltr".into())]);
    let div = Block::Div(
        rtl,
        vec![
            paragraph("Leftward"),
            Block::Div(ltr, vec![paragraph("Rightward")]),
            paragraph("Leftward again"),
        ],
    );
    let xml = render_test(
        vec![paragraph("Before"), div, paragraph("After")],
        &[],
        Some("$automatic-styles$\n$body$"),
        &[],
    )?;
    for (style, text) in [
        (BODY, "Before"),
        ("P1", "Leftward"),
        ("P2", "Rightward"),
        ("P1", "Leftward again"),
        (BODY, "After"),
    ] {
        assert!(xml.contains(&format!(
            "<text:p text:style-name=\"{style}\">{text}</text:p>"
        )));
    }
    Ok(())
}

#[test]
fn cross_references_render_both_number_and_name() -> Result<()> {
    let heading = Block::Header(
        1,
        Attr::new("intro", vec![], vec![]),
        vec![Inline::Str("Introduction".into())],
    );
    let figure = Block::Figure(
        Attr::new("diagram", vec![], vec![]),
        Caption {
            short: None,
            long: vec![paragraph("Caption")],
        },
        vec![paragraph("Drawing")],
    );
    let link = |id| {
        Block::Para(vec![Inline::Link(
            Attr::default(),
            vec![Inline::Str("Reference".into())],
            Target::new(format!("#{id}"), ""),
        )])
    };
    let xml = render_test(
        vec![heading, figure, link("intro"), link("diagram")],
        &[],
        None,
        &[Extension::XrefsNumber, Extension::XrefsName],
    )?;
    for (kind, id, numeric, label) in [
        ("bookmark", "intro", "number", "text"),
        ("sequence", "diagram", "value", "caption"),
    ] {
        assert!(xml.contains(&format!("<text:{kind}-ref text:reference-format=\"{numeric}\" text:ref-name=\"{id}\"></text:{kind}-ref><text:s /><text:{kind}-ref text:reference-format=\"{label}\" text:ref-name=\"{id}\">Reference</text:{kind}-ref>")));
    }
    Ok(())
}

#[test]
fn block_title_preserves_inline_styles_inside_one_paragraph() -> Result<()> {
    let title = vec![
        Block::Para(vec![Inline::Emph(vec![Inline::Str("First".into())])]),
        paragraph("Second"),
    ];
    let xml = render_test(
        vec![],
        &[("title", MetaValue::MetaBlocks(title))],
        Some(r#"<text:p text:style-name="Title">$title$</text:p>"#),
        &[],
    )?;
    assert_eq!(
        xml,
        r#"<text:p text:style-name="Title"><text:span text:style-name="Emphasis">First</text:span><text:line-break />Second</text:p>"#
    );
    Ok(())
}

#[test]
fn bibliography_entries_use_first_paragraph_style_once() {
    let entry = |id, body: &str| {
        Block::Div(
            Attr::new(id, vec!["csl-entry".into()], vec![]),
            vec![Block::Para(vec![Inline::Str(body.into())])],
        )
    };
    let doc = Pandoc {
        blocks: vec![
            Block::Header(1, Attr::default(), vec![Inline::Str("Works".into())]),
            Block::Div(
                Attr::new("refs", vec!["csl-bib-body".into()], vec![]),
                vec![
                    entry("ref-a", "First entry"),
                    entry("ref-b", "Second entry"),
                ],
            ),
        ],
        ..Pandoc::default()
    };
    let output = write_opendocument(&doc, &WriterOptions::default(), &mut Context::pure()).unwrap();
    assert!(output.contains("text:style-name=\"First_20_paragraph\">First entry"));
    assert!(output.contains("text:style-name=\"Text_20_body\">Second entry"));
}
