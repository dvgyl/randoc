//! The encoder from the document model to JSON text, with the bytes of `aeson`.

use std::collections::BTreeMap;

use super::haskell::show_double;
use crate::{
    API_VERSION, Alignment, Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, ColWidth,
    Format, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, MathType, Meta, MetaValue,
    Pandoc, QuoteType, Row, Table, TableBody, TableFoot, TableHead, Target, Text,
};

/// A type that the encoder can write as JSON.
pub trait Encode {
    /// Appends the JSON text of the value.
    fn encode(&self, out: &mut String);
}

/// Appends a JSON string literal. Only `"`, `\` and control characters get escapes.
pub fn write_string(out: &mut String, text: &str) {
    out.push('"');
    let bytes = text.as_bytes();
    let mut start = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        let escape = match byte {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0x00..=0x1f => "",
            _ => continue,
        };
        out.push_str(&text[start..index]);
        if escape.is_empty() {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            out.push_str("\\u00");
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0xf)]));
        } else {
            out.push_str(escape);
        }
        start = index + 1;
    }
    out.push_str(&text[start..]);
    out.push('"');
}

impl Encode for Text {
    fn encode(&self, out: &mut String) {
        write_string(out, self);
    }
}

impl Encode for i64 {
    fn encode(&self, out: &mut String) {
        let mut buffer = [0u8; 20];
        let mut at = buffer.len();
        let mut rest = self.unsigned_abs();
        loop {
            at -= 1;
            buffer[at] = b'0' + (rest % 10) as u8;
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        if *self < 0 {
            out.push('-');
        }
        out.extend(buffer[at..].iter().map(|&b| char::from(b)));
    }
}

impl Encode for f64 {
    fn encode(&self, out: &mut String) {
        if self.is_nan() {
            out.push_str("null");
        } else if self.is_infinite() {
            out.push_str(if *self > 0.0 { "\"+inf\"" } else { "\"-inf\"" });
        } else {
            out.push_str(&show_double(*self));
        }
    }
}

impl Encode for bool {
    fn encode(&self, out: &mut String) {
        out.push_str(if *self { "true" } else { "false" });
    }
}

impl<T: Encode> Encode for Vec<T> {
    fn encode(&self, out: &mut String) {
        self.as_slice().encode(out);
    }
}

impl<T: Encode> Encode for [T] {
    fn encode(&self, out: &mut String) {
        out.push('[');
        for (index, item) in self.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            item.encode(out);
        }
        out.push(']');
    }
}

impl<T: Encode> Encode for Option<T> {
    fn encode(&self, out: &mut String) {
        match self {
            Some(value) => value.encode(out),
            None => out.push_str("null"),
        }
    }
}

impl<A: Encode, B: Encode> Encode for (A, B) {
    fn encode(&self, out: &mut String) {
        out.push('[');
        self.0.encode(out);
        out.push(',');
        self.1.encode(out);
        out.push(']');
    }
}

/// Appends the items of an array, separated by commas.
macro_rules! array {
    ($out:expr, $first:expr $(, $rest:expr)* $(,)?) => {{
        $out.push('[');
        $first.encode($out);
        $( $out.push(','); $rest.encode($out); )*
        $out.push(']');
    }};
}

/// Appends `{"t":"TAG"` and, when there is content, `,"c":` and the content.
fn open_tag(out: &mut String, tag: &str) {
    out.push_str("{\"t\":\"");
    out.push_str(tag);
    out.push('"');
}

fn tag_only(out: &mut String, tag: &str) {
    open_tag(out, tag);
    out.push('}');
}

fn tag_with<T: Encode + ?Sized>(out: &mut String, tag: &str, content: &T) {
    open_tag(out, tag);
    out.push_str(",\"c\":");
    content.encode(out);
    out.push('}');
}

