use randoc_ast::{CitationMode, Inline, MathType, QuoteType, Target};
use randoc_core::{Extension, LogMessage, Result, WrapOption};
use randoc_layout::{Doc, after_break, cr, hang, literal, space};

use super::Render;
use crate::shared::{raw_format, scheme};

impl Render<'_> {
    pub(super) fn inlines(&mut self, inlines: &[Inline], depth: usize) -> Result<Doc> {
        self.depth(depth)?;
        let mut doc = Doc::default();
        for (position, inline) in inlines.iter().enumerate() {
            // A line must not start with a list marker or a footnote reference.
            let glued = match inlines.get(position + 1) {
                Some(Inline::Note(_)) => true,
                Some(Inline::Str(mark)) => list_word(mark),
                _ => false,
            };
            if matches!(inline, Inline::Space | Inline::SoftBreak) && glued {
                doc += literal(" ");
            } else {
                doc += self.inline(inline, depth)?;
            }
        }
        Ok(doc)
    }

    fn inline(&mut self, inline: &Inline, depth: usize) -> Result<Doc> {
        let smart = self.enabled(Extension::Smart) || self.enabled(Extension::SmartQuotes);
        let rendered = match inline {
            Inline::Str(text) => {
                let mut text = text.clone();
                if self.enabled(Extension::SpecialStrings) || self.enabled(Extension::Smart) {
                    text = text
                        .replace('—', "---")
                        .replace('–', "--")
                        .replace('…', "...")
                        .replace('’', "'")
                        .replace('\u{ad}', "\\-");
                }
                let escape = text.starts_with(['*', '#', '|']);
                if escape {
                    after_break("\u{200b}") + literal(text)
                } else {
                    literal(text)
                }
            }
            Inline::Space => space(),
            Inline::SoftBreak if self.options.wrap_text == WrapOption::WrapPreserve => cr(),
            Inline::SoftBreak => space(),
            Inline::LineBreak => literal("\\\\") + cr(),
            Inline::Emph(body) => self.delimit("/", body, "/", depth)?,
            Inline::Strong(body) => self.delimit("*", body, "*", depth)?,
            Inline::Underline(body) => self.delimit("_", body, "_", depth)?,
            Inline::Strikeout(body) => self.delimit("+", body, "+", depth)?,
            Inline::Superscript(body) => self.delimit("^{", body, "}", depth)?,
            Inline::Subscript(body) => self.delimit("_{", body, "}", depth)?,
            Inline::SmallCaps(body) => self.inlines(body, depth + 1)?,
            Inline::Span(attr, body) if body.is_empty() && !attr.id.is_empty() => {
                literal(format!("<<{}>>", attr.id))
            }
            Inline::Span(_, body) => self.inlines(body, depth + 1)?,
            Inline::Code(_, s) => {
                let marker = if s.contains('=') && !s.contains('~') {
                    '~'
                } else {
                    '='
                };
                literal(format!("{marker}{s}{marker}"))
            }
            Inline::Quoted(kind, body) => {
                let (start, end) = match (kind, smart) {
                    (QuoteType::SingleQuote, false) => ("‘", "’"),
                    (QuoteType::DoubleQuote, false) => ("“", "”"),
                    (QuoteType::SingleQuote, true) => ("'", "'"),
                    (QuoteType::DoubleQuote, true) => ("\"", "\""),
                };
                self.delimit(start, body, end, depth)?
            }
            Inline::Math(kind, body) => {
                let (start, end) = match kind {
                    MathType::InlineMath => ("\\(", "\\)"),
                    MathType::DisplayMath => ("\\[", "\\]"),
                };
                literal(format!("{start}{body}{end}"))
            }
            Inline::RawInline(format, text) if raw_format(format) => literal(text),
            Inline::RawInline(_, _) => {
                self.context
                    .report(LogMessage::InlineNotRendered(randoc_native::show_inline(
                        inline,
                        self.context
                            .limits
                            .max_nesting_depth
                            .min(crate::shared::STACK_LIMIT),
                    )));
                Doc::default()
            }
            Inline::Link(_, body, target) => self.link(body, target, depth)?,
            Inline::Image(_, _, target) => literal(format!("[[{}]]", url(target))),
            Inline::Note(blocks) => {
                let index = self.notes.len();
                let label = format!("[fn:{}]", index + 1);
                self.notes.push(Doc::default());
                let note = self.blocks(blocks, depth + 1)?;
                self.notes[index] = hang(label.len() + 1, literal(format!("{label} ")), note);
                literal(label)
            }
            Inline::Cite(citations, _)
                if self.enabled(Extension::Citations) && !citations.is_empty() =>
            {
                let style = match citations.as_slice() {
                    [citation, ..] if citation.mode == CitationMode::AuthorInText => "/t",
                    [citation] if citation.mode == CitationMode::SuppressAuthor => "/na",
                    _ => "",
                };
                let mut doc = literal(format!("[cite{style}:"));
                for (index, citation) in citations.iter().enumerate() {
                    if index > 0 {
                        doc += literal(";") + space();
                    }
                    if !citation.prefix.is_empty() {
                        doc += self.inlines(&citation.prefix, depth + 1)? + space();
                    }
                    doc += literal(format!("@{}", citation.id));
                    doc += self.citation_suffix(&citation.suffix, depth)?;
                }
                doc + literal("]")
            }
            Inline::Cite(_, body) => self.inlines(body, depth + 1)?,
        };
        Ok(rendered)
    }

    /// Writes a citation suffix. A suffix that does not start with white space
    /// loses its leading comma, and a comma separates it from the key unless it
    /// starts with a locator such as `p. 12`.
    fn citation_suffix(&mut self, suffix: &[Inline], depth: usize) -> Result<Doc> {
        if matches!(suffix.first(), Some(Inline::Space | Inline::SoftBreak)) {
            let start = suffix
                .iter()
                .position(|inline| !matches!(inline, Inline::Space | Inline::SoftBreak))
                .unwrap_or(suffix.len());
            if start == suffix.len() {
                return Ok(Doc::default());
            }
            let suffix = locator_space(&suffix[start..]);
            return Ok(space() + self.inlines(&suffix, depth + 1)?);
        }
        let mut suffix = suffix.to_vec();
        if let Some(Inline::Str(first)) = suffix.first_mut()
            && let Some(rest) = first.strip_prefix(',')
        {
            if rest.is_empty() {
                suffix.remove(0);
            } else {
                *first = rest.into();
            }
        }
        let start = suffix
            .iter()
            .position(|inline| !matches!(inline, Inline::Space | Inline::SoftBreak))
            .unwrap_or(suffix.len());
        if start == suffix.len() {
            return Ok(Doc::default());
        }
        let text = self.inlines(&locator_space(&suffix[start..]), depth + 1)?;
        if locator(&text.render(None)) {
            Ok(space() + text)
        } else {
            Ok(space() + literal(",") + space() + text)
        }
    }

    fn delimit(&mut self, start: &str, body: &[Inline], end: &str, depth: usize) -> Result<Doc> {
        Ok(literal(start) + self.inlines(body, depth + 1)? + literal(end))
    }

    fn link(&mut self, body: &[Inline], target: &Target, depth: usize) -> Result<Doc> {
        let destination = url(target);
        let description = self.inlines(body, depth + 1)?;
        let text = description.render(None);
        if text == target.url && !text.contains(char::is_whitespace) {
            return Ok(literal(format!("[[{destination}]]")));
        }
        let trailing_bracket = text.ends_with(']');
        let content = text.strip_suffix(']').unwrap_or(&text);
        let mut escaped = String::with_capacity(text.len());
        let mut chars = content.chars().peekable();
        while let Some(c) = chars.next() {
            escaped.push(c);
            if c == ']'
                && (chars.peek() == Some(&']') || (chars.peek().is_none() && trailing_bracket))
            {
                escaped.push('\u{200b}');
            }
        }
        let end = if trailing_bracket {
            "\u{200b}]]]"
        } else {
            "]]"
        };
        Ok(literal(format!("[[{destination}][{escaped}{end}")))
    }
}

