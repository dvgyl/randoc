//! The JSON text parser. It gives a tape: the values of the text in document
//! order, in one vector. The tape keeps the text of each number.
//!
//! The parser does not use recursion. The error messages are the messages of
//! the token parser of the Haskell library `aeson` 2.2, which Pandoc uses.

use super::haskell::{show_byte_char, show_byte_string};

/// One value on the tape. An array or an object is followed by its items;
/// `end` is the index after the last of them. An object item is a key and a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Node<'a> {
    Null,
    Bool(bool),
    /// A number, as written in the input.
    Number(&'a str),
    /// A string without escapes.
    Text(&'a str),
    /// A string with escapes: the index of the decoded text.
    Owned(usize),
    Array {
        end: usize,
        len: usize,
    },
    Object {
        end: usize,
        len: usize,
    },
}

/// The parsed values of one JSON text.
#[derive(Debug)]
pub struct Tape<'a> {
    nodes: Vec<Node<'a>>,
    owned: Vec<String>,
}

impl<'a> Tape<'a> {
    /// The first value of the text.
    pub fn root(&self) -> Value<'_> {
        Value { tape: self, at: 0 }
    }
}

/// A view of one value on a tape.
#[derive(Debug, Clone, Copy)]
pub struct Value<'t> {
    tape: &'t Tape<'t>,
    at: usize,
}

/// The kind of a value and its data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind<'t> {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number, as written in the input.
    Number(&'t str),
    /// A string with its escapes decoded.
    String(&'t str),
    /// An array.
    Array,
    /// An object.
    Object,
}

impl<'t> Value<'t> {
    fn node(&self) -> Node<'t> {
        // An index outside the tape cannot occur; it reads as `null`.
        self.tape.nodes.get(self.at).copied().unwrap_or(Node::Null)
    }

    /// The kind and the data of the value.
    pub fn kind(&self) -> Kind<'t> {
        match self.node() {
            Node::Null => Kind::Null,
            Node::Bool(flag) => Kind::Bool(flag),
            Node::Number(text) => Kind::Number(text),
            Node::Text(text) => Kind::String(text),
            Node::Owned(index) => {
                Kind::String(self.tape.owned.get(index).map_or("", String::as_str))
            }
            Node::Array { .. } => Kind::Array,
            Node::Object { .. } => Kind::Object,
        }
    }

    /// The text of a string, or `None` for another kind of value.
    pub fn as_str(&self) -> Option<&'t str> {
        match self.kind() {
            Kind::String(text) => Some(text),
            _ => None,
        }
    }

    /// The name of the JSON type, as `aeson` writes it in a message.
    pub fn type_name(&self) -> &'static str {
        match self.node() {
            Node::Null => "Null",
            Node::Bool(_) => "Boolean",
            Node::Number(_) => "Number",
            Node::Text(_) | Node::Owned(_) => "String",
            Node::Array { .. } => "Array",
            Node::Object { .. } => "Object",
        }
    }

    /// The index after this value and its items.
    fn next(&self) -> usize {
        match self.node() {
            Node::Array { end, .. } | Node::Object { end, .. } => end,
            _ => self.at + 1,
        }
    }

    /// The number of items of an array or of entries of an object.
    pub fn len(&self) -> usize {
        match self.node() {
            Node::Array { len, .. } | Node::Object { len, .. } => len,
            _ => 0,
        }
    }

    /// Returns `true` for an empty array or object, and for each other kind of value.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The items of an array, in order.
    pub fn items(&self) -> Items<'t> {
        Items {
            tape: self.tape,
            at: self.at + 1,
            left: if matches!(self.node(), Node::Array { .. }) {
                self.len()
            } else {
                0
            },
        }
    }

    /// The entries of an object, in input order. A key can occur more than one time.
    pub fn entries(&self) -> Entries<'t> {
        Entries {
            tape: self.tape,
            at: self.at + 1,
            left: if matches!(self.node(), Node::Object { .. }) {
                self.len()
            } else {
                0
            },
        }
    }

    /// Returns the values of the first entries with the keys `t` and `c`, in one pass.
    pub fn tag_and_content(&self) -> (Option<Value<'t>>, Option<Value<'t>>) {
        let (mut tag, mut content) = (None, None);
        for (key, value) in self.entries() {
            match key {
                "t" if tag.is_none() => tag = Some(value),
                "c" if content.is_none() => content = Some(value),
                _ => {}
            }
            if tag.is_some() && content.is_some() {
                break;
            }
        }
        (tag, content)
    }

    /// Returns the value of the first entry with the key, as `aeson` does.
    pub fn lookup(&self, key: &str) -> Option<Value<'t>> {
        self.entries().find(|(k, _)| *k == key).map(|(_, v)| v)
    }
}

