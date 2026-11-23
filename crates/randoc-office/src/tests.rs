#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write;
use std::thread;

use randoc_archive::opc::{parse_relationships, resolve_target};
use randoc_archive::xml::{XmlDoc, XmlLimits, XmlOptions, parse};
use randoc_archive::zip::{Archive, ArchiveLimits, ArchiveWriter};
use randoc_ast::{
    Attr, Block, Cell, Inline, ListAttributes, MathType, MetaValue, Pandoc, Row, TableHead, Target,
};
use randoc_core::{
    Context, HighlightMethod, Limits, LogMessage, PureHost, ReaderOptions, Result, WriterOptions,
};

use crate::package::{A, P, R, RELS};

const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

fn zipped(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = ArchiveWriter::new();
    for (name, data) in parts {
        zip.write_deflated(name, data.as_bytes()).unwrap();
    }
    zip.finish().unwrap()
}

fn xlsx(cells: &str, strings: &str, target: &str) -> Vec<u8> {
    zipped(&[
        (
            "_rels/.rels",
            &format!(
                r#"<Relationships xmlns="{RELS}"><Relationship Id="rId1" Type="{R}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
            ),
        ),
        (
            "xl/workbook.xml",
            &format!(
                r#"<workbook xmlns="{S}" xmlns:r="{R}"><sheets><sheet name="Data Set" r:id="rId1"/></sheets></workbook>"#
            ),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            &format!(
                r#"<Relationships xmlns="{RELS}"><Relationship Id="rId1" Type="{R}/worksheet" Target="{target}"/><Relationship Id="rId2" Type="{R}/sharedStrings" Target="sharedStrings.xml"/><Relationship Id="rId3" Type="{R}/styles" Target="styles.xml"/></Relationships>"#
            ),
        ),
        (
            "xl/worksheets/sheet.xml",
            &format!(
                r#"<worksheet xmlns="{S}"><sheetData><row>{cells}</row></sheetData></worksheet>"#
            ),
        ),
        (
            "xl/sharedStrings.xml",
            &format!(r#"<sst xmlns="{S}">{strings}</sst>"#),
        ),
        (
            "xl/styles.xml",
            &format!(
                r#"<styleSheet xmlns="{S}"><fonts><font><b val="0"/><i/></font></fonts><cellXfs><xf fontId="4"/></cellXfs></styleSheet>"#
            ),
        ),
    ])
}

fn read(format: &str, bytes: &[u8], context: &mut Context) -> Result<Pandoc> {
    let randoc_core::Reader::Binary(reader) = crate::reader(format).unwrap() else {
        panic!("binary reader")
    };
    reader.read(bytes, &ReaderOptions::default(), context)
}

fn write(doc: &Pandoc, options: &WriterOptions, context: &mut Context) -> Result<Vec<u8>> {
    let randoc_core::Writer::Binary(writer) = crate::writer("pptx").unwrap() else {
        panic!("binary writer")
    };
    writer.write(doc, options, context)
}

fn on_small_stack(run: impl FnOnce() + Send + 'static) {
    thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}

fn header(level: i64, text: &str) -> Block {
    Block::Header(
        level,
        Attr::new(text, vec![], vec![]),
        vec![Inline::Str(text.into())],
    )
}

fn para(text: &str) -> Block {
    Block::Para(vec![Inline::Str(text.into())])
}

#[test]
fn xlsx_reads_serials_sparse_cells_and_fonts() {
    let bytes = xlsx(
        r#"<c r="C3" t="s"><v>0</v></c><c r="D3" s="0"><v>43363</v></c><c r="D4"><v>35287</v></c><c r="E6"/>"#,
        "<si><r><t>00</t></r><r><t>1</t></r></si>",
        "worksheets/sheet.xml",
    );
    let doc = read("xlsx", &bytes, &mut Context::pure()).unwrap();
    assert_eq!(
        doc.blocks[0],
        Block::Header(
            2,
            Attr::new("sheet-1", vec![], vec![]),
            vec![
                Inline::Str("Data".into()),
                Inline::Space,
                Inline::Str("Set".into())
            ]
        )
    );
    let Block::Table(table) = &doc.blocks[1] else {
        panic!("table")
    };
    assert_eq!(table.col_specs.len(), 3);
    assert_eq!(table.bodies[0].body.len(), 1);
    assert_eq!(
        table.head.rows[0].cells[0].content,
        [Block::Plain(vec![
            Inline::Str("00".into()),
            Inline::Space,
            Inline::Str("1".into())
        ])]
    );
    assert_eq!(
        table.head.rows[0].cells[1].content,
        [Block::Plain(vec![Inline::Emph(vec![Inline::Strong(
            vec![Inline::Str("43363.0".into())]
        )])])]
    );
    assert_eq!(
        table.bodies[0].body[0].cells[1].content,
        [Block::Plain(vec![Inline::Str("35287.0".into())])]
    );
    assert_eq!(
        table.bodies[0].body[0].cells[2].content,
        [Block::Plain(vec![])]
    );
}

#[test]
fn xlsx_preserves_absolute_target_error() {
    let bytes = xlsx("", "", "/xl/worksheets/sheet.xml");
    let error = read("xlsx", &bytes, &mut Context::pure()).unwrap_err();
    assert_eq!(error.exit_code(), 64);
    assert!(
        error
            .to_string()
            .contains("Entry not found: xl//xl/worksheets/sheet.xml")
    );
}

#[test]
fn xlsx_checks_full_extent_before_filling_gaps() {
    let bytes = xlsx(
        r#"<c r="A1"><v>1</v></c><c r="XFD1048576"><v>2</v></c>"#,
        "",
        "worksheets/sheet.xml",
    );
    let error = read("xlsx", &bytes, &mut Context::pure()).unwrap_err();
    assert!(error.to_string().contains("cell limit"));
}

#[test]
fn xlsx_charges_decoded_shared_string_for_each_reference() {
    let strings = format!("<si><t>{}</t></si>", "x".repeat(1024));
    let mut cells = String::new();
    for row in 1..=32 {
        write!(cells, r#"<c r="A{row}" t="s"><v>0</v></c>"#).unwrap();
    }
    let bytes = xlsx(&cells, &strings, "worksheets/sheet.xml");
    let mut context = Context::pure();
    context.limits.max_input_size = 8 * 1024;
    assert!(
        read("xlsx", &bytes, &mut context)
            .unwrap_err()
            .to_string()
            .contains("Shared strings exceed")
    );
}

#[test]
fn xlsx_on_two_mib_stack() {
    on_small_stack(|| {
        let mut context = Context::pure();
        context.limits = Limits::restricted();
        context.limits.max_nesting_depth = 2100;
        let deep = |depth| {
            format!(
                "<si>{}<t>ignored</t>{}</si>",
                "<x>".repeat(depth),
                "</x>".repeat(depth)
            )
        };
        let cells = r#"<c r="A1" t="s"><v>0</v></c>"#;
        let doc = read(
            "xlsx",
            &xlsx(cells, &deep(2000), "worksheets/sheet.xml"),
            &mut context,
        )
        .unwrap();
        assert_eq!(doc.blocks.len(), 2);
        drop(doc);
        assert!(
            read(
                "xlsx",
                &xlsx(cells, &deep(2100), "worksheets/sheet.xml"),
                &mut context
            )
            .is_err()
        );
    });
}

#[test]
fn pptx_reader_on_two_mib_stack() {
    on_small_stack(|| {
        let mut context = Context::pure();
        context.limits.max_nesting_depth = 2100;
        let make = |depth| {
            zipped(&[
                (
                    "ppt/presentation.xml",
                    &format!(
                        r#"<p:presentation xmlns:p="{P}" xmlns:r="{R}"><p:sldIdLst><p:sldId r:id="slide"/></p:sldIdLst></p:presentation>"#
                    ),
                ),
                (
                    "ppt/_rels/presentation.xml.rels",
                    &format!(
                        r#"<Relationships xmlns="{RELS}"><Relationship Id="slide" Type="{R}/slide" Target="slides/custom.xml"/></Relationships>"#
                    ),
                ),
                (
                    "ppt/slides/custom.xml",
                    &format!(
                        r#"<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>OK</a:t></a:r></a:p></p:txBody></p:sp>{}{}</p:spTree></p:cSld></p:sld>"#,
                        "<p:grpSp>".repeat(depth),
                        "</p:grpSp>".repeat(depth)
                    ),
                ),
            ])
        };
        let doc = read("pptx", &make(2000), &mut context).unwrap();
        assert_eq!(doc.blocks, [para("OK")]);
        drop(doc);
        assert!(read("pptx", &make(2100), &mut context).is_err());
    });
}

#[test]
fn pptx_writer_on_two_mib_stack() {
    on_small_stack(|| {
        let mut context = Context::pure();
        context.limits.max_nesting_depth = 180;
        let mut inline = Inline::Str("deep".into());
        for _ in 0..150 {
            inline = Inline::Emph(vec![inline]);
        }
        let doc = Pandoc {
            blocks: vec![Block::Para(vec![inline])],
            ..Pandoc::default()
        };
        let options = WriterOptions::default();
        let bytes = write(&doc, &options, &mut context).unwrap();
        assert_eq!(
            read("pptx", &bytes, &mut context).unwrap().blocks,
            [para("deep")]
        );
        context.limits.max_nesting_depth = 150;
        assert!(
            write(&doc, &options, &mut context)
                .unwrap_err()
                .to_string()
                .contains("nested deeper")
        );
        drop(doc);
        context.limits.max_nesting_depth = Limits::default().max_nesting_depth;
        let mut block = para("deep block");
        for depth in 0..context.limits.max_nesting_depth - 1 {
            block = match depth % 3 {
                0 => Block::Div(Attr::default(), vec![block]),
                1 => Block::BlockQuote(vec![block]),
                _ => Block::BulletList(vec![vec![block]]),
            };
        }
        let mut doc = Pandoc {
            blocks: vec![block],
            ..Pandoc::default()
        };
        let bytes = write(&doc, &options, &mut context).unwrap();
        assert_eq!(
            read("pptx", &bytes, &mut context).unwrap().blocks,
            [para("deep block")]
        );
        doc.blocks = vec![Block::BlockQuote(std::mem::take(&mut doc.blocks))];
        assert!(
            write(&doc, &options, &mut context)
                .unwrap_err()
                .to_string()
                .contains("nested deeper")
        );
    });
}

#[test]
fn pptx_uses_shared_slide_level_for_nested_heading() {
    let doc = Pandoc {
        blocks: vec![
            header(4, "A"),
            Block::Div(Attr::default(), vec![header(2, "B"), para("q")]),
        ],
        ..Pandoc::default()
    };
    let mut context = Context::pure();
    let bytes = write(&doc, &WriterOptions::default(), &mut context).unwrap();
    let round_trip = read("pptx", &bytes, &mut context).unwrap();
    assert_eq!(
        round_trip.blocks,
        [
            Block::Header(
                2,
                Attr::new("slide-1", vec![], vec![]),
                vec![Inline::Str("A".into())]
            ),
            para("B"),
            para("q")
        ]
    );
}

#[test]
fn pptx_packages_math_links_notes_images_and_animation() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB";
    let doc = Pandoc {
        blocks: vec![
            header(2, "first"),
            Block::Para(vec![
                Inline::Math(MathType::DisplayMath, "x^2".into()),
                Inline::Link(
                    Attr::default(),
                    vec![Inline::Str("next".into())],
                    Target::new("#second", ""),
                ),
                Inline::Note(vec![para("Endnote")]),
            ]),
            Block::BulletList(vec![vec![para("One")], vec![para("Two")]]),
            Block::Div(
                Attr::new("", vec!["notes".into()], vec![]),
                vec![para("Speaker one")],
            ),
            Block::Div(
                Attr::new("", vec!["notes".into()], vec![]),
                vec![para("Speaker two")],
            ),
            header(2, "second"),
            Block::Para(vec![Inline::Image(
                Attr::default(),
                vec![],
                Target::new(image, ""),
            )]),
            Block::Para(vec![Inline::Image(
                Attr::default(),
                vec![],
                Target::new(image, ""),
            )]),
        ],
        ..Pandoc::default()
    };
    let mut options = WriterOptions::default();
    options.incremental = true;
    options.table_of_contents = true;
    let host = PureHost {
        environment: BTreeMap::from([("SOURCE_DATE_EPOCH".into(), "1700000000".into())]),
        ..PureHost::default()
    };
    let mut context = Context::new(Box::new(host));
    let bytes = write(&doc, &options, &mut context).unwrap();
    assert_eq!(bytes, write(&doc, &options, &mut context).unwrap());
    let mut archive = Archive::open(&bytes, ArchiveLimits::from_limits(&context.limits)).unwrap();
    let names: HashSet<_> = archive
        .entry_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("ppt/media/"))
            .count(),
        1
    );
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("ppt/slideLayouts/slideLayout"))
            .count(),
        7
    );
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("ppt/slides/slide"))
            .count(),
        4
    );
    for path in names.iter().filter(|name| name.ends_with(".rels")) {
        let data = archive.read(path).unwrap().unwrap();
        let doc = parse(
            std::str::from_utf8(&data).unwrap(),
            &XmlLimits::default(),
            XmlOptions::default(),
        )
        .unwrap();
        let (base, file) = path.rsplit_once("_rels/").unwrap();
        let owner = format!("{base}{}", file.strip_suffix(".rels").unwrap());
        for rel in parse_relationships(&doc)
            .into_iter()
            .filter(|rel| !rel.external)
        {
            let target = resolve_target(&owner, &rel.target).unwrap();
            assert!(names.contains(&target), "{owner}: {target}");
        }
    }
    let get = |archive: &mut Archive<'_>, path| {
        String::from_utf8(archive.read(path).unwrap().unwrap()).unwrap()
    };
    let slide = get(&mut archive, "ppt/slides/slide2.xml");
    let slide_xml = parse(&slide, &XmlLimits::default(), XmlOptions::default()).unwrap();
    let mc = "http://schemas.openxmlformats.org/markup-compatibility/2006";
    let a14 = "http://schemas.microsoft.com/office/drawing/2010/main";
    let m = "http://schemas.openxmlformats.org/officeDocument/2006/math";
    let alternate = slide_xml
        .root
        .child(P, "cSld")
        .unwrap()
        .child(P, "spTree")
        .unwrap()
        .child(mc, "AlternateContent")
        .unwrap();
    let choice = alternate.child(mc, "Choice").unwrap();
    assert_eq!(choice.attr_unqualified("Requires"), Some("a14"));
    let body = choice.child(P, "sp").unwrap().child(P, "txBody").unwrap();
    let math = body
        .children_ns(A, "p")
        .next()
        .unwrap()
        .child(a14, "m")
        .unwrap();
    let alignment = math
        .child(m, "oMathPara")
        .unwrap()
        .child(m, "oMathParaPr")
        .unwrap()
        .child(m, "jc")
        .unwrap();
    assert_eq!(alignment.attr(m, "val"), Some("center"));
    assert!(slide.contains("<p:pRg st=\"1\" end=\"1\""));
    let timing = slide_xml.root.child(P, "timing").unwrap();
    fn child_timing(node: &randoc_archive::xml::XmlElement) -> &randoc_archive::xml::XmlElement {
        node.child(P, "par")
            .unwrap()
            .child(P, "cTn")
            .unwrap()
            .child(P, "childTnLst")
            .unwrap()
    }
    let seq = child_timing(timing.child(P, "tnLst").unwrap());
    let seq = seq.child(P, "seq").unwrap();
    let events = seq.child(P, "cTn").unwrap().child(P, "childTnLst").unwrap();
    let event = child_timing(events);
    let click = child_timing(event);
    let effect = click.child(P, "par").unwrap().child(P, "cTn").unwrap();
    assert_eq!(effect.attr_unqualified("nodeType"), Some("clickEffect"));
    assert_eq!(effect.attr_unqualified("grpId"), Some("0"));
    assert!(
        effect
            .child(P, "childTnLst")
            .unwrap()
            .child(P, "set")
            .is_some()
    );
    for (condition, event) in [("prevCondLst", "onPrev"), ("nextCondLst", "onNext")] {
        let condition = seq.child(P, condition).unwrap().child(P, "cond").unwrap();
        assert_eq!(condition.attr_unqualified("evt"), Some(event));
        assert!(
            condition
                .child(P, "tgtEl")
                .unwrap()
                .child(P, "sldTgt")
                .is_some()
        );
    }
    let build = timing.child(P, "bldLst").unwrap().child(P, "bldP").unwrap();
    assert_eq!(build.attr_unqualified("build"), Some("p"));
    assert_eq!(build.attr_unqualified("uiExpand"), Some("1"));
    let toc = "ppt/slides/slide1.xml";
    let toc_xml = part(&mut archive, toc);
    let toc_body = toc_xml
        .root
        .child(P, "cSld")
        .unwrap()
        .child(P, "spTree")
        .unwrap()
        .children_ns(P, "sp")
        .nth(1)
        .unwrap()
        .child(P, "txBody")
        .unwrap();
    let notes = toc_body
        .children_ns(A, "p")
        .last()
        .unwrap()
        .child(A, "r")
        .unwrap();
    assert_eq!(notes.child(A, "t").unwrap().text(), "Notes");
    let link = notes
        .child(A, "rPr")
        .unwrap()
        .child(A, "hlinkClick")
        .unwrap();
    let rels = part(&mut archive, "ppt/slides/_rels/slide1.xml.rels");
    let rel = parse_relationships(&rels)
        .into_iter()
        .find(|rel| Some(rel.id.as_str()) == link.attr(R, "id"))
        .unwrap();
    assert_eq!(
        resolve_target(toc, &rel.target).as_deref(),
        Some("ppt/slides/slide4.xml")
    );
    let notes = get(&mut archive, "ppt/notesSlides/randocNotes2.xml");
    assert!(notes.find("Speaker one").unwrap() < notes.find("Speaker two").unwrap());
    assert!(get(&mut archive, "ppt/slides/slide4.xml").contains("1. Endnote"));
    assert!(get(&mut archive, "docProps/core.xml").contains("2023-11-14T22:13:20Z"));
    let round_trip = read("pptx", &bytes, &mut context).unwrap();
    assert_eq!(context.media.len(), 1);
    assert!(round_trip.blocks.contains(&para("1. Endnote")));
}

