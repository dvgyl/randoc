use randoc_ast::{Attr, Block, Inline, MathType, QuoteType, Target};
use randoc_core::WrapOption;
use randoc_layout::{Doc, chomp, cr, empty, hcat, literal, space, text};

use super::{Dialect, Renderer};

impl Renderer<'_> {
    pub(super) fn inlines(&self, inlines: &[Inline], in_link: bool, in_table: bool) -> Doc {
        hcat(inlines.iter().enumerate().map(|(index, inline)| {
            let before = index.checked_sub(1).and_then(|i| inlines.get(i));
            let after = inlines.get(index + 1);
            let adjacent = before.is_some_and(|item| joins_markup(item, true))
                || after.is_some_and(|item| joins_markup(item, false));
            self.inline(inline, in_link, in_table, adjacent)
        }))
    }

    /// Places display math on separate lines within a paragraph.
    pub(super) fn paragraph(&self, mut inlines: &[Inline], in_table: bool) -> Doc {
        let mut out = empty();
        while let Some(index) = inlines
            .iter()
            .position(|node| matches!(node, Inline::Math(MathType::DisplayMath, _)))
        {
            out = out.above(chomp(self.inlines(&inlines[..index], false, in_table)));
            out = out.above_blank(self.inlines(&inlines[index..=index], false, in_table));
            inlines = &inlines[index + 1..];
            while matches!(inlines.first(), Some(Inline::Space | Inline::SoftBreak)) {
                inlines = &inlines[1..];
            }
        }
        out.above(self.inlines(inlines, false, in_table))
    }

    fn inline(&self, inline: &Inline, in_link: bool, in_table: bool, adjacent: bool) -> Doc {
        let wrap = |open: &str, body: &[Inline], close: &str| {
            hcat([
                literal(open),
                self.inlines(body, in_link, in_table),
                literal(close),
            ])
        };
        match inline {
            Inline::Str(value) => {
                let escaped = escape(value, in_link, in_table);
                let words = escaped.split(' ').enumerate().flat_map(|(index, word)| {
                    (index > 0)
                        .then(space)
                        .into_iter()
                        .chain(std::iter::once(text(word)))
                });
                hcat(words)
            }
            Inline::Space => space(),
            Inline::SoftBreak if self.options.wrap_text == WrapOption::WrapPreserve => cr(),
            Inline::SoftBreak => space(),
            Inline::LineBreak => hcat([literal(" +"), cr()]),
            Inline::Strong(body) => {
                let mark = if adjacent { "**" } else { "*" };
                wrap(mark, body, mark)
            }
            Inline::Emph(body) => {
                let mark = if adjacent { "__" } else { "_" };
                wrap(mark, body, mark)
            }
            Inline::Strikeout(body) => wrap("[line-through]#", body, "#"),
            Inline::Underline(body) => wrap("[.underline]#", body, "#"),
            Inline::Superscript(body) | Inline::Subscript(body) => {
                let mark = if matches!(inline, Inline::Superscript(_)) {
                    "^"
                } else {
                    "~"
                };
                let body = self
                    .inlines(body, in_link, in_table)
                    .render(None)
                    .replace(' ', "\u{a0}");
                literal(format!("{mark}{body}{mark}"))
            }
            Inline::SmallCaps(body) => wrap("[smallcaps]#", body, "#"),
            Inline::Cite(_, body) => self.inlines(body, in_link, in_table),
            Inline::Quoted(kind, body) => {
                let (open, close) = match (self.dialect == Dialect::Legacy, kind) {
                    (true, QuoteType::DoubleQuote) => ("``", "''"),
                    (true, QuoteType::SingleQuote) => ("`", "'"),
                    (false, QuoteType::DoubleQuote) => ("\"`", "`\""),
                    (false, QuoteType::SingleQuote) => ("'`", "`'"),
                };
                wrap(open, body, close)
            }
            Inline::Code(_, code) => {
                let value = if self.dialect == Dialect::Legacy {
                    code.replace('`', "\\'")
                } else {
                    escape(code, false, in_table)
                };
                literal(format!("`{value}`"))
            }
            Inline::Math(MathType::InlineMath, value) if self.dialect == Dialect::Legacy => {
                literal(format!("latexmath:[${value}$]"))
            }
            Inline::Math(MathType::InlineMath, value) => literal(format!("latexmath:[{value}]")),
            Inline::Math(MathType::DisplayMath, value) => {
                let value = if self.dialect == Dialect::Legacy {
                    format!("\\[{value}\\]")
                } else {
                    value.clone()
                };
                literal(format!("[latexmath]\n++++\n{value}\n++++"))
            }
            Inline::RawInline(format, value) if format.0.eq_ignore_ascii_case("asciidoc") => {
                literal(value)
            }
            Inline::RawInline(..) => empty(),
            Inline::Link(_, body, target) => self.link(body, target, in_table),
            Inline::Image(attr, body, target) => {
                self.image(attr, body, target, false, in_link, in_table)
            }
            Inline::Note(blocks) => match blocks.as_slice() {
                [Block::Para(body) | Block::Plain(body)] => wrap("footnote:[", body, "]"),
                [] => literal("footnote:[]"),
                _ => literal("[multiblock footnote omitted]"),
            },
            Inline::Span(attr, body) => {
                if attr.id.is_empty() && attr.classes.is_empty() {
                    return self.inlines(body, in_link, in_table);
                }
                if body.is_empty() && !attr.id.is_empty() {
                    return literal(format!("[[{}]]", attr.id));
                }
                let mut styles = Vec::new();
                if !attr.id.is_empty() {
                    styles.push(format!("#{}", attr.id));
                }
                styles.extend(attr.classes.iter().map(|class| format!(".{class}")));
                let mark = if adjacent { "##" } else { "#" };
                if attr.id.is_empty() && attr.classes == ["mark"] {
                    wrap(mark, body, mark)
                } else {
                    wrap(&format!("[{}]{mark}", styles.join(" ")), body, mark)
                }
            }
        }
    }

    fn link(&self, body: &[Inline], target: &Target, in_table: bool) -> Doc {
        let url = &target.url;
        let autolink = body.len() == 1
            && matches!(&body[0], Inline::Str(label)
            if label == url.strip_prefix("mailto:").unwrap_or(url));
        let explicit =
            url.contains("--") || url.contains("__") || url.contains('[') || url.contains(']');
        if autolink && !explicit {
            return literal(url.strip_prefix("mailto:").unwrap_or(url));
        }
        let prefix = if explicit {
            format!("link:++{url}++[")
        } else if ["http://", "https://", "ftp://", "mailto:"]
            .iter()
            .any(|prefix| url.starts_with(prefix))
        {
            format!("{url}[")
        } else {
            format!("link:{url}[")
        };
        hcat([
            literal(prefix),
            if autolink {
                empty()
            } else {
                self.inlines(body, true, in_table)
            },
            literal("]"),
        ])
    }

    pub(super) fn image(
        &self,
        attr: &Attr,
        body: &[Inline],
        target: &Target,
        figure: bool,
        in_link: bool,
        in_table: bool,
    ) -> Doc {
        let prefix = if figure { "image::" } else { "image:" };
        let mut options = Vec::new();
        if !target.title.is_empty() {
            options.push(format!("title=\"{}\"", target.title.replace('"', "&quot;")));
        }
        for key in ["width", "height"] {
            if let Some(value) = attr
                .get(key)
                .and_then(|value| dimension(key, value, self.options.dpi))
            {
                options.push(value);
            }
        }
        let alt = self.inlines(body, in_link, in_table);
        let options = if options.is_empty() {
            String::new()
        } else {
            format!(",{}", options.join(","))
        };
        literal(format!("{prefix}{}[", target.url)) + alt + literal(format!("{options}]"))
    }
}

