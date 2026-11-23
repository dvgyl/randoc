//! Lightweight text and outline formats for Randoc.

#![forbid(unsafe_code)]

use std::sync::Arc;

use randoc_ast::Pandoc;
use randoc_core::{
    Context, Error, Reader, ReaderOptions, Result, Sources, TemplateWriter, Writer, WriterOptions,
};

mod inline;
mod opml;
mod read;
mod write;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Textile,
    Muse,
    T2t,
    Haddock,
    Opml,
    Vimdoc,
}

impl Format {
    fn get(name: &str) -> Option<Self> {
        Some(match name {
            "textile" => Self::Textile,
            "muse" => Self::Muse,
            "t2t" => Self::T2t,
            "haddock" => Self::Haddock,
            "opml" => Self::Opml,
            "vimdoc" => Self::Vimdoc,
            _ => return None,
        })
    }
}

/// Returns the reader for a supported input format.
pub fn reader(name: &str) -> Option<Reader> {
    let format = Format::get(name)?;
    if format == Format::Vimdoc {
        return None;
    }
    Some(Reader::Text(Arc::new(
        move |sources: &Sources, options: &ReaderOptions, context: &mut Context| {
            let size = sources
                .0
                .iter()
                .map(|source| source.text.len() as u64)
                .fold(0, u64::saturating_add);
            if size > context.limits.max_input_size {
                return Err(Error::App("The input is too large.".into()));
            }
            let input = sources.concat();
            if format == Format::Opml {
                opml::read(&input, options, context)
            } else {
                let paths = (format == Format::T2t).then(|| {
                    (
                        context
                            .state
                            .input_files
                            .first()
                            .map_or("-", String::as_str),
                        context.state.output_file.as_deref().unwrap_or(""),
                    )
                });
                read::read(format, &input, options, &context.limits, paths)
            }
        },
    )))
}

/// Returns the writer for a supported output format.
pub fn writer(name: &str) -> Option<Writer> {
    let format = Format::get(name)?;
    Some(Writer::Text(Arc::new(TemplateWriter(
        move |doc: &Pandoc, options: &WriterOptions, context: &mut Context| {
            if format == Format::Opml {
                opml::write(doc, options, context)
            } else {
                write::write(format, doc, options, context)
            }
        },
    ))))
}

fn check_depth(depth: usize, limit: usize) -> Result<()> {
    if depth > limit {
        return Err(Error::App("The document is nested too deeply.".into()));
    }
    Ok(())
}

fn expansion_budget(size: usize, limit: u64) -> usize {
    size.max(1024)
        .saturating_mul(64)
        .min(usize::try_from(limit).unwrap_or(usize::MAX))
}

#[cfg(test)]
mod tests;
