//! Runs the reader on the inputs of the conformance cases and compares the
//! trees with the stored results of the oracle.

use std::collections::BTreeMap;
use std::path::Path;

use randoc_core::{Context, FormatSpec, Reader, ReaderOptions, Source, Sources};

struct Outcome {
    total: usize,
    failures: Vec<String>,
}

fn read_json(from: &str, input: &str) -> Option<String> {
    read_json_with(from, input, ReaderOptions::default())
}

fn read_json_with(from: &str, input: &str, options: ReaderOptions) -> Option<String> {
    read_result(from, input, options).ok()
}

/// The JSON document, or the error text.
fn read_result(from: &str, input: &str, mut options: ReaderOptions) -> Result<String, String> {
    let spec = FormatSpec::parse(from);
    options.extensions = spec.extensions().map_err(|e| e.to_string())?;
    let Some(Reader::Text(reader)) = crate::reader(&spec.name) else {
        return Err(format!("no reader {from}"));
    };
    let mut context = Context::pure();
    context.limits.max_nesting_depth = 16384;
    let sources = Sources(vec![Source {
        name: String::new(),
        text: input.replace('\r', ""),
    }]);
    let doc = reader
        .read(&sources, &options, &mut context)
        .map_err(|e| e.to_string())?;
    Ok(randoc_ast::json::to_json(&doc))
}

fn run_suite(name: &str) -> Outcome {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance");
    let cases_text = std::fs::read_to_string(root.join(format!("cases/commonmark/{name}.toml")))
        .unwrap_or_default();
    let cases: toml::Table = toml::from_str(&cases_text).unwrap_or_default();
    let expected_text =
        std::fs::read_to_string(root.join(format!("expected/commonmark/{name}.json")))
            .unwrap_or_default();
    let expected: serde_json::Value =
        serde_json::from_str(&expected_text).unwrap_or(serde_json::Value::Null);
    let default_from = cases
        .get("from")
        .and_then(|v| v.as_str())
        .unwrap_or("commonmark");
    let mut outcome = Outcome {
        total: 0,
        failures: Vec::new(),
    };
    let list = cases
        .get("case")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for case in list {
        let (Some(id), Some(input)) = (
            case.get("id").and_then(|v| v.as_str()),
            case.get("input").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        if case.get("cmd").is_some() {
            continue;
        }
        let from = case
            .get("from")
            .and_then(|v| v.as_str())
            .unwrap_or(default_from);
        let unit = &expected["units"][format!("{id}#read")];
        let exit = unit["exit"].as_i64().unwrap_or(-1);
        let want = unit["stdout"].as_str().unwrap_or("").trim_end_matches('\n');
        outcome.total += 1;
        let got = read_json(from, input);
        let ok = match got {
            Some(json) => exit == 0 && json == want,
            None => exit != 0,
        };
        if !ok {
            outcome.failures.push(id.to_owned());
        }
    }
    outcome
}

/// Runs a check on a thread with a large stack, as the `randoc` program does.
fn on_big_stack(check: impl FnOnce() + Send + 'static) {
    let handle = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(check)
        .unwrap();
    if let Err(panic) = handle.join() {
        std::panic::resume_unwind(panic);
    }
}

/// The cases that the library alone does not give: a known divergence in `docs/ISSUES.md`
/// (`sp-`), and the `east_asian_line_breaks` cases (`eal-`), whose transform the command
/// line does after each reader. The conformance harness checks both through the program.
fn known_divergence(id: &str) -> bool {
    id.starts_with("sp-") || id.starts_with("eal-") || id.starts_with("rv-eal")
}

#[test]
fn rule_and_regression_cases_match_the_oracle() {
    on_big_stack(|| check_suites(&["rules", "regression"], 30));
}

#[test]
fn delimiter_run_matrix_matches_the_oracle() {
    on_big_stack(|| check_suites(&["runs"], 9));
}

#[test]
fn extension_cases_match_the_oracle() {
    on_big_stack(|| check_suites(&["gfm", "extensions", "extensions-off", "extra"], 100));
}

/// Replays `tools/commonmark-cases/matrices.json`, the oracle results that
/// `tools/commonmark-cases/matrices.py` records.
#[test]
fn oracle_matrices_match() {
    on_big_stack(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/commonmark-cases/matrices.json");
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let entries: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap_or_default();
        let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let mut failures = Vec::new();
        for entry in &entries {
            let from = entry["from"].as_str().unwrap_or("");
            let input = entry["input"].as_str().unwrap_or("");
            let mut options = ReaderOptions::default();
            options.strip_comments = entry["strip_comments"].as_bool().unwrap_or(false);
            let ok = match read_result(from, input, options) {
                Ok(json) => entry["exit"] == 0 && entry["stdout"] == json.as_str(),
                Err(error) => {
                    entry["exit"] != 0
                        && entry
                            .get("stderr")
                            .is_none_or(|stderr| stderr == error.as_str())
                }
            };
            let count = counts
                .entry(entry["matrix"].as_str().unwrap_or("").to_owned())
                .or_default();
            count.0 += 1;
            // A known difference (a row of `docs/ISSUES.md`) must still differ.
            if let Some(issue) = entry.get("known").and_then(|k| k.as_str()) {
                if ok {
                    failures.push(format!(
                        "{from}: {input:?} equals the oracle: close {issue}"
                    ));
                }
            } else if !ok {
                count.1 += 1;
                failures.push(format!("{from}: {input:?}"));
            }
        }
        assert!(counts.len() >= 3, "matrices missing: {counts:?}");
        assert!(failures.is_empty(), "{counts:?}\n{}", failures.join("\n"));
    });
}

