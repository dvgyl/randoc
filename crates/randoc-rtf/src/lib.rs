//! Rich Text Format (`rtf`) reader and writer for Randoc.

use randoc_core::{Reader, Writer};

/// Returns the reader for `rtf`.
pub fn reader(_format: &str) -> Option<Reader> {
    None
}

/// Returns the writer for `rtf`.
pub fn writer(_format: &str) -> Option<Writer> {
    None
}
