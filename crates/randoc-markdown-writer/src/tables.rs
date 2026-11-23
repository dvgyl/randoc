//! Tables: simple, pipe, multiline and grid tables, and the HTML form.

use randoc_ast::{Alignment, Attr, Block, Cell, ColWidth, Inline, Row, Table, Text};
use randoc_core::{Extension, Result};
use randoc_layout::{Doc, blankline, cr, literal, nest, real_length};

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
        .any(|cell| cell.row_span != 1 || cell.col_span != 1);
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
    let head = if head_rows.is_empty()
        || head_rows.len() == 1 && head_rows[0].iter().all(|cell| cell.content.is_empty())
    {
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

/// A cell at its first row and column of the table grid.
struct Placed<'t> {
    cell: &'t Cell,
    row: usize,
    col: usize,
    cols: usize,
}

/// A table on its grid of rows and columns, for the forms that show spans.
struct Layout<'t> {
    cells: Vec<Placed<'t>>,
    /// The index in `cells` of the cell that covers each row and column.
    owner: Vec<Vec<Option<usize>>>,
    head_rows: usize,
    /// The first row of the table foot.
    foot_start: Option<usize>,
}

/// Places each cell at the first free column of its row. A span stops at
/// the end of its table section and at a column that a cell above covers.
fn layout(table: &Table, columns: usize) -> Layout<'_> {
    let mut sections: Vec<Vec<&Row>> = vec![table.head.rows.iter().collect()];
    for body in &table.bodies {
        sections.push(body.head.iter().chain(body.body.iter()).collect());
    }
    sections.push(table.foot.rows.iter().collect());
    let foot_rows = table.foot.rows.len();
    let mut out = Layout {
        cells: Vec::new(),
        owner: Vec::new(),
        head_rows: table.head.rows.len(),
        foot_start: None,
    };
    for rows in &sections {
        let start = out.owner.len();
        out.owner.extend(rows.iter().map(|_| vec![None; columns]));
        for (offset, row) in rows.iter().enumerate() {
            let r = start + offset;
            let mut c = 0;
            for cell in &row.cells {
                while out.owner[r].get(c).is_some_and(Option::is_some) {
                    c += 1;
                }
                if c >= columns {
                    break;
                }
                let wanted = usize::try_from(cell.col_span).unwrap_or(1).max(1);
                let cols = out.owner[r][c..]
                    .iter()
                    .take(wanted)
                    .take_while(|o| o.is_none())
                    .count();
                let span_rows = usize::try_from(cell.row_span).unwrap_or(1).max(1);
                let span_rows = span_rows.min(rows.len() - offset);
                let index = out.cells.len();
                for line in &mut out.owner[r..r + span_rows] {
                    for slot in &mut line[c..c + cols] {
                        slot.get_or_insert(index);
                    }
                }
                out.cells.push(Placed {
                    cell,
                    row: r,
                    col: c,
                    cols,
                });
                c += cols;
            }
        }
    }
    if foot_rows > 0 {
        out.foot_start = Some(out.owner.len() - foot_rows);
    }
    out
}

impl<'t> Layout<'t> {
    /// The cell that starts at a position, else `blank`: the first row is
    /// the header and the other rows are the body.
    fn positions<'a>(&self, g: &Grid<'a>, blank: &'a Cell) -> Grid<'a>
    where
        't: 'a,
    {
        let rows: Vec<Vec<&'a Cell>> = self
            .owner
            .iter()
            .enumerate()
            .map(|(r, line)| {
                line.iter()
                    .enumerate()
                    .map(|(c, slot)| match slot.map(|i| &self.cells[i]) {
                        Some(p) if p.row == r && p.col == c => p.cell,
                        _ => blank,
                    })
                    .collect()
            })
            .collect();
        let mut rows = rows.into_iter();
        let head = if self.head_rows > 0 {
            rows.next()
        } else {
            None
        };
        Grid {
            aligns: g.aligns.clone(),
            widths: g.widths.clone(),
            head,
            body: rows.collect(),
            foot_start: self.foot_start.map(|f| f - usize::from(self.head_rows > 0)),
            has_spans: false,
        }
    }