impl Encode for Attr {
    fn encode(&self, out: &mut String) {
        out.push('[');
        self.id.encode(out);
        out.push(',');
        self.classes.encode(out);
        out.push(',');
        out.push('[');
        for (index, (key, value)) in self.attributes.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push('[');
            write_string(out, key);
            out.push(',');
            write_string(out, value);
            out.push(']');
        }
        out.push(']');
        out.push(']');
    }
}

impl Encode for Target {
    fn encode(&self, out: &mut String) {
        array!(out, self.url, self.title);
    }
}

impl Encode for Format {
    fn encode(&self, out: &mut String) {
        write_string(out, &self.0);
    }
}

impl Encode for ListAttributes {
    fn encode(&self, out: &mut String) {
        array!(out, self.start, self.style, self.delim);
    }
}

/// Implements `Encode` for an enumeration that `aeson` writes as `{"t": NAME}`.
macro_rules! encode_enum {
    ($type:ident { $($variant:ident),+ $(,)? }) => {
        impl Encode for $type {
            fn encode(&self, out: &mut String) {
                tag_only(out, match self {
                    $($type::$variant => stringify!($variant),)+
                });
            }
        }
    };
}

encode_enum!(ListNumberStyle {
    DefaultStyle,
    Example,
    Decimal,
    LowerRoman,
    UpperRoman,
    LowerAlpha,
    UpperAlpha
});
encode_enum!(ListNumberDelim {
    DefaultDelim,
    Period,
    OneParen,
    TwoParens
});
encode_enum!(QuoteType {
    SingleQuote,
    DoubleQuote
});
encode_enum!(MathType {
    DisplayMath,
    InlineMath
});
encode_enum!(CitationMode {
    AuthorInText,
    SuppressAuthor,
    NormalCitation
});
encode_enum!(Alignment {
    AlignLeft,
    AlignRight,
    AlignCenter,
    AlignDefault
});

impl Encode for ColWidth {
    fn encode(&self, out: &mut String) {
        match self {
            ColWidth::ColWidth(width) => tag_with(out, "ColWidth", width),
            ColWidth::ColWidthDefault => tag_only(out, "ColWidthDefault"),
        }
    }
}

impl Encode for ColSpec {
    fn encode(&self, out: &mut String) {
        array!(out, self.align, self.width);
    }
}

