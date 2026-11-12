//! A small YAML tree on top of the `saphyr-parser` event parser.
//!
//! The tree follows the rules of the YAML library of Pandoc: the core schema
//! for plain scalars, no duplicate keys, scalar keys only, and a limit on the
//! nodes and characters that aliases add.

use std::collections::HashMap;

use saphyr_parser::{Event, Parser, ScalarStyle, Span};

/// A position in a YAML text. Lines and columns count from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mark {
    /// The line.
    pub line: usize,
    /// The column, in characters.
    pub column: usize,
}

/// A YAML node.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// A scalar. `plain` is true when the scalar has no quotes and no block style.
    Scalar {
        /// The text.
        text: String,
        /// True for a plain scalar.
        plain: bool,
        /// The position.
        mark: Mark,
    },
    /// A sequence.
    Seq(Vec<Node>, Mark),
    /// A mapping in source order.
    Map(Vec<(Node, Node)>, Mark),
}

/// A YAML parse error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlError {
    /// The message.
    pub message: String,
    /// The position.
    pub mark: Mark,
    /// A second position with its text, for example the first of two equal keys.
    pub note: Option<(Mark, String)>,
}

impl YamlError {
    fn new(message: impl Into<String>, mark: Mark) -> Self {
        YamlError {
            message: message.into(),
            mark,
            note: None,
        }
    }
}

/// The value of a scalar by the YAML core schema.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    /// `null`, `~` or an empty plain scalar.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// An integer that fits in 64 bits.
    Int(i64),
    /// Another number, in the text form of Pandoc, for example `1.5e-7` or `+inf`.
    Number(String),
    /// `.nan`.
    NaN,
    /// A string.
    Str(String),
}

/// The value of the YAML library of Pandoc: aliases can add this many nodes
/// and characters, or as many as the document has, when that is more.
const ALIAS_BUDGET: u64 = 100_000;

impl Node {
    /// Returns the position of the node.
    pub fn mark(&self) -> Mark {
        match self {
            Node::Scalar { mark, .. } | Node::Seq(_, mark) | Node::Map(_, mark) => *mark,
        }
    }

    /// Returns the text of a scalar.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Node::Scalar { text, .. } => Some(text),
            _ => None,
        }
    }

    /// Returns the value of a scalar by the core schema. A quoted scalar is a string.
    pub fn resolve(&self) -> Option<Resolved> {
        match self {
            Node::Scalar {
                text, plain: true, ..
            } => Some(resolve_plain(text)),
            Node::Scalar { text, .. } => Some(Resolved::Str(text.clone())),
            _ => None,
        }
    }

    /// Returns true for a plain scalar that the YAML core schema reads as null.
    pub fn is_null(&self) -> bool {
        self.resolve() == Some(Resolved::Null)
    }

    /// Returns the Boolean value of a plain scalar.
    pub fn as_bool(&self) -> Option<bool> {
        match self.resolve()? {
            Resolved::Bool(b) => Some(b),
            _ => None,
        }
    }

    /// Returns the integer value of a plain scalar.
    pub fn as_int(&self) -> Option<i64> {
        match self.resolve()? {
            Resolved::Int(n) => Some(n),
            _ => None,
        }
    }

    /// Returns true for a plain scalar that the YAML core schema reads as a number.
    pub fn is_number(&self) -> bool {
        matches!(
            self.resolve(),
            Some(Resolved::Int(_) | Resolved::Number(_) | Resolved::NaN)
        )
    }

    /// Returns the text of a scalar as Pandoc prints a YAML value as a string:
    /// a number in its normal form, null as an empty string.
    pub fn scalar_text(&self) -> Option<String> {
        Some(match self.resolve()? {
            Resolved::Null | Resolved::NaN => String::new(),
            Resolved::Bool(b) => b.to_string(),
            Resolved::Int(n) => n.to_string(),
            Resolved::Number(text) | Resolved::Str(text) => text,
        })
    }

    /// Returns the name of the JSON type of the node, as Aeson prints it.
    pub fn type_name(&self) -> &'static str {
        match self {
            Node::Seq(..) => "Array",
            Node::Map(..) => "Object",
            node => match node.resolve() {
                Some(Resolved::Null) => "Null",
                Some(Resolved::Bool(_)) => "Boolean",
                Some(Resolved::Int(_) | Resolved::Number(_) | Resolved::NaN) => "Number",
                _ => "String",
            },
        }
    }

    /// Converts the node to a JSON value with the YAML core schema.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value;
        match self {
            Node::Seq(items, _) => Value::Array(items.iter().map(Node::to_json).collect()),
            Node::Map(entries, _) => Value::Object(
                entries
                    .iter()
                    .map(|(k, v)| (k.scalar_text().unwrap_or_default(), v.to_json()))
                    .collect(),
            ),
            node => match node.resolve() {
                Some(Resolved::Null | Resolved::NaN) | None => Value::Null,
                Some(Resolved::Bool(b)) => Value::Bool(b),
                Some(Resolved::Int(n)) => Value::from(n),
                Some(Resolved::Number(text)) => text
                    .parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map_or(Value::String(text), Value::Number),
                Some(Resolved::Str(text)) => Value::String(text),
            },
        }
    }

    /// Converts a JSON value to a node. All marks are line 1, column 1.
    pub fn from_json(value: &serde_json::Value) -> Node {
        use serde_json::Value;
        let mark = Mark { line: 1, column: 1 };
        let scalar = |text: String, plain: bool| Node::Scalar { text, plain, mark };
        match value {
            Value::Null => scalar("null".to_owned(), true),
            Value::Bool(b) => scalar(b.to_string(), true),
            Value::Number(n) => scalar(n.to_string(), true),
            Value::String(s) => scalar(s.clone(), false),
            Value::Array(items) => Node::Seq(items.iter().map(Node::from_json).collect(), mark),
            Value::Object(map) => Node::Map(
                map.iter()
                    .map(|(k, v)| (scalar(k.clone(), false), Node::from_json(v)))
                    .collect(),
                mark,
            ),
        }
    }
}

