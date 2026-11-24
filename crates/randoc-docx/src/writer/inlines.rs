use std::fmt::Write;

use randoc_ast::{Attr, Inline, MathType, QuoteType, Target};
use randoc_core::{LogMessage, Result};

use super::styles::{Kind, Styles};
use super::xml::{Xml, bookmark_end, size_error};
use super::{Renderer, bookmark_name};

#[derive(Clone, Copy, Default)]
pub(super) struct Run<'a> {
    style: Option<&'a str>,
    lang: Option<&'a str>,
    rtl: bool,
    marks: u8,
    bold: usize,
    italic: usize,
    vertical: Option<&'static str>,
}

const UNDERLINE: u8 = 1;
const STRIKE: u8 = 2;
const SMALL_CAPS: u8 = 4;
const MARK: u8 = 8;

impl<'a> Run<'a> {
    pub(super) fn styled<'b>(self, name: &'b str) -> Run<'b>
    where
        'a: 'b,
    {
        Run {
            style: Some(name),
            ..self
        }
    }

    pub(super) fn with_language(self, lang: Option<&'a str>) -> Self {
        Self { lang, ..self }
    }

    pub(super) fn with_attr(self, attr: &'a Attr) -> Self {
        Self {
            rtl: attr.get("dir").map_or(self.rtl, |dir| dir == "rtl"),
            lang: attr.get("lang").or(self.lang),
            ..self
        }
    }

    fn start(self, out: &mut Xml, east_asia: bool) {
        out.raw("<w:r>");
        if self.style.is_none()
            && self.lang.is_none()
            && !self.rtl
            && !east_asia
            && self.marks == 0
            && self.bold == 0
            && self.italic == 0
            && self.vertical.is_none()
        {
            return;
        }
        out.raw("<w:rPr>");
        if let Some(style) = self.style {
            out.raw(r#"<w:rStyle w:val=""#);
            out.escaped(style);
            out.raw("\" />");
        }
        if east_asia {
            out.raw(r#"<w:rFonts w:hint="eastAsia" />"#);
        }
        for _ in 0..self.bold {
            out.raw("<w:b />");
        }
        for _ in 0..self.bold {
            out.raw("<w:bCs />");
        }
        for _ in 0..self.italic {
            out.raw("<w:i />");
        }
        for _ in 0..self.italic {
            out.raw("<w:iCs />");
        }
        if self.marks & SMALL_CAPS != 0 {
            out.raw("<w:smallCaps />");
        }
        if self.marks & STRIKE != 0 {
            out.raw("<w:strike />");
        }
        if self.marks & MARK != 0 {
            out.raw(r#"<w:highlight w:val="yellow" />"#);
        }
        if self.marks & UNDERLINE != 0 {
            out.raw(r#"<w:u w:val="single" />"#);
        }
        if let Some(vertical) = self.vertical {
            let _ = write!(out, "<w:vertAlign w:val=\"{vertical}\" />");
        }
        if self.rtl {
            out.raw("<w:rtl />");
        }
        if let Some(lang) = self.lang {
            out.raw(r#"<w:lang w:val=""#);
            out.escaped(lang);
            out.raw("\" />");
        }
        out.raw("</w:rPr>");
    }

    pub(super) fn markup(self, out: &mut Xml, xml: &str) {
        self.start(out, false);
        out.raw(xml);
        out.raw("</w:r>");
    }

    /// Writes text in runs as Pandoc does: a word with an East Asian
    /// character gets its own run with the East Asian font hint; the other
    /// words and the white space between words share runs.
    pub(super) fn text(self, out: &mut Xml, value: &str) {
        let mut start = 0;
        let mut index = 0;
        while index < value.len() {
            let rest = &value[index..];
            let end = rest
                .char_indices()
                .find(|(_, ch)| is_space(*ch) != rest.starts_with(is_space))
                .map_or(value.len(), |(offset, _)| index + offset);
            if value[index..end].chars().any(is_east_asian) {
                if start < index {
                    self.text_run(out, &value[start..index], false);
                }
                self.text_run(out, &value[index..end], true);
                start = end;
            }
            index = end;
        }
        if start < value.len() || value.is_empty() {
            self.text_run(out, &value[start..], false);
        }
    }

    fn text_run(self, out: &mut Xml, value: &str, east_asia: bool) {
        self.start(out, east_asia);
        out.raw(r#"<w:t xml:space="preserve">"#);
        out.escaped(value);
        out.raw("</w:t></w:r>");
    }
}

impl Renderer<'_> {
    pub(super) fn inlines(
        &mut self,
        out: &mut Xml,
        inlines: &[Inline],
        run: Run<'_>,
        depth: usize,
    ) -> Result<()> {
        let pieces: Vec<_> = inlines.iter().map(Piece::Inline).collect();
        self.pieces(out, &pieces, run, depth)
    }

    /// Writes inlines as Pandoc does: `Str Str` and `Str Space Str` join
    /// into one run, and each other inline gives its own runs.
    pub(super) fn pieces(
        &mut self,
        out: &mut Xml,
        pieces: &[Piece<'_>],
        run: Run<'_>,
        depth: usize,
    ) -> Result<()> {
        if !pieces.is_empty() {
            self.depth(depth)?;
        }
        let mut rest = pieces;
        while let Some((piece, tail)) = rest.split_first() {
            if piece.text().is_some() {
                let mut count = 1;
                loop {
                    match rest.get(count..) {
                        Some([next, ..]) if next.text().is_some() => count += 1,
                        Some([Piece::Inline(Inline::Space), next, ..]) if next.text().is_some() => {
                            count += 2;
                        }
                        _ => break,
                    }
                }
                let limit = self.budget.remaining();
                let mut value = String::new();
                for piece in &rest[..count] {
                    let text = piece.text().unwrap_or(" ");
                    if text.len() > limit.saturating_sub(value.len()) {
                        return Err(size_error());
                    }
                    value.push_str(text);
                }
                if !value.is_empty() {
                    run.text(out, &value);
                }
                rest = &rest[count..];
            } else {
                if let Piece::Inline(inline) = piece {
                    self.inline(out, inline, run, depth + 1)?;
                    if matches!(inline, Inline::Span(attr, _) if attr.classes == ["csl-left-margin"])
                        && matches!(tail.first(), Some(Piece::Inline(Inline::Span(attr, _)))
                            if attr.classes == ["csl-right-inline"])
                    {
                        run.text(out, "\t");
                    }
                }
                rest = tail;
            }
            self.budget.check()?;
        }
        Ok(())
    }

    fn inline(
        &mut self,
        out: &mut Xml,
        inline: &Inline,
        mut run: Run<'_>,
        depth: usize,
    ) -> Result<()> {
        match inline {
            Inline::Str(value) => run.text(out, value),
            Inline::Space | Inline::SoftBreak => run.text(out, " "),
            Inline::LineBreak => run.markup(out, "<w:br />"),
            Inline::RawInline(format, xml) => {
                if format.0 == "openxml" {
                    out.raw(xml);
                }
            }
            Inline::Emph(children) => {
                run.italic += 1;
                return self.inlines(out, children, run, depth);
            }
            Inline::Strong(children) => {
                run.bold += 1;
                return self.inlines(out, children, run, depth);
            }
            Inline::Underline(children) => {
                run.marks |= UNDERLINE;
                return self.inlines(out, children, run, depth);
            }
            Inline::Strikeout(children) => {
                run.marks |= STRIKE;
                return self.inlines(out, children, run, depth);
            }
            Inline::SmallCaps(children) => {
                run.marks |= SMALL_CAPS;
                return self.inlines(out, children, run, depth);
            }
            Inline::Superscript(children) => {
                run.vertical = Some("superscript");
                return self.inlines(out, children, run, depth);
            }
            Inline::Subscript(children) => {
                run.vertical = Some("subscript");
                return self.inlines(out, children, run, depth);
            }
            Inline::Quoted(kind, children) => {
                let (left, right) = match kind {
                    QuoteType::SingleQuote => ("‘", "’"),
                    QuoteType::DoubleQuote => ("“", "”"),
                };
                let mut pieces = vec![Piece::Text(left)];
                pieces.extend(children.iter().map(Piece::Inline));
                pieces.push(Piece::Text(right));
                return self.pieces(out, &pieces, run, depth);
            }
            Inline::Cite(_, children) => return self.inlines(out, children, run, depth),
            Inline::Span(attr, children) => return self.span(out, attr, children, run, depth),
            Inline::Code(attr, code) => {
                let bookmark = self.bookmark(out, &attr.id);
                self.code(out, attr, code, run.with_attr(attr))?;
                bookmark_end(out, bookmark);
            }
            Inline::Link(attr, children, target) => {
                return self.link(out, attr, children, target, run, depth);
            }
            Inline::Image(attr, children, target) => {
                return self.image(out, attr, children, target, run, depth);
            }
            Inline::Math(kind, tex) => self.math(out, *kind, tex, run, depth)?,
            Inline::Note(blocks) => return self.note(out, blocks, depth),
        }
        Ok(())
    }

    fn span(
        &mut self,
        out: &mut Xml,
        attr: &Attr,
        children: &[Inline],
        mut run: Run<'_>,
        depth: usize,
    ) -> Result<()> {
        if attr.has_class("comment-start") || attr.has_class("comment-end") {
            return self.comment(out, attr, children, run, depth);
        }
        let bookmark = self.bookmark(out, &attr.id);
        if attr.has_class("mark") {
            run.marks |= MARK;
        }
        let style = attr
            .get("custom-style")
            .map(|name| self.styles.custom(Kind::Character, name, &self.budget))
            .transpose()?;
        let run = run.with_attr(attr);
        self.inlines(
            out,
            children,
            style.as_deref().map_or(run, |style| run.styled(style)),
            depth,
        )?;
        bookmark_end(out, bookmark);
        Ok(())
    }

    fn link(
        &mut self,
        out: &mut Xml,
        attr: &Attr,
        children: &[Inline],
        target: &Target,
        run: Run<'_>,
        depth: usize,
    ) -> Result<()> {
        let bookmark = self.bookmark(out, &attr.id);
        out.raw("<w:hyperlink");
        if let Some(anchor) = target.url.strip_prefix('#') {
            let _ = write!(out, " w:anchor=\"{}\"", bookmark_name(anchor));
        } else {
            let id = if let Some(id) = self.links.get(&target.url) {
                *id
            } else {
                let id = self.id();
                self.relation(id, "hyperlink", &target.url, true)?;
                self.links.insert(target.url.clone(), id);
                id
            };
            let _ = write!(out, " r:id=\"rId{id}\"");
        }
        if !target.title.is_empty() {
            out.raw(" w:tooltip=\"");
            out.escaped(&target.title);
            out.raw("\"");
        }
        out.raw(">");
        let content_start = out.len();
        let style = self.styles.id(Kind::Character, "Hyperlink").to_owned();
        self.inlines(out, children, run.styled(&style).with_attr(attr), depth)?;
        out.close_element("w:hyperlink", content_start)?;
        bookmark_end(out, bookmark);
        Ok(())
    }

    pub(super) fn code(
        &mut self,
        out: &mut Xml,
        attr: &Attr,
        code: &str,
        run: Run<'_>,
    ) -> Result<()> {
        if code.len() > self.budget.remaining() {
            return Err(size_error());
        }
        if self.styles.highlighting()
            && let Some(result) = self.registry.highlight_classes(
                &attr.classes,
                code,
                self.context.limits.max_nesting_depth,
            )
        {
            match result {
                Ok(code) => {
                    for (index, line) in code.lines.iter().enumerate() {
                        if index > 0 {
                            Run::default().markup(out, "<w:br />");
                        }
                        for (token, text) in line {
                            let style = Styles::token(*token);
                            Run::default().styled(&style).text(out, text);
                        }
                        self.budget.check()?;
                    }
                    return Ok(());
                }
                Err(message) => self.context.report(LogMessage::CouldNotHighlight(message)),
            }
        }
        let run = run.styled(self.styles.id(Kind::Character, "VerbatimChar"));
        for (index, line) in code.split('\n').enumerate() {
            if index > 0 {
                Run::default().markup(out, "<w:br />");
            }
            run.text(out, line);
            self.budget.check()?;
        }
        Ok(())
    }

    fn math(
        &mut self,
        out: &mut Xml,
        kind: MathType,
        tex: &str,
        run: Run<'_>,
        depth: usize,
    ) -> Result<()> {
        if tex.len() > self.budget.remaining() {
            return Err(size_error());
        }
        let max_depth = self.context.limits.max_nesting_depth.saturating_sub(depth);
        match randoc_math::to_omml(kind, tex, max_depth) {
            Ok(xml) => {
                if kind == MathType::DisplayMath {
                    out.raw(
                        r#"<m:oMathPara><m:oMathParaPr><m:jc m:val="center" /></m:oMathParaPr>"#,
                    );
                }
                out.raw(&xml);
                if kind == MathType::DisplayMath {
                    out.raw("</m:oMathPara>");
                }
            }
            Err(error) => {
                self.context.report(LogMessage::CouldNotConvertTexMath(
                    tex.to_owned(),
                    error.detail(tex),
                ));
                let delimiter = if kind == MathType::DisplayMath {
                    "$$"
                } else {
                    "$"
                };
                run.text(out, &format!("{delimiter}{tex}{delimiter}"));
            }
        }
        Ok(())
    }
}

/// An inline, or text that Pandoc adds as a `Str`, such as a quotation mark.
pub(super) enum Piece<'a> {
    Inline(&'a Inline),
    Text(&'static str),
}

impl Piece<'_> {
    fn text(&self) -> Option<&str> {
        match self {
            Piece::Inline(Inline::Str(text)) => Some(text),
            Piece::Text(text) => Some(text),
            Piece::Inline(_) => None,
        }
    }
}

/// Returns `true` for the white space of Haskell's `isSpace`: the ASCII
/// spaces, the no-break space and the other space separators.
fn is_space(ch: char) -> bool {
    matches!(ch, ' ' | '\t'..='\r' | '\u{a0}')
        || (ch > '\u{ff}' && ch.is_whitespace() && !matches!(ch, '\u{2028}' | '\u{2029}'))
}

fn is_east_asian(ch: char) -> bool {
    matches!(
        ch,
        '\u{2E80}'..='\u{A4CF}'
            | '\u{AC00}'..='\u{D7AF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FF00}'..='\u{FFEE}'
            | '\u{20000}'..='\u{323AF}'
    )
}
