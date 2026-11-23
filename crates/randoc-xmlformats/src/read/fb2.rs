use randoc_archive::media::{base64_decode_lenient, media_key};
use randoc_ast::{Attr, Block, Inline, MetaValue, Target};
use randoc_core::{LogMessage, MediaItem, Result};

use super::metadata::name_parts;
use super::{Element, Node, Read, attr, class, id, limit_error, name, words};

impl<'a> Read<'a, '_> {
    pub(super) fn fb2_block(
        &mut self,
        el: &'a Element,
        level: i64,
        depth: usize,
        out: &mut Vec<Block>,
    ) -> Result<()> {
        let tag = name(el);
        let block = match tag {
            "FictionBook" | "body" | "poem" => {
                if tag != "body" || attr(el, "name") != "notes" {
                    out.extend(self.blocks(&el.children, level, depth)?);
                }
                return Ok(());
            }
            "description" => {
                if let Some(info) = el.child_local("title-info") {
                    self.fb2_meta(info, depth)?;
                }
                return Ok(());
            }
            "section" => {
                let mut a = id(el);
                a.classes.push("section".into());
                Block::Div(a, self.blocks(&el.children, level + 1, depth)?)
            }
            "title" => Block::Header(
                level,
                id(el),
                self.join_inlines(el.children_local("p"), depth)?,
            ),
            "subtitle" => Block::Header(
                level,
                class("unnumbered"),
                self.inlines(&el.children, depth)?,
            ),
            "p" | "text-author" | "date" => Block::Para(self.inlines(&el.children, depth)?),
            "cite" => Block::BlockQuote(self.blocks(&el.children, level, depth)?),
            "epigraph" => Block::Div(class("epigraph"), self.blocks(&el.children, level, depth)?),
            "stanza" => {
                out.extend(self.blocks_skipping(&el.children, level, depth, &["v"])?);
                let mut lines = Vec::new();
                for v in el.children_local("v") {
                    lines.push(self.inlines(&v.children, depth)?);
                }
                Block::LineBlock(lines)
            }
            "empty-line" => Block::HorizontalRule,
            "image" => Block::Para(vec![image(el, true)]),
            "binary" | "table" => return Ok(()),
            _ => {
                self.context.report(LogMessage::IgnoredElement(tag.into()));
                return Ok(());
            }
        };
        out.push(block);
        Ok(())
    }

    fn fb2_meta(&mut self, info: &'a Element, depth: usize) -> Result<()> {
        for (tag, key) in [("book-title", "title"), ("date", "date")] {
            if let Some(child) = info.child_local(tag) {
                self.meta_inlines(key, child, depth)?;
            }
        }
        let authors: Vec<_> = info
            .children_local("author")
            .map(|el| {
                MetaValue::MetaInlines(words(&name_parts(
                    el,
                    &["first-name", "middle-name", "last-name", "nickname"],
                )))
            })
            .collect();
        if !authors.is_empty() {
            self.meta.insert("author", MetaValue::MetaList(authors));
        }
        if let Some(annotation) = info.child_local("annotation") {
            let blocks = self.blocks(&annotation.children, 1, depth)?;
            self.meta.insert("abstract", MetaValue::MetaBlocks(blocks));
        }
        if let Some(keywords) = info.child_local("keywords") {
            self.meta.insert(
                "keywords",
                MetaValue::MetaList(
                    keywords
                        .all_text()
                        .split(',')
                        .map(|word| MetaValue::MetaString(word.trim().into()))
                        .collect(),
                ),
            );
        }
        if let Some(image) = info
            .child_local("coverpage")
            .and_then(|page| page.child_local("image"))
        {
            self.meta.insert(
                "cover-image",
                MetaValue::MetaString(attr(image, "href").trim_start_matches('#').into()),
            );
        }
        Ok(())
    }

    pub(super) fn media(&mut self, nodes: &[Node]) -> Result<()> {
        let mut staged = Vec::new();
        let limit = usize::try_from(self.context.limits.max_resource_size).unwrap_or(usize::MAX);
        let mut remaining = limit;
        for root in nodes {
            let Node::Element(root) = root else { continue };
            for binary in root.children_local("binary") {
                let (Some(id), Some(mime)) =
                    (binary.attr_local("id"), binary.attr_local("content-type"))
                else {
                    continue;
                };
                let bytes = base64_decode_lenient(&binary.text(), remaining)
                    .ok_or_else(|| limit_error("FB2 decoded media size", limit))?;
                remaining -= bytes.len();
                staged.push((
                    media_key(id, Some(mime), &bytes),
                    MediaItem {
                        mime_type: mime.into(),
                        contents: bytes,
                    },
                ));
            }
        }
        for (key, item) in staged {
            self.context.media.insert(key, item);
        }
        Ok(())
    }
}

pub(super) fn image(el: &Element, block: bool) -> Inline {
    let href = attr(el, "href");
    let alt = el.attr_local("alt").map(|text| Inline::Str(text.into()));
    let (attributes, title) = if block {
        (id(el), attr(el, "title"))
    } else {
        (Attr::default(), "")
    };
    Inline::Image(
        attributes,
        alt.into_iter().collect(),
        Target::new(href.strip_prefix('#').unwrap_or(href), title),
    )
}
