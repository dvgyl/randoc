//! The tree rules above the tokens. Each rule was found with Pandoc 3.13 as
//! a black box.
//!
//! The parser is a recursive descent over the token list. An element whose
//! content does not fit (for example a `b` element with a `</div>` inside)
//! fails, and the caller reads its tag as unknown markup and its content
//! again, as Pandoc does. Three things keep that linear: a failure is
//! remembered for its token, a failed inline element is not tried a second
//! way, and a step budget stops input that needs more work than that.

use randoc_ast::{
    Attr, Block, Caption, Format, Inline, ListAttributes, ListNumberDelim, ListNumberStyle,
    MathType, MetaValue, QuoteType, Target, Text,
};
use randoc_core::shared::stringify;
use randoc_core::{Extension, Extensions, LogMessage, SourcePos};

use crate::text::{
    Inlines, TextMode, attribute, clean_attributes, escape_uri, is_blank, quoted_text_inlines,
    read_integer, style_property, text_inlines, to_attr,
};
use crate::tokens::{Token, TokenKind, escape_into, render_close, render_comment, render_open};

/// Why a rule stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// The rule does not match here. The caller can try another rule.
    Fail,
    /// The input is nested deeper than the limit.
    TooDeep,
    /// The input needs more backtracking than the step budget permits.
    Budget,
    /// The tables need more generated cells than `table::MAX_GENERATED_CELLS`.
    TooLarge,
    /// Pandoc stops with an error at the token with this index.
    Fatal(usize),
}

pub(crate) type Parse<T> = Result<T, Stop>;

/// One item of a definition list: the term and the definitions.
type DefinitionItem = (Vec<Inline>, Vec<Vec<Block>>);

/// The format name of a placeholder for a note reference. Input cannot make
/// a raw inline with this format.
pub(crate) const NOTE_REF_FORMAT: &str = "\u{0}randoc-noteref";

/// The steps that the parser can use for each token before it stops.
const STEPS_PER_TOKEN: u64 = 64;

const FAILED_IN_PARA: u8 = 1;
const FAILED_IN_PLAIN: u8 = 2;
const FAILED_BLOCK: u8 = 4;
const FAILED_DEFINITION_PART: u8 = 8;

/// The tags of HTML that are block-level for Pandoc. The close tag of one of
/// them ends an open `p` element. The list is sorted. Found with the oracle
/// from a list of candidates.
#[rustfmt::skip]
const HTML_BLOCK_TAGS: &[&str] = &[
    "!DOCTYPE", "?xml", "address", "article", "aside", "blockquote", "body", "canvas", "caption",
    "center", "col", "colgroup", "dd", "details", "dir", "div", "dl", "dt", "fieldset",
    "figcaption", "figure", "footer", "form", "frameset", "h1", "h2", "h3", "h4", "h5", "h6",
    "head", "header", "hgroup", "hr", "html", "isindex", "li", "main", "menu", "meta", "nav",
    "noframes", "ol", "output", "p", "pre", "script", "section", "style", "summary", "table",
    "tbody", "td", "textarea", "tfoot", "th", "thead", "tr", "ul",
];

/// The other tags that are block-level for Pandoc: tags of DocBook, of EPUB,
/// and `title`. Their close tag inside a `p` element makes the element fail.
/// The list is sorted.
#[rustfmt::skip]
const OTHER_BLOCK_TAGS: &[&str] = &[
    "bibliolist", "calloutlist", "case", "caution", "classsynopsis", "cmdsynopsis", "default",
    "epigraph", "equation", "example", "formalpara", "funcsynopsis", "glosslist", "important",
    "informalequation", "informalexample", "informalfigure", "informaltable", "itemizedlist",
    "literallayout", "mediaobject", "msgset", "note", "orderedlist", "para", "procedure",
    "programlisting", "programlistingco", "qandaset", "screen", "screenco", "screenshot",
    "segmentedlist", "sidebar", "simpara", "simplelist", "switch", "synopsis", "task", "tip",
    "title", "variablelist", "warning",
];

/// The tags that are block-level or inline. The list is sorted.
#[rustfmt::skip]
const EITHER_TAGS: &[&str] = &[
    "applet", "area", "audio", "button", "del", "embed", "iframe", "ins", "map", "noscript",
    "object", "progress", "source", "svg", "track", "video",
];

/// The open tags that end an open `p` element. The list is sorted.
const CLOSES_P: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "dir",
    "div",
    "dl",
    "fieldset",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "main",
    "menu",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
];

fn in_list(list: &[&str], name: &str) -> bool {
    list.binary_search(&name).is_ok()
}

fn is_block_name(name: &str) -> bool {
    in_list(HTML_BLOCK_TAGS, name) || in_list(OTHER_BLOCK_TAGS, name)
}

/// A processing instruction and a declaration are block-level or inline too.
fn is_either_name(name: &str) -> bool {
    in_list(EITHER_TAGS, name) || name.starts_with(['?', '!'])
}

