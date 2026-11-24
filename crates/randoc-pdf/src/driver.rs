use std::io;

use randoc_ast::{MetaValue, Pandoc};
use randoc_core::{
    Context, Error, Host, Limits, Output, ProcessOutput, ProcessRequest, Result, Text,
};

use crate::plan::Engine;
use crate::staging::{Staging, join, read_optional};
use crate::{PdfOptions, PdfPlan, check_size};

pub(crate) fn intermediate_bytes(
    engine: Engine,
    output: Output,
    context: &mut Context,
    host: &mut dyn crate::PdfHost,
) -> Result<Vec<u8>> {
    match output {
        Output::Text(html)
            if !context.media.is_empty()
                && matches!(
                    engine,
                    Engine::WeasyPrint | Engine::PagedJs | Engine::Prince
                ) =>
        {
            check_size(
                html.len(),
                context.limits.max_input_size,
                "Intermediate document",
            )?;
            let html = if context.host().is_sandboxed() {
                randoc_embed::embed_resources_with_host(&html, context, host)?
            } else {
                randoc_embed::embed_resources(&html, context)?
            };
            Ok(html.into_bytes())
        }
        output => Ok(output.into_bytes()),
    }
}

pub(crate) fn run(
    plan: &PdfPlan,
    doc: &Pandoc,
    input: &[u8],
    staging: &mut Staging<'_>,
    options: &mut PdfOptions<'_>,
    limits: Limits,
) -> Result<Vec<u8>> {
    let mut invocation = Invocation {
        arguments: Vec::new(),
        cwd: staging.cwd.clone(),
        environment: Vec::new(),
        stdin: &[],
        output: None,
        tex_log: None,
        toc: None,
    };
    let args = options.engine_arguments;

    match plan.engine {
        Engine::LatexMk
        | Engine::PdfLatex
        | Engine::LuaLatex
        | Engine::XeLatex
        | Engine::PdfLatexDev
        | Engine::LuaLatexDev => {
            invocation.latex(plan.engine, staging, input, args)?;
        }
        Engine::Tectonic => {
            let (directory, remaining) = tectonic_options(args, &staging.root);
            invocation.arguments = vec!["--outdir".into(), directory.into()];
            invocation.arguments.extend(remaining);
            invocation.arguments.push("-".into());
            invocation.stdin = input;
            invocation.output = Some(join(&join(&staging.cwd, directory), "texput.pdf"));
        }
        Engine::Context => {
            staging.write("input.tex", input)?;
            invocation.cwd.clone_from(&staging.root);
            invocation.arguments.push("--batchmode".into());
            invocation.arguments.extend_from_slice(args);
            invocation.arguments.push("input.tex".into());
            invocation.output = Some(join(&staging.root, "input.pdf"));
        }
        Engine::Groff | Engine::PdfRoff => {
            let flags: &[&str] = if plan.engine == Engine::Groff {
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
            invocation
                .arguments
                .extend(flags.iter().map(|&flag| flag.into()));
            if let Some(paper) = options.variables.get("papersize") {
                invocation.arguments.push(format!("-P-p{paper}"));
            }
            invocation.arguments.extend_from_slice(args);
            invocation.stdin = input;
        }
        _ => invocation.temp_files(plan.engine, staging, input, doc, options)?,
    }

    invocation.execute(plan.program(), staging.host, options, limits)
}

struct Invocation<'a> {
    arguments: Vec<Text>,
    cwd: Text,
    environment: Vec<(Text, Option<Text>)>,
    stdin: &'a [u8],
    output: Option<Text>,
    tex_log: Option<Text>,
    toc: Option<Text>,
}