/// The items of an array.
#[derive(Debug, Clone)]
pub struct Items<'t> {
    tape: &'t Tape<'t>,
    at: usize,
    left: usize,
}

impl<'t> Items<'t> {
    /// The next item. After the last item it gives `null` (the index after the tape);
    /// a decoder checks the length first.
    pub fn next_item(&mut self) -> Value<'t> {
        let end = self.tape.nodes.len();
        self.next().unwrap_or(Value {
            tape: self.tape,
            at: end,
        })
    }
}

impl<'t> Iterator for Items<'t> {
    type Item = Value<'t>;

    fn next(&mut self) -> Option<Value<'t>> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        let value = Value {
            tape: self.tape,
            at: self.at,
        };
        self.at = value.next();
        Some(value)
    }
}

/// The entries of an object.
#[derive(Debug, Clone)]
pub struct Entries<'t> {
    tape: &'t Tape<'t>,
    at: usize,
    left: usize,
}

impl<'t> Iterator for Entries<'t> {
    type Item = (&'t str, Value<'t>);

    fn next(&mut self) -> Option<(&'t str, Value<'t>)> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        let key = Value {
            tape: self.tape,
            at: self.at,
        };
        let value = Value {
            tape: self.tape,
            at: self.at + 1,
        };
        self.at = value.next();
        Some((key.as_str().unwrap_or(""), value))
    }
}

/// A failure of the parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The text is not valid JSON. The message is the text of `aeson`.
    Syntax(String),
    /// The text has more levels of arrays and objects than the limit.
    TooDeep,
}

const INVALID_STRING: &str =
    "Cannot decode input: Data.Text.Internal.Encoding.decodeUtf8: Invalid UTF-8 stream";

/// The number of input bytes that a message about unexpected input shows.
const SNIPPET_BYTES: usize = 30;

/// One open array or object: the index of its node and its number of items.
struct Frame {
    node: usize,
    len: usize,
    object: bool,
}

/// Parses one JSON value at the start of `text`.
///
/// Returns the tape and the byte offset after the value. The caller checks
/// the rest of the text, because `aeson` converts the value before it checks the rest.
pub fn parse(text: &str, max_depth: usize) -> Result<(Tape<'_>, usize), ParseError> {
    let mut parser = Parser {
        text,
        bytes: text.as_bytes(),
        pos: 0,
        nodes: Vec::with_capacity(text.len() / 8 + 1),
        owned: Vec::new(),
    };
    let end = parser.run(max_depth)?;
    Ok((
        Tape {
            nodes: parser.nodes,
            owned: parser.owned,
        },
        end,
    ))
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    nodes: Vec<Node<'a>>,
    owned: Vec<String>,
}

