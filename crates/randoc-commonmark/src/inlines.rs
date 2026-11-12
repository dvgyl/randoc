//! The inline parser: code spans, links, emphasis and the inline extensions.
//!
//! The parser makes a list of nodes with a delimiter stack and a bracket
//! stack, as the CommonMark reference implementations do. A node is text,
//! a finished inline, or a run of delimiters.

use std::collections::HashMap;

use randoc_ast::{Attr, Block, Format, Inline, MathType, QuoteType, Target};

use crate::Config;
use crate::builder::Inlines;
use crate::scan::{
    self, HtmlMemo, is_ascii_punct, normalize_label, scan_email_autolink, scan_entity,
    scan_html_tag, scan_link_destination, scan_link_label, scan_link_title, scan_uri_autolink,
    skip_spnl, unescape, unescape_destination,
};
use randoc_core::shared::emoji_from_alias;

/// A link reference definition.
#[derive(Debug, Clone)]
pub(crate) struct LinkDef {
    pub(crate) url: String,
    pub(crate) title: String,
    pub(crate) attr: Attr,
}

/// What the inline parser needs from the document.
pub(crate) struct Env<'a> {
    pub(crate) config: &'a Config,
    pub(crate) refs: &'a HashMap<String, LinkDef>,
    /// The footnotes: label to converted blocks.
    pub(crate) notes: &'a HashMap<String, Vec<Block>>,
    /// The label of the footnote body that is parsed. In a body, a reference
    /// to this label stays text, and a reference to another note becomes an empty `Str`.
    pub(crate) current_note: Option<&'a str>,
    /// The levels of nesting that the inlines can still use.
    pub(crate) depth_budget: usize,
    /// For `rebase_relative_paths`: the folder of the file that holds the text.
    pub(crate) dir: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ctor {
    Emph,
    Strong,
    Strikeout,
    Subscript,
    Superscript,
    SingleQuoted,
    DoubleQuoted,
}

#[derive(Debug, Clone, Copy)]
struct DelimSpec {
    intraword: bool,
    ignore_punct: bool,
    single: Option<Ctor>,
    double: Option<Ctor>,
    unmatched: char,
}

fn delim_spec(config: &Config, ch: u8) -> Option<DelimSpec> {
    match ch {
        b'*' => Some(DelimSpec {
            intraword: true,
            ignore_punct: false,
            single: Some(Ctor::Emph),
            double: Some(Ctor::Strong),
            unmatched: '*',
        }),
        b'_' => Some(DelimSpec {
            intraword: false,
            ignore_punct: false,
            single: Some(Ctor::Emph),
            double: Some(Ctor::Strong),
            unmatched: '_',
        }),
        b'~' if config.strikeout || config.subscript => Some(DelimSpec {
            intraword: true,
            ignore_punct: true,
            single: config.subscript.then_some(Ctor::Subscript),
            double: config.strikeout.then_some(Ctor::Strikeout),
            unmatched: '~',
        }),
        b'^' if config.superscript => Some(DelimSpec {
            intraword: true,
            ignore_punct: true,
            single: Some(Ctor::Superscript),
            double: None,
            unmatched: '^',
        }),
        b'\'' if config.smart => Some(DelimSpec {
            intraword: false,
            ignore_punct: false,
            single: Some(Ctor::SingleQuoted),
            double: None,
            unmatched: '\u{2019}',
        }),
        b'"' if config.smart => Some(DelimSpec {
            intraword: false,
            ignore_punct: false,
            single: Some(Ctor::DoubleQuoted),
            double: None,
            unmatched: '\u{201C}',
        }),
        _ => None,
    }
}

#[derive(Debug)]
enum Kind {
    Text(String),
    /// An opening bracket. It never joins with the text after it.
    Bracket(&'static str),
    /// Text of one token, which attributes take as a whole. It never joins with other text.
    Token(String),
    Inline(Inline, usize),
    /// Several inlines, from a delimiter pair without a constructor.
    Many(Vec<Inline>, usize),
    /// Attributes for the node before this one.
    Attrs(crate::attrs::Pairs),
    Delim(Run),
    Removed,
}

#[derive(Debug)]
struct Node {
    kind: Kind,
    prev: Option<usize>,
    next: Option<usize>,
    /// The place of the node in the source order. An element has the place of its opener,
    /// as `chunkPos` in `Commonmark/Inlines.hs`.
    pos: usize,
}

/// A run of one delimiter character (`Delim` in `Commonmark/Inlines.hs`).
#[derive(Debug, Clone, Copy)]
struct Run {
    ch: u8,
    /// The characters that no element has used yet.
    count: usize,
    /// The length of the run in the source (`delimLength`).
    orig: usize,
    can_open: bool,
    can_close: bool,
    spec: DelimSpec,
}

#[derive(Debug, Clone)]
struct Bracket {
    node: usize,
    image: bool,
    bracket_after: bool,
    /// Byte position after the opening bracket.
    start: usize,
}

/// The kind of character next to a delimiter run, as the `commonmark` tokenizer classifies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Space,
    Word,
    Symbol,
}

/// Returns `true` for a letter, a number or a combining mark (the Unicode
/// categories L, N and M), as the `commonmark` tokenizer groups word characters.
pub(crate) fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || crate::marks::is_mark(c)
}

/// The trailing punctuation of `Commonmark/Extensions/Autolink.hs`.
fn is_link_trailing(c: char) -> bool {
    matches!(
        c,
        '!' | '"' | '\'' | ')' | '*' | ',' | '.' | ':' | ';' | '?' | '_' | '~' | '<'
    )
}

fn classify(c: Option<char>) -> CharClass {
    match c {
        None => CharClass::Space,
        Some(c) if c.is_whitespace() => CharClass::Space,
        Some(c) if is_word_char(c) => CharClass::Word,
        Some(_) => CharClass::Symbol,
    }
}

/// For each byte position, the end of the run of characters that can be in a
/// domain part, and in the local part of an email address.
struct Runs {
    domain_end: Vec<usize>,
    email_end: Vec<usize>,
}

