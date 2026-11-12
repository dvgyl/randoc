//! The readers for the `commonmark`, `gfm` and `commonmark_x` formats.
//!
//! The parser follows the CommonMark specification 0.31.2 and the syntax
//! extensions of the Haskell libraries `commonmark` and
//! `commonmark-extensions`, which Pandoc uses for these formats.

mod attrs;
mod blocks;
mod builder;
#[cfg(test)]
mod case_tests;
mod convert;
mod inlines;
mod marks;
mod meta;
mod rebase;
mod scan;
mod schemes;

use std::sync::Arc;

use randoc_ast::{Meta, Pandoc};
use randoc_core::{Context, Extension, Limits, Reader, ReaderOptions, Result, Sources};

/// A count of the work of the parsers in test builds, for tests of the time that inputs need.
#[cfg(test)]
pub(crate) mod steps {
    use std::cell::Cell;

    thread_local! {
        static COUNT: Cell<u64> = const { Cell::new(0) };
    }

    /// Adds work units.
    pub(crate) fn add(units: u64) {
        COUNT.with(|count| count.set(count.get() + units));
    }

    /// Returns the work units since the last call and starts a new count.
    pub(crate) fn take() -> u64 {
        COUNT.with(|count| count.replace(0))
    }
}

/// Outside test builds, the count does nothing.
#[cfg(not(test))]
pub(crate) mod steps {
    /// Does nothing.
    #[inline(always)]
    pub(crate) fn add(_units: u64) {}
}

/// The format names that this crate reads.
const FORMATS: &[&str] = &["commonmark", "gfm", "commonmark_x"];

/// Returns the reader for `commonmark`, `gfm` or `commonmark_x`.
pub fn reader(format: &str) -> Option<Reader> {
    FORMATS
        .contains(&format)
        .then(|| Reader::Text(Arc::new(read)))
}

/// The settings of one parse, taken from the extensions of the reader options.
#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub(crate) smart: bool,
    pub(crate) strikeout: bool,
    pub(crate) subscript: bool,
    pub(crate) superscript: bool,
    pub(crate) footnotes: bool,
    pub(crate) pipe_tables: bool,
    pub(crate) task_lists: bool,
    pub(crate) autolink: bool,
    pub(crate) emoji: bool,
    pub(crate) math_dollars: bool,
    pub(crate) math_gfm: bool,
    pub(crate) attributes: bool,
    pub(crate) spans: bool,
    pub(crate) fenced_divs: bool,
    pub(crate) raw_attribute: bool,
    pub(crate) auto_identifiers: bool,
    pub(crate) ascii_identifiers: bool,
    pub(crate) hard_line_breaks: bool,
    pub(crate) wiki_after: bool,
    pub(crate) wiki_before: bool,
    pub(crate) alerts: bool,
    pub(crate) implicit_figures: bool,
    pub(crate) definition_lists: bool,
    pub(crate) fancy_lists: bool,
    pub(crate) implicit_refs: bool,
    /// The reader option `strip_comments`: HTML comments leave raw HTML of the body.
    pub(crate) strip_comments: bool,
    /// For `rebase_relative_paths`: the first line of each source and its directory.
    pub(crate) sources: Vec<(usize, String)>,
    pub(crate) max_depth: usize,
    /// The largest number of cells that the reader adds to the short rows of one table
    /// (`Limits::max_table_cells`). A row after that ends the table (`pipeTableBlockSpec`
    /// in `Commonmark/Extensions/PipeTable.hs`).
    pub(crate) max_added_cells: usize,
}

impl Config {
    /// Returns the directory to put before relative paths in the block that starts at `line`.
    pub(crate) fn source_dir(&self, line: usize) -> Option<&str> {
        // The starts never decrease, so a binary search finds the last source that starts
        // at or before `line`.
        let after = self.sources.partition_point(|(start, _)| {
            crate::steps::add(1);
            *start <= line
        });
        let (_, dir) = self.sources.get(after.checked_sub(1)?)?;
        (!dir.is_empty() && dir != ".").then_some(dir.as_str())
    }

