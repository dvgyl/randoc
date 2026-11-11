//! Conversion of values of the document model to Lua values.
//!
//! An element becomes a userdata value that holds a copy of the element. A
//! list becomes a Lua table with the metatable `List`, `Inlines` or
//! `Blocks`. Metadata becomes plain Lua values, as in `pandoc-lua-marshal`.

use mlua::{IntoLua, Lua, Table, Value};
use randoc_ast::{
    Alignment, Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, ColWidth, Inline,
    ListAttributes, ListNumberDelim, ListNumberStyle, MathType, Meta, MetaValue, Pandoc, QuoteType,
    Row, TableBody, TableFoot, TableHead,
};

use crate::object::{Node, new_object};

/// The prefix of the registry names of the list metatables.
pub(crate) const METATABLE_PREFIX: &str = "randoc.metatable.";

/// Returns the metatable with the given `__name` that the prelude made.
pub(crate) fn metatable(lua: &Lua, name: &str) -> mlua::Result<Table> {
    lua.named_registry_value(&format!("{METATABLE_PREFIX}{name}"))
}

/// Makes a list table with the metatable `name` from Lua values.
pub(crate) fn list_of(lua: &Lua, name: &str, items: Vec<Value>) -> mlua::Result<Value> {
    let table = lua.create_sequence_from(items)?;
    table.set_metatable(Some(metatable(lua, name)?))?;
    Ok(Value::Table(table))
}

/// Makes a `List` from the items of a slice.
pub(crate) fn list<T>(
    lua: &Lua,
    items: &[T],
    push: impl Fn(&Lua, &T) -> mlua::Result<Value>,
) -> mlua::Result<Value> {
    let values = items
        .iter()
        .map(|item| push(lua, item))
        .collect::<mlua::Result<Vec<Value>>>()?;
    list_of(lua, "List", values)
}

/// Makes a Lua string.
pub(crate) fn text(lua: &Lua, text: &str) -> mlua::Result<Value> {
    lua.create_string(text).map(Value::String)
}

/// Makes a plain table of two items.
fn pair(lua: &Lua, first: Value, second: Value) -> mlua::Result<Value> {
    lua.create_sequence_from([first, second]).map(Value::Table)
}

/// Makes an `Inline` object.
pub(crate) fn inline(lua: &Lua, inline: &Inline) -> mlua::Result<Value> {
    new_object(lua, Node::Inline(inline.clone()))
}

/// Makes a `Block` object.
pub(crate) fn block(lua: &Lua, block: &Block) -> mlua::Result<Value> {
    new_object(lua, Node::Block(block.clone()))
}

/// Makes an `Inlines` list.
pub(crate) fn inlines(lua: &Lua, items: &[Inline]) -> mlua::Result<Value> {
    let values = items
        .iter()
        .map(|item| inline(lua, item))
        .collect::<mlua::Result<Vec<Value>>>()?;
    list_of(lua, "Inlines", values)
}

/// Makes a `Blocks` list.
pub(crate) fn blocks(lua: &Lua, items: &[Block]) -> mlua::Result<Value> {
    let values = items
        .iter()
        .map(|item| block(lua, item))
        .collect::<mlua::Result<Vec<Value>>>()?;
    list_of(lua, "Blocks", values)
}

/// Makes an `Attr` object.
pub(crate) fn attr(lua: &Lua, attr: &Attr) -> mlua::Result<Value> {
    new_object(lua, Node::Attr(attr.clone()))
}

/// Makes a `Citation` object.
pub(crate) fn citation(lua: &Lua, citation: &Citation) -> mlua::Result<Value> {
    new_object(lua, Node::Citation(citation.clone()))
}

/// Makes a `ListAttributes` object.
pub(crate) fn list_attributes(lua: &Lua, attributes: &ListAttributes) -> mlua::Result<Value> {
    new_object(lua, Node::ListAttributes(*attributes))
}

/// Makes a `Caption` object.
pub(crate) fn caption(lua: &Lua, caption: &Caption) -> mlua::Result<Value> {
    new_object(lua, Node::Caption(caption.clone()))
}

/// Makes a `Cell` object.
pub(crate) fn cell(lua: &Lua, cell: &Cell) -> mlua::Result<Value> {
    new_object(lua, Node::Cell(cell.clone()))
}

/// Makes a `Row` object.
pub(crate) fn row(lua: &Lua, row: &Row) -> mlua::Result<Value> {
    new_object(lua, Node::Row(row.clone()))
}

/// Makes a `TableHead` object.
pub(crate) fn table_head(lua: &Lua, head: &TableHead) -> mlua::Result<Value> {
    new_object(lua, Node::TableHead(head.clone()))
}

/// Makes a `TableFoot` object.
pub(crate) fn table_foot(lua: &Lua, foot: &TableFoot) -> mlua::Result<Value> {
    new_object(lua, Node::TableFoot(foot.clone()))
}

