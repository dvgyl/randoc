mod expand;
mod table;

use randoc_ast::{
    Attr, Block, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, Meta, MetaValue, Pandoc,
    Target,
};
use randoc_core::shared::{IdentifierSet, auto_identifier};
use randoc_core::{Context, Extension, Extensions, ReaderOptions, Result, Sources};

use crate::escape::{Action, Line, args, decode, depth, escape, input_size, source_lines, words};

pub(crate) fn read(sources: &Sources, opts: &ReaderOptions, ctx: &mut Context) -> Result<Pandoc> {
    let size = input_size(sources, ctx)?;
    let lines = expand::expand(source_lines(sources, true), size, ctx)?;
    let mut parser = Parser {
        limit: ctx.limits.max_nesting_depth,
        cells: ctx.limits.max_table_cells,
        meta: Meta::default(),
        ids: IdentifierSet::new(),
        extensions: opts.extensions,
    };
    let blocks = parser.blocks(&mut Cursor::new(&lines), Stop::Document, 0)?;
    Ok(Pandoc {
        meta: parser.meta,
        blocks,
    })
}

struct Parser {
    limit: usize,
    cells: usize,
    meta: Meta,
    ids: IdentifierSet,
    extensions: Extensions,
}

struct Cursor<'a> {
    lines: &'a [Line],
    index: usize,
}

impl<'a> Cursor<'a> {
    fn new(lines: &'a [Line]) -> Self {
        Self { lines, index: 0 }
    }

    fn peek(&self) -> Option<&'a Line> {
        self.lines.get(self.index)
    }

    fn next(&mut self) -> Option<&'a Line> {
        let line = self.peek()?;
        self.index += 1;
        Some(line)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Stop {
    Document,
    Quote,
    Item,
    Definition,
    Indent,
}

fn stop_at(mode: Stop, mac: &str, rest: &str) -> bool {
    match mode {
        Stop::Document => false,
        Stop::Quote => mac == "RE",
        Stop::Item | Stop::Definition | Stop::Indent => {
            matches!(
                mac,
                "SH" | "SS" | "TP" | "TQ" | "HP" | "PP" | "P" | "LP" | "RE"
            ) || (mac == "IP" && (mode == Stop::Indent || !rest.is_empty()))
        }
    }
}

