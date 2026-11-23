use randoc_archive::media::{base64_encode, decode_data_uri, mime_from_extension};
use randoc_ast::{Block, MetaValue, Pandoc, Target};
use randoc_core::{
    LogMessage, Result,
    shared::{show_string, stringify_meta},
};
use randoc_layout::{Doc, hcat, literal};

use super::{Write, compact_tag, empty_tag, limit_error, meta_items, tagged};

impl Write<'_> {
    pub(super) fn fb2_image(&mut self, target: &Target, alt: &str, block: bool) -> Result<Doc> {
        let limit = self.remaining_media;
        let resource = if let Some(item) = self.context.media.get(&target.url) {
            if item.contents.len() > limit {
                return Err(limit_error("FB2 embedded media size", limit));
            }
            Some((item.mime_type.clone(), item.contents.clone()))
        } else if target.url.starts_with("data:") {
            decode_data_uri(&target.url, limit as u64)
        } else {
            let paths = self.context.state.resource_path.clone();
            let candidates = std::iter::once(target.url.clone()).chain(
                paths
                    .into_iter()
                    .map(|base| format!("{base}/{}", target.url)),
            );
            candidates
                .filter_map(|path| self.context.read_file_limited(&path, limit).ok())
                .next()
                .map(|bytes| {
                    (
                        mime_from_extension(&target.url)
                            .unwrap_or("application/octet-stream")
                            .to_owned(),
                        bytes,
                    )
                })
        };
        let Some((mime, bytes)) = resource else {
            self.context.report(LogMessage::CouldNotFetchResource(
                target.url.clone(),
                format!("PandocResourceNotFound {}", show_string(&target.url)),
            ));
            return Ok(if block {
                compact_tag("p", "", self.esc(alt))
            } else {
                self.esc(alt)
            });
        };
        let encoded_size = bytes
            .len()
            .div_ceil(3)
            .checked_mul(4)
            .filter(|size| *size <= self.remaining_encoded_media)
            .ok_or_else(|| limit_error("FB2 encoded media size", self.remaining_encoded_media))?;
        self.remaining_media -= bytes.len();
        self.remaining_encoded_media -= encoded_size;
        let key = format!("image{}", self.binaries.len() + 1);
        let binary_attrs = self.attr("id", &key) + &self.attr("content-type", &mime);
        self.binaries.push(compact_tag(
            "binary",
            &binary_attrs,
            literal(base64_encode(&bytes)),
        ));
        let mut attrs = self.attr("l:href", &format!("#{key}"))
            + &self.attr(
                "l:type",
                if block {
                    "imageType"
                } else {
                    "inlineImageType"
                },
            );
        if !alt.is_empty() {
            attrs.push_str(&self.attr("alt", alt));
        }
        if !target.title.is_empty() {
            attrs.push_str(&self.attr("title", &target.title));
        }
        Ok(empty_tag("image", &attrs))
    }

    pub(super) fn fb2(&mut self, doc: &Pandoc, blocks: &[Block]) -> Result<String> {
        let mut info = Vec::new();
        let genre = doc
            .meta
            .get("genre")
            .map(stringify_meta)
            .unwrap_or_else(|| "unrecognised".into());
        info.push(compact_tag("genre", "", self.esc(&genre)));
        if let Some(authors) = doc.meta.get("author") {
            for value in meta_items(authors) {
                let text = stringify_meta(value);
                let words: Vec<_> = text.split_whitespace().collect();
                let fields = match words.as_slice() {
                    [] => Doc::default(),
                    [one] => tagged("nickname", "", self.esc(one)),
                    [first, middle @ .., last] => {
                        let middle = if middle.is_empty() {
                            Doc::default()
                        } else {
                            tagged("middle-name", "", self.esc(&middle.join(" ")))
                        };
                        tagged("first-name", "", self.esc(first))
                            + middle
                            + tagged("last-name", "", self.esc(last))
                    }
                };
                info.push(compact_tag("author", "", fields));
            }
        }
        let title = match doc.meta.get("title") {
            Some(MetaValue::MetaInlines(title)) => self.inlines(title)?,
            Some(other) => self.esc(&stringify_meta(other)),
            None => Doc::default(),
        };
        if !title.is_empty() {
            info.push(tagged("book-title", "", title.clone()));
        }
        if let Some(MetaValue::MetaBlocks(abstract_)) = doc.meta.get("abstract") {
            info.push(compact_tag("annotation", "", self.blocks(abstract_, 0)?));
        }
        for key in ["keywords", "date", "lang"] {
            if let Some(value) = doc.meta.get(key) {
                info.push(compact_tag(key, "", self.esc(&stringify_meta(value))));
            }
        }
        let description = tagged(
            "description",
            "",
            tagged("title-info", "", hcat(info))
                + literal("<document-info><program-used>pandoc</program-used></document-info>"),
        );
        let split = blocks
            .iter()
            .position(|b| matches!(b, Block::Div(a, _) if a.has_class("section")))
            .unwrap_or(blocks.len());
        let (intro, sections) = blocks.split_at(split);
        let intro = if intro.is_empty() {
            Doc::default()
        } else {
            tagged("section", "", self.blocks(intro, 0)?)
        };
        let body = tagged(
            "body",
            "",
            tagged("title", "", compact_tag("p", "", title)) + intro + self.blocks(sections, 0)?,
        );
        let notes = if self.notes.is_empty() {
            Doc::default()
        } else {
            tagged(
                "body",
                " name=\"notes\"",
                hcat(std::mem::take(&mut self.notes)),
            )
        };
        let binaries = hcat(self.binaries.drain(..).rev());
        let root = tagged(
            "FictionBook",
            " xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\" xmlns:l=\"http://www.w3.org/1999/xlink\"",
            description + body + notes + binaries,
        );
        let mut xml = (literal("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n") + root).render(None);
        xml.push('\n');
        Ok(xml)
    }
}
