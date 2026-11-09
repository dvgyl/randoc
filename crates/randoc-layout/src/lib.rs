//! Text layout with line wrapping for Randoc writers.
//!
//! This crate is a port of the Haskell library `doclayout` 0.6, which
//! Pandoc 3.13 uses to lay out plain-text output. A [`Doc`] holds text with
//! breakable spaces, line breaks, blank lines, indentation, prefixes and
//! fixed-width blocks. [`Doc::render`] lays the document out at a line width
//! and gives the same bytes as Pandoc.
//!
//! The functions have the names of the `doclayout` functions in snake case.
//! The operators of `doclayout` map to Rust as follows:
//!
//! | `doclayout` | Rust |
//! | --- | --- |
//! | `a <> b` | `a + b` |
//! | `a <+> b` | `a.spaced(b)` |
//! | `a $$ b` | `a.above(b)` |
//! | `a $+$ b` | `a.above_blank(b)` |
//!
//! ```
//! use randoc_layout::{hsep, literal, nest};
//!
//! let doc = nest(2, hsep(["one", "two", "three"].map(literal)));
//! assert_eq!(doc.render(Some(8)), "  one\n  two\n  three");
//! ```

mod render;
mod style;
mod tables;
mod width;

use std::ops::{Add, AddAssign};

pub use style::Color;
pub use width::{char_width, real_length};

use render::Line;
use style::StyleReq;
use width::str_length;

/// A document that can be rendered at a given line width.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Doc(Node);

#[derive(Debug, Clone, PartialEq, Default)]
enum Node {
    #[default]
    Empty,
    /// Text without newlines, with its width.
    Text(isize, String),
    /// A block with a width and its lines.
    Block(isize, Vec<Line>),
    /// A border that expands to the height of the adjacent blocks.
    VFill(isize, String),
    /// Content with each line prefixed by the text.
    Prefixed(String, Box<Doc>),
    /// Content that is rendered only before non-blank content.
    BeforeNonBlank(Box<Doc>),
    /// Content laid out without the current prefix.
    Flush(Box<Doc>),
    BreakingSpace,
    /// Text that is rendered only at the start of a line.
    AfterBreak(String),
    CarriageReturn,
    NewLine,
    BlankLines(usize),
    /// Two or more documents. No item is empty or a concatenation, so a
    /// long chain of concatenations does not nest.
    Concat(Vec<Doc>),
    Styled(StyleReq, Box<Doc>),
    Linked(String, Box<Doc>),
}

fn to_isize(n: usize) -> isize {
    isize::try_from(n).unwrap_or(isize::MAX)
}

fn to_usize(n: isize) -> usize {
    usize::try_from(n).unwrap_or(0)
}

impl Doc {
    /// Makes a document from literal text. Each newline in the text is a
    /// forced line break. Spaces in the text are not breakable.
    pub fn literal(text: impl Into<String>) -> Self {
        let text = text.into();
        if !text.contains('\n') {
            return if text.is_empty() {
                Doc(Node::Empty)
            } else {
                Doc(Node::Text(str_length(&text), text))
            };
        }
        let mut doc = Doc::default();
        for (index, piece) in text.split('\n').enumerate() {
            if index > 0 {
                doc += Doc(Node::NewLine);
            }
            if !piece.is_empty() {
                doc += Doc(Node::Text(str_length(piece), piece.to_owned()));
            }
        }
        doc
    }

    /// Returns `true` when the document is the empty document.
    ///
    /// A document that renders as empty text can still be non-empty, for
    /// example `cr()`.
    pub fn is_empty(&self) -> bool {
        matches!(self.0, Node::Empty)
    }

    /// Renders the document as plain text. A width of `None` disables line
    /// wrapping.
    pub fn render(&self, width: Option<usize>) -> String {
        render::render_plain(self, width.map(to_isize))
    }

    /// Renders the document with ANSI escape sequences for styles and OSC 8
    /// sequences for links. A width of `None` disables line wrapping.
    pub fn render_ansi(&self, width: Option<usize>) -> String {
        render::render_ansi(self, width.map(to_isize))
    }

