//! The LaTeX and Beamer writers of Randoc.
//!
//! The writer builds a [`randoc_layout::Doc`] from the document tree and
//! renders it at the line width of the options, as Pandoc does. With a
//! template in the options, the writer renders the template with the
//! context of `standalone.rs`.

#![forbid(unsafe_code)]

mod beamer;
mod blocks;
mod escape;
mod inlines;
mod lang;
mod md5;
#[cfg(test)]
mod option_tests;
pub mod shared;
mod standalone;
mod table;
#[cfg(test)]
mod wave9_tests;

use std::collections::HashSet;
use std::sync::Arc;

use randoc_ast::{Block, Inline, Pandoc, Text};
use randoc_core::{
    CiteMethod, Context, Error, Extension, LogMessage, Result, TemplateWriter, TopLevelDivision,
    WrapOption, Writer, WriterOptions,
};
use randoc_layout::Doc;

pub use escape::{StringContext, escape_uri, string_to_latex, to_label};

/// Returns the writer for `latex` or `beamer`.
pub fn writer(format: &str) -> Option<Writer> {
    match format {
        "latex" => Some(Writer::Text(Arc::new(TemplateWriter(write_latex)))),
        "beamer" => Some(Writer::Text(Arc::new(TemplateWriter(write_beamer)))),
        _ => None,
    }
}

/// Renders a document as LaTeX: a fragment, or the template of the options.
pub fn write_latex(doc: &Pandoc, options: &WriterOptions, context: &mut Context) -> Result<Text> {
    write(doc, options, context, false)
}

/// Returns the inline layout for `-t latex`.
///
/// BibTeX and BibLaTeX writers can indent continuation lines by four spaces
/// before they render the layout.
pub fn inlines_to_doc(
    inlines: &[Inline],
    options: &WriterOptions,
    context: &mut Context,
) -> Result<Doc> {
    let mut writer = LatexWriter::new(options, context.limits.max_nesting_depth, false)?;
    let doc = writer.inlines(inlines)?;
    for warning in writer.warnings {
        context.report(warning);
    }
    Ok(doc)
}

/// Renders a document as Beamer slides: a fragment, or the template of the options.
pub fn write_beamer(doc: &Pandoc, options: &WriterOptions, context: &mut Context) -> Result<Text> {
    write(doc, options, context, true)
}

fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
    beamer: bool,
) -> Result<Text> {
    let max_depth = context.limits.max_nesting_depth;
    // The class lookup below reads the variables, also for a fragment.
    standalone::check_variable_depth(&options.variables, max_depth)?;
    let mut writer = LatexWriter::new(options, max_depth, beamer)?;
    if options.cite_method == CiteMethod::Biblatex
        && let Some(lang) = doc
            .meta
            .get("lang")
            .map(randoc_core::shared::stringify_meta)
    {
        if randoc_citeproc::parse_lang(&lang).is_err() {
            writer.warnings.push(LogMessage::InvalidLang(lang));
        } else {
            writer.citation_lang = Some(lang);
        }
    }
    if options.variables.get("csquotes").is_none() {
        writer.csquotes = doc.meta.get("csquotes").is_some();
    }
    writer.chapter_class = standalone::has_chapters(
        standalone::document_class(&doc.meta, &options.variables).as_deref(),
    );
    // Pandoc converts the metadata before the body.
    let mut fields = match options.template {
        Some(_) => Some(writer.meta_context(&doc.meta)?),
        None => None,
    };
    let blocks = body_blocks(&doc.blocks, options.cite_method);
    let body = if beamer {
        writer.slides(&blocks)?
    } else {
        writer.blocks_after(Vec::new(), blocks)?
    };
    let width = match options.wrap_text {
        WrapOption::WrapAuto => usize::try_from(options.columns).ok(),
        WrapOption::WrapNone | WrapOption::WrapPreserve => None,
    };
    let text = match (&options.template, fields.as_mut()) {
        (Some(template), Some(fields)) => {
            writer.add_writer_fields(fields, doc, body, context)?;
            context.render_template(template, fields, width)?
        }
        _ => body.render(width),
    };
    for warning in writer.warnings {
        context.report(warning);
    }
    Ok(text)
}

/// Adds the syntax definition files of the options to the embedded
/// definitions. The result is `None` when the embedded definitions are
/// enough.
fn load_registry(options: &WriterOptions) -> Result<Option<randoc_highlight::Registry>> {
    if options.highlight_style.is_none() || options.syntax_definitions.is_empty() {
        return Ok(None);
    }
    let mut registry = randoc_highlight::Registry::embedded().clone();
    registry
        .add_definitions(
            options
                .syntax_definitions
                .iter()
                .map(|file| (file.path.as_str(), &*file.bytes)),
        )
        .map_err(Error::SyntaxMap)?;
    Ok(Some(registry))
}

