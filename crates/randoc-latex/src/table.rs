//! Tables as `longtable` environments.

use randoc_ast::{Alignment, Block, Cell, ColWidth, Inline, Row, Table};
use randoc_core::{CaptionPosition, Result};
use randoc_layout::{Doc, cr, hsep, literal, vcat};

use crate::LatexWriter;
use crate::shared::blocks_any;

/// The nesting levels that one table counts.
const TABLE_LEVELS: usize = 3;

/// One column of the output: its alignment and its width, when the table
/// has widths.
struct Column {
    align: Alignment,
    width: Option<f64>,
    explicit: bool,
}

impl LatexWriter<'_> {
    pub(crate) fn table(&mut self, table: &Table) -> Result<Doc> {
        // One table level uses the stack of several block levels.
        self.enter_levels(TABLE_LEVELS)?;
        let columns = columns(table);
        let result = self.with_note_marks(|writer| writer.table_body(table, &columns));
        self.leave_levels(TABLE_LEVELS);
        let (table_doc, notes) = result?;
        Ok(table_doc.above(notes))
    }

    fn table_body(&mut self, table: &Table, columns: &[Column]) -> Result<Doc> {
        let spec = column_spec(columns);
        let has_caption = !table.caption.long.is_empty()
            || table.caption.short.is_some()
            || !table.attr.id.is_empty();
        let caption = if has_caption {
            Some(self.caption(&table.caption, &table.attr.id)?)
        } else {
            None
        };
        let head_rows = self.rows(&table.head.rows, columns, true)?;
        let has_head = table.head.rows.iter().any(|row| !row.cells.is_empty());
        let head = if has_head {
            literal("\\toprule\\noalign{}")
                .above(head_rows.clone())
                .above(literal("\\midrule\\noalign{}"))
        } else {
            literal("\\toprule\\noalign{}")
        };
        let caption_above = self.options.table_caption_position == CaptionPosition::CaptionAbove;
        let mut doc = literal(format!("\\begin{{longtable}}[]{spec}"));
        match &caption {
            Some(caption) if caption_above => {
                doc = doc
                    .above(caption.clone() + literal("\\tabularnewline"))
                    .above(head.clone())
                    .above(literal("\\endfirsthead"));
                if has_head {
                    // The repeated head gives no second note mark.
                    let was_repeating = std::mem::replace(&mut self.repeated_head, true);
                    let rows = self.rows(&table.head.rows, columns, true);
                    self.repeated_head = was_repeating;
                    doc = doc
                        .above(literal("\\toprule\\noalign{}"))
                        .above(rows?)
                        .above(literal("\\midrule\\noalign{}"));
                }
                doc = doc.above(literal("\\endhead"));
            }
            _ => {
                doc = doc.above(head).above(literal("\\endhead"));
            }
        }
        let foot_rows = self.rows(&table.foot.rows, columns, false)?;
        let mut body_rows = Vec::with_capacity(table.bodies.len() * 2);
        for body in &table.bodies {
            body_rows.push(self.rows(&body.head, columns, false)?);
            body_rows.push(self.rows(&body.body, columns, false)?);
        }
        let mut body_rows = Some(vcat(body_rows));
        // Beamer has no foot that repeats on each page.
        if self.beamer {
            doc = doc.above(body_rows.take().unwrap_or_default());
        }
        if !foot_rows.is_empty() {
            doc = doc.above(literal("\\midrule\\noalign{}")).above(foot_rows);
        }
        doc = doc.above(literal("\\bottomrule\\noalign{}"));
        if let Some(caption) = caption.as_ref().filter(|_| !caption_above) {
            doc = doc
                .above(literal("\\tabularnewline"))
                .above(caption.clone());
        }
        if let Some(body_rows) = body_rows {
            doc = doc.above(literal("\\endlastfoot")).above(body_rows);
        }
        doc = doc.above(literal("\\end{longtable}"));
        Ok(wrap_uncaptioned(doc, caption.is_none()))
    }

    fn rows(&mut self, rows: &[Row], columns: &[Column], header: bool) -> Result<Doc> {
        let mut docs = Vec::with_capacity(rows.len());
        for slots in layout(rows, columns.len()) {
            let mut parts = Vec::with_capacity(slots.len() * 2);
            for (index, slot) in slots.into_iter().enumerate() {
                if index > 0 {
                    parts.push(literal("&"));
                }
                if let Some(slot) = slot {
                    parts.push(self.cell(&slot, columns, header)?);
                }
            }
            docs.push(hsep(parts) + literal(" \\\\"));
        }
        Ok(vcat(docs))
    }

    /// Renders the blocks of a cell. A line break in a table with no widths
    /// gives a box of lines.
    fn cell_contents(&mut self, cell: &Cell, explicit_widths: bool) -> Result<Doc> {
        let was_boxing = std::mem::replace(&mut self.box_lines, !explicit_widths);
        let result = self.blocks(&cell.content);
        self.box_lines = was_boxing;
        result
    }

    fn cell(&mut self, slot: &Slot<'_>, columns: &[Column], header: bool) -> Result<Doc> {
        let Slot {
            cell,
            column,
            span,
            row_span,
        } = *slot;
        let spanned = &columns[column.min(columns.len().saturating_sub(1))..];
        let first = spanned.first();
        let align = match cell.align {
            Alignment::AlignDefault => first.map_or(Alignment::AlignDefault, |c| c.align),
            other => other,
        };
        let sized = first.is_some_and(|c| c.explicit && c.width.is_some_and(|w| w > 0.0));
        let simple = cell
            .content
            .iter()
            .all(|block| matches!(block, Block::Plain(_) | Block::Para(_)));
        let has_line_break = cell.content.iter().any(|block| {
            matches!(block, Block::Plain(_) | Block::Para(_))
                && blocks_have_line_break(std::slice::from_ref(block), MAX_SCAN_DEPTH)
        });
        let minipage = !header && sized && (!simple || has_line_break) && span == 1;
        let in_minipage_env = minipage || (header && sized && span == 1);
        // Display math needs a minipage in a cell.
        let was_in_cell = std::mem::replace(
            &mut self.in_cell,
            !(minipage || (header && sized && span == 1)),
        );
        let was_in_env = std::mem::replace(&mut self.in_minipage_env, in_minipage_env);
        let contents = self.cell_contents(cell, first.is_some_and(|c| c.explicit));
        self.in_cell = was_in_cell;
        self.in_minipage_env = was_in_env;
        let mut contents = contents?;
        if has_line_break && (minipage || (header && sized && span == 1)) {
            contents += literal("\\strut");
        }
        if minipage {
            contents = literal(format!(
                "\\begin{{minipage}}[t]{{\\linewidth}}{}",
                align_command(align)
            ))
            .above(contents)
            .above(literal("\\end{minipage}"));
        }
        if header && sized && span == 1 {
            contents = literal(format!(
                "\\begin{{minipage}}[b]{{\\linewidth}}{}",
                align_command(align)
            ))
            .above(contents)
            .above(literal("\\end{minipage}"));
        }
        if row_span > 1 {
            let width = if first.is_some_and(|c| c.width.is_some()) {
                "="
            } else {
                "*"
            };
            contents =
                literal(format!("\\multirow{{{row_span}}}{{{width}}}{{")) + contents + literal("}");
        }
        if span > 1 {
            let width: Option<f64> = spanned
                .iter()
                .take(span)
                .map(|c| c.width)
                .sum::<Option<f64>>();
            let left = if column == 0 { "@{}" } else { "" };
            let right = if column + span == columns.len() {
                "@{}"
            } else {
                ""
            };
            let body = match width {
                Some(width) => format!(
                    ">{{{}\\arraybackslash}}p{{(\\linewidth - {}\\tabcolsep) * \\real{{{width:.4}}} + {}\\tabcolsep}}",
                    align_command(align),
                    2 * columns.len().saturating_sub(1),
                    2 * (span - 1)
                ),
                None => align_letter(align).to_string(),
            };
            contents = literal(format!("\\multicolumn{{{span}}}{{{left}{body}{right}}}{{%"))
                + cr()
                + contents
                + literal("}");
        }
        Ok(contents)
    }
}