impl<'a> Parser<'a> {
    fn run(&mut self, max_depth: usize) -> Result<usize, ParseError> {
        let mut stack: Vec<Frame> = Vec::new();
        // The expectation for the next value: after `[` the end of input has its own message.
        let mut after_open_bracket = false;
        loop {
            self.skip_space();
            match self.peek() {
                None if after_open_bracket => {
                    return Err(self.end("JSON value or ]"));
                }
                None => return Err(self.end("JSON value")),
                Some(b'{') | Some(b'[') if stack.len() >= max_depth => {
                    return Err(ParseError::TooDeep);
                }
                Some(b'{') => {
                    self.pos += 1;
                    self.skip_space();
                    let node = self.nodes.len();
                    match self.peek() {
                        Some(b'}') => {
                            self.pos += 1;
                            self.nodes.push(Node::Object {
                                end: node + 1,
                                len: 0,
                            });
                        }
                        Some(b'"') => {
                            self.nodes.push(Node::Object { end: 0, len: 0 });
                            self.key()?;
                            stack.push(Frame {
                                node,
                                len: 0,
                                object: true,
                            });
                            after_open_bracket = false;
                            continue;
                        }
                        _ => return Err(self.unexpected("record key literal or }")),
                    }
                }
                Some(b'[') => {
                    self.pos += 1;
                    self.skip_space();
                    let node = self.nodes.len();
                    if self.peek() == Some(b']') {
                        self.pos += 1;
                        self.nodes.push(Node::Array {
                            end: node + 1,
                            len: 0,
                        });
                    } else {
                        self.nodes.push(Node::Array { end: 0, len: 0 });
                        stack.push(Frame {
                            node,
                            len: 0,
                            object: false,
                        });
                        after_open_bracket = true;
                        continue;
                    }
                }
                Some(b'"') => self.string()?,
                Some(b'-' | b'0'..=b'9') => {
                    let number = self.number()?;
                    self.nodes.push(Node::Number(number));
                }
                Some(b't') if self.bytes[self.pos..].starts_with(b"true") => {
                    self.pos += 4;
                    self.nodes.push(Node::Bool(true));
                }
                Some(b'f') if self.bytes[self.pos..].starts_with(b"false") => {
                    self.pos += 5;
                    self.nodes.push(Node::Bool(false));
                }
                Some(b'n') if self.bytes[self.pos..].starts_with(b"null") => {
                    self.pos += 4;
                    self.nodes.push(Node::Null);
                }
                Some(_) => return Err(self.unexpected("JSON value")),
            }
            after_open_bracket = false;
            // Close each container that this value completes.
            loop {
                let Some(frame) = stack.last_mut() else {
                    return Ok(self.pos);
                };
                frame.len += 1;
                self.skip_space();
                let (close, expected) = if frame.object {
                    (b'}', ", or }")
                } else {
                    (b']', ", or ]")
                };
                match self.peek() {
                    Some(b',') if frame.object => {
                        // `aeson` shows the text from the comma in this message.
                        let comma = self.pos;
                        self.pos += 1;
                        self.skip_space();
                        if self.peek() != Some(b'"') {
                            if self.peek().is_some() {
                                self.pos = comma;
                            }
                            return Err(self.unexpected("key literal"));
                        }
                        self.key()?;
                        break;
                    }
                    Some(b',') => {
                        self.pos += 1;
                        break;
                    }
                    Some(byte) if byte == close => {
                        self.pos += 1;
                        let end = self.nodes.len();
                        let Some(Frame { node, len, object }) = stack.pop() else {
                            return unreachable_frame();
                        };
                        if let Some(slot) = self.nodes.get_mut(node) {
                            *slot = if object {
                                Node::Object { end, len }
                            } else {
                                Node::Array { end, len }
                            };
                        }
                    }
                    None => return Err(self.end(expected)),
                    Some(_) => return Err(self.unexpected(expected)),
                }
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while let Some(b' ' | b'\n' | b'\r' | b'\t') = self.peek() {
            self.pos += 1;
        }
    }

    fn end(&self, expected: &str) -> ParseError {
        ParseError::Syntax(format!("Unexpected end-of-input, expecting {expected}"))
    }

    fn unexpected(&self, expected: &str) -> ParseError {
        if self.pos >= self.bytes.len() {
            return self.end(expected);
        }
        let end = (self.pos + SNIPPET_BYTES).min(self.bytes.len());
        ParseError::Syntax(format!(
            "Unexpected {}, expecting {expected}",
            show_byte_string(&self.bytes[self.pos..end])
        ))
    }

    /// Reads a key and the `:` after it, and puts the key on the tape. The current byte is `"`.
    fn key(&mut self) -> Result<(), ParseError> {
        self.string()?;
        self.skip_space();
        if self.peek() != Some(b':') {
            return Err(self.unexpected(":"));
        }
        self.pos += 1;
        Ok(())
    }

    /// Reads a string literal and puts it on the tape. The current byte is `"`.
    fn string(&mut self) -> Result<(), ParseError> {
        self.pos += 1;
        let start = self.pos;
        let mut escaped = false;
        loop {
            match self.peek() {
                None => {
                    return Err(ParseError::Syntax(
                        "Unexpected end-of-input while parsing string literal".to_owned(),
                    ));
                }
                Some(b'"') => break,
                Some(b'\\') => {
                    escaped = true;
                    self.pos += 2;
                }
                Some(byte) if byte < 0x20 => {
                    return Err(ParseError::Syntax(
                        "Unespected control character while parsing string literal".to_owned(),
                    ));
                }
                Some(_) => self.pos += 1,
            }
        }
        let end = self.pos;
        self.pos += 1;
        // `start` and `end` are next to ASCII bytes, so they are character boundaries.
        let raw = self.text.get(start..end).ok_or_else(invalid_string)?;
        if escaped {
            let index = self.owned.len();
            self.owned.push(unescape(raw)?);
            self.nodes.push(Node::Owned(index));
        } else {
            self.nodes.push(Node::Text(raw));
        }
        Ok(())
    }

    /// Reads a number literal and returns its text.
    fn number(&mut self) -> Result<&'a str, ParseError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if let Some(b'0'..=b'9') = self.peek() {
                    return Err(ParseError::Syntax(
                        "Number literal with leading zero".to_owned(),
                    ));
                }
            }
            Some(b'1'..=b'9') => self.digits(),
            other => return Err(number_error(other, false)),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            match self.peek() {
                Some(b'0'..=b'9') => self.digits(),
                other => return Err(number_error(other, false)),
            }
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.pos += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.pos += 1;
            }
            match self.peek() {
                Some(b'0'..=b'9') => self.digits(),
                other => return Err(number_error(other, true)),
            }
        }
        Ok(&self.text[start..self.pos])
    }

    fn digits(&mut self) {
        while let Some(b'0'..=b'9') = self.peek() {
            self.pos += 1;
        }
    }
}