/// The top-level blocks that the writer renders. With `natbib` or
/// `biblatex` the template places the bibliography: a top-level `refs` div
/// goes away, and so does a last level 1 heading, which titles the
/// bibliography.
fn body_blocks(blocks: &[Block], cite_method: CiteMethod) -> Vec<&Block> {
    let mut kept: Vec<&Block> = blocks.iter().collect();
    if cite_method == CiteMethod::Citeproc {
        return kept;
    }
    kept.retain(|block| !matches!(block, Block::Div(attr, _) if attr.id == "refs"));
    if let Some(Block::Header(1, _, _)) = kept.last() {
        kept.pop();
    }
    kept
}

/// The `\footnotetext` commands for notes that have marks. Several texts go
/// back to the number of the first mark and count up again.
pub(crate) fn footnote_texts(notes: Vec<Doc>) -> Doc {
    use randoc_layout::literal;
    let count = notes.len();
    let mut out = Doc::default();
    for (index, note) in notes.into_iter().enumerate() {
        if index == 0 && count > 1 {
            out = out.above(literal(format!(
                "\\addtocounter{{footnote}}{{-{}}}",
                count - 1
            )));
        } else if index > 0 {
            out = out.above(literal("\\addtocounter{footnote}{1}"));
        }
        out = out.above(literal("\\footnotetext{") + note + literal("}"));
    }
    out
}

/// The error for a document that is nested deeper than the limit.
pub(crate) fn too_deep() -> Error {
    Error::Parse(Text::from(
        "the document is nested too deeply for the LaTeX writer",
    ))
}

/// The state of one run of the writer.
pub(crate) struct LatexWriter<'a> {
    pub(crate) options: &'a WriterOptions,
    pub(crate) beamer: bool,
    pub(crate) smart: bool,
    pub(crate) csquotes: bool,
    /// The language of the BibLaTeX page labels.
    citation_lang: Option<String>,
    pub(crate) max_depth: usize,
    pub(crate) depth: usize,
    /// `true` when the writer skips notes, images and spans with an
    /// identifier, for the text of a PDF bookmark.
    pub(crate) skip_invalid: bool,
    /// `true` when the writer skips notes, for a short caption.
    pub(crate) skip_notes: bool,
    /// Set when the writer meets a note, an image or a span with an identifier.
    pub(crate) saw_invalid: bool,
    /// Set when the writer meets a note.
    pub(crate) saw_note: bool,
    /// Set when the writer meets code, which makes a Beamer frame fragile.
    pub(crate) saw_code: bool,
    /// The texts of notes in notes, which follow the enclosing block.
    pub(crate) block_notes: Vec<Doc>,
    /// `true` inside a footnote.
    pub(crate) in_note: bool,
    /// `true` inside a table or a minipage, where footnotes become marks.
    pub(crate) in_minipage: bool,
    /// `true` in the blocks of a figure, where a figure has no float.
    pub(crate) in_figure_body: bool,
    /// `true` directly inside the div with the identifier `refs`.
    pub(crate) in_references: bool,
    /// `true` in a cell of a table with no widths, where a line break gives
    /// a box of lines.
    pub(crate) box_lines: bool,
    /// `true` in an entry of the table of contents, where a link is a group.
    pub(crate) links_as_groups: bool,
    /// `true` while the head of a table is written a second time.
    pub(crate) repeated_head: bool,
    /// `true` inside a `minipage` environment of a table cell.
    pub(crate) in_minipage_env: bool,
    /// `true` inside a table cell, where display math is inline math.
    pub(crate) in_cell: bool,
    /// `true` when the document class has chapters, which makes the
    /// default top-level division the chapter.
    pub(crate) chapter_class: bool,
    /// The slide level of a Beamer document.
    pub(crate) slide_level: i64,
    /// `true` in the first block of the first definition of a definition
    /// list, where each list follows `\\hfill`.
    pub(crate) in_definition: bool,
    /// `true` inside a command of the `soul` package.
    pub(crate) in_soul: bool,
    /// `true` inside a heading.
    pub(crate) in_heading: bool,
    /// `true` inside the term of a definition list.
    pub(crate) in_term: bool,
    /// `true` inside a caption, where `\texttt` needs `\protect`.
    pub(crate) in_caption: bool,
    /// `true` inside the text of a citation of the citation filter, where a
    /// link to a bibliography entry is `\citeproc`.
    pub(crate) in_cite: bool,
    /// `true` inside a block quote.
    pub(crate) in_quote: bool,
    /// `true` at the start of a line, for `\strut` before a line break.
    pub(crate) empty_line: bool,
    /// The nesting level of ordered lists.
    pub(crate) enum_level: usize,
    /// `true` when lists show one item at a time (Beamer).
    pub(crate) incremental: bool,
    /// The footnote texts that wait for the end of a table.
    pub(crate) table_notes: Vec<Doc>,
    /// The addresses of the emphasis commands that the writer searched for
    /// notes that split them.
    pub(crate) emphasis_seen: HashSet<usize>,
    /// The addresses of the emphasis commands that such a note splits.
    pub(crate) emphasis_split: HashSet<usize>,
    /// The syntax definitions when the options add files to the embedded
    /// definitions.
    pub(crate) registry: Option<randoc_highlight::Registry>,
    /// The features that the output uses, for the template variables.
    pub(crate) usage: standalone::Usage,
    /// The warnings of the conversion, in order.
    pub(crate) warnings: Vec<LogMessage>,
}

