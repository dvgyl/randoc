use std::cmp::Reverse;

use randoc_ast::walk::{Visitor, Walkable};
use randoc_ast::{Attr, Block, Format, Inline, MathType, Pandoc, QuoteType, Target};
use randoc_core::{Extension, Result, shared};

use super::{State, attrs};

const REFERENCE: &str = "asciidoc-reference";
const NOTE: &str = "asciidoc-note";
const DELIMITERS: &[&str] = &[
    "+++", "++", "+", "**", "*", "__", "_", "``", "`", "##", "#", "~", "^", "`\"", "`'", ">>",
];

const MACROS: &[&str] = &[
    "footnote:",
    "footnoteref:",
    "latexmath:",
    "asciimath:",
    "stem:",
    "image:",
    "link:",
    "mailto:",
    "xref:",
    "pass:",
    "kbd:",
    "btn:",
    "menu:",
    "icon:",
    "http://",
    "https://",
    "ftp://",
];

// The scan adds offsets in descending order.
struct Positions<T>(Vec<(usize, T)>);

impl<T> Default for Positions<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> Positions<T> {
    fn push(&mut self, offset: usize, value: T) {
        self.0.push((offset, value));
    }

    fn get(&self, offset: usize) -> Option<&T> {
        let index = self
            .0
            .binary_search_by_key(&Reverse(offset), |&(offset, _)| Reverse(offset))
            .ok()?;
        Some(&self.0[index].1)
    }
}

#[derive(Default)]
struct Index {
    closing: [Positions<usize>; DELIMITERS.len()],
    brackets: Positions<usize>,
    attribute_ends: Positions<usize>,
    macros: Positions<(&'static str, usize)>,
}

impl Index {
    fn new(text: &str) -> Self {
        let mut index = Self::default();
        // Keep enough closing positions to skip matches that overlap the opener.
        let mut next = [[None; 4]; DELIMITERS.len()];
        let mut bracket_stack = Vec::new();
        let mut attribute_end = None;
        let mut macro_end = text.len();
        for (offset, ch) in text.char_indices().rev() {
            let tail = &text[offset..];
            let before = text[..offset].chars().next_back();
            if ch == '[' || ch.is_ascii_whitespace() {
                macro_end = offset;
            }
            if let Some(name) = MACROS.iter().find(|name| tail.starts_with(**name))
                && text.as_bytes().get(macro_end) == Some(&b'[')
            {
                index.macros.push(offset, (name, macro_end));
            }
            if ch == ']' {
                bracket_stack.push(offset);
            } else if ch == '['
                && let Some(end) = bracket_stack.pop()
            {
                index.brackets.push(offset, end);
            }
            if ch == '}' {
                attribute_end = Some(offset);
            } else if ch == '{'
                && let Some(end) = attribute_end.take()
            {
                index.attribute_ends.push(offset, end);
            }
            if before != Some('\\') || text[..offset].ends_with("\\\\") {
                for (kind, delimiter) in DELIMITERS.iter().enumerate() {
                    let opener = match *delimiter {
                        "`\"" => "\"`",
                        "`'" => "'`",
                        ">>" => "<<",
                        _ => delimiter,
                    };
                    if tail.starts_with(opener)
                        && let Some(end) = next[kind]
                            .iter()
                            .flatten()
                            .copied()
                            .find(|end| *end > offset + opener.len())
                    {
                        index.closing[kind].push(offset, end);
                    }
                    if !tail.starts_with(delimiter) {
                        continue;
                    }
                    let has_boundary = !tail[delimiter.len()..].starts_with(char::is_alphanumeric);
                    if match *delimiter {
                        "+" => before != Some('+') && has_boundary,
                        "*" | "_" | "#" | "`" => {
                            before.is_some_and(|ch| !ch.is_whitespace()) && has_boundary
                        }
                        _ => true,
                    } {
                        next[kind].rotate_right(1);
                        next[kind][0] = Some(offset);
                    }
                }
            }
        }
        index
    }
}

impl State<'_> {
    pub(super) fn inlines(&mut self, text: &str, depth: usize) -> Result<Vec<Inline>> {
        self.depth(depth)?;
        let text = text.trim();
        self.inline_range(text, &Index::new(text), 0, text.len(), depth, false)
    }

