use randoc_ast::{Attr, Inline, MathType, QuoteType, Target};
use randoc_core::shared::stringify;
use randoc_core::{Extension, LogMessage, Result, WrapOption};
use randoc_layout::{Doc, blankline, cr, hcat, literal, nest, space, vcat};

use super::{Image, Writer};

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let before = prev.is_none_or(|c: char| c.is_whitespace() || "-:/'\"<([{".contains(c));
        let after = chars.peek().is_none_or(|next| {
            next.is_whitespace()
                || "-:/'\">)]}.,;!?".contains(*next)
                || c == '_' && matches!(next, '_' | '*' | '`' | '|')
        });
        let standalone_backtick =
            c == '`' && prev.is_none() && chars.peek().is_some_and(|c| c.is_whitespace());
        if c == '\\'
            || !standalone_backtick
                && "*`_|".contains(c)
                && (after || before && !chars.peek().is_some_and(|c| c.is_whitespace()))
        {
            out.push('\\');
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

fn escape_smart(mut text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    while let Some(c) = text.chars().next() {
        text = &text[c.len_utf8()..];
        match c {
            '-' | '.' => {
                let run = text.bytes().take_while(|&next| next == c as u8).count();
                text = &text[run..];
                let len = run + 1;
                let minimum = if c == '-' { 2 } else { 3 };
                if len >= minimum {
                    for _ in 0..=len - minimum {
                        out.push('\\');
                        out.push(c);
                    }
                    out.extend(std::iter::repeat_n(c, minimum - 1));
                } else {
                    out.extend(std::iter::repeat_n(c, len));
                }
            }
            '"' | '\'' => {
                out.push('\\');
                out.push(c);
            }
            '\u{2019}' => out.push('\''),
            '\u{2026}' => out.push_str("..."),
            '\u{2013}' => out.push_str("--"),
            '\u{2014}' => out.push_str("---"),
            _ => out.push(c),
        }
    }
    out
}

fn wrap(start: &str, body: Doc, end: &str) -> Doc {
    literal(start) + body + literal(end)
}

fn markup(inline: &Inline) -> bool {
    matches!(
        inline,
        Inline::Emph(_)
            | Inline::Underline(_)
            | Inline::Strong(_)
            | Inline::Subscript(_)
            | Inline::Superscript(_)
            | Inline::Code(..)
            | Inline::Math(MathType::InlineMath, _)
            | Inline::Image(..)
            | Inline::Link(..)
    )
}

fn starts(inline: &Inline) -> Option<char> {
    match inline {
        Inline::Str(text) => text.chars().next(),
        Inline::Space | Inline::SoftBreak | Inline::LineBreak => Some(' '),
        _ => None,
    }
}

fn ends(inline: &Inline) -> Option<char> {
    match inline {
        Inline::Str(text) => text.chars().next_back(),
        Inline::Space | Inline::SoftBreak | Inline::LineBreak => Some(' '),
        _ => None,
    }
}

impl Writer<'_> {
    pub(super) fn inlines(&mut self, ils: &[Inline], depth: usize) -> Result<Doc> {
        self.inline_sequence(ils.iter(), depth)
    }

    fn inline_sequence<'i>(
        &mut self,
        ils: impl IntoIterator<Item = &'i Inline>,
        depth: usize,
    ) -> Result<Doc> {
        self.check_depth(depth)?;
        let mut docs = Vec::new();
        let mut previous = None;
        for inline in ils {
            if let Some(before) = previous {
                let left = markup(inline)
                    && (markup(before)
                        || ends(before).is_none_or(|c| {
                            !(c.is_whitespace() || "-:/'\"<([{\u{2018}\u{201c}".contains(c))
                        }));
                let right = markup(before)
                    && starts(inline).is_none_or(|c| {
                        !(c.is_whitespace() || "-:/'\">)]}.,;!?\u{2019}\u{201d}".contains(c))
                    });
                let note = markup(before) && matches!(inline, Inline::Note(_));
                if left || right || note {
                    docs.push(literal("\\ "));
                }
            }
            docs.push(self.inline(inline, depth + 1)?);
            previous = Some(inline);
        }
        Ok(hcat(docs))
    }

    fn inline(&mut self, inline: &Inline, depth: usize) -> Result<Doc> {
        Ok(match inline {
            Inline::Str(s) => {
                let text = escape(s);
                literal(if self.options.extensions.contains(Extension::Smart) {
                    escape_smart(&text)
                } else {
                    text
                })
            }
            Inline::Space => space(),
            Inline::SoftBreak if self.options.wrap_text == WrapOption::WrapPreserve => cr(),
            Inline::SoftBreak => space(),
            Inline::LineBreak => cr(),
            Inline::Emph(ils) | Inline::Underline(ils) => self.styled(ils, depth, "*")?,
            Inline::Strong(ils) => self.styled(ils, depth, "**")?,
            Inline::Strikeout(ils) => wrap("[STRIKEOUT:", literal(escape(&stringify(ils))), "]"),
            Inline::Subscript(ils) => wrap(":sub:`", literal(escape(&stringify(ils))), "`"),
            Inline::Superscript(ils) => wrap(":sup:`", literal(escape(&stringify(ils))), "`"),
            Inline::Span(attr, ils) if attr.has_class("mark") => {
                wrap(":mark:`", self.inlines(ils, depth)?, "`")
            }
            Inline::Span(_, ils) | Inline::SmallCaps(ils) | Inline::Cite(_, ils) => {
                self.inlines(ils, depth)?
            }
            Inline::Quoted(kind, ils) => {
                let smart = self.options.extensions.contains(Extension::Smart);
                let (start, end) = match (kind, smart) {
                    (QuoteType::SingleQuote, true) => ("'", "'"),
                    (QuoteType::DoubleQuote, true) => ("\"", "\""),
                    (QuoteType::SingleQuote, false) => ("‘", "’"),
                    (QuoteType::DoubleQuote, false) => ("“", "”"),
                };
                wrap(start, self.inlines(ils, depth)?, end)
            }
            Inline::Code(attr, text) => {
                if let Some(role) = attr
                    .get("role")
                    .filter(|_| attr.has_class("interpreted-text"))
                {
                    literal(format!(":{role}:`{}`", text.replace('`', "\\`")))
                } else if text.contains('`') {
                    literal(format!(":literal:`{}`", escape(text)))
                } else {
                    literal(format!("``{text}``"))
                }
            }
            Inline::Math(MathType::InlineMath, s) => literal(format!(":math:`{s}`")),
            Inline::Math(MathType::DisplayMath, s) => {
                let math = if s.contains('\n') {
                    literal(".. math::").above_blank(nest(3, literal(s)))
                } else {
                    literal(format!(".. math:: {s}"))
                };
                blankline() + math + blankline()
            }
            Inline::RawInline(format, text) if format == "rst" => literal(text),
            Inline::RawInline(format, text) if format == "tex" || format == "latex" => {
                self.raw_tex = true;
                literal(format!(":raw-latex:`{text}`"))
            }
            Inline::RawInline(..) => {
                let text =
                    randoc_native::show_inline(inline, self.context.limits.max_nesting_depth);
                self.context.report(LogMessage::InlineNotRendered(text));
                Doc::default()
            }
            Inline::Link(_, label, target) => {
                if let [Inline::Image(a, alt, image)] = label.as_slice() {
                    self.image(a, alt, image, Some(&target.url), depth)?
                } else {
                    self.link(label, target, depth)?
                }
            }
            Inline::Image(a, label, target) => self.image(a, label, target, None, depth)?,
            Inline::Note(blocks) => {
                let number = self.notes.len() + 1;
                self.check_depth(depth)?;
                self.notes.push((blocks.clone(), depth));
                literal(format!(" [{number}]_"))
            }
        })
    }

    fn styled(&mut self, ils: &[Inline], depth: usize, mark: &str) -> Result<Doc> {
        if let [Inline::Link(..) | Inline::Image(..)] = ils {
            return self.inlines(ils, depth);
        }
        let mut strong = mark == "**";
        let mut plain = Vec::new();
        let mut stack = vec![(ils.iter(), depth)];
        while let Some((iter, depth)) = stack.last_mut() {
            let Some(inline) = iter.next() else {
                stack.pop();
                continue;
            };
            match inline {
                Inline::Emph(inner) | Inline::Underline(inner) | Inline::Strong(inner) => {
                    self.check_depth(*depth)?;
                    let next = *depth + 1;
                    strong |= matches!(inline, Inline::Strong(_));
                    stack.push((inner.iter(), next));
                }
                _ => plain.push(inline),
            }
        }
        let mark = if strong { "**" } else { "*" };
        let mut docs = Vec::new();
        let mut start = 0;
        for (index, inline) in plain.iter().enumerate() {
            if matches!(inline, Inline::Link(..) | Inline::Image(..)) {
                if start < index {
                    let run = &plain[start..index];
                    let first = usize::from(matches!(run.first(), Some(Inline::Space)));
                    let end = run.len() - usize::from(matches!(run.last(), Some(Inline::Space)));
                    if first > 0 {
                        docs.push(space());
                    }
                    if first < end {
                        docs.push(wrap(
                            mark,
                            self.inline_sequence(run[first..end].iter().copied(), depth)?,
                            mark,
                        ));
                    }
                    if end > 0 && end < run.len() {
                        docs.push(space());
                    }
                }
                docs.push(self.inline(inline, depth)?);
                start = index + 1;
            }
        }
        if start == 0 {
            return Ok(wrap(mark, self.inline_sequence(plain, depth)?, mark));
        }
        if start < plain.len() {
            if matches!(plain[start], Inline::Space) {
                docs.push(space());
                start += 1;
            }
            if start < plain.len() {
                docs.push(wrap(
                    mark,
                    self.inline_sequence(plain[start..].iter().copied(), depth)?,
                    mark,
                ));
            }
        }
        Ok(hcat(docs))
    }

    fn link(&mut self, label: &[Inline], target: &Target, depth: usize) -> Result<Doc> {
        self.check_depth(depth)?;
        let plain = stringify(label);
        let text = hcat(plain.split_inclusive(' ').map(|part| {
            if let Some(word) = part.strip_suffix(' ') {
                literal(word) + space()
            } else {
                literal(part)
            }
        }));
        let url = &target.url;
        if plain == *url || url.strip_prefix("mailto:") == Some(&plain) {
            return Ok(literal(plain));
        }
        if self.options.reference_links
            && !plain.is_empty()
            && self.link_urls.get(&plain).is_none_or(|old| old == url)
        {
            if self.link_urls.insert(plain, url.clone()).is_none() {
                self.links
                    .push(literal(".. _") + text.clone() + literal(format!(": {url}")));
            }
            return Ok(wrap("`", text, "`_"));
        }
        Ok(wrap("`", text, &format!(" <{url}>`__")))
    }

    fn image(
        &mut self,
        a: &Attr,
        label: &[Inline],
        target: &Target,
        link: Option<&str>,
        depth: usize,
    ) -> Result<Doc> {
        let image = Image {
            attr: a.clone(),
            url: target.url.clone(),
            target: link.map(str::to_owned),
        };
        let mut name = stringify(label);
        let generated =
            name.is_empty() || self.image_ids.get(&name).is_some_and(|old| *old != image);
        if generated {
            self.image_number += 1;
            name = format!("image{}", self.image_number);
        }
        if generated || !self.image_ids.contains_key(&name) {
            let definition = literal(format!(".. |{name}| image:: {}", target.url));
            let options = image_options(a, None, None, link);
            self.images.push(definition.above(vcat(options)));
            self.image_ids.insert(name.clone(), image);
        }
        let label = if generated {
            literal(name)
        } else {
            self.inlines(label, depth)?
        };
        Ok(wrap("|", label, "|"))
    }
}

