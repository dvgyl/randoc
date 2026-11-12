#!/usr/bin/env python3
"""Writes crates/randoc-commonmark/src/marks.rs: the ranges of the Unicode
general categories Mn, Mc and Me (combining marks), Ll (Haskell `isLower`)
and Lu with Lt (Haskell `isUpper`), from the `unicodedata` module of Python.

Run: python3 tools/commonmark-cases/marks.py
"""

import unicodedata
from pathlib import Path


def ranges_of(categories):
    ranges = []
    for cp in range(0x110000):
        if unicodedata.category(chr(cp)) in categories:
            if ranges and ranges[-1][1] == cp - 1:
                ranges[-1][1] = cp
            else:
                ranges.append([cp, cp])
    return "\n".join(f"    (0x{a:X}, 0x{b:X})," for a, b in ranges)


text = f"""//! Character classes of Unicode {unicodedata.unidata_version}: the combining marks (general
//! categories Mn, Mc and Me) and the letters of Haskell `isLower` (Ll) and `isUpper` (Lu, Lt).
//! `tools/commonmark-cases/marks.py` writes this file.

/// Returns `true` for a combining mark, which the `commonmark` tokenizer
/// counts as a word character together with letters and numbers.
pub(crate) fn is_mark(c: char) -> bool {{
    in_ranges(MARKS, c)
}}

/// Haskell `isLower`: the general category Ll.
pub(crate) fn is_lower(c: char) -> bool {{
    in_ranges(LOWER, c)
}}

/// Haskell `isUpper`: the general categories Lu and Lt.
pub(crate) fn is_upper(c: char) -> bool {{
    in_ranges(UPPER, c)
}}

fn in_ranges(ranges: &[(u32, u32)], c: char) -> bool {{
    let cp = c as u32;
    ranges
        .binary_search_by(|&(low, high)| {{
            if high < cp {{
                std::cmp::Ordering::Less
            }} else if low > cp {{
                std::cmp::Ordering::Greater
            }} else {{
                std::cmp::Ordering::Equal
            }}
        }})
        .is_ok()
}}

#[rustfmt::skip]
static MARKS: &[(u32, u32)] = &[
{ranges_of(("Mn", "Mc", "Me"))}
];

#[rustfmt::skip]
static LOWER: &[(u32, u32)] = &[
{ranges_of(("Ll",))}
];

#[rustfmt::skip]
static UPPER: &[(u32, u32)] = &[
{ranges_of(("Lu", "Lt"))}
];

#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn marks_of_several_scripts() {{
        assert!(is_mark('\\u{{301}}'));
        assert!(is_mark('\\u{{BCD}}'));
        assert!(is_mark('\\u{{20DD}}'));
        assert!(!is_mark('a'));
        assert!(!is_mark('\\u{{BA4}}'));
    }}

    #[test]
    fn letter_cases_follow_the_general_category() {{
        assert!(is_lower('é') && is_lower('α') && is_lower('ĸ'));
        assert!(!is_lower('ª') && !is_lower('ⓐ') && !is_lower('A'));
        assert!(is_upper('Ｂ') && is_upper('ǅ') && is_upper('Ä'));
        assert!(!is_upper('Ⅳ') && !is_upper('a'));
    }}
}}
"""
Path(__file__).resolve().parents[2].joinpath("crates/randoc-commonmark/src/marks.rs").write_text(text)
