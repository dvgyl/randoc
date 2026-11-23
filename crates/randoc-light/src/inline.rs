use std::collections::{BTreeMap, BTreeSet};

use randoc_ast::{Attr, Block, Format as RawFormat, Inline, MathType, QuoteType, Target};
use randoc_core::{Extension, ReaderOptions, Result, shared};

use crate::{Format, check_depth};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    Strong,
    Emph,
    Underline,
    Strike,
    Sup,
    Sub,
    Cite,
    Span,
    Single,
    Double,
    SmallCaps,
    Verbatim,
    StrongEmph,
}

impl Mark {
    fn wrap(self, attr: Attr, xs: Vec<Inline>) -> Inline {
        let xs = if !attr.is_empty() && !matches!(self, Self::Span | Self::Verbatim) {
            vec![Inline::Span(attr.clone(), xs)]
        } else {
            xs
        };
        match self {
            Self::Strong => Inline::Strong(xs),
            Self::Emph => Inline::Emph(xs),
            Self::Underline => Inline::Underline(xs),
            Self::Strike => Inline::Strikeout(xs),
            Self::Sup => Inline::Superscript(xs),
            Self::Sub => Inline::Subscript(xs),
            Self::Cite => Inline::Cite(Vec::new(), xs),
            Self::Span => Inline::Span(attr, xs),
            Self::Single => Inline::Quoted(QuoteType::SingleQuote, xs),
            Self::Double => Inline::Quoted(QuoteType::DoubleQuote, xs),
            Self::SmallCaps => Inline::SmallCaps(xs),
            Self::Verbatim => Inline::Span(attr, xs),
            Self::StrongEmph => Inline::Strong(vec![Inline::Emph(xs)]),
        }
    }
}

const KIND_COUNT: usize = 32;

enum Token<'a> {
    Text(&'a str),
    Marker(&'a str),
    Node(Inline),
    Open { mark: Mark, attr: Attr, end: usize },
    Close,
}

struct Opener {
    kind: usize,
    index: usize,
    previous: Option<usize>,
    start: usize,
    attr: Attr,
}

#[derive(Default)]
struct Matches {
    positions: BTreeMap<char, (Vec<usize>, usize)>,
    equals: (Vec<usize>, usize),
    underscores: (Vec<usize>, usize),
    math_closers: [(Vec<usize>, usize); 2],
    raw_closers: BTreeMap<char, (Vec<usize>, usize)>,
}

impl Matches {
    fn new(input: &str, format: Format) -> Self {
        let mut matches = Self::default();
        for (offset, ch) in input.char_indices() {
            if ch == '=' && input[offset..].starts_with("==") {
                matches.equals.0.push(offset);
            }
            if ch == '_' && input[offset..].starts_with("__") {
                matches.underscores.0.push(offset);
            }
            if "<({[\n)}]>!@;'`\"#=\u{7e}".contains(ch) {
                matches.positions.entry(ch).or_default().0.push(offset);
            }
            if format == Format::Haddock && ch == '\\' {
                match input.as_bytes().get(offset + 1) {
                    Some(b')') => matches.math_closers[0].0.push(offset),
                    Some(b']') => matches.math_closers[1].0.push(offset),
                    _ => {}
                }
            }
            if format == Format::T2t && matches!(ch, '`' | '\'' | '"') {
                let previous = input[..offset].chars().next_back();
                if previous != Some(ch) {
                    let length = input[offset..]
                        .bytes()
                        .take_while(|&byte| byte == ch as u8)
                        .count();
                    if length > 2 || length == 2 && previous.is_some_and(|c| !markup_space(c)) {
                        matches
                            .raw_closers
                            .entry(ch)
                            .or_default()
                            .0
                            .push(offset + length - 2);
                    }
                }
            }
        }
        matches
    }

    fn next(&mut self, ch: char, start: usize) -> Option<usize> {
        let (positions, cursor) = self.positions.get_mut(&ch)?;
        advance(positions, cursor, start)
    }

    fn next_underscores(&mut self, start: usize) -> Option<usize> {
        advance(&self.underscores.0, &mut self.underscores.1, start)
    }

    fn next_equals(&mut self, start: usize) -> Option<usize> {
        advance(&self.equals.0, &mut self.equals.1, start)
    }

    fn raw_close(&mut self, ch: char, start: usize) -> Option<usize> {
        let (positions, cursor) = self.raw_closers.get_mut(&ch)?;
        advance(positions, cursor, start)
    }
}

fn advance(positions: &[usize], cursor: &mut usize, start: usize) -> Option<usize> {
    while positions.get(*cursor).is_some_and(|&offset| offset < start) {
        *cursor += 1;
    }
    positions.get(*cursor).copied()
}

pub(crate) struct Parser<'a> {
    pub(crate) format: Format,
    pub(crate) options: &'a ReaderOptions,
    pub(crate) limit: usize,
    pub(crate) notes: BTreeMap<&'a str, &'a str>,
    pub(crate) links: BTreeMap<&'a str, &'a str>,
    pub(crate) active: BTreeSet<&'a str>,
    pub(crate) budget: usize,
    pub(crate) decode_entities: bool,
    pub(crate) t2t_paths: Option<(&'a str, &'a str)>,
}

pub(crate) fn words(text: &str) -> Vec<Inline> {
    let mut inlines = Vec::new();
    for word in text.split_whitespace() {
        if !inlines.is_empty() {
            inlines.push(Inline::Space);
        }
        inlines.push(Inline::Str(word.into()));
    }
    inlines
}

