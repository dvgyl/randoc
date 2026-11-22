use randoc_ast::{Block, Cell, ColSpec, ColWidth, Row, Table, TableBody};
use randoc_core::{Error, LogMessage, Result};

use super::blocks::{indented, joined, plain_first, skip_blanks};
use super::directives::field;
use super::{Line, Options, Parser, lines};

fn border(text: &str, marker: char) -> Vec<(usize, usize)> {
    let mut groups = Vec::new();
    let mut i = 0;
    for group in text.split(' ') {
        if !group.is_empty() {
            if !group.chars().all(|c| c == marker) {
                return Vec::new();
            }
            groups.push((i, i + group.len()));
        }
        i += group.len() + 1;
    }
    groups
}

fn cuts(text: &str, points: &[usize]) -> Vec<usize> {
    if text.is_ascii() {
        return points.iter().map(|p| (*p).min(text.len())).collect();
    }
    let mut chars = text.char_indices().peekable();
    let mut col = 0;
    points
        .iter()
        .map(|p| {
            while col < *p {
                let Some((_, c)) = chars.next() else {
                    break;
                };
                col += randoc_layout::real_length(c.encode_utf8(&mut [0; 4]));
            }
            chars.peek().map_or(text.len(), |(i, _)| *i)
        })
        .collect()
}

fn make_table(cols: Vec<ColSpec>, mut rows: Vec<Row>, head: usize) -> Table {
    let body = rows.split_off(head.min(rows.len()));
    let mut table = Table {
        col_specs: cols,
        ..Table::default()
    };
    table.head.rows = rows;
    table.bodies.push(TableBody {
        body,
        ..TableBody::default()
    });
    table
}

fn grid_separator(text: &str) -> bool {
    let text = text.trim_end();
    if !text.starts_with(['+', '|']) || !text.ends_with(['+', '|']) || !text.contains('+') {
        return false;
    }
    let mut parts = text.split('+');
    parts.next();
    let mut parts = parts.peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_some()
            && !part.is_empty()
            && (part.bytes().all(|b| b == b'-') || part.bytes().all(|b| b == b'='))
        {
            return true;
        }
    }
    false
}

fn full_rule(text: &str, marker: u8) -> bool {
    let text = text.trim_end();
    text.starts_with('+') && text.ends_with('+') && text.bytes().all(|b| b == b'+' || b == marker)
}

struct GridRow<'a> {
    content: &'a [Line<'a>],
    bottom: Line<'a>,
    end: usize,
}

struct GridCell<'a> {
    row: usize,
    start: usize,
    end: usize,
    span: usize,
    content: Vec<Line<'a>>,
}

