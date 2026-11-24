//! Process and staging tests with an in-memory host.

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::path::Path;

use randoc_archive::media::sha1_hex;
use randoc_ast::{Attr, Block, Inline, MetaValue, Pandoc, Target};
use randoc_core::{
    Context, Error, Host, MediaItem, Output, ProcessOutput, ProcessRequest, Result, Text,
    WriterOptions,
};
use randoc_pdf::{PDF_ENGINES, PdfHost, PdfOptions, make_pdf, resolve_pdf_plan};

const CWD: &str = "/work/dir with spaces";
const TMP: &str = "/work/test run";
const TEX_ROOT: &str = "/work/test run/media-test";
const LOCAL_ROOT: &str = "/work/dir with spaces/media-test";
const SOURCE: &str = "Intermediate document.";
const PDF: &[u8] = b"%PDF-1.7";
const ARG: &str = "argument with spaces";

#[derive(Default)]
struct Pass {
    status: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    log: Vec<u8>,
    toc: bool,
    pdf: Option<Vec<u8>>,
}

#[derive(Debug)]
struct Call {
    program: Text,
    argv: Vec<Text>,
    cwd: Text,
    environment: BTreeMap<Text, Option<Text>>,
    input: Vec<u8>,
}

#[derive(Default)]
struct Mock {
    files: BTreeMap<Text, Vec<u8>>,
    calls: Vec<Call>,
    passes: VecDeque<Pass>,
    converter: Option<Pass>,
    reads: Vec<Text>,
    remote_media: BTreeMap<Text, MediaItem>,
    request_headers: Vec<(Text, Text)>,
    writes: BTreeMap<Text, Vec<u8>>,
    racing_file: Option<Text>,
    removed: Vec<Text>,
    tmp_root: Text,
    texinputs: Option<Text>,
    pdf_path: Option<Text>,
    log_dir: Option<Text>,
    spawn_error: Option<io::ErrorKind>,
    sandbox: bool,
}

impl Host for Mock {
    fn is_sandboxed(&self) -> bool {
        self.sandbox
    }

    fn working_directory(&mut self) -> io::Result<Text> {
        Ok(CWD.into())
    }

    fn environment_variable(&mut self, name: &str) -> Option<Text> {
        match name {
            "TMPDIR" => Some(TMP.into()),
            "TEXINPUTS" => self.texinputs.clone(),
            _ => None,
        }
    }

    fn create_temporary_directory(&mut self, parent: &str, template: &str) -> io::Result<Text> {
        self.tmp_root = format!("{parent}/{template}-test");
        Ok(self.tmp_root.clone())
    }