impl Runs {
    fn new(text: &str) -> Self {
        let mut domain_end: Vec<usize> = (0..=text.len()).collect();
        let mut email_end = domain_end.clone();
        for (index, c) in text.char_indices().rev() {
            crate::steps::add(1);
            let next = index + c.len_utf8();
            let word = is_word_char(c);
            if word || c == '-' || c == '_' {
                let end = domain_end.get(next).copied().unwrap_or(next);
                if let Some(slot) = domain_end.get_mut(index) {
                    *slot = end;
                }
            }
            if word || matches!(c, '.' | '-' | '_' | '+') {
                let end = email_end.get(next).copied().unwrap_or(next);
                if let Some(slot) = email_end.get_mut(index) {
                    *slot = end;
                }
            }
        }
        Runs {
            domain_end,
            email_end,
        }
    }
}

struct Parser<'a, 'e> {
    s: &'a [u8],
    text: &'a str,
    pos: usize,
    env: &'e Env<'e>,
    nodes: Vec<Node>,
    head: Option<usize>,
    tail: Option<usize>,
    brackets: Vec<Bracket>,
    memo: HtmlMemo,
    /// The run tables for bare autolinks, made at the first use.
    runs: Option<Runs>,
    /// The last search for `]`: where it started and what it found.
    close_memo: Option<(usize, Option<usize>)>,
    /// The brackets below this index are no link openers any more, because a link was made.
    link_floor: usize,
    /// For a position after a domain part: the end of the domain and the number of parts after it.
    domain_chain: HashMap<usize, (usize, usize)>,
    /// The nesting limit stopped an element. Later brackets and delimiters stay text.
    exhausted: bool,
    /// The position after the last bracket pair or link suffix that `processBs` in
    /// `Commonmark/Inlines.hs` handles as a pair. A single quote there cannot open
    /// (`fixSingleQuote`). Link attributes are a separate chunk upstream, so they end no suffix.
    bracket_end: Option<usize>,
    /// The absolute bottom of `processBs`: the position of the first bracket opener, and
    /// after a pair that makes nothing and has no chunk between this position and its
    /// opener, the position after its closer. Openers before it never pair.
    bracket_bottom: Option<usize>,
    /// For each backtick run length, the start positions of runs, in order.
    backtick_runs: Option<HashMap<usize, Vec<usize>>>,
}

/// Parses inline content. Leading and trailing white space is removed first.
pub(crate) fn parse_inlines(text: &str, env: &Env<'_>) -> Vec<Inline> {
    let trimmed = text.trim_matches(|c| c == ' ' || c == '\t' || c == '\n' || c == '\r');
    let mut parser = Parser {
        s: trimmed.as_bytes(),
        text: trimmed,
        pos: 0,
        env,
        nodes: Vec::new(),
        head: None,
        tail: None,
        brackets: Vec::new(),
        memo: HtmlMemo::default(),
        runs: None,
        close_memo: None,
        link_floor: 0,
        domain_chain: HashMap::new(),
        exhausted: false,
        bracket_end: None,
        bracket_bottom: None,
        backtick_runs: None,
    };
    parser.run();
    parser.process_emphasis(None);
    let head = parser.head;
    parser.emit_seq(head, None, false).0
}

/// Removes the backticks of `tex_math_gfm` math: the same number of backticks at both ends.
fn strip_math_ticks(content: String) -> String {
    let lead = content.bytes().take_while(|&b| b == b'`').count();
    let trail = content.bytes().rev().take_while(|&b| b == b'`').count();
    if lead > 0 && lead == trail && content.len() > 2 * lead {
        content
            .get(lead..content.len() - lead)
            .map_or(content.clone(), str::to_owned)
    } else {
        content
    }
}

/// Adds attributes to an inline as `commonmark-pandoc` does. An inline
/// without attributes goes into a span with the attribute `wrapper="1"`.
fn add_inline_attrs(pairs: &crate::attrs::Pairs, inline: Inline) -> Inline {
    use crate::attrs::merge;
    match inline {
        Inline::Link(a, c, t) => Inline::Link(merge(pairs.clone(), a), c, t),
        Inline::Image(a, c, t) => Inline::Image(merge(pairs.clone(), a), c, t),
        Inline::Span(a, c) => Inline::Span(merge(pairs.clone(), a), c),
        Inline::Code(a, t) => Inline::Code(merge(pairs.clone(), a), t),
        other => {
            let mut all = vec![("wrapper".to_owned(), "1".to_owned())];
            all.extend(pairs.iter().cloned());
            Inline::Span(merge(all, Attr::default()), vec![other])
        }
    }
}

impl<'a, 'e> Parser<'a, 'e> {
    fn config(&self) -> &Config {
        self.env.config
    }

    fn peek(&self) -> u8 {
        self.s.get(self.pos).copied().unwrap_or(0)
    }

    fn byte(&self, pos: usize) -> u8 {
        self.s.get(pos).copied().unwrap_or(0)
    }

