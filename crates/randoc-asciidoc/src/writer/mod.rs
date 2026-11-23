mod blocks;
mod inlines;
mod tables;

use randoc_ast::{Block, Inline, MetaValue, Pandoc};
use randoc_core::{Context, LogMessage, Result, WrapOption, WriterOptions};
use randoc_layout::{Doc, literal};
use randoc_templates::{Context as TemplateContext, Val};

use crate::limit;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Dialect {
    Modern,
    Asciidoctor,
    Legacy,
}

pub(super) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
    dialect: Dialect,
) -> Result<String> {
    let math = inspect(doc, context.limits.max_nesting_depth)?;
    if dialect == Dialect::Asciidoctor {
        context.report(LogMessage::Deprecated(
            "asciidoctor".into(),
            "use asciidoc instead".into(),
        ));
    }
    let mut renderer = Renderer {
        options,
        dialect,
        messages: Vec::new(),
        ids: Default::default(),
    };
    let body = renderer.blocks(&doc.blocks, 0, 0);
    let body = renderer.render(body);
    let output = if let Some(template) = &options.template {
        let meta_renderer = std::cell::RefCell::new(Renderer {
            options,
            dialect,
            messages: Vec::new(),
            ids: Default::default(),
        });
        let mut variables = TemplateContext::from_meta(
            &doc.meta,
            &mut |blocks| meta_renderer.borrow_mut().blocks(blocks, 0, 0),
            &mut |inlines| meta_renderer.borrow().inlines(inlines, false, false),
        );
        renderer
            .messages
            .extend(meta_renderer.into_inner().messages);
        variables.0.extend(options.variables.0.clone());
        if ["title", "author", "date"]
            .iter()
            .any(|key| variables.get(key).is_some())
        {
            variables.insert("titleblock", Val::Bool(true));
        }
        if options.table_of_contents {
            variables.insert("toc", Val::Bool(true));
        }
        if math && dialect != Dialect::Legacy {
            variables.insert("math", Val::Bool(true));
        }
        variables.insert("body", literal(body));
        let max = usize::try_from(context.limits.max_input_size).unwrap_or(usize::MAX);
        template
            .render_limited(&variables, max, max)
            .ok_or_else(|| limit("output size"))
    } else {
        Ok(body)
    };
    for message in renderer.messages {
        context.report(message);
    }
    output
}

struct Renderer<'a> {
    options: &'a WriterOptions,
    dialect: Dialect,
    messages: Vec<LogMessage>,
    ids: randoc_core::shared::IdentifierSet,
}

impl Renderer<'_> {
    fn width(&self) -> Option<usize> {
        (self.options.wrap_text == WrapOption::WrapAuto && self.options.columns > 0)
            .then_some(self.options.columns as usize)
    }

    fn render(&self, doc: Doc) -> String {
        doc.render(self.width())
    }
}

enum Node<'a> {
    Blocks(&'a [Block]),
    Inlines(&'a [Inline]),
    Meta(&'a MetaValue),
}

fn inspect(doc: &Pandoc, limit_depth: usize) -> Result<bool> {
    let mut stack = vec![(0, Node::Blocks(&doc.blocks))];
    stack.extend(doc.meta.0.values().map(|value| (0, Node::Meta(value))));
    let mut math = false;
    while let Some((depth, node)) = stack.pop() {
        if depth > limit_depth {
            return Err(limit("nesting depth"));
        }
        let next = depth + 1;
        match node {
            Node::Meta(meta) => match meta {
                MetaValue::MetaBlocks(blocks) => stack.push((next, Node::Blocks(blocks))),
                MetaValue::MetaInlines(inlines) => stack.push((next, Node::Inlines(inlines))),
                MetaValue::MetaMap(map) => {
                    stack.extend(map.values().map(|value| (next, Node::Meta(value))))
                }
                MetaValue::MetaList(list) => {
                    stack.extend(list.iter().map(|value| (next, Node::Meta(value))))
                }
                _ => {}
            },
            Node::Blocks(blocks) => {
                for block in blocks {
                    match block {
                        Block::Plain(inlines)
                        | Block::Para(inlines)
                        | Block::Header(_, _, inlines) => {
                            stack.push((next, Node::Inlines(inlines)))
                        }
                        Block::LineBlock(lines) => {
                            stack.extend(lines.iter().map(|line| (next, Node::Inlines(line))))
                        }
                        Block::BlockQuote(blocks) | Block::Div(_, blocks) => {
                            stack.push((next, Node::Blocks(blocks)))
                        }
                        Block::Figure(_, caption, blocks) => {
                            stack.push((next, Node::Blocks(&caption.long)));
                            stack.push((next, Node::Blocks(blocks)));
                        }
                        Block::BulletList(items) | Block::OrderedList(_, items) => {
                            stack.extend(items.iter().map(|item| (next, Node::Blocks(item))))
                        }
                        Block::DefinitionList(items) => {
                            for (term, definitions) in items {
                                stack.push((next, Node::Inlines(term)));
                                stack.extend(
                                    definitions.iter().map(|item| (next, Node::Blocks(item))),
                                );
                            }
                        }
                        Block::Table(table) => {
                            stack.push((next, Node::Blocks(&table.caption.long)));
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
                            stack.extend(rows.flat_map(|row| {
                                row.cells
                                    .iter()
                                    .map(|cell| (next, Node::Blocks(&cell.content)))
                            }));
                        }
                        _ => {}
                    }
                }
            }
            Node::Inlines(inlines) => {
                for inline in inlines {
                    match inline {
                        Inline::Emph(xs)
                        | Inline::Underline(xs)
                        | Inline::Strong(xs)
                        | Inline::Strikeout(xs)
                        | Inline::Superscript(xs)
                        | Inline::Subscript(xs)
                        | Inline::SmallCaps(xs)
                        | Inline::Quoted(_, xs)
                        | Inline::Cite(_, xs)
                        | Inline::Link(_, xs, _)
                        | Inline::Image(_, xs, _)
                        | Inline::Span(_, xs) => stack.push((next, Node::Inlines(xs))),
                        Inline::Note(blocks) => stack.push((next, Node::Blocks(blocks))),
                        Inline::Math(..) => math = true,
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(math)
}
