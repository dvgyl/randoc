//! The table parser of the Markdown reader: simple, multiline, grid and pipe tables.

use crate::block::SPACE;
use std::collections::BTreeSet;

use randoc_ast::{
    Alignment, Attr, Block, Caption, Cell, ColSpec, ColWidth, Inline, Row, Table, TableBody,
    TableFoot, TableHead,
};

/// The callbacks that the main parser gives to the table parser.
pub(crate) trait CellParser {
    /// Parses the inline content of a cell or a caption. The text is raw Markdown and can contain newlines.
    fn inlines(&mut self, text: &str) -> Vec<Inline>;
    /// Parses the block content of a grid table cell. The text is raw Markdown with the cell indentation removed.
    fn blocks(&mut self, text: &str) -> Vec<Block>;
    /// Parses an attribute text such as `{#id .class key=value}` (with the braces). Returns None when the text is not valid attributes.
    fn attributes(&mut self, text: &str) -> Option<Attr>;
}

/// The table extensions that are on, and the column count for relative widths.
pub(crate) struct TableOptions {
    /// The `simple_tables` extension.
    pub simple: bool,
    /// The `multiline_tables` extension.
    pub multiline: bool,
    /// The `grid_tables` extension.
    pub grid: bool,
    /// The `pipe_tables` extension.
    pub pipe: bool,
    /// The `table_captions` extension.
    pub captions: bool,
    /// The `table_attributes` extension.
    pub attributes: bool,
    /// `ReaderOptions::columns`, 72 by default.
    pub columns: usize,
}

/// Tries to parse a table at the start of `lines`.
/// `lines` are the remaining lines of the current block container. Each line has no newline at the end; tabs are already expanded to spaces.
/// A caption line before the table (`Table: ...` or `: ...`) is part of the table.
/// Returns the table block and the number of lines that it uses (including the caption and the blank line after a caption or a table when Pandoc uses it), or None when no table starts here.
pub(crate) fn parse_table(
    lines: &[&str],
    options: &TableOptions,
    parser: &mut dyn CellParser,
) -> Option<(Block, usize)> {
    let mut pos = 0;
    let mut caption_text = None;
    if options.captions
        && let Some((text, used)) = caption_at(lines)
    {
        let after = used + count_blank(rest(lines, used));
        if after == used {
            return None;
        }
        caption_text = Some(text);
        pos = after;
    }
    let (parts, used, grid) = table_parts(rest(lines, pos), options, parser)?;
    pos += used;
    pos += count_blank(rest(lines, pos));
    if caption_text.is_none()
        && options.captions
        && let Some((text, used)) = caption_at(rest(lines, pos))
    {
        caption_text = Some(text);
        pos += used;
        pos += count_blank(rest(lines, pos));
    }
    let (mut attr, caption) = build_caption(caption_text.as_deref(), options, parser);
    if grid {
        attr.attributes.push((GRID_MARK.into(), String::new()));
    }
    let empty_head = parts
        .head
        .iter()
        .all(|row| row.cells.iter().all(|cell| cell.content.is_empty()));
    let head_rows = if empty_head { Vec::new() } else { parts.head };
    let table = Table {
        attr,
        caption,
        col_specs: parts.col_specs,
        head: TableHead {
            attr: Attr::default(),
            rows: head_rows,
        },
        bodies: vec![TableBody {
            attr: Attr::default(),
            row_head_columns: 0,
            head: Vec::new(),
            body: parts.body,
        }],
        foot: TableFoot {
            attr: Attr::default(),
            rows: parts.foot,
        },
    };
    Some((Block::Table(Box::new(table)), pos))
}

/// The parts of a parsed table, before the caption is added.
struct Parts {
    /// One specification for each column.
    col_specs: Vec<ColSpec>,
    /// The head rows.
    head: Vec<Row>,
    /// The body rows.
    body: Vec<Row>,
    /// The foot rows.
    foot: Vec<Row>,
}

/// The hidden attribute of a grid table. A cell of a grid table holds blocks,
/// so an image alone in it is a figure; the resolve step removes the mark.
pub(crate) const GRID_MARK: &str = "\u{0}grid";

/// Tries each table kind in the order that gives the same choice as Pandoc.
/// The flag tells whether the table is a grid table.
fn table_parts(
    lines: &[&str],
    options: &TableOptions,
    parser: &mut dyn CellParser,
) -> Option<(Parts, usize, bool)> {
    if options.grid
        && let Some((parts, used)) = grid_table(lines, options, parser)
    {
        return Some((parts, used, true));
    }
    if options.multiline
        && let Some(found) = multiline_table(lines, options, parser, false)
    {
        return Some((found.0, found.1, false));
    }
    if options.simple
        && let Some(found) = simple_table(lines, parser)
    {
        return Some((found.0, found.1, false));
    }
    if options.multiline
        && let Some(found) = multiline_table(lines, options, parser, true)
    {
        return Some((found.0, found.1, false));
    }
    if options.pipe
        && let Some(found) = pipe_table(lines, options, parser)
    {
        return Some((found.0, found.1, false));
    }
    None
}

/// Returns the lines from `start`, or no lines when `start` is past the end.
fn rest<'a, 'b>(lines: &'a [&'b str], start: usize) -> &'a [&'b str] {
    lines.get(start..).unwrap_or(&[])
}

/// Returns true when the line has only spaces.
fn is_blank(line: &str) -> bool {
    line.trim_matches(SPACE).is_empty()
}

/// Counts the blank lines at the start of `lines`.
fn count_blank(lines: &[&str]) -> usize {
    lines.iter().take_while(|line| is_blank(line)).count()
}

/// Counts the leading spaces of a line.
fn indent(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

/// Finds a caption paragraph at the start of `lines`. Returns the caption text and the number of lines.
fn caption_at(lines: &[&str]) -> Option<(String, usize)> {
    let first = lines.first()?;
    if indent(first) > 3 {
        return None;
    }
    let body = first.trim_start_matches(SPACE);
    if body.starts_with(":::") {
        return None;
    }
    let text = body
        .strip_prefix("Table:")
        .or_else(|| body.strip_prefix("table:"))
        .or_else(|| body.strip_prefix(':'))?;
    let mut parts = vec![text.trim_matches(SPACE)];
    let mut used = 1;
    for line in rest(lines, 1) {
        if is_blank(line) {
            break;
        }
        parts.push(line.trim_matches(SPACE));
        used += 1;
    }
    Some((parts.join("\n").trim_matches(SPACE).to_string(), used))
}

/// Builds the table attributes and the caption from the caption text.
fn build_caption(
    text: Option<&str>,
    options: &TableOptions,
    parser: &mut dyn CellParser,
) -> (Attr, Caption) {
    let Some(text) = text else {
        return (Attr::default(), Caption::default());
    };
    let (text, attr) = if options.attributes {
        split_attributes(text, parser)
    } else {
        (text, Attr::default())
    };
    let caption = if text.is_empty() {
        Caption::default()
    } else {
        Caption {
            short: None,
            long: vec![Block::Plain(parser.inlines(text))],
        }
    };
    (attr, caption)
}

/// Splits valid `{...}` attributes from the end of a caption text.
fn split_attributes<'a>(text: &'a str, parser: &mut dyn CellParser) -> (&'a str, Attr) {
    if text.ends_with('}') {
        for (index, _) in text.match_indices('{') {
            let Some(candidate) = text.get(index..) else {
                continue;
            };
            if let Some(attr) = parser.attributes(candidate) {
                let before = text.get(..index).unwrap_or("").trim_end_matches(SPACE);
                return (before, attr);
            }
        }
    }
    (text, Attr::default())
}

