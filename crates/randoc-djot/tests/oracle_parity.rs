//! Parity tests for Djot.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    env, fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use randoc_ast::{Block, Pandoc, json};
use randoc_core::{Context, FormatSpec, Reader, ReaderOptions, Sources, WriterOptions};
use serde_json::Value;

fn oracle(args: &[&str], input: &str) -> (String, String) {
    let mut child = Command::new(env::var_os("RANDOC_ORACLE_PANDOC").unwrap())
        .current_dir(test_dir())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

fn test_dir() -> PathBuf {
    PathBuf::from(env::var_os("RANDOC_PANDOC_SRC").unwrap()).join("test")
}

fn read(input: &str, context: &mut Context) -> Pandoc {
    let Reader::Text(reader) = randoc_djot::reader("djot").unwrap() else {
        unreachable!()
    };
    reader
        .read(
            &Sources::from_text(input),
            &ReaderOptions::default(),
            context,
        )
        .unwrap()
}

fn write(doc: &Pandoc, options: &WriterOptions) -> String {
    let bytes = randoc_djot::writer("djot")
        .unwrap()
        .write(doc, options, &mut Context::pure())
        .unwrap()
        .into_bytes();
    String::from_utf8(bytes).unwrap()
}

fn check(expected: &Value, actual: &Value, path: &str) -> Option<String> {
    if expected == actual {
        return None;
    }
    if expected.get("t").is_some() && expected.get("t") != actual.get("t") {
        return Some(format!(
            "{path}: expected {}, got {}",
            short(&expected.to_string()),
            short(&actual.to_string())
        ));
    }
    match (expected, actual) {
        (Value::Array(a), Value::Array(b)) => {
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                if let Some(diff) = check(a, b, &format!("{path}/{index}")) {
                    return Some(diff);
                }
            }
            Some(format!(
                "{path}: lengths {} and {}.\nExpected {}\nActual {}",
                a.len(),
                b.len(),
                short(&expected.to_string()),
                short(&actual.to_string())
            ))
        }
        (Value::Object(a), Value::Object(b)) => a
            .iter()
            .find_map(|(key, a)| check(a, &b[key], &format!("{path}/{key}")))
            .or_else(|| Some(format!("{path}: different keys"))),
        _ => Some(format!(
            "{path}: expected {}, got {}",
            short(&expected.to_string()),
            short(&actual.to_string())
        )),
    }
}

fn short(value: &str) -> String {
    value.chars().take(280).collect()
}

fn ast(expected: &str, actual: &Pandoc) {
    let expected: Value = serde_json::from_str(expected).unwrap();
    let actual: Value = serde_json::from_str(&json::to_json(actual)).unwrap();
    let difference = check(&expected, &actual, "");
    assert!(difference.is_none(), "{}", difference.unwrap_or_default());
}

fn equal_text(expected: &str, actual: &str) {
    let differences = expected
        .lines()
        .zip(actual.lines())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .take(3)
        .collect::<Vec<_>>();
    assert!(differences.is_empty(), "{differences:?}");
    assert_eq!(expected.len(), actual.len());
}

