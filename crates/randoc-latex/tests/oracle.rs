//! Runs the writer on each recorded `#write:` unit of the `latex-writer`
//! conformance suite and compares the output with the oracle output.
//!
//! The program runs the same units through the harness. This test calls the
//! writer directly, so that coverage and mutation tests see the units too.

// Test code: a missing or broken record file must stop the test.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::Path;

use randoc_ast::json::from_json;
use randoc_core::{CiteMethod, Context, Extension, WriterOptions, default_extensions};

/// Units that differ from the oracle for a known reason (see `docs/ISSUES.md`).
const KNOWN_DIVERGENCES: &[&str] = &[
    // I-LATEX-01: tokens of the syntax highlighter.
    "blocks/block-9#write:latex",
    "blocks/block-10#write:latex",
];

/// The command-line options of each case of a suite, by case identifier.
fn case_args(suite: &str) -> HashMap<String, Vec<String>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/cases/latex-writer")
        .join(format!("{suite}.toml"));
    let text = std::fs::read_to_string(path).unwrap();
    let value: toml::Table = toml::from_str(&text).unwrap();
    let strings = |value: Option<&toml::Value>| -> Vec<String> {
        value
            .and_then(toml::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let file_args = strings(value.get("args"));
    let mut args = HashMap::new();
    for case in value["case"].as_array().unwrap() {
        let own = case.get("args").map(|_| strings(case.get("args")));
        args.insert(
            case["id"].as_str().unwrap().to_owned(),
            own.unwrap_or_else(|| file_args.clone()),
        );
    }
    args
}

/// Sets the options of `args` that this test knows. `false` for another option.
fn apply(args: &[String], options: &mut WriterOptions) -> bool {
    for arg in args {
        if let Some(level) = arg.strip_prefix("--slide-level=") {
            options.slide_level = Some(level.parse().unwrap());
        } else if arg == "--natbib" {
            options.cite_method = CiteMethod::Natbib;
        } else if arg == "--biblatex" {
            options.cite_method = CiteMethod::Biblatex;
        } else {
            return false;
        }
    }
    true
}

fn units() -> Vec<(String, String, String, String, Vec<String>)> {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/expected/latex-writer");
    let mut entries: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            !name.starts_with("._") && name.ends_with(".json") && name != "options.json"
        })
        .collect();
    entries.sort();
    let mut units = Vec::new();
    for path in entries {
        let suite = path.file_stem().unwrap().to_string_lossy().into_owned();
        let args = case_args(&suite);
        let text = std::fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let recorded = value["units"].as_object().unwrap();
        for (id, result) in recorded {
            let Some((case, format)) = id.split_once("#write:") else {
                continue;
            };
            let input = recorded[&format!("{case}#read")]["stdout"]
                .as_str()
                .unwrap();
            let expected = result["stdout"].as_str().unwrap();
            units.push((
                format!("{suite}/{id}"),
                format.to_owned(),
                input.to_owned(),
                expected.to_owned(),
                args.get(case).cloned().unwrap_or_default(),
            ));
        }
    }
    units
}

#[test]
fn the_writer_matches_the_recorded_oracle_output() {
    let units = units();
    assert!(units.len() >= 600, "only {} units found", units.len());
    let mut failures = Vec::new();
    for (id, format, input, expected, args) in &units {
        let doc = from_json(input).unwrap();
        let mut options = WriterOptions::default();
        if !apply(args, &mut options) {
            continue;
        }
        let (name, smart) = match format.strip_suffix("-smart") {
            Some(name) => (name, false),
            None => (format.as_str(), true),
        };
        options.extensions = default_extensions(name);
        if !smart {
            options.extensions.disable(Extension::Smart);
        }
        let mut context = Context::pure();
        let mut text = match name {
            "beamer" => randoc_latex::write_beamer(&doc, &options, &mut context),
            _ => randoc_latex::write_latex(&doc, &options, &mut context),
        }
        .unwrap();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        let known = KNOWN_DIVERGENCES.contains(&id.as_str());
        if (text == *expected) == known {
            failures.push(id.clone());
        }
    }
    assert!(
        failures.is_empty(),
        "units that differ from the record: {failures:?}"
    );
}
