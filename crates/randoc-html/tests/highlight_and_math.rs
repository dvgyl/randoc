//! Checks highlighted blocks and TeX fallback warnings against Pandoc 3.13.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::Path;

use randoc_core::{Context, FormatSpec, Output, WriterOptions};
use serde_json::Value;

fn oracle(suite: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/expected")
        .join(format!("{suite}.json"));
    let data = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&data).unwrap()
}

fn check_write(oracle: &Value, case: &str, format: &str) {
    let units = &oracle["units"];
    let input = units[format!("{case}#read")]["stdout"].as_str().unwrap();
    let expected = &units[format!("{case}#write:{format}")];
    let doc = randoc_ast::json::from_json(input).unwrap();
    let mut options = WriterOptions::default();
    options.extensions = FormatSpec::parse(format).extensions().unwrap();
    let mut context = Context::pure();
    let writer = randoc_html::writer(format).unwrap();
    let Output::Text(text) = writer.write(&doc, &options, &mut context).unwrap() else {
        panic!("the HTML writer gives text");
    };
    assert_eq!(
        format!("{text}\n"),
        expected["stdout"].as_str().unwrap(),
        "{case} in {format}"
    );
    let warnings: String = context
        .log()
        .iter()
        .map(|message| message.render())
        .collect();
    assert_eq!(
        warnings,
        expected["stderr"].as_str().unwrap(),
        "{case} in {format}"
    );
}

#[test]
fn m4_blocks_match_the_oracle() {
    let oracle = oracle("highlight/blocks-2");
    for index in 0..5 {
        check_write(&oracle, &format!("m4-{index}"), "html");
    }
}

#[test]
fn tex_fallback_warning_matches_the_oracle() {
    let oracle = oracle("html-writer/cite-math");
    for format in ["html", "html4"] {
        check_write(&oracle, "math-special-chars", format);
    }
}