/// `aeson` shows an unexpected byte in the exponent as a character and other bytes as numbers.
fn number_error(byte: Option<u8>, in_exponent: bool) -> ParseError {
    ParseError::Syntax(match byte {
        None => "Unexpected end-of-input while parsing number literal".to_owned(),
        Some(byte) if in_exponent => format!(
            "Unexpected {} while parsing number literal",
            show_byte_char(byte)
        ),
        Some(byte) => format!("Unexpected {byte} while parsing number literal"),
    })
}

fn invalid_string() -> ParseError {
    ParseError::Syntax(INVALID_STRING.to_owned())
}

fn unreachable_frame<T>() -> Result<T, ParseError> {
    Err(ParseError::Syntax(
        "internal error: the parser stack is not consistent".to_owned(),
    ))
}

/// Decodes the escapes of a string literal.
fn unescape(raw: &str) -> Result<String, ParseError> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(index) = rest.find('\\') {
        out.push_str(&rest[..index]);
        let bytes = rest.as_bytes();
        let escape = bytes.get(index + 1).copied().ok_or_else(invalid_string)?;
        let mut next = index + 2;
        match escape {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{8}'),
            b'f' => out.push('\u{c}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let unit = hex4(bytes, next)?;
                next += 4;
                let code = match unit {
                    0xD800..=0xDBFF => {
                        if bytes.get(next) != Some(&b'\\') || bytes.get(next + 1) != Some(&b'u') {
                            return Err(invalid_string());
                        }
                        let low = hex4(bytes, next + 2)?;
                        if !(0xDC00..=0xDFFF).contains(&low) {
                            return Err(invalid_string());
                        }
                        next += 6;
                        0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00)
                    }
                    0xDC00..=0xDFFF => return Err(invalid_string()),
                    _ => unit,
                };
                out.push(char::from_u32(code).ok_or_else(invalid_string)?);
            }
            _ => return Err(invalid_string()),
        }
        rest = rest.get(next..).ok_or_else(invalid_string)?;
    }
    out.push_str(rest);
    Ok(out)
}