    /// Concatenates two documents with a breakable space between them
    /// (`<+>`). An empty operand gives the other operand.
    #[must_use]
    pub fn spaced(self, other: Doc) -> Doc {
        if self.is_empty() {
            other
        } else if other.is_empty() {
            self
        } else {
            self + space() + other
        }
    }

    /// Puts this document above the other document (`$$`). An empty operand
    /// gives the other operand.
    #[must_use]
    pub fn above(self, other: Doc) -> Doc {
        if self.is_empty() {
            other
        } else if other.is_empty() {
            self
        } else {
            self + cr() + other
        }
    }

    /// Puts this document above the other document with a blank line
    /// between them (`$+$`). An empty operand gives the other operand.
    #[must_use]
    pub fn above_blank(self, other: Doc) -> Doc {
        if self.is_empty() {
            other
        } else if other.is_empty() {
            self
        } else {
            self + blankline() + other
        }
    }

    /// Returns the width of the widest line of the document when it is not
    /// wrapped.
    pub fn offset(&self) -> usize {
        let (longest, current) = get_offset(&|_| false, 0, (0, 0), self);
        to_usize(longest.max(current))
    }

    /// Returns the minimal width of the document when it is wrapped at each
    /// breakable space.
    pub fn min_offset(&self) -> usize {
        let (longest, current) = get_offset(&|c| c > 0, 0, (0, 0), self);
        to_usize(longest.max(current))
    }

    /// Returns the column after the last character of the document when
    /// the document starts at column `column` and is not wrapped.
    pub fn update_column(&self, column: usize) -> usize {
        to_usize(get_offset(&|_| false, 0, (0, to_isize(column)), self).1)
    }

    /// Returns the number of lines of the document rendered without
    /// wrapping.
    pub fn height(&self) -> usize {
        self.render(None).matches('\n').count() + 1
    }
}

impl From<&str> for Doc {
    fn from(text: &str) -> Doc {
        Doc::literal(text)
    }
}

impl From<String> for Doc {
    fn from(text: String) -> Doc {
        Doc::literal(text)
    }
}

/// Concatenates two documents horizontally (`<>`).
impl Add for Doc {
    type Output = Doc;

    fn add(self, other: Doc) -> Doc {
        match (self.0, other.0) {
            (Node::Empty, node) | (node, Node::Empty) => Doc(node),
            (Node::Concat(mut first), Node::Concat(second)) => {
                first.extend(second);
                Doc(Node::Concat(first))
            }
            (Node::Concat(mut first), node) => {
                first.push(Doc(node));
                Doc(Node::Concat(first))
            }
            (node, Node::Concat(second)) => {
                let mut items = Vec::with_capacity(second.len() + 1);
                items.push(Doc(node));
                items.extend(second);
                Doc(Node::Concat(items))
            }
            (first, second) => Doc(Node::Concat(vec![Doc(first), Doc(second)])),
        }
    }
}

/// Appends a document horizontally (`<>`).
impl AddAssign for Doc {
    fn add_assign(&mut self, other: Doc) {
        let this = std::mem::take(self);
        *self = this + other;
    }
}

impl std::iter::Sum for Doc {
    fn sum<I: Iterator<Item = Doc>>(iter: I) -> Doc {
        hcat(iter)
    }
}

