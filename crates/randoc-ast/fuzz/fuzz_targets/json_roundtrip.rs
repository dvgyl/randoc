#![no_main]
//! Builds an arbitrary document, writes it with `to_json` and reads it with
//! `from_json`. The result must be the same document, except that `-0.0`
//! reads as `0.0` and NaN is not equal to itself, as in Pandoc.

use arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;
use randoc_ast::{
    Alignment, Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, ColWidth, Format,
    Inline, ListAttributes, ListNumberDelim, ListNumberStyle, MathType, Meta, MetaValue, Pandoc,
    QuoteType, Row, Table, TableBody, TableFoot, TableHead, Target, json,
};

const MAX_LEVELS: u32 = 6;

fn text(u: &mut Unstructured) -> Result<String> {
    u.arbitrary()
}

fn list<T>(u: &mut Unstructured, f: impl Fn(&mut Unstructured) -> Result<T>) -> Result<Vec<T>> {
    let len = u.int_in_range(0..=3)?;
    (0..len).map(|_| f(u)).collect()
}

fn attr(u: &mut Unstructured) -> Result<Attr> {
    Ok(Attr::new(text(u)?, list(u, text)?, list(u, |u| Ok((text(u)?, text(u)?)))?))
}

fn width(u: &mut Unstructured) -> Result<f64> {
    let value: f64 = u.arbitrary()?;
    Ok(if value == 0.0 { 0.0 } else { value })
}

fn inline(u: &mut Unstructured, level: u32) -> Result<Inline> {
    if level == 0 {
        return Ok(Inline::Str(text(u)?));
    }
    let l = level - 1;
    let inlines = |u: &mut Unstructured| list(u, |u| inline(u, l));
    let blocks = |u: &mut Unstructured| list(u, |u| block(u, l));
    Ok(match u.int_in_range(0..=19)? {
        0 => Inline::Str(text(u)?),
        1 => Inline::Emph(inlines(u)?),
        2 => Inline::Underline(inlines(u)?),
        3 => Inline::Strong(inlines(u)?),
        4 => Inline::Strikeout(inlines(u)?),
        5 => Inline::Superscript(inlines(u)?),
        6 => Inline::Subscript(inlines(u)?),
        7 => Inline::SmallCaps(inlines(u)?),
        8 => Inline::Quoted(
            *u.choose(&[QuoteType::SingleQuote, QuoteType::DoubleQuote])?,
            inlines(u)?,
        ),
        9 => Inline::Cite(
            list(u, |u| {
                Ok(Citation {
                    id: text(u)?,
                    prefix: inlines(u)?,
                    suffix: inlines(u)?,
                    mode: *u.choose(&[
                        CitationMode::AuthorInText,
                        CitationMode::SuppressAuthor,
                        CitationMode::NormalCitation,
                    ])?,
                    note_num: u.arbitrary()?,
                    hash: u.arbitrary()?,
                })
            })?,
            inlines(u)?,
        ),
        10 => Inline::Code(attr(u)?, text(u)?),
        11 => Inline::Space,
        12 => Inline::SoftBreak,
        13 => Inline::LineBreak,
        14 => Inline::Math(*u.choose(&[MathType::DisplayMath, MathType::InlineMath])?, text(u)?),
        15 => Inline::RawInline(Format::new(text(u)?), text(u)?),
        16 => Inline::Link(attr(u)?, inlines(u)?, Target::new(text(u)?, text(u)?)),
        17 => Inline::Image(attr(u)?, inlines(u)?, Target::new(text(u)?, text(u)?)),
        18 => Inline::Note(blocks(u)?),
        _ => Inline::Span(attr(u)?, inlines(u)?),
    })
}

fn rows(u: &mut Unstructured, l: u32) -> Result<Vec<Row>> {
    list(u, |u| {
        Ok(Row {
            attr: attr(u)?,
            cells: list(u, |u| {
                Ok(Cell {
                    attr: attr(u)?,
                    align: alignment(u)?,
                    row_span: u.arbitrary()?,
                    col_span: u.arbitrary()?,
                    content: list(u, |u| block(u, l))?,
                })
            })?,
        })
    })
}

fn alignment(u: &mut Unstructured) -> Result<Alignment> {
    Ok(*u.choose(&[
        Alignment::AlignLeft,
        Alignment::AlignRight,
        Alignment::AlignCenter,
        Alignment::AlignDefault,
    ])?)
}

