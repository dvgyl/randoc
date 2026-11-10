//! Typed decoders from the tape of the parser to the document model.
//!
//! Each decoder gives the error text and the error path of the generic
//! `aeson` instances that `pandoc-types` uses. The fields of a constructor add
//! no path item. The items of a list and of a tuple add an index. The fields of
//! a record and the keys of a map add a key.

use std::collections::BTreeMap;

use super::categories::{LETTERS, NUMBERS};
use super::haskell::show_digits;
use super::parse::{Items, Kind, Value};
use crate::{
    API_VERSION, Alignment, Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, ColWidth,
    Format, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, MathType, Meta, MetaValue,
    Pandoc, QuoteType, Row, Table, TableBody, TableFoot, TableHead, Target, Text,
};

/// One item of an error path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathItem {
    /// A key of an object.
    Key(String),
    /// An index of an array.
    Index(usize),
}

/// A failure of a typed decoder.
///
/// The data is in a box: a small error type keeps the stack frames of the
/// recursive decoders small.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(Box<ErrorData>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ErrorData {
    /// The path from the error to the root, in reverse order.
    reverse_path: Vec<PathItem>,
    /// The message of `aeson`.
    message: String,
}

impl DecodeError {
    fn new(message: impl Into<String>) -> DecodeError {
        DecodeError(Box::new(ErrorData {
            reverse_path: Vec::new(),
            message: message.into(),
        }))
    }

    fn within(mut self, item: PathItem) -> DecodeError {
        self.0.reverse_path.push(item);
        self
    }

    /// Formats the error as `aeson` does: `Error in $.blocks[0]: message`.
    pub fn render(&self) -> String {
        let mut out = String::from("Error in $");
        for item in self.0.reverse_path.iter().rev() {
            match item {
                PathItem::Index(index) => {
                    out.push('[');
                    out.push_str(&index.to_string());
                    out.push(']');
                }
                PathItem::Key(key) if is_identifier(key) => {
                    out.push('.');
                    out.push_str(key);
                }
                PathItem::Key(key) => {
                    out.push_str("['");
                    for c in key.chars() {
                        if c == '\'' || c == '\\' {
                            out.push('\\');
                        }
                        out.push(c);
                    }
                    out.push_str("']");
                }
            }
        }
        out.push_str(": ");
        out.push_str(&self.0.message);
        out
    }
}

/// `aeson` writes a key after a dot when the Haskell functions accept it:
/// `isAlpha` for the first character and `isAlphaNum` for the others. They use
/// the general categories (letters L, numbers N) of the Unicode tables of GHC.
fn is_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(is_letter) && chars.all(|c| is_letter(c) || in_table(&NUMBERS, c))
}

fn is_letter(c: char) -> bool {
    in_table(&LETTERS, c)
}

fn in_table(table: &[(u32, u32)], c: char) -> bool {
    let code = u32::from(c);
    table
        .binary_search_by(|&(low, high)| {
            if high < code {
                std::cmp::Ordering::Less
            } else if low > code {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// The result of a decoder.
pub type Decoded<T> = Result<T, DecodeError>;

/// A type that the decoder can read from a value on the tape.
pub trait Decode: Sized {
    /// Reads a value of the type.
    fn decode(value: Value<'_>) -> Decoded<Self>;
}

fn at<T>(index: usize, result: Decoded<T>) -> Decoded<T> {
    result.map_err(|e| e.within(PathItem::Index(index)))
}

fn at_key<T>(key: &str, result: Decoded<T>) -> Decoded<T> {
    result.map_err(|e| e.within(PathItem::Key(key.to_owned())))
}

impl Decode for Text {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        match value.kind() {
            Kind::String(text) => Ok(text.to_owned()),
            _ => Err(DecodeError::new(format!(
                "parsing Text failed, expected String, but encountered {}",
                value.type_name()
            ))),
        }
    }
}

impl Decode for i64 {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        match value.kind() {
            Kind::Number(text) => {
                if let Some(small) = small_int(text) {
                    return Ok(small);
                }
                let number = Decimal::parse(text);
                number.to_i64().ok_or_else(|| {
                    DecodeError::new(format!(
                        "parsing Int failed, value is either floating or will cause over or underflow {}",
                        number.show()
                    ))
                })
            }
            _ => Err(DecodeError::new(format!(
                "parsing Int failed, expected Number, but encountered {}",
                value.type_name()
            ))),
        }
    }
}

impl Decode for f64 {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        match value.kind() {
            Kind::Number(text) => Ok(Decimal::parse(text).to_f64()),
            Kind::Null => Ok(f64::NAN),
            Kind::String("+inf") => Ok(f64::INFINITY),
            Kind::String("-inf") => Ok(f64::NEG_INFINITY),
            _ => Err(DecodeError::new(format!(
                "parsing Double failed, unexpected {}",
                value.type_name()
            ))),
        }
    }
}

impl Decode for bool {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        match value.kind() {
            Kind::Bool(flag) => Ok(flag),
            _ => Err(DecodeError::new(format!(
                "expected Bool, but encountered {}",
                value.type_name()
            ))),
        }
    }
}

impl<T: Decode> Decode for Vec<T> {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        match value.kind() {
            Kind::Array => {
                let mut out = Vec::with_capacity(value.len());
                for (index, item) in value.items().enumerate() {
                    out.push(at(index, T::decode(item))?);
                }
                Ok(out)
            }
            _ => Err(DecodeError::new(format!(
                "parsing [] failed, expected Array, but encountered {}",
                value.type_name()
            ))),
        }
    }
}