/// Computes `(longest line, current column)`. `prefix` is the width of the
/// current prefix, to which the column returns after a line break.
fn get_offset(
    break_when: &dyn Fn(isize) -> bool,
    prefix: isize,
    start: (isize, isize),
    doc: &Doc,
) -> (isize, isize) {
    let (mut longest, mut column) = start;
    let mut stack = vec![doc];
    while let Some(d) = stack.pop() {
        match &d.0 {
            Node::Text(n, _) | Node::Block(n, _) | Node::VFill(n, _) => column += n,
            Node::Empty => {}
            Node::Styled(_, inner) | Node::Linked(_, inner) => {
                (longest, column) = get_offset(break_when, prefix, (longest, column), inner);
            }
            Node::CarriageReturn | Node::NewLine | Node::BlankLines(_) => {
                longest = longest.max(column);
                column = prefix;
            }
            Node::Prefixed(text, inner) => {
                let inner_prefix = prefix + str_length(text);
                let start_column = if column <= prefix {
                    inner_prefix
                } else {
                    column
                };
                (longest, column) =
                    get_offset(break_when, inner_prefix, (longest, start_column), inner);
            }
            Node::BeforeNonBlank(inner) => {
                if stack.last().is_some_and(|next| is_non_blank(next)) {
                    push_items(&mut stack, inner);
                }
            }
            Node::Flush(inner) => {
                (longest, column) = get_offset(break_when, 0, (longest, column), inner);
            }
            Node::BreakingSpace => {
                if break_when(column) {
                    longest = longest.max(column);
                    column = prefix;
                } else {
                    column += 1;
                }
            }
            Node::AfterBreak(text) => {
                if column == prefix {
                    column += str_length(text);
                }
            }
            Node::Concat(_) => push_items(&mut stack, d),
        }
    }
    (longest, column)
}

fn push_items<'a>(stack: &mut Vec<&'a Doc>, doc: &'a Doc) {
    match &doc.0 {
        Node::Concat(items) => stack.extend(items.iter().rev()),
        _ => stack.push(doc),
    }
}

fn is_non_blank(doc: &Doc) -> bool {
    match &doc.0 {
        Node::Text(..) => true,
        Node::BeforeNonBlank(inner) | Node::Flush(inner) => is_non_blank(inner),
        Node::Concat(items) => items.first().is_some_and(is_non_blank),
        _ => false,
    }
}

/// The empty document.
pub fn empty() -> Doc {
    Doc::default()
}

/// Makes a document from literal text. See [`Doc::literal`].
pub fn literal(text: impl Into<String>) -> Doc {
    Doc::literal(text)
}

/// Makes a document from literal text. This is the same as [`literal`].
pub fn text(text: impl Into<String>) -> Doc {
    Doc::literal(text)
}

/// Makes a document from one character.
pub fn char(c: char) -> Doc {
    Doc::literal(c.to_string())
}

/// A breakable space. It renders as a space or as a line break, as the
/// line width requires. Spaces at the start or the end of a line go away.
pub fn space() -> Doc {
    Doc(Node::BreakingSpace)
}

/// A line break that has no effect at the start of a line.
pub fn cr() -> Doc {
    Doc(Node::CarriageReturn)
}

/// A blank line, unless there is a blank line already.
pub fn blankline() -> Doc {
    Doc(Node::BlankLines(1))
}

/// `n` blank lines, unless there are `n` blank lines already.
/// `blanklines(m) + blanklines(n)` has the effect of `blanklines(max(m, n))`.
pub fn blanklines(n: usize) -> Doc {
    Doc(Node::BlankLines(n))
}

/// Concatenates documents horizontally.
pub fn hcat(docs: impl IntoIterator<Item = Doc>) -> Doc {
    let mut out = Doc::default();
    for doc in docs {
        out += doc;
    }
    out
}

fn join_with(docs: impl IntoIterator<Item = Doc>, separator: fn() -> Doc) -> Doc {
    let mut out = Doc::default();
    for doc in docs.into_iter().filter(|d| !d.is_empty()) {
        if !out.is_empty() {
            out += separator();
        }
        out += doc;
    }
    out
}

/// Concatenates documents with a breakable space between each pair.
/// Empty documents are skipped.
pub fn hsep(docs: impl IntoIterator<Item = Doc>) -> Doc {
    join_with(docs, space)
}

/// Puts documents above each other. Empty documents are skipped.
pub fn vcat(docs: impl IntoIterator<Item = Doc>) -> Doc {
    join_with(docs, cr)
}

/// Puts documents above each other with a blank line between each pair.
/// Empty documents are skipped.
pub fn vsep(docs: impl IntoIterator<Item = Doc>) -> Doc {
    join_with(docs, blankline)
}

