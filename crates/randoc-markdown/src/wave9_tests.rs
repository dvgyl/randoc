//! The oracle cases of the Wave 9 parity fixes of this reader (batches MD-B1
//! and MD-B2), and of the raw TeX that it reads with `randoc-latex-reader`
//! (batch MD-A). `tests/wave9_cases.json` holds each input with the JSON
//! output of Pandoc 3.13 (`-t json`, `--strip-comments` when the case says
//! so) and, for some cases, its warnings in a short form.

use randoc_core::{Context, Extension, FormatSpec, LogMessage, ReaderOptions, default_extensions};

/// Reads one case as the program does. Returns the JSON text and the
/// warnings in the short form of the case file.
fn read(case: &serde_json::Value) -> (String, Vec<String>) {
    let spec = FormatSpec::parse(case["from"].as_str().unwrap_or("markdown"));
    let mut options = ReaderOptions::default();
    options.extensions = default_extensions(&spec.name);
    for (enable, name) in &spec.changes {
        if let Some(extension) = Extension::from_name(name) {
            if *enable {
                options.extensions.enable(extension);
            } else {
                options.extensions.disable(extension);
            }
        }
    }
    options.columns = 72;
    options.strip_comments = case["strip_comments"].as_bool().unwrap_or(false);
    let mut context = Context::pure();
    let input = case["input"].as_str().unwrap_or("");
    let doc = crate::read_markdown(input, "", &options, &mut context).unwrap_or_default();
    let messages = context.log().iter().map(short_message).collect();
    (randoc_ast::json::to_json(&doc), messages)
}

/// Returns the short form of a warning, for example `unclosed-div 1:1 5:1`.
fn short_message(message: &LogMessage) -> String {
    match message {
        LogMessage::UnclosedDiv(open, close) => format!(
            "unclosed-div {}:{} {}:{}",
            open.line, open.column, close.line, close.column
        ),
        LogMessage::DuplicateIdentifier(id, pos) => {
            format!("duplicate-identifier {id} {}:{}", pos.line, pos.column)
        }
        other => other.render(),
    }
}

/// Checks each case whose identifier starts with `fix` and a space.
fn check(fix: &str) {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/wave9_cases.json");
    let text = std::fs::read_to_string(path).unwrap();
    let cases: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap();
    let prefix = format!("{fix} ");
    let mut count = 0;
    let mut failures = Vec::new();
    for case in &cases {
        let id = case["id"].as_str().unwrap_or("");
        if !id.starts_with(&prefix) {
            continue;
        }
        count += 1;
        let (json, messages) = read(case);
        let expected: Option<Vec<String>> = case.get("messages").map(|list| {
            list.as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| m.as_str().map(String::from))
                .collect()
        });
        let json_ok = json == case["json"].as_str().unwrap_or("");
        let messages_ok = expected.as_ref().is_none_or(|e| *e == messages);
        if !json_ok || !messages_ok {
            failures.push(format!(
                "{id}\n  oracle {} {expected:?}\n  randoc {json} {messages:?}",
                case["json"]
            ));
        }
    }
    assert!(count > 0, "no case for {fix}");
    assert!(
        failures.is_empty(),
        "{} of {count} cases of {fix} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn md_b1_01_grid_cell_ends_at_its_own_border() {
    check("MD-B1-1");
}

#[test]
fn md_b1_02_grid_column_widths() {
    check("MD-B1-2");
}

#[test]
fn md_b1_03_caption_before_a_multiline_table() {
    check("MD-B1-3");
}

#[test]
fn md_b1_04_multimarkdown_heading_labels() {
    check("MD-B1-4");
}

#[test]
fn md_b1_05_continuation_after_a_wide_marker() {
    check("MD-B1-5");
}

#[test]
fn md_b1_06_reference_attributes_on_the_next_line() {
    check("MD-B1-6");
}