impl<'a> LatexWriter<'a> {
    fn new(options: &'a WriterOptions, max_depth: usize, beamer: bool) -> Result<Self> {
        Ok(LatexWriter {
            options,
            beamer,
            smart: options.extensions.contains(Extension::Smart),
            csquotes: options
                .variables
                .get("csquotes")
                .is_some_and(standalone::csquotes_enabled),
            citation_lang: None,
            max_depth,
            depth: 0,
            skip_invalid: false,
            skip_notes: false,
            saw_invalid: false,
            saw_note: false,
            saw_code: false,
            block_notes: Vec::new(),
            in_note: false,
            in_minipage: false,
            in_figure_body: false,
            in_references: false,
            box_lines: false,
            links_as_groups: false,
            repeated_head: false,
            in_minipage_env: false,
            in_cell: false,
            in_soul: false,
            in_definition: false,
            chapter_class: false,
            slide_level: 0,
            in_heading: false,
            in_term: false,
            in_caption: false,
            in_cite: false,
            in_quote: false,
            empty_line: true,
            enum_level: 0,
            incremental: options.incremental,
            table_notes: Vec::new(),
            emphasis_seen: HashSet::new(),
            emphasis_split: HashSet::new(),
            registry: load_registry(options)?,
            usage: standalone::Usage::default(),
            warnings: Vec::new(),
        })
    }

    /// Enters one nesting level. Fails when the document is too deep.
    pub(crate) fn enter(&mut self) -> Result<()> {
        self.enter_levels(1)
    }

    /// Enters `levels` nesting levels at one time, for an element that
    /// needs much stack space.
    pub(crate) fn enter_levels(&mut self, levels: usize) -> Result<()> {
        self.depth = self.depth.saturating_add(levels);
        if self.depth > self.max_depth {
            return Err(too_deep());
        }
        Ok(())
    }

    /// Leaves one nesting level.
    pub(crate) fn leave(&mut self) {
        self.leave_levels(1);
    }

    /// Leaves `levels` nesting levels.
    pub(crate) fn leave_levels(&mut self, levels: usize) {
        self.depth = self.depth.saturating_sub(levels);
    }

    pub(crate) fn escape(&self, context: StringContext, text: &str) -> String {
        escape::escape_string(context, text, self.smart, self.options.prefer_ascii)
    }

    pub(crate) fn top_level_offset(&self) -> i64 {
        match self.options.top_level_division {
            TopLevelDivision::TopLevelPart => -1,
            TopLevelDivision::TopLevelChapter if self.beamer => -1,
            TopLevelDivision::TopLevelChapter => 0,
            TopLevelDivision::TopLevelDefault if self.chapter_class && self.beamer => -1,
            TopLevelDivision::TopLevelDefault if self.chapter_class => 0,
            TopLevelDivision::TopLevelSection | TopLevelDivision::TopLevelDefault => 1,
        }
    }

    /// Renders with footnotes as marks and returns the footnote texts after the result.
    pub(crate) fn with_note_marks<T>(
        &mut self,
        render: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<(T, Doc)> {
        let was_in_minipage = self.in_minipage;
        let outer_notes = std::mem::take(&mut self.table_notes);
        self.in_minipage = true;
        let result = render(self);
        self.in_minipage = was_in_minipage;
        let notes = std::mem::replace(&mut self.table_notes, outer_notes);
        Ok((result?, footnote_texts(notes)))
    }
}
