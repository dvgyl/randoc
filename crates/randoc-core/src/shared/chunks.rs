//! Section-based document chunks and links between chunks.

mod tree;

use std::cmp::Ordering;
use std::collections::HashMap;
use std::ops::Deref;

use randoc_ast::walk::{query_inlines, walk_blocks_mut, walk_inlines_mut};
use randoc_ast::{Attr, Block, Inline, Pandoc, Text};

use crate::shared::{IdentifierSet, auto_identifier, sections::make_sections, stringify, toc};
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

/// Options for splitting a document.
#[derive(Debug, Clone, Copy)]
pub struct ChunkOptions<'a> {
    /// The largest heading level that starts a chunk.
    pub level: i64,
    /// The path template. See [`split_into_chunks`] for the substitutions.
    pub path_template: &'a str,
    /// Whether to number the sections.
    pub number_sections: bool,
    /// The level to assign to the lowest heading before splitting.
    pub base_heading_level: Option<i64>,
}

impl Default for ChunkOptions<'_> {
    fn default() -> Self {
        Self {
            level: 1,
            path_template: "chunk-%n",
            number_sections: false,
            base_heading_level: None,
        }
    }
}

/// A chunk with its section visibility and parent index.
#[derive(Debug)]
pub struct RichChunk {
    /// The path, heading, and content of the chunk.
    pub chunk: Chunk,
    /// Whether the section has the `unlisted` class.
    pub unlisted: bool,
    /// The zero-based index of the parent chunk, if present.
    pub up: Option<usize>,
}

impl Deref for RichChunk {
    type Target = Chunk;

    fn deref(&self) -> &Self::Target {
        &self.chunk
    }
}

/// The chunks and the full table of contents of a document.
#[derive(Debug)]
pub struct ChunkedDoc {
    /// The chunks in document order.
    pub chunks: Vec<RichChunk>,
    /// The headings at all levels, with links to their chunks.
    pub toc: Vec<toc::TocEntry>,
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
    let sections = section_document(doc, number_sections, None, depth)?;
    let mut chunks = split_sections(sections, Some(split_level), path_template);
    rewrite_chunk_links(chunk_blocks(&mut chunks), str::to_owned);
    Ok(chunks.into_iter().map(|chunk| chunk.chunk).collect())
}

/// Splits a document and builds its full table of contents.
/// A nonpositive level keeps all blocks in one chunk. An empty document has no chunks.
pub fn split_into_chunked_doc(
    doc: &Pandoc,
    options: &ChunkOptions<'_>,
    depth: usize,
) -> Result<ChunkedDoc> {
    let sections = section_document(
        doc,
        options.number_sections,
        options.base_heading_level,
        depth,
    )?;
    let mut toc = toc::toc_entries(&sections, &|id| format!("#{id}"), depth)?;
    let level = (options.level > 0).then_some(options.level);
    let mut chunks = split_sections(sections, level, options.path_template);
    let paths = rewrite_links(chunk_blocks(&mut chunks), str::to_owned);
    attach_toc_paths(&mut toc, &paths);
    Ok(ChunkedDoc { chunks, toc })
}

fn attach_toc_paths(entries: &mut [toc::TocEntry], paths: &HashMap<Text, &str>) {
    for entry in entries {
        if let Some(path) = paths.get(&entry.id) {
            entry.target.insert_str(0, path);
        }
        attach_toc_paths(&mut entry.children, paths);
    }
}

fn chunk_blocks(chunks: &mut [RichChunk]) -> impl Iterator<Item = (&str, &mut Vec<Block>)> {
    chunks
        .iter_mut()
        .map(|entry| (entry.chunk.path.as_str(), &mut entry.chunk.blocks))
}

fn section_document(
    doc: &Pandoc,
    number_sections: bool,
    base_level: Option<i64>,
    depth: usize,
) -> Result<Vec<Block>> {
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
    let sections = make_sections(blocks, number_sections, base_level, depth)?;
    inspect_tree(&sections, std::iter::empty(), depth, |_| {})?;

    Ok(sections)
}