/// Resolves a plain scalar by the YAML 1.2 core schema.
pub fn resolve_plain(text: &str) -> Resolved {
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return Resolved::Null,
        "true" | "True" | "TRUE" => return Resolved::Bool(true),
        "false" | "False" | "FALSE" => return Resolved::Bool(false),
        ".nan" | ".NaN" | ".NAN" => return Resolved::NaN,
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => {
            return Resolved::Number("+inf".to_owned());
        }
        "-.inf" | "-.Inf" | "-.INF" => return Resolved::Number("-inf".to_owned()),
        _ => {}
    }
    let radix = |prefix: &str, radix: u32| {
        let digits = text.strip_prefix(prefix)?;
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
            return None;
        }
        let value = u128::from_str_radix(digits, radix).ok()?;
        Some(Decimal {
            negative: false,
            digits: value.to_string(),
            exponent: 0,
        })
    };
    let number = radix("0x", 16)
        .or_else(|| radix("0o", 8))
        .or_else(|| parse_decimal(text));
    match number {
        Some(number) => number.resolve(),
        None => Resolved::Str(text.to_owned()),
    }
}

/// An exact decimal number: `digits × 10^exponent`.
struct Decimal {
    negative: bool,
    digits: String,
    exponent: i64,
}

/// Parses `[-+]?(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?`.
fn parse_decimal(text: &str) -> Option<Decimal> {
    let (negative, rest) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let (mantissa, exponent) = match rest.find(['e', 'E']) {
        Some(index) => (&rest[..index], Some(&rest[index + 1..])),
        None => (rest, None),
    };
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (mantissa, ""),
    };
    let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if !all_digits(whole) || !all_digits(fraction) || (whole.is_empty() && fraction.is_empty()) {
        return None;
    }
    if whole.is_empty() && !mantissa.starts_with('.') {
        return None;
    }
    let exponent = match exponent {
        None => 0,
        Some(e) => {
            let digits = e.strip_prefix(['-', '+']).unwrap_or(e);
            if digits.is_empty() || !all_digits(digits) {
                return None;
            }
            let value = digits.parse::<i64>().unwrap_or(i64::MAX / 4);
            if e.starts_with('-') { -value } else { value }
        }
    };
    let fraction_len = i64::try_from(fraction.len()).ok()?;
    Some(Decimal {
        negative,
        digits: format!("{whole}{fraction}"),
        exponent: exponent.saturating_sub(fraction_len),
    })
}

