use randoc_ast::MetaValue;
use randoc_core::shared::stringify;
use randoc_core::{Limits, Result};
use randoc_latex_reader::FieldOptions;

use crate::{Fields, parse_field, string};

/// Splits a list at separators outside protected groups.
pub(crate) fn split<'a>(text: &'a str, separator: &str) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut escaped = false;
    let mut start = 0;
    let mut end = 0;
    for (index, c) in text.char_indices() {
        if index < end {
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if depth == 0 && text[index..].starts_with(separator) {
            parts.push(text[start..index].trim());
            end = index + separator.len();
            start = end;
        } else {
            match c {
                '\\' => escaped = true,
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    parts.push(text[start..].trim());
    parts
}

pub(crate) fn split_and(text: &str) -> Vec<&str> {
    split(text, " and ")
}

pub(crate) fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn plain(text: &str, limits: &Limits) -> Result<Option<String>> {
    Ok(parse_field(text, FieldOptions::default(), limits)?.map(|parts| stringify(&parts)))
}

pub(crate) fn bib(text: &str, use_prefix: bool, limits: &Limits) -> Result<Option<MetaValue>> {
    let text = normalized(text);
    let mut names = Vec::new();
    for part in split_and(&text).into_iter().filter(|part| !part.is_empty()) {
        let Some(name) = bib_name(part, use_prefix, limits)? else {
            return Ok(None);
        };
        names.push(MetaValue::MetaMap(name));
    }
    Ok(Some(MetaValue::MetaList(names)))
}

fn bib_name(text: &str, use_prefix: bool, limits: &Limits) -> Result<Option<Fields>> {
    let mut name = Fields::new();
    if text == "others" || whole_group(text) {
        return Ok(plain(text, limits)?.map(|value| {
            string(&mut name, "literal", value);
            name
        }));
    }
    let parts = split(text, ",");
    if parts.first().is_some_and(|part| part.contains('=')) {
        let use_prefix = parts
            .iter()
            .find_map(|part| {
                let (key, value) = part.split_once('=')?;
                (key.trim() == "useprefix").then(|| value.trim().trim_matches(['{', '}']) == "true")
            })
            .unwrap_or(use_prefix);
        for part in parts {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            let key = match key.trim() {
                "family" => "family",
                "given" => "given",
                "suffix" => "suffix",
                "prefix" => particle_key(use_prefix),
                _ => continue,
            };
            let Some(value) = plain(value.trim(), limits)? else {
                return Ok(None);
            };
            string(&mut name, key, value);
        }
        return Ok(Some(name));
    }
    for (key, part) in name_parts(&parts, use_prefix) {
        if !part.is_empty() {
            let Some(mut value) = plain(&part, limits)? else {
                return Ok(None);
            };
            if key == "given" {
                value = space_initials(&value);
            }
            string(&mut name, key, value);
        }
    }
    Ok(Some(name))
}

fn name_parts(parts: &[&str], use_prefix: bool) -> [(&'static str, String); 4] {
    let (given, particle, family, suffix) = match parts {
        [only] => {
            let tokens = split(only, " ");
            let last = tokens.len() - 1;
            let start = tokens[..last]
                .iter()
                .position(|word| lowercase(word))
                .unwrap_or(last);
            let end = start
                + tokens[start..last]
                    .iter()
                    .take_while(|word| lowercase(word))
                    .count();
            (
                tokens[..start].join(" "),
                tokens[start..end].join(" "),
                tokens[end..].join(" "),
                "",
            )
        }
        [last, rest @ ..] => {
            let tokens = split(last, " ");
            let end = tokens[..tokens.len() - 1]
                .iter()
                .take_while(|word| lowercase(word))
                .count();
            let suffix = if let [suffix, _, ..] = rest {
                *suffix
            } else {
                ""
            };
            (
                rest.last().copied().unwrap_or("").to_owned(),
                tokens[..end].join(" "),
                tokens[end..].join(" "),
                suffix,
            )
        }
        [] => (String::new(), String::new(), String::new(), ""),
    };
    [
        ("given", given),
        (particle_key(use_prefix), particle),
        ("family", family),
        ("suffix", suffix.to_owned()),
    ]
}

fn lowercase(word: &str) -> bool {
    !whole_group(word)
        && (word.starts_with("{\\")
            || word
                .chars()
                .find(|c| c.is_alphabetic())
                .is_some_and(char::is_lowercase))
}

fn whole_group(text: &str) -> bool {
    let Some(body) = text.strip_prefix('{') else {
        return false;
    };
    let mut depth = 1;
    let mut escaped = false;
    for (index, c) in body.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return index + 1 == body.len();
                }
            }
            _ => {}
        }
    }
    false
}

fn particle_key(use_prefix: bool) -> &'static str {
    if use_prefix {
        "non-dropping-particle"
    } else {
        "dropping-particle"
    }
}

pub(crate) fn ris(text: &str) -> MetaValue {
    let mut name = Fields::new();
    let text = normalized(text);
    for (key, value) in name_parts(&split(&text, ","), false) {
        if !value.is_empty() {
            string(&mut name, key, value);
        }
    }
    MetaValue::MetaMap(name)
}

fn space_initials(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '.' && chars.peek().is_some_and(|c| c.is_uppercase()) {
            out.push(' ');
        }
    }
    out
}
