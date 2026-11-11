//! The application of a Lua filter to a document or to a part of it.
//!
//! Sources: the traversal rules of `doc/lua-filters.md` of Pandoc 3.13, the
//! files `Marshal/Filter.hs` and `Marshal/Shared.hs` of `pandoc-lua-marshal`
//! 0.3.2.1 (MIT), and the oracle matrix `tests/replay/walk.txt`, which
//! also gives the order of the nodes. No file of `pandoc-types` is a source
//! of this file.
//!
//! A typewise walk makes four passes over the tree, each from the leaves to
//! the root: the inline element functions, `Inlines`, the block element
//! functions, `Blocks`. Then `Meta` and `Pandoc` run. A topdown walk makes
//! one pass from the root: the function of a list or an element runs before
//! the walk enters it, and `false` as the second result stops the descent.

use std::collections::HashMap;

use mlua::{Function, MultiValue, ObjectLike, Value};
use randoc_ast::{Block, Caption, Cell, Inline, Meta, MetaValue, Pandoc, Row};

use crate::object::{Node, block_tag, inline_tag};
use crate::peek::{Cx, Peek, PeekError, depth_message, lua_type_name};
use crate::push;

/// The registry name of the Lua function that calls a function with a
/// traceback handler (`call_traced` of the prelude).
pub(crate) const CALL_TRACED: &str = "randoc.call_traced";

const INLINE_NAMES: [&str; 20] = [
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
];

const BLOCK_NAMES: [&str; 14] = [
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
];

/// How a filter walks the tree: the field `traverse` of the filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Order {
    Typewise,
    Topdown,
}

/// The four passes of a typewise walk, in their order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pass {
    InlineElements,
    InlineLists,
    BlockElements,
    BlockLists,
}

const PASSES: [Pass; 4] = [
    Pass::InlineElements,
    Pass::InlineLists,
    Pass::BlockElements,
    Pass::BlockLists,
];

/// One filter: its traversal order and each field whose value is a Lua
/// function and whose name is a filter function name.
pub(crate) struct Filter {
    order: Order,
    functions: HashMap<&'static str, Function>,
}

/// The names of the filter functions in the order in which Pandoc reads
/// them from a filter table, after `traverse`. A table with an `__index`
/// function can see the order.
const LOOKUP_ORDER: [&str; 40] = [
    "Div",
    "Figure",
    "Table",
    "HorizontalRule",
    "Header",
    "DefinitionList",
    "BulletList",
    "OrderedList",
    "BlockQuote",
    "RawBlock",
    "CodeBlock",
    "LineBlock",
    "Para",
    "Plain",
    "Span",
    "Note",
    "Image",
    "Link",
    "RawInline",
    "Math",
    "LineBreak",
    "SoftBreak",
    "Space",
    "Code",
    "Cite",
    "Quoted",
    "SmallCaps",
    "Subscript",
    "Superscript",
    "Strikeout",
    "Strong",
    "Underline",
    "Emph",
    "Str",
    "Inlines",
    "Blocks",
    "Inline",
    "Block",
    "Meta",
    "Pandoc",
];

impl Filter {
    /// Reads a filter. Lua indexes the value, as `lua_getfield` does: a
    /// table or a userdata value with `__index` can be a filter, a string
    /// is an empty filter, and each other value gives the error of Lua.
    pub(crate) fn peek(value: &Value) -> Peek<Filter> {
        let get = |key: &str| -> Peek<Value> {
            match value {
                Value::Table(table) => Ok(table.get::<Value>(key)?),
                Value::UserData(userdata) => Ok(userdata.get::<Value>(key)?),
                Value::String(_) => Ok(Value::Nil),
                other => Err(PeekError::Thrown(format!(
                    "attempt to index a {} value",
                    lua_type_name(other)
                ))),
            }
        };
        let order = match get("traverse")? {
            Value::String(text) if text.as_bytes() == b"topdown" => Order::Topdown,
            _ => Order::Typewise,
        };
        let mut functions = HashMap::new();
        for name in LOOKUP_ORDER {
            if let Value::Function(function) = get(name)? {
                functions.insert(name, function);
            }
        }
        Ok(Filter { order, functions })
    }

