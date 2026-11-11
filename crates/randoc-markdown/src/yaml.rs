//! The YAML metadata block converter of the Markdown reader.
//!
//! The converter reads the text of a YAML metadata block with
//! `saphyr-parser`, checks it with the rules of Pandoc 3.13, and converts it
//! to metadata values. Each rule comes from experiments with the oracle.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use randoc_ast::{Block, Inline, MetaValue, Text};
use saphyr_parser::{Event, Marker, Parser, ScalarStyle, ScanError, Span, Tag};

/// An error in a YAML metadata block.
pub(crate) struct YamlError {
    /// The full message text, equal to the text Pandoc prints after its error prefix (check with the oracle).
    pub message: Text,
}

/// The fields of a metadata block, in document order.
pub(crate) type MetaFields = Vec<(Text, MetaValue)>;

/// Converts the text between the `---` start line and the `---` or `...` end line of a YAML metadata block.
///
/// `source_name` is the input name for error messages; an empty string means
/// standard input. `first_line` is the 1-based input line number of the first
/// text line (the line after `---`). `max_depth` limits the nesting of YAML
/// collections and of the blocks that `markdown` returns. The text must have
/// its tabs expanded already, as Pandoc expands them before it parses.
///
/// `markdown` parses a string as Markdown blocks. It must parse the string as
/// given, without an added final newline: Pandoc does not read the last line
/// of `"- a"` as a list item.
///
/// Returns `Ok(None)` when the block is valid YAML but its top level is not a
/// mapping or null; Pandoc then reads the lines as ordinary Markdown. Returns
/// the top-level fields otherwise. Keys that end in `_` are left out.
pub(crate) fn parse_yaml_metadata(
    text: &str,
    source_name: &str,
    first_line: u32,
    max_depth: usize,
    markdown: &mut dyn FnMut(&str) -> Vec<Block>,
) -> Result<Option<MetaFields>, YamlError> {
    let errors = ErrorContext {
        text,
        source_name,
        first_line,
    };
    let root = match read_document(text, max_depth) {
        Ok(root) => root,
        Err(problem) => return Err(errors.render(&problem)),
    };
    let mut converter = Converter {
        markdown,
        max_depth,
    };
    match root {
        None => Ok(Some(Vec::new())),
        Some(Node {
            kind: NodeKind::Map(entries),
            ..
        }) => converter
            .fields(entries, "", 0)
            .map(Some)
            .map_err(|problem| errors.render(&problem)),
        Some(node) => match resolve_scalar_node(&node, "") {
            Ok(Some(Scalar::Null)) => Ok(Some(Vec::new())),
            Ok(_) => Ok(None),
            Err(problem) => Err(errors.render(&problem)),
        },
    }
}

/// A position in the YAML text: a 1-based line and a 0-based character column.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pos {
    line: usize,
    col: usize,
}

impl Pos {
    /// Takes the position of a parser marker.
    fn of(marker: &Marker) -> Pos {
        Pos {
            line: marker.line(),
            col: marker.col(),
        }
    }
}

/// One located message of an error.
#[derive(Debug, PartialEq)]
struct Located {
    pos: Pos,
    message: String,
}

/// An error before it gets the source name and the line offset.
#[derive(Debug, PartialEq)]
struct Problem(Vec<Located>);

impl Problem {
    /// Makes an error with one message.
    fn at(pos: Pos, message: impl Into<String>) -> Problem {
        Problem(vec![Located {
            pos,
            message: message.into(),
        }])
    }
}

/// The data that turns a [`Problem`] into the text that Pandoc prints.
struct ErrorContext<'a> {
    text: &'a str,
    source_name: &'a str,
    first_line: u32,
}

impl ErrorContext<'_> {
    /// Formats the error as Pandoc does, without a final newline.
    fn render(&self, problem: &Problem) -> YamlError {
        let start = self.first_line.saturating_sub(1);
        let mut message = if self.source_name.is_empty() {
            format!("Error parsing YAML metadata at (line {start}, column 1):")
        } else {
            format!(
                "Error parsing YAML metadata at {} (line {start}, column 1):",
                quote(self.source_name)
            )
        };
        for located in &problem.0 {
            let line_index = located.pos.line.saturating_sub(1);
            let line = u64::from(self.first_line).saturating_add(line_index as u64);
            let column = located.pos.col.saturating_add(1);
            let number = line.to_string();
            let gutter = " ".repeat(number.len());
            let source_line = self.text.lines().nth(line_index).unwrap_or("");
            message.push_str(&format!(
                "\n{}:{line}:{column}: {}\n{gutter} |\n{number} | {source_line}\n{gutter} | {}^",
                self.source_name,
                located.message,
                " ".repeat(located.pos.col)
            ));
        }
        YamlError { message }
    }
}

/// Quotes a string as Pandoc does in YAML errors.
///
/// Quotes and backslashes get a backslash. ASCII control characters get
/// their Haskell names. Other characters, also non-ASCII ones, stay as they are.
fn quote(text: &str) -> String {
    const NAMES: [&str; 32] = [
        "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "a", "b", "t", "n", "v", "f", "r", "SO",
        "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC",
        "FS", "GS", "RS", "US",
    ];
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{7f}' => out.push_str("\\DEL"),
            _ => match NAMES.get(c as usize) {
                Some(name) => {
                    out.push('\\');
                    out.push_str(name);
                }
                None => out.push(c),
            },
        }
    }
    out.push('"');
    out
}

/// A YAML node after alias expansion.
#[derive(Clone, Debug)]
struct Node {
    pos: Pos,
    kind: NodeKind,
}

/// The content of a YAML node.
#[derive(Clone, Debug)]
enum NodeKind {
    /// A scalar with its style and its tag. `value_pos` is the position after the tag.
    Scalar {
        value: Rc<str>,
        value_pos: Pos,
        style: ScalarStyle,
        tag: Option<Tag>,
    },
    /// A sequence.
    Seq(Vec<Node>),
    /// A mapping, with its entries in document order.
    Map(Vec<(Node, Node)>),
}

/// The number of nodes and characters that aliases can add, when the document itself is smaller.
const ALIAS_LIMIT: usize = 100_000;

/// Reads the first YAML document of `text` into a node tree.
fn read_document(text: &str, max_depth: usize) -> Result<Option<Node>, Problem> {
    if let Some(problem) = check_directive(text) {
        return Err(problem);
    }
    let (size, syntax_ok) = document_size(text);
    let mut reader = Reader {
        text,
        parser: Parser::new_from_str(text),
        anchors: HashMap::new(),
        open_flows: Vec::new(),
        alias_limit: syntax_ok.then_some(size.max(ALIAS_LIMIT)),
        added: 0,
        anchor_names: AnchorNames::new(text),
        max_depth,
    };
    loop {
        let (event, _) = reader.next("")?;
        match event {
            Event::StreamStart | Event::Nothing => {}
            Event::DocumentStart(_) => break,
            _ => return Ok(None),
        }
    }
    let (event, span) = reader.next("")?;
    match event {
        Event::DocumentEnd | Event::StreamEnd => Ok(None),
        event => reader.node(event, span, "", 0).map(|(node, _)| Some(node)),
    }
}