fn wrap_uncaptioned(doc: Doc, uncaptioned: bool) -> Doc {
    if uncaptioned {
        literal("{\\def\\LTcaptype{none} % do not increment counter")
            .above(doc)
            .above(literal("}"))
    } else {
        doc
    }
}

fn columns(table: &Table) -> Vec<Column> {
    let count = table.col_specs.len();
    let explicit = table
        .col_specs
        .iter()
        .any(|spec| matches!(spec.width, ColWidth::ColWidth(_)));
    let all_simple = all_cells_simple(table);
    table
        .col_specs
        .iter()
        .map(|spec| {
            let width = match spec.width {
                ColWidth::ColWidth(width) => Some(width),
                ColWidth::ColWidthDefault if explicit => Some(0.0),
                ColWidth::ColWidthDefault if !all_simple && count > 0 => Some(1.0 / count as f64),
                ColWidth::ColWidthDefault => None,
            };
            Column {
                align: spec.align,
                width,
                explicit,
            }
        })
        .collect()
}

/// The nesting depth that the search for a line break looks into.
const MAX_SCAN_DEPTH: usize = 64;

/// Returns `true` when blocks hold a line break, also in notes.
fn blocks_have_line_break(blocks: &[Block], depth: usize) -> bool {
    blocks_any(
        blocks,
        depth,
        &|inline| matches!(inline, Inline::LineBreak),
        &|_| false,
    )
}