/// Uses `prefix` as a prefix for each line of `doc`. The first line gets
/// the prefix only when `doc` starts at the start of a line. A prefix at
/// the start of an empty line loses its trailing spaces.
pub fn prefixed(prefix: &str, doc: Doc) -> Doc {
    if doc.is_empty() {
        doc
    } else {
        Doc(Node::Prefixed(prefix.to_owned(), Box::new(doc)))
    }
}

/// Lays `doc` out against the left margin, without the current prefix.
pub fn flush(doc: Doc) -> Doc {
    if doc.is_empty() {
        doc
    } else {
        Doc(Node::Flush(Box::new(doc)))
    }
}

/// Indents `doc` by `indent` spaces.
pub fn nest(indent: usize, doc: Doc) -> Doc {
    prefixed(&" ".repeat(indent), doc)
}

/// A hanging indent: `start`, then `doc` with an indent of `indent` spaces
/// on each line but the first.
pub fn hang(indent: usize, start: Doc, doc: Doc) -> Doc {
    start + nest(indent, doc)
}

/// Includes `doc` only when the content that follows it is not blank.
pub fn before_non_blank(doc: Doc) -> Doc {
    Doc(Node::BeforeNonBlank(Box::new(doc)))
}

/// Makes each breakable space in `doc` a normal space.
pub fn nowrap(doc: Doc) -> Doc {
    let wrap = |inner: Box<Doc>| Box::new(nowrap(*inner));
    Doc(match doc.0 {
        Node::BreakingSpace => Node::Text(1, " ".to_owned()),
        Node::Concat(items) => Node::Concat(items.into_iter().map(nowrap).collect()),
        Node::Styled(style, inner) => Node::Styled(style, wrap(inner)),
        Node::Linked(target, inner) => Node::Linked(target, wrap(inner)),
        Node::Prefixed(prefix, inner) => Node::Prefixed(prefix, wrap(inner)),
        Node::BeforeNonBlank(inner) => Node::BeforeNonBlank(wrap(inner)),
        Node::Flush(inner) => Node::Flush(wrap(inner)),
        other => other,
    })
}

/// Text that is rendered only at the start of a line, for example to
/// escape a `.` at the start of a line in roff.
pub fn after_break(text: &str) -> Doc {
    Doc(Node::AfterBreak(text.to_owned()))
}

/// Removes trailing spaces, line breaks and blank lines from `doc`.
pub fn chomp(doc: Doc) -> Doc {
    match doc.0 {
        Node::BlankLines(_) | Node::NewLine | Node::CarriageReturn | Node::BreakingSpace => empty(),
        Node::Prefixed(prefix, inner) => Doc(Node::Prefixed(prefix, Box::new(chomp(*inner)))),
        Node::Concat(mut items) => {
            while let Some(last) = items.pop() {
                let chomped = chomp(last);
                if !chomped.is_empty() {
                    return hcat(items) + chomped;
                }
            }
            empty()
        }
        other => Doc(other),
    }
}

/// Removes leading line breaks and blank lines from `doc`.
pub fn nestle(doc: Doc) -> Doc {
    match doc.0 {
        Node::BlankLines(_) | Node::NewLine => empty(),
        Node::Concat(items) => {
            let mut rest = items.into_iter().peekable();
            while let Some(first) = rest.next_if(|d| {
                matches!(
                    d.0,
                    Node::BlankLines(_) | Node::NewLine | Node::CarriageReturn
                )
            }) {
                if rest.peek().is_none() {
                    return nestle(first);
                }
            }
            let items: Vec<Doc> = rest.collect();
            match <[Doc; 1]>::try_from(items) {
                Ok([single]) => nestle(single),
                Err(items) => Doc(Node::Concat(items)),
            }
        }
        other => Doc(other),
    }
}

/// Encloses `contents` between `start` and `end`.
pub fn inside(start: Doc, end: Doc, contents: Doc) -> Doc {
    start + contents + end
}

/// Puts `doc` in curly braces.
pub fn braces(doc: Doc) -> Doc {
    inside(char('{'), char('}'), doc)
}

/// Puts `doc` in square brackets.
pub fn brackets(doc: Doc) -> Doc {
    inside(char('['), char(']'), doc)
}

