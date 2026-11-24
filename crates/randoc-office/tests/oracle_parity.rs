//! Differential checks against an external Pandoc 3.13 installation.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use randoc_archive::opc::{parse_relationships, rels_part_for, resolve_target};
use randoc_archive::xml::{XmlDoc, XmlElement, XmlLimits, XmlOptions, parse};
use randoc_archive::zip::{Archive, ArchiveLimits, ArchiveWriter};
use randoc_ast::json;
use randoc_core::{Context, Reader, ReaderOptions, Writer, WriterOptions};
use serde_json::Value;

const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

fn env_path(key: &str) -> PathBuf {
    env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("Set {key} before running the oracle tests"))
}

fn oracle(args: &[&str], input: &[u8]) -> Vec<u8> {
    convert(&env_path("RANDOC_ORACLE_PANDOC"), args, input)
}

fn convert(program: &Path, args: &[&str], input: &[u8]) -> Vec<u8> {
    let mut child = Command::new(program)
        .args(args)
        .env("SOURCE_DATE_EPOCH", "1700000000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start conversion process");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    thread::scope(|threads| {
        let writer = threads.spawn(move || stdin.write_all(input));
        let output = threads.spawn(move || drain(stdout));
        let errors = threads.spawn(move || drain(stderr));
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if started.elapsed() > Duration::from_secs(30) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("Conversion timed out for {args:?}");
            }
            thread::sleep(Duration::from_millis(10));
        };
        let stderr = errors.join().unwrap();
        assert!(
            status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&stderr)
        );
        writer.join().unwrap().unwrap();
        assert!(
            stderr.is_empty(),
            "{args:?}: {}",
            String::from_utf8_lossy(&stderr)
        );
        output.join().unwrap()
    })
}

fn drain(mut pipe: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes).unwrap();
    bytes
}

fn version() {
    let version = oracle(&["--version"], b"");
    assert_eq!(
        String::from_utf8_lossy(&version).lines().next(),
        Some("pandoc 3.13")
    );
}

fn compare(path: &str, actual: &Value, expected: &Value) {
    if actual == expected {
        return;
    }
    match (actual, expected) {
        (Value::Array(a), Value::Array(e)) if a.len() == e.len() => {
            for (index, (a, e)) in a.iter().zip(e).enumerate() {
                compare(&format!("{path}[{index}]"), a, e);
            }
        }
        (Value::Object(a), Value::Object(e)) if a.keys().eq(e.keys()) => {
            for (key, value) in a {
                compare(&format!("{path}.{key}"), value, &e[key]);
            }
        }
        (Value::Array(a), Value::Array(e)) => {
            let same = a.iter().zip(e).take_while(|(a, e)| a == e).count();
            panic!(
                "{path} has {} entries, expected {}. At {same}, actual: {:?}, expected: {:?}",
                a.len(),
                e.len(),
                a.get(same),
                e.get(same)
            );
        }
        _ => panic!("{path}: actual {actual}, expected {expected}"),
    }
}

fn actual_reader(format: &str, bytes: &[u8]) -> (Value, Context) {
    let Reader::Binary(reader) = randoc_office::reader(format).unwrap() else {
        panic!("binary reader")
    };
    let mut context = Context::pure();
    let document = reader
        .read(bytes, &ReaderOptions::default(), &mut context)
        .unwrap();
    let actual = serde_json::from_str(&json::to_json(&document)).unwrap();
    (actual, context)
}

fn readers(format: &str, corpus: &[&str]) {
    version();
    let data = env_path("RANDOC_CORPUS");
    let fixture = env_path("RANDOC_PANDOC_SRC")
        .join("test")
        .join(format!("{format}-reader/basic.{format}"));
    for path in corpus.iter().map(|file| data.join(file)).chain([fixture]) {
        let bytes = fs::read(&path).unwrap();
        let expected =
            serde_json::from_slice(&oracle(&["-f", format, "-t", "json"], &bytes)).unwrap();
        let (actual, context) = actual_reader(format, &bytes);
        compare(&path.display().to_string(), &actual, &expected);
        assert!(
            context.log().is_empty(),
            "{}: {:?}",
            path.display(),
            context.log()
        );
        if format == "pptx" {
            verify_media(&actual, &bytes, &context);
        }
        eprintln!("PASS {}", path.display());
    }
}

