//! The registry of readers and writers.
//!
//! The coordinator adds one line here for each format that a workstream
//! completes. A format crate exports a constructor and does not change this file.

use std::sync::Arc;

use randoc_core::{Reader, TextReader, Writer};

use crate::json_format;

/// Returns the reader for a format name without extension changes, for example `markdown`.
pub fn reader(name: &str) -> Option<Reader> {
    match name {
        "json" => Some(Reader::Text(Arc::new(json_format::read))),
        _ => None,
    }
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
    match name {
        "json" => Some(Writer::Text(Arc::new(json_format::write))),
        _ => None,
    }
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