fn split_sections(
    sections: Vec<Block>,
    split_level: Option<i64>,
    path_template: &str,
) -> Vec<RichChunk> {
    let mut chunks: Vec<RichChunk> = Vec::new();
    let mut pending: Vec<_> = sections.into_iter().rev().collect();
    while let Some(mut block) = pending.pop() {
        let Some((level, attr, title)) = section(&block, split_level) else {
            if let Some(chunk) = chunks.last_mut() {
                chunk.chunk.blocks.push(block);
            } else {
                chunks.push(RichChunk {
                    chunk: Chunk {
                        path: chunk_path(path_template, 1, "0", "-1", ""),
                        id: "-1".into(),
                        level: 0,
                        number: None,
                        title: Vec::new(),
                        blocks: vec![block],
                    },
                    unlisted: false,
                    up: None,
                });
            }
            continue;
        };
        let id = attr.id.clone();
        let number = attr.get("number").map(str::to_owned);
        let unlisted = attr.classes.iter().any(|class| class == "unlisted");
        let title = title.to_vec();
        let up = chunks
            .last()
            .and_then(|previous| match previous.level.cmp(&level) {
                Ordering::Less => Some(chunks.len() - 1),
                Ordering::Equal => previous.up,
                Ordering::Greater => None,
            });
        if let Block::Div(_, children) = &mut block
            && let Some(next) = children
                .iter()
                .position(|child| section(child, split_level).is_some())
        {
            pending.extend(children.drain(next..).rev());
        }
        chunks.push(RichChunk {
            chunk: Chunk {
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
            },
            unlisted,
            up,
        });
    }
    chunks
}

fn section(block: &Block, split_level: Option<i64>) -> Option<(i64, &Attr, &[Inline])> {
    let split_level = split_level?;
    if let Block::Div(attr, children) = block
        && attr.classes.first().is_some_and(|class| class == "section")
        && let Some(Block::Header(level, _, title)) = children.first()
        && *level <= split_level
    {
        return Some((*level, attr, title));
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
    rewrite_links(chunks, fragment);
}

fn rewrite_links<'a>(
    chunks: impl IntoIterator<Item = (&'a str, &'a mut Vec<Block>)>,
    fragment: impl Fn(&str) -> Text,
) -> HashMap<Text, &'a str> {
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
    paths
}

#[cfg(test)]
mod tests {
    use randoc_ast::{Attr, Target};

    use super::*;

    fn heading(level: i64, title: &str) -> Block {
        Block::Header(level, Attr::default(), vec![Inline::Str(title.into())])
    }

    #[test]
    fn chunked_doc_with_no_blocks_has_no_chunks_or_toc() {
        for level in [1, 0, -1] {
            let options = ChunkOptions {
                level,
                ..ChunkOptions::default()
            };
            let split = split_into_chunked_doc(&Pandoc::default(), &options, 16).unwrap();
            assert!(split.chunks.is_empty());
            assert!(split.toc.is_empty());
        }
    }

    #[test]
    fn chunked_doc_keeps_all_blocks_at_nonpositive_levels() {
        let doc = Pandoc {
            blocks: vec![heading(1, "First"), heading(2, "Second")],
            ..Pandoc::default()
        };
        for level in [0, -2] {
            let options = ChunkOptions {
                level,
                path_template: "%n-%s-%i-%h",
                ..ChunkOptions::default()
            };
            let split = split_into_chunked_doc(&doc, &options, 16).unwrap();
            let [chunk] = split.chunks.as_slice() else {
                panic!("expected one chunk");
            };
            assert_eq!(chunk.path, "001-0--1-");
            assert_eq!(chunk.id, "-1");
            assert_eq!(chunk.level, 0);
            assert!(chunk.title.is_empty());
            assert_eq!(chunk.up, None);
            assert!(matches!(chunk.blocks[0], Block::Div(..)));
            assert_eq!(split.toc[0].target, "001-0--1-#first");
            assert_eq!(split.toc[0].children[0].target, "001-0--1-#second");
        }
    }

