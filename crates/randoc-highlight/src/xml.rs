//! A small XML parser for syntax definition files.
//!
//! It supports elements, attributes, text, CDATA sections, comments,
//! processing instructions, the predefined entities, character references
//! and the general entities that the internal DTD subset declares. Input is
//! not trusted: the parser limits the element depth and the size of entity
//! expansion, and it never panics.

use std::collections::{HashMap, HashSet};

/// The deepest element nesting that the parser accepts.
const MAX_DEPTH: usize = 256;

/// The largest number of characters that entity expansion can produce in
/// one document. This stops exponential entity definitions.
const MAX_EXPANSION: usize = 16 * 1024 * 1024;

/// The deepest chain of entities that refer to other entities.
const MAX_ENTITY_DEPTH: usize = 32;

/// A node of the element tree.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Element(Element),
    Text(String),
}

/// An element with its attributes in document order and its child nodes.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Element {
    pub(crate) name: String,
    pub(crate) attrs: Vec<(String, String)>,
    pub(crate) children: Vec<Node>,
}

impl Element {
    /// The value of an attribute, or an empty text when it is absent.
    pub(crate) fn attr(&self, key: &str) -> &str {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map_or("", |(_, v)| v.as_str())
    }

    /// Tests whether the element has the attribute.
    pub(crate) fn has_attr(&self, key: &str) -> bool {
        self.attrs.iter().any(|(k, _)| k == key)
    }

    /// The child elements, in document order.
    pub(crate) fn child_elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// The child elements with a name.
    pub(crate) fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.child_elements().filter(move |e| e.name == name)
    }

    /// The concatenated text of the direct text children.
    pub(crate) fn text(&self) -> String {
        self.children
            .iter()
            .filter_map(|n| match n {
                Node::Text(t) => Some(t.as_str()),
                Node::Element(_) => None,
            })
            .collect()
    }
}

/// Parses a document and returns its root element.
pub(crate) fn parse(text: &str) -> Result<Element, String> {
    let mut p = Parser {
        src: text,
        pos: 0,
        entities: HashMap::new(),
        expanded: 0,
        namespaces: Vec::new(),
    };
    p.prolog()?;
    if p.peek().is_none() {
        return Err("Missing root element\n".to_string());
    }
    let root = p.element(0)?;
    p.misc()?;
    if p.pos < p.src.len() {
        return Err(p.error("content after the root element"));
    }
    Ok(root)
}

#[cfg(test)]
thread_local! {
    /// The bytes and loop rounds that the parser examined, for the tests of
    /// linear work.
    static STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts parser work in test builds.
#[cfg(test)]
fn count(n: usize) {
    STEPS.with(|s| s.set(s.get() + n));
}

#[cfg(not(test))]
fn count(_: usize) {}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    entities: HashMap<String, String>,
    expanded: usize,
    /// The namespace declarations of the open elements, innermost last:
    /// the prefix (`None` for the default namespace) and the URI.
    namespaces: Vec<(Option<String>, String)>,
}

/// The namespace that the prefix `xml` always has.
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == ':' || (c as u32) > 0x7f
}

fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-' || c == '.'
}

impl<'a> Parser<'a> {
    fn error(&self, what: &str) -> String {
        format!("XML parse error at byte {}: {what}", self.pos)
    }

    /// An error in the form of the attoparsec `ParseError` of Pandoc 3.13,
    /// with a final newline: Pandoc prints an empty line after it. The
    /// position is `line:column (offset)` in characters.
    fn parse_error(&self, contexts: &[&str], message: &str, pos: usize) -> String {
        let before = self.src.get(..pos).unwrap_or(self.src);
        let line = 1 + before.matches('\n').count();
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = 1 + before.get(line_start..).unwrap_or("").chars().count();
        let offset = before.chars().count();
        let contexts: Vec<String> = contexts.iter().map(|c| haskell_string(c)).collect();
        format!(
            "ParseError {{errorContexts = [{}], errorMessage = {}, errorPosition = {line}:{column} ({offset})}}\n",
            contexts.join(","),
            haskell_string(message)
        )
    }

