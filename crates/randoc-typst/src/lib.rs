//! Reads and writes Typst documents.

#![forbid(unsafe_code)]

mod reader;
mod writer;

use std::sync::Arc;

use randoc_core::{Reader, TemplateWriter, Writer};

/// Returns the reader for Typst.
pub fn reader(format: &str) -> Option<Reader> {
    (format == "typst").then(|| Reader::Text(Arc::new(reader::read)))
}

/// Returns the writer for Typst.
pub fn writer(format: &str) -> Option<Writer> {
    (format == "typst").then(|| Writer::Text(Arc::new(TemplateWriter(writer::write))))
}

#[cfg(test)]
mod tests;