impl<T: Decode> Decode for Option<T> {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        match value.kind() {
            Kind::Null => Ok(None),
            _ => T::decode(value).map(Some),
        }
    }
}

/// Checks a tuple, which `aeson` writes as an array with one item for each part.
fn tuple(value: Value<'_>, len: usize) -> Decoded<Items<'_>> {
    if value.kind() != Kind::Array {
        let parts = ["a", "b", "c", "d", "e", "f"];
        return Err(DecodeError::new(format!(
            "parsing ({}) failed, expected Array, but encountered {}",
            parts[..len].join(", "),
            value.type_name()
        )));
    }
    if value.len() != len {
        return Err(DecodeError::new(format!(
            "cannot unpack array of length {} into a tuple of length {len}",
            value.len()
        )));
    }
    Ok(value.items())
}

impl<A: Decode, B: Decode> Decode for (A, B) {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        let mut items = tuple(value, 2)?;
        Ok((
            at(0, A::decode(items.next_item()))?,
            at(1, B::decode(items.next_item()))?,
        ))
    }
}

impl Decode for Attr {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        let mut items = tuple(value, 3)?;
        Ok(Attr {
            id: at(0, Text::decode(items.next_item()))?,
            classes: at(1, Vec::decode(items.next_item()))?,
            attributes: at(2, Vec::decode(items.next_item()))?,
        })
    }
}

impl Decode for Target {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        let (url, title) = <(Text, Text)>::decode(value)?;
        Ok(Target { url, title })
    }
}

impl Decode for ListAttributes {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        let mut items = tuple(value, 3)?;
        Ok(ListAttributes {
            start: at(0, i64::decode(items.next_item()))?,
            style: at(1, ListNumberStyle::decode(items.next_item()))?,
            delim: at(2, ListNumberDelim::decode(items.next_item()))?,
        })
    }
}

impl Decode for ColSpec {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        let (align, width) = <(Alignment, ColWidth)>::decode(value)?;
        Ok(ColSpec { align, width })
    }
}

impl Decode for Format {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        Text::decode(value).map(Format)
    }
}

/// Checks the fields of a constructor with more than one field.
/// The fields are an array and add no path item.
fn fields<'t>(
    value: Value<'t>,
    constructor: &str,
    type_name: &str,
    len: usize,
) -> Decoded<Items<'t>> {
    if value.kind() != Kind::Array {
        return Err(DecodeError::new(format!(
            "When parsing the constructor {constructor} of type Text.Pandoc.Definition.{type_name} expected Array but got {}.",
            value.type_name()
        )));
    }
    if value.len() != len {
        return Err(DecodeError::new(format!(
            "When parsing the constructor {constructor} of type Text.Pandoc.Definition.{type_name} expected Array of length {len} but got Array of length {}.",
            value.len()
        )));
    }
    Ok(value.items())
}

