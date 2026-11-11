// Ported from `doclayout` 0.6 (BSD-3-Clause), Copyright John MacFarlane.
// See third_party/licences/doclayout.txt.
//! The layout engine: it flattens a [`Doc`] into a list of items and lays
//! the items out into lines.

use crate::style::{Font, StyleReq};
use crate::width::{char_width, chars_length, is_space, str_length};
use crate::{Doc, Node};

/// A run of output text with one font and one link target.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Attr {
    pub(crate) link: Option<String>,
    pub(crate) font: Font,
    pub(crate) text: String,
}

impl Attr {
    fn plain(text: String) -> Attr {
        Attr {
            link: None,
            font: Font::default(),
            text,
        }
    }
}

/// One output line of a block.
pub(crate) type Line = Vec<Attr>;

pub(crate) fn line_length(line: &[Attr]) -> isize {
    chars_length(line.iter().flat_map(|attr| attr.text.chars()))
}

fn line_starts_blank(line: &[Attr]) -> bool {
    line.iter()
        .flat_map(|attr| attr.text.chars())
        .next()
        .is_some_and(is_space)
}

fn text_starts_blank(text: &str) -> bool {
    text.chars().next().is_some_and(is_space)
}

/// A layout item. Styled and linked content becomes a pair of open and
/// close items around the content.
#[derive(Debug, Clone)]
enum Flat<'a> {
    Text(isize, &'a str),
    Block(isize, &'a [Line]),
    VFill(isize, &'a str),
    Cooked(isize, Line),
    Prefixed(&'a str, Vec<Flat<'a>>),
    BeforeNonBlank(Vec<Flat<'a>>),
    Flush(Vec<Flat<'a>>),
    BreakingSpace,
    AfterBreak(Vec<Flat<'a>>),
    CarriageReturn,
    NewLine,
    BlankLines(usize),
    StyleOpen(StyleReq),
    StyleClose,
    LinkOpen(&'a str),
    LinkClose,
}

impl<'a> Flat<'a> {
    fn is_breakable(&self) -> bool {
        matches!(
            self,
            Flat::BreakingSpace | Flat::CarriageReturn | Flat::NewLine | Flat::BlankLines(_)
        )
    }

    fn is_printable(&self) -> bool {
        !matches!(
            self,
            Flat::StyleOpen(_) | Flat::StyleClose | Flat::LinkOpen(_) | Flat::LinkClose
        )
    }

    fn is_block(&self) -> bool {
        matches!(self, Flat::Block(..) | Flat::VFill(..))
    }

    fn offset(&self) -> isize {
        match self {
            Flat::Text(width, _)
            | Flat::Block(width, _)
            | Flat::VFill(width, _)
            | Flat::Cooked(width, _) => *width,
            Flat::BreakingSpace => 1,
            _ => 0,
        }
    }

    fn starts_blank(&self) -> bool {
        let mut item = self;
        loop {
            return match item {
                Flat::Text(_, text) => text_starts_blank(text),
                Flat::Cooked(_, line) => line_starts_blank(line),
                Flat::Block(width, lines) => {
                    *width > 0 && lines.iter().all(|l| line_starts_blank(l))
                }
                Flat::VFill(width, text) => *width > 0 && text_starts_blank(text),
                Flat::BeforeNonBlank(items)
                | Flat::Prefixed(_, items)
                | Flat::Flush(items)
                | Flat::AfterBreak(items) => match items.first() {
                    Some(first) => {
                        item = first;
                        continue;
                    }
                    None => false,
                },
                Flat::BreakingSpace
                | Flat::CarriageReturn
                | Flat::NewLine
                | Flat::BlankLines(_)
                | Flat::StyleOpen(_)
                | Flat::StyleClose
                | Flat::LinkOpen(_)
                | Flat::LinkClose => true,
            };
        }
    }

    fn children_mut(&mut self) -> Option<&mut Vec<Flat<'a>>> {
        match self {
            Flat::Prefixed(_, items)
            | Flat::BeforeNonBlank(items)
            | Flat::Flush(items)
            | Flat::AfterBreak(items) => Some(items),
            _ => None,
        }
    }
}

/// A wrapper item whose content `flatten` is still collecting.
enum Open<'a> {
    Prefixed(&'a str),
    BeforeNonBlank,
    Flush,
}

fn flatten(doc: &Doc) -> Vec<Flat<'_>> {
    enum Work<'a> {
        Doc(&'a Doc),
        Emit(Flat<'a>),
        Close(Open<'a>),
    }
    let mut levels: Vec<Vec<Flat<'_>>> = vec![Vec::new()];
    let mut work = vec![Work::Doc(doc)];
    while let Some(step) = work.pop() {
        crate::steps::count(1);
        match step {
            Work::Emit(item) => push_flat(&mut levels, item),
            Work::Close(kind) => {
                let items = levels.pop().unwrap_or_default();
                if !items.is_empty() {
                    push_flat(
                        &mut levels,
                        match kind {
                            Open::Prefixed(prefix) => Flat::Prefixed(prefix, items),
                            Open::BeforeNonBlank => Flat::BeforeNonBlank(items),
                            Open::Flush => Flat::Flush(items),
                        },
                    );
                }
            }
            Work::Doc(d) => match &d.0 {
                Node::Empty => {}
                Node::Text(width, text) => push_flat(&mut levels, Flat::Text(*width, text)),
                Node::Block(width, lines) => push_flat(&mut levels, Flat::Block(*width, lines)),
                Node::VFill(width, text) => push_flat(&mut levels, Flat::VFill(*width, text)),
                Node::Prefixed(prefix, inner) => {
                    levels.push(Vec::new());
                    work.push(Work::Close(Open::Prefixed(prefix)));
                    work.push(Work::Doc(inner));
                }
                Node::BeforeNonBlank(_, inner) => {
                    levels.push(Vec::new());
                    work.push(Work::Close(Open::BeforeNonBlank));
                    work.push(Work::Doc(inner));
                }
                Node::Flush(_, inner) => {
                    levels.push(Vec::new());
                    work.push(Work::Close(Open::Flush));
                    work.push(Work::Doc(inner));
                }
                Node::BreakingSpace => push_flat(&mut levels, Flat::BreakingSpace),
                Node::CarriageReturn => push_flat(&mut levels, Flat::CarriageReturn),
                Node::NewLine => push_flat(&mut levels, Flat::NewLine),
                Node::BlankLines(n) => push_flat(&mut levels, Flat::BlankLines(*n)),
                Node::AfterBreak(text) => {
                    let mut items = Vec::new();
                    flatten_literal(text, &mut items);
                    if !items.is_empty() {
                        push_flat(&mut levels, Flat::AfterBreak(items));
                    }
                }
                Node::Concat(items, _) => work.extend(items.iter().rev().map(Work::Doc)),
                Node::Styled(style, inner) => {
                    push_flat(&mut levels, Flat::StyleOpen(*style));
                    work.push(Work::Emit(Flat::StyleClose));
                    work.push(Work::Doc(inner));
                }
                Node::Linked(target, inner) => {
                    push_flat(&mut levels, Flat::LinkOpen(target));
                    work.push(Work::Emit(Flat::LinkClose));
                    work.push(Work::Doc(inner));
                }
            },
        }
    }
    levels.pop().unwrap_or_default()
}

fn push_flat<'a>(levels: &mut [Vec<Flat<'a>>], item: Flat<'a>) {
    if let Some(level) = levels.last_mut() {
        level.push(item);
    }
}

/// Drops nested items without recursion.
fn dispose(items: Vec<Flat<'_>>) {
    let mut pending = vec![items];
    while let Some(mut items) = pending.pop() {
        for item in &mut items {
            if let Some(children) = item.children_mut() {
                pending.push(std::mem::take(children));
            }
        }
    }
}

/// Flattens the text as `literal` would make it.
fn flatten_literal<'a>(text: &'a str, out: &mut Vec<Flat<'a>>) {
    for (index, piece) in text.split('\n').enumerate() {
        if index > 0 {
            out.push(Flat::NewLine);
        }
        if !piece.is_empty() {
            out.push(Flat::Text(str_length(piece), piece));
        }
    }
}

/// Joins two adjacent line-break items into one, or returns `None`.
fn join<'a>(first: &Flat<'a>, second: &Flat<'a>) -> Option<Flat<'a>> {
    use Flat::{BlankLines, BreakingSpace, CarriageReturn, NewLine};
    match (first, second) {
        (BlankLines(m), BlankLines(n)) => Some(BlankLines(*m.max(n))),
        (BlankLines(m), BreakingSpace | CarriageReturn | NewLine) => Some(BlankLines(*m)),
        (NewLine | CarriageReturn | BreakingSpace, BlankLines(m)) => Some(BlankLines(*m)),
        (NewLine, BreakingSpace | CarriageReturn) => Some(NewLine),
        (CarriageReturn, CarriageReturn | BreakingSpace) => Some(CarriageReturn),
        (BreakingSpace, CarriageReturn) => Some(CarriageReturn),
        (BreakingSpace, NewLine) => Some(NewLine),
        (BreakingSpace, BreakingSpace) => Some(BreakingSpace),
        _ => None,
    }
}

/// Joins adjacent line breaks and spaces. A line break at the end becomes a
/// carriage return, and a space at the end goes away.
fn normalize(items: Vec<Flat<'_>>) -> Vec<Flat<'_>> {
    let mut out = Vec::with_capacity(items.len());
    let mut rest = items.into_iter().peekable();
    let mut head = rest.next();
    while let Some(current) = head {
        match rest.peek() {
            None => {
                match current {
                    Flat::NewLine | Flat::BlankLines(_) => out.push(Flat::CarriageReturn),
                    Flat::BreakingSpace => {}
                    other => out.push(other),
                }
                head = None;
            }
            Some(next) => match join(&current, next) {
                Some(joined) => {
                    rest.next();
                    head = Some(joined);
                }
                None => {
                    out.push(current);
                    head = rest.next();
                }
            },
        }
    }
    out
}

#[derive(Clone)]
struct Prefix {
    text: String,
    trimmed: String,
    length: isize,
}

impl Prefix {
    fn new(text: String) -> Prefix {
        let trimmed = text.trim_end_matches(is_space).to_owned();
        let length = str_length(&text);
        Prefix {
            text,
            trimmed,
            length,
        }
    }
}

struct State<'a> {
    output: Vec<Attr>,
    prefix: Prefix,
    use_prefix: bool,
    line_length: Option<isize>,
    column: isize,
    newlines: usize,
    fonts: Vec<Font>,
    link: Option<&'a str>,
}

impl<'a> State<'a> {
    fn font(&self) -> Font {
        self.fonts.last().copied().unwrap_or_default()
    }

    fn push(&mut self, attr: Attr) {
        if !attr.text.is_empty() {
            self.output.push(attr);
        }
    }

    fn newline(&mut self) {
        if self.column == 0 && self.use_prefix && !self.prefix.text.is_empty() {
            let trimmed = self.prefix.trimmed.clone();
            self.push(Attr::plain(trimmed));
        }
        self.push(Attr::plain("\n".to_owned()));
        self.column = 0;
        self.newlines += 1;
    }

    fn current_prefix(&self) -> &str {
        if self.use_prefix {
            &self.prefix.text
        } else {
            ""
        }
    }

    fn out_text(&mut self, width: isize, text: &str) {
        let font = self.font();
        let prefix = self.current_prefix().to_owned();
        let attr = Attr {
            link: self.link.map(str::to_owned),
            font,
            text: text.to_owned(),
        };
        if self.column == 0 && !(prefix.is_empty() && font == Font::default()) {
            self.push(Attr::plain(prefix));
            self.push(attr);
            self.column = self.prefix.length.wrapping_add(width);
        } else {
            self.push(attr);
            self.column = self.column.wrapping_add(width);
        }
        self.newlines = 0;
    }

    fn out_cooked(&mut self, width: isize, line: Line) {
        let prefix = self.current_prefix().to_owned();
        if self.column == 0 && !prefix.is_empty() {
            self.push(Attr::plain(prefix));
            self.column = self.prefix.length.wrapping_add(width);
        } else {
            self.column = self.column.wrapping_add(width);
        }
        for attr in line {
            self.push(attr);
        }
        self.newlines = 0;
    }

    /// Lays out the items. Each nested list is a frame on an explicit
    /// stack, so the nesting depth does not use the call stack.
    fn render_list(&mut self, items: Vec<Flat<'a>>) {
        let mut frames = vec![Frame {
            items,
            next: 0,
            restore: Restore::Nothing,
        }];
        while let Some(frame) = frames.last_mut() {
            if frame.next >= frame.items.len() {
                if let Some(done) = frames.pop() {
                    match done.restore {
                        Restore::Nothing => {}
                        Restore::Prefix(prefix) => self.prefix = prefix,
                        Restore::UsePrefix(use_prefix) => self.use_prefix = use_prefix,
                    }
                }
                continue;
            }
            if let Some(child) = self.step(frame) {
                frames.push(child);
            }
        }
    }

    /// Lays out the next item of `frame`. Returns a frame for the content of
    /// the item when the item has content to lay out.
    fn step(&mut self, frame: &mut Frame<'a>) -> Option<Frame<'a>> {
        let i = frame.next;
        frame.next = i + 1;
        let items = &mut frame.items;
        let item = items.get_mut(i)?;
        match item {
            Flat::Text(width, text) => {
                let (width, text) = (*width, *text);
                self.out_text(width, text);
            }
            Flat::Cooked(width, line) => {
                let width = *width;
                let line = std::mem::take(line);
                self.out_cooked(width, line);
            }
            Flat::StyleOpen(style) => {
                let font = self.font().with(*style);
                self.fonts.push(font);
            }
            Flat::StyleClose => {
                self.fonts.pop();
            }
            Flat::LinkOpen(target) => {
                if self.link.is_none() {
                    self.link = Some(*target);
                } else {
                    let mut rest = items.split_off(i + 1);
                    if let Some(close) = rest.iter().position(|x| matches!(x, Flat::LinkClose)) {
                        rest.remove(close);
                    }
                    *items = rest;
                    frame.next = 0;
                }
            }
            Flat::LinkClose => self.link = None,
            Flat::Prefixed(prefix, inner) => {
                let inner = std::mem::take(inner);
                let saved = self.prefix.clone();
                self.prefix = Prefix::new(format!("{}{}", saved.text, prefix));
                return Some(Frame::new(normalize(inner), Restore::Prefix(saved)));
            }
            Flat::Flush(inner) => {
                let inner = std::mem::take(inner);
                let saved = self.use_prefix;
                self.use_prefix = false;
                return Some(Frame::new(normalize(inner), Restore::UsePrefix(saved)));
            }
            Flat::BeforeNonBlank(inner) => {
                let inner = std::mem::take(inner);
                let next = items.iter().skip(i + 1).find(|x| x.is_printable());
                if next.is_some_and(|x| !x.starts_blank()) {
                    return Some(Frame::new(normalize(inner), Restore::Nothing));
                }
                dispose(inner);
            }
            Flat::BlankLines(num) => {
                let num = to_isize(*num);
                let newlines = to_isize(self.newlines);
                if newlines <= num {
                    let count = 1isize.wrapping_add(num).wrapping_sub(newlines);
                    for _ in 0..count {
                        self.newline();
                    }
                }
            }
            Flat::CarriageReturn => {
                if self.newlines == 0 {
                    self.newline();
                }
            }
            Flat::NewLine => self.newline(),
            Flat::BreakingSpace => {
                let mut j = i + 1;
                while items
                    .get(j)
                    .is_some_and(|x| matches!(x, Flat::BreakingSpace))
                {
                    j += 1;
                }
                let following = items
                    .iter()
                    .skip(j)
                    .take_while(|x| !x.is_breakable())
                    .fold(0isize, |sum, x| sum.wrapping_add(x.offset()));
                match self.line_length {
                    Some(limit) if self.column.wrapping_add(1).wrapping_add(following) > limit => {
                        self.newline();
                    }
                    _ => {
                        if self.column > 0 {
                            self.out_text(1, " ");
                        }
                    }
                }
                frame.next = j;
            }
            Flat::AfterBreak(inner) => {
                let inner = std::mem::take(inner);
                if self.newlines > 0 {
                    return Some(Frame::new(inner, Restore::Nothing));
                }
            }
            Flat::Block(..) | Flat::VFill(..) => {
                let mut j = i + 1;
                while items.get(j).is_some_and(Flat::is_block) {
                    j += 1;
                }
                let blocks: Vec<Flat<'a>> = items.drain(i..j).collect();
                frame.next = i;
                return Some(self.render_blocks(&blocks));
            }
        }
        None
    }

    /// Joins adjacent blocks line by line. Returns the frame that lays out
    /// the joined lines with the current column as the prefix.
    fn render_blocks(&mut self, blocks: &[Flat<'a>]) -> Frame<'a> {
        let font = self.font();
        let height = blocks
            .iter()
            .map(|b| match b {
                Flat::Block(_, lines) => lines.len(),
                _ => 1,
            })
            .max()
            .unwrap_or(0);
        let spec = |b: &Flat<'a>| -> BlockSpec {
            match b {
                Flat::Block(width, lines) => (
                    *width,
                    lines.iter().map(|l| (line_length(l), l.clone())).collect(),
                ),
                Flat::VFill(width, text) => {
                    let attr = Attr {
                        link: self.link.map(str::to_owned),
                        font,
                        text: (*text).to_owned(),
                    };
                    (*width, vec![(str_length(text), vec![attr]); height])
                }
                _ => (0, Vec::new()),
            }
        };
        let mut specs = blocks.iter();
        let first = specs.next().map(spec).unwrap_or_default();
        let (_, lines) = specs.fold(first, |acc, b| merge_blocks(height, acc, spec(b)));
        let saved = self.prefix.clone();
        let extra = self.column.wrapping_sub(saved.length);
        if extra > 0 {
            let spaces = " ".repeat(extra.unsigned_abs());
            self.prefix.text.push_str(&spaces);
            self.prefix.length = self.prefix.length.wrapping_add(extra);
        }
        let mut cooked = Vec::new();
        for (_, line) in lines {
            if line.iter().all(|attr| attr.text.is_empty()) {
                continue;
            }
            if !cooked.is_empty() {
                cooked.push(Flat::CarriageReturn);
            }
            cooked.push(Flat::Cooked(line_length(&line), line));
        }
        Frame::new(cooked, Restore::Prefix(saved))
    }
}

/// What to reset when a frame ends.
enum Restore {
    Nothing,
    Prefix(Prefix),
    UsePrefix(bool),
}

/// A list of items that the layout engine works through.
struct Frame<'a> {
    items: Vec<Flat<'a>>,
    next: usize,
    restore: Restore,
}

impl<'a> Frame<'a> {
    fn new(items: Vec<Flat<'a>>, restore: Restore) -> Frame<'a> {
        Frame {
            items,
            next: 0,
            restore,
        }
    }
}

fn to_isize(n: usize) -> isize {
    isize::try_from(n).unwrap_or(isize::MAX)
}

type BlockSpec = (isize, Vec<(isize, Line)>);

fn spaces(count: isize) -> Attr {
    Attr::plain(" ".repeat(usize::try_from(count).unwrap_or(0)))
}

fn merge_blocks(height: usize, first: BlockSpec, second: BlockSpec) -> BlockSpec {
    let (w1, mut lines1) = first;
    let (w2, mut lines2) = second;
    lines1.resize(height, (0, Vec::new()));
    lines2.resize(height, (0, Vec::new()));
    let merged = lines1
        .into_iter()
        .zip(lines2)
        .map(|((len1, mut l1), (len2, l2))| {
            l1.push(spaces(w1.wrapping_sub(len1)));
            l1.extend(l2);
            (w1.wrapping_add(len2), l1)
        })
        .collect();
    (w1.wrapping_add(w2), merged)
}

/// Lays out a document into attributed output text.
pub(crate) fn prerender(doc: &Doc, line_length: Option<isize>) -> Vec<Attr> {
    let items = flatten(doc);
    let mut state = State {
        output: Vec::new(),
        prefix: Prefix::new(String::new()),
        use_prefix: true,
        line_length,
        column: 0,
        newlines: 2,
        fonts: Vec::new(),
        link: None,
    };
    state.render_list(normalize(items));
    state.output.retain(|attr| !attr.text.is_empty());
    state.output
}

pub(crate) fn render_plain(doc: &Doc, line_length: Option<isize>) -> String {
    prerender(doc, line_length)
        .into_iter()
        .map(|attr| attr.text)
        .collect()
}

pub(crate) fn render_ansi(doc: &Doc, line_length: Option<isize>) -> String {
    let mut out = String::new();
    let mut link: Option<String> = None;
    let mut font = Font::default();
    for attr in prerender(doc, line_length) {
        if attr.font != font {
            attr.font.write_sgr(&mut out);
        }
        if attr.link != link {
            crate::style::write_osc8(attr.link.as_deref(), &mut out);
        }
        out.push_str(&attr.text);
        font = attr.font;
        link = attr.link;
    }
    Font::default().write_sgr(&mut out);
    crate::style::write_osc8(None, &mut out);
    out
}

/// Splits attributed text into lines at each newline.
fn split_lines(attrs: Vec<Attr>) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut current = Vec::new();
    for attr in attrs {
        for (index, piece) in attr.text.split('\n').enumerate() {
            if index > 0 {
                lines.push(std::mem::take(&mut current));
            }
            if !piece.is_empty() {
                current.push(Attr {
                    link: attr.link.clone(),
                    font: attr.font,
                    text: piece.to_owned(),
                });
            }
        }
    }
    lines.push(current);
    lines
}

/// Splits each line that is wider than `width` into lines of at most
/// `width` columns. A character of width 0 never starts a new line.
pub(crate) fn chop(width: isize, attrs: Vec<Attr>) -> Vec<Line> {
    let mut lines: Vec<(isize, Line)> = split_lines(attrs)
        .into_iter()
        .map(|line| (line_length(&line), line))
        .collect();
    if lines.last().is_some_and(|(len, _)| *len == 0) {
        lines.pop();
    }
    let mut out = Vec::new();
    for (len, line) in lines {
        if len <= width {
            out.push(line);
            continue;
        }
        let mut used = 0;
        let mut current: Line = Vec::new();
        for attr in line {
            let mut pending = String::new();
            for c in attr.text.chars() {
                let cw = isize::try_from(char_width(c)).unwrap_or(0);
                if used + cw > width && used > 0 {
                    if !pending.is_empty() {
                        current.push(Attr {
                            link: attr.link.clone(),
                            font: attr.font,
                            text: std::mem::take(&mut pending),
                        });
                    }
                    out.push(std::mem::take(&mut current));
                    used = cw;
                } else {
                    used += cw;
                }
                pending.push(c);
            }
            if !pending.is_empty() {
                current.push(Attr {
                    link: attr.link,
                    font: attr.font,
                    text: pending,
                });
            }
        }
        out.push(current);
    }
    out
}