impl Parser {
    fn blocks(&mut self, cursor: &mut Cursor<'_>, mode: Stop, level: usize) -> Result<Vec<Block>> {
        depth(level, self.limit)?;
        let mut blocks = Vec::new();
        let mut para = Vec::new();
        while let Some(line) = cursor.peek() {
            let (mac, rest) = line.request(true);
            if stop_at(mode, mac, rest) {
                break;
            }
            cursor.next();
            match mac {
                "" if line.text == "." || line.text == "'" => {}
                "" if rest.trim().is_empty() => flush(&mut blocks, &mut para),
                "" => append(&mut para, styled(rest, Font::Roman), true),
                "TH" => {
                    flush(&mut blocks, &mut para);
                    for (key, arg) in ["title", "section", "date", "footer", "header"]
                        .into_iter()
                        .zip(args(rest))
                    {
                        self.meta
                            .insert(key, MetaValue::MetaInlines(styled(arg.text, Font::Roman)));
                    }
                }
                "SH" | "SS" => {
                    flush(&mut blocks, &mut para);
                    let content = if rest.is_empty() {
                        cursor
                            .next()
                            .map_or_else(Vec::new, |line| inline_request(cursor, line))
                    } else {
                        request_words(rest)
                    };
                    let identifier = if self.extensions.contains(Extension::AutoIdentifiers) {
                        self.ids.unique(&auto_identifier(&content, self.extensions))
                    } else {
                        String::new()
                    };
                    blocks.push(Block::Header(
                        if mac == "SH" { 1 } else { 2 },
                        Attr {
                            id: identifier,
                            ..Attr::default()
                        },
                        content,
                    ));
                }
                "PP" | "P" | "LP" | "sp" => flush(&mut blocks, &mut para),
                "br" => para.push(Inline::LineBreak),
                "B" | "I" | "SM" | "SB" | "BI" | "BR" | "IB" | "IR" | "RB" | "RI" => {
                    append(&mut para, inline_request(cursor, line), true);
                }
                "TP" | "TQ" | "IP" | "HP" => {
                    flush(&mut blocks, &mut para);
                    if mac == "IP"
                        && rest.is_empty()
                        && (mode == Stop::Item || (mode == Stop::Definition && !blocks.is_empty()))
                    {
                        if blocks.is_empty() {
                            blocks.push(Block::Para(Vec::new()));
                        }
                    } else {
                        self.list(cursor, line, &mut blocks, level + 1)?;
                    }
                }
                "RS" => {
                    flush(&mut blocks, &mut para);
                    let inside = self.blocks(cursor, Stop::Quote, level + 1)?;
                    if cursor
                        .peek()
                        .is_some_and(|line| line.request(true).0 == "RE")
                    {
                        cursor.next();
                    }
                    if matches!(mode, Stop::Item | Stop::Definition) {
                        blocks.extend(inside);
                    } else {
                        blocks.push(Block::BlockQuote(inside));
                    }
                }
                "EX" | "nf" => {
                    flush(&mut blocks, &mut para);
                    blocks.push(code_block(cursor, if mac == "EX" { "EE" } else { "fi" }));
                }
                "UR" | "MT" => {
                    let mut label = Vec::new();
                    let mut suffix = Vec::new();
                    let end = if mac == "UR" { "UE" } else { "ME" };
                    while let Some(line) = cursor.next() {
                        let (mac, rest) = line.request(true);
                        if mac == end {
                            suffix = styled(rest, Font::Roman);
                            break;
                        }
                        append(&mut label, inline_request(cursor, line), false);
                    }
                    let url = decode(rest, true);
                    append(
                        &mut para,
                        vec![Inline::Link(
                            Attr::default(),
                            label,
                            Target::new(
                                if mac == "MT" {
                                    format!("mailto:{url}")
                                } else {
                                    url
                                },
                                "",
                            ),
                        )],
                        true,
                    );
                    para.extend(suffix);
                }
                "SY" => append(&mut para, styled(rest, Font::Bold), true),
                "OP" => {
                    let mut content = vec![Inline::Str("[".into())];
                    for (i, arg) in args(rest).into_iter().enumerate() {
                        append(
                            &mut content,
                            styled(arg.text, if i == 0 { Font::Bold } else { Font::Roman }),
                            true,
                        );
                    }
                    append(&mut content, vec![Inline::Str("]".into())], true);
                    append(&mut para, content, true);
                }
                "YS" => {}
                "TS" => {
                    flush(&mut blocks, &mut para);
                    blocks.push(self.table(cursor, level + 1)?);
                }
                _ => flush(&mut blocks, &mut para),
            }
        }
        flush(&mut blocks, &mut para);
        Ok(blocks)
    }