/// An object with a `t` field, as `aeson` writes a constructor of a sum type.
struct Tagged<'t> {
    tag: &'t str,
    content: Option<Value<'t>>,
}

impl<'t> Tagged<'t> {
    fn content(&self) -> Decoded<Value<'t>> {
        self.content
            .ok_or_else(|| DecodeError::new("key \"c\" not found"))
    }

    fn one<T: Decode>(&self) -> Decoded<T> {
        T::decode(self.content()?)
    }

    fn fields(&self, type_name: &str, len: usize) -> Decoded<Items<'t>> {
        fields(self.content()?, self.tag, type_name, len)
    }
}

/// Implements `Decode` for an enumeration that `aeson` writes as `{"t": NAME}`.
/// A `c` field is ignored.
macro_rules! decode_enum {
    ($type:ident { $($variant:ident),+ $(,)? }) => {
        impl Decode for $type {
            fn decode(value: Value<'_>) -> Decoded<Self> {
                const TABLE: &[(&str, Constructor<$type>)] =
                    &[$((stringify!($variant), |_| Ok($type::$variant))),+];
                decode_sum(value, stringify!($type), TABLE)
            }
        }
    };
}

decode_enum!(ListNumberStyle {
    DefaultStyle,
    Example,
    Decimal,
    LowerRoman,
    UpperRoman,
    LowerAlpha,
    UpperAlpha
});
decode_enum!(ListNumberDelim {
    DefaultDelim,
    Period,
    OneParen,
    TwoParens
});
decode_enum!(QuoteType {
    SingleQuote,
    DoubleQuote
});
decode_enum!(MathType {
    DisplayMath,
    InlineMath
});
decode_enum!(CitationMode {
    AuthorInText,
    SuppressAuthor,
    NormalCitation
});
decode_enum!(Alignment {
    AlignLeft,
    AlignRight,
    AlignCenter,
    AlignDefault
});

impl Decode for ColWidth {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        const TABLE: &[(&str, Constructor<ColWidth>)] = &[
            ("ColWidth", |t| t.one().map(ColWidth::ColWidth)),
            ("ColWidthDefault", |_| Ok(ColWidth::ColWidthDefault)),
        ];
        decode_sum(value, "ColWidth", TABLE)
    }
}

/// A decoder of the content of one constructor.
type Constructor<T> = fn(&Tagged<'_>) -> Decoded<T>;

/// Reads a sum type with a table of constructors. The table keeps the stack
/// frame of each recursive call small.
fn decode_sum<T>(
    value: Value<'_>,
    type_name: &str,
    table: &[(&str, Constructor<T>)],
) -> Decoded<T> {
    if value.kind() != Kind::Object {
        return Err(DecodeError::new(format!(
            "When parsing Text.Pandoc.Definition.{type_name} expected Object but got {}.",
            value.type_name()
        )));
    }
    let (tag, content) = value.tag_and_content();
    let tag = match tag {
        None => return Err(DecodeError::new("key \"t\" not found")),
        Some(found) => match found.as_str() {
            Some(tag) => tag,
            None => {
                return Err(DecodeError::new(format!(
                    "parsing Key failed, expected String, but encountered {}",
                    found.type_name()
                ))
                .within(PathItem::Key("t".to_owned())));
            }
        },
    };
    // The length and the first byte reject most names without a full comparison.
    let first = tag.as_bytes().first();
    let found = table.iter().find(|(name, _)| {
        name.len() == tag.len() && name.as_bytes().first() == first && *name == tag
    });
    match found {
        Some((_, constructor)) => constructor(&Tagged { tag, content }),
        None => {
            let names: Vec<&str> = table.iter().map(|(name, _)| *name).collect();
            Err(DecodeError::new(format!(
                "When parsing Text.Pandoc.Definition.{type_name} expected an Object with a tag field where the value is one of [{}], but got {tag}.",
                names.join(", ")
            )))
        }
    }
}

const BLOCK: &str = "Block";

const BLOCKS: &[(&str, Constructor<Block>)] = &[
    ("Plain", |t| t.one().map(Block::Plain)),
    ("Para", |t| t.one().map(Block::Para)),
    ("LineBlock", |t| t.one().map(Block::LineBlock)),
    ("CodeBlock", |t| {
        let mut f = t.fields(BLOCK, 2)?;
        Ok(Block::CodeBlock(
            Attr::decode(f.next_item())?,
            Text::decode(f.next_item())?,
        ))
    }),
    ("RawBlock", |t| {
        let mut f = t.fields(BLOCK, 2)?;
        Ok(Block::RawBlock(
            Format::decode(f.next_item())?,
            Text::decode(f.next_item())?,
        ))
    }),
    ("BlockQuote", |t| t.one().map(Block::BlockQuote)),
    ("OrderedList", |t| {
        let mut f = t.fields(BLOCK, 2)?;
        Ok(Block::OrderedList(
            ListAttributes::decode(f.next_item())?,
            Vec::decode(f.next_item())?,
        ))
    }),
    ("BulletList", |t| t.one().map(Block::BulletList)),
    ("DefinitionList", |t| t.one().map(Block::DefinitionList)),
    ("Header", |t| {
        let mut f = t.fields(BLOCK, 3)?;
        Ok(Block::Header(
            i64::decode(f.next_item())?,
            Attr::decode(f.next_item())?,
            Vec::decode(f.next_item())?,
        ))
    }),
    ("HorizontalRule", |_| Ok(Block::HorizontalRule)),
    ("Table", |t| decode_table(t.content()?).map(Block::Table)),
    ("Figure", |t| {
        let mut f = t.fields(BLOCK, 3)?;
        Ok(Block::Figure(
            Attr::decode(f.next_item())?,
            Caption::decode(f.next_item())?,
            Vec::decode(f.next_item())?,
        ))
    }),
    ("Div", |t| {
        let mut f = t.fields(BLOCK, 2)?;
        Ok(Block::Div(
            Attr::decode(f.next_item())?,
            Vec::decode(f.next_item())?,
        ))
    }),
];

impl Decode for Block {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        decode_sum(value, BLOCK, BLOCKS)
    }
}

