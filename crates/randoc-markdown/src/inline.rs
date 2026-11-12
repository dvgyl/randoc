//! The inline phase: Pandoc's inline grammar for Markdown.

use crate::block::SPACE;
use randoc_ast::{
    Attr, Block, Citation, CitationMode, Format, Inline, MathType, QuoteType, Target, Text,
};
use randoc_core::Extension;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use crate::block::{Parser, escape_uri, html_attr, html_tag_len, normalize_key};

/// The format of a marker inline that the parser removes after merging.
const NOTHING: &str = "\u{0}nothing";

/// What ends a sequence of inlines.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Stop {
    None,
    DoubleQuote,
    SingleQuote,
    Superscript,
    Subscript,
    Strikeout,
    Mark,
}

/// The kind of text that a window parse reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Window {
    /// The text of a block, trimmed at both ends.
    Text,
    /// The text of a link or an image: trimmed, and it ends at a block tag.
    Label,
    /// The text of a bracketed span or an inline note: it keeps its white
    /// space at the ends, and it ends at a block tag.
    Content,
    /// The contents of a native span: it keeps its white space at the ends,
    /// and it ends at a block tag.
    Element,
}

/// A window `lo..hi` of one text buffer. A sub-parse of a label or of span
/// contents reads the buffer of its parent by absolute position, with no copy.
struct Chars {
    all: Rc<[char]>,
    lo: usize,
    hi: usize,
}

/// A position or a range in a `Chars` window.
trait At {
    type Out: ?Sized;
    fn at(self, chars: &Chars) -> Option<&Self::Out>;
}

impl At for usize {
    type Out = char;
    fn at(self, chars: &Chars) -> Option<&char> {
        if self < chars.lo || self >= chars.hi {
            return None;
        }
        chars.all.get(self)
    }
}

impl At for Range<usize> {
    type Out = [char];
    fn at(self, chars: &Chars) -> Option<&[char]> {
        if self.start < chars.lo || self.end > chars.hi {
            return None;
        }
        chars.all.get(self)
    }
}

impl Chars {
    fn get<I: At>(&self, index: I) -> Option<&I::Out> {
        index.at(self)
    }

    /// The end of the window: positions are absolute.
    fn len(&self) -> usize {
        self.hi
    }
}

struct Inl<'p> {
    p: &'p mut Parser,
    s: Chars,
    i: usize,
    in_link: bool,
    /// The kind of text. Each kind but `Window::Text` ends at the first
    /// block tag that the parse reaches.
    window: Window,
    depth: usize,
    ix: Index,
    /// The number of open superscripts and subscripts.
    scripts: usize,
    /// Bit 1: inside a single quote; bit 2: inside a double quote.
    quotes: u8,
    /// The shared result buffer of emphasis attempts.
    buf: Vec<Inline>,
    /// The next call of `inline` comes from an emphasis loop.
    direct: bool,
    /// The number of active `seq_closing` loops. When it is above zero, the
    /// result of an inline that runs to the end of the text is discarded.
    closing: usize,
}

/// A construct whose failure the memo keeps.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Construct {
    /// An emphasis opener: the delimiter and the length of its run.
    Emphasis(char, usize),
    /// A quote, script, strikeout or mark opener, by its stop.
    Opener(Stop),
    /// The contents of a delimited sequence from a position, by its stop.
    Contents(Stop),
}

/// The memo key: the construct, its start, and the context flags
/// (`Inl::memo_flags`). The text and the extensions are fixed for one `Inl`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MemoKey {
    at: usize,
    construct: Construct,
    flags: u8,
}

/// A character class that `find` searches for.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Find {
    Char(char),
    /// A `'` that can close a single quote.
    QuoteCloser,
    /// A `$` with no backslash before it.
    Dollar,
    /// Two `$` characters.
    DoubleDollar,
    /// A blank line inside the text.
    BlankLine,
    /// Two `~` characters.
    DoubleTilde,
    /// Two `=` characters.
    DoubleEquals,
    /// A character with an even number of backslashes before it.
    Unescaped(char),
    /// A white-space character.
    Space,
    /// A run of the emphasis delimiter that can close an emphasis.
    Ender(char, usize),
}

/// The memo of one parse, and the lookup tables that it shares with the
/// sub-parses of windows of its buffer.
#[derive(Default)]
struct Index {
    tables: Rc<RefCell<Tables>>,
    /// The memo of failed attempts, with the largest depth of a failure.
    /// A failure holds at each depth that is not larger. If the depth limit
    /// did not stop a parse inside the attempt, a smaller depth gives the
    /// same parse. If it did, the input is deeper than the limit, and the
    /// result stays limited in this way so that the time stays linear.
    memo: HashMap<MemoKey, usize>,
}

/// Lazily built lookup tables over the whole buffer, so that a failed opener
/// does not scan the rest of the text again. A sub-parse window starts after
/// a `[` or a `>` and ends before the matching `]` or `<`, so a match that
/// the tables give inside the window is a match of the window text too; a
/// match past the end of the window is no match.
#[derive(Default)]
struct Tables {
    /// For each `[`, the index of its `]`, or `u32::MAX`.
    brackets: Option<Vec<u32>>,
    /// The start of each maximal backtick run, by run length.
    code_runs: Option<HashMap<usize, Vec<usize>>>,
    /// The last query and its result for each class. The first match at or
    /// after a later start is the same while the start does not pass it.
    finds: HashMap<Find, (usize, Option<usize>)>,
    /// The same for literal patterns.
    patterns: HashMap<Text, (usize, Option<usize>)>,
    /// The positions of each `\end{name}`, by name.
    env_ends: Option<HashMap<Text, Vec<usize>>>,
    /// For each `(`, the index of its `)`, or `u32::MAX`.
    parens: Option<Vec<u32>>,
    /// For each `<span>` tag, the start of its closing tag.
    spans: Option<HashMap<usize, usize>>,
}

fn is_alnum(c: Option<char>) -> bool {
    c.is_some_and(char::is_alphanumeric)
}

fn is_space(c: Option<char>) -> bool {
    matches!(c, Some(' ' | '\t' | '\n') | None)
}

impl Parser {
    /// Parses raw Markdown text into inlines.
    pub(crate) fn parse_inlines(&mut self, text: &str) -> Vec<Inline> {
        self.parse_inlines_in(text, false)
    }

    fn parse_inlines_in(&mut self, text: &str, in_link: bool) -> Vec<Inline> {
        crate::step(text.len());
        let all: Rc<[char]> = text.chars().collect();
        let hi = all.len();
        let chars = Chars { all, lo: 0, hi };
        self.parse_window(chars, Index::default(), in_link, Window::Text)
    }

    fn parse_window(
        &mut self,
        chars: Chars,
        ix: Index,
        in_link: bool,
        kind: Window,
    ) -> Vec<Inline> {
        let depth = self.depth;
        let mut inl = Inl::new(self, chars, ix, in_link, depth);
        inl.window = kind;
        let seq = inl.seq(Stop::None);
        let mut out = match kind {
            Window::Text | Window::Label => normalize(seq),
            Window::Content | Window::Element => join_white_space(seq),
        };
        out.retain(|x| !matches!(x, Inline::RawInline(f, _) if f.as_str() == NOTHING));
        if self.ext(Extension::Smart) {
            let abbreviations = std::mem::take(&mut self.abbreviations);
            let out = join_abbreviations(out, &abbreviations);
            self.abbreviations = abbreviations;
            return out;
        }
        out
    }
}

impl<'p> Inl<'p> {
    fn new(p: &'p mut Parser, s: Chars, ix: Index, in_link: bool, depth: usize) -> Self {
        Inl {
            p,
            i: s.lo,
            s,
            in_link,
            window: Window::Text,
            depth,
            ix,
            scripts: 0,
            quotes: 0,
            buf: Vec::new(),
            direct: false,
            closing: 0,
        }
    }

    fn memo_key(&self, at: usize, construct: Construct) -> MemoKey {
        MemoKey {
            at,
            construct,
            flags: self.memo_flags(),
        }
    }

    /// Returns true when an earlier attempt of `construct` at `at` failed in
    /// the current context, at the current depth or deeper.
    fn failed_before(&self, at: usize, construct: Construct) -> bool {
        self.ix
            .memo
            .get(&self.memo_key(at, construct))
            .is_some_and(|depth| self.depth <= *depth)
    }

    fn remember_failure(&mut self, at: usize, construct: Construct) {
        let depth = self.depth;
        let entry = self
            .ix
            .memo
            .entry(self.memo_key(at, construct))
            .or_default();
        *entry = (*entry).max(depth);
    }
}