    fn slice(&self, from: usize, to: usize) -> &'a str {
        self.text.get(from..to).unwrap_or("")
    }

    fn char_before(&self, pos: usize) -> Option<char> {
        self.text.get(..pos).and_then(|t| t.chars().next_back())
    }

    fn char_at(&self, pos: usize) -> Option<char> {
        self.text.get(pos..).and_then(|t| t.chars().next())
    }

    fn append(&mut self, kind: Kind) -> usize {
        let index = self.nodes.len();
        self.nodes.push(Node {
            kind,
            prev: self.tail,
            next: None,
            pos: index,
        });
        if let Some(tail) = self.tail
            && let Some(node) = self.nodes.get_mut(tail)
        {
            node.next = Some(index);
        }
        if self.head.is_none() {
            self.head = Some(index);
        }
        self.tail = Some(index);
        index
    }

    fn append_text(&mut self, text: &str) {
        if let Some(tail) = self.tail
            && let Some(Node {
                kind: Kind::Text(existing),
                ..
            }) = self.nodes.get_mut(tail)
        {
            existing.push_str(text);
            return;
        }
        self.append(Kind::Text(text.to_owned()));
    }

    fn append_inline(&mut self, inline: Inline) {
        self.append(Kind::Inline(inline, 1));
    }

    fn unlink(&mut self, index: usize) {
        let (prev, next) = match self.nodes.get(index) {
            Some(node) => (node.prev, node.next),
            None => return,
        };
        match prev {
            Some(p) => {
                if let Some(node) = self.nodes.get_mut(p) {
                    node.next = next;
                }
            }
            None => self.head = next,
        }
        match next {
            Some(n) => {
                if let Some(node) = self.nodes.get_mut(n) {
                    node.prev = prev;
                }
            }
            None => self.tail = prev,
        }
        if let Some(node) = self.nodes.get_mut(index) {
            node.kind = Kind::Removed;
            node.prev = None;
            node.next = None;
        }
    }

    fn insert_after(&mut self, after: usize, kind: Kind) -> usize {
        let index = self.nodes.len();
        let (next, pos) = self
            .nodes
            .get(after)
            .map_or((None, index), |n| (n.next, n.pos));
        self.nodes.push(Node {
            kind,
            prev: Some(after),
            next,
            pos,
        });
        if let Some(node) = self.nodes.get_mut(after) {
            node.next = Some(index);
        }
        match next {
            Some(n) => {
                if let Some(node) = self.nodes.get_mut(n) {
                    node.prev = Some(index);
                }
            }
            None => self.tail = Some(index),
        }
        index
    }

    /// Takes the nodes strictly after `from` and before `until` out of the list.
    fn take_between(&mut self, from: usize, until: Option<usize>) -> (Vec<Inline>, usize) {
        let first = self.nodes.get(from).and_then(|n| n.next);
        self.emit_seq(first, until, true)
    }

    /// Emits the nodes from `first` up to `until`, and applies attribute nodes
    /// to the node before them. Returns the inlines and their nesting depth.
    fn emit_seq(
        &mut self,
        first: Option<usize>,
        until: Option<usize>,
        unlink: bool,
    ) -> (Vec<Inline>, usize) {
        let mut out = Inlines::new();
        let mut depth = 0;
        let mut cursor = first;
        while let Some(index) = cursor {
            crate::steps::add(1);
            if Some(index) == until {
                break;
            }
            cursor = self.nodes.get(index).and_then(|n| n.next);
            depth = depth.max(self.node_depth(index));
            let attrs = match cursor.and_then(|c| self.nodes.get(c)) {
                Some(Node {
                    kind: Kind::Attrs(pairs),
                    ..
                }) if cursor != until => Some(pairs.clone()),
                _ => None,
            };
            let bang = matches!(
                self.nodes.get(index).map(|n| &n.kind),
                Some(Kind::Bracket("!["))
            );
            match attrs {
                // An image opener that stays text is the token `!` and the token `[`.
                Some(pairs) if bang => {
                    if let Some(node) = self.nodes.get_mut(index) {
                        node.kind = Kind::Removed;
                    }
                    out.text("!");
                    out.push(add_inline_attrs(&pairs, Inline::Str("[".into())));
                }
                Some(pairs) => {
                    let mut one = Inlines::new();
                    self.emit(index, &mut one);
                    for inline in one.into_vec() {
                        out.push(add_inline_attrs(&pairs, inline));
                    }
                }
                None => self.emit(index, &mut out),
            }
            if unlink {
                self.unlink(index);
            }
        }
        (out.into_vec(), depth)
    }

    /// Splits the last token of a text node into its own node, so that attributes apply to it only.
    fn split_last_token(&mut self) {
        let Some(tail) = self.tail else { return };
        let Some(Node {
            kind: Kind::Text(text),
            ..
        }) = self.nodes.get_mut(tail)
        else {
            return;
        };
        let Some(last) = text.chars().next_back() else {
            return;
        };
        let split = if last == ' ' {
            text.trim_end_matches(' ').len()
        } else if is_word_char(last) {
            text.trim_end_matches(is_word_char).len()
        } else {
            text.len() - last.len_utf8()
        };
        if split == 0 {
            return;
        }
        let suffix = text.split_off(split);
        self.append(Kind::Text(suffix));
    }

    fn emit(&mut self, index: usize, out: &mut Inlines) {
        let Some(node) = self.nodes.get_mut(index) else {
            return;
        };
        match std::mem::replace(&mut node.kind, Kind::Removed) {
            Kind::Text(text) => out.text(&text),
            Kind::Bracket(text) => out.text(text),
            Kind::Token(text) => out.text(&text),
            Kind::Inline(inline, _) => out.push(inline),
            Kind::Many(inlines, _) => out.extend(inlines),
            Kind::Attrs(_) => {}
            Kind::Delim(run) => {
                let text: String = std::iter::repeat_n(run.spec.unmatched, run.count).collect();
                out.text(&text);
            }
            Kind::Removed => {}
        }
    }

    fn node_depth(&self, index: usize) -> usize {
        match self.nodes.get(index) {
            Some(Node {
                kind: Kind::Inline(_, d) | Kind::Many(_, d),
                ..
            }) => *d,
            _ => 0,
        }
    }

    fn run(&mut self) {
        while self.pos < self.s.len() {
            crate::steps::add(1);
            if !self.parse_one() {
                let start = self.pos;
                self.pos += self.char_at(self.pos).map_or(1, char::len_utf8);
                while self.pos < self.s.len() && !self.is_special(self.pos) {
                    crate::steps::add(1);
                    self.pos += self.char_at(self.pos).map_or(1, char::len_utf8);
                }
                let text = self.slice(start, self.pos);
                self.append_text(text);
            }
        }
    }

    fn is_special(&self, pos: usize) -> bool {
        let c = self.config();
        let b = self.byte(pos);
        match b {
            b'\n' | b'`' | b'[' | b']' | b'\\' | b'!' | b'<' | b'&' | b'*' | b'_' | b' ' => true,
            b'{' if c.attributes => true,
            b'~' if c.strikeout || c.subscript => true,
            b'^' if c.superscript => true,
            b'\'' | b'"' | b'.' | b'-' if c.smart => true,
            b'$' if c.math_dollars || c.math_gfm => true,
            b':' if c.emoji => true,
            _ if c.autolink => {
                let here = self.char_at(pos);
                matches!(here, Some('.' | '-' | '+'))
                    || (here.is_some_and(is_word_char)
                        && !self.char_before(pos).is_some_and(is_word_char))
            }
            _ => false,
        }
    }

    /// Tries the parser for the character at the position. Returns `false` when
    /// the character is plain text.
    fn parse_one(&mut self) -> bool {
        let b = self.peek();
        match b {
            b'\n' => {
                self.parse_newline();
                true
            }
            b' ' => {
                let start = self.pos;
                while self.peek() == b' ' {
                    self.pos += 1;
                }
                if self.peek() == b'\n' {
                    let hard = self.pos - start >= 2;
                    self.pos += 1;
                    self.append_inline(if hard || self.config().hard_line_breaks {
                        Inline::LineBreak
                    } else {
                        Inline::SoftBreak
                    });
                } else {
                    self.append_text(self.slice(start, self.pos));
                }
                true
            }
            b'\\' => {
                self.parse_backslash();
                true
            }
            b'`' => {
                self.parse_backticks();
                true
            }
            b'[' => self.parse_open_bracket(),
            b']' => {
                self.close_bracket();
                true
            }
            b'!' => {
                let wiki = self.config().wiki_after || self.config().wiki_before;
                if self.byte(self.pos + 1) == b'['
                    && !(wiki && self.byte(self.pos + 2) == b'[')
                    && !(self.config().footnotes && self.note_label_at(self.pos + 1).is_some())
                {
                    let node = self.append(Kind::Bracket("!["));
                    self.push_bracket(node, true, self.pos + 2);
                    self.pos += 2;
                } else {
                    self.pos += 1;
                    self.append_text("!");
                }
                true
            }
            b'<' => {
                self.parse_lt();
                true
            }
            b'&' => {
                match scan_entity(self.s, self.pos) {
                    Some((end, text)) => {
                        self.pos = end;
                        // An entity is one token for attributes.
                        self.append(Kind::Token(text));
                    }
                    None => {
                        self.pos += 1;
                        self.append_text("&");
                    }
                }
                true
            }
            b'_' if self.config().autolink && self.parse_bare_autolink() => true,
            b'*' | b'_' | b'~' | b'^' | b'\'' | b'"' => self.parse_delims(b),
            b'.' if self.config().smart => {
                if self.s.get(self.pos..self.pos + 3) == Some(b"...") {
                    self.pos += 3;
                    self.append_text("\u{2026}");
                    true
                } else {
                    false
                }
            }
            b'-' if self.config().smart => self.parse_dashes(),
            b'$' => self.parse_math(),
            b':' if self.config().emoji => self.parse_emoji() || self.parse_bare_autolink(),
            b'{' if self.config().attributes => {
                let Some((end, pairs)) = crate::attrs::parse_attributes(self.text, self.pos) else {
                    return false;
                };
                self.pos = end;
                self.split_last_token();
                if self.tail.is_some() {
                    self.append(Kind::Attrs(pairs));
                }
                true
            }
            _ if self.config().autolink => self.parse_bare_autolink(),
            _ => false,
        }
    }

    fn parse_newline(&mut self) {
        self.pos += 1;
        let hard = self.config().hard_line_breaks;
        self.append_inline(if hard {
            Inline::LineBreak
        } else {
            Inline::SoftBreak
        });
    }

    fn parse_backslash(&mut self) {
        let next = self.byte(self.pos + 1);
        if next == b'\n' {
            self.pos += 2;
            self.append_inline(Inline::LineBreak);
        } else if is_ascii_punct(next) {
            self.pos += 2;
            self.append_inline(Inline::Str((next as char).to_string()));
        } else {
            self.pos += 1;
            self.append_text("\\");
        }
    }

    fn backtick_closer(&mut self, len: usize, after: usize) -> Option<usize> {
        if self.backtick_runs.is_none() {
            let mut runs: HashMap<usize, Vec<usize>> = HashMap::new();
            let mut p = 0;
            while p < self.s.len() {
                crate::steps::add(1);
                if self.byte(p) == b'`' {
                    let start = p;
                    while self.byte(p) == b'`' {
                        p += 1;
                    }
                    let escaped = start > 0 && self.byte(start - 1) == b'\\';
                    let _ = escaped;
                    runs.entry(p - start).or_default().push(start);
                } else {
                    p += 1;
                }
            }
            self.backtick_runs = Some(runs);
        }
        let runs = self.backtick_runs.as_ref()?.get(&len)?;
        let index = runs.partition_point(|&start| start < after);
        runs.get(index).copied()
    }

    fn parse_backticks(&mut self) {
        let start = self.pos;
        while self.peek() == b'`' {
            self.pos += 1;
        }
        let len = self.pos - start;
        match self.backtick_closer(len, self.pos) {
            Some(close) => {
                let raw = self.slice(self.pos, close);
                self.pos = close + len;
                if self.config().raw_attribute
                    && let Some((end, format)) = self.scan_raw_attribute(self.pos)
                {
                    self.pos = end;
                    let raw = self.config().raw_text(&format, raw.to_owned());
                    self.append_inline(Inline::RawInline(Format::new(format), raw));
                    return;
                }
                let code = normalize_code(raw);
                self.append_inline(Inline::Code(Attr::default(), code));
            }
            None => {
                let ticks = self.slice(start, self.pos).to_owned();
                self.append(Kind::Token(ticks));
            }
        }
    }

    fn scan_raw_attribute(&self, pos: usize) -> Option<(usize, String)> {
        if self.byte(pos) != b'{' {
            return None;
        }
        let mut p = pos + 1;
        while scan::is_space_or_tab(self.byte(p)) || self.byte(p) == b'\n' {
            p += 1;
        }
        if self.byte(p) != b'=' {
            return None;
        }
        p += 1;
        let start = p;
        while self.char_at(p).is_some_and(is_word_char) {
            p += self.char_at(p).map_or(1, char::len_utf8);
        }
        if p == start {
            return None;
        }
        let format = self.slice(start, p).to_owned();
        while scan::is_space_or_tab(self.byte(p)) || self.byte(p) == b'\n' {
            p += 1;
        }
        (self.byte(p) == b'}').then_some((p + 1, format))
    }

    fn parse_lt(&mut self) {
        let start = self.pos;
        if let Some(end) = scan_uri_autolink(self.s, start + 1) {
            let url = self.slice(start + 1, end).to_owned();
            self.pos = end + 1;
            let text = crate::builder::text(&url);
            self.append_inline(Inline::Link(Attr::default(), text, Target::new(url, "")));
            return;
        }
        if let Some(end) = scan_email_autolink(self.s, start + 1) {
            let email = self.slice(start + 1, end).to_owned();
            self.pos = end + 1;
            let text = crate::builder::text(&email);
            self.append_inline(Inline::Link(
                Attr::default(),
                text,
                Target::new(format!("mailto:{email}"), ""),
            ));
            return;
        }
        if let Some(end) = scan_html_tag(self.s, start, &mut self.memo) {
            let raw = self
                .config()
                .raw_text("html", self.slice(start, end).to_owned());
            self.pos = end;
            self.append_inline(Inline::RawInline(Format::new("html"), raw));
            return;
        }
        self.pos += 1;
        self.append_text("<");
    }

    fn parse_delims(&mut self, ch: u8) -> bool {
        let Some(spec) = delim_spec(self.config(), ch) else {
            return false;
        };
        let start = self.pos;
        while self.peek() == ch {
            self.pos += 1;
        }
        let count = self.pos - start;
        let before = classify(self.char_before(start));
        let after = classify(self.char_at(self.pos));
        let preceded_ws = before == CharClass::Space;
        let followed_ws = after == CharClass::Space;
        let preceded_punct = !spec.ignore_punct && before == CharClass::Symbol;
        let followed_punct = !spec.ignore_punct && !followed_ws && after != CharClass::Word;
        let left = !followed_ws && (!followed_punct || preceded_ws || preceded_punct);
        let right = !preceded_ws && (!preceded_punct || followed_ws || followed_punct);
        let can_open = left
            && (spec.intraword || !right || preceded_punct)
            && !(ch == b'\'' && self.bracket_end == Some(start));
        let can_close = right && (spec.intraword || !left || followed_punct);
        self.append(Kind::Delim(Run {
            ch,
            count,
            orig: count,
            can_open,
            can_close,
            spec,
        }));
        true
    }

    fn parse_dashes(&mut self) -> bool {
        let start = self.pos;
        while self.peek() == b'-' {
            self.pos += 1;
        }
        let n = self.pos - start;
        if n < 2 {
            self.pos = start;
            return false;
        }
        let (em, en) = if n.is_multiple_of(3) {
            (n / 3, 0)
        } else if n.is_multiple_of(2) {
            (0, n / 2)
        } else if n % 3 == 2 {
            ((n - 2) / 3, 1)
        } else {
            ((n - 4) / 3, 2)
        };
        let mut text = "\u{2014}".repeat(em);
        text.push_str(&"\u{2013}".repeat(en));
        self.append_text(&text);
        true
    }

    fn parse_math(&mut self) -> bool {
        if !self.config().math_dollars {
            return false;
        }
        let display = self.byte(self.pos + 1) == b'$';
        let mut p = self.pos + if display { 2 } else { 1 };
        if !display && classify(self.char_at(p)) == CharClass::Space {
            return false;
        }
        let content_start = p;
        let mut depth = 0usize;
        let mut steps = 0usize;
        let content_end;
        loop {
            steps += 1;
            if steps > 1000 && depth > 1000 {
                return false;
            }
            match self.s.get(p) {
                None => return false,
                Some(b'$') if depth == 0 => {
                    if display {
                        if self.byte(p + 1) == b'$' {
                            content_end = p;
                            p += 2;
                            break;
                        }
                        p += 1;
                    } else {
                        content_end = p;
                        p += 1;
                        break;
                    }
                }
                Some(b'\\') => p += 1 + self.char_at(p + 1).map_or(0, char::len_utf8),
                Some(b'{') => {
                    depth += 1;
                    if depth > 1000 {
                        return false;
                    }
                    p += 1;
                }
                Some(b'}') => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                    p += 1;
                }
                Some(_) => p += 1,
            }
        }
        let content = self.slice(content_start, content_end).to_owned();
        if display {
            self.pos = p;
            self.append_inline(Inline::Math(MathType::DisplayMath, content));
            return true;
        }
        if content.is_empty()
            || content.ends_with([' ', '\t', '\n', '\r'])
            || self.char_at(p).is_some_and(|c| c.is_ascii_digit())
        {
            return false;
        }
        self.pos = p;
        let content = if self.config().math_gfm {
            strip_math_ticks(content)
        } else {
            content
        };
        self.append_inline(Inline::Math(MathType::InlineMath, content));
        true
    }

    fn parse_emoji(&mut self) -> bool {
        let start = self.pos + 1;
        let mut p = start;
        while let Some(c) = self.char_at(p) {
            crate::steps::add(1);
            if is_word_char(c) || matches!(c, '_' | '+' | '-') {
                p += c.len_utf8();
            } else {
                break;
            }
        }
        if p == start || self.byte(p) != b':' {
            return false;
        }
        let alias = self.slice(start, p);
        let Some(emoji) = emoji_from_alias(alias) else {
            return false;
        };
        self.pos = p + 1;
        self.append_inline(Inline::Span(
            Attr::new(
                "",
                vec!["emoji".into()],
                vec![("data-emoji".into(), alias.into())],
            ),
            crate::builder::text(emoji),
        ));
        true
    }

    fn parse_bare_autolink(&mut self) -> bool {
        if !self.config().autolink {
            return false;
        }
        let Some(first) = self.char_at(self.pos) else {
            return false;
        };
        if !(is_word_char(first) || matches!(first, '.' | '-' | '_' | '+'))
            || (is_word_char(first) && self.char_before(self.pos).is_some_and(is_word_char))
        {
            return false;
        }
        let preceded_ok = match self.char_before(self.pos) {
            None => true,
            Some(c) if c.is_whitespace() => true,
            Some(c) if is_word_char(c) => false,
            Some(c) => matches!(c, '(' | '*' | '_' | '~' | '['),
        };
        let start = self.pos;
        let mut prefix = "";
        let mut end = None;
        if preceded_ok {
            let word_end = self.word_end(start);
            let word = self.slice(start, word_end);
            if word == "www" {
                if let Some(p) = self.valid_domain(start) {
                    end = Some(self.link_path(p));
                    prefix = "http://";
                }
            } else if matches!(word, "http" | "https" | "ftp")
                && self.s.get(word_end..word_end + 3) == Some(b"://")
                && let Some(p) = self.valid_domain(word_end + 3)
            {
                end = Some(self.link_path(p));
            }
        }
        if end.is_none() {
            let p = self.runs().email_end.get(start).copied().unwrap_or(start);
            if p > start
                && self.byte(p) == b'@'
                && let Some(domain_end) = self.valid_domain(p + 1)
            {
                end = Some(domain_end);
                prefix = "mailto:";
            }
        }
        let Some(end) = end else {
            return false;
        };
        let text = self.slice(start, end).to_owned();
        self.pos = end;
        self.append_inline(Inline::Link(
            Attr::default(),
            crate::builder::text(&text),
            Target::new(format!("{prefix}{text}"), ""),
        ));
        true
    }

    fn word_end(&self, mut p: usize) -> usize {
        while let Some(c) = self.char_at(p) {
            crate::steps::add(1);
            if is_word_char(c) {
                p += c.len_utf8();
            } else {
                break;
            }
        }
        p
    }

    /// The run tables of the text, made at the first use.
    fn runs(&mut self) -> &Runs {
        let text = self.text;
        self.runs.get_or_insert_with(|| Runs::new(text))
    }

    /// Returns the end of one domain part: a run of word characters, `-` and
    /// `_` that ends with a word character.
    fn domain_part(&mut self, start: usize) -> Option<usize> {
        let end = self.runs().domain_end.get(start).copied().unwrap_or(start);
        (end > start && self.char_before(end).is_some_and(is_word_char)).then_some(end)
    }

    /// Returns the end of a domain that starts at `start`: parts joined by dots,
    /// at least two. The result after each part is kept, so that each part is read once.
    fn valid_domain(&mut self, start: usize) -> Option<usize> {
        let first = self.domain_part(start)?;
        let mut path = Vec::new();
        let mut p = first;
        let (mut end, mut parts) = loop {
            crate::steps::add(1);
            if let Some(&known) = self.domain_chain.get(&p) {
                break known;
            }
            if self.byte(p) == b'.'
                && let Some(next) = self.domain_part(p + 1)
            {
                path.push(p);
                p = next;
            } else {
                break (p, 0);
            }
        };
        self.domain_chain.insert(p, (end, parts));
        while let Some(position) = path.pop() {
            parts += 1;
            self.domain_chain.insert(position, (end, parts));
        }
        if parts == 0 {
            end = first;
        }
        (parts > 0).then_some(end)
    }

    /// Returns the end of the path of a bare autolink, as `linkPath` of
    /// `Commonmark/Extensions/Autolink.hs` reads it. Spaces, line ends, `<`
    /// and an unmatched `)` or `]` end the path. A run of trailing punctuation
    /// (which can hold `)` and `<`) stays in the path only when the path goes
    /// on after the run. `&` ends the path when an entity and the end of the
    /// link follow it.
    fn link_path(&self, start: usize) -> usize {
        let mut p = start;
        let (mut parens, mut brackets) = (0usize, 0usize);
        loop {
            crate::steps::add(1);
            let Some(c) = self.char_at(p) else {
                return p;
            };
            match c {
                '&' if self.entity_ends_link(p) => return p,
                '(' => parens += 1,
                ')' if parens > 0 => parens -= 1,
                '[' => brackets += 1,
                ']' if brackets > 0 => brackets -= 1,
                ')' | ']' | '<' | ' ' | '\t' | '\n' | '\r' => return p,
                c if is_link_trailing(c) => {
                    let run_end = self.trailing_run_end(p);
                    let goes_on = match self.char_at(run_end) {
                        None | Some(' ' | '\t' | '\n' | '\r') => false,
                        Some('&') => !self.entity_ends_link(run_end),
                        Some(']') => brackets > 0,
                        Some(_) => true,
                    };
                    if !goes_on {
                        return p;
                    }
                    p = run_end;
                    continue;
                }
                _ => {}
            }
            p += c.len_utf8();
        }
    }

    /// Returns the end of the run of trailing punctuation at `p`.
    fn trailing_run_end(&self, mut p: usize) -> usize {
        while self.char_at(p).is_some_and(is_link_trailing) {
            crate::steps::add(1);
            p += 1;
        }
        p
    }

    /// Returns `true` when the `&` at `p` starts a word, `;`, trailing
    /// punctuation and then white space or the end of the text.
    fn entity_ends_link(&self, p: usize) -> bool {
        let word_end = self.word_end(p + 1);
        if word_end == p + 1 || self.byte(word_end) != b';' {
            return false;
        }
        let end = self.trailing_run_end(word_end + 1);
        matches!(self.char_at(end), None | Some(' ' | '\t' | '\n' | '\r'))
    }

    fn pop_bracket(&mut self) {
        self.brackets.pop();
        self.link_floor = self.link_floor.min(self.brackets.len());
    }

    fn push_bracket(&mut self, node: usize, image: bool, start: usize) {
        self.bracket_bottom.get_or_insert(start.saturating_sub(1));
        if let Some(last) = self.brackets.last_mut() {
            last.bracket_after = true;
        }
        self.brackets.push(Bracket {
            node,
            image,
            bracket_after: false,
            start,
        });
    }

    fn parse_open_bracket(&mut self) -> bool {
        let wiki = self.config().wiki_after || self.config().wiki_before;
        if wiki && self.parse_wikilink() {
            return true;
        }
        if self.config().footnotes && self.byte(self.pos + 1) == b'^' && self.parse_note_ref() {
            return true;
        }
        let node = self.append(Kind::Bracket("["));
        self.push_bracket(node, false, self.pos + 1);
        self.pos += 1;
        true
    }

    /// Returns the position of the next `]` at or after `from`. The last search is
    /// kept, so that many open wikilinks read the text once.
    fn next_close_bracket(&mut self, from: usize) -> Option<usize> {
        if let Some((searched, found)) = self.close_memo
            && searched <= from
            && found.is_none_or(|f| f >= from)
        {
            return found;
        }
        let found = self
            .s
            .get(from..)
            .and_then(|r| r.iter().position(|&b| b == b']'))
            .map(|i| from + i);
        crate::steps::add(found.map_or(self.s.len(), |f| f + 1).saturating_sub(from) as u64);
        self.close_memo = Some((from, found));
        found
    }

    fn parse_wikilink(&mut self) -> bool {
        if self.s.get(self.pos..self.pos + 2) != Some(b"[[") || self.byte(self.pos + 2) == b'[' {
            return false;
        }
        let start = self.pos + 2;
        let Some(close) = self.next_close_bracket(start) else {
            return false;
        };
        if self.byte(close + 1) != b']' {
            return false;
        }
        let inner = self.slice(start, close);
        let (title, url) = match inner.split_once('|') {
            None => (inner, inner),
            Some((a, b)) if self.config().wiki_before => (a, b),
            Some((a, b)) => (b, a),
        };
        let title = unescape(title.as_bytes());
        let url = unescape(url.as_bytes());
        let trim = |s: &str| s.trim_matches([' ', '\t', '\n', '\r']).to_owned();
        self.pos = close + 2;
        self.append_inline(Inline::Link(
            Attr::new("", vec!["wikilink".into()], Vec::new()),
            crate::builder::text(&trim(&title)),
            Target::new(trim(&url), ""),
        ));
        true
    }

    /// Returns the end and the label of a footnote reference `[^label]` at `pos`
    /// when the label has a definition.
    fn note_label_at(&self, pos: usize) -> Option<(usize, String)> {
        if self.byte(pos) != b'[' || self.byte(pos + 1) != b'^' {
            return None;
        }
        let end = crate::scan::scan_note_label(self.s, pos)?;
        let label = normalize_label(self.slice(pos + 2, end - 1));
        if label.is_empty() || self.env.current_note == Some(label.as_str()) {
            return None;
        }
        self.env.notes.contains_key(&label).then_some((end, label))
    }

    fn parse_note_ref(&mut self) -> bool {
        let Some((end, label)) = self.note_label_at(self.pos) else {
            return false;
        };
        let Some(blocks) = self.env.notes.get(&label) else {
            return false;
        };
        self.pos = end;
        if self.env.current_note.is_some() {
            self.append_inline(Inline::Str(String::new()));
        } else {
            let blocks = blocks.clone();
            self.append_inline(Inline::Note(blocks));
        }
        true
    }

    fn close_bracket(&mut self) {
        self.pos += 1;
        let after = self.pos;
        let Some(opener) = self.brackets.last().cloned() else {
            self.append_text("]");
            return;
        };
        if self.config().spans
            && self.peek() == b'{'
            && let Some((end, pairs)) = crate::attrs::parse_attributes(self.text, self.pos)
        {
            self.pos = end;
            self.bracket_end = Some(end);
            let attr = crate::attrs::merge(pairs, Attr::default());
            // After `!`, a span keeps the `!` as text.
            self.finish_bracket(&opener, after, opener.image, |children| {
                Inline::Span(attr, children)
            });
            return;
        }
        let index = self.brackets.len().saturating_sub(1);
        if !opener.image && index < self.link_floor {
            self.bracket_end = Some(after);
            self.pop_bracket();
            self.append_text("]");
            return;
        }
        let mut found: Option<(String, String, Attr)> = None;
        if self.peek() == b'(' {
            let p = skip_spnl(self.s, self.pos + 1);
            if let Some((dest_end, dest)) = scan_link_destination(self.s, p) {
                let mut q = skip_spnl(self.s, dest_end);
                let mut title = String::new();
                let space_before = q > dest_end;
                let mut has_title = false;
                if space_before && let Some((title_end, raw)) = scan_link_title(self.s, q) {
                    title = unescape(raw);
                    q = skip_spnl(self.s, title_end);
                    has_title = true;
                }
                if self.byte(q) == b')' && (has_title || !space_before) {
                    let url = crate::rebase::rebase_path(self.env.dir, unescape_destination(dest));
                    found = Some((url, title, Attr::default()));
                    self.pos = q + 1;
                }
            }
        }
        if found.is_none() {
            let label_end = scan_link_label(self.s, self.pos);
            let label = match label_end {
                Some(end) if end > self.pos + 2 => Some(self.slice(self.pos + 1, end - 1)),
                _ if !opener.bracket_after => Some(self.slice(opener.start, after - 1)),
                _ => None,
            };
            if let Some(label) = label
                && let Some(def) = self.lookup(label)
            {
                found = Some(def);
                if let Some(end) = label_end {
                    self.pos = end;
                }
            }
        }
        match found {
            Some((url, title, attr)) => {
                self.bracket_end = Some(self.pos);
                let target = Target::new(url, title);
                let mut attr = attr;
                if self.config().attributes
                    && let Some((end, extra)) = crate::attrs::parse_attributes(self.text, self.pos)
                {
                    self.pos = end;
                    attr = crate::attrs::merge(extra, attr);
                }
                let image = opener.image;
                let made = self.finish_bracket(&opener, after, false, |children| {
                    if image {
                        Inline::Image(attr, children, target)
                    } else {
                        Inline::Link(attr, children, target)
                    }
                });
                if made && !image {
                    self.link_floor = self.brackets.len();
                }
            }
            None => {
                self.pop_bracket();
                self.pos = after;
                self.bracket_end = Some(after);
                self.append_text("]");
                // With no chunk between the bottom and the opener, `processBs` moves its
                // bottom after the closer, and a `[]` right after it never pairs: it
                // stays text, and attributes after it go to the token `]` only.
                let opener_at = opener.start.saturating_sub(1);
                if self.bracket_bottom.is_none_or(|bottom| opener_at <= bottom) {
                    self.bracket_bottom = Some(after);
                    if self.s.get(after..after + 2) == Some(b"[]") {
                        self.pos = after + 2;
                        self.append_text("[");
                        self.append(Kind::Token("]".into()));
                    }
                }
            }
        }
    }

    /// Makes an inline from the nodes after the opener. Returns `false` when the
    /// nesting limit stops it.
    /// Makes an inline from the nodes after the opener. When the nesting limit
    /// stops it, the text after `]` (at `after`) is read again as text.
    fn finish_bracket(
        &mut self,
        opener: &Bracket,
        after: usize,
        bang: bool,
        make: impl FnOnce(Vec<Inline>) -> Inline,
    ) -> bool {
        if self.exhausted {
            self.pop_bracket();
            self.pos = after;
            self.append_text("]");
            return false;
        }
        self.process_emphasis(Some(opener.node));
        self.pop_bracket();
        let mut depth = 0;
        let mut cursor = self.nodes.get(opener.node).and_then(|n| n.next);
        while let Some(index) = cursor {
            crate::steps::add(1);
            depth = depth.max(self.node_depth(index));
            cursor = self.nodes.get(index).and_then(|n| n.next);
        }
        if depth + 1 >= self.env.depth_budget {
            self.exhausted = true;
            self.pos = after;
            self.append_text("]");
            return false;
        }
        let (children, depth) = self.take_between(opener.node, None);
        let node = opener.node;
        let inline = make(children);
        let index = self.insert_after(node, Kind::Inline(inline, depth + 1));
        if bang {
            if let Some(n) = self.nodes.get_mut(node) {
                n.kind = Kind::Token("!".into());
            }
        } else {
            self.unlink(node);
        }
        self.tail = Some(index);
        true
    }

    fn lookup(&self, label: &str) -> Option<(String, String, Attr)> {
        let key = normalize_label(label);
        if key.is_empty() {
            return None;
        }
        self.env
            .refs
            .get(&key)
            .map(|def| (def.url.clone(), def.title.clone(), def.attr.clone()))
    }

    fn run_at(&self, index: usize) -> Option<Run> {
        match self.nodes.get(index) {
            Some(Node {
                kind: Kind::Delim(run),
                ..
            }) => Some(*run),
            _ => None,
        }
    }

    fn use_delims(&mut self, index: usize, used: usize) {
        if let Some(Node {
            kind: Kind::Delim(run),
            ..
        }) = self.nodes.get_mut(index)
        {
            run.count -= used;
        }
    }

    fn next_node(&self, index: usize) -> Option<usize> {
        self.nodes.get(index).and_then(|n| n.next)
    }

    /// Matches the delimiter runs of the nodes after `after` (of all nodes for `None`).
    ///
    /// This is a port of `processEmphasis` and `processEm` of `Commonmark/Inlines.hs`
    /// (`commonmark` 0.3.1). The search for an opener goes left over each node, not only
    /// over the runs, because the place of an element can stop the search.
    fn process_emphasis(&mut self, after: Option<usize>) {
        let mut right = match after {
            Some(a) => self.next_node(a),
            None => self.head,
        };
        while let Some(r) = right {
            crate::steps::add(1);
            if self.run_at(r).is_some_and(|run| run.can_open) {
                break;
            }
            right = self.next_node(r);
        }
        // For each kind of closer, the place of the last closer that found no opener
        // (`stackBottoms`). The length in the key is the length that is left.
        let mut bottoms: HashMap<(u8, bool, usize), usize> = HashMap::new();
        while let Some(r) = right {
            crate::steps::add(1);
            let Some(close) = self.run_at(r).filter(|run| run.can_close) else {
                right = self.next_node(r);
                continue;
            };
            let key = (close.ch, close.can_open, close.count % 3);
            let bottom = bottoms.get(&key).copied();
            let mut found = None;
            let mut left = self.nodes.get(r).and_then(|n| n.prev);
            while let Some(l) = left {
                crate::steps::add(1);
                if Some(l) == after {
                    break;
                }
                let Some(node) = self.nodes.get(l) else { break };
                if let Kind::Delim(open) = &node.kind
                    && delims_match(open, &close)
                {
                    found = Some((l, *open));
                    break;
                }
                if bottom.is_some_and(|b| node.pos <= b) {
                    break;
                }
                left = node.prev;
            }
            let matched = found.filter(|&(l, _)| {
                let mut depth = 0;
                if !self.exhausted {
                    let mut cursor = self.next_node(l);
                    while let Some(index) = cursor {
                        crate::steps::add(1);
                        if index == r {
                            break;
                        }
                        depth = depth.max(self.node_depth(index));
                        cursor = self.next_node(index);
                    }
                }
                self.exhausted = self.exhausted || depth + 1 >= self.env.depth_budget;
                !self.exhausted
            });
            let Some((l, open)) = matched else {
                if let Some(node) = self.nodes.get(r) {
                    bottoms.insert(key, node.pos);
                }
                right = self.next_node(r);
                continue;
            };
            let (ctor, used) = match (close.spec.single, close.spec.double) {
                (_, Some(double)) if open.count.min(close.count) >= 2 => (Some(double), 2),
                (Some(single), _) => (Some(single), 1),
                _ => (None, 1),
            };
            self.use_delims(l, used);
            self.use_delims(r, used);
            let (children, depth) = self.take_between(l, Some(r));
            let kind = match make(ctor, children) {
                Ok(inline) => Kind::Inline(inline, depth + 1),
                Err(children) => {
                    let mut out = Inlines::new();
                    let mark = (close.ch as char).to_string();
                    out.text(&mark);
                    out.extend(children);
                    out.text(&mark);
                    Kind::Many(out.into_vec(), depth)
                }
            };
            self.insert_after(l, kind);
            if open.count == used {
                self.unlink(l);
            }
            if close.count == used {
                right = self.next_node(r);
                self.unlink(r);
            }
        }
    }
}