/// Puts `doc` in parentheses.
pub fn parens(doc: Doc) -> Doc {
    inside(char('('), char(')'), doc)
}

/// Puts `doc` in single quotes.
pub fn quotes(doc: Doc) -> Doc {
    inside(char('\''), char('\''), doc)
}

/// Puts `doc` in double quotes.
pub fn double_quotes(doc: Doc) -> Doc {
    inside(char('"'), char('"'), doc)
}

fn block(width: usize, doc: &Doc, align: fn(isize, Line) -> Line) -> Doc {
    let width = if width < 1 && !doc.is_empty() {
        1
    } else {
        to_isize(width)
    };
    let output = render::prerender(doc, Some(width));
    let lines = render::chop(width, output)
        .into_iter()
        .map(|line| align(width, line))
        .collect();
    Doc(Node::Block(width, lines))
}

fn pad_left(count: isize, line: Line) -> Line {
    if count <= 0 {
        return line;
    }
    let mut out = Vec::with_capacity(line.len() + 1);
    out.push(render::Attr {
        link: None,
        font: style::Font::default(),
        text: " ".repeat(count.unsigned_abs()),
    });
    out.extend(line);
    out
}

/// A block of width `width` with the content of `doc`, aligned to the left.
/// The content is wrapped at the width, and longer words are split.
pub fn lblock(width: usize, doc: Doc) -> Doc {
    block(width, &doc, |_, line| line)
}

/// A block like [`lblock`], with the content aligned to the right.
pub fn rblock(width: usize, doc: Doc) -> Doc {
    block(width, &doc, |w, line| {
        pad_left(w - render::line_length(&line), line)
    })
}

/// A block like [`lblock`], with the content centered.
pub fn cblock(width: usize, doc: Doc) -> Doc {
    block(width, &doc, |w, line| {
        pad_left((w - render::line_length(&line)).div_euclid(2), line)
    })
}

/// A border that expands to the height of the adjacent blocks. Each line of
/// the border contains `text`.
pub fn vfill(text: &str) -> Doc {
    Doc(Node::VFill(str_length(text), text.to_owned()))
}

fn styled(style: StyleReq, doc: Doc) -> Doc {
    if doc.is_empty() {
        doc
    } else {
        Doc(Node::Styled(style, Box::new(doc)))
    }
}

/// Renders `doc` in bold with [`Doc::render_ansi`].
pub fn bold(doc: Doc) -> Doc {
    styled(StyleReq::Bold, doc)
}

/// Renders `doc` in italics with [`Doc::render_ansi`].
pub fn italic(doc: Doc) -> Doc {
    styled(StyleReq::Italic, doc)
}

/// Underlines `doc` with [`Doc::render_ansi`].
pub fn underlined(doc: Doc) -> Doc {
    styled(StyleReq::Underline, doc)
}

/// Puts a line through `doc` with [`Doc::render_ansi`].
pub fn strikeout(doc: Doc) -> Doc {
    styled(StyleReq::Strikeout, doc)
}

/// Sets the foreground color of `doc` for [`Doc::render_ansi`].
pub fn fg(color: Color, doc: Doc) -> Doc {
    styled(StyleReq::Foreground(color), doc)
}

/// Sets the background color of `doc` for [`Doc::render_ansi`].
pub fn bg(color: Color, doc: Doc) -> Doc {
    styled(StyleReq::Background(color), doc)
}

/// Makes `doc` a hyperlink to `target` for [`Doc::render_ansi`]. A link
/// inside a link has no effect.
pub fn link(target: &str, doc: Doc) -> Doc {
    Doc(Node::Linked(target.to_owned(), Box::new(doc)))
}

#[cfg(test)]
mod tests {
    //! Compares the crate with the oracle results in `tests/vectors.txt`.
    //! `tests/vectors.lua` makes that file with Pandoc 3.13.

    use super::*;

    const VECTORS: &str = include_str!("../tests/vectors.txt");

    #[derive(Debug, Clone)]
    enum Expr {
        Str(String),
        Atom(String),
        List(Vec<Expr>),
    }

