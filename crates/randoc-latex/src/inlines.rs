//! Inline elements.

use std::collections::HashSet;

use randoc_ast::{
    Attr, Block, Citation, CitationMode, Format, Inline, MathType, QuoteType, Target,
};
use randoc_core::{CiteMethod, Result, WrapOption};
use randoc_layout::{Doc, cr, empty, literal, nest, space};

use crate::LatexWriter;
use crate::escape::{StringContext, escape_uri, to_label};
use crate::shared::{Dimension, Direction, dimension, inlines_any, show_fl, unescape_uri};

const NBSP: char = '\u{a0}';

impl LatexWriter<'_> {
    /// Renders a list of inlines.
    pub(crate) fn inlines(&mut self, inlines: &[Inline]) -> Result<Doc> {
        self.inline_seq(inlines.iter())
    }

    /// Renders a sequence of inlines, for example the lines of a line block
    /// with line breaks between them.
    pub(crate) fn inline_seq<'i>(
        &mut self,
        inlines: impl Iterator<Item = &'i Inline>,
    ) -> Result<Doc> {
        let mut items: Vec<&Inline> = inlines.collect();
        if self.box_lines {
            // A line break at the end of a boxed cell gives no line.
            while matches!(items.last(), Some(Inline::LineBreak)) {
                items.pop();
            }
        }
        if self.box_lines && items.iter().any(|i| matches!(i, Inline::LineBreak)) {
            // A cell of a column with no width cannot break lines: each line is a box.
            let mut doc = literal("\\vtop{");
            for line in items.split(|i| matches!(i, Inline::LineBreak)) {
                doc += literal("\\hbox{\\strut ")
                    + self.inline_seq_unboxed(line.iter().copied())?
                    + literal("}");
            }
            return Ok(doc + literal("}"));
        }
        self.inline_seq_unboxed(items.into_iter())
    }

    fn inline_seq_unboxed<'i>(&mut self, inlines: impl Iterator<Item = &'i Inline>) -> Result<Doc> {
        let mut out = Doc::default();
        let mut rest = inlines.peekable();
        // `\\` at the start of a paragraph is an error in LaTeX.
        while rest
            .next_if(|inline| matches!(inline, Inline::LineBreak))
            .is_some()
        {
            out += literal("\\hfill\\break\n");
            // A line break after it needs no strut.
            self.empty_line = false;
        }
        let mut after_break = false;
        let mut previous: Option<&Inline> = None;
        for inline in rest {
            if quote_meets_quote(previous, inline) {
                out += literal("\\,");
            }
            previous = Some(inline);
            if after_break
                && let Inline::Str(text) = inline
                && text.starts_with(NBSP)
            {
                // A tilde after a line break needs a strut to show its space.
                out += literal("\\strut ");
            }
            after_break = matches!(inline, Inline::LineBreak);
            out += self.inline(inline)?;
        }
        Ok(out)
    }

    fn command(&mut self, name: &str, inlines: &[Inline]) -> Result<Doc> {
        let contents = self.inlines(inlines)?;
        Ok(literal(format!("\\{name}{{")) + contents + literal("}"))
    }

    /// Renders a command of the `soul` package, which needs code and math in
    /// an `\mbox`.
    fn soul(&mut self, name: &str, inlines: &[Inline]) -> Result<Doc> {
        let was_in_soul = self.in_soul;
        self.in_soul = true;
        let result = self.command(name, inlines);
        self.in_soul = was_in_soul;
        result
    }

    /// Renders `\emph`, `\textbf`, `\ul` or `\st`. A note that is not one
    /// paragraph goes after the command, and the command starts again after
    /// the note, as in Pandoc.
    fn emphasis(&mut self, name: &str, inline: &Inline, inlines: &[Inline]) -> Result<Doc> {
        let key = std::ptr::from_ref(inline) as usize;
        if !self.emphasis_seen.contains(&key) {
            let budget = self.max_depth.saturating_sub(self.depth);
            mark_split_emphasis(
                inline,
                budget,
                &mut self.emphasis_seen,
                &mut self.emphasis_split,
            );
        }
        if !self.emphasis_split.contains(&key) {
            return self.emphasis_command(name, inlines);
        }
        let mut out = Doc::default();
        for part in self.emphasis_parts(name, inlines)? {
            out += part;
        }
        Ok(out)
    }

    fn emphasis_command(&mut self, name: &str, inlines: &[Inline]) -> Result<Doc> {
        if name == "ul" || name == "st" {
            self.soul(name, inlines)
        } else {
            self.command(name, inlines)
        }
    }

    /// The parts of an emphasis command that a note splits: each segment in
    /// the command, with each note between two segments.
    fn emphasis_parts(&mut self, name: &str, inlines: &[Inline]) -> Result<Vec<Doc>> {
        let soul = name == "ul" || name == "st";
        let wrap = |contents: Doc| literal(format!("\\{name}{{")) + contents + literal("}");
        let mut parts = Vec::new();
        let mut segment = Doc::default();
        let mut start = 0;
        for (index, child) in inlines.iter().enumerate() {
            let key = std::ptr::from_ref(child) as usize;
            let is_note = matches!(child, Inline::Note(blocks) if is_complex_note(blocks));
            if !is_note && !self.emphasis_split.contains(&key) {
                continue;
            }
            segment += self.soul_seq(soul, &inlines[start..index])?;
            start = index + 1;
            if is_note {
                parts.push(wrap(std::mem::take(&mut segment)));
                parts.push(self.inline(child)?);
                continue;
            }
            let (Inline::Emph(inner)
            | Inline::Strong(inner)
            | Inline::Underline(inner)
            | Inline::Strikeout(inner)) = child
            else {
                continue;
            };
            let inner_name = match child {
                Inline::Emph(_) => "emph",
                Inline::Strong(_) => "textbf",
                Inline::Underline(_) => "ul",
                _ => "st",
            };
            self.enter()?;
            let inner_parts = self.emphasis_parts(inner_name, inner);
            self.leave();
            for (position, part) in inner_parts?.into_iter().enumerate() {
                if position % 2 == 0 {
                    segment += part;
                } else {
                    parts.push(wrap(std::mem::take(&mut segment)));
                    parts.push(part);
                }
            }
        }
        segment += self.soul_seq(soul, &inlines[start..])?;
        parts.push(wrap(segment));
        Ok(parts)
    }

    fn soul_seq(&mut self, soul: bool, inlines: &[Inline]) -> Result<Doc> {
        let was_in_soul = self.in_soul;
        self.in_soul |= soul;
        let result = self.inlines(inlines);
        self.in_soul = was_in_soul;
        result
    }

    fn inline(&mut self, inline: &Inline) -> Result<Doc> {
        if self.skip_notes && matches!(inline, Inline::Note(_)) {
            return Ok(empty());
        }
        if is_invalid_in_bookmark(inline) {
            self.saw_invalid = true;
            if self.skip_invalid {
                return Ok(empty());
            }
        }
        if matches!(inline, Inline::Note(_)) {
            self.saw_note = true;
        }
        self.enter()?;
        let outer_saw_invalid = std::mem::replace(&mut self.saw_invalid, false);
        let mut result = self.inline_inner(inline);
        if self.skip_invalid && self.saw_invalid {
            // An element that holds a note is left out as a whole.
            result = result.map(|_| empty());
        }
        self.saw_invalid |= outer_saw_invalid;
        self.leave();
        result
    }

    fn inline_inner(&mut self, inline: &Inline) -> Result<Doc> {
        // Code and raw text for other formats keep the start of a line, as in Pandoc.
        let keeps_line_start = match inline {
            Inline::LineBreak
            | Inline::Space
            | Inline::SoftBreak
            | Inline::Code(..)
            | Inline::Emph(_)
            | Inline::Strong(_)
            | Inline::Underline(_)
            | Inline::Strikeout(_)
            | Inline::Superscript(_)
            | Inline::Subscript(_)
            | Inline::SmallCaps(_)
            | Inline::Span(..)
            | Inline::Link(..)
            | Inline::Quoted(..)
            | Inline::Cite(..) => true,
            Inline::RawInline(format, _) => !self.is_raw_tex(format),
            _ => false,
        };
        if !keeps_line_start {
            self.empty_line = false;
        }
        Ok(match inline {
            Inline::Str(text) => literal(self.escape(StringContext::Text, text)),
            Inline::Emph(inlines) => self.emphasis("emph", inline, inlines)?,
            Inline::Strong(inlines) => self.emphasis("textbf", inline, inlines)?,
            Inline::Underline(inlines) => self.emphasis("ul", inline, inlines)?,
            Inline::Strikeout(inlines) => self.emphasis("st", inline, inlines)?,
            Inline::Superscript(inlines) => self.command("textsuperscript", inlines)?,
            Inline::Subscript(inlines) => self.command("textsubscript", inlines)?,
            Inline::SmallCaps(inlines) => self.command("textsc", inlines)?,
            Inline::Quoted(kind, inlines) => self.quoted(*kind, inlines)?,
            Inline::Cite(citations, inlines) => self.cite(citations, inlines)?,
            Inline::Code(attr, code) => {
                self.saw_code = true;
                let code = self.code(attr, code);
                if self.in_soul {
                    literal("\\mbox{") + code + literal("}")
                } else {
                    code
                }
            }
            Inline::Space => space(),
            Inline::SoftBreak => match self.options.wrap_text {
                WrapOption::WrapPreserve => cr(),
                WrapOption::WrapAuto | WrapOption::WrapNone => space(),
            },
            Inline::LineBreak => {
                let strut = if self.empty_line { "\\strut " } else { "" };
                self.empty_line = true;
                literal(format!("{strut}\\\\")) + cr()
            }
            Inline::Math(kind, math) => {
                let math = handle_math_comment(math);
                match kind {
                    MathType::InlineMath if self.in_soul => literal(format!("${math}$")),
                    MathType::DisplayMath if self.in_soul => literal(format!("$${math}$$")),
                    MathType::InlineMath => literal(format!("\\({math}\\)")),
                    MathType::DisplayMath if self.in_cell => literal(format!("\\({math}\\)")),
                    MathType::DisplayMath => literal(format!("\\[{math}\\]")),
                }
            }
            Inline::RawInline(format, text) => {
                if self.is_raw_tex(format) {
                    literal(text.clone())
                } else {
                    empty()
                }
            }
            Inline::Link(_, inlines, _) if self.links_as_groups => {
                literal("{") + self.inlines(inlines)? + literal("}")
            }
            Inline::Link(attr, inlines, target) => {
                let link = self.link(attr, inlines, target)?;
                if self.in_soul && !target.url.starts_with('#') {
                    literal("\\mbox{") + link + literal("}")
                } else {
                    link
                }
            }
            Inline::Image(attr, inlines, target) => self.image(attr, inlines, target)?,
            Inline::Note(blocks) => self.note(blocks)?,
            Inline::Span(attr, inlines) => self.span(attr, inlines)?,
        })
    }

    pub(crate) fn is_raw_tex(&self, format: &Format) -> bool {
        *format == "latex" || *format == "tex" || (self.beamer && *format == "beamer")
    }

    fn quoted(&mut self, kind: QuoteType, inlines: &[Inline]) -> Result<Doc> {
        let contents = self.inlines(inlines)?;
        let is_quoted = |inline: Option<&Inline>| matches!(inline, Some(Inline::Quoted(..)));
        let before = if is_quoted(inlines.first()) {
            literal("\\,")
        } else {
            empty()
        };
        let after = if is_quoted(inlines.last()) {
            literal("\\,")
        } else {
            empty()
        };
        let inner = before + contents + after;
        let (open, close) = match (kind, self.smart) {
            (QuoteType::DoubleQuote, true) => ("``", "''"),
            (QuoteType::DoubleQuote, false) => ("\u{201C}", "\u{201D}"),
            (QuoteType::SingleQuote, true) => ("`", "'"),
            (QuoteType::SingleQuote, false) => ("\u{2018}", "\u{2019}"),
        };
        Ok(literal(open) + inner + literal(close))
    }

    fn code(&mut self, attr: &Attr, code: &str) -> Doc {
        // `\lstinline` cannot be in a moving argument, so a heading and a
        // term get `\texttt`.
        if self.options.listings && !self.in_heading && !self.in_term {
            let protect = if self.in_caption { "\\protect" } else { "" };
            return literal(format!("{protect}{}", listings_inline(attr, code)));
        }
        if self.options.highlight_style.is_some()
            && !self.in_heading
            && (attr
                .classes
                .iter()
                .any(|class| crate::blocks::is_known_language(class))
                || crate::blocks::has_number_lines(attr))
        {
            let escaped = crate::blocks::highlight_escape(code);
            return literal(format!("\\VERB|\\NormalTok{{{escaped}}}|"));
        }
        let escaped = self.escape(StringContext::Code, code).replace(' ', "\\ ");
        let protect = if self.in_caption { "\\protect" } else { "" };
        literal(format!("{protect}\\texttt{{{escaped}}}"))
    }

    fn link(&mut self, attr: &Attr, inlines: &[Inline], target: &Target) -> Result<Doc> {
        let url = &target.url;
        let doc = if let Some(identifier) = url.strip_prefix('#') {
            let contents = self.inlines(inlines)?;
            if self.beamer {
                literal(format!("\\hyperlink{{{}}}{{", to_label(identifier)))
                    + contents
                    + literal("}")
            } else {
                literal(format!("\\hyperref[{}]{{", to_label(identifier))) + contents + literal("}")
            }
        } else {
            let source = self.escape(StringContext::Url, &escape_uri(url));
            match inlines {
                [Inline::Str(text)]
                    if url.is_ascii() && unescape_uri(text) == unescape_uri(url) =>
                {
                    literal(format!("\\url{{{source}}}"))
                }
                [Inline::Str(text)]
                    if url
                        .strip_prefix("mailto:")
                        .is_some_and(|rest| unescape_uri(text) == unescape_uri(rest)) =>
                {
                    let contents = self.inlines(inlines)?;
                    literal(format!("\\href{{{source}}}{{\\nolinkurl{{")) + contents + literal("}}")
                }
                _ => {
                    let contents = self.inlines(inlines)?;
                    literal(format!("\\href{{{source}}}{{")) + contents + literal("}")
                }
            }
        };
        Ok(self.inline_anchor(&attr.id) + doc)
    }

    pub(crate) fn inline_anchor(&self, identifier: &str) -> Doc {
        if identifier.is_empty() {
            empty()
        } else if self.in_heading {
            literal(format!(
                "\\protect\\hypertarget{{{}}}{{}}",
                to_label(identifier)
            ))
        } else {
            literal(format!(
                "\\protect\\phantomsection\\label{{{}}}",
                to_label(identifier)
            ))
        }
    }

    fn image(&mut self, attr: &Attr, description: &[Inline], target: &Target) -> Result<Doc> {
        let width = dimension(Direction::Width, attr);
        let height = dimension(Direction::Height, attr);
        let mut options: Vec<String> = Vec::new();
        let bounded = width.is_none() && height.is_none();
        if bounded {
            options.push("keepaspectratio".into());
        } else {
            options.push(match &width {
                Some(dim) => format!("width={}", self.show_dim(dim, Direction::Width)),
                None => "width=\\linewidth".into(),
            });
            options.push(match &height {
                Some(dim) => format!("height={}", self.show_dim(dim, Direction::Height)),
                None => "height=\\textheight".into(),
            });
            if width.is_none() || height.is_none() {
                options.push("keepaspectratio".into());
            }
        }
        for key in ["page", "trim"] {
            if let Some(value) = attr.get(key) {
                options.push(format!("{key}={value}"));
            }
        }
        let clip = attr.get("clip").is_some();
        let alt = randoc_core::shared::stringify(description);
        if contains_code(description, self.max_depth.saturating_sub(self.depth)) {
            // Code in an image description also makes a Beamer frame fragile.
            self.saw_code = true;
        }
        let url = &target.url;
        let source = if url.contains("://") {
            url.clone()
        } else {
            unescape_uri(url)
        };
        let source = self.escape(StringContext::Url, &encode_image_path(&source));
        let protect = if self.in_heading { "\\protect" } else { "" };
        let mut option_doc = literal(options.join(","));
        let svg = url.to_ascii_lowercase().ends_with(".svg");
        let command = if svg { "includesvg" } else { "includegraphics" };
        if !description.is_empty() && !svg {
            let escaped = self.escape(StringContext::Text, &alt);
            let mut words = Doc::default();
            for (index, word) in escaped.split_whitespace().enumerate() {
                if index > 0 {
                    words += space();
                }
                words += literal(word.to_owned());
            }
            option_doc += literal(",alt={") + words + literal("}");
        }
        if clip {
            option_doc += literal(",clip");
        }
        let graphic =
            literal(format!("\\{command}[")) + option_doc + literal(format!("]{{{source}}}"));
        Ok(if bounded {
            literal(format!("{protect}\\pandocbounded{{")) + graphic + literal("}")
        } else {
            literal(protect) + graphic
        })
    }

    fn show_dim(&self, dim: &Dimension, direction: Direction) -> String {
        match dim {
            Dimension::Pixel(pixels) => {
                let dpi = if self.options.dpi > 0 {
                    self.options.dpi
                } else {
                    96
                };
                format!("{}in", show_fl(pixels.floor() / dpi as f64))
            }
            Dimension::Percent(percent) => {
                let base = match direction {
                    Direction::Width => "\\linewidth",
                    Direction::Height => "\\textheight",
                };
                format!("{}{base}", show_fl(percent / 100.0))
            }
            Dimension::Unit(value, unit) => format!("{}{unit}", show_fl(*value)),
        }
    }

    fn note(&mut self, blocks: &[Block]) -> Result<Doc> {
        if self.repeated_head {
            return Ok(literal("{}"));
        }
        let nested = self.in_note;
        self.in_note = true;
        let outer_notes = if nested || self.in_minipage {
            None
        } else {
            Some(std::mem::take(&mut self.table_notes))
        };
        let contents = self.blocks(blocks);
        self.in_note = nested;
        let inner_notes = match outer_notes {
            Some(outer) => std::mem::replace(&mut self.table_notes, outer),
            None => Vec::new(),
        };
        let contents = contents?;

        let open = if self.beamer && self.incremental {
            "\\footnote<.->[frame]{"
        } else if self.beamer {
            "\\footnote<\\value{beamerpauses}->[frame]{"
        } else {
            "\\footnote{"
        };
        let contents = nest(2, contents);
        if self.in_minipage || nested {
            self.table_notes.push(contents);
            return Ok(literal("\\footnotemark{}"));
        }
        self.block_notes.extend(inner_notes);
        Ok(literal(open) + contents + literal("}"))
    }

    fn span(&mut self, attr: &Attr, inlines: &[Inline]) -> Result<Doc> {
        for (class, command) in CSL_SPANS {
            if attr.has_class(class) {
                let contents = self.inlines(inlines)?;
                let doc = literal(format!("\\{command}{{")) + contents + literal("}");
                return Ok(match *class {
                    // No space may come between the margin and the text.
                    "csl-right-inline" => literal("%") + cr() + doc,
                    _ => cr() + doc,
                });
            }
        }
        let mut commands: Vec<String> = Vec::new();
        if attr.has_class("csl-no-emph") {
            commands.push("textup".into());
        }
        if attr.has_class("csl-no-strong") || attr.has_class("csl-no-smallcaps") {
            commands.push("textnormal".into());
        }
        match attr.get("dir") {
            Some("rtl") => commands.push("RL".into()),
            Some("ltr") => commands.push("LR".into()),
            _ => {}
        }
        let soul = attr.has_class("mark") && !attr.has_class("underline") && attr.id.is_empty();
        if soul {
            commands.push("hl".into());
        }
        if let Some(name) = attr.get("lang").and_then(crate::lang::babel_name) {
            commands.push(format!("foreignlanguage{{{name}}}"));
        }
        let was_in_soul = self.in_soul;
        self.in_soul |= soul;
        let contents = self.inlines(inlines);
        self.in_soul = was_in_soul;
        let mut contents = contents?;
        if commands.is_empty() {
            contents = literal("{") + contents + literal("}");
        } else {
            for command in commands.iter().rev() {
                contents = literal(format!("\\{command}{{")) + contents + literal("}");
            }
        }
        Ok(self.inline_anchor(&attr.id) + contents)
    }

    fn cite(&mut self, citations: &[Citation], inlines: &[Inline]) -> Result<Doc> {
        match self.options.cite_method {
            CiteMethod::Citeproc => self.inlines(inlines),
            CiteMethod::Natbib => self.natbib(citations),
            CiteMethod::Biblatex => self.biblatex(citations),
        }
    }

    /// Renders the optional arguments and the key of one citation.
    fn cite_arguments(
        &mut self,
        prefix: &[Inline],
        suffix: &[Inline],
        key: &str,
        biblatex: bool,
    ) -> Result<Doc> {
        let prefix_doc = self.inlines(prefix)?;
        let mut suffix_doc = Doc::default();
        for piece in clean_suffix(suffix, biblatex) {
            suffix_doc += match piece {
                SuffixPiece::Text(text) => literal(self.escape(StringContext::Text, &text)),
                SuffixPiece::Inline(inline) => self.inline(inline)?,
                SuffixPiece::Unwrapped(inlines) => {
                    let mut flat = Vec::new();
                    let budget = self.max_depth.saturating_sub(self.depth);
                    without_emphasis(inlines, &mut flat, budget)?;
                    self.inline_seq(flat.into_iter())?
                }
            };
        }
        let arguments = match (prefix_doc.is_empty(), suffix_doc.is_empty()) {
            (true, true) => empty(),
            (true, false) => literal("[") + suffix_doc + literal("]"),
            (false, _) => literal("[") + prefix_doc + literal("][") + suffix_doc + literal("]"),
        };
        Ok(arguments + literal(format!("{{{key}}}")))
    }

    fn cite_command(&mut self, command: &str, citation: &Citation, biblatex: bool) -> Result<Doc> {
        let arguments =
            self.cite_arguments(&citation.prefix, &citation.suffix, &citation.id, biblatex)?;
        Ok(literal(command) + arguments)
    }

    fn natbib(&mut self, citations: &[Citation]) -> Result<Doc> {
        self.natbib_with(citations, None)
    }

    /// Renders natbib citations. `first_mode` replaces the mode of the first citation.
    fn natbib_with(
        &mut self,
        citations: &[Citation],
        first_mode: Option<CitationMode>,
    ) -> Result<Doc> {
        let (Some(first), Some(last)) = (citations.first(), citations.last()) else {
            return Ok(literal("\\citep{}"));
        };
        let mode_of = |index: usize, citation: &Citation| match first_mode {
            Some(mode) if index == 0 => mode,
            _ => citation.mode,
        };
        if citations.len() == 1 {
            let command = match mode_of(0, first) {
                CitationMode::AuthorInText => "\\citet",
                CitationMode::SuppressAuthor => "\\citeyearpar",
                CitationMode::NormalCitation => "\\citep",
            };
            return self.cite_command(command, first, false);
        }
        let all_normal = citations
            .iter()
            .enumerate()
            .all(|(index, c)| mode_of(index, c) == CitationMode::NormalCitation);
        let inner_affixes = citations.iter().skip(1).any(|c| !c.prefix.is_empty())
            || citations.iter().rev().skip(1).any(|c| !c.suffix.is_empty());
        if all_normal && !inner_affixes {
            let keys: Vec<&str> = citations.iter().map(|c| c.id.as_str()).collect();
            let arguments =
                self.cite_arguments(&first.prefix, &last.suffix, &keys.join(", "), false)?;
            return Ok(literal("\\citep") + arguments);
        }
        if mode_of(0, first) == CitationMode::AuthorInText {
            let author = literal(format!("\\citeauthor{{{}}}", first.id));
            let rest = self.natbib_with(citations, Some(CitationMode::SuppressAuthor))?;
            return Ok(author.spaced(rest));
        }
        let mut joined = Doc::default();
        for (index, citation) in citations.iter().enumerate() {
            if index > 0 {
                joined += literal("; ");
            }
            let command = match mode_of(index, citation) {
                CitationMode::AuthorInText => "\\citealt",
                CitationMode::SuppressAuthor => "\\citeyear",
                CitationMode::NormalCitation => "\\citealp",
            };
            joined += self.cite_command(command, citation, false)?;
        }
        Ok(literal("\\citetext{") + joined + literal("}"))
    }

    fn biblatex(&mut self, citations: &[Citation]) -> Result<Doc> {
        let Some(first) = citations.first() else {
            return Ok(empty());
        };
        let single = match first.mode {
            CitationMode::AuthorInText => "\\textcite",
            CitationMode::SuppressAuthor => "\\autocite*",
            CitationMode::NormalCitation => "\\autocite",
        };
        if citations.len() == 1 {
            return self.cite_command(single, first, true);
        }
        if citations
            .iter()
            .all(|c| c.prefix.is_empty() && c.suffix.is_empty())
        {
            let keys: Vec<&str> = citations.iter().map(|c| c.id.as_str()).collect();
            return Ok(literal(format!("{single}{{{}}}", keys.join(","))));
        }
        let plural = match first.mode {
            CitationMode::AuthorInText => "\\textcites",
            CitationMode::SuppressAuthor => "\\autocites*",
            CitationMode::NormalCitation => "\\autocites",
        };
        let mut doc = literal(plural);
        for citation in citations {
            doc += self.cite_arguments(&citation.prefix, &citation.suffix, &citation.id, true)?;
        }
        Ok(doc)
    }
}

