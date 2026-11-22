//! Wiki format readers and writers (`mediawiki`, `dokuwiki`, `jira`, `creole`) for Randoc.

use randoc_core::{Reader, Writer};

/// Returns the reader for `mediawiki`, `dokuwiki`, `jira`, or `creole`.
pub fn reader(_format: &str) -> Option<Reader> {
    None
}

/// Returns the writer for `mediawiki`, `dokuwiki`, or `jira`.
pub fn writer(_format: &str) -> Option<Writer> {
    None
}
