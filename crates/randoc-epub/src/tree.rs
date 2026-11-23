use randoc_ast::{Attr, Block, Inline, MetaValue, Row, Table};
use randoc_core::{Error, Result};

/// Bounds recursive calls to shared tree and HTML functions on a 2 MiB stack.
pub(crate) const STACK_DEPTH: usize = 128;

enum Node<'a> {
    Block(&'a Block),
    Inline(&'a Inline),
    Meta(&'a MetaValue),
}

pub(crate) fn inspect<'a>(
    blocks: &'a [Block],
    meta: impl Iterator<Item = &'a MetaValue>,
    max_depth: usize,
) -> Result<usize> {
    use Node::{Block as B, Inline as I, Meta as M};
    let mut pending = Vec::new();
    pending.extend(blocks.iter().map(|b| (1, B(b))));
    pending.extend(meta.map(|m| (1, M(m))));
    let mut cells = 0usize;
    while let Some((depth, node)) = pending.pop() {
        if depth > max_depth.min(STACK_DEPTH) {
            return Err(Error::Parse("the document is nested too deeply".into()));
        }
        let child = depth + 1;
        let mut add_inlines = |items: &'a [Inline]| {
            pending.extend(items.iter().map(|i| (child, I(i))));
        };
        match node {
            B(Block::Para(items) | Block::Plain(items) | Block::Header(_, _, items)) => {
                add_inlines(items);
            }
            B(Block::LineBlock(lines)) => lines.iter().for_each(|line| add_inlines(line)),
            B(Block::DefinitionList(items)) => {
                for (term, defs) in items {
                    pending.extend(term.iter().map(|i| (child, I(i))));
                    pending.extend(defs.iter().flatten().map(|b| (child, B(b))));
                }
            }
            B(Block::BlockQuote(items) | Block::Div(_, items))
            | I(Inline::Note(items))
            | M(MetaValue::MetaBlocks(items)) => {
                pending.extend(items.iter().map(|b| (child, B(b))));
            }
            B(Block::BulletList(items) | Block::OrderedList(_, items)) => {
                pending.extend(items.iter().flatten().map(|b| (child, B(b))));
            }
            B(Block::Figure(_, caption, items)) => {
                if let Some(short) = &caption.short {
                    add_inlines(short);
                }
                pending.extend(caption.long.iter().chain(items).map(|b| (child, B(b))));
            }
            B(Block::Table(table)) => {
                if let Some(short) = &table.caption.short {
                    add_inlines(short);
                }
                pending.extend(table.caption.long.iter().map(|b| (child, B(b))));
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
                    pending.extend(
                        row.cells
                            .iter()
                            .flat_map(|cell| &cell.content)
                            .map(|b| (child, B(b))),
                    );
                }
            }
            I(
                Inline::Emph(items)
                | Inline::Underline(items)
                | Inline::Strong(items)
                | Inline::Strikeout(items)
                | Inline::Superscript(items)
                | Inline::Subscript(items)
                | Inline::SmallCaps(items)
                | Inline::Quoted(_, items)
                | Inline::Link(_, items, _)
                | Inline::Image(_, items, _)
                | Inline::Span(_, items),
            )
            | M(MetaValue::MetaInlines(items)) => add_inlines(items),
            I(Inline::Cite(citations, items)) => {
                add_inlines(items);
                for citation in citations {
                    add_inlines(&citation.prefix);
                    add_inlines(&citation.suffix);
                }
            }
            M(MetaValue::MetaList(items)) => pending.extend(items.iter().map(|m| (child, M(m)))),
            M(MetaValue::MetaMap(items)) => pending.extend(items.values().map(|m| (child, M(m)))),
            _ => {}
        }
    }
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

pub(crate) fn block_attrs(block: &mut Block, mut visit: impl FnMut(&mut Attr)) {
    match block {
        Block::Header(_, attr, _)
        | Block::Div(attr, _)
        | Block::CodeBlock(attr, _)
        | Block::Figure(attr, _, _) => visit(attr),
        Block::Table(table) => {
            visit(&mut table.attr);
            visit(&mut table.head.attr);
            visit(&mut table.foot.attr);
            for body in &mut table.bodies {
                visit(&mut body.attr);
            }
            let rows = table
                .head
                .rows
                .iter_mut()
                .chain(
                    table
                        .bodies
                        .iter_mut()
                        .flat_map(|body| body.head.iter_mut().chain(&mut body.body)),
                )
                .chain(&mut table.foot.rows);
            for row in rows {
                visit(&mut row.attr);
                for cell in &mut row.cells {
                    visit(&mut cell.attr);
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn inline_attr(inline: &mut Inline) -> Option<&mut Attr> {
    match inline {
        Inline::Code(attr, _)
        | Inline::Span(attr, _)
        | Inline::Image(attr, _, _)
        | Inline::Link(attr, _, _) => Some(attr),
        _ => None,
    }
}
