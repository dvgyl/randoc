//! Tests for PDF conversion through the command-line program.

#![cfg(unix)]
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, reason = "a fixture error must stop the test")]

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const RANDOC: &str = env!("CARGO_BIN_EXE_randoc");
const PDF: &[u8] = b"%PDF-mock\n";
const ENGINE_STDERR: &str = "PDF engine stderr\n";
const TERMINAL_ERROR: &str = concat!(
    "Cannot write pdf output to terminal.\n",
    "Specify an output file using the -o option, or use '-o -' to force output to stdout.\n",
);
const BAD_ENGINE: &str = concat!(
    "Argument of --pdf-engine must be one of\n",
    "\tweasyprint\n\twkhtmltopdf\n\tpagedjs-cli\n\tprince\n\tpdflatex\n",
    "\tlualatex\n\txelatex\n\tlatexmk\n\ttectonic\n\tpdflatex-dev\n",
    "\tlualatex-dev\n\tgroff\n\tpdfroff\n\ttypst\n\tcontext\n\n",
);
const MOCK_ENGINE: &str = r#"#!/bin/sh
set -eu
printf '%s\n' "$@" > "$RANDOC_PDF_TEST_DIR/arguments"
printf 'PDF engine stderr\n' >&2
case "${0##*/}" in
    pdflatex|pdflatex.exe)
        while [ "$#" -gt 1 ]; do shift; done
        /bin/cat "$1" > "$RANDOC_PDF_TEST_DIR/intermediate"
        if [ -f "$RANDOC_PDF_TEST_DIR/local.png" ]; then
            cmp "$RANDOC_PDF_TEST_DIR/local.png" "${1%/*}/local.png"
        fi
        printf '%%PDF-mock\n' > "${1%/*}/input.pdf"
        ;;
    groff|pdfroff)
        /bin/cat > "$RANDOC_PDF_TEST_DIR/intermediate"
        printf '%%PDF-mock\n'
        ;;
    context)
        /bin/cat input.tex > "$RANDOC_PDF_TEST_DIR/intermediate"
        printf '%%PDF-mock\n' > input.pdf
        ;;
    *)
        while [ "$#" -gt 2 ]; do shift; done
        /bin/cat "$1" > "$RANDOC_PDF_TEST_DIR/intermediate"
        printf '%%PDF-mock\n' > "$2"
        ;;
