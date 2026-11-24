//! Parity tests for Jupyter notebooks.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    env, fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use randoc_ast::{Pandoc, json};
use randoc_core::{
    Context, IpynbOutput, Reader, ReaderOptions, Sources, WrapOption, WriterOptions,
    default_extensions,
};
use serde_json::Value;

fn oracle(args: &[&str], input: &str) -> String {
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
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn read(input: &str, mode: IpynbOutput, output_format: &str) -> Pandoc {
    let Reader::Text(reader) = randoc_ipynb::reader("ipynb").unwrap() else {
        unreachable!()
    };
    let mut options = ReaderOptions::default();
    options.extensions = default_extensions("ipynb");
    options.ipynb_output = mode;
    options.output_format = Some(output_format.into());
    reader
        .read(&Sources::from_text(input), &options, &mut Context::pure())
        .unwrap()
}

fn check(expected: &Value, actual: &Value, path: &str) -> Option<String> {
    if expected == actual {
        return None;
    }
    match (expected, actual) {
        (Value::Array(a), Value::Array(b)) => {
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                if let Some(diff) = check(a, b, &format!("{path}/{index}")) {
                    return Some(diff);
                }
            }
            Some(format!(
                "{path}: lengths {} and {}\n{expected}\n{actual}",
                a.len(),
                b.len()
            ))
        }
        (Value::Object(a), Value::Object(b)) => a
            .iter()
            .find_map(|(key, a)| check(a, &b[key], &format!("{path}/{key}")))
            .or_else(|| Some(format!("{path}: different keys"))),
        _ => Some(format!("{path}: expected {expected}, got {actual}")),
    }
}

fn ast(input: &str, mode: IpynbOutput, output_format: &str, expected: &str) {
    let doc = read(input, mode, output_format);
    let actual: Value = serde_json::from_str(&json::to_json(&doc)).unwrap();
    let mut expected: Value = serde_json::from_str(expected).unwrap();
    if mode == IpynbOutput::All
        && output_format == "native"
        && expected["meta"] == serde_json::json!({})
    {
        expected["meta"] = actual["meta"].clone();
    }
    assert!(
        check(&expected, &actual, "").is_none(),
        "{:?}",
        check(&expected, &actual, "")
    );
}

fn test_dir() -> PathBuf {
    PathBuf::from(env::var_os("RANDOC_PANDOC_SRC").unwrap()).join("test")
}

#[test]
#[ignore = "Requires the Pandoc oracle"]
fn markdown_cells_use_the_notebook_extensions() {
    let sources = [
        "see www.example.org and http://x.org",
        "a\n- b",
        "x<span>y</span>",
        "Term\n: def",
        "H~2~O and 2^10^",
        "[x]{.c}",
        "see <b>x</b>",
    ];
    let cells: Vec<_> = sources
        .into_iter()
        .map(|source| serde_json::json!({"cell_type":"markdown","metadata":{},"source":source}))
        .collect();
    let input = serde_json::json!({"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":cells})
        .to_string();

    for format in ["ipynb", "ipynb-raw_html"] {
        let expected: Value =
            serde_json::from_str(&oracle(&["-f", format, "-t", "json"], &input)).unwrap();
        let mut options = ReaderOptions::default();
        options.extensions = randoc_core::FormatSpec::parse(format).extensions().unwrap();
        let Reader::Text(reader) = randoc_ipynb::reader("ipynb").unwrap() else {
            unreachable!()
        };
        let doc = reader
            .read(&Sources::from_text(&input), &options, &mut Context::pure())
            .unwrap();
        let actual = serde_json::from_str(&json::to_json(&doc)).unwrap();
        assert_eq!(check(&expected, &actual, ""), None, "{format}");
    }
}

