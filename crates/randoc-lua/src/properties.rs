//! The properties of each object type: the getter that converts one field
//! to Lua, and the setter that puts a Lua value into the field.
//!
//! This is a port of the property tables of `pandoc-lua-marshal` 0.3.2.1.

use mlua::{Lua, Value};
use randoc_ast::{Block, Inline, Table, Target};

use crate::object::{Node, block_tag, inline_tag, new_object};
use crate::peek::{Cx, DefinitionItem, Peek, PeekError};
use crate::push;

/// The content of an element, which has one of five forms.
enum Content {
    Blocks(Vec<Block>),
    Inlines(Vec<Inline>),
    Lines(Vec<Vec<Inline>>),
    DefItems(Vec<DefinitionItem>),
    ListItems(Vec<Vec<Block>>),
}

// A content of the wrong form gives an empty list and no error. The Haskell
// original raises its error with `throwM` in the list monad, and `throwM`
// of a list is the empty list. So Pandoc never shows the text "expected
// Inlines, got ...", and Randoc does the same.
impl Content {
    fn inlines(self) -> Vec<Inline> {
        match self {
            Content::Inlines(inlines) => inlines,
            _ => Vec::new(),
        }
    }

    fn note_blocks(self) -> Vec<Block> {
        match self {
            Content::Blocks(blocks) => blocks,
            _ => Vec::new(),
        }
    }

    fn blocks(self) -> Vec<Block> {
        match self {
            Content::Blocks(blocks) => blocks,
            Content::Inlines(inlines) if !inlines.is_empty() => vec![Block::Plain(inlines)],
            _ => Vec::new(),
        }
    }

    fn lines(self) -> Vec<Vec<Inline>> {
        match self {
            Content::Lines(lines) => lines,
            _ => Vec::new(),
        }
    }

    fn def_items(self) -> Vec<DefinitionItem> {
        match self {
            Content::DefItems(items) => items,
            _ => Vec::new(),
        }
    }

    fn list_items(self) -> Vec<Vec<Block>> {
        match self {
            Content::Blocks(blocks) => blocks.into_iter().map(|block| vec![block]).collect(),
            Content::Lines(lines) => lines
                .into_iter()
                .map(|line| vec![Block::Plain(line)])
                .collect(),
            Content::ListItems(items) => items,
            _ => Vec::new(),
        }
    }
}

/// Reads a value as the content of an element. The first form that
/// converts wins.
fn peek_content(cx: &Cx, value: &Value) -> Peek<Content> {
    let failed = |result: &Peek<Content>| matches!(result, Err(PeekError::Failure { .. }));
    let mut result = cx.inlines_fuzzy(value).map(Content::Inlines);
    if failed(&result) {
        result = cx
            .list(value, |item| cx.inlines_fuzzy(item))
            .map(Content::Lines);
    }
    if failed(&result) {
        result = cx.blocks_fuzzy(value).map(Content::Blocks);
    }
    if failed(&result) {
        result = cx
            .list(value, |item| cx.blocks_fuzzy(item))
            .map(Content::ListItems);
    }
    if failed(&result) {
        result = cx
            .list(value, |item| cx.definition_item(item))
            .map(Content::DefItems);
    }
    result
}

