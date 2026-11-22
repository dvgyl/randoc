//! Bounded, iterative, namespace-aware XML DOM parser and serializer.
//!
//! Designed for strict OOXML (DOCX/PPTX/XLSX), ODF (ODT/FODT), EPUB (OPF/NCX),
//! and future XML readers without recursion on document depth.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::sync::Arc;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use randoc_ast::Text;
use randoc_core::Limits;
use randoc_core::shared::named_entity;

/// Default maximum bytes expanded by entity references in one document (16 MiB).
pub const DEFAULT_MAX_ENTITY_BYTES: usize = 16 << 20;

/// Default maximum number of internal-subset `<!ENTITY>` declarations.
pub const DEFAULT_MAX_ENTITIES: usize = 100_000;

/// Default maximum size of a single attribute value in bytes (16 MiB).
pub const DEFAULT_MAX_ATTR_BYTES: usize = 16 << 20;

/// Default maximum total DOM nodes (elements, text nodes, attributes, declarations).
pub const DEFAULT_MAX_DOM_NODES: usize = 1_000_000;

/// Default maximum total allocated DOM string bytes in one document (64 MiB).
pub const DEFAULT_MAX_DOM_BYTES: usize = 64 << 20;

/// Approximate per-element structural overhead charged to `dom_bytes`.
const ELEMENT_OVERHEAD_BYTES: usize = 64;

/// Maximum depth of nested entity expansion.
const MAX_ENTITY_DEPTH: usize = 64;

/// Resource limits enforced by [`parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XmlLimits {
    /// Maximum element nesting depth.
    pub max_depth: usize,
    /// Maximum cumulative bytes added by entity expansion across text and attributes.
    pub max_entity_bytes: usize,
    /// Maximum number of `<!ENTITY>` declarations in the internal DTD subset.
    pub max_entities: usize,
    /// Maximum byte length of any single attribute value.
    pub max_attr_bytes: usize,
    /// Maximum total DOM nodes (elements, text nodes, attributes, namespace declarations).
    pub max_nodes: usize,
    /// Maximum cumulative bytes allocated for DOM strings and interned names.
    pub max_dom_bytes: usize,
}

impl XmlLimits {
    /// Derives XML limits from [`randoc_core::Limits`].
    pub fn from_limits(limits: &Limits) -> Self {
        let cap = usize::try_from(limits.max_resource_size).unwrap_or(usize::MAX);
        Self {
            max_depth: limits.max_nesting_depth,
            max_entity_bytes: DEFAULT_MAX_ENTITY_BYTES.min(cap),
            max_entities: DEFAULT_MAX_ENTITIES,
            max_attr_bytes: DEFAULT_MAX_ATTR_BYTES.min(cap),
            max_nodes: DEFAULT_MAX_DOM_NODES.min(limits.max_table_cells.saturating_mul(5)),
            max_dom_bytes: DEFAULT_MAX_DOM_BYTES.min(cap),
        }
    }
}

impl Default for XmlLimits {
    fn default() -> Self {
        Self::from_limits(&Limits::default())
    }
}

/// Per-format parsing options for [`parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct XmlOptions {
    /// When `true`, resolve HTML5 named character references (e.g. `&eacute;`)
    /// even without an internal DTD declaration. DOCX/ODT/EPUB set this `false`.
    pub html_entities: bool,
}

/// A namespace-resolved XML qualified name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QName {
    /// Resolved namespace URI, if any (interned across the document).
    pub ns: Option<Arc<str>>,
    /// Namespace prefix as written in the XML, if any (interned across the document).
    pub prefix: Option<Arc<str>>,
    /// Local name (after `:` if prefixed).
    pub local: Text,
}

/// One child node of an [`XmlElement`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlNode {
    /// A child element.
    Element(XmlElement),
    /// Character data.
    Text(Text),
}

/// An XML element node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlElement {
    /// Qualified element name.
    pub name: QName,
    /// Non-`xmlns` attributes in document order.
    pub attrs: Vec<(QName, Text)>,
    /// Namespace declarations (`xmlns` -> `None`, `xmlns:p` -> `Some("p")`)
    /// declared directly on this element, in document order.
    pub ns_decls: Vec<(Option<Arc<str>>, Arc<str>)>,
    /// Child nodes (elements and text).
    pub children: Vec<XmlNode>,
}

/// A parsed XML document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlDoc {
    /// The single root element.
    pub root: XmlElement,
}

/// Errors produced by [`decode_utf8`] and [`parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlError {
    /// XML well-formedness or syntax error.
    Syntax {
        /// Character offset in the input string.
        offset_chars: usize,
        /// 1-based line number.
        line: usize,
        /// 1-based column number.
        column: usize,
        /// Human-readable description.
        detail: Text,
    },
    /// Element nesting exceeded `max_depth`.
    Depth(usize),
    /// Entity expansion or declaration count exceeded configured bounds.
    EntityBudget,
    /// Total DOM node count or allocated string bytes exceeded configured bounds.
    DomBudget,
    /// Reference to an undeclared named entity.
    UnknownEntity(Text),
    /// Input bytes are not valid UTF-8.
    NotUtf8 {
        /// First invalid byte.
        byte: u8,
        /// Byte offset of the first invalid byte.
        offset: usize,
    },
}

