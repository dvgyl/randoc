//! OASIS OpenDocument Format (ODF 1.3) namespace URIs and XML element helpers.

use randoc_archive::xml::XmlElement;

/// `xmlns:office`
pub const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
/// `xmlns:style`
pub const STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
/// `xmlns:text`
pub const TEXT: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
/// `xmlns:table`
pub const TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
/// `xmlns:draw`
pub const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
/// `xmlns:fo`
pub const FO: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
/// `xmlns:xlink`
pub const XLINK: &str = "http://www.w3.org/1999/xlink";
/// `xmlns:svg`
pub const SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
/// `xmlns:math`
pub const MATH: &str = "http://www.w3.org/1998/Math/MathML";
/// `xmlns:xml`
pub const XML: &str = "http://www.w3.org/XML/1998/namespace";

/// Returns `true` when `el` has the exact resolved namespace URI `ns` and local name `local`.
#[inline]
pub fn is_el(el: &XmlElement, ns: &str, local: &str) -> bool {
    el.name.local == local && el.name.ns.as_deref() == Some(ns)
}
