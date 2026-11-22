//! Compare the RTF crate with the pinned Pandoc fixtures and corpus.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use randoc_ast::{Block, Cell, ColSpec, ColWidth, Inline, Pandoc, Row, Table, TableBody};
use randoc_core::{Context, PureHost, Reader, ReaderOptions, Sources, WriterOptions};
use randoc_templates::{Template, Val};
use serde_json::Value;

fn run_process(oracle: &Path, suite: &Path, flags: &[&str], bytes: &[u8]) -> Output {
    let mut process = Command::new(oracle)
        .args(flags)
        .current_dir(suite)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Start the Pandoc executable");
    process.stdin.take().unwrap().write_all(bytes).unwrap();
    process.wait_with_output().unwrap()
}

fn run(oracle: &Path, suite: &Path, flags: &[&str], bytes: &[u8]) -> Vec<u8> {
    let result = run_process(oracle, suite, flags, bytes);
    assert!(
        result.status.success(),
        "{flags:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}

fn read(reader: Reader, bytes: &[u8]) -> Pandoc {
    let Reader::Text(reader) = reader else {
        panic!("Text reader required")
    };
    let input = match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().copied().map(char::from).collect(),
    };
    reader
        .read(
            &Sources::from_text(input),
            &ReaderOptions::default(),
            &mut Context::pure(),
        )
        .unwrap()
}

fn read_rtf(bytes: &[u8]) -> Pandoc {
    read(randoc_rtf::reader("rtf").unwrap(), bytes)
}

fn read_native(bytes: &[u8]) -> Pandoc {
    read(randoc_native::reader("native").unwrap(), bytes)
}

fn write_rtf(doc: &Pandoc, opts: &WriterOptions, ctx: &mut Context) -> Vec<u8> {
    randoc_rtf::writer("rtf")
        .unwrap()
        .write(doc, opts, ctx)
        .unwrap()
        .into_bytes()
}

fn compare_json(actual: &Value, expected: &Value, name: &str) -> bool {
    fn first_difference(a: &Value, b: &Value, path: &mut String) -> Option<String> {
        match (a, b) {
            (Value::Array(a), Value::Array(b)) => {
                for (index, (a, b)) in a.iter().zip(b).enumerate() {
                    path.push_str(&format!("[{index}]"));
                    if let Some(diff) = first_difference(a, b, path) {
                        return Some(diff);
                    }
                    path.truncate(path.rfind('[').unwrap());
                }
                if a.len() != b.len() {
                    return Some(format!(
                        "{path}: length {} vs {}, next {:?} vs {:?}",
                        a.len(),
                        b.len(),
                        a.get(b.len()).map(short),
                        b.get(a.len()).map(short)
                    ));
                }
            }
            (Value::Object(a), Value::Object(b)) if a.keys().eq(b.keys()) => {
                if a.get("t") != b.get("t") {
                    let a = short(&Value::Object(a.clone()));
                    let b = short(&Value::Object(b.clone()));
                    return Some(format!("{path}: {a} vs {b}"));
                }
                for (key, value) in a {
                    let length = path.len();
                    path.push('.');
                    path.push_str(key);
                    if let Some(diff) = first_difference(value, &b[key], path) {
                        return Some(diff);
                    }
                    path.truncate(length);
                }
            }
            _ if a != b => return Some(format!("{path}: {} vs {}", short(a), short(b))),
            _ => {}
        }
        None
    }

    fn short(value: &Value) -> String {
        value.to_string().chars().take(260).collect()
    }

    if let Some(difference) = first_difference(actual, expected, &mut String::new()) {
        eprintln!("{name}: {difference}");
        return false;
    }
    true
}

fn compare_bytes(actual: &[u8], expected: &[u8], name: &str) -> bool {
    if actual == expected {
        return true;
    }
    if name == "writer.rtf"
        && let (Some(a), Some(b)) = (
            actual.windows(10).position(|w| w == b"\\widowctrl"),
            expected.windows(10).position(|w| w == b"\\widowctrl"),
        )
    {
        compare_bytes(&actual[a..], &expected[b..], "writer body");
    }
    let offset = actual
        .iter()
        .zip(expected)
        .position(|(a, b)| a != b)
        .unwrap_or(actual.len().min(expected.len()));
    let excerpt = |bytes: &[u8]| {
        String::from_utf8_lossy(&bytes[offset.saturating_sub(65)..bytes.len().min(offset + 175)])
            .into_owned()
    };
    eprintln!(
        "{name}: first different byte {offset}, length {} vs {}\n  Actual: {:?}\nExpected: {:?}",
        actual.len(),
        expected.len(),
        excerpt(actual),
        excerpt(expected)
    );
    false
}

fn files(directory: &Path, extension: &str) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == extension)
                && !path.file_name().unwrap().to_string_lossy().starts_with('.')
        })
        .collect();
    files.sort();
    files
}