impl Parser<'_> {
    fn reserve_cells(&mut self, amount: usize) -> Result<()> {
        self.cells = self.cells.saturating_add(amount);
        if self.cells > self.context.limits.max_table_cells {
            return Err(Error::Parse(
                "The reStructuredText document exceeds the table cell limit.".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn table(
        &mut self,
        input: &[Line<'_>],
        i: usize,
        depth: usize,
    ) -> Result<Option<(Block, usize)>> {
        let text = input[i].text.trim_end();
        if full_rule(text, b'-') && text.contains('-') {
            return self.grid(input, i, depth);
        }
        if text.starts_with('=') && text.contains(' ') {
            let layout = border(text, '=');
            if layout.len() > 1 {
                return self.simple(input, i, &layout, depth);
            }
        }
        Ok(None)
    }

    fn simple(
        &mut self,
        input: &[Line<'_>],
        start: usize,
        layout: &[(usize, usize)],
        depth: usize,
    ) -> Result<Option<(Block, usize)>> {
        let shape = input[start].text.trim_end();
        let mut first_rule = None;
        let mut end = start + 1;
        for (i, line) in input.iter().enumerate().skip(start + 1) {
            if line.text.trim_end() == shape {
                if first_rule.is_some() {
                    end = i;
                    break;
                }
                first_rule = Some(i);
                end = i;
                if input.get(i + 1).is_none_or(|line| line.blank()) {
                    break;
                }
            } else if line.blank()
                && input
                    .get(i + 1)
                    .is_none_or(|line| line.blank() || line.indent() == 0)
            {
                break;
            }
        }
        if end <= start + 1 {
            return Ok(None);
        }
        let head_end = first_rule.filter(|&i| i != end);
        let mut rows = Vec::new();
        let mut header = 0;
        let mut cursor = start + 1;
        while cursor < end {
            if Some(cursor) == head_end {
                header = rows.len();
                cursor += 1;
                continue;
            }
            let row_start = cursor;
            cursor += 1;
            while cursor < end && Some(cursor) != head_end && input[cursor].indent() >= layout[1].0
            {
                cursor += 1;
            }
            let row_end = cursor;
            let mut spans = Vec::new();
            if cursor < end && input[cursor].text.starts_with('-') {
                let rule = border(input[cursor].text.trim_end(), '-');
                if !rule.is_empty() && rule.first().map(|p| p.0) == Some(0) {
                    let mut col = 0;
                    for (left, right) in rule {
                        let begin = col;
                        while col < layout.len() && layout[col].1 <= right {
                            col += 1;
                        }
                        if col == begin || layout[begin].0 != left {
                            spans.clear();
                            break;
                        }
                        spans.push((begin, col));
                    }
                    if col == layout.len() && !spans.is_empty() {
                        cursor += 1;
                    } else {
                        spans.clear();
                    }
                }
            }
            self.reserve_cells(layout.len())?;
            if spans.is_empty() {
                spans.extend((0..layout.len()).map(|i| (i, i + 1)));
            }
            let mut positions: Vec<_> = spans.iter().map(|(i, _)| layout[*i].0).collect();
            positions.push(usize::MAX);
            let mut contents = vec![Vec::new(); spans.len()];
            for &line in &input[row_start..row_end] {
                let offsets = cuts(line.text, &positions);
                for (cell, range) in contents.iter_mut().zip(offsets.windows(2)) {
                    cell.push(line.with(line.text[range[0]..range[1]].trim()));
                }
            }
            let cells = contents
                .into_iter()
                .zip(spans)
                .map(|(content, (left, right))| {
                    self.parse_cell(&content, depth + 1).map(|mut cell| {
                        cell.col_span = (right - left) as i64;
                        cell
                    })
                })
                .collect::<Result<_>>()?;
            rows.push(Row {
                cells,
                ..Row::default()
            });
        }
        let table = make_table(vec![ColSpec::default(); layout.len()], rows, header);
        Ok(Some((Block::Table(Box::new(table)), end + 1)))
    }

    fn grid(
        &mut self,
        input: &[Line<'_>],
        start: usize,
        depth: usize,
    ) -> Result<Option<(Block, usize)>> {
        let mut rows = Vec::new();
        let mut boundaries = Vec::new();
        let mut width = 0;
        let mut band = start + 1;
        let mut end = start;
        let mut head = 0;
        for (i, &line) in input.iter().enumerate().skip(start) {
            let text = line.text.trim_end();
            if !(text.starts_with(['|', '+']) && text.ends_with(['|', '+'])) {
                break;
            }
            if i == start {
                width = text.len().saturating_sub(1);
                boundaries.resize(width + 1, false);
            }
            if randoc_layout::real_length(text) != width + 1 {
                break;
            }
            if !grid_separator(text) {
                continue;
            }
            let mut position = 0;
            for c in text.chars() {
                if c == '+' {
                    boundaries[position] = true;
                }
                position += randoc_layout::real_length(c.encode_utf8(&mut [0; 4]));
            }
            if i >= band {
                if i == band {
                    break;
                }
                rows.push(GridRow {
                    content: &input[band..i],
                    bottom: line,
                    end: i,
                });
                if full_rule(text, b'=') {
                    head = rows.len();
                }
                if full_rule(text, b'-') {
                    end = i;
                }
            }
            band = i + 1;
        }
        if end <= start {
            return Ok(None);
        }
        while rows.last().is_some_and(|row| row.end > end) {
            rows.pop();
        }
        let points: Vec<_> = boundaries
            .iter()
            .enumerate()
            .filter_map(|(i, marked)| marked.then_some(i))
            .collect();
        let columns = points.len().saturating_sub(1);
        if columns == 0 {
            return Ok(None);
        }
        self.reserve_cells(columns.saturating_mul(rows.len()))?;
        let mut all = Vec::<GridCell<'_>>::new();
        let mut active = vec![None::<usize>; columns];
        for (row_index, row) in rows.iter().enumerate() {
            let first = row.content[0].text;
            let boundaries = cuts(first, &points);
            let bottom = cuts(row.bottom.text, &points);
            let mut row_cells = Vec::new();
            let mut col = 0;
            while col < columns {
                let left = col;
                col += 1;
                while col < columns && first.as_bytes().get(boundaries[col]) != Some(&b'|') {
                    col += 1;
                }
                let existing = active[left].filter(|&i| all[i].end == col);
                let index = existing.unwrap_or_else(|| {
                    let index = all.len();
                    all.push(GridCell {
                        row: row_index,
                        start: left,
                        end: col,
                        span: 0,
                        content: Vec::new(),
                    });
                    index
                });
                let cell = &mut all[index];
                cell.span += 1;
                let segment = &row.bottom.text[bottom[left]..bottom[col]];
                let closed =
                    segment.starts_with('+') && segment.bytes().all(|b| b"+-=".contains(&b));
                active[left..col].fill(if closed { None } else { Some(index) });
                row_cells.push((index, closed));
            }
            for (offset, &line) in row
                .content
                .iter()
                .chain(std::iter::once(&row.bottom))
                .enumerate()
            {
                let offsets = cuts(line.text, &points);
                for &(index, closed) in &row_cells {
                    if closed && offset == row.content.len() {
                        continue;
                    }
                    let cell = &mut all[index];
                    let start = offsets[cell.start].saturating_add(1).min(line.text.len());
                    let end = offsets[cell.end];
                    if let Some(text) = line.text.get(start..end) {
                        cell.content.push(line.with(text.trim_end()));
                    }
                }
            }
        }
        let mut rendered = vec![Row::default(); rows.len()];
        for cell in all {
            let offset = cell
                .content
                .iter()
                .filter(|l| !l.blank())
                .map(|l| l.indent())
                .min()
                .unwrap_or(0);
            let content: Vec<_> = cell.content.into_iter().map(|l| l.cut(offset)).collect();
            let blocks = self.blocks(&content, depth + 1)?;
            let mut parsed = Cell {
                content: blocks,
                ..Cell::default()
            };
            parsed.row_span = cell.span as i64;
            parsed.col_span = (cell.end - cell.start) as i64;
            rendered[cell.row].cells.push(parsed);
        }
        for row in &mut rendered {
            if row.cells.iter().all(|cell| cell.content.len() <= 1) {
                for cell in &mut row.cells {
                    plain_first(&mut cell.content);
                }
            }
        }
        let cols = points
            .windows(2)
            .map(|p| ColSpec {
                width: ColWidth::ColWidth(
                    (p[1] - p[0]) as f64 / (self.options.columns.max(1) as f64),
                ),
                ..ColSpec::default()
            })
            .collect();
        Ok(Some((
            Block::Table(Box::new(make_table(cols, rendered, head))),
            end + 1,
        )))
    }

    fn parse_cell(&mut self, content: &[Line<'_>], depth: usize) -> Result<Cell> {
        let mut blocks = self.blocks(content, depth)?;
        if blocks.len() == 1 {
            plain_first(&mut blocks);
        }
        Ok(Cell {
            content: blocks,
            ..Cell::default()
        })
    }

    pub(super) fn directive_table(
        &mut self,
        kind: &str,
        arg: &str,
        fields: &Options,
        body: &[Line<'_>],
        line: Line<'_>,
        depth: usize,
    ) -> Result<Vec<Block>> {
        let mut table = if kind == "table" {
            let Some((Block::Table(table), _)) = (!body.is_empty())
                .then(|| self.table(body, 0, depth))
                .transpose()?
                .flatten()
            else {
                return self.blocks(body, depth);
            };
            *table
        } else if kind == "list-table" {
            self.list_table(body, fields, depth)?
        } else {
            self.csv_table(body, fields, line, depth)?
        };
        if !arg.is_empty() {
            table
                .caption
                .long
                .push(Block::Plain(self.inlines(arg, &line.pos())));
        }
        if let Some(widths) = field(fields, "widths") {
            let widths: Vec<f64> = widths
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter_map(|s| s.parse().ok())
                .collect();
            let total: f64 = if kind == "table" {
                self.options.columns.max(1) as f64
            } else {
                widths.iter().sum()
            };
            if total.is_finite() && total > 0.0 && widths.len() == table.col_specs.len() {
                for (col, width) in table.col_specs.iter_mut().zip(widths) {
                    if width.is_finite() && width >= 0.0 {
                        col.width = ColWidth::ColWidth(width / total);
                    }
                }
            }
        }
        Ok(vec![Block::Table(Box::new(table))])
    }

    fn list_table(&mut self, body: &[Line<'_>], fields: &Options, depth: usize) -> Result<Table> {
        let mut layout = Vec::new();
        let mut i = 0;
        while i < body.len() {
            i = skip_blanks(body, i);
            let Some(&line) = body.get(i) else {
                break;
            };
            let Some((first, size)) = bullet(line.text) else {
                break;
            };
            let (mut nested, end) = indented(body, i + 1, size);
            nested.insert(0, line.with(first));
            let mut cells = Vec::new();
            let mut cursor = 0;
            while cursor < nested.len() {
                cursor = skip_blanks(&nested, cursor);
                let Some(&line) = nested.get(cursor) else {
                    break;
                };
                let Some((first, size)) = bullet(line.text) else {
                    break;
                };
                let (mut content, end) = indented(&nested, cursor + 1, size);
                self.reserve_cells(1)?;
                content.insert(0, line.with(first));
                cells.push(self.parse_cell(&content, depth + 1)?);
                cursor = end;
            }
            layout.push(Row {
                cells,
                ..Row::default()
            });
            i = end;
        }
        self.directive_rows(layout, header_count(fields))
    }

    fn csv_table(
        &mut self,
        body: &[Line<'_>],
        fields: &Options,
        line: Line<'_>,
        depth: usize,
    ) -> Result<Table> {
        let text = if let Some(file) = field(fields, "file") {
            if let Some((_, text)) = self.load(file)? {
                text
            } else {
                self.context
                    .report(LogMessage::CouldNotLoadIncludeFile(file.into(), line.pos()));
                String::new()
            }
        } else {
            joined(body)
        };
        let delimiter = match field(fields, "delim") {
            Some("tab") => '\t',
            Some("space") => ' ',
            Some(delim) => delim.chars().next().unwrap_or(','),
            None => ',',
        };
        let quote = field(fields, "quote")
            .and_then(|s| s.chars().next())
            .unwrap_or('"');
        let escape = field(fields, "escape").and_then(|s| s.chars().next());
        let header = field(fields, "header");
        let mut rows = Vec::new();
        for text in header.into_iter().chain(std::iter::once(text.as_str())) {
            let mut chars = text.chars().peekable();
            let mut quoted = false;
            let mut value = String::new();
            let mut cells = Vec::new();
            while let Some(c) = chars.next() {
                if quoted {
                    if Some(c) == escape {
                        value.push(chars.next().unwrap_or(c));
                    } else if c == quote {
                        if chars.peek() == Some(&quote) {
                            value.push(c);
                            chars.next();
                        } else {
                            quoted = false;
                        }
                    } else {
                        value.push(c);
                    }
                } else if c == quote && value.trim().is_empty() {
                    value.clear();
                    quoted = true;
                } else if c == delimiter || c == '\n' {
                    if c == '\n' && cells.is_empty() && value.trim().is_empty() {
                        continue;
                    }
                    cells.push(self.csv_cell(&value, line, depth)?);
                    value.clear();
                    if c == '\n' {
                        rows.push(Row {
                            cells: std::mem::take(&mut cells),
                            ..Row::default()
                        });
                    }
                } else {
                    value.push(c);
                }
            }
            if !value.is_empty() || !cells.is_empty() || text.ends_with(quote) {
                cells.push(self.csv_cell(&value, line, depth)?);
                rows.push(Row {
                    cells,
                    ..Row::default()
                });
            }
        }
        self.directive_rows(
            rows,
            if header.is_some() {
                1
            } else {
                header_count(fields)
            },
        )
    }

    fn csv_cell(&mut self, text: &str, line: Line<'_>, depth: usize) -> Result<Cell> {
        self.reserve_cells(1)?;
        let input: Vec<_> = lines(text.trim(), line.source, line.number).collect();
        self.parse_cell(&input, depth + 1)
    }

    fn directive_rows(&mut self, mut rows: Vec<Row>, header: usize) -> Result<Table> {
        let cols = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
        for row in &mut rows {
            self.reserve_cells(cols - row.cells.len())?;
            row.cells.resize_with(cols, Cell::default);
        }
        Ok(make_table(vec![ColSpec::default(); cols], rows, header))
    }
}

fn header_count(fields: &Options) -> usize {
    field(fields, "header-rows")
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn bullet(text: &str) -> Option<(&str, usize)> {
    let first = text.strip_prefix(['-', '*', '+'])?;
    let value = first.strip_prefix(' ')?.trim_start_matches(' ');
    Some((value, text.len() - value.len()))
}
