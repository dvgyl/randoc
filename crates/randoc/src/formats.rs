//! The registry of readers and writers.
//!
//! The coordinator adds one line here for each format that a workstream
//! completes. A format crate exports a constructor and does not change this file.

use std::sync::Arc;

use randoc_core::{Reader, TextReader, Writer};

use crate::json_format;

type ReaderLookup = fn(&str) -> Option<Reader>;
type WriterLookup = fn(&str) -> Option<Writer>;

/// The reader lookup of each format crate. Add one line for a new crate.
#[rustfmt::skip]
const READERS: &[ReaderLookup] = &[
    json_format::reader,
    randoc_markdown::reader,
    randoc_native::reader,
    randoc_citeproc::csljson::reader,
    randoc_html_reader::reader,
    randoc_commonmark::reader,
    randoc_latex_reader::reader,
];

/// The writer lookup of each format crate. Add one line for a new crate.
#[rustfmt::skip]
const WRITERS: &[WriterLookup] = &[
    json_format::writer,
    randoc_html::writer,
    randoc_native::writer,
    randoc_citeproc::csljson::writer,
];

/// Returns the reader for a format name without extension changes, for example `markdown`.
pub fn reader(name: &str) -> Option<Reader> {
    READERS.iter().find_map(|lookup| lookup(name))
}

/// Returns the text reader for a format name, or `None` for a binary format or an unknown format.
pub fn text_reader(name: &str) -> Option<Arc<dyn TextReader>> {
    match reader(name)? {
        Reader::Text(reader) => Some(reader),
        Reader::Binary(_) => None,
    }
}

/// Returns the writer for a format name without extension changes, for example `html`.
pub fn writer(name: &str) -> Option<Writer> {
    WRITERS.iter().find_map(|lookup| lookup(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_registered_and_unknown_names_are_not() {
        assert!(matches!(reader("json"), Some(Reader::Text(_))));
        assert!(matches!(writer("json"), Some(Writer::Text(_))));
        assert!(text_reader("json").is_some());
        assert!(reader("nosuch").is_none());
        assert!(text_reader("nosuch").is_none());
        assert!(writer("nosuch").is_none());
    }
}
