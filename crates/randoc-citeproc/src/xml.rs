//! The XML layer: a document tree from `quick-xml` events (ADR 10), with the
//! error texts of the Haskell library `xml-conduit`, which the oracle shows
//! after `CiteprocXMLError: `. The texts come from the oracle matrix in
//! `tests/replay/parse-errors.txt`.

use std::collections::BTreeMap;

use quick_xml::Reader;
use quick_xml::errors::{Error as QuickError, IllFormedError, SyntaxError};
use quick_xml::events::{BytesStart, Event};

/// The largest number of bytes that entity references can add to one document.
const MAX_ENTITY_BYTES: usize = 16 << 20;

/// The largest number of entity declarations in one document.
const MAX_ENTITIES: usize = 100_000;

/// An element name with its namespace, as `xml-conduit` gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Name {
    pub(crate) local: String,
    pub(crate) namespace: Option<String>,
    pub(crate) prefix: Option<String>,
}

/// One child of an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Node {
    Element(XmlElement),
    Text(String),
}

/// An element. The attributes are keyed by local name, as `getAttributes` of
/// the upstream code keys them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct XmlElement {
    pub(crate) name: Name,
    pub(crate) attributes: BTreeMap<String, String>,
    pub(crate) children: Vec<Node>,
}

impl XmlElement {
    /// The element children with the given local name (`getChildren`).
    pub(crate) fn children_named<'a>(
        &'a self,
        local: &'a str,
    ) -> impl Iterator<Item = &'a XmlElement> {
        self.elements().filter(move |e| e.name.local == local)
    }

    /// All element children (`allChildren`).
    pub(crate) fn elements(&self) -> impl Iterator<Item = &XmlElement> {
        self.children.iter().filter_map(|node| match node {
            Node::Element(element) => Some(element),
            Node::Text(_) => None,
        })
    }

    /// The text children joined (`getTextContent`).
    pub(crate) fn text(&self) -> String {
        self.children
            .iter()
            .filter_map(|node| match node {
                Node::Text(text) => Some(text.as_str()),
                Node::Element(_) => None,
            })
            .collect()
    }

    /// The value of an attribute.
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }
}

/// An XML error: the text after `CiteprocXMLError: `.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct XmlError(pub(crate) String);

/// Parses a document and returns its root element.
pub(crate) fn parse_document(input: &str, max_depth: usize) -> Result<XmlElement, XmlError> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    Parser::new(input, max_depth).run()
}

/// An event of the `xml-conduit` stream, for error texts.
enum ShownEvent {
    Begin(Name, Vec<(Name, String)>),
    End(Name),
    Content(String),
}

struct Open {
    element: XmlElement,
    /// The prefixes that this element binds (`None` for the default namespace).
    bindings: Vec<Option<String>>,
}

struct Parser<'a> {
    input: &'a str,
    max_depth: usize,
    stack: Vec<Open>,
    root: Option<XmlElement>,
    /// The namespace bindings in scope: a stack of URIs for each prefix.
    namespaces: BTreeMap<Option<String>, Vec<String>>,
    entities: BTreeMap<String, String>,
    entity_bytes: usize,
    /// A document type declaration was read.
    seen_doctype: bool,
    /// The entities that are expanding now, innermost last.
    expanding: Vec<String>,
}

/// The configuration of `quick-xml` for a document and for the replacement text of an entity.
fn new_reader(input: &str) -> Reader<&[u8]> {
    let mut reader = Reader::from_str(input);
    let config = reader.config_mut();
    config.trim_text(false);
    config.check_end_names = false;
    config.allow_unmatched_ends = true;
    config.check_comments = false;
    config.expand_empty_elements = false;
    config.allow_dangling_amp = true;
    reader
}