/// Measures the first document before alias expansion, as Pandoc does for its alias limit.
///
/// Returns the size and `false` when the document has a syntax error. An
/// undefined alias is not a syntax error here: Pandoc finds it during alias
/// expansion, in document order with the limit.
fn document_size(text: &str) -> (usize, bool) {
    let mut anchor_names = AnchorNames::new(text);
    let mut size = 0usize;
    for item in Parser::new_from_str(text) {
        match item {
            Ok((Event::DocumentEnd, _)) => break,
            Ok((Event::Alias(_), _)) => size = size.saturating_add(1),
            Ok((event, span)) => {
                size = size.saturating_add(own_size(&event, &span, &mut anchor_names).0);
            }
            Err(error) => return (size, error.info().contains("found unknown anchor")),
        }
    }
    (size, true)
}

/// Returns the size of the node that starts with `event`, without its children, and the length of its anchor name.
///
/// The size is one for the node, plus the characters of its scalar value,
/// its tag and its anchor name. Other events have no size.
fn own_size(event: &Event, span: &Span, anchor_names: &mut AnchorNames) -> (usize, usize) {
    let (value, anchor, tag) = match event {
        Event::Scalar(value, _, anchor, tag) => (value.chars().count(), *anchor, tag.as_deref()),
        Event::SequenceStart(anchor, tag) | Event::MappingStart(anchor, tag) => {
            (0, *anchor, tag.as_deref())
        }
        _ => return (0, 0),
    };
    let anchor = if anchor == 0 {
        0
    } else {
        anchor_names.len_at(span.start.index())
    };
    let tag = match tag {
        Some(tag) if !(tag.handle.is_empty() && tag.suffix == "!") => {
            tag.handle.chars().count() + tag.suffix.chars().count()
        }
        _ => 0,
    };
    (1 + value + anchor + tag, anchor)
}

/// Finds anchor names in the text, because the parser gives an anchor only as a number.
struct AnchorNames<'a> {
    text: &'a str,
    rest: std::str::Chars<'a>,
    index: usize,
}

impl<'a> AnchorNames<'a> {
    fn new(text: &'a str) -> AnchorNames<'a> {
        AnchorNames {
            text,
            rest: text.chars(),
            index: 0,
        }
    }

    /// Returns the number of characters of the anchor name in the node properties at character `start`.
    ///
    /// Calls with increasing `start` take linear time in total.
    fn len_at(&mut self, start: usize) -> usize {
        if start < self.index {
            self.rest = self.text.chars();
            self.index = 0;
        }
        if start > self.index {
            self.rest.nth(start - self.index - 1);
            self.index = start;
        }
        let mut props = self.rest.clone().peekable();
        while props.next_if_eq(&'!').is_some() {
            while props.next_if(|c| !c.is_whitespace()).is_some() {}
            while props.next_if(|c| c.is_whitespace()).is_some() {}
        }
        if props.next_if_eq(&'&').is_none() {
            return 0;
        }
        props
            .take_while(|c| !c.is_whitespace() && !",[]{}".contains(*c))
            .count()
    }
}

/// Rejects a `%` directive line, as Pandoc does.
fn check_directive(text: &str) -> Option<Problem> {
    let (index, line) = text
        .lines()
        .enumerate()
        .find(|(_, line)| !line.trim().is_empty() && !line.trim_start().starts_with('#'))?;
    line.starts_with('%').then(|| {
        Problem::at(
            Pos {
                line: index + 1,
                col: 0,
            },
            "unexpected '%', a directive needs '...' on a line above it to end the document",
        )
    })
}

/// Builds the node tree from the parser events.
struct Reader<'a> {
    text: &'a str,
    parser: Parser<'a, saphyr_parser::StrInput<'a>>,
    anchors: HashMap<usize, (Node, usize)>,
    open_flows: Vec<(Pos, &'static str)>,
    /// The limit for the nodes and characters that aliases add. `None` when
    /// the text has a syntax error: then aliases are not expanded, because
    /// reading stops with an error.
    alias_limit: Option<usize>,
    added: usize,
    anchor_names: AnchorNames<'a>,
    max_depth: usize,
}

impl<'a> Reader<'a> {
    /// Takes the next event. `path` names the current node for alias errors.
    fn next(&mut self, path: &str) -> Result<(Event<'a>, Span), Problem> {
        match self.parser.next_event() {
            Some(Ok(item)) => Ok(item),
            Some(Err(error)) => Err(self.scan_problem(&error, path)),
            None => Err(Problem::at(
                Pos { line: 1, col: 0 },
                "unexpected end of the YAML stream",
            )),
        }
    }

    /// Expands the alias of anchor `id`. Returns the copy and its size.
    ///
    /// Checks the limit before it copies, so the work stays proportional to the limit.
    fn alias(&mut self, id: usize, pos: Pos, path: &str) -> Result<(Node, usize), Problem> {
        let Some((target, size)) = self.anchors.get(&id) else {
            return Err(self.alias_problem(pos, path));
        };
        let Some(limit) = self.alias_limit else {
            let kind = match &target.kind {
                NodeKind::Scalar {
                    value,
                    value_pos,
                    style,
                    ..
                } => NodeKind::Scalar {
                    value: Rc::clone(value),
                    value_pos: *value_pos,
                    style: *style,
                    tag: None,
                },
                _ => NodeKind::Seq(Vec::new()),
            };
            return Ok((Node { pos, kind }, 1));
        };
        let size = *size;
        let added = self.added.saturating_add(size);
        if added > limit {
            let message = format!("the aliases add more than {limit} nodes and characters");
            return Err(Problem::at(pos, with_path(path, &message)));
        }
        let kind = target.kind.clone();
        self.added = added;
        Ok((Node { pos, kind }, size))
    }

