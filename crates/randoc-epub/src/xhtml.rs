use std::borrow::Cow;
use std::ops::Range;

use randoc_core::{Error, Result};

const MATHML: &str = "http://www.w3.org/1998/Math/MathML";

struct Switch {
    edit: usize,
    branch: Option<(Branch, usize)>,
    supported: Option<Range<usize>>,
    fallback: Option<Range<usize>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Branch {
    Supported,
    Unsupported,
    Default,
}

struct Edit {
    start: usize,
    end: usize,
    keep: Option<Range<usize>>,
    paragraph: bool,
}

/// Selects the first supported case in an EPUB switch, or its default branch.
pub(crate) fn select_switches(input: &str, max_depth: usize) -> Result<Cow<'_, str>> {
    if !input.contains("<epub:switch") && !input.contains("<switch") {
        return Ok(Cow::Borrowed(input));
    }

    let mut switches = Vec::<Switch>::new();
    let mut edits = Vec::new();
    let mut position = 0;
    while let Some(open) = input[position..].find('<') {
        let start = position + open;
        let tail = &input[start + 1..];
        let special = if tail.starts_with("!--") {
            Some("-->")
        } else if tail.starts_with("![CDATA[") {
            Some("]]>")
        } else {
            None
        };
        if let Some(marker) = special {
            let Some(end) = tail.find(marker) else { break };
            position = start + 1 + end + marker.len();
            continue;
        }
        let Some(end) = end_of_tag(tail) else { break };
        position = start + end + 2;
        let tag = tail[..end].trim_end();
        let closed = tag.starts_with('/');
        let tag = tag.trim_start_matches('/');
        let name_end = tag
            .find(|c: char| c == '/' || c.is_ascii_whitespace())
            .unwrap_or(tag.len());
        let name = &tag[..name_end];
        let name = name.strip_prefix("epub:").unwrap_or(name);
        if name == "switch" && !closed && !tag.ends_with('/') {
            if switches.len() >= max_depth {
                return Err(Error::Parse("the document is nested too deeply".into()));
            }
            switches.push(Switch {
                edit: edits.len(),
                branch: None,
                supported: None,
                fallback: None,
            });
            edits.push(Edit {
                start,
                end: start,
                keep: None,
                paragraph: false,
            });
        } else if let Some(switch) = switches.last_mut() {
            match (name, closed) {
                ("switch", true) => {
                    if let Some(switch) = switches.pop() {
                        let edit = &mut edits[switch.edit];
                        edit.end = position;
                        if let Some(range) = switch.supported {
                            edit.paragraph = starts_with_math(&input[range.clone()]);
                            edit.keep = Some(range);
                        } else {
                            edit.keep = switch.fallback;
                        }
                    }
                }
                ("case" | "default", false) if switch.branch.is_none() && !tag.ends_with('/') => {
                    let kind = if name == "default" {
                        Branch::Default
                    } else if required_namespace(&tag[name_end..]) == Some(MATHML) {
                        Branch::Supported
                    } else {
                        Branch::Unsupported
                    };
                    switch.branch = Some((kind, position));
                }
                ("case" | "default", true) => {
                    if let Some((branch, begin)) = switch.branch.take() {
                        match branch {
                            Branch::Supported if name == "case" => {
                                switch.supported.get_or_insert(begin..start);
                            }
                            Branch::Default if name == "default" => {
                                switch.fallback.get_or_insert(begin..start);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    edits.retain(|edit| edit.end > edit.start);
    if edits.is_empty() {
        return Ok(Cow::Borrowed(input));
    }
    let mut output = String::with_capacity(input.len());
    apply_edits(
        input,
        0..input.len(),
        &mut edits.into_iter().peekable(),
        &mut output,
    );
    Ok(Cow::Owned(output))
}

fn apply_edits(
    input: &str,
    range: Range<usize>,
    edits: &mut std::iter::Peekable<impl Iterator<Item = Edit>>,
    output: &mut String,
) {
    let mut position = range.start;
    while edits.peek().is_some_and(|edit| edit.start < range.end) {
        let Some(edit) = edits.next() else { break };
        if edit.start < position || edit.end > range.end {
            continue;
        }
        output.push_str(&input[position..edit.start]);
        if let Some(keep) = edit.keep {
            if edit.paragraph {
                output.push_str("<p>");
            }
            apply_edits(input, keep, edits, output);
            if edit.paragraph {
                output.push_str("</p>");
            }
        }
        position = edit.end;
    }
    output.push_str(&input[position..range.end]);
}

fn starts_with_math(input: &str) -> bool {
    input.trim_start().strip_prefix('<').is_some_and(|tag| {
        let name = tag
            .split(|c: char| c == '>' || c == '/' || c.is_ascii_whitespace())
            .next()
            .unwrap_or_default();
        name.rsplit(':').next() == Some("math")
    })
}

fn required_namespace(attributes: &str) -> Option<&str> {
    let (_, value) = attributes
        .match_indices("required-namespace")
        .find_map(|(start, name)| {
            let preceding = attributes[..start].chars().next_back()?;
            let value = attributes[start + name.len()..].trim_start();
            (preceding.is_ascii_whitespace() && value.starts_with('=')).then_some((start, value))
        })?;
    let value = value.strip_prefix('=')?.trim_start();
    let quote = value.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    value[1..].split_once(quote).map(|(namespace, _)| namespace)
}

pub(crate) fn end_of_tag(input: &str) -> Option<usize> {
    let mut quote = None;
    for (index, byte) in input.bytes().enumerate() {
        match (quote, byte) {
            (Some(active), current) if active == current => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Some(index),
            _ => {}
        }
    }
    None
}
