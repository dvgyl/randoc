//! CSV and TSV formats support for Randoc.

#![forbid(unsafe_code)]

use randoc_core::{Reader, Writer};

/// Returns the reader for a supported format.
pub fn reader(_format: &str) -> Option<Reader> {
    None
}

/// Returns the writer for a supported format.
pub fn writer(_format: &str) -> Option<Writer> {
    None
}
