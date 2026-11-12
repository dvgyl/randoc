//! The template context of a document, with the rules of Pandoc 3.13.
//!
//! [`meta_to_context`] converts the metadata, adds the variables of the
//! writer options and sets `meta-json`. The writer then adds its own fields
//! only where the context has no value yet. The oracle (`pandoc lua`,
//! `pandoc.write` with a template) gave these rules:
//!
//! - A map, a list and a Boolean keep their kind. A string goes through the
//!   inline callback, split into words; inlines and blocks go through their
//!   callbacks.
//! - A variable replaces a metadata field of the same name.
//! - `meta-json` is the JSON text of the converted metadata, without the
//!   variables. It replaces a field or a variable of that name.
//!
//! [`normalize_date`] gives the `YYYY-MM-DD` form of a date for fields such
//! as `date-meta`.

use std::cell::RefCell;

use randoc_ast::{Block, Inline, Meta, MetaValue};
use randoc_layout::Doc;
use randoc_templates::{Context as TemplateContext, Val};

use crate::{Error, Result};

/// Makes the template context of a document from its metadata and the
/// variables of the writer options. `blocks` and `inlines` render content in
/// the output format of the writer. A metadata value deeper than `max_depth`
/// levels gives an error.
pub fn meta_to_context(
    meta: &Meta,
    variables: &TemplateContext,
    max_depth: usize,
    blocks: &mut dyn FnMut(&[Block]) -> Result<Doc>,
    inlines: &mut dyn FnMut(&[Inline]) -> Result<Doc>,
) -> Result<TemplateContext> {
    let mut converter = Converter {
        max_depth,
        blocks,
        inlines,
    };
    let mut context = TemplateContext::new();
    for (key, value) in &meta.0 {
        context.insert(key.clone(), converter.value(value, 0)?);
    }
    let mut json = String::new();
    write_map(&mut json, &context);
    for (key, value) in &variables.0 {
        context.insert(key.clone(), value.clone());
    }
    context.insert("meta-json", Val::text(json));
    Ok(context)
}

struct Converter<'a> {
    max_depth: usize,
    blocks: &'a mut dyn FnMut(&[Block]) -> Result<Doc>,
    inlines: &'a mut dyn FnMut(&[Inline]) -> Result<Doc>,
}

impl Converter<'_> {
    fn value(&mut self, value: &MetaValue, depth: usize) -> Result<Val> {
        if depth >= self.max_depth {
            return Err(Error::App("The metadata is nested too deeply.".into()));
        }
        Ok(match value {
            MetaValue::MetaMap(map) => {
                let mut context = TemplateContext::new();
                for (key, item) in map {
                    context.insert(key.clone(), self.value(item, depth + 1)?);
                }
                Val::Map(context)
            }
            MetaValue::MetaList(items) => Val::List(
                items
                    .iter()
                    .map(|item| self.value(item, depth + 1))
                    .collect::<Result<_>>()?,
            ),
            MetaValue::MetaBool(b) => Val::Bool(*b),
            MetaValue::MetaString(_) | MetaValue::MetaInlines(_) | MetaValue::MetaBlocks(_) => {
                // `Val::from_meta_value` splits a string into words as Pandoc
                // does; the callbacks keep the first error.
                let error = RefCell::new(None);
                let val = Val::from_meta_value(
                    value,
                    &mut |b| keep_error((self.blocks)(b), &error),
                    &mut |i| keep_error((self.inlines)(i), &error),
                );
                match error.into_inner() {
                    Some(e) => return Err(e),
                    None => val,
                }
            }
        })
    }
}

fn keep_error(result: Result<Doc>, error: &RefCell<Option<Error>>) -> Doc {
    result.unwrap_or_else(|e| {
        error.borrow_mut().get_or_insert(e);
        Doc::default()
    })
}

/// Writes a value as `aeson` does: text without line wrapping, keys in order.
fn write_value(out: &mut String, value: &Val) {
    match value {
        Val::Simple(doc) => write_string(out, &doc.render(None)),
        Val::List(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Val::Map(map) => write_map(out, map),
        Val::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Val::Null => out.push_str("null"),
    }
}

fn write_map(out: &mut String, map: &TemplateContext) {
    out.push('{');
    for (index, (key, item)) in map.0.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_string(out, key);
        out.push(':');
        write_value(out, item);
    }
    out.push('}');
}

