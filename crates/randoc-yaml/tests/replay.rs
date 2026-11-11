//! Replays YAML texts against the results of Pandoc 3.13.
//!
//! `tools/replay.py` makes the data file. Each case is a YAML text, its
//! entrance (a metadata block or a metadata file) and the oracle result: the
//! metadata or the exact error text. The metadata strings went through the
//! Markdown reader, so the test compares only the shape of that metadata.
//! Each accepted text with a value has two exact fields: `literal`, the text
//! read as the `variables` of a defaults file, where Pandoc keeps each string
//! literal, and `strings`, the text read as its `metadata`, which keeps the
//! line breaks at the end. A case without them fails.
//! Set `RANDOC_YAML_REPLAY` to replay another data file.

#![allow(
    clippy::panic,
    reason = "the helpers of the test stop on a broken data file"
)]

use randoc_yaml::{Entrance, Limits, Node, Value, parse};
use serde_json::Value as Json;

const FILE_NAME: &str = "m.yaml";

/// Texts whose result differs from the oracle, each with its row in
/// `docs/ISSUES.md`.
const KNOWN: &[(&str, &str, &str)] = &[
    ("block", "x: !<!t>: y\n", "I-YAML-03"),
    ("block", "a:\n &x\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n  &x\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n &x\n !u\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n &x\n  !u\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n &x\n   !u\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n  &x\n !u\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n  &x\n  !u\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n  &x\n   !u\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n !t\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n  !t\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n !t\n &y\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n !t\n  &y\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n !t\n   &y\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n  !t\n &y\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n  !t\n  &y\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n  !t\n   &y\n{c: d}\n", "I-YAML-04"),
    ("block", "a:\n ? q\n  ? k\n  : v\n", "I-YAML-04"),
    ("block", "s:\n- ? q\n   ? k\n   : v\n", "I-YAML-04"),
    ("block", "s:\n- ? q\n  - s\n", "I-YAML-04"),
    ("block", "a:\n  ? q\n   ? k\n   : v\n", "I-YAML-04"),
    ("block", "a:\n    ? q\n     ? k\n     : v\n", "I-YAML-04"),
    ("block", "{a: b, a: c}: v\n", "I-YAML-02"),
    ("block", "[*b]: v\n", "I-YAML-02"),
    ("block", "a: {k: \na: k: \n  {a: }}\n", "I-YAML-04"),
    ("block", ": y\n: y\n", "I-YAML-02"),
    ("block", "\"\":>\n", "I-YAML-04"),
    ("block", "[a, b]: v\n[a, b]: v\n\n", "I-YAML-02"),
    ("block", "a:\n  }\"null\"\"\n", "I-YAML-04"),
    ("block", ": a}\n: a}\n: a}\n", "I-YAML-02"),
    ("block", "[[]]: ]v\n", "I-YAML-02"),
    ("block", "a:\n  \"\\n\"!%\n", "I-YAML-04"),
    (
        "block",
        ": {k: !!in*t \"12\"}\n: {k: !!in*t \"12\"}\n",
        "I-YAML-02",
    ),
    ("block", "z: &z q\nb:\n  *z:\n  c: d\n", "I-YAML-11"),
    ("block", "z: &a-1 q\nb: [*a-1:}, x]\n", "I-YAML-11"),
    ("block", "- &z q\n- *z:\n  - li\n", "I-YAML-11"),
    ("block", "k: &a:b [1, 2]\n*a:b:{v: w}\nc: d\n", "I-YAML-11"),
    ("block", "a: [&a\n !t x, !u\n\t!v y]\n", "I-YAML-10"),
    ("block", "- [!t &a\nx: v]\n", "I-YAML-10"),
    ("block", "z: &z q\n? &a\n  *z\n", "I-YAML-13"),
    ("block", "z: &z q\n? k\n: !t\n  *z\n", "I-YAML-13"),
    ("block", "z: &z q\nb:\n  *z: \n  c: d\n", "I-YAML-11"),
    ("block", "z: &z q\nb:\n  *z: #c\n  c: d\n", "I-YAML-11"),
    ("block", "z: &z q\nb:\n  *z:\t\n  c: d\n", "I-YAML-11"),
    ("file", "--- \n---\n", "I-YAML-04"),
    ("file", "- \"a`\\\"b\"*\n", "I-YAML-04"),
    (
        "file",
        "a: {k: {a: {b: c}}\na: {k: {a: {b: c}}\na: {k: {a: {b: c}}\n",
        "I-YAML-04",
    ),
    ("file", "a: [1e-1001]\n!\n", "I-YAML-04"),
    ("file", "ax:b\n--- \nc: [\n|\n", "I-YAML-04"),
];

