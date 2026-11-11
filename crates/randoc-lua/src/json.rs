//! The module `pandoc.json`.
//!
//! The rules come from the oracle matrix `tests/replay/modules.txt`: Pandoc
//! converts a Lua value to an `aeson` value, so each number goes through a
//! `Double`, the keys of an object are in order, and the document types use
//! the JSON form of `pandoc-types`.

use std::collections::BTreeMap;

use mlua::{Lua, Table, Value};
use randoc_ast::json::{value_from_json, value_to_json};
use randoc_ast::{Block, Inline, Pandoc};

use crate::construct::Call;
use crate::object::{Node, with_node};
use crate::peek::{Cx, Peek, PeekError, depth_message, described_type, is_none_or_nil, none};
use crate::push;
use crate::runtime;

/// A JSON value with the keys of each object in order.
enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
    /// The text that a `__tojson` metamethod gave.
    Raw(String),
}

/// Runs the function `name` of `pandoc.json`.
pub(crate) fn call(call: &Call, name: &str) -> Peek<Vec<Value>> {
    let cx = call.cx;
    let lua = cx.lua;
    match name {
        "encode" => {
            // `pandoc.json.null` is a null light userdata, the value that
            // stands for a missing argument elsewhere.
            let Some(value) = call.arguments.first() else {
                return Err(no_tojson(call));
            };
            let json = to_json(cx, value, 0).map_err(|error| match error {
                Fail::NoToJson => no_tojson(call),
                Fail::Key(found) => argument_error(
                    call,
                    &format!("string expected, got {found}"),
                    &["retrieving key", "retrieving key-value pair"],
                ),
                Fail::Error(error) => error,
            })?;
            let mut out = String::new();
            write(&json, &mut out);
            Ok(vec![push::text(lua, &out)?])
        }
        "decode" => {
            let text = call.required(0, "str", |v| cx.text(v))?;
            let typed = call.argument(1);
            let typed = is_none_or_nil(&typed) || !matches!(typed, Value::Boolean(false));
            if typed && let Some(value) = decode_typed(lua, &text)? {
                return Ok(vec![value]);
            }
            match Parser::new(&text, cx.max_depth).document() {
                Ok(json) => Ok(vec![to_lua(lua, &json)?]),
                Err(()) => Ok(vec![Value::Nil]),
            }
        }
        "list_tojson" => {
            // The metamethod gives the JSON text of `pandoc-types` as it
            // is, without the key order of `encode`.
            let kind = call.required(0, "kind", |v| cx.text(v))?;
            let list = call.argument(1);
            let text = if kind == "Inlines" {
                value_to_json(&cx.inlines_fuzzy(&list).map_err(PeekError::thrown)?)
            } else {
                value_to_json(&cx.blocks_fuzzy(&list).map_err(PeekError::thrown)?)
            };
            Ok(vec![push::text(lua, &text)?])
        }
        other => Err(PeekError::Thrown(format!(
            "internal error: unknown function {other}"
        ))),
    }
}

/// The value of `pandoc.json.null`.
pub(crate) fn null() -> Value {
    none()
}

enum Fail {
    NoToJson,
    /// A key of a table that is not a string or a number.
    Key(String),
    Error(PeekError),
}

impl From<mlua::Error> for Fail {
    fn from(error: mlua::Error) -> Self {
        Fail::Error(error.into())
    }
}

fn no_tojson(call: &Call) -> PeekError {
    argument_error(call, "Object does not have a `__tojson` metamethod.", &[])
}

fn argument_error(call: &Call, message: &str, inner: &[&str]) -> PeekError {
    let mut context: Vec<String> = inner.iter().map(|item| item.to_string()).collect();
    context.push("retrieving function argument object".into());
    context.push(format!("retrieving arguments for function {}", call.name));
    PeekError::Failure {
        message: message.into(),
        context,
    }
    .thrown()
}

/// Reads the JSON text of a document value into a [`Json`] value.
fn document_json(text: &str, max_depth: usize) -> Result<Json, Fail> {
    Parser::new(text, max_depth.saturating_mul(4))
        .document()
        .map_err(|()| Fail::Error(PeekError::Thrown("internal error: bad JSON".into())))
}

