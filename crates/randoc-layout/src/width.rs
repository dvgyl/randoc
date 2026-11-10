// Ported from `doclayout` 0.6 (BSD-3-Clause), Copyright John MacFarlane.
// See third_party/licences/doclayout.txt.
//! Display width of text in a monospace font, as `doclayout` computes it.

use crate::tables::{EMOJI, RANGES};

/// The width class of a code point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    Narrow,
    Wide,
    Combining,
    Control,
    Zwj,
    EmojiPresentationMod,
    EmojiSkinToneMod,
}

/// The class of the code points in the ranges that `doclayout` checks
/// before its table. For a few code points the result differs from the
/// table, and Pandoc uses this result.
fn shortcut_class(c: char) -> Option<Class> {
    use Class::{Combining as M, Control as C, Narrow as N, Wide as W};
    let class = match c {
        '\0'..='\u{1F}' => C,
        '\u{20}'..='\u{7E}' => N,
        '\u{7F}'..='\u{9F}' => C,
        '\u{AD}' => C,
        '\u{A0}'..='\u{2FF}' => N,
        '\u{300}'..='\u{36F}' => M,
        '\u{3250}'..='\u{A4CF}' => W,
        '\u{600}'..='\u{6FF}' => match c {
            '\u{600}'..='\u{605}' => C,
            '\u{606}'..='\u{60F}' => N,
            '\u{610}'..='\u{61A}' => M,
            '\u{61B}' => N,
            '\u{61C}' => C,
            '\u{61D}'..='\u{64A}' => N,
            '\u{64B}'..='\u{65F}' => M,
            '\u{670}' => M,
            '\u{660}'..='\u{6D5}' => N,
            '\u{6D6}'..='\u{6DC}' => M,
            '\u{6DD}' => C,
            '\u{6DE}' => N,
            '\u{6DF}'..='\u{6E4}' => M,
            '\u{6E5}'..='\u{6E6}' => N,
            '\u{6E9}' => N,
            '\u{6E7}'..='\u{6ED}' => M,
            _ => N,
        },
        '\u{900}'..='\u{97F}' => match c {
            '\u{900}'..='\u{902}' => M,
            '\u{903}'..='\u{939}' => N,
            '\u{93A}' | '\u{93C}' => M,
            '\u{93B}'..='\u{940}' => N,
            '\u{941}'..='\u{948}' => M,
            '\u{94D}' => M,
            '\u{949}'..='\u{950}' => N,
            '\u{951}'..='\u{957}' => M,
            '\u{962}' | '\u{963}' => M,
            _ => N,
        },
        '\u{980}'..='\u{A02}' => match c {
            '\u{981}' | '\u{9BC}' => M,
            '\u{980}'..='\u{9C0}' => N,
            '\u{9C1}'..='\u{9C4}' => M,
            '\u{9CD}' => M,
            '\u{9C5}'..='\u{9E1}' => N,
            '\u{9E2}'..='\u{9E3}' => M,
            '\u{9E4}'..='\u{9FD}' => N,
            _ => M,
        },
        '\u{370}'..='\u{58F}' => match c {
            '\u{483}'..='\u{489}' => M,
            _ => N,
        },
        '\u{2E80}'..='\u{324F}' => match c {
            '\u{2E80}'..='\u{3029}' => W,
            '\u{302A}'..='\u{302D}' => M,
            '\u{303F}' => N,
            '\u{302E}'..='\u{3096}' => W,
            '\u{3097}'..='\u{309A}' => M,
            '\u{309B}'..='\u{3247}' => W,
            _ => N,
        },
        '\u{AC00}'..='\u{D7A3}' => W,
        '\u{C00}'..='\u{C80}' => match c {
            '\u{C00}' | '\u{C04}' => M,
            '\u{C01}'..='\u{C39}' => N,
            '\u{C3D}' => N,
            '\u{C3A}'..='\u{C40}' => M,
            '\u{C41}'..='\u{C44}' => N,
            '\u{C45}'..='\u{C56}' => M,
            '\u{C62}' | '\u{C63}' => M,
            _ => N,
        },
        '\u{B80}'..='\u{BFF}' => match c {
            '\u{B80}'..='\u{B82}' | '\u{BC0}' | '\u{BCD}' => M,
            _ => N,
        },
        '\u{E00}'..='\u{E7F}' => match c {
            '\u{E31}' | '\u{E34}'..='\u{E3A}' | '\u{E47}'..='\u{E4E}' => M,
            _ => N,
        },
        '\u{F900}'..='\u{FAFF}' | '\u{FF01}'..='\u{FF60}' | '\u{FFE0}'..='\u{FFE6}' => W,
        '\u{20000}'..='\u{3FFFF}' => W,
        _ => return None,
    };
    Some(class)
}

fn class_of(c: char) -> Class {
    shortcut_class(c).unwrap_or_else(|| table_class(c))
}

fn table_class(c: char) -> Class {
    let cp = u32::from(c);
    let index = RANGES.partition_point(|&(start, _)| start <= cp);
    match index.checked_sub(1).and_then(|i| RANGES.get(i)) {
        Some(&(_, class)) => class,
        None => Class::Control,
    }
}

