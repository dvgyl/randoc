//! The oracle matrix of the style loader: each form of a style path and of
//! the `href` of a parent style, and the decoding of a style file.
//!
//! The expected exit codes and texts come from Pandoc 3.13 (`pandoc
//! --citeproc --csl PATH`, run in the folder `D`). The test makes the same
//! files in a new temporary folder; `D` stands for that folder.

#![allow(clippy::unwrap_used, clippy::panic)]

use randoc_core::Context;

const PARENT: &str = "<style xmlns=\"http://purl.org/net/xbiblio/csl\" class=\"in-text\" version=\"1.0\"><citation><layout><text value=\"p\"/></layout></citation></style>";

fn dependent(href: &str) -> String {
    format!(
        "<style xmlns=\"http://purl.org/net/xbiblio/csl\" class=\"in-text\" version=\"1.0\"><info><link rel=\"independent-parent\" href=\"{href}\"/></info></style>"
    )
}

/// The result of `read_style`: the exit code and the text, or `0` and `ok`.
fn load(folder: &str, path: &str) -> (i32, String) {
    let mut context = Context::native();
    std::env::set_current_dir(folder).unwrap();
    match randoc_citeproc::read_style(&mut context, path) {
        Ok(_) => (0, "ok".into()),
        Err(error) => (
            error.exit_code(),
            error
                .to_string()
                .replace(folder, "D")
                .replace(&folder.replace(' ', "%20"), "D"),
        ),
    }
}