/// Returns the Lua value of a property of an object. The value is `nil`
/// when the object has no such property.
pub(crate) fn get_property(lua: &Lua, node: &Node, key: &str) -> mlua::Result<Value> {
    let absent = Ok(Value::Nil);
    match node {
        Node::Inline(inline) => match (key, inline) {
            ("tag", _) => push::text(lua, inline_tag(inline)),
            ("attr", Inline::Code(attr, _))
            | ("attr", Inline::Image(attr, ..))
            | ("attr", Inline::Link(attr, ..))
            | ("attr", Inline::Span(attr, _)) => push::attr(lua, attr),
            ("caption", Inline::Image(_, caption, _)) => push::inlines(lua, caption),
            ("citations", Inline::Cite(citations, _)) => push::list(lua, citations, push::citation),
            ("content", Inline::Cite(_, content))
            | ("content", Inline::Emph(content))
            | ("content", Inline::Link(_, content, _))
            | ("content", Inline::Quoted(_, content))
            | ("content", Inline::SmallCaps(content))
            | ("content", Inline::Span(_, content))
            | ("content", Inline::Strikeout(content))
            | ("content", Inline::Strong(content))
            | ("content", Inline::Subscript(content))
            | ("content", Inline::Superscript(content))
            | ("content", Inline::Underline(content)) => push::inlines(lua, content),
            ("content", Inline::Note(content)) => push::blocks(lua, content),
            ("format", Inline::RawInline(format, _)) => push::text(lua, format.as_str()),
            ("mathtype", Inline::Math(math_type, _)) => {
                push::text(lua, push::math_type_name(*math_type))
            }
            ("quotetype", Inline::Quoted(quote_type, _)) => {
                push::text(lua, push::quote_type_name(*quote_type))
            }
            ("src", Inline::Image(_, _, target)) => push::text(lua, &target.url),
            ("target", Inline::Link(_, _, target)) => push::text(lua, &target.url),
            ("title", Inline::Image(_, _, target)) | ("title", Inline::Link(_, _, target)) => {
                push::text(lua, &target.title)
            }
            ("text", Inline::Code(_, text))
            | ("text", Inline::Math(_, text))
            | ("text", Inline::RawInline(_, text))
            | ("text", Inline::Str(text)) => push::text(lua, text),
            _ => absent,
        },
        Node::Block(block) => match (key, block) {
            ("tag", _) => push::text(lua, block_tag(block)),
            ("attr", Block::CodeBlock(attr, _))
            | ("attr", Block::Div(attr, _))
            | ("attr", Block::Figure(attr, ..))
            | ("attr", Block::Header(_, attr, _)) => push::attr(lua, attr),
            ("attr", Block::Table(table)) => push::attr(lua, &table.attr),
            ("bodies", Block::Table(table)) => push::list(lua, &table.bodies, push::table_body),
            ("caption", Block::Figure(_, caption, _)) => push::caption(lua, caption),
            ("caption", Block::Table(table)) => push::caption(lua, &table.caption),
            ("colspecs", Block::Table(table)) => push::list(lua, &table.col_specs, push::col_spec),
            ("content", Block::Para(content))
            | ("content", Block::Plain(content))
            | ("content", Block::Header(_, _, content)) => push::inlines(lua, content),
            ("content", Block::BlockQuote(content))
            | ("content", Block::Div(_, content))
            | ("content", Block::Figure(_, _, content)) => push::blocks(lua, content),
            ("content", Block::LineBlock(lines)) => {
                push::list(lua, lines, |lua, line| push::inlines(lua, line))
            }
            ("content", Block::BulletList(items)) | ("content", Block::OrderedList(_, items)) => {
                push::list(lua, items, |lua, item| push::blocks(lua, item))
            }
            ("content", Block::DefinitionList(items)) => {
                push::list(lua, items, push::definition_item)
            }
            ("foot", Block::Table(table)) => push::table_foot(lua, &table.foot),
            ("format", Block::RawBlock(format, _)) => push::text(lua, format.as_str()),
            ("head", Block::Table(table)) => push::table_head(lua, &table.head),
            ("level", Block::Header(level, ..)) => push::integer(lua, *level),
            ("listAttributes", Block::OrderedList(attributes, _)) => {
                push::list_attributes(lua, attributes)
            }
            ("text", Block::CodeBlock(_, text)) | ("text", Block::RawBlock(_, text)) => {
                push::text(lua, text)
            }
            _ => absent,
        },
        Node::Pandoc(doc) => match key {
            "blocks" => push::blocks(lua, &doc.blocks),
            "meta" => push::meta(lua, &doc.meta),
            _ => absent,
        },
        Node::Attr(attr) => match key {
            "identifier" => push::text(lua, &attr.id),
            "classes" => push::list(lua, &attr.classes, |lua, class| push::text(lua, class)),
            "attributes" => new_object(lua, Node::AttributeList(attr.attributes.clone())),
            "tag" => push::text(lua, "Attr"),
            _ => absent,
        },
        Node::Citation(citation) => match key {
            "id" => push::text(lua, &citation.id),
            "mode" => push::text(lua, push::citation_mode_name(citation.mode)),
            "prefix" => push::inlines(lua, &citation.prefix),
            "suffix" => push::inlines(lua, &citation.suffix),
            "note_num" => push::integer(lua, citation.note_num),
            "hash" => push::integer(lua, citation.hash),
            _ => absent,
        },
        Node::ListAttributes(attributes) => match key {
            "start" => push::integer(lua, attributes.start),
            "style" => push::text(lua, push::number_style_name(attributes.style)),
            "delimiter" => push::text(lua, push::number_delim_name(attributes.delim)),
            _ => absent,
        },
        Node::Cell(cell) => match key {
            "attr" => push::attr(lua, &cell.attr),
            "alignment" => push::text(lua, push::alignment_name(cell.align)),
            "row_span" => push::integer(lua, cell.row_span),
            "col_span" => push::integer(lua, cell.col_span),
            "contents" => push::blocks(lua, &cell.content),
            _ => absent,
        },
        Node::Row(row) => match key {
            "attr" => push::attr(lua, &row.attr),
            "cells" => push::list(lua, &row.cells, push::cell),
            _ => absent,
        },
        Node::TableHead(head) => match key {
            "attr" => push::attr(lua, &head.attr),
            "rows" => push::list(lua, &head.rows, push::row),
            _ => absent,
        },
        Node::TableFoot(foot) => match key {
            "attr" => push::attr(lua, &foot.attr),
            "rows" => push::list(lua, &foot.rows, push::row),
            _ => absent,
        },
        Node::TableBody(body) => match key {
            "attr" => push::attr(lua, &body.attr),
            "row_head_columns" => push::integer(lua, body.row_head_columns),
            "head" => push::list(lua, &body.head, push::row),
            "body" => push::list(lua, &body.body, push::row),
            _ => absent,
        },
        Node::Caption(caption) => match key {
            "short" => match &caption.short {
                Some(short) => push::inlines(lua, short),
                None => absent,
            },
            "long" => push::blocks(lua, &caption.long),
            _ => absent,
        },
        Node::AttributeList(_) | Node::Version(_) => absent,
    }
}

