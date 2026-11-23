//! Comparison of a Randoc result with the oracle result.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use serde::Serialize;

use crate::cases::{Outcome, Unit, UnitKind};
use crate::files::{FileChanges, FileState};

/// The output formats that are ZIP archives.
const ZIP_FORMATS: [&str; 7] = [
    "docx",
    "odt",
    "epub",
    "epub2",
    "epub3",
    "pptx",
    "chunkedhtml",
];

/// The largest total size of the entries that the harness reads from one archive.
const MAX_ARCHIVE_BYTES: u64 = 512 << 20;

/// The largest number of entries that the harness reads from one archive.
const MAX_ARCHIVE_ENTRIES: usize = 100_000;

/// The result of one comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Verdict {
    /// The exit codes are equal.
    pub exit_ok: bool,
    /// The standard error outputs are equal.
    pub stderr_ok: bool,
    /// The standard outputs have the same bytes.
    pub bytes_ok: bool,
    /// The standard outputs have the same content after normalization.
    pub content_ok: bool,
    /// The changes to the work folder are equal.
    pub files_ok: bool,
    /// The unit passes.
    pub pass: bool,
}

/// The first four bytes of a ZIP archive: a local file header signature.
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// Returns `true` when the unit passes on content and not on bytes.
///
/// A read unit compares document trees. A ZIP format compares archive entries.
/// A command unit whose oracle stdout is a ZIP archive compares archive entries.
/// Each other unit must have the same bytes.
fn content_is_enough(unit: &Unit, expected: &[u8]) -> bool {
    unit.kind == UnitKind::Read || has_zip_format(unit) || is_zip_stdout(unit, expected)
}

fn has_zip_format(unit: &Unit) -> bool {
    unit.to
        .as_deref()
        .is_some_and(|to| ZIP_FORMATS.contains(&to))
}

/// A command unit has no output format, so the harness identifies a ZIP
/// archive on stdout by its first bytes.
fn is_zip_stdout(unit: &Unit, stdout: &[u8]) -> bool {
    unit.kind == UnitKind::Command && stdout.starts_with(ZIP_MAGIC)
}

/// The semantic content of a ZIP archive. The timestamps, the compression
/// level and the central directory layout are not part of it.
#[derive(Debug, PartialEq, Eq)]
struct Archive {
    /// The contents of each file entry by name. Directory entries are not included.
    entries: BTreeMap<String, Vec<u8>>,
    /// The name of the entry with the first local header.
    first: Option<String>,
    /// The first entry is stored without compression.
    first_stored: bool,
}

/// Reads an archive. An archive that is not valid, that is too large or that
/// has two entries with one name gives `None`.
fn read_archive(bytes: &[u8]) -> Option<Archive> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    if archive.len() > MAX_ARCHIVE_ENTRIES || archive.has_duplicate_names() {
        return None;
    }
    let mut entries = BTreeMap::new();
    let mut first: Option<(u64, String, bool)> = None;
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).ok()?;
        let name = file.name().ok()?.into_owned();
        let start = file.header_start();
        if first.as_ref().is_none_or(|(offset, _, _)| start < *offset) {
            let stored = file.compression() == zip::CompressionMethod::Stored;
            first = Some((start, name.clone(), stored));
        }
        if file.is_dir() {
            continue;
        }
        let mut contents = Vec::new();
        file.by_ref()
            .take(MAX_ARCHIVE_BYTES - total + 1)
            .read_to_end(&mut contents)
            .ok()?;
        total += contents.len() as u64;
        if total > MAX_ARCHIVE_BYTES {
            return None;
        }
        if entries.insert(name, contents).is_some() {
            return None;
        }
    }
    let (first, first_stored) = match first {
        Some((_, name, stored)) => (Some(name), stored),
        None => (None, false),
    };
    Some(Archive {
        entries,
        first,
        first_stored,
    })
}

