use randoc_ast::{Alignment, Block, Cell, ColWidth, Row, Table};
use randoc_core::LogMessage;
use randoc_layout::{Doc, empty, literal, vcat};

use super::Renderer;

impl Renderer<'_> {
    pub(super) fn table(
        &mut self,
        table: &Table,
        list: super::blocks::ListDepth,
        level: usize,
    ) -> Doc {
        if level > 1 {
            return empty();
        }
        if level == 1
            && table
                .head
                .rows
                .iter()
                .chain(
                    table
                        .bodies
                        .iter()
                        .flat_map(|body| body.head.iter().chain(&body.body)),
                )
                .chain(&table.foot.rows)
                .flat_map(|row| &row.cells)
                .any(|cell| {
                    cell.content
                        .iter()
                        .any(|block| matches!(block, Block::Table(_)))
                })
        {
            self.messages
                .push(LogMessage::BlockNotRendered(show_table(table)));
        }
        let width: f64 = table
            .col_specs
            .iter()
            .filter_map(|column| match column.width {
                ColWidth::ColWidth(width) => Some(width),
                _ => None,
            })
            .sum();
        let mut percentages: Vec<_> = table
            .col_specs
            .iter()
            .map(|column| match column.width {
                ColWidth::ColWidth(col) if width > 0.0 => {
                    Some((100.0 * col / width).floor() as i64)
                }
                _ => None,
            })
            .collect();
        let remaining = 100_i64.saturating_sub(
            percentages
                .iter()
                .flatten()
                .copied()
                .fold(0_i64, i64::saturating_add),
        );
        if let Some(first) = percentages.iter_mut().flatten().next() {
            *first = first.saturating_add(remaining);
        }
        let columns = table
            .col_specs
            .iter()
            .zip(percentages)
            .map(|(column, percent)| {
                let align = alignment(column.align);
                match percent {
                    Some(percent) => format!("{align}{percent}%"),
                    None => align.to_owned(),
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let mut attributes = String::new();
        if width > 0.0 {
            attributes.push_str(&format!("width=\"{:.0}%\",", (width * 100.0).round()));
        }
        attributes.push_str(&format!("cols=\"{columns}\","));
        let header = !table.head.rows.is_empty();
        let footer = !table.foot.rows.is_empty();
        if header || footer {
            let options = match (header, footer) {
                (true, true) => "header,footer",
                (true, false) => "header",
                _ => "footer",
            };
            attributes.push_str(&format!("options=\"{options}\","));
        }
        let marker = if level == 0 { '|' } else { '!' };
        let fence = format!("{marker}===");
        let mut output = vec![
            self.caption(&table.caption, level),
            literal(format!("[{attributes}]")),
            literal(&fence),
        ];
        self.table_rows(
            table,
            &table.head.rows.iter().collect::<Vec<_>>(),
            list,
            level,
            marker,
            Section::Head,
            &mut output,
        );
        for body in &table.bodies {
            let rows = body.head.iter().chain(&body.body).collect::<Vec<_>>();
            self.table_rows(
                table,
                &rows,
                list,
                level,
                marker,
                Section::Body,
                &mut output,
            );
        }
        self.table_rows(
            table,
            &table.foot.rows.iter().collect::<Vec<_>>(),
            list,
            level,
            marker,
            Section::Foot,
            &mut output,
        );
        output.push(literal(fence));
        vcat(output)
    }

    #[allow(clippy::too_many_arguments)]
    fn table_rows(
        &mut self,
        table: &Table,
        rows: &[&Row],
        list: super::blocks::ListDepth,
        level: usize,
        marker: char,
        section: Section,
        output: &mut Vec<Doc>,
    ) {
        let rows = visible_rows(rows, table.col_specs.len(), section);
        let mut occupied = vec![0; table.col_specs.len()];
        for (index, row) in rows.iter().enumerate() {
            output.push(literal(self.row(
                table,
                row,
                index,
                &mut occupied,
                list,
                level,
                marker,
            )));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        table: &Table,
        cells: &[Placement<'_>],
        index: usize,
        occupied: &mut [usize],
        list: super::blocks::ListDepth,
        level: usize,
        marker: char,
    ) -> String {
        let mut output = String::new();
        let mut col = 0;
        for cell in cells {
            let rows = cell.rows;
            let cols = cell.cols;
            while occupied.get(col).is_some_and(|until| *until > index) {
                col += 1;
            }
            let (align, complex, content) = if let Some(cell) = cell.content {
                let complex = !cell.content.is_empty()
                    && !matches!(cell.content.as_slice(), [Block::Plain(_) | Block::Para(_)])
                    && !(level > 0
                        && cell
                            .content
                            .iter()
                            .all(|block| matches!(block, Block::Table(_))));
                (
                    cell.align,
                    complex,
                    self.blocks(&cell.content, list, level + 1).render(None),
                )
            } else {
                (Alignment::AlignDefault, false, String::new())
            };
            let mut prefix = String::new();
            if cols > 1 {
                prefix.push_str(&cols.to_string());
            }
            if rows > 1 {
                prefix.push_str(&format!(".{rows}"));
            }
            if cols > 1 || rows > 1 {
                prefix.push('+');
            }
            if table
                .col_specs
                .get(col)
                .is_none_or(|spec| spec.align != align)
            {
                prefix.push_str(alignment(align));
            }
            if complex {
                prefix.push('a');
            }
            if !output.is_empty() {
                if output.ends_with('\n') {
                    output.push('\n');
                } else {
                    output.push(' ');
                }
            }
            prefix.push(marker);
            output.push_str(&prefix);
            if complex {
                output.push('\n');
                output.push_str(&content);
                output.push('\n');
            } else {
                output.push_str(&content);
            }
            for until in occupied.iter_mut().skip(col).take(cols) {
                *until = index.saturating_add(rows);
            }
            col = col.saturating_add(cols);
        }
        while col < occupied.len() {
            if occupied[col] <= index {
                if !output.is_empty() {
                    output.push(' ');
                }
                output.push(marker);
            }
            col += 1;
        }
        if output.ends_with('\n') {
            output.push('\n');
        }
        output
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Head,
    Body,
    Foot,
}

struct Placement<'a> {
    content: Option<&'a Cell>,
    cols: usize,
    rows: usize,
}

// First place the original cells. Then shorten row spans when rows disappear.
fn visible_rows<'a>(rows: &[&'a Row], width: usize, section: Section) -> Vec<Vec<Placement<'a>>> {
    let mut occupied = vec![0_usize; width];
    let mut placed = Vec::new();
    let mut count = vec![0_usize; rows.len() + 1];
    for (index, row) in rows.iter().enumerate() {
        let mut cells = Vec::new();
        let mut cols = 0;
        for cell in &row.cells {
            while occupied.get(cols).is_some_and(|until| *until > index) {
                cols += 1;
            }
            let end = index
                .saturating_add(cell.row_span.max(1) as usize)
                .min(rows.len());
            let span = cell.col_span.max(1) as usize;
            for until in occupied.iter_mut().skip(cols).take(span) {
                *until = end;
            }
            cells.push((cols, cell, end));
            cols = cols.saturating_add(span);
        }
        let visible = !cells.is_empty() || occupied.iter().all(|until| *until <= index);
        count[index + 1] = count[index] + usize::from(visible);
        if visible {
            placed.push((index, cells));
        }
    }
    let end = placed
        .len()
        .saturating_sub(usize::from(section == Section::Foot));
    let mut output = Vec::with_capacity(placed.len());
    let mut spans = vec![(0, 0, 0); width];
    for (row, (original, cells)) in placed.into_iter().enumerate() {
        let mut result = Vec::new();
        let mut col = 0;
        for (start, cell, original_end) in cells {
            fill_gaps(&mut result, &spans, &mut col, start, row);
            let cols = cell.col_span.max(1) as usize;
            let span_end = if section == Section::Head {
                row + 1
            } else {
                count[original_end].min(end).max(count[original] + 1)
            };
            result.push(Placement {
                content: Some(cell),
                cols,
                rows: span_end - row,
            });
            for span in spans.iter_mut().skip(start).take(cols) {
                *span = (span_end, start, cols);
            }
            col = start.saturating_add(cols);
        }
        fill_gaps(&mut result, &spans, &mut col, width, row);
        output.push(result);
    }
    output
}

fn fill_gaps(
    output: &mut Vec<Placement<'_>>,
    spans: &[(usize, usize, usize)],
    col: &mut usize,
    end: usize,
    row: usize,
) {
    while *col < end && *col < spans.len() {
        let (until, start, size) = spans[*col];
        if until > row {
            *col += 1;
            continue;
        }
        let cols = if *col == start {
            size.max(1).min(end - *col)
        } else {
            1
        };
        output.push(Placement {
            content: None,
            rows: 1,
            cols,
        });
        *col += cols;
    }
}

fn alignment(value: Alignment) -> &'static str {
    match value {
        Alignment::AlignLeft => "<",
        Alignment::AlignCenter => "^",
        Alignment::AlignRight => ">",
        Alignment::AlignDefault => "",
    }
}