/// The text of a list of inlines when it has only `Str`, `Space` and
/// `SoftBreak`, as `crates/randoc-yaml/tools/replay.py` writes it.
fn inline_text(inlines: &serde_json::Value) -> Option<String> {
    let mut out = String::new();
    for inline in inlines.as_array()? {
        match inline["t"].as_str()? {
            "Str" => out.push_str(inline["c"].as_str()?),
            "Space" => out.push(' '),
            "SoftBreak" => out.push('\n'),
            _ => return None,
        }
    }
    Some(out)
}

/// The text of each `Str`, `Code` and break of a JSON tree, in order.
fn all_text(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Array(items) => items.iter().for_each(|item| all_text(item, out)),
        serde_json::Value::Object(map) => match map.get("t").and_then(|t| t.as_str()) {
            Some("Str") => out.push_str(map["c"].as_str().unwrap_or("")),
            Some("Space" | "SoftBreak" | "LineBreak") => out.push(' '),
            Some("Code") => out.push_str(map["c"][1].as_str().unwrap_or("")),
            _ => all_text(map.get("c").unwrap_or(&serde_json::Value::Null), out),
        },
        _ => {}
    }
}

/// The compact form of a metadata value of `replay.py`.
fn compact(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::json;
    let content = &value["c"];
    let text_of = |v: &serde_json::Value| {
        let mut out = String::new();
        all_text(v, &mut out);
        out
    };
    match value["t"].as_str().unwrap_or("") {
        "MetaMap" => {
            let map = content.as_object().cloned().unwrap_or_default();
            json!({ "m": map.iter().map(|(k, v)| (k.clone(), compact(v))).collect::<serde_json::Map<_, _>>() })
        }
        "MetaList" => {
            json!({ "l": content.as_array().map(|items| items.iter().map(compact).collect::<Vec<_>>()) })
        }
        "MetaBool" => json!({ "b": content }),
        "MetaString" => json!({ "s": content }),
        "MetaInlines" => match inline_text(content) {
            Some(text) => json!({ "i": text }),
            None => json!({ "i?": text_of(content) }),
        },
        "MetaBlocks" => {
            let paras: Option<Vec<String>> = content
                .as_array()
                .into_iter()
                .flatten()
                .map(|block| {
                    matches!(block["t"].as_str(), Some("Para" | "Plain"))
                        .then(|| inline_text(&block["c"]))
                        .flatten()
                })
                .collect();
            match paras {
                Some(paras) => json!({ "k": paras }),
                None => json!({ "k?": text_of(content) }),
            }
        }
        other => json!({ "unknown": other }),
    }
}

