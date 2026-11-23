use std::collections::BTreeMap;

use randoc_archive::xml::{XmlElement, XmlLimits, XmlNode, XmlOptions, parse_fragment};
use randoc_ast::*;
use randoc_core::{Context, Error, ReaderOptions, Result, Sources};

#[derive(Clone, Copy)]
enum Kind {
    Root,
    Block,
    Inline,
    Literal,
    Meta,
    Entry,
    Structure(&'static [&'static str]),
    Caption,
    Figure,
    Item,
    Cite,
}

enum Value {
    Block(Block),
    Inline(Inline),
    Text(String),
    Meta(MetaValue),
    Entry(String, MetaValue),
    Part(String, Vec<Value>),
    Caption(Caption),
    Citation(Citation),
    Column(ColSpec),
    Row(Row),
    Cell(Cell),
    TableBody(TableBody),
}

enum Task<'a> {
    Visit(&'a XmlNode, Kind, usize),
    Finish(&'a XmlElement, usize),
}

pub(crate) fn read(sources: &Sources, _: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    let maximum = context.limits.max_input_size;
    let size = sources.0.iter().try_fold(0u64, |total, source| {
        let length = source.text.len() as u64;
        total.checked_add(length).filter(|&sum| sum <= maximum)
    });
    if size.is_none() {
        return Err(error("The input exceeds the size limit."));
    }
    let depth = context.limits.max_nesting_depth;
    let mut limits = XmlLimits::from_limits(&context.limits);
    // XML containers such as list items and table rows add markup levels.
    limits.max_depth = depth.saturating_mul(6).saturating_add(3);
    let mut doc = Pandoc::default();
    let mut count_budget = limits.max_nodes;
    for source in &sources.0 {
        let fragment = parse_fragment(&source.text, &limits, XmlOptions::default())
            .map_err(|cause| error(cause.format_pandoc()))?;
        let result = decode(&fragment.nodes, depth, &mut count_budget);
        drop_nodes(fragment.nodes);
        for value in result? {
            match value {
                Value::Block(block) => doc.blocks.push(block),
                Value::Part(name, values) if name == "Pandoc" => {
                    for value in values {
                        match value {
                            Value::Part(name, entries) if name == "meta" => {
                                doc.meta.0.extend(entries_map(entries)?)
                            }
                            Value::Part(name, items) if name == "blocks" => {
                                doc.blocks.extend(blocks(items)?)
                            }
                            _ => return Err(error("unexpected content in Pandoc")),
                        }
                    }
                }
                _ => return Err(error("unexpected content in blocks context")),
            }
        }
    }
    Ok(doc)
}

fn drop_nodes(mut nodes: Vec<XmlNode>) {
    while let Some(node) = nodes.pop() {
        if let XmlNode::Element(mut element) = node {
            nodes.append(&mut element.children);
        }
    }
}

fn error(message: impl AsRef<str>) -> Error {
    Error::Xml {
        file: String::new(),
        message: message.as_ref().into(),
    }
}

fn decode(nodes: &[XmlNode], max_depth: usize, budget: &mut usize) -> Result<Vec<Value>> {
    let mut tasks: Vec<_> = nodes
        .iter()
        .rev()
        .map(|node| Task::Visit(node, Kind::Root, 0))
        .collect();
    let mut values = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Visit(XmlNode::Text(text), kind, depth) => match kind {
                Kind::Literal => values.push(Value::Text(text.clone())),
                Kind::Inline | Kind::Cite => {
                    if !text.is_empty() && depth >= max_depth {
                        return Err(error("the document is nested too deeply"));
                    }
                    tokenize(text, &mut values, budget)?;
                }
                _ if text.trim().is_empty() => {}
                _ => return Err(error("non-space characters out of inline context")),
            },
            Task::Visit(XmlNode::Element(element), kind, depth) => {
                let name = element.name.local.as_str();
                let increment = matches!(
                    category(name),
                    Some(Kind::Block | Kind::Inline | Kind::Meta)
                );
                let depth = depth + usize::from(increment);
                if depth > max_depth {
                    return Err(error("the document is nested too deeply"));
                }
                let children = child_kind(kind, name)?;
                if name == "Str" {
                    values.extend(build(element, Vec::new(), budget)?);
                    continue;
                }
                tasks.push(Task::Finish(element, values.len()));
                tasks.extend(
                    element
                        .children
                        .iter()
                        .rev()
                        .map(|node| Task::Visit(node, children, depth)),
                );
            }
            Task::Finish(element, index) => {
                let children = values.split_off(index);
                values.extend(build(element, children, budget)?);
            }
        }
    }
    Ok(values)
}

