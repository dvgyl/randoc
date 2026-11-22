//! Inline elements.

use randoc_ast::{Attr, Citation, CitationMode, Inline, MathType, QuoteType, Target, Text};
use randoc_core::{Extension, LogMessage, Result, WrapOption};
use randoc_layout::{Doc, cr, empty, literal, space};

use crate::escape::is_interrupting_marker;
use crate::html::{escape_html, html_attr_parts};
use randoc_core::shared::stringify;

use crate::{LinkRef, Variant, W};

enum SpanStyle {
    Underline,
    SmallCaps,
}

enum SpanForm {
    Span(&'static str),
    Delimited(&'static str, &'static str),
    Upper,
}

/// Returns `true` when the inlines after a shortcut reference `[text]` would
/// change its meaning: a link, a citation or a bracket text next to it, or a
/// parenthesis or a colon right after it.
fn continues_reference(rest: &[Inline]) -> bool {
    fn opens_bracket(inline: &Inline) -> bool {
        match inline {
            Inline::Link(..) | Inline::Cite(..) => true,
            Inline::Str(text) | Inline::RawInline(_, text) => text.starts_with('['),
            _ => false,
        }
    }
    match rest {
        [
            Inline::Space | Inline::SoftBreak | Inline::LineBreak,
            next,
            ..,
        ] => opens_bracket(next),
        [Inline::Str(text) | Inline::RawInline(_, text), ..] => text.starts_with(['[', '(', ':']),
        [next, ..] => opens_bracket(next),
        [] => false,
    }
}

/// Returns the parts of the brace form of attributes: `#id`, `.class`, `key="value"`.
fn attr_parts(attr: &Attr) -> Vec<Text> {
    let mut parts = Vec::new();
    if !attr.id.is_empty() {
        parts.push(format!("#{}", attr.id));
    }
    for class in &attr.classes {
        parts.push(format!(".{class}"));
    }
    for (key, value) in &attr.attributes {
        parts.push(format!(
            "{key}=\"{}\"",
            value.replace('\\', "\\\\").replace('"', "\\\"")
        ));
    }
    parts
}

/// Returns the attributes in the brace form `{#id .class key="value"}`.
pub(crate) fn attr_text(attr: &Attr) -> Text {
    format!("{{{}}}", attr_parts(attr).join(" "))
}

/// The brace form of attributes, with a line break permitted between parts.
fn attr_doc(attr: &Attr) -> Doc {
    let mut out = literal("{");
    for (index, part) in attr_parts(attr).into_iter().enumerate() {
        if index > 0 {
            out += space();
        }
        out += literal(part);
    }
    out + literal("}")
}

/// Returns the length of the longest run of `c` in `text`.
pub(crate) fn longest_run(text: &str, c: char) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for ch in text.chars() {
        if ch == c {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

fn superscript_char(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '\u{2212}' => '⁻',
        '-' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'i' => 'ⁱ',
        'n' => 'ⁿ',
        _ => return None,
    })
}

fn subscript_char(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' => '₋',
        '\u{2212}' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        _ => return None,
    })
}

fn is_space(inline: &Inline) -> bool {
    matches!(inline, Inline::Space | Inline::SoftBreak)
}

/// The first (`first`) or the last element of a list.
fn edge(list: &[Inline], first: bool) -> Option<&Inline> {
    if first { list.first() } else { list.last() }
}

/// Returns `true` when the element ends a line or a word for the purpose of
/// line-start escapes.
fn is_separator(inline: Option<&Inline>) -> bool {
    matches!(
        inline,
        None | Some(Inline::Space | Inline::SoftBreak | Inline::LineBreak)
    )
}

