// Ported from `doclayout` 0.6 (BSD-3-Clause), Copyright John MacFarlane.
// See third_party/licences/doclayout.txt.
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
//! The layout engine, the queries, `clone`, `==` and drop use explicit
//! stacks, so the stack cost does not grow with the nesting depth of a
//! document. Only `Debug` output recurses. The engine uses wrapping
//! integer arithmetic, as Haskell `Int` does.
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
pub use width::{
    char_width, is_emoji_variation, is_skin_tone_modifier, is_zwj, real_length,
    real_length_narrow_context, real_length_narrow_context_no_shortcut,
};

use render::Line;
use style::StyleReq;
use width::str_length;

/// A document that can be rendered at a given line width.
///
/// Each operation on a document, including `clone`, `==` and drop, works
/// without recursion that grows with the nesting depth of the document.
/// `Debug` output is the exception.
#[derive(Debug, Default)]
pub struct Doc(Node);

/// The node of a document. Only `Doc` clones and compares nodes with
/// children, so the derived `Clone` and `PartialEq` impls here run on one
/// level.
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

/// A wrapper node without its child.
enum Shell {
    Prefixed(String),
    BeforeNonBlank,
    Flush,
    Styled(StyleReq),
    Linked(String),
}

impl Shell {
    fn of(node: &Node) -> Option<(Shell, &Doc)> {
        Some(match node {
            Node::Prefixed(prefix, inner) => (Shell::Prefixed(prefix.clone()), inner),
            Node::BeforeNonBlank(inner) => (Shell::BeforeNonBlank, inner),
            Node::Flush(inner) => (Shell::Flush, inner),
            Node::Styled(style, inner) => (Shell::Styled(*style), inner),
            Node::Linked(target, inner) => (Shell::Linked(target.clone()), inner),
            _ => return None,
        })
    }

    fn wrap(self, doc: Doc) -> Doc {
        let inner = Box::new(doc);
        Doc(match self {
            Shell::Prefixed(prefix) => Node::Prefixed(prefix, inner),
            Shell::BeforeNonBlank => Node::BeforeNonBlank(inner),
            Shell::Flush => Node::Flush(inner),
            Shell::Styled(style) => Node::Styled(style, inner),
            Shell::Linked(target) => Node::Linked(target, inner),
        })
    }
}

/// Copies `doc` with the same structure. `leaf` makes each node that has no
/// children.
fn rebuild(doc: &Doc, leaf: &dyn Fn(&Node) -> Node) -> Doc {
    enum Step<'a> {
        Visit(&'a Doc),
        Wrap(Shell),
        Join(usize),
    }
    let mut steps = vec![Step::Visit(doc)];
    let mut done: Vec<Doc> = Vec::new();
    while let Some(step) = steps.pop() {
        match step {
            Step::Visit(d) => {
                if let Node::Concat(items) = &d.0 {
                    steps.push(Step::Join(items.len()));
                    steps.extend(items.iter().rev().map(Step::Visit));
                } else if let Some((shell, inner)) = Shell::of(&d.0) {
                    steps.push(Step::Wrap(shell));
                    steps.push(Step::Visit(inner));
                } else {
                    done.push(Doc(leaf(&d.0)));
                }
            }
            Step::Wrap(shell) => {
                let inner = done.pop().unwrap_or_default();
                done.push(shell.wrap(inner));
            }
            Step::Join(count) => {
                let items = done.split_off(done.len().saturating_sub(count));
                done.push(Doc(Node::Concat(items)));
            }
        }
    }
    done.pop().unwrap_or_default()
}

fn children(node: &Node) -> &[Doc] {
    match node {
        Node::Concat(items) => items,
        _ => Shell::of(node).map_or(&[], |(_, inner)| std::slice::from_ref(inner)),
    }
}

impl Clone for Doc {
    fn clone(&self) -> Doc {
        rebuild(self, &Node::clone)
    }
}