fn category(name: &str) -> Option<Kind> {
    Some(match name {
        "Plain" | "Para" | "Header" | "LineBlock" | "CodeBlock" | "RawBlock" | "BlockQuote"
        | "Div" | "BulletList" | "OrderedList" | "DefinitionList" | "HorizontalRule" | "Figure"
        | "Table" => Kind::Block,
        "Emph" | "Strong" | "Underline" | "Strikeout" | "Superscript" | "Subscript"
        | "SmallCaps" | "Quoted" | "Cite" | "Code" | "Math" | "RawInline" | "Link" | "Image"
        | "Span" | "Note" | "Str" | "Space" | "SoftBreak" | "LineBreak" => Kind::Inline,
        "MetaMap" | "MetaList" | "MetaBool" | "MetaString" | "MetaInlines" | "MetaBlocks" => {
            Kind::Meta
        }
        "entry" => Kind::Entry,
        "Pandoc" => Kind::Root,
        "Caption" => Kind::Caption,
        "citations" => Kind::Cite,
        "meta" | "blocks" | "item" | "def" | "term" | "line" | "ShortCaption" | "colspecs"
        | "ColSpec" | "TableHead" | "TableBody" | "TableFoot" | "header" | "body" | "Row"
        | "Cell" | "Citation" | "prefix" | "suffix" => Kind::Structure(&[]),
        _ => return None,
    })
}

fn child_kind(parent: Kind, name: &str) -> Result<Kind> {
    let allowed = match parent {
        Kind::Structure(names) => names.contains(&name),
        Kind::Block => matches!(category(name), Some(Kind::Block)),
        Kind::Inline => matches!(category(name), Some(Kind::Inline)),
        Kind::Meta => matches!(category(name), Some(Kind::Meta)),
        Kind::Entry => name == "entry",
        Kind::Root => matches!(category(name), Some(Kind::Block | Kind::Root)),
        Kind::Caption => name == "ShortCaption" || matches!(category(name), Some(Kind::Block)),
        Kind::Figure => name == "Caption" || matches!(category(name), Some(Kind::Block)),
        Kind::Item => matches!(name, "term" | "def") || matches!(category(name), Some(Kind::Block)),
        Kind::Cite => name == "citations" || matches!(category(name), Some(Kind::Inline)),
        Kind::Literal => false,
    };
    if !allowed {
        let label = match parent {
            Kind::Inline | Kind::Cite => "inline context",
            Kind::Meta | Kind::Entry => "metadata",
            Kind::Literal => "literal text",
            _ => "blocks context",
        };
        return Err(error(format!("unexpected element {name:?} in {label}")));
    }
    Ok(match name {
        "Plain" | "Para" | "Header" | "line" | "term" | "ShortCaption" | "Emph" | "Strong"
        | "Underline" | "Strikeout" | "Superscript" | "Subscript" | "SmallCaps" | "Quoted"
        | "Link" | "Image" | "Span" | "MetaInlines" | "prefix" | "suffix" => Kind::Inline,
        "CodeBlock" | "RawBlock" | "Code" | "Math" | "RawInline" | "MetaString" | "Str" => {
            Kind::Literal
        }
        "BlockQuote" | "Div" | "def" | "Cell" | "blocks" | "Note" | "MetaBlocks" => Kind::Block,
        "entry" | "MetaList" => Kind::Meta,
        "meta" | "MetaMap" => Kind::Entry,
        "Caption" => Kind::Caption,
        "Figure" => Kind::Figure,
        "Pandoc" => Kind::Structure(&["meta", "blocks"]),
        "LineBlock" => Kind::Structure(&["line"]),
        "OrderedList" | "BulletList" | "DefinitionList" => Kind::Structure(&["item"]),
        "item" => Kind::Item,
        "Table" => Kind::Structure(&["Caption", "colspecs", "TableHead", "TableBody", "TableFoot"]),
        "colspecs" => Kind::Structure(&["ColSpec"]),
        "TableHead" | "TableFoot" | "header" | "body" => Kind::Structure(&["Row"]),
        "TableBody" => Kind::Structure(&["header", "body"]),
        "Row" => Kind::Structure(&["Cell"]),
        "citations" => Kind::Structure(&["Citation"]),
        "Citation" => Kind::Structure(&["prefix", "suffix"]),
        "Cite" => Kind::Cite,
        _ => Kind::Structure(&[]),
    })
}

