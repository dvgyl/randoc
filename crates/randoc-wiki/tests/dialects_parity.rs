//! Compares the additional wiki formats with their reference fixtures.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use randoc_ast::{Pandoc, json};
use randoc_core::{Context, FormatSpec, Reader, ReaderOptions, Sources, WriterOptions};
use serde_json::Value;

const READERS: [&str; 3] = ["tikiwiki", "twiki", "vimwiki"];
const WRITERS: [&str; 2] = ["xwiki", "zimwiki"];

fn read(format: &str, text: &str, context: &mut Context) -> randoc_core::Result<Pandoc> {
    let spec = FormatSpec::parse(format);
    let Some(Reader::Text(reader)) = randoc_wiki::reader(&spec.name) else {
        panic!("Missing reader for {format}");
    };
    let mut options = ReaderOptions::default();
    options.extensions = spec.extensions()?;
    reader.read(&Sources::from_text(text), &options, context)
}

fn write(
    format: &str,
    doc: &Pandoc,
    standalone: bool,
    context: &mut Context,
) -> randoc_core::Result<String> {
    let writer = randoc_wiki::writer(format).expect("Get wiki writer");
    let template = standalone.then(|| {
        let name = format!("default.{format}");
        let source = randoc_data::default_template(format).expect("Get wiki template");
        randoc_templates::Template::compile(
            &name,
            source,
            128,
            &mut |_: &str| -> Result<String, ()> { Err(()) },
        )
        .expect("Compile wiki template")
    });
    let mut options = WriterOptions::default();
    options.template = template;
    let output = writer.write(doc, &options, context)?;
    Ok(String::from_utf8(output.into_bytes()).expect("Decode wiki output"))
}

fn load(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path.as_ref())
        .unwrap_or_else(|err| panic!("{}: {err}", path.as_ref().display()))
}

