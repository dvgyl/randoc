//! Emacs Org-mode (`org`) reader and writer for Randoc.

use randoc_core::{Reader, Writer};

/// Returns the reader for `org`.
pub fn reader(_format: &str) -> Option<Reader> {
    None
}

/// Returns the writer for `org`.
pub fn writer(_format: &str) -> Option<Writer> {
    None
}
