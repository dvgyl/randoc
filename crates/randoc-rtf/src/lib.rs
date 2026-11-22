//! Rich Text Format (RTF) reader and writer.

#![forbid(unsafe_code)]

use std::sync::Arc;

use randoc_core::{Reader, TemplateWriter, Writer};

mod blocks;
mod lexer;
mod reader;
mod writer;
mod writer_math;

/// Returns the reader for the RTF format.
pub fn reader(format: &str) -> Option<Reader> {
    (format == "rtf").then(|| Reader::Text(Arc::new(reader::read)))
}

/// Returns the writer for the RTF format.
pub fn writer(format: &str) -> Option<Writer> {
    (format == "rtf").then(|| Writer::Text(Arc::new(TemplateWriter(writer::write))))
}