fn to_json(cx: &Cx, value: &Value, depth: usize) -> Result<Json, Fail> {
    if depth > cx.max_depth {
        return Err(Fail::Error(PeekError::Thrown(depth_message(cx.max_depth))));
    }
    Ok(match value {
        Value::Nil => Json::Null,
        Value::LightUserData(data) if data.0.is_null() => Json::Null,
        Value::Boolean(flag) => Json::Bool(*flag),
        Value::Integer(number) => Json::Number(*number as f64),
        Value::Number(number) => Json::Number(*number),
        Value::String(text) => Json::String(text.to_string_lossy()),
        Value::UserData(_) => {
            let thrown = |error: PeekError| Fail::Error(error.thrown());
            let type_name = with_node(value, Node::type_name).unwrap_or("");
            let text = match cx.object(type_name, value).map_err(thrown)? {
                Node::Inline(inline) => value_to_json(&inline),
                Node::Block(block) => value_to_json(&block),
                Node::Pandoc(doc) => randoc_ast::json::to_json(&doc),
                Node::Attr(attr) => value_to_json(&attr),
                Node::Citation(citation) => value_to_json(&citation),
                Node::Caption(caption) => value_to_json(&caption),
                Node::Cell(cell) => value_to_json(&cell),
                Node::Row(row) => value_to_json(&row),
                Node::TableHead(head) => value_to_json(&head),
                Node::TableFoot(foot) => value_to_json(&foot),
                Node::TableBody(body) => value_to_json(&body),
                Node::ListAttributes(attributes) => value_to_json(&attributes),
                Node::AttributeList(_) | Node::Version(_) => return Err(Fail::NoToJson),
            };
            document_json(&text, cx.max_depth)?
        }
        Value::Table(table) => table_json(cx, table, depth)?,
        _ => return Err(Fail::NoToJson),
    })
}

fn table_json(cx: &Cx, table: &Table, depth: usize) -> Result<Json, Fail> {
    if let Some(metatable) = table.metatable() {
        let name: Value = metatable.raw_get("__name")?;
        let list_name = match &name {
            Value::String(name) => Some(name.to_string_lossy()),
            _ => None,
        };
        let value = Value::Table(table.clone());
        match list_name.as_deref() {
            Some("Inlines") => {
                let items: Vec<Inline> = cx
                    .inlines_fuzzy(&value)
                    .map_err(|e| Fail::Error(e.thrown()))?;
                return document_json(&value_to_json(&items), cx.max_depth);
            }
            Some("Blocks") => {
                let items: Vec<Block> = cx
                    .blocks_fuzzy(&value)
                    .map_err(|e| Fail::Error(e.thrown()))?;
                return document_json(&value_to_json(&items), cx.max_depth);
            }
            _ => {}
        }
        if let Value::Function(tojson) = metatable.raw_get::<Value>("__tojson")? {
            let result: Value = tojson.call(value).map_err(|error| {
                // Pandoc gives the message of the error without a traceback.
                let message = runtime::message(&error);
                let message = match message.split_once("\nstack traceback:") {
                    Some((head, _)) => head.to_string(),
                    None => message,
                };
                Fail::Error(PeekError::Thrown(message))
            })?;
            return Ok(Json::Raw(match result {
                Value::String(text) => text.to_string_lossy(),
                other => other.to_string()?,
            }));
        }
    }
    let length = table.raw_len();
    if length > 0 {
        let mut items = Vec::with_capacity(length);
        for index in 1..=length {
            items.push(to_json(cx, &table.raw_get::<Value>(index)?, depth + 1)?);
        }
        return Ok(Json::Array(items));
    }
    let mut entries = BTreeMap::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, item) = pair?;
        let key = match &key {
            Value::String(text) => text.to_string_lossy(),
            Value::Integer(_) | Value::Number(_) => key.to_string()?,
            other => return Err(Fail::Key(described_type(other))),
        };
        entries.insert(key, to_json(cx, &item, depth + 1)?);
    }
    Ok(Json::Object(entries))
}

