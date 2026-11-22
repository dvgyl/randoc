//! Checks wiki readers and writers against the Pandoc 3.13 oracle.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use randoc_ast::{Pandoc, json};
use randoc_core::{
    Context, FormatSpec, Reader, ReaderOptions, Source, Sources, WrapOption, WriterOptions,
};
use serde_json::Value;

const FORMATS: [&str; 4] = ["mediawiki", "dokuwiki", "jira", "creole"];

struct Oracle {
    path: PathBuf,
    fixtures: PathBuf,
}

impl Oracle {
    fn run(&self, args: &[&str], input: &str) -> Output {
        let mut child = Command::new(&self.path)
            .current_dir(&self.fixtures)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start the Pandoc oracle");
        let mut stdin = child.stdin.take().expect("open oracle input");
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let _ = stdin.write_all(input.as_bytes());
            });
            child.wait_with_output().expect("read oracle output")
        })
    }

    fn ast(&self, format: &str, input: &str) -> Pandoc {
        let output = self.run(&["-f", format, "-t", "json"], input);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        json::from_json(&String::from_utf8_lossy(&output.stdout)).expect("decode oracle JSON")
    }
}

fn read(format: &str, name: &str, text: &str) -> randoc_core::Result<Pandoc> {
    let spec = FormatSpec::parse(format);
    let Some(Reader::Text(reader)) = randoc_wiki::reader(&spec.name) else {
        panic!("missing reader for {format}");
    };
    let mut options = ReaderOptions::default();
    options.extensions = spec.extensions()?;
    let sources = Sources(vec![Source {
        name: name.into(),
        text: text.into(),
    }]);
    reader.read(&sources, &options, &mut Context::pure())
}

fn write(format: &str, doc: &Pandoc, standalone: bool, wrap: WrapOption) -> String {
    let spec = FormatSpec::parse(format);
    let writer = randoc_wiki::writer(&spec.name).expect("wiki writer");
    let mut options = WriterOptions::default();
    options.extensions = spec.extensions().expect("writer extensions");
    options.wrap_text = wrap;
    if standalone {
        let name = format!("default.{}", spec.name);
        let source = randoc_data::default_template(&spec.name).expect("wiki template");
        options.template = Some(
            randoc_templates::Template::compile(&name, source, 128, &mut |_: &str| -> Result<
                String,
                (),
            > { Err(()) })
            .expect("compile wiki template"),
        );
    }
    let bytes = writer
        .write(doc, &options, &mut Context::pure())
        .expect("render wiki text")
        .into_bytes();
    String::from_utf8(bytes).expect("UTF-8 wiki text")
}

