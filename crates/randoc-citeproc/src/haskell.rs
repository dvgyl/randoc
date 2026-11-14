//! Haskell text functions whose Rust counterparts differ: `isSpace`,
//! `Data.Text.strip` and `read` for an `Int`.

/// Haskell `isSpace`: tab to carriage return and the category Zs. Rust
/// `char::is_whitespace` also takes U+0085, U+2028 and U+2029.
pub fn is_space(c: char) -> bool {
    matches!(
        c,
        '\t'..='\r'
            | ' '
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

/// `Data.Text.strip`.
pub fn strip(text: &str) -> &str {
    text.trim_matches(is_space)
}

/// `read :: String -> Int` as `readMaybe` gives it: white space around the
/// number, parentheses around it, a minus sign with optional space, and
/// decimal, `0x` or `0o` digits. A plus sign, an exponent or a fraction
/// fails. A number outside 64 bits wraps, as `fromInteger` does.
pub fn read_int(text: &str) -> Option<i64> {
    let mut text = strip(text);
    while let Some(inner) = text.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        text = strip(inner);
    }
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start_matches(is_space)),
        None => (false, text),
    };
    let (radix, digits) = match digits.get(..2) {
        Some("0x" | "0X") => (16, &digits[2..]),
        Some("0o" | "0O") => (8, &digits[2..]),
        _ => (10, digits),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value = 0i64;
    for c in digits.chars() {
        let digit = c.to_digit(radix)?;
        value = value
            .wrapping_mul(i64::from(radix))
            .wrapping_add(i64::from(digit));
    }
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_int_has_the_oracle_forms() {
        // From the oracle matrix of `year` values (`tools/csljson_cases.py`).
        let cases = [
            ("1", Some(1)),
            ("+1", None),
            ("-1", Some(-1)),
            ("- 1", Some(-1)),
            ("(1)", Some(1)),
            ("(-1)", Some(-1)),
            ("( 1 )", Some(1)),
            (" 7 ", Some(7)),
            ("\t3\n", Some(3)),
            ("0x1F", Some(31)),
            ("0X1f", Some(31)),
            ("0o17", Some(15)),
            ("0b1", None),
            ("1e3", None),
            ("1.0", None),
            ("1_0", None),
            ("01", Some(1)),
            ("9223372036854775808", Some(i64::MIN)),
            ("18446744073709551617", Some(1)),
            ("-9223372036854775809", Some(i64::MAX)),
            ("\u{0661}", None),
            ("", None),
            ("x", None),
            ("1x", None),
            ("--1", None),
            ("0x", None),
            ("1 2", None),
        ];
        for (text, expected) in cases {
            assert_eq!(read_int(text), expected, "{text:?}");
        }
    }

    #[test]
    fn strip_keeps_the_separators_that_haskell_keeps() {
        assert_eq!(strip("\u{2028}x\u{85} \u{A0}"), "\u{2028}x\u{85}");
        assert_eq!(strip("\u{3000}\u{2009}x\u{202F}"), "x");
    }
}
