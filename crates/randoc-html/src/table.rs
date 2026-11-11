//! Tables: the normalization of rows and spans, and the HTML form.

use randoc_ast::{Alignment, Attr, Block, Cell, ColWidth, Row, Table};
use randoc_core::Result;

use crate::html::{Attrs, Html};
use randoc_layout::{Doc, blankline, cr, empty};

use crate::shared::lit;

/// A cell after normalization, with the column where it starts.
struct PlacedCell<'a> {
    column: usize,
    attr: &'a Attr,
    align: Alignment,
    row_span: usize,
    col_span: usize,
    content: &'a [Block],
}

/// A row after normalization.
struct PlacedRow<'a> {
    attr: &'a Attr,
    cells: Vec<PlacedCell<'a>>,
}

/// Places the cells of a group of rows on a grid of `columns` columns.
///
/// A span that leaves the group or the grid is cut, and a cell in the first
/// `row_heads` columns does not span past them. A free grid position gets an
/// empty cell. A cell that has no free position is dropped. The time is
/// linear in the number of cells and grid positions.
fn normalize(rows: &[Row], columns: usize, row_heads: usize) -> Vec<PlacedRow<'_>> {
    #[cfg(test)]
    steps::add(columns);
    let mut covered = vec![0usize; columns];
    let mut out = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let rows_left = rows.len() - index;
        let mut taken: Vec<bool> = covered.iter().map(|&n| n > 0).collect();
        let mut placed = Vec::new();
        let mut cells = row.cells.iter();
        let mut column = 0;
        'cells: for cell in cells.by_ref() {
            while taken.get(column).copied().unwrap_or(false) {
                column += 1;
            }
            if column >= columns {
                break 'cells;
            }
            let limit = if column < row_heads {
                row_heads.min(columns) - column
            } else {
                columns - column
            };
            let wanted = span(cell.col_span).min(limit);
            let col_span = free_run(taken.get(column..column + wanted).unwrap_or_default());
            let row_span = span(cell.row_span).min(rows_left);
            for position in column..column + col_span {
                if let Some(slot) = taken.get_mut(position) {
                    *slot = true;
                }
                if let Some(slot) = covered.get_mut(position) {
                    *slot = row_span;
                }
            }
            placed.push(place(cell, column, row_span, col_span));
            column += col_span;
        }
        for (position, is_taken) in taken.iter().enumerate() {
            if !is_taken {
                placed.push(PlacedCell {
                    column: position,
                    attr: &EMPTY_ATTR,
                    align: Alignment::AlignDefault,
                    row_span: 1,
                    col_span: 1,
                    content: &[],
                });
            }
        }
        placed.sort_by_key(|cell| cell.column);
        for count in &mut covered {
            *count = count.saturating_sub(1);
        }
        out.push(PlacedRow {
            attr: &row.attr,
            cells: placed,
        });
    }
    out
}

/// Returns the number of free positions at the start of `slots`.
fn free_run(slots: &[bool]) -> usize {
    let mut count = 0;
    for taken in slots {
        #[cfg(test)]
        steps::add(1);
        if *taken {
            break;
        }
        count += 1;
    }
    count
}

static EMPTY_ATTR: Attr = Attr {
    id: String::new(),
    classes: Vec::new(),
    attributes: Vec::new(),
};

fn span(value: i64) -> usize {
    usize::try_from(value).unwrap_or(0).max(1)
}

fn place(cell: &Cell, column: usize, row_span: usize, col_span: usize) -> PlacedCell<'_> {
    PlacedCell {
        column,
        attr: &cell.attr,
        align: cell.align,
        row_span,
        col_span,
        content: &cell.content,
    }
}