    fn function(&self, name: &str) -> Option<&Function> {
        self.functions.get(name)
    }

    fn for_inline(&self, inline: &Inline) -> Option<&Function> {
        self.function(inline_tag(inline))
            .or_else(|| self.function("Inline"))
    }

    fn for_block(&self, block: &Block) -> Option<&Function> {
        self.function(block_tag(block))
            .or_else(|| self.function("Block"))
    }

    fn has_inline_elements(&self) -> bool {
        self.function("Inline").is_some() || INLINE_NAMES.iter().any(|n| self.function(n).is_some())
    }

    fn has_block_elements(&self) -> bool {
        self.function("Block").is_some() || BLOCK_NAMES.iter().any(|n| self.function(n).is_some())
    }

    fn runs(&self, pass: Pass) -> bool {
        match pass {
            Pass::InlineElements => self.has_inline_elements(),
            Pass::InlineLists => self.function("Inlines").is_some(),
            Pass::BlockElements => self.has_block_elements(),
            Pass::BlockLists => self.function("Blocks").is_some(),
        }
    }
}

/// The part of a tree that a `walk` method or a filter changes.
pub(crate) enum Subject<'a> {
    Inline(&'a mut Inline),
    Block(&'a mut Block),
    Inlines(&'a mut Vec<Inline>),
    Blocks(&'a mut Vec<Block>),
    Cell(&'a mut Cell),
    Row(&'a mut Row),
}

/// A list of the tree that a filter can change.
enum ListMut<'a> {
    Inlines(&'a mut Vec<Inline>),
    Blocks(&'a mut Vec<Block>),
}

/// A list of the tree, for the depth check.
enum ListRef<'a> {
    Inlines(&'a [Inline]),
    Blocks(&'a [Block]),
}

/// Defines the functions that give the child lists of a node in the order
/// of the walk, once for shared and once for mutable references.
macro_rules! child_lists {
    ($list:ident, $inline:ident, $block:ident, $caption:ident, $row:ident, $($m:tt)?) => {
        fn $inline(inline: & $($m)? Inline) -> Vec<$list<'_>> {
            match inline {
                Inline::Emph(content)
                | Inline::Underline(content)
                | Inline::Strong(content)
                | Inline::Strikeout(content)
                | Inline::Superscript(content)
                | Inline::Subscript(content)
                | Inline::SmallCaps(content)
                | Inline::Quoted(_, content)
                | Inline::Link(_, content, _)
                | Inline::Image(_, content, _)
                | Inline::Span(_, content) => vec![$list::Inlines(content)],
                Inline::Cite(citations, content) => {
                    let mut lists = Vec::new();
                    for citation in citations {
                        lists.push($list::Inlines(& $($m)? citation.prefix));
                        lists.push($list::Inlines(& $($m)? citation.suffix));
                    }
                    lists.push($list::Inlines(content));
                    lists
                }
                Inline::Note(content) => vec![$list::Blocks(content)],
                Inline::Str(_)
                | Inline::Code(..)
                | Inline::Space
                | Inline::SoftBreak
                | Inline::LineBreak
                | Inline::Math(..)
                | Inline::RawInline(..) => Vec::new(),
            }
        }

        fn $caption<'a>(caption: &'a $($m)? Caption, lists: &mut Vec<$list<'a>>) {
            if let Some(short) = & $($m)? caption.short {
                lists.push($list::Inlines(short));
            }
            lists.push($list::Blocks(& $($m)? caption.long));
        }

        fn $row<'a>(row: &'a $($m)? Row, lists: &mut Vec<$list<'a>>) {
            for cell in & $($m)? row.cells {
                lists.push($list::Blocks(& $($m)? cell.content));
            }
        }

        fn $block(block: & $($m)? Block) -> Vec<$list<'_>> {
            match block {
                Block::Plain(content) | Block::Para(content) | Block::Header(_, _, content) => {
                    vec![$list::Inlines(content)]
                }
                Block::LineBlock(lines) => lines.into_iter().map(|line| $list::Inlines(line)).collect(),
                Block::BlockQuote(content) | Block::Div(_, content) => vec![$list::Blocks(content)],
                Block::OrderedList(_, items) | Block::BulletList(items) => {
                    items.into_iter().map(|item| $list::Blocks(item)).collect()
                }
                Block::DefinitionList(items) => {
                    let mut lists = Vec::new();
                    for (term, definitions) in items {
                        lists.push($list::Inlines(term));
                        lists.extend(definitions.into_iter().map(|item| $list::Blocks(item)));
                    }
                    lists
                }
                Block::Table(table) => {
                    let mut lists = Vec::new();
                    let table = & $($m)? **table;
                    $caption(& $($m)? table.caption, &mut lists);
                    for row in & $($m)? table.head.rows {
                        $row(row, &mut lists);
                    }
                    for body in & $($m)? table.bodies {
                        for row in & $($m)? body.head {
                            $row(row, &mut lists);
                        }
                        for row in & $($m)? body.body {
                            $row(row, &mut lists);
                        }
                    }
                    for row in & $($m)? table.foot.rows {
                        $row(row, &mut lists);
                    }
                    lists
                }
                Block::Figure(_, caption, content) => {
                    let mut lists = Vec::new();
                    $caption(caption, &mut lists);
                    lists.push($list::Blocks(content));
                    lists
                }
                Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => Vec::new(),
            }
        }
    };
}

child_lists!(
    ListMut,
    inline_lists_mut,
    block_lists_mut,
    caption_lists_mut,
    row_lists_mut,
    mut
);
child_lists!(ListRef, inline_lists, block_lists, caption_lists, row_lists,);

fn depth_error(cx: &Cx) -> PeekError {
    PeekError::Thrown(depth_message(cx.max_depth))
}

/// Checks that each element of `list` is at most at the depth limit when
/// the list is at `level`. The recursion stops at the limit.
fn check_list(cx: &Cx, list: ListRef, level: usize) -> Peek<()> {
    let empty = match &list {
        ListRef::Inlines(items) => items.is_empty(),
        ListRef::Blocks(items) => items.is_empty(),
    };
    if empty {
        return Ok(());
    }
    if level > cx.max_depth {
        return Err(depth_error(cx));
    }
    let children: Vec<ListRef> = match list {
        ListRef::Inlines(items) => items.iter().flat_map(inline_lists).collect(),
        ListRef::Blocks(items) => items.iter().flat_map(block_lists).collect(),
    };
    children
        .into_iter()
        .try_for_each(|child| check_list(cx, child, level + 1))
}

fn check_meta_value(cx: &Cx, value: &MetaValue, level: usize) -> Peek<()> {
    if level > cx.max_depth {
        return Err(depth_error(cx));
    }
    match value {
        MetaValue::MetaMap(map) => map
            .values()
            .try_for_each(|item| check_meta_value(cx, item, level + 1)),
        MetaValue::MetaList(items) => items
            .iter()
            .try_for_each(|item| check_meta_value(cx, item, level + 1)),
        MetaValue::MetaInlines(items) => check_list(cx, ListRef::Inlines(items), level),
        MetaValue::MetaBlocks(items) => check_list(cx, ListRef::Blocks(items), level),
        MetaValue::MetaBool(_) | MetaValue::MetaString(_) => Ok(()),
    }
}

fn check_meta(cx: &Cx, meta: &Meta) -> Peek<()> {
    meta.0
        .values()
        .try_for_each(|value| check_meta_value(cx, value, 1))
}

/// Checks that a list of blocks is not nested deeper than the limit.
pub(crate) fn check_blocks(cx: &Cx, blocks: &[Block]) -> Peek<()> {
    check_list(cx, ListRef::Blocks(blocks), 1)
}

/// Checks that a document is not nested deeper than the limit.
pub(crate) fn check_document(cx: &Cx, doc: &Pandoc) -> Peek<()> {
    check_meta(cx, &doc.meta)?;
    check_list(cx, ListRef::Blocks(&doc.blocks), 1)
}

/// Checks that a value of the `pandoc` module is not nested deeper than the
/// limit.
pub(crate) fn check_node(cx: &Cx, node: &Node) -> Peek<()> {
    let mut lists = Vec::new();
    match node {
        Node::Pandoc(doc) => return check_document(cx, doc),
        Node::Inline(inline) => lists.push(ListRef::Inlines(std::slice::from_ref(inline))),
        Node::Block(block) => lists.push(ListRef::Blocks(std::slice::from_ref(block))),
        Node::Cell(cell) => lists.push(ListRef::Blocks(&cell.content)),
        Node::Row(row) => row_lists(row, &mut lists),
        Node::TableHead(head) => head.rows.iter().for_each(|row| row_lists(row, &mut lists)),
        Node::TableFoot(foot) => foot.rows.iter().for_each(|row| row_lists(row, &mut lists)),
        Node::TableBody(body) => body
            .head
            .iter()
            .chain(&body.body)
            .for_each(|row| row_lists(row, &mut lists)),
        Node::Caption(caption) => caption_lists(caption, &mut lists),
        Node::Citation(citation) => {
            lists.push(ListRef::Inlines(&citation.prefix));
            lists.push(ListRef::Inlines(&citation.suffix));
        }
        Node::Attr(_) | Node::AttributeList(_) | Node::ListAttributes(_) | Node::Version(_) => {}
    }
    lists
        .into_iter()
        .try_for_each(|list| check_list(cx, list, 1))
}

/// Calls one filter function with one argument. Returns the first result
/// and whether the walk enters the result: a second result `false` stops
/// it.
fn call(cx: &Cx, function: &Function, argument: Value) -> Peek<(Value, bool)> {
    let call_traced: Function = cx.lua.named_registry_value(CALL_TRACED)?;
    let mut results = call_traced
        .call::<MultiValue>((function.clone(), argument))?
        .into_iter();
    match results.next() {
        Some(Value::Boolean(true)) => {
            let first = results.next().unwrap_or(Value::Nil);
            let descend = !matches!(results.next(), Some(Value::Boolean(false)));
            Ok((first, descend))
        }
        _ => Err(PeekError::Thrown(match results.next() {
            Some(Value::String(message)) => message.to_string_lossy(),
            _ => "the filter failed with an error that is not a string".to_string(),
        })),
    }
}

/// The new elements for one element: the element itself when the function
/// gives `nil`.
fn splice_inline(cx: &Cx, function: &Function, inline: Inline) -> Peek<(Vec<Inline>, bool)> {
    let (result, descend) = call(cx, function, push::inline(cx.lua, &inline)?)?;
    if result.is_nil() {
        return Ok((vec![inline], descend));
    }
    Ok((cx.inlines_fuzzy(&result)?, descend))
}

fn splice_block(cx: &Cx, function: &Function, block: Block) -> Peek<(Vec<Block>, bool)> {
    let (result, descend) = call(cx, function, push::block(cx.lua, &block)?)?;
    if result.is_nil() {
        return Ok((vec![block], descend));
    }
    Ok((cx.blocks_fuzzy(&result)?, descend))
}

/// Applies the function of a whole list. Returns whether the walk enters
/// the list.
fn apply_to_list(cx: &Cx, function: &Function, list: &mut ListMut, level: usize) -> Peek<bool> {
    let (result, descend) = match list {
        ListMut::Inlines(items) => call(cx, function, push::inlines(cx.lua, items)?)?,
        ListMut::Blocks(items) => call(cx, function, push::blocks(cx.lua, items)?)?,
    };
    if !result.is_nil() {
        match list {
            ListMut::Inlines(items) => {
                let new = cx.inlines_fuzzy(&result)?;
                check_list(cx, ListRef::Inlines(&new), level)?;
                **items = new;
            }
            ListMut::Blocks(items) => {
                let new = cx.blocks_fuzzy(&result)?;
                check_list(cx, ListRef::Blocks(&new), level)?;
                **items = new;
            }
        }
    }
    Ok(descend)
}

/// One pass of a typewise walk over a list at `level`: first the children
/// of each element, then the function of the element, then the function
/// of the list.
fn bottom_up(cx: &Cx, filter: &Filter, pass: Pass, mut list: ListMut, level: usize) -> Peek<()> {
    if level > cx.max_depth {
        return Err(depth_error(cx));
    }
    match &mut list {
        ListMut::Inlines(items) => {
            let function = |inline: &Inline| match pass {
                Pass::InlineElements => filter.for_inline(inline),
                _ => None,
            };
            let mut result = Vec::with_capacity(items.len());
            for mut inline in std::mem::take(*items) {
                for child in inline_lists_mut(&mut inline) {
                    bottom_up(cx, filter, pass, child, level + 1)?;
                }
                match function(&inline) {
                    Some(function) => {
                        let (new, _) = splice_inline(cx, function, inline)?;
                        check_list(cx, ListRef::Inlines(&new), level)?;
                        result.extend(new);
                    }
                    None => result.push(inline),
                }
            }
            **items = result;
        }
        ListMut::Blocks(items) => {
            let function = |block: &Block| match pass {
                Pass::BlockElements => filter.for_block(block),
                _ => None,
            };
            let mut result = Vec::with_capacity(items.len());
            for mut block in std::mem::take(*items) {
                for child in block_lists_mut(&mut block) {
                    bottom_up(cx, filter, pass, child, level + 1)?;
                }
                match function(&block) {
                    Some(function) => {
                        let (new, _) = splice_block(cx, function, block)?;
                        check_list(cx, ListRef::Blocks(&new), level)?;
                        result.extend(new);
                    }
                    None => result.push(block),
                }
            }
            **items = result;
        }
    }
    let list_function = match (&list, pass) {
        (ListMut::Inlines(_), Pass::InlineLists) => filter.function("Inlines"),
        (ListMut::Blocks(_), Pass::BlockLists) => filter.function("Blocks"),
        _ => None,
    };
    if let Some(function) = list_function {
        apply_to_list(cx, function, &mut list, level)?;
    }
    Ok(())
}

/// A topdown walk over a list at `level`: the function of the list, then
/// for each element its function and the walk of its children.
fn top_down(cx: &Cx, filter: &Filter, mut list: ListMut, level: usize) -> Peek<()> {
    if level > cx.max_depth {
        return Err(depth_error(cx));
    }
    let list_function = match &list {
        ListMut::Inlines(_) => filter.function("Inlines"),
        ListMut::Blocks(_) => filter.function("Blocks"),
    };
    if let Some(function) = list_function
        && !apply_to_list(cx, function, &mut list, level)?
    {
        return Ok(());
    }
    match list {
        ListMut::Inlines(items) => {
            let mut result = Vec::with_capacity(items.len());
            for inline in std::mem::take(items) {
                let (mut new, descend) = match filter.for_inline(&inline) {
                    Some(function) => splice_inline(cx, function, inline)?,
                    None => (vec![inline], true),
                };
                if descend {
                    for inline in &mut new {
                        for child in inline_lists_mut(inline) {
                            top_down(cx, filter, child, level + 1)?;
                        }
                    }
                }
                check_list(cx, ListRef::Inlines(&new), level)?;
                result.extend(new);
            }
            *items = result;
        }
        ListMut::Blocks(items) => {
            let mut result = Vec::with_capacity(items.len());
            for block in std::mem::take(items) {
                let (mut new, descend) = match filter.for_block(&block) {
                    Some(function) => splice_block(cx, function, block)?,
                    None => (vec![block], true),
                };
                if descend {
                    for block in &mut new {
                        for child in block_lists_mut(block) {
                            top_down(cx, filter, child, level + 1)?;
                        }
                    }
                }
                check_list(cx, ListRef::Blocks(&new), level)?;
                result.extend(new);
            }
            *items = result;
        }
    }
    Ok(())
}

/// Walks the lists of a metadata value.
fn walk_meta_value(
    cx: &Cx,
    value: &mut MetaValue,
    level: usize,
    walk: &mut dyn FnMut(ListMut, usize) -> Peek<()>,
) -> Peek<()> {
    if level > cx.max_depth {
        return Err(depth_error(cx));
    }
    match value {
        MetaValue::MetaMap(map) => map
            .values_mut()
            .try_for_each(|item| walk_meta_value(cx, item, level + 1, walk)),
        MetaValue::MetaList(items) => items
            .iter_mut()
            .try_for_each(|item| walk_meta_value(cx, item, level + 1, walk)),
        MetaValue::MetaInlines(items) => walk(ListMut::Inlines(items), level),
        MetaValue::MetaBlocks(items) => walk(ListMut::Blocks(items), level),
        MetaValue::MetaBool(_) | MetaValue::MetaString(_) => Ok(()),
    }
}

/// The lists of a subject and their level.
fn subject_lists<'a>(subject: &'a mut Subject) -> (Vec<ListMut<'a>>, usize) {
    match subject {
        Subject::Inline(inline) => (inline_lists_mut(inline), 2),
        Subject::Block(block) => (block_lists_mut(block), 2),
        Subject::Inlines(items) => (vec![ListMut::Inlines(items)], 1),
        Subject::Blocks(items) => (vec![ListMut::Blocks(items)], 1),
        Subject::Cell(cell) => (vec![ListMut::Blocks(&mut cell.content)], 1),
        Subject::Row(row) => {
            let lists = row
                .cells
                .iter_mut()
                .map(|cell| ListMut::Blocks(&mut cell.content))
                .collect();
            (lists, 1)
        }
    }
}

/// Applies a filter to the inlines and the blocks of a subject, but not to
/// the subject itself.
pub(crate) fn walk_blocks_and_inlines(cx: &Cx, filter: &Filter, mut subject: Subject) -> Peek<()> {
    match filter.order {
        Order::Topdown => {
            let (lists, level) = subject_lists(&mut subject);
            for list in lists {
                top_down(cx, filter, list, level)?;
            }
        }
        Order::Typewise => {
            for pass in PASSES.into_iter().filter(|pass| filter.runs(*pass)) {
                let (lists, level) = subject_lists(&mut subject);
                for list in lists {
                    bottom_up(cx, filter, pass, list, level)?;
                }
            }
        }
    }
    Ok(())
}

/// Applies the function `Meta` of a filter.
fn apply_meta(cx: &Cx, filter: &Filter, doc: &mut Pandoc) -> Peek<()> {
    if let Some(function) = filter.function("Meta") {
        let (result, _) = call(cx, function, push::meta(cx.lua, &doc.meta)?)?;
        if !result.is_nil() {
            let meta = cx.meta(&result)?;
            check_meta(cx, &meta)?;
            doc.meta = meta;
        }
    }
    Ok(())
}

/// Applies the function `Pandoc` of a filter.
fn apply_pandoc(cx: &Cx, filter: &Filter, doc: &mut Pandoc) -> Peek<()> {
    if let Some(function) = filter.function("Pandoc") {
        let (result, _) = call(cx, function, push::pandoc(cx.lua, doc)?)?;
        if !result.is_nil() {
            let new = cx.pandoc(&result)?;
            check_document(cx, &new)?;
            *doc = new;
        }
    }
    Ok(())
}

/// Applies a filter to a whole document: the walk of the metadata and the
/// blocks, and the functions `Meta` and `Pandoc`.
pub(crate) fn apply_fully(cx: &Cx, filter: &Filter, doc: &mut Pandoc) -> Peek<()> {
    match filter.order {
        Order::Topdown => {
            apply_pandoc(cx, filter, doc)?;
            apply_meta(cx, filter, doc)?;
            let mut walk = |list: ListMut, level| top_down(cx, filter, list, level);
            for value in doc.meta.0.values_mut() {
                walk_meta_value(cx, value, 1, &mut walk)?;
            }
            top_down(cx, filter, ListMut::Blocks(&mut doc.blocks), 1)?;
        }
        Order::Typewise => {
            for pass in PASSES.into_iter().filter(|pass| filter.runs(*pass)) {
                let mut walk = |list: ListMut, level| bottom_up(cx, filter, pass, list, level);
                for value in doc.meta.0.values_mut() {
                    walk_meta_value(cx, value, 1, &mut walk)?;
                }
                bottom_up(cx, filter, pass, ListMut::Blocks(&mut doc.blocks), 1)?;
            }
            apply_meta(cx, filter, doc)?;
            apply_pandoc(cx, filter, doc)?;
        }
    }
    Ok(())
}