fn charge(budget: &mut usize, count: usize) -> Result<()> {
    *budget = budget
        .checked_sub(count)
        .ok_or_else(|| error("The XML node limit was exceeded."))?;
    Ok(())
}

fn inline_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

fn tokenize(text: &str, values: &mut Vec<Value>, budget: &mut usize) -> Result<()> {
    let mut start = 0;
    let mut whitespace = text.chars().next().is_some_and(inline_whitespace);
    let mut newline = false;
    for (i, c) in text.char_indices() {
        if inline_whitespace(c) != whitespace {
            charge(budget, 1)?;
            values.push(Value::Inline(token(&text[start..i], whitespace, newline)));
            start = i;
            whitespace = !whitespace;
            newline = false;
        }
        newline |= matches!(c, '\r' | '\n');
    }
    if start < text.len() {
        charge(budget, 1)?;
        values.push(Value::Inline(token(&text[start..], whitespace, newline)));
    }
    Ok(())
}

fn token(text: &str, whitespace: bool, newline: bool) -> Inline {
    match (whitespace, newline) {
        (true, true) => Inline::SoftBreak,
        (true, false) => Inline::Space,
        _ => Inline::Str(text.into()),
    }
}

fn attrs(element: &XmlElement, fields: &[&str]) -> Attr {
    Attr::new(
        get(element, "id"),
        get(element, "class")
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
        element
            .attrs
            .iter()
            .filter(|(name, _)| {
                !matches!(name.local.as_str(), "id" | "class")
                    && !fields.contains(&name.local.as_str())
            })
            .map(|(name, value)| {
                (
                    name.prefix
                        .as_ref()
                        .map_or_else(|| name.local.clone(), |p| format!("{p}:{}", name.local)),
                    value.clone(),
                )
            })
            .collect(),
    )
}

fn get<'a>(element: &'a XmlElement, key: &str) -> &'a str {
    element.attr_unqualified(key).unwrap_or_default()
}

fn number(element: &XmlElement, key: &str, default: i64) -> i64 {
    get(element, key).parse().unwrap_or(default)
}

fn align(element: &XmlElement) -> Alignment {
    match get(element, "alignment") {
        "AlignLeft" => Alignment::AlignLeft,
        "AlignRight" => Alignment::AlignRight,
        "AlignCenter" => Alignment::AlignCenter,
        _ => Alignment::AlignDefault,
    }
}

fn list_attrs(element: &XmlElement) -> ListAttributes {
    ListAttributes {
        start: number(element, "start", 1),
        style: match get(element, "number-style") {
            "Example" => ListNumberStyle::Example,
            "Decimal" => ListNumberStyle::Decimal,
            "LowerRoman" => ListNumberStyle::LowerRoman,
            "UpperRoman" => ListNumberStyle::UpperRoman,
            "LowerAlpha" => ListNumberStyle::LowerAlpha,
            "UpperAlpha" => ListNumberStyle::UpperAlpha,
            _ => ListNumberStyle::DefaultStyle,
        },
        delim: match get(element, "number-delim") {
            "Period" => ListNumberDelim::Period,
            "OneParen" => ListNumberDelim::OneParen,
            "TwoParens" => ListNumberDelim::TwoParens,
            _ => ListNumberDelim::DefaultDelim,
        },
    }
}