/// Makes the error for a property that the constructor of the object does
/// not have.
fn unavailable<T>(key: &str) -> Peek<T> {
    Err(PeekError::Thrown(format!(
        "Trying to set unavailable property {key}."
    )))
}

/// Converts a failure of a property value to a Lua error, as `forcePeek`
/// does in the setter of a property.
fn force<T>(result: Peek<T>) -> Peek<T> {
    result.map_err(PeekError::thrown)
}

fn set_inline(cx: &Cx, inline: Inline, key: &str, value: &Value) -> Peek<Inline> {
    match key {
        "attr" => {
            let attr = force(cx.attr(value))?;
            match inline {
                Inline::Code(_, text) => Ok(Inline::Code(attr, text)),
                Inline::Image(_, caption, target) => Ok(Inline::Image(attr, caption, target)),
                Inline::Link(_, content, target) => Ok(Inline::Link(attr, content, target)),
                Inline::Span(_, content) => Ok(Inline::Span(attr, content)),
                _ => unavailable(key),
            }
        }
        "caption" => {
            let caption = force(cx.inlines_fuzzy(value))?;
            match inline {
                Inline::Image(attr, _, target) => Ok(Inline::Image(attr, caption, target)),
                _ => unavailable(key),
            }
        }
        "citations" => {
            let citations = force(cx.list(value, |item| cx.citation(item)))?;
            match inline {
                Inline::Cite(_, content) => Ok(Inline::Cite(citations, content)),
                _ => unavailable(key),
            }
        }
        "content" => {
            let content = force(peek_content(cx, value))?;
            match inline {
                Inline::Cite(citations, _) => Ok(Inline::Cite(citations, content.inlines())),
                Inline::Emph(_) => Ok(Inline::Emph(content.inlines())),
                Inline::Link(attr, _, target) => Ok(Inline::Link(attr, content.inlines(), target)),
                Inline::Quoted(quote_type, _) => Ok(Inline::Quoted(quote_type, content.inlines())),
                Inline::SmallCaps(_) => Ok(Inline::SmallCaps(content.inlines())),
                Inline::Span(attr, _) => Ok(Inline::Span(attr, content.inlines())),
                Inline::Strikeout(_) => Ok(Inline::Strikeout(content.inlines())),
                Inline::Strong(_) => Ok(Inline::Strong(content.inlines())),
                Inline::Subscript(_) => Ok(Inline::Subscript(content.inlines())),
                Inline::Superscript(_) => Ok(Inline::Superscript(content.inlines())),
                Inline::Underline(_) => Ok(Inline::Underline(content.inlines())),
                Inline::Note(_) => Ok(Inline::Note(content.note_blocks())),
                _ => unavailable(key),
            }
        }
        "format" => {
            let format = force(cx.format(value))?;
            match inline {
                Inline::RawInline(_, text) => Ok(Inline::RawInline(format, text)),
                _ => unavailable(key),
            }
        }
        "mathtype" => {
            let math_type = force(cx.math_type(value))?;
            match inline {
                Inline::Math(_, text) => Ok(Inline::Math(math_type, text)),
                _ => unavailable(key),
            }
        }
        "quotetype" => {
            let quote_type = force(cx.quote_type(value))?;
            match inline {
                Inline::Quoted(_, content) => Ok(Inline::Quoted(quote_type, content)),
                _ => unavailable(key),
            }
        }
        "src" => {
            let url = force(cx.text(value))?;
            match inline {
                Inline::Image(attr, caption, target) => {
                    Ok(Inline::Image(attr, caption, Target::new(url, target.title)))
                }
                _ => unavailable(key),
            }
        }
        "target" => {
            let url = force(cx.text(value))?;
            match inline {
                Inline::Link(attr, content, target) => {
                    Ok(Inline::Link(attr, content, Target::new(url, target.title)))
                }
                _ => unavailable(key),
            }
        }
        "title" => {
            let title = force(cx.text(value))?;
            match inline {
                Inline::Image(attr, caption, target) => {
                    Ok(Inline::Image(attr, caption, Target::new(target.url, title)))
                }
                Inline::Link(attr, content, target) => {
                    Ok(Inline::Link(attr, content, Target::new(target.url, title)))
                }
                _ => unavailable(key),
            }
        }
        "text" => {
            let text = force(cx.text(value))?;
            match inline {
                Inline::Code(attr, _) => Ok(Inline::Code(attr, text)),
                Inline::Math(math_type, _) => Ok(Inline::Math(math_type, text)),
                Inline::RawInline(format, _) => Ok(Inline::RawInline(format, text)),
                Inline::Str(_) => Ok(Inline::Str(text)),
                _ => unavailable(key),
            }
        }
        _ => Ok(inline),
    }
}