/// The limits of the `randoc` program.
const LIMITS: Limits = Limits {
    max_depth: 16384,
    alias_budget: 100_000,
};

/// `true` when the YAML value of `a` in `text` is the literal oracle string.
fn exact(text: &str, oracle: &str) -> bool {
    let node = parse(text, LIMITS).unwrap_or_else(|e| panic!("{text}: {e}"));
    literal_matches(&serde_json::json!({ "a": oracle }), &node)
}

#[test]
fn a_changed_string_fails_the_exact_comparison() {
    // Each pair: a text and its oracle string. A change of one character of
    // each class fails: a Markdown mark, an apostrophe, an entity, a tag, a
    // letter, a space and a line break.
    let pairs = [
        ("a: my_file.bib\n", "my_file.bib"),
        ("a: a*b\n", "a*b"),
        ("a: \\usepackage{x}\n", "\\usepackage{x}"),
        ("a: \"It's a test\"\n", "It's a test"),
        ("a: \"It's a-test\"\n", "It's a-test"),
        ("a: x &amp; z\n", "x &amp; z"),
        ("a: <br>\n", "<br>"),
        (
            "a: \"`c` [l](u) # h ~x~ ^y^ _u_ | > !\"\n",
            "`c` [l](u) # h ~x~ ^y^ _u_ | > !",
        ),
        ("a: \" x \"\n", " x "),
        ("a: \"x\\n\"\n", "x\n"),
        ("a: \"x\\n\\n\"\n", "x\n"),
        ("a: \"\\n\"\n", ""),
    ];
    for (text, oracle) in pairs {
        assert!(exact(text, oracle), "{text:?} must equal {oracle:?}");
        for (i, c) in oracle.char_indices() {
            let mut lost = oracle.to_string();
            lost.remove(i);
            assert!(
                !exact(text, &lost),
                "{text:?} without {c:?} at {i} must fail"
            );
        }
        assert!(
            !exact(text, &format!("{oracle}y")),
            "{text:?} with an added letter must fail"
        );
    }
    assert!(!exact("a: myfile.bib\n", "my_file.bib"));
    assert!(!exact("a: ab\n", "a*b"));
    assert!(!exact("a: \"Its a test\"\n", "It's a test"));
    assert!(!exact("a: xy & z\n", "x &amp; z"));
    assert!(!exact("a: x\n", "&amp;"));
}

/// A replay case of the metadata block entrance with its stored oracle data.
fn stored(text: &str, meta: &Json, literal: &Json, strings: &Json) -> Json {
    serde_json::json!({"entrance": "block", "yaml": text, "ok": {"meta": meta, "blocks": 0}, "literal": literal, "strings": strings})
}

#[test]
fn a_changed_value_fails_against_stored_fixtures() {
    // Each fixture with its oracle data passes; the changed text fails.
    let string = |c: &str| serde_json::json!({"a": {"t": "MetaString", "c": c}});
    let fixtures = [
        (
            "a: \"\\n\"\n",
            "a: \"\"\n",
            serde_json::json!({"a": {"k": []}}),
            serde_json::json!({"a": ""}),
            string("\n\n\n"),
        ),
        (
            "a: \"x\\n\"\n",
            "a: \"x\\n\\n\"\n",
            serde_json::json!({"a": {"k": ["x"]}}),
            serde_json::json!({"a": "x\n"}),
            string("x\n\n\n"),
        ),
        (
            "a: 1.50\n",
            "a: 1.5001\n",
            serde_json::json!({"a": {"i": "1.5"}}),
            serde_json::json!({"a": "1.5"}),
            string("1.5"),
        ),
        (
            "a: 0x1F\n",
            "a: 30\n",
            serde_json::json!({"a": {"i": "31"}}),
            serde_json::json!({"a": "31"}),
            string("31"),
        ),
        (
            "a: true\n",
            "a: \"true\"\n",
            serde_json::json!({"a": {"b": true}}),
            serde_json::json!({"a": true}),
            serde_json::json!({"a": {"t": "MetaBool", "c": true}}),
        ),
    ];
    for (text, changed, meta, literal, strings) in fixtures {
        assert_eq!(
            check(&stored(text, &meta, &literal, &strings)),
            None,
            "{text:?}"
        );
        assert!(
            check(&stored(changed, &meta, &literal, &strings)).is_some(),
            "{changed:?} must fail"
        );
    }
}

