use randoc_ast::*;
use randoc_core::Result;

use crate::{limit_error, table::check_grid};

enum Ast<'a> {
    Block(&'a Block),
    Inline(&'a Inline),
    Meta(&'a MetaValue),
}

pub(crate) fn validate(
    doc: &Pandoc,
    max_depth: usize,
    max_input: usize,
    max_cells: usize,
) -> Result<()> {
    let mut pending: Vec<_> = doc
        .blocks
        .iter()
        .map(|b| (1, Ast::Block(b)))
        .chain(doc.meta.0.values().map(|m| (1, Ast::Meta(m))))
        .collect();
    let mut bytes = 0usize;
    let mut table_cells_left = max_cells;
    while let Some((depth, node)) = pending.pop() {
        if depth > max_depth {
            return Err(limit_error("XML writer nesting depth", max_depth));
        }
        bytes = bytes.saturating_add(1);
        let child = depth + 1;
        let attributes = match &node {
            Ast::Block(
                Block::Header(_, a, _)
                | Block::Div(a, _)
                | Block::CodeBlock(a, _)
                | Block::Figure(a, _, _),
            ) => Some(a),
            Ast::Inline(
                Inline::Code(a, _)
                | Inline::Link(a, ..)
                | Inline::Image(a, ..)
                | Inline::Span(a, _),
            ) => Some(a),
            _ => None,
        };
        if let Some(a) = attributes {
            bytes = bytes.saturating_add(attr_bytes(a));
        }
        match node {
            Ast::Meta(value) => match value {
                MetaValue::MetaMap(map) => {
                    for (key, value) in map {
                        bytes = bytes.saturating_add(key.len());
                        pending.push((child, Ast::Meta(value)));
                    }
                }
                MetaValue::MetaList(items) => {
                    pending.extend(items.iter().map(|m| (child, Ast::Meta(m))))
                }
                MetaValue::MetaBool(_) => {}
                MetaValue::MetaString(text) => bytes = bytes.saturating_add(text.len()),
                MetaValue::MetaBlocks(blocks) => add_blocks(&mut pending, child, blocks),
                MetaValue::MetaInlines(inlines) => add_inlines(&mut pending, child, inlines),
            },
            Ast::Block(block) => match block {
                Block::Plain(lines) | Block::Para(lines) | Block::Header(_, _, lines) => {
                    add_inlines(&mut pending, child, lines)
                }
                Block::LineBlock(lines) => {
                    for line in lines {
                        add_inlines(&mut pending, child, line);
                    }
                }
                Block::CodeBlock(_, text) | Block::RawBlock(_, text) => {
                    bytes = bytes.saturating_add(text.len())
                }
                Block::BlockQuote(blocks) | Block::Div(_, blocks) => {
                    add_blocks(&mut pending, child, blocks)
                }
                Block::OrderedList(_, items) | Block::BulletList(items) => {
                    for blocks in items {
                        add_blocks(&mut pending, child, blocks);
                    }
                }
                Block::DefinitionList(items) => {
                    for (term, definitions) in items {
                        add_inlines(&mut pending, child, term);
                        for blocks in definitions {
                            add_blocks(&mut pending, child, blocks);
                        }
                    }
                }
                Block::Figure(_, caption, blocks) => {
                    add_caption(&mut pending, child, caption);
                    add_blocks(&mut pending, child, blocks);
                }
                Block::Table(table) => {
                    bytes = bytes
                        .saturating_add(attr_bytes(&table.attr))
                        .saturating_add(attr_bytes(&table.head.attr))
                        .saturating_add(attr_bytes(&table.foot.attr));
                    for body in &table.bodies {
                        bytes = bytes.saturating_add(attr_bytes(&body.attr));
                    }
                    add_caption(&mut pending, child, &table.caption);
                    let rows = table
                        .head
                        .rows
                        .iter()
                        .chain(
                            table
                                .bodies
                                .iter()
                                .flat_map(|body| body.head.iter().chain(&body.body)),
                        )
                        .chain(&table.foot.rows);
                    let mut count = 0usize;
                    let mut cols = table.col_specs.len();
                    for row in rows {
                        count += 1;
                        bytes = bytes.saturating_add(attr_bytes(&row.attr));
                        let mut span = 0usize;
                        for cell in &row.cells {
                            bytes = bytes.saturating_add(attr_bytes(&cell.attr));
                            let row_span =
                                usize::try_from(cell.row_span.max(1)).unwrap_or(usize::MAX);
                            let col_span =
                                usize::try_from(cell.col_span.max(1)).unwrap_or(usize::MAX);
                            check_grid(row_span, col_span, max_cells)?;
                            span = span.saturating_add(col_span);
                            add_blocks(&mut pending, child, &cell.content);
                        }
                        cols = cols.max(span);
                        check_grid(count, cols, max_cells)?;
                    }
                    let grid = check_grid(count, cols, max_cells)?;
                    table_cells_left = table_cells_left
                        .checked_sub(grid)
                        .ok_or_else(|| limit_error("Document table cell count", max_cells))?;
                }
                Block::HorizontalRule => {}
            },
            Ast::Inline(inline) => match inline {
                Inline::Str(text)
                | Inline::Code(_, text)
                | Inline::Math(_, text)
                | Inline::RawInline(_, text) => {
                    bytes = bytes.saturating_add(text.len());
                }
                Inline::Emph(xs)
                | Inline::Underline(xs)
                | Inline::Strong(xs)
                | Inline::Strikeout(xs)
                | Inline::Superscript(xs)
                | Inline::Subscript(xs)
                | Inline::SmallCaps(xs)
                | Inline::Quoted(_, xs)
                | Inline::Span(_, xs) => add_inlines(&mut pending, child, xs),
                Inline::Image(_, xs, target) | Inline::Link(_, xs, target) => {
                    bytes = bytes
                        .saturating_add(target.url.len())
                        .saturating_add(target.title.len());
                    add_inlines(&mut pending, child, xs);
                }
                Inline::Cite(cites, xs) => {
                    add_inlines(&mut pending, child, xs);
                    for cite in cites {
                        bytes = bytes.saturating_add(cite.id.len());
                        add_inlines(&mut pending, child, &cite.prefix);
                        add_inlines(&mut pending, child, &cite.suffix);
                    }
                }
                Inline::Note(blocks) => add_blocks(&mut pending, child, blocks),
                Inline::Space | Inline::SoftBreak | Inline::LineBreak => {}
            },
        }
        if bytes > max_input {
            return Err(limit_error("XML writer input size", max_input));
        }
    }
    Ok(())
}

fn add_blocks<'a>(pending: &mut Vec<(usize, Ast<'a>)>, depth: usize, blocks: &'a [Block]) {
    pending.extend(blocks.iter().map(|block| (depth, Ast::Block(block))));
}

fn add_inlines<'a>(pending: &mut Vec<(usize, Ast<'a>)>, depth: usize, inlines: &'a [Inline]) {
    pending.extend(inlines.iter().map(|inline| (depth, Ast::Inline(inline))));
}

fn add_caption<'a>(pending: &mut Vec<(usize, Ast<'a>)>, depth: usize, caption: &'a Caption) {
    add_blocks(pending, depth, &caption.long);
    if let Some(short) = &caption.short {
        add_inlines(pending, depth, short);
    }
}

fn attr_bytes(attr: &Attr) -> usize {
    let mut bytes = attr.id.len();
    for class in &attr.classes {
        bytes = bytes.saturating_add(class.len());
    }
    for (key, value) in &attr.attributes {
        bytes = bytes.saturating_add(key.len()).saturating_add(value.len());
    }
    bytes
}