/// The result of `replay.py` for a document.
fn compact_document(json: &str) -> serde_json::Value {
    let doc: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let meta: serde_json::Map<String, serde_json::Value> = doc["meta"]
        .as_object()
        .map(|meta| meta.iter().map(|(k, v)| (k.clone(), compact(v))).collect())
        .unwrap_or_default();
    serde_json::json!({ "meta": meta, "blocks": doc["blocks"].as_array().map_or(0, Vec::len) })
}

/// Replays each metadata block text of `crates/randoc-yaml/tests/replay.json`
/// through the reader: the accept or refuse decision, the error text and the
/// metadata must be the oracle result. A refused text whose error text
/// differs is a known difference only when `randoc-yaml` alone gives the
/// same text as the reader (rows I-YAML-02 to I-YAML-10 of the yaml crate).
#[test]
fn yaml_replay_matches_the_oracle() {
    on_big_stack(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../randoc-yaml/tests/replay.json");
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let cases: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap_or_default();
        let (mut total, mut known) = (0, 0);
        let mut failures = Vec::new();
        for case in cases.iter().filter(|case| case["entrance"] == "block") {
            let yaml = case["yaml"].as_str().unwrap_or("");
            total += 1;
            // `pandoc.read` has no tab filter, unlike the program.
            let mut options = ReaderOptions::default();
            options.tab_stop = 0;
            let got = read_result(
                "commonmark+yaml_metadata_block",
                &format!("---\n{yaml}---\n"),
                options,
            );
            let same = match (&got, case.get("err").and_then(|e| e.as_str())) {
                (Err(ours), Some(theirs)) if ours == theirs => true,
                (Err(ours), Some(_)) => {
                    let alone = randoc_yaml::parse(
                        &format!("---\n{yaml}"),
                        randoc_yaml::Limits {
                            max_depth: 16384,
                            ..randoc_yaml::Limits::default()
                        },
                    )
                    .err()
                    .map(|e| e.report(randoc_yaml::Entrance::MetadataBlock { name: "", line: 1 }));
                    known += usize::from(alone.as_ref() == Some(ours));
                    alone.as_ref() == Some(ours)
                }
                (Ok(json), None) => compact_document(json) == case["ok"],
                _ => false,
            };
            if !same {
                failures.push(format!("{yaml:?}\n  oracle {case}\n  randoc {got:?}"));
            }
        }
        eprintln!(
            "yaml replay: {total} blocks, {} different, {known} known error texts",
            failures.len()
        );
        assert!(total > 10_000, "replay data missing: {total}");
        assert!(
            failures.is_empty(),
            "{} of {total} differ:\n{}",
            failures.len(),
            failures
                .iter()
                .take(30)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    });
}

fn check_suites(suites: &[&str], minimum: usize) {
    let mut counts = BTreeMap::new();
    for suite in suites {
        let outcome = run_suite(suite);
        assert!(outcome.total > minimum, "{suite}: no cases found");
        let unexpected: Vec<&String> = outcome
            .failures
            .iter()
            .filter(|id| !known_divergence(id))
            .collect();
        assert!(unexpected.is_empty(), "{suite}: {unexpected:?}");
        counts.insert(*suite, outcome.total);
    }
    assert_eq!(counts.len(), suites.len());
}

