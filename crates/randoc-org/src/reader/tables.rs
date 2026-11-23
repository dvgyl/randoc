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
            .map_or(0, |text| cells(text).count());
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
            if alignment_row(text) {
                for (spec, cell) in cookies
                    .iter_mut()
                    .zip(cells(text).chain(std::iter::repeat("")))
                {
                    *spec = cookie(cell).unwrap_or((Alignment::AlignDefault, None));
                }
            } else {
                self.charge_table_cells(columns)?;
                rows.push(cells(text).take(columns).collect::<Vec<_>>());
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

    /// Reads a `table.el` grid table: the lines from one border line to the
    /// last border line of [`grid_table_len`].
    pub(super) fn grid_table(
        &mut self,
        lines: &[&str],
        affiliated: &Affiliated<'_>,
        depth: usize,
    ) -> Result<Block> {
        let border = lines.first().map_or("", |line| line.trim_end());
        let bounds = border
            .chars()
            .enumerate()
            .filter_map(|(index, c)| (c == '+').then_some(index))
            .collect::<Vec<_>>();
        let columns = bounds.len().saturating_sub(1);
        self.charge_table_cells(columns)?;
        let separator = lines
            .iter()
            .position(|line| grid_border(line) && line.contains('='));
        let alignments = segments(lines[separator.unwrap_or(0)].trim_end(), &bounds);
        let total = bounds
            .windows(2)
            .map(|pair| (pair[1] - pair[0]) as f64)
            .sum::<f64>();
        let width = total.max(self.options.columns.max(1) as f64);
        let col_specs = bounds
            .windows(2)
            .zip(alignments)
            .map(|(pair, align)| ColSpec {
                align,
                width: ColWidth::ColWidth((pair[1] - pair[0]) as f64 / width),
            })
            .collect();
        let mut rows = Vec::new();
        let mut head = 0;
        let mut current = Vec::new();
        for (index, line) in lines.iter().enumerate().skip(1) {
            if !grid_border(line) {
                current.push(line.chars().collect::<Vec<_>>());
                continue;
            }
            if !current.is_empty() {
                self.charge_table_cells(columns)?;
                rows.push(self.grid_row(&std::mem::take(&mut current), &bounds, depth)?);
            }
            if separator == Some(index) {
                head = rows.len();
            }
        }
        if rows
            .iter()
            .flatten()
            .any(|cell| !matches!(cell.content.as_slice(), [] | [Block::Plain(_)]))
        {
            for cell in rows.iter_mut().flatten() {
                for block in &mut cell.content {
                    if let Block::Plain(text) = block {
                        *block = Block::Para(std::mem::take(text));
                    }
                }
            }
        }
        let mut rows = rows.into_iter().map(|cells| Row {
            cells,
            ..Row::default()
        });
        let headers = rows.by_ref().take(head).collect();
        let body = rows.collect();
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

    /// Reads the cells of one grid table row. A column boundary without a bar
    /// on each line of the row joins the two cells.
    fn grid_row(
        &mut self,
        lines: &[Vec<char>],
        bounds: &[usize],
        depth: usize,
    ) -> Result<Vec<Cell>> {
        let mut cells = Vec::new();
        let mut first = 0;
        for column in 1..bounds.len() {
            let last = column + 1 == bounds.len();
            if !last
                && !lines
                    .iter()
                    .all(|line| line.get(bounds[column]) == Some(&'|'))
            {
                continue;
            }
            let start = bounds[first] + 1;
            let end = bounds[column];
            let text = lines
                .iter()
                .map(|line| {
                    line.get(start.min(line.len())..end.min(line.len()))
                        .map_or_else(String::new, |part| {
                            part.iter().collect::<String>().trim_end().to_owned()
                        })
                })
                .collect::<Vec<_>>();
            let indent = text
                .iter()
                .filter(|line| !line.is_empty())
                .map(|line| line.len() - line.trim_start_matches(' ').len())
                .min()
                .unwrap_or(0);
            let text = text
                .iter()
                .map(|line| line.get(indent..).unwrap_or_default())
                .collect::<Vec<_>>();
            let mut content = self.blocks(&text, depth + 1)?;
            if let [Block::Para(inlines)] = content.as_mut_slice() {
                content = vec![Block::Plain(std::mem::take(inlines))];
            }
            cells.push(Cell {
                col_span: (column - first) as i64,
                content,
                ..Cell::default()
            });
            first = column;
        }
        Ok(cells)
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

/// Returns the text of a data row after its first bar. A row with nothing after
/// the first bar has no cells.
fn data_cells(line: &str) -> Option<&str> {
    let text = line.trim().strip_prefix('|')?;
    (!text.starts_with(['-', '+'])).then_some(text)
}

fn cells(text: &str) -> impl Iterator<Item = &str> {
    let inner = text.strip_suffix('|').unwrap_or(text);
    inner
        .split('|')
        .map(str::trim)
        .take(if text.is_empty() { 0 } else { usize::MAX })
}

fn alignment_row(text: &str) -> bool {
    text.bytes().any(|byte| byte == b'<') && cells(text).all(|cell| cookie(cell).is_some())
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

/// Returns the number of lines of the grid table that starts at the first line:
/// the lines up to the last border line in the run of lines that start with a
/// bar or a plus sign.
pub(super) fn grid_table_len(lines: &[&str]) -> Option<usize> {
    if !lines.first().is_some_and(|line| grid_border(line)) {
        return None;
    }
    let run = lines
        .iter()
        .take_while(|line| line.starts_with(['+', '|']))
        .count();
    let last = lines[..run].iter().rposition(|line| grid_border(line))?;
    (last > 0).then_some(last + 1)
}

fn grid_border(line: &str) -> bool {
    let line = line.trim_end();
    line.len() > 2
        && line.starts_with('+')
        && line.ends_with('+')
        && line.bytes().all(|byte| b"+-=:".contains(&byte))
        && line.bytes().any(|byte| matches!(byte, b'-' | b'='))
}

/// Returns the alignment of each column from the colons of a border line.
fn segments(border: &str, bounds: &[usize]) -> Vec<Alignment> {
    let chars = border.chars().collect::<Vec<_>>();
    bounds
        .windows(2)
        .map(|pair| {
            let part = chars.get(pair[0] + 1..pair[1]).unwrap_or_default();
            match (part.first() == Some(&':'), part.last() == Some(&':')) {
                (true, true) => Alignment::AlignCenter,
                (true, false) => Alignment::AlignLeft,
                (false, true) => Alignment::AlignRight,
                (false, false) => Alignment::AlignDefault,
            }
        })
        .collect()
}