/// Lists each difference between two archives, as text for a person.
///
/// The entry names and contents must be equal. The first entry must have the
/// same name: an EPUB container must start with `mimetype`. When the oracle
/// stores the first entry without compression, Randoc must do the same.
fn archive_differences(expected: &Archive, actual: &Archive) -> Vec<String> {
    let mut differences = Vec::new();
    if expected.first != actual.first {
        differences.push(format!(
            "first entry {:?}, randoc gives {:?}",
            expected.first.as_deref().unwrap_or(""),
            actual.first.as_deref().unwrap_or("")
        ));
    } else if expected.first_stored && !actual.first_stored {
        differences.push(format!(
            "first entry {:?} is compressed, the oracle stores it",
            expected.first.as_deref().unwrap_or("")
        ));
    }
    for (name, contents) in &expected.entries {
        match actual.entries.get(name) {
            None => differences.push(format!("missing entry {name:?}")),
            Some(other) if other != contents => {
                differences.push(format!("different entry {name:?}"));
            }
            Some(_) => {}
        }
    }
    for name in actual.entries.keys() {
        if !expected.entries.contains_key(name) {
            differences.push(format!("extra entry {name:?}"));
        }
    }
    differences
}

/// When the oracle archive starts with a stored `mimetype` entry, as an EPUB
/// or ODT container must, the Randoc archive must start with it too. The
/// order of the other entries does not matter.
fn keeps_stored_mimetype(expected: &Archive, actual: &Archive) -> bool {
    let stored_mimetype =
        |archive: &Archive| archive.first.as_deref() == Some("mimetype") && archive.first_stored;
    !stored_mimetype(expected) || stored_mimetype(actual)
}

/// Describes why the ZIP archive on the stdout of a command unit is different
/// from the oracle archive. Gives `None` when the unit does not compare
/// archives or when the archives are equal.
pub fn zip_stdout_difference(unit: &Unit, expected: &[u8], actual: &[u8]) -> Option<String> {
    if !is_zip_stdout(unit, expected) || expected == actual {
        return None;
    }
    let (Some(expected), Some(actual)) = (read_archive(expected), read_archive(actual)) else {
        return Some("stdout is not a valid ZIP archive, the bytes are different".to_owned());
    };
    let differences = archive_differences(&expected, &actual);
    (!differences.is_empty()).then(|| format!("ZIP stdout: {}", differences.join("; ")))
}

fn same_content(unit: &Unit, expected: &[u8], actual: &[u8]) -> bool {
    if expected == actual {
        return true;
    }
    let is_json = unit.kind == UnitKind::Read || unit.to.as_deref() == Some("json");
    if is_json {
        let parse = |bytes: &[u8]| serde_json::from_slice::<serde_json::Value>(bytes).ok();
        return matches!((parse(expected), parse(actual)), (Some(a), Some(b)) if a == b);
    }
    if has_zip_format(unit) {
        return matches!(
            (read_archive(expected), read_archive(actual)),
            (Some(a), Some(b)) if a.entries == b.entries && keeps_stored_mimetype(&a, &b)
        );
    }
    if is_zip_stdout(unit, expected) {
        return matches!(
            (read_archive(expected), read_archive(actual)),
            (Some(a), Some(b)) if archive_differences(&a, &b).is_empty()
        );
    }
    false
}

fn same_file_changes(expected: &FileChanges, actual: &FileChanges) -> bool {
    expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual)
            .all(|((name, expected), (path, actual))| {
                name == path && same_file_state(expected, actual)
            })
}

fn same_file_state(expected: &Option<FileState>, actual: &Option<FileState>) -> bool {
    match (expected, actual) {
        (Some(FileState::InvalidZip { .. }), _) => false,
        (
            Some(FileState::Zip {
                sha256: expected_hash,
                first_stored_mimetype: expected_mimetype,
            }),
            Some(FileState::Zip {
                sha256: actual_hash,
                first_stored_mimetype: actual_mimetype,
            }),
        ) => expected_hash == actual_hash && (!expected_mimetype || *actual_mimetype),
        _ => expected == actual,
    }
}

