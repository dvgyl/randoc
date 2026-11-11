#![no_main]
//! Reads arbitrary text as LaTeX. The first byte selects the extensions. The
//! reader must not panic and must not overflow the stack.

use libfuzzer_sys::fuzz_target;
use randoc_core::{Context, FormatSpec, Reader, ReaderOptions, Sources};

const FORMATS: &[&str] = &[
    "latex",
    "latex+raw_tex",
    "latex-latex_macros",
    "latex-smart-auto_identifiers+raw_tex",
];

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else { return };
    let Ok(text) = std::str::from_utf8(rest) else { return };
    let Some(Reader::Text(reader)) = randoc_latex_reader::reader("latex") else {
        panic!("the latex format is not registered")
    };
    let format = FORMATS[usize::from(selector) % FORMATS.len()];
    let mut options = ReaderOptions::default();
    options.extensions = FormatSpec::parse(format).extensions().expect("a format of the list");
    let mut context = Context::pure();
    let _ = reader.read(&Sources::from_text(text), &options, &mut context);
});
