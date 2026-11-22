use randoc_ast::{Row, Table};
use randoc_core::{Error, Result};
use randoc_layout::{Doc, chomp, cr, literal, real_length};

use super::Render;

const MAX_TABLE_OUTPUT_BYTES: u64 = 64 << 20;

impl Render<'_> {
    pub(super) fn table(&mut self, table: &Table, depth: usize) -> Result<Doc> {
        let cols = table.col_specs.len();
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
            .chain(&table.foot.rows);
        let count = rows.clone().count();
        self.charge_cells(cols.saturating_mul(count.saturating_add(1)))?;
        let mut widths = vec![0; cols];
        let mut spans = vec![0; cols];
        let mut rendered = Vec::new();
        for row in rows {
            let row = self.row(row, &mut spans, depth)?;
            for (lines, width) in row.iter().zip(&mut widths) {
                for line in lines {
                    *width = (*width).max(real_length(line));
                }
            }
            rendered.push(row);
        }
        let size_error = || Error::Parse("Org tables exceed the output size limit.".into());
        let line_width = widths
            .iter()
            .try_fold(2usize, |bytes, width| {
                bytes.checked_add(*width)?.checked_add(3)
            })
            .ok_or_else(size_error)?;
        let mut size = 0usize;
        let mut heights = Vec::with_capacity(rendered.len());
        for row in &rendered {
            let height = row.iter().map(Vec::len).max().unwrap_or(1).max(1);
            self.charge_cells(cols.saturating_mul(height - 1))?;
            heights.push(height);
            size = height
                .checked_mul(line_width)
                .and_then(|bytes| size.checked_add(bytes))
                .ok_or_else(size_error)?;
            for data in row.iter().flatten() {
                size = size
                    .checked_sub(real_length(data))
                    .and_then(|bytes| bytes.checked_add(data.len()))
                    .ok_or_else(size_error)?;
            }
        }
        if !table.head.rows.is_empty() {
            let rule_width = if cols > 0 { line_width } else { 3 };
            size = size.checked_add(rule_width).ok_or_else(size_error)?;
        }
        let limit = self
            .context
            .limits
            .max_input_size
            .min(MAX_TABLE_OUTPUT_BYTES);
        self.table_bytes = self
            .table_bytes
            .checked_add(u64::try_from(size).map_err(|_| size_error())?)
            .filter(|&bytes| bytes <= limit)
            .ok_or_else(size_error)?;
        let mut output = String::with_capacity(size);
        for (i, (row, height)) in rendered.iter().zip(heights).enumerate() {
            for line in 0..height {
                output.push('|');
                for (cell, width) in row.iter().zip(&widths) {
                    let data = cell.get(line).map_or("", String::as_str);
                    output.push(' ');
                    output.push_str(data);
                    output.extend(std::iter::repeat_n(' ', width - real_length(data) + 1));
                    output.push('|');
                }
                output.push('\n');
            }
            if i + 1 == table.head.rows.len() {
                output.push('|');
                for (column, width) in widths.iter().enumerate() {
                    if column > 0 {
                        output.push('+');
                    }
                    output.extend(std::iter::repeat_n('-', width + 2));
                }
                output.push_str("|\n");
            }
        }
        Ok(literal(output.trim_end_matches('\n')) + cr() + self.caption(&table.caption, depth)?)
    }

    fn charge_cells(&mut self, cells: usize) -> Result<()> {
        self.table_cells = self.table_cells.saturating_add(cells);
        if self.table_cells > self.context.limits.max_table_cells {
            return Err(Error::Parse("Org tables exceed the cell limit.".into()));
        }
        Ok(())
    }

    fn row(&mut self, row: &Row, spans: &mut [usize], depth: usize) -> Result<Vec<Vec<String>>> {
        let mut columns = vec![Vec::new(); spans.len()];
        let mut cells = row.cells.iter();
        let mut column = 0;
        while column < spans.len() {
            if spans[column] != 0 {
                column += 1;
                continue;
            }
            let Some(cell) = cells.next() else { break };
            let height = usize::try_from(cell.row_span.max(1)).unwrap_or(usize::MAX);
            let width = usize::try_from(cell.col_span.max(1)).unwrap_or(usize::MAX);
            let end = column.saturating_add(width).min(spans.len());
            let content = chomp(self.blocks(&cell.content, depth + 1)?).render(None);
            columns[column] = content.split('\n').map(str::to_owned).collect();
            for span in &mut spans[column..end] {
                *span = (*span).max(height);
            }
            column = end;
        }
        for span in spans {
            *span = span.saturating_sub(1);
        }
        Ok(columns)
    }
}