const INLINE: &str = "Inline";

const INLINES: &[(&str, Constructor<Inline>)] = &[
    ("Str", |t| t.one().map(Inline::Str)),
    ("Emph", |t| t.one().map(Inline::Emph)),
    ("Underline", |t| t.one().map(Inline::Underline)),
    ("Strong", |t| t.one().map(Inline::Strong)),
    ("Strikeout", |t| t.one().map(Inline::Strikeout)),
    ("Superscript", |t| t.one().map(Inline::Superscript)),
    ("Subscript", |t| t.one().map(Inline::Subscript)),
    ("SmallCaps", |t| t.one().map(Inline::SmallCaps)),
    ("Quoted", |t| {
        let mut f = t.fields(INLINE, 2)?;
        Ok(Inline::Quoted(
            QuoteType::decode(f.next_item())?,
            Vec::decode(f.next_item())?,
        ))
    }),
    ("Cite", |t| {
        let mut f = t.fields(INLINE, 2)?;
        Ok(Inline::Cite(
            Vec::decode(f.next_item())?,
            Vec::decode(f.next_item())?,
        ))
    }),
    ("Code", |t| {
        let mut f = t.fields(INLINE, 2)?;
        Ok(Inline::Code(
            Attr::decode(f.next_item())?,
            Text::decode(f.next_item())?,
        ))
    }),
    ("Space", |_| Ok(Inline::Space)),
    ("SoftBreak", |_| Ok(Inline::SoftBreak)),
    ("LineBreak", |_| Ok(Inline::LineBreak)),
    ("Math", |t| {
        let mut f = t.fields(INLINE, 2)?;
        Ok(Inline::Math(
            MathType::decode(f.next_item())?,
            Text::decode(f.next_item())?,
        ))
    }),
    ("RawInline", |t| {
        let mut f = t.fields(INLINE, 2)?;
        Ok(Inline::RawInline(
            Format::decode(f.next_item())?,
            Text::decode(f.next_item())?,
        ))
    }),
    ("Link", |t| {
        let mut f = t.fields(INLINE, 3)?;
        Ok(Inline::Link(
            Attr::decode(f.next_item())?,
            Vec::decode(f.next_item())?,
            Target::decode(f.next_item())?,
        ))
    }),
    ("Image", |t| {
        let mut f = t.fields(INLINE, 3)?;
        Ok(Inline::Image(
            Attr::decode(f.next_item())?,
            Vec::decode(f.next_item())?,
            Target::decode(f.next_item())?,
        ))
    }),
    ("Note", |t| t.one().map(Inline::Note)),
    ("Span", |t| {
        let mut f = t.fields(INLINE, 2)?;
        Ok(Inline::Span(
            Attr::decode(f.next_item())?,
            Vec::decode(f.next_item())?,
        ))
    }),
];