    /// Returns `true` when two positions belong to one cell.
    fn same(&self, row: usize, a: usize, b: usize) -> bool {
        let line = &self.owner[row];
        line[a].is_some() && line[a] == line[b]
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

/// Fits the natural widths within the available room. Give each long column
/// the same share and redistribute the room that short columns do not use.
fn fit_grid_widths(natural: &[usize], columns: usize) -> Vec<usize> {
    let mut room = columns.saturating_sub(3 * natural.len() + 1);
    let mut remaining = natural.len();
    let mut sorted = natural.to_vec();
    sorted.sort_unstable();
    for width in sorted {
        let share = room / remaining;
        if width > share {
            break;
        }
        room -= width;
        remaining -= 1;
    }
    let share = room.checked_div(remaining).unwrap_or(usize::MAX);
    natural.iter().map(|width| (*width).min(share)).collect()
}

fn cell_width(doc: &Doc, width: Option<usize>) -> usize {
    doc_lines(doc, width)
        .iter()
        .map(|line| real_length(line))
        .max()
        .unwrap_or(0)
        .max(1)
}

/// The plain-text table forms.
#[derive(Clone, Copy)]
enum Kind {
    Simple,
    Pipe,
    Multiline,
    Grid,
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
        let blank = Cell {
            attr: Attr::default(),
            align: Alignment::AlignDefault,
            row_span: 1,
            col_span: 1,
            content: Vec::new(),
        };
        let columns = table.col_specs.len();
        let placed = layout(table, columns);
        let many_heads = table.head.rows.len() > 1;
        let regular = |g: &Grid<'_>| {
            g.head
                .iter()
                .chain(g.body.iter())
                .all(|row| row.len() == columns)
        };
        let first = grid(table);
        // With no span, Pandoc fills short rows with empty cells, and the
        // header rows after the first one go to the body.
        let g = if !first.has_spans && (many_heads || !regular(&first)) {
            placed.positions(&first, &blank)
        } else {
            first
        };
        let rows_ok = regular(&g);
        let all_simple = g
            .head
            .iter()
            .chain(g.body.iter())
            .flatten()
            .all(|cell| is_simple_cell(cell));
        let no_widths = g.widths.iter().all(|w| *w == 0.0);
        // Markua has no table extensions and always writes pipe tables.
        let pipe = self.has(Extension::PipeTables) || self.variant == crate::Variant::Markua;
        let plain_ok = rows_ok && !g.has_spans && columns > 0;
        // Only grid and pipe tables can show a table foot.
        let no_foot = g.foot_start.is_none();
        let (kind, indented) = if !plain_ok {
            (None, false)
        } else if !no_foot && self.has(Extension::GridTables) {
            (Some(Kind::Grid), false)
        } else if all_simple && no_widths && no_foot && self.has(Extension::SimpleTables) {
            (Some(Kind::Simple), true)
        } else if all_simple && no_widths && pipe {
            (Some(Kind::Pipe), false)
        } else if no_foot
            && !g.body.is_empty()
            && self.has(Extension::MultilineTables)
            && g.head
                .iter()
                .chain(g.body.iter())
                .flatten()
                .all(|c| matches!(c.content.as_slice(), [] | [Block::Plain(_)]))
        {
            (Some(Kind::Multiline), true)
        } else if self.has(Extension::GridTables) {
            (Some(Kind::Grid), false)
        } else if all_simple && pipe {
            (Some(Kind::Pipe), false)
        } else {
            (None, false)
        };
        let Some(kind) = kind else {
            let spans_ok = !plain_ok && columns > 0;
            let lines = if spans_ok && self.has(Extension::GridTables) {
                self.span_grid_lines(&g, &placed)?
            } else if self.has(Extension::RawHtml) {
                // A blank line would end the HTML block in Markdown.
                let html = self.html_text(std::slice::from_ref(block))?;
                return Ok(literal(html.replace("\n\n", "\n&#10;")) + blankline());
            } else if spans_ok && all_simple && pipe {
                self.pipe_lines(&placed.positions(&g, &blank))?
            } else {
                return Ok(literal("[TABLE]") + self.caption_doc(table)?);
            };
            return Ok(literal(lines.join("\n")) + cr() + self.caption_doc(table)?);
        };
        let lines = match kind {
            Kind::Simple => self.simple_lines(&g)?,
            Kind::Pipe => self.pipe_lines(&g)?,
            Kind::Multiline => self.multiline_lines(&g)?,
            Kind::Grid if many_heads => self.span_grid_lines(&g, &placed)?,
            Kind::Grid => self.grid_lines(&g)?,
        };
        let body = literal(lines.join("\n")) + cr();
        let caption_doc = self.caption_doc(table)?;
        Ok(if indented {
            cr() + nest(2, body + caption_doc) + blankline()
        } else {
            body + caption_doc
        })
    }