/// Why the replacement text of an entity does not expand.
enum FragmentError {
    /// An error that the oracle shows as it is.
    Keep(XmlError),
    /// An error that the oracle shows as the entity being unresolved.
    Unresolved,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, max_depth: usize) -> Self {
        Parser {
            input,
            max_depth,
            stack: Vec::new(),
            root: None,
            namespaces: BTreeMap::new(),
            entities: BTreeMap::new(),
            entity_bytes: 0,
            seen_doctype: false,
            expanding: Vec::new(),
        }
    }

    fn run(mut self) -> Result<XmlElement, XmlError> {
        let mut reader = new_reader(self.input);
        loop {
            let start = offset(reader.buffer_position());
            let event = match reader.read_event() {
                Ok(event) => event,
                Err(error) => {
                    return Err(self.quick_error(&error, offset(reader.error_position()), start));
                }
            };
            let end = offset(reader.buffer_position());
            match event {
                Event::Start(tag) => self.start(&tag, start, end, false)?,
                Event::Empty(tag) => self.start(&tag, start, end, true)?,
                Event::End(tag) => {
                    let local = tag.name().0.to_owned();
                    if local.is_empty() {
                        return Err(name_error("close tag", &position(self.input, start + 2)));
                    }
                    self.end(&local, start, end)?;
                }
                Event::Text(text) => {
                    let content = text.xml10_content();
                    if let Some(index) = content.find('&') {
                        return Err(self.text_error(ValueError::Syntax, start + index));
                    }
                    self.text(&content, start, end)?;
                }
                Event::CData(data) => self.text(&data.into_inner(), start, end)?,
                Event::GeneralRef(reference) => {
                    let name: &str = &reference;
                    match self.simple_reference(name) {
                        Some(text) => {
                            let text = text.map_err(|e| self.text_error(e, start))?;
                            self.text(&text, start, end)?;
                        }
                        None => self.content_entity(name, start, end)?,
                    }
                }
                Event::DocType(_) => self.doctype(start, end)?,
                Event::PI(pi) => {
                    if !pi
                        .target()
                        .chars()
                        .next()
                        .is_some_and(|c| !c.is_ascii_whitespace())
                    {
                        return Err(name_error_in(
                            &["instruction", "identifier"],
                            &position(self.input, start + 2),
                        ));
                    }
                }
                Event::Comment(_) | Event::Decl(_) => {}
                Event::Eof => return self.finish(),
            }
        }
    }

    fn finish(mut self) -> Result<XmlElement, XmlError> {
        if let Some(open) = self.stack.last() {
            return Err(XmlError(format!(
                "Expected end element for: {}, but received: EventEndDocument",
                show_name(&open.element.name)
            )));
        }
        self.root
            .take()
            .ok_or_else(|| XmlError("Missing root element".to_owned()))
    }

    fn range(&self, start: usize, end: usize) -> String {
        format!(
            "{}-{}",
            position(self.input, start),
            position(self.input, end)
        )
    }

    fn after_root(&self, event: ShownEvent, start: usize, end: usize) -> XmlError {
        XmlError(format!(
            "{}: Found content after root element: {}",
            self.range(start, end),
            show_event(&event)
        ))
    }

    fn start(
        &mut self,
        tag: &BytesStart<'_>,
        start: usize,
        end: usize,
        empty: bool,
    ) -> Result<(), XmlError> {
        // `xml-conduit` skips spaces after `<`; `quick-xml` puts them in the name.
        let content = format!("{}{}", tag.name().0, tag.attributes_raw());
        let lead = content.len() - content.trim_start().len();
        let name_length = content[lead..]
            .find(|c: char| !is_name_char(c))
            .unwrap_or(content.len() - lead);
        if name_length == 0 {
            return Err(name_error(
                "open tag",
                &position(self.input, start + 1 + lead),
            ));
        }
        let qname = content[lead..lead + name_length].to_owned();
        if let Some(second) = second_colon(&qname) {
            return Err(self.open_tag_error(start + 1 + lead + second));
        }
        let raw = &content[lead + name_length..];
        let attributes_offset = start + 1 + lead + name_length;
        let raw_attributes = scan_attributes(raw, true).map_err(|error| match error {
            ScanError::At(at) => self.open_tag_error(attributes_offset + at),
            ScanError::Eof => self.unclosed_tag_error(),
        })?;
        let mut bindings = Vec::new();
        let mut plain = Vec::new();
        for (at, name, value) in raw_attributes {
            if let Some(second) = second_colon(name) {
                let _ = second;
                return Err(self.open_tag_error(attributes_offset + at));
            }
            let value = self.attribute_value(value).map_err(|e| match e {
                ValueError::Syntax => self.open_tag_error(attributes_offset + at),
                ValueError::Unresolved(name) => unresolved(&name),
                ValueError::Budget => budget_error(),
            })?;
            let prefix = match name.strip_prefix("xmlns") {
                Some("") => Some(None),
                Some(rest) => rest.strip_prefix(':').map(|p| Some(p.to_owned())),
                None => None,
            };
            if let Some(prefix) = prefix {
                self.namespaces
                    .entry(prefix.clone())
                    .or_default()
                    .push(value);
                bindings.push(prefix);
            } else {
                plain.push((name.to_owned(), value));
            }
        }
        let name = self.resolve(&qname, false);
        let attributes: Vec<(Name, String)> = plain
            .into_iter()
            .map(|(qname, value)| (self.resolve(&qname, true), value))
            .collect();
        if self.root.is_some() {
            return Err(self.after_root(ShownEvent::Begin(name, attributes), start, end));
        }
        if self.stack.len() >= self.max_depth {
            return Err(XmlError(format!(
                "the style is nested deeper than {} levels (a Randoc limit)",
                self.max_depth
            )));
        }
        let mut keyed: BTreeMap<String, (Option<String>, String)> = BTreeMap::new();
        for (name, value) in attributes {
            // `M.mapKeys` keeps the value of the greatest name: a namespace beats none.
            match keyed.get(&name.local) {
                Some((namespace, _)) if *namespace > name.namespace => {}
                _ => {
                    keyed.insert(name.local, (name.namespace, value));
                }
            }
        }
        let element = XmlElement {
            name,
            attributes: keyed.into_iter().map(|(k, (_, v))| (k, v)).collect(),
            children: Vec::new(),
        };
        self.stack.push(Open { element, bindings });
        if empty {
            self.close();
        }
        Ok(())
    }

    fn close(&mut self) {
        if let Some(open) = self.stack.pop() {
            for prefix in &open.bindings {
                if let Some(uris) = self.namespaces.get_mut(prefix) {
                    uris.pop();
                }
            }
            match self.stack.last_mut() {
                Some(parent) => parent.element.children.push(Node::Element(open.element)),
                None => self.root = Some(open.element),
            }
        }
    }

    fn end(&mut self, qname: &str, start: usize, end: usize) -> Result<(), XmlError> {
        let name = self.resolve(qname, false);
        let Some(open) = self.stack.last() else {
            return Err(self.after_root(ShownEvent::End(name), start, end));
        };
        if open.element.name != name {
            return Err(XmlError(format!(
                "{}: Expected end element for: {}, but received: {}",
                self.range(start, end),
                show_name(&open.element.name),
                show_event(&ShownEvent::End(name))
            )));
        }
        self.close();
        Ok(())
    }

    fn text(&mut self, text: &str, start: usize, end: usize) -> Result<(), XmlError> {
        if text.is_empty() {
            return Ok(());
        }
        match self.stack.last_mut() {
            Some(open) => {
                match open.element.children.last_mut() {
                    Some(Node::Text(previous)) => previous.push_str(text),
                    _ => open.element.children.push(Node::Text(text.to_owned())),
                }
                Ok(())
            }
            None if text.chars().all(char::is_whitespace) => Ok(()),
            None => Err(self.after_root(ShownEvent::Content(text.to_owned()), start, end)),
        }
    }

    fn resolve(&self, qname: &str, attribute: bool) -> Name {
        let (prefix, local) = match qname.split_once(':') {
            Some((prefix, local)) => (Some(prefix), local),
            None => (None, qname),
        };
        let namespace = match prefix {
            Some("xml") => Some("http://www.w3.org/XML/1998/namespace".to_owned()),
            Some(prefix) => self.lookup(Some(prefix)),
            None if attribute => None,
            None => self.lookup(None).filter(|uri| !uri.is_empty()),
        };
        let prefix = match (prefix, &namespace) {
            (Some(prefix), None) => Some(prefix.to_owned()),
            _ => None,
        };
        Name {
            local: local.to_owned(),
            namespace,
            prefix,
        }
    }

    fn lookup(&self, prefix: Option<&str>) -> Option<String> {
        self.namespaces
            .get(&prefix.map(str::to_owned))
            .and_then(|uris| uris.last())
            .cloned()
    }

    /// Reads a document type declaration: checks its place and reads the
    /// `ENTITY` declarations of the internal subset in one pass.
    fn doctype(&mut self, start: usize, end: usize) -> Result<(), XmlError> {
        let raw = self.input.get(start..end).unwrap_or("");
        if !raw.starts_with("<!DOCTYPE") {
            return Err(parse_error(
                &["DOCTYPE"],
                "string",
                &position(self.input, start + 2),
            ));
        }
        let body = raw
            .get("<!DOCTYPE".len()..raw.len().saturating_sub(1))
            .unwrap_or("");
        let name: String = body
            .trim_start()
            .chars()
            .take_while(|c| is_name_char(*c) && *c != '[')
            .collect();
        let shown = format!("EventBeginDoctype {} Nothing", show_string(&name));
        if let Some(open) = self.stack.last() {
            return Err(XmlError(format!(
                "{}: Expected end element for: {}, but received: {shown}",
                self.range(start, end),
                show_name(&open.element.name)
            )));
        }
        if self.root.is_some() || self.seen_doctype {
            return Err(XmlError(format!(
                "{}: Found content after root element: {shown}",
                self.range(start, end)
            )));
        }
        self.seen_doctype = true;
        let base = start + "<!DOCTYPE".len();
        let Some(open) = body.find('[') else {
            return Ok(());
        };
        let entities = parse_subset(body, open + 1).map_err(|(at, contexts)| {
            parse_error(
                contexts,
                "Failed reading: satisfy",
                &position(self.input, base + at),
            )
        })?;
        for (name, value) in entities {
            if self.entities.len() >= MAX_ENTITIES {
                return Err(XmlError(format!(
                    "the style declares more than {MAX_ENTITIES} entities (a Randoc limit)"
                )));
            }
            self.entities.entry(name).or_insert(value);
        }
        Ok(())
    }

    /// A character reference or one of the five predefined entities, or
    /// `None` for a name that the DTD must declare.
    fn simple_reference(&mut self, name: &str) -> Option<Result<String, ValueError>> {
        if let Some(number) = name.strip_prefix('#') {
            return Some(
                char_reference(number)
                    .map(String::from)
                    .ok_or(ValueError::Syntax),
            );
        }
        let text = match name {
            "lt" => "<",
            "gt" => ">",
            "amp" => "&",
            "quot" => "\"",
            "apos" => "'",
            _ => return None,
        };
        Some(self.count(text.len()).map(|()| text.to_owned()))
    }

    fn count(&mut self, bytes: usize) -> Result<(), ValueError> {
        self.entity_bytes += bytes;
        if self.entity_bytes > MAX_ENTITY_BYTES {
            return Err(ValueError::Budget);
        }
        Ok(())
    }

    /// The replacement text of a declared entity. An entity that expands
    /// inside itself, or deeper than the depth limit, is unresolved.
    fn replacement(&mut self, name: &str) -> Result<String, ValueError> {
        let unresolved = || ValueError::Unresolved(name.to_owned());
        if self.expanding.iter().any(|n| n == name) || self.expanding.len() >= self.max_depth {
            return Err(unresolved());
        }
        let value = self.entities.get(name).cloned().ok_or_else(unresolved)?;
        self.count(value.len())?;
        Ok(value)
    }

    /// Expands a declared entity in text content. Markup in the replacement
    /// text goes on the same element stack.
    fn content_entity(&mut self, name: &str, start: usize, end: usize) -> Result<(), XmlError> {
        let value = self
            .replacement(name)
            .map_err(|e| self.text_error(e, start))?;
        if !value.contains(['<', '&']) {
            return self.text(&value, start, end);
        }
        self.expanding.push(name.to_owned());
        let result = self.fragment(&value, start, end);
        self.expanding.pop();
        result.map_err(|error| match error {
            FragmentError::Keep(error) => error,
            FragmentError::Unresolved => unresolved(name),
        })
    }

    fn fragment(&mut self, value: &str, start: usize, end: usize) -> Result<(), FragmentError> {
        let mut reader = new_reader(value);
        loop {
            let event = reader.read_event().map_err(|_| FragmentError::Unresolved)?;
            match event {
                Event::Start(tag) => self
                    .start(&tag, start, end, false)
                    .map_err(FragmentError::Keep)?,
                Event::Empty(tag) => self
                    .start(&tag, start, end, true)
                    .map_err(FragmentError::Keep)?,
                Event::End(tag) => {
                    let local = tag.name().0.to_owned();
                    self.end(&local, start, end).map_err(FragmentError::Keep)?;
                }
                Event::Text(text) => {
                    let content = text.xml10_content();
                    if content.contains('&') {
                        return Err(FragmentError::Unresolved);
                    }
                    self.text(&content, start, end)
                        .map_err(FragmentError::Keep)?;
                }
                Event::CData(data) => self
                    .text(&data.into_inner(), start, end)
                    .map_err(FragmentError::Keep)?,
                Event::GeneralRef(reference) => {
                    let name: &str = &reference;
                    match self.simple_reference(name) {
                        Some(Ok(text)) => {
                            self.text(&text, start, end).map_err(FragmentError::Keep)?;
                        }
                        Some(Err(ValueError::Budget)) => {
                            return Err(FragmentError::Keep(budget_error()));
                        }
                        Some(Err(_)) => return Err(FragmentError::Unresolved),
                        None => self
                            .content_entity(name, start, end)
                            .map_err(|_| FragmentError::Unresolved)?,
                    }
                }
                Event::Eof => return Ok(()),
                Event::DocType(_) | Event::PI(_) | Event::Comment(_) | Event::Decl(_) => {}
            }
        }
    }

    /// Expands a declared entity in an attribute value: the text of its
    /// replacement text, with the references in it expanded.
    fn attribute_entity(&mut self, name: &str) -> Result<String, ValueError> {
        if !self.expanding.is_empty() {
            // The oracle does not expand a declared entity in an attribute
            // of markup that comes from an entity.
            return Err(ValueError::Unresolved(name.to_owned()));
        }
        self.attribute_entity_text(name)
    }

    fn attribute_entity_text(&mut self, name: &str) -> Result<String, ValueError> {
        let value = self.replacement(name)?;
        if !value.contains(['<', '&']) {
            return Ok(value);
        }
        self.expanding.push(name.to_owned());
        let result = self.attribute_fragment(name, &value);
        self.expanding.pop();
        result
    }

    fn attribute_fragment(&mut self, name: &str, value: &str) -> Result<String, ValueError> {
        let unresolved = || ValueError::Unresolved(name.to_owned());
        let mut reader = new_reader(value);
        let mut out = String::new();
        loop {
            match reader.read_event().map_err(|_| unresolved())? {
                Event::Text(text) => {
                    let content = text.xml10_content();
                    if content.contains('&') {
                        return Err(unresolved());
                    }
                    out.push_str(&content);
                }
                Event::CData(data) => out.push_str(&data.into_inner()),
                Event::GeneralRef(reference) => {
                    let inner: &str = &reference;
                    let text = match self.simple_reference(inner) {
                        Some(text) => text,
                        None => self.attribute_entity_text(inner),
                    };
                    match text {
                        Ok(text) => out.push_str(&text),
                        Err(ValueError::Budget) => return Err(ValueError::Budget),
                        Err(_) => return Err(unresolved()),
                    }
                }
                Event::Eof => return Ok(out),
                _ => {}
            }
        }
    }

    fn attribute_value(&mut self, raw: &str) -> Result<String, ValueError> {
        let mut out = String::with_capacity(raw.len());
        let mut rest = raw;
        while let Some(index) = rest.find(['&', '<']) {
            out.push_str(&rest[..index]);
            if rest[index..].starts_with('<') {
                return Err(ValueError::Syntax);
            }
            let after = &rest[index + 1..];
            let end = after.find(';').ok_or(ValueError::Syntax)?;
            let name = &after[..end];
            if name.is_empty() || name.contains(|c: char| c.is_whitespace() || c == '&') {
                return Err(ValueError::Syntax);
            }
            let text = match self.simple_reference(name) {
                Some(text) => text?,
                None => self.attribute_entity(name)?,
            };
            out.push_str(&text);
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    fn unclosed_tag_error(&self) -> XmlError {
        parse_error(
            &["open tag", "'>'"],
            "not enough input",
            &position(self.input, self.input.len()),
        )
    }

    fn open_tag_error(&self, at: usize) -> XmlError {
        parse_error(
            &["open tag", "'>'"],
            "Failed reading: satisfy",
            &position(self.input, at),
        )
    }

    fn text_error(&self, error: ValueError, at: usize) -> XmlError {
        match error {
            ValueError::Syntax => parse_error(
                &["text content"],
                "Failed reading: parseTextContent",
                &position(self.input, at),
            ),
            ValueError::Unresolved(name) => unresolved(&name),
            ValueError::Budget => budget_error(),
        }
    }

    fn quick_error(&self, error: &QuickError, at: usize, start: usize) -> XmlError {
        let eof = position(self.input, self.input.len());
        match error {
            QuickError::Syntax(
                SyntaxError::UnclosedComment
                | SyntaxError::UnclosedCData
                | SyntaxError::InvalidBangMarkup,
            ) => parse_error(&["DOCTYPE"], "string", &position(self.input, start + 2)),
            QuickError::Syntax(SyntaxError::UnclosedDoctype) => {
                // With an internal subset, the recursive upstream parser
                // reports the first item of the subset.
                match self.input.get(start..).and_then(|rest| rest.find('[')) {
                    Some(open) => {
                        let first = start + open + 1;
                        let message = if first >= self.input.len() {
                            "not enough input"
                        } else {
                            "Failed reading: satisfy"
                        };
                        parse_error(&["DOCTYPE"], message, &position(self.input, first))
                    }
                    None => parse_error(&["DOCTYPE"], "not enough input", &eof),
                }
            }
            QuickError::Syntax(SyntaxError::UnclosedXmlDecl) => {
                parse_error(&["instruction", "'?'"], "not enough input", &eof)
            }
            QuickError::Syntax(SyntaxError::UnclosedPI) => {
                parse_error(&["instruction"], "not enough input", &eof)
            }
            QuickError::IllFormed(IllFormedError::MissingEndTag(_)) => match self.stack.last() {
                Some(open) => XmlError(format!(
                    "Expected end element for: {}, but received: EventEndDocument",
                    show_name(&open.element.name)
                )),
                None => XmlError("Missing root element".to_owned()),
            },
            QuickError::Syntax(
                SyntaxError::UnclosedTag
                | SyntaxError::UnclosedSingleQuotedAttributeValue
                | SyntaxError::UnclosedDoubleQuotedAttributeValue,
            ) => {
                let tag = self.input.get(start + 1..).unwrap_or("");
                if tag.starts_with('/') {
                    return parse_error(&["close tag", "'>'"], "not enough input", &eof);
                }
                let lead = tag.len() - tag.trim_start().len();
                let name_length = tag[lead..]
                    .find(|c: char| !is_name_char(c))
                    .unwrap_or(tag.len() - lead);
                let at = start + 1 + lead + name_length;
                match scan_attributes(tag.get(lead + name_length..).unwrap_or(""), false) {
                    Err(ScanError::At(offset)) => self.open_tag_error(at + offset),
                    _ => self.unclosed_tag_error(),
                }
            }
            _ => parse_error(
                &["open tag", "'>'"],
                "Failed reading: satisfy",
                &position(self.input, at),
            ),
        }
    }
}

enum ValueError {
    Syntax,
    Unresolved(String),
    Budget,
}

fn unresolved(name: &str) -> XmlError {
    XmlError(format!(
        "UnresolvedEntityException (fromList [{}])",
        show_string(name)
    ))
}

fn budget_error() -> XmlError {
    XmlError(format!(
        "entity references add more than {MAX_ENTITY_BYTES} bytes (a Randoc limit)"
    ))
}

/// The byte offset of a second colon: a qualified name has at most one.
fn second_colon(name: &str) -> Option<usize> {
    let first = name.find(':')?;
    name[first + 1..].find(':').map(|i| first + 1 + i)
}

/// A character of a name for the attoparsec grammar of `xml-conduit`.
fn is_name_char(c: char) -> bool {
    !c.is_whitespace()
        && !matches!(
            c,
            '/' | '>' | '<' | '=' | '"' | '\'' | '?' | '!' | '&' | ';'
        )
}

fn name_error(context: &str, position: &str) -> XmlError {
    name_error_in(&[context, "name", "identifier"], position)
}

fn name_error_in(contexts: &[&str], position: &str) -> XmlError {
    parse_error(contexts, "Failed reading: takeWhile1", position)
}

#[cfg(test)]
thread_local! {
    /// The bytes that the reader of the internal subset looks at.
    static SUBSET_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts bytes that the reader of the internal subset looks at (test builds only).
fn step(_bytes: usize) {
    #[cfg(test)]
    SUBSET_STEPS.with(|steps| steps.set(steps.get() + _bytes));
}

/// The contexts of an error in a document type declaration.
type SubsetError = (usize, &'static [&'static str]);

/// Reads the internal subset of a document type declaration from byte `at`
/// of `body` (the text between `<!DOCTYPE` and the final `>`). Each byte is
/// read once or twice, so time is linear. The grammar comes from an oracle
/// matrix (`tests/replay/parse-errors.txt`, cases `dtd-*`):
///
/// - `<!ENTITY NAME "VALUE">` defines an entity. After the name, another
///   form is skipped to the next `>` and defines nothing. A missing name is
///   an error.
/// - Each other `<` is skipped to the next `>`, and other text to the next
///   `<` or `]`.
///
/// The upstream parser is recursive, so each error shows the position of
/// the first item of the subset.
fn parse_subset(body: &str, start: usize) -> Result<Vec<(String, String)>, SubsetError> {
    const DOCTYPE: &[&str] = &["DOCTYPE"];
    let mut at = start;
    let mut entities = Vec::new();
    loop {
        let rest = &body[at..];
        match rest.as_bytes().first() {
            None => return Err((start, DOCTYPE)),
            Some(b']') => {
                let after = at + 1 + (rest[1..].len() - rest[1..].trim_start().len());
                return if after == body.len() {
                    Ok(entities)
                } else {
                    Err((after, &["DOCTYPE", "'>'"]))
                };
            }
            Some(b'<') => {
                let (entity, length) = match rest.strip_prefix("<!ENTITY") {
                    Some(declaration) => entity_declaration(declaration).ok_or((start, DOCTYPE))?,
                    None => (None, rest.find('>').ok_or((start, DOCTYPE))?),
                };
                let length = if entity.is_some() {
                    length
                } else {
                    // Skip to the next `>` from the end of the part that was read.
                    let skipped = rest[length..].find('>').ok_or((start, DOCTYPE))?;
                    length + skipped + 1
                };
                entities.extend(entity);
                step(length);
                at += length;
            }
            Some(_) => {
                let length = rest.find(['<', ']']).unwrap_or(rest.len());
                step(length);
                at += length;
            }
        }
    }
}

fn skip_space(text: &str) -> &str {
    text.trim_start_matches([' ', '\t', '\n', '\r'])
}

/// The rest of `<!ENTITY`: the entity, if the declaration has the form
/// `NAME "VALUE" >`, and the length that was read including `<!ENTITY`.
/// `None` means that the name is missing.
fn entity_declaration(text: &str) -> Option<(Option<(String, String)>, usize)> {
    let read = |rest: &str| "<!ENTITY".len() + text.len() - rest.len();
    let rest = skip_space(text);
    let name_length = rest
        .find(|c: char| !is_name_char(c) || c == ':')
        .unwrap_or(rest.len());
    if name_length == 0 {
        return None;
    }
    let (name, rest) = rest.split_at(name_length);
    let rest = skip_space(rest);
    let Some(quote) = rest.chars().next().filter(|q| matches!(q, '"' | '\'')) else {
        return Some((None, read(rest)));
    };
    let Some(close) = rest[1..].find(quote) else {
        return Some((None, read(rest)));
    };
    let value = &rest[1..=close];
    let after = skip_space(&rest[close + 2..]);
    match after.strip_prefix('>') {
        Some(end) => Some((Some((name.to_owned(), value.to_owned())), read(end))),
        None => Some((None, read(after))),
    }
}

fn parse_error(contexts: &[&str], message: &str, position: &str) -> XmlError {
    let contexts: Vec<String> = contexts.iter().map(|c| show_string(c)).collect();
    XmlError(format!(
        "ParseError {{errorContexts = [{}], errorMessage = {}, errorPosition = {position}}}",
        contexts.join(","),
        show_string(message)
    ))
}

fn char_reference(number: &str) -> Option<char> {
    let value = match number.strip_prefix('x') {
        Some(hex) if !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()) => {
            u32::from_str_radix(hex, 16).ok()?
        }
        Some(_) => return None,
        None if !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) => {
            number.parse().ok()?
        }
        None => return None,
    };
    // The characters that XML allows (production `Char`).
    char::from_u32(value).filter(|&c| {
        matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
    })
}

