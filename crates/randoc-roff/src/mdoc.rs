use randoc_ast::{
    Attr, Block, Cell, ColSpec, Inline, ListAttributes, Meta, MetaValue, Pandoc, QuoteType, Row,
    Table, TableBody, Target,
};
use randoc_core::{Context, Error, ReaderOptions, Result, Sources};

use crate::escape::{
    Arg, Line, args, attr, charge_cells, decode, depth, input_size, push_str, source_lines, words,
};

pub(crate) fn read(sources: &Sources, _: &ReaderOptions, ctx: &mut Context) -> Result<Pandoc> {
    input_size(sources, ctx)?;
    let lines = source_lines(sources, false);
    let mut reader = Parser {
        lines: &lines,
        i: 0,
        limit: ctx.limits.max_nesting_depth,
        cells: ctx.limits.max_table_cells,
        name: String::new(),
        section: String::new(),
        spacing: true,
        author_split: false,
    };
    let meta = reader.prologue()?;
    Ok(Pandoc {
        meta,
        blocks: reader.blocks(&[], 0)?,
    })
}

struct Parser<'a> {
    lines: &'a [Line],
    i: usize,
    limit: usize,
    cells: usize,
    name: String,
    section: String,
    spacing: bool,
    author_split: bool,
}

fn standard_name(key: &str) -> &'static str {
    match key {
        "-ansiC" | "-ansiC-89" => "ANSI X3.159-1989 (“ANSI C89”)",
        "-isoC" | "-isoC-90" => "ISO/IEC 9899:1990 (“ISO C90”)",
        "-isoC-amd1" => "ISO/IEC 9899/AMD1:1995 (“ISO C90, Amendment 1”)",
        "-isoC-tcor1" => "ISO/IEC 9899/TCOR1:1994 (“ISO C90, Technical Corrigendum 1”)",
        "-isoC-tcor2" => "ISO/IEC 9899/TCOR2:1995 (“ISO C90, Technical Corrigendum 2”)",
        "-isoC-2023" => "ISO/IEC 9899:2024 (“ISO C23”)",
        "-ieee1275-94" => "IEEE Std 1275-1994 (“Open Firmware”)",
        "-isoC-99" => "ISO/IEC 9899:1999 (“ISO C99”)",
        "-isoC-2011" => "ISO/IEC 9899:2011 (“ISO C11”)",
        "-iso9945-1-90" => "ISO/IEC 9945-1:1990 (“POSIX.1”)",
        "-iso9945-1-96" | "-p1003.1-96" => "ISO/IEC 9945-1:1996 (“POSIX.1”)",
        "-iso9945-2-93" => "ISO/IEC 9945-2:1993 (“POSIX.2”)",
        "-ieee754" => "IEEE Std 754-1985",
        "-iso8802-3" => "ISO 8802-3: 1989",
        "-iso8601" => "ISO 8601",
        "-svid4" => "System V Interface Definition, Fourth Edition (“SVID4”)",
        "-xbd5" => "X/Open Base Definitions Issue 5 (“XBD5”)",
        "-xcu5" => "X/Open Commands and Utilities Issue 5 (“XCU5”)",
        "-xsh5" => "X/Open System Interfaces and Headers Issue 5 (“XSH5”)",
        "-xns5" => "X/Open Networking Services Issue 5 (“XNS5”)",
        "-xns5.2" => "X/Open Networking Services Issue 5.2 (“XNS5.2”)",
        "-xpg3" => "X/Open Portability Guide Issue 3 (“XPG3”)",
        "-xpg4" => "X/Open Portability Guide Issue 4 (“XPG4”)",
        "-xpg4.2" => "X/Open Portability Guide Issue 4, Version 2 (“XPG4.2”)",
        "-p1003.1" => "IEEE Std 1003.1 (“POSIX.1”)",
        "-p1003.1-88" => "IEEE Std 1003.1-1988 (“POSIX.1”)",
        "-p1003.1-90" => "IEEE Std 1003.1-1990 (“POSIX.1”)",
        "-p1003.1-2001" => "IEEE Std 1003.1-2001 (“POSIX.1”)",
        "-p1003.1-2004" => "IEEE Std 1003.1-2004 (“POSIX.1”)",
        "-p1003.1-2008" => "IEEE Std 1003.1-2008 (“POSIX.1”)",
        "-p1003.1-2024" => "IEEE Std 1003.1-2024 (“POSIX.1”)",
        "-p1003.1b" => "IEEE Std 1003.1b (“POSIX.1b”)",
        "-p1003.1b-93" => "IEEE Std 1003.1b-1993 (“POSIX.1b”)",
        "-p1003.1c-95" => "IEEE Std 1003.1c-1995 (“POSIX.1c”)",
        "-p1003.1g-2000" => "IEEE Std 1003.1g-2000 (“POSIX.1g”)",
        "-p1003.1i-95" => "IEEE Std 1003.1i-1995 (“POSIX.1i”)",
        "-p1003.2" => "IEEE Std 1003.2 (“POSIX.2”)",
        "-p1003.2-92" => "IEEE Std 1003.2-1992 (“POSIX.2”)",
        "-p1003.2a-92" => "IEEE Std 1003.2a-1992 (“POSIX.2”)",
        "-susv1" => "Version 1 of the Single UNIX Specification (“SUSv1”)",
        "-susv2" => "Version 2 of the Single UNIX Specification (“SUSv2”)",
        "-susv3" => "Version 3 of the Single UNIX Specification (“SUSv3”)",
        "-susv4" => "Version 4 of the Single UNIX Specification (“SUSv4”)",
        _ => "",
    }
}