#[test]
#[ignore = "Requires the Pandoc oracle and corpus"]
fn corpus() {
    let root = PathBuf::from(env::var_os("RANDOC_CORPUS").unwrap());
    let mut failed = Vec::new();
    for name in [
        "connecting-with-the-qt-console",
        "custom-keyboard-shortcuts",
        "importing-notebooks",
        "notebook-basics",
        "running-code",
        "typesetting-equations",
        "what-is-the-jupyter-notebook",
        "working-with-markdown-cells",
    ] {
        let input = fs::read_to_string(root.join(format!("jupyter-{name}.ipynb"))).unwrap();
        let expected = oracle(&["-f", "ipynb", "-t", "json"], &input);
        if std::panic::catch_unwind(|| ast(&input, IpynbOutput::Best, "json", &expected)).is_err() {
            failed.push(name);
        } else {
            println!("PASS {name}");
        }
    }
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
#[ignore = "Requires the Pandoc oracle and fixtures"]
fn simple_reader() {
    let dir = test_dir().join("ipynb");
    let input = fs::read_to_string(dir.join("simple.ipynb")).unwrap();
    let native = fs::read_to_string(dir.join("simple.out.native")).unwrap();
    ast(
        &input,
        IpynbOutput::Best,
        "native",
        &oracle(&["-f", "native", "-t", "json"], &native),
    );
}

#[test]
#[ignore = "Requires the Pandoc oracle and fixtures"]
fn mime_reader() {
    let dir = test_dir().join("ipynb");
    let input = fs::read_to_string(dir.join("mime.ipynb")).unwrap();
    let native = fs::read_to_string(dir.join("mime.native")).unwrap();
    ast(
        &input,
        IpynbOutput::All,
        "native",
        &oracle(&["-f", "native", "-t", "json"], &native),
    );
}

#[test]
#[ignore = "Requires the Pandoc oracle and fixtures"]
fn rank_reader() {
    let input = fs::read_to_string(test_dir().join("ipynb/rank.ipynb")).unwrap();
    let expected = fs::read_to_string(test_dir().join("ipynb/rank.out.html")).unwrap();
    let actual = read(&input, IpynbOutput::Best, "html");
    assert_eq!(
        oracle(&["-f", "json", "-t", "html"], &json::to_json(&actual)),
        expected
    );
}

fn write_fixture(name: &str, expected_file: Option<&str>) {
    let dir = test_dir().join("ipynb");
    let native = fs::read_to_string(dir.join(name)).unwrap();
    let doc = json::from_json(&oracle(&["-f", "native", "-t", "json"], &native)).unwrap();
    let mut options = WriterOptions::default();
    options.extensions = default_extensions("ipynb");
    options.wrap_text = WrapOption::WrapNone;
    let expected = expected_file.map_or_else(
        || oracle(&["-f", "native", "-t", "ipynb", "--wrap=none"], &native),
        |name| fs::read_to_string(dir.join(name)).unwrap(),
    );
    let mut context = Context::native();
    context
        .state
        .resource_path
        .insert(0, test_dir().to_string_lossy().into());
    let actual = randoc_ipynb::writer("ipynb")
        .unwrap()
        .write(&doc, &options, &mut context)
        .unwrap()
        .into_bytes();
    let actual = String::from_utf8(actual).unwrap();
    let normalize = |text: &str| {
        text.lines()
            .map(|line| {
                if line.starts_with("   \"id\": \"") && line.trim().len() == 44 {
                    let id = &line[10..46];
                    assert_eq!(&id[14..15], "4");
                    assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
                    "   \"id\": \"UUID\""
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(actual.ends_with('\n'));
    let expected = normalize(&expected);
    let actual = normalize(&actual);
    let difference = expected
        .lines()
        .zip(actual.lines())
        .position(|(a, b)| a != b);
    assert!(difference.is_none(), "{difference:?}");
    assert_eq!(expected.len(), actual.len());
}

#[test]
#[ignore = "Requires the Pandoc oracle and fixtures"]
fn mime_writer() {
    write_fixture("mime.native", Some("mime.out.ipynb"));
}

#[test]
#[ignore = "Requires the Pandoc oracle and fixtures"]
fn simple_writer() {
    write_fixture("simple.in.native", None);
}

#[test]
#[ignore = "Requires the Pandoc oracle and fixtures"]
fn command_6367_reader() {
    let script = r#"
import json, sys
sys.path.insert(0, sys.argv[1])
import suite
cases = suite.Discovery()
suite.discover_commands(sys.argv[2], cases)
print(json.dumps([case.__dict__ for case in cases.cases if case.id == 'command/6367.md#1']))
"#;
    let scripts = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/pandoc-testsuite");
    let output = Command::new("python3")
        .args(["-B", "-c", script])
        .arg(scripts)
        .arg(test_dir())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cases: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(cases.len(), 1);
    let input = cases[0]["stdin"].as_str().unwrap();
    let expected = cases[0]["expected"].as_str().unwrap();
    assert_eq!(oracle(&["-f", "ipynb", "-t", "org"], input), expected);
    ast(
        input,
        IpynbOutput::Best,
        "org",
        &oracle(&["-f", "ipynb", "-t", "json"], input),
    );
}