impl Inl<'_> {
    fn peek(&self, k: usize) -> Option<char> {
        self.s.get(self.i + k).copied()
    }

    fn cur(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn prev(&self) -> Option<char> {
        self.i.checked_sub(1).and_then(|k| self.s.get(k)).copied()
    }

    fn ext(&self, e: Extension) -> bool {
        self.p.ext(e)
    }

    fn starts_with(&self, pat: &str) -> bool {
        pat.chars()
            .enumerate()
            .all(|(k, c)| self.s.get(self.i + k) == Some(&c))
    }

    fn text(&self, from: usize, to: usize) -> Text {
        crate::step(to.saturating_sub(from));
        self.s.get(from..to).unwrap_or(&[]).iter().collect()
    }

    fn matches_at(&self, kind: Find, k: usize) -> bool {
        crate::step(1);
        let at = |j: usize| self.s.get(j).copied();
        match kind {
            Find::Char(c) => at(k) == Some(c),
            Find::QuoteCloser => at(k) == Some('\'') && !is_alnum(at(k + 1)),
            Find::Dollar => at(k) == Some('$') && !self.escaped(k),
            Find::DoubleDollar => at(k) == Some('$') && at(k + 1) == Some('$') && !self.escaped(k),
            Find::Unescaped(c) => at(k) == Some(c) && !self.escaped(k),
            Find::Space => at(k).is_some_and(char::is_whitespace),
            Find::BlankLine => at(k) == Some('\n') && at(k + 1) == Some('\n'),
            Find::DoubleTilde => at(k) == Some('~') && at(k + 1) == Some('~'),
            Find::DoubleEquals => at(k) == Some('=') && at(k + 1) == Some('='),
            Find::Ender(c, n) => self.ender_at(k, c, n),
        }
    }

    /// Runs `f` on the whole buffer, so that a table holds for each window.
    fn widened<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let (lo, hi) = (self.s.lo, self.s.hi);
        self.s.lo = 0;
        self.s.hi = self.s.all.len();
        let out = f(self);
        self.s.lo = lo;
        self.s.hi = hi;
        out
    }

    /// Returns `k` when it is a match inside the window.
    fn inside(&self, k: u32) -> Option<usize> {
        let k = usize::try_from(k).ok()?;
        (k < self.s.hi && k != u32::MAX as usize).then_some(k)
    }

    /// Returns the first index at or after `from` where `kind` matches.
    fn find(&mut self, kind: Find, from: usize) -> Option<usize> {
        let hi = self.s.hi;
        self.widened(|inl| inl.find_in_buffer(kind, from))
            .filter(|k| *k < hi)
    }

    fn find_in_buffer(&mut self, kind: Find, from: usize) -> Option<usize> {
        let tables = Rc::clone(&self.ix.tables);
        if let Some(&(start, result)) = tables.borrow().finds.get(&kind)
            && from >= start
            && result.is_none_or(|r| r >= from)
        {
            return result;
        }
        // No match from `start` on: only the text before `start` is new.
        let known_empty = match tables.borrow().finds.get(&kind) {
            Some(&(start, None)) if from < start => start,
            _ => self.s.len(),
        };
        let result = (from..known_empty).find(|k| self.matches_at(kind, *k));
        tables.borrow_mut().finds.insert(kind, (from, result));
        result
    }

    /// Returns the first index at or after `from` where the literal `pat` starts.
    fn find_pattern(&mut self, pat: &str, from: usize) -> Option<usize> {
        let end = self.s.hi.checked_sub(pat.chars().count())?;
        self.widened(|inl| inl.find_pattern_in_buffer(pat, from))
            .filter(|k| *k <= end)
    }

    fn find_pattern_in_buffer(&mut self, pat: &str, from: usize) -> Option<usize> {
        let tables = Rc::clone(&self.ix.tables);
        if let Some(&(start, result)) = tables.borrow().patterns.get(pat)
            && from >= start
            && result.is_none_or(|r| r >= from)
        {
            return result;
        }
        let chars: Vec<char> = pat.chars().collect();
        let result = (from..self.s.len()).find(|k| {
            crate::step(1);
            chars
                .iter()
                .enumerate()
                .all(|(j, c)| self.s.get(k + j) == Some(c))
        });
        tables
            .borrow_mut()
            .patterns
            .insert(pat.to_string(), (from, result));
        result
    }

    /// Returns the text from `from` up to the next `>` (included), but not
    /// past the next `<`. A tag or an automatic link fits in this window.
    fn tag_window(&mut self, from: usize) -> Text {
        let end = if self.s.get(from + 1..from + 4) == Some(&['!', '-', '-']) {
            self.find_pattern("-->", from + 4)
                .map_or(from + 4, |k| k + 3)
        } else if self.s.get(from + 1) == Some(&'?') {
            self.find(Find::Char('>'), from + 2)
                .map_or(from + 2, |k| k + 1)
        } else {
            let mut k = from + 1;
            loop {
                let gt = self.find(Find::Char('>'), k).unwrap_or(self.s.len());
                let lt = self.find(Find::Char('<'), k).unwrap_or(self.s.len());
                let dq = self.find(Find::Char('"'), k).unwrap_or(self.s.len());
                let sq = self.find(Find::Char('\''), k).unwrap_or(self.s.len());
                let quote = dq.min(sq);
                if quote < gt && quote < lt {
                    let mark = if quote == dq { '"' } else { '\'' };
                    match self.find(Find::Char(mark), quote + 1) {
                        Some(close) => k = close + 1,
                        None => break gt.min(lt).min(self.s.len()),
                    }
                    continue;
                }
                break if gt < lt { gt + 1 } else { lt };
            }
        };
        self.text(from, end.min(self.s.len()))
    }

    fn code_closer(&mut self, after: usize, n: usize) -> Option<usize> {
        if self.ix.tables.borrow().code_runs.is_none() {
            let runs = self.widened(Self::backtick_runs);
            self.ix.tables.borrow_mut().code_runs = Some(runs);
        }
        let tables = self.ix.tables.borrow();
        let starts = tables.code_runs.as_ref()?.get(&n)?;
        let index = starts.partition_point(|s| *s <= after);
        starts.get(index).copied().filter(|k| *k < self.s.hi)
    }

    fn backtick_runs(&mut self) -> HashMap<usize, Vec<usize>> {
        {
            let mut runs: HashMap<usize, Vec<usize>> = HashMap::new();
            let mut k = 0;
            while k < self.s.len() {
                if self.s.get(k) == Some(&'`') {
                    let start = k;
                    while self.s.get(k) == Some(&'`') {
                        k += 1;
                    }
                    runs.entry(k - start).or_default().push(start);
                } else {
                    k += 1;
                }
            }
            runs
        }
    }

    /// Returns the start of the `</span>` tag that closes the `<span>` tag at `open`.
    /// One pass over the tags of the text matches each opening tag.
    fn span_close(&mut self, open: usize) -> Option<usize> {
        if self.ix.tables.borrow().spans.is_none() {
            let spans = self.widened(|inl| inl.span_tags(0, |_, _| false));
            self.ix.tables.borrow_mut().spans = Some(spans);
        }
        let close = self.ix.tables.borrow().spans.as_ref()?.get(&open).copied();
        #[cfg(test)]
        crate::check_cache(&close, || {
            let mut plain = None;
            self.widened(|inl| {
                inl.span_tags(open, |o, c| {
                    if o == open {
                        plain = Some(c);
                    }
                    o == open
                })
            });
            plain
        });
        close.filter(|c| *c < self.s.hi)
    }

    /// Matches the `<span>` tags from `from` on, with the token boundaries of
    /// the inline parse: a tag in an escape, a code span, a math span, a
    /// comment or an attribute value does not count. `done` gets each
    /// opening tag and the start of its closing tag; the scan stops when it
    /// returns true.
    fn span_tags(
        &mut self,
        from: usize,
        mut done: impl FnMut(usize, usize) -> bool,
    ) -> HashMap<usize, usize> {
        let mut matches = HashMap::new();
        let mut stack: Vec<usize> = Vec::new();
        let mut k = from;
        while let Some(&c) = self.s.get(k) {
            crate::step(1);
            match c {
                '\\' => k += 2,
                '`' => {
                    let mut n = 0;
                    while self.s.get(k + n) == Some(&'`') {
                        n += 1;
                    }
                    k = self.code_closer(k, n).map_or(k + n, |end| end + n);
                }
                '$' if self.ext(Extension::TexMathDollars) => {
                    let saved = self.i;
                    self.i = k;
                    let math = self.dollar_math().is_some();
                    k = if math { self.i } else { k + 1 };
                    self.i = saved;
                }
                '<' => {
                    let window = self.tag_window(k);
                    let lower = window.to_ascii_lowercase();
                    let Some(len) = html_tag_len(&window) else {
                        k += 1;
                        continue;
                    };
                    if lower.starts_with("</span") {
                        if let Some(o) = stack.pop() {
                            matches.insert(o, k);
                            if done(o, k) {
                                break;
                            }
                        }
                    } else if lower.starts_with("<span")
                        && !window.get(..len).is_some_and(|t| t.ends_with("/>"))
                    {
                        stack.push(k);
                    }
                    k += window.get(..len).map_or(1, |t| t.chars().count());
                }
                _ => k += 1,
            }
        }
        matches
    }

    fn build_brackets(&mut self) -> Vec<u32> {
        let mut matches = vec![u32::MAX; self.s.len()];
        let mut stack: Vec<usize> = Vec::new();
        let mut k = 0;
        while let Some(&c) = self.s.get(k) {
            match c {
                '\\' => {
                    k += 2;
                    continue;
                }
                '`' => {
                    let mut n = 0;
                    while self.s.get(k + n) == Some(&'`') {
                        n += 1;
                    }
                    k = match self.code_closer(k, n) {
                        Some(end) => end + n,
                        None => k + n,
                    };
                    continue;
                }
                '<' => {
                    let window = self.tag_window(k);
                    if let Some(len) = html_tag_len(&window).or_else(|| autolink_len(&window)) {
                        k += window.get(..len).map_or(1, |t| t.chars().count());
                        continue;
                    }
                }
                '[' => stack.push(k),
                ']' => {
                    if let Some(open) = stack.pop()
                        && let Some(slot) = matches.get_mut(open)
                    {
                        *slot = u32::try_from(k).unwrap_or(u32::MAX);
                    }
                }
                _ => {}
            }
            k += 1;
        }
        matches
    }

    fn ender_at(&self, at: usize, c: char, n: usize) -> bool {
        for k in 0..n {
            if self.s.get(at + k) != Some(&c) {
                return false;
            }
        }
        c == '*'
            || !self.ext(Extension::IntrawordUnderscores)
            || !is_alnum(self.s.get(at + n).copied())
    }

    fn at_stop(&self, stop: Stop) -> bool {
        match stop {
            Stop::None => false,
            Stop::DoubleQuote => self.cur() == Some('"'),
            Stop::SingleQuote => self.cur() == Some('\'') && !is_alnum(self.peek(1)),
            Stop::Superscript => self.cur() == Some('^'),
            Stop::Subscript => self.cur() == Some('~'),
            Stop::Strikeout => self.starts_with("~~"),
            Stop::Mark => self.starts_with("=="),
        }
    }

    fn seq(&mut self, stop: Stop) -> Vec<Inline> {
        let mut out = Vec::new();
        while self.i < self.s.len() && !self.at_stop(stop) {
            if matches!(stop, Stop::Superscript | Stop::Subscript)
                && matches!(self.cur(), Some(' ' | '\n'))
            {
                break;
            }
            let start = self.i;
            out.extend(self.inline());
            if self.i == start {
                self.i += 1;
            }
        }
        out
    }

    /// Returns the context flags that change the result of a sequence: inside
    /// a link, inside a superscript or a subscript, inside a single quote,
    /// inside a double quote. Each context has its own bit.
    fn memo_flags(&self) -> u8 {
        u8::from(self.in_link)
            | (u8::from(self.scripts > 0) << 1)
            | (u8::from(self.quotes & 1 != 0) << 2)
            | (u8::from(self.quotes & 2 != 0) << 3)
    }

    /// Parses inlines up to `stop` and returns them when the stop is found.
    ///
    /// A sequence that starts at a position with the same stop and flags
    /// always ends the same way. When one such sequence finds no stop, each
    /// position where it read an inline is remembered: a sequence that
    /// starts there reads the same inlines. A later sequence that reaches or
    /// starts at one of them fails at once. Thus each position is read at
    /// most one time for each stop.
    fn seq_closing(&mut self, stop: Stop) -> Option<Vec<Inline>> {
        let begin = self.i;
        let mut visited = Vec::new();
        // A failed sequence is not in the result, so its note numbers do not count.
        let note_number = self.p.note_number;
        // The contents go into the shared buffer, so that a nested emphasis
        // that fails does not copy its contents into this sequence.
        let mark = self.buf.len();
        self.closing += 1;
        let found = loop {
            if self.failed_before(self.i, Construct::Contents(stop)) {
                break false;
            }
            if self.at_stop(stop) {
                break true;
            }
            if self.i >= self.s.len()
                || matches!(stop, Stop::Superscript | Stop::Subscript)
                    && matches!(self.cur(), Some(' ' | '\n'))
            {
                break false;
            }
            visited.push(self.i);
            if self.at_limit() {
                match self.literal_stop(stop) {
                    Some(end) => self.literal_to(end),
                    None => break false,
                }
                continue;
            }
            self.inline_into_buffer();
        };
        self.closing -= 1;
        if found {
            return Some(self.buf.drain(mark..).collect());
        }
        self.buf.truncate(mark);
        self.p.note_number = note_number;
        self.remember_failure(begin, Construct::Contents(stop));
        for at in visited {
            self.remember_failure(at, Construct::Contents(stop));
        }
        None
    }

    /// Returns true at the depth limit. There `inline` gives each character
    /// as literal text, so a loop only looks for its own closer. The loops
    /// find the closer with `find`, so that the passes of many failed
    /// attempts over the rest of the text do not read each character again.
    fn at_limit(&self) -> bool {
        self.depth >= self.p.max_depth
    }

    /// Appends the text up to `end` to the buffer as one literal `Str`.
    fn literal_to(&mut self, end: usize) {
        if end > self.i {
            let text = self.text(self.i, end);
            self.buf.push(Inline::Str(text));
            self.i = end;
        }
    }

    /// Returns the position of `stop` when the text from here is literal.
    /// The caller checked that the current position is no stop.
    fn literal_stop(&mut self, stop: Stop) -> Option<usize> {
        let from = self.i + 1;
        let script = |inl: &mut Self, closer: char| {
            let end = inl.find(Find::Char(closer), from)?;
            let space = inl.find(Find::Char(' '), from).unwrap_or(end);
            let newline = inl.find(Find::Char('\n'), from).unwrap_or(end);
            (end <= space.min(newline)).then_some(end)
        };
        match stop {
            Stop::None => None,
            Stop::DoubleQuote => self.find(Find::Char('"'), from),
            Stop::SingleQuote => self.find(Find::QuoteCloser, from),
            Stop::Superscript => script(self, '^'),
            Stop::Subscript => script(self, '~'),
            Stop::Strikeout => self.find(Find::DoubleTilde, from),
            Stop::Mark => self.find(Find::DoubleEquals, from),
        }
    }

    /// Moves a literal emphasis loop to the next ender of `n` characters, or
    /// to the end of the text. The caller checked that the current position
    /// is no ender. An active sequence fails at the end of the text and
    /// discards the result, so no text is made for it.
    fn literal_to_ender(&mut self, c: char, n: usize) {
        match self.find(Find::Ender(c, n), self.i + 1) {
            Some(end) => self.literal_to(end),
            None if self.closing > 0 => self.i = self.s.len(),
            None => self.literal_to(self.s.len()),
        }
    }

    fn inline(&mut self) -> Vec<Inline> {
        // Only the call that an emphasis loop makes may write into the
        // emphasis buffer.
        let direct = std::mem::take(&mut self.direct);
        let Some(c) = self.cur() else {
            return Vec::new();
        };
        crate::step(1);
        if self.at_limit() {
            self.i += 1;
            return vec![Inline::Str(c.to_string())];
        }
        self.depth += 1;
        let out = if matches!(c, '*' | '_') {
            self.enclosure(c, direct)
        } else {
            self.inline_inner(c)
        };
        self.depth -= 1;
        out
    }

    /// Parses one inline inside an emphasis and appends it to the emphasis buffer.
    fn inline_into_buffer(&mut self) {
        let start = self.i;
        self.direct = true;
        let out = self.inline();
        self.direct = false;
        self.buf.extend(out);
        if self.i == start {
            self.i += 1;
        }
    }

    fn inline_inner(&mut self, c: char) -> Vec<Inline> {
        let tried = match c {
            ' ' | '\t' => return self.whitespace(),
            '\n' => return self.newline(),
            '\\' => return self.escape(),
            '*' | '_' => return self.enclosure(c, false),
            '`' => return self.code(),
            '[' => self.bracket(false),
            '!' if self.peek(1) == Some('[') => self.bracket(true),
            '^' => self.caret(),
            '~' => self.tilde(),
            '<' => self.angle(),
            '$' if self.ext(Extension::TexMathDollars) => self.dollar_math(),
            '&' => self.entity(),
            '@' if self.ext(Extension::Citations) || self.ext(Extension::ExampleLists) => {
                self.at_sign()
            }
            '"' if self.ext(Extension::Smart) && is_alnum(self.prev()) => {
                self.i += 1;
                return vec![Inline::Str("\u{201d}".into())];
            }
            '"' if self.ext(Extension::Smart) => {
                let out = self.double_quoted();
                if out.is_none() {
                    self.i += 1;
                    return vec![Inline::Str("\u{201d}".into())];
                }
                out
            }
            '\'' if self.ext(Extension::Smart) => return self.single_quote(),
            '-' if self.peek(1) == Some('@')
                && self.ext(Extension::Citations)
                && !is_alnum(self.prev()) =>
            {
                self.suppress_cite()
            }
            '-' if self.ext(Extension::Smart) => self.dash(),
            '.' if self.ext(Extension::Smart) && self.starts_with("...") => {
                self.i += 3;
                Some(vec![Inline::Str("\u{2026}".into())])
            }
            '=' if self.ext(Extension::Mark) && self.starts_with("==") => self.mark(),
            ':' if self.ext(Extension::Emoji) => self.emoji(),
            _ => None,
        };
        if let Some(out) = tried {
            return out;
        }
        if let Some(out) = self.bare_uri() {
            return out;
        }
        self.str_run()
    }

    fn is_special(&self, c: char) -> bool {
        matches!(
            c,
            ' ' | '\t'
                | '\n'
                | '\\'
                | '*'
                | '_'
                | '`'
                | '['
                | ']'
                | '!'
                | '^'
                | '~'
                | '<'
                | '$'
                | '&'
                | '@'
                | '"'
                | '\''
                | '-'
                | '.'
                | '='
                | ':'
        )
    }

    fn str_run(&mut self) -> Vec<Inline> {
        let start = self.i;
        self.i += 1;
        while let Some(c) = self.cur() {
            if self.is_special(c) {
                break;
            }
            self.i += 1;
        }
        vec![Inline::Str(self.text(start, self.i))]
    }

    fn whitespace(&mut self) -> Vec<Inline> {
        let start = self.i;
        while matches!(self.cur(), Some(' ' | '\t')) {
            self.i += 1;
        }
        if self.cur() == Some('\n') {
            if self.i - start >= 2 && self.text_at(self.i + 1) {
                self.i += 1;
                self.skip_spaces();
                return vec![Inline::LineBreak];
            }
            return self.newline();
        }
        vec![Inline::Space]
    }

    /// Tells whether text follows at `k`, so that a line break before `k`
    /// is not at the end. A window in brackets or in a native span ends
    /// before its closer, so text always follows it.
    fn text_at(&self, k: usize) -> bool {
        k < self.s.len() || self.window != Window::Text
    }

    fn skip_spaces(&mut self) {
        while matches!(self.cur(), Some(' ' | '\t')) {
            self.i += 1;
        }
    }

    fn newline(&mut self) -> Vec<Inline> {
        let before = self.prev();
        self.i += 1;
        self.skip_spaces();
        // A soft break at the end of a bracket is dropped; a native span
        // ends before its closing tag, so it keeps the break.
        if self.i >= self.s.len() && self.window != Window::Element {
            return Vec::new();
        }
        if self.ext(Extension::HardLineBreaks) {
            return vec![Inline::LineBreak];
        }
        if self.ext(Extension::IgnoreLineBreaks) {
            return Vec::new();
        }
        if self.ext(Extension::EastAsianLineBreaks)
            && before.is_some_and(is_east_asian)
            && self.cur().is_some_and(is_east_asian)
        {
            return vec![Inline::RawInline(Format::new(NOTHING), Text::new())];
        }
        vec![Inline::SoftBreak]
    }

    fn escapable(&self, c: char) -> bool {
        if self.ext(Extension::AllSymbolsEscapable) {
            !c.is_alphanumeric() && !c.is_whitespace() || c == ' '
        } else {
            "\\`*_{}[]()>#+-.!".contains(c)
                || (self.ext(Extension::AngleBracketsEscapable) && (c == '<' || c == '>'))
        }
    }

    fn escape(&mut self) -> Vec<Inline> {
        let next = self.peek(1);
        if self.ext(Extension::TexMathDoubleBackslash)
            && next == Some('\\')
            && matches!(self.peek(2), Some('(' | '['))
            && let Some(out) = self.backslash_math(2)
        {
            return out;
        }
        if self.ext(Extension::TexMathSingleBackslash)
            && matches!(next, Some('(' | '['))
            && let Some(out) = self.backslash_math(1)
        {
            return out;
        }
        match next {
            None if self.ext(Extension::EscapedLineBreaks) => {
                self.i += 1;
                vec![Inline::LineBreak]
            }
            Some('\n') if self.ext(Extension::EscapedLineBreaks) && self.text_at(self.i + 2) => {
                self.i += 2;
                self.skip_spaces();
                vec![Inline::LineBreak]
            }
            Some(' ') if self.ext(Extension::AllSymbolsEscapable) => {
                self.i += 2;
                vec![Inline::Str("\u{a0}".into())]
            }
            Some(c) if self.escapable(c) => {
                self.i += 2;
                vec![Inline::Str(c.to_string())]
            }
            Some(c) if c.is_ascii_alphabetic() && self.ext(Extension::RawTex) => {
                if let Some(out) = self.raw_tex() {
                    return out;
                }
                self.i += 1;
                vec![Inline::Str("\\".into())]
            }
            _ => {
                self.i += 1;
                vec![Inline::Str("\\".into())]
            }
        }
    }

    fn raw_tex(&mut self) -> Option<Vec<Inline>> {
        let start = self.i;
        let mut k = self.i + 1;
        while self.s.get(k).is_some_and(char::is_ascii_alphabetic) {
            k += 1;
        }
        let name = self.text(start + 1, k);
        if name == "begin" {
            if self.s.get(k) != Some(&'{') {
                return None;
            }
            let close = self.find(Find::Char('}'), k)?;
            let env = self.text(k + 1, close);
            let end = format!("\\end{{{env}}}");
            let pos = self.env_end(&env, close)?;
            let raw = self.text(start, pos + end.chars().count());
            self.i = start + raw.chars().count();
            return Some(vec![Inline::RawInline(Format::new("tex"), raw)]);
        }
        loop {
            match self.s.get(k) {
                Some('{') => {
                    let mut depth = 0usize;
                    let mut j = k;
                    let mut ok = false;
                    while let Some(&c) = self.s.get(j) {
                        if c == '{' {
                            depth += 1;
                        } else if c == '}' {
                            depth -= 1;
                            if depth == 0 {
                                ok = true;
                                break;
                            }
                        } else if c == '\\' {
                            j += 1;
                        }
                        j += 1;
                    }
                    if !ok {
                        break;
                    }
                    k = j + 1;
                }
                Some('[') if self.s.get(k + 1) != Some(&']') => {
                    let mut j = k;
                    while self.s.get(j).is_some_and(|c| *c != ']' && *c != '\n') {
                        j += 1;
                    }
                    if self.s.get(j) != Some(&']') {
                        break;
                    }
                    k = j + 1;
                }
                _ => break,
            }
        }
        self.i = k;
        Some(vec![Inline::RawInline(
            Format::new("tex"),
            self.text(start, k),
        )])
    }

    fn backslash_math(&mut self, slashes: usize) -> Option<Vec<Inline>> {
        let open = self.peek(slashes)?;
        let (close, kind) = if open == '(' {
            (')', MathType::InlineMath)
        } else {
            (']', MathType::DisplayMath)
        };
        let closer: Text = format!("{}{close}", "\\".repeat(slashes));
        let start = self.i + slashes + 1;
        let rest = self.text(start, self.s.len());
        let pos = rest.find(&closer)?;
        let body = rest.get(..pos)?.to_string();
        self.i = start + body.chars().count() + closer.chars().count();
        Some(vec![Inline::Math(
            kind,
            body.trim_matches(SPACE).to_string(),
        )])
    }

    /// Parses an emphasis opener. An emphasis writes its result into one
    /// buffer that nested emphasis shares, so that a failed attempt (a
    /// literal opener and its contents) is not copied again by each level
    /// around it. `direct` is true when the caller is an emphasis loop that
    /// reads the buffer; otherwise the result is taken out of the buffer.
    fn enclosure(&mut self, c: char, direct: bool) -> Vec<Inline> {
        if c == '_' && self.ext(Extension::IntrawordUnderscores) && is_alnum(self.prev()) {
            let start = self.i;
            while self.cur() == Some('_') {
                self.i += 1;
            }
            return vec![Inline::Str(self.text(start, self.i))];
        }
        let start = self.i;
        let mut n = 0;
        while self.cur() == Some(c) {
            n += 1;
            self.i += 1;
        }
        if matches!(self.cur(), None | Some(' ' | '\t')) || n > 3 {
            return vec![Inline::Str(self.text(start, self.i))];
        }
        let construct = Construct::Emphasis(c, n);
        // An emphasis that failed here before runs to the end of the text
        // again, and an active sequence then fails and discards the result.
        if self.closing > 0 && self.failed_before(start, construct) {
            self.p.emphasis_failures += 1;
            self.i = self.s.len();
            return Vec::new();
        }
        let note_number = self.p.note_number;
        let mark = self.buf.len();
        let closed = match n {
            1 => self.one(c, None),
            2 => self.two(c, None),
            _ => self.three(c),
        };
        let failed = !closed && self.i >= self.s.len();
        if failed {
            self.p.emphasis_failures += 1;
            if self.scripts == 0 {
                self.remember_failure(start, construct);
            }
        }
        // Inside a superscript or a subscript, an emphasis with no closer
        // is a literal run and does not take the closing delimiter.
        if self.scripts > 0 && failed {
            self.buf.truncate(mark);
            self.p.note_number = note_number;
            self.i = start + n;
            return vec![Inline::Str(self.text(start, start + n))];
        }
        if direct {
            Vec::new()
        } else {
            self.buf.drain(mark..).collect()
        }
    }

    /// Replaces the buffer from `mark` (the literal opener) with `make` of the contents.
    fn close_emphasis(&mut self, mark: usize, make: fn(Vec<Inline>) -> Inline) {
        let contents: Vec<Inline> = self.buf.drain(mark + 1..).collect();
        self.buf.truncate(mark);
        self.buf.push(make(merge(contents)));
    }

    fn one(&mut self, c: char, prefix: Option<Inline>) -> bool {
        let mark = self.buf.len();
        self.buf.push(Inline::Str(c.to_string()));
        self.buf.extend(prefix);
        loop {
            if self.i >= self.s.len() {
                return false;
            }
            if self.ender_at(self.i, c, 1) {
                if self.peek(1) == Some(c) && !self.ender_at(self.i + 2, c, 1) {
                    self.i += 2;
                    self.two(c, None);
                    continue;
                }
                self.i += 1;
                self.close_emphasis(mark, Inline::Emph);
                return true;
            }
            if self.at_limit() {
                self.literal_to_ender(c, 1);
                continue;
            }
            self.inline_into_buffer();
        }
    }

    fn two(&mut self, c: char, prefix: Option<Inline>) -> bool {
        let mark = self.buf.len();
        self.buf.push(Inline::Str(format!("{c}{c}")));
        self.buf.extend(prefix);
        loop {
            if self.ender_at(self.i, c, 2) {
                self.i += 2;
                self.close_emphasis(mark, Inline::Strong);
                return true;
            }
            if self.i >= self.s.len() {
                return false;
            }
            if self.at_limit() {
                self.literal_to_ender(c, 2);
                continue;
            }
            self.inline_into_buffer();
        }
    }

    fn three(&mut self, c: char) -> bool {
        let mark = self.buf.len();
        self.buf.push(Inline::Str(format!("{c}{c}{c}")));
        while self.i < self.s.len() && !self.ender_at(self.i, c, 1) {
            if self.at_limit() {
                self.literal_to_ender(c, 1);
                continue;
            }
            self.inline_into_buffer();
        }
        if self.ender_at(self.i, c, 3) {
            self.i += 3;
            self.close_emphasis(mark, |xs| Inline::Strong(vec![Inline::Emph(xs)]));
            return true;
        }
        if self.ender_at(self.i, c, 2) {
            self.i += 2;
            let contents: Vec<Inline> = self.buf.drain(mark + 1..).collect();
            self.buf.truncate(mark);
            return self.one(c, Some(Inline::Strong(contents)));
        }
        if self.ender_at(self.i, c, 1) {
            self.i += 1;
            let contents: Vec<Inline> = self.buf.drain(mark + 1..).collect();
            self.buf.truncate(mark);
            return self.two(c, Some(Inline::Emph(contents)));
        }
        false
    }

    fn code(&mut self) -> Vec<Inline> {
        let start = self.i;
        let mut run = 0;
        while self.s.get(start + run) == Some(&'`') {
            run += 1;
        }
        // An unmatched run gives one literal backtick, and the rest of the
        // run is tried again as a shorter opener.
        for skip in 0..run {
            let n = run - skip;
            let open = start + skip;
            let Some(k) = self.code_closer(open + n - 1, n) else {
                continue;
            };
            let body = self.text(open + n, k);
            self.i = k + n;
            let body: Text = body.split('\n').collect::<Vec<_>>().join(" ");
            let body = body.trim_matches(|c| c == ' ').to_string();
            let attr = self.trailing_attr(Extension::InlineCodeAttributes);
            let mut out = Vec::new();
            if skip > 0 {
                out.push(Inline::Str("`".repeat(skip)));
            }
            if let Some(format) = self.trailing_raw_attr() {
                out.push(Inline::RawInline(Format::new(format), body));
            } else {
                out.push(Inline::Code(attr.unwrap_or_default(), body));
            }
            return out;
        }
        self.i = start + run;
        vec![Inline::Str("`".repeat(run))]
    }

    fn trailing_raw_attr(&mut self) -> Option<Text> {
        if !self.ext(Extension::RawAttribute) || self.cur() != Some('{') {
            return None;
        }
        let mut k = self.i + 1;
        while self.s.get(k).is_some_and(|c| c.is_whitespace()) {
            k += 1;
        }
        if self.s.get(k) != Some(&'=') {
            return None;
        }
        let start = k + 1;
        k = start;
        while self
            .s
            .get(k)
            .is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        {
            k += 1;
        }
        let name = self.text(start, k);
        while self.s.get(k).is_some_and(|c| c.is_whitespace()) {
            k += 1;
        }
        if name.is_empty() || self.s.get(k) != Some(&'}') {
            return None;
        }
        self.i = k + 1;
        Some(name)
    }

    fn trailing_attr(&mut self, extension: Extension) -> Option<Attr> {
        if !self.ext(extension) || self.cur() != Some('{') {
            return None;
        }
        let (attr, end) = self.attrs_at(self.i)?;
        self.i = end;
        Some(attr)
    }

    /// Parses an attribute block `{#id .class key=value}` that starts at
    /// `from`, and returns it with the position after the `}`. The parser
    /// reads the text in place; a quoted value ends at the next unescaped
    /// quote, which the cached search finds.
    fn attrs_at(&mut self, from: usize) -> Option<(Attr, usize)> {
        if self.s.get(from) != Some(&'{') {
            return None;
        }
        let ident =
            |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.') || !c.is_ascii();
        let mut attr = Attr::default();
        let mut k = from + 1;
        loop {
            while self.s.get(k).is_some_and(|c| c.is_whitespace()) {
                k += 1;
            }
            let c = *self.s.get(k)?;
            match c {
                '}' => return Some((attr, k + 1)),
                '=' if k == from + 1 => return None,
                '#' | '.' => {
                    let start = k + 1;
                    k = start;
                    while self.s.get(k).is_some_and(|c| ident(*c)) {
                        k += 1;
                    }
                    if k == start {
                        return None;
                    }
                    let name = self.text(start, k);
                    if c == '#' {
                        attr.id = name;
                    } else {
                        attr.classes.push(name);
                    }
                }
                '-' if self
                    .s
                    .get(k + 1)
                    .is_some_and(|c| c.is_whitespace() || *c == '}') =>
                {
                    attr.classes.push("unnumbered".into());
                    k += 1;
                }
                _ => {
                    let start = k;
                    while self.s.get(k).is_some_and(|c| ident(*c) || *c == ':') {
                        k += 1;
                    }
                    if k == start || self.s.get(k) != Some(&'=') {
                        return None;
                    }
                    let key = self.text(start, k);
                    k += 1;
                    let value = match self.s.get(k).copied() {
                        Some(q @ ('"' | '\'')) => {
                            let end = self.find(Find::Unescaped(q), k + 1)?;
                            let mut v = Text::new();
                            let mut j = k + 1;
                            while j < end {
                                let ch = *self.s.get(j)?;
                                if ch == '\\' && j + 1 < end {
                                    v.push(*self.s.get(j + 1)?);
                                    j += 2;
                                } else {
                                    v.push(ch);
                                    j += 1;
                                }
                            }
                            k = end + 1;
                            v
                        }
                        _ => {
                            let vstart = k;
                            while self
                                .s
                                .get(k)
                                .is_some_and(|c| !c.is_whitespace() && *c != '}')
                            {
                                k += 1;
                            }
                            self.text(vstart, k)
                        }
                    };
                    match key.as_str() {
                        "id" => attr.id = value,
                        "class" => attr
                            .classes
                            .extend(value.split_whitespace().map(str::to_string)),
                        _ => attr.attributes.push((key, value)),
                    }
                }
            }
        }
    }

    /// Finds the `]` that matches the `[` at `open` and returns its index.
    fn matching_bracket(&mut self, open: usize) -> Option<usize> {
        if self.ix.tables.borrow().brackets.is_none() {
            let brackets = self.widened(Self::build_brackets);
            self.ix.tables.borrow_mut().brackets = Some(brackets);
        }
        let close = *self.ix.tables.borrow().brackets.as_ref()?.get(open)?;
        self.inside(close)
    }

    /// Returns the end (exclusive) of an attribute block that starts at `from`.
    /// Returns the first `\end{env}` after `from`. One pass collects the
    /// positions of each `\end{...}` by environment name.
    fn env_end(&mut self, env: &str, from: usize) -> Option<usize> {
        if self.ix.tables.borrow().env_ends.is_none() {
            let ends = self.widened(Self::env_ends);
            self.ix.tables.borrow_mut().env_ends = Some(ends);
        }
        let end = format!("\\end{{{env}}}").chars().count();
        let tables = self.ix.tables.borrow();
        let list = tables.env_ends.as_ref()?.get(env)?;
        let index = list.partition_point(|p| *p < from);
        list.get(index).copied().filter(|p| p + end <= self.s.hi)
    }

    fn env_ends(&mut self) -> HashMap<Text, Vec<usize>> {
        {
            let mut ends: HashMap<Text, Vec<usize>> = HashMap::new();
            let mut k = 0;
            while let Some(pos) = self.find_pattern("\\end{", k) {
                let name_start = pos + 5;
                let close = self.find(Find::Char('}'), name_start);
                if let Some(close) = close {
                    ends.entry(self.text(name_start, close))
                        .or_default()
                        .push(pos);
                }
                k = pos + 1;
            }
            ends
        }
    }

    /// Returns the `)` that matches the `(` at `open`. One pass with a stack
    /// matches all parentheses of the text; escaped ones do not count.
    fn paren_close(&mut self, open: usize) -> Option<usize> {
        if self.ix.tables.borrow().parens.is_none() {
            let parens = self.widened(Self::paren_matches);
            self.ix.tables.borrow_mut().parens = Some(parens);
        }
        let close = *self.ix.tables.borrow().parens.as_ref()?.get(open)?;
        self.inside(close)
    }

    fn paren_matches(&mut self) -> Vec<u32> {
        {
            let mut matches = vec![u32::MAX; self.s.len()];
            let mut stack: Vec<usize> = Vec::new();
            let mut k = 0;
            while let Some(&c) = self.s.get(k) {
                match c {
                    '\\' => k += 1,
                    '(' => stack.push(k),
                    ')' => {
                        if let Some(o) = stack.pop()
                            && let Some(slot) = matches.get_mut(o)
                        {
                            *slot = u32::try_from(k).unwrap_or(u32::MAX);
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            matches
        }
    }

    /// Returns `true` when an odd number of backslashes comes before `k`.
    fn escaped(&self, k: usize) -> bool {
        let mut n = 0;
        while k > n && self.s.get(k - n - 1) == Some(&'\\') {
            n += 1;
        }
        n % 2 == 1
    }

    fn bracket(&mut self, image: bool) -> Option<Vec<Inline>> {
        let open = if image { self.i + 1 } else { self.i };
        let close = self.matching_bracket(open)?;
        // Inside a link, a bracket can only be a note, a citation or a wiki
        // link. Without a copy of each label, nested brackets stay linear.
        if self.in_link
            && !image
            && !matches!(self.s.get(open + 1), Some('^' | '['))
            && self
                .find(Find::Char('@'), open + 1)
                .is_none_or(|at| at > close)
        {
            return None;
        }
        if !image && self.ext(Extension::Footnotes) && self.s.get(open + 1) == Some(&'^') {
            let key = self.text(open + 2, close);
            let key = key.as_str();
            if self.p.in_note {
                if self.p.notes.contains_key(key) {
                    self.p.used_notes.push(key.to_string());
                }
                return None;
            }
            if let Some(blocks) = self.p.notes.get(key).cloned() {
                self.i = close + 1;
                self.p.used_notes.push(key.to_string());
                return Some(vec![self.note(blocks)]);
            }
            // A reference to a note with no definition is text, and it
            // takes a note number.
            if !key.is_empty()
                && !key.contains(|c: char| c.is_whitespace() || matches!(c, '^' | '[' | ']'))
            {
                let text = format!("[^{key}]");
                self.i = close + 1;
                self.p.note_number += 1;
                return Some(vec![Inline::Str(text)]);
            }
        }
        let after = self.s.get(close + 1).copied();
        if after == Some('(')
            && let Some(out) = self.inline_link(image, open, close)
        {
            return Some(out);
        }
        if !image
            && after == Some('{')
            && self.ext(Extension::BracketedSpans)
            && !self.in_link
            && let Some((attr, end)) = self.attrs_at(close + 1)
        {
            let inner = self.sub_window(open + 1..close, false, Window::Content);
            self.i = end;
            return Some(vec![make_span(attr, inner)]);
        }
        if (self.ext(Extension::WikilinksTitleAfterPipe)
            || self.ext(Extension::WikilinksTitleBeforePipe))
            && !image
            && close > open + 2
            && self.s.get(open + 1) == Some(&'[')
            && self.s.get(close - 1) == Some(&']')
        {
            let inner = self.text(open + 2, close - 1);
            let (url, title) = match inner.split_once('|') {
                Some((a, b)) if self.ext(Extension::WikilinksTitleAfterPipe) => {
                    (a.to_string(), b.to_string())
                }
                Some((a, b)) => (b.to_string(), a.to_string()),
                None => (inner.clone(), inner.clone()),
            };
            self.i = close + 1;
            let text = self.sub(title.trim_matches(SPACE), true);
            let attr = Attr::new("", vec!["wikilink".into()], vec![]);
            return Some(vec![Inline::Link(
                attr,
                text,
                Target::new(escape_uri(url.trim_matches(SPACE)), ""),
            )]);
        }
        let label = self.text(open + 1, close);
        if let Some(out) = self.reference_link(image, open, close, &label) {
            return Some(out);
        }
        if !image
            && self.ext(Extension::Citations)
            && label.contains('@')
            && let Some(cites) = self.citations(&label)
        {
            let raw = self.text(self.i, close + 1);
            self.i = close + 1;
            return Some(vec![Inline::Cite(cites, raw_content(&raw))]);
        }
        if image || self.in_link {
            return None;
        }
        // A bracket that is not a link keeps its brackets, and its content
        // is parsed on its own, so that a delimiter inside cannot close an
        // emphasis outside. The label of a reference link with no definition
        // is text in the same way, so that `[a][b](u)` has no link.
        let mut out = self.bracket_text(open + 1..close);
        let mut last = close;
        let second = self.second_label(close);
        if let Some((start, label_end)) = second {
            if start > close + 1 && !self.text(close + 1, start).contains('\n') {
                out.push(Inline::Space);
            }
            out.extend(self.bracket_text(start + 1..label_end));
            last = label_end;
        }
        // With link attributes, a reference link with no definition keeps
        // its text and loses its attributes; a single bracket only when
        // bracketed spans are off.
        let end = if self.s.get(last + 1) == Some(&'{')
            && self.ext(Extension::LinkAttributes)
            && (second.is_some() || !self.ext(Extension::BracketedSpans))
        {
            self.attrs_at(last + 1).map(|(_, end)| end)
        } else {
            None
        };
        self.i = end.unwrap_or(last + 1);
        Some(merge(out))
    }

    /// Parses the window `range` as the text of a bracket that is no link,
    /// with the brackets around it.
    fn bracket_text(&mut self, range: Range<usize>) -> Vec<Inline> {
        let mut out = vec![Inline::Str("[".into())];
        let window = self.window(range);
        let ix = self.shared_index();
        let mut inner = Inl::new(&mut *self.p, window, ix, false, self.depth);
        inner.window = Window::Content;
        out.extend(inner.seq(Stop::None));
        out.push(Inline::Str("]".into()));
        out
    }

    fn window(&self, range: Range<usize>) -> Chars {
        Chars {
            all: Rc::clone(&self.s.all),
            lo: range.start,
            hi: range.end,
        }
    }

    fn shared_index(&self) -> Index {
        Index {
            tables: Rc::clone(&self.ix.tables),
            memo: HashMap::new(),
        }
    }

    /// Parses the window `range` of this buffer as a separate text.
    fn sub_window(&mut self, range: Range<usize>, in_link: bool, kind: Window) -> Vec<Inline> {
        crate::step(1);
        let saved = self.p.depth;
        self.p.depth = self.depth;
        let window = self.window(range);
        let ix = self.shared_index();
        let out = self
            .p
            .parse_window(window, ix, in_link || self.in_link, kind);
        self.p.depth = saved;
        out
    }

    fn sub(&mut self, text: &str, in_link: bool) -> Vec<Inline> {
        let saved = self.p.depth;
        self.p.depth = self.depth;
        let out = self.p.parse_inlines_in(text, in_link || self.in_link);
        self.p.depth = saved;
        out
    }

    fn note(&mut self, blocks: Vec<Block>) -> Inline {
        self.p.note_number += 1;
        let saved_depth = self.p.depth;
        self.p.depth = self.depth;
        let saved = std::mem::replace(&mut self.p.in_note, true);
        let blocks = self.p.resolve_blocks(blocks);
        self.p.in_note = saved;
        self.p.depth = saved_depth;
        Inline::Note(blocks)
    }

    fn make_link(
        &mut self,
        image: bool,
        label: Range<usize>,
        target: Target,
        attr: Attr,
    ) -> Inline {
        if image {
            let alt = self.sub_window(label, false, Window::Label);
            Inline::Image(attr, alt, target)
        } else {
            let text = self.sub_window(label, true, Window::Label);
            Inline::Link(attr, text, target)
        }
    }

    fn inline_link(&mut self, image: bool, open: usize, close: usize) -> Option<Vec<Inline>> {
        if self.in_link && !image {
            return None;
        }
        if self.s.get(close + 2) != Some(&'<') && self.paren_close(close + 1).is_none() {
            return None;
        }
        let mut k = close + 2;
        while matches!(self.s.get(k), Some(' ' | '\n')) {
            k += 1;
        }
        let url_start = k;
        let url: Text;
        if self.s.get(k) == Some(&'<') {
            let mut j = k + 1;
            while let Some(&c) = self.s.get(j) {
                if c == '>' || c == '\n' {
                    break;
                }
                j += 1;
            }
            if self.s.get(j) != Some(&'>') {
                return None;
            }
            url = self.text(url_start + 1, j);
            k = j + 1;
        } else {
            let mut depth = 0usize;
            loop {
                match self.s.get(k) {
                    None => return None,
                    Some('\\') => k += 2,
                    Some('(') => {
                        depth += 1;
                        k += 1;
                    }
                    Some(')') if depth == 0 => break,
                    Some(')') => {
                        depth -= 1;
                        k += 1;
                    }
                    Some(' ' | '\n') => {
                        let mut m = k;
                        while matches!(self.s.get(m), Some(' ' | '\n')) {
                            m += 1;
                        }
                        if matches!(self.s.get(m), Some('"' | '\'' | ')')) {
                            break;
                        }
                        k = m;
                    }
                    Some(_) => k += 1,
                }
            }
            url = self.text(url_start, k.min(self.s.len()));
        }
        while matches!(self.s.get(k), Some(' ' | '\n')) {
            k += 1;
        }
        let mut title = Text::new();
        if matches!(self.s.get(k), Some('"' | '\'')) {
            let q = self.s.get(k).copied().unwrap_or('"');
            let close_q = q;
            let mut j = k + 1;
            let mut found = None;
            while let Some(&c) = self.s.get(j) {
                if c == '\\' {
                    j += 2;
                    continue;
                }
                if c == close_q {
                    let mut m = j + 1;
                    while matches!(self.s.get(m), Some(' ' | '\n')) {
                        m += 1;
                    }
                    if self.s.get(m) == Some(&')') {
                        found = Some((j, m));
                        break;
                    }
                }
                j += 1;
            }
            let (end, paren) = found?;
            title = decode_entities(&unescape(&self.text(k + 1, end)));
            k = paren;
        }
        if self.s.get(k) != Some(&')') {
            return None;
        }
        self.i = k + 1;
        let attr = self
            .trailing_attr(Extension::LinkAttributes)
            .unwrap_or_default();
        let url = escape_uri(&decode_entities(&unescape(&url.replace('\n', " "))));
        let target = Target::new(url, title);
        // In Pandoc, the text of an inline link does not advance the note
        // number and does not mark a note as used.
        let note_number = self.p.note_number;
        let used_notes = self.p.used_notes.len();
        let link = self.make_link(image, open + 1..close, target, attr);
        if !image {
            self.p.note_number = note_number;
            self.p.used_notes.truncate(used_notes);
        }
        Some(vec![link])
    }

    fn reference_link(
        &mut self,
        image: bool,
        open: usize,
        close: usize,
        label: &str,
    ) -> Option<Vec<Inline>> {
        if self.in_link && !image {
            return None;
        }
        let mut key_text = label.to_string();
        let mut explicit = false;
        let mut k = close + 1;
        if let Some((start, end)) = self.second_label(close) {
            let second = self.text(start + 1, end);
            if !second.is_empty() {
                key_text = second;
            }
            explicit = true;
            k = end + 1;
        }
        if !explicit && !self.ext(Extension::ShortcutReferenceLinks) {
            return None;
        }
        let key = normalize_key(&key_text);
        let found = self.p.refs.get(&key).cloned().or_else(|| {
            self.p
                .header_refs
                .get(&key)
                .map(|t| (t.clone(), Attr::default()))
        });
        let (target, attr) = found?;
        self.i = k;
        Some(vec![self.make_link(image, open + 1..close, target, attr)])
    }

    /// Returns the brackets of the label of a reference link after the
    /// text that ends at `close`: `[text][label]`, or with spaced reference
    /// links `[text] [label]` and one line break at most.
    fn second_label(&mut self, close: usize) -> Option<(usize, usize)> {
        let mut k = close + 1;
        if self.ext(Extension::SpacedReferenceLinks) {
            let mut j = k;
            let mut newlines = 0;
            while matches!(self.s.get(j), Some(' ' | '\n')) {
                if self.s.get(j) == Some(&'\n') {
                    newlines += 1;
                }
                j += 1;
            }
            if newlines <= 1 && self.s.get(j) == Some(&'[') && j > k {
                k = j;
            }
        }
        if self.s.get(k) != Some(&'[') {
            return None;
        }
        Some((k, self.matching_bracket(k)?))
    }

    fn cite_note_num(&mut self) -> i64 {
        if !self.p.in_note {
            self.p.note_number += 1;
        }
        self.p.note_number
    }

    fn citations(&mut self, label: &str) -> Option<Vec<Citation>> {
        let mut parts = Vec::new();
        for part in split_citations(label) {
            parts.push(find_cite_key(part)?);
        }
        let saved_note_number = self.p.note_number;
        let note_num = self.cite_note_num();
        let _ = parts;
        let failures = self.p.emphasis_failures;
        let mut cites = Vec::new();
        for part in split_citations(label) {
            let at = find_cite_key(part)?;
            let (prefix_text, rest) = part.split_at(at.0);
            let mut prefix_text = prefix_text.to_string();
            let mode = if prefix_text.ends_with('-') {
                prefix_text.pop();
                CitationMode::SuppressAuthor
            } else {
                CitationMode::NormalCitation
            };
            let key = at.1.clone();
            let suffix_text = rest.get(at.2..).unwrap_or("");
            let prefix = trim_inlines(self.sub(prefix_text.trim_start_matches(SPACE), false));
            let suffix = self.suffix(suffix_text);
            // Pandoc parses the bracket content as one text, so an emphasis
            // opener that finds no closer takes the closing bracket.
            for text in [prefix_text.as_str(), suffix_text] {
                if text.contains(['*', '_']) {
                    let _ = self.sub(&format!("{text}]"), false);
                }
            }
            cites.push(Citation {
                id: key,
                prefix,
                suffix,
                mode,
                note_num,
                hash: 0,
            });
        }
        // An emphasis with no closer in a prefix or a suffix would take
        // the closing bracket, so the text is not a citation.
        if self.p.emphasis_failures != failures {
            self.p.note_number = saved_note_number;
            return None;
        }
        Some(cites)
    }

    fn suffix(&mut self, text: &str) -> Vec<Inline> {
        let t = text.trim_end_matches(SPACE);
        if t.is_empty() {
            return Vec::new();
        }
        let mut out = self.sub(t, false);
        if t.starts_with([' ', '\n']) && !matches!(out.first(), Some(Inline::Space)) {
            out.insert(0, Inline::Space);
        }
        out
    }

    fn at_sign(&mut self) -> Option<Vec<Inline>> {
        if is_alnum(self.prev()) {
            return None;
        }
        // The key ends at the first character that cannot be in a key.
        let end =
            if self.peek(1) == Some('{') {
                self.find(Find::Char('}'), self.i + 1)
                    .map_or(self.s.len(), |k| k + 1)
            } else {
                let mut k = self.i + 1;
                while self.s.get(k).is_some_and(|c| {
                    c.is_alphanumeric() || *c == '_' || ":.#$%&-+?<>~/".contains(*c)
                }) {
                    k += 1;
                }
                k
            };
        let rest = self.text(self.i, end);
        let (key, len) = cite_key(&rest)?;
        let len = rest.get(..len)?.chars().count();
        if self.ext(Extension::ExampleLists)
            && let Some(n) = self.p.examples.get(&key).copied()
        {
            self.i += len;
            return Some(vec![Inline::Str(n.to_string())]);
        }
        if !self.ext(Extension::Citations) {
            return None;
        }
        let suppress = false;
        let start = self.i;
        self.i += len;
        let note_num = self.cite_note_num();
        let mut more: Vec<Citation> = Vec::new();
        let mut suffix = Vec::new();
        let mut k = self.i;
        while self.s.get(k) == Some(&' ') {
            k += 1;
        }
        if self.s.get(k) == Some(&'[')
            && let Some(end) = self.matching_bracket(k)
        {
            let inner = self.text(k + 1, end);
            if inner.contains('@') && find_cite_key(&inner).is_some() {
                let after = self.s.get(end + 1).copied();
                if after != Some('(') && after != Some('{') {
                    let saved = self.p.note_number;
                    if !self.p.in_note {
                        self.p.note_number = note_num - 1;
                    }
                    if let Some(cites) = self.citations(&inner) {
                        more = cites;
                        self.i = end + 1;
                    }
                    self.p.note_number = saved.max(note_num);
                }
            } else if !inner.starts_with('^') {
                // A note reference after the key is not a suffix.
                let after = self.s.get(end + 1).copied();
                if after != Some('(') && after != Some('[') && after != Some('{') {
                    suffix = self.suffix(inner.trim_matches(SPACE));
                    self.i = end + 1;
                }
            }
        }
        let raw = self.text(start, self.i);
        let cite = Citation {
            id: key,
            prefix: Vec::new(),
            suffix,
            mode: if suppress {
                CitationMode::SuppressAuthor
            } else {
                CitationMode::AuthorInText
            },
            note_num,
            hash: 0,
        };
        let mut all = vec![cite];
        all.extend(more);
        Some(vec![Inline::Cite(all, raw_content(&raw))])
    }

    fn suppress_cite(&mut self) -> Option<Vec<Inline>> {
        let start = self.i;
        self.i += 1;
        let mut out = match self.at_sign() {
            Some(out) => out,
            None => {
                self.i = start;
                return None;
            }
        };
        // The citation text leaves out the `-`.
        if let Some(Inline::Cite(cites, _)) = out.first_mut()
            && let Some(first) = cites.first_mut()
        {
            first.mode = CitationMode::SuppressAuthor;
        }
        Some(out)
    }

    fn caret(&mut self) -> Option<Vec<Inline>> {
        if self.peek(1) == Some('[') && self.ext(Extension::InlineNotes) {
            let close = self.matching_bracket(self.i + 1)?;
            let start = self.i + 2;
            self.i = close + 1;
            self.p.note_number += 1;
            let saved_in_note = std::mem::replace(&mut self.p.in_note, true);
            let inner = self.sub_window(start..close, false, Window::Content);
            self.p.in_note = saved_in_note;
            return Some(vec![Inline::Note(vec![Block::Para(inner)])]);
        }
        if self.ext(Extension::Superscript)
            && let Some(out) = self.delimited(1, Stop::Superscript, Inline::Superscript)
        {
            return Some(out);
        }
        self.short_script(Inline::Superscript)
    }

    fn delimited(
        &mut self,
        n: usize,
        stop: Stop,
        make: fn(Vec<Inline>) -> Inline,
    ) -> Option<Vec<Inline>> {
        let start = self.i;
        let closer = match stop {
            Stop::Superscript => Find::Char('^'),
            Stop::Subscript => Find::Char('~'),
            Stop::Strikeout => Find::DoubleTilde,
            _ => Find::DoubleEquals,
        };
        if self.failed_before(start, Construct::Opener(stop))
            || self.find(closer, start + n).is_none()
        {
            return None;
        }
        self.i += n;
        if is_space(self.cur()) {
            self.i = start;
            return None;
        }
        let script = matches!(stop, Stop::Superscript | Stop::Subscript);
        self.scripts += usize::from(script);
        let contents = self.seq_closing(stop);
        self.scripts -= usize::from(script);
        if let Some(contents) = contents {
            self.i += n;
            return Some(vec![make(merge(contents))]);
        }
        self.i = start;
        self.remember_failure(start, Construct::Opener(stop));
        None
    }

    fn tilde(&mut self) -> Option<Vec<Inline>> {
        if self.starts_with("~~")
            && self.ext(Extension::Strikeout)
            && self.peek(2) != Some('~')
            && let Some(out) = self.delimited(2, Stop::Strikeout, Inline::Strikeout)
        {
            return Some(out);
        }
        if self.ext(Extension::Subscript)
            && let Some(out) = self.delimited(1, Stop::Subscript, Inline::Subscript)
        {
            return Some(out);
        }
        self.short_script(Inline::Subscript)
    }

    fn short_script(&mut self, make: fn(Vec<Inline>) -> Inline) -> Option<Vec<Inline>> {
        if !self.ext(Extension::ShortSubsuperscripts) {
            return None;
        }
        let start = self.i + 1;
        let mut k = start;
        while self.s.get(k).is_some_and(|c| c.is_alphanumeric()) {
            k += 1;
        }
        if k == start {
            return None;
        }
        let text = self.text(start, k);
        self.i = k;
        Some(vec![make(vec![Inline::Str(text)])])
    }

    fn mark(&mut self) -> Option<Vec<Inline>> {
        self.delimited(2, Stop::Mark, |xs| {
            Inline::Span(Attr::new("", vec!["mark".into()], vec![]), xs)
        })
    }

    fn angle(&mut self) -> Option<Vec<Inline>> {
        let rest = self.tag_window(self.i);
        if let Some(len) = autolink_len(&rest) {
            let inner = decode_entities(rest.get(1..len - 1)?);
            self.i += rest.get(..len)?.chars().count();
            let (class, url) = if inner.contains(':') && !is_email(&inner) {
                ("uri", escape_uri(&inner))
            } else {
                ("email", format!("mailto:{}", escape_uri(&inner)))
            };
            let mut attr = Attr::new("", vec![class.into()], vec![]);
            if let Some(extra) = self.trailing_attr(Extension::LinkAttributes) {
                attr = extra;
            }
            return Some(vec![Inline::Link(
                attr,
                vec![Inline::Str(inner)],
                Target::new(url, ""),
            )]);
        }
        if !self.ext(Extension::RawHtml) && !self.ext(Extension::NativeSpans) {
            return None;
        }
        if self.window != Window::Text && self.p.ends_inline_text(&rest) {
            self.s.hi = self.i;
            self.p.block_tag_cuts += 1;
            return Some(Vec::new());
        }
        let len = html_tag_len(&rest)?;
        let tag = rest.get(..len)?.to_string();
        if self.ext(Extension::NativeSpans)
            && tag.to_ascii_lowercase().starts_with("<span")
            && !tag.ends_with("/>")
        {
            let tag_end = self.i + tag.chars().count();
            if let Some(close) = self.span_close(self.i) {
                let close_end = self
                    .find(Find::Char('>'), close)
                    .map_or(self.s.len(), |k| k + 1);
                let failures = (self.p.emphasis_failures, self.p.block_tag_cuts);
                let content = self.sub_window(tag_end..close, false, Window::Element);
                // An emphasis with no closer would take the closing tag, and
                // a block tag ends the content before it, so the tags stay
                // raw HTML.
                if (self.p.emphasis_failures, self.p.block_tag_cuts) == failures {
                    let attr = html_attr(&tag);
                    self.i = close_end;
                    return Some(vec![make_span(attr, content)]);
                }
            }
        }
        if !self.ext(Extension::RawHtml) {
            return None;
        }
        self.i += tag.chars().count();
        Some(vec![Inline::RawInline(Format::new("html"), tag)])
    }

    fn dollar_math(&mut self) -> Option<Vec<Inline>> {
        if self.starts_with("$$") {
            let start = self.i + 2;
            let k = self.find(Find::DoubleDollar, start)?;
            let body = self.text(start, k);
            if body.trim_matches(SPACE).is_empty() {
                return None;
            }
            self.i = k + 2;
            return Some(vec![Inline::Math(
                MathType::DisplayMath,
                body.trim_matches(SPACE).to_string(),
            )]);
        }
        if is_space(self.peek(1)) {
            return None;
        }
        let start = self.i + 1;
        // The next unescaped `$` closes the math or ends the attempt.
        let k = self.find(Find::Dollar, start)?;
        if self.find(Find::BlankLine, start).is_some_and(|b| b < k) {
            return None;
        }
        let before = k.checked_sub(1).and_then(|j| self.s.get(j)).copied();
        if k == start || is_space(before) || self.s.get(k + 1).is_some_and(char::is_ascii_digit) {
            return None;
        }
        let body = self.text(start, k);
        self.i = k + 1;
        Some(vec![Inline::Math(MathType::InlineMath, body)])
    }

    fn entity(&mut self) -> Option<Vec<Inline>> {
        let rest = self.text(self.i, (self.i + 40).min(self.s.len()));
        let (text, len) = entity_at(&rest)?;
        self.i += len;
        Some(vec![Inline::Str(text)])
    }

    fn double_quoted(&mut self) -> Option<Vec<Inline>> {
        if is_space(self.peek(1)) {
            return None;
        }
        let start = self.i;
        if self.failed_before(start, Construct::Opener(Stop::DoubleQuote))
            || self.find(Find::Char('"'), start + 1).is_none()
        {
            self.i = start + 1;
            return Some(vec![Inline::Str("\u{201c}".into())]);
        }
        if self.quotes & 2 != 0 {
            return None;
        }
        self.i += 1;
        self.quotes |= 2;
        let contents = self.seq_closing(Stop::DoubleQuote);
        self.quotes &= !2;
        if let Some(contents) = contents {
            // An empty quote is not a quote: the opener is a literal mark,
            // and the next `"` is read again.
            if contents.is_empty() {
                self.i = start + 1;
                return Some(vec![Inline::Str("\u{201c}".into())]);
            }
            self.i += 1;
            return Some(vec![Inline::Quoted(
                QuoteType::DoubleQuote,
                merge(contents),
            )]);
        }
        self.remember_failure(start, Construct::Opener(Stop::DoubleQuote));
        self.i = start + 1;
        Some(vec![Inline::Str("\u{201c}".into())])
    }

    fn single_quote(&mut self) -> Vec<Inline> {
        let start = self.i;
        if !is_alnum(self.prev())
            && !is_space(self.peek(1))
            && !self.peek(1).is_some_and(|c| ");-?".contains(c))
            && !self.failed_before(start, Construct::Opener(Stop::SingleQuote))
            && self.quotes & 1 == 0
            && self.find(Find::QuoteCloser, start + 1).is_some()
        {
            self.i += 1;
            // A single quote cannot open inside a single quote.
            self.quotes |= 1;
            let contents = self.seq_closing(Stop::SingleQuote);
            self.quotes &= !1;
            if let Some(contents) = contents
                && !contents.is_empty()
            {
                self.i += 1;
                return vec![Inline::Quoted(QuoteType::SingleQuote, merge(contents))];
            }
            self.remember_failure(start, Construct::Opener(Stop::SingleQuote));
            self.i = start;
        }
        self.i = start + 1;
        vec![Inline::Str("\u{2019}".into())]
    }

    fn dash(&mut self) -> Option<Vec<Inline>> {
        if self.ext(Extension::OldDashes) {
            if self.starts_with("--") {
                self.i += 2;
                return Some(vec![Inline::Str("\u{2014}".into())]);
            }
            if self.peek(1).is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
                return Some(vec![Inline::Str("\u{2013}".into())]);
            }
            return None;
        }
        if self.starts_with("---") {
            self.i += 3;
            return Some(vec![Inline::Str("\u{2014}".into())]);
        }
        if self.starts_with("--") {
            self.i += 2;
            return Some(vec![Inline::Str("\u{2013}".into())]);
        }
        None
    }

    fn emoji(&mut self) -> Option<Vec<Inline>> {
        let mut k = self.i + 1;
        while self
            .s
            .get(k)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '+' || *c == '-')
        {
            k += 1;
        }
        if self.s.get(k) != Some(&':') || k == self.i + 1 {
            return None;
        }
        let name = self.text(self.i + 1, k);
        let symbol = randoc_core::shared::emoji_from_alias(&name)?;
        self.i = k + 1;
        let attr = Attr::new("", vec!["emoji".into()], vec![("data-emoji".into(), name)]);
        Some(vec![Inline::Span(attr, vec![Inline::Str(symbol.into())])])
    }

    fn bare_uri(&mut self) -> Option<Vec<Inline>> {
        if !self.ext(Extension::AutolinkBareUris) || is_alnum(self.prev()) {
            return None;
        }
        // A URI starts with a scheme and an e-mail address with a user name.
        // Both start with a letter or a digit, so other characters fail at once.
        if !is_alnum(self.cur()) {
            return None;
        }
        let space = self.find(Find::Space, self.i).unwrap_or(self.s.len());
        let has_at = self
            .find(Find::Char('@'), self.i)
            .is_some_and(|k| k < space);
        let mut scheme_end = self.i;
        while scheme_end < space
            && scheme_end - self.i < 12
            && self
                .s
                .get(scheme_end)
                .is_some_and(char::is_ascii_alphabetic)
        {
            scheme_end += 1;
        }
        let has_scheme = self.s.get(scheme_end) == Some(&':');
        if !has_at && !has_scheme {
            return None;
        }
        let mut end = self.i;
        while self
            .s
            .get(end)
            .is_some_and(|c| !c.is_whitespace() && *c != '<' && *c != '>')
        {
            end += 1;
        }
        let rest = self.text(self.i, end);
        if let Some(len) = bare_email_len(&rest) {
            let email = rest.get(..len)?.to_string();
            self.i += email.chars().count();
            let attr = Attr::new("", vec!["email".into()], vec![]);
            let target = Target::new(format!("mailto:{email}"), "");
            return Some(vec![Inline::Link(attr, vec![Inline::Str(email)], target)]);
        }
        let scheme_len = rest.find(':')?;
        let scheme = rest.get(..scheme_len)?;
        if !matches!(
            scheme.to_ascii_lowercase().as_str(),
            "http" | "https" | "ftp" | "mailto" | "file" | "data" | "irc" | "tel"
        ) {
            return None;
        }
        let mut end = rest
            .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
            .unwrap_or(rest.len());
        let url_part = rest.get(..end)?;
        let mut trimmed = url_part.trim_end_matches(['.', ',', ';', ':', '!', '?', '"', '\'']);
        while trimmed.ends_with(')') && trimmed.matches(')').count() > trimmed.matches('(').count()
        {
            trimmed = trimmed.get(..trimmed.len() - 1).unwrap_or("");
            trimmed = trimmed.trim_end_matches(['.', ',', ';', ':', '!', '?', '"', '\'']);
        }
        end = trimmed.len();
        if end <= scheme_len + 1 {
            return None;
        }
        let url = rest.get(..end)?.to_string();
        self.i += url.chars().count();
        let attr = Attr::new("", vec!["uri".into()], vec![]);
        Some(vec![Inline::Link(
            attr,
            vec![Inline::Str(url.clone())],
            Target::new(escape_uri(&url), ""),
        )])
    }
}

/// Makes a span, or `Underline` or `SmallCaps` for the classes that Pandoc maps to them.
fn make_span(attr: Attr, inner: Vec<Inline>) -> Inline {
    if attr.id.is_empty() && attr.attributes.is_empty() {
        match attr.classes.as_slice() {
            [c] if c == "underline" || c == "ul" => return Inline::Underline(inner),
            [c] if c == "smallcaps" => return Inline::SmallCaps(inner),
            _ => {}
        }
    }
    if attr.id.is_empty()
        && attr.classes.is_empty()
        && let [(k, v)] = attr.attributes.as_slice()
        && k == "style"
        && v.replace(' ', "").trim_end_matches(';') == "font-variant:small-caps"
    {
        return Inline::SmallCaps(inner);
    }
    Inline::Span(attr, inner)
}

/// Splits the text of a bracketed citation at each `;` that does not end an
/// HTML character reference.
fn split_citations(label: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    for (i, c) in label.char_indices() {
        if c != ';' {
            continue;
        }
        let before = label.get(start..i).unwrap_or("");
        let is_entity = before.rfind('&').is_some_and(|amp| {
            let name = before.get(amp + 1..).unwrap_or("");
            !name.is_empty()
                && name.len() <= 32
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '#')
                && entity_at(&format!("&{name};")).is_some()
        });
        if !is_entity {
            parts.push(before);
            start = i + 1;
        }
    }
    parts.push(label.get(start..).unwrap_or(""));
    parts
}

