//! Captions as lines of inlines: Pandoc writes the blocks of a table or
//! figure caption as one list of inlines, with a line break between the
//! lines that the blocks give.

use randoc_ast::{Block, Inline, Row};
use randoc_core::Result;
use randoc_core::shared::stringify;
use randoc_layout::{Doc, empty, literal};

use crate::W;

/// One line of a caption.
pub(crate) struct Line {
    pub doc: Doc,
    /// The plain text of the line, as `stringify` gives it.
    pub text: String,
    /// `true` when the line has no inlines.
    pub empty: bool,
}

impl Line {
    fn new(doc: Doc, text: String, empty: bool) -> Self {
        Line { doc, text, empty }
    }
}

/// How to walk the blocks of a caption.
#[derive(Clone, Copy)]
struct Mode {
    /// `true` when the first line and the other lines are at the start of
    /// a line of the output.
    starts: (bool, bool),
    /// `false` to find only the text of the lines.
    render: bool,
}

/// The lines of a caption joined with line breaks.
pub(crate) struct Lines {
    pub doc: Doc,
    pub text: String,
    /// `true` when the caption has no inlines, so that it is not written.
    pub empty: bool,
}

impl W<'_> {
    /// Renders caption blocks as lines joined with line breaks. `starts`
    /// tells if the first line and the other lines are at the start of a
    /// line of the output.
    pub(crate) fn caption_lines(
        &mut self,
        blocks: &[Block],
        starts: (bool, bool),
    ) -> Result<Lines> {
        let mut lines = Vec::new();
        self.caption_blocks(
            blocks,
            Mode {
                starts,
                render: true,
            },
            &mut lines,
        )?;
        Ok(self.join_lines(lines))
    }

    /// The plain text of caption blocks, as [`Self::caption_lines`] gives
    /// it, without writing anything (no notes or references are stored).
    pub(crate) fn caption_text(&mut self, blocks: &[Block]) -> Result<String> {
        let mut lines = Vec::new();
        let mode = Mode {
            starts: (false, false),
            render: false,
        };
        self.caption_blocks(blocks, mode, &mut lines)?;
        Ok(self.join_lines(lines).text)
    }

    fn join_lines(&self, lines: Vec<Line>) -> Lines {
        let empty_all = match lines.as_slice() {
            [] => true,
            [only] => only.empty,
            _ => false,
        };
        let mut doc = empty();
        let mut texts = Vec::new();
        for (index, line) in lines.into_iter().enumerate() {
            if index > 0 {
                doc += self.line_break();
            }
            doc += line.doc;
            texts.push(line.text);
        }
        Lines {
            doc,
            text: texts.join(" "),
            empty: empty_all,
        }
    }

    fn caption_blocks(&mut self, blocks: &[Block], mode: Mode, out: &mut Vec<Line>) -> Result<()> {
        self.enter()?;
        let result = self.caption_blocks_in(blocks, mode, out);
        self.leave();
        result
    }

    fn caption_blocks_in(
        &mut self,
        blocks: &[Block],
        mode: Mode,
        out: &mut Vec<Line>,
    ) -> Result<()> {
        let starts = mode.starts;
        for block in blocks {
            let line_start = if out.is_empty() { starts.0 } else { starts.1 };
            let line = match block {
                Block::Plain(inlines) | Block::Para(inlines) | Block::Header(_, _, inlines) => {
                    self.caption_inlines(inlines, line_start, mode.render)?
                }
                Block::LineBlock(lines) => {
                    let mut parts = Vec::new();
                    for (index, inlines) in lines.iter().enumerate() {
                        let start = if index == 0 { line_start } else { starts.1 };
                        parts.push(self.caption_inlines(inlines, start, mode.render)?);
                    }
                    let joined = self.join_lines(parts);
                    Line::new(joined.doc, joined.text, joined.empty)
                }
                Block::CodeBlock(attr, text) => {
                    let doc = if mode.render {
                        self.code(attr, text)
                    } else {
                        empty()
                    };
                    Line::new(doc, text.clone(), false)
                }
                Block::RawBlock(format, text) => {
                    let doc = if mode.render {
                        self.raw_inline(&format.as_str().to_lowercase(), text)
                    } else {
                        empty()
                    };
                    Line::new(doc, String::new(), false)
                }
                Block::HorizontalRule => Line::new(empty(), String::new(), true),
                Block::BlockQuote(inner) | Block::Div(_, inner) | Block::Figure(_, _, inner) => {
                    self.caption_blocks(inner, mode, out)?;
                    continue;
                }
                Block::BulletList(items) | Block::OrderedList(_, items) => {
                    let items: Vec<&[Block]> = items.iter().map(Vec::as_slice).collect();
                    self.caption_items(&items, line_start, mode)?
                }
                Block::DefinitionList(items) => {
                    let mut doc = empty();
                    let mut text = String::new();
                    for (index, (term, definitions)) in items.iter().enumerate() {
                        let start = index == 0 && line_start;
                        let term_line = self.caption_inlines(term, start, mode.render)?;
                        let definitions: Vec<&[Block]> =
                            definitions.iter().map(Vec::as_slice).collect();
                        let rest = self.caption_items(&definitions, false, mode)?;
                        doc += term_line.doc + literal(": ") + rest.doc;
                        text.push_str(&term_line.text);
                        text.push_str(": ");
                        text.push_str(&rest.text);
                    }
                    Line::new(doc, text, items.is_empty())
                }
                Block::Table(table) => {
                    let bodies = table
                        .bodies
                        .iter()
                        .flat_map(|b| b.head.iter().chain(&b.body));
                    let rows: Vec<&Row> = table
                        .head
                        .rows
                        .iter()
                        .chain(bodies)
                        .chain(&table.foot.rows)
                        .collect();
                    for row in rows {
                        let start = if out.is_empty() { starts.0 } else { starts.1 };
                        let cells: Vec<&[Block]> =
                            row.cells.iter().map(|c| c.content.as_slice()).collect();
                        let line = self.caption_items(&cells, start, mode)?;
                        out.push(line);
                    }
                    continue;
                }
            };
            out.push(line);
        }
        Ok(())
    }

    fn caption_inlines(
        &mut self,
        inlines: &[Inline],
        line_start: bool,
        render: bool,
    ) -> Result<Line> {
        let doc = if render {
            self.inline_list(inlines, line_start)?
        } else {
            empty()
        };
        Ok(Line::new(doc, stringify(inlines), inlines.is_empty()))
    }

    /// The items of a list as one line: the lines of each item joined with
    /// line breaks, and the items one after the other.
    fn caption_items(&mut self, items: &[&[Block]], line_start: bool, mode: Mode) -> Result<Line> {
        let mut doc = empty();
        let mut text = String::new();
        let mut empty_all = true;
        for (index, item) in items.iter().enumerate() {
            let mut lines = Vec::new();
            let start = index == 0 && line_start;
            let item_mode = Mode {
                starts: (start, mode.starts.1),
                ..mode
            };
            self.caption_blocks(item, item_mode, &mut lines)?;
            let joined = self.join_lines(lines);
            doc += joined.doc;
            text.push_str(&joined.text);
            empty_all &= joined.empty;
        }
        Ok(Line::new(doc, text, empty_all))
    }
}
