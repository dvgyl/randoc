//! XML document format format entry points.

#![forbid(unsafe_code)]

use randoc_core::{Reader, Writer};

/// Returns a reader if the format is available.
pub fn reader(_format: &str) -> Option<Reader> {
    None
}

/// Returns a writer if the format is available.
pub fn writer(_format: &str) -> Option<Writer> {
    None
}
