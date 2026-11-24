use randoc_ast::{Alignment, Block, Cell, ColSpec, ColWidth, Inline, Row, Table, TableBody};
use randoc_core::Result;

use crate::reader::{Parser, append_paragraph, is_code_block_tag};
use crate::scan::{Scan, attrs, char_len, line_end};
use crate::{Wiki, cell_error};

struct CellSource {
    start: usize,
    first_end: usize,
    end: usize,
    rescan: bool,
}

#[derive(Default)]
struct TableSource {
    caption: Option<(usize, usize)>,
    rows: Vec<(bool, Vec<CellSource>)>,
    multiline: bool,
}

impl Parser<'_> {
    fn charge_cells(&mut self, cells: usize) -> Result<()> {
        self.cells_left = self.cells_left.checked_sub(cells).ok_or_else(cell_error)?;
        Ok(())
    }

    pub(crate) fn media_table(
        &mut self,
        scan: &Scan<'_>,
        opener_start: usize,
        opener_end: usize,
        close: usize,
        depth: usize,
    ) -> Result<Option<Block>> {
        self.depth(depth)?;
        let sources = self.media_table_sources(scan, opener_end, close)?;
        let mut table = Table::default();
        if let Some((start, end)) = sources.caption {
            table.caption.long = vec![Block::Plain(self.inlines(scan, start, end, depth)?)];
        }
        let mut rows = Vec::new();
        let mut multiline = sources.multiline
            || table
                .caption
                .long
                .iter()
                .any(|block| !plain_cell_block(block));
        for (head, cells) in sources.rows {
            let row = self.media_row(scan, &cells, depth)?;
            multiline |= row
                .cells
                .iter()
                .any(|cell| cell.content.iter().any(|block| !plain_cell_block(block)));
            rows.push((head, row));
        }
        let width = percent_width(attrs(&scan.text[opener_start + 2..opener_end]).get("width"))
            .unwrap_or(1.0);
        self.finish_media_table(table, rows, multiline, width)
    }

    fn finish_media_table(
        &mut self,
        mut table: Table,
        mut rows: Vec<(bool, Row)>,
        multiline: bool,
        table_width: f64,
    ) -> Result<Option<Block>> {
        if multiline {
            for (_, row) in &mut rows {
                for cell in &mut row.cells {
                    for block in &mut cell.content {
                        if let Block::Plain(items) = block {
                            *block = Block::Para(std::mem::take(items));
                        }
                    }
                }
            }
        }
        let Some((_, first)) = rows.first() else {
            return Ok(None);
        };
        let width = first
            .cells
            .iter()
            .try_fold(0usize, |width, cell| {
                width.checked_add(usize::try_from(cell.col_span).ok()?)
            })
            .ok_or_else(cell_error)?;
        table.col_specs.resize(width, ColSpec::default());
        let mut column = 0;
        for cell in &first.cells {
            let span = cell.col_span as usize;
            let width = percent_width(cell.attr.get("width"));
            for spec in &mut table.col_specs[column..column + span] {
                spec.align = cell.align;
                if let Some(width) = width {
                    spec.width = ColWidth::ColWidth(width / span as f64);
                }
            }
            column += span;
        }
        let mut specified = 0;
        let mut total = 0.0;
        for spec in &table.col_specs {
            if let ColWidth::ColWidth(width) = spec.width {
                total += width;
                specified += 1;
            }
        }
        if specified > 0 {
            let remaining = width - specified;
            for spec in &mut table.col_specs {
                let share = match spec.width {
                    ColWidth::ColWidth(width) => width,
                    ColWidth::ColWidthDefault => (1.0 - total).max(0.0) / remaining as f64,
                };
                spec.width = ColWidth::ColWidth(share * table_width);
            }
        }
        let first_header = rows.first().is_some_and(|(header, _)| *header);
        let mut body = TableBody::default();
        let row_count = rows.len();
        let mut occupancy = vec![0; width];
        for (index, (_, row)) in rows.into_iter().enumerate() {
            if first_header && index == 1 {
                occupancy.fill(0);
            }
            let group_end = if first_header && index == 0 {
                1
            } else {
                row_count
            };
            let row = self.normalize_row(row, &mut occupancy, index, group_end)?;
            if first_header && index == 0 {
                table.head.rows.push(row);
            } else {
                body.body.push(row);
            }
        }
        table.bodies.push(body);
        Ok(Some(Block::Table(Box::new(table))))
    }

    fn media_table_sources(
        &mut self,
        scan: &Scan<'_>,
        opener_end: usize,
        close: usize,
    ) -> Result<TableSource> {
        let mut sources = TableSource::default();
        let mut cells = Vec::new();
        let mut heading = false;
        let mut multiline = false;
        let mut explicit_row = false;
        let mut region_end = 0;
        let mut at = (opener_end + 1).min(close);
        while at < close {
            let rescan = at < region_end;
            let mut stop = at;
            while stop < close && scan.text.as_bytes()[stop] != b'\n' {
                let next = scan.table_skip(stop, close);
                if let Some(region) = scan.region(stop, close)
                    && region.end > next
                {
                    region_end = region_end.max(region.end);
                }
                stop = next;
            }
            let line = &scan.text[at..stop];
            let trim = line.trim_start_matches([' ', '\t']);
            let first = stop - trim.len();
            if trim.starts_with("{|")
                && let Some(&nested_close) = scan
                    .tables
                    .get(&first)
                    .filter(|&&nested_close| nested_close < close)
            {
                let stop = line_end(scan.text, nested_close + 2).min(close);
                extend_cell(&mut cells, stop);
                multiline = true;
                at = stop.saturating_add(1).min(close);
                continue;
            }
            if trim.starts_with("|-") {
                if !cells.is_empty() || explicit_row {
                    sources.rows.push((heading, std::mem::take(&mut cells)));
                }
                explicit_row = true;
            } else if trim.starts_with("|+") {
                sources.caption = Some((first + 2, stop));
            } else if trim.starts_with(['|', '!']) {
                let field = &trim[1..];
                if !field.contains('|')
                    && let Some(rest) = after_cell_attribute(field)
                {
                    if cells.is_empty() {
                        let offset = stop - rest.len();
                        let token = scan.text[offset..].chars().next().unwrap_or('\n');
                        return Err(self.unexpected(scan.text, offset, &format!("{token:?}")));
                    }
                    extend_cell(&mut cells, stop);
                    at = stop.saturating_add(1).min(close);
                    continue;
                }
                if cells.is_empty() {
                    heading = trim.starts_with('!');
                }
                let mut physical_end = line_end(scan.text, first).min(stop);
                let mut at = first + 1;
                let mut cell_at = at;
                while at < stop {
                    if at == cell_at
                        && let Some(prefix) = cell_attributes(&scan.text[at..stop])
                    {
                        at += prefix.len() + 1;
                    } else if scan.text[at..].starts_with("{{")
                        && let Some(r) = scan.region(at, stop)
                        && r.end > physical_end
                    {
                        at = r.end;
                        physical_end = line_end(scan.text, at).min(stop);
                    } else if scan.text[at..stop].starts_with("||")
                        || trim.starts_with('!') && scan.text[at..stop].starts_with("!!")
                    {
                        cells.push(CellSource {
                            start: cell_at,
                            first_end: at,
                            end: at,
                            rescan,
                        });
                        at += 2;
                        cell_at = at;
                    } else {
                        at += char_len(&scan.text[at..stop]);
                    }
                }
                cells.push(CellSource {
                    start: cell_at,
                    first_end: stop,
                    end: stop,
                    rescan,
                });
            } else if !trim.trim().is_empty() {
                extend_cell(&mut cells, stop);
            }
            at = stop.saturating_add(1).min(close);
        }
        if !cells.is_empty() || explicit_row {
            sources.rows.push((heading, cells));
        }
        sources.multiline = multiline;
        Ok(sources)
    }

    fn media_row(&mut self, scan: &Scan<'_>, sources: &[CellSource], depth: usize) -> Result<Row> {
        let mut cells = Vec::with_capacity(sources.len());
        for source in sources {
            let text = &scan.text[source.start..source.first_end];
            let (mut attr, start) = if let Some(prefix) = cell_attributes(text) {
                (attrs(prefix), source.start + prefix.len() + 1)
            } else {
                (Default::default(), source.start)
            };
            let align = match attr.get("align") {
                Some("left") => Alignment::AlignLeft,
                Some("right") => Alignment::AlignRight,
                Some("center") => Alignment::AlignCenter,
                _ => Alignment::AlignDefault,
            };
            let row_span = span(attr.get("rowspan"))?;
            let col_span = span(attr.get("colspan"))?;
            self.charge_cells(
                row_span
                    .checked_mul(col_span)
                    .and_then(|area| area.checked_sub(1))
                    .ok_or_else(cell_error)?,
            )?;
            attr.attributes
                .retain(|(key, _)| !matches!(key.as_str(), "align" | "rowspan" | "colspan"));
            let mut content = if source.rescan {
                // The table split a region which hid markup from the document scan.
                let cell = Scan::new(&scan.text[start..source.end], Wiki::Media);
                self.media_cell_content(&cell, 0, source.first_end - start, cell.text.len(), depth)?
            } else {
                self.media_cell_content(scan, start, source.first_end, source.end, depth)?
            };
            if source.first_end < source.end
                && let [Block::Para(items)] = content.as_mut_slice()
            {
                content = vec![Block::Plain(std::mem::take(items))];
            }
            if let [Block::Plain(items)] = content.as_mut_slice()
                && matches!(items.as_slice(), [Inline::Image(..)])
            {
                let items = std::mem::take(items);
                content.clear();
                append_paragraph(Wiki::Media, &mut content, items);
            }
            cells.push(Cell {
                attr,
                align,
                row_span: row_span as i64,
                col_span: col_span as i64,
                content,
            });
        }
        Ok(Row {
            cells,
            ..Row::default()
        })
    }

    fn media_cell_content(
        &mut self,
        scan: &Scan<'_>,
        start: usize,
        first_end: usize,
        end: usize,
        depth: usize,
    ) -> Result<Vec<Block>> {
        let line = scan.text[start..first_end].trim_start();
        let first = first_end - line.len();
        let stop = cell_content_end(scan, first, end);
        if stop == first && first == end {
            return Ok(Vec::new());
        }
        if first_end < end {
            let allowance = if line.starts_with(['*', '#', ';', ':']) {
                start
            } else {
                first
            };
            self.block_range(scan, allowance, stop, depth)
        } else if (line.starts_with("{{")
            || scan.tags.get(&first).is_some_and(|tag| {
                is_code_block_tag(tag.name)
                    || tag.name.eq_ignore_ascii_case("blockquote")
                    || tag.name.eq_ignore_ascii_case("div")
            }))
            && scan.region(first, stop).is_some()
        {
            self.block_range(scan, first, stop, depth)
        } else {
            let items = self.inlines(scan, start, stop, depth)?;
            Ok(if items.is_empty() {
                vec![]
            } else {
                vec![Block::Plain(items)]
            })
        }
    }

    fn normalize_row(
        &mut self,
        mut row: Row,
        occupancy: &mut [usize],
        index: usize,
        group_end: usize,
    ) -> Result<Row> {
        let mut cells = Vec::new();
        let mut column = 0;
        for mut cell in row.cells {
            while column < occupancy.len() && occupancy[column] > index {
                column += 1;
            }
            if column == occupancy.len() {
                break;
            }
            let span = (cell.col_span as usize).min(occupancy.len() - column);
            let span = occupancy[column..column + span]
                .iter()
                .take_while(|&&end| end <= index)
                .count();
            let height = (cell.row_span as usize).min(group_end - index);
            occupancy[column..column + span].fill(index + height);
            column += span;
            cell.col_span = span as i64;
            cell.row_span = height as i64;
            cells.push(cell);
        }
        let padding = occupancy[column..]
            .iter()
            .filter(|&&end| end <= index)
            .count();
        self.charge_cells(padding)?;
        cells.resize(cells.len() + padding, Cell::default());
        row.cells = cells;
        Ok(row)
    }

    pub(crate) fn pipe_table(
        &mut self,
        scan: &Scan<'_>,
        start: usize,
        end: usize,
        depth: usize,
    ) -> Result<Option<(Block, usize)>> {
        self.depth(depth)?;
        let mut table = Table::default();
        let mut rows = Vec::new();
        let mut at = start;
        while at < end {
            let stop = line_end(scan.text, at).min(end);
            let line = scan.text[at..stop].trim();
            let first = at + (line.as_ptr() as usize - scan.text[at..stop].as_ptr() as usize);
            let Some(marker) = line
                .chars()
                .next()
                .filter(|&c| c == '|' || self.wiki == Wiki::Doku && c == '^')
            else {
                break;
            };
            let jira_head = self.wiki == Wiki::Jira && line.starts_with("||");
            let offset = if jira_head { 2 } else { 1 };
            let mut cursor = first + offset;
            let mut fields = Vec::new();
            let mut from = cursor;
            let mut all_headers = marker == '^' || jira_head || self.wiki == Wiki::Creole;
            while cursor < first + line.len() {
                let tail = &scan.text[cursor..first + line.len()];
                if let Some(r) = scan.region(cursor, first + line.len()) {
                    cursor = r.end;
                } else if tail.starts_with('|') || self.wiki == Wiki::Doku && tail.starts_with('^')
                {
                    fields.push((from, cursor));
                    if self.wiki == Wiki::Creole {
                        all_headers &= scan.text[from..cursor].starts_with('=');
                    }
                    let header = self.wiki == Wiki::Jira && tail.starts_with("||");
                    cursor += if header { 2 } else { 1 };
                    if self.wiki == Wiki::Jira && cursor < first + line.len() {
                        all_headers &= header;
                    }
                    from = cursor;
                } else {
                    cursor = scan.skip(cursor, first + line.len());
                }
            }
            if self.wiki == Wiki::Creole && from < first + line.len() {
                all_headers &= scan.text[from..first + line.len()].starts_with('=');
                fields.push((from, first + line.len()));
            }
            if fields.is_empty() {
                break;
            }
            let head = all_headers && rows.is_empty();
            let mut cells = Vec::with_capacity(fields.len());
            for (from, to) in fields {
                if rows.is_empty() {
                    let text = &scan.text[from..to];
                    let align = if self.wiki == Wiki::Doku {
                        match (text.starts_with("  "), text.ends_with("  ")) {
                            (true, true) => Alignment::AlignCenter,
                            (true, false) => Alignment::AlignRight,
                            (false, true) => Alignment::AlignLeft,
                            _ => Alignment::AlignDefault,
                        }
                    } else {
                        Alignment::AlignDefault
                    };
                    table.col_specs.push(ColSpec {
                        align,
                        ..Default::default()
                    });
                }
                let from = from + usize::from(head && self.wiki == Wiki::Creole);
                let items = self.inlines(scan, from, to, depth)?;
                let content = if items.is_empty() {
                    vec![]
                } else {
                    vec![if self.wiki == Wiki::Jira {
                        Block::Para(items)
                    } else {
                        Block::Plain(items)
                    }]
                };
                cells.push(Cell {
                    content,
                    ..Cell::default()
                });
            }
            rows.push((
                head,
                Row {
                    cells,
                    ..Row::default()
                },
            ));
            at = stop.saturating_add(1).min(end);
        }
        if rows.is_empty() {
            return Ok(None);
        }
        let width = table.col_specs.len();
        if self.wiki == Wiki::Creole && !rows[0].0 {
            self.charge_cells(width)?;
            table.head.rows.push(Row {
                cells: vec![Cell::default(); width],
                ..Row::default()
            });
        }
        let mut body = TableBody::default();
        for (head, mut row) in rows {
            self.charge_cells(width.saturating_sub(row.cells.len()))?;
            row.cells.resize(width, Cell::default());
            if head {
                table.head.rows.push(row);
            } else {
                body.body.push(row);
            }
        }
        table.bodies.push(body);
        Ok(Some((Block::Table(Box::new(table)), at)))
    }
}