fn verify_media(ast: &Value, bytes: &[u8], context: &Context) {
    let limits = ArchiveLimits::from_limits(&context.limits);
    let mut archive = Archive::open(bytes, limits).unwrap();
    let mut work = vec![ast];
    while let Some(value) = work.pop() {
        match value {
            Value::Array(items) => work.extend(items),
            Value::Object(items) => {
                if items.get("t").and_then(Value::as_str) == Some("Image") {
                    let path = items["c"][2][0].as_str().unwrap();
                    assert_eq!(
                        context.media.get(path).unwrap().contents,
                        archive.read(path).unwrap().unwrap()
                    );
                }
                work.extend(items.values());
            }
            _ => {}
        }
    }
}

#[test]
#[ignore = "requires the Pandoc 3.13 oracle, corpus, and test tree"]
fn xlsx_reader() {
    readers(
        "xlsx",
        &[
            "tika-testexcel-dateformats-xlsx.xlsx",
            "tika-testexcel-formats-xlsx.xlsx",
            "tika-testexcel-hyperlinks-xlsx.xlsx",
            "tika-testexcel-xlsx.xlsx",
        ],
    );
}

#[test]
#[ignore = "requires the Pandoc 3.13 oracle, corpus, and test tree"]
fn pptx_reader() {
    readers(
        "pptx",
        &[
            "tika-testcomment-pptx.pptx",
            "tika-testppt-groups-pptx.pptx",
            "tika-testppt-mastertext-pptx.pptx",
            "tika-testppt-pptx.pptx",
            "tika-testppt-various-pptx.pptx",
        ],
    );
}

fn write_pptx(document: &randoc_ast::Pandoc, reference: Option<&Path>) -> Vec<u8> {
    let Writer::Binary(writer) = randoc_office::writer("pptx").unwrap() else {
        panic!("binary writer")
    };
    let mut options = WriterOptions::default();
    options.reference_doc = reference.map(|path| path.to_str().unwrap().into());
    writer
        .write(document, &options, &mut Context::native())
        .unwrap()
}

fn part(archive: &mut Archive<'_>, path: &str) -> XmlDoc {
    let bytes = archive
        .read(path)
        .unwrap()
        .unwrap_or_else(|| panic!("Missing {path}"));
    parse(
        std::str::from_utf8(&bytes).unwrap(),
        &XmlLimits::default(),
        XmlOptions::default(),
    )
    .unwrap()
}

fn slide_structure(bytes: &[u8]) -> (String, Vec<(String, Vec<String>)>) {
    let mut archive =
        Archive::open(bytes, ArchiveLimits::from_limits(&Context::pure().limits)).unwrap();
    let presentation = part(&mut archive, "ppt/presentation.xml");
    assert_eq!(
        presentation
            .root
            .child(P, "sldIdLst")
            .unwrap()
            .children_ns(P, "sldId")
            .count(),
        1
    );
    let slide = "ppt/slides/slide1.xml";
    let rels = part(&mut archive, &rels_part_for(slide));
    let rels = parse_relationships(&rels);
    let layout = rels
        .iter()
        .find(|rel| rel.kind.ends_with("/slideLayout"))
        .unwrap();
    let layout_path = resolve_target(slide, &layout.target).unwrap();
    let layout = part(&mut archive, &layout_path);
    assert_eq!(
        layout
            .root
            .child(P, "cSld")
            .unwrap()
            .attr_unqualified("name"),
        Some("Title and Content")
    );
    let slide = part(&mut archive, slide);
    let shapes = slide
        .root
        .child(P, "cSld")
        .unwrap()
        .child(P, "spTree")
        .unwrap();
    fn paragraphs(element: &XmlElement, output: &mut Vec<String>) {
        if element.name.ns.as_deref() == Some(A) && element.name.local == "p" {
            output.push(
                element
                    .children_ns(A, "r")
                    .filter_map(|run| run.child(A, "t"))
                    .map(XmlElement::text)
                    .collect::<Vec<_>>()
                    .join(""),
            );
        } else {
            for child in element.elements() {
                paragraphs(child, output);
            }
        }
    }
    let ordered_shapes = shapes
        .elements()
        .filter(|shape| matches!(shape.name.local.as_str(), "sp" | "graphicFrame"))
        .map(|shape| {
            let mut text = Vec::new();
            paragraphs(shape, &mut text);
            (shape.name.local.clone(), text)
        })
        .collect();
    (layout_path, ordered_shapes)
}

