use std::collections::HashMap;

use randoc_ast::{Attr, Format, Inline, MathType, QuoteType, Target};
use randoc_core::{
    Result,
    shared::{emoji_from_alias, stringify},
};

use crate::{attrs, depth_error};

pub(crate) const REFERENCE: &str = "\0djot-reference";
pub(crate) const NOTE: &str = "\0djot-note";

const POSITION_STEP: usize = 256;

#[derive(Clone, Copy)]
struct Opening {
    index: usize,
    slot: usize,
    previous: Option<usize>,
    start: usize,
}

#[derive(Clone, Copy)]
struct Position {
    offset: usize,
    line: usize,
    column: usize,
}

pub(crate) struct Positions<'a> {
    source: &'a str,
    checkpoints: Vec<Position>,
}

impl<'a> Positions<'a> {
    pub(crate) fn new(source: &'a str, lines: impl IntoIterator<Item = (usize, usize)>) -> Self {
        let mut checkpoints = Vec::new();
        let mut offset = 0;
        for (text, (line, column)) in source.split('\n').zip(lines) {
            checkpoints.push(Position {
                offset,
                line,
                column,
            });
            let mut next = POSITION_STEP;
            for (index, (byte, _)) in text.char_indices().enumerate() {
                if byte >= next {
                    checkpoints.push(Position {
                        offset: offset + byte,
                        line,
                        column: column + index,
                    });
                    next = byte + POSITION_STEP;
                }
            }
            offset += text.len() + 1;
        }
        Self {
            source,
            checkpoints,
        }
    }

    fn range(&self, start: usize, end: usize) -> Option<String> {
        let (start_line, start_column) = self.coordinate(start, false)?;
        let (line, column) = self.coordinate(end.checked_sub(1)?, true)?;
        Some(format!("{start_line}:{start_column}-{line}:{column}"))
    }

    fn coordinate(&self, offset: usize, end: bool) -> Option<(usize, usize)> {
        let byte = *self.source.as_bytes().get(offset)?;
        let next = self
            .checkpoints
            .partition_point(|point| point.offset <= offset);
        if end && byte == b'\n' {
            return Some((self.checkpoints.get(next)?.line, 0));
        }
        let point = self.checkpoints.get(next.checked_sub(1)?)?;
        // Count UTF-8 leading bytes to include the character at the end offset.
        let width = self.source.as_bytes()[point.offset..=offset]
            .iter()
            .filter(|&&b| b & 0xc0 != 0x80)
            .count();
        Some((point.line, point.column + width - 1))
    }
}

struct Token {
    node: Inline,
    start: usize,
    end: usize,
}

struct Scan<'a> {
    source: &'a str,
    nodes: Vec<Token>,
    positions: Option<&'a Positions<'a>>,
    cursor: usize,
    open: Vec<Opening>,
    tops: [Option<usize>; 20],
    ticks: HashMap<usize, usize>,
    parens: HashMap<usize, usize>,
    limit: usize,
}

pub(crate) fn parse(
    source: &str,
    limit: usize,
    positions: Option<&Positions<'_>>,
) -> Result<Vec<Inline>> {
    let mut last = HashMap::new();
    let mut ticks = HashMap::new();
    let mut run_end = None;
    for (index, c) in source.char_indices().rev() {
        if c == '`' {
            run_end.get_or_insert(index + 1);
        } else if let Some(end) = run_end.take() {
            let start = index + c.len_utf8();
            if let Some(next) = last.insert(end - start, start) {
                ticks.insert(start, next);
            }
        }
    }
    if let Some(end) = run_end
        && let Some(next) = last.insert(end, 0)
    {
        ticks.insert(0, next);
    }
    let mut scan = Scan {
        source,
        positions,
        cursor: 0,
        nodes: Vec::new(),
        open: Vec::new(),
        tops: [None; 20],
        ticks,
        parens: parentheses(source),
        limit,
    };
    scan.run()?;
    Ok(collect(scan.nodes, positions))
}