impl Invocation<'_> {
    fn latex(
        &mut self,
        engine: Engine,
        staging: &mut Staging<'_>,
        input: &[u8],
        args: &[Text],
    ) -> Result<()> {
        staging.write("input.tex", input)?;
        let directory = args
            .iter()
            .find_map(|arg| {
                arg.strip_prefix("-output-directory=")
                    .or_else(|| arg.strip_prefix("-outdir="))
            })
            .unwrap_or(&staging.root);
        self.arguments = if engine == Engine::LatexMk {
            vec![
                "-interaction=batchmode".into(),
                "-halt-on-error".into(),
                "-pdf".into(),
                "-quiet".into(),
                format!("-outdir={directory}"),
            ]
        } else {
            vec![
                "-halt-on-error".into(),
                "-interaction".into(),
                "nonstopmode".into(),
                "-output-directory".into(),
                directory.into(),
            ]
        };
        self.arguments.extend_from_slice(args);
        self.arguments.push(join(&staging.root, "input.tex"));
        let old_inputs = staging
            .host
            .environment_variable("TEXINPUTS")
            .unwrap_or_default();
        let separator = if cfg!(windows) { ';' } else { ':' };
        self.environment = vec![
            (
                "TEXINPUTS".into(),
                Some(format!("{}{separator}{old_inputs}", staging.root)),
            ),
            ("TEXMFOUTPUT".into(), Some(directory.into())),
        ];
        let absolute_dir = join(&staging.cwd, directory);
        self.output = Some(join(&absolute_dir, "input.pdf"));
        self.tex_log = Some(join(&absolute_dir, "input.log"));
        if engine.reruns() {
            self.toc = Some(join(&absolute_dir, "input.toc"));
        }
        Ok(())
    }

    fn temp_files(
        &mut self,
        engine: Engine,
        staging: &mut Staging<'_>,
        input: &[u8],
        doc: &Pandoc,
        options: &PdfOptions<'_>,
    ) -> Result<()> {
        let extension = engine
            .temp_file_extension()
            .ok_or_else(|| Error::Pdf("The engine has no intermediate file extension.".into()))?;
        let source = staging.caller_file(&format!("0.{extension}"), input)?;
        let output = staging.caller_file("1.pdf", &[])?;
        match engine {
            Engine::WeasyPrint => self.arguments.push("--presentational-hints".into()),
            Engine::Wkhtmltopdf => self.arguments = wkhtmltopdf_options(doc, options),
            Engine::Typst => self.arguments.push("compile".into()),
            _ => {}
        }
        self.arguments.extend_from_slice(options.engine_arguments);
        self.arguments.push(source);
        if matches!(engine, Engine::Prince | Engine::PagedJs) {
            self.arguments.push("-o".into());
        }
        self.output = Some(join(&staging.cwd, output.trim_start_matches("./")));
        self.arguments.push(output);
        Ok(())
    }

    fn execute(
        self,
        program: &str,
        host: &mut dyn Host,
        options: &mut PdfOptions<'_>,
        limits: Limits,
    ) -> Result<Vec<u8>> {
        let pdf_on_stdout = self.output.is_none();
        let request = ProcessRequest {
            program,
            arguments: &self.arguments,
            input: self.stdin,
            working_directory: Some(&self.cwd),
            environment: &self.environment,
            max_stdout_bytes: Some(if pdf_on_stdout {
                limits.max_input_size
            } else {
                limits.max_resource_size
            }),
            max_stderr_bytes: Some(limits.max_resource_size),
        };
        let passes = if self.toc.is_some() { 4 } else { 1 };
        for pass in 1..=passes {
            let output = run_process(host, &request, options, limits, pdf_on_stdout)?;
            let log = match &self.tex_log {
                Some(path) => read_optional(host, path, limits.max_resource_size)?,
                None => None,
            };
            if pass < passes
                && (log.as_deref().is_some_and(needs_rerun)
                    || self.toc.as_ref().is_some_and(|path| host.file_exists(path)))
            {
                continue;
            }
            if output.exit_code != 0 {
                return Err(Error::Pdf(diagnostic(
                    log.as_deref(),
                    &output,
                    options.stderr.is_some(),
                )));
            }

            return match &self.output {
                Some(path) => read_optional(host, path, limits.max_input_size)?
                    .ok_or_else(|| Error::Pdf(Text::new())),
                None => Ok(output.stdout),
            };
        }
        Err(Error::Pdf("The PDF engine did not run.".into()))
    }
}

