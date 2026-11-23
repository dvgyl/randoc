//! Section-based document chunks and links between chunks.

mod tree;

use std::collections::HashMap;

use randoc_ast::walk::{query_inlines, walk_blocks_mut, walk_inlines_mut};
use randoc_ast::{Block, Inline, Pandoc, Text};

use crate::shared::{IdentifierSet, auto_identifier, sections::make_sections, stringify};
use crate::{Extensions, Result};

pub use tree::{block_attrs, inline_attr, inspect_tree};

/// One part of a document split at section boundaries.
#[derive(Debug)]
pub struct Chunk {
    /// The output path with the chunk template applied.
    pub path: Text,
    /// The identifier of the section. The text before the first section uses `-1`.
    pub id: Text,
    /// The heading level. The text before the first section uses level 0.
    pub level: i64,
    /// The section number, if present.
    pub number: Option<Text>,
    /// The heading text and inline formatting.
    pub title: Vec<Inline>,
    /// The section and its content.
    pub blocks: Vec<Block>,
}

/// Splits a document at sections up to `split_level` and resolves local links.
///
/// A path template uses `%n` for the one-based chunk index, padded to three
/// digits, `%s` for the section number, `%i` for the section identifier,
/// and `%h` for the heading text.
pub fn split_into_chunks(
    doc: &Pandoc,
    split_level: i64,
    path_template: &str,
    number_sections: bool,
    depth: usize,
) -> Result<Vec<Chunk>> {
    inspect_tree(&doc.blocks, std::iter::empty(), depth, |_| {})?;
    let mut blocks = doc.blocks.clone();
    let mut ids = IdentifierSet::new();
    walk_blocks_mut(&mut blocks, |block| {
        if let Block::Header(_, attr, text) = block {
            if attr.id.is_empty() {
                attr.id = ids.unique(&auto_identifier(text, Extensions::default()));
            } else {
                ids.register(&attr.id);
            }
        }
    });
    let sections = make_sections(blocks, number_sections, None, depth)?;
    inspect_tree(&sections, std::iter::empty(), depth, |_| {})?;

    let mut chunks: Vec<Chunk> = Vec::new();
    let mut pending: Vec<_> = sections.into_iter().rev().collect();
    while let Some(mut block) = pending.pop() {
        let Some((level, id, number, title)) = section(&block, split_level) else {
            if chunks.is_empty() {
                chunks.push(Chunk {
                    path: chunk_path(path_template, 1, "0", "-1", ""),
                    id: "-1".into(),
                    level: 0,
                    number: None,
                    title: Vec::new(),
                    blocks: Vec::new(),
                });
            }
            if let Some(chunk) = chunks.last_mut() {
                chunk.blocks.push(block);
            }
            continue;
        };
        let id = id.to_owned();
        let number = number.map(str::to_owned);
        let title = title.to_vec();
        if let Block::Div(_, children) = &mut block
            && let Some(next) = children
                .iter()
                .position(|child| section(child, split_level).is_some())
        {
            pending.extend(children.drain(next..).rev());
        }
        chunks.push(Chunk {
            path: chunk_path(
                path_template,
                chunks.len() + 1,
                number.as_deref().unwrap_or_default(),
                &id,
                &stringify(&title),
            ),
            id,
            level,
            number,
            title,
            blocks: vec![block],
        });
    }
    rewrite_chunk_links(
        chunks
            .iter_mut()
            .map(|chunk| (chunk.path.as_str(), &mut chunk.blocks)),
        str::to_owned,
    );
    Ok(chunks)
}

fn section(block: &Block, split_level: i64) -> Option<(i64, &str, Option<&str>, &[Inline])> {
    if let Block::Div(attr, children) = block
        && attr.classes.first().is_some_and(|class| class == "section")
        && let Some(Block::Header(level, _, title)) = children.first()
        && *level <= split_level
    {
        return Some((*level, &attr.id, attr.get("number"), title));
    }
    None
}

