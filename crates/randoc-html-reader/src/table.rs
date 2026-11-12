//! Tables. The grammar was found with Pandoc 3.13 as a black box. The grid
//! rules (`place_row_section`, `clip_rows`) are a port of the table
//! normalization of `pandoc-types` 1.23.1 (`Text.Pandoc.Builder`, BSD-3-Clause).

use randoc_ast::walk::query_inlines;
use randoc_ast::{
    Alignment, Attr, Block, Caption, Cell, ColSpec, ColWidth, Inline, Row, Table, TableBody,
    TableFoot, TableHead, Text,
};

use crate::parse::{Parse, Parser, Stop};
use crate::text::{attribute, clean_attributes, is_blank, read_decimal, read_integer, to_attr};
use crate::tokens::TokenKind;

/// The cost of a table for the budget of the document: one for each column
/// specification, and one for each grid slot that no cell of the input
/// starts in its row (a slot of a span or of padding). A cell that the
/// input writes costs nothing, so the memory of an ordinary table stays
/// linear in the input. `row_cells` gives the number of cells of each row.
fn generated_cells(columns: usize, row_cells: impl Iterator<Item = usize>) -> usize {
    row_cells.fold(columns, |sum, cells| {
        sum.saturating_add(columns.saturating_sub(cells))
    })
}

/// The cells that the tables of the document made without input text for
/// them; see `generated_cells`. Pandoc has no limit; one short `colspan`
/// value can make it allocate gigabytes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CellBudget {
    /// The cells that the tables made so far.
    pub(crate) used: usize,
    /// The largest sum (`Limits::max_table_cells`).
    pub(crate) max: usize,
}

impl CellBudget {
    /// Adds `cost` to the used cells. Returns false, and changes nothing,
    /// when the sum is above the maximum.
    fn take(&mut self, cost: usize) -> bool {
        match self.used.checked_add(cost) {
            Some(total) if total <= self.max => {
                self.used = total;
                true
            }
            _ => false,
        }
    }
}

/// A cell and the kind of its tag.
struct ReadCell {
    is_header: bool,
    cell: Cell,
}

type ReadRow = (Attr, Vec<ReadCell>);

fn at_open(parser: &Parser<'_>, tag: &str) -> bool {
    matches!(parser.peek_kind(), Some(TokenKind::Open(name, _)) if name == tag)
}

fn open_attr(parser: &Parser<'_>) -> Attr {
    match parser.peek_kind() {
        Some(TokenKind::Open(_, attributes)) => to_attr(&clean_attributes(attributes)),
        _ => Attr::default(),
    }
}

/// Moves over white space and comments between the parts of a table.
fn skip_blank(parser: &mut Parser<'_>) {
    while let Some(kind) = parser.peek_kind() {
        match kind {
            TokenKind::Text(text) if is_blank(text) => parser.advance(),
            TokenKind::Comment(_) => parser.advance(),
            _ => return,
        }
    }
}

/// Reads the `table` element at the current token.
pub(crate) fn table(parser: &mut Parser<'_>, out: &mut Vec<Block>) -> Parse<()> {
    let attr = open_attr(parser);
    let table = parser.within(|parser| {
        parser.advance();
        skip_blank(parser);
        let caption = if at_open(parser, "caption") {
            parser.container("caption")?
        } else {
            Vec::new()
        };
        skip_blank(parser);
        let widths = columns(parser)?;
        skip_blank(parser);
        let mut head = (Attr::default(), Vec::new());
        let mut pending: Option<ReadRow> = None;
        if at_open(parser, "thead") {
            head = section(parser, "thead")?;
        } else if at_open(parser, "tr") {
            let first = row(parser)?;
            if !first.1.is_empty() && first.1.iter().all(|cell| cell.is_header) {
                head.1.push(first);
            } else {
                pending = Some(first);
            }
        }
        skip_blank(parser);
        let mut foot: Option<(Attr, Vec<ReadRow>)> = None;
        if pending.is_none() && at_open(parser, "tfoot") {
            foot = Some(section(parser, "tfoot")?);
            skip_blank(parser);
        }
        let mut bodies: Vec<(Attr, Vec<ReadRow>)> = Vec::new();
        loop {
            parser.count_step()?;
            if pending.is_none() && at_open(parser, "tbody") {
                bodies.push(section(parser, "tbody")?);
            } else if pending.is_some() || at_open(parser, "tr") {
                let mut rows: Vec<ReadRow> = pending.take().into_iter().collect();
                skip_blank(parser);
                while at_open(parser, "tr") {
                    rows.push(row(parser)?);
                    skip_blank(parser);
                }
                bodies.push((Attr::default(), rows));
                // Rows without an open `tbody` tag can end with its close tag.
                if matches!(parser.peek_kind(), Some(TokenKind::Close(name)) if name == "tbody") {
                    parser.advance();
                }
            } else {
                break;
            }
            skip_blank(parser);
        }
        if foot.is_none() && at_open(parser, "tfoot") {
            foot = Some(section(parser, "tfoot")?);
            skip_blank(parser);
        }
        // A table needs its close tag.
        match parser.peek_kind() {
            Some(TokenKind::Close(name)) if name == "table" => parser.advance(),
            _ => return Err(Stop::Fail),
        }
        build(
            attr,
            caption,
            &widths,
            head,
            bodies,
            foot.unwrap_or_default(),
            &mut parser.cell_budget,
        )
    })?;
    out.push(Block::Table(Box::new(table)));
    Ok(())
}

