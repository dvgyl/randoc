//! Inline elements.

use randoc_ast::{Attr, Citation, CitationMode, Inline, MathType, QuoteType, Target, Text};
use randoc_core::{Extension, LogMessage, Result, WrapOption};
use randoc_layout::{Doc, cr, empty, literal, space};

use crate::escape::is_interrupting_marker;
use crate::html::{escape_html, html_attr_parts};
use randoc_core::shared::stringify;

use crate::{LinkRef, Variant, W};

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
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        _ => return None,
    })
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
                other => self.inline(other)?,
            };
            let doc = match (inline, next) {
                (Inline::Math(MathType::InlineMath, _), Some(Inline::Str(s)))
                    if s.starts_with(|c: char| c.is_ascii_digit())
                        && self.has(Extension::TexMathDollars)
                        && !self.has(Extension::TexMathGfm)
                        && !self.plain() =>
                {
                    if self.has(Extension::RawAttribute) {
                        doc + literal("`<!-- -->`{=html}")
                    } else {
                        doc + literal("<!-- -->")
                    }
                }
                _ => doc,
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

    fn line_break(&self) -> Doc {
        if self.in_heading && self.variant == Variant::Commonmark && !self.plain() {
            return literal("\\") + cr();
        }
        if self.in_heading {
            return literal(if self.in_script { "\\ " } else { " " });
        }
        if self.plain() {
            return cr();
        }
        if self.variant == Variant::Commonmark || self.has(Extension::EscapedLineBreaks) {
            literal("\\") + cr()
        } else {
            literal("  ") + cr()
        }
    }

    fn inline(&mut self, inline: &Inline) -> Result<Doc> {
        let plain = self.plain();
        Ok(match inline {
            Inline::Str(_) | Inline::Space | Inline::SoftBreak | Inline::LineBreak => empty(),
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
            Inline::SmallCaps(content) => {
                if plain {
                    let outer = self.upper;
                    self.upper = true;
                    let inner = self.inline_list(content, false);
                    self.upper = outer;
                    inner?
                } else {
                    let attr = Attr::new("", vec![Text::from("smallcaps")], Vec::new());
                    self.span(&attr, content)?
                }
            }
            Inline::Underline(content) if content.is_empty() => empty(),
            Inline::Underline(content) => {
                if !plain && self.has(Extension::BracketedSpans) {
                    self.delimited(content, "[", "]{.underline}")?
                } else if !plain && self.has(Extension::RawHtml) {
                    self.delimited(content, "<u>", "</u>")?
                } else {
                    self.delimited(content, "", "")?
                }
            }
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
            Inline::Link(attr, content, target) => self.link(attr, content, target)?,
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
                    literal(format!("[{number}]"))
                }
            }
            Inline::Span(attr, content) => self.span(attr, content)?,
        })
    }

    /// Renders content between delimiters. Spaces at the ends of the
    /// content, also inside nested emphasis, go outside the delimiters, and
    /// empty content gives nothing.
    fn delimited(&mut self, content: &[Inline], open: &str, close: &str) -> Result<Doc> {
        let (cut_lead, cut_trail) = std::mem::take(&mut self.cut_edges);
        if content.is_empty() {
            return Ok(empty());
        }
        // HTML tags keep the spaces inside.
        if self.in_script || open.starts_with('<') {
            let inner = self.inline_list(content, false)?;
            return Ok(literal(open) + inner + literal(close));
        }
        let is_space = |i: &Inline| matches!(i, Inline::Space | Inline::SoftBreak);
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
                    || !self.has(Extension::RawHtml) =>
            {
                Some(inner)
            }
            _ => None,
        }
    }

    /// Returns `true` when a space goes before (`first`) or after the
    /// delimited `content`. Content of spaces only gives a space before.
    fn edge_space(&self, content: &[Inline], first: bool) -> bool {
        let is_space = |i: &Inline| matches!(i, Inline::Space | Inline::SoftBreak);
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

    /// Returns `true` when links and images take attributes in braces.
    fn link_attributes(&self) -> bool {
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
            let converted = text
                .chars()
                .map(|c| if c == ' ' { Some(' ') } else { map(c) })
                .collect::<Option<Text>>();
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

    fn code(&self, attr: &Attr, text: &str) -> Doc {
        if self.plain() {
            return literal(text);
        }
        let ticks = "`".repeat(longest_run(text, '`') + 1);
        let pad = if text.contains('`') { " " } else { "" };
        let out = literal(format!("{ticks}{pad}{text}{pad}{ticks}"));
        if !attr.is_empty()
            && (self.has(Extension::InlineCodeAttributes) || self.has(Extension::Attributes))
        {
            return out + attr_doc(attr);
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

    fn raw_inline(&self, format: &str, text: &str) -> Doc {
        if self.plain() {
            return if format == "plain" {
                literal(text)
            } else {
                empty()
            };
        }
        let markdown = format.starts_with("markdown")
            || matches!(format, "commonmark" | "gfm" | "commonmark_x")
                && self.variant == Variant::Commonmark;
        if markdown {
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

    fn link(&mut self, attr: &Attr, content: &[Inline], target: &Target) -> Result<Doc> {
        if self.plain() {
            let inner = self.inline_list(content, false)?;
            if self.opts.reference_links {
                self.add_reference(&inner.render(None), target, "");
            }
            return Ok(inner);
        }
        let text_plain = stringify(content);
        let is_uri_auto =
            matches!(content, [Inline::Str(s)] if *s == target.url) && target.url.contains(':');
        let is_mail_auto =
            matches!(content, [Inline::Str(s)] if format!("mailto:{s}") == target.url);
        let auto_attr = attr.id.is_empty()
            && attr.attributes.is_empty()
            && attr.classes.iter().all(|c| c == "uri" || c == "email");
        if (is_uri_auto || is_mail_auto) && auto_attr {
            return Ok(literal(format!("<{text_plain}>")));
        }
        if !attr.is_empty() && !self.link_attributes() && self.has(Extension::RawHtml) {
            let inner = self.inline_list(content, false)?;
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
            return Ok(open + inner + literal("</a>"));
        }
        let inner = self.inline_list(content, false)?;
        let attrs = if !attr.is_empty() && self.link_attributes() {
            attr_text(attr)
        } else {
            Text::new()
        };
        if self.opts.reference_links {
            let label_text = inner.render(None);
            let label = self.add_reference(&label_text, target, &attrs);
            return Ok(if label == label_text {
                literal("[") + inner + literal("]")
            } else {
                literal("[") + inner + literal(format!("][{label}]"))
            });
        }
        let attrs = if attrs.is_empty() {
            empty()
        } else {
            attr_doc(attr)
        };
        Ok(literal("[") + inner + literal("](") + destination(target) + literal(")") + attrs)
    }

    /// Stores a link reference and returns its label.
    fn add_reference(&mut self, label: &str, target: &Target, attrs: &str) -> Text {
        let lower = label.to_lowercase();
        if let Some(found) = self
            .ref_index
            .get(&lower)
            .and_then(|&i| self.all_refs.get(i))
            && found.target == *target
            && found.attrs == attrs
        {
            return found.label.clone();
        }
        let label = if self.ref_index.contains_key(&lower) || label.is_empty() {
            let mut number = 1;
            while self.ref_index.contains_key(&number.to_string()) {
                number += 1;
            }
            number.to_string()
        } else {
            label.to_owned()
        };
        let reference = LinkRef {
            label: label.clone(),
            target: target.clone(),
            attrs: attrs.to_owned(),
        };
        self.ref_index
            .insert(label.to_lowercase(), self.all_refs.len());
        self.refs.push(LinkRef {
            label: label.clone(),
            target: target.clone(),
            attrs: attrs.to_owned(),
        });
        self.all_refs.push(reference);
        label
    }

    /// Renders an image in Markdown syntax with rendered alternative text.
    pub(crate) fn image_doc(&mut self, attr: &Attr, inner: Doc, target: &Target) -> Doc {
        let attrs = if !attr.is_empty() && self.link_attributes() {
            attr_text(attr)
        } else {
            Text::new()
        };
        if self.opts.reference_links {
            let label_text = inner.render(None);
            let label = self.add_reference(&label_text, target, &attrs);
            return if label == label_text {
                literal("![") + inner + literal("]")
            } else {
                literal("![") + inner + literal(format!("][{label}]"))
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
        if self.plain() {
            let inner = self.inline_list(content, false)?;
            if self.opts.reference_links {
                self.add_reference(&inner.render(None), target, "");
            }
            return Ok(literal("[") + inner + literal("]"));
        }
        if !attr.is_empty() && !self.link_attributes() && self.has(Extension::RawHtml) {
            // The HTML writer uses only the text of the description.
            let alt = vec![Inline::Str(stringify(content))];
            let image = Inline::Image(attr.clone(), alt, target.clone());
            return self.html_blocks(&[randoc_ast::Block::Plain(vec![image])]);
        }
        let inner = self.inline_list(content, false)?;
        Ok(self.image_doc(attr, inner, target))
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
        if attr.is_empty() {
            return self.inline_list(content, false);
        }
        let inner = self.inline_list(content, false)?;
        if self.has(Extension::BracketedSpans) {
            return Ok(literal("[") + inner + literal("]") + attr_doc(attr));
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

/// The link destination with the title.
fn destination(target: &Target) -> Doc {
    let url = crate::url_text(&target.url);
    if target.title.is_empty() {
        literal(url)
    } else {
        literal(url) + literal(format!(" \"{}\"", target.title))
    }
}
