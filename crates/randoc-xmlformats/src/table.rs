use std::collections::HashMap;

use randoc_archive::xml::XmlElement;
use randoc_ast::*;
use randoc_core::Result;

use crate::limit_error;
use crate::read::{Read, attr, id, name};

pub(crate) fn check_grid(rows: usize, cols: usize, limit: usize) -> Result<usize> {
    rows.max(1)
        .checked_mul(cols.max(1))
        .filter(|size| *size <= limit)
        .ok_or_else(|| limit_error("Table cell count", limit))
}

pub(crate) fn alignment(s: &str) -> Alignment {
    match s {
        "left" => Alignment::AlignLeft,
        "right" => Alignment::AlignRight,
        "center" => Alignment::AlignCenter,
        _ => Alignment::AlignDefault,
    }
}

pub(crate) fn align_name(align: Alignment) -> &'static str {
    match align {
        Alignment::AlignLeft => "left",
        Alignment::AlignRight => "right",
        Alignment::AlignCenter => "center",
        Alignment::AlignDefault => "",
    }
}

fn rows(el: &XmlElement) -> impl Iterator<Item = &XmlElement> {
    el.elements().filter(|el| matches!(name(el), "row" | "tr"))
}

fn cells(el: &XmlElement) -> impl Iterator<Item = &XmlElement> {
    el.elements()
        .filter(|el| matches!(name(el), "entry" | "td" | "th"))
}

