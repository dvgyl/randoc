//! Traversal of the document tree.
//!
//! [`walk_mut`] visits each element after its children, the same order as the
//! Haskell function `walk`. Use it for transformations. [`query`] visits each
//! element before its children, in document order. Use it to collect data.

use crate::{Block, Caption, Citation, Inline, MetaValue, Pandoc, Row, Table};

/// A transformation of the document tree.
///
/// Each method has an empty default. Implement the methods that you need.
pub trait VisitorMut {
    /// Called for each inline, after its children.
    fn inline(&mut self, _inline: &mut Inline) {}
    /// Called for each list of inlines, after its items.
    fn inlines(&mut self, _inlines: &mut Vec<Inline>) {}
    /// Called for each block, after its children.
    fn block(&mut self, _block: &mut Block) {}
    /// Called for each list of blocks, after its items.
    fn blocks(&mut self, _blocks: &mut Vec<Block>) {}
}

/// A read-only pass over the document tree.
pub trait Visitor {
    /// Called for each inline, before its children.
    fn inline(&mut self, _inline: &Inline) {}
    /// Called for each block, before its children.
    fn block(&mut self, _block: &Block) {}
}

/// A part of the tree that the walk functions can traverse.
pub trait Walkable {
    /// Applies `visitor` to each element, children first.
    fn walk_mut(&mut self, visitor: &mut dyn VisitorMut);
    /// Shows each element to `visitor`, parents first, in document order.
    fn query(&self, visitor: &mut dyn Visitor);
}

/// Applies `visitor` to each element of `node`, children first.
pub fn walk_mut<T: Walkable + ?Sized>(node: &mut T, visitor: &mut dyn VisitorMut) {
    node.walk_mut(visitor);
}

/// Shows each element of `node` to `visitor`, parents first, in document order.
pub fn query<T: Walkable + ?Sized>(node: &T, visitor: &mut dyn Visitor) {
    node.query(visitor);
}

/// Calls `f` for each inline in `node`, children first.
pub fn walk_inlines_mut<T: Walkable + ?Sized>(node: &mut T, f: impl FnMut(&mut Inline)) {
    struct Each<F>(F);
    impl<F: FnMut(&mut Inline)> VisitorMut for Each<F> {
        fn inline(&mut self, inline: &mut Inline) {
            (self.0)(inline);
        }
    }
    node.walk_mut(&mut Each(f));
}

/// Calls `f` for each block in `node`, children first.
pub fn walk_blocks_mut<T: Walkable + ?Sized>(node: &mut T, f: impl FnMut(&mut Block)) {
    struct Each<F>(F);
    impl<F: FnMut(&mut Block)> VisitorMut for Each<F> {
        fn block(&mut self, block: &mut Block) {
            (self.0)(block);
        }
    }
    node.walk_mut(&mut Each(f));
}

/// Calls `f` for each inline in `node`, in document order.
pub fn query_inlines<T: Walkable + ?Sized>(node: &T, f: impl FnMut(&Inline)) {
    struct Each<F>(F);
    impl<F: FnMut(&Inline)> Visitor for Each<F> {
        fn inline(&mut self, inline: &Inline) {
            (self.0)(inline);
        }
    }
    node.query(&mut Each(f));
}

/// Calls `f` for each block in `node`, in document order.
pub fn query_blocks<T: Walkable + ?Sized>(node: &T, f: impl FnMut(&Block)) {
    struct Each<F>(F);
    impl<F: FnMut(&Block)> Visitor for Each<F> {
        fn block(&mut self, block: &Block) {
            (self.0)(block);
        }
    }
    node.query(&mut Each(f));
}

impl Walkable for Pandoc {
    fn walk_mut(&mut self, visitor: &mut dyn VisitorMut) {
        for value in self.meta.0.values_mut() {
            value.walk_mut(visitor);
        }
        self.blocks.walk_mut(visitor);
    }

    fn query(&self, visitor: &mut dyn Visitor) {
        for value in self.meta.0.values() {
            value.query(visitor);
        }
        self.blocks.query(visitor);
    }
}

impl Walkable for MetaValue {
    fn walk_mut(&mut self, visitor: &mut dyn VisitorMut) {
        match self {
            MetaValue::MetaMap(map) => map.values_mut().for_each(|v| v.walk_mut(visitor)),
            MetaValue::MetaList(list) => list.iter_mut().for_each(|v| v.walk_mut(visitor)),
            MetaValue::MetaInlines(inlines) => inlines.walk_mut(visitor),
            MetaValue::MetaBlocks(blocks) => blocks.walk_mut(visitor),
            MetaValue::MetaBool(_) | MetaValue::MetaString(_) => {}
        }
    }