impl XmlError {
    /// Formats the error in a concise conduit-style message suitable for
    /// `randoc_core::Error::Xml`.
    pub fn format_conduit(&self) -> Text {
        match self {
            XmlError::Syntax {
                line,
                column,
                detail,
                ..
            } => format!("Line {line}, column {column}: {detail}"),
            XmlError::Depth(limit) => {
                format!("XML nesting exceeded limit of {limit} levels")
            }
            XmlError::EntityBudget => "XML entity expansion exceeded limit".to_owned(),
            XmlError::DomBudget => "XML DOM allocation exceeded limit".to_owned(),
            XmlError::UnknownEntity(name) => format!("Unresolved entity &{name};"),
            XmlError::NotUtf8 { byte, .. } => {
                format!(
                    "Cannot decode byte '\\x{byte:02x}': Data.Text.Internal.Encoding: Invalid UTF-8 stream"
                )
            }
        }
    }
}

impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format_conduit())
    }
}

impl std::error::Error for XmlError {}

impl XmlElement {
    /// Returns the value of the first attribute with matching namespace URI and local name.
    pub fn attr(&self, ns: &str, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(q, _)| q.local == local && q.ns.as_deref() == Some(ns))
            .map(|(_, v)| v.as_str())
    }

    /// Returns the value of the first attribute with matching local name (ignoring namespace).
    pub fn attr_local(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(q, _)| q.local == local)
            .map(|(_, v)| v.as_str())
    }

    /// Returns the value of the first attribute with matching local name and no namespace.
    pub fn attr_unqualified(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(q, _)| q.local == local && q.ns.is_none())
            .map(|(_, v)| v.as_str())
    }

    /// Returns the first child element with matching namespace URI and local name.
    pub fn child(&self, ns: &str, local: &str) -> Option<&XmlElement> {
        self.elements()
            .find(|el| el.name.local == local && el.name.ns.as_deref() == Some(ns))
    }

    /// Returns the first child element with matching local name.
    pub fn child_local(&self, local: &str) -> Option<&XmlElement> {
        self.elements().find(|el| el.name.local == local)
    }

    /// Iterates over all child elements with matching namespace URI and local name.
    pub fn children_ns<'a>(
        &'a self,
        ns: &'a str,
        local: &'a str,
    ) -> impl Iterator<Item = &'a XmlElement> {
        self.elements()
            .filter(move |el| el.name.local == local && el.name.ns.as_deref() == Some(ns))
    }

    /// Iterates over all child elements with matching local name.
    pub fn children_local<'a>(&'a self, local: &'a str) -> impl Iterator<Item = &'a XmlElement> {
        self.elements().filter(move |el| el.name.local == local)
    }

    /// Iterates over all direct child elements.
    pub fn elements(&self) -> impl Iterator<Item = &XmlElement> {
        self.children.iter().filter_map(|node| match node {
            XmlNode::Element(el) => Some(el),
            XmlNode::Text(_) => None,
        })
    }

    /// Concatenates all direct text children of this element.
    pub fn text(&self) -> Text {
        let mut out = Text::new();
        for node in &self.children {
            if let XmlNode::Text(t) = node {
                out.push_str(t);
            }
        }
        out
    }

    /// Concatenates all descendant text iteratively (never recurses on depth).
    pub fn all_text(&self) -> Text {
        let mut out = Text::new();
        let mut stack: Vec<&[XmlNode]> = vec![&self.children];
        while let Some(slice) = stack.pop() {
            if let Some((first, rest)) = slice.split_first() {
                if !rest.is_empty() {
                    stack.push(rest);
                }
                match first {
                    XmlNode::Text(t) => out.push_str(t),
                    XmlNode::Element(el) => {
                        if !el.children.is_empty() {
                            stack.push(&el.children);
                        }
                    }
                }
            }
        }
        out
    }

    /// Serializes this element and its subtree to XML iteratively.
    pub fn write_to(&self, out: &mut String) {
        enum Step<'a> {
            Element(&'a XmlElement),
            Text(&'a str),
            Close(&'a XmlElement),
        }
        let mut stack = vec![Step::Element(self)];
        while let Some(step) = stack.pop() {
            match step {
                Step::Text(t) => escape_xml_text(t, out),
                Step::Element(elem) => {
                    out.push('<');
                    write_qname(&elem.name, out);
                    for (prefix, uri) in &elem.ns_decls {
                        match prefix {
                            Some(p) => {
                                out.push_str(" xmlns:");
                                out.push_str(p);
                            }
                            None => out.push_str(" xmlns"),
                        }
                        out.push_str("=\"");
                        escape_xml_attr(uri, out);
                        out.push('"');
                    }
                    for (attr_name, value) in &elem.attrs {
                        out.push(' ');
                        write_qname(attr_name, out);
                        out.push_str("=\"");
                        escape_xml_attr(value, out);
                        out.push('"');
                    }
                    if elem.children.is_empty() {
                        out.push_str(" />");
                    } else {
                        out.push('>');
                        stack.push(Step::Close(elem));
                        for child in elem.children.iter().rev() {
                            match child {
                                XmlNode::Element(el) => stack.push(Step::Element(el)),
                                XmlNode::Text(t) => stack.push(Step::Text(t.as_str())),
                            }
                        }
                    }
                }
                Step::Close(elem) => {
                    out.push_str("</");
                    write_qname(&elem.name, out);
                    out.push('>');
                }
            }
        }
    }
}

fn write_qname(name: &QName, out: &mut String) {
    if let Some(prefix) = &name.prefix {
        out.push_str(prefix);
        out.push(':');
    }
    out.push_str(&name.local);
}

/// Escapes character data for XML text content.
pub fn escape_xml_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

/// Escapes character data for a double-quoted XML attribute value.
pub fn escape_xml_attr(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

/// Decodes strict UTF-8 bytes, stripping an optional UTF-8 BOM.
pub fn decode_utf8(bytes: &[u8]) -> Result<&str, XmlError> {
    let stripped = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let bom_len = bytes.len() - stripped.len();
    std::str::from_utf8(stripped).map_err(|err| {
        let idx = err.valid_up_to();
        let byte = stripped.get(idx).copied().unwrap_or(0);
        XmlError::NotUtf8 {
            byte,
            offset: bom_len + idx,
        }
    })
}

/// Parses an XML document string into an [`XmlDoc`] within `limits`.
pub fn parse(input: &str, limits: &XmlLimits, options: XmlOptions) -> Result<XmlDoc, XmlError> {
    let input = input.strip_prefix('\u{FEFF}').unwrap_or(input);
    let mut parser = Parser::new(input, *limits, options)?;
    if let Some((idx, _)) = input.char_indices().find(|&(_, ch)| !is_valid_xml_char(ch)) {
        return Err(parser.syntax_at(idx, "invalid XML 1.0 character"));
    }
    parser.parse_stream(input, 0)?;
    parser.finish()
}

struct OpenElement {
    element: XmlElement,
    bound_prefixes: Vec<Option<Arc<str>>>,
}

struct Parser<'a> {
    root_input: &'a str,
    limits: XmlLimits,
    options: XmlOptions,
    stack: Vec<OpenElement>,
    root: Option<XmlElement>,
    namespaces: BTreeMap<Option<Arc<str>>, Vec<Arc<str>>>,
    interned: HashSet<Arc<str>>,
    entities: BTreeMap<Text, Text>,
    entity_bytes: usize,
    dom_nodes: usize,
    dom_bytes: usize,
    expanding: BTreeSet<Text>,
    seen_doctype: bool,
}

fn new_strict_reader(input: &str) -> Reader<&[u8]> {
    let mut reader = Reader::from_str(input);
    let cfg = reader.config_mut();
    cfg.trim_text(false);
    cfg.check_end_names = true;
    cfg.allow_unmatched_ends = false;
    cfg.allow_dangling_amp = false;
    cfg.expand_empty_elements = false;
    cfg.check_comments = true;
    reader
}

impl<'a> Parser<'a> {
    fn new(root_input: &'a str, limits: XmlLimits, options: XmlOptions) -> Result<Self, XmlError> {
        let mut parser = Self {
            root_input,
            limits,
            options,
            stack: Vec::new(),
            root: None,
            namespaces: BTreeMap::new(),
            interned: HashSet::new(),
            entities: BTreeMap::new(),
            entity_bytes: 0,
            dom_nodes: 0,
            dom_bytes: 0,
            expanding: BTreeSet::new(),
            seen_doctype: false,
        };
        let xml_prefix: Arc<str> = Arc::from("xml");
        let xml_uri: Arc<str> = Arc::from("http://www.w3.org/XML/1998/namespace");
        parser.interned.insert(Arc::clone(&xml_prefix));
        parser.interned.insert(Arc::clone(&xml_uri));
        parser.namespaces.insert(Some(xml_prefix), vec![xml_uri]);
        Ok(parser)
    }

    fn charge_dom(&mut self, nodes: usize, bytes: usize) -> Result<(), XmlError> {
        self.dom_nodes = self.dom_nodes.saturating_add(nodes);
        self.dom_bytes = self.dom_bytes.saturating_add(bytes);
        if self.dom_nodes > self.limits.max_nodes || self.dom_bytes > self.limits.max_dom_bytes {
            return Err(XmlError::DomBudget);
        }
        Ok(())
    }

    fn intern(&mut self, s: &str) -> Result<Arc<str>, XmlError> {
        if let Some(existing) = self.interned.get(s) {
            return Ok(Arc::clone(existing));
        }
        self.charge_dom(0, s.len())?;
        let arc: Arc<str> = Arc::from(s);
        self.interned.insert(Arc::clone(&arc));
        Ok(arc)
    }

    fn syntax_at(&self, byte_offset: usize, detail: impl Into<Text>) -> XmlError {
        let clamped = byte_offset.min(self.root_input.len());
        let prefix = self.root_input.get(..clamped).unwrap_or("");
        let mut line = 1usize;
        let mut column = 1usize;
        let mut offset_chars = 0usize;
        for ch in prefix.chars() {
            offset_chars += 1;
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        XmlError::Syntax {
            offset_chars,
            line,
            column,
            detail: detail.into(),
        }
    }

    fn parse_stream(&mut self, src: &str, entity_depth: usize) -> Result<(), XmlError> {
        if entity_depth > MAX_ENTITY_DEPTH {
            return Err(XmlError::EntityBudget);
        }
        let mut reader = new_strict_reader(src);
        loop {
            let start_pos = usize::try_from(reader.buffer_position()).unwrap_or(0);
            let event = match reader.read_event() {
                Ok(ev) => ev,
                Err(err) => {
                    let err_pos = usize::try_from(reader.error_position()).unwrap_or(start_pos);
                    return Err(self.syntax_at(err_pos, err.to_string()));
                }
            };
            match event {
                Event::Start(tag) => self.on_start(&tag, start_pos, false)?,
                Event::Empty(tag) => self.on_start(&tag, start_pos, true)?,
                Event::End(tag) => self.on_end(&tag, start_pos)?,
                Event::Text(text) => {
                    let content = text.xml10_content();
                    if content.contains('&') {
                        return Err(self.syntax_at(start_pos, "unescaped '&' in text"));
                    }
                    self.push_text(&content, start_pos)?;
                }
                Event::CData(data) => {
                    let content = data.into_inner();
                    self.push_text(&content, start_pos)?;
                }
                Event::GeneralRef(ref_ev) => {
                    let name: &str = &ref_ev;
                    self.on_general_ref(name, start_pos, entity_depth)?;
                }
                Event::DocType(dt) => {
                    if entity_depth > 0 {
                        return Err(self.syntax_at(start_pos, "DOCTYPE not allowed inside entity"));
                    }
                    if self.seen_doctype || self.root.is_some() || !self.stack.is_empty() {
                        return Err(self.syntax_at(start_pos, "unexpected DOCTYPE"));
                    }
                    self.seen_doctype = true;
                    let raw = dt.into_inner();
                    self.parse_doctype(&raw, start_pos)?;
                }
                Event::Decl(_) | Event::Comment(_) | Event::PI(_) => {}
                Event::Eof => break,
            }
        }
        Ok(())
    }

    fn on_start(&mut self, tag: &BytesStart<'_>, pos: usize, empty: bool) -> Result<(), XmlError> {
        if self.root.is_some() {
            return Err(self.syntax_at(pos, "multiple root elements"));
        }
        if self.stack.len() >= self.limits.max_depth {
            return Err(XmlError::Depth(self.limits.max_depth));
        }

        let raw_name = tag.name().0;
        if raw_name.is_empty() {
            return Err(self.syntax_at(pos, "empty element name"));
        }

        let mut ns_decls: Vec<(Option<Arc<str>>, Arc<str>)> = Vec::new();
        let mut bound_prefixes: Vec<Option<Arc<str>>> = Vec::new();
        let mut raw_attrs: Vec<(Text, Text)> = Vec::new();

        for attr_res in tag.attributes() {
            let attr = attr_res.map_err(|err| self.syntax_at(pos, err.to_string()))?;
            let key = attr.key.0;
            let raw_val_str = attr.value.as_ref();
            let value = self.expand_attr_value(raw_val_str, pos)?;
            if value.len() > self.limits.max_attr_bytes {
                return Err(XmlError::EntityBudget);
            }
            if key == "xmlns" {
                let uri = self.intern(&value)?;
                self.charge_dom(1, 0)?;
                self.namespaces
                    .entry(None)
                    .or_default()
                    .push(Arc::clone(&uri));
                bound_prefixes.push(None);
                ns_decls.push((None, uri));
            } else if let Some(prefix) = key.strip_prefix("xmlns:") {
                if prefix.is_empty() {
                    return Err(self.syntax_at(pos, "empty namespace prefix"));
                }
                let p = self.intern(prefix)?;
                let uri = self.intern(&value)?;
                self.charge_dom(1, 0)?;
                self.namespaces
                    .entry(Some(Arc::clone(&p)))
                    .or_default()
                    .push(Arc::clone(&uri));
                bound_prefixes.push(Some(Arc::clone(&p)));
                ns_decls.push((Some(p), uri));
            } else {
                raw_attrs.push((Text::from(key), value));
            }
        }

        let elem_name = self.resolve_qname(raw_name, false)?;
        self.charge_dom(1, ELEMENT_OVERHEAD_BYTES + elem_name.local.len())?;
        let mut attrs = Vec::with_capacity(raw_attrs.len());
        for (k, v) in raw_attrs {
            let q = self.resolve_qname(&k, true)?;
            self.charge_dom(1, q.local.len().saturating_add(v.len()))?;
            attrs.push((q, v));
        }

        let element = XmlElement {
            name: elem_name,
            attrs,
            ns_decls,
            children: Vec::new(),
        };
        self.stack.push(OpenElement {
            element,
            bound_prefixes,
        });
        if empty {
            self.pop_element();
        }
        Ok(())
    }

    fn on_end(
        &mut self,
        tag: &quick_xml::events::BytesEnd<'_>,
        pos: usize,
    ) -> Result<(), XmlError> {
        let raw_end = tag.name().0;
        let (end_prefix, end_local) = match raw_end.split_once(':') {
            Some((p, l)) => (Some(p), l),
            None => (None, raw_end),
        };
        let Some(open) = self.stack.last() else {
            return Err(self.syntax_at(pos, "unexpected closing tag"));
        };
        if open.element.name.local != end_local || open.element.name.prefix.as_deref() != end_prefix
        {
            return Err(self.syntax_at(
                pos,
                format!(
                    "mismatched closing tag </{}> for <{}>",
                    raw_end, open.element.name.local
                ),
            ));
        }
        self.pop_element();
        Ok(())
    }

    fn pop_element(&mut self) {
        if let Some(open) = self.stack.pop() {
            for prefix in &open.bound_prefixes {
                if let Some(uris) = self.namespaces.get_mut(prefix) {
                    uris.pop();
                }
            }
            match self.stack.last_mut() {
                Some(parent) => parent.element.children.push(XmlNode::Element(open.element)),
                None => self.root = Some(open.element),
            }
        }
    }

    fn push_text(&mut self, text: &str, pos: usize) -> Result<(), XmlError> {
        if text.is_empty() {
            return Ok(());
        }
        if text.chars().any(|ch| !is_valid_xml_char(ch)) {
            return Err(self.syntax_at(pos, "invalid XML 1.0 character in text"));
        }
        let Some(open) = self.stack.last() else {
            if text.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')) {
                return Ok(());
            }
            return Err(self.syntax_at(pos, "character data outside root element"));
        };
        let adding_node = usize::from(!matches!(
            open.element.children.last(),
            Some(XmlNode::Text(_))
        ));
        self.charge_dom(adding_node, text.len())?;
        if let Some(open) = self.stack.last_mut() {
            if let Some(XmlNode::Text(prev)) = open.element.children.last_mut() {
                prev.push_str(text);
            } else {
                open.element.children.push(XmlNode::Text(Text::from(text)));
            }
        }
        Ok(())
    }

    fn on_general_ref(
        &mut self,
        name: &str,
        pos: usize,
        entity_depth: usize,
    ) -> Result<(), XmlError> {
        if let Some(resolved) = resolve_builtin_or_char_ref(name) {
            let resolved = resolved.map_err(|msg| self.syntax_at(pos, msg))?;
            self.push_text(&resolved, pos)?;
            return Ok(());
        }
        if let Some(replacement) = self.entities.get(name).cloned() {
            if self.expanding.contains(name) {
                return Err(self.syntax_at(pos, format!("recursive entity &{name};")));
            }
            self.charge_entity_bytes(replacement.len())?;
            let key = Text::from(name);
            self.expanding.insert(key.clone());
            let res = if replacement.contains('<') || replacement.contains('&') {
                self.parse_stream(&replacement, entity_depth + 1)
            } else {
                self.push_text(&replacement, pos)
            };
            self.expanding.remove(&key);
            return res;
        }
        if self.options.html_entities
            && let Some(val) = named_entity(name)
        {
            self.push_text(val, pos)?;
            return Ok(());
        }
        Err(XmlError::UnknownEntity(Text::from(name)))
    }

    fn expand_attr_value(&mut self, raw: &str, pos: usize) -> Result<Text, XmlError> {
        if raw.chars().any(|ch| !is_valid_xml_char(ch)) {
            return Err(self.syntax_at(pos, "invalid XML 1.0 character in attribute"));
        }
        if !raw.contains('&') {
            return Ok(Text::from(raw));
        }
        let mut out = String::with_capacity(raw.len());
        self.expand_attr_into(raw, pos, 0, &mut out)?;
        Ok(out)
    }

    fn expand_attr_into(
        &mut self,
        raw: &str,
        pos: usize,
        depth: usize,
        out: &mut String,
    ) -> Result<(), XmlError> {
        if depth > MAX_ENTITY_DEPTH {
            return Err(XmlError::EntityBudget);
        }
        let mut rest = raw;
        while let Some(amp) = rest.find('&') {
            out.push_str(&rest[..amp]);
            let after_amp = &rest[amp + 1..];
            let Some(semi) = after_amp.find(';') else {
                return Err(self.syntax_at(pos, "unterminated entity reference in attribute"));
            };
            let name = &after_amp[..semi];
            rest = &after_amp[semi + 1..];
            if let Some(resolved) = resolve_builtin_or_char_ref(name) {
                let s = resolved.map_err(|msg| self.syntax_at(pos, msg))?;
                out.push_str(&s);
            } else if let Some(replacement) = self.entities.get(name).cloned() {
                if self.expanding.contains(name) {
                    return Err(self.syntax_at(pos, format!("recursive entity &{name};")));
                }
                self.charge_entity_bytes(replacement.len())?;
                if out.len().saturating_add(replacement.len()) > self.limits.max_attr_bytes {
                    return Err(XmlError::EntityBudget);
                }
                let key = Text::from(name);
                self.expanding.insert(key.clone());
                let r = self.expand_attr_into(&replacement, pos, depth + 1, out);
                self.expanding.remove(&key);
                r?;
            } else if self.options.html_entities
                && let Some(val) = named_entity(name)
            {
                out.push_str(val);
            } else {
                return Err(XmlError::UnknownEntity(Text::from(name)));
            }
            if out.len() > self.limits.max_attr_bytes {
                return Err(XmlError::EntityBudget);
            }
        }
        out.push_str(rest);
        Ok(())
    }

    fn charge_entity_bytes(&mut self, added: usize) -> Result<(), XmlError> {
        self.entity_bytes = self.entity_bytes.saturating_add(added);
        if self.entity_bytes > self.limits.max_entity_bytes {
            return Err(XmlError::EntityBudget);
        }
        Ok(())
    }

    fn resolve_qname(&mut self, raw: &str, is_attr: bool) -> Result<QName, XmlError> {
        if let Some((prefix, local)) = raw.split_once(':') {
            let p = self.intern(prefix)?;
            let ns = self
                .namespaces
                .get(&Some(Arc::clone(&p)))
                .and_then(|v| v.last())
                .filter(|u| !u.is_empty())
                .cloned();
            Ok(QName {
                ns,
                prefix: Some(p),
                local: Text::from(local),
            })
        } else {
            // Unprefixed attributes do not inherit the default namespace in XML.
            let ns = if is_attr {
                None
            } else {
                self.namespaces
                    .get(&None)
                    .and_then(|v| v.last())
                    .filter(|u| !u.is_empty())
                    .cloned()
            };
            Ok(QName {
                ns,
                prefix: None,
                local: Text::from(raw),
            })
        }
    }

    /// Parses internal DTD subset `[ ... ]` for `<!ENTITY name "value">`
    /// declarations while ignoring external `PUBLIC`/`SYSTEM` identifiers.
    fn parse_doctype(&mut self, raw: &str, pos: usize) -> Result<(), XmlError> {
        // Locate internal subset `[ ... ]` outside quotes.
        let bytes = raw.as_bytes();
        let mut i = 0usize;
        let mut subset_start = None;
        while i < bytes.len() {
            match bytes[i] {
                b'"' | b'\'' => {
                    let q = bytes[i];
                    i += 1;
                    while i < bytes.len() && bytes[i] != q {
                        i += 1;
                    }
                    i += 1;
                }
                b'[' => {
                    subset_start = Some(i + 1);
                    break;
                }
                _ => i += 1,
            }
        }
        let Some(start) = subset_start else {
            return Ok(());
        };
        // Find matching closing `]` scanning from the end.
        let Some(rel_end) = raw[start..].rfind(']') else {
            return Err(self.syntax_at(pos, "unclosed DOCTYPE internal subset"));
        };
        let subset = &raw[start..start + rel_end];
        let mut cur = subset;
        while let Some(decl_idx) = cur.find("<!") {
            cur = &cur[decl_idx + 2..];
            if let Some(after_comment) = cur.strip_prefix("--") {
                if let Some(end_c) = after_comment.find("-->") {
                    cur = &after_comment[end_c + 3..];
                } else {
                    return Err(self.syntax_at(pos, "unclosed comment in DTD"));
                }
                continue;
            }
            if let Some(after_ent) = cur.strip_prefix("ENTITY")
                && after_ent
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_whitespace())
            {
                let rest = after_ent.trim_start();
                // Skip parameter entities (`<!ENTITY % ...>`).
                if rest.starts_with('%') {
                    if let Some(gt) = rest.find('>') {
                        cur = &rest[gt + 1..];
                    } else {
                        break;
                    }
                    continue;
                }
                let name_end = rest
                    .find(|c: char| c.is_ascii_whitespace())
                    .unwrap_or(rest.len());
                let name = &rest[..name_end];
                let after_name = rest[name_end..].trim_start();
                if let Some(quote) = after_name.as_bytes().first().copied()
                    && (quote == b'"' || quote == b'\'')
                {
                    let body = &after_name[1..];
                    if let Some(end_q) = body.find(char::from(quote)) {
                        let val = &body[..end_q];
                        if self.entities.len() >= self.limits.max_entities {
                            return Err(XmlError::EntityBudget);
                        }
                        // First declaration wins per XML 1.0 §4.2.
                        if !self.entities.contains_key(name) {
                            self.charge_dom(1, name.len().saturating_add(val.len()))?;
                            self.entities.insert(Text::from(name), Text::from(val));
                        }
                        let after_val = &body[end_q + 1..];
                        if let Some(gt) = after_val.find('>') {
                            cur = &after_val[gt + 1..];
                            continue;
                        }
                    }
                }
                // External entity or unrecognized form: ignore without fetching.
                if let Some(gt) = after_name.find('>') {
                    cur = &after_name[gt + 1..];
                } else {
                    break;
                }
            } else if let Some(gt) = cur.find('>') {
                cur = &cur[gt + 1..];
            } else {
                break;
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Result<XmlDoc, XmlError> {
        if let Some(open) = self.stack.last() {
            return Err(self.syntax_at(
                self.root_input.len(),
                format!("unclosed element <{}>", open.element.name.local),
            ));
        }
        let root = self
            .root
            .take()
            .ok_or_else(|| self.syntax_at(self.root_input.len(), "missing root element"))?;
        Ok(XmlDoc { root })
    }
}

fn resolve_builtin_or_char_ref(name: &str) -> Option<Result<String, &'static str>> {
    match name {
        "amp" => Some(Ok("&".to_owned())),
        "lt" => Some(Ok("<".to_owned())),
        "gt" => Some(Ok(">".to_owned())),
        "quot" => Some(Ok("\"".to_owned())),
        "apos" => Some(Ok("'".to_owned())),
        _ if name.starts_with('#') => {
            let body = &name[1..];
            let parsed = if let Some(hex) = body.strip_prefix('x') {
                if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Some(Err("invalid hex character reference"));
                }
                u32::from_str_radix(hex, 16).ok()
            } else {
                if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
                    return Some(Err("invalid decimal character reference"));
                }
                body.parse::<u32>().ok()
            };
            match parsed.and_then(char::from_u32) {
                Some(ch) if is_valid_xml_char(ch) => Some(Ok(ch.to_string())),
                _ => Some(Err("invalid XML character reference")),
            }
        }
        _ => None,
    }
}