impl Decimal {
    /// Returns an integer when the value is one and fits in 64 bits, else the
    /// text form of the Haskell `Scientific` type.
    fn resolve(self) -> Resolved {
        let mut digits = self.digits.trim_start_matches('0').to_owned();
        let mut exponent = self.exponent;
        while digits.ends_with('0') {
            digits.pop();
            exponent = exponent.saturating_add(1);
        }
        if digits.is_empty() {
            return Resolved::Int(0);
        }
        let sign = if self.negative { "-" } else { "" };
        if (0..=19).contains(&exponent) {
            let zeros = usize::try_from(exponent).unwrap_or(0);
            if let Ok(value) = format!("{sign}{digits}{}", "0".repeat(zeros)).parse::<i64>() {
                return Resolved::Int(value);
            }
        }
        let count = i64::try_from(digits.len()).unwrap_or(i64::MAX);
        let point = count.saturating_add(exponent);
        let text = if (0..=7).contains(&point) {
            let point = usize::try_from(point).unwrap_or(0);
            if point == 0 {
                format!("0.{digits}")
            } else if point >= digits.len() {
                format!("{digits}{}.0", "0".repeat(point - digits.len()))
            } else {
                format!("{}.{}", &digits[..point], &digits[point..])
            }
        } else {
            let rest = if digits.len() > 1 { &digits[1..] } else { "0" };
            format!("{}.{rest}e{}", &digits[..1], point.saturating_sub(1))
        };
        Resolved::Number(format!("{sign}{text}"))
    }
}

fn mark_of(span: &Span) -> Mark {
    Mark {
        line: span.start.line(),
        column: span.start.col() + 1,
    }
}

/// Returns the error text with the position, the source line and a caret,
/// for example `a.yaml:2:7: message`, as Pandoc prints it.
pub fn render_error(file: &str, text: &str, error: &YamlError) -> String {
    let mut out = render_position(file, text, &error.message, error.mark);
    if let Some((note_mark, note)) = &error.note {
        out.push('\n');
        out.push_str(&render_position(file, text, note, *note_mark));
    }
    out
}

/// Returns the text of a YAML file in the encoding that its first bytes
/// give, as the YAML specification (section 5.2) detects it: UTF-32 and
/// UTF-16 in both byte orders, with or without a byte order mark, else UTF-8
/// (see [`utf8_text`]). An invalid UTF-16 or UTF-32 sequence becomes U+FFFD.
pub fn decode_text(bytes: &[u8]) -> Result<std::borrow::Cow<'_, str>, (YamlError, &str)> {
    use std::borrow::Cow;
    let units = |data: &[u8], size: usize, big: bool| -> Vec<u32> {
        data.chunks(size)
            .map(|chunk| {
                let mut unit = 0u32;
                for index in 0..size {
                    let byte = u32::from(chunk.get(index).copied().unwrap_or(0));
                    let shift = if big { size - 1 - index } else { index } * 8;
                    unit |= byte << shift;
                }
                unit
            })
            .collect()
    };
    let utf32 = |data: &[u8], big: bool| -> String {
        units(data, 4, big)
            .into_iter()
            .map(|unit| char::from_u32(unit).unwrap_or('\u{fffd}'))
            .collect()
    };
    let utf16 = |data: &[u8], big: bool| -> String {
        let units: Vec<u16> = units(data, 2, big)
            .into_iter()
            .map(|unit| u16::try_from(unit).unwrap_or(0xfffd))
            .collect();
        char::decode_utf16(units)
            .map(|c| c.unwrap_or('\u{fffd}'))
            .collect()
    };
    let text = match bytes {
        [0, 0, 0xfe, 0xff, rest @ ..] => utf32(rest, true),
        [0, 0, 0, _, ..] => utf32(bytes, true),
        [0xff, 0xfe, 0, 0, rest @ ..] => utf32(rest, false),
        [_, 0, 0, 0, ..] => utf32(bytes, false),
        [0xfe, 0xff, rest @ ..] => utf16(rest, true),
        [0, _, ..] => utf16(bytes, true),
        [0xff, 0xfe, rest @ ..] => utf16(rest, false),
        [_, 0, ..] => utf16(bytes, false),
        _ => return utf8_text(bytes).map(Cow::Borrowed),
    };
    Ok(Cow::Owned(text))
}