/// The span classes of citation processor output and their commands.
const CSL_SPANS: &[(&str, &str)] = &[
    ("csl-block", "CSLBlock"),
    ("csl-left-margin", "CSLLeftMargin"),
    ("csl-right-inline", "CSLRightInline"),
    ("csl-indent", "CSLIndent"),
];

/// Two quotes that meet need a thin space between them.
fn quote_meets_quote(previous: Option<&Inline>, current: &Inline) -> bool {
    const QUOTES: [char; 4] = ['\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}'];
    let ends_with_quote = match previous {
        Some(Inline::Str(text)) => text.ends_with(QUOTES),
        Some(Inline::Quoted(..)) => true,
        _ => false,
    };
    let starts_with_quote = match current {
        Inline::Str(text) => text.starts_with(QUOTES),
        Inline::Quoted(..) => true,
        _ => false,
    };
    let one_quoted =
        matches!(previous, Some(Inline::Quoted(..))) != matches!(current, Inline::Quoted(..));
    ends_with_quote && starts_with_quote && one_quoted
}

/// The labels of page locators, which biblatex adds itself.
const PAGE_LABELS: &[&str] = &["pp.", "p.", "pages", "page"];

/// One part of a cleaned citation suffix.
#[derive(Debug, PartialEq)]
enum SuffixPiece<'a> {
    /// Text of a `Str`, with locator braces removed.
    Text(String),
    /// Any other inline.
    Inline(&'a Inline),
    /// The contents of emphasis around a locator, which loses each level
    /// of emphasis.
    Unwrapped(&'a [Inline]),
}

fn is_space(piece: &SuffixPiece<'_>) -> bool {
    matches!(
        piece,
        SuffixPiece::Inline(Inline::Space | Inline::SoftBreak)
    )
}

/// Removes a leading comma and locator braces from a citation suffix. For
/// biblatex it also removes a page label.
fn clean_suffix(suffix: &[Inline], biblatex: bool) -> Vec<SuffixPiece<'_>> {
    let mut items: Vec<SuffixPiece<'_>> = suffix
        .iter()
        .map(|inline| match inline {
            Inline::Str(text) => SuffixPiece::Text(text.replace(['{', '}'], "")),
            other => SuffixPiece::Inline(other),
        })
        .collect();
    // The pieces before `start` go away in one drain, so that a long run of
    // spaces costs linear time.
    let mut start = 0;
    if let Some(Inline::Str(text)) = suffix.first()
        && let Some(rest) = text.strip_prefix(',')
    {
        if rest.is_empty() {
            start = after_spaces(&items, 1);
        } else if let Some(first) = items.first_mut() {
            *first = SuffixPiece::Text(rest.replace(['{', '}'], ""));
        }
    }
    if biblatex {
        let first = after_spaces(&items, start);
        let space_after = items.get(first + 1).is_some_and(is_space);
        if let Some(SuffixPiece::Text(text)) = items.get_mut(first)
            && let Some(rest) = PAGE_LABELS
                .iter()
                .find_map(|label| text.strip_prefix(label))
        {
            if let Some(value) = rest.strip_prefix('\u{a0}') {
                *text = value.to_owned();
                start = first;
            } else if rest.is_empty() && space_after {
                start = after_spaces(&items, first + 1);
            }
        }
    }
    if biblatex {
        // Biblatex sets the leading locators itself, so their emphasis goes.
        for piece in items.iter_mut().skip(start) {
            match piece {
                SuffixPiece::Text(text) if text == "," => {}
                SuffixPiece::Inline(Inline::Emph(inner) | Inline::Strong(inner))
                    if randoc_core::shared::stringify(inner)
                        .chars()
                        .any(|c| c.is_ascii_digit()) =>
                {
                    *piece = SuffixPiece::Unwrapped(inner);
                }
                other if is_space(other) => {}
                _ => break,
            }
        }
    }
    items.drain(..start);
    items
}