/// A column width from a `col` element.
#[derive(Clone, Copy)]
enum Width {
    Unknown,
    Fraction(f64),
    Relative(f64),
}

fn columns(parser: &mut Parser<'_>) -> Parse<Vec<Width>> {
    let mut widths = Vec::new();
    if at_open(parser, "colgroup") {
        while at_open(parser, "colgroup") {
            parser.count_step()?;
            parser.advance();
            loop {
                skip_blank(parser);
                if parser.element_ends("colgroup") {
                    break;
                }
                if !at_open(parser, "col") {
                    return Err(Stop::Fail);
                }
                column(parser, &mut widths);
            }
            skip_blank(parser);
        }
    } else {
        while at_open(parser, "col") {
            parser.count_step()?;
            column(parser, &mut widths);
            skip_blank(parser);
        }
    }
    Ok(widths)
}

fn column(parser: &mut Parser<'_>, widths: &mut Vec<Width>) {
    let clean = match parser.peek_kind() {
        Some(TokenKind::Open(_, attributes)) => clean_attributes(attributes),
        _ => Vec::new(),
    };
    parser.advance();
    if matches!(parser.peek_kind(), Some(TokenKind::Close(name)) if name == "col") {
        parser.advance();
    }
    // A relative width (`2*`) is only a `width` attribute. Its factor is an
    // integer; another factor counts as 1.
    let (text, relative_allowed) = match attribute(&clean, "width") {
        Some(width) => (Some(width.to_owned()), true),
        // The oracle reads a style only of the form `width:VALUE`, with
        // semicolons and spaces after the value.
        None => (
            attribute(&clean, "style")
                .and_then(|style| style.strip_prefix("width:"))
                .map(|value| value.trim_end_matches([';', ' ']).to_owned()),
            false,
        ),
    };
    let width = match text.as_deref() {
        Some(text) if text.ends_with('%') => read_decimal(&text[..text.len() - 1])
            .map_or(Width::Unknown, |value| Width::Fraction(value / 100.0)),
        Some(text) if relative_allowed && text.ends_with('*') => {
            let factor = read_integer(&text[..text.len() - 1]).unwrap_or(1);
            Width::Relative(factor as f64)
        }
        _ => Width::Unknown,
    };
    widths.push(width);
}

/// Reads `thead`, `tbody` or `tfoot`: the attributes and the rows.
fn section(parser: &mut Parser<'_>, tag: &str) -> Parse<(Attr, Vec<ReadRow>)> {
    let attr = open_attr(parser);
    let rows = parser.within(|parser| {
        parser.advance();
        let mut rows = Vec::new();
        loop {
            parser.count_step()?;
            skip_blank(parser);
            if parser.element_ends(tag) {
                break;
            }
            if !at_open(parser, "tr") {
                return Err(Stop::Fail);
            }
            rows.push(row(parser)?);
        }
        Ok(rows)
    })?;
    Ok((attr, rows))
}