    /// Returns the text of a raw element of `format`. With `strip_comments`, Pandoc
    /// removes each closed comment from the text of each raw `html` element, in one pass.
    pub(crate) fn raw_text(&self, format: &str, text: String) -> String {
        if !self.strip_comments || format != "html" {
            return text;
        }
        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some((before, after)) = rest.split_once("<!--")
            && let Some((_, tail)) = after.split_once("-->")
        {
            out.push_str(before);
            rest = tail;
        }
        out.push_str(rest);
        out
    }

    fn new(options: &ReaderOptions, limits: &Limits) -> Self {
        let on = |e| options.extensions.contains(e);
        Config {
            smart: on(Extension::Smart),
            strikeout: on(Extension::Strikeout),
            subscript: on(Extension::Subscript),
            superscript: on(Extension::Superscript),
            footnotes: on(Extension::Footnotes),
            pipe_tables: on(Extension::PipeTables),
            task_lists: on(Extension::TaskLists),
            autolink: on(Extension::AutolinkBareUris),
            emoji: on(Extension::Emoji),
            math_dollars: on(Extension::TexMathDollars),
            math_gfm: on(Extension::TexMathGfm),
            attributes: on(Extension::Attributes),
            spans: on(Extension::BracketedSpans),
            fenced_divs: on(Extension::FencedDivs),
            raw_attribute: on(Extension::RawAttribute),
            auto_identifiers: on(Extension::GfmAutoIdentifiers),
            ascii_identifiers: on(Extension::AsciiIdentifiers),
            hard_line_breaks: on(Extension::HardLineBreaks),
            wiki_after: on(Extension::WikilinksTitleAfterPipe),
            wiki_before: on(Extension::WikilinksTitleBeforePipe),
            alerts: on(Extension::Alerts),
            implicit_figures: on(Extension::ImplicitFigures),
            definition_lists: on(Extension::DefinitionLists),
            fancy_lists: on(Extension::FancyLists),
            implicit_refs: on(Extension::ImplicitHeaderReferences),
            strip_comments: options.strip_comments,
            sources: Vec::new(),
            max_depth: limits.max_nesting_depth.max(4),
            max_added_cells: limits.max_table_cells,
        }
    }
}

fn read(sources: &Sources, options: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    let mut config = Config::new(options, &context.limits);
    if options.extensions.contains(Extension::RebaseRelativePaths) {
        // `Sources::concat` ends each source with a line ending and puts one more between sources.
        let mut line = 1;
        for (index, source) in sources.0.iter().enumerate() {
            if index > 0 {
                line += 1;
            }
            config.sources.push((line, directory(&source.name)));
            let text = &source.text;
            line +=
                text.matches('\n').count() + usize::from(!text.is_empty() && !text.ends_with('\n'));
        }
    }
    // The `commonmark` tokenizer normalizes the input to Unicode NFC.
    let joined = sources.concat();
    let text = expand_tabs(&randoc_core::shared::nfc(&joined), options.tab_stop);
    let mut meta = Meta::default();
    let mut body = text.as_str();
    if options.extensions.contains(Extension::YamlMetadataBlock)
        && let Some(block) = meta::split_metadata(&text, config.max_depth)
    {
        // Pandoc does not rebase the targets in a metadata value.
        let value_config = Config {
            sources: Vec::new(),
            strip_comments: false,
            ..config.clone()
        };
        // The `commonmark` tokenizer normalizes each decoded metadata string to NFC too.
        let read_value = |value: &str, open_last_line: bool| {
            let value = randoc_core::shared::nfc(value);
            convert::convert(
                &blocks::parse_with(&value, &value_config, open_last_line),
                &value_config,
            )
        };
        let rest = block.rest;
        let name = sources.0.first().map_or("", |source| source.name.as_str());
        meta = meta::parse_metadata(block, name, &read_value, config.max_depth)?;
        let skipped = text.len() - rest.len();
        let lines = text.get(..skipped).unwrap_or("").matches('\n').count();
        for (start, _) in &mut config.sources {
            *start = start.saturating_sub(lines).max(1);
        }
        body = rest;
    }
    let mut body = body.to_owned();
    // Pandoc parses the input with one more line ending, which an open block at the end keeps.
    body.push('\n');
    let mut doc = parse(&body, &config);
    doc.meta = meta;
    Ok(doc)
}

