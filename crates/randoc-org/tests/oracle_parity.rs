//! Compares Org conversions with Pandoc 3.13.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use randoc_ast::{Pandoc, json};
use randoc_core::{
    Context, Reader, ReaderOptions, Sources, Writer, WriterOptions, default_extensions,
};
use randoc_templates::Template;
use serde_json::Value;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn oracle(args: &[&str], input: &str) -> TestResult<String> {
    let mut child = Command::new(env::var("RANDOC_ORACLE_PANDOC")?)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut pipe = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("Missing oracle stdin."))?;
    let result = std::thread::scope(|scope| {
        let send = scope.spawn(move || pipe.write_all(input.as_bytes()));
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

fn read_org(input: &str, smart: bool, context: &mut Context) -> TestResult<Pandoc> {
    let Some(Reader::Text(reader)) = randoc_org::reader("org") else {
        return Err(io::Error::other("Missing Org reader.").into());
    };
    let mut options = ReaderOptions::default();
    options.extensions = default_extensions("org");
    if smart {
        options.extensions.enable(randoc_core::Extension::Smart);
    }
    Ok(reader.read(&Sources::from_text(input), &options, context)?)
}

fn read_native(input: &str) -> TestResult<Pandoc> {
    let Some(Reader::Text(reader)) = randoc_native::reader("native") else {
        return Err(io::Error::other("Missing native reader.").into());
    };
    Ok(reader.read(
        &Sources::from_text(input),
        &ReaderOptions::default(),
        &mut Context::pure(),
    )?)
}

fn write_org(doc: &Pandoc, standalone: bool) -> TestResult<String> {
    write_org_with_wrap(doc, standalone, randoc_core::WrapOption::WrapAuto)
}

fn write_org_with_wrap(
    doc: &Pandoc,
    standalone: bool,
    wrap: randoc_core::WrapOption,
) -> TestResult<String> {
    let Some(Writer::Text(writer)) = randoc_org::writer("org") else {
        return Err(io::Error::other("Missing Org writer.").into());
    };
    let mut options = WriterOptions::default();
    options.extensions = default_extensions("org");
    options.wrap_text = wrap;
    if standalone {
        options.columns = 80;
        let source = randoc_data::default_template("org")
            .ok_or_else(|| io::Error::other("Missing Org template."))?;
        options.template = Some(Template::compile(
            "default.org",
            source,
            128,
            &mut |path| Err(io::Error::other(format!("Missing partial {path}."))),
        )?);
    }
    let mut text = writer.write(doc, &options, &mut Context::pure())?;
    if !standalone {
        text.push('\n');
    }
    Ok(text)
}

fn difference(expected: &Value, actual: &Value, path: &str) -> Option<String> {
    if expected == actual {
        return None;
    }
    match (expected, actual) {
        (Value::Array(a), Value::Array(b)) => {
            for (i, (left, right)) in a.iter().zip(b).enumerate() {
                if let Some(diff) = difference(left, right, &format!("{path}[{i}]")) {
                    return Some(diff);
                }
            }
            Some(format!(
                "{path} has {} items. Expected {}.",
                b.len(),
                a.len()
            ))
        }
        (Value::Object(a), Value::Object(b)) if a.keys().eq(b.keys()) => {
            let diff = a
                .iter()
                .find_map(|(key, left)| difference(left, &b[key], &format!("{path}.{key}")));
            if a.get("t") != b.get("t") {
                return diff.map(|diff| {
                    format!(
                        "{diff} Context expected {} and got {}.",
                        brief(&expected.to_string()),
                        brief(&actual.to_string())
                    )
                });
            }
            diff
        }
        _ => Some(format!(
            "{path} expected {} and got {}.",
            brief(&expected.to_string()),
            brief(&actual.to_string())
        )),
    }
}

fn brief(text: &str) -> String {
    text.chars().take(210).collect()
}

fn check_reader(name: &str, input: &str) -> TestResult<(bool, Pandoc, Pandoc)> {
    let expected = oracle(&["-f", "org", "-t", "json"], input)?;
    let actual = read_org(input, false, &mut Context::pure())?;
    let fix = json::to_json(&actual);
    let diff = difference(
        &serde_json::from_str(&expected)?,
        &serde_json::from_str(&fix)?,
        "$",
    );
    if let Some(diff) = &diff {
        eprintln!("{name} reader {diff}");
    }
    Ok((diff.is_none(), json::from_json(&expected)?, actual))
}

fn check_writer(name: &str, expected: &str, doc: &Pandoc, standalone: bool) -> TestResult<bool> {
    let actual = write_org(doc, standalone)?;
    if actual == expected {
        return Ok(true);
    }
    let mismatch = expected
        .split('\n')
        .zip(actual.split('\n'))
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| expected.split('\n').count().min(actual.split('\n').count()));
    eprintln!(
        "{name} writer line {} expected {:?} and got {:?}.",
        mismatch + 1,
        expected.split('\n').nth(mismatch),
        actual.split('\n').nth(mismatch),
    );
    Ok(false)
}

#[test]
#[ignore = "Requires the sourced Randoc environment and the Pandoc 3.13 oracle."]
fn oracle_parity() -> TestResult {
    let corpus = env::var("RANDOC_CORPUS")?;
    let fixtures = Path::new(&env::var("RANDOC_PANDOC_SRC")?).join("test");
    let mut documents = fs::read_dir(&corpus)?.collect::<io::Result<Vec<_>>>()?;
    documents.retain(|file| {
        file.file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("doom-") && name.ends_with(".org"))
    });
    documents.sort_by_key(|file| file.file_name());
    assert_eq!(documents.len(), 9);
    let mut corpus_pass = 0;
    let mut reader_pass = 0;
    let mut writer_pass = 0;
    let mut roundtrip_pass = 0;
    for file in documents {
        let name = file.file_name().to_string_lossy().into_owned();
        let input = fs::read_to_string(file.path())?;
        let (reader_ok, oracle_doc, actual_doc) = check_reader(&name, &input)?;
        let expected = oracle(&["-f", "org", "-t", "org"], &input)?;
        let writer_ok = check_writer(&name, &expected, &oracle_doc, false)?;
        let roundtrip_ok = write_org(&actual_doc, false)? == expected;
        reader_pass += usize::from(reader_ok);
        writer_pass += usize::from(writer_ok);
        roundtrip_pass += usize::from(roundtrip_ok);
        corpus_pass += usize::from(reader_ok && writer_ok);
        eprintln!("{name} reader={reader_ok} writer={writer_ok}");
    }
    let mut suite_pass = 0;
    let input = fs::read_to_string(fixtures.join("org-select-tags.org"))?;
    let (passed, _, _) = check_reader("old/org/reader", &input)?;
    suite_pass += usize::from(passed);
    for (name, file, standalone) in [
        ("old/org/writer/basic", "testsuite.native", true),
        ("old/org/writer/tables", "tables.native", false),
    ] {
        let input = fs::read_to_string(fixtures.join(file))?;
        let mut args = vec!["-f", "native", "-t", "org", "--columns=80"];
        if standalone {
            args.push("-s");
        }
        let expected = oracle(&args, &input)?;
        suite_pass += usize::from(check_writer(
            name,
            &expected,
            &read_native(&input)?,
            standalone,
        )?);
    }
    let figures = fs::read_to_string(fixtures.join("command/figures-org.md"))?;
    let mut figures_count = 0;
    for snippet in figures
        .split("```")
        .filter(|part| part.contains("% pandoc"))
    {
        let (input, _) = snippet
            .split_once("\n^D\n")
            .ok_or_else(|| io::Error::other("Missing command fixture separator."))?;
        let input = input
            .trim_start()
            .split_once('\n')
            .map_or(input, |(_, body)| body);
        let expected = oracle(&["-f", "native", "-t", "org"], input)?;
        figures_count += 1;
        suite_pass += usize::from(check_writer(
            &format!("command/figures-org.md#{figures_count}"),
            &expected,
            &read_native(input)?,
            false,
        )?);
    }
    assert_eq!(figures_count, 2);
    eprintln!(
        "Corpus {corpus_pass}/9. Reader {reader_pass}/9. Writer {writer_pass}/9. Round trip {roundtrip_pass}/9."
    );
    eprintln!("Pandoc testsuite {suite_pass}/5.");
    assert_eq!((corpus_pass, suite_pass), (9, 5));
    Ok(())
}