#[test]
#[ignore = "Requires the Pandoc oracle and corpus"]
fn corpus() {
    let root = PathBuf::from(env::var_os("RANDOC_CORPUS").unwrap());
    let mut failures = Vec::new();
    for name in ["advanced", "cli", "corelang", "formats", "intro"] {
        let source = fs::read_to_string(root.join(format!("jaq-{name}.dj"))).unwrap();
        let (expected, stderr) = oracle(&["-f", "djot", "-t", "json"], &source);
        let mut context = Context::pure();
        let actual = read(&source, &mut context);
        let warnings = context
            .log()
            .iter()
            .map(|msg| format!("{}\n", msg.render()))
            .collect::<String>();
        if std::panic::catch_unwind(|| {
            ast(&expected, &actual);
            assert_eq!(warnings, stderr);
        })
        .is_err()
        {
            failures.push(name);
        } else {
            println!("PASS {name}");
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
#[ignore = "Requires the Pandoc oracle and fixtures"]
fn fixture_reader() {
    let input = fs::read_to_string(test_dir().join("djot-reader.djot")).unwrap();
    let native = fs::read_to_string(test_dir().join("djot-reader.native")).unwrap();
    let (expected, _) = oracle(&["-f", "native", "-t", "json"], &native);
    ast(&expected, &read(&input, &mut Context::pure()));
}

fn writer_fixture(name: &str, standalone: bool) {
    let native = fs::read_to_string(test_dir().join(format!("{name}.native"))).unwrap();
    let (json, _) = oracle(&["-f", "native", "-t", "json"], &native);
    let doc = json::from_json(&json).unwrap();
    let mut options = WriterOptions::default();
    options.columns = 80;
    if standalone {
        options.template = Some(
            randoc_templates::Template::compile(
                "default.djot",
                randoc_data::default_template("djot").unwrap(),
                128,
                &mut |_| Ok::<_, std::io::Error>(String::new()),
            )
            .unwrap(),
        );
    }
    let expected = fs::read_to_string(test_dir().join(format!("{name}.djot"))).unwrap();
    let actual = write(&doc, &options);
    equal_text(&expected, &actual);
}

#[test]
#[ignore = "Requires the Pandoc oracle and fixtures"]
fn fixture_writer() {
    writer_fixture("writer", true);
}

#[test]
#[ignore = "Requires the Pandoc oracle and fixtures"]
fn fixture_tables() {
    writer_fixture("tables", false);
}

#[test]
#[ignore = "Requires the Pandoc oracle and command fixtures"]
fn command_cases() {
    let script = r#"
import json, os, sys
sys.path.insert(0, sys.argv[1])
import suite
cases = suite.Discovery()
suite.discover_commands(sys.argv[2], cases)
names = {'10459.md','10984.md','11014.md','5793.md','mdoc-Bl-column.md'}
print(json.dumps([case.__dict__ for case in cases.cases
    if case.id.split('/')[-1].split('#')[0] in names]))
"#;
    let tools = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/pandoc-testsuite");
    let output = Command::new("python3")
        .args(["-B", "-c", script])
        .arg(tools)
        .arg(test_dir())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cases: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(cases.len(), 7);
    let mut failed = Vec::new();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let command = case["command"].as_str().unwrap();
        let input = case["stdin"].as_str().unwrap();
        let expected = case["expected"].as_str().unwrap();
        let args = command.split_whitespace().skip(1).collect::<Vec<_>>();
        let (oracle_output, stderr) = oracle(&args, input);
        assert!(stderr.is_empty());
        assert_eq!(oracle_output, expected);
        if std::panic::catch_unwind(|| {
            let from = args[args.iter().position(|arg| *arg == "-f").unwrap() + 1];
            let to = args[args.iter().position(|arg| *arg == "-t").unwrap() + 1];
            if to == "djot" {
                let doc = json::from_json(&oracle(&["-f", from, "-t", "json"], input).0).unwrap();
                let actual = write(&doc, &WriterOptions::default());
                equal_text(expected, &actual);
            } else {
                let mut doc = read(input, &mut Context::pure());
                let expected_json = if args.contains(&"--shift-heading-level-by=-1") {
                    if let [Block::Div(_, children)] = doc.blocks.as_mut_slice()
                        && let [Block::Header(1, _, title)] = children.as_mut_slice()
                    {
                        doc.meta.insert(
                            "title",
                            randoc_ast::MetaValue::MetaInlines(std::mem::take(title)),
                        );
                        children.clear();
                    }
                    oracle(&["-f", "native", "-t", "json"], expected).0
                } else {
                    oracle(&["-f", from, "-t", "json"], input).0
                };
                ast(&expected_json, &doc);
                if to == "html" {
                    let (rendered, _) = oracle(&["-f", "json", "-t", "html"], &json::to_json(&doc));
                    equal_text(expected, &rendered);
                }
            }
        })
        .is_err()
        {
            failed.push(id.to_string());
        } else {
            println!("PASS {id}");
        }
    }
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
#[ignore = "Requires the Pandoc oracle"]
fn extension_flags() {
    for (extension, input) in [
        ("sourcepos", "日\n本\n"),
        ("sourcepos", "é *é*  \n日\n"),
        ("sourcepos", "# Heading\n\nA *bold* word.\n"),
        ("sourcepos", "A\n\n- x\n- y\n\nB\n"),
        ("sourcepos", "| a | b |\n|---|---|\n| c | d |\n"),
        ("east_asian_line_breaks", "日\n本\n"),
    ] {
        let format = format!("djot+{extension}");
        let spec = FormatSpec::parse(&format);
        let mut options = ReaderOptions::default();
        options.extensions = spec.extensions().unwrap();
        let Reader::Text(reader) = randoc_djot::reader("djot").unwrap() else {
            unreachable!()
        };
        let actual = reader
            .read(&Sources::from_text(input), &options, &mut Context::pure())
            .unwrap();
        ast(&oracle(&["-f", &format, "-t", "json"], input).0, &actual);
    }
}