/// Returns the text of a YAML file, or the error of Pandoc for a file that
/// is not UTF-8 with the text before the first bad byte, for `render_error`.
pub fn utf8_text(bytes: &[u8]) -> Result<&str, (YamlError, &str)> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok(text),
        Err(error) => {
            let valid = std::str::from_utf8(&bytes[..error.valid_up_to()]).unwrap_or_default();
            let line_start = valid.rfind('\n').map_or(0, |index| index + 1);
            let mark = Mark {
                line: valid.matches('\n').count() + 1,
                column: valid[line_start..].chars().count() + 1,
            };
            let error = YamlError {
                message: "invalid UTF-8".to_owned(),
                mark,
                note: None,
            };
            Err((error, valid))
        }
    }
}

fn render_position(file: &str, text: &str, message: &str, mark: Mark) -> String {
    let line_text = text.lines().nth(mark.line.saturating_sub(1)).unwrap_or("");
    let number = mark.line.to_string();
    let gutter = " ".repeat(number.len() + 1);
    let caret = " ".repeat(mark.column.saturating_sub(1));
    format!(
        "{file}:{}:{}: {message}\n{gutter}|\n{number} | {line_text}\n{gutter}| {caret}^",
        mark.line, mark.column
    )
}

/// A node of the first pass: aliases stay references.
enum Raw {
    Scalar(String, bool, Mark),
    Seq(Vec<Raw>, Mark),
    Map(Vec<(Raw, Raw)>, Mark),
    Alias(usize),
}

/// A collection that the first pass is building.
struct Frame {
    node: Raw,
    pending_key: Option<Raw>,
    keys: HashMap<String, Mark>,
    anchor: usize,
    /// The nodes and characters after alias expansion.
    size: u64,
    /// The depth after alias expansion.
    depth: usize,
    path: String,
    index: usize,
}

/// The size and the depth of an anchored node after alias expansion.
#[derive(Clone, Copy)]
struct Expanded {
    size: u64,
    depth: usize,
}

/// Returns an error for a line that starts with a tab, which YAML does not
/// permit as indentation.
fn check_tab_indentation(text: &str) -> Result<(), YamlError> {
    for (index, line) in text.lines().enumerate() {
        if line.starts_with('\t') && !line.trim().is_empty() && !line.trim_start().starts_with('#')
        {
            return Err(YamlError::new(
                "tabs cannot be used for indentation",
                Mark {
                    line: index + 1,
                    column: 1,
                },
            ));
        }
    }
    Ok(())
}