fn json(doc: &Pandoc) -> Value {
    serde_json::from_str(&randoc_ast::json::to_json(doc)).unwrap()
}

#[test]
#[ignore = "Requires the pinned Pandoc executable, fixtures, and corpus"]
fn oracle_parity() {
    let oracle = PathBuf::from(std::env::var_os("RANDOC_ORACLE_PANDOC").unwrap());
    let suite = PathBuf::from(std::env::var_os("RANDOC_PANDOC_SRC").unwrap()).join("test");
    let corpus = PathBuf::from(std::env::var_os("RANDOC_CORPUS").unwrap());

    let reader_fixtures = files(&suite.join("rtf"), "rtf");
    assert_eq!(reader_fixtures.len(), 16);
    let readers = reader_fixtures
        .iter()
        .filter(|file| {
            let expected = run(
                &oracle,
                &suite,
                &["-f", "native", "-t", "json"],
                &fs::read(file.with_extension("native")).unwrap(),
            );
            compare_json(
                &json(&read_rtf(&fs::read(file).unwrap())),
                &serde_json::from_slice(&expected).unwrap(),
                &file.file_name().unwrap().to_string_lossy(),
            )
        })
        .count();
    eprintln!("Reader fixtures: {readers}/16");

    let writers = [
        ("testsuite.native", "writer.rtf", true),
        ("tables.native", "tables.rtf", false),
    ]
    .into_iter()
    .filter(|(input, expected, standalone)| {
        let mut host = PureHost::default();
        for image in ["lalune.jpg", "movie.jpg"] {
            host.files
                .insert(image.into(), fs::read(suite.join(image)).unwrap());
        }
        let mut ctx = Context::new(Box::new(host));
        let mut opts = WriterOptions::default();
        if *standalone {
            let source = randoc_data::default_template("rtf").unwrap();
            opts.template = Some(
                Template::compile::<String>("default.rtf", source, 128, &mut |_| Ok(String::new()))
                    .unwrap(),
            );
            opts.variables.insert("pandoc-version", Val::text(""));
        }
        let doc = read_native(&fs::read(suite.join(input)).unwrap());
        compare_bytes(
            &write_rtf(&doc, &opts, &mut ctx),
            &fs::read(suite.join(expected)).unwrap(),
            expected,
        )
    })
    .count();
    eprintln!("Writer fixtures: {writers}/2");

    let mut commands = 0;
    let mut command_count = 0;
    for id in [8170, 8783, 10145, 10942, 11211, 11364, 11682, 11981] {
        let fixture = fs::read_to_string(suite.join("command").join(format!("{id}.md"))).unwrap();
        for fence in fixture.split("```\n") {
            let Some((command, code)) = fence.split_once('\n') else {
                continue;
            };
            if !command.starts_with("% pandoc ") {
                continue;
            }
            let (input, expected) = code.split_once("\n^D\n").unwrap();
            let input = format!("{input}\n");
            let name = format!("Command {id}/{}", command_count + 1);
            let ok = if command.contains(" | ") {
                let rtf = write_rtf(
                    &read_native(input.as_bytes()),
                    &WriterOptions::default(),
                    &mut Context::pure(),
                );
                compare_json(
                    &json(&read_rtf(&rtf)),
                    &json(&read_native(expected.as_bytes())),
                    &name,
                )
            } else if command.ends_with("-t html") {
                let oracle_html = run(
                    &oracle,
                    &suite,
                    &["-f", "rtf", "-t", "html"],
                    input.as_bytes(),
                );
                let oracle_json = run(
                    &oracle,
                    &suite,
                    &["-f", "rtf", "-t", "json"],
                    input.as_bytes(),
                );
                compare_bytes(&oracle_html, expected.as_bytes(), &name)
                    && compare_json(
                        &json(&read_rtf(input.as_bytes())),
                        &serde_json::from_slice(&oracle_json).unwrap(),
                        &name,
                    )
            } else {
                let mut doc = read_rtf(input.as_bytes());
                // The native writer omits metadata in fragment output.
                doc.meta = Default::default();
                compare_json(&json(&doc), &json(&read_native(expected.as_bytes())), &name)
            };
            command_count += 1;
            commands += usize::from(ok);
        }
    }
    assert_eq!(command_count, 9);
    eprintln!("Command cases: {commands}/9");

    let corpus = files(&corpus, "rtf");
    assert_eq!(corpus.len(), 8);
    let documents = corpus
        .iter()
        .filter(|file| {
            let data = fs::read(file).unwrap();
            let expected = run(&oracle, &suite, &["-f", "rtf", "-t", "json"], &data);
            compare_json(
                &json(&read_rtf(&data)),
                &serde_json::from_slice(&expected).unwrap(),
                &file.file_name().unwrap().to_string_lossy(),
            )
        })
        .count();
    eprintln!("Corpus documents: {documents}/8");
    assert_eq!((readers, writers, commands, documents), (16, 2, 9, 8));
}

