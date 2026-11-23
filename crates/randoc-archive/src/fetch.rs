//! Shared entry points for resource lookup.

use randoc_ast::Text;
use randoc_core::{Context, Error, Result};

/// Reads a resource as bytes and a MIME type.
/// This entry point returns an error until resource lookup is available.
pub fn fetch_resource(
    _context: &mut Context,
    _source: &str,
    _resource_path: &[Text],
    _headers: &[(Text, Text)],
    _max_bytes: u64,
) -> Result<(Vec<u8>, Text)> {
    Err(Error::App(
        "Resource lookup is not implemented in Randoc.".into(),
    ))
}

/// Returns the media bag key for a resource.
/// The stub returns the source for all resources.
pub fn media_key(source: &str, _mime: &str, _contents: &[u8]) -> Text {
    source.to_owned()
}