/// Why the attributes of a tag do not parse.
enum ScanError {
    /// The attribute at this offset does not parse.
    At(usize),
    /// The input ends inside the tag.
    Eof,
}

/// Splits the attributes of a tag into (offset, name, raw value). With
/// `bounded`, `raw` is the text between the name and the end of the tag. Else
/// `raw` runs to the end of the input, and a quoted value can contain `>`, as
/// in the attoparsec grammar of `xml-conduit`.
fn scan_attributes(raw: &str, bounded: bool) -> Result<Vec<(usize, &str, &str)>, ScanError> {
    let bytes = raw.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let skip = |mut i: usize| {
        while bytes.get(i).is_some_and(|b| b.is_ascii_whitespace()) {
            i += 1;
        }
        i
    };
    loop {
        i = skip(i);
        match bytes.get(i) {
            None if bounded => return Ok(out),
            None => return Err(ScanError::Eof),
            Some(b'>' | b'/') if !bounded => return Ok(out),
            Some(_) => {}
        }
        let at = i;
        while bytes
            .get(i)
            .is_some_and(|&b| b >= 0x80 || is_name_char(char::from(b)))
        {
            i += 1;
        }
        let name = &raw[at..i];
        i = skip(i);
        if name.is_empty() {
            return Err(ScanError::At(at));
        }
        match bytes.get(i) {
            Some(b'=') => {}
            None if !bounded => return Err(ScanError::Eof),
            _ => return Err(ScanError::At(at)),
        }
        i = skip(i + 1);
        let quote = match bytes.get(i) {
            Some(&q @ (b'"' | b'\'')) => q,
            None if !bounded => return Err(ScanError::Eof),
            _ => return Err(ScanError::At(at)),
        };
        let value_start = i + 1;
        let Some(length) = bytes
            .get(value_start..)
            .and_then(|rest| rest.iter().position(|&b| b == quote))
        else {
            return Err(ScanError::At(at));
        };
        out.push((at, name, &raw[value_start..value_start + length]));
        i = value_start + length + 1;
        match bytes.get(i) {
            Some(b) if b.is_ascii_whitespace() => {}
            Some(b'>' | b'/') if !bounded => {}
            None => {}
            Some(_) => return Err(ScanError::At(at)),
        }
    }
}