#[test]
#[ignore = "Requires the pinned Pandoc executable"]
fn review_reader_cases() {
    let oracle = PathBuf::from(std::env::var_os("RANDOC_ORACLE_PANDOC").unwrap());
    let directory = Path::new(".");
    let cases = [
        r"{\rtf1 {\*\pn\pnlvlblt\ls1}\pard\intbl\itap1\ls1 A\par\pard\intbl\itap2 B\nestcell\nestrow\pard\intbl\itap1\cell\row}",
        r"{\rtf1 {\*\pn\pnlvlblt\ls1}\intbl\ls1 A\par\cell\row}",
        r#"{\rtf1 X{\field{\*\fldinst HYPERLINK "https://e.test/a  b"}{\fldrslt  label }}Y}"#,
        r#"{\rtf1 {\*\bkmkstart mark}A{\*\bkmkend mark}{\field{\*\fldinst HYPERLINK \l "mark"}{\fldrslt B}}}"#,
        r#"{\rtf1 {\*\bkmkstart mark}A{\*\bkmkend mark}{\field{\*\fldinst HYPERLINK \\l "mark"}{\fldrslt B}}}"#,
        r#"{\rtf1 {\field{\*\fldinst {HYPERLINK \\l "mark"}}{\fldrslt B}}}"#,
        r#"{\rtf1 {\field\fldinst{HYPERLINK "https://e.test"}{\fldrslt A}}}"#,
        r"{\rtf1{\pict\pngblip 01\bin2 AB}}",
        r"{\rtf1{\pict\pngblip \bin2 AB01}}",
        r"{\rtf1{\pict\pngblip a\picw100 bc}}",
        "{\\rtf1{\\pict\\pngblip abc\ndef}}",
        r"{\rtf1{\pict\pngblip\picwgoal1440\picwgoal720\pichgoal720\pichgoal1440 01}}",
        "{\\rtf1 \\uc2\\u945?\t?X}",
        r"{\rtf1 \uc2\uc\u945??X}",
        r"{\rtf1 \uc0\u55357\emdash\u56832}",
        r"{\rtf1 \uc0\u55357{}\u56832}",
        r"{\rtf1 \uc0\u55357{\b\u56832}}",
        r"{\rtf1 \uc0\u55357{\pard}\u56832}",
        r"{\rtf1 \uc0\u55357{\v hidden}\u56832 X}",
        r"{\rtf1 A{\v {\footnote secret}}B}",
        r"{\rtf1 \uc0\u9223372036854775808}",
        r"{\rtf1 \uc0\u-9223372036854775809}",
    ];
    for input in cases {
        let expected = run(
            &oracle,
            directory,
            &["-f", "rtf", "-t", "json"],
            input.as_bytes(),
        );
        assert!(compare_json(
            &json(&read_rtf(input.as_bytes())),
            &serde_json::from_slice(&expected).unwrap(),
            input,
        ));
    }

    let malformed = "{\\rtf1 \t\\'zz}";
    let expected = run_process(
        &oracle,
        directory,
        &["-f", "rtf", "-t", "json"],
        malformed.as_bytes(),
    );
    let Reader::Text(reader) = randoc_rtf::reader("rtf").unwrap() else {
        panic!("Text reader required");
    };
    let error = reader
        .read(
            &Sources::from_text(malformed),
            &ReaderOptions::default(),
            &mut Context::pure(),
        )
        .unwrap_err();
    assert_eq!(Some(error.exit_code()), expected.status.code());
    assert_eq!(
        error.to_string(),
        String::from_utf8(expected.stderr).unwrap().trim_end()
    );
}

