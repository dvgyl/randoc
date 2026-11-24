//! Compares AsciiDoc conversions with Pandoc 3.13.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use randoc_ast::{Pandoc, json};
use randoc_core::{
    Context, FormatSpec, Host, Reader, ReaderOptions, Source, Sources, Verbosity, WrapOption,
    WriterOptions,
};
use randoc_templates::Template;
use serde_json::Value;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const DIALECTS: &[&str] = &["asciidoc", "asciidoctor", "asciidoc_legacy"];

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(dir: &Path, args: &[&str], input: &str) -> TestResult<Outcome> {
    let mut child = Command::new(env::var("RANDOC_ORACLE_PANDOC")?)
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("Missing standard input"))?;
    let output = std::thread::scope(|scope| {
        let send = scope.spawn(move || stdin.write_all(input.as_bytes()));
        let output = child.wait_with_output();
        send.join()
            .map_err(|_| io::Error::other("Input thread stopped"))??;
        output
    })?;
    Ok(Outcome {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8(output.stdout)?,
        stderr: String::from_utf8(output.stderr)?,
    })
}

struct Files {
    root: PathBuf,
    sandbox: bool,
}

impl Host for Files {
    fn is_sandboxed(&self) -> bool {
        self.sandbox
    }

    fn read_file_limited(&mut self, path: &str, max: usize) -> io::Result<Vec<u8>> {
        randoc_core::read_real_file_limited(&self.root.join(path).to_string_lossy(), max)
    }
}

fn flag(args: &[&str], flags: &[&str]) -> bool {
    args.iter().any(|arg| flags.contains(arg))
}

fn option<'a>(args: &[&'a str], short: &str, long: &str) -> Option<&'a str> {
    args.iter().enumerate().find_map(|(index, arg)| {
        if *arg == short || *arg == long {
            args.get(index + 1).copied()
        } else {
            arg.strip_prefix(long)?.strip_prefix('=')
        }
    })
}

fn convert(dir: &Path, args: &[&str], source: Source) -> TestResult<Outcome> {
    let from = option(args, "-f", "--from").unwrap_or("markdown");
    let to = option(args, "-t", "--to").unwrap_or("html");
    fn base(format: &str) -> &str {
        format.split(['+', '-']).next().unwrap_or("")
    }
    let standalone = flag(args, &["-s", "--standalone"]);
    let mut context = Context::new(Box::new(Files {
        root: dir.into(),
        sandbox: flag(args, &["--sandbox"]),
    }));
    if flag(args, &["--verbose"]) {
        context.state.verbosity = Verbosity::Info;
    }
    let doc: Pandoc = if base(from) == "asciidoc" {
        let Some(Reader::Text(reader)) = randoc_asciidoc::reader("asciidoc") else {
            return Err(io::Error::other("Missing reader").into());
        };
        let mut options = ReaderOptions::default();
        options.extensions = FormatSpec::parse(from).extensions()?;
        options.standalone = standalone;
        options.output_format = Some(base(to).into());
        match reader.read(&Sources(vec![source]), &options, &mut context) {
            Ok(doc) => doc,
            Err(err) => {
                return Ok(Outcome {
                    code: err.exit_code(),
                    stdout: String::new(),
                    stderr: format!("{err}\n"),
                });
            }
        }
    } else {
        // The oracle supplies the input AST when a case uses another reader.
        let upstream = run(dir, &["-f", from, "-t", "json"], &source.text)?;
        if upstream.code != 0 {
            return Ok(upstream);
        }
        json::from_json(&upstream.stdout)?
    };
    let stderr = |context: &Context| {
        context
            .log()
            .iter()
            .filter(|msg| msg.level() <= context.state.verbosity)
            .map(|msg| format!("[{}] {}\n", msg.level().label(), msg.text()))
            .collect::<String>()
    };
    if base(to) == "json" {
        return Ok(Outcome {
            code: 0,
            stdout: format!("{}\n", json::to_json(&doc)),
            stderr: stderr(&context),
        });
    }
    if let Some(writer) = randoc_asciidoc::writer(base(to)) {
        if base(to) == "asciidoctor" {
            context.report(randoc_core::LogMessage::Deprecated(
                "asciidoctor".into(),
                "use asciidoc instead".into(),
            ));
        }
        let mut options = WriterOptions::default();
        options.extensions = FormatSpec::parse(to).extensions()?;
        options.columns = option(args, "-c", "--columns")
            .map(str::parse)
            .transpose()?
            .unwrap_or(72);
        options.wrap_text = match option(args, "-w", "--wrap") {
            Some("preserve") => WrapOption::WrapPreserve,
            Some("none") => WrapOption::WrapNone,
            _ => WrapOption::WrapAuto,
        };
        options.table_of_contents = flag(args, &["--toc", "--table-of-contents"]);
        if standalone {
            let template = randoc_data::default_template(base(to))
                .ok_or_else(|| io::Error::other("Missing default template"))?;
            options.template = Some(Template::compile(
                "default.asciidoc",
                template,
                128,
                &mut |path| Err(io::Error::other(format!("Missing template partial {path}"))),
            )?);
        }
        for (i, arg) in args.iter().enumerate() {
            if *arg == "-V"
                && let Some((key, value)) = args.get(i + 1).and_then(|value| value.split_once('='))
            {
                options
                    .variables
                    .insert(key, randoc_templates::Val::text(value));
            }
        }
        let stdout = String::from_utf8(writer.write(&doc, &options, &mut context)?.into_bytes())?;
        Ok(Outcome {
            code: 0,
            stdout: if standalone || stdout.ends_with('\n') {
                stdout
            } else {
                format!("{stdout}\n")
            },
            stderr: stderr(&context),
        })
    } else {
        // Use the oracle to render the result of the AsciiDoc reader.
        let mut downstream = vec!["-f", "json", "-t", to];
        if standalone {
            downstream.push("-s");
        }
        let mut output = run(dir, &downstream, &json::to_json(&doc))?;
        output.stderr.insert_str(0, &stderr(&context));
        Ok(output)
    }
}