fn hex4(bytes: &[u8], at: usize) -> Result<u32, ParseError> {
    let digits = bytes.get(at..at + 4).ok_or_else(invalid_string)?;
    digits.iter().try_fold(0u32, |acc, &byte| {
        let digit = (byte as char).to_digit(16).ok_or_else(invalid_string)?;
        Ok(acc * 16 + digit)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn syntax(text: &str) -> String {
        match parse(text, 100) {
            Err(ParseError::Syntax(message)) => message,
            Err(other) => format!("{other:?}"),
            Ok(_) => "no error".to_owned(),
        }
    }

    #[test]
    fn messages_match_the_oracle() {
        // Each expected text is from Pandoc 3.13, after "JSON parse error: ".
        let cases = [
            (
                "hello\n\n",
                r#"Unexpected "hello\n\n", expecting JSON value"#,
            ),
            ("\n\n", "Unexpected end-of-input, expecting JSON value"),
            (
                "{\n\n",
                "Unexpected end-of-input, expecting record key literal or }",
            ),
            (
                "{5:1}\n\n",
                r#"Unexpected "5:1}\n\n", expecting record key literal or }"#,
            ),
            ("{\"a\"\n\n", "Unexpected end-of-input, expecting :"),
            ("{\"a\" 1}\n\n", r#"Unexpected "1}\n\n", expecting :"#),
            ("{\"a\":1\n\n", "Unexpected end-of-input, expecting , or }"),
            (
                "{\"a\":1,\n\n",
                "Unexpected end-of-input, expecting key literal",
            ),
            (
                "{\"a\":1,}\n\n",
                r#"Unexpected ",}\n\n", expecting key literal"#,
            ),
            (
                "[\n\n",
                "Unexpected end-of-input, expecting JSON value or ]",
            ),
            ("[x\n\n", r#"Unexpected "x\n\n", expecting JSON value"#),
            ("[1,]\n\n", r#"Unexpected "]\n\n", expecting JSON value"#),
            ("[1 2]\n\n", r#"Unexpected "2]\n\n", expecting , or ]"#),
            ("[1,2\n\n", "Unexpected end-of-input, expecting , or ]"),
            (
                "\"abc\n\n",
                "Unespected control character while parsing string literal",
            ),
            ("\"a\\qb\"\n\n", INVALID_STRING),
            ("\"\\ud83d\"\n\n", INVALID_STRING),
            ("\"\\ude00\\ud83d\"\n\n", INVALID_STRING),
            ("\"\\ud83d\\u0041\"\n\n", INVALID_STRING),
            ("\"\\u12g4\"\n\n", INVALID_STRING),
            ("tru\n\n", r#"Unexpected "tru\n\n", expecting JSON value"#),
            ("-\n\n", "Unexpected 10 while parsing number literal"),
            ("[-x]\n\n", "Unexpected 120 while parsing number literal"),
            ("01\n\n", "Number literal with leading zero"),
            ("-01\n\n", "Number literal with leading zero"),
            ("[0.]\n\n", "Unexpected 93 while parsing number literal"),
            ("1e\n\n", r"Unexpected '\n' while parsing number literal"),
            ("[1.5e+]\n\n", "Unexpected ']' while parsing number literal"),
            (
                "[1e\u{e9}]\n\n",
                r"Unexpected '\195' while parsing number literal",
            ),
            (
                "[1e\\]\n\n",
                r"Unexpected '\\' while parsing number literal",
            ),
            (
                "abcdefghijklmnopqrstuvwxyzabcdefghij",
                r#"Unexpected "abcdefghijklmnopqrstuvwxyzabcd", expecting JSON value"#,
            ),
            (
                "x\u{1}\u{7f}\u{5}\u{e9}1\n\n",
                r#"Unexpected "x\SOH\DEL\ENQ\195\169\&1\n\n", expecting JSON value"#,
            ),
            (
                "x\u{e}H\n\n",
                r#"Unexpected "x\SO\&H\n\n", expecting JSON value"#,
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(syntax(text), expected, "input {text:?}");
        }
    }

    #[test]
    fn values_keep_number_text_and_duplicate_keys() {
        let (tape, end) = parse(
            r#" {"a":[1.50e1,-0,true,false,null],"a":"xé\/","b":{}} x"#,
            10,
        )
        .unwrap();
        assert_eq!(end, 53);
        let root = tape.root();
        assert_eq!(root.kind(), Kind::Object);
        assert_eq!(root.len(), 3);
        let keys: Vec<&str> = root.entries().map(|(k, _)| k).collect();
        assert_eq!(keys, ["a", "a", "b"]);
        let first = root.lookup("a").unwrap();
        let kinds: Vec<Kind> = first.items().map(|v| v.kind()).collect();
        assert_eq!(
            kinds,
            [
                Kind::Number("1.50e1"),
                Kind::Number("-0"),
                Kind::Bool(true),
                Kind::Bool(false),
                Kind::Null
            ]
        );
        let second: Vec<Kind> = root.entries().map(|(_, v)| v.kind()).collect();
        assert_eq!(second[1], Kind::String("x\u{e9}/"));
        assert_eq!(
            root.lookup("b").map(|v| (v.kind(), v.len())),
            Some((Kind::Object, 0))
        );
        assert!(root.lookup("c").is_none());
        let mut items = first.items();
        for _ in 0..5 {
            items.next_item();
        }
        let end = items.next_item();
        assert_eq!(end.kind(), Kind::Null);
        assert_eq!(
            (end.items().count(), end.entries().count(), end.len()),
            (0, 0, 0)
        );
        assert!(end.lookup("a").is_none());
        assert_eq!(first.as_str(), None);
        assert_eq!(first.entries().count(), 0);
        assert_eq!(root.items().count(), 0);
    }

    #[test]
    fn strings_decode_escapes_and_surrogate_pairs() {
        let (tape, _) = parse(r#""😀\b\f\n\r\t\"\\\u0000""#, 1).unwrap();
        assert_eq!(
            tape.root().kind(),
            Kind::String("\u{1f600}\u{8}\u{c}\n\r\t\"\\\u{0}")
        );
    }

    #[test]
    fn nested_values_skip_to_their_end() {
        let (tape, _) = parse(r#"[[1,[2,{"k":[3]}]],{"x":{"y":4}},5]"#, 10).unwrap();
        let items: Vec<Value> = tape.root().items().collect();
        assert_eq!(items.len(), 3);
        assert_eq!(items[2].kind(), Kind::Number("5"));
        let inner = items[1].lookup("x").unwrap().lookup("y").unwrap();
        assert_eq!(inner.kind(), Kind::Number("4"));
    }

    #[test]
    fn the_depth_limit_counts_each_array_and_object() {
        assert_eq!(parse("[[{}]]", 2).err(), Some(ParseError::TooDeep));
        assert!(parse("[[{}]]", 3).is_ok());
        assert!(parse("[[],[]]", 2).is_ok());
        assert_eq!(parse("[", 0).err(), Some(ParseError::TooDeep));
    }

    #[test]
    fn value_type_names_are_the_aeson_names() {
        let (tape, _) = parse(r#"[null,true,1,"",[],{},"\n"]"#, 2).unwrap();
        let names: Vec<_> = tape.root().items().map(|v| v.type_name()).collect();
        assert_eq!(
            names,
            [
                "Null", "Boolean", "Number", "String", "Array", "Object", "String"
            ]
        );
    }
}
