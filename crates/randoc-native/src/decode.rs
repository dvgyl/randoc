//! Decodes a [`Value`] into the document tree. Each function returns `None`
//! when the value does not have the shape of the type.

use std::collections::BTreeMap;

use randoc_ast::{
    Attr, Block, Caption, Cell, Citation, ColSpec, ColWidth, Format, Inline, ListAttributes, Meta,
    MetaValue, Pandoc, Row, Table, TableBody, TableFoot, TableHead, Target, Text,
};

use crate::value::{Value, align_from, delim_from, math_from, mode_from, quote_from, style_from};

type Decoded<T> = Option<T>;

/// Returns the arguments when the value is the constructor `name` with `N` arguments.
fn con<'a, const N: usize>(value: &'a Value, name: &str) -> Decoded<&'a [Value; N]> {
    match value {
        Value::Con(found, args) if found == name => args.as_slice().try_into().ok(),
        _ => None,
    }
}

fn nullary(value: &Value) -> Decoded<&str> {
    match value {
        Value::Con(name, args) if args.is_empty() => Some(name),
        _ => None,
    }
}

fn list(value: &Value) -> Decoded<&[Value]> {
    match value {
        Value::List(items) => Some(items),
        _ => None,
    }
}

fn list_of<T>(value: &Value, item: impl Fn(&Value) -> Decoded<T>) -> Decoded<Vec<T>> {
    list(value)?.iter().map(item).collect()
}

fn tuple<const N: usize>(value: &Value) -> Decoded<&[Value; N]> {
    match value {
        Value::Tuple(items) => items.as_slice().try_into().ok(),
        _ => None,
    }
}

fn record<'a, const N: usize>(
    value: &'a Value,
    name: &str,
    fields: [&str; N],
) -> Decoded<[&'a Value; N]> {
    let Value::Rec(found, values) = value else {
        return None;
    };
    if found != name || values.len() != N {
        return None;
    }
    let mut out = [value; N];
    for ((slot, expected), (field, field_value)) in out.iter_mut().zip(fields).zip(values) {
        if field != expected {
            return None;
        }
        *slot = field_value;
    }
    Some(out)
}