#[test]
fn the_loader_gives_the_oracle_result_for_each_form() {
    let folder = std::env::temp_dir().join(format!("citeproc-loader-test-{}", std::process::id()));
    std::fs::create_dir_all(folder.join("dir")).unwrap();
    let folder = folder.to_str().unwrap().to_owned();
    std::fs::write(format!("{folder}/parent.csl"), PARENT).unwrap();
    std::fs::create_dir_all(format!("{folder}/sp ace")).unwrap();
    std::fs::write(format!("{folder}/sp ace/p.csl"), PARENT).unwrap();
    // The file name of a link is found in `D` first, so each absolute
    // `file:` URL of `D/parent.csl` gives the parent, also when the folder
    // name has a space (oracle probe of 2026-11-15, Opus citeproc-R3b-04).
    let parents: [(&str, String, i32, &str); 33] = [
        (
            "missing-escape",
            "missing%65.csl".into(),
            99,
            "File 'missinge.csl' not found in resource path",
        ),
        ("file-upper", "FILE:parent.csl".into(), 0, "ok"),
        ("file-mixed", "File:parent.csl".into(), 0, "ok"),
        (
            "file-upper-escape",
            "FILE:par%65nt.csl".into(),
            99,
            "File 'par%65nt.csl' not found in resource path",
        ),
        (
            "file-query-slash",
            "file://h?x=/dev/null".into(),
            99,
            "File '' not found in resource path",
        ),
        (
            "file-fragment-slash",
            "file://h#/dev/null".into(),
            99,
            "File '' not found in resource path",
        ),
        ("rel", "parent.csl".into(), 0, "ok"),
        (
            "rel-missing",
            "nope.csl".into(),
            99,
            "File 'nope.csl' not found in resource path",
        ),
        (
            "abs-missing",
            "/nonexistent/p.csl".into(),
            1,
            "/nonexistent/p.csl: withBinaryFile: does not exist (No such file or directory)",
        ),
        ("abs-ok", format!("{folder}/parent.csl"), 0, "ok"),
        (
            "folder-abs",
            format!("{folder}/dir"),
            1,
            "D/dir: withBinaryFile: inappropriate type (is a directory)",
        ),
        (
            "folder-rel",
            "dir".into(),
            99,
            "File 'dir' not found in resource path",
        ),
        ("file-rel", "file:parent.csl".into(), 0, "ok"),
        ("file-abs", format!("file://{folder}/parent.csl"), 0, "ok"),
        (
            "file-abs-missing",
            "file:///nonexistent/p.csl".into(),
            1,
            "/nonexistent/p.csl: withBinaryFile: does not exist (No such file or directory)",
        ),
        (
            "file-localhost",
            format!("file://localhost{folder}/parent.csl"),
            0,
            "ok",
        ),
        (
            "file-one-slash",
            format!("file:{folder}/parent.csl"),
            0,
            "ok",
        ),
        (
            "file-encoded",
            format!("file://{folder}/par%65nt.csl"),
            0,
            "ok",
        ),
        (
            "file-rel-space",
            "file:sp ace/p.csl".into(),
            99,
            "File 'sp%20ace/p.csl' not found in resource path",
        ),
        (
            "file-rel-escape",
            "file:par%65nt.csl".into(),
            99,
            "File 'par%65nt.csl' not found in resource path",
        ),
        // The file name of the link is found first (Opus citeproc-R3b-04;
        // probe of 2026-11-15 in a copy of `D`).
        ("file-host", "file://dir/parent.csl".into(), 0, "ok"),
        ("name-abs", "/nonexistent/dir/parent.csl".into(), 0, "ok"),
        (
            "name-file-url",
            "file:///nonexistent/dir/parent.csl".into(),
            0,
            "ok",
        ),
        ("name-no-extension", "nonexistent/parent".into(), 0, "ok"),
        ("name-escape", "x/par%65nt.csl".into(), 0, "ok"),
        (
            "name-fragment",
            "x/parent.csl#frag".into(),
            99,
            "File 'x/parent.csl#frag' not found in resource path",
        ),
        ("file-fragment", "file:parent.csl#x".into(), 0, "ok"),
        ("file-query", "file:parent.csl?x".into(), 0, "ok"),
        (
            "file-bad-escape",
            "file:par%zz.csl".into(),
            99,
            "File 'file:par%zz.csl' not found in resource path",
        ),
        ("plain-space", "sp ace/p.csl".into(), 0, "ok"),
        ("plain-escape", "sp%20ace/p.csl".into(), 0, "ok"),
        (
            "file-dir",
            "file:dir".into(),
            99,
            "File 'dir' not found in resource path",
        ),
        ("dot", "./parent.csl".into(), 0, "ok"),
    ];
    let mut failures = Vec::new();
    for (name, href, code, text) in parents {
        std::fs::write(format!("{folder}/dep.csl"), dependent(&href)).unwrap();
        let got = load(&folder, "dep.csl");
        if got != (code, text.to_owned()) {
            failures.push(format!("parent {name}: {got:?}"));
        }
    }
    let files: [(&str, &[u8], i32, &str); 8] = [
        ("bad-utf8", b"<style><citation><layout><text value=\"\xff\"/></layout></citation></style>", 92, "UTF-8 decoding error in 's.csl' at byte offset 38 (ff).\nThe input must be a UTF-8 encoded text."),
        ("bad-cont", b"<style><citation><layout><text value=\"\xc3(\"/></layout></citation></style>", 92, "UTF-8 decoding error in 's.csl' at byte offset 39 (28).\nThe input must be a UTF-8 encoded text."),
        ("truncated", b"<style><citation><layout><text value=\"a\"/></layout></citation></style>\xe2\x82", 92, "UTF-8 decoding error in 's.csl' at byte offset 70 (e2).\nThe input must be a UTF-8 encoded text."),
        ("latin1", b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><style><citation><layout><text value=\"\xe9\"/></layout></citation></style>", 92, "UTF-8 decoding error in 's.csl' at byte offset 82 (22).\nThe input must be a UTF-8 encoded text."),
        ("utf16", b"\xff\xfe<\x00", 92, "UTF-8 decoding error in 's.csl' at byte offset 0 (ff).\nThe input must be a UTF-8 encoded text."),
        ("cr", b"<style><citation><layout><text value=\"a\rb\"/></layout></citation></style>", 0, "ok"),
        ("bom", b"\xef\xbb\xbf<style><citation><layout><text value=\"x\"/></layout></citation></style>", 0, "ok"),
        ("crlf", b"<style>\r\n<citation><layout><text value=\"x\"/></layout></citation></style>", 0, "ok"),
    ];
    for (name, bytes, code, text) in files {
        std::fs::write(format!("{folder}/s.csl"), bytes).unwrap();
        let got = load(&folder, "s.csl");
        if got != (code, text.to_owned()) {
            failures.push(format!("file {name}: {got:?}"));
        }
    }
    let paths: [(&str, String, i32, &str); 5] = [
        (
            "missing-rel",
            "nope.csl".into(),
            99,
            "File 'nope.csl' not found in resource path",
        ),
        (
            "missing-abs",
            "/nonexistent/s.csl".into(),
            1,
            "/nonexistent/s.csl: withBinaryFile: does not exist (No such file or directory)",
        ),
        (
            "folder",
            "dir".into(),
            99,
            "File 'dir.csl' not found in resource path",
        ),
        (
            "folder-abs",
            format!("{folder}/dir"),
            1,
            "D/dir.csl: withBinaryFile: does not exist (No such file or directory)",
        ),
        ("file-url", "file:s.csl".into(), 0, "ok"),
    ];
    for (name, path, code, text) in paths {
        let got = load(&folder, &path);
        if got != (code, text.to_owned()) {
            failures.push(format!("path {name}: {got:?}"));
        }
    }
    std::fs::remove_dir_all(&folder).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn carriage_returns_go_before_the_parser() {
    // Oracle: `a\rb` in a term renders `Ab`, `a\r\nb` renders `A b`.
    let folder = std::env::temp_dir().join(format!("citeproc-cr-test-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("t.csl");
    let style = |term: &str| {
        format!(
            "<style><locale><terms><term name=\"and\">{term}</term></terms></locale><citation><layout><text term=\"and\"/></layout></citation></style>"
        )
    };
    let term_text = |bytes: String| {
        std::fs::write(&path, bytes).unwrap();
        let style =
            randoc_citeproc::read_style(&mut Context::native(), path.to_str().unwrap()).unwrap();
        format!("{:?}", style.locales)
    };
    assert!(term_text(style("a\rb")).contains("\"ab\""));
    assert!(term_text(style("a\r\nb")).contains("\"a\\nb\""));
    std::fs::remove_dir_all(&folder).unwrap();
}