fn first_difference(expected: &str, actual: &str) -> String {
    let mut a = expected.lines();
    let mut b = actual.lines();
    let mut i = 1;
    loop {
        let (left, right) = (a.next(), b.next());
        if left != right {
            let brief = |value: Option<&str>| {
                value.map(|value| value.chars().take(160).collect::<String>())
            };
            return format!(
                "line {i}, expected {:?}, got {:?}",
                brief(left),
                brief(right)
            );
        }
        if left.is_none() {
            return "line endings differ".into();
        }
        i += 1;
    }
}

fn json_difference(expected: &Value, actual: &Value, path: &str) -> Option<String> {
    if expected == actual {
        return None;
    }
    match (expected, actual) {
        (Value::Array(left), Value::Array(right)) => left
            .iter()
            .zip(right)
            .enumerate()
            .find_map(|(i, (a, b))| json_difference(a, b, &format!("{path}[{i}]")))
            .or_else(|| {
                Some(format!(
                    "{path} has {} items, expected {}. First extra item is {}",
                    right.len(),
                    left.len(),
                    left.get(right.len())
                        .or_else(|| right.get(left.len()))
                        .map_or(String::new(), |value| value
                            .to_string()
                            .chars()
                            .take(250)
                            .collect())
                ))
            }),
        (Value::Object(left), Value::Object(right))
            if left.keys().eq(right.keys()) && left.get("t") == right.get("t") =>
        {
            left.iter().find_map(|(key, value)| {
                json_difference(value, &right[key], &format!("{path}.{key}"))
            })
        }
        _ => {
            let brief = |value: &Value| value.to_string().chars().take(160).collect::<String>();
            Some(format!(
                "{path} expected {}, got {}",
                brief(expected),
                brief(actual)
            ))
        }
    }
}