impl W<'_> {
    /// Renders a list of inlines that starts at the start of a line.
    pub(crate) fn inlines(&mut self, inlines: &[Inline]) -> Result<Doc> {
        self.inline_list(inlines, true)
    }

    /// Renders a list of inlines. `line_start` is `true` when the first
    /// element is at the start of a line.
    pub(crate) fn inline_list(&mut self, inlines: &[Inline], line_start: bool) -> Result<Doc> {
        self.enter()?;
        let (cut_first, cut_last) = std::mem::take(&mut self.cut_list);
        let mut out = empty();
        let mut at_start = line_start;
        for (index, inline) in inlines.iter().enumerate() {
            let next = inlines.get(index + 1);
            self.cut_edges = (
                cut_first && index == 0,
                cut_last && index + 1 == inlines.len(),
            );
            let doc = match inline {
                Inline::Str(text) => {
                    let upper;
                    let text = if self.upper {
                        upper = text.to_uppercase();
                        &upper
                    } else {
                        text
                    };
                    let escaped = self.escape_text(text);
                    let escaped = if at_start {
                        self.escape_line_start(text, escaped, is_separator(next))
                    } else {
                        escaped
                    };
                    literal(escaped)
                }
                Inline::Space | Inline::SoftBreak if self.in_script => literal("\\ "),
                Inline::Space => self.space_before(next, inlines.get(index + 2)),
                Inline::SoftBreak => {
                    if self.opts.wrap_text == WrapOption::WrapPreserve && !self.in_heading {
                        cr()
                    } else {
                        self.space_before(next, inlines.get(index + 2))
                    }
                }
                Inline::LineBreak => self.line_break(),
                other => self.inline(other, &inlines[index + 1..])?,
            };
            // Pandoc separates math from a following digit or empty text
            // with an HTML comment in each form of the math, also when it
            // converts the math.
            let digit_after = matches!(inline, Inline::Math(..))
                && matches!(next, Some(Inline::Str(s))
                    if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()));
            let doc = if !digit_after {
                doc
            } else if self.has(Extension::RawAttribute) {
                doc + literal("`<!-- -->`{=html}")
            } else if self.has(Extension::RawHtml) {
                doc + literal("<!-- -->")
            } else {
                doc
            };
            at_start = match inline {
                Inline::LineBreak => !self.in_heading,
                _ => false,
            };
            out += doc;
        }
        self.cut_edges = (false, false);
        self.leave();
        Ok(out)
    }

    /// A space that cannot break, before a word that would start a list
    /// that can interrupt a paragraph at the start of a line.
    fn space_before(&self, next: Option<&Inline>, after: Option<&Inline>) -> Doc {
        if self.in_heading {
            return literal(" ");
        }
        match (next, after) {
            (Some(Inline::Str(text)), Some(Inline::Space | Inline::SoftBreak))
                if is_interrupting_marker(&self.escape_text(text)) =>
            {
                literal(" ")
            }
            _ => space(),
        }
    }

    pub(crate) fn line_break(&self) -> Doc {
        if self.in_heading && self.variant == Variant::Commonmark && !self.plain() {
            return literal("\\") + cr();
        }
        if self.in_heading {
            return literal(if self.in_script { "\\ " } else { " " });
        }
        if self.plain() || self.has(Extension::HardLineBreaks) {
            return cr();
        }
        if self.variant == Variant::Commonmark || self.has(Extension::EscapedLineBreaks) {
            literal("\\") + cr()
        } else {
            literal("  ") + cr()
        }
    }

    fn inline(&mut self, inline: &Inline, rest: &[Inline]) -> Result<Doc> {
        let plain = self.plain();
        if self.in_toc {
            // An entry of the table of contents has no notes and no links.
            match inline {
                Inline::Note(_) => return Ok(empty()),
                Inline::Link(_, content, _) => return self.inline_list(content, false),
                _ => {}
            }
        }
        Ok(match inline {
            Inline::Str(_) | Inline::Space | Inline::SoftBreak | Inline::LineBreak => empty(),
            Inline::Emph(content) if matches!(content.as_slice(), [Inline::Emph(_)]) => {
                // Pandoc writes directly nested emphasis as no emphasis.
                match content.as_slice() {
                    [Inline::Emph(inner)] => self.inline_list(inner, false)?,
                    _ => empty(),
                }
            }
            Inline::Emph(content) => {
                let mark = if plain { "" } else { "*" };
                self.delimited(content, mark, mark)?
            }
            Inline::Strong(content) => {
                let mark = if plain { "" } else { "**" };
                self.delimited(content, mark, mark)?
            }
            Inline::Strikeout(content) => {
                if self.has(Extension::Strikeout) {
                    self.delimited(content, "~~", "~~")?
                } else if self.has(Extension::RawHtml) && !plain {
                    self.delimited(content, "<s>", "</s>")?
                } else {
                    self.delimited(content, "", "")?
                }
            }
            Inline::Superscript(content) => self.script(content, true)?,
            Inline::Subscript(content) => self.script(content, false)?,
            Inline::SmallCaps(content) => self.styled_span(SpanStyle::SmallCaps, content)?,
            Inline::Underline(content) if content.is_empty() => empty(),
            Inline::Underline(content) => self.styled_span(SpanStyle::Underline, content)?,
            Inline::Quoted(kind, content) => {
                let inner = self.inline_list(content, false)?;
                let ascii = self.has(Extension::Smart);
                let (open, close) = match (kind, ascii) {
                    (QuoteType::SingleQuote, true) => ("'", "'"),
                    (QuoteType::DoubleQuote, true) => ("\"", "\""),
                    (QuoteType::SingleQuote, false) => ("\u{2018}", "\u{2019}"),
                    (QuoteType::DoubleQuote, false) => ("\u{201C}", "\u{201D}"),
                };
                literal(open) + inner + literal(close)
            }
            Inline::Cite(citations, fallback) => self.cite(citations, fallback)?,
            Inline::Code(attr, text) => self.code(attr, text),
            Inline::Math(kind, text) => self.math(*kind, text)?,
            Inline::RawInline(format, text) => {
                let name = format.as_str().to_lowercase();
                self.raw_inline(&name, text)
            }
            Inline::Link(attr, content, target) => {
                let collapsed = !self.shortcut_references() || continues_reference(rest);
                self.link(attr, content, target, collapsed)?
            }
            Inline::Image(attr, content, target) => self.image(attr, content, target)?,
            Inline::Note(blocks) => {
                let slot = self.notes.len();
                self.notes.push(empty());
                self.note_starts.push(matches!(
                    blocks.first(),
                    Some(randoc_ast::Block::Para(_) | randoc_ast::Block::Plain(_))
                ));
                let number = self.notes_written + slot + 1;
                let doc = self.block_list(blocks)?;
                if let Some(entry) = self.notes.get_mut(slot) {
                    *entry = doc;
                }
                if self.has(Extension::Footnotes) && !plain {
                    literal(format!("[^{}{number}]", self.opts.identifier_prefix))
                } else {
                    literal(format!("[{}{number}]", self.opts.identifier_prefix))
                }
            }
            Inline::Span(attr, content) => self.span(attr, content)?,
        })
    }

    /// Renders content between delimiters. Spaces at the ends of the
    /// content, also inside nested emphasis, go outside the delimiters, and
    /// empty content gives nothing.
    fn delimited(&mut self, content: &[Inline], open: &str, close: &str) -> Result<Doc> {
        // The results that this element stores are for lists inside its
        // content, so they stay valid only while the content is written.
        let mark = self.edge_keys.len();
        let result = self.delimited_content(content, open, close);
        for key in self.edge_keys.drain(mark..) {
            self.edge_memo.remove(&key);
        }
        result
    }

    fn delimited_content(&mut self, content: &[Inline], open: &str, close: &str) -> Result<Doc> {
        let (cut_lead, cut_trail) = std::mem::take(&mut self.cut_edges);
        if content.is_empty() {
            return Ok(empty());
        }
        // HTML tags keep the spaces inside.
        if self.in_script || open.starts_with('<') {
            let inner = self.inline_list(content, false)?;
            return Ok(literal(open) + inner + literal(close));
        }
        let start = content
            .iter()
            .position(|i| !is_space(i))
            .unwrap_or(content.len());
        let end = content
            .iter()
            .rposition(|i| !is_space(i))
            .map_or(start, |e| e + 1);
        let inner_list = content.get(start..end).unwrap_or(&[]);
        let lead = self.edge_space(content, true) && !cut_lead;
        let trail = self.edge_space(content, false) && !cut_trail;
        if inner_list.is_empty() {
            // Content of spaces only gives the two delimiters.
            return Ok(if lead {
                space() + literal(format!("{open}{close}"))
            } else {
                literal(format!("{open}{close}"))
            });
        }
        self.cut_list = (
            self.edge_space(inner_list, true),
            self.edge_space(inner_list, false),
        );
        let inner = self.inline_list(inner_list, false)?;
        let mut out = if lead { space() } else { empty() };
        out += literal(open) + inner + literal(close);
        if trail {
            out += space();
        }
        Ok(out)
    }

    /// The content of an element whose edge spaces go outside its
    /// delimiters.
    fn hoisting_content<'i>(&self, inline: &'i Inline) -> Option<&'i [Inline]> {
        match inline {
            Inline::Emph(inner) | Inline::Strong(inner) => Some(inner),
            Inline::Strikeout(inner)
                if self.has(Extension::Strikeout)
                    || !self.has(Extension::RawHtml)
                    || self.plain() =>
            {
                Some(inner)
            }
            Inline::Underline(inner)
                if self.plain()
                    || self.has(Extension::BracketedSpans)
                    || !(self.has(Extension::NativeSpans) || self.has(Extension::RawHtml)) =>
            {
                Some(inner)
            }
            _ => None,
        }
    }

    /// Returns `true` when a space goes before (`first`) or after the
    /// delimited `content`. Content of spaces only gives a space before.
    fn edge_space(&mut self, content: &[Inline], first: bool) -> bool {
        match edge(content, first) {
            Some(i) if is_space(i) => true,
            Some(i) => match self.hoisting_content(i) {
                Some(inner) => self.edge_chain(inner, first),
                None => false,
            },
            None => false,
        }
    }

    /// The result of [`Self::edge_space`] for a list inside delimited
    /// content: a walk down the chain of elements whose edge spaces go
    /// outside. Each list of the chain has the same result, and the walk
    /// stores it for each one, so that the nested elements do not walk the
    /// chain again.
    fn edge_chain(&mut self, start: &[Inline], first: bool) -> bool {
        let mut walked = Vec::new();
        let mut list = start;
        let result = loop {
            let key = (list.as_ptr() as usize, list.len(), first);
            if let Some(&known) = self.edge_memo.get(&key) {
                break known;
            }
            // A chain deeper than the nesting limit fails later.
            if walked.len() > self.max_depth {
                break false;
            }
            walked.push(key);
            #[cfg(test)]
            {
                self.edge_steps += 1;
            }
            if !first && list.iter().all(is_space) {
                break false;
            }
            match edge(list, first) {
                Some(i) if is_space(i) => break true,
                Some(i) => match self.hoisting_content(i) {
                    Some(inner) => list = inner,
                    None => break false,
                },
                None => break false,
            }
        };
        for key in walked {
            if self.edge_memo.insert(key, result).is_none() {
                self.edge_keys.push(key);
            }
        }
        result
    }

    /// The plain walk that [`Self::edge_space`] replaces, for the tests.
    #[cfg(test)]
    fn edge_space_scan(&self, content: &[Inline], first: bool) -> bool {
        let mut list = content;
        // A chain deeper than the nesting limit fails later, so the walk
        // can stop there.
        for _ in 0..=self.max_depth.saturating_sub(self.depth) {
            if !first && list.iter().all(is_space) && !std::ptr::eq(list, content) {
                return false;
            }
            match if first { list.first() } else { list.last() } {
                Some(i) if is_space(i) => return true,
                Some(i) => match self.hoisting_content(i) {
                    Some(inner) => list = inner,
                    None => return false,
                },
                None => return false,
            }
        }
        false
    }

    /// Returns the attributes with the identifier prefix of the options
    /// before the identifier.
    pub(crate) fn prefixed<'x>(&self, attr: &'x Attr) -> std::borrow::Cow<'x, Attr> {
        if self.opts.identifier_prefix.is_empty() || attr.id.is_empty() {
            return std::borrow::Cow::Borrowed(attr);
        }
        let mut out = attr.clone();
        out.id = format!("{}{}", self.opts.identifier_prefix, attr.id);
        std::borrow::Cow::Owned(out)
    }

    /// Returns `true` when links and images take attributes in braces.
    pub(crate) fn link_attributes(&self) -> bool {
        self.has(Extension::LinkAttributes) || self.has(Extension::Attributes)
    }

    fn script(&mut self, content: &[Inline], superscript: bool) -> Result<Doc> {
        let extension = if superscript {
            Extension::Superscript
        } else {
            Extension::Subscript
        };
        if content.is_empty() {
            return Ok(empty());
        }
        let map = if superscript {
            superscript_char
        } else {
            subscript_char
        };
        if self.plain() {
            let text = self.inline_list(content, false)?.render(None);
            let only_text = content
                .iter()
                .all(|i| matches!(i, Inline::Str(_) | Inline::Space | Inline::SoftBreak));
            let converted = text
                .chars()
                .map(|c| if c == ' ' { Some(' ') } else { map(c) })
                .collect::<Option<Text>>()
                .filter(|converted| only_text || converted.trim().is_empty());
            return Ok(match converted {
                Some(converted) if converted.is_empty() => {
                    if content
                        .iter()
                        .all(|i| matches!(i, Inline::Space | Inline::SoftBreak))
                    {
                        space()
                    } else {
                        empty()
                    }
                }
                Some(converted) if converted.trim().is_empty() => space(),
                Some(converted) => literal(converted),
                None => {
                    let mark = if superscript { "^(" } else { "_(" };
                    literal(format!("{mark}{})", text.trim()))
                }
            });
        }
        if !self.has(extension) && !self.has(Extension::RawHtml) {
            if content.iter().all(|i| matches!(i, Inline::Str(_))) {
                let plain_text = stringify(content);
                if let Some(converted) = plain_text.chars().map(map).collect::<Option<Text>>() {
                    return Ok(literal(converted));
                }
            }
            let mark = if superscript { "^(" } else { "_(" };
            let inner = self.inline_list(content, false)?;
            return Ok(literal(mark) + inner + literal(")"));
        }
        let outer = self.in_script;
        self.in_script = self.variant == Variant::Markdown;
        let inner = self.inline_list(content, false);
        self.in_script = outer;
        let inner = inner?;
        Ok(if self.has(extension) {
            let mark = if superscript { "^" } else { "~" };
            literal(mark) + inner + literal(mark)
        } else {
            let tag = if superscript { "sup" } else { "sub" };
            literal(format!("<{tag}>")) + inner + literal(format!("</{tag}>"))
        })
    }

    /// Renders lines of inlines joined with hard line breaks.
    pub(crate) fn inline_lines(&mut self, parts: &[&[Inline]]) -> Result<Doc> {
        let mut out = empty();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                out += self.line_break();
            }
            out += self.inline_list(part, index > 0)?;
        }
        Ok(out)
    }

    pub(crate) fn code(&self, attr: &Attr, text: &str) -> Doc {
        if self.plain() {
            return literal(text);
        }
        let ticks = "`".repeat(longest_run(text, '`') + 1);
        let pad = if text.contains('`') { " " } else { "" };
        let out = literal(format!("{ticks}{pad}{text}{pad}{ticks}"));
        if !attr.is_empty()
            && (self.has(Extension::InlineCodeAttributes) || self.has(Extension::Attributes))
        {
            return out + attr_doc(&self.prefixed(attr));
        }
        out
    }

    fn math(&mut self, kind: MathType, text: &str) -> Result<Doc> {
        let display = kind == MathType::DisplayMath;
        let text = if display { text } else { text.trim() };
        let syntax = if self.plain() {
            None
        } else {
            [
                Extension::TexMathGfm,
                Extension::TexMathDollars,
                Extension::TexMathSingleBackslash,
                Extension::TexMathDoubleBackslash,
            ]
            .into_iter()
            .find(|extension| self.has(*extension))
        };
        Ok(match (syntax, display) {
            (None, _) => return self.math_fallback(kind, text),
            (Some(Extension::TexMathGfm), true) => {
                let text = text.trim_matches('\n');
                cr() + literal(format!("``` math\n{text}\n```")) + cr()
            }
            (Some(Extension::TexMathGfm), false) => literal(format!("$`{text}`$")),
            (Some(Extension::TexMathDollars), true) => literal(format!("$${text}$$")),
            (Some(Extension::TexMathDollars), false) => literal(format!("${text}$")),
            (Some(Extension::TexMathSingleBackslash), true) => literal(format!("\\[{text}\\]")),
            (Some(Extension::TexMathSingleBackslash), false) => literal(format!("\\({text}\\)")),
            (Some(_), true) => literal(format!("\\\\[{text}\\\\]")),
            (Some(_), false) => literal(format!("\\\\({text}\\\\)")),
        })
    }

    /// Writes math when the format has no math syntax: the conversion of
    /// `randoc-math` as inlines, or the TeX in dollars with a warning.
    fn math_fallback(&mut self, kind: MathType, tex: &str) -> Result<Doc> {
        let display = kind == MathType::DisplayMath;
        let remaining = self.max_depth.saturating_sub(self.depth);
        let inlines = match randoc_math::to_inlines(kind, tex, remaining) {
            Ok(inlines) => inlines,
            Err(error) => {
                self.messages.push(LogMessage::CouldNotConvertTexMath(
                    Text::from(tex),
                    error.detail(tex),
                ));
                let delimiter = if display { "$$" } else { "$" };
                vec![Inline::Str(format!("{delimiter}{tex}{delimiter}"))]
            }
        };
        let body = self.inline_list(&inlines, false)?;
        Ok(if display { cr() + body + cr() } else { body })
    }

    pub(crate) fn raw_inline(&self, format: &str, text: &str) -> Doc {
        if self.plain() {
            return if format == "plain" {
                literal(text)
            } else {
                empty()
            };
        }
        let own = if self.variant == Variant::Commonmark {
            matches!(format, "markdown" | "commonmark" | "gfm" | "commonmark_x")
        } else {
            format.starts_with("markdown")
        };
        if own {
            return literal(text);
        }
        if self.has(Extension::RawAttribute) {
            let ticks = "`".repeat(longest_run(text, '`') + 1);
            return literal(format!("{ticks}{text}{ticks}{{={format}}}"));
        }
        let html = matches!(format, "html" | "html4" | "html5");
        let tex = matches!(format, "latex" | "tex");
        if html && self.has(Extension::RawHtml) || tex && self.has(Extension::RawTex) {
            return literal(text);
        }
        empty()
    }

    fn cite(&mut self, citations: &[Citation], fallback: &[Inline]) -> Result<Doc> {
        if self.plain() || !self.has(Extension::Citations) || citations.is_empty() {
            return self.inline_list(fallback, false);
        }
        let first_in_text = citations
            .first()
            .is_some_and(|c| c.mode == CitationMode::AuthorInText);
        let mut parts = Vec::new();
        let mut in_text_suffix = empty();
        for (index, citation) in citations.iter().enumerate() {
            let in_text = index == 0 && first_in_text;
            let mut part = empty();
            if !citation.prefix.is_empty() && !in_text {
                part += self.inline_list(&citation.prefix, false)? + space();
            }
            let dash = if citation.mode == CitationMode::SuppressAuthor {
                "-"
            } else {
                ""
            };
            let plain_key = !citation.id.is_empty()
                && citation
                    .id
                    .chars()
                    .all(|c| c.is_alphanumeric() || "_-:.#$%&+?<>~/".contains(c));
            part += if plain_key {
                literal(format!("{dash}@{}", citation.id))
            } else {
                literal(format!("{dash}@{{{}}}", citation.id))
            };
            let suffix = citation.suffix.as_slice();
            if in_text {
                // The suffix of an author-in-text citation opens the bracket
                // group of the other citations.
                let start = suffix
                    .iter()
                    .position(|i| !matches!(i, Inline::Space | Inline::SoftBreak))
                    .unwrap_or(suffix.len());
                in_text_suffix = self.inline_list(suffix.get(start..).unwrap_or(&[]), false)?;
            } else if !suffix.is_empty() {
                let separated = match suffix.first() {
                    Some(Inline::Space | Inline::SoftBreak) => true,
                    Some(Inline::Str(s)) => s.starts_with([',', ';', ':', '.']),
                    _ => false,
                };
                if !separated {
                    part += space();
                }
                part += self.inline_list(suffix, false)?;
            }
            parts.push(part);
        }
        let join = |parts: Vec<Doc>| {
            let mut out = empty();
            for (index, part) in parts.into_iter().enumerate() {
                if index > 0 {
                    out += literal("; ");
                }
                out += part;
            }
            out
        };
        if !first_in_text {
            return Ok(literal("[") + join(parts) + literal("]"));
        }
        let mut rest = parts.into_iter();
        let key = rest.next().unwrap_or_else(empty);
        let rest = join(rest.collect());
        let group = match (in_text_suffix.is_empty(), rest.is_empty()) {
            (true, true) => return Ok(key),
            (false, false) => in_text_suffix + literal("; ") + rest,
            (false, true) => in_text_suffix,
            (true, false) => rest,
        };
        Ok(key + space() + literal("[") + group + literal("]"))
    }

    /// Writes a link. `collapsed` asks for the `[text][]` form of a
    /// reference whose label is its text.
    fn link(
        &mut self,
        attr: &Attr,
        content: &[Inline],
        target: &Target,
        collapsed: bool,
    ) -> Result<Doc> {
        if !self.plain() {
            let after_pipe = self.has(Extension::WikilinksTitleAfterPipe);
            if (after_pipe || self.has(Extension::WikilinksTitleBeforePipe))
                && attr.classes.iter().any(|c| c == "wikilink")
            {
                let text = stringify(content);
                let url = &target.url;
                if self.opts.reference_links {
                    // The definition takes the Markdown form of the text.
                    let label = self.inline_list(content, false)?.render(None);
                    let attrs = if self.link_attributes() {
                        attr_text(&self.prefixed(attr))
                    } else {
                        Text::new()
                    };
                    self.add_reference(&label, false, target, attr, &attrs);
                }
                return Ok(literal(if text == *url {
                    format!("[[{url}]]")
                } else if after_pipe {
                    format!("[[{url}|{text}]]")
                } else {
                    format!("[[{text}|{url}]]")
                }));
            }
            if let [Inline::Str(text)] = content {
                let is_uri_auto = *text == target.url && target.url.contains(':');
                let is_mail_auto = format!("mailto:{text}") == target.url;
                let auto_attr = attr.id.is_empty()
                    && attr.attributes.is_empty()
                    && attr.classes.iter().all(|c| c == "uri" || c == "email");
                if (is_uri_auto || is_mail_auto) && auto_attr {
                    return Ok(literal(format!("<{text}>")));
                }
            }
        }
        let images = self.images;
        let inner = self.inline_list(content, false)?;
        let has_image = self.images != images;
        Ok(self.link_with_inner(attr, inner, has_image, target, collapsed))
    }

    /// Writes a link with rendered content: in `plain` the content alone,
    /// else an HTML link, a reference or an inline link. `has_image` is
    /// `true` when the content holds an image, which gives a reference a
    /// numbered label.
    pub(crate) fn link_with_inner(
        &mut self,
        attr: &Attr,
        inner: Doc,
        has_image: bool,
        target: &Target,
        collapsed: bool,
    ) -> Doc {
        if self.plain() {
            if self.opts.reference_links {
                self.add_reference(&inner.render(None), has_image, target, attr, "");
            }
            return inner;
        }
        let original = attr;
        let shown = self.prefixed(attr);
        let attr = &*shown;
        // A reference link drops the attributes that the format cannot write.
        if !attr.is_empty()
            && !self.link_attributes()
            && self.has(Extension::RawHtml)
            && !self.opts.reference_links
        {
            let mut open = literal(format!("<a href=\"{}\"", escape_html(&target.url)));
            let mut parts = html_attr_parts(attr);
            if !target.title.is_empty() {
                parts.push(format!("title=\"{}\"", escape_html(&target.title)));
            }
            for part in parts {
                open += space() + literal(part);
            }
            // Pandoc wraps the tag on its own and puts the lines in the text.
            let open = literal(self.render_alone(open + literal(">")));
            return open + inner + literal("</a>");
        }
        let attrs = if !attr.is_empty() && self.link_attributes() {
            attr_text(attr)
        } else {
            Text::new()
        };
        if self.opts.reference_links {
            let label_text = inner.render(None);
            let label = self.add_reference(&label_text, has_image, target, original, &attrs);
            return if reference_key(&label) != reference_key(&label_text) {
                literal("[") + inner + literal(format!("][{label}]"))
            } else if collapsed {
                literal("[") + inner + literal("][]")
            } else {
                literal("[") + inner + literal("]")
            };
        }
        let attrs = if attrs.is_empty() {
            empty()
        } else {
            attr_doc(attr)
        };
        literal("[") + inner + literal("](") + destination(target) + literal(")") + attrs
    }

    /// Stores a link reference and returns its label. `attr` gives the
    /// attributes of the element, and `attrs` the attributes text that the
    /// definition shows. A target that waits for output already keeps its
    /// label. A text label that is new gets its target. A target that has
    /// a text label from before gets it again, and a target that has a
    /// numbered label `N` from an earlier block gets `textN`. Else the label
    /// is the next free number, also when `numbered` is `true` or `text` is
    /// not a valid label. Labels compare as [`reference_key`] gives them.
    fn add_reference(
        &mut self,
        text: &str,
        numbered: bool,
        target: &Target,
        attr: &Attr,
        attrs: &str,
    ) -> Text {
        let attr_key = if attr.is_empty() {
            Text::new()
        } else {
            attr_text(attr)
        };
        let key = (target.url.clone(), target.title.clone(), attr_key);
        if let Some(label) = self.pending_targets.get(&key) {
            return label.clone();
        }
        let normalized = reference_key(text);
        // A label with a bracket would not read back as a label.
        let text_label = !numbered && valid_label(text);
        let known = self
            .ref_index
            .get(&normalized)
            .filter(|_| text_label)
            .map(|targets| targets.get(&key).copied());
        let label = match known {
            Some(Some(0)) => text.to_owned(),
            Some(Some(number)) => format!("{text}{number}"),
            Some(None) => {
                let number = self.next_free_number();
                self.ref_index
                    .entry(normalized)
                    .or_default()
                    .insert(key.clone(), number);
                number.to_string()
            }
            None if text_label => {
                self.ref_index
                    .entry(normalized)
                    .or_default()
                    .insert(key.clone(), 0);
                text.to_owned()
            }
            // Numbered labels are not in the index: Pandoc can give a later
            // text the same label.
            None => self.next_free_number().to_string(),
        };
        self.refs.push(LinkRef {
            label: label.clone(),
            target: target.clone(),
            attrs: attrs.to_owned(),
        });
        self.pending_targets.insert(key, label.clone());
        label
    }

    /// The next number that no text label has, for a numbered label.
    fn next_free_number(&mut self) -> u64 {
        while self
            .ref_index
            .contains_key(&self.next_ref_number.to_string())
        {
            self.next_ref_number += 1;
        }
        let number = self.next_ref_number;
        self.next_ref_number += 1;
        number
    }

    /// Renders an image in Markdown syntax with rendered alternative text.
    pub(crate) fn image_doc(&mut self, attr: &Attr, inner: Doc, target: &Target) -> Doc {
        let original = attr;
        let shown = self.prefixed(attr);
        let attr = &*shown;
        let attrs = if !attr.is_empty() && self.link_attributes() {
            attr_text(attr)
        } else {
            Text::new()
        };
        if self.opts.reference_links {
            let label_text = inner.render(None);
            let label = self.add_reference(&label_text, false, target, original, &attrs);
            return if reference_key(&label) != reference_key(&label_text) {
                literal("![") + inner + literal(format!("][{label}]"))
            } else if self.shortcut_references() {
                literal("![") + inner + literal("]")
            } else {
                literal("![") + inner + literal("][]")
            };
        }
        let attrs = if attrs.is_empty() {
            empty()
        } else {
            attr_doc(attr)
        };
        literal("![") + inner + literal("](") + destination(target) + literal(")") + attrs
    }

    fn image(&mut self, attr: &Attr, content: &[Inline], target: &Target) -> Result<Doc> {
        self.images += 1;
        if self.plain() {
            let inner = self.inline_list(content, false)?;
            if self.opts.reference_links {
                self.add_reference(&inner.render(None), false, target, attr, "");
            }
            return Ok(literal("[") + inner + literal("]"));
        }
        if !attr.is_empty() && !self.link_attributes() && self.has(Extension::RawHtml) {
            // The HTML writer uses only the text of the description.
            let alt = alt_inlines(stringify(content), content);
            let image = Inline::Image(attr.clone(), alt, target.clone());
            return self.html_blocks(&[randoc_ast::Block::Plain(vec![image])]);
        }
        let inner = self.inline_list(content, false)?;
        Ok(self.image_doc(attr, inner, target))
    }

    fn styled_span(&mut self, style: SpanStyle, content: &[Inline]) -> Result<Doc> {
        let native = self.has(Extension::NativeSpans);
        let raw = self.has(Extension::RawHtml);
        let bracketed = self.has(Extension::BracketedSpans);
        let form = match style {
            SpanStyle::Underline if self.plain() => SpanForm::Delimited("", ""),
            SpanStyle::Underline if bracketed => SpanForm::Delimited("[", "]{.underline}"),
            SpanStyle::Underline if native => SpanForm::Span("underline"),
            SpanStyle::Underline if raw => SpanForm::Delimited("<u>", "</u>"),
            SpanStyle::Underline => SpanForm::Delimited("*", "*"),
            SpanStyle::SmallCaps if !self.plain() && (native || raw) => SpanForm::Span("smallcaps"),
            SpanStyle::SmallCaps => SpanForm::Upper,
        };
        match form {
            SpanForm::Span(class) => {
                let attr = Attr::new("", vec![class.into()], Vec::new());
                self.span(&attr, content)
            }
            SpanForm::Delimited(open, close) => self.delimited(content, open, close),
            SpanForm::Upper => {
                let outer = std::mem::replace(&mut self.upper, true);
                let inner = self.inline_list(content, false);
                self.upper = outer;
                inner
            }
        }
    }

    pub(crate) fn span(&mut self, attr: &Attr, content: &[Inline]) -> Result<Doc> {
        if self.plain() {
            return self.inline_list(content, false);
        }
        if attr.classes.len() == 1 && attr.has_class("emoji") {
            if self.has(Extension::Emoji)
                && let Some(name) = attr.get("data-emoji")
            {
                return Ok(literal(format!(":{name}:")));
            }
            return self.inline_list(content, false);
        }
        if self.has(Extension::Mark)
            && attr.id.is_empty()
            && attr.attributes.is_empty()
            && attr.classes.as_slice() == ["mark"]
        {
            let inner = self.inline_list(content, false)?;
            return Ok(literal("==") + inner + literal("=="));
        }
        if attr.is_empty() {
            return self.inline_list(content, false);
        }
        let inner = self.inline_list(content, false)?;
        if self.has(Extension::BracketedSpans) {
            return Ok(literal("[") + inner + literal("]") + attr_doc(&self.prefixed(attr)));
        }
        if self.has(Extension::NativeSpans) || self.has(Extension::RawHtml) {
            let mut open = literal("<span");
            for part in html_attr_parts(attr) {
                open += space() + literal(part);
            }
            return Ok(open + literal(">") + inner + literal("</span>"));
        }
        Ok(inner)
    }
}

