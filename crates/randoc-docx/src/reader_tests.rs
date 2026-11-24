//! Reader rules found with black-box probes of Pandoc 3.13: emoji runs,
//! horizontal rules, table rows, captions, list continuations, comment ends
//! and table styles. Each input is a small document written for the test.

#![allow(clippy::unwrap_used)]

use randoc_archive::zip::ArchiveWriter;
use randoc_ast::{Attr, Block, Inline, ListAttributes, Pandoc, Table};
use randoc_core::{BinaryWriter, Context, Extension, ReaderOptions, TrackChanges, WriterOptions};

const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NAMESPACES: &str = concat!(
    r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
    r#"xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" "#,
    r#"xmlns:w16se="http://schemas.microsoft.com/office/word/2015/wordml/symex" "#,
    r#"xmlns:v="urn:schemas-microsoft-com:vml" "#,
    r#"xmlns:o="urn:schemas-microsoft-com:office:office" "#,
    r#"mc:Ignorable="w16se""#,
);

/// The optional parts of a test document.
#[derive(Default)]
struct Parts<'a> {
    styles: &'a str,
    numbering: &'a str,
    comments: &'a str,
}

fn docx(body: &str, parts: &Parts<'_>) -> Vec<u8> {
    let mut archive = ArchiveWriter::new();
    let mut rels = String::new();
    for (kind, name, root, content) in [
        ("styles", "styles.xml", "styles", parts.styles),
        ("numbering", "numbering.xml", "numbering", parts.numbering),
        ("comments", "comments.xml", "comments", parts.comments),
    ] {
        if !content.is_empty() {
            rels.push_str(&format!(
                r#"<Relationship Id="r{kind}" Type="{R}/{kind}" Target="{name}"/>"#
            ));
            let xml = format!("<w:{root} {NAMESPACES}>{content}</w:{root}>");
            archive
                .write_deflated(&format!("word/{name}"), xml.as_bytes())
                .unwrap();
        }
    }
    let package =
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#;
    let root_rels = format!(
        r#"{package}<Relationship Id="rId1" Type="{R}/officeDocument" Target="word/document.xml"/></Relationships>"#
    );
    archive
        .write_deflated("_rels/.rels", root_rels.as_bytes())
        .unwrap();
    archive
        .write_deflated(
            "word/_rels/document.xml.rels",
            format!("{package}{rels}</Relationships>").as_bytes(),
        )
        .unwrap();
    let document = format!("<w:document {NAMESPACES}><w:body>{body}</w:body></w:document>");
    archive
        .write_deflated("word/document.xml", document.as_bytes())
        .unwrap();
    archive.finish().unwrap()
}

fn read_with(bytes: &[u8], options: &ReaderOptions) -> Vec<Block> {
    crate::convert::read_docx(bytes, options, &mut Context::pure())
        .unwrap()
        .blocks
}

fn read(body: &str, parts: &Parts<'_>) -> Vec<Block> {
    read_with(&docx(body, parts), &ReaderOptions::default())
}

/// Writes `blocks` with the DOCX writer and reads the result again, as the
/// upstream round-trip commands do.
fn round_trip(blocks: Vec<Block>, options: &ReaderOptions) -> Vec<Block> {
    let doc = Pandoc {
        blocks,
        ..Pandoc::default()
    };
    let bytes = crate::writer::DocxWriter
        .write(&doc, &WriterOptions::default(), &mut Context::pure())
        .unwrap();
    read_with(&bytes, options)
}

fn text(value: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    for (index, word) in value.split(' ').enumerate() {
        if index > 0 {
            out.push(Inline::Space);
        }
        out.push(Inline::Str(word.into()));
    }
    out
}

fn para(value: &str) -> Block {
    Block::Para(text(value))
}

