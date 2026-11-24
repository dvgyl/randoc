//! Backslash escapes for text in Markdown output.

use std::fmt::Write;

use randoc_ast::Text;
use randoc_core::Extension;

use crate::{Variant, W};

/// Returns `true` when `rest` (the text after an `&`) starts with an entity reference.
fn starts_with_entity(rest: &str) -> bool {
    let Some(end) = rest.bytes().take(40).position(|b| b == b';') else {
        return false;
    };
    let body = &rest[..end];
    if let Some(number) = body.strip_prefix('#') {
        if let Some(hex) = number.strip_prefix(['x', 'X']) {
            return !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit());
        }
        return !number.is_empty() && number.chars().all(|c| c.is_ascii_digit());
    }
    randoc_core::shared::named_entity(&rest[..=end]).is_some()
}

fn unsmart(c: char) -> Option<&'static str> {
    Some(match c {
        '‘' | '’' => "'",
        '“' | '”' => "\"",
        '—' => "---",
        '–' => "--",
        '…' => "...",
        _ => return None,
    })
}

fn ascii_text(text: Text, prefer_ascii: bool) -> Text {
    if !prefer_ascii || text.is_ascii() {
        return text;
    }
    let mut out = Text::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
        } else if let Some(name) = crate::entities::name(c) {
            out.push('&');
            out.push_str(name);
            out.push(';');
        } else {
            let _ = write!(out, "&#{};", u32::from(c));
        }
    }
    out
}

impl W<'_> {
    /// Escapes the text of a `Str` element.
    pub(crate) fn escape_text(&self, text: &str) -> Text {
        let smart = self.has(Extension::Smart);
        if self.plain() {
            let text = if smart {
                let mut out = Text::with_capacity(text.len());
                for c in text.chars() {
                    match unsmart(c) {
                        Some(punctuation) => out.push_str(punctuation),
                        None => out.push(c),
                    }
                }
                out
            } else {
                text.to_owned()
            };
            return ascii_text(text, self.opts.prefer_ascii);
        }
        if self.variant == Variant::Markua {
            return text.replace("//", "~/~/");
        }
        let commonmark = self.variant == Variant::Commonmark;
        let escapable = commonmark || self.has(Extension::AllSymbolsEscapable);
        // Pandoc escapes each backslash only where a backslash can start raw TeX.
        let raw_tex = self.variant == Variant::Markdown && self.has(Extension::RawTex);
        // A run of three colons could open or close a fenced div.
        let fenced_divs = self.has(Extension::FencedDivs);
        let chars: Vec<char> = text.chars().collect();
        let offsets: Vec<usize> = text.char_indices().map(|(offset, _)| offset).collect();
        let mut out = Text::with_capacity(text.len() + 8);
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let prev = if i == 0 {
                None
            } else {
                chars.get(i - 1).copied()
            };
            let next = chars.get(i + 1).copied();
            match c {
                '!' if next == Some('[') => {
                    out.push_str("\\![");
                    i += 1;
                }
                '\\' if !raw_tex
                    && next.is_some_and(|n| !n.is_ascii_punctuation() && !n.is_whitespace()) =>
                {
                    out.push('\\');
                }
                '\\' | '*' | '`' | '[' | ']' => {
                    out.push('\\');
                    out.push(c);
                }
                '_' => {
                    let inside = (commonmark || self.has(Extension::IntrawordUnderscores))
                        && prev.is_some_and(char::is_alphanumeric)
                        && next.is_some_and(char::is_alphanumeric);
                    if !inside {
                        out.push('\\');
                    }
                    out.push('_');
                }
                '<' if escapable => out.push_str("\\<"),
                '<' => out.push_str("&lt;"),
                '>' if escapable => out.push_str("\\>"),
                '>' => out.push_str("&gt;"),
                '$' if self.has(Extension::TexMathDollars) => out.push_str("\\$"),
                '=' if next == Some('=') && self.has(Extension::Mark) => out.push_str("\\="),
                '^' if self.has(Extension::Superscript) => out.push_str("\\^"),
                '~' if self.has(Extension::Subscript)
                    || (self.has(Extension::Strikeout) && next == Some('~')) =>
                {
                    out.push_str("\\~");
                }
                '|' if self.has(Extension::PipeTables) => out.push_str("\\|"),
                '\'' | '"' if smart => {
                    out.push('\\');
                    out.push(c);
                }
                '-' if smart && next == Some('-') => {
                    if chars.get(i + 2) == Some(&'-') {
                        out.push_str("\\-");
                    } else {
                        out.push_str("\\--");
                        i += 1;
                    }
                }
                '.' if smart && next == Some('.') && chars.get(i + 2) == Some(&'.') => {
                    out.push_str("\\...");
                    i += 2;
                }
                '@' if i == 0
                    && self.has(Extension::Citations)
                    && next.is_some_and(|n| n.is_alphanumeric() || n == '_') =>
                {
                    out.push_str("\\@");
                }
                '&' if starts_with_entity(
                    text.get(offsets.get(i + 1).copied().unwrap_or(text.len())..)
                        .unwrap_or(""),
                ) =>
                {
                    out.push_str("\\&");
                }
                '#' if i == 0 && (commonmark || chars.iter().all(|&h| h == '#')) => {
                    out.push_str("\\#");
                }
                ':' if fenced_divs && next == Some(':') && chars.get(i + 2) == Some(&':') => {
                    out.push_str("\\:::");
                    i += 2;
                }
                _ => match smart.then(|| unsmart(c)).flatten() {
                    Some(punctuation) => out.push_str(punctuation),
                    None => out.push(c),
                },
            }
            i += 1;
        }
        ascii_text(out, self.opts.prefer_ascii)
    }

    /// Escapes a list marker at the start of a line.
    ///
    /// `text` is the escaped text of the first `Str` of a line. `alone` is
    /// `true` when a space or the end of the line follows it.
    pub(crate) fn escape_line_start(&self, raw: &str, escaped: Text, alone: bool) -> Text {
        if matches!(self.variant, Variant::Plain | Variant::Markua) {
            return escaped;
        }
        if raw.starts_with(":::") && self.has(Extension::FencedDivs) {
            return escaped;
        }
        if self.in_heading {
            return escaped;
        }
        if raw == "-" || raw == "+" {
            return format!("\\{raw}");
        }
        if alone && raw == "%" && self.has(Extension::PandocTitleBlock) {
            return format!("\\{escaped}");
        }
        if alone
            && let Some(label) = raw.strip_prefix("(@").and_then(|r| r.strip_suffix(')'))
            && label
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return format!("\\(@{}\\)", self.escape_text(label));
        }
        if raw
            .as_bytes()
            .get(..9)
            .is_some_and(|n| n.iter().all(u8::is_ascii_digit))
            && raw
                .as_bytes()
                .get(9)
                .is_some_and(|b| matches!(b, b'.' | b')'))
        {
            return match escaped.split_at_checked(9) {
                Some((digits, suffix)) => format!("{digits}\\{suffix}"),
                None => escaped,
            };
        }
        if alone && let Some(marker) = ordered_marker(raw) {
            return marker;
        }
        escaped
    }
}

