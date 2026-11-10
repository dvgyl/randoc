#![no_main]
//! Reads arbitrary text as a JSON document, writes it as native text at a
//! width from the input, and reads it back. The two trees must be equal.

use libfuzzer_sys::fuzz_target;
use randoc_ast::json;
use randoc_core::{Context, Reader, ReaderOptions, Sources, Writer, WriterOptions};

/// The harness thread has a small stack, so deep input is out of scope here.
const MAX_DEPTH: usize = 64;

fuzz_target!(|data: &[u8]| {
    let Some((&width, rest)) = data.split_first() else { return };
    let Ok(text) = std::str::from_utf8(rest) else { return };
    if json::nesting_depth(text) > MAX_DEPTH {
        return;
    }
    let Ok(doc) = json::from_json(text) else { return };
    let (Some(Reader::Text(reader)), Some(Writer::Text(writer))) =
        (randoc_native::reader("native"), randoc_native::writer("native"))
    else {
        panic!("the native format is not registered")
    };
    let mut context = Context::pure();
    let mut options = WriterOptions::default();
    options.template = Some(Default::default());
    options.columns = i64::from(width);
    let written = writer.write(&doc, &options, &mut context).expect("the writer failed");
    let again = reader
        .read(&Sources::from_text(written.as_str()), &ReaderOptions::default(), &mut context)
        .unwrap_or_else(|error| panic!("the writer output does not read: {error}\n{written}"));
    assert_eq!(json::to_json(&again), json::to_json(&doc));
});
