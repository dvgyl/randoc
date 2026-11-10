//! HTML character references: named entities and numeric references.
//!
//! The name table is the table of `commonmark` 0.3.1 (`Commonmark.Entity`,
//! BSD-3-Clause, from TagSoup). The Markdown and CommonMark readers of Pandoc
//! 3.13 decode references with this table.

use super::tables::ENTITIES;

/// Returns the text of a named character reference.
///
/// `name` is the text after `&`, with the final `;` when the reference has
/// one. The table has each name with `;`, and the legacy names (for example
/// `amp` and `copy`) also without `;`. The Markdown and CommonMark readers of
/// Pandoc accept only the names with `;`.
///
/// ```
/// use randoc_core::shared::named_entity;
///
/// assert_eq!(named_entity("amp;"), Some("&"));
/// assert_eq!(named_entity("copy"), Some("\u{A9}"));
/// assert_eq!(named_entity("ngeqq;"), Some("\u{2267}\u{338}"));
/// assert_eq!(named_entity("nosuch;"), None);
/// ```
pub fn named_entity(name: &str) -> Option<&'static str> {
    let index = ENTITIES.binary_search_by(|&(key, _)| key.cmp(name)).ok()?;
    ENTITIES.get(index).map(|&(_, text)| text)
}

/// Returns the character of a numeric character reference.
///
/// `digits` is the text between `&#` (or `&#x` when `hex` is true) and `;`.
/// It must have one or more digits of the radix, and any number of them.
/// Code point 0, a surrogate and a value above U+10FFFF give U+FFFD. A
/// CommonMark parser must also check the CommonMark limit on the number of
/// digits (7 decimal digits, 6 hexadecimal digits) before it calls this
/// function.
///
/// ```
/// use randoc_core::shared::numeric_entity;
///
/// assert_eq!(numeric_entity("65", false), Some('A'));
/// assert_eq!(numeric_entity("4e", true), Some('N'));
/// assert_eq!(numeric_entity("0", false), Some('\u{FFFD}'));
/// assert_eq!(numeric_entity("99999999999", false), Some('\u{FFFD}'));
/// assert_eq!(numeric_entity("", false), None);
/// assert_eq!(numeric_entity("x41", false), None);
/// ```
pub fn numeric_entity(digits: &str, hex: bool) -> Option<char> {
    let radix = if hex { 16 } else { 10 };
    if digits.is_empty() {
        return None;
    }
    let mut value: u32 = 0;
    for c in digits.chars() {
        let digit = c.to_digit(radix)?;
        value = value.saturating_mul(radix).saturating_add(digit);
    }
    Some(
        char::from_u32(value)
            .filter(|&c| c != '\0')
            .unwrap_or(char::REPLACEMENT_CHARACTER),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_for_binary_search() {
        assert!(ENTITIES.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert_eq!(ENTITIES.len(), 2231);
    }

    // Pandoc 3.13, `-f markdown`: `&#xD800;` and `&#1114112;` give U+FFFD.
    #[test]
    fn invalid_code_points_give_the_replacement_character() {
        assert_eq!(numeric_entity("D800", true), Some('\u{FFFD}'));
        assert_eq!(numeric_entity("1114112", false), Some('\u{FFFD}'));
        assert_eq!(numeric_entity("10FFFF", true), Some('\u{10FFFF}'));
        assert_eq!(numeric_entity("FG", true), None);
    }
}
