//! The Markdown writer family of Randoc.
//!
//! One writer gives the output of Pandoc 3.13 for the formats `markdown`,
//! `markdown_strict`, `markdown_phpextra`, `markdown_mmd`, `markdown_github`,
//! `gfm`, `commonmark`, `commonmark_x`, `plain` and `markua`. The extensions
//! in [`WriterOptions::extensions`] select most of the syntax. The variant of
//! the format selects the rest.

mod blocks;
mod caption;
#[cfg(test)]
mod conformance_tests;
mod entities;
mod escape;
mod html;
mod inlines;
mod standalone;
mod tables;
mod uri;

use std::sync::Arc;

use randoc_ast::{Block, Pandoc, Target, Text};
use randoc_core::{
    Context, Error, Extension, Extensions, LogMessage, ReferenceLocation, Result, WrapOption,
    Writer, WriterOptions,
};
use randoc_layout::{Doc, blankline, cr, empty, hang, literal, nest, space};

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
            return Some(Writer::Text(Arc::new(randoc_core::TemplateWriter(
                |doc: &Pandoc, options: &WriterOptions, context: &mut Context| {
                    context.report(LogMessage::Deprecated(
                        Text::from("markdown_github"),
                        Text::from("Use gfm instead."),
                    ));
                    write(doc, options, context, Variant::Markdown)
                },
            ))));
        }
        "gfm" | "commonmark" | "commonmark_x" => Variant::Commonmark,
        "plain" => Variant::Plain,
        "markua" => Variant::Markua,
        _ => return None,
    };
    Some(Writer::Text(Arc::new(randoc_core::TemplateWriter(
        move |doc: &Pandoc, options: &WriterOptions, context: &mut Context| {
            write(doc, options, context, variant)
        },
    ))))
}