    fn attribute(&self, key: &str) -> Option<&str> {
        self.attributes.get(key).map(String::as_str).or_else(|| {
            Some(match key {
                "blank" | "empty" => "",
                "sp" => " ",
                "nbsp" => "\u{a0}",
                "zwsp" => "\u{200b}",
                "apos" => "'",
                "lsquo" => "‘",
                "rsquo" => "’",
                "ldquo" => "“",
                "rdquo" => "”",
                "plus" => "+",
                "vbar" => "|",
                "amp" => "&",
                "lt" => "<",
                "gt" => ">",
                "mdash" => "\u{2009}—\u{2009}",
                "ndash" => "–",
                "asterisk" => "*",
                "backtick" => "`",
                "backslash" => "\\",
                "startsb" => "[",
                "endsb" => "]",
                _ => return None,
            })
        })
    }

    fn inline_range(
        &mut self,
        text: &str,
        index: &Index,
        start: usize,
        end: usize,
        depth: usize,
        hard: bool,
    ) -> Result<Vec<Inline>> {
        self.depth(depth)?;
        let mut output = Vec::new();
        let mut i = start;
        while i < end {
            let tail = &text[i..end];
            let Some(ch) = tail.chars().next() else { break };
            let before = text[start..i].chars().next_back();
            if tail.starts_with(" +\n") {
                output.push(Inline::LineBreak);
                i += 3;
                continue;
            }
            if tail.starts_with(" -- ") {
                for ch in "\u{2009}—\u{2009}".chars() {
                    push_char(&mut output, ch);
                }
                i += 4;
                continue;
            }
            if ch == '\n' {
                let east_asian = self
                    .options
                    .extensions
                    .contains(Extension::EastAsianLineBreaks)
                    && before.is_some_and(wide)
                    && tail[1..].chars().next().is_some_and(wide);
                if matches!(output.last(), Some(Inline::Space)) {
                    output.pop();
                }
                if !east_asian || hard {
                    output.push(if hard {
                        Inline::LineBreak
                    } else {
                        Inline::SoftBreak
                    });
                }
            } else if ch.is_ascii_whitespace() {
                if !matches!(
                    output.last(),
                    None | Some(Inline::Space | Inline::SoftBreak | Inline::LineBreak)
                ) {
                    output.push(Inline::Space);
                }
            } else if ch == '\\'
                && let Some(escaped) = tail[1..].chars().next().filter(|ch| {
                    matches!(
                        ch,
                        '*' | '_'
                            | '#'
                            | '['
                            | ']'
                            | '{'
                            | '}'
                            | '\\'
                            | '\''
                            | '"'
                            | '`'
                            | '+'
                            | '~'
                            | '^'
                            | '|'
                    )
                })
            {
                push_char(&mut output, escaped);
                i += escaped.len_utf8();
            } else if ch == '{'
                && let Some(close) = index.attribute_ends.get(i).filter(|close| **close < end)
                && let Some(length) = self.attribute(&text[i + 1..*close]).map(str::len)
            {
                self.charge(length as u64)?;
                let value = self.attribute(&text[i + 1..*close]).unwrap_or_default();
                for ch in value.chars() {
                    if ch == ' ' {
                        output.push(Inline::Space);
                    } else {
                        push_char(&mut output, ch);
                    }
                }
                i = close + 1;
                continue;
            } else if ch == '&'
                && let Some((entity, length)) = entity(tail)
            {
                push_char(&mut output, entity);
                i += length;
                continue;
            } else if ch == '<'
                && let Some((url, length)) = angle_link(tail)
            {
                output.push(Inline::Link(
                    Attr::default(),
                    vec![Inline::Str(url.into())],
                    Target::new(url, ""),
                ));
                i += length;
                continue;
            } else if ch == '['
                && tail.starts_with("[[")
                && let Some(close) = index
                    .brackets
                    .get(i)
                    .or_else(|| {
                        let nested = index.brackets.get(i + 1)?;
                        text[i..end].starts_with("[[[").then_some(nested)
                    })
                    .filter(|close| **close < end)
                && let Some(value) = text[i + 2..*close].strip_suffix(']')
            {
                let (id, label) = value.split_once(',').unwrap_or((value, value));
                let label = self.inlines(label, depth + 1)?;
                self.define_label(id, label);
                output.push(Inline::Span(Attr::new(id, vec![], vec![]), vec![]));
                i = close + 1;
                continue;
            } else if let Some((next, inline)) = self.inline_macro(text, index, i, end, depth)? {
                for node in inline {
                    if let Inline::Str(value) = node {
                        if let Some(Inline::Str(text)) = output.last_mut() {
                            text.push_str(&value);
                        } else {
                            output.push(Inline::Str(value));
                        }
                    } else {
                        output.push(node);
                    }
                }
                i = next;
                continue;
            } else if let Some((next, inline)) =
                self.delimited(text, index, i, end, depth, before)?
            {
                for node in inline {
                    if let Inline::Str(value) = node {
                        if let Some(Inline::Str(text)) = output.last_mut() {
                            text.push_str(&value);
                        } else {
                            output.push(Inline::Str(value));
                        }
                    } else {
                        output.push(node);
                    }
                }
                i = next;
                continue;
            } else if !before.is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '@'))
                && (tail.starts_with("http://")
                    || tail.starts_with("https://")
                    || tail.starts_with("ftp://"))
            {
                let url = tail
                    .split(|ch: char| {
                        ch.is_ascii_whitespace() || matches!(ch, '<' | '>' | ']' | '"' | '`')
                    })
                    .next()
                    .unwrap_or(tail)
                    .trim_end_matches([',', '.', ';', ':', ')', '!', '?']);
                output.push(Inline::Link(
                    Attr::default(),
                    vec![Inline::Str(url.into())],
                    Target::new(url, ""),
                ));
                i += url.len();
                continue;
            } else if ch.is_ascii_alphanumeric()
                && before.is_none_or(|ch| !email_char(ch))
                && let Some(email) = email(tail)
            {
                output.push(Inline::Link(
                    Attr::default(),
                    vec![Inline::Str(email.into())],
                    Target::new(email, ""),
                ));
                i += email.len();
                continue;
            } else if let Some((token, replacement)) = [
                ("(C)", '©'),
                ("(R)", '®'),
                ("(TM)", '™'),
                ("=>", '⇒'),
                ("->", '→'),
                ("<=", '⇐'),
                ("<-", '←'),
            ]
            .into_iter()
            .find(|(token, _)| tail.starts_with(token))
            {
                push_char(&mut output, replacement);
                i += token.len();
                continue;
            } else if tail.starts_with("...") {
                push_char(&mut output, '…');
                i += 2;
            } else if tail.starts_with("--")
                && !tail.starts_with("---")
                && before.is_some_and(char::is_alphanumeric)
                && tail[2..].starts_with(char::is_alphanumeric)
            {
                push_char(&mut output, '—');
                push_char(&mut output, '\u{200b}');
                i += 1;
            } else if ch == '\'' {
                push_char(&mut output, '’');
            } else {
                push_char(&mut output, ch);
            }
            i += ch.len_utf8();
        }
        if matches!(output.last(), Some(Inline::Space)) {
            output.pop();
        }
        Ok(output)
    }

    fn delimited(
        &mut self,
        text: &str,
        index: &Index,
        start: usize,
        end: usize,
        depth: usize,
        before: Option<char>,
    ) -> Result<Option<(usize, Vec<Inline>)>> {
        let style = (text[start..end].starts_with('['))
            .then(|| index.brackets.get(start).copied())
            .flatten()
            .filter(|close| {
                close + 1 < end && text[*close + 1..end].starts_with(['#', '_', '*', '`', '~', '^'])
            });
        let i = style.map_or(start, |close| close + 1);
        for (kind, delimiter) in DELIMITERS.iter().enumerate() {
            let opener = match *delimiter {
                "`\"" => "\"`",
                "`'" => "'`",
                ">>" => "<<",
                _ => delimiter,
            };
            if !text[i..end].starts_with(opener) {
                continue;
            }
            let inner = i + opener.len();
            let Some(close) = index.closing[kind]
                .get(i)
                .copied()
                .filter(|close| *close > inner && *close + delimiter.len() <= end)
            else {
                continue;
            };
            let value = &text[inner..close];
            if matches!(*delimiter, "*" | "_" | "#" | "`")
                && (style.is_none() && before.is_some_and(char::is_alphanumeric)
                    || value.starts_with(char::is_whitespace)
                    || value.ends_with(char::is_whitespace))
            {
                continue;
            }
            if *delimiter == "+" {
                if before.is_some_and(|ch| ch.is_alphanumeric() || ch == '+')
                    || value.starts_with(char::is_whitespace)
                {
                    continue;
                }
                let after_run = value.trim_start_matches('+');
                if value.starts_with("++")
                    && (after_run.starts_with(char::is_whitespace) || after_run.contains('+'))
                    || value.contains('\n')
                {
                    continue;
                }
            }
            if matches!(*delimiter, "~" | "^")
                && value.bytes().any(|byte| byte.is_ascii_whitespace())
            {
                continue;
            }
            let next = close + delimiter.len();
            let attributes = style.map(|close| {
                let mut parsed = attrs::Attributes::default();
                parsed.parse(&text[start + 1..close]);
                if !parsed.style().is_empty() {
                    parsed.attr.classes.insert(0, parsed.style().to_owned());
                }
                parsed.finish()
            });
            let wrapper = if delimiter.starts_with('#') {
                None
            } else {
                attributes.clone()
            };
            let nodes = match *delimiter {
                "+" | "++" => plain_text(value),
                "+++" => html_inlines(self.html(value, depth)?),
                "`" | "``" => {
                    let parsed = self.inline_range(text, index, inner, close, depth + 1, false)?;
                    monospace(parsed)
                }
                ">>" => {
                    let (id, label) = value.split_once(',').unwrap_or((value, ""));
                    vec![self.xref(id, label, depth + 1)?]
                }
                _ => {
                    let contents =
                        self.inline_range(text, index, inner, close, depth + 1, false)?;
                    vec![match *delimiter {
                        "*" | "**" => Inline::Strong(contents),
                        "_" | "__" => Inline::Emph(contents),
                        "~" => Inline::Subscript(contents),
                        "^" => Inline::Superscript(contents),
                        "`\"" => Inline::Quoted(QuoteType::DoubleQuote, contents),
                        "`'" => Inline::Quoted(QuoteType::SingleQuote, contents),
                        _ => Inline::Span(
                            attributes
                                .unwrap_or_else(|| Attr::new("", vec!["mark".into()], vec![])),
                            contents,
                        ),
                    }]
                }
            };
            let mut nodes = if let Some(mut wrapper) = wrapper {
                wrapper.attributes.insert(0, ("wrapper".into(), "1".into()));
                vec![Inline::Span(wrapper, nodes)]
            } else {
                nodes
            };
            if *delimiter == "+"
                && value.ends_with(char::is_whitespace)
                && !text[next..end].starts_with(char::is_whitespace)
            {
                nodes.push(Inline::Space);
            }
            return Ok(Some((next, nodes)));
        }
        Ok(None)
    }

    fn inline_macro(
        &mut self,
        text: &str,
        index: &Index,
        start: usize,
        end: usize,
        depth: usize,
    ) -> Result<Option<(usize, Vec<Inline>)>> {
        let Some(&(name, stop)) = index.macros.get(start).filter(|(_, stop)| *stop < end) else {
            return Ok(None);
        };
        let body = &text[start + name.len()..end];
        let (target, open) = if let Some(rest) = body.strip_prefix("pass:[")
            && let Some(close) = index
                .brackets
                .get(start + name.len() + 5)
                .filter(|i| **i < end)
            && text[*close + 1..end].starts_with('[')
        {
            (&rest[..close - start - name.len() - 6], *close + 1)
        } else {
            (&text[start + name.len()..stop], stop)
        };
        let Some(close) = index
            .brackets
            .get(open)
            .copied()
            .filter(|close| *close < end)
        else {
            return Ok(None);
        };
        let value = &text[open + 1..close];
        let mut url = target.trim_matches('+').to_owned();

        let inline = match name {
            "footnote:" if target.is_empty() => Inline::Note(vec![Block::Para(
                self.inline_range(text, index, open + 1, close, depth + 1, false)?,
            )]),
            "footnote:" | "footnoteref:" => {
                let key = if name == "footnoteref:" {
                    value.split(',').next().unwrap_or("").trim()
                } else {
                    target
                };
                if !target.is_empty() && !value.is_empty() && name == "footnote:" {
                    let note = self.inline_range(text, index, open + 1, close, depth + 1, false)?;
                    let cost = inline_cost(&note);
                    self.notes.entry(key.into()).or_insert((note, cost));
                }
                Inline::RawInline(Format(NOTE.into()), key.into())
            }
            "latexmath:" | "asciimath:" | "stem:" => {
                Inline::Math(MathType::InlineMath, value.into())
            }
            "image:" => self.image(&url, value, depth + 1)?,
            "xref:" => self.xref(&url, value, depth + 1)?,
            "pass:" => return Ok(Some((close + 1, html_inlines(self.html(value, depth)?)))),
            "kbd:" => {
                let mut nodes = Vec::new();
                for key in value.split('+') {
                    if !nodes.is_empty() {
                        nodes.push(Inline::Str("+".into()));
                    }
                    nodes.push(strong_span("kbd", key.into()));
                }
                return Ok(Some((close + 1, nodes)));
            }
            "btn:" => strong_span("button", format!("[{value}]")),
            "menu:" => strong_span(
                "menu",
                std::iter::once(target)
                    .chain(value.split('>'))
                    .map(str::trim)
                    .collect::<Vec<_>>()
                    .join("\u{a0}›\u{a0}"),
            ),
            "icon:" => {
                let mut parsed = attrs::Attributes::default();
                parsed.parse(value);
                parsed.attr.classes.push("icon".into());
                Inline::Image(
                    parsed.attr,
                    vec![],
                    Target::new(format!("./images/icons/{url}.png"), ""),
                )
            }
            _ => {
                if !matches!(name, "link:" | "mailto:") {
                    url.insert_str(0, name);
                }
                let mut attr = Attr::default();
                let label = if value.is_empty() {
                    vec![Inline::Str(url.clone())]
                } else if value.starts_with('"')
                    && let Some((label, options)) = value[1..].split_once('"')
                {
                    let mut parsed = attrs::Attributes::default();
                    parsed.parse(options.trim_start_matches(','));
                    attr = parsed.finish();
                    self.inlines(label, depth + 1)?
                } else {
                    self.inline_range(text, index, open + 1, close, depth + 1, false)?
                };
                Inline::Link(attr, label, Target::new(url, ""))
            }
        };
        Ok(Some((close + 1, vec![inline])))
    }

    pub(super) fn image(&mut self, path: &str, value: &str, _depth: usize) -> Result<Inline> {
        let mut fields = attrs::fields(value);
        let mut label = String::new();
        let mut attr = Attr::default();
        let mut title = String::new();
        for (index, field) in fields.by_ref().enumerate() {
            let field = field.trim();
            let (key, value) = field.split_once('=').unwrap_or({
                (
                    match index {
                        0 => "alt",
                        1 => "width",
                        2 => "height",
                        _ => "",
                    },
                    field,
                )
            });
            let value = attrs::unquote(value);
            match key {
                "alt" => label = value.into(),
                "title" => title = value.into(),
                "width" | "height" => {
                    let number =
                        value.trim_end_matches(|ch: char| !ch.is_ascii_digit() && ch != '.');
                    if !number.is_empty() {
                        attr.attributes.push((key.into(), format!("{number}px")));
                    }
                }
                "role" => attr
                    .classes
                    .extend(value.split_whitespace().map(str::to_owned)),
                _ if !key.is_empty() => attr.attributes.push((key.into(), value.into())),
                _ => {}
            }
        }
        Ok(Inline::Image(
            attr,
            label
                .split_whitespace()
                .enumerate()
                .flat_map(|(i, word)| {
                    (i > 0)
                        .then_some(Inline::Space)
                        .into_iter()
                        .chain(std::iter::once(Inline::Str(word.into())))
                })
                .collect(),
            Target::new(path, title),
        ))
    }

    fn xref(&mut self, key: &str, label: &str, depth: usize) -> Result<Inline> {
        if label.is_empty() {
            return Ok(Inline::RawInline(Format(REFERENCE.into()), key.into()));
        }
        Ok(link(key, self.inlines(label.trim(), depth)?))
    }

    pub(super) fn paragraph(
        &mut self,
        text: &str,
        depth: usize,
        hard: bool,
    ) -> Result<Vec<Inline>> {
        let text = text.trim();
        self.inline_range(text, &Index::new(text), 0, text.len(), depth, hard)
    }

    pub(super) fn resolve(&mut self, document: &mut Pandoc) -> Result<()> {
        let headings = std::mem::take(&mut self.headings);
        let mut stack = Vec::new();
        let mut order = Vec::new();
        for (index, (level, ..)) in headings.iter().enumerate() {
            while stack.last().is_some_and(|(previous, _)| previous >= level) {
                if let Some((_, previous)) = stack.pop() {
                    order.push(previous);
                }
            }
            stack.push((*level, index));
        }
        order.extend(stack.into_iter().rev().map(|(_, index)| index));
        let mut ids = vec![String::new(); headings.len()];
        for i in order {
            let (_, base, generated, separator) = &headings[i];
            ids[i] = if *generated {
                self.unique_id(base, separator)
            } else {
                base.clone()
            };
        }
        randoc_ast::walk::walk_blocks_mut(document, |block| {
            if let Block::Header(_, attr, title) = block
                && let Some(index) = attr
                    .id
                    .strip_prefix('\0')
                    .and_then(|index| index.parse::<usize>().ok())
                && let Some(id) = ids.get_mut(index)
            {
                if !id.is_empty() {
                    self.define_label(id, title.clone());
                }
                attr.id = std::mem::take(id);
            }
        });
        let mut result = Ok(());
        randoc_ast::walk::walk_inlines_mut(document, |inline| {
            if result.is_ok() {
                result = self.resolve_inline(inline, 0);
            }
        });
        result
    }

    fn resolve_inline(&mut self, inline: &mut Inline, depth: usize) -> Result<()> {
        self.depth(depth)?;
        let Inline::RawInline(format, key) = inline else {
            return Ok(());
        };
        if format.0 == REFERENCE {
            let key = std::mem::take(key);
            let label = if let Some((label, cost)) = self.labels.remove(&key) {
                self.charge(cost)?;
                let mut copy = label.clone();
                self.resolve_copy(&mut copy, depth + 1)?;
                self.labels.insert(key.clone(), (label, cost));
                copy
            } else {
                vec![Inline::Str(key.clone())]
            };
            *inline = link(&key, label);
        } else if format.0 == NOTE {
            let key = std::mem::take(key);
            let mut contents = if let Some((note, cost)) = self.notes.remove(&key) {
                self.charge(cost)?;
                let mut copy = note.clone();
                self.resolve_copy(&mut copy, depth + 1)?;
                self.notes.insert(key, (note, cost));
                copy
            } else {
                self.charge(128)?;
                Vec::new()
            };
            *inline = Inline::Note(vec![Block::Para(std::mem::take(&mut contents))]);
        }
        Ok(())
    }

    fn resolve_copy(&mut self, inlines: &mut Vec<Inline>, depth: usize) -> Result<()> {
        let mut result = Ok(());
        randoc_ast::walk::walk_inlines_mut(inlines, |inline| {
            if result.is_ok() {
                result = self.resolve_inline(inline, depth);
            }
        });
        result
    }

    pub(super) fn define_label(&mut self, id: &str, label: Vec<Inline>) {
        let cost = inline_cost(&label);
        self.labels.insert(id.into(), (label, cost));
    }

    fn unique_id(&mut self, base: &str, separator: &str) -> String {
        if self.ids.insert(base.into()) {
            return base.into();
        }
        let count = self.suffixes.entry(base.into()).or_insert(2);
        loop {
            let id = format!("{base}{separator}{count}");
            *count += 1;
            if self.ids.insert(id.clone()) {
                return id;
            }
        }
    }
}