/// The parts of the roman numeral grammar of the Markdown reader, in order:
/// the letters, the value and `true` when the part can repeat.
const ROMAN_PARTS: &[(&[u8], u64, bool)] = &[
    (b"M", 1000, true),
    (b"CM", 900, false),
    (b"D", 500, false),
    (b"CD", 400, false),
    (b"C", 100, true),
    (b"XC", 90, false),
    (b"L", 50, false),
    (b"XL", 40, false),
    (b"X", 10, true),
    (b"IX", 9, false),
    (b"V", 5, false),
    (b"IV", 4, false),
    (b"I", 1, true),
];

/// Returns the value of a roman numeral of one case, or `None` when the
/// reader does not accept the text as one (`ID`, `VV` or `Iv`).
fn roman_value(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    let one_case =
        bytes.iter().all(u8::is_ascii_uppercase) || bytes.iter().all(u8::is_ascii_lowercase);
    if bytes.is_empty() || !one_case {
        return None;
    }
    let mut rest = bytes;
    let mut value: u64 = 0;
    for &(letters, part, repeats) in ROMAN_PARTS {
        while let Some((head, after)) = rest.split_at_checked(letters.len())
            && head.eq_ignore_ascii_case(letters)
        {
            rest = after;
            value = value.saturating_add(part);
            if !repeats {
                break;
            }
        }
    }
    rest.is_empty().then_some(value)
}