fn is_heading(name: &str) -> bool {
    matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

/// Returns true when the open tag `opening` ends an open element `open`.
fn closes(opening: &str, open: &str) -> bool {
    match (opening, open) {
        (_, "body" | "html") => false,
        ("body", "head") | ("a", "a") | ("li", "li") => true,
        ("th" | "td", "th" | "td") => true,
        ("tr", "th" | "td" | "tr" | "colgroup") => true,
        ("dd" | "dt", "dd" | "dt") => true,
        ("rt", "rb") => true,
        ("optgroup", "optgroup" | "option") | ("option", "option") => true,
        (_, "p") if in_list(CLOSES_P, opening) => true,
        (_, "meta") => true,
        ("form", "form") | ("label", "label") | ("map", "map") | ("object", "object") => true,
        (_, "option" | "style" | "script" | "textarea" | "title") => true,
        (_, "select") if opening != "option" => true,
        ("thead", "colgroup") => true,
        ("tfoot", "thead" | "colgroup") => true,
        ("tbody", "tbody" | "tfoot" | "thead" | "colgroup") => true,
        _ if (is_heading(opening)
            || matches!(opening, "dl" | "ol" | "ul" | "table" | "div" | "main" | "p"))
            && (is_heading(open) || open == "p") =>
        {
            true
        }
        _ => is_block_name(opening) && !is_block_name(open) && !is_either_name(open),
    }
}

/// Returns, for each token, the index of the next `dt` or `dd` open tag or
/// `dl` close tag at or after it, or the length of the list.
fn next_definition_parts(toks: &[Token]) -> Vec<usize> {
    let mut next = vec![toks.len(); toks.len()];
    let mut part = toks.len();
    for (index, token) in toks.iter().enumerate().rev() {
        let is_part = match &token.kind {
            TokenKind::Open(name, _) => matches!(name.as_str(), "dt" | "dd"),
            TokenKind::Close(name) => name == "dl",
            _ => false,
        };
        if is_part {
            part = index;
        }
        next[index] = part;
    }
    next
}

/// The state that a failed rule restores.
#[derive(Clone, Copy)]
struct Snapshot {
    pos: usize,
    generated_cells: usize,
    logs: usize,
    notes: usize,
    headings: usize,
}

/// The parser of one token list.
pub(crate) struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
    extensions: Extensions,
    source: Text,
    depth: usize,
    max_depth: usize,
    pub(crate) steps: u64,
    max_steps: u64,
    failed: Vec<u8>,
    in_plain: bool,
    mode: TextMode,
    /// The innermost `q` element: a double quotation, or a single one inside it.
    quote_context: Option<QuoteType>,
    in_list_item: bool,
    /// The cells that the tables of the document made without input text
    /// for them; see `table::generated_cells`. A failed rule gives them back.
    pub(crate) generated_cells: usize,
    /// The number of open code elements.
    code_depth: usize,
    /// True when the last element ended at the end of the input.
    ended_at_eof: bool,
    /// The last token at which an inline rule failed.
    furthest_failure: usize,
    close_tables: Vec<(&'static str, Vec<u32>)>,
    /// For each token, the index of the next `dt` or `dd` open tag or `dl`
    /// close tag at or after it. Empty until the first definition list.
    next_definition_part: Vec<usize>,
    /// The token after each heading, in parse order, for a duplicate identifier message.
    pub(crate) heading_ends: Vec<usize>,
    /// The messages, each with the index of the token that it is about.
    pub(crate) logs: Vec<(usize, LogMessage)>,
    /// The notes of the document: the identifier, the content, and the
    /// indexes in `heading_ends` of the headings that the content holds.
    pub(crate) notes: Vec<(Text, Vec<Block>, std::ops::Range<usize>)>,
    /// The metadata of the `head` element, in source order.
    pub(crate) meta: Vec<(Text, MetaValue)>,
    meta_index: std::collections::HashMap<Text, usize>,
    /// The `href` of the `base` element.
    pub(crate) base: Option<Text>,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(
        toks: &'a [Token],
        extensions: Extensions,
        source: Text,
        max_depth: usize,
    ) -> Self {
        let tokens = u64::try_from(toks.len()).unwrap_or(u64::MAX);
        Parser {
            toks,
            pos: 0,
            extensions,
            source,
            depth: 0,
            max_depth,
            steps: 0,
            max_steps: tokens.saturating_add(16).saturating_mul(STEPS_PER_TOKEN),
            failed: vec![0; toks.len()],
            in_plain: false,
            mode: TextMode::Normal,
            quote_context: None,
            in_list_item: false,
            code_depth: 0,
            generated_cells: 0,
            ended_at_eof: false,
            furthest_failure: 0,
            close_tables: Vec::new(),
            next_definition_part: Vec::new(),
            heading_ends: Vec::new(),
            logs: Vec::new(),
            notes: Vec::new(),
            meta: Vec::new(),
            meta_index: std::collections::HashMap::new(),
            base: None,
        }
    }

    fn has(&self, extension: Extension) -> bool {
        self.extensions.contains(extension)
    }

    fn peek(&self) -> Option<&'a TokenKind> {
        self.toks.get(self.pos).map(|token| &token.kind)
    }

    fn at_end(&self) -> bool {
        self.pos >= self.toks.len()
    }

    fn step(&mut self) -> Parse<()> {
        self.steps += 1;
        if self.steps > self.max_steps {
            return Err(Stop::Budget);
        }
        Ok(())
    }

    fn nested<T>(&mut self, rule: impl FnOnce(&mut Self) -> Parse<T>) -> Parse<T> {
        if self.depth >= self.max_depth {
            return Err(Stop::TooDeep);
        }
        self.depth += 1;
        let result = rule(self);
        self.depth -= 1;
        result
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            pos: self.pos,
            generated_cells: self.generated_cells,
            logs: self.logs.len(),
            notes: self.notes.len(),
            headings: self.heading_ends.len(),
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.pos = snapshot.pos;
        self.generated_cells = snapshot.generated_cells;
        self.logs.truncate(snapshot.logs);
        self.notes.truncate(snapshot.notes);
        self.heading_ends.truncate(snapshot.headings);
    }

    fn position(&self, index: usize) -> SourcePos {
        let (line, column) = self
            .toks
            .get(index)
            .map_or((1, 1), |token| (token.line, token.column));
        SourcePos::new(self.source.clone(), line, column)
    }

    /// Renders the token at `index` as HTML text.
    fn render(&self, index: usize, out: &mut String) {
        match self.toks.get(index).map(|token| &token.kind) {
            Some(TokenKind::Open(name, attributes)) => render_open(name, attributes, out),
            Some(TokenKind::Close(name)) => render_close(name, out),
            Some(TokenKind::Comment(text)) => render_comment(text, out),
            Some(TokenKind::Text(text)) => out.push_str(text),
            None => {}
        }
    }

    /// Returns the index of the next close tag `name` after the current token.
    ///
    /// The first call for a name makes a table for all positions, so that
    /// many unclosed elements do not scan the input many times.
    fn next_close(&mut self, name: &'static str) -> Option<usize> {
        let table = match self.close_tables.iter().position(|(key, _)| *key == name) {
            Some(index) => index,
            None => {
                let mut table = vec![u32::MAX; self.toks.len() + 1];
                let mut next = u32::MAX;
                for (index, token) in self.toks.iter().enumerate().rev() {
                    if matches!(&token.kind, TokenKind::Close(close) if close == name) {
                        next = u32::try_from(index).unwrap_or(u32::MAX);
                    }
                    if let Some(slot) = table.get_mut(index) {
                        *slot = next;
                    }
                }
                self.close_tables.push((name, table));
                self.close_tables.len() - 1
            }
        };
        let next = *self.close_tables.get(table)?.1.get(self.pos + 1)?;
        (next != u32::MAX).then_some(next as usize)
    }

    /// Returns the index after the close tag of the element at the current
    /// token, or the index after the open tag when the element has no close tag.
    fn raw_element_end(&mut self, name: &'static str) -> usize {
        self.next_close(name)
            .map_or(self.pos + 1, |close| close + 1)
    }

    /// Returns the index after the close tag of the element at the current
    /// token, or the end of the input.
    fn element_end(&mut self, name: &'static str) -> usize {
        self.next_close(name)
            .map_or(self.toks.len(), |close| close + 1)
    }

    /// Consumes the tokens up to `end` as markup that Pandoc does not read.
    /// The result is the raw text with `raw_html`, or nothing and a message.
    fn skip_raw(&mut self, end: usize) -> Option<Text> {
        let start = self.pos;
        let mut raw = String::new();
        for index in start..end {
            self.render(index, &mut raw);
        }
        self.pos = end;
        if self.has(Extension::RawHtml) {
            Some(raw)
        } else {
            // Pandoc gives the position of the last token of the skipped part.
            let position = self.position(end.saturating_sub(1).max(start));
            self.logs
                .push((start, LogMessage::SkippedContent(raw, position)));
            None
        }
    }

    // ---- The end of an element.

    /// Returns true when the element `tag` ends at the current token. The
    /// close tag of the element is consumed.
    fn ends(&mut self, tag: &str) -> bool {
        self.ended_at_eof = self.at_end();
        match self.peek() {
            None => true,
            Some(TokenKind::Close(name)) if name == tag => {
                self.pos += 1;
                true
            }
            Some(TokenKind::Open(name, _)) => closes(name, tag),
            // The close tag of a table cell ends each open block element.
            Some(TokenKind::Close(name))
                if matches!(name.as_str(), "td" | "th") && is_block_name(tag) =>
            {
                true
            }
            Some(TokenKind::Close(name)) => match tag {
                "li" => matches!(name.as_str(), "ul" | "ol"),
                "dd" | "dt" => name == "dl",
                "td" | "th" => {
                    matches!(name.as_str(), "tr" | "tbody" | "thead" | "tfoot" | "table")
                }
                "tr" => matches!(name.as_str(), "tbody" | "thead" | "tfoot" | "table"),
                "thead" | "tbody" | "tfoot" | "caption" | "colgroup" => name == "table",
                "p" => in_list(HTML_BLOCK_TAGS, name),
                _ => false,
            },
            Some(_) => false,
        }
    }

    // ---- Blocks.

    /// Reads blocks to the end of the input.
    pub(crate) fn document(&mut self) -> Parse<Vec<Block>> {
        let mut out = Vec::new();
        while !self.at_end() {
            self.block(&mut out)?;
        }
        Ok(fix_plains(out, false))
    }

    /// Reads the content of a block container whose open tag is the current token.
    fn blocks_in(&mut self, tag: &str) -> Parse<Vec<Block>> {
        self.nested(|parser| {
            parser.pos += 1;
            let mut out = Vec::new();
            while !parser.ends(tag) {
                parser.block(&mut out)?;
            }
            Ok(out)
        })
    }

    fn block(&mut self, out: &mut Vec<Block>) -> Parse<()> {
        self.step()?;
        let Some(kind) = self.peek() else {
            return Ok(());
        };
        match kind {
            TokenKind::Text(text) if is_blank(text) && self.mode == TextMode::Normal => {
                self.pos += 1;
                Ok(())
            }
            TokenKind::Open(name, attributes) => self.block_element(name, attributes, out),
            TokenKind::Close(name) if matches!(name.as_str(), "html" | "body" | "head") => {
                self.pos += 1;
                Ok(())
            }
            _ => self.plain_or_raw(out),
        }
    }

    fn block_element(
        &mut self,
        name: &'a str,
        attributes: &'a [(Text, Text)],
        out: &mut Vec<Block>,
    ) -> Parse<()> {
        let divs = self.has(Extension::NativeDivs);
        match name {
            "html" | "body" => {
                self.set_lang(attributes);
                self.pos += 1;
                Ok(())
            }
            "!DOCTYPE" => {
                self.pos += 1;
                Ok(())
            }
            "head" => {
                self.head();
                Ok(())
            }
            "p" => self.attempt(out, Self::para),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => self.attempt(out, Self::header),
            "blockquote" => {
                let content = self.blocks_in("blockquote")?;
                out.push(Block::BlockQuote(fix_plains(content, false)));
                Ok(())
            }
            "pre" => self.attempt(out, Self::pre),
            "ul" | "ol" => self.attempt(out, Self::list),
            "dl" => self.attempt(out, Self::definition_list),
            "hr" => {
                self.pos += 1;
                self.skip_close("hr");
                out.push(Block::HorizontalRule);
                Ok(())
            }
            "table" => self.attempt(out, Self::table),
            // A `div` with the class `line-block` and other attributes is
            // unknown markup, also with `native_divs`.
            "div"
                if self.has(Extension::LineBlocks)
                    && attribute(&clean_attributes(attributes), "class") == Some("line-block") =>
            {
                if attributes.len() == 1 {
                    self.attempt(out, Self::line_block)
                } else {
                    self.raw_block(out);
                    Ok(())
                }
            }
            "div" | "section" | "header" | "aside" | "main"
                if divs || attribute(attributes, "role") == Some("doc-endnotes") =>
            {
                self.attempt(out, Self::div)
            }
            "figure" => self.attempt(out, Self::figure),
            "script" if self.is_math_script() => self.plain_or_raw(out),
            "iframe"
                if self.has(Extension::RawHtml)
                    || attribute(attributes, "src").is_none_or(str::is_empty) =>
            {
                self.plain_or_raw(out)
            }
            "script" | "style" | "textarea" | "iframe" => {
                let end = match name {
                    "script" => self.raw_element_end("script"),
                    "style" => self.raw_element_end("style"),
                    "textarea" => self.raw_element_end("textarea"),
                    _ => self.element_end("iframe"),
                };
                if let Some(raw) = self.skip_raw(end) {
                    out.push(Block::RawBlock(Format::new("html"), raw));
                }
                Ok(())
            }
            _ => self.plain_or_raw(out),
        }
    }

    /// Runs a block rule. When the rule fails, the tag is unknown markup.
    fn attempt(
        &mut self,
        out: &mut Vec<Block>,
        rule: fn(&mut Self, &mut Vec<Block>) -> Parse<()>,
    ) -> Parse<()> {
        let start = self.pos;
        let known_failure = self
            .failed
            .get(start)
            .is_some_and(|f| f & FAILED_BLOCK != 0);
        if !known_failure {
            let snapshot = self.snapshot();
            let mut blocks = Vec::new();
            match rule(self, &mut blocks) {
                Ok(()) => {
                    out.append(&mut blocks);
                    return Ok(());
                }
                Err(Stop::Fail) => {
                    self.restore(snapshot);
                    if let Some(flags) = self.failed.get_mut(start) {
                        *flags |= FAILED_BLOCK;
                    }
                }
                Err(stop) => return Err(stop),
            }
        }
        self.raw_block(out);
        Ok(())
    }

    /// Reads inlines as a `Plain` block, or one tag as unknown markup.
    fn plain_or_raw(&mut self, out: &mut Vec<Block>) -> Parse<()> {
        let start = self.pos;
        let was_plain = std::mem::replace(&mut self.in_plain, true);
        let mut inlines = Inlines::new();
        let result = loop {
            if self.at_end() {
                break Ok(());
            }
            match self.inline(&mut inlines) {
                Ok(()) => {}
                Err(Stop::Fail) => break Ok(()),
                Err(stop) => break Err(stop),
            }
        };
        self.in_plain = was_plain;
        result?;
        if self.pos == start {
            self.raw_block(out);
            return Ok(());
        }
        let inlines = inlines.trimmed();
        if !inlines.is_empty() {
            out.push(Block::Plain(inlines.0));
        }
        Ok(())
    }

    /// Consumes one tag as unknown markup at block level.
    fn raw_block(&mut self, out: &mut Vec<Block>) {
        let silent = match self.peek() {
            Some(TokenKind::Open(name, _)) => matches!(name.as_str(), "!DOCTYPE" | "?xml"),
            _ => false,
        };
        if silent {
            self.pos += 1;
            return;
        }
        if let Some(raw) = self.skip_raw(self.pos + 1) {
            out.push(Block::RawBlock(Format::new("html"), raw));
        }
    }

    fn skip_close(&mut self, name: &str) {
        if matches!(self.peek(), Some(TokenKind::Close(close)) if close == name) {
            self.pos += 1;
        }
    }

    fn para(&mut self, out: &mut Vec<Block>) -> Parse<()> {
        let content = self.inlines_in("p")?.trimmed();
        if !content.is_empty() || self.has(Extension::EmptyParagraphs) {
            out.push(Block::Para(content.0));
        }
        Ok(())
    }

    fn header(&mut self, out: &mut Vec<Block>) -> Parse<()> {
        let Some(TokenKind::Open(name, attributes)) = self.peek() else {
            return Err(Stop::Fail);
        };
        let level = name
            .strip_prefix('h')
            .and_then(|digits| digits.parse::<i64>().ok())
            .ok_or(Stop::Fail)?;
        let clean = clean_attributes(attributes);
        let mut attr = to_attr(&clean);
        // A heading does not take its `epub:type` values as classes.
        let types =
            attribute(&clean, "epub:type").map_or(0, |types| types.split_whitespace().count());
        attr.classes.truncate(attr.classes.len() - types);
        let content = self.inlines_in(name)?.trimmed();
        let closed = matches!(
            self.toks.get(self.pos.wrapping_sub(1)).map(|token| &token.kind),
            Some(TokenKind::Close(close)) if close == name
        );
        self.heading_ends
            .push(if closed { self.pos - 1 } else { self.pos });
        out.push(Block::Header(level, attr, content.0));
        Ok(())
    }

    fn div(&mut self, out: &mut Vec<Block>) -> Parse<()> {
        let Some(TokenKind::Open(name, attributes)) = self.peek() else {
            return Err(Stop::Fail);
        };
        let clean = clean_attributes(attributes);
        let mut attr = to_attr(&clean);
        if attribute(&clean, "role") == Some("doc-endnotes") {
            return self.endnotes(name, attr, out);
        }
        if self.has(Extension::EpubHtmlExts) {
            let kinds = attribute(&clean, "epub:type").unwrap_or_default();
            if kinds
                .split_whitespace()
                .any(|kind| matches!(kind, "footnote" | "rearnote"))
            {
                let first = self.heading_ends.len();
                let content = self.blocks_in(name)?;
                let headings = first..self.heading_ends.len();
                self.notes.push((attr.id, content, headings));
                return Ok(());
            }
            if name == "section" && !kinds.is_empty() {
                out.append(&mut self.blocks_in(name)?);
                return Ok(());
            }
        }
        match name.as_str() {
            "section" | "header" | "aside" => attr.classes.insert(0, name.clone()),
            "main" if attr.get("role").is_none() => {
                attr.attributes.push(("role".to_owned(), "main".to_owned()));
            }
            _ => {}
        }
        let content = self.blocks_in(name)?;
        out.push(Block::Div(attr, content));
        Ok(())
    }

    /// The section of the notes that the HTML writer of Pandoc makes: each
    /// list item with an identifier is a note. The rest stays in a `Div`.
    fn endnotes(&mut self, name: &str, attr: Attr, out: &mut Vec<Block>) -> Parse<()> {
        let content = self.nested(|parser| {
            parser.pos += 1;
            let mut content = Vec::new();
            while !parser.ends(name) {
                parser.step()?;
                match parser.peek() {
                    Some(TokenKind::Open(tag, _)) if tag == "ol" => {
                        parser.note_list(tag)?;
                    }
                    Some(TokenKind::Open(tag, _)) if tag == "hr" => {
                        parser.pos += 1;
                        parser.skip_close("hr");
                    }
                    _ => parser.block(&mut content)?,
                }
            }
            Ok(content)
        })?;
        if !content.is_empty() {
            out.push(Block::Div(attr, content));
        }
        Ok(())
    }

    fn note_list(&mut self, tag: &str) -> Parse<()> {
        self.nested(|parser| {
            parser.pos += 1;
            while !parser.ends(tag) {
                parser.step()?;
                match parser.peek() {
                    Some(TokenKind::Open(item, attributes)) if item == "li" => {
                        let id = attribute(&clean_attributes(attributes), "id")
                            .unwrap_or_default()
                            .to_owned();
                        let first = parser.heading_ends.len();
                        let content = fix_plains(parser.blocks_in("li")?, true);
                        if !id.is_empty() {
                            let headings = first..parser.heading_ends.len();
                            parser.notes.push((id, content, headings));
                        }
                    }
                    Some(TokenKind::Text(text)) if is_blank(text) => parser.pos += 1,
                    Some(TokenKind::Close(_)) => return Err(Stop::Fail),
                    _ => parser.block(&mut Vec::new())?,
                }
            }
            Ok(())
        })
    }

    /// A `div` with the class `line-block`: lines that `br` elements separate.
    fn line_block(&mut self, out: &mut Vec<Block>) -> Parse<()> {
        let content = self.inlines_in("div")?;
        let mut lines: Vec<Vec<Inline>> = Vec::new();
        let mut line = Inlines::new();
        for inline in content.0 {
            if inline == Inline::LineBreak {
                lines.push(std::mem::take(&mut line).0);
            } else {
                line.0.push(inline);
            }
        }
        lines.push(line.0);
        let lines = lines
            .into_iter()
            .map(|line| {
                // A newline inside a line is not kept.
                let mut line = Inlines(line).trimmed().0;
                line.retain(|inline| *inline != Inline::SoftBreak);
                line
            })
            .collect();
        out.push(Block::LineBlock(lines));
        Ok(())
    }

    fn figure(&mut self, out: &mut Vec<Block>) -> Parse<()> {
        let Some(TokenKind::Open(_, attributes)) = self.peek() else {
            return Err(Stop::Fail);
        };
        let attr = to_attr(&clean_attributes(attributes));
        let (content, caption) = self.nested(|parser| {
            parser.pos += 1;
            let mut content = Vec::new();
            let mut caption = Vec::new();
            while !parser.ends("figure") {
                if matches!(parser.peek(), Some(TokenKind::Open(name, _)) if name == "figcaption") {
                    caption.append(&mut parser.blocks_in("figcaption")?);
                } else {
                    parser.block(&mut content)?;
                }
            }
            Ok((content, caption))
        })?;
        let caption = Caption {
            short: None,
            long: caption,
        };
        out.push(Block::Figure(attr, caption, content));
        Ok(())
    }

    fn pre(&mut self, out: &mut Vec<Block>) -> Parse<()> {
        let Some(TokenKind::Open(_, pre_attributes)) = self.peek() else {
            return Err(Stop::Fail);
        };
        let code = match self.toks.get(self.pos + 1).map(|token| &token.kind) {
            Some(TokenKind::Open(name, attributes)) if name == "code" => Some(attributes),
            _ => None,
        };
        let Some(code_attributes) = code else {
            // Without a `code` element first, the content is read as blocks.
            // White space stays: see `TextMode::Pre`.
            let was = std::mem::replace(&mut self.mode, TextMode::Pre);
            let content = self.blocks_in("pre");
            self.mode = was;
            out.append(&mut content?);
            return Ok(());
        };
        let mut joined = pre_attributes.clone();
        joined.extend(code_attributes.iter().cloned());
        let mut attr = to_attr(&clean_attributes(&joined));
        for class in &mut attr.classes {
            if let Some(language) = class.strip_prefix("language-") {
                *class = language.to_owned();
            }
        }
        let end = self.element_end("pre");
        let mut text = String::new();
        for token in self.toks.get(self.pos..end).unwrap_or_default() {
            if let TokenKind::Text(piece) = &token.kind {
                text.push_str(piece);
            }
        }
        if text.ends_with('\n') {
            text.pop();
        }
        self.pos = end;
        out.push(Block::CodeBlock(attr, text));
        Ok(())
    }

    fn list(&mut self, out: &mut Vec<Block>) -> Parse<()> {
        let Some(TokenKind::Open(name, attributes)) = self.peek() else {
            return Err(Stop::Fail);
        };
        let ordered = name == "ol";
        let clean = clean_attributes(attributes);
        let items = self.nested(|parser| {
            parser.pos += 1;
            let mut items: Vec<Vec<Block>> = Vec::new();
            while !parser.ends(name) {
                parser.step()?;
                match parser.peek() {
                    Some(TokenKind::Open(tag, item_attributes)) if tag == "li" => {
                        let item = parser.list_item(item_attributes)?;
                        items.push(item);
                    }
                    Some(TokenKind::Text(text)) if is_blank(text) => parser.pos += 1,
                    Some(TokenKind::Comment(_)) => parser.pos += 1,
                    Some(TokenKind::Close(_)) => return Err(Stop::Fail),
                    _ => {
                        let mut blocks = Vec::new();
                        parser.block(&mut blocks)?;
                        match items.last_mut() {
                            Some(last) => last.append(&mut blocks),
                            None if !blocks.is_empty() => items.push(blocks),
                            None => {}
                        }
                    }
                }
            }
            // A list needs its close tag.
            if parser.ended_at_eof {
                return Err(Stop::Fail);
            }
            Ok(items)
        })?;
        let items = items
            .into_iter()
            .map(|item| fix_plains(item, true))
            .collect();
        out.push(if ordered {
            Block::OrderedList(list_attributes(&clean), items)
        } else {
            Block::BulletList(items)
        });
        Ok(())
    }

    fn list_item(&mut self, attributes: &[(Text, Text)]) -> Parse<Vec<Block>> {
        let was = std::mem::replace(&mut self.in_list_item, true);
        let content = self.blocks_in("li");
        self.in_list_item = was;
        let mut content = content?;
        let id = attribute(&clean_attributes(attributes), "id")
            .unwrap_or_default()
            .to_owned();
        if id.is_empty() {
            return Ok(content);
        }
        let attr = Attr::new(id, Vec::new(), Vec::new());
        match content.first_mut() {
            Some(Block::Plain(inlines)) => {
                let inner = std::mem::take(inlines);
                inlines.push(Inline::Span(attr, inner));
                Ok(content)
            }
            _ => Ok(vec![Block::Div(attr, content)]),
        }
    }

    fn definition_list(&mut self, out: &mut Vec<Block>) -> Parse<()> {
        let mut visited: Vec<usize> = Vec::new();
        let items = self.nested(|parser| parser.definition_items(&mut visited));
        if items == Err(Stop::Fail) {
            // The rest of a definition list does not depend on its open tag.
            // A later list that reaches one of these parts fails in the same way.
            for index in visited {
                if let Some(flags) = self.failed.get_mut(index) {
                    *flags |= FAILED_DEFINITION_PART;
                }
            }
        }
        out.push(Block::DefinitionList(items?));
        Ok(())
    }

    /// Returns true when an earlier definition list failed after it read
    /// the `dt` or `dd` element at the current token.
    fn definition_part_failed(&mut self, visited: &mut Vec<usize>) -> bool {
        visited.push(self.pos);
        self.failed
            .get(self.pos)
            .is_some_and(|flags| flags & FAILED_DEFINITION_PART != 0)
    }

    fn definition_items(&mut self, visited: &mut Vec<usize>) -> Parse<Vec<DefinitionItem>> {
        self.pos += 1;
        let mut items: Vec<DefinitionItem> = Vec::new();
        loop {
            if self.ends_definition_list() {
                return Ok(items);
            }
            self.skip_to_definition_part();
            let mut term = Inlines::new();
            let mut terms = 0usize;
            while self.at_open("dt") {
                self.step()?;
                if self.definition_part_failed(visited) {
                    return Err(Stop::Fail);
                }
                let part = self.inlines_in("dt")?.trimmed();
                if terms > 0 {
                    term.0.push(Inline::LineBreak);
                }
                term.append(part);
                terms += 1;
                self.skip_to_definition_part();
            }
            if terms == 0 {
                return Err(Stop::Fail);
            }
            let mut definitions = Vec::new();
            while self.at_open("dd") {
                self.step()?;
                if self.definition_part_failed(visited) {
                    return Err(Stop::Fail);
                }
                let content = self.blocks_in("dd")?;
                definitions.push(fix_plains(content, true));
                self.skip_to_definition_part();
            }
            if definitions.is_empty() {
                return Err(Stop::Fail);
            }
            items.push((term.0, definitions));
        }
    }

    /// Returns true at the end of the input or at the close tag of a
    /// definition list, which it consumes.
    fn ends_definition_list(&mut self) -> bool {
        match self.peek() {
            Some(TokenKind::Close(name)) if name == "dl" => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    fn at_open(&self, tag: &str) -> bool {
        matches!(self.peek(), Some(TokenKind::Open(name, _)) if name == tag)
    }

    /// Moves to the next `dt` or `dd` open tag or to the `dl` close tag.
    fn skip_to_definition_part(&mut self) {
        if self.next_definition_part.is_empty() {
            self.next_definition_part = next_definition_parts(self.toks);
        }
        if let Some(&next) = self.next_definition_part.get(self.pos) {
            self.pos = next;
        }
    }

    // ---- The head element.

    fn set_lang(&mut self, attributes: &[(Text, Text)]) {
        if let Some(lang) = attribute(&clean_attributes(attributes), "lang") {
            let mut inlines = Inlines::new();
            text_inlines(lang, TextMode::Attribute, Extensions::empty(), &mut inlines);
            self.set_meta("lang", MetaValue::MetaInlines(inlines.0), true);
        }
    }

    fn set_meta(&mut self, key: &str, value: MetaValue, replace: bool) {
        let known = self
            .meta_index
            .get(key)
            .and_then(|index| self.meta.get_mut(*index));
        let Some(entry) = known else {
            self.meta_index.insert(key.to_owned(), self.meta.len());
            self.meta.push((key.to_owned(), value));
            return;
        };
        if replace {
            entry.1 = value;
            return;
        }
        match &mut entry.1 {
            MetaValue::MetaList(list) => list.push(value),
            old => {
                let first = std::mem::replace(old, MetaValue::MetaList(Vec::new()));
                *old = MetaValue::MetaList(vec![first, value]);
            }
        }
    }

    /// Reads the `head` element at the current token: the title, the `meta`
    /// elements with a name and a content, and the `base` element.
    fn head(&mut self) {
        self.pos += 1;
        while let Some(kind) = self.peek() {
            match kind {
                TokenKind::Close(name) if name == "head" => {
                    self.pos += 1;
                    return;
                }
                TokenKind::Open(name, _) if name == "body" => return,
                TokenKind::Close(name) if name == "td" || name == "th" => return,
                TokenKind::Open(name, attributes) => {
                    // A title needs its close tag inside the head element.
                    let closed_title = name == "title"
                        && match (self.next_close("title"), self.next_close("head")) {
                            (Some(title_end), Some(head_end)) => title_end < head_end,
                            (Some(_), None) => true,
                            (None, _) => false,
                        };
                    self.pos += 1;
                    let clean = clean_attributes(attributes);
                    match name.as_str() {
                        "title" if closed_title => {
                            // The title ends at the first open tag. A comment
                            // or a close tag before it is raw HTML.
                            let mut inlines = Inlines::new();
                            loop {
                                match self.peek() {
                                    Some(TokenKind::Text(text)) => {
                                        self.pos += 1;
                                        self.text(text, &mut inlines);
                                    }
                                    Some(TokenKind::Comment(_)) => self.raw_inline(&mut inlines),
                                    Some(TokenKind::Close(name)) if name != "title" => {
                                        self.raw_inline(&mut inlines);
                                    }
                                    _ => break,
                                }
                            }
                            let title = MetaValue::MetaInlines(inlines.trimmed().0);
                            self.set_meta("title", title, true);
                        }
                        "meta" => {
                            let name = attribute(&clean, "name").unwrap_or_default();
                            if !name.is_empty() {
                                let content = attribute(&clean, "content").unwrap_or_default();
                                let mut inlines = Inlines::new();
                                text_inlines(
                                    content,
                                    TextMode::Attribute,
                                    Extensions::empty(),
                                    &mut inlines,
                                );
                                let value = MetaValue::MetaInlines(inlines.0);
                                self.set_meta(name, value, false);
                            }
                        }
                        "base" => {
                            if let Some(href) = attribute(&clean, "href") {
                                self.base = Some(href.to_owned());
                            }
                        }
                        _ => {}
                    }
                }
                _ => self.pos += 1,
            }
        }
    }

    // ---- Inlines.

    /// Reads the inline content of an element whose open tag is the current token.
    fn inlines_in(&mut self, tag: &str) -> Parse<Inlines> {
        self.nested(|parser| {
            parser.pos += 1;
            let mut content = Inlines::new();
            while !parser.ends(tag) {
                parser.inline(&mut content)?;
            }
            Ok(content)
        })
    }

    fn inline(&mut self, out: &mut Inlines) -> Parse<()> {
        self.step()?;
        let Some(kind) = self.peek() else {
            return Err(Stop::Fail);
        };
        self.furthest_failure = self.furthest_failure.max(self.pos);
        match kind {
            TokenKind::Text(text) => {
                self.pos += 1;
                self.text(text, out);
                Ok(())
            }
            TokenKind::Comment(_) => {
                self.raw_inline(out);
                Ok(())
            }
            TokenKind::Close(name) => {
                if !self.raw_inline_allowed(name) {
                    return Err(Stop::Fail);
                }
                self.raw_inline(out);
                Ok(())
            }
            TokenKind::Open(name, attributes) => {
                let flag = if self.in_plain {
                    FAILED_IN_PLAIN
                } else {
                    FAILED_IN_PARA
                };
                let start = self.pos;
                if self
                    .failed
                    .get(start)
                    .is_some_and(|flags| flags & flag != 0)
                {
                    return Err(Stop::Fail);
                }
                let snapshot = self.snapshot();
                let result = self.inline_element(name, attributes, out);
                if result == Err(Stop::Fail) {
                    self.restore(snapshot);
                    if let Some(flags) = self.failed.get_mut(start) {
                        *flags |= flag;
                    }
                }
                result
            }
        }
    }

    fn text(&self, text: &str, out: &mut Inlines) {
        quoted_text_inlines(text, self.mode, self.extensions, self.quote_context, out);
    }

    fn raw_inline_allowed(&self, name: &str) -> bool {
        if self.in_plain {
            !is_block_name(name) && !is_either_name(name)
        } else {
            !is_block_name(name)
        }
    }

    /// Consumes one tag or comment as unknown markup inside inline content.
    fn raw_inline(&mut self, out: &mut Inlines) {
        if let Some(raw) = self.skip_raw(self.pos + 1) {
            out.push(Inline::RawInline(Format::new("html"), raw));
        }
    }

    fn is_math_script(&self) -> bool {
        match self.peek() {
            Some(TokenKind::Open(name, attributes)) if name == "script" => {
                attribute(attributes, "type").is_some_and(|kind| kind.starts_with("math/tex"))
            }
            _ => false,
        }
    }

    fn inline_element(
        &mut self,
        name: &'a str,
        attributes: &'a [(Text, Text)],
        out: &mut Inlines,
    ) -> Parse<()> {
        match name {
            "br" => {
                self.pos += 1;
                self.skip_close("br");
                out.push(Inline::LineBreak);
                Ok(())
            }
            "em" | "i" => self.wrapped(name, out, Inline::Emph),
            "strong" | "b" => self.wrapped(name, out, Inline::Strong),
            "u" | "ins" => self.wrapped(name, out, Inline::Underline),
            "s" | "strike" | "del" => self.wrapped(name, out, Inline::Strikeout),
            "sub" => self.wrapped(name, out, Inline::Subscript),
            "sup" => self.wrapped(name, out, Inline::Superscript),
            "small" => {
                let attr = Attr::new("", vec![name.to_owned()], Vec::new());
                self.wrapped(name, out, |content| Inline::Span(attr, content))
            }
            "mark" | "kbd" | "dfn" | "abbr" => {
                let mut attr = to_attr(&clean_attributes(attributes));
                attr.classes.insert(0, name.to_owned());
                let content = self.inlines_or_stop(name)?;
                out.push(Inline::Span(attr, content.0));
                Ok(())
            }
            "bdo" => {
                let direction = attribute(&clean_attributes(attributes), "dir").map(str::to_owned);
                let content = self.inlines_in(name)?;
                match direction {
                    Some(direction) => {
                        let pair = ("dir".to_owned(), direction);
                        let attr = Attr::new("", Vec::new(), vec![pair]);
                        out.push(Inline::Span(attr, content.0));
                    }
                    None => out.append(content),
                }
                Ok(())
            }
            "q" => self.quoted(attributes, out),
            "a" => self.link(attributes, out),
            "img" => self.image(attributes, out),
            "code" | "tt" | "samp" | "var" => self.code(name, attributes, out),
            "span" if self.has(Extension::NativeSpans) => self.span(attributes, out),
            "span" if !self.has(Extension::RawHtml) => {
                let content = self.inlines_in("span")?;
                out.append(content);
                Ok(())
            }
            "svg" if self.has(Extension::RawHtml) => Err(Stop::Fail),
            "svg" => self.svg(attributes, out),
            "script" => self.script_math(attributes, out),
            "input"
                if attribute(attributes, "type")
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("checkbox")) =>
            {
                self.checkbox(attributes, out)
            }
            "style" if self.next_close("style").is_none() => Err(Stop::Fail),
            "math" if self.next_close("math").is_none() => Err(Stop::Fail),
            "style" | "math" => {
                let end = if name == "math" {
                    self.element_end("math")
                } else {
                    self.raw_element_end("style")
                };
                if let Some(raw) = self.skip_raw(end) {
                    out.push(Inline::RawInline(Format::new("html"), raw));
                }
                Ok(())
            }
            _ if self.raw_inline_allowed(name) => {
                self.raw_inline(out);
                Ok(())
            }
            _ => Err(Stop::Fail),
        }
    }

    /// Reads the inline content of an element that Pandoc does not read a
    /// second way: when the content does not fit, the conversion stops.
    fn inlines_or_stop(&mut self, tag: &str) -> Parse<Inlines> {
        self.furthest_failure = self.pos;
        match self.inlines_in(tag) {
            Err(Stop::Fail) => Err(Stop::Fatal(self.furthest_failure)),
            result => result,
        }
    }

    fn wrapped(
        &mut self,
        tag: &str,
        out: &mut Inlines,
        wrap: impl FnOnce(Vec<Inline>) -> Inline,
    ) -> Parse<()> {
        let content = self.inlines_in(tag)?;
        out.append(content.wrap_extracting_spaces(wrap));
        Ok(())
    }

    fn quoted(&mut self, attributes: &[(Text, Text)], out: &mut Inlines) -> Parse<()> {
        let kind = if self.quote_context == Some(QuoteType::DoubleQuote) {
            QuoteType::SingleQuote
        } else {
            QuoteType::DoubleQuote
        };
        let was = self.quote_context.replace(kind);
        let content = self.inlines_or_stop("q");
        self.quote_context = was;
        let content = content?;
        if self.ended_at_eof {
            return Err(Stop::Fatal(self.toks.len()));
        }
        let clean = clean_attributes(attributes);
        out.append(content.wrap_extracting_spaces(|content| {
            let content = match attribute(&clean, "cite") {
                Some(_) => vec![Inline::Span(to_attr(&clean), content)],
                None => content,
            };
            Inline::Quoted(kind, content)
        }));
        Ok(())
    }

    fn url(&self, url: &str) -> Text {
        let resolved = self
            .base
            .as_deref()
            .and_then(|base| resolve_reference(base, url));
        escape_uri(resolved.as_deref().unwrap_or(url))
    }

    fn link(&mut self, attributes: &[(Text, Text)], out: &mut Inlines) -> Parse<()> {
        let clean = clean_attributes(attributes);
        let start = self.pos;
        let is_note_reference = attribute(&clean, "role") == Some("doc-noteref")
            || (self.has(Extension::EpubHtmlExts)
                && (attribute(&clean, "epub:type") == Some("noteref")
                    || attribute(&clean, "type") == Some("noteref")));
        let note_id = attribute(&clean, "href").and_then(|href| href.strip_prefix('#'));
        if let (true, Some(id)) = (is_note_reference, note_id) {
            self.pos = self.element_end("a");
            let reference = format!("{id}\u{0}{start}");
            out.push(Inline::RawInline(Format::new(NOTE_REF_FORMAT), reference));
            return Ok(());
        }
        if attribute(&clean, "role") == Some("doc-backlink") {
            self.pos = self.element_end("a");
            return Ok(());
        }
        let content = self.inlines_in("a")?;
        if self.ended_at_eof {
            return Err(Stop::Fail);
        }
        let mut attr = to_attr(&clean);
        let Some(href) = attribute(&clean, "href") else {
            if attr.id.is_empty() {
                attr.id = attribute(&clean, "name").unwrap_or_default().to_owned();
            }
            attr.attributes
                .retain(|(key, _)| key != "name" && key != "title");
            out.append(content.wrap_extracting_spaces(|content| Inline::Span(attr, content)));
            return Ok(());
        };
        let target = Target::new(
            self.url(href),
            attribute(&clean, "title").unwrap_or_default(),
        );
        attr.attributes
            .retain(|(key, _)| key != "href" && key != "title");
        out.append(content.wrap_extracting_spaces(|content| Inline::Link(attr, content, target)));
        Ok(())
    }

    fn image(&mut self, attributes: &[(Text, Text)], out: &mut Inlines) -> Parse<()> {
        let clean = clean_attributes(attributes);
        let Some(source) = attribute(&clean, "src") else {
            return Err(Stop::Fail);
        };
        self.pos += 1;
        self.skip_close("img");
        let mut alt = Inlines::new();
        let alt_text = attribute(&clean, "alt").unwrap_or_default();
        text_inlines(alt_text, TextMode::Attribute, Extensions::empty(), &mut alt);
        let target = Target::new(
            self.url(source),
            attribute(&clean, "title").unwrap_or_default(),
        );
        let mut attr = to_attr(&clean);
        attr.attributes
            .retain(|(key, _)| !matches!(key.as_str(), "src" | "alt" | "title"));
        out.push(Inline::Image(attr, alt.0, target));
        Ok(())
    }

    fn code(&mut self, name: &str, attributes: &[(Text, Text)], out: &mut Inlines) -> Parse<()> {
        let mut attr = to_attr(&clean_attributes(attributes));
        match name {
            "samp" => attr.classes.insert(0, "sample".to_owned()),
            "var" => attr.classes.insert(0, "variable".to_owned()),
            _ => {}
        }
        self.code_depth += 1;
        let content = self.inlines_in(name);
        self.code_depth -= 1;
        let content = content?;
        if self.ended_at_eof {
            return Err(Stop::Fail);
        }
        if content.is_empty() {
            return Ok(());
        }
        if self.code_depth > 0 {
            // The outermost code element converts this content in the same
            // walk as its own. A conversion here would make that walk visit
            // this content again, and nested code elements quadratic.
            out.push(Inline::Cite(
                Vec::new(),
                vec![Inline::Span(attr, content.0)],
            ));
        } else {
            out.append(Inlines(codify(&attr, content.0)));
        }
        Ok(())
    }

    fn span(&mut self, attributes: &[(Text, Text)], out: &mut Inlines) -> Parse<()> {
        let clean = clean_attributes(attributes);
        let attr = to_attr(&clean);
        let content = self.inlines_in("span")?;
        let small_caps = attr.classes.iter().any(|class| class == "smallcaps")
            || attribute(&clean, "style")
                .and_then(|style| style_property(style, "font-variant"))
                .is_some_and(|value| value.eq_ignore_ascii_case("small-caps"));
        out.push(if small_caps {
            Inline::SmallCaps(content.0)
        } else {
            Inline::Span(attr, content.0)
        });
        Ok(())
    }

    /// An inline `svg` element is an image with the element as a data URI,
    /// rendered as `tagsoup` does: a tag that closes itself in the source
    /// (`<rect/>`) is minimized, and text is escaped, but not in `style` and
    /// `script`. The image has the identifier and the classes of the element.
    fn svg(&mut self, attributes: &[(Text, Text)], out: &mut Inlines) -> Parse<()> {
        let full = to_attr(&clean_attributes(attributes));
        let attr = Attr::new(full.id, full.classes, Vec::new());
        let end = self.element_end("svg");
        let mut raw = String::new();
        let mut index = self.pos;
        while let Some(token) = self.toks.get(index).filter(|_| index < end) {
            let next = self.toks.get(index + 1).filter(|_| index + 1 < end);
            let previous = index.checked_sub(1).and_then(|at| self.toks.get(at));
            match (&token.kind, next) {
                (TokenKind::Open(name, attributes), Some(close))
                    if close.kind == TokenKind::Close(name.clone())
                        && (close.line, close.column) == (token.line, token.column) =>
                {
                    render_open(name, attributes, &mut raw);
                    raw.pop();
                    raw.push_str(" />");
                    index += 2;
                    continue;
                }
                (TokenKind::Text(text), _)
                    if !previous.is_some_and(|token| {
                        matches!(&token.kind, TokenKind::Open(name, _) if name == "style" || name == "script")
                    }) =>
                {
                    escape_into(text, &mut raw);
                }
                _ => self.render(index, &mut raw),
            }
            index += 1;
        }
        let closed = matches!(
            self.toks.get(end.wrapping_sub(1)).map(|token| &token.kind),
            Some(TokenKind::Close(name)) if name == "svg"
        );
        if !closed {
            raw.push_str("</svg>");
        }
        self.pos = end;
        let mut url = String::from("data:image/svg+xml;base64,");
        base64_into(raw.as_bytes(), &mut url);
        out.push(Inline::Image(attr, Vec::new(), Target::new(url, "")));
        Ok(())
    }

    /// A `script` element with the type `math/tex` is TeX math.
    fn script_math(&mut self, attributes: &[(Text, Text)], out: &mut Inlines) -> Parse<()> {
        if !self.is_math_script() {
            return Err(Stop::Fail);
        }
        let display =
            attribute(attributes, "type").is_some_and(|kind| kind.contains("mode=display"));
        let end = self.raw_element_end("script");
        let text = match self.toks.get(self.pos + 1).map(|token| &token.kind) {
            Some(TokenKind::Text(text)) if end > self.pos + 1 => text.trim().to_owned(),
            _ => String::new(),
        };
        self.pos = end;
        let kind = if display {
            MathType::DisplayMath
        } else {
            MathType::InlineMath
        };
        out.push(Inline::Math(kind, text));
        Ok(())
    }

    /// A check box inside a list item is a ballot box character and a space.
    fn checkbox(&mut self, attributes: &[(Text, Text)], out: &mut Inlines) -> Parse<()> {
        if !self.in_list_item {
            return Err(Stop::Fail);
        }
        self.pos += 1;
        self.skip_close("input");
        let checked = attribute(attributes, "checked").is_some();
        let mark = if checked { "\u{2612}" } else { "\u{2610}" };
        out.push(Inline::Str(mark.to_owned()));
        out.push(Inline::Space);
        Ok(())
    }

    fn table(&mut self, out: &mut Vec<Block>) -> Parse<()> {
        crate::table::table(self, out)
    }
}

/// The parts of the parser that the table rules use.
impl<'a> Parser<'a> {
    pub(crate) fn peek_kind(&self) -> Option<&'a TokenKind> {
        self.peek()
    }

    pub(crate) fn advance(&mut self) {
        self.pos += 1;
    }

    pub(crate) fn count_step(&mut self) -> Parse<()> {
        self.step()
    }

    pub(crate) fn within<T>(&mut self, rule: impl FnOnce(&mut Self) -> Parse<T>) -> Parse<T> {
        self.nested(rule)
    }

    pub(crate) fn element_ends(&mut self, tag: &str) -> bool {
        self.ends(tag)
    }

    pub(crate) fn container(&mut self, tag: &str) -> Parse<Vec<Block>> {
        self.blocks_in(tag)
    }
}

