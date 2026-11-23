use randoc_ast::{Block, MetaValue, Row, Table};
use randoc_core::Result;
use randoc_core::shared::chunks::inspect_tree;

pub(crate) use randoc_core::shared::chunks::{block_attrs, inline_attr};

/// Bounds recursive calls to shared tree and HTML functions on a 2 MiB stack.
pub(crate) const STACK_DEPTH: usize = 128;

pub(crate) fn inspect<'a>(
    blocks: &'a [Block],
    meta: impl Iterator<Item = &'a MetaValue>,
    max_depth: usize,
) -> Result<usize> {
    let mut cells = 0usize;
    inspect_tree(blocks, meta, max_depth.min(STACK_DEPTH), |table| {
        let width = table.col_specs.len();
        cells = cells.saturating_add(width);
        for row in rows(table) {
            // The HTML reader pads short rows with empty cells.
            let written = row
                .cells
                .iter()
                .filter(|cell| **cell != Default::default())
                .count();
            cells = cells.saturating_add(width.saturating_sub(written));
        }
    })?;
    Ok(cells)
}

fn rows(table: &Table) -> impl Iterator<Item = &Row> {
    table
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
}