fn compare(label: &str, expected: &Outcome, actual: &Outcome, is_json: bool) -> TestResult<bool> {
    if expected.code != actual.code || expected.stderr != actual.stderr {
        eprintln!(
            "{label} expected exit {} and {:?}, got exit {} and {:?}",
            expected.code, expected.stderr, actual.code, actual.stderr
        );
        return Ok(false);
    }
    if is_json && expected.code == 0 {
        let expected = serde_json::from_str(&expected.stdout)?;
        let actual = serde_json::from_str(&actual.stdout)?;
        if let Some(diff) = json_difference(&expected, &actual, "$") {
            eprintln!("{label} {diff}");
            return Ok(false);
        }
    } else if expected.stdout != actual.stdout {
        eprintln!(
            "{label} {}",
            first_difference(&expected.stdout, &actual.stdout)
        );
        return Ok(false);
    }
    Ok(true)
}

fn test_dir() -> TestResult<PathBuf> {
    Ok(PathBuf::from(env::var("RANDOC_PANDOC_SRC")?).join("test"))
}

#[test]
#[ignore = "Requires the Pandoc 3.13 binary and its command fixtures"]
fn command_cases() -> TestResult {
    const DISCOVER: &str = r#"
import json, shlex, sys
sys.path.insert(0, sys.argv[1])
import suite
result = suite.Discovery()
suite.discover_commands(sys.argv[2], result)
for case in result.cases:
    args = shlex.split(case.command)
    if args and args[0] == 'pandoc' and any(
        i and args[i - 1] in ('-f', '-t', '--from', '--to')
        and arg.split('+')[0].split('-')[0] in ('asciidoc', 'asciidoctor', 'asciidoc_legacy')
        for i, arg in enumerate(args)
    ):
        print(json.dumps([case.id, args[1:], case.stdin]))
"#;
    let dir = test_dir()?;
    let tools = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/pandoc-testsuite");
    let cases = Command::new("python3")
        .args(["-B", "-c", DISCOVER])
        .arg(tools)
        .arg(&dir)
        .output()?;
    assert!(
        cases.status.success(),
        "{}",
        String::from_utf8_lossy(&cases.stderr)
    );
    let mut count = 0;
    let mut passed = 0;
    for line in String::from_utf8(cases.stdout)?.lines() {
        let (name, args, input): (String, Vec<String>, String) = serde_json::from_str(line)?;
        let args = args.iter().map(String::as_str).collect::<Vec<_>>();
        count += 1;
        let expected = run(&dir, &args, &input)?;
        let actual = convert(
            &dir,
            &args,
            Source {
                name: String::new(),
                text: input,
            },
        )?;
        if compare(
            &name,
            &expected,
            &actual,
            option(&args, "-t", "--to") == Some("json"),
        )? {
            passed += 1;
        }
    }
    eprintln!("Command cases: {passed}/{count}");
    assert_eq!(count, 31);
    assert_eq!(passed, count);
    Ok(())
}

#[test]
#[ignore = "Requires the Pandoc 3.13 binary and its format fixtures"]
fn old_fixtures() -> TestResult {
    let dir = test_dir()?;
    let mut passed = 0;
    let name = "asciidoc-reader.adoc";
    let args = ["-f", "asciidoc", "-t", "native", "-s"];
    let input = fs::read_to_string(dir.join(name))?;
    let expected = run(&dir, &args, &input)?;
    assert_eq!(
        expected.stdout,
        fs::read_to_string(dir.join("asciidoc-reader.native"))?
    );

    let actual = convert(
        &dir,
        &args,
        Source {
            name: name.into(),
            text: input,
        },
    )?;
    passed += usize::from(compare(name, &expected, &actual, false)?);
    for (file, standalone) in [("writer.native", true), ("tables.native", false)] {
        let input = fs::read_to_string(dir.join(file))?;
        for to in ["asciidoc", "asciidoc_legacy"] {
            let mut args = vec!["-f", "native", "-t", to];
            if standalone {
                args.extend(["-s", "--columns=80"]);
            }
            let expected = run(&dir, &args, &input)?;
            let fixture = format!("{}.{to}", file.trim_end_matches(".native"));
            assert_eq!(expected.stdout, fs::read_to_string(dir.join(fixture))?);

            let actual = convert(
                &dir,
                &args,
                Source {
                    name: file.into(),
                    text: input.clone(),
                },
            )?;
            passed += usize::from(compare(
                &format!("{file} to {to}"),
                &expected,
                &actual,
                false,
            )?);
        }
    }
    eprintln!("Old fixtures: {passed}/5");
    assert_eq!(passed, 5);
    Ok(())
}