pub(super) fn image_options(
    a: &Attr,
    figure: Option<&Attr>,
    alt: Option<Doc>,
    link: Option<&str>,
) -> Vec<Doc> {
    let mut out = Vec::new();
    if let Some(f) = figure.filter(|a| !a.id.is_empty()) {
        out.push(literal(format!("   name: {}", f.id)));
    }
    if let Some(alt) = alt {
        out.push(literal("   :alt: ") + alt);
    }
    if let Some(f) = figure.filter(|a| !a.classes.is_empty()) {
        out.push(literal(format!("   :figclass: {}", f.classes.join(" "))));
    }
    if !a.id.is_empty() {
        out.push(literal(format!("   :name: {}", a.id)));
    }
    match a.classes.as_slice() {
        [align]
            if matches!(
                align.as_str(),
                "align-top" | "align-middle" | "align-bottom"
            ) =>
        {
            out.push(literal(format!("   :align: {}", &align[6..])));
        }
        [align]
            if matches!(
                align.as_str(),
                "align-left" | "align-center" | "align-right"
            ) => {}
        classes if !classes.is_empty() => {
            out.push(literal(format!("   :class: {}", classes.join(" "))));
        }
        _ => {}
    }
    for name in ["width", "height"] {
        if let Some(value) = a.get(name) {
            let value = value
                .strip_suffix('%')
                .and_then(|v| v.parse::<f64>().ok())
                .map_or_else(|| value.into(), |p| format!("{p:?}%"));
            out.push(literal(format!("   :{name}: {value}")));
        }
    }
    if let Some(link) = link {
        out.push(literal(format!("   :target: {link}")));
    }
    out
}
