//! Compares ODT output with the Pandoc 3.13 executable when both use the
//! same project-owned reference document.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use randoc_archive::zip::{Archive, ArchiveLimits, ArchiveWriter};
use randoc_ast::{Pandoc, json};
use randoc_core::{Context, Output, PureHost, WriterOptions};

const EPOCH: &str = "1700000001";
const GIF: &str = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";

fn oracle(dir: &Path, args: &[&str], input: &[u8]) -> Vec<u8> {
    let executable = std::env::var_os("RANDOC_ORACLE_PANDOC").expect("Set RANDOC_ORACLE_PANDOC.");
    let mut process = Command::new(executable)
        .current_dir(dir)
        .args(args)
        .env("SOURCE_DATE_EPOCH", EPOCH)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    process.stdin.take().unwrap().write_all(input).unwrap();
    let result = process.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}

fn host(files: &[(&str, Vec<u8>)]) -> Context {
    Context::new(Box::new(PureHost {
        files: files
            .iter()
            .map(|(name, bytes)| ((*name).into(), bytes.clone()))
            .collect(),
        environment: BTreeMap::from([("SOURCE_DATE_EPOCH".into(), EPOCH.into())]),
        ..PureHost::default()
    }))
}

fn write(format: &str, doc: &Pandoc, options: &WriterOptions, context: &mut Context) -> Vec<u8> {
    match randoc_odt::writer(format)
        .unwrap()
        .write(doc, options, context)
        .unwrap()
    {
        Output::Binary(bytes) => bytes,
        Output::Text(text) => text.into_bytes(),
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("randoc-odt-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn show_difference(label: &str, name: &str, left: &[u8], right: &[u8]) -> String {
    let at = left.iter().zip(right).take_while(|(x, y)| x == y).count();
    let show = |bytes: &[u8]| {
        String::from_utf8_lossy(&bytes[at.saturating_sub(80)..bytes.len().min(at + 160)])
            .into_owned()
    };
    format!(
        "{label}: {name} at byte {at}\n  randoc {:?}\n  pandoc {:?}",
        show(left),
        show(right)
    )
}

/// Returns each difference between two ODT archives, entry by entry. The
/// annotation of a formula holds StarMath text that Randoc does not make
/// yet, so `Formula-N/content.xml` is not compared.
fn entry_differences(label: &str, actual: &[u8], expected: &[u8]) -> Vec<String> {
    let mut a = Archive::open(actual, ArchiveLimits::default()).unwrap();
    let mut b = Archive::open(expected, ArchiveLimits::default()).unwrap();
    let mut differences = Vec::new();
    if a.entry_names() != b.entry_names() {
        differences.push(format!(
            "{label}: entry order\n  randoc {:?}\n  pandoc {:?}",
            a.entry_names(),
            b.entry_names()
        ));
    }
    let names: Vec<String> = b.entry_names().into_iter().map(str::to_owned).collect();
    for name in names {
        if (name.starts_with("Formula-") && name.ends_with("/content.xml")) || !a.contains(&name) {
            continue;
        }
        let left = a.read(&name).unwrap().unwrap();
        let right = b.read(&name).unwrap().unwrap();
        if left != right {
            differences.push(show_difference(label, &name, &left, &right));
        }
    }
    differences
}

/// Returns the difference between two FODT outputs. A formula holds
/// StarMath text that Randoc does not make yet, so the annotation of each
/// formula is left out.
fn fodt_differences(label: &str, actual: &[u8], expected: &[u8]) -> Vec<String> {
    let without_annotations = |bytes: &[u8]| {
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        let mut out = String::new();
        let mut rest = text.as_str();
        while let Some(start) = rest.find("<annotation") {
            out.push_str(&rest[..start]);
            let end = rest[start..].find("</annotation>").unwrap() + start;
            rest = &rest[end + "</annotation>".len()..];
        }
        out.push_str(rest);
        out.into_bytes()
    };
    // The command line ends text output with a line break.
    let mut left = without_annotations(actual);
    if !left.ends_with(b"\n") {
        left.push(b'\n');
    }
    let right = without_annotations(expected);
    if left == right {
        Vec::new()
    } else {
        vec![show_difference(label, "fodt", &left, &right)]
    }
}

/// Writes a new archive with `entries` in this order, the `mimetype` first.
fn archive(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut output = ArchiveWriter::new();
    for (name, bytes) in entries {
        if *name == "mimetype" {
            output.write_stored(name, bytes).unwrap();
        } else {
            output.write_deflated(name, bytes).unwrap();
        }
    }
    output.finish().unwrap()
}

#[test]
#[ignore = "Requires Pandoc 3.13."]
fn shared_reference_entries_match_oracle() {
    let scratch = Scratch::new("entries");
    let dir = &scratch.0;
    // A project-owned reference document: Randoc's output for an empty
    // document, and the same parts in another order with more parts.
    let base = write(
        "odt",
        &Pandoc::default(),
        &WriterOptions::default(),
        &mut host(&[]),
    );
    let mut source = Archive::open(&base, ArchiveLimits::default()).unwrap();
    let mut part = |name: &str| source.read(name).unwrap().unwrap();
    let styles = String::from_utf8(part("styles.xml"))
        .unwrap()
        .replacen("<office:styles>", "<office:styles>\n  <!-- c -->\n  ", 1)
        .replacen(
            "</office:styles>",
            "<style:style style:name=\"Quote's\" style:family=\"paragraph\"/></office:styles>",
            1,
        );
    let reordered = archive(&[
        ("mimetype", part("mimetype")),
        ("styles.xml", styles.into_bytes()),
        (
            "settings.xml",
            br#"<?xml version="1.0"?><office:document-settings xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"/>"#.to_vec(),
        ),
        ("Thumbnails/thumbnail.png", b"not an image".to_vec()),
        ("content.xml", part("content.xml")),
        ("meta.xml", part("meta.xml")),
        ("notes.txt", b"text".to_vec()),
        ("META-INF/manifest.xml", part("META-INF/manifest.xml")),
    ]);
    let references = [("base.odt", base.clone()), ("reordered.odt", reordered)];
    for (name, bytes) in &references {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    let cases: &[(&str, String)] = &[
        ("core", format!("---\ntitle: The Title\nauthor: [Ann Author, Bob]\ndate: 2024-01-02\nsubject: S & 'q'\nkeywords: [k1, k2]\nlang: en-GB\ncustom: \"x < y\"\n---\n\n# Heading one\n\nSome *emphasis*, **strong**, `code` and a [link](https://example.com).[^1]\n\n- item\n\n> Quote.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n![An image]({GIF})\n\n[^1]: The note.")),
        ("formulas", format!("A $a$ and $b$.\n\n![one]({GIF})\n\n![two]({GIF} \"t\")\n\n$$x^2$$\n\nAfter.")),
        ("plain", "Plain text.".into()),
        ("after figure", format!("![Caption]({GIF})\n\nAfter the figure.\n\n![]({GIF})\n\nAgain.")),
        ("highlight", "```rust\nlet x = 1;\n```\n\n::: {custom-style=\"Local\"}\nA [b]{custom-style=\"Char\"}.\n:::".into()),
    ];
    let mut differences = Vec::new();
    let mut matched = 0;
    let mut total = 0;
    for (reference, reference_bytes) in &references {
        for (label, source) in cases {
            let json = oracle(dir, &["-f", "markdown", "-t", "json"], source.as_bytes());
            let doc = json::from_json(std::str::from_utf8(&json).unwrap()).unwrap();
            let mut options = WriterOptions::default();
            options.reference_doc = Some((*reference).into());
            for format in ["odt", "fodt"] {
                let label = format!("{reference}/{label}/{format}");
                let reference_arg = format!("--reference-doc={reference}");
                let expected = oracle(
                    dir,
                    &["-f", "json", "-t", format, &reference_arg, "-o", "-"],
                    &json,
                );
                let mut context = host(&[(reference, reference_bytes.clone())]);
                let actual = write(format, &doc, &options, &mut context);
                if let Some(dump) = std::env::var_os("RANDOC_ODT_DUMP") {
                    let stem = label.replace(['/', '.'], "-");
                    std::fs::write(Path::new(&dump).join(format!("randoc-{stem}")), &actual)
                        .unwrap();
                    std::fs::write(Path::new(&dump).join(format!("oracle-{stem}")), &expected)
                        .unwrap();
                }
                let found = if format == "odt" {
                    entry_differences(&label, &actual, &expected)
                } else {
                    fodt_differences(&label, &actual, &expected)
                };
                total += 1;
                if found.is_empty() {
                    matched += 1;
                }
                differences.extend(found);
            }
        }
    }
    eprintln!("{matched}/{total} ODT and FODT outputs match");
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}