fn is_valid_xml_char(ch: char) -> bool {
    let cp = u32::from(ch);
    cp == 0x9
        || cp == 0xA
        || cp == 0xD
        || (0x20..=0xD7FF).contains(&cp)
        || (0xE000..=0xFFFD).contains(&cp)
        || (0x10000..=0x10FFFF).contains(&cp)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_namespaces_and_preserves_root_decls() {
        let xml = r#"<w:document xmlns:w="urn:custom" xmlns:r="urn:rel"><w:body><y:p xmlns:y="urn:custom" r:id="1">hi &amp; bye</y:p></w:body></w:document>"#;
        let doc = parse(xml, &XmlLimits::default(), XmlOptions::default()).expect("parse");
        assert_eq!(doc.root.name.local, "document");
        assert_eq!(doc.root.name.ns.as_deref(), Some("urn:custom"));
        assert_eq!(doc.root.ns_decls.len(), 2);
        let body = doc.root.child("urn:custom", "body").expect("body");
        let p = body.child("urn:custom", "p").expect("p");
        assert_eq!(p.attr("urn:rel", "id"), Some("1"));
        assert_eq!(p.text(), "hi & bye");
    }

    #[test]
    fn expands_internal_entities_and_stops_billion_laughs() {
        let ok = r#"<!DOCTYPE doc [<!ENTITY x "EXPANDED">]><doc a="&x;">&x;</doc>"#;
        let doc = parse(ok, &XmlLimits::default(), XmlOptions::default()).expect("ok");
        assert_eq!(doc.root.text(), "EXPANDED");
        assert_eq!(doc.root.attr_local("a"), Some("EXPANDED"));

        let mut bomb = String::from("<!DOCTYPE lolz [\n<!ENTITY lol \"lol\">\n");
        for i in 1..=20 {
            let prev = if i == 1 {
                "lol".to_owned()
            } else {
                format!("lol{}", i - 1)
            };
            bomb.push_str(&format!(
                "<!ENTITY lol{i} \"&{p};&{p};&{p};&{p};&{p};&{p};&{p};&{p};&{p};&{p};\">\n",
                p = prev
            ));
        }
        bomb.push_str("]><lolz>&lol20;</lolz>");
        let limits = XmlLimits {
            max_entity_bytes: 64 * 1024,
            ..XmlLimits::default()
        };
        assert_eq!(
            parse(&bomb, &limits, XmlOptions::default()).err(),
            Some(XmlError::EntityBudget)
        );
    }

    #[test]
    fn skips_external_public_dtd_without_io() {
        let mathml = r#"<!DOCTYPE math:math PUBLIC "-//OpenOffice.org//DTD Modified W3C MathML 1.01//EN" "math.dtd"><math:math xmlns:math="http://www.w3.org/1998/Math/MathML"><math:mi>x</math:mi></math:math>"#;
        let doc = parse(mathml, &XmlLimits::default(), XmlOptions::default()).expect("mathml");
        assert_eq!(doc.root.name.local, "math");
    }

    #[test]
    fn strict_mode_rejects_unknown_entities_and_bare_amp() {
        assert!(matches!(
            parse("<a b='1&2'/>", &XmlLimits::default(), XmlOptions::default()),
            Err(XmlError::Syntax { .. })
        ));
        assert!(matches!(
            parse("<a>&eacute;</a>", &XmlLimits::default(), XmlOptions::default()),
            Err(XmlError::UnknownEntity(name)) if name == "eacute"
        ));
        let doc = parse(
            "<a>&eacute;</a>",
            &XmlLimits::default(),
            XmlOptions {
                html_entities: true,
            },
        )
        .expect("html entities");
        assert_eq!(doc.root.text(), "é");
    }

    #[test]
    fn deep_nesting_safe_on_small_stack() {
        let handle = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let depth = 50_000usize;
                let mut xml = String::with_capacity(depth * 7);
                for _ in 0..depth {
                    xml.push_str("<a>");
                }
                for _ in 0..depth {
                    xml.push_str("</a>");
                }
                let err = parse(&xml, &XmlLimits::default(), XmlOptions::default());
                assert_eq!(err, Err(XmlError::Depth(128)));
            })
            .expect("spawn");
        handle.join().expect("no stack overflow");
    }

    #[test]
    fn decode_utf8_strips_bom_and_reports_bad_byte() {
        assert_eq!(decode_utf8(b"\xEF\xBB\xBF<a/>"), Ok("<a/>"));
        assert_eq!(
            decode_utf8(b"abc\xE9def"),
            Err(XmlError::NotUtf8 {
                byte: 0xE9,
                offset: 3
            })
        );
    }

    #[test]
    fn interns_namespace_uris_and_caps_dom_budget() {
        let long_ns = format!("urn:{}", "x".repeat(4096));
        let mut xml = format!(r#"<p:root xmlns:p="{long_ns}">"#);
        for _ in 0..500 {
            xml.push_str("<p:item/>");
        }
        xml.push_str("</p:root>");
        // Without interning, 501 elements * 4 KiB > 2 MiB; with interning, well under 128 KiB.
        let limits = XmlLimits {
            max_dom_bytes: 128 * 1024,
            max_nodes: 2_000,
            ..XmlLimits::default()
        };
        let doc = parse(&xml, &limits, XmlOptions::default()).expect("interned ns fits");
        let root_ns = doc.root.name.ns.as_ref().expect("root ns");
        let first_child = doc.root.elements().next().expect("child");
        let child_ns = first_child.name.ns.as_ref().expect("child ns");
        assert!(Arc::ptr_eq(root_ns, child_ns));

        // Node cap rejects excess DOM nodes.
        let tight_nodes = XmlLimits {
            max_nodes: 100,
            ..XmlLimits::default()
        };
        assert_eq!(
            parse(&xml, &tight_nodes, XmlOptions::default()).err(),
            Some(XmlError::DomBudget)
        );

        // Byte cap rejects excess DOM string bytes.
        let tight_bytes = XmlLimits {
            max_dom_bytes: 2 * 1024,
            ..XmlLimits::default()
        };
        assert_eq!(
            parse(&xml, &tight_bytes, XmlOptions::default()).err(),
            Some(XmlError::DomBudget)
        );
    }

    #[test]
    fn rejects_forbidden_xml10_literal_chars() {
        for sample in [
            "<a b='x\x00y'/>",
            "<a>\x01</a>",
            "<a>\x0B</a>",
            "<a>\x0C</a>",
            "<a>\x1F</a>",
            "<a><![CDATA[bad\x08data]]></a>",
            "<a>\u{FFFE}</a>",
            "<a>\u{FFFF}</a>",
        ] {
            assert!(
                matches!(
                    parse(sample, &XmlLimits::default(), XmlOptions::default()),
                    Err(XmlError::Syntax { .. })
                ),
                "expected syntax error for {sample:?}"
            );
        }
        // Legal whitespace (TAB, LF, CR) remains accepted.
        let ok = parse(
            "<a>\t\n\r ok</a>",
            &XmlLimits::default(),
            XmlOptions::default(),
        )
        .expect("legal whitespace");
        assert!(ok.root.text().contains("ok"));
    }
}