fn collect<T>(
    values: Vec<Value>,
    kind: &str,
    extract: impl Fn(Value) -> Option<T>,
) -> Result<Vec<T>> {
    values
        .into_iter()
        .map(|value| extract(value).ok_or_else(|| error(format!("expected {kind}"))))
        .collect()
}

fn blocks(values: Vec<Value>) -> Result<Vec<Block>> {
    collect(values, "blocks", |value| {
        if let Value::Block(block) = value {
            Some(block)
        } else {
            None
        }
    })
}

fn inlines(values: Vec<Value>) -> Result<Vec<Inline>> {
    collect(values, "inlines", |value| {
        if let Value::Inline(inline) = value {
            Some(inline)
        } else {
            None
        }
    })
}

fn text(values: Vec<Value>) -> Result<String> {
    Ok(collect(values, "text", |value| {
        if let Value::Text(text) = value {
            Some(text)
        } else {
            None
        }
    })?
    .concat())
}

fn entries_map(values: Vec<Value>) -> Result<BTreeMap<String, MetaValue>> {
    Ok(collect(values, "metadata entries", |value| {
        if let Value::Entry(key, meta) = value {
            Some((key, meta))
        } else {
            None
        }
    })?
    .into_iter()
    .collect())
}

fn part(value: Value, name: &str) -> Result<Vec<Value>> {
    match value {
        Value::Part(tag, children) if tag == name => Ok(children),
        _ => Err(error(format!("expected {name}"))),
    }
}

fn rows(values: Vec<Value>) -> Result<Vec<Row>> {
    collect(values, "rows", |value| {
        if let Value::Row(row) = value {
            Some(row)
        } else {
            None
        }
    })
}

