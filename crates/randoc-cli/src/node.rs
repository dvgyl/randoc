//! The JSON view of a YAML node from `randoc-yaml`, as Pandoc decodes a
//! defaults file or a metadata file through Aeson, and a JSON reader that
//! gives the same nodes.

use randoc_yaml::{Kind, Mark, Node, Style, Value, YamlError};

/// The accessors that the decoders of the program use.
pub trait NodeExt {
    /// The position of the node.
    fn mark(&self) -> Mark;
    /// The source text of a scalar, for a mapping key or a name.
    fn as_str(&self) -> Option<&str>;
    /// The value of the node. Null for NaN.
    fn resolve(&self) -> Value<'_>;
    /// True for null.
    fn is_null(&self) -> bool;
    /// The Boolean value.
    fn as_bool(&self) -> Option<bool>;
    /// True for a JSON string.
    fn is_string(&self) -> bool;
    /// The text of a scalar as Pandoc prints a YAML value as a string: a
    /// number in its normal form, null as an empty string.
    fn text_form(&self) -> Option<String>;
    /// The name of the JSON type, as Aeson prints it in an error.
    fn type_name(&self) -> &'static str;
    /// The JSON value.
    fn to_json(&self) -> serde_json::Value;
}

impl NodeExt for Node {
    fn mark(&self) -> Mark {
        self.mark
    }

    fn as_str(&self) -> Option<&str> {
        self.scalar_text()
    }

    fn resolve(&self) -> Value<'_> {
        // `randoc_yaml::parse` checks each tagged scalar, so a parsed node
        // always has a value.
        self.value().unwrap_or(Value::Null)
    }

    fn is_null(&self) -> bool {
        self.resolve() == Value::Null
    }

    fn as_bool(&self) -> Option<bool> {
        match self.resolve() {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }

    fn is_string(&self) -> bool {
        matches!(self.resolve(), Value::String(_))
    }

    fn text_form(&self) -> Option<String> {
        Some(match self.resolve() {
            Value::Null => String::new(),
            Value::Bool(b) => b.to_string(),
            Value::Integer(n) => n.to_string(),
            Value::Float(text) => text,
            Value::String(text) => text.to_owned(),
            Value::Sequence(_) | Value::Mapping(_) => return None,
        })
    }

    fn type_name(&self) -> &'static str {
        match self.resolve() {
            Value::Null => "Null",
            Value::Bool(_) => "Boolean",
            Value::Integer(_) | Value::Float(_) => "Number",
            Value::String(_) => "String",
            Value::Sequence(_) => "Array",
            Value::Mapping(_) => "Object",
        }
    }

    fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as Json;
        match self.resolve() {
            Value::Null => Json::Null,
            Value::Bool(b) => Json::Bool(b),
            Value::Integer(n) => Json::from(n),
            Value::Float(text) => text
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map_or(Json::String(text), Json::Number),
            Value::String(text) => Json::String(text.to_owned()),
            Value::Sequence(items) => Json::Array(items.iter().map(NodeExt::to_json).collect()),
            Value::Mapping(entries) => Json::Object(
                entries
                    .iter()
                    .map(|(k, v)| (k.text_form().unwrap_or_default(), v.to_json()))
                    .collect(),
            ),
        }
    }
}

const START: Mark = Mark { line: 1, column: 1 };