    fn read_file(&mut self, path: &str) -> io::Result<Vec<u8>> {
        self.reads.push(path.into());
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn write_file(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
        self.writes.insert(path.into(), bytes.to_vec());
        self.files.insert(path.into(), bytes.to_vec());
        Ok(())
    }

    fn fetch_url(
        &mut self,
        url: &str,
        headers: &[(Text, Text)],
    ) -> Result<(Vec<u8>, Option<Text>)> {
        self.request_headers = headers.to_vec();
        self.remote_media
            .get(url)
            .map(|item| (item.contents.clone(), Some(item.mime_type.clone())))
            .ok_or_else(|| Error::Http {
                url: url.into(),
                message: "Image is missing.".into(),
            })
    }

    fn file_exists(&mut self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn remove_file(&mut self, path: &str) -> io::Result<()> {
        self.removed.push(path.into());
        self.files.remove(path);
        Ok(())
    }

    fn remove_directory(&mut self, path: &str, recursive: bool) -> io::Result<()> {
        assert!(recursive);
        self.removed.push(path.into());
        self.files
            .retain(|name, _| !name.starts_with(&format!("{path}/")));
        Ok(())
    }

    fn run_process_with(&mut self, request: &ProcessRequest<'_>) -> io::Result<ProcessOutput> {
        self.calls.push(Call {
            program: request.program.into(),
            argv: request.arguments.to_vec(),
            cwd: request.working_directory.unwrap_or_default().into(),
            environment: request.environment.iter().cloned().collect(),
            input: request.input.to_vec(),
        });
        if request.program == "rsvg-convert" {
            let pass = self.converter.take().ok_or(io::ErrorKind::NotFound)?;
            assert!(self.files.contains_key(&request.arguments[9]));
            if let Some(pdf) = pass.pdf {
                self.files.insert(request.arguments[8].clone(), pdf);
            }
            return Ok(ProcessOutput {
                exit_code: pass.status,
                stdout: pass.stdout,
                stderr: pass.stderr,
            });
        }
        if let Some(kind) = self.spawn_error {
            return Err(kind.into());
        }
        let pass = self.passes.pop_front().unwrap_or_default();
        if let Some(path) = &self.pdf_path
            && let Some(pdf) = pass.pdf
        {
            self.files.insert(path.clone(), pdf);
        }
        if let Some(dir) = &self.log_dir {
            let toc = format!("{dir}/input.toc");
            if pass.toc {
                self.files.insert(toc, Vec::new());
            } else {
                self.files.remove(&toc);
            }
            self.files.insert(format!("{dir}/input.log"), pass.log);
        }
        Ok(ProcessOutput {
            exit_code: pass.status,
            stdout: pass.stdout,
            stderr: pass.stderr,
        })
    }
}

impl PdfHost for Mock {
    fn create_new_file(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
        if self.racing_file.as_deref() == Some(path) {
            self.files.insert(path.into(), b"Concurrent file".to_vec());
        }
        if self.files.contains_key(path) {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        self.write_file(path, bytes)
    }
}

fn render(
    host: &mut Mock,
    engine: &str,
    context: &mut Context,
    doc: &Pandoc,
    options: PdfOptions<'_>,
) -> Result<Vec<u8>> {
    make_pdf(
        &resolve_pdf_plan(None, Some(engine))?,
        doc,
        &WriterOptions::default(),
        context,
        host,
        options,
        |_, _, _| Ok(Output::Text(SOURCE.into())),
    )
}

fn arguments(expected: &[&str], extra: &[&str], tail: &[&str]) -> Vec<Text> {
    expected
        .iter()
        .chain(extra)
        .chain(tail)
        .map(|part| (*part).into())
        .collect()
}

#[test]
fn all_engine_process_contracts() -> Result<()> {
    let input_tex = format!("{TEX_ROOT}/input.tex");
    for engine in PDF_ENGINES {
        let mut host = Mock::default();
        let mut cwd = CWD;
        let mut writer = "latex";
        let mut expected_source: Option<Text> = None;
        let mut stream = false;
        let mut tex_env = false;
        let mut root = TEX_ROOT;
        let mut output_name = "input.pdf";
        let args = match engine {
            "pdflatex" | "pdflatex-dev" | "xelatex" | "lualatex" | "lualatex-dev" => {
                tex_env = true;
                expected_source = Some(input_tex.clone());
                arguments(
                    &[
                        "-halt-on-error",
                        "-interaction",
                        "nonstopmode",
                        "-output-directory",
                        TEX_ROOT,
                    ],
                    &[ARG],
                    &[&input_tex],
                )
            }
            "latexmk" => {
                tex_env = true;
                expected_source = Some(input_tex.clone());
                arguments(
                    &[
                        "-interaction=batchmode",
                        "-halt-on-error",
                        "-pdf",
                        "-quiet",
                        &format!("-outdir={TEX_ROOT}"),
                    ],
                    &[ARG],
                    &[&input_tex],
                )
            }
            "tectonic" => {
                stream = true;
                output_name = "texput.pdf";
                arguments(&["--outdir", TEX_ROOT], &[ARG], &["-"])
            }
            "context" => {
                writer = "context";
                cwd = TEX_ROOT;
                expected_source = Some(input_tex.clone());
                arguments(&["--batchmode"], &[ARG], &["input.tex"])
            }
            "groff" | "pdfroff" => {
                writer = "ms";
                stream = true;
                let flags: &[&str] = if engine == "groff" {
                    &["-ms", "-Tpdf", "-e", "-t", "-k", "-KUTF-8"]
                } else {
                    &[
                        "-ms",
                        "-mpdfmark",
                        "-mspdf",
                        "-e",
                        "-t",
                        "-k",
                        "-KUTF-8",
                        "-i",
                    ]
                };
                arguments(flags, &[ARG], &[])
            }
            _ => {
                root = LOCAL_ROOT;
                let output = "./toPdfViaTempFilemedia-test-1.pdf";
                let source = if engine == "typst" {
                    writer = "typst";
                    "./toPdfViaTempFilemedia-test-0.typ"
                } else {
                    writer = "html";
                    "./toPdfViaTempFilemedia-test-0.html"
                };
                expected_source = Some(format!("{CWD}/{}", &source[2..]));
                output_name = &output[2..];
                match engine {
                    "typst" => arguments(&["compile"], &[ARG], &[source, output]),
                    "weasyprint" => {
                        arguments(&["--presentational-hints"], &[ARG], &[source, output])
                    }
                    "wkhtmltopdf" => arguments(
                        &[
                            "--margin-bottom",
                            "1.2in",
                            "--margin-top",
                            "1.25in",
                            "--margin-right",
                            "1.25in",
                            "--margin-left",
                            "1.25in",
                            "--enable-local-file-access",
                        ],
                        &[ARG],
                        &[source, output],
                    ),
                    "prince" | "pagedjs-cli" => arguments(&[], &[ARG], &[source, "-o", output]),
                    _ => return Err(Error::App(format!("Uncovered engine {engine}"))),
                }
            }
        };
        let pdf_on_stdout = matches!(engine, "groff" | "pdfroff");
        let output_dir = if root == LOCAL_ROOT { CWD } else { root };
        host.pdf_path = (!pdf_on_stdout).then(|| format!("{output_dir}/{output_name}"));
        host.passes.push_back(Pass {
            pdf: Some(PDF.to_vec()),
            stdout: if pdf_on_stdout {
                PDF.to_vec()
            } else {
                Vec::new()
            },
            ..Pass::default()
        });
        assert_eq!(resolve_pdf_plan(None, Some(engine))?.writer(), writer);
        let pdf = render(
            &mut host,
            engine,
            &mut Context::pure(),
            &Pandoc::default(),
            PdfOptions {
                engine_arguments: &[ARG.into()],
                ..PdfOptions::default()
            },
        )?;
        assert_eq!(pdf, PDF, "{engine}");
        assert_eq!(host.calls.len(), 1, "{engine}");
        let call = &host.calls[0];
        assert_eq!(call.program, engine);
        assert_eq!(call.argv, args, "{engine}");
        assert_eq!(call.cwd, cwd, "{engine}");
        assert_eq!(
            &*call.input,
            if stream { SOURCE.as_bytes() } else { b"" },
            "{engine}"
        );
        if tex_env {
            assert_eq!(
                call.environment,
                BTreeMap::from([
                    ("TEXINPUTS".into(), Some(format!("{TEX_ROOT}:"))),
                    ("TEXMFOUTPUT".into(), Some(TEX_ROOT.into())),
                ])
            );
        } else {
            assert!(call.environment.is_empty(), "{engine}");
        }
        if let Some(path) = expected_source {
            assert_eq!(
                host.writes.get(&path).map(Vec::as_slice),
                Some(SOURCE.as_bytes()),
                "{engine}"
            );
        }
        if root == LOCAL_ROOT {
            assert_eq!(
                host.writes.get(&format!("{CWD}/{output_name}")),
                Some(&Vec::new())
            );
            assert_eq!(host.removed.len(), 3, "{engine}");
        }
        assert!(host.files.is_empty(), "{engine}");
        assert!(host.removed.iter().any(|path| path == root), "{engine}");
    }
    Ok(())
}

#[test]
fn missing_program_reports_the_original_path_and_exit_47() {
    let engine = "/work/tool bin/xelatex.exe";
    let mut host = Mock {
        spawn_error: Some(io::ErrorKind::NotFound),
        ..Mock::default()
    };
    let result = render(
        &mut host,
        engine,
        &mut Context::pure(),
        &Pandoc::default(),
        PdfOptions::default(),
    );
    let expected = Error::PdfProgramNotFound(format!("'{engine}'"));
    assert_eq!(result, Err(expected.clone()));
    assert_eq!(expected.exit_code(), 47);
    assert_eq!(
        expected.to_string(),
        format!(
            "'{engine}' not found. Please select a different --pdf-engine or install '{engine}'"
        )
    );
    assert!(host.files.is_empty());
}

#[test]
fn engine_failure_reports_tex_excerpt_and_forwards_stderr() {
    let mut host = Mock {
        log_dir: Some(TEX_ROOT.into()),
        passes: VecDeque::from([Pass {
            status: 7,
            stdout: b"standard output".to_vec(),
            stderr: b"standard error\n".to_vec(),
            log: b"header\n! Error detail\n\ncontext\nl.5 invalid input\nfooter\n".to_vec(),
            ..Pass::default()
        }]),
        ..Mock::default()
    };
    let mut diagnostics = Vec::new();
    let expected = Error::Pdf("! Error detail\n\ncontext\nl.5 invalid input\n".into());
    let result = render(
        &mut host,
        "pdflatex",
        &mut Context::pure(),
        &Pandoc::default(),
        PdfOptions {
            stderr: Some(&mut diagnostics),
            ..PdfOptions::default()
        },
    );
    assert_eq!(result, Err(expected.clone()));
    assert_eq!(expected.exit_code(), 43);
    assert_eq!(diagnostics, b"standard error\n");
    assert!(host.files.is_empty());
}

#[test]
fn tex_failure_excerpt_includes_the_first_source_line() {
    for (log, excerpt) in [
        (
            "preamble\n! Error detail\nl. 5 invalid input\nfooter\n",
            "! Error detail\nl. 5 invalid input\n",
        ),
        (
            "preamble\n! Error detail\nl. source context\nfooter\n",
            "! Error detail\nl. source context\n",
        ),
        (
            "preamble\n! Error detail\nl.5 invalid input",
            "! Error detail\nl.5 invalid input\n",
        ),
    ] {
        let mut host = tex_host([Pass {
            status: 1,
            log: log.as_bytes().to_vec(),
            ..Pass::default()
        }]);
        assert_eq!(
            render_tex(&mut host, "pdflatex"),
            Err(Error::Pdf(excerpt.into()))
        );
    }
}

#[test]
fn missing_file_output_reports_exit_43() {
    for engine in ["pdflatex", "latexmk", "tectonic", "context"] {
        let result = render(
            &mut Mock::default(),
            engine,
            &mut Context::pure(),
            &Pandoc::default(),
            PdfOptions::default(),
        );
        assert_eq!(result, Err(Error::Pdf(Text::new())), "{engine}");
    }
}

#[test]
fn temporary_output_placeholders_and_streams_allow_empty_pdfs() -> Result<()> {
    for engine in [
        "weasyprint",
        "wkhtmltopdf",
        "prince",
        "pagedjs-cli",
        "typst",
        "groff",
        "pdfroff",
    ] {
        let result = render(
            &mut Mock::default(),
            engine,
            &mut Context::pure(),
            &Pandoc::default(),
            PdfOptions::default(),
        )?;
        assert!(result.is_empty(), "{engine}");
    }
    Ok(())
}

#[test]
fn sandboxed_pdf_host_is_refused_before_staging_and_execution() {
    let mut host = Mock {
        sandbox: true,
        ..Mock::default()
    };
    let result = render(
        &mut host,
        "pdflatex",
        &mut Context::pure(),
        &Pandoc::default(),
        PdfOptions::default(),
    );
    assert!(matches!(result, Err(Error::Pdf(message)) if message.contains("sandbox")));
    assert!(host.tmp_root.is_empty());
    assert!(host.calls.is_empty());
}

#[test]
fn latex_runs_once_for_log_text_without_a_rerun_warning() -> Result<()> {
    let mut host = tex_host([Pass {
        log: b"LaTeX Warning: There were undefined references.\nRerun alone\n".to_vec(),
        stdout: b"Warning: Rerun in standard output".to_vec(),
        pdf: Some(PDF.to_vec()),
        ..Pass::default()
    }]);
    assert_eq!(render_tex(&mut host, "pdflatex")?, PDF);
    assert_eq!(host.calls.len(), 1);
    Ok(())
}

#[test]
fn latex_reruns_once_when_the_warning_clears() -> Result<()> {
    let mut host = tex_host([
        Pass {
            log: b"LaTeX Warning: Rerun to get cross-references right.\n".to_vec(),
            ..Pass::default()
        },
        Pass {
            pdf: Some(PDF.to_vec()),
            ..Pass::default()
        },
    ]);
    assert_eq!(render_tex(&mut host, "xelatex")?, PDF);
    assert_eq!(host.calls.len(), 2);
    Ok(())
}

#[test]
fn latex_reruns_when_rerun_precedes_warning_on_the_same_line() -> Result<()> {
    let mut host = tex_host([
        Pass {
            log: b"Rerun LaTeX. Warning: Cross-references changed.\n".to_vec(),
            ..Pass::default()
        },
        Pass {
            pdf: Some(PDF.to_vec()),
            ..Pass::default()
        },
    ]);
    assert_eq!(render_tex(&mut host, "pdflatex")?, PDF);
    assert_eq!(host.calls.len(), 2);
    Ok(())
}

#[test]
fn latex_toc_can_require_another_pass() -> Result<()> {
    let mut host = tex_host([
        Pass {
            toc: true,
            ..Pass::default()
        },
        Pass {
            pdf: Some(PDF.to_vec()),
            ..Pass::default()
        },
    ]);
    assert_eq!(render_tex(&mut host, "lualatex")?, PDF);
    assert_eq!(host.calls.len(), 2);
    Ok(())
}

#[test]
fn latex_stops_on_the_fourth_pass_even_if_the_warning_persists() {
    let mut host = tex_host(
        std::iter::repeat_with(|| Pass {
            log: b"pdfTeX Warning: Label(s) may have changed. Rerun LaTeX.\n".to_vec(),
            status: 1,
            ..Pass::default()
        })
        .take(5),
    );
    assert!(matches!(
        render_tex(&mut host, "pdflatex-dev"),
        Err(Error::Pdf(_))
    ));
    assert_eq!(host.calls.len(), 4);
    assert_eq!(host.passes.len(), 1);
    assert!(host.files.is_empty());
}

#[test]
fn latexmk_leaves_reruns_to_the_engine() -> Result<()> {
    let mut host = tex_host([Pass {
        toc: true,
        log: b"Warning: Rerun".to_vec(),
        pdf: Some(PDF.to_vec()),
        ..Pass::default()
    }]);
    assert_eq!(render_tex(&mut host, "latexmk")?, PDF);
    assert_eq!(host.calls.len(), 1);
    Ok(())
}

fn tex_host(passes: impl IntoIterator<Item = Pass>) -> Mock {
    Mock {
        pdf_path: Some(format!("{TEX_ROOT}/input.pdf")),
        log_dir: Some(TEX_ROOT.into()),
        passes: passes.into_iter().collect(),
        ..Mock::default()
    }
}

fn render_tex(host: &mut Mock, engine: &str) -> Result<Vec<u8>> {
    render(
        host,
        engine,
        &mut Context::pure(),
        &Pandoc::default(),
        PdfOptions::default(),
    )
}

#[test]
fn latex_output_override_preserves_directory_and_engine_options() -> Result<()> {
    let directory = format!("{CWD}/my output");
    let output = format!("{directory}/input.pdf");
    let mut host = Mock {
        pdf_path: Some(output.clone()),
        log_dir: Some(directory),
        texinputs: Some("another path".into()),
        passes: VecDeque::from([Pass {
            pdf: Some(PDF.to_vec()),
            ..Pass::default()
        }]),
        ..Mock::default()
    };
    let override_arg = "-output-directory=my output";
    let pdf = render(
        &mut host,
        "pdflatex",
        &mut Context::pure(),
        &Pandoc::default(),
        PdfOptions {
            engine_arguments: &[override_arg.into(), ARG.into()],
            ..PdfOptions::default()
        },
    )?;
    let call = &host.calls[0];
    assert_eq!(&call.argv[4..7], &["my output", override_arg, ARG]);
    assert_eq!(
        call.environment["TEXMFOUTPUT"].as_deref(),
        Some("my output")
    );
    assert_eq!(
        call.environment["TEXINPUTS"].as_deref(),
        Some(format!("{TEX_ROOT}:another path").as_str())
    );
    assert_eq!(host.removed, [TEX_ROOT]);
    assert_eq!(host.files[&output], PDF);
    assert_eq!(pdf, PDF);
    Ok(())
}

#[test]
fn tectonic_replaces_only_the_leading_outdir_pair() -> Result<()> {
    for (args, expected) in [
        (
            vec!["--outdir", "out dir", ARG],
            vec!["--outdir", "out dir", ARG, "-"],
        ),
        (
            vec![ARG, "--outdir", "out dir"],
            vec!["--outdir", TEX_ROOT, ARG, "--outdir", "out dir", "-"],
        ),
        (
            vec!["--outdir=out dir"],
            vec!["--outdir", TEX_ROOT, "--outdir=out dir", "-"],
        ),
    ] {
        let mut host = Mock::default();
        let result = render(
            &mut host,
            "tectonic",
            &mut Context::pure(),
            &Pandoc::default(),
            PdfOptions {
                engine_arguments: &args.iter().map(|arg| (*arg).into()).collect::<Vec<_>>(),
                ..PdfOptions::default()
            },
        );
        assert_eq!(result, Err(Error::Pdf(Text::new())));
        assert_eq!(host.calls[0].argv, expected);
        assert_eq!(
            host.reads,
            [format!(
                "{}/texput.pdf",
                if expected[1] == TEX_ROOT {
                    TEX_ROOT.into()
                } else {
                    format!("{CWD}/{}", expected[1])
                }
            )]
        );
    }
    Ok(())
}

#[test]
fn wkhtmltopdf_page_flags_use_variables_and_document_metadata() -> Result<()> {
    let mut doc = Pandoc::default();
    doc.meta
        .insert("papersize", MetaValue::MetaString("letter".into()));
    doc.meta
        .insert("title", MetaValue::MetaString("A title with spaces".into()));
    doc.meta
        .insert("margin-top", MetaValue::MetaString("2in".into()));
    let mut host = Mock::default();
    render(
        &mut host,
        "wkhtmltopdf",
        &mut Context::pure(),
        &doc,
        PdfOptions {
            variables: BTreeMap::from([("papersize".into(), "A4".into())]),
            ..PdfOptions::default()
        },
    )?;
    assert_eq!(
        &host.calls[0].argv[..13],
        &[
            "--page-size",
            "A4",
            "--title",
            "A title with spaces",
            "--margin-bottom",
            "1.2in",
            "--margin-top",
            "2in",
            "--margin-right",
            "1.25in",
            "--margin-left",
            "1.25in",
            "--enable-local-file-access"
        ]
    );
    Ok(())
}

#[test]
fn wkhtmltopdf_uses_only_text_metadata_for_page_flags() -> Result<()> {
    let mut doc = Pandoc::default();
    doc.meta.insert("papersize", MetaValue::MetaBool(false));
    doc.meta.insert(
        "title",
        MetaValue::MetaList(vec![MetaValue::MetaString("A title".into())]),
    );
    doc.meta
        .insert("pagetitle", MetaValue::MetaString("Browser title".into()));
    doc.meta
        .insert("margin-top", MetaValue::MetaMap(BTreeMap::new()));
    doc.meta.insert(
        "margin-right",
        MetaValue::MetaInlines(vec![Inline::Str("2cm".into())]),
    );
    doc.meta.insert(
        "margin-left",
        MetaValue::MetaBlocks(vec![Block::Para(vec![Inline::Str("3cm".into())])]),
    );
    let mut host = Mock::default();
    render(
        &mut host,
        "wkhtmltopdf",
        &mut Context::pure(),
        &doc,
        PdfOptions::default(),
    )?;
    assert_eq!(
        &host.calls[0].argv[..9],
        &[
            "--margin-bottom",
            "1.2in",
            "--margin-top",
            "1.25in",
            "--margin-right",
            "2cm",
            "--margin-left",
            "3cm",
            "--enable-local-file-access",
        ]
    );
    Ok(())
}

#[test]
fn oversized_intermediate_document_does_not_start_the_engine() {
    let mut host = Mock::default();
    let mut context = Context::pure();
    context.limits.max_input_size = 1;
    let result = render(
        &mut host,
        "groff",
        &mut context,
        &Pandoc::default(),
        PdfOptions::default(),
    );
    assert!(matches!(result, Err(Error::Pdf(text)) if text.starts_with("Intermediate document")));
    assert!(host.calls.is_empty());
    assert_eq!(host.removed, [TEX_ROOT]);
}

#[test]
fn oversized_pdf_is_rejected_for_file_and_stream_output() {
    for engine in ["pdflatex", "groff"] {
        let mut host = tex_host([Pass {
            pdf: Some(vec![0; 128]),
            stdout: if engine == "groff" {
                vec![0; 128]
            } else {
                Vec::new()
            },
            ..Pass::default()
        }]);
        let mut context = Context::pure();
        context.limits.max_input_size = 64;
        let result = render(
            &mut host,
            engine,
            &mut context,
            &Pandoc::default(),
            PdfOptions::default(),
        );
        assert!(matches!(result, Err(Error::Pdf(text)) if text.contains("64 bytes")));
        assert!(host.files.is_empty());
    }
}

#[test]
fn oversized_latex_log_is_rejected() {
    let mut host = tex_host([Pass {
        log: vec![0; 64],
        pdf: Some(PDF.to_vec()),
        ..Pass::default()
    }]);
    let mut context = Context::pure();
    context.limits.max_resource_size = 16;
    let result = render(
        &mut host,
        "pdflatex",
        &mut context,
        &Pandoc::default(),
        PdfOptions::default(),
    );
    assert!(
        matches!(result, Err(Error::Pdf(text)) if text.contains("input.log") && text.contains("16 bytes"))
    );
    assert!(host.files.is_empty());
}

#[test]
fn stdout_and_stderr_have_independent_limits() {
    for (stdout, stderr, label) in [
        (vec![0; 17], vec![0; 16], "Engine standard output"),
        (vec![0; 16], vec![0; 17], "Engine standard error"),
    ] {
        let mut host = tex_host([Pass {
            stdout,
            stderr,
            pdf: Some(PDF.to_vec()),
            ..Pass::default()
        }]);
        let mut context = Context::pure();
        context.limits.max_resource_size = 16;
        let result = render(
            &mut host,
            "pdflatex",
            &mut context,
            &Pandoc::default(),
            PdfOptions::default(),
        );
        assert!(matches!(result, Err(Error::Pdf(text)) if text.starts_with(label)));
    }
}

fn image(url: &str) -> Inline {
    Inline::Image(
        Attr::default(),
        vec![Inline::Str("Description".into())],
        Target::new(url, "Title"),
    )
}

fn document_with_images(urls: &[&str]) -> Pandoc {
    Pandoc {
        blocks: vec![Block::Para(urls.iter().map(|url| image(url)).collect())],
        ..Pandoc::default()
    }
}

fn rendered_image_urls(doc: &Pandoc) -> Vec<Text> {
    let mut urls = Vec::new();
    randoc_ast::walk::query_inlines(doc, |inline| {
        if let Inline::Image(_, _, target) = inline {
            urls.push(target.url.clone());
        }
    });
    urls
}

fn render_image_paths(
    host: &mut Mock,
    context: &mut Context,
    engine: &str,
    urls: &[&str],
    options: PdfOptions<'_>,
) -> Result<Vec<Text>> {
    let mut rendered = Vec::new();
    make_pdf(
        &resolve_pdf_plan(None, Some(engine))?,
        &document_with_images(urls),
        &WriterOptions::default(),
        context,
        host,
        options,
        |staged, _, _| {
            rendered = rendered_image_urls(staged);
            Ok(Output::Text(SOURCE.into()))
        },
    )?;
    Ok(rendered)
}

#[test]
fn stages_local_and_embedded_images_with_a_sandboxed_context() -> Result<()> {
    let doc = document_with_images(&[
        "./sub/pixel.png",
        "../embedded.png",
        "data:image/png;base64,cG5n",
    ]);
    let mut context = Context::new(Box::new(Mock {
        sandbox: true,
        ..Mock::default()
    }));
    context.state.resource_path = vec!["missing".into(), "images".into()];
    context.media.insert(
        "../embedded.png",
        MediaItem {
            mime_type: "image/png".into(),
            contents: b"embedded".to_vec(),
        },
    );
    let mut host = tex_host([Pass {
        pdf: Some(PDF.to_vec()),
        ..Pass::default()
    }]);
    host.files
        .insert("images/sub/pixel.png".into(), b"fetched".to_vec());
    let mut urls = Vec::new();
    let pdf = make_pdf(
        &resolve_pdf_plan(None, Some("pdflatex"))?,
        &doc,
        &WriterOptions::default(),
        &mut context,
        &mut host,
        PdfOptions::default(),
        |staged, _, context| {
            assert_eq!(
                context
                    .media
                    .get("sub/pixel.png")
                    .map(|item| item.contents.as_slice()),
                Some(&b"fetched"[..])
            );
            urls = rendered_image_urls(staged);
            Ok(Output::Text(SOURCE.into()))
        },
    )?;
    assert_eq!(pdf, PDF);
    assert_eq!(urls.len(), 3);
    assert_eq!(urls[0], format!("{TEX_ROOT}/sub/pixel.png"));
    for (url, bytes) in urls
        .iter()
        .zip([&b"fetched"[..], &b"embedded"[..], &b"png"[..]])
    {
        assert!(
            !Path::new(url)
                .components()
                .any(|part| part == std::path::Component::ParentDir)
        );
        assert!(url.starts_with(&format!("{TEX_ROOT}/")));
        assert_eq!(host.writes[url], bytes);
    }
    assert_eq!(
        rendered_image_urls(&doc),
        [
            "./sub/pixel.png",
            "../embedded.png",
            "data:image/png;base64,cG5n"
        ]
    );
    assert_eq!(host.calls.len(), 1);
    assert_eq!(host.calls[0].program, "pdflatex");
    assert_eq!(
        host.files,
        BTreeMap::from([("images/sub/pixel.png".into(), b"fetched".to_vec())])
    );
    Ok(())
}

#[test]
fn sandboxed_context_fetches_remote_images_through_the_pdf_host() -> Result<()> {
    let url = "https://example.test/pixel.png";
    let mut context = Context::new(Box::new(Mock {
        sandbox: true,
        ..Mock::default()
    }));
    context.state.request_headers = vec![("X-Image-Test".into(), "value".into())];
    let mut host = tex_host([Pass {
        pdf: Some(PDF.to_vec()),
        ..Pass::default()
    }]);
    host.remote_media.insert(
        url.into(),
        MediaItem {
            mime_type: "image/png".into(),
            contents: b"remote".to_vec(),
        },
    );
    let urls = render_image_paths(
        &mut host,
        &mut context,
        "pdflatex",
        &[url],
        PdfOptions::default(),
    )?;
    assert_eq!(host.request_headers, context.state.request_headers);
    assert_eq!(urls, [format!("{TEX_ROOT}/{}.png", sha1_hex(b"remote"))]);
    assert_eq!(host.writes[&urls[0]], b"remote");
    Ok(())
}

#[test]
fn images_with_reserved_names_get_a_digest_prefix() -> Result<()> {
    let names = [
        "input.tex",
        "INPUT.TEX",
        "INPUT.TOC",
        "texput.pdf",
        "TEXPUT.PDF",
    ];
    let mut host = tex_host([Pass {
        pdf: Some(PDF.to_vec()),
        ..Pass::default()
    }]);
    for name in names {
        host.files
            .insert(format!("./{name}"), name.as_bytes().to_vec());
    }
    let urls = render_image_paths(
        &mut host,
        &mut Context::pure(),
        "pdflatex",
        &names,
        PdfOptions::default(),
    )?;
    assert_eq!(
        urls,
        names.map(|name| format!("{TEX_ROOT}/{}/{name}", sha1_hex(name.as_bytes())))
    );
    assert_eq!(host.calls.len(), 1);
    for (url, name) in urls.iter().zip(names) {
        assert_eq!(host.writes[url], name.as_bytes());
    }
    assert_eq!(
        host.writes[&format!("{TEX_ROOT}/input.tex")],
        SOURCE.as_bytes()
    );
    Ok(())
}

#[test]
fn rejects_distinct_images_with_the_same_staging_path() {
    let mut context = Context::pure();
    for (url, bytes) in [("pixel.png", b"first"), ("./pixel.png", b"other")] {
        context.media.insert(
            url,
            MediaItem {
                mime_type: "image/png".into(),
                contents: bytes.to_vec(),
            },
        );
    }
    let mut host = Mock::default();
    let result = render_image_paths(
        &mut host,
        &mut context,
        "pdflatex",
        &["pixel.png", "./pixel.png"],
        PdfOptions::default(),
    );
    assert_eq!(
        result,
        Err(Error::Pdf(
            "Image resources use the same staging path pixel.png.".into()
        ))
    );
    assert!(host.calls.is_empty());
    assert!(host.files.is_empty());
}

#[test]
fn latex_converts_svg_before_rendering() -> Result<()> {
    let mut host = tex_host([Pass {
        pdf: Some(PDF.to_vec()),
        ..Pass::default()
    }]);
    host.files
        .insert("./sub/an image.SVG".into(), b"<svg/>".to_vec());
    host.converter = Some(Pass {
        pdf: Some(PDF.to_vec()),
        stderr: b"Converter notice.\n".to_vec(),
        ..Pass::default()
    });
    let mut diagnostics = Vec::new();
    let mut urls = Vec::new();
    let mut writer_options = WriterOptions::default();
    writer_options.dpi = 144;
    make_pdf(
        &resolve_pdf_plan(None, Some("pdflatex"))?,
        &document_with_images(&["sub/an image.SVG", "./sub/an image.SVG"]),
        &writer_options,
        &mut Context::pure(),
        &mut host,
        PdfOptions {
            stderr: Some(&mut diagnostics),
            ..PdfOptions::default()
        },
        |staged, _, _| {
            urls = rendered_image_urls(staged);
            Ok(Output::Text(SOURCE.into()))
        },
    )?;
    let source = format!("{TEX_ROOT}/sub/an image.SVG");
    let output = format!("{TEX_ROOT}/{}.pdf", sha1_hex(source.as_bytes()));
    assert_eq!(urls, [output.clone(), output.clone()]);
    assert_eq!(host.calls.len(), 2);
    let converter = &host.calls[0];
    assert_eq!(converter.program, "rsvg-convert");
    assert_eq!(converter.cwd, CWD);
    assert!(converter.environment.is_empty());
    assert!(converter.input.is_empty());
    assert_eq!(
        converter.argv,
        [
            "-f", "pdf", "-a", "--dpi-x", "144", "--dpi-y", "144", "-o", &output, &source
        ],
    );
    assert_eq!(host.calls[1].program, "pdflatex");
    assert_eq!(diagnostics, b"Converter notice.\n");
    assert_eq!(host.files.len(), 1);
    assert!(host.files.contains_key("./sub/an image.SVG"));
    Ok(())
}

#[test]
fn latex_keeps_svg_if_the_converter_is_missing_or_fails() -> Result<()> {
    for (converter, message, stderr) in [
        (None, "check that rsvg-convert is in path.", ""),
        (
            Some(Pass {
                status: 7,
                stderr: b"Converter error.\n".to_vec(),
                ..Pass::default()
            }),
            "conversion from SVG failed",
            "Converter error.\n",
        ),
    ] {
        let mut host = tex_host([Pass {
            pdf: Some(PDF.to_vec()),
            ..Pass::default()
        }]);
        host.files.insert("./pixel.svg".into(), b"<svg/>".to_vec());
        host.converter = converter;
        let mut context = Context::pure();
        let mut diagnostics = Vec::new();
        let urls = render_image_paths(
            &mut host,
            &mut context,
            "pdflatex",
            &["pixel.svg"],
            PdfOptions {
                stderr: Some(&mut diagnostics),
                ..PdfOptions::default()
            },
        )?;
        assert_eq!(urls, [format!("{TEX_ROOT}/pixel.svg")]);
        assert_eq!(context.log().len(), 1);
        assert!(context.log()[0].text().contains(message));
        assert_eq!(diagnostics, stderr.as_bytes());
        assert_eq!(host.calls.len(), 2);
        assert_eq!(
            &host.calls[0].argv[3..7],
            &["--dpi-x", "96", "--dpi-y", "96"]
        );
    }
    Ok(())
}

#[test]
fn latex_with_an_explicit_engine_path_keeps_the_staged_svg() -> Result<()> {
    let mut host = tex_host([Pass {
        pdf: Some(PDF.to_vec()),
        ..Pass::default()
    }]);
    host.files.insert("./pixel.svg".into(), b"<svg/>".to_vec());
    assert_eq!(
        render_image_paths(
            &mut host,
            &mut Context::pure(),
            "/tools/pdflatex",
            &["pixel.svg"],
            PdfOptions::default(),
        )?,
        [format!("{TEX_ROOT}/pixel.svg")]
    );
    assert_eq!(host.calls.len(), 1);
    assert_eq!(host.calls[0].program, "/tools/pdflatex");
    Ok(())
}

#[test]
fn typst_uses_media_paths_relative_to_the_input_file() -> Result<()> {
    let doc = document_with_images(&["data:image/png;base64,eA=="]);
    let mut host = Mock::default();
    let mut urls = Vec::new();
    make_pdf(
        &resolve_pdf_plan(None, Some("typst"))?,
        &doc,
        &WriterOptions::default(),
        &mut Context::pure(),
        &mut host,
        PdfOptions::default(),
        |staged, _, _| {
            urls = rendered_image_urls(staged);
            Ok(Output::Text(SOURCE.into()))
        },
    )?;
    assert_eq!(urls.len(), 1);
    assert!(urls[0].starts_with("./media-test/"));
    assert!(urls[0].ends_with(".png"));
    assert_eq!(host.writes[&format!("{CWD}/{}", &urls[0][2..])], b"x");
    assert!(host.files.is_empty());
    Ok(())
}

#[test]
fn html_routes_embed_media_and_report_missing_images() -> Result<()> {
    let doc = document_with_images(&["pic.png", "missing.png"]);
    for engine in ["weasyprint", "prince", "pagedjs-cli", "wkhtmltopdf"] {
        let mut context = Context::pure();
        context.media.insert(
            "pic.png",
            MediaItem {
                mime_type: "image/png".into(),
                contents: b"png".to_vec(),
            },
        );
        make_pdf(
            &resolve_pdf_plan(None, Some(engine))?,
            &doc,
            &WriterOptions::default(),
            &mut context,
            &mut Mock::default(),
            PdfOptions::default(),
            |staged, _, _| {
                if engine == "wkhtmltopdf" {
                    assert_eq!(staged, &doc);
                } else {
                    assert_eq!(rendered_image_urls(staged), ["data:image/png;base64,cG5n"]);
                    let [Block::Para(inlines)] = staged.blocks.as_slice() else {
                        return Err(Error::App("The paragraph is missing.".into()));
                    };
                    assert!(matches!(&inlines[1], Inline::Span(..)));
                }
                Ok(Output::Text(SOURCE.into()))
            },
        )?;
        assert_eq!(context.log().len(), 1);
        assert!(context.log()[0].text().contains("missing.png"));
    }
    Ok(())
}

#[test]
fn a_preexisting_temporary_file_is_preserved() {
    let path = format!("{CWD}/toPdfViaTempFilemedia-test-1.pdf");
    let mut host = Mock {
        files: BTreeMap::from([(path.clone(), PDF.to_vec())]),
        ..Mock::default()
    };
    let result = render(
        &mut host,
        "typst",
        &mut Context::pure(),
        &Pandoc::default(),
        PdfOptions::default(),
    );
    assert!(matches!(result, Err(Error::Pdf(_))));
    assert!(host.calls.is_empty());
    assert_eq!(host.files, BTreeMap::from([(path, PDF.to_vec())]));
}

#[test]
fn a_file_created_during_temporary_file_creation_is_preserved() {
    let path = format!("{CWD}/toPdfViaTempFilemedia-test-1.pdf");
    let mut host = Mock {
        racing_file: Some(path.clone()),
        ..Mock::default()
    };
    let result = render(
        &mut host,
        "typst",
        &mut Context::pure(),
        &Pandoc::default(),
        PdfOptions::default(),
    );
    assert!(matches!(result, Err(Error::Pdf(_))));
    assert!(host.calls.is_empty());
    assert_eq!(
        host.files,
        BTreeMap::from([(path, b"Concurrent file".to_vec())])
    );
}

#[test]
fn html_embedding_matches_the_engine_route() -> Result<()> {
    const HTML: &str = "<link href=\"styles/main.css\">\n<img src=\"pixel.png\">\n";
    const EMBEDDED: &str = concat!(
        "<style type=\"text/css\">a{background:url(data:image/png;base64,eA==)}</style>\n",
        "<img src=\"data:image/png;base64,eA==\">\n",
    );

    for engine in ["weasyprint", "pagedjs-cli", "prince", "wkhtmltopdf"] {
        for (image, extra_media, sandbox) in [
            (None, false, false),
            (Some("missing.gif"), false, false),
            (Some("data:image/gif;base64,R0lGODlh"), false, false),
            (None, true, false),
            (Some("trigger.gif"), false, true),
        ] {
            let files = BTreeMap::from([
                (
                    "styles/main.css".into(),
                    b"a{background:url(../pixel.png)}".to_vec(),
                ),
                ("styles/../pixel.png".into(), b"x".to_vec()),
                ("pixel.png".into(), b"x".to_vec()),
                ("./trigger.gif".into(), b"GIF89a".to_vec()),
            ]);
            let mut context = if sandbox {
                Context::new(Box::new(Mock {
                    sandbox: true,
                    ..Mock::default()
                }))
            } else {
                Context::new(Box::new(randoc_core::PureHost {
                    files: files.clone(),
                    ..randoc_core::PureHost::default()
                }))
            };
            if extra_media {
                context.media.insert(
                    "unused.gif",
                    MediaItem {
                        contents: b"GIF89a".to_vec(),
                        mime_type: "image/gif".into(),
                    },
                );
            }
            let doc = Pandoc {
                blocks: image
                    .map(|url| {
                        Block::Para(vec![Inline::Image(
                            Attr::default(),
                            vec![],
                            Target {
                                url: url.into(),
                                title: Text::new(),
                            },
                        )])
                    })
                    .into_iter()
                    .collect(),
                ..Pandoc::default()
            };
            let mut host = Mock {
                files,
                pdf_path: Some(format!("{CWD}/toPdfViaTempFilemedia-test-1.pdf")),
                passes: VecDeque::from([Pass {
                    pdf: Some(PDF.to_vec()),
                    ..Pass::default()
                }]),
                ..Mock::default()
            };
            assert_eq!(
                make_pdf(
                    &resolve_pdf_plan(None, Some(engine))?,
                    &doc,
                    &WriterOptions::default(),
                    &mut context,
                    &mut host,
                    PdfOptions::default(),
                    |_, _, _| Ok(Output::Text(HTML.into())),
                )?,
                PDF
            );
            let call = &host.calls[0];
            assert_eq!(call.program, engine);
            let input_path = call
                .argv
                .iter()
                .find(|arg| arg.ends_with(".html"))
                .ok_or_else(|| {
                    Error::App("The engine did not receive an HTML input path.".into())
                })?;
            let input_path = format!("{}/{}", call.cwd, input_path.trim_start_matches("./"));
            let expected = if engine != "wkhtmltopdf"
                && (extra_media || image.is_some_and(|url| url != "missing.gif"))
            {
                EMBEDDED
            } else {
                HTML
            };
            assert_eq!(
                host.writes[&input_path],
                expected.as_bytes(),
                "{engine}, {image:?}, {extra_media}, {sandbox}"
            );
            assert_eq!(context.host().is_sandboxed(), sandbox);
        }
    }
    Ok(())
}
