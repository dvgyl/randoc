//! OOXML namespace resolution following Pandoc's dynamic root `w` prefix rule.

use std::sync::Arc;

use randoc_archive::xml::XmlElement;

/// Standard Transitional OOXML relationship namespace URI.
pub const R_NS_TRANSITIONAL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// Strict OOXML relationship namespace URI.
pub const R_NS_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";

/// Standard Transitional OOXML math namespace URI.
pub const M_NS_TRANSITIONAL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
/// Strict OOXML math namespace URI.
pub const M_NS_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/math";

/// DrawingML wordprocessingDrawing namespaces.
pub const WP_NS_TRANSITIONAL: &str =
    "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
pub const WP_NS_STRICT: &str = "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing";

/// DrawingML main namespaces.
pub const A_NS_TRANSITIONAL: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub const A_NS_STRICT: &str = "http://purl.oclc.org/ooxml/drawingml/main";

/// DrawingML picture namespaces.
pub const PIC_NS_TRANSITIONAL: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
pub const PIC_NS_STRICT: &str = "http://purl.oclc.org/ooxml/drawingml/picture";

/// VML namespace.
pub const V_NS: &str = "urn:schemas-microsoft-com:vml";
/// Markup Compatibility namespace.
pub const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
/// WordprocessingShape namespace.
pub const WPS_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
/// Word 2010 extensions namespace (e.g. `w14:checkbox`).
pub const W14_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordml";

/// Namespace context extracted from an OOXML part's root element.
///
/// Pandoc matches WordprocessingML elements by reading the `xmlns:w` binding
/// declared on the root element and matching any element whose resolved
/// namespace URI equals that URI.
#[derive(Debug, Clone)]
pub struct DocxNs {
    /// Resolved URI bound to the `w` prefix on the part's root element.
    pub w_uri: Option<Arc<str>>,
    /// Resolved URI bound to the `r` prefix on the part's root element, if any.
    pub r_uri: Option<Arc<str>>,
    /// Resolved URI bound to the `m` prefix on the part's root element, if any.
    pub m_uri: Option<Arc<str>>,
}

impl DocxNs {
    /// Extracts prefix bindings from a part's root element.
    pub fn from_root(root: &XmlElement) -> Self {
        let mut w_uri = None;
        let mut r_uri = None;
        let mut m_uri = None;
        for (prefix, uri) in &root.ns_decls {
            match prefix.as_deref() {
                Some("w") => w_uri = Some(Arc::clone(uri)),
                Some("r") => r_uri = Some(Arc::clone(uri)),
                Some("m") => m_uri = Some(Arc::clone(uri)),
                _ => {}
            }
        }
        Self {
            w_uri,
            r_uri,
            m_uri,
        }
    }

    /// Returns `true` if `el` is a WordprocessingML element with local name `local`.
    pub fn is_w(&self, el: &XmlElement, local: &str) -> bool {
        el.name.local == local && self.w_uri.is_some() && el.name.ns == self.w_uri
    }

    /// Returns `true` if `el` is in the WordprocessingML namespace.
    pub fn in_w(&self, el: &XmlElement) -> bool {
        self.w_uri.is_some() && el.name.ns == self.w_uri
    }