    /// The error of a start tag at `start`: the end of the input, or a
    /// character that does not fit.
    fn open_tag_error(&self, start: usize) -> String {
        if self.peek().is_none() {
            self.parse_error(&["open tag", "'>'"], "not enough input", self.src.len())
        } else {
            self.parse_error(&["open tag", "'>'"], "Failed reading: satisfy", start)
        }
    }

    /// A comment without its end: Pandoc reports the `DOCTYPE` parser.
    fn unterminated_comment(&self, start: usize) -> String {
        self.parse_error(&["DOCTYPE"], "string", start + 2)
    }

    fn rest(&self) -> &'a str {
        self.src.get(self.pos..).unwrap_or("")
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn starts(&self, s: &str) -> bool {
        self.rest().starts_with(s)
    }

    fn eat(&mut self, s: &str) -> bool {
        if self.starts(s) {
            self.pos += s.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, s: &str) -> Result<(), String> {
        if self.eat(s) {
            Ok(())
        } else {
            Err(self.error(&format!("expected {s:?}")))
        }
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            count(1);
            if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// Moves past the next `end` and returns the text before it.
    fn until(&mut self, end: &str) -> Result<&'a str, String> {
        let found = self.rest().find(end);
        count(found.map_or(self.rest().len(), |i| i + end.len()));
        match found {
            Some(i) => {
                let s = &self.rest()[..i];
                self.pos += i + end.len();
                Ok(s)
            }
            None => Err(self.error(&format!("missing {end:?}"))),
        }
    }

    fn name(&mut self) -> Result<String, String> {
        let rest = self.rest();
        let mut chars = rest.char_indices();
        match chars.next() {
            Some((_, c)) if is_name_start(c) => {}
            _ => return Err(self.error("expected a name")),
        }
        let end = chars
            .find(|&(_, c)| !is_name_char(c))
            .map_or(rest.len(), |(i, _)| i);
        self.pos += end;
        Ok(rest[..end].to_string())
    }

    fn prolog(&mut self) -> Result<(), String> {
        self.eat("\u{feff}");
        loop {
            self.skip_ws();
            let start = self.pos;
            if self.eat("<?") {
                self.until("?>")?;
            } else if self.eat("<!--") {
                self.until("-->")
                    .map_err(|_| self.unterminated_comment(start))?;
            } else if self.starts("<!DOCTYPE") {
                self.pos += "<!DOCTYPE".len();
                self.doctype()?;
            } else {
                return Ok(());
            }
        }
    }

    fn misc(&mut self) -> Result<(), String> {
        loop {
            self.skip_ws();
            if self.eat("<?") {
                self.until("?>")?;
            } else if self.eat("<!--") {
                self.until("-->")?;
            } else {
                return Ok(());
            }
        }
    }

    fn doctype(&mut self) -> Result<(), String> {
        loop {
            match self.peek() {
                None => return Err(self.error("unterminated DOCTYPE")),
                Some('>') => {
                    self.pos += 1;
                    return Ok(());
                }
                Some('[') => {
                    self.pos += 1;
                    self.internal_subset()?;
                }
                Some(q @ ('"' | '\'')) => {
                    self.pos += 1;
                    self.until(if q == '"' { "\"" } else { "'" })?;
                }
                Some(c) => self.pos += c.len_utf8(),
            }
        }
    }

    fn internal_subset(&mut self) -> Result<(), String> {
        loop {
            self.skip_ws();
            if self.eat("]") {
                return Ok(());
            } else if self.eat("<!--") {
                self.until("-->")?;
            } else if self.eat("<?") {
                self.until("?>")?;
            } else if self.eat("<!ENTITY") {
                self.entity_decl()?;
            } else if self.eat("<!") {
                self.skip_markup_decl()?;
            } else if self.peek() == Some('%') {
                self.until(";")?;
            } else {
                return Err(self.error("bad DTD content"));
            }
        }
    }

    fn skip_markup_decl(&mut self) -> Result<(), String> {
        loop {
            match self.peek() {
                None => return Err(self.error("unterminated declaration")),
                Some('>') => {
                    self.pos += 1;
                    return Ok(());
                }
                Some(q @ ('"' | '\'')) => {
                    self.pos += 1;
                    self.until(if q == '"' { "\"" } else { "'" })?;
                }
                Some(c) => self.pos += c.len_utf8(),
            }
        }
    }

    fn entity_decl(&mut self) -> Result<(), String> {
        self.skip_ws();
        let parameter = self.eat("%");
        self.skip_ws();
        let name = self.name()?;
        self.skip_ws();
        let value = match self.peek() {
            Some(q @ ('"' | '\'')) => {
                self.pos += 1;
                let raw = self.until(if q == '"' { "\"" } else { "'" })?;
                Some(self.char_refs_only(raw)?)
            }
            _ => None,
        };
        self.skip_markup_decl()?;
        if let Some(value) = value
            && !parameter
        {
            // The first declaration of an entity is binding.
            self.entities.entry(name).or_insert(value);
        }
        Ok(())
    }

    /// Replaces the character references of an entity value. General
    /// entity references stay, because they expand at the place of use.
    fn char_refs_only(&self, raw: &str) -> Result<String, String> {
        let mut out = String::with_capacity(raw.len());
        let mut rest = raw;
        while let Some(i) = rest.find("&#") {
            out.push_str(&rest[..i]);
            let after = &rest[i + 2..];
            let end = after
                .find(';')
                .ok_or_else(|| self.error("unterminated character reference"))?;
            out.push(char_ref(&after[..end]).ok_or_else(|| self.error("bad character reference"))?);
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    fn element(&mut self, depth: usize) -> Result<Element, String> {
        if depth > MAX_DEPTH {
            return Err(self.error("elements are nested too deeply"));
        }
        self.expect("<")?;
        let name = match self.name() {
            Ok(name) => name,
            Err(_) if self.peek().is_none() => {
                return Err(self.parse_error(
                    &["open tag", "name", "identifier"],
                    "not enough input",
                    self.src.len(),
                ));
            }
            Err(_) => {
                return Err(self.parse_error(
                    &["open tag", "name", "identifier"],
                    "Failed reading: satisfy",
                    self.pos,
                ));
            }
        };
        let mut el = Element {
            name,
            ..Element::default()
        };
        let scope = self.namespaces.len();
        let mut keys = HashSet::new();
        loop {
            count(1);
            let before = self.pos;
            self.skip_ws();
            if self.eat("/>") {
                self.namespaces.truncate(scope);
                return Ok(el);
            }
            if self.eat(">") {
                break;
            }
            let start = self.pos;
            if start == before || self.peek().is_none() {
                return Err(self.open_tag_error(start));
            }
            let key = self.name().map_err(|_| self.open_tag_error(start))?;
            self.skip_ws();
            self.expect("=").map_err(|_| self.open_tag_error(start))?;
            self.skip_ws();
            let quote = match self.peek() {
                Some(q @ ('"' | '\'')) => q,
                _ => return Err(self.open_tag_error(start)),
            };
            self.pos += 1;
            let raw = self
                .until(if quote == '"' { "\"" } else { "'" })
                .map_err(|_| {
                    self.parse_error(&["open tag", "'>'"], "not enough input", self.src.len())
                })?;
            if raw.contains('<') {
                return Err(self.error("'<' in an attribute value"));
            }
            let mut value = String::new();
            self.expand(raw, &mut value, 0, true)?;
            if let Some(prefix) = declared_prefix(&key) {
                // xml-conduit keeps the white space of a namespace and
                // drops a literal carriage return.
                let mut uri = String::new();
                self.expand(&raw.replace('\r', ""), &mut uri, 0, false)?;
                self.namespaces.push((prefix.map(str::to_string), uri));
            }
            if !keys.insert(key.clone()) {
                return Err(self.error("duplicate attribute"));
            }
            el.attrs.push((key, value));
        }
        let mut text = String::new();
        loop {
            count(1);
            if self.eat("</") {
                let close = self.name()?;
                self.skip_ws();
                self.expect(">")?;
                if close != el.name {
                    return Err(self.error("mismatched end tag"));
                }
                flush_text(&mut el, &mut text);
                self.namespaces.truncate(scope);
                return Ok(el);
            } else if self.eat("<!--") {
                self.until("-->")?;
            } else if self.eat("<![CDATA[") {
                let data = self.until("]]>")?;
                text.push_str(data);
            } else if self.eat("<?") {
                self.until("?>")?;
            } else if self.starts("<") {
                flush_text(&mut el, &mut text);
                let child = self.element(depth + 1)?;
                el.children.push(Node::Element(child));
            } else if self.peek().is_none() {
                return Err(self.end_element_error(&el.name));
            } else {
                let rest = self.rest();
                let end = rest.find('<').unwrap_or(rest.len());
                count(end);
                let raw = &rest[..end];
                self.pos += end;
                self.expand(raw, &mut text, 0, false)?;
            }
        }
    }

    /// The namespace of `prefix` (`None` for the default namespace) in the
    /// open elements. An empty default namespace is no namespace.
    fn namespace(&self, prefix: Option<&str>) -> Option<&str> {
        if prefix == Some("xml") {
            return Some(XML_NAMESPACE);
        }
        let (_, uri) = self
            .namespaces
            .iter()
            .rev()
            .find(|(p, _)| p.as_deref() == prefix)?;
        match (prefix, uri.as_str()) {
            (None, "") => None,
            (_, uri) => Some(uri),
        }
    }

    /// The error of `xml-conduit` for an element without its end tag.
    fn end_element_error(&self, name: &str) -> String {
        let (prefix, local) = match name.split_once(':') {
            Some((prefix, local)) => (Some(prefix), local),
            None => (None, name),
        };
        format!(
            "Expected end element for: Name {{nameLocalName = {}, nameNamespace = {}, namePrefix = {}}}, but received: EventEndDocument\n",
            haskell_string(local),
            haskell_maybe(self.namespace(prefix)),
            haskell_maybe(prefix)
        )
    }

    /// Appends `raw` to `out` with each reference expanded. In an attribute
    /// value, white space characters become spaces.
    fn expand(
        &mut self,
        raw: &str,
        out: &mut String,
        depth: usize,
        attribute: bool,
    ) -> Result<(), String> {
        if depth > MAX_ENTITY_DEPTH {
            return Err(self.error("entities are nested too deeply"));
        }
        let mut rest = raw;
        while let Some(i) = rest.find('&') {
            count(i + 1);
            push_text(out, &rest[..i], attribute);
            let after = &rest[i + 1..];
            let end = after
                .find(';')
                .ok_or_else(|| self.error("unterminated reference"))?;
            let reference = &after[..end];
            rest = &after[end + 1..];
            if let Some(code) = reference.strip_prefix('#') {
                out.push(char_ref(code).ok_or_else(|| self.error("bad character reference"))?);
                continue;
            }
            let predefined = match reference {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => None,
            };
            if let Some(c) = predefined {
                out.push(c);
                continue;
            }
            let value = self
                .entities
                .get(reference)
                .cloned()
                .ok_or_else(|| self.error(&format!("unknown entity {reference:?}")))?;
            self.expanded += value.len();
            if self.expanded > MAX_EXPANSION {
                return Err(self.error("entity expansion is too large"));
            }
            // xml-conduit keeps white space that comes from an entity in an
            // attribute value; only literal white space becomes a space.
            self.expand(&value, out, depth + 1, false)?;
        }
        push_text(out, rest, attribute);
        Ok(())
    }
}

/// The prefix that a namespace attribute declares: `Some(None)` for
/// `xmlns`, `Some(Some(p))` for `xmlns:p`.
fn declared_prefix(key: &str) -> Option<Option<&str>> {
    match key.strip_prefix("xmlns") {
        Some("") => Some(None),
        Some(rest) => rest.strip_prefix(':').map(Some),
        None => None,
    }
}

/// The names of the ASCII control characters that Haskell's `show` uses.
const ASCII_NAMES: [&str; 32] = [
    "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "a", "b", "t", "n", "v", "f", "r", "SO", "SI",
    "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC", "FS", "GS",
    "RS", "US",
];

/// A string as Haskell's `show` gives it. A character above U+007F
/// becomes its decimal code, for example `\233` for `é`. The empty escape
/// `\&` separates a numeric escape from a next digit, and `\SO` from a
/// next `H`.
fn haskell_string(s: &str) -> String {
    let mut out = String::from("\"");
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        let next = chars.peek().copied();
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ' '..='~' => out.push(c),
            '\u{7f}' => out.push_str("\\DEL"),
            '\0'..='\u{1f}' => {
                out.push('\\');
                out.push_str(ASCII_NAMES.get(c as usize).copied().unwrap_or(""));
                if c == '\u{e}' && next == Some('H') {
                    out.push_str("\\&");
                }
            }
            _ => {
                out.push_str(&format!("\\{}", u32::from(c)));
                if next.is_some_and(|d| d.is_ascii_digit()) {
                    out.push_str("\\&");
                }
            }
        }
    }
    out.push('"');
    out
}

/// A `Maybe String` as Haskell's `show` gives it.
fn haskell_maybe(s: Option<&str>) -> String {
    s.map_or_else(
        || "Nothing".to_string(),
        |s| format!("Just {}", haskell_string(s)),
    )
}

fn push_text(out: &mut String, s: &str, attribute: bool) {
    if attribute {
        out.extend(s.chars().map(|c| match c {
            '\t' | '\n' | '\r' => ' ',
            c => c,
        }));
    } else {
        out.push_str(s);
    }
}

fn flush_text(el: &mut Element, text: &mut String) {
    if !text.is_empty() {
        el.children.push(Node::Text(std::mem::take(text)));
    }
}

fn char_ref(code: &str) -> Option<char> {
    let n = match code.strip_prefix('x') {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => code.parse::<u32>().ok()?,
    };
    char::from_u32(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_elements_attributes_and_text() {
        let doc = parse(
            "<?xml version=\"1.0\"?>\n<a x='1' y=\"&lt;&#65;&#x42;\">t<b/>u<!-- c --><![CDATA[<&>]]></a>\n",
        );
        assert_eq!(
            doc,
            Ok(Element {
                name: "a".into(),
                attrs: vec![("x".into(), "1".into()), ("y".into(), "<AB".into())],
                children: vec![
                    Node::Text("t".into()),
                    Node::Element(Element {
                        name: "b".into(),
                        ..Element::default()
                    }),
                    Node::Text("u<&>".into()),
                ],
            })
        );
    }

    #[test]
    fn expands_internal_entities() {
        let src = "<!DOCTYPE l [\n<!ENTITY a \"x&#34;\">\n<!ENTITY b \"[&a;]\">\n<!ELEMENT l ANY>\n]>\n<l v=\"&b;&b;\">&a;</l>";
        let doc = parse(src);
        assert_eq!(doc.as_ref().map(|e| e.attr("v")), Ok("[x\"][x\"]"));
        assert_eq!(doc.map(|e| e.text()), Ok("x\"".to_string()));
    }

    #[test]
    fn entity_white_space_stays_in_attributes() {
        let src = "<!DOCTYPE a [<!ENTITY tab \"&#009;\">]><a v=\"&tab;x\"/>";
        assert_eq!(
            parse(src).map(|e| e.attr("v").to_string()),
            Ok("\tx".to_string())
        );
    }

    #[test]
    fn attribute_white_space_becomes_spaces() {
        assert_eq!(
            parse("<a v=\"1\t2\n3\"/>").map(|e| e.attr("v").to_string()),
            Ok("1 2 3".to_string())
        );
    }

    #[test]
    fn rejects_bad_documents() {
        for bad in [
            "",
            "<a>",
            "<a></b>",
            "<a x=1/>",
            "<a x='1' x='2'/>",
            "<a>&nope;</a>",
            "<a>&#xZZ;</a>",
            "<a/><b/>",
            "<a v='<'/>",
            "<a>&amp</a>",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    /// The messages of Pandoc 3.13 for bad definition files.
    #[test]
    fn errors_have_the_messages_of_pandoc() {
        let cases = [
            (
                "<language",
                r#"ParseError {errorContexts = ["open tag","'>'"], errorMessage = "not enough input", errorPosition = 1:10 (9)}"#,
            ),
            (
                "<language name=\"X\" ",
                r#"ParseError {errorContexts = ["open tag","'>'"], errorMessage = "not enough input", errorPosition = 1:20 (19)}"#,
            ),
            (
                "<language x>",
                r#"ParseError {errorContexts = ["open tag","'>'"], errorMessage = "Failed reading: satisfy", errorPosition = 1:11 (10)}"#,
            ),
            (
                "<",
                r#"ParseError {errorContexts = ["open tag","name","identifier"], errorMessage = "not enough input", errorPosition = 1:2 (1)}"#,
            ),
            (
                "<!-- x",
                r#"ParseError {errorContexts = ["DOCTYPE"], errorMessage = "string", errorPosition = 1:3 (2)}"#,
            ),
            (
                "<language\nname=\"X\"",
                r#"ParseError {errorContexts = ["open tag","'>'"], errorMessage = "not enough input", errorPosition = 2:9 (18)}"#,
            ),
            (
                "<a>",
                r#"Expected end element for: Name {nameLocalName = "a", nameNamespace = Nothing, namePrefix = Nothing}, but received: EventEndDocument"#,
            ),
            ("", "Missing root element"),
        ];
        for (input, message) in cases {
            assert_eq!(parse(input), Err(format!("{message}\n")), "{input:?}");
        }
    }

    /// The end-of-file error of Pandoc 3.13: the tag text as Haskell's
    /// `show` gives it, with the namespace that is in scope.
    #[test]
    fn end_element_errors_escape_and_resolve_namespaces() {
        let error = |local: &str, namespace: &str, prefix: &str| {
            Err(format!(
                "Expected end element for: Name {{nameLocalName = {local}, nameNamespace = {namespace}, namePrefix = {prefix}}}, but received: EventEndDocument\n"
            ))
        };
        let xml = format!("Just {XML_NAMESPACE:?}");
        let cases = [
            ("<é>", r#""\233""#, "Nothing", "Nothing"),
            ("<aé1>", r#""a\233\&1""#, "Nothing", "Nothing"),
            ("<a\u{85}b>", r#""a\133b""#, "Nothing", "Nothing"),
            (
                r#"<a xmlns="urn:x">"#,
                r#""a""#,
                r#"Just "urn:x""#,
                "Nothing",
            ),
            (
                r#"<a xmlns="urn:é">"#,
                r#""a""#,
                r#"Just "urn:\233""#,
                "Nothing",
            ),
            (
                r#"<a xmlns="&quot;\">"#,
                r#""a""#,
                r#"Just "\"\\""#,
                "Nothing",
            ),
            (
                r#"<a xmlns="x&#127;1&#128512;x&#233;2">"#,
                r#""a""#,
                r#"Just "x\DEL1\128512x\233\&2""#,
                "Nothing",
            ),
            ("<a xmlns=\"x\ty\">", r#""a""#, r#"Just "x\ty""#, "Nothing"),
            (
                "<a xmlns=\"x\r\ny\">",
                r#""a""#,
                r#"Just "x\ny""#,
                "Nothing",
            ),
            (
                r#"<p:a xmlns:p="urn:p">"#,
                r#""a""#,
                r#"Just "urn:p""#,
                r#"Just "p""#,
            ),
            (r#"<p:a xmlns:p="">"#, r#""a""#, r#"Just """#, r#"Just "p""#),
            ("<p:a>", r#""a""#, "Nothing", r#"Just "p""#),
            (
                r#"<a xmlns="urn:x"><b>"#,
                r#""b""#,
                r#"Just "urn:x""#,
                "Nothing",
            ),
            (
                r#"<a xmlns="urn:x"><b xmlns="">"#,
                r#""b""#,
                "Nothing",
                "Nothing",
            ),
            (
                r#"<a xmlns:p="u"><b xmlns:p="v"/><p:c>"#,
                r#""c""#,
                r#"Just "u""#,
                r#"Just "p""#,
            ),
            (
                r#"<a><b xmlns:p="v"></b><p:c>"#,
                r#""c""#,
                "Nothing",
                r#"Just "p""#,
            ),
            (
                r#"<a xmlns:p="u"><p:b xmlns:p="v">"#,
                r#""b""#,
                r#"Just "v""#,
                r#"Just "p""#,
            ),
            (r#"<xml:a xmlns:xml="u">"#, r#""a""#, &xml, r#"Just "xml""#),
            ("<xmlns:a>", r#""a""#, "Nothing", r#"Just "xmlns""#),
        ];
        for (input, local, namespace, prefix) in cases {
            assert_eq!(parse(input), error(local, namespace, prefix), "{input:?}");
        }
    }

    #[test]
    fn haskell_show_escapes() {
        let cases = [
            ("a\u{7f}1", r#""a\DEL1""#),
            ("\0\u{1}\u{e}H\u{1f}", r#""\NUL\SOH\SO\&H\US""#),
            ("\u{7}\u{8}\t\n\u{b}\u{c}\r", r#""\a\b\t\n\v\f\r""#),
            ("\u{1f600}x", r#""\128512x""#),
        ];
        for (input, shown) in cases {
            assert_eq!(haskell_string(input), shown, "{input:?}");
        }
    }

    #[test]
    fn limits_depth_and_expansion() {
        let deep = "<a>".repeat(MAX_DEPTH + 2);
        assert!(parse(&deep).is_err());
        let mut dtd = String::from("<!DOCTYPE a [<!ENTITY e0 \"xxxxxxxxxx\">");
        for i in 1..12 {
            let p = i - 1;
            dtd.push_str(&format!(
                "<!ENTITY e{i} \"&e{p};&e{p};&e{p};&e{p};&e{p};&e{p};&e{p};&e{p};\">"
            ));
        }
        dtd.push_str("]><a>&e11;</a>");
        assert!(parse(&dtd).is_err());
        let cyclic = "<!DOCTYPE a [<!ENTITY x \"&y;\"><!ENTITY y \"&x;\">]><a>&x;</a>";
        assert!(parse(cyclic).is_err());
    }

    #[test]
    fn helpers_find_children() {
        let doc = parse("<r><k n='1'/><j/><k n='2'>a<z/>b</k></r>").unwrap_or_default();
        let names: Vec<&str> = doc.children_named("k").map(|e| e.attr("n")).collect();
        assert_eq!(names, vec!["1", "2"]);
        assert_eq!(doc.child_elements().count(), 3);
        let last = doc.children_named("k").nth(1).cloned().unwrap_or_default();
        assert_eq!(last.text(), "ab");
        assert!(last.has_attr("n"));
        assert!(!last.has_attr("m"));
        assert_eq!(last.attr("m"), "");
    }

    /// Runs `f` on a thread with a 2 MB stack, as a library user has it.
    fn on_small_stack(f: impl FnOnce() + Send + 'static) {
        let done = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(f)
            .map(|h| h.join().is_ok());
        assert_eq!(done.ok(), Some(true));
    }

    #[test]
    fn deep_nesting_gives_an_error_on_a_small_stack() {
        on_small_stack(|| {
            for n in [10_000, 100_000] {
                assert!(parse(&"<a>".repeat(n)).is_err());
                let closed = format!("{}{}", "<a>".repeat(n), "</a>".repeat(n));
                assert!(parse(&closed).is_err());
            }
            let ok = format!("{}{}", "<a>".repeat(MAX_DEPTH), "</a>".repeat(MAX_DEPTH));
            assert!(parse(&ok).is_ok());
        });
    }

    /// The work of the parser for one input, in counted steps.
    fn steps(text: &str) -> usize {
        STEPS.with(|s| s.set(0));
        let _ = parse(text);
        STEPS.with(std::cell::Cell::get)
    }

    #[test]
    fn repeated_constructs_take_linear_work() {
        let constructs: [fn(usize) -> String; 6] = [
            |n| {
                let attrs: Vec<String> = (0..n).map(|i| format!("k{i}='v'")).collect();
                format!("<a {}/>", attrs.join(" "))
            },
            |n| format!("<a>{}</a>", "<b/>".repeat(n)),
            |n| format!("<a>{}</a>", "<!-- c -->".repeat(n)),
            |n| format!("<a>{}</a>", "&amp;x".repeat(n)),
            |n| format!("<!DOCTYPE a [<!ENTITY e 'xy'>]><a>{}</a>", "&e;".repeat(n)),
            |n| format!("<a>{}", "<!-- ".repeat(n)),
        ];
        for (index, make) in constructs.into_iter().enumerate() {
            let small = make(12_500);
            let large = make(50_000);
            let (s_small, s_large) = (steps(&small), steps(&large));
            assert!(
                s_large <= 4 * s_small + 64 && s_large <= 4 * large.len() + 64,
                "construct {index}: {s_small} {s_large} for {} bytes",
                large.len()
            );
        }
    }
}