impl Decode for Inline {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        decode_sum(value, INLINE, INLINES)
    }
}

const METAS: &[(&str, Constructor<MetaValue>)] = &[
    ("MetaMap", |t| {
        decode_map(t.content()?).map(MetaValue::MetaMap)
    }),
    ("MetaList", |t| t.one().map(MetaValue::MetaList)),
    ("MetaBool", |t| t.one().map(MetaValue::MetaBool)),
    ("MetaString", |t| t.one().map(MetaValue::MetaString)),
    ("MetaInlines", |t| t.one().map(MetaValue::MetaInlines)),
    ("MetaBlocks", |t| t.one().map(MetaValue::MetaBlocks)),
];

impl Decode for MetaValue {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        decode_sum(value, "MetaValue", METAS)
    }
}

/// Reads a small non-negative or negative integer without the general path.
fn small_int(text: &str) -> Option<i64> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || digits.len() > 18 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value = digits
        .bytes()
        .fold(0i64, |acc, b| acc * 10 + i64::from(b - b'0'));
    Some(if digits.len() == text.len() {
        value
    } else {
        -value
    })
}

/// Reads a map. The first of two equal keys wins, and the keys are read in sorted order.
fn decode_map(value: Value<'_>) -> Decoded<BTreeMap<Text, MetaValue>> {
    if value.kind() != Kind::Object {
        return Err(DecodeError::new(format!(
            "parsing Map ~Text failed, expected Object, but encountered {}",
            value.type_name()
        )));
    }
    let mut first: BTreeMap<&str, Value<'_>> = BTreeMap::new();
    for (key, item) in value.entries() {
        first.entry(key).or_insert(item);
    }
    // A plain loop: an iterator chain here uses a large stack frame at each level.
    let mut out = BTreeMap::new();
    for (key, item) in first {
        let value = at_key(key, MetaValue::decode(item))?;
        out.insert(key.to_owned(), value);
    }
    Ok(out)
}

/// The content of the constructor `Table` of `Block`. The errors name that constructor.
impl Decode for Table {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        decode_table(value).map(|table| *table)
    }
}

/// Reads the fields of a table into a box. A table is large, and a value on
/// the stack in each level of the recursion would make the frames large.
fn decode_table(value: Value<'_>) -> Decoded<Box<Table>> {
    let mut f = fields(value, "Table", BLOCK, 6)?;
    let mut table = Box::<Table>::default();
    table.attr = Attr::decode(f.next_item())?;
    table.caption = Caption::decode(f.next_item())?;
    table.col_specs = Vec::decode(f.next_item())?;
    table.head = TableHead::decode(f.next_item())?;
    table.bodies = Vec::decode(f.next_item())?;
    table.foot = TableFoot::decode(f.next_item())?;
    Ok(table)
}

impl Decode for BTreeMap<Text, MetaValue> {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        decode_map(value)
    }
}

impl Decode for Meta {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        decode_map(value).map(Meta)
    }
}

impl Decode for Citation {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        if value.kind() != Kind::Object {
            return Err(DecodeError::new(format!(
                "When parsing the constructor Citation of type Text.Pandoc.Definition.Citation expected Object but got {}.",
                value.type_name()
            )));
        }
        let entries = value;
        fn field<T: Decode>(entries: Value<'_>, key: &str) -> Decoded<T> {
            let item = entries.lookup(key).ok_or_else(|| {
                DecodeError::new(format!(
                    "When parsing the record Citation of type Text.Pandoc.Definition.Citation the key {key} was not present."
                ))
            })?;
            at_key(key, T::decode(item))
        }
        Ok(Citation {
            id: field(entries, "citationId")?,
            prefix: field(entries, "citationPrefix")?,
            suffix: field(entries, "citationSuffix")?,
            mode: field(entries, "citationMode")?,
            note_num: field(entries, "citationNoteNum")?,
            hash: field(entries, "citationHash")?,
        })
    }
}