    /// Builds the caption that follows a table, with the blank lines around
    /// it: the lines of the caption joined by line breaks, then the
    /// attributes of the table.
    fn caption_doc(&mut self, table: &Table) -> Result<Doc> {
        // Pandoc escapes no list marker at the start of a caption line.
        let lines = self.caption_lines(&table.caption.long, (false, false))?;
        if lines.empty {
            return Ok(blankline());
        }
        let mut caption = lines.doc;
        if !table.attr.is_empty() {
            caption += literal(format!(
                " {}",
                crate::inlines::attr_text(&self.prefixed(&table.attr))
            ));
        }
        Ok(if self.has(Extension::TableCaptions) {
            blankline() + literal(": ") + caption + blankline()
        } else {
            blankline() + caption + blankline()
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

    /// Renders the cell documents at the column widths.
    fn rows_lines(
        docs: &CellDocs,
        widths: Option<&[usize]>,
        keep_empty_head: bool,
    ) -> (Option<RowLines>, Vec<RowLines>) {
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
            .filter(|cells| keep_empty_head || cells.iter().any(|c| !c.is_empty()))
            .map(row_lines);
        let body = docs.1.iter().map(|row| row_lines(row)).collect();
        (head, body)
    }

    fn natural_widths<T>(
        head: &Option<Vec<T>>,
        body: &[Vec<T>],
        columns: usize,
        cell_width: impl Fn(&T) -> usize,
    ) -> Vec<usize> {
        let mut widths = vec![0usize; columns];
        for row in head.iter().chain(body.iter()) {
            for (index, cell) in row.iter().enumerate() {
                if let Some(slot) = widths.get_mut(index) {
                    *slot = (*slot).max(cell_width(cell));
                }
            }
        }
        widths
    }

    fn rendered_widths(head: &Option<RowLines>, body: &[RowLines], columns: usize) -> Vec<usize> {
        Self::natural_widths(head, body, columns, |cell| {
            cell.iter().map(|line| real_length(line)).max().unwrap_or(0)
        })
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
        let docs = self.cell_docs(g)?;
        let (head, body) = Self::rows_lines(&docs, None, true);
        let columns = g.aligns.len();
        // The document width includes spaces that the renderer trims.
        let widths: Vec<usize> = Self::natural_widths(&docs.0, &docs.1, columns, Doc::offset)
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
        let (head, body) = Self::rows_lines(docs, Some(&vec![1; columns]), false);
        let words = Self::rendered_widths(&head, &body, columns);
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
        let relative = Self::relative_widths(g, &docs, self.opts.columns.saturating_sub(1), 0, 2);
        let (head, body) = Self::rows_lines(&docs, relative.as_deref(), true);
        let columns = g.aligns.len();
        let widths: Vec<usize> = match &relative {
            Some(widths) => widths.clone(),
            None => Self::natural_widths(&docs.0, &docs.1, columns, Doc::offset)
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
        let (head, body) = Self::rows_lines(&self.cell_docs(g)?, None, false);
        let columns = g.aligns.len();
        let natural: Vec<usize> = Self::rendered_widths(&head, &body, columns)
            .iter()
            .map(|w| (*w).max(3))
            .collect();
        // A table that is wider than the line gets no padding. Its separator
        // then shows the relative column widths, which only Pandoc Markdown keeps.
        let padded = natural.iter().sum::<usize>() <= crate::run_length(self.opts.columns);
        let widths: Vec<usize> = if padded {
            natural
        } else if self.variant != crate::Variant::Commonmark && g.widths.iter().any(|w| *w > 0.0) {
            let room = self
                .opts
                .columns
                .saturating_sub(i64::try_from(columns).unwrap_or(i64::MAX))
                .saturating_sub(1);
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

    /// Lays out a grid table whose cells can span rows and columns.
    fn span_grid_lines(&mut self, g: &Grid<'_>, l: &Layout<'_>) -> Result<Vec<Text>> {
        let columns = g.aligns.len();
        let mut docs = Vec::with_capacity(l.cells.len());
        for placed in &l.cells {
            docs.push(self.cell_doc(placed.cell)?);
        }
        let relative = !g.widths.iter().all(|w| *w == 0.0);
        // The characters of a part of the line, less the border of a cell.
        let room = |part: f64| {
            let chars = (self.opts.columns.max(0) as f64 * part).floor();
            let chars = if chars.is_finite() && chars > 0.0 {
                (chars as usize).min(crate::MAX_RUN)
            } else {
                0
            };
            chars.saturating_sub(3)
        };
        let mut widths = vec![1usize; columns];
        if relative {
            for (slot, w) in widths.iter_mut().zip(&g.widths) {
                *slot = room(*w).max(1);
            }
        }
        for (placed, doc) in l.cells.iter().zip(&docs) {
            if placed.cols == 1 {
                let width = cell_width(doc, relative.then_some(1));
                widths[placed.col] = widths[placed.col].max(width);
            }
        }
        for (placed, doc) in l.cells.iter().zip(&docs) {
            if placed.cols > 1 {
                let span = placed.col..placed.col + placed.cols;
                let width = if relative {
                    room(g.widths[span.clone()].iter().sum())
                } else {
                    cell_width(doc, None)
                };
                // Pandoc gives each spanned column the quotient plus the remainder.
                let share = width / placed.cols + width % placed.cols;
                for slot in &mut widths[span] {
                    *slot = (*slot).max(share);
                }
            }
        }
        if !relative {
            widths = fit_grid_widths(&widths, crate::run_length(self.opts.columns));
            for (placed, doc) in l.cells.iter().zip(&docs) {
                let width = cell_width(doc, Some(1));
                let share = width / placed.cols + width % placed.cols;
                for slot in &mut widths[placed.col..placed.col + placed.cols] {
                    *slot = (*slot).max(share);
                }
            }
        }
        let span_width =
            |p: &Placed<'_>| widths[p.col..p.col + p.cols].iter().sum::<usize>() + 3 * (p.cols - 1);
        let texts: Vec<Vec<Text>> = l
            .cells
            .iter()
            .zip(&docs)
            .map(|(p, doc)| doc_lines(doc, Some(span_width(p))))
            .collect();
        let rows = l.owner.len();
        // The text of a cell that spans rows stays in its first row.
        let mut heights = vec![1usize; rows];
        for (p, text) in l.cells.iter().zip(&texts) {
            heights[p.row] = heights[p.row].max(text.len());
        }
        let mut tops = Vec::with_capacity(rows);
        let mut y = 0;
        for height in &heights {
            tops.push(y);
            y += height + 1;
        }
        // One line of the table at height `y`: the columns where `text` holds
        // show the text of a cell, the others a border of `fill`.
        let line = |y: usize,
                    above: Option<usize>,
                    below: Option<usize>,
                    fill: char,
                    aligns: bool|
         -> Text {
            let owner = |c: usize| {
                let row = above.or(below)?;
                let index = l.owner[row][c]?;
                let shown = match (above, below) {
                    (Some(a), Some(b)) => l.owner[a][c] == l.owner[b][c],
                    _ => above == below,
                };
                shown.then_some(index)
            };
            let vertical = |row: Option<usize>, c: usize| row.is_some_and(|r| !l.same(r, c - 1, c));
            let mut out = Text::new();
            let mut c = 0;
            while c < columns {
                let shown = owner(c);
                let junction = |filled: bool| {
                    if c == 0 {
                        if filled { '+' } else { '|' }
                    } else {
                        let before = owner(c - 1);
                        match (before, shown) {
                            (Some(_), Some(_)) => '|',
                            (None, None)
                                if !(aligns && (above.is_none() || fill == '='))
                                    && !vertical(above, c)
                                    && !vertical(below, c) =>
                            {
                                fill
                            }
                            _ => '+',
                        }
                    }
                };
                match shown {
                    Some(index) => {
                        let p = &l.cells[index];
                        out.push(junction(false));
                        let text = texts[index].get(y - tops[p.row]).map_or("", String::as_str);
                        out.push(' ');
                        out.push_str(&pad_text(Alignment::AlignLeft, span_width(p), text));
                        out.push(' ');
                        c = p.col + p.cols;
                    }
                    None => {
                        out.push(if fill == ' ' { '|' } else { junction(true) });
                        let align = g.aligns.get(c).copied().unwrap_or(Alignment::AlignDefault);
                        let (left, right) = match (aligns, align) {
                            (true, Alignment::AlignLeft) => (':', fill),
                            (true, Alignment::AlignRight) => (fill, ':'),
                            (true, Alignment::AlignCenter) => (':', ':'),
                            _ => (fill, fill),
                        };
                        out.push(left);
                        out.extend(std::iter::repeat_n(fill, widths[c]));
                        out.push(right);
                        c += 1;
                    }
                }
            }
            out.push(if fill == ' ' || owner(columns - 1).is_some() {
                '|'
            } else {
                '+'
            });
            out
        };
        let fill_after = |r: usize| {
            let foot_edge = l.foot_start.is_some_and(|f| r + 1 == f || r + 1 == rows);
            if r + 1 == l.head_rows || foot_edge {
                '='
            } else {
                '-'
            }
        };
        let mut lines = vec![line(
            0,
            None,
            (rows > 0).then_some(0),
            '-',
            l.head_rows == 0,
        )];
        for r in 0..rows {
            for k in 0..heights[r] {
                lines.push(line(tops[r] + k, Some(r), Some(r), ' ', false));
            }
            let below = (r + 1 < rows).then_some(r + 1);
            let aligns = r + 1 == l.head_rows;
            lines.push(line(
                tops[r] + heights[r],
                Some(r),
                below,
                fill_after(r),
                aligns,
            ));
        }
        Ok(lines)
    }

    fn grid_lines(&mut self, g: &Grid<'_>) -> Result<Vec<Text>> {
        let docs = self.cell_docs(g)?;
        let columns = g.aligns.len();
        let widths =
            Self::relative_widths(g, &docs, self.opts.columns, 3, 0).unwrap_or_else(|| {
                let natural =
                    Self::natural_widths(&docs.0, &docs.1, columns, |doc| cell_width(doc, None));
                let mut widths = fit_grid_widths(&natural, crate::run_length(self.opts.columns));
                let minimum =
                    Self::natural_widths(&docs.0, &docs.1, columns, |doc| cell_width(doc, Some(1)));
                for (width, minimum) in widths.iter_mut().zip(minimum) {
                    *width = (*width).max(minimum).max(1);
                }
                widths
            });
        let (head, body) = Self::rows_lines(&docs, Some(&widths), true);
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
            let foot_end = g.foot_start.is_some() && index + 1 == body.len();
            if g.foot_start == Some(index)
                && let Some(last) = lines.last_mut()
            {
                *last = border('=', false);
            }
            lines.extend(make_row(row));
            lines.push(border(if foot_end { '=' } else { '-' }, false));
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
