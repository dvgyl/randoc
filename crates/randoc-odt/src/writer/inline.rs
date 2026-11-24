use randoc_ast::{Attr, Inline, MathType, QuoteType};
use randoc_citeproc::parse_lang;
use randoc_core::{Extension, LogMessage, Result, WrapOption};
use randoc_layout::{Doc, cr, space, text};

use super::{Format, Renderer, attribute, bookmark, code_text, escaped, span, tag};

const EMPH: u8 = 1;
const STRONG: u8 = 2;
const STRIKE: u8 = 4;
const SUPER: u8 = 8;
const SUB: u8 = 16;
const SMALL: u8 = 32;
const UNDERLINE: u8 = 64;

impl Renderer<'_> {
    pub(super) fn inlines(&mut self, inlines: &[Inline], depth: usize) -> Result<Doc> {
        self.inline_run(inlines, 0, depth)
    }

    /// Renders inlines as Pandoc does: each run of words and spaces gets the
    /// text style of the enclosing formats and languages. The other inlines
    /// carry the style down to their own runs.
    fn inline_run(&mut self, inlines: &[Inline], styles: u8, depth: usize) -> Result<Doc> {
        self.check_depth(depth)?;
        let mut out = Doc::default();
        let mut run = Doc::default();
        for inline in inlines {
            if matches!(inline, Inline::Str(_) | Inline::Space | Inline::SoftBreak) {
                run += self.inline(inline, styles, depth)?;
                continue;
            }
            out += self.styled_run(styles, std::mem::take(&mut run));
            let formatted = match inline {
                Inline::Emph(items) => Some((EMPH, items)),
                Inline::Strong(items) => Some((STRONG, items)),
                Inline::Strikeout(items) => Some((STRIKE, items)),
                Inline::Superscript(items) => Some((SUPER, items)),
                Inline::Subscript(items) => Some((SUB, items)),
                Inline::SmallCaps(items) => Some((SMALL, items)),
                Inline::Underline(items) => Some((UNDERLINE, items)),
                _ => None,
            };
            out += match formatted {
                Some((flag, items)) => self.inline_run(items, styles | flag, depth + 1)?,
                None => self.inline(inline, styles, depth)?,
            };
        }
        Ok(out + self.styled_run(styles, run))
    }

    /// Renders `body` with the language of `attr` added to the text style.
    pub(super) fn with_language<T>(
        &mut self,
        attr: &Attr,
        body: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let Some(value) = attr.get("lang") else {
            return body(self);
        };
        let lang = match parse_lang(value) {
            Ok(lang) => lang,
            Err(_) => {
                self.context.report(LogMessage::InvalidLang(value.into()));
                return body(self);
            }
        };
        if !self.languages.insert(lang.clone()) {
            return body(self);
        }
        let result = body(self);
        self.languages.remove(&lang);
        result
    }

    fn styled_run(&mut self, styles: u8, run: Doc) -> Doc {
        if run.is_empty() || (styles == 0 && self.languages.is_empty()) {
            return run;
        }
        let builtin = match styles {
            _ if !self.languages.is_empty() => None,
            EMPH => Some("Emphasis"),
            STRONG => Some("Strong_20_Emphasis"),
            STRIKE => Some("Strikeout"),
            SUPER => Some("Superscript"),
            SUB => Some("Subscript"),
            _ => None,
        };
        match builtin {
            Some(name) => span(name, run),
            None => span(&self.text_style(styles), run),
        }
    }

    fn text_style(&mut self, styles: u8) -> String {
        let next = self.text_styles.len() + 1;
        let index = *self
            .text_style_index
            .entry((styles, self.languages.clone()))
            .or_insert(next);
        let name = format!("T{index}");
        if index == next {
            // Pandoc writes the properties in the order of their names.
            let lang = self.languages.last();
            let region = lang.and_then(|lang| lang.region.as_deref());
            let mut properties = String::new();
            if let Some(region) = region {
                properties.push_str(&attribute("fo:country", region));
            }
            for (flag, attrs) in [
                (EMPH, " fo:font-style=\"italic\""),
                (SMALL, " fo:font-variant=\"small-caps\""),
                (STRONG, " fo:font-weight=\"bold\""),
                (0, ""),
                (
                    EMPH,
                    " style:font-style-asian=\"italic\" style:font-style-complex=\"italic\"",
                ),
                (
                    STRONG,
                    " style:font-weight-asian=\"bold\" style:font-weight-complex=\"bold\"",
                ),
                (STRIKE, " style:text-line-through-style=\"solid\""),
                (SUPER, " style:text-position=\"super 58%\""),
                (SUB, " style:text-position=\"sub 58%\""),
                (
                    UNDERLINE,
                    " style:text-underline-color=\"font-color\" style:text-underline-style=\"solid\" style:text-underline-width=\"auto\"",
                ),
            ] {
                if flag == 0 {
                    if let Some(lang) = lang {
                        properties.push_str(&attribute("fo:language", &lang.language));
                    }
                } else if styles & flag != 0 {
                    properties.push_str(attrs);
                }
            }
            self.text_styles.push(format!("<style:style style:name=\"{name}\" style:family=\"text\"><style:text-properties{properties} /></style:style>"));
        }
        name
    }

    fn inline(&mut self, inline: &Inline, styles: u8, depth: usize) -> Result<Doc> {
        Ok(match inline {
            Inline::Str(value) => text(code_text(
                value,
                self.options.tab_stop.clamp(1, 1024) as usize,
                true,
            )),
            Inline::Space => space(),
            Inline::SoftBreak if self.options.wrap_text == WrapOption::WrapPreserve => cr(),
            Inline::SoftBreak => space(),
            Inline::LineBreak => text("<text:line-break />"),
            Inline::Code(_, value) => span(
                "Source_20_Text",
                text(code_text(
                    value,
                    self.options.tab_stop.clamp(1, 1024) as usize,
                    true,
                )),
            ),
            Inline::Quoted(kind, inlines) => {
                let (start, end) = match kind {
                    QuoteType::SingleQuote => ("‘", "’"),
                    QuoteType::DoubleQuote => ("“", "”"),
                };
                text(start) + self.inline_run(inlines, styles, depth + 1)? + text(end)
            }
            Inline::Cite(_, inlines) => self.inline_run(inlines, styles, depth + 1)?,
            Inline::RawInline(format, raw) if format.0 == "opendocument" => text(raw),
            Inline::RawInline(..) => Doc::default(),
            Inline::Span(attr, inlines) => {
                let mut content = self.with_language(attr, |renderer| {
                    renderer.inline_run(inlines, styles, depth + 1)
                })?;
                if let Some(style) = attr.get("custom-style") {
                    content = span(style, content);
                }
                bookmark(&attr.id, content)
            }
            Inline::Link(_, inlines, target) => {
                let numbered = self.has(Extension::XrefsNumber);
                let named = self.has(Extension::XrefsName);
                if let Some((id, kind)) = target
                    .url
                    .strip_prefix('#')
                    .filter(|_| numbered || named)
                    .and_then(|id| self.references.get(id).map(|kind| (id, *kind)))
                {
                    let (number_format, name_format) = if kind == "bookmark" {
                        ("number", "text")
                    } else {
                        ("value", "caption")
                    };
                    let element = format!("text:{kind}-ref");
                    let reference = |format, content| {
                        let attrs = attribute("text:reference-format", format)
                            + &attribute("text:ref-name", id);
                        tag(&element, &attrs, content)
                    };
                    let mut out = Doc::default();
                    if numbered {
                        out += reference(number_format, Doc::default());
                    }
                    if numbered && named {
                        out += text("<text:s />");
                    }
                    if named {
                        out += reference(name_format, self.inline_run(inlines, styles, depth + 1)?);
                    }
                    out
                } else {
                    let attrs = format!(
                        " xlink:type=\"simple\"{}{}",
                        attribute("xlink:href", &self.href(&target.url)),
                        attribute("office:name", &target.title)
                    );
                    tag(
                        "text:a",
                        &attrs,
                        span("Definition", self.inline_run(inlines, styles, depth + 1)?),
                    )
                }
            }
            Inline::Image(attr, alt, target) => self.image(attr, alt, target, depth)?,
            Inline::Note(blocks) => {
                let id = self.note_count;
                self.note_count += 1;
                // The text of a note starts with no text style.
                let languages = std::mem::take(&mut self.languages);
                let content = self.blocks(blocks, Some("Footnote"), depth + 1, false);
                self.languages = languages;
                let content = content?;
                let number = id + 1;
                tag(
                    "text:note",
                    &format!(" text:id=\"ftn{id}\" text:note-class=\"footnote\""),
                    text(format!("<text:note-citation>{number}</text:note-citation>"))
                        + tag("text:note-body", "", content),
                )
            }
            Inline::Math(kind, tex) => self.math(*kind, tex, styles, depth)?,
            Inline::Emph(_)
            | Inline::Underline(_)
            | Inline::Strong(_)
            | Inline::Strikeout(_)
            | Inline::Superscript(_)
            | Inline::Subscript(_)
            | Inline::SmallCaps(_) => Doc::default(),
        })
    }

    fn math(&mut self, kind: MathType, tex: &str, styles: u8, depth: usize) -> Result<Doc> {
        let max_depth = self.context.limits.max_nesting_depth;
        if self.format != Format::Fragment {
            // A lone percent sign in Markdown math delimiters is literal text.
            if kind == MathType::DisplayMath && tex == "%" {
                return Ok(literal_math(kind, tex));
            }
            match randoc_math::to_mathml_flavor(
                kind,
                tex,
                max_depth,
                randoc_math::MathMlFlavor::Odt,
            ) {
                Ok(math) => return self.formula(kind, math),
                Err(error) => self.context.report(LogMessage::CouldNotConvertTexMath(
                    tex.into(),
                    error.detail(tex),
                )),
            }
            return Ok(literal_math(kind, tex));
        }
        match randoc_math::to_inlines(kind, tex, max_depth) {
            Ok(inlines) => self.inline_run(&inlines, styles, depth + 1),
            Err(error) => {
                self.context.report(LogMessage::CouldNotConvertTexMath(
                    tex.into(),
                    error.detail(tex),
                ));
                Ok(literal_math(kind, tex))
            }
        }
    }
}

fn literal_math(kind: MathType, tex: &str) -> Doc {
    let delim = if kind == MathType::DisplayMath {
        "$$"
    } else {
        "$"
    };
    text(format!("{delim}{}{delim}", escaped(tex)))
}
