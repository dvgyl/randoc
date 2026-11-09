//! Pandoc-compatible JSON for the document model.
//!
//! The output has the same bytes as the output of `pandoc -t json`. Pandoc
//! writes JSON with the Haskell library `aeson`, so numbers and string escapes
//! follow the rules of that library.

use std::io;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::ser::{CharEscape, Formatter};

use crate::Pandoc;

/// An error from [`from_json`].
#[derive(Debug)]
pub struct JsonError(serde_json::Error);

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for JsonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl JsonError {
    /// The 1-based line of the error, or 0 when the error has no position.
    pub fn line(&self) -> usize {
        self.0.line()
    }

    /// The 1-based column of the error, or 0 when the error has no position.
    pub fn column(&self) -> usize {
        self.0.column()
    }
}

/// Writes a document as JSON, with the same bytes as `pandoc -t json` and no final newline.
pub fn to_json(doc: &Pandoc) -> String {
    value_to_json(doc)
}

/// Writes a part of a document as JSON, for example one block for a filter.
pub fn value_to_json<T: Serialize + ?Sized>(value: &T) -> String {
    let mut out = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, AesonFormatter);
    // The document model has string keys only and the buffer does not fail,
    // so an error is not possible here. An empty string is the safe fallback.
    if value.serialize(&mut serializer).is_err() {
        return String::new();
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Writes a document as JSON to `writer`.
pub fn write_json<W: io::Write>(doc: &Pandoc, writer: W) -> io::Result<()> {
    let mut serializer = serde_json::Serializer::with_formatter(writer, AesonFormatter);
    doc.serialize(&mut serializer).map_err(io::Error::from)
}

/// Reads a document from JSON.
///
/// This function sets no nesting limit. For untrusted input, call
/// [`nesting_depth`] first and reject text that is too deep for the stack.
pub fn from_json(text: &str) -> Result<Pandoc, JsonError> {
    value_from_json(text)
}

/// Reads a part of a document from JSON, for example the output of a filter.
pub fn value_from_json<T: DeserializeOwned>(text: &str) -> Result<T, JsonError> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    deserializer.disable_recursion_limit();
    let value = T::deserialize(&mut deserializer).map_err(JsonError)?;
    deserializer.end().map_err(JsonError)?;
    Ok(value)
}

/// Returns the largest depth of arrays and objects in JSON text.
///
/// The scan does not validate the text. It only counts brackets outside strings.
pub fn nesting_depth(text: &str) -> usize {
    let mut depth = 0usize;
    let mut max_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in text.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                max_depth = max_depth.max(depth);
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    max_depth
}

/// Formats numbers and string escapes as `aeson` does.
struct AesonFormatter;

impl Formatter for AesonFormatter {
    fn write_f64<W: ?Sized + io::Write>(&mut self, writer: &mut W, value: f64) -> io::Result<()> {
        writer.write_all(show_double(value).as_bytes())
    }

    fn write_char_escape<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        char_escape: CharEscape,
    ) -> io::Result<()> {
        match char_escape {
            CharEscape::Quote => writer.write_all(b"\\\""),
            CharEscape::ReverseSolidus => writer.write_all(b"\\\\"),
            CharEscape::Solidus => writer.write_all(b"/"),
            CharEscape::LineFeed => writer.write_all(b"\\n"),
            CharEscape::CarriageReturn => writer.write_all(b"\\r"),
            CharEscape::Tab => writer.write_all(b"\\t"),
            CharEscape::Backspace => writer.write_all(b"\\u0008"),
            CharEscape::FormFeed => writer.write_all(b"\\u000c"),
            CharEscape::AsciiControl(byte) => write!(writer, "\\u{byte:04x}"),
        }
    }
}

