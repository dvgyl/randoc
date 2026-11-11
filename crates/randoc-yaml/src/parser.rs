//! The syntax layer: YAML text to a tree of raw nodes with unresolved aliases.
//!
//! The parser reads block mappings and sequences, plain, quoted and block
//! scalars, flow collections, comments, anchors, aliases, tags and document
//! markers. It gives the error texts of Pandoc 3.13.

use std::collections::HashMap;

use crate::error::{LineIndex, YamlError};
use crate::{Limits, Style};

/// A node before alias expansion. `pos` is a byte position in the text.
pub(crate) struct Raw {
    pub(crate) pos: usize,
    pub(crate) tag: Option<String>,
    /// The anchor numbers of the node in the document.
    pub(crate) anchors: Vec<usize>,
    pub(crate) kind: RawKind,
}

pub(crate) enum RawKind {
    Scalar {
        text: String,
        style: Style,
    },
    Sequence(Vec<Raw>),
    Mapping(Vec<(Raw, Raw)>),
    /// An alias and the anchor number that its name refers to at its place.
    Alias {
        name: String,
        target: Option<usize>,
    },
}

/// One document of the stream.
pub(crate) struct RawDocument {
    pub(crate) root: Raw,
    /// The number of aliases that refer to each anchor.
    pub(crate) references: Vec<usize>,
}

type Result<T> = std::result::Result<T, YamlError>;

/// Reads each document of the text.
pub(crate) fn parse(text: &str, lines: &LineIndex<'_>, limits: Limits) -> Result<Vec<RawDocument>> {
    let mut documents = Vec::new();
    let mut pos = 0;
    let mut explicit = false;
    loop {
        let mut handles = Handles::new();
        if !explicit && first_content(text, pos).starts_with('%') {
            (pos, handles) = directives(text, pos, lines)?;
            explicit = true;
        }
        // Find the end of this document: the next line with a marker.
        let mut end = pos;
        let mut next = None;
        let mut first_line = true;
        while end < text.len() {
            let rest = text.get(end..).unwrap_or("");
            let line_len = rest.find('\n').unwrap_or(rest.len());
            crate::steps::add(line_len + 1);
            let marker = marker(rest);
            if marker.is_some() && !(first_line && explicit) {
                next = marker.map(|m| (end, m));
                break;
            }
            first_line = false;
            end = end
                .saturating_add(line_len)
                .saturating_add(1)
                .min(text.len());
        }
        let mut parser = Parser::new(text.get(..end).unwrap_or(""), lines, pos, limits, explicit);
        parser.handles = handles;
        let root = parser.document(explicit)?;
        if let Some(root) = root {
            documents.push(RawDocument {
                root,
                references: std::mem::take(&mut parser.references),
            });
        }
        match next {
            None => return Ok(documents),
            Some((at, Marker::Start)) => {
                pos = at.saturating_add(3);
                explicit = true;
            }
            Some((at, Marker::End)) => {
                let rest = text.get(at..).unwrap_or("");
                let line_len = rest.find('\n').unwrap_or(rest.len());
                crate::steps::add(line_len + 1);
                let mut after = Parser::new(
                    text.get(..at.saturating_add(line_len)).unwrap_or(""),
                    lines,
                    at.saturating_add(3),
                    limits,
                    false,
                );
                if let Err(at) = after.finish_line() {
                    return Err(
                        after.error(at, "unexpected content after the document end marker (...)")
                    );
                }
                pos = at
                    .saturating_add(line_len)
                    .saturating_add(1)
                    .min(text.len());
                explicit = false;
            }
        }
    }
}

/// Reads the directive lines at `pos` and the `---` marker after them.
/// Returns the position after the marker and the tag handles.
fn directives(text: &str, mut pos: usize, lines: &LineIndex<'_>) -> Result<(usize, Handles)> {
    let mut handles = Handles::new();
    let mut version = false;
    loop {
        let rest = text.get(pos..).unwrap_or("");
        let line_len = rest.find('\n').unwrap_or(rest.len());
        crate::steps::add(line_len + 1);
        let line = rest.get(..line_len).unwrap_or("");
        if line.starts_with('%') {
            // A comment starts at a `#` after a space or a tab.
            let comment = line
                .char_indices()
                .find(|(i, c)| {
                    *c == '#' && line.get(..*i).is_some_and(|b| b.ends_with([' ', '\t']))
                })
                .map_or(line.len(), |(i, _)| i);
            let content = line.get(..comment).unwrap_or(line);
            let mut words = content.split([' ', '\t']).filter(|w| !w.is_empty());
            let end = pos + content.trim_end().len();
            match words.next() {
                Some("%YAML") => {
                    if version {
                        return Err(lines.error(pos, "duplicate %YAML directive"));
                    }
                    version = true;
                    let Some(number) = words.next() else {
                        return Err(lines.error(end, "expected a version such as 1.2 after %YAML"));
                    };
                    let at = pos + content.find(number).unwrap_or(0);
                    let valid = number.split_once('.').is_some_and(|(a, b)| {
                        !a.is_empty()
                            && !b.is_empty()
                            && a.bytes().chain(b.bytes()).all(|c| c.is_ascii_digit())
                    });
                    if !valid {
                        return Err(lines.error(at, "expected a version such as 1.2 after %YAML"));
                    }
                    if let Some(extra) = words.next() {
                        let at = pos + content.rfind(extra).unwrap_or(0);
                        return Err(lines.error(at, "unexpected text after the directive"));
                    }
                    let major = number
                        .split('.')
                        .next()
                        .unwrap_or("")
                        .trim_start_matches('0');
                    if major != "1" {
                        return Err(lines.error(pos, format!("unsupported YAML version {number}")));
                    }
                }
                Some("%TAG") => {
                    let Some(handle) = words.next() else {
                        return Err(lines.error(end, "expected a tag handle after %TAG"));
                    };
                    let Some(prefix) = words.next() else {
                        return Err(lines.error(
                            end,
                            "expected a prefix after the tag handle, e.g. tag:example.com,2000:",
                        ));
                    };
                    let inner = handle.strip_prefix('!').and_then(|h| h.strip_suffix('!'));
                    let valid_handle = handle == "!"
                        || inner.is_some_and(|h| {
                            h.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                        });
                    if !valid_handle {
                        let at = pos + content.find(handle).unwrap_or(0);
                        return Err(lines.error(at, "invalid tag handle"));
                    }
                    if decode_tag(prefix).is_none() {
                        let at = pos + content.find(prefix).unwrap_or(0);
                        return Err(lines.error(
                            at,
                            "invalid escape in the tag, write '%' and two hexadecimal digits",
                        ));
                    }
                    if let Some(extra) = words.next() {
                        let at = pos + content.rfind(extra).unwrap_or(0);
                        return Err(lines.error(at, "unexpected text after the directive"));
                    }
                    if handles
                        .insert(handle.to_string(), prefix.to_string())
                        .is_some()
                    {
                        return Err(
                            lines.error(pos, format!("duplicate %TAG directive for {handle}"))
                        );
                    }
                }
                _ => {}
            }
        } else if !{
            let trimmed = line.trim_start_matches([' ', '\t']);
            trimmed.is_empty() || trimmed.starts_with('#')
        } {
            if matches!(marker(rest), Some(Marker::Start)) {
                return Ok((pos + 3, handles));
            }
            return Err(lines.error(
                pos,
                "expected a document start marker (---) after the directives",
            ));
        }
        if line_len == rest.len() {
            return Err(lines.error(
                text.len(),
                "expected a document start marker (---) after the directives",
            ));
        }
        pos += line_len + 1;
    }
}

/// Returns the text from the first line at `pos` that is not blank and not a
/// comment.
fn first_content(text: &str, mut pos: usize) -> &str {
    loop {
        let rest = text.get(pos..).unwrap_or("");
        let line_len = rest.find('\n').unwrap_or(rest.len());
        crate::steps::add(line_len + 1);
        let line = rest
            .get(..line_len)
            .unwrap_or("")
            .trim_start_matches([' ', '\t']);
        if !(line.is_empty() || line.starts_with('#')) || line_len == rest.len() {
            return rest;
        }
        pos += line_len + 1;
    }
}

/// The tag handles of a document, from its `%TAG` directives.
type Handles = HashMap<String, String>;

/// The prefix of the tags of the core schema.
const CORE_PREFIX: &str = "tag:yaml.org,2002:";

/// Decodes the `%XX` escapes of a tag. Returns `None` for a bad escape and
/// `Some(Err(()))` for escapes that are not UTF-8.
fn decode_tag(text: &str) -> Option<std::result::Result<String, ()>> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&b, tail)) = rest.split_first() {
        if b == b'%' {
            let hex = tail.get(..2)?;
            let value = std::str::from_utf8(hex)
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())?;
            bytes.push(value);
            rest = tail.get(2..).unwrap_or(&[]);
        } else {
            bytes.push(b);
            rest = tail;
        }
    }
    Some(String::from_utf8(bytes).map_err(|_| ()))
}

/// Writes a full tag in its short form: `!!name` for the core schema, `!name`
/// for a local tag, else `!<uri>`. A local tag that starts with `!!` keeps
/// the verbatim form `!<!!name>`, so that it is not a core tag.
fn short_tag(full: &str) -> String {
    if let Some(name) = full.strip_prefix(CORE_PREFIX) {
        format!("!!{name}")
    } else if full.starts_with('!') && !full.starts_with("!!") {
        full.to_string()
    } else {
        format!("!<{full}>")
    }
}

#[derive(Clone, Copy)]
enum Marker {
    Start,
    End,
}

fn marker(line: &str) -> Option<Marker> {
    let kind = if line.starts_with("---") {
        Marker::Start
    } else if line.starts_with("...") {
        Marker::End
    } else {
        return None;
    };
    match line.as_bytes().get(3) {
        None | Some(b' ' | b'\t' | b'\n') => Some(kind),
        _ => None,
    }
}

/// A scalar before tag resolution.
struct Scalar {
    text: String,
    style: Style,
    pos: usize,
}

/// A parsed node before its properties apply.
enum Parsed {
    Scalar(Scalar),
    Node(RawKind, usize),
    /// A complete node from a later line.
    Below(Raw),
}

/// The anchor and the tag in front of a node.
#[derive(Default)]
struct Props {
    anchor: Option<(String, usize)>,
    tag: Option<String>,
}

