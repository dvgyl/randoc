use super::*;

impl Html<'_> {
    pub(crate) fn revealjs(&self) -> bool {
        self.slides
            .is_some_and(|slides| slides.format == SlideFormat::RevealJs)
    }

    pub(super) fn push_incremental(&mut self, flag: bool) {
        self.incremental_flags
            .push(std::mem::replace(&mut self.incremental, flag));
    }

    pub(super) fn pop_incremental(&mut self) {
        if let Some(flag) = self.incremental_flags.pop() {
            self.incremental = flag;
        }
    }

    pub(super) fn revealjs_code_block(&self, attr: &Attr, code: &str, extra: &Attrs) -> Doc {
        let mut attr = attr.clone();
        if let Some(language) = attr.classes.first_mut() {
            *language = format!("language-{language}");
        }
        let mut doc = if extra.is_empty() {
            lit("<pre>")
        } else {
            // Attributes from an explicit wrapper can wrap.
            self.open("pre", extra)
        };
        doc += lit("<code");
        for (key, value) in self.attrs(&attr) {
            let value = escape_attribute(&value, false);
            doc += lit(&format!(" {key}=\"{value}\""));
        }
        doc += lit(">");
        doc += flush(lit(&escape_attribute(code, false)));
        doc += lit("</code></pre>");
        doc
    }

    pub(super) fn list_item_class(&self) -> Attrs {
        if self.incremental && self.revealjs() {
            vec![("class".into(), "fragment".into())]
        } else {
            Attrs::new()
        }
    }

    pub(super) fn slide_section<B: Borrow<Block>>(
        &mut self,
        slides: Slides,
        section: &LooseSection<'_, B>,
        attrs: &Attrs,
        number: Option<&SectionNumber>,
        depth: usize,
    ) -> Result<Doc> {
        let title = section.level < slides.level;
        let split = if title {
            section
                .rest
                .iter()
                .position(|block| self.section_level(block.borrow()).is_some())
                .unwrap_or(section.rest.len())
        } else {
            section.rest.len()
        };
        let (blocks, descendants) = section.rest.split_at(split);
        let name = if self.html5() { "section" } else { "div" };
        let untitled = slides::untitled(section.inlines);
        let mut body = if untitled {
            empty()
        } else {
            let level = if slides.format == SlideFormat::Slidy {
                1
            } else {
                section.level
            };
            self.header(
                level,
                section.heading,
                Attrs::new(),
                number,
                section.inlines,
                depth,
            )?
        };
        if self.block_level == 0 {
            self.flush_notes(&mut body, ReferenceLocation::EndOfBlock, "")?;
        }
        let children = self.section_content(
            blocks,
            section.rest_spans.and_then(|spans| spans.get(..split)),
            depth,
            "\n",
            &[],
        )?;
        if (untitled && !children.is_empty()) || (title && children.is_empty()) {
            body += lit("\n");
            body += children;
        } else {
            self.then_section_children(&mut body, children, blocks);
        }
        self.flush_leaf_section_notes(&mut body, section.rest)?;
        let mut slide = self.block_element(name, attrs, body);
        if title {
            let wrap = self.revealjs() && self.slide_group_depth == 0 && !descendants.is_empty();
            self.slide_group_depth += 1;
            let children = self.section_units(
                descendants,
                section.rest_spans.and_then(|spans| spans.get(split..)),
                depth,
                "\n",
                &[],
            );
            self.slide_group_depth -= 1;
            then_lines(&mut slide, children?);
            if wrap {
                slide = lit("<section>\n") + slide + lit("</section>");
            } else {
                slide += cr();
            }
        }
        self.toc_open.pop();
        Ok(slide)
    }

    pub(super) fn section_children<B: Borrow<Block>>(
        &mut self,
        blocks: &[B],
        spans: Option<&[usize]>,
        depth: usize,
        extra: &[(String, String)],
    ) -> Result<Doc> {
        if self.slides.is_some() {
            self.section_content(blocks, spans, depth, "\n", extra)
        } else {
            self.section_units(blocks, spans, depth, "\n", extra)
        }
    }

    pub(super) fn section_content<B: Borrow<Block>>(
        &mut self,
        blocks: &[B],
        spans: Option<&[usize]>,
        depth: usize,
        separator: &str,
        extra: &[(String, String)],
    ) -> Result<Doc> {
        let Some(slides) = self.slides else {
            return self.section_units(blocks, spans, depth, separator, extra);
        };
        let computed;
        let spans = match spans {
            Some(spans) => spans,
            None => {
                computed = self.section_spans(blocks);
                &computed
            }
        };
        // A heading owns its following blocks. Its section handles any pauses
        // there, even when there is no section element around those blocks.
        let mut index = 0;
        let mut pauses = std::iter::from_fn(|| {
            while let Some(block) = blocks.get(index).map(Borrow::borrow) {
                let start = index;
                index += if matches!(block, Block::Header(..)) {
                    spans.get(index).copied().unwrap_or(1).max(1)
                } else {
                    1
                };
                if slides::is_pause(block) {
                    return Some(start);
                }
            }
            None
        });
        let Some(first) = pauses.next() else {
            return self.section_units(blocks, Some(spans), depth, separator, extra);
        };
        self.pause_groups(
            blocks,
            (first, pauses),
            depth,
            (separator, slides.format.fragment_class()),
            |writer, group, span, depth| {
                writer.section_units(group, spans.get(span), depth, separator, extra)
            },
        )
    }

    pub(super) fn block_content(
        &mut self,
        blocks: &[Block],
        depth: usize,
        separator: &str,
        extra: &[(String, String)],
    ) -> Result<Doc> {
        let mut pauses = blocks
            .iter()
            .enumerate()
            .filter_map(|(index, block)| slides::is_pause(block).then_some(index));
        let Some((slides, first)) = self.slides.zip(pauses.next()) else {
            return self.lines(blocks, depth, separator, extra);
        };
        self.pause_groups(
            blocks,
            (first, pauses),
            depth,
            (separator, slides.format.fragment_class()),
            |writer, group, _, depth| writer.lines(group, depth, separator, extra),
        )
    }

    fn pause_groups<B: Borrow<Block>>(
        &mut self,
        blocks: &[B],
        (first, pauses): (usize, impl Iterator<Item = usize>),
        depth: usize,
        (separator, class): (&str, &str),
        mut render: impl FnMut(&mut Self, &[B], std::ops::Range<usize>, usize) -> Result<Doc>,
    ) -> Result<Doc> {
        let mut doc = render(self, &blocks[..first], 0..first, depth)?;
        let attrs = [("class".into(), class.into())];
        let mut start = first + 1;
        for end in pauses.chain(std::iter::once(blocks.len())) {
            if let Some(group) = blocks.get(start..end).filter(|group| !group.is_empty()) {
                let body = render(self, group, start..end, self.enter(depth)?)?;
                if !doc.is_empty() {
                    doc += lit(separator);
                }
                doc += if separator.is_empty() {
                    self.inline_element("div", &attrs, body)
                } else if body.is_empty() {
                    self.inline_element("div", &attrs, lit("\n"))
                } else {
                    self.block_element("div", &attrs, body)
                };
            }
            start = end + 1;
        }
        Ok(doc)
    }
}
