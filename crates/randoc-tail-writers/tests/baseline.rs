//! Baseline conversion tests.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::Path;

use randoc_ast::json;
use randoc_core::{
    Context, FormatSpec, Output, Reader, ReaderOptions, Sources, Verbosity, WriterOptions,
};
use randoc_tail_writers::{reader, writer};
use serde_json::Value;

fn expected(category: &str, suite: &str) -> Value {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/expected");
    let text =
        std::fs::read_to_string(directory.join(category).join(format!("{suite}.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn warnings(context: &Context) -> String {
    context
        .log()
        .iter()
        .filter(|message| message.level() <= Verbosity::default())
        .map(randoc_core::LogMessage::render)
        .collect()
}

fn check_reader(format: &str, case: &str, input: &str) {
    let Reader::Text(read) = reader(format).unwrap() else {
        panic!("Expected a text reader.");
    };
    let mut options = ReaderOptions::default();
    options.extensions = FormatSpec::parse(format).extensions().unwrap();
    let mut context = Context::pure();
    let doc = read
        .read(&Sources::from_text(input), &options, &mut context)
        .unwrap();
    let expected_doc = expected("reader-baseline", format);
    let recorded = &expected_doc["units"][format!("{case}#read")];
    let expected_output = recorded["stdout"].as_str().unwrap();
    assert_eq!(doc, json::from_json(expected_output).unwrap());
    assert_eq!(recorded["exit"], 0);
    assert_eq!(recorded["stderr"], warnings(&context));
}

fn check_writer(category: &str, suite: &str, format: &str) {
    let expected_doc = expected(category, suite);
    let units = expected_doc["units"].as_object().unwrap();
    let mut checked = 0;
    let mut failures = Vec::new();
    for (id, recorded) in units {
        let Some((case, target)) = id.split_once("#write:") else {
            continue;
        };
        let spec = FormatSpec::parse(target);
        if spec.name != format {
            continue;
        }
        let ast = units[&format!("{case}#read")]["stdout"].as_str().unwrap();
        let doc = json::from_json(ast).unwrap();
        let mut options = WriterOptions::default();
        options.extensions = spec.extensions().unwrap();
        let mut context = Context::pure();
        let Output::Text(mut actual) = writer(format)
            .unwrap()
            .write(&doc, &options, &mut context)
            .unwrap()
        else {
            panic!("Expected text output.");
        };
        if options.template.is_none() && !actual.ends_with('\n') {
            actual.push('\n');
        }
        let expected_output = recorded["stdout"].as_str().unwrap();
        assert_eq!(recorded["exit"], 0, "{id}");
        assert_eq!(recorded["stderr"], warnings(&context), "{id}");
        let end_to_end = &units[&format!("{case}#e2e:{target}")];
        assert_eq!(recorded["stdout"], end_to_end["stdout"], "{id}");
        assert_eq!(recorded["exit"], end_to_end["exit"], "{id}");
        assert_eq!(recorded["stderr"], end_to_end["stderr"], "{id}");
        if actual != expected_output {
            failures.push(format!(
                "{suite}/{id}
EXPECTED {expected_output:?}
ACTUAL   {actual:?}"
            ));
        }
        checked += 1;
    }
    assert!(checked > 0, "{suite}/{format}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn write_ansi() {
    check_writer("writer-baseline", "tail", "ansi");
}

#[test]
fn write_texinfo() {
    check_writer("writer-baseline", "tail", "texinfo");
}

#[test]
fn write_icml() {
    check_writer("writer-baseline", "tail", "icml");
}

#[test]
fn write_context() {
    check_writer("writer-baseline", "tail", "context");
}

#[test]
fn write_xml() {
    check_writer("writer-baseline", "tail", "xml");
}

#[test]
fn write_bbcode() {
    check_writer("writer-baseline", "bbcode", "bbcode");
}

#[test]
fn write_bbcode_fluxbb() {
    check_writer("writer-baseline", "bbcode", "bbcode_fluxbb");
}

#[test]
fn write_bbcode_hubzilla() {
    check_writer("writer-baseline", "bbcode", "bbcode_hubzilla");
}

#[test]
fn write_bbcode_phpbb() {
    check_writer("writer-baseline", "bbcode", "bbcode_phpbb");
}

#[test]
fn write_bbcode_steam() {
    check_writer("writer-baseline", "bbcode", "bbcode_steam");
}

#[test]
fn write_bbcode_xenforo() {
    check_writer("writer-baseline", "bbcode", "bbcode_xenforo");
}

#[test]
fn write_context_extensions() {
    check_writer("extensions", "writer-extensions", "context");
}

#[test]
fn read_xml_metadata() {
    check_reader(
        "xml",
        "metadata",
        r#"<Pandoc api-version="1,23,1,2">
<meta><entry key="title"><MetaInlines>Rain log</MetaInlines></entry><entry key="draft"><MetaBool>true</MetaBool></entry><entry key="site"><MetaString>East bank</MetaString></entry></meta>
<blocks>
<Para>Meet at the north gate.</Para>
</blocks>
</Pandoc>
"#,
    );
}