#[test]
fn md_b1_07_citation_is_no_reference_definition() {
    check("MD-B1-7");
}

#[test]
fn md_b1_08_macros_from_metadata() {
    check("MD-B1-8");
}

#[test]
fn md_b1_09_plain_before_an_outer_closing_tag() {
    check("MD-B1-9");
}

#[test]
fn md_b1_10_invalid_fence_information() {
    check("MD-B1-10");
}

#[test]
fn md_b1_11_code_keeps_the_input_tabs() {
    check("MD-B1-11");
}

#[test]
fn md_b1_12_unclosed_div_in_a_container() {
    check("MD-B1-12");
}

#[test]
fn md_b1_13_list_looseness() {
    check("MD-B1-13");
}

#[test]
fn md_b1_14_roman_numeral_grammar() {
    check("MD-B1-14");
}

#[test]
fn md_b2_1_strip_comments() {
    check("MD-B2-1");
}

#[test]
fn md_b2_2_empty_implicit_header_reference() {
    check("MD-B2-2");
}

#[test]
fn md_b2_3_braced_citation_key_text() {
    check("MD-B2-3");
}

#[test]
fn md_b2_4_bracket_in_math_in_link_text() {
    check("MD-B2-4");
}

#[test]
fn md_b2_5_note_number_after_a_failed_image() {
    check("MD-B2-5");
}

#[test]
fn md_a_raw_tex_through_read_markdown() {
    check("MD-A");
}

#[test]
fn four_space_rule_continues_wide_ordered_list_at_four_spaces() {
    use randoc_ast::{Block, Inline, ListAttributes, ListNumberDelim, ListNumberStyle};
    use randoc_core::{Context, FormatSpec, ReaderOptions};

    let input = "1000. one\n\n    continuation\n";
    let mut opts = ReaderOptions::default();
    opts.extensions = FormatSpec::parse("markdown+four_space_rule")
        .extensions()
        .unwrap();
    let mut ctx = Context::pure();
    let doc = crate::read_markdown(input, "", &opts, &mut ctx).unwrap();
    assert_eq!(
        doc.blocks,
        vec![Block::OrderedList(
            ListAttributes {
                start: 1000,
                style: ListNumberStyle::Decimal,
                delim: ListNumberDelim::Period,
            },
            vec![vec![
                Block::Para(vec![Inline::Str("one".into())]),
                Block::Para(vec![Inline::Str("continuation".into())]),
            ]],
        )]
    );

    let mut default_opts = ReaderOptions::default();
    default_opts.extensions = FormatSpec::parse("markdown").extensions().unwrap();
    let mut ctx2 = Context::pure();
    let doc2 = crate::read_markdown(input, "", &default_opts, &mut ctx2).unwrap();
    assert_eq!(doc2.blocks.len(), 2);
    assert!(matches!(doc2.blocks[1], Block::CodeBlock(_, _)));
}

#[test]
fn wave10_reader_probes_match_the_oracle() {
    for line in include_str!("../tests/wave10_cases.jsonl").lines() {
        let case: serde_json::Value = serde_json::from_str(line).unwrap();
        let (json, messages) = read(&case);
        assert_eq!(json, case["json"].as_str().unwrap(), "{}", case["id"]);
        if let Some(expected) = case.get("messages") {
            assert_eq!(serde_json::json!(messages), *expected, "{}", case["id"]);
        }
    }
}

#[test]
fn yaml_metadata_reports_a_control_character() {
    let spec = FormatSpec::parse("markdown");
    let mut options = ReaderOptions::default();
    options.extensions = spec.extensions().unwrap();
    let error = crate::read_markdown(
        "---\ntitle: \u{b} t \u{b}\n---\n",
        "",
        &options,
        &mut Context::pure(),
    )
    .unwrap_err();
    assert!(error.to_string().contains(":2:8: invalid character U+000B"));
    assert!(error.to_string().contains("2 | title: ␋ t ␋"));
}