/// Collects the inlines with each level of `Emph` and `Strong` removed.
fn without_emphasis<'i>(
    inlines: &'i [Inline],
    out: &mut Vec<&'i Inline>,
    budget: usize,
) -> Result<()> {
    let budget = budget.checked_sub(1).ok_or_else(crate::too_deep)?;
    for inline in inlines {
        match inline {
            Inline::Emph(inner) | Inline::Strong(inner) => without_emphasis(inner, out, budget)?,
            other => out.push(other),
        }
    }
    Ok(())
}

/// The index of the first piece at or after `from` that is not a space.
fn after_spaces(items: &[SuffixPiece<'_>], from: usize) -> usize {
    let rest = items.get(from..).unwrap_or_default();
    from + rest.iter().take_while(|piece| is_space(piece)).count()
}

/// Returns `true` when inlines hold code, down to `depth` levels.
fn contains_code(inlines: &[Inline], depth: usize) -> bool {
    inlines_any(
        inlines,
        depth,
        &|inline| matches!(inline, Inline::Code(..)),
        &|block| matches!(block, Block::CodeBlock(..)),
    )
}

/// `true` for a note that is not one paragraph.
fn is_complex_note(blocks: &[Block]) -> bool {
    !matches!(blocks, [] | [Block::Plain(_) | Block::Para(_)])
}

/// Visits an emphasis command and the emphasis commands in it. Each one that
/// holds a note that is not one paragraph goes into `split`. Returns `true`
/// when `inline` goes into `split`.
fn mark_split_emphasis(
    inline: &Inline,
    budget: usize,
    seen: &mut HashSet<usize>,
    split: &mut HashSet<usize>,
) -> bool {
    let (Inline::Emph(inlines)
    | Inline::Strong(inlines)
    | Inline::Underline(inlines)
    | Inline::Strikeout(inlines)) = inline
    else {
        return matches!(inline, Inline::Note(blocks) if is_complex_note(blocks));
    };
    let key = std::ptr::from_ref(inline) as usize;
    seen.insert(key);
    let Some(budget) = budget.checked_sub(1) else {
        return false;
    };
    let mut holds = false;
    for child in inlines {
        holds |= mark_split_emphasis(child, budget, seen, split);
    }
    if holds {
        split.insert(key);
    }
    holds
}

/// Notes, images and spans with an identifier cannot be in a PDF bookmark.
pub(crate) fn is_invalid_in_bookmark(inline: &Inline) -> bool {
    match inline {
        Inline::Note(_) | Inline::Image(..) => true,
        Inline::Span(attr, _) => !attr.id.is_empty(),
        _ => false,
    }
}

/// Math that ends in a comment needs a newline before the closing delimiter.
fn handle_math_comment(math: &str) -> String {
    let last_line = math.rsplit('\n').next().unwrap_or(math);
    let mut previous = ' ';
    let mut comment = false;
    for c in last_line.chars() {
        if c == '%' && previous != '\\' {
            comment = true;
            break;
        }
        previous = c;
    }
    if comment {
        format!("{math}\n")
    } else {
        math.to_owned()
    }
}

/// Percent-encodes the characters of an image path that Pandoc encodes.
fn encode_image_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if "[]^`{|}".contains(c) {
            out.push_str(&format!("%{:02X}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    out
}

fn listings_inline(attr: &Attr, code: &str) -> String {
    let delimiter = "!\"'()*,-./:;?@_|~"
        .chars()
        .find(|c| !code.contains(*c))
        .unwrap_or('!');
    let mut escaped = String::with_capacity(code.len());
    for c in code.chars() {
        if "\\{}%~_&#^".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    let language = crate::blocks::listings_language(attr)
        .map(|language| format!("[language={language}]"))
        .unwrap_or_default();
    format!("\\passthrough{{\\lstinline{language}{delimiter}{escaped}{delimiter}}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_comments_get_a_newline() {
        assert_eq!(handle_math_comment("x % c"), "x % c\n");
        assert_eq!(handle_math_comment("x \\% c"), "x \\% c");
        assert_eq!(handle_math_comment("a % b\nc"), "a % b\nc");
    }

    #[test]
    fn citation_suffixes_lose_a_leading_comma_and_page_labels() {
        let text = |t: &str| SuffixPiece::Text(t.into());
        let suffix = vec![
            Inline::Str(",".into()),
            Inline::Space,
            Inline::Str("p.\u{a0}3".into()),
        ];
        assert_eq!(clean_suffix(&suffix, false), vec![text("p.\u{a0}3")]);
        assert_eq!(clean_suffix(&suffix, true), vec![text("3")]);
        let spaced = vec![Inline::Space, Inline::Str("{p.}".into())];
        assert_eq!(
            clean_suffix(&spaced, false),
            vec![SuffixPiece::Inline(&Inline::Space), text("p.")]
        );
        let labelled = vec![
            Inline::Space,
            Inline::Str("page".into()),
            Inline::Space,
            Inline::Str("3".into()),
        ];
        assert_eq!(clean_suffix(&labelled, true), vec![text("3")]);
        let comma_word = vec![Inline::Str(",x".into())];
        assert_eq!(clean_suffix(&comma_word, false), vec![text("x")]);
    }
}
