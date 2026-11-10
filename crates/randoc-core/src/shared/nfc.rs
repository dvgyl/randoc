//! Unicode Normalization Form C (NFC), with the tables of Unicode 18.0.0.
//!
//! A measurement with the oracle chose 18.0 for NFC; the identifier classes use
//! 15.1 (`I-SHARED-04`).
//!
//! The CommonMark reader of Pandoc 3.13 normalizes its input to NFC before it
//! parses. The algorithm is the one of Unicode Standard Annex #15: full
//! canonical decomposition, canonical ordering of each run of combining
//! marks, then canonical composition. Hangul syllables use the arithmetic
//! rule of the Unicode Standard (section 3.12).

use std::borrow::Cow;
use std::cmp::Ordering;

use super::nfc_tables::{COMBINING_CLASSES, COMPOSITIONS, DECOMPOSITIONS, UNSAFE};

const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;

fn in_ranges<T>(ranges: &[(u32, u32, T)], code: u32) -> Option<&T> {
    let index = ranges
        .binary_search_by(|(low, high, _)| {
            if *high < code {
                Ordering::Less
            } else if *low > code {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        })
        .ok()?;
    ranges.get(index).map(|(_, _, value)| value)
}

fn combining_class(c: char) -> u8 {
    in_ranges(&COMBINING_CLASSES, u32::from(c))
        .copied()
        .unwrap_or(0)
}

fn is_unsafe(c: char) -> bool {
    let code = u32::from(c);
    code >= 0x300
        && UNSAFE
            .binary_search_by(|&(low, high)| {
                if high < code {
                    Ordering::Less
                } else if low > code {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            })
            .is_ok()
}

/// Returns `true` when NFC does not change the text.
///
/// The check is fast: it looks only for characters that can change, and
/// text with none of them is in NFC. A text with such a character can still
/// be in NFC; then the function returns `false`.
///
/// ```
/// use randoc_core::shared::is_nfc_quick;
///
/// assert!(is_nfc_quick("caf\u{E9}"));
/// assert!(!is_nfc_quick("cafe\u{301}"));
/// ```
pub fn is_nfc_quick(text: &str) -> bool {
    !text.chars().any(is_unsafe)
}

/// Returns the text in Unicode Normalization Form C.
///
/// The function borrows the text when [`is_nfc_quick`] holds. Time is linear in
/// the length of the text, except the sort of each run of combining marks
/// (n log n in the length of the run).
///
/// ```
/// use randoc_core::shared::nfc;
///
/// assert_eq!(nfc("e\u{301}"), "\u{E9}");
/// assert_eq!(nfc("\u{212B}"), "\u{C5}");
/// assert_eq!(nfc("\u{1100}\u{1161}\u{11A8}"), "\u{AC01}");
/// assert_eq!(nfc("a\u{323}\u{302}"), "\u{1EAD}");
/// ```
pub fn nfc(text: &str) -> Cow<'_, str> {
    if is_nfc_quick(text) {
        return Cow::Borrowed(text);
    }
    let mut decomposed: Vec<(char, u8)> = Vec::with_capacity(text.len());
    for c in text.chars() {
        decompose(c, &mut decomposed);
    }
    order(&mut decomposed);
    Cow::Owned(compose(&decomposed).into_iter().collect())
}

fn decompose(c: char, out: &mut Vec<(char, u8)>) {
    let code = u32::from(c);
    if let Some(index) = code.checked_sub(S_BASE).filter(|&index| index < S_COUNT) {
        let jamo = [
            L_BASE + index / N_COUNT,
            V_BASE + (index % N_COUNT) / T_COUNT,
            T_BASE + index % T_COUNT,
        ];
        for code in jamo.into_iter().filter(|&code| code != T_BASE) {
            if let Some(c) = char::from_u32(code) {
                out.push((c, 0));
            }
        }
        return;
    }
    match DECOMPOSITIONS.binary_search_by_key(&c, |&(from, _)| from) {
        Ok(index) => {
            let parts = DECOMPOSITIONS.get(index).map_or("", |&(_, parts)| parts);
            out.extend(parts.chars().map(|part| (part, combining_class(part))));
        }
        Err(_) => out.push((c, combining_class(c))),
    }
}

/// Sorts each run of characters with a combining class that is not zero, by class.
fn order(chars: &mut [(char, u8)]) {
    for run in chars.split_mut(|&(_, class)| class == 0) {
        run.sort_by_key(|&(_, class)| class);
    }
}

fn compose_pair(first: char, second: char) -> Option<char> {
    let (a, b) = (u32::from(first), u32::from(second));
    if let (Some(l), Some(v)) = (a.checked_sub(L_BASE), b.checked_sub(V_BASE))
        && l < L_COUNT
        && v < V_COUNT
    {
        return char::from_u32(S_BASE + (l * V_COUNT + v) * T_COUNT);
    }
    if let (Some(s), Some(t)) = (a.checked_sub(S_BASE), b.checked_sub(T_BASE))
        && s < S_COUNT
        && s % T_COUNT == 0
        && t > 0
        && t < T_COUNT
    {
        return char::from_u32(a + t);
    }
    let index = COMPOSITIONS
        .binary_search_by(|&(x, y, _)| (x, y).cmp(&(first, second)))
        .ok()?;
    COMPOSITIONS.get(index).map(|&(_, _, composite)| composite)
}

fn compose(chars: &[(char, u8)]) -> Vec<char> {
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut starter: Option<usize> = None;
    let mut last_class = 0;
    for &(c, class) in chars {
        if let Some(position) = starter {
            let adjacent = position + 1 == out.len();
            if (adjacent || (last_class != 0 && last_class < class))
                && let Some(first) = out.get(position).copied()
                && let Some(composite) = compose_pair(first, c)
            {
                if let Some(slot) = out.get_mut(position) {
                    *slot = composite;
                }
                continue;
            }
        }
        if class == 0 {
            starter = Some(out.len());
        }
        last_class = class;
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_sorted_for_binary_search() {
        assert!(
            COMBINING_CLASSES
                .windows(2)
                .all(|pair| pair[0].1 < pair[1].0)
        );
        assert!(DECOMPOSITIONS.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(
            COMPOSITIONS
                .windows(2)
                .all(|pair| (pair[0].0, pair[0].1) < (pair[1].0, pair[1].1))
        );
        assert!(UNSAFE.windows(2).all(|pair| pair[0].1 < pair[1].0));
    }

    #[test]
    fn a_long_run_of_marks_ends_fast() {
        let text = format!("a{}", "\u{301}\u{323}".repeat(50_000));
        let out = nfc(&text);
        assert!(out.starts_with('\u{1EA1}'));
        assert_eq!(out.chars().count(), 100_000);
    }

    // `e\u{301}` makes each text take the full path.
    #[test]
    fn code_points_next_to_the_hangul_ranges_stay() {
        for text in [
            "\u{D7A4}e\u{301}",
            "\u{D7A4}\u{11A8}e\u{301}",
            "\u{1113}\u{1161}e\u{301}",
            "\u{1100}\u{1176}e\u{301}",
            "\u{AC00}\u{11C3}e\u{301}",
            "\u{AC00}\u{11A7}e\u{301}",
        ] {
            let expected = text.replace("e\u{301}", "\u{E9}");
            assert_eq!(nfc(text), expected.as_str(), "{text:?}");
        }
        assert_eq!(nfc("\u{1112}\u{1175}\u{11C2}e\u{301}"), "\u{D7A3}\u{E9}");
        assert_eq!(nfc("\u{AC00}\u{11A8}e\u{301}"), "\u{AC01}\u{E9}");
    }

    // Pandoc 3.13, `-f commonmark`: marks of Unicode 17.0 and 18.0.
    #[test]
    fn marks_of_unicode_18_are_ordered() {
        assert_eq!(nfc("e\u{5C8}\u{301}"), "\u{E9}\u{5C8}");
        assert_eq!(nfc("a\u{1AD3}\u{316}"), "a\u{316}\u{1AD3}");
        assert_eq!(nfc("\u{1ACF}\u{323}"), "\u{323}\u{1ACF}");
        assert_eq!(nfc("e\u{1ACF}\u{323}"), "\u{1EB9}\u{1ACF}");
        assert!(!is_nfc_quick("\u{1ACF}\u{1ADD}"));
    }

    #[test]
    fn normalized_text_is_borrowed() {
        assert!(matches!(nfc("plain \u{E9}t\u{E9}"), Cow::Borrowed(_)));
    }
}
