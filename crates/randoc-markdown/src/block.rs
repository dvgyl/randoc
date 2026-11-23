//! The block phase and the driver of the reader.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use randoc_ast::{
    Attr, Block, Caption, Format, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, Meta,
    MetaValue, Pandoc, Target, Text,
};
use randoc_core::{Error, Extension, Extensions, LogMessage, ReaderOptions, Result, SourcePos};

use crate::tables::{CellParser, GRID_MARK, TableOptions, parse_table};
use crate::tokens::{TokenIndex, TokenRules, View};
use randoc_core::shared::{IdentifierSet, auto_identifier};

/// The format name of a placeholder inline that holds raw Markdown text.
pub(crate) const PLACEHOLDER: &str = "\u{0}markdown";

/// One input line after tab expansion, with its 1-based line number.
///
/// A line of a nested container shares the text of the outer line, so that
/// removing a marker or an indentation does not copy the text.
#[derive(Debug, Clone)]
pub(crate) struct Ln {
    src: Rc<str>,
    /// The start of the line in `src`. `gobble` moves it in place, so that a
    /// block inside a raw HTML element starts with no copy of its lines.
    off: Cell<usize>,
    /// The number of leading spaces. Each level of a nested container asks
    /// for it, so the line keeps it and does not count the spaces again.
    spaces: Cell<usize>,
    /// The line has only spaces and tabs.
    blank: bool,
    pub n: u32,
}

impl Ln {
    pub(crate) fn new(t: impl Into<Text>, n: u32) -> Self {
        let t: Text = t.into();
        crate::step(t.len());
        Ln {
            spaces: Cell::new(indent(&t)),
            blank: blank(&t),
            src: Rc::from(t),
            off: Cell::new(0),
            n,
        }
    }

    /// Returns the number of leading spaces.
    fn indent(&self) -> usize {
        self.spaces.get()
    }

    /// Moves the start of this line `k` bytes forward, for each reader of it,
    /// and returns the old start for `restore`.
    fn skip_to(&self, k: usize) -> (usize, usize) {
        let saved = (self.off.get(), self.spaces.get());
        self.off.set(saved.0 + k);
        self.spaces.set(indent(self.t()));
        saved
    }

    /// Gives this line the start that `skip_to` returned.
    fn restore(&self, (off, spaces): (usize, usize)) {
        self.off.set(off);
        self.spaces.set(spaces);
    }

    /// Removes up to `n` leading spaces from this line, for each reader of it.
    fn gobble(&self, n: usize) {
        let k = self.spaces.get().min(n);
        self.off.set(self.off.get() + k);
        self.spaces.set(self.spaces.get() - k);
    }

    /// Returns the position of the first character after the leading spaces
    /// in the shared text. `gobble`, `dedent` and `tail` keep it, so it names
    /// the line in a cache.
    fn text_start(&self) -> usize {
        self.off.get() + self.spaces.get()
    }

    /// Names the shared text of the line. With `text_start` it names a
    /// position of the input, also for a line that a parse made from the
    /// rest of a line. A cache keeps the text alive, so no later text can
    /// reuse the name.
    fn src_id(&self) -> usize {
        Rc::as_ptr(&self.src).cast::<u8>() as usize
    }

    /// Returns true when the line has only spaces and tabs.
    pub(crate) fn is_blank(&self) -> bool {
        self.blank
    }

    /// Makes a line without up to `n` leading spaces. It shares the text.
    fn dedent(&self, n: usize) -> Ln {
        let k = self.indent().min(n);
        crate::step(1);
        Ln {
            src: Rc::clone(&self.src),
            off: Cell::new(self.off.get() + k),
            spaces: Cell::new(self.indent() - k),
            blank: self.blank,
            n: self.n,
        }
    }

    /// Returns the text of the line.
    pub(crate) fn t(&self) -> &str {
        self.src.get(self.off.get()..).unwrap_or("")
    }

    /// Makes a line from `rest`, which shares the text when it is the end of this line.
    fn tail(&self, rest: &str, n: u32) -> Ln {
        let t = self.t();
        let base = t.as_ptr() as usize;
        let start = rest.as_ptr() as usize;
        if start >= base && start + rest.len() == base + t.len() {
            crate::step(1);
            let removed = start - base;
            // Only spaces went away: the counts of this line give the new ones.
            let (spaces, blank) = if removed <= self.indent() {
                (self.indent() - removed, self.blank)
            } else {
                (indent(rest), blank(rest))
            };
            Ln {
                src: Rc::clone(&self.src),
                off: Cell::new(self.off.get() + removed),
                spaces: Cell::new(spaces),
                blank,
                n,
            }
        } else {
            Ln::new(rest, n)
        }
    }
}

/// The parser state of both phases.
pub(crate) struct Parser {
    pub exts: Extensions,
    pub abbreviations: BTreeSet<Text>,
    columns: usize,
    tab_stop: usize,
    indented_code_classes: Vec<Text>,
    pub source: Text,
    source_lines: Vec<(u32, Text)>,
    source_line: u32,
    inline_directory: Text,
    pub max_depth: usize,
    pub depth: usize,
    log: Vec<LogMessage>,
    pub refs: HashMap<Text, (Target, Attr)>,
    pub header_refs: HashMap<Text, Target>,
    /// The largest number of characters of a key of `refs` or `header_refs`.
    pub longest_key: usize,
    pub notes: HashMap<Text, Vec<Block>>,
    note_pos: Vec<(Text, SourcePos)>,
    /// The keys of the note references in the order of use. A parse that
    /// Pandoc discards truncates it.
    pub used_notes: Vec<Text>,
    end_blank: bool,
    /// For the opening fence of a div (`Ln::src_id` and `Ln::text_start`),
    /// the number of lines to its closing fence, or `None` when it has no
    /// closer.
    /// The second number is the number of lines from the opening fence to
    /// the end of the slice that the scan read.
    div_close: HashMap<(usize, usize), (Option<usize>, usize)>,
    /// For an HTML element (`Ln::src_id` and the position of its opening
    /// tag in that text, tag name): the line distance to its closing tag,
    /// the `Ln::src_id` of that line and the byte position of the tag in
    /// its shared text, or `None` when it has no closer.
    /// The second number is the number of lines from the opening tag to the
    /// end of the slice that the scan read.
    html_close: HashMap<HtmlOpener, (Option<HtmlCloser>, usize)>,
    /// The texts that the keys of `html_close` and `no_raw_end` name.
    kept_sources: Vec<Rc<str>>,
    /// The token indexes of the slices that `blocks` parses now, innermost
    /// last.
    token_scopes: Vec<Tokens>,
    /// For a line (`Ln::src_id`, `Ln::text_start`) and a comment (`true`) or
    /// a processing instruction: the last line of a slice (the same names)
    /// such that no line from this one to it ends the token.
    no_raw_end: HashMap<(usize, usize, bool), (usize, usize)>,
    /// For a line (`Ln::src_id`, `Ln::text_start`) and an end marker such
    /// as `\end{code}`: the last line of a slice (the same names) such that
    /// no line from this one to it holds the marker.
    no_marker_below: HashMap<(usize, usize, Text), (usize, usize)>,
    rest: RestHandoff,
    idset: IdentifierSet,
    pub examples: HashMap<Text, i64>,
    example_count: i64,
    pub note_number: i64,
    pub in_note: bool,
    /// The bytes that splits of paragraphs at block tags can still read again.
    split_budget: usize,
    /// The resolve step is in the cells of a table that is not a grid table.
    inline_cells: bool,
    /// The macro definitions of the raw TeX read so far (`latex_macros`).
    macros: randoc_latex_reader::Macros,
    /// The blocks being read are in an HTML element with Markdown content
    /// that `markdown_attribute` gave: an element without the attribute
    /// has Markdown content too.
    markdown_inside: bool,
    /// The number of emphasis openers that found no closer.
    pub emphasis_failures: usize,
    /// The number of bracket texts that ended at a block tag.
    pub block_tag_cuts: usize,
    /// The number of brackets and quotes in the contents of a native span
    /// whose closer follows the contents: such a closer takes the closing tag.
    pub crossing_closers: usize,
    in_list: bool,
    /// The items being read belong to a loose list.
    loose_list: bool,
    /// The number of spaces that each block of the next block loop can start
    /// with: the indentation of the content of a raw HTML element.
    gobble: usize,
    end_line: u32,
    meta: Meta,
}

/// The characters that Pandoc trims as white space in Markdown. Other
/// Unicode spaces, such as U+00A0, are text.
pub(crate) const SPACE: [char; 4] = [' ', '\t', '\n', '\r'];

/// Tells whether `t` starts with an example marker (`(@)`, `@x.`, `(@x)`)
/// and a space or the line end.
fn is_example_marker(t: &str) -> bool {
    let (paren, body) = match t.strip_prefix('(') {
        Some(rest) => (true, rest),
        None => (false, t),
    };
    let Some(label) = body
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .strip_prefix('@')
    else {
        return false;
    };
    let rest = label.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_' || c == '-');
    let after = if paren {
        rest.strip_prefix(')')
    } else {
        rest.strip_prefix(['.', ')'])
    };
    after.is_some_and(|a| a.is_empty() || a.starts_with(' '))
}

/// Tells whether `s` ends with a backslash that is not escaped.
fn ends_with_escape(s: &str) -> bool {
    (s.len() - s.trim_end_matches('\\').len()) % 2 == 1
}

/// Removes the characters of `strip` at the end of `s`, but keeps the
/// first removed character when a backslash escapes it: an escaped space
/// is a nonbreaking space and an escaped `#` is text, not a closing hash.
fn trim_end_unescaped<'a>(s: &'a str, strip: &[char]) -> &'a str {
    let trimmed = s.trim_end_matches(strip);
    if trimmed.len() < s.len() && ends_with_escape(trimmed) {
        s.get(..trimmed.len() + 1).unwrap_or(trimmed)
    } else {
        trimmed
    }
}

/// Splits the author field of a title block. A `;` or a line break ends
/// an author, and the spaces and line breaks after a `;` belong to it, so
/// `a;` gives two authors, the second one empty.
fn title_block_authors(field: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(field.trim_matches(SPACE));
    std::iter::from_fn(move || {
        let s = rest?;
        match s.find([';', '\n']) {
            Some(k) => {
                let after = s.get(k + 1..).unwrap_or("");
                rest = Some(if s.get(k..k + 1) == Some(";") {
                    after.trim_start_matches(SPACE)
                } else {
                    after
                });
                Some(s.get(..k).unwrap_or("").trim_matches(SPACE))
            }
            None => {
                rest = None;
                Some(s.trim_matches(SPACE))
            }
        }
    })
}

/// Removes the closing hashes of an ATX heading and the spaces before them.
fn strip_closing_hashes(text: &str) -> &str {
    trim_end_unescaped(trim_end_unescaped(text, &['#']), &SPACE)
}

fn blank(s: &str) -> bool {
    let rest = s.trim_start_matches([' ', '\t']);
    crate::step(s.len() - rest.len());
    rest.trim_end_matches([' ', '\t']).is_empty()
}

fn indent(s: &str) -> usize {
    let spaces = s.len() - s.trim_start_matches(' ').len();
    crate::step(spaces);
    spaces
}

/// Removes up to `n` leading spaces.
fn dedent(s: &str, n: usize) -> &str {
    let k = indent(s).min(n);
    s.get(k..).unwrap_or("")
}

fn expand_tabs(line: &str, tab_stop: usize) -> Text {
    if !line.contains('\t') {
        return line.to_string();
    }
    let mut out = Text::new();
    let mut col = 0usize;
    for c in line.chars() {
        if c == '\t' {
            let n = tab_stop - col % tab_stop;
            out.extend(std::iter::repeat_n(' ', n));
            col += n;
        } else {
            out.push(c);
            col += 1;
        }
    }
    out
}

/// The failed searches for a closing fence in one slice of lines. When a
/// search from line `at` for `n` or more fence characters finds no closer,
/// a later search for `n` or more of the same character finds none either.
#[derive(Default)]
struct FenceMemo {
    /// For a comment and a processing instruction: the line where the last
    /// search for the line of its end started, and the line that it found.
    raw_ends: [Option<(usize, Option<usize>)>; 2],
    /// The same for the end of a TeX environment, by name.
    tex_ends: HashMap<Text, (usize, Option<usize>)>,
    tokens: Tokens,
}

/// The token index of a slice of lines. The blocks of a container are a
/// part of the slice of the container, so they share its index: a token
/// with no end makes one search to the end of the lines, not one at each
/// level of nesting.
#[derive(Clone, Default)]
struct Tokens {
    index: Rc<RefCell<TokenIndex>>,
    /// The line of the index that is the first line of the slice.
    offset: usize,
    /// The address of the slice, which names it while it is parsed.
    base: usize,
    len: usize,
    fences: Rc<RefCell<FenceFailures>>,
}

/// Which closing line a fence search looks for: the closer of a fenced
/// code block, or the closer that an HTML closer scan skips to.
#[derive(Clone, Copy)]
enum FenceSearch {
    Block,
    Closer,
}

/// The last failed search for a closing fence of each search kind and
/// fence character, in lines of the token index: the opener line, the run
/// length and the end of the lines that the search read. The parts of a
/// slice share it, so that nested containers that each hold the same
/// unclosed fence make one search, not one at each level.
#[derive(Default)]
struct FenceFailures([Option<(usize, usize, usize)>; 4]);

impl FenceFailures {
    fn slot(search: FenceSearch, c: char) -> usize {
        2 * usize::from(matches!(search, FenceSearch::Closer)) + usize::from(c == '~')
    }

    /// Tells whether an earlier search proves that no line after `from`
    /// and before `end` closes a fence of `n` characters `c`.
    fn covers(&self, search: FenceSearch, c: char, from: usize, n: usize, end: usize) -> bool {
        self.0
            .get(Self::slot(search, c))
            .copied()
            .flatten()
            .is_some_and(|(line, len, read_to)| from >= line && n >= len && end <= read_to)
    }

    fn record(&mut self, search: FenceSearch, c: char, from: usize, n: usize, end: usize) {
        if let Some(slot) = self.0.get_mut(Self::slot(search, c))
            && slot.is_none_or(|(_, len, read_to)| n <= len && end >= read_to)
        {
            *slot = Some((from, n, end));
        }
    }
}

impl Tokens {
    /// Returns the last line of `all`, the slice of this index, that the
    /// complete tokens from line `j` on reach.
    fn reach(&self, all: &[Ln], j: usize, rules: TokenRules) -> usize {
        let view = View {
            lines: all,
            at: self.offset,
        };
        let reached = self.index.borrow_mut().reach(view, self.offset + j, rules);
        reached - self.offset
    }

    /// Reads the lines of `all`, the slice of this index, up to line `j`, so
    /// that a part of the slice from line `j + 1` on can share the index.
    fn read_through(&self, all: &[Ln], j: usize) {
        let view = View {
            lines: all,
            at: self.offset,
        };
        self.index.borrow_mut().read_through(view, self.offset + j);
    }

    /// Returns the tokens of `lines` when they are a part of this slice and
    /// the index read the lines before them, so that it can go on with
    /// their lines.
    fn part(&self, lines: &[Ln]) -> Option<Tokens> {
        let size = std::mem::size_of::<Ln>();
        let distance = (lines.as_ptr() as usize).checked_sub(self.base)?;
        let k = distance / size;
        let inside = distance % size == 0 && k + lines.len() <= self.len;
        let offset = self.offset + k;
        (inside && self.index.borrow().frontier() >= offset).then(|| Tokens {
            index: Rc::clone(&self.index),
            offset,
            base: lines.as_ptr() as usize,
            len: lines.len(),
            fences: Rc::clone(&self.fences),
        })
    }
}

/// A list marker at the start of a line.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ListKind {
    Bullet,
    Ordered(ListNumberStyle, ListNumberDelim),
}

struct Marker {
    kind: ListKind,
    start: i64,
    label: Option<Text>,
    /// An example marker with a number before `@`: `start` is its number.
    numbered: bool,
    /// The column of the item content.
    content: usize,
    /// The text of the first line after the marker.
    /// The byte offset in the line where the item text starts.
    rest_from: usize,
}

fn roman_value(s: &str) -> Option<i64> {
    let mut total = 0i64;
    let mut prev = 0i64;
    for c in s.chars().rev() {
        let v = match c.to_ascii_lowercase() {
            'i' => 1,
            'v' => 5,
            'x' => 10,
            'l' => 50,
            'c' => 100,
            'd' => 500,
            'm' => 1000,
            _ => return None,
        };
        if v < prev {
            total -= v;
        } else {
            total += v;
            prev = v;
        }
    }
    (total > 0).then_some(total)
}

fn is_hrule(s: &str) -> bool {
    let t = s.trim_matches(SPACE);
    crate::step(1);
    let Some(first) = t.chars().next() else {
        return false;
    };
    if !matches!(first, '*' | '-' | '_') || !t.ends_with(first) {
        return false;
    }
    let mut count = 0;
    for c in t.chars() {
        if c == first {
            count += 1;
        } else if c != ' ' {
            return false;
        }
    }
    count >= 3
}

fn fence_open(s: &str) -> Option<(char, usize, usize, &str)> {
    let ind = indent(s);
    if ind > 3 {
        return None;
    }
    let t = s.get(ind..)?;
    let c = t.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = t.len() - t.trim_start_matches(c).len();
    if n < 3 {
        return None;
    }
    let info = t.get(n..)?;
    if c == '`' && info.contains('`') {
        return None;
    }
    Some((c, n, ind, info))
}

fn div_fence(t: &str) -> Option<(usize, &str)> {
    let n = t.len() - t.trim_start_matches(':').len();
    if n < 3 {
        return None;
    }
    Some((n, t.get(n..)?.trim_matches(SPACE)))
}

/// Block-level HTML tag names. A black-box test of each HTML element name
/// with the oracle (`<name>x</name>` gives a raw block) gave this list.
const BLOCK_TAGS: &[&str] = &[
    "address",
    "applet",
    "area",
    "article",
    "aside",
    "audio",
    "blockquote",
    "body",
    "button",
    "canvas",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "del",
    "details",
    "dir",
    "div",
    "dl",
    "dt",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "iframe",
    "ins",
    "isindex",
    "li",
    "main",
    "map",
    "menu",
    "meta",
    "nav",
    "noframes",
    "noscript",
    "object",
    "ol",
    "output",
    "p",
    "pre",
    "progress",
    "script",
    "section",
    "source",
    "style",
    "summary",
    "svg",
    "table",
    "tbody",
    "td",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
    "video",
];

/// The LaTeX environments that a paragraph reads as raw TeX inlines, not as
/// a raw block.
const MATH_ENVIRONMENTS: &[&str] = &[
    "align",
    "alignat",
    "displaymath",
    "eqnarray",
    "equation",
    "flalign",
    "gather",
    "math",
    "multline",
];

/// Block tags that do not end a paragraph after text on the same line.
const MID_LINE_INLINE_TAGS: &[&str] = &[
    "embed", "iframe", "map", "noscript", "object", "source", "style", "track", "video",
];

/// Block tags that start a raw block but do not end a paragraph.
/// The block tags of elements that have no content and no closing tag.
const VOID_TAGS: &[&str] = &["area", "col", "embed", "hr", "meta", "source", "track"];

const NON_INTERRUPTING_TAGS: &[&str] = &["applet", "area", "audio", "button", "del", "ins", "svg"];

/// Returns the name and the closing flag of an HTML tag at the start of `s`.
fn html_tag_name(s: &str) -> Option<(Text, bool)> {
    let t = s.strip_prefix('<')?;
    let (closing, t) = match t.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, t),
    };
    let name: Text = t
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == ':')
        .collect();
    if name.is_empty() || !name.starts_with(char::is_alphabetic) || name.ends_with(':') {
        return None;
    }
    let after = t.get(name.len()..)?.chars().next();
    match after {
        None | Some(' ' | '>' | '/' | '\n') => Some((name.to_ascii_lowercase(), closing)),
        _ => None,
    }
}

/// Tells whether `s` starts a comment (`<!--` not followed by `>` or `->`)
/// or a processing instruction (`<?` and an ASCII letter).
pub(crate) fn starts_comment_or_pi(s: &str) -> bool {
    match s.strip_prefix("<!--") {
        Some(rest) => !rest.starts_with('>') && !rest.starts_with("->"),
        None => s
            .strip_prefix("<?")
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_alphabetic())),
    }
}

/// Returns the byte length of a comment, which ends at `-->`, or of a
/// processing instruction, which ends at the first `>`.
fn html_comment_or_pi_len(s: &str) -> Option<usize> {
    if !starts_comment_or_pi(s) {
        return None;
    }
    if s.starts_with("<!--") {
        s.get(4..)?.find("-->").map(|i| i + 7)
    } else {
        s.find('>').map(|i| i + 1)
    }
}

/// Returns 0 when `text` has a comment with no end outside code spans, and
/// 1 for a processing instruction with no end.
fn open_comment_or_pi(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut no_closer: HashSet<usize> = HashSet::new();
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        crate::step(1);
        match b {
            b'\\' => i += 2,
            b'`' => {
                let n = bytes.get(i..)?.iter().take_while(|c| **c == b'`').count();
                i += n;
                if no_closer.contains(&n) {
                    continue;
                }
                match backtick_run(bytes, i, n) {
                    Some(end) => i = end,
                    None => {
                        no_closer.insert(n);
                    }
                }
            }
            b'<' => {
                let rest = text.get(i..)?;
                if !starts_comment_or_pi(rest) {
                    i += 1;
                    continue;
                }
                match html_comment_or_pi_len(rest) {
                    Some(len) => i += len,
                    None => return Some(usize::from(rest.starts_with("<?"))),
                }
            }
            _ => i += 1,
        }
    }
    None
}