pub(crate) fn run_process(
    host: &mut dyn Host,
    request: &ProcessRequest<'_>,
    options: &mut PdfOptions<'_>,
    limits: Limits,
    pdf_on_stdout: bool,
) -> Result<ProcessOutput> {
    let program = request.program;
    let output = host.run_process_with(request).map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
            Error::PdfProgramNotFound(format!("'{program}'"))
        } else {
            Error::Pdf(format!("{program}: {err}"))
        }
    })?;
    check_process_output(&output, options, limits, pdf_on_stdout)?;
    Ok(output)
}

pub(crate) fn check_process_output(
    output: &ProcessOutput,
    options: &mut PdfOptions<'_>,
    limits: Limits,
    pdf_on_stdout: bool,
) -> Result<()> {
    if pdf_on_stdout && output.exit_code == 0 {
        check_size(output.stdout.len(), limits.max_input_size, "PDF output")?;
    } else {
        check_size(
            output.stdout.len(),
            limits.max_resource_size,
            "Engine standard output",
        )?;
    }
    check_size(
        output.stderr.len(),
        limits.max_resource_size,
        "Engine standard error",
    )?;
    if let Some(stderr) = &mut options.stderr {
        let _ = stderr.write_all(&output.stderr);
    }
    Ok(())
}

fn needs_rerun(log: &[u8]) -> bool {
    log.split(|&byte| byte == b'\n').any(|line| {
        line.windows(b"Warning:".len())
            .any(|part| part == b"Warning:")
            && line.windows(b"Rerun".len()).any(|part| part == b"Rerun")
    })
}

fn diagnostic(log: Option<&[u8]>, output: &ProcessOutput, stderr_forwarded: bool) -> Text {
    let Some(log) = log else {
        let bytes = if output.stdout.is_empty() && !stderr_forwarded {
            &output.stderr
        } else {
            &output.stdout
        };
        return String::from_utf8_lossy(bytes).into_owned();
    };
    let text = String::from_utf8_lossy(log);
    let mut lines = text
        .split_inclusive('\n')
        .skip_while(|line| !line.starts_with('!'));
    let Some(first) = lines.next() else {
        return text.into_owned();
    };
    let mut excerpt = first.to_owned();
    for line in lines {
        excerpt.push_str(line);
        if line.starts_with("l.") {
            break;
        }
    }
    if !excerpt.ends_with('\n') {
        excerpt.push('\n');
    }
    excerpt
}

fn tectonic_options<'a>(args: &'a [Text], root: &'a str) -> (&'a str, Vec<Text>) {
    if let [flag, directory, rest @ ..] = args
        && flag == "--outdir"
    {
        return (directory, rest.to_vec());
    }
    (root, args.to_vec())
}

fn wkhtmltopdf_options(doc: &Pandoc, options: &PdfOptions<'_>) -> Vec<Text> {
    let mut args = Vec::new();
    for (key, flag, fallback) in [
        ("papersize", "--page-size", None),
        ("title", "--title", None),
        ("margin-bottom", "--margin-bottom", Some("1.2in")),
        ("margin-top", "--margin-top", Some("1.25in")),
        ("margin-right", "--margin-right", Some("1.25in")),
        ("margin-left", "--margin-left", Some("1.25in")),
    ] {
        let value = options
            .variables
            .get(key)
            .cloned()
            .or_else(|| {
                doc.meta.get(key).and_then(|value| match value {
                    MetaValue::MetaString(_)
                    | MetaValue::MetaInlines(_)
                    | MetaValue::MetaBlocks(_) => Some(randoc_core::shared::stringify_meta(value)),
                    _ => None,
                })
            })
            .or_else(|| fallback.map(Text::from));
        if let Some(value) = value {
            args.extend([flag.into(), value]);
        }
    }
    args.push("--enable-local-file-access".into());
    args
}
