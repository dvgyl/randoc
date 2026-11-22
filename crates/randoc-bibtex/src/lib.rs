//! BibTeX, BibLaTeX and RIS format entry points.

#![forbid(unsafe_code)]

use randoc_ast::MetaValue;
use randoc_citeproc::filter::{BibFormat, unsupported_bibliography};
use randoc_core::{Context, Reader, Result, Writer};

/// Returns a reader if the format is available.
pub fn reader(_format: &str) -> Option<Reader> {
    None
}

/// Returns a writer if the format is available.
pub fn writer(_format: &str) -> Option<Writer> {
    None
}

/// Returns the unsupported bibliography error until the reader is available.
pub fn references(
    _context: &mut Context,
    _text: &str,
    file: &str,
    _format: BibFormat,
) -> Result<Vec<MetaValue>> {
    Err(unsupported_bibliography(file))
}