/// Parses the first document of a YAML text.
///
/// Returns `None` for a text without a document. Nesting deeper than
/// `max_depth`, also after alias expansion, is an error. With `single`, a
/// second document is an error, as in a defaults file.
pub fn parse(text: &str, max_depth: usize, single: bool) -> Result<Option<Node>, YamlError> {
    check_tab_indentation(text)?;
    let mut parser = Parser::new_from_str(text);
    let mut stack: Vec<Frame> = Vec::new();
    let mut anchors: HashMap<usize, (Expanded, usize)> = HashMap::new();
    let mut raw_anchors: Vec<Option<Raw>> = Vec::new();
    let mut root: Option<Raw> = None;
    let mut root_expanded = Expanded { size: 0, depth: 0 };
    let mut document_size: u64 = 0;
    let mut added: u64 = 0;
    let mut alias_marks: Vec<(u64, Mark, String)> = Vec::new();
    let mut in_second_document = false;
    while let Some(next) = parser.next_event() {
        let (event, span) = next.map_err(|error| {
            YamlError::new(
                error.info(),
                Mark {
                    line: error.marker().line(),
                    column: error.marker().col() + 1,
                },
            )
        })?;
        let mark = mark_of(&span);
        if in_second_document {
            match event {
                Event::DocumentEnd | Event::DocumentStart(_) | Event::StreamEnd => continue,
                _ if single => {
                    return Err(YamlError::new(
                        "expected a single document, but got a second one",
                        mark,
                    ));
                }
                _ => break,
            }
        }
        let path = match stack.last() {
            None => String::new(),
            Some(frame) => match &frame.node {
                Raw::Seq(..) => format!("{}[{}]", frame.path, frame.index),
                _ => match &frame.pending_key {
                    Some(Raw::Scalar(key, ..)) if frame.path.is_empty() => key.clone(),
                    Some(Raw::Scalar(key, ..)) => format!("{}.{key}", frame.path),
                    _ => frame.path.clone(),
                },
            },
        };
        let (node, expanded, anchor) = match event {
            Event::StreamEnd => break,
            Event::DocumentEnd => {
                if root.is_some() {
                    in_second_document = true;
                }
                continue;
            }
            Event::Scalar(value, style, anchor, _) => {
                let chars = u64::try_from(value.chars().count()).unwrap_or(u64::MAX);
                let size = chars.saturating_add(1);
                document_size = document_size.saturating_add(size);
                // The parser puts an empty value at the `:` before it; HsYAML
                // puts it at the next character.
                let mark = if value.is_empty() && span.start.index() == span.end.index() {
                    Mark {
                        column: mark.column + 1,
                        ..mark
                    }
                } else {
                    mark
                };
                (
                    Raw::Scalar(value.into_owned(), style == ScalarStyle::Plain, mark),
                    Expanded { size, depth: 0 },
                    anchor,
                )
            }
            Event::Alias(id) => {
                let Some((target, _)) = anchors.get(&id).copied() else {
                    return Err(YamlError::new("unknown alias", mark));
                };
                document_size = document_size.saturating_add(1);
                added = added.saturating_add(target.size);
                alias_marks.push((added, mark, path.clone()));
                (Raw::Alias(id), target, 0)
            }
            Event::SequenceStart(anchor, _) | Event::MappingStart(anchor, _) => {
                if stack.len() >= max_depth {
                    return Err(YamlError::new("the YAML text is nested too deeply", mark));
                }
                document_size = document_size.saturating_add(1);
                let node = if matches!(event, Event::SequenceStart(..)) {
                    Raw::Seq(Vec::new(), mark)
                } else {
                    Raw::Map(Vec::new(), mark)
                };
                stack.push(Frame {
                    node,
                    pending_key: None,
                    keys: HashMap::new(),
                    anchor,
                    size: 1,
                    depth: 1,
                    path,
                    index: 0,
                });
                continue;
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let Some(frame) = stack.pop() else {
                    continue;
                };
                (
                    frame.node,
                    Expanded {
                        size: frame.size,
                        depth: frame.depth,
                    },
                    frame.anchor,
                )
            }
            _ => continue,
        };
        if expanded.depth > max_depth {
            return Err(YamlError::new("the YAML text is nested too deeply", mark));
        }
        let node = if anchor > 0 {
            if raw_anchors.len() <= anchor {
                raw_anchors.resize_with(anchor + 1, || None);
            }
            anchors.insert(anchor, (expanded, anchor));
            // The anchored node is kept once; each use refers to it.
            if let Some(slot) = raw_anchors.get_mut(anchor) {
                *slot = Some(node);
            }
            Raw::Alias(anchor)
        } else {
            node
        };
        match stack.last_mut() {
            None => {
                root = Some(node);
                root_expanded = expanded;
            }
            Some(frame) => {
                frame.size = frame.size.saturating_add(expanded.size);
                frame.depth = frame.depth.max(expanded.depth + 1);
                match &mut frame.node {
                    Raw::Seq(items, _) => {
                        items.push(node);
                        frame.index += 1;
                    }
                    Raw::Map(entries, _) => match frame.pending_key.take() {
                        None => {
                            let key_text = match &node {
                                Raw::Scalar(text, ..) => text.clone(),
                                Raw::Alias(id) => match raw_anchors.get(*id) {
                                    Some(Some(Raw::Scalar(text, ..))) => text.clone(),
                                    _ => return Err(key_error(&node, &frame.path, mark)),
                                },
                                _ => return Err(key_error(&node, &frame.path, mark)),
                            };
                            if let Some(first) = frame.keys.get(&key_text) {
                                let prefix = if frame.path.is_empty() {
                                    String::new()
                                } else {
                                    format!("{}: ", frame.path)
                                };
                                return Err(YamlError {
                                    message: format!("{prefix}duplicate key \"{key_text}\""),
                                    mark,
                                    note: Some((
                                        *first,
                                        format!("{prefix}the first key \"{key_text}\""),
                                    )),
                                });
                            }
                            frame.keys.insert(key_text, mark);
                            frame.pending_key = Some(node);
                        }
                        Some(key) => entries.push((key, node)),
                    },
                    _ => {}
                }
            }
        }
    }
    let limit = ALIAS_BUDGET.max(document_size);
    if let Some((_, mark, path)) = alias_marks.iter().find(|(total, ..)| *total > limit) {
        let prefix = if path.is_empty() {
            String::new()
        } else {
            format!("{path}: ")
        };
        return Err(YamlError::new(
            format!("{prefix}the aliases add more than {limit} nodes and characters"),
            *mark,
        ));
    }
    let _ = root_expanded;
    Ok(root.map(|raw| expand(&raw, &raw_anchors)))
}