impl PartialEq for Doc {
    fn eq(&self, other: &Doc) -> bool {
        let mut pairs = vec![(self, other)];
        while let Some((a, b)) = pairs.pop() {
            let (ca, cb) = (children(&a.0), children(&b.0));
            let same_shell = match (&a.0, &b.0) {
                (Node::Concat(_), Node::Concat(_)) => true,
                (Node::Prefixed(x, _), Node::Prefixed(y, _))
                | (Node::Linked(x, _), Node::Linked(y, _)) => x == y,
                (Node::Styled(x, _), Node::Styled(y, _)) => x == y,
                (Node::BeforeNonBlank(_), Node::BeforeNonBlank(_))
                | (Node::Flush(_), Node::Flush(_)) => true,
                (x, y) if ca.is_empty() && cb.is_empty() => x == y,
                _ => false,
            };
            if !same_shell || ca.len() != cb.len() {
                return false;
            }
            pairs.extend(ca.iter().zip(cb));
        }
        true
    }
}

impl Drop for Doc {
    fn drop(&mut self) {
        if children(&self.0).is_empty() {
            return;
        }
        let mut nodes = vec![std::mem::take(&mut self.0)];
        while let Some(mut node) = nodes.pop() {
            match &mut node {
                Node::Concat(items) => nodes.extend(items.iter_mut().map(Doc::take_node)),
                Node::Prefixed(_, inner)
                | Node::BeforeNonBlank(inner)
                | Node::Flush(inner)
                | Node::Styled(_, inner)
                | Node::Linked(_, inner) => nodes.push(inner.take_node()),
                _ => {}
            }
        }
    }
}

fn to_isize(n: usize) -> isize {
    isize::try_from(n).unwrap_or(isize::MAX)
}

fn to_usize(n: isize) -> usize {
    usize::try_from(n).unwrap_or(0)
}

impl Doc {
    fn take_node(&mut self) -> Node {
        std::mem::take(&mut self.0)
    }