#[test]
fn pptx_reference_lookup_prefers_explicit_resource() {
    let mut doc = Pandoc::default();
    doc.meta
        .insert("title", MetaValue::MetaString("Title".into()));
    let mut options = WriterOptions::default();
    let reference = write(&doc, &options, &mut Context::pure()).unwrap();
    let host = PureHost {
        files: BTreeMap::from([
            ("assets/example.pptx".into(), reference),
            ("data/reference.pptx".into(), b"invalid reference".to_vec()),
        ]),
        ..PureHost::default()
    };
    let mut context = Context::new(Box::new(host));
    context.state.user_data_dir = Some("data".into());
    context.state.resource_path = vec!["assets".into()];
    assert!(write(&doc, &options, &mut context).is_err());
    options.reference_doc = Some("example.pptx".into());
    let bytes = write(&doc, &options, &mut context).unwrap();
    let result = read("pptx", &bytes, &mut context).unwrap();
    assert_eq!(result.blocks.len(), 1);
}

fn part(archive: &mut Archive<'_>, path: &str) -> XmlDoc {
    let bytes = archive.read(path).unwrap().unwrap();
    parse(
        std::str::from_utf8(&bytes).unwrap(),
        &XmlLimits::default(),
        XmlOptions::default(),
    )
    .unwrap()
}

