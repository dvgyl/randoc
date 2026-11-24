//! Compare roff conversions with Pandoc 3.13 and the recorded baseline.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use randoc_ast::Pandoc;
use randoc_core::{Context, FormatSpec, Reader, ReaderOptions, Source, Sources, WriterOptions};
use randoc_templates::{Template, Val};
use serde_json::Value;

struct Oracle {
    binary: PathBuf,
    suite: PathBuf,
    repo: PathBuf,
}

impl Oracle {
    fn new() -> Self {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let oracle = Self {
            binary: env_path("RANDOC_ORACLE_PANDOC"),
            suite: env_path("RANDOC_PANDOC_SRC").join("test"),
            repo,
        };
        let version = oracle.run(&["--version"], "");
        assert!(String::from_utf8_lossy(&version.stdout).starts_with("pandoc 3.13\n"));
        oracle
    }

    fn run(&self, flags: &[&str], input: &str) -> Output {
        let mut process = Command::new(&self.binary)
            .args(flags)
            .current_dir(&self.suite)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = process.stdin.take().unwrap();
        std::thread::scope(|scope| {
            let send = scope.spawn(move || stdin.write_all(input.as_bytes()));
            let output = process.wait_with_output().unwrap();
            if output.status.success() {
                send.join().unwrap().unwrap();
            }
            output
        })
    }

    fn json(&self, format: &str, input: &str) -> Pandoc {
        let output = self.run(&["-f", format, "-t", "json"], input);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        randoc_ast::json::from_json(std::str::from_utf8(&output.stdout).unwrap()).unwrap()
    }
}

fn env_path(key: &str) -> PathBuf {
    PathBuf::from(
        std::env::var_os(key).unwrap_or_else(|| panic!("Set {key} before running this test.")),
    )
}

fn read(format: &str, text: &str, name: &str, ctx: &mut Context) -> randoc_core::Result<Pandoc> {
    let spec = FormatSpec::parse(format);
    let Reader::Text(reader) = randoc_roff::reader(&spec.name).unwrap() else {
        panic!("The reader must accept text.");
    };
    let mut options = ReaderOptions::default();
    options.extensions = spec.extensions().unwrap();
    reader.read(
        &Sources(vec![Source {
            name: name.into(),
            text: text.into(),
        }]),
        &options,
        ctx,
    )
}

fn write(format: &str, doc: &Pandoc, flags: &[&str]) -> String {
    let mut options = WriterOptions::default();
    for (i, flag) in flags.iter().enumerate() {
        match *flag {
            "--ascii" => options.prefer_ascii = true,
            "--columns=80" => options.columns = 80,
            "-s" | "--standalone" => {
                let template = randoc_data::default_template(format).unwrap();
                options.template = Some(
                    Template::compile::<()>(
                        &format!("default.{format}"),
                        template,
                        128,
                        &mut |_| Err(()),
                    )
                    .unwrap(),
                );
            }
            flag if flag.starts_with("-V") => {
                let value = flag.strip_prefix("-V").unwrap();
                let value = if value.is_empty() {
                    flags[i + 1]
                } else {
                    value
                };
                let (name, value) = value.split_once('=').unwrap();
                options.variables.insert(name, Val::text(value));
            }
            _ => {}
        }
    }
    let writer = randoc_roff::writer(format).unwrap();
    let output = writer.write(doc, &options, &mut Context::pure()).unwrap();
    let bytes = output.into_bytes();
    let text = std::str::from_utf8(&bytes).unwrap();
    if text.is_empty() || text.ends_with('\n') {
        text.into()
    } else {
        format!("{text}\n")
    }
}

