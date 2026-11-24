//! Tests for page labels in BibLaTeX postnotes.

#![allow(clippy::unwrap_used)]

use randoc_ast::{Block, Citation, CitationMode, Inline, MetaValue, Pandoc};
use randoc_core::{CiteMethod, Context, LogMessage, WrapOption, WriterOptions, default_extensions};

#[test]
fn page_labels_follow_the_document_language() {
    // These inlines match the Markdown reader's citation suffixes.
    let suffixes = [
        "p.\u{a0}33",
        "pp.\u{a0}33",
        "page 33",
        "S. 33",
        "Seite 33",
        "s. 33",
        "P. 33",
        "PAGE 33",
        "p.33",
        "pp 33",
    ];
    let mut options = WriterOptions::default();
    options.cite_method = CiteMethod::Biblatex;
    options.wrap_text = WrapOption::WrapNone;
    options.extensions = default_extensions("latex");
    let mut count = 0;
    for row in include_str!("replay/biblatex-page-labels.txt")
        .lines()
        .filter(|row| !row.starts_with('#'))
    {
        let (lang, cells) = row.split_once(' ').unwrap();
        assert_eq!(cells.len(), suffixes.len());
        let mut doc = Pandoc::default();
        if lang != "-" {
            doc.meta.insert("lang", MetaValue::MetaString(lang.into()));
        }
        for (suffix, cell) in suffixes.iter().zip(cells.bytes()) {
            let mut inlines = vec![Inline::Str(",".into())];
            for part in suffix.split(' ') {
                inlines.extend([Inline::Space, Inline::Str(part.into())]);
            }
            doc.blocks = vec![Block::Para(vec![Inline::Cite(
                vec![Citation {
                    id: "r".into(),
                    prefix: vec![],
                    suffix: inlines,
                    mode: CitationMode::NormalCitation,
                    note_num: 1,
                    hash: 0,
                }],
                vec![],
            )])];
            let note = if cell == b'1' {
                "33".into()
            } else {
                suffix.replace('\u{a0}', "~")
            };
            let mut context = Context::pure();
            assert_eq!(
                randoc_latex::write_latex(&doc, &options, &mut context).unwrap(),
                format!(r"\autocite[{note}]{{r}}"),
                "lang={lang}, suffix={suffix}"
            );
            if lang == "x-foo" {
                assert_eq!(context.log(), &[LogMessage::InvalidLang(lang.into())]);
            } else {
                assert!(context.log().is_empty());
            }
            count += 1;
        }
    }
    assert_eq!(count, 300);
}
