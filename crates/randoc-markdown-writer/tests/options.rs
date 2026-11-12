//! Runs the writer-option cases of `conformance/cases/markdown-writer/options.toml`
//! through the library with a `WriterOptions` value, and compares the output with
//! the stored oracle results. The program cannot pass these options yet.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::Path;

use randoc_core::{
    Context, FormatSpec, Output, ReferenceLocation, TopLevelDivision, WrapOption, WriterOptions,
};

/// The units that differ from Pandoc, with the reason in `docs/ISSUES.md`
/// (`I-MDW-05` to `I-MDW-08`).
const KNOWN: &[&str] = &[
    "ascii#write:commonmark",
    "ascii#write:gfm",
    "ascii#write:markdown",
    "ascii#write:plain",
    "ascii-smart-off#write:gfm",
    "ascii-smart-off#write:markdown-smart",
    "columns-table#write:gfm",
    "dpi-image#write:gfm",
    "dpi-image#write:markdown-link_attributes+raw_html",
    "reference-links#write:gfm",
    "reference-links#write:markdown",
    "reference-links#write:plain",
    "reference-links-block#write:gfm",
    "reference-links-block#write:markdown",
    "reference-links-block#write:plain",
    "reference-links-commonmark#write:commonmark",
    "reference-links-commonmark#write:markdown_strict",
    "reference-links-figure#write:markdown",
    "reference-links-same-text#write:gfm",
    "reference-links-same-text#write:markdown",
    "reference-links-same-text#write:plain",
    "setext-attr#write:commonmark",
    "setext-attr#write:gfm",
    "wrap-none-table#write:gfm",
];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."))
}

/// Applies the command-line options to writer options. Returns `None` for an
/// option that the library cannot apply, for example standalone output.
fn apply(args: &[String], options: &mut WriterOptions) -> Option<()> {
    for arg in args {
        let (name, value) = arg.split_once('=').unwrap_or((arg.as_str(), ""));
        match name {
            "--wrap" => {
                options.wrap_text = match value {
                    "none" => WrapOption::WrapNone,
                    "preserve" => WrapOption::WrapPreserve,
                    _ => WrapOption::WrapAuto,
                }
            }
            "--columns" => options.columns = value.parse().ok()?,
            "--tab-stop" => options.tab_stop = value.parse().ok()?,
            "--reference-links" => options.reference_links = true,
            "--reference-location" => {
                options.reference_location = match value {
                    "block" => ReferenceLocation::EndOfBlock,
                    "section" => ReferenceLocation::EndOfSection,
                    _ => ReferenceLocation::EndOfDocument,
                }
            }
            "--markdown-headings" => options.setext_headers = value == "setext",
            "--ascii" => options.prefer_ascii = true,
            "--id-prefix" => options.identifier_prefix = value.into(),
            "--number-sections" => options.number_sections = true,
            "--list-tables" => options.list_tables = true,
            "--dpi" => options.dpi = value.parse().ok()?,
            "--toc" => options.table_of_contents = true,
            "--top-level-division" => {
                options.top_level_division = match value {
                    "part" => TopLevelDivision::TopLevelPart,
                    "chapter" => TopLevelDivision::TopLevelChapter,
                    "section" => TopLevelDivision::TopLevelSection,
                    _ => TopLevelDivision::TopLevelDefault,
                }
            }
            _ => return None,
        }
    }
    Some(())
}

/// Runs each write unit of one case file through the library. Returns
/// `true` for each unit with the oracle output.
fn run_file(name: &str) -> BTreeMap<String, bool> {
    let cases_text = std::fs::read_to_string(
        root().join(format!("conformance/cases/markdown-writer/{name}.toml")),
    )
    .unwrap();
    let cases: toml::Value = toml::from_str(&cases_text).unwrap();
    let expected_text = std::fs::read_to_string(
        root().join(format!("conformance/expected/markdown-writer/{name}.json")),
    )
    .unwrap();
    let expected: serde_json::Value = serde_json::from_str(&expected_text).unwrap();
    let default_to: Vec<String> = cases["to"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    let mut results: BTreeMap<String, bool> = BTreeMap::new();
    for case in cases["case"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let args: Vec<String> = case
            .get("args")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().map(|v| v.as_str().unwrap().to_owned()).collect())
            .unwrap_or_default();
        let to: Vec<String> = case
            .get("to")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().map(|v| v.as_str().unwrap().to_owned()).collect())
            .unwrap_or_else(|| default_to.clone());
        let doc = randoc_ast::json::from_json(case["input"].as_str().unwrap()).unwrap();
        for format in to {
            let unit = format!("{id}#write:{format}");
            let mut options = WriterOptions::default();
            if apply(&args, &mut options).is_none() {
                continue;
            }
            let spec = FormatSpec::parse(&format);
            options.extensions = spec.extensions().unwrap();
            let writer = randoc_markdown_writer::writer(&spec.name).unwrap();
            let Output::Text(mut text) =
                writer.write(&doc, &options, &mut Context::pure()).unwrap()
            else {
                panic!("text output expected")
            };
            if !text.ends_with('\n') {
                text.push('\n');
            }
            let want = expected["units"][&unit]["stdout"].as_str().unwrap();
            results.insert(unit, text == want);
        }
    }
    results
}

/// The case files without command-line options, with the number of write
/// units that pass. A change that lowers a number is a regression.
const FLOORS: &[(&str, usize)] = &[
    ("blocks", 336),
    ("code", 286),
    ("escaping", 1437),
    ("figures-divs-spans", 306),
    ("headings", 222),
    ("inlines", 471),
    ("links-images", 260),
    ("lists", 503),
    ("math-raw", 243),
    ("notes-citations", 229),
    ("tables", 250),
    ("wrapping", 388),
];

#[test]
fn conformance_cases_give_the_oracle_output() {
    let mut report = Vec::new();
    for (name, floor) in FLOORS {
        let results = run_file(name);
        let passing = results.values().filter(|p| **p).count();
        report.push(format!("{name} {passing}/{}", results.len()));
        assert!(
            passing >= *floor,
            "{name}: {passing} units pass, {floor} before"
        );
    }
    println!("{report:?}");
}

#[test]
fn writer_options_give_the_oracle_output() {
    let results = run_file("options");
    let failing: Vec<&String> = results
        .iter()
        .filter(|(unit, pass)| !**pass && !KNOWN.contains(&unit.as_str()))
        .map(|(unit, _)| unit)
        .collect();
    let passing = results.values().filter(|p| **p).count();
    println!("option units: {passing} of {} pass", results.len());
    assert!(failing.is_empty(), "units that differ: {failing:#?}");
}
