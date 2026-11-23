//! Pandoc 3.13 compatible DOCX reader built on `randoc-archive`.

#![forbid(unsafe_code)]

mod convert;
mod ns;
mod numbering;
mod omml;
mod styles;
mod symbol;

/// DOCX writer implementation.
pub mod writer;

use std::sync::Arc;

use randoc_ast::Pandoc;
use randoc_core::{BinaryReader, Context, Reader, ReaderOptions, Result, Writer};

/// Returns the DOCX reader when `format == "docx"`.
pub fn reader(format: &str) -> Option<Reader> {
    match format {
        "docx" => Some(Reader::Binary(Arc::new(DocxReader))),
        _ => None,
    }
}

/// Returns the DOCX writer when the format is supported.
pub fn writer(_format: &str) -> Option<Writer> {
    None
}

struct DocxReader;

impl BinaryReader for DocxReader {
    fn read(&self, bytes: &[u8], options: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
        convert::read_docx(bytes, options, context)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use randoc_archive::zip::ArchiveWriter;
    use randoc_ast::{Block, Inline, MathType};
    use randoc_core::{Context, Error, Extension, Limits, ReaderOptions, TrackChanges};

    use super::*;

    fn build_docx(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = ArchiveWriter::new();
        for &(name, data) in entries {
            w.write_deflated(name, data).expect("write entry");
        }
        w.finish().expect("finish zip")
    }

    const ROOT_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

    #[test]
    fn container_error_messages_match_pandoc_3_13() {
        let mut ctx = Context::pure();
        let opts = ReaderOptions::default();
        for len in 0..4 {
            let err = convert::read_docx(&vec![b'x'; len], &opts, &mut ctx).unwrap_err();
            assert_eq!(err.exit_code(), 63);
            assert_eq!(
                err.to_string(),
                "couldn't unpack docx container: not enough bytes"
            );
        }
        for len in [4, 5, 22] {
            let err = convert::read_docx(&vec![b'x'; len], &opts, &mut ctx).unwrap_err();
            assert_eq!(err.exit_code(), 63);
            assert_eq!(
                err.to_string(),
                "couldn't unpack docx container: Did not find end of central directory signature"
            );
        }
        let empty_zip = build_docx(&[]);
        let err = convert::read_docx(&empty_zip, &opts, &mut ctx).unwrap_err();
        assert_eq!(err.exit_code(), 63);
        assert_eq!(err.to_string(), "couldn't parse docx file: DocxError");
    }

    #[test]
    fn p7_dynamic_w_namespace_rule() {
        let mut ctx = Context::pure();
        let opts = ReaderOptions::default();

        // Custom URI bound to `w:` works, including inner `y:` bound to the same URI.
        let ok_doc = build_docx(&[
            ("_rels/.rels", ROOT_RELS),
            (
                "word/document.xml",
                br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="urn:custom-word">
  <w:body><y:p xmlns:y="urn:custom-word"><y:r><y:t>ok</y:t></y:r></y:p></w:body>
</w:document>"#,
            ),
        ]);
        let doc = convert::read_docx(&ok_doc, &opts, &mut ctx).expect("p7 ok");
        assert_eq!(
            doc.blocks,
            vec![Block::Para(vec![Inline::Str("ok".into())])]
        );

        // Root element `x:document` without `xmlns:w` fails with DocxError.
        let bad_doc = build_docx(&[
            ("_rels/.rels", ROOT_RELS),
            (
                "word/document.xml",
                br#"<?xml version="1.0" encoding="UTF-8"?>
<x:document xmlns:x="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <x:body><x:p><x:r><x:t>bad</x:t></x:r></x:p></x:body>
</x:document>"#,
            ),
        ]);
        assert!(matches!(
            convert::read_docx(&bad_doc, &opts, &mut ctx),
            Err(Error::Some(msg)) if msg == "couldn't parse docx file: DocxError"
        ));
    }

    #[test]
    fn p8_self_referencing_footnote_terminates_via_cycle_guard() {
        let doc_rels = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/>
</Relationships>"#;
        let footnotes = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:footnote w:id="1"><w:p><w:r><w:t>loop</w:t><w:footnoteReference w:id="1"/></w:r></w:p></w:footnote>
</w:footnotes>"#;
        let document = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body><w:p><w:r><w:t>hi</w:t><w:footnoteReference w:id="1"/></w:r></w:p></w:body>
</w:document>"#;
        let bytes = build_docx(&[
            ("_rels/.rels", ROOT_RELS),
            ("word/_rels/document.xml.rels", doc_rels),
            ("word/footnotes.xml", footnotes),
            ("word/document.xml", document),
        ]);
        let mut ctx = Context::pure();
        let doc = convert::read_docx(&bytes, &ReaderOptions::default(), &mut ctx)
            .expect("terminates cleanly");
        assert_eq!(
            doc.blocks,
            vec![Block::Para(vec![
                Inline::Str("hi".into()),
                Inline::Note(vec![Block::Para(vec![
                    Inline::Str("loop".into()),
                    Inline::Note(vec![]),
                ])]),
            ])]
        );
    }

    #[test]
    fn enforces_table_cells_and_depth_limits_on_small_stack() {
        // Huge gridSpan DoS must fail cleanly in O(1) without OOM.
        let huge_span = build_docx(&[
            ("_rels/.rels", ROOT_RELS),
            (
                "word/document.xml",
                br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body><w:tbl><w:tr><w:tc><w:tcPr><w:gridSpan w:val="100000000"/></w:tcPr><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body>
</w:document>"#,
            ),
        ]);
        let mut ctx = Context::pure();
        assert!(matches!(
            convert::read_docx(&huge_span, &ReaderOptions::default(), &mut ctx),
            Err(Error::Parse(_))
        ));

        // Deep XML nesting fails cleanly at max_nesting_depth on a 2 MB stack.
        let handle = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let mut xml = String::from(
                    r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
                );
                for _ in 0..5_000 {
                    xml.push_str("<w:sdt><w:sdtContent>");
                }
                xml.push_str("<w:p><w:r><w:t>deep</w:t></w:r></w:p>");
                for _ in 0..5_000 {
                    xml.push_str("</w:sdtContent></w:sdt>");
                }
                xml.push_str("</w:body></w:document>");
                let bytes = build_docx(&[
                    ("_rels/.rels", ROOT_RELS),
                    ("word/document.xml", xml.as_bytes()),
                ]);
                let mut ctx = Context::pure();
                ctx.limits = Limits::restricted();
                assert!(matches!(
                    convert::read_docx(&bytes, &ReaderOptions::default(), &mut ctx),
                    Err(Error::Parse(_))
                ));
            })
            .expect("spawn");
        handle.join().expect("no stack overflow");
    }

    #[test]
    fn non_utf8_document_xml_returns_fatal_exit_1() {
        let bytes = build_docx(&[
            ("_rels/.rels", ROOT_RELS),
            ("word/document.xml", b"<w:document>\xe9</w:document>"),
        ]);
        let mut ctx = Context::pure();
        let err = convert::read_docx(&bytes, &ReaderOptions::default(), &mut ctx).unwrap_err();
        assert_eq!(err.exit_code(), 1);
        assert!(err.to_string().contains("Cannot decode byte '\\xe9'"));
    }

    #[test]
    fn omml_math_and_tracked_changes_and_styles() {
        let document = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
  <w:body>
    <w:p><m:oMathPara><m:oMath><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath></m:oMathPara></w:p>
    <w:p><w:ins w:author="Alice"><w:r><w:t>added</w:t></w:r></w:ins><w:del w:author="Bob"><w:r><w:delText>removed</w:delText></w:r></w:del></w:p>
  </w:body>
</w:document>"#;
        let bytes = build_docx(&[("_rels/.rels", ROOT_RELS), ("word/document.xml", document)]);
        let mut ctx = Context::pure();
        let mut opts = ReaderOptions::default();
        opts.extensions.enable(Extension::AutoIdentifiers);
        opts.track_changes = TrackChanges::AcceptChanges;
        let doc = convert::read_docx(&bytes, &opts, &mut ctx).expect("read");
        assert_eq!(
            doc.blocks,
            vec![
                Block::Para(vec![Inline::Math(
                    MathType::DisplayMath,
                    "\\frac{a}{b}".into()
                )]),
                Block::Para(vec![Inline::Str("added".into())]),
            ]
        );
    }

    #[test]
    fn enforces_sparse_table_grid_map_bound() {
        // 500 gridCols * 100 empty rows = 50,000 grid_map slots > restricted max_table_cells (20,000).
        let mut xml = String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tblGrid>"#,
        );
        for _ in 0..500 {
            xml.push_str(r#"<w:gridCol w:w="100"/>"#);
        }
        xml.push_str("</w:tblGrid>");
        for _ in 0..100 {
            xml.push_str("<w:tr/>");
        }
        xml.push_str("</w:tbl></w:body></w:document>");
        let bytes = build_docx(&[
            ("_rels/.rels", ROOT_RELS),
            ("word/document.xml", xml.as_bytes()),
        ]);
        let mut ctx = Context::pure();
        ctx.limits = Limits::restricted();
        assert!(matches!(
            convert::read_docx(&bytes, &ReaderOptions::default(), &mut ctx),
            Err(Error::Parse(_))
        ));
    }

    #[test]
    fn post_xml_list_and_field_nesting_safe_on_small_stack() {
        // 1. Flat paragraphs with increasing ilvl 0..500 must fail cleanly at max_nesting_depth on 2 MiB stack.
        let handle = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let doc_rels = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>
</Relationships>"#;
                let mut numbering = String::from(
                    r#"<?xml version="1.0" encoding="UTF-8"?><w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="0">"#,
                );
                for lvl in 0..500 {
                    numbering.push_str(&format!(
                        r#"<w:lvl w:ilvl="{lvl}"><w:numFmt w:val="bullet"/><w:lvlText w:val="*"/></w:lvl>"#
                    ));
                }
                numbering.push_str(
                    r#"</w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#,
                );
                let mut doc = String::from(
                    r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
                );
                for lvl in 0..500 {
                    doc.push_str(&format!(
                        r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{lvl}"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>x</w:t></w:r></w:p>"#
                    ));
                }
                doc.push_str("</w:body></w:document>");
                let bytes = build_docx(&[
                    ("_rels/.rels", ROOT_RELS),
                    ("word/_rels/document.xml.rels", doc_rels),
                    ("word/numbering.xml", numbering.as_bytes()),
                    ("word/document.xml", doc.as_bytes()),
                ]);
                let mut ctx = Context::pure();
                ctx.limits = Limits::restricted();
                assert!(matches!(
                    convert::read_docx(&bytes, &ReaderOptions::default(), &mut ctx),
                    Err(Error::Parse(_))
                ));
            })
            .expect("spawn list test");
        handle.join().expect("no stack overflow on deep list");

        // 2. Flat runs with 500 nested complex HYPERLINK fields must fail cleanly on 2 MiB stack.
        let handle = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let mut doc = String::from(
                    r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p>"#,
                );
                for _ in 0..500 {
                    doc.push_str(
                        r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>HYPERLINK "https://example.com"</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
                    );
                }
                doc.push_str("<w:r><w:t>deep</w:t></w:r>");
                for _ in 0..500 {
                    doc.push_str(r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#);
                }
                doc.push_str("</w:p></w:body></w:document>");
                let bytes = build_docx(&[("_rels/.rels", ROOT_RELS), ("word/document.xml", doc.as_bytes())]);
                let mut ctx = Context::pure();
                ctx.limits = Limits::restricted();
                assert!(matches!(
                    convert::read_docx(&bytes, &ReaderOptions::default(), &mut ctx),
                    Err(Error::Parse(_))
                ));
            })
            .expect("spawn field test");
        handle.join().expect("no stack overflow on deep fields");
    }

    #[test]
    fn comment_cycle_guard_and_footnote_dag_budget() {
        // 1. Self-referencing comment terminates cleanly via cycle guard.
        let doc_rels = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/>
</Relationships>"#;
        let comments = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:comment w:id="1" w:author="A"><w:p><w:commentRangeStart w:id="1"/><w:r><w:t>loop</w:t></w:r></w:p></w:comment>
</w:comments>"#;
        let document = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body><w:p><w:commentRangeStart w:id="1"/><w:r><w:t>hi</w:t></w:r><w:commentRangeEnd w:id="1"/></w:p></w:body>
</w:document>"#;
        let bytes = build_docx(&[
            ("_rels/.rels", ROOT_RELS),
            ("word/_rels/document.xml.rels", doc_rels),
            ("word/comments.xml", comments),
            ("word/document.xml", document),
        ]);
        let mut ctx = Context::pure();
        let mut opts = ReaderOptions::default();
        opts.track_changes = TrackChanges::AllChanges;
        assert!(convert::read_docx(&bytes, &opts, &mut ctx).is_ok());

        // 2. Exponential footnote DAG (depth 20, fanout 2 -> 2^20 nodes) hits expansion budget.
        let mut fns = String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?><w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        );
        for i in 1..20 {
            let next = i + 1;
            fns.push_str(&format!(
                r#"<w:footnote w:id="{i}"><w:p><w:r><w:footnoteReference w:id="{next}"/><w:footnoteReference w:id="{next}"/></w:r></w:p></w:footnote>"#
            ));
        }
        fns.push_str(r#"<w:footnote w:id="20"><w:p><w:r><w:t>leaf</w:t></w:r></w:p></w:footnote></w:footnotes>"#);
        let dag_doc = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body><w:p><w:r><w:footnoteReference w:id="1"/></w:r></w:p></w:body>
</w:document>"#;
        let dag_bytes = build_docx(&[
            ("_rels/.rels", ROOT_RELS),
            ("word/_rels/document.xml.rels", doc_rels),
            ("word/footnotes.xml", fns.as_bytes()),
            ("word/document.xml", dag_doc),
        ]);
        let mut ctx = Context::pure();
        ctx.limits = Limits::restricted();
        assert!(matches!(
            convert::read_docx(&dag_bytes, &ReaderOptions::default(), &mut ctx),
            Err(Error::Parse(_))
        ));
    }

    #[test]
    fn repeated_notes_charge_run_children_and_text() {
        const NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
          <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/>
        </Relationships>"#;
        let refs = r#"<w:footnoteReference w:id="1"/>"#.repeat(100);
        let document = format!(
            r#"<w:document xmlns:w="{NS}"><w:body><w:p><w:r>{refs}</w:r></w:p></w:body></w:document>"#
        );
        for (body, limit) in [
            ("<w:br/>".repeat(510), "nodes"),
            (format!("<w:t>{}</w:t>", "a".repeat(512)), "text bytes"),
        ] {
            let notes = format!(
                r#"<w:footnotes xmlns:w="{NS}"><w:footnote w:id="1"><w:p><w:r>{body}</w:r></w:p></w:footnote></w:footnotes>"#
            );
            let bytes = build_docx(&[
                ("_rels/.rels", ROOT_RELS),
                ("word/_rels/document.xml.rels", rels),
                ("word/document.xml", document.as_bytes()),
                ("word/footnotes.xml", notes.as_bytes()),
            ]);
            let mut ctx = Context::pure();
            ctx.limits.max_table_cells = 1_000;
            ctx.limits.max_archive_size = 12_000;
            let result = convert::read_docx(&bytes, &ReaderOptions::default(), &mut ctx);
            assert!(matches!(result, Err(Error::Parse(message))
                if message.contains("DOCX note/comment expansion") && message.ends_with(limit)));
        }
    }

    #[test]
    fn zotero_locator_suffix_preserves_trailing_space() {
        let document = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p>
      <w:r><w:fldChar w:fldCharType="begin"/></w:r>
      <w:r><w:instrText> ADDIN ZOTERO_ITEM CSL_CITATION {"citationItems":[{"id":"s1","prefix":"see ","locator":"42","label":"page"},{"id":"s2","locator":"7","label":"chapter","suffix":", and more"}]} </w:instrText></w:r>
      <w:r><w:fldChar w:fldCharType="separate"/></w:r>
      <w:r><w:t>(Smith, 2020)</w:t></w:r>
      <w:r><w:fldChar w:fldCharType="end"/></w:r>
    </w:p>
  </w:body>
</w:document>"#;
        let bytes = build_docx(&[("_rels/.rels", ROOT_RELS), ("word/document.xml", document)]);
        let mut ctx = Context::pure();
        let mut opts = ReaderOptions::default();
        opts.extensions.enable(Extension::Citations);
        let doc = convert::read_docx(&bytes, &opts, &mut ctx).expect("read");
        let Block::Para(inlines) = &doc.blocks[0] else {
            panic!("expected para");
        };
        let Inline::Cite(cites, _) = &inlines[0] else {
            panic!("expected cite");
        };
        assert_eq!(
            cites[0].prefix,
            vec![Inline::Str("see".into()), Inline::Space]
        );
        assert_eq!(
            cites[0].suffix,
            vec![
                Inline::Str(",".into()),
                Inline::Space,
                Inline::Str("page".into()),
                Inline::Space,
                Inline::Str("42".into()),
                Inline::Space,
            ]
        );
        assert_eq!(
            cites[1].suffix,
            vec![
                Inline::Str(",".into()),
                Inline::Space,
                Inline::Str("chapter".into()),
                Inline::Space,
                Inline::Str("7".into()),
                Inline::Space,
                Inline::Str(",".into()),
                Inline::Space,
                Inline::Str("and".into()),
                Inline::Space,
                Inline::Str("more".into()),
            ]
        );
    }
}