/// Number texts of variables that differ from the metadata text: each one
/// passes, and each changed digit fails.
#[test]
fn a_changed_number_fails_the_exact_comparison() {
    let pairs = [
        ("12345678901234567890", "1.234567890123456789e19"),
        ("0.12345678901234568", "0.1234567890123456789"),
        ("0.0", "1.0e-1000"),
        ("1.5", "1.5"),
        ("-9223372036854775809", "-9.223372036854775809e18"),
    ];
    for (variable, ours) in pairs {
        assert!(same_number(variable, ours), "{variable} = {ours}");
        for (i, c) in variable.char_indices().filter(|(_, c)| c.is_ascii_digit()) {
            let changed = format!(
                "{}{}{}",
                &variable[..i],
                if c == '9' { '8' } else { '9' },
                &variable[i + 1..]
            );
            assert!(!same_number(&changed, ours), "{changed} != {ours}");
            let mut lost = variable.to_string();
            lost.remove(i);
            if decimal(&lost) != decimal(variable) {
                assert!(!same_number(&lost, ours), "{lost} != {ours}");
            }
        }
    }
}

#[test]
fn a_fixture_without_exact_data_fails() {
    // Codex review round 7: a metadata file with an end marker had no exact
    // data, and its changed text passed.
    let case = |yaml: &str| serde_json::json!({"entrance": "file", "yaml": yaml, "ok": {"meta": {"a": {"i": "b"}}, "blocks": 0}});
    assert!(check(&case("a: b\n...\n")).is_some());
    assert!(check(&case("a: b*\n...\n")).is_some());
}

/// The number of changed texts whose result differs from the oracle
/// (I-YAML-12).
const CHANGED_PARSER_DIFFERENCES: usize = 304;

/// The oracle fields of a case.
fn reference(case: &Json) -> [&Json; 4] {
    ["ok", "err", "literal", "strings"].map(|k| case.get(k).unwrap_or(&Json::Null))
}

/// Each changed text of `tests/changed.json` against the oracle data of its
/// original: the comparison accepts it exactly when the oracle results of
/// the two texts are equal.
#[test]
fn changed_fixtures_pass_only_with_an_equal_oracle_result() {
    let read = |path: &str| -> Vec<Json> {
        let data = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        serde_json::from_str(&data).unwrap_or_else(|e| panic!("{path}: {e}"))
    };
    let originals = read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/replay.json"));
    let originals: std::collections::HashMap<(&str, &str), &Json> = originals
        .iter()
        .map(|c| {
            (
                (
                    c["entrance"].as_str().unwrap_or(""),
                    c["yaml"].as_str().unwrap_or(""),
                ),
                c,
            )
        })
        .collect();
    let changed = read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/changed.json"));
    let [
        mut equal,
        mut wrong_accepts,
        mut wrong_rejects,
        mut parser_differs,
    ] = [0usize; 4];
    for case in &changed {
        let entrance = case["entrance"].as_str().unwrap_or("");
        let from = case["from"].as_str().unwrap_or("");
        let original = originals
            .get(&(entrance, from))
            .unwrap_or_else(|| panic!("no original {entrance} {from:?}"));
        if let Some(problem) = check(case) {
            parser_differs += 1;
            eprintln!(
                "--- the parser differs: {entrance} {:?}\n{problem}\n",
                case["yaml"]
            );
            continue;
        }
        let mut crossed = (*original).clone();
        crossed["yaml"] = case["yaml"].clone();
        let same = reference(case) == reference(original);
        equal += usize::from(same);
        match (same, check(&crossed)) {
            (true, Some(problem)) => {
                wrong_rejects += 1;
                eprintln!(
                    "--- wrong reject: {entrance} {:?}\n{problem}\n",
                    case["yaml"]
                );
            }
            (false, None) => {
                wrong_accepts += 1;
                eprintln!(
                    "--- wrong accept: {entrance} {:?} from {from:?}\n",
                    case["yaml"]
                );
            }
            _ => {}
        }
    }
    eprintln!(
        "changed: {} texts, {equal} with the oracle result of the original, {wrong_accepts} wrong accepts, {wrong_rejects} wrong rejects, {parser_differs} parser differences",
        changed.len()
    );
    assert!(changed.len() >= 5000);
    assert_eq!([wrong_accepts, wrong_rejects], [0, 0]);
    // Changed texts whose own result differs from the oracle (I-YAML-12)
    // cannot test the comparison.
    assert_eq!(parser_differs, CHANGED_PARSER_DIFFERENCES);
}