    fn query(&self, visitor: &mut dyn Visitor) {
        match self {
            MetaValue::MetaMap(map) => map.values().for_each(|v| v.query(visitor)),
            MetaValue::MetaList(list) => list.iter().for_each(|v| v.query(visitor)),
            MetaValue::MetaInlines(inlines) => inlines.query(visitor),
            MetaValue::MetaBlocks(blocks) => blocks.query(visitor),
            MetaValue::MetaBool(_) | MetaValue::MetaString(_) => {}
        }
    }
}

impl Walkable for Vec<Block> {
    fn walk_mut(&mut self, visitor: &mut dyn VisitorMut) {
        for block in self.iter_mut() {
            block.walk_mut(visitor);
        }
        visitor.blocks(self);
    }

    fn query(&self, visitor: &mut dyn Visitor) {
        for block in self {
            block.query(visitor);
        }
    }
}

impl Walkable for Vec<Inline> {
    fn walk_mut(&mut self, visitor: &mut dyn VisitorMut) {
        for inline in self.iter_mut() {
            inline.walk_mut(visitor);
        }
        visitor.inlines(self);
    }

    fn query(&self, visitor: &mut dyn Visitor) {
        for inline in self {
            inline.query(visitor);
        }
    }
}

impl Walkable for Caption {
    fn walk_mut(&mut self, visitor: &mut dyn VisitorMut) {
        if let Some(short) = &mut self.short {
            short.walk_mut(visitor);
        }
        self.long.walk_mut(visitor);
    }

    fn query(&self, visitor: &mut dyn Visitor) {
        if let Some(short) = &self.short {
            short.query(visitor);
        }
        self.long.query(visitor);
    }
}

fn walk_rows_mut(rows: &mut [Row], visitor: &mut dyn VisitorMut) {
    for row in rows {
        for cell in &mut row.cells {
            cell.content.walk_mut(visitor);
        }
    }
}

fn query_rows(rows: &[Row], visitor: &mut dyn Visitor) {
    for row in rows {
        for cell in &row.cells {
            cell.content.query(visitor);
        }
    }
}

impl Walkable for Table {
    fn walk_mut(&mut self, visitor: &mut dyn VisitorMut) {
        self.caption.walk_mut(visitor);
        walk_rows_mut(&mut self.head.rows, visitor);
        for body in &mut self.bodies {
            walk_rows_mut(&mut body.head, visitor);
            walk_rows_mut(&mut body.body, visitor);
        }
        walk_rows_mut(&mut self.foot.rows, visitor);
    }

    fn query(&self, visitor: &mut dyn Visitor) {
        self.caption.query(visitor);
        query_rows(&self.head.rows, visitor);
        for body in &self.bodies {
            query_rows(&body.head, visitor);
            query_rows(&body.body, visitor);
        }
        query_rows(&self.foot.rows, visitor);
    }
}

impl Walkable for Block {
    fn walk_mut(&mut self, visitor: &mut dyn VisitorMut) {
        match self {
            Block::Plain(inlines) | Block::Para(inlines) | Block::Header(_, _, inlines) => {
                inlines.walk_mut(visitor);
            }
            Block::LineBlock(lines) => lines.iter_mut().for_each(|line| line.walk_mut(visitor)),
            Block::BlockQuote(blocks) | Block::Div(_, blocks) => blocks.walk_mut(visitor),
            Block::OrderedList(_, items) | Block::BulletList(items) => {
                items.iter_mut().for_each(|item| item.walk_mut(visitor));
            }
            Block::DefinitionList(items) => {
                for (term, definitions) in items {
                    term.walk_mut(visitor);
                    definitions.iter_mut().for_each(|d| d.walk_mut(visitor));
                }
            }
            Block::Table(table) => table.walk_mut(visitor),
            Block::Figure(_, caption, blocks) => {
                caption.walk_mut(visitor);
                blocks.walk_mut(visitor);
            }
            Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => {}
        }
        visitor.block(self);
    }

    fn query(&self, visitor: &mut dyn Visitor) {
        visitor.block(self);
        match self {
            Block::Plain(inlines) | Block::Para(inlines) | Block::Header(_, _, inlines) => {
                inlines.query(visitor);
            }
            Block::LineBlock(lines) => lines.iter().for_each(|line| line.query(visitor)),
            Block::BlockQuote(blocks) | Block::Div(_, blocks) => blocks.query(visitor),
            Block::OrderedList(_, items) | Block::BulletList(items) => {
                items.iter().for_each(|item| item.query(visitor));
            }
            Block::DefinitionList(items) => {
                for (term, definitions) in items {
                    term.query(visitor);
                    definitions.iter().for_each(|d| d.query(visitor));
                }
            }
            Block::Table(table) => table.query(visitor),
            Block::Figure(_, caption, blocks) => {
                caption.query(visitor);
                blocks.query(visitor);
            }
            Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => {}
        }
    }
}