fn row(parser: &mut Parser<'_>) -> Parse<ReadRow> {
    let attr = open_attr(parser);
    let cells = parser.within(|parser| {
        parser.advance();
        let mut cells = Vec::new();
        loop {
            parser.count_step()?;
            skip_blank(parser);
            if parser.element_ends("tr") {
                break;
            }
            match parser.peek_kind() {
                Some(TokenKind::Open(name, attributes)) if name == "td" || name == "th" => {
                    let content = parser.container(name)?;
                    cells.push(ReadCell {
                        is_header: name == "th",
                        cell: cell(attributes, content),
                    });
                }
                _ => return Err(Stop::Fail),
            }
        }
        Ok(cells)
    })?;
    Ok((attr, cells))
}

fn cell(attributes: &[(Text, Text)], content: Vec<Block>) -> Cell {
    let mut clean = clean_attributes(attributes);
    let span = |name: &str| attribute(&clean, name).and_then(read_integer).unwrap_or(1);
    let (row_span, col_span) = (span("rowspan"), span("colspan"));
    let alignment = |name: &str| match name {
        "left" => Some(Alignment::AlignLeft),
        "right" => Some(Alignment::AlignRight),
        "center" => Some(Alignment::AlignCenter),
        _ => None,
    };
    let mut align = attribute(&clean, "align").and_then(alignment);
    let mut style_out: Option<String> = None;
    if let Some(style) = attribute(&clean, "style") {
        let mut kept: Vec<String> = Vec::new();
        for declaration in style.split(';') {
            let Some((name, value)) = declaration.split_once(':') else {
                continue;
            };
            let (name, value) = (name.trim(), value.trim());
            if name == "text-align" {
                align = align.or_else(|| alignment(value));
                if alignment(value).is_some() {
                    continue;
                }
            }
            kept.push(format!("{name}: {value}"));
        }
        style_out = Some(kept.join("; "));
    }
    clean.retain(|(key, _)| !matches!(key.as_str(), "align" | "rowspan" | "colspan"));
    match style_out {
        Some(style) if style.is_empty() => clean.retain(|(key, _)| key != "style"),
        Some(style) => {
            if let Some(entry) = clean.iter_mut().find(|(key, _)| key == "style") {
                entry.1 = style;
            }
        }
        None => {}
    }
    Cell {
        attr: to_attr(&clean),
        align: align.unwrap_or(Alignment::AlignDefault),
        row_span,
        col_span,
        content,
    }
}

/// The sum of the column spans of a row. Pandoc adds with 64-bit wrapping.
fn row_width(row: &ReadRow) -> i64 {
    row.1
        .iter()
        .fold(0i64, |sum, cell| sum.wrapping_add(cell.cell.col_span))
}

/// A simple cell has at most one paragraph, and no line break.
fn is_simple_cell(cell: &ReadCell) -> bool {
    match cell.cell.content.as_slice() {
        [] => true,
        [block @ (Block::Plain(_) | Block::Para(_))] => {
            let mut has_break = false;
            query_inlines(block, |inline| has_break |= *inline == Inline::LineBreak);
            !has_break
        }
        _ => false,
    }
}

