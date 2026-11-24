//! Replays `tests/parity_cases.jsonl` (`tools/parity_cases.py`): original
//! inputs with the output of Pandoc 3.13 and the first line of each warning.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use randoc_core::{Context, FormatSpec, Output, ReferenceLocation, WrapOption, WriterOptions};
use serde_json::Value;

fn options(format: &str, opts: &Value) -> WriterOptions {
    let spec = FormatSpec::parse(format);
    let mut options = WriterOptions::default();
    options.extensions = spec.extensions().unwrap();
    let flag = |key: &str| opts[key].as_bool().unwrap_or(false);
    if let Some(columns) = opts["columns"].as_i64() {
        options.columns = columns;
    }
    if let Some(tab_stop) = opts["tab_stop"].as_i64() {
        options.tab_stop = tab_stop;
    }
    options.wrap_text = match opts["wrap"].as_str() {
        Some("none") => WrapOption::WrapNone,
        Some("preserve") => WrapOption::WrapPreserve,
        _ => WrapOption::WrapAuto,
    };
    options.reference_location = match opts["refloc"].as_str() {
        Some("block") => ReferenceLocation::EndOfBlock,
        Some("section") => ReferenceLocation::EndOfSection,
        _ => ReferenceLocation::EndOfDocument,
    };
    options.prefer_ascii = flag("ascii");
    options.table_of_contents = flag("toc");
    options.setext_headers = flag("setext");
    options.reference_links = flag("reference_links");
    if flag("standalone") {
        let source = randoc_data::default_template(&spec.name).unwrap();
        let mut load = |_: &str| Err("no partials");
        options.template =
            Some(randoc_templates::Template::compile("t", source, 64, &mut load).unwrap());
    }
    options
}

#[test]
fn the_parity_cases_give_the_output_of_pandoc() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity_cases.jsonl");
    let data = std::fs::read_to_string(path).unwrap();
    let mut failures = Vec::new();
    let mut cases = 0;
    for line in data.lines() {
        let case: Value = serde_json::from_str(line).unwrap();
        let name = case["name"].as_str().unwrap();
        let format = case["format"].as_str().unwrap();
        let doc = randoc_ast::json::from_json(case["doc"].as_str().unwrap()).unwrap();
        let options = options(format, &case["opts"]);
        let writer = randoc_markdown_writer::writer(&FormatSpec::parse(format).name).unwrap();
        let mut context = Context::pure();
        let Output::Text(mut text) = writer.write(&doc, &options, &mut context).unwrap() else {
            panic!("{name}: the writer gives text");
        };
        // The program adds the final line break of the output.
        if !text.ends_with('\n') {
            text.push('\n');
        }
        let warnings: String = context
            .log()
            .iter()
            .map(|message| format!("{}\n", message.render().lines().next().unwrap_or("")))
            .collect();
        cases += 1;
        if text != case["expected"].as_str().unwrap()
            || warnings != case["warnings"].as_str().unwrap()
        {
            failures.push(format!("{name}:\n{text}{warnings}"));
        }
    }
    assert!(cases >= 110, "only {cases} cases");
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