fn diagnostic(line: &Line, byte: usize, description: &str, expected: &str) -> Error {
    let column = line.text[..byte].chars().count() + 1;
    let location = format!("(line {}, column {column})", line.number);
    let source = if line.source.is_empty() {
        String::new()
    } else {
        format!("{:?} ", line.source)
    };
    Error::Parse(format!(
        "Error at {source}{location}:\nunexpected {description} {source}{location}\n{expected}{}\n{}^",
        line.text,
        " ".repeat(column - 1)
    ))
}

fn macro_error(line: &Line, byte: usize, name: &str) -> Error {
    diagnostic(line, byte, &format!("Macro {name:?}"), "")
}

fn param_offset(line: &Line, text: &str) -> usize {
    line.text.len().saturating_sub(text.len())
}

fn attach(out: &mut Vec<Inline>, incoming: Vec<Inline>, space: bool) {
    if incoming.is_empty() {
        return;
    }
    if matches!(out.as_slice(), [Inline::Str(text)] if text.is_empty()) {
        out.clear();
    }
    if space && !out.is_empty() && !matches!(out.last(), Some(Inline::LineBreak | Inline::Space)) {
        out.push(Inline::Space);
    }
    for inline in incoming {
        match inline {
            Inline::Str(ref s) if !s.is_empty() => push_str(out, s),
            other => out.push(other),
        }
    }
}

fn request(line: &Line) -> (&str, &str) {
    line.request(false)
}

fn printed(text: &str) -> Vec<Inline> {
    words(&decode(text, false))
}

fn code(class: &str, text: String) -> Inline {
    Inline::Code(
        if class.is_empty() {
            Attr::default()
        } else {
            attr(class)
        },
        text,
    )
}

fn opening(s: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match s {
        "Ao" => ("Ac", "⟨", "⟩"),
        "Bo" => ("Bc", "[", "]"),
        "Bro" => ("Brc", "{", "}"),
        "Oo" => ("Oc", "[", "]"),
        "Po" => ("Pc", "(", ")"),
        "Qo" => ("Qc", "\"", "\""),
        "So" => ("Sc", "‘", "’"),
        "Do" => ("Dc", "“", "”"),
        "Eo" => ("Ec", "", ""),
        "Xo" => ("Xc", "", ""),
        _ => return None,
    })
}

fn callable(s: &str) -> bool {
    opening(s).is_some()
        || matches!(
            s,
            "Ad" | "An"
                | "Ap"
                | "Aq"
                | "Ar"
                | "At"
                | "Bq"
                | "Brq"
                | "Bx"
                | "Cd"
                | "Cm"
                | "Dx"
                | "Dv"
                | "Dq"
                | "Em"
                | "Er"
                | "Ev"
                | "Fa"
                | "Fl"
                | "Fn"
                | "Fr"
                | "Ft"
                | "Fx"
                | "Ic"
                | "In"
                | "Lb"
                | "Li"
                | "Lk"
                | "Ms"
                | "Mt"
                | "Nd"
                | "Nm"
                | "No"
                | "Ns"
                | "Nx"
                | "Op"
                | "Ox"
                | "Pa"
                | "Pf"
                | "Pq"
                | "Ql"
                | "Qq"
                | "Rv"
                | "Sm"
                | "Sq"
                | "St"
                | "Sx"
                | "Sy"
                | "Tn"
                | "Ud"
                | "Ux"
                | "Va"
                | "Vt"
                | "Xr"
                | "Ex"
        )
}

