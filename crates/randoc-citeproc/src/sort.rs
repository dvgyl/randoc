//! Sort keys: a port of `SortKeyValue`, `normalizeSortKey`,
//! `compSortKeyValue(s)` and `dateToText` of `Citeproc.Eval` (`citeproc`
//! 0.14.1.1). Each word of a key keeps its collation key, so that a sort
//! computes each key once.

use std::cmp::Ordering;

use crate::collate::Collator;
use crate::reference::Date;
use crate::types::SortDirection;

/// The value of one sort key of an item (`SortKeyValue`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKeyValue {
    /// The direction of the key.
    pub direction: SortDirection,
    /// The collation keys of the words, or `None` when the item has no value.
    pub words: Option<Vec<Vec<u16>>>,
}

impl SortKeyValue {
    /// A key from the words of a value.
    pub fn new(collator: &Collator, direction: SortDirection, words: Option<Vec<String>>) -> Self {
        SortKeyValue {
            direction,
            words: words.map(|words| words.iter().map(|w| collator.sort_key(w)).collect()),
        }
    }
}

/// `normalizeSortKey`: the case-folded words, split at spaces, apostrophes,
/// commas, ayn and hamza.
pub fn normalize_sort_key(text: &str) -> Vec<String> {
    crate::case_fold::fold(text)
        .split(|c: char| {
            crate::haskell::is_space(c) || matches!(c, '\'' | '\u{2019}' | ',' | 'ʾ' | 'ʿ')
        })
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `dateToText`: `P` and the year for a year after zero, `N` and
/// 999999999 plus the year before, then the month and the day.
pub fn date_to_text(date: &Date) -> String {
    let year = |y: i64| {
        if y < 0 {
            format!("N{:09}", 999_999_999 + y)
        } else {
            format!("P{y:09}")
        }
    };
    let mut out = String::new();
    for parts in &date.parts {
        match parts.0.as_slice() {
            [] => {}
            [y] => out.push_str(&format!("{}0000", year(*y))),
            [y, m] => out.push_str(&format!("{}{m:02}00", year(*y))),
            [y, m, d, ..] => out.push_str(&format!("{}{m:02}{d:02}", year(*y))),
        }
    }
    out
}

/// `compSortKeyValue`: a missing value sorts after each value; keys with
/// different directions are equal.
pub fn compare_value(a: &SortKeyValue, b: &SortKeyValue) -> Ordering {
    match (&a.words, &b.words) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => match (a.direction, b.direction) {
            (SortDirection::Ascending, SortDirection::Ascending) => x.cmp(y),
            (SortDirection::Descending, SortDirection::Descending) => y.cmp(x),
            _ => Ordering::Equal,
        },
    }
}

/// `compSortKeyValues`: the keys in order; a shorter list sorts first.
pub fn compare_values(a: &[SortKeyValue], b: &[SortKeyValue]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        match compare_value(x, y) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::DateParts;

    #[test]
    fn keys_fold_case_and_split_words() {
        assert_eq!(
            normalize_sort_key("O’Brien, Ann  de'la"),
            ["o", "brien", "ann", "de", "la"]
        );
        assert_eq!(normalize_sort_key("STRASSE"), ["strasse"]);
        assert_eq!(normalize_sort_key("Straße"), ["strasse"]);
    }

    #[test]
    fn dates_before_zero_sort_first() {
        let date = |parts: Vec<i64>| Date {
            parts: vec![DateParts(parts)],
            ..Date::default()
        };
        assert_eq!(date_to_text(&date(vec![-44, 3, 15])), "N9999999550315");
        assert_eq!(date_to_text(&date(vec![2001])), "P0000020010000");
        assert_eq!(date_to_text(&date(vec![2001, 7])), "P0000020010700");
        assert!(date_to_text(&date(vec![-44])) < date_to_text(&date(vec![1])));
    }

    #[test]
    fn a_missing_value_sorts_last_in_both_directions() {
        let collator = Collator::for_lang(None);
        let value =
            |dir, w: Option<&str>| SortKeyValue::new(&collator, dir, w.map(|w| vec![w.to_owned()]));
        for dir in [SortDirection::Ascending, SortDirection::Descending] {
            assert_eq!(
                compare_value(&value(dir, None), &value(dir, Some("a"))),
                Ordering::Greater
            );
        }
        assert_eq!(
            compare_value(
                &value(SortDirection::Descending, Some("a")),
                &value(SortDirection::Descending, Some("b"))
            ),
            Ordering::Greater
        );
        assert_eq!(
            compare_value(
                &value(SortDirection::Ascending, Some("a")),
                &value(SortDirection::Descending, Some("b"))
            ),
            Ordering::Equal
        );
        let short = [value(SortDirection::Ascending, Some("a"))];
        let long = [short[0].clone(), value(SortDirection::Ascending, Some("b"))];
        assert_eq!(compare_values(&short, &long), Ordering::Less);
    }
}