fn offset(position: u64) -> usize {
    usize::try_from(position).unwrap_or(usize::MAX)
}

/// The `xml-conduit` position of a byte offset: `LINE:COLUMN (OFFSET)`,
/// counted in characters.
fn position(input: &str, byte: usize) -> String {
    let prefix = input.get(..byte.min(input.len())).unwrap_or(input);
    let line = 1 + prefix.matches('\n').count();
    let line_start = prefix.rfind('\n').map_or(0, |i| i + 1);
    let column = 1 + prefix[line_start..].chars().count();
    format!("{line}:{column} ({})", prefix.chars().count())
}

fn show_name(name: &Name) -> String {
    let maybe = |value: &Option<String>| match value {
        Some(text) => format!("Just {}", show_string(text)),
        None => "Nothing".to_owned(),
    };
    format!(
        "Name {{nameLocalName = {}, nameNamespace = {}, namePrefix = {}}}",
        show_string(&name.local),
        maybe(&name.namespace),
        maybe(&name.prefix)
    )
}

fn show_event(event: &ShownEvent) -> String {
    match event {
        ShownEvent::Begin(name, attributes) => {
            let attributes: Vec<String> = attributes
                .iter()
                .map(|(name, value)| {
                    format!("({},[ContentText {}])", show_name(name), show_string(value))
                })
                .collect();
            format!(
                "EventBeginElement ({}) [{}]",
                show_name(name),
                attributes.join(",")
            )
        }
        ShownEvent::End(name) => format!("EventEndElement ({})", show_name(name)),
        ShownEvent::Content(text) => format!("EventContent (ContentText {})", show_string(text)),
    }
}

