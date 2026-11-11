//! Scalar resolution: the YAML 1.2 core schema, the standard tags and the
//! text form of numbers that Pandoc 3.13 prints.

use crate::{Node, Style};

/// The value of a node, as Pandoc resolves it.
#[derive(Debug, Clone, PartialEq)]
pub enum Value<'a> {
    /// `null`, `~`, an empty plain scalar, or NaN.
    Null,
    /// A Boolean value.
    Bool(bool),
    /// An integer that fits in 64 bits.
    Integer(i64),
    /// Another number, as the text that Pandoc prints, for example `1.5`,
    /// `1.0e-4`, `+inf` or `1.2345678901234567e29`.
    Float(String),
    /// A string.
    String(&'a str),
    /// A sequence.
    Sequence(&'a [Node]),
    /// A mapping, with its entries in source order.
    Mapping(&'a [(Node, Node)]),
}

/// The largest number of significant digits of a hexadecimal or octal number.
/// Pandoc has no limit; this limit bounds the time of the conversion.
const MAX_RADIX_DIGITS: usize = 20_000;

/// The range of the decimal exponent of a number.
const MAX_EXPONENT: i64 = 1000;

/// A number before its type: `digits × 10^exp10`.
struct Decimal {
    negative: bool,
    digits: String,
    exp10: i64,
    float: bool,
}

/// Resolves a scalar with its tag, or with the core schema when it is plain
/// and has no tag or the tag `!`. A hexadecimal or octal integer can have at
/// most 20000 significant digits (`MAX_RADIX_DIGITS`); Pandoc has no limit.
/// The conversion takes time that grows with the square of the digits: one
/// number of 19999 hexadecimal digits takes 0.036 s and 49 such numbers
/// (957 KB) take 4.5 s of wall time in a debug build on a loaded machine.
/// The error is the message of an invalid value.
pub(crate) fn resolve<'a>(
    text: &'a str,
    style: Style,
    tag: Option<&str>,
) -> Result<Value<'a>, String> {
    let invalid = |name: &str| format!("invalid value for the tag !!{name}");
    match tag {
        Some("!!str") => Ok(Value::String(text)),
        Some("!!null") => is_null(text)
            .then_some(Value::Null)
            .ok_or_else(|| invalid("null")),
        Some("!!bool") => bool_value(text).map(Value::Bool).ok_or_else(|| {
            if is_yaml11_bool(text) {
                format!(
                    "{}, \"{text}\" is a boolean only in YAML 1.1",
                    invalid("bool")
                )
            } else {
                invalid("bool")
            }
        }),
        Some("!!int") => match int_number(text) {
            Some(number) => number.value(text, false),
            None => Err(invalid("int")),
        },
        Some("!!float") => match float_number(text) {
            Some(number) => number.value(text, false),
            None => Err(invalid("float")),
        },
        None if style == Style::Plain => core_schema(text),
        _ => Ok(Value::String(text)),
    }
}

/// Returns `true` when a scalar resolves to NaN.
pub(crate) fn is_nan(text: &str, style: Style, tag: Option<&str>) -> bool {
    let nan = matches!(text, ".nan" | ".NaN" | ".NAN");
    nan && (tag == Some("!!float") || (tag.is_none() && style == Style::Plain))
}

fn core_schema(text: &str) -> Result<Value<'_>, String> {
    if is_null(text) {
        return Ok(Value::Null);
    }
    if let Some(value) = bool_value(text) {
        return Ok(Value::Bool(value));
    }
    match number(text) {
        Some(number) => number.value(text, true),
        None => Ok(Value::String(text)),
    }
}

fn is_null(text: &str) -> bool {
    matches!(text, "" | "~" | "null" | "Null" | "NULL")
}

fn bool_value(text: &str) -> Option<bool> {
    match text {
        "true" | "True" | "TRUE" => Some(true),
        "false" | "False" | "FALSE" => Some(false),
        _ => None,
    }
}

fn is_yaml11_bool(text: &str) -> bool {
    matches!(
        text.to_ascii_lowercase().as_str(),
        "y" | "yes" | "n" | "no" | "on" | "off"
    )
}

/// A parsed number, or one of the special floating-point values.
enum Number {
    Decimal(Decimal),
    Infinity {
        negative: bool,
    },
    NaN,
    /// A hexadecimal or octal integer with more than `MAX_RADIX_DIGITS` digits.
    TooLong,
}

impl Number {
    /// `hint` adds the advice to quote the value, which Pandoc gives only
    /// for a scalar without a tag.
    fn value<'a>(self, text: &str, hint: bool) -> Result<Value<'a>, String> {
        match self {
            Number::NaN => Ok(Value::Null),
            Number::Infinity { negative } => Ok(Value::Float(
                if negative { "-inf" } else { "+inf" }.to_string(),
            )),
            Number::Decimal(decimal) => decimal.value(text, hint),
            Number::TooLong => Err(format!(
                "the number has more than {MAX_RADIX_DIGITS} digits, quote the value if it is a string"
            )),
        }
    }
}

