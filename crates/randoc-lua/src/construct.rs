//! The constructors of the `pandoc` module, for example `pandoc.Str`.
//!
//! This is a port of the constructor functions of `pandoc-lua-marshal`
//! 0.3.2.1. The name of each parameter is part of the error text that a
//! script sees.

use mlua::Value;
use randoc_ast::{
    Attr, Block, Caption, Cell, Citation, Inline, ListAttributes, ListNumberDelim, ListNumberStyle,
    Meta, MetaValue, Pandoc, Row, Table, TableBody, TableFoot, TableHead, Target,
};

use crate::object::{Node, new_object};
use crate::peek::{Cx, Peek, PeekError, all_choices, is_none_or_nil, none, retrieving, within};
use crate::push;

/// The arguments of one call of a function of the `pandoc` module.
pub(crate) struct Call<'a> {
    /// The conversion state.
    pub(crate) cx: &'a Cx<'a>,
    /// The function name, for error messages.
    pub(crate) name: &'a str,
    /// The arguments.
    pub(crate) arguments: &'a [Value],
}

impl Call<'_> {
    /// Returns argument `index` (from 0), or the "no value" marker.
    pub(crate) fn argument(&self, index: usize) -> Value {
        self.arguments.get(index).cloned().unwrap_or_else(none)
    }

    fn in_context<T>(&self, parameter: &str, result: Peek<T>) -> Peek<T> {
        let result = within(
            || format!("retrieving function argument {parameter}"),
            result,
        );
        within(
            || format!("retrieving arguments for function {}", self.name),
            result,
        )
    }

    /// Reads a required argument.
    pub(crate) fn required<T>(
        &self,
        index: usize,
        parameter: &str,
        read: impl FnOnce(&Value) -> Peek<T>,
    ) -> Peek<T> {
        self.in_context(parameter, read(&self.argument(index)))
    }

    /// Reads an optional argument: `nil` and a missing argument give `None`.
    pub(crate) fn optional<T>(
        &self,
        index: usize,
        parameter: &str,
        read: impl FnOnce(&Value) -> Peek<T>,
    ) -> Peek<Option<T>> {
        let value = self.argument(index);
        if is_none_or_nil(&value) {
            return Ok(None);
        }
        self.in_context(parameter, read(&value)).map(Some)
    }

    fn inlines(&self, index: usize, parameter: &str) -> Peek<Vec<Inline>> {
        self.required(index, parameter, |v| self.cx.inlines_fuzzy(v))
    }

    fn blocks(&self, index: usize, parameter: &str) -> Peek<Vec<Block>> {
        self.required(index, parameter, |v| self.cx.blocks_fuzzy(v))
    }

    fn text(&self, index: usize, parameter: &str) -> Peek<String> {
        self.required(index, parameter, |v| self.cx.text(v))
    }

    fn attr(&self, index: usize) -> Peek<Attr> {
        Ok(self
            .optional(index, "attr", |v| self.cx.attr(v))?
            .unwrap_or_default())
    }

    fn rows(&self, index: usize, parameter: &str) -> Peek<Vec<Row>> {
        Ok(self
            .optional(index, parameter, |v| self.cx.rows(v))?
            .unwrap_or_default())
    }
}

/// The names of the constructors that [`construct`] implements.
pub(crate) const CONSTRUCTORS: [&str; 54] = [
    "Cite",
    "Code",
    "Emph",
    "Image",
    "LineBreak",
    "Link",
    "Math",
    "Note",
    "Quoted",
    "RawInline",
    "SmallCaps",
    "SoftBreak",
    "Space",
    "Span",
    "Str",
    "Strikeout",
    "Strong",
    "Subscript",
    "Superscript",
    "Underline",
    "Inlines",
    "BlockQuote",
    "BulletList",
    "CodeBlock",
    "DefinitionList",
    "Div",
    "Figure",
    "Header",
    "HorizontalRule",
    "LineBlock",
    "OrderedList",
    "Para",
    "Plain",
    "RawBlock",
    "Table",
    "Blocks",
    "Attr",
    "AttributeList",
    "Caption",
    "Cell",
    "Citation",
    "ListAttributes",
    "Meta",
    "MetaBlocks",
    "MetaBool",
    "MetaInlines",
    "MetaList",
    "MetaMap",
    "MetaString",
    "Pandoc",
    "Row",
    "TableBody",
    "TableFoot",
    "TableHead",
];

fn inline(call: &Call, inline: Inline) -> Peek<Value> {
    Ok(new_object(call.cx.lua, Node::Inline(inline))?)
}

