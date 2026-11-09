//! Comparison of a Randoc result with the oracle result.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use serde::Serialize;

use crate::cases::{Outcome, Unit, UnitKind};

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
    /// The unit passes.
    pub pass: bool,
}

/// Returns `true` when the unit passes on content and not on bytes.
///
/// A read unit compares document trees. A ZIP format compares archive entries.
/// Each other unit must have the same bytes.
fn content_is_enough(unit: &Unit) -> bool {
    unit.kind == UnitKind::Read
        || unit
            .to
            .as_deref()
            .is_some_and(|to| ZIP_FORMATS.contains(&to))
}

/// Reads each entry of an archive. An archive that is not valid, that is too
/// large or that has two entries with one name gives `None`.
fn zip_entries(bytes: &[u8]) -> Option<BTreeMap<String, Vec<u8>>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    if archive.len() > MAX_ARCHIVE_ENTRIES || archive.has_duplicate_names() {
        return None;
    }
    let mut entries = BTreeMap::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).ok()?;
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
        if entries
            .insert(file.name().ok()?.into_owned(), contents)
            .is_some()
        {
            return None;
        }
    }
    Some(entries)
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
    if unit
        .to
        .as_deref()
        .is_some_and(|to| ZIP_FORMATS.contains(&to))
    {
        return matches!((zip_entries(expected), zip_entries(actual)), (Some(a), Some(b)) if a == b);
    }
    false
}

/// Compares the outcome of Randoc with the outcome of the oracle.
pub fn compare(unit: &Unit, expected: &Outcome, actual: &Outcome) -> Verdict {
    let expected_out = expected.stdout_bytes();
    let actual_out = actual.stdout_bytes();
    let exit_ok = expected.exit == actual.exit;
    let stderr_ok = expected.stderr_bytes() == actual.stderr_bytes();
    let bytes_ok = expected_out == actual_out;
    let content_ok = same_content(unit, &expected_out, &actual_out);
    let output_ok = if content_is_enough(unit) {
        content_ok
    } else {
        bytes_ok
    };
    Verdict {
        exit_ok,
        stderr_ok,
        bytes_ok,
        content_ok,
        pass: exit_ok && stderr_ok && output_ok,
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
            from: None,
            to: to.map(str::to_owned),
            covers: Vec::new(),
        }
    }

    fn outcome(exit: i32, stdout: &[u8], stderr: &[u8]) -> Outcome {
        Outcome::new(exit, stdout.to_vec(), stderr.to_vec())
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
        assert!(zip_entries(&actual).is_none());
    }

    #[test]
    fn a_zip_comparison_does_not_apply_to_text_formats() {
        let html = unit(UnitKind::Write, Some("html"));
        let a = zip_bytes(&[("a", b"1"), ("b", b"2")], false);
        let b = zip_bytes(&[("b", b"2"), ("a", b"1")], false);
        assert!(!compare(&html, &outcome(0, &a, b""), &outcome(0, &b, b"")).pass);
    }
}