impl Scan<'_> {
    fn literal(&mut self, text: impl Into<String>) {
        self.emit(Inline::Str(text.into()), self.cursor);
    }

    fn emit(&mut self, node: Inline, start: usize) {
        self.nodes.push(Token {
            node,
            start,
            end: usize::MAX,
        });
    }

    fn finish(&mut self, end: usize) {
        if let Some(node) = self.nodes.last_mut()
            && node.end == usize::MAX
        {
            node.end = end;
        }
    }

    fn push(&mut self, slot: usize, start: usize, literal: &str) -> Result<()> {
        if self.open.len() >= self.limit {
            return Err(depth_error());
        }
        let index = self.nodes.len();
        self.literal(literal);
        let previous = self.tops[slot].replace(self.open.len());
        self.open.push(Opening {
            index,
            slot,
            previous,
            start,
        });
        Ok(())
    }

    fn pop(&mut self, index: usize) -> Option<(Opening, Vec<Inline>)> {
        let opening = *self.open.get(index)?;
        while self.open.len() > index {
            let item = self.open.pop()?;
            self.tops[item.slot] = item.previous;
        }
        let children = collect(self.nodes.split_off(opening.index + 1), self.positions);
        self.nodes.pop();
        Some((opening, children))
    }

    fn run(&mut self) -> Result<()> {
        let mut pos = 0;
        while pos < self.source.len() {
            self.finish(pos);
            self.cursor = pos;
            let text = &self.source[pos..];
            let Some(c) = text.chars().next() else { break };
            let after = &text[c.len_utf8()..];
            let prev = self.source[..pos].chars().next_back();
            let next = after.chars().next();
            if c == '\\' && next.is_some_and(|c| matches!(c, ' ' | '\t' | '\n')) {
                let rest = after.trim_start_matches([' ', '\t']);
                if let Some(rest) = rest.strip_prefix('\n') {
                    self.emit(Inline::LineBreak, self.cursor);
                    pos = self.source.len() - rest.len();
                    continue;
                }
            }
            if c == '\\'
                && let Some(escaped) = next
            {
                let node = match escaped {
                    ' ' => Some(Inline::Str("\u{a0}".into())),
                    c if c.is_ascii_punctuation() => Some(Inline::Str(c.to_string())),
                    _ => None,
                };
                if let Some(node) = node {
                    self.emit(node, self.cursor);
                    pos += 1 + escaped.len_utf8();
                    continue;
                }
            }
            let dollars = if text.starts_with("$$`") {
                2
            } else if text.starts_with("$`") {
                1
            } else {
                0
            };
            if c == '`' || dollars > 0 {
                let start = pos + dollars;
                let width = self.source[start..]
                    .bytes()
                    .take_while(|b| *b == b'`')
                    .count();
                let close = self.ticks.get(&start).copied();
                let end = close.unwrap_or(self.source.len());
                let payload = &self.source[start + width..end];
                let payload = if payload.starts_with(' ')
                    && payload.ends_with(' ')
                    && payload.len() >= 2
                    && (payload.trim_matches(' ').starts_with('`')
                        || payload.trim_matches(' ').ends_with('`'))
                {
                    &payload[1..payload.len() - 1]
                } else {
                    payload
                };
                pos = close.map_or(end, |end| end + width);
                let node = if dollars > 0 {
                    Inline::Math(
                        if dollars == 1 {
                            MathType::InlineMath
                        } else {
                            MathType::DisplayMath
                        },
                        payload.into(),
                    )
                } else if let Some(raw) = self.source[pos..].strip_prefix("{=")
                    && let Some((format, remainder)) =
                        bounded_suffix(raw, '}', |c| c.is_whitespace() || c == '{')
                    && !format.is_empty()
                {
                    pos = self.source.len() - remainder.len();
                    Inline::RawInline(Format(format.into()), payload.into())
                } else {
                    Inline::Code(Attr::default(), payload.into())
                };
                self.emit(node, self.cursor);
                continue;
            }
            if c == '[' || text.starts_with("![") {
                let image = c == '!';
                let width = if image { 2 } else { 1 };
                self.push(if image { 17 } else { 16 }, pos, &text[..width])?;
                pos += width;
                continue;
            }
            if c == ']'
                && let Some(index) = self.tops[16].max(self.tops[17])
            {
                let opening = self.open[index];
                let label_start = opening.start + if opening.slot == 17 { 2 } else { 1 };
                let label = &self.source[label_start..pos];
                if label.starts_with('^')
                    && !label[1..].contains(|c: char| c.is_whitespace() || matches!(c, '[' | ']'))
                {
                    self.pop(index);
                    self.emit(
                        Inline::RawInline(Format(NOTE.into()), label[1..].into()),
                        opening.start,
                    );
                } else if let Some((url, end)) = self
                    .parens
                    .get(&(pos + 1))
                    .filter(|_| after.starts_with('('))
                    .map(|&end| (destination(&self.source[pos + 2..end]), end))
                {
                    if let Some((_, children)) = self.pop(index) {
                        self.link(opening, children, url, None);
                        pos = end + 1;
                        continue;
                    }
                } else if let Some((key, rest)) = after
                    .strip_prefix('[')
                    .and_then(|text| bounded_suffix(text, ']', |c| matches!(c, '[' | '\n')))
                {
                    if let Some((_, children)) = self.pop(index) {
                        let key = if key.is_empty() {
                            stringify(&children)
                        } else {
                            key.into()
                        };
                        self.link(opening, children, String::new(), Some(key));
                        pos = self.source.len() - rest.len();
                        continue;
                    }
                } else if after.starts_with('{')
                    && attrs::parse(after).is_some()
                    && let Some((_, children)) = self.pop(index)
                {
                    self.emit(Inline::Span(Attr::default(), children), opening.start);
                } else {
                    while self.open.len() > index {
                        if let Some(item) = self.open.pop() {
                            self.tops[item.slot] = item.previous;
                        }
                    }
                    self.literal("]");
                }
                pos += 1;
                continue;
            }
            if c == '{' {
                if let Some((attr, rest)) = attrs::parse(text) {
                    self.apply_attribute(attr);
                    pos = self.source.len() - rest.len();
                    continue;
                }
                if let Some(marker) = next.and_then(marker_slot) {
                    let literal = match next {
                        Some('\'') => "’",
                        Some('"') => "“",
                        _ => &text[..2],
                    };
                    self.push(explicit_slot(marker), pos, literal)?;
                    pos += 2;
                    continue;
                }
            }
            if let Some(marker) = marker_slot(c) {
                let explicit_end = next == Some('}');
                let close = prev.is_some_and(|c| !c.is_whitespace())
                    && !(c == '\'' && next.is_some_and(char::is_alphanumeric));
                let opening = if explicit_end {
                    self.tops[explicit_slot(marker)].or(self.tops[marker])
                } else if close {
                    self.tops[marker].filter(|&i| self.open[i].start + 1 < pos)
                } else {
                    None
                };
                if let Some(index) = opening
                    && let Some((opening, children)) = self.pop(index)
                {
                    self.emit(delimited(c, children), opening.start);
                    pos += if explicit_end { 2 } else { 1 };
                    continue;
                }
                let opens = next.is_some_and(|c| !c.is_whitespace())
                    && !(matches!(c, '\'' | '"') && prev.is_some_and(char::is_alphanumeric))
                    && !(c == '\''
                        && matches!(prev, Some('`' | ')' | ']'))
                        && next.is_some_and(char::is_alphanumeric));
                if opens && !matches!(c, '+' | '=' | '-') {
                    self.push(
                        marker,
                        pos,
                        if c == '\'' {
                            "’"
                        } else if c == '"' {
                            "“"
                        } else {
                            &text[..1]
                        },
                    )?;
                    pos += 1;
                    continue;
                }
            }
            if c == '<'
                && let Some((link, rest)) =
                    bounded_suffix(after, '>', |c| c.is_whitespace() || c == '<')
            {
                let class = if link.split_once(':').is_some_and(|(scheme, _)| {
                    !scheme.is_empty()
                        && scheme
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
                }) {
                    Some("uri")
                } else if link
                    .split_once('@')
                    .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty())
                {
                    Some("email")
                } else {
                    None
                };
                if let Some(class) = class {
                    let target = if class == "email" {
                        format!("mailto:{link}")
                    } else {
                        link.into()
                    };
                    self.emit(
                        Inline::Link(
                            Attr::new("", vec![class.into()], vec![]),
                            vec![Inline::Str(link.into())],
                            Target::new(target, ""),
                        ),
                        self.cursor,
                    );
                    pos = self.source.len() - rest.len();
                    continue;
                }
            }
            if c == ':'
                && let Some((name, rest)) = bounded_suffix(after, ':', |c| {
                    !c.is_ascii_alphanumeric() && !matches!(c, '_' | '-' | '+')
                })
                && !name.is_empty()
            {
                pos = self.source.len() - rest.len();
                let mut rest = rest;
                let mut attr = Attr::default();
                while let Some((next, tail)) = attrs::parse(rest) {
                    attrs::merge(&mut attr, next);
                    rest = tail;
                }
                let payload = if let Some(emoji) = emoji_from_alias(name) {
                    attrs::merge(
                        &mut attr,
                        Attr::new(
                            "",
                            vec!["emoji".into()],
                            vec![("data-emoji".into(), name.into())],
                        ),
                    );
                    emoji
                } else {
                    attr.classes.push("symbol".into());
                    name
                };
                self.emit(
                    Inline::Span(attr, vec![Inline::Str(payload.into())]),
                    self.cursor,
                );
                self.finish(pos);
                pos = self.source.len() - rest.len();
                continue;
            }
            match c {
                ' ' | '\t' => {
                    if let Some(last) = self.nodes.last_mut()
                        && matches!(last.node, Inline::Space)
                    {
                        last.end = usize::MAX;
                    } else {
                        self.emit(Inline::Space, self.cursor);
                    }
                    pos += text
                        .bytes()
                        .take_while(|b| matches!(b, b' ' | b'\t'))
                        .count();
                    continue;
                }
                '\n' => {
                    if self.positions.is_none() || !prev.is_some_and(|c| matches!(c, ' ' | '\t')) {
                        while self
                            .nodes
                            .last()
                            .is_some_and(|last| matches!(last.node, Inline::Space))
                        {
                            self.nodes.pop();
                        }
                    }
                    self.emit(Inline::SoftBreak, self.cursor);
                }
                '\'' => self.literal("’"),
                '"' => self.literal("”"),
                '.' if text.starts_with("...") => {
                    self.literal("…");
                    pos += 3;
                    continue;
                }
                '-' if text.starts_with("--") => {
                    let count = text.bytes().take_while(|b| *b == b'-').count();
                    let (em, en) = if count % 3 == 0 {
                        (count / 3, 0)
                    } else if count % 2 == 0 {
                        (0, count / 2)
                    } else if count % 3 == 2 {
                        ((count - 2) / 3, 1)
                    } else {
                        ((count - 4) / 3, 2)
                    };
                    self.literal(format!("{}{}", "—".repeat(em), "–".repeat(en)));
                    pos += count;
                    continue;
                }
                _ => {
                    let width = if !special(c) {
                        after
                            .find(special)
                            .map_or(text.len(), |offset| c.len_utf8() + offset)
                    } else {
                        c.len_utf8()
                    };
                    self.literal(&text[..width]);
                    pos += width;
                    continue;
                }
            }
            pos += c.len_utf8();
        }
        self.finish(pos);
        Ok(())
    }

    fn apply_attribute(&mut self, attr: Attr) {
        if let Some(opening) = self.open.last().copied()
            && opening.index + 1 == self.nodes.len()
        {
            if matches!(opening.slot, 16 | 17) {
                if let Some(last) = self.nodes.last_mut() {
                    last.end = usize::MAX;
                }
                return;
            }
            self.open.pop();
            self.tops[opening.slot] = opening.previous;
        }
        if attr.is_empty()
            || self
                .nodes
                .last()
                .is_some_and(|last| matches!(last.node, Inline::Space))
        {
            return;
        }
        if let Some(last) = self.nodes.pop() {
            let Token { node, start, end } = self.attribute_word(last);
            self.nodes.push(Token {
                node: attrs::inline(attr, node),
                start,
                end,
            });
        }
    }

    fn attribute_word(&mut self, last: Token) -> Token {
        if !matches!(last.node, Inline::Str(_)) {
            return last;
        }
        let floor = self.open.last().map_or(0, |opening| opening.index + 1);
        let len = self.nodes.len();
        let start = self.nodes[floor..]
            .iter()
            .rposition(|token| !matches!(token.node, Inline::Str(_)))
            .map_or(floor, |index| floor + index + 1);
        if start == len {
            return last;
        }
        let first = self.nodes[start].start;
        let mut text = String::new();
        for token in self.nodes.drain(start..).chain(std::iter::once(last)) {
            if let Inline::Str(s) = token.node {
                text.push_str(&s);
            }
        }
        Token {
            node: Inline::Str(text),
            start: first,
            end: self.cursor,
        }
    }

    fn link(
        &mut self,
        opening: Opening,
        children: Vec<Inline>,
        url: String,
        reference: Option<String>,
    ) {
        let mut attr = Attr::default();
        if let Some(reference) = reference {
            attr.attributes.push((REFERENCE.into(), reference));
        }
        let target = Target::new(url, "");
        let node = if opening.slot == 17 {
            Inline::Image(attr, children, target)
        } else {
            Inline::Link(attr, children, target)
        };
        self.emit(node, opening.start);
    }
}

