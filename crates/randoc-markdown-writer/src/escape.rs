//! Backslash escapes for text in Markdown output.

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

impl W<'_> {
    /// Escapes the text of a `Str` element.
    pub(crate) fn escape_text(&self, text: &str) -> Text {
        if self.plain() {
            return text.to_owned();
        }
        let commonmark = self.variant == Variant::Commonmark;
        let escapable = commonmark || self.has(Extension::AllSymbolsEscapable);
        let escapable_all = self.has(Extension::AllSymbolsEscapable);
        let smart = self.has(Extension::Smart);
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
                '\\' if !escapable_all && next.is_some_and(|n| !n.is_ascii_punctuation()) => {
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
                '$' if self.has(Extension::TexMathDollars) || self.has(Extension::TexMathGfm) => {
                    out.push_str("\\$");
                }
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
                '\u{2019}' | '\u{2018}' if smart => out.push('\''),
                '\u{201C}' | '\u{201D}' if smart => out.push('"'),
                '\u{2014}' if smart => out.push_str("---"),
                '\u{2013}' if smart => out.push_str("--"),
                '\u{2026}' if smart => out.push_str("..."),
                _ => out.push(c),
            }
            i += 1;
        }
        out
    }

    /// Escapes a list marker at the start of a line.
    ///
    /// `text` is the escaped text of the first `Str` of a line. `alone` is
    /// `true` when a space or the end of the line follows it.
    pub(crate) fn escape_line_start(&self, raw: &str, escaped: Text, alone: bool) -> Text {
        if self.plain() {
            return escaped;
        }
        if raw.starts_with(":::") && self.has(Extension::FencedDivs) {
            return format!("\\{escaped}");
        }
        if self.in_heading {
            return escaped;
        }
        if alone && (raw == "-" || raw == "+") {
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
        if alone && let Some(marker) = ordered_marker(raw) {
            return marker;
        }
        escaped
    }
}

/// Returns `true` when `text` is a roman numeral of one case.
fn is_roman(text: &str) -> bool {
    !text.is_empty()
        && (text.chars().all(|c| "ivxlcdm".contains(c))
            || text.chars().all(|c| "IVXLCDM".contains(c)))
}

/// Returns the escaped form of an ordered list marker, or `None` when the
/// text is not a marker.
pub(crate) fn ordered_marker(text: &str) -> Option<Text> {
    let (open, body) = match text.strip_prefix('(') {
        Some(body) => (true, body),
        None => (false, text),
    };
    let delim = body.chars().last()?;
    if delim != '.' && delim != ')' {
        return None;
    }
    if open && delim != ')' {
        return None;
    }
    let number = &body[..body.len() - 1];
    let valid =
        (number.len() <= 9 && !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
            || (number.chars().count() == 1
                && number
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_uppercase() && delim == ')'))
            || number == "#"
            || is_roman(number);
    if !valid {
        return None;
    }
    Some(if open {
        format!("\\({number}\\{delim}")
    } else {
        format!("{number}\\{delim}")
    })
}

/// Returns `true` when this escaped text starts a list item or a block
/// quote that can interrupt a paragraph.
pub(crate) fn is_interrupting_marker(text: &str) -> bool {
    matches!(text, "-" | "+" | "*" | ">" | "1." | "1)")
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
