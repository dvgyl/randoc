//! The module `pandoc.text`.
//!
//! A port of `HsLua.Module.Text` of `hslua-module-text` 1.2.0 (MIT), and
//! the functions `subscript` and `superscript` of Pandoc, from the oracle
//! matrix. The case functions map each character alone, as `toUpper` and
//! `toLower` of `Data.Text` do (no rule for a final sigma).

use mlua::Value;

use crate::construct::Call;
use crate::peek::{Cx, Peek, PeekError, mismatch};
use crate::push;
use crate::utils::is_space;

/// Runs the function `name` of `pandoc.text`.
pub(crate) fn call(call: &Call, name: &str) -> Peek<Vec<Value>> {
    let cx = call.cx;
    let lua = cx.lua;
    let text = |value: &str| -> Peek<Vec<Value>> { Ok(vec![push::text(lua, value)?]) };
    let input = |parameter: &str| call.required(0, parameter, |v| cx.text(v));
    match name {
        "len" => Ok(vec![Value::Integer(input("s")?.chars().count() as i64)]),
        "lower" => text(
            &input("s")?
                .chars()
                .flat_map(char::to_lowercase)
                .collect::<String>(),
        ),
        "upper" => text(
            &input("s")?
                .chars()
                .flat_map(char::to_uppercase)
                .collect::<String>(),
        ),
        "reverse" => text(&input("s")?.chars().rev().collect::<String>()),
        "sub" => {
            let s = input("s")?;
            let start = call.required(1, "i", |v| cx.integral(v))?;
            let end = call.optional(2, "j", |v| cx.integral(v))?;
            text(&substring(&s, start, end.unwrap_or(-1)))
        }
        "subscript" | "superscript" => {
            let s = input("input")?;
            let table = if name == "subscript" {
                &SUBSCRIPTS
            } else {
                &SUPERSCRIPTS
            };
            let mapped: Option<String> = s.chars().map(|c| script(table, c)).collect();
            match mapped {
                Some(mapped) => text(&mapped),
                None => Ok(vec![Value::Nil]),
            }
        }
        "fromencoding" => {
            let bytes = call.required(0, "s", |v| string_bytes(cx, v))?;
            let encoding = call.optional(1, "encoding", |v| cx.text(v))?;
            let decoded = match encoding {
                None => decode_lenient(&bytes),
                Some(name) => Encoding::parse(&name)?.decode(&bytes)?,
            };
            text(&decoded)
        }
        "toencoding" => {
            let s = input("s")?;
            let encoding = call.optional(1, "enc", |v| cx.text(v))?;
            let bytes = match encoding {
                None => s.into_bytes(),
                Some(name) => Encoding::parse(&name)?.encode(&s)?,
            };
            Ok(vec![Value::String(lua.create_string(bytes)?)])
        }
        other => Err(PeekError::Thrown(format!(
            "internal error: unknown function {other}"
        ))),
    }
}

/// The bytes of a string or a number, as `lua_tolstring` gives them.
fn string_bytes(cx: &Cx, value: &Value) -> Peek<Vec<u8>> {
    match value {
        Value::String(text) => Ok(text.as_bytes().to_vec()),
        Value::Integer(_) | Value::Number(_) => match cx.lua.coerce_string(value.clone())? {
            Some(text) => Ok(text.as_bytes().to_vec()),
            None => Err(mismatch("string", value)),
        },
        _ => Err(mismatch("string", value)),
    }
}

/// A substring with the index rules of `string.sub` of Lua, on characters.
fn substring(s: &str, start: i64, end: i64) -> String {
    let chars: Vec<char> = s.chars().collect();
    let length = chars.len() as i64;
    let from_start = if start >= 0 {
        start.wrapping_sub(1)
    } else {
        length.wrapping_add(start)
    };
    let from_end = if end < 0 {
        end.wrapping_neg().wrapping_sub(1)
    } else {
        length.wrapping_sub(end)
    };
    let first = usize::try_from(from_start.max(0)).unwrap_or(usize::MAX);
    let dropped_end = usize::try_from(from_end.max(0)).unwrap_or(usize::MAX);
    let rest = chars.get(first..).unwrap_or_default();
    let keep = rest.len().saturating_sub(dropped_end);
    rest.iter().take(keep).collect()
}

const SUBSCRIPTS: [(char, char); 16] = [
    ('\u{2212}', '₋'),
    ('(', '₍'),
    (')', '₎'),
    ('+', '₊'),
    ('-', '₋'),
    ('0', '₀'),
    ('1', '₁'),
    ('2', '₂'),
    ('3', '₃'),
    ('4', '₄'),
    ('5', '₅'),
    ('6', '₆'),
    ('7', '₇'),
    ('8', '₈'),
    ('9', '₉'),
    ('=', '₌'),
];

const SUPERSCRIPTS: [(char, char); 16] = [
    ('\u{2212}', '⁻'),
    ('(', '⁽'),
    (')', '⁾'),
    ('+', '⁺'),
    ('-', '⁻'),
    ('0', '⁰'),
    ('1', '¹'),
    ('2', '²'),
    ('3', '³'),
    ('4', '⁴'),
    ('5', '⁵'),
    ('6', '⁶'),
    ('7', '⁷'),
    ('8', '⁸'),
    ('9', '⁹'),
    ('=', '⁼'),
];

/// The script form of a character. White space stays.
fn script(table: &[(char, char)], c: char) -> Option<char> {
    if is_space(c) {
        return Some(c);
    }
    table.iter().find(|(from, _)| *from == c).map(|(_, to)| *to)
}