fn run(value: &str) -> String {
    format!(r#"<w:r><w:t xml:space="preserve">{value}</w:t></w:r>"#)
}

fn paragraph(properties: &str, value: &str) -> String {
    format!("<w:p><w:pPr>{properties}</w:pPr>{}</w:p>", run(value))
}

const BORDER: &str =
    r#"<w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="auto"/></w:pBdr>"#;

#[test]
fn run_with_alternate_content_is_its_fallback() {
    let emoji = |code: &str, fallback: &str| {
        format!(
            r#"<w:r><w:rPr><mc:AlternateContent><mc:Choice Requires="w16se"/><mc:Fallback><w:rFonts w:ascii="Segoe UI Emoji" w:hAnsi="Segoe UI Emoji"/></mc:Fallback></mc:AlternateContent></w:rPr><mc:AlternateContent><mc:Choice Requires="w16se"><w16se:symEx w16se:font="Segoe UI Emoji" w16se:char="{code}"/></mc:Choice><mc:Fallback><w:t>{fallback}</w:t></mc:Fallback></mc:AlternateContent></w:r>"#
        )
    };
    let body = [
        format!(
            "<w:p>{}{}{}</w:p>",
            run("Colored:"),
            emoji("2764", "\u{2764}"),
            emoji("1F60F", "\u{1F60F}")
        ),
        // The other children of the run and its properties do not count.
        r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>P</w:t><mc:AlternateContent><mc:Choice Requires="w16se"><w:t>C</w:t></mc:Choice><mc:Fallback><w:t>Q</w:t></mc:Fallback></mc:AlternateContent><w:t>R</w:t></w:r></w:p>"#.into(),
        // Without a fallback, the run is an ordinary run.
        r#"<w:p><w:r><w:t>P</w:t><mc:AlternateContent><mc:Choice Requires="w16se"><w:t>C</w:t></mc:Choice></mc:AlternateContent><w:t>R</w:t></w:r></w:p>"#.into(),
        // The first `w:rFonts` at any depth gives the symbol font.
        r#"<w:p><w:r><w:rPr><mc:AlternateContent><mc:Choice Requires="w16se"/><mc:Fallback><w:rFonts w:ascii="Wingdings" w:hAnsi="Wingdings"/></mc:Fallback></mc:AlternateContent></w:rPr><w:t>&#xF04A;</w:t></w:r></w:p>"#.into(),
        r#"<w:p><w:r><w:rPr><w:rFonts w:ascii="Symbol"/><mc:AlternateContent><mc:Choice Requires="w16se"/><mc:Fallback><w:rFonts w:ascii="Wingdings"/></mc:Fallback></mc:AlternateContent></w:rPr><w:t>&#xF04A;</w:t></w:r></w:p>"#.into(),
    ]
    .concat();
    assert_eq!(
        read(&body, &Parts::default()),
        [
            para("Colored:\u{2764}\u{1F60F}"),
            para("Q"),
            para("PR"),
            para("\u{263A}"),
            para("\u{03D1}"),
        ]
    );
}

#[test]
fn bottom_border_adds_a_horizontal_rule_after_the_paragraph() {
    let styles = r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Ruled"><w:name w:val="Ruled"/><w:pPr><w:pBdr><w:bottom w:val="single"/></w:pBdr></w:pPr></w:style><w:style w:type="paragraph" w:styleId="BlockText"><w:name w:val="Block Text"/></w:style>"#;
    let numbering = r#"<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="*"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#;
    let item = r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>"#;
    let body = [
        paragraph(BORDER, "a1"),
        paragraph(r#"<w:pBdr><w:bottom w:val="none"/></w:pBdr>"#, "none"),
        paragraph(r#"<w:pBdr><w:top w:val="single"/></w:pBdr>"#, "top"),
        paragraph(
            &format!(r#"<w:pStyle w:val="Heading1"/>{BORDER}"#),
            "Heading",
        ),
        paragraph(r#"<w:pStyle w:val="Ruled"/>"#, "styled"),
        format!("<w:p><w:pPr>{BORDER}</w:pPr></w:p>"),
        paragraph(&format!("{item}{BORDER}"), "li1"),
        paragraph(item, "li2"),
        paragraph(&format!(r#"<w:pStyle w:val="BlockText"/>{BORDER}"#), "q1"),
        paragraph(r#"<w:pStyle w:val="BlockText"/>"#, "q2"),
    ]
    .concat();
    let parts = Parts {
        styles,
        numbering,
        ..Parts::default()
    };
    assert_eq!(
        read(&body, &parts),
        [
            para("a1"),
            Block::HorizontalRule,
            para("none"),
            Block::HorizontalRule,
            para("top"),
            Block::Header(1, Attr::new("heading", vec![], vec![]), text("Heading")),
            para("styled"),
            Block::HorizontalRule,
            Block::BulletList(vec![
                vec![para("li1"), Block::HorizontalRule],
                vec![para("li2")],
            ]),
            Block::BlockQuote(vec![para("q1")]),
            Block::HorizontalRule,
            Block::BlockQuote(vec![para("q2")]),
        ]
    );
}

#[test]
fn vml_rule_paragraph_is_a_horizontal_rule() {
    let rule = r#"<w:r><w:pict><v:rect style="width:0;height:1.5pt" o:hralign="center" o:hrstd="t" o:hr="t"/></w:pict></w:r>"#;
    let body = [
        format!("<w:p>{rule}</w:p>"),
        // Paragraph properties, other content or another shape: no rule.
        format!("<w:p><w:pPr></w:pPr>{rule}</w:p>"),
        format!("<w:p>{rule}{rule}</w:p>"),
        r#"<w:p><w:r><w:pict><v:line o:hr="t"/></w:pict></w:r></w:p>"#.into(),
        format!("<w:p>{}{rule}</w:p>", run("text")),
    ]
    .concat();
    assert_eq!(
        read(&body, &Parts::default()),
        [Block::HorizontalRule, para("text")]
    );
    assert_eq!(
        round_trip(vec![Block::HorizontalRule], &ReaderOptions::default()),
        [Block::HorizontalRule]
    );
}

/// Returns the text of each cell of the head rows and of the body rows.
fn table_text(block: &Block) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
    let Block::Table(table) = block else {
        return (Vec::new(), Vec::new());
    };
    let rows = |rows: &[randoc_ast::Row]| {
        rows.iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| randoc_core::shared::stringify_blocks(&cell.content))
                    .collect()
            })
            .collect()
    };
    let Table { head, bodies, .. } = table.as_ref();
    let body: Vec<_> = bodies.iter().flat_map(|body| body.body.clone()).collect();
    (rows(&head.rows), rows(&body))
}

fn cell(value: &str) -> String {
    format!("<w:tc><w:p>{}</w:p></w:tc>", run(value))
}

fn table_row(header: Option<bool>, cells: &[&str]) -> String {
    let properties = match header {
        Some(true) => r#"<w:trPr><w:tblHeader/></w:trPr>"#,
        Some(false) => r#"<w:trPr><w:tblHeader w:val="0"/></w:trPr>"#,
        None => "",
    };
    let cells: String = cells.iter().map(|value| cell(value)).collect();
    format!("<w:tr>{properties}{cells}</w:tr>")
}

#[test]
fn head_has_the_rows_before_the_first_header_row_and_each_header_row() {
    let grid = r#"<w:tblGrid><w:gridCol w:w="1000"/></w:tblGrid>"#;
    // The `w:tblHeader` flag of each row (`1` true, `0` false, `-` absent),
    // then the rows of the head and the rows of the body.
    let cases = [
        ("0,1,-", "1 2", "3"),
        ("-,-,1", "1 2 3", ""),
        ("1,0,1", "1 3", "2"),
        ("-,1,-,1", "1 2 4", "3"),
        ("1,-", "1", "2"),
        ("0,0", "", "1 2"),
    ];
    for (flags, head, body) in cases {
        let rows: String = flags
            .split(',')
            .enumerate()
            .map(|(index, flag)| {
                let flag = match flag {
                    "1" => Some(true),
                    "0" => Some(false),
                    _ => None,
                };
                table_row(flag, &[&(index + 1).to_string()])
            })
            .collect();
        let blocks = read(&format!("<w:tbl>{grid}{rows}</w:tbl>"), &Parts::default());
        let (head_rows, body_rows) = table_text(&blocks[0]);
        let first = |rows: Vec<Vec<String>>| {
            rows.into_iter()
                .map(|row| row[0].clone())
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(first(head_rows), head, "{flags}");
        assert_eq!(first(body_rows), body, "{flags}");
    }
}

#[test]
fn rows_take_the_width_of_the_first_body_row() {
    let grid = r#"<w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/></w:tblGrid>"#;
    let empty = String::new;
    // A short row gets empty cells; the grid limits the width.
    let blocks = read(
        &format!(
            "<w:tbl>{grid}{}{}{}</w:tbl>",
            table_row(Some(true), &["h"]),
            table_row(None, &["a", "b", "c", "x"]),
            table_row(None, &["d"]),
        ),
        &Parts::default(),
    );
    let (head, body) = table_text(&blocks[0]);
    assert_eq!(head, [vec!["h".to_owned(), empty(), empty()]]);
    assert_eq!(
        body,
        [
            vec!["a".to_owned(), "b".into(), "c".into()],
            vec!["d".to_owned(), empty(), empty()],
        ]
    );
    // Without a header row, the first row gives the width.
    let blocks = read(
        &format!(
            "<w:tbl>{grid}{}{}</w:tbl>",
            table_row(None, &["one"]),
            table_row(None, &["y", "z"]),
        ),
        &Parts::default(),
    );
    let Block::Table(table) = &blocks[0] else {
        panic!("{blocks:?}");
    };
    assert_eq!(table.col_specs.len(), 1);
    assert_eq!(table_text(&blocks[0]).1, [vec!["one"], vec!["y"]]);
}

#[test]
fn newline_in_run_text_is_a_soft_break() {
    let body = [
        "<w:p><w:r><w:t>question\n</w:t></w:r><w:r><w:t>2\n</w:t></w:r></w:p>",
        "<w:p><w:r><w:t>e \n f</w:t></w:r></w:p>",
        "<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>p\n</w:t></w:r><w:r><w:t> q</w:t></w:r></w:p>",
    ]
    .concat();
    let soft = |a: &str, b: &str| {
        Block::Para(vec![
            Inline::Str(a.into()),
            Inline::SoftBreak,
            Inline::Str(b.into()),
        ])
    };
    assert_eq!(
        read(&body, &Parts::default()),
        [
            soft("question", "2"),
            soft("e", "f"),
            Block::Para(vec![
                Inline::Strong(vec![Inline::Str("p".into())]),
                Inline::SoftBreak,
                Inline::Str("q".into()),
            ]),
        ]
    );
}

const CAPTION_STYLES: &str = r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Caption"><w:name w:val="caption"/></w:style><w:style w:type="paragraph" w:styleId="BodyText"><w:name w:val="Body Text"/></w:style>"#;

fn simple_field(instruction: &str, value: &str) -> String {
    format!(
        r#"<w:fldSimple w:instr="{instruction}">{}</w:fldSimple>"#,
        run(value)
    )
}

#[test]
fn caption_drops_the_label_of_a_simple_table_or_figure_field() {
    let caption = |content: String| {
        format!(r#"<w:p><w:pPr><w:pStyle w:val="Caption"/></w:pPr>{content}</w:p>"#)
    };
    let bookmark = r#"<w:bookmarkStart w:id="0" w:name="_Ref1"/>"#;
    let complex = r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> SEQ Table \* ARABIC </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>2</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#;
    let body = [
        caption(format!(
            "{bookmark}{}{}{}<w:bookmarkEnd w:id=\"0\"/>",
            run("Tabelle "),
            simple_field(r" SEQ Table \* ARABIC ", "1"),
            run(" My table!")
        )),
        caption(format!(
            "{}{}{}",
            run("Figure "),
            simple_field(" SEQ Figure ", "1"),
            run(" Blue.")
        )),
        // Another sequence name or a complex field keeps the label.
        caption(format!(
            "{}{}{}",
            run("Figur "),
            simple_field(" SEQ Figur ", "1"),
            run(" Red.")
        )),
        caption(format!("{}{complex}{}", run("Table "), run(": complex"))),
        // Only the first label field counts.
        caption(format!(
            "{}{}{}{}",
            run("A "),
            simple_field(" SEQ Table ", "4"),
            run(" B "),
            simple_field(" SEQ Figure ", "5"),
        )),
        format!(
            r#"<w:p><w:hyperlink w:anchor="_Ref1">{}</w:hyperlink></w:p>"#,
            run("See")
        ),
    ]
    .concat();
    let parts = Parts {
        styles: CAPTION_STYLES,
        ..Parts::default()
    };
    let anchor = Inline::Span(Attr::new("_Ref1", vec!["anchor".into()], vec![]), vec![]);
    let mut labelled = vec![anchor, Inline::Space];
    labelled.extend(text("My table!"));
    assert_eq!(
        read(&body, &parts),
        [
            Block::Para(labelled),
            para("Blue."),
            para("Figur 1 Red."),
            para("Table 2: complex"),
            para("B 5"),
            Block::Para(vec![Inline::Link(
                Attr::default(),
                text("See"),
                randoc_ast::Target {
                    url: "#_Ref1".into(),
                    title: String::new(),
                },
            )]),
        ]
    );
}

#[test]
fn only_a_caption_style_makes_a_table_caption() {
    let table = format!(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="1000"/></w:tblGrid>{}</w:tbl>"#,
        table_row(None, &["a"])
    );
    let label = |style: &str| {
        format!(
            r#"<w:p><w:pPr><w:pStyle w:val="{style}"/></w:pPr>{}{}{}</w:p>"#,
            run("Table "),
            simple_field(" SEQ Table ", "1"),
            run(" Caption text.")
        )
    };
    let parts = Parts {
        styles: CAPTION_STYLES,
        ..Parts::default()
    };
    let blocks = read(&format!("{table}{}", label("BodyText")), &parts);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[1], para("Table 1 Caption text."));
    let blocks = read(&format!("{table}{}", label("Caption")), &parts);
    let [Block::Table(table)] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    assert_eq!(table.caption.long, [para("Caption text.")]);
}

#[test]
fn continuation_paragraph_joins_the_open_item_with_the_same_indent() {
    let numbering = r#"<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="*"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl><w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/><w:lvlText w:val="-"/><w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#;
    let item = |level: u8, value: &str| {
        paragraph(
            &format!(r#"<w:numPr><w:ilvl w:val="{level}"/><w:numId w:val="1"/></w:numPr>"#),
            value,
        )
    };
    let indented =
        |left: u16, value: &str| paragraph(&format!(r#"<w:ind w:left="{left}"/>"#), value);
    let parts = Parts {
        numbering,
        ..Parts::default()
    };
    let body = [
        item(0, "one"),
        item(1, "nested"),
        indented(1440, "nested sub"),
        indented(720, "back"),
        item(1, "again"),
    ]
    .concat();
    assert_eq!(
        read(&body, &parts),
        [Block::BulletList(vec![vec![
            para("one"),
            Block::BulletList(vec![vec![para("nested"), para("nested sub")]]),
            para("back"),
            Block::BulletList(vec![vec![para("again")]]),
        ]])]
    );
    // Another indent ends the list.
    let body = [item(0, "one"), item(1, "nested"), indented(1000, "other")].concat();
    assert_eq!(
        read(&body, &parts),
        [
            Block::BulletList(vec![vec![
                para("one"),
                Block::BulletList(vec![vec![para("nested")]]),
            ]]),
            Block::BlockQuote(vec![para("other")]),
        ]
    );
    // The direct indent of the item paragraph counts, not the level indent.
    let body = [
        paragraph(
            r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr><w:ind w:left="540"/>"#,
            "item",
        ),
        indented(540, "goal"),
    ]
    .concat();
    assert_eq!(
        read(&body, &parts),
        [Block::BulletList(vec![vec![para("item"), para("goal")]])]
    );
    let list = vec![Block::BulletList(vec![
        vec![
            para("one"),
            para("sub"),
            Block::BulletList(vec![vec![para("nested"), para("nested sub")]]),
            para("back"),
        ],
        vec![para("two")],
    ])];
    assert_eq!(round_trip(list.clone(), &ReaderOptions::default()), list);
}

fn comment_end(id: &str, inner: Vec<Inline>) -> Inline {
    Inline::Span(
        Attr::new(
            "",
            vec!["comment-end".into()],
            vec![("comment-id".into(), id.into())],
        ),
        inner,
    )
}

#[test]
fn adjacent_comment_ends_nest() {
    let comments: String = (1..=3)
        .map(|id| {
            format!(
                r#"<w:comment w:id="{id}" w:author="A">{}</w:comment>"#,
                paragraph("", "c")
            )
        })
        .collect();
    let start = |id: u8| format!(r#"<w:commentRangeStart w:id="{id}"/>"#);
    let end = |id: u8| {
        format!(r#"<w:commentRangeEnd w:id="{id}"/><w:r><w:commentReference w:id="{id}"/></w:r>"#)
    };
    let body = [
        format!(
            "<w:p>{}{}{}{}{}{}{}</w:p>",
            start(1),
            start(2),
            start(3),
            run("A"),
            end(1),
            end(2),
            end(3)
        ),
        format!(
            "<w:p>{}{}{}{}{}{}</w:p>",
            start(1),
            start(2),
            run("B"),
            end(1),
            run("x"),
            end(2)
        ),
    ]
    .concat();
    let parts = Parts {
        comments: &comments,
        ..Parts::default()
    };
    let mut options = ReaderOptions::default();
    options.track_changes = TrackChanges::AllChanges;
    let blocks = read_with(&docx(&body, &parts), &options);
    let ends = |block: &Block| {
        match block {
        Block::Para(inlines) => inlines
            .iter()
            .filter(|inline| {
                !matches!(inline, Inline::Span(attr, _) if attr.classes.as_slice() == ["comment-start"])
            })
            .cloned()
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    }
    };
    assert_eq!(
        ends(&blocks[0]),
        [
            Inline::Str("A".into()),
            comment_end("1", vec![comment_end("2", vec![comment_end("3", vec![])])]),
        ]
    );
    assert_eq!(
        ends(&blocks[1]),
        [
            Inline::Str("B".into()),
            comment_end("1", vec![]),
            Inline::Str("x".into()),
            comment_end("2", vec![]),
        ]
    );

    let comment_start = |id: &str, value: &str| {
        Inline::Span(
            Attr::new(
                id,
                vec!["comment-start".into()],
                vec![("author".into(), "Mike".into())],
            ),
            text(value),
        )
    };
    let end_span =
        |id: &str, inner| Inline::Span(Attr::new(id, vec!["comment-end".into()], vec![]), inner);
    let mut source = vec![
        comment_start("1", "comment"),
        Inline::Space,
        comment_start("2", "reply"),
        Inline::Space,
    ];
    source.extend(text("content "));
    source.push(end_span("1", vec![end_span("2", vec![])]));
    let blocks = round_trip(vec![Block::Para(source)], &options);
    assert_eq!(
        ends(&blocks[0]).last(),
        Some(&comment_end("1", vec![comment_end("2", vec![])]))
    );
}

#[test]
fn styles_extension_keeps_the_table_style_id() {
    let table = format!(
        r#"<w:tbl><w:tblPr><w:tblStyle w:val="TableContemporary"/></w:tblPr><w:tblGrid><w:gridCol w:w="1000"/></w:tblGrid>{}</w:tbl>"#,
        table_row(None, &["a"])
    );
    let styles = r#"<w:style w:type="table" w:styleId="TableContemporary"><w:name w:val="Table Contemporary"/></w:style>"#;
    let bytes = docx(
        &table,
        &Parts {
            styles,
            ..Parts::default()
        },
    );
    let attr = |options: &ReaderOptions| match read_with(&bytes, options).first() {
        Some(Block::Table(table)) => table.attr.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(attr(&ReaderOptions::default()), Attr::default());
    let mut options = ReaderOptions::default();
    options.extensions.enable(Extension::Styles);
    assert_eq!(
        attr(&options),
        Attr::new(
            "",
            vec![],
            vec![("custom-style".into(), "TableContemporary".into())]
        )
    );
}

#[test]
fn ordered_list_attributes_survive_the_new_list_rules() {
    let numbering = r#"<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="3"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#;
    let item = |value: &str| {
        paragraph(
            r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>"#,
            value,
        )
    };
    let body = [
        item("one"),
        paragraph(r#"<w:ind w:left="720"/>"#, "more"),
        item("two"),
    ]
    .concat();
    assert_eq!(
        read(
            &body,
            &Parts {
                numbering,
                ..Parts::default()
            }
        ),
        [Block::OrderedList(
            ListAttributes {
                start: 3,
                style: randoc_ast::ListNumberStyle::Decimal,
                delim: randoc_ast::ListNumberDelim::Period,
            },
            vec![vec![para("one"), para("more")], vec![para("two")]],
        )]
    );
}

const INDENT_STYLES: &str = r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Ind"><w:name w:val="Ind"/><w:pPr><w:ind w:left="720"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Ind1000"><w:name w:val="Ind1000"/><w:pPr><w:ind w:left="1000"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Zero"><w:name w:val="Zero"/><w:pPr><w:ind w:left="0"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Huge"><w:name w:val="Huge"/><w:pPr><w:ind w:left="9223372036854775807" w:hanging="-1"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Caption"><w:name w:val="caption"/></w:style>"#;

/// List 1 has a level indent of 720; list 2 has no level indent.
const INDENT_NUMBERING: &str = r#"<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="*"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="*"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>"#;

fn indent_parts() -> Parts<'static> {
    Parts {
        styles: INDENT_STYLES,
        numbering: INDENT_NUMBERING,
        ..Parts::default()
    }
}

fn list_item(list: u8, properties: &str, value: &str) -> String {
    paragraph(
        &format!(r#"{properties}<w:numPr><w:ilvl w:val="0"/><w:numId w:val="{list}"/></w:numPr>"#),
        value,
    )
}

fn style(id: &str) -> String {
    format!(r#"<w:pStyle w:val="{id}"/>"#)
}

fn indent(left: i64) -> String {
    format!(r#"<w:ind w:left="{left}"/>"#)
}

#[test]
fn unmatched_extreme_list_indent_does_not_overflow() {
    let huge = r#"<w:ind w:left="9223372036854775807" w:hanging="-1"/>"#;
    let body = [
        list_item(1, "", "i"),
        paragraph(huge, "big"),
        paragraph(&style("Huge"), "styled"),
    ]
    .concat();
    assert_eq!(
        read(&body, &indent_parts()),
        [
            Block::BulletList(vec![vec![para("i")]]),
            Block::BlockQuote(vec![para("big")]),
            para("styled"),
        ]
    );
}

#[test]
fn continuation_matches_the_resolved_indent_of_the_open_item() {
    // The properties of the item, of the follow-up paragraph, and whether
    // the follow-up joins the item.
    let cases: [(u8, String, String, bool); 11] = [
        (1, String::new(), style("Ind"), true),
        (1, indent(0), indent(0), true),
        (1, indent(0), style("Zero"), true),
        (1, indent(0), style("Ind"), false),
        (
            1,
            String::new(),
            format!("{}{}", style("Ind"), indent(0)),
            false,
        ),
        (2, String::new(), String::new(), false),
        (2, String::new(), indent(0), false),
        (2, String::new(), style("Zero"), false),
        // The level indent comes before the indent of the item style.
        (1, style("Ind1000"), indent(1000), false),
        (1, style("Ind1000"), indent(720), true),
        (2, style("Ind1000"), indent(1000), true),
    ];
    for (list, item, follow_up, joins) in cases {
        let body = [list_item(list, &item, "i"), paragraph(&follow_up, "c")].concat();
        let blocks = read(&body, &indent_parts());
        let expected = if joins {
            vec![Block::BulletList(vec![vec![para("i"), para("c")]])]
        } else {
            vec![Block::BulletList(vec![vec![para("i")]])]
        };
        assert_eq!(blocks[..1], expected[..], "{item} / {follow_up}");
        assert_eq!(
            blocks.len(),
            if joins { 1 } else { 2 },
            "{item} / {follow_up}"
        );
    }
}

#[test]
fn ruled_paragraphs_keep_their_rule_on_list_drop_cap_and_caption_paths() {
    let drop_cap = r#"<w:framePr w:dropCap="drop"/>"#;
    let table = format!(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="1000"/></w:tblGrid>{}</w:tbl>"#,
        table_row(None, &["a"])
    );
    let caption = |properties: &str, content: String| {
        format!(
            r#"<w:p><w:pPr>{}{properties}</w:pPr>{content}</w:p>"#,
            style("Caption")
        )
    };
    let label = || format!("{}{}", run("Table "), simple_field(" SEQ Table ", "1"));
    let read_caption = |body: String| match read(&body, &indent_parts()).as_slice() {
        [Block::Table(table)] => table.caption.long.clone(),
        other => panic!("{other:?}"),
    };

    // An empty ruled paragraph between items ends the list.
    let body = [
        list_item(1, "", "a"),
        format!("<w:p><w:pPr>{BORDER}</w:pPr></w:p>"),
        list_item(1, "", "b"),
    ]
    .concat();
    assert_eq!(
        read(&body, &indent_parts()),
        [
            Block::BulletList(vec![vec![para("a")]]),
            Block::HorizontalRule,
            Block::BulletList(vec![vec![para("b")]]),
        ]
    );

    // The rule of the target follows the merged paragraph; the rule of the
    // drop cap comes before it.
    let merged = |cap: &str, target: &str| {
        read(
            &[paragraph(cap, "B"), paragraph(target, "ody")].concat(),
            &indent_parts(),
        )
    };
    assert_eq!(
        merged(drop_cap, BORDER),
        [para("Body"), Block::HorizontalRule]
    );
    assert_eq!(
        merged(&format!("{drop_cap}{BORDER}"), ""),
        [Block::HorizontalRule, para("Body")]
    );
    assert_eq!(
        merged(&format!("{drop_cap}{BORDER}"), BORDER),
        [Block::HorizontalRule, para("Body"), Block::HorizontalRule]
    );
    assert_eq!(
        read(
            &[
                paragraph(&format!("{drop_cap}{BORDER}"), "B"),
                list_item(1, "", "ody")
            ]
            .concat(),
            &indent_parts()
        ),
        [
            Block::HorizontalRule,
            Block::BulletList(vec![vec![para("Body")]])
        ]
    );

    // A caption keeps its rule, before or after the table.
    let ruled = vec![para("Cap"), Block::HorizontalRule];
    assert_eq!(
        read_caption(format!("{}{table}", caption(BORDER, run("Cap")))),
        ruled
    );
    assert_eq!(
        read_caption(format!("{table}{}", caption(BORDER, run("Cap")))),
        ruled
    );
    // A caption with only a label, or with no content, is empty.
    assert_eq!(read_caption(format!("{}{table}", caption("", label()))), []);
    assert_eq!(read_caption(format!("{table}{}", caption("", label()))), []);
    assert_eq!(
        read_caption(format!("{table}{}", caption("", String::new()))),
        []
    );
    assert_eq!(
        read_caption(format!("{}{table}", caption(BORDER, label()))),
        [Block::HorizontalRule]
    );
    assert_eq!(
        read_caption(format!("{table}{}", caption(BORDER, String::new()))),
        [Block::HorizontalRule]
    );
}

fn merge_cell(value: &str, merge: &str, span: usize) -> String {
    let merge = match merge {
        "r" => r#"<w:vMerge w:val="restart"/>"#,
        "c" => "<w:vMerge/>",
        _ => "",
    };
    let span = if span > 1 {
        format!(r#"<w:gridSpan w:val="{span}"/>"#)
    } else {
        String::new()
    };
    format!(
        "<w:tc><w:tcPr>{span}{merge}</w:tcPr><w:p>{}</w:p></w:tc>",
        run(value)
    )
}

/// The text and row span of each cell of the head rows and of the body rows.
type SpanRows = Vec<Vec<(String, i64)>>;

fn row_spans(block: &Block) -> (SpanRows, SpanRows) {
    let Block::Table(table) = block else {
        panic!("{block:?}");
    };
    let rows = |rows: &[randoc_ast::Row]| {
        rows.iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| {
                        (
                            randoc_core::shared::stringify_blocks(&cell.content),
                            cell.row_span,
                        )
                    })
                    .collect()
            })
            .collect()
    };
    let body: Vec<_> = table
        .bodies
        .iter()
        .flat_map(|body| body.body.clone())
        .collect();
    (rows(&table.head.rows), rows(&body))
}

#[test]
fn vertical_merges_survive_the_header_row_order() {
    let grid = r#"<w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>"#;
    // Each row: header flag, then (text, merge, span) of each cell.
    type Spec<'a> = &'a [(bool, &'a [(&'a str, &'a str, usize)])];
    let table = |rows: Spec<'_>| {
        let rows: String = rows
            .iter()
            .map(|(header, cells)| {
                let properties = if *header {
                    "<w:trPr><w:tblHeader/></w:trPr>"
                } else {
                    ""
                };
                let cells: String = cells
                    .iter()
                    .map(|(value, merge, span)| merge_cell(value, merge, *span))
                    .collect();
                format!("<w:tr>{properties}{cells}</w:tr>")
            })
            .collect();
        row_spans(&read(&format!("<w:tbl>{grid}{rows}</w:tbl>"), &Parts::default())[0])
    };
    let cells = |values: &[(&str, i64)]| -> Vec<(String, i64)> {
        values.iter().map(|(v, s)| ((*v).to_owned(), *s)).collect()
    };

    // A continuation row stays with the header row before it.
    let (head, body) = table(&[
        (true, &[("H1", "r", 1), ("h1", "", 1)]),
        (false, &[("", "c", 1), ("x", "", 1)]),
        (true, &[("H2", "", 1), ("h2", "", 1)]),
        (false, &[("B", "", 1), ("b", "", 1)]),
    ]);
    assert_eq!(
        head,
        [
            cells(&[("H1", 2), ("h1", 1)]),
            cells(&[("x", 1)]),
            cells(&[("H2", 1), ("h2", 1)])
        ]
    );
    assert_eq!(body, [cells(&[("B", 1), ("b", 1)])]);

    // A header continuation joins the header row above it after the move.
    let (head, body) = table(&[
        (true, &[("H1", "", 1), ("h1", "", 1)]),
        (false, &[("B0", "r", 1), ("b0", "", 1)]),
        (true, &[("H2", "c", 1), ("h2", "", 1)]),
        (false, &[("B", "", 1), ("b", "", 1)]),
    ]);
    assert_eq!(head, [cells(&[("H1", 2), ("h1", 1)]), cells(&[("h2", 1)])]);
    assert_eq!(
        body,
        [cells(&[("B0", 1), ("b0", 1)]), cells(&[("B", 1), ("b", 1)])]
    );

    // A body continuation after a moved header row goes with it.
    let (head, body) = table(&[
        (true, &[("H1", "r", 1), ("h1", "", 1)]),
        (false, &[("B0", "", 1), ("b0", "", 1)]),
        (true, &[("H2", "c", 1), ("h2", "", 1)]),
        (false, &[("B", "c", 1), ("b", "", 1)]),
    ]);
    assert_eq!(
        head,
        [
            cells(&[("H1", 3), ("h1", 1)]),
            cells(&[("h2", 1)]),
            cells(&[("b", 1)])
        ]
    );
    assert_eq!(body, [cells(&[("B0", 1), ("b0", 1)])]);

    // A body continuation after a body row stays in the body.
    let (head, body) = table(&[
        (true, &[("H1", "r", 1), ("h1", "", 1)]),
        (false, &[("B0", "", 1), ("b0", "", 1)]),
        (false, &[("B1", "c", 1), ("b1", "", 1)]),
        (true, &[("H2", "", 1), ("h2", "", 1)]),
    ]);
    assert_eq!(
        head,
        [
            cells(&[("H1", 1), ("h1", 1)]),
            cells(&[("H2", 1), ("h2", 1)])
        ]
    );
    assert_eq!(body, [cells(&[("B0", 2), ("b0", 1)]), cells(&[("b1", 1)])]);

    // A continuation joins the cell above it without `w:vMerge`.
    let (_, body) = table(&[
        (false, &[("B0", "r", 1), ("b0", "", 1)]),
        (false, &[("B1", "", 1), ("b1", "", 1)]),
        (false, &[("", "c", 1), ("x", "", 1)]),
    ]);
    assert_eq!(
        body,
        [
            cells(&[("B0", 1), ("b0", 1)]),
            cells(&[("B1", 2), ("b1", 1)]),
            cells(&[("x", 1)])
        ]
    );

    // A cell that spans the column from the left ends the merge.
    let (_, body) = table(&[
        (false, &[("A", "", 1), ("B", "", 1)]),
        (false, &[("C", "", 2)]),
        (false, &[("D", "", 1), ("E", "c", 1)]),
    ]);
    assert_eq!(body[0], cells(&[("A", 1), ("B", 1)]));
    assert_eq!(body[1], cells(&[("C", 1)]));
    assert_eq!(body[2][0], ("D".to_owned(), 1));
}

#[test]
fn discarded_cells_and_columns_do_not_change_the_kept_cells() {
    let table = |widths: &[&str], rows: &[&[&str]]| {
        let grid: String = widths
            .iter()
            .map(|w| format!(r#"<w:gridCol w:w="{w}"/>"#))
            .collect();
        let rows: String = rows
            .iter()
            .map(|row| {
                let cells: String = row
                    .iter()
                    .map(|value| {
                        // `a|b` is a cell with two paragraphs.
                        let paragraphs: String = value
                            .split('|')
                            .map(|part| format!("<w:p>{}</w:p>", run(part)))
                            .collect();
                        format!("<w:tc>{paragraphs}</w:tc>")
                    })
                    .collect();
                format!("<w:tr>{cells}</w:tr>")
            })
            .collect();
        let blocks = read(
            &format!("<w:tbl><w:tblGrid>{grid}</w:tblGrid>{rows}</w:tbl>"),
            &Parts::default(),
        );
        let [Block::Table(table)] = blocks.as_slice() else {
            panic!("{blocks:?}");
        };
        table.as_ref().clone()
    };
    let widths = |table: &Table| -> Vec<Option<f64>> {
        table
            .col_specs
            .iter()
            .map(|spec| match spec.width {
                randoc_ast::ColWidth::ColWidth(w) => Some(w),
                randoc_ast::ColWidth::ColWidthDefault => None,
            })
            .collect()
    };

    // A discarded two-paragraph cell does not stop `Plain` cells.
    let kept = table(&["2000"], &[&["a", "b|c"], &["d", "e"]]);
    assert_eq!(
        kept.bodies[0].body[0].cells[0].content,
        [Block::Plain(text("a"))]
    );
    assert_eq!(widths(&kept), [Some(2000.0 / 9360.0)]);

    // The formula counts the whole grid; a zero width is the default.
    let two = 2000.0 / 9350.0;
    assert_eq!(widths(&table(&["2000", "0"], &[&["a"]])), [Some(two)]);
    assert_eq!(
        widths(&table(&["0", "2000"], &[&["a", "b"]])),
        [None, Some(two)]
    );
    assert_eq!(
        widths(&table(&["2000", "0"], &[&["a", "b"]])),
        [Some(two), None]
    );
    assert_eq!(widths(&table(&["8000", "8000"], &[&["a"]])), [Some(0.5)]);
    assert_eq!(
        widths(&table(&["2000", "-500"], &[&["a", "b"]])),
        [Some(two), Some(-500.0 / 9350.0)]
    );
    assert_eq!(
        widths(&table(&["2000", " 300"], &[&["a", "b"]])),
        [Some(two), Some(300.0 / 9350.0)]
    );
    // A width that is not an integer makes all the widths default.
    for bad in ["2000.5", "+300", "1e3", "abc", ""] {
        assert_eq!(
            widths(&table(&["2000", bad], &[&["a", "b"]])),
            [None, None],
            "{bad}"
        );
    }
}

#[test]
fn first_alternate_content_without_fallback_keeps_the_run() {
    let body = r#"<w:p><w:r><w:t>P</w:t><mc:AlternateContent><mc:Choice Requires="w16se"><w:t>C</w:t></mc:Choice></mc:AlternateContent><mc:AlternateContent><mc:Choice Requires="w16se"><w:t>D</w:t></mc:Choice><mc:Fallback><w:t>Q</w:t></mc:Fallback></mc:AlternateContent><w:t>R</w:t></w:r></w:p>"#;
    assert_eq!(read(body, &Parts::default()), [para("PR")]);

    let body = r#"<w:p><w:r><w:t>P</w:t><mc:AlternateContent><mc:Choice Requires="v"><w:t>C</w:t></mc:Choice></mc:AlternateContent><mc:AlternateContent><mc:Fallback><w:pict><v:shape><v:textbox><w:txbxContent><w:p><w:r><w:t>Box</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict></mc:Fallback></mc:AlternateContent><w:t>R</w:t></w:r></w:p>"#;
    assert_eq!(read(body, &Parts::default()), [para("PR")]);
}

#[test]
fn comment_end_nests_in_the_anchor_before_it_and_in_the_title() {
    let comments: String = (1..=2)
        .map(|id| {
            format!(
                r#"<w:comment w:id="{id}" w:author="A">{}</w:comment>"#,
                paragraph("", "c")
            )
        })
        .collect();
    let start = |id: u8| format!(r#"<w:commentRangeStart w:id="{id}"/>"#);
    let end = |id: u8| {
        format!(r#"<w:commentRangeEnd w:id="{id}"/><w:r><w:commentReference w:id="{id}"/></w:r>"#)
    };
    let bookmark = r#"<w:bookmarkStart w:id="10" w:name="bA"/><w:bookmarkEnd w:id="10"/>"#;
    let link = format!(
        r#"<w:p><w:hyperlink w:anchor="bA">{}</w:hyperlink></w:p>"#,
        run("L")
    );
    let styles = r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/></w:style>"#;
    let parts = Parts {
        styles,
        comments: &comments,
        ..Parts::default()
    };
    let mut options = ReaderOptions::default();
    options.track_changes = TrackChanges::AllChanges;
    let read_doc = |body: String| {
        crate::convert::read_docx(&docx(&body, &parts), &options, &mut Context::pure()).unwrap()
    };
    let without_starts = |inlines: &[Inline]| -> Vec<Inline> {
        inlines
            .iter()
            .filter(|inline| {
                !matches!(inline, Inline::Span(attr, _) if attr.classes.as_slice() == ["comment-start"])
            })
            .cloned()
            .collect()
    };
    let anchor = |inner| Inline::Span(Attr::new("bA", vec!["anchor".into()], vec![]), inner);

    let doc = read_doc(format!(
        "<w:p>{}{}{bookmark}{}</w:p>{link}",
        start(1),
        run("A"),
        end(1)
    ));
    let Block::Para(inlines) = &doc.blocks[0] else {
        panic!("{:?}", doc.blocks);
    };
    assert_eq!(
        without_starts(inlines),
        [
            Inline::Str("A".into()),
            anchor(vec![comment_end("1", vec![])])
        ]
    );

    // Other content between the anchor and the end: no nesting.
    let doc = read_doc(format!(
        "<w:p>{}{}{bookmark}{}{}</w:p>{link}",
        start(1),
        run("A"),
        run("y"),
        end(1)
    ));
    let Block::Para(inlines) = &doc.blocks[0] else {
        panic!("{:?}", doc.blocks);
    };
    assert_eq!(
        without_starts(inlines),
        [
            Inline::Str("A".into()),
            anchor(vec![]),
            Inline::Str("y".into()),
            comment_end("1", vec![])
        ]
    );

    let doc = read_doc(format!(
        r#"<w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr>{}{}{}{}{}</w:p>{}"#,
        start(1),
        start(2),
        run("T"),
        end(1),
        end(2),
        paragraph("", "x")
    ));
    let Some(randoc_ast::MetaValue::MetaInlines(title)) = doc.meta.get("title") else {
        panic!("{:?}", doc.meta);
    };
    assert_eq!(
        without_starts(title),
        [
            Inline::Str("T".into()),
            comment_end("1", vec![comment_end("2", vec![])])
        ]
    );
}