/// Formats a number as the Haskell function `show` does for a `Double`.
///
/// Values from 0.1 up to 10^7 use fixed notation with one or more decimal
/// digits. Other values use the form `1.0e-3`.
pub fn show_double(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let magnitude = value.abs();
    if magnitude == 0.0 {
        return format!("{sign}0.0");
    }

    // `{:e}` gives the shortest digits that read back as the same number.
    let scientific = format!("{magnitude:e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();

    if (0.1..1.0e7).contains(&magnitude) {
        if exponent < 0 {
            let zeros = "0".repeat((-exponent - 1) as usize);
            return format!("{sign}0.{zeros}{digits}");
        }
        let int_len = exponent as usize + 1;
        if digits.len() <= int_len {
            let zeros = "0".repeat(int_len - digits.len());
            return format!("{sign}{digits}{zeros}.0");
        }
        let (int_part, frac_part) = digits.split_at(int_len);
        return format!("{sign}{int_part}.{frac_part}");
    }

    let (first, rest) = digits.split_at(1);
    let rest = if rest.is_empty() { "0" } else { rest };
    format!("{sign}{first}.{rest}e{exponent}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Block, Inline};

    #[test]
    fn show_double_matches_pandoc_output() {
        // Each expected string is from `pandoc -f json -t json` (Pandoc 3.13).
        let cases = [
            (0.5, "0.5"),
            (1.0, "1.0"),
            (0.001, "1.0e-3"),
            (0.1, "0.1"),
            (0.333_333_333_333_333_3, "0.3333333333333333"),
            (12_345_678.9, "1.23456789e7"),
            (1e22, "1.0e22"),
            (2.0, "2.0"),
            (1e-7, "1.0e-7"),
        ];
        for (value, expected) in cases {
            assert_eq!(show_double(value), expected);
        }
    }

    #[test]
    fn show_double_handles_range_limits_and_signs() {
        assert_eq!(show_double(0.0), "0.0");
        assert_eq!(show_double(-0.0), "-0.0");
        assert_eq!(show_double(-2.5), "-2.5");
        assert_eq!(show_double(9_999_999.0), "9999999.0");
        assert_eq!(show_double(1.0e7), "1.0e7");
        assert_eq!(show_double(0.099), "9.9e-2");
        assert_eq!(show_double(120.0), "120.0");
        assert_eq!(show_double(f64::NAN), "NaN");
        assert_eq!(show_double(f64::INFINITY), "Infinity");
        assert_eq!(show_double(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn string_escapes_match_pandoc_output() {
        // The expected string is from `pandoc -f json -t json` (Pandoc 3.13).
        let doc = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Para(vec![Inline::Str(
                "a\u{1}\u{7f}\u{8}\u{c}\n\t<>&\u{e9}\u{2028}\u{1f600}/\\\"\r".into(),
            )])],
        };
        let expected = concat!(
            r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"a\u0001"#,
            "\u{7f}",
            r#"\u0008\u000c\n\t<>&"#,
            "\u{e9}\u{2028}\u{1f600}",
            r#"/\\\"\r"}]}]}"#,
        );
        assert_eq!(to_json(&doc), expected);
    }

    #[test]
    fn nesting_depth_ignores_brackets_in_strings() {
        assert_eq!(nesting_depth(r#"{"a":[1,[2]],"b":"[[[[\"[["}"#), 3);
        assert_eq!(nesting_depth("1"), 0);
        assert_eq!(nesting_depth("]]"), 0);
    }

    #[test]
    fn reads_deep_documents_without_the_default_recursion_limit() {
        let depth = 200;
        let mut text = String::from(r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":["#);
        for _ in 0..depth {
            text.push_str(r#"{"t":"BlockQuote","c":["#);
        }
        for _ in 0..depth {
            text.push_str("]}");
        }
        text.push_str("]}");
        let doc = from_json(&text).unwrap();
        assert_eq!(nesting_depth(&text), 2 * depth + 2);
        let mut levels = 0;
        let mut blocks = &doc.blocks;
        while let Some(Block::BlockQuote(inner)) = blocks.first() {
            levels += 1;
            blocks = inner;
        }
        assert_eq!(levels, depth);
    }

    #[test]
    fn rejects_text_after_the_document() {
        let text = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]} x"#;
        assert!(from_json(text).is_err());
    }

    #[test]
    fn version_errors_match_pandoc_messages() {
        // Pandoc 3.13 prints these messages after "JSON parse error: Error in $: ".
        let old = from_json(r#"{"pandoc-api-version":[1,22],"meta":{},"blocks":[]}"#).unwrap_err();
        assert!(old.to_string().starts_with(
            "Incompatible API versions: encoded with [1,22] but attempted to decode with [1,23,1,2]."
        ));
        let missing = from_json(r#"{"meta":{},"blocks":[]}"#).unwrap_err();
        assert!(
            missing
                .to_string()
                .starts_with("JSON missing pandoc-api-version.")
        );
    }

    #[test]
    fn version_error_comes_before_a_content_error() {
        let text = r#"{"blocks":[{"t":"Nonsense"}],"meta":{},"pandoc-api-version":[2,0]}"#;
        let error = from_json(text).unwrap_err();
        assert!(error.to_string().starts_with("Incompatible API versions"));
    }

    #[test]
    fn accepts_a_version_with_the_same_first_two_parts() {
        let text = r#"{"pandoc-api-version":[1,23],"meta":{},"blocks":[]}"#;
        assert_eq!(from_json(text).unwrap(), Pandoc::default());
        let short = r#"{"pandoc-api-version":[1],"meta":{},"blocks":[]}"#;
        assert!(from_json(short).is_err());
    }
}
