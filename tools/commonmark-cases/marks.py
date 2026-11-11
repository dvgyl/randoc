#!/usr/bin/env python3
"""Writes crates/randoc-commonmark/src/marks.rs: the ranges of the Unicode
general categories Mn, Mc and Me, from the `unicodedata` module of Python.

Run: python3 tools/commonmark-cases/marks.py
"""

import unicodedata
from pathlib import Path

ranges = []
for cp in range(0x110000):
    if unicodedata.category(chr(cp)) in ("Mn", "Mc", "Me"):
        if ranges and ranges[-1][1] == cp - 1:
            ranges[-1][1] = cp
        else:
            ranges.append([cp, cp])
rows = "\n".join(f"    (0x{a:X}, 0x{b:X})," for a, b in ranges)
text = f"""//! The combining marks of Unicode {unicodedata.unidata_version} (general categories Mn, Mc and Me).
//! `tools/commonmark-cases/marks.py` writes this file.

/// Returns `true` for a combining mark, which the `commonmark` tokenizer
/// counts as a word character together with letters and numbers.
pub(crate) fn is_mark(c: char) -> bool {{
    let cp = c as u32;
    MARKS
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
{rows}
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
}}
"""
Path(__file__).resolve().parents[2].joinpath("crates/randoc-commonmark/src/marks.rs").write_text(text)