/// Returns the display width of a character: 0, 1 or 2 terminal columns.
fn char_width(c: char) -> usize {
    let code = u32::from(c);
    let zero = [(0x0300, 0x036F), (0x200B, 0x200F), (0xFE00, 0xFE0F)];
    let wide = [
        (0x1100, 0x115F),
        (0x2E80, 0x303E),
        (0x3041, 0x33FF),
        (0x3400, 0x4DBF),
        (0x4E00, 0x9FFF),
        (0xA000, 0xA4CF),
        (0xAC00, 0xD7A3),
        (0xF900, 0xFAFF),
        (0xFE30, 0xFE4F),
        (0xFF00, 0xFF60),
        (0xFFE0, 0xFFE6),
        (0x1F300, 0x1F64F),
        (0x1F900, 0x1F9FF),
        (0x20000, 0x2FFFD),
        (0x30000, 0x3FFFD),
    ];
    if zero.iter().any(|(lo, hi)| (*lo..=*hi).contains(&code)) {
        0
    } else if wide.iter().any(|(lo, hi)| (*lo..=*hi).contains(&code)) {
        2
    } else {
        1
    }
}

/// Returns the display width of a text.
fn text_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

/// Returns the characters of `line` that start at a display column in `start..end` (no end when `end` is None).
fn slice_columns(line: &str, start: usize, end: Option<usize>) -> String {
    let mut column = 0;
    let mut out = String::new();
    for c in line.chars() {
        if column >= start && end.is_none_or(|end| column < end) {
            out.push(c);
        }
        column += char_width(c);
    }
    out
}

/// One column of a simple or multiline table, from its run of dashes.
#[derive(Clone, Copy)]
struct DashColumn {
    /// The display column where the dashes start.
    start: usize,
    /// The number of dashes.
    dashes: usize,
}

/// Parses a line of dash runs separated by spaces. Returns the runs, or None when the line is not a dashed line.
fn dashed_line(line: &str) -> Option<Vec<DashColumn>> {
    let line = line.trim_end_matches(SPACE);
    if indent(line) > 3 || line.is_empty() {
        return None;
    }
    let mut columns: Vec<DashColumn> = Vec::new();
    let mut previous = ' ';
    for (position, c) in line.chars().enumerate() {
        match c {
            '-' if previous == '-' => {
                if let Some(last) = columns.last_mut() {
                    last.dashes += 1;
                }
            }
            '-' => columns.push(DashColumn {
                start: position,
                dashes: 1,
            }),
            ' ' => {}
            _ => return None,
        }
        previous = c;
    }
    if columns.is_empty() {
        None
    } else {
        Some(columns)
    }
}

/// Returns true when the line is a dashed line.
fn is_dashed(line: &str) -> bool {
    dashed_line(line).is_some()
}

/// Returns the end display column of each column (the start of the next column), or None for the last column.
fn column_ends(columns: &[DashColumn]) -> Vec<Option<usize>> {
    let mut ends: Vec<Option<usize>> = columns
        .iter()
        .skip(1)
        .map(|column| Some(column.start))
        .collect();
    ends.push(None);
    ends
}

/// Splits a line into the raw text of each column.
fn split_columns(line: &str, columns: &[DashColumn]) -> Vec<String> {
    columns
        .iter()
        .zip(column_ends(columns))
        .map(|(column, end)| slice_columns(line, column.start, end))
        .collect()
}

/// Finds the alignment of a column from its raw text lines and its dash count.
fn column_alignment(texts: &[&str], dashes: usize) -> Alignment {
    let texts: Vec<&str> = texts
        .iter()
        .map(|text| text.trim_end_matches(SPACE))
        .filter(|text| !text.is_empty())
        .collect();
    let left = texts.iter().any(|text| text.starts_with(' '));
    let right = texts.iter().any(|text| text_width(text) < dashes);
    match (left, right) {
        (true, true) => Alignment::AlignCenter,
        (false, true) => Alignment::AlignLeft,
        (true, false) => Alignment::AlignRight,
        (false, false) => Alignment::AlignDefault,
    }
}

/// Finds the alignment of each column from the lines of the header (or of the first row).
fn alignments(lines: &[&str], columns: &[DashColumn]) -> Vec<Alignment> {
    let split: Vec<Vec<String>> = lines
        .iter()
        .map(|line| split_columns(line, columns))
        .collect();
    columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let texts: Vec<&str> = split
                .iter()
                .filter_map(|parts| parts.get(index).map(String::as_str))
                .collect();
            column_alignment(&texts, column.dashes)
        })
        .collect()
}

/// Makes a cell with a `Plain` block of the inlines, or no block for an empty text.
fn inline_cell(text: &str, parser: &mut dyn CellParser) -> Cell {
    let text = text.trim_matches(SPACE);
    let content = if text.is_empty() {
        Vec::new()
    } else {
        vec![Block::Plain(parser.inlines(text))]
    };
    Cell {
        content,
        ..Cell::default()
    }
}

/// Makes a row from the lines of a simple or multiline row. The lines of a column are joined with newlines.
fn line_row(lines: &[&str], columns: &[DashColumn], parser: &mut dyn CellParser) -> Row {
    let split: Vec<Vec<String>> = lines
        .iter()
        .map(|line| split_columns(line, columns))
        .collect();
    let cells = (0..columns.len())
        .map(|index| {
            let parts: Vec<&str> = split
                .iter()
                .filter_map(|parts| parts.get(index).map(|part| part.trim_matches(SPACE)))
                .filter(|part| !part.is_empty())
                .collect();
            inline_cell(&parts.join("\n"), parser)
        })
        .collect();
    Row {
        attr: Attr::default(),
        cells,
    }
}

/// Parses a simple table, with a header line or without one.
fn simple_table(lines: &[&str], parser: &mut dyn CellParser) -> Option<(Parts, usize)> {
    let first = lines.first()?;
    let (header, columns, mut pos) = match dashed_line(first) {
        Some(columns) => (None, columns, 1),
        None => {
            if is_blank(first) || indent(first) > 3 {
                return None;
            }
            (Some(*first), dashed_line(lines.get(1)?)?, 2)
        }
    };
    let rows_start = pos;
    let mut footer = false;
    while let Some(line) = lines.get(pos) {
        if is_blank(line) {
            break;
        }
        pos += 1;
        if is_dashed(line) {
            footer = true;
            break;
        }
    }
    let row_end = if footer { pos - 1 } else { pos };
    let rows = lines.get(rows_start..row_end)?;
    if rows.is_empty() || (header.is_none() && !footer) {
        return None;
    }
    let align_lines: Vec<&str> = match header {
        Some(line) => vec![line],
        None => rows.iter().take(1).copied().collect(),
    };
    let col_specs = alignments(&align_lines, &columns)
        .into_iter()
        .map(|align| ColSpec {
            align,
            width: ColWidth::ColWidthDefault,
        })
        .collect();
    let head = header
        .map(|line| line_row(&[line], &columns, parser))
        .into_iter()
        .collect();
    let body = rows
        .iter()
        .map(|line| line_row(&[line], &columns, parser))
        .collect();
    let parts = Parts {
        col_specs,
        head,
        body,
        foot: Vec::new(),
    };
    Some((parts, pos))
}