    #[test]
    fn chunked_doc_shifts_headings_before_splitting() {
        let doc = Pandoc {
            blocks: vec![heading(1, "First"), heading(1, "Second")],
            ..Pandoc::default()
        };
        let options = ChunkOptions {
            base_heading_level: Some(2),
            ..ChunkOptions::default()
        };
        let split = split_into_chunked_doc(&doc, &options, 16).unwrap();
        assert_eq!(split.chunks.len(), 1);
        assert_eq!(split.chunks[0].id, "-1");
        assert_eq!(split.toc[0].level, 2);
        assert_eq!(split.toc[1].target, "chunk-001#second");

        let split = split_into_chunked_doc(
            &doc,
            &ChunkOptions {
                level: 2,
                ..options
            },
            16,
        )
        .unwrap();
        assert_eq!(split.chunks.len(), 2);
        assert_eq!(split.chunks[0].id, "first");
        assert_eq!(split.chunks[0].level, 2);
        assert_eq!(split.toc[1].target, "chunk-002#second");
    }

    #[test]
    fn chunked_doc_tracks_parent_indices_and_full_toc_targets() {
        let doc = Pandoc {
            blocks: vec![
                Block::Para(vec![Inline::Link(
                    Attr::default(),
                    vec![Inline::Str("deep".into())],
                    Target {
                        url: "#deep".into(),
                        title: String::new(),
                    },
                )]),
                Block::Header(
                    1,
                    Attr::new("", vec!["unlisted".into()], vec![]),
                    vec![Inline::Str("First".into())],
                ),
                heading(2, "Child"),
                heading(3, "Deep"),
                heading(2, "Child"),
                heading(1, "Last"),
            ],
            ..Pandoc::default()
        };
        let options = ChunkOptions {
            level: 2,
            path_template: "%n-%s-%i-%h-%%-%x-%",
            number_sections: true,
            ..ChunkOptions::default()
        };
        let split = split_into_chunked_doc(&doc, &options, 16).unwrap();
        let parents: Vec<_> = split.chunks.iter().map(|chunk| chunk.up).collect();
        assert_eq!(parents, [None, Some(0), Some(1), Some(1), None]);
        assert!(split.chunks[1].unlisted);
        assert!(split.toc[0].unlisted);
        assert_eq!(split.chunks[3].id, "child-1");
        assert_eq!(split.chunks[3].number.as_deref(), Some("1.2"));
        let deep = &split.toc[0].children[0].children[0];
        assert_eq!(deep.target, "003-1.1-child-Child-%%-%x-%#deep");
        assert_eq!(deep.number.as_deref(), Some("1.1.1"));
        assert_eq!(
            split.toc[0].children[1].target,
            "004-1.2-child-1-Child-%%-%x-%#child-1"
        );
        assert!(matches!(
            split.chunks[0].blocks.as_slice(),
            [Block::Para(inlines)] if matches!(
                inlines.as_slice(),
                [Inline::Link(_, _, target)] if target.url == deep.target
            )
        ));
        assert!(split_into_chunked_doc(&doc, &options, 2).is_err());
    }

    #[test]
    fn chunked_doc_assigns_heading_identifiers_in_document_order() {
        let doc = Pandoc {
            blocks: vec![
                heading(1, "First"),
                Block::Header(
                    1,
                    Attr::new("first", vec![], vec![]),
                    vec![Inline::Str("Other".into())],
                ),
                heading(1, "First"),
            ],
            ..Pandoc::default()
        };
        let split = split_into_chunked_doc(&doc, &ChunkOptions::default(), 16).unwrap();
        let ids: Vec<_> = split.chunks.iter().map(|chunk| chunk.id.as_str()).collect();
        assert_eq!(ids, ["first", "first", "first-1"]);
    }

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