fn string(value: &Value) -> Decoded<Text> {
    match value {
        Value::Str(text) => Some(text.clone()),
        Value::List(items) => items
            .iter()
            .map(|item| match item {
                Value::Char(c) => Some(*c),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// Reads an `Int` as `Read Int` does: an integer that is too large wraps.
fn int(value: &Value) -> Decoded<i64> {
    let Value::Num { negative, digits } = value else {
        return None;
    };
    let lower = digits.to_ascii_lowercase();
    let (radix, body) = if let Some(hex) = lower.strip_prefix("0x") {
        (16, hex)
    } else if let Some(octal) = lower.strip_prefix("0o") {
        (8, octal)
    } else {
        (10, lower.as_str())
    };
    let mut result: i64 = 0;
    for c in body.chars() {
        let digit = c.to_digit(radix)?;
        result = result
            .wrapping_mul(i64::from(radix))
            .wrapping_add(i64::from(digit));
    }
    Some(if *negative {
        result.wrapping_neg()
    } else {
        result
    })
}

/// Reads a `Double` as `Read Double` does.
fn double(value: &Value) -> Decoded<f64> {
    let (negative, magnitude) = match value {
        Value::Num { negative, digits } => (*negative, digits.as_str()),
        Value::Con(name, args) if args.is_empty() => (false, name.as_str()),
        _ => return None,
    };
    let number = match magnitude {
        "Infinity" => f64::INFINITY,
        "NaN" => f64::NAN,
        _ if is_radix_literal(magnitude) => radix_double(magnitude)?,
        _ if magnitude.starts_with(|c: char| c.is_ascii_digit()) => magnitude.parse().ok()?,
        _ => return None,
    };
    Some(if negative { -number } else { number })
}

/// Converts a hexadecimal or octal literal to the nearest `Double`, as
/// `fromRational` does. The radix is a power of two, so the value is the first
/// 128 bits, with one sticky bit for the dropped digits, times a power of two.
fn radix_double(literal: &str) -> Decoded<f64> {
    let (radix, body) = match literal.get(..2).map(str::to_ascii_lowercase).as_deref() {
        Some("0x") => (16_u32, literal.get(2..)?),
        Some("0o") => (8, literal.get(2..)?),
        _ => return None,
    };
    if body.is_empty() {
        return None;
    }
    let bits = radix.trailing_zeros();
    let mut top: u128 = 0;
    let mut dropped: i32 = 0;
    let mut sticky = false;
    for c in body.chars() {
        let digit = c.to_digit(radix)?;
        if dropped == 0 && top.leading_zeros() >= bits {
            top = (top << bits) | u128::from(digit);
        } else {
            dropped = dropped.saturating_add(bits as i32);
            sticky |= digit != 0;
        }
    }
    Some((top | u128::from(sticky)) as f64 * 2f64.powi(dropped))
}

fn is_radix_literal(digits: &str) -> bool {
    let lower = digits.get(..2).map(str::to_ascii_lowercase);
    matches!(lower.as_deref(), Some("0x" | "0o"))
}

fn bool_value(value: &Value) -> Decoded<bool> {
    match nullary(value)? {
        "True" => Some(true),
        "False" => Some(false),
        _ => None,
    }
}

fn attr(value: &Value) -> Decoded<Attr> {
    let [id, classes, attributes] = tuple(value)?;
    Some(Attr {
        id: string(id)?,
        classes: list_of(classes, string)?,
        attributes: list_of(attributes, |pair| {
            let [key, value] = tuple(pair)?;
            Some((string(key)?, string(value)?))
        })?,
    })
}

fn target(value: &Value) -> Decoded<Target> {
    let [url, title] = tuple(value)?;
    Some(Target {
        url: string(url)?,
        title: string(title)?,
    })
}

fn format(value: &Value) -> Decoded<Format> {
    let [name] = con(value, "Format")?;
    Some(Format(string(name)?))
}

fn newtype_int(value: &Value, name: &str) -> Decoded<i64> {
    let [inner] = con(value, name)?;
    int(inner)
}

/// Decodes the whole document.
pub(crate) fn pandoc(value: &Value) -> Decoded<Pandoc> {
    let [meta_value, blocks_value] = con(value, "Pandoc")?;
    let [map] = record(meta_value, "Meta", ["unMeta"])?;
    Some(Pandoc {
        meta: Meta(meta_map(map)?),
        blocks: blocks(blocks_value)?,
    })
}

fn meta_map(value: &Value) -> Decoded<BTreeMap<Text, MetaValue>> {
    let [pairs] = con(value, "fromList")?;
    let mut map = BTreeMap::new();
    for pair in list(pairs)? {
        let [key, value] = tuple(pair)?;
        map.insert(string(key)?, meta_value(value)?);
    }
    Some(map)
}

fn meta_value(value: &Value) -> Decoded<MetaValue> {
    let Value::Con(name, args) = value else {
        return None;
    };
    let [arg] = args.as_slice() else {
        return None;
    };
    Some(match name.as_str() {
        "MetaMap" => MetaValue::MetaMap(meta_map(arg)?),
        "MetaList" => MetaValue::MetaList(list_of(arg, meta_value)?),
        "MetaBool" => MetaValue::MetaBool(bool_value(arg)?),
        "MetaString" => MetaValue::MetaString(string(arg)?),
        "MetaInlines" => MetaValue::MetaInlines(inlines(arg)?),
        "MetaBlocks" => MetaValue::MetaBlocks(blocks(arg)?),
        _ => return None,
    })
}

/// Decodes a list of blocks.
pub(crate) fn blocks(value: &Value) -> Decoded<Vec<Block>> {
    list_of(value, block)
}

fn block_lists(value: &Value) -> Decoded<Vec<Vec<Block>>> {
    list_of(value, blocks)
}

/// Decodes one block.
pub(crate) fn block(value: &Value) -> Decoded<Block> {
    let Value::Con(name, args) = value else {
        return None;
    };
    Some(match (name.as_str(), args.as_slice()) {
        ("Plain", [content]) => Block::Plain(inlines(content)?),
        ("Para", [content]) => Block::Para(inlines(content)?),
        ("LineBlock", [lines]) => Block::LineBlock(list_of(lines, inlines)?),
        ("CodeBlock", [a, text]) => Block::CodeBlock(attr(a)?, string(text)?),
        ("RawBlock", [f, text]) => Block::RawBlock(format(f)?, string(text)?),
        ("BlockQuote", [content]) => Block::BlockQuote(blocks(content)?),
        ("OrderedList", [attrs, items]) => {
            let [start, style, delim] = tuple(attrs)?;
            Block::OrderedList(
                ListAttributes {
                    start: int(start)?,
                    style: style_from(nullary(style)?)?,
                    delim: delim_from(nullary(delim)?)?,
                },
                block_lists(items)?,
            )
        }
        ("BulletList", [items]) => Block::BulletList(block_lists(items)?),
        ("DefinitionList", [items]) => Block::DefinitionList(list_of(items, |item| {
            let [term, definitions] = tuple(item)?;
            Some((inlines(term)?, block_lists(definitions)?))
        })?),
        ("Header", [level, a, content]) => Block::Header(int(level)?, attr(a)?, inlines(content)?),
        ("HorizontalRule", []) => Block::HorizontalRule,
        ("Table", [a, cap, specs, head, bodies, foot]) => Block::Table(Box::new(Table {
            attr: attr(a)?,
            caption: caption(cap)?,
            col_specs: list_of(specs, col_spec)?,
            head: table_head(head)?,
            bodies: list_of(bodies, table_body)?,
            foot: table_foot(foot)?,
        })),
        ("Figure", [a, cap, content]) => Block::Figure(attr(a)?, caption(cap)?, blocks(content)?),
        ("Div", [a, content]) => Block::Div(attr(a)?, blocks(content)?),
        _ => return None,
    })
}

fn caption(value: &Value) -> Decoded<Caption> {
    let [short, long] = con(value, "Caption")?;
    let short = match short {
        Value::Con(name, args) if name == "Nothing" && args.is_empty() => None,
        _ => {
            let [content] = con(short, "Just")?;
            Some(inlines(content)?)
        }
    };
    Some(Caption {
        short,
        long: blocks(long)?,
    })
}

fn col_spec(value: &Value) -> Decoded<ColSpec> {
    let [align, width] = tuple(value)?;
    let width = match width {
        Value::Con(name, args) if name == "ColWidthDefault" && args.is_empty() => {
            ColWidth::ColWidthDefault
        }
        _ => {
            let [fraction] = con(width, "ColWidth")?;
            ColWidth::ColWidth(double(fraction)?)
        }
    };
    Some(ColSpec {
        align: align_from(nullary(align)?)?,
        width,
    })
}

fn table_head(value: &Value) -> Decoded<TableHead> {
    let [a, rows] = con(value, "TableHead")?;
    Some(TableHead {
        attr: attr(a)?,
        rows: list_of(rows, row)?,
    })
}

fn table_body(value: &Value) -> Decoded<TableBody> {
    let [a, columns, head, body] = con(value, "TableBody")?;
    Some(TableBody {
        attr: attr(a)?,
        row_head_columns: newtype_int(columns, "RowHeadColumns")?,
        head: list_of(head, row)?,
        body: list_of(body, row)?,
    })
}

fn table_foot(value: &Value) -> Decoded<TableFoot> {
    let [a, rows] = con(value, "TableFoot")?;
    Some(TableFoot {
        attr: attr(a)?,
        rows: list_of(rows, row)?,
    })
}

fn row(value: &Value) -> Decoded<Row> {
    let [a, cells] = con(value, "Row")?;
    Some(Row {
        attr: attr(a)?,
        cells: list_of(cells, cell)?,
    })
}

fn cell(value: &Value) -> Decoded<Cell> {
    let [a, align, rows, columns, content] = con(value, "Cell")?;
    Some(Cell {
        attr: attr(a)?,
        align: align_from(nullary(align)?)?,
        row_span: newtype_int(rows, "RowSpan")?,
        col_span: newtype_int(columns, "ColSpan")?,
        content: blocks(content)?,
    })
}

/// Decodes a list of inlines.
pub(crate) fn inlines(value: &Value) -> Decoded<Vec<Inline>> {
    list_of(value, inline)
}

/// Decodes one inline.
pub(crate) fn inline(value: &Value) -> Decoded<Inline> {
    let Value::Con(name, args) = value else {
        return None;
    };
    Some(match (name.as_str(), args.as_slice()) {
        ("Str", [text]) => Inline::Str(string(text)?),
        ("Emph", [content]) => Inline::Emph(inlines(content)?),
        ("Underline", [content]) => Inline::Underline(inlines(content)?),
        ("Strong", [content]) => Inline::Strong(inlines(content)?),
        ("Strikeout", [content]) => Inline::Strikeout(inlines(content)?),
        ("Superscript", [content]) => Inline::Superscript(inlines(content)?),
        ("Subscript", [content]) => Inline::Subscript(inlines(content)?),
        ("SmallCaps", [content]) => Inline::SmallCaps(inlines(content)?),
        ("Quoted", [kind, content]) => {
            Inline::Quoted(quote_from(nullary(kind)?)?, inlines(content)?)
        }
        ("Cite", [citations, content]) => {
            Inline::Cite(list_of(citations, citation)?, inlines(content)?)
        }
        ("Code", [a, text]) => Inline::Code(attr(a)?, string(text)?),
        ("Space", []) => Inline::Space,
        ("SoftBreak", []) => Inline::SoftBreak,
        ("LineBreak", []) => Inline::LineBreak,
        ("Math", [kind, text]) => Inline::Math(math_from(nullary(kind)?)?, string(text)?),
        ("RawInline", [f, text]) => Inline::RawInline(format(f)?, string(text)?),
        ("Link", [a, content, t]) => Inline::Link(attr(a)?, inlines(content)?, target(t)?),
        ("Image", [a, content, t]) => Inline::Image(attr(a)?, inlines(content)?, target(t)?),
        ("Note", [content]) => Inline::Note(blocks(content)?),
        ("Span", [a, content]) => Inline::Span(attr(a)?, inlines(content)?),
        _ => return None,
    })
}

fn citation(value: &Value) -> Decoded<Citation> {
    let [id, prefix, suffix, mode, note_num, hash] = record(
        value,
        "Citation",
        [
            "citationId",
            "citationPrefix",
            "citationSuffix",
            "citationMode",
            "citationNoteNum",
            "citationHash",
        ],
    )?;
    Some(Citation {
        id: string(id)?,
        prefix: inlines(prefix)?,
        suffix: inlines(suffix)?,
        mode: mode_from(nullary(mode)?)?,
        note_num: int(note_num)?,
        hash: int(hash)?,
    })
}