/// Returns `true` when a text can be the label of a reference: it is not
/// empty and has no brackets, or it is one pair of brackets around a text
/// without brackets, as `[@key]`.
/// The form in which two reference labels are equal: lower case, with each
/// run of white space as one space, as Pandoc compares them (oracle: `A B`,
/// `A  B`, `A\tB` and `a\u{a0}b` are one label; `Straße` and `STRASSE`
/// are two).
pub(crate) fn reference_key(label: &str) -> Text {
    label
        .split(is_haskell_space)
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The white space of Haskell's `isSpace`.
fn is_haskell_space(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t'..='\r' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}'
            | '\u{205f}' | '\u{3000}'
    )
}

fn valid_label(text: &str) -> bool {
    let inner = text
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .filter(|inner| !inner.is_empty())
        .unwrap_or(text);
    !text.is_empty() && !inner.contains(['[', ']'])
}

/// The alternative text of an image for the HTML writer: one `Str` with the
/// text of `content`, or nothing for empty content. The HTML writer uses only
/// the text, so the result is the same as for `content`.
pub(crate) fn alt_inlines(text: Text, content: &[Inline]) -> Vec<Inline> {
    if content.is_empty() {
        Vec::new()
    } else {
        vec![Inline::Str(text)]
    }
}

/// The link destination with the title.
fn destination(target: &Target) -> Doc {
    let url = crate::url_text(&target.url);
    if target.title.is_empty() {
        literal(url)
    } else {
        literal(url) + literal(format!(" \"{}\"", target.title))
    }
}