fn caption(u: &mut Unstructured, l: u32) -> Result<Caption> {
    Ok(Caption {
        short: if u.arbitrary()? { Some(list(u, |u| inline(u, l))?) } else { None },
        long: list(u, |u| block(u, l))?,
    })
}

fn block(u: &mut Unstructured, level: u32) -> Result<Block> {
    if level == 0 {
        return Ok(Block::HorizontalRule);
    }
    let l = level - 1;
    let inlines = |u: &mut Unstructured| list(u, |u| inline(u, l));
    let blocks = |u: &mut Unstructured| list(u, |u| block(u, l));
    Ok(match u.int_in_range(0..=13)? {
        0 => Block::Plain(inlines(u)?),
        1 => Block::Para(inlines(u)?),
        2 => Block::LineBlock(list(u, inlines)?),
        3 => Block::CodeBlock(attr(u)?, text(u)?),
        4 => Block::RawBlock(Format::new(text(u)?), text(u)?),
        5 => Block::BlockQuote(blocks(u)?),
        6 => Block::OrderedList(
            ListAttributes {
                start: u.arbitrary()?,
                style: *u.choose(&[
                    ListNumberStyle::DefaultStyle,
                    ListNumberStyle::Example,
                    ListNumberStyle::Decimal,
                    ListNumberStyle::LowerRoman,
                    ListNumberStyle::UpperRoman,
                    ListNumberStyle::LowerAlpha,
                    ListNumberStyle::UpperAlpha,
                ])?,
                delim: *u.choose(&[
                    ListNumberDelim::DefaultDelim,
                    ListNumberDelim::Period,
                    ListNumberDelim::OneParen,
                    ListNumberDelim::TwoParens,
                ])?,
            },
            list(u, blocks)?,
        ),
        7 => Block::BulletList(list(u, blocks)?),
        8 => Block::DefinitionList(list(u, |u| Ok((inlines(u)?, list(u, blocks)?)))?),
        9 => Block::Header(u.arbitrary()?, attr(u)?, inlines(u)?),
        10 => Block::HorizontalRule,
        11 => Block::Table(Box::new(Table {
            attr: attr(u)?,
            caption: caption(u, l)?,
            col_specs: list(u, |u| {
                Ok(ColSpec {
                    align: alignment(u)?,
                    width: if u.arbitrary()? {
                        ColWidth::ColWidth(width(u)?)
                    } else {
                        ColWidth::ColWidthDefault
                    },
                })
            })?,
            head: TableHead { attr: attr(u)?, rows: rows(u, l)? },
            bodies: list(u, |u| {
                Ok(TableBody {
                    attr: attr(u)?,
                    row_head_columns: u.arbitrary()?,
                    head: rows(u, l)?,
                    body: rows(u, l)?,
                })
            })?,
            foot: TableFoot { attr: attr(u)?, rows: rows(u, l)? },
        })),
        12 => Block::Figure(attr(u)?, caption(u, l)?, blocks(u)?),
        _ => Block::Div(attr(u)?, blocks(u)?),
    })
}

fn meta_value(u: &mut Unstructured, level: u32) -> Result<MetaValue> {
    if level == 0 {
        return Ok(MetaValue::MetaBool(u.arbitrary()?));
    }
    let l = level - 1;
    Ok(match u.int_in_range(0..=5)? {
        0 => MetaValue::MetaMap(list(u, |u| Ok((text(u)?, meta_value(u, l)?)))?.into_iter().collect()),
        1 => MetaValue::MetaList(list(u, |u| meta_value(u, l))?),
        2 => MetaValue::MetaBool(u.arbitrary()?),
        3 => MetaValue::MetaString(text(u)?),
        4 => MetaValue::MetaInlines(list(u, |u| inline(u, l))?),
        _ => MetaValue::MetaBlocks(list(u, |u| block(u, l))?),
    })
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(meta) = list(&mut u, |u| Ok((text(u)?, meta_value(u, 3)?))) else { return };
    let Ok(blocks) = list(&mut u, |u| block(u, MAX_LEVELS)) else { return };
    let doc = Pandoc { meta: Meta(meta.into_iter().collect()), blocks };
    let written = json::to_json(&doc);
    match json::from_json(&written) {
        Ok(again) => {
            let rewritten = json::to_json(&again);
            assert_eq!(rewritten, written);
            // NaN is not equal to itself. Compare the trees when the text has no NaN.
            if !written.contains(r#""t":"ColWidth","c":null"#) {
                assert_eq!(again, doc);
            }
        }
        Err(error) => panic!("the writer output does not read: {error}\n{written}"),
    }
});