fn delimiter(s: &str) -> bool {
    matches!(s, "." | "," | ":" | ";" | "!" | "?" | ")" | "]")
}

fn is_macro(arg: &Arg<'_>) -> bool {
    !arg.quoted && callable(arg.text)
}

// Index closing macros and trailing punctuation in one pass.
struct InlineTokens<'a, 'text> {
    tokens: &'a [Arg<'text>],
    closes: Vec<usize>,
    content_ends: Vec<usize>,
}

impl<'a, 'text> InlineTokens<'a, 'text> {
    fn new(tokens: &'a [Arg<'text>]) -> Self {
        let mut opens = Vec::new();
        let mut closes = vec![tokens.len(); tokens.len()];
        let mut content_ends = Vec::with_capacity(tokens.len().saturating_add(1));
        content_ends.push(0);
        let mut content_end = 0;
        for (index, token) in tokens.iter().enumerate() {
            if token.quoted || !delimiter(token.text) {
                content_end = index + 1;
            }
            content_ends.push(content_end);
            if !token.quoted {
                if let Some((close, _, _)) = opening(token.text) {
                    opens.push((index, close));
                } else if let Some(&(start, close)) = opens.last()
                    && token.text == close
                {
                    closes[start] = index;
                    opens.pop();
                }
            }
        }
        Self {
            tokens,
            closes,
            content_ends,
        }
    }
}

impl<'a> Parser<'a> {
    fn prologue(&mut self) -> Result<Meta> {
        let mut meta = Meta::default();
        while self
            .lines
            .get(self.i)
            .is_some_and(|line| line.text.is_empty())
        {
            self.i += 1;
        }
        if let Some([dd, dt, os]) = self.lines.get(self.i..self.i + 3)
            && let ("Dd", date) = request(dd)
            && let ("Dt", title) = request(dt)
            && let ("Os", system) = request(os)
        {
            if let Some(arg) = args(system).first() {
                return Err(diagnostic(
                    os,
                    param_offset(os, system),
                    &format!("Text {:?}", arg.text),
                    "",
                ));
            }
            meta.insert("date", MetaValue::MetaInlines(printed(date)));
            for (key, arg) in ["title", "section", "architecture"]
                .into_iter()
                .zip(args(title))
            {
                meta.insert(key, MetaValue::MetaInlines(printed(arg.text)));
            }
            self.i += 3;
        }
        Ok(meta)
    }