#[test]
fn xlsx_accepts_dense_sheet_with_zero_padding_budget() {
    let mut cells = String::new();
    for row in 1..=450 {
        for col in 1..=450 {
            let (first, second) = ((col - 1) / 26, (col - 1) % 26);
            let second = char::from(b'A' + second as u8);
            let first = if first == 0 {
                String::new()
            } else {
                char::from(b'A' + (first - 1) as u8).to_string()
            };
            write!(cells, r#"<c r="{first}{second}{row}"><v>1</v></c>"#).unwrap();
        }
    }
    let bytes = xlsx(&cells, "", "worksheets/sheet.xml");
    let mut context = Context::pure();
    context.limits.max_table_cells = 0;
    let doc = read("xlsx", &bytes, &mut context).unwrap();
    let Block::Table(table) = &doc.blocks[1] else {
        panic!("table")
    };
    let rows = table.head.rows.iter().chain(&table.bodies[0].body);
    assert_eq!(rows.clone().count(), 450);
    assert!(rows.clone().all(|row| row.cells.len() == 450));
    assert_eq!(
        rows.last().unwrap().cells[449].content,
        [Block::Plain(vec![Inline::Str("1.0".into())])]
    );
}

#[test]
fn xlsx_trims_trailing_rows_without_visible_text() {
    let bytes = xlsx(
        concat!(
            r#"<c r="A1"/><c r="A2"><v>2</v></c><c r="B2" s="0"/>"#,
            r#"<c r="A3" s="0"/><c r="A4" s="0" t="str"><v> </v></c>"#,
            r#"<c r="C5" t="str"><v> &#xA; </v></c>"#,
        ),
        "",
        "worksheets/sheet.xml",
    );
    let doc = read("xlsx", &bytes, &mut Context::pure()).unwrap();
    let Block::Table(table) = &doc.blocks[1] else {
        panic!("table")
    };
    assert_eq!(table.head.rows[0].cells[0].content, [Block::Plain(vec![])]);
    assert_eq!(table.col_specs.len(), 3);
    assert_eq!(table.bodies[0].body.len(), 1);
    assert_eq!(
        table.bodies[0].body[0].cells[1].content,
        [Block::Plain(vec![Inline::Emph(vec![Inline::Strong(
            vec![]
        )])])]
    );
}

#[test]
fn pptx_charges_only_generated_cells() {
    let mut table = randoc_ast::Table {
        head: TableHead {
            rows: vec![
                Row {
                    cells: vec![Cell::default(); 2],
                    ..Row::default()
                };
                2
            ],
            ..TableHead::default()
        },
        ..randoc_ast::Table::default()
    };
    let mut context = Context::pure();
    context.limits.max_table_cells = 0;
    let options = WriterOptions::default();
    let document = |table| Pandoc {
        blocks: vec![Block::Table(Box::new(table))],
        ..Pandoc::default()
    };
    let bytes = write(&document(table.clone()), &options, &mut context).unwrap();
    let doc = read("pptx", &bytes, &mut context).unwrap();
    let Block::Table(dense) = &doc.blocks[0] else {
        panic!("table")
    };
    assert_eq!(dense.head.rows[0].cells.len(), 2);
    assert_eq!(dense.bodies[0].body[0].cells.len(), 2);

    table.head.rows[0].cells = vec![Cell {
        col_span: 2,
        row_span: 2,
        ..Cell::default()
    }];
    table.head.rows[1].cells.truncate(1);
    let doc = document(table);
    context.limits.max_table_cells = 2;
    assert!(
        write(&doc, &options, &mut context)
            .unwrap_err()
            .to_string()
            .contains("cell limit")
    );
    context.limits.max_table_cells = 3;
    let bytes = write(&doc, &options, &mut context).unwrap();

    let mut pkg = Archive::open(&bytes, ArchiveLimits::from_limits(&context.limits)).unwrap();
    let mut output = ArchiveWriter::new();
    for name in pkg
        .entry_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>()
    {
        let mut bytes = pkg.read(&name).unwrap().unwrap();
        if name == "ppt/slides/slide1.xml" {
            let slide = std::str::from_utf8(&bytes).unwrap();
            let (before, after) = slide.split_once("<a:tc>").unwrap();
            let (_, after) = after.split_once("</a:tc>").unwrap();
            bytes = format!("{before}{after}").into_bytes();
        }
        output.write_deflated(&name, &bytes).unwrap();
    }
    let bytes = output.finish().unwrap();
    context.limits.max_table_cells = 0;
    assert!(
        read("pptx", &bytes, &mut context)
            .unwrap_err()
            .to_string()
            .contains("cell limit")
    );
    context.limits.max_table_cells = 1;
    assert!(read("pptx", &bytes, &mut context).is_ok());
}

#[test]
fn pptx_highlights_code_with_the_selected_style() {
    let doc = Pandoc {
        blocks: vec![Block::CodeBlock(
            Attr::new("", vec!["python".into()], vec![]),
            "def f():\n    return 2".into(),
        )],
        ..Pandoc::default()
    };
    let mut options = WriterOptions::default();
    let slide = |options: &WriterOptions| {
        let mut context = Context::pure();
        let bytes = write(&doc, options, &mut context).unwrap();
        let mut archive =
            Archive::open(&bytes, ArchiveLimits::from_limits(&context.limits)).unwrap();
        String::from_utf8(archive.read("ppt/slides/slide1.xml").unwrap().unwrap()).unwrap()
    };
    let pygments = slide(&options);
    assert!(pygments.contains(r#"<a:pPr lvl="0" indent="0"><a:buNone/>"#));
    assert!(pygments.contains(r#"<a:rPr b="1"><a:solidFill><a:srgbClr val="007020"/></a:solidFill><a:latin typeface="Courier"/></a:rPr><a:t>def</a:t>"#));
    assert!(pygments.contains("<a:br/>"));
    options.highlight_method = HighlightMethod::Style("monochrome".into());
    assert_ne!(slide(&options), pygments);
    options.highlight_method = HighlightMethod::None;
    let plain = slide(&options);
    assert!(!plain.contains("srgbClr"));
    assert!(plain.contains("<a:t>def f():\n    return 2</a:t>"));
}

#[test]
fn pptx_master_styles_and_ordered_list_indents() {
    let doc = Pandoc {
        blocks: vec![Block::OrderedList(
            ListAttributes::default(),
            vec![vec![
                para("Outer"),
                Block::OrderedList(
                    ListAttributes {
                        start: 4,
                        ..ListAttributes::default()
                    },
                    vec![vec![para("Inner")]],
                ),
            ]],
        )],
        ..Pandoc::default()
    };
    let mut context = Context::pure();
    let bytes = write(&doc, &WriterOptions::default(), &mut context).unwrap();
    let mut pkg = Archive::open(&bytes, ArchiveLimits::from_limits(&context.limits)).unwrap();
    let master = part(&mut pkg, "ppt/slideMasters/slideMaster1.xml");
    let styles = master.root.child(P, "txStyles").unwrap();
    assert!(styles.child(P, "titleStyle").is_some());
    assert!(styles.child(P, "otherStyle").is_some());
    let body = styles.child(P, "bodyStyle").unwrap();
    for level in 1..=9 {
        let para = body.child(A, &format!("lvl{level}pPr")).unwrap();
        assert_eq!(
            para.attr_unqualified("marL")
                .unwrap()
                .parse::<usize>()
                .unwrap(),
            342_900 * level
        );
        assert_eq!(para.attr_unqualified("indent"), Some("-285750"));
        assert!(para.child(A, "buChar").is_some());
    }
    let main = "ppt/presentation.xml";
    let rels = parse_relationships(&part(&mut pkg, "ppt/_rels/presentation.xml.rels"));
    let types = part(&mut pkg, "[Content_Types].xml");
    for kind in ["presProps", "viewProps", "tableStyles"] {
        let rel = rels.iter().find(|rel| rel.kind.ends_with(kind)).unwrap();
        let path = resolve_target(main, &rel.target).unwrap();
        assert!(pkg.contains(&path));
        assert!(
            types
                .root
                .elements()
                .any(|node| node.attr_unqualified("PartName") == Some(&format!("/{path}")))
        );
    }
    let theme = part(&mut pkg, "ppt/theme/theme1.xml");
    assert_eq!(theme.root.attr_unqualified("name"), Some("Randoc"));
    let slide = String::from_utf8(pkg.read("ppt/slides/slide1.xml").unwrap().unwrap()).unwrap();
    assert!(slide.contains(
        r#"<a:pPr lvl="0" indent="-342900" marL="342900"><a:buAutoNum type="arabicPeriod"/>"#
    ));
    assert!(slide.contains(r#"<a:pPr lvl="1" indent="-342900" marL="685800"><a:buAutoNum type="arabicPeriod" startAt="4"/>"#));
}

#[test]
fn pptx_falls_back_to_the_missing_named_layout() {
    let mut context = Context::pure();
    let doc = Pandoc {
        blocks: vec![header(1, "Section"), header(2, "Content"), para("Text")],
        ..Pandoc::default()
    };
    let mut options = WriterOptions::default();
    options.slide_level = Some(2);
    let bytes = write(&doc, &options, &mut context).unwrap();
    let mut package = Archive::open(&bytes, ArchiveLimits::from_limits(&context.limits)).unwrap();
    let mut modified = ArchiveWriter::new();
    for name in package
        .entry_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>()
    {
        let bytes = package.read(&name).unwrap().unwrap();
        if name.starts_with("ppt/slideLayouts/slideLayout") {
            let xml = std::str::from_utf8(&bytes)
                .unwrap()
                .replace("Section Header", "Former Section Header");
            modified.write_deflated(&name, xml.as_bytes()).unwrap();
        } else {
            modified.write_deflated(&name, &bytes).unwrap();
        }
    }
    let host = PureHost {
        files: BTreeMap::from([
            ("ref.pptx".into(), modified.finish().unwrap()),
            ("bad.pptx".into(), b"invalid ZIP".to_vec()),
        ]),
        ..PureHost::default()
    };
    let mut context = Context::new(Box::new(host));
    options.reference_doc = Some("bad.pptx".into());
    let error = write(&doc, &options, &mut context).unwrap_err();
    assert_eq!(error.exit_code(), 64);
    assert_eq!(
        error.to_string(),
        "Could not parse reference-doc \"bad.pptx\": Did not find end of central directory signature"
    );
    options.reference_doc = Some("ref.pptx".into());
    let bytes = write(&doc, &options, &mut context).unwrap();
    assert!(
        matches!(context.log(), [LogMessage::PowerpointTemplateWarning(message)] if message == "Couldn't find layout named \"Section Header\" in provided reference doc. Falling back to the default included with pandoc.")
    );
    let mut package = Archive::open(&bytes, ArchiveLimits::from_limits(&context.limits)).unwrap();
    let slide = "ppt/slides/slide1.xml";
    let rels = parse_relationships(&part(&mut package, "ppt/slides/_rels/slide1.xml.rels"));
    let layout = resolve_target(slide, &rels[0].target).unwrap();
    let layout_xml = part(&mut package, &layout);
    assert_eq!(
        layout_xml
            .root
            .child(P, "cSld")
            .unwrap()
            .attr_unqualified("name"),
        Some("Section Header")
    );
    let rels = parse_relationships(&part(
        &mut package,
        &randoc_archive::opc::rels_part_for(&layout),
    ));
    let master = resolve_target(&layout, &rels[0].target).unwrap();
    assert!(
        part(&mut package, &master)
            .root
            .child(P, "txStyles")
            .is_some()
    );
    let rels = parse_relationships(&part(&mut package, "ppt/_rels/presentation.xml.rels"));
    assert!(rels.iter().any(
        |rel| resolve_target("ppt/presentation.xml", &rel.target).as_deref() == Some(&master)
    ));
}

#[test]
fn pptx_toc_handles_minimum_heading_level_and_keeps_column_notes() {
    let class = |name: &str| Attr::new("", vec![name.into()], vec![]);
    let doc = Pandoc {
        blocks: vec![
            header(i64::MIN, "Minimum level"),
            para("Before columns"),
            Block::Div(
                class("columns"),
                vec![Block::Div(
                    class("column"),
                    vec![
                        Block::Div(class("notes"), vec![para("Column notes")]),
                        para("Column text"),
                    ],
                )],
            ),
        ],
        ..Pandoc::default()
    };
    let mut options = WriterOptions::default();
    options.table_of_contents = true;
    options.slide_level = Some(2);
    let mut context = Context::pure();
    let bytes = write(&doc, &options, &mut context).unwrap();
    let mut package = Archive::open(&bytes, ArchiveLimits::from_limits(&context.limits)).unwrap();
    let toc = String::from_utf8(package.read("ppt/slides/slide1.xml").unwrap().unwrap()).unwrap();
    assert!(toc.contains(r#"<a:pPr lvl="0"/>"#));
    assert!(toc.contains("Minimum level"));
    assert!(!toc.contains(">Notes<"));
    let notes = String::from_utf8(
        package
            .read("ppt/notesSlides/randocNotes3.xml")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert!(notes.contains("Column notes"));
}