/// Writes a JSON string. Only `"`, `\` and control characters get escapes.
fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// One part of a date format.
#[derive(Clone, Copy)]
enum Part {
    /// One or more white space characters.
    Space,
    Char(char),
    /// A year of one or more digits.
    Year,
    /// A year of two digits: 69 to 99 is 1969 to 1999, 00 to 68 is 2000 to 2068.
    ShortYear,
    /// A month of two digits.
    Month,
    /// A day of two digits.
    Day,
    /// A day of one or two digits, after optional spaces.
    PaddedDay,
    /// The name of a month, abbreviated (`Jan`) or full (`January`).
    MonthName {
        full: bool,
    },
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The formats in the order that Pandoc tries them. The oracle gave the
/// parts: for example `3/5/2021` and `5 Mar 2021` give no date, and
/// `05 Mar 2021` and `202103` do.
const DATE_FORMATS: &[&[Part]] = {
    use Part::*;
    &[
        &[Month, Char('/'), Day, Char('/'), ShortYear],
        &[Month, Char('/'), Day, Char('/'), Year],
        &[Year, Char('-'), Month, Char('-'), Day],
        &[Day, Space, MonthName { full: false }, Space, Year],
        &[PaddedDay, Space, MonthName { full: true }, Space, Year],
        &[
            MonthName { full: false },
            Char('.'),
            Space,
            PaddedDay,
            Char(','),
            Space,
            Year,
        ],
        &[
            MonthName { full: true },
            Space,
            PaddedDay,
            Char(','),
            Space,
            Year,
        ],
        &[Year, Month, Day],
        &[Year, Month],
        &[Year],
    ]
};

/// Returns a date as `YYYY-MM-DD`, or `None` when the text has none of the
/// date forms of Pandoc or the year is outside 1601 to 9999. White space
/// around the date is permitted, and month names ignore case.
pub fn normalize_date(text: &str) -> Option<String> {
    let chars: Vec<char> = text.trim().chars().collect();
    DATE_FORMATS.iter().find_map(|format| {
        let mut date = (None, None, None);
        parse_date(format, &chars, &mut date)
            .then_some(date)
            .and_then(|(year, month, day)| valid_date(year?, month.unwrap_or(1), day.unwrap_or(1)))
    })
}

type DateParts = (Option<i64>, Option<u32>, Option<u32>);

/// Matches `chars` against `format`, with backtracking for the parts of
/// variable length. Each format has at most seven parts, so the recursion
/// is shallow.
fn parse_date(format: &[Part], chars: &[char], date: &mut DateParts) -> bool {
    let Some((part, rest)) = format.split_first() else {
        return chars.is_empty();
    };
    let digits = chars.iter().take_while(|c| c.is_ascii_digit()).count();
    let number =
        |n: usize| -> Option<i64> { chars.get(..n)?.iter().collect::<String>().parse().ok() };
    match *part {
        Part::Space => {
            let n = chars.iter().take_while(|c| c.is_whitespace()).count();
            n > 0 && parse_date(rest, &chars[n..], date)
        }
        Part::Char(c) => chars.first() == Some(&c) && parse_date(rest, &chars[1..], date),
        Part::Year => (1..=digits.min(9)).rev().any(|n| {
            date.0 = number(n);
            parse_date(rest, &chars[n..], date)
        }),
        Part::ShortYear => {
            digits >= 2 && {
                let y = number(2).unwrap_or(0);
                date.0 = Some(if y >= 69 { 1900 + y } else { 2000 + y });
                parse_date(rest, &chars[2..], date)
            }
        }
        Part::Month => {
            digits >= 2 && {
                date.1 = number(2).and_then(|m| u32::try_from(m).ok());
                parse_date(rest, &chars[2..], date)
            }
        }
        Part::Day => {
            digits >= 2 && {
                date.2 = number(2).and_then(|d| u32::try_from(d).ok());
                parse_date(rest, &chars[2..], date)
            }
        }
        Part::PaddedDay => {
            let spaces = chars.iter().take_while(|c| **c == ' ').count();
            let tail = &chars[spaces..];
            let digits = tail.iter().take_while(|c| c.is_ascii_digit()).count();
            (1..=digits.min(2)).rev().any(|n| {
                date.2 = tail[..n].iter().collect::<String>().parse().ok();
                parse_date(rest, &tail[n..], date)
            })
        }
        Part::MonthName { full } => MONTHS.iter().zip(1..).any(|(name, month)| {
            let name: Vec<char> = if full {
                name.chars().collect()
            } else {
                name.chars().take(3).collect()
            };
            chars.len() >= name.len()
                && chars
                    .iter()
                    .zip(&name)
                    .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
                && {
                    date.1 = Some(month);
                    parse_date(rest, &chars[name.len()..], date)
                }
        }),
    }
}

fn valid_date(year: i64, month: u32, day: u32) -> Option<String> {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    ((1601..=9999).contains(&year) && (1..=days).contains(&day))
        .then(|| format!("{year:04}-{month:02}-{day:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use randoc_layout::literal;

    /// Pandoc 3.13 (`pandoc lua`, `$date-meta$` of `pandoc.write` with a
    /// template): each text and its result; an empty result is no date.
    const DATES: &[(&str, &str)] = &[
        ("202103", "2021-03-01"),
        ("20210305", "2021-03-05"),
        ("Mar. 5, 2021", "2021-03-05"),
        ("Mar 5, 2021", ""),
        ("march 5, 2021", "2021-03-05"),
        ("MARCH 5, 2021", "2021-03-05"),
        ("March 05, 2021", "2021-03-05"),
        ("March  5, 2021", "2021-03-05"),
        ("March. 5, 2021", ""),
        ("March 5,  2021", "2021-03-05"),
        ("March 5 , 2021", ""),
        ("5 Mar 2021", ""),
        ("05 Mar 2021", "2021-03-05"),
        ("05\tMar 2021", "2021-03-05"),
        ("05 MAR 2021", "2021-03-05"),
        ("5 March 2021", "2021-03-05"),
        ("05  March 2021", "2021-03-05"),
        ("5 May 2021", "2021-05-05"),
        ("03/05/21", "2021-03-05"),
        ("3/5/2021", ""),
        ("03/05/2021", "2021-03-05"),
        ("12/31/99", "1999-12-31"),
        ("12/31/68", "2068-12-31"),
        ("12/31/69", "1969-12-31"),
        ("12/31/00", "2000-12-31"),
        (" 2021-03-05 ", "2021-03-05"),
        ("2021-3-5", ""),
        ("1600-01-01", ""),
        ("1601-01-01", "1601-01-01"),
        ("9999-12-31", "9999-12-31"),
        ("10000-01-01", ""),
        ("0999", ""),
        ("1601", "1601-01-01"),
        ("02021", "2021-01-01"),
        ("+2021", ""),
        ("2021-02-29", ""),
        ("2020-02-29", "2020-02-29"),
        ("2021-13-01", ""),
        ("Sept. 5, 2021", ""),
        ("Sep. 5, 2021", "2021-09-05"),
        ("Mar.  05, 2021", "2021-03-05"),
        ("2021-03-05x", ""),
        ("2021031", ""),
        ("next week", ""),
        ("2021-03-05T10:00", ""),
    ];

    #[test]
    fn dates_follow_the_oracle() {
        for (text, expected) in DATES {
            let got = normalize_date(text).unwrap_or_default();
            assert_eq!(&got, expected, "{text:?}");
        }
    }

    fn render(meta: Meta, variables: TemplateContext) -> Result<TemplateContext> {
        meta_to_context(
            &meta,
            &variables,
            8,
            &mut |b| Ok(literal(format!("B{}", b.len()))),
            &mut |i| Ok(literal(format!("I{}", i.len()))),
        )
    }

    fn text(val: Option<&Val>) -> String {
        match val {
            Some(Val::Simple(doc)) => doc.render(None),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn each_value_kind_and_the_precedence_of_variables() {
        let mut meta = Meta::default();
        let s = |t: &str| MetaValue::MetaString(t.into());
        meta.0.insert("a".into(), s("x  y\nz"));
        meta.0.insert("b".into(), MetaValue::MetaBool(true));
        meta.0.insert(
            "c".into(),
            MetaValue::MetaList(vec![s("q\"\\\u{1}"), MetaValue::MetaBlocks(vec![])]),
        );
        meta.0.insert(
            "d".into(),
            MetaValue::MetaMap([("k".into(), s("v"))].into_iter().collect()),
        );
        meta.0.insert("meta-json".into(), s("m"));
        let mut variables = TemplateContext::new();
        variables.insert("b", "var");
        variables.insert("meta-json", "var");
        let context = render(meta, variables).unwrap();
        // "x  y\nz" is Str, Space, Str, SoftBreak, Str.
        assert_eq!(text(context.get("a")), "I5");
        assert_eq!(text(context.get("b")), "var");
        assert_eq!(
            text(context.get("meta-json")),
            r#"{"a":"I5","b":true,"c":["I1","B0"],"d":{"k":"I1"},"meta-json":"I1"}"#
        );
        let mut json = String::new();
        write_string(&mut json, "q\"\\\u{1}\n\t<\u{e9}");
        assert_eq!(json, "\"q\\\"\\\\\\u0001\\n\\t<\u{e9}\"");
        assert!(matches!(context.get("c"), Some(Val::List(items)) if items.len() == 2));
    }

    #[test]
    fn deep_metadata_is_an_error_and_a_callback_error_returns() {
        let mut value = MetaValue::MetaBool(true);
        for _ in 0..1_000 {
            value = MetaValue::MetaList(vec![value]);
        }
        let mut meta = Meta::default();
        meta.0.insert("deep".into(), value);
        assert!(render(meta, TemplateContext::new()).is_err());
        let mut meta = Meta::default();
        meta.0.insert("t".into(), MetaValue::MetaInlines(vec![]));
        let failed = meta_to_context(
            &meta,
            &TemplateContext::new(),
            8,
            &mut |_| Ok(Doc::default()),
            &mut |_| Err(Error::App("no".into())),
        );
        assert!(failed.is_err());
    }
}