/// The rule of `delimsMatch` in `Commonmark/Inlines.hs`. It uses the lengths of the runs
/// in the source, not the lengths that are left.
fn delims_match(open: &Run, close: &Run) -> bool {
    let both = (open.can_open && open.can_close) || (close.can_open && close.can_close);
    open.can_open
        && close.can_close
        && open.ch == close.ch
        && (!both || close.orig.is_multiple_of(3) || !(open.orig + close.orig).is_multiple_of(3))
}

/// Makes the inline for a delimiter pair, or gives the children back when the pair has no constructor.
fn make(ctor: Option<Ctor>, children: Vec<Inline>) -> Result<Inline, Vec<Inline>> {
    Ok(match ctor {
        Some(Ctor::Emph) => Inline::Emph(children),
        Some(Ctor::Strong) => Inline::Strong(children),
        Some(Ctor::Strikeout) => Inline::Strikeout(children),
        Some(Ctor::Subscript) => Inline::Subscript(children),
        Some(Ctor::Superscript) => Inline::Superscript(children),
        Some(Ctor::SingleQuoted) => Inline::Quoted(QuoteType::SingleQuote, children),
        Some(Ctor::DoubleQuoted) => Inline::Quoted(QuoteType::DoubleQuote, children),
        None => return Err(children),
    })
}

/// Normalizes the content of a code span: line endings become spaces, and one
/// space is removed from each end when both ends have one.
pub(crate) fn normalize_code(raw: &str) -> String {
    let text: String = raw
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    if text.len() >= 2
        && text.starts_with(' ')
        && text.ends_with(' ')
        && !text.bytes().all(|b| b == b' ')
    {
        text.get(1..text.len() - 1).unwrap_or("").to_owned()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_ticks() {
        assert_eq!(strip_math_ticks("``".into()), "``");
        assert_eq!(strip_math_ticks("``a`b``".into()), "a`b");
        assert_eq!(strip_math_ticks("` `".into()), " ");
        assert_eq!(strip_math_ticks("`a".into()), "`a");
    }

    #[test]
    fn code_normalization() {
        assert_eq!(normalize_code(" a "), "a");
        assert_eq!(normalize_code("  "), "  ");
        assert_eq!(normalize_code("a\nb"), "a b");
        assert_eq!(normalize_code(" a"), " a");
    }
}