/// Returns `true` when each cell of the rows is a default cell: no content,
/// no attributes, the default alignment and no span. Pandoc leaves out a
/// table head or foot of such rows.
fn all_blank(rows: &[PlacedRow<'_>]) -> bool {
    rows.iter().all(|row| {
        row.cells.iter().all(|cell| {
            cell.content.is_empty()
                && cell.attr.is_empty()
                && cell.align == Alignment::AlignDefault
                && cell.row_span == 1
                && cell.col_span == 1
        })
    })
}

impl Html<'_> {
    /// Renders a table.
    /// `extra` are attributes after the own attributes of the table.
    pub(crate) fn table(&mut self, table: &Table, depth: usize, extra: Attrs) -> Result<Doc> {
        let columns = table.col_specs.len();
        let aligns: Vec<Alignment> = table.col_specs.iter().map(|spec| spec.align).collect();
        let mut inner = empty();
        if !table.caption.long.is_empty() {
            let caption = self.caption_body(&table.caption, depth)?;
            inner += self.inline_element("caption", &[], caption);
            inner += lit("\n");
        }
        let has_widths = table
            .col_specs
            .iter()
            .any(|spec| matches!(spec.width, ColWidth::ColWidth(_)));
        let total_width: f64 = table
            .col_specs
            .iter()
            .map(|spec| match spec.width {
                ColWidth::ColWidth(width) => width,
                ColWidth::ColWidthDefault => 0.0,
            })
            .sum();
        if has_widths {
            let mut cols = empty();
            for spec in &table.col_specs {
                cols += cr();
                let attrs = match spec.width {
                    ColWidth::ColWidth(width) => {
                        let percent = format!("{:.0}%", (width * 100.0).floor());
                        if self.html5() {
                            vec![("style".to_owned(), format!("width: {percent}"))]
                        } else {
                            vec![("width".to_owned(), percent)]
                        }
                    }
                    _ => Vec::new(),
                };
                cols += self.void("col", &attrs);
            }
            inner += cr();
            inner += self.section_element("colgroup", &[], cols);
            inner += lit("\n");
        }
        let head = normalize(&table.head.rows, columns, 0);
        if !all_blank(&head) {
            let body = self.rows(&head, &aligns, usize::MAX, depth)?;
            inner += cr();
            inner += self.section_element("thead", &self.attrs(&table.head.attr), body);
            inner += lit("\n");
        }
        for body in &table.bodies {
            let row_heads = usize::try_from(body.row_head_columns).unwrap_or(0);
            let mut content = empty();
            if !body.head.is_empty() {
                let rows = normalize(&body.head, columns, 0);
                content += self.rows(&rows, &aligns, usize::MAX, depth)?;
                content += blankline();
            }
            let rows = normalize(&body.body, columns, row_heads);
            content += self.rows(&rows, &aligns, row_heads, depth)?;
            inner += cr();
            inner += self.section_element("tbody", &self.attrs(&body.attr), content);
        }
        let foot = normalize(&table.foot.rows, columns, 0);
        if !all_blank(&foot) {
            let rows = foot;
            let body = self.rows(&rows, &aligns, 0, depth)?;
            inner += self.section_element("tfoot", &self.attrs(&table.foot.attr), body);
            inner += lit("\n");
        }
        let mut table_attrs = self.attrs(&table.attr);
        if has_widths && total_width > 0.0 && total_width < 1.0 {
            let percent = (total_width * 100.0).floor() as i64;
            table_attrs.push(("style".to_owned(), format!("width:{percent}%;")));
        }
        table_attrs.extend(extra);
        let mut doc = self.open("table", &table_attrs);
        doc += lit("\n");
        doc += inner;
        doc += lit("\n");
        doc += lit("</table>");
        Ok(doc)
    }

    /// Returns an element whose children each start on a new line.
    fn section_element(&self, name: &str, attrs: &[(String, String)], body: Doc) -> Doc {
        let mut doc = self.open(name, attrs);
        doc += lit("\n");
        doc += body;
        doc += cr();
        doc += lit(&format!("</{name}>"));
        doc
    }

    /// Renders rows. A cell is a heading cell when its column is before `row_heads`.
    fn rows(
        &mut self,
        rows: &[PlacedRow<'_>],
        aligns: &[Alignment],
        row_heads: usize,
        depth: usize,
    ) -> Result<Doc> {
        let mut doc = empty();
        for row in rows {
            let mut cells = empty();
            for cell in &row.cells {
                cells += cr();
                let name = if cell.column < row_heads { "th" } else { "td" };
                let mut attrs = Vec::new();
                if cell.col_span > 1 {
                    attrs.push(("colspan".to_owned(), cell.col_span.to_string()));
                }
                if cell.row_span > 1 {
                    attrs.push(("rowspan".to_owned(), cell.row_span.to_string()));
                }
                let align = match cell.align {
                    Alignment::AlignDefault => aligns
                        .get(cell.column)
                        .copied()
                        .unwrap_or(Alignment::AlignDefault),
                    other => other,
                };
                let align = match align {
                    Alignment::AlignLeft => Some("left"),
                    Alignment::AlignRight => Some("right"),
                    Alignment::AlignCenter => Some("center"),
                    Alignment::AlignDefault => None,
                };
                // The alignment comes before the key-value pairs of the cell, or
                // takes the value of a pair with the same key, as in Pandoc.
                let mut pairs: Vec<(String, String)> = cell.attr.attributes.clone();
                if let Some(align) = align {
                    let generated = if self.html5() {
                        ("style".to_owned(), format!("text-align: {align};"))
                    } else {
                        ("align".to_owned(), align.to_owned())
                    };
                    match pairs.iter_mut().find(|(key, _)| *key == generated.0) {
                        Some(pair) if self.html5() => pair.1 = merge_alignment(align, &pair.1),
                        Some(pair) => pair.1 = generated.1,
                        None => pairs.insert(0, generated),
                    }
                }
                let cell_attr = Attr::new(cell.attr.id.clone(), cell.attr.classes.clone(), pairs);
                attrs.extend(self.attrs(&cell_attr));
                let content = self.blocks(cell.content, depth)?;
                cells += self.inline_element(name, &attrs, content);
            }
            doc += cr();
            doc += self.section_element("tr", &self.attrs(row.attr), cells);
        }
        Ok(doc)
    }
}

/// Returns the `style` value of an aligned HTML5 cell with its own `style`.
/// The `text-align` declaration takes the place of the first one in the
/// style, or comes first. A style that does not parse gives no declarations.
fn merge_alignment(align: &str, style: &str) -> String {
    let mut declarations = css_declarations(style);
    let value = align.to_owned();
    match declarations
        .iter_mut()
        .find(|(property, _)| property == "text-align")
    {
        Some(declaration) => declaration.1 = value,
        None => declarations.insert(0, ("text-align".to_owned(), value)),
    }
    declarations
        .iter()
        .map(|(property, value)| format!("{property}: {value};"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parses the declarations of a `style` value as Pandoc does. Each declaration
/// is a property, a colon and a value, with an optional semicolon and spaces
/// after it. The parse stops at a colon where a property must start. A
/// declaration that starts and does not end makes the result empty.
fn css_declarations(style: &str) -> Vec<(String, String)> {
    let trim = |text: &str| {
        text.trim_matches(|c: char| c.is_ascii_whitespace())
            .to_owned()
    };
    let mut declarations = Vec::new();
    let mut rest = style;
    while !rest.is_empty() && !rest.starts_with(':') {
        let Some((property, after)) = rest.split_once(':') else {
            return Vec::new();
        };
        let end = after.find([':', ';']).unwrap_or(after.len());
        if end == 0 {
            return Vec::new();
        }
        let (value, after) = after.split_at(end);
        declarations.push((trim(property), trim(value)));
        let after = after.strip_prefix(';').unwrap_or(after);
        rest = after.trim_start_matches(|c: char| c.is_ascii_whitespace());
    }
    declarations
}

/// A step counter of `normalize`, only in test builds.
#[cfg(test)]
pub(crate) mod steps {
    use std::cell::Cell;

    thread_local!(static STEPS: Cell<usize> = const { Cell::new(0) });

    pub(crate) fn add(count: usize) {
        STEPS.with(|steps| steps.set(steps.get() + count));
    }

    /// Returns the steps since the last call and sets the counter to zero.
    pub(crate) fn take() -> usize {
        STEPS.with(|steps| steps.replace(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pandoc 3.13 writes an aligned cell with these styles as
    /// `style="text-align: right;"` (no declaration), `"text-align: right; a: b;"`
    /// and `"text-align: right; \u{a0}a: \u{a0}b\u{a0};"`.
    #[test]
    fn css_declarations_follow_the_oracle_at_the_edges() {
        let pair = |p: &str, v: &str| (p.to_owned(), v.to_owned());
        assert_eq!(css_declarations("a:;b:c"), vec![]);
        assert_eq!(css_declarations("a::b"), vec![]);
        assert_eq!(css_declarations("a:b; :c"), vec![pair("a", "b")]);
        assert_eq!(
            css_declarations("\u{a0}a:\u{a0}b\u{a0}"),
            vec![pair("\u{a0}a", "\u{a0}b\u{a0}")]
        );
    }

    /// Pandoc 3.13 replaces an own `align` pair of an aligned HTML4 cell, and
    /// keeps it as `data-align` in HTML5.
    #[test]
    fn an_aligned_cell_with_an_align_pair() {
        let tree = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Table","c":[["",[],[]],[null,[]],[[{"t":"AlignRight"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[["align","left"]]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"p"}]}]]]]]]],[["",[],[]],[]]]}]}"#;
        let doc = randoc_ast::json::from_json(tree).unwrap();
        let write = |format: &str| {
            crate::writer(format)
                .unwrap()
                .write(
                    &doc,
                    &randoc_core::WriterOptions::default(),
                    &mut randoc_core::Context::pure(),
                )
                .unwrap()
        };
        let table = |cell: &str| {
            randoc_core::Output::Text(format!(
                "<table>\n<tbody>\n<tr>\n{cell}\n</tr>\n</tbody>\n</table>"
            ))
        };
        assert_eq!(write("html4"), table(r#"<td align="right">p</td>"#));
        assert_eq!(
            write("html"),
            table(r#"<td style="text-align: right;" data-align="left">p</td>"#)
        );
    }

    fn cell(row_span: i64, col_span: i64) -> Cell {
        Cell {
            row_span,
            col_span,
            ..Cell::default()
        }
    }

    fn row(cells: Vec<Cell>) -> Row {
        Row {
            attr: Attr::default(),
            cells,
        }
    }

    fn shape(rows: &[PlacedRow<'_>]) -> Vec<Vec<(usize, usize, usize)>> {
        rows.iter()
            .map(|r| {
                r.cells
                    .iter()
                    .map(|c| (c.column, c.row_span, c.col_span))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn cuts_spans_and_fills_free_positions() {
        // Pandoc 3.13 writes this body as "r" (rowspan 3), "y" (colspan 2),
        // two empty cells, then "z" and "w"; the third cell of the last row is dropped.
        let rows = vec![
            row(vec![cell(3, 1), cell(1, 5)]),
            row(vec![]),
            row(vec![cell(1, 1), cell(1, 1), cell(1, 1)]),
        ];
        let placed = normalize(&rows, 3, 0);
        assert_eq!(
            shape(&placed),
            vec![
                vec![(0, 3, 1), (1, 1, 2)],
                vec![(1, 1, 1), (2, 1, 1)],
                vec![(1, 1, 1), (2, 1, 1)],
            ]
        );
    }

    #[test]
    fn normalization_takes_linear_steps() {
        let steps_for = |count: usize| {
            let rows = vec![row(vec![cell(1, 1); count])];
            steps::take();
            let placed = normalize(&rows, count, 0);
            assert_eq!(placed.first().map(|r| r.cells.len()), Some(count));
            steps::take()
        };
        let small = steps_for(10_000);
        let large = steps_for(40_000);
        // Linear work gives a ratio of 4; the old quadratic scan gave 16.
        assert!(large <= small * 5, "{small} steps, then {large} steps");
        // A wide span scans only the positions that it takes.
        let rows = vec![row(vec![cell(1, i64::MAX); 3])];
        steps::take();
        normalize(&rows, 100_000, 0);
        assert!(steps::take() < 300_100);
    }

    #[test]
    fn a_row_span_stops_at_the_end_of_the_group_and_bad_spans_become_one() {
        let rows = vec![row(vec![cell(5, 0), cell(-1, 1)])];
        assert_eq!(
            shape(&normalize(&rows, 2, 0)),
            vec![vec![(0, 1, 1), (1, 1, 1)]]
        );
        assert_eq!(shape(&normalize(&rows, 0, 0)), vec![Vec::new()]);
    }
}
