use std::collections::{HashMap, HashSet};

use randoc_ast::walk::{query_blocks, query_inlines, walk_blocks_mut, walk_inlines_mut};
use randoc_ast::{Attr, Block, Inline};
use randoc_core::shared::{auto_identifier, sections::make_sections, stringify};
use randoc_core::{Result, WriterOptions};

use randoc_html::epub_identifier;

use crate::tree;

pub(super) struct Chapter {
    pub blocks: Vec<Block>,
    pub matter: &'static str,
}

pub(super) struct Heading {
    pub level: i64,
    pub label: String,
    pub inlines: Vec<Inline>,
    pub number: Option<String>,
    pub href: String,
}

pub(super) fn name(index: usize) -> String {
    format!("ch{:03}.xhtml", index + 1)
}

pub(super) fn split(
    blocks: Vec<Block>,
    title: &[Inline],
    options: &WriterOptions,
    max_depth: usize,
) -> Result<(Vec<Chapter>, Vec<Heading>)> {
    let sections = make_sections(blocks, options.number_sections, None, max_depth)?;
    tree::inspect(&sections, std::iter::empty(), max_depth)?;
    let mut blocks = Vec::new();
    flatten(sections, &mut blocks);
    let mut used = HashSet::new();
    let mut suffixes = HashMap::new();
    query_blocks(&blocks, |block| {
        if let Block::Header(_, attr, _) = block
            && !attr.id.is_empty()
        {
            used.insert(attr.id.clone());
        }
    });
    walk_blocks_mut(&mut blocks, |block| {
        if let Block::Header(_, attr, text) = block
            && attr.id.is_empty()
        {
            attr.id = unique_id(text, &mut used, &mut suffixes, options);
        }
    });
    let split_level = options.split_level.clamp(1, 6);
    let starts_chapter =
        |b: &Block| matches!(b, Block::Header(level, _, _) if *level <= split_level);
    if !blocks.first().is_some_and(starts_chapter) {
        let attr = Attr::new(
            unique_id(title, &mut used, &mut suffixes, options),
            vec!["unnumbered".into()],
            Vec::new(),
        );
        blocks.insert(0, Block::Header(1, attr, title.to_vec()));
    }
    let mut chapters = Vec::new();
    let mut headings = Vec::new();
    for block in blocks {
        if starts_chapter(&block) {
            let Block::Header(_, attr, _) = &block else {
                continue;
            };
            chapters.push(Chapter {
                blocks: Vec::new(),
                matter: matter(attr),
            });
        }
        let index = chapters.len().saturating_sub(1);
        if let Block::Header(level, attr, text) = &block
            && *level <= options.toc_depth
            && !attr.has_class("unlisted")
        {
            headings.push(Heading {
                level: *level,
                label: stringify(text),
                inlines: text.clone(),
                number: options
                    .number_sections
                    .then(|| attr.get("number").map(str::to_owned))
                    .flatten(),
                href: format!(
                    "text/{}#{}",
                    name(index),
                    epub_identifier(&options.identifier_prefix, &attr.id)
                ),
            });
        }
        if let Some(chapter) = chapters.last_mut() {
            chapter.blocks.push(block);
        }
    }
    rewrite_links(&mut chapters, options);
    Ok((chapters, headings))
}

fn flatten(sections: Vec<Block>, output: &mut Vec<Block>) {
    for section in sections {
        match section {
            Block::Div(attr, items)
                if matches!(items.first(), Some(Block::Header(..)))
                    && attr.classes.first().is_some_and(|class| class == "section") =>
            {
                let mut items = items.into_iter();
                if let Some(Block::Header(level, mut heading, text)) = items.next() {
                    if heading.id.is_empty() {
                        heading.id = attr.id;
                    }
                    for class in attr.classes.into_iter().skip(1) {
                        if !heading.classes.contains(&class) {
                            heading.classes.push(class);
                        }
                    }
                    for (key, value) in attr.attributes {
                        if heading.get(&key).is_none() {
                            heading.attributes.push((key, value));
                        }
                    }
                    output.push(Block::Header(level, heading, text));
                    flatten(items.collect(), output);
                }
            }
            other => output.push(other),
        }
    }
}

fn unique_id(
    text: &[Inline],
    used: &mut HashSet<String>,
    suffixes: &mut HashMap<String, usize>,
    options: &WriterOptions,
) -> String {
    let base = auto_identifier(text, options.extensions);
    let base = if base.is_empty() {
        "section".to_owned()
    } else {
        base
    };
    if used.insert(base.clone()) {
        return base;
    }
    let number = suffixes.entry(base.clone()).or_insert(0);
    loop {
        *number += 1;
        let id = format!("{base}-{number}");
        if used.insert(id.clone()) {
            return id;
        }
    }
}

fn matter(attr: &Attr) -> &'static str {
    let mut types = attr.get("epub:type").unwrap_or_default().split_whitespace();
    for kind in &mut types {
        match kind {
            "prologue" | "foreword" | "preface" | "dedication" | "acknowledgments"
            | "titlepage" | "toc" | "introduction" | "halftitlepage" | "copyright-page" => {
                return "frontmatter";
            }
            "appendix" | "afterword" | "bibliography" | "colophon" | "epilogue" | "glossary"
            | "index" => return "backmatter",
            _ => {}
        }
    }
    "bodymatter"
}

fn rewrite_links(chapters: &mut [Chapter], options: &WriterOptions) {
    let mut ids = HashMap::new();
    for (index, chapter) in chapters.iter_mut().enumerate() {
        let mut record = |attr: &Attr| {
            if !attr.id.is_empty() {
                ids.entry(attr.id.clone()).or_insert(index);
            }
        };
        walk_blocks_mut(&mut chapter.blocks, |block| {
            tree::block_attrs(block, |attr| record(attr))
        });
        query_inlines(&chapter.blocks, |inline| match inline {
            Inline::Code(attr, _)
            | Inline::Span(attr, _)
            | Inline::Image(attr, _, _)
            | Inline::Link(attr, _, _) => record(attr),
            _ => {}
        });
    }
    for chapter in chapters {
        walk_inlines_mut(&mut chapter.blocks, |inline| {
            if let Inline::Link(_, _, target) = inline
                && let Some(id) = target.url.strip_prefix('#')
                && let Some(&owner) = ids.get(id)
            {
                target.url = format!(
                    "{}#{}",
                    name(owner),
                    epub_identifier(&options.identifier_prefix, id)
                );
            }
        });
    }
}