fn url(target: &Target) -> String {
    let url = &target.url;
    let prefix = if url.is_empty()
        || scheme(url)
        || url.starts_with(['#', '/'])
        || url.starts_with("./")
        || url.starts_with("../")
    {
        ""
    } else {
        "file:"
    };
    format!("{prefix}{}", url.replace('[', "\\[").replace(']', "\\]"))
}

/// Returns `true` when text starts with a locator: a word with a digit, or a
/// locator label such as `p.` or `chap.` before a word with a digit.
/// Pandoc writes the no-break space after a locator label, as in `p. 12`,
/// as a normal space.
fn locator_space(suffix: &[Inline]) -> Vec<Inline> {
    let mut suffix = suffix.to_vec();
    if let Some(Inline::Str(first)) = suffix.first_mut()
        && let Some((label, rest)) = first.split_once('\u{a0}')
        && !label.is_empty()
        && locator(first)
    {
        *first = format!("{label} {rest}");
    }
    suffix
}

fn locator(text: &str) -> bool {
    let digit = |word: &str| word.bytes().any(|b| b.is_ascii_digit());
    let mut words = text.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    digit(first)
        || ([
            "book",
            "bk",
            "books",
            "bks",
            "chapter",
            "chap",
            "chapters",
            "chaps",
            "column",
            "col",
            "columns",
            "cols",
            "figure",
            "fig",
            "figures",
            "figs",
            "folio",
            "fol",
            "folios",
            "fols",
            "number",
            "no",
            "numbers",
            "nos",
            "line",
            "l",
            "lines",
            "ll",
            "note",
            "n",
            "notes",
            "nn",
            "opus",
            "op",
            "opera",
            "opp",
            "page",
            "p",
            "pages",
            "pp",
            "paragraph",
            "para",
            "paragraphs",
            "paras",
            "part",
            "pt",
            "parts",
            "pts",
            "section",
            "sec",
            "sections",
            "secs",
            "verse",
            "v",
            "verses",
            "vv",
            "volume",
            "vol",
            "volumes",
            "vols",
        ]
        .contains(&first.strip_suffix('.').unwrap_or(first))
            && words.next().is_some_and(digit))
}

fn list_word(word: &str) -> bool {
    word == "-"
        || word
            .strip_suffix(['.', ')'])
            .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}