    /// Reads the node that starts with `event`. Returns the node and its size for the alias limit.
    fn node(
        &mut self,
        event: Event<'a>,
        span: Span,
        path: &str,
        depth: usize,
    ) -> Result<(Node, usize), Problem> {
        let pos = Pos::of(&span.start);
        if depth > self.max_depth {
            return Err(Problem::at(pos, "maximum nesting depth exceeded"));
        }
        if let Event::Alias(id) = event {
            return self.alias(id, pos, path);
        }
        let (mut size, name) = own_size(&event, &span, &mut self.anchor_names);
        let (anchor, node) = match event {
            Event::Scalar(value, style, anchor, tag) => (
                anchor,
                Node {
                    pos,
                    kind: NodeKind::Scalar {
                        value_pos: if tag.is_some() {
                            self.after_tag(pos)
                        } else {
                            pos
                        },
                        value: Rc::from(value),
                        style,
                        tag: tag.map(Cow::into_owned),
                    },
                },
            ),
            Event::SequenceStart(anchor, tag) => {
                self.check_collection_tag(tag.as_deref(), "seq", "a sequence", pos, path)?;
                self.open_flow(pos, "unterminated flow sequence");
                let mut items = Vec::new();
                loop {
                    let item_path = format!("{path}[{}]", items.len());
                    let (event, span) = self.next(&item_path)?;
                    if matches!(event, Event::SequenceEnd) {
                        break;
                    }
                    let (item, item_size) = self.node(event, span, &item_path, depth + 1)?;
                    size = size.saturating_add(item_size);
                    items.push(item);
                }
                self.close_flow(pos);
                (
                    anchor,
                    Node {
                        pos,
                        kind: NodeKind::Seq(items),
                    },
                )
            }
            Event::MappingStart(anchor, tag) => {
                self.check_collection_tag(tag.as_deref(), "map", "a mapping", pos, path)?;
                self.open_flow(pos, "unterminated flow mapping");
                let mut entries = Vec::new();
                loop {
                    let (event, span) = self.next(path)?;
                    if matches!(event, Event::MappingEnd) {
                        break;
                    }
                    let (key, key_size) = self.node(event, span, path, depth + 1)?;
                    let value_path = child_path(path, &key);
                    let (event, span) = self.next(&value_path)?;
                    let (value, value_size) = self.node(event, span, &value_path, depth + 1)?;
                    size = size.saturating_add(key_size).saturating_add(value_size);
                    entries.push((key, value));
                }
                self.close_flow(pos);
                (
                    anchor,
                    Node {
                        pos,
                        kind: NodeKind::Map(entries),
                    },
                )
            }
            _ => return Err(Problem::at(pos, "unexpected YAML event")),
        };
        if anchor != 0 {
            // An alias copy has no anchor of its own, so Pandoc does not count the name.
            self.anchors
                .insert(anchor, (node.clone(), size.saturating_sub(name)));
        }
        Ok((node, size))
    }

    /// Rejects a core schema scalar tag on a collection, as Pandoc does.
    fn check_collection_tag(
        &self,
        tag: Option<&Tag>,
        own: &str,
        noun: &str,
        pos: Pos,
        path: &str,
    ) -> Result<(), Problem> {
        match tag {
            Some(tag) if tag.handle == CORE_TAG && tag.suffix != own => {
                let message = format!("the tag !!{} cannot be used on {noun}", tag.suffix);
                Err(Problem::at(self.after_tag(pos), with_path(path, &message)))
            }
            _ => Ok(()),
        }
    }

    /// Returns the position after the tag that starts at `pos` and the spaces after it.
    fn after_tag(&self, pos: Pos) -> Pos {
        let rest = self.line_rest(pos);
        let tag = rest.chars().take_while(|c| !c.is_whitespace()).count();
        let spaces = rest.chars().skip(tag).take_while(|c| *c == ' ').count();
        Pos {
            line: pos.line,
            col: pos.col + tag + spaces,
        }
    }

    /// Records the start of a flow collection, for "unterminated" errors.
    fn open_flow(&mut self, pos: Pos, message: &'static str) {
        let first = self
            .text
            .lines()
            .nth(pos.line.saturating_sub(1))
            .and_then(|line| line.chars().nth(pos.col));
        if matches!(first, Some('[' | '{')) {
            self.open_flows.push((pos, message));
        }
    }

    /// Removes the flow collection that starts at `pos`.
    fn close_flow(&mut self, pos: Pos) {
        if self.open_flows.last().is_some_and(|(open, _)| *open == pos) {
            self.open_flows.pop();
        }
    }

    /// Makes the error for an alias without an anchor.
    fn alias_problem(&self, pos: Pos, path: &str) -> Problem {
        let name: String = self
            .line_rest(pos)
            .chars()
            .skip(1)
            .take_while(|c| !c.is_whitespace() && !",[]{}".contains(*c))
            .collect();
        Problem::at(pos, with_path(path, &format!("undefined alias *{name}")))
    }

    /// Returns the text of the line at `pos`, from the column of `pos`.
    fn line_rest(&self, pos: Pos) -> &'a str {
        let line = self
            .text
            .lines()
            .nth(pos.line.saturating_sub(1))
            .unwrap_or("");
        let start = line
            .char_indices()
            .nth(pos.col)
            .map_or(line.len(), |(i, _)| i);
        line.get(start..).unwrap_or("")
    }

    /// Converts a parser error to the message that Pandoc gives.
    fn scan_problem(&self, error: &ScanError, path: &str) -> Problem {
        let pos = Pos::of(error.marker());
        let info = error.info();
        let line = self
            .text
            .lines()
            .nth(pos.line.saturating_sub(1))
            .unwrap_or("");
        let indent = line.chars().take_while(|c| *c == ' ').count();
        let first_char = line.chars().nth(indent);
        let line_start = Pos {
            line: pos.line,
            col: indent,
        };
        let at_pos = self.line_rest(pos).chars().next().unwrap_or(' ');
        if (info.contains("flow sequence")
            || info.contains("flow mapping")
            || info.contains("illegal placement of ':'"))
            && let Some((open, message)) = self.open_flows.last()
        {
            let in_sequence = message.ends_with("sequence");
            return match (at_pos, in_sequence) {
                ('}', true) => Problem::at(pos, "expected ',' or ']'"),
                (']', false) => Problem::at(pos, "expected ',' or '}'"),
                _ => Problem::at(*open, *message),
            };
        }
        if info.contains("misplaced bracket") {
            return Problem::at(
                pos,
                format!("unexpected '{at_pos}' after the end of a flow collection"),
            );
        }
        if info.contains("invalid trailing content after") {
            let before = line.chars().take(pos.col).last();
            let message = if before == Some('"') {
                format!(
                    "unexpected '{at_pos}' after a double-quoted scalar, write \\\" for a quote inside it"
                )
            } else {
                format!("unexpected '{at_pos}' after the end of a quoted scalar")
            };
            return Problem::at(pos, message);
        }
        if info.contains("found unknown anchor") {
            return self.alias_problem(pos, path);
        }
        if info.contains("quoted scalar, found unexpected end of stream") {
            return match self.line_rest(pos).chars().next() {
                Some('\'') => Problem::at(pos, "unterminated single-quoted scalar"),
                _ => Problem::at(pos, "unterminated double-quoted scalar"),
            };
        }
        if info.contains("unknown escape character") {
            let rest = self.line_rest(pos);
            let offset = rest
                .chars()
                .skip(1)
                .position(|c| c == '\\')
                .map_or(0, |i| i + 1);
            return Problem::at(
                Pos {
                    line: pos.line,
                    col: pos.col + offset,
                },
                "invalid escape sequence, write \\\\ for a backslash or use single quotes",
            );
        }
        if info.contains("mapping values are not allowed") {
            let before: String = line.chars().take(pos.col).collect();
            let message = if before.contains(": ") {
                "unexpected ':', quote the value if it contains \": \""
            } else {
                "unexpected ':', this line continues the scalar from the line above, check the indentation and the line above"
            };
            return Problem::at(pos, message);
        }
        if info.contains("did not find expected key") {
            let after_dash = line.chars().nth(indent + 1);
            let message = if first_char == Some('-') && after_dash.is_none_or(|c| c == ' ') {
                "unexpected list item among mapping entries"
            } else {
                "unexpected indentation"
            };
            return Problem::at(line_start, message);
        }
        if info.contains("did not find expected '-'") {
            return Problem::at(line_start, "unexpected key among list items");
        }
        if info.contains("simple key expect") {
            let lines: Vec<&str> = self.text.lines().collect();
            let mut number = pos.line.min(lines.len()).max(1);
            while number > 1 && lines.get(number - 1).is_none_or(|l| l.trim().is_empty()) {
                number -= 1;
            }
            let content = lines.get(number - 1).map_or("", |l| l.trim_end());
            let end = Pos {
                line: number,
                col: content.chars().count(),
            };
            return Problem::at(end, "expected ':' after the key");
        }
        if info.contains("block sequence entries are not allowed") {
            return Problem::at(
                pos,
                "unexpected '-', a list cannot start on the line of its key",
            );
        }
        if let Some(found) = info.strip_prefix("unexpected character: `") {
            let c = found.chars().next().unwrap_or('?');
            return Problem::at(
                pos,
                format!("unexpected '{c}', a plain scalar cannot start with it, quote the value"),
            );
        }
        Problem::at(pos, info)
    }
}