fn build(element: &XmlElement, mut children: Vec<Value>, budget: &mut usize) -> Result<Vec<Value>> {
    let name = element.name.local.as_str();
    let raw = || Format::new(get(element, "format"));
    charge(budget, 1)?;
    let value = match name {
        "Plain" => Value::Block(Block::Plain(inlines(children)?)),
        "Para" => Value::Block(Block::Para(inlines(children)?)),
        "Header" => Value::Block(Block::Header(
            number(element, "level", 1),
            attrs(element, &["level"]),
            inlines(children)?,
        )),
        "CodeBlock" => Value::Block(Block::CodeBlock(attrs(element, &[]), text(children)?)),
        "RawBlock" => Value::Block(Block::RawBlock(raw(), text(children)?)),
        "Div" => Value::Block(Block::Div(attrs(element, &[]), blocks(children)?)),
        "BlockQuote" => Value::Block(Block::BlockQuote(blocks(children)?)),
        "HorizontalRule" => Value::Block(Block::HorizontalRule),
        "LineBlock" => Value::Block(Block::LineBlock(
            children
                .into_iter()
                .map(|value| inlines(part(value, "line")?))
                .collect::<Result<_>>()?,
        )),
        "BulletList" | "OrderedList" => {
            let items = children
                .into_iter()
                .map(|value| blocks(part(value, "item")?))
                .collect::<Result<_>>()?;
            Value::Block(if name == "BulletList" {
                Block::BulletList(items)
            } else {
                Block::OrderedList(list_attrs(element), items)
            })
        }
        "DefinitionList" => {
            let items = children
                .into_iter()
                .map(|value| {
                    let mut item = part(value, "item")?.into_iter();
                    let term = item
                        .next()
                        .map_or_else(|| Ok(Vec::new()), |value| inlines(part(value, "term")?))?;
                    let defs = item
                        .map(|value| blocks(part(value, "def")?))
                        .collect::<Result<_>>()?;
                    Ok((term, defs))
                })
                .collect::<Result<_>>()?;
            Value::Block(Block::DefinitionList(items))
        }
        "Figure" => {
            let caption = if matches!(children.first(), Some(Value::Caption(_))) {
                match children.remove(0) {
                    Value::Caption(caption) => caption,
                    _ => return Err(error("expected a caption")),
                }
            } else {
                Caption::default()
            };
            Value::Block(Block::Figure(
                attrs(element, &[]),
                caption,
                blocks(children)?,
            ))
        }
        "Table" => Value::Block(Block::Table(Box::new(table(element, children)?))),
        "Caption" => {
            let short = if matches!(children.first(), Some(Value::Part(name, _)) if name == "ShortCaption")
            {
                Some(inlines(part(children.remove(0), "ShortCaption")?)?)
            } else {
                None
            };
            Value::Caption(Caption {
                short,
                long: blocks(children)?,
            })
        }
        "ColSpec" => Value::Column(ColSpec {
            align: align(element),
            width: match get(element, "col-width").parse::<f64>() {
                Ok(width) if width != 0.0 && width.is_finite() => ColWidth::ColWidth(width),
                _ => ColWidth::ColWidthDefault,
            },
        }),
        "Row" => Value::Row(Row {
            attr: attrs(element, &[]),
            cells: collect(children, "cells", |value| {
                if let Value::Cell(cell) = value {
                    Some(cell)
                } else {
                    None
                }
            })?,
        }),
        "Cell" => Value::Cell(Cell {
            attr: attrs(element, &["alignment", "row-span", "col-span"]),
            align: align(element),
            row_span: number(element, "row-span", 1),
            col_span: number(element, "col-span", 1),
            content: blocks(children)?,
        }),
        "TableBody" => {
            let mut head = Vec::new();
            let mut body = Vec::new();
            for value in children {
                match value {
                    Value::Part(name, items) if name == "header" => head = rows(items)?,
                    Value::Part(name, items) if name == "body" => body = rows(items)?,
                    _ => return Err(error("expected table body rows")),
                }
            }
            Value::TableBody(TableBody {
                attr: attrs(element, &["row-head-columns"]),
                row_head_columns: number(element, "row-head-columns", 0),
                head,
                body,
            })
        }
        "TableHead" | "TableFoot" => {
            let attr = attrs(element, &[]);
            Value::Part(
                name.into(),
                vec![Value::TableBody(TableBody {
                    attr,
                    body: rows(children)?,
                    ..TableBody::default()
                })],
            )
        }
        "MetaMap" => Value::Meta(MetaValue::MetaMap(entries_map(children)?)),
        "MetaList" => Value::Meta(MetaValue::MetaList(collect(
            children,
            "metadata",
            |value| {
                if let Value::Meta(meta) = value {
                    Some(meta)
                } else {
                    None
                }
            },
        )?)),
        "MetaString" => Value::Meta(MetaValue::MetaString(text(children)?)),
        "MetaBool" => Value::Meta(MetaValue::MetaBool(get(element, "value") == "true")),
        "MetaBlocks" => Value::Meta(MetaValue::MetaBlocks(blocks(children)?)),
        "MetaInlines" => Value::Meta(MetaValue::MetaInlines(inlines(children)?)),
        "entry" => match children.pop() {
            Some(Value::Meta(meta)) if children.is_empty() => {
                Value::Entry(get(element, "key").into(), meta)
            }
            _ => return Err(error("expected one metadata value")),
        },
        "Citation" => Value::Citation(citation(element, children)?),
        "Space" | "SoftBreak" | "LineBreak" => {
            let inline = match name {
                "Space" => Inline::Space,
                "SoftBreak" => Inline::SoftBreak,
                _ => Inline::LineBreak,
            };
            let count = if name == "Space" {
                usize::try_from(number(element, "count", 1)).unwrap_or_default()
            } else {
                1
            };
            charge(budget, count)?;
            return Ok((0..count).map(|_| Value::Inline(inline.clone())).collect());
        }
        "Str" => Value::Inline(Inline::Str(get(element, "content").into())),
        "Emph" => Value::Inline(Inline::Emph(inlines(children)?)),
        "Strong" => Value::Inline(Inline::Strong(inlines(children)?)),
        "Underline" => Value::Inline(Inline::Underline(inlines(children)?)),
        "Strikeout" => Value::Inline(Inline::Strikeout(inlines(children)?)),
        "Superscript" => Value::Inline(Inline::Superscript(inlines(children)?)),
        "Subscript" => Value::Inline(Inline::Subscript(inlines(children)?)),
        "SmallCaps" => Value::Inline(Inline::SmallCaps(inlines(children)?)),
        "Quoted" => Value::Inline(Inline::Quoted(
            if get(element, "quote-type") == "SingleQuote" {
                QuoteType::SingleQuote
            } else {
                QuoteType::DoubleQuote
            },
            inlines(children)?,
        )),
        "Code" => Value::Inline(Inline::Code(attrs(element, &[]), text(children)?)),
        "Math" => Value::Inline(Inline::Math(
            if get(element, "math-type") == "DisplayMath" {
                MathType::DisplayMath
            } else {
                MathType::InlineMath
            },
            text(children)?,
        )),
        "RawInline" => Value::Inline(Inline::RawInline(raw(), text(children)?)),
        "Link" | "Image" => {
            let key = if name == "Link" { "href" } else { "src" };
            let attr = attrs(element, &[key, "title"]);
            let target = Target::new(get(element, key), get(element, "title"));
            let label = inlines(children)?;
            Value::Inline(if name == "Link" {
                Inline::Link(attr, label, target)
            } else {
                Inline::Image(attr, label, target)
            })
        }
        "Note" => Value::Inline(Inline::Note(blocks(children)?)),
        "Span" => Value::Inline(Inline::Span(attrs(element, &[]), inlines(children)?)),
        "Cite" => {
            let cites = if matches!(children.first(), Some(Value::Part(tag, _)) if tag == "citations")
            {
                collect(
                    part(children.remove(0), "citations")?,
                    "citations",
                    |value| {
                        if let Value::Citation(cite) = value {
                            Some(cite)
                        } else {
                            None
                        }
                    },
                )?
            } else {
                Vec::new()
            };
            Value::Inline(Inline::Cite(cites, inlines(children)?))
        }
        _ => Value::Part(name.into(), children),
    };
    Ok(vec![value])
}