fn special(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t'
            | '\n'
            | '\\'
            | '`'
            | '$'
            | '['
            | ']'
            | '!'
            | '{'
            | '}'
            | '_'
            | '*'
            | '~'
            | '^'
            | '+'
            | '='
            | '-'
            | '<'
            | ':'
            | '\''
            | '"'
            | '.'
    )
}

fn marker_slot(marker: char) -> Option<usize> {
    Some(match marker {
        '_' => 0,
        '*' => 1,
        '~' => 2,
        '^' => 3,
        '+' => 4,
        '-' => 5,
        '=' => 6,
        '\'' => 7,
        '"' => 18,
        _ => return None,
    })
}

fn delimited(marker: char, children: Vec<Inline>) -> Inline {
    match marker {
        '_' => Inline::Emph(children),
        '*' => Inline::Strong(children),
        '~' => Inline::Subscript(children),
        '^' => Inline::Superscript(children),
        '\'' => Inline::Quoted(QuoteType::SingleQuote, children),
        '"' => Inline::Quoted(QuoteType::DoubleQuote, children),
        _ => {
            let class = match marker {
                '-' => "deleted",
                '+' => "inserted",
                _ => "mark",
            };
            Inline::Span(Attr::new("", vec![class.into()], vec![]), children)
        }
    }
}