/// Returns the path of the value of `key` in the map at `path`.
fn child_path(path: &str, key: &Node) -> String {
    let name = match &key.kind {
        NodeKind::Scalar { value, .. } => value,
        _ => "?",
    };
    if path.is_empty() {
        name.to_string()
    } else {
        format!("{path}.{name}")
    }
}

/// Puts the node path in front of a message, as Pandoc does.
fn with_path(path: &str, message: &str) -> String {
    if path.is_empty() {
        message.to_string()
    } else {
        format!("{path}: {message}")
    }
}

/// A resolved scalar value.
#[derive(Debug, PartialEq)]
enum Scalar {
    /// A null value.
    Null,
    /// A Boolean value.
    Bool(bool),
    /// A number, already formatted as Pandoc formats it.
    Number(String),
    /// A not-a-number float. Pandoc gives an empty `MetaString`.
    NotANumber,
    /// A string to read as Markdown.
    Str(String),
}

/// The tag handle of the YAML core schema.
const CORE_TAG: &str = "tag:yaml.org,2002:";

/// Resolves a scalar node. Returns `None` for a collection.
fn resolve_scalar_node(node: &Node, path: &str) -> Result<Option<Scalar>, Problem> {
    let NodeKind::Scalar {
        value,
        style,
        tag,
        value_pos,
    } = &node.kind
    else {
        return Ok(None);
    };
    let core = tag
        .as_ref()
        .filter(|tag| tag.handle == CORE_TAG)
        .map(|tag| tag.suffix.as_str());
    let resolved = match (core, tag.is_some()) {
        (Some("str"), _) => Some(Scalar::Str(value.to_string())),
        (Some(suffix @ ("int" | "float" | "bool" | "null")), _) => {
            let scalar = resolve_plain(value).filter(|scalar| match (suffix, scalar) {
                ("int", Scalar::Number(_)) => is_int(value),
                ("float", Scalar::Number(_) | Scalar::NotANumber) => true,
                ("bool", Scalar::Bool(_)) | ("null", Scalar::Null) => true,
                _ => false,
            });
            if scalar.is_none() {
                let message = with_path(path, &format!("invalid value for the tag !!{suffix}"));
                return Err(Problem::at(*value_pos, message));
            }
            scalar
        }
        (None, false) if *style == ScalarStyle::Plain => resolve_plain(value),
        _ => Some(Scalar::Str(value.to_string())),
    };
    Ok(Some(
        resolved.unwrap_or_else(|| Scalar::Str(value.to_string())),
    ))
}

/// Resolves a plain scalar with the YAML 1.2 core schema. Returns `None` for a string.
fn resolve_plain(value: &str) -> Option<Scalar> {
    match value {
        "" | "~" | "null" | "Null" | "NULL" => return Some(Scalar::Null),
        "true" | "True" | "TRUE" => return Some(Scalar::Bool(true)),
        "false" | "False" | "FALSE" => return Some(Scalar::Bool(false)),
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => {
            return Some(Scalar::Number("+inf".into()));
        }
        "-.inf" | "-.Inf" | "-.INF" => return Some(Scalar::Number("-inf".into())),
        ".nan" | ".NaN" | ".NAN" => return Some(Scalar::NotANumber),
        _ => {}
    }
    parse_number(value).map(|number| Scalar::Number(number.format()))
}

/// Tells whether a scalar has the form of a core schema integer.
fn is_int(value: &str) -> bool {
    let unsigned = value.strip_prefix(['-', '+']).unwrap_or(value);
    let octal = value.strip_prefix("0o");
    let hex = value.strip_prefix("0x");
    (!unsigned.is_empty() && unsigned.bytes().all(|b| b.is_ascii_digit()))
        || octal.is_some_and(|d| !d.is_empty() && d.bytes().all(|b| (b'0'..=b'7').contains(&b)))
        || hex.is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// An exact decimal number: `0.d1 d2 ... dn` times `10^exponent`.
#[derive(Debug, PartialEq)]
struct Decimal {
    negative: bool,
    /// The digits, with no zero at the start or the end. Empty for zero.
    digits: Vec<u8>,
    exponent: i64,
}

/// Parses a core schema integer or float.
fn parse_number(value: &str) -> Option<Decimal> {
    if let Some(octal) = value.strip_prefix("0o") {
        return from_radix(octal, 8);
    }
    if let Some(hex) = value.strip_prefix("0x") {
        return from_radix(hex, 16);
    }
    let (negative, rest) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value.strip_prefix('+').unwrap_or(value)),
    };
    let (mantissa, exponent_text) = match rest.find(['e', 'E']) {
        Some(i) => (rest.get(..i)?, Some(rest.get(i + 1..)?)),
        None => (rest, None),
    };
    let (whole, fraction) = match mantissa.find('.') {
        Some(i) => (mantissa.get(..i)?, mantissa.get(i + 1..)?),
        None => (mantissa, ""),
    };
    let digits_only = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if !digits_only(whole) || !digits_only(fraction) || (whole.is_empty() && fraction.is_empty()) {
        return None;
    }
    let mut exponent: i64 = match exponent_text {
        None => 0,
        Some(text) => {
            let (sign, digits) = match text.strip_prefix('-') {
                Some(d) => (-1, d),
                None => (1, text.strip_prefix('+').unwrap_or(text)),
            };
            if digits.is_empty() || !digits_only(digits) {
                return None;
            }
            digits.bytes().fold(0i64, |acc, b| {
                acc.saturating_mul(10).saturating_add(i64::from(b - b'0'))
            }) * sign
        }
    };
    exponent = exponent.saturating_add(whole.len() as i64);
    let mut digits: Vec<u8> = whole
        .bytes()
        .chain(fraction.bytes())
        .map(|b| b - b'0')
        .collect();
    let leading = digits.iter().take_while(|d| **d == 0).count();
    digits.drain(..leading);
    exponent = exponent.saturating_sub(leading as i64);
    while digits.last() == Some(&0) {
        digits.pop();
    }
    Some(Decimal::new(negative, digits, exponent))
}

