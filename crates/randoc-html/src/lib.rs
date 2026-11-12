//! The HTML writer of Randoc: the formats `html`, `html5` and `html4`.
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
mod html;
pub mod shared;
mod standalone;
mod table;

use std::sync::Arc;

use randoc_core::Writer;

/// The HTML version of the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Version {
    Html4,
    Html5,
}

/// Returns the writer for `html`, `html5` or `html4`.
pub fn writer(format: &str) -> Option<Writer> {
    let version = match format {
        "html" | "html5" => Version::Html5,
        "html4" => Version::Html4,
        _ => return None,
    };
    Some(Writer::Text(Arc::new(
        move |doc: &randoc_ast::Pandoc,
              options: &randoc_core::WriterOptions,
              context: &mut randoc_core::Context| {
            html::write(doc, options, context, version)
        },
    )))
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

    /// Write units with a known divergence. See `docs/ISSUES.md`, I-HTML-01 and I-HTML-02.
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
    fn knows_the_three_html_formats_only() {
        assert!(writer("html").is_some());
        assert!(writer("html5").is_some());
        assert!(writer("html4").is_some());
        assert!(writer("xhtml").is_none());
        assert!(writer("markdown").is_none());
    }
}
