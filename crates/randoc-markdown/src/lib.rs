//! The Pandoc Markdown reader.
//!
//! One parser reads `markdown`, `markdown_strict`, `markdown_phpextra`,
//! `markdown_mmd` and `markdown_github`. The formats differ only in their
//! default extensions, which the program puts in `ReaderOptions::extensions`.
//!
//! The reader works in two phases. The block phase splits the lines into
//! blocks and keeps the raw text of each inline run in a placeholder. It also
//! collects the link references and the note definitions. The inline phase
//! then parses each placeholder, so that a link can use a reference that the
//! document defines later.

mod block;
#[cfg(test)]
mod conformance_tests;
#[cfg(test)]
mod depth_tests;
mod inline;
#[cfg(test)]
mod perf_tests;
mod tables;
mod yaml;

use std::sync::Arc;

use randoc_ast::Pandoc;
use randoc_core::{Context, LogMessage, Reader, ReaderOptions, Result, Sources};

#[cfg(test)]
thread_local! {
    /// The work counter of the time tests.
    pub(crate) static STEPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Counts `n` units of work. Only test builds count.
#[inline]
pub(crate) fn step(n: usize) {
    #[cfg(test)]
    STEPS.with(|s| s.set(s.get().saturating_add(n as u64)));
    #[cfg(not(test))]
    let _ = n;
}

/// The format names that this crate reads.
pub const FORMATS: &[&str] = &[
    "markdown",
    "markdown_strict",
    "markdown_phpextra",
    "markdown_mmd",
    "markdown_github",
];

/// Returns the reader for a format name of this crate, for example `markdown`.
pub fn reader(format: &str) -> Option<Reader> {
    if !FORMATS.contains(&format) {
        return None;
    }
    let deprecated = format == "markdown_github";
    Some(Reader::Text(Arc::new(
        move |sources: &Sources, options: &ReaderOptions, context: &mut Context| {
            if deprecated {
                context.report(LogMessage::Deprecated(
                    "markdown_github".into(),
                    "Use gfm instead.".into(),
                ));
            }
            read_sources(sources, options, context)
        },
    )))
}

/// Returns the writer for a format name of this crate. This crate has no writer yet.
pub fn writer(_format: &str) -> Option<randoc_core::Writer> {
    None
}

fn read_sources(
    sources: &Sources,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let name = sources
        .0
        .first()
        .map(|source| source.name.clone())
        .unwrap_or_default();
    read_markdown(&sources.concat(), &name, options, context)
}

/// Reads Markdown text into a document.
///
/// `source_name` is the name for messages. An empty name means standard input.
pub fn read_markdown(
    text: &str,
    source_name: &str,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let max_depth = context.limits.max_nesting_depth;
    let mut parser = block::Parser::new(options, source_name, max_depth);
    let result = parser.parse_document(text);
    for message in parser.take_log() {
        context.report(message);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use randoc_core::{Extensions, default_extensions};

    #[allow(dead_code)]
    pub(crate) fn read_with(text: &str, format: &str) -> Pandoc {
        let mut options = ReaderOptions::default();
        options.extensions = default_extensions(format);
        let mut context = Context::pure();
        read_markdown(text, "", &options, &mut context).unwrap_or_default()
    }

    #[test]
    fn reader_knows_each_format() {
        for format in FORMATS {
            assert!(reader(format).is_some());
        }
        assert!(reader("commonmark").is_none());
        assert!(writer("markdown").is_none());
    }

    #[test]
    fn reads_through_the_reader_interface() {
        let Some(Reader::Text(reader)) = reader("markdown") else {
            return;
        };
        let mut options = ReaderOptions::default();
        options.extensions = Extensions::empty();
        let mut context = Context::pure();
        let doc = reader
            .read(&Sources::from_text("*a*"), &options, &mut context)
            .unwrap_or_default();
        assert_eq!(
            randoc_ast::json::to_json(&doc),
            r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Emph","c":[{"t":"Str","c":"a"}]}]}]}"#
        );
    }
}
