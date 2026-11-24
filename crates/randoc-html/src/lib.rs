//! Writers for HTML documents and HTML slide decks.
//!
//! The writer gives the same bytes as `pandoc -t html` in Pandoc 3.13 for
//! fragment output (no template). The program adds the final newline.
//!
//! The oracle matrices `src/*.jsonl` (from the scripts in `tools/`) are not
//! part of the package. A unit test reads them from the source tree.
//!
//! ```
//! use randoc_ast::{Block, Inline, Pandoc};
//! use randoc_core::{Context, Output, WriterOptions};
//!
//! let writer = randoc_html::writer("html").unwrap();
//! let doc = Pandoc {
//!     meta: Default::default(),
//!     blocks: vec![Block::Para(vec![Inline::Str("Hi".into())])],
//! };
//! let output = writer.write(&doc, &WriterOptions::default(), &mut Context::pure()).unwrap();
//! assert_eq!(output, Output::Text("<p>Hi</p>".into()));
//! ```

mod attributes;
#[cfg(test)]
mod digests;
mod highlight;
mod html;
#[cfg(test)]
mod parity_tests;
pub mod shared;
mod slides;
mod standalone;
mod table;

use std::sync::Arc;

use randoc_core::Writer;

/// The HTML version of the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Version {
    Html4,
    Html5,
    Epub2,
    Epub3,
}

/// The EPUB version that determines the chapter markup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpubVersion {
    /// XHTML 1.1 with EPUB 2 footnotes.
    Epub2,
    /// XHTML with EPUB 3 semantic attributes.
    Epub3,
}

/// The rendered chapter and the properties for its package entry.
#[derive(Debug, Default)]
pub struct EpubChapter {
    /// The XHTML fragment for the chapter body.
    pub body: randoc_core::Text,
    /// The CSS for highlighted code in this chapter.
    pub highlighting_css: Option<randoc_core::Text>,
    /// The body contains MathML.
    pub has_math: bool,
    /// The body contains inline SVG.
    pub has_svg: bool,
    /// The body contains a script.
    pub has_script: bool,
}

/// Returns the prefixed EPUB identifier. Adds `id_` before an identifier
/// that starts with a character other than a letter.
pub fn epub_identifier(prefix: &str, id: &str) -> String {
    prefixed_id(prefix, id, true)
}

fn prefixed_id(prefix: &str, id: &str, requires_letter: bool) -> String {
    if requires_letter && id.chars().next().is_some_and(|c| !c.is_alphabetic()) {
        format!("{prefix}id_{id}")
    } else {
        format!("{prefix}{id}")
    }
}

/// Renders one EPUB chapter with its own footnotes and highlighting.
pub fn write_epub_chapter(
    blocks: &[randoc_ast::Block],
    options: &randoc_core::WriterOptions,
    context: &mut randoc_core::Context,
    version: EpubVersion,
) -> randoc_core::Result<EpubChapter> {
    let version = match version {
        EpubVersion::Epub2 => Version::Epub2,
        EpubVersion::Epub3 => Version::Epub3,
    };
    html::write_epub_chapter(blocks, options, context, version)
}

/// Returns the writer for an HTML document or slide deck.
pub fn writer(format: &str) -> Option<Writer> {
    use slides::SlideFormat;
    let (version, slides) = match format {
        "html" | "html5" => (Version::Html5, None),
        "html4" => (Version::Html4, None),
        name => {
            let slides = SlideFormat::from_name(name)?;
            (slides.version(), Some(slides))
        }
    };
    Some(Writer::Text(Arc::new(randoc_core::TemplateWriter(
        move |doc: &randoc_ast::Pandoc,
              options: &randoc_core::WriterOptions,
              context: &mut randoc_core::Context| {
            html::write_with_slides(doc, options, context, version, slides)
        },
    ))))
}