/// Splits an ordered list marker into the opening parenthesis, the number
/// and the delimiter, or returns `None` when the text is not a marker.
///
/// The reader needs two spaces after an upper-case letter with a period,
/// and after an upper-case roman numeral with a period whose value is that
/// of one letter (`V.` or `IIIII.`). `loose` accepts these markers too.
fn marker_parts(text: &str, loose: bool) -> Option<(bool, &str, char)> {
    let (open, body) = match text.strip_prefix('(') {
        Some(body) => (true, body),
        None => (false, text),
    };
    let delim = body.chars().last()?;
    if delim != '.' && delim != ')' || open && delim != ')' {
        return None;
    }
    let number = &body[..body.len() - 1];
    let upper_period = delim == '.' && number.starts_with(|c: char| c.is_ascii_uppercase());
    let valid =
        (number.len() <= 9 && !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
            || number == "#"
            || (number.len() == 1 && number.starts_with(|c: char| c.is_ascii_lowercase()))
            || (number.len() == 1
                && number.starts_with(|c: char| c.is_ascii_uppercase())
                && (loose || !upper_period))
            || roman_value(number).is_some_and(|value| {
                loose || !upper_period || !matches!(value, 1 | 5 | 10 | 50 | 100 | 500 | 1000)
            });
    valid.then_some((open, number, delim))
}

/// Returns the escaped form of an ordered list marker, or `None` when the
/// text is not a marker.
pub(crate) fn ordered_marker(text: &str) -> Option<Text> {
    let (open, number, delim) = marker_parts(text, false)?;
    Some(if open {
        format!("\\({number}\\{delim}")
    } else {
        format!("{number}\\{delim}")
    })
}

/// Returns `true` when this escaped text starts a list item or a block
/// quote that can interrupt a paragraph.
pub(crate) fn is_interrupting_marker(text: &str, in_list: bool) -> bool {
    matches!(text, "-" | "+" | "*" | ">" | "1." | "1)")
        || in_list && marker_parts(text, true).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_markers_get_an_escaped_delimiter() {
        assert_eq!(ordered_marker("1.").as_deref(), Some("1\\."));
        assert_eq!(ordered_marker("A)").as_deref(), Some("A\\)"));
        assert_eq!(ordered_marker("(1)").as_deref(), Some("\\(1\\)"));
        assert_eq!(ordered_marker("iv.").as_deref(), Some("iv\\."));
        assert_eq!(ordered_marker("#.").as_deref(), Some("#\\."));
        assert_eq!(ordered_marker("12345678901."), None);
        assert_eq!(ordered_marker("1.x"), None);
        assert_eq!(ordered_marker("(1."), None);
        assert_eq!(ordered_marker("."), None);
    }

    #[test]
    fn roman_numerals_follow_the_grammar_of_the_reader() {
        // Values and refusals of the Pandoc 3.13 reader.
        for (text, value) in [
            ("IV", 4),
            ("IIII", 4),
            ("IIIII", 5),
            ("IVI", 5),
            ("VIV", 9),
            ("XCXX", 110),
            ("LXL", 90),
            ("CMD", 1400),
            ("DCD", 900),
            ("CDC", 500),
            ("MCMXCIX", 1999),
            ("MMMMM", 5000),
            ("xcxx", 110),
        ] {
            assert_eq!(roman_value(text), Some(value), "{text}");
        }
        for text in [
            "ID", "XM", "IC", "VV", "LL", "il", "Iv", "IVV", "XLL", "XCC", "DCM", "MDM", "IIX",
            "VX", "CMCM", "",
        ] {
            assert_eq!(roman_value(text), None, "{text}");
        }
    }

    #[test]
    fn an_upper_case_numeral_with_a_period_needs_a_value_of_two_letters() {
        assert_eq!(ordered_marker("IV.").as_deref(), Some("IV\\."));
        assert_eq!(ordered_marker("XCXX.").as_deref(), Some("XCXX\\."));
        assert_eq!(ordered_marker("IIIII)").as_deref(), Some("IIIII\\)"));
        assert_eq!(ordered_marker("iiiii.").as_deref(), Some("iiiii\\."));
        for text in [
            "IIIII.", "IVI.", "CDC.", "CMC.", "I.", "V.", "ID.", "VV.", "il.",
        ] {
            assert_eq!(ordered_marker(text), None, "{text}");
        }
        // In a list a break before these words would start an item.
        for text in ["IIIII.", "IVI.", "CDC.", "V.", "A.", "XCXX."] {
            assert!(is_interrupting_marker(text, true), "{text}");
        }
        for text in ["ID.", "XM.", "IC.", "VV.", "LL.", "il.", "ID)"] {
            assert!(!is_interrupting_marker(text, true), "{text}");
        }
    }

    #[test]
    fn entities_need_a_known_name_or_a_number() {
        assert!(starts_with_entity("amp; x"));
        assert!(starts_with_entity("#1;"));
        assert!(starts_with_entity("#x1F;"));
        assert!(!starts_with_entity("x;"));
        assert!(!starts_with_entity(";"));
        assert!(!starts_with_entity("#;"));
        assert!(!starts_with_entity("amp"));
    }
}
