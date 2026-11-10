//! Replays the oracle results in `shared_oracle.json` against `randoc_core::shared`.
//!
//! `tools/shared_oracle.py` writes the data file. Each test reports the
//! number of differences and the first examples.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use randoc_ast::{Block, Pandoc, json};
use randoc_core::shared::{
    CommonmarkIdentifierSet, IdentifierSet, auto_identifier, commonmark_auto_identifier,
    default_abbreviations, emoji_from_alias, is_nfc_quick, named_entity, nfc, numeric_entity,
    stringify, stringify_blocks, stringify_meta,
};
use randoc_core::{Extensions, FormatSpec};

type Data = (
    Vec<(String, Pandoc, Vec<Option<String>>)>,
    (Pandoc, Vec<String>, BTreeMap<String, String>),
    Vec<(String, String)>,
    Vec<(String, Option<String>)>,
);

fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(load)
}

/// Reads the data file. The JSON reader of `randoc-ast` reads only the types
/// of the document model, so a general JSON parser reads the lists, and
/// `randoc-ast` reads each document in them.
#[allow(clippy::expect_used)]
fn load() -> Data {
    use serde_json::Value;
    let value: Value = serde_json::from_str(include_str!("shared_oracle.json"))
        .expect("shared_oracle.json is valid");
    let list = |v: &Value| v.as_array().expect("a list").clone();
    let text = |v: &Value| v.as_str().expect("a string").to_owned();
    let doc = |v: &Value| json::from_json(&v.to_string()).expect("a document");
    let headings = list(&value[0])
        .iter()
        .map(|row| {
            let explicit = list(&row[2])
                .iter()
                .map(|id| id.as_str().map(str::to_owned))
                .collect();
            (text(&row[0]), doc(&row[1]), explicit)
        })
        .collect();
    let plain = (
        doc(&value[1][0]),
        list(&value[1][1]).iter().map(text).collect(),
        value[1][2]
            .as_object()
            .expect("a map")
            .iter()
            .map(|(key, v)| (key.clone(), text(v)))
            .collect(),
    );
    let entities = list(&value[2])
        .iter()
        .map(|row| (text(&row[0]), text(&row[1])))
        .collect();
    let emoji = list(&value[3])
        .iter()
        .map(|row| (text(&row[0]), row[1].as_str().map(str::to_owned)))
        .collect();
    (headings, plain, entities, emoji)
}

fn check(name: &str, total: usize, differences: &[String]) {
    assert!(total > 0, "{name}: no cases");
    let examples: Vec<&String> = differences.iter().take(20).collect();
    assert!(
        differences.is_empty(),
        "{name}: {} of {total} differ:\n{examples:#?}",
        differences.len()
    );
}

/// Gives each heading its identifier: the explicit one, or an automatic one.
///
/// The Pandoc rules take explicit identifiers in document order. The
/// CommonMark rules take all explicit identifiers first.
fn replay_identifiers(
    mode: &str,
    extensions: Extensions,
    doc: &Pandoc,
    explicit: &[Option<String>],
) -> Vec<String> {
    let headings: Vec<_> = doc
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Header(_, _, inlines) => Some(inlines),
            _ => None,
        })
        .collect();
    assert_eq!(headings.len(), explicit.len(), "{mode}");
    if mode.starts_with("markdown") {
        let mut ids = IdentifierSet::new();
        headings
            .iter()
            .zip(explicit)
            .map(|(inlines, id)| match id {
                Some(id) => {
                    ids.register(id);
                    id.clone()
                }
                None => ids.unique(&auto_identifier(inlines, extensions)),
            })
            .collect()
    } else {
        let mut ids =
            CommonmarkIdentifierSet::with_explicit(explicit.iter().flatten().map(String::as_str));
        headings
            .iter()
            .zip(explicit)
            .map(|(inlines, id)| match id {
                Some(id) => id.clone(),
                None => ids.unique(&commonmark_auto_identifier(inlines, extensions)),
            })
            .collect()
    }
}

#[test]
fn identifiers_match_the_oracle() {
    for (mode, doc, explicit) in &data().0 {
        let extensions = FormatSpec::parse(mode)
            .extensions()
            .unwrap_or_else(|error| panic!("{mode}: {error}"));
        let actual = replay_identifiers(mode, extensions, doc, explicit);
        let mut differences = Vec::new();
        let mut index = 0;
        for block in &doc.blocks {
            let Block::Header(_, attr, inlines) = block else {
                continue;
            };
            let id = actual.get(index).cloned().unwrap_or_default();
            index += 1;
            if id != attr.id {
                differences.push(format!(
                    "{:?}: oracle {:?}, randoc {id:?}",
                    stringify(inlines),
                    attr.id
                ));
            }
        }
        assert!(index >= 3000, "{mode}: only {index} headings");
        check(mode, index, &differences);
    }
}

