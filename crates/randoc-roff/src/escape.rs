use std::sync::Arc;

use randoc_ast::{Attr, Inline};
use randoc_core::{Context, Error, Result, SourcePos, Sources};

#[derive(Clone)]
pub(crate) struct Line {
    pub text: String,
    pub source: Arc<str>,
    pub number: u32,
}

impl Line {
    pub fn pos(&self, column: u32) -> SourcePos {
        SourcePos::new(&*self.source, self.number, column)
    }

    pub fn error(&self, message: &str) -> Error {
        Error::Parse(format!("{}: {message}", self.pos(1)))
    }

    pub fn request(&self, man: bool) -> (&str, &str) {
        if let Some(rest) = self.text.strip_prefix('.').or_else(|| {
            if man {
                self.text.strip_prefix('\'')
            } else {
                None
            }
        }) {
            let rest = rest.trim_start_matches([' ', '\t']);
            let (name, args) = rest.split_once([' ', '\t']).unwrap_or((rest, ""));
            (name, args.trim_start_matches([' ', '\t']))
        } else {
            ("", &self.text)
        }
    }
}

pub(crate) fn input_size(sources: &Sources, ctx: &Context) -> Result<usize> {
    let size = sources
        .0
        .iter()
        .fold(0usize, |size, src| size.saturating_add(src.text.len()));
    if size as u64 > ctx.limits.max_input_size {
        return Err(Error::Parse(
            "The roff input exceeds the size limit.".into(),
        ));
    }
    Ok(size)
}

pub(crate) fn source_lines(sources: &Sources, man: bool) -> Vec<Line> {
    let mut lines = Vec::new();
    for source in &sources.0 {
        if let Some(last) = lines.last().cloned() {
            lines.push(Line {
                text: String::new(),
                ..last
            });
        }
        lines.extend(scan(&source.name, &source.text, man));
    }
    lines
}

fn line_number(index: usize) -> u32 {
    u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX)
}

pub(crate) fn scan(source: &str, text: &str, man: bool) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut logical = String::new();
    let mut start = 1;
    let mut pending = false;
    let source: Arc<str> = Arc::from(source);
    for (index, physical) in text.lines().enumerate() {
        if !pending {
            start = line_number(index);
        }
        let mut chars = physical.char_indices();
        let mut end = physical.len();
        let mut continues = false;
        while let Some((pos, ch)) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some((_, '"')) => {
                        end = pos;
                        break;
                    }
                    Some((_, '#')) if man => {
                        end = pos;
                        continues = true;
                        break;
                    }
                    None if man => {
                        end = pos;
                        continues = true;
                    }
                    _ => {}
                }
            }
        }
        // Keep comment requests out of the parser.
        if logical.is_empty() && physical.starts_with(".\\\"") {
            continue;
        }
        logical.push_str(&physical[..end]);
        pending = continues;
        if !continues {
            lines.push(Line {
                text: std::mem::take(&mut logical),
                source: source.clone(),
                number: start,
            });
        }
    }
    if !logical.is_empty() {
        lines.push(Line {
            text: logical,
            source,
            number: start,
        });
    }
    lines
}

#[derive(Clone, Copy)]
pub(crate) struct Arg<'a> {
    pub text: &'a str,
    pub quoted: bool,
    pub offset: usize,
}

pub(crate) fn args(mut input: &str) -> Vec<Arg<'_>> {
    let length = input.len();
    let mut out = Vec::new();
    while !input.is_empty() {
        input = input.trim_start_matches([' ', '\t']);
        if input.is_empty() {
            break;
        }
        let offset = length - input.len();
        let quoted = input.starts_with('"');
        if quoted {
            input = &input[1..];
        }
        let mut chars = input.char_indices();
        let mut end = input.len();
        let mut tail = end;
        while let Some((i, ch)) = chars.next() {
            if ch == '\\' {
                chars.next();
                continue;
            }
            if (quoted && ch == '"') || (!quoted && matches!(ch, ' ' | '\t')) {
                end = i;
                tail = i + 1;
                break;
            }
        }
        out.push(Arg {
            text: &input[..end],
            quoted,
            offset,
        });
        input = &input[tail..];
    }
    out
}

pub(crate) fn name(input: &str) -> (&str, &str) {
    if let Some(rest) = input.strip_prefix('[') {
        return rest.split_once(']').unwrap_or((rest, ""));
    }
    let (count, input) = input.strip_prefix('(').map_or((1, input), |rest| (2, rest));
    let end = input
        .char_indices()
        .nth(count)
        .map_or(input.len(), |(i, _)| i);
    input.split_at(end)
}