#[cfg(test)]
mod tests {
    use randoc_ast::{Attr, Inline};
    use randoc_core::{WriterOptions, default_extensions};

    use crate::{Variant, W};

    const FORMATS: [(&str, Variant); 4] = [
        ("markdown", Variant::Markdown),
        ("markdown_strict", Variant::Markdown),
        ("gfm", Variant::Commonmark),
        ("plain", Variant::Plain),
    ];

    fn options(format: &str) -> WriterOptions {
        let mut options = WriterOptions::default();
        options.extensions = default_extensions(format);
        options
    }

    /// A random inline list: spaces, words and the elements whose edge
    /// spaces can go outside, with a linear congruential generator.
    fn random_list(seed: &mut u64, depth: usize) -> Vec<Inline> {
        fn next(seed: &mut u64, n: u64) -> u64 {
            *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (*seed >> 33) % n
        }
        let length = next(seed, 4);
        let mut out = Vec::new();
        for _ in 0..length {
            let kind = next(seed, if depth == 0 { 3 } else { 8 });
            out.push(match kind {
                0 => Inline::Space,
                1 => Inline::SoftBreak,
                2 => Inline::Str("w".into()),
                _ => {
                    let inner = random_list(seed, depth - 1);
                    match kind {
                        3 => Inline::Emph(inner),
                        4 => Inline::Strong(inner),
                        5 => Inline::Strikeout(inner),
                        6 => Inline::Underline(inner),
                        _ => Inline::Span(Attr::default(), inner),
                    }
                }
            });
        }
        out
    }