fn push(xs: &mut Vec<Inline>, x: Inline) {
    if let Inline::Str(text) = x {
        if let Some(Inline::Str(previous)) = xs.last_mut() {
            previous.push_str(&text);
        } else {
            xs.push(Inline::Str(text));
        }
    } else {
        if matches!(x, Inline::LineBreak) && matches!(xs.last(), Some(Inline::Space)) {
            xs.pop();
        }
        if !matches!(x, Inline::Space) || !matches!(xs.last(), Some(Inline::LineBreak)) {
            xs.push(x);
        }
    }
}

fn trim(xs: &mut Vec<Inline>, format: Format) {
    let boundary_space = |x: &Inline| {
        matches!(x, Inline::Space | Inline::SoftBreak)
            || matches!(x, Inline::LineBreak) && format != Format::Muse
    };
    while xs.last().is_some_and(boundary_space) {
        xs.pop();
    }
    let start = xs.iter().take_while(|x| boundary_space(x)).count();
    xs.drain(..start);
}

pub(crate) fn attributes(mut text: &str) -> (Attr, &str) {
    let mut attr = Attr::default();
    while let Some(ch) = text.chars().next() {
        let end = match ch {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            '=' | '>' | '<' => {
                let (align, width) = if text.starts_with("<>") {
                    ("justify", 2)
                } else {
                    (
                        match ch {
                            '=' => "center",
                            '>' => "right",
                            _ => "left",
                        },
                        1,
                    )
                };
                attr.attributes
                    .push(("style".into(), format!("text-align:{align};")));
                text = &text[width..];
                continue;
            }
            _ => break,
        };
        let Some((value, rest)) = text[1..].split_once(end) else {
            break;
        };
        match ch {
            '(' => {
                for word in value.split_whitespace() {
                    if let Some(id) = word.strip_prefix('#') {
                        attr.id = id.into();
                    } else {
                        attr.classes.push(word.into());
                    }
                }
            }
            '[' => attr.attributes.push(("lang".into(), value.into())),
            '{' => attr
                .attributes
                .push(("style".into(), format!("{};", value.trim_end_matches(';')))),
            _ => {}
        }
        text = rest;
    }
    attr.classes.reverse();
    attr.attributes.reverse();
    let style_index = attr.attributes.iter().position(|(name, _)| name == "style");
    let mut style = String::new();
    attr.attributes.retain(|(name, value)| {
        if name == "style" {
            style.push_str(value);
            false
        } else {
            true
        }
    });
    if let Some(index) = style_index {
        attr.attributes.insert(index, ("style".into(), style));
    }
    (attr, text)
}