#[test]
fn plain_text_matches_the_oracle() {
    let (doc, blocks, meta) = &data().1;
    assert_eq!(doc.blocks.len(), blocks.len());
    let mut differences = Vec::new();
    for (block, expected) in doc.blocks.iter().zip(blocks) {
        let actual = match block {
            Block::Para(inlines) => stringify(inlines),
            _ => stringify_blocks(std::slice::from_ref(block)),
        };
        if &actual != expected {
            differences.push(format!("{block:?}: oracle {expected:?}, randoc {actual:?}"));
        }
    }
    for (key, value) in &doc.meta.0 {
        let actual = stringify_meta(value);
        if meta.get(key) != Some(&actual) {
            differences.push(format!(
                "{key}: oracle {:?}, randoc {actual:?}",
                meta.get(key)
            ));
        }
    }
    check("stringify", blocks.len() + meta.len(), &differences);
}

/// Decodes the references in a text with the rules of the Markdown reader of Pandoc.
fn decode_markdown_references(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let (decoded, length) = reference(rest).unwrap_or_else(|| ("&".to_owned(), 1));
        out.push_str(&decoded);
        rest = &rest[length..];
    }
    out.push_str(rest);
    out
}

fn reference(text: &str) -> Option<(String, usize)> {
    let body = text.strip_prefix('&')?;
    if let Some(number) = body.strip_prefix('#') {
        let (hex, digits) = match number.strip_prefix(['x', 'X']) {
            Some(digits) => (true, digits),
            None => (false, number),
        };
        let end = digits.find(';')?;
        let c = numeric_entity(&digits[..end], hex)?;
        return Some((c.to_string(), text.len() - digits.len() + end + 1));
    }
    let end = body.find(|c: char| !c.is_ascii_alphanumeric())?;
    if body[end..].starts_with(';') {
        let value = named_entity(&body[..=end])?;
        return Some((value.to_owned(), end + 2));
    }
    None
}

#[test]
fn entities_match_the_oracle() {
    let mut differences = Vec::new();
    for (reference, expected) in &data().2 {
        let actual = decode_markdown_references(&format!("X{reference}Y"));
        if &actual != expected {
            differences.push(format!(
                "{reference:?}: oracle {expected:?}, randoc {actual:?}"
            ));
        }
    }
    assert!(data().2.len() >= 2231 + 300);
    check("entities", data().2.len(), &differences);
}

#[test]
fn emoji_aliases_match_the_oracle() {
    let mut differences = Vec::new();
    for (alias, expected) in &data().3 {
        let actual = emoji_from_alias(alias);
        if actual != expected.as_deref() {
            differences.push(format!("{alias:?}: oracle {expected:?}, randoc {actual:?}"));
        }
    }
    check("emoji", data().3.len(), &differences);
}

#[test]
fn abbreviations_match_the_oracle() {
    let sample: Vec<(String, bool)> =
        serde_json::from_str(include_str!("abbreviations_oracle.json")).expect("valid sample");
    let set = default_abbreviations();
    let mut differences = Vec::new();
    for (candidate, joined) in &sample {
        let actual = set.binary_search(&candidate.as_str()).is_ok();
        if actual != *joined {
            differences.push(format!("{candidate:?}: oracle {joined}, randoc {actual}"));
        }
    }
    let kept = sample.iter().filter(|(_, joined)| *joined).count();
    assert_eq!(kept, set.len().min(300));
    assert_eq!(sample.len() - kept, 300);
    check("abbreviations", sample.len(), &differences);
}

#[test]
fn nfc_matches_the_oracle() {
    let cases: Vec<(String, String)> =
        serde_json::from_str(include_str!("nfc_oracle.json")).expect("valid cases");
    let mut differences = Vec::new();
    for (input, expected) in &cases {
        let actual = nfc(input);
        if actual != expected.as_str() {
            differences.push(format!("{input:?}: oracle {expected:?}, randoc {actual:?}"));
        }
        if is_nfc_quick(input) && input != expected {
            differences.push(format!("{input:?}: the quick check is wrong"));
        }
    }
    assert!(cases.len() >= 2000);
    check("nfc", cases.len(), &differences);
}
