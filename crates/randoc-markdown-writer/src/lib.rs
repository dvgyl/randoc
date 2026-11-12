//! The Markdown writer family of Randoc.
//!
//! One writer gives the output of Pandoc 3.13 for the formats `markdown`,
//! `markdown_strict`, `markdown_phpextra`, `markdown_mmd`, `markdown_github`,
//! `gfm`, `commonmark`, `commonmark_x`, `plain` and `markua`. The extensions
//! in [`WriterOptions::extensions`] select most of the syntax. The variant of
//! the format selects the rest.

mod blocks;
mod escape;
mod html;
mod inlines;
mod tables;

use std::sync::Arc;

use randoc_ast::{Block, Pandoc, Target, Text};
use randoc_core::{
    Context, Error, Extension, Extensions, LogMessage, ReferenceLocation, Result, WrapOption,
    Writer, WriterOptions,
};
use randoc_layout::{Doc, blankline, cr, empty, hang, literal, nest};

/// The syntax family of an output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Variant {
    /// Pandoc Markdown and its variants such as `markdown_strict`.
    Markdown,
    /// `commonmark`, `commonmark_x` and `gfm`.
    Commonmark,
    /// `plain`.
    Plain,
    /// `markua`.
    Markua,
}

/// Returns the writer for a format name of this crate.
pub fn writer(format: &str) -> Option<Writer> {
    let variant = match format {
        "markdown" | "markdown_strict" | "markdown_phpextra" | "markdown_mmd" => Variant::Markdown,
        "markdown_github" => {
            return Some(Writer::Text(Arc::new(
                |doc: &Pandoc, options: &WriterOptions, context: &mut Context| {
                    context.report(LogMessage::Deprecated(
                        Text::from("markdown_github"),
                        Text::from("Use gfm instead."),
                    ));
                    write(doc, options, context, Variant::Markdown)
                },
            )));
        }
        "gfm" | "commonmark" | "commonmark_x" => Variant::Commonmark,
        "plain" => Variant::Plain,
        "markua" => Variant::Markua,
        _ => return None,
    };
    Some(Writer::Text(Arc::new(
        move |doc: &Pandoc, options: &WriterOptions, context: &mut Context| {
            write(doc, options, context, variant)
        },
    )))
}

/// Renders a document as a fragment.
fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
    variant: Variant,
) -> Result<Text> {
    let mut w = W::new(options, variant, context.limits.max_nesting_depth);
    // Pandoc converts the metadata before the body, also for a fragment, so
    // the notes of the metadata take the first numbers.
    w.meta_context(&doc.meta)?;
    let body = w.document(&doc.blocks)?;
    for message in std::mem::take(&mut w.messages) {
        context.report(message);
    }
    let mut text = body.render(line_width(options));
    while text.ends_with('\n') {
        text.pop();
    }
    Ok(text)
}

/// The line width of the output, or `None` for lines without a limit.
fn line_width(options: &WriterOptions) -> Option<usize> {
    match options.wrap_text {
        WrapOption::WrapAuto => usize::try_from(options.columns).ok(),
        WrapOption::WrapNone | WrapOption::WrapPreserve => None,
    }
}

/// A link reference: the label, the target and the attributes text.
pub(crate) struct LinkRef {
    pub label: Text,
    pub target: Target,
    pub attrs: Text,
}

/// The state of one run of the writer.
pub(crate) struct W<'a> {
    pub opts: &'a WriterOptions,
    pub variant: Variant,
    pub ext: Extensions,
    /// The rendered notes that wait for output.
    pub notes: Vec<Doc>,
    /// For each waiting note: `true` when it starts with a paragraph.
    pub note_starts: Vec<bool>,
    /// The number of notes that were already written.
    pub notes_written: usize,
    /// The link references that wait for output.
    pub refs: Vec<LinkRef>,
    /// The labels of all link references of the document.
    pub all_refs: Vec<LinkRef>,
    pub depth: usize,
    pub max_depth: usize,
    pub messages: Vec<LogMessage>,
    /// `true` inside a table cell.
    pub in_table: bool,
    /// `true` inside a heading.
    pub in_heading: bool,
    /// The number of fenced divs in the content written last, counted
    /// through divs only: a list or a quote between two divs stops the count.
    pub div_count: usize,
    /// `true` when plain text must be in upper case (small caps in `plain`).
    pub upper: bool,
    /// `true` inside a superscript or subscript, where spaces need a backslash.
    pub in_script: bool,
    /// The labels of the link references, in lower case, with their index.
    pub ref_index: std::collections::HashMap<Text, usize>,
    /// The number of enclosing list items.
    pub list_level: usize,
    /// For the next inline list: drop the outer spaces of its first and of
    /// its last element, because the enclosing emphasis writes them.
    pub cut_list: (bool, bool),
    /// For the next delimited element: drop its leading and trailing space.
    pub cut_edges: (bool, bool),
    /// The automatic identifiers of the headings written so far.
    pub heading_ids: randoc_core::shared::IdentifierSet,
}