impl Parser<'_> {
    pub(crate) fn parse(&mut self, text: &str, depth: usize) -> Result<Vec<Inline>> {
        let mut xs = self.parse_unresolved(text, depth, true)?;
        self.resolve_notes(&mut xs, depth)?;
        Ok(xs)
    }

    fn parse_unresolved(
        &mut self,
        text: &str,
        depth: usize,
        autolinks: bool,
    ) -> Result<Vec<Inline>> {
        check_depth(depth, self.limit)?;
        let mut look = Matches::new(text, self.format);
        let mut tokens = Vec::new();
        let mut stack: Vec<Opener> = Vec::new();
        // Each marker indexes its most recent opener. A close removes openers above it once.
        let mut bottom = [None; KIND_COUNT];
        let mut pos = 0;
        let mut prev = None;
        while let Some(ch) = text[pos..].chars().next() {
            let rest = &text[pos..];
            let width = ch.len_utf8();
            let boundary = if self.format == Format::T2t
                && ch == '['
                && matches!(
                    tokens.last(),
                    Some(Token::Node(Inline::Link(..) | Inline::Image(..)))
                ) {
                None
            } else {
                prev
            };
            match self.atom(
                text,
                pos,
                boundary,
                &mut look,
                depth,
                autolinks && stack.is_empty(),
            )? {
                Some((end, x)) => {
                    if self.format == Format::T2t
                        && ch == '"'
                        && let Inline::Str(raw) = x
                    {
                        tokens.extend(words(&raw).into_iter().map(Token::Node));
                    } else {
                        tokens.push(Token::Node(x));
                    }
                    pos = end;
                }
                None => {
                    let bracket = self.format == Format::Textile && rest.starts_with('[');
                    let inner = if bracket {
                        rest[1..].trim_start_matches([' ', '\t'])
                    } else {
                        rest
                    };
                    let bracket_size = rest.len() - inner.len();
                    if let Some((mark, kind, size)) = self.marker(inner) {
                        let after = &inner[size..];
                        let bracket_close =
                            self.format == Format::Textile && after.starts_with(']');
                        let next = after.chars().next();
                        let html_close = inner.starts_with("</");
                        let extra = if self.format == Format::T2t {
                            after.bytes().take_while(|&c| c == ch as u8).count()
                        } else {
                            0
                        };
                        let can_close = (html_close
                            || extra > 0
                            || !inner.starts_with('<')
                                && prev.is_some_and(|c: char| !c.is_whitespace()))
                            && (self.format != Format::Textile
                                || !next.is_some_and(char::is_alphanumeric));
                        let bracket_kind = kind + KIND_COUNT / 2;
                        let close_kind = if bracket_close && bottom[bracket_kind].is_some() {
                            bracket_kind
                        } else {
                            kind
                        };
                        let opener = if !bracket && can_close {
                            bottom[close_kind].filter(|&index: &usize| stack[index].start < pos)
                        } else {
                            None
                        };
                        if let Some(index) = opener {
                            let opening = stack[index].index;
                            let mut attr = Attr::default();
                            while stack.len() > index {
                                if let Some(item) = stack.pop() {
                                    bottom[item.kind] = item.previous;
                                    attr = item.attr;
                                }
                            }
                            if extra > 0 {
                                tokens.push(Token::Text(&text[pos..pos + extra]));
                            }
                            let end = tokens.len();
                            tokens[opening] = Token::Open { mark, attr, end };
                            tokens.push(Token::Close);
                            pos += size + extra + usize::from(close_kind == bracket_kind);
                        } else {
                            let start = pos + size + bracket_size;
                            let (attr, body) = if self.format == Format::Textile {
                                let (attr, body) = inline_attributes(text, start, &mut look);
                                if body.starts_with(char::is_whitespace) {
                                    (Attr::default(), after)
                                } else {
                                    (attr, body)
                                }
                            } else {
                                (Attr::default(), after)
                            };
                            let muse_word = self.format == Format::Muse
                                && inner.starts_with('*')
                                && (prev.is_some_and(char::is_alphanumeric)
                                    || next.is_some_and(|c| ")]}>".contains(c)));
                            let allowed = !html_close
                                && !muse_word
                                && (self.format != Format::T2t || bottom[kind].is_none())
                                && (self.format != Format::Opml || prev.is_none_or(markup_space))
                                && (bracket
                                    || self.format != Format::Textile
                                    || !prev.is_some_and(char::is_alphanumeric))
                                && body
                                    .chars()
                                    .next()
                                    .is_some_and(|c| !c.is_whitespace() || bracket);
                            let kind = if bracket { bracket_kind } else { kind };
                            let end = if allowed
                                && (attr.is_empty() || !body.starts_with(char::is_whitespace))
                            {
                                start + after.len() - body.len()
                            } else {
                                start
                            };
                            if allowed {
                                let previous = bottom[kind];
                                bottom[kind] = Some(stack.len());
                                stack.push(Opener {
                                    kind,
                                    index: tokens.len(),
                                    previous,
                                    start: end,
                                    attr,
                                });
                            }
                            if allowed && bracket_size > 1 {
                                tokens.push(Token::Node(Inline::Space));
                                if let Some(opener) = stack.last_mut() {
                                    opener.index += 1;
                                }
                            }
                            if self.smart()
                                && matches!(mark, Mark::Single | Mark::Double)
                                && !allowed
                            {
                                tokens.push(Token::Node(Inline::Str(
                                    if ch == '\'' {
                                        "’"
                                    } else if self.format == Format::Opml {
                                        if matches!(prev, Some('.' | '-' | '/')) {
                                            "“"
                                        } else {
                                            "”"
                                        }
                                    } else {
                                        "\""
                                    }
                                    .into(),
                                )));
                            } else {
                                tokens.push(Token::Marker(&text[pos..end]));
                            }
                            pos = end;
                        }
                    } else if markup_space(ch)
                        && (!matches!(self.format, Format::T2t)
                            || ch != '\t'
                            || prev.is_some_and(|c| "|-+:*".contains(c)))
                    {
                        let end = rest.find(|c: char| !markup_space(c)).unwrap_or(rest.len());
                        let spaces = &rest[..end];
                        let x = if spaces.contains('\n') {
                            if self.format == Format::Textile {
                                Inline::LineBreak
                            } else {
                                Inline::SoftBreak
                            }
                        } else {
                            Inline::Space
                        };
                        let x = if self.smart() && matches!(x, Inline::Space) && prev == Some('.') {
                            let word = text[..pos]
                                .rsplit_once(char::is_whitespace)
                                .map_or(&text[..pos], |(_, word)| word);
                            if word.ends_with('.')
                                && super::read::is_abbreviation(word, self.options)
                            {
                                Inline::Str("\u{a0}".into())
                            } else {
                                x
                            }
                        } else {
                            x
                        };
                        tokens.push(Token::Node(x));
                        pos += end;
                    } else {
                        if let Some(Token::Text(previous)) = tokens.last_mut() {
                            let start = pos - previous.len();
                            *previous = &text[start..pos + width];
                        } else {
                            tokens.push(Token::Text(&rest[..width]));
                        }
                        pos += width;
                    }
                }
            }
            prev = text[..pos].chars().next_back();
        }
        let mut frames: Vec<(Mark, Attr, usize, Vec<Inline>)> = Vec::new();
        let mut out = Vec::new();
        for (index, token) in tokens.into_iter().enumerate() {
            let xs = frames.last_mut().map_or(&mut out, |frame| &mut frame.3);
            match token {
                Token::Text(s) | Token::Marker(s) => push(
                    xs,
                    Inline::Str(if self.smart() && s == "'" {
                        "’".into()
                    } else if self.smart() && self.format == Format::Opml && s == "\"" {
                        "“".into()
                    } else {
                        s.into()
                    }),
                ),
                Token::Node(x) => push(xs, x),
                Token::Open { mark, attr, end } => {
                    check_depth(depth.saturating_add(frames.len() + 1), self.limit)?;
                    frames.push((mark, attr, end, Vec::new()));
                }
                Token::Close => {
                    if let Some((mark, attr, end, mut children)) = frames.pop() {
                        debug_assert_eq!(index, end);
                        trim(&mut children, self.format);
                        let xs = frames.last_mut().map_or(&mut out, |frame| &mut frame.3);
                        push(xs, mark.wrap(attr, children));
                    }
                }
            }
        }
        trim(&mut out, self.format);
        Ok(out)
    }

    fn resolve_notes(&mut self, xs: &mut [Inline], depth: usize) -> Result<()> {
        // Note references contain a temporary raw block with the definition key.
        enum Work<'a, 'n> {
            Inline(&'a mut Inline, usize),
            Leave(&'n str),
        }
        let mut work = xs
            .iter_mut()
            .rev()
            .map(|x| Work::Inline(x, depth))
            .collect::<Vec<_>>();
        while let Some(event) = work.pop() {
            let (x, depth) = match event {
                Work::Leave(id) => {
                    self.active.remove(id);
                    continue;
                }
                Work::Inline(x, depth) => (x, depth),
            };
            check_depth(depth, self.limit)?;
            let note = if let Inline::Note(blocks) = &*x
                && let [Block::RawBlock(_, id)] = blocks.as_slice()
            {
                self.notes
                    .get_key_value(id.as_str())
                    .map(|(&id, &body)| (id, body))
            } else {
                None
            };
            if let Some((id, body)) = note {
                if !self.active.insert(id) {
                    *x = Inline::Str(format!("[{id}]"));
                    continue;
                }
                let next = depth + 1;
                check_depth(next, self.limit)?;
                self.budget = self.budget.checked_sub(body.len().max(1)).ok_or_else(|| {
                    randoc_core::Error::App("Expanded notes are too large.".into())
                })?;
                let mut paragraphs = Vec::new();
                if self.format == Format::Muse {
                    let mut start = 0;
                    let mut offset = 0;
                    for line in body.split_inclusive('\n').chain(std::iter::once("")) {
                        if line.trim().is_empty() {
                            let text = body[start..offset].trim();
                            if !text.is_empty() {
                                paragraphs
                                    .push(Block::Para(self.parse_unresolved(text, next, true)?));
                            }
                            start = offset + line.len();
                        }
                        offset += line.len();
                    }
                } else {
                    paragraphs.push(Block::Para(self.parse_unresolved(body, next, true)?));
                }
                *x = Inline::Note(paragraphs);
                work.push(Work::Leave(id));
            }
            let xs = match x {
                Inline::Note(blocks) => {
                    for block in blocks.iter_mut().rev() {
                        if let Block::Para(xs) = block {
                            work.extend(xs.iter_mut().rev().map(|x| Work::Inline(x, depth + 1)));
                        }
                    }
                    continue;
                }
                Inline::Emph(xs)
                | Inline::Strong(xs)
                | Inline::Underline(xs)
                | Inline::Strikeout(xs)
                | Inline::Superscript(xs)
                | Inline::Subscript(xs)
                | Inline::SmallCaps(xs)
                | Inline::Quoted(_, xs)
                | Inline::Cite(_, xs)
                | Inline::Span(_, xs)
                | Inline::Link(_, xs, _)
                | Inline::Image(_, xs, _) => xs,
                _ => continue,
            };
            work.extend(xs.iter_mut().rev().map(|x| Work::Inline(x, depth + 1)));
        }
        Ok(())
    }

    fn smart(&self) -> bool {
        matches!(self.format, Format::Textile | Format::Opml)
            && self.options.extensions.contains(Extension::Smart)
    }

    fn marker(&self, rest: &str) -> Option<(Mark, usize, usize)> {
        use Mark::*;
        let markers: &[(&str, Mark)] = match self.format {
            Format::Textile => &[
                ("**", Strong),
                ("__", Emph),
                ("??", Cite),
                ("*", Strong),
                ("_", Emph),
                ("+", Underline),
                ("-", Strike),
                ("^", Sup),
                ("~", Sub),
                ("%", Span),
                ("'", Single),
                ("\"", Double),
            ],
            Format::Muse => &[
                ("***", StrongEmph),
                ("**", Strong),
                ("*", Emph),
                ("<strong>", Strong),
                ("<em>", Emph),
                ("<u>", Underline),
                ("<del>", Strike),
                ("<sup>", Sup),
                ("<sub>", Sub),
                ("<sc>", SmallCaps),
                ("<verbatim>", Verbatim),
            ],
            Format::T2t => &[
                ("**", Strong),
                ("//", Emph),
                ("__", Underline),
                ("--", Strike),
            ],
            Format::Haddock => &[("__", Strong), ("/", Emph)],
            Format::Opml => &[("'", Single), ("\"", Double)],
            _ => &[],
        };
        markers
            .iter()
            .enumerate()
            .find_map(|(index, (prefix, mark))| {
                if matches!(mark, Single | Double) && !self.smart() {
                    return None;
                }
                if rest.starts_with(prefix) {
                    Some((*mark, index, prefix.len()))
                } else if let Some(tail) = prefix.strip_prefix('<')
                    && rest
                        .strip_prefix("</")
                        .is_some_and(|rest| rest.starts_with(tail))
                {
                    Some((*mark, index, prefix.len() + 1))
                } else {
                    None
                }
            })
    }
}

impl Parser<'_> {
    fn atom(
        &mut self,
        text: &str,
        pos: usize,
        prev: Option<char>,
        look: &mut Matches,
        depth: usize,
        top_level: bool,
    ) -> Result<Option<(usize, Inline)>> {
        let rest = &text[pos..];
        let Some(ch) = rest.chars().next() else {
            return Ok(None);
        };
        let textile = self.format == Format::Textile;
        let haddock = self.format == Format::Haddock;
        let muse = self.format == Format::Muse;
        let t2t = self.format == Format::T2t;
        if muse && depth == 0 && ch == '#' && (pos == 0 || prev == Some('\n')) {
            let suffix = &rest[1..];
            let end = suffix.find(char::is_whitespace).unwrap_or(suffix.len());
            let id = &suffix[..end];
            if !id.is_empty() {
                let tail = suffix[end..].trim_start_matches([' ', '\t']);
                return Ok(Some((
                    pos + rest.len() - tail.len(),
                    Inline::Span(Attr::new(id, vec![], vec![]), vec![]),
                )));
            }
        }
        if self.format == Format::Opml && self.smart() && rest.starts_with("\"\"") {
            return Ok(Some((
                pos + 2,
                Inline::Quoted(QuoteType::DoubleQuote, Vec::new()),
            )));
        }
        if self.format == Format::Opml
            && self.smart()
            && ch == '\''
            && prev.is_some_and(char::is_alphanumeric)
            && rest[1..].starts_with(char::is_alphanumeric)
        {
            return Ok(Some((pos + 1, Inline::Str("’".into()))));
        }
        if muse && rest.starts_with("<br>") {
            return Ok(Some((pos + 4, Inline::LineBreak)));
        }
        if haddock && ch == '\\' {
            let math = match rest.as_bytes().get(1) {
                Some(b'(') => Some((0, MathType::InlineMath)),
                Some(b'[') => Some((1, MathType::DisplayMath)),
                _ => None,
            };
            if let Some((kind, math)) = math {
                let (positions, cursor) = &mut look.math_closers[kind];
                if let Some(end) = advance(positions, cursor, pos + 2)
                    && end > pos + 2
                    && look.next('\n', pos + 2).is_none_or(|line| line > end)
                {
                    return Ok(Some((
                        end + 2,
                        Inline::Math(math, text[pos + 2..end].into()),
                    )));
                }
            }
            return Ok(rest[1..]
                .chars()
                .next()
                .map(|next| (pos + 1 + next.len_utf8(), Inline::Str(next.to_string()))));
        }
        if self.decode_entities
            && ch == '&'
            && !t2t
            && let Some(end) = look.next(';', pos + 1)
            && end - pos <= 40
        {
            let name = &text[pos + 1..end];
            let entity = if let Some(number) = name.strip_prefix('#') {
                number
                    .strip_prefix(['x', 'X'])
                    .map_or_else(
                        || shared::numeric_entity(number, false),
                        |hex| shared::numeric_entity(hex, true),
                    )
                    .map(|c| c.to_string())
            } else if haddock {
                None
            } else {
                shared::named_entity(name).map(str::to_owned)
            };
            if let Some(entity) = entity {
                return Ok(Some((end + 1, Inline::Str(entity))));
            }
        }
        if textile {
            if rest.starts_with("__*") && look.next_underscores(pos + 2).is_none() {
                return Ok(Some((pos + 2, Inline::Emph(Vec::new()))));
            }
            for (pattern, symbol) in [("(tm)", "™"), ("(r)", "®"), ("(c)", "©")] {
                if rest
                    .get(..pattern.len())
                    .is_some_and(|s| s.eq_ignore_ascii_case(pattern))
                {
                    return Ok(Some((pos + pattern.len(), Inline::Str(symbol.into()))));
                }
            }
            if ch.is_ascii_uppercase() && !prev.is_some_and(char::is_alphanumeric) {
                let len = rest.bytes().take_while(u8::is_ascii_uppercase).count();
                if len >= 2
                    && rest[len..].starts_with('(')
                    && let Some(end) = look.next(')', pos + len + 1)
                    && !text[pos + len + 1..end].contains(['(', '\n'])
                {
                    return Ok(Some((
                        end + 1,
                        Inline::Str(format!("{} {}", &rest[..len], &text[pos + len..=end])),
                    )));
                }
            }
            if self.smart() {
                if ch == '-'
                    && text[..pos].ends_with('.')
                    && rest[1..].starts_with(|c: char| c.is_ascii_digit())
                {
                    return Ok(Some((pos + 1, Inline::Str("–".into()))));
                }
                if rest.starts_with("...") {
                    return Ok(Some((pos + 3, Inline::Str("…".into()))));
                }
                if rest.starts_with("--") {
                    let (len, dash) = if self.options.extensions.contains(Extension::OldDashes) {
                        (2, "—")
                    } else if rest.starts_with("---") {
                        (3, "—")
                    } else {
                        (2, "–")
                    };
                    return Ok(Some((pos + len, Inline::Str(dash.into()))));
                }
                if ch == '\''
                    && (prev.is_some_and(char::is_alphanumeric)
                        && rest[1..].starts_with(char::is_alphanumeric)
                        || rest.starts_with("'("))
                {
                    return Ok(Some((pos + 1, Inline::Str("’".into()))));
                }
            }
            if rest.starts_with("==")
                && let Some(end) = look.next_equals(pos + 2)
            {
                return Ok(Some((end + 2, Inline::Str(text[pos + 2..end].into()))));
            }
        }
        // Find the first closer for each atomic delimiter with a forward cursor.
        if (textile && ch == '@' || muse && ch == '=')
            && let Some(end) = look.next(ch, pos + 1)
            && end > pos + 1
            && look.next('\n', pos + 1).is_none_or(|line| line > end)
        {
            return Ok(Some((
                end + 1,
                Inline::Code(Attr::default(), text[pos + 1..end].into()),
            )));
        }
        if haddock
            && matches!(ch, '@' | '"' | '\'' | '`' | '#')
            && (matches!(ch, '@' | '#') || !prev.is_some_and(char::is_alphanumeric))
            && let Some(end) = look.next(ch, pos + 1)
            && let Some(body) = text.get(pos + 1..end)
            && !body.is_empty()
        {
            let attr = match ch {
                '#' => {
                    return Ok(Some((
                        end + 1,
                        Inline::Span(Attr::new(body, vec!["anchor".into()], vec![]), vec![]),
                    )));
                }
                '"' if haddock_module(body) => {
                    Attr::new("", vec!["haskell".into(), "module".into()], vec![])
                }
                '\'' | '`' if !body.contains(char::is_whitespace) => {
                    Attr::new("", vec!["haskell".into(), "identifier".into()], vec![])
                }
                '@' => Attr::default(),
                _ => return Ok(None),
            };
            return Ok(Some((
                end + 1,
                Inline::Code(attr, body.trim_matches('\n').into()),
            )));
        }
        if t2t && matches!(ch, '*' | '/' | '_' | '-') && !prev.is_some_and(|c| c == ch) {
            let run = rest.bytes().take_while(|&c| c == ch as u8).count();
            if run >= 5 {
                let mark = match ch {
                    '*' => Mark::Strong,
                    '/' => Mark::Emph,
                    '_' => Mark::Underline,
                    _ => Mark::Strike,
                };
                let next = rest[run..].trim_start_matches([' ', '\t']);
                let second = if next.len() < rest.len() - run {
                    next.bytes().take_while(|&c| c == ch as u8).count()
                } else {
                    0
                };
                let punctuation = next
                    .chars()
                    .next()
                    .filter(|c| !c.is_alphanumeric() && !c.is_whitespace() && *c != ch);
                let attached = punctuation.map_or(0, |p| {
                    next[p.len_utf8()..]
                        .bytes()
                        .take_while(|&c| c == ch as u8)
                        .count()
                });
                let (end, xs) = if second >= 3 {
                    let end = rest.len() - next.len() + second;
                    (
                        end,
                        vec![
                            Inline::Str(rest[2..run].into()),
                            Inline::Space,
                            Inline::Str(next[..second - 2].into()),
                        ],
                    )
                } else if attached >= 3 && next.len() == rest.len() - run {
                    let end = run + punctuation.map_or(0, char::len_utf8) + attached;
                    (end, vec![Inline::Str(rest[2..end - 2].into())])
                } else {
                    (run, vec![Inline::Str(rest[2..run - 2].into())])
                };
                return Ok(Some((pos + end, mark.wrap(Attr::default(), xs))));
            }
        }
        if let Some((input, output)) = self.t2t_paths
            && rest.starts_with("%%")
        {
            for (name, value) in [("%%infile", input), ("%%outfile", output)] {
                if let Some(tail) = rest.strip_prefix(name)
                    && !tail.starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '(')
                {
                    self.budget = self.budget.checked_sub(value.len()).ok_or_else(|| {
                        randoc_core::Error::App("The input expands to too much text.".into())
                    })?;
                    return Ok(Some((pos + name.len(), Inline::Str(value.into()))));
                }
            }
        }
        if t2t
            && prev.is_none_or(markup_space)
            && (rest.starts_with("``") || rest.starts_with("\"\"") || rest.starts_with("''"))
            && let Some(end) = look.raw_close(ch, pos + 3)
            && look.next('\n', pos + 2).is_none_or(|line| line > end)
            && !text[pos + 2..end].starts_with(char::is_whitespace)
            && !text[pos + 2..end].ends_with(char::is_whitespace)
        {
            let body = &text[pos + 2..end];
            let node = match ch {
                '`' => Inline::Code(Attr::default(), body.into()),
                '\'' => Inline::RawInline(RawFormat::new("html"), body.into()),
                _ => Inline::Str(body.into()),
            };
            return Ok(Some((end + 2, node)));
        }
        if (textile || muse)
            && ch == '['
            && let Some(end) = look.next(']', pos + 1)
        {
            let name = &text[pos + 1..end];
            if name.bytes().all(|c| c.is_ascii_digit()) && self.notes.contains_key(name) {
                return Ok(Some((
                    end + 1,
                    Inline::Note(vec![Block::RawBlock(RawFormat::new(""), name.into())]),
                )));
            }
        }
        let bracket_link = textile && rest.starts_with("[\"");
        if textile
            && (ch == '"' || bracket_link)
            && let Some(start) = pos.checked_add(1 + usize::from(bracket_link))
            && let Some(end) = look.next('"', start)
            && let Some(url) = text[end + 1..].strip_prefix(':')
            && url.starts_with(|c: char| !c.is_whitespace())
        {
            let len = url.find(char::is_whitespace).unwrap_or(url.len());
            let len = if bracket_link {
                look.next(']', end + 2)
                    .map_or(len, |close| (close - end - 2).min(len))
            } else {
                len
            };
            let target = url[..len].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']']);
            if !target.is_empty() {
                let url = self.links.get(target).copied().unwrap_or(target);
                let label = &text[start..end];
                let xs = if label == "$" {
                    vec![Inline::Str(url.into())]
                } else {
                    self.parse_unresolved(label, depth + 1, true)?
                };
                return Ok(Some((
                    end + 2
                        + target.len()
                        + usize::from(
                            bracket_link
                                && target.len() == len
                                && text[end + 2 + len..].starts_with(']'),
                        ),
                    Inline::Link(Attr::default(), xs, Target::new(url, "")),
                )));
            }
        }
        if textile
            && ch == '!'
            && let Some(end) = look.next('!', pos + 1)
            && let Some(body) = text.get(pos + 1..end)
        {
            let (_, body) = attributes(body);
            let (url, alt) = body
                .split_once('(')
                .map_or((body, ""), |(url, alt)| (url, alt.trim_end_matches(')')));
            if !url.is_empty() && !url.contains(char::is_whitespace) {
                let mut result = Inline::Image(
                    Attr::default(),
                    vec![Inline::Str(alt.into())],
                    Target::new(url, alt),
                );
                let mut end = end + 1;
                if let Some(url) = text[end..].strip_prefix(':') {
                    let url = url.split_whitespace().next().unwrap_or("");
                    let link = url.trim_end_matches(['.', ',', ';']);
                    if !link.is_empty() {
                        result = Inline::Link(Attr::default(), vec![result], Target::new(link, ""));
                        end += link.len() + 1;
                    }
                }
                return Ok(Some((end, result)));
            }
        }
        if muse
            && rest.starts_with("[[")
            && let Some(end) = look.next(']', pos + 2)
        {
            let source = &text[pos + 2..end];
            let (url, force_link) = source
                .strip_prefix("URL:")
                .map_or((source, false), |url| (url, true));
            let tail = &text[end + 1..];
            let mut label = None;
            let mut finish = end + 1;
            if tail.starts_with('[')
                && let Some(close) = look.next(']', end + 2)
            {
                label = Some(&text[end + 2..close]);
                finish = close + 1;
            }
            if text[finish..].starts_with(']') {
                let target = Target::new(url, "");
                let x = if !force_link && is_image(url) {
                    let label = self.parse_unresolved(label.unwrap_or(""), depth + 1, true)?;
                    Inline::Image(Attr::default(), label, target)
                } else {
                    let label = self.parse_unresolved(label.unwrap_or(url), depth + 1, true)?;
                    Inline::Link(Attr::default(), label, target)
                };
                return Ok(Some((finish + 1, x)));
            }
        }
        if t2t
            && prev.is_none_or(markup_space)
            && rest.starts_with("[[")
            && let Some(end) = look.next(']', pos + 2)
            && let Some(path) = text.get(pos + 2..end).filter(|path| is_image(path))
            && let Some(url) = text[end + 1..].strip_prefix(' ')
            && let Some(close) = look.next(']', end + 2)
            && let Some(url) = url
                .get(..close - end - 2)
                .filter(|url| !url.is_empty() && !url.contains(char::is_whitespace))
        {
            let image = Inline::Image(Attr::default(), vec![], Target::new(path, ""));
            return Ok(Some((
                close + 1,
                Inline::Link(Attr::default(), vec![image], Target::new(url, "")),
            )));
        }
        if t2t
            && prev.is_none_or(markup_space)
            && ch == '['
            && let Some(end) = look.next(']', pos + 1)
            && look.next('[', pos + 1).is_none_or(|next| next > end)
        {
            let body = &text[pos + 1..end];
            if let Some((label, url)) = body.rsplit_once(' ').filter(|(label, url)| {
                !label.is_empty()
                    && !label.starts_with(char::is_whitespace)
                    && url.starts_with(|c: char| c.is_ascii_alphabetic() || "/#.".contains(c))
                    && !label.contains(['[', '\n'])
            }) {
                let mut xs = self.parse_unresolved(label, depth + 1, false)?;
                if label.ends_with(char::is_whitespace) {
                    xs.push(Inline::Space);
                }
                return Ok(Some((
                    end + 1,
                    Inline::Link(Attr::default(), xs, Target::new(url, "")),
                )));
            }
            if !body.contains(char::is_whitespace) && is_image(body) {
                return Ok(Some((
                    end + 1,
                    Inline::Image(Attr::default(), vec![], Target::new(body, "")),
                )));
            }
        }
        if t2t
            && top_level
            && prev.is_none_or(char::is_whitespace)
            && (rest.starts_with("https://")
                || rest.starts_with("http://")
                || rest.starts_with("ftp://")
                || ch.is_ascii_alphanumeric())
        {
            let word = rest
                .split(|c: char| c.is_whitespace() || c == ']')
                .next()
                .unwrap_or("");
            let url = word.trim_end_matches([')', ']']);
            if url.starts_with("https://")
                || url.starts_with("http://")
                || url.starts_with("ftp://")
            {
                let url = if let Some((base, fragment)) = url.rsplit_once('#') {
                    let prefix = fragment
                        .bytes()
                        .take_while(|byte| byte.is_ascii_alphanumeric() || b"_.,".contains(byte))
                        .count();
                    &url[..base.len() + 1 + prefix]
                } else {
                    url
                };
                return Ok(Some((
                    pos + url.len(),
                    Inline::Link(
                        Attr::default(),
                        vec![Inline::Str(url.into())],
                        Target::new(url, ""),
                    ),
                )));
            }
            let host = url.get(..3);
            let web_host = host.is_some_and(|host| host.eq_ignore_ascii_case("www"))
                && url[3..]
                    .trim_start_matches(|c: char| c.is_ascii_digit())
                    .starts_with('.');
            let ftp_host = host.is_some_and(|host| host.eq_ignore_ascii_case("ftp"))
                && url[3..].starts_with('.');
            if (web_host || ftp_host) && url.len() > 4 {
                return Ok(Some((
                    pos + url.len(),
                    Inline::Link(
                        Attr::default(),
                        vec![Inline::Str(url.into())],
                        Target::new(url, ""),
                    ),
                )));
            }
            let url = url.trim_end_matches(['.', ',', ';']);
            if let Some((user, host)) = url.split_once('@')
                && !user.is_empty()
                && host.contains('.')
                && !host.starts_with('.')
            {
                return Ok(Some((
                    pos + url.len(),
                    Inline::Link(
                        Attr::default(),
                        vec![Inline::Str(format!("mailto:{url}"))],
                        Target::new(url, ""),
                    ),
                )));
            }
        }
        for (prefix, suffix, code) in match self.format {
            Format::Textile => &[("<tt>", "</tt>", true)][..],
            Format::Muse => &[
                ("<code>", "</code>", true),
                ("<verbatim>", "</verbatim>", false),
            ][..],
            _ => &[],
        } {
            if let Some(value) = rest.strip_prefix(prefix)
                && let Some(end) = look.next('<', pos + prefix.len())
                && text[end..].starts_with(suffix)
            {
                let value = &value[..end - pos - prefix.len()];
                let xs = if *code {
                    Inline::Code(Attr::default(), value.into())
                } else {
                    Inline::Str(value.into())
                };
                return Ok(Some((end + suffix.len(), xs)));
            }
        }
        if muse
            && rest.starts_with("<literal")
            && let Some(end) = look.next('>', pos + 8)
            && look.next('<', pos + 8).is_none_or(|next| next > end)
        {
            let attr = &text[pos + 8..end];
            let format = attr
                .split_once("style=")
                .and_then(|(_, value)| value.trim().strip_prefix('"'))
                .and_then(|value| value.split_once('"').map(|(value, _)| value))
                .unwrap_or("html");
            if let Some(close) = look.next('<', end + 1)
                && text[close..].starts_with("</literal>")
            {
                return Ok(Some((
                    close + 10,
                    Inline::RawInline(RawFormat::new(format), text[end + 1..close].into()),
                )));
            }
        }
        if ch == '<'
            && let Some(end) = look.next('>', pos + 1)
        {
            let body = &text[pos + 1..end];
            if haddock
                && !body.is_empty()
                && !body.starts_with("<<<")
                && look.next('\n', pos + 1).is_none_or(|line| line > end)
            {
                let image = body.starts_with('<') && text[end + 1..].starts_with('>');
                let body = if image { &body[1..] } else { body };
                if look
                    .next('<', pos + 1 + usize::from(image))
                    .is_some_and(|next| next < end)
                {
                    return Ok(None);
                }
                let (url, label) = body.split_once(char::is_whitespace).unwrap_or((body, body));
                if !url.is_empty() && !url.contains('<') {
                    let label = if image && label == url {
                        ""
                    } else {
                        label.trim()
                    };
                    let xs = if !image && label == url {
                        vec![Inline::Str(url.into())]
                    } else {
                        self.parse_unresolved(label, depth + 1, true)?
                    };
                    let node = if image {
                        Inline::Image(
                            Attr::default(),
                            xs,
                            Target::new(url, if label.is_empty() { url } else { label }),
                        )
                    } else {
                        Inline::Link(Attr::default(), xs, Target::new(url, url))
                    };
                    return Ok(Some((end + 1 + usize::from(image), node)));
                }
            }
            if textile
                && body
                    .trim_start_matches('/')
                    .starts_with(|c: char| c.is_ascii_alphabetic() || c == '!')
                && look.next('<', pos + 1).is_none_or(|next| next > end)
            {
                return Ok(Some((
                    end + 1,
                    Inline::RawInline(RawFormat::new("html"), text[pos..=end].into()),
                )));
            }
        }
        Ok(None)
    }
}

