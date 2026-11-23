//! Replays the standalone matrix of Pandoc 3.13 (`tests/data/writer-capabilities.json`,
//! made by `tools/writer_capabilities.py`) for each registered writer. A writer
//! that does not render the template gives the scope-gap error for a format
//! with a default template.

#![allow(clippy::expect_used, reason = "a broken replay record stops the test")]

use std::collections::BTreeSet;
use std::io::Write;
use std::process::{Command, Stdio};

use serde::Deserialize;

#[derive(Deserialize)]
struct Matrix {
    template: String,
    odf_template: String,
    docx_template: String,
    records: Vec<Record>,
}

#[derive(Deserialize)]
struct Record {
    writer: String,
    request: String,
    document: String,
    args: Vec<String>,
    stdin: String,
    exit: i32,
    stdout: String,
    stderr: String,
}

fn is_epub(writer: &str) -> bool {
    matches!(writer, "epub" | "epub2" | "epub3")
}

/// Reads the document body of OpenDocument output. The default styles depend
/// on the reference document, so the replay compares the body separately.
/// For EPUB, compare the first chapter. The archive has a random identifier
/// and format-specific default styles. For DOCX, compare the body text.
/// For Jupyter notebooks, replace the generated cell identifiers.
fn document_body(writer: &str, data: Vec<u8>) -> Vec<u8> {
    use randoc_archive::zip::{Archive, ArchiveLimits};

    if is_epub(writer) {
        let mut archive = Archive::open(&data, ArchiveLimits::default()).expect("EPUB output");
        return archive
            .read("EPUB/text/ch001.xhtml")
            .expect("read ch001.xhtml")
            .expect("EPUB has a first chapter");
    }
    if writer == "docx" {
        use randoc_archive::xml::{self, XmlLimits, XmlOptions};
        const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

        let mut archive = Archive::open(&data, ArchiveLimits::default()).expect("DOCX output");
        let document = archive
            .read("word/document.xml")
            .expect("read word/document.xml")
            .expect("DOCX has a document part");
        let source = std::str::from_utf8(&document).expect("UTF-8 document part");
        let xml = xml::parse(source, &XmlLimits::default(), XmlOptions::default())
            .expect("parse the document part");
        return xml
            .root
            .child(W, "body")
            .expect("DOCX has a body")
            .children_ns(W, "p")
            .map(|paragraph| {
                paragraph
                    .children_ns(W, "r")
                    .flat_map(|run| run.children_ns(W, "t"))
                    .map(|text| text.text())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes();
    }
    if writer == "ipynb" {
        let text = String::from_utf8(data).expect("UTF-8 notebook");
        return text
            .split_inclusive('\n')
            .map(|line| {
                if let Some(id) = line
                    .strip_prefix("   \"id\": \"")
                    .and_then(|rest| rest.strip_suffix("\"\n"))
                {
                    assert_eq!(id.len(), 36);
                    assert!(
                        id.bytes()
                            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
                    );
                    "   \"id\": \"UUID\"\n"
                } else {
                    line
                }
            })
            .collect::<String>()
            .into_bytes();
    }
    let data = if writer == "odt" {
        let mut archive = Archive::open(&data, ArchiveLimits::default()).expect("ODT output");
        archive
            .read("content.xml")
            .expect("read content.xml")
            .expect("ODT has content.xml")
    } else {
        data
    };
    if matches!(writer, "fodt" | "odt" | "opendocument")
        && let Ok(text) = std::str::from_utf8(&data)
        && let Some((_, body)) = text.split_once("<office:body>")
        && let Some((body, _)) = body.split_once("</office:body>")
    {
        return body.as_bytes().to_vec();
    }
    data
}

#[test]
fn standalone_output_preserves_the_oracle_result_or_reports_the_scope_gap() {
    let matrix: Matrix = serde_json::from_str(include_str!("data/writer-capabilities.json"))
        .expect("the writer matrix is JSON");
    let inventory: Vec<String> =
        serde_json::from_str(include_str!("../../../spec/inventory/output-formats.json"))
            .expect("the output format inventory is JSON");
    let registered: BTreeSet<_> = inventory
        .into_iter()
        .filter(|name| randoc::formats::writer(name).is_some())
        .collect();
    let recorded: BTreeSet<_> = matrix.records.iter().map(|r| r.writer.clone()).collect();
    assert_eq!(
        recorded, registered,
        "each registered writer has replay data"
    );
    assert_eq!(matrix.records.len(), registered.len() * 8);

    let root = std::env::temp_dir().join(format!("randoc-cli-writers-{}", std::process::id()));
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("the replay folder exists");
    std::fs::write(root.join("t"), &matrix.template).expect("the template exists");
    std::fs::write(root.join("t-odf"), &matrix.odf_template)
        .expect("the OpenDocument template exists");
    std::fs::write(root.join("t-docx"), &matrix.docx_template).expect("the DOCX template exists");
    let mut differences = Vec::new();
    let mut checked = BTreeSet::new();
    for record in matrix.records {
        assert!(checked.insert((
            record.writer.clone(),
            record.request.clone(),
            record.document.clone(),
        )));
        let writer = randoc::formats::writer(&record.writer).expect("a registered writer");
        let gap = record.request != "none"
            && !writer.renders_template()
            && randoc_data::default_template(&record.writer).is_some();
        let mut child = Command::new(env!("CARGO_BIN_EXE_randoc"))
            .args(&record.args)
            .current_dir(&root)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("share"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the program starts");
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(record.stdin.as_bytes());
        }
        let output = child.wait_with_output().expect("the program ends");
        let mut expected = if gap {
            (
                Some(4),
                Vec::new(),
                b"Randoc does not support templates and standalone output yet.\n".to_vec(),
            )
        } else {
            (
                Some(record.exit),
                record.stdout.into_bytes(),
                record.stderr.into_bytes(),
            )
        };
        // These custom-template cases omit the final newline. See I-CLI-36.
        if matches!(record.request.as_str(), "template" | "both")
            && (record.writer == "djot"
                || record.document == "titled"
                    && matches!(
                        record.writer.as_str(),
                        "asciidoc"
                            | "asciidoc_legacy"
                            | "asciidoctor"
                            | "mediawiki"
                            | "org"
                            | "rst"
                            | "typst"
                    ))
        {
            assert_eq!(expected.1.pop(), Some(b'\n'));
        }
        // A custom EPUB chapter has an empty template title and a final
        // newline. See I-EPUB-03 in docs/ISSUES.md.
        if is_epub(&record.writer) && matches!(record.request.as_str(), "template" | "both") {
            if record.document == "titled" {
                let body = expected
                    .1
                    .strip_prefix(b"marker:Title|")
                    .expect("the EPUB title");
                expected.1 = [b"marker:|".as_slice(), body].concat();
            }
            expected.1.push(b'\n');
        }
        let stdout = if output.status.success() {
            document_body(&record.writer, output.stdout)
        } else {
            output.stdout
        };
        let actual = (output.status.code(), stdout, output.stderr);
        if actual != expected {
            differences.push(format!(
                "{} {} {} {:?}\nexpected {expected:?}\nactual   {actual:?}",
                record.writer, record.request, record.document, record.args,
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        differences.is_empty(),
        "{} writer matrix differences:\n{}",
        differences.len(),
        differences.join("\n")
    );
}
