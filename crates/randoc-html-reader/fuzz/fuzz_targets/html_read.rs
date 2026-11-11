#![no_main]
//! Reads arbitrary text as HTML. The first byte selects the extensions. The
//! reader must not panic, and the tree must not hold a note placeholder.

use libfuzzer_sys::fuzz_target;
use randoc_ast::Inline;
use randoc_ast::walk::query_inlines;
use randoc_core::{Context, FormatSpec, Reader, ReaderOptions, Sources};

const FORMATS: &[&str] = &[
    "html",
    "html+raw_html",
    "html-native_divs-native_spans",
    "html-native_divs-native_spans+raw_html",
    "html+smart+tex_math_dollars+tex_math_single_backslash+tex_math_double_backslash+raw_tex",
    "html+epub_html_exts+empty_paragraphs+task_lists-auto_identifiers-line_blocks",
    "html+ascii_identifiers+gfm_auto_identifiers+east_asian_line_breaks+literate_haskell",
];

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else { return };
    let Ok(text) = std::str::from_utf8(rest) else { return };
    let Some(Reader::Text(reader)) = randoc_html_reader::reader("html") else {
        panic!("the html format is not registered")
    };
    let format = FORMATS[usize::from(selector) % FORMATS.len()];
    let mut options = ReaderOptions::default();
    options.extensions = FormatSpec::parse(format).extensions().expect("a format of the list");
    let mut context = Context::pure();
    let Ok(doc) = reader.read(&Sources::from_text(text), &options, &mut context) else { return };
    query_inlines(&doc.blocks, |inline| {
        if let Inline::RawInline(format, _) = inline {
            assert!(matches!(format.as_str(), "html" | "tex"), "a placeholder is in the tree");
        }
    });
});