fn glyph(name: &str) -> char {
    if let Some(hex) = name.strip_prefix('u')
        && let Some(ch) = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
    {
        return ch;
    }
    match name {
        "em" => '—',
        "en" => '–',
        "hy" => '-',
        "mi" => '−',
        "bu" => '·',
        "lq" => '“',
        "rq" => '”',
        "oq" => '‘',
        "cq" => '’',
        "aq" => '\'',
        "at" => '@',
        "dq" => '"',
        "rs" => '\\',
        "ti" => '~',
        "ha" => '^',
        "co" => '©',
        "rg" => '®',
        "tm" => '™',
        "de" => '°',
        "pl" => '+',
        "mu" => '×',
        "di" => '÷',
        "lt" => '<',
        "gt" => '>',
        "le" => '≤',
        "ge" => '≥',
        "ne" => '≠',
        "la" => '⟨',
        "ra" => '⟩',
        "ul" => '_',
        "ba" => '|',
        "ua" => '↑',
        "da" => '↓',
        "->" => '→',
        "<-" => '←',
        "el" => '…',
        "Fo" => '«',
        "Fc" => '»',
        "sc" => '§',
        "sq" => '□',
        "14" => '¼',
        "12" => '½',
        "34" => '¾',
        "Eu" => '€',
        "Po" => '£',
        "ct" => '¢',
        _ => '�',
    }
}

pub(crate) fn words(text: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    for word in text
        .split(|ch: char| ch.is_ascii_whitespace())
        .filter(|word| !word.is_empty())
    {
        if !out.is_empty() {
            out.push(Inline::Space);
        }
        out.push(Inline::Str(word.into()));
    }
    out
}

pub(crate) fn push_str(out: &mut Vec<Inline>, text: &str) {
    if let Some(Inline::Str(last)) = out.last_mut() {
        last.push_str(text);
    } else {
        out.push(Inline::Str(text.into()));
    }
}

pub(crate) fn decode(mut text: &str, man: bool) -> String {
    let mut out = String::with_capacity(text.len());
    while let Some((before, rest)) = text.split_once('\\') {
        out.push_str(before);
        let (tail, action) = escape(rest, man);
        if let Action::Char(ch) = action {
            out.push(ch);
        }
        text = tail;
    }
    out.push_str(text);
    out
}

pub(crate) enum Action<'a> {
    Skip,
    Char(char),
    Font(&'a str),
}

pub(crate) fn escape(rest: &str, man: bool) -> (&str, Action<'_>) {
    use Action::{Char, Font, Skip};
    let Some(ch) = rest.chars().next() else {
        return ("", Skip);
    };
    let tail = &rest[ch.len_utf8()..];
    match ch {
        '(' | '[' => {
            let (symbol, tail) = name(rest);
            (tail, Char(glyph(symbol)))
        }
        '*' if !man => (tail, Skip),
        'f' | 'm' | 'M' | '*' | 'n' => {
            let (arg, tail) = name(tail);
            (tail, if ch == 'f' && man { Font(arg) } else { Skip })
        }
        's' => {
            let tail = tail.trim_start_matches(['+', '-']);
            let (_, tail) = name(tail);
            (tail, Skip)
        }
        'h' | 'v' | 'w' | 'X' | 'A' | 'B' | 'o' | 'z' | 'N' => {
            let mut chars = tail.chars();
            let Some(delimiter) = chars.next() else {
                return ("", Skip);
            };
            if ch == 'z' {
                return (chars.as_str(), Char(delimiter));
            }
            let (arg, tail) = chars.as_str().split_once(delimiter).unwrap_or(("", ""));
            let action = if ch == 'N' {
                arg.parse().ok().and_then(char::from_u32).map_or(Skip, Char)
            } else {
                Skip
            };
            (tail, action)
        }
        '-' | '.' | '!' | '/' => (tail, Char(ch)),
        ' ' | '0' | '|' | '^' => (tail, Char(' ')),
        '~' => (tail, Char('\u{a0}')),
        't' => (tail, Char('\t')),
        'e' | '\\' => (tail, Char('\\')),
        '&' | ':' | 'c' | '%' | '{' | '}' | 'u' | 'd' => (tail, Skip),
        _ => (tail, Char(ch)),
    }
}

pub(crate) fn attr(class: &str) -> Attr {
    Attr {
        classes: vec![class.into()],
        ..Attr::default()
    }
}

pub(crate) fn depth(depth: usize, limit: usize) -> Result<()> {
    if depth > limit {
        return Err(Error::Parse(
            "The roff nesting depth exceeds the limit.".into(),
        ));
    }
    Ok(())
}

pub(crate) fn charge_cells(count: usize, remaining: &mut usize) -> Result<()> {
    *remaining = remaining
        .checked_sub(count)
        .ok_or_else(|| Error::Parse("The roff table exceeds the cell limit.".into()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::line_number;

    #[test]
    fn source_line_numbers_saturate() {
        assert_eq!(line_number(0), 1);
        assert_eq!(line_number(u32::MAX as usize - 1), u32::MAX);
        assert_eq!(line_number(u32::MAX as usize), u32::MAX);
        assert_eq!(line_number(usize::MAX), u32::MAX);
    }
}