fn plain_cell_block(block: &Block) -> bool {
    let Block::Plain(items) = block else {
        return false;
    };
    let mut pending: Vec<&Inline> = items.iter().collect();
    while let Some(item) = pending.pop() {
        match item {
            Inline::Note(blocks) => {
                for block in blocks {
                    if let Block::Plain(items) = block {
                        pending.extend(items);
                    } else {
                        return false;
                    }
                }
            }
            Inline::Emph(items)
            | Inline::Strong(items)
            | Inline::Underline(items)
            | Inline::Strikeout(items)
            | Inline::Superscript(items)
            | Inline::Subscript(items)
            | Inline::SmallCaps(items)
            | Inline::Cite(_, items)
            | Inline::Quoted(_, items)
            | Inline::Link(_, items, _)
            | Inline::Image(_, items, _)
            | Inline::Span(_, items) => pending.extend(items),
            _ => {}
        }
    }
    true
}

fn after_cell_attribute(text: &str) -> Option<&str> {
    let field = text.trim_start();
    let name_end = field
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':'))
        .count();
    if name_end == 0 || !field.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let value = field[name_end..]
        .trim_start()
        .strip_prefix('=')?
        .trim_start();
    let rest = if let Some(quote @ ('"' | '\'')) = value.chars().next() {
        value[1..].split_once(quote)?.1
    } else {
        &value[value
            .find(|ch: char| ch == '|' || ch.is_whitespace())
            .unwrap_or(value.len())..]
    };
    Some(rest.trim_start())
}

