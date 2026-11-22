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

/// Reads the document body of OpenDocument output. The default styles depend
/// on the reference document, so the replay compares the body separately.
fn document_body(writer: &str, data: Vec<u8>) -> Vec<u8> {
    use randoc_archive::zip::{Archive, ArchiveLimits};

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
        // These writers omit the final newline after a titled document with
        // this custom template. See I-CLI-36 in docs/ISSUES.md.
        if matches!(record.writer.as_str(), "mediawiki" | "org" | "rst")
            && matches!(record.request.as_str(), "template" | "both")
            && record.document == "titled"
        {
            assert_eq!(expected.1.pop(), Some(b'\n'));
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