fn chunk_path(template: &str, index: usize, number: &str, id: &str, title: &str) -> Text {
    let index = format!("{index:03}");
    let mut parts = template.split('%');
    let mut path = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        let value = match part.as_bytes().first() {
            Some(b'n') => &index,
            Some(b's') => number,
            Some(b'i') => id,
            Some(b'h') => title,
            _ => {
                path.push('%');
                path.push_str(part);
                continue;
            }
        };
        path.push_str(value);
        path.push_str(&part[1..]);
    }
    path
}

/// Rewrites fragment links to the path of the chunk that contains the target.
///
/// The first chunk with an identifier owns it. `fragment` formats the target
/// identifier for the output format.
pub fn rewrite_chunk_links<'a>(
    chunks: impl IntoIterator<Item = (&'a str, &'a mut Vec<Block>)>,
    fragment: impl Fn(&str) -> Text,
) {
    let mut chunks: Vec<_> = chunks.into_iter().collect();
    let mut paths = HashMap::new();
    for (path, blocks) in &mut chunks {
        let mut record = |attr: &randoc_ast::Attr| {
            if !attr.id.is_empty() {
                paths.entry(attr.id.clone()).or_insert(*path);
            }
        };
        walk_blocks_mut(*blocks, |block| block_attrs(block, |attr| record(attr)));
        query_inlines(*blocks, |inline| match inline {
            Inline::Code(attr, _)
            | Inline::Span(attr, _)
            | Inline::Image(attr, _, _)
            | Inline::Link(attr, _, _) => record(attr),
            _ => {}
        });
    }
    for (_, blocks) in chunks {
        walk_inlines_mut(blocks, |inline| {
            if let Inline::Link(_, _, target) = inline
                && let Some(id) = target.url.strip_prefix('#')
                && let Some(path) = paths.get(id)
            {
                target.url = format!("{path}#{}", fragment(id));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use randoc_ast::{Attr, Target};

    use super::*;

    #[test]
    fn chunks_use_the_callers_depth_limit() {
        let mut blocks = vec![Block::Header(
            1,
            Attr::default(),
            vec![Inline::Str("Deep".into())],
        )];
        for _ in 0..150 {
            blocks = vec![Block::Div(Attr::default(), blocks)];
        }
        let doc = Pandoc {
            blocks,
            ..Pandoc::default()
        };
        assert_eq!(
            split_into_chunks(&doc, 2, "%i.html", true, 200)
                .unwrap()
                .len(),
            1
        );
        assert!(split_into_chunks(&doc, 2, "%i.html", true, 10).is_err());
    }

    #[test]
    fn chunks_keep_section_metadata_and_resolve_links() {
        let link = Inline::Link(
            Attr::default(),
            vec![Inline::Str("Second".into())],
            Target {
                url: "#sub".into(),
                title: String::new(),
            },
        );
        let doc = Pandoc {
            blocks: vec![
                Block::Para(vec![link]),
                Block::Header(1, Attr::default(), vec![Inline::Str("First".into())]),
                Block::Header(2, Attr::default(), vec![Inline::Str("Sub".into())]),
            ],
            ..Pandoc::default()
        };
        let chunks = split_into_chunks(&doc, 2, "%n-%s-%i-%h.html", true, 16).unwrap();
        let paths: Vec<_> = chunks.iter().map(|chunk| chunk.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "001-0--1-.html",
                "002-1-first-First.html",
                "003-1.1-sub-Sub.html"
            ]
        );
        assert_eq!(chunks[2].number.as_deref(), Some("1.1"));
        assert!(matches!(
            chunks[0].blocks.as_slice(),
            [Block::Para(inlines)] if matches!(
                inlines.as_slice(),
                [Inline::Link(_, _, target)] if target.url == "003-1.1-sub-Sub.html#sub"
            )
        ));
        assert!(split_into_chunks(&doc, 2, "%i.html", true, 1).is_err());
    }
}