fn all_cells_simple(table: &Table) -> bool {
    let simple = |cell: &Cell| match cell.content.as_slice() {
        [] => true,
        [block @ (Block::Plain(_) | Block::Para(_))] => {
            !blocks_have_line_break(std::slice::from_ref(block), MAX_SCAN_DEPTH)
        }
        _ => false,
    };
    let count = table.col_specs.len();
    let mut sections: Vec<&[Row]> = vec![&table.head.rows, &table.foot.rows];
    for body in &table.bodies {
        sections.push(&body.head);
        sections.push(&body.body);
    }
    sections.into_iter().all(|rows| {
        layout(rows, count)
            .iter()
            .flatten()
            .flatten()
            .all(|slot| simple(slot.cell))
    })
}

/// A cell at its place in the grid of the table.
#[derive(Clone, Copy)]
struct Slot<'t> {
    cell: &'t Cell,
    column: usize,
    span: usize,
    row_span: i64,
}

/// Places the cells of rows on a grid of `count` columns, as Pandoc
/// normalizes a table. Each row gives one entry for each cell start, and
/// `None` for a place that a row span covers or that has no cell. Cells
/// after the last column are dropped.
fn layout(rows: &[Row], count: usize) -> Vec<Vec<Option<Slot<'_>>>> {
    let mut covered: Vec<i64> = vec![0; count];
    let mut out = Vec::with_capacity(rows.len());
    for (row_index, row) in rows.iter().enumerate() {
        let remaining = i64::try_from(rows.len() - row_index).unwrap_or(i64::MAX);
        let mut slots = Vec::new();
        let mut column = 0usize;
        let mut source = row.cells.iter();
        while column < count {
            if let Some(slot) = covered.get_mut(column)
                && *slot > 0
            {
                *slot -= 1;
                slots.push(None);
                column += 1;
                continue;
            }
            let Some(cell) = source.next() else {
                slots.push(None);
                column += 1;
                continue;
            };
            let row_span = cell.row_span.min(remaining);
            let span = usize::try_from(cell.col_span.max(1))
                .unwrap_or(1)
                .min(count - column)
                .max(1);
            if row_span > 1 {
                for slot in covered.iter_mut().skip(column).take(span) {
                    *slot = row_span - 1;
                }
            }
            slots.push(Some(Slot {
                cell,
                column,
                span,
                row_span,
            }));
            column += span;
        }
        out.push(slots);
    }
    out
}

fn column_spec(columns: &[Column]) -> String {
    if columns.is_empty() {
        return "{@{}l@{}}".to_owned();
    }
    if columns.iter().all(|c| c.width.is_none()) {
        let letters: String = columns.iter().map(|c| align_letter(c.align)).collect();
        return format!("{{@{{}}{letters}@{{}}}}");
    }
    let separators = 2 * columns.len().saturating_sub(1);
    let lines: Vec<String> = columns
        .iter()
        .map(|c| {
            format!(
                "  >{{{}\\arraybackslash}}p{{(\\linewidth - {separators}\\tabcolsep) * \\real{{{:.4}}}}}",
                align_command(c.align),
                c.width.unwrap_or(0.0)
            )
        })
        .collect();
    format!("{{@{{}}\n{}@{{}}}}", lines.join("\n"))
}

fn align_letter(align: Alignment) -> char {
    match align {
        Alignment::AlignLeft | Alignment::AlignDefault => 'l',
        Alignment::AlignRight => 'r',
        Alignment::AlignCenter => 'c',
    }
}

fn align_command(align: Alignment) -> &'static str {
    match align {
        Alignment::AlignLeft | Alignment::AlignDefault => "\\raggedright",
        Alignment::AlignRight => "\\raggedleft",
        Alignment::AlignCenter => "\\centering",
    }
}
