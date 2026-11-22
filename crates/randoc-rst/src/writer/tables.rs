use randoc_ast::{Block, ColWidth, Row, Table};
use randoc_core::{Error, Result};
use randoc_layout::{Doc, hang, literal, nest, real_length, vcat};

use super::Writer;

struct RenderedCell {
    doc: Doc,
    start: usize,
    end: usize,
    row: usize,
    bottom: usize,
}

struct Layout {
    cells: Vec<RenderedCell>,
    slots: Vec<Vec<Option<usize>>>,
    widths: Vec<usize>,
    head: usize,
    foot: usize,
}

impl Writer<'_> {
    pub(super) fn table(&mut self, table: &Table, depth: usize) -> Result<Doc> {
        let caption = self.blocks(&table.caption.long, depth)?;
        let rows = table
            .head
            .rows
            .iter()
            .chain(
                table
                    .bodies
                    .iter()
                    .flat_map(|b| b.head.iter().chain(&b.body)),
            )
            .chain(&table.foot.rows);
        let layout = self.layout(table, rows, depth)?;
        let grid = layout.widths.len() < 2
            || table
                .col_specs
                .iter()
                .any(|c| matches!(c.width, ColWidth::ColWidth(_)))
            || layout.cells.iter().any(|c| c.bottom > c.row + 1)
            || layout
                .widths
                .iter()
                .sum::<usize>()
                .saturating_add(layout.widths.len().saturating_sub(1))
                > self.width().unwrap_or(usize::MAX);
        if self.options.list_tables {
            let mut header = literal(".. list-table::");
            if !caption.is_empty() {
                header = header + literal(" ") + caption;
            }
            if table
                .col_specs
                .iter()
                .any(|c| matches!(c.width, ColWidth::ColWidth(_)))
            {
                let widths = table
                    .col_specs
                    .iter()
                    .map(|col| {
                        if let ColWidth::ColWidth(w) = col.width {
                            (w * 100.0).round() as usize
                        } else {
                            0
                        }
                    })
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                header = header.above(literal(format!("   :widths: {widths}")));
            }
            if layout.head > 0 {
                header = header.above(literal(format!("   :header-rows: {}", layout.head)));
            }
            return Ok(header.above_blank(nest(3, layout.list())));
        }
        let body = if grid { layout.grid() } else { layout.simple() };
        if caption.is_empty() {
            Ok(body)
        } else {
            Ok((literal(".. table:: ") + caption).above_blank(nest(3, body)))
        }
    }

    fn layout<'b>(
        &mut self,
        table: &Table,
        rows: impl Iterator<Item = &'b Row> + Clone,
        depth: usize,
    ) -> Result<Layout> {
        let count = rows.clone().count();
        let cols = table.col_specs.len();
        if cols.saturating_mul(count.max(1)) > self.context.limits.max_table_cells {
            return Err(Error::Parse(
                "The reStructuredText document exceeds the table cell limit.".into(),
            ));
        }
        let mut slots = vec![vec![None; cols]; count];
        let mut widths = vec![1; cols];
        let mut cells = Vec::new();
        let mut has_blocks = false;
        for (r, row) in rows.enumerate() {
            let mut col = 0;
            for cell in &row.cells {
                while col < cols && slots[r][col].is_some() {
                    col += 1;
                }
                if col >= cols {
                    break;
                }
                let end = col
                    .saturating_add(usize::try_from(cell.col_span).unwrap_or(1).max(1))
                    .min(cols);
                let bottom = r
                    .saturating_add(usize::try_from(cell.row_span).unwrap_or(1).max(1))
                    .min(count);
                let doc = self.blocks(&cell.content, depth)?;
                has_blocks |= cell
                    .content
                    .iter()
                    .any(|b| !matches!(b, Block::Para(_) | Block::Plain(_)));
                if end == col + 1 {
                    widths[col] = widths[col].max(doc.offset());
                }
                for slot in &mut slots[r..bottom] {
                    slot[col..end].fill(Some(cells.len()));
                }
                cells.push(RenderedCell {
                    doc,
                    start: col,
                    end,
                    row: r,
                    bottom,
                });
                col = end;
            }
        }
        let mut min_widths = vec![1; cols];
        for cell in &cells {
            let span = cell.end - cell.start;
            let min_width = cell.doc.min_offset();
            let needed = min_width
                .saturating_sub(span.saturating_sub(1) * 3)
                .div_ceil(span);
            for w in &mut min_widths[cell.start..cell.end] {
                *w = (*w).max(needed);
            }
            let available =
                widths[cell.start..cell.end].iter().sum::<usize>() + span.saturating_sub(1);
            if available < cell.doc.offset() {
                widths[cell.end - 1] += cell.doc.offset() - available;
            }
        }
        let columns = self
            .width()
            .unwrap_or_else(|| usize::try_from(self.options.columns).unwrap_or(72))
            .clamp(1, 100_000);
        let constrained = widths
            .iter()
            .sum::<usize>()
            .saturating_add(cols.saturating_sub(1))
            > columns;
        let explicit = table
            .col_specs
            .iter()
            .any(|c| matches!(c.width, ColWidth::ColWidth(_)));
        if explicit || constrained || has_blocks {
            for (i, (spec, w)) in table.col_specs.iter().zip(&mut widths).enumerate() {
                let preferred = match spec.width {
                    ColWidth::ColWidth(fraction) => ((fraction * columns as f64).floor() as usize)
                        .min(columns)
                        .saturating_sub(3),
                    ColWidth::ColWidthDefault if constrained => (*w).min(
                        columns
                            .saturating_sub(1)
                            .checked_div(cols)
                            .unwrap_or(1)
                            .saturating_sub(3),
                    ),
                    ColWidth::ColWidthDefault => *w,
                };
                *w = preferred.max(min_widths[i]);
            }
        }
        Ok(Layout {
            cells,
            slots,
            widths,
            head: table.head.rows.len(),
            foot: table.foot.rows.len(),
        })
    }
}