/// Converts the digits of an octal or hexadecimal integer.
fn from_radix(text: &str, radix: u32) -> Option<Decimal> {
    if text.is_empty() {
        return None;
    }
    let mut decimal: Vec<u8> = Vec::new();
    for c in text.chars() {
        let mut carry = c.to_digit(radix)?;
        for digit in decimal.iter_mut().rev() {
            let total = u32::from(*digit) * radix + carry;
            *digit = (total % 10) as u8;
            carry = total / 10;
        }
        while carry > 0 {
            decimal.insert(0, (carry % 10) as u8);
            carry /= 10;
        }
    }
    let leading = decimal.iter().take_while(|d| **d == 0).count();
    decimal.drain(..leading);
    let exponent = decimal.len() as i64;
    while decimal.last() == Some(&0) {
        decimal.pop();
    }
    Some(Decimal::new(false, decimal, exponent))
}

impl Decimal {
    /// Makes a number; zero gets exponent 0 and no sign.
    fn new(negative: bool, digits: Vec<u8>, exponent: i64) -> Decimal {
        if digits.is_empty() {
            Decimal {
                negative: false,
                digits,
                exponent: 0,
            }
        } else {
            Decimal {
                negative,
                digits,
                exponent,
            }
        }
    }

    /// Formats the number as Pandoc does.
    ///
    /// An integer in the range of a 64-bit signed integer gets plain digits.
    /// Other numbers get the Haskell `Scientific` format: fixed notation when
    /// `0.1 <= |x| < 10^7`, exponent notation otherwise.
    fn format(&self) -> String {
        if self.digits.is_empty() {
            return "0".into();
        }
        let sign = if self.negative { "-" } else { "" };
        let text: String = self.digits.iter().map(|d| char::from(b'0' + d)).collect();
        let count = self.digits.len() as i64;
        if count <= self.exponent && self.fits_i64() {
            let zeros = usize::try_from(self.exponent - count).unwrap_or(0);
            return format!("{sign}{text}{}", "0".repeat(zeros));
        }
        if self.exponent < 0 || self.exponent > 7 {
            let first = text.get(..1).unwrap_or("0");
            let rest = text.get(1..).unwrap_or("");
            let rest = if rest.is_empty() { "0" } else { rest };
            return format!("{sign}{first}.{rest}e{}", self.exponent.saturating_sub(1));
        }
        let point = usize::try_from(self.exponent).unwrap_or(0);
        if point == 0 {
            return format!("{sign}0.{text}");
        }
        let whole = text.get(..point).unwrap_or("");
        let fraction = text.get(point..).unwrap_or("");
        let fraction = if fraction.is_empty() { "0" } else { fraction };
        format!(
            "{sign}{whole}.{}{fraction}",
            "0".repeat(point.saturating_sub(text.len()))
        )
    }

    /// Tells whether the integer value fits in a 64-bit signed integer.
    fn fits_i64(&self) -> bool {
        if self.exponent > 19 {
            return false;
        }
        let mut value: i128 = 0;
        for i in 0..self.exponent {
            let digit = self.digits.get(i as usize).copied().unwrap_or(0);
            value = value * 10 + i128::from(digit);
        }
        if self.negative {
            value = -value;
        }
        i128::from(i64::MIN) <= value && value <= i128::from(i64::MAX)
    }
}

/// Converts the node tree to metadata values.
struct Converter<'m> {
    markdown: &'m mut dyn FnMut(&str) -> Vec<Block>,
    max_depth: usize,
}

impl Converter<'_> {
    /// Converts the entries of a mapping. Checks the keys and drops keys that end in `_`.
    fn fields(
        &mut self,
        entries: Vec<(Node, Node)>,
        path: &str,
        depth: usize,
    ) -> Result<MetaFields, Problem> {
        let mut seen: HashMap<String, Pos> = HashMap::new();
        let mut fields = Vec::new();
        for (key, value) in entries {
            let name = match &key.kind {
                NodeKind::Scalar { value, .. } => value.to_string(),
                NodeKind::Seq(_) => {
                    return Err(Problem::at(
                        key.pos,
                        "expected a scalar key, but got a list",
                    ));
                }
                NodeKind::Map(_) => {
                    return Err(Problem::at(
                        key.pos,
                        "expected a scalar key, but got a mapping",
                    ));
                }
            };
            if let Some(first) = seen.get(&name) {
                return Err(Problem(vec![
                    Located {
                        pos: key.pos,
                        message: format!("duplicate key {}", quote(&name)),
                    },
                    Located {
                        pos: *first,
                        message: format!("the first key {}", quote(&name)),
                    },
                ]));
            }
            seen.insert(name.clone(), key.pos);
            let value_path = child_path(path, &key);
            let converted = self.value(value, &value_path, depth + 1)?;
            if !name.ends_with('_') {
                fields.push((name, converted));
            }
        }
        Ok(fields)
    }

    /// Converts one node.
    fn value(&mut self, node: Node, path: &str, depth: usize) -> Result<MetaValue, Problem> {
        if depth > self.max_depth {
            return Err(Problem::at(node.pos, "maximum nesting depth exceeded"));
        }
        if let Some(scalar) = resolve_scalar_node(&node, path)? {
            return self.scalar(scalar, node.pos);
        }
        match node.kind {
            NodeKind::Seq(items) => items
                .into_iter()
                .enumerate()
                .map(|(i, item)| self.value(item, &format!("{path}[{i}]"), depth + 1))
                .collect::<Result<_, _>>()
                .map(MetaValue::MetaList),
            NodeKind::Map(entries) => Ok(MetaValue::MetaMap(
                self.fields(entries, path, depth)?
                    .into_iter()
                    .collect::<BTreeMap<_, _>>(),
            )),
            NodeKind::Scalar { .. } => Ok(MetaValue::MetaString(Text::new())),
        }
    }

    /// Converts a resolved scalar.
    fn scalar(&mut self, scalar: Scalar, pos: Pos) -> Result<MetaValue, Problem> {
        Ok(match scalar {
            Scalar::Null | Scalar::NotANumber => MetaValue::MetaString(Text::new()),
            Scalar::Bool(value) => MetaValue::MetaBool(value),
            Scalar::Number(text) => MetaValue::MetaInlines(vec![Inline::Str(text)]),
            Scalar::Str(text) => {
                let trimmed_end = text.trim_end_matches([' ', '\t']);
                if trimmed_end.ends_with('\n') {
                    MetaValue::MetaBlocks((self.markdown)(&text))
                } else {
                    let start = text.trim_start_matches([' ', '\t', '\n', '\r']);
                    let blocks = (self.markdown)(start);
                    let inlines = blocks_to_inlines(&blocks, self.max_depth)
                        .ok_or_else(|| Problem::at(pos, "maximum nesting depth exceeded"))?;
                    MetaValue::MetaInlines(inlines)
                }
            }
        })
    }
}

