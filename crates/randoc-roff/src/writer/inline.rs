use randoc_ast::{Inline, MathType, QuoteType, Target};
use randoc_core::shared::stringify;
use randoc_core::{LogMessage, Result, WrapOption};
use randoc_layout::{Doc, after_break, cr, literal, space};

use super::{Format, Lead, Renderer, protect, render_parts};

#[derive(Clone, Copy)]
pub(super) struct Font(u8);

impl Font {
    pub const ROMAN: Self = Self(0);
    pub const BOLD: Self = Self(1);
    pub const ITALIC: Self = Self(2);
    pub const CODE: Self = Self(4);

    fn with(self, style: Self) -> Self {
        Self(self.0 | style.0)
    }

    fn name(self) -> &'static str {
        ["R", "B", "I", "BI", "CR", "CB", "CI", "CBI"][usize::from(self.0)]
    }

    fn switch(self) -> Doc {
        literal(format!("\\f[{}]", self.name()))
    }
}

impl<'doc> Renderer<'_, 'doc> {
    pub(super) fn escape(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for ch in text.chars() {
            let escaped = match ch {
                '\\' => "\\(rs",
                '\'' => "\\(aq",
                '"' => "\\(dq",
                '‘' => "\\(oq",
                '’' => "\\(cq",
                '“' => "\\(lq",
                '”' => "\\(rq",
                '–' => "\\(en",
                '—' => "\\(em",
                '…' => "\\&...",
                '`' => "\\(ga",
                '~' => "\\(ti",
                '^' => "\\(ha",
                '\u{a0}' => "\\ ",
                '-' if self.format == Format::Man => "\\-",
                'ä' if self.options.prefer_ascii => "\\(:a",
                'é' if self.options.prefer_ascii => "\\('e",
                'ı' if self.options.prefer_ascii => "\\(.i",
                'å' if self.options.prefer_ascii => "\\(oa",
                'ö' if self.options.prefer_ascii => "\\(:o",
                'ü' if self.options.prefer_ascii => "\\(:u",
                'ß' if self.options.prefer_ascii => "\\(ss",
                'α' if self.options.prefer_ascii => "\\(*a",
                '@' => "\\(at",
                _ => {
                    out.push(ch);
                    continue;
                }
            };
            out.push_str(escaped);
        }
        out
    }

    pub(super) fn paragraph(&mut self, text: &'doc [Inline], level: usize) -> Result<String> {
        self.render_inlines(text, Font::ROMAN, level, true)
    }

    pub(super) fn render_inlines(
        &mut self,
        text: &'doc [Inline],
        font: Font,
        level: usize,
        wrap: bool,
    ) -> Result<String> {
        let mut parts = Vec::new();
        self.inlines(text, font, level, wrap, &mut parts)?;
        Ok(render_parts(parts, if wrap { self.width() } else { None }))
    }

    fn inlines(
        &mut self,
        text: &'doc [Inline],
        font: Font,
        level: usize,
        wrap: bool,
        out: &mut Vec<Doc>,
    ) -> Result<()> {
        self.check_depth(level)?;
        let mut after_link = false;
        let mut after_display = false;
        let mut terminal: Option<&str> = None;
        let mut iter = text.iter().peekable();
        while let Some(inline) = iter.next() {
            let suffix = after_link;
            let display = after_display;
            after_link = false;
            after_display = false;
            match inline {
                Inline::Str(text) => {
                    if display {
                        out.push(space());
                    }
                    if self.format == Format::Ms && suffix {
                        out.push(literal("\\&"));
                    }
                    if text.starts_with('.') && !(self.format == Format::Ms && suffix) {
                        out.push(after_break("\\&"));
                    }
                    let escaped = if self.small_caps {
                        self.small_caps_text(text)
                    } else {
                        self.escape(text)
                    };
                    out.push(literal(escaped));
                    terminal = Some(text);
                    continue;
                }
                Inline::Space | Inline::SoftBreak => {
                    if suffix {
                        out.push(literal(if self.format == Format::Man {
                            "\\ "
                        } else {
                            "\\& "
                        }));
                    } else if display {
                        if self.format == Format::Man {
                            out.push(literal(" "));
                        }
                    } else if self.format == Format::Ms
                        && self.options.wrap_text == WrapOption::WrapPreserve
                        && matches!(inline, Inline::SoftBreak)
                        || wrap && !self.csl_entry && terminal.is_some_and(sentence)
                    {
                        out.push(cr());
                    } else if self.format == Format::Man
                        && matches!(iter.peek(), Some(Inline::RawInline(format, _)) if format != "man")
                    {
                        out.push(literal(" "));
                    } else {
                        out.push(space());
                    }
                }
                Inline::LineBreak => {
                    out.extend([cr(), literal(self.hard_break()), cr()]);
                }
                Inline::Emph(value) | Inline::Underline(value) => {
                    self.with_font(value, font, Font::ITALIC, level, wrap, out)?;
                }
                Inline::Strong(value) => {
                    self.with_font(value, font, Font::BOLD, level, wrap, out)?
                }
                Inline::Code(attr, value) => {
                    let code = (self.format == Format::Ms)
                        .then(|| self.highlighted(attr, value))
                        .flatten()
                        .unwrap_or_else(|| self.escape(value));
                    out.extend([font.with(Font::CODE).switch(), literal(code), font.switch()]);
                }
                Inline::SmallCaps(value) if self.format == Format::Ms => {
                    let was_small_caps = std::mem::replace(&mut self.small_caps, true);
                    let result = self.inlines(value, font, level + 1, wrap, out);
                    self.small_caps = was_small_caps;
                    result?;
                }
                Inline::SmallCaps(value) | Inline::Span(_, value) | Inline::Cite(_, value) => {
                    self.inlines(value, font, level + 1, wrap, out)?;
                }
                Inline::Quoted(quote, value) => {
                    let (open, close) = if *quote == QuoteType::SingleQuote {
                        ("`", "'")
                    } else {
                        ("\\(lq", "\\(rq")
                    };
                    out.push(literal(open));
                    self.inlines(value, font, level + 1, wrap, out)?;
                    out.push(literal(close));
                }
                Inline::Strikeout(value) => {
                    let (open, close) = if self.format == Format::Man {
                        ("[STRIKEOUT:", "]")
                    } else {
                        ("\\m[strikecolor]", "\\m[]")
                    };
                    out.push(literal(open));
                    self.inlines(value, font, level + 1, wrap, out)?;
                    out.push(literal(close));
                }
                Inline::Superscript(value) | Inline::Subscript(value) => {
                    let (open, close) =
                        script(self.format, matches!(inline, Inline::Superscript(_)));
                    out.push(literal(open));
                    self.inlines(value, font, level + 1, wrap, out)?;
                    out.push(literal(close));
                }
                Inline::Math(kind, tex) => {
                    if self.format == Format::Ms {
                        match randoc_math::to_eqn(*kind, tex, self.limit) {
                            Ok(eq) if *kind == MathType::DisplayMath => {
                                out.extend([cr(), literal(format!(".EQ\n{eq}\n.EN")), cr()]);
                                after_display = true;
                            }
                            Ok(eq) => {
                                self.inline_math = true;
                                out.push(literal(format!("@{eq}@")));
                            }
                            Err(err) => {
                                self.ctx.report(LogMessage::CouldNotConvertTexMath(
                                    tex.clone(),
                                    err.detail(tex),
                                ));
                                out.push(literal(self.escape(&format!("${tex}$"))));
                            }
                        }
                    } else {
                        let eq = match randoc_math::to_inlines(*kind, tex, self.limit) {
                            Ok(eq) => simple_math(self, &eq, font),
                            Err(err) => {
                                self.ctx.report(LogMessage::CouldNotConvertTexMath(
                                    tex.clone(),
                                    err.detail(tex),
                                ));
                                let delimiter = if *kind == MathType::DisplayMath {
                                    "$$"
                                } else {
                                    "$"
                                };
                                self.escape(&format!("{delimiter}{tex}{delimiter}"))
                            }
                        };
                        if *kind == MathType::DisplayMath {
                            out.extend([cr(), literal(format!(".RS\n{eq}\n.RE"))]);
                            after_display = true;
                        } else {
                            out.push(literal(eq));
                        }
                    }
                }
                Inline::RawInline(format, value) => {
                    if format == self.format.name() {
                        out.push(literal(value));
                    }
                }
                Inline::Link(_, label, Target { url, .. }) => {
                    if self.format == Format::Man && super::uri::is_uri(url) {
                        let (start, end, target) = man_link(url);
                        let mut request = format!("\\c\n{start}\n");
                        if stringify(label) != target && !label.is_empty() {
                            request.push_str(&self.render_inlines(
                                label,
                                font,
                                level + 1,
                                false,
                            )?);
                            request.push('\n');
                        }
                        request.push_str(end);
                        request.push_str(" \\c");
                        out.extend([literal(request), cr()]);
                        after_link = true;
                    } else if self.pdf {
                        let (kind, target) = url
                            .strip_prefix('#')
                            .map_or(("W", url.as_str()), |id| ("L", id));
                        let target = target.replace('@', "%40");
                        let label = self.render_inlines(label, font, level + 1, false)?;
                        out.extend([literal(format!("\\c\n.pdfhref {kind} -D \"{target}\" -A \"\\c\" \\\n -- \"{label}\"")), cr()]);
                        after_link = true;
                    } else {
                        self.inlines(label, font, level + 1, wrap, out)?;
                    }
                }
                Inline::Image(attr, label, Target { url: src, .. }) => {
                    let mut text = self.render_inlines(label, font, level + 1, false)?;
                    if self.format == Format::Man {
                        if text.is_empty() {
                            text = "image".into();
                        }
                        let image = if super::uri::is_uri(src) {
                            let (start, end, _) = man_link(src);
                            format!("[IMAGE: \\c\n{start}\n{text}\n{end} \\c\n]")
                        } else {
                            format!("[IMAGE: {text}]")
                        };
                        out.push(literal(image));
                    } else {
                        let width = attr
                            .get("width")
                            .and_then(|width| image_points(width, self.options.dpi))
                            .map_or_else(String::new, |pts| format!(" \"{pts:.0}p\""));
                        out.extend([
                            cr(),
                            literal(format!(
                                "[IMAGE: {text}] \\\" \"{}\"{width}",
                                self.escape(src)
                            )),
                            cr(),
                        ]);
                        if iter
                            .peek()
                            .is_some_and(|item| matches!(item, Inline::SoftBreak | Inline::Space))
                        {
                            iter.next();
                        }
                    }
                }
                Inline::Note(blocks) => {
                    if self.format == Format::Man {
                        self.notes.push(blocks);
                        out.push(literal(format!("[{}]", self.notes.len())));
                    } else {
                        out.push(literal("\\**"));
                        if let Some(Inline::Str(val)) = iter.peek()
                            && val.chars().all(|ch| {
                                matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']')
                            })
                        {
                            out.push(literal(self.escape(val)));
                            iter.next();
                        }
                        let mut note = String::new();
                        self.blocks(blocks, &mut note, Lead::Bare, level + 1)?;
                        super::line(&mut note, ".FE");
                        out.extend([cr(), literal(format!(".FS\n{note}")), cr()]);
                        if iter
                            .peek()
                            .is_some_and(|item| matches!(item, Inline::SoftBreak | Inline::Space))
                        {
                            iter.next();
                        }
                    }
                }
            }
            if suffix
                && !matches!(inline, Inline::Space | Inline::SoftBreak | Inline::Str(_))
                && self.format == Format::Ms
                && !after_link
            {
                out.push(literal("\\&"));
            }
            terminal = None;
        }
        if after_link && self.format == Format::Ms {
            out.push(literal("\\&"));
        }
        Ok(())
    }

    fn with_font(
        &mut self,
        content: &'doc [Inline],
        enclosing: Font,
        style: Font,
        level: usize,
        wrap: bool,
        out: &mut Vec<Doc>,
    ) -> Result<()> {
        let font = enclosing.with(style);
        if font.0 != enclosing.0 {
            out.push(font.switch());
        }
        self.inlines(content, font, level + 1, wrap, out)?;
        if font.0 != enclosing.0 {
            out.push(enclosing.switch());
        }
        Ok(())
    }

    fn small_caps_text(&self, text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while !rest.is_empty() {
            let lower = rest.starts_with(char::is_lowercase);
            let end = rest
                .find(|ch: char| ch.is_lowercase() != lower)
                .unwrap_or(rest.len());
            let (run, tail) = rest.split_at(end);
            if lower {
                out.push_str("\\s-2");
                out.push_str(&self.escape(&run.to_uppercase()));
                out.push_str("\\s0");
            } else {
                out.push_str(&self.escape(run));
            }
            rest = tail;
        }
        out
    }

    pub(super) fn plain_code(&self, input: &str) -> String {
        input
            .split('\n')
            .map(|line| protect(&self.escape(line)))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn sentence(text: &str) -> bool {
    let text = text.trim_end_matches(['"', '\'', '”', '’', ')', ']']);
    text.ends_with(['.', '?', '!', '…'])
        && !(text.len() == 2 && text.as_bytes()[0].is_ascii_uppercase())
        && !matches!(
            text,
            "Mr." | "Mrs." | "Dr." | "Ms." | "e.g." | "i.e." | "vs."
        )
}

fn script(format: Format, sup: bool) -> (&'static str, &'static str) {
    match (format, sup) {
        (Format::Man, true) => ("^", "^"),
        (Format::Man, false) => ("~", "~"),
        (Format::Ms, true) => ("\\*{", "\\*}"),
        (Format::Ms, false) => ("\\*<", "\\*>"),
    }
}

fn simple_math(renderer: &Renderer<'_, '_>, inlines: &[Inline], font: Font) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Emph(contents) | Inline::Strong(contents) => {
                let style = if matches!(inline, Inline::Emph(_)) {
                    Font::ITALIC
                } else {
                    Font::BOLD
                };
                let next = font.with(style);
                out.push_str(&format!("\\f[{}]", next.name()));
                out.push_str(&simple_math(renderer, contents, next));
                out.push_str(&format!("\\f[{}]", font.name()));
            }
            Inline::Subscript(contents) | Inline::Superscript(contents) => {
                let (open, close) =
                    script(renderer.format, matches!(inline, Inline::Superscript(_)));
                out.push_str(open);
                out.push_str(&simple_math(renderer, contents, font));
                out.push_str(close);
            }
            Inline::Space => out.push(' '),
            Inline::Str(text) => out.push_str(&renderer.escape(text)),
            _ => {}
        }
    }
    out
}

fn image_points(width: &str, dpi: i64) -> Option<f64> {
    let end = width
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(width.len());
    let (number, unit) = width.split_at(end);
    let number: f64 = number.parse().ok()?;
    let scale = match unit.trim_start() {
        "" | "px" => 72.0 / dpi.max(1) as f64,
        "cm" => 72.0 / 2.54,
        "mm" => 72.0 / 25.4,
        "in" | "inch" => 72.0,
        "pt" => 1.0,
        "pc" => 12.0,
        "em" => 12.0,
        "%" => 0.0,
        _ => return None,
    };
    let points = number * scale;
    points.is_finite().then_some(points)
}

/// Returns the start request with its target, the end request and the
/// target of a man link. An e-mail address uses `.MT`, and an empty target
/// leaves no space after the request name.
fn man_link(url: &str) -> (String, &'static str, &str) {
    let (start, end, target) = match url.strip_prefix("mailto:") {
        Some(address) => (".MT", ".ME", address),
        None => (".UR", ".UE", url),
    };
    let start = if target.is_empty() {
        start.to_owned()
    } else {
        format!("{start} {target}")
    };
    (start, end, target)
}