/// Parses a multiline table. With `headless`, the first dashed line gives the columns and there is no header.
fn multiline_table(
    lines: &[&str],
    options: &TableOptions,
    parser: &mut dyn CellParser,
    headless: bool,
) -> Option<(Parts, usize)> {
    let first_columns = dashed_line(lines.first()?)?;
    let mut pos = 1;
    let mut header_lines: Vec<&str> = Vec::new();
    let columns = if headless {
        first_columns
    } else {
        while let Some(line) = lines.get(pos) {
            if is_blank(line) {
                return None;
            }
            if is_dashed(line) {
                break;
            }
            header_lines.push(line);
            pos += 1;
        }
        let separator = dashed_line(lines.get(pos)?)?;
        if header_lines.is_empty() {
            return None;
        }
        pos += 1;
        separator
    };
    let mut rows: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    loop {
        let line = lines.get(pos)?;
        pos += 1;
        if is_dashed(line) {
            break;
        }
        if is_blank(line) {
            if !current.is_empty() {
                rows.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        rows.push(current);
    }
    if rows.is_empty() {
        return None;
    }
    let align_lines = if headless {
        rows.first().cloned().unwrap_or_default()
    } else {
        header_lines.clone()
    };
    let widths = multiline_widths(lines.first().map_or(0, |l| indent(l)), &columns, options);
    let col_specs = alignments(&align_lines, &columns)
        .into_iter()
        .zip(widths)
        .map(|(align, width)| ColSpec {
            align,
            width: ColWidth::ColWidth(width),
        })
        .collect();
    let head = if headless {
        Vec::new()
    } else {
        vec![line_row(&header_lines, &columns, parser)]
    };
    let body = rows
        .iter()
        .map(|row| line_row(row, &columns, parser))
        .collect();
    let parts = Parts {
        col_specs,
        head,
        body,
        foot: Vec::new(),
    };
    Some((parts, pos))
}

/// Computes the relative column widths of a multiline table.
/// A column is as wide as its dashes and the spaces after them; the last column gets one extra space.
fn multiline_widths(
    first_indent: usize,
    columns: &[DashColumn],
    options: &TableOptions,
) -> Vec<f64> {
    let mut lengths: Vec<usize> = columns
        .iter()
        .zip(column_ends(columns))
        .map(|(column, end)| match end {
            Some(end) => end.saturating_sub(column.start),
            None => column.dashes + 1,
        })
        .collect();
    let line_end = first_indent + lengths.iter().sum::<usize>();
    // A last column that is up to 2 columns narrower than the column before it gets the same width.
    if let [.., before, last] = lengths.as_mut_slice()
        && *last < *before
        && *before - *last <= 2
    {
        *last = *before;
    }
    let total: usize = lengths.iter().sum();
    let available = options.columns.max(line_end);
    let quotient = if total > available { total } else { available };
    lengths
        .iter()
        .map(|length| *length as f64 / quotient as f64)
        .collect()
}

/// Splits a pipe table row at the pipes that are not escaped and not in a code span.
/// Returns None when the line has no such pipe.
fn split_pipes(line: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = line.chars().collect();
    let mut cells = vec![String::new()];
    let mut pipes = 0;
    let mut index = 0;
    while let Some(&c) = chars.get(index) {
        let mut take = 1;
        if c == '\\' {
            take = if chars.get(index + 1).is_some() { 2 } else { 1 };
        } else if c == '`' {
            let run = chars.iter().skip(index).take_while(|c| **c == '`').count();
            take = run;
            if let Some(close) = closing_backticks(&chars, index + run, run) {
                take = close + run - index;
            }
        } else if c == '|' {
            pipes += 1;
            cells.push(String::new());
            index += 1;
            continue;
        }
        if let Some(cell) = cells.last_mut() {
            cell.extend(chars.iter().skip(index).take(take));
        }
        index += take;
    }
    if pipes == 0 {
        return None;
    }
    if cells.first().is_some_and(|cell| is_blank(cell)) {
        cells.remove(0);
    }
    if cells.last().is_some_and(|cell| is_blank(cell)) {
        cells.pop();
    }
    Some(cells)
}

/// Finds the start of a run of exactly `run` backticks at or after `from`.
fn closing_backticks(chars: &[char], from: usize, run: usize) -> Option<usize> {
    let mut index = from;
    while index < chars.len() {
        let length = chars.iter().skip(index).take_while(|c| **c == '`').count();
        if length == run {
            return Some(index);
        }
        index += length.max(1);
    }
    None
}

/// Parses the separator line of a pipe table. Returns the alignment and the width (dashes and colons) of each column.
fn pipe_separator(line: &str) -> Option<Vec<(Alignment, usize)>> {
    if !line.contains('|') || !line.contains('-') {
        return None;
    }
    if !line
        .chars()
        .all(|c| matches!(c, ' ' | '|' | '+' | ':' | '-'))
    {
        return None;
    }
    let mut parts: Vec<&str> = line.split(['|', '+']).collect();
    if parts.first().is_some_and(|part| is_blank(part)) {
        parts.remove(0);
    }
    if parts.last().is_some_and(|part| is_blank(part)) {
        parts.pop();
    }
    parts
        .iter()
        .map(|part| {
            let part = part.trim_matches(SPACE);
            let left = part.starts_with(':');
            let right = part.ends_with(':') && part.len() > 1;
            let dashes = part.trim_matches(':');
            if dashes.is_empty() || !dashes.chars().all(|c| c == '-') {
                return None;
            }
            let align = match (left, right) {
                (true, true) => Alignment::AlignCenter,
                (true, false) => Alignment::AlignLeft,
                (false, true) => Alignment::AlignRight,
                (false, false) => Alignment::AlignDefault,
            };
            Some((align, part.chars().count()))
        })
        .collect()
}

/// Makes a pipe table row with exactly `count` cells: missing cells are empty and extra cells are dropped.
fn pipe_row(cells: &[String], count: usize, parser: &mut dyn CellParser) -> Row {
    let cells = (0..count)
        .map(|index| inline_cell(cells.get(index).map_or("", String::as_str), parser))
        .collect();
    Row {
        attr: Attr::default(),
        cells,
    }
}

/// Parses a pipe table.
fn pipe_table(
    lines: &[&str],
    options: &TableOptions,
    parser: &mut dyn CellParser,
) -> Option<(Parts, usize)> {
    let header_line = lines.first()?;
    if indent(header_line) > 3 {
        return None;
    }
    let header = split_pipes(header_line)?;
    let separator_line = lines.get(1)?;
    let separator = pipe_separator(separator_line)?;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut pos = 2;
    while let Some(line) = lines.get(pos) {
        if is_blank(line) {
            break;
        }
        let Some(cells) = split_pipes(line) else {
            break;
        };
        rows.push(cells);
        pos += 1;
    }
    let used = lines.get(..pos).unwrap_or(&[]);
    let wide = used
        .iter()
        .any(|line| line.chars().count() > options.columns);
    let total: usize = separator.iter().map(|(_, width)| width).sum();
    let col_specs = separator
        .iter()
        .map(|(align, width)| ColSpec {
            align: *align,
            width: if wide && total > 0 {
                ColWidth::ColWidth(*width as f64 / total as f64)
            } else {
                ColWidth::ColWidthDefault
            },
        })
        .collect();
    let count = separator.len();
    let head = vec![pipe_row(&header, count, parser)];
    let body = rows
        .iter()
        .map(|cells| pipe_row(cells, count, parser))
        .collect();
    let parts = Parts {
        col_specs,
        head,
        body,
        foot: Vec::new(),
    };
    Some((parts, pos))
}

/// The marker for the second display column of a wide character in the grid.
const WIDE_FILL: char = '\0';

/// The characters of a grid table, one entry for each display column.
struct Grid {
    /// The lines of the table, with the table indentation removed.
    rows: Vec<Vec<char>>,
    /// For each line, true when `=` and `:` count as `-` in the line: the top line and each `=` separator line.
    rules: Vec<bool>,
}

/// A traced cell: its bottom line and right column, and the row and column borders that the trace found.
struct Trace {
    /// The line of the bottom border.
    bottom: usize,
    /// The display column of the right border.
    right: usize,
    /// The lines where a `+` is on the left or the right border of the cell.
    row_borders: Vec<usize>,
    /// The display columns where a `+` is on the top or the bottom border of the cell.
    col_borders: Vec<usize>,
}

impl Grid {
    /// Returns the character at a line and a display column, or None past the end of the line.
    fn at(&self, y: usize, x: usize) -> Option<char> {
        self.rows.get(y).and_then(|row| row.get(x)).copied()
    }

    /// Returns true when the position is a `-` of a horizontal border.
    fn is_horizontal(&self, y: usize, x: usize) -> bool {
        match self.at(y, x) {
            Some('-') => true,
            Some('=' | ':') => self.rules.get(y).copied().unwrap_or(false),
            _ => false,
        }
    }

    /// Traces the cell with the top-left corner at (`top`, `left`): right along the top border, down the right border, left along the bottom border and up the left border.
    fn scan_cell(&self, top: usize, left: usize) -> Option<Trace> {
        if self.at(top, left) != Some('+') {
            return None;
        }
        let width = self.rows.get(top).map_or(0, Vec::len);
        let mut col_borders = Vec::new();
        for right in left + 1..width {
            if self.at(top, right) == Some('+') {
                col_borders.push(right);
                if let Some(mut trace) = self.scan_down(top, left, right) {
                    trace.col_borders.extend(col_borders);
                    return Some(trace);
                }
            } else if !self.is_horizontal(top, right) {
                return None;
            }
        }
        None
    }

    /// Follows the right border down from a corner until a bottom border closes the cell. A position past the end of the line counts as a vertical border.
    fn scan_down(&self, top: usize, left: usize, right: usize) -> Option<Trace> {
        let mut row_borders = Vec::new();
        for bottom in top + 1..self.rows.len() {
            match self.at(bottom, right) {
                Some('+') => {
                    row_borders.push(bottom);
                    if let Some((col_borders, left_rows)) = self.scan_left(top, left, bottom, right)
                    {
                        row_borders.extend(left_rows);
                        return Some(Trace {
                            bottom,
                            right,
                            row_borders,
                            col_borders,
                        });
                    }
                }
                Some('|') | None => {}
                Some(_) => return None,
            }
        }
        None
    }

    /// Checks the bottom border and the left border of a cell. Returns the column borders of the bottom border and the row borders of the left border.
    fn scan_left(
        &self,
        top: usize,
        left: usize,
        bottom: usize,
        right: usize,
    ) -> Option<(Vec<usize>, Vec<usize>)> {
        let mut col_borders = Vec::new();
        for x in left + 1..right {
            if self.at(bottom, x) == Some('+') {
                col_borders.push(x);
            } else if !self.is_horizontal(bottom, x) {
                return None;
            }
        }
        if self.at(bottom, left) != Some('+') {
            return None;
        }
        let mut row_borders = Vec::new();
        for y in top + 1..bottom {
            match self.at(y, left) {
                Some('+') => row_borders.push(y),
                Some('|') => {}
                _ => return None,
            }
        }
        Some((col_borders, row_borders))
    }

    /// Returns the text inside a cell, one line for each grid line, without trailing spaces.
    fn cell_text(&self, cell: &GridCell) -> String {
        let lines: Vec<String> = (cell.top + 1..cell.bottom)
            .map(|y| {
                let text: String = (cell.left + 1..cell.right)
                    .filter_map(|x| self.at(y, x))
                    .filter(|c| *c != WIDE_FILL)
                    .collect();
                text.trim_end_matches(SPACE).to_string()
            })
            .collect();
        let all_indented = lines
            .iter()
            .all(|line| line.is_empty() || line.starts_with(' '));
        let lines: Vec<&str> = lines
            .iter()
            .map(|line| {
                if all_indented {
                    line.strip_prefix(' ').unwrap_or(line)
                } else {
                    line
                }
            })
            .collect();
        lines.join("\n")
    }
}

/// The corners of one grid table cell, as line indexes and display columns.
struct GridCell {
    /// The line of the top border.
    top: usize,
    /// The display column of the left border.
    left: usize,
    /// The line of the bottom border.
    bottom: usize,
    /// The display column of the right border.
    right: usize,
}

/// Returns true when the text (without the indentation) is a grid border line such as `+---+===+`.
fn is_grid_border(text: &str) -> bool {
    let text = text.trim_end_matches(SPACE);
    text.len() > 1
        && text.starts_with('+')
        && text.ends_with('+')
        && text.chars().all(|c| matches!(c, '+' | '-' | '=' | ':'))
        && text.chars().any(|c| matches!(c, '-' | '='))
}

/// Returns true when the text is a border line of `fill` characters with optional alignment colons at the segment ends, such as `+:---+---:+` for `-`.
fn is_rule_line(text: &str, fill: char) -> bool {
    let Some(inner) = text.trim_end_matches(SPACE).strip_prefix('+') else {
        return false;
    };
    let Some(segments) = inner.strip_suffix('+') else {
        return false;
    };
    segments.split('+').all(|segment| {
        let segment = segment.strip_prefix(':').unwrap_or(segment);
        let segment = segment.strip_suffix(':').unwrap_or(segment);
        !segment.is_empty() && segment.chars().all(|c| c == fill)
    })
}

/// Parses a grid table.
fn grid_table(
    lines: &[&str],
    options: &TableOptions,
    parser: &mut dyn CellParser,
) -> Option<(Parts, usize)> {
    let first = lines.first()?;
    let margin = indent(first);
    if margin > 3 {
        return None;
    }
    let strip = |line: &str| -> Option<String> {
        if indent(line) < margin {
            return None;
        }
        let text: String = line.chars().skip(margin).collect();
        (text.starts_with('+') || text.starts_with('|')).then_some(text)
    };
    let top_line = strip(first)?;
    if !is_rule_line(&top_line, '-') {
        return None;
    }
    let texts: Vec<String> = lines.iter().map_while(|line| strip(line)).collect();
    let last_border = texts.iter().rposition(|text| is_grid_border(text))?;
    if last_border == 0 {
        return None;
    }
    let texts = texts.get(..=last_border)?;
    let grid = Grid {
        rows: texts
            .iter()
            .map(|text| {
                let mut row = Vec::new();
                for c in text.chars() {
                    row.push(c);
                    row.extend(std::iter::repeat_n(
                        WIDE_FILL,
                        char_width(c).saturating_sub(1),
                    ));
                }
                row
            })
            .collect(),
        rules: texts
            .iter()
            .enumerate()
            .map(|(y, text)| y == 0 || is_rule_line(text, '='))
            .collect(),
    };
    let (cells, ys, xs) = grid_cells(&grid);
    if cells.is_empty() {
        return None;
    }
    let position = |list: &[usize], value: usize| list.binary_search(&value).unwrap_or(0);

    // The parts come from the `=` borders: the first one ends the head, and the two last ones enclose the foot.
    let separators: Vec<usize> = texts
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, text)| is_rule_line(text, '='))
        .map(|(y, _)| y)
        .collect();
    let head_end = separators.first().copied();
    let foot_start = match separators.as_slice() {
        [.., before, last] if *last == last_border => Some(*before),
        _ => None,
    };

    let alignment_line = head_end
        .and_then(|y| grid.rows.get(y))
        .or(grid.rows.first());
    let col_specs = grid_col_specs(&xs, alignment_line.map_or(&[][..], Vec::as_slice), options);

    let mut built: Vec<(usize, usize, Cell)> = cells
        .iter()
        .map(|cell| {
            let text = grid.cell_text(cell);
            let content = if is_blank(&text) {
                Vec::new()
            } else {
                parser.blocks(&text)
            };
            let row = position(&ys, cell.top);
            let left = position(&xs, cell.left);
            let built = Cell {
                row_span: span(position(&ys, cell.bottom), row),
                col_span: span(position(&xs, cell.right), left),
                content,
                ..Cell::default()
            };
            (row, left, built)
        })
        .collect();
    // Pandoc fills each grid position that no cell covers with an empty cell.
    let width = xs.len().saturating_sub(1);
    let mut covered = vec![false; ys.len().saturating_sub(1) * width];
    for (row, left, cell) in &built {
        let rows = usize::try_from(cell.row_span).unwrap_or(1);
        let cols = usize::try_from(cell.col_span).unwrap_or(1);
        for y in *row..row + rows {
            for x in *left..left + cols {
                if let Some(slot) = covered.get_mut(y * width + x) {
                    *slot = true;
                }
            }
        }
    }
    for (index, _) in covered.iter().enumerate().filter(|(_, slot)| !**slot) {
        built.push((index / width.max(1), index % width.max(1), Cell::default()));
    }
    built.sort_by_key(|(row, left, _)| (*row, *left));
    // Pandoc keeps paragraphs only when some cell has more than a single paragraph.
    let single_paragraphs = built.iter().all(|(_, _, cell)| {
        matches!(
            cell.content.as_slice(),
            [] | [Block::Para(_)] | [Block::Plain(_)]
        )
    });
    if single_paragraphs {
        for (_, _, cell) in &mut built {
            for block in &mut cell.content {
                if let Block::Para(inlines) = block {
                    *block = Block::Plain(std::mem::take(inlines));
                }
            }
        }
    }
    let mut rows: Vec<Row> = (0..ys.len().saturating_sub(1))
        .map(|_| Row::default())
        .collect();
    for (row, _, cell) in built {
        if let Some(target) = rows.get_mut(row) {
            target.cells.push(cell);
        }
    }

    let mut head = Vec::new();
    let mut body = Vec::new();
    let mut foot = Vec::new();
    for (index, row) in rows.into_iter().enumerate() {
        let top = ys.get(index).copied().unwrap_or(0);
        if head_end.is_some_and(|end| top < end) {
            head.push(row);
        } else if foot_start.is_some_and(|start| top >= start) {
            foot.push(row);
        } else {
            body.push(row);
        }
    }
    let parts = Parts {
        col_specs,
        head,
        body,
        foot,
    };
    Some((parts, last_border + 1))
}