impl Layout {
    fn list(&self) -> Doc {
        let mut rows = Vec::new();
        for r in 0..self.slots.len() {
            let mut cells = Vec::new();
            for c in 0..self.widths.len() {
                let doc = self.slots[r][c]
                    .and_then(|i| self.cells.get(i))
                    .filter(|cell| cell.row == r && cell.start == c)
                    .map(|cell| cell.doc.clone())
                    .unwrap_or_default();
                cells.push(hang(2, literal("- "), doc));
            }
            rows.push(hang(2, literal("* "), vcat(cells)));
        }
        vcat(rows)
    }

    fn simple(&self) -> Doc {
        let rule = self
            .widths
            .iter()
            .map(|w| "=".repeat(*w))
            .collect::<Vec<_>>()
            .join(" ");
        let mut lines = vec![rule.clone()];
        for (r, row) in self.slots.iter().enumerate() {
            let mut parts = Vec::new();
            let mut spans = Vec::new();
            let mut c = 0;
            while c < row.len() {
                let cell = row[c].and_then(|i| self.cells.get(i));
                let end = cell.map_or(c + 1, |cell| cell.end);
                let width = self.widths[c..end].iter().sum::<usize>() + end - c - 1;
                let doc = cell.map(|cell| cell.doc.render(None)).unwrap_or_default();
                spans.push((width, end > c + 1));
                parts.push((
                    width,
                    doc.split('\n').map(str::to_owned).collect::<Vec<_>>(),
                ));
                c = end;
            }
            let height = parts
                .iter()
                .map(|(_, lines)| lines.len())
                .max()
                .unwrap_or(1);
            for i in 0..height {
                let mut line = String::new();
                for (j, (width, part)) in parts.iter().enumerate() {
                    let text = part.get(i).map_or("", String::as_str);
                    line.push_str(text);
                    if j + 1 < parts.len() {
                        line.push_str(&" ".repeat(width.saturating_sub(real_length(text)) + 1));
                    }
                }
                lines.push(line);
            }
            if spans.iter().any(|(_, span)| *span) {
                lines.push(
                    spans
                        .into_iter()
                        .map(|(w, _)| "-".repeat(w))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
            if r + 1 == self.head {
                lines.push(rule.clone());
            }
        }
        if self.head != self.slots.len() || self.slots.is_empty() {
            lines.push(rule);
        }
        literal(lines.join("\n"))
    }

    fn grid(&self) -> Doc {
        let cols = self.widths.len();
        let count = self.slots.len();
        let mut contents: Vec<_> = self
            .cells
            .iter()
            .map(|c| {
                let width =
                    self.widths[c.start..c.end].iter().sum::<usize>() + (c.end - c.start - 1) * 3;
                c.doc
                    .render(Some(width))
                    .split('\n')
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
                    .into_iter()
            })
            .collect();
        let mut heights = vec![1; count];
        for (cell, lines) in self.cells.iter().zip(&contents) {
            let previously =
                heights[cell.row..cell.bottom].iter().sum::<usize>() + (cell.bottom - cell.row - 1);
            if previously < lines.len() {
                heights[cell.bottom - 1] += lines.len() - previously;
            }
        }
        let mut lines = Vec::new();
        for (row, height) in heights.iter().enumerate() {
            self.grid_rule(row, cols, &mut contents, &mut lines);
            for _ in 0..*height {
                let mut line = "|".to_owned();
                let mut i = 0;
                while i < cols {
                    let index = self.slots[row][i];
                    let end = index.map_or(i + 1, |id| self.cells[id].end);
                    let width = self.widths[i..end].iter().sum::<usize>() + (end - i - 1) * 3;
                    let value = index.and_then(|id| contents[id].next()).unwrap_or_default();
                    line.push(' ');
                    line.push_str(&value);
                    line.push_str(&" ".repeat(width.saturating_sub(real_length(&value)) + 1));
                    line.push('|');
                    i = end;
                }
                lines.push(line);
            }
        }
        self.grid_rule(count, cols, &mut contents, &mut lines);
        literal(lines.join("\n"))
    }

    fn grid_rule(
        &self,
        row: usize,
        cols: usize,
        contents: &mut [std::vec::IntoIter<String>],
        lines: &mut Vec<String>,
    ) {
        let above = row.checked_sub(1).and_then(|i| self.slots.get(i));
        let below = self.slots.get(row);
        let marker = if self.head > 0 && row == self.head
            || self.foot > 0 && (row == self.slots.len() - self.foot || row == self.slots.len())
        {
            '='
        } else {
            '-'
        };
        let mut line = String::new();
        let mut col = 0;
        let mut prev_span = false;
        while col < cols {
            let a = above.and_then(|r| r[col]);
            let b = below.and_then(|r| r[col]);
            let span = a.filter(|_| a == b);
            let end = span.map_or_else(
                || {
                    let mut next = col + 1;
                    while next < cols {
                        let ax = above.and_then(|r| r[next]);
                        let bx = below.and_then(|r| r[next]);
                        let same_above = above.is_none() || a.is_some() && ax == a;
                        let same_below = below.is_none() || b.is_some() && bx == b;
                        if !(same_above && same_below) {
                            break;
                        }
                        next += 1;
                    }
                    next
                },
                |id| self.cells[id].end,
            );
            line.push(if (col == 0 || prev_span) && span.is_some() {
                '|'
            } else {
                '+'
            });
            let width = self.widths[col..end].iter().sum::<usize>() + (end - col - 1) * 3;
            if let Some(id) = span {
                let value = contents[id].next().unwrap_or_default();
                line.push(' ');
                line.push_str(&value);
                line.push_str(&" ".repeat(width.saturating_sub(real_length(&value)) + 1));
            } else {
                line.extend(std::iter::repeat_n(marker, width + 2));
            }
            col = end;
            prev_span = span.is_some();
        }
        line.push(if prev_span { '|' } else { '+' });
        lines.push(line);
    }
}
