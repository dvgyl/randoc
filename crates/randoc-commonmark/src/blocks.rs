//! The block parser. It reads the input line by line and builds a tree of
//! blocks, as the CommonMark reference implementations do. Inline content
//! stays raw text here; `convert` parses it later.

use std::collections::HashMap;

use randoc_ast::Attr;

use crate::Config;
use crate::attrs;
use crate::inlines::LinkDef;
use crate::scan::{
    is_space_or_tab, normalize_label, scan_closing_tag, scan_link_destination, scan_link_label,
    scan_link_title, scan_open_tag, skip_spnl, unescape, unescape_destination,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListKind {
    Bullet(u8),
    /// The number style and the delimiter: `.`, `)` or `(` for two parentheses.
    Ordered(NumStyle, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumStyle {
    Decimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ListData {
    pub(crate) kind: ListKind,
    pub(crate) start: i64,
    pub(crate) tight: bool,
    pub(crate) marker_offset: usize,
    pub(crate) padding: usize,
    pub(crate) task: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    Document,
    BlockQuote,
    List(ListData),
    Item(ListData),
    Paragraph,
    Heading {
        level: i64,
        setext: bool,
    },
    ThematicBreak,
    CodeBlock {
        fenced: bool,
        fence_char: u8,
        fence_len: usize,
        fence_offset: usize,
        info: String,
    },
    HtmlBlock(u8),
    /// A pipe table. The index points into `Tree::tables`.
    Table(usize),
    FootnoteDef(String),
    Div {
        fence_len: usize,
        attr: Attr,
    },
    DefList,
    /// A term and its definitions. The term is the content.
    DefItem {
        tight: bool,
    },
    Definition {
        indent: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Align {
    Left,
    Right,
    Center,
    Default,
}

#[derive(Debug)]
pub(crate) struct Node {
    pub(crate) kind: Kind,
    pub(crate) parent: usize,
    pub(crate) children: Vec<usize>,
    pub(crate) open: bool,
    pub(crate) last_line_blank: bool,
    pub(crate) start_line: usize,
    pub(crate) depth: usize,
    pub(crate) content: String,
    pub(crate) attr: Attr,
    /// Attributes from attribute lines before the block.
    pub(crate) block_attrs: Option<attrs::Pairs>,
    /// An empty paragraph that stays.
    pub(crate) keep: bool,
    /// The line that set `last_line_blank`. A newer value of a child wins at finalization.
    pub(crate) blank_line: usize,
    /// The blank lines that this container got directly (`blockBlanks` in `Commonmark/Blocks.hs`).
    pub(crate) blanks: Vec<usize>,
    /// For a list item: a blank line ends the item (`listItemBlanksAtEnd`).
    pub(crate) blank_end: bool,
    /// For a list item: a blank line is inside the item (`listItemBlanksInside`).
    pub(crate) blank_inside: bool,
}

/// The cells of a pipe table.
#[derive(Debug, Default)]
pub(crate) struct TableData {
    pub(crate) aligns: Vec<Align>,
    pub(crate) header: Vec<String>,
    pub(crate) rows: Vec<Vec<String>>,
    /// The number of cells that the body rows hold in the source.
    pub(crate) cells: usize,
}

/// The largest number of cells that the reader adds to the short rows of one table.
/// A row after that ends the table (`pipeTableBlockSpec` in
/// `Commonmark/Extensions/PipeTable.hs`).
const MAX_ADDED_CELLS: usize = 200_000;

/// The result of block parsing.
pub(crate) struct Tree {
    pub(crate) nodes: Vec<Node>,
    pub(crate) refs: HashMap<String, LinkDef>,
    pub(crate) tables: Vec<TableData>,
    /// The footnote definitions, in the order in which they closed. A closed
    /// definition is not a child of a block, as in `Commonmark/Extensions/Footnote.hs`.
    pub(crate) notes: Vec<usize>,
}

struct Parser<'c> {
    config: &'c Config,
    /// The input ended: the finalizations of the open blocks run.
    at_end: bool,
    nodes: Vec<Node>,
    tables: Vec<TableData>,
    /// The open fenced divs, outermost first, with their fence lengths.
    /// Each entry has the node, the fence length and the index of the nearest
    /// entry below with a shorter fence, so that a search skips longer fences.
    open_divs: Vec<(usize, usize, Option<usize>)>,
    /// For each div that starts a chain of nested divs, the deepest open div of the chain.
    div_chain_end: HashMap<usize, usize>,
    /// The line number and the div that the closing fence of that line closes.
    div_close: Option<(usize, usize, Option<usize>)>,
    /// A run of spaces of the line that is known: start offset and end offset.
    spaces: Option<(usize, usize)>,
    /// The last line has no line ending (a metadata value).
    open_last_line: bool,
    /// The whole text and the byte offset of each line, for attribute groups that span lines.
    full: String,
    line_starts: Vec<usize>,
    /// Lines that an attribute group of an earlier line holds.
    skip_lines: usize,
    /// A task item with no text after its box takes the next line when that
    /// line is blank (`taskListItemBlockSpec` reads it in its start), so the
    /// line is no blank line of any container.
    skip_blank_line: bool,
    is_last_line: bool,
    refs: HashMap<String, LinkDef>,
    notes: Vec<usize>,
    tip: usize,
    oldtip: usize,
    line: String,
    line_number: usize,
    offset: usize,
    column: usize,
    next_nonspace: usize,
    next_nonspace_column: usize,
    indent: usize,
    indented: bool,
    blank: bool,
    partially_consumed_tab: bool,
    all_closed: bool,
    last_matched_container: usize,
    consumed: bool,
    pending_attrs: Option<attrs::Pairs>,
    /// The container and the last line of the last block of attribute lines.
    attr_block: Option<(usize, usize)>,
    /// The position of the last byte of the line that cannot be in a thematic break.
    last_other: Option<usize>,
}

/// Result of a block start: no match, a container, or a leaf that takes the line.
#[derive(PartialEq, Eq)]
enum Start {
    None,
    Container,
    Leaf,
}

/// Result of a block continuation.
#[derive(PartialEq, Eq)]
enum Continue {
    Matched,
    NotMatched,
    LineDone,
}

pub(crate) fn parse(text: &str, config: &Config) -> Tree {
    parse_with(text, config, false)
}

/// Parses blocks. With `open_last_line`, the last line of a text without a
/// final line ending has no line ending, as in a metadata value: block starts
/// that need a line ending do not match on it.
pub(crate) fn parse_with(text: &str, config: &Config, open_last_line: bool) -> Tree {
    let mut parser = Parser {
        config,
        tables: Vec::new(),
        open_divs: Vec::new(),
        div_chain_end: HashMap::new(),
        div_close: None,
        spaces: None,
        open_last_line: open_last_line && !text.ends_with('\n'),
        full: String::new(),
        line_starts: Vec::new(),
        skip_lines: 0,
        skip_blank_line: false,
        is_last_line: false,
        nodes: vec![Node {
            kind: Kind::Document,
            parent: 0,
            children: Vec::new(),
            open: true,
            last_line_blank: false,
            start_line: 1,
            depth: 0,
            content: String::new(),
            attr: Attr::default(),
            block_attrs: None,
            keep: false,
            blank_line: 0,
            blanks: Vec::new(),
            blank_end: false,
            blank_inside: false,
        }],
        refs: HashMap::new(),
        notes: Vec::new(),
        tip: 0,
        oldtip: 0,
        line: String::new(),
        line_number: 0,
        offset: 0,
        column: 0,
        next_nonspace: 0,
        next_nonspace_column: 0,
        indent: 0,
        indented: false,
        blank: false,
        partially_consumed_tab: false,
        all_closed: true,
        last_matched_container: 0,
        consumed: false,
        pending_attrs: None,
        attr_block: None,
        last_other: None,
        at_end: false,
    };
    let text = text.replace('\0', "\u{FFFD}");
    let mut start = 0;
    for line in text.split('\n') {
        parser.line_starts.push(start);
        start += line.len() + 1;
    }
    parser.full = text.clone();
    let mut lines: Vec<&str> = text.split('\n').collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    let count = lines.len();
    for (index, line) in lines.into_iter().enumerate() {
        parser.is_last_line = parser.open_last_line && index + 1 == count;
        parser.incorporate_line(line);
    }
    parser.at_end = true;
    while parser.tip != 0 {
        let tip = parser.tip;
        parser.finalize(tip);
    }
    parser.finalize(0);
    Tree {
        nodes: parser.nodes,
        refs: parser.refs,
        tables: parser.tables,
        notes: parser.notes,
    }
}

impl Parser<'_> {
    fn byte(&self, pos: usize) -> u8 {
        self.line.as_bytes().get(pos).copied().unwrap_or(0)
    }

    fn kind(&self, index: usize) -> &Kind {
        self.nodes.get(index).map_or(&Kind::Document, |n| &n.kind)
    }

    fn last_child(&self, index: usize) -> Option<usize> {
        self.nodes
            .get(index)
            .and_then(|n| n.children.last().copied())
    }

    /// The last child that stays in the tree: an open footnote definition leaves
    /// the tree when it closes, so the child before it counts.
    fn last_block(&self, index: usize) -> Option<usize> {
        let children = &self.nodes.get(index)?.children;
        let last = children.last().copied()?;
        if matches!(self.kind(last), Kind::FootnoteDef(_)) {
            children
                .len()
                .checked_sub(2)
                .and_then(|i| children.get(i).copied())
        } else {
            Some(last)
        }
    }

    fn is_open(&self, index: usize) -> bool {
        self.nodes.get(index).is_some_and(|n| n.open)
    }

    fn parent(&self, index: usize) -> usize {
        self.nodes.get(index).map_or(0, |n| n.parent)
    }

    fn find_next_nonspace(&mut self) {
        if let Some((start, end)) = self.spaces
            && start <= self.offset
            && self.offset <= end
        {
            self.blank = end >= self.line.len();
            self.next_nonspace = end;
            self.next_nonspace_column = self.column + (end - self.offset);
            self.indent = end - self.offset;
            self.indented = self.indent >= 4;
            return;
        }
        let mut i = self.offset;
        let mut cols = self.column;
        let mut tab = false;
        loop {
            crate::steps::add(1);
            match self.byte(i) {
                b' ' => {
                    i += 1;
                    cols += 1;
                }
                b'\t' => {
                    i += 1;
                    cols += 4 - cols % 4;
                    tab = true;
                }
                _ => break,
            }
        }
        if !tab && !self.partially_consumed_tab {
            self.spaces = Some((self.offset, i));
        }
        self.blank = i >= self.line.len();
        self.next_nonspace = i;
        self.next_nonspace_column = cols;
        self.indent = cols - self.column;
        self.indented = self.indent >= 4;
    }

    fn advance_next_nonspace(&mut self) {
        self.offset = self.next_nonspace;
        self.column = self.next_nonspace_column;
        self.partially_consumed_tab = false;
    }

    fn advance_offset(&mut self, mut count: usize, columns: bool) {
        while count > 0 && self.offset < self.line.len() {
            if self.byte(self.offset) == b'\t' {
                let to_tab = 4 - self.column % 4;
                if columns {
                    self.partially_consumed_tab = to_tab > count;
                    let advance = to_tab.min(count);
                    self.column += advance;
                    if !self.partially_consumed_tab {
                        self.offset += 1;
                    }
                    count -= advance;
                } else {
                    self.partially_consumed_tab = false;
                    self.column += to_tab;
                    self.offset += 1;
                    count -= 1;
                }
            } else {
                self.partially_consumed_tab = false;
                self.offset += 1;
                self.column += 1;
                count -= 1;
            }
        }
    }

    fn rest(&self) -> &str {
        self.line.get(self.next_nonspace..).unwrap_or("")
    }

    fn add_line(&mut self) {
        let mut text = String::new();
        if self.partially_consumed_tab {
            self.offset += 1;
            let to_tab = 4 - self.column % 4;
            text.push_str(&" ".repeat(to_tab));
        }
        text.push_str(self.line.get(self.offset..).unwrap_or(""));
        if !self.is_last_line {
            text.push('\n');
        }
        let tip = self.tip;
        if let Some(node) = self.nodes.get_mut(tip) {
            node.content.push_str(&text);
        }
    }

    fn can_contain(&self, parent: usize, child: &Kind) -> bool {
        match self.kind(parent) {
            Kind::Document
            | Kind::BlockQuote
            | Kind::Item(_)
            | Kind::FootnoteDef(_)
            | Kind::Definition { .. }
            | Kind::Div { .. } => !matches!(child, Kind::Item(_)),
            Kind::List(_) => matches!(child, Kind::Item(_)),
            Kind::DefList => matches!(child, Kind::DefItem { .. }),
            Kind::DefItem { .. } => matches!(child, Kind::Definition { .. }),
            _ => false,
        }
    }

    fn add_child(&mut self, kind: Kind) -> usize {
        while !self.can_contain(self.tip, &kind) {
            let tip = self.tip;
            self.finalize(tip);
        }
        let parent = self.tip;
        let depth = self.nodes.get(parent).map_or(0, |n| n.depth) + 1;
        let index = self.nodes.len();
        self.nodes.push(Node {
            kind,
            parent,
            children: Vec::new(),
            open: true,
            last_line_blank: false,
            start_line: self.line_number,
            depth,
            content: String::new(),
            attr: Attr::default(),
            block_attrs: None,
            keep: false,
            blank_line: 0,
            blanks: Vec::new(),
            blank_end: false,
            blank_inside: false,
        });
        if let Some(node) = self.nodes.get_mut(parent) {
            node.children.push(index);
        }
        if let Some(pairs) = self.pending_attrs.take()
            && let Some(node) = self.nodes.get_mut(index)
        {
            node.block_attrs = Some(pairs);
        }
        self.tip = index;
        index
    }

    fn close_unmatched_blocks(&mut self) {
        if !self.all_closed {
            while self.oldtip != self.last_matched_container {
                let parent = self.parent(self.oldtip);
                let oldtip = self.oldtip;
                self.finalize(oldtip);
                self.oldtip = parent;
            }
            self.all_closed = true;
        }
    }

    fn depth_ok(&self) -> bool {
        self.nodes.get(self.tip).map_or(0, |n| n.depth) + 2 < self.config.max_depth
    }

    fn incorporate_line(&mut self, line: &str) {
        if self.skip_lines > 0 {
            self.skip_lines -= 1;
            self.line_number += 1;
            return;
        }
        if std::mem::take(&mut self.skip_blank_line) && line.bytes().all(is_space_or_tab) {
            self.line_number += 1;
            return;
        }
        self.line = line.to_owned();
        crate::steps::add(1 + line.len() as u64 / 64);
        self.spaces = None;
        self.last_other = line
            .bytes()
            .rposition(|b| !matches!(b, b' ' | b'\t' | b'*' | b'-' | b'_'));
        self.offset = 0;
        self.column = 0;
        self.blank = false;
        self.partially_consumed_tab = false;
        self.consumed = false;
        self.line_number += 1;
        let mut container = 0;
        self.oldtip = self.tip;
        while let Some(last) = self.last_child(container) {
            crate::steps::add(1);
            if !self.is_open(last) {
                break;
            }
            container = last;
            self.find_next_nonspace();
            if matches!(self.kind(container), Kind::Div { .. }) {
                let target = self.div_close_target();
                let end = self.div_chain_end(container);
                let depth = |node: usize| self.nodes.get(node).map_or(0, |n| n.depth);
                match target {
                    Some(t) if depth(container) <= depth(t) && depth(t) <= depth(end) => {
                        container = t;
                    }
                    _ => {
                        container = end;
                        continue;
                    }
                }
            }
            match self.continue_block(container) {
                Continue::Matched => {}
                Continue::NotMatched => {
                    container = self.parent(container);
                    break;
                }
                Continue::LineDone => return,
            }
        }
        self.all_closed = container == self.oldtip;
        self.last_matched_container = container;
        let mut matched_leaf =
            !matches!(self.kind(container), Kind::Paragraph) && accepts_lines(self.kind(container));
        while !matched_leaf {
            self.find_next_nonspace();
            if !self.indented && !maybe_special(self.byte(self.next_nonspace)) {
                self.advance_next_nonspace();
                break;
            }
            match self.try_starts(container) {
                Start::Container => container = self.tip,
                Start::Leaf => {
                    container = self.tip;
                    matched_leaf = true;
                }
                Start::None => {
                    self.advance_next_nonspace();
                    break;
                }
            }
        }
        if self.consumed {
            self.close_unmatched_blocks();
        } else if !self.all_closed && !self.blank && matches!(self.kind(self.tip), Kind::Paragraph)
        {
            self.add_line();
        } else {
            self.close_unmatched_blocks();
            let line = self.line_number;
            if self.blank
                && let Some(last) = self.last_child(container)
                && let Some(node) = self.nodes.get_mut(last)
            {
                node.last_line_blank = true;
                node.blank_line = line;
            }
            let kind = self.kind(container).clone();
            // A block start on the line makes it no blank line (`maybeBlank` upstream).
            if self.blank
                && container == self.last_matched_container
                && !accepts_lines(&kind)
                && let Some(node) = self.nodes.get_mut(container)
            {
                node.blanks.push(line);
            }
            let last_line_blank = self.blank
                && !(matches!(kind, Kind::BlockQuote)
                    || matches!(kind, Kind::CodeBlock { .. })
                    || matches!(kind, Kind::Div { .. })
                    || (matches!(kind, Kind::Item(_))
                        && self.nodes.get(container).is_some_and(|n| {
                            n.children.is_empty() && n.start_line == self.line_number
                        })));
            // The value reaches the ancestors when they are finalized.
            let line = self.line_number;
            if let Some(node) = self.nodes.get_mut(container) {
                node.last_line_blank = last_line_blank;
                node.blank_line = line;
            }
            if matches!(kind, Kind::Table(_)) {
                self.add_table_row(container);
            } else if accepts_lines(&kind) {
                self.add_line();
                // On the first line of a comment, the end `-->` comes after `<!--`.
                let skip = if kind == Kind::HtmlBlock(2)
                    && self
                        .nodes
                        .get(container)
                        .is_some_and(|n| n.start_line == self.line_number)
                {
                    self.line
                        .get(self.offset..)
                        .and_then(|rest| rest.find("<!--"))
                        .map_or(self.offset, |at| self.offset + at + 4)
                } else {
                    self.offset
                };
                if let Kind::HtmlBlock(t @ 1..=5) = kind
                    && html_block_ends(t, self.line.get(skip..).unwrap_or(""))
                {
                    self.finalize(container);
                }
            } else if self.offset < self.line.len() && !self.blank {
                self.add_child(Kind::Paragraph);
                self.advance_next_nonspace();
                self.add_line();
            }
        }
    }

    fn continue_block(&mut self, container: usize) -> Continue {
        let kind = self.kind(container).clone();
        match kind {
            Kind::Document | Kind::List(_) => Continue::Matched,
            Kind::BlockQuote => {
                if !self.indented && self.byte(self.next_nonspace) == b'>' {
                    self.advance_next_nonspace();
                    self.advance_offset(1, false);
                    if is_space_or_tab(self.byte(self.offset)) {
                        self.advance_offset(1, true);
                    }
                    Continue::Matched
                } else {
                    Continue::NotMatched
                }
            }
            Kind::Item(data) => {
                // After a blank line, an item with no closed child continues only on a blank
                // line (`listItemSpec`). The open last child is on the node stack upstream, not a
                // child yet. A block of attribute lines is a child of the item upstream.
                let (blanks, closed) = self.nodes.get(container).map_or((false, 0), |n| {
                    let open_last = n.children.last().is_some_and(|&c| self.is_open(c));
                    (
                        !n.blanks.is_empty(),
                        n.children.len() - usize::from(open_last),
                    )
                });
                let no_children =
                    closed == 0 && self.attr_block.is_none_or(|(block, _)| block != container);
                if blanks && no_children && !self.blank {
                    return Continue::NotMatched;
                }
                if self.blank {
                    self.advance_next_nonspace();
                } else if self.indent >= data.marker_offset + data.padding {
                    self.advance_offset(data.marker_offset + data.padding, true);
                } else {
                    return Continue::NotMatched;
                }
                Continue::Matched
            }
            Kind::FootnoteDef(_) => {
                if self.blank {
                    self.advance_next_nonspace();
                } else if self.indent >= 4 {
                    self.advance_offset(4, true);
                } else {
                    return Continue::NotMatched;
                }
                Continue::Matched
            }
            Kind::Div { .. } => {
                if self.div_close_target() == Some(container) {
                    let parent = self.parent(container);
                    while self.tip != parent {
                        let tip = self.tip;
                        self.finalize(tip);
                        if tip == container {
                            break;
                        }
                    }
                    self.add_empty_paragraph();
                    return Continue::LineDone;
                }
                Continue::Matched
            }
            Kind::DefList | Kind::DefItem { .. } => Continue::Matched,
            Kind::Definition { indent } => {
                if self.indent >= indent {
                    self.advance_offset(indent, true);
                } else if self.blank {
                    self.advance_next_nonspace();
                } else {
                    return Continue::NotMatched;
                }
                Continue::Matched
            }
            Kind::Heading { .. } | Kind::ThematicBreak => Continue::NotMatched,
            Kind::CodeBlock {
                fenced,
                fence_char,
                fence_len,
                fence_offset,
                ..
            } => {
                if fenced {
                    let rest = self.rest().as_bytes();
                    let run = rest.iter().take_while(|&&b| b == fence_char).count();
                    let after = rest.get(run..).unwrap_or(&[]);
                    if self.indent <= 3
                        && run >= fence_len
                        && run >= 3
                        && after.iter().all(|&b| is_space_or_tab(b))
                    {
                        self.finalize(container);
                        self.add_empty_paragraph();
                        return Continue::LineDone;
                    }
                    let mut i = fence_offset;
                    while i > 0 && is_space_or_tab(self.byte(self.offset)) {
                        self.advance_offset(1, true);
                        i -= 1;
                    }
                    Continue::Matched
                } else if self.indent >= 4 {
                    self.advance_offset(4, true);
                    Continue::Matched
                } else if self.blank {
                    self.advance_next_nonspace();
                    Continue::Matched
                } else {
                    Continue::NotMatched
                }
            }
            Kind::HtmlBlock(t) => {
                if self.blank && (t == 6 || t == 7) {
                    Continue::NotMatched
                } else {
                    Continue::Matched
                }
            }
            Kind::Paragraph => {
                if self.blank {
                    Continue::NotMatched
                } else {
                    Continue::Matched
                }
            }
            Kind::Table(_) => {
                let full = match kind {
                    Kind::Table(table) => self.tables.get(table).is_some_and(|data| {
                        data.rows.len() * data.aligns.len() - data.cells > MAX_ADDED_CELLS
                    }),
                    _ => false,
                };
                if self.blank || self.is_last_line || full || self.table_row().is_none() {
                    Continue::NotMatched
                } else {
                    Continue::Matched
                }
            }
        }
    }

    /// Returns the open div that the closing fence of this line closes: the
    /// innermost open div with a fence that is not longer than the line's fence.
    fn div_close_target(&mut self) -> Option<usize> {
        if let Some((line, offset, target)) = self.div_close
            && line == self.line_number
            && offset == self.offset
        {
            return target;
        }
        let rest = self.rest();
        let colons = rest.bytes().take_while(|&b| b == b':').count();
        let closing = !self.indented
            && colons >= 3
            && rest
                .get(colons..)
                .is_some_and(|r| r.bytes().all(is_space_or_tab));
        let mut target = None;
        if closing {
            let mut cursor = self.open_divs.len().checked_sub(1);
            while let Some(index) = cursor {
                crate::steps::add(1);
                let Some(&(node, fence, shorter)) = self.open_divs.get(index) else {
                    break;
                };
                if fence <= colons {
                    target = Some(node);
                    break;
                }
                cursor = shorter;
            }
        }
        self.div_close = Some((self.line_number, self.offset, target));
        target
    }

    /// Returns the deepest open div that a chain of nested open divs reaches from `top`.
    fn div_chain_end(&mut self, top: usize) -> usize {
        let mut end = self.div_chain_end.get(&top).copied().unwrap_or(top);
        while end != top && !self.is_open(end) {
            crate::steps::add(1);
            end = self.parent(end);
        }
        while let Some(child) = self.last_child(end) {
            crate::steps::add(1);
            if self.is_open(child) && matches!(self.kind(child), Kind::Div { .. }) {
                end = child;
            } else {
                break;
            }
        }
        self.div_chain_end.insert(top, end);
        end
    }

    fn try_starts(&mut self, container: usize) -> Start {
        let config = self.config;
        if config.attributes {
            let start = self.start_attributes(container);
            if start != Start::None {
                return start;
            }
        }
        let start = self.start_block_quote();
        if start != Start::None {
            return start;
        }
        let start = self.start_atx_heading();
        if start != Start::None {
            return start;
        }
        let start = self.start_fenced_code();
        if start != Start::None {
            return start;
        }
        if config.fenced_divs {
            let start = self.start_div();
            if start != Start::None {
                return start;
            }
        }
        if config.definition_lists {
            let start = self.start_definition(container);
            if start != Start::None {
                return start;
            }
        }
        let start = self.start_html_block(container);
        if start != Start::None {
            return start;
        }
        let start = self.start_setext_heading(container);
        if start != Start::None {
            return start;
        }
        if config.fancy_lists {
            let start = self.start_list_item(container);
            if start != Start::None {
                return start;
            }
        }
        let start = self.start_thematic_break();
        if start != Start::None {
            return start;
        }
        if config.footnotes {
            let start = self.start_footnote(container);
            if start != Start::None {
                return start;
            }
        }
        let start = self.start_list_item(container);
        if start != Start::None {
            return start;
        }
        if config.pipe_tables {
            let start = self.start_table(container);
            if start != Start::None {
                return start;
            }
        }
        self.start_indented_code()
    }

    fn start_attributes(&mut self, container: usize) -> Start {
        if self.indented
            || self.byte(self.next_nonspace) != b'{'
            || matches!(self.kind(container), Kind::Paragraph)
        {
            return Start::None;
        }
        let rest = self.rest();
        let pairs = match attrs::parse_attributes(rest, 0) {
            Some((end, pairs))
                if rest
                    .get(end..)
                    .is_some_and(|r| r.bytes().all(is_space_or_tab)) =>
            {
                pairs
            }
            _ => match self.multiline_attributes(self.next_nonspace) {
                Some((pairs, lines)) => {
                    self.skip_lines = lines;
                    pairs
                }
                None => return Start::None,
            },
        };
        self.close_unmatched_blocks();
        // Attribute lines that follow each other are one block. A new block replaces the
        // attributes that wait (`attributeSpec` and `addNodeToStack` in `Commonmark/Blocks.hs`).
        let follows = self
            .attr_block
            .is_some_and(|(block, last)| block == container && last + 1 == self.line_number);
        match &mut self.pending_attrs {
            Some(waiting) if follows => waiting.extend(pairs),
            waiting => *waiting = Some(pairs),
        }
        self.attr_block = Some((container, self.line_number + self.skip_lines));
        self.consumed = true;
        self.offset = self.line.len();
        Start::Leaf
    }

    fn push_open_div(&mut self, node: usize, fence: usize) {
        let mut shorter = self.open_divs.len().checked_sub(1);
        while let Some(index) = shorter {
            crate::steps::add(1);
            match self.open_divs.get(index) {
                Some(&(_, other, below)) if other >= fence => shorter = below,
                _ => break,
            }
        }
        self.open_divs.push((node, fence, shorter));
    }

    /// Reads an attribute group that starts at `offset` of this line and ends on
    /// a later line, with only white space after it on that line, as the
    /// `commonmark` parser reads the raw input. Returns the attributes and the
    /// number of later lines that the group holds.
    fn multiline_attributes(&self, offset: usize) -> Option<(attrs::Pairs, usize)> {
        let index = self.line_number.checked_sub(1)?;
        let start = self.line_starts.get(index)? + offset;
        let (end, pairs) = attrs::parse_attributes(&self.full, start)?;
        let line_end = self
            .full
            .get(end..)?
            .find('\n')
            .map_or(self.full.len(), |i| end + i);
        if !self.full.get(end..line_end)?.bytes().all(is_space_or_tab) {
            return None;
        }
        let last = self
            .line_starts
            .partition_point(|&s| s <= end)
            .checked_sub(1)?;
        let lines = last.checked_sub(index)?;
        (lines > 0).then_some((pairs, lines))
    }

    /// Returns `true` when an attribute group starts at `offset` of this line and ends on a later line.
    fn attribute_group_ends(&self, offset: usize) -> bool {
        let Some(start) = self
            .line_number
            .checked_sub(1)
            .and_then(|index| self.line_starts.get(index))
            .map(|s| s + offset)
        else {
            return false;
        };
        let line_end = self
            .full
            .get(start..)
            .and_then(|r| r.find('\n'))
            .map(|i| start + i);
        attrs::parse_attributes(&self.full, start)
            .is_some_and(|(end, _)| line_end.is_some_and(|line_end| end > line_end))
    }

    fn start_block_quote(&mut self) -> Start {
        if self.indented || self.byte(self.next_nonspace) != b'>' || !self.depth_ok() {
            return Start::None;
        }
        self.advance_next_nonspace();
        self.advance_offset(1, false);
        if is_space_or_tab(self.byte(self.offset)) {
            self.advance_offset(1, true);
        }
        self.close_unmatched_blocks();
        let index = self.add_child(Kind::BlockQuote);
        if self.config.alerts
            && self
                .nodes
                .get(index)
                .is_some_and(|n| n.start_line == self.line_number)
        {
            let rest = self
                .line
                .get(self.offset..)
                .unwrap_or("")
                .trim_end_matches([' ', '\t']);
            if let Some(name) = rest.strip_prefix("[!").and_then(|r| r.strip_suffix(']'))
                && alert_title(name).is_some()
            {
                if let Some(node) = self.nodes.get_mut(index) {
                    node.content = name.to_ascii_uppercase();
                }
                self.consumed = true;
                self.offset = self.line.len();
                self.add_empty_paragraph();
                return Start::Leaf;
            }
        }
        Start::Container
    }

    fn start_atx_heading(&mut self) -> Start {
        if self.indented {
            return Start::None;
        }
        let Some(level) = atx_level(self.rest()) else {
            return Start::None;
        };
        self.advance_next_nonspace();
        self.advance_offset(level, false);
        self.close_unmatched_blocks();
        let content = self.line.get(self.offset..).unwrap_or("").to_owned();
        let mut content = strip_closing_hashes(&content).to_owned();
        let mut attr = Attr::default();
        if self.config.attributes {
            let (rest, pairs) = trailing_attributes(&content);
            if let Some(pairs) = pairs {
                content = strip_closing_hashes(rest).to_owned();
                // A heading with no text keeps no attributes.
                if !content.trim().is_empty() {
                    attr = attrs::merge(pairs, Attr::default());
                }
            }
        }
        let index = self.add_child(Kind::Heading {
            level: level as i64,
            setext: false,
        });
        if let Some(node) = self.nodes.get_mut(index) {
            node.content = content;
            node.attr = attr;
        }
        self.offset = self.line.len();
        self.consumed = true;
        self.close_unmatched_blocks();
        self.add_empty_paragraph();
        Start::Leaf
    }

    /// On an open last line, a block that takes the whole line is followed by an
    /// empty paragraph, as the `commonmark` parser makes one at the end of input.
    fn add_empty_paragraph(&mut self) {
        if !self.is_last_line {
            return;
        }
        let mut parent = self.tip;
        while matches!(
            self.kind(parent),
            Kind::Heading { .. } | Kind::CodeBlock { .. }
        ) {
            let tip = self.tip;
            self.finalize(tip);
            parent = self.tip;
        }
        if matches!(self.kind(parent), Kind::Item(_)) {
            return;
        }
        let index = self.add_child(Kind::Paragraph);
        if let Some(node) = self.nodes.get_mut(index) {
            node.keep = true;
        }
    }

    fn start_fenced_code(&mut self) -> Start {
        if self.indented {
            return Start::None;
        }
        let rest = self.rest();
        let fence_char = match rest.as_bytes().first() {
            Some(&c @ (b'`' | b'~')) => c,
            _ => return Start::None,
        };
        let fence_len = rest.bytes().take_while(|&b| b == fence_char).count();
        if fence_len < 3 {
            return Start::None;
        }
        let info = rest.get(fence_len..).unwrap_or("");
        if fence_char == b'`' && info.contains('`') {
            return Start::None;
        }
        let info = info.trim_matches([' ', '\t']).to_owned();
        self.close_unmatched_blocks();
        let fence_offset = self.indent;
        self.add_child(Kind::CodeBlock {
            fenced: true,
            fence_char,
            fence_len,
            fence_offset,
            info,
        });
        self.consumed = true;
        self.offset = self.line.len();
        Start::Leaf
    }

    fn start_div(&mut self) -> Start {
        if self.indented || !self.depth_ok() {
            return Start::None;
        }
        let rest = self.rest();
        let fence_len = rest.bytes().take_while(|&b| b == b':').count();
        if fence_len < 3 {
            return Start::None;
        }
        let after = rest.get(fence_len..).unwrap_or("");
        let spec = after.trim_matches([' ', '\t']);
        if spec.is_empty() {
            return Start::None;
        }
        let spec_offset = self.next_nonspace
            + fence_len
            + (after.len() - after.trim_start_matches([' ', '\t']).len());
        let attr = match attrs::parse_attributes(spec, 0) {
            Some((end, pairs)) if spec.get(end..).is_some_and(|r| r.trim().is_empty()) => {
                attrs::merge(pairs, Attr::default())
            }
            _ if spec.starts_with('{') && self.multiline_attributes(spec_offset).is_some() => {
                let (pairs, lines) = self.multiline_attributes(spec_offset).unwrap_or_default();
                self.skip_lines = lines;
                attrs::merge(pairs, Attr::default())
            }
            // A complete group with text after it on its last line is no div start.
            _ if spec.starts_with('{') && self.attribute_group_ends(spec_offset) => {
                return Start::None;
            }
            _ => {
                if spec.contains([' ', '\t']) {
                    return Start::None;
                }
                Attr::new("", vec![spec.to_owned()], Vec::new())
            }
        };
        self.close_unmatched_blocks();
        let index = self.add_child(Kind::Div {
            fence_len,
            attr: Attr::default(),
        });
        self.push_open_div(index, fence_len);
        if let Some(node) = self.nodes.get_mut(index) {
            node.attr = attr;
        }
        self.offset = self.line.len();
        Start::Container
    }

    fn start_html_block(&mut self, container: usize) -> Start {
        if self.indented || self.byte(self.next_nonspace) != b'<' {
            return Start::None;
        }
        let rest = self.rest().to_owned();
        let Some(kind) = html_block_start(&rest) else {
            return Start::None;
        };
        if kind == 7 && (self.is_last_line || matches!(self.kind(container), Kind::Paragraph)) {
            return Start::None;
        }
        self.close_unmatched_blocks();
        self.add_child(Kind::HtmlBlock(kind));
        Start::Leaf
    }

    fn start_setext_heading(&mut self, container: usize) -> Start {
        if self.indented || !matches!(self.kind(container), Kind::Paragraph) {
            return Start::None;
        }
        let rest = self.rest();
        let level = match rest.as_bytes().first() {
            Some(b'=') => 1,
            Some(b'-') => 2,
            _ => return Start::None,
        };
        let marker = if level == 1 { b'=' } else { b'-' };
        let run = rest.bytes().take_while(|&b| b == marker).count();
        if !rest
            .get(run..)
            .is_some_and(|r| r.bytes().all(is_space_or_tab))
        {
            return Start::None;
        }
        self.close_unmatched_blocks();
        let content = self
            .nodes
            .get(container)
            .map(|n| n.content.clone())
            .unwrap_or_default();
        let content = self.extract_refs(container, &content);
        if content.trim().is_empty() {
            if let Some(node) = self.nodes.get_mut(container) {
                node.content = content;
            }
            return Start::None;
        }
        let mut content = content;
        let mut attr = Attr::default();
        if self.config.attributes {
            let trimmed = content.trim_end().to_owned();
            let (rest, pairs) = trailing_attributes(&trimmed);
            if let Some(pairs) = pairs {
                attr = attrs::merge(pairs, Attr::default());
                content = rest.to_owned();
            }
        }
        if let Some(node) = self.nodes.get_mut(container) {
            node.kind = Kind::Heading {
                level,
                setext: true,
            };
            node.content = content;
            node.attr = attr;
        }
        self.offset = self.line.len();
        Start::Leaf
    }

    fn start_thematic_break(&mut self) -> Start {
        let other_after = self.last_other.is_some_and(|i| i >= self.next_nonspace);
        if self.indented || self.is_last_line || other_after || !is_thematic_break(self.rest()) {
            return Start::None;
        }
        self.close_unmatched_blocks();
        self.add_child(Kind::ThematicBreak);
        self.offset = self.line.len();
        Start::Leaf
    }

    fn start_table(&mut self, container: usize) -> Start {
        if self.indented || self.is_last_line || !matches!(self.kind(container), Kind::Paragraph) {
            return Start::None;
        }
        let Some(aligns) = parse_dividers(self.rest()) else {
            return Start::None;
        };
        let content = self
            .nodes
            .get(container)
            .map(|n| n.content.clone())
            .unwrap_or_default();
        let lines: Vec<&str> = content.trim_end_matches('\n').split('\n').collect();
        let [header_line] = lines.as_slice() else {
            return Start::None;
        };
        let Some(header) = parse_cells(header_line) else {
            return Start::None;
        };
        if header.len() != aligns.len() {
            return Start::None;
        }
        self.close_unmatched_blocks();
        let table = self.tables.len();
        self.tables.push(TableData {
            aligns,
            header,
            rows: Vec::new(),
            cells: 0,
        });
        if let Some(node) = self.nodes.get_mut(container) {
            node.kind = Kind::Table(table);
            node.content.clear();
        }
        self.consumed = true;
        self.offset = self.line.len();
        Start::Leaf
    }

    /// The cells of this line as a table row. The parser removes at most 3 spaces
    /// before a row, so a fourth space is an empty cell before the first `|`
    /// (`pCells` in `Commonmark/Extensions/PipeTable.hs`).
    fn table_row(&self) -> Option<Vec<String>> {
        let line = self.rest();
        let lead = self.indent >= 4 && line.starts_with('|');
        let mut cells = match parse_cells(line) {
            Some(cells) => cells,
            None if lead && line.trim_matches([' ', '\t']) == "|" => Vec::new(),
            None => return None,
        };
        if lead {
            cells.insert(0, String::new());
        }
        Some(cells)
    }

    fn add_table_row(&mut self, container: usize) {
        let mut cells = self
            .table_row()
            .unwrap_or_else(|| vec![self.rest().trim().to_owned()]);
        if let Kind::Table(table) = *self.kind(container)
            && let Some(data) = self.tables.get_mut(table)
        {
            cells.truncate(data.aligns.len());
            data.cells += cells.len();
            while cells.len() < data.aligns.len() {
                cells.push(String::new());
            }
            data.rows.push(cells);
        }
    }

    fn start_footnote(&mut self, _container: usize) -> Start {
        if self.indented || !self.depth_ok() {
            return Start::None;
        }
        let rest = self.rest().as_bytes();
        if rest.get(..2) != Some(b"[^") {
            return Start::None;
        }
        let Some(end) = crate::scan::scan_note_label(rest, 0) else {
            return Start::None;
        };
        if rest.get(end) != Some(&b':') {
            return Start::None;
        }
        let label = String::from_utf8_lossy(rest.get(2..end - 1).unwrap_or(&[])).into_owned();
        if label.trim().is_empty() {
            return Start::None;
        }
        self.close_unmatched_blocks();
        self.advance_next_nonspace();
        self.advance_offset(end + 1, false);
        self.add_child(Kind::FootnoteDef(normalize_label(&label)));
        Start::Container
    }

    fn start_list_item(&mut self, container: usize) -> Start {
        let in_list = matches!(self.kind(container), Kind::List(_));
        if (self.indented && !in_list) || self.indent >= 4 || !self.depth_ok() {
            return Start::None;
        }
        let rest: Vec<u8> = self.rest().bytes().take(16).collect();
        let in_paragraph = matches!(self.kind(container), Kind::Paragraph);
        // A task list is not a parent list for the marker rules, as in the oracle.
        let parent = match self.kind(container) {
            Kind::List(data) if data.task.is_none() => Some(data.kind),
            _ => None,
        };
        let (kind, start, marker_len) = match rest.first() {
            Some(&c @ (b'*' | b'+' | b'-')) => (ListKind::Bullet(c), 1, 1),
            _ => match ordered_marker(&rest, self.config.fancy_lists, parent) {
                Some((style, delim, number, len)) => {
                    if in_paragraph && (number != 1 || style != NumStyle::Decimal) {
                        return Start::None;
                    }
                    (ListKind::Ordered(style, delim), number, len)
                }
                None => return Start::None,
            },
        };
        match rest.get(marker_len) {
            None if self.is_last_line => return Start::None,
            None | Some(b' ' | b'\t') => {}
            _ => return Start::None,
        }
        if in_paragraph
            && self
                .rest()
                .as_bytes()
                .get(marker_len..)
                .is_none_or(|r| r.iter().all(|&b| is_space_or_tab(b)))
        {
            return Start::None;
        }
        let marker_offset = self.indent;
        self.advance_next_nonspace();
        self.advance_offset(marker_len, true);
        let spaces_start_col = self.column;
        let spaces_start_offset = self.offset;
        loop {
            self.advance_offset(1, true);
            if !(self.column - spaces_start_col < 5 && is_space_or_tab(self.byte(self.offset))) {
                break;
            }
        }
        let blank_item = self.offset >= self.line.len();
        let spaces_after = self.column - spaces_start_col;
        let padding = if !(1..5).contains(&spaces_after) || blank_item {
            self.column = spaces_start_col;
            self.offset = spaces_start_offset;
            if is_space_or_tab(self.byte(self.offset)) {
                self.advance_offset(1, true);
            }
            marker_len + 1
        } else {
            marker_len + spaces_after
        };
        let mut task = None;
        let mut task_text = None;
        if self.config.task_lists && !blank_item && matches!(kind, ListKind::Bullet(_)) {
            let tail = self.line.get(self.offset..).unwrap_or("").as_bytes();
            // The marker is `[x]`, `[X]` or `[` with only spaces and `]`; the scan stops at the first other byte.
            let close = if tail.first() == Some(&b'[') {
                let spaces = tail.iter().skip(1).take_while(|&&b| b == b' ').count();
                match tail.get(1 + spaces) {
                    Some(b']') => 1 + spaces,
                    Some(b'x' | b'X') if spaces == 0 && tail.get(2) == Some(&b']') => 2,
                    _ => 0,
                }
            } else {
                0
            };
            let inner = tail.get(1..close).unwrap_or(&[]);
            let checked = matches!(inner, [b'x' | b'X']);
            if tail.first() == Some(&b'[')
                && close > 1
                && (checked || inner.iter().all(|&b| b == b' '))
                && tail.get(close + 1).is_none_or(|&b| is_space_or_tab(b))
            {
                task = Some(checked);
                let rest =
                    String::from_utf8_lossy(tail.get(close + 1..).unwrap_or(&[])).into_owned();
                if rest.trim().is_empty() {
                    self.offset = self.line.len();
                    self.skip_blank_line = true;
                } else {
                    task_text = Some(rest.trim_start_matches([' ', '\t']).to_owned());
                }
            }
        }
        let data = ListData {
            kind,
            start,
            tight: true,
            marker_offset,
            padding,
            task,
        };
        self.close_unmatched_blocks();
        let matches = match self.kind(self.tip) {
            Kind::List(existing) => {
                continues_list(existing.kind, data.kind, data.start)
                    && existing.task.is_some() == data.task.is_some()
            }
            _ => false,
        };
        if !matches {
            self.add_child(Kind::List(data));
        }
        self.add_child(Kind::Item(data));
        if let Some(text) = task_text {
            let index = self.add_child(Kind::Paragraph);
            if let Some(node) = self.nodes.get_mut(index) {
                node.content = text;
                node.content.push('\n');
            }
            self.consumed = true;
            self.offset = self.line.len();
            return Start::Leaf;
        }
        Start::Container
    }

    fn start_definition(&mut self, container: usize) -> Start {
        if self.indented || !self.depth_ok() {
            return Start::None;
        }
        let marker = self.byte(self.next_nonspace);
        if marker != b':' && marker != b'~' {
            return Start::None;
        }
        let after = self.byte(self.next_nonspace + 1);
        if !(after == b' ' || after == b'\t' || self.next_nonspace + 1 >= self.line.len()) {
            return Start::None;
        }
        let container_kind = self.kind(container).clone();
        let term = match container_kind {
            Kind::DefItem { .. } => None,
            Kind::Paragraph => Some((container, true)),
            _ => match self.last_block(container) {
                Some(last) if matches!(self.kind(last), Kind::Paragraph) => Some((last, false)),
                _ => return Start::None,
            },
        };
        let init_col = self.column;
        self.close_unmatched_blocks();
        if let Some((para, tight)) = term {
            self.make_def_item(para, tight);
        }
        self.advance_next_nonspace();
        self.advance_offset(1, true);
        let spaces_col = self.column;
        let spaces_offset = self.offset;
        let mut count = 0;
        while count < 4 && is_space_or_tab(self.byte(self.offset)) {
            self.advance_offset(1, true);
            count += 1;
        }
        if is_space_or_tab(self.byte(self.offset)) || self.offset >= self.line.len() {
            self.column = spaces_col;
            self.offset = spaces_offset;
            if is_space_or_tab(self.byte(self.offset)) {
                self.advance_offset(1, true);
            } else {
                self.column += 1;
            }
        }
        let indent = self.column - init_col;
        self.add_child(Kind::Definition { indent });
        Start::Container
    }

    /// Turns a paragraph into a definition list item and puts it into a definition list.
    fn make_def_item(&mut self, para: usize, tight: bool) {
        let parent = self.parent(para);
        let siblings = self
            .nodes
            .get(parent)
            .map(|n| n.children.clone())
            .unwrap_or_default();
        let position = siblings.iter().position(|&c| c == para);
        let previous = position
            .and_then(|p| p.checked_sub(1))
            .and_then(|p| siblings.get(p).copied())
            .filter(|&p| matches!(self.kind(p), Kind::DefList));
        let list = match previous {
            Some(list) => {
                if let Some(node) = self.nodes.get_mut(parent) {
                    node.children.retain(|&c| c != para);
                }
                list
            }
            None => {
                let list = self.nodes.len();
                let depth = self.nodes.get(para).map_or(1, |n| n.depth);
                self.nodes.push(Node {
                    kind: Kind::DefList,
                    parent,
                    children: Vec::new(),
                    open: true,
                    last_line_blank: false,
                    start_line: self.line_number,
                    depth,
                    content: String::new(),
                    attr: Attr::default(),
                    block_attrs: None,
                    keep: false,
                    blank_line: 0,
                    blanks: Vec::new(),
                    blank_end: false,
                    blank_inside: false,
                });
                if let Some(node) = self.nodes.get_mut(parent)
                    && let Some(p) = position
                    && let Some(slot) = node.children.get_mut(p)
                {
                    *slot = list;
                }
                list
            }
        };
        if let Some(node) = self.nodes.get_mut(list) {
            node.open = true;
            node.children.push(para);
        }
        if let Some(node) = self.nodes.get_mut(para) {
            node.kind = Kind::DefItem { tight };
            node.parent = list;
            node.open = true;
            node.depth += 1;
        }
        self.tip = para;
    }

    fn start_indented_code(&mut self) -> Start {
        if !self.indented || matches!(self.kind(self.tip), Kind::Paragraph) || self.blank {
            return Start::None;
        }
        self.advance_offset(4, true);
        self.close_unmatched_blocks();
        self.add_child(Kind::CodeBlock {
            fenced: false,
            fence_char: 0,
            fence_len: 0,
            fence_offset: 0,
            info: String::new(),
        });
        Start::Leaf
    }

    /// Removes link reference definitions from the start of the text of the paragraph
    /// `index`. With `rebase_relative_paths`, a target gets the folder of the file
    /// that holds the definition.
    fn extract_refs(&mut self, index: usize, content: &str) -> String {
        let line = self.nodes.get(index).map_or(1, |n| n.start_line);
        let dir = self.config.source_dir(line);
        let mut rest = content;
        while rest.starts_with('[') {
            crate::steps::add(1);
            match parse_reference(rest, self.config.attributes) {
                Some((len, label, mut def)) => {
                    def.url = crate::rebase::rebase_path(dir, def.url);
                    self.refs.entry(label).or_insert(def);
                    rest = rest.get(len..).unwrap_or("");
                }
                None => break,
            }
        }
        rest.to_owned()
    }

    fn finalize(&mut self, index: usize) {
        let parent = self.parent(index);
        let (mut line, mut blank) = self
            .nodes
            .get(index)
            .map_or((0, false), |n| (n.blank_line, n.last_line_blank));
        // The newest value comes from the last child, because later lines go to later children.
        // Reading only that child keeps a block that opens again (a definition list) at constant cost.
        let last = self
            .nodes
            .get(index)
            .and_then(|n| n.children.last().copied());
        if let Some(child) = last.and_then(|c| self.nodes.get(c))
            && child.blank_line > line
        {
            line = child.blank_line;
            blank = child.last_line_blank;
        }
        if let Some(node) = self.nodes.get_mut(index) {
            node.blank_line = line;
            node.last_line_blank = blank;
        }
        if let Some(node) = self.nodes.get_mut(index) {
            node.open = false;
        }
        let kind = self.kind(index).clone();
        if matches!(kind, Kind::Div { .. })
            && let Some(position) = self
                .open_divs
                .iter()
                .rposition(|(node, _, _)| *node == index)
        {
            let above: Vec<(usize, usize)> = self
                .open_divs
                .drain(position..)
                .skip(1)
                .map(|(node, fence, _)| (node, fence))
                .collect();
            for (node, fence) in above {
                self.push_open_div(node, fence);
            }
        }
        match kind {
            Kind::Paragraph => {
                let content = self
                    .nodes
                    .get(index)
                    .map(|n| n.content.clone())
                    .unwrap_or_default();
                let content = self.extract_refs(index, &content);
                let empty = content.trim_matches([' ', '\t', '\n']).is_empty()
                    && !self.nodes.get(index).is_some_and(|n| n.keep);
                if let Some(node) = self.nodes.get_mut(index) {
                    node.content = content;
                }
                if empty
                    && let Some(p) = self.nodes.get_mut(parent)
                    && p.children.last() == Some(&index)
                {
                    p.children.pop();
                }
            }
            Kind::CodeBlock { fenced: false, .. } => {
                if let Some(node) = self.nodes.get_mut(index) {
                    let content = std::mem::take(&mut node.content);
                    let mut lines: Vec<&str> = content.split_terminator('\n').collect();
                    while lines.last().is_some_and(|l| l.bytes().all(|b| b == b' ')) {
                        lines.pop();
                    }
                    let mut text = lines.join("\n");
                    text.push('\n');
                    node.content = text;
                }
            }
            Kind::Item(_) => self.finalize_item_blanks(index),
            Kind::List(mut data) => {
                data.tight = self.finalize_list_blanks(index);
                if let Some(node) = self.nodes.get_mut(index) {
                    node.kind = Kind::List(data);
                }
            }
            Kind::FootnoteDef(_) => {
                if let Some(node) = self.nodes.get_mut(parent)
                    && node.children.last() == Some(&index)
                {
                    node.children.pop();
                }
                self.notes.push(index);
            }
            _ => {}
        }
        self.tip = parent;
    }

    /// The line of the parser position at a finalization: the end of the input is one line
    /// after the last line, as `sourceLine` of the end position in `Commonmark/Blocks.hs`.
    fn current_line(&self) -> usize {
        self.line_number + usize::from(self.at_end)
    }

    /// The blank line rule of the finalizer of `listItemSpec` in `Commonmark/Blocks.hs`.
    fn finalize_item_blanks(&mut self, item: usize) {
        let Some(node) = self.nodes.get(item) else {
            return;
        };
        let mut all = node.blanks.clone();
        for &child in &node.children {
            if let Some(list) = self.nodes.get(child)
                && matches!(list.kind, Kind::List(_))
            {
                all.extend_from_slice(&list.blanks);
            }
        }
        all.sort_unstable_by(|a, b| b.cmp(a));
        let at_end = all
            .first()
            .is_some_and(|&last| last + 1 >= self.current_line());
        // `removeConsecutive` joins only the run of lines at the end of the item.
        let mut groups = all.len();
        for pair in all.windows(2) {
            match pair {
                [later, earlier] if *later == *earlier + 1 => groups -= 1,
                _ => break,
            }
        }
        if let Some(node) = self.nodes.get_mut(item) {
            node.blank_end = at_end;
            node.blank_inside = groups > 1 || (groups == 1 && !at_end);
        }
    }

    /// The finalizer of `listSpec` in `Commonmark/Blocks.hs`. Returns true for a tight list.
    fn finalize_list_blanks(&mut self, list: usize) -> bool {
        let Some(node) = self.nodes.get(list) else {
            return true;
        };
        let items: Vec<(bool, bool)> = node
            .children
            .iter()
            .filter_map(|&item| self.nodes.get(item).map(|n| (n.blank_inside, n.blank_end)))
            .collect();
        let last_end = items.last().is_some_and(|&(_, end)| end);
        let loose = items.iter().any(|&(inside, _)| inside)
            || items
                .iter()
                .take(items.len().saturating_sub(1))
                .any(|&(_, end)| end);
        let line = self.current_line().saturating_sub(1);
        if last_end
            && let Some(node) = self.nodes.get_mut(list)
            && node.blanks.last() != Some(&line)
        {
            node.blanks.push(line);
        }
        !loose
    }
}

/// Parses an ordered list marker. Returns the style, the delimiter, the start number and the length.
fn ordered_marker(
    rest: &[u8],
    fancy: bool,
    parent: Option<ListKind>,
) -> Option<(NumStyle, u8, i64, usize)> {
    if !fancy {
        let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 || digits > 9 {
            return None;
        }
        let delim = match rest.get(digits) {
            Some(&d @ (b'.' | b')')) => d,
            _ => return None,
        };
        let number = parse_number(rest.get(..digits)?)?;
        return Some((NumStyle::Decimal, delim, number, digits + 1));
    }
    if let Some(ListKind::Ordered(style, delim)) = parent
        && let Some(found) = fancy_marker(rest, Some((style, delim)), true)
    {
        return Some(found);
    }
    fancy_marker(rest, None, parent.is_none())
}

fn parse_number(digits: &[u8]) -> Option<i64> {
    std::str::from_utf8(digits).ok()?.parse().ok()
}

fn fancy_marker(
    rest: &[u8],
    series: Option<(NumStyle, u8)>,
    roman_one: bool,
) -> Option<(NumStyle, u8, i64, usize)> {
    let paren = rest.first() == Some(&b'(');
    if series.is_some_and(|(_, d)| (d == b'(') != paren) {
        return None;
    }
    let start = usize::from(paren);
    let word_len = rest
        .get(start..)?
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric())
        .count();
    let word = rest.get(start..start + word_len)?;
    let end = start + word_len;
    let (style, number) = match series {
        Some((style, _)) => (style, number_in_style(word, style)?),
        None => {
            let candidates: &[NumStyle] = &[
                NumStyle::Decimal,
                NumStyle::LowerAlpha,
                NumStyle::UpperAlpha,
                NumStyle::LowerRoman,
                NumStyle::UpperRoman,
            ];
            if roman_one && word == b"i" {
                (NumStyle::LowerRoman, 1)
            } else if roman_one && word == b"I" {
                (NumStyle::UpperRoman, 1)
            } else {
                candidates
                    .iter()
                    .find_map(|&style| number_in_style(word, style).map(|n| (style, n)))?
            }
        }
    };
    let delim = match (paren, rest.get(end)) {
        (true, Some(b')')) => b'(',
        (false, Some(&d @ (b'.' | b')'))) => d,
        _ => return None,
    };
    if series.is_some_and(|(_, d)| d != delim) {
        return None;
    }
    if delim == b'.' && matches!(style, NumStyle::UpperAlpha | NumStyle::UpperRoman) {
        let after = rest.get(end + 1..).unwrap_or(&[]);
        let spaces = after.iter().take_while(|&&b| b == b' ').count();
        if !(after.is_empty() || spaces > 1 || after.first() == Some(&b'\t')) {
            return None;
        }
    }
    Some((style, delim, number, end + 1))
}

fn number_in_style(word: &[u8], style: NumStyle) -> Option<i64> {
    match style {
        NumStyle::Decimal => {
            (!word.is_empty() && word.len() < 10 && word.iter().all(u8::is_ascii_digit))
                .then(|| parse_number(word))
                .flatten()
        }
        NumStyle::LowerAlpha => match word {
            [c] if c.is_ascii_lowercase() => Some(i64::from(c - b'a') + 1),
            _ => None,
        },
        NumStyle::UpperAlpha => match word {
            [c] if c.is_ascii_uppercase() => Some(i64::from(c - b'A') + 1),
            _ => None,
        },
        NumStyle::LowerRoman => roman(word, false),
        NumStyle::UpperRoman => roman(word, true),
    }
}

/// Reads a Roman numeral in the way of the `commonmark-extensions` parser.
fn roman(word: &[u8], upper: bool) -> Option<i64> {
    if word.is_empty() || word.len() >= 10 {
        return None;
    }
    let lower: Vec<u8> = word.to_ascii_lowercase();
    if word.iter().any(|b| b.is_ascii_uppercase() != upper) {
        return None;
    }
    let s = lower.as_slice();
    let mut p = 0;
    let mut total = 0;
    let count = |p: &mut usize, c: u8| {
        let mut n = 0;
        while s.get(*p) == Some(&c) {
            *p += 1;
            n += 1;
        }
        n
    };
    let pair = |p: &mut usize, a: u8, b: u8| {
        if s.get(*p) == Some(&a) && s.get(*p + 1) == Some(&b) {
            *p += 2;
            true
        } else {
            false
        }
    };
    let one = |p: &mut usize, a: u8| {
        if s.get(*p) == Some(&a) {
            *p += 1;
            true
        } else {
            false
        }
    };
    total += 1000 * count(&mut p, b'm');
    if pair(&mut p, b'c', b'm') {
        total += 900;
    }
    if one(&mut p, b'd') {
        total += 500;
    }
    if pair(&mut p, b'c', b'd') {
        total += 400;
    }
    total += 100 * count(&mut p, b'c');
    if pair(&mut p, b'x', b'c') {
        total += 90;
    }
    if one(&mut p, b'l') {
        total += 50;
    }
    if pair(&mut p, b'x', b'l') {
        total += 40;
    }
    total += 10 * count(&mut p, b'x');
    if pair(&mut p, b'i', b'x') {
        total += 9;
    }
    if one(&mut p, b'v') {
        total += 5;
    }
    if pair(&mut p, b'i', b'v') {
        total += 4;
    }
    total += count(&mut p, b'i');
    (p == s.len() && total > 0).then_some(total)
}

/// Returns `true` when an item continues an open list, with the rule of the
/// `commonmark` library: the same delimiter, and the same number style, or a
/// letter style and a Roman style of the same case when the number of the new
/// item is the value of one Roman letter (1, 5, 10, 50, 100, 500, 1000).
fn continues_list(open: ListKind, new: ListKind, start: i64) -> bool {
    match (open, new) {
        (ListKind::Ordered(a, d1), ListKind::Ordered(b, d2)) => {
            use NumStyle::*;
            let mixed = matches!(
                (a, b),
                (LowerAlpha, LowerRoman)
                    | (LowerRoman, LowerAlpha)
                    | (UpperAlpha, UpperRoman)
                    | (UpperRoman, UpperAlpha)
            );
            d1 == d2 && (a == b || (mixed && matches!(start, 1 | 5 | 10 | 50 | 100 | 500 | 1000)))
        }
        _ => open == new,
    }
}

/// Returns the title of an alert type, for example `Note` for `NOTE`.
pub(crate) fn alert_title(name: &str) -> Option<&'static str> {
    match name.to_ascii_uppercase().as_str() {
        "NOTE" => Some("Note"),
        "TIP" => Some("Tip"),
        "IMPORTANT" => Some("Important"),
        "WARNING" => Some("Warning"),
        "CAUTION" => Some("Caution"),
        _ => None,
    }
}

fn accepts_lines(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::Paragraph | Kind::CodeBlock { .. } | Kind::HtmlBlock(_) | Kind::Table(_)
    )
}

fn maybe_special(b: u8) -> bool {
    matches!(
        b,
        b'#' | b'`'
            | b'~'
            | b'*'
            | b'+'
            | b'_'
            | b'='
            | b'<'
            | b'>'
            | b'-'
            | b':'
            | b'|'
            | b'['
            | b'('
            | b'{'
    ) || b.is_ascii_alphanumeric()
}

fn atx_level(rest: &str) -> Option<usize> {
    let level = rest.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    match rest.as_bytes().get(level) {
        None | Some(b' ' | b'\t') => Some(level),
        _ => None,
    }
}

fn strip_closing_hashes(content: &str) -> &str {
    let trimmed = content.trim_end_matches([' ', '\t']);
    let without = trimmed.trim_end_matches('#');
    if without.len() == trimmed.len() {
        return trimmed;
    }
    if without.is_empty() {
        return "";
    }
    if without.ends_with([' ', '\t']) {
        return without.trim_end_matches([' ', '\t']);
    }
    trimmed
}

/// Splits `{...}` attributes from the end of a heading.
fn trailing_attributes(content: &str) -> (&str, Option<attrs::Pairs>) {
    let trimmed = content.trim_end_matches([' ', '\t']);
    if !trimmed.ends_with('}') {
        return (content, None);
    }
    let mut search = trimmed.len();
    while let Some(open) = trimmed.get(..search).and_then(|t| t.rfind('{')) {
        // Attributes directly after a word belong to the word, not to the heading.
        let separated = open == 0
            || trimmed
                .get(..open)
                .is_some_and(|t| t.ends_with([' ', '\t']));
        if separated
            && let Some((end, pairs)) = attrs::parse_attributes(trimmed, open)
            && end == trimmed.len()
        {
            return (trimmed.get(..open).unwrap_or(""), Some(pairs));
        }
        search = open;
    }
    (content, None)
}

fn is_thematic_break(rest: &str) -> bool {
    let mut marker = None;
    let mut count = 0;
    for b in rest.bytes() {
        match b {
            b' ' | b'\t' => {}
            b'*' | b'-' | b'_' => {
                if marker.is_some_and(|m| m != b) {
                    return false;
                }
                marker = Some(b);
                count += 1;
            }
            _ => return false,
        }
    }
    count >= 3
}

const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

fn html_block_start(rest: &str) -> Option<u8> {
    let lower: String = rest
        .chars()
        .take(16)
        .collect::<String>()
        .to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for tag in ["script", "pre", "textarea", "style"] {
        if let Some(after) = lower.strip_prefix('<').and_then(|r| r.strip_prefix(tag))
            && matches!(after.as_bytes().first(), None | Some(b' ' | b'\t' | b'>'))
        {
            return Some(1);
        }
    }
    if lower.starts_with("<!--") {
        return Some(2);
    }
    if lower.starts_with("<?") {
        return Some(3);
    }
    if lower.starts_with("<!") && bytes.get(2).is_some_and(u8::is_ascii_alphabetic) {
        return Some(4);
    }
    if rest.starts_with("<![CDATA[") {
        return Some(5);
    }
    let name_start = if lower.starts_with("</") { 2 } else { 1 };
    let name_len = bytes.get(name_start..).map_or(0, |r| {
        r.iter().take_while(|b| b.is_ascii_alphanumeric()).count()
    });
    let name = lower.get(name_start..name_start + name_len).unwrap_or("");
    if BLOCK_TAGS.contains(&name) {
        let after = bytes.get(name_start + name_len..).unwrap_or(&[]);
        if matches!(after.first(), None | Some(b' ' | b'\t' | b'>')) || after.starts_with(b"/>") {
            return Some(6);
        }
    }
    let b = rest.as_bytes();
    let end = scan_open_tag(b, 0).or_else(|| scan_closing_tag(b, 0))?;
    if b.get(end..)
        .is_some_and(|r| r.iter().all(|&c| is_space_or_tab(c)))
    {
        return Some(7);
    }
    None
}

fn html_block_ends(kind: u8, line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    match kind {
        1 => ["</script>", "</pre>", "</textarea>", "</style>"]
            .iter()
            .any(|t| lower.contains(t)),
        2 => line.contains("-->"),
        3 => line.contains("?>"),
        4 => line.contains('>'),
        5 => line.contains("]]>"),
        _ => false,
    }
}

/// Parses a link reference definition at the start of `s`.
fn parse_reference(text: &str, attributes: bool) -> Option<(usize, String, LinkDef)> {
    let s = text.as_bytes();
    let label_end = scan_link_label(s, 0)?;
    if s.get(label_end) != Some(&b':') {
        return None;
    }
    let raw_label = String::from_utf8_lossy(s.get(1..label_end - 1)?).into_owned();
    let p = skip_spnl(s, label_end + 1);
    let (dest_end, dest) = scan_link_destination(s, p)?;
    if dest_end == p && s.get(p) != Some(&b'<') {
        return None;
    }
    let before_title = dest_end;
    let q = skip_spnl(s, before_title);
    let at_line_end = |pos: usize| -> Option<usize> {
        let mut p = pos;
        while s.get(p).is_some_and(|&b| is_space_or_tab(b)) {
            p += 1;
        }
        match s.get(p) {
            None => Some(p),
            Some(b'\n') => Some(p + 1),
            _ => None,
        }
    };
    let with_attrs = |pos: usize| -> Option<(usize, attrs::Pairs)> {
        if let Some(e) = at_line_end(pos) {
            return Some((e, Vec::new()));
        }
        if !attributes {
            return None;
        }
        let mut p = pos;
        while s.get(p).is_some_and(|&b| is_space_or_tab(b)) {
            p += 1;
        }
        let (attr_end, pairs) = attrs::parse_attributes(text, p)?;
        at_line_end(attr_end).map(|e| (e, pairs))
    };
    let mut title = String::new();
    let mut found = None;
    if q != before_title
        && let Some((title_end, raw)) = scan_link_title(s, q)
        && let Some(e) = with_attrs(title_end)
    {
        title = unescape(raw);
        found = Some(e);
    }
    let (end, pairs) = match found {
        Some(e) => e,
        None => with_attrs(before_title)?,
    };
    let label = normalize_label(&raw_label);
    if label.is_empty() {
        return None;
    }
    Some((
        end,
        label,
        LinkDef {
            url: unescape_destination(dest),
            title,
            attr: attrs::merge(pairs, Attr::default()),
        },
    ))
}

fn parse_dividers(line: &str) -> Option<Vec<Align>> {
    let trimmed = line.trim_matches([' ', '\t']);
    let has_lead = trimmed.starts_with('|');
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let has_trail = inner.ends_with('|') && !inner.is_empty();
    let inner = if has_trail {
        inner.get(..inner.len() - 1).unwrap_or("")
    } else {
        inner
    };
    let parts: Vec<&str> = inner.split('|').collect();
    if !has_lead && !has_trail && parts.len() < 2 {
        return None;
    }
    let mut aligns = Vec::new();
    for part in parts {
        let p = part.trim_matches([' ', '\t']);
        let left = p.starts_with(':');
        let right = p.ends_with(':') && p.len() > 1;
        let dashes = p.trim_start_matches(':').trim_end_matches(':');
        if dashes.is_empty() || !dashes.bytes().all(|b| b == b'-') {
            return None;
        }
        if p.len() > dashes.len() + usize::from(left) + usize::from(right) {
            return None;
        }
        aligns.push(match (left, right) {
            (true, true) => Align::Center,
            (true, false) => Align::Left,
            (false, true) => Align::Right,
            (false, false) => Align::Default,
        });
    }
    Some(aligns)
}

/// Splits a table row into cells. A `\|` stays in the cell as `|`.
fn parse_cells(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim_matches([' ', '\t']);
    if trimmed == "|" {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut pipes = 0;
    let mut i = 0;
    let mut start = 0;
    if bytes.first() == Some(&b'|') {
        pipes += 1;
        i = 1;
        start = 1;
    }
    let mut seg_start = start;
    let mut ended_with_pipe = false;
    while i < bytes.len() {
        ended_with_pipe = false;
        match bytes.get(i) {
            Some(b'\\') if bytes.get(i + 1) == Some(&b'|') => {
                current.push_str(trimmed.get(seg_start..i).unwrap_or(""));
                current.push('|');
                i += 2;
                seg_start = i;
            }
            Some(b'|') => {
                current.push_str(trimmed.get(seg_start..i).unwrap_or(""));
                cells.push(current.trim_matches([' ', '\t']).to_owned());
                current = String::new();
                pipes += 1;
                i += 1;
                seg_start = i;
                ended_with_pipe = true;
            }
            _ => i += 1,
        }
    }
    let tail = trimmed.get(seg_start..).unwrap_or("");
    current.push_str(tail);
    if !ended_with_pipe {
        cells.push(current.trim_matches([' ', '\t']).to_owned());
    }
    if pipes == 0 {
        return None;
    }
    Some(cells)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_rows() {
        assert_eq!(
            parse_cells("| a | b \\| c |"),
            Some(vec!["a".to_owned(), "b | c".to_owned()])
        );
        assert_eq!(
            parse_cells("a|b"),
            Some(vec!["a".to_owned(), "b".to_owned()])
        );
        assert_eq!(parse_cells("ab"), None);
        assert_eq!(
            parse_dividers("|:-|-:|:-:|-|"),
            Some(vec![
                Align::Left,
                Align::Right,
                Align::Center,
                Align::Default
            ])
        );
        assert_eq!(parse_dividers("--"), None);
        assert_eq!(parse_dividers("-|x"), None);
    }

    #[test]
    fn list_markers() {
        assert_eq!(roman(b"xiv", false), Some(14));
        assert_eq!(roman(b"MCMXC", true), Some(1990));
        assert_eq!(roman(b"iiv", false), None);
        assert_eq!(
            ordered_marker(b"(c) x", true, None),
            Some((NumStyle::LowerAlpha, b'(', 3, 3))
        );
        assert_eq!(
            ordered_marker(b"i. x", true, None),
            Some((NumStyle::LowerRoman, b'.', 1, 2))
        );
        assert_eq!(ordered_marker(b"B. x", true, None), None);
        assert_eq!(
            ordered_marker(b"B.  x", true, None),
            Some((NumStyle::UpperAlpha, b'.', 2, 2))
        );
        assert_eq!(ordered_marker(b"a. x", false, None), None);
        assert_eq!(
            ordered_marker(b"12) x", false, None),
            Some((NumStyle::Decimal, b')', 12, 3))
        );
    }

    #[test]
    fn small_scanners() {
        assert_eq!(strip_closing_hashes("a ##"), "a");
        assert_eq!(strip_closing_hashes("a#"), "a#");
        assert_eq!(strip_closing_hashes("##"), "");
        assert!(is_thematic_break(" - - -"));
        assert!(!is_thematic_break("-*-"));
        assert_eq!(html_block_start("<div>"), Some(6));
        assert_eq!(html_block_start("<a href=\"x\">"), Some(7));
        assert_eq!(html_block_start("<a href=\"x\"> b"), None);
        assert!(html_block_ends(2, "x -->"));
    }
}