impl Decode for Caption {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        let mut f = fields(value, "Caption", "Caption", 2)?;
        Ok(Caption {
            short: Option::decode(f.next_item())?,
            long: Vec::decode(f.next_item())?,
        })
    }
}

impl Decode for TableHead {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        let mut f = fields(value, "TableHead", "TableHead", 2)?;
        Ok(TableHead {
            attr: Attr::decode(f.next_item())?,
            rows: Vec::decode(f.next_item())?,
        })
    }
}

impl Decode for TableBody {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        let mut f = fields(value, "TableBody", "TableBody", 4)?;
        Ok(TableBody {
            attr: Attr::decode(f.next_item())?,
            row_head_columns: i64::decode(f.next_item())?,
            head: Vec::decode(f.next_item())?,
            body: Vec::decode(f.next_item())?,
        })
    }
}

impl Decode for TableFoot {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        let mut f = fields(value, "TableFoot", "TableFoot", 2)?;
        Ok(TableFoot {
            attr: Attr::decode(f.next_item())?,
            rows: Vec::decode(f.next_item())?,
        })
    }
}

impl Decode for Row {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        let mut f = fields(value, "Row", "Row", 2)?;
        Ok(Row {
            attr: Attr::decode(f.next_item())?,
            cells: Vec::decode(f.next_item())?,
        })
    }
}

impl Decode for Cell {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        let mut f = fields(value, "Cell", "Cell", 5)?;
        Ok(Cell {
            attr: Attr::decode(f.next_item())?,
            align: Alignment::decode(f.next_item())?,
            row_span: i64::decode(f.next_item())?,
            col_span: i64::decode(f.next_item())?,
            content: Vec::decode(f.next_item())?,
        })
    }
}

impl Decode for Pandoc {
    fn decode(value: Value<'_>) -> Decoded<Self> {
        if value.kind() != Kind::Object {
            return Err(DecodeError::new("mempty"));
        }
        const VERSION_KEY: &str = "pandoc-api-version";
        let version = match value.lookup(VERSION_KEY) {
            None => {
                return Err(DecodeError::new("JSON missing pandoc-api-version."));
            }
            Some(found) if found.kind() == Kind::Null => {
                return Err(DecodeError::new("JSON missing pandoc-api-version."));
            }
            Some(found) => at_key(VERSION_KEY, Vec::<i64>::decode(found))?,
        };
        if version.len() < 2 || version[..2] != API_VERSION[..2] {
            return Err(DecodeError::new(format!(
                "Incompatible API versions: encoded with {} but attempted to decode with {}.",
                show_int_list(&version),
                show_int_list(&API_VERSION)
            )));
        }
        let meta = value
            .lookup("meta")
            .ok_or_else(|| DecodeError::new("key \"meta\" not found"))?;
        let meta = at_key("meta", Meta::decode(meta))?;
        let blocks = value
            .lookup("blocks")
            .ok_or_else(|| DecodeError::new("key \"blocks\" not found"))?;
        let blocks = at_key("blocks", Vec::decode(blocks))?;
        Ok(Pandoc { meta, blocks })
    }
}

fn show_int_list(values: &[i64]) -> String {
    let items: Vec<String> = values.iter().map(i64::to_string).collect();
    format!("[{}]", items.join(","))
}

/// A JSON number as the Haskell type `Scientific`: `coefficient * 10^exponent`.
///
/// `aeson` reads the exponent as a 64-bit integer that wraps on overflow.
/// This type keeps that exponent unchanged: the removal of trailing zeros
/// must not wrap it again, because the conversions use the exact value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decimal<'a> {
    negative: bool,
    /// The digits of the coefficient, with no leading or trailing zeros. Empty for zero.
    digits: Vec<u8>,
    /// The number of trailing zeros of the coefficient that `digits` does not hold.
    zeros: usize,
    /// The exponent of the last digit of the coefficient, as `aeson` reads it.
    exponent: i64,
    text: &'a str,
}