/// The Haskell `show` of a string.
pub(crate) fn show_string(text: &str) -> String {
    const NAMES: [&str; 32] = [
        "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "a", "b", "t", "n", "v", "f", "r", "SO",
        "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC",
        "FS", "GS", "RS", "US",
    ];
    let mut out = String::from("\"");
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let next = chars.peek().copied();
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{7f}' => out.push_str("\\DEL"),
            ' '..='~' => out.push(c),
            c if (c as u32) < 32 => {
                out.push('\\');
                out.push_str(NAMES.get(c as usize).copied().unwrap_or(""));
                if c == '\u{e}' && next == Some('H') {
                    out.push_str("\\&");
                }
            }
            c => {
                out.push_str(&format!("\\{}", c as u32));
                if next.is_some_and(|n| n.is_ascii_digit()) {
                    out.push_str("\\&");
                }
            }
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Result<XmlElement, String> {
        parse_document(input, 128).map_err(|e| e.0)
    }

    #[test]
    fn the_tree_keeps_text_children_and_attributes_by_local_name() {
        let root = parse("<a xmlns='u' xml:lang='de' b='1'>x<c/>y&amp;z<![CDATA[<w>]]></a>")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(root.name.namespace.as_deref(), Some("u"));
        assert_eq!(root.attr("lang"), Some("de"));
        assert_eq!(root.attr("b"), Some("1"));
        assert_eq!(root.attr("xmlns"), None);
        assert_eq!(root.text(), "xy&z<w>");
        assert_eq!(root.elements().count(), 1);
        assert_eq!(root.children_named("c").count(), 1);
    }

    #[test]
    fn a_namespaced_attribute_beats_a_plain_one_and_the_last_duplicate_wins() {
        let root =
            parse("<a xml:lang='x' lang='y' b='1' b='2'/>").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(root.attr("lang"), Some("x"));
        assert_eq!(root.attr("b"), Some("2"));
        let root = parse("<a lang='y' xml:lang='x'/>").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(root.attr("lang"), Some("x"));
    }

    #[test]
    fn entities_of_the_internal_subset_expand() {
        let root = parse(
            "<!DOCTYPE a [<!ENTITY e \"vv\"><!ENTITY % p \"q\">]><a t='&e;&#65;&#x42;'>&e;</a>",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(root.attr("t"), Some("vvAB"));
        assert_eq!(root.text(), "vv");
    }

    #[test]
    fn errors_have_the_texts_of_xml_conduit() {
        assert_eq!(parse(""), Err("Missing root element".to_owned()));
        assert_eq!(parse("<!-- c -->"), Err("Missing root element".to_owned()));
        assert_eq!(
            parse("hello"),
            Err("1:1 (0)-1:6 (5): Found content after root element: EventContent (ContentText \"hello\")".to_owned())
        );
        assert_eq!(
            parse("<a/><b c='1'/>"),
            Err("1:5 (4)-1:15 (14): Found content after root element: EventBeginElement (Name {nameLocalName = \"b\", nameNamespace = Nothing, namePrefix = Nothing}) [(Name {nameLocalName = \"c\", nameNamespace = Nothing, namePrefix = Nothing},[ContentText \"1\"])]".to_owned())
        );
        assert_eq!(
            parse("<a>"),
            Err("Expected end element for: Name {nameLocalName = \"a\", nameNamespace = Nothing, namePrefix = Nothing}, but received: EventEndDocument".to_owned())
        );
        assert_eq!(
            parse("<a t='&e;'/>"),
            Err("UnresolvedEntityException (fromList [\"e\"])".to_owned())
        );
        assert_eq!(
            parse("<a>&e;</a>"),
            Err("UnresolvedEntityException (fromList [\"e\"])".to_owned())
        );
        assert_eq!(
            parse("<a b=c>"),
            Err("ParseError {errorContexts = [\"open tag\",\"'>'\"], errorMessage = \"Failed reading: satisfy\", errorPosition = 1:4 (3)}".to_owned())
        );
        assert_eq!(
            parse("<a"),
            Err("ParseError {errorContexts = [\"open tag\",\"'>'\"], errorMessage = \"not enough input\", errorPosition = 1:3 (2)}".to_owned())
        );
        assert_eq!(
            parse("<a/><!-- x"),
            Err("ParseError {errorContexts = [\"DOCTYPE\"], errorMessage = \"string\", errorPosition = 1:7 (6)}".to_owned())
        );
    }

    #[test]
    fn attribute_values_reject_lt_bare_amp_and_bad_char_refs() {
        for value in [
            "a<b",
            "a & b",
            "&#0;",
            "&#xD800;",
            "&#x110000;",
            "&#xZ;",
            "&;",
            "&#;",
        ] {
            let input = format!("<a x='1' b='{value}'/>");
            assert_eq!(
                parse(&input),
                Err("ParseError {errorContexts = [\"open tag\",\"'>'\"], errorMessage = \"Failed reading: satisfy\", errorPosition = 1:10 (9)}".to_owned()),
                "{value}"
            );
        }
    }

    #[test]
    fn positions_count_characters_and_lines() {
        assert_eq!(position("ab\ncé\nx", 7), "3:1 (6)");
        assert_eq!(position("é", 2), "1:2 (1)");
    }

    #[test]
    fn show_string_escapes_as_haskell_does() {
        assert_eq!(show_string("a\"b\\"), "\"a\\\"b\\\\\"");
        assert_eq!(show_string("é1é"), "\"\\233\\&1\\233\"");
        assert_eq!(show_string("\n\u{e}H\u{7f}"), "\"\\n\\SO\\&H\\DEL\"");
    }

    fn steps_of(input: &str) -> usize {
        SUBSET_STEPS.with(|steps| steps.set(0));
        let _ = parse(input);
        SUBSET_STEPS.with(std::cell::Cell::get)
    }

    #[test]
    fn the_internal_subset_reads_in_linear_steps() {
        // Review finding citeproc-R1-01: overlapping declarations.
        const BODY: &str = "<style><citation><layout/></citation></style>";
        let overlap: fn(usize) -> String = |n| {
            format!(
                "<!DOCTYPE s [{}{}]>{BODY}",
                "<!ENTITY".repeat(n),
                " x>".repeat(n)
            )
        };
        let many: fn(usize) -> String = |n| {
            let decls: String = (0..n)
                .map(|i| format!("<!ENTITY e{i} 'v'><!-- c --><!ELEMENT a ANY>t"))
                .collect();
            format!("<!DOCTYPE s [{decls}]>{BODY}")
        };
        for make in [overlap, many] {
            let (small, large) = (make(1_000), make(4_000));
            let (a, b) = (steps_of(&small), steps_of(&large));
            assert!(a <= 3 * small.len() && b <= 3 * large.len(), "{a} {b}");
        }
    }

    #[test]
    fn entities_with_markup_go_on_the_element_stack() {
        // Oracle: each style renders the text in the comment (replay cases `entity-*`).
        let root =
            parse("<!DOCTYPE s [<!ENTITY f '<c v=\"q\"/>'><!ENTITY e \"&f;<d/>\">]><a>&e;</a>")
                .unwrap_or_else(|e| panic!("{e}"));
        let names: Vec<&str> = root.elements().map(|e| e.name.local.as_str()).collect();
        assert_eq!(names, ["c", "d"]);
        assert_eq!(root.elements().next().and_then(|c| c.attr("v")), Some("q"));
        let root = parse("<!DOCTYPE s [<!ENTITY a 'x'><!ENTITY b '&a;y&#65;&amp;'>]><a t='&b;'/>")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(root.attr("t"), Some("xyA&"));
        assert_eq!(
            parse("<!DOCTYPE s [<!ENTITY f 'y'><!ENTITY e '<c v=\"&f;\"/>'>]><a>&e;</a>"),
            Err("UnresolvedEntityException (fromList [\"f\"])".to_owned())
        );
        assert_eq!(
            parse("<!DOCTYPE s [<!ENTITY a '&b;'><!ENTITY b '&a;'>]><a>&a;</a>"),
            Err("UnresolvedEntityException (fromList [\"a\"])".to_owned())
        );
        assert_eq!(
            parse("<!DOCTYPE s [<!-- <!ENTITY e 'x'> -->]><a>&e;</a>"),
            Err("UnresolvedEntityException (fromList [\"e\"])".to_owned())
        );
    }

    #[test]
    fn nesting_stops_at_the_limit() {
        let deep = format!("{}{}", "<g>".repeat(5), "</g>".repeat(5));
        assert!(parse_document(&deep, 5).is_ok());
        let error = parse_document(&deep, 4)
            .err()
            .map(|e| e.0)
            .unwrap_or_default();
        assert!(error.contains("nested deeper than 4 levels"), "{error}");
    }
}
