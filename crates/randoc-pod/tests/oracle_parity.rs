//! Black-box comparisons with Pandoc 3.13. These tests run the reader in process.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use randoc_ast::{Pandoc, json};
use randoc_core::{Context, Reader, ReaderOptions, Sources};
use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn env_path(key: &str) -> PathBuf {
    PathBuf::from(env::var_os(key).unwrap_or_else(|| panic!("Set {key} with env/env.sh")))
}

fn oracle(args: &[&str], input: &str, cwd: &Path) -> Output {
    let mut child = Command::new(env_path("RANDOC_ORACLE_PANDOC"))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let result = child.stdin.take().unwrap().write_all(input.as_bytes());
    let out = child.wait_with_output().unwrap();
    if let Err(error) = result {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    out
}

fn read(input: &str, format: &str) -> randoc_core::Result<Pandoc> {
    let Reader::Text(reader) = randoc_pod::reader(format).unwrap() else {
        panic!()
    };

    let mut preprocessed = String::new();
    let mut column = 0;
    for ch in input.chars() {
        match ch {
            '\r' => {}
            '\n' => {
                preprocessed.push(ch);
                column = 0;
            }
            '\t' if format != "tsv" => {
                let spaces = 4 - column % 4;
                preprocessed.extend(std::iter::repeat_n(' ', spaces));
                column += spaces;
            }
            _ => {
                preprocessed.push(ch);
                column += 1;
            }
        }
    }
    let input = preprocessed.as_str();
    reader.read(
        &Sources::from_text(input),
        &ReaderOptions::default(),
        &mut Context::pure(),
    )
}

fn first_diff(actual: &Value, expected: &Value, path: &str) -> Option<String> {
    match (actual, expected) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys().filter(|key| !a.contains_key(*key))) {
                if let Some(diff) =
                    first_diff(&actual[key], &expected[key], &format!("{path}.{key}"))
                {
                    return Some(diff);
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                if let Some(diff) = first_diff(a, b, &format!("{path}[{index}]")) {
                    return Some(diff);
                }
            }
            if a.len() != b.len() {
                return Some(format!("{path} count: {} != {}", a.len(), b.len()));
            }
        }
        _ if actual != expected => return Some(format!("{path}: {actual} != {expected}")),
        _ => {}
    }
    None
}

fn compare(actual: &str, expected: &[u8]) -> Option<String> {
    let actual: Value = serde_json::from_str(actual).unwrap();
    let expected: Value = serde_json::from_slice(expected).unwrap();
    first_diff(&actual, &expected, "")
}

fn report(group: &str, count: usize, failures: Vec<String>) {
    eprintln!("{group}: {}/{count}", count - failures.len());
    for failure in &failures {
        eprintln!("{failure}");
    }
    assert!(failures.is_empty());
}

fn python(script: &str, argument: &Path) -> Value {
    let result = Command::new("python3")
        .args(["-B", "-c", script])
        .arg(argument)
        .current_dir(root())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

fn replay(format: &str) {
    let cases = root().join(format!("conformance/cases/reader-baseline/{format}.toml"));
    let expected = root().join(format!(
        "conformance/expected/reader-baseline/{format}.json"
    ));
    let cases = python(
        r#"
import json, sys, tomllib
with open(sys.argv[1], "rb") as source:
    print(json.dumps(tomllib.load(source)["case"]))
"#,
        &cases,
    );
    let expected: Value = serde_json::from_slice(&fs::read(expected).unwrap()).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 40);
    assert_eq!(expected["units"].as_object().unwrap().len(), cases.len());
    let mut failures = Vec::new();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let unit = &expected["units"][format!("{id}#read")];
        if let Some(diff) = check(
            case["input"].as_str().unwrap(),
            format,
            unit["exit"].as_i64().unwrap() as i32,
            unit["stdout"].as_str().unwrap().as_bytes(),
            unit["stderr"].as_str().unwrap().as_bytes(),
        ) {
            failures.push(format!("{id} {diff}"));
        }
    }
    report(format, cases.len(), failures);
}