fn walk_citations_mut(citations: &mut [Citation], visitor: &mut dyn VisitorMut) {
    for citation in citations {
        citation.prefix.walk_mut(visitor);
        citation.suffix.walk_mut(visitor);
    }
}

fn query_citations(citations: &[Citation], visitor: &mut dyn Visitor) {
    for citation in citations {
        citation.prefix.query(visitor);
        citation.suffix.query(visitor);
    }
}

impl Walkable for Inline {
    fn walk_mut(&mut self, visitor: &mut dyn VisitorMut) {
        match self {
            Inline::Emph(inlines)
            | Inline::Underline(inlines)
            | Inline::Strong(inlines)
            | Inline::Strikeout(inlines)
            | Inline::Superscript(inlines)
            | Inline::Subscript(inlines)
            | Inline::SmallCaps(inlines)
            | Inline::Quoted(_, inlines)
            | Inline::Link(_, inlines, _)
            | Inline::Image(_, inlines, _)
            | Inline::Span(_, inlines) => inlines.walk_mut(visitor),
            Inline::Cite(citations, inlines) => {
                walk_citations_mut(citations, visitor);
                inlines.walk_mut(visitor);
            }
            Inline::Note(blocks) => blocks.walk_mut(visitor),
            Inline::Str(_)
            | Inline::Code(..)
            | Inline::Space
            | Inline::SoftBreak
            | Inline::LineBreak
            | Inline::Math(..)
            | Inline::RawInline(..) => {}
        }
        visitor.inline(self);
    }