/// One step of the encoder for the types that can nest without a limit.
///
/// The encoder keeps the open steps in a vector instead of on the call
/// stack, so a deep tree that a caller builds cannot overflow the stack.
#[derive(Clone, Copy)]
enum Task<'a> {
    Lit(&'static str),
    Leaf(&'a dyn Encode),
    Block(&'a Block),
    Inline(&'a Inline),
    Meta(&'a MetaValue),
    Blocks(&'a [Block]),
    Inlines(&'a [Inline]),
    /// The items of a list of blocks from an index, after the first item.
    BlocksFrom(&'a [Block], usize),
    /// The items of a list of inlines from an index, after the first item.
    InlinesFrom(&'a [Inline], usize),
    BlockLists(&'a [Vec<Block>]),
    InlineLists(&'a [Vec<Inline>]),
    Metas(&'a [MetaValue]),
    MetaMap(&'a BTreeMap<Text, MetaValue>),
    Citation(&'a Citation),
    Citations(&'a [Citation]),
    Definitions(&'a [(Vec<Inline>, Vec<Vec<Block>>)]),
    Definition(&'a (Vec<Inline>, Vec<Vec<Block>>)),
    Table(&'a Table),
    Caption(&'a Caption),
    Head(&'a TableHead),
    Bodies(&'a [TableBody]),
    Body(&'a TableBody),
    Foot(&'a TableFoot),
    Rows(&'a [Row]),
    Row(&'a Row),
    Cells(&'a [Cell]),
    Cell(&'a Cell),
}

/// Pushes the steps of a JSON array or a constructor: `items` separated by
/// commas, then `close`. The caller writes the opening text.
fn push_seq<'a>(stack: &mut Vec<Task<'a>>, close: &'static str, items: &[Task<'a>]) {
    stack.push(Task::Lit(close));
    for (index, item) in items.iter().enumerate().rev() {
        stack.push(*item);
        if index > 0 {
            stack.push(Task::Lit(","));
        }
    }
}

/// Pushes the steps of a JSON array with one step for each item.
fn push_list<'a, T>(
    stack: &mut Vec<Task<'a>>,
    out: &mut String,
    items: &'a [T],
    task: fn(&'a T) -> Task<'a>,
) {
    out.push('[');
    stack.push(Task::Lit("]"));
    for (index, item) in items.iter().enumerate().rev() {
        stack.push(task(item));
        if index > 0 {
            stack.push(Task::Lit(","));
        }
    }
}

/// Writes `{"t":"TAG","c":[` and pushes the fields and the end `]}`.
fn constructor<'a>(stack: &mut Vec<Task<'a>>, out: &mut String, tag: &str, fields: &[Task<'a>]) {
    open_tag(out, tag);
    out.push_str(",\"c\":[");
    push_seq(stack, "]}", fields);
}

/// Writes `{"t":"TAG","c":` and pushes the content and the end `}`.
fn with_content<'a>(stack: &mut Vec<Task<'a>>, out: &mut String, tag: &str, content: Task<'a>) {
    open_tag(out, tag);
    out.push_str(",\"c\":");
    stack.push(Task::Lit("}"));
    stack.push(content);
}

/// Runs the encoder from one step until no step is left.
fn run(first: Task<'_>, out: &mut String) {
    let mut stack = vec![first];
    while let Some(task) = stack.pop() {
        step(task, &mut stack, out);
    }
}

fn step<'a>(task: Task<'a>, stack: &mut Vec<Task<'a>>, out: &mut String) {
    use Task as T;
    match task {
        T::Lit(text) => out.push_str(text),
        T::Leaf(value) => value.encode(out),
        T::Block(block) => block_step(block, stack, out),
        T::Inline(inline) => inline_step(inline, stack, out),
        T::Meta(meta) => match meta {
            MetaValue::MetaMap(map) => with_content(stack, out, "MetaMap", T::MetaMap(map)),
            MetaValue::MetaList(items) => with_content(stack, out, "MetaList", T::Metas(items)),
            MetaValue::MetaBool(flag) => tag_with(out, "MetaBool", flag),
            MetaValue::MetaString(text) => tag_with(out, "MetaString", text),
            MetaValue::MetaInlines(inlines) => {
                with_content(stack, out, "MetaInlines", T::Inlines(inlines))
            }
            MetaValue::MetaBlocks(blocks) => {
                with_content(stack, out, "MetaBlocks", T::Blocks(blocks))
            }
        },
        T::Blocks(items) => {
            out.push('[');
            stack.push(T::Lit("]"));
            blocks_from(items, 0, stack, out);
        }
        T::Inlines(items) => {
            out.push('[');
            stack.push(T::Lit("]"));
            inlines_from(items, 0, stack, out);
        }
        T::BlocksFrom(items, index) => blocks_from(items, index, stack, out),
        T::InlinesFrom(items, index) => inlines_from(items, index, stack, out),
        T::BlockLists(items) => push_list(stack, out, items, |item| T::Blocks(item)),
        T::InlineLists(items) => push_list(stack, out, items, |item| T::Inlines(item)),
        T::Metas(items) => push_list(stack, out, items, T::Meta),
        T::Citations(items) => push_list(stack, out, items, T::Citation),
        T::Rows(rows) => push_list(stack, out, rows, T::Row),
        T::Row(row) => {
            out.push('[');
            push_seq(stack, "]", &[T::Leaf(&row.attr), T::Cells(&row.cells)]);
        }
        T::Cells(cells) => push_list(stack, out, cells, T::Cell),
        T::Cell(cell) => {
            out.push('[');
            push_seq(
                stack,
                "]",
                &[
                    T::Leaf(&cell.attr),
                    T::Leaf(&cell.align),
                    T::Leaf(&cell.row_span),
                    T::Leaf(&cell.col_span),
                    T::Blocks(&cell.content),
                ],
            );
        }
        T::MetaMap(map) => {
            out.push('{');
            stack.push(T::Lit("}"));
            let len = map.len();
            for (index, (key, value)) in map.iter().rev().enumerate() {
                stack.push(T::Meta(value));
                stack.push(T::Lit(":"));
                stack.push(T::Leaf(key));
                if index + 1 < len {
                    stack.push(T::Lit(","));
                }
            }
        }
        T::Citation(citation) => {
            out.push_str("{\"citationId\":");
            write_string(out, &citation.id);
            out.push_str(",\"citationPrefix\":");
            stack.push(T::Lit("}"));
            stack.push(T::Leaf(&citation.hash));
            stack.push(T::Lit(",\"citationHash\":"));
            stack.push(T::Leaf(&citation.note_num));
            stack.push(T::Lit(",\"citationNoteNum\":"));
            stack.push(T::Leaf(&citation.mode));
            stack.push(T::Lit(",\"citationMode\":"));
            stack.push(T::Inlines(&citation.suffix));
            stack.push(T::Lit(",\"citationSuffix\":"));
            stack.push(T::Inlines(&citation.prefix));
        }
        T::Definitions(items) => push_list(stack, out, items, T::Definition),
        T::Definition((term, definitions)) => {
            out.push('[');
            push_seq(stack, "]", &[T::Inlines(term), T::BlockLists(definitions)]);
        }
        T::Table(table) => {
            out.push('[');
            push_seq(
                stack,
                "]",
                &[
                    T::Leaf(&table.attr),
                    T::Caption(&table.caption),
                    T::Leaf(&table.col_specs),
                    T::Head(&table.head),
                    T::Bodies(&table.bodies),
                    T::Foot(&table.foot),
                ],
            );
        }
        T::Caption(caption) => {
            out.push('[');
            let short = match &caption.short {
                Some(inlines) => T::Inlines(inlines),
                None => T::Lit("null"),
            };
            push_seq(stack, "]", &[short, T::Blocks(&caption.long)]);
        }
        T::Head(head) => {
            out.push('[');
            push_seq(stack, "]", &[T::Leaf(&head.attr), T::Rows(&head.rows)]);
        }
        T::Foot(foot) => {
            out.push('[');
            push_seq(stack, "]", &[T::Leaf(&foot.attr), T::Rows(&foot.rows)]);
        }
        T::Bodies(bodies) => push_list(stack, out, bodies, T::Body),
        T::Body(body) => {
            out.push('[');
            push_seq(
                stack,
                "]",
                &[
                    T::Leaf(&body.attr),
                    T::Leaf(&body.row_head_columns),
                    T::Rows(&body.head),
                    T::Rows(&body.body),
                ],
            );
        }
    }
}

/// Writes a constructor with two fields that cannot nest, with no step on the stack.
fn leaf_pair(out: &mut String, tag: &str, first: &dyn Encode, text: &str) {
    open_tag(out, tag);
    out.push_str(",\"c\":[");
    first.encode(out);
    out.push(',');
    write_string(out, text);
    out.push_str("]}");
}

/// Writes the items of a list from `index`. A leaf item is written at once.
/// For an item with content, the rest of the list waits on the stack.
fn inlines_from<'a>(
    items: &'a [Inline],
    index: usize,
    stack: &mut Vec<Task<'a>>,
    out: &mut String,
) {
    for (offset, item) in items.iter().enumerate().skip(index) {
        if offset > 0 {
            out.push(',');
        }
        let leaf = matches!(
            item,
            Inline::Str(_)
                | Inline::Space
                | Inline::SoftBreak
                | Inline::LineBreak
                | Inline::Code(..)
                | Inline::Math(..)
                | Inline::RawInline(..)
        );
        if !leaf {
            stack.push(Task::InlinesFrom(items, offset + 1));
        }
        inline_step(item, stack, out);
        if !leaf {
            return;
        }
    }
}

fn blocks_from<'a>(items: &'a [Block], index: usize, stack: &mut Vec<Task<'a>>, out: &mut String) {
    for (offset, item) in items.iter().enumerate().skip(index) {
        if offset > 0 {
            out.push(',');
        }
        let leaf = matches!(
            item,
            Block::HorizontalRule | Block::CodeBlock(..) | Block::RawBlock(..)
        );
        if !leaf {
            stack.push(Task::BlocksFrom(items, offset + 1));
        }
        block_step(item, stack, out);
        if !leaf {
            return;
        }
    }
}

fn block_step<'a>(block: &'a Block, stack: &mut Vec<Task<'a>>, out: &mut String) {
    use Task as T;
    match block {
        Block::Plain(inlines) => with_content(stack, out, "Plain", T::Inlines(inlines)),
        Block::Para(inlines) => with_content(stack, out, "Para", T::Inlines(inlines)),
        Block::LineBlock(lines) => with_content(stack, out, "LineBlock", T::InlineLists(lines)),
        Block::CodeBlock(attr, text) => leaf_pair(out, "CodeBlock", attr, text),
        Block::RawBlock(format, text) => leaf_pair(out, "RawBlock", format, text),
        Block::BlockQuote(blocks) => with_content(stack, out, "BlockQuote", T::Blocks(blocks)),
        Block::OrderedList(attrs, items) => constructor(
            stack,
            out,
            "OrderedList",
            &[T::Leaf(attrs), T::BlockLists(items)],
        ),
        Block::BulletList(items) => with_content(stack, out, "BulletList", T::BlockLists(items)),
        Block::DefinitionList(items) => {
            with_content(stack, out, "DefinitionList", T::Definitions(items))
        }
        Block::Header(level, attr, inlines) => constructor(
            stack,
            out,
            "Header",
            &[T::Leaf(level), T::Leaf(attr), T::Inlines(inlines)],
        ),
        Block::HorizontalRule => tag_only(out, "HorizontalRule"),
        Block::Table(table) => with_content(stack, out, "Table", T::Table(table)),
        Block::Figure(attr, caption, blocks) => constructor(
            stack,
            out,
            "Figure",
            &[T::Leaf(attr), T::Caption(caption), T::Blocks(blocks)],
        ),
        Block::Div(attr, blocks) => {
            constructor(stack, out, "Div", &[T::Leaf(attr), T::Blocks(blocks)])
        }
    }
}

fn inline_step<'a>(inline: &'a Inline, stack: &mut Vec<Task<'a>>, out: &mut String) {
    use Task as T;
    match inline {
        Inline::Str(text) => tag_with(out, "Str", text),
        Inline::Emph(inlines) => with_content(stack, out, "Emph", T::Inlines(inlines)),
        Inline::Underline(inlines) => with_content(stack, out, "Underline", T::Inlines(inlines)),
        Inline::Strong(inlines) => with_content(stack, out, "Strong", T::Inlines(inlines)),
        Inline::Strikeout(inlines) => with_content(stack, out, "Strikeout", T::Inlines(inlines)),
        Inline::Superscript(inlines) => {
            with_content(stack, out, "Superscript", T::Inlines(inlines))
        }
        Inline::Subscript(inlines) => with_content(stack, out, "Subscript", T::Inlines(inlines)),
        Inline::SmallCaps(inlines) => with_content(stack, out, "SmallCaps", T::Inlines(inlines)),
        Inline::Quoted(kind, inlines) => {
            constructor(stack, out, "Quoted", &[T::Leaf(kind), T::Inlines(inlines)])
        }
        Inline::Cite(citations, inlines) => constructor(
            stack,
            out,
            "Cite",
            &[T::Citations(citations), T::Inlines(inlines)],
        ),
        Inline::Code(attr, text) => leaf_pair(out, "Code", attr, text),
        Inline::Space => tag_only(out, "Space"),
        Inline::SoftBreak => tag_only(out, "SoftBreak"),
        Inline::LineBreak => tag_only(out, "LineBreak"),
        Inline::Math(kind, text) => leaf_pair(out, "Math", kind, text),
        Inline::RawInline(format, text) => leaf_pair(out, "RawInline", format, text),
        Inline::Link(attr, inlines, target) => constructor(
            stack,
            out,
            "Link",
            &[T::Leaf(attr), T::Inlines(inlines), T::Leaf(target)],
        ),
        Inline::Image(attr, inlines, target) => constructor(
            stack,
            out,
            "Image",
            &[T::Leaf(attr), T::Inlines(inlines), T::Leaf(target)],
        ),
        Inline::Note(blocks) => with_content(stack, out, "Note", T::Blocks(blocks)),
        Inline::Span(attr, inlines) => {
            constructor(stack, out, "Span", &[T::Leaf(attr), T::Inlines(inlines)])
        }
    }
}

/// Implements `Encode` with the explicit stack for a type that can nest.
macro_rules! encode_with_stack {
    ($($type:ty => $task:expr;)+) => {
        $(
            impl Encode for $type {
                fn encode(&self, out: &mut String) {
                    run(($task)(self), out);
                }
            }
        )+
    };
}

encode_with_stack! {
    Block => Task::Block;
    Inline => Task::Inline;
    MetaValue => Task::Meta;
    Citation => Task::Citation;
    Table => Task::Table;
    Caption => Task::Caption;
    TableHead => Task::Head;
    TableFoot => Task::Foot;
    BTreeMap<Text, MetaValue> => Task::MetaMap;
    Row => Task::Row;
    Cell => Task::Cell;
    TableBody => Task::Body;
}

impl Encode for Meta {
    fn encode(&self, out: &mut String) {
        run(Task::MetaMap(&self.0), out);
    }
}

impl Encode for Pandoc {
    fn encode(&self, out: &mut String) {
        out.push_str("{\"pandoc-api-version\":");
        API_VERSION.as_slice().encode(out);
        out.push_str(",\"meta\":");
        let mut stack = vec![
            Task::Lit("}"),
            Task::Blocks(&self.blocks),
            Task::Lit(",\"blocks\":"),
            Task::MetaMap(&self.meta.0),
        ];
        while let Some(task) = stack.pop() {
            step(task, &mut stack, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json<T: Encode + ?Sized>(value: &T) -> String {
        let mut out = String::new();
        value.encode(&mut out);
        out
    }

    #[test]
    fn numbers_use_the_aeson_forms() {
        assert_eq!(json(&i64::MIN), "-9223372036854775808");
        assert_eq!(json(&0i64), "0");
        assert_eq!(json(&f64::NAN), "null");
        assert_eq!(json(&f64::INFINITY), "\"+inf\"");
        assert_eq!(json(&f64::NEG_INFINITY), "\"-inf\"");
        assert_eq!(json(&-0.0f64), "-0.0");
        assert_eq!(json(&1e23f64), "9.999999999999999e22");
    }

    #[test]
    fn strings_escape_only_quotes_backslashes_and_controls() {
        assert_eq!(
            json(&Text::from(
                "a\u{1}\u{1f}\u{7f}\u{8}\u{c}\n\t<>&\u{e9}/\\\"\r"
            )),
            "\"a\\u0001\\u001f\u{7f}\\u0008\\u000c\\n\\t<>&\u{e9}/\\\\\\\"\\r\""
        );
    }
}