/// Returns the end of the next run of exactly `n` backticks at or after `from`.
fn backtick_run(bytes: &[u8], from: usize, n: usize) -> Option<usize> {
    let mut i = from;
    while let Some(&b) = bytes.get(i) {
        if b != b'`' {
            i += 1;
            continue;
        }
        let m = bytes.get(i..)?.iter().take_while(|c| **c == b'`').count();
        crate::step(1);
        i += m;
        if m == n {
            return Some(i);
        }
    }
    None
}

/// Tells whether `s` starts with a complete tag that ends a paragraph.
fn is_block_tag(s: &str) -> bool {
    html_tag_name(s).is_some_and(|(name, _)| {
        BLOCK_TAGS.contains(&name.as_str()) && !NON_INTERRUPTING_TAGS.contains(&name.as_str())
    }) && html_tag_len(s).is_some()
}

/// Tells whether `s` starts with a block tag that ends a paragraph after
/// text on the same line.
fn is_mid_line_block_tag(s: &str) -> bool {
    is_block_tag(s)
        && html_tag_name(s).is_some_and(|(name, closing)| {
            !MID_LINE_INLINE_TAGS.contains(&name.as_str()) && !(closing && name == "script")
        })
}

/// Maps the position of each `[` in `bytes` that has a matching `]` to the
/// position of that `]`. A `[` or `]` after a backslash does not count.
fn matching_brackets(bytes: &[u8]) -> HashMap<usize, usize> {
    let mut open = Vec::new();
    let mut pairs = HashMap::new();
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        match b {
            b'\\' => i += 1,
            b'[' => open.push(i),
            b']' => {
                if let Some(start) = open.pop() {
                    pairs.insert(start, i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    pairs
}

/// Tells whether `s` is a line of `=` or of `-` with only spaces after it.
fn is_setext_underline(s: &str) -> bool {
    let u = s.trim_end_matches(SPACE);
    !u.is_empty() && (u.chars().all(|c| c == '=') || u.chars().all(|c| c == '-'))
}

/// Returns the byte length of a complete HTML tag at the start of `s`.
pub(crate) fn html_tag_len(s: &str) -> Option<usize> {
    if let Some(len) = html_comment_or_pi_len(s) {
        return Some(len);
    }
    html_tag_name(s)?;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices().skip(1) {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '>' => return Some(i + 1),
            None if c == '<' => return None,
            None => {}
        }
    }
    None
}

/// Returns the length of the tag at the start of `s`, as `html_tag_len`
/// does. The error tells whether a quoted value has no end: that value then
/// holds the rest of the text.
fn tag_extent(s: &str) -> std::result::Result<usize, bool> {
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices().skip(1) {
        crate::step(1);
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '>' => return Ok(i + 1),
            None if c == '<' => return Err(false),
            None => {}
        }
    }
    Err(quote.is_some())
}

/// An opening tag of an HTML element: the `Ln::src_id` of its line, the
/// position of the tag in that text, and the tag name.
type HtmlOpener = (usize, usize, Text);

/// The rest of a line after an HTML element is read in a loop, not by a
/// new level of recursion for each element of the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestHandoff {
    Off,
    /// `rest_as_lines` calls `block_at` for the rest of a line.
    Armed,
    /// That `block_at` calls `html_block` for its first line.
    Html,
    /// That element ends before more text of line `line` of the slice: the
    /// last `len` bytes of its text.
    Deferred {
        line: usize,
        len: usize,
    },
}

/// The closing tag of an HTML element: the line distance from the opening
/// tag, the `Ln::src_id` of its line, and its position in that text.
type HtmlCloser = (usize, usize, usize);

/// The closing tag search of an HTML element whose content is Markdown.
/// Pandoc finds the closer with the parser of the content, so a tag in a
/// token that holds literal text does not count: a fenced code block, a
/// code span, a math span, an escape, a comment, a quoted attribute, or
/// an indented code block.
#[derive(Clone, Copy)]
struct CloserScan<'a> {
    name: &'a str,
    rules: TokenRules,
    /// The index of the first content line when the tag line has no
    /// content after the tag: an indented line there starts a code block.
    first_line: Option<usize>,
    /// The spaces that each content line loses before its blocks start.
    content_indent: usize,
    /// The element is a native div: its content starts with no spaces lost.
    native_div: bool,
    /// Without `markdown_in_html_blocks`, a tag of a native div counts only
    /// at the start of a line: a closing tag there ends a paragraph, and an
    /// opening tag starts a nested div only where a block can start.
    line_start_tags: bool,
    /// Without `raw_html`, a tag of a native div counts only at the first
    /// column of a line: elsewhere it is text.
    column_one_tags: bool,
}

impl<'a> CloserScan<'a> {
    fn new(
        parser: &Parser,
        name: &'a str,
        first_line: Option<usize>,
        content_indent: usize,
    ) -> Self {
        CloserScan {
            name,
            rules: parser.token_rules(true),
            first_line,
            content_indent,
            native_div: name == "div" && parser.ext(Extension::NativeDivs),
            line_start_tags: name == "div"
                && parser.ext(Extension::NativeDivs)
                && !parser.ext(Extension::MarkdownInHtmlBlocks),
            column_one_tags: name == "div"
                && parser.ext(Extension::NativeDivs)
                && !parser.ext(Extension::RawHtml),
        }
    }

    /// Returns the closer of the element that opens in `lines[0]`, as the
    /// line index and the byte position in its text. `found` gets the
    /// position of each opening tag and its closer, or `None` when it has
    /// no closer, unless an indented line follows a nested tag. `tokens` is
    /// the token index of `lines`.
    fn run(
        self,
        lines: &[Ln],
        tokens: &Tokens,
        mut found: impl FnMut((usize, usize), Option<(usize, usize)>),
    ) -> Option<(usize, usize)> {
        let mut stack: Vec<(usize, usize)> = Vec::new();
        let mut fence: Option<(char, usize)> = None;
        let mut no_fence_closer = tokens.fences.borrow_mut();
        let mut index = tokens.index.borrow_mut();
        let off = tokens.offset;
        let view = View { lines, at: off };
        let local = |(line, byte): (usize, usize)| (line - off, byte);
        let mut literal_until = None;
        let mut code_may_start = false;
        // The last line with four spaces or more. The indentation inside a
        // container differs, so an element that opens before such a line
        // has an answer only for this scan.
        let mut last_indented = 0;
        // A block can start at the line: the line before it is blank or
        // holds only a tag of the element.
        let mut block_start = true;
        for (j, line) in lines.iter().enumerate() {
            let t = line.t();
            // The loop over the bytes below counts its own steps.
            crate::step(1);
            let mut i = 0;
            let starts_block = std::mem::replace(&mut block_start, line.is_blank());
            if j > 0 && line.indent() >= 4 {
                last_indented = j;
            }
            let code_line = (code_may_start || self.first_line == Some(j))
                && !line.is_blank()
                && line.indent() >= 4 + self.content_indent;
            code_may_start = j > 0 && (line.is_blank() || code_line);
            let mut bare_open = false;
            if code_line && fence.is_none() && literal_until.is_none() {
                continue;
            }
            if let Some((end_line, end_byte)) = literal_until {
                if j < end_line {
                    continue;
                }
                literal_until = None;
                i = end_byte;
            } else if let Some((c, n)) = fence {
                if closes_fence(t, c, n) {
                    fence = None;
                    block_start = true;
                }
                continue;
            } else if j > 0
                && let Some((c, n, _, _)) = fence_open(container_content(t))
            {
                let end = off + lines.len();
                let closer_below = |count: bool| {
                    lines.get(j + 1..).is_some_and(|rest| {
                        rest.iter().any(|l| {
                            if count {
                                crate::step(1);
                            }
                            closes_fence(l.t(), c, n)
                        })
                    })
                };
                let known = no_fence_closer.covers(FenceSearch::Closer, c, off + j, n, end);
                #[cfg(test)]
                if known {
                    crate::check_cache(&false, || closer_below(false));
                }
                if !known {
                    if closer_below(true) {
                        fence = Some((c, n));
                        continue;
                    }
                    no_fence_closer.record(FenceSearch::Closer, c, off + j, n, end);
                }
            }
            let bytes = t.as_bytes();
            // Only tags of the element precede position `i` on the line.
            let mut lead = i == 0;
            while let Some(&b) = bytes.get(i) {
                crate::step(1);
                let at_lead = std::mem::replace(&mut lead, stack.is_empty() && b == b' ');
                if let Some(end) = index.end(view, (off + j, i), self.rules).map(local) {
                    if end.0 > j {
                        literal_until = Some(end);
                        break;
                    }
                    i = end.1;
                    continue;
                }
                match b {
                    b'\\' => i += 2,
                    b'<' => {
                        let rest = t.get(i..).unwrap_or("");
                        let tag = self.tag_at(rest).filter(|tag| {
                            (!self.column_one_tags || i == 0)
                                && (!self.line_start_tags
                                    || at_lead
                                        && (matches!(tag, Tag::Close)
                                            || j == 0
                                            || starts_block
                                            || i > 0))
                        });
                        lead = tag.is_some();
                        match tag {
                            Some(Tag::Close) => {
                                if let Some(open) = stack.pop() {
                                    if open.0 == 0 || last_indented <= open.0 {
                                        found(open, Some((j, i)));
                                    }
                                    if stack.is_empty() {
                                        return Some((j, i));
                                    }
                                }
                                i += self.name.len() + 2;
                                block_start = t.get(i..).is_some_and(|r| {
                                    r.trim_start_matches(|c| c != '>')
                                        .get(1..)
                                        .is_some_and(blank)
                                });
                            }
                            Some(Tag::Open) => {
                                let end = index.tag_end(view, (off + j, i)).map(local);
                                bare_open = self.native_div
                                    && end.is_some_and(|(line, byte)| {
                                        line == j && t.get(byte..).is_some_and(blank)
                                    });
                                stack.push((j, i));
                                if let Some(end) = end {
                                    if end.0 > j {
                                        literal_until = Some(end);
                                        break;
                                    }
                                    i = end.1;
                                    block_start = t.get(i..).is_some_and(blank);
                                } else {
                                    i += self.name.len() + 1;
                                }
                            }
                            None => {
                                if let Some(end) = index.tag_end(view, (off + j, i)).map(local) {
                                    if end.0 > j {
                                        literal_until = Some(end);
                                        break;
                                    }
                                    i = end.1;
                                } else {
                                    i += 1;
                                }
                            }
                        }
                    }
                    _ => i += 1,
                }
            }
            if stack.is_empty() {
                return None;
            }
            code_may_start |= bare_open;
        }
        for open in stack {
            if open.0 == 0 || last_indented <= open.0 {
                found(open, None);
            }
        }
        None
    }

    /// Tells whether `s` starts with an opening or a closing tag of the
    /// element.
    fn tag_at(self, s: &str) -> Option<Tag> {
        let (closing, rest) = match s.strip_prefix("</") {
            Some(rest) => (true, rest),
            None => (false, s.strip_prefix('<')?),
        };
        let name = rest.get(..self.name.len())?;
        if !name.eq_ignore_ascii_case(self.name) {
            return None;
        }
        let next = rest.get(self.name.len()..).and_then(|r| r.chars().next());
        if closing {
            // The name of a closing tag ends at its end, as in `</div >`.
            return matches!(next, None | Some(' ' | '>' | '\t' | '\n')).then_some(Tag::Close);
        }
        matches!(next, None | Some(' ' | '>' | '/' | '\t')).then_some(Tag::Open)
    }
}

/// A tag of the element that a `CloserScan` searches.
#[derive(Clone, Copy)]
enum Tag {
    Open,
    Close,
}

/// Makes a line from `rest`, the end of `lines[at]`, that shares its text
/// when it can: a cache keyed by the shared text then knows the line.
fn rest_line(lines: &[Ln], at: usize, rest: &str, n: u32) -> Ln {
    lines
        .get(at)
        .map_or_else(|| Ln::new(rest, n), |line| line.tail(rest, n))
}

/// Returns the index of the fence that closes the div whose opening fence
/// is `lines[0]`. One pass with a stack of open fences gives `found` the
/// closer of each opener in it, or `None` for one with no closer. With
/// `raw_html`, a fence line inside a comment or a processing instruction
/// over lines does not count. An opening fence right after a line of a
/// paragraph is text of the paragraph.
fn div_closer_scan(
    lines: &[Ln],
    rules: TokenRules,
    mut found: impl FnMut(usize, Option<usize>),
) -> Option<usize> {
    let mut stack: Vec<usize> = vec![0];
    let mut in_fence: Option<(char, usize)> = None;
    let mut raw_end: Option<usize> = None;
    let mut tokens = TokenIndex::default();
    let view = View { lines, at: 0 };
    let mut after_text = false;
    let mut j = 1;
    while let Some(line) = lines.get(j) {
        crate::step(1);
        let mut text = false;
        if let Some((c, n)) = in_fence {
            let tt = line.t().trim_matches(SPACE);
            if tt.len() - tt.trim_start_matches(c).len() >= n && tt.trim_start_matches(c).is_empty()
            {
                in_fence = None;
            }
        } else if let Some(end) = raw_end {
            if j >= end {
                raw_end = None;
            }
        } else if let Some((c, n, _, _)) = fence_open(line.t()) {
            in_fence = Some((c, n));
        } else if let Some((_, rest)) = div_fence(line.t()) {
            if rest.trim_end_matches(':').trim_matches(SPACE).is_empty() {
                if let Some(open) = stack.pop() {
                    found(open, Some(j));
                }
                if stack.is_empty() {
                    return Some(j);
                }
            } else if after_text {
                text = true;
            } else {
                stack.push(j);
            }
        } else {
            let covered = tokens.reach(view, j, rules);
            raw_end = (covered > j).then_some(covered);
            let t = line.t().trim_start_matches(' ');
            text = raw_end.is_none()
                && !line.is_blank()
                && !t.starts_with(['#', '>'])
                && !(rules.raw_html && t.starts_with('<'));
        }
        after_text = text;
        j += 1;
    }
    for open in stack {
        found(open, None);
    }
    None
}

/// Returns the text of a line after its block quote and list markers.
fn container_content(t: &str) -> &str {
    let mut s = t;
    loop {
        let u = s.trim_start_matches(' ');
        if let Some(r) = u.strip_prefix('>') {
            s = r.strip_prefix(' ').unwrap_or(r);
            continue;
        }
        let digits = u.len() - u.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let marker = if digits > 0 {
            u.get(digits..)
                .and_then(|r| r.strip_prefix('.').or_else(|| r.strip_prefix(')')))
        } else {
            u.strip_prefix(['-', '*', '+'])
        };
        match marker.and_then(|r| r.strip_prefix(' ')) {
            Some(r) => s = r,
            None => return s,
        }
    }
}

/// Tells whether the line `t` closes a fence of `n` or more characters `c`.
/// Tells whether the line `t` closes a fenced code block of `n` characters `c`.
fn closes_code_fence(t: &str, c: char, n: usize) -> bool {
    if indent(t) >= 4 {
        return false;
    }
    let tt = t.trim_matches(SPACE);
    tt.len() - tt.trim_start_matches(c).len() >= n && tt.trim_start_matches(c).is_empty()
}

fn closes_fence(t: &str, c: char, n: usize) -> bool {
    let tt = container_content(t).trim_matches(SPACE);
    tt.len() - tt.trim_start_matches(c).len() >= n && tt.trim_start_matches(c).is_empty()
}

impl Parser {
    pub(crate) fn new(options: &ReaderOptions, source: &str, max_depth: usize) -> Self {
        let abbreviations = options.abbreviations.clone();
        Parser {
            exts: options.extensions,
            abbreviations,
            columns: usize::try_from(options.columns).unwrap_or(72),
            tab_stop: usize::try_from(options.tab_stop).unwrap_or(4).clamp(1, 256),
            indented_code_classes: options.indented_code_classes.clone(),
            source: source.to_string(),
            source_lines: Vec::new(),
            source_line: 1,
            inline_directory: Text::new(),
            max_depth,
            depth: 0,
            log: Vec::new(),
            refs: HashMap::new(),
            header_refs: HashMap::new(),
            longest_key: 0,
            notes: HashMap::new(),
            note_pos: Vec::new(),
            used_notes: Vec::new(),
            end_blank: false,
            div_close: HashMap::new(),
            html_close: HashMap::new(),
            kept_sources: Vec::new(),
            token_scopes: Vec::new(),
            no_raw_end: HashMap::new(),
            no_marker_below: HashMap::new(),
            rest: RestHandoff::Off,
            idset: IdentifierSet::new(),
            examples: HashMap::new(),
            example_count: 0,
            note_number: 0,
            in_note: false,
            inline_cells: false,
            markdown_inside: false,
            macros: randoc_latex_reader::Macros::default(),
            split_budget: source.len().saturating_mul(4).saturating_add(4096),
            emphasis_failures: 0,
            crossing_closers: 0,
            block_tag_cuts: 0,
            in_list: false,
            loose_list: false,
            gobble: 0,
            end_line: 1,
            meta: Meta::default(),
        }
    }

    /// Records the first line and directory of each named input.
    pub(crate) fn set_sources(&mut self, sources: &randoc_core::Sources) {
        let mut line = 1u32;
        for source in &sources.0 {
            self.source_lines
                .push((line, source_directory(&source.name).into()));
            let newlines = u32::try_from(source.text.bytes().filter(|byte| *byte == b'\n').count())
                .unwrap_or(u32::MAX);
            let implicit_newline =
                u32::from(!source.text.is_empty() && !source.text.ends_with('\n'));
            line = line
                .saturating_add(newlines)
                .saturating_add(1 + implicit_newline);
        }
    }

    fn source_directory(&self) -> &str {
        let index = self
            .source_lines
            .partition_point(|(line, _)| *line <= self.source_line);
        index
            .checked_sub(1)
            .and_then(|index| self.source_lines.get(index))
            .map_or_else(
                || source_directory(&self.source),
                |(_, directory)| directory,
            )
    }

    fn placeholder(&self, text: &str) -> Inline {
        let format = if self.ext(Extension::RebaseRelativePaths) {
            Format::new(format!("{PLACEHOLDER}{}", self.source_directory()))
        } else {
            Format::new(PLACEHOLDER)
        };
        Inline::RawInline(format, text.into())
    }

    pub(crate) fn rebase_inline_target(&self, target: Text) -> Text {
        rebase_target(target, &self.inline_directory)
    }

    pub(crate) fn ext(&self, extension: Extension) -> bool {
        self.exts.contains(extension)
    }

    pub(crate) fn warn(&mut self, message: LogMessage) {
        self.log.push(message);
    }

    pub(crate) fn take_log(&mut self) -> Vec<LogMessage> {
        std::mem::take(&mut self.log)
    }

    fn pos(&self, line: u32) -> SourcePos {
        SourcePos::new(self.source.clone(), line, 1)
    }

    /// Parses a whole document.
    pub(crate) fn parse_document(&mut self, text: &str) -> Result<Pandoc> {
        let tab_stop = self.tab_stop;
        let mut lines: Vec<Ln> = Vec::new();
        let body = text.strip_suffix('\n').unwrap_or(text);
        if !text.is_empty() {
            for (i, line) in body.split('\n').enumerate() {
                let n = u32::try_from(i + 1).unwrap_or(u32::MAX);
                lines.push(Ln::new(expand_tabs(line, tab_stop), n));
            }
        }
        let count = u32::try_from(lines.len()).unwrap_or(u32::MAX);
        // Pandoc reports the end of the input after the last non-blank line.
        let content = lines
            .iter()
            .rposition(|l| !l.is_blank())
            .map_or(0, |k| k + 1);
        self.end_line = u32::try_from(content).unwrap_or(u32::MAX).saturating_add(3);
        lines.push(Ln::new("", count.saturating_add(1)));
        lines.push(Ln::new("", count.saturating_add(2)));
        let mut start = 0;
        if self.ext(Extension::PandocTitleBlock) {
            start = self.title_block(&lines);
        }
        if start == 0 && self.ext(Extension::MmdTitleBlock) {
            start = self.mmd_title_block(&lines);
        }
        let rest = lines.get(start..).unwrap_or(&[]);
        let blocks = self.blocks(rest)?;
        // The metadata text is read after all references are known, but
        // before the text of the body: its headings, notes and citations
        // count first.
        let mut meta = std::mem::take(&mut self.meta);
        for value in meta.0.values_mut() {
            self.assign_meta_header_ids(value);
        }
        let mut blocks = blocks;
        self.assign_header_ids(&mut blocks);
        for (key, value) in std::mem::take(&mut meta.0) {
            let value = self.resolve_meta(value);
            meta.insert(key, value);
        }
        let blocks = self.resolve_document(blocks);
        let mut doc = Pandoc { meta, blocks };
        if self.ext(Extension::Smart) {
            let mark = format!("{}.", crate::inline::ESCAPED_PERIOD);
            randoc_ast::walk::walk_inlines_mut(&mut doc, |x| {
                if let Inline::Str(s) = x
                    && s.contains(&mark)
                {
                    *s = s.replace(&mark, ".");
                }
            });
        }
        Ok(doc)
    }