fn link(key: &str, label: Vec<Inline>) -> Inline {
    Inline::Link(
        Attr::new("", vec!["cross-reference".into()], vec![]),
        label,
        Target::new(format!("#{key}"), ""),
    )
}

fn push_char(output: &mut Vec<Inline>, ch: char) {
    if let Some(Inline::Str(text)) = output.last_mut() {
        text.push(ch);
    } else {
        output.push(Inline::Str(ch.to_string()));
    }
}

fn html_inlines(blocks: Vec<Block>) -> Vec<Inline> {
    blocks
        .into_iter()
        .flat_map(|block| match block {
            Block::Plain(inlines) | Block::Para(inlines) => inlines,
            Block::RawBlock(format, text) => vec![Inline::RawInline(format, text)],
            _ => Vec::new(),
        })
        .collect()
}

fn entity(text: &str) -> Option<(char, usize)> {
    let end = text.as_bytes().iter().take(35).position(|ch| *ch == b';')?;
    let name = &text[1..end];
    let value = if let Some(number) = name.strip_prefix('#') {
        let (number, radix) = number
            .strip_prefix(['x', 'X'])
            .map_or((number, 10), |value| (value, 16));
        char::from_u32(u32::from_str_radix(number, radix).ok()?)?
    } else {
        shared::named_entity(name)?.chars().next()?
    };
    Some((value, end + 1))
}