#[test]
fn replay_matches_the_oracle() {
    let worker = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(run)
        .unwrap_or_else(|e| panic!("cannot start the replay thread: {e}"));
    let failures = worker
        .join()
        .unwrap_or_else(|_| panic!("the replay thread panicked"));
    assert!(failures == 0, "{failures} cases differ from the oracle");
}

fn run() -> usize {
    let path = std::env::var("RANDOC_YAML_REPLAY")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/tests/replay.json").into());
    let data = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let cases: Vec<Json> = serde_json::from_str(&data).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut failures = 0;
    let mut known = 0;
    let mut exit_class = 0;
    let limit: usize = std::env::var("RANDOC_YAML_SHOW")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    let mut by_entrance = [0usize; 2];
    // Metadata blocks whose duplicate-key error text differs (I-YAML-09).
    let known_keys: std::collections::HashSet<String> = include_str!("known_keys.txt")
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    // Metadata blocks with properties in a flow collection whose error text
    // differs (I-YAML-10).
    let known_flow: std::collections::HashSet<String> = include_str!("known_flow.txt")
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    for case in &cases {
        if case.get("timeout").is_some() {
            continue;
        }
        let entrance = case["entrance"].as_str().unwrap_or("");
        let text = case["yaml"].as_str().unwrap_or("");
        let issue = KNOWN
            .iter()
            .find(|(_, t, _)| *t == text)
            .map(|(_, _, issue)| *issue)
            .or_else(|| (entrance == "block" && known_keys.contains(text)).then_some("I-YAML-09"))
            .or_else(|| (entrance == "block" && known_flow.contains(text)).then_some("I-YAML-10"));
        if let Some(issue) = issue {
            known += 1;
            if check(case).is_none() {
                failures += 1;
                eprintln!(
                    "--- {entrance} {text:?}\nmatches the oracle now: remove it from KNOWN ({issue})\n"
                );
            }
            continue;
        }
        if let Some(problem) = check(case) {
            failures += 1;
            // A text that one program accepts and the other refuses.
            let refused = problem.starts_with("expected {")
                && (problem.contains("\ngot\nError") || problem.contains("got the error"));
            if problem.starts_with("expected the error") || refused {
                exit_class += 1;
                eprintln!(
                    "exit class differs: {} {:?}",
                    case["entrance"].as_str().unwrap_or("?"),
                    shorten(case["yaml"].as_str().unwrap_or(""))
                );
            }
            let file = case["entrance"] == "file";
            by_entrance[usize::from(file)] += 1;
            if failures <= limit {
                eprintln!(
                    "--- {} {:?}\n{problem}\n",
                    case["entrance"].as_str().unwrap_or("?"),
                    shorten(case["yaml"].as_str().unwrap_or(""))
                );
            }
        }
    }
    eprintln!(
        "replay: {} cases, {failures} differ ({} block, {} file), {exit_class} with another exit class, {known} known divergences",
        cases.len(),
        by_entrance[0],
        by_entrance[1]
    );
    failures
}

fn shorten(text: &str) -> String {
    if text.chars().count() > 200 {
        let head: String = text.chars().take(200).collect();
        format!("{head}...")
    } else {
        text.to_string()
    }
}

