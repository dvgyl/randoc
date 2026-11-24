use randoc_ast::{Attr, Inline, QuoteType, Target};
use randoc_core::shared::{auto_identifier, named_entity, stringify};
use randoc_core::{Error, Extensions, Result};

use crate::{class, depth_error};

#[derive(Clone, Copy)]
struct Delimiter {
    start: usize,
    padding_end: usize,
    width: usize,
}

#[derive(Clone, Copy)]
struct Span {
    body: usize,
    close: usize,
    end: usize,
}

// Scan each angle run once. A multi-angle opener can fall back to a single
// angle when the rest of the paragraph has no matching multi-angle close.
fn delimiters(text: &str) -> Vec<(usize, Option<Span>)> {
    let first = scan_delimiters(text, None);
    if first
        .iter()
        .any(|&(_, span, width)| width > 1 && span.is_none())
    {
        scan_delimiters(text, Some(&first))
    } else {
        first
    }
    .into_iter()
    .map(|(start, span, _)| (start, span))
    .collect()
}

type Matches = Vec<(usize, Option<Span>, usize)>;

fn scan_delimiters(text: &str, first: Option<&Matches>) -> Matches {
    let bytes = text.as_bytes();
    let mut stack: Vec<(usize, Delimiter)> = Vec::new();
    let mut spans = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        if bytes[pos].is_ascii_uppercase() && bytes.get(pos + 1) == Some(&b'<') {
            let start = pos;
            pos += 2;
            let mut end = pos;
            while bytes.get(end) == Some(&b'<') {
                end += 1;
            }
            let width = if let Some(first) = first {
                let (_, span, width) = first[spans.len()];
                if span.is_some() { width } else { 1 }
            } else if bytes
                .get(end)
                .is_some_and(|&b| horizontal(b) || (b == b'\n' && bytes[start] != b'L'))
            {
                end - start - 1
            } else {
                1
            };
            let mut padding_end = end;
            if width > 1 {
                while bytes.get(padding_end).is_some_and(|&byte| horizontal(byte)) {
                    padding_end += 1;
                }
                if bytes.get(padding_end) == Some(&b'\n') && bytes[start] != b'L' {
                    padding_end += 1;
                }
            }
            stack.push((
                spans.len(),
                Delimiter {
                    start,
                    padding_end,
                    width,
                },
            ));
            spans.push((start, None, width));
            pos = end;
        } else if bytes[pos] == b'>' {
            let mut end = pos + 1;
            while bytes.get(end) == Some(&b'>') {
                end += 1;
            }
            while let Some((index, open)) = stack.last() {
                if open.width > end - pos
                    || (open.width > 1 && (pos <= open.padding_end || !horizontal(bytes[pos - 1])))
                {
                    break;
                }
                spans[*index].1 = Some(Span {
                    body: open.start + 1 + open.width,
                    close: pos,
                    end: pos + open.width,
                });
                pos += open.width;
                stack.pop();
                if pos == end {
                    break;
                }
            }
            pos = end;
        } else {
            pos += 1;
        }
    }
    spans
}

fn horizontal(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t')
}

#[derive(Default)]
struct Parts(Vec<Inline>);

impl Parts {
    fn push(&mut self, inline: Inline) {
        match (self.0.last_mut(), inline) {
            (Some(Inline::Str(previous)), Inline::Str(text)) => previous.push_str(&text),
            (Some(Inline::Space), Inline::Space) => {}
            (_, inline) => self.0.push(inline),
        }
    }

    fn text(&mut self, text: &str) {
        if let Some(Inline::Str(previous)) = self.0.last_mut() {
            previous.push_str(text);
        } else {
            self.0.push(Inline::Str(text.into()));
        }
    }

    fn append(&mut self, parts: Self) {
        for inline in parts.0 {
            self.push(inline);
        }
    }
}

struct Frame {
    code: u8,
    span: Span,
    parts: Parts,
    label: Option<Parts>,
    section: Option<Parts>,
    uri: Option<bool>,
    target_start: Option<usize>,
    target_space: Option<usize>,
}

impl Frame {
    fn new(code: u8, span: Span) -> Self {
        Self {
            code,
            span,
            parts: Parts::default(),
            label: None,
            section: None,
            uri: None,
            target_start: None,
            target_space: None,
        }
    }