/// Makes a `TableBody` object.
pub(crate) fn table_body(lua: &Lua, body: &TableBody) -> mlua::Result<Value> {
    new_object(lua, Node::TableBody(body.clone()))
}

/// Makes a `Pandoc` object.
pub(crate) fn pandoc(lua: &Lua, doc: &Pandoc) -> mlua::Result<Value> {
    new_object(lua, Node::Pandoc(doc.clone()))
}

/// Makes the pair of an alignment and a width. The default width is `nil`.
pub(crate) fn col_spec(lua: &Lua, spec: &ColSpec) -> mlua::Result<Value> {
    let width = match spec.width {
        ColWidth::ColWidth(width) => Value::Number(width),
        ColWidth::ColWidthDefault => Value::Nil,
    };
    pair(lua, text(lua, alignment_name(spec.align))?, width)
}

/// Makes the pair of a term and its definitions.
pub(crate) fn definition_item(
    lua: &Lua,
    item: &(Vec<Inline>, Vec<Vec<Block>>),
) -> mlua::Result<Value> {
    let definitions = list(lua, &item.1, |lua, definition| blocks(lua, definition))?;
    pair(lua, inlines(lua, &item.0)?, definitions)
}

/// Makes the Lua value of a metadata value.
pub(crate) fn meta_value(lua: &Lua, value: &MetaValue) -> mlua::Result<Value> {
    match value {
        MetaValue::MetaBlocks(items) => blocks(lua, items),
        MetaValue::MetaBool(flag) => Ok(Value::Boolean(*flag)),
        MetaValue::MetaInlines(items) => inlines(lua, items),
        MetaValue::MetaList(items) => list(lua, items, meta_value),
        MetaValue::MetaMap(map) => {
            let table = lua.create_table()?;
            for (key, item) in map {
                table.raw_set(key.as_str(), meta_value(lua, item)?)?;
            }
            Ok(Value::Table(table))
        }
        MetaValue::MetaString(string) => text(lua, string),
    }
}

/// Makes the table of the document metadata, with the metatable `Meta`.
pub(crate) fn meta(lua: &Lua, meta: &Meta) -> mlua::Result<Value> {
    let table = lua.create_table()?;
    for (key, item) in &meta.0 {
        table.raw_set(key.as_str(), meta_value(lua, item)?)?;
    }
    table.set_metatable(Some(metatable(lua, "Meta")?))?;
    Ok(Value::Table(table))
}

/// Makes a Lua integer.
pub(crate) fn integer(lua: &Lua, number: i64) -> mlua::Result<Value> {
    number.into_lua(lua)
}

/// Returns the constructor name of an alignment.
pub(crate) fn alignment_name(alignment: Alignment) -> &'static str {
    match alignment {
        Alignment::AlignLeft => "AlignLeft",
        Alignment::AlignRight => "AlignRight",
        Alignment::AlignCenter => "AlignCenter",
        Alignment::AlignDefault => "AlignDefault",
    }
}

/// Returns the constructor name of a citation mode.
pub(crate) fn citation_mode_name(mode: CitationMode) -> &'static str {
    match mode {
        CitationMode::AuthorInText => "AuthorInText",
        CitationMode::SuppressAuthor => "SuppressAuthor",
        CitationMode::NormalCitation => "NormalCitation",
    }
}

/// Returns the constructor name of a math type.
pub(crate) fn math_type_name(math_type: MathType) -> &'static str {
    match math_type {
        MathType::DisplayMath => "DisplayMath",
        MathType::InlineMath => "InlineMath",
    }
}

/// Returns the constructor name of a quote type.
pub(crate) fn quote_type_name(quote_type: QuoteType) -> &'static str {
    match quote_type {
        QuoteType::SingleQuote => "SingleQuote",
        QuoteType::DoubleQuote => "DoubleQuote",
    }
}

/// Returns the constructor name of a number style.
pub(crate) fn number_style_name(style: ListNumberStyle) -> &'static str {
    match style {
        ListNumberStyle::DefaultStyle => "DefaultStyle",
        ListNumberStyle::Example => "Example",
        ListNumberStyle::Decimal => "Decimal",
        ListNumberStyle::LowerRoman => "LowerRoman",
        ListNumberStyle::UpperRoman => "UpperRoman",
        ListNumberStyle::LowerAlpha => "LowerAlpha",
        ListNumberStyle::UpperAlpha => "UpperAlpha",
    }
}

/// Returns the constructor name of a number delimiter.
pub(crate) fn number_delim_name(delim: ListNumberDelim) -> &'static str {
    match delim {
        ListNumberDelim::DefaultDelim => "DefaultDelim",
        ListNumberDelim::Period => "Period",
        ListNumberDelim::OneParen => "OneParen",
        ListNumberDelim::TwoParens => "TwoParens",
    }
}