fn haddock_module(body: &str) -> bool {
    let (module, anchor) = body.split_once('#').unwrap_or((body, ""));
    module.starts_with(char::is_uppercase)
        && module
            .chars()
            .all(|c| c.is_alphanumeric() || "._'".contains(c))
        && !anchor.contains(char::is_whitespace)
}

fn is_image(url: &str) -> bool {
    url.rsplit_once('.').is_some_and(|(_, suffix)| {
        ["png", "jpg", "jpeg", "gif", "svg", "bmp", "webp"]
            .iter()
            .any(|ext| suffix.eq_ignore_ascii_case(ext))
    })
}

pub(crate) fn markup_space(ch: char) -> bool {
    ch.is_whitespace() && ch != '\u{a0}'
}

fn inline_attributes<'a>(text: &'a str, start: usize, look: &mut Matches) -> (Attr, &'a str) {
    let mut end = start;
    while let Some(ch) = text[end..].chars().next() {
        let closer = match ch {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            _ => break,
        };
        let Some(close) = look.next(closer, end + 1) else {
            break;
        };
        if look.next(ch, end + 1).is_some_and(|next| next < close)
            || look.next('\n', end + 1).is_some_and(|line| line < close)
        {
            break;
        }
        end = close + 1;
    }
    let (attr, remaining) = attributes(&text[start..end]);
    (attr, &text[end - remaining.len()..])
}