/// Makes each `Plain` a `Para` when the list has a block that needs a
/// paragraph next to it, as Pandoc does for the document, a block quote, a
/// list item and a definition.
pub(crate) fn fix_plains(mut blocks: Vec<Block>, in_list: bool) -> Vec<Block> {
    let needs_paragraphs = blocks.iter().any(|block| match block {
        Block::Para(_) | Block::CodeBlock(..) | Block::Header(..) | Block::BlockQuote(_) => true,
        Block::BulletList(_) | Block::OrderedList(..) | Block::DefinitionList(_) => !in_list,
        _ => false,
    });
    if needs_paragraphs {
        for block in &mut blocks {
            if let Block::Plain(inlines) = block {
                *block = Block::Para(std::mem::take(inlines));
            }
        }
    }
    blocks
}

#[cfg(test)]
thread_local! {
    /// The inlines that `codify` visited on this thread.
    pub(crate) static CODIFY_VISITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Makes the content of a `code` element code: each run of words and spaces
/// is one `Code`, and an element with formatting keeps its place around it.
/// A `Cite` without citations holds a nested code element (see `code`); the
/// reader makes no other `Cite`.
fn codify(attr: &Attr, inlines: Vec<Inline>) -> Vec<Inline> {
    #[cfg(test)]
    CODIFY_VISITS.with(|visits| visits.set(visits.get() + inlines.len() as u64));
    let mut out = Inlines::new();
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut Inlines| {
        if !run.is_empty() {
            out.0.push(Inline::Code(attr.clone(), std::mem::take(run)));
        }
    };
    for inline in inlines {
        let inner = match inline {
            Inline::Str(text) => {
                run.push_str(&text);
                continue;
            }
            Inline::Space | Inline::SoftBreak => {
                run.push(' ');
                continue;
            }
            Inline::Quoted(..) => {
                run.push_str(&stringify(std::slice::from_ref(&inline)));
                continue;
            }
            Inline::Emph(content) => Inline::Emph(codify(attr, content)),
            Inline::Strong(content) => Inline::Strong(codify(attr, content)),
            Inline::Underline(content) => Inline::Underline(codify(attr, content)),
            Inline::Strikeout(content) => Inline::Strikeout(codify(attr, content)),
            Inline::Superscript(content) => Inline::Superscript(codify(attr, content)),
            Inline::Subscript(content) => Inline::Subscript(codify(attr, content)),
            Inline::SmallCaps(content) => Inline::SmallCaps(codify(attr, content)),
            Inline::Span(span, content) => Inline::Span(span, codify(attr, content)),
            Inline::Link(link, content, target) => {
                Inline::Link(link, codify(attr, content), target)
            }
            Inline::Cite(citations, mut nested) if citations.is_empty() => {
                if let Some(Inline::Span(inner, content)) = nested.pop() {
                    flush(&mut run, &mut out);
                    out.append(Inlines(codify(&inner, content)));
                }
                continue;
            }
            other => other,
        };
        flush(&mut run, &mut out);
        out.push(inner);
    }
    flush(&mut run, &mut out);
    out.0
}

/// The attributes of an ordered list: the start number and the number style.
fn list_attributes(clean: &[(Text, Text)]) -> ListAttributes {
    let start = attribute(clean, "start")
        .and_then(read_integer)
        .unwrap_or(1);
    let from_type = attribute(clean, "type").and_then(|kind| match kind {
        "1" => Some(ListNumberStyle::Decimal),
        "a" => Some(ListNumberStyle::LowerAlpha),
        "A" => Some(ListNumberStyle::UpperAlpha),
        "i" => Some(ListNumberStyle::LowerRoman),
        "I" => Some(ListNumberStyle::UpperRoman),
        _ => None,
    });
    let from_class = || {
        attribute(clean, "class")
            .unwrap_or_default()
            .split_whitespace()
            .find_map(style_from_name)
    };
    let from_style = || {
        attribute(clean, "style")
            .and_then(|style| style_property(style, "list-style-type"))
            .and_then(|name| style_from_name(&name))
    };
    ListAttributes {
        start,
        style: from_type
            .or_else(from_class)
            .or_else(from_style)
            .unwrap_or(ListNumberStyle::DefaultStyle),
        delim: ListNumberDelim::DefaultDelim,
    }
}

fn style_from_name(name: &str) -> Option<ListNumberStyle> {
    match name {
        "decimal" => Some(ListNumberStyle::Decimal),
        "lower-alpha" => Some(ListNumberStyle::LowerAlpha),
        "upper-alpha" => Some(ListNumberStyle::UpperAlpha),
        "lower-roman" => Some(ListNumberStyle::LowerRoman),
        "upper-roman" => Some(ListNumberStyle::UpperRoman),
        _ => None,
    }
}

/// Writes the standard base64 form of `bytes`.
fn base64_into(bytes: &[u8], out: &mut String) {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let symbol =
        |index: u32| char::from(ALPHABET.get(index as usize & 63).copied().unwrap_or(b'='));
    for chunk in bytes.chunks(3) {
        let byte = |index: usize| u32::from(chunk.get(index).copied().unwrap_or(0));
        let group = (byte(0) << 16) | (byte(1) << 8) | byte(2);
        out.push(symbol(group >> 18));
        out.push(symbol(group >> 12));
        out.push(if chunk.len() > 1 {
            symbol(group >> 6)
        } else {
            '='
        });
        out.push(if chunk.len() > 2 { symbol(group) } else { '=' });
    }
}

/// The parts of a URI reference (RFC 3986, section 3).
struct UriParts<'a> {
    scheme: Option<&'a str>,
    authority: Option<&'a str>,
    path: &'a str,
    query: Option<&'a str>,
    fragment: Option<&'a str>,
}