fn block(call: &Call, block: Block) -> Peek<Value> {
    Ok(new_object(call.cx.lua, Node::Block(block))?)
}

fn object(call: &Call, node: Node) -> Peek<Value> {
    Ok(new_object(call.cx.lua, node)?)
}

/// Runs the constructor `call.name`.
pub(crate) fn construct(call: &Call) -> Peek<Value> {
    let cx = call.cx;
    let lua = cx.lua;
    match call.name {
        "Cite" => {
            let content = call.inlines(0, "content")?;
            let citations = call.required(1, "citations", |v| cx.list(v, |c| cx.citation(c)))?;
            inline(call, Inline::Cite(citations, content))
        }
        "Code" => {
            let text = call.text(0, "code")?;
            inline(call, Inline::Code(call.attr(1)?, text))
        }
        "Emph" => inline(call, Inline::Emph(call.inlines(0, "content")?)),
        "Image" => {
            let caption = call.inlines(0, "caption")?;
            let src = call.text(1, "src")?;
            let title = call
                .optional(2, "title", |v| cx.text(v))?
                .unwrap_or_default();
            let attr = call.attr(3)?;
            inline(call, Inline::Image(attr, caption, Target::new(src, title)))
        }
        "LineBreak" => inline(call, Inline::LineBreak),
        "Link" => {
            let content = call.inlines(0, "content")?;
            let target = call.text(1, "target")?;
            let title = call
                .optional(2, "title", |v| cx.text(v))?
                .unwrap_or_default();
            let attr = call.attr(3)?;
            inline(
                call,
                Inline::Link(attr, content, Target::new(target, title)),
            )
        }
        "Math" => {
            let math_type = call.required(0, "mathtype", |v| cx.math_type(v))?;
            inline(call, Inline::Math(math_type, call.text(1, "text")?))
        }
        "Note" => inline(call, Inline::Note(call.blocks(0, "content")?)),
        "Quoted" => {
            let quote_type = call.required(0, "quotetype", |v| cx.quote_type(v))?;
            inline(
                call,
                Inline::Quoted(quote_type, call.inlines(1, "content")?),
            )
        }
        "RawInline" => {
            let format = call.required(0, "format", |v| cx.format(v))?;
            inline(call, Inline::RawInline(format, call.text(1, "text")?))
        }
        "SmallCaps" => inline(call, Inline::SmallCaps(call.inlines(0, "content")?)),
        "SoftBreak" => inline(call, Inline::SoftBreak),
        "Space" => inline(call, Inline::Space),
        "Span" => {
            let content = call.inlines(0, "content")?;
            inline(call, Inline::Span(call.attr(1)?, content))
        }
        "Str" => inline(call, Inline::Str(call.text(0, "text")?)),
        "Strikeout" => inline(call, Inline::Strikeout(call.inlines(0, "content")?)),
        "Strong" => inline(call, Inline::Strong(call.inlines(0, "content")?)),
        "Subscript" => inline(call, Inline::Subscript(call.inlines(0, "content")?)),
        "Superscript" => inline(call, Inline::Superscript(call.inlines(0, "content")?)),
        "Underline" => inline(call, Inline::Underline(call.inlines(0, "content")?)),
        "Inlines" => Ok(push::inlines(
            lua,
            &call.inlines(0, "inline_like_elements")?,
        )?),
        "BlockQuote" => block(call, Block::BlockQuote(call.blocks(0, "content")?)),
        "BulletList" => {
            let items = call.required(0, "items", |v| cx.items_fuzzy(v))?;
            block(call, Block::BulletList(items))
        }
        "CodeBlock" => {
            let text = call.text(0, "text")?;
            block(call, Block::CodeBlock(call.attr(1)?, text))
        }
        "DefinitionList" => {
            let items = call.required(0, "content", |v| {
                all_choices(match cx.list(v, |item| cx.definition_item(item)) {
                    Err(PeekError::Failure { .. }) => cx.definition_item(v).map(|item| vec![item]),
                    other => other,
                })
            })?;
            block(call, Block::DefinitionList(items))
        }
        "Div" => {
            let content = call.blocks(0, "content")?;
            block(call, Block::Div(call.attr(1)?, content))
        }
        "Figure" => {
            let content = call.blocks(0, "content")?;
            let caption = call
                .optional(1, "caption", |v| cx.caption_fuzzy(v))?
                .unwrap_or_default();
            block(call, Block::Figure(call.attr(2)?, caption, content))
        }
        "Header" => {
            let level = call.required(0, "level", |v| cx.integral(v))?;
            let content = call.inlines(1, "content")?;
            block(call, Block::Header(level, call.attr(2)?, content))
        }
        "HorizontalRule" => block(call, Block::HorizontalRule),
        "LineBlock" => {
            let lines = call.required(0, "content", |v| cx.list(v, |l| cx.inlines_fuzzy(l)))?;
            block(call, Block::LineBlock(lines))
        }
        "OrderedList" => {
            let items = call.required(0, "items", |v| cx.items_fuzzy(v))?;
            let attributes = call
                .optional(1, "listAttributes", |v| cx.list_attributes(v))?
                .unwrap_or_default();
            block(call, Block::OrderedList(attributes, items))
        }
        "Para" => block(call, Block::Para(call.inlines(0, "content")?)),
        "Plain" => block(call, Block::Plain(call.inlines(0, "content")?)),
        "RawBlock" => {
            let format = call.required(0, "format", |v| cx.format(v))?;
            block(call, Block::RawBlock(format, call.text(1, "text")?))
        }
        "Table" => {
            let caption = call.required(0, "caption", |v| cx.caption_fuzzy(v))?;
            let col_specs = call.required(1, "colspecs", |v| cx.list(v, |c| cx.col_spec(c)))?;
            let head = call.required(2, "head", |v| cx.table_head(v))?;
            let bodies = call.required(3, "bodies", |v| {
                cx.list(v, |body| cx.table_body_fuzzy(body))
            })?;
            let foot = call.required(4, "foot", |v| cx.table_foot(v))?;
            let attr = call.attr(5)?;
            block(
                call,
                Block::Table(Box::new(Table {
                    attr,
                    caption,
                    col_specs,
                    head,
                    bodies,
                    foot,
                })),
            )
        }
        "Blocks" => Ok(push::blocks(lua, &call.blocks(0, "block_like_elements")?)?),
        "Attr" => {
            let attr =
                cx.attr_arguments(&call.argument(0), &call.argument(1), &call.argument(2))?;
            object(call, Node::Attr(attr))
        }
        "AttributeList" => {
            let pairs = call.required(0, "attribs", |v| cx.attribute_list(v))?;
            object(call, Node::AttributeList(pairs))
        }
        "Caption" => {
            let long = call.optional(0, "long", |v| cx.blocks_fuzzy(v))?;
            let short = call.optional(1, "short", |v| cx.inlines_fuzzy(v))?;
            object(
                call,
                Node::Caption(Caption {
                    short,
                    long: long.unwrap_or_default(),
                }),
            )
        }
        "Cell" => {
            let content = call.blocks(0, "blocks")?;
            let align = call.optional(1, "align", |v| cx.alignment(v))?;
            let row_span = call.optional(2, "rowspan", |v| cx.integral(v))?;
            let col_span = call.optional(3, "colspan", |v| cx.integral(v))?;
            let attr = call.attr(4)?;
            let default = Cell::default();
            object(
                call,
                Node::Cell(Cell {
                    attr,
                    align: align.unwrap_or(default.align),
                    row_span: row_span.unwrap_or(default.row_span),
                    col_span: col_span.unwrap_or(default.col_span),
                    content,
                }),
            )
        }
        "Citation" => {
            let id = call.text(0, "id")?;
            let mode = call.required(1, "mode", |v| cx.citation_mode(v))?;
            let prefix = call.optional(2, "prefix", |v| cx.inlines_fuzzy(v))?;
            let suffix = call.optional(3, "suffix", |v| cx.inlines_fuzzy(v))?;
            let note_num = call.optional(4, "note_num", |v| cx.integral(v))?;
            let hash = call.optional(5, "hash", |v| cx.integral(v))?;
            object(
                call,
                Node::Citation(Citation {
                    id,
                    prefix: prefix.unwrap_or_default(),
                    suffix: suffix.unwrap_or_default(),
                    mode,
                    note_num: note_num.unwrap_or(0),
                    hash: hash.unwrap_or(0),
                }),
            )
        }
        "ListAttributes" => {
            let start = call.optional(0, "start", |v| cx.integral(v))?;
            let style = call.optional(1, "style", |v| cx.number_style(v))?;
            let delim = call.optional(2, "delimiter", |v| cx.number_delim(v))?;
            object(
                call,
                Node::ListAttributes(ListAttributes {
                    start: start.unwrap_or(1),
                    style: style.unwrap_or(ListNumberStyle::DefaultStyle),
                    delim: delim.unwrap_or(ListNumberDelim::DefaultDelim),
                }),
            )
        }
        "Meta" => {
            let meta = call.required(0, "meta", |v| cx.meta(v))?;
            Ok(push::meta(lua, &meta)?)
        }
        "MetaBlocks" => {
            let content = call.blocks(0, "content")?;
            Ok(push::meta_value(lua, &MetaValue::MetaBlocks(content))?)
        }
        "MetaBool" => {
            let argument = call.argument(0);
            let falsy = is_none_or_nil(&argument) || argument == Value::Boolean(false);
            Ok(Value::Boolean(!falsy))
        }
        "MetaInlines" => {
            let content = call.inlines(0, "inlines")?;
            Ok(push::meta_value(lua, &MetaValue::MetaInlines(content))?)
        }
        "MetaList" => {
            let items =
                call.required(0, "values", |v| cx.list(v, |item| cx.meta_value(item, 1)))?;
            Ok(push::meta_value(lua, &MetaValue::MetaList(items))?)
        }
        "MetaMap" => {
            let map = call.required(0, "key_value_map", |v| {
                cx.meta(v).map_err(|error| match error {
                    // `cx.meta` adds the context of the type `Meta`, which
                    // the constructor `MetaMap` does not have.
                    PeekError::Failure {
                        message,
                        mut context,
                    } => {
                        context.pop();
                        PeekError::Failure { message, context }
                    }
                    thrown => thrown,
                })
            })?;
            Ok(push::meta_value(lua, &MetaValue::MetaMap(map.0))?)
        }
        "MetaString" => Ok(push::text(lua, &call.text(0, "s")?)?),
        "Pandoc" => {
            let blocks = call.blocks(0, "blocks")?;
            let meta = call.optional(1, "meta", |v| cx.meta(v))?;
            object(
                call,
                Node::Pandoc(Pandoc {
                    meta: meta.unwrap_or_else(Meta::default),
                    blocks,
                }),
            )
        }
        "Row" => {
            let cells = call
                .optional(0, "cells", |v| cx.list(v, |cell| cx.cell_fuzzy(cell)))?
                .unwrap_or_default();
            object(
                call,
                Node::Row(Row {
                    attr: call.attr(1)?,
                    cells,
                }),
            )
        }
        "TableBody" => {
            let body = call.rows(0, "body")?;
            let head = call.rows(1, "head")?;
            let row_head_columns = call
                .optional(2, "row_head_columns", |v| cx.integral(v))?
                .unwrap_or(0);
            object(
                call,
                Node::TableBody(TableBody {
                    attr: call.attr(3)?,
                    row_head_columns,
                    head,
                    body,
                }),
            )
        }
        "TableFoot" => {
            let rows = call.rows(0, "rows")?;
            object(
                call,
                Node::TableFoot(TableFoot {
                    attr: call.attr(1)?,
                    rows,
                }),
            )
        }
        "TableHead" => {
            let rows = call.rows(0, "rows")?;
            object(
                call,
                Node::TableHead(TableHead {
                    attr: call.attr(1)?,
                    rows,
                }),
            )
        }
        other => Err(PeekError::Thrown(format!(
            "pandoc.{other} is not a constructor of Randoc"
        ))),
    }
}

/// Reads the version specifier of `pandoc.types.Version`: a string of
/// numbers with periods between them, one number, or a list of numbers.
pub(crate) fn version(cx: &Cx, value: &Value) -> Peek<Vec<i64>> {
    let parse = |text: &str| -> Option<Vec<i64>> {
        if text.is_empty() {
            return None;
        }
        text.split('.')
            .map(|part| {
                let valid = !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
                valid.then(|| part.parse::<i64>().ok()).flatten()
            })
            .collect()
    };
    let read = || match value {
        Value::String(text) => {
            let text = text.to_string_lossy();
            parse(&text)
                .ok_or_else(|| PeekError::failure(format!("could not parse as Version: {text}")))
        }
        Value::Integer(number) => Ok(vec![*number]),
        Value::Number(_) => cx.integral(value).map(|number| vec![number]),
        Value::Table(_) => cx.list(value, |item| cx.integral(item)),
        Value::UserData(_) => match cx.object("Version", value)? {
            Node::Version(parts) => Ok(parts),
            _ => Err(crate::peek::mismatch("Version", value)),
        },
        _ => Err(crate::peek::mismatch("Version", value)),
    };
    retrieving("Version", read())
}
