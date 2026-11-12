//! Tables: simple, pipe, multiline and grid tables, and the HTML form.

use randoc_ast::{Alignment, Block, Cell, ColWidth, Inline, Table, Text};
use randoc_core::{Extension, Result};
use randoc_layout::{Doc, blankline, cr, empty, literal, nest, real_length};

use crate::W;

/// The cells of a table in a plain grid: one header row and the body rows.
struct Grid<'t> {
    aligns: Vec<Alignment>,
    widths: Vec<f64>,
    head: Option<Vec<&'t Cell>>,
    body: Vec<Vec<&'t Cell>>,
    /// The index in `body` of the first row of the table foot.
    foot_start: Option<usize>,
    has_spans: bool,
}

fn grid(table: &Table) -> Grid<'_> {
    let aligns = table.col_specs.iter().map(|spec| spec.align).collect();
    let widths = table
        .col_specs
        .iter()
        .map(|spec| match spec.width {
            ColWidth::ColWidth(w) => w,
            ColWidth::ColWidthDefault => 0.0,
        })
        .collect();
    let all_rows = table
        .head
        .rows
        .iter()
        .chain(
            table
                .bodies
                .iter()
                .flat_map(|b| b.head.iter().chain(b.body.iter())),
        )
        .chain(table.foot.rows.iter());
    let has_spans = all_rows
        .flat_map(|row| row.cells.iter())
        .any(|cell| cell.row_span != 1 || cell.col_span != 1)
        || table.head.rows.len() > 1;
    let mut head_rows: Vec<Vec<&Cell>> = table
        .head
        .rows
        .iter()
        .map(|r| r.cells.iter().collect())
        .collect();
    let mut body: Vec<Vec<&Cell>> = Vec::new();
    for section in &table.bodies {
        for row in section.head.iter().chain(section.body.iter()) {
            body.push(row.cells.iter().collect());
        }
    }
    let foot_start = (!table.foot.rows.is_empty()).then_some(body.len());
    for row in &table.foot.rows {
        body.push(row.cells.iter().collect());
    }
    let head = if head_rows.is_empty() {
        None
    } else {
        Some(head_rows.remove(0))
    };
    Grid {
        aligns,
        widths,
        head,
        body,
        foot_start,
        has_spans,
    }
}

/// The lines of each cell of one row.
type RowLines = Vec<Vec<Text>>;

/// The documents of the header cells and of the body cells.
type CellDocs = (Option<Vec<Doc>>, Vec<Vec<Doc>>);

/// Renders a cell document as lines at a width.
fn doc_lines(doc: &Doc, width: Option<usize>) -> Vec<Text> {
    let text = doc.render(width);
    let text = text.trim_end_matches('\n');
    if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').map(|l| l.trim_end().to_owned()).collect()
    }
}

/// The plain-text table forms.
#[derive(Clone, Copy)]
enum Kind {
    Simple,
    Pipe,
    Multiline,
    Grid,
}

/// Returns the inline lists of the paragraphs of a table caption.
fn caption_parts(table: &Table) -> Vec<&[Inline]> {
    table
        .caption
        .long
        .iter()
        .filter_map(|block| match block {
            Block::Plain(inlines) | Block::Para(inlines) => Some(inlines.as_slice()),
            _ => None,
        })
        .collect()
}

/// Returns `true` when the cell holds at most one `Plain` or `Para` block.
fn is_simple_cell(cell: &Cell) -> bool {
    match cell.content.as_slice() {
        [] => true,
        [Block::Plain(inlines) | Block::Para(inlines)] => {
            !inlines.iter().any(|i| matches!(i, Inline::LineBreak))
        }
        _ => false,
    }
}