/// Writes JSON text as `aeson` does.
fn write(json: &Json, out: &mut String) {
    match json {
        Json::Null => out.push_str("null"),
        Json::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Json::Number(number) => out.push_str(&number_text(*number)),
        Json::String(text) => write_string(text, out),
        Json::Raw(text) => out.push_str(text),
        Json::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write(item, out);
            }
            out.push(']');
        }
        Json::Object(entries) => {
            out.push('{');
            for (index, (key, item)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write(item, out);
            }
            out.push('}');
        }
    }
}

fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The shortest decimal digits of a finite positive number and its
/// exponent `e`, with the value `0.d1d2... * 10^e` (`floatToDigits`).
fn digits(number: f64) -> (Vec<u8>, i64) {
    let text = format!("{number:e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let digits: Vec<u8> = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|byte| byte - b'0')
        .collect();
    let digits = match digits.iter().rposition(|d| *d != 0) {
        Some(last) => digits.get(..=last).unwrap_or_default().to_vec(),
        None => vec![0],
    };
    (digits, exponent.parse::<i64>().unwrap_or(0) + 1)
}

/// A number as `aeson` writes a `Scientific` made from a `Double`: an
/// integer value in full, each other value in the `Generic` form. An
/// infinite or not-a-number value becomes a number near the largest
/// `Double`, as in Pandoc.
fn number_text(number: f64) -> String {
    if number == 0.0 {
        return "0".into();
    }
    let sign = if number < 0.0 { "-" } else { "" };
    // GHC gives these digits for an infinite value and for not-a-number.
    let (ds, e) = if number.is_finite() {
        digits(number.abs())
    } else {
        (vec![1, 7, 9, 7, 6, 9, 3, 1, 3, 4, 8, 6, 2, 3, 1, 6], 309)
    };
    let count = ds.len() as i64;
    let text: String = ds.iter().map(|d| char::from(b'0' + d)).collect();
    if e >= count {
        let zeros = "0".repeat(usize::try_from(e - count).unwrap_or(0));
        return format!("{sign}{text}{zeros}");
    }
    let body = if !(0..=7).contains(&e) {
        let (first, rest) = text.split_at(1);
        let rest = if rest.is_empty() { "0" } else { rest };
        format!("{first}.{rest}e{}", e - 1)
    } else if e <= 0 {
        format!("0.{}{text}", "0".repeat(usize::try_from(-e).unwrap_or(0)))
    } else {
        let (whole, fraction) = text.split_at(usize::try_from(e).unwrap_or(0));
        let fraction = if fraction.is_empty() { "0" } else { fraction };
        format!("{whole}.{fraction}")
    };
    format!("{sign}{body}")
}

/// Tries the document types in the order of Pandoc.
fn decode_typed(lua: &Lua, text: &str) -> Peek<Option<Value>> {
    if let Ok(inline) = value_from_json::<Inline>(text) {
        return Ok(Some(push::inline(lua, &inline)?));
    }
    if let Ok(block) = value_from_json::<Block>(text) {
        return Ok(Some(push::block(lua, &block)?));
    }
    if let Ok(doc) = value_from_json::<Pandoc>(text) {
        return Ok(Some(push::pandoc(lua, &doc)?));
    }
    if let Ok(inlines) = value_from_json::<Vec<Inline>>(text) {
        return Ok(Some(push::inlines(lua, &inlines)?));
    }
    if let Ok(blocks) = value_from_json::<Vec<Block>>(text) {
        return Ok(Some(push::blocks(lua, &blocks)?));
    }
    Ok(None)
}

/// Converts a JSON value to Lua: an object to a table, an array to a
/// `pandoc.List`, each number to a float, `null` to `pandoc.json.null`.
fn to_lua(lua: &Lua, json: &Json) -> mlua::Result<Value> {
    Ok(match json {
        Json::Null => null(),
        Json::Bool(flag) => Value::Boolean(*flag),
        Json::Number(number) => Value::Number(*number),
        Json::String(text) | Json::Raw(text) => Value::String(lua.create_string(text)?),
        Json::Array(items) => {
            let values = items
                .iter()
                .map(|item| to_lua(lua, item))
                .collect::<mlua::Result<Vec<_>>>()?;
            push::list_of(lua, "List", values)?
        }
        Json::Object(entries) => {
            let table = lua.create_table()?;
            for (key, item) in entries {
                table.raw_set(key.as_str(), to_lua(lua, item)?)?;
            }
            Value::Table(table)
        }
    })
}

/// A JSON parser with the rules of `aeson`: the first of two equal keys
/// wins, and the nesting depth has a limit.
struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
    max_depth: usize,
}

