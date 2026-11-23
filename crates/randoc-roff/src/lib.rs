//! Reads man and mdoc documents and writes man and ms documents.

#![forbid(unsafe_code)]

mod escape;
mod man;
mod mdoc;
mod writer;

use std::sync::Arc;

use randoc_core::{Reader, TemplateWriter, Writer};
use writer::Format;

/// Returns the reader for a supported format.
pub fn reader(format: &str) -> Option<Reader> {
    match format {
        "man" => Some(Reader::Text(Arc::new(man::read))),
        "mdoc" => Some(Reader::Text(Arc::new(mdoc::read))),
        _ => None,
    }
}

/// Returns the writer for a supported format.
pub fn writer(format: &str) -> Option<Writer> {
    let format = match format {
        "man" => Format::Man,
        "ms" => Format::Ms,
        _ => return None,
    };
    Some(Writer::Text(Arc::new(TemplateWriter(
        move |doc: &randoc_ast::Pandoc,
              options: &randoc_core::WriterOptions,
              context: &mut randoc_core::Context| {
            writer::write(format, doc, options, context)
        },
    ))))
}

#[cfg(test)]
mod tests;