/// Converts blocks to inlines as Pandoc does for a metadata string without a final newline.
///
/// Blocks are joined with `LineBreak`. List items, definitions and table
/// cells are joined with nothing. Returns `None` when the blocks nest deeper
/// than `max_depth`.
fn blocks_to_inlines(blocks: &[Block], max_depth: usize) -> Option<Vec<Inline>> {
    let mut out = Vec::new();
    join_blocks(&mut out, blocks, max_depth)?;
    Some(out)
}

/// Appends the inlines of `blocks`, joined with `LineBreak`.
fn join_blocks(out: &mut Vec<Inline>, blocks: &[Block], depth: usize) -> Option<()> {
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            append(out, vec![Inline::LineBreak]);
        }
        block_inlines(out, block, depth)?;
    }
    Some(())
}

/// Appends the inlines of one block.
fn block_inlines(out: &mut Vec<Inline>, block: &Block, depth: usize) -> Option<()> {
    let depth = depth.checked_sub(1)?;
    match block {
        Block::Plain(inlines) | Block::Para(inlines) | Block::Header(_, _, inlines) => {
            append(out, inlines.clone())
        }
        Block::LineBlock(lines) => {
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    append(out, vec![Inline::LineBreak]);
                }
                append(out, line.clone());
            }
        }
        Block::CodeBlock(attr, text) => append(out, vec![Inline::Code(attr.clone(), text.clone())]),
        Block::RawBlock(format, text) => {
            append(out, vec![Inline::RawInline(format.clone(), text.clone())])
        }
        Block::BlockQuote(blocks) | Block::Div(_, blocks) | Block::Figure(_, _, blocks) => {
            join_blocks(out, blocks, depth)?
        }
        Block::BulletList(items) | Block::OrderedList(_, items) => {
            for item in items {
                append_part(out, item, depth)?;
            }
        }
        Block::DefinitionList(items) => {
            for (term, definitions) in items {
                append(out, term.clone());
                append(out, vec![Inline::Str(":".into()), Inline::Space]);
                for definition in definitions {
                    append_part(out, definition, depth)?;
                }
            }
        }
        Block::HorizontalRule => {}
        Block::Table(table) => {
            let rows = table
                .head
                .rows
                .iter()
                .chain(
                    table
                        .bodies
                        .iter()
                        .flat_map(|body| body.head.iter().chain(&body.body)),
                )
                .chain(&table.foot.rows);
            for (i, row) in rows.enumerate() {
                if i > 0 {
                    append(out, vec![Inline::LineBreak]);
                }
                for cell in &row.cells {
                    append_part(out, &cell.content, depth)?;
                }
            }
        }
    }
    Some(())
}

/// Appends the inlines of a group of blocks as one unit, so that merges happen only at its edges.
fn append_part(out: &mut Vec<Inline>, blocks: &[Block], depth: usize) -> Option<()> {
    let mut part = Vec::new();
    join_blocks(&mut part, blocks, depth)?;
    append(out, part);
    Some(())
}

/// Appends inlines with the merge rules of the `pandoc-types` builder at the join point.
fn append(out: &mut Vec<Inline>, items: Vec<Inline>) {
    let mut items = items.into_iter();
    let Some(first) = items.next() else {
        return;
    };
    match out.pop() {
        Some(last) => out.extend(merge(last, first)),
        None => out.push(first),
    }
    out.extend(items);
}