    fn title_block(&mut self, lines: &[Ln]) -> usize {
        let mut fields: Vec<Text> = Vec::new();
        let mut i = 0;
        while let Some(line) = lines.get(i) {
            let Some(rest) = line.t().strip_prefix('%') else {
                break;
            };
            let mut field = rest.trim_matches(SPACE).to_string();
            i += 1;
            while let Some(next) = lines.get(i) {
                if next.t().starts_with(' ') && !next.is_blank() {
                    field.push('\n');
                    field.push_str(next.t().trim_matches(SPACE));
                    i += 1;
                } else {
                    break;
                }
            }
            fields.push(field);
        }
        if fields.is_empty() || fields.len() > 3 {
            return 0;
        }
        while lines.get(i).is_some_and(|l| l.is_blank()) {
            i += 1;
        }
        let mut it = fields.into_iter();
        if let Some(title) = it.next()
            && !title.is_empty()
        {
            let value = MetaValue::MetaInlines(vec![self.placeholder(&title)]);
            self.meta.insert("title", value);
        }
        if let Some(authors) = it.next() {
            let list = title_block_authors(&authors)
                .map(|a| MetaValue::MetaInlines(vec![self.placeholder(a)]))
                .collect();
            self.meta.insert("author", MetaValue::MetaList(list));
        }
        if let Some(date) = it.next()
            && !date.is_empty()
        {
            self.meta.insert(
                "date",
                MetaValue::MetaInlines(vec![self.placeholder(&date)]),
            );
        }
        i
    }

