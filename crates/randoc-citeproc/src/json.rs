//! JSON values and decoding errors with the rules of the Haskell library `aeson`.
//!
//! `aeson` keeps the first of two equal keys in an object, gives the keys in
//! code point order, and reports a decoding error with a path such as
//! `$[0]['date-parts'][0]`.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

/// A JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number.
    Number(Number),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Json>),
    /// An object. The first of two equal keys wins.
    Object(BTreeMap<String, Json>),
}

/// A JSON number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    /// A number with no fraction and no exponent that fits in 64 bits.
    Int(i64),
    /// Each other number.
    Float(f64),
}

impl Json {
    /// The name of the value type in an `aeson` error text.
    pub fn type_name(&self) -> &'static str {
        match self {
            Json::Null => "Null",
            Json::Bool(_) => "Boolean",
            Json::Number(_) => "Number",
            Json::String(_) => "String",
            Json::Array(_) => "Array",
            Json::Object(_) => "Object",
        }
    }
}

impl Number {
    /// The value as a Haskell `Int`, as `aeson` converts it: a whole number
    /// in the range of 64 bits.
    pub fn to_int(self) -> Result<i64, DecodeError> {
        match self {
            Number::Int(value) => Ok(value),
            Number::Float(value)
                if value.fract() == 0.0
                    && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0)
                        .contains(&value) =>
            {
                #[allow(clippy::cast_possible_truncation)]
                Ok(value as i64)
            }
            Number::Float(value) => Err(DecodeError::new(format!(
                "parsing Int failed, value is either floating or will cause over or underflow {}",
                randoc_ast::json::show_double(value)
            ))),
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Json, D::Error> {
        deserializer.deserialize_any(JsonVisitor)
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Json, E> {
        Ok(Json::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Json, E> {
        Ok(Json::Number(Number::Int(value)))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Json, E> {
        Ok(Json::Number(match i64::try_from(value) {
            Ok(value) => Number::Int(value),
            #[allow(clippy::cast_precision_loss)]
            Err(_) => Number::Float(value as f64),
        }))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Json, E> {
        Ok(Json::Number(Number::Float(value)))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Json, E> {
        Ok(Json::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Json, E> {
        Ok(Json::String(value))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Json::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut entries = BTreeMap::new();
        while let Some((key, value)) = map.next_entry::<String, Json>()? {
            entries.entry(key).or_insert(value);
        }
        Ok(Json::Object(entries))
    }
}

/// One step of a decoding path.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Index(usize),
    Key(String),
}

/// A decoding error: a message and the path of the value that has the error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError {
    /// The steps from the value to the root: the innermost step first.
    steps: Vec<Step>,
    message: String,
}

impl DecodeError {
    /// An error at the current value.
    pub fn new(message: impl Into<String>) -> DecodeError {
        DecodeError {
            steps: Vec::new(),
            message: message.into(),
        }
    }

    /// The `aeson` error of a value with the wrong type, from a parser with a name.
    pub fn mismatch_in(parser: &str, expected: &str, value: &Json) -> DecodeError {
        DecodeError::new(format!(
            "parsing {parser} failed, expected {expected}, but encountered {}",
            value.type_name()
        ))
    }

    /// The `aeson` error of `typeMismatch`.
    pub fn mismatch(expected: &str, value: &Json) -> DecodeError {
        DecodeError::new(format!(
            "expected {expected}, but encountered {}",
            value.type_name()
        ))
    }

    /// Moves the error under an array index.
    pub fn at_index(mut self, index: usize) -> DecodeError {
        self.steps.push(Step::Index(index));
        self
    }

    /// Moves the error under an object key.
    pub fn at_key(mut self, key: &str) -> DecodeError {
        self.steps.push(Step::Key(key.to_owned()));
        self
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Error in $")?;
        for step in self.steps.iter().rev() {
            match step {
                Step::Index(index) => write!(f, "[{index}]")?,
                Step::Key(key) if is_identifier(key) => write!(f, ".{key}")?,
                Step::Key(key) => {
                    f.write_str("['")?;
                    for c in key.chars() {
                        if c == '\'' || c == '\\' {
                            f.write_str("\\")?;
                        }
                        write!(f, "{c}")?;
                    }
                    f.write_str("']")?;
                }
            }
        }
        write!(f, ": {}", self.message)
    }
}

fn is_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(char::is_alphabetic) && chars.all(char::is_alphanumeric)
}

/// Reads a value with `parseJSON :: Text`.
pub fn text(value: &Json) -> Result<&str, DecodeError> {
    match value {
        Json::String(text) => Ok(text),
        other => Err(DecodeError::mismatch_in("Text", "String", other)),
    }
}

/// Reads a value with `parseJSON :: [a]`.
pub fn array(value: &Json) -> Result<&[Json], DecodeError> {
    match value {
        Json::Array(items) => Ok(items),
        other => Err(DecodeError::mismatch_in("[]", "Array", other)),
    }
}

/// Reads an optional field with `.:?`: a missing key and `null` give `None`.
pub fn optional<'a>(object: &'a BTreeMap<String, Json>, key: &str) -> Option<&'a Json> {
    object.get(key).filter(|value| **value != Json::Null)
}

/// Writes a string as `aeson` does.
pub fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Json {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn the_first_of_two_equal_keys_wins() {
        // Oracle: `[{"id":"a","id":"b"}]` gives the id `a`.
        let Json::Object(map) = parse(r#"{"id":"a","id":"b"}"#) else {
            panic!("object")
        };
        assert_eq!(map.get("id"), Some(&Json::String("a".into())));
    }

    #[test]
    fn paths_have_the_aeson_form() {
        let error = DecodeError::new("not a number")
            .at_index(0)
            .at_key("date-parts")
            .at_index(3);
        assert_eq!(
            error.to_string(),
            "Error in $[3]['date-parts'][0]: not a number"
        );
        let error = DecodeError::new("m").at_key("family").at_key("it's");
        assert_eq!(error.to_string(), r"Error in $['it\'s'].family: m");
        assert_eq!(DecodeError::new("m").to_string(), "Error in $: m");
    }

    #[test]
    fn numbers_convert_to_int_as_aeson_does() {
        assert_eq!(Number::Float(1000.0).to_int(), Ok(1000));
        assert_eq!(
            Number::Float(1.5).to_int().unwrap_err().message,
            "parsing Int failed, value is either floating or will cause over or underflow 1.5"
        );
        assert_eq!(
            Number::Float(1e30).to_int().unwrap_err().message,
            "parsing Int failed, value is either floating or will cause over or underflow 1.0e30"
        );
        let Json::Number(big) = parse("18446744073709551615") else {
            panic!("number")
        };
        assert!(big.to_int().is_err());
    }

    #[test]
    fn strings_have_the_aeson_escapes() {
        let mut out = String::new();
        write_string(&mut out, "a\"b\\c\nd\te\u{1}f<\u{7f}");
        assert_eq!(out, "\"a\\\"b\\\\c\\nd\\te\\u0001f<\u{7f}\"");
    }
}