/// Merges the two inlines at a join point, or keeps both.
fn merge(left: Inline, right: Inline) -> Vec<Inline> {
    use Inline::*;
    match (left, right) {
        (Space, Space) => vec![Space],
        (Space | SoftBreak, SoftBreak) | (SoftBreak, Space) => vec![SoftBreak],
        (Space | SoftBreak, LineBreak) | (LineBreak, Space | SoftBreak) => {
            vec![LineBreak]
        }
        (Str(a), Str(b)) => vec![Str(a + &b)],
        (Emph(mut a), Emph(b)) => {
            a.extend(b);
            vec![Emph(a)]
        }
        (Underline(mut a), Underline(b)) => {
            a.extend(b);
            vec![Underline(a)]
        }
        (Strong(mut a), Strong(b)) => {
            a.extend(b);
            vec![Strong(a)]
        }
        (Strikeout(mut a), Strikeout(b)) => {
            a.extend(b);
            vec![Strikeout(a)]
        }
        (Superscript(mut a), Superscript(b)) => {
            a.extend(b);
            vec![Superscript(a)]
        }
        (Subscript(mut a), Subscript(b)) => {
            a.extend(b);
            vec![Subscript(a)]
        }
        (left, right) => vec![left, right],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use randoc_ast::Attr;

    fn s(text: &str) -> Inline {
        Inline::Str(text.into())
    }

    /// One paragraph of one `Str`, or one paragraph for each part between blank lines.
    fn fake(text: &str) -> Vec<Block> {
        text.split("\n\n")
            .filter(|part| !part.trim().is_empty())
            .map(|part| Block::Para(vec![s(part.trim())]))
            .collect()
    }

    fn parse(text: &str) -> Result<Option<MetaFields>, String> {
        parse_yaml_metadata(text, "", 2, 50, &mut fake).map_err(|e| e.message)
    }

    fn field(text: &str) -> MetaValue {
        match parse(text) {
            Ok(Some(mut fields)) if fields.len() == 1 => fields.remove(0).1,
            other => panic!("unexpected result {other:?}"),
        }
    }

    fn inlines(text: &str) -> MetaValue {
        MetaValue::MetaInlines(vec![s(text)])
    }

    #[test]
    fn plain_string_is_inlines() {
        assert_eq!(field("t: Hello\n"), inlines("Hello"));
    }

    #[test]
    fn string_with_final_newline_is_blocks() {
        let expected = MetaValue::MetaBlocks(vec![
            Block::Para(vec![s("Para one.")]),
            Block::Para(vec![s("Para two.")]),
        ]);
        assert_eq!(field("a: |\n  Para one.\n\n  Para two.\n"), expected);
        assert_eq!(
            field("a: \"x\\n \"\n"),
            MetaValue::MetaBlocks(vec![Block::Para(vec![s("x")])])
        );
    }

    #[test]
    fn several_paragraphs_without_final_newline_join_with_line_break() {
        let expected = MetaValue::MetaInlines(vec![s("a"), Inline::LineBreak, s("b")]);
        assert_eq!(field("t: \"a\\n\\nb\"\n"), expected);
        assert_eq!(field("t: |-\n  a\n\n  b\n"), expected);
    }

    #[test]
    fn inline_string_loses_leading_space_only() {
        let mut seen = Vec::new();
        let mut record = |text: &str| {
            seen.push(text.to_string());
            Vec::new()
        };
        let result = parse_yaml_metadata("t: \"  x  \"\n", "", 2, 50, &mut record);
        assert!(result.is_ok());
        assert_eq!(seen, vec!["x  ".to_string()]);
    }

    #[test]
    fn empty_quoted_string_is_empty_inlines() {
        assert_eq!(field("t: \"\"\n"), MetaValue::MetaInlines(vec![]));
    }

    #[test]
    fn core_schema_booleans_and_nulls() {
        for text in ["true", "True", "TRUE"] {
            assert_eq!(field(&format!("a: {text}\n")), MetaValue::MetaBool(true));
        }
        assert_eq!(field("a: FALSE\n"), MetaValue::MetaBool(false));
        for text in ["yes", "no", "On"] {
            assert_eq!(field(&format!("a: {text}\n")), inlines(text));
        }
        for text in ["null", "Null", "NULL", "~", "", ".nan"] {
            let value = field(&format!("a: {text}\n"));
            assert_eq!(value, MetaValue::MetaString(Text::new()), "{text}");
        }
        assert_eq!(field("a: \"true\"\n"), inlines("true"));
    }

    #[test]
    fn numbers_use_pandoc_format() {
        let cases = [
            ("42", "42"),
            ("1e3", "1000"),
            ("0x10", "16"),
            ("0o17", "15"),
            ("3.14", "3.14"),
            ("-7", "-7"),
            ("1.0", "1"),
            ("1.50", "1.5"),
            ("1e-3", "1.0e-3"),
            ("0.1", "0.1"),
            ("0.01", "1.0e-2"),
            ("1.5e10", "15000000000"),
            ("100000000000000000000", "1.0e20"),
            ("9223372036854775807", "9223372036854775807"),
            ("9223372036854775808", "9.223372036854775808e18"),
            ("-9223372036854775808", "-9223372036854775808"),
            ("1234567.5", "1234567.5"),
            ("12345678.5", "1.23456785e7"),
            ("-0", "0"),
            ("+5", "5"),
            (".5", "0.5"),
            ("1.", "1"),
            ("007", "7"),
            ("1e400", "1.0e400"),
            (".inf", "+inf"),
            ("-.inf", "-inf"),
            ("1_000", "1_000"),
            ("0b101", "0b101"),
            ("0o8", "0o8"),
            ("2024-01-02", "2024-01-02"),
        ];
        for (text, expected) in cases {
            assert_eq!(field(&format!("a: {text}\n")), inlines(expected), "{text}");
        }
    }

    #[test]
    fn tags_change_resolution() {
        assert_eq!(field("a: !!str 42\n"), inlines("42"));
        assert_eq!(field("a: !!int \"42\"\n"), inlines("42"));
        assert_eq!(field("a: !!float 3\n"), inlines("3"));
        assert_eq!(field("a: !!bool \"true\"\n"), MetaValue::MetaBool(true));
        assert_eq!(field("a: ! 42\n"), inlines("42"));
        assert_eq!(field("a: !foo bar\n"), inlines("bar"));
        let error = parse("a: !!int abc\n").err().unwrap_or_default();
        assert!(
            error.ends_with(
                ":2:10: a: invalid value for the tag !!int\n  |\n2 | a: !!int abc\n  |          ^"
            ),
            "{error}"
        );
        let error = parse("a: !!str [1]\n").err().unwrap_or_default();
        assert!(
            error.contains(":2:10: a: the tag !!str cannot be used on a sequence"),
            "{error}"
        );
    }

    #[test]
    fn collections_and_aliases() {
        let expected = MetaValue::MetaList(vec![
            inlines("one"),
            inlines("2"),
            MetaValue::MetaBool(true),
        ]);
        assert_eq!(field("l: [one, 2, true]\n"), expected);
        let map = MetaValue::MetaMap(BTreeMap::from([("k".to_string(), inlines("v"))]));
        assert_eq!(
            parse("a: &A\n  k: v\nb: *A\n"),
            Ok(Some(vec![("a".into(), map.clone()), ("b".into(), map)]))
        );
        assert_eq!(field("m: {}\n"), MetaValue::MetaMap(BTreeMap::new()));
    }

    #[test]
    fn keys_ending_in_underscore_are_dropped_and_keys_keep_their_text() {
        let fields = parse("draft_: x\n_: y\nkeep: z\nm:\n  in_: 1\n  1.50: f\n  ~: n\n");
        let nested = MetaValue::MetaMap(BTreeMap::from([
            ("1.50".to_string(), inlines("f")),
            ("~".to_string(), inlines("n")),
        ]));
        assert_eq!(
            fields,
            Ok(Some(vec![
                ("keep".into(), inlines("z")),
                ("m".into(), nested)
            ]))
        );
    }

    #[test]
    fn top_level_that_is_not_a_map() {
        assert_eq!(parse("just a string\n"), Ok(None));
        assert_eq!(parse("- a\n- b\n"), Ok(None));
        assert_eq!(parse("[]\n"), Ok(None));
        assert_eq!(parse("~\n"), Ok(Some(vec![])));
        assert_eq!(parse("# comment\n"), Ok(Some(vec![])));
        assert_eq!(parse(""), Ok(Some(vec![])));
    }

    #[test]
    fn syntax_error_text_matches_pandoc() {
        assert_eq!(
            parse("title: [unclosed\n"),
            Err("Error parsing YAML metadata at (line 1, column 1):\n:2:8: unterminated flow sequence\n  |\n2 | title: [unclosed\n  |        ^".into())
        );
        let error = parse_yaml_metadata("a: b: c\n", "doc.md", 4, 50, &mut fake)
            .err()
            .map(|e| e.message);
        assert_eq!(
            error.as_deref(),
            Some(
                "Error parsing YAML metadata at \"doc.md\" (line 3, column 1):\ndoc.md:4:5: unexpected ':', quote the value if it contains \": \"\n  |\n4 | a: b: c\n  |     ^"
            )
        );
    }

    #[test]
    fn mapped_syntax_errors() {
        let cases = [
            ("a: \"open\n", ":2:4: unterminated double-quoted scalar"),
            ("a: 'open\n", ":2:4: unterminated single-quoted scalar"),
            ("a:\n  - x\n y: z\n", ":4:2: unexpected indentation"),
            (
                "a: 1\n- b\n",
                ":3:1: unexpected list item among mapping entries",
            ),
            ("- a\nb: c\n", ":3:1: unexpected key among list items"),
            ("m:\n  k: *nope\n", ":3:6: m.k: undefined alias *nope"),
            ("l:\n  - *nope\n", ":3:5: l[0]: undefined alias *nope"),
            ("a: |\nnot indented\n", ":3:13: expected ':' after the key"),
            ("{a: 1\n", ":2:1: unterminated flow mapping"),
            (
                "a: 1\n  b: 2\n",
                ":3:4: unexpected ':', this line continues",
            ),
            (
                "a: @x\n",
                ":2:4: unexpected '@', a plain scalar cannot start with it",
            ),
            ("a: \"x\\q\"\n", ":2:6: invalid escape sequence"),
            (
                "a: -\n",
                ":2:4: unexpected '-', a list cannot start on the line of its key",
            ),
            (
                "a: [1, 2]]\n",
                ":2:10: unexpected ']' after the end of a flow collection",
            ),
            ("a: {x: [1, 2}\n", ":2:13: expected ',' or ']'"),
            (
                "a: \"x\"y\n",
                ":2:7: unexpected 'y' after a double-quoted scalar",
            ),
            (
                "%YAML 1.2\n---\na: 1\n",
                ":2:1: unexpected '%', a directive needs",
            ),
            (
                "? [a]\n: x\n",
                ":2:3: expected a scalar key, but got a list",
            ),
            (
                "? {a: 1}\n: x\n",
                ":2:3: expected a scalar key, but got a mapping",
            ),
        ];
        for (text, expected) in cases {
            let error = parse(text).err().unwrap_or_default();
            assert!(error.contains(expected), "{text:?} gave {error}");
        }
    }

    #[test]
    fn duplicate_key_names_both_keys() {
        assert_eq!(
            parse("a: 1\n\"a\": 2\n"),
            Err("Error parsing YAML metadata at (line 1, column 1):\n:3:1: duplicate key \"a\"\n  |\n3 | \"a\": 2\n  | ^\n:2:1: the first key \"a\"\n  |\n2 | a: 1\n  | ^".into())
        );
    }

    #[test]
    fn depth_limit_stops_deep_nesting() {
        let text = format!("a: {}{}\n", "[".repeat(80), "]".repeat(80));
        let error = parse(&text).err().unwrap_or_default();
        assert!(error.contains("maximum nesting depth exceeded"), "{error}");
        let deep = (0..5).fold(vec![Block::Para(vec![s("x")])], |inner, _| {
            vec![Block::BlockQuote(inner)]
        });
        assert_eq!(blocks_to_inlines(&deep, 3), None);
        assert_eq!(blocks_to_inlines(&deep, 10), Some(vec![s("x")]));
    }

    #[test]
    fn alias_limit_counts_nodes() {
        let mut text = String::from("a: &a [x,x,x,x,x,x,x,x,x,x]\n");
        for (name, target) in [("b", "a"), ("c", "b"), ("d", "c"), ("e", "d")] {
            let items = vec![format!("*{target}"); 10].join(",");
            text.push_str(&format!("{name}: &{name} [{items}]\n"));
        }
        assert_eq!(
            parse(&text),
            Err("Error parsing YAML metadata at (line 1, column 1):\n:6:17: e[3]: the aliases add more than 100000 nodes and characters\n  |\n6 | e: &e [*d,*d,*d,*d,*d,*d,*d,*d,*d,*d]\n  |                 ^".into())
        );
    }

    #[test]
    fn alias_limit_counts_characters() {
        let text = format!(
            "a: &a {}\nb: [{}]\n",
            "x".repeat(10000),
            ["*a"; 11].join(",")
        );
        assert_eq!(
            parse(&text),
            Err("Error parsing YAML metadata at (line 1, column 1):\n:3:32: b[9]: the aliases add more than 100000 nodes and characters\n  |\n3 | b: [*a,*a,*a,*a,*a,*a,*a,*a,*a,*a,*a]\n  |                                ^".into())
        );
    }

    #[test]
    fn alias_limit_is_the_document_size_when_that_is_larger() {
        let text = format!("a: &a {}\nb: [*a, *a, *a]\n", "x".repeat(100_000));
        let error = parse(&text).err().unwrap_or_default();
        assert_eq!(
            error.lines().nth(1),
            Some(":3:9: b[1]: the aliases add more than 100011 nodes and characters")
        );
    }

    #[test]
    fn deep_doubling_aliases_stop_quickly() {
        let mut text = String::from("a0: &a0 [x,x]\n");
        for i in 1..64 {
            let p = i - 1;
            text.push_str(&format!("a{i}: &a{i} [*a{p}, *a{p}]\n"));
        }
        let start = std::time::Instant::now();
        let error = parse(&text).err().unwrap_or_default();
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        assert_eq!(
            error,
            "Error parsing YAML metadata at (line 1, column 1):\n:16:12: a14[0]: the aliases add more than 100000 nodes and characters\n   |\n16 | a14: &a14 [*a13, *a13]\n   |            ^"
        );
    }

    #[test]
    fn aliases_just_under_the_limit_expand() {
        let long = "x".repeat(9999);
        let text = format!("a: &a {long}\nb: [{}]\n", ["*a"; 10].join(","));
        let fields = parse(&text).ok().flatten().unwrap_or_default();
        assert_eq!(
            fields.get(1),
            Some(&("b".into(), MetaValue::MetaList(vec![inlines(&long); 10])))
        );
    }

    #[test]
    fn blocks_to_inlines_rules() {
        let list = Block::BulletList(vec![
            vec![Block::Plain(vec![s("one")])],
            vec![Block::Plain(vec![Inline::Emph(vec![s("a")])])],
            vec![Block::Plain(vec![Inline::Emph(vec![s("b")])])],
        ]);
        let blocks = vec![
            list,
            Block::HorizontalRule,
            Block::HorizontalRule,
            Block::CodeBlock(Attr::default(), "c".into()),
            Block::DefinitionList(vec![(
                vec![s("Term")],
                vec![
                    vec![Block::Plain(vec![s("d1")])],
                    vec![Block::Plain(vec![s("d2")])],
                ],
            )]),
            Block::Para(vec![s("z"), Inline::Space]),
            Block::Para(vec![Inline::SmallCaps(vec![s("p")])]),
            Block::Para(vec![Inline::SmallCaps(vec![s("q")])]),
        ];
        let expected = vec![
            s("one"),
            Inline::Emph(vec![s("a"), s("b")]),
            Inline::LineBreak,
            Inline::LineBreak,
            Inline::LineBreak,
            Inline::Code(Attr::default(), "c".into()),
            Inline::LineBreak,
            s("Term:"),
            Inline::Space,
            s("d1d2"),
            Inline::LineBreak,
            s("z"),
            Inline::LineBreak,
            Inline::SmallCaps(vec![s("p")]),
            Inline::LineBreak,
            Inline::SmallCaps(vec![s("q")]),
        ];
        assert_eq!(blocks_to_inlines(&blocks, 10), Some(expected));
    }

    #[test]
    fn quote_escapes_like_pandoc() {
        assert_eq!(quote("é\"\\\u{1}\u{7f}"), "\"é\\\"\\\\\\SOH\\DEL\"");
    }
}