impl Props {
    /// Adds the properties of a later line. Returns `false` when both have
    /// an anchor or both have a tag.
    fn add(&mut self, more: Props) -> bool {
        if (self.anchor.is_some() && more.anchor.is_some())
            || (self.tag.is_some() && more.tag.is_some())
        {
            return false;
        }
        self.anchor = self.anchor.take().or(more.anchor);
        self.tag = self.tag.take().or(more.tag);
        true
    }

    fn is_empty(&self) -> bool {
        self.anchor.is_none() && self.tag.is_none()
    }
}

/// The reason that a plain scalar stops on its line.
#[derive(Clone, Copy, PartialEq)]
enum Stop {
    Eol,
    Comment,
    Colon(usize),
    Flow,
}

/// A part of the path of a node, for error messages.
pub(crate) enum Segment {
    Key(String),
    Index(usize),
}

/// Writes a message with the path of its node in front, as Pandoc does:
/// `a.b[0]: message`.
pub(crate) fn with_path(path: &[Segment], message: &str) -> String {
    let mut out = String::new();
    for part in path {
        match part {
            Segment::Key(key) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(key);
            }
            Segment::Index(i) => out.push_str(&format!("[{i}]")),
        }
    }
    if out.is_empty() {
        message.to_string()
    } else {
        format!("{out}: {message}")
    }
}

/// Writes a key in double quotes, with the escapes that Pandoc uses.
pub(crate) fn quote_key(key: &str) -> String {
    let mut out = String::from("\"");
    for c in key.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// A character of a URI in a tag.
fn is_uri_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-#;/?:@&=+$,_.!~*'()[]%".contains(c)
}

/// The scheme of a URI (RFC 3986): a letter, then letters, digits, `+`, `-`
/// or `.`.
fn is_uri_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
}

/// A character of a tag after its handle: a URI character, but not `!` and
/// not a flow indicator.
fn is_tag_char(c: char) -> bool {
    is_uri_char(c) && c != '!' && !is_flow_indicator(c)
}

/// Writes a character in single quotes, with the escapes that Pandoc uses.
fn shown(c: char) -> String {
    match c {
        '\'' => "'\\''".to_string(),
        '\\' => "'\\\\'".to_string(),
        '\t' => "'\\t'".to_string(),
        c => format!("'{c}'"),
    }
}

const CONTINUED_SCALAR: &str = "unexpected ':', this line continues the scalar from the line above, check the indentation and the line above";

/// The index of the first `:` of a key text that a character that starts a
/// value follows: the author forgot the space after `:`.
fn colon_without_space(text: &str) -> Option<usize> {
    text.match_indices(':').map(|(i, _)| i).find(|&i| {
        text.get(i + 1..)
            .and_then(|t| t.chars().next())
            .is_some_and(|after| after.is_alphanumeric() || "[{\"'".contains(after))
    })
}

fn is_flow_indicator(c: char) -> bool {
    matches!(c, ',' | '[' | ']' | '{' | '}')
}

fn key_kind_message(open: char) -> &'static str {
    if open == '[' {
        "expected a scalar key, but got a list"
    } else {
        "expected a scalar key, but got a mapping"
    }
}

struct Parser<'a, 'b> {
    text: &'a str,
    lines: &'b LineIndex<'a>,
    pos: usize,
    max_depth: usize,
    anchors: HashMap<String, usize>,
    /// The number of aliases that refer to each anchor.
    references: Vec<usize>,
    path: Vec<Segment>,
    /// The properties in front of a node, for its first key when the node is
    /// a block mapping.
    key_props: Option<Props>,
    /// The smallest column of a continuation line of a quoted scalar.
    quote_indent: usize,
    /// The first line in a flow collection with too little indentation. It
    /// is an error when the collection ends.
    flow_violation: Option<YamlError>,
    /// The first bad continuation line of the current quoted scalar.
    quote_error: Option<YamlError>,
    /// `true` after an explicit key without a `:` line.
    empty_value: bool,
    /// `true` after the `:` line of an explicit key.
    explicit_value: bool,
    /// `true` in a document that starts with `---`.
    explicit: bool,
    /// The smallest indentation of a line in the current flow collection.
    flow_indent: usize,
    /// The open flow collections, as their opening characters.
    flow_kinds: Vec<char>,
    /// The tag handles of the document.
    handles: Handles,
}

fn empty_scalar(pos: usize) -> Parsed {
    Parsed::Scalar(Scalar {
        text: String::new(),
        style: Style::Plain,
        pos,
    })
}

impl<'a, 'b> Parser<'a, 'b> {
    fn new(
        text: &'a str,
        lines: &'b LineIndex<'a>,
        pos: usize,
        limits: Limits,
        explicit: bool,
    ) -> Self {
        Parser {
            text,
            lines,
            pos,
            max_depth: limits.max_depth,
            anchors: HashMap::new(),
            references: Vec::new(),
            path: Vec::new(),
            key_props: None,
            empty_value: false,
            quote_error: None,
            explicit_value: false,
            quote_indent: 0,
            flow_violation: None,
            explicit,
            flow_indent: 0,
            flow_kinds: Vec::new(),
            handles: Handles::new(),
        }
    }