/// Pandoc 3.13 gives a table with 201 body rows and then one paragraph for this input:
/// the table ends when the reader must add more than 200,000 cells. The oracle
/// result has 201,000 cells, so it is not a stored case.
#[test]
fn a_table_ends_at_the_limit_of_added_cells() {
    on_big_stack(|| {
        let input = format!(
            "|{}\n|{}\n{}",
            "a|".repeat(1000),
            "-|".repeat(1000),
            "|x|\n".repeat(250)
        );
        let doc = read_doc("gfm", &input, 128).unwrap();
        let [randoc_ast::Block::Table(table), randoc_ast::Block::Para(_)] = doc.blocks.as_slice()
        else {
            panic!("a table and a paragraph are expected");
        };
        let rows: usize = table.bodies.iter().map(|body| body.body.len()).sum();
        assert_eq!(rows, 201);
    });
}

#[test]
fn deep_nesting_stays_within_the_default_limit() {
    let inputs = [
        ">".repeat(20000) + " a\n",
        "- ".repeat(20000) + "a\n",
        "[".repeat(20000) + &"]".repeat(20000),
        "*".repeat(20000) + "a" + &"*".repeat(20000),
        "![".repeat(20000) + &"](u)".repeat(20000),
        "::: a\n".repeat(5000),
        "[^1]\n\n[^1]: ".to_owned() + &">".repeat(20000),
    ];
    for input in inputs {
        let spec = FormatSpec::parse("commonmark_x+footnotes");
        let mut options = ReaderOptions::default();
        options.extensions = spec.extensions().unwrap();
        let Some(Reader::Text(reader)) = crate::reader("commonmark_x") else {
            panic!("no reader");
        };
        let mut context = Context::pure();
        let doc = reader
            .read(&Sources::from_text(input), &options, &mut context)
            .unwrap();
        let mut depth = 0;
        randoc_ast::walk::query_blocks(&doc, |_| depth += 1);
        assert!(depth > 0);
    }
}

fn read_doc(format: &str, input: &str, depth: usize) -> randoc_core::Result<randoc_ast::Pandoc> {
    let spec = FormatSpec::parse(format);
    let mut options = ReaderOptions::default();
    options.extensions = spec.extensions()?;
    let Some(Reader::Text(reader)) = crate::reader(&spec.name) else {
        return Err(randoc_core::Error::App("no reader".into()));
    };
    let mut context = Context::pure();
    context.limits.max_nesting_depth = depth;
    reader.read(&Sources::from_text(input), &options, &mut context)
}

#[test]
fn deep_metadata_stays_within_the_default_limit_on_a_small_stack() {
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let input = format!("---\na: {}x{}\n---\n", "[".repeat(1000), "]".repeat(1000));
            read_doc(
                "commonmark_x",
                &input,
                Context::pure().limits.max_nesting_depth,
            )
            .is_err()
        })
        .unwrap();
    assert!(handle.join().unwrap());
}

/// The shortest of five runs, in seconds.
fn shortest_run(format: &str, input: &str) -> f64 {
    (0..5)
        .map(|_| {
            let start = std::time::Instant::now();
            let _ = read_doc(format, input, 16384);
            start.elapsed().as_secs_f64()
        })
        .fold(f64::INFINITY, f64::min)
}

type Pattern = (&'static str, &'static str, fn(usize) -> String);