impl<'a> W<'a> {
    fn new(opts: &'a WriterOptions, variant: Variant, max_depth: usize) -> Self {
        W {
            opts,
            variant,
            ext: opts.extensions,
            notes: Vec::new(),
            note_starts: Vec::new(),
            notes_written: 0,
            refs: Vec::new(),
            all_refs: Vec::new(),
            depth: 0,
            max_depth,
            messages: Vec::new(),
            in_table: false,
            in_heading: false,
            list_level: 0,
            div_count: 0,
            upper: false,
            in_script: false,
            ref_index: std::collections::HashMap::new(),
            cut_list: (false, false),
            cut_edges: (false, false),
            heading_ids: randoc_core::shared::IdentifierSet::new(),
        }
    }

    pub(crate) fn has(&self, extension: Extension) -> bool {
        self.ext.contains(extension)
    }

    /// Renders a document at the line width of the options, from column 0.
    pub(crate) fn render_alone(&self, doc: Doc) -> Text {
        doc.render(line_width(self.opts))
    }

    pub(crate) fn plain(&self) -> bool {
        self.variant == Variant::Plain
    }

    /// Counts one nesting level and fails when the document is too deep.
    pub(crate) fn enter(&mut self) -> Result<()> {
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(depth_error());
        }
        Ok(())
    }

    pub(crate) fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Converts the metadata with this writer, as the template context.
    fn meta_context(&mut self, meta: &randoc_ast::Meta) -> Result<randoc_templates::Context> {
        let max_depth = self.max_depth;
        let variables = &self.opts.variables;
        let writer = std::cell::RefCell::new(&mut *self);
        randoc_core::shared::meta_context::meta_to_context(
            meta,
            variables,
            max_depth,
            &mut |blocks| writer.borrow_mut().block_list(blocks),
            &mut |inlines| writer.borrow_mut().inline_list(inlines, false),
        )
    }

    fn document(&mut self, blocks: &[Block]) -> Result<Doc> {
        let location = self.opts.reference_location;
        let mut out = empty();
        if location == ReferenceLocation::EndOfDocument {
            out = self.block_list(blocks)?;
        } else {
            let mut parts = Vec::new();
            for block in blocks {
                if location == ReferenceLocation::EndOfSection && matches!(block, Block::Header(..))
                {
                    parts.push(self.pending_notes_and_refs());
                }
                parts.push(self.block_list(std::slice::from_ref(block))?);
                if location == ReferenceLocation::EndOfBlock {
                    parts.push(self.pending_notes_and_refs());
                }
            }
            for part in parts {
                out = out.above_blank(part);
            }
        }
        let rest = self.pending_notes_and_refs();
        Ok(out.above_blank(rest))
    }

    /// Renders the notes and references that wait for output.
    fn pending_notes_and_refs(&mut self) -> Doc {
        let notes = std::mem::take(&mut self.notes);
        let note_starts_inline = std::mem::take(&mut self.note_starts);
        let first = self.notes_written + 1;
        self.notes_written += notes.len();
        let mut notes_doc = empty();
        for (index, note) in notes.into_iter().enumerate() {
            let number = first + index;
            let item = if self.has(Extension::Footnotes) && !self.plain() {
                let marker = format!("[^{}{number}]:", self.opts.identifier_prefix);
                if note.is_empty() {
                    literal(marker)
                } else if note_starts_inline.get(index).copied().unwrap_or(true) {
                    hang(4, literal(marker) + literal(" "), note)
                } else {
                    literal(marker) + cr() + nest(4, note)
                }
            } else {
                literal(format!("[{number}]")).spaced(note)
            };
            notes_doc = notes_doc.above_blank(item);
        }
        let refs = std::mem::take(&mut self.refs);
        let mut refs_doc = empty();
        for reference in refs {
            refs_doc = refs_doc.above(self.reference_line(&reference));
        }
        if !refs_doc.is_empty() {
            refs_doc = nest(2, refs_doc) + blankline();
        }
        notes_doc.above_blank(refs_doc)
    }

    fn reference_line(&self, reference: &LinkRef) -> Doc {
        let mut line = format!("[{}]: {}", reference.label, url_text(&reference.target.url));
        if !reference.target.title.is_empty() {
            line.push_str(&format!(" \"{}\"", reference.target.title));
        }
        if !reference.attrs.is_empty() {
            line.push(' ');
            line.push_str(&reference.attrs);
        }
        literal(line)
    }
}

/// The longest run of one character that a size from the input or the
/// options can make (heading level, column width, tab stop). Larger values
/// are cut to this length, so that an absurd number cannot exhaust memory.
pub(crate) const MAX_RUN: usize = 100_000;

/// Converts a size from the input or the options to a run length.
pub(crate) fn run_length(value: i64) -> usize {
    usize::try_from(value).unwrap_or(0).min(MAX_RUN)
}

/// The error for a document that is deeper than the nesting limit.
pub(crate) fn depth_error() -> Error {
    Error::App(Text::from(
        "The document is nested too deeply for the Markdown writer",
    ))
}

/// Writes a URL for a link destination.
pub(crate) fn url_text(url: &str) -> Text {
    if url.contains(' ') {
        format!("<{url}>")
    } else {
        url.to_owned()
    }
}