/// Renders a document as a fragment.
fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
    variant: Variant,
) -> Result<Text> {
    let mut w = W::new(options, variant, context.limits.max_nesting_depth);
    let blocks = match doc.blocks.split_last() {
        Some((Block::Div(attr, _), rest)) if attr.id == "refs" && w.has(Extension::Citations) => {
            rest
        }
        _ => &doc.blocks,
    };
    let output = match &options.template {
        Some(template) => w.standalone(doc, blocks, template)?,
        None => {
            // Pandoc converts the metadata before the body, also for a
            // fragment, so the notes of the metadata take the first numbers.
            w.meta_context(&doc.meta)?;
            w.document(blocks)?
        }
    };
    for message in std::mem::take(&mut w.messages) {
        context.report(message);
    }
    let mut text = output.render(line_width(options));
    // Remove the layout's final line break. A final blank line can
    // be part of a raw block and must stay.
    if options.template.is_none() && text.ends_with('\n') && !text.ends_with("\n\n") {
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

/// The target of a link reference: the URL, the title and the attributes
/// text of the element.
pub(crate) type RefKey = (Text, Text, Text);

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
    pub depth: usize,
    pub max_depth: usize,
    pub messages: Vec<LogMessage>,
    /// `true` inside a table cell.
    pub in_table: bool,
    /// `true` inside a heading.
    pub in_heading: bool,
    /// The number of images written so far.
    pub images: usize,
    pub image_label_depth: usize,
    pub image_label_repeats: usize,
    /// `true` inside an entry of the table of contents, which drops notes
    /// and writes links as their content.
    pub in_toc: bool,
    /// The number of fenced divs in the content written last, counted
    /// through divs only: a list or a quote between two divs stops the count.
    pub div_count: usize,
    /// `true` inside small caps that use upper-case text.
    pub upper: bool,
    /// `true` inside a superscript or subscript, where spaces need a backslash.
    pub in_script: bool,
    /// The text labels of the link references, as [`reference_key`] gives
    /// them, with each target of the label and its number: 0 for the
    /// target of the text label, else the number of its numbered label.
    pub ref_index: std::collections::HashMap<Text, std::collections::HashMap<RefKey, u64>>,
    /// The label of each target in `refs`.
    pub pending_targets: std::collections::HashMap<RefKey, Text>,
    /// The first number that can be free for a numbered reference label.
    pub next_ref_number: u64,
    /// The number of enclosing list items.
    pub list_level: usize,
    /// For the next inline list: drop the outer spaces of its first and of
    /// its last element, because the enclosing emphasis writes them.
    pub cut_list: (bool, bool),
    /// For the next delimited element: drop its leading and trailing space.
    pub cut_edges: (bool, bool),
    /// The automatic identifiers of the headings written so far.
    pub heading_ids: randoc_core::shared::IdentifierSet,
    /// The edge-space results of lists inside the delimited elements that
    /// are being written, by address, length and side of the list.
    pub edge_memo: std::collections::HashMap<(usize, usize, bool), bool>,
    /// The keys of `edge_memo` in the order of insertion, so that each
    /// delimited element removes its own keys at its end.
    pub edge_keys: Vec<(usize, usize, bool)>,
    /// The number of lists that the edge-space walk visited.
    #[cfg(test)]
    pub edge_steps: usize,
}

impl<'a> W<'a> {
    pub(crate) fn new(opts: &'a WriterOptions, variant: Variant, max_depth: usize) -> Self {
        W {
            opts,
            variant,
            ext: opts.extensions,
            notes: Vec::new(),
            note_starts: Vec::new(),
            notes_written: 0,
            refs: Vec::new(),
            depth: 0,
            max_depth,
            messages: Vec::new(),
            in_table: false,
            in_heading: false,
            in_toc: false,
            images: 0,
            image_label_depth: 0,
            image_label_repeats: 0,
            list_level: 0,
            div_count: 0,
            upper: false,
            in_script: false,
            ref_index: std::collections::HashMap::new(),
            pending_targets: std::collections::HashMap::new(),
            next_ref_number: 1,
            cut_list: (false, false),
            cut_edges: (false, false),
            heading_ids: randoc_core::shared::IdentifierSet::new(),
            edge_memo: std::collections::HashMap::new(),
            edge_keys: Vec::new(),
            #[cfg(test)]
            edge_steps: 0,
        }
    }

    pub(crate) fn has(&self, extension: Extension) -> bool {
        self.ext.contains(extension)
    }

    /// Returns `true` when a reference whose label is its text can take the
    /// shortcut form `[text]`. The CommonMark variants have no extension for it.
    pub(crate) fn shortcut_references(&self) -> bool {
        self.variant == Variant::Commonmark || self.has(Extension::ShortcutReferenceLinks)
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

    /// Converts the metadata with this writer, without the variables.
    fn meta_context(&mut self, meta: &randoc_ast::Meta) -> Result<randoc_templates::Context> {
        let max_depth = self.max_depth;
        let writer = std::cell::RefCell::new(&mut *self);
        randoc_core::shared::meta_context::meta_to_context(
            meta,
            &randoc_templates::Context::new(),
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
                    let pending = self.pending_notes_and_refs();
                    // With a table of contents, Pandoc starts the body with
                    // a blank line before the notes of the first section.
                    let lead = parts.is_empty()
                        && self.opts.table_of_contents
                        && self.opts.template.is_some()
                        && !pending.is_empty();
                    parts.push(if lead { blankline() + pending } else { pending });
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
                let marker = literal(format!("[{}{number}]", self.opts.identifier_prefix));
                if note_starts_inline.get(index).copied().unwrap_or(true) {
                    marker.spaced(note)
                } else {
                    marker + cr() + note
                }
            };
            notes_doc = notes_doc.above_blank(item);
        }
        let refs = std::mem::take(&mut self.refs);
        self.pending_targets.clear();
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
        let label = format!("[{}]:", reference.label);
        // The destination goes to the next line only when the label fills
        // the line (the references have an indent of 2).
        let full = line_width(self.opts)
            .is_some_and(|width| 2 + randoc_layout::real_length(&label) >= width);
        let url = literal(url_text(&reference.target.url));
        let mut line = if full {
            literal(label) + cr() + nest(2, url)
        } else {
            literal(label) + literal(" ") + url
        };
        // The title can go to the next line.
        let mut rest = empty();
        if !reference.target.title.is_empty() {
            rest += nest(
                2,
                space() + literal(format!("\"{}\"", reference.target.title)),
            );
        }
        if !reference.attrs.is_empty() {
            rest += literal(format!(" {}", reference.attrs));
        }
        if full {
            line += nest(2, rest);
        } else {
            line += rest;
        }
        line
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