fn scalar(text: String, style: Style) -> Node {
    Node {
        mark: START,
        tag: None,
        kind: Kind::Scalar { text, style },
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
        Ok(FirstWins(scalar(v.to_string(), Style::Plain)))
    }
    fn visit_i64<E>(self, v: i64) -> Result<FirstWins, E> {
        Ok(FirstWins(scalar(v.to_string(), Style::Plain)))
    }
    fn visit_u64<E>(self, v: u64) -> Result<FirstWins, E> {
        Ok(FirstWins(scalar(v.to_string(), Style::Plain)))
    }
    fn visit_f64<E>(self, v: f64) -> Result<FirstWins, E> {
        Ok(FirstWins(scalar(
            serde_json::Value::from(v).to_string(),
            Style::Plain,
        )))
    }
    fn visit_str<E>(self, v: &str) -> Result<FirstWins, E> {
        Ok(FirstWins(scalar(v.to_owned(), Style::DoubleQuoted)))
    }
    fn visit_unit<E>(self) -> Result<FirstWins, E> {
        Ok(FirstWins(scalar("null".to_owned(), Style::Plain)))
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<FirstWins, A::Error> {
        let mut items = Vec::new();
        while let Some(FirstWins(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(FirstWins(Node {
            mark: START,
            tag: None,
            kind: Kind::Sequence(items),
        }))
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<FirstWins, A::Error> {
        let mut entries: Vec<(Node, Node)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        while let Some((key, FirstWins(value))) = map.next_entry::<String, FirstWins>()? {
            if seen.insert(key.clone()) {
                entries.push((scalar(key, Style::DoubleQuoted), value));
            }
        }
        Ok(FirstWins(Node {
            mark: START,
            tag: None,
            kind: Kind::Mapping(entries),
        }))
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

/// Returns the text of a YAML file in the encoding that its first bytes
/// give, as the YAML specification (section 5.2) detects it: UTF-32 and
/// UTF-16 in both byte orders, with or without a byte order mark, else UTF-8
/// (see [`utf8_text`]). An invalid UTF-16 or UTF-32 sequence becomes U+FFFD.
pub fn decode_text(bytes: &[u8]) -> Result<std::borrow::Cow<'_, str>, YamlError> {
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

/// Returns the text of a YAML file, or the error of Pandoc at the first byte
/// that is not UTF-8. The error shows the line up to that byte.
pub fn utf8_text(bytes: &[u8]) -> Result<&str, YamlError> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    std::str::from_utf8(bytes).map_err(|error| {
        let valid = std::str::from_utf8(bytes.get(..error.valid_up_to()).unwrap_or_default())
            .unwrap_or_default();
        let line_start = valid.rfind('\n').map_or(0, |index| index + 1);
        let mark = Mark {
            line: valid.matches('\n').count() + 1,
            column: valid.get(line_start..).unwrap_or_default().chars().count() + 1,
        };
        YamlError::at(valid, mark, "invalid UTF-8")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaml(text: &str) -> Node {
        randoc_yaml::parse_single(text, randoc_yaml::Limits::default()).unwrap()
    }

    #[test]
    fn json_keeps_the_first_of_two_equal_keys_and_the_order() {
        let node = parse_json(r#"{"b":1,"a":{"x":1,"x":2},"b":2,"c":"1.5","d":2.5}"#).unwrap();
        let Kind::Mapping(entries) = &node.kind else {
            panic!("not a map")
        };
        let keys: Vec<_> = entries.iter().filter_map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["b", "a", "c", "d"]);
        assert_eq!(entries[0].1.resolve(), Value::Integer(1));
        assert_eq!(entries[2].1.type_name(), "String");
        assert_eq!(entries[3].1.type_name(), "Number");
        assert_eq!(node.to_json()["a"]["x"], 1);
        assert_eq!(
            parse_json("{").unwrap_err(),
            "Unexpected end-of-input, expecting JSON value"
        );
        assert!(parse_json("{} x").is_err());
    }

    #[test]
    fn the_json_view_follows_aeson() {
        let node = yaml("a: 1\nb: [x, 'y']\nc: &k {d: true}\ne: *k\nf: ~\ng: 1.50\nh: '2'\n");
        assert_eq!(
            node.to_json(),
            serde_json::json!({"a": 1, "b": ["x", "y"], "c": {"d": true}, "e": {"d": true},
                "f": null, "g": 1.5, "h": "2"})
        );
        let names: Vec<_> = match &node.kind {
            Kind::Mapping(entries) => entries.iter().map(|(_, v)| v.type_name()).collect(),
            _ => panic!("not a map"),
        };
        assert_eq!(
            names,
            [
                "Number", "Array", "Object", "Object", "Null", "Number", "String"
            ]
        );
        assert_eq!(yaml("1.0e20\n").text_form().as_deref(), Some("1.0e20"));
        assert_eq!(yaml("null\n").text_form().as_deref(), Some(""));
        assert_eq!(yaml("[a]\n").text_form(), None);
    }
}