    fn into_node(mut self) -> Node {
        self.take_node()
    }

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
        let (longest, current) = get_offset(&|_| false, (0, 0), self);
        to_usize(longest.max(current))
    }

    /// Returns the minimal width of the document when it is wrapped at each
    /// breakable space.
    pub fn min_offset(&self) -> usize {
        let (longest, current) = get_offset(&|c| c > 0, (0, 0), self);
        to_usize(longest.max(current))
    }

    /// Returns the column after the last character of the document when
    /// the document starts at column `column` and is not wrapped.
    pub fn update_column(&self, column: usize) -> usize {
        to_usize(get_offset(&|_| false, (0, to_isize(column)), self).1)
    }

    /// Returns the number of lines of the document rendered without
    /// wrapping.
    pub fn height(&self) -> usize {
        self.render(None).matches('\n').count() + 1
    }

    /// Applies `f` to the text of each text, fill and block line, as the
    /// `Functor` instance of `doclayout` does. Each stored width stays the
    /// same, so the layout uses the widths of the old text. Prefixes,
    /// after-break text and link targets do not change. `f` must map the
    /// empty string to the empty string.
    ///
    /// ```
    /// use randoc_layout::{literal, space};
    ///
    /// let doc = literal("ab") + space() + literal("cd");
    /// assert_eq!(doc.map_text(&|s| s.to_uppercase()).render(None), "AB CD");
    /// ```
    #[must_use]
    pub fn map_text(&self, f: &dyn Fn(&str) -> String) -> Doc {
        rebuild(self, &|node| match node {
            Node::Text(width, text) => Node::Text(*width, f(text)),
            Node::VFill(width, text) => Node::VFill(*width, f(text)),
            Node::Block(width, lines) => Node::Block(
                *width,
                lines
                    .iter()
                    .map(|line| {
                        line.iter()
                            .map(|attr| render::Attr {
                                text: f(&attr.text),
                                ..attr.clone()
                            })
                            .collect()
                    })
                    .collect(),
            ),
            other => other.clone(),
        })
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
        match (self.into_node(), other.into_node()) {
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

/// Computes `(longest line, current column)`. Each stack entry has the
/// width of its prefix, to which the column returns after a line break, and
/// the number of its concatenation chain.
fn get_offset(
    break_when: &dyn Fn(isize) -> bool,
    start: (isize, isize),
    doc: &Doc,
) -> (isize, isize) {
    let (mut longest, mut column) = start;
    let mut chains = 0;
    let mut stack = vec![(doc, 0, chains)];
    while let Some((d, prefix, chain)) = stack.pop() {
        match &d.0 {
            Node::Text(n, _) | Node::Block(n, _) | Node::VFill(n, _) => {
                column = column.wrapping_add(*n);
            }
            Node::Empty => {}
            Node::Styled(_, inner) | Node::Linked(_, inner) => {
                chains += 1;
                stack.push((inner, prefix, chains));
            }
            Node::CarriageReturn | Node::NewLine | Node::BlankLines(_) => {
                longest = longest.max(column);
                column = prefix;
            }
            Node::Prefixed(text, inner) => {
                let inner_prefix = prefix.wrapping_add(str_length(text));
                if column <= prefix {
                    column = inner_prefix;
                }
                chains += 1;
                stack.push((inner, inner_prefix, chains));
            }
            Node::BeforeNonBlank(inner) => {
                let followed = stack
                    .last()
                    .is_some_and(|&(next, _, c)| c == chain && is_non_blank(next));
                if followed {
                    stack.push((inner, prefix, chain));
                }
            }
            Node::Flush(inner) => {
                chains += 1;
                stack.push((inner, 0, chains));
            }
            Node::BreakingSpace => {
                if break_when(column) {
                    longest = longest.max(column);
                    column = prefix;
                } else {
                    column = column.wrapping_add(1);
                }
            }
            Node::AfterBreak(text) => {
                if column == prefix {
                    column = column.wrapping_add(str_length(text));
                }
            }
            Node::Concat(items) => stack.extend(items.iter().rev().map(|i| (i, prefix, chain))),
        }
    }
    (longest, column)
}

fn is_non_blank(doc: &Doc) -> bool {
    let mut doc = doc;
    loop {
        match &doc.0 {
            Node::Text(..) => return true,
            Node::BeforeNonBlank(inner) | Node::Flush(inner) => doc = inner,
            Node::Concat(items) => match items.first() {
                Some(first) => doc = first,
                None => return false,
            },
            _ => return false,
        }
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
    if doc.is_empty() {
        return doc;
    }
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
    rebuild(&doc, &|node| match node {
        Node::BreakingSpace => Node::Text(1, " ".to_owned()),
        other => other.clone(),
    })
}

/// Text that is rendered only at the start of a line, for example to
/// escape a `.` at the start of a line in roff.
pub fn after_break(text: &str) -> Doc {
    Doc(Node::AfterBreak(text.to_owned()))
}

/// Removes trailing spaces, line breaks and blank lines from `doc`.
pub fn chomp(doc: Doc) -> Doc {
    enum Context {
        Prefixed(String),
        Before(Vec<Doc>),
    }
    let is_break = |node: &Node| {
        matches!(
            node,
            Node::BlankLines(_) | Node::NewLine | Node::CarriageReturn | Node::BreakingSpace
        )
    };
    let mut contexts = Vec::new();
    let mut current = doc;
    let mut result = 'outer: loop {
        match current.into_node() {
            node if is_break(&node) => break empty(),
            Node::Prefixed(prefix, mut inner) => {
                contexts.push(Context::Prefixed(prefix));
                current = Doc(inner.take_node());
            }
            Node::Concat(mut items) => {
                while let Some(last) = items.pop() {
                    if is_break(&last.0) {
                        continue;
                    }
                    if matches!(last.0, Node::Prefixed(..)) {
                        contexts.push(Context::Before(items));
                        current = last;
                        continue 'outer;
                    }
                    items.push(last);
                    break 'outer hcat(items);
                }
                break empty();
            }
            other => break Doc(other),
        }
    };
    while let Some(context) = contexts.pop() {
        result = match context {
            Context::Prefixed(prefix) => Doc(Node::Prefixed(prefix, Box::new(result))),
            Context::Before(items) => hcat(items) + result,
        };
    }
    result
}

/// Removes leading line breaks and blank lines from `doc`.
pub fn nestle(doc: Doc) -> Doc {
    match doc.into_node() {
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

/// Wraps `doc` at `width`, or at width 1 when `width` is 0. The alignment
/// uses the requested width, as in `doclayout`.
fn block(width: usize, doc: &Doc, align: fn(isize, Line) -> Line) -> Doc {
    let requested = to_isize(width);
    let wrap = if width < 1 && !doc.is_empty() {
        1
    } else {
        requested
    };
    let output = render::prerender(doc, Some(wrap));
    let lines = render::chop(wrap, output)
        .into_iter()
        .map(|line| align(requested, line))
        .collect();
    Doc(Node::Block(wrap, lines))
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
        pad_left(w.wrapping_sub(render::line_length(&line)), line)
    })
}

/// A block like [`lblock`], with the content centered.
pub fn cblock(width: usize, doc: Doc) -> Doc {
    block(width, &doc, |w, line| {
        pad_left(
            w.wrapping_sub(render::line_length(&line)).div_euclid(2),
            line,
        )
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

/// Removes a final newline or carriage return from `doc`, as `removeFinalNl`
/// of `doctemplates` does. Only the last item of a concatenation is checked.
///
/// ```
/// use randoc_layout::{literal, remove_final_newline};
///
/// assert_eq!(remove_final_newline(literal("ab\n")).render(None), "ab");
/// ```
pub fn remove_final_newline(doc: Doc) -> Doc {
    match doc.into_node() {
        Node::NewLine | Node::CarriageReturn => empty(),
        Node::Concat(mut items) => {
            if items
                .last()
                .is_some_and(|d| matches!(d.0, Node::NewLine | Node::CarriageReturn))
            {
                items.pop();
            }
            hcat(items)
        }
        other => Doc(other),
    }
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

    /// The render cases of `test/test.hs` in `doclayout` 0.6, with the
    /// expected values of that file. The case "lblock with blank line"
    /// uses the `Block` constructor directly and has no equivalent here.
    const PORTED_RENDERS: &[(&str, Option<usize>, &str)] = &[
        (r#"(lblock 4 "hi there")"#, None, "hi t\nhere"),
        (r#"(lblock 3 "abcdefg")"#, None, "abc\ndef\ng"),
        (r#"(lblock 2 "ab\u{302}cd")"#, None, "ab\u{302}\ncd"),
        (r#"(nest 3 "abcdefghi")"#, Some(10), "   abcdefghi"),
        (
            r#"(vcat (chomp "aaa") (chomp "bbb") (chomp "ccc"))"#,
            Some(10),
            "aaa\nbbb\nccc",
        ),
        (
            r#"(above (chomp "aaa") (above (chomp "bbb") (chomp "ccc")))"#,
            Some(10),
            "aaa\nbbb\nccc",
        ),
        (r#"(sp "hello" "there")"#, Some(4), "hello\nthere"),
        (r#"(cat "hello" cr "there")"#, Some(60), "hello\nthere"),
        (r#"(above "x" cr)"#, None, "x\n"),
        (
            r#"(hsep "hello" "there" "this" "is" "a" "test")"#,
            Some(10),
            "hello\nthere this\nis a test",
        ),
        (
            r#"(cat (lblock 3 "aa") (lblock 3 "bb") (lblock 3 (sp "aa" "bbbb")))"#,
            Some(50),
            "aa bb aa\n      bbb\n      b",
        ),
        (
            r#"(prefixed "> " (cat (sp "foo" "bar") cr "baz" blankline (sp "bim" "bam")))"#,
            Some(80),
            "> foo bar\n> baz\n>\n> bim bam",
        ),
        (r#"(prefixed "> " (hsep "a" "b" "c"))"#, None, "> a b c"),
        (
            r#"(nest 4 "aa\u{A}\u{A}bb\u{A}cc")"#,
            None,
            "    aa\n\n    bb\n    cc",
        ),
        (
            r#"(cat (hang 4 "  - " (cat (chomp "aa\u{A}\u{A}bb\u{A}cc") cr)) (hang 4 "  - " (cat (chomp "dd\u{A}") cr)))"#,
            None,
            "  - aa\n\n    bb\n    cc\n  - dd\n",
        ),
        (
            r#"(cat "aa" (lblock 2 (above "bb" "cc")) "dd")"#,
            None,
            "aabb\n  ccdd",
        ),
        (
            r#"(cat "aa" (cblock 4 (above "bb" "cc")))"#,
            None,
            "aa bb\n   cc",
        ),
        (r#"(cat (blanklines 2) "aa")"#, None, "\naa"),
        (r#"(cat "aa" (blanklines 2))"#, None, "aa\n"),
        (
            r#"(cat "aa" cr (blanklines 2) (blanklines 0))"#,
            None,
            "aa\n",
        ),
        (
            r#"(cat (nest 2 (cat (nest 3 (cat "aa" blankline)) cr (blanklines 2))) (blanklines 2))"#,
            None,
            "     aa\n",
        ),
        (
            r#"(cat "aa" blankline cr blankline "bb")"#,
            None,
            "aa\n\nbb",
        ),
        (
            r#"(vcat (hang 2 "- " (chomp (cat "aaaaa" space "bbb"))) (hang 2 "- " "aoeu"))"#,
            Some(8),
            "- aaaaa\n  bbb\n- aoeu",
        ),
        (
            r#"(cat "aaaaa" space "bbbbb" blankline "ccccc")"#,
            Some(8),
            "aaaaa\nbbbbb\n\nccccc",
        ),
        (
            r#"(cat (chomp (cat (cat "aa" space) blankline)) "bb")"#,
            None,
            "aabb",
        ),
        (r#"(cat (chomp (cat "aa" space)) "bb")"#, None, "aabb"),
        (r#"(chomp "aa")"#, None, "aa"),
        (r#"(chomp (nest 3 (cat "aa" blankline)))"#, None, "   aa"),
        (
            r#"(cat "aa" cr (chomp (cat (lblock 2 (cat "aa" blankline)) blankline)))"#,
            None,
            "aa\naa",
        ),
        (r#"(above "aa" (above empty "bb"))"#, None, "aa\nbb"),
        (
            r#"(vcat (cat (rblock 4 "aa") (lblock 3 " | ") (cblock 4 "bb") (lblock 3 " | ") (lblock 4 "cc")) (cat (rblock 4 "----") (lblock 3 " | ") (cblock 4 "----") (lblock 3 " | ") (lblock 4 "----")) (cat (rblock 4 "dd") (lblock 3 " | ") (cblock 4 "ee") (lblock 3 " | ") (lblock 4 "ff")))"#,
            None,
            "  aa |  bb  | cc\n---- | ---- | ----\n  dd |  ee  | ff",
        ),
        (
            r#"(cat "aa" space "bbbbb" "ccccc")"#,
            Some(10),
            "aa\nbbbbbccccc",
        ),
        (
            r#"(cat (nest 5 (hsep "hi" "there" "my" "friend")) cr)"#,
            Some(10),
            "     hi\n     there\n     my\n     friend\n",
        ),
        (r#"(cblock 7 (sp "hi" "there"))"#, None, "  hi\n there"),
        (
            r#"(cat (sp "hi" (after_break "!")) (after_break "?") "x" (after_break "?"))"#,
            Some(2),
            "hi\n!x",
        ),
        (
            r#"(cat "[" (nest 1 (above "aaaaaaaaa" "bbbbbbb")) "]")"#,
            Some(5),
            "[aaaaaaaaa\n bbbbbbb]",
        ),
        (
            r#"(above "aa" (above (nest 3 empty) "bb"))"#,
            None,
            "aa\nbb",
        ),
        (r#"(hsep "a" empty "b")"#, None, "a b"),
        (r#"(sp "a" (sp empty "b"))"#, None, "a b"),
        (r#"(cat (vcat "aa" "bb") "cc")"#, None, "aa\nbbcc"),
        ("(vcat)", None, ""),
        (
            r#"(nestle (above (blanklines 2) (above "aa" (cat (blanklines 2) cr))))"#,
            None,
            "aa\n",
        ),
        (
            r#"(prefixed "> " (cblock 4 (above "aa" "bb")))"#,
            None,
            ">  aa\n>  bb",
        ),
        (r#"(cat empty space "x")"#, None, "x"),
        (r#"(hang 5 (nowrap "xxx") empty)"#, None, "xxx"),
        (
            r#"(above (cat (before_non_blank "!!") " ab") (cat (before_non_blank "!!") "a b"))"#,
            None,
            " ab\n!!a b",
        ),
        (
            r#"(cat (vfill "|") (cblock 5 (vcat "a" "bbb" "ccccc")) (lblock 3 "dd") (vfill "+"))"#,
            None,
            "|  a  dd +\n| bbb    +\n|ccccc   +",
        ),
        (
            r#"(cat (vfill "| ") (cblock 5 (vcat "a" "bbb")) (vfill " | ") (lblock 2 (vcat "dd" "ee" "ff")) (vfill " |"))"#,
            None,
            "|   a   | dd |\n|  bbb  | ee |\n|       | ff |",
        ),
        (r#"(cat "\u{366}" space "a")"#, None, "\u{366} a"),
        (r#"(cat "a" cr "\u{A}b" cr "c")"#, None, "a\n\nb\nc"),
        (r#"(nowrap (bold (cat "aa" space "bb")))"#, Some(4), "aa bb"),
        (
            r#"(nowrap (prefixed "> " (cat "aa" space "bb")))"#,
            Some(4),
            "> aa bb",
        ),
        (
            r#"(sp "hi" (fg blue (sp "mom" (sp "and" "dad"))))"#,
            Some(5),
            "hi\nmom\nand\ndad",
        ),
    ];

    #[test]
    fn ported_doclayout_renders() {
        for (src, width, want) in PORTED_RENDERS {
            let doc = build(&parse(src).0);
            assert_eq!(doc.render(*width), *want, "{src} at {width:?}");
        }
    }

    #[test]
    fn ported_doclayout_offsets_and_lengths() {
        let offset = |src: &str| build(&parse(src).0).offset();
        assert_eq!(
            offset(r#"(cat (nest 3 (cat "**" "thisIsGoingToBeTooLongAnyway" "**")) blankline)"#),
            35
        );
        assert_eq!(offset(r#"(cat "hello" (nest 2 "world"))"#), 10);
        assert_eq!(offset(r#"(cat "aa" (nest 2 (cat "bb" cr "cc")))"#), 4);
        let bold_chopped = lblock(4, bold(literal("hi there"))).render_ansi(None);
        assert!(bold_chopped.contains("\u{1b}[1m"));
        let lengths: &[(&str, usize)] = &[
            ("This is going to be too long anyway", 35),
            ("Circle\u{25EF}Circle", 13),
            ("*a", 2),
            ("*\u{FE0F}\u{20E3}a", 3),
            ("\u{231A}", 2),
            ("\u{A9}\u{FE0F}", 2),
            ("\u{A9}", 1),
            ("\u{1F170}\u{FE0F}\u{1F1E6}\u{1F1E8}", 4),
            ("\u{1F590}\u{1F3FF}", 2),
            ("1\u{1F3FF}", 3),
        ];
        for (text, want) in lengths {
            assert_eq!(real_length(text), *want, "{text:?}");
        }
        for &(cp, _, _) in tables::EMOJI {
            let Some(c) = std::char::from_u32(cp) else {
                continue;
            };
            let base = real_length(&c.to_string());
            assert!(base == 1 || base == 2, "U+{cp:04X}");
        }
    }

    #[test]
    fn links_render_as_osc8() {
        let doc = literal("see") + space() + link("https://a.b", bold(literal("here")));
        assert_eq!(
            doc.render_ansi(None),
            "see \u{1b}[1m\u{1b}[23m\u{1b}[39m\u{1b}[49m\u{1b}[24m\u{1b}[29m\
             \u{1b}]8;;https://a.b\u{1b}\\here\u{1b}[0m\u{1b}]8;;\u{1b}\\"
        );
        assert_eq!(doc.render(None), "see here");
        let nested = link("x", literal("a") + link("y", literal("b")) + literal("c"));
        assert_eq!(
            nested.render_ansi(None),
            "\u{1b}]8;;x\u{1b}\\abc\u{1b}[0m\u{1b}]8;;\u{1b}\\"
        );
        let after = link("x", literal("a") + link("y", literal("b"))) + literal("c");
        assert_eq!(
            after.render_ansi(None),
            "\u{1b}]8;;x\u{1b}\\ab\u{1b}]8;;\u{1b}\\c\u{1b}[0m\u{1b}]8;;\u{1b}\\"
        );
        let filled = link("x", vfill("|")) + lblock(1, literal("a"));
        assert_eq!(
            filled.render_ansi(None),
            "\u{1b}]8;;x\u{1b}\\|\u{1b}]8;;\u{1b}\\a\u{1b}[0m\u{1b}]8;;\u{1b}\\"
        );
        assert!(!link("x", empty()).is_empty());
        assert_eq!(
            link("x", empty()).render_ansi(None),
            "\u{1b}[0m\u{1b}]8;;\u{1b}\\"
        );
    }

    #[test]
    fn small_constructors() {
        assert_eq!(char('x'), literal("x"));
        assert_eq!(text("a b"), literal("a b"));
        assert_eq!(Doc::from("a"), literal("a"));
        assert_eq!(Doc::from(String::from("a")), literal("a"));
        let summed: Doc = ["a", "b"].into_iter().map(literal).sum();
        assert_eq!(summed.render(None), "ab");
        let mut doc = literal("a");
        doc += literal("b") + (literal("c") + literal("d"));
        assert_eq!(doc.render(None), "abcd");
        assert_eq!((space() + literal("a")).render(None), "a");
        assert!(before_non_blank(empty()).render(None).is_empty());
        assert!(!before_non_blank(empty()).is_empty());
        assert!(!after_break("").is_empty());
        assert_eq!(empty().height(), 1);
        assert_eq!(literal("a\nb\n").height(), 3);
        assert_eq!(real_length("\u{FE0F}\u{200D}"), 0);
        assert_eq!(char_width('\u{1F3FB}'), 2);
        assert_eq!(char_width('\u{200D}'), 0);
        assert_eq!(char_width('\u{FE0F}'), 0);
    }

    /// Expected values from `pandoc.template` with the `uppercase` and
    /// `lowercase` pipes in Pandoc 3.13.
    #[test]
    fn map_text_keeps_stored_widths() {
        let upper = |d: &Doc| d.map_text(&|s| s.to_uppercase());
        let lower = |d: &Doc| d.map_text(&|s| s.to_lowercase());
        let sharp = literal("ßßßß")
            .spaced(literal("ßßßß"))
            .spaced(literal("ab"));
        assert_eq!(upper(&sharp).render(Some(9)), "SSSSSSSS SSSSSSSS\nAB");
        assert_eq!(upper(&sharp).render(None), "SSSSSSSS SSSSSSSS AB");
        let blocks = literal("[")
            + literal("ß")
            + lblock(3, literal("aß") + cr() + literal("Bc"))
            + vfill("ß|")
            + literal("]");
        assert_eq!(upper(&blocks).render(None), "[SSASSSS|\n  BC SS|]");
        let kept = prefixed("P> ", literal("AB").spaced(literal("CD")))
            + cr()
            + after_break("X")
            + literal("EF");
        assert_eq!(lower(&kept).render(Some(4)), "P> ab\nP> cd\nXef");
        let nested = bold(literal("ÀÉ"))
            + flush(literal("ÖÜ"))
            + before_non_blank(literal("QQ"))
            + literal("Z");
        assert_eq!(lower(&nested).render(None), "àéöüqqz");
        let block_prefix = lblock(6, prefixed("p> ", literal("ab") + cr() + literal("cd")));
        assert_eq!(upper(&block_prefix).render(None), "P> AB\nP> CD");
        let linked = link("http://x.y/Z", literal("a"));
        assert_eq!(
            upper(&linked).render_ansi(None),
            "\u{1b}]8;;http://x.y/Z\u{1b}\\A\u{1b}[0m\u{1b}]8;;\u{1b}\\"
        );
        assert_eq!(
            upper(&(literal("a") + space() + blankline())).render(None),
            "A\n"
        );
    }

    /// Expected values from `pandoc.template` (`$x$|`) in Pandoc 3.13.
    #[test]
    fn remove_final_newline_drops_one_break() {
        let apply = |d: Doc| (remove_final_newline(d) + literal("|")).render(None);
        assert_eq!(apply(literal("ab") + cr()), "ab|");
        assert_eq!(apply(literal("ab\n")), "ab|");
        assert_eq!(apply(literal("ab") + blankline()), "ab\n\n|");
        assert_eq!(apply(cr()), "|");
        assert_eq!(apply(literal("\n")), "|");
        assert_eq!(apply(literal("a") + cr() + cr()), "a\n|");
        assert_eq!(remove_final_newline(literal("a") + cr()), literal("a"));
        assert_eq!(remove_final_newline(literal("ab")), literal("ab"));
        assert_eq!(remove_final_newline(nest(2, cr())), nest(2, cr()));
    }

    #[test]
    fn nesting_empty_does_not_allocate_the_indent() {
        assert!(nest(isize::MAX.unsigned_abs(), empty()).is_empty());
        assert!(hang(usize::MAX, empty(), empty()).is_empty());
    }

    /// Expected values from `pandoc.layout` in Pandoc 3.13.
    #[test]
    fn zero_width_blocks_align_at_the_requested_width() {
        for block in [lblock, cblock, rblock] {
            let doc = block(0, literal("\na"));
            assert_eq!(doc.render(None), "a");
            let queries = (
                doc.offset(),
                doc.min_offset(),
                doc.height(),
                doc.update_column(0),
            );
            assert_eq!(queries, (1, 1, 1, 1));
            assert_eq!(block(0, literal("a\nbb")).render(None), "a\nb\nb");
        }
    }

    /// Review findings LAY-001 and F4. Expected values from `pandoc.layout`
    /// in Pandoc 3.13 for the same 10000 levels.
    #[test]
    fn deep_nesting_uses_no_call_stack() {
        let mut doc = literal("x") + cr() + literal("y") + space();
        for i in 1..=10_000 {
            doc = match i % 4 {
                0 => bold(doc),
                1 => prefixed("", doc),
                2 => flush(doc),
                _ => before_non_blank(doc) + literal("z"),
            };
        }
        let rendered = doc.render(None);
        assert_eq!(rendered.len(), 2503);
        assert!(rendered.starts_with("x\nyzzz"));
        assert_eq!(
            (doc.offset(), doc.min_offset(), doc.update_column(0)),
            (2502, 2500, 2502)
        );
        assert!(chomp(doc.clone()).render(None).ends_with("zzzzz"));
        assert!(
            nowrap(doc.clone())
                .render(Some(3))
                .starts_with("x\ny zzzzzzzz")
        );
        assert_eq!(doc.map_text(&|s| s.to_uppercase()).render(None).len(), 2503);
        assert_eq!(doc.clone(), doc);
        assert_ne!(doc.clone() + literal("q"), doc);
        assert!(doc.render_ansi(Some(5)).contains("\u{1b}[1m"));
        drop(doc);
        let mut prefixes = literal("x");
        for _ in 0..10_000 {
            prefixes = prefixed("", prefixes);
        }
        assert_eq!(prefixes.render(None), "x");
        let mut chain = literal("a");
        for _ in 0..10_000 {
            chain = prefixed("", chain + blankline());
        }
        assert_eq!(chomp(chain).render(None), "a");
    }

    /// Review finding F1. Expected values from `pandoc.layout`.
    #[test]
    fn rblock_zero_width_drops_empty_line() {
        assert_eq!(rblock(0, literal("x\n\ny")).render(None), "x\ny");
        assert_eq!(rblock(0, literal("x\n\ny")).height(), 2);
    }

    /// Review finding F3. Expected values from `pandoc.layout`, where
    /// `Int` arithmetic wraps.
    #[test]
    fn large_arguments_do_not_panic() {
        assert!(hang(usize::MAX, empty(), empty()).is_empty());
        let max = isize::MAX.unsigned_abs();
        assert!(nest(max, nest(max, empty())).is_empty());
        let d = literal("a") + blanklines(usize::MAX) + literal("b");
        assert_eq!(d.render(Some(72)), "ab");
        let wide = lblock(max, literal("")) + lblock(max, literal("")) + literal("c");
        assert_eq!(wide.render(None), "c");
        assert_eq!(wide.offset(), 0);
    }

    /// Review finding F5. The shortcut and table values come from the
    /// width sweep of the oracle and from the `doclayout` table.
    #[test]
    fn character_predicates_and_length_forms() {
        assert!(is_skin_tone_modifier('\u{1F3FB}') && is_skin_tone_modifier('\u{1F3FF}'));
        assert!(!is_skin_tone_modifier('\u{1F3FA}') && !is_skin_tone_modifier('\u{1F400}'));
        assert!(is_emoji_variation('\u{FE0E}') && is_emoji_variation('\u{FE0F}'));
        assert!(!is_emoji_variation('\u{FE0D}') && !is_emoji_variation('\u{FE10}'));
        assert!(is_zwj('\u{200D}') && !is_zwj('\u{200C}'));
        assert_eq!(real_length_narrow_context("a\u{9C5}"), 2);
        assert_eq!(real_length_narrow_context_no_shortcut("a\u{9C5}"), 1);
        assert_eq!(
            real_length_narrow_context_no_shortcut("日本\u{1F44B}\u{1F3FD}"),
            6
        );
    }

    #[test]
    fn offset_looks_into_flushed_concatenations() {
        let joined = before_non_blank(literal("xx")) + flush(literal("a") + literal("b"));
        assert_eq!((joined.offset(), joined.update_column(0)), (4, 4));
        let broken = before_non_blank(literal("xx")) + flush(cr() + literal("b"));
        assert_eq!(broken.offset(), 1);
    }
}
