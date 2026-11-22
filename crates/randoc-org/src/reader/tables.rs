use randoc_ast::{Alignment, Block, Cell, ColSpec, ColWidth, Row, Table, TableBody, TableHead};
use randoc_core::{Error, Result};

use super::Parser;
use super::directives::Affiliated;

impl Parser<'_> {
    pub(super) fn table(
        &mut self,
        lines: &[&str],
        affiliated: &Affiliated<'_>,
        depth: usize,
    ) -> Result<Block> {
        let columns = lines
            .iter()
            .filter_map(|line| data_cells(line))
            .find(|&text| !alignment_row(text))
            .map_or(0, |text| text.split('|').count());
        self.charge_table_cells(columns)?;
        let mut cookies = vec![(Alignment::AlignDefault, None); columns];
        let mut head = None;
        let mut rows = Vec::new();
        for line in lines {
            let Some(text) = data_cells(line) else {
                if !rows.is_empty() && head.is_none() {
                    head = Some(rows.len());
                }
                continue;
            };
            let cells = text.split('|').map(str::trim);
            if alignment_row(text) {
                for (spec, cell) in cookies.iter_mut().zip(cells.chain(std::iter::repeat(""))) {
                    *spec = cookie(cell).unwrap_or((Alignment::AlignDefault, None));
                }
            } else {
                self.charge_table_cells(columns)?;
                rows.push(cells.take(columns).collect::<Vec<_>>());
            }
        }
        let total = cookies
            .iter()
            .map(|(_, width)| width.unwrap_or(1) as f64)
            .sum::<f64>();
        let col_specs = cookies
            .into_iter()
            .map(|(align, width)| ColSpec {
                align,
                width: width
                    .filter(|_| total > 0.0)
                    .map_or(ColWidth::ColWidthDefault, |width| {
                        ColWidth::ColWidth(width as f64 / total)
                    }),
            })
            .collect();
        let mut headers = Vec::new();
        let mut body = Vec::new();
        for (index, raw) in rows.into_iter().enumerate() {
            let mut row = Row::default();
            for value in raw.into_iter().chain(std::iter::repeat("")).take(columns) {
                let value = self.inlines(value, depth + 1)?;
                let content = if value.is_empty() {
                    vec![]
                } else {
                    vec![Block::Plain(value)]
                };
                row.cells.push(Cell {
                    content,
                    ..Cell::default()
                });
            }
            if head == Some(1) && index == 0 {
                headers.push(row);
            } else {
                body.push(row);
            }
        }
        Ok(Block::Table(Box::new(Table {
            attr: affiliated.attr(),
            caption: self.caption(affiliated, depth)?,
            col_specs,
            head: TableHead {
                rows: headers,
                ..TableHead::default()
            },
            bodies: vec![TableBody {
                body,
                ..TableBody::default()
            }],
            ..Table::default()
        })))
    }

    fn charge_table_cells(&mut self, columns: usize) -> Result<()> {
        self.table_cells = self
            .table_cells
            .checked_add(columns)
            .filter(|&cells| cells <= self.context.limits.max_table_cells)
            .ok_or_else(|| Error::Parse("Org tables exceed the cell limit.".into()))?;
        Ok(())
    }
}

fn data_cells(line: &str) -> Option<&str> {
    let text = line.trim().strip_prefix('|')?;
    (!text.starts_with(['-', '+'])).then(|| text.strip_suffix('|').unwrap_or(text))
}

fn alignment_row(text: &str) -> bool {
    text.bytes().any(|byte| byte == b'<')
        && text.split('|').all(|cell| cookie(cell.trim()).is_some())
}

fn cookie(cell: &str) -> Option<(Alignment, Option<usize>)> {
    if cell.is_empty() {
        return Some((Alignment::AlignDefault, None));
    }
    let value = cell.strip_prefix('<')?.strip_suffix('>')?;
    let (align, tail) = match value.as_bytes().first() {
        Some(b'l') => (Alignment::AlignLeft, &value[1..]),
        Some(b'c') => (Alignment::AlignCenter, &value[1..]),
        Some(b'r') => (Alignment::AlignRight, &value[1..]),
        _ => (Alignment::AlignDefault, value),
    };
    if tail.is_empty() {
        (align != Alignment::AlignDefault).then_some((align, None))
    } else {
        Some((align, Some(tail.parse().ok()?)))
    }
}
