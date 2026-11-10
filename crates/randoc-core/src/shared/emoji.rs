//! Emoji and their aliases, from the `emojis` package 0.1.5 (BSD-3-Clause),
//! with data from GitHub `gemoji` (MIT).

use super::tables::{ALIAS_BY_EMOJI, EMOJI_BY_ALIAS, EMOJI_STARTS};
use randoc_ast::Text;

/// The largest number of characters of an emoji in [`ALIAS_BY_EMOJI`].
const MAX_EMOJI_CHARS: usize = 8;

/// Returns the emoji of an alias, as the `emoji` extension uses it for `:alias:`.
///
/// ```
/// use randoc_core::shared::emoji_from_alias;
///
/// assert_eq!(emoji_from_alias("smile"), Some("\u{1F604}"));
/// assert_eq!(emoji_from_alias("nosuch"), None);
/// ```
pub fn emoji_from_alias(alias: &str) -> Option<&'static str> {
    lookup(&EMOJI_BY_ALIAS, alias)
}

/// Returns the first alias of an emoji.
///
/// ```
/// use randoc_core::shared::alias_from_emoji;
///
/// assert_eq!(alias_from_emoji("\u{1F606}"), Some("laughing"));
/// assert_eq!(alias_from_emoji("a"), None);
/// ```
pub fn alias_from_emoji(emoji: &str) -> Option<&'static str> {
    lookup(&ALIAS_BY_EMOJI, emoji)
}

/// Replaces each emoji in a text with its first alias.
///
/// At each position the longest emoji wins, so a flag or a sequence with a
/// joiner becomes one alias. Other characters stay.
///
/// ```
/// use randoc_core::shared::replace_emoji_with_aliases;
///
/// assert_eq!(replace_emoji_with_aliases("a \u{1F604}!"), "a smile!");
/// ```
pub fn replace_emoji_with_aliases(text: &str) -> Text {
    let mut out = Text::with_capacity(text.len());
    let mut rest = text;
    while let Some(first) = rest.chars().next() {
        match longest_emoji(rest) {
            Some((length, alias)) => {
                out.push_str(alias);
                rest = rest.get(length..).unwrap_or_default();
            }
            None => {
                out.push(first);
                rest = rest.get(first.len_utf8()..).unwrap_or_default();
            }
        }
    }
    out
}

/// Returns the byte length and the alias of the longest emoji at the start of `text`.
fn longest_emoji(text: &str) -> Option<(usize, &'static str)> {
    let first = text.chars().next()?;
    EMOJI_STARTS.binary_search(&first).ok()?;
    let mut ends = [0; MAX_EMOJI_CHARS];
    let mut count = 0;
    for (slot, (index, c)) in ends.iter_mut().zip(text.char_indices()) {
        *slot = index + c.len_utf8();
        count += 1;
    }
    ends.get(..count)?.iter().rev().find_map(|&end| {
        let candidate = text.get(..end)?;
        alias_from_emoji(candidate).map(|alias| (end, alias))
    })
}

#[cfg(test)]
thread_local! {
    static LOOKUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn lookup(table: &'static [(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    #[cfg(test)]
    LOOKUPS.set(LOOKUPS.get() + 1);
    let index = table.binary_search_by(|&(k, _)| k.cmp(key)).ok()?;
    table.get(index).map(|&(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_sorted_and_the_length_limit_holds() {
        assert!(EMOJI_BY_ALIAS.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(ALIAS_BY_EMOJI.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(EMOJI_STARTS.windows(2).all(|pair| pair[0] < pair[1]));
        let longest = ALIAS_BY_EMOJI
            .iter()
            .map(|(emoji, _)| emoji.chars().count())
            .max();
        assert_eq!(longest, Some(MAX_EMOJI_CHARS));
    }

    #[test]
    fn replacement_is_linear_in_the_length_of_the_text() {
        let size = 20000;
        let text = "a\u{301}\u{1F604}".repeat(size);
        LOOKUPS.set(0);
        let out = replace_emoji_with_aliases(&text);
        assert_eq!(out, "a\u{301}smile".repeat(size));
        // Only the emoji can start a match: one lookup for each other character.
        let chars = size;
        assert!(
            LOOKUPS.get() <= MAX_EMOJI_CHARS * chars,
            "{} lookups",
            LOOKUPS.get()
        );
    }

    // Pandoc 3.13, `-f gfm`: `# 🇫🇷 x` gives `fr-x`, and `# 👍🏽` keeps no alias for the skin tone.
    #[test]
    fn longest_emoji_wins() {
        assert_eq!(replace_emoji_with_aliases("\u{1F1EB}\u{1F1F7} x"), "fr x");
        assert_eq!(
            replace_emoji_with_aliases("\u{1F44D}\u{1F3FD}"),
            "+1\u{1F3FD}"
        );
        assert_eq!(replace_emoji_with_aliases(""), "");
    }
}
