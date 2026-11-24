use randoc_ast::{Alignment, Block, Cell, Inline, Row, Table};
use randoc_core::Result;
use randoc_layout::{self as layout, Doc};

use crate::size_error;

use super::{Render, block_attr};

impl<'a> Render<'a> {
    pub(super) fn table(&mut self, table: &'a Table, depth: usize) -> Result<Doc> {
        self.depth(depth)?;
        let head = table.head.rows.len();
        let rows = table
            .head
            .rows
            .iter()
            .chain(
                table
                    .bodies
                    .iter()
                    .flat_map(|body| body.head.iter().chain(&body.body)),
            )
            .chain(&table.foot.rows)
            .collect::<Vec<_>>();
        let columns = table
            .col_specs
            .len()
            .max(rows.iter().map(|r| r.cells.len()).max().unwrap_or(0));
        self.table_cells = self
            .table_cells
            .checked_sub(rows.len().max(1).saturating_mul(columns))
            .ok_or_else(size_error)?;
        let rows = rectangular_rows(&rows, columns);
        if rows
            .iter()
            .flatten()
            .flatten()
            .any(|cell| complex_cell(cell))
        {
            return self.list_table(&rows, head, depth + 1);
        }
        let aligns = (0..columns)
            .map(|index| {
                table
                    .col_specs
                    .get(index)
                    .map_or(Alignment::AlignDefault, |spec| spec.align)
            })
            .collect::<Vec<_>>();
        let mut widths = vec![1; columns];
        let mut rendered = Vec::new();
        for row in rows {
            rendered.push(self.row(row, &mut widths, depth + 1)?);
        }
        let separator = head > 0 || aligns.iter().any(|align| *align != Alignment::AlignDefault);
        let min_row = widths.iter().fold(2usize, |total, &width| {
            total.saturating_add(width.saturating_add(3))
        });
        let rows = rendered.len().saturating_add(usize::from(separator));
        if min_row.saturating_mul(rows) > self.budget {
            return Err(size_error());
        }
        let pipe = |row: &[String]| {
            let mut out = String::from("|");
            for (index, (width, alignment)) in widths.iter().zip(&aligns).enumerate() {
                let value = row.get(index).map_or("", String::as_str);
                let pad = width.saturating_sub(layout::literal(value).offset());
                let left = match alignment {
                    Alignment::AlignRight => pad,
                    Alignment::AlignCenter => pad / 2,
                    _ => 0,
                };
                out.push_str(&" ".repeat(left + 1));
                out.push_str(value);
                out.push_str(&" ".repeat(pad - left + 1));
                out.push('|');
            }
            layout::literal(out)
        };
        let mut lines = Vec::new();
        for index in 0..=rendered.len() {
            if separator && index == head {
                let mut out = String::from("|");
                for (width, align) in widths.iter().zip(&aligns) {
                    let left = matches!(align, Alignment::AlignLeft | Alignment::AlignCenter);
                    let right = matches!(align, Alignment::AlignRight | Alignment::AlignCenter);
                    if left {
                        out.push(':');
                    }
                    out.push_str(&"-".repeat(if left || right {
                        width + 2 - usize::from(left) - usize::from(right)
                    } else {
                        *width
                    }));
                    if right {
                        out.push(':');
                    }
                    out.push('|');
                }
                lines.push(layout::literal(out));
            }
            if let Some(row) = rendered.get(index) {
                lines.push(pipe(row));
            }
        }
        let body = block_attr(&table.attr).above(layout::vcat(lines));
        if table.caption.long.is_empty() {
            return Ok(body);
        }
        Ok(body.above_blank(layout::hang(
            2,
            layout::literal("^ "),
            self.blocks(&table.caption.long, depth + 1)?,
        )))
    }

    fn list_table(
        &mut self,
        rows: &[Vec<Option<&'a Cell>>],
        head: usize,
        depth: usize,
    ) -> Result<Doc> {
        let mut rendered = Vec::new();
        if head == 0 {
            rendered.push(layout::literal("- "));
        }
        for (index, row) in rows.iter().enumerate() {
            let bullet = if index == 0 && head > 0 { '-' } else { '+' };
            let marker = if bullet == '-' { "- " } else { "+ " };
            let cells = row
                .iter()
                .map(|cell| {
                    let blocks = cell.map_or(&[][..], |cell| cell.content.as_slice());
                    let content = if let [first @ Block::BulletList(_), rest @ ..] = blocks {
                        self.block(first, depth, bullet)?
                            .above_blank(self.blocks(rest, depth)?)
                    } else {
                        self.blocks(blocks, depth)?
                    };
                    Ok(if content.is_empty() {
                        layout::literal(marker.trim_end())
                    } else {
                        layout::hang(2, layout::literal(marker), content)
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            rendered.push(layout::hang(2, layout::literal("- "), layout::vsep(cells)));
        }
        Ok(layout::literal("{.table}").above(layout::vsep(rendered)))
    }

    fn row(
        &mut self,
        row: Vec<Option<&'a Cell>>,
        widths: &mut [usize],
        depth: usize,
    ) -> Result<Vec<String>> {
        let mut rendered = Vec::with_capacity(row.len());
        for (cell, width) in row.into_iter().zip(widths) {
            let doc = match cell.map(|cell| cell.content.as_slice()) {
                Some([Block::Plain(nodes) | Block::Para(nodes)]) => self.inlines(nodes, depth)?,
                _ => Doc::default(),
            };
            *width = (*width).max(doc.offset());
            let text = doc.render_limited(self.budget).ok_or_else(size_error)?;
            rendered.push(text.replace('\n', " "));
        }
        Ok(rendered)
    }
}

fn complex_cell(cell: &Cell) -> bool {
    match cell.content.as_slice() {
        [] => false,
        [Block::Plain(nodes) | Block::Para(nodes)] => {
            nodes.iter().any(|node| matches!(node, Inline::LineBreak))
        }
        _ => true,
    }
}

fn rectangular_rows<'a>(rows: &[&'a Row], columns: usize) -> Vec<Vec<Option<&'a Cell>>> {
    let mut occupied_until = vec![0; columns];
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let mut cells = vec![None; columns];
            let mut column = 0;
            for cell in &row.cells {
                while column < columns && occupied_until[column] > index {
                    column += 1;
                }
                if column == columns {
                    break;
                }
                let width = usize::try_from(cell.col_span.max(1)).unwrap_or(usize::MAX);
                let next = column.saturating_add(width).min(columns);
                let height = usize::try_from(cell.row_span.max(1)).unwrap_or(usize::MAX);
                cells[column] = Some(cell);
                occupied_until[column..next].fill(index.saturating_add(height));
                column = next;
            }
            cells
        })
        .collect()
}