#[test]
#[ignore = "Requires the pinned Pandoc executable"]
fn review_writer_cases() {
    let oracle = PathBuf::from(std::env::var_os("RANDOC_ORACLE_PANDOC").unwrap());
    let directory = Path::new(".");
    let markers =
        r#"[BulletList [[],[BlockQuote [Header 2 ("",[],[]) [Str "A"],Para [Str "B"]]]]]"#;
    let tabs = r#"[CodeBlock ("",[],[]) "A\tB",Para [Str "C\tD"]]"#;
    let math = r#"[Para [Math InlineMath "-x",Space,Math InlineMath "\\theta",Space,Math InlineMath "a-b",Space,Math InlineMath "x^2_i",Space,Math InlineMath "\\sin x",Space,Math InlineMath "\\frac{1}{2}",Space,Math InlineMath "\\unknown{x}"]]"#;
    let table = |widths: &[ColWidth], content: Vec<Block>| {
        let col_specs = widths
            .iter()
            .map(|&width| ColSpec {
                width,
                ..Default::default()
            })
            .collect();
        let mut cells = vec![Cell::default(); widths.len()];
        cells[0].content = content;
        Block::Table(Box::new(Table {
            col_specs,
            bodies: vec![TableBody {
                body: vec![Row {
                    cells,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }))
    };
    let document = |block| Pandoc {
        blocks: vec![block],
        ..Default::default()
    };
    let default = [ColWidth::ColWidthDefault];
    let mixed = document(table(
        &[ColWidth::ColWidth(0.3), ColWidth::ColWidthDefault],
        Vec::new(),
    ));
    let fractional = document(table(&[ColWidth::ColWidth(0.3333); 3], Vec::new()));
    let inside = table(&default, vec![Block::Para(vec![Inline::Str("B".into())])]);
    let outside = table(
        &default,
        vec![
            Block::Para(vec![Inline::Str("A".into())]),
            Block::Plain(vec![Inline::Str("P".into())]),
            inside,
        ],
    );
    let nested = document(Block::BlockQuote(vec![outside]));
    let mut options = WriterOptions::default();
    for (name, doc) in [
        ("List markers", read_native(markers.as_bytes())),
        ("Literal tabs", read_native(tabs.as_bytes())),
        ("Math", read_native(math.as_bytes())),
        ("Mixed columns", mixed),
        ("Fractional columns", fractional),
        ("Nested cells", nested),
    ] {
        let mut context = Context::pure();
        let expected = run_process(
            &oracle,
            directory,
            &["-f", "json", "-t", "rtf"],
            randoc_ast::json::to_json(&doc).as_bytes(),
        );
        assert!(expected.status.success(), "{name}: {:?}", expected.stderr);
        assert!(compare_bytes(
            &write_rtf(&doc, &options, &mut context),
            &expected.stdout,
            name
        ));
        let stderr = String::from_utf8(expected.stderr).unwrap();
        let warnings: Vec<_> = stderr
            .lines()
            .filter_map(|line| line.strip_prefix("[WARNING] "))
            .map(|warning| warning.trim_end_matches(':'))
            .collect();
        assert_eq!(
            context
                .log()
                .iter()
                .map(|message| message.text())
                .collect::<Vec<_>>(),
            warnings
        );
    }

    let source = randoc_data::default_template("rtf").unwrap();
    options.template = Some(
        Template::compile::<String>("default.rtf", source, 128, &mut |_| Ok(String::new()))
            .unwrap(),
    );
    for title in [
        r#"MetaBlocks [BulletList [[Para [Str "title"]]]]"#,
        r#"MetaBlocks [Para [Str "title"]]"#,
    ] {
        let native = format!(
            r#"Pandoc (Meta {{unMeta = fromList [("title",{title})]}}) [BulletList [[Para [Str "A"]]]]"#
        );
        let expected = run(
            &oracle,
            directory,
            &["-f", "native", "-t", "rtf", "-s"],
            native.as_bytes(),
        );
        let actual = write_rtf(
            &read_native(native.as_bytes()),
            &options,
            &mut Context::pure(),
        );
        assert!(compare_bytes(&actual, &expected, title));
    }
}