pub(crate) fn read<'a>(
    state: &mut Read<'a, '_>,
    el: &'a XmlElement,
    level: i64,
    depth: usize,
) -> Result<Block> {
    let docbook = state.dialect.docbook();
    let group = el.child_local("tgroup").unwrap_or(el);
    let cal = name(group) == "tgroup";
    let limit = state.context.limits.max_table_cells;
    let head: Vec<_> = group
        .child_local("thead")
        .into_iter()
        .flat_map(rows)
        .collect();
    let foot: Vec<_> = group
        .child_local("tfoot")
        .into_iter()
        .flat_map(rows)
        .collect();
    let mut bodies: Vec<Vec<_>> = group
        .children_local("tbody")
        .map(|body| rows(body).collect())
        .collect();
    if docbook && bodies.is_empty() {
        bodies.push(rows(group).collect());
    }
    let widest_head = head.iter().map(|row| cells(row).count()).max();
    let widest_body = bodies.iter().flatten().map(|row| cells(row).count()).max();
    let columns: Vec<_> = group
        .elements()
        .flat_map(|child| {
            let cols = matches!(name(child), "colspec" | "col").then_some(child);
            cols.into_iter().chain(
                child
                    .children_local("col")
                    .filter(|_| name(child) == "colgroup"),
            )
        })
        .collect();
    let widths: Vec<_> = columns
        .iter()
        .map(|column| {
            let value = if cal {
                attr(column, "colwidth").trim_end_matches('*')
            } else {
                attr(column, "width").strip_suffix('%')?
            };
            value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite() && *value >= 0.0)
        })
        .collect();
    let cols = if docbook {
        // A positive width can add columns. Alignment alone applies to existing columns.
        let width_cols = if widths.iter().flatten().any(|width| *width > 0.0) {
            columns.len()
        } else {
            0
        };
        width_cols.max(widest_head.max(widest_body).unwrap_or(0))
    } else {
        attr(group, "cols")
            .parse::<usize>()
            .unwrap_or_else(|_| columns.len().max(widest_body.or(widest_head).unwrap_or(0)))
    };
    let row_count = head
        .len()
        .saturating_add(foot.len())
        .saturating_add(bodies.iter().map(Vec::len).sum());
    let grid = check_grid(row_count, cols, limit)?;
    state.table_cells_left = state
        .table_cells_left
        .checked_sub(grid)
        .ok_or_else(|| limit_error("Document table cell count", limit))?;
    let widths = &widths[..widths.len().min(cols)];
    let has_width = widths.iter().any(Option::is_some);
    let total = if cal {
        if docbook && has_width {
            widths.iter().map(|w| w.unwrap_or(1.0)).sum()
        } else {
            widths.iter().flatten().sum()
        }
    } else {
        100.0
    };
    let mut table = Table {
        attr: if docbook {
            let mut attributes = id(el);
            attributes
                .classes
                .extend(attr(el, "class").split_whitespace().map(str::to_owned));
            for (field, key) in [("role", "role"), ("tabstyle", "custom-style")] {
                if let Some(value) = el.attr_local(field) {
                    attributes.attributes.push((key.into(), value.into()));
                }
            }
            attributes
        } else {
            Attr::default()
        },
        col_specs: (0..cols)
            .map(|i| ColSpec {
                align: columns
                    .get(i)
                    .map_or(Alignment::AlignDefault, |el| alignment(attr(el, "align"))),
                width: widths
                    .get(i)
                    .copied()
                    .flatten()
                    .or_else(|| (cal && docbook && has_width).then_some(1.0))
                    .filter(|_| total > 0.0 && total.is_finite())
                    .map_or(ColWidth::ColWidthDefault, |width| {
                        ColWidth::ColWidth(width / total)
                    }),
            })
            .collect(),
        ..Table::default()
    };
    if let Some(caption) = el.child_local(if docbook { "title" } else { "caption" }) {
        table.caption.long = if docbook {
            vec![Block::Plain(state.inlines(&caption.children, depth)?)]
        } else {
            state.blocks(&caption.children, level, depth)?
        };
    }
    let names: HashMap<_, _> = columns
        .iter()
        .enumerate()
        .filter_map(|(i, el)| Some((el.attr_local("colname")?, i)))
        .collect();
    let mut convert = |rows: &[&'a XmlElement]| -> Result<Vec<Row>> {
        let mut occupied = vec![0usize; cols];
        let mut result = Vec::new();
        for (r, row) in rows.iter().enumerate() {
            let mut out = Row::default();
            let mut col = 0;
            for cell in cells(row) {
                while col < cols && occupied[col] > r {
                    col += 1;
                }
                if col >= cols {
                    break;
                }
                let (mut rowspan, mut colspan) = (1usize, 1usize);
                if docbook && cal {
                    rowspan = attr(cell, "morerows")
                        .parse::<usize>()
                        .unwrap_or_default()
                        .saturating_add(1);
                    if let (Some(start), Some(end)) = (
                        names.get(attr(cell, "namest")),
                        names.get(attr(cell, "nameend")),
                    ) {
                        colspan = end.saturating_sub(*start).saturating_add(1);
                    }
                } else if !docbook && !cal {
                    rowspan = attr(cell, "rowspan").parse().unwrap_or(1).max(1);
                    colspan = attr(cell, "colspan").parse().unwrap_or(1).max(1);
                }
                rowspan = rowspan.min(rows.len() - r);
                colspan = colspan.min(cols - col);
                let free = occupied[col..col + colspan]
                    .iter()
                    .take_while(|end| **end <= r)
                    .count();
                colspan = colspan.min(free);
                occupied[col..col + colspan].fill(r + rowspan);
                col += colspan;
                out.cells.push(Cell {
                    attr: id(cell),
                    align: alignment(attr(cell, "align")),
                    row_span: rowspan as i64,
                    col_span: colspan as i64,
                    content: state.cell_blocks(cell, level, depth)?,
                });
            }
            out.cells.extend(
                (col..cols)
                    .filter(|i| occupied[*i] <= r)
                    .map(|_| Cell::default()),
            );
            result.push(out);
        }
        Ok(result)
    };
    table.head.rows = convert(&head)?;
    for body in bodies {
        table.bodies.push(TableBody {
            body: convert(&body)?,
            ..TableBody::default()
        });
    }
    table.foot.rows = convert(&foot)?;
    Ok(Block::Table(Box::new(table)))
}
