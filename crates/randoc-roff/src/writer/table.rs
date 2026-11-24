use randoc_ast::{Alignment, Block, ColWidth, Row, Table};
use randoc_core::Result;

use crate::escape::charge_cells;

use super::{Format, Lead, Renderer, line};

impl<'doc> Renderer<'_, 'doc> {
    pub(super) fn write_table(
        &mut self,
        table: &'doc Table,
        out: &mut String,
        level: usize,
    ) -> Result<()> {
        self.check_depth(level)?;
        self.table = true;
        let is_ms = self.format == Format::Ms;
        let head = &table.head.rows;
        let rows = table
            .bodies
            .iter()
            .flat_map(|body| body.head.iter().chain(&body.body))
            .chain(&table.foot.rows);
        let widths = table
            .col_specs
            .iter()
            .map(|col| match col.width {
                ColWidth::ColWidth(width) if width.is_finite() && width > 0.0 => Some(width * 70.0),
                _ => None,
            })
            .collect::<Vec<_>>();
        let fixed = widths.iter().any(Option::is_some);
        let columns = table
            .col_specs
            .iter()
            .zip(&widths)
            .map(|(col, width)| {
                let align = match col.align {
                    Alignment::AlignCenter => 'c',
                    Alignment::AlignRight => 'r',
                    _ => 'l',
                };
                width.map_or_else(
                    || align.to_string(),
                    |width| format!("{align}w({width:.1}n)"),
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        line(out, ".PP");
        if !table.caption.long.is_empty() {
            for block in &table.caption.long {
                if let Block::Para(text) | Block::Plain(text) = block {
                    line(
                        out,
                        &self.render_inlines(text, super::inline::Font::ROMAN, level, false)?,
                    );
                } else {
                    self.block(block, out, Lead::Bare, level)?;
                }
            }
        }
        if is_ms {
            line(out, ".na");
            if fixed {
                line(out, ".nr LLold \\n[LL]");
            }
        }
        line(out, ".TS");
        line(
            out,
            if is_ms {
                "delim(@@) tab(\t);"
            } else {
                "tab(@);"
            },
        );
        line(out, &format!("{columns}."));
        let mut occupancy = vec![0; widths.len()];
        for (index, row) in head.iter().chain(rows).enumerate() {
            self.row(
                row,
                &widths,
                is_ms && fixed && (index > 0 || head.is_empty()),
                &mut occupancy,
                index,
                out,
                level,
            )?;
            if index == 0 && !head.is_empty() {
                line(out, "_");
            }
        }
        line(out, ".TE");
        if is_ms {
            if fixed {
                line(out, ".nr LL \\n[LLold]");
            }
            line(out, ".ad");
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        row: &'doc Row,
        widths: &[Option<f64>],
        fixed: bool,
        occupancy: &mut [usize],
        row_index: usize,
        out: &mut String,
        level: usize,
    ) -> Result<()> {
        charge_cells(
            widths.len().saturating_sub(row.cells.len()),
            &mut self.cells,
        )?;
        let is_ms = self.format == Format::Ms;
        if !out.ends_with('\n') {
            out.push('\n');
        }
        let mut cells = row.cells.iter();
        for (index, width) in widths.iter().enumerate() {
            if index > 0 {
                out.push(if is_ms { '\t' } else { '@' });
            }
            out.push_str("T{\n");
            if fixed && let Some(width) = width {
                line(out, &format!(".nr LL {width:.1}n"));
            }
            if occupancy[index] <= row_index
                && let Some(cell) = cells.next()
            {
                let end = index
                    .saturating_add(cell.col_span.max(1) as usize)
                    .min(widths.len());
                occupancy[index..end].fill(row_index.saturating_add(cell.row_span.max(1) as usize));
                self.blocks(&cell.content, out, Lead::Normal, level)?;
            }
            line(out, "T}");
        }
        out.push('\n');
        Ok(())
    }
}