impl<'a> Decimal<'a> {
    /// Reads the text of a valid JSON number.
    pub fn parse(text: &'a str) -> Decimal<'a> {
        let (negative, body) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (mantissa, exp_text) = match body.find(['e', 'E']) {
            Some(index) => (&body[..index], &body[index + 1..]),
            None => (body, ""),
        };
        let (int_part, frac_part) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let (exp_negative, exp_digits) = match exp_text.as_bytes().first() {
            Some(b'-') => (true, &exp_text[1..]),
            Some(b'+') => (false, &exp_text[1..]),
            _ => (false, exp_text),
        };
        let mut exp = exp_digits.bytes().fold(0i64, |acc, byte| {
            acc.wrapping_mul(10).wrapping_add(i64::from(byte - b'0'))
        });
        if exp_negative {
            exp = exp.wrapping_neg();
        }
        let exponent = exp.wrapping_sub(frac_part.len() as i64);
        let mut digits: Vec<u8> = int_part
            .bytes()
            .chain(frac_part.bytes())
            .map(|byte| byte - b'0')
            .skip_while(|&d| d == 0)
            .collect();
        let mut zeros = 0;
        while digits.last() == Some(&0) {
            digits.pop();
            zeros += 1;
        }
        Decimal {
            negative,
            digits,
            zeros,
            exponent,
            text,
        }
    }

    /// The exponent of the last non-zero digit, without overflow.
    fn normal_exponent(&self) -> i128 {
        i128::from(self.exponent) + self.zeros as i128
    }

    /// The value as an `Int`, when it is an integer in the 64-bit range.
    pub fn to_i64(&self) -> Option<i64> {
        if self.digits.is_empty() {
            return Some(0);
        }
        let exponent = self.normal_exponent();
        if !(0..=18).contains(&exponent) || self.digits.len() > 19 {
            return None;
        }
        let mut value: i128 = 0;
        for &digit in &self.digits {
            value = value * 10 + i128::from(digit);
        }
        value *= 10i128.pow(exponent as u32);
        if self.negative {
            value = -value;
        }
        i64::try_from(value).ok()
    }

    /// The nearest `Double`. Zero is always positive.
    pub fn to_f64(&self) -> f64 {
        if self.digits.is_empty() {
            return 0.0;
        }
        if !self.text.contains(['e', 'E']) && self.text.len() < 16 {
            return self.text.parse().unwrap_or(f64::NAN);
        }
        let mut text = String::with_capacity(self.digits.len() + 48);
        if self.negative {
            text.push('-');
        }
        text.extend(self.digits.iter().map(|d| char::from(b'0' + d)));
        text.push('e');
        text.push_str(&self.normal_exponent().to_string());
        text.parse().unwrap_or(f64::NAN)
    }

    /// The text of `show` for the value. The exponent wraps as a Haskell `Int` does.
    pub fn show(&self) -> String {
        if self.digits.is_empty() {
            return "0.0".to_owned();
        }
        let point = self
            .exponent
            .wrapping_add(self.zeros as i64)
            .wrapping_add(self.digits.len() as i64);
        show_digits(self.negative, &self.digits, point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_follow_the_scientific_rules() {
        // Each expected text is from a Pandoc 3.13 error message for a Header level.
        let shown = [
            ("1.5", "1.5"),
            ("9223372036854775808", "9.223372036854775808e18"),
            ("-9223372036854775809", "-9.223372036854775809e18"),
            ("1e19", "1.0e19"),
            ("1e1000", "1.0e1000"),
            ("1e-1000", "1.0e-1000"),
            ("12345678.9", "1.23456789e7"),
            ("0.001", "1.0e-3"),
            ("-1.50", "-1.5"),
            ("1.0e-1", "0.1"),
            ("1234567.5", "1234567.5"),
            ("99999999999999999999", "9.9999999999999999999e19"),
            ("0.09", "9.0e-2"),
            ("1e99999999999999999999", "1.0e7766279631452241919"),
            ("1e-99999999999999999999", "1.0e-7766279631452241919"),
        ];
        for (text, expected) in shown {
            let number = Decimal::parse(text);
            assert_eq!(number.to_i64(), None, "{text}");
            assert_eq!(number.show(), expected);
        }
        let ints = [
            ("9223372036854775807", i64::MAX),
            ("-9223372036854775808", i64::MIN),
            ("1e18", 1_000_000_000_000_000_000),
            ("1000e-3", 1),
            ("-0", 0),
            ("-0.0", 0),
            ("0e999", 0),
            ("100000000000000000000e-2", 1_000_000_000_000_000_000),
            ("1.23e2", 123),
            ("0.5e1", 5),
            ("10e-1", 1),
            ("1E0", 1),
        ];
        for (text, expected) in ints {
            assert_eq!(Decimal::parse(text).to_i64(), Some(expected), "{text}");
        }
        assert_eq!(Decimal::parse("0").show(), "0.0");
    }

    #[test]
    fn doubles_follow_the_oracle() {
        let cases = [
            ("1e400", f64::INFINITY),
            ("-1e400", f64::NEG_INFINITY),
            ("1e18446744073709551615", 0.1),
            ("0.5", 0.5),
            ("1e23", 1e23),
            ("5e-324", 5e-324),
            ("123456789012345678901234567890", 1.234_567_890_123_456_8e29),
        ];
        for (text, expected) in cases {
            assert_eq!(Decimal::parse(text).to_f64(), expected, "{text}");
        }
        assert!(Decimal::parse("-0.0").to_f64().is_sign_positive());
        // Trailing zeros of the coefficient must not wrap the exponent of `aeson` again.
        let limits = [
            ("10e9223372036854775807", f64::INFINITY),
            ("-100e9223372036854775807", f64::NEG_INFINITY),
            ("1.50e9223372036854775807", f64::INFINITY),
            ("100e-9223372036854775808", 0.0),
            ("10.0e-9223372036854775808", f64::INFINITY),
        ];
        for (text, expected) in limits {
            assert_eq!(Decimal::parse(text).to_f64(), expected, "{text}");
        }
        assert_eq!(Decimal::parse("10e9223372036854775807").to_i64(), None);
        assert_eq!(Decimal::parse("1000e-3").to_i64(), Some(1));
        assert!(Decimal::parse("-1e-400").to_f64().is_sign_negative());
    }

    #[test]
    fn paths_use_the_aeson_key_forms() {
        let error = DecodeError::new("m")
            .within(PathItem::Key("a'b\\".into()))
            .within(PathItem::Index(3))
            .within(PathItem::Key("é1".into()))
            .within(PathItem::Key("_x".into()))
            .within(PathItem::Key("".into()));
        assert_eq!(error.render(), r"Error in $['']['_x'].é1[3]['a\'b\\']: m");
        // Each expected form is from Pandoc 3.13 (opus review F-03).
        let forms = [
            ("\u{2163}", false),
            ("a\u{2163}", true),
            ("a\u{2150}", true),
            ("a\u{1c89}", false),
            ("a\u{905}\u{93e}", false),
            ("\u{24d0}", false),
            ("x\u{661}", true),
            ("\u{4e2d}\u{6587}", true),
        ];
        for (key, dotted) in forms {
            assert_eq!(is_identifier(key), dotted, "{key:?}");
        }
        // The first and the last character of a range of the tables.
        for key in ["a", "z", "A", "Z", "z9", "a0", "\u{10fffd}"] {
            assert_eq!(is_identifier(key), key != "\u{10fffd}", "{key:?}");
        }
        assert!(in_table(&LETTERS, 'z') && in_table(&LETTERS, 'a'));
        assert!(in_table(&NUMBERS, '9') && in_table(&NUMBERS, '0'));
        assert!(!in_table(&LETTERS, '{') && !in_table(&LETTERS, '`'));
    }

    #[test]
    fn trailing_zeros_keep_the_aeson_exponent() {
        // Opus review F-01. Pandoc 3.13 writes these column widths as "+inf" and "-inf".
        assert_eq!(
            Decimal::parse("1.0e-9223372036854775808").to_f64(),
            f64::INFINITY
        );
        assert_eq!(
            Decimal::parse("-1.0e-9223372036854775808").to_f64(),
            f64::NEG_INFINITY
        );
        assert_eq!(
            Decimal::parse("2.50e-9223372036854775807").to_f64(),
            f64::INFINITY
        );
        assert_eq!(Decimal::parse("0.10e-9223372036854775806").to_f64(), 0.0);
    }
}