#[test]
#[ignore = "Requires the Pandoc 3.13 binary and the AsciiDoc corpus"]
fn corpus() -> TestResult {
    let dir = PathBuf::from(env::var("RANDOC_CORPUS")?);
    let mut files = fs::read_dir(&dir)?.collect::<Result<Vec<_>, _>>()?;
    files.sort_by_key(fs::DirEntry::file_name);
    let args = ["-f", "asciidoc", "-t", "json"];
    let mut count = 0;
    let mut passed = 0;
    for file in files {
        let name = file.file_name().into_string().unwrap_or_default();
        if !name.starts_with("asciidoctor-") || !name.ends_with(".adoc") {
            continue;
        }
        count += 1;
        let text = fs::read_to_string(file.path())?;
        let expected = run(&dir, &["-t", "json", &name], "")?;
        let actual = convert(
            &dir,
            &args,
            Source {
                name: name.clone(),
                text,
            },
        )?;
        passed += usize::from(compare(&name, &expected, &actual, true)?);
    }
    eprintln!("Corpus documents: {passed}/{count}");
    assert_eq!(count, 8);
    assert_eq!(passed, count);
    Ok(())
}

#[test]
#[ignore = "Requires the Pandoc 3.13 binary for error and writer probes"]
fn error_and_dialect_cases() -> TestResult {
    let dir = test_dir()?;
    let error_cases = [
        ("second title", "== First\n\n= Later\n", false, 64),
        (
            "missing include",
            "include::absent-randoc-test.adoc[]\n",
            false,
            1,
        ),
        (
            "sandboxed include",
            "include::asciidoc-reader-include.adoc[]\n",
            true,
            99,
        ),
    ];
    let mut passed = 0;
    for (name, text, sandbox, code) in error_cases {
        let mut args = vec!["-f", "asciidoc", "-t", "json"];
        if sandbox {
            args.push("--sandbox");
        }
        let expected = run(&dir, &args, text)?;
        assert_eq!(expected.code, code, "{name}");
        let actual = convert(
            &dir,
            &args,
            Source {
                name: String::new(),
                text: text.into(),
            },
        )?;
        passed += usize::from(compare(name, &expected, &actual, false)?);
    }
    for to in DIALECTS {
        let text = "# Heading\n\nA math expression $x^2$.\n";
        let args = [
            "-f",
            "markdown",
            "-t",
            to,
            "-s",
            "-V",
            "include-after=AFTER",
        ];
        let expected = run(&dir, &args, text)?;
        let actual = convert(
            &dir,
            &args,
            Source {
                name: String::new(),
                text: text.into(),
            },
        )?;
        passed += usize::from(compare(to, &expected, &actual, false)?);
    }
    eprintln!("Error and writer probes: {passed}/6");
    assert_eq!(passed, 6);
    Ok(())
}

