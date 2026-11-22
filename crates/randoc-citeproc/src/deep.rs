//! Walks of pandoc trees with a heap stack: the depth check and the drop of
//! a deep tree. The derived `Drop` and `Clone` of the AST recurse, so the
//! crate checks the depth before it clones and takes deep trees apart
//! before it drops them (review citeproc-R3b-04).

use randoc_ast::{Block, Inline, MetaValue, Pandoc, Row};

use crate::markup::TooDeep;

enum Node<'a> {
    Inline(&'a Inline),
    Block(&'a Block),
    Meta(&'a MetaValue),
}

/// Checks that no part of a document (the metadata and the blocks) is
/// nested deeper than `max_depth`. The walks of the citation filter recurse
/// once for each level, so this check comes first.
pub(crate) fn check_document(doc: &Pandoc, max_depth: usize) -> Result<(), TooDeep> {
    let pending = doc
        .meta
        .0
        .values()
        .map(Node::Meta)
        .chain(doc.blocks.iter().map(Node::Block))
        .map(|node| (node, 1))
        .collect();
    check(pending, max_depth)
}

/// Checks that no inline and no block (also in a note) is nested deeper
/// than `max_depth`.
pub(crate) fn check_inlines(inlines: &[Inline], max_depth: usize) -> Result<(), TooDeep> {
    check(
        inlines.iter().map(|i| (Node::Inline(i), 1)).collect(),
        max_depth,
    )
}

fn check(mut pending: Vec<(Node<'_>, usize)>, max_depth: usize) -> Result<(), TooDeep> {
    let mut children = Vec::new();
    while let Some((node, depth)) = pending.pop() {
        if depth > max_depth {
            return Err(TooDeep);
        }
        push_children(node, &mut children);
        pending.extend(children.drain(..).map(|child| (child, depth + 1)));
    }
    Ok(())
}

fn push_children<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
    let inlines = |parts: &'a [Inline], out: &mut Vec<Node<'a>>| {
        out.extend(parts.iter().map(Node::Inline));
    };
    let blocks = |parts: &'a [Block], out: &mut Vec<Node<'a>>| {
        out.extend(parts.iter().map(Node::Block));
    };
    let rows = |rows: &'a [Row], out: &mut Vec<Node<'a>>| {
        for row in rows {
            for cell in &row.cells {
                out.extend(cell.content.iter().map(Node::Block));
            }
        }
    };
    match node {
        Node::Meta(value) => match value {
            MetaValue::MetaMap(map) => out.extend(map.values().map(Node::Meta)),
            MetaValue::MetaList(list) => out.extend(list.iter().map(Node::Meta)),
            MetaValue::MetaInlines(parts) => inlines(parts, out),
            MetaValue::MetaBlocks(parts) => blocks(parts, out),
            MetaValue::MetaBool(_) | MetaValue::MetaString(_) => {}
        },
        Node::Inline(inline) => match inline {
            Inline::Emph(parts)
            | Inline::Underline(parts)
            | Inline::Strong(parts)
            | Inline::Strikeout(parts)
            | Inline::Superscript(parts)
            | Inline::Subscript(parts)
            | Inline::SmallCaps(parts)
            | Inline::Quoted(_, parts)
            | Inline::Link(_, parts, _)
            | Inline::Image(_, parts, _)
            | Inline::Span(_, parts) => inlines(parts, out),
            Inline::Cite(citations, parts) => {
                inlines(parts, out);
                for citation in citations {
                    inlines(&citation.prefix, out);
                    inlines(&citation.suffix, out);
                }
            }
            Inline::Note(parts) => blocks(parts, out),
            Inline::Str(_)
            | Inline::Code(..)
            | Inline::Space
            | Inline::SoftBreak
            | Inline::LineBreak
            | Inline::Math(..)
            | Inline::RawInline(..) => {}
        },
        Node::Block(block) => match block {
            Block::Plain(parts) | Block::Para(parts) | Block::Header(_, _, parts) => {
                inlines(parts, out);
            }
            Block::LineBlock(lines) => lines.iter().for_each(|line| inlines(line, out)),
            Block::BlockQuote(parts) | Block::Div(_, parts) => blocks(parts, out),
            Block::OrderedList(_, items) | Block::BulletList(items) => {
                items.iter().for_each(|item| blocks(item, out));
            }
            Block::DefinitionList(items) => {
                for (term, definitions) in items {
                    inlines(term, out);
                    definitions.iter().for_each(|d| blocks(d, out));
                }
            }
            Block::Figure(_, caption, parts) => {
                if let Some(short) = &caption.short {
                    inlines(short, out);
                }
                blocks(&caption.long, out);
                blocks(parts, out);
            }
            Block::Table(table) => {
                if let Some(short) = &table.caption.short {
                    inlines(short, out);
                }
                blocks(&table.caption.long, out);
                rows(&table.head.rows, out);
                for body in &table.bodies {
                    rows(&body.head, out);
                    rows(&body.body, out);
                }
                rows(&table.foot.rows, out);
            }
            Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => {}
        },
    }
}

enum Owned {
    Inline(Inline),
    Block(Block),
}

/// Drops inlines of any depth: each node gives its children to a heap stack
/// before it drops.
pub(crate) fn drop_inlines(inlines: Vec<Inline>) {
    let mut pending: Vec<Owned> = inlines.into_iter().map(Owned::Inline).collect();
    while let Some(mut node) = pending.pop() {
        take_children(&mut node, &mut pending);
    }
}

fn take_children(node: &mut Owned, out: &mut Vec<Owned>) {
    let inlines = |parts: &mut Vec<Inline>, out: &mut Vec<Owned>| {
        out.extend(std::mem::take(parts).into_iter().map(Owned::Inline));
    };
    let blocks = |parts: &mut Vec<Block>, out: &mut Vec<Owned>| {
        out.extend(std::mem::take(parts).into_iter().map(Owned::Block));
    };
    let rows = |rows: &mut Vec<Row>, out: &mut Vec<Owned>| {
        for row in rows {
            for cell in &mut row.cells {
                out.extend(
                    std::mem::take(&mut cell.content)
                        .into_iter()
                        .map(Owned::Block),
                );
            }
        }
    };
    match node {
        Owned::Inline(inline) => match inline {
            Inline::Emph(parts)
            | Inline::Underline(parts)
            | Inline::Strong(parts)
            | Inline::Strikeout(parts)
            | Inline::Superscript(parts)
            | Inline::Subscript(parts)
            | Inline::SmallCaps(parts)
            | Inline::Quoted(_, parts)
            | Inline::Link(_, parts, _)
            | Inline::Image(_, parts, _)
            | Inline::Span(_, parts) => inlines(parts, out),
            Inline::Cite(citations, parts) => {
                inlines(parts, out);
                for citation in citations {
                    inlines(&mut citation.prefix, out);
                    inlines(&mut citation.suffix, out);
                }
            }
            Inline::Note(parts) => blocks(parts, out),
            Inline::Str(_)
            | Inline::Code(..)
            | Inline::Space
            | Inline::SoftBreak
            | Inline::LineBreak
            | Inline::Math(..)
            | Inline::RawInline(..) => {}
        },
        Owned::Block(block) => match block {
            Block::Plain(parts) | Block::Para(parts) | Block::Header(_, _, parts) => {
                inlines(parts, out);
            }
            Block::LineBlock(lines) => lines.iter_mut().for_each(|line| inlines(line, out)),
            Block::BlockQuote(parts) | Block::Div(_, parts) => blocks(parts, out),
            Block::OrderedList(_, items) | Block::BulletList(items) => {
                items.iter_mut().for_each(|item| blocks(item, out));
            }
            Block::DefinitionList(items) => {
                for (term, definitions) in items {
                    inlines(term, out);
                    definitions.iter_mut().for_each(|d| blocks(d, out));
                }
            }
            Block::Figure(_, caption, parts) => {
                if let Some(short) = &mut caption.short {
                    inlines(short, out);
                }
                blocks(&mut caption.long, out);
                blocks(parts, out);
            }
            Block::Table(table) => {
                if let Some(short) = &mut table.caption.short {
                    inlines(short, out);
                }
                blocks(&mut table.caption.long, out);
                rows(&mut table.head.rows, out);
                for body in &mut table.bodies {
                    rows(&mut body.head, out);
                    rows(&mut body.body, out);
                }
                rows(&mut table.foot.rows, out);
            }
            Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => {}
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nested(levels: usize, wrap: fn(Vec<Inline>) -> Inline) -> Vec<Inline> {
        let mut inlines = vec![Inline::Str("x".into())];
        for _ in 0..levels {
            inlines = vec![wrap(inlines)];
        }
        inlines
    }

    #[test]
    fn the_check_counts_inlines_and_note_blocks() {
        let emph = nested(3, Inline::Emph);
        assert_eq!(check_inlines(&emph, 4), Ok(()));
        assert_eq!(check_inlines(&emph, 3), Err(TooDeep));
        drop_inlines(emph);
        // A note and its paragraph are two levels.
        let note = nested(2, |parts| Inline::Note(vec![Block::Para(parts)]));
        assert_eq!(check_inlines(&note, 5), Ok(()));
        assert_eq!(check_inlines(&note, 4), Err(TooDeep));
        drop_inlines(note);
    }
}