#[test]
#[ignore = "Requires the sourced Randoc environment and the Pandoc 3.13 oracle."]
fn review_reader_parity() -> TestResult {
    let cases = [
        (
            "H1",
            "#+LINK: a https://x/%h-%s-%s\n#+LINK: b https://x/%h-%h\n[[a:xyz][One]] [[b:a /é?][Two]]",
        ),
        ("H2", "x [cite: [cite: [cite: @]]] y"),
        (
            "H3",
            "See [fn:x] twice [fn:x].\n\n[fn:x] Note.\n\n#+TITLE: Title\nSecond paragraph.\n\n\nOutside.",
        ),
        ("H3 unused", "[fn:x] Note.\n\n#+TITLE: Outside\n"),
        (
            "M1",
            "See [fn:x] and [[hidden]].\n\n~[fn:x: literal]~ ~<<hidden>>~\n\n#+begin_src org\n* Fake :export:\n#+end_src\n* Real",
        ),
        (
            "M1 multiline",
            "See [fn:x] and [[hidden]].\n\nHere ~literal\n[fn:x: private] <<hidden>>~.",
        ),
        (
            "M2 source",
            "#+begin_src sh\n  echo hi\n  #+begin_src test\n#+end_src",
        ),
        (
            "M2 quote",
            "#+begin_quote\n#+begin_quote\nbody\n#+end_quote\n#+end_quote",
        ),
        (
            "M3",
            r"[[https://x][hello [world]]] [[https://x][]] [[https://x/a\[b\]][Label]]",
        ),
        (
            "M4",
            r"Text \alpha2 \alpha{}2 \sin2 \sin{}2 \implies \leq2 \therefore2 \cos2 \foo2 \frac12",
        ),
        (
            "M5 missing",
            "#+include: \"randoc-review-missing.org\" src sh\n#+include: \"randoc-review-missing.org\" example\n#+include: \"randoc-review-missing.org\" export html",
        ),
        ("M6", "| a |\n| b | c | d |"),
        (
            "M7",
            "#+begin_src sh\n  before\n  ,#+end_src\n  ,,,* head\n#+end_src",
        ),
        (
            "L1 late",
            "Intro.\n\n:PROPERTIES:\n:FOO: bar\n:END:\n\n* :foo:\n* TODO :foo:",
        ),
        (
            "L1 leading",
            "# comment\n:PROPERTIES:\n:FOO: bar\n:END:\n\nIntro.",
        ),
        ("L2", "- term :: [X] done\n\n1. [X] done"),
        (
            "L3",
            "#+begin_verse\n  First\n    Second\n\n  Third\n#+end_verse",
        ),
        ("L5", "| <r> | <20> |"),
    ];
    let mut failures = Vec::new();
    for (name, input) in cases {
        if !check_reader(name, input)?.0 {
            failures.push(name);
        }
    }
    let input = r#""a"b"#;
    let expected = oracle(&["-f", "org+smart", "-t", "json"], input)?;
    let actual = read_org(input, true, &mut Context::pure())?;
    assert_eq!(actual, json::from_json(&expected)?, "M8");
    assert!(failures.is_empty(), "{failures:?}");
    Ok(())
}

