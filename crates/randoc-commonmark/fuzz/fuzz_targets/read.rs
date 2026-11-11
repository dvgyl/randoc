#![no_main]
//! Reads arbitrary text with each reader of the crate. The reader must not
//! panic. The result is not checked.

use libfuzzer_sys::fuzz_target;
use randoc_core::{
    Context, Extensions, Reader, ReaderOptions, Sources, default_extensions, supported_extensions,
};

const FORMATS: [&str; 3] = ["commonmark", "gfm", "commonmark_x"];

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data).into_owned();
    let sources = Sources::from_text(text);
    for format in FORMATS {
        let Some(Reader::Text(reader)) = randoc_commonmark::reader(format) else {
            panic!("no text reader for {format}");
        };
        let extension_sets: [Extensions; 2] =
            [supported_extensions(format), default_extensions(format)];
        for extensions in extension_sets {
            let mut options = ReaderOptions::default();
            options.extensions = extensions;
            let mut context = Context::pure();
            context.limits.max_nesting_depth = 128;
            let _ = reader.read(&sources, &options, &mut context);
        }
    }
});