const PATTERNS: &[Pattern] = &[
    ("gfm", "table rows", |n| {
        format!("| a | b |\n|---|---|\n{}", "| x | y |\n".repeat(n))
    }),
    ("gfm", "underscore words", |n| "_a".repeat(n)),
    ("gfm", "dotted words", |n| "a.".repeat(n)),
    ("gfm", "plus words", |n| "a+".repeat(n)),
    ("gfm", "www after underscore", |n| "_www".repeat(n)),
    ("commonmark_x", "nested spans", |n| {
        format!("{}x{}", "[".repeat(n), "]{.a}".repeat(n))
    }),
    ("commonmark", "nested images", |n| {
        format!("{}x{}", "![".repeat(n), "](u)".repeat(n))
    }),
    ("commonmark_x", "nested divs", |n| {
        format!("{}x\n{}", "::: a\n".repeat(n), ":::\n".repeat(n))
    }),
    ("commonmark_x", "div fences", |n| {
        (0..n)
            .map(|i| format!("{} x\n", ":".repeat(3 + i % 5)))
            .collect()
    }),
    ("commonmark", "reference definitions", |n| {
        "[a]: u\n".repeat(n)
    }),
    ("commonmark", "nested emphasis", |n| {
        format!("{}a{}", "*".repeat(n), "*".repeat(n))
    }),
    ("gfm", "nested quotes", |n| format!("{} a\n", ">".repeat(n))),
    ("gfm", "nested list markers", |n| {
        format!("{}a\n", "- ".repeat(n))
    }),
    ("gfm", "two domain forms", |n| {
        format!("{}.{}-", "_www".repeat(n), "a".repeat(n))
    }),
    ("gfm", "three domain forms", |n| "_www.a-b_c.".repeat(n)),
    ("gfm", "email forms", |n| "a.b-c+d@e_f.".repeat(n)),
    ("commonmark_x", "short div fences", |n| {
        format!("{}{}", ":::: d\n".repeat(n), ":::\n".repeat(n))
    }),
    ("commonmark_x", "mixed div fences", |n| {
        format!("{}{}", "::::: d\n:::: e\n".repeat(n), ":::\n".repeat(n))
    }),
    ("commonmark", "links after open brackets", |n| {
        format!("{}{}", "[".repeat(n), "[a](u)".repeat(n))
    }),
    ("gfm", "list markers with task text", |n| {
        format!("{}x", "- ".repeat(n))
    }),
    ("gfm+wikilinks_title_after_pipe", "open wikilinks", |n| {
        "[[a|".repeat(n)
    }),
    (
        "gfm+wikilinks_title_before_pipe",
        "open wikilinks and brackets",
        |n| "[[a[[b]".repeat(n),
    ),
    ("commonmark", "comments, instructions and sections", |n| {
        "<!--<?<![CDATA[<!X".repeat(n)
    }),
    ("commonmark", "backtick runs of three lengths", |n| {
        "`a``b```c".repeat(n)
    }),
    ("gfm", "metadata list", |n| {
        format!("---\na: '{}x'\n---\n", "- ".repeat(n))
    }),
    ("gfm", "metadata emphasis", |n| {
        format!("---\na: |-\n{}---\n", "  - *a*\n".repeat(n))
    }),
    ("commonmark_x", "definition items", |n| {
        "t\n: d\n\n".repeat(n)
    }),
    ("commonmark_x", "tight definition items", |n| {
        "t\n: d\n".repeat(n)
    }),
    ("commonmark_x", "distinct attribute classes", |n| {
        format!(
            "a{{{}}}",
            (0..n).map(|i| format!(".c{i} ")).collect::<String>()
        )
    }),
    ("commonmark_x", "distinct attribute keys", |n| {
        format!(
            "a{{{}}}",
            (0..n).map(|i| format!("k{i}=v ")).collect::<String>()
        )
    }),
    ("commonmark_x", "repeated attribute groups", |n| {
        format!(
            "a{}",
            (0..n)
                .map(|i| format!("{{.c{i} k{i}=v}}"))
                .collect::<String>()
        )
    }),
    ("commonmark_x", "open attribute lines", |n| {
        "{.a\n".repeat(n)
    }),
    ("commonmark_x", "attribute group that never closes", |n| {
        format!("{{{}", ".a\n".repeat(n))
    }),
    ("commonmark_x", "div fences with open groups", |n| {
        "::: {.a\n".repeat(n)
    }),
    ("commonmark_x", "attribute groups over two lines", |n| {
        "{.a\n}\nx\n\n".repeat(n)
    }),
    ("commonmark", "runs of two lengths inside words", |n| {
        "a****b***c ".repeat(n)
    }),
    ("commonmark", "closers of each kind with no opener", |n| {
        "a* b** c*** d_ e__ f___ ".repeat(n)
    }),
    ("commonmark", "closers longer than the openers", |n| {
        "*a** _b___ ".repeat(n)
    }),
    ("commonmark", "openers longer than the closers", |n| {
        "**a* ___b_ ".repeat(n)
    }),
    ("commonmark", "nested openers, then closers", |n| {
        format!("{}b{}", "*a **a ".repeat(n), " a** a*".repeat(n))
    }),
    ("commonmark_x", "runs of four kinds", |n| {
        "a^^b^^^c~d~~e*f**g_ _h__ ".repeat(n)
    }),
    ("commonmark_x", "definitions after note definitions", |n| {
        "p\n[^t]: n\n:   d\n\n".repeat(n)
    }),
    ("commonmark_x", "definition lines after many notes", |n| {
        format!("{}{}", "[^a]: x\n".repeat(n), ": d\n".repeat(n))
    }),
    ("gfm", "table rows with an indentation of four", |n| {
        format!("| a | b |\n|---|---|\n{}", "    | x | y |\n".repeat(n))
    }),
    ("commonmark_x+smart", "single quotes after links", |n| {
        "[a](u)'s b' ".repeat(n)
    }),
    ("commonmark", "list staircase", |n| {
        let lines = (n as f64).sqrt() as usize * 8;
        (0..lines)
            .map(|i| format!("{}- a\n", "  ".repeat(i)))
            .collect()
    }),
];