    fn rest(&self) -> &'a str {
        self.text.get(self.pos..).unwrap_or("")
    }

    fn slice(&self, start: usize, end: usize) -> &'a str {
        self.text.get(start..end).unwrap_or("")
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        self.rest().chars().nth(1)
    }

    fn bump(&mut self) -> Option<char> {
        crate::steps::add(1);
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn eof(&self) -> bool {
        self.pos >= self.text.len()
    }

    fn at_comment_or_eol(&self) -> bool {
        matches!(self.peek(), None | Some('\n' | '#'))
    }

    /// Returns `true` when the next text is a `-` list item indicator.
    fn at_dash_item(&self) -> bool {
        self.peek() == Some('-') && matches!(self.peek_second(), None | Some(' ' | '\t' | '\n'))
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            crate::steps::add(1);
            self.pos += 1;
        }
    }

    fn skip_to_eol(&mut self) {
        let len = self.rest().find('\n').unwrap_or(self.rest().len());
        crate::steps::add(len);
        self.pos += len;
    }

    /// Moves past lines that hold only spaces or a comment, and stops at the
    /// start of the next line with content.
    fn skip_blank_lines(&mut self) {
        loop {
            let start = self.pos;
            self.skip_spaces();
            if self.peek() == Some('#') {
                self.skip_to_eol();
            }
            match self.peek() {
                Some('\n') => self.pos += 1,
                None => return,
                Some(_) => {
                    self.pos = start;
                    return;
                }
            }
        }
    }

    /// Moves past spaces, newlines and comments in a flow collection.
    fn skip_flow_space(&mut self) -> Result<()> {
        loop {
            match self.peek() {
                Some(' ' | '\t') => self.pos += 1,
                Some('\n') => {
                    self.pos += 1;
                    self.check_flow_line()?;
                }
                Some('#') if self.after_space() => self.skip_to_eol(),
                _ => return Ok(()),
            }
        }
    }

    /// Checks the indentation of a new line in a flow collection.
    fn check_flow_line(&mut self) -> Result<()> {
        if self.flow_violation.is_none() {
            self.flow_violation = self.flow_line_error().err();
        }
        Ok(())
    }

    fn flow_line_error(&self) -> Result<()> {
        let n = self.rest().bytes().take_while(|b| *b == b' ').count();
        if n >= self.flow_indent {
            return Ok(());
        }
        let at = self.pos + n;
        match self.rest().as_bytes().get(n) {
            None | Some(b'\n' | b'#') => Ok(()),
            Some(b'\t') => Err(self.error(at, "tabs cannot be used for indentation")),
            Some(&c) => {
                let (kind, close) = if self.flow_kinds.last() == Some(&'[') {
                    ("sequence", b']')
                } else {
                    ("mapping", b'}')
                };
                let message = if c == close {
                    format!(
                        "'{}' is indented too little to end the flow {kind}",
                        char::from(c)
                    )
                } else {
                    format!("the line is indented too little to continue the flow {kind}")
                };
                Err(self.error(at, message))
            }
        }
    }

    /// Returns `true` when the character before the position is a space or a
    /// line break, or when the position is the start of the text.
    fn after_space(&self) -> bool {
        self.text
            .get(..self.pos)
            .and_then(|before| before.chars().next_back())
            .is_none_or(|c| matches!(c, ' ' | '\t' | '\n'))
    }

    /// Returns the number of spaces at the position. The position must be at
    /// the start of a line with content. A tab in the indentation is an error.
    fn indent_here(&self) -> Result<usize> {
        let n = self.rest().bytes().take_while(|b| *b == b' ').count();
        if self.rest().as_bytes().get(n) == Some(&b'\t') {
            return Err(self.error(self.pos + n, "tabs cannot be used for indentation"));
        }
        Ok(n)
    }

    fn column(&self, pos: usize) -> usize {
        self.lines.mark(pos).column.saturating_sub(1)
    }

    fn error(&self, pos: usize, message: impl Into<String>) -> YamlError {
        self.lines.error(pos, message)
    }

    /// Makes an error whose message starts with the path of the node.
    fn path_error(&self, pos: usize, message: &str) -> YamlError {
        self.error(pos, with_path(&self.path, message))
    }

    fn check_depth(&self, depth: usize) -> Result<()> {
        if depth > self.max_depth {
            Err(self.error(self.pos, "the YAML metadata is nested too deeply"))
        } else {
            Ok(())
        }
    }

    /// Moves past spaces, a comment and the line break at the end of a node.
    /// Returns the position of other text when there is some.
    fn finish_line(&mut self) -> std::result::Result<(), usize> {
        self.skip_spaces();
        if self.peek() == Some('#') && self.after_space() {
            self.skip_to_eol();
        }
        match self.peek() {
            None => Ok(()),
            Some('\n') => {
                self.pos += 1;
                Ok(())
            }
            Some(_) => Err(self.pos),
        }
    }

    fn unexpected_here(&self) -> YamlError {
        match self.peek() {
            Some(c) => self.error(self.pos, format!("unexpected {}", shown(c))),
            None => self.error(self.pos, "unexpected end of input"),
        }
    }

    /// Reads one document. `explicit` is `true` after a `---` marker, with
    /// the position after the marker. Returns `None` for a bare document
    /// with no content.
    fn document(&mut self, explicit: bool) -> Result<Option<Raw>> {
        if explicit {
            self.skip_spaces();
            if !self.at_comment_or_eol() {
                let value = self.block_node(-1, Context::Marker, 0)?;
                return self.end_of_document(value).map(Some);
            }
        }
        let after_marker = self.pos;
        if explicit {
            let _ = self.finish_line();
        }
        self.skip_blank_lines();
        if self.eof() {
            return Ok(explicit.then(|| scalar_raw(empty_scalar_at(after_marker))));
        }
        let n = self.indent_here()?;
        self.pos += n;
        let value = self.block_node(-1, Context::Block, 0)?;
        self.end_of_document(value).map(Some)
    }

    fn end_of_document(&mut self, value: Raw) -> Result<Raw> {
        self.skip_blank_lines();
        if self.eof() {
            Ok(value)
        } else {
            let n = self.indent_here()?;
            self.pos += n;
            if n == 0 && self.at_dash_item() {
                Err(self.error(self.pos, "unexpected list item among mapping entries"))
            } else if n == 0 {
                Err(self.unexpected_here())
            } else {
                Err(self.error(self.pos, "unexpected indentation"))
            }
        }
    }

    /// Reads a node in block context. The position is at its first character.
    /// `parent` is the indentation of the parent collection, or -1.
    fn block_node(&mut self, parent: isize, context: Context, depth: usize) -> Result<Raw> {
        self.check_depth(depth)?;
        let node_col = self.column(self.pos);
        let mut props = self.properties(None)?;
        let parsed = if !props.is_empty() && self.at_comment_or_eol() {
            self.below_properties(&mut props, node_col, parent, context, depth)?
        } else {
            let col = if props.is_empty() {
                node_col
            } else {
                // Properties in front of a mapping belong to its first key.
                let content_col = self.column(self.pos);
                if self.line_is_key() {
                    node_col
                } else {
                    content_col
                }
            };
            self.key_props = Some(props);
            let parsed = self.block_content(col, parent, context, depth)?;
            let props = self.key_props.take().unwrap_or_default();
            return Ok(self.finish_node(parsed, props));
        };
        Ok(self.finish_node(parsed, props))
    }

    /// Reads the node below properties that end their line. More lines with
    /// only properties add to them: a node has at most one anchor and one
    /// tag. Properties in front of a mapping key belong to that key.
    fn below_properties(
        &mut self,
        props: &mut Props,
        mut props_col: usize,
        parent: isize,
        context: Context,
        depth: usize,
    ) -> Result<Parsed> {
        let here = self.pos;
        let mut lowest_col = props_col;
        loop {
            let _ = self.finish_line();
            let line = self.pos;
            self.skip_blank_lines();
            self.skip_spaces();
            let at = self.pos;
            let more = matches!(self.peek(), Some('&' | '!'));
            if more && context == Context::AfterKey {
                let end = at + self.rest().find('\n').unwrap_or(self.rest().len());
                return Err(self.error(end, "unexpected end of line"));
            }
            if !more || self.column(at) as isize <= parent || self.key_after_properties() {
                if self.peek() == Some('*')
                    && self.column(at) as isize > parent
                    && !self.line_is_key()
                {
                    let end = at + self.rest().find('\n').unwrap_or(self.rest().len());
                    self.bump();
                    self.take_name();
                    let name_end = self.pos;
                    self.skip_spaces();
                    let col = self.column(at);
                    let key =
                        context != Context::AfterKey && (col == props_col || col == lowest_col);
                    return Err(match self.peek() {
                        Some('#') if key => self.error(name_end, "expected ':' after the key"),
                        None | Some('\n') if key => self.error(end, "expected ':' after the key"),
                        None | Some('\n') => self.error(end, "unexpected end of line"),
                        Some(_) => self.unexpected_here(),
                    });
                }
                self.pos = line;
                break;
            }
            let added = self.properties(None)?;
            if !props.add(added) {
                if self.at_comment_or_eol() {
                    return Err(self.error(
                        at,
                        "an anchor or a tag cannot be on a line of its own here, write it after the key or the '-'",
                    ));
                }
                let end = self.pos + self.rest().find('\n').unwrap_or(self.rest().len());
                // The text depends on the lines of the properties above.
                let col = self.column(at);
                let message = if col == props_col || col == lowest_col {
                    "expected ':' after the key"
                } else {
                    "unexpected end of line"
                };
                return Err(self.error(end, message));
            }
            props_col = self.column(at);
            lowest_col = lowest_col.min(props_col);
            if !self.at_comment_or_eol() {
                let col = self.column(self.pos);
                self.key_props = None;
                return self.block_content(col, parent, Context::Block, depth);
            }
        }
        let same_level_list = matches!(context, Context::AfterKey | Context::BelowKey);
        Ok(match self.parsed_below(parent, same_level_list, depth)? {
            // `parsed_below` gives a scalar only for an empty value.
            Parsed::Scalar(mut s) => {
                s.pos = here;
                Parsed::Scalar(s)
            }
            parsed => parsed,
        })
    }

    /// Returns `true` when the line is a mapping key after the properties at
    /// the position.
    fn key_after_properties(&mut self) -> bool {
        let start = self.pos;
        while matches!(self.peek(), Some('&' | '!')) {
            let word = self
                .rest()
                .find([' ', '\t', '\n'])
                .unwrap_or(self.rest().len());
            self.pos += word;
            self.skip_spaces();
        }
        let key = !self.at_comment_or_eol() && self.line_is_key();
        self.pos = start;
        key
    }

    /// Reads a node that starts on a later line, or gives an empty scalar.
    /// `same_level_list` permits a list at the indentation of `parent`.
    fn parsed_below(
        &mut self,
        parent: isize,
        same_level_list: bool,
        depth: usize,
    ) -> Result<Parsed> {
        self.skip_blank_lines();
        let empty = empty_scalar(self.pos);
        if self.eof() {
            return Ok(empty);
        }
        let spaces = self.rest().bytes().take_while(|b| *b == b' ').count();
        if spaces as isize > parent && self.rest().as_bytes().get(spaces) == Some(&b'\t') {
            // After enough spaces, a tab separates a scalar or a flow node,
            // but it cannot indent a block collection.
            let line_start = self.pos;
            self.pos += spaces;
            self.skip_spaces();
            if !(self.at_dash_item() || self.line_is_key() || self.at_comment_or_eol()) {
                return Ok(Parsed::Below(self.block_node(
                    parent,
                    Context::Block,
                    depth + 1,
                )?));
            }
            self.pos = line_start;
        }
        let n = self.indent_here()?;
        let start = self.pos;
        self.pos += n;
        if n as isize > parent {
            let context = if same_level_list {
                Context::BelowKey
            } else {
                Context::Block
            };
            return Ok(Parsed::Below(self.block_node(
                parent,
                context,
                depth + 1,
            )?));
        }
        if same_level_list && n as isize == parent && self.at_dash_item() {
            let pos = self.pos;
            let kind = self.block_sequence(n, true, depth + 1)?;
            return Ok(Parsed::Node(kind, pos));
        }
        self.pos = start;
        Ok(empty)
    }

    /// Reads the anchor and the tag in front of a node. In a flow collection,
    /// `flow` is the error text for a token that cannot follow the node.
    fn properties(&mut self, flow: Option<&'static str>) -> Result<Props> {
        let mut props = Props::default();
        loop {
            match self.peek() {
                Some('&') => {
                    let named = self
                        .rest()
                        .chars()
                        .nth(1)
                        .is_some_and(|c| !c.is_whitespace() && !is_flow_indicator(c));
                    if props.anchor.is_some() && named {
                        return Err(self.duplicate_property(flow));
                    }
                    self.bump();
                    let name = self.take_name();
                    if name.is_empty() {
                        let at = if flow.is_some() {
                            self.pos - 1
                        } else {
                            self.pos
                        };
                        return Err(self.error(at, "expected an anchor name after '&'"));
                    }
                    let id = self.references.len();
                    self.references.push(0);
                    self.anchors.insert(name.to_string(), id);
                    props.anchor = Some((name.to_string(), id));
                }
                Some('!') => {
                    if props.tag.is_some() {
                        return Err(self.duplicate_property(flow));
                    }
                    let verbatim = self.rest().starts_with("!<");
                    props.tag = Some(self.tag()?);
                    // A verbatim tag ends at '>', so a ':' right after it
                    // follows an empty node.
                    if verbatim && self.peek() == Some(':') {
                        let plain = self
                            .peek_second()
                            .is_some_and(|c| !c.is_whitespace() && !is_flow_indicator(c));
                        return match flow {
                            Some(expected) if plain => Err(self.error(self.pos, expected)),
                            _ => Ok(props),
                        };
                    }
                }
                _ => return Ok(props),
            }
            match self.peek() {
                None | Some(' ' | '\t' | '\n') => self.skip_spaces(),
                Some(',' | ']' | '}') if flow.is_some() => return Ok(props),
                Some(c @ ('|' | '>')) if flow.is_some() => {
                    return Err(self.error(
                        self.pos,
                        format!(
                            "unexpected {}, a block scalar cannot be inside a flow collection",
                            shown(c)
                        ),
                    ));
                }
                Some('`' | '@' | '%') => return Ok(props),
                Some(c) => {
                    return Err(match flow {
                        Some(_) if c == '*' => self.error(
                            self.pos,
                            "unexpected '*', an alias cannot have an anchor or a tag",
                        ),
                        Some(expected) => self.error(self.pos, expected),
                        None => self.unexpected_here(),
                    });
                }
            }
            if self.peek() == Some('*') {
                return Err(self.error(
                    self.pos,
                    "unexpected '*', an alias cannot have an anchor or a tag",
                ));
            }
        }
    }

    /// The error for a second anchor or a second tag of one node.
    fn duplicate_property(&self, flow: Option<&'static str>) -> YamlError {
        match flow {
            Some(expected) => self.error(self.pos, expected),
            None => self.unexpected_here(),
        }
    }

    /// Reads a tag and returns its short form (see `short_tag`). The
    /// position is at its `!`.
    fn tag(&mut self) -> Result<String> {
        let start = self.pos;
        self.bump();
        if self.peek() == Some('<') {
            let rest = self.rest();
            let Some(end) = rest.find('>') else {
                return Err(self.error(start, "invalid verbatim tag"));
            };
            let inner = rest.get(1..end).unwrap_or("");
            let valid = if inner.starts_with('!') {
                inner.chars().count() > 1
            } else {
                inner
                    .split_once(':')
                    .is_some_and(|(scheme, _)| is_uri_scheme(scheme))
            };
            if !valid || !inner.chars().all(|c| is_uri_char(c) || c == '!') {
                return Err(self.error(start, "invalid verbatim tag"));
            }
            self.pos += end + 1;
            return match decode_tag(inner) {
                None => Err(self.error(start, "invalid verbatim tag")),
                Some(Err(())) => {
                    Err(self.error(start, "the escapes of the tag are not valid UTF-8"))
                }
                Some(Ok(decoded)) => Ok(short_tag(&decoded)),
            };
        }
        let word_end = self
            .rest()
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .unwrap_or(self.rest().len());
        let handle = if self.rest().as_bytes().get(word_end) == Some(&b'!') {
            self.pos += word_end + 1;
            let handle = self.slice(start, self.pos);
            if !self.peek().is_some_and(is_tag_char) {
                return Err(self.error(
                    self.pos,
                    format!("expected the rest of the tag after {handle}"),
                ));
            }
            if word_end > 0 && !self.handles.contains_key(handle) {
                return Err(self.error(start, format!("undefined tag handle {handle}")));
            }
            handle
        } else {
            "!"
        };
        let suffix_start = self.pos;
        while let Some(c) = self.peek() {
            if c == '%' {
                let hex = self.rest().get(1..3).unwrap_or("");
                if hex.len() != 2 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(self.error(
                        self.pos,
                        "invalid escape in the tag, write '%' and two hexadecimal digits",
                    ));
                }
            }
            if !is_tag_char(c) {
                break;
            }
            self.bump();
        }
        let suffix = self.slice(suffix_start, self.pos);
        if handle == "!" && suffix.is_empty() {
            return Ok("!".to_string());
        }
        let prefix = match (self.handles.get(handle), handle) {
            (Some(prefix), _) => prefix.as_str(),
            (None, "!!") => CORE_PREFIX,
            (None, _) => "!",
        };
        match decode_tag(&format!("{prefix}{suffix}")) {
            Some(Ok(full)) => Ok(short_tag(&full)),
            _ => Err(self.error(start, "the escapes of the tag are not valid UTF-8")),
        }
    }

    fn take_name(&mut self) -> &'a str {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_whitespace() || is_flow_indicator(c) {
                break;
            }
            self.bump();
        }
        self.slice(start, self.pos)
    }

    /// Applies the properties to a parsed node.
    fn finish_node(&mut self, parsed: Parsed, props: Props) -> Raw {
        let (pos, kind) = match parsed {
            Parsed::Below(mut raw) => {
                raw.anchors.extend(props.anchor.map(|(_, id)| id));
                if raw.tag.is_none() {
                    raw.tag = props.tag;
                }
                return raw;
            }
            Parsed::Node(kind, pos) => (pos, kind),
            Parsed::Scalar(s) => (
                s.pos,
                RawKind::Scalar {
                    text: s.text,
                    style: s.style,
                },
            ),
        };
        Raw {
            pos,
            tag: props.tag,
            anchors: props.anchor.map(|(_, id)| id).into_iter().collect(),
            kind,
        }
    }

    fn alias(&mut self) -> Result<RawKind> {
        let star = self.pos;
        self.bump();
        let name = self.take_name();
        if name.is_empty() {
            let at = if self.flow_kinds.is_empty() {
                self.pos
            } else {
                star
            };
            return Err(self.error(at, "expected an alias name after '*'"));
        }
        let target = self.anchors.get(name).copied();
        if let Some(count) = target.and_then(|id| self.references.get_mut(id)) {
            *count += 1;
        }
        Ok(RawKind::Alias {
            name: name.to_string(),
            target,
        })
    }

    /// Returns `true` when the rest of the line is a block mapping key.
    fn line_is_key(&self) -> bool {
        match self.peek() {
            Some(quote @ ('"' | '\'')) => {
                let line = self.rest().split('\n').next().unwrap_or("");
                let mut chars = line.char_indices().skip(1);
                let mut end = None;
                while let Some((i, c)) = chars.next() {
                    if quote == '"' && c == '\\' {
                        chars.next();
                    } else if c == quote {
                        if quote == '\'' && line.get(i + 1..).is_some_and(|r| r.starts_with('\'')) {
                            chars.next();
                            continue;
                        }
                        end = Some(i + 1);
                        break;
                    }
                }
                end.and_then(|e| line.get(e..)).is_some_and(|after| {
                    let after = after.trim_start_matches([' ', '\t']);
                    after.starts_with(':')
                        && matches!(after.as_bytes().get(1), None | Some(b' ' | b'\t'))
                })
            }
            Some('[' | '{' | '|' | '>') | None => false,
            _ => matches!(self.scan_plain(false).1, Stop::Colon(_)),
        }
    }

    /// Reads an alias at the column of block mapping keys. Its name can end
    /// with `:` only when a separate `:` follows it.
    fn alias_key(&mut self) -> Result<RawKind> {
        let kind = self.alias()?;
        if let RawKind::Alias { name, .. } = &kind
            && name.ends_with(':')
            && matches!(self.peek(), None | Some(' ' | '\t' | '\n'))
            && !self.rest().trim_start_matches([' ', '\t']).starts_with(':')
        {
            return Err(self.alias_colon_error(self.pos));
        }
        Ok(kind)
    }

    /// The error at a character that cannot follow the alias `name`.
    /// `after_colon` is `true` after the `:` of an alias key.
    fn unexpected_after_alias(&self, name: &str, after_colon: bool, context: Context) -> YamlError {
        match self.peek() {
            Some(c) if !after_colon && name.ends_with(['\'', '"']) => self.error(
                self.pos,
                format!("unexpected {} after the end of a quoted scalar", shown(c)),
            ),
            Some(':') if after_colon && name.ends_with(':') => {
                self.colon_after_key(self.pos, context)
            }
            Some(':') if after_colon => self.error(self.pos, CONTINUED_SCALAR),
            Some('#') if after_colon => self.error(
                self.pos,
                "unexpected '#', a comment needs a space before it",
            ),
            Some('-') if after_colon => self.error(
                self.pos,
                "unexpected '-', a list cannot start on the line of its key",
            ),
            _ => self.unexpected_here(),
        }
    }

    /// The error for an alias name that ends with `:` at `name_end`.
    fn alias_colon_error(&self, name_end: usize) -> YamlError {
        self.error(
            name_end - 1,
            "the name of the alias includes the ':', write a space before ':' if the alias is a key",
        )
    }

    /// Reads the content of a block node at column `col`.
    fn block_content(
        &mut self,
        col: usize,
        parent: isize,
        context: Context,
        depth: usize,
    ) -> Result<Parsed> {
        let start = self.pos;
        let after_key = !context.own_line();
        match self.peek() {
            None => Ok(empty_scalar(start)),
            Some('-') if self.at_dash_item() => {
                if self.key_props.as_ref().is_some_and(|p| !p.is_empty()) {
                    return Err(self.error(
                        start,
                        "unexpected '-', a list cannot start on the line of its anchor or tag",
                    ));
                }
                if after_key {
                    let message = if context == Context::Marker {
                        "unexpected '-', a list cannot start on the line of '---'"
                    } else {
                        "unexpected '-', a list cannot start on the line of its key"
                    };
                    return Err(self.error(start, message));
                }
                Ok(Parsed::Node(self.block_sequence(col, false, depth)?, start))
            }
            Some('|' | '>') => self.block_scalar_content(start, col, parent, context),
            Some(open @ ('[' | '{')) => self.flow_content(open, start, parent, depth),
            Some(quote @ ('"' | '\'')) => self.quoted_content(quote, start, col, parent, context, depth),
            Some('*') => {
                let save = self.pos;
                let value = self.alias()?;
                let name_end = self.pos;
                self.skip_spaces();
                // After a name that ends with `:`, only a separate `:` of a
                // key on its own line can follow.
                if self.slice(save, name_end).ends_with(':')
                    && !self.at_comment_or_eol()
                    && (after_key || self.peek() != Some(':'))
                {
                    return Err(self.alias_colon_error(name_end));
                }
                if self.peek() == Some(':') && matches!(self.peek_second(), None | Some(' ' | '\t' | '\n')) {
                    if after_key {
                        return Err(if context == Context::AfterKey {
                            self.unexpected_after_alias(self.slice(save + 1, name_end), false, context)
                        } else {
                            self.colon_after_key(self.pos, context)
                        });
                    }
                    self.pos = save;
                    return Ok(Parsed::Node(self.block_mapping(col, depth)?, start));
                }
                let after_colon = self.peek() == Some(':') && !after_key;
                if after_colon {
                    self.bump();
                }
                if self.finish_line().is_err() {
                    let name = self.slice(save + 1, name_end);
                    return Err(self.unexpected_after_alias(name, after_colon, context));
                }
                Ok(Parsed::Node(value, start))
            }
            Some('%') if self.explicit && col == 0 && self.peek_second().is_some_and(|c| !c.is_whitespace()) => {
                Err(self.error(
                    start,
                    "unexpected '%', a directive needs '...' on a line above it to end the document",
                ))
            }
            Some(c @ ('@' | '`' | '%')) => Err(self.error(
                start,
                format!("unexpected {}, a plain scalar cannot start with it, quote the value", shown(c)),
            )),
            Some('?') if !self.plain_indicator() && context.own_line() => {
                Ok(Parsed::Node(self.block_mapping(col, depth)?, start))
            }
            Some('?') if self.plain_indicator() => self.block_plain_or_mapping(col, parent, context, depth),
            Some('?' | ']' | '}' | ',') => Err(self.unexpected_here()),
            Some(_) => self.block_plain_or_mapping(col, parent, context, depth),
        }
    }

    // The arms of `block_content` that do not start a nested block node are
    // separate functions, so that the frame of `block_content`, which is on
    // the stack once for each nesting level, stays small.

    fn block_scalar_content(
        &mut self,
        start: usize,
        col: usize,
        parent: isize,
        context: Context,
    ) -> Result<Parsed> {
        // A block scalar that starts its own line is indented from its column.
        let line_start = self
            .text
            .get(..start)
            .map_or(0, |t| t.rfind('\n').map_or(0, |i| i + 1));
        let own_line = self.slice(line_start, start).bytes().all(|b| b == b' ');
        let base = if own_line && context.own_line() {
            col as isize
        } else {
            parent
        };
        Ok(Parsed::Scalar(self.block_scalar(parent, base)?))
    }

    fn flow_content(
        &mut self,
        open: char,
        start: usize,
        parent: isize,
        depth: usize,
    ) -> Result<Parsed> {
        self.flow_indent = usize::try_from(parent.saturating_add(1)).unwrap_or(0);
        let value = self.flow_collection(depth)?;
        self.skip_spaces();
        if self.peek() == Some(':') && matches!(self.peek_second(), None | Some(' ' | '\t' | '\n'))
        {
            return Err(self.path_error(start, key_kind_message(open)));
        }
        if let Err(pos) = self.finish_line() {
            let c = self.peek().unwrap_or(' ');
            let message = if matches!(c, '@' | '`' | '%') {
                format!(
                    "unexpected {}, a plain scalar cannot start with it, quote the value",
                    shown(c)
                )
            } else {
                format!("unexpected {} after the end of a flow collection", shown(c))
            };
            return Err(self.error(pos, message));
        }
        Ok(Parsed::Node(value, start))
    }

    #[allow(clippy::too_many_arguments)]
    fn quoted_content(
        &mut self,
        quote: char,
        start: usize,
        col: usize,
        parent: isize,
        context: Context,
        depth: usize,
    ) -> Result<Parsed> {
        let after_key = !context.own_line();
        self.quote_indent = usize::try_from(parent.saturating_add(1)).unwrap_or(0);
        let text = self.quoted(quote)?;
        self.quote_indent = 0;
        let end = self.pos;
        self.skip_spaces();
        if self.peek() == Some(':') && matches!(self.peek_second(), None | Some(' ' | '\t' | '\n'))
        {
            if self.source_has_break(start) {
                return Err(self.error(self.pos, "unexpected ':', a key must be on a single line"));
            }
            if after_key {
                return Err(self.colon_after_key(self.pos, context));
            }
            self.pos = start;
            return Ok(Parsed::Node(self.block_mapping(col, depth)?, start));
        }
        self.pos = end;
        if let Err(pos) = self.finish_line() {
            let c = self.peek().unwrap_or(' ');
            let message = if c == '#' {
                "unexpected '#', a comment needs a space before it".to_string()
            } else if pos > end {
                format!("unexpected {} after the end of a quoted scalar", shown(c))
            } else if !(c.is_alphanumeric() || matches!(c, '\\' | '"')) {
                format!("unexpected {}", shown(c))
            } else if quote == '"' {
                format!(
                    "unexpected {} after a double-quoted scalar, write \\\" for a quote inside it",
                    shown(c)
                )
            } else {
                format!(
                    "unexpected {} after a single-quoted scalar, write '' for a quote inside it",
                    shown(c)
                )
            };
            return Err(self.error(pos, message));
        }
        Ok(Parsed::Scalar(Scalar {
            text,
            style: if quote == '"' {
                Style::DoubleQuoted
            } else {
                Style::SingleQuoted
            },
            pos: start,
        }))
    }

    /// Returns `true` when an indicator `?`, `-` or `:` at the position starts
    /// a plain scalar, because a character that is not a space follows it.
    fn plain_indicator(&self) -> bool {
        let flow = !self.flow_kinds.is_empty();
        self.peek_second()
            .is_some_and(|c| !c.is_whitespace() && !(flow && is_flow_indicator(c)))
    }

    fn block_plain_or_mapping(
        &mut self,
        col: usize,
        parent: isize,
        context: Context,
        depth: usize,
    ) -> Result<Parsed> {
        let start = self.pos;
        let (_, stop) = self.scan_plain(false);
        if let Stop::Colon(colon) = stop {
            if !context.own_line() {
                if colon == start && context == Context::AfterKey {
                    return Err(self.error(colon, "unexpected ':'"));
                }
                return Err(self.colon_after_key(colon, context));
            }
            return Ok(Parsed::Node(self.block_mapping(col, depth)?, start));
        }
        Ok(Parsed::Scalar(self.block_plain(parent)?))
    }

    fn colon_after_key(&self, pos: usize, context: Context) -> YamlError {
        if context == Context::Marker {
            self.error(
                pos,
                "unexpected ':', a mapping cannot start on the line of '---'",
            )
        } else {
            self.error(pos, "unexpected ':', quote the value if it contains \": \"")
        }
    }

    /// Reads a block sequence whose `-` indicators are at column `col`.
    /// `same_level` is `true` for a list at the indentation of its key.
    fn block_sequence(&mut self, col: usize, same_level: bool, depth: usize) -> Result<RawKind> {
        self.check_depth(depth)?;
        let mut items = Vec::new();
        loop {
            self.bump();
            self.skip_spaces();
            self.path.push(Segment::Index(items.len()));
            let item = if self.at_comment_or_eol() {
                let _ = self.finish_line();
                let parsed = self.parsed_below(col as isize, false, depth)?;
                self.finish_node(parsed, Props::default())
            } else {
                self.block_node(col as isize, Context::Block, depth + 1)?
            };
            self.path.pop();
            items.push(item);
            self.skip_blank_lines();
            if self.eof() {
                break;
            }
            let n = self.indent_here()?;
            if n < col {
                break;
            }
            let line_start = self.pos;
            self.pos += n;
            if n > col {
                return Err(self.error(self.pos, "unexpected indentation"));
            }
            if self.at_dash_item() {
                continue;
            }
            if same_level {
                self.pos = line_start;
                break;
            }
            if self.peek() == Some('-') {
                return Err(self.error(self.pos + 1, "expected a space after '-'"));
            }
            if let (_, Stop::Colon(_)) = self.scan_plain(false) {
                return Err(self.error(self.pos, "unexpected key among list items"));
            }
            return Err(self.error(self.pos, "unexpected value among list items"));
        }
        Ok(RawKind::Sequence(items))
    }

    /// Reads a block mapping whose keys are at column `col`.
    fn block_mapping(&mut self, col: usize, depth: usize) -> Result<RawKind> {
        self.check_depth(depth)?;
        let mut entries = Vec::new();
        loop {
            // An explicit key skips `block_key`, so that a chain of explicit
            // keys has one stack frame less for each level.
            let explicit = self.key_props.as_ref().is_none_or(Props::is_empty)
                && self.peek() == Some('?')
                && !self.plain_indicator();
            let key = if explicit {
                self.key_props = None;
                self.explicit_key(col, depth)?
            } else {
                self.block_key()?
            };
            self.path
                .push(Segment::Key(key.text().unwrap_or_default().to_string()));
            let explicit_value = std::mem::take(&mut self.explicit_value);
            let empty_value = std::mem::take(&mut self.empty_value);
            // After an explicit key without a value, the position is at the
            // start of the next line, and its indentation belongs to the loop.
            if !empty_value {
                self.skip_spaces();
            }
            let value = if empty_value {
                scalar_raw(empty_scalar_at(self.pos))
            } else if self.at_comment_or_eol() {
                let _ = self.finish_line();
                let parsed = self.parsed_below(col as isize, true, depth)?;
                self.finish_node(parsed, Props::default())
            } else if explicit_value {
                self.block_node(col as isize, Context::Block, depth + 1)?
            } else {
                self.block_node(col as isize, Context::AfterKey, depth + 1)?
            };
            self.path.pop();
            entries.push((key, value));
            self.skip_blank_lines();
            if self.eof() {
                break;
            }
            let n = self.indent_here()?;
            if n < col {
                break;
            }
            self.pos += n;
            if n > col {
                return Err(self.error(self.pos, "unexpected indentation"));
            }
            if self.at_dash_item() {
                return Err(self.error(self.pos, "unexpected list item among mapping entries"));
            }
        }
        Ok(RawKind::Mapping(entries))
    }

    /// Reads a key of a block mapping and the `:` after it.
    fn block_key(&mut self) -> Result<Raw> {
        let props_at = self.pos;
        let props = match self.key_props.take() {
            Some(props) => props,
            None => self.properties(None)?,
        };
        if !props.is_empty() && self.at_comment_or_eol() {
            return Err(self.error(
                props_at,
                "an anchor or a tag cannot be on a line of its own here, write it after the key or the '-'",
            ));
        }
        let start = self.pos;
        match self.peek() {
            Some('%')
                if self.explicit
                    && self.column(start) == 0
                    && self.peek_second().is_some_and(|c| !c.is_whitespace()) =>
            {
                return Err(self.error(
                    start,
                    "unexpected '%', a directive needs '...' on a line above it to end the document",
                ));
            }
            Some(c @ ('@' | '`' | '%')) => {
                return Err(self.error(
                    start,
                    format!(
                        "unexpected {}, a plain scalar cannot start with it, quote the value",
                        shown(c)
                    ),
                ));
            }
            Some(',' | ']' | '}' | '|' | '>') => return Err(self.unexpected_here()),
            _ => {}
        }
        if self.peek() == Some('*') {
            let kind = self.alias_key()?;
            let name_end = self.pos;
            self.skip_spaces();
            match self.peek() {
                Some(':') => {
                    self.bump();
                    if !matches!(self.peek(), None | Some(' ' | '\t' | '\n')) {
                        let name = self.slice(start + 1, name_end);
                        return Err(self.unexpected_after_alias(name, true, Context::Block));
                    }
                }
                Some(_) if !self.at_comment_or_eol() => {
                    let name = self.slice(start + 1, name_end);
                    return Err(self.unexpected_after_alias(name, false, Context::Block));
                }
                _ => {
                    if let Some(i) = colon_without_space(self.slice(start + 1, name_end)) {
                        return Err(self.error(start + i + 2, "expected a space after ':'"));
                    }
                    let at = if self.peek() == Some('#') {
                        name_end
                    } else {
                        self.pos
                    };
                    return Err(self.error(at, "expected ':' after the key"));
                }
            }
            return Ok(self.finish_node(Parsed::Node(kind, start), props));
        }
        let (text, style) = match self.peek() {
            Some(quote @ ('"' | '\'')) => {
                let key = self.quoted(quote)?;
                if self.source_has_break(start) {
                    self.skip_spaces();
                    return Err(
                        self.error(self.pos, "unexpected ':', a key must be on a single line")
                    );
                }
                self.skip_spaces();
                if self.peek() == Some(':')
                    && !matches!(self.peek_second(), None | Some(' ' | '\t' | '\n'))
                {
                    self.bump();
                    return Err(self.unexpected_here());
                }
                let style = if quote == '"' {
                    Style::DoubleQuoted
                } else {
                    Style::SingleQuoted
                };
                (key, style)
            }
            Some(open @ ('[' | '{')) => {
                let line = self.rest().split('\n').next().unwrap_or("");
                let line = line.split(" #").next().unwrap_or(line).trim_end();
                if !(line.contains(": ") || line.contains(":\t") || line.ends_with(':')) {
                    let end = self.pos + self.rest().find('\n').unwrap_or(self.rest().len());
                    return Err(self.error(end, "expected ':' after the key"));
                }
                return Err(self.path_error(start, key_kind_message(open)));
            }
            Some('-') if self.at_dash_item() => {
                return Err(self.error(start, "unexpected list item among mapping entries"));
            }
            Some('?') if !self.plain_indicator() => return Err(self.unexpected_here()),
            _ => {
                let (end, stop) = self.scan_plain(false);
                let key = self.slice(start, end).to_string();
                match stop {
                    Stop::Colon(colon) => self.pos = colon,
                    _ => self.pos = end,
                }
                (key, Style::Plain)
            }
        };
        if self.peek() != Some(':')
            && style == Style::Plain
            && let Some(i) = colon_without_space(&text)
        {
            return Err(self.error(start + i + 1, "expected a space after ':'"));
        }
        if self.peek() == Some(':') {
            self.bump();
            Ok(self.finish_node(
                Parsed::Scalar(Scalar {
                    text,
                    style,
                    pos: start,
                }),
                props,
            ))
        } else {
            let line_end = self.pos + self.rest().find('\n').unwrap_or(self.rest().len());
            Err(self.error(line_end, "expected ':' after the key"))
        }
    }

    /// Reads an explicit key `? key` and the `:` line after it. Without a
    /// `:` line, the value is empty.
    fn explicit_key(&mut self, col: usize, depth: usize) -> Result<Raw> {
        self.bump();
        self.skip_spaces();
        let key = if self.at_comment_or_eol() {
            let here = self.pos;
            let _ = self.finish_line();
            let parsed = match self.parsed_below(col as isize, false, depth)? {
                // `parsed_below` gives a scalar only for an empty value.
                Parsed::Scalar(mut s) => {
                    s.pos = here;
                    Parsed::Scalar(s)
                }
                parsed => parsed,
            };
            self.finish_node(parsed, Props::default())
        } else {
            self.block_node(col as isize, Context::Block, depth + 1)?
        };
        let line_start = self.pos;
        self.skip_blank_lines();
        let n = self.indent_here()?;
        let at = self.pos + n;
        let colon = self.text.get(at..).unwrap_or("");
        if n == col
            && colon.starts_with(':')
            && matches!(colon.as_bytes().get(1), None | Some(b' ' | b'\t' | b'\n'))
        {
            self.pos = at + 1;
            self.explicit_value = true;
        } else {
            self.pos = line_start;
            self.empty_value = true;
        }
        Ok(key)
    }

    /// Finds where a plain scalar stops on the current line. Returns the end
    /// of its last character that is not a space, and the reason.
    fn scan_plain(&self, flow: bool) -> (usize, Stop) {
        let base = self.pos;
        let mut end = base;
        let mut after_space = false;
        let mut chars = self.rest().char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            crate::steps::add(1);
            let at = base + i;
            let next = chars.peek().map(|(_, n)| *n);
            match c {
                '\n' => return (end, Stop::Eol),
                '#' if after_space => return (end, Stop::Comment),
                ':' if matches!(next, None | Some(' ' | '\t' | '\n'))
                    || (flow && next.is_some_and(is_flow_indicator)) =>
                {
                    return (end, Stop::Colon(at));
                }
                c if flow && is_flow_indicator(c) => return (end, Stop::Flow),
                ' ' | '\t' => {
                    after_space = true;
                    continue;
                }
                _ => {}
            }
            after_space = false;
            end = at + c.len_utf8();
        }
        (end, Stop::Eol)
    }

    /// Reads a plain scalar in block context, with its continuation lines.
    fn block_plain(&mut self, parent: isize) -> Result<Scalar> {
        let start = self.pos;
        let (end, mut stop) = self.scan_plain(false);
        let mut text = self.slice(start, end).to_string();
        self.pos = end;
        while stop == Stop::Eol {
            self.skip_spaces();
            if self.peek() != Some('\n') {
                break;
            }
            let line_end = self.pos;
            self.pos += 1;
            let mut empty_lines = 0;
            loop {
                let line_start = self.pos;
                self.skip_spaces();
                if self.peek() == Some('\n') {
                    self.pos += 1;
                    empty_lines += 1;
                } else {
                    self.pos = line_start;
                    break;
                }
            }
            let n = self.rest().bytes().take_while(|b| *b == b' ').count();
            self.pos += n;
            if self.eof() || n as isize <= parent {
                self.pos = line_end;
                break;
            }
            if self.peek() == Some('#') {
                self.pos = line_end;
                stop = Stop::Comment;
                break;
            }
            self.skip_spaces();
            let line_start = self.pos;
            let (end, next_stop) = self.scan_plain(false);
            if let Stop::Colon(colon) = next_stop {
                return Err(self.error(colon, CONTINUED_SCALAR));
            }
            if empty_lines == 0 {
                text.push(' ');
            } else {
                text.push_str(&"\n".repeat(empty_lines));
            }
            text.push_str(self.slice(line_start, end));
            self.pos = end;
            stop = next_stop;
        }
        if self.finish_line().is_err() {
            return Err(self.unexpected_here());
        }
        if stop == Stop::Comment {
            let line_start = self.pos;
            self.skip_blank_lines();
            let n = self.rest().bytes().take_while(|b| *b == b' ').count();
            if !self.eof() && n as isize > parent {
                return Err(self.error(
                    self.pos + n,
                    "a comment ends a plain scalar, so this line cannot continue it",
                ));
            }
            self.pos = line_start;
        }
        Ok(Scalar {
            text,
            style: Style::Plain,
            pos: start,
        })
    }

    /// Reads a literal `|` or folded `>` block scalar.
    /// `base` is the indentation that an indentation indicator adds to.
    fn block_scalar(&mut self, parent: isize, base: isize) -> Result<Scalar> {
        let start = self.pos;
        let literal = self.bump() == Some('|');
        let mut chomp = None;
        let mut explicit = None;
        loop {
            match self.peek() {
                Some('-') if chomp.is_none() => chomp = Some(Chomp::Strip),
                Some('+') if chomp.is_none() => chomp = Some(Chomp::Keep),
                Some('0') if explicit.is_none() => return Err(self.indent_indicator_error()),
                Some(c @ '1'..='9') if explicit.is_none() => explicit = c.to_digit(10),
                Some('0'..='9') => return Err(self.indent_indicator_error()),
                _ => break,
            }
            self.bump();
        }
        let chomp = chomp.unwrap_or(Chomp::Clip);
        match self.peek() {
            None | Some('\n') => {}
            Some(' ' | '\t') => {
                self.skip_spaces();
                if !self.at_comment_or_eol() {
                    return Err(self.error(
                        self.pos,
                        "the content of a block scalar starts on the next line",
                    ));
                }
            }
            Some(_) => return Err(self.error(start, "invalid block scalar header")),
        }
        let _ = self.finish_line();
        let indent = match explicit {
            Some(digit) => (parent.max(0) + digit as isize) as usize,
            None => self.detect_indent(parent)?,
        };
        let mut lines: Vec<&str> = Vec::new();
        while !self.eof() {
            let rest = self.rest();
            let line = rest.split('\n').next().unwrap_or("");
            let n = line.bytes().take_while(|b| *b == b' ').count();
            let skip = (line.len() + 1).min(rest.len());
            if n == line.len() || n >= indent {
                lines.push(line.get(indent..).unwrap_or(""));
                self.pos += skip;
                continue;
            }
            if line.as_bytes().get(n) == Some(&b'\t') && n as isize > base {
                return Err(self.error(self.pos + n, "tabs cannot be used for indentation"));
            }
            if n as isize > base {
                return Err(self.error(
                    self.pos + n,
                    "unexpected indentation, the line has less indentation than the block scalar above it",
                ));
            }
            break;
        }
        let (text, style) = if literal {
            (literal_text(&lines, chomp), Style::Literal)
        } else {
            (folded_text(&lines, chomp), Style::Folded)
        };
        Ok(Scalar {
            text,
            style,
            pos: start,
        })
    }

    fn indent_indicator_error(&self) -> YamlError {
        self.error(
            self.pos,
            "the indentation indicator of a block scalar must be from 1 to 9",
        )
    }

    /// Finds the indentation of a block scalar from its first line with text.
    /// Without such a line, the longest line of spaces gives it.
    fn detect_indent(&self, parent: isize) -> Result<usize> {
        let minimum = (parent + 1).max(0) as usize;
        let mut longest_blank = 0;
        let mut longest_at = self.pos;
        let mut at = self.pos;
        for line in self.rest().split('\n') {
            let n = line.bytes().take_while(|b| *b == b' ').count();
            if n < line.len() {
                if n as isize > parent {
                    if longest_blank > n.max(minimum) {
                        return Err(self.error(
                            longest_at,
                            "a leading empty line of a block scalar has more spaces than the first non-empty line",
                        ));
                    }
                    return Ok(n.max(minimum));
                }
                break;
            }
            if n > longest_blank {
                longest_blank = n;
                longest_at = at + n;
            }
            at += line.len() + 1;
        }
        Ok(longest_blank.max(minimum))
    }

    /// Returns `true` when the source from `start` to the position has a line break.
    fn source_has_break(&self, start: usize) -> bool {
        self.slice(start, self.pos).contains('\n')
    }

    fn quoted(&mut self, quote: char) -> Result<String> {
        if quote == '"' {
            self.double_quoted()
        } else {
            self.single_quoted()
        }
    }

    fn double_quoted(&mut self) -> Result<String> {
        let start = self.pos;
        self.bump();
        let mut out = String::new();
        let mut floor = 0;
        loop {
            let at = self.pos;
            match self.bump() {
                None => return Err(self.error(start, "unterminated double-quoted scalar")),
                Some('"') => return self.quote_error.take().map_or(Ok(out), Err),
                Some('\n') => {
                    self.fold_line_break(&mut out, floor, true, start)?;
                    floor = out.len();
                }
                Some('\\') => {
                    match self.bump() {
                        None => {
                            return Err(self.error(start, "unterminated double-quoted scalar"));
                        }
                        Some('\n') => loop {
                            // An escaped line break keeps the empty lines after it.
                            self.quote_line(true, start)?;
                            self.skip_spaces();
                            if self.peek() != Some('\n') {
                                break;
                            }
                            self.pos += 1;
                            out.push('\n');
                        },
                        Some(c) => out.push(self.escape(c, at)?),
                    }
                    floor = out.len();
                }
                Some(c) => out.push(c),
            }
        }
    }

    /// Returns the character of the escape `\c`. `at` is the backslash.
    fn escape(&mut self, c: char, at: usize) -> Result<char> {
        let digits = match c {
            '0' => return Ok('\0'),
            'a' => return Ok('\u{7}'),
            'b' => return Ok('\u{8}'),
            't' | '\t' => return Ok('\t'),
            'n' => return Ok('\n'),
            'v' => return Ok('\u{b}'),
            'f' => return Ok('\u{c}'),
            'r' => return Ok('\r'),
            'e' => return Ok('\u{1b}'),
            ' ' | '"' | '/' | '\\' => return Ok(c),
            'N' => return Ok('\u{85}'),
            '_' => return Ok('\u{a0}'),
            'L' => return Ok('\u{2028}'),
            'P' => return Ok('\u{2029}'),
            'x' => 2,
            'u' => 4,
            'U' => 8,
            _ => {
                return Err(self.error(
                    at,
                    "invalid escape sequence, write \\\\ for a backslash or use single quotes",
                ));
            }
        };
        let hex = self.rest().get(..digits).unwrap_or("");
        let code = if hex.len() == digits && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else {
            None
        };
        match code {
            Some(ch) => {
                self.pos += digits;
                Ok(ch)
            }
            None if self.rest().starts_with(|c: char| c.is_ascii_hexdigit()) => {
                Err(self.error(at, "invalid escape sequence"))
            }
            None => Err(self.error(
                at,
                "invalid escape sequence, write \\\\ for a backslash or use single quotes",
            )),
        }
    }

    fn single_quoted(&mut self) -> Result<String> {
        let start = self.pos;
        self.bump();
        let mut out = String::new();
        let mut floor = 0;
        loop {
            match self.bump() {
                None => return Err(self.error(start, "unterminated single-quoted scalar")),
                Some('\'') if self.peek() == Some('\'') => {
                    self.bump();
                    out.push('\'');
                    floor = out.len();
                }
                Some('\'') => return self.quote_error.take().map_or(Ok(out), Err),
                Some('\n') => {
                    self.fold_line_break(&mut out, floor, false, start)?;
                    floor = out.len();
                }
                Some(c) => out.push(c),
            }
        }
    }

    /// Folds a line break in a quoted scalar. The break is already consumed.
    /// Spaces after `floor` at the end of `out` are removed. Returns `false`
    /// when the next line has too little indentation to continue the scalar.
    fn fold_line_break(
        &mut self,
        out: &mut String,
        floor: usize,
        double: bool,
        start: usize,
    ) -> Result<()> {
        while out.len() > floor && out.ends_with([' ', '\t']) {
            out.pop();
        }
        let mut empty_lines = 0;
        loop {
            self.quote_line(double, start)?;
            self.skip_spaces();
            if self.peek() == Some('\n') {
                self.pos += 1;
                empty_lines += 1;
            } else {
                break;
            }
        }
        if empty_lines == 0 {
            out.push(' ');
        } else {
            out.push_str(&"\n".repeat(empty_lines));
        }
        Ok(())
    }

    /// Checks the indentation of a continuation line of a quoted scalar. The
    /// position is at the start of the line. A line with text needs more
    /// spaces than the indentation of the parent node, and a tab cannot stand
    /// in their place.
    ///
    /// Pandoc reports a bad line only when the scalar ends; a line that looks
    /// like a mapping key ends the scalar as unterminated.
    fn quote_line(&mut self, double: bool, start: usize) -> Result<()> {
        let n = self.rest().bytes().take_while(|b| *b == b' ').count();
        if n >= self.quote_indent || self.quote_error.is_some() {
            return Ok(());
        }
        let after = self.rest().get(n..).unwrap_or("");
        let line = after.split('\n').next().unwrap_or("");
        let content = line.trim_start_matches([' ', '\t']);
        if content.is_empty() {
            // An empty line can have spaces, but no tab in the indentation.
            if after.starts_with('\t') {
                self.quote_error =
                    Some(self.error(self.pos + n, "tabs cannot be used for indentation"));
            }
            return Ok(());
        }
        let kind = if double { "double" } else { "single" };
        if content.contains(": ") || content.ends_with(':') {
            return Err(self.error(start, format!("unterminated {kind}-quoted scalar")));
        }
        self.quote_error = Some(if after.starts_with('\t') {
            self.error(self.pos + n, "tabs cannot be used for indentation")
        } else {
            self.error(
                self.pos + n,
                format!("invalid indentation of a line in a {kind}-quoted scalar"),
            )
        });
        Ok(())
    }

    /// Reads a flow sequence or a flow mapping.
    fn flow_collection(&mut self, depth: usize) -> Result<RawKind> {
        self.check_depth(depth)?;
        let open = self.peek().unwrap_or('[');
        self.flow_kinds.push(open);
        let kind = if open == '[' {
            self.flow_sequence(depth)
        } else {
            self.flow_mapping(depth)
        }?;
        self.flow_kinds.pop();
        if self.flow_kinds.is_empty()
            && let Some(error) = self.flow_violation.take()
        {
            return Err(error);
        }
        Ok(kind)
    }

    /// Reads a node in a flow collection. `next` is the error text for a
    /// token that cannot follow the node.
    fn flow_value(&mut self, depth: usize, next: &'static str) -> Result<Raw> {
        let (parsed, props, _) = self.flow_parsed(depth, next, false)?;
        Ok(self.finish_node(parsed, props))
    }

    /// Reads a node in a flow collection. The flag is `true` when a line
    /// break separates the properties from the content: then the content
    /// cannot be an alias, and in a flow sequence it cannot be a key. `key` is
    /// `true` in the place of a key.
    fn flow_parsed(
        &mut self,
        depth: usize,
        next: &'static str,
        key: bool,
    ) -> Result<(Parsed, Props, bool)> {
        self.check_depth(depth)?;
        let mut props = self.properties(Some(next))?;
        let mut broken = false;
        loop {
            let end = self.pos;
            self.skip_flow_space()?;
            broken |= !props.is_empty() && self.source_has_break(end);
            if props.is_empty() || !matches!(self.peek(), Some('&' | '!')) {
                break;
            }
            let at = self.pos;
            if !props.add(self.properties(Some(next))?) {
                return Err(self.error(at, next));
            }
        }
        if broken && self.peek() == Some('*') {
            return Err(self.error(self.pos, next));
        }
        let start = self.pos;
        let parsed = match self.peek() {
            None | Some(',' | ']' | '}') => empty_scalar(start),
            Some('[' | '{') => Parsed::Node(self.flow_collection(depth)?, start),
            Some(quote @ ('"' | '\'')) => Parsed::Scalar(Scalar {
                text: {
                    let outer = std::mem::replace(&mut self.quote_indent, self.flow_indent);
                    let text = self.quoted(quote);
                    self.quote_indent = outer;
                    text?
                },
                style: if quote == '"' {
                    Style::DoubleQuoted
                } else {
                    Style::SingleQuoted
                },
                pos: start,
            }),
            Some('*') => {
                let alias = self.alias()?;
                let after = self.rest().trim_start_matches([' ', '\t']);
                let separate_colon = key
                    && after.strip_prefix(':').is_some_and(|rest| {
                        matches!(
                            rest.chars().next(),
                            None | Some(' ' | '\t' | '\n' | ',' | ']' | '}')
                        )
                    });
                if self.slice(start, self.pos).ends_with(':')
                    && !separate_colon
                    && !matches!(
                        after.chars().next(),
                        None | Some('\n' | '#' | ',' | ']' | '}')
                    )
                {
                    return Err(self.alias_colon_error(self.pos));
                }
                Parsed::Node(alias, start)
            }
            Some(c @ ('@' | '`' | '%')) => {
                return Err(self.error(
                    start,
                    format!(
                        "unexpected {}, a plain scalar cannot start with it, quote the value",
                        shown(c)
                    ),
                ));
            }
            Some('#') => {
                return Err(self.error(start, "unexpected '#', a comment needs a space before it"));
            }
            Some('-')
                if self.at_dash_item()
                    || self
                        .peek_second()
                        .is_some_and(|c| matches!(c, ',' | ']' | '}')) =>
            {
                return Err(self.error(
                    start,
                    "unexpected '-', a list item cannot be inside a flow collection, quote '-' if it is a string",
                ));
            }
            Some(c @ ('|' | '>')) => {
                return Err(self.error(
                    start,
                    format!(
                        "unexpected {}, a block scalar cannot be inside a flow collection",
                        shown(c)
                    ),
                ));
            }
            Some('?') if !self.plain_indicator() => return Err(self.unexpected_here()),
            Some(_) => Parsed::Scalar(self.flow_plain()?),
        };
        Ok((parsed, props, broken))
    }

    /// Reads a plain scalar in a flow collection, with its continuation lines.
    fn flow_plain(&mut self) -> Result<Scalar> {
        let start = self.pos;
        let mut text = String::new();
        loop {
            let line_start = self.pos;
            let (end, stop) = self.scan_plain(true);
            text.push_str(self.slice(line_start, end));
            self.pos = end;
            if stop != Stop::Eol {
                break;
            }
            let line_end = self.pos;
            self.skip_spaces();
            if self.peek() != Some('\n') {
                self.pos = line_end;
                break;
            }
            let mut empty_lines = 0;
            while self.peek() == Some('\n') {
                self.pos += 1;
                self.check_flow_line()?;
                self.skip_spaces();
                if self.peek() == Some('\n') {
                    empty_lines += 1;
                }
            }
            match self.peek() {
                None | Some(',' | '[' | ']' | '{' | '}' | '#' | ':') => {
                    self.pos = line_end;
                    break;
                }
                _ => {}
            }
            if empty_lines == 0 {
                text.push(' ');
            } else {
                text.push_str(&"\n".repeat(empty_lines));
            }
        }
        Ok(Scalar {
            text,
            style: Style::Plain,
            pos: start,
        })
    }

    fn flow_sequence(&mut self, depth: usize) -> Result<RawKind> {
        let start = self.pos;
        self.bump();
        let mut items = Vec::new();
        loop {
            self.skip_flow_space()?;
            match self.peek() {
                None => return Err(self.error(start, "unterminated flow sequence")),
                Some(']') => {
                    self.bump();
                    return Ok(RawKind::Sequence(items));
                }
                Some(',') => {
                    return Err(self.error(
                        self.pos,
                        "unexpected ',', a flow collection cannot have an empty entry",
                    ));
                }
                _ => {}
            }
            self.path.push(Segment::Index(items.len()));
            let entry = self.pos;
            if self.peek() == Some('?') && self.peek_second().is_some_and(is_flow_indicator) {
                return Err(self.error(self.pos, "expected ',' or ']'"));
            }
            let explicit = self.peek() == Some('?') && !self.plain_indicator();
            if explicit {
                self.bump();
                self.skip_flow_space()?;
                if self.peek() == Some('?') && !self.plain_indicator() {
                    return Err(self.error(self.pos, "expected ',' or ']'"));
                }
            }
            let key_pos = self.pos;
            let (parsed, props, broken) = self.flow_parsed(depth + 1, SEQUENCE_NEXT, true)?;
            self.skip_flow_space()?;
            if broken && !explicit && self.peek() == Some(':') {
                return Err(self.error(self.pos, SEQUENCE_NEXT));
            }
            if matches!(parsed, Parsed::Node(RawKind::Alias { .. }, _)) {
                self.check_alias_colon(SEQUENCE_NEXT, SEQUENCE_NEXT)?;
            }
            let item = if explicit || self.peek() == Some(':') {
                let key = self.flow_key(parsed, props, key_pos)?;
                let colon = self.peek() == Some(':');
                if colon {
                    self.bump();
                }
                self.skip_flow_space()?;
                let value = if !colon || matches!(self.peek(), Some(',' | ']')) {
                    scalar_raw(empty_scalar_at(self.pos))
                } else {
                    self.path
                        .push(Segment::Key(key.text().unwrap_or_default().to_string()));
                    let value = self.flow_value(depth + 2, SEQUENCE_NEXT)?;
                    self.path.pop();
                    value
                };
                Raw {
                    pos: entry,
                    tag: None,
                    anchors: Vec::new(),
                    kind: RawKind::Mapping(vec![(key, value)]),
                }
            } else {
                self.finish_node(parsed, props)
            };
            self.path.pop();
            items.push(item);
            self.skip_flow_space()?;
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some(']') => {}
                None => return Err(self.error(start, "unterminated flow sequence")),
                Some(_) => return Err(self.error(self.pos, SEQUENCE_NEXT)),
            }
        }
    }

    /// After an alias key in a flow collection, a `:` before a space, a line
    /// break or a flow indicator separates the value. A collection cannot
    /// start right after that `:`, and another `:` is not a separator.
    fn check_alias_colon(&self, key_message: &str, next_message: &str) -> Result<()> {
        if self.peek() != Some(':') {
            return Ok(());
        }
        match self.peek_second() {
            None | Some(' ' | '\t' | '\n' | ',' | ']' | '}') => Ok(()),
            Some('[' | '{') => Err(self.error(self.pos + 1, next_message)),
            Some(_) => Err(self.error(self.pos, key_message)),
        }
    }

    fn flow_mapping(&mut self, depth: usize) -> Result<RawKind> {
        let start = self.pos;
        self.bump();
        let mut entries = Vec::new();
        loop {
            self.skip_flow_space()?;
            match self.peek() {
                None => return Err(self.error(start, "unterminated flow mapping")),
                Some('}') => {
                    self.bump();
                    return Ok(RawKind::Mapping(entries));
                }
                Some(',') => {
                    return Err(self.error(
                        self.pos,
                        "unexpected ',', a flow collection cannot have an empty entry",
                    ));
                }
                Some(open @ ('[' | '{')) => {
                    return Err(self.path_error(self.pos, key_kind_message(open)));
                }
                _ => {}
            }
            if self.peek() == Some('?') && self.peek_second().is_some_and(is_flow_indicator) {
                return Err(self.error(self.pos, "expected ',' or '}'"));
            }
            if self.peek() == Some('?') && !self.plain_indicator() {
                self.bump();
                self.skip_flow_space()?;
            }
            let key_pos = self.pos;
            let (parsed, props, _) =
                self.flow_parsed(depth + 1, "expected ':', ',' or '}'", true)?;
            let key = self.flow_key(parsed, props, key_pos)?;
            self.skip_flow_space()?;
            if !matches!(self.peek(), None | Some(':' | ',' | '}')) {
                return Err(self.error(self.pos, "expected ':', ',' or '}'"));
            }
            if matches!(key.kind, RawKind::Alias { .. }) {
                self.check_alias_colon("expected ':', ',' or '}'", MAPPING_NEXT)?;
            }
            let value = if self.peek() == Some(':') {
                self.bump();
                let plain = matches!(
                    key.kind,
                    RawKind::Scalar {
                        style: Style::Plain,
                        ..
                    }
                );
                if plain && !matches!(self.peek(), None | Some(' ' | '\t' | '\n' | ',' | '}')) {
                    return Err(self.error(self.pos, "expected ',' or '}'"));
                }
                self.skip_flow_space()?;
                if matches!(self.peek(), Some(',' | '}')) {
                    scalar_raw(empty_scalar_at(self.pos))
                } else {
                    self.path
                        .push(Segment::Key(key.text().unwrap_or_default().to_string()));
                    let value = self.flow_value(depth + 1, MAPPING_NEXT)?;
                    self.path.pop();
                    value
                }
            } else {
                scalar_raw(empty_scalar_at(self.pos))
            };
            entries.push((key, value));
            self.skip_flow_space()?;
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some('}') => {}
                None => return Err(self.error(start, "unterminated flow mapping")),
                Some(_) => return Err(self.error(self.pos, MAPPING_NEXT)),
            }
        }
    }

    /// Returns a key in a flow collection. A key must be a scalar.
    fn flow_key(&mut self, parsed: Parsed, props: Props, pos: usize) -> Result<Raw> {
        match parsed {
            Parsed::Scalar(_) | Parsed::Node(RawKind::Alias { .. }, _) => {
                Ok(self.finish_node(parsed, props))
            }
            Parsed::Node(RawKind::Sequence(_), _) => {
                Err(self.path_error(pos, key_kind_message('[')))
            }
            Parsed::Node(RawKind::Mapping(_), _) => {
                Err(self.path_error(pos, key_kind_message('{')))
            }
            Parsed::Node(..) | Parsed::Below(_) => {
                Err(self.path_error(pos, "expected a scalar key"))
            }
        }
    }
}