#[test]
#[ignore = "Requires the Pandoc 3.13 binary for review probes"]
fn review_cases() -> TestResult {
    let dir = env::temp_dir().join(format!("randoc-asciidoc-review-{}", std::process::id()));
    fs::create_dir_all(dir.join("sub"))?;
    let file_cases = [
        ("top.adoc", "First\n\n= Later\n", 64),
        ("sub/main.adoc", "= Document\n\ninclude::bad.adoc[]\n", 64),
        ("sub/self.adoc", "include::self.adoc[]\n", 0),
        ("sub/c1.adoc", "include::c2.adoc[]\n", 0),
        (
            "sub/path.adoc",
            ":target: bad.adoc\n\ninclude::{target}[]\n",
            1,
        ),
    ];
    for (name, content) in [
        ("sub/bad.adoc", "== Inc\n\n= Top\n"),
        ("sub/c2.adoc", "include::c1.adoc[]\n"),
    ] {
        fs::write(dir.join(name), content)?;
    }
    for (name, content, _) in file_cases {
        fs::write(dir.join(name), content)?;
    }
    let mut count = 0;
    let mut passed = 0;
    for (name, content, status) in file_cases {
        let args = ["-f", "asciidoc", "-t", "json", name];
        let expected = run(&dir, &args, "")?;
        assert_eq!(expected.code, status, "{name}");
        let actual = convert(
            &dir,
            &args,
            Source {
                name: name.into(),
                text: content.into(),
            },
        )?;
        count += 1;
        passed += usize::from(compare(name, &expected, &actual, status == 0)?);
    }
    let read_cases = [
        (
            "row-local copies",
            "[cols=\"3*\"]\n|===\n|x 5*|y\n|z|w|u\n|===\n",
        ),
        ("copy styles", "[cols=\"e,s,d\"]\n|===\n3*|value\n|===\n"),
        ("copy heading IDs", "[cols=\"2*a\"]\n|===\n2*|== X\n|===\n"),
        (
            "paragraphs after a description",
            "a:: x\ny\n\np1\n\np2\n\nb:: z\n",
        ),
        (
            "headings in delimited blocks",
            "====\nStart\n== Sub\n====\n\n****\n= Later\n****\n\n--\n= Later\n--\n\n____\n== Sub\n____\n",
        ),
        (
            "headings in list continuations",
            "* a\n+\n= Later\n\nb:: y\n+\n== Sub\n",
        ),
        (
            "body attribute entries",
            "para\n\n:foo: a \\\n b\n:other: {foo}\n:flag:\n:stem:\n\n{foo} {other}\n",
        ),
        (
            "attribute entry after list continuation",
            "* a\n+\n:foo: bar\n\n{foo}\n",
        ),
        (
            "generated heading IDs",
            "== a.b-c_d/e:f(g)h+i#j&k\n\n== *Bold* `code` x.y\n\n== a . b\n\n== -lead\n\n== _\n\n== Hello, Élan!\n\n== Hello, Élan!\n",
        ),
        (
            "custom heading ID separator",
            ":idprefix:\n:idseparator: -\n\n== A b\n\n== A b\n",
        ),
        (
            "first named footnote",
            "a footnote:n[One] b footnote:n[Two] c footnote:n[] footnoteref:[n]\n",
        ),
        (
            "literal macro targets",
            ":f: p.png\n\nlink:{f}[x] xref:{f}[y] image:{f}[z]\n\nimage::{f}[]\n",
        ),
        ("zero-width space after dash", "x--y\n\n== x--y\n"),
    ];
    for (name, content) in read_cases {
        let args = ["-f", "asciidoc", "-t", "json"];
        let expected = run(&dir, &args, content)?;
        let actual = convert(
            &dir,
            &args,
            Source {
                name: String::new(),
                text: content.into(),
            },
        )?;
        count += 1;
        passed += usize::from(compare(name, &expected, &actual, true)?);
    }

    let content = "a|b and `k|+*`\n\n| Column |\n| ------ |\n| a\\|b |\n";
    let args = ["-f", "markdown", "-t", "asciidoc"];
    let expected = run(&dir, &args, content)?;
    let actual = convert(
        &dir,
        &args,
        Source {
            name: String::new(),
            text: content.into(),
        },
    )?;
    count += 1;
    passed += usize::from(compare(
        "vertical bars in writer output",
        &expected,
        &actual,
        false,
    )?);

    fs::remove_dir_all(&dir)?;
    eprintln!("Review probes: {passed}/{count}");
    assert_eq!(passed, count);
    Ok(())
}

#[test]
#[ignore = "Requires the Pandoc oracle."]
fn longtail_writer_parity() -> TestResult {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for dialect in DIALECTS {
        for source in [
            "1. one\n   - bullet\n     1. deep",
            "| line one\n|\n|   line two\n\n[under]{.underline}",
            "Inline ![alt](i.png){height=2in width=2.54cm} end.",
            "A ``one`two`` and $$x$$ end.",
            "[1]{.csl-left-margin}[Entry]{.csl-right-inline} and a[X]{.mark}b.",
        ] {
            let args = ["-f", "markdown", "-t", dialect, "--wrap=none"];
            assert_eq!(
                convert(
                    root,
                    &args,
                    Source {
                        name: String::new(),
                        text: source.into()
                    }
                )?,
                run(root, &args, source)?,
                "{dialect}: {source}",
            );
        }
    }
    Ok(())
}