    fn blocks(&mut self, stops: &[&str], level: usize) -> Result<Vec<Block>> {
        depth(level, self.limit)?;
        let mut out = Vec::new();
        let mut para = Vec::new();
        let mut glue = false;
        while let Some(line) = self.lines.get(self.i) {
            let (mac, text) = request(line);
            if stops.contains(&mac) {
                break;
            }
            self.i += 1;
            match mac {
                "Ss" if self.section == "SYNOPSIS" => {
                    return Err(diagnostic(
                        line,
                        1,
                        "Macro \"Ss\"",
                        "expecting end of input or text lines or inline macros\n",
                    ));
                }
                "Sh" | "Ss" => {
                    flush(&mut out, &mut para);
                    if mac == "Sh" {
                        self.section = args(text)
                            .iter()
                            .map(|arg| arg.text)
                            .collect::<Vec<_>>()
                            .join(" ");
                    }
                    out.push(Block::Header(
                        if mac == "Sh" { 1 } else { 2 },
                        Attr::default(),
                        printed(
                            &args(text)
                                .iter()
                                .map(|arg| arg.text)
                                .collect::<Vec<_>>()
                                .join(" "),
                        ),
                    ));
                    if mac == "Sh" && self.section == "SYNOPSIS" {
                        self.synopsis(&mut out, level)?;
                    }
                }
                "Pp" | "sp" => flush(&mut out, &mut para),
                "br" => para.push(Inline::LineBreak),
                "Bl" => {
                    flush(&mut out, &mut para);
                    out.push(self.list(text, level + 1)?);
                }
                "Bd" => {
                    flush(&mut out, &mut para);
                    out.push(self.display(text, level + 1)?);
                }
                "Dl" | "D1" => {
                    flush(&mut out, &mut para);
                    let tokens = self.enclosure(&args(text), level)?;
                    let inlines = self.inlines(&tokens, level)?;
                    out.push(if mac == "Dl" {
                        Block::CodeBlock(Attr::default(), randoc_core::shared::stringify(&inlines))
                    } else {
                        Block::Div(attr("display"), vec![Block::Plain(inlines)])
                    });
                }
                "Rs" => {
                    flush(&mut out, &mut para);
                    out.push(self.reference());
                }
                "Bk" | "Ek" => {}
                "Xo" | "Xc" | "." | "It" | "El" | "Ed" => {
                    return Err(macro_error(line, 1, mac));
                }
                _ if mac.starts_with('%') => return Err(macro_error(line, 1, mac)),
                "" if line.text == "." => {}
                "" if line.text.is_empty() => flush(&mut out, &mut para),
                "" if !line.text.starts_with('.') => {
                    attach(&mut para, printed(text), self.spacing && !glue);
                    glue = false;
                }
                _ if callable(mac) => {
                    let tokens = self.macro_tokens(line, mac, text, level)?;
                    if mac == "Sm"
                        && self.spacing
                        && text == "off"
                        && !para.is_empty()
                        && !matches!(para.last(), Some(Inline::Space | Inline::LineBreak))
                    {
                        para.push(Inline::Space);
                    }
                    let joint = matches!(mac, "Ns" | "Ap")
                        || tokens
                            .first()
                            .is_some_and(|t| !t.quoted && delimiter(t.text));
                    let mut inline = self.inlines(&tokens, level)?;
                    if mac == "Nm"
                        && self.section == "NAME"
                        && matches!(inline.last(), Some(Inline::Code(..)))
                    {
                        inline.push(Inline::Str(String::new()));
                    }
                    let split = matches!(inline.first(), Some(Inline::LineBreak));
                    if mac == "St" && inline.is_empty() && para.is_empty() {
                        para.push(Inline::Str(String::new()));
                    } else {
                        attach(&mut para, inline, self.spacing && !joint && !glue && !split);
                    }
                    glue = mac == "Ns"
                        || mac == "Ap"
                        || tokens.last().is_some_and(|t| !t.quoted && t.text == "Ns");
                }
                _ => flush(&mut out, &mut para),
            }
        }
        flush(&mut out, &mut para);
        Ok(out)
    }

