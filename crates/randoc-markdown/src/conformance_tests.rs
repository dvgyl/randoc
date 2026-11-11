//! Runs the reader on the Markdown conformance cases and compares the tree,
//! the messages and the error text with the stored oracle results.
//!
//! `passing_cases.txt` lists each case that must pass. Set the environment
//! variable `RANDOC_UPDATE_PASSING=1` to write the list again from the
//! current results.

use std::path::{Path, PathBuf};

use randoc_core::{
    Context, Extension, FormatSpec, Reader, ReaderOptions, Sources, default_extensions,
};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn conformance_dir() -> PathBuf {
    crate_dir().join("../../conformance")
}

fn options_for(spec: &FormatSpec) -> ReaderOptions {
    let mut extensions = default_extensions(&spec.name);
    for (enable, name) in &spec.changes {
        if let Some(extension) = Extension::from_name(name) {
            if *enable {
                extensions.enable(extension);
            } else {
                extensions.disable(extension);
            }
        }
    }
    let mut options = ReaderOptions::default();
    options.extensions = extensions;
    options.columns = 72;
    options
}

/// Runs one case as the program does and returns its standard output,
/// standard error and exit code.
fn run_case(from: &str, input: &str) -> (String, String, i64) {
    let spec = FormatSpec::parse(from);
    let Some(Reader::Text(reader)) = crate::reader(&spec.name) else {
        return (String::new(), String::new(), -1);
    };
    let options = options_for(&spec);
    let mut context = Context::pure();
    let result = reader.read(&Sources::from_text(input), &options, &mut context);
    let mut stderr: String = context.log().iter().map(|m| m.render()).collect();
    match result {
        Ok(doc) => (format!("{}\n", randoc_ast::json::to_json(&doc)), stderr, 0),
        Err(error) => {
            stderr.push_str(&format!("{error}\n"));
            (String::new(), stderr, i64::from(error.exit_code()))
        }
    }
}

/// Returns the identifier and the verdict of each case.
fn results() -> Vec<(String, bool)> {
    let cases_dir = conformance_dir().join("cases/markdown");
    let mut out = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&cases_dir)
        .map(|dir| dir.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    entries.sort();
    for path in entries {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if name.starts_with("._") || path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        out.extend(file_results(&path, name));
    }
    out
}

fn file_results(path: &Path, name: &str) -> Vec<(String, bool)> {
    let cases: toml::Table = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or_default();
    let expected: serde_json::Value =
        std::fs::read_to_string(conformance_dir().join(format!("expected/markdown/{name}.json")))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
    let default_from = cases
        .get("from")
        .and_then(|v| v.as_str())
        .unwrap_or("markdown");
    let list = cases
        .get("case")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for case in list {
        let id = case.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let input = case.get("input").and_then(|v| v.as_str()).unwrap_or("");
        let from = case
            .get("from")
            .and_then(|v| v.as_str())
            .unwrap_or(default_from);
        let unit = &expected["units"][format!("{id}#read")];
        let (stdout, stderr, exit) = run_case(from, input);
        let ok = unit["stdout"].as_str() == Some(stdout.as_str())
            && unit["stderr"].as_str() == Some(stderr.as_str())
            && unit["exit"].as_i64() == Some(exit);
        out.push((format!("{name}/{id}"), ok));
    }
    out
}

#[test]
fn listed_cases_match_the_oracle() {
    let list_path = crate_dir().join("src/passing_cases.txt");
    let results = results();
    if std::env::var_os("RANDOC_UPDATE_PASSING").is_some() {
        let passing: Vec<&str> = results
            .iter()
            .filter(|(_, ok)| *ok)
            .map(|(id, _)| id.as_str())
            .collect();
        let _ = std::fs::write(&list_path, passing.join("\n") + "\n");
        return;
    }
    let listed = std::fs::read_to_string(&list_path).unwrap_or_default();
    let mut missing = Vec::new();
    let mut failing = Vec::new();
    for id in listed.lines().filter(|l| !l.is_empty()) {
        match results.iter().find(|(r, _)| r == id) {
            Some((_, true)) => {}
            Some((_, false)) => failing.push(id.to_string()),
            None => missing.push(id.to_string()),
        }
    }
    assert!(
        failing.is_empty() && missing.is_empty() && !listed.is_empty(),
        "failing: {failing:?}; missing: {missing:?}"
    );
}