/// Returns a description of the difference, or `None` when the result is the
/// result of the oracle.
fn check(case: &Json) -> Option<String> {
    let text = case["yaml"].as_str().unwrap_or("");
    let file = case["entrance"] == "file";
    let entrance = if file {
        Entrance::MetadataFile { name: FILE_NAME }
    } else {
        Entrance::MetadataBlock { name: "", line: 1 }
    };
    let block_text = format!("---\n{text}");
    let ours = parse(if file { text } else { &block_text }, LIMITS);
    if !file && text.split('\n').next().is_some_and(|l| l.trim().is_empty()) {
        // The caller rule: a metadata block cannot start with a blank line.
        let blocks = case["ok"]["blocks"].as_u64().unwrap_or(0);
        return (blocks == 0).then(|| "expected a text that is not a block".to_string());
    }
    if let Some(expected) = case.get("err").and_then(Json::as_str) {
        let got = match &ours {
            Err(error) => error.report(entrance),
            Ok(node) if file && !is_object(node) => not_an_object(),
            Ok(node) => return Some(format!("expected the error\n{expected}\ngot {node:?}")),
        };
        return (got != expected).then(|| format!("expected\n{expected}\ngot\n{got}"));
    }
    let ok = &case["ok"];
    let node = match ours {
        Ok(node) => node,
        Err(error) => return Some(format!("expected {ok}\ngot\n{}", error.report(entrance))),
    };
    let blocks = ok["blocks"].as_u64().unwrap_or(0);
    let meta = &ok["meta"];
    let empty = meta.as_object().is_some_and(serde_json::Map::is_empty);
    if !file
        && text
            .split('\n')
            .any(|l| l.starts_with("---") || l.starts_with("..."))
    {
        // The caller rule: such a line can end the metadata block.
        return None;
    }
    if !file && text.split('\n').next().is_some_and(|l| l.trim().is_empty()) {
        // The caller rule: a metadata block cannot start with a blank line.
        return (!empty || blocks == 0)
            .then(|| format!("expected {ok}, a text that is not a block"));
    }
    if matches!(node.value(), Ok(Value::Null)) {
        return (!empty || blocks > 0).then(|| format!("expected {ok}\ngot null"));
    }
    if !is_object(&node) {
        if file {
            return Some(format!("expected {ok}\ngot the error for a non-object"));
        }
        // A text that is not a mapping is not metadata: the block is read as
        // Markdown.
        return (!empty || blocks == 0)
            .then(|| format!("expected {ok}\ngot the non-mapping {node:?}"));
    }
    let wrapped = serde_json::json!({ "m": meta });
    if !shape_matches(&wrapped, &node) {
        return Some(format!("expected {ok}\ngot {node:?}"));
    }
    let (Some(literal), Some(strings)) = (case.get("literal"), case.get("strings")) else {
        return Some("the case has no exact data: run `tools/replay.py --exact`".into());
    };
    if !literal_matches(literal, &node) {
        return Some(format!(
            "expected the literal value {literal}\ngot {node:?}"
        ));
    }
    (!strings_match(&serde_json::json!({"t": "MetaMap", "c": strings}), &node))
        .then(|| format!("expected the metadata strings {strings}\ngot {node:?}"))
}

/// Compares a value with the oracle value of the `variables` of a defaults
/// file: each key and string must be equal, and each number must have the
/// value of the oracle text. The caller also compares the metadata value,
/// which keeps the type and the line breaks that a variable loses.
fn literal_matches(oracle: &Json, node: &Node) -> bool {
    match node.value() {
        Ok(Value::Mapping(entries)) => oracle.as_object().is_some_and(|map| {
            map.len() == entries.len()
                && entries.iter().all(|(key, value)| {
                    key.scalar_text()
                        .and_then(|k| map.get(k))
                        .is_some_and(|o| literal_matches(o, value))
                })
        }),
        Ok(Value::Sequence(items)) => oracle.as_array().is_some_and(|list| {
            list.len() == items.len() && list.iter().zip(items).all(|(o, n)| literal_matches(o, n))
        }),
        Ok(Value::Bool(b)) => oracle.as_bool() == Some(b),
        Ok(Value::Null) => oracle.is_null(),
        Ok(Value::Integer(i)) => oracle
            .as_str()
            .is_some_and(|o| same_number(o, &i.to_string())),
        Ok(Value::Float(text)) => oracle.as_str().is_some_and(|o| same_number(o, &text)),
        Ok(Value::String(text)) => oracle.as_str() == Some(variable_text(text)),
        Err(_) => false,
    }
}