/// The error for a token after a node in a flow sequence.
const SEQUENCE_NEXT: &str = "expected ',' or ']'";
/// The error for a token after a value in a flow mapping.
const MAPPING_NEXT: &str = "expected ',' or '}'";

/// Where a block node starts.
#[derive(Clone, Copy, PartialEq)]
enum Context {
    /// At the start of its own line.
    Block,
    /// At the start of its own line, as the value of a mapping key. After
    /// properties alone on that line, a list can start at the indentation
    /// of the key.
    BelowKey,
    /// On the line of its mapping key.
    AfterKey,
    /// On the line of a `---` marker.
    Marker,
}

impl Context {
    fn own_line(self) -> bool {
        matches!(self, Context::Block | Context::BelowKey)
    }
}

impl Raw {
    fn text(&self) -> Option<&str> {
        match &self.kind {
            RawKind::Scalar { text, .. } => Some(text),
            _ => None,
        }
    }
}

fn empty_scalar_at(pos: usize) -> Scalar {
    Scalar {
        text: String::new(),
        style: Style::Plain,
        pos,
    }
}

fn scalar_raw(s: Scalar) -> Raw {
    Raw {
        pos: s.pos,
        tag: None,
        anchors: Vec::new(),
        kind: RawKind::Scalar {
            text: s.text,
            style: s.style,
        },
    }
}