impl W<'_> {
    pub(crate) fn table(&mut self, block: &Block, table: &Table) -> Result<Doc> {
        let g = grid(table);
        let columns = g.aligns.len();
        let rows_ok = g
            .head
            .iter()
            .chain(g.body.iter())
            .all(|row| row.len() == columns);
        let all_simple = g
            .head
            .iter()
            .chain(g.body.iter())
            .flatten()
            .all(|cell| is_simple_cell(cell));
        let no_widths = g.widths.iter().all(|w| *w == 0.0);
        let plain_ok = rows_ok && !g.has_spans && columns > 0;
        let (kind, indented) = if !plain_ok {
            (None, false)
        } else if g.foot_start.is_some() && self.has(Extension::GridTables) {
            (Some(Kind::Grid), false)
        } else if all_simple && no_widths && self.has(Extension::SimpleTables) {
            (Some(Kind::Simple), true)
        } else if all_simple && no_widths && self.has(Extension::PipeTables) {
            (Some(Kind::Pipe), false)
        } else if self.has(Extension::MultilineTables)
            && g.head
                .iter()
                .chain(g.body.iter())
                .flatten()
                .all(|c| matches!(c.content.as_slice(), [] | [Block::Plain(_)]))
        {
            (Some(Kind::Multiline), true)
        } else if self.has(Extension::GridTables) {
            (Some(Kind::Grid), false)
        } else if all_simple && self.has(Extension::PipeTables) {
            (Some(Kind::Pipe), false)
        } else {
            (None, false)
        };
        let Some(kind) = kind else {
            if self.has(Extension::RawHtml) && !self.plain() {
                return Ok(self.html_blocks(std::slice::from_ref(block))? + blankline());
            }
            let mut out = empty();
            for row in g.head.iter().chain(g.body.iter()) {
                for cell in row {
                    out += self.block_list(&cell.content)?;
                }
            }
            return Ok(out + blankline());
        };
        let lines = match kind {
            Kind::Simple => self.simple_lines(&g)?,
            Kind::Pipe => self.pipe_lines(&g)?,
            Kind::Multiline => self.multiline_lines(&g)?,
            Kind::Grid => self.grid_lines(&g)?,
        };
        let body = literal(lines.join("\n")) + cr();
        let parts = caption_parts(table);
        let mut caption = empty();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                caption += randoc_layout::space();
            }
            caption += self.inline_list(part, index == 0)?;
        }
        if !parts.is_empty() && !table.attr.is_empty() && !self.plain() {
            caption += literal(format!(" {}", crate::inlines::attr_text(&table.attr)));
        }
        let caption_doc = if parts.is_empty() {
            blankline()
        } else if self.has(Extension::TableCaptions) {
            blankline() + literal(": ") + caption + blankline()
        } else {
            blankline() + caption + blankline()
        };
        Ok(if indented {
            cr() + nest(2, body + caption_doc) + blankline()
        } else {
            body + caption_doc
        })
    }

    /// Builds the document of the content of a cell.
    fn cell_doc(&mut self, cell: &Cell) -> Result<Doc> {
        self.in_table = true;
        self.list_level += 1;
        let doc = self.block_list(&cell.content);
        self.list_level -= 1;
        self.in_table = false;
        doc
    }

    /// Builds the documents of all cells once, so that a table can render
    /// them at more than one width without building nested tables again.
    fn cell_docs(&mut self, g: &Grid<'_>) -> Result<CellDocs> {
        let mut head = None;
        if let Some(row) = &g.head {
            let mut cells = Vec::new();
            for cell in row {
                cells.push(self.cell_doc(cell)?);
            }
            head = Some(cells);
        }
        let mut body = Vec::new();
        for row in &g.body {
            let mut cells = Vec::new();
            for cell in row {
                cells.push(self.cell_doc(cell)?);
            }
            body.push(cells);
        }
        Ok((head, body))
    }

    /// Renders the cell documents as lines at the column widths. A header
    /// row with only empty cells counts as no header.
    fn rows_lines(docs: &CellDocs, widths: Option<&[usize]>) -> (Option<RowLines>, Vec<RowLines>) {
        let width_of = |index: usize| widths.and_then(|w| w.get(index).copied());
        let row_lines = |row: &[Doc]| -> RowLines {
            row.iter()
                .enumerate()
                .map(|(index, doc)| doc_lines(doc, width_of(index)))
                .collect()
        };
        let head = docs
            .0
            .as_deref()
            .map(row_lines)
            .filter(|cells| cells.iter().any(|c| !c.is_empty()));
        let body = docs.1.iter().map(|row| row_lines(row)).collect();
        (head, body)
    }

    fn natural_widths(
        head: &Option<Vec<Vec<Text>>>,
        body: &[Vec<Vec<Text>>],
        columns: usize,
    ) -> Vec<usize> {
        let mut widths = vec![0usize; columns];
        for row in head.iter().chain(body.iter()) {
            for (index, cell) in row.iter().enumerate() {
                let width = cell.iter().map(|l| real_length(l)).max().unwrap_or(0);
                if let Some(slot) = widths.get_mut(index) {
                    *slot = (*slot).max(width);
                }
            }
        }
        widths
    }

    /// Lays out the lines of one row with aligned columns separated by a space.
    fn aligned_row(g: &Grid<'_>, widths: &[usize], cells: &[Vec<Text>]) -> Vec<Text> {
        let height = cells.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let mut out = Vec::new();
        for line in 0..height {
            let mut parts = Vec::new();
            for (index, cell) in cells.iter().enumerate() {
                let align = g
                    .aligns
                    .get(index)
                    .copied()
                    .unwrap_or(Alignment::AlignDefault);
                let width = widths.get(index).copied().unwrap_or(0);
                let text = cell.get(line).map_or("", String::as_str);
                parts.push(pad_text(align, width, text));
            }
            if let Some(last) = parts.last_mut() {
                let trimmed = last.trim_end().to_owned();
                *last = trimmed;
            }
            out.push(parts.join(" "));
        }
        out
    }

    fn simple_lines(&mut self, g: &Grid<'_>) -> Result<Vec<Text>> {
        let (head, body) = Self::rows_lines(&self.cell_docs(g)?, None);
        let columns = g.aligns.len();
        let widths: Vec<usize> = Self::natural_widths(&head, &body, columns)
            .iter()
            .map(|w| w + 2)
            .collect();
        let dashes: Vec<Text> = widths.iter().map(|w| "-".repeat(*w)).collect();
        let dashes = dashes.join(" ");
        let mut lines = Vec::new();
        match &head {
            Some(cells) => {
                lines.extend(Self::aligned_row(g, &widths, cells));
                lines.push(dashes.clone());
            }
            None => lines.push(dashes.clone()),
        }
        for row in &body {
            lines.extend(Self::aligned_row(g, &widths, row));
        }
        if head.is_none() {
            lines.push(dashes);
        }
        Ok(lines)
    }

    /// The column widths from the relative widths of the table: a part of
    /// `room` characters less `border`, and at least the longest word of the
    /// column plus `pad`.
    fn relative_widths(
        g: &Grid<'_>,
        docs: &CellDocs,
        room: i64,
        border: usize,
        pad: usize,
    ) -> Option<Vec<usize>> {
        if g.widths.iter().all(|w| *w == 0.0) {
            return None;
        }
        let columns = g.aligns.len();
        let (head, body) = Self::rows_lines(docs, Some(&vec![1; columns]));
        let words = Self::natural_widths(&head, &body, columns);
        Some(
            g.widths
                .iter()
                .zip(words)
                .map(|(w, word)| {
                    let chars = (room.max(0) as f64 * w).floor();
                    let chars = if chars.is_finite() && chars > 0.0 {
                        (chars as usize).min(crate::MAX_RUN)
                    } else {
                        0
                    };
                    chars.saturating_sub(border).max(word + pad).max(1)
                })
                .collect(),
        )
    }

    fn multiline_lines(&mut self, g: &Grid<'_>) -> Result<Vec<Text>> {
        let docs = self.cell_docs(g)?;
        let relative = Self::relative_widths(g, &docs, self.opts.columns - 1, 0, 2);
        let (head, body) = Self::rows_lines(&docs, relative.as_deref());
        let columns = g.aligns.len();
        let widths: Vec<usize> = match &relative {
            Some(widths) => widths.clone(),
            None => Self::natural_widths(&head, &body, columns)
                .iter()
                .map(|w| w + 2)
                .collect(),
        };
        let total = widths.iter().sum::<usize>() + widths.len().saturating_sub(1);
        let dashes = widths
            .iter()
            .map(|w| "-".repeat(*w))
            .collect::<Vec<_>>()
            .join(" ");
        // A headless table has no header line, so its borders show the columns.
        let border = if head.is_some() {
            "-".repeat(total)
        } else {
            dashes.clone()
        };
        let mut lines = vec![border.clone()];
        if let Some(cells) = &head {
            lines.extend(Self::aligned_row(g, &widths, cells));
            lines.push(dashes);
        }
        for (index, row) in body.iter().enumerate() {
            if index > 0 {
                lines.push(Text::new());
            }
            lines.extend(Self::aligned_row(g, &widths, row));
        }
        if body.len() == 1 {
            lines.push(Text::new());
        }
        lines.push(border);
        Ok(lines)
    }

    fn pipe_lines(&mut self, g: &Grid<'_>) -> Result<Vec<Text>> {
        let (head, body) = Self::rows_lines(&self.cell_docs(g)?, None);
        let columns = g.aligns.len();
        let natural: Vec<usize> = Self::natural_widths(&head, &body, columns)
            .iter()
            .map(|w| (*w).max(3))
            .collect();
        // A table that is wider than the line gets no padding. Its separator
        // then shows the relative column widths, which only Pandoc Markdown keeps.
        let padded = natural.iter().sum::<usize>() <= crate::run_length(self.opts.columns);
        let widths: Vec<usize> = if padded {
            natural
        } else if self.variant != crate::Variant::Commonmark && g.widths.iter().any(|w| *w > 0.0) {
            let room = self.opts.columns - i64::try_from(columns).unwrap_or(i64::MAX) - 1;
            g.widths
                .iter()
                .map(|w| {
                    let chars = (room as f64 * w).floor();
                    if chars.is_finite() && chars > 0.0 {
                        (chars as usize).min(crate::MAX_RUN)
                    } else {
                        0
                    }
                })
                .collect()
        } else {
            vec![2; columns]
        };
        let align_of = |index: usize| {
            g.aligns
                .get(index)
                .copied()
                .unwrap_or(Alignment::AlignDefault)
        };
        let make_row = |cells: &[Vec<Text>]| -> Text {
            let mut line = Text::from("|");
            for index in 0..columns {
                let text = cells
                    .get(index)
                    .and_then(|c| c.first())
                    .map_or("", String::as_str);
                let text = if padded {
                    let width = widths.get(index).copied().unwrap_or(3);
                    pad_text(align_of(index), width, text)
                } else {
                    text.to_owned()
                };
                line.push_str(&format!(" {text} |"));
            }
            line
        };
        let mut sep = Text::from("|");
        for (index, width) in widths.iter().enumerate() {
            let cell = match align_of(index) {
                Alignment::AlignLeft => format!(":{}", "-".repeat(width + 1)),
                Alignment::AlignRight => format!("{}:", "-".repeat(width + 1)),
                Alignment::AlignCenter => format!(":{}:", "-".repeat(*width)),
                Alignment::AlignDefault => "-".repeat(width + 2),
            };
            sep.push_str(&cell);
            sep.push('|');
        }
        let empty_head = vec![Vec::new(); columns];
        let mut lines = vec![make_row(head.as_deref().unwrap_or(&empty_head)), sep];
        for row in &body {
            lines.push(make_row(row));
        }
        Ok(lines)
    }

    fn grid_lines(&mut self, g: &Grid<'_>) -> Result<Vec<Text>> {
        let docs = self.cell_docs(g)?;
        let relative = Self::relative_widths(g, &docs, self.opts.columns, 3, 0);
        let (head, body) = Self::rows_lines(&docs, relative.as_deref());
        let columns = g.aligns.len();
        let widths: Vec<usize> = match &relative {
            Some(widths) => widths.clone(),
            None => Self::natural_widths(&head, &body, columns)
                .iter()
                .map(|w| (*w).max(1))
                .collect(),
        };
        let border = |fill: char, aligns: bool| -> Text {
            let mut out = Text::from("+");
            for (index, width) in widths.iter().enumerate() {
                let align = g
                    .aligns
                    .get(index)
                    .copied()
                    .unwrap_or(Alignment::AlignDefault);
                let (left, right) = match (aligns, align) {
                    (true, Alignment::AlignLeft) => (':', fill),
                    (true, Alignment::AlignRight) => (fill, ':'),
                    (true, Alignment::AlignCenter) => (':', ':'),
                    _ => (fill, fill),
                };
                out.push(left);
                out.extend(std::iter::repeat_n(fill, *width));
                out.push(right);
                out.push('+');
            }
            out
        };
        let make_row = |cells: &[Vec<Text>]| -> Vec<Text> {
            let height = cells.iter().map(Vec::len).max().unwrap_or(0).max(1);
            (0..height)
                .map(|line| {
                    let mut out = Text::from("|");
                    for index in 0..columns {
                        let width = widths.get(index).copied().unwrap_or(1);
                        let text = cells
                            .get(index)
                            .and_then(|c| c.get(line))
                            .map_or("", String::as_str);
                        out.push_str(&format!(
                            " {} |",
                            pad_text(Alignment::AlignLeft, width, text)
                        ));
                    }
                    out
                })
                .collect()
        };
        let mut lines = Vec::new();
        match &head {
            Some(cells) => {
                lines.push(border('-', false));
                lines.extend(make_row(cells));
                lines.push(border('=', true));
            }
            None => lines.push(border('-', true)),
        }
        for (index, row) in body.iter().enumerate() {
            let in_foot = g.foot_start.is_some_and(|start| index >= start);
            if g.foot_start == Some(index)
                && let Some(last) = lines.last_mut()
            {
                *last = border('=', false);
            }
            lines.extend(make_row(row));
            lines.push(border(if in_foot { '=' } else { '-' }, false));
        }
        Ok(lines)
    }
}

/// Pads one line of text to a width with the alignment.
fn pad_text(align: Alignment, width: usize, text: &str) -> Text {
    let gap = width.saturating_sub(real_length(text));
    match align {
        Alignment::AlignRight => format!("{}{text}", " ".repeat(gap)),
        Alignment::AlignCenter => {
            let left = gap / 2;
            format!("{}{text}{}", " ".repeat(left), " ".repeat(gap - left))
        }
        Alignment::AlignLeft | Alignment::AlignDefault => format!("{text}{}", " ".repeat(gap)),
    }
}