/// Returns `(accepts a variation selector, accepts a skin tone modifier)`
/// when `c` starts an emoji.
fn emoji_info(c: char) -> Option<(bool, bool)> {
    let cp = u32::from(c);
    EMOJI
        .binary_search_by_key(&cp, |&(start, _, _)| start)
        .ok()
        .and_then(|i| EMOJI.get(i))
        .map(|&(_, variation, skin)| (variation, skin))
}

/// Returns `true` for an emoji skin tone modifier (U+1F3FB to U+1F3FF).
pub fn is_skin_tone_modifier(c: char) -> bool {
    ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}

/// Returns `true` for an emoji variation selector (U+FE0E or U+FE0F).
pub fn is_emoji_variation(c: char) -> bool {
    ('\u{FE0E}'..='\u{FE0F}').contains(&c)
}

/// Returns `true` for the zero-width joiner (U+200D).
pub fn is_zwj(c: char) -> bool {
    c == '\u{200D}'
}

/// The state of a width computation over a sequence of characters.
///
/// `total` excludes `tentative`. An emoji modifier can replace the
/// tentative width of the current character group.
#[derive(Debug, Clone, Copy)]
struct MatchState {
    first: bool,
    total: isize,
    last: char,
    tentative: isize,
}

impl MatchState {
    fn new(first: bool) -> Self {
        MatchState {
            first,
            total: 0,
            last: ' ',
            tentative: 0,
        }
    }

    fn length(self) -> isize {
        self.total + self.tentative
    }

    fn step(self, c: char) -> Self {
        self.step_with(c, class_of(c))
    }

    fn step_with(self, c: char, class: Class) -> Self {
        let settled = |width| MatchState {
            first: false,
            total: self.total + self.tentative,
            last: c,
            tentative: width,
        };
        let replace = |total| MatchState {
            first: false,
            total,
            last: c,
            tentative: 2,
        };
        match class {
            Class::Narrow => settled(1),
            Class::Wide => settled(2),
            Class::Combining => settled(if self.first { 1 } else { 0 }),
            Class::Control => settled(0),
            Class::Zwj => {
                if emoji_info(self.last).is_some()
                    || self.last == '\u{FE0F}'
                    || is_skin_tone_modifier(self.last)
                {
                    replace(self.total - 2)
                } else {
                    settled(0)
                }
            }
            Class::EmojiPresentationMod => match emoji_info(self.last) {
                Some((true, _)) => replace(self.total),
                _ => settled(0),
            },
            Class::EmojiSkinToneMod => match emoji_info(self.last) {
                Some((_, true)) => replace(self.total),
                _ => settled(2),
            },
        }
    }
}

pub(crate) fn chars_length(chars: impl IntoIterator<Item = char>) -> isize {
    chars
        .into_iter()
        .fold(MatchState::new(true), MatchState::step)
        .length()
}

/// Returns the width of a character in a monospace font.
///
/// A combining character has width 0, a regular character has width 1,
/// and an East Asian wide character has width 2. Control characters have
/// width 0. Ambiguous characters have width 1.
///
/// ```
/// assert_eq!(randoc_layout::char_width('a'), 1);
/// assert_eq!(randoc_layout::char_width('語'), 2);
/// assert_eq!(randoc_layout::char_width('\u{301}'), 0);
/// ```
pub fn char_width(c: char) -> usize {
    let width = MatchState::new(false).step(c).length();
    usize::try_from(width).unwrap_or(0)
}

/// Returns the width of a string in a monospace font.
///
/// The width includes combining characters, wide characters and emoji
/// sequences. A combining character at the start of the string has width 1.
/// Ambiguous characters have width 1.
///
/// ```
/// assert_eq!(randoc_layout::real_length("abc"), 3);
/// assert_eq!(randoc_layout::real_length("日本"), 4);
/// assert_eq!(randoc_layout::real_length("\u{1F44B}\u{1F3FD}"), 2);
/// ```
pub fn real_length(text: &str) -> usize {
    usize::try_from(chars_length(text.chars())).unwrap_or(0)
}

/// Returns the width of a string with ambiguous characters as width 1.
/// This is the same as [`real_length`].
pub fn real_length_narrow_context(text: &str) -> usize {
    real_length(text)
}

/// Returns the width of a string from the width table only. `doclayout`
/// exposes this form to test its quick path. For a few code points, for
/// example U+09C5, the result differs from [`real_length`], and Pandoc uses
/// [`real_length`].
pub fn real_length_narrow_context_no_shortcut(text: &str) -> usize {
    let state = text.chars().fold(MatchState::new(true), |state, c| {
        state.step_with(c, table_class(c))
    });
    usize::try_from(state.length()).unwrap_or(0)
}

/// The width of a string as the layout engine uses it. The value can be
/// negative for some malformed emoji sequences.
pub(crate) fn str_length(text: &str) -> isize {
    chars_length(text.chars())
}

/// Returns `true` for the characters that Haskell's `Data.Char.isSpace`
/// accepts.
pub(crate) fn is_space(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t'..='\r'
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
    )
}