    fn output(&mut self) -> &mut Parts {
        self.section.as_mut().unwrap_or(&mut self.parts)
    }

    fn link_separator(&mut self, byte: u8) -> bool {
        if self.code != b'L' {
            return false;
        }
        if byte == b'|' && self.label.is_none() {
            if let Some(section) = self.section.take() {
                self.parts.text("/");
                self.parts.append(section);
            }
            self.label = Some(std::mem::take(&mut self.parts));
            self.uri = None;
            return true;
        }
        if byte == b'/' && self.section.is_none() {
            let uri = *self.uri.get_or_insert_with(|| uri_scheme(&self.parts.0));
            if !uri {
                self.section = Some(Parts::default());
                return true;
            }
        }
        false
    }
}

pub(super) enum InlineError {
    Unclosed,
    UrlSpace(usize),
    Parse(Error),
}

fn check_code(code: u8) -> std::result::Result<(), InlineError> {
    if matches!(
        code,
        b'B' | b'I' | b'C' | b'F' | b'S' | b'X' | b'Z' | b'E' | b'L'
    ) {
        Ok(())
    } else {
        Err(InlineError::Parse(unknown_code(code)))
    }
}

fn unknown_code(code: u8) -> Error {
    Error::Parse(format!("unknown Pod formatting code {}", char::from(code)))
}

pub(super) fn parse(
    text: &str,
    limit: usize,
    extensions: Extensions,
) -> std::result::Result<Vec<Inline>, InlineError> {
    let spans = delimiters(text);
    let mut spans = spans.into_iter().peekable();
    let mut frames = vec![Frame::new(
        0,
        Span {
            body: 0,
            close: text.len(),
            end: text.len(),
        },
    )];
    let mut pos = 0;
    while let Some(frame) = frames.last_mut() {
        if pos == frame.span.close {
            let Some(frame) = frames.pop() else {
                break;
            };
            let Some(parent) = frames.last_mut() else {
                return Ok(frame.parts.0);
            };
            if let (Some(start), Some(space)) = (frame.target_start, frame.target_space)
                && has_url_prefix(&text[start..space])
            {
                return Err(InlineError::UrlSpace(space));
            }
            pos = frame.span.end;
            let output = build(frame, extensions).map_err(InlineError::Parse)?;
            parent.output().append(output);
            continue;
        }
        if let Some(&(start, span)) = spans.peek()
            && pos == start
        {
            spans.next();
            let code = text.as_bytes()[pos];
            check_code(code)?;
            let Some(mut span) = span.filter(|span| span.end <= frame.span.close) else {
                // An unclosed code can still contain an unknown code.
                for (start, _) in spans {
                    check_code(text.as_bytes()[start])?;
                }
                return Err(InlineError::Unclosed);
            };
            if frames.len() > limit {
                return Err(InlineError::Parse(depth_error()));
            }
            let content = &text[span.body..span.close];
            if content.ends_with([' ', '\t']) {
                let start = content.trim_start_matches([' ', '\t']);
                let start = if code == b'L' {
                    start
                } else {
                    start.strip_prefix('\n').unwrap_or(start)
                };
                if !start.is_empty() && start.len() != content.len() {
                    span.body += content.len() - start.len();
                    span.close = span.body + start.trim_end_matches([' ', '\t']).len();
                }
            }
            let mut child = Frame::new(code, span);
            if code == b'L' && span.body == pos + 2 {
                child.target_start = Some(span.body);
            }
            frames.push(child);
            pos = span.body;
            continue;
        }
        let Some(ch) = text[pos..].chars().next() else {
            break;
        };
        let byte = text.as_bytes()[pos];
        if matches!(byte, b'|' | b'/') && frame.link_separator(byte) {
            if byte == b'|' && frame.target_start.is_some() {
                frame.target_start = Some(pos + 1);
                frame.target_space = None;
            }
            pos += 1;
            continue;
        }
        if matches!(ch, ' ' | '\t' | '\n') {
            if frame.target_start.is_some() {
                frame.target_space.get_or_insert(pos);
            }
            frame.output().push(Inline::Space);
        } else {
            frame.output().text(&text[pos..pos + ch.len_utf8()]);
        }
        pos += ch.len_utf8();
    }
    Err(InlineError::Unclosed)
}