// The native renderer is outside this crate's dependencies. This summary
// uses Rust Debug for block and inline types it does not format.
fn show_table(table: &Table) -> String {
    let cols = table
        .col_specs
        .iter()
        .map(|col| {
            let width = match col.width {
                ColWidth::ColWidth(value) => {
                    format!("ColWidth {}", randoc_ast::json::show_double(value))
                }
                ColWidth::ColWidthDefault => "ColWidthDefault".into(),
            };
            format!("({:?},{width})", col.align)
        })
        .collect::<Vec<_>>()
        .join(",");
    let bodies = table
        .bodies
        .iter()
        .map(|body| {
            let rows = body
                .body
                .iter()
                .map(|row| {
                    let cells = row
                        .cells
                        .iter()
                        .map(|cell| {
                            let contents = cell
                                .content
                                .iter()
                                .map(|block| match block {
                                    Block::Table(table) => show_table(table),
                                    Block::Plain(xs) | Block::Para(xs) => {
                                        let inlines = xs
                                            .iter()
                                            .map(|x| match x {
                                                randoc_ast::Inline::Str(text) => format!(
                                                    "Str {}",
                                                    randoc_ast::json::show_string(text)
                                                ),
                                                randoc_ast::Inline::Space => "Space".into(),
                                                _ => format!("{x:?}"),
                                            })
                                            .collect::<Vec<_>>()
                                            .join(",");
                                        let kind = if matches!(block, Block::Plain(_)) {
                                            "Plain"
                                        } else {
                                            "Para"
                                        };
                                        format!("{kind} [{inlines}]")
                                    }
                                    _ => format!("{block:?}"),
                                })
                                .collect::<Vec<_>>()
                                .join(",");
                            format!(
                                "Cell (\"\",[],[]) {:?} (RowSpan {}) (ColSpan {}) [{contents}]",
                                cell.align, cell.row_span, cell.col_span
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    format!("Row (\"\",[],[]) [{cells}]")
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "TableBody (\"\",[],[]) (RowHeadColumns {}) [] [{rows}]",
                body.row_head_columns
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "Table (\"\",[],[]) (Caption Nothing []) [{cols}] (TableHead (\"\",[],[]) []) [{bodies}] (TableFoot (\"\",[],[]) [])"
    )
}
