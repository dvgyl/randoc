//! AsciiDoc readers and writers.

#![forbid(unsafe_code)]

mod reader;
mod writer;

use std::sync::Arc;

use randoc_core::{Error, Reader, TemplateWriter, Writer};

/// Returns the reader for a supported format.
pub fn reader(format: &str) -> Option<Reader> {
    (format == "asciidoc").then(|| Reader::Text(Arc::new(reader::read)))
}

/// Returns the writer for a supported format.
pub fn writer(format: &str) -> Option<Writer> {
    let dialect = match format {
        "asciidoc" => writer::Dialect::Modern,
        "asciidoctor" => writer::Dialect::Asciidoctor,
        "asciidoc_legacy" => writer::Dialect::Legacy,
        _ => return None,
    };
    Some(Writer::Text(Arc::new(TemplateWriter(
        move |document: &_, options: &_, context: &mut _| {
            writer::write(document, options, context, dialect)
        },
    ))))
}

fn limit(kind: &str) -> Error {
    Error::Parse(format!("AsciiDoc {kind} limit exceeded"))
}

#[cfg(test)]
mod tests;