/// Renders blocks as an HTML5 fragment without a final newline, as the
/// writer `html` does for a document of these blocks. A writer of a
/// different format calls it for content that its syntax cannot show, for
/// example a table in Markdown.
pub fn render_blocks(
    blocks: &[randoc_ast::Block],
    options: &randoc_core::WriterOptions,
    context: &mut randoc_core::Context,
) -> randoc_core::Result<randoc_core::Text> {
    html::write_blocks(blocks, options, context, Version::Html5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_context_sets_citation_css_for_a_bibliography() {
        use randoc_ast::{Attr, Block, Pandoc};
        use randoc_core::{Context, WriterOptions};

        let template = randoc_templates::Template::compile(
            "html",
            "$if(csl-css)$citation CSS$else$body only$endif$",
            64,
            &mut |_| Err("no partials"),
        )
        .unwrap();
        let mut options = WriterOptions::default();
        options.template = Some(template);
        for (classes, expected) in [
            (vec!["csl-bib-body".into()], "citation CSS"),
            (vec!["other".into()], "body only"),
        ] {
            let doc = Pandoc {
                blocks: vec![Block::BlockQuote(vec![Block::Div(
                    Attr::new("", classes, vec![]),
                    vec![],
                )])],
                ..Default::default()
            };
            for version in [Version::Html5, Version::Html4] {
                assert_eq!(
                    html::write(&doc, &options, &mut Context::pure(), version).unwrap(),
                    expected
                );
            }
        }
    }

    #[test]
    fn render_blocks_gives_the_fragment_of_the_html_writer() {
        use randoc_ast::{Block, Inline};
        let blocks = [Block::Div(
            randoc_ast::Attr::new("d", Vec::new(), Vec::new()),
            vec![Block::Para(vec![Inline::Str("a<b".into())])],
        )];
        let mut context = randoc_core::Context::pure();
        let options = randoc_core::WriterOptions::default();
        let html = render_blocks(&blocks, &options, &mut context).unwrap();
        // `pandoc -f json -t html` gives these bytes and a final newline.
        assert_eq!(html, "<div id=\"d\">\n<p>a&lt;b</p>\n</div>");
    }

    #[test]
    fn renders_epub_footnotes_and_marks_chapter_resources() {
        use randoc_ast::{Attr, Block, Format, Inline, MathType, Target};
        use randoc_core::{Context, WriterOptions};
        let blocks = [
            Block::Header(
                1,
                Attr::new("one", Vec::new(), Vec::new()),
                vec![Inline::Str("One".into())],
            ),
            Block::Para(vec![
                Inline::Note(vec![Block::Para(vec![Inline::Str("Note".into())])]),
                Inline::Math(MathType::InlineMath, "x^2".into()),
                Inline::Image(
                    Attr::default(),
                    Vec::new(),
                    Target::new("https://example.test/image.png", ""),
                ),
            ]),
            Block::RawBlock(Format::new("html"), "<svg></svg><script></script>".into()),
        ];
        for version in [EpubVersion::Epub2, EpubVersion::Epub3] {
            let result = write_epub_chapter(
                &blocks,
                &WriterOptions::default(),
                &mut Context::pure(),
                version,
            )
            .unwrap();
            assert!(result.has_math && result.has_svg && result.has_script);
            assert!(result.body.contains("href=\"#fn1\""));
            assert!(!result.body.contains("<sup>1</sup>"));
            match version {
                EpubVersion::Epub2 => {
                    assert!(
                        result
                            .body
                            .contains("<div id=\"one\" class=\"section level1\">")
                    );
                    assert!(result.body.contains("class=\"footnote-back\">1</a>. Note"));
                }
                EpubVersion::Epub3 => {
                    assert!(
                        result
                            .body
                            .contains("<section id=\"one\" class=\"level1\">")
                    );
                    assert!(result.body.contains("epub:type=\"noteref\""));
                    assert!(result.body.contains("<aside epub:type=\"footnote\""));
                }
            }
        }
    }

    /// Write units with a known divergence. See `docs/ISSUES.md`, I-HTML-01 and
    /// I-HL-01 (`haskell` is not an embedded syntax).
    const KNOWN_DIVERGENCES: &[&str] = &[
        "cite-math/math-inline#",
        "cite-math/math-display#",
        "cite-math/math-in-text#",
        "cite-math/math-special-chars#",
        "trees/tree-math-empty#",
        "wrapping/long-math#",
        "inlines/code-inline-attrs#",
    ];

    /// Runs each recorded `#write:` unit of the conformance suite through the
    /// writer: the oracle tree of the `#read` unit is the input, and the oracle
    /// output without its final newline is the expected text.
    #[test]
    fn conformance_write_units_match_the_oracle() {
        // Run as the program does: a large stack and its nesting limit.
        std::thread::Builder::new()
            .stack_size(1 << 30)
            .spawn(check_write_units)
            .unwrap()
            .join()
            .unwrap();
    }

    fn check_write_units() {
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../conformance/expected/html-writer"
        );
        let mut checked = 0;
        let mut failures = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name.starts_with("._") || !name.ends_with(".json") {
                continue;
            }
            let suite = name.trim_end_matches(".json");
            let text = std::fs::read_to_string(&path).unwrap();
            let json: serde_json::Value = serde_json::from_str(&text).unwrap();
            let units = json["units"].as_object().unwrap();
            for (id, unit) in units {
                let Some((case, format)) = id.split_once("#write:") else {
                    continue;
                };
                let full = format!("{suite}/{case}#");
                if case.starts_with("opt-") || KNOWN_DIVERGENCES.contains(&full.as_str()) {
                    continue;
                }
                let tree = units[&format!("{case}#read")]["stdout"].as_str().unwrap();
                let doc = randoc_ast::json::from_json_with_limit(tree, 1 << 20).unwrap();
                let spec = randoc_core::FormatSpec::parse(format);
                let mut options = randoc_core::WriterOptions::default();
                options.extensions = spec.extensions().unwrap();
                let Some(Writer::Text(w)) = writer(&spec.name) else {
                    panic!("no writer for {format}");
                };
                let mut context = randoc_core::Context::pure();
                context.limits.max_nesting_depth = 16_384;
                let output = w.write(&doc, &options, &mut context).unwrap();
                let expected = unit["stdout"].as_str().unwrap();
                let expected = expected.strip_suffix('\n').unwrap_or(expected);
                checked += 1;
                if output != expected {
                    failures.push(format!("{full}{format}"));
                }
            }
        }
        assert!(checked > 800, "only {checked} units");
        assert!(
            failures.is_empty(),
            "{} failures: {failures:?}",
            failures.len()
        );
    }

    #[test]
    fn recognizes_html_documents_and_rejects_other_formats() {
        assert!(writer("html").is_some());
        assert!(writer("html5").is_some());
        assert!(writer("html4").is_some());
        assert!(writer("xhtml").is_none());
        assert!(writer("markdown").is_none());
    }
}