#[test]
#[ignore = "requires the Pandoc 3.13 oracle and test tree"]
fn pptx_writer() {
    version();
    let tree = env_path("RANDOC_PANDOC_SRC").join("test");
    let fixture =
        fs::read_to_string(tree.join("command/pptx-table-content-same-slide.md")).unwrap();
    let command = fixture
        .split_once("% pandoc -t native\n")
        .unwrap()
        .1
        .split_once("\n^D")
        .unwrap()
        .0;
    let ast = oracle(&["-f", "markdown", "-t", "json"], command.as_bytes());
    let ast = json::from_json(std::str::from_utf8(&ast).unwrap()).unwrap();
    let mut round_trip = None;
    for name in ["reference-no-slides.pptx", "reference-moved-layouts.pptx"] {
        let reference = tree.join("pptx").join(name);
        let reference_arg = format!("--reference-doc={}", reference.display());
        let expected = oracle(
            &["-f", "markdown", "-t", "pptx", &reference_arg],
            command.as_bytes(),
        );
        let actual = write_pptx(&ast, Some(&reference));
        assert_eq!(slide_structure(&actual), slide_structure(&expected));
        let (layout, shapes) = slide_structure(&actual);
        let reference_bytes = fs::read(reference).unwrap();
        let mut reference_archive = Archive::open(
            &reference_bytes,
            ArchiveLimits::from_limits(&Context::pure().limits),
        )
        .unwrap();
        let limits = ArchiveLimits::from_limits(&Context::pure().limits);
        let mut output_archive = Archive::open(&actual, limits).unwrap();
        assert_eq!(
            output_archive.read(&layout).unwrap().unwrap(),
            reference_archive.read(&layout).unwrap().unwrap(),
        );
        let expected =
            serde_json::from_slice(&oracle(&["-f", "pptx", "-t", "json"], &expected)).unwrap();
        let actual_pandoc =
            serde_json::from_slice(&oracle(&["-f", "pptx", "-t", "json"], &actual)).unwrap();
        compare(name, &actual_pandoc, &expected);
        compare(name, &actual_reader("pptx", &actual).0, &expected);
        round_trip = Some((shapes, expected));
        eprintln!("PASS pptx-table-content-same-slide.md with {name}");
    }
    let (shapes, expected) = round_trip.unwrap();
    let actual = write_pptx(&ast, None);
    assert_eq!(slide_structure(&actual).1, shapes);
    let pandoc = serde_json::from_slice(&oracle(&["-f", "pptx", "-t", "json"], &actual)).unwrap();
    compare(
        "default deck round trip",
        &actual_reader("pptx", &actual).0,
        &pandoc,
    );
    compare("default deck content", &pandoc, &expected);
    eprintln!("PASS clean-room default PPTX round trip");
}

#[test]
#[ignore = "requires the Pandoc 3.13 oracle"]
fn pptx_adjacent_runs() {
    version();
    let deck = oracle(
        &["-f", "markdown", "-t", "pptx"],
        b"# Slide\n\nSee [site](https://e.org) and **bold** text.\n\n**W**ord.\n",
    );
    let expected: Value =
        serde_json::from_slice(&oracle(&["-f", "pptx", "-t", "json"], &deck)).unwrap();
    let actual = actual_reader("pptx", &deck).0;
    compare("adjacent text runs", &actual, &expected);
    assert_eq!(
        actual["blocks"][1]["c"][0]["c"],
        "See  site  and  bold  text."
    );
    assert_eq!(actual["blocks"][2]["c"][0]["c"], "W ord.");
    eprintln!("PASS adjacent runs in oracle deck");
}

