//! Compares Typst conversions with Pandoc 3.13.

use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use randoc_ast::{Pandoc, json};
use randoc_core::{
    Context, FormatSpec, HighlightMethod, Reader, ReaderOptions, Source, Sources, WrapOption,
    WriterOptions,
};
use randoc_templates::Template;
use serde_json::Value;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn oracle(dir: &Path, args: &[&str], input: &str) -> TestResult<String> {
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
        .ok_or_else(|| io::Error::other("Missing oracle input pipe."))?;
    let result = std::thread::scope(|scope| {
        let send = scope.spawn(move || stdin.write_all(input.as_bytes()));
        let result = child.wait_with_output();
        send.join()
            .map_err(|_| io::Error::other("Oracle input thread stopped."))??;
        result
    })?;
    if !result.status.success() {
        return Err(io::Error::other(String::from_utf8_lossy(&result.stderr).into_owned()).into());
    }
    Ok(String::from_utf8(result.stdout)?)
}

fn read(name: &str, input: String, format: &str, standalone: bool) -> TestResult<Pandoc> {
    let Some(Reader::Text(reader)) = randoc_typst::reader("typst") else {
        return Err(io::Error::other("Missing Typst reader.").into());
    };
    let mut opts = ReaderOptions::default();
    opts.extensions = FormatSpec::parse(format).extensions()?;
    opts.standalone = standalone;
    Ok(reader.read(
        &Sources(vec![Source {
            name: name.into(),
            text: input,
        }]),
        &opts,
        &mut Context::native(),
    )?)
}

fn writer_options(standalone: bool) -> TestResult<WriterOptions> {
    let mut opts = WriterOptions::default();
    opts.extensions = FormatSpec::parse("typst").extensions()?;
    if standalone {
        let template = randoc_data::default_template("typst")
            .ok_or_else(|| io::Error::other("Missing Typst template."))?;
        opts.template = Some(Template::compile(
            "default.typst",
            template,
            128,
            &mut |path| {
                randoc_data::data_file(&format!("templates/{path}"))
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        io::Error::other(format!("Missing template partial {path}.").as_str())
                    })
            },
        )?);
    }
    Ok(opts)
}

