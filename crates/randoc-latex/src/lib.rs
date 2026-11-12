//! The LaTeX and Beamer writers of Randoc.
//!
//! The writer builds a [`randoc_layout::Doc`] from the document tree and
//! renders it at the line width of the options, as Pandoc does.

mod beamer;
mod blocks;
mod escape;
mod inlines;
mod lang;
#[cfg(test)]
mod option_tests;
pub mod shared;
mod table;

use std::collections::HashSet;
use std::sync::Arc;

use randoc_ast::{Block, Pandoc, Text};
use randoc_core::{
    CiteMethod, Context, Error, Extension, Result, TopLevelDivision, WrapOption, Writer,
    WriterOptions,
};
use randoc_layout::Doc;

pub use escape::{StringContext, escape_uri, string_to_latex, to_label};

/// Returns the writer for `latex` or `beamer`.
pub fn writer(format: &str) -> Option<Writer> {
    match format {
        "latex" => Some(Writer::Text(Arc::new(write_latex))),
        "beamer" => Some(Writer::Text(Arc::new(write_beamer))),
        _ => None,
    }
}

/// Renders a document as a LaTeX fragment.
pub fn write_latex(doc: &Pandoc, options: &WriterOptions, context: &mut Context) -> Result<Text> {
    write(doc, options, context, false)
}

/// Renders a document as a Beamer fragment.
pub fn write_beamer(doc: &Pandoc, options: &WriterOptions, context: &mut Context) -> Result<Text> {
    write(doc, options, context, true)
}

fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
    beamer: bool,
) -> Result<Text> {
    let mut writer = LatexWriter::new(options, context.limits.max_nesting_depth, beamer);
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
    let mut text = body.render(width);
    // A fragment has no final newline. The program adds one.
    if text.ends_with('\n') {
        text.pop();
    }
    Ok(text)
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
}

impl<'a> LatexWriter<'a> {
    fn new(options: &'a WriterOptions, max_depth: usize, beamer: bool) -> Self {
        LatexWriter {
            options,
            beamer,
            smart: options.extensions.contains(Extension::Smart),
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
            slide_level: 0,
            in_heading: false,
            in_term: false,
            in_caption: false,
            in_quote: false,
            empty_line: true,
            enum_level: 0,
            incremental: options.incremental,
            table_notes: Vec::new(),
            emphasis_seen: HashSet::new(),
            emphasis_split: HashSet::new(),
        }
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
        string_to_latex(context, text, self.smart)
    }

    pub(crate) fn top_level_offset(&self) -> i64 {
        match self.options.top_level_division {
            TopLevelDivision::TopLevelPart => -1,
            TopLevelDivision::TopLevelChapter if self.beamer => -1,
            TopLevelDivision::TopLevelChapter => 0,
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
