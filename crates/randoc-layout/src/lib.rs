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
//! A concatenation is a list of items, and an item can be a concatenation,
//! so `+` costs amortized constant work. The layout engine, the queries, `clone`, `==`,
//! `Debug` and drop use explicit stacks, so the stack cost does not grow
//! with the nesting depth of a document. The engine uses wrapping
//! integer arithmetic, as Haskell `Int` does. Each level of a non-empty
//! indent or prefix adds to the prefix of each line below it, so deep
//! nesting of such levels takes quadratic time, as in `doclayout`.
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
/// Each operation on a document, including `clone`, `==`, `Debug` and
/// drop, works without recursion that grows with the nesting depth of the
/// document. `+` costs amortized constant work. `==` and `Debug` see the sequence of concatenated
/// items, so `(a + b) + c == a + (b + c)`.
#[derive(Default)]
pub struct Doc(Node);

/// The node of a document. Only `Doc` clones, compares and formats nodes
/// with children, so the derived impls here run on one level.
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
    /// Content that is rendered only before non-blank content. The flag is
    /// `is_non_blank` of the content.
    BeforeNonBlank(bool, Box<Doc>),
    /// Content laid out without the current prefix. The flag is
    /// `is_non_blank` of the content.
    Flush(bool, Box<Doc>),
    BreakingSpace,
    /// Text that is rendered only at the start of a line.
    AfterBreak(String),
    CarriageReturn,
    NewLine,
    BlankLines(usize),
    /// Two or more documents, none of them empty. An item can be a
    /// concatenation, so `+` never copies the items of its right operand.
    Concat(Vec<Doc>, Summary),
    Styled(StyleReq, Box<Doc>),
    Linked(String, Box<Doc>),
}

/// What the queries need to know about a concatenation without a walk of
/// its items. An append updates it in constant time.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Summary {
    /// The number of items: the nodes below the concatenation that are not
    /// concatenations.
    items: usize,
    /// `is_non_blank` of the first item.
    non_blank: bool,
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
            Node::BeforeNonBlank(_, inner) => (Shell::BeforeNonBlank, inner),
            Node::Flush(_, inner) => (Shell::Flush, inner),
            Node::Styled(style, inner) => (Shell::Styled(*style), inner),
            Node::Linked(target, inner) => (Shell::Linked(target.clone()), inner),
            _ => return None,
        })
    }

    fn wrap(self, doc: Doc) -> Doc {
        let non_blank = is_non_blank(&doc);
        let inner = Box::new(doc);
        Doc(match self {
            Shell::Prefixed(prefix) => Node::Prefixed(prefix, inner),
            Shell::BeforeNonBlank => Node::BeforeNonBlank(non_blank, inner),
            Shell::Flush => Node::Flush(non_blank, inner),
            Shell::Styled(style) => Node::Styled(style, inner),
            Shell::Linked(target) => Node::Linked(target, inner),
        })
    }
}

/// Joins two non-empty documents. A concatenation on the left gets
/// `second` as a new item, so the cost is amortized constant.
fn join(first: Doc, second: Doc) -> Doc {
    let added = item_count(&second);
    match first.into_node() {
        Node::Concat(mut items, mut summary) => {
            items.push(second);
            summary.items += added;
            Doc(Node::Concat(items, summary))
        }
        node => {
            let first = Doc(node);
            let summary = Summary {
                items: item_count(&first) + added,
                non_blank: is_non_blank(&first),
            };
            Doc(Node::Concat(vec![first, second], summary))
        }
    }
}

/// Makes a concatenation of two or more non-empty documents.
fn concat(items: Vec<Doc>) -> Doc {
    let summary = Summary {
        items: items.iter().map(item_count).sum(),
        non_blank: items.first().is_some_and(is_non_blank),
    };
    Doc(Node::Concat(items, summary))
}

