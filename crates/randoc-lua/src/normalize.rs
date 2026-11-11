//! The method `normalize` of `Pandoc`.
//!
//! The rule is a port of `normalize` in `Text.Pandoc.Lua.Marshal.Pandoc`
//! of `pandoc-lua-marshal` 0.3.2.1 (MIT): each list of inlines is joined
//! again with the `<>` of `Inlines`, then each table is normalized with
//! `normalizeTableHead`, `normalizeTableBody` and `normalizeTableFoot` of
//! `Text.Pandoc.Builder` in `pandoc-types` 1.23.1 (BSD-3-Clause).

use std::iter::Peekable;

use randoc_ast::walk::{VisitorMut, walk_mut};
use randoc_ast::{Alignment, Attr, Block, Cell, Inline, Pandoc, Row, Table};

use crate::utils::append;

/// Normalizes a document as `Pandoc:normalize()` does.
pub(crate) fn normalize(doc: &mut Pandoc) {
    walk_mut(doc, &mut JoinInlines);
    walk_mut(doc, &mut NormalizeTables);
}

struct JoinInlines;

impl VisitorMut for JoinInlines {
    fn inlines(&mut self, inlines: &mut Vec<Inline>) {
        let items = std::mem::take(inlines);
        for item in items {
            append(inlines, vec![item]);
        }
    }
}

struct NormalizeTables;

impl VisitorMut for NormalizeTables {
    fn block(&mut self, block: &mut Block) {
        if let Block::Table(table) = block {
            normalize_table(table);
        }
    }
}

fn normalize_table(table: &mut Table) {
    let width = table.col_specs.len();
    header_section(width, &mut table.head.rows);
    for body in &mut table.bodies {
        let row_head = usize::try_from(body.row_head_columns)
            .unwrap_or(0)
            .min(width);
        body.row_head_columns = row_head as i64;
        header_section(width, &mut body.head);
        body_section(width, row_head, &mut body.body);
    }
    header_section(width, &mut table.foot.rows);
}

fn empty_cell() -> Cell {
    Cell {
        attr: Attr::default(),
        align: Alignment::AlignDefault,
        row_span: 1,
        col_span: 1,
        content: Vec::new(),
    }
}

/// Clamps the row span of each cell to the rows from its row to the end of
/// the section (`clipRows`).
fn clip_rows(rows: &mut [Row]) {
    let count = rows.len() as i64;
    for (index, row) in rows.iter_mut().enumerate() {
        let high = count - index as i64;
        for cell in &mut row.cells {
            cell.row_span = cell.row_span.max(1).min(high);
        }
    }
}

fn header_section(width: usize, rows: &mut [Row]) {
    clip_rows(rows);
    let mut hang = vec![1; width];
    for row in rows {
        let mut stream = std::mem::take(&mut row.cells).into_iter().peekable();
        let (new_hang, cells) = place_row_section(&hang, &mut stream);
        hang = new_hang;
        row.cells = cells;
    }
}

fn body_section(width: usize, row_head: usize, rows: &mut [Row]) {
    clip_rows(rows);
    let mut head_hang = vec![1; row_head];
    let mut body_hang = vec![1; width.saturating_sub(row_head)];
    for row in rows {
        let mut stream = std::mem::take(&mut row.cells).into_iter().peekable();
        let (new_head, mut cells) = place_row_section(&head_hang, &mut stream);
        let (new_body, body) = place_row_section(&body_hang, &mut stream);
        head_hang = new_head;
        body_hang = new_body;
        cells.extend(body);
        row.cells = cells;
    }
}

/// Places cells on one grid row (`placeRowSection`). `hang` is the number
/// of rows that each grid column of the previous row still covers. The
/// cells of `stream` come first; empty cells follow when it ends. Returns
/// the overhang of this row and the placed cells; the cells that do not
/// fit stay in `stream`.
fn place_row_section<I: Iterator<Item = Cell>>(
    hang: &[i64],
    stream: &mut Peekable<I>,
) -> (Vec<i64>, Vec<Cell>) {
    let mut new_hang = Vec::new();
    let mut placed = Vec::new();
    let mut at = 0;
    while let Some(&over) = hang.get(at) {
        if over > 1 {
            new_hang.push(over - 1);
            at += 1;
            continue;
        }
        let wanted = stream.peek().map_or(1, |cell| cell.col_span.max(1));
        let free = hang
            .get(at..)
            .unwrap_or_default()
            .iter()
            .take_while(|o| **o == 1)
            .count()
            .min(usize::try_from(wanted).unwrap_or(usize::MAX));
        if free == 0 {
            break;
        }
        let mut cell = stream.next().unwrap_or_else(empty_cell);
        cell.col_span = free as i64;
        new_hang.extend(std::iter::repeat_n(cell.row_span, free));
        placed.push(cell);
        at += free;
    }
    (new_hang, placed)
}