/// Applies `change` to the table of a block. Each other block has no such
/// property.
fn set_table(block: Block, key: &str, change: impl FnOnce(&mut Table)) -> Peek<Block> {
    match block {
        Block::Table(mut table) => {
            change(&mut table);
            Ok(Block::Table(table))
        }
        _ => unavailable(key),
    }
}

fn set_block(cx: &Cx, block: Block, key: &str, value: &Value) -> Peek<Block> {
    match key {
        "attr" => {
            let attr = force(cx.attr(value))?;
            match block {
                Block::CodeBlock(_, text) => Ok(Block::CodeBlock(attr, text)),
                Block::Div(_, content) => Ok(Block::Div(attr, content)),
                Block::Figure(_, caption, content) => Ok(Block::Figure(attr, caption, content)),
                Block::Header(level, _, content) => Ok(Block::Header(level, attr, content)),
                other => set_table(other, key, |table| table.attr = attr),
            }
        }
        "bodies" => {
            let bodies = force(cx.list(value, |item| cx.table_body_fuzzy(item)))?;
            set_table(block, key, |table| table.bodies = bodies)
        }
        "caption" => {
            let caption = force(cx.caption_fuzzy(value))?;
            match block {
                Block::Figure(attr, _, content) => Ok(Block::Figure(attr, caption, content)),
                other => set_table(other, key, |table| table.caption = caption),
            }
        }
        "colspecs" => {
            let col_specs = force(cx.list(value, |item| cx.col_spec(item)))?;
            set_table(block, key, |table| table.col_specs = col_specs)
        }
        "content" => {
            let content = force(peek_content(cx, value))?;
            match block {
                Block::Para(_) => Ok(Block::Para(content.inlines())),
                Block::Plain(_) => Ok(Block::Plain(content.inlines())),
                Block::Header(level, attr, _) => Ok(Block::Header(level, attr, content.inlines())),
                Block::BlockQuote(_) => Ok(Block::BlockQuote(content.blocks())),
                Block::Div(attr, _) => Ok(Block::Div(attr, content.blocks())),
                Block::Figure(attr, caption, _) => {
                    Ok(Block::Figure(attr, caption, content.blocks()))
                }
                Block::LineBlock(_) => Ok(Block::LineBlock(content.lines())),
                Block::BulletList(_) => Ok(Block::BulletList(content.list_items())),
                Block::OrderedList(attributes, _) => {
                    Ok(Block::OrderedList(attributes, content.list_items()))
                }
                Block::DefinitionList(_) => Ok(Block::DefinitionList(content.def_items())),
                _ => unavailable(key),
            }
        }
        "foot" => {
            let foot = force(cx.table_foot(value))?;
            set_table(block, key, |table| table.foot = foot)
        }
        "format" => {
            let format = force(cx.format(value))?;
            match block {
                Block::RawBlock(_, text) => Ok(Block::RawBlock(format, text)),
                _ => unavailable(key),
            }
        }
        "head" => {
            let head = force(cx.table_head(value))?;
            set_table(block, key, |table| table.head = head)
        }
        "level" => {
            let level = force(cx.integral(value))?;
            match block {
                Block::Header(_, attr, content) => Ok(Block::Header(level, attr, content)),
                _ => unavailable(key),
            }
        }
        "listAttributes" => {
            let attributes = force(cx.list_attributes(value))?;
            match block {
                Block::OrderedList(_, items) => Ok(Block::OrderedList(attributes, items)),
                _ => unavailable(key),
            }
        }
        "text" => {
            let text = force(cx.text(value))?;
            match block {
                Block::CodeBlock(attr, _) => Ok(Block::CodeBlock(attr, text)),
                Block::RawBlock(format, _) => Ok(Block::RawBlock(format, text)),
                _ => unavailable(key),
            }
        }
        _ => Ok(block),
    }
}