fn item_count(doc: &Doc) -> usize {
    match &doc.0 {
        Node::Concat(_, summary) => summary.items,
        _ => 1,
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
        steps::count(1);
        match step {
            Step::Visit(d) => {
                if let Node::Concat(items, _) = &d.0 {
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
                done.push(concat(items));
            }
        }
    }
    done.pop().unwrap_or_default()
}

fn children(node: &Node) -> &[Doc] {
    match node {
        Node::Concat(items, _) => items,
        _ => Shell::of(node).map_or(&[], |(_, inner)| std::slice::from_ref(inner)),
    }
}

/// Returns the items of `doc` in order: `doc` itself, or the nodes below a
/// concatenation that are not concatenations.
fn items(doc: &Doc) -> Vec<&Doc> {
    let mut out = Vec::with_capacity(item_count(doc));
    let mut pending = vec![doc];
    while let Some(d) = pending.pop() {
        steps::count(1);
        match &d.0 {
            Node::Concat(items, _) => pending.extend(items.iter().rev()),
            _ => out.push(d),
        }
    }
    out
}

impl Clone for Doc {
    fn clone(&self) -> Doc {
        rebuild(self, &Node::clone)
    }
}

/// Compares the sequences of items, so `(a + b) + c == a + (b + c)`. Two
/// concatenations with different item counts differ at once.
impl PartialEq for Doc {
    fn eq(&self, other: &Doc) -> bool {
        let mut pairs = vec![(self, other)];
        while let Some((a, b)) = pairs.pop() {
            steps::count(1);
            let same_shell = match (&a.0, &b.0) {
                (Node::Concat(_, x), Node::Concat(_, y)) => {
                    if x.items != y.items {
                        return false;
                    }
                    pairs.extend(items(a).into_iter().zip(items(b)));
                    continue;
                }
                (Node::Prefixed(x, _), Node::Prefixed(y, _))
                | (Node::Linked(x, _), Node::Linked(y, _)) => x == y,
                (Node::Styled(x, _), Node::Styled(y, _)) => x == y,
                (Node::BeforeNonBlank(..), Node::BeforeNonBlank(..))
                | (Node::Flush(..), Node::Flush(..)) => true,
                (x, y) => children(x).is_empty() && children(y).is_empty() && x == y,
            };
            if !same_shell {
                return false;
            }
            pairs.extend(children(&a.0).iter().zip(children(&b.0)));
        }
        true
    }
}

/// Gives the text that `#[derive(Debug)]` gives for the flat list of items
/// of a concatenation, `Doc(Concat([Doc(..), ..]))`, in both the plain and
/// the alternate form, without recursion.
impl std::fmt::Debug for Doc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        enum Out<'a> {
            Doc(&'a Doc),
            Value(&'a dyn std::fmt::Debug),
            Open(&'static str),
            Close(&'static str),
            Start,
            End { last: bool },
        }
        let alternate = f.alternate();
        let mut depth = 0usize;
        let mut work = vec![Out::Doc(self)];
        while let Some(out) = work.pop() {
            steps::count(1);
            match out {
                Out::Doc(doc) => {
                    let mut plan = vec![Out::Open("Doc("), Out::Start];
                    match &doc.0 {
                        Node::Concat(..) => {
                            plan.extend([Out::Open("Concat("), Out::Start, Out::Open("[")]);
                            let all = items(doc);
                            let count = all.len();
                            for (index, item) in all.into_iter().enumerate() {
                                let last = index + 1 == count;
                                plan.extend([Out::Start, Out::Doc(item), Out::End { last }]);
                            }
                            plan.extend([Out::Close("]"), Out::End { last: true }]);
                            plan.push(Out::Close(")"));
                        }
                        Node::Prefixed(text, d) | Node::Linked(text, d) => {
                            let name = if matches!(doc.0, Node::Prefixed(..)) {
                                "Prefixed("
                            } else {
                                "Linked("
                            };
                            plan.extend([Out::Open(name), Out::Start, Out::Value(text)]);
                            plan.extend([Out::End { last: false }, Out::Start, Out::Doc(d)]);
                            plan.extend([Out::End { last: true }, Out::Close(")")]);
                        }
                        Node::Styled(style, d) => {
                            plan.extend([Out::Open("Styled("), Out::Start, Out::Value(style)]);
                            plan.extend([Out::End { last: false }, Out::Start, Out::Doc(d)]);
                            plan.extend([Out::End { last: true }, Out::Close(")")]);
                        }
                        Node::BeforeNonBlank(_, d) | Node::Flush(_, d) => {
                            let name = if matches!(doc.0, Node::Flush(..)) {
                                "Flush("
                            } else {
                                "BeforeNonBlank("
                            };
                            plan.extend([Out::Open(name), Out::Start, Out::Doc(d)]);
                            plan.extend([Out::End { last: true }, Out::Close(")")]);
                        }
                        leaf => plan.push(Out::Value(leaf)),
                    }
                    plan.extend([Out::End { last: true }, Out::Close(")")]);
                    work.extend(plan.into_iter().rev());
                }
                Out::Value(value) => {
                    if alternate {
                        let text = format!("{value:#?}");
                        let indent = format!("\n{}", "    ".repeat(depth));
                        f.write_str(&text.replace('\n', &indent))?;
                    } else {
                        write!(f, "{value:?}")?;
                    }
                }
                Out::Open(name) => {
                    f.write_str(name)?;
                    depth += 1;
                    if alternate {
                        f.write_str("\n")?;
                    }
                }
                Out::Close(bracket) => {
                    depth -= 1;
                    if alternate {
                        f.write_str(&"    ".repeat(depth))?;
                    }
                    f.write_str(bracket)?;
                }
                Out::Start => {
                    if alternate {
                        f.write_str(&"    ".repeat(depth))?;
                    }
                }
                Out::End { last } => {
                    if alternate {
                        f.write_str(",\n")?;
                    } else if !last {
                        f.write_str(", ")?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Drop for Doc {
    fn drop(&mut self) {
        if children(&self.0).is_empty() {
            return;
        }
        let mut nodes = vec![std::mem::take(&mut self.0)];
        while let Some(mut node) = nodes.pop() {
            steps::count(1);
            match &mut node {
                Node::Concat(items, _) => nodes.extend(items.iter_mut().map(Doc::take_node)),
                Node::Prefixed(_, inner)
                | Node::BeforeNonBlank(_, inner)
                | Node::Flush(_, inner)
                | Node::Styled(_, inner)
                | Node::Linked(_, inner) => nodes.push(inner.take_node()),
                _ => {}
            }
        }
    }
}

/// Counts the documents that a concatenation moves or copies, so that a
/// test can check that the cost is linear without a clock.
mod steps {
    #[cfg(test)]
    thread_local! {
        static STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    #[cfg(test)]
    pub(crate) fn count(n: usize) {
        STEPS.with(|s| s.set(s.get() + n));
    }

    #[cfg(not(test))]
    pub(crate) fn count(_: usize) {}

    /// Returns the count of `f`.
    #[cfg(test)]
    pub(crate) fn of(f: impl FnOnce()) -> usize {
        let before = STEPS.with(std::cell::Cell::get);
        f();
        STEPS.with(std::cell::Cell::get) - before
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
    /// empty string to the empty string. Pandoc changes the case of each
    /// character on its own, so a caller who wants the Pandoc result must
    /// map each character on its own and not use `str::to_lowercase`.
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
        if self.is_empty() {
            other
        } else if other.is_empty() {
            self
        } else {
            steps::count(1);
            join(self, other)
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
        steps::count(1);
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
            Node::BeforeNonBlank(_, inner) => {
                let followed = stack
                    .last()
                    .is_some_and(|&(next, _, c)| c == chain && is_non_blank(next));
                if followed {
                    stack.push((inner, prefix, chain));
                }
            }
            Node::Flush(_, inner) => {
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
            Node::Concat(items, _) => stack.extend(items.iter().rev().map(|i| (i, prefix, chain))),
        }
    }
    (longest, column)
}

/// Tells whether the first item of `doc` is text, through `flush` and
/// `before_non_blank` wrappers. The nodes keep the answer, so this is
/// constant work.
fn is_non_blank(doc: &Doc) -> bool {
    match &doc.0 {
        Node::Text(..) => true,
        Node::BeforeNonBlank(non_blank, _) | Node::Flush(non_blank, _) => *non_blank,
        Node::Concat(_, summary) => summary.non_blank,
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
        Shell::Flush.wrap(doc)
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
    Shell::BeforeNonBlank.wrap(doc)
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

/// Takes the right edge of `doc` apart: returns its last item and pushes
/// the documents before it on `before`, the nearest one last. The cost is
/// the depth of that edge.
fn split_last(mut doc: Doc, before: &mut Vec<Doc>) -> Doc {
    loop {
        steps::count(1);
        match doc.into_node() {
            Node::Concat(mut items, mut summary) => {
                let last = items.pop().unwrap_or_default();
                summary.items -= item_count(&last);
                before.push(match <[Doc; 1]>::try_from(items) {
                    Ok([single]) => single,
                    Err(items) => Doc(Node::Concat(items, summary)),
                });
                doc = last;
            }
            node => return Doc(node),
        }
    }
}

/// The mirror of [`split_last`] for the left edge. It pushes the items
/// after the first one by one, as `nestle` of `main` did, so the cost is
/// the number of items on that edge.
fn split_first(mut doc: Doc, after: &mut Vec<Doc>) -> Doc {
    loop {
        steps::count(1);
        match doc.into_node() {
            Node::Concat(mut items, _) => {
                steps::count(items.len());
                let rest = items.split_off(1.min(items.len()));
                after.extend(rest.into_iter().rev());
                doc = items.pop().unwrap_or_default();
            }
            node => return Doc(node),
        }
    }
}

/// Joins the documents that [`split_last`] took off in front of `last`.
fn join_before(mut before: Vec<Doc>, mut last: Doc) -> Doc {
    while let Some(doc) = before.pop() {
        last = doc + last;
    }
    last
}

/// Joins the documents that [`split_first`] took off after `first`.
fn join_after(mut first: Doc, mut after: Vec<Doc>) -> Doc {
    while let Some(doc) = after.pop() {
        first += doc;
    }
    first
}

/// Removes trailing spaces, line breaks and blank lines from `doc`.
pub fn chomp(doc: Doc) -> Doc {
    enum Context {
        Prefixed(String),
        Before(Vec<Doc>),
    }
    let mut contexts = Vec::new();
    let mut before = Vec::new();
    let mut current = doc;
    let mut result = loop {
        match split_last(current, &mut before).into_node() {
            Node::BlankLines(_) | Node::NewLine | Node::CarriageReturn | Node::BreakingSpace => {
                match before.pop() {
                    Some(previous) => current = previous,
                    None => break empty(),
                }
            }
            Node::Prefixed(prefix, mut inner) => {
                contexts.push(Context::Before(std::mem::take(&mut before)));
                contexts.push(Context::Prefixed(prefix));
                current = Doc(inner.take_node());
            }
            other => break join_before(std::mem::take(&mut before), Doc(other)),
        }
    };
    while let Some(context) = contexts.pop() {
        result = match context {
            Context::Prefixed(prefix) => Doc(Node::Prefixed(prefix, Box::new(result))),
            Context::Before(before) => join_before(before, result),
        };
    }
    result
}

/// Removes leading line breaks and blank lines from `doc`. A carriage
/// return stays when it is the last item, as in `doclayout`.
pub fn nestle(doc: Doc) -> Doc {
    let mut after = Vec::new();
    let mut current = doc;
    loop {
        let first = split_first(current, &mut after);
        if !matches!(
            first.0,
            Node::BlankLines(_) | Node::NewLine | Node::CarriageReturn
        ) {
            return join_after(first, after);
        }
        match after.pop() {
            Some(next) => current = next,
            None if matches!(first.0, Node::CarriageReturn) => return first,
            None => return empty(),
        }
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
/// The content is wrapped at the width, and longer words are split. The
/// width must be a real column width, because the block pads its lines
/// with spaces to that width.
pub fn lblock(width: usize, doc: Doc) -> Doc {
    block(width, &doc, |_, line| line)
}

/// A block like [`lblock`], with the content aligned to the right. The
/// width must be a real column width.
pub fn rblock(width: usize, doc: Doc) -> Doc {
    block(width, &doc, |w, line| {
        pad_left(w.wrapping_sub(render::line_length(&line)), line)
    })
}

/// A block like [`lblock`], with the content centered. The width must be
/// a real column width.
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
    let mut before = Vec::new();
    let last = split_last(doc, &mut before);
    if matches!(last.0, Node::NewLine | Node::CarriageReturn) {
        join_before(before, empty())
    } else {
        join_before(before, last)
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
        let lower = |d: &Doc| d.map_text(&|s| s.chars().flat_map(char::to_lowercase).collect());
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

    /// A before-non-blank item at the end of nested content has no
    /// follower in `doclayout`, so the offset does not count it. Expected
    /// values from `pandoc.layout`.
    #[test]
    fn offset_ignores_before_non_blank_at_end_of_nested_content() {
        let inner = || literal("a") + before_non_blank(literal("xx"));
        let styled = bold(inner()) + literal("b");
        assert_eq!((styled.offset(), styled.update_column(0)), (2, 2));
        let prefix = prefixed("> ", inner()) + literal("b");
        assert_eq!((prefix.offset(), prefix.update_column(0)), (4, 4));
        let flushed = flush(inner()) + literal("b");
        assert_eq!((flushed.offset(), flushed.update_column(0)), (2, 2));
    }

    /// I-LAYOUT-04: Pandoc gives -1 for this column, and the `usize`
    /// result is 0. The other queries are equal to Pandoc.
    #[test]
    fn negative_column_after_overflow_becomes_zero() {
        let max = isize::MAX.unsigned_abs();
        let doc = lblock(max, literal("")) + lblock(max, literal("")) + literal("c");
        let queries = (
            doc.offset(),
            doc.min_offset(),
            doc.height(),
            doc.update_column(0),
        );
        assert_eq!(queries, (0, 0, 1, 0));
    }

    /// Expected values from `pandoc.layout` in Pandoc 3.13.
    #[test]
    fn before_non_blank_looks_at_blocks_and_fills() {
        let after = |d: Doc| (before_non_blank(literal("x")) + d).render(None);
        assert_eq!(after(lblock(0, empty())), "x");
        assert_eq!(after(vfill("|")), "x|");
        assert_eq!(after(vfill("\t") + lblock(1, literal("a"))), "x\ta");
        assert_eq!(after(vfill(" ")), " ");
    }

    /// Skipped content is dropped without recursion. With 100,000 levels a
    /// recursive drop overflows the stack of the test thread.
    #[test]
    fn skipped_deep_content_drops_without_recursion() {
        let mut deep = literal("x");
        for _ in 0..100_000 {
            deep = prefixed("", deep);
        }
        let doc = before_non_blank(deep) + literal(" y");
        assert_eq!(doc.render(None), " y");
    }

    #[test]
    fn offset_looks_into_flushed_concatenations() {
        let joined = before_non_blank(literal("xx")) + flush(literal("a") + literal("b"));
        assert_eq!((joined.offset(), joined.update_column(0)), (4, 4));
        let broken = before_non_blank(literal("xx")) + flush(cr() + literal("b"));
        assert_eq!(broken.offset(), 1);
    }

    /// Review finding P03 (row I-HTML-28). Each builder makes a document
    /// with `n` levels or items and returns the step count of its
    /// concatenations. With 4 times the levels, a linear cost gives 4 times
    /// the steps and a quadratic cost gives 16 times the steps.
    fn assert_linear(name: &str, build: fn(usize) -> Doc) {
        let mut counts = [0; 2];
        for (count, n) in counts.iter_mut().zip([4_000, 16_000]) {
            *count = steps::of(|| drop(build(n)));
        }
        assert!(counts[0] > 0, "{name}: no steps");
        assert!(
            counts[1] <= counts[0] * 5,
            "{name}: {} steps at 4,000 and {} steps at 16,000",
            counts[0],
            counts[1]
        );
    }

    #[test]
    fn concatenation_steps_are_linear() {
        assert_linear("wrap with +", |n| {
            let mut doc = literal("x");
            for _ in 0..n {
                doc = literal("<div>") + literal("\n") + doc + literal("\n</div>");
            }
            doc
        });
        assert_linear("wrap with +=", |n| {
            let mut doc = literal("x");
            for _ in 0..n {
                let mut outer = literal("<div>");
                outer += literal("\n");
                outer += doc;
                outer += literal("\n</div>");
                doc = outer;
            }
            doc
        });
        assert_linear("grow on the left", |n| {
            let mut doc = literal("x");
            for _ in 0..n {
                doc = literal("a") + space() + doc;
            }
            doc
        });
        assert_linear("grow on the right", |n| {
            let mut doc = literal("x");
            for _ in 0..n {
                doc = doc + space() + literal("a");
            }
            doc
        });
    }

    #[test]
    fn concatenation_of_two_large_documents_is_constant() {
        let large = |n: usize| hsep((0..n).map(|_| literal("a")));
        let join = |n: usize| {
            let (a, b) = (large(n), large(n));
            let mut joined = empty();
            steps::of(|| joined = a + b)
        };
        assert_eq!(join(4_000), join(16_000));
        let mut a = large(16_000);
        let (b, c) = (large(16_000), large(1));
        assert_eq!(steps::of(|| a += b), steps::of(|| a += c));
        assert_eq!(a.render(None).len(), 63_999);
    }

    /// Runs `f` on a thread with a 2 MB stack.
    fn on_small_stack(f: impl FnOnce() + Send + 'static) {
        let thread = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(f)
            .unwrap_or_else(|e| panic!("spawn: {e}"));
        assert!(thread.join().is_ok(), "the thread failed");
    }

    /// Rule 21b: 100,000 appends on each side. Each operation of the crate
    /// runs on a 2 MB stack, and the drop at the end too.
    #[test]
    fn long_chains_use_no_call_stack() {
        for left in [false, true] {
            on_small_stack(move || {
                let mut doc = literal("x");
                for _ in 0..100_000 {
                    doc = if left {
                        literal("a") + space() + doc
                    } else {
                        doc + space() + literal("a")
                    };
                }
                let (first, last) = if left { ("a a", "a x") } else { ("x a", "a a") };
                let rendered = doc.render(None);
                assert!(rendered.starts_with(first) && rendered.ends_with(last));
                assert!(doc.render_ansi(Some(10)).starts_with(&first[..2]));
                assert_eq!((doc.offset(), doc.min_offset()), (200_001, 1));
                assert_eq!((doc.update_column(2), doc.height()), (200_003, 1));
                assert!(!doc.is_empty());
                let copy = doc.clone();
                assert!(copy == doc);
                let debug = format!("{doc:?}");
                let end = format!("Doc(BreakingSpace), Doc(Text(1, {:?}))]))", &last[2..]);
                assert!(debug.ends_with(&end));
                assert_eq!(doc.map_text(&str::to_uppercase).offset(), 200_001);
                assert_eq!(nowrap(copy).render(Some(1)).len(), 200_001);
                assert_eq!(chomp(doc.clone() + cr()).offset(), 200_001);
                assert_eq!(nestle(cr() + doc.clone()).offset(), 200_001);
                assert_eq!(remove_final_newline(doc.clone() + cr()).offset(), 200_001);
                let wrapped = (0..100_000).fold(literal("x"), |d, _| {
                    before_non_blank(literal("<")) + d + flush(literal(">"))
                });
                assert_eq!(wrapped.offset(), 200_001);
                assert!(wrapped.clone() == wrapped);
            });
        }
    }

    /// `==` and `Debug` see the sequence of items, not the shape of the
    /// tree, as the flat concatenation of the earlier version did.
    #[test]
    fn equality_and_debug_ignore_the_grouping() {
        let (a, b, c) = (|| literal("a"), || space(), || prefixed("> ", literal("c")));
        assert_eq!((a() + b()) + c(), a() + (b() + c()));
        assert_eq!(
            format!("{:?}", (a() + b()) + c()),
            format!("{:?}", a() + (b() + c()))
        );
        assert_eq!(empty(), empty());
        assert_ne!(a(), empty());
        assert_ne!(empty(), a());
        assert_ne!(a() + b(), a());
        assert_ne!(a(), a() + b());
        assert_ne!(a(), b());
        assert_ne!(prefixed("> ", a()), prefixed(">", a()));
        assert_ne!(link("x", a()), link("y", a()));
        assert_ne!(bold(a()), italic(a()));
        assert_ne!(flush(a()), before_non_blank(a()));
        assert_ne!(flush(a()) + b(), flush(a() + b()));
        assert_eq!(flush(a()), flush(a()));
        assert_eq!(before_non_blank(a()), before_non_blank(a()));
        assert_ne!(bold(a()), a());
    }

    /// `Debug` gives the text of the flat list that `main` gave. The
    /// expected text was recorded from the crate on `main` (`8b43988`).
    #[test]
    fn debug_text_matches_main() {
        let cases = [
            (empty(), "Doc(Empty)", "Doc(\n    Empty,\n)"),
            (
                literal("a"),
                "Doc(Text(1, \"a\"))",
                "Doc(\n    Text(\n        1,\n        \"a\",\n    ),\n)",
            ),
            (
                literal("a") + space(),
                "Doc(Concat([Doc(Text(1, \"a\")), Doc(BreakingSpace)]))",
                "Doc(\n    Concat(\n        [\n            Doc(\n                Text(\n                    1,\n                    \"a\",\n                ),\n            ),\n            Doc(\n                BreakingSpace,\n            ),\n        ],\n    ),\n)",
            ),
            (
                prefixed("> ", literal("q") + cr() + literal("r")),
                "Doc(Prefixed(\"> \", Doc(Concat([Doc(Text(1, \"q\")), Doc(CarriageReturn), Doc(Text(1, \"r\"))]))))",
                "Doc(\n    Prefixed(\n        \"> \",\n        Doc(\n            Concat(\n                [\n                    Doc(\n                        Text(\n                            1,\n                            \"q\",\n                        ),\n                    ),\n                    Doc(\n                        CarriageReturn,\n                    ),\n                    Doc(\n                        Text(\n                            1,\n                            \"r\",\n                        ),\n                    ),\n                ],\n            ),\n        ),\n    ),\n)",
            ),
            (
                before_non_blank(literal("x")) + flush(literal("a") + literal("b")),
                "Doc(Concat([Doc(BeforeNonBlank(Doc(Text(1, \"x\")))), Doc(Flush(Doc(Concat([Doc(Text(1, \"a\")), Doc(Text(1, \"b\"))]))))]))",
                "Doc(\n    Concat(\n        [\n            Doc(\n                BeforeNonBlank(\n                    Doc(\n                        Text(\n                            1,\n                            \"x\",\n                        ),\n                    ),\n                ),\n            ),\n            Doc(\n                Flush(\n                    Doc(\n                        Concat(\n                            [\n                                Doc(\n                                    Text(\n                                        1,\n                                        \"a\",\n                                    ),\n                                ),\n                                Doc(\n                                    Text(\n                                        1,\n                                        \"b\",\n                                    ),\n                                ),\n                            ],\n                        ),\n                    ),\n                ),\n            ),\n        ],\n    ),\n)",
            ),
            (
                bold(literal("b")) + fg(Color::Red, italic(literal("i") + space())),
                "Doc(Concat([Doc(Styled(Bold, Doc(Text(1, \"b\")))), Doc(Styled(Foreground(Red), Doc(Styled(Italic, Doc(Concat([Doc(Text(1, \"i\")), Doc(BreakingSpace)]))))))]))",
                "Doc(\n    Concat(\n        [\n            Doc(\n                Styled(\n                    Bold,\n                    Doc(\n                        Text(\n                            1,\n                            \"b\",\n                        ),\n                    ),\n                ),\n            ),\n            Doc(\n                Styled(\n                    Foreground(\n                        Red,\n                    ),\n                    Doc(\n                        Styled(\n                            Italic,\n                            Doc(\n                                Concat(\n                                    [\n                                        Doc(\n                                            Text(\n                                                1,\n                                                \"i\",\n                                            ),\n                                        ),\n                                        Doc(\n                                            BreakingSpace,\n                                        ),\n                                    ],\n                                ),\n                            ),\n                        ),\n                    ),\n                ),\n            ),\n        ],\n    ),\n)",
            ),
            (
                link("t", empty()) + link("u", literal("v") + after_break(".")),
                "Doc(Concat([Doc(Linked(\"t\", Doc(Empty))), Doc(Linked(\"u\", Doc(Concat([Doc(Text(1, \"v\")), Doc(AfterBreak(\".\"))]))))]))",
                "Doc(\n    Concat(\n        [\n            Doc(\n                Linked(\n                    \"t\",\n                    Doc(\n                        Empty,\n                    ),\n                ),\n            ),\n            Doc(\n                Linked(\n                    \"u\",\n                    Doc(\n                        Concat(\n                            [\n                                Doc(\n                                    Text(\n                                        1,\n                                        \"v\",\n                                    ),\n                                ),\n                                Doc(\n                                    AfterBreak(\n                                        \".\",\n                                    ),\n                                ),\n                            ],\n                        ),\n                    ),\n                ),\n            ),\n        ],\n    ),\n)",
            ),
            (
                blanklines(3) + char('é') + text("t"),
                "Doc(Concat([Doc(BlankLines(3)), Doc(Text(1, \"é\")), Doc(Text(1, \"t\"))]))",
                "Doc(\n    Concat(\n        [\n            Doc(\n                BlankLines(\n                    3,\n                ),\n            ),\n            Doc(\n                Text(\n                    1,\n                    \"é\",\n                ),\n            ),\n            Doc(\n                Text(\n                    1,\n                    \"t\",\n                ),\n            ),\n        ],\n    ),\n)",
            ),
        ];
        for (doc, plain, alternate) in cases {
            assert_eq!(format!("{doc:?}"), plain);
            assert_eq!(format!("{doc:#?}"), alternate);
        }
        let (a, b, c) = (|| literal("a"), || space(), || prefixed("> ", literal("c")));
        assert_eq!(
            format!("{:#?}", (a() + b()) + c()),
            format!("{:#?}", a() + (b() + c()))
        );
    }

    /// One step of a growing loop for each document shape of item 3 of the
    /// brief `layout-perf-b`.
    fn grow(shape: &str, i: usize, doc: Doc) -> Doc {
        let a = || literal("a");
        match shape {
            "left" => a() + space() + doc,
            "right" => doc + space() + a(),
            "wrap" => literal("<div>") + literal("\n") + doc + literal("\n</div>"),
            "before_non_blank" => before_non_blank(literal("x") + doc),
            "prefixed" => prefixed(">", a() + doc),
            "nest" => nest(2, a() + space() + doc),
            _ => match i % 5 {
                0 => bold(doc + a()),
                1 => flush(a() + doc),
                2 => link("t", doc) + a(),
                3 => before_non_blank(doc) + a(),
                _ => fg(Color::Red, cr() + doc),
            },
        }
    }

    /// Grows a document of each shape for `n` steps. Each step also runs
    /// the operations that were constant work on `main`. At the end each
    /// other operation runs one time.
    fn exercise(shape: &str, n: usize) {
        let mut doc = literal("x");
        let other = literal("p") + literal("q") + literal("r");
        for i in 0..n {
            doc = grow(shape, i, doc);
            assert!(doc != empty() && empty() != doc && !doc.is_empty());
            assert!(doc != literal("z") && literal("z") != doc);
            assert!(i < 2 || doc != other);
        }
        let ends = before_non_blank(literal("<")) + doc + flush(literal(">"));
        let doc = ends;
        assert!(!doc.render(Some(20)).is_empty());
        assert!(!doc.render_ansi(None).is_empty());
        assert!(doc.offset() >= doc.min_offset() && doc.update_column(3) > 0);
        assert!(doc.height() > 0);
        let copy = doc.clone();
        assert!(copy == doc);
        assert!(!format!("{doc:?}").is_empty());
        assert!(!doc.map_text(&str::to_uppercase).is_empty());
        assert!(!nowrap(copy).is_empty());
        assert!(!chomp(doc.clone() + cr()).is_empty());
        assert!(!nestle(blankline() + doc.clone()).is_empty());
        assert!(!remove_final_newline(doc.clone() + cr()).is_empty());
        assert!(!lblock(10, doc.clone()).is_empty());
        assert!(!hsep([doc.clone(), doc.clone()]).is_empty());
        drop(doc);
    }

    /// Review findings Codex R1-01 and Opus R1-01. Each operation runs on
    /// each shape; 4 times the steps of the loop must give at most 5 times
    /// the count.
    #[test]
    fn each_operation_is_linear_on_each_shape() {
        let shapes = [
            "left",
            "right",
            "wrap",
            "before_non_blank",
            "prefixed",
            "nest",
            "wrappers",
        ];
        for shape in shapes {
            let counts = [1_000, 4_000].map(|n| steps::of(|| exercise(shape, n)));
            assert!(counts[0] > 1_000, "{shape}: {counts:?}");
            assert!(counts[1] <= counts[0] * 5, "{shape}: {counts:?}");
        }
    }

    /// The inputs of the two findings. Codex R1-01: `d == empty()` in an
    /// append loop. Opus R1-01: nested `before_non_blank` in front of a long
    /// concatenation, then `offset`, `min_offset` and `update_column`.
    #[test]
    fn findings_of_round_one_are_linear() {
        let equality = |n: usize| {
            steps::of(|| {
                let mut d = literal("x");
                for _ in 0..n {
                    d += literal("a");
                    assert!(d != empty());
                }
            })
        };
        let nested = |n: usize| {
            steps::of(|| {
                let mut front = literal("<");
                let mut trail = literal("<");
                for _ in 0..n {
                    front = before_non_blank(front);
                    trail = before_non_blank(literal("x") + trail);
                }
                for d in [front, trail] {
                    let d = d + hcat((0..n).map(|_| literal("a")));
                    assert!(d.offset() >= n && d.min_offset() >= 1 && d.update_column(0) >= n);
                }
            })
        };
        for (name, count) in [
            ("equality", &equality as &dyn Fn(usize) -> usize),
            ("nested", &nested),
        ] {
            let counts = [4_000, 16_000].map(count);
            assert!(counts[1] <= counts[0] * 5, "{name}: {counts:?}");
        }
    }

    /// Rule 21b for the new code: 100,000 levels of each shape on a 2 MB
    /// stack.
    #[test]
    fn each_shape_uses_no_call_stack() {
        for shape in ["before_non_blank", "wrappers", "wrap", "left"] {
            on_small_stack(move || exercise(shape, 100_000));
        }
    }
}