fn write(doc: &Pandoc, opts: &WriterOptions) -> TestResult<String> {
    let writer =
        randoc_typst::writer("typst").ok_or_else(|| io::Error::other("Missing Typst writer."))?;
    let mut text = String::from_utf8(writer.write(doc, opts, &mut Context::pure())?.into_bytes())?;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

fn text_diff(expected: &str, actual: &str) -> TestResult {
    let mismatch = expected
        .lines()
        .zip(actual.lines())
        .position(|(want, got)| want != got);
    if let Some(n) = mismatch {
        let snippet = |s: &str| {
            s.lines()
                .skip(n.saturating_sub(1))
                .take(6)
                .collect::<Vec<_>>()
                .join("\n")
        };
        return Err(io::Error::other(format!(
            "Line {}\nExpected\n{}\nActual\n{}",
            n + 1,
            snippet(expected),
            snippet(actual)
        ))
        .into());
    }
    if expected != actual {
        return Err(io::Error::other(format!(
            "Output lengths differ. Expected {} bytes. Got {} bytes.",
            expected.len(),
            actual.len()
        ))
        .into());
    }
    Ok(())
}

fn ast_diff(expected: &str, actual: &Pandoc) -> TestResult {
    let expected = json::from_json(expected)?;
    if expected == *actual {
        return Ok(());
    }
    if expected.meta != actual.meta {
        return Err(io::Error::other(format!(
            "Expected metadata {:?}. Got {:?}.",
            expected.meta, actual.meta
        ))
        .into());
    }
    let mut differences = Vec::new();
    block_differences(&expected.blocks, &actual.blocks, "", &mut differences);
    Err(io::Error::other(differences.join("\n")).into())
}

fn block_differences(
    expected: &[randoc_ast::Block],
    actual: &[randoc_ast::Block],
    path: &str,
    diff: &mut Vec<String>,
) {
    if expected.len() != actual.len() {
        diff.push(format!(
            "Block count at {path} is {}. Expected {}.",
            actual.len(),
            expected.len()
        ));
    }
    for (index, (want, got)) in expected.iter().zip(actual).enumerate() {
        if want == got {
            continue;
        }
        let path = format!("{path}/{index}");
        if let (randoc_ast::Block::Div(a, want), randoc_ast::Block::Div(b, got)) = (want, got)
            && a == b
        {
            block_differences(want, got, &path, diff);
        } else if diff.len() < 12 {
            let math = |b: &randoc_ast::Block| {
                let mut equations = Vec::new();
                randoc_ast::walk::query_inlines(b, |inline| {
                    if let randoc_ast::Inline::Math(_, tex) = inline {
                        equations.push(tex.clone());
                    }
                });
                equations
            };
            let expected_math = math(want);
            let actual_math = math(got);
            if expected_math.len() == actual_math.len() {
                for (w, g) in expected_math
                    .iter()
                    .zip(&actual_math)
                    .filter(|(w, g)| w != g)
                    .take(8)
                {
                    diff.push(format!("Math at {path} expected {w:?}. Got {g:?}."));
                }
            }
            let want = json::value_to_json(want);
            let got = json::value_to_json(got);
            let offset = want
                .bytes()
                .zip(got.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            let excerpt = |s: &str| {
                String::from_utf8_lossy(
                    &s.as_bytes()[offset.saturating_sub(65)..s.len().min(offset + 180)],
                )
                .into_owned()
            };
            diff.push(format!(
                "Block {path}, byte {offset}\nExpected {}\nActual {}",
                excerpt(&want),
                excerpt(&got)
            ));
        }
    }
}

fn command_case(dir: &Path, command: &str, input: &str) -> TestResult {
    let args: Vec<_> = command.split_whitespace().skip(1).collect();
    let format = |flag: &str, default| {
        args.windows(2)
            .find(|pair| pair[0] == flag)
            .map_or(default, |pair| pair[1])
    };
    let source = format("-f", "markdown");
    let target = format("-t", "native");
    let standalone = args.contains(&"-s");
    if target == "native" {
        let mut oracle_args: Vec<_> = args.iter().copied().filter(|arg| *arg != "-s").collect();
        if let Some(output) = oracle_args.iter_mut().find(|arg| **arg == "native") {
            *output = "json";
        }
        return ast_diff(
            &oracle(dir, &oracle_args, input)?,
            &read("", input.into(), source, standalone)?,
        );
    }
    let doc = if source.starts_with("typst") {
        read("", input.into(), source, standalone)?
    } else {
        json::from_json(&oracle(dir, &["-f", source, "-t", "json"], input)?)?
    };
    let mut opts = writer_options(standalone)?;
    for arg in &args {
        if let Some(style) = arg.strip_prefix("--syntax-highlighting=") {
            opts.highlight_method = match style {
                "none" => HighlightMethod::None,
                "idiomatic" => HighlightMethod::Idiomatic,
                "default" => HighlightMethod::Default,
                name => HighlightMethod::Style(name.into()),
            };
        }
    }
    text_diff(&oracle(dir, &args, input)?, &write(&doc, &opts)?)
}

fn fixture(dir: &Path, path: &str, standalone: bool, wrap: WrapOption) -> TestResult {
    let mut args = vec![
        "-f",
        "native",
        "-t",
        "typst",
        "--columns=80",
        "-V",
        "pandoc-version=",
        path,
    ];
    if standalone {
        args.push("-s");
    }
    if wrap == WrapOption::WrapNone {
        args.push("--wrap=none");
    }
    let expected = oracle(dir, &args, "")?;
    if wrap == WrapOption::WrapAuto {
        let fixture = if standalone {
            dir.join("writer.typst")
        } else {
            dir.join(path).with_extension("typst")
        };
        text_diff(&std::fs::read_to_string(fixture)?, &expected)?;
    }
    let doc = json::from_json(&oracle(dir, &["-f", "native", "-t", "json", path], "")?)?;
    let mut opts = writer_options(standalone)?;
    opts.columns = 80;
    opts.wrap_text = wrap;
    opts.variables.insert("pandoc-version", "");
    text_diff(&expected, &write(&doc, &opts)?)
}

fn record(name: &str, result: TestResult, failures: &mut Vec<String>) {
    if let Err(error) = result {
        eprintln!("FAIL {name}\n{error}");
        failures.push(name.into());
    }
}

#[test]
#[ignore = "Requires Pandoc 3.13, its test files, and the corpus"]
fn oracle_parity() -> TestResult {
    let dir = PathBuf::from(env::var("RANDOC_PANDOC_SRC")?).join("test");
    let tools = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/pandoc-testsuite");
    let discovery = Command::new("python3")
        .arg("-B")
        .arg("-c")
        .arg(
            r#"
import dataclasses, json, sys
sys.path.insert(0, sys.argv[1])
import suite
found = suite.Discovery()
suite.discover_commands(sys.argv[2], found)
print(json.dumps([dataclasses.asdict(case) for case in found.cases
                  if case.id.startswith("command/typst-")]))
"#,
        )
        .arg(tools)
        .arg(&dir)
        .output()?;
    if !discovery.status.success() {
        return Err(
            io::Error::other(String::from_utf8_lossy(&discovery.stderr).into_owned()).into(),
        );
    }
    let cases: Vec<Value> = serde_json::from_slice(&discovery.stdout)?;
    assert_eq!(cases.len(), 39);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["id"]
            .as_str()
            .ok_or_else(|| io::Error::other("Missing command case ID."))?;
        let command = case["command"]
            .as_str()
            .ok_or_else(|| io::Error::other("Missing command."))?;
        let stdin = case["stdin"]
            .as_str()
            .ok_or_else(|| io::Error::other("Missing command input."))?;
        record(name, command_case(&dir, command, stdin), &mut failures);
    }
    let fixture_name = dir.join("typst-reader.typ");
    let fixture_name = fixture_name.to_string_lossy();
    let doc = read(
        &fixture_name,
        std::fs::read_to_string(fixture_name.as_ref())?,
        "typst",
        false,
    );
    record(
        "old/typst/reader/typst-reader",
        doc.and_then(|doc| {
            ast_diff(
                &oracle(&dir, &["-f", "typst", "-t", "json", "typst-reader.typ"], "")?,
                &doc,
            )
        }),
        &mut failures,
    );
    for (name, file, standalone, wrap) in [
        ("basic", "testsuite.native", true, WrapOption::WrapAuto),
        ("basic#2", "testsuite.native", true, WrapOption::WrapNone),
        (
            "nordics table",
            "tables/nordics.native",
            false,
            WrapOption::WrapAuto,
        ),
        (
            "planets table",
            "tables/planets.native",
            false,
            WrapOption::WrapAuto,
        ),
        (
            "students table",
            "tables/students.native",
            false,
            WrapOption::WrapAuto,
        ),
        ("tables", "tables.native", false, WrapOption::WrapAuto),
        ("tables#2", "tables.native", false, WrapOption::WrapNone),
    ] {
        record(
            &format!("old/typst/writer/{name}"),
            fixture(&dir, file, standalone, wrap),
            &mut failures,
        );
    }
    let suite_failed = failures.len();
    let corpus = PathBuf::from(env::var("RANDOC_CORPUS")?);
    for name in [
        "appreciated-letter",
        "charged-ieee",
        "icicle",
        "wonderous-book",
    ] {
        let name = format!("typst-{name}-lib.typ");
        let file = corpus.join(&name);
        let doc = read(
            &file.to_string_lossy(),
            std::fs::read_to_string(&file)?,
            "typst",
            false,
        );
        record(
            &name,
            doc.and_then(|doc| {
                ast_diff(
                    &oracle(&corpus, &["-f", "typst", "-t", "json", &name], "")?,
                    &doc,
                )
            }),
            &mut failures,
        );
    }
    eprintln!(
        "Command and fixture parity {}/47. Corpus parity {}/4.",
        47 - suite_failed,
        4 - (failures.len() - suite_failed)
    );
    assert!(failures.is_empty(), "Failed cases {failures:?}");
    Ok(())
}

#[test]
#[ignore = "Requires Pandoc 3.13"]
fn review_oracle_parity() -> TestResult {
    use randoc_ast::{Block, Inline};
    use std::fmt::Write as _;

    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut functions = String::new();
    for index in 0..64 {
        writeln!(functions, "#let f{index}(x) = x")?;
    }
    functions.push_str("#f63[ok]");
    let mut lists = String::new();
    for level in 0..45 {
        writeln!(lists, "{}- x", "  ".repeat(level))?;
    }
    let emph = format!("{}x{}", "#emph[".repeat(60), "]".repeat(60));
    let sum = format!("#(1{})", " + 1".repeat(119));
    let mut failures = Vec::new();
    let reader_cases = [
        ("64 functions", functions.as_str()),
        ("60 emphasis levels", &emph),
        ("120 terms", &sum),
        ("45 list levels", &lists),
        (
            "30 recursive calls",
            "#let f(n) = if n == 0 { 0 } else { 1 + f(n - 1) }\n#f(30)",
        ),
        (
            "loop break",
            "#let i = 0\n#while true { i += 1; if i > 3 { break } }\n#i",
        ),
        (
            "loop continue",
            "#for i in range(3) { [x]; if i == 1 { continue }; [y] }",
        ),
        (
            "nested return",
            "#let f(t) = { if t { return [yes] }; [no] }\n#f(true)",
        ),
        ("string iteration", "#for c in \"aé😀\" [#c,]"),
        (
            "dictionary iteration",
            "#for (k, v) in (z: 1, a: 2) [#k=#v,]",
        ),
        ("heading label and reference", "= H\n<x>\nsee @x"),
        ("radix integers", "#(0x1F + 0o17 + 0b101)"),
    ];
    for (name, input) in reader_cases {
        record(
            name,
            (|| {
                let doc = read("", input.into(), "typst", false)?;
                ast_diff(&oracle(dir, &["-f", "typst", "-t", "json"], input)?, &doc)
            })(),
            &mut failures,
        );
    }
    let writer_cases = [
        (
            "ordered list and punctuation",
            "xx 12. y and z / w ; v ( u\n",
            "--columns=3",
            3,
            WrapOption::WrapAuto,
        ),
        (
            "list and heading markers",
            "aaa + b - c = d\n",
            "--columns=4",
            4,
            WrapOption::WrapAuto,
        ),
        (
            "preserved break",
            "a\n+ b\n",
            "--wrap=preserve",
            72,
            WrapOption::WrapPreserve,
        ),
        (
            "ordered list marker at line end",
            "xx\n12.\n",
            "--wrap=preserve",
            72,
            WrapOption::WrapPreserve,
        ),
    ];
    for (name, input, flag, columns, wrap) in writer_cases {
        record(
            name,
            (|| {
                let doc = json::from_json(&oracle(dir, &["-f", "markdown", "-t", "json"], input)?)?;
                let mut options = writer_options(false)?;
                options.columns = columns;
                options.wrap_text = wrap;
                text_diff(
                    &oracle(dir, &["-f", "markdown", "-t", "typst", flag], input)?,
                    &write(&doc, &options)?,
                )
            })(),
            &mut failures,
        );
    }
    record(
        "200 writer emphasis levels",
        (|| {
            let mut inline = Inline::Str("x".into());
            for _ in 0..200 {
                inline = Inline::Emph(vec![inline]);
            }
            let doc = Pandoc {
                blocks: vec![Block::Para(vec![inline])],
                ..Pandoc::default()
            };
            let writer = randoc_typst::writer("typst")
                .ok_or_else(|| io::Error::other("Missing Typst writer."))?;
            let mut context = Context::pure();
            context.limits.max_nesting_depth = 256;
            let output = writer.write(&doc, &writer_options(false)?, &mut context)?;
            let actual = String::from_utf8(output.into_bytes())?;
            let html = format!("{}x{}", "<em>".repeat(200), "</em>".repeat(200));
            let expected = oracle(dir, &["-f", "html", "-t", "typst"], &html)?;
            text_diff(expected.trim_end(), actual.trim_end())
        })(),
        &mut failures,
    );
    let total = reader_cases.len() + writer_cases.len() + 1;
    eprintln!(
        "Review regression parity {}/{total}.",
        total - failures.len()
    );
    assert!(failures.is_empty(), "Failed review cases {failures:?}");
    Ok(())
}
