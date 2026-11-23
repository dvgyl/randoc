//! PDF engine support for Randoc.

#![forbid(unsafe_code)]

use randoc_core::{Reader, Writer};

/// Returns a reader for the requested format, if available.
pub fn reader(_format: &str) -> Option<Reader> {
    None
}

/// Returns a writer for the requested format, if available.
pub fn writer(_format: &str) -> Option<Writer> {
    None
}
