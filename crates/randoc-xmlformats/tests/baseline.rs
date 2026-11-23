//! Baseline conversion tests.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::Path;

use randoc_ast::json;
use randoc_core::{
    Context, FormatSpec, Output, Reader, ReaderOptions, Sources, Verbosity, WriterOptions,
};
use randoc_xmlformats::{reader, writer};
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
        if category == "extensions" && spec.name == "jats" {
            options.template = Some(
                randoc_templates::Template::compile(
                    "citations.jats",
                    "<article>\n<body>\n$body$\n</body>\n<back>\n$back$\n</back>\n</article>\n",
                    128,
                    &mut |_| Err(()),
                )
                .unwrap(),
            );
        }
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
fn write_docbook() {
    check_writer("writer-baseline", "xmlformats", "docbook");
}

#[test]
fn write_docbook4() {
    check_writer("writer-baseline", "xmlformats", "docbook4");
}

#[test]
fn write_docbook5() {
    check_writer("writer-baseline", "xmlformats", "docbook5");
}

#[test]
fn write_jats() {
    check_writer("writer-baseline", "xmlformats", "jats");
}

#[test]
fn write_jats_archiving() {
    check_writer("writer-baseline", "xmlformats", "jats_archiving");
}

#[test]
fn write_jats_publishing() {
    check_writer("writer-baseline", "xmlformats", "jats_publishing");
}

#[test]
fn write_jats_articleauthoring() {
    check_writer("writer-baseline", "xmlformats", "jats_articleauthoring");
}

#[test]
fn write_tei() {
    check_writer("writer-baseline", "xmlformats", "tei");
}

#[test]
fn write_fb2() {
    check_writer("writer-baseline", "xmlformats", "fb2");
}

#[test]
fn write_jats_extensions() {
    check_writer("extensions", "writer-extensions", "jats");
}

#[test]
fn read_fb2_inline_image() {
    check_reader(
        "fb2",
        "inline-image",
        r#"<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink">
<body><section>
<p>The sign <image l:href="lamp.png" alt="A lantern"/> marked the steps.</p>
</section></body>
</FictionBook>
"#,
    );
}

#[test]
fn read_fb2_block_image() {
    check_reader(
        "fb2",
        "block-image",
        r#"<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink">
<body><section>
<image l:href="bank.png" alt="Steps by the bank" title="The landing"/>
</section></body>
</FictionBook>
"#,
    );
}
