//! Reader and writer for Djot.

#![forbid(unsafe_code)]

mod attrs;
mod inlines;
mod reader;
mod writer;

use std::sync::Arc;

use randoc_core::{Reader, TemplateWriter, Writer};

/// Returns the Djot reader for `djot`.
pub fn reader(format: &str) -> Option<Reader> {
    (format == "djot").then(|| Reader::Text(Arc::new(reader::read)))
}

/// Returns the Djot writer for `djot`.
pub fn writer(format: &str) -> Option<Writer> {
    (format == "djot").then(|| Writer::Text(Arc::new(TemplateWriter(writer::write))))
}

fn identifier(text: &str) -> String {
    let mut id = String::new();
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            space = !id.is_empty();
        } else if c.is_alphanumeric() || matches!(c, '-' | '_') {
            if space {
                id.push('-');
                space = false;
            }
            id.push(c);
        }
    }
    if id.is_empty() { "sec".into() } else { id }
}

fn depth_error() -> randoc_core::Error {
    randoc_core::Error::App("Djot exceeds the nesting limit".into())
}

fn size_error() -> randoc_core::Error {
    randoc_core::Error::App("Djot exceeds the conversion size limit".into())
}

#[cfg(test)]
mod tests;