fn build(
    attr: Attr,
    caption: Vec<Block>,
    widths: &[Width],
    head: (Attr, Vec<ReadRow>),
    bodies: Vec<(Attr, Vec<ReadRow>)>,
    foot: (Attr, Vec<ReadRow>),
    budget: &mut CellBudget,
) -> Parse<Table> {
    let all_rows = || {
        head.1
            .iter()
            .chain(bodies.iter().flat_map(|(_, rows)| rows.iter()))
            .chain(foot.1.iter())
    };
    // A table without cells is not a table. A table whose widest row has no
    // positive width has no columns.
    if all_rows().all(|row| row.1.is_empty()) {
        return Err(Stop::Fail);
    }
    let cell_columns = all_rows().map(row_width).max().unwrap_or(0);
    let column_count = usize::try_from(cell_columns).unwrap_or(0);
    // The check comes before each allocation whose size depends on a span.
    let cost = generated_cells(column_count, all_rows().map(|row| row.1.len()));
    if !budget.take(cost) {
        return Err(Stop::TooLarge);
    }
    let simple = all_rows().all(|row| row.1.iter().all(is_simple_cell));
    let sum = |pick: fn(&Width) -> f64| widths.iter().map(pick).sum::<f64>();
    let relative_total = sum(|width| match width {
        Width::Relative(value) => *value,
        _ => 0.0,
    });
    // The relative columns share the width that the percentages leave.
    let remaining = 1.0
        - sum(|width| match width {
            Width::Fraction(value) => *value,
            _ => 0.0,
        });
    let any_width = widths.iter().any(|width| !matches!(width, Width::Unknown));
    let default_width = if any_width || simple {
        ColWidth::ColWidthDefault
    } else {
        ColWidth::ColWidth(1.0 / column_count as f64)
    };
    // The alignment of a column is the alignment of its cell in the first body row.
    let mut aligns: Vec<Alignment> = Vec::new();
    if let Some(first) = bodies.iter().flat_map(|(_, rows)| rows.iter()).next() {
        for cell in &first.1 {
            let span = usize::try_from(cell.cell.col_span.max(1)).unwrap_or(1);
            let room = column_count.saturating_sub(aligns.len());
            aligns.extend(std::iter::repeat_n(cell.cell.align, span.min(room)));
        }
    }
    let col_specs = (0..column_count)
        .map(|index| ColSpec {
            align: aligns
                .get(index)
                .copied()
                .unwrap_or(Alignment::AlignDefault),
            width: match widths.get(index) {
                Some(Width::Fraction(value)) => ColWidth::ColWidth(*value),
                Some(Width::Relative(value)) => {
                    ColWidth::ColWidth(value * (remaining / relative_total))
                }
                _ => default_width,
            },
        })
        .collect();
    let table_bodies = bodies
        .into_iter()
        .map(|(attr, rows)| body(attr, rows, column_count))
        .collect();
    Ok(Table {
        attr,
        caption: Caption {
            short: None,
            long: caption,
        },
        col_specs,
        head: TableHead {
            attr: head.0,
            rows: header_section(head.1, column_count),
        },
        bodies: table_bodies,
        foot: TableFoot {
            attr: foot.0,
            rows: header_section(foot.1, column_count),
        },
    })
}

/// The row head of the rows of a body: the header columns of each row, when
/// that number is the same for each row, clipped to the columns. A row counts
/// the `colspan` of its leading `th` cells and of each `th` cell of an earlier
/// row whose `rowspan` reaches it. A cell with a `rowspan` below 1 counts
/// nothing. Pandoc adds the raw values with 64-bit wrapping.
fn row_head_columns(rows: &[ReadRow], column_count: usize) -> usize {
    // `ends[i]` is the sum of the spans that stop at row `i`; the sums
    // wrap, so a wrapping subtraction removes them exactly.
    let mut ends = vec![0i64; rows.len()];
    let mut carried = 0i64;
    let mut first = None;
    for (index, (_, cells)) in rows.iter().enumerate() {
        carried = carried.wrapping_sub(ends.get(index).copied().unwrap_or(0));
        let count = cells
            .iter()
            .take_while(|cell| cell.is_header)
            .filter(|cell| cell.cell.row_span >= 1)
            .fold(carried, |sum, cell| sum.wrapping_add(cell.cell.col_span));
        if *first.get_or_insert(count) != count {
            return 0;
        }
        for cell in cells
            .iter()
            .filter(|cell| cell.is_header && cell.cell.row_span > 1)
        {
            carried = carried.wrapping_add(cell.cell.col_span);
            let end = usize::try_from(cell.cell.row_span)
                .map_or(usize::MAX, |span| index.saturating_add(span));
            if let Some(slot) = ends.get_mut(end) {
                *slot = slot.wrapping_add(cell.cell.col_span);
            }
        }
    }
    first.map_or(0, |count| {
        usize::try_from(count).unwrap_or(0).min(column_count)
    })
}

/// Makes one table body. The first rows that have only `th` cells are the
/// head of the body.
fn body(attr: Attr, rows: Vec<ReadRow>, column_count: usize) -> TableBody {
    let head_count = rows
        .iter()
        .take_while(|row| !row.1.is_empty() && row.1.iter().all(|cell| cell.is_header))
        .count();
    let mut rows = rows;
    let body_rows = rows.split_off(head_count);
    let row_head = row_head_columns(&body_rows, column_count);
    TableBody {
        attr,
        row_head_columns: i64::try_from(row_head).unwrap_or(0),
        head: header_section(rows, column_count),
        body: body_section(body_rows, row_head, column_count),
    }
}