    fn unquote(src: &str) -> (String, &str) {
        let mut out = String::new();
        let mut chars = src.char_indices();
        assert_eq!(chars.next().map(|(_, c)| c), Some('"'));
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => return (out, &src[i + 1..]),
                '\\' => match chars.next() {
                    Some((_, 'u')) => {
                        let rest = &src[i + 3..];
                        let close = rest.find('}').unwrap_or(0);
                        let code = u32::from_str_radix(&rest[..close], 16).unwrap_or(0);
                        out.push(std::char::from_u32(code).unwrap_or('?'));
                        for _ in 0..close + 2 {
                            chars.next();
                        }
                    }
                    Some((_, other)) => out.push(other),
                    None => break,
                },
                other => out.push(other),
            }
        }
        (out, "")
    }

    fn parse(src: &str) -> (Expr, &str) {
        let src = src.trim_start();
        if let Some(mut rest) = src.strip_prefix('(') {
            let mut items = Vec::new();
            loop {
                rest = rest.trim_start();
                if let Some(after) = rest.strip_prefix(')') {
                    return (Expr::List(items), after);
                }
                let (item, after) = parse(rest);
                items.push(item);
                rest = after;
            }
        }
        if src.starts_with('"') {
            let (text, rest) = unquote(src);
            return (Expr::Str(text), rest);
        }
        let end = src
            .find(|c: char| c.is_whitespace() || c == '(' || c == ')')
            .unwrap_or(src.len());
        (Expr::Atom(src[..end].to_owned()), &src[end..])
    }

    fn string(e: &Expr) -> String {
        match e {
            Expr::Str(s) | Expr::Atom(s) => s.clone(),
            Expr::List(_) => panic!("expected a string: {e:?}"),
        }
    }

    fn number(e: &Expr) -> usize {
        string(e).parse().unwrap_or(0)
    }

    fn color(e: &Expr) -> Color {
        match string(e).as_str() {
            "black" => Color::Black,
            "red" => Color::Red,
            "green" => Color::Green,
            "yellow" => Color::Yellow,
            "blue" => Color::Blue,
            "magenta" => Color::Magenta,
            "cyan" => Color::Cyan,
            _ => Color::White,
        }
    }

    fn build(e: &Expr) -> Doc {
        let items = match e {
            Expr::Str(s) => return literal(s.as_str()),
            Expr::Atom(a) => {
                return match a.as_str() {
                    "space" => space(),
                    "cr" => cr(),
                    "blankline" => blankline(),
                    "empty" => empty(),
                    _ => panic!("unknown atom {a}"),
                };
            }
            Expr::List(items) => items,
        };
        let name = string(&items[0]);
        let arg = |i: usize| build(&items[i]);
        let rest = |from: usize| items[from..].iter().map(build).collect::<Vec<_>>();
        match name.as_str() {
            "cat" => hcat(rest(1)),
            "sp" => arg(1).spaced(arg(2)),
            "above" => arg(1).above(arg(2)),
            "above_blank" => arg(1).above_blank(arg(2)),
            "hsep" => hsep(rest(1)),
            "vcat" => vcat(rest(1)),
            "vsep" => vsep(rest(1)),
            "blanklines" => blanklines(number(&items[1])),
            "nest" => nest(number(&items[1]), arg(2)),
            "hang" => hang(number(&items[1]), arg(2), arg(3)),
            "prefixed" => prefixed(&string(&items[1]), arg(2)),
            "inside" => inside(arg(1), arg(2), arg(3)),
            "after_break" => after_break(&string(&items[1])),
            "vfill" => vfill(&string(&items[1])),
            "fg" => fg(color(&items[1]), arg(2)),
            "bg" => bg(color(&items[1]), arg(2)),
            "lblock" => lblock(number(&items[1]), arg(2)),
            "cblock" => cblock(number(&items[1]), arg(2)),
            "rblock" => rblock(number(&items[1]), arg(2)),
            "flush" => flush(arg(1)),
            "nowrap" => nowrap(arg(1)),
            "chomp" => chomp(arg(1)),
            "nestle" => nestle(arg(1)),
            "braces" => braces(arg(1)),
            "brackets" => brackets(arg(1)),
            "parens" => parens(arg(1)),
            "quotes" => quotes(arg(1)),
            "double_quotes" => double_quotes(arg(1)),
            "before_non_blank" => before_non_blank(arg(1)),
            "bold" => bold(arg(1)),
            "italic" => italic(arg(1)),
            "underlined" => underlined(arg(1)),
            "strikeout" => strikeout(arg(1)),
            _ => panic!("unknown form {name}"),
        }
    }

    fn width_arg(label: &str) -> Option<usize> {
        label.parse().ok()
    }

    #[test]
    fn docs_match_oracle() {
        let mut failures = Vec::new();
        let mut count = 0;
        let mut current: Option<(String, Doc)> = None;
        for line in VECTORS.lines() {
            let (kind, rest) = line.split_once(' ').unwrap_or((line, ""));
            match kind {
                "doc" => {
                    count += 1;
                    current = Some((rest.to_owned(), build(&parse(rest).0)));
                }
                "meta" => {
                    let Some((src, doc)) = &current else { continue };
                    let got = format!(
                        "{} {} {} {} {} {}",
                        doc.is_empty(),
                        doc.offset(),
                        doc.min_offset(),
                        doc.height(),
                        doc.update_column(0),
                        doc.update_column(5)
                    );
                    if got != rest {
                        failures.push(format!("{src}\n  meta: want {rest}\n         got {got}"));
                    }
                }
                "plain" | "ansi" => {
                    let Some((src, doc)) = &current else { continue };
                    let (label, quoted) = rest.split_once(' ').unwrap_or((rest, ""));
                    let want = unquote(quoted).0;
                    let width = width_arg(label);
                    let got = if kind == "plain" {
                        doc.render(width)
                    } else {
                        doc.render_ansi(width)
                    };
                    if got != want {
                        failures.push(format!(
                            "{src}\n  {kind} {label}: want {want:?}\n  {:width$}  got  {got:?}",
                            "",
                            width = kind.len() + label.len() + 1
                        ));
                    }
                }
                _ => {}
            }
        }
        assert!(count >= 200, "only {count} documents");
        assert!(
            failures.is_empty(),
            "{} failures:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn widths_match_oracle() {
        let mut failures = Vec::new();
        let mut checked = 0u64;
        for line in VECTORS.lines() {
            let fields: Vec<&str> = line.split(' ').collect();
            if fields.first() == Some(&"len") {
                let joined = fields[1..fields.len() - 1].join(" ");
                let text = unquote(&joined).0;
                let want: usize = fields[fields.len() - 1].parse().unwrap_or(usize::MAX);
                if real_length(&text) != want {
                    failures.push(format!(
                        "len {text:?}: want {want} got {}",
                        real_length(&text)
                    ));
                }
                continue;
            }
            if fields.first() != Some(&"width") {
                continue;
            }
            let start = u32::from_str_radix(fields[1], 16).unwrap_or(0);
            let end = u32::from_str_radix(fields[2], 16).unwrap_or(0);
            let want: Vec<isize> = fields[3..]
                .iter()
                .map(|f| f.parse().unwrap_or(-99))
                .collect();
            for cp in start..=end {
                let Some(c) = std::char::from_u32(cp) else {
                    continue;
                };
                checked += 1;
                let got = [
                    width::chars_length([c]),
                    width::chars_length(['a', c]),
                    width::chars_length([c, '\u{FE0F}']),
                    width::chars_length([c, '\u{1F3FB}']),
                    width::chars_length([c, '\u{200D}', 'a']),
                ];
                if got[..] != want[..] {
                    failures.push(format!("U+{cp:04X}: want {want:?} got {got:?}"));
                } else if to_isize(char_width(c)) != want[1] - 1 {
                    failures.push(format!("char_width U+{cp:04X}: got {}", char_width(c)));
                }
            }
        }
        assert!(checked > 1_000_000, "only {checked} code points");
        let shown: Vec<_> = failures.iter().take(40).cloned().collect();
        assert!(
            failures.is_empty(),
            "{} failures:\n{}",
            failures.len(),
            shown.join("\n")
        );
    }
}