fn build(frame: Frame, extensions: Extensions) -> Result<Parts> {
    let parts = frame.parts;
    let node = match frame.code {
        b'B' => Inline::Strong(parts.0),
        b'I' => Inline::Emph(parts.0),
        b'F' => Inline::Span(class("filename"), parts.0),
        b'C' => Inline::Code(Attr::default(), stringify(&parts.0)),
        b'S' => return Ok(parts),
        b'X' | b'Z' => return Ok(Parts::default()),
        b'E' => {
            let mut output = Parts::default();
            let name = stringify(&parts.0);
            let value = entity(&name).unwrap_or_else(|| format!("E<{name}>"));
            output.text(&value);
            return Ok(output);
        }
        b'L' => link(parts, frame.label, frame.section, extensions),
        code => return Err(unknown_code(code)),
    };
    Ok(Parts(vec![node]))
}

fn entity(text: &str) -> Option<String> {
    match text {
        "sol" => return Some("/".into()),
        "verbar" => return Some("|".into()),
        "lchevron" => return Some("«".into()),
        "rchevron" => return Some("»".into()),
        _ => {}
    }
    let (digits, radix) = if let Some(hex) = text.strip_prefix("0x") {
        (hex, 16)
    } else if text.starts_with('0') && text.bytes().all(|b| matches!(b, b'0'..=b'7')) {
        (text, 8)
    } else {
        (text, 10)
    };
    if !digits.is_empty() && digits.chars().all(|ch| ch.is_digit(radix)) {
        let value = u32::from_str_radix(digits, radix)
            .ok()
            .filter(|&value| value != 0)
            .and_then(char::from_u32);
        return Some(value.unwrap_or(char::REPLACEMENT_CHARACTER).to_string());
    }
    if text.len() <= 31 {
        return named_entity(&format!("{text};")).map(str::to_owned);
    }
    None
}

fn uri_scheme(parts: &[Inline]) -> bool {
    let [Inline::Str(text)] = parts else {
        return false;
    };
    text.split_once(':').is_some_and(|(scheme, rest)| {
        !rest.starts_with(':')
            && scheme.starts_with(|ch: char| ch.is_ascii_alphabetic())
            && scheme
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'+' | b'.' | b'-'))
    })
}

fn has_url_prefix(text: &str) -> bool {
    text.split_once(':').is_some_and(|(scheme, rest)| {
        !scheme.is_empty() && scheme.chars().all(char::is_alphanumeric) && !rest.starts_with(':')
    })
}

fn link(
    manual: Parts,
    label: Option<Parts>,
    section: Option<Parts>,
    extensions: Extensions,
) -> Inline {
    let mut attr = Attr::default();
    let target;
    if let Some(mut section) = section {
        strip_double_quotes(&mut section.0);
        let manual_name = stringify(&manual.0);
        if manual_name.is_empty() {
            target = format!("#{}", auto_identifier(&section.0, extensions));
        } else {
            target = String::new();
            attr.attributes.push(("manual".into(), manual_name));
            attr.attributes
                .push(("section".into(), stringify(&section.0)));
        }
        let label = label.map_or_else(
            || {
                let mut out = vec![Inline::Quoted(QuoteType::DoubleQuote, section.0)];
                if !manual.0.is_empty() {
                    out.extend([Inline::Space, Inline::Str("in".into()), Inline::Space]);
                    out.extend(manual.0);
                }
                out
            },
            |parts| parts.0,
        );
        return Inline::Link(attr, label, Target::new(target, ""));
    }
    if uri_scheme(&manual.0) {
        target = stringify(&manual.0);
    } else {
        target = String::new();
        attr.attributes
            .push(("manual".into(), stringify(&manual.0)));
    }
    Inline::Link(attr, label.unwrap_or(manual).0, Target::new(target, ""))
}

fn strip_double_quotes(parts: &mut Vec<Inline>) {
    if matches!(parts.first(), Some(Inline::Str(text)) if text.starts_with('"'))
        && matches!(parts.last(), Some(Inline::Str(text)) if text.ends_with('"'))
    {
        if let Some(Inline::Str(first)) = parts.first_mut() {
            first.remove(0);
        }
        if let Some(Inline::Str(last)) = parts.last_mut() {
            last.pop();
        }
        parts.retain(|part| !matches!(part, Inline::Str(text) if text.is_empty()));
    }
}