fn email_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '+' | '-')
}

fn email(text: &str) -> Option<&str> {
    let local = text.find(|ch| !email_char(ch))?;
    let domain = text[local..].strip_prefix('@')?;
    let end = domain
        .find(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '.' | '-'))
        .unwrap_or(domain.len());
    let domain = domain[..end].trim_end_matches('.');
    let (host, top) = domain.rsplit_once('.')?;
    (!host.is_empty() && top.len() >= 2).then(|| &text[..local + 1 + domain.len()])
}

fn wide(ch: char) -> bool {
    matches!(ch as u32, 0x2e80..=0x9fff | 0xac00..=0xd7af | 0xf900..=0xfaff | 0x20000..=0x3ffff)
}

fn strong_span(class: &str, text: String) -> Inline {
    Inline::Span(
        Attr::new("", vec![class.into()], vec![]),
        vec![Inline::Strong(vec![Inline::Str(text)])],
    )
}

fn angle_link(text: &str) -> Option<(&str, usize)> {
    let value = text.strip_prefix('<')?;
    let end = value.find(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '<' | '>'))?;
    if value.as_bytes()[end] != b'>' {
        return None;
    }
    let value = &value[..end];
    ["http://", "https://", "ftp://"]
        .iter()
        .any(|prefix| value.starts_with(prefix))
        .then_some((value, end + 2))
}