    fn query(&self, visitor: &mut dyn Visitor) {
        visitor.inline(self);
        match self {
            Inline::Emph(inlines)
            | Inline::Underline(inlines)
            | Inline::Strong(inlines)
            | Inline::Strikeout(inlines)
            | Inline::Superscript(inlines)
            | Inline::Subscript(inlines)
            | Inline::SmallCaps(inlines)
            | Inline::Quoted(_, inlines)
            | Inline::Link(_, inlines, _)
            | Inline::Image(_, inlines, _)
            | Inline::Span(_, inlines) => inlines.query(visitor),
            Inline::Cite(citations, inlines) => {
                query_citations(citations, visitor);
                inlines.query(visitor);
            }
            Inline::Note(blocks) => blocks.query(visitor),
            Inline::Str(_)
            | Inline::Code(..)
            | Inline::Space
            | Inline::SoftBreak
            | Inline::LineBreak
            | Inline::Math(..)
            | Inline::RawInline(..) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Attr, Cell, TableBody, TableHead, Target};

    fn sample() -> Pandoc {
        let cell = |text: &str| Cell {
            content: vec![Block::Plain(vec![Inline::Str(text.into())])],
            ..Cell::default()
        };
        let table = Table {
            caption: Caption {
                short: Some(vec![Inline::Str("short".into())]),
                long: vec![Block::Plain(vec![Inline::Str("long".into())])],
            },
            head: TableHead {
                attr: Attr::default(),
                rows: vec![Row {
                    attr: Attr::default(),
                    cells: vec![cell("head")],
                }],
            },
            bodies: vec![TableBody {
                head: vec![Row {
                    attr: Attr::default(),
                    cells: vec![cell("bodyhead")],
                }],
                body: vec![Row {
                    attr: Attr::default(),
                    cells: vec![cell("body")],
                }],
                ..TableBody::default()
            }],
            foot: crate::TableFoot {
                attr: Attr::default(),
                rows: vec![Row {
                    attr: Attr::default(),
                    cells: vec![cell("foot")],
                }],
            },
            ..Table::default()
        };
        let mut doc = Pandoc::default();
        doc.meta.insert(
            "title",
            MetaValue::MetaList(vec![MetaValue::MetaInlines(vec![Inline::Str(
                "meta".into(),
            )])]),
        );
        doc.blocks = vec![
            Block::Para(vec![
                Inline::Emph(vec![Inline::Str("a".into())]),
                Inline::Link(
                    Attr::default(),
                    vec![Inline::Str("b".into())],
                    Target::new("u", ""),
                ),
                Inline::Note(vec![Block::Para(vec![Inline::Str("c".into())])]),
                Inline::Cite(
                    vec![Citation {
                        id: "k".into(),
                        prefix: vec![Inline::Str("pre".into())],
                        suffix: vec![Inline::Str("suf".into())],
                        mode: crate::CitationMode::NormalCitation,
                        note_num: 0,
                        hash: 0,
                    }],
                    vec![Inline::Str("cite".into())],
                ),
            ]),
            Block::DefinitionList(vec![(
                vec![Inline::Str("term".into())],
                vec![vec![Block::Plain(vec![Inline::Str("def".into())])]],
            )]),
            Block::LineBlock(vec![vec![Inline::Str("line".into())]]),
            Block::BulletList(vec![vec![Block::Plain(vec![Inline::Str("item".into())])]]),
            Block::Table(Box::new(table)),
            Block::Figure(
                Attr::default(),
                Caption {
                    short: None,
                    long: vec![Block::Plain(vec![Inline::Str("figcap".into())])],
                },
                vec![Block::Plain(vec![Inline::Str("fig".into())])],
            ),
            Block::Div(
                Attr::default(),
                vec![Block::BlockQuote(vec![Block::HorizontalRule])],
            ),
        ];
        doc
    }

    fn all_strings(doc: &Pandoc) -> Vec<String> {
        let mut found = Vec::new();
        query_inlines(doc, |inline| {
            if let Inline::Str(text) = inline {
                found.push(text.clone());
            }
        });
        found
    }

    #[test]
    fn query_visits_every_string_in_document_order() {
        assert_eq!(
            all_strings(&sample()),
            [
                "meta", "a", "b", "c", "pre", "suf", "cite", "term", "def", "line", "item",
                "short", "long", "head", "bodyhead", "body", "foot", "figcap", "fig"
            ]
        );
    }

    #[test]
    fn walk_mut_changes_every_string() {
        let mut doc = sample();
        let expected: Vec<String> = all_strings(&doc).iter().map(|s| s.to_uppercase()).collect();
        walk_inlines_mut(&mut doc, |inline| {
            if let Inline::Str(text) = inline {
                *text = text.to_uppercase();
            }
        });
        assert_eq!(all_strings(&doc), expected);
    }

    #[test]
    fn walk_mut_visits_children_before_parents() {
        let mut doc = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Para(vec![Inline::Emph(vec![Inline::Str(
                "x".into(),
            )])])],
        };
        let mut order = Vec::new();
        walk_inlines_mut(&mut doc, |inline| {
            order.push(match inline {
                Inline::Str(_) => "Str",
                Inline::Emph(_) => "Emph",
                _ => "other",
            });
        });
        assert_eq!(order, ["Str", "Emph"]);
    }

    #[test]
    fn query_visits_parents_before_children() {
        let doc = sample();
        let mut names = Vec::new();
        query_blocks(&doc, |block| {
            names.push(match block {
                Block::Div(..) => "Div",
                Block::BlockQuote(_) => "BlockQuote",
                Block::HorizontalRule => "HorizontalRule",
                _ => "",
            });
        });
        names.retain(|name| !name.is_empty());
        assert_eq!(names, ["Div", "BlockQuote", "HorizontalRule"]);
    }

    #[test]
    fn list_visitors_can_remove_items() {
        struct DropSpaces;
        impl VisitorMut for DropSpaces {
            fn inlines(&mut self, inlines: &mut Vec<Inline>) {
                inlines.retain(|inline| *inline != Inline::Space);
            }
            fn blocks(&mut self, blocks: &mut Vec<Block>) {
                blocks.retain(|block| *block != Block::HorizontalRule);
            }
        }
        let mut doc = Pandoc {
            meta: Default::default(),
            blocks: vec![
                Block::HorizontalRule,
                Block::Para(vec![
                    Inline::Str("a".into()),
                    Inline::Space,
                    Inline::Str("b".into()),
                ]),
                Block::BlockQuote(vec![Block::HorizontalRule]),
            ],
        };
        walk_mut(&mut doc, &mut DropSpaces);
        assert_eq!(
            doc.blocks,
            vec![
                Block::Para(vec![Inline::Str("a".into()), Inline::Str("b".into())]),
                Block::BlockQuote(vec![]),
            ]
        );
    }

    #[test]
    fn walk_blocks_mut_reaches_nested_blocks() {
        let mut doc = sample();
        let mut count = 0;
        walk_blocks_mut(&mut doc, |_| count += 1);
        let mut queried = 0;
        query_blocks(&doc, |_| queried += 1);
        assert_eq!(count, queried);
        assert!(count > 15);
    }
}