fn citation(element: &XmlElement, children: Vec<Value>) -> Result<Citation> {
    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    for value in children {
        match value {
            Value::Part(name, items) if name == "prefix" => prefix = inlines(items)?,
            Value::Part(name, items) if name == "suffix" => suffix = inlines(items)?,
            _ => return Err(error("expected a citation prefix or suffix")),
        }
    }
    Ok(Citation {
        id: get(element, "id").into(),
        mode: match get(element, "mode") {
            "AuthorInText" => CitationMode::AuthorInText,
            "SuppressAuthor" => CitationMode::SuppressAuthor,
            _ => CitationMode::NormalCitation,
        },
        note_num: number(element, "note-num", 0),
        hash: number(element, "hash", 0),
        prefix,
        suffix,
    })
}

fn table(element: &XmlElement, children: Vec<Value>) -> Result<Table> {
    let mut table = Table {
        attr: attrs(element, &[]),
        ..Table::default()
    };
    for value in children {
        match value {
            Value::Caption(caption) => table.caption = caption,
            Value::Part(name, items) if name == "colspecs" => {
                table.col_specs = collect(items, "column specifications", |value| {
                    if let Value::Column(col) = value {
                        Some(col)
                    } else {
                        None
                    }
                })?;
            }
            Value::Part(name, mut items) if matches!(name.as_str(), "TableHead" | "TableFoot") => {
                let Some(Value::TableBody(section)) = items.pop() else {
                    return Err(error("expected table rows"));
                };
                if name == "TableHead" {
                    table.head = TableHead {
                        attr: section.attr,
                        rows: section.body,
                    };
                } else {
                    table.foot = TableFoot {
                        attr: section.attr,
                        rows: section.body,
                    };
                }
            }
            Value::TableBody(body) => table.bodies.push(body),
            _ => return Err(error("unexpected content in Table")),
        }
    }
    Ok(table)
}
