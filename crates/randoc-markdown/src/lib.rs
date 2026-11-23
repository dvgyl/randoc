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
mod cache_tests;
#[cfg(test)]
mod conformance_tests;
#[cfg(test)]
mod depth_tests;
mod inline;
#[cfg(test)]
mod perf_tests;
mod tables;
mod tokens;
mod yaml;

use std::sync::Arc;

use randoc_ast::Pandoc;
use randoc_core::{Context, LogMessage, Reader, ReaderOptions, Result, Sources};

#[cfg(test)]
thread_local! {
    /// The work counter of the time tests.
    pub(crate) static STEPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
thread_local! {
    /// The number of cache answers that differ from the plain scan, or
    /// `None` when the cache test does not run: the plain scans take
    /// quadratic time.
    pub(crate) static CACHE_MISMATCHES: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Runs `plain` when the cache test runs, without counting its steps, and
/// counts a mismatch when it differs from `cached`.
#[cfg(test)]
pub(crate) fn check_cache<T: PartialEq>(cached: &T, plain: impl FnOnce() -> T) {
    let Some(count) = CACHE_MISMATCHES.with(std::cell::Cell::get) else {
        return;
    };
    let steps = STEPS.with(std::cell::Cell::get);
    let differs = plain() != *cached;
    STEPS.with(|s| s.set(steps));
    CACHE_MISMATCHES.with(|m| m.set(Some(count + u64::from(differs))));
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
    let mut parser = block::Parser::new(options, &name, context.limits.max_nesting_depth);
    if options
        .extensions
        .contains(randoc_core::Extension::RebaseRelativePaths)
    {
        parser.set_sources(sources);
    }
    parse(&sources.concat(), parser, context)
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
    let parser = block::Parser::new(options, source_name, context.limits.max_nesting_depth);
    parse(text, parser, context)
}

fn parse(text: &str, mut parser: block::Parser, context: &mut Context) -> Result<Pandoc> {
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
    fn a_url_citation_key_keeps_its_internal_slashes() {
        use randoc_ast::{Block, Inline};
        let doc = read_with(
            "[@url:http://example.org/article]. @url:https://example.org/path.",
            "markdown",
        );
        assert!(matches!(
            doc.blocks.as_slice(),
            [Block::Para(inlines)] if matches!(
                inlines.as_slice(),
                [Inline::Cite(first, _), Inline::Str(period), Inline::Space,
                 Inline::Cite(second, _), Inline::Str(last_period)]
                    if first[0].id == "url:http://example.org/article"
                        && second[0].id == "url:https://example.org/path"
                        && period == "." && last_period == "."
            )
        ));
    }

    #[test]
    fn author_in_text_citations_share_a_note_number() {
        use randoc_ast::{Block, CitationMode, Inline};

        for (source, suffix, prefix) in [
            ("@a [p. 33; @b]", "p.\u{a0}33", vec![]),
            (
                "@a [p.12; see also @b]",
                "p.12",
                vec![
                    Inline::Str("see".into()),
                    Inline::Space,
                    Inline::Str("also".into()),
                ],
            ),
        ] {
            let doc = read_with(source, "markdown");
            let [Block::Para(inlines)] = doc.blocks.as_slice() else {
                panic!("Expected one paragraph");
            };
            let [Inline::Cite(citations, _)] = inlines.as_slice() else {
                panic!("Expected one citation group");
            };
            let [first, second] = citations.as_slice() else {
                panic!("Expected two citations");
            };
            assert_eq!(first.id, "a");
            assert_eq!(first.mode, CitationMode::AuthorInText);
            assert_eq!(first.suffix, [Inline::Str(suffix.into())]);
            assert_eq!(second.id, "b");
            assert_eq!(second.prefix, prefix);
            assert_eq!(second.mode, CitationMode::NormalCitation);
            assert_eq!((first.note_num, second.note_num), (1, 1));
        }
    }

    #[test]
    fn an_abbreviation_keeps_a_space_before_a_citation() {
        use randoc_ast::{Block, Inline};
        let doc = read_with("al. [@k] and al. example", "markdown");
        assert!(matches!(
            doc.blocks.as_slice(),
            [Block::Para(inlines)] if matches!(
                inlines.as_slice(),
                [Inline::Str(abbr), Inline::Space, Inline::Cite(..), Inline::Space,
                 Inline::Str(join), Inline::Space, Inline::Str(example)]
                    if abbr == "al." && join == "and" && example == "al.\u{a0}example"
            )
        ));
    }

    #[test]
    fn a_nested_list_can_end_a_tight_ordered_list_item() {
        use randoc_ast::Block;
        let doc = read_with("1. First\n    * Nested\n\n1. Second\n", "markdown");
        assert!(matches!(
            doc.blocks.as_slice(),
            [Block::OrderedList(_, items)]
                if matches!(items.as_slice(), [first, second]
                    if matches!(first.as_slice(), [Block::Plain(_), Block::BulletList(_)])
                        && matches!(second.as_slice(), [Block::Plain(_)]))
        ));
    }

    /// `pandoc -t native -s`: the key `*` of `nocite: '@*'` and of the
    /// body, in each citation form.
    #[test]
    fn a_star_is_a_citation_key() {
        use randoc_ast::{CitationMode, Inline, MetaValue};
        let doc = read_with(
            "---\nnocite: '@*'\n---\n\nA @* and [@*] and [-@*] and @*x and @**\n",
            "markdown",
        );
        let keys = |inlines: &[Inline]| -> Vec<(String, CitationMode)> {
            inlines
                .iter()
                .filter_map(|inline| match inline {
                    Inline::Cite(citations, _) => {
                        Some((citations[0].id.clone(), citations[0].mode))
                    }
                    _ => None,
                })
                .collect()
        };
        let Some(MetaValue::MetaInlines(nocite)) = doc.meta.get("nocite") else {
            panic!("no nocite inlines: {:?}", doc.meta);
        };
        assert_eq!(keys(nocite), [("*".into(), CitationMode::AuthorInText)]);
        let Some(randoc_ast::Block::Para(inlines)) = doc.blocks.first() else {
            panic!("no paragraph");
        };
        assert_eq!(
            keys(inlines),
            [
                ("*".into(), CitationMode::AuthorInText),
                ("*".into(), CitationMode::NormalCitation),
                ("*".into(), CitationMode::SuppressAuthor),
                ("*x".into(), CitationMode::AuthorInText),
                ("*".into(), CitationMode::AuthorInText),
            ]
        );
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
