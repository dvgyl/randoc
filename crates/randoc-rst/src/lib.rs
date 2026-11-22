//! Reads and writes reStructuredText documents.

mod reader;
mod writer;

use std::sync::Arc;

use randoc_core::{Reader, TemplateWriter, Writer};

/// Returns the reader for `rst`.
pub fn reader(format: &str) -> Option<Reader> {
    (format == "rst").then(|| Reader::Text(Arc::new(reader::read)))
}

/// Returns the writer for `rst`.
pub fn writer(format: &str) -> Option<Writer> {
    (format == "rst").then(|| Writer::Text(Arc::new(TemplateWriter(writer::write))))
}

#[cfg(test)]
mod tests;