    fn list(
        &mut self,
        cursor: &mut Cursor<'_>,
        first: &Line,
        blocks: &mut Vec<Block>,
        level: usize,
    ) -> Result<()> {
        depth(level, self.limit)?;
        let kind = list_item(first);
        if kind == Kind::Indent {
            if let Some(line) = cursor.peek()
                && matches!(line.request(true).0, "nf" | "EX")
            {
                cursor.next();
                blocks.push(code_block(
                    cursor,
                    if line.request(true).0 == "nf" {
                        "fi"
                    } else {
                        "EE"
                    },
                ));
                return Ok(());
            }
            let body = self.blocks(cursor, Stop::Indent, level)?;
            if !body.is_empty() {
                blocks.push(Block::BlockQuote(body));
            }
            return Ok(());
        }
        let mut terms = Vec::new();
        let mut items = Vec::new();
        let mut line = first;
        loop {
            let (mac, rest) = line.request(true);
            let term = if kind == Kind::Definition {
                if matches!(mac, "TP" | "TQ") {
                    cursor
                        .next()
                        .map_or_else(Vec::new, |line| inline_request(cursor, line))
                } else {
                    let args = args(rest);
                    words(&decode(args.first().map_or("", |arg| arg.text), true))
                }
            } else {
                Vec::new()
            };
            let mut body = self.blocks(
                cursor,
                if kind == Kind::Definition {
                    Stop::Definition
                } else {
                    Stop::Item
                },
                level,
            )?;
            if body.is_empty()
                || (mac == "IP" && term.is_empty() && !matches!(body.first(), Some(Block::Para(_))))
            {
                body.insert(0, Block::Para(Vec::new()));
            }
            if kind == Kind::Definition {
                terms.push((term, vec![body]));
            } else {
                items.push(body);
            }
            let Some(next) = cursor.peek() else {
                break;
            };
            if !matches!(next.request(true).0, "TP" | "TQ" | "IP") || !kind.same(list_item(next)) {
                break;
            }
            cursor.next();
            line = next;
        }
        blocks.push(match kind {
            Kind::Definition => Block::DefinitionList(terms),
            Kind::Ordered(attributes) => Block::OrderedList(attributes, items),
            _ => Block::BulletList(items),
        });
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Bullet,
    Ordered(ListAttributes),
    Definition,
    Indent,
}

impl Kind {
    fn same(self, other: Self) -> bool {
        match (self, other) {
            (Self::Ordered(a), Self::Ordered(b)) => a.style == b.style && a.delim == b.delim,
            _ => self == other,
        }
    }
}

fn list_item(line: &Line) -> Kind {
    let (mac, rest) = line.request(true);
    if mac != "IP" {
        return if mac == "HP" {
            Kind::Indent
        } else {
            Kind::Definition
        };
    }
    let args = args(rest);
    let Some(arg) = args.first() else {
        return Kind::Indent;
    };
    let tag = decode(arg.text, true);

    if matches!(tag.as_str(), "-" | "*" | "·" | "•" | "−") {
        return Kind::Bullet;
    }
    if let Some(attrs) = ordered(&tag) {
        return Kind::Ordered(attrs);
    }
    Kind::Definition
}

fn ordered(tag: &str) -> Option<ListAttributes> {
    use ListNumberDelim::{OneParen, Period, TwoParens};
    use ListNumberStyle::{Decimal, LowerAlpha, LowerRoman, UpperAlpha, UpperRoman};
    let (number, delim) = if let Some(s) = tag.strip_suffix('.') {
        (s, Period)
    } else {
        let s = tag.strip_suffix(')')?;
        s.strip_prefix('(')
            .map_or((s, OneParen), |s| (s, TwoParens))
    };
    if let Ok(number) = number.parse() {
        return Some(ListAttributes {
            start: number,
            style: Decimal,
            delim,
        });
    }
    let upper = number.bytes().all(|ch| ch.is_ascii_uppercase());
    if matches!(
        number,
        "i" | "ii"
            | "iii"
            | "iv"
            | "v"
            | "vi"
            | "vii"
            | "viii"
            | "ix"
            | "x"
            | "I"
            | "II"
            | "III"
            | "IV"
            | "V"
            | "VI"
            | "VII"
            | "VIII"
            | "IX"
            | "X"
    ) {
        let value = ["i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x"]
            .iter()
            .position(|roman| number.eq_ignore_ascii_case(roman))?;
        return Some(ListAttributes {
            start: value as i64 + 1,
            style: if upper { UpperRoman } else { LowerRoman },
            delim,
        });
    }
    let byte = *number.as_bytes().first()?;
    if number.len() == 1 && byte.is_ascii_alphabetic() {
        return Some(ListAttributes {
            start: i64::from(byte.to_ascii_lowercase() - b'a') + 1,
            style: if upper { UpperAlpha } else { LowerAlpha },
            delim,
        });
    }
    None
}

fn request_words(rest: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    for arg in args(rest) {
        append(&mut out, styled(arg.text, Font::Roman), true);
    }
    out
}

fn inline_request(cursor: &mut Cursor<'_>, line: &Line) -> Vec<Inline> {
    let (mac, rest) = line.request(true);
    let fonts = match mac {
        "B" | "SB" => "B",
        "I" => "I",
        "SM" => "R",
        "BI" | "BR" | "IB" | "IR" | "RB" | "RI" => mac,
        _ => return styled(rest, Font::Roman),
    };
    if rest.is_empty() && fonts.len() == 1 {
        return cursor.next().map_or_else(Vec::new, |line| {
            Font::from(fonts).apply(styled(&line.text, Font::Roman))
        });
    }
    let mut out = Vec::new();
    for (i, arg) in args(rest).into_iter().enumerate() {
        let font = &fonts[i % fonts.len()..][..1];
        let value = styled(arg.text, Font::Roman);
        if fonts.len() == 1 {
            append(&mut out, value, true);
        } else {
            append(
                &mut out,
                Font::from(font).apply(value),
                arg.quoted && arg.text.starts_with([' ', '\t']),
            );
            if arg.quoted
                && arg.text.ends_with([' ', '\t'])
                && !matches!(out.last(), None | Some(Inline::Space))
            {
                out.push(Inline::Space);
            }
        }
    }
    if matches!(out.last(), Some(Inline::Space)) {
        out.pop();
    }
    if fonts.len() == 1 {
        Font::from(fonts).apply(out)
    } else {
        out
    }
}

#[derive(Clone, Copy)]
enum Font {
    Roman,
    Bold,
    Italic,
    BoldItalic,
    Code,
    BoldCode,
    ItalicCode,
}

impl Font {
    fn from(name: &str) -> Self {
        match name {
            "B" | "3" => Self::Bold,
            "I" | "2" => Self::Italic,
            "BI" | "4" => Self::BoldItalic,
            "C" | "CW" | "CR" => Self::Code,
            "CB" => Self::BoldCode,
            "CI" => Self::ItalicCode,
            _ => Self::Roman,
        }
    }