/// Returns the number of boundaries between two boundary indexes, at least 1.
fn span(end: usize, start: usize) -> i64 {
    i64::try_from(end.saturating_sub(start).max(1)).unwrap_or(1)
}

/// Finds all cells of a grid, sorted by top line and then by left column.
/// Also returns the sorted row border lines and column border columns. These come from each `+` on a traced cell border, also where no cell starts or ends.
fn grid_cells(grid: &Grid) -> (Vec<GridCell>, Vec<usize>, Vec<usize>) {
    let mut corners: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut row_borders: BTreeSet<usize> = BTreeSet::from([0]);
    let mut col_borders: BTreeSet<usize> = BTreeSet::from([0]);
    // For each display column, the line above which cells already cover the column.
    let mut done: Vec<usize> = vec![0; grid.rows.first().map_or(0, Vec::len) + 1];
    let mut cells = Vec::new();
    corners.insert((0, 0));
    while let Some((top, left)) = corners.pop_first() {
        if done.get(left).is_none_or(|covered| top < *covered) {
            continue;
        }
        let Some(trace) = grid.scan_cell(top, left) else {
            continue;
        };
        row_borders.extend(trace.row_borders);
        col_borders.extend(trace.col_borders);
        for covered in done.iter_mut().take(trace.right).skip(left) {
            *covered = trace.bottom;
        }
        corners.insert((top, trace.right));
        corners.insert((trace.bottom, left));
        cells.push(GridCell {
            top,
            left,
            bottom: trace.bottom,
            right: trace.right,
        });
    }
    cells.sort_by_key(|cell| (cell.top, cell.left));
    (
        cells,
        row_borders.into_iter().collect(),
        col_borders.into_iter().collect(),
    )
}