/// Splits a URI reference into its parts. `None` when it has a character
/// that a URI cannot have, or a colon in its first segment that does not
/// end a scheme.
fn uri_parts(text: &str) -> Option<UriParts<'_>> {
    let valid = |c: char| c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c);
    if !text.chars().all(valid) {
        return None;
    }
    let (rest, fragment) = match text.split_once('#') {
        Some((rest, fragment)) => (rest, Some(fragment)),
        None => (text, None),
    };
    let (rest, query) = match rest.split_once('?') {
        Some((rest, query)) => (rest, Some(query)),
        None => (rest, None),
    };
    let first_segment = rest.split('/').next().unwrap_or_default();
    let (scheme, rest) = match first_segment.split_once(':') {
        Some((scheme, _)) => {
            let is_scheme = scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
            if !is_scheme {
                return None;
            }
            (Some(scheme), &rest[scheme.len() + 1..])
        }
        None => (None, rest),
    };
    let (authority, path) = match rest.strip_prefix("//") {
        Some(rest) => {
            let end = rest.find('/').unwrap_or(rest.len());
            (Some(&rest[..end]), &rest[end..])
        }
        None => (None, rest),
    };
    Some(UriParts {
        scheme,
        authority,
        path,
        query,
        fragment,
    })
}

/// Resolves a reference against a base URI (RFC 3986, section 5.2). The
/// base can be relative. Returns `None` when the base or the reference is
/// not a URI reference.
fn resolve_reference(base: &str, reference: &str) -> Option<Text> {
    let base = uri_parts(base)?;
    let mut reference = uri_parts(reference)?;
    // A reference with the scheme of the base is relative to it.
    if reference.scheme.is_some() && reference.scheme == base.scheme {
        reference.scheme = None;
    }
    let (scheme, authority, path, query) = if reference.scheme.is_some() {
        (
            reference.scheme,
            reference.authority,
            remove_dot_segments(reference.path),
            reference.query,
        )
    } else if reference.authority.is_some() {
        (
            base.scheme,
            reference.authority,
            remove_dot_segments(reference.path),
            reference.query,
        )
    } else if reference.path.is_empty() {
        (
            base.scheme,
            base.authority,
            remove_dot_segments(base.path),
            reference.query.or(base.query),
        )
    } else if reference.path.starts_with('/') {
        (
            base.scheme,
            base.authority,
            remove_dot_segments(reference.path),
            reference.query,
        )
    } else {
        let merged = if base.authority.is_some() && base.path.is_empty() {
            format!("/{}", reference.path)
        } else {
            let directory = base.path.rfind('/').map_or("", |end| &base.path[..=end]);
            format!("{directory}{}", reference.path)
        };
        (
            base.scheme,
            base.authority,
            remove_dot_segments(&merged),
            reference.query,
        )
    };
    let mut out = String::new();
    if let Some(scheme) = scheme {
        out.push_str(scheme);
        out.push(':');
    }
    if let Some(authority) = authority {
        out.push_str("//");
        out.push_str(authority);
    }
    out.push_str(&path);
    if let Some(query) = query {
        out.push('?');
        out.push_str(query);
    }
    if let Some(fragment) = reference.fragment {
        out.push('#');
        out.push_str(fragment);
    }
    Some(out)
}

