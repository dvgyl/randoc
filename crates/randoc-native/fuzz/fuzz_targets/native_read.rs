#![no_main]
//! Reads arbitrary text as native input. A document that reads must write
//! and read again as the same document.

use libfuzzer_sys::fuzz_target;
use randoc_ast::json;
use randoc_core::{Context, Reader, ReaderOptions, Sources, Writer, WriterOptions};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let (Some(Reader::Text(reader)), Some(Writer::Text(writer))) =
        (randoc_native::reader("native"), randoc_native::writer("native"))
    else {
        panic!("the native format is not registered")
    };
    let mut context = Context::pure();
    let options = ReaderOptions::default();
    let Ok(doc) = reader.read(&Sources::from_text(text), &options, &mut context) else { return };
    let mut writer_options = WriterOptions::default();
    writer_options.template = Some(Default::default());
    let written = writer.write(&doc, &writer_options, &mut context).expect("the writer failed");
    let again = reader
        .read(&Sources::from_text(written.as_str()), &options, &mut context)
        .unwrap_or_else(|error| panic!("the writer output does not read: {error}\n{written}"));
    // NaN is not equal to itself, so compare the JSON text.
    assert_eq!(json::to_json(&again), json::to_json(&doc));
});
