use randoc_ast::{Block, Caption, Inline, MetaValue, Pandoc, Row, Table};
use randoc_core::{Error, Result};

/// Bounds the document before rendering or extracting plain text.
pub(super) fn validate_depth(doc: &Pandoc, limit: usize) -> Result<()> {
    let root = doc
        .meta
        .0
        .values()
        .map(Node::Meta)
        .chain(blocks(&doc.blocks));
    let mut stack = vec![iter(root)];
    loop {
        let depth = stack.len() - 1;
        let Some(frame) = stack.last_mut() else {
            return Ok(());
        };
        if let Some(node) = frame.next() {
            if depth >= limit {
                return Err(Error::Parse("DOCX content is nested too deeply.".into()));
            }
            if let Some(children) = node.children() {
                stack.push(children);
            }
        } else {
            stack.pop();
            if stack.is_empty() {
                return Ok(());
            }
        }
    }
}

type Children<'a> = Box<dyn Iterator<Item = Node<'a>> + 'a>;

enum Node<'a> {
    Block(&'a Block),
    Inline(&'a Inline),
    Meta(&'a MetaValue),
}

impl<'a> Node<'a> {
    fn children(self) -> Option<Children<'a>> {
        use Block as B;
        use Inline as I;
        use MetaValue as M;
        Some(match self {
            Self::Block(B::Plain(content) | B::Para(content) | B::Header(_, _, content)) => {
                iter(inlines(content))
            }
            Self::Block(B::BlockQuote(content) | B::Div(_, content)) => iter(blocks(content)),
            Self::Block(B::LineBlock(lines)) => iter(lines.iter().flat_map(|line| inlines(line))),
            Self::Block(B::BulletList(items) | B::OrderedList(_, items)) => {
                iter(items.iter().flat_map(|item| blocks(item)))
            }
            Self::Block(B::DefinitionList(items)) => {
                iter(items.iter().flat_map(|(term, definitions)| {
                    inlines(term)
                        .chain(definitions.iter().flat_map(|definition| blocks(definition)))
                }))
            }
            Self::Block(B::Table(table)) => iter(caption(&table.caption).chain(
                rows(table).flat_map(|row| row.cells.iter().flat_map(|cell| blocks(&cell.content))),
            )),
            Self::Block(B::Figure(_, label, content)) => {
                iter(caption(label).chain(blocks(content)))
            }
            Self::Inline(
                I::Emph(content)
                | I::Strong(content)
                | I::Underline(content)
                | I::Strikeout(content)
                | I::SmallCaps(content)
                | I::Superscript(content)
                | I::Subscript(content)
                | I::Quoted(_, content)
                | I::Span(_, content)
                | I::Link(_, content, _)
                | I::Image(_, content, _),
            ) => iter(inlines(content)),
            Self::Inline(I::Cite(citations, content)) => iter(
                citations
                    .iter()
                    .flat_map(|cite| inlines(&cite.prefix).chain(inlines(&cite.suffix)))
                    .chain(inlines(content)),
            ),
            Self::Inline(I::Note(content)) => iter(blocks(content)),
            Self::Meta(M::MetaInlines(content)) => iter(inlines(content)),
            Self::Meta(M::MetaBlocks(content)) => iter(blocks(content)),
            Self::Meta(M::MetaList(items)) => iter(items.iter().map(Self::Meta)),
            Self::Meta(M::MetaMap(items)) => iter(items.values().map(Self::Meta)),
            _ => return None,
        })
    }
}

fn iter<'a>(nodes: impl Iterator<Item = Node<'a>> + 'a) -> Children<'a> {
    Box::new(nodes)
}

fn inlines(inlines: &[Inline]) -> impl Iterator<Item = Node<'_>> {
    inlines.iter().map(Node::Inline)
}

fn blocks(blocks: &[Block]) -> impl Iterator<Item = Node<'_>> {
    blocks.iter().map(Node::Block)
}

fn caption(value: &Caption) -> impl Iterator<Item = Node<'_>> {
    inlines(value.short.as_deref().unwrap_or_default()).chain(blocks(&value.long))
}

pub(super) fn rows(table: &Table) -> impl Iterator<Item = &Row> {
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
