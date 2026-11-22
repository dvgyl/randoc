//! Reads and writes Emacs Org-mode documents.

#![forbid(unsafe_code)]

mod reader;
mod shared;
mod writer;

use std::sync::Arc;

use randoc_core::{Reader, TemplateWriter, Writer};

/// Returns the text reader for Org.
pub fn reader(format: &str) -> Option<Reader> {
    (format == "org").then(|| Reader::Text(Arc::new(reader::read)))
}

/// Returns the text writer for Org.
pub fn writer(format: &str) -> Option<Writer> {
    (format == "org").then(|| Writer::Text(Arc::new(TemplateWriter(writer::write))))
}

#[cfg(test)]
mod tests;