    /// Each list of the tree, also without its outer spaces.
    fn lists<'a>(list: &'a [Inline], out: &mut Vec<&'a [Inline]>) {
        out.push(list);
        let start = list.iter().position(|i| !super::is_space(i));
        let end = list.iter().rposition(|i| !super::is_space(i));
        if let (Some(start), Some(end)) = (start, end) {
            out.push(&list[start..=end]);
        }
        for inline in list {
            if let Inline::Emph(inner)
            | Inline::Strong(inner)
            | Inline::Strikeout(inner)
            | Inline::Underline(inner)
            | Inline::Span(_, inner) = inline
            {
                lists(inner, out);
            }
        }
    }

    #[test]
    fn the_stored_edge_spaces_equal_the_plain_walk() {
        let mut seed = 7;
        let mut compared = 0;
        for _ in 0..2000 {
            let tree = random_list(&mut seed, 5);
            let mut all = Vec::new();
            lists(&tree, &mut all);
            for (format, variant) in FORMATS {
                let options = options(format);
                let mut w = W::new(&options, variant, 128);
                for list in &all {
                    for first in [true, false] {
                        let plain = w.edge_space_scan(list, first);
                        assert_eq!(w.edge_space(list, first), plain, "{format} {list:?}");
                        compared += 1;
                    }
                }
            }
        }
        assert!(compared > 100_000, "{compared}");
    }

    /// Nested emphasis, `n` levels of `Emph` and `Strong` in turn, with a
    /// space at each end of the innermost text.
    fn nested(n: usize) -> Vec<Inline> {
        let mut inner = vec![Inline::Space, Inline::Str("x".into()), Inline::Space];
        for level in 0..n {
            inner = vec![if level % 2 == 0 {
                Inline::Emph(inner)
            } else {
                Inline::Strong(inner)
            }];
        }
        inner
    }

    #[test]
    fn the_edge_space_walk_is_linear_in_the_depth() {
        std::thread::Builder::new()
            .stack_size(1 << 28)
            .spawn(|| {
                for n in [1000, 4000] {
                    let list = nested(n);
                    let options = options("markdown");
                    let mut w = W::new(&options, Variant::Markdown, 2 * n + 10);
                    let text = w.inline_list(&list, true).map(|doc| doc.render(None));
                    assert!(
                        text.is_ok_and(|t| t.trim().starts_with("**") && t.contains('x')),
                        "{n}"
                    );
                    // Each list of the chain is walked once for each side.
                    assert!(w.edge_steps <= 2 * n + 2, "{n}: {} steps", w.edge_steps);
                    assert!(w.edge_memo.is_empty() && w.edge_keys.is_empty());
                    std::mem::forget(list);
                }
            })
            .map(|thread| thread.join().is_ok())
            .map_or_else(|_| unreachable!("the thread starts"), |ok| assert!(ok));
    }
}
