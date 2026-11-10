//! Automatic identifiers of headings, and the sets that make them unique.
//!
//! The rules come from the oracle. Pandoc 3.13 uses the Unicode 15.1 tables
//! of GHC for the character classes, so a character that Unicode 16.0 or
//! later added is not a letter here.
//!
//! The Pandoc rules ([`auto_identifier`], [`IdentifierSet`]) apply to the
//! Markdown reader and to the other readers that Pandoc writes itself. The
//! CommonMark reader of Pandoc uses the `commonmark-extensions` package, which
//! has different rules ([`commonmark_auto_identifier`],
//! [`CommonmarkIdentifierSet`]).

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use randoc_ast::{Inline, Text};

use super::emoji::replace_emoji_with_aliases;
use super::tables::{CLASSES, COMMONMARK_ASCII, PANDOC_ASCII};
use super::text::{Purpose, identifier_text, stringify};
use crate::{Extension, Extensions};

/// The class of a character that the identifier rules use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Class {
    /// The general category letter (L).
    Letter,
    /// The general category number (N).
    Number,
    /// The general category mark (M).
    Mark,
    /// The general category connector punctuation (Pc).
    Connector,
    /// The general category space separator (Zs).
    Space,
}

fn class(c: char) -> Option<Class> {
    let code = u32::from(c);
    let index = CLASSES
        .binary_search_by(|&(low, high, _)| {
            if high < code {
                Ordering::Less
            } else if low > code {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        })
        .ok()?;
    CLASSES.get(index).map(|&(_, _, class)| class)
}

/// The `isSpace` function of GHC: the ASCII controls from tab to carriage return, and Zs.
fn is_space(c: char) -> bool {
    ('\t'..='\r').contains(&c) || class(c) == Some(Class::Space)
}

fn is_letter(c: char) -> bool {
    class(c) == Some(Class::Letter)
}

/// Lowercases each character alone, as `Data.Text.toLower` does: `Σ` always gives `σ`.
fn lowercase(text: &str) -> Text {
    text.chars().flat_map(char::to_lowercase).collect()
}

fn ascii_form(table: &[(char, char)], c: char) -> Option<char> {
    if c.is_ascii() {
        return Some(c);
    }
    let index = table.binary_search_by_key(&c, |&(from, _)| from).ok()?;
    table.get(index).map(|&(_, to)| to)
}

/// Returns the automatic identifier of a heading with the Pandoc rules.
///
/// The `auto_identifiers` rules: the text is lowercased; letters, numbers,
/// `_`, `-` and `.` stay; each run of spaces becomes one `-`; characters up to
/// the first letter go. With `gfm_auto_identifiers`: letters, numbers, marks
/// and connector punctuation stay; each space becomes one `-`; nothing goes
/// from the start. With `ascii_identifiers`: a character outside ASCII becomes
/// the first character of its canonical decomposition, or goes when that is
/// not ASCII.
///
/// With either of the two extensions, an emoji span gives the alias of its
/// source (the `data-emoji` attribute of a span with no identifier, the one
/// class `emoji` and no other attribute), and each other emoji character in
/// plain text (`Str`) becomes its first alias. Thus `:thumbsup:` gives
/// `thumbsup`, and the character 👍 gives `+1`. An emoji in code, in math or
/// in the alias value of an emoji span does not become an alias.
///
/// The result can be empty. [`IdentifierSet::unique`] then gives `section`.
///
/// ```
/// use randoc_ast::Inline;
/// use randoc_core::shared::auto_identifier;
/// use randoc_core::{Extension, Extensions};
///
/// let heading = [Inline::Str("3.".into()), Inline::Space, Inline::Str("Čas".into())];
/// assert_eq!(auto_identifier(&heading, Extensions::default()), "čas");
/// let ascii = Extensions::from_list(&[Extension::AsciiIdentifiers]);
/// assert_eq!(auto_identifier(&heading, ascii), "cas");
/// let gfm = Extensions::from_list(&[Extension::GfmAutoIdentifiers]);
/// assert_eq!(auto_identifier(&heading, gfm), "3-čas");
/// ```
pub fn auto_identifier(inlines: &[Inline], extensions: Extensions) -> Text {
    let gfm = extensions.contains(Extension::GfmAutoIdentifiers);
    let ascii = extensions.contains(Extension::AsciiIdentifiers);
    let text = if gfm || ascii {
        identifier_text(inlines, Purpose::PandocIdentifier)
    } else {
        stringify(inlines)
    };
    let lower = lowercase(&text);
    let fold = ascii.then_some(&PANDOC_ASCII[..]);
    if gfm {
        return gfm_identifier(&lower, fold);
    }
    let joined = lower
        .split(is_space)
        .map(|word| {
            word.chars()
                .filter(|&c| {
                    matches!(c, '_' | '-' | '.')
                        || matches!(class(c), Some(Class::Letter | Class::Number))
                })
                .collect::<Text>()
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let mut out: Text = match fold {
        Some(table) => joined
            .chars()
            .filter_map(|c| ascii_form(table, c))
            .collect(),
        None => joined,
    };
    match out.find(is_letter) {
        Some(start) => out.split_off(start),
        None => Text::new(),
    }
}

fn gfm_identifier(lower: &str, fold: Option<&[(char, char)]>) -> Text {
    let mut out = Text::with_capacity(lower.len());
    for c in lower.chars() {
        if c == '-' || c == '_' {
            out.push(c);
        } else if is_space(c) {
            out.push('-');
        } else if class(c).is_some_and(|class| class != Class::Space)
            && let Some(c) = fold.map_or(Some(c), |table| ascii_form(table, c))
        {
            out.push(c);
        }
    }
    out
}

/// Returns the automatic identifier of a heading with the rules of the
/// CommonMark reader (`gfm_auto_identifiers` of `commonmark-extensions`).
///
/// The rules are the `gfm_auto_identifiers` rules of [`auto_identifier`],
/// also for emoji spans, with two differences. An emoji character becomes
/// its first alias in each inline that carries text, also in code and in
/// math. And `ascii_identifiers` uses the smaller table of
/// `commonmark-extensions` 0.2.7.3: for example `ǖ` goes, and the Pandoc
/// rules give `u`.
///
/// This function does not normalize the text. The CommonMark reader of Pandoc
/// normalizes its whole input to NFC before it parses, so the text of its
/// inlines is already in NFC. The Markdown reader does not normalize.
///
/// ```
/// use randoc_ast::Inline;
/// use randoc_core::shared::commonmark_auto_identifier;
/// use randoc_core::{Extension, Extensions};
///
/// let ascii = Extensions::from_list(&[Extension::AsciiIdentifiers]);
/// let heading = [Inline::Str("Ǖber é \u{1F604}".into())];
/// assert_eq!(commonmark_auto_identifier(&heading, ascii), "ber-e-smile");
/// ```
pub fn commonmark_auto_identifier(inlines: &[Inline], extensions: Extensions) -> Text {
    let ascii = extensions.contains(Extension::AsciiIdentifiers);
    let text = identifier_text(inlines, Purpose::CommonmarkIdentifier);
    let lower = lowercase(&replace_emoji_with_aliases(&text));
    gfm_identifier(&lower, ascii.then_some(&COMMONMARK_ASCII[..]))
}

/// The identifiers of one document with the Pandoc rules.
///
/// ```
/// use randoc_core::shared::IdentifierSet;
///
/// let mut ids = IdentifierSet::new();
/// ids.register("s-1");
/// assert_eq!(ids.unique("s"), "s");
/// assert_eq!(ids.unique("s"), "s-2");
/// assert_eq!(ids.unique(""), "section");
/// ```
#[derive(Debug, Clone, Default)]
pub struct IdentifierSet {
    used: HashSet<Text>,
    next_suffix: HashMap<Text, usize>,
    #[cfg(test)]
    probes: usize,
}

impl IdentifierSet {
    /// Makes an empty set.
    ///
    /// ```
    /// assert_eq!(randoc_core::shared::IdentifierSet::new().unique("a"), "a");
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an explicit identifier, in document order. Returns `false` when
    /// the document already uses the identifier.
    ///
    /// The Markdown reader of Pandoc then warns, for example
    /// `Duplicate identifier 'x' at line 7 column 1`.
    ///
    /// ```
    /// let mut ids = randoc_core::shared::IdentifierSet::new();
    /// assert!(ids.register("a"));
    /// assert_eq!(ids.unique("a"), "a-1");
    /// assert!(!ids.register("a-1"));
    /// ```
    pub fn register(&mut self, id: &str) -> bool {
        self.used.insert(id.to_owned())
    }

    /// Returns a unique identifier for `base` and records it.
    ///
    /// An empty `base` becomes `section`. When `base` is in use, the result is
    /// the first of `base-1`, `base-2` and so on that is not in use.
    ///
    /// ```
    /// let mut ids = randoc_core::shared::IdentifierSet::new();
    /// assert_eq!(ids.unique("x"), "x");
    /// assert_eq!(ids.unique("x"), "x-1");
    /// assert_eq!(ids.unique("x-1"), "x-1-1");
    /// ```
    pub fn unique(&mut self, base: &str) -> Text {
        let base = if base.is_empty() { "section" } else { base };
        if self.used.insert(base.to_owned()) {
            return base.to_owned();
        }
        let next = self.next_suffix.entry(base.to_owned()).or_insert(1);
        loop {
            #[cfg(test)]
            {
                self.probes += 1;
            }
            let candidate = format!("{base}-{next}");
            *next += 1;
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
        }
    }
}

/// The identifiers of one document with the rules of the CommonMark reader.
///
/// The CommonMark reader of Pandoc counts all explicit heading identifiers of
/// the document before it makes the first automatic one, so a later explicit
/// heading identifier changes an earlier automatic one. Identifiers of other
/// elements (spans, links, code, divs) do not count. Thus the set starts from
/// all explicit heading identifiers ([`CommonmarkIdentifierSet::with_explicit`]).
///
/// Each base has a count. The first use of a base gives the base, and each
/// next use gives `base-1`, `base-2` and so on. An explicit identifier sets
/// the count of its text to zero. The result can be equal to an identifier
/// that is already in the document, and an empty base stays empty.
///
/// ```
/// use randoc_core::shared::CommonmarkIdentifierSet;
///
/// // Pandoc 3.13, `-f commonmark_x`: `# x`, `# x`, `# y {#x}`, `# z {#x-1}`, `# x`
/// // gives `x-1`, `x-2`, `x`, `x-1`, `x-3`.
/// let mut ids = CommonmarkIdentifierSet::with_explicit(["x", "x-1"]);
/// assert_eq!(ids.unique("x"), "x-1");
/// assert_eq!(ids.unique("x"), "x-2");
/// assert_eq!(ids.unique("x"), "x-3");
/// assert_eq!(ids.unique(""), "");
/// assert_eq!(ids.unique(""), "-1");
/// ```
#[derive(Debug, Clone, Default)]
pub struct CommonmarkIdentifierSet {
    counts: HashMap<Text, usize>,
}

impl CommonmarkIdentifierSet {
    /// Makes the set from all explicit heading identifiers of the document.
    ///
    /// Do not give identifiers of other elements: in `# x` followed by `[y]{#x}`,
    /// the heading gets `x`.
    ///
    /// ```
    /// let mut ids = randoc_core::shared::CommonmarkIdentifierSet::with_explicit(["a"]);
    /// assert_eq!(ids.unique("a"), "a-1");
    /// ```
    pub fn with_explicit<'a>(ids: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            counts: ids.into_iter().map(|id| (id.to_owned(), 0)).collect(),
        }
    }

    /// Returns the identifier for `base` and counts the use.
    ///
    /// ```
    /// let mut ids = randoc_core::shared::CommonmarkIdentifierSet::default();
    /// assert_eq!(ids.unique("a"), "a");
    /// assert_eq!(ids.unique("a"), "a-1");
    /// ```
    pub fn unique(&mut self, base: &str) -> Text {
        match self.counts.get_mut(base) {
            Some(count) => {
                *count += 1;
                format!("{base}-{count}")
            }
            None => {
                self.counts.insert(base.to_owned(), 0);
                base.to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_table_is_sorted_for_binary_search() {
        assert!(CLASSES.iter().all(|&(low, high, _)| low <= high));
        assert!(CLASSES.windows(2).all(|pair| pair[0].1 < pair[1].0));
        assert!(PANDOC_ASCII.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(
            COMMONMARK_ASCII
                .windows(2)
                .all(|pair| pair[0].0 < pair[1].0)
        );
    }

    // Pandoc 3.13, `-f markdown`: U+1C89 is new in Unicode 16.0 and goes; U+0131 gives `i`.
    #[test]
    fn classes_follow_unicode_15_1() {
        let plain = Extensions::default();
        let ascii = Extensions::from_list(&[Extension::AsciiIdentifiers]);
        let heading = |text: &str| vec![Inline::Str(text.into())];
        assert_eq!(auto_identifier(&heading("a\u{1C89}b"), plain), "ab");
        assert_eq!(auto_identifier(&heading("\u{131}"), ascii), "i");
        assert_eq!(auto_identifier(&heading("\u{2003}x"), plain), "x");
        assert_eq!(auto_identifier(&heading("a\u{2003}b"), plain), "a-b");
    }

    // A search for a free suffix that starts at `-1` each time makes about size * size / 2 probes.
    #[test]
    fn suffix_search_is_linear_in_the_number_of_identifiers() {
        let size = 4000;
        let mut ids = IdentifierSet::new();
        for n in 1..size {
            ids.register(&format!("x-{n}"));
        }
        for _ in 0..size {
            ids.unique("");
            ids.unique("x");
        }
        assert!(
            (size..=3 * size).contains(&ids.probes),
            "{} probes",
            ids.probes
        );
    }

    #[test]
    fn commonmark_counts_ignore_used_identifiers() {
        let mut ids = CommonmarkIdentifierSet::with_explicit(["a", "a"]);
        assert_eq!(ids.unique("a"), "a-1");
        assert_eq!(ids.unique("a-1"), "a-1");
    }

    fn emoji_span(alias: &str, emoji: &str) -> Inline {
        Inline::Span(
            randoc_ast::Attr {
                id: String::new(),
                classes: vec!["emoji".into()],
                attributes: vec![("data-emoji".into(), alias.into())],
            },
            vec![Inline::Str(emoji.into())],
        )
    }

    // Pandoc 3.13, `-f gfm`: `# :satisfied:` gives `satisfied`, `# :thumbsup: z` gives
    // `thumbsup-z`, and `# 👍 r` gives `1-r`. `-f markdown+emoji` gives `z`.
    #[test]
    fn an_emoji_span_gives_the_alias_of_the_source() {
        let gfm = Extensions::from_list(&[Extension::GfmAutoIdentifiers]);
        let ascii = Extensions::from_list(&[Extension::AsciiIdentifiers]);
        let thumbs = [
            emoji_span("thumbsup", "\u{1F44D}"),
            Inline::Space,
            Inline::Str("z".into()),
        ];
        assert_eq!(
            auto_identifier(&[emoji_span("satisfied", "\u{1F606}")], gfm),
            "satisfied"
        );
        assert_eq!(auto_identifier(&thumbs, gfm), "thumbsup-z");
        assert_eq!(auto_identifier(&thumbs, ascii), "thumbsup-z");
        assert_eq!(auto_identifier(&thumbs, Extensions::default()), "z");
        assert_eq!(commonmark_auto_identifier(&thumbs, gfm), "thumbsup-z");
        let raw = [
            Inline::Str("\u{1F44D}".into()),
            Inline::Space,
            Inline::Str("r".into()),
        ];
        assert_eq!(commonmark_auto_identifier(&raw, gfm), "1-r");
    }

    // Pandoc 3.13: "# Use `👍` $👍$ r👍" and "# [x]{.emoji data-emoji=😆} f".
    #[test]
    fn the_pandoc_rules_change_emoji_only_in_str() {
        let gfm = Extensions::from_list(&[Extension::GfmAutoIdentifiers]);
        let ascii = Extensions::from_list(&[Extension::AsciiIdentifiers]);
        let heading = [
            Inline::Str("Use".into()),
            Inline::Space,
            Inline::Code(randoc_ast::Attr::default(), "\u{1F44D}".into()),
            Inline::Space,
            Inline::Math(randoc_ast::MathType::InlineMath, "\u{1F44D}".into()),
            Inline::Space,
            Inline::Str("r\u{1F44D}".into()),
        ];
        assert_eq!(auto_identifier(&heading, gfm), "use---r1"); // -f markdown+gfm_auto_identifiers
        assert_eq!(auto_identifier(&heading, ascii), "use-r1"); // -f markdown+ascii_identifiers
        assert_eq!(commonmark_auto_identifier(&heading, gfm), "use-1-1-r1"); // -f commonmark_x
        let span = [
            emoji_span("\u{1F606}", "x"),
            Inline::Space,
            Inline::Str("f".into()),
        ];
        assert_eq!(auto_identifier(&span, gfm), "-f");
    }

    // Pandoc 3.13, `-f markdown`: `# x`, `# x`, `# y {#x}` warns `Duplicate identifier 'x'`.
    #[test]
    fn register_reports_a_duplicate() {
        let mut ids = IdentifierSet::new();
        ids.unique("x");
        assert!(!ids.register("x"));
        assert!(ids.register("y"));
    }
}