fn dimension(key: &str, value: &str, dpi: i64) -> Option<String> {
    if !value.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let boundary = value
        .find(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(boundary);
    let number = number
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())?;
    let pixels = match unit.trim_start_matches(' ') {
        "%" => return Some(format!("scaled{key}={number:.1}%")),
        "" | "px" => number,
        "in" | "inch" => number * dpi as f64,
        "cm" => number * 0.393_700_787 * dpi as f64,
        "mm" => number * 0.039_370_078_7 * dpi as f64,
        "pt" => number * (dpi as f64 / 72.0),
        "pc" => number * (dpi as f64 / 6.0),
        "em" => number * (16.5 / 96.0) * dpi as f64,
        _ => return None,
    };
    Some(format!("{key}={}", pixels as i64))
}

fn joins_markup(inline: &Inline, before: bool) -> bool {
    match inline {
        Inline::Str(text) => {
            let mut chars = text.chars();
            let edge = if before {
                chars.next_back()
            } else {
                chars.next()
            };
            edge.is_some_and(char::is_alphanumeric)
        }
        Inline::Link(_, body, target) if before => {
            matches!(body.as_slice(), [Inline::Str(label)]
                if label == target.url.strip_prefix("mailto:").unwrap_or(&target.url))
        }
        Inline::Space | Inline::SoftBreak | Inline::LineBreak => false,
        _ => true,
    }
}

fn escape(value: &str, in_link: bool, in_table: bool) -> String {
    let mut output = String::new();
    let mut passthrough = false;
    for ch in value.chars() {
        let special = matches!(
            ch,
            '*' | '_' | '`' | '#' | '{' | '[' | ']' | '<' | '>' | '\\'
        ) || ch == '|' && !in_table;
        if special != passthrough {
            output.push_str("++");
            passthrough = special;
        }
        match ch {
            '+' => output.push_str("{plus}"),
            '|' if in_table => output.push_str("{vbar}"),
            ',' if in_link => output.push_str("&#44;"),
            _ => output.push(ch),
        }
    }
    if passthrough {
        output.push_str("++");
    }
    output
}
