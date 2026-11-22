use randoc_ast::{Inline, MathType, QuoteType};
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

    fn inline_run(&mut self, inlines: &[Inline], styles: u8, depth: usize) -> Result<Doc> {
        self.check_depth(depth)?;
        let mut out = Doc::default();
        let mut run = Doc::default();
        for inline in inlines {
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
            if let Some((flag, items)) = formatted {
                out += self.styled_run(styles, std::mem::take(&mut run));
                out += self.inline_run(items, styles | flag, depth + 1)?;
            } else if matches!(inline, Inline::Link(..)) {
                out += self.styled_run(styles, std::mem::take(&mut run));
                out += self.inline(inline, styles, depth)?;
            } else {
                run += self.inline(inline, 0, depth)?;
            }
        }
        Ok(out + self.styled_run(styles, run))
    }

    fn styled_run(&mut self, styles: u8, run: Doc) -> Doc {
        if run.is_empty() || styles == 0 {
            return run;
        }
        let builtin = match styles {
            EMPH => "Emphasis",
            STRONG => "Strong_20_Emphasis",
            STRIKE => "Strikeout",
            SUPER => "Superscript",
            SUB => "Subscript",
            _ => return span(&self.text_style(styles), run),
        };
        span(builtin, run)
    }

    fn text_style(&mut self, styles: u8) -> String {
        let next = self.text_styles.len() + 1;
        let index = *self.text_style_index.entry(styles).or_insert(next);
        let name = format!("T{index}");
        if index == next {
            let mut properties = String::new();
            for (flag, attrs) in [
                (EMPH, " fo:font-style=\"italic\""),
                (SMALL, " fo:font-variant=\"small-caps\""),
                (STRONG, " fo:font-weight=\"bold\""),
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
                if styles & flag != 0 {
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
                text(start) + self.inlines(inlines, depth + 1)? + text(end)
            }
            Inline::Cite(_, inlines) => self.inlines(inlines, depth + 1)?,
            Inline::RawInline(format, raw) if format.0 == "opendocument" => text(raw),
            Inline::RawInline(..) => Doc::default(),
            Inline::Span(attr, inlines) => {
                let mut content = self.inlines(inlines, depth + 1)?;
                if let Some(style) = attr.get("custom-style") {
                    self.custom_styles.insert((style.into(), "text"));
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
                    let format = match (kind, numbered) {
                        ("bookmark", false) => "text",
                        ("bookmark", true) => "number",
                        (_, false) => "caption",
                        (_, true) => "value",
                    };
                    let attrs = attribute("text:reference-format", format)
                        + &attribute("text:ref-name", id);
                    let content = if numbered {
                        Doc::default()
                    } else {
                        self.inline_run(inlines, styles, depth + 1)?
                    };
                    tag(&format!("text:{kind}-ref"), &attrs, content)
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
                let content = self.blocks(blocks, Some("Footnote"), depth + 1, false)?;
                let number = id + 1;
                tag(
                    "text:note",
                    &format!(" text:id=\"ftn{id}\" text:note-class=\"footnote\""),
                    text(format!("<text:note-citation>{number}</text:note-citation>"))
                        + tag("text:note-body", "", content),
                )
            }
            Inline::Math(kind, tex) => self.math(*kind, tex, depth)?,
            Inline::Emph(_)
            | Inline::Underline(_)
            | Inline::Strong(_)
            | Inline::Strikeout(_)
            | Inline::Superscript(_)
            | Inline::Subscript(_)
            | Inline::SmallCaps(_) => Doc::default(),
        })
    }

    fn math(&mut self, kind: MathType, tex: &str, depth: usize) -> Result<Doc> {
        let max_depth = self.context.limits.max_nesting_depth;
        if self.format != Format::Fragment {
            match randoc_math::to_mathml(kind, tex, max_depth) {
                Ok(math) => return self.formula(kind, math),
                Err(error) => self.context.report(LogMessage::CouldNotConvertTexMath(
                    tex.into(),
                    error.detail(tex),
                )),
            }
            return Ok(literal_math(kind, tex));
        }
        match randoc_math::to_inlines(kind, tex, max_depth) {
            Ok(inlines) => self.inlines(&inlines, depth + 1),
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
