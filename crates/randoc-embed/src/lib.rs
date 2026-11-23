//! The HTML resource embedding entry point.

#![forbid(unsafe_code)]

use randoc_core::{Context, Result};

/// Returns the input HTML unchanged until resource embedding is available.
pub fn embed_resources(html: &str, _context: &mut Context) -> Result<String> {
    Ok(html.to_owned())
}
