use randoc_ast::{Alignment, Block, Cell, ColSpec, ColWidth, Row, Table, TableBody, TableHead};
use randoc_core::Result;

use super::{Cursor, Font, Parser, Stop, styled};
use crate::escape::{charge_cells, depth};

impl Parser {
    pub(super) fn table(&mut self, cursor: &mut Cursor<'_>, level: usize) -> Result<Block> {
        depth(level, self.limit)?;
        let mut sep = '\t';
        if let Some(line) = cursor.peek()
            && line.text.trim_end().ends_with(';')
        {
            if let Some((_, tail)) = line.text.split_once("tab(") {
                sep = tail.chars().next().unwrap_or(sep);
            }
            cursor.next();
        }
        let mut columns = Vec::new();
        if let Some(line) = cursor.peek()
            && line.request(true).0 != "TE"
        {
            let layout = line.text.trim_end();
            let mut width = 0usize;
            for ch in layout.chars() {
                match ch {
                    '(' => width += 1,
                    ')' => width = width.saturating_sub(1),
                    _ if width > 0 => {}
                    'l' | 'c' | 'r' | 'n' | 'a' | 's' | '^' => {
                        columns.push(ColSpec {
                            align: match ch {
                                'l' => Alignment::AlignLeft,
                                'c' => Alignment::AlignCenter,
                                'r' | 'n' => Alignment::AlignRight,
                                _ => Alignment::AlignDefault,
                            },
                            ..ColSpec::default()
                        });
                    }
                    _ => {}
                }
            }
        }
        while let Some(line) = cursor.peek() {
            if line.request(true).0 == "TE" {
                break;
            }
            cursor.next();
            if line.text.trim_end().ends_with('.') {
                break;
            }
        }
        let mut rows = Vec::new();
        let mut head_count = 0;
        while let Some(line) = cursor.next() {
            if line.request(true).0 == "TE" {
                break;
            }
            let mut text = line.text.as_str();
            if matches!(text.trim(), "_" | "=") {
                if rows.len() == 1 {
                    head_count = 1;
                }
                continue;
            }
            if text.is_empty() {
                continue;
            }
            let mut cells = Vec::new();
            loop {
                let (value, tail) = text
                    .split_once(sep)
                    .map_or((text, None), |(a, b)| (a, Some(b)));
                let (content, next) = if value.trim() == "T{" {
                    let start = cursor.index;
                    let mut end = start;
                    let mut next = None;
                    while let Some(line) = cursor.peek() {
                        if let Some(rest) = line.text.strip_prefix("T}") {
                            next = rest.strip_prefix(sep);
                            cursor.next();
                            break;
                        }
                        if line.request(true).0 == "TE" {
                            break;
                        }
                        cursor.next();
                        end = cursor.index;
                    }
                    let mut sub = Cursor::new(&cursor.lines[start..end]);
                    let mut content = self.blocks(&mut sub, Stop::Document, level)?;
                    if let [Block::Para(text)] = content.as_mut_slice()
                        && !cursor.lines[start..end]
                            .iter()
                            .any(|line| matches!(line.request(true).0, "PP" | "P" | "LP"))
                    {
                        content = vec![Block::Plain(std::mem::take(text))];
                    }
                    (content, next)
                } else {
                    let text = styled(value, Font::Roman);
                    (
                        if text.is_empty() {
                            Vec::new()
                        } else {
                            vec![Block::Plain(text)]
                        },
                        tail,
                    )
                };
                cells.push(Cell {
                    content,
                    ..Cell::default()
                });
                if let Some(next) = next {
                    text = next;
                } else {
                    break;
                }
            }
            rows.push(Row {
                cells,
                ..Row::default()
            });
        }
        let cols = columns
            .len()
            .max(rows.iter().map(|row| row.cells.len()).max().unwrap_or(0));
        columns.resize_with(cols, ColSpec::default);
        if cols > 0
            && rows
                .iter()
                .flat_map(|row| &row.cells)
                .any(|cell| cell.content.len() > 1)
        {
            for column in &mut columns {
                column.width = ColWidth::ColWidth(1.0 / cols as f64);
            }
        }
        for row in &mut rows {
            charge_cells(cols - row.cells.len(), &mut self.cells)?;
            row.cells.resize_with(cols, Cell::default);
        }
        let body = rows.split_off(head_count);
        Ok(Block::Table(Box::new(Table {
            col_specs: columns,
            head: TableHead {
                rows,
                ..TableHead::default()
            },
            bodies: vec![TableBody {
                body,
                ..TableBody::default()
            }],
            ..Table::default()
        })))
    }
}