/// `true` when two number texts have the same value. A variable can print a
/// float as the shortest form of the nearest `f64` (`0.0` below its range);
/// other numbers keep each digit.
fn same_number(variable: &str, ours: &str) -> bool {
    let Some(v) = decimal(variable) else {
        return variable == ours;
    };
    if decimal(ours).as_ref() == Some(&v) {
        return true;
    }
    variable.contains('.')
        && ours
            .parse::<f64>()
            .is_ok_and(|f| decimal(&format!("{f:?}")) == Some(v))
}

/// The sign, the significant digits and the exponent of a decimal number
/// text, so that `1.50`, `15e-1` and `0.15e1` are equal. Zero has no sign.
fn decimal(text: &str) -> Option<(bool, String, i64)> {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (mantissa, exponent) = match rest.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse::<i64>().ok()?),
        None => (rest, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let point = i64::try_from(whole.len()).ok()? + exponent;
    let leading = digits.len() - digits.trim_start_matches('0').len();
    let significant = digits.trim_matches('0').to_string();
    if significant.is_empty() {
        return Some((false, String::new(), 0));
    }
    Some((negative, significant, point - i64::try_from(leading).ok()?))
}

/// Compares each string with the oracle value of the `metadata` of a defaults
/// file, which keeps each line break at the end of a string.
fn strings_match(oracle: &Json, node: &Node) -> bool {
    let text = |t: &str| oracle["t"] == "MetaString" && oracle["c"].as_str() == Some(t);
    match node.value() {
        Ok(Value::Mapping(entries)) => oracle["c"].as_object().is_some_and(|map| {
            map.len() == entries.len()
                && entries.iter().all(|(key, value)| {
                    key.scalar_text()
                        .and_then(|k| map.get(k))
                        .is_some_and(|o| strings_match(o, value))
                })
        }),
        Ok(Value::Sequence(items)) => oracle["c"].as_array().is_some_and(|list| {
            list.len() == items.len() && list.iter().zip(items).all(|(o, n)| strings_match(o, n))
        }),
        Ok(Value::String(s)) => text(&metadata_text(s)),
        Ok(Value::Bool(b)) => oracle["t"] == "MetaBool" && oracle["c"].as_bool() == Some(b),
        Ok(Value::Null) => text(""),
        Ok(Value::Integer(i)) => {
            oracle["t"] == "MetaString"
                && oracle["c"]
                    .as_str()
                    .is_some_and(|o| same_number(o, &i.to_string()))
        }
        Ok(Value::Float(f)) => {
            oracle["t"] == "MetaString" && oracle["c"].as_str().is_some_and(|o| same_number(o, &f))
        }
        Err(_) => false,
    }
}

/// The text of a string as metadata of a defaults file: `\r\n` becomes
/// `\n`, and each other `\r` goes. A text whose last line has only spaces
/// and tabs gets two more line breaks; another text loses its leading spaces,
/// tabs and line breaks.
fn metadata_text(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let last = text.rsplit('\n').next().unwrap_or("");
    let blank_end = text.contains('\n') && last.trim_matches([' ', '\t']).is_empty();
    let text = text.replace('\r', "");
    if blank_end {
        format!("{text}\n\n")
    } else {
        text.trim_start_matches([' ', '\t', '\n']).to_string()
    }
}

/// The text of a string as a template variable: when the string ends with an
/// empty line, or is one line break, Pandoc removes the last line break.
fn variable_text(text: &str) -> &str {
    match text.strip_suffix('\n') {
        Some(rest) if rest.is_empty() || rest.ends_with('\n') => rest,
        _ => text,
    }
}

fn is_object(node: &Node) -> bool {
    matches!(node.value(), Ok(Value::Mapping(_) | Value::Null))
}

fn not_an_object() -> String {
    format!("Error at \"{FILE_NAME}\" (line 1, column 1):\nexpected YAML object")
}