type Parsed<T> = Result<T, ()>;

impl<'a> Parser<'a> {
    fn new(text: &'a str, max_depth: usize) -> Self {
        Parser {
            bytes: text.as_bytes(),
            at: 0,
            max_depth,
        }
    }

    fn document(&mut self) -> Parsed<Json> {
        let value = self.value(0)?;
        self.space();
        if self.at == self.bytes.len() {
            Ok(value)
        } else {
            Err(())
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Parsed<Json> {
        if self.bytes.get(self.at..self.at + word.len()) == Some(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(())
        }
    }

    fn value(&mut self, depth: usize) -> Parsed<Json> {
        if depth > self.max_depth {
            return Err(());
        }
        self.space();
        match self.peek().ok_or(())? {
            b'n' => self.literal("null", Json::Null),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            b'"' => self.string().map(Json::String),
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.peek() == Some(b']') {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.space();
                    match self.peek() {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Json::Array(items));
                        }
                        _ => return Err(()),
                    }
                }
            }
            b'{' => {
                self.at += 1;
                let mut entries = BTreeMap::new();
                self.space();
                if self.peek() == Some(b'}') {
                    self.at += 1;
                    return Ok(Json::Object(entries));
                }
                loop {
                    self.space();
                    if self.peek() != Some(b'"') {
                        return Err(());
                    }
                    let key = self.string()?;
                    self.space();
                    if self.peek() != Some(b':') {
                        return Err(());
                    }
                    self.at += 1;
                    let item = self.value(depth + 1)?;
                    entries.entry(key).or_insert(item);
                    self.space();
                    match self.peek() {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(Json::Object(entries));
                        }
                        _ => return Err(()),
                    }
                }
            }
            _ => self.number(),
        }
    }

    fn number(&mut self) -> Parsed<Json> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        let digits = |parser: &mut Self| {
            let begin = parser.at;
            while matches!(parser.peek(), Some(b'0'..=b'9')) {
                parser.at += 1;
            }
            parser.at - begin
        };
        let whole_start = self.at;
        let whole = digits(self);
        if whole == 0 || (whole > 1 && self.bytes.get(whole_start) == Some(&b'0')) {
            return Err(());
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if digits(self) == 0 {
                return Err(());
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if digits(self) == 0 {
                return Err(());
            }
        }
        let text =
            std::str::from_utf8(self.bytes.get(start..self.at).ok_or(())?).map_err(|_| ())?;
        // `0.0` in place of `-0.0`: a `Scientific` has no negative zero.
        text.parse::<f64>()
            .map(|number| Json::Number(number + 0.0))
            .map_err(|_| ())
    }

    fn hex4(&mut self) -> Parsed<u32> {
        let text = self.bytes.get(self.at..self.at + 4).ok_or(())?;
        let text = std::str::from_utf8(text).map_err(|_| ())?;
        let value = u32::from_str_radix(text, 16).map_err(|_| ())?;
        self.at += 4;
        Ok(value)
    }

    fn string(&mut self) -> Parsed<String> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            let byte = self.peek().ok_or(())?;
            self.at += 1;
            match byte {
                b'"' => return String::from_utf8(out).map_err(|_| ()),
                b'\\' => {
                    let escape = self.peek().ok_or(())?;
                    self.at += 1;
                    let c = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first)
                                && self.bytes.get(self.at..self.at + 2) == Some(b"\\u")
                            {
                                let save = self.at;
                                self.at += 2;
                                let second = self.hex4()?;
                                if (0xDC00..0xE000).contains(&second) {
                                    0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                                } else {
                                    self.at = save;
                                    return Err(());
                                }
                            } else {
                                first
                            };
                            char::from_u32(code).ok_or(())?
                        }
                        _ => return Err(()),
                    };
                    out.extend(c.encode_utf8(&mut [0; 4]).as_bytes());
                }
                0..0x20 => return Err(()),
                other => out.push(other),
            }
        }
    }
}
