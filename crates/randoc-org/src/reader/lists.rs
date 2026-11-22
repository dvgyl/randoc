use randoc_ast::{Block, Inline, ListAttributes, ListNumberDelim, ListNumberStyle};
use randoc_core::{Extension, Result};

use super::Parser;

#[derive(Clone, Copy)]
pub(super) struct Marker<'a> {
    indent: usize,
    ordering: Option<(ListNumberStyle, ListNumberDelim)>,
    text: &'a str,
}

pub(super) fn marker(line: &str, fancy: bool) -> Option<Marker<'_>> {
    let text = line.trim_start_matches([' ', '\t']);
    let indent = line.len() - text.len();
    let bytes = text.as_bytes();
    let bullet = *bytes.first()?;
    let (ordering, end) = if matches!(bullet, b'-' | b'+') || (bullet == b'*' && indent > 0) {
        (None, 1)
    } else {
        let (style, end) = if bullet.is_ascii_digit() {
            (
                if fancy {
                    ListNumberStyle::Decimal
                } else {
                    ListNumberStyle::DefaultStyle
                },
                bytes.iter().take_while(|b| b.is_ascii_digit()).count(),
            )
        } else if fancy && bullet.is_ascii_alphabetic() {
            (
                if bullet.is_ascii_lowercase() {
                    ListNumberStyle::LowerAlpha
                } else {
                    ListNumberStyle::UpperAlpha
                },
                1,
            )
        } else {
            return None;
        };
        let delimiter = match (bytes.get(end), fancy) {
            (Some(b'.'), true) => ListNumberDelim::Period,
            (Some(b')'), true) => ListNumberDelim::OneParen,
            (Some(b'.' | b')'), false) => ListNumberDelim::DefaultDelim,
            _ => return None,
        };
        (Some((style, delimiter)), end + 1)
    };
    let tail = text.get(end..)?;
    if !tail.starts_with([' ', '\t']) && !tail.is_empty() {
        return None;
    }
    let text = tail.trim_start_matches([' ', '\t']);
    Some(Marker {
        indent,
        ordering,
        text,
    })
}

impl Marker<'_> {
    fn same_list(self, item: Self) -> bool {
        self.indent == item.indent && self.ordering.is_some() == item.ordering.is_some()
    }
}

impl Parser<'_> {
    pub(super) fn list(
        &mut self,
        lines: &[&str],
        first: Marker<'_>,
        depth: usize,
    ) -> Result<(Block, usize)> {
        self.check_depth(depth + 1)?;
        let fancy = self.enabled(Extension::FancyLists);
        let tasks = self.enabled(Extension::TaskLists);
        let definitions = first.ordering.is_none() && definition(first.text).is_some();
        let mut position = 0;
        let mut items = Vec::new();
        let mut terms = Vec::new();
        let mut start = 1;
        let mut loose = false;
        while let Some(item) = lines.get(position).and_then(|line| marker(line, fancy)) {
            if !first.same_list(item)
                || (first.ordering.is_none() && definitions != definition(item.text).is_some())
            {
                break;
            }
            position += 1;
            let mut body = item.text;
            if item.ordering.is_some()
                && let Some((value, text)) = body
                    .strip_prefix("[@")
                    .and_then(|tail| tail.split_once(']'))
                && let Ok(number) = value.parse()
            {
                if items.is_empty() {
                    start = number;
                }
                body = text.trim_start();
            }
            if definitions && let Some((term, text)) = definition(body) {
                terms.push(self.inlines(term.trim(), depth + 1)?);
                body = text.trim_start();
            }
            let mut checkbox = None;
            if tasks
                && !definitions
                && let Some((mark, text)) = body.split_at_checked(3)
                && matches!(mark, "[ ]" | "[X]" | "[-]")
                && (text.is_empty() || text.starts_with(char::is_whitespace))
            {
                checkbox = Some(if mark == "[X]" { "☒" } else { "☐" });
                body = text.trim_start();
            }
            let mut child = vec![body];
            let mut blank = 0;
            while let Some(&line) = lines.get(position) {
                if line.trim().is_empty() {
                    blank += 1;
                    position += 1;
                    if blank == 2 {
                        break;
                    }
                    continue;
                }
                let trimmed = line.trim_start_matches([' ', '\t']);
                let indent = line.len() - trimmed.len();
                if indent <= item.indent {
                    break;
                }
                if blank > 0 {
                    child.push("");
                    loose = true;
                    blank = 0;
                }
                child.push(&line[item.indent + 1..]);
                position += 1;
            }
            let mut blocks = self.blocks(&child, depth + 1)?;
            if let Some(checkbox) = checkbox {
                if let Some(Block::Para(text) | Block::Plain(text)) = blocks.first_mut() {
                    text.splice(0..0, [Inline::Str(checkbox.into()), Inline::Space]);
                } else {
                    blocks.insert(0, Block::Para(vec![Inline::Str(checkbox.into())]));
                }
            }
            let next_item = lines
                .get(position)
                .and_then(|line| marker(line, fancy))
                .is_some_and(|next| {
                    first.same_list(next)
                        && (first.ordering.is_some()
                            || definitions == definition(next.text).is_some())
                });
            if definitions {
                let internal_blank = child.iter().skip(1).any(|line| line.is_empty());
                let keep_last = blank > 0 && (next_item || internal_blank);
                if !keep_last
                    && let Some(last @ Block::Para(_)) = blocks.last_mut()
                    && let Block::Para(text) = last
                {
                    *last = Block::Plain(std::mem::take(text));
                }
                if !keep_last && !internal_blank {
                    for block in &mut blocks {
                        if let Block::Para(text) = block {
                            *block = Block::Plain(std::mem::take(text));
                        }
                    }
                }
            } else {
                let is_list = |block: &Block| {
                    matches!(
                        block,
                        Block::BulletList(_) | Block::OrderedList(_, _) | Block::DefinitionList(_)
                    )
                };
                loose |= (blank == 1 && next_item && !blocks.last().is_some_and(is_list))
                    || blocks.iter().skip(1).any(|block| !is_list(block));
            }
            items.push(blocks);
            if blank == 2 {
                break;
            }
        }
        if !loose && !definitions {
            for item in &mut items {
                for block in item {
                    if let Block::Para(text) = block {
                        *block = Block::Plain(std::mem::take(text));
                    }
                }
            }
        }
        Ok((
            if definitions {
                Block::DefinitionList(
                    terms
                        .into_iter()
                        .zip(items.into_iter().map(|item| vec![item]))
                        .collect(),
                )
            } else if let Some((style, delim)) = first.ordering {
                Block::OrderedList(
                    ListAttributes {
                        start,
                        style,
                        delim,
                    },
                    items,
                )
            } else {
                Block::BulletList(items)
            },
            position,
        ))
    }
}

fn definition(text: &str) -> Option<(&str, &str)> {
    text.split_once(" :: ")
        .or_else(|| text.strip_suffix(" ::").map(|term| (term, "")))
}
