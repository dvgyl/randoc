//! The block phase and the driver of the reader.

use std::cell::Cell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use randoc_ast::{
    Attr, Block, Caption, Format, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, Meta,
    MetaValue, Pandoc, Target, Text,
};
use randoc_core::{Error, Extension, Extensions, LogMessage, ReaderOptions, Result, SourcePos};

use crate::tables::{CellParser, GRID_MARK, TableOptions, parse_table};
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
    fn new(t: impl Into<Text>, n: u32) -> Self {
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
    fn is_blank(&self) -> bool {
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
    pub max_depth: usize,
    pub depth: usize,
    log: Vec<LogMessage>,
    pub refs: HashMap<Text, (Target, Attr)>,
    pub header_refs: HashMap<Text, Target>,
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
    /// For a line (`Ln::src_id`, `Ln::text_start`) and a comment (`true`) or
    /// a processing instruction: the last line of a slice (the same names)
    /// such that no line from this one to it ends the token.
    no_raw_end: HashMap<(usize, usize, bool), (usize, usize)>,
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
    /// The number of emphasis openers that found no closer.
    pub emphasis_failures: usize,
    /// The number of bracket texts that ended at a block tag.
    pub block_tag_cuts: usize,
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
    let Some(label) = body.strip_prefix('@') else {
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

/// Removes the spaces at the end of `s`, but keeps the space of a final
/// escaped space (`\ `), which is a nonbreaking space.
fn trim_end_unescaped(s: &str) -> &str {
    let trimmed = s.trim_end_matches(SPACE);
    if ends_with_escape(trimmed) {
        s.get(..trimmed.len() + 1).unwrap_or(trimmed)
    } else {
        trimmed
    }
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
    /// For backticks and tildes: the line of the search and the run length.
    failed: [Option<(usize, usize)>; 2],
    /// For a comment and a processing instruction: the line where the last
    /// search for the line of its end started, and the line that it found.
    raw_ends: [Option<(usize, Option<usize>)>; 2],
}

impl FenceMemo {
    fn slot(c: char) -> usize {
        usize::from(c == '~')
    }

    fn has_no_closer(&self, c: char, at: usize, n: usize) -> bool {
        self.failed
            .get(Self::slot(c))
            .copied()
            .flatten()
            .is_some_and(|(line, len)| at >= line && n >= len)
    }

    fn no_closer(&mut self, c: char, at: usize, n: usize) {
        if let Some(slot) = self.failed.get_mut(Self::slot(c))
            && slot.is_none_or(|(_, len)| n <= len)
        {
            *slot = Some((at, n));
        }
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
    raw_html: bool,
    math: bool,
    /// The index of the first content line when the tag line has no
    /// content after the tag: an indented line there starts a code block.
    first_line: Option<usize>,
    /// The spaces that each content line loses before its blocks start.
    content_indent: usize,
    /// The element is a native div: its content starts with no spaces lost.
    native_div: bool,
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
            raw_html: parser.ext(Extension::RawHtml),
            math: parser.ext(Extension::TexMathDollars),
            first_line,
            content_indent,
            native_div: name == "div" && parser.ext(Extension::NativeDivs),
        }
    }

    /// Returns the closer of the element that opens in `lines[0]`, as the
    /// line index and the byte position in its text. `found` gets the
    /// position of each opening tag and its closer, or `None` when it has
    /// no closer, unless an indented line follows a nested tag.
    fn run(
        self,
        lines: &[Ln],
        mut found: impl FnMut((usize, usize), Option<(usize, usize)>),
    ) -> Option<(usize, usize)> {
        let mut stack: Vec<(usize, usize)> = Vec::new();
        let mut fence: Option<(char, usize)> = None;
        let mut no_fence_closer = FenceMemo::default();
        let mut comment_until: Option<usize> = None;
        let mut no_comment_end_from: Option<usize> = None;
        let mut code_may_start = false;
        // The last line with four spaces or more. The indentation inside a
        // container differs, so an element that opens before such a line
        // has an answer only for this scan.
        let mut last_indented = 0;
        for (j, line) in lines.iter().enumerate() {
            let t = line.t();
            crate::step(t.len() + 1);
            let mut i = 0;
            if j > 0 && line.indent() >= 4 {
                last_indented = j;
            }
            let code_line = (code_may_start || self.first_line == Some(j))
                && !line.is_blank()
                && line.indent() >= 4 + self.content_indent;
            code_may_start = j > 0 && (line.is_blank() || code_line);
            let mut bare_open = false;
            if code_line && fence.is_none() && comment_until.is_none() {
                continue;
            }
            if let Some(end_line) = comment_until {
                if j < end_line {
                    continue;
                }
                comment_until = None;
                i = t.find("-->").map_or(t.len(), |k| k + 3);
            } else if let Some((c, n)) = fence {
                if closes_fence(t, c, n) {
                    fence = None;
                }
                continue;
            } else if j > 0
                && let Some((c, n, _, _)) = fence_open(container_content(t))
                && !no_fence_closer.has_no_closer(c, j, n)
            {
                if lines.get(j + 1..).is_some_and(|rest| {
                    rest.iter().any(|l| {
                        crate::step(1);
                        closes_fence(l.t(), c, n)
                    })
                }) {
                    fence = Some((c, n));
                    continue;
                }
                no_fence_closer.no_closer(c, j, n);
            }
            let bytes = t.as_bytes();
            let mut no_code_closer: HashSet<usize> = HashSet::new();
            let mut no_dollar = false;
            while let Some(&b) = bytes.get(i) {
                crate::step(1);
                match b {
                    b'\\' => i += 2,
                    b'`' => {
                        let n = bytes
                            .get(i..)
                            .unwrap_or(&[])
                            .iter()
                            .take_while(|c| **c == b'`')
                            .count();
                        i += n;
                        if !no_code_closer.contains(&n) {
                            match backtick_run(bytes, i, n) {
                                Some(end) => i = end,
                                None => {
                                    no_code_closer.insert(n);
                                }
                            }
                        }
                    }
                    b'$' if self.math && !no_dollar => match math_end(bytes, i) {
                        Ok(end) => i = end,
                        Err(none_after) => {
                            no_dollar = none_after;
                            i += 1;
                        }
                    },
                    b'<' => {
                        let rest = t.get(i..).unwrap_or("");
                        match self.tag_at(rest) {
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
                            }
                            Some(Tag::Open) => {
                                let len = html_tag_len(rest);
                                // A nested div with no content after its tag
                                // can start with an indented code block.
                                bare_open = self.native_div
                                    && len.is_some_and(|n| rest.get(n..).is_some_and(blank));
                                stack.push((j, i));
                                i += len.unwrap_or(self.name.len() + 1);
                            }
                            None if self.raw_html
                                && rest.starts_with("<!--")
                                && starts_comment_or_pi(rest) =>
                            {
                                if let Some(k) = rest.get(4..).and_then(|r| r.find("-->")) {
                                    i += k + 7;
                                } else if no_comment_end_from.is_none_or(|from| j < from)
                                    && let Some(k) = lines.get(j + 1..).and_then(|rest| {
                                        rest.iter().position(|l| {
                                            crate::step(1);
                                            l.t().contains("-->")
                                        })
                                    })
                                {
                                    comment_until = Some(j + 1 + k);
                                    break;
                                } else {
                                    no_comment_end_from = Some(j);
                                    i += 1;
                                }
                            }
                            None => i += html_tag_len(rest).unwrap_or(1),
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
        if closing {
            return Some(Tag::Close);
        }
        let next = rest.get(self.name.len()..).and_then(|r| r.chars().next());
        matches!(next, None | Some(' ' | '>' | '/' | '\t')).then_some(Tag::Open)
    }
}

/// A tag of the element that a `CloserScan` searches.
#[derive(Clone, Copy)]
enum Tag {
    Open,
    Close,
}

/// Returns the end of the math span that starts with the `$` at `i`. The
/// error tells whether no `$` follows, so that no later `$` starts one.
fn math_end(bytes: &[u8], i: usize) -> std::result::Result<usize, bool> {
    let space = |b: Option<&u8>| matches!(b, None | Some(b' ' | b'\t'));
    if bytes.get(i + 1) == Some(&b'$') {
        let body = bytes.get(i + 2..).unwrap_or(&[]);
        return body
            .windows(2)
            .position(|w| w == b"$$")
            .map(|k| i + k + 4)
            .ok_or(false);
    }
    if space(bytes.get(i + 1)) {
        return Err(false);
    }
    let k = i
        + 1
        + bytes
            .get(i + 1..)
            .unwrap_or(&[])
            .iter()
            .position(|c| *c == b'$')
            .ok_or(true)?;
    let ends =
        k > i + 1 && !space(bytes.get(k - 1)) && !bytes.get(k + 1).is_some_and(u8::is_ascii_digit);
    if ends { Ok(k + 1) } else { Err(false) }
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
/// closer of each opener in it, or `None` for one with no closer.
fn div_closer_scan(lines: &[Ln], mut found: impl FnMut(usize, Option<usize>)) -> Option<usize> {
    let mut stack: Vec<usize> = vec![0];
    let mut in_fence: Option<(char, usize)> = None;
    let mut j = 1;
    while let Some(line) = lines.get(j) {
        crate::step(1);
        if let Some((c, n)) = in_fence {
            let tt = line.t().trim_matches(SPACE);
            if tt.len() - tt.trim_start_matches(c).len() >= n && tt.trim_start_matches(c).is_empty()
            {
                in_fence = None;
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
            } else {
                stack.push(j);
            }
        }
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
            max_depth,
            depth: 0,
            log: Vec::new(),
            refs: HashMap::new(),
            header_refs: HashMap::new(),
            notes: HashMap::new(),
            note_pos: Vec::new(),
            used_notes: Vec::new(),
            end_blank: false,
            div_close: HashMap::new(),
            html_close: HashMap::new(),
            kept_sources: Vec::new(),
            no_raw_end: HashMap::new(),
            rest: RestHandoff::Off,
            idset: IdentifierSet::new(),
            examples: HashMap::new(),
            example_count: 0,
            note_number: 0,
            in_note: false,
            inline_cells: false,
            split_budget: source.len().saturating_mul(4).saturating_add(4096),
            emphasis_failures: 0,
            block_tag_cuts: 0,
            in_list: false,
            loose_list: false,
            gobble: 0,
            end_line: 1,
            meta: Meta::default(),
        }
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
        let blocks = self.resolve_document(blocks);
        let mut meta = std::mem::take(&mut self.meta);
        for (key, value) in std::mem::take(&mut meta.0) {
            let value = self.resolve_meta(value);
            meta.insert(key, value);
        }
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
            let value = MetaValue::MetaInlines(vec![placeholder(&title)]);
            self.meta.insert("title", value);
        }
        if let Some(authors) = it.next() {
            let list: Vec<MetaValue> = authors
                .split([';', '\n'])
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .map(|a| MetaValue::MetaInlines(vec![placeholder(a)]))
                .collect();
            if !list.is_empty() {
                self.meta.insert("author", MetaValue::MetaList(list));
            }
        }
        if let Some(date) = it.next()
            && !date.is_empty()
        {
            self.meta
                .insert("date", MetaValue::MetaInlines(vec![placeholder(&date)]));
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
                .insert(key, MetaValue::MetaInlines(vec![placeholder(&value)]));
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

    /// Parses lines into blocks. Each block can start after up to `gobble` spaces.
    fn blocks_inner(&mut self, lines: &[Ln], gobble: usize) -> Result<Vec<Block>> {
        let mut out = Vec::new();
        let mut fences = FenceMemo::default();
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
            let used = self.block_at(lines, i, prev_blank, &mut fences, &mut out)?;
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
        if let Some(n) = self.header(rest, out) {
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
        if let Some(n) = self.raw_tex_block(rest, out) {
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
        self.paragraph(rest, i, fences, out)
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
        if self.in_list && indent(s) <= 3 && !is_hrule(s) && self.marker(s, None).is_some() {
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

    /// Parses a paragraph at `lines`, which start at line `at` of the slice
    /// of `fences`.
    fn paragraph(
        &mut self,
        lines: &[Ln],
        at: usize,
        fences: &mut FenceMemo,
        out: &mut Vec<Block>,
    ) -> Result<usize> {
        let mut j = 1;
        let mut para = None;
        // The paragraph text ends at the first line with a block tag after
        // text, so that the text after the tag is not read again.
        let has_tag = |p: &Self, line: &Ln| p.inline_block_tag(line.t()).is_some();
        let mut tagged = lines.first().is_some_and(|l| has_tag(self, l));
        // The first line of the text that has no open comment yet.
        let mut scan_from = 0;
        while let Some(line) = lines.get(j).filter(|_| !tagged) {
            crate::step(1);
            if line.is_blank() {
                // A comment or a processing instruction that is open at the
                // blank line continues the paragraph to the line of its end.
                if let Some(m) = self.open_raw_end(lines, scan_from..j, at, fences) {
                    scan_from = m;
                    tagged = lines.get(m).is_some_and(|l| has_tag(self, l));
                    j = m + 1;
                    continue;
                }
                para = Some(true);
                break;
            }
            // Only a backtick fence with no space before it ends a paragraph.
            // A tilde fence or an indented fence is text of the paragraph.
            if line.t().starts_with('`')
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
            if let Some(kind) = self.interrupts(line.t()) {
                para = Some(kind);
                break;
            }
            tagged = has_tag(self, line);
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
        let inlines = vec![placeholder(text.trim_start_matches(SPACE))];
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
        if self.split_budget == 0
            || !self.ext(Extension::RawHtml)
            || !(self.ext(Extension::MarkdownInHtmlBlocks)
                || self.ext(Extension::MarkdownAttribute))
        {
            return None;
        }
        let bytes = text.as_bytes();
        let brackets = matching_brackets(bytes);
        // The lengths of the backtick runs that have no closer after the
        // current position. A later run of such a length has none either.
        let mut no_closer: HashSet<usize> = HashSet::new();
        let mut no_dollar = false;
        let mut no_raw_end = [false; 2];
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
                b'[' => i = brackets.get(&i).map_or(i + 1, |close| close + 1),
                b'$' if !no_dollar && self.ext(Extension::TexMathDollars) => {
                    match bytes.get(i + 1..)?.iter().position(|c| *c == b'$') {
                        Some(k) => i += k + 2,
                        None => {
                            no_dollar = true;
                            i += 1;
                        }
                    }
                }
                b'<' => {
                    let rest = text.get(i..)?;
                    if i > 0 && is_mid_line_block_tag(rest) {
                        return Some(i);
                    }
                    // A comment or a processing instruction with no end is
                    // not read again: a later one has no end either.
                    let kind =
                        starts_comment_or_pi(rest).then(|| usize::from(rest.starts_with("<?")));
                    let len = match kind {
                        Some(k) if no_raw_end[k] => None,
                        _ => html_tag_len(rest),
                    };
                    if let (Some(k), None) = (kind, len) {
                        no_raw_end[k] = true;
                    }
                    i += len.unwrap_or(1);
                }
                _ => i += 1,
            }
        }
        None
    }

    /// Ends the paragraph of `lines` at the byte offset `k` of its text, where
    /// a block tag starts, and reads the tag and the rest as blocks.
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
        let parsed = self.html_block_at(rest, &mut blocks);
        line.restore(saved);
        let Some(n) = parsed? else {
            return Ok(None);
        };
        out.push(Block::Plain(vec![placeholder(before.trim_matches(SPACE))]));
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
        if fences.has_no_closer(c, at, n) {
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
            let ti = indent(t);
            if ti < 4 {
                let tt = t.trim_matches(SPACE);
                let m = tt.len() - tt.trim_start_matches(c).len();
                if m >= n && tt.trim_start_matches(c).is_empty() {
                    closed = true;
                    j += 1;
                    break;
                }
            }
            body.push(dedent(t, ind));
            j += 1;
        }
        if !closed {
            fences.no_closer(c, at, n);
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
        let t = s.trim_end_matches(SPACE);
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
            rest.trim_matches(SPACE).to_string(),
        ))
    }

    fn header(&mut self, lines: &[Ln], out: &mut Vec<Block>) -> Option<usize> {
        let first = lines.first()?;
        let (level, content, used) = if let Some((level, content)) = self.atx(first.t()) {
            // A block tag in the text makes the line a paragraph.
            if self.inline_block_tag(&content).is_some() {
                return None;
            }
            (level, content, 1)
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
            (level, first.t().trim_matches(SPACE).to_string(), 2)
        };
        let (text, attr) = self.header_attr(&content);
        let text = if used == 1 {
            trim_end_unescaped(text.trim_end_matches('#')).to_string()
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
        out.push(Block::Header(level, attr, vec![placeholder(&text)]));
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

    fn header_attr(&self, content: &str) -> (Text, Attr) {
        if self.ext(Extension::HeaderAttributes)
            && content.ends_with('}')
            && let Some(open) = content.rfind('{')
            && !self.inline_takes_attr(content.get(..open).unwrap_or(""))
            && let Some(attr) = self.parse_attr(content.get(open..).unwrap_or(""))
        {
            let text = trim_end_unescaped(content.get(..open).unwrap_or(""));
            return (text.to_string(), attr);
        }
        if self.ext(Extension::MmdHeaderIdentifiers)
            && content.ends_with(']')
            && let Some(open) = content.rfind('[')
        {
            let id = content.get(open + 1..content.len() - 1).unwrap_or("");
            if !id.is_empty() && !id.contains([' ', '[', ']']) {
                let text = content.get(..open).unwrap_or("").trim_end_matches(SPACE);
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
        Some(Marker {
            kind,
            start,
            label,
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
        } else if let Some(label) = token.strip_prefix('@') {
            if !self.ext(Extension::ExampleLists)
                || !label
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                return None;
            }
            candidates.push((Example, 1, Some(label.to_string())));
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
                        self.example_count += 1;
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
            let is_marker =
                |l: &Ln| l.indent() == 0 && !is_hrule(l.t()) && self.marker(l.t(), None).is_some();
            let mut sublist = None;
            for i in (1..=last_text).rev() {
                let Some(l) = item.get(i) else { break };
                if is_marker(l) {
                    sublist = Some(i);
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
        if *format != Format::new(PLACEHOLDER) {
            return;
        }
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
            inlines.push(placeholder(rest));
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
            items.push((vec![placeholder(term.t().trim_matches(SPACE))], defs));
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
                inlines.push(placeholder(&format!("{rest}\\")));
            } else if !rest.is_empty() {
                inlines.push(placeholder(rest));
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
                let found = div_closer_scan(lines, |open, close| results.push((open, close)));
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
        crate::check_cache(&found, || div_closer_scan(lines, |_, _| {}));
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
                let found = scan.run(lines, |open, close| results.push((open, close)));
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
        crate::check_cache(&found, || scan.run(lines, |_, _| {}));
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
        // Join the lines so that a tag can span lines. A raw element can
        // contain blank lines, so it uses all lines; a tag ends at the first
        // blank line.
        let needs_all = html_tag_name(s).is_some_and(|(name, _)| {
            matches!(name.as_str(), "pre" | "script" | "style" | "textarea")
                || !self.ext(Extension::MarkdownInHtmlBlocks)
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
        if !native_div && !self.ext(Extension::RawHtml) {
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
        let in_markdown = self.ext(Extension::MarkdownInHtmlBlocks) || native_div;
        if verbatim || !in_markdown {
            let close = format!("</{name}");
            let mut depth = 0usize;
            let lower = text.to_ascii_lowercase();
            let open = format!("<{name}");
            let mut idx = 0;
            let mut end = None;
            while idx < lower.len() {
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
                } else {
                    idx += rest.chars().next().map_or(1, char::len_utf8);
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
            let following = lines.get(tag_lines + 1..).unwrap_or(&[]);
            let saved = std::mem::replace(&mut self.in_list, false);
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
        self.gobble = content_indent;
        let blocks = if whole_lines {
            let content = lines.get(tag_lines + 1..close_line).unwrap_or(&[]);
            self.blocks_open(content, native_div)
        } else {
            if native_div && closer_own_line {
                inner.push(Ln::new("", 0));
            }
            self.blocks(&inner)
        };
        self.in_list = saved;
        let blocks = blocks?;
        if native_div {
            let attr = html_attr(tag);
            out.push(Block::Div(attr, blocks));
        } else {
            out.push(Block::RawBlock(Format::new("html"), tag.to_string()));
            out.extend(blocks);
            if let Some((closer, _)) = &close {
                out.push(Block::RawBlock(Format::new("html"), closer.clone()));
            }
        }
        if let Some((_, rest)) = close
            && !blank(rest)
        {
            let n = lines.get(j.saturating_sub(1)).map_or(0, |l| l.n);
            let mut more = vec![rest_line(
                lines,
                close_line,
                rest.trim_start_matches(SPACE),
                n,
            )];
            let mut k = j;
            while let Some(line) = lines.get(k) {
                if line.is_blank() {
                    break;
                }
                more.push(line.clone());
                k += 1;
            }
            more.push(Ln::new("", 0));
            out.extend(self.blocks(&more)?);
            return Ok(Some(k));
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
            let used = self.block_at(rest, 0, false, &mut FenceMemo::default(), out);
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

    fn raw_tex_block(&mut self, lines: &[Ln], out: &mut Vec<Block>) -> Option<usize> {
        if !self.ext(Extension::RawTex) {
            return None;
        }
        let first = lines.first()?;
        let s = first.t().trim_start_matches(SPACE);
        const DEFINITIONS: &[&str] = &[
            "\\newcommand",
            "\\renewcommand",
            "\\providecommand",
            "\\def\\",
            "\\let\\",
            "\\DeclareMathOperator",
            "\\newenvironment",
        ];
        if DEFINITIONS.iter().any(|d| s.starts_with(d)) {
            let mut j = 0;
            while lines.get(j).is_some_and(|l| !l.is_blank()) {
                j += 1;
            }
            let body: Vec<&str> = lines
                .get(..j)
                .unwrap_or(&[])
                .iter()
                .map(|l| l.t())
                .collect();
            out.push(Block::RawBlock(Format::new("tex"), body.join("\n")));
            return Some(j);
        }
        let rest = s.strip_prefix("\\begin{")?;
        let name: Text = rest.chars().take_while(|c| *c != '}').collect();
        if name.is_empty()
            || !rest.get(name.len()..)?.starts_with('}')
            || MATH_ENVIRONMENTS.contains(&name.trim_end_matches('*'))
        {
            return None;
        }
        let end = format!("\\end{{{name}}}");
        let begin = format!("\\begin{{{name}}}");
        let mut depth = 0usize;
        let mut j = 0;
        while let Some(line) = lines.get(j) {
            crate::step(1);
            depth += line.t().matches(&begin).count();
            let closes = line.t().matches(&end).count();
            if closes >= depth && closes > 0 {
                let tail = line.t().rsplit(&end).next().unwrap_or("");
                let mut body: Vec<&str> = lines
                    .get(..j)
                    .unwrap_or(&[])
                    .iter()
                    .map(|l| l.t())
                    .collect();
                let last = line.t().get(..line.t().len() - tail.len()).unwrap_or("");
                body.push(last);
                if !blank(tail) {
                    return None;
                }
                out.push(Block::RawBlock(Format::new("tex"), body.join("\n")));
                return Some(j + 1);
            }
            depth = depth.saturating_sub(closes);
            j += 1;
        }
        None
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
        let mut markdown = |s: &str| -> Vec<Block> { self.sub_document(s) };
        let fields =
            crate::yaml::parse_yaml_metadata(&body, &source, first_line, max_depth, &mut markdown);
        match fields {
            Ok(Some(fields)) => {
                for (key, value) in fields {
                    if self.meta.get(&key).is_none() {
                        self.meta.insert(key, value);
                    }
                }
                Ok(Some(end + 1))
            }
            Ok(None) => Ok(None),
            Err(error) => Err(Error::Parse(error.message)),
        }
    }

    /// Parses a string of a metadata field as blocks with the main reader state.
    fn sub_document(&mut self, text: &str) -> Vec<Block> {
        // Some blocks need the line break at the end of their line, so one
        // line without it is a paragraph.
        if !text.contains('\n') && self.needs_line_end(text) {
            let para = Block::Para(vec![placeholder(text.trim_matches(SPACE))]);
            return self.resolve_blocks(vec![para]);
        }
        let mut lines: Vec<Ln> = text
            .split('\n')
            .map(|t| Ln::new(expand_tabs(t, 4), 0))
            .collect();
        if text.ends_with('\n') {
            lines.pop();
            lines.push(Ln::new("", 0));
        }
        let blocks = self.blocks(&lines).unwrap_or_default();
        self.resolve_blocks(blocks)
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
        if label.starts_with('^') && self.ext(Extension::Footnotes) {
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
        let t = text.trim_matches(SPACE);
        if t.starts_with('[') {
            return None;
        }
        let (url, after) = if let Some(inner) = t.strip_prefix('<') {
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
        let attr = if after.is_empty() {
            Attr::default()
        } else if self.ext(Extension::LinkAttributes) && after.starts_with('{') {
            self.parse_attr(&after)?
        } else {
            return None;
        };
        let key = normalize_key(label);
        let target = Target::new(escape_uri(&crate::inline::unescape(&url)), title);
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
        let mut inner = vec![Ln::new(rest.trim_start_matches(SPACE), first.n)];
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
        let (attr, used) = crate::inline::parse_attributes(text)?;
        (used == text.len()).then_some(attr)
    }

    // ----- the inline phase -----

    fn resolve_document(&mut self, mut blocks: Vec<Block>) -> Vec<Block> {
        self.assign_header_ids(&mut blocks);
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
        let counts = (self.emphasis_failures, self.block_tag_cuts);
        let out = f(self);
        self.note_number = note_number;
        self.used_notes.truncate(used_notes);
        self.log.truncate(log);
        (self.emphasis_failures, self.block_tag_cuts) = counts;
        out
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
        Some(Block::Figure(
            Attr::new(id, vec![], vec![]),
            Caption {
                short: None,
                long: vec![Block::Plain(alt.clone())],
            },
            vec![Block::Plain(vec![Inline::Image(
                image_attr,
                alt.clone(),
                target.clone(),
            )])],
        ))
    }

    fn resolve_block(&mut self, block: Block) -> Block {
        match block {
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
            Block::Figure(attr, caption, xs) => {
                Block::Figure(attr, caption, self.resolve_blocks(xs))
            }
            other => other,
        }
    }

    pub(crate) fn resolve_inlines(&mut self, inlines: Vec<Inline>) -> Vec<Inline> {
        let mut out = Vec::new();
        for inline in inlines {
            match inline {
                Inline::RawInline(format, text) if format.as_str() == PLACEHOLDER => {
                    out.extend(self.parse_inlines(&text));
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
            vec![placeholder(&format!("{t}\\"))]
        } else {
            vec![placeholder(t)]
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
pub(crate) fn placeholder(text: &str) -> Inline {
    Inline::RawInline(Format::new(PLACEHOLDER), text.to_string())
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
            ']' if depth == 0 => {
                let label = inner.get(..i)?;
                if label.is_empty() {
                    return None;
                }
                return Some((label, inner.get(i + 1..)?));
            }
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

/// Parses the attributes of an HTML start tag into an `Attr`.
pub(crate) fn html_attr(tag: &str) -> Attr {
    let mut attr = Attr::default();
    let inner = tag
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim_end_matches('/');
    let mut rest = inner
        .trim_start_matches(|c: char| c.is_ascii_alphanumeric())
        .trim_start_matches(SPACE);
    while !rest.is_empty() {
        let name_len = rest
            .find(|c: char| c == '=' || c.is_whitespace())
            .unwrap_or(rest.len());
        let name = rest.get(..name_len).unwrap_or("").to_string();
        rest = rest.get(name_len..).unwrap_or("").trim_start_matches(SPACE);
        let mut value = Text::new();
        if let Some(r) = rest.strip_prefix('=') {
            let r = r.trim_start_matches(SPACE);
            if let Some(q) = r.chars().next().filter(|c| *c == '"' || *c == '\'') {
                let body = r.get(1..).unwrap_or("");
                let end = body.find(q).unwrap_or(body.len());
                value = body.get(..end).unwrap_or("").to_string();
                rest = body.get(end + 1..).unwrap_or("").trim_start_matches(SPACE);
            } else {
                let end = r.find(char::is_whitespace).unwrap_or(r.len());
                value = r.get(..end).unwrap_or("").to_string();
                rest = r.get(end..).unwrap_or("").trim_start_matches(SPACE);
            }
        }
        if name.is_empty() {
            break;
        }
        let value = crate::inline::decode_entities(&value);
        match name.as_str() {
            "id" => attr.id = value,
            "class" => attr.classes = value.split_whitespace().map(str::to_string).collect(),
            _ => attr.attributes.push((name, value)),
        }
    }
    attr
}
