#![no_main]
//! Parses arbitrary text as the markup of a CSL JSON string and renders it.
//! Text with no special character is one text part.

use libfuzzer_sys::fuzz_target;
use randoc_citeproc::markup::{self, Markup};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let Ok(parts) = markup::parse(text, 128) else { return };
    let rendered = markup::render(&parts);
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric()) {
        assert_eq!(parts, vec![Markup::Text(text.to_owned())]);
        assert_eq!(rendered, text);
    }
});