/// Limits each row span to the rows that follow in the section.
fn clip_rows(rows: &mut [ReadRow]) {
    let total = rows.len();
    for (index, row) in rows.iter_mut().enumerate() {
        let high = i64::try_from(total - index).unwrap_or(i64::MAX);
        for cell in &mut row.1 {
            cell.cell.row_span = cell.cell.row_span.max(1).min(high);
        }
    }
}

/// Puts cells on one grid row. `hang` holds, for each column, the height of
/// the cell that comes down from the row above, counted from that row.
fn place_row_section(hang: &mut [i64], cells: &mut impl Iterator<Item = Cell>) -> Vec<Cell> {
    let mut placed = Vec::new();
    let mut index = 0;
    while let Some(&over) = hang.get(index) {
        if over > 1 {
            if let Some(slot) = hang.get_mut(index) {
                *slot = over - 1;
            }
            index += 1;
            continue;
        }
        let mut cell = cells.next().unwrap_or_default();
        let wanted = usize::try_from(cell.col_span.max(1)).unwrap_or(1);
        let free = hang
            .get(index..)
            .unwrap_or_default()
            .iter()
            .take(wanted)
            .take_while(|over| **over == 1)
            .count()
            .max(1);
        cell.col_span = i64::try_from(free).unwrap_or(1);
        for slot in hang.iter_mut().skip(index).take(free) {
            *slot = cell.row_span;
        }
        placed.push(cell);
        index += free;
    }
    placed
}

fn header_section(mut rows: Vec<ReadRow>, column_count: usize) -> Vec<Row> {
    clip_rows(&mut rows);
    let mut hang = vec![1i64; column_count];
    rows.into_iter()
        .map(|(attr, cells)| {
            let mut cells = cells.into_iter().map(|cell| cell.cell);
            Row {
                attr,
                cells: place_row_section(&mut hang, &mut cells),
            }
        })
        .collect()
}

