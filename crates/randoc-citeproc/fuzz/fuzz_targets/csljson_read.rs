#![no_main]
//! Reads arbitrary text as CSL JSON. Text that reads writes CSL JSON that
//! reads again. The second result can differ: the oracle also changes some
//! dates in a round trip.

use libfuzzer_sys::fuzz_target;
use randoc_core::{Context, Reader, ReaderOptions, Sources, Writer, WriterOptions};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let (Some(Reader::Text(reader)), Some(Writer::Text(writer))) = (
        randoc_citeproc::csljson::reader("csljson"),
        randoc_citeproc::csljson::writer("csljson"),
    ) else {
        return;
    };
    let mut context = Context::pure();
    let Ok(doc) = reader.read(&Sources::from_text(text), &ReaderOptions::default(), &mut context)
    else {
        return;
    };
    let written = writer
        .write(&doc, &WriterOptions::default(), &mut context)
        .expect("the writer accepts the reader output");
    let again = reader
        .read(&Sources::from_text(written.clone()), &ReaderOptions::default(), &mut context)
        .expect("the writer output reads again");
    writer
        .write(&again, &WriterOptions::default(), &mut context)
        .expect("the writer accepts the reader output");
});