esac
"#;

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir()
            .join("w6-int1")
            .join(format!("randoc PDF {} {name}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn executable(&self, name: &str, script: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(&self.0)
            .env("TMPDIR", &self.0)
            .env("HOME", &self.0)
            .env("XDG_DATA_HOME", &self.0)
            .env("RANDOC_PDF_TEST_DIR", &self.0)
            .env(
                "PATH",
                std::env::join_paths([self.0.as_path(), Path::new("/usr/bin")]).unwrap(),
            );
        command
    }

    fn run(&self, args: &[&str], input: &str) -> Output {
        let mut child = self
            .command(RANDOC)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn read_text(&self, name: &str) -> String {
        fs::read_to_string(self.0.join(name)).unwrap()
    }

    fn assert_pdf(&self, result: &Output, name: &str) {
        assert_exit(result, 0);
        assert!(result.stdout.is_empty());
        assert_eq!(fs::read(self.0.join(name)).unwrap(), PDF);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn assert_exit(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn pdf_uses_the_selected_writer_and_engine() {
    let fixture = Fixture::new("writers");
    for (writer, engine, standalone_marker) in [
        ("latex+smart", "pdflatex.exe", "\\documentclass"),
        ("beamer+smart", "pdflatex.exe", "{beamer}"),
        ("typst", "typst", "#let conf("),
        ("html5", "weasyprint", "<!DOCTYPE html>"),
        ("ms", "groff", ".pdfinfo"),
        ("context", "context", "\\starttext"),
    ] {
        let engine = fixture.executable(engine, MOCK_ENGINE);
        let output = fixture.run(
            &[
                "-t",
                writer,
                "--pdf-engine",
                engine.to_str().unwrap(),
                "-o",
                "out.PDF",
                "--pdf-engine-opt=one argument",
                "--pdf-engine-opt=two",
            ],
            "---\ntitle: PDF fixture\n---\n\nA short paragraph.\n",
        );
        fixture.assert_pdf(&output, "out.PDF");
        assert_eq!(output.stderr, ENGINE_STDERR.as_bytes());
        let text = fixture.read_text("intermediate");
        assert!(text.contains(standalone_marker), "{writer}: {text}");
        assert!(text.contains("A short paragraph."), "{writer}: {text}");
        assert!(
            fixture
                .read_text("arguments")
                .contains("one argument\ntwo\n")
        );
    }
}

#[test]
fn pdf_can_write_to_piped_stdout() {
    let fixture = Fixture::new("stdout");
    fixture.executable("pdflatex", MOCK_ENGINE);
    for args in [
        &["-t", "pdf", "-o", "-", "--quiet"][..],
        &["-t", "pdf"],
        &["-t", "pdf", "-o", "-", "--verbose"],
    ] {
        let output = fixture.run(args, "Text\n");
        assert_exit(&output, 0);
        assert_eq!(output.stdout, PDF);
        assert!(String::from_utf8_lossy(&output.stderr).contains(ENGINE_STDERR));
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn pdf_without_an_output_option_refuses_a_terminal() {
    let fixture = Fixture::new("terminal");
    let mut command = fixture.command("/usr/bin/script");
    if cfg!(target_os = "macos") {
        command.args(["-q", "/dev/null", RANDOC, "-t", "pdf"]);
    } else {
        command.env("RANDOC_PDF_TEST_CLI", RANDOC).args([
            "-q",
            "-e",
            "-c",
            r#"exec "$RANDOC_PDF_TEST_CLI" -t pdf"#,
            "/dev/null",
        ]);
    }
    let output = command.stdin(Stdio::null()).output().unwrap();
    assert_exit(&output, 4);
    let text = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert!(text.contains(TERMINAL_ERROR), "{text:?}");
}

#[test]
fn pdf_engine_and_writer_errors_have_pandoc_exit_codes() {
    let fixture = Fixture::new("errors");
    fs::write(fixture.0.join("invalid.yaml"), "pdf-engine: foo\n").unwrap();
    let missing = fixture.0.join("missing/pdflatex");
    let missing = missing.to_str().unwrap();
    let missing_error = format!(
        "'{missing}' not found. Please select a different --pdf-engine or install '{missing}'\n"
    );
    for (args, code, message) in [
        (&["--pdf-engine", "foo"][..], 6, BAD_ENGINE),
        (&["--pdf-engine", "pdflatex/"], 6, BAD_ENGINE),
        (
            &["-o", "out.pdf", "-d", "invalid.yaml"],
            4,
            "pdf-engine foo not known\n",
        ),
        (
            &["-t", "html", "--pdf-engine", "pdflatex", "-o", "out.pdf"],
            4,
            "pdf-engine pdflatex is not compatible with output format html\n",
        ),
        (
            &["-t", "markdown", "-o", "out.pdf"],
            4,
            "cannot produce pdf output from markdown\n",
        ),
        (&["-t", "pdf+smart", "-o", "out.pdf"], 4, "pdf writer\n"),
        (
            &["-t", "pdf+smart"],
            22,
            concat!(
                "Unknown output format 'pdf'To create a pdf using pandoc, ",
                "use -t latex|beamer|context|ms|html5|typst\n",
                "and specify an output file with .pdf extension (-o filename.pdf).\n",
            ),
        ),
        (
            &["-t", "pdf", "-o", "-", "--pdf-engine", missing],
            47,
            missing_error.as_str(),
        ),
    ] {
        let output = fixture.run(args, "Text\n");
        assert_exit(&output, code);
        assert_eq!(output.stderr, message.as_bytes(), "{args:?}");
        assert!(output.stdout.is_empty());
    }
    let html = fixture.run(&["-d", "invalid.yaml", "-t", "html"], "Text\n");
    assert_exit(&html, 0);
    assert_eq!(html.stdout, b"<p>Text</p>\n");
}

#[test]
fn pdf_engines_get_template_variables_and_metadata() {
    let fixture = Fixture::new("variables");
    let input = "---\ntitle: Meta title\npapersize: letter\nmargin-top: 2in\n---\n\nText\n";
    for engine in ["groff", "pdfroff", "wkhtmltopdf"] {
        fixture.executable(engine, MOCK_ENGINE);
        for variables in [&[][..], &["-V", "papersize=a4", "-V", "margin-top=1in"]] {
            let mut args = vec!["--pdf-engine", engine, "-o", "out.pdf"];
            args.extend_from_slice(variables);
            fixture.assert_pdf(&fixture.run(&args, input), "out.pdf");
            let actual = fixture.read_text("arguments");
            if engine == "wkhtmltopdf" {
                let expected = if variables.is_empty() {
                    "--page-size\nletter\n--title\nMeta title\n--margin-bottom\n1.2in\n--margin-top\n2in\n"
                } else {
                    "--page-size\na4\n--title\nMeta title\n--margin-bottom\n1.2in\n--margin-top\n1in\n"
                };
                assert!(actual.starts_with(expected), "{actual:?}");
            } else if variables.is_empty() {
                assert!(!actual.contains("-P-p"), "{actual:?}");
            } else {
                assert!(actual.ends_with("-P-pa4\n"), "{actual:?}");
            }
        }
    }
}

#[test]
fn sandbox_pdf_runs_the_engine_and_stages_a_local_image() {
    let fixture = Fixture::new("sandbox");
    fixture.executable("pdflatex", MOCK_ENGINE);
    fs::write(fixture.0.join("local.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    let output = fixture.run(&["--sandbox", "-o", "out.pdf"], "![Caption](local.png)\n");
    fixture.assert_pdf(&output, "out.pdf");
    let tex = fixture.read_text("intermediate");
    assert!(
        tex.contains("includegraphics") && tex.contains("/local.png"),
        "{tex}"
    );
}

#[test]
fn pdf_json_filter_gets_the_intermediate_writer() {
    let fixture = Fixture::new("json filter");
    fixture.executable("pdflatex", MOCK_ENGINE);
    let filter = fixture.executable("filter", "#!/bin/sh\n[ \"$1\" = latex ] && exec /bin/cat\n");
    let output = fixture.run(
        &["-o", "out.pdf", "--filter", filter.to_str().unwrap()],
        "Text\n",
    );
    fixture.assert_pdf(&output, "out.pdf");
}

#[cfg(feature = "lua")]
#[test]
fn pdf_lua_filter_gets_the_writer_format_and_template() {
    let fixture = Fixture::new("lua filter");
    fixture.executable("pdflatex", MOCK_ENGINE);
    fs::write(
        fixture.0.join("filter.lua"),
        r#"function Pandoc(doc)
          assert(FORMAT == "latex")
          assert(PANDOC_WRITER_OPTIONS.template ~= nil)
          assert(tostring(PANDOC_WRITER_OPTIONS.variables["pdf-engine"]) == "pdflatex")
          assert(PANDOC_READER_OPTIONS.standalone)
          return doc
        end"#,
    )
    .unwrap();
    let output = fixture.run(&["-o", "out.pdf", "--lua-filter", "filter.lua"], "Text\n");
    fixture.assert_pdf(&output, "out.pdf");
}
