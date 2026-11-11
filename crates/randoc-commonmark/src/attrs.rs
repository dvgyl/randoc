//! Attribute syntax of the `attributes` extension: `{#id .class key=value}`.

use randoc_ast::Attr;

use crate::inlines::is_word_char;
use crate::scan::unescape;

/// A list of attributes in source order. The key `id` sets the identifier
/// and the key `class` adds classes.
pub(crate) type Pairs = Vec<(String, String)>;

fn char_at(text: &str, pos: usize) -> Option<char> {
    text.get(pos..).and_then(|t| t.chars().next())
}

fn skip_ws(text: &str, mut pos: usize) -> usize {
    while matches!(char_at(text, pos), Some(' ' | '\t' | '\n' | '\r')) {
        pos += 1;
    }
    pos
}

fn take_while(text: &str, mut pos: usize, ok: impl Fn(char) -> bool) -> usize {
    while let Some(c) = char_at(text, pos) {
        if !ok(c) {
            break;
        }
        pos += c.len_utf8();
    }
    pos
}

fn one(text: &str, pos: usize) -> Option<(usize, (String, String))> {
    let decode = |s: &str| unescape(s.as_bytes());
    match char_at(text, pos)? {
        '#' => {
            let end = take_while(text, pos + 1, |c| {
                is_word_char(c) || matches!(c, '-' | '_' | ':' | '.')
            });
            if end > pos + 1 {
                Some((end, ("id".into(), decode(text.get(pos + 1..end)?))))
            } else {
                None
            }
        }
        '.' => {
            let end = take_while(text, pos + 1, |c| is_word_char(c) || matches!(c, '-' | '_'));
            if end > pos + 1 {
                Some((end, ("class".into(), decode(text.get(pos + 1..end)?))))
            } else {
                None
            }
        }
        c if c.is_ascii_alphabetic() || c == '_' || c == ':' => {
            let name_end = take_while(text, pos, |c| {
                c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-')
            });
            if char_at(text, name_end) != Some('=') {
                return None;
            }
            let name = text.get(pos..name_end)?.to_owned();
            let value_start = name_end + 1;
            if char_at(text, value_start) == Some('"') {
                let close = text.get(value_start + 1..)?.find('"')?;
                let end = value_start + 1 + close;
                let value = decode(text.get(value_start + 1..end)?);
                return Some((end + 1, (name, value)));
            }
            let end = take_while(text, value_start, |c| {
                !matches!(
                    c,
                    ' ' | '\t' | '\n' | '\r' | '<' | '>' | '=' | '`' | '\'' | '"' | '}'
                )
            });
            if end == value_start {
                return None;
            }
            Some((end, (name, decode(text.get(value_start..end)?))))
        }
        _ => None,
    }
}

fn group(text: &str, pos: usize) -> Option<(usize, Pairs)> {
    if char_at(text, pos)? != '{' {
        return None;
    }
    let mut p = skip_ws(text, pos + 1);
    let mut pairs = Vec::new();
    let (end, first) = one(text, p)?;
    pairs.push(first);
    p = end;
    loop {
        let q = skip_ws(text, p);
        if q == p {
            break;
        }
        match one(text, q) {
            Some((end, pair)) => {
                pairs.push(pair);
                p = end;
            }
            None => {
                p = q;
                break;
            }
        }
    }
    let p = skip_ws(text, p);
    (char_at(text, p)? == '}').then_some((p + 1, pairs))
}

/// Parses one or more attribute groups at `pos`. Returns the end and the attributes.
pub(crate) fn parse_attributes(text: &str, pos: usize) -> Option<(usize, Pairs)> {
    let (mut end, mut pairs) = group(text, pos)?;
    while let Some((next, more)) = group(text, end) {
        pairs.extend(more);
        end = next;
    }
    Some((end, pairs))
}

/// Adds attributes to an element as `commonmark-pandoc` does: the first
/// identifier wins, classes keep their order, and a key that is already present stays.
pub(crate) fn merge(pairs: Pairs, mut attr: Attr) -> Attr {
    use std::collections::HashSet;
    let mut classes: HashSet<String> = attr.classes.iter().cloned().collect();
    let mut keys: HashSet<String> = attr.attributes.iter().map(|(k, _)| k.clone()).collect();
    let mut new_classes: Vec<Vec<String>> = Vec::new();
    let mut new_pairs: Vec<(String, String)> = Vec::new();
    let mut id = None;
    // Pandoc applies the attributes from the last one to the first one.
    for (key, value) in pairs.into_iter().rev() {
        crate::steps::add(1);
        match key.as_str() {
            "id" => id = Some(value),
            "class" => {
                if !classes.contains(&value) {
                    let words: Vec<String> = value.split_whitespace().map(str::to_owned).collect();
                    classes.extend(words.iter().cloned());
                    new_classes.push(words);
                }
            }
            _ => {
                if keys.insert(key.clone()) {
                    new_pairs.push((key, value));
                }
            }
        }
    }
    if let Some(id) = id {
        attr.id = id;
    }
    let mut all: Vec<String> = new_classes.into_iter().rev().flatten().collect();
    all.append(&mut attr.classes);
    attr.classes = all;
    new_pairs.reverse();
    new_pairs.append(&mut attr.attributes);
    attr.attributes = new_pairs;
    attr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_and_merging() {
        let text = "{#a .b k=v x=\"y z\"}{.c} rest";
        let (end, pairs) = parse_attributes(text, 0).unwrap();
        assert_eq!(&text[end..], " rest");
        let attr = merge(pairs, Attr::default());
        assert_eq!(attr.id, "a");
        assert_eq!(attr.classes, vec!["b", "c"]);
        assert_eq!(
            attr.attributes,
            vec![("k".into(), "v".into()), ("x".into(), "y z".into())]
        );
        assert!(parse_attributes("{}", 0).is_none());
        assert!(parse_attributes("{#a", 0).is_none());
        assert!(parse_attributes("{ = }", 0).is_none());
    }
}