impl Decimal {
    fn value<'a>(self, text: &str, hint: bool) -> Result<Value<'a>, String> {
        let significant = self.digits.trim_start_matches('0');
        let mantissa = significant.trim_end_matches('0');
        if mantissa.is_empty() {
            return Ok(if self.float {
                Value::Float("0".to_string())
            } else {
                Value::Integer(0)
            });
        }
        let zeros = i64::try_from(significant.len() - mantissa.len()).unwrap_or(i64::MAX);
        let exp10 = self.exp10.saturating_add(zeros);
        let mantissa_len = i64::try_from(mantissa.len()).unwrap_or(i64::MAX);
        // The exponent of the scientific form d.ddd × 10^e.
        let exponent = mantissa_len.saturating_add(exp10).saturating_sub(1);
        if self.float && !(-MAX_EXPONENT..=MAX_EXPONENT).contains(&exponent) {
            let range = format!(
                "the exponent of the number is out of the range from -{MAX_EXPONENT} to {MAX_EXPONENT}"
            );
            return Err(if hint {
                format!("{range}, quote the value if it is a string, e.g. '{text}'")
            } else {
                range
            });
        }
        let sign = if self.negative { "-" } else { "" };
        if exp10 >= 0 && mantissa_len.saturating_add(exp10) <= 19 {
            let integer = format!(
                "{sign}{mantissa}{}",
                "0".repeat(usize::try_from(exp10).unwrap_or(0))
            );
            if let Ok(value) = integer.parse::<i64>() {
                return Ok(if self.float {
                    Value::Float(integer)
                } else {
                    Value::Integer(value)
                });
            }
        }
        Ok(Value::Float(format!(
            "{sign}{}",
            show_scientific(mantissa, exponent.saturating_add(1))
        )))
    }
}

fn number(text: &str) -> Option<Number> {
    int_number(text).or_else(|| float_number(text))
}

fn int_number(text: &str) -> Option<Number> {
    let radix_digits = |prefix: &str, radix: u32| {
        text.strip_prefix(prefix)
            .filter(|d| !d.is_empty() && d.chars().all(|c| c.is_digit(radix)))
            .map(|d| d.trim_start_matches('0'))
            .map(|d| (d, radix))
    };
    if let Some((digits, radix)) = radix_digits("0x", 16).or_else(|| radix_digits("0o", 8)) {
        if digits.len() > MAX_RADIX_DIGITS {
            return Some(Number::TooLong);
        }
        return Some(Number::Decimal(Decimal {
            negative: false,
            digits: to_decimal(digits, radix),
            exp10: 0,
            float: false,
        }));
    }
    let (negative, digits) = split_sign(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(Number::Decimal(Decimal {
        negative,
        digits: digits.to_string(),
        exp10: 0,
        float: false,
    }))
}

fn float_number(text: &str) -> Option<Number> {
    let (negative, body) = split_sign(text);
    if matches!(body, ".inf" | ".Inf" | ".INF") {
        return Some(Number::Infinity { negative });
    }
    if matches!(text, ".nan" | ".NaN" | ".NAN") {
        return Some(Number::NaN);
    }
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(i) => (body.get(..i)?, Some(body.get(i + 1..)?)),
        None => (body, None),
    };
    let (int_part, frac_part) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits_only = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if !digits_only(int_part) || !digits_only(frac_part) || int_part.len() + frac_part.len() == 0 {
        return None;
    }
    let exponent: i64 = match exponent {
        Some(e) => {
            let (exp_negative, exp_digits) = split_sign(e);
            if exp_digits.is_empty() || !digits_only(exp_digits) {
                return None;
            }
            let value: i64 = exp_digits.parse().unwrap_or(i64::MAX);
            if exp_negative { -value } else { value }
        }
        None => 0,
    };
    let frac_len = i64::try_from(frac_part.len()).ok()?;
    Some(Number::Decimal(Decimal {
        negative,
        digits: format!("{int_part}{frac_part}"),
        exp10: exponent.saturating_sub(frac_len),
        float: true,
    }))
}

fn split_sign(text: &str) -> (bool, &str) {
    if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else {
        (false, text.strip_prefix('+').unwrap_or(text))
    }
}

/// Converts digits in a radix to decimal digits. The work is about
/// `(n / 7) × (n / 8)` steps for `n` digits, which `MAX_RADIX_DIGITS` bounds.
fn to_decimal(digits: &str, radix: u32) -> String {
    const BASE: u64 = 1_000_000_000;
    // A chunk of digits has a value below 2^31.
    let chunk = if radix == 16 { 7 } else { 10 };
    let mut limbs: Vec<u64> = vec![0];
    let bytes = digits.as_bytes();
    for part in bytes.chunks(chunk) {
        let mut multiplier = 1u64;
        let mut carry = 0u64;
        for b in part {
            multiplier *= u64::from(radix);
            carry =
                carry * u64::from(radix) + u64::from(char::from(*b).to_digit(radix).unwrap_or(0));
        }
        crate::steps::add(limbs.len());
        for limb in limbs.iter_mut() {
            let value = *limb * multiplier + carry;
            *limb = value % BASE;
            carry = value / BASE;
        }
        while carry > 0 {
            limbs.push(carry % BASE);
            carry /= BASE;
        }
    }
    let mut out = String::new();
    for (i, limb) in limbs.iter().rev().enumerate() {
        if i == 0 {
            out.push_str(&limb.to_string());
        } else {
            out.push_str(&format!("{limb:09}"));
        }
    }
    out
}

/// Writes `0.digits × 10^exponent` like Haskell's `show` for `Scientific`.
fn show_scientific(digits: &str, exponent: i64) -> String {
    if !(0..=7).contains(&exponent) {
        let mut chars = digits.chars();
        let first = chars.next().unwrap_or('0');
        let rest = chars.as_str();
        let rest = if rest.is_empty() { "0" } else { rest };
        return format!("{first}.{rest}e{}", exponent - 1);
    }
    let point = usize::try_from(exponent).unwrap_or(0);
    if point == 0 {
        return format!("0.{digits}");
    }
    match (digits.get(..point), digits.get(point..)) {
        (Some(int_part), Some(frac_part)) if !frac_part.is_empty() => {
            format!("{int_part}.{frac_part}")
        }
        _ => format!(
            "{digits}{}.0",
            "0".repeat(point.saturating_sub(digits.len()))
        ),
    }
}
