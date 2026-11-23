//! Spreadsheet and presentation formats for Randoc.

#![forbid(unsafe_code)]

mod package;
mod pptx;
mod xlsx;

use randoc_core::{Reader, Writer};

/// Returns a reader for the requested format, if available.
pub fn reader(format: &str) -> Option<Reader> {
    match format {
        "xlsx" => Some(Reader::Binary(std::sync::Arc::new(xlsx::XlsxReader))),
        "pptx" => Some(Reader::Binary(std::sync::Arc::new(pptx::PptxReader))),
        _ => None,
    }
}

/// Returns a writer for the requested format, if available.
pub fn writer(format: &str) -> Option<Writer> {
    match format {
        "pptx" => Some(Writer::Binary(std::sync::Arc::new(pptx::PptxWriter))),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