pub(super) fn monospace(mut inlines: Vec<Inline>) -> Vec<Inline> {
    for inline in &mut inlines {
        match inline {
            Inline::Str(text) => *inline = Inline::Code(Attr::default(), std::mem::take(text)),
            Inline::Strong(children)
            | Inline::Emph(children)
            | Inline::Link(_, children, _)
            | Inline::Span(_, children)
            | Inline::Superscript(children)
            | Inline::Subscript(children)
            | Inline::Quoted(_, children) => *children = monospace(std::mem::take(children)),
            _ => {}
        }
    }
    inlines
}

fn inline_cost(inlines: &[Inline]) -> u64 {
    clone_cost(inlines)
}

pub(super) fn block_cost(blocks: &[Block]) -> u64 {
    clone_cost(blocks)
}

fn clone_cost(nodes: &[impl Walkable]) -> u64 {
    use randoc_ast::{Cell, ColSpec, Row, Table, TableBody};
    use std::mem::size_of;

    struct Cost(u64);

    impl Cost {
        fn add(&mut self, bytes: usize) {
            self.0 = self.0.saturating_add(bytes as u64);
        }

        fn attr(&mut self, attr: &Attr) {
            self.add(attr.id.len());
            for class in &attr.classes {
                self.add(size_of::<String>() + class.len());
            }
            for (key, value) in &attr.attributes {
                self.add(size_of::<(String, String)>() + key.len() + value.len());
            }
        }

        fn rows(&mut self, rows: &[Row]) {
            self.add(std::mem::size_of_val(rows));
            for row in rows {
                self.attr(&row.attr);
                self.add(row.cells.len() * size_of::<Cell>());
                for cell in &row.cells {
                    self.attr(&cell.attr);
                }
            }
        }
    }

    impl Visitor for Cost {
        fn inline(&mut self, inline: &Inline) {
            self.add(size_of::<Inline>());
            match inline {
                Inline::Str(text) | Inline::Math(_, text) => self.add(text.len()),
                Inline::RawInline(format, text) => self.add(format.0.len() + text.len()),
                Inline::Code(attr, text) => {
                    self.attr(attr);
                    self.add(text.len());
                }
                Inline::Span(attr, _) => self.attr(attr),
                Inline::Link(attr, _, target) | Inline::Image(attr, _, target) => {
                    self.attr(attr);
                    self.add(target.url.len() + target.title.len());
                }
                _ => {}
            }
        }

        fn block(&mut self, block: &Block) {
            self.add(size_of::<Block>());
            match block {
                Block::CodeBlock(attr, text) => {
                    self.attr(attr);
                    self.add(text.len());
                }
                Block::RawBlock(format, text) => self.add(format.0.len() + text.len()),
                Block::Header(_, attr, _) | Block::Figure(attr, ..) | Block::Div(attr, _) => {
                    self.attr(attr);
                }
                Block::Table(table) => {
                    self.add(size_of::<Table>());
                    self.attr(&table.attr);
                    self.add(table.col_specs.len() * size_of::<ColSpec>());
                    self.attr(&table.head.attr);
                    self.rows(&table.head.rows);
                    self.add(table.bodies.len() * size_of::<TableBody>());
                    for body in &table.bodies {
                        self.attr(&body.attr);
                        self.rows(&body.head);
                        self.rows(&body.body);
                    }
                    self.attr(&table.foot.attr);
                    self.rows(&table.foot.rows);
                }
                _ => {}
            }
        }
    }
    let mut cost = Cost(128);
    for node in nodes {
        node.query(&mut cost);
    }
    cost.0
}

fn plain_text(value: &str) -> Vec<Inline> {
    let mut nodes = Vec::new();
    for word in value.split_ascii_whitespace() {
        if !nodes.is_empty() {
            nodes.push(Inline::Space);
        }
        nodes.push(Inline::Str(word.into()));
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::Index;

    #[test]
    fn dense_delimiters_keep_compact_closing_positions() {
        let input = "+".repeat(100_000);
        let index = Index::new(&input);
        for (kind, advance) in [(0, 4), (1, 3)] {
            assert_eq!(index.closing[kind].get(0), Some(&advance));
            assert_eq!(index.closing[kind].get(75_000), Some(&(75_000 + advance)));
            assert_eq!(index.closing[kind].get(99_999), None);
        }
        assert!(index.closing[2].0.is_empty());
        assert!(
            index
                .closing
                .iter()
                .map(|matches| matches.0.len())
                .sum::<usize>()
                < 2 * input.len()
        );
    }
}