/// Compares the shape of the metadata of the oracle: the keys, the lists,
/// the booleans and the place of each scalar. The Markdown reader changed the
/// strings, so `literal_matches` and `strings_match` compare them.
fn shape_matches(oracle: &Json, node: &Node) -> bool {
    match node.value() {
        Ok(Value::Mapping(entries)) => {
            let Some(map) = oracle.get("m").and_then(Json::as_object) else {
                return false;
            };
            map.len() == entries.len()
                && entries.iter().all(|(key, value)| {
                    key.scalar_text()
                        .and_then(|k| map.get(k))
                        .is_some_and(|o| shape_matches(o, value))
                })
        }
        Ok(Value::Sequence(items)) => {
            let Some(list) = oracle.get("l").and_then(Json::as_array) else {
                return false;
            };
            list.len() == items.len() && list.iter().zip(items).all(|(o, n)| shape_matches(o, n))
        }
        Ok(Value::Bool(b)) => oracle.get("b").and_then(Json::as_bool) == Some(b),
        Ok(Value::Null) => oracle.get("s").and_then(Json::as_str) == Some(""),
        Ok(Value::Integer(_) | Value::Float(_) | Value::String(_)) => ["i", "i?", "k", "k?", "s"]
            .iter()
            .any(|k| oracle.get(*k).is_some()),
        Err(_) => false,
    }
}

/// The worst inputs that the parser knows, each of about `size` bytes.
fn worst_inputs(size: usize) -> Vec<(&'static str, String)> {
    let deep = (size / 8).min(16_000);
    let pad = size.saturating_sub(2 * deep + 8);
    vec![
        ("long scalar", format!("a: {}\n", "x".repeat(size))),
        (
            "long quoted",
            format!("a: \"{}\"\n", "\\u00e9 ".repeat(size / 7)),
        ),
        (
            "long flow line",
            format!("a: [{}x]\n", "x, ".repeat(size / 3)),
        ),
        (
            "many keys",
            (0..size / 12).map(|i| format!("k{i:08}: v\n")).collect(),
        ),
        (
            "many aliases",
            format!("a: &a x\nb: [{}*a]\n", "*a, ".repeat(size / 4)),
        ),
        ("many documents", "a: 1\n---\n".repeat(size / 9)),
        ("plain lines", format!("a: x\n{}", "  x\n".repeat(size / 4))),
        (
            "deep nesting",
            format!(
                "a: {}{}\nb: {}\n",
                "[".repeat(deep),
                "]".repeat(deep),
                "x".repeat(pad)
            ),
        ),
        (
            "block scalar",
            format!("a: |\n{}", "  line\n".repeat(size / 7)),
        ),
    ]
}

/// The smallest time of five parses, in seconds.
fn min_time(text: &str) -> f64 {
    (0..5)
        .map(|_| {
            let start = std::time::Instant::now();
            let _ = parse(text, LIMITS);
            start.elapsed().as_secs_f64()
        })
        .fold(f64::INFINITY, f64::min)
}

/// Runs `check` on a thread with the stack of the `randoc` program.
fn on_big_stack(check: impl FnOnce() + Send + 'static) {
    let worker = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(check)
        .unwrap_or_else(|e| panic!("{e}"));
    worker
        .join()
        .unwrap_or_else(|_| panic!("the time test failed"));
}

/// Run with `cargo test --release -p randoc-yaml --test replay -- --ignored --nocapture`.
#[test]
#[ignore = "a benchmark of 1 MB and 2 MB inputs"]
fn worst_inputs_of_one_and_two_megabytes() {
    on_big_stack(|| {
        for ((name, a), (_, b)) in worst_inputs(1 << 20).iter().zip(&worst_inputs(2 << 20)) {
            let (t1, t2) = (min_time(a), min_time(b));
            eprintln!(
                "{name}: 1 MB {t1:.3} s, 2 MB {t2:.3} s, ratio {:.2}",
                t2 / t1.max(1e-9)
            );
        }
    });
}

/// Run with `cargo test -p randoc-yaml --test replay radix -- --ignored --nocapture`.
#[test]
#[ignore = "a time measurement"]
fn radix_numbers_near_the_limit() {
    let one = format!("a: 0x{}\n", "f".repeat(19_999));
    let many: String = (0..49)
        .map(|i| format!("k{i}: 0x{}\n", "f".repeat(19_999)))
        .collect();
    for (name, text) in [
        ("one number of 19999 digits", one),
        ("49 numbers, 957 KB", many),
    ] {
        let start = std::time::Instant::now();
        let ok = parse(&text, LIMITS).is_ok();
        eprintln!(
            "{name}: {} bytes, {:.3} s, {ok}",
            text.len(),
            start.elapsed().as_secs_f64()
        );
    }
}