    fn apply(self, inlines: Vec<Inline>) -> Vec<Inline> {
        if inlines.is_empty() {
            return inlines;
        }
        match self {
            Self::Roman => inlines,
            Self::Bold => vec![Inline::Strong(inlines)],
            Self::Italic => vec![Inline::Emph(inlines)],
            Self::BoldItalic => vec![Inline::Strong(vec![Inline::Emph(inlines)])],
            Self::Code | Self::BoldCode | Self::ItalicCode => {
                let code = vec![Inline::Code(
                    Attr::default(),
                    randoc_core::shared::stringify(&inlines),
                )];
                match self {
                    Self::BoldCode => vec![Inline::Strong(code)],
                    Self::ItalicCode => vec![Inline::Emph(code)],
                    _ => code,
                }
            }
        }
    }
}

fn styled(mut text: &str, mut font: Font) -> Vec<Inline> {
    let mut previous = Font::Roman;
    let mut out = Vec::new();
    let mut segment = String::new();
    while let Some((head, rest)) = text.split_once('\\') {
        segment.push_str(head);
        let (tail, action) = escape(rest, true);
        match action {
            Action::Skip => {}
            Action::Char(ch) => segment.push(ch),
            Action::Font(name) => {
                font_segment(&mut out, &segment, font);
                segment.clear();
                let next = if name == "P" || name.is_empty() {
                    previous
                } else {
                    Font::from(name)
                };
                previous = font;
                font = next;
            }
        }
        text = tail;
    }
    segment.push_str(text);
    font_segment(&mut out, &segment, font);
    if matches!(out.last(), Some(Inline::Space)) {
        out.pop();
    }
    out
}

fn font_segment(out: &mut Vec<Inline>, text: &str, font: Font) {
    let value = words(text);
    let starts_space = text.as_bytes().first().is_some_and(u8::is_ascii_whitespace);
    let ends_space = text.as_bytes().last().is_some_and(u8::is_ascii_whitespace);
    append(out, font.apply(value), starts_space);
    if ends_space && !out.is_empty() && !matches!(out.last(), Some(Inline::Space)) {
        out.push(Inline::Space);
    }
}

fn code_block(cursor: &mut Cursor<'_>, until: &str) -> Block {
    let mut lines = Vec::new();
    while let Some(line) = cursor.next() {
        let (mac, _) = line.request(true);
        if mac == until {
            break;
        }
        if line.text != "."
            && !(line.text.starts_with("\\f") && decode(&line.text, true).is_empty())
        {
            lines.push(decode(&line.text, true));
        }
    }
    Block::CodeBlock(Attr::default(), lines.join("\n"))
}

fn append(out: &mut Vec<Inline>, mut value: Vec<Inline>, space: bool) {
    if value.is_empty() {
        return;
    }
    if space && !out.is_empty() && !matches!(out.last(), Some(Inline::Space | Inline::LineBreak)) {
        out.push(Inline::Space);
    }
    if let (Some(Inline::Str(last)), Some(Inline::Str(first))) = (out.last_mut(), value.first()) {
        last.push_str(first);
        out.extend(value.drain(1..));
    } else {
        out.extend(value);
    }
}

fn flush(blocks: &mut Vec<Block>, para: &mut Vec<Inline>) {
    if !para.is_empty() {
        blocks.push(Block::Para(std::mem::take(para)));
    }
}
