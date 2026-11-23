use std::path::Path;

use randoc_ast::{Alignment, Block, Cell, ColSpec, ColWidth, Inline, Row, Table, TableBody};
use randoc_core::Result;

use super::{
    State, attrs,
    blocks::{Sections, caption},
    inlines::{block_cost, monospace},
};
use crate::limit;

#[derive(Default)]
struct Column {
    align: Option<Alignment>,
    weight: Option<f64>,
    style: Option<char>,
}

struct ParsedCell {
    mode: Option<char>,
    index: usize,
    cost: Option<u64>,
    has_headings: bool,
}

struct Entry<'a> {
    spec: &'a str,
    value: &'a str,
    row: usize,
    col: usize,
    copies: usize,
}

impl State<'_> {
    pub(super) fn table(
        &mut self,
        lines: &[&str],
        fence: &str,
        attributes: &attrs::Attributes<'_>,
        path: &Path,
        depth: usize,
    ) -> Result<Block> {
        self.depth(depth)?;
        let cap = self.context.limits.max_table_cells;
        let mut generated = cap;
        let mut columns = Vec::new();
        if let Some(specs) = attributes.attr.get("cols") {
            for spec in specs.split(',') {
                let (repeat, spec) = spec
                    .split_once('*')
                    .map_or(Ok((1, spec)), |(count, spec)| Ok((number(count)?, spec)))?;
                charge_cells(&mut generated, repeat - 1)?;
                for _ in 0..repeat {
                    columns.push(Column {
                        align: alignment(spec),
                        weight: spec
                            .trim_matches(|ch: char| !ch.is_ascii_digit() && ch != '.')
                            .parse::<f64>()
                            .ok(),
                        style: column_style(spec),
                    });
                }
            }
        }
        let separator = fence.as_bytes()[0] as char;
        let format = attributes.attr.get("format").unwrap_or(match separator {
            ',' => "csv",
            ':' => "dsv",
            _ => "psv",
        });
        let body = lines.join("\n");
        let mut entries = Vec::new();
        let mut first_columns = 0_usize;
        let mut inferred_extra = 0_usize;
        if matches!(format, "csv" | "dsv") {
            let separator = attributes
                .attr
                .get("separator")
                .and_then(|value| value.chars().next())
                .unwrap_or(if format == "csv" { ',' } else { ':' });
            for (index, line) in lines
                .iter()
                .filter(|line| !line.trim().is_empty())
                .enumerate()
            {
                let mut quoted = false;
                for value in line.split(|ch| {
                    if format == "csv" && ch == '"' {
                        quoted = !quoted;
                    }
                    ch == separator && !quoted
                }) {
                    if index == 0 {
                        first_columns += 1;
                    }
                    entries.push(Entry {
                        spec: "",
                        value: attrs::unquote(value.trim()),
                        row: 1,
                        col: 1,
                        copies: 1,
                    });
                }
            }
        } else {
            let mut previous: Option<(usize, &str)> = None;
            let mut first_line = None;
            let mut line = 0;
            let mut token = 0;
            let mut escaped = false;
            for (index, ch) in body.char_indices() {
                if ch == separator && !escaped {
                    let candidate = &body[token..index];
                    let spec = if valid_spec(candidate) { candidate } else { "" };
                    let start = index - spec.len();
                    if let Some((start_value, previous_spec)) = previous {
                        entries.push(entry(previous_spec, &body[start_value..start], cap)?);
                    }
                    previous = Some((index + ch.len_utf8(), spec));
                    if columns.is_empty() && *first_line.get_or_insert(line) == line {
                        let cell = entry(spec, "", cap)?;
                        let slots = cell.col.saturating_mul(cell.copies);
                        first_columns = first_columns.saturating_add(slots);
                        inferred_extra = inferred_extra.saturating_add(slots - 1);
                        if inferred_extra > generated {
                            return Err(limit("table cells"));
                        }
                    }
                    token = index + ch.len_utf8();
                } else if ch.is_ascii_whitespace() {
                    token = index + ch.len_utf8();
                    if ch == '\n' {
                        line += 1;
                    }
                }
                escaped = ch == '\\' && !escaped;
            }
            if let Some((start, spec)) = previous {
                entries.push(entry(spec, &body[start..], cap)?);
            }
        }
        if columns.is_empty() {
            columns.resize_with(first_columns, Column::default);
        }
        let width = columns.len();
        let weights = columns.iter().any(|column| column.weight.is_some());
        let sum: f64 = columns
            .iter()
            .map(|column| column.weight.unwrap_or(1.0))
            .sum();
        let specs = columns
            .iter()
            .map(|column| ColSpec {
                align: column.align.unwrap_or(Alignment::AlignDefault),
                width: if weights && sum > 0.0 {
                    ColWidth::ColWidth(column.weight.unwrap_or(1.0) / sum)
                } else {
                    ColWidth::ColWidthDefault
                },
            })
            .collect();
        let loose = entries
            .iter()
            .any(|entry| entry.value.trim().contains("\n\n"));
        let mut rows = Vec::new();
        let mut row = Row::default();
        let mut col = 0;
        let mut occupied = vec![0_usize; width];
        for item in entries {
            while col == width || occupied[col] > rows.len() {
                if col == width {
                    if row.cells.is_empty() {
                        charge_cells(&mut generated, width)?;
                    }
                    rows.push(std::mem::take(&mut row));
                    col = 0;
                } else {
                    col += 1;
                }
            }

            charge_cells(&mut generated, item.row.saturating_mul(item.col) - 1)?;
            let count = item.copies.min(width - col);
            let mut parsed: Vec<ParsedCell> = Vec::new();
            let mut styles = Vec::new();
            for copy in 0..count {
                while col < width && occupied[col] > rows.len() {
                    col += 1;
                }
                if col == width {
                    break;
                }
                let span = occupied[col..]
                    .iter()
                    .take(item.col)
                    .take_while(|until| **until <= rows.len())
                    .count();
                occupied[col..col + span].fill(rows.len().saturating_add(item.row));
                let style = column_style(item.spec).or(columns[col].style);
                // Each parsing mode reads the input once. Inline styles apply after parsing.
                let mode = style.filter(|style| matches!(style, 'a' | 'l'));
                if copy != 0 {
                    charge_cells(&mut generated, 1)?;
                }
                let mut content =
                    if let Some(parsed) = parsed.iter_mut().find(|parsed| parsed.mode == mode) {
                        let content = &row.cells[parsed.index].content;
                        let cost = parsed.cost.get_or_insert_with(|| block_cost(content));
                        self.charge(*cost)?;
                        let mut copy = content.clone();
                        if parsed.has_headings {
                            self.copy_cell_headings(&mut copy);
                        }
                        copy
                    } else {
                        let before = self.headings.len();
                        let content = self.table_cell(item.value, mode, loose, path, depth)?;
                        if copy != 0 {
                            self.charge(block_cost(&content))?;
                        }
                        if copy + 1 < count {
                            parsed.push(ParsedCell {
                                mode,
                                index: row.cells.len(),
                                cost: None,
                                has_headings: self.headings.len() != before,
                            });
                        }
                        content
                    };
                if let Some(style @ ('m' | 'e' | 's')) = style {
                    if count == 1 {
                        style_cell(&mut content, style);
                    } else {
                        styles.push((row.cells.len(), style));
                    }
                }
                row.cells.push(Cell {
                    align: alignment(item.spec).unwrap_or(Alignment::AlignDefault),
                    row_span: item.row as i64,
                    col_span: span as i64,
                    content,
                    ..Cell::default()
                });
                col += span;
            }
            for (index, style) in styles {
                style_cell(&mut row.cells[index].content, style);
            }
        }
        if !row.cells.is_empty() {
            rows.push(row);
        }
        let implicit = lines.first().is_some_and(|first| !first.trim().is_empty())
            && lines.get(1).is_some_and(|second| second.trim().is_empty());
        let header = !attributes.option("noheader")
            && (attributes.option("header") && attributes.options.len() == 1 || implicit);
        let footer = attributes.option("footer") && !attributes.option("header");
        let mut table = Table {
            col_specs: specs,
            ..Table::default()
        };
        if let Some(title) = attributes.title {
            table.caption = caption(self.inlines(title, depth + 1)?);
        }
        if footer
            && !rows.is_empty()
            && let Some(row) = rows.pop()
        {
            table.foot.rows.push(row);
        }
        let mut rows = rows.into_iter();
        if header && let Some(row) = rows.next() {
            table.head.rows.push(row);
        }
        table.bodies.push(TableBody {
            body: rows.collect(),
            ..TableBody::default()
        });
        Ok(Block::Table(Box::new(table)))
    }

    fn copy_cell_headings(&mut self, blocks: &mut Vec<Block>) {
        randoc_ast::walk::walk_blocks_mut(blocks, |block| {
            if let Block::Header(_, attr, _) = block
                && let Some(index) = attr
                    .id
                    .strip_prefix('\0')
                    .and_then(|id| id.parse::<usize>().ok())
                && let Some(heading) = self.headings.get(index).cloned()
            {
                attr.id = format!("\0{}", self.headings.len());
                self.headings.push(heading);
            }
        });
    }

    fn table_cell(
        &mut self,
        raw: &str,
        mode: Option<char>,
        loose: bool,
        path: &Path,
        depth: usize,
    ) -> Result<Vec<Block>> {
        let value = raw.trim();
        let content = match mode {
            Some('a') => self.blocks(
                &value.lines().collect::<Vec<_>>(),
                path,
                depth + 1,
                Sections::Cell,
            )?,
            Some('l') => vec![Block::CodeBlock(
                Default::default(),
                raw.trim_start().into(),
            )],
            _ if value.is_empty() => Vec::new(),
            _ => {
                let mut contents = Vec::new();

                for text in value.split("\n\n") {
                    let comment =
                        |line: &str| line.trim_start().starts_with("//") && line.trim() != "//";
                    let trailing_comment = text.lines().last().is_some_and(comment);
                    let text = text
                        .lines()
                        .filter(|line| {
                            !comment(line) && !(line.starts_with('[') && line.ends_with(']'))
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let mut inline = self.inlines(&text, depth + 1)?;
                    if trailing_comment && !inline.is_empty() {
                        inline.push(Inline::SoftBreak);
                    }
                    if inline.is_empty() {
                        continue;
                    }
                    contents.push(if loose {
                        Block::Para(inline)
                    } else {
                        Block::Plain(inline)
                    });
                }
                contents
            }
        };
        Ok(content)
    }
}

fn charge_cells(remaining: &mut usize, count: usize) -> Result<()> {
    *remaining = remaining
        .checked_sub(count)
        .ok_or_else(|| limit("table cells"))?;
    Ok(())
}

fn style_cell(content: &mut [Block], style: char) {
    for block in content {
        if let Block::Plain(inlines) | Block::Para(inlines) = block {
            let body = std::mem::take(inlines);
            *inlines = match style {
                'm' => monospace(body),
                'e' => vec![Inline::Emph(body)],
                _ => vec![Inline::Strong(body)],
            };
        }
    }
}

fn valid_spec(spec: &str) -> bool {
    spec.bytes().all(|ch| {
        matches!(
            ch,
            b'0'..=b'9'
                | b'+'
                | b'*'
                | b'.'
                | b'<'
                | b'>'
                | b'^'
                | b'a'
                | b'd'
                | b'e'
                | b'h'
                | b'l'
                | b'm'
                | b's'
                | b'v'
        )
    })
}

fn number(value: &str) -> Result<usize> {
    if value.is_empty() {
        return Ok(1);
    }
    let number = value.parse::<usize>().map_err(|_| limit("table cells"))?;
    Ok(number.max(1))
}

fn entry<'a>(spec: &'a str, value: &'a str, cap: usize) -> Result<Entry<'a>> {
    let (mut row, mut col, mut copies) = (1, 1, 1);
    if let Some((span, _)) = spec.split_once('+') {
        let (horizontal, vertical) = span.split_once('.').unwrap_or((span, ""));
        col = number(horizontal)?;
        row = number(vertical)?;
        if row.saturating_mul(col) - 1 > cap || row > i64::MAX as usize {
            return Err(limit("table cells"));
        }
    } else if let Some((count, _)) = spec.split_once('*') {
        copies = number(count)?;
    }
    Ok(Entry {
        spec,
        value,
        row,
        col,
        copies,
    })
}

fn column_style(spec: &str) -> Option<char> {
    spec.chars()
        .last()
        .filter(|ch| matches!(ch, 'a' | 'd' | 'e' | 'h' | 'l' | 'm' | 's' | 'v'))
}

fn alignment(spec: &str) -> Option<Alignment> {
    let rest = spec.rsplit_once(['+', '*']).map_or(spec, |(_, rest)| rest);
    rest.split('.').next().and_then(|text| {
        text.chars().find_map(|ch| match ch {
            '<' => Some(Alignment::AlignLeft),
            '>' => Some(Alignment::AlignRight),
            '^' => Some(Alignment::AlignCenter),
            _ => None,
        })
    })
}