#[test]
#[ignore = "Uses the local conformance fixtures"]
fn reader_baselines() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance");
    let mut failures = Vec::new();
    for format in READERS {
        let cases = load(root.join(format!("cases/reader-baseline/{format}.toml")));
        let expected = load(root.join(format!("expected/reader-baseline/{format}.json")));
        let expected: Value = serde_json::from_str(&expected).expect("Decode baseline");
        let units = expected["units"].as_object().expect("Get baseline units");
        assert_eq!(units.len(), 40);
        let (header, cases) = cases
            .split_once("\n[[case]]\n")
            .expect("Read baseline header");
        assert_eq!(header.trim(), format!("from = \"{format}\""));
        let mut total = 0;
        let mut passed = 0;
        // These fixtures use one literal multiline input and one ID per case.
        for case in cases.split("\n[[case]]\n") {
            let (id, data) = case.split_once("\ninput = ").expect("Read baseline input");
            let id = id
                .strip_prefix("id = \"")
                .and_then(|s| s.strip_suffix('"'))
                .expect("Read baseline ID");
            let input = data
                .trim_end()
                .strip_prefix("'''\n")
                .and_then(|s| s.strip_suffix("'''"))
                .expect("Read literal block");
            let expected = &units[&format!("{id}#read")];
            let doc = read(format, input, &mut Context::pure());
            let (code, stdout, stderr) = match doc {
                Ok(doc) => (0, json::to_json(&doc), String::new()),
                Err(err) => (err.exit_code(), String::new(), format!("{err}\n")),
            };
            let json_value = |s: &str| serde_json::from_str::<Value>(s).ok();
            total += 1;
            if expected["exit"] == code
                && expected["stderr"] == stderr
                && json_value(expected["stdout"].as_str().unwrap()) == json_value(&stdout)
            {
                passed += 1;
            } else {
                failures.push(format!(
                    "{format}/{id}\nexpected: {}\nactual: {stdout}{stderr}",
                    expected["stdout"].as_str().unwrap()
                ));
            }
        }
        assert_eq!(total, 40);
        eprintln!("{format} baseline {passed}/{total}");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

struct Oracle {
    path: PathBuf,
    fixtures: PathBuf,
}

impl Oracle {
    fn from_env() -> Self {
        let fixtures =
            PathBuf::from(std::env::var_os("RANDOC_PANDOC_SRC").expect("Source env/env.sh"))
                .join("test");
        let path = std::env::var_os("RANDOC_ORACLE_PANDOC")
            .expect("Source env/env.sh")
            .into();
        Self { path, fixtures }
    }

    fn run(&self, args: &[&str], input: &str) -> String {
        let mut child = Command::new(&self.path)
            .args(args)
            .current_dir(&self.fixtures)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Start oracle");
        let mut stdin = child.stdin.take().expect("Open oracle stdin");
        let output = std::thread::scope(|scope| {
            scope.spawn(move || {
                stdin
                    .write_all(input.as_bytes())
                    .expect("Write oracle input")
            });
            child.wait_with_output().expect("Read oracle output")
        });
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("Decode oracle output")
    }

    fn native(&self, source: &str) -> Pandoc {
        json::from_json(&self.run(&["-f", "native", "-t", "json"], source))
            .expect("Decode reference AST")
    }
}

fn command_cases(text: &str) -> impl Iterator<Item = (&str, &str, &str)> {
    text.split("```\n").filter_map(|block| {
        let (command, rest) = block.strip_prefix("% pandoc ")?.split_once('\n')?;
        let (input, expected) = rest.split_once("^D\n")?;
        Some((
            command,
            input,
            if expected.ends_with("\n\n") {
                &expected[..expected.len() - 1]
            } else {
                expected
            },
        ))
    })
}

fn diff<T: PartialEq + std::fmt::Debug>(actual: &[T], expected: &[T]) -> String {
    use std::fmt::Write as _;
    let mut matches = vec![vec![0u16; expected.len() + 1]; actual.len() + 1];
    for i in (0..actual.len()).rev() {
        for j in (0..expected.len()).rev() {
            matches[i][j] = if actual[i] == expected[j] {
                1 + matches[i + 1][j + 1]
            } else {
                matches[i + 1][j].max(matches[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut result = String::new();
    while i < actual.len() || j < expected.len() {
        if actual
            .get(i)
            .zip(expected.get(j))
            .is_some_and(|(a, b)| a == b)
        {
            i += 1;
            j += 1;
        } else if j == expected.len() || i < actual.len() && matches[i + 1][j] >= matches[i][j + 1]
        {
            let _ = writeln!(result, " got {i} (want {j}): {:?}", actual[i]);
            i += 1;
        } else {
            let _ = writeln!(result, "want {j} (got {i}): {:?}", expected[j]);
            j += 1;
        }
    }
    result
}

#[test]
#[ignore = "Uses the upstream reference fixtures and Pandoc oracle"]
fn reader_oracle() {
    let oracle = Oracle::from_env();
    let mut failures = Vec::new();
    for (format, suffix) in [
        ("tikiwiki", "tikiwiki"),
        ("twiki", "twiki"),
        ("vimwiki", "wiki"),
    ] {
        let input = load(oracle.fixtures.join(format!("{format}-reader.{suffix}")));
        let expected = oracle.native(&load(
            oracle.fixtures.join(format!("{format}-reader.native")),
        ));
        let actual = read(format, &input, &mut Context::pure()).expect("Read wiki fixture");
        let pass = actual == expected;
        eprintln!("{format} old {}/1", usize::from(pass));
        if !pass {
            if actual.meta != expected.meta {
                failures.push(format!(
                    "{format} metadata: {:?} != {:?}",
                    actual.meta, expected.meta
                ));
            }
            failures.push(format!(
                "{format}\n{}",
                diff(&actual.blocks, &expected.blocks)
            ));
        }
    }
    let mut commands = 0;
    let fixture = load(oracle.fixtures.join("command/4722.md"));
    assert_eq!(command_cases(&fixture).count(), 2);
    for (command, input, expected) in command_cases(&fixture) {
        assert_eq!(command, "-f tikiwiki -t native");
        let actual =
            read("tikiwiki", input, &mut Context::pure()).expect("Read TikiWiki command input");
        if actual == oracle.native(expected) {
            commands += 1;
        } else {
            failures.push(format!("TikiWiki command differs for {input:?}"));
        }
    }
    eprintln!("tikiwiki command {commands}/2");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "Uses the upstream reference fixtures and Pandoc oracle"]
fn writer_oracle() {
    let oracle = Oracle::from_env();
    let mut failures = Vec::new();
    for format in WRITERS {
        let mut old = 0;
        for (input, expected, standalone) in [
            ("testsuite.native", "writer", true),
            ("tables.native", "tables", false),
        ] {
            let source = load(oracle.fixtures.join(input));
            let doc = oracle.native(&source);
            let mut args = vec![
                "-f",
                "native",
                "-t",
                format,
                "--columns=80",
                "-V",
                "pandoc-version=",
            ];
            if standalone {
                args.push("-s");
            }
            let expected = load(oracle.fixtures.join(format!("{expected}.{format}")));
            assert_eq!(oracle.run(&args, &source), expected);
            let actual =
                write(format, &doc, standalone, &mut Context::pure()).expect("Write wiki fixture");
            if actual == expected {
                old += 1;
            } else {
                failures.push(format!(
                    "{format}/{input}\n{}",
                    diff(
                        &actual.split('\n').collect::<Vec<_>>(),
                        &expected.split('\n').collect::<Vec<_>>()
                    )
                ));
            }
        }
        eprintln!("{format} old {old}/2");
        let fixture = load(oracle.fixtures.join(format!("command/figures-{format}.md")));
        assert_eq!(command_cases(&fixture).count(), 1);
        for (command, input, expected) in command_cases(&fixture) {
            assert_eq!(command, format!("-f native -t {format}"));
            assert_eq!(oracle.run(&["-f", "native", "-t", format], input), expected);
            let actual = write(format, &oracle.native(input), false, &mut Context::pure())
                .expect("Write wiki command input");
            eprintln!("{format} command {}/1", usize::from(actual == expected));
            if actual != expected {
                failures.push(format!("{format} command: {actual:?} != {expected:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