fn bounded_suffix(text: &str, close: char, invalid: impl Fn(char) -> bool) -> Option<(&str, &str)> {
    let end = text.find(|c| c == close || invalid(c))?;
    (text[end..].starts_with(close)).then(|| (&text[..end], &text[end + close.len_utf8()..]))
}

// Index matching link destinations once so unmatched openings do not rescan the input.
fn parentheses(source: &str) -> HashMap<usize, usize> {
    let mut pairs = HashMap::new();
    let mut stack = Vec::new();
    let mut escaped = false;
    for (index, c) in source.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '(' {
            stack.push(index);
        } else if c == ')'
            && let Some(start) = stack.pop()
        {
            pairs.insert(start, index);
        }
    }
    pairs
}

fn destination(text: &str) -> String {
    let mut url = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let c = if c == '\\' {
            let Some(escaped) = chars.next() else { break };
            escaped
        } else {
            c
        };
        if c != '\n' && !(c == '\r' && chars.peek() == Some(&'\n')) {
            url.push(c);
        }
    }
    url
}

fn collect(tokens: Vec<Token>, positions: Option<&Positions<'_>>) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut plain = Vec::<Inline>::new();
    let mut start = 0;
    let mut end = 0;
    for token in tokens {
        if matches!(token.node, Inline::Str(_) | Inline::Space) {
            if plain.is_empty() {
                start = token.start;
            }
            end = token.end;
            match (plain.last_mut(), token.node) {
                (Some(Inline::Str(last)), Inline::Str(text)) => last.push_str(&text),
                (_, node) => plain.push(node),
            }
        } else {
            out.extend(
                plain
                    .drain(..)
                    .map(|node| positioned(node, positions, start, end)),
            );
            out.push(positioned(token.node, positions, token.start, token.end));
        }
    }
    out.extend(
        plain
            .into_iter()
            .map(|node| positioned(node, positions, start, end)),
    );
    out
}

fn positioned(node: Inline, positions: Option<&Positions<'_>>, start: usize, end: usize) -> Inline {
    let Some(positions) = positions else {
        return node;
    };
    if matches!(&node, Inline::RawInline(format, _) if format.0 == NOTE) {
        return node;
    }
    let Some(pos) = positions.range(start, end) else {
        return node;
    };
    attrs::inline(Attr::new("", vec![], vec![("data-pos".into(), pos)]), node)
}

fn explicit_slot(marker: usize) -> usize {
    if marker == 18 { 19 } else { marker + 8 }
}

#[cfg(test)]
mod tests {
    use super::Positions;

    #[test]
    fn source_positions_use_sparse_checkpoints() {
        let source = format!("{}\n{}", "é".repeat(4096), "a".repeat(2048));
        let positions = Positions::new(&source, [(1, 1), (2, 3)]);
        assert!(positions.checkpoints.len() <= 41);
        assert_eq!(
            positions.range(8188, 8192).as_deref(),
            Some("1:4095-1:4096")
        );
        assert_eq!(positions.range(8192, 8193).as_deref(), Some("1:4097-2:0"));
        assert_eq!(positions.range(8193, 8194).as_deref(), Some("2:3-2:3"));
    }
}