fn check(input: &str, format: &str, exit: i32, stdout: &[u8], stderr: &[u8]) -> Option<String> {
    match read(input, format) {
        Ok(doc) if exit == 0 && stderr.is_empty() => compare(&json::to_json(&doc), stdout),
        Err(error) if error.exit_code() == exit && stdout.is_empty() => {
            (format!("{error}\n").as_bytes() != stderr)
                .then(|| format!("{error} != {}", String::from_utf8_lossy(stderr)))
        }
        Ok(_) => Some(format!("Randoc returned a document. Oracle exit {exit}")),
        Err(error) => Some(format!(
            "{error} (exit {}) vs oracle exit {exit}",
            error.exit_code()
        )),
    }
}

fn commands(formats: &str, count: usize) {
    let suite = env_path("RANDOC_PANDOC_SRC").join("test");
    let script = format!(
        r#"
import json, shlex, sys
sys.path.insert(0, "tools/pandoc-testsuite")
import suite
found = suite.Discovery()
suite.discover_commands(sys.argv[1], found)
result = []
for case in found.cases:
    args = shlex.split(case.command)
    if "-f" in args and args[args.index("-f") + 1] in {formats:?}.split():
        result.append([case.id, args[args.index("-f") + 1], args[args.index("-t") + 1], case.stdin, case.expected])
print(json.dumps(result))
"#
    );
    let cases = python(&script, &suite);
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), count);
    let mut failures = Vec::new();
    for case in cases {
        let id = case[0].as_str().unwrap();
        let format = case[1].as_str().unwrap();
        let target = case[2].as_str().unwrap();
        let input = case[3].as_str().unwrap();
        let direct = oracle(&["-f", format, "-t", "json"], input, &suite);
        if let Some(diff) = check(
            input,
            format,
            direct.status.code().unwrap(),
            &direct.stdout,
            &direct.stderr,
        ) {
            failures.push(format!("{id} {diff}"));
            continue;
        }
        let doc = read(input, format).unwrap();
        // Use the oracle writer because these reader crates have no writer dependencies.
        let rendered = oracle(&["-f", "json", "-t", target], &json::to_json(&doc), &suite);
        if !rendered.status.success()
            || !rendered.stderr.is_empty()
            || rendered.stdout != case[4].as_str().unwrap().as_bytes()
        {
            failures.push(format!("{id} {target} output differs"));
        }
    }
    report("Command cases", count, failures);
}

fn corpus(format: &str, names: &[&str]) {
    let mut failures = Vec::new();
    for name in names {
        let input = fs::read_to_string(env_path("RANDOC_CORPUS").join(name)).unwrap();
        let expected = oracle(&["-f", format, "-t", "json"], &input, &root());
        if let Some(diff) = check(
            &input,
            format,
            expected.status.code().unwrap(),
            &expected.stdout,
            &expected.stderr,
        ) {
            failures.push(format!("{name} {diff}"));
        }
    }
    report("Corpus documents", names.len(), failures);
}

#[test]
#[ignore = "requires the conformance fixtures"]
fn conformance() {
    replay("pod");
}

#[test]
#[ignore = "requires the Pandoc executable and the POD corpus"]
fn pod_corpus() {
    corpus(
        "pod",
        &[
            "perl-perlintro.pod",
            "perl-perlpod.pod",
            "perl-perlre.pod",
            "perl-perlstyle.pod",
            "perl-perlsyn.pod",
        ],
    );
}

#[test]
#[ignore = "requires the Pandoc executable and the POD fixture"]
fn pod_old_fixture() {
    let suite = env_path("RANDOC_PANDOC_SRC").join("test");
    let input = fs::read_to_string(suite.join("pod-reader.pod")).unwrap();
    let native = fs::read_to_string(suite.join("pod-reader.native")).unwrap();
    let expected = oracle(&["-f", "native", "-t", "json"], &native, &suite);
    assert!(expected.status.success());
    assert!(expected.stderr.is_empty());
    let mut failures = Vec::new();
    if let Some(diff) = check(&input, "pod", 0, &expected.stdout, &[]) {
        failures.push(diff);
    }
    report("POD old fixture", 1, failures);
}

#[test]
#[ignore = "requires the Pandoc executable and the command fixtures"]
fn pod_commands() {
    commands("pod", 2);
}