fn compare_text(name: &str, actual: &str, expected: &str) -> bool {
    if actual == expected {
        return true;
    }
    let offset = actual
        .bytes()
        .zip(expected.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let snippet = |text: &str| {
        String::from_utf8_lossy(
            &text.as_bytes()[offset.saturating_sub(60)..text.len().min(offset + 180)],
        )
        .into_owned()
    };
    eprintln!(
        "{name}, first difference at byte {offset}\n  Actual {:?}\nExpected {:?}",
        snippet(actual),
        snippet(expected)
    );
    false
}

fn compare_doc(name: &str, actual: &Pandoc, expected: &Pandoc) -> bool {
    if actual == expected {
        return true;
    }
    for (index, (a, b)) in actual.blocks.iter().zip(&expected.blocks).enumerate() {
        if a != b {
            let a = randoc_ast::json::value_to_json(a);
            let b = randoc_ast::json::value_to_json(b);
            return compare_text(&format!("{name}, block {index}"), &a, &b);
        }
    }
    compare_text(
        name,
        &randoc_ast::json::to_json(actual),
        &randoc_ast::json::to_json(expected),
    )
}

fn commands(oracle: &Oracle) -> (usize, usize) {
    let script = r#"
import json, shlex, sys
sys.path.insert(0, sys.argv[1])
import suite
found = suite.Discovery()
suite.discover_commands(sys.argv[2], found)
formats = {'man', 'mdoc', 'ms'}
for case in found.cases:
    command = shlex.split(case.command)
    if not command or command[0] != 'pandoc' or '|' in command:
        continue
    def option(flag, default):
        return command[command.index(flag) + 1] if flag in command else default
    source = option('-f', 'markdown')
    target = option('-t', 'html')
    if source.split('+')[0].split('-')[0] in formats or target in formats:
        print(json.dumps([case.id, command[1:], source, target, case.stdin, case.expected]))
"#;
    let output = Command::new("python3")
        .args(["-B", "-c", script])
        .arg(oracle.repo.join("tools/pandoc-testsuite"))
        .arg(&oracle.suite)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut matched = 0;
    let mut total = 0;
    for line in std::str::from_utf8(&output.stdout).unwrap().lines() {
        let (name, command, from, to, input, golden): (
            String,
            Vec<String>,
            String,
            String,
            String,
            String,
        ) = serde_json::from_str(line).unwrap();
        total += 1;
        let flags = command.iter().map(String::as_str).collect::<Vec<_>>();
        let expected = oracle.run(&flags, &input);
        assert!(expected.status.success(), "{name}");
        let expected = std::str::from_utf8(&expected.stdout).unwrap();
        assert_eq!(
            expected.trim_end_matches('\n'),
            golden.trim_end_matches('\n'),
            "{name}"
        );
        let passed = if matches!(to.as_str(), "man" | "ms") {
            compare_text(
                &name,
                &write(&to, &oracle.json(&from, &input), &flags),
                expected,
            )
        } else {
            match read(&from, &input, "", &mut Context::pure()) {
                Ok(doc) => compare_doc(&name, &doc, &oracle.json(&from, &input)),
                Err(err) => {
                    eprintln!("{name}: {err}");
                    false
                }
            }
        };
        matched += usize::from(passed);
    }
    (matched, total)
}

fn legacy(oracle: &Oracle) -> usize {
    let mut passed = 0;
    let input = fs::read_to_string(oracle.suite.join("man-reader.man")).unwrap();
    let expected = fs::read_to_string(oracle.suite.join("man-reader.native")).unwrap();
    let expected = oracle.json("native", &expected);
    let actual = read("man", &input, "", &mut Context::pure()).unwrap();
    passed += usize::from(compare_doc("old/man/reader", &actual, &expected));
    for (format, input, target, standalone) in [
        ("man", "testsuite.native", "writer.man", true),
        ("man", "tables.native", "tables.man", false),
        ("ms", "testsuite.native", "writer.ms", true),
        ("ms", "tables.native", "tables.ms", false),
    ] {
        let mut flags = vec!["--columns=80", "-Vpandoc-version="];
        if standalone {
            flags.push("-s");
            if format == "ms" {
                flags.push("-Vpdf-engine=pdfroff");
            }
        }
        let input = fs::read_to_string(oracle.suite.join(input)).unwrap();
        let expected = fs::read_to_string(oracle.suite.join(target)).unwrap();
        let actual = write(format, &oracle.json("native", &input), &flags);
        passed += usize::from(compare_text(target, &actual, &expected));
    }
    passed
}

fn corpus(oracle: &Oracle) -> usize {
    let corpus = env_path("RANDOC_CORPUS");
    let mut passed = 0;
    for name in [
        "man-jq",
        "man-lz4",
        "man-pandoc-citeproc",
        "man-zstd",
        "openbsd-doas",
        "openbsd-httpd-conf",
        "openbsd-ksh",
        "openbsd-mg",
        "openbsd-ssh",
        "openbsd-tmux",
    ] {
        let path = corpus.join(format!("{name}.1"));
        let path = path.to_str().unwrap();
        let format = if name.starts_with("man-") {
            "man"
        } else {
            "mdoc"
        };
        let expected = oracle.run(&["-f", format, "-t", "json", path], "");
        let input = fs::read_to_string(path).unwrap();
        let mut ctx = Context::pure();
        let actual = read(format, &input, path, &mut ctx);
        let ok = match actual {
            Ok(doc) if expected.status.success() => {
                let other =
                    randoc_ast::json::from_json(std::str::from_utf8(&expected.stdout).unwrap())
                        .unwrap();
                ctx.log().is_empty()
                    && expected.stderr.is_empty()
                    && compare_doc(name, &doc, &other)
            }
            Err(err) if expected.status.code() == Some(err.exit_code()) => {
                expected.stdout.is_empty()
                    && compare_text(
                        name,
                        &format!("{err}\n"),
                        std::str::from_utf8(&expected.stderr).unwrap(),
                    )
            }
            other => {
                eprintln!(
                    "{name}: expected exit {}, Randoc result {}",
                    expected.status,
                    if let Err(err) = other {
                        err.to_string()
                    } else {
                        "success".into()
                    }
                );
                false
            }
        };
        passed += usize::from(ok);
    }
    passed
}

fn conformance(oracle: &Oracle) -> (usize, usize) {
    let output = Command::new("python3")
        .args([
            "-B",
            "-c",
            "
import json, sys, tomllib
with open(sys.argv[1], 'rb') as handle:
    print(json.dumps(tomllib.load(handle)['case']))
",
        ])
        .arg(
            oracle
                .repo
                .join("conformance/cases/reader-baseline/mdoc.toml"),
        )
        .output()
        .unwrap();
    assert!(output.status.success());
    let cases: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    let expected: Value = serde_json::from_slice(
        &fs::read(
            oracle
                .repo
                .join("conformance/expected/reader-baseline/mdoc.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(cases.len(), 40);
    let mut exact = 0;
    let mut controlled = 0;
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let key = format!("{id}#read");
        let input = case["input"].as_str().unwrap();
        let reference = &expected["units"][&key];
        let output = oracle.run(&["-f", "mdoc", "-t", "json"], input);
        assert_eq!(
            output.status.code(),
            Some(reference["exit"].as_i64().unwrap() as i32),
            "{id}"
        );
        assert_eq!(
            output.stdout,
            reference["stdout"].as_str().unwrap().as_bytes(),
            "{id}"
        );
        assert_eq!(
            output.stderr,
            reference["stderr"].as_str().unwrap().as_bytes(),
            "{id}"
        );
        let mut ctx = Context::pure();
        let actual = read("mdoc", input, "", &mut ctx);
        // Pandoc 3.13 exits with an internal parser error for these final sections.
        if matches!(
            id,
            "synopsis-pump" | "synopsis-valve" | "synopsis-filter" | "synopsis-gauge"
        ) {
            assert_eq!(output.status.code(), Some(1), "{id}");
            assert!(output.stdout.is_empty(), "{id}");
            let with_next_section = format!("{input}\n.Sh DESCRIPTION\n");
            let mut reference = oracle.json("mdoc", &with_next_section);
            assert!(matches!(
                reference.blocks.pop(),
                Some(randoc_ast::Block::Header(1, ..))
            ));
            assert!(ctx.log().is_empty());
            assert!(compare_doc(id, &actual.unwrap(), &reference));
            controlled += 1;
        } else {
            let actual = actual.unwrap();
            let reference =
                randoc_ast::json::from_json(std::str::from_utf8(&output.stdout).unwrap()).unwrap();
            exact += usize::from(ctx.log().is_empty() && compare_doc(id, &actual, &reference));
        }
    }
    (exact, controlled)
}

#[test]
#[ignore = "Requires Pandoc 3.13, its text fixtures, and the local corpus."]
fn oracle_parity() {
    let oracle = Oracle::new();
    let (commands, count) = commands(&oracle);
    assert_eq!(count, 29);
    let legacy = legacy(&oracle);
    let corpus = corpus(&oracle);
    let (mdoc, controlled) = conformance(&oracle);
    eprintln!(
        "Testsuite {}/34 ({commands}/29 commands and {legacy}/5 legacy fixtures)",
        commands + legacy
    );
    eprintln!("Corpus {corpus}/10");
    eprintln!("Mdoc conformance {mdoc}/40 exact and {controlled} documented oracle crashes");
    check_man_regressions(&oracle);
    check_ms_highlighting(&oracle);
    check_synopsis_subsection(&oracle);
    assert_eq!(
        (commands, legacy, corpus, mdoc, controlled),
        (29, 5, 10, 36, 4)
    );
}

fn check_man_regressions(oracle: &Oracle) {
    let page = "one line of text\n".repeat(300);
    let mut host = randoc_core::PureHost::default();
    host.files
        .insert("./big.man".into(), page.as_bytes().to_vec());
    let mut ctx = Context::new(Box::new(host));
    let actual = read("man", ".so big.man\n", "", &mut ctx).unwrap();
    assert!(ctx.log().is_empty());
    assert!(compare_doc(
        "man include stub",
        &actual,
        &oracle.json("man", &page)
    ));

    let input = ".TH X 1\n.TS\nc c\nl l.\nA\tB\n1\t2\n.TE\n";
    let actual = read("man", input, "", &mut Context::pure()).unwrap();
    assert!(compare_doc(
        "man multiline tbl format",
        &actual,
        &oracle.json("man", input)
    ));
}

fn check_ms_highlighting(oracle: &Oracle) {
    let flags = ["-s", "-Vpandoc-version="];
    let input = "A value of $x^2$.\n\n```rust\nlet n = 1;\n```\n";
    let expected = oracle.run(&["-t", "ms", flags[0], flags[1]], input);
    assert!(expected.status.success() && expected.stderr.is_empty());
    assert!(compare_text(
        "ms with math and syntax highlighting",
        &write("ms", &oracle.json("markdown", input), &flags),
        std::str::from_utf8(&expected.stdout).unwrap(),
    ));

    // The default registry highlights Python, as the oracle does.
    let input = "```python\nprint(1)\n```\n";
    let expected = oracle.run(&["-t", "ms"], input);
    assert!(expected.status.success() && expected.stderr.is_empty());
    assert!(compare_text(
        "ms with Python highlighting",
        &write("ms", &oracle.json("markdown", input), &[]),
        std::str::from_utf8(&expected.stdout).unwrap(),
    ));
}

fn check_synopsis_subsection(oracle: &Oracle) {
    let input = ".Sh SYNOPSIS\n.Nm x\n.Ss Sub\ntext\n";
    let expected = oracle.run(&["-f", "mdoc", "-t", "json"], input);
    assert_eq!(expected.status.code(), Some(64));
    assert!(expected.stdout.is_empty());
    let error = read("mdoc", input, "", &mut Context::pure()).unwrap_err();
    assert_eq!(error.exit_code(), 64);
    let diagnostic = std::str::from_utf8(&expected.stderr).unwrap();
    assert_eq!(format!("{error}\n"), diagnostic);
}

#[test]
#[ignore = "Requires the Pandoc 3.13 executable."]
fn predefined_strings_and_spanned_tables() {
    let oracle = Oracle::new();
    let source = concat!(
        ".TH X 1\n.PP\n",
        r#"Say \*(lqhi\*(rq and \*(Tm and \*[lq]x\*[rq] \*R. [\*S] [\*(HF]"#,
        "\n",
        r#".BR \-v ", " \-\-verbose"#,
        "\n",
    );
    let actual = read("man", source, "", &mut Context::pure()).unwrap();
    assert_eq!(actual, oracle.json("man", source));

    let source = concat!(
        "<table><thead><tr><th rowspan='2'>A</th><th>B</th></tr>",
        "<tr><th>C</th></tr></thead><tbody>",
        "<tr><td colspan='2'><p>one</p><p>two</p></td></tr>",
        "</tbody></table>"
    );
    let doc = oracle.json("html", source);
    for format in ["man", "ms"] {
        let output = oracle.run(&["-f", "html", "-t", format], source);
        assert!(output.status.success());
        let expected = String::from_utf8(output.stdout).unwrap();
        assert!(compare_text(format, &write(format, &doc, &[]), &expected));
    }
}

#[test]
#[ignore = "Requires the Pandoc 3.13 executable."]
fn medium_highlighting_and_ms_bibliography() {
    let oracle = Oracle::new();
    for (finding, format, input) in [
        (
            "M1 man block",
            "man",
            "~~~python\ndef f(x):\n    return x # note\n~~~",
        ),
        ("M1 ms inline", "ms", "`x = 1`{.python} and `plain`"),
        (
            "M1 ms small caps",
            "ms",
            "[Small CAPS *mixed* foo-bar]{.smallcaps}",
        ),
        (
            "W10-W11 ms bibliography",
            "ms",
            concat!(
                "::: {.references .csl-bib-body .hanging-indent entry-spacing=1 line-spacing=2}\n",
                "::: {#ref-a .csl-entry}\nFirst. Second.\n\nThird. Fourth.\n:::\n",
                "::: {#ref-b .csl-entry}\n[2 ]{.csl-left-margin}[Fifth. Sixth.]{.csl-right-inline}\n:::\n",
                ":::\n"
            ),
        ),
    ] {
        let doc = oracle.json("markdown", input);
        let expected = oracle.run(&["-t", format], input);
        assert!(expected.status.success());
        assert!(compare_text(
            finding,
            &write(format, &doc, &[]),
            std::str::from_utf8(&expected.stdout).unwrap(),
        ));
    }
}
