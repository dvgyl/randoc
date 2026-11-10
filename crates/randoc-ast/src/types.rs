use std::collections::BTreeMap;

/// The string type of the document model.
///
/// Use this alias for each string that the tree holds.
pub type Text = String;

/// The version of the document model, as `pandoc -t json` writes it.
pub const API_VERSION: [i64; 4] = [1, 23, 1, 2];

/// A document: metadata and a list of blocks.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pandoc {
    /// The document metadata.
    pub meta: Meta,
    /// The document body.
    pub blocks: Vec<Block>,
}

/// Document metadata: a map from a field name to a value.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Meta(pub BTreeMap<Text, MetaValue>);

impl Meta {
    /// Returns the value of a metadata field.
    pub fn get(&self, key: &str) -> Option<&MetaValue> {
        self.0.get(key)
    }

    /// Sets a metadata field and returns the value that it replaces.
    pub fn insert(&mut self, key: impl Into<Text>, value: MetaValue) -> Option<MetaValue> {
        self.0.insert(key.into(), value)
    }

    /// Returns `true` when there are no metadata fields.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A metadata value.
#[derive(Debug, Clone, PartialEq)]
pub enum MetaValue {
    /// A map from a field name to a value.
    MetaMap(BTreeMap<Text, MetaValue>),
    /// A list of values.
    MetaList(Vec<MetaValue>),
    /// A Boolean value.
    MetaBool(bool),
    /// A plain string.
    MetaString(Text),
    /// Inline content.
    MetaInlines(Vec<Inline>),
    /// Block content.
    MetaBlocks(Vec<Block>),
}

/// A block element.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// Plain text that is not a paragraph.
    Plain(Vec<Inline>),
    /// A paragraph.
    Para(Vec<Inline>),
    /// A block of lines. Each item is one line.
    LineBlock(Vec<Vec<Inline>>),
    /// A code block with attributes.
    CodeBlock(Attr, Text),
    /// A raw block in the given format.
    RawBlock(Format, Text),
    /// A block quote.
    BlockQuote(Vec<Block>),
    /// An ordered list: the list attributes and the items.
    OrderedList(ListAttributes, Vec<Vec<Block>>),
    /// A bullet list. Each item is a list of blocks.
    BulletList(Vec<Vec<Block>>),
    /// A definition list. Each item is a term and its definitions.
    DefinitionList(Vec<(Vec<Inline>, Vec<Vec<Block>>)>),
    /// A heading: the level, the attributes and the text.
    Header(i64, Attr, Vec<Inline>),
    /// A horizontal rule.
    HorizontalRule,
    /// A table.
    Table(Box<Table>),
    /// A figure: the attributes, the caption and the content.
    Figure(Attr, Caption, Vec<Block>),
    /// A generic block container with attributes.
    Div(Attr, Vec<Block>),
}

/// An inline element.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    /// Text without spaces.
    Str(Text),
    /// Emphasized text.
    Emph(Vec<Inline>),
    /// Underlined text.
    Underline(Vec<Inline>),
    /// Strongly emphasized text.
    Strong(Vec<Inline>),
    /// Text with a line through it.
    Strikeout(Vec<Inline>),
    /// Superscript text.
    Superscript(Vec<Inline>),
    /// Subscript text.
    Subscript(Vec<Inline>),
    /// Text in small capitals.
    SmallCaps(Vec<Inline>),
    /// Quoted text.
    Quoted(QuoteType, Vec<Inline>),
    /// A citation: the citation data and the fallback text.
    Cite(Vec<Citation>, Vec<Inline>),
    /// Inline code with attributes.
    Code(Attr, Text),
    /// A space between words.
    Space,
    /// A line break in the source that is not a line break in the output.
    SoftBreak,
    /// A hard line break.
    LineBreak,
    /// TeX math.
    Math(MathType, Text),
    /// Raw inline content in the given format.
    RawInline(Format, Text),
    /// A hyperlink: the attributes, the link text and the target.
    Link(Attr, Vec<Inline>, Target),
    /// An image: the attributes, the description and the target.
    Image(Attr, Vec<Inline>, Target),
    /// A footnote or an endnote.
    Note(Vec<Block>),
    /// A generic inline container with attributes.
    Span(Attr, Vec<Inline>),
}

/// The attributes of an element: an identifier, classes and key-value pairs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct Attr {
    /// The identifier. An empty string means no identifier.
    pub id: Text,
    /// The classes.
    pub classes: Vec<Text>,
    /// The key-value pairs, in source order.
    pub attributes: Vec<(Text, Text)>,
}

impl Attr {
    /// Makes an attribute set.
    pub fn new(id: impl Into<Text>, classes: Vec<Text>, attributes: Vec<(Text, Text)>) -> Self {
        Attr {
            id: id.into(),
            classes,
            attributes,
        }
    }

    /// Returns `true` when there is no identifier, no class and no key-value pair.
    pub fn is_empty(&self) -> bool {
        self.id.is_empty() && self.classes.is_empty() && self.attributes.is_empty()
    }

    /// Returns `true` when the classes include `class`.
    pub fn has_class(&self, class: &str) -> bool {
        self.classes.iter().any(|c| c == class)
    }