fn from_file(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

type Report = BTreeMap<(String, &'static str), (usize, usize)>;

fn record(
    report: &mut Report,
    format: &str,
    group: &'static str,
    id: &str,
    actual: &str,
    expected: &str,
) {
    let known = matches!(
        (format, group, id),
        (
            "mediawiki",
            "corpus",
            "wp-markdown.wiki" | "wp-printing-press.wiki"
        ) | ("jira", "old", "writer/basic")
    );
    let (passed, total) = report.entry((format.into(), group)).or_default();
    *total += 1;
    if actual == expected {
        assert!(!known, "Remove the resolved difference for {id}");
        *passed += 1;
    } else {
        let offset = actual
            .as_bytes()
            .iter()
            .zip(expected.as_bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let snippet = |text: &str| {
            String::from_utf8_lossy(
                &text.as_bytes()[offset.saturating_sub(35)..(offset + 110).min(text.len())],
            )
            .into_owned()
        };
        eprintln!(
            "{group}/{id} differs at byte {offset}\n  want {:?}\n  got  {:?}",
            snippet(expected),
            snippet(actual)
        );
        assert!(known, "Unexpected wiki difference in {group}/{id}");
    }
}

fn corpus(oracle: &Oracle, report: &mut Report) {
    let root = PathBuf::from(std::env::var_os("RANDOC_CORPUS").expect("source env/env.sh"));
    let mut files: Vec<_> = fs::read_dir(root)
        .expect("list wiki corpus")
        .map(|entry| entry.expect("read corpus entry").path())
        .collect();
    files.sort();
    for file in files {
        let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let format = if name.starts_with("wp-") && name.ends_with(".wiki") {
            "mediawiki"
        } else if name.starts_with("creole-") && name.ends_with(".creole") {
            "creole"
        } else {
            continue;
        };
        let path = file.to_str().expect("UTF-8 corpus path");
        let expected = oracle.run(&["-f", format, "-t", "json", path], "");
        let text = from_file(&file);
        let actual = read(format, path, &text);
        let expected = if expected.status.success() {
            json::to_json(
                &json::from_json(&String::from_utf8_lossy(&expected.stdout))
                    .expect("oracle corpus AST"),
            )
        } else {
            assert_eq!(
                expected.status.code(),
                Some(64),
                "unexpected oracle error for {name}"
            );
            format!(
                "64\n{}",
                String::from_utf8_lossy(&expected.stderr).trim_end_matches('\n')
            )
        };
        let actual = match actual {
            Ok(doc) => json::to_json(&doc),
            Err(error) => format!("{}\n{}", error.exit_code(), error),
        };
        record(report, format, "corpus", name, &actual, &expected);
    }
}

fn old(oracle: &Oracle, report: &mut Report) {
    for (format, input, expected) in [
        (
            "mediawiki",
            "mediawiki-reader.wiki",
            "mediawiki-reader.native",
        ),
        ("jira", "jira-reader.jira", "jira-reader.native"),
        ("creole", "creole-reader.txt", "creole-reader.native"),
    ] {
        let input = from_file(&oracle.fixtures.join(input));
        let expected = oracle.ast("native", &from_file(&oracle.fixtures.join(expected)));
        let actual =
            read(format, "", &input).map_or_else(|err| err.to_string(), |doc| json::to_json(&doc));
        record(
            report,
            format,
            "old",
            "reader",
            &actual,
            &json::to_json(&expected),
        );
    }

    // The inventory gives duplicate MediaWiki case names. Test both writer
    // entry points with the same public fixture and the default template.
    let cases = [
        (
            "mediawiki",
            "writer/basic",
            "writer.native",
            "writer.mediawiki",
            false,
        ),
        (
            "mediawiki",
            "writer/basic#2",
            "writer.native",
            "writer.mediawiki",
            true,
        ),
        (
            "mediawiki",
            "writer/tables",
            "tables.native",
            "tables.mediawiki",
            false,
        ),
        (
            "mediawiki",
            "writer/tables#2",
            "tables.native",
            "tables.mediawiki",
            true,
        ),
        (
            "mediawiki",
            "writer/nordics table",
            "tables/nordics.native",
            "tables/nordics.mediawiki",
            false,
        ),
        (
            "mediawiki",
            "writer/planets table",
            "tables/planets.native",
            "tables/planets.mediawiki",
            false,
        ),
        (
            "mediawiki",
            "writer/students table",
            "tables/students.native",
            "tables/students.mediawiki",
            false,
        ),
        ("jira", "writer/basic", "writer.native", "writer.jira", true),
        (
            "jira",
            "writer/tables",
            "tables.native",
            "tables.jira",
            false,
        ),
        (
            "dokuwiki",
            "writer/basic",
            "writer.native",
            "writer.dokuwiki",
            false,
        ),
        (
            "dokuwiki",
            "writer/tables",
            "tables.native",
            "tables.dokuwiki",
            false,
        ),
    ];
    for (format, id, input, expected, standalone) in cases {
        let doc = oracle.ast("native", &from_file(&oracle.fixtures.join(input)));
        let actual = write(format, &doc, standalone, WrapOption::WrapAuto);
        let expected = from_file(&oracle.fixtures.join(expected));
        if format == "jira" && id == "writer/basic" {
            // The Jira fixture has other differences. Compare its math section on its own.
            let math_section = |text: &str| {
                text.split_once("h1. {anchor:latex}LaTeX")
                    .and_then(|(_, body)| body.split_once("These shouldn’t be math:"))
                    .map(|(math, _)| math.to_owned())
                    .expect("Jira math section")
            };
            assert_eq!(math_section(&actual), math_section(&expected));
        }
        record(report, format, "old", id, &actual, &expected);
    }
    for (id, file) in [
        ("external images", "dokuwiki_external_images"),
        ("inline_formatting", "dokuwiki_inline_formatting"),
        ("multiblock table", "dokuwiki_multiblock_table"),
    ] {
        let doc = oracle.ast(
            "native",
            &from_file(&oracle.fixtures.join(format!("{file}.native"))),
        );
        let input = from_file(&oracle.fixtures.join(format!("{file}.dokuwiki")));
        let (actual, expected) = if id == "external images" {
            (
                read("dokuwiki", "", &input)
                    .map_or_else(|err| err.to_string(), |doc| json::to_json(&doc)),
                json::to_json(&doc),
            )
        } else {
            (write("dokuwiki", &doc, false, WrapOption::WrapAuto), input)
        };
        record(report, "dokuwiki", "old", id, &actual, &expected);
    }
}

fn commands(oracle: &Oracle, report: &mut Report) {
    let tools = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/pandoc-testsuite");
    let discovery = Command::new("python3")
        .args([
            "-B",
            "-c",
            r#"
import json, re, sys
sys.path.insert(0, sys.argv[1])
import suite
found = suite.Discovery()
suite.discover_commands(sys.argv[2], found)
wiki = re.compile(r"\b(mediawiki|dokuwiki|jira|creole)\b", re.I)
print(json.dumps([vars(case) for case in found.cases if wiki.search(case.command)]))
"#,
        ])
        .arg(tools)
        .arg(&oracle.fixtures)
        .output()
        .expect("discover wiki command fixtures");
    assert!(
        discovery.status.success(),
        "{}",
        String::from_utf8_lossy(&discovery.stderr)
    );
    let cases: Vec<Value> =
        serde_json::from_slice(&discovery.stdout).expect("decode command fixtures");
    assert_eq!(cases.len(), 64);
    for case in cases {
        let field = |name: &str| case[name].as_str().expect("command fixture field");
        let command = field("command");
        let args: Vec<_> = command.split_whitespace().collect();
        let argument = |short, default| {
            args.windows(2)
                .find_map(|pair| (pair[0] == short).then_some(pair[1]))
                .unwrap_or(default)
        };
        let from = argument("-f", "markdown");
        let to = argument("-t", "html");
        let wiki = |spec: &str| {
            FORMATS
                .into_iter()
                .find(|format| FormatSpec::parse(spec).name == *format)
        };
        let input = field("stdin");
        let document = if wiki(from).is_some() {
            read(from, "", input)
        } else {
            Ok(oracle.ast(from, input))
        };
        let actual = match document {
            Err(error) => error.to_string(),
            Ok(doc) if wiki(to).is_some() => {
                let wrapping = if args.contains(&"--wrap=preserve") {
                    WrapOption::WrapPreserve
                } else {
                    WrapOption::WrapAuto
                };
                write(to, &doc, args.contains(&"-s"), wrapping)
            }
            Ok(doc) => {
                let output = oracle.run(&["-f", "json", "-t", to], &json::to_json(&doc));
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                String::from_utf8(output.stdout).expect("UTF-8 command output")
            }
        };
        record(
            report,
            wiki(from).or_else(|| wiki(to)).expect("wiki command"),
            "command",
            field("id"),
            &actual,
            field("expected"),
        );
    }
}

#[test]
#[ignore = "Requires the installed Pandoc oracle, fixtures, and corpus"]
fn oracle_parity() {
    let oracle = Oracle {
        path: PathBuf::from(std::env::var_os("RANDOC_ORACLE_PANDOC").expect("source env/env.sh")),
        fixtures: PathBuf::from(std::env::var_os("RANDOC_PANDOC_SRC").expect("source env/env.sh"))
            .join("test"),
    };
    let mut report = Report::new();
    corpus(&oracle, &mut report);
    old(&oracle, &mut report);
    commands(&oracle, &mut report);
    for format in FORMATS {
        for group in ["corpus", "old", "command"] {
            let (passed, count) = report
                .get(&(format.into(), group))
                .copied()
                .unwrap_or_default();
            eprintln!("{format} {group} {passed}/{count}");
        }
    }
    assert_eq!(report.values().map(|(_, total)| total).sum::<usize>(), 100);
    assert_eq!(report.values().map(|(passed, _)| passed).sum::<usize>(), 97);
}