    fn mmd_title_block(&mut self, lines: &[Ln]) -> usize {
        let mut i = 0;
        let mut fields: Vec<(Text, Text)> = Vec::new();
        while let Some(line) = lines.get(i) {
            if line.is_blank() {
                break;
            }
            if line.t().starts_with([' ', '\t']) {
                if let Some(last) = fields.last_mut() {
                    last.1.push('\n');
                    last.1.push_str(line.t().trim_matches(SPACE));
                    i += 1;
                    continue;
                }
                return 0;
            }
            let Some((key, value)) = line.t().split_once(':') else {
                return 0;
            };
            if key.is_empty()
                || !key
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == ' ' || c == '_' || c == '-')
            {
                return 0;
            }
            let key: Text = key
                .to_lowercase()
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            fields.push((key, value.trim_matches(SPACE).to_string()));
            i += 1;
        }
        if fields.is_empty() {
            return 0;
        }
        for (key, value) in fields {
            self.meta
                .insert(key, MetaValue::MetaInlines(vec![self.placeholder(&value)]));
        }
        i
    }

    /// Parses lines into blocks.
    pub(crate) fn blocks(&mut self, lines: &[Ln]) -> Result<Vec<Block>> {
        self.blocks_open(lines, false)
    }

    /// Parses lines into blocks. With `end_blank`, the end of the lines
    /// counts as a blank line, so a last paragraph is a `Para`.
    fn blocks_open(&mut self, lines: &[Ln], end_blank: bool) -> Result<Vec<Block>> {
        let saved = std::mem::replace(&mut self.end_blank, end_blank);
        let result = self.blocks_limited(lines);
        self.end_blank = saved;
        result
    }

    fn blocks_limited(&mut self, lines: &[Ln]) -> Result<Vec<Block>> {
        // Only this loop gobbles: a container inside it starts at zero again.
        let gobble = std::mem::take(&mut self.gobble);
        if self.depth >= self.max_depth {
            let text: Vec<&str> = lines.iter().map(|l| l.t()).collect();
            let text = text.join("\n");
            if blank(&text) {
                return Ok(Vec::new());
            }
            return Ok(vec![Block::Para(vec![Inline::Str(
                text.trim_matches(SPACE).to_string(),
            )])]);
        }
        self.depth += 1;
        let result = self.blocks_inner(lines, gobble);
        self.depth -= 1;
        result
    }

    /// Returns the token index for `lines`: a part of the index of the
    /// slice that `blocks` parses now when it can, or a new one.
    fn tokens_for(&self, lines: &[Ln]) -> Tokens {
        self.token_scopes
            .last()
            .and_then(|scope| scope.part(lines))
            .unwrap_or_else(|| Tokens {
                base: lines.as_ptr() as usize,
                len: lines.len(),
                ..Tokens::default()
            })
    }

    /// Reads the token index of the slice that `blocks` parses now up to
    /// line `j` of `lines`, a part of that slice.
    fn cover_tokens(&self, lines: &[Ln], j: usize) {
        if let Some(part) = self.token_scopes.last().and_then(|scope| scope.part(lines)) {
            part.read_through(lines, j);
        }
    }

    /// Makes the failed-search memory of `lines`.
    fn fence_memo(&self, lines: &[Ln]) -> FenceMemo {
        FenceMemo {
            tokens: self.tokens_for(lines),
            ..FenceMemo::default()
        }
    }

    /// Parses lines into blocks. Each block can start after up to `gobble` spaces.
    fn blocks_inner(&mut self, lines: &[Ln], gobble: usize) -> Result<Vec<Block>> {
        let mut fences = self.fence_memo(lines);
        self.token_scopes.push(fences.tokens.clone());
        let line = self.source_line;
        let result = self.blocks_in_scope(lines, gobble, &mut fences);
        self.source_line = line;
        self.token_scopes.pop();
        result
    }

    fn blocks_in_scope(
        &mut self,
        lines: &[Ln],
        gobble: usize,
        fences: &mut FenceMemo,
    ) -> Result<Vec<Block>> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            crate::step(1);
            let Some(line) = lines.get(i) else { break };
            if line.is_blank() {
                i += 1;
                continue;
            }
            line.gobble(gobble);
            let prev_blank = i == 0 || lines.get(i - 1).is_some_and(|l| l.is_blank());
            fences.tokens.read_through(lines, i);
            let used = self.block_at(lines, i, prev_blank, fences, &mut out)?;
            i += used.max(1);
        }
        Ok(out)
    }

    fn block_at(
        &mut self,
        lines: &[Ln],
        i: usize,
        prev_blank: bool,
        fences: &mut FenceMemo,
        out: &mut Vec<Block>,
    ) -> Result<usize> {
        let armed = std::mem::replace(&mut self.rest, RestHandoff::Off) == RestHandoff::Armed;
        let rest = lines.get(i..).unwrap_or(&[]);
        let Some(line) = rest.first() else {
            return Ok(1);
        };
        if line.n != 0 {
            self.source_line = line.n;
        }
        let s = line.t();
        if let Some(n) = self.fenced_code(rest, i, fences, out) {
            return Ok(n);
        }
        if prev_blank
            && self.ext(Extension::YamlMetadataBlock)
            && let Some(n) = self.yaml_block(rest)?
        {
            return Ok(n);
        }
        if let Some(n) = self.literate_code(rest, out) {
            return Ok(n);
        }
        if let Some(n) = self.list(rest, true, out)? {
            return Ok(n);
        }
        if armed {
            self.rest = RestHandoff::Html;
        }
        let html = self.html_block(rest, out);
        if self.rest == RestHandoff::Html {
            self.rest = RestHandoff::Off;
        }
        if let Some(n) = html? {
            return Ok(n);
        }
        if let Some(n) = self.fenced_div(rest, out)? {
            return Ok(n);
        }
        if let Some(n) = self.header(lines, i, fences, out) {
            return Ok(n);
        }
        if let Some(n) = self.table(rest, out) {
            return Ok(n);
        }
        if let Some(n) = self.line_block(rest, out) {
            return Ok(n);
        }
        if indent(s) >= 4 {
            return Ok(self.indented_code(rest, out));
        }
        if let Some(n) = self.raw_tex_block(rest, out)? {
            return Ok(n);
        }
        if let Some(n) = self.block_quote(rest, out)? {
            return Ok(n);
        }
        if is_hrule(s) {
            out.push(Block::HorizontalRule);
            return Ok(1);
        }
        if let Some(n) = self.list(rest, false, out)? {
            return Ok(n);
        }
        if let Some(n) = self.definition_list(rest, out)? {
            return Ok(n);
        }
        if let Some(n) = self.note_definition(rest)? {
            return Ok(n);
        }
        if let Some(n) = self.reference(rest) {
            return Ok(n);
        }
        if let Some(n) = self.abbreviation(rest) {
            return Ok(n);
        }
        self.paragraph(lines, i, fences, out)
    }

    // ----- paragraphs -----

    /// Returns the kind of paragraph that the line `s` ends, or `None` when the paragraph continues.
    fn interrupts(&self, s: &str) -> Option<bool> {
        if !self.ext(Extension::BlankBeforeBlockquote)
            && s.trim_start_matches(SPACE).starts_with('>')
        {
            return Some(true);
        }
        if !self.ext(Extension::BlankBeforeHeader) && self.atx(s).is_some() {
            return Some(true);
        }
        if self.html_interrupts(s) {
            return Some(false);
        }
        // A list marker ends a paragraph of an item: a paragraph of a
        // loose list, as all the others in it.
        if self.in_list
            && indent(s) <= 3
            && !is_hrule(s)
            && (self.marker(s, None).is_some() || self.foreign_marker(s))
        {
            return Some(self.loose_list);
        }
        // In a list item, a definition marker ends the paragraph, also with
        // `definition_lists` off. With no term before it, the marker line is
        // then a paragraph of its own.
        if self.in_list && self.def_marker(s).is_some() {
            return Some(false);
        }
        // The marker forms of `fancy_lists` end the paragraph also when
        // that extension is off.
        if self.ext(Extension::ListsWithoutPrecedingBlankline)
            && indent(s) <= 3
            && !is_hrule(s)
            && (self.marker(s, None).is_some()
                || self.foreign_marker(s)
                || self.def_marker(s).is_some())
        {
            return Some(true);
        }
        None
    }

    /// Tells whether the line `s` starts a block that needs a line break at
    /// the end of the line: a list item, an ATX heading, a line block or a
    /// horizontal rule.
    fn needs_line_end(&self, s: &str) -> bool {
        let item = indent(s) <= 3
            && !is_hrule(s)
            && (self.marker(s, None).is_some() || self.foreign_marker(s));
        let line_block = self.ext(Extension::LineBlocks) && (s == "|" || s.starts_with("| "));
        item || self.atx(s).is_some() || line_block || is_hrule(s)
    }

    /// Tells whether a line without a quote marker ends a block quote. A
    /// heading, a list marker or a definition marker that can end a
    /// paragraph does; a block tag does not.
    fn ends_lazy_quote(&self, s: &str) -> bool {
        (!self.ext(Extension::BlankBeforeHeader) && self.atx(s).is_some())
            || (self.ext(Extension::ListsWithoutPrecedingBlankline)
                && indent(s) <= 3
                && !is_hrule(s)
                && (self.marker(s, None).is_some()
                    || self.foreign_marker(s)
                    || self.def_marker(s).is_some()))
    }

    /// Tells whether a block tag ends a paragraph. Its indentation does not
    /// matter. With neither `markdown_in_html_blocks` nor
    /// `markdown_attribute`, a tag is text of the paragraph.
    fn html_interrupts(&self, s: &str) -> bool {
        self.ext(Extension::RawHtml)
            && (self.ext(Extension::MarkdownInHtmlBlocks) || self.ext(Extension::MarkdownAttribute))
            && is_block_tag(s.trim_start_matches(SPACE))
    }

    /// Parses a paragraph at line `at` of `all`, the slice of `fences`.
    fn paragraph(
        &mut self,
        all: &[Ln],
        at: usize,
        fences: &mut FenceMemo,
        out: &mut Vec<Block>,
    ) -> Result<usize> {
        let lines = all.get(at..).unwrap_or(&[]);
        let mut j = 1;
        let mut para = None;
        // The paragraph text ends at the first line with a block tag after
        // text, so that the text after the tag is not read again.
        let has_tag = |p: &Self, line: &Ln| p.inline_block_tag(line.t()).is_some();
        let mut tagged = lines.first().is_some_and(|l| has_tag(self, l));
        // The first line of the text that has no open comment yet.
        let mut scan_from = 0;
        let rules = self.token_rules(false);
        let mut covered = fences.tokens.reach(all, at, rules) - at;
        while let Some(line) = lines.get(j).filter(|_| !tagged) {
            crate::step(1);
            let inside = j <= covered;
            if line.is_blank() && !inside {
                // A comment or a processing instruction that is open at the
                // blank line continues the paragraph to the line of its end.
                if let Some(m) = self.open_raw_end(lines, scan_from..j, at, fences) {
                    scan_from = m;
                    tagged = lines.get(m).is_some_and(|l| has_tag(self, l));
                    j = m + 1;
                    continue;
                }
                // So does a TeX environment that is open there.
                if let Some(m) = self.open_tex_end(lines, scan_from..j, at, fences) {
                    scan_from = m;
                    j = m + 1;
                    continue;
                }
                para = Some(true);
                break;
            }
            // Only a backtick fence with no space before it ends a paragraph.
            // A tilde fence or an indented fence is text of the paragraph,
            // and so is a fence line that closes a code span of the text.
            if !inside
                && line.t().starts_with('`')
                && self
                    .fenced_code(
                        lines.get(j..).unwrap_or(&[]),
                        at + j,
                        fences,
                        &mut Vec::new(),
                    )
                    .is_some()
            {
                para = Some(true);
                break;
            }
            if !inside && let Some(kind) = self.interrupts(line.t()) {
                para = Some(kind);
                break;
            }
            tagged = !inside && has_tag(self, line);
            if !inside {
                covered = fences.tokens.reach(all, at + j, rules) - at;
            }
            j += 1;
        }
        let text: Vec<&str> = lines
            .get(..j)
            .unwrap_or(&[])
            .iter()
            .map(|l| l.t())
            .collect();
        let text = text.join("\n");
        if let Some(k) = self.inline_block_tag(&text)
            && let Some(n) = self.paragraph_before_tag(lines, &text, k, out)?
        {
            return Ok(n);
        }
        let inlines = vec![self.placeholder(text.trim_start_matches(SPACE))];
        if para.unwrap_or(self.end_blank) {
            out.push(Block::Para(inlines));
        } else {
            out.push(Block::Plain(inlines));
        }
        // The block tag that ended the paragraph starts an HTML block, also
        // when it is indented by 4 spaces or more.
        if let Some(rest) = lines.get(j..)
            && rest.first().is_some_and(|l| self.html_interrupts(l.t()))
            && let Some(n) = self.html_block_at(rest, out)?
        {
            return Ok(j + n);
        }
        Ok(j)
    }

    /// Returns the index of the line that ends a comment or a processing
    /// instruction that the paragraph lines `range` leave open. `at` is the
    /// index of `lines` in the slice of `fences`.
    fn open_raw_end(
        &self,
        lines: &[Ln],
        range: Range<usize>,
        at: usize,
        fences: &mut FenceMemo,
    ) -> Option<usize> {
        if !self.ext(Extension::RawHtml) {
            return None;
        }
        let end = range.end;
        let text: Vec<&str> = lines.get(range)?.iter().map(|l| l.t()).collect();
        let kind = open_comment_or_pi(&text.join("\n"))?;
        let slot = fences.raw_ends.get_mut(kind)?;
        let from = at + end + 1;
        if let Some((start, found)) = *slot
            && from >= start
            && found.is_none_or(|m| m >= from)
        {
            return found.map(|m| m - at);
        }
        let closer = if kind == 0 { "-->" } else { ">" };
        let found = lines
            .get(end + 1..)?
            .iter()
            .position(|l| {
                crate::step(1);
                l.t().contains(closer)
            })
            .map(|k| at + end + 1 + k);
        *slot = Some((from, found));
        found.map(|m| m - at)
    }

    /// Returns the index of the line that ends a TeX environment that the
    /// paragraph lines `range` leave open. `at` is the index of `lines` in
    /// the slice of `fences`.
    fn open_tex_end(
        &self,
        lines: &[Ln],
        range: Range<usize>,
        at: usize,
        fences: &mut FenceMemo,
    ) -> Option<usize> {
        if !self.ext(Extension::RawTex) {
            return None;
        }
        let end = range.end;
        let text: Vec<&str> = lines.get(range)?.iter().map(|l| l.t()).collect();
        let text = text.join("\n");
        crate::step(text.len());
        let name = text.match_indices("\\begin{").find_map(|(k, _)| {
            let rest = text.get(k + 7..)?;
            let name = rest.get(..rest.find('}')?)?;
            let marker = format!("\\end{{{name}}}");
            (!name.is_empty() && !rest.contains(&marker)).then(|| name.to_string())
        })?;
        let marker = format!("\\end{{{name}}}");
        let from = at + end + 1;
        if let Some(&(start, found)) = fences.tex_ends.get(&name)
            && from >= start
            && found.is_none_or(|m| m >= from)
        {
            return found.map(|m| m - at);
        }
        let found = lines
            .get(end + 1..)?
            .iter()
            .position(|l| {
                crate::step(1 + l.t().len());
                l.t().contains(&marker)
            })
            .map(|k| at + end + 1 + k);
        fences.tex_ends.insert(name, (from, found));
        found.map(|m| m - at)
    }

    /// Tells whether `s` starts with a block tag that ends the inline text
    /// of a bracket: the inline parse of Pandoc stops at it.
    pub(crate) fn ends_inline_text(&self, s: &str) -> bool {
        self.ext(Extension::RawHtml)
            && (self.ext(Extension::MarkdownInHtmlBlocks) || self.ext(Extension::MarkdownAttribute))
            && is_mid_line_block_tag(s)
    }

    /// Returns the byte offset in the paragraph text `text` of the first
    /// block tag after the start that is not in a code span. The inline
    /// parse of Pandoc stops there, so the paragraph ends before the tag.
    fn inline_block_tag(&self, text: &str) -> Option<usize> {
        let html = self.ext(Extension::RawHtml)
            && (self.ext(Extension::MarkdownInHtmlBlocks)
                || self.ext(Extension::MarkdownAttribute));
        let tex = self.ext(Extension::RawTex) && text.contains('\\');
        if self.split_budget == 0 || !(html || tex) {
            return None;
        }
        // The last position of each environment end of the text.
        let mut environment_ends = None;
        let bytes = text.as_bytes();
        let brackets = matching_brackets(bytes);
        let lines: Vec<Ln> = text.split('\n').map(|t| Ln::new(t, 0)).collect();
        let mut offsets = Vec::with_capacity(lines.len());
        let mut offset = 0;
        for line in &lines {
            offsets.push(offset);
            offset += line.t().len() + 1;
        }
        let view = View {
            lines: &lines,
            at: 0,
        };
        let mut tokens = TokenIndex::default();
        let mut i = 0;
        let mut j = 0;
        while let Some(&b) = bytes.get(i) {
            crate::step(1);
            while offsets.get(j + 1).is_some_and(|start| *start <= i) {
                j += 1;
            }
            let here = (j, i - offsets[j]);
            let absolute = |(line, byte): (usize, usize)| offsets[line] + byte;
            if let Some(end) = tokens.end(view, here, self.token_rules(true)) {
                i = absolute(end);
                continue;
            }
            match b {
                b'\\' => {
                    if tex && i > 0 && tex_block_at(text, i, &mut environment_ends) {
                        return Some(i);
                    }
                    i += 2;
                }
                // A bracket that a link destination follows is read at
                // its closing bracket.
                b'[' => i = brackets.get(&i).copied().unwrap_or(i + 1),
                b'<' => {
                    let rest = text.get(i..)?;
                    if html && i > 0 && is_mid_line_block_tag(rest) {
                        return Some(i);
                    }
                    i = tokens.tag_end(view, here).map_or(i + 1, absolute);
                }
                _ => i += 1,
            }
        }
        None
    }

    /// Ends the paragraph of `lines` at the byte offset `k` of its text, where
    /// a block tag or raw TeX of a block starts, and reads it and the rest
    /// as blocks.
    fn paragraph_before_tag(
        &mut self,
        lines: &[Ln],
        text: &str,
        k: usize,
        out: &mut Vec<Block>,
    ) -> Result<Option<usize>> {
        let before = text.get(..k).unwrap_or("");
        let l = before.matches('\n').count();
        let col = k - before.rfind('\n').map_or(0, |p| p + 1);
        let (Some(line), Some(rest)) = (lines.get(l), lines.get(l..)) else {
            return Ok(None);
        };
        // The blocks after the tag read the rest of the line again. A
        // budget keeps the time linear for a long line with many tags.
        let again = line.t().len().saturating_sub(col);
        if again > self.split_budget {
            self.split_budget = 0;
            return Ok(None);
        }
        self.split_budget -= again;
        // The tag line starts at the tag for the HTML block, with no copy of
        // the lines after it.
        let saved = line.skip_to(col);
        let mut blocks = Vec::new();
        let parsed = if text.get(k..).is_some_and(|t| t.starts_with('\\')) {
            self.raw_tex_block(rest, &mut blocks)
        } else {
            self.html_block_at(rest, &mut blocks)
        };
        line.restore(saved);
        let Some(n) = parsed? else {
            return Ok(None);
        };
        out.push(Block::Plain(vec![
            self.placeholder(before.trim_matches(SPACE)),
        ]));
        out.extend(blocks);
        Ok(Some(l + n))
    }

    // ----- code -----

    /// Parses a fenced code block at `lines`, which start at line `at` of
    /// the slice of `fences`.
    fn fenced_code(
        &mut self,
        lines: &[Ln],
        at: usize,
        fences: &mut FenceMemo,
        out: &mut Vec<Block>,
    ) -> Option<usize> {
        let first = lines.first()?;
        let (c, n, ind, info) = fence_open(first.t())?;
        let (from, end) = (
            fences.tokens.offset + at,
            fences.tokens.offset + at + lines.len(),
        );
        if fences
            .tokens
            .fences
            .borrow()
            .covers(FenceSearch::Block, c, from, n, end)
        {
            #[cfg(test)]
            crate::check_cache(&false, || {
                lines.iter().skip(1).any(|l| closes_code_fence(l.t(), c, n))
            });
            return None;
        }
        let enabled = if c == '`' {
            self.ext(Extension::BacktickCodeBlocks)
        } else {
            self.ext(Extension::FencedCodeBlocks)
        };
        if !enabled {
            return None;
        }
        let info = info.trim_matches(SPACE);
        let mut raw_format = None;
        let attr = if info.is_empty() {
            Attr::default()
        } else if info.starts_with('{') {
            if let Some(format) = self.raw_attribute(info) {
                raw_format = Some(format);
                Attr::default()
            } else if self.ext(Extension::FencedCodeAttributes) {
                self.parse_attr(info)?
            } else {
                return None;
            }
        } else {
            let word = info.split_whitespace().next().unwrap_or("");
            let after = info.get(word.len()..).unwrap_or("").trim_matches(SPACE);
            let mut attr = if after.is_empty() {
                Attr::default()
            } else if after.starts_with('{') && self.ext(Extension::FencedCodeAttributes) {
                self.parse_attr(after)?
            } else {
                return None;
            };
            attr.classes.insert(0, language_id(word));
            attr
        };
        let mut body = Vec::new();
        let mut j = 1;
        let mut closed = false;
        while let Some(line) = lines.get(j) {
            crate::step(1);
            let t = line.t();
            if closes_code_fence(t, c, n) {
                closed = true;
                j += 1;
                break;
            }
            body.push(dedent(t, ind));
            j += 1;
        }
        if !closed {
            let mut failures = fences.tokens.fences.borrow_mut();
            failures.record(FenceSearch::Block, c, from, n, end);
            return None;
        }
        let text = body.join("\n");
        match raw_format {
            Some(format) => out.push(Block::RawBlock(Format::new(format), text)),
            None => out.push(Block::CodeBlock(attr, text)),
        }
        Some(j)
    }

    fn raw_attribute(&self, info: &str) -> Option<Text> {
        if !self.ext(Extension::RawAttribute) {
            return None;
        }
        let inner = info
            .strip_prefix('{')?
            .strip_suffix('}')?
            .trim_matches(SPACE);
        let name = inner.strip_prefix('=')?;
        (!name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-'))
        .then(|| name.to_string())
    }

    fn indented_code(&mut self, lines: &[Ln], out: &mut Vec<Block>) -> usize {
        let mut body: Vec<&str> = Vec::new();
        let mut j = 0;
        let mut last_text = 0;
        while let Some(line) = lines.get(j) {
            crate::step(1);
            if line.is_blank() {
                body.push(dedent(line.t(), 4));
                j += 1;
                continue;
            }
            if line.indent() < 4 {
                break;
            }
            body.push(dedent(line.t(), 4));
            j += 1;
            last_text = j;
        }
        body.truncate(last_text);
        let attr = Attr::new("", self.indented_code_classes.clone(), vec![]);
        out.push(Block::CodeBlock(attr, body.join("\n")));
        last_text
    }

    // ----- headers -----

    fn atx(&self, s: &str) -> Option<(i64, Text)> {
        let t = trim_end_unescaped(s, &SPACE);
        let level = t.len() - t.trim_start_matches('#').len();
        if level == 0 || indent(s) > 0 {
            return None;
        }
        let rest = t.get(level..)?;
        if self.ext(Extension::SpaceInAtxHeader) && !rest.is_empty() && !rest.starts_with(' ') {
            return None;
        }
        Some((
            i64::try_from(level).unwrap_or(1),
            rest.trim_start_matches(SPACE).to_string(),
        ))
    }

    fn token_rules(&self, comment_over_blank: bool) -> TokenRules {
        TokenRules::from_extensions(self.exts, comment_over_blank)
    }

    fn header(
        &mut self,
        all: &[Ln],
        at: usize,
        fences: &mut FenceMemo,
        out: &mut Vec<Block>,
    ) -> Option<usize> {
        let lines = all.get(at..)?;
        let first = lines.first()?;
        let (level, content, used) = if let Some((level, content)) = self.atx(first.t()) {
            // A block tag in the text makes the line a paragraph.
            if self.inline_block_tag(&content).is_some() {
                return None;
            }
            // A code span that opens in the heading and closes on a later
            // line of the same block holds those lines too.
            let extra = fences.tokens.reach(all, at, self.token_rules(true)) - at;
            if extra == 0 {
                (level, content, 1)
            } else {
                let mut text = content;
                for line in lines.get(1..=extra).unwrap_or(&[]) {
                    text.push('\n');
                    text.push_str(line.t().trim_start_matches(SPACE));
                }
                (level, text.trim_end_matches(SPACE).to_string(), 1 + extra)
            }
        } else {
            let next = lines.get(1)?;
            let level = match is_setext_underline(next.t()).then(|| next.t().chars().next()) {
                Some(Some('=')) => 1,
                Some(_) => 2,
                None => return None,
            };
            if first.indent() >= 4 || first.t().trim_start_matches(SPACE).starts_with('>') {
                return None;
            }
            (
                level,
                trim_end_unescaped(first.t().trim_start_matches(SPACE), &SPACE).to_string(),
                2,
            )
        };
        let atx = used == 1 || self.atx(first.t()).is_some();
        let (text, attr) = self.header_attr(&content, atx);
        let text = if atx {
            strip_closing_hashes(&text).to_string()
        } else {
            text
        };
        let mut j = used;
        while lines.get(j).is_some_and(|l| l.is_blank()) {
            j += 1;
        }
        let next_line = lines.get(j).map_or(self.end_line, |l| l.n);
        let mut attr = attr;
        attr.attributes
            .push(("\u{0}line".into(), next_line.to_string()));
        attr.attributes.push(("\u{0}raw".into(), text.clone()));
        out.push(Block::Header(level, attr, vec![self.placeholder(&text)]));
        Some(used)
    }

    /// Tells whether an inline construct at the end of `before` takes the
    /// attributes that follow it: a code span, a bracketed span, or a link
    /// or an image.
    fn inline_takes_attr(&self, before: &str) -> bool {
        (before.ends_with('`') && self.ext(Extension::InlineCodeAttributes))
            || (before.ends_with(']')
                && (self.ext(Extension::BracketedSpans) || self.ext(Extension::LinkAttributes)))
            || (before.ends_with(')')
                && before.contains("](")
                && self.ext(Extension::LinkAttributes))
    }

    /// Splits the attributes from the text of a heading. An attribute
    /// block must end the text before closing hashes; an identifier of
    /// `mmd_header_identifiers` must end it after them.
    fn header_attr(&self, content: &str, atx: bool) -> (Text, Attr) {
        if self.ext(Extension::HeaderAttributes)
            && content.ends_with('}')
            && let Some(open) = content.rfind('{')
            && !self.inline_takes_attr(content.get(..open).unwrap_or(""))
            && let Some(attr) = self.parse_attr(content.get(open..).unwrap_or(""))
        {
            let text = trim_end_unescaped(content.get(..open).unwrap_or(""), &SPACE);
            return (text.to_string(), attr);
        }
        let mmd = if atx {
            strip_closing_hashes(content)
        } else {
            content
        };
        if self.ext(Extension::MmdHeaderIdentifiers)
            && mmd.ends_with(']')
            && let Some(open) = mmd.rfind('[')
            && !ends_with_escape(mmd.get(..open).unwrap_or(""))
        {
            let id = mmd.get(open + 1..mmd.len() - 1).unwrap_or("");
            if !id.is_empty() && !id.contains([' ', '[', ']']) {
                let text = mmd.get(..open).unwrap_or("").trim_end_matches(SPACE);
                return (text.to_string(), Attr::new(id, vec![], vec![]));
            }
        }
        (content.to_string(), Attr::default())
    }

    // ----- block quotes -----

    fn block_quote(&mut self, lines: &[Ln], out: &mut Vec<Block>) -> Result<Option<usize>> {
        fn quote_start(s: &str) -> Option<&str> {
            if indent(s) > 3 {
                return None;
            }
            let t = s.trim_start_matches(SPACE).strip_prefix('>')?;
            Some(t.strip_prefix(' ').unwrap_or(t))
        }
        let Some(first) = lines.first() else {
            return Ok(None);
        };
        let Some(text) = quote_start(first.t()) else {
            return Ok(None);
        };
        let single_line = lines.get(1).is_none_or(|l| l.is_blank());
        if single_line && !self.ext(Extension::Alerts) && quote_start(text).is_some() {
            // Nested markers on one line: strip them all at once, so that
            // the line is not copied again for each level.
            let mut levels = 0usize;
            let mut rest = first.t();
            let budget = self.max_depth.saturating_sub(self.depth).max(1);
            while levels < budget
                && indent(rest) <= 3
                && let Some(after) = rest.trim_start_matches(SPACE).strip_prefix('>')
            {
                rest = after.strip_prefix(' ').unwrap_or(after);
                levels += 1;
            }
            let inner = [Ln::new(rest, first.n), Ln::new("", 0)];
            let saved = std::mem::replace(&mut self.in_list, false);
            // Each stripped level counts toward the nesting limit.
            self.depth += levels.saturating_sub(1);
            let blocks = self.blocks(&inner);
            self.depth -= levels.saturating_sub(1);
            self.in_list = saved;
            let mut blocks = blocks?;
            for _ in 0..levels {
                blocks = vec![Block::BlockQuote(blocks)];
            }
            out.extend(blocks);
            return Ok(Some(1));
        }
        let mut inner = vec![first.tail(text, first.n)];
        let saved = std::mem::replace(&mut self.in_list, false);
        let mut fences = FenceMemo::default();
        let mut j = 1;
        while let Some(line) = lines.get(j) {
            crate::step(1);
            if line.is_blank() {
                break;
            }
            match quote_start(line.t()) {
                Some(text) => inner.push(line.tail(text, line.n)),
                None if self.ends_lazy_quote(line.t()) => break,
                // In a list item, a list marker ends the quote as it ends a
                // paragraph of the item.
                None if saved
                    && indent(line.t()) <= 3
                    && !is_hrule(line.t())
                    && (self.marker(line.t(), None).is_some() || self.foreign_marker(line.t())) =>
                {
                    break;
                }
                // A backtick fence ends a paragraph, so it ends the quote.
                None if line.t().starts_with('`')
                    && self
                        .fenced_code(
                            lines.get(j..).unwrap_or(&[]),
                            j,
                            &mut fences,
                            &mut Vec::new(),
                        )
                        .is_some() =>
                {
                    break;
                }
                None => inner.push(line.clone()),
            }
            j += 1;
        }
        inner.push(Ln::new("", 0));
        let alert = if self.ext(Extension::Alerts) {
            inner
                .first()
                .and_then(|l| alert_kind(l.t().trim_matches(SPACE)))
        } else {
            None
        };
        if alert.is_some() {
            inner.remove(0);
        }
        let blocks = self.blocks(&inner);
        self.in_list = saved;
        let blocks = blocks?;
        match alert {
            Some((class, title)) => {
                let mut content = vec![Block::Div(
                    Attr::new("", vec!["title".into()], vec![]),
                    vec![Block::Para(vec![Inline::Str(title.into())])],
                )];
                content.extend(blocks);
                out.push(Block::Div(
                    Attr::new("", vec![class.into(), "alert".into()], vec![]),
                    content,
                ));
            }
            None => out.push(Block::BlockQuote(blocks)),
        }
        Ok(Some(j))
    }

    // ----- lists -----

    fn marker(&self, s: &str, expected: Option<ListKind>) -> Option<Marker> {
        self.marker_with(s, expected, self.ext(Extension::FancyLists))
    }

    /// Tells whether `s` starts with a marker of `fancy_lists` or
    /// `example_lists` that the flavor does not read as a list marker. Such
    /// a line ends a list item, and with `lists_without_preceding_blankline`
    /// a paragraph. A number with one parenthesis (`1)`) is no such marker.
    fn foreign_marker(&self, s: &str) -> bool {
        if indent(s) > 3 || self.marker(s, None).is_some() {
            return false;
        }
        let fancy = !self.ext(Extension::FancyLists)
            && self.marker_with(s, None, true).is_some_and(|m| {
                m.kind != ListKind::Ordered(ListNumberStyle::Decimal, ListNumberDelim::OneParen)
            });
        fancy
            || (!self.ext(Extension::ExampleLists)
                && is_example_marker(s.trim_start_matches(SPACE)))
    }

    /// Reads a list marker; `fancy` allows the markers of `fancy_lists`.
    fn marker_with(&self, s: &str, expected: Option<ListKind>, fancy: bool) -> Option<Marker> {
        let ind = indent(s);
        let t = s.get(ind..)?;
        let mut chars = t.char_indices();
        let (_, c0) = chars.next()?;
        let (kind, start, label, width) = if matches!(c0, '*' | '+' | '-') {
            if expected.is_some_and(|k| k != ListKind::Bullet) {
                return None;
            }
            (ListKind::Bullet, 1, None, 1)
        } else {
            if expected == Some(ListKind::Bullet) {
                return None;
            }
            self.ordered_marker(t, fancy, expected)?
        };
        let after = t.get(width..)?;
        let spaces = indent(after);
        let rest_text = after.get(spaces..)?;
        if !rest_text.is_empty() && spaces == 0 {
            return None;
        }
        if let ListKind::Ordered(
            ListNumberStyle::UpperAlpha | ListNumberStyle::UpperRoman,
            ListNumberDelim::Period,
        ) = kind
            && width == 2
            && spaces < 2
            && !rest_text.is_empty()
        {
            return None;
        }
        // `p. 3` is a page reference, not a list item, when `p.` is an
        // abbreviation (black-box tests: only `p` followed by one space and a
        // digit; `n. 3`, `p. a` and `p.  3` are list items).
        if let ListKind::Ordered(ListNumberStyle::LowerAlpha, ListNumberDelim::Period) = kind
            && t.starts_with("p.")
            && spaces == 1
            && rest_text.starts_with(|c: char| c.is_ascii_digit())
            && self.abbreviations.contains("p.")
        {
            return None;
        }
        let base = ind + width;
        let text_from = s.len() - rest_text.len();
        let (content, rest_from) = if rest_text.is_empty() {
            (base + 1, s.len())
        } else if spaces >= 5 {
            (base + 1, text_from - (spaces - 1))
        } else {
            (base + spaces, text_from)
        };
        let content = if self.ext(Extension::FourSpaceRule) {
            content.max(4)
        } else {
            content
        };
        let numbered = matches!(kind, ListKind::Ordered(ListNumberStyle::Example, _))
            && t.trim_start_matches('(')
                .starts_with(|c: char| c.is_ascii_digit());
        Some(Marker {
            kind,
            start,
            label,
            numbered,
            content,
            rest_from,
        })
    }

    fn ordered_marker(
        &self,
        t: &str,
        fancy: bool,
        expected: Option<ListKind>,
    ) -> Option<(ListKind, i64, Option<Text>, usize)> {
        let (paren, body) = match t.strip_prefix('(') {
            Some(rest) if fancy => (true, rest),
            _ => (false, t),
        };
        let token: Text = body
            .chars()
            .take_while(|c| {
                c.is_ascii_alphanumeric() || *c == '#' || *c == '@' || *c == '_' || *c == '-'
            })
            .collect();
        let token_len = token.len();
        let delim_char = body.get(token_len..)?.chars().next()?;
        let delim = match (paren, delim_char) {
            (true, ')') => ListNumberDelim::TwoParens,
            (false, ')') if fancy => ListNumberDelim::OneParen,
            (false, '.') => ListNumberDelim::Period,
            _ => return None,
        };
        let width = usize::from(paren) + token_len + 1;
        use ListNumberStyle::*;
        let mut candidates: Vec<(ListNumberStyle, i64, Option<Text>)> = Vec::new();
        if !token.is_empty() && token.chars().all(|c| c.is_ascii_digit()) {
            // Pandoc reads the number as a machine integer, which wraps.
            let n = token.bytes().fold(0i64, |acc, b| {
                acc.wrapping_mul(10).wrapping_add(i64::from(b - b'0'))
            });
            candidates.push((Decimal, n, None));
        } else if token == "#" && (fancy || delim == ListNumberDelim::Period) {
            candidates.push((DefaultStyle, 1, None));
        } else if !fancy {
            return None;
        } else if let Some((number, label)) = token.split_once('@')
            && number.bytes().all(|b| b.is_ascii_digit())
        {
            if !self.ext(Extension::ExampleLists)
                || !label
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                return None;
            }
            // A number before `@` sets the number of the example.
            let n = if number.is_empty() {
                1
            } else {
                number.bytes().fold(0i64, |acc, b| {
                    acc.wrapping_mul(10).wrapping_add(i64::from(b - b'0'))
                })
            };
            candidates.push((Example, n, Some(label.to_string())));
        } else {
            let mut chars = token.chars();
            let single = match (chars.next(), chars.next()) {
                (Some(c), None) => Some(c),
                _ => None,
            };
            if let Some(v) = roman_value(&token) {
                if token.chars().all(|c| c.is_ascii_lowercase()) {
                    candidates.push((LowerRoman, v, None));
                } else if token.chars().all(|c| c.is_ascii_uppercase()) {
                    candidates.push((UpperRoman, v, None));
                }
            }
            if let Some(c) = single {
                let v =
                    i64::from(u32::from(c.to_ascii_lowercase())) - i64::from(u32::from('a')) + 1;
                if c.is_ascii_lowercase() {
                    candidates.push((LowerAlpha, v, None));
                } else if c.is_ascii_uppercase() {
                    candidates.push((UpperAlpha, v, None));
                }
                if !matches!(c.to_ascii_lowercase(), 'i') {
                    candidates.reverse();
                }
            }
        }
        let style_of = |style: ListNumberStyle| {
            if fancy && style == DefaultStyle && delim == ListNumberDelim::Period {
                ListKind::Ordered(DefaultStyle, ListNumberDelim::DefaultDelim)
            } else if fancy {
                ListKind::Ordered(style, delim)
            } else {
                ListKind::Ordered(DefaultStyle, ListNumberDelim::DefaultDelim)
            }
        };
        let chosen = match expected {
            Some(kind) => candidates
                .into_iter()
                .find(|(style, _, _)| same_list(style_of(*style), kind))?,
            None => candidates.into_iter().next()?,
        };
        let (style, start, label) = chosen;
        let kind = style_of(style);
        Some((kind, start, label, width))
    }

    fn list(&mut self, lines: &[Ln], bullets: bool, out: &mut Vec<Block>) -> Result<Option<usize>> {
        let Some(first) = lines.first() else {
            return Ok(None);
        };
        if is_hrule(first.t()) || first.indent() > 3 {
            return Ok(None);
        }
        let Some(marker) = self.marker(first.t(), None) else {
            return Ok(None);
        };
        if (marker.kind == ListKind::Bullet) != bullets {
            return Ok(None);
        }
        // An ordered list inside a list item must start at 1.
        if self.in_list && matches!(marker.kind, ListKind::Ordered(..)) && marker.start != 1 {
            return Ok(None);
        }
        if let Some(n) = self.nested_list_line(lines, out)? {
            return Ok(Some(n));
        }
        let kind = marker.kind;
        let attrs = ListAttributes {
            start: if self.ext(Extension::Startnum) {
                marker.start
            } else {
                1
            },
            style: match kind {
                ListKind::Ordered(style, _) => style,
                ListKind::Bullet => ListNumberStyle::DefaultStyle,
            },
            delim: match kind {
                ListKind::Ordered(_, delim) => delim,
                ListKind::Bullet => ListNumberDelim::DefaultDelim,
            },
        };
        let mut items: Vec<Vec<Block>> = Vec::new();
        let mut raw_items: Vec<Vec<Ln>> = Vec::new();
        let mut fences = FenceMemo::default();
        let mut j = 0;
        let mut current = Some(marker);
        let mut first_example = None;
        while let Some(m) = current.take() {
            let start_line = lines.get(j).map_or(0, |l| l.n);
            if let ListKind::Ordered(ListNumberStyle::Example, _) = m.kind {
                let known = m
                    .label
                    .as_ref()
                    .and_then(|label| self.examples.get(label).copied());
                let number = match known {
                    Some(n) => n,
                    None => {
                        self.example_count = if m.numbered {
                            m.start
                        } else {
                            self.example_count.wrapping_add(1)
                        };
                        if let Some(label) = &m.label
                            && !label.is_empty()
                        {
                            self.examples.insert(label.clone(), self.example_count);
                        }
                        self.example_count
                    }
                };
                if first_example.is_none() {
                    first_example = Some(number);
                }
            }
            let item_text = lines
                .get(j)
                .and_then(|l| l.t().get(m.rest_from..))
                .unwrap_or("");
            let mut item = match lines.get(j) {
                Some(line) => vec![line.tail(item_text, start_line)],
                None => vec![Ln::new(item_text, start_line)],
            };
            j += 1;
            let mut prev_blank = false;
            let mut had_blank = false;
            while let Some(line) = lines.get(j) {
                crate::step(1);
                if line.is_blank() {
                    item.push(Ln::new("", line.n));
                    prev_blank = true;
                    had_blank = true;
                    j += 1;
                    continue;
                }
                let ind = line.indent();
                if ind >= m.content.min(4) {
                    item.push(line.dedent(m.content.min(4)));
                    prev_blank = false;
                    j += 1;
                    continue;
                }
                if !is_hrule(line.t())
                    && let Some(next) = self.marker(line.t(), None)
                {
                    if self.marker(line.t(), Some(kind)).is_some() && ind <= 3 {
                        current = self.marker(line.t(), Some(kind));
                        let _ = next;
                    }
                    break;
                }
                if prev_blank {
                    break;
                }
                // A code fence or a definition marker with less indentation
                // than the item content ends the list. After a blank line
                // in the item, a fence is a lazy line.
                if !had_blank
                    && fence_open(line.t()).is_some()
                    && self
                        .fenced_code(
                            lines.get(j..).unwrap_or(&[]),
                            j,
                            &mut fences,
                            &mut Vec::new(),
                        )
                        .is_some()
                {
                    break;
                }
                if self.def_marker(line.t()).is_some() || self.foreign_marker(line.t()) {
                    break;
                }
                // A lazy line keeps its spaces, so it is not a setext underline.
                item.push(line.dedent(0));
                j += 1;
            }
            raw_items.push(item);
            if current.is_none() {
                break;
            }
        }
        let count = raw_items.len();
        let loose = raw_items.iter().enumerate().any(|(k, item)| {
            let Some(last_text) = item.iter().rposition(|l| !l.is_blank()) else {
                return false;
            };
            // A nested list at the end of the item takes the blank lines in
            // it and after it, so they do not make this list loose.
            let mut sublist = None;
            for i in (1..=last_text).rev() {
                let Some(l) = item.get(i) else { break };
                if l.indent() < 4
                    && !is_hrule(l.t())
                    && let Some(marker) = self.marker(l.t(), None)
                {
                    if marker.start == 1 {
                        sublist = Some(i);
                    }
                } else if !(l.is_blank() || l.indent() > 0) {
                    break;
                }
            }
            // The marker line of an empty item is no blank line of it.
            let has_inner_blank = item
                .iter()
                .take(sublist.unwrap_or(last_text))
                .skip(1)
                .any(|l| l.is_blank());
            has_inner_blank
                || (k + 1 < count
                    && sublist.is_none()
                    && item.iter().skip(last_text).any(|l| l.is_blank()))
        });
        for mut item in raw_items {
            if loose && !item.last().is_some_and(|l| l.is_blank()) {
                item.push(Ln::new("", 0));
            }
            let saved = std::mem::replace(&mut self.in_list, true);
            let saved_loose = std::mem::replace(&mut self.loose_list, loose);
            let blocks = self.blocks(&item);
            self.in_list = saved;
            self.loose_list = saved_loose;
            let mut blocks = blocks?;
            self.task_box(&mut blocks);
            items.push(blocks);
        }
        let mut attrs = attrs;
        if let Some(n) = first_example {
            attrs.start = n;
        }
        compactify(&mut items);
        let consumed = trailing_blank_trim(lines, j);
        out.push(match kind {
            ListKind::Bullet => Block::BulletList(items),
            ListKind::Ordered(..) => Block::OrderedList(attrs, items),
        });
        Ok(Some(consumed))
    }

    /// Parses a single line with nested list markers (`- - - a`) at once,
    /// so that the line is not copied again for each level.
    fn nested_list_line(&mut self, lines: &[Ln], out: &mut Vec<Block>) -> Result<Option<usize>> {
        let Some(first) = lines.first() else {
            return Ok(None);
        };
        // The line must hold the whole list: after the blank lines, a line
        // with spaces or a marker can continue an item or start a sibling.
        let next = lines.iter().skip(1).find(|l| !l.is_blank());
        if lines.get(1).is_some_and(|l| !l.is_blank())
            || next.is_some_and(|l| l.indent() > 0 || self.marker(l.t(), None).is_some())
            || self.ext(Extension::TaskLists) && first.t().contains('[')
        {
            return Ok(None);
        }
        let budget = self.max_depth.saturating_sub(self.depth).max(1);
        let mut levels: Vec<(ListKind, ListAttributes)> = Vec::new();
        let mut rest = first.t();
        while levels.len() < budget && !is_hrule(rest) && indent(rest) <= 3 {
            let Some(m) = self.marker(rest, None) else {
                break;
            };
            if let ListKind::Ordered(ListNumberStyle::Example, _) = m.kind {
                break;
            }
            // A nested ordered list on the marker line must start at 1.
            if !levels.is_empty() && matches!(m.kind, ListKind::Ordered(..)) && m.start != 1 {
                break;
            }
            let attrs = match m.kind {
                ListKind::Bullet => ListAttributes::default(),
                ListKind::Ordered(style, delim) => ListAttributes {
                    start: if self.ext(Extension::Startnum) {
                        m.start
                    } else {
                        1
                    },
                    style,
                    delim,
                },
            };
            levels.push((m.kind, attrs));
            rest = rest.get(m.rest_from..).unwrap_or("");
            if rest.is_empty() {
                break;
            }
        }
        if levels.len() < 2 {
            return Ok(None);
        }
        let inner = [Ln::new(rest, first.n)];
        let saved = std::mem::replace(&mut self.in_list, true);
        let extra = levels.len().saturating_sub(1);
        self.depth += extra;
        let blocks = self.blocks(&inner);
        self.depth -= extra;
        self.in_list = saved;
        let mut blocks = blocks?;
        for (kind, attrs) in levels.into_iter().rev() {
            blocks = vec![match kind {
                ListKind::Bullet => Block::BulletList(vec![blocks]),
                ListKind::Ordered(..) => Block::OrderedList(attrs, vec![blocks]),
            }];
        }
        out.extend(blocks);
        Ok(Some(1))
    }

    /// Turns a task box at the start of the first paragraph of an item
    /// into a symbol. The item is parsed first, so the text after the box
    /// stays text of the paragraph.
    fn task_box(&self, blocks: &mut [Block]) {
        if !self.ext(Extension::TaskLists) {
            return;
        }
        let Some(Block::Plain(xs) | Block::Para(xs)) = blocks.first_mut() else {
            return;
        };
        let [Inline::RawInline(format, text)] = xs.as_slice() else {
            return;
        };
        if !format.as_str().starts_with(PLACEHOLDER) {
            return;
        }
        let format = format.clone();
        let symbol = match text.get(..3) {
            Some("[ ]") => "\u{2610}",
            Some("[x]" | "[X]") => "\u{2612}",
            _ => return,
        };
        let rest = text.get(3..).unwrap_or("");
        if !rest.is_empty() && !rest.starts_with(' ') {
            return;
        }
        let (first_line, more) = rest.split_once('\n').unwrap_or((rest, ""));
        if blank(first_line) && !more.is_empty() {
            return;
        }
        let rest = rest.trim_start_matches(SPACE);
        let mut inlines = vec![Inline::Str(symbol.to_string())];
        if !rest.is_empty() {
            inlines.push(Inline::Space);
            inlines.push(Inline::RawInline(format, rest.into()));
        }
        *xs = inlines;
    }

    // ----- definition lists -----

    fn def_marker(&self, s: &str) -> Option<Text> {
        let ind = indent(s);
        if ind > 3 {
            return None;
        }
        let t = s.get(ind..)?;
        let rest = t.strip_prefix(':').or_else(|| t.strip_prefix('~'))?;
        if !(rest.is_empty() || rest.starts_with(' ')) {
            return None;
        }
        let spaces = indent(rest);
        let text = rest.get(spaces..)?;
        if spaces >= 5 && !text.is_empty() {
            return Some(format!("{}{}", " ".repeat(spaces - 1), text));
        }
        Some(text.to_string())
    }

    fn definition_list(&mut self, lines: &[Ln], out: &mut Vec<Block>) -> Result<Option<usize>> {
        if !self.ext(Extension::DefinitionLists) {
            return Ok(None);
        }
        let mut items = Vec::new();
        let mut fences = FenceMemo::default();
        let mut j = 0;
        while let Some(term) = lines.get(j) {
            if term.is_blank() {
                break;
            }
            let mut k = j + 1;
            let loose = lines.get(k).is_some_and(|l| l.is_blank());
            if loose {
                k += 1;
            }
            if lines.get(k).and_then(|l| self.def_marker(l.t())).is_none() {
                break;
            }
            // The first definition after a blank line is the caption of a
            // table that follows it: the term is then a paragraph.
            if j == 0
                && loose
                && self.ext(Extension::TableCaptions)
                && lines
                    .get(k)
                    .is_some_and(|l| l.t().trim_start_matches(' ').starts_with(':'))
                && self
                    .without_effects(|p| p.table(lines.get(k..).unwrap_or(&[]), &mut Vec::new()))
                    .is_some()
            {
                return Ok(None);
            }
            let mut defs = Vec::new();
            while let Some(first) = lines.get(k).and_then(|l| self.def_marker(l.t())) {
                let n = lines.get(k).map_or(0, |l| l.n);
                let col = lines.get(k).map_or(4, |l| {
                    let ind = l.indent();
                    let after = l.t().get(ind + 1..).unwrap_or("");
                    let spaces = indent(after);
                    if spaces >= 5 || after.len() == spaces {
                        ind + 2
                    } else {
                        ind + 1 + spaces
                    }
                });
                let mut inner = vec![Ln::new(first, n)];
                k += 1;
                let mut prev_blank = false;
                let mut had_blank = false;
                while let Some(line) = lines.get(k) {
                    if line.is_blank() {
                        inner.push(Ln::new("", line.n));
                        prev_blank = true;
                        had_blank = true;
                        k += 1;
                        continue;
                    }
                    if line.indent() >= col {
                        inner.push(line.dedent(col));
                        prev_blank = false;
                        k += 1;
                        continue;
                    }
                    // A list marker or a code fence with less indentation
                    // than the content ends the definition list, as in a
                    // list item.
                    if prev_blank
                        || self.def_marker(line.t()).is_some()
                        || !is_hrule(line.t()) && self.marker(line.t(), None).is_some()
                        || !had_blank
                            && fence_open(line.t()).is_some()
                            && self
                                .fenced_code(
                                    lines.get(k..).unwrap_or(&[]),
                                    k,
                                    &mut fences,
                                    &mut Vec::new(),
                                )
                                .is_some()
                    {
                        break;
                    }
                    inner.push(line.tail(line.t().trim_start_matches(SPACE), line.n));
                    k += 1;
                }
                while inner.last().is_some_and(|l| l.is_blank()) && !loose {
                    inner.pop();
                }
                let k_back = {
                    let mut b = k;
                    while b > 0 && lines.get(b - 1).is_some_and(|l| l.is_blank()) {
                        b -= 1;
                    }
                    b
                };
                // A definition is a list item: a list marker ends its
                // paragraph, and a nested ordered list starts at 1.
                let saved = std::mem::replace(&mut self.in_list, true);
                let blocks = self.blocks(&inner);
                self.in_list = saved;
                let mut blocks = blocks?;
                if !loose {
                    for block in &mut blocks {
                        if let Block::Para(xs) = block {
                            *block = Block::Plain(std::mem::take(xs));
                        }
                    }
                }
                defs.push(blocks);
                if lines.get(k).and_then(|l| self.def_marker(l.t())).is_none() {
                    k = k_back.max(k_back);
                    let mut b = k;
                    while lines.get(b).is_some_and(|l| l.is_blank()) {
                        b += 1;
                    }
                    if lines.get(b).and_then(|l| self.def_marker(l.t())).is_some() {
                        k = b;
                    } else {
                        k = b;
                        break;
                    }
                }
            }
            items.push((vec![self.placeholder(term.t().trim_matches(SPACE))], defs));
            j = k;
        }
        if items.is_empty() {
            return Ok(None);
        }
        out.push(Block::DefinitionList(items));
        Ok(Some(j))
    }

    // ----- line blocks -----

    fn line_block(&mut self, lines: &[Ln], out: &mut Vec<Block>) -> Option<usize> {
        if !self.ext(Extension::LineBlocks) {
            return None;
        }
        let first = lines.first()?;
        if !(first.t() == "|" || first.t().starts_with("| ")) {
            return None;
        }
        let mut entries: Vec<Text> = Vec::new();
        let mut j = 0;
        while let Some(line) = lines.get(j) {
            crate::step(1);
            if line.t() == "|" || line.t().starts_with("| ") {
                let text = line.t().get(1..).unwrap_or("");
                let text = text.strip_prefix(' ').unwrap_or(text);
                entries.push(text.to_string());
            } else if line.t().starts_with(' ') && !line.is_blank() && !entries.is_empty() {
                if let Some(last) = entries.last_mut() {
                    last.push(' ');
                    last.push_str(line.t());
                }
            } else {
                break;
            }
            j += 1;
        }
        let mut result = Vec::new();
        for entry in entries {
            let spaces = indent(&entry);
            let mut inlines = Vec::new();
            if spaces > 0 {
                inlines.push(Inline::Str("\u{a0}".repeat(spaces)));
            }
            let rest = entry.get(spaces..).unwrap_or("");
            // A line ends without a line break, so its final backslash is
            // text and not an escaped line break.
            if ends_with_escape(rest) {
                inlines.push(self.placeholder(&format!("{rest}\\")));
            } else if !rest.is_empty() {
                inlines.push(self.placeholder(rest));
            }
            result.push(inlines);
        }
        out.push(Block::LineBlock(result));
        Some(j)
    }

    // ----- divs and HTML -----

    fn fenced_div(&mut self, lines: &[Ln], out: &mut Vec<Block>) -> Result<Option<usize>> {
        if !self.ext(Extension::FencedDivs) {
            return Ok(None);
        }
        let Some(first) = lines.first() else {
            return Ok(None);
        };
        let Some((_, info)) = div_fence(first.t()) else {
            return Ok(None);
        };
        let info = info.trim_end_matches(':').trim_matches(SPACE);
        if info.is_empty() {
            return Ok(None);
        }
        let attr = if info.starts_with('{') {
            match self.parse_attr(info) {
                Some(attr) => attr,
                None => return Ok(None),
            }
        } else if !info.contains(' ') {
            Attr::new("", vec![info.to_string()], vec![])
        } else {
            return Ok(None);
        };
        let closer = self.div_closer(lines);
        let closed = closer.is_some();
        let j = closer.map_or(lines.len(), |c| c + 1);
        let inner_end = if closed { j - 1 } else { lines.len() };
        let inner = lines.get(1..inner_end).unwrap_or(&[]);
        let saved = std::mem::replace(&mut self.in_list, false);
        let blocks = self.blocks_open(inner, true);
        self.in_list = saved;
        if !closed {
            let open = self.pos(first.n);
            let close = self.pos(self.end_line);
            self.warn(LogMessage::UnclosedDiv(open, close));
        }
        out.push(Block::Div(attr, blocks?));
        Ok(Some(if closed { j } else { lines.len() }))
    }

    /// Returns the index of the fence that closes the div whose opening fence
    /// is `lines[0]`. One pass with a stack of open fences finds the closer of
    /// each opener in it, so a nested div does not search again.
    fn div_closer(&mut self, lines: &[Ln]) -> Option<usize> {
        let first = lines.first()?;
        let rules = self.token_rules(true);
        let key = |line: &Ln| (line.src_id(), line.text_start());
        // An answer holds for a slice that ends at the same line or before
        // it: a closer inside the slice, or no closer up to its end.
        let cached = self
            .div_close
            .get(&key(first))
            .and_then(|&(distance, rest)| match distance {
                Some(d) => (d < lines.len()).then_some(Some(d)),
                None => (lines.len() <= rest).then_some(None),
            });
        let found = match cached {
            Some(distance) => distance,
            None => {
                let mut results = Vec::new();
                let found =
                    div_closer_scan(lines, rules, |open, close| results.push((open, close)));
                for (open, close) in results {
                    if let Some(line) = lines.get(open) {
                        self.kept_sources.push(Rc::clone(&line.src));
                        let rest = lines.len() - open;
                        self.div_close
                            .insert(key(line), (close.map(|c| c - open), rest));
                    }
                }
                found
            }
        };
        #[cfg(test)]
        crate::check_cache(&found, || div_closer_scan(lines, rules, |_, _| {}));
        found
    }

    /// Finds the closing tag of the element `name` that opens at the start of
    /// `lines[0]`: the line index and the byte position in its text. One
    /// pass finds the closer of each element in it that opens at the start
    /// of a line, so a nested element does not search again.
    fn html_closer(&mut self, scan: CloserScan<'_>, lines: &[Ln]) -> Option<(usize, usize)> {
        let first = lines.first()?;
        let name = scan.name;
        let key = |line: &Ln| (line.src_id(), line.text_start(), name.to_string());
        let cached = self
            .html_close
            .get(&key(first))
            .and_then(|(found, rest)| match found {
                // No closer up to the end of the slice that the scan read.
                None => (lines.len() <= *rest).then_some(None),
                // The position is in the shared text of the closing line: a line
                // can lose leading spaces after the pass that found its tag.
                Some((distance, src, position)) => {
                    let line = lines.get(*distance).filter(|l| l.src_id() == *src)?;
                    Some(Some((*distance, position.checked_sub(line.off.get())?)))
                }
            });
        let found = match cached {
            Some(found) => found,
            None => {
                let mut results = Vec::new();
                let tokens = self.tokens_for(lines);
                let found = scan.run(lines, &tokens, |open, close| results.push((open, close)));
                for ((open, open_pos), close) in results {
                    let Some(opener) = lines.get(open) else {
                        continue;
                    };
                    let value = close.and_then(|(j, pos)| {
                        let line = lines.get(j)?;
                        Some((j - open, line.src_id(), line.off.get() + pos))
                    });
                    self.kept_sources.push(Rc::clone(&opener.src));
                    if let Some((j, _)) = close
                        && let Some(line) = lines.get(j)
                    {
                        self.kept_sources.push(Rc::clone(&line.src));
                    }
                    let k = (
                        opener.src_id(),
                        opener.off.get() + open_pos,
                        name.to_string(),
                    );
                    self.html_close.insert(k, (value, lines.len() - open));
                }
                found
            }
        };
        #[cfg(test)]
        crate::check_cache(&found, || scan.run(lines, &Tokens::default(), |_, _| {}));
        found
    }

    fn html_block(&mut self, lines: &[Ln], out: &mut Vec<Block>) -> Result<Option<usize>> {
        if lines.first().is_none_or(|l| l.indent() > 3) {
            return Ok(None);
        }
        self.html_block_at(lines, out)
    }

    /// Parses an HTML block at the first line, at any indentation.
    fn html_block_at(&mut self, lines: &[Ln], out: &mut Vec<Block>) -> Result<Option<usize>> {
        let defer = std::mem::replace(&mut self.rest, RestHandoff::Off) == RestHandoff::Html;
        if !self.ext(Extension::RawHtml) && !self.ext(Extension::NativeDivs) {
            return Ok(None);
        }
        let Some(first) = lines.first() else {
            return Ok(None);
        };
        let s = first.t().trim_start_matches(SPACE);
        if !s.starts_with('<') {
            return Ok(None);
        }
        // A comment or a processing instruction is a block only at the start
        // of the line.
        if first.t().starts_with('<') && starts_comment_or_pi(s) {
            return self.raw_token_block(lines, defer, out);
        }
        self.html_element_block(lines, defer, out)
    }

    /// Tells whether the start tag at the start of `s`, complete on its
    /// line, gives its element Markdown content with `markdown_attribute`.
    fn markdown_content(&self, s: &str) -> bool {
        let Some(tag) = html_tag_len(s).and_then(|n| s.get(..n)) else {
            return false;
        };
        match tag_parts(tag)
            .1
            .into_iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("markdown"))
        {
            Some((_, value)) => value != "0",
            None => self.markdown_inside,
        }
    }

    /// Parses an HTML element at the first line. A separate function keeps
    /// its large stack frame off the path of a comment.
    #[inline(never)]
    fn html_element_block(
        &mut self,
        lines: &[Ln],
        defer: bool,
        out: &mut Vec<Block>,
    ) -> Result<Option<usize>> {
        let Some(first) = lines.first() else {
            return Ok(None);
        };
        let s = first.t().trim_start_matches(SPACE);
        // Join the lines so that a tag can span lines. A raw element can
        // contain blank lines, so it uses all lines; a tag ends at the first
        // blank line.
        let needs_all = html_tag_name(s).is_some_and(|(name, _)| {
            matches!(name.as_str(), "pre" | "script" | "style" | "textarea")
                || !self.ext(Extension::MarkdownInHtmlBlocks)
                // With `markdown_attribute`, an element without the
                // attribute is raw, except a native div.
                || (self.ext(Extension::MarkdownAttribute)
                    && !(name == "div" && self.ext(Extension::NativeDivs))
                    && !self.markdown_content(s))
        });
        let block_end = if needs_all {
            lines.len()
        } else {
            let mut k = 0;
            while let Some(line) = lines.get(k) {
                if line.is_blank() {
                    break;
                }
                k += 1;
                if line.t().contains('>') {
                    break;
                }
            }
            k
        };
        // One line is not copied: a line with many elements reads each one
        // from the shared text.
        let joined: std::borrow::Cow<'_, str> = if block_end <= 1 {
            std::borrow::Cow::Borrowed(first.t())
        } else {
            let parts: Vec<&str> = lines
                .get(..block_end)
                .unwrap_or(&[])
                .iter()
                .map(|l| l.t())
                .collect();
            let joined = parts.join("\n");
            crate::step(joined.len());
            std::borrow::Cow::Owned(joined)
        };
        let start = joined.len() - joined.trim_start_matches(' ').len();
        let text = joined.get(start..).unwrap_or("");
        // The length of the text after the element on its last line. Only a
        // joined text has a line break, so one long line is not searched
        // again for each of its elements.
        let line_len = |after: &str| {
            if block_end <= 1 {
                after.len()
            } else {
                after.find('\n').unwrap_or(after.len())
            }
        };
        let Some((name, closing)) = html_tag_name(s) else {
            return Ok(None);
        };
        if !BLOCK_TAGS.contains(&name.as_str()) {
            return Ok(None);
        }
        let Some(tag_len) = html_tag_len(text) else {
            return Ok(None);
        };
        let tag = text.get(..tag_len).unwrap_or("");
        let tag_lines = tag.matches('\n').count();
        let after_tag = text.get(tag_len..).unwrap_or("");
        let native_div = name == "div" && !closing && self.ext(Extension::NativeDivs);
        if !self.ext(Extension::RawHtml) && (!native_div || first.indent() > 0) {
            return Ok(None);
        }
        if closing {
            out.push(Block::RawBlock(Format::new("html"), tag.to_string()));
            return Ok(Some(self.rest_after(
                lines,
                tag_lines,
                line_len(after_tag),
                defer,
                out,
            )?));
        }
        let verbatim = matches!(name.as_str(), "pre" | "script" | "style" | "textarea");
        // With `markdown_attribute`, the attribute `markdown` with a value
        // other than `0` makes the content Markdown. Without the attribute,
        // the content of an element inside such an element is Markdown.
        let by_attribute = self.ext(Extension::MarkdownAttribute) && !native_div;
        let in_markdown = native_div
            || if by_attribute {
                self.markdown_content(tag)
            } else {
                self.ext(Extension::MarkdownInHtmlBlocks)
            };
        let inside = by_attribute || self.markdown_inside;
        if verbatim || !in_markdown {
            let close = format!("</{name}");
            let mut depth = 0usize;
            let lower = text.to_ascii_lowercase();
            let open = format!("<{name}");
            let mut idx = 0;
            let mut end = None;
            while idx < lower.len() {
                crate::step(1);
                let rest = lower.get(idx..).unwrap_or("");
                if rest.starts_with(&close) {
                    if depth <= 1 {
                        let close_len = rest.find('>').map_or(rest.len(), |k| k + 1);
                        end = Some(idx + close_len);
                        break;
                    }
                    depth -= 1;
                    idx += close.len();
                } else if rest.starts_with(&open) && !verbatim {
                    depth += 1;
                    idx += open.len();
                } else if rest.starts_with(&open) && depth == 0 {
                    depth = 1;
                    idx += open.len();
                } else if verbatim || !rest.starts_with('<') {
                    idx += rest.chars().next().map_or(1, char::len_utf8);
                } else if rest.starts_with("<!--") {
                    // A comment hides the tags in it. A comment with no end
                    // holds the rest of the text, so no closer follows.
                    match rest.get(4..).and_then(|r| r.find("-->")) {
                        Some(k) => idx += k + 7,
                        None => break,
                    }
                } else if html_tag_name(rest).is_some() {
                    match tag_extent(rest) {
                        Ok(n) => idx += n,
                        Err(true) => break,
                        Err(false) => idx += 1,
                    }
                } else {
                    idx += 1;
                }
            }
            // With no closing tag, and for an element that can be inline,
            // the raw block is the tag alone.
            let inline_element = !verbatim
                && (NON_INTERRUPTING_TAGS.contains(&name.as_str())
                    || MID_LINE_INLINE_TAGS.contains(&name.as_str()));
            let Some(end) = end.filter(|_| !inline_element) else {
                out.push(Block::RawBlock(Format::new("html"), tag.to_string()));
                return Ok(Some(self.rest_after(
                    lines,
                    tag_lines,
                    line_len(after_tag),
                    defer,
                    out,
                )?));
            };
            let raw = text.get(..end).unwrap_or("");
            let used_lines = raw.matches('\n').count();
            let after = text.get(end..).unwrap_or("");
            out.push(Block::RawBlock(Format::new("html"), raw.to_string()));
            return Ok(Some(self.rest_after(
                lines,
                used_lines,
                line_len(after),
                defer,
                out,
            )?));
        }
        // Markdown inside the element: find the matching closing tag.
        let first_rest = after_tag.get(..line_len(after_tag)).unwrap_or("");
        // The line after the tag gives the indentation of the content of a
        // raw element. Each block of the content can start after that many
        // spaces, so indented content is not a code block.
        let content_indent = if native_div || !blank(first_rest) {
            0
        } else {
            lines
                .get(tag_lines + 1)
                .filter(|line| !line.is_blank())
                .map_or(0, Ln::indent)
        };
        let first_line = blank(first_rest).then_some(tag_lines + 1);
        let scan = CloserScan::new(self, &name, first_line, content_indent);
        let closer = self.html_closer(scan, lines);
        if closer.is_none() {
            // No closing tag follows: the element holds the rest of the lines.
            // Consecutive opening tags on the first line nest without copies.
            let mut attrs = vec![(html_attr(tag), 1)];
            let mut text = first_rest.trim_start_matches(SPACE);
            while native_div
                && attrs.len() < self.max_depth.saturating_sub(self.depth).max(1)
                && html_tag_name(text).is_some_and(|(n, c)| n == "div" && !c)
                && let Some(len) = html_tag_len(text)
            {
                let column = if tag_lines == 0 {
                    first.t().len() - text.len() + 1
                } else {
                    1
                };
                attrs.push((html_attr(text.get(..len).unwrap_or("")), column));
                text = text.get(len..).unwrap_or("").trim_start_matches(SPACE);
            }
            self.cover_tokens(lines, tag_lines);
            let following = lines.get(tag_lines + 1..).unwrap_or(&[]);
            let saved = std::mem::replace(&mut self.in_list, false);
            let saved_inside = std::mem::replace(&mut self.markdown_inside, inside);
            let extra = attrs.len().saturating_sub(1);
            self.depth += extra;
            let blocks = if blank(text) {
                if VOID_TAGS.contains(&name.as_str()) || tag.ends_with("/>") {
                    // An element with no content: only the next line loses
                    // the spaces.
                    if let Some(next) = following.first() {
                        next.gobble(content_indent);
                    }
                } else {
                    self.gobble = content_indent;
                }
                self.blocks_open(following, true)
            } else if let Some(line) = lines.get(tag_lines).filter(|l| l.t().ends_with(text)) {
                // The content starts after the tag on its line: the line
                // starts there while the content is read, so the content is
                // a part of these lines and shares their token index.
                self.cover_tokens(lines, tag_lines.saturating_sub(1));
                let saved = line.skip_to(line.t().len() - text.len());
                let blocks = self.blocks_open(lines.get(tag_lines..).unwrap_or(&[]), true);
                line.restore(saved);
                blocks
            } else {
                let mut inner = vec![rest_line(
                    lines,
                    tag_lines,
                    text,
                    first_n_of(lines, tag_lines),
                )];
                inner.extend(following.iter().cloned());
                self.blocks_open(&inner, true)
            };
            self.depth -= extra;
            self.in_list = saved;
            self.markdown_inside = saved_inside;
            let mut blocks = blocks?;
            if native_div {
                let close = self.pos(self.end_line);
                for (attr, column) in attrs.into_iter().rev() {
                    let mut open = self.pos(first.n);
                    open.column = u32::try_from(column).unwrap_or(1);
                    self.warn(LogMessage::UnclosedDiv(open, close.clone()));
                    blocks = vec![Block::Div(attr, blocks)];
                }
                out.extend(blocks);
            } else {
                out.push(Block::RawBlock(Format::new("html"), tag.to_string()));
                out.extend(blocks);
            }
            return Ok(Some(lines.len()));
        }
        let Some((close_line, close_pos)) = closer else {
            return Ok(None);
        };
        let mut inner: Vec<Ln> = Vec::new();
        let mut closer_own_line = true;
        let first_n = first_n_of(lines, tag_lines);
        let first_line = lines.get(tag_lines).map_or("", |l| l.t());
        let first_rest_start = first_line.len().saturating_sub(first_rest.len());
        if close_line == tag_lines {
            let before = first_line.get(first_rest_start..close_pos).unwrap_or("");
            if !blank(before) {
                inner.push(Ln::new(before.trim_start_matches(SPACE), first_n));
                closer_own_line = false;
            }
        } else {
            if !blank(first_rest) {
                inner.push(Ln::new(first_rest.trim_start_matches(SPACE), first_n));
            }
            // Spaces before the closing tag of a native div end its content
            // like text does: the last paragraph is a Plain.
            let spaced_div_closer = |before: &str| native_div && !before.is_empty();
            let before_blank = lines.get(close_line).is_none_or(|l| {
                let before = l.t().get(..close_pos).unwrap_or("");
                blank(before) && !spaced_div_closer(before)
            });
            if !blank(first_rest) || !before_blank {
                inner.extend(
                    lines
                        .get(tag_lines + 1..close_line)
                        .unwrap_or(&[])
                        .iter()
                        .cloned(),
                );
            }
            if let Some(line) = lines.get(close_line) {
                let before = line.t().get(..close_pos).unwrap_or("");
                if !blank(before) {
                    inner.push(Ln::new(before, line.n));
                    closer_own_line = false;
                } else if spaced_div_closer(before) {
                    closer_own_line = false;
                }
            }
        }
        let close_text = lines
            .get(close_line)
            .and_then(|l| l.t().get(close_pos..))
            .unwrap_or("");
        let tag_end = close_text.find('>').map_or(close_text.len(), |k| k + 1);
        let close = Some((
            close_text.get(..tag_end).unwrap_or("").to_string(),
            close_text.get(tag_end..).unwrap_or(""),
        ));
        let j = close_line + 1;
        // When the content is a run of whole lines, it is a slice of the
        // lines, so a nested element does not copy the lines again.
        let whole_lines = close_line > tag_lines && blank(first_rest) && closer_own_line;
        let saved = std::mem::replace(&mut self.in_list, false);
        let saved_inside = std::mem::replace(&mut self.markdown_inside, inside);
        self.gobble = content_indent;
        let blocks = if whole_lines {
            self.cover_tokens(lines, tag_lines);
            let content = lines.get(tag_lines + 1..close_line).unwrap_or(&[]);
            self.blocks_open(content, native_div)
        } else {
            if native_div && closer_own_line {
                inner.push(Ln::new("", 0));
            }
            self.blocks(&inner)
        };
        self.in_list = saved;
        self.markdown_inside = saved_inside;
        let blocks = blocks?;
        if native_div {
            let attr = html_attr(tag);
            out.push(Block::Div(attr, blocks));
        } else {
            let start = if tag.ends_with("/>") {
                tag.to_string()
            } else {
                render_start_tag(tag)
            };
            out.push(Block::RawBlock(Format::new("html"), start));
            out.extend(blocks);
            if let Some((closer, _)) = &close {
                out.push(Block::RawBlock(Format::new("html"), closer.clone()));
            }
        }
        if let Some((_, rest)) = close
            && !blank(rest)
        {
            return Ok(Some(self.rest_after(
                lines,
                close_line,
                rest.len(),
                defer,
                out,
            )?));
        }
        Ok(Some(j))
    }

    /// Reads the last `len` bytes of `lines[at]`, the text after an element,
    /// as blocks and returns the number of lines used. With `defer`, the
    /// loop of `rest_as_lines` that reads this element reads the text.
    fn rest_after(
        &mut self,
        lines: &[Ln],
        at: usize,
        len: usize,
        defer: bool,
        out: &mut Vec<Block>,
    ) -> Result<usize> {
        let t = lines.get(at).map_or("", Ln::t);
        let rest = t.get(t.len().saturating_sub(len)..).unwrap_or("");
        if blank(rest) {
            return Ok(at + 1);
        }
        let len = rest.trim_start_matches(SPACE).len();
        if defer {
            self.rest = RestHandoff::Deferred { line: at, len };
            return Ok(at + 1);
        }
        self.rest_as_lines(lines, at, len, out)
    }

    /// Reads the last `len` bytes of `lines[at]` and the lines after it as
    /// one block and returns the number of lines used. The line starts
    /// there in place, so the lines are not copied. An element in that
    /// block with more text after it on its line hands the text back to
    /// this loop, so many elements on one line use no recursion.
    fn rest_as_lines(
        &mut self,
        lines: &[Ln],
        mut at: usize,
        mut len: usize,
        out: &mut Vec<Block>,
    ) -> Result<usize> {
        loop {
            let Some(line) = lines.get(at) else {
                return Ok(at + 1);
            };
            let saved = line.skip_to(line.t().len().saturating_sub(len));
            self.rest = RestHandoff::Armed;
            let rest = lines.get(at..).unwrap_or(&[]);
            // The rest of a line inside a container is one more level of
            // recursion, so it counts against the depth limit.
            self.depth += 1;
            self.cover_tokens(lines, at.saturating_sub(1));
            let mut fences = self.fence_memo(rest);
            let used = self.block_at(rest, 0, false, &mut fences, out);
            self.depth -= 1;
            line.restore(saved);
            let handoff = std::mem::replace(&mut self.rest, RestHandoff::Off);
            let used = used?;
            match handoff {
                RestHandoff::Deferred {
                    line,
                    len: rest_len,
                } => {
                    at += line;
                    len = rest_len;
                }
                _ => return Ok(at + used.max(1)),
            }
        }
    }

    /// Reads a comment or a processing instruction at the start of
    /// `lines[0]` as a raw block.
    fn raw_token_block(
        &mut self,
        lines: &[Ln],
        defer: bool,
        out: &mut Vec<Block>,
    ) -> Result<Option<usize>> {
        let Some(first) = lines.first().filter(|_| self.ext(Extension::RawHtml)) else {
            return Ok(None);
        };
        let comment = first.t().starts_with("<!--");
        let found = match html_comment_or_pi_len(first.t()) {
            Some(end) => Some((0, end)),
            None => self.raw_end_below(lines, comment),
        };
        let Some((end_line, end)) = found else {
            return Ok(None);
        };
        // A setext underline on the next line makes the line a heading.
        if end_line == 0 && lines.get(1).is_some_and(|l| is_setext_underline(l.t())) {
            return Ok(None);
        }
        let mut raw = String::new();
        for (k, line) in lines.iter().take(end_line + 1).enumerate() {
            if k > 0 {
                raw.push('\n');
            }
            let t = line.t();
            raw.push_str(if k == end_line {
                t.get(..end).unwrap_or(t)
            } else {
                t
            });
        }
        crate::step(raw.len());
        out.push(Block::RawBlock(Format::new("html"), raw));
        let last = lines.get(end_line).map_or(0, |l| l.t().len());
        Ok(Some(self.rest_after(
            lines,
            end_line,
            last.saturating_sub(end),
            defer,
            out,
        )?))
    }

    /// Finds the end of a comment (`-->`) or a processing instruction (`>`)
    /// that starts in `lines[0]` and does not end there: the line and the
    /// byte position after the end. A search that finds no end marks each
    /// line that it read, so a later token there does not search again.
    fn raw_end_below(&mut self, lines: &[Ln], comment: bool) -> Option<(usize, usize)> {
        let last = lines.last()?;
        let end_key = (last.src_id(), last.text_start());
        let key = |line: &Ln| (line.src_id(), line.text_start(), comment);
        let token_end = |t: &str| {
            if comment {
                t.find("-->").map(|i| i + 3)
            } else {
                t.find('>').map(|i| i + 1)
            }
        };
        let mut found = None;
        for (k, line) in lines.iter().enumerate().skip(1) {
            crate::step(1);
            if self.no_raw_end.get(&key(line)) == Some(&end_key) {
                break;
            }
            crate::step(line.t().len());
            if let Some(end) = token_end(line.t()) {
                found = Some((k, end));
                break;
            }
        }
        #[cfg(test)]
        crate::check_cache(&found, || {
            lines
                .iter()
                .enumerate()
                .skip(1)
                .find_map(|(k, line)| token_end(line.t()).map(|end| (k, end)))
        });
        if found.is_none() {
            for line in lines.iter().skip(1) {
                if self.no_raw_end.insert(key(line), end_key) == Some(end_key) {
                    break;
                }
                self.kept_sources.push(Rc::clone(&line.src));
            }
            self.kept_sources.push(Rc::clone(&last.src));
        }
        found
    }

    /// Reads a code block of `literate_haskell`: lines with bird tracks
    /// (`>`, or `<` before a space) that a blank line or the end follows, or
    /// a `code` environment.
    fn literate_code(&mut self, lines: &[Ln], out: &mut Vec<Block>) -> Option<usize> {
        if !self.ext(Extension::LiterateHaskell) {
            return None;
        }
        let first = lines.first()?;
        let t = first.t();
        if let Some(track) = t.chars().next().filter(|c| matches!(c, '>' | '<'))
            && (track == '>'
                || t.get(1..)
                    .is_none_or(|r| r.is_empty() || r.starts_with(' ')))
        {
            let mut body = Vec::new();
            let mut j = 0;
            while let Some(rest) = lines.get(j).and_then(|l| l.t().strip_prefix(track)) {
                crate::step(1);
                body.push(rest.strip_prefix(' ').unwrap_or(rest));
                j += 1;
            }
            if lines.get(j).is_some_and(|l| !l.is_blank()) {
                return None;
            }
            let classes = if track == '>' {
                vec!["haskell".into(), "literate".into()]
            } else {
                vec!["haskell".into()]
            };
            out.push(Block::CodeBlock(
                Attr::new("", classes, vec![]),
                body.join("\n"),
            ));
            return Some(j);
        }
        if first.indent() > 3
            || !t
                .trim_start_matches(' ')
                .strip_prefix("\\begin{code}")
                .is_some_and(blank)
        {
            return None;
        }
        let (end_line, end) = self.marker_below(lines, "\\end{code}")?;
        let last = lines.get(end_line)?.t();
        if !blank(last.get(end + "\\end{code}".len()..)?) {
            return None;
        }
        let mut body: Vec<&str> = lines
            .get(1..end_line)
            .unwrap_or(&[])
            .iter()
            .map(Ln::t)
            .collect();
        body.push(last.get(..end)?);
        let body = body.join("\n");
        let classes = vec!["haskell".into(), "literate".into()];
        out.push(Block::CodeBlock(
            Attr::new("", classes, vec![]),
            body.trim_end_matches('\n').to_string(),
        ));
        Some(end_line + 1)
    }

    /// Finds the first `marker` after `lines[0]`: the line and the byte
    /// position. A search that finds none marks each line that it read, so
    /// that a later search from there stops at once.
    fn marker_below(&mut self, lines: &[Ln], marker: &str) -> Option<(usize, usize)> {
        let last = lines.last()?;
        let end_key = (last.src_id(), last.text_start());
        let key = |line: &Ln| (line.src_id(), line.text_start(), marker.to_string());
        for (k, line) in lines.iter().enumerate().skip(1) {
            crate::step(1);
            if self.no_marker_below.get(&key(line)) == Some(&end_key) {
                return None;
            }
            crate::step(line.t().len());
            if let Some(end) = line.t().find(marker) {
                return Some((k, end));
            }
        }
        for line in lines.iter().skip(1) {
            self.no_marker_below.insert(key(line), end_key);
            self.kept_sources.push(Rc::clone(&line.src));
        }
        None
    }

    /// Reads raw TeX at the start of `lines[0]` as a raw block: a macro
    /// definition, an environment that is not math, or commands that
    /// `randoc_latex_reader::raw_tex_block` gives. The text after the raw
    /// TeX on its line is read as blocks.
    fn raw_tex_block(&mut self, lines: &[Ln], out: &mut Vec<Block>) -> Result<Option<usize>> {
        let start = out.len();
        let Some(mut used) = self.raw_tex_unit(lines, out)? else {
            return Ok(None);
        };
        if !lines
            .first()
            .is_some_and(|line| line.t().trim_start_matches(SPACE).starts_with("\\begin{"))
        {
            return Ok(Some(used));
        }
        while out.len() == start + 1 {
            let Some(more) = self.raw_tex_unit(&lines[used..], out)? else {
                break;
            };
            used += more;
            let [Block::RawBlock(_, first), Block::RawBlock(_, second), ..] = &mut out[start..]
            else {
                break;
            };
            first.push('\n');
            first.push_str(second);
            out.remove(start + 1);
        }
        Ok(Some(used))
    }

    fn raw_tex_unit(&mut self, lines: &[Ln], out: &mut Vec<Block>) -> Result<Option<usize>> {
        if !self.ext(Extension::RawTex) {
            return Ok(None);
        }
        let Some(first) = lines.first() else {
            return Ok(None);
        };
        let s = first.t().trim_start_matches(SPACE);
        if !s.starts_with('\\') || first.indent() > 3 {
            return Ok(None);
        }
        if let Some(rest) = s.strip_prefix("\\begin{") {
            return self.raw_tex_environment(lines, rest, out);
        }
        // Commands: this line and the lines after it that start with a
        // backslash, as a command can follow a command on the next line.
        if !s
            .get(1..)
            .is_some_and(|r| r.starts_with(char::is_alphabetic))
        {
            return Ok(None);
        }
        let mut j = 1;
        let mut bytes = s.len();
        let may_span_lines = s.starts_with("\\parbox") || s.starts_with("\\makeatletter");
        while let Some(line) = lines.get(j) {
            if line.is_blank()
                || bytes >= 64 * 1024
                || (!may_span_lines && !line.t().trim_start_matches(SPACE).starts_with('\\'))
            {
                break;
            }
            bytes = bytes.saturating_add(line.t().len() + 1);
            crate::step(1);
            j += 1;
        }
        let parts: Vec<&str> = std::iter::once(s)
            .chain(lines.get(1..j).unwrap_or(&[]).iter().map(Ln::t))
            .collect();
        let text = parts.join("\n");
        crate::step(text.len());
        let Some((end, raw)) = randoc_latex_reader::raw_tex_block(&text) else {
            return Ok(None);
        };
        let before = text.get(..end).unwrap_or("");
        let line = before.matches('\n').count();
        let line_text = parts.get(line).copied().unwrap_or("");
        let col = end - before.rfind('\n').map_or(0, |k| k + 1);
        out.push(Block::RawBlock(Format::new("tex"), raw));
        let rest_len = line_text.len().saturating_sub(col);
        Ok(Some(self.rest_after(lines, line, rest_len, false, out)?))
    }

    /// Reads an environment that is not math, from `\begin{` to its
    /// `\end`, as a raw block. `rest` is the text after `\begin{`.
    fn raw_tex_environment(
        &mut self,
        lines: &[Ln],
        rest: &str,
        out: &mut Vec<Block>,
    ) -> Result<Option<usize>> {
        let name: Text = rest.chars().take_while(|c| *c != '}').collect();
        if name.is_empty()
            || !rest.get(name.len()..).is_some_and(|r| r.starts_with('}'))
            || MATH_ENVIRONMENTS.contains(&name.trim_end_matches('*'))
        {
            return Ok(None);
        }
        let end = format!("\\end{{{name}}}");
        let begin = format!("\\begin{{{name}}}");
        // No end marker on this line or below: no search for the closer.
        if !lines.first().is_some_and(|l| l.t().contains(&end))
            && self.marker_below(lines, &end).is_none()
        {
            return Ok(None);
        }
        let mut depth = 0usize;
        for (j, line) in lines.iter().enumerate() {
            crate::step(1);
            let t = if j == 0 {
                line.t().trim_start_matches(SPACE)
            } else {
                line.t()
            };
            // The markers of the line in order; the closer that brings the
            // depth to zero ends the environment.
            let mut marks: Vec<(usize, bool)> = t
                .match_indices(&begin)
                .map(|(k, _)| (k, true))
                .chain(t.match_indices(&end).map(|(k, _)| (k, false)))
                .collect();
            marks.sort_unstable();
            let mut close_at = None;
            for (k, opens) in marks {
                if opens {
                    depth += 1;
                } else {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        close_at = Some(k + end.len());
                        break;
                    }
                }
            }
            let Some(close_at) = close_at else {
                continue;
            };
            let mut body: Vec<&str> = std::iter::once(
                lines
                    .first()
                    .map_or("", |l| l.t().trim_start_matches(SPACE)),
            )
            .chain(lines.get(1..j).unwrap_or(&[]).iter().map(Ln::t))
            .take(j)
            .collect();
            body.push(t.get(..close_at).unwrap_or(t));
            out.push(Block::RawBlock(Format::new("tex"), body.join("\n")));
            let rest_len = t.len().saturating_sub(close_at);
            return Ok(Some(self.rest_after(lines, j, rest_len, false, out)?));
        }
        Ok(None)
    }

    // ----- tables -----

    fn table(&mut self, lines: &[Ln], out: &mut Vec<Block>) -> Option<usize> {
        let options = TableOptions {
            simple: self.ext(Extension::SimpleTables),
            multiline: self.ext(Extension::MultilineTables),
            grid: self.ext(Extension::GridTables),
            pipe: self.ext(Extension::PipeTables),
            captions: self.ext(Extension::TableCaptions),
            attributes: self.ext(Extension::TableAttributes),
            columns: self.columns,
        };
        if !(options.simple || options.multiline || options.grid || options.pipe) {
            return None;
        }
        if lines.first().is_some_and(|l| is_hrule(l.t()))
            && lines.get(1).is_some_and(|l| l.is_blank())
        {
            return None;
        }
        let is_dashes = |s: &str| {
            let t = s.trim_matches(SPACE);
            !t.is_empty() && t.chars().all(|c| c == '-' || c == ' ' || c == ':')
        };
        let first = lines.first()?.t();
        let first_trim = first.trim_start_matches(SPACE);
        let dash_start = is_dashes(first);
        let possible = dash_start
            || first.contains('|')
            || first_trim.starts_with('+')
            || first_trim.starts_with(':')
            || first_trim.starts_with("Table:")
            || first_trim.starts_with("table:")
            || lines.get(1).is_some_and(|l| is_dashes(l.t()));
        if !possible {
            return None;
        }
        // A table and its caption are two blocks with blank lines between
        // them. Only a table that starts with a dashed line can contain blank
        // lines, so it gets all lines.
        let end = if dash_start {
            lines.len()
        } else {
            let mut k = 0;
            while lines.get(k).is_some_and(|l| !l.is_blank()) {
                k += 1;
            }
            while lines.get(k).is_some_and(|l| l.is_blank()) {
                k += 1;
            }
            while lines.get(k).is_some_and(|l| !l.is_blank()) {
                k += 1;
            }
            (k + 1).min(lines.len())
        };
        let text: Vec<&str> = lines
            .get(..end)
            .unwrap_or(&[])
            .iter()
            .map(|l| l.t())
            .collect();
        crate::step(text.len());
        let (block, used) = parse_table(&text, &options, self)?;
        out.push(block);
        Some(used.max(1))
    }

    // ----- metadata -----

    fn yaml_block(&mut self, lines: &[Ln]) -> Result<Option<usize>> {
        let Some(first) = lines.first() else {
            return Ok(None);
        };
        if first.t().trim_end_matches(SPACE) != "---" || lines.get(1).is_none_or(|l| l.is_blank()) {
            return Ok(None);
        }
        let mut j = 1;
        let mut end = None;
        while let Some(line) = lines.get(j) {
            crate::step(1);
            let t = line.t().trim_end_matches(SPACE);
            if t == "---" || t == "..." {
                end = Some(j);
                break;
            }
            j += 1;
        }
        let Some(end) = end else {
            return Ok(None);
        };
        let body: Vec<&str> = lines
            .get(1..end)
            .unwrap_or(&[])
            .iter()
            .map(|l| l.t())
            .collect();
        let body = body.join("\n");
        let source = self.source.clone();
        let max_depth = self.max_depth.saturating_sub(self.depth);
        let first_line = lines.get(1).map_or(1, |l| l.n);
        let mut markdown = |s: &str| {
            self.sub_document(s)
                .map_err(|error| crate::yaml::YamlError {
                    message: match error {
                        Error::Parse(message) => message,
                        other => other.to_string(),
                    },
                })
        };
        let fields =
            crate::yaml::parse_yaml_metadata(&body, &source, first_line, max_depth, &mut markdown);
        match fields {
            Ok(Some(fields)) => {
                // A later block replaces the value of a key, also a value
                // of the title block.
                for (key, value) in fields {
                    self.meta.insert(key, value);
                }
                Ok(Some(end + 1))
            }
            Ok(None) => Ok(None),
            Err(error) => Err(Error::Parse(error.message)),
        }
    }

    /// Parses a string of a metadata field as blocks with the main reader
    /// state. The inline text stays unread until the end of the document,
    /// so that it can use the references and notes of later lines. Pandoc
    /// names the string `SOURCE_chunk` and counts its lines from 1.
    fn sub_document(&mut self, text: &str) -> Result<Vec<Block>> {
        // Some blocks need the line break at the end of their line, so one
        // line without it is a paragraph.
        if !text.contains('\n') && self.needs_line_end(text) {
            return Ok(vec![Block::Para(vec![
                self.placeholder(text.trim_matches(SPACE)),
            ])]);
        }
        let lines: Vec<Ln> = text
            .split('\n')
            .enumerate()
            .map(|(i, t)| Ln::new(expand_tabs(t, 4), u32::try_from(i + 1).unwrap_or(u32::MAX)))
            .collect();
        let content = lines
            .iter()
            .rposition(|l| !l.is_blank())
            .map_or(0, |k| k + 1);
        let chunk = format!("{}_chunk", self.source);
        let source = std::mem::replace(&mut self.source, chunk);
        let end_line = std::mem::replace(
            &mut self.end_line,
            u32::try_from(content).unwrap_or(u32::MAX).saturating_add(3),
        );
        let blocks = self.blocks(&lines);
        self.source = source;
        self.end_line = end_line;
        blocks
    }

    // ----- references and notes -----

    fn reference(&mut self, lines: &[Ln]) -> Option<usize> {
        let first = lines.first()?;
        if first.indent() > 3 {
            return None;
        }
        let s = first.t().trim_start_matches(SPACE);
        // A label can continue on the next line.
        let (label, rest, mut used) = match bracket_label(s) {
            Some((label, rest)) => (label.to_string(), rest.to_string(), 1),
            None => {
                let next = lines
                    .get(1)
                    .filter(|l| s.starts_with('[') && !l.is_blank())?;
                let joined = format!("{s}\n{}", next.t());
                let (label, rest) = bracket_label(&joined)?;
                if !label.contains('\n') {
                    return None;
                }
                (label.to_string(), rest.to_string(), 2)
            }
        };
        let label = label.as_str();
        // A label that is a note key belongs to a note definition.
        if self.ext(Extension::Footnotes)
            && label
                .strip_prefix('^')
                .is_some_and(|key| !key.is_empty() && !key.contains(char::is_whitespace))
        {
            return None;
        }
        let rest = rest.strip_prefix(':')?;
        let mut text = rest.to_string();
        if blank(&text) {
            // With no text on the next line, the destination is empty.
            if let Some(next) = lines.get(used).filter(|l| !l.is_blank()) {
                text = next.t().to_string();
                used += 1;
            }
        }
        let mut t = text.trim_matches(SPACE);
        if t.starts_with('[') {
            return None;
        }
        // MultiMarkdown `key=value` pairs after the destination and the
        // title, on this line and on the lines after it.
        let mut pairs = None;
        if self.ext(Extension::MmdLinkAttributes)
            && let Some(start) = mmd_pairs_start(t)
        {
            let (found, more) = mmd_pairs(t.get(start..)?, lines.get(used..).unwrap_or(&[]))?;
            pairs = Some(found);
            used += more;
            t = t.get(..start)?.trim_end_matches(SPACE);
        }
        let (url, after) = if title_of(t).is_some_and(|(_, rest)| blank(rest)) {
            (Text::new(), t)
        } else if let Some(inner) = t.strip_prefix('<') {
            let close = inner.find('>')?;
            (inner.get(..close)?.to_string(), inner.get(close + 1..)?)
        } else {
            // The destination can contain spaces. It ends where a title or
            // attributes start after white space.
            // A title ends at the end of the text, so only an opening mark
            // that pairs with the last character can start it.
            let last = t.trim_end_matches(SPACE).chars().last();
            let pairs = move |c: char| {
                last == Some('}')
                    || match c {
                        '"' => last == Some('"'),
                        '\'' => last == Some('\''),
                        '(' => last == Some(')'),
                        _ => false,
                    }
            };
            let starts: Vec<usize> = t
                .char_indices()
                .filter(|(i, c)| {
                    *i > 0
                        && matches!(c, '"' | '\'' | '(' | '{')
                        && pairs(*c)
                        && t.get(..*i)
                            .is_some_and(|b| b.ends_with(char::is_whitespace))
                })
                .map(|(i, _)| i)
                .collect();
            let end = starts
                .into_iter()
                .find(|i| {
                    let tail = t.get(*i..).unwrap_or("");
                    title_of(tail).is_some_and(|(_, r)| {
                        let r = r.trim_matches(SPACE);
                        r.is_empty() || r.starts_with('{')
                    }) || (tail.starts_with('{')
                        && self.parse_attr(tail.trim_matches(SPACE)).is_some())
                })
                .unwrap_or(t.len());
            let destination = t.get(..end)?.trim_end_matches(SPACE);
            // A closed title or attribute group in the middle of the
            // destination makes the line no definition.
            if self.has_inner_title(t, end) {
                return None;
            }
            (destination.to_string(), t.get(end..)?)
        };
        let mut after = after.trim_matches(SPACE).to_string();
        if after.is_empty()
            && let Some(next) = lines.get(used)
            && !next.is_blank()
        {
            let nt = next.t().trim_matches(SPACE);
            if nt.starts_with(['"', '\'', '(']) && title_of(nt).is_some_and(|(_, r)| blank(r)) {
                after = nt.to_string();
                used += 1;
            }
        }
        let (title, after) = match title_of(&after) {
            Some((title, rest)) => (title, rest.trim_matches(SPACE).to_string()),
            None => (Text::new(), after),
        };
        if pairs.is_none()
            && after.is_empty()
            && self.ext(Extension::MmdLinkAttributes)
            && let Some(next) = lines.get(used).filter(|l| !l.is_blank())
            && mmd_key(next.t().trim_start_matches(SPACE)).is_some()
        {
            let (found, more) = mmd_pairs("", lines.get(used..).unwrap_or(&[]))?;
            pairs = Some(found);
            used += more;
        }
        let attr = if let Some(pairs) = pairs {
            if !after.is_empty() {
                return None;
            }
            mmd_attr(pairs)
        } else if after.is_empty() {
            Attr::default()
        } else if self.ext(Extension::LinkAttributes) && after.starts_with('{') {
            self.parse_attr(&after)?
        } else {
            return None;
        };
        let key = normalize_key(label);
        let url = escape_uri(&crate::inline::unescape(&url));
        let url = if self.ext(Extension::RebaseRelativePaths) {
            rebase_target(url, self.source_directory())
        } else {
            url
        };
        let target = Target::new(url, title);
        if self
            .refs
            .get(&key)
            .is_some_and(|old| old.0 != target || old.1 != attr)
        {
            let pos = self.pos(first.n);
            self.warn(LogMessage::DuplicateLinkReference(
                format!("[{label}]"),
                pos,
            ));
        }
        self.longest_key = self.longest_key.max(key.chars().count());
        self.refs.insert(key, (target, attr));
        Some(used)
    }

    /// Tells whether a title (`"t"`, `'t'`, `(t)`) or attributes (`{.c}`)
    /// start after white space in `text`, not after `end`, and more text
    /// follows them. Such a line is no reference definition.
    fn has_inner_title(&self, text: &str, end: usize) -> bool {
        // For each closing character: the start of the last search and the
        // closer that it found. A later search finds the same closer while
        // it does not start past it.
        let mut searches: HashMap<char, (usize, Option<usize>)> = HashMap::new();
        text.char_indices()
            .take_while(|(i, _)| *i <= end)
            .any(|(i, open)| {
                let close = match open {
                    '"' => '"',
                    '\'' => '\'',
                    '(' => ')',
                    '{' => '}',
                    _ => return false,
                };
                if !text
                    .get(..i)
                    .is_some_and(|b| b.ends_with(char::is_whitespace))
                {
                    return false;
                }
                let from = i + 1;
                let found = match searches.get(&close) {
                    Some(&(start, found)) if from >= start && found.is_none_or(|k| k >= from) => {
                        found
                    }
                    _ => {
                        let found = text
                            .get(from..)
                            .and_then(|rest| rest.find(close))
                            .map(|k| from + k);
                        searches.insert(close, (from, found));
                        found
                    }
                };
                let Some(k) = found else {
                    return false;
                };
                let group_end = k + 1;
                let after = text.get(group_end..).unwrap_or("");
                after.starts_with(char::is_whitespace)
                    && !after.trim_matches(SPACE).is_empty()
                    && !after.trim_start_matches(SPACE).starts_with('{')
                    && (open != '{'
                        || self
                            .parse_attr(text.get(i..group_end).unwrap_or(""))
                            .is_some())
            })
    }

    fn abbreviation(&mut self, lines: &[Ln]) -> Option<usize> {
        if !self.ext(Extension::Abbreviations) {
            return None;
        }
        let first = lines.first()?;
        let rest = first.t().strip_prefix("*[")?;
        let close = rest.find("]:")?;
        if close == 0 {
            return None;
        }
        Some(1)
    }

    fn note_definition(&mut self, lines: &[Ln]) -> Result<Option<usize>> {
        if !self.ext(Extension::Footnotes) {
            return Ok(None);
        }
        let Some(first) = lines.first() else {
            return Ok(None);
        };
        if first.indent() > 3 {
            return Ok(None);
        }
        let s = first.t().trim_start_matches(SPACE);
        let Some((label, rest)) = bracket_label(s) else {
            return Ok(None);
        };
        let Some(key) = label.strip_prefix('^') else {
            return Ok(None);
        };
        let Some(rest) = rest.strip_prefix(':') else {
            return Ok(None);
        };
        if key.is_empty() || key.contains(char::is_whitespace) {
            return Ok(None);
        }
        // The first line keeps the spaces after the colon, less an indent
        // of four, so that a heading mark there is no heading.
        let first_text = rest.strip_prefix("    ").unwrap_or(rest);
        let mut inner = vec![Ln::new(first_text, first.n)];
        let mut j = 1;
        let mut prev_blank = false;
        while let Some(line) = lines.get(j) {
            crate::step(1);
            if line.is_blank() {
                inner.push(Ln::new("", line.n));
                prev_blank = true;
                j += 1;
                continue;
            }
            if line.indent() >= 4 {
                inner.push(line.dedent(4));
                prev_blank = false;
                j += 1;
                continue;
            }
            if prev_blank {
                break;
            }
            if bracket_label(line.t().trim_start_matches(SPACE))
                .is_some_and(|(l, r)| l.starts_with('^') && r.starts_with(':'))
            {
                break;
            }
            inner.push(line.clone());
            j += 1;
        }
        let consumed = trailing_blank_trim(lines, j);
        let saved = std::mem::replace(&mut self.in_list, false);
        let blocks = self.blocks_open(&inner, true);
        self.in_list = saved;
        let blocks = blocks?;
        let key = key.to_string();
        if self.notes.contains_key(&key) {
            let pos = self.pos(first.n);
            self.warn(LogMessage::DuplicateNoteReference(key.clone(), pos));
        }
        self.note_pos.push((key.clone(), self.pos(first.n)));
        self.notes.insert(key, blocks);
        Ok(Some(consumed))
    }

    // ----- attributes -----

    /// Parses `{#id .class key=value}` attributes. The text must be the whole attribute block.
    pub(crate) fn parse_attr(&self, text: &str) -> Option<Attr> {
        let all = self.ext(Extension::AllSymbolsEscapable);
        let angles = self.ext(Extension::AngleBracketsEscapable);
        let (attr, used) = crate::inline::parse_attributes(text, |b| {
            if all {
                b.is_ascii_punctuation()
            } else {
                b"\\`*_{}[]()>#+-.!".contains(&b) || (angles && b == b'<')
            }
        })?;
        (used == text.len()).then_some(attr)
    }

    // ----- the inline phase -----

    fn resolve_document(&mut self, blocks: Vec<Block>) -> Vec<Block> {
        let blocks = self.resolve_blocks(blocks);
        let mut unused: Vec<(Text, SourcePos)> = Vec::new();
        let mut seen = HashSet::new();
        let used: HashSet<&Text> = self.used_notes.iter().collect();
        for (key, pos) in self.note_pos.iter().rev() {
            if seen.insert(key.clone()) && !used.contains(key) {
                unused.push((key.clone(), pos.clone()));
            }
        }
        unused.reverse();
        let mut messages: Vec<LogMessage> = unused
            .into_iter()
            .map(|(key, pos)| LogMessage::NoteDefinedButNotUsed(key, pos))
            .collect();
        let mut others: Vec<LogMessage> = Vec::new();
        for message in self.log.drain(..) {
            if let LogMessage::DuplicateNoteReference(..) = message {
                messages.push(message);
            } else {
                others.push(message);
            }
        }
        others.extend(messages);
        self.log = others;
        blocks
    }

    /// Gives a heading its identifier and removes the parser data from its attributes.
    fn finish_header(&mut self, attr: &mut Attr, inlines: &[Inline]) {
        let Some(position) = attr.attributes.iter().position(|(k, _)| k == "\u{0}line") else {
            return;
        };
        let line = attr
            .attributes
            .remove(position)
            .1
            .parse::<u32>()
            .unwrap_or(1);
        let raw_text = attr
            .attributes
            .iter()
            .position(|(k, _)| k == "\u{0}raw")
            .map(|p| attr.attributes.remove(p).1)
            .unwrap_or_default();
        if attr.id.is_empty() {
            if self.ext(Extension::AutoIdentifiers) {
                let base = if raw_text.contains('[') {
                    let first = self.first_pass_inlines(&raw_text);
                    auto_identifier(&first, self.exts)
                } else {
                    auto_identifier(inlines, self.exts)
                };
                attr.id = self.idset.unique(&base);
            }
        } else if !self.idset.register(&attr.id) {
            let pos = self.pos(line);
            self.warn(LogMessage::DuplicateIdentifier(attr.id.clone(), pos));
        }
        if self.ext(Extension::ImplicitHeaderReferences) {
            let key = normalize_key(&raw_text);
            let target = Target::new(format!("#{}", attr.id), "");
            self.longest_key = self.longest_key.max(key.chars().count());
            self.header_refs.entry(key).or_insert(target);
        }
    }

    /// Parses the text of a heading as Pandoc does in its first pass, which
    /// makes the identifier: no reference link and no note is known yet, so
    /// `[text][ref]` and `[^n]` stay text. The parse changes no state.
    fn first_pass_inlines(&mut self, text: &str) -> Vec<Inline> {
        let refs = std::mem::take(&mut self.refs);
        let header_refs = std::mem::take(&mut self.header_refs);
        let notes = std::mem::take(&mut self.notes);
        let inlines = self.without_effects(|p| p.parse_inlines(text));
        self.refs = refs;
        self.header_refs = header_refs;
        self.notes = notes;
        inlines
    }

    /// Runs `f` and then restores the note number, the used notes, the
    /// messages and the failure counts, so that a parse made only for an
    /// identifier leaves no trace.
    fn without_effects<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let note_number = self.note_number;
        let used_notes = self.used_notes.len();
        let log = self.log.len();
        let counts = (
            self.emphasis_failures,
            self.block_tag_cuts,
            self.crossing_closers,
        );
        let out = f(self);
        self.note_number = note_number;
        self.used_notes.truncate(used_notes);
        self.log.truncate(log);
        (
            self.emphasis_failures,
            self.block_tag_cuts,
            self.crossing_closers,
        ) = counts;
        out
    }

    fn assign_meta_header_ids(&mut self, value: &mut MetaValue) {
        match value {
            MetaValue::MetaBlocks(xs) => self.assign_header_ids(xs),
            MetaValue::MetaList(xs) => xs.iter_mut().for_each(|x| self.assign_meta_header_ids(x)),
            MetaValue::MetaMap(map) => map
                .values_mut()
                .for_each(|x| self.assign_meta_header_ids(x)),
            _ => {}
        }
    }

    fn assign_header_ids(&mut self, blocks: &mut [Block]) {
        for block in blocks.iter_mut() {
            match block {
                // The identifiers come before the inline phase, so that a
                // link can name a later heading. The inlines are read again
                // in document order, which numbers their notes and citations.
                Block::Header(_, attr, inlines) => {
                    let resolved = self.without_effects(|p| p.resolve_inlines(inlines.clone()));
                    self.finish_header(attr, &resolved);
                }
                Block::BlockQuote(xs) | Block::Div(_, xs) => self.assign_header_ids(xs),
                Block::BulletList(items) | Block::OrderedList(_, items) => {
                    for item in items {
                        self.assign_header_ids(item);
                    }
                }
                Block::DefinitionList(items) => {
                    for (_, defs) in items {
                        for def in defs {
                            self.assign_header_ids(def);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn resolve_blocks(&mut self, blocks: Vec<Block>) -> Vec<Block> {
        if self.depth >= self.max_depth {
            return blocks;
        }
        self.depth += 1;
        let out = blocks.into_iter().map(|b| self.resolve_block(b)).collect();
        self.depth -= 1;
        out
    }

    fn implicit_figure(&self, xs: &[Inline]) -> Option<Block> {
        let [Inline::Image(attr, alt, target)] = xs else {
            return None;
        };
        if !self.ext(Extension::ImplicitFigures) || alt.is_empty() {
            return None;
        }
        let mut image_attr = attr.clone();
        let id = std::mem::take(&mut image_attr.id);
        // The first `alt` value is the image description and the first
        // `latex-placement` value moves to the figure. Each copy of the two
        // keys leaves the image.
        let first = |key: &str| {
            image_attr
                .attributes
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        let description = first("alt").map_or_else(|| alt.clone(), |text| words(&text));
        let placement = first("latex-placement").map(|v| ("latex-placement".into(), v));
        image_attr
            .attributes
            .retain(|(k, _)| k != "alt" && k != "latex-placement");
        Some(Block::Figure(
            Attr::new(id, vec![], placement.into_iter().collect()),
            Caption {
                short: None,
                long: vec![Block::Plain(alt.clone())],
            },
            vec![Block::Plain(vec![Inline::Image(
                image_attr,
                description,
                target.clone(),
            )])],
        ))
    }

    /// Keeps the macro definitions of the raw TeX `raw` for the math that
    /// follows, with `latex_macros`.
    pub(crate) fn define_macros(&mut self, raw: &str) {
        const DEFINITIONS: &[&str] = &[
            "\\newcommand",
            "\\renewcommand",
            "\\providecommand",
            "\\def",
            "\\gdef",
            "\\edef",
            "\\xdef",
            "\\let",
            "\\DeclareMathOperator",
            "\\newenvironment",
            "\\renewenvironment",
        ];
        if self.ext(Extension::LatexMacros) && DEFINITIONS.iter().any(|d| raw.contains(d)) {
            crate::step(raw.len());
            self.macros.define(raw);
        }
    }

    /// Expands the macros of the raw TeX read so far.
    pub(crate) fn expand_math(&mut self, math: &str) -> Text {
        if !self.ext(Extension::LatexMacros) {
            return math.to_string();
        }
        self.macros.expand_math(math)
    }

    /// Expands the macros in one raw TeX command.
    pub(crate) fn expand_raw_inline(&mut self, text: &str) -> Text {
        if self.ext(Extension::LatexMacros) {
            self.macros.expand_inline(text)
        } else {
            text.into()
        }
    }

    fn resolve_block(&mut self, block: Block) -> Block {
        match block {
            Block::RawBlock(format, raw) if format.as_str() == "tex" => {
                let raw = if self.ext(Extension::LatexMacros) {
                    crate::step(raw.len());
                    self.macros.expand_block(&raw)
                } else {
                    raw
                };
                Block::RawBlock(format, raw)
            }
            // An image alone in a paragraph is a figure, also when the
            // paragraph is plain, as in a tight list item.
            Block::Para(xs) => {
                let xs = self.resolve_inlines(xs);
                self.implicit_figure(&xs).unwrap_or(Block::Para(xs))
            }
            Block::Plain(xs) => {
                let xs = self.resolve_inlines(xs);
                match self.inline_cells {
                    true => Block::Plain(xs),
                    false => self.implicit_figure(&xs).unwrap_or(Block::Plain(xs)),
                }
            }
            Block::Header(level, mut attr, xs) => {
                let xs = self.resolve_inlines(xs);
                self.finish_header(&mut attr, &xs);
                Block::Header(level, attr, xs)
            }
            Block::LineBlock(lines) => Block::LineBlock(
                lines
                    .into_iter()
                    .map(|xs| self.resolve_inlines(xs))
                    .collect(),
            ),
            Block::BlockQuote(xs) => Block::BlockQuote(self.resolve_blocks(xs)),
            Block::Div(attr, xs) => Block::Div(attr, self.resolve_blocks(xs)),
            Block::BulletList(items) => {
                Block::BulletList(items.into_iter().map(|i| self.resolve_blocks(i)).collect())
            }
            Block::OrderedList(attrs, items) => Block::OrderedList(
                attrs,
                items.into_iter().map(|i| self.resolve_blocks(i)).collect(),
            ),
            Block::DefinitionList(items) => Block::DefinitionList(
                items
                    .into_iter()
                    .map(|(term, defs)| {
                        let term = self.resolve_inlines(term);
                        let defs = defs.into_iter().map(|d| self.resolve_blocks(d)).collect();
                        (term, defs)
                    })
                    .collect(),
            ),
            Block::Table(mut table) => {
                let caption = std::mem::take(&mut table.caption.long);
                table.caption.long = self.resolve_blocks(caption);
                let attributes = &mut table.attr.attributes;
                let grid = attributes.iter().any(|(k, _)| k.as_str() == GRID_MARK);
                attributes.retain(|(k, _)| k.as_str() != GRID_MARK);
                let saved = std::mem::replace(&mut self.inline_cells, !grid);
                for row in table
                    .head
                    .rows
                    .iter_mut()
                    .chain(
                        table
                            .bodies
                            .iter_mut()
                            .flat_map(|b| b.head.iter_mut().chain(b.body.iter_mut())),
                    )
                    .chain(table.foot.rows.iter_mut())
                {
                    for cell in &mut row.cells {
                        let content = std::mem::take(&mut cell.content);
                        cell.content = self.resolve_blocks(content);
                    }
                }
                self.inline_cells = saved;
                Block::Table(table)
            }
            // A figure comes from a paragraph that is resolved already, for
            // example in a metadata value that is resolved a second time.
            other => other,
        }
    }

    pub(crate) fn resolve_inlines(&mut self, inlines: Vec<Inline>) -> Vec<Inline> {
        let mut out = Vec::new();
        for inline in inlines {
            match inline {
                Inline::RawInline(format, text) if format.as_str().starts_with(PLACEHOLDER) => {
                    let directory = format.as_str().strip_prefix(PLACEHOLDER).unwrap_or("");
                    let previous = std::mem::replace(&mut self.inline_directory, directory.into());
                    out.extend(self.parse_inlines(&text));
                    self.inline_directory = previous;
                }
                other => out.push(other),
            }
        }
        crate::inline::normalize(out)
    }

    fn resolve_meta(&mut self, value: MetaValue) -> MetaValue {
        match value {
            MetaValue::MetaInlines(xs) => MetaValue::MetaInlines(self.resolve_inlines(xs)),
            MetaValue::MetaBlocks(xs) => MetaValue::MetaBlocks(self.resolve_blocks(xs)),
            MetaValue::MetaList(xs) => {
                MetaValue::MetaList(xs.into_iter().map(|x| self.resolve_meta(x)).collect())
            }
            MetaValue::MetaMap(map) => MetaValue::MetaMap(
                map.into_iter()
                    .map(|(k, v)| (k, self.resolve_meta(v)))
                    .collect(),
            ),
            other => other,
        }
    }
}

impl CellParser for Parser {
    fn inlines(&mut self, text: &str) -> Vec<Inline> {
        let t = text.trim_matches(SPACE);
        if t.is_empty() {
            Vec::new()
        } else if ends_with_escape(t) {
            // The text ends without a line break, so its final backslash
            // is text and not an escaped line break.
            vec![self.placeholder(&format!("{t}\\"))]
        } else {
            vec![self.placeholder(t)]
        }
    }

    fn blocks(&mut self, text: &str) -> Vec<Block> {
        let mut lines: Vec<Ln> = text.split('\n').map(|t| Ln::new(t, 0)).collect();
        lines.push(Ln::new("", 0));
        Parser::blocks(self, &lines).unwrap_or_default()
    }

    fn attributes(&mut self, text: &str) -> Option<Attr> {
        self.parse_attr(text.trim_matches(SPACE))
    }
}

/// Makes a placeholder inline for raw Markdown text.
/// Tells whether the text from byte `i` of `text` starts with raw TeX that
/// Pandoc reads as a block in a paragraph: a command of class `Block` or an
/// environment that is not math, with its end. `ends` gets the last
/// position of each `\end{name}` of `text` at the first environment.
fn tex_block_at<'t>(text: &'t str, i: usize, ends: &mut Option<HashMap<&'t str, usize>>) -> bool {
    use randoc_latex_reader::{CommandClass, RawKind};
    let rest = text.get(i..).unwrap_or("");
    let name_len = rest
        .get(1..)
        .unwrap_or("")
        .find(|c: char| !c.is_alphabetic())
        .unwrap_or(rest.len().saturating_sub(1));
    let name = rest.get(1..1 + name_len).unwrap_or("");
    let window = if name == "begin" {
        let Some(env) = rest
            .strip_prefix("\\begin{")
            .and_then(|r| r.get(..r.find('}')?))
        else {
            return false;
        };
        if MATH_ENVIRONMENTS.contains(&env.trim_end_matches('*')) {
            return false;
        }
        let ends = ends.get_or_insert_with(|| {
            crate::step(text.len());
            let mut ends = HashMap::new();
            for (k, _) in text.match_indices("\\end{") {
                let after = text.get(k + 5..).unwrap_or("");
                if let Some(close) = after.find('}') {
                    ends.insert(after.get(..close).unwrap_or(""), k);
                }
            }
            ends
        });
        match ends.get(env) {
            Some(&end) if end > i => rest.get(..end - i + env.len() + 6).unwrap_or(rest),
            _ => return false,
        }
    } else if randoc_latex_reader::command_class(name) == CommandClass::Block {
        let mut window = rest.len().min(1024);
        while !rest.is_char_boundary(window) {
            window -= 1;
        }
        rest.get(..window).unwrap_or("")
    } else {
        return false;
    };
    crate::step(window.len().min(64));
    matches!(
        randoc_latex_reader::raw_tex_inline(window).map(|raw| raw.kind),
        Some(RawKind::Environment { math: false } | RawKind::Command(CommandClass::Block))
    )
}

/// Splits `text` into words, with one space for each run of white space.
fn words(text: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    for (k, word) in text.split(SPACE).enumerate() {
        if k > 0 && out.last() != Some(&Inline::Space) {
            out.push(Inline::Space);
        }
        if !word.is_empty() {
            out.push(Inline::Str(word.into()));
        }
    }
    out
}

fn source_directory(source: &str) -> &str {
    source.rsplit_once('/').map_or(
        "",
        |(directory, _)| {
            if directory.is_empty() { "/" } else { directory }
        },
    )
}

fn rebase_target(target: Text, directory: &str) -> Text {
    if directory.is_empty()
        || target.is_empty()
        || target.starts_with(['#', '/'])
        || target
            .split('/')
            .next()
            .is_some_and(|head| head.contains(':'))
    {
        return target;
    }
    format!("{}/{target}", directory.trim_end_matches('/'))
}

/// Returns the number of lines to consume so that the trailing blank lines stay for the next block.
fn trailing_blank_trim(_lines: &[Ln], j: usize) -> usize {
    j
}

fn first_n_of(lines: &[Ln], index: usize) -> u32 {
    lines.get(index).map_or(0, |l| l.n)
}

fn alert_kind(s: &str) -> Option<(&'static str, &'static str)> {
    match s {
        "[!NOTE]" => Some(("note", "Note")),
        "[!TIP]" => Some(("tip", "Tip")),
        "[!IMPORTANT]" => Some(("important", "Important")),
        "[!WARNING]" => Some(("warning", "Warning")),
        "[!CAUTION]" => Some(("caution", "Caution")),
        _ => None,
    }
}

/// Turns the final `Para` of the last item into `Plain` when the other items have no `Para`.
fn compactify(items: &mut [Vec<Block>]) {
    let Some((last, others)) = items.split_last_mut() else {
        return;
    };
    let others_plain = others
        .iter()
        .all(|item| !item.iter().any(|b| matches!(b, Block::Para(_))));
    if !others_plain {
        return;
    }
    let paras = last.iter().filter(|b| matches!(b, Block::Para(_))).count();
    if paras == 1
        && let Some(Block::Para(xs)) = last.last_mut()
    {
        let xs = std::mem::take(xs);
        last.pop();
        last.push(Block::Plain(xs));
    }
}

/// Tells whether an item of kind `a` continues a list of kind `b`: `#.`
/// and a decimal number with a period mix in one list.
fn same_list(a: ListKind, b: ListKind) -> bool {
    let hash = ListKind::Ordered(ListNumberStyle::DefaultStyle, ListNumberDelim::DefaultDelim);
    let decimal = ListKind::Ordered(ListNumberStyle::Decimal, ListNumberDelim::Period);
    // A `#.` item continues a decimal or lowercase list with periods.
    a == b
        || (a == hash
            && matches!(
                b,
                ListKind::Ordered(
                    ListNumberStyle::Decimal
                        | ListNumberStyle::LowerAlpha
                        | ListNumberStyle::LowerRoman,
                    ListNumberDelim::Period
                )
            ))
        || (a == decimal && b == hash)
}

/// Returns the length of the MultiMarkdown attribute key at the start of
/// `s` when an `=` follows it: a letter, then letters, digits and `-_:.`.
fn mmd_key(s: &str) -> Option<usize> {
    if !s.starts_with(|c: char| c.is_alphabetic()) {
        return None;
    }
    let len = s
        .find(|c: char| !(c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.')))
        .unwrap_or(s.len());
    s.get(len..)?.starts_with('=').then_some(len)
}

/// Returns the start of the first `key=value` word of a reference
/// definition line `t` that does not start the line. A title that ends
/// before white space or the end of the line is skipped.
fn mmd_pairs_start(t: &str) -> Option<usize> {
    let mut skip_to = 0;
    for (i, c) in t.char_indices() {
        if i == 0 || i < skip_to || !t.get(..i).is_some_and(|b| b.ends_with(SPACE)) {
            continue;
        }
        let rest = t.get(i..)?;
        if matches!(c, '"' | '\'' | '(')
            && let Some((_, after)) = title_of(rest)
            && (after.is_empty() || after.starts_with(SPACE))
        {
            skip_to = t.len() - after.len();
        } else if mmd_key(rest).is_some() {
            return Some(i);
        }
    }
    None
}

/// Parses MultiMarkdown `key=value` pairs from `first` and from the lines
/// of `next` that continue them. Pairs are separated by white space, and a
/// quoted value can span lines. Returns the pairs and the number of lines
/// of `next` that they use, or `None` when other text follows them.
fn mmd_pairs(first: &str, next: &[Ln]) -> Option<(Vec<(Text, Text)>, usize)> {
    let mut pairs = Vec::new();
    let mut line = first;
    let mut used = 0;
    let mut pos = 0;
    loop {
        crate::step(1);
        pos = line.len() - line.get(pos..)?.trim_start_matches(SPACE).len();
        if pos == line.len() {
            match next.get(used) {
                Some(l) if !l.is_blank() && mmd_key(l.t().trim_start_matches(SPACE)).is_some() => {
                    line = l.t();
                    pos = 0;
                    used += 1;
                    continue;
                }
                _ => return Some((pairs, used)),
            }
        }
        let rest = line.get(pos..)?;
        let key_len = mmd_key(rest)?;
        let key = rest.get(..key_len)?;
        pos += key_len + 1;
        let rest = line.get(pos..)?;
        let value = match rest.chars().next() {
            Some(quote @ ('"' | '\'')) => {
                let mut value = String::new();
                let mut part = rest.get(1..)?;
                loop {
                    crate::step(part.len());
                    if let Some(end) = part.find(quote) {
                        value.push_str(part.get(..end)?);
                        pos = line.len() - part.len() + end + 1;
                        break;
                    }
                    value.push_str(part);
                    value.push(' ');
                    let l = next.get(used).filter(|l| !l.is_blank())?;
                    line = l.t();
                    part = line;
                    used += 1;
                }
                if !line.get(pos..)?.is_empty() && !line.get(pos..)?.starts_with(SPACE) {
                    return None;
                }
                value
            }
            _ => {
                let len = rest.find(SPACE).unwrap_or(rest.len());
                pos += len;
                rest.get(..len)?.to_string()
            }
        };
        pairs.push((key.to_string(), value));
    }
}

/// Makes the attributes of MultiMarkdown pairs: `id` is the identifier and
/// each `class` value adds its words as classes.
fn mmd_attr(pairs: Vec<(Text, Text)>) -> Attr {
    let mut attr = Attr::default();
    for (key, value) in pairs {
        match key.as_str() {
            "id" => attr.id = value,
            "class" => attr
                .classes
                .extend(value.split(SPACE).filter(|w| !w.is_empty()).map(Text::from)),
            _ => attr.attributes.push((key, value)),
        }
    }
    attr
}

/// Returns the label inside `[...]` at the start of `s` and the text after `]`.
fn bracket_label(s: &str) -> Option<(&str, &str)> {
    let inner = s.strip_prefix('[')?;
    let mut depth = 0usize;
    let mut escaped = false;
    for (i, c) in inner.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '[' => depth += 1,
            ']' if depth == 0 => return Some((inner.get(..i)?, inner.get(i + 1..)?)),
            ']' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Parses a link title at the start of `s` and returns it with the text after it.
pub(crate) fn title_of(s: &str) -> Option<(Text, &str)> {
    let open = s.chars().next()?;
    let close = match open {
        '"' => '"',
        '\'' => '\'',
        '(' => ')',
        _ => return None,
    };
    let inner = s.get(1..)?;
    let mut escaped = false;
    let mut last = None;
    for (i, c) in inner.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
        } else if c == close {
            let after = inner.get(i + 1..)?;
            if blank(after) || after.trim_start_matches(SPACE).starts_with('{') {
                last = Some(i);
            }
        }
    }
    let end = last?;
    let title = crate::inline::unescape(inner.get(..end)?);
    Some((title, inner.get(end + 1..)?))
}

/// Normalizes a reference label: lowercase, with runs of white space as one space.
pub(crate) fn normalize_key(label: &str) -> Text {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Escapes the characters of a URL that Pandoc escapes.
pub(crate) fn escape_uri(url: &str) -> Text {
    let mut out = Text::new();
    for c in url.chars() {
        if c.is_whitespace()
            || matches!(c, '<' | '>' | '|' | '"' | '{' | '}' | '[' | ']' | '^' | '`')
        {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn language_id(word: &str) -> Text {
    let lower = word.to_lowercase();
    match lower.as_str() {
        "c++" => "cpp".into(),
        "objective-c" => "objectivec".into(),
        _ => lower,
    }
}

/// Splits an HTML start tag into its name and its attributes, with the
/// character references of each value decoded. A name with no value has an
/// empty value.
fn tag_parts(tag: &str) -> (&str, Vec<(&str, Text)>) {
    let inner = tag
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim_end_matches('/');
    let name_len = inner.find(char::is_whitespace).unwrap_or(inner.len());
    let name = inner.get(..name_len).unwrap_or("");
    let mut rest = inner
        .get(name_len..)
        .unwrap_or("")
        .trim_start_matches(SPACE);
    let mut attributes = Vec::new();
    while !rest.is_empty() {
        let name_len = rest
            .find(|c: char| c == '=' || c.is_whitespace())
            .unwrap_or(rest.len());
        let key = rest.get(..name_len).unwrap_or("");
        rest = rest.get(name_len..).unwrap_or("").trim_start_matches(SPACE);
        let mut value = "";
        if let Some(r) = rest.strip_prefix('=') {
            let r = r.trim_start_matches(SPACE);
            if let Some(q) = r.chars().next().filter(|c| *c == '"' || *c == '\'') {
                let body = r.get(1..).unwrap_or("");
                let end = body.find(q).unwrap_or(body.len());
                value = body.get(..end).unwrap_or("");
                rest = body.get(end + 1..).unwrap_or("").trim_start_matches(SPACE);
            } else {
                let end = r.find(char::is_whitespace).unwrap_or(r.len());
                value = r.get(..end).unwrap_or("");
                rest = r.get(end..).unwrap_or("").trim_start_matches(SPACE);
            }
        }
        if key.is_empty() {
            break;
        }
        attributes.push((key, crate::inline::decode_entities(value)));
    }
    (name, attributes)
}

/// Parses the attributes of an HTML start tag into an `Attr`. Names do not
/// depend on case; the first `id` and the first `class` count.
pub(crate) fn html_attr(tag: &str) -> Attr {
    let mut attr = Attr::default();
    let (mut id, mut class) = (None, None);
    for (key, value) in tag_parts(tag).1 {
        let key = key.to_lowercase();
        match key.as_str() {
            "id" => {
                id.get_or_insert(value);
            }
            "class" => {
                class.get_or_insert(value);
            }
            _ => attr.attributes.push((key, value)),
        }
    }
    attr.id = id.unwrap_or_default();
    attr.classes = class
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    attr
}

/// Writes an HTML start tag again from its parts, as Pandoc does for the
/// tag of an element with Markdown content: each value in double quotes
/// with `&<>"'` escaped, and a name with an empty value alone.
fn render_start_tag(tag: &str) -> Text {
    let (name, attributes) = tag_parts(tag);
    let mut out = format!("<{name}");
    for (key, value) in attributes.into_iter().filter(|(k, _)| *k != "markdown") {
        out.push(' ');
        out.push_str(key);
        if value.is_empty() {
            continue;
        }
        out.push_str("=\"");
        for c in value.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&#39;"),
                c => out.push(c),
            }
        }
        out.push('"');
    }
    out.push('>');
    out
}