fn cell_attributes(text: &str) -> Option<&str> {
    let mut rest = text;
    while let Some(next) = after_cell_attribute(rest) {
        if next.starts_with('|') {
            return Some(&text[..text.len() - next.len()]);
        }
        rest = next;
    }
    None
}

fn percent_width(value: Option<&str>) -> Option<f64> {
    value?
        .strip_suffix('%')?
        .parse::<f64>()
        .ok()
        .filter(|width| width.is_finite() && *width >= 0.0)
        .map(|width| width / 100.0)
}

fn extend_cell(sources: &mut [CellSource], stop: usize) {
    if let Some(cell) = sources.last_mut() {
        cell.end = stop;
    }
}

fn cell_content_end(scan: &Scan<'_>, start: usize, end: usize) -> usize {
    let mut at = start;
    while at < end {
        if scan.tags.get(&at).is_some_and(|tag| {
            !tag.close
                && (is_code_block_tag(tag.name) || tag.name.eq_ignore_ascii_case("blockquote"))
        }) && scan.region(at, end).is_none()
        {
            return at;
        }
        at = scan.skip(at, end);
    }
    end
}

fn span(value: Option<&str>) -> Result<usize> {
    let Some(value) = value.filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    else {
        return Ok(1);
    };
    Ok(value.parse::<usize>().map_err(|_| cell_error())?.max(1))
}