fn bare_email_len(s: &str) -> Option<usize> {
    let at = s.find('@')?;
    let user = s.get(..at)?;
    if user.is_empty()
        || !user
            .chars()
            .all(|c| c.is_alphanumeric() || "._%+-".contains(c))
    {
        return None;
    }
    let domain_part = s.get(at + 1..)?;
    let end = domain_part
        .find(|c: char| !(c.is_alphanumeric() || c == '.' || c == '-'))
        .unwrap_or(domain_part.len());
    let domain = domain_part.get(..end)?.trim_end_matches(['.', '-']);
    if !domain.contains('.') || domain.starts_with('.') {
        return None;
    }
    Some(at + 1 + domain.len())
}

fn is_email(s: &str) -> bool {
    let Some((user, domain)) = s.split_once('@') else {
        return false;
    };
    !user.is_empty()
        && !domain.is_empty()
        && !user.contains(':')
        && user
            .chars()
            .all(|c| c.is_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-".contains(c))
        && domain
            .chars()
            .all(|c| c.is_alphanumeric() || c == '.' || c == '-')
}

/// Returns the byte length of an automatic link `<...>` at the start of `s`.
fn autolink_len(s: &str) -> Option<usize> {
    let inner = s.strip_prefix('<')?;
    let end = inner.find(|c: char| c == '>' || c == '<' || c.is_whitespace())?;
    if !inner.get(end..)?.starts_with('>') {
        return None;
    }
    let body = inner.get(..end)?;
    if is_email(body) {
        return Some(end + 2);
    }
    let colon = body.find(':')?;
    let scheme = body.get(..colon)?;
    if scheme.len() < 2
        || !scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
        || body.len() == colon + 1
    {
        return None;
    }
    Some(end + 2)
}

/// Returns the citation key at the start of `s` (which starts with `@`) and the byte length used.
fn cite_key(s: &str) -> Option<(Text, usize)> {
    let rest = s.strip_prefix('@')?;
    if let Some(inner) = rest.strip_prefix('{') {
        let close = inner.find('}')?;
        let key = inner.get(..close)?;
        if key.is_empty() || key.contains(char::is_whitespace) {
            return None;
        }
        return Some((key.to_string(), close + 3));
    }
    let chars: Vec<char> = rest.chars().collect();
    let first = *chars.first()?;
    if !(first.is_alphanumeric() || first == '_') {
        return None;
    }
    let mut k = 1;
    while let Some(&c) = chars.get(k) {
        let inner_punct = ":.#$%&-+?<>~/".contains(c)
            && chars
                .get(k + 1)
                .is_some_and(|d| d.is_alphanumeric() || *d == '_');
        if c.is_alphanumeric() || c == '_' || inner_punct {
            k += 1;
        } else {
            break;
        }
    }
    let key: Text = chars.get(..k)?.iter().collect();
    let len = key.len() + 1;
    Some((key, len))
}

/// Finds the citation key in one part of a bracketed citation.
/// Returns the byte index of `@` (or of `-@`), the key and the byte length from that index to the end of the key.
fn find_cite_key(part: &str) -> Option<(usize, Text, usize)> {
    let mut prev: Option<char> = None;
    for (i, c) in part.char_indices() {
        if c == '@' && !is_alnum(prev) {
            let (key, len) = cite_key(part.get(i..)?)?;
            return Some((i, key, len));
        }
        prev = Some(c);
    }
    None
}

/// Splits raw citation text into `Str` and `Space` inlines.
fn raw_content(raw: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut word = Text::new();
    let mut gap: Option<Inline> = None;
    for c in raw.chars() {
        if c == ' ' || c == '\n' {
            if !word.is_empty() {
                if let Some(g) = gap.take() {
                    out.push(g);
                }
                out.push(Inline::Str(std::mem::take(&mut word)));
                gap = Some(Inline::Space);
            }
            if c == '\n' && gap.is_some() {
                gap = Some(Inline::SoftBreak);
            }
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        if let Some(g) = gap.take() {
            out.push(g);
        }
        out.push(Inline::Str(word));
    }
    out
}

fn trim_inlines(mut xs: Vec<Inline>) -> Vec<Inline> {
    while matches!(xs.first(), Some(Inline::Space | Inline::SoftBreak)) {
        xs.remove(0);
    }
    while matches!(xs.last(), Some(Inline::Space | Inline::SoftBreak)) {
        xs.pop();
    }
    xs
}

fn is_east_asian(c: char) -> bool {
    matches!(c as u32, 0x1100..=0x11FF | 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x2FFFF)
}

/// Decodes the HTML character reference at the start of `s` and returns its text and char length.
fn entity_at(s: &str) -> Option<(Text, usize)> {
    let rest = s.strip_prefix('&')?;
    let semi = rest.find(';')?;
    let name = rest.get(..semi)?;
    let text = if let Some(num) = name.strip_prefix('#') {
        let (digits, hex) = match num.strip_prefix(['x', 'X']) {
            Some(hex) => (hex, true),
            None => (num, false),
        };
        randoc_core::shared::numeric_entity(digits, hex)?.to_string()
    } else {
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        randoc_core::shared::named_entity(&format!("{name};"))?.to_string()
    };
    Some((text, name.chars().count() + 2))
}

/// Decodes the HTML character references in a string.
pub(crate) fn decode_entities(s: &str) -> Text {
    let mut out = Text::new();
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(rest.get(..pos).unwrap_or(""));
        let tail = rest.get(pos..).unwrap_or("");
        if let Some((text, len)) = entity_at(tail) {
            out.push_str(&text);
            let bytes: usize = tail.chars().take(len).map(char::len_utf8).sum();
            rest = tail.get(bytes..).unwrap_or("");
        } else {
            out.push('&');
            rest = tail.get(1..).unwrap_or("");
        }
    }
    out.push_str(rest);
    out
}

/// Removes the backslash before each escaped ASCII punctuation character.
pub(crate) fn unescape(s: &str) -> Text {
    let mut out = Text::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&next) = chars.peek()
            && next.is_ascii_punctuation()
        {
            out.push(next);
            chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

/// Parses an attribute block `{#id .class key=value}` at the start of `s`.
/// Returns the attributes and the byte length of the block.
pub(crate) fn parse_attributes(s: &str) -> Option<(Attr, usize)> {
    let body = s.strip_prefix('{')?;
    let mut attr = Attr::default();
    let mut k = 0;
    let bytes = body.as_bytes();
    loop {
        while bytes.get(k).is_some_and(|b| b.is_ascii_whitespace()) {
            k += 1;
        }
        match bytes.get(k)? {
            b'}' => return Some((attr, k + 2)),
            b'#' => {
                let start = k + 1;
                k = start;
                while bytes.get(k).is_some_and(|b| is_ident_byte(*b)) {
                    k += 1;
                }
                if k == start {
                    return None;
                }
                attr.id = body.get(start..k)?.to_string();
            }
            b'.' => {
                let start = k + 1;
                k = start;
                while bytes.get(k).is_some_and(|b| is_ident_byte(*b)) {
                    k += 1;
                }
                if k == start {
                    return None;
                }
                attr.classes.push(body.get(start..k)?.to_string());
            }
            b'-' if bytes
                .get(k + 1)
                .is_some_and(|b| b.is_ascii_whitespace() || *b == b'}') =>
            {
                attr.classes.push("unnumbered".into());
                k += 1;
            }
            _ => {
                let start = k;
                while bytes
                    .get(k)
                    .is_some_and(|b| is_ident_byte(*b) || *b == b':')
                {
                    k += 1;
                }
                if k == start || bytes.get(k) != Some(&b'=') {
                    return None;
                }
                let key = body.get(start..k)?.to_string();
                k += 1;

                let value = match bytes.get(k) {
                    Some(&q) if q == b'"' || q == b'\'' => {
                        let vstart = k + 1;
                        k = vstart;
                        let mut v = Text::new();
                        loop {
                            match bytes.get(k)? {
                                b'\\' if bytes.get(k + 1).is_some() => {
                                    let ch = body.get(k + 1..)?.chars().next()?;
                                    v.push(ch);
                                    k += 1 + ch.len_utf8();
                                }
                                b if *b == q => break,
                                _ => {
                                    let ch = body.get(k..)?.chars().next()?;
                                    v.push(ch);
                                    k += ch.len_utf8();
                                }
                            }
                        }
                        k += 1;
                        v
                    }
                    _ => {
                        let vstart = k;
                        while bytes
                            .get(k)
                            .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'}')
                        {
                            k += 1;
                        }
                        body.get(vstart..k)?.to_string()
                    }
                };
                match key.as_str() {
                    "id" => attr.id = value,
                    "class" => attr
                        .classes
                        .extend(value.split_whitespace().map(str::to_string)),
                    _ => attr.attributes.push((key, value)),
                }
            }
        }
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':' || b == b'.' || b >= 0x80
}

/// Merges adjacent `Str` inlines and white space, and removes white space at the ends.
pub(crate) fn normalize(xs: Vec<Inline>) -> Vec<Inline> {
    let mut out = join_white_space(xs);
    let lead = out
        .iter()
        .take_while(|x| matches!(x, Inline::Space | Inline::SoftBreak))
        .count();
    out.drain(..lead);
    while matches!(out.last(), Some(Inline::Space | Inline::SoftBreak)) {
        out.pop();
    }
    out
}

/// Merges adjacent `Str` inlines and runs of white space, as `normalize`
/// does, without trimming the ends.
fn join_white_space(xs: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(xs.len());
    for x in xs {
        match (out.last_mut(), x) {
            (Some(Inline::Str(a)), Inline::Str(b)) => a.push_str(&b),
            (Some(Inline::Space), Inline::Space) => {}
            (Some(Inline::Space), Inline::SoftBreak) => {
                out.pop();
                out.push(Inline::SoftBreak);
            }
            (Some(Inline::Space), Inline::LineBreak) => {
                out.pop();
                out.push(Inline::LineBreak);
            }
            (Some(Inline::SoftBreak | Inline::LineBreak), Inline::Space) => {}
            (Some(Inline::SoftBreak), Inline::SoftBreak) => {}
            (_, Inline::Str(b)) if b.is_empty() => {}
            (_, x) => out.push(x),
        }
    }
    out
}

/// Merges adjacent `Str` inlines and white space, without trimming the ends.
pub(crate) fn merge(xs: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(xs.len());
    for x in xs {
        match (out.last_mut(), x) {
            (Some(Inline::Str(a)), Inline::Str(b)) => a.push_str(&b),
            (Some(Inline::Space), Inline::Space) => {}
            (_, Inline::Str(b)) if b.is_empty() => {}
            (_, x) => out.push(x),
        }
    }
    out
}

/// Joins an abbreviation and the next word with a non-breaking space.
pub(crate) fn join_abbreviations(
    xs: Vec<Inline>,
    abbreviations: &std::collections::BTreeSet<Text>,
) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(xs.len());
    let mut iter = xs.into_iter().peekable();
    while let Some(x) = iter.next() {
        if let Inline::Str(s) = &x {
            let word = s
                .rsplit(|c: char| !c.is_alphanumeric() && c != '.')
                .next()
                .unwrap_or(s);
            if abbreviations.contains(word) && matches!(iter.peek(), Some(Inline::Space)) {
                let mut joined = s.clone();
                iter.next();
                joined.push('\u{a0}');
                if let Some(Inline::Str(next)) = iter.peek() {
                    joined.push_str(next);
                    iter.next();
                }
                out.push(Inline::Str(joined));
                continue;
            }
        }
        out.push(x);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn with_inl<R>(text: &str, run: impl FnOnce(&mut Inl<'_>) -> R) -> R {
        let mut options = randoc_core::ReaderOptions::default();
        options.extensions = randoc_core::default_extensions("markdown");
        let mut parser = Parser::new(&options, "", 128);
        let all: Rc<[char]> = text.chars().collect();
        let hi = all.len();
        let chars = Chars { all, lo: 0, hi };
        let mut inl = Inl::new(&mut parser, chars, Index::default(), false, 0);
        run(&mut inl)
    }

    #[test]
    fn each_context_has_its_own_memo_flags() {
        let flags = with_inl("", |inl| {
            let mut all = Vec::new();
            for in_link in [false, true] {
                for scripts in [0, 1] {
                    for quotes in 0..4u8 {
                        inl.in_link = in_link;
                        inl.scripts = scripts;
                        inl.quotes = quotes;
                        all.push(inl.memo_flags());
                    }
                }
            }
            all
        });
        let distinct: HashSet<u8> = flags.iter().copied().collect();
        assert_eq!(distinct.len(), 16, "{flags:?}");
        assert!(flags.contains(&0));
    }

    #[test]
    fn a_failed_sequence_does_not_fail_in_another_context() {
        // Outside a superscript, the unmatched `*` runs to the end, so the
        // double quote has no closer. Inside a superscript the `*` stays
        // literal and the quote closes. The first failure must not decide
        // the second attempt at the same position.
        let (outside, inside) = with_inl("\"*b\" x", |inl| {
            inl.i = 1;
            let outside = inl.seq_closing(Stop::DoubleQuote).is_some();
            inl.i = 1;
            inl.scripts = 1;
            let inside = inl.seq_closing(Stop::DoubleQuote).is_some();
            (outside, inside)
        });
        assert!(!outside);
        assert!(inside);
    }

    #[test]
    fn attributes_parse() {
        let (attr, used) = parse_attributes("{#a .b c=d e=\"f g\"} x").unwrap_or_default();
        assert_eq!(used, 19);
        assert_eq!(attr.id, "a");
        assert_eq!(attr.classes, vec!["b"]);
        assert_eq!(
            attr.attributes,
            vec![("c".into(), "d".into()), ("e".into(), "f g".into())]
        );
        assert!(parse_attributes("{a b}").is_none());
        assert!(parse_attributes("{#}").is_none());
    }

    #[test]
    fn entities_and_escapes() {
        assert_eq!(decode_entities("a&amp;b&#65;&#x42;&bogus;"), "a&bAB&bogus;");
        assert_eq!(unescape("a\\*b\\c"), "a*b\\c");
    }

    #[test]
    fn autolinks_and_keys() {
        assert_eq!(autolink_len("<http://x>"), Some(10));
        assert_eq!(autolink_len("<a@b.c>"), Some(7));
        assert_eq!(autolink_len("<a b>"), None);
        assert_eq!(cite_key("@doe:2001."), Some(("doe:2001".into(), 9)));
        assert_eq!(cite_key("@{a.b}"), Some(("a.b".into(), 6)));
        assert_eq!(cite_key("@{a b}"), None);
    }
}