fn remove_dot_segments(path: &str) -> String {
    let mut output: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').collect();
    let last = segments.len().saturating_sub(1);
    for (index, segment) in segments.iter().enumerate() {
        match *segment {
            "." => {
                if index == last {
                    output.push("");
                }
            }
            ".." => {
                if output.last().is_some_and(|last| !last.is_empty()) || output.len() > 1 {
                    output.pop();
                }
                if index == last {
                    output.push("");
                }
            }
            _ => output.push(segment),
        }
    }
    output.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tag_lists_are_sorted_for_the_binary_search() {
        for list in [HTML_BLOCK_TAGS, OTHER_BLOCK_TAGS, EITHER_TAGS, CLOSES_P] {
            assert!(list.windows(2).all(|pair| pair[0] < pair[1]), "{list:?}");
        }
    }

    // Pandoc 3.13: `<b>a<script>s</script>b</b>` ends the `b` element at the
    // `script` tag, and `<b>a<video>` does not end it.
    #[test]
    fn a_block_tag_ends_an_inline_element() {
        assert!(closes("script", "b"));
        assert!(closes("div", "span"));
        assert!(!closes("video", "b"));
        assert!(!closes("div", "del"));
        assert!(closes("p", "p") && closes("ul", "p") && !closes("script", "p"));
        assert!(closes("li", "li") && closes("dd", "dt") && closes("tr", "td"));
        assert!(!closes("div", "body") && !closes("p", "div"));
    }

    #[test]
    fn resolves_a_reference_against_a_base() {
        let base = "http://e.com/a/b/c.html";
        let resolve = |reference: &str| resolve_reference(base, reference);
        assert_eq!(resolve("../x").as_deref(), Some("http://e.com/a/x"));
        assert_eq!(resolve("/y").as_deref(), Some("http://e.com/y"));
        assert_eq!(resolve("#f").as_deref(), Some("http://e.com/a/b/c.html#f"));
        assert_eq!(resolve("?q").as_deref(), Some("http://e.com/a/b/c.html?q"));
        assert_eq!(resolve("").as_deref(), Some("http://e.com/a/b/c.html"));
        assert_eq!(resolve("//p.com/w").as_deref(), Some("http://p.com/w"));
        assert_eq!(resolve("mailto:a@b").as_deref(), Some("mailto:a@b"));
        assert_eq!(resolve("./d/../e").as_deref(), Some("http://e.com/a/b/e"));
        assert_eq!(resolve("z z.html"), None);
        // Pandoc 3.13: an empty segment before `..` is the segment that goes.
        assert_eq!(resolve("/a//../b").as_deref(), Some("http://e.com/a/b"));
    }

    #[test]
    fn writes_base64() {
        let mut out = String::new();
        base64_into(b"<svg></svg>", &mut out);
        assert_eq!(out, "PHN2Zz48L3N2Zz4=");
        out.clear();
        base64_into(b"ab", &mut out);
        assert_eq!(out, "YWI=");
    }
}