#[test]
#[ignore = "Requires the sourced Randoc environment and the Pandoc 3.13 oracle."]
fn review_include_parity() -> TestResult {
    let temp = Path::new(&env::var("RANDOC_ENV")?).join("tmp");
    let file = temp.join(format!("org-review-include-{}", std::process::id()));
    let cases: &[(&str, &[u8], &str)] = &[
        (
            "H4",
            b"line one\nline two\nline three\n",
            "src text :lines \"2-3\"",
        ),
        ("M5", b"a\xffb\n", "src sh"),
    ];
    for (name, bytes, args) in cases {
        fs::write(&file, bytes)?;
        let input = format!("#+include: \"{}\" {args}", file.display());
        let result = (|| -> TestResult {
            let expected = oracle(&["-f", "org", "-t", "json"], &input)?;
            let actual = read_org(&input, false, &mut Context::native())?;
            assert_eq!(actual, json::from_json(&expected)?, "{name}");
            Ok(())
        })();
        fs::remove_file(&file)?;
        result?;
    }
    Ok(())
}

#[test]
#[ignore = "Requires the sourced Randoc environment and the Pandoc 3.13 oracle."]
fn review_writer_parity() -> TestResult {
    use randoc_core::WrapOption;

    let table = "| wide |\n| x |\n| x |";
    let expected = oracle(&["-f", "org", "-t", "org"], table)?;
    assert!(check_writer(
        "H5",
        &expected,
        &read_org(table, false, &mut Context::pure())?,
        false
    )?);
    let cases = [
        (
            "M7",
            r#"[CodeBlock ("",["bash"],[]) "  * head\n  ,* escaped\n  ,,#+abc\n  #+mark"]"#,
        ),
        ("L3", r#"[LineBlock [[Str "First"],[Str "Second"],[]]]"#),
    ];
    for (name, input) in cases {
        let expected = oracle(&["-f", "native", "-t", "org"], input)?;
        assert!(check_writer(name, &expected, &read_native(input)?, false)?);
    }

    let native =
        r#"[Para [Str "a",SoftBreak,Str "-",Space,Str "b",SoftBreak,Str "1.",Space,Str "c"]]"#;
    let expected = oracle(&["-f", "native", "-t", "org", "--wrap=preserve"], native)?;
    assert_eq!(
        write_org_with_wrap(&read_native(native)?, false, WrapOption::WrapPreserve)?,
        expected
    );

    let native = r#"[Para [Link ("",[],[]) [Str "Label"] ("https://x/a[b]","")]]"#;
    let input = write_org(&read_native(native)?, false)?;
    assert!(check_reader("M3 writer round trip", &input)?.0);

    Ok(())
}