/// Builds the column specifications of a grid table from the column borders and the alignment line.
fn grid_col_specs(xs: &[usize], alignment_line: &[char], options: &TableOptions) -> Vec<ColSpec> {
    let total = xs
        .last()
        .copied()
        .unwrap_or(0)
        .saturating_sub(xs.first().copied().unwrap_or(0));
    let quotient = options.columns.max(total).max(1) as f64;
    xs.windows(2)
        .map(|pair| {
            let (left, right) = match pair {
                [left, right] => (*left, *right),
                _ => (0, 0),
            };
            let left_colon = alignment_line.get(left + 1) == Some(&':');
            let right_colon = right > 0 && alignment_line.get(right - 1) == Some(&':');
            let align = match (left_colon, right_colon) {
                (true, true) => Alignment::AlignCenter,
                (true, false) => Alignment::AlignLeft,
                (false, true) => Alignment::AlignRight,
                (false, false) => Alignment::AlignDefault,
            };
            ColSpec {
                align,
                width: ColWidth::ColWidth((right - left) as f64 / quotient),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cell parser that splits words into `Str` and `Space` (or `SoftBreak` at a newline).
    struct WordParser;

    impl CellParser for WordParser {
        fn inlines(&mut self, text: &str) -> Vec<Inline> {
            let mut out = Vec::new();
            for (index, line) in text.lines().enumerate() {
                if index > 0 {
                    out.push(Inline::SoftBreak);
                }
                for (word_index, word) in line.split_whitespace().enumerate() {
                    if word_index > 0 {
                        out.push(Inline::Space);
                    }
                    out.push(Inline::Str(word.to_string()));
                }
            }
            out
        }

        fn blocks(&mut self, text: &str) -> Vec<Block> {
            text.split("\n\n")
                .filter(|chunk| !chunk.trim_matches(SPACE).is_empty())
                .map(|chunk| Block::Para(self.inlines(chunk.trim_matches(SPACE))))
                .collect()
        }

        fn attributes(&mut self, text: &str) -> Option<Attr> {
            let inner = text.strip_prefix('{')?.strip_suffix('}')?;
            let mut attr = Attr::default();
            for word in inner.split_whitespace() {
                if let Some(id) = word.strip_prefix('#') {
                    attr.id = id.to_string();
                } else if let Some(class) = word.strip_prefix('.') {
                    attr.classes.push(class.to_string());
                } else {
                    let (key, value) = word.split_once('=')?;
                    if key.is_empty() || value.is_empty() {
                        return None;
                    }
                    attr.attributes.push((key.to_string(), value.to_string()));
                }
            }
            Some(attr)
        }
    }

    fn options() -> TableOptions {
        TableOptions {
            simple: true,
            multiline: true,
            grid: true,
            pipe: true,
            captions: true,
            attributes: true,
            columns: 72,
        }
    }

    fn inline_text(inlines: &[Inline]) -> String {
        inlines
            .iter()
            .map(|inline| match inline {
                Inline::Str(text) => text.clone(),
                Inline::Space => " ".to_string(),
                Inline::SoftBreak => "/".to_string(),
                _ => "<other>".to_string(),
            })
            .collect()
    }

    fn block_text(block: &Block) -> String {
        match block {
            Block::Plain(inlines) => inline_text(inlines),
            Block::Para(inlines) => format!("P:{}", inline_text(inlines)),
            _ => "<block>".to_string(),
        }
    }

    fn rows_text(rows: &[Row]) -> String {
        rows.iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| {
                        let mut text: Vec<String> = cell.content.iter().map(block_text).collect();
                        let mut text = std::mem::take(&mut text).join(" ~ ");
                        if cell.row_span != 1 || cell.col_span != 1 {
                            text.push_str(&format!("{{{},{}}}", cell.row_span, cell.col_span));
                        }
                        text
                    })
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .collect::<Vec<_>>()
            .join(";")
    }

    /// Summarizes a table in the same format as the oracle script.
    fn summary(block: &Block) -> String {
        let Block::Table(table) = block else {
            return "not a table".to_string();
        };
        let cols = table
            .col_specs
            .iter()
            .map(|spec| {
                let align = match spec.align {
                    Alignment::AlignLeft => "L",
                    Alignment::AlignRight => "R",
                    Alignment::AlignCenter => "C",
                    Alignment::AlignDefault => "D",
                };
                let width = match spec.width {
                    ColWidth::ColWidth(width) => format!("{width:?}"),
                    ColWidth::ColWidthDefault => "D".to_string(),
                };
                format!("{align}:{width}")
            })
            .collect::<Vec<_>>()
            .join(",");
        let bodies = table
            .bodies
            .iter()
            .map(|body| rows_text(&body.body))
            .collect::<Vec<_>>()
            .join(";");
        let caption = table
            .caption
            .long
            .iter()
            .map(block_text)
            .collect::<Vec<_>>()
            .join(" ~ ");
        let attr = format!(
            "{}{}",
            table.attr.id,
            table
                .attr
                .classes
                .iter()
                .map(|class| format!(".{class}"))
                .collect::<String>()
        );
        format!(
            "cols={cols} head=[{}] body=[{bodies}] foot=[{}] cap=[{caption}] attr=[{attr}]",
            rows_text(&table.head.rows),
            rows_text(&table.foot.rows)
        )
    }

    /// The input, then the oracle summary and the line count, or None when Pandoc gives no table.
    /// The summaries come from `pandoc -f markdown -t json`.
    type Case = (&'static str, &'static str, Option<(&'static str, usize)>);

    /// The test cases.
    const CASES: &[Case] = &[
        (
            "simple_basic",
            r#"a   b
--- ---
1   2"#,
            Some((
                "cols=L:D,L:D head=[a|b] body=[1|2] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "simple_alignment_from_header",
            r#"Right  Left   Center
-----  ----  --------
    1  1        1"#,
            Some((
                "cols=D:D,D:D,C:D head=[Right|Left|Center] body=[1|1|1] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "simple_caption_before",
            r#"Table: Caption first.

a   b
--- ---
1   2"#,
            Some((
                "cols=L:D,L:D head=[a|b] body=[1|2] foot=[] cap=[Caption first.] attr=[]",
                5,
            )),
        ),
        (
            "simple_caption_after",
            r#"a   b
--- ---
1   2


: Colon caption.

next"#,
            Some((
                "cols=L:D,L:D head=[a|b] body=[1|2] foot=[] cap=[Colon caption.] attr=[]",
                7,
            )),
        ),
        (
            "simple_empty_cell",
            r#"a   b
--- ---
1
    2"#,
            Some((
                "cols=L:D,L:D head=[a|b] body=[1|;|2] foot=[] cap=[] attr=[]",
                4,
            )),
        ),
        (
            "simple_ends_at_blank",
            r#"a   b
--- ---
1   2

Paragraph."#,
            Some((
                "cols=L:D,L:D head=[a|b] body=[1|2] foot=[] cap=[] attr=[]",
                4,
            )),
        ),
        (
            "simple_headerless",
            r#"--- ---
1   2
--- ---"#,
            Some(("cols=L:D,L:D head=[] body=[1|2] foot=[] cap=[] attr=[]", 3)),
        ),
        (
            "simple_text_wider_than_column",
            r#"a   b
--- ---
long text 2"#,
            Some((
                "cols=L:D,L:D head=[a|b] body=[long|text 2] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "simple_footer_then_text",
            r#"a   b
--- ---
1   2
--- ---
x"#,
            Some((
                "cols=L:D,L:D head=[a|b] body=[1|2] foot=[] cap=[] attr=[]",
                4,
            )),
        ),
        (
            "simple_indented_dashes",
            r#"a  b
  --- ---
  1   2"#,
            Some((
                "cols=C:D,D:D head=[b|] body=[1|2] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "simple_wide_characters",
            r#"日本  b
----- ---
日本  2"#,
            Some((
                "cols=L:D,L:D head=[日本|b] body=[日本|2] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "simple_headerless_needs_footer",
            r#"--- ---
1   2
3   4"#,
            None,
        ),
        (
            "simple_needs_rows",
            r#"a   b
--- ---

x"#,
            None,
        ),
        (
            "multiline_basic",
            r#"----------- -----------
Head 1      Head 2
----------- -----------
cell one    cell two
continued

second row  x
----------- -----------"#,
            Some((
                "cols=L:0.16666666666666666,L:0.16666666666666666 head=[Head 1|Head 2] body=[cell one/continued|cell two;second row|x] foot=[] cap=[] attr=[]",
                8,
            )),
        ),
        (
            "multiline_relative_widths",
            r#"---------------------------- ----
Wide column                  N
---------------------------- ----
text                         1
---------------------------- ----"#,
            Some((
                "cols=L:0.4027777777777778,L:0.06944444444444445 head=[Wide column|N] body=[text|1] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "multiline_caption_attributes",
            r#"----- -----
a     b
----- -----
1     2
----- -----

Table: Cap. {#tbl:x}"#,
            Some((
                "cols=L:0.08333333333333333,L:0.08333333333333333 head=[a|b] body=[1|2] foot=[] cap=[Cap.] attr=[tbl:x]",
                7,
            )),
        ),
        (
            "multiline_last_column_widened",
            r#"-----   --  -----
a       b   c
-----   --  -----
1       2   3
-----   --  -----"#,
            Some((
                "cols=L:0.1111111111111111,L:0.05555555555555555,L:0.08333333333333333 head=[a|b|c] body=[1|2|3] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "multiline_last_column_copy",
            r#"-----   -----
a       b
-----   -----
1       2
-----   -----"#,
            Some((
                "cols=L:0.1111111111111111,L:0.1111111111111111 head=[a|b] body=[1|2] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "multiline_wider_than_columns",
            r#"------------------------------------------------ ------------------------------------
a b
------------------------------------------------ ------------------------------------
1 2
------------------------------------------------ ------------------------------------"#,
            Some((
                "cols=L:0.5697674418604651,D:0.43023255813953487 head=[a b|] body=[1 2|] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "multiline_center_alignment",
            r#"-----  -----
 a     b
 c
-----  -----
1     2

-----  -----"#,
            Some((
                "cols=C:0.09722222222222222,L:0.09722222222222222 head=[a/c|b] body=[1 2|] foot=[] cap=[] attr=[]",
                7,
            )),
        ),
        (
            "multiline_headless",
            r#"----- -----
1     2

3     4
----- -----"#,
            Some((
                "cols=L:0.08333333333333333,L:0.08333333333333333 head=[] body=[1|2;3|4] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "multiline_headless_one_row_is_simple",
            r#"----- -----
1     2
3     4
----- -----"#,
            Some((
                "cols=L:D,L:D head=[] body=[1|2;3|4] foot=[] cap=[] attr=[]",
                4,
            )),
        ),
        (
            "multiline_one_row_no_blank",
            r#"-----------
a     b
----- -----
1     2
-----------"#,
            Some((
                "cols=L:0.08333333333333333,L:0.08333333333333333 head=[a|b] body=[1|2] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_basic",
            r#"+---+---+
| a | b |
+===+===+
| 1 | 2 |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[a|b] body=[1|2] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_headerless",
            r#"+---+---+
| 1 | 2 |
+---+---+
| 3 | 4 |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[1|2;3|4] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_col_span",
            r#"+---+---+
| a | b |
+===+===+
| spans |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[a|b] body=[spans{1,2}] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_block_content",
            r#"+---------------+
| Head          |
+===============+
| Para one.     |
|               |
| Para two.     |
+---------------+"#,
            Some((
                "cols=D:0.2222222222222222 head=[P:Head] body=[P:Para one. ~ P:Para two.] foot=[] cap=[] attr=[]",
                7,
            )),
        ),
        (
            "grid_row_spans",
            r#"+---------------------+----------+
| Fruit               | Plum     |
+=============+=======+==========+
|             | low   | 3 kg     |
| Harvest     +-------+----------+
| 2024        | mid   | 7 kg     |
|             +-------+----------+
|             | top   | 12 kg    |
+-------------+-------+----------+"#,
            Some((
                "cols=D:0.19444444444444445,D:0.1111111111111111,D:0.1527777777777778 head=[Fruit{1,2}|Plum] body=[Harvest/2024{3,1}|low|3 kg;mid|7 kg;top|12 kg] foot=[] cap=[] attr=[]",
                9,
            )),
        ),
        (
            "grid_multi_row_header",
            r#"+---------------------+-----------------------+
| Station             | Rainfall 2001-2010    |
|                     | in millimetres        |
|                     +-------+-------+-------+
|                     | low   | avg   | high  |
+=====================+=======+=======+=======+
| Lowlands            | 12.5  | N/A   | 90.1  |
+---------------------+-------+-------+-------+"#,
            Some((
                "cols=D:0.3055555555555556,D:0.1111111111111111,D:0.1111111111111111,D:0.1111111111111111 head=[Station{2,1}|Rainfall 2001-2010/in millimetres{1,3};low|avg|high] body=[Lowlands|12.5|N/A|90.1] foot=[] cap=[] attr=[]",
                8,
            )),
        ),
        (
            "grid_foot",
            r#"+-----+-----+
| F   | P   |
+=====+=====+
| B   | 1   |
+-----+-----+
| O   | 2   |
+=====+=====+
| Sum | 3   |
+=====+=====+"#,
            Some((
                "cols=D:0.08333333333333333,D:0.08333333333333333 head=[F|P] body=[B|1;O|2] foot=[Sum|3] cap=[] attr=[]",
                9,
            )),
        ),
        (
            "grid_foot_without_head",
            r#"+---+
| a |
+---+
| b |
+===+
| c |
+===+"#,
            Some((
                "cols=D:0.05555555555555555 head=[a;b] body=[] foot=[c] cap=[] attr=[]",
                7,
            )),
        ),
        (
            "grid_head_only",
            r#"+---+
| a |
+---+
| b |
+===+"#,
            Some((
                "cols=D:0.05555555555555555 head=[a;b] body=[] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_middle_separators",
            r#"+---+
| a |
+===+
| b |
+===+
| c |
+---+
| d |
+---+"#,
            Some((
                "cols=D:0.05555555555555555 head=[a] body=[b;c;d] foot=[] cap=[] attr=[]",
                9,
            )),
        ),
        (
            "grid_alignments",
            r#"+-----+-----+------+
| R   | L   | C    |
+====:+:====+:====:+
| 1   | 2   | 3    |
+-----+-----+------+"#,
            Some((
                "cols=R:0.08333333333333333,L:0.08333333333333333,C:0.09722222222222222 head=[R|L|C] body=[1|2|3] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_headerless_alignments",
            r#"+-----:+:-----+:----:+
| R    | L    | C    |
+------+------+------+"#,
            Some((
                "cols=R:0.09722222222222222,L:0.09722222222222222,C:0.09722222222222222 head=[] body=[R|L|C] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "grid_unicode_width",
            r#"+------+------+
| 日本 | ab   |
+======+======+
| é    | 😀   |
+------+------+"#,
            Some((
                "cols=D:0.09722222222222222,D:0.09722222222222222 head=[日本|ab] body=[é|😀] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_broken_right_border",
            r#"+---+---+
| a   b
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a b{1,2}] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "grid_header_span",
            r#"+---+---+
| a | b |
+=======+
| 1 | 2 |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[a | b{1,2}] body=[1 | 2{1,2}] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_empty_cells",
            r#"+---+---+
|   |   |
+===+===+
|   | x |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[|x] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_wider_than_columns",
            r#"+------------------------------------------------------------+------------------------------+
| a                                                          | b                            |
+------------------------------------------------------------+------------------------------+"#,
            Some((
                "cols=D:0.6630434782608695,D:0.33695652173913043 head=[] body=[a|b] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "grid_no_closing_border",
            r#"+---+
| a |"#,
            None,
        ),
        (
            "grid_caption_after",
            r#"+---+
| a |
+---+

Table: After."#,
            Some((
                "cols=D:0.05555555555555555 head=[] body=[a] foot=[] cap=[After.] attr=[]",
                5,
            )),
        ),
        (
            "grid_then_text",
            r#"+---+
| a |
+---+
text"#,
            Some((
                "cols=D:0.05555555555555555 head=[] body=[a] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "pipe_basic",
            r#"| a | b |
|---|---|
| 1 | 2 |"#,
            Some((
                "cols=D:D,D:D head=[a|b] body=[1|2] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "pipe_alignments",
            r#"| l | c | r | d |
|:--|:-:|--:|---|
| 1 | 2 | 3 | 4 |"#,
            Some((
                "cols=L:D,C:D,R:D,D:D head=[l|c|r|d] body=[1|2|3|4] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "pipe_header_only",
            r#"| a | b |
|---|---|"#,
            Some(("cols=D:D,D:D head=[a|b] body=[] foot=[] cap=[] attr=[]", 2)),
        ),
        (
            "pipe_uneven_cells",
            r#"| a | b | c |
|---|---|---|
| 1 |
| 1 | 2 | 3 | 4 |"#,
            Some((
                "cols=D:D,D:D,D:D head=[a|b|c] body=[1||;1|2|3] foot=[] cap=[] attr=[]",
                4,
            )),
        ),
        (
            "pipe_long_row_relative_widths",
            r#"| a | b |
|---|---------------------------------------------------------------------------|
| short | this is a much longer cell that should make the table use relative widths when long |"#,
            Some((
                "cols=D:0.038461538461538464,D:0.9615384615384616 head=[a|b] body=[short|this is a much longer cell that should make the table use relative widths when long] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "pipe_line_of_72_keeps_default_widths",
            r#"| a | b |
|---|---|
| 1 | yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy |"#,
            Some((
                "cols=D:D,D:D head=[a|b] body=[1|yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "pipe_line_of_73_gives_widths",
            r#"| a | b |
|:-|---|
| 1 | yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy |"#,
            Some((
                "cols=L:0.4,D:0.6 head=[a|b] body=[1|yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "pipe_no_leading_pipe",
            r#"a | b
--|--
1 | 2"#,
            Some((
                "cols=D:D,D:D head=[a|b] body=[1|2] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "pipe_orgtbl_separator",
            r#"| a | b |
|-----+-----|
| 1 | 2 |"#,
            Some((
                "cols=D:D,D:D head=[a|b] body=[1|2] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "pipe_empty_cells",
            r#"| a |  |
|---|--|
|   | 1 |"#,
            Some(("cols=D:D,D:D head=[a|] body=[|1] foot=[] cap=[] attr=[]", 3)),
        ),
        (
            "pipe_double_leading_pipe",
            r#"| a | b |
|---|---|
|| x |"#,
            Some((
                "cols=D:D,D:D head=[a|b] body=[|x] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "pipe_link_is_split",
            r#"| [a|b](u) | c |
|---|---|
| 1 | 2 |"#,
            Some((
                "cols=D:D,D:D head=[[a|b](u)] body=[1|2] foot=[] cap=[] attr=[]",
                3,
            )),
        ),
        (
            "pipe_row_needs_pipe",
            r#"| a |
|---|
1
two | three"#,
            Some(("cols=D:D head=[a] body=[] foot=[] cap=[] attr=[]", 2)),
        ),
        (
            "pipe_header_needs_pipe",
            r#"a
|-
1"#,
            None,
        ),
        (
            "pipe_caption_attributes",
            r#"| a |
|---|
| 1 |

: Caption {#tbl .wide}"#,
            Some((
                "cols=D:D head=[a] body=[1] foot=[] cap=[Caption] attr=[tbl.wide]",
                5,
            )),
        ),
        (
            "pipe_caption_directly_after",
            r#"| a |
|---|
| 1 |
: cap
more"#,
            Some((
                "cols=D:D head=[a] body=[1] foot=[] cap=[cap/more] attr=[]",
                5,
            )),
        ),
        (
            "caption_attributes_only",
            r#"| a |
|---|
| 1 |

: {#id}"#,
            Some(("cols=D:D head=[a] body=[1] foot=[] cap=[] attr=[id]", 5)),
        ),
        (
            "caption_invalid_attributes",
            r#"| a |
|---|
| 1 |

: cap {.a b c=}"#,
            Some((
                "cols=D:D head=[a] body=[1] foot=[] cap=[cap {.a b c=}] attr=[]",
                5,
            )),
        ),
        (
            "caption_before_wins",
            r#"Table: before

| a |
|---|
| 1 |

Table: after"#,
            Some(("cols=D:D head=[a] body=[1] foot=[] cap=[before] attr=[]", 6)),
        ),
        (
            "caption_before_needs_blank",
            r#"Table: cap
| a |
|---|
| 1 |"#,
            None,
        ),
        (
            "caption_lowercase",
            r#"| a |
|---|
| 1 |

table: lower"#,
            Some(("cols=D:D head=[a] body=[1] foot=[] cap=[lower] attr=[]", 5)),
        ),
        (
            "grid_colon_line_is_cell_text",
            r#"+---+---+
| a | b |
+---:---+
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a | b/---:---/c | d{2,2};] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_partial_border_open_column",
            r#"+---+---+
| a | b |
+---+   +
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[P:a|P:b ~ P:d{2,1};P:c] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_partial_border_letter",
            r#"+---+---+
| a | b |
+---+-x-+
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a|b/-x-/d{2,1};c] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_top_border_without_column_corner",
            r#"+-------+
| a | b |
+---+---+
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a | b{1,2};c|d] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_partial_border_letter_left",
            r#"+---+---+
| a | b |
+-x-+---+
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a/-x-/c{2,1}|b;d] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_partial_border_three_columns",
            r#"+---+---+---+
| a | b | c |
+---+---+   +
| d | e | f |
+---+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555,D:0.05555555555555555 head=[] body=[P:a|P:b|P:c ~ P:f{2,1};P:d|P:e] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_colon_line_before_head_separator",
            r#"+---+---+
| a | b |
+---:---+
| c | d |
+===+===+
| e | f |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[a | b/---:---/c | d{2,2};] body=[e|f] foot=[] cap=[] attr=[]",
                7,
            )),
        ),
        (
            "grid_equals_colon_line_is_cell_text",
            r#"+---+---+
| a | b |
+===:===+
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a | b/===:===/c | d{2,2};] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_inner_colons_are_cell_text",
            r#"+---+---+
| a | b |
+--:+:--+
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a/--:/c{2,1}|b/:--/d{2,1};] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_plus_inside_cell_adds_row",
            r#"+---+---+
| a | b |
|   +   |
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[P:a ~ P:c{2,1}|P:b ~ P:d{2,1};] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_plus_at_edges_adds_row",
            r#"+---+---+
| a | b |
+ x   y +
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a | b/x y/c | d{2,2};] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_plus_at_right_edge_adds_row",
            r#"+---+---+
| a | b |
| x   y +
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a | b/x y/c | d{2,2};] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_head_separator_alignment",
            r#"+:--+---+
| a | b |
+:==+===+
| c | d |
+---+---+"#,
            Some((
                "cols=L:0.05555555555555555,D:0.05555555555555555 head=[a|b] body=[c|d] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_colon_in_body_border_is_cell_text",
            r#"+---+---+
| a | b |
+:--+---+
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a/:--/c{2,1}|b;d] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_bottom_corner_adds_column",
            r#"+---+---+
| a | b |
+--++---+
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.041666666666666664,D:0.013888888888888888,D:0.05555555555555555 head=[] body=[a{1,2}|b;c{1,2}|d] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_full_rule_without_corner",
            r#"+---+---+
| a | b |
+-------+
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a | b{1,2};c | d{1,2}] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_two_colon_lines",
            r#"+---+---+
| a | b |
+---:---+
| c | d |
+---:---+
| e | f |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a | b/---:---/c | d/---:---/e | f{3,2};;] foot=[] cap=[] attr=[]",
                7,
            )),
        ),
        (
            "grid_top_border_inner_colon",
            r#"+-:-+---+
| a | b |
+---+---+"#,
            None,
        ),
        (
            "grid_top_border_alignment",
            r#"+:--+--:+
| a | b |
+---+---+
| c | d |
+---+---+"#,
            Some((
                "cols=L:0.05555555555555555,R:0.05555555555555555 head=[] body=[a|b;c|d] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_head_separator_both_colons",
            r#"+---+---+
| a | b |
+:=:+===+
| c | d |
+---+---+"#,
            Some((
                "cols=C:0.05555555555555555,D:0.05555555555555555 head=[a|b] body=[c|d] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_colon_bottom_border_leaves_hole",
            r#"+---+---+
| a | b |
+---+---+
| c | d |
+:--+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a|b;|d] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_foot_separator_with_colon",
            r#"+---+---+
| a | b |
+===+===+
| c | d |
+===+===+
| e | f |
+:==+===+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[a|b] body=[c|d] foot=[e|f] cap=[] attr=[]",
                7,
            )),
        ),
        (
            "grid_equals_bottom_border",
            r#"+---+---+
| a | b |
+---+---+
| c | d |
+===+===+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[a|b;c|d] body=[] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_mixed_rule_is_cell_text",
            r#"+---+---+
| a | b |
+=-=+---+
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a/=-=/c{2,1}|b;d] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_colon_equals_dash_rule_is_cell_text",
            r#"+---+---+
| a | b |
+:=:+---+
| c | d |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a/:=:/c{2,1}|b;d] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_colon_line_merges_rows",
            r#"+---+---+
| a | b |
+---+---+
| c | d |
+:--+---+
| e | f |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a|b;c/:--/e{2,1}|d;f] foot=[] cap=[] attr=[]",
                7,
            )),
        ),
        (
            "grid_missing_cell_is_empty",
            r#"+---+---+---+
| a | b | c |
+---+---+---+
| d | e |
+---+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a|b|c;d|e|] foot=[] cap=[] attr=[]",
                5,
            )),
        ),
        (
            "grid_open_cell_spans_rows",
            r#"+---+---+
| a | b |
+---+---+
| c |   |
+---+   +
| e | f |
+---+---+"#,
            Some((
                "cols=D:0.05555555555555555,D:0.05555555555555555 head=[] body=[a|b;c|f{2,1};e] foot=[] cap=[] attr=[]",
                7,
            )),
        ),
    ];

    #[test]
    fn matches_the_oracle() {
        let mut failures = Vec::new();
        for (name, input, expected) in CASES {
            let lines: Vec<&str> = input.lines().collect();
            let got = parse_table(&lines, &options(), &mut WordParser)
                .map(|(block, used)| (summary(&block), used));
            let expected = expected.map(|(text, used)| (text.to_string(), used));
            if got != expected {
                failures.push(format!(
                    "{name}:\n  got      {got:?}\n  expected {expected:?}"
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn extensions_turn_kinds_off() {
        let pipe = ["| a |", "|---|", "| 1 |"];
        let off = TableOptions {
            pipe: false,
            ..options()
        };
        assert!(parse_table(&pipe, &off, &mut WordParser).is_none());
        let grid = ["+---+", "| a |", "+---+"];
        let off = TableOptions {
            grid: false,
            ..options()
        };
        assert!(parse_table(&grid, &off, &mut WordParser).is_none());
    }

    #[test]
    fn caption_extensions() {
        let lines = ["| a |", "|---|", "| 1 |", "", ": Caption {#tbl}"];
        let no_captions = TableOptions {
            captions: false,
            ..options()
        };
        let (block, used) = parse_table(&lines, &no_captions, &mut WordParser)
            .unwrap_or((Block::HorizontalRule, 0));
        assert_eq!(used, 4);
        assert!(summary(&block).ends_with("cap=[] attr=[]"));
        let no_attributes = TableOptions {
            attributes: false,
            ..options()
        };
        let (block, used) = parse_table(&lines, &no_attributes, &mut WordParser)
            .unwrap_or((Block::HorizontalRule, 0));
        assert_eq!(used, 5);
        assert!(summary(&block).ends_with("cap=[Caption {#tbl}] attr=[]"));
    }

    #[test]
    fn grid_cell_text_keeps_indentation_after_one_space() {
        struct Recorder(Vec<String>);
        impl CellParser for Recorder {
            fn inlines(&mut self, _: &str) -> Vec<Inline> {
                Vec::new()
            }
            fn blocks(&mut self, text: &str) -> Vec<Block> {
                self.0.push(text.to_string());
                Vec::new()
            }
            fn attributes(&mut self, _: &str) -> Option<Attr> {
                None
            }
        }
        let mut recorder = Recorder(Vec::new());
        let lines = ["+--------+", "|     x  |", "+--------+", "|x     |"];
        let lines2 = ["+------+", "|x     |", "|  y   |", "+------+"];
        assert!(parse_table(&lines, &options(), &mut recorder).is_some());
        assert!(parse_table(&lines2, &options(), &mut recorder).is_some());
        assert_eq!(recorder.0, vec!["    x".to_string(), "x\n  y".to_string()]);
    }
}
