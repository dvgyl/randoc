//! PDF engine support for Randoc.

#![forbid(unsafe_code)]

mod driver;
mod media;
mod plan;
mod staging;

use std::collections::BTreeMap;
use std::io::{self, Write};

use randoc_ast::Pandoc;
use randoc_core::{Context, Error, Output, Reader, Result, Text, Writer, WriterOptions};

pub use plan::{PDF_ENGINES, PdfPlan, resolve_pdf_plan, validate_pdf_engine};
pub use staging::PdfHost;

use staging::Staging;

/// Arguments and diagnostics for one PDF conversion.
#[derive(Default)]
pub struct PdfOptions<'a> {
    /// Engine arguments in command-line order. Each entry is one argument.
    pub engine_arguments: &'a [Text],
    /// Plain text template variables for engine flags, such as `papersize`.
    pub variables: BTreeMap<Text, Text>,
    /// The destination for child standard error, including on success.
    pub stderr: Option<&'a mut dyn Write>,
}

/// Stages media, renders the intermediate document once, and produces PDF bytes.
///
/// Resolve the plan before selecting the writer and template. Pass the
/// standalone writer options and a callback such as
/// `|doc, options, ctx| writer.write(doc, options, ctx)`. The callback receives
/// a document with the staged image references. The PDF host loads resources
/// outside the media bag, stages files, and runs child processes.
/// The driver checks stream sizes when the host returns the captured output.
pub fn make_pdf(
    plan: &PdfPlan,
    doc: &Pandoc,
    writer_options: &WriterOptions,
    context: &mut Context,
    host: &mut dyn PdfHost,
    mut options: PdfOptions<'_>,
    render: impl FnOnce(&Pandoc, &WriterOptions, &mut Context) -> Result<Output>,
) -> Result<Vec<u8>> {
    if host.is_sandboxed() {
        return Err(Error::Pdf(
            "The sandbox does not permit PDF engine execution.".into(),
        ));
    }

    let mut staging = Staging::new(host, plan.engine)?;
    let limits = context.limits;
    let prepared = media::prepare(
        doc,
        plan,
        context,
        &mut staging,
        writer_options.dpi,
        &mut options,
    )?;
    let input = render(&prepared, writer_options, context)?.into_bytes();
    check_size(input.len(), limits.max_input_size, "Intermediate document")?;
    driver::run(plan, doc, &input, &mut staging, &mut options, limits)
}

fn check_size(size: usize, limit: u64, label: &str) -> Result<()> {
    if size as u64 > limit {
        return Err(Error::Pdf(format!("{label} exceeds {limit} bytes.")));
    }
    Ok(())
}

fn file_error(path: &str, error: io::Error) -> Error {
    Error::Pdf(format!("{path}: {error}"))
}

/// Returns a reader for the requested format, if available.
pub fn reader(_format: &str) -> Option<Reader> {
    None
}

/// Returns a writer for the requested format, if available.
pub fn writer(_format: &str) -> Option<Writer> {
    None
}