/// The chomping indicator of a block scalar.
#[derive(Clone, Copy)]
enum Chomp {
    Clip,
    Strip,
    Keep,
}

/// Adds the final line breaks of a block scalar. `trailing` is the number of
/// empty lines after the last line with text.
fn chomp_text(mut body: String, has_text: bool, trailing: usize, chomp: Chomp) -> String {
    match chomp {
        Chomp::Strip => {}
        Chomp::Clip if has_text => body.push('\n'),
        Chomp::Clip => {}
        Chomp::Keep if has_text => body.push_str(&"\n".repeat(trailing + 1)),
        Chomp::Keep => body.push_str(&"\n".repeat(trailing)),
    }
    body
}

fn literal_text(lines: &[&str], chomp: Chomp) -> String {
    let Some(last) = lines.iter().rposition(|line| !line.is_empty()) else {
        return chomp_text(String::new(), false, lines.len(), chomp);
    };
    let body = lines.get(..=last).unwrap_or(&[]).join("\n");
    chomp_text(body, true, lines.len() - last - 1, chomp)
}

fn folded_text(lines: &[&str], chomp: Chomp) -> String {
    let Some(last) = lines.iter().rposition(|line| !line.is_empty()) else {
        return chomp_text(String::new(), false, lines.len(), chomp);
    };
    let more_indented = |line: &str| line.starts_with([' ', '\t']);
    let mut body = String::new();
    let mut previous: Option<&str> = None;
    let mut empty_lines = 0;
    for line in lines.get(..=last).unwrap_or(&[]) {
        if line.is_empty() {
            empty_lines += 1;
            continue;
        }
        match previous {
            None => body.push_str(&"\n".repeat(empty_lines)),
            Some(prev) if !more_indented(prev) && !more_indented(line) => {
                if empty_lines == 0 {
                    body.push(' ');
                } else {
                    body.push_str(&"\n".repeat(empty_lines));
                }
            }
            Some(_) => body.push_str(&"\n".repeat(empty_lines + 1)),
        }
        body.push_str(line);
        previous = Some(line);
        empty_lines = 0;
    }
    chomp_text(body, true, lines.len() - last - 1, chomp)
}