fn key_error(node: &Raw, path: &str, mark: Mark) -> YamlError {
    let mark = match node {
        Raw::Seq(_, start) | Raw::Map(_, start) => *start,
        _ => mark,
    };
    let kind = match node {
        Raw::Seq(..) => "a list",
        _ => "a mapping",
    };
    let prefix = if path.is_empty() {
        String::new()
    } else {
        format!("{path}: ")
    };
    YamlError::new(
        format!("{prefix}expected a scalar key, but got {kind}"),
        mark,
    )
}

/// Builds the tree and copies each anchored node at each of its uses.
///
/// The depth after expansion is at most the limit of [`parse`], and the size
/// is at most the alias budget plus the document.
fn expand(raw: &Raw, anchors: &[Option<Raw>]) -> Node {
    match raw {
        Raw::Scalar(text, plain, mark) => Node::Scalar {
            text: text.clone(),
            plain: *plain,
            mark: *mark,
        },
        Raw::Seq(items, mark) => {
            Node::Seq(items.iter().map(|i| expand(i, anchors)).collect(), *mark)
        }
        Raw::Map(entries, mark) => Node::Map(
            entries
                .iter()
                .map(|(k, v)| (expand(k, anchors), expand(v, anchors)))
                .collect(),
            *mark,
        ),
        Raw::Alias(id) => match anchors.get(*id) {
            Some(Some(target)) => expand(target, anchors),
            _ => Node::Scalar {
                text: String::new(),
                plain: true,
                mark: Mark::default(),
            },
        },
    }
}

/// A JSON value read in source order, where the first of two equal keys
/// wins, as in the JSON library of Pandoc.
struct FirstWins(Node);

impl<'de> serde::Deserialize<'de> for FirstWins {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(FirstWinsVisitor)
    }
}

struct FirstWinsVisitor;