/// Returns the directory part of a file path, as `takeDirectory` does: the
/// separators at its end go, except when the directory is only separators.
fn directory(path: &str) -> String {
    let Some((dir, _)) = path.rsplit_once('/') else {
        return ".".to_owned();
    };
    let trimmed = dir.trim_end_matches('/');
    if trimmed.is_empty() {
        path.get(..=dir.len()).unwrap_or("/").to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Replaces each tab with spaces to the next tab stop, as Pandoc does before it parses.
fn expand_tabs(text: &str, tab_stop: i64) -> String {
    let Ok(stop) = usize::try_from(tab_stop) else {
        return text.to_owned();
    };
    if stop == 0 || !text.contains('\t') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut column = 0;
    for c in text.chars() {
        match c {
            '\t' => {
                let spaces = stop - column % stop;
                out.extend(std::iter::repeat_n(' ', spaces));
                column += spaces;
            }
            '\n' => {
                out.push(c);
                column = 0;
            }
            _ => {
                out.push(c);
                column += 1;
            }
        }
    }
    out
}

fn parse(text: &str, config: &Config) -> Pandoc {
    let tree = blocks::parse(text, config);
    let blocks = convert::convert(&tree, config);
    Pandoc {
        meta: Meta::default(),
        blocks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use randoc_ast::{Block, Inline};
    use randoc_core::default_extensions;

    fn read_with(format: &str, text: &str) -> Vec<Block> {
        let mut options = ReaderOptions::default();
        options.extensions = default_extensions(format);
        let Some(Reader::Text(reader)) = reader(format) else {
            panic!("no reader")
        };
        reader
            .read(&Sources::from_text(text), &options, &mut Context::pure())
            .unwrap()
            .blocks
    }

    #[test]
    fn the_added_cells_of_a_pipe_table_come_from_the_limits_of_the_context() {
        let text = "| a | b | c |\n|---|---|---|\n| 1 |\n| 2 |\n| 3 |\n| 4 |\n";
        let read = |max_table_cells| {
            let mut context = Context::pure();
            context.limits.max_table_cells = max_table_cells;
            let mut options = ReaderOptions::default();
            options.extensions = default_extensions("gfm");
            let Some(Reader::Text(reader)) = reader("gfm") else {
                panic!("no reader")
            };
            let blocks = reader
                .read(&Sources::from_text(text), &options, &mut context)
                .unwrap()
                .blocks;
            blocks.len()
        };
        assert_eq!(read(200_000), 1);
        // Two short rows add 4 cells; the next row ends the table.
        assert_eq!(read(3), 2);
    }

    fn read_sources(format: &str, sources: Vec<(&str, &str)>) -> Pandoc {
        let spec = randoc_core::FormatSpec::parse(format);
        let mut options = ReaderOptions::default();
        options.extensions = spec.extensions().unwrap();
        let sources = Sources(
            sources
                .into_iter()
                .map(|(name, text)| randoc_core::Source {
                    name: name.into(),
                    text: text.into(),
                })
                .collect(),
        );
        read(&sources, &options, &mut Context::pure()).unwrap()
    }

    /// Codex review round 7, R7-05: each block looked for its source with a scan of the
    /// source list, so the time grew with the square of the number of sources.
    #[test]
    fn the_work_grows_linearly_with_the_number_of_sources() {
        let steps_for = |count: usize| {
            crate::steps::take();
            let doc = read_sources(
                "commonmark+rebase_relative_paths",
                vec![("sub/b.md", "[a](u)\n"); count],
            );
            assert_eq!(doc.blocks.len(), count);
            crate::steps::take()
        };
        let small = steps_for(2000);
        let large = steps_for(8000);
        assert!(
            large < 6 * small,
            "{small} steps for 2000, {large} for 8000"
        );
    }

    fn image_urls(format: &str, sources: Vec<(&str, &str)>) -> Vec<String> {
        let doc = read_sources(format, sources);
        let mut urls = Vec::new();
        randoc_ast::walk::query_inlines(&doc, |inline| {
            if let Inline::Image(_, _, target) = inline {
                urls.push(target.url.clone());
            }
        });
        urls
    }

    /// Pandoc 3.13 gives `sub/j` (case `rebase/r5-lines-after-attr-group`). The lines of
    /// the attribute group count, so the image is on a line of the second file.
    #[test]
    fn lines_of_an_attribute_group_count_for_the_source_of_a_block() {
        let urls = image_urls(
            "commonmark_x+rebase_relative_paths",
            vec![("a.md", "{.a\n}\n# h\n"), ("sub/b.md", "![i](j)\n")],
        );
        assert_eq!(urls, ["sub/j"]);
    }

    #[test]
    fn each_source_rebases_its_own_paths() {
        let format = "commonmark+rebase_relative_paths";
        assert_eq!(
            image_urls(format, vec![("a.md", "x"), ("sub/b.md", "\n![b](q.png)\n")]),
            vec!["sub/q.png"]
        );
        assert_eq!(
            image_urls(
                format,
                vec![("x/a.md", "x\n\n![a](p.png)\n"), ("b.md", "![b](q.png)\n")]
            ),
            vec!["x/p.png", "q.png"]
        );
        assert_eq!(
            image_urls(
                "commonmark+rebase_relative_paths+yaml_metadata_block",
                vec![
                    ("a.md", "---\ntitle: t\n---\n\nx\n\n"),
                    ("sub/b.md", "![b](q.png)\n")
                ]
            ),
            vec!["sub/q.png"]
        );
        assert_eq!(
            image_urls(
                format,
                vec![
                    ("a.md", ""),
                    ("x/b.md", "![b](q.png)"),
                    ("c.md", "![c](r.png)\n")
                ]
            ),
            vec!["x/q.png", "r.png"]
        );
        assert_eq!(
            image_urls(
                format,
                vec![("x/a.md", "![a](p.png)\n"), ("b.md", "\n![b](q.png)\n")]
            ),
            vec!["x/p.png", "q.png"]
        );
        assert_eq!(directory("/a.md"), "/");
        // Oracle: `sub//a.md` and `sub///a.md` give the target `sub/x`, `./sub//a.md` gives
        // `./sub/x`, and `/t//rb//sub//b.md` keeps the inner `//`: `/t//rb//sub/x`.
        assert_eq!(directory("sub//a.md"), "sub");
        assert_eq!(directory("sub///a.md"), "sub");
        assert_eq!(directory("./sub//a.md"), "./sub");
        assert_eq!(directory("/t//rb//sub//b.md"), "/t//rb//sub");
    }

    #[test]
    fn tabs_expand_to_tab_stops() {
        assert_eq!(expand_tabs("\ta\tbc\td\n\tx", 4), "    a   bc  d\n    x");
        assert_eq!(expand_tabs("\ta", 0), "\ta");
        assert_eq!(expand_tabs("\ta", -1), "\ta");
    }

    #[test]
    fn formats_are_registered() {
        assert!(reader("commonmark").is_some());
        assert!(reader("gfm").is_some());
        assert!(reader("commonmark_x").is_some());
        assert!(reader("markdown").is_none());
    }

    #[test]
    fn paragraph_and_emphasis() {
        assert_eq!(
            read_with("commonmark", "*a* b\n"),
            vec![Block::Para(vec![
                Inline::Emph(vec![Inline::Str("a".into())]),
                Inline::Space,
                Inline::Str("b".into())
            ])]
        );
    }
}
