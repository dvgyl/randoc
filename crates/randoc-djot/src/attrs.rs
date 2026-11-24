use randoc_ast::{Attr, Block, Inline};

pub(crate) fn merge(target: &mut Attr, other: Attr) {
    if !other.id.is_empty() {
        target.id = other.id;
    }
    target.classes.extend(other.classes);
    for (key, value) in other.attributes {
        if let Some((_, old)) = target.attributes.iter_mut().find(|(k, _)| *k == key) {
            *old = value;
        } else {
            target.attributes.push((key, value));
        }
    }
}

pub(crate) fn block(attr: Attr, block: Block) -> Block {
    if attr.is_empty() {
        return block;
    }
    let mut block = block;
    match &mut block {
        Block::CodeBlock(old, _)
        | Block::Header(_, old, _)
        | Block::Div(old, _)
        | Block::Figure(old, _, _) => {
            let mut attr = attr;
            merge(&mut attr, std::mem::take(old));
            *old = attr;
        }
        Block::Table(table) => {
            merge(&mut table.attr, attr);
        }
        _ => {
            let mut attr = attr;
            let position = attr
                .attributes
                .iter()
                .position(|(key, _)| key == "data-pos")
                .unwrap_or(attr.attributes.len());
            attr.attributes
                .insert(position, ("wrapper".into(), "1".into()));
            block = Block::Div(attr, vec![block]);
        }
    }
    block
}

pub(crate) fn inline(attr: Attr, mut inline: Inline) -> Inline {
    if attr.is_empty() {
        return inline;
    }
    match &mut inline {
        Inline::Code(old, _)
        | Inline::Image(old, _, _)
        | Inline::Link(old, _, _)
        | Inline::Span(old, _) => {
            merge(old, attr);
            inline
        }
        _ => {
            let mut attr = attr;
            let position = attr
                .attributes
                .iter()
                .position(|(key, _)| key == "data-pos")
                .unwrap_or(attr.attributes.len());
            attr.attributes
                .insert(position, ("wrapper".into(), "1".into()));
            Inline::Span(attr, vec![inline])
        }
    }
}

pub(crate) fn parse(mut text: &str) -> Option<(Attr, &str)> {
    text = text.strip_prefix('{')?;
    let mut attr = Attr::default();
    loop {
        text = text.trim_start();
        if let Some(rest) = text.strip_prefix('}') {
            return Some((attr, rest));
        }
        if let Some(comment) = text.strip_prefix('%') {
            let end = comment.find(['%', '}'])?;
            text = if comment.as_bytes()[end] == b'%' {
                &comment[end + 1..]
            } else {
                &comment[end..]
            };
            continue;
        }
        let first = text.as_bytes().first().copied()?;
        if matches!(first, b'#' | b'.') {
            let body = &text[1..];
            let end = body
                .find(|c: char| c.is_whitespace() || matches!(c, '}' | '{' | '=' | '"'))
                .unwrap_or(body.len());
            if end == 0 {
                return None;
            }
            if first == b'#' {
                attr.id = body[..end].into();
            } else {
                attr.classes.push(body[..end].into());
            }
            text = &body[end..];
            continue;
        }
        let end = text.find(|c: char| c.is_whitespace() || matches!(c, '=' | '}' | '{'))?;
        let key = &text[..end];
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ':'))
        {
            return None;
        }
        let body = text[end..].strip_prefix('=')?;
        let value;
        if let Some(body) = body.strip_prefix('"') {
            let mut chars = body.char_indices();
            let mut result = String::new();
            loop {
                let (offset, c) = chars.next()?;
                if c == '"' {
                    text = &body[offset + 1..];
                    break;
                }
                if c == '\\'
                    && let Some((_, next)) = chars.next()
                {
                    result.push(next);
                } else {
                    result.push(c);
                }
            }
            value = result;
        } else {
            let end = body
                .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':')))
                .unwrap_or(body.len());
            if end == 0 {
                return None;
            }
            value = body[..end].into();
            text = &body[end..];
        }
        attr.attributes.push((key.into(), value));
    }
}