/// Puts a cached property value into the base of an object.
///
/// A key that is not a property that a script can write leaves the object
/// as it is. A value of the wrong form gives a Lua error.
pub(crate) fn set_property(cx: &Cx, node: Node, key: &str, value: &Value) -> Peek<Node> {
    match node {
        Node::Inline(inline) => set_inline(cx, inline, key, value).map(Node::Inline),
        Node::Block(block) => set_block(cx, block, key, value).map(Node::Block),
        Node::Pandoc(mut doc) => {
            match key {
                "blocks" => doc.blocks = force(cx.blocks_fuzzy(value))?,
                "meta" => doc.meta = force(cx.meta(value))?,
                _ => {}
            }
            Ok(Node::Pandoc(doc))
        }
        Node::Attr(mut attr) => {
            match key {
                "identifier" => attr.id = force(cx.text(value))?,
                "classes" => attr.classes = force(cx.list(value, |item| cx.text(item)))?,
                "attributes" => attr.attributes = force(cx.attribute_list(value))?,
                _ => {}
            }
            Ok(Node::Attr(attr))
        }
        Node::Citation(mut citation) => {
            match key {
                "id" => citation.id = force(cx.text(value))?,
                "mode" => citation.mode = force(cx.citation_mode(value))?,
                "prefix" => citation.prefix = force(cx.inlines_fuzzy(value))?,
                "suffix" => citation.suffix = force(cx.inlines_fuzzy(value))?,
                "note_num" => citation.note_num = force(cx.integral(value))?,
                "hash" => citation.hash = force(cx.integral(value))?,
                _ => {}
            }
            Ok(Node::Citation(citation))
        }
        Node::ListAttributes(mut attributes) => {
            match key {
                "start" => attributes.start = force(cx.integral(value))?,
                "style" => attributes.style = force(cx.number_style(value))?,
                "delimiter" => attributes.delim = force(cx.number_delim(value))?,
                _ => {}
            }
            Ok(Node::ListAttributes(attributes))
        }
        Node::Cell(mut cell) => {
            match key {
                "attr" => cell.attr = force(cx.attr(value))?,
                "alignment" => cell.align = force(cx.alignment(value))?,
                "row_span" => cell.row_span = force(cx.integral(value))?,
                "col_span" => cell.col_span = force(cx.integral(value))?,
                "contents" => cell.content = force(cx.blocks_fuzzy(value))?,
                _ => {}
            }
            Ok(Node::Cell(cell))
        }
        Node::Row(mut row) => {
            match key {
                "attr" => row.attr = force(cx.attr(value))?,
                "cells" => row.cells = force(cx.list(value, |item| cx.cell_fuzzy(item)))?,
                _ => {}
            }
            Ok(Node::Row(row))
        }
        Node::TableHead(mut head) => {
            match key {
                "attr" => head.attr = force(cx.attr(value))?,
                "rows" => head.rows = force(cx.rows(value))?,
                _ => {}
            }
            Ok(Node::TableHead(head))
        }
        Node::TableFoot(mut foot) => {
            match key {
                "attr" => foot.attr = force(cx.attr(value))?,
                "rows" => foot.rows = force(cx.rows(value))?,
                _ => {}
            }
            Ok(Node::TableFoot(foot))
        }
        Node::TableBody(mut body) => {
            match key {
                "attr" => body.attr = force(cx.attr(value))?,
                "row_head_columns" => body.row_head_columns = force(cx.integral(value))?,
                "head" => body.head = force(cx.rows(value))?,
                "body" => body.body = force(cx.rows(value))?,
                _ => {}
            }
            Ok(Node::TableBody(body))
        }
        Node::Caption(mut caption) => {
            match key {
                "short" => {
                    caption.short = match value {
                        Value::Nil => None,
                        other => Some(force(cx.inlines_fuzzy(other))?),
                    }
                }
                "long" => caption.long = force(cx.blocks_fuzzy(value))?,
                _ => {}
            }
            Ok(Node::Caption(caption))
        }
        other @ (Node::AttributeList(_) | Node::Version(_)) => Ok(other),
    }
}
