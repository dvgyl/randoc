//! Reads and writes EPUB archives.

#![forbid(unsafe_code)]

mod path;
mod reader;
mod tree;
mod writer;
mod xhtml;

use std::sync::Arc;

use randoc_core::{Reader, Writer};
use randoc_html::EpubVersion;

/// Returns the EPUB reader for EPUB 2 and EPUB 3 archives.
pub fn reader(format: &str) -> Option<Reader> {
    (format == "epub").then(|| Reader::Binary(Arc::new(reader::EpubReader)))
}

/// Returns the writer for the requested EPUB version.
pub fn writer(format: &str) -> Option<Writer> {
    let version = match format {
        "epub2" => EpubVersion::Epub2,
        "epub" | "epub3" => EpubVersion::Epub3,
        _ => return None,
    };
    Some(Writer::Binary(Arc::new(writer::EpubWriter(version))))
}

#[cfg(test)]
mod tests;