/// Compares the outcome of Randoc with the outcome of the oracle.
pub fn compare(unit: &Unit, expected: &Outcome, actual: &Outcome) -> Verdict {
    let expected_out = expected.stdout_bytes();
    let actual_out = actual.stdout_bytes();
    let exit_ok = expected.exit == actual.exit;
    let stderr_ok = expected.stderr_bytes() == actual.stderr_bytes();
    let bytes_ok = expected_out == actual_out;
    let content_ok = same_content(unit, &expected_out, &actual_out);
    let files_ok = unit.capture_files.is_none()
        || matches!(
            (&expected.file_changes, &actual.file_changes),
            (Some(expected), Some(actual)) if same_file_changes(expected, actual)
        );
    let output_ok = if content_is_enough(unit, &expected_out) {
        content_ok
    } else {
        bytes_ok
    };
    Verdict {
        exit_ok,
        stderr_ok,
        bytes_ok,
        content_ok,
        files_ok,
        pass: exit_ok && stderr_ok && output_ok && files_ok,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::cases::Stdin;

    fn unit(kind: UnitKind, to: Option<&str>) -> Unit {
        Unit {
            id: "s/f/c#u".into(),
            file_key: "s/f".into(),
            local_id: "c#u".into(),
            kind,
            args: Vec::new(),
            stdin: Stdin::Bytes(Vec::new()),
            files: BTreeMap::new(),
            capture_files: None,
            fixtures: BTreeMap::new(),
            from: None,
            to: to.map(str::to_owned),
            covers: Vec::new(),
        }
    }

    fn outcome(exit: i32, stdout: &[u8], stderr: &[u8]) -> Outcome {
        Outcome::new(exit, stdout.to_vec(), stderr.to_vec())
    }

    #[test]
    fn identical_invalid_zip_files_do_not_pass() {
        let mut unit = unit(UnitKind::Command, None);
        unit.capture_files = Some(crate::files::CaptureFiles {
            zip: ["out.zip".to_owned()].into(),
        });
        let mut result = outcome(0, b"", b"");
        result.file_changes = Some(BTreeMap::from([(
            "out.zip".to_owned(),
            Some(FileState::InvalidZip {
                sha256: "same".to_owned(),
            }),
        )]));
        assert!(!compare(&unit, &result, &result).pass);
    }

    #[test]
    fn written_zip_needs_a_first_stored_mimetype_when_the_oracle_has_one() {
        let mut unit = unit(UnitKind::Command, None);
        unit.capture_files = Some(crate::files::CaptureFiles {
            zip: ["out.epub".to_owned()].into(),
        });
        let with_mimetype = |first_stored_mimetype| {
            let mut result = outcome(0, b"", b"");
            result.file_changes = Some(BTreeMap::from([(
                "out.epub".to_owned(),
                Some(FileState::Zip {
                    sha256: "same".to_owned(),
                    first_stored_mimetype,
                }),
            )]));
            result
        };
        let stored_first = with_mimetype(true);
        let repacked = with_mimetype(false);
        assert!(compare(&unit, &stored_first, &stored_first).pass);
        assert!(!compare(&unit, &stored_first, &repacked).pass);
        assert!(compare(&unit, &repacked, &stored_first).pass);
    }

    fn zip_bytes(entries: &[(&str, &[u8])], stored: bool) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, contents) in entries {
            let method = if stored {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            };
            let options = zip::write::SimpleFileOptions::default().compression_method(method);
            writer.start_file(*name, options).unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn text_formats_need_the_same_bytes() {
        let html = unit(UnitKind::Write, Some("html"));
        let same = compare(
            &html,
            &outcome(0, b"<p>a</p>\n", b""),
            &outcome(0, b"<p>a</p>\n", b""),
        );
        assert_eq!(
            same,
            Verdict {
                exit_ok: true,
                stderr_ok: true,
                bytes_ok: true,
                content_ok: true,
                files_ok: true,
                pass: true
            }
        );
        let different = compare(
            &html,
            &outcome(0, b"<p>a</p>\n", b""),
            &outcome(0, b"<p>a</p>", b""),
        );
        assert!(!different.bytes_ok && !different.content_ok && !different.pass);
    }

    #[test]
    fn exit_code_and_stderr_are_part_of_the_verdict() {
        let html = unit(UnitKind::EndToEnd, Some("html"));
        let exit = compare(&html, &outcome(0, b"x", b""), &outcome(1, b"x", b""));
        assert!(!exit.exit_ok && exit.stderr_ok && exit.bytes_ok && !exit.pass);
        let stderr = compare(
            &html,
            &outcome(0, b"x", b"[WARNING] a\n"),
            &outcome(0, b"x", b""),
        );
        assert!(stderr.exit_ok && !stderr.stderr_ok && stderr.bytes_ok && !stderr.pass);
    }

    #[test]
    fn stderr_is_compared_by_bytes() {
        let command = unit(UnitKind::Command, None);
        let verdict = compare(
            &command,
            &outcome(0, b"", &[0xff]),
            &outcome(0, b"", &[0xfe]),
        );
        assert!(!verdict.stderr_ok && !verdict.pass);
        let verdict = compare(
            &command,
            &outcome(0, b"", &[0xff]),
            &outcome(0, b"", &[0xff]),
        );
        assert!(verdict.pass);
    }

    #[test]
    fn a_read_unit_compares_the_tree_and_not_the_bytes() {
        let read = unit(UnitKind::Read, None);
        let spaced = compare(
            &read,
            &outcome(0, br#"{"a":[1,2]}"#, b""),
            &outcome(0, br#"{ "a": [1, 2] }"#, b""),
        );
        assert!(!spaced.bytes_ok && spaced.content_ok && spaced.pass);
        let different = compare(
            &read,
            &outcome(0, br#"{"a":[1,2]}"#, b""),
            &outcome(0, br#"{"a":[1,3]}"#, b""),
        );
        assert!(!different.content_ok && !different.pass);
        let invalid = compare(&read, &outcome(0, b"{", b""), &outcome(0, b"{ ", b""));
        assert!(!invalid.content_ok && !invalid.pass);
    }

    #[test]
    fn json_as_a_text_output_format_needs_the_same_bytes() {
        let write = unit(UnitKind::Write, Some("json"));
        let spaced = compare(
            &write,
            &outcome(0, br#"{"a":1}"#, b""),
            &outcome(0, br#"{"a": 1}"#, b""),
        );
        assert!(spaced.content_ok && !spaced.pass);
    }

    #[test]
    fn zip_formats_ignore_entry_order_and_compression() {
        let docx = unit(UnitKind::Write, Some("docx"));
        let a = zip_bytes(
            &[
                ("word/document.xml", b"<w/>"),
                ("[Content_Types].xml", b"<t/>"),
            ],
            false,
        );
        let b = zip_bytes(
            &[
                ("[Content_Types].xml", b"<t/>"),
                ("word/document.xml", b"<w/>"),
            ],
            true,
        );
        assert_ne!(a, b);
        let same = compare(&docx, &outcome(0, &a, b""), &outcome(0, &b, b""));
        assert!(!same.bytes_ok && same.content_ok && same.pass);

        let changed = zip_bytes(
            &[
                ("[Content_Types].xml", b"<t/>"),
                ("word/document.xml", b"<x/>"),
            ],
            true,
        );
        let different = compare(&docx, &outcome(0, &a, b""), &outcome(0, &changed, b""));
        assert!(!different.content_ok && !different.pass);

        let missing = zip_bytes(&[("[Content_Types].xml", b"<t/>")], true);
        assert!(!compare(&docx, &outcome(0, &a, b""), &outcome(0, &missing, b"")).pass);
        assert!(!compare(&docx, &outcome(0, &a, b""), &outcome(0, b"not a zip", b"")).pass);
    }

    #[test]
    fn an_archive_with_two_entries_of_one_name_does_not_pass() {
        let docx = unit(UnitKind::Write, Some("docx"));
        let expected = zip_bytes(&[("a.txt", b"good")], true);
        let mut actual = zip_bytes(&[("a.txt", b"bad!"), ("b.txt", b"good")], true);
        // Rename the second entry to the name of the first entry, in the local
        // header and in the central directory.
        let mut renamed = 0;
        for start in 0..actual.len().saturating_sub(4) {
            if &actual[start..start + 5] == b"b.txt" {
                actual[start..start + 5].copy_from_slice(b"a.txt");
                renamed += 1;
            }
        }
        assert_eq!(renamed, 2);
        assert!(
            !compare(
                &docx,
                &outcome(0, &expected, b""),
                &outcome(0, &actual, b"")
            )
            .pass
        );
        assert!(read_archive(&actual).is_none());
    }

    /// One entry of a test archive: name, contents, compression method,
    /// compression level and modification year.
    type TestEntry<'a> = (&'a str, &'a [u8], zip::CompressionMethod, Option<i64>, u16);

    fn archive_bytes(entries: &[TestEntry<'_>]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, contents, method, level, year) in entries {
            let time = zip::DateTime::from_date_and_time(*year, 1, 2, 3, 4, 6).unwrap();
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(*method)
                .compression_level(*level)
                .last_modified_time(time);
            writer.start_file(*name, options).unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    const STORED: zip::CompressionMethod = zip::CompressionMethod::Stored;
    const DEFLATED: zip::CompressionMethod = zip::CompressionMethod::Deflated;
    const PAGE: &[u8] = b"<html><body><p>Some text, some text, some text.</p></body></html>\n";

    fn oracle_archive() -> Vec<u8> {
        archive_bytes(&[
            ("mimetype", b"application/epub+zip", STORED, None, 2023),
            ("index.html", PAGE, DEFLATED, Some(9), 2023),
            ("sitemap.json", b"{}", DEFLATED, Some(9), 2023),
        ])
    }

    fn command_verdict(expected: &[u8], actual: &[u8]) -> (Verdict, Option<String>) {
        let command = unit(UnitKind::Command, None);
        (
            compare(
                &command,
                &outcome(0, expected, b""),
                &outcome(0, actual, b""),
            ),
            zip_stdout_difference(&command, expected, actual),
        )
    }

    #[test]
    fn a_command_zip_on_stdout_ignores_time_stamps_and_compression_level() {
        let expected = oracle_archive();
        let actual = archive_bytes(&[
            ("mimetype", b"application/epub+zip", STORED, None, 2030),
            ("sitemap.json", b"{}", STORED, None, 2030),
            ("index.html", PAGE, DEFLATED, Some(1), 2030),
        ]);
        assert_ne!(expected, actual);
        let (verdict, difference) = command_verdict(&expected, &actual);
        assert!(!verdict.bytes_ok && verdict.content_ok && verdict.pass);
        assert_eq!(difference, None);
    }

    #[test]
    fn a_command_zip_on_stdout_with_other_entries_fails_with_the_entry_names() {
        let expected = oracle_archive();
        let cases: [(Vec<u8>, &str); 5] = [
            (
                archive_bytes(&[
                    ("mimetype", b"application/epub+zip", STORED, None, 2023),
                    ("index.html", b"<p>changed</p>", DEFLATED, Some(9), 2023),
                    ("sitemap.json", b"{}", DEFLATED, Some(9), 2023),
                ]),
                "ZIP stdout: different entry \"index.html\"",
            ),
            (
                archive_bytes(&[
                    ("mimetype", b"application/epub+zip", STORED, None, 2023),
                    ("index.html", PAGE, DEFLATED, Some(9), 2023),
                ]),
                "ZIP stdout: missing entry \"sitemap.json\"",
            ),
            (
                archive_bytes(&[
                    ("mimetype", b"application/epub+zip", STORED, None, 2023),
                    ("index.html", PAGE, DEFLATED, Some(9), 2023),
                    ("sitemap.json", b"{}", DEFLATED, Some(9), 2023),
                    ("extra.css", b"p {}", DEFLATED, Some(9), 2023),
                ]),
                "ZIP stdout: extra entry \"extra.css\"",
            ),
            (
                archive_bytes(&[
                    ("index.html", PAGE, DEFLATED, Some(9), 2023),
                    ("mimetype", b"application/epub+zip", STORED, None, 2023),
                    ("sitemap.json", b"{}", DEFLATED, Some(9), 2023),
                ]),
                "ZIP stdout: first entry \"mimetype\", randoc gives \"index.html\"",
            ),
            (
                archive_bytes(&[
                    ("mimetype", b"application/epub+zip", DEFLATED, Some(9), 2023),
                    ("index.html", PAGE, DEFLATED, Some(9), 2023),
                    ("sitemap.json", b"{}", DEFLATED, Some(9), 2023),
                ]),
                "ZIP stdout: first entry \"mimetype\" is compressed, the oracle stores it",
            ),
        ];
        for (actual, message) in cases {
            let (verdict, difference) = command_verdict(&expected, &actual);
            assert!(!verdict.content_ok && !verdict.pass, "{message}");
            assert_eq!(difference.as_deref(), Some(message));
        }
    }

    #[test]
    fn a_damaged_zip_on_stdout_needs_the_same_bytes() {
        let valid = oracle_archive();
        let truncated = &valid[..valid.len() - 10];
        assert!(truncated.starts_with(ZIP_MAGIC) && read_archive(truncated).is_none());
        let mut damaged = valid.clone();
        let end = damaged.len() - 22;
        damaged[end..end + 4].copy_from_slice(b"XXXX");
        assert!(read_archive(&damaged).is_none());

        let not_valid = Some("stdout is not a valid ZIP archive, the bytes are different");
        for (expected, actual) in [
            (truncated, valid.as_slice()),
            (valid.as_slice(), truncated),
            (damaged.as_slice(), valid.as_slice()),
            (truncated, damaged.as_slice()),
        ] {
            let (verdict, difference) = command_verdict(expected, actual);
            assert!(!verdict.content_ok && !verdict.pass);
            assert_eq!(difference.as_deref(), not_valid);
        }
        let (verdict, difference) = command_verdict(truncated, truncated);
        assert!(verdict.bytes_ok && verdict.pass);
        assert_eq!(difference, None);
    }

    #[test]
    fn command_stdout_that_is_not_a_zip_needs_the_same_bytes() {
        let valid = oracle_archive();
        let (verdict, difference) = command_verdict(b"PK text\n", b"PK text \n");
        assert!(!verdict.content_ok && !verdict.pass);
        assert_eq!(difference, None);
        // Only the oracle stdout selects the ZIP comparison.
        let (verdict, difference) = command_verdict(b"text\n", &valid);
        assert!(!verdict.pass);
        assert_eq!(difference, None);
        let (verdict, _) = command_verdict(&valid, b"text\n");
        assert!(!verdict.pass);
    }

    #[test]
    fn a_zip_comparison_does_not_apply_to_text_formats() {
        let html = unit(UnitKind::Write, Some("html"));
        let a = zip_bytes(&[("a", b"1"), ("b", b"2")], false);
        let b = zip_bytes(&[("b", b"2"), ("a", b"1")], false);
        assert!(!compare(&html, &outcome(0, &a, b""), &outcome(0, &b, b"")).pass);
    }

    #[test]
    fn a_zip_writer_unit_needs_the_stored_mimetype_first_only_when_the_oracle_has_it() {
        let write = |to: &str, expected: &[u8], actual: &[u8]| {
            compare(
                &unit(UnitKind::Write, Some(to)),
                &outcome(0, expected, b""),
                &outcome(0, actual, b""),
            )
            .pass
        };
        let expected = oracle_archive();
        let reordered = archive_bytes(&[
            ("mimetype", b"application/epub+zip", STORED, None, 2030),
            ("sitemap.json", b"{}", DEFLATED, Some(1), 2030),
            ("index.html", PAGE, DEFLATED, Some(1), 2030),
        ]);
        assert!(write("epub", &expected, &reordered));
        let mimetype_later = archive_bytes(&[
            ("index.html", PAGE, DEFLATED, Some(9), 2023),
            ("mimetype", b"application/epub+zip", STORED, None, 2023),
            ("sitemap.json", b"{}", DEFLATED, Some(9), 2023),
        ]);
        assert!(!write("epub", &expected, &mimetype_later));
        let compressed = archive_bytes(&[
            ("mimetype", b"application/epub+zip", DEFLATED, Some(9), 2023),
            ("index.html", PAGE, DEFLATED, Some(9), 2023),
            ("sitemap.json", b"{}", DEFLATED, Some(9), 2023),
        ]);
        assert!(!write("epub", &expected, &compressed));
        let docx = archive_bytes(&[
            ("[Content_Types].xml", b"<Types/>", DEFLATED, Some(9), 2023),
            ("word/document.xml", PAGE, DEFLATED, Some(9), 2023),
        ]);
        let docx_reordered = archive_bytes(&[
            ("word/document.xml", PAGE, DEFLATED, Some(9), 2023),
            ("[Content_Types].xml", b"<Types/>", DEFLATED, Some(9), 2023),
        ]);
        assert!(write("docx", &docx, &docx_reordered));
    }
}