fn body_section(mut rows: Vec<ReadRow>, row_head: usize, column_count: usize) -> Vec<Row> {
    clip_rows(&mut rows);
    let mut head_hang = vec![1i64; row_head];
    let mut body_hang = vec![1i64; column_count - row_head];
    rows.into_iter()
        .map(|(attr, cells)| {
            let mut cells = cells.into_iter().map(|cell| cell.cell);
            let mut placed = place_row_section(&mut head_hang, &mut cells);
            placed.extend(place_row_section(&mut body_hang, &mut cells));
            Row {
                attr,
                cells: placed,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_cell(row_span: i64, col_span: i64) -> ReadCell {
        ReadCell {
            is_header: false,
            cell: Cell {
                row_span,
                col_span,
                ..Cell::default()
            },
        }
    }

    // Pandoc 3.13: `<table><tr><td rowspan="3">a</td><td>b</td></tr><tr></tr>
    // <tr><td>c</td><td>d</td><td>e</td></tr></table>` has three columns, a
    // padded first row, a second row of two empty cells and no cell `e`.
    #[test]
    fn places_cells_on_the_grid() {
        let rows = vec![
            (Attr::default(), vec![plain_cell(3, 1), plain_cell(1, 1)]),
            (Attr::default(), vec![]),
            (
                Attr::default(),
                vec![plain_cell(1, 1), plain_cell(1, 1), plain_cell(1, 1)],
            ),
        ];
        let placed = header_section(rows, 3);
        let shape: Vec<Vec<(i64, i64)>> = placed
            .iter()
            .map(|row| row.cells.iter().map(|c| (c.row_span, c.col_span)).collect())
            .collect();
        assert_eq!(
            shape,
            vec![
                vec![(3, 1), (1, 1), (1, 1)],
                vec![(1, 1), (1, 1)],
                vec![(1, 1), (1, 1)],
            ]
        );
    }

    #[test]
    fn the_budget_counts_spans_and_padding_only() {
        // An ordinary table: 10 columns, 20,001 full rows.
        assert_eq!(generated_cells(10, std::iter::repeat_n(10, 20_001)), 10);
        // `<td colspan="5">a</td><td>b</td>`, then a row of one cell.
        assert_eq!(generated_cells(6, [2, 1].into_iter()), 6 + 4 + 5);
        assert_eq!(generated_cells(usize::MAX, [1].into_iter()), usize::MAX);
        let max = 200_000;
        let mut budget = CellBudget { used: 0, max };
        assert!(budget.take(max - 1));
        assert!(budget.take(1));
        assert!(!budget.take(1));
        assert_eq!(budget.used, max);
        let mut budget = CellBudget { used: 1, max };
        assert!(!budget.take(usize::MAX));
        assert_eq!(budget.used, 1);
    }

    fn header_cell(row_span: i64, col_span: i64) -> ReadCell {
        ReadCell {
            is_header: true,
            ..plain_cell(row_span, col_span)
        }
    }

    fn rows(cells: Vec<Vec<ReadCell>>) -> Vec<ReadRow> {
        cells
            .into_iter()
            .map(|row| (Attr::default(), row))
            .collect()
    }

    // Each expected value is the row head that Pandoc 3.13 gives for the
    // same body rows (the matrix `tables rowhead-*`).
    #[test]
    fn the_row_head_counts_carried_and_leading_header_spans() {
        let row_head = |cells, columns| row_head_columns(&rows(cells), columns);
        // A `th` with `rowspan` 2 counts in the next row.
        assert_eq!(
            row_head(
                vec![
                    vec![header_cell(2, 1), plain_cell(1, 1)],
                    vec![plain_cell(1, 1)]
                ],
                2
            ),
            1
        );
        // A carried `th` counts after a carried `td` too.
        let carried = vec![
            vec![header_cell(3, 1), plain_cell(2, 1)],
            vec![header_cell(1, 1)],
            vec![plain_cell(1, 1)],
        ];
        assert_eq!(row_head(carried, 2), 0);
        // A `rowspan` below 1 counts nothing and does not end the leading cells.
        assert_eq!(
            row_head(vec![vec![header_cell(0, 1), plain_cell(1, 1)]], 2),
            0
        );
        assert_eq!(
            row_head(
                vec![
                    vec![header_cell(1, 1), plain_cell(1, 1)],
                    vec![header_cell(0, 0), header_cell(1, 1)]
                ],
                2
            ),
            1
        );
        // Raw values, compared before the clip to the columns.
        assert_eq!(
            row_head(
                vec![
                    vec![header_cell(1, 3), plain_cell(1, -2)],
                    vec![header_cell(1, 4), plain_cell(1, -3)]
                ],
                1
            ),
            0
        );
        assert_eq!(
            row_head(
                vec![
                    vec![header_cell(1, 3), plain_cell(1, -2)],
                    vec![header_cell(1, 3), plain_cell(1, -2)]
                ],
                1
            ),
            1
        );
        // The largest values wrap and allocate nothing.
        assert_eq!(
            row_head(
                vec![vec![
                    header_cell(1, i64::MAX),
                    header_cell(1, 1),
                    plain_cell(1, 1)
                ]],
                0
            ),
            0
        );
        assert_eq!(
            row_head(
                vec![vec![
                    header_cell(1, i64::MAX),
                    header_cell(1, 1),
                    plain_cell(1, 1)
                ]],
                5
            ),
            0
        );
        let tall = vec![
            vec![header_cell(i64::MAX, i64::MAX), plain_cell(1, 1)],
            vec![plain_cell(i64::MIN, 1)],
        ];
        assert_eq!(row_head(tall, 7), 7);
        assert_eq!(row_head(vec![], 3), 0);
    }

    #[test]
    fn clips_spans_to_the_section() {
        let rows = vec![(Attr::default(), vec![plain_cell(9, 5), plain_cell(0, 0)])];
        let placed = body_section(rows, 1, 3);
        let shape: Vec<(i64, i64)> = placed[0]
            .cells
            .iter()
            .map(|c| (c.row_span, c.col_span))
            .collect();
        assert_eq!(shape, vec![(1, 1), (1, 1), (1, 1)]);
    }
}