/// Decodes UTF-8 as the file system encoding of GHC does: each byte that
/// is not part of a valid sequence becomes U+FFFD.
fn decode_lenient(bytes: &[u8]) -> String {
    let mut text = String::new();
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                text.push_str(valid);
                return text;
            }
            Err(error) => {
                let (valid, after) = rest.split_at(error.valid_up_to());
                text.push_str(std::str::from_utf8(valid).unwrap_or_default());
                let bad = error.error_len().unwrap_or(after.len());
                text.extend(std::iter::repeat_n('\u{FFFD}', bad));
                rest = after.get(bad..).unwrap_or_default();
            }
        }
    }
}

/// The encodings that GHC has without `iconv`.
#[derive(Clone, Copy)]
enum Encoding {
    Utf8,
    Utf16 { little: bool },
    Utf32 { little: bool },
    Latin1,
    Ascii,
}

fn encode_error(c: char) -> PeekError {
    let shown = if c.is_ascii_graphic() || c == ' ' {
        c.to_string()
    } else {
        format!("\\{}", u32::from(c))
    };
    PeekError::Thrown(format!(
        "recoverEncode: invalid argument (cannot encode character '{shown}')"
    ))
}

fn decode_error(byte: u8) -> PeekError {
    PeekError::Thrown(format!(
        "recoverDecode: invalid argument (cannot decode byte sequence starting from {byte})"
    ))
}

impl Encoding {
    /// Reads an encoding name as `mkTextEncoding` does: the case, `-` and
    /// `_` do not matter, and a suffix such as `//IGNORE` is removed.
    fn parse(name: &str) -> Peek<Encoding> {
        let base = name.split("//").next().unwrap_or_default();
        let key: String = base
            .chars()
            .filter(|c| *c != '-' && *c != '_')
            .map(|c| c.to_ascii_uppercase())
            .collect();
        Ok(match key.as_str() {
            "UTF8" => Encoding::Utf8,
            "UTF16" | "UTF16BE" => Encoding::Utf16 { little: false },
            "UTF16LE" => Encoding::Utf16 { little: true },
            "UTF32" | "UTF32BE" => Encoding::Utf32 { little: false },
            "UTF32LE" => Encoding::Utf32 { little: true },
            "LATIN1" | "ISO88591" | "L1" => Encoding::Latin1,
            "ASCII" | "USASCII" => Encoding::Ascii,
            _ => {
                return Err(PeekError::Thrown(format!(
                    "mkTextEncoding: does not exist (unknown encoding:{name})"
                )));
            }
        })
    }

    fn encode(self, text: &str) -> Peek<Vec<u8>> {
        let mut bytes = Vec::new();
        for c in text.chars() {
            match self {
                Encoding::Utf8 => bytes.extend(c.encode_utf8(&mut [0; 4]).as_bytes()),
                Encoding::Utf16 { little } => {
                    for unit in c.encode_utf16(&mut [0; 2]) {
                        let unit = *unit;
                        bytes.extend(if little {
                            unit.to_le_bytes()
                        } else {
                            unit.to_be_bytes()
                        });
                    }
                }
                Encoding::Utf32 { little } => {
                    let code = u32::from(c);
                    bytes.extend(if little {
                        code.to_le_bytes()
                    } else {
                        code.to_be_bytes()
                    });
                }
                Encoding::Latin1 | Encoding::Ascii => {
                    let limit = if matches!(self, Encoding::Latin1) {
                        0xFF
                    } else {
                        0x7F
                    };
                    match u8::try_from(u32::from(c)) {
                        Ok(byte) if u32::from(byte) <= limit => bytes.push(byte),
                        _ => return Err(encode_error(c)),
                    }
                }
            }
        }
        Ok(bytes)
    }

    fn decode(self, bytes: &[u8]) -> Peek<String> {
        match self {
            Encoding::Utf8 => match std::str::from_utf8(bytes) {
                Ok(text) => Ok(text.to_string()),
                Err(error) => Err(decode_error(
                    bytes.get(error.valid_up_to()).copied().unwrap_or(0),
                )),
            },
            Encoding::Utf16 { little } => {
                let units = bytes.chunks(2).map(|pair| match pair {
                    [a, b] => Ok(if little {
                        u16::from_le_bytes([*a, *b])
                    } else {
                        u16::from_be_bytes([*a, *b])
                    }),
                    [a] => Err(*a),
                    _ => Err(0),
                });
                let mut text = String::new();
                let mut valid = Vec::new();
                for unit in units {
                    valid.push(unit.map_err(decode_error)?);
                }
                for c in char::decode_utf16(valid) {
                    match c {
                        Ok(c) => text.push(c),
                        Err(error) => {
                            let unit = error.unpaired_surrogate();
                            let first = if little {
                                unit as u8
                            } else {
                                (unit >> 8) as u8
                            };
                            return Err(decode_error(first));
                        }
                    }
                }
                Ok(text)
            }
            Encoding::Utf32 { little } => {
                let mut text = String::new();
                for chunk in bytes.chunks(4) {
                    let Ok(quad) = <[u8; 4]>::try_from(chunk) else {
                        return Err(decode_error(chunk.first().copied().unwrap_or(0)));
                    };
                    let code = if little {
                        u32::from_le_bytes(quad)
                    } else {
                        u32::from_be_bytes(quad)
                    };
                    match char::from_u32(code) {
                        Some(c) => text.push(c),
                        None => return Err(decode_error(quad[0])),
                    }
                }
                Ok(text)
            }
            Encoding::Latin1 => Ok(bytes.iter().map(|byte| char::from(*byte)).collect()),
            Encoding::Ascii => bytes
                .iter()
                .map(|byte| {
                    if byte.is_ascii() {
                        Ok(char::from(*byte))
                    } else {
                        Err(decode_error(*byte))
                    }
                })
                .collect(),
        }
    }
}