    fn macro_tokens(
        &mut self,
        line: &'a Line,
        mac: &'a str,
        text: &'a str,
        level: usize,
    ) -> Result<Vec<Arg<'a>>> {
        let mut tokens = vec![Arg {
            text: mac,
            quoted: false,
            offset: 1,
        }];
        let offset = param_offset(line, text);
        for mut token in args(text) {
            token.offset += offset;
            if !token.quoted && token.text == "Xo" {
                return Err(macro_error(line, token.offset, token.text));
            }
            tokens.push(token);
        }
        self.enclosure(&tokens, level)
    }

    fn enclosure(&mut self, initial: &[Arg<'a>], level: usize) -> Result<Vec<Arg<'a>>> {
        let mut result = Vec::new();
        let mut pending = initial.to_vec();
        let mut ends = Vec::new();
        loop {
            for token in pending.drain(..) {
                if !token.quoted {
                    if let Some((end, _, _)) = opening(token.text) {
                        ends.push(end);
                        depth(level + ends.len(), self.limit)?;
                    } else if ends.last() == Some(&token.text) {
                        ends.pop();
                    }
                }
                result.push(token);
            }
            if ends.is_empty() {
                return Ok(result);
            }
            let Some(line) = self.lines.get(self.i) else {
                return Err(Error::Parse(
                    "The mdoc enclosure has no closing macro.".into(),
                ));
            };
            self.i += 1;
            let (mac, text) = request(line);
            if !mac.is_empty() {
                pending.push(Arg {
                    text: mac,
                    quoted: false,
                    offset: 1,
                });
            }
            let mut arguments = args(text);
            if mac.is_empty() {
                for token in &mut arguments {
                    token.quoted = true;
                }
            }
            pending.extend(arguments);
        }
    }

    fn inlines(&mut self, tokens: &[Arg<'_>], level: usize) -> Result<Vec<Inline>> {
        let mut index = 0;
        self.inline_range(&InlineTokens::new(tokens), &mut index, tokens.len(), level)
    }

    fn inline_range(
        &mut self,
        line: &InlineTokens<'_, '_>,
        index: &mut usize,
        end: usize,
        level: usize,
    ) -> Result<Vec<Inline>> {
        depth(level, self.limit)?;
        let tokens = &line.tokens[..end];
        let mut out = Vec::new();
        let mut glue = false;
        while let Some(token) = tokens.get(*index) {
            let name = token.text;
            *index += 1;
            let mut joint = !token.quoted && delimiter(name);
            let value = if !is_macro(token) {
                vec![Inline::Str(decode(name, false))]
            } else if let Some((_, left, right)) = opening(name) {
                let close = line.closes[*index - 1].min(end);
                let value = self.inline_range(line, index, close, level + 1)?;
                *index += usize::from(close < end);
                surround(left, value, right)
            } else {
                match name {
                    "Ns" => {
                        glue = true;
                        continue;
                    }
                    "Sm" => {
                        if let Some(arg) = tokens.get(*index) {
                            if self.spacing
                                && arg.text == "off"
                                && !out.is_empty()
                                && !matches!(out.last(), Some(Inline::Space | Inline::LineBreak))
                            {
                                out.push(Inline::Space);
                            }
                            self.spacing = arg.text != "off";
                            *index += 1;
                        }
                        continue;
                    }
                    "Ap" => {
                        joint = true;
                        vec![Inline::Str("’".into())]
                    }
                    "Op" | "Aq" | "Bq" | "Brq" | "Pq" | "Dq" | "Sq" | "Qq" | "Pf" => {
                        let start = *index;
                        let content = if name == "Pf" {
                            end.min(start + 1)
                        } else if tokens.get(start).is_some_and(is_macro) {
                            end
                        } else {
                            start.max(line.content_ends[end])
                        };
                        let value = self.inline_range(line, index, content, level + 1)?;
                        match name {
                            "Op" | "Bq" => surround("[", value, "]"),
                            "Pq" => surround("(", value, ")"),
                            "Aq" => surround("⟨", value, "⟩"),
                            "Brq" => surround("{", value, "}"),
                            "Sq" => vec![Inline::Quoted(QuoteType::SingleQuote, value)],
                            "Qq" => surround("\"", value, "\""),
                            "Pf" => value,
                            _ => vec![Inline::Quoted(QuoteType::DoubleQuote, value)],
                        }
                    }
                    _ => self.arguments_for(name, tokens, index),
                }
            };
            let split = matches!(value.first(), Some(Inline::LineBreak));
            attach(&mut out, value, self.spacing && !joint && !glue && !split);
            glue = matches!(name, "Ap" | "Pf" | "(" | "[");
        }
        Ok(out)
    }

    fn arguments_for(&mut self, name: &str, tokens: &[Arg<'_>], index: &mut usize) -> Vec<Inline> {
        let mut out = Vec::new();
        loop {
            let start = *index;
            while let Some(arg) = tokens.get(*index) {
                if !arg.quoted && (is_macro(arg) || name != "St" && delimiter(arg.text)) {
                    break;
                }
                *index += 1;
            }
            let parameters = tokens[start..*index]
                .iter()
                .map(|arg| decode(arg.text, false))
                .collect::<Vec<_>>();
            if parameters.is_empty()
                && tokens
                    .get(*index)
                    .is_some_and(|arg| !arg.quoted && arg.text == "Xo")
            {
                break;
            }
            let value = self.inline_macro(name, &parameters);
            attach(&mut out, value, self.spacing);
            let punctuation = *index;
            while let Some(arg) = tokens.get(*index) {
                if arg.quoted || !delimiter(arg.text) {
                    break;
                }
                *index += 1;
            }
            if !tokens
                .get(*index)
                .is_some_and(|arg| arg.quoted || !is_macro(arg))
            {
                *index = punctuation;
                break;
            }
            for arg in &tokens[punctuation..*index] {
                push_str(&mut out, arg.text);
            }
        }
        out
    }

    fn inline_macro(&mut self, name: &str, values: &[String]) -> Vec<Inline> {
        let arg = values.first().map_or("", String::as_str);
        let text = values.join(if self.spacing { " " } else { "" });
        match name {
            "Nm" => {
                if self.section == "NAME" && self.name.is_empty() {
                    self.name.clone_from(&text);
                }
                vec![code(
                    if self.section == "NAME" { "" } else { "Nm" },
                    if values.is_empty() {
                        self.name.clone()
                    } else {
                        text
                    },
                )]
            }
            "Nd" if self.section == "NAME" => words(&format!("— {text}")),
            "Nd" => Vec::new(),
            "Fl" if values.is_empty() => vec![code("Fl", "-".into())],
            "Fl" => spaced(values.iter().map(|v| code("Fl", format!("-{v}")))),
            "Ar" | "Fa" | "Ft" | "Va" if !values.is_empty() => vec![code("variable", text)],
            "Ar" | "Fa" | "Ft" | "Va" => Vec::new(),
            "Cm" | "Ic" | "Ev" | "Li" | "Ql" | "Dv" | "Er" | "Ad" | "Cd" | "Vt" | "Ms" => {
                vec![code(name, text)]
            }
            "In" => vec![code(name, format!("<{text}>"))],
            "Fn" => {
                let mut out = vec![code("Fn", arg.into()), Inline::Str("(".into())];
                for (i, value) in values.iter().skip(1).enumerate() {
                    if i > 0 {
                        out.extend([Inline::Str(",".into()), Inline::Space]);
                    }
                    out.push(code("variable", value.clone()));
                }
                push_str(&mut out, ")");
                out
            }
            "St" => {
                let mut standard = words(standard_name(arg));
                for value in values.iter().skip(1) {
                    attach(&mut standard, words(value), self.spacing);
                }
                standard
            }
            "Em" => vec![Inline::Emph(words(&text))],
            "Sy" => vec![Inline::Strong(words(&text))],
            "An" if arg.starts_with('-') => {
                self.author_split = arg == "-split";
                Vec::new()
            }
            "An" => {
                let mut out = Vec::new();
                if self.author_split {
                    out.push(Inline::LineBreak);
                }
                out.push(Inline::Span(attr(name), words(&text)));
                out
            }
            "Pa" if values.is_empty() => Vec::new(),
            "Pa" | "Sx" | "Tn" => vec![Inline::Span(attr(name), words(&text))],
            "Xr" => {
                let label = values
                    .get(1)
                    .map_or_else(|| arg.into(), |s| format!("{arg}({s})"));
                vec![Inline::Span(attr(name), vec![Inline::Str(label)])]
            }
            "Lk" | "Mt" => {
                let label = if name == "Lk" && values.len() > 1 {
                    values[1..].join(" ")
                } else {
                    arg.into()
                };
                let url = if name == "Mt" {
                    format!("mailto:{arg}")
                } else {
                    arg.into()
                };
                vec![Inline::Link(
                    Attr::default(),
                    words(&label),
                    Target::new(url, ""),
                )]
            }
            "Ux" | "Ox" | "Fx" | "Nx" | "Dx" | "Bx" | "At" => {
                let system = match name {
                    "Ux" => "UNIX",
                    "Ox" => "OpenBSD",
                    "Fx" => "FreeBSD",
                    "Nx" => "NetBSD",
                    "Dx" => "DragonFly BSD",
                    "At" => "AT&T UNIX",
                    _ => "BSD",
                };
                if name == "Bx" && !text.is_empty() {
                    words(&format!("{text}BSD"))
                } else {
                    words(&format!("{system} {text}"))
                }
            }
            "Ex" => {
                let mut out = if let Some(utility) = values.get(1) {
                    vec![
                        Inline::Str("The".into()),
                        Inline::Space,
                        code("Nm", utility.clone()),
                        Inline::Space,
                        Inline::Str("utility".into()),
                    ]
                } else {
                    words("The utility")
                };
                attach(
                    &mut out,
                    words("exits 0 on success, and >0 if an error occurs."),
                    true,
                );
                out
            }
            "Lb" => vec![
                Inline::Str("library".into()),
                Inline::Space,
                Inline::Quoted(QuoteType::DoubleQuote, words(&text)),
            ],
            "No" if text.is_empty() && !values.is_empty() => vec![Inline::Str(String::new())],
            _ => words(&text),
        }
    }

    fn list(&mut self, text: &str, level: usize) -> Result<Block> {
        depth(level, self.limit)?;
        let kind = args(text).first().map_or("-item", |arg| arg.text);
        if kind == "-column" {
            return self.columns(level);
        }
        let definitions = matches!(kind, "-tag" | "-hang" | "-ohang" | "-inset" | "-diag");
        let mut terms = Vec::new();
        let mut items = Vec::new();
        while let Some(line) = self.lines.get(self.i) {
            let (mac, text) = request(line);
            match mac {
                "El" => {
                    self.i += 1;
                    break;
                }
                "It" => {
                    self.i += 1;
                    if definitions {
                        let tokens = self.enclosure(&args(text), level)?;
                        let term = self.inlines(&tokens, level)?;
                        let body = self.blocks(&["It", "El"], level)?;
                        terms.push((term, vec![body]));
                    } else {
                        if let Some(arg) = args(text).first() {
                            return Err(diagnostic(
                                line,
                                param_offset(line, text) + arg.offset,
                                &format!("Text {:?}", arg.text),
                                "",
                            ));
                        }
                        items.push(self.blocks(&["It", "El"], level)?);
                    }
                }
                _ => self.i += 1,
            }
        }
        Ok(if definitions {
            Block::DefinitionList(terms)
        } else if kind == "-enum" {
            Block::OrderedList(ListAttributes::default(), items)
        } else {
            Block::BulletList(items)
        })
    }

    fn columns(&mut self, level: usize) -> Result<Block> {
        let mut rows = Vec::<Row>::new();
        let mut cells = Vec::new();
        let mut content = Vec::new();
        let mut has_row = false;
        let finish_cell = |cells: &mut Vec<Cell>, content: &mut Vec<Inline>| {
            cells.push(mdoc_cell(std::mem::take(content)));
        };
        while let Some(line) = self.lines.get(self.i) {
            let (mac, text) = request(line);
            self.i += 1;
            if matches!(mac, "It" | "El") && (!cells.is_empty() || !content.is_empty() || has_row) {
                finish_cell(&mut cells, &mut content);
                rows.push(Row {
                    cells: std::mem::take(&mut cells),
                    ..Row::default()
                });
                has_row = false;
            }
            if mac == "El" {
                break;
            }
            if matches!(mac, "It" | "Ta") {
                if mac == "Ta" {
                    finish_cell(&mut cells, &mut content);
                }
                has_row = true;
                let tokens = args(text);
                for (i, cell) in tokens.split(|t| !t.quoted && t.text == "Ta").enumerate() {
                    if i > 0 {
                        finish_cell(&mut cells, &mut content);
                    }
                    let value = self.inlines(cell, level)?;
                    attach(&mut content, value, true);
                }
            } else if mac == "Pp" {
                continue;
            } else if mac.is_empty() {
                attach(&mut content, printed(text), true);
            } else if callable(mac) {
                let tokens = self.macro_tokens(line, mac, text, level)?;
                let value = self.inlines(&tokens, level)?;
                attach(&mut content, value, true);
            }
        }
        if !cells.is_empty() || !content.is_empty() || has_row {
            finish_cell(&mut cells, &mut content);
            rows.push(Row {
                cells,
                ..Row::default()
            });
        }
        let cols = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
        for row in &mut rows {
            charge_cells(cols - row.cells.len(), &mut self.cells)?;
            row.cells.resize_with(cols, Cell::default);
        }
        Ok(Block::Table(Box::new(Table {
            col_specs: vec![ColSpec::default(); cols],
            bodies: vec![TableBody {
                body: rows,
                ..TableBody::default()
            }],
            ..Table::default()
        })))
    }

    fn display(&mut self, text: &str, level: usize) -> Result<Block> {
        depth(level, self.limit)?;
        let kind = args(text).first().map_or("", |arg| arg.text);
        if !matches!(kind, "-literal" | "-unfilled") {
            let out = self.blocks(&["Ed"], level)?;
            if self.i < self.lines.len() {
                self.i += 1;
            }
            return Ok(Block::Div(attr("display"), out));
        }
        let mut text = String::new();
        let mut lines = Vec::new();
        while let Some(line) = self.lines.get(self.i) {
            self.i += 1;
            let (mac, args) = request(line);
            if mac == "Ed" {
                break;
            }
            let value = if mac.is_empty() {
                let line = decode(args, false);
                if line.is_empty() {
                    Vec::new()
                } else {
                    vec![Inline::Str(line)]
                }
            } else if callable(mac) {
                let tokens = self.macro_tokens(line, mac, args, level)?;
                self.inlines(&tokens, level)?
            } else {
                continue;
            };
            if kind == "-literal" {
                text.push_str(&randoc_core::shared::stringify(&value));
                text.push('\n');
            } else {
                lines.push(value);
            }
        }
        Ok(if kind == "-literal" {
            Block::CodeBlock(Attr::default(), text)
        } else {
            Block::LineBlock(lines)
        })
    }

    fn synopsis(&mut self, out: &mut Vec<Block>, level: usize) -> Result<()> {
        let mut lines = Vec::new();
        let mut line = Vec::new();
        let mut function = false;
        while let Some(source) = self.lines.get(self.i) {
            let (mac, text) = request(source);
            if mac == "Ss" {
                return Err(diagnostic(
                    source,
                    1,
                    "Macro \"Ss\"",
                    "expecting synopsis inlines, end of input or text lines or inline macros\n",
                ));
            }
            if mac == "Sh" {
                break;
            }
            if !mac.is_empty() && !callable(mac) && !matches!(mac, "Pp" | "Bk" | "Ek" | "br") {
                break;
            }
            self.i += 1;
            if matches!(mac, "Nm" | "Ft" | "Fn" | "In" | "br") && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            if matches!(mac, "In" | "Pp") && !lines.is_empty() {
                out.push(Block::LineBlock(std::mem::take(&mut lines)));
            }
            match mac {
                "Pp" | "Bk" | "Ek" | "br" => {}
                "" => attach(&mut line, printed(text), true),
                _ => {
                    let tokens = self.macro_tokens(source, mac, text, level)?;
                    let mut value = self.inlines(&tokens, level)?;
                    if mac == "Fn" && function {
                        push_str(&mut value, ";");
                    }
                    attach(&mut line, value, self.spacing);
                }
            }
            function = mac == "Ft";
        }
        if !line.is_empty() {
            lines.push(line);
        }
        if !lines.is_empty() {
            out.push(Block::LineBlock(lines));
        }
        Ok(())
    }

    fn reference(&mut self) -> Block {
        const FIELDS: &[&str] = &[
            "%A", "%T", "%B", "%J", "%R", "%N", "%V", "%P", "%I", "%C", "%D", "%O", "%U", "%Q",
        ];
        let mut fields = Vec::<(&str, &str)>::new();
        while let Some(line) = self.lines.get(self.i) {
            let (mac, text) = request(line);
            if mac == "Re" {
                self.i += 1;
                break;
            }
            if !mac.starts_with('%') {
                break;
            }
            fields.push((mac, text));
            self.i += 1;
        }
        let mut text = Vec::new();
        for key in FIELDS {
            for (_, value) in fields.iter().rev().filter(|(mac, _)| mac == key) {
                text.push(*value);
            }
        }
        Block::Para(printed(&format!("{}.", text.join(", "))))
    }
}

fn mdoc_cell(content: Vec<Inline>) -> Cell {
    Cell {
        content: if content.is_empty() {
            Vec::new()
        } else {
            vec![Block::Plain(content)]
        },
        ..Cell::default()
    }
}

fn surround(left: &str, value: Vec<Inline>, right: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    if !left.is_empty() {
        push_str(&mut out, left);
    }
    attach(&mut out, value, false);
    if !right.is_empty() {
        push_str(&mut out, right);
    }
    out
}

fn spaced(iter: impl IntoIterator<Item = Inline>) -> Vec<Inline> {
    let mut out = Vec::new();
    for inline in iter {
        if !out.is_empty() {
            out.push(Inline::Space);
        }
        out.push(inline);
    }
    out
}

fn flush(out: &mut Vec<Block>, para: &mut Vec<Inline>) {
    if !para.is_empty() {
        let mut contents = std::mem::take(para);
        if matches!(contents.as_slice(), [Inline::Str(text)] if text.is_empty()) {
            contents.clear();
        }
        out.push(Block::Para(contents));
    }
}