/// The work count of one read.
fn steps_of(format: &str, input: &str) -> u64 {
    crate::steps::take();
    let _ = read_doc(format, input, 16384);
    crate::steps::take()
}

/// The work for 4 times the input stays below 6 times the work for the input.
#[test]
fn work_grows_linearly_with_the_input() {
    on_big_stack(|| {
        for (format, name, make) in PATTERNS {
            let small = steps_of(format, &make(2000));
            let large = steps_of(format, &make(8000));
            assert!(
                large < 6 * small.max(1),
                "{name}: {small} steps for 2000, {large} steps for 8000"
            );
        }
    });
}

/// The time for 4 times the input stays below 10 times the time for the input.
#[test]
#[ignore = "timing; run with --ignored on a machine with a low load"]
fn time_grows_linearly_with_large_inputs() {
    on_big_stack(|| {
        for (format, name, make) in PATTERNS {
            let small = shortest_run(format, &make(12500));
            let large = shortest_run(format, &make(50000));
            assert!(
                large < 10.0 * small.max(0.005),
                "{name}: {small:.3} s for 12500, {large:.3} s for 50000"
            );
        }
    });
}

#[test]
fn each_container_level_counts_against_the_inline_nesting_limit() {
    let images = format!("{}x{}", "![".repeat(20), "](u)".repeat(20));
    let nested_images = |prefix: &str| {
        let doc = read_doc("commonmark_x", &format!("{prefix}{images}\n"), 12).unwrap();
        let mut count = 0;
        randoc_ast::walk::query_inlines(&doc, |inline| {
            if matches!(inline, randoc_ast::Inline::Image(..)) {
                count += 1;
            }
        });
        count
    };
    let top = nested_images("");
    let one_quote = nested_images("> ");
    let two_quotes = nested_images("> > ");
    assert!(
        top > one_quote && one_quote > two_quotes,
        "{top} {one_quote} {two_quotes}"
    );
    assert_eq!(nested_images("- "), one_quote, "list item");
    assert_eq!(nested_images("t\n: "), two_quotes, "definition");
}

#[test]
fn depth_fallback_keeps_later_text_complete() {
    on_big_stack(|| {
        let input = format!("{}x{} [ok](url)", "[".repeat(200), "]{.a}".repeat(200));
        let doc = read_doc("commonmark_x", &input, 128).unwrap();
        let mut text = String::new();
        randoc_ast::walk::query_inlines(&doc, |inline| {
            if let randoc_ast::Inline::Str(s) = inline {
                text.push_str(s);
            }
        });
        assert!(
            text.ends_with("[ok](url)"),
            "{}",
            &text[text.len().saturating_sub(40)..]
        );
    });
}