#[test]
#[ignore = "requires the Pandoc 3.13 oracle"]
fn xlsx_number_and_shared_string_lexemes() {
    version();
    const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    const RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
    let mut cells = String::new();
    for (index, value) in [
        ".5",
        "-.5",
        "1.",
        "+1",
        "inf",
        "INF",
        "nan",
        "0x10",
        "0X1F",
        "0o17",
        "(1)",
        "( -2 )",
        "- 3",
        " 12 ",
        "1E5",
        "1e+5",
        "00012",
        "-0",
        "1e400",
        "1e-400",
        "NaN",
        "Infinity",
        "-Infinity",
        "0b101",
        "1e",
        "1_0",
        "0x20000000000001",
    ]
    .into_iter()
    .enumerate()
    {
        write!(cells, r#"<c r="A{}"><v>{value}</v></c>"#, index + 1).unwrap();
    }
    for (index, value) in [" 0 ", "0x0", "+0", "1", "(2)"].into_iter().enumerate() {
        write!(cells, r#"<c r="B{}" t="s"><v>{value}</v></c>"#, index + 1).unwrap();
    }
    cells.push_str(r#"<c r="b1"><v>lowercase</v></c>"#);
    let mut zip = ArchiveWriter::new();
    for (path, xml) in [
        (
            "_rels/.rels",
            format!(
                r#"<Relationships xmlns="{RELS}"><Relationship Id="rId1" Type="{R}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
            ),
        ),
        (
            "xl/workbook.xml",
            format!(
                r#"<workbook xmlns="{S}" xmlns:r="{R}"><sheets><sheet name="Numbers" sheetId="1" r:id="rId1"/></sheets></workbook>"#
            ),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            format!(
                r#"<Relationships xmlns="{RELS}"><Relationship Id="rId1" Type="{R}/worksheet" Target="worksheets/sheet.xml"/><Relationship Id="rId2" Type="{R}/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#
            ),
        ),
        (
            "xl/sharedStrings.xml",
            format!(
                r#"<sst xmlns="{S}"><si><r><t>a</t></r><r><t>b</t></r></si><si><r><t>a</t></r><rPh><t>PH</t></rPh><r><t>b</t></r></si><si><t>x</t><rPh><t>y</t></rPh></si></sst>"#
            ),
        ),
        (
            "xl/worksheets/sheet.xml",
            format!(
                r#"<worksheet xmlns="{S}"><sheetData><row r="1">{cells}</row></sheetData></worksheet>"#
            ),
        ),
    ] {
        zip.write_deflated(path, xml.as_bytes()).unwrap();
    }
    let sheet = zip.finish().unwrap();
    let expected = serde_json::from_slice(&oracle(&["-f", "xlsx", "-t", "json"], &sheet)).unwrap();
    compare(
        "number and shared-string lexemes",
        &actual_reader("xlsx", &sheet).0,
        &expected,
    );
    eprintln!("PASS number and shared-string lexemes, including lowercase cell references");
}

#[test]
#[ignore = "requires the Pandoc 3.13 oracle and the writer-baseline reference"]
fn pptx_reference_shape_id_parity() {
    version();
    let reference = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/fixtures/writer-baseline/reference.pptx");
    let reference_arg = format!("--reference-doc={}", reference.display());
    let table = "| Key | Value |\n| --- | --- |\n| One | Two |";
    for (markdown, ids) in [
        (
            format!(
                "# Two tables\n\n:::: {{.columns}}\n::: {{.column}}\n\n{table}\n\nTable: First caption\n\n:::\n::: {{.column}}\n\n{table}\n\nTable: Second caption\n\n:::\n::::\n"
            ),
            [1, 2, 6, 1, 6, 1],
        ),
        (
            format!(
                "# Mixed slot\n\nBefore the table.\n\n{table}\n\nTable: Middle caption\n\nAfter the table.\n"
            ),
            [1, 2, 3, 6, 1, 3],
        ),
    ] {
        let ast = oracle(&["-f", "markdown", "-t", "json"], markdown.as_bytes());
        let expected = oracle(&["-f", "json", "-t", "pptx", &reference_arg], &ast);
        let doc = json::from_json(std::str::from_utf8(&ast).unwrap()).unwrap();
        let actual = write_pptx(&doc, Some(&reference));
        for pptx in [&expected, &actual] {
            let limits = ArchiveLimits::from_limits(&Context::pure().limits);
            let mut archive = Archive::open(pptx, limits).unwrap();
            assert!(!archive.contains("ppt/slides/slide2.xml"));
            let slide = part(&mut archive, "ppt/slides/slide1.xml");
            let tree = slide
                .root
                .child(P, "cSld")
                .unwrap()
                .child(P, "spTree")
                .unwrap();
            let actual_ids: Vec<usize> = tree
                .elements()
                .flat_map(XmlElement::elements)
                .filter_map(|properties| {
                    if properties.name.ns.as_deref() == Some(P) && properties.name.local == "cNvPr"
                    {
                        Some(properties)
                    } else {
                        properties.child(P, "cNvPr")
                    }
                })
                .map(|shape| shape.attr_unqualified("id").unwrap().parse().unwrap())
                .collect();
            assert_eq!(actual_ids, ids, "{markdown}");
        }
    }
}

#[test]
#[ignore = "requires the Pandoc 3.13 oracle, writer-baseline fixtures, and RANDOC_TEST_NATIVE_READER"]
fn pptx_reference_writer_baseline() {
    version();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let reference = repo.join("conformance/fixtures/writer-baseline/reference.pptx");
    let reference_arg = format!("--reference-doc={}", reference.display());
    let suite =
        fs::read_to_string(repo.join("conformance/cases/writer-baseline/pptx.toml")).unwrap();
    let fixture = fs::read(&reference).unwrap();
    let native_reader = env_path("RANDOC_TEST_NATIVE_READER");
    let mut count = 0;
    for case in suite.split("[[case]]").skip(1) {
        let id = case
            .split_once("id = \"")
            .unwrap()
            .1
            .split('"')
            .next()
            .unwrap();
        let native = case
            .split_once("input = '''\n")
            .unwrap()
            .1
            .split_once("'''")
            .unwrap()
            .0;
        let ast = oracle(&["-f", "native", "-t", "json"], native.as_bytes());
        let native_ast = convert(
            &native_reader,
            &["-f", "native", "-t", "json"],
            native.as_bytes(),
        );
        for (unit, format, input, writer_ast) in [
            ("write:pptx", "json", ast.as_slice(), ast.as_slice()),
            (
                "e2e:pptx",
                "native",
                native.as_bytes(),
                native_ast.as_slice(),
            ),
        ] {
            let doc = json::from_json(std::str::from_utf8(writer_ast).unwrap()).unwrap();
            let expected = oracle(&["-f", format, "-t", "pptx", &reference_arg], input);
            let Writer::Binary(writer) = randoc_office::writer("pptx").unwrap() else {
                panic!("binary writer");
            };
            let mut context = Context::new(Box::new(randoc_core::PureHost {
                files: [("reference.pptx".into(), fixture.clone())].into(),
                environment: [("SOURCE_DATE_EPOCH".into(), "1700000000".into())].into(),
                ..randoc_core::PureHost::default()
            }));
            let mut options = WriterOptions::default();
            options.reference_doc = Some("reference.pptx".into());
            let actual = writer.write(&doc, &options, &mut context).unwrap();
            assert!(context.log().is_empty(), "{id}#{unit}: {:?}", context.log());
            compare_pptx_entries(&format!("{id}#{unit}"), &actual, &expected);
        }
        count += 1;
    }
    assert_eq!(count, 8);
}

fn compare_pptx_entries(id: &str, actual: &[u8], expected: &[u8]) {
    let limits = ArchiveLimits::from_limits(&Context::pure().limits);
    let mut actual = Archive::open(actual, limits).unwrap();
    let mut expected = Archive::open(expected, limits).unwrap();
    let entry_names = |archive: &Archive<'_>| {
        let mut names: Vec<_> = archive
            .entry_names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        names.sort();
        names
    };
    let entries = entry_names(&expected);
    assert_eq!(entry_names(&actual), entries, "{id}: archive entries");
    let mut changed = Vec::new();
    for path in &entries {
        let a = actual.read(path).unwrap().unwrap();
        let e = expected.read(path).unwrap().unwrap();
        if a != e {
            let offset = a.iter().zip(&e).take_while(|(a, e)| a == e).count();
            let snippet = |bytes: &[u8]| {
                String::from_utf8_lossy(
                    &bytes[offset.saturating_sub(36)..bytes.len().min(offset + 240)],
                )
                .into_owned()
            };
            changed.push(format!(
                "{path} at byte {offset}\nactual: {:?}\nexpected: {:?}",
                snippet(&a),
                snippet(&e)
            ));
        }
    }
    assert!(changed.is_empty(), "{id}:\n{}", changed.join("\n"));
    eprintln!("PASS {id} ({} archive entries)", entries.len());
}