impl<'de> serde::de::Visitor<'de> for FirstWinsVisitor {
    type Value = FirstWins;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<FirstWins, E> {
        Ok(FirstWins(Node::from_json(&serde_json::Value::Bool(v))))
    }
    fn visit_i64<E>(self, v: i64) -> Result<FirstWins, E> {
        Ok(FirstWins(Node::from_json(&serde_json::Value::from(v))))
    }
    fn visit_u64<E>(self, v: u64) -> Result<FirstWins, E> {
        Ok(FirstWins(Node::from_json(&serde_json::Value::from(v))))
    }
    fn visit_f64<E>(self, v: f64) -> Result<FirstWins, E> {
        Ok(FirstWins(Node::from_json(&serde_json::Value::from(v))))
    }
    fn visit_str<E>(self, v: &str) -> Result<FirstWins, E> {
        Ok(FirstWins(Node::from_json(&serde_json::Value::String(
            v.to_owned(),
        ))))
    }
    fn visit_unit<E>(self) -> Result<FirstWins, E> {
        Ok(FirstWins(Node::from_json(&serde_json::Value::Null)))
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<FirstWins, A::Error> {
        let mut items = Vec::new();
        while let Some(FirstWins(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(FirstWins(Node::Seq(items, Mark { line: 1, column: 1 })))
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<FirstWins, A::Error> {
        let mark = Mark { line: 1, column: 1 };
        let mut entries: Vec<(Node, Node)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        while let Some((key, FirstWins(value))) = map.next_entry::<String, FirstWins>()? {
            if seen.insert(key.clone()) {
                entries.push((
                    Node::Scalar {
                        text: key,
                        plain: false,
                        mark,
                    },
                    value,
                ));
            }
        }
        Ok(FirstWins(Node::Map(entries, mark)))
    }
}

/// Parses a JSON text into a node: keys stay in source order and the first
/// of two equal keys wins. The error text is the Aeson text for the end of
/// the input.
pub fn parse_json(text: &str) -> Result<Node, String> {
    let message = |error: serde_json::Error| {
        if error.is_eof() {
            "Unexpected end-of-input, expecting JSON value".to_owned()
        } else {
            format!("Unexpected {error}")
        }
    };
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let FirstWins(node) = serde::Deserialize::deserialize(&mut deserializer).map_err(message)?;
    deserializer.end().map_err(message)?;
    Ok(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Node {
        parse(text, 16, true).unwrap().unwrap()
    }

    #[test]
    fn builds_maps_sequences_and_scalars_with_positions() {
        let node = one("a: 1\nb: [x, 'y']\nc: &k {d: true}\ne: *k\n");
        let Node::Map(entries, _) = &node else {
            panic!("not a map")
        };
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].1.as_int(), Some(1));
        assert_eq!(entries[0].1.mark(), Mark { line: 1, column: 4 });
        assert_eq!(entries[1].1.type_name(), "Array");
        assert_eq!(entries[3].1, entries[2].1);
        assert_eq!(
            node.to_json(),
            serde_json::json!({"a": 1, "b": ["x", "y"], "c": {"d": true}, "e": {"d": true}})
        );
    }

    #[test]
    fn reports_errors_and_deep_nesting() {
        let error = parse("to: [json\n", 16, true).unwrap_err();
        assert!(error.mark.line <= 2 && !error.message.is_empty());
        assert!(parse(&"[".repeat(20), 8, true).is_err());
        assert_eq!(parse("", 8, true).unwrap(), None);
        assert_eq!(one("'1'").as_int(), None);
        assert_eq!(one("~").type_name(), "Null");
        assert_eq!(one("1.5").type_name(), "Number");
        // Aliases cannot build a tree deeper than the limit.
        let mut text = String::from("a0: &a0 [x]\n");
        for i in 1..6 {
            text.push_str(&format!("a{i}: &a{i} [[*a{}]]\n", i - 1));
        }
        assert!(parse(&text, 8, true).is_err());
    }

    #[test]
    fn alias_expansion_has_the_pandoc_limit() {
        // Pandoc 3.13: `x14[0]: the aliases add more than 100000 nodes and characters` at 16:12.
        let mut text = String::from("a: &a x\n");
        let mut previous = "a".to_owned();
        for i in 0..16 {
            text.push_str(&format!("x{i}: &x{i} [*{previous}, *{previous}]\n"));
            previous = format!("x{i}");
        }
        let error = parse(&text, 16384, false).unwrap_err();
        assert_eq!(
            error.message,
            "x14[0]: the aliases add more than 100000 nodes and characters"
        );
        assert_eq!(
            error.mark,
            Mark {
                line: 16,
                column: 12
            }
        );
        assert_eq!(
            render_error("m.yaml", &text, &error),
            "m.yaml:16:12: x14[0]: the aliases add more than 100000 nodes and characters\n   |\n16 | x14: &x14 [*x13, *x13]\n   |            ^"
        );
        // Two aliases of a 50000 character scalar add 100002.
        let long = format!("a: &a {}\nb: *a\nc: *a\n", "x".repeat(50_000));
        let error = parse(&long, 16, false).unwrap_err();
        assert_eq!(error.mark, Mark { line: 3, column: 4 });
        assert!(error.message.starts_with("c: the aliases add"));
        let once = format!("a: &a {}\nb: *a\n", "x".repeat(100_001));
        assert!(parse(&once, 16, false).is_ok());
        let nested = "a: &a [x]\nb: {c: [*a]}\n";
        assert!(parse(nested, 16, false).is_ok());
    }

    #[test]
    fn keys_must_be_unique_scalars_and_one_document_is_permitted() {
        let error = parse("from: json\nfrom: json\n", 16, true).unwrap_err();
        assert_eq!(error.message, "duplicate key \"from\"");
        assert_eq!(
            render_error("p.yaml", "from: json\nfrom: json\n", &error),
            "p.yaml:2:1: duplicate key \"from\"\n  |\n2 | from: json\n  | ^\np.yaml:1:1: the first key \"from\"\n  |\n1 | from: json\n  | ^"
        );
        let error = parse("metadata:\n  ? [a]\n  : b\n", 16, true).unwrap_err();
        assert_eq!(
            error.message,
            "metadata: expected a scalar key, but got a list"
        );
        assert_eq!(error.mark, Mark { line: 2, column: 5 });
        let error = parse("---\na: 1\n---\nb: 2\n", 16, true).unwrap_err();
        assert_eq!(
            error.message,
            "expected a single document, but got a second one"
        );
        assert_eq!(error.mark, Mark { line: 4, column: 1 });
        assert!(parse("---\na: 1\n---\nb: 2\n", 16, false).is_ok());
        let error = parse("\tfrom: json\n", 16, true).unwrap_err();
        assert_eq!(error.message, "tabs cannot be used for indentation");
    }

    #[test]
    fn json_keeps_the_first_of_two_equal_keys_and_the_order() {
        let node = parse_json(r#"{"b":1,"a":{"x":1,"x":2},"b":2}"#).unwrap();
        let Node::Map(entries, _) = &node else {
            panic!("not a map")
        };
        let keys: Vec<_> = entries.iter().filter_map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["b", "a"]);
        assert_eq!(entries[0].1.as_int(), Some(1));
        assert_eq!(node.to_json()["a"]["x"], 1);
        assert_eq!(
            parse_json("{").unwrap_err(),
            "Unexpected end-of-input, expecting JSON value"
        );
        assert!(parse_json("{} x").is_err());
    }

    #[test]
    fn plain_scalars_follow_the_core_schema() {
        // Each text is the `Str` that Pandoc 3.13 makes for `a: VALUE` in a metadata file.
        let cases = [
            ("1.0", "1"),
            ("1.5", "1.5"),
            ("0.1", "0.1"),
            ("1e3", "1000"),
            ("1e20", "1.0e20"),
            ("1.5e-7", "1.5e-7"),
            ("010", "10"),
            ("0x10", "16"),
            ("0o17", "15"),
            ("+5", "5"),
            ("-0", "0"),
            (".inf", "+inf"),
            ("-.inf", "-inf"),
            (
                "123456789012345678901234567890",
                "1.2345678901234567890123456789e29",
            ),
            ("1_000", "1_000"),
            ("yes", "yes"),
            ("0.0", "0"),
            ("-1.25", "-1.25"),
            ("12.0e1", "120"),
            ("3.14159265358979", "3.14159265358979"),
            ("1e400", "1.0e400"),
            ("12345678", "12345678"),
            ("1234567.5", "1234567.5"),
            ("12345678.5", "1.23456785e7"),
            (".5", "0.5"),
            ("1.", "1"),
        ];
        for (text, expected) in cases {
            let node = one(&format!("{text}\n"));
            assert_eq!(node.scalar_text().as_deref(), Some(expected), "{text}");
        }
        assert_eq!(one(".nan").scalar_text().as_deref(), Some(""));
        assert_eq!(one("null").scalar_text().as_deref(), Some(""));
        assert_eq!(one("'0x10'").scalar_text().as_deref(), Some("0x10"));
        assert_eq!(resolve_plain("-"), Resolved::Str("-".to_owned()));
        assert_eq!(resolve_plain("e3"), Resolved::Str("e3".to_owned()));
        assert_eq!(resolve_plain("1e"), Resolved::Str("1e".to_owned()));
    }
}
