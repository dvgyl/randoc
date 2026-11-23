use randoc_ast::{Alignment, Block, Row, Table};
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

    fn row(&mut self, row: &'a Row, widths: &mut [usize], depth: usize) -> Result<Vec<String>> {
        let mut rendered = Vec::with_capacity(row.cells.len());
        for (cell, width) in row.cells.iter().zip(widths) {
            let mut parts = Vec::new();
            for block in &cell.content {
                let doc = match block {
                    Block::Plain(nodes) | Block::Para(nodes) => self.inlines(nodes, depth)?,
                    _ => self.block(block, depth, '-')?,
                };
                parts.push(doc);
            }
            let doc = layout::hsep(parts);
            *width = (*width).max(doc.offset());
            let text = doc.render_limited(self.budget).ok_or_else(size_error)?;
            rendered.push(text.replace('\n', " "));
        }
        Ok(rendered)
    }
}