    /// Returns the first direct WordprocessingML child with local name `local`.
    pub fn w_child<'a>(&self, el: &'a XmlElement, local: &str) -> Option<&'a XmlElement> {
        el.elements().find(|c| self.is_w(c, local))
    }

    /// Iterates direct WordprocessingML children with local name `local`.
    pub fn w_children<'a>(
        &'a self,
        el: &'a XmlElement,
        local: &'a str,
    ) -> impl Iterator<Item = &'a XmlElement> {
        el.elements().filter(move |c| self.is_w(c, local))
    }

    /// Reads a WordprocessingML attribute (`w:local`), also accepting an
    /// unqualified attribute of the same local name when absent.
    pub fn w_attr<'a>(&self, el: &'a XmlElement, local: &str) -> Option<&'a str> {
        if let Some(w_ns) = self.w_uri.as_deref()
            && let Some(val) = el.attr(w_ns, local)
        {
            return Some(val);
        }
        el.attr_unqualified(local)
    }

    /// Reads an `r:local` relationship attribute (matching root `r` URI,
    /// Transitional, or Strict relationship namespace).
    pub fn r_attr<'a>(&self, el: &'a XmlElement, local: &str) -> Option<&'a str> {
        if let Some(r_ns) = self.r_uri.as_deref()
            && let Some(val) = el.attr(r_ns, local)
        {
            return Some(val);
        }
        el.attr(R_NS_TRANSITIONAL, local)
            .or_else(|| el.attr(R_NS_STRICT, local))
    }

    /// Returns `true` if `el` is an OMML math element with local name `local`.
    pub fn is_m(&self, el: &XmlElement, local: &str) -> bool {
        if el.name.local != local {
            return false;
        }
        let ns = el.name.ns.as_deref();
        ns == Some(M_NS_TRANSITIONAL)
            || ns == Some(M_NS_STRICT)
            || (self.m_uri.is_some() && el.name.ns == self.m_uri)
    }

    /// Reads an `m:local` attribute (or unqualified fallback).
    pub fn m_attr<'a>(&self, el: &'a XmlElement, local: &str) -> Option<&'a str> {
        if let Some(m_ns) = self.m_uri.as_deref()
            && let Some(val) = el.attr(m_ns, local)
        {
            return Some(val);
        }
        el.attr(M_NS_TRANSITIONAL, local)
            .or_else(|| el.attr(M_NS_STRICT, local))
            .or_else(|| el.attr_unqualified(local))
    }

    /// Returns the first direct OMML child with local name `local`.
    pub fn m_child<'a>(&self, el: &'a XmlElement, local: &str) -> Option<&'a XmlElement> {
        el.elements().find(|c| self.is_m(c, local))
    }

    /// Parses an OOXML boolean toggle element (`<w:b/>` => `true`, `val="0"|"false"|"off"|"none"` => `false`).
    pub fn toggle_val(&self, el: &XmlElement) -> bool {
        match self.w_attr(el, "val") {
            None => true,
            Some(v) => !matches!(
                v.to_ascii_lowercase().as_str(),
                "0" | "false" | "off" | "none"
            ),
        }
    }
}

/// Returns `true` if `el` is a DrawingML `wp:*` element with `local`.
pub fn is_wp(el: &XmlElement, local: &str) -> bool {
    el.name.local == local
        && matches!(
            el.name.ns.as_deref(),
            Some(WP_NS_TRANSITIONAL | WP_NS_STRICT)
        )
}

/// Returns `true` if `el` is a DrawingML `a:*` element with `local`.
pub fn is_a(el: &XmlElement, local: &str) -> bool {
    el.name.local == local && matches!(el.name.ns.as_deref(), Some(A_NS_TRANSITIONAL | A_NS_STRICT))
}

/// Returns `true` if `el` is a DrawingML `pic:*` element with `local`.
pub fn is_pic(el: &XmlElement, local: &str) -> bool {
    el.name.local == local
        && matches!(
            el.name.ns.as_deref(),
            Some(PIC_NS_TRANSITIONAL | PIC_NS_STRICT)
        )
}

/// Returns `true` if `el` is a VML `v:*` element with `local`.
pub fn is_v(el: &XmlElement, local: &str) -> bool {
    el.name.local == local && el.name.ns.as_deref() == Some(V_NS)
}

/// Returns `true` if `el` is a Markup Compatibility `mc:*` element with `local`.
pub fn is_mc(el: &XmlElement, local: &str) -> bool {
    el.name.local == local && el.name.ns.as_deref() == Some(MC_NS)
}

/// Returns `true` if `el` is a WordprocessingShape `wps:*` element with `local`.
pub fn is_wps(el: &XmlElement, local: &str) -> bool {
    el.name.local == local && el.name.ns.as_deref() == Some(WPS_NS)
}

/// Returns `true` if `el` is a Word 2010 `w14:*` element with `local`.
pub fn is_w14(el: &XmlElement, local: &str) -> bool {
    el.name.local == local && el.name.ns.as_deref() == Some(W14_NS)
}