    /// Returns the value of the first key-value pair with the given key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// The target of a link or an image: a URL and a title.
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct Target {
    /// The URL.
    pub url: Text,
    /// The title. An empty string means no title.
    pub title: Text,
}

impl Target {
    /// Makes a target.
    pub fn new(url: impl Into<Text>, title: impl Into<Text>) -> Self {
        Target {
            url: url.into(),
            title: title.into(),
        }
    }
}

/// The name of an output format, for raw content.
///
/// Two names are equal when they differ only in letter case, as in Pandoc.
#[derive(Debug, Clone, Default)]
pub struct Format(pub Text);

impl Format {
    /// Makes a format name.
    pub fn new(name: impl Into<Text>) -> Self {
        Format(name.into())
    }

    /// Returns the name as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for Format {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 || self.0.to_lowercase() == other.0.to_lowercase()
    }
}

impl Eq for Format {}

impl PartialEq<str> for Format {
    fn eq(&self, other: &str) -> bool {
        self.0 == other || self.0.to_lowercase() == other.to_lowercase()
    }
}

impl PartialEq<&str> for Format {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

/// The start number, number style and delimiter of an ordered list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListAttributes {
    /// The number of the first item.
    pub start: i64,
    /// The number style.
    pub style: ListNumberStyle,
    /// The delimiter after or around the number.
    pub delim: ListNumberDelim,
}

impl Default for ListAttributes {
    fn default() -> Self {
        ListAttributes {
            start: 1,
            style: ListNumberStyle::DefaultStyle,
            delim: ListNumberDelim::DefaultDelim,
        }
    }
}

/// The number style of an ordered list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListNumberStyle {
    /// The default style of the output format.
    DefaultStyle,
    /// A numbered example.
    Example,
    /// Decimal numbers.
    Decimal,
    /// Lowercase Roman numerals.
    LowerRoman,
    /// Uppercase Roman numerals.
    UpperRoman,
    /// Lowercase letters.
    LowerAlpha,
    /// Uppercase letters.
    UpperAlpha,
}

/// The delimiter of the numbers in an ordered list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListNumberDelim {
    /// The default delimiter of the output format.
    DefaultDelim,
    /// A period after the number.
    Period,
    /// A parenthesis after the number.
    OneParen,
    /// Parentheses around the number.
    TwoParens,
}

/// The type of quotation marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuoteType {
    /// Single quotation marks.
    SingleQuote,
    /// Double quotation marks.
    DoubleQuote,
}

/// The type of a math element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MathType {
    /// Math on its own line.
    DisplayMath,
    /// Math in a line of text.
    InlineMath,
}

/// One citation in a [`Inline::Cite`] element.
#[derive(Debug, Clone, PartialEq)]
pub struct Citation {
    /// The citation key.
    pub id: Text,
    /// The text before the citation.
    pub prefix: Vec<Inline>,
    /// The text after the citation, for example a locator.
    pub suffix: Vec<Inline>,
    /// The citation mode.
    pub mode: CitationMode,
    /// The number of the note that holds the citation.
    pub note_num: i64,
    /// A hash that makes citations with the same key different.
    pub hash: i64,
}

/// The mode of a citation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CitationMode {
    /// The author name is part of the sentence.
    AuthorInText,
    /// The author name is not shown.
    SuppressAuthor,
    /// A normal citation.
    NormalCitation,
}

/// The alignment of a table column or a table cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Alignment {
    /// Left alignment.
    AlignLeft,
    /// Right alignment.
    AlignRight,
    /// Center alignment.
    AlignCenter,
    /// The default alignment.
    AlignDefault,
}

/// The width of a table column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColWidth {
    /// A fraction of the text width.
    ColWidth(f64),
    /// The default width.
    ColWidthDefault,
}

/// The alignment and width of a table column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColSpec {
    /// The column alignment.
    pub align: Alignment,
    /// The column width.
    pub width: ColWidth,
}

impl Default for ColSpec {
    fn default() -> Self {
        ColSpec {
            align: Alignment::AlignDefault,
            width: ColWidth::ColWidthDefault,
        }
    }
}

/// The caption of a table or a figure.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Caption {
    /// The optional short caption.
    pub short: Option<Vec<Inline>>,
    /// The full caption.
    pub long: Vec<Block>,
}

/// A table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    /// The table attributes.
    pub attr: Attr,
    /// The table caption.
    pub caption: Caption,
    /// One specification for each column.
    pub col_specs: Vec<ColSpec>,
    /// The table head.
    pub head: TableHead,
    /// The table bodies.
    pub bodies: Vec<TableBody>,
    /// The table foot.
    pub foot: TableFoot,
}

/// The head of a table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableHead {
    /// The attributes of the head.
    pub attr: Attr,
    /// The rows of the head.
    pub rows: Vec<Row>,
}

/// A body of a table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableBody {
    /// The attributes of the body.
    pub attr: Attr,
    /// The number of row head columns.
    pub row_head_columns: i64,
    /// The intermediate head rows.
    pub head: Vec<Row>,
    /// The body rows.
    pub body: Vec<Row>,
}

/// The foot of a table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableFoot {
    /// The attributes of the foot.
    pub attr: Attr,
    /// The rows of the foot.
    pub rows: Vec<Row>,
}

/// A table row.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Row {
    /// The row attributes.
    pub attr: Attr,
    /// The cells of the row.
    pub cells: Vec<Cell>,
}

/// A table cell.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// The cell attributes.
    pub attr: Attr,
    /// The cell alignment.
    pub align: Alignment,
    /// The number of rows that the cell spans.
    pub row_span: i64,
    /// The number of columns that the cell spans.
    pub col_span: i64,
    /// The cell content.
    pub content: Vec<Block>,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            attr: Attr::default(),
            align: Alignment::AlignDefault,
            row_span: 1,
            col_span: 1,
            content: Vec::new(),
        }
    }
}
