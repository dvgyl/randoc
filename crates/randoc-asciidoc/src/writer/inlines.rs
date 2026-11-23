use randoc_ast::{Attr, Block, Inline, MathType, QuoteType, Target};
use randoc_core::WrapOption;
use randoc_layout::{Doc, cr, empty, hcat, literal, space, text};

use super::{Dialect, Renderer};

impl Renderer<'_> {
    pub(super) fn inlines(&self, inlines: &[Inline], in_link: bool, in_table: bool) -> Doc {
        hcat(inlines.iter().enumerate().map(|(index, inline)| {
            let adjacent = [index.checked_sub(1), Some(index + 1)]
                .into_iter().flatten().filter_map(|i| inlines.get(i))
                .any(|inline| matches!(inline, Inline::Note(_)) || matches!(inline, Inline::Str(text) if text.chars().next().is_some_and(char::is_alphanumeric)));
            self.inline(inline, in_link, in_table, adjacent)
        }))
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
            Inline::Underline(body) => wrap("[underline]#", body, "#"),
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
                let mark = if adjacent { "``" } else { "`" };
                let value = if self.dialect == Dialect::Legacy {
                    code.clone()
                } else {
                    escape(code, false, in_table)
                };
                literal(format!("{mark}{value}{mark}"))
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
                wrap(&format!("[{}]{mark}", styles.join(" ")), body, mark)
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
        for (key, value) in &attr.attributes {
            if matches!(key.as_str(), "width" | "height") {
                if let Some(value) = value.strip_suffix("px") {
                    options.push(format!("{key}={value}"));
                } else if let Some(value) = value.strip_suffix('%') {
                    if let Ok(value) = value.parse::<f64>() {
                        options.push(format!("scaled{key}={value:.1}%"));
                    }
                } else if value.bytes().all(|ch| ch.is_ascii_digit()) {
                    options.push(format!("{key}={value}"));
                }
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
