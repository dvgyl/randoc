//! Reads arbitrary JSON as a document tree and writes it in a random format
//! with random extensions and options.
#![no_main]

use libfuzzer_sys::fuzz_target;
use randoc_ast::json;
use randoc_core::{
    ALL_EXTENSIONS, Context, ReferenceLocation, WrapOption, WriterOptions, default_extensions,
};

const FORMATS: &[&str] = &[
    "markdown",
    "markdown_strict",
    "markdown_phpextra",
    "markdown_mmd",
    "markdown_github",
    "gfm",
    "commonmark",
    "commonmark_x",
    "plain",
    "markua",
];

fuzz_target!(|data: &[u8]| {
    let [format, wrap, columns, location, flags, toggles @ .., ] = data else {
        return;
    };
    let Some(split) = toggles.iter().position(|&b| b == b'{') else {
        return;
    };
    let (toggles, body) = toggles.split_at(split);
    let Ok(text) = std::str::from_utf8(body) else {
        return;
    };
    if json::nesting_depth(text) > 200 {
        return;
    }
    let Ok(doc) = json::from_json(text) else {
        return;
    };
    let name = FORMATS[usize::from(*format) % FORMATS.len()];
    let mut options = WriterOptions::default();
    options.extensions = default_extensions(name);
    for &toggle in toggles {
        let (extension, _) = ALL_EXTENSIONS[usize::from(toggle) % ALL_EXTENSIONS.len()];
        if toggle & 0x80 == 0 {
            options.extensions.enable(extension);
        } else {
            options.extensions.disable(extension);
        }
    }
    options.wrap_text = match wrap % 3 {
        0 => WrapOption::WrapAuto,
        1 => WrapOption::WrapNone,
        _ => WrapOption::WrapPreserve,
    };
    options.columns = i64::from(*columns) - 8;
    options.reference_location = match location % 3 {
        0 => ReferenceLocation::EndOfDocument,
        1 => ReferenceLocation::EndOfSection,
        _ => ReferenceLocation::EndOfBlock,
    };
    options.reference_links = flags & 1 != 0;
    options.setext_headers = flags & 2 != 0;
    options.tab_stop = i64::from(flags >> 4);
    let mut context = Context::pure();
    if let Some(writer) = randoc_markdown_writer::writer(name) {
        let _ = writer.write(&doc, &options, &mut context);
    }
});
