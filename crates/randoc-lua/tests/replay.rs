//! Replays the recorded results of Pandoc 3.13 for a set of Lua filters.
//!
//! `tests/replay/cases.txt` holds the documents and the filters.
//! `tests/replay/expected.txt` holds the result of the oracle for each run;
//! `tools/record-replay.py` writes it. `tests/replay/walk.txt` and
//! `walk-expected.txt` are the oracle matrix of the tree walk, which
//! `tools/walk-matrix.py` writes. The test applies each filter to each
//! of its documents and compares the JSON output, or the error text and the
//! exit code, with the record.
#![cfg(not(target_arch = "wasm32"))]
// The helpers of the test stop the test with a panic.
#![allow(clippy::panic, clippy::unwrap_used)]

use std::collections::HashMap;

use randoc_ast::json;
use randoc_core::Context;
use randoc_lua::{FilterEnvironment, LuaFilter, RuntimeLimits, apply_filter};

const HEADER: &str = "@@@ ";

/// The runs that differ from the oracle for a reason that a row of
/// `docs/ISSUES.md` records. The test fails when one of them becomes equal,
/// so that the list stays exact.
const KNOWN: [(&str, &str, &str); 2] = [
    ("walk-error-error-in-walk", "walk-allkinds", "I-LUA-01"),
    ("walk-error-error-in-walk", "walk-deep", "I-LUA-01"),
];

/// Splits a file into its sections: the words of each header line and the
/// text below it.
fn sections(text: &str) -> Vec<(Vec<&str>, String)> {
    let mut sections: Vec<(Vec<&str>, String)> = Vec::new();
    for line in text.split_inclusive('\n') {
        match line.strip_prefix(HEADER) {
            Some(header) => sections.push((header.split_whitespace().collect(), String::new())),
            None => {
                if let Some((_, body)) = sections.last_mut() {
                    body.push_str(line);
                }
            }
        }
    }
    sections
}

/// Applies one filter to one document. Returns the exit code and the text
/// that the program writes: the JSON form, or the error message.
fn run(filter_name: &str, source: &str, doc_name: &str, doc: &str) -> (i32, String) {
    let mut doc = json::from_json(doc).unwrap_or_else(|e| panic!("document {doc_name}: {e}"));
    let path = format!("{filter_name}.lua");
    let filter = LuaFilter {
        path: &path,
        source: source.as_bytes(),
    };
    let mut environment = FilterEnvironment::new("json");
    environment.input_files = vec![format!("{doc_name}.json")];
    environment.limits = RuntimeLimits {
        max_instructions: Some(2_000_000_000),
        max_memory: Some(64 << 20),
    };
    match apply_filter(&mut doc, &filter, &environment, &mut Context::pure()) {
        Ok(()) => (0, format!("{}\n", json::to_json(&doc))),
        Err(error) => (error.exit_code(), format!("{error}\n")),
    }
}

#[test]
fn each_filter_gives_the_result_of_the_oracle() {
    replay(
        include_str!("replay/cases.txt"),
        include_str!("replay/expected.txt"),
        150,
    );
}

#[test]
fn each_walk_of_the_matrix_gives_the_result_and_the_order_of_the_oracle() {
    replay(
        include_str!("replay/walk.txt"),
        include_str!("replay/walk-expected.txt"),
        80,
    );
}

#[test]
fn each_probe_of_the_module_matrix_gives_the_result_of_the_oracle() {
    replay(
        include_str!("replay/modules.txt"),
        include_str!("replay/modules-expected.txt"),
        5,
    );
}

/// Runs each filter of `cases` and compares the results with `expected`.
/// `minimum` is the number of filters that the file must have.
fn replay(cases: &str, expected_text: &str, minimum: usize) {
    let mut docs = HashMap::new();
    let mut filters = Vec::new();
    for (header, body) in sections(cases) {
        match header.as_slice() {
            ["doc", name] => {
                docs.insert(*name, body);
            }
            ["filter", name, doc_names @ ..] => filters.push((*name, doc_names.to_vec(), body)),
            other => panic!("bad header in cases.txt: {other:?}"),
        }
    }
    let mut expected = HashMap::new();
    for (header, body) in sections(expected_text) {
        match header.as_slice() {
            [filter, doc, exit] => {
                let exit: i32 = exit.parse().unwrap();
                expected.insert((filter.to_string(), doc.to_string()), (exit, body));
            }
            other => panic!("bad header in expected.txt: {other:?}"),
        }
    }
    assert!(filters.len() >= minimum, "{} filters", filters.len());
    let mut runs = 0;
    let mut failures = Vec::new();
    for (name, doc_names, source) in &filters {
        assert!(
            doc_names.len() >= 2,
            "filter {name} has fewer than 2 documents"
        );
        for doc_name in doc_names {
            let doc = docs
                .get(doc_name)
                .unwrap_or_else(|| panic!("filter {name}: no document {doc_name}"));
            let record = expected
                .get(&(name.to_string(), doc_name.to_string()))
                .unwrap_or_else(|| panic!("{name} {doc_name}: no oracle result is recorded"));
            let result = run(name, source, doc_name, doc);
            runs += 1;
            let known = KNOWN
                .iter()
                .find(|(filter, document, _)| filter == name && document == doc_name);
            if let Some((_, _, issue)) = known {
                if result == *record {
                    failures.push(format!(
                        "{name} {doc_name}: equal now; remove {issue} from KNOWN"
                    ));
                }
                continue;
            }
            if result != *record {
                failures.push(format!(
                    "{name} {doc_name}\n  oracle: exit {} {}\n  randoc: exit {} {}",
                    record.0,
                    excerpt(&record.1, &result.1),
                    result.0,
                    excerpt(&result.1, &record.1),
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {runs} runs differ from the oracle:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Returns the part of `text` near its first difference from `other`, so
/// that a failure stays short.
fn excerpt(text: &str, other: &str) -> String {
    let start = text
        .char_indices()
        .zip(other.chars())
        .find(|((_, a), b)| a != b)
        .map_or(text.len().min(other.len()), |((index, _), _)| index);
    let from = text.floor_char_boundary(start.saturating_sub(80));
    let to = text.ceil_char_boundary((start + 160).min(text.len()));
    format!("[at {start}] {}", text[from..to].trim_end())
}
