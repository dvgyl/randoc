use std::fmt::Write;

use randoc_ast::{Attr, Block, Inline, ListAttributes, MathType};
use randoc_core::{Extension, Result};

use super::Renderer;
use super::inlines::Run;
use super::numbering::Marker;
use super::styles::Kind;
use super::xml::{Xml, block_bookmark_end, bookmark_end};

#[derive(Clone, Copy, Default)]
pub(super) struct Flow<'a> {
    pub(super) style: Option<&'a str>,
    pub(super) custom: Option<&'a str>,
    pub(super) alignment: Option<&'a str>,
    pub(super) run: Run<'a>,
    pub(super) level: usize,
    pub(super) bidi: bool,
    pub(super) list: Option<i64>,
    pub(super) note: bool,
    /// Inside a table cell, Pandoc gives each `Plain` block the `Compact` style.
    pub(super) compact: bool,
    /// The bibliography attributes in effect for this paragraph.
    pub(super) bibliography: Option<&'a Attr>,
}

impl Renderer<'_> {
    pub(super) fn blocks(
        &mut self,
        out: &mut Xml,
        blocks: &[Block],
        mut flow: Flow<'_>,
        depth: usize,
    ) -> Result<()> {
        if !blocks.is_empty() {
            self.depth(depth)?;
        }
        for block in blocks {
            self.block(out, block, &mut flow, depth)?;
            self.budget.check()?;
        }
        Ok(())
    }

    fn block(
        &mut self,
        out: &mut Xml,
        block: &Block,
        flow: &mut Flow<'_>,
        depth: usize,
    ) -> Result<()> {
        match block {
            Block::Plain(inlines) if flow.compact => {
                let style = flow.style.replace("Compact");
                let result = self.paragraphs(out, inlines, flow, depth);
                flow.style = style;
                result
            }
            Block::Plain(inlines) | Block::Para(inlines) => {
                self.paragraphs(out, inlines, flow, depth)
            }
            Block::LineBlock(lines) => {
                self.open_paragraph(out, flow, None);
                for (index, line) in lines.iter().enumerate() {
                    if index > 0 {
                        flow.run.markup(out, "<w:br />");
                    }
                    self.inlines(out, line, flow.run, depth + 1)?;
                }
                out.raw("</w:p>");
                Ok(())
            }
            Block::CodeBlock(attr, code) => {
                let bookmark = self.block_bookmark(out, &attr.id);
                self.open_paragraph(out, flow, Some("SourceCode"));
                self.code(out, attr, code, flow.run)?;
                out.raw("</w:p>");
                block_bookmark_end(out, bookmark);
                self.first = true;
                Ok(())
            }
            Block::Header(level, attr, inlines) => self.heading(
                out,
                (*level).clamp(1, 10) as usize,
                attr,
                inlines,
                *flow,
                depth,
            ),
            Block::HorizontalRule => {
                out.element();
                out.raw(r#"<w:p><w:r><w:pict><v:rect style="width:0;height:1.5pt" o:hralign="center" o:hrstd="t" o:hr="t" /></w:pict></w:r></w:p>"#);
                self.first = true;
                Ok(())
            }
            Block::RawBlock(format, xml) => {
                if format.0 == "openxml" {
                    out.element();
                    out.raw(xml);
                }
                Ok(())
            }
            Block::Div(attr, blocks) => self.div(out, attr, blocks, *flow, depth),
            Block::BlockQuote(blocks) => {
                self.blocks(
                    out,
                    blocks,
                    Flow {
                        style: Some("BlockText"),
                        ..*flow
                    },
                    depth + 1,
                )?;
                self.first = true;
                Ok(())
            }
            Block::BulletList(items) => self.list(out, None, items, *flow, depth),
            Block::OrderedList(attributes, items) => {
                self.list(out, Some(*attributes), items, *flow, depth)
            }
            Block::DefinitionList(items) => {
                for (term, definitions) in items {
                    self.open_paragraph(out, flow, Some("DefinitionTerm"));
                    self.inlines(out, term, flow.run, depth + 1)?;
                    out.raw("</w:p>");
                    for definition in definitions {
                        self.blocks(
                            out,
                            definition,
                            Flow {
                                style: Some("Definition"),
                                ..*flow
                            },
                            depth + 1,
                        )?;
                    }
                }
                self.first = true;
                Ok(())
            }
            Block::Table(table) => self.table(out, table, *flow, depth),
            Block::Figure(attr, caption, blocks) => {
                self.figure(out, attr, caption, blocks, *flow, depth)
            }
        }
    }

    fn div(
        &mut self,
        out: &mut Xml,
        attr: &Attr,
        blocks: &[Block],
        flow: Flow<'_>,
        depth: usize,
    ) -> Result<()> {
        let style = attr
            .get("custom-style")
            .map(|name| self.styles.custom(Kind::Paragraph, name, &self.budget))
            .transpose()?;
        let bibliography = if attr.id == "refs" {
            Some(attr)
        } else {
            flow.bibliography
        };
        let mut body = out.fragment();
        // Bibliography divs get their bookmark IDs after their contents.
        let bookmark = if bibliography.is_some() {
            None
        } else {
            self.block_bookmark(out, &attr.id)
        };
        let first = self.first;
        self.first = true;
        self.blocks(
            if bibliography.is_some() {
                &mut body
            } else {
                out
            },
            blocks,
            Flow {
                bibliography,
                custom: style.as_deref().or(flow.custom),
                bidi: attr.get("dir").map_or(flow.bidi, |dir| dir == "rtl"),
                run: flow.run.with_attr(attr),
                ..flow
            },
            depth + 1,
        )?;
        self.first = first;
        let bookmark = if bibliography.is_some() {
            let bookmark = self.block_bookmark(out, &attr.id);
            out.append(body);
            bookmark
        } else {
            bookmark
        };
        block_bookmark_end(out, bookmark);
        Ok(())
    }

    fn heading(
        &mut self,
        out: &mut Xml,
        level: usize,
        attr: &Attr,
        inlines: &[Inline],
        mut flow: Flow<'_>,
        depth: usize,
    ) -> Result<()> {
        self.open_paragraph(out, &mut flow, Some(&format!("Heading{level}")));
        let bookmark = self.bookmark(out, &attr.id);
        if self.options.number_sections && !attr.has_class("unnumbered") {
            self.headings[level - 1] = self.headings[level - 1].saturating_add(1);
            self.headings[level..].fill(0);
            let mut number = String::new();
            for (index, value) in self.headings[..level].iter().enumerate() {
                if index > 0 {
                    number.push('.');
                }
                let offset = self
                    .options
                    .number_offset
                    .get(index)
                    .copied()
                    .unwrap_or_default();
                let _ = write!(number, "{}", value.saturating_add(offset));
            }
            flow.run
                .styled(self.styles.id(Kind::Character, "SectionNumber"))
                .text(out, &number);
            flow.run.markup(out, "<w:tab />");
        }
        self.inlines(out, inlines, flow.run.with_attr(attr), depth + 1)?;
        bookmark_end(out, bookmark);
        out.raw("</w:p>");
        self.first = true;
        Ok(())
    }

    fn paragraphs(
        &mut self,
        out: &mut Xml,
        inlines: &[Inline],
        flow: &mut Flow<'_>,
        depth: usize,
    ) -> Result<()> {
        if inlines.is_empty() {
            if self.options.extensions.contains(Extension::EmptyParagraphs)
                || flow.note
                || flow.list.is_some()
            {
                self.open_paragraph(out, flow, None);
                out.raw("</w:p>");
            }
            return Ok(());
        }
        let mut start = 0;
        for (index, inline) in inlines.iter().enumerate() {
            if matches!(inline, Inline::Math(MathType::DisplayMath, _)) {
                if let Some(text) = trim_breaks(&inlines[start..index]) {
                    self.first = true;
                    self.paragraph(out, text, flow, depth)?;
                }
                self.paragraph(out, std::slice::from_ref(inline), flow, depth)?;
                self.first = true;
                start = index + 1;
            }
        }
        if start == 0 {
            self.paragraph(out, inlines, flow, depth)?;
        } else {
            if let Some(text) = trim_breaks(&inlines[start..]) {
                self.paragraph(out, text, flow, depth)?;
            }
            self.first = true;
        }
        Ok(())
    }

    fn paragraph(
        &mut self,
        out: &mut Xml,
        inlines: &[Inline],
        flow: &mut Flow<'_>,
        depth: usize,
    ) -> Result<()> {
        self.open_paragraph(out, flow, None);
        self.inlines(out, inlines, flow.run, depth + 1)?;
        if matches!(inlines, [Inline::Math(MathType::InlineMath, _)]) {
            flow.run.text(out, "\u{200b}");
        }
        out.raw("</w:p>");
        Ok(())
    }

    pub(super) fn open_paragraph(
        &mut self,
        out: &mut Xml,
        flow: &mut Flow<'_>,
        explicit: Option<&str>,
    ) {
        let ordinary = if flow.bibliography.is_some() {
            "Bibliography"
        } else if self.first {
            "FirstParagraph"
        } else {
            "BodyText"
        };
        let style = flow.custom.unwrap_or_else(|| {
            self.styles
                .id(Kind::Paragraph, explicit.or(flow.style).unwrap_or(ordinary))
        });
        out.element();
        out.raw(r#"<w:p><w:pPr><w:pStyle w:val=""#);
        out.escaped(style);
        out.raw("\" />");
        let list = flow.list.take();
        if let Some(list) = list {
            let level = flow.level.saturating_sub(1);
            let _ = write!(
                out,
                "<w:numPr><w:ilvl w:val=\"{level}\" /><w:numId w:val=\"{list}\" /></w:numPr>"
            );
        }
        if flow.bidi {
            out.raw("<w:bidi />");
        }
        if let Some(attr) = flow.bibliography {
            bibliography_spacing(out, attr);
        }
        if flow
            .bibliography
            .is_some_and(|attr| attr.has_class("hanging-indent"))
        {
            out.raw(r#"<w:ind w:left="720" w:hanging="720" />"#);
        } else if flow.level > 0 && list.is_none() {
            let indent = 720 * flow.level;
            let _ = write!(out, "<w:ind w:left=\"{indent}\" w:firstLine=\"0\" />");
        }
        if let Some(align) = flow.alignment {
            let _ = write!(out, "<w:jc w:val=\"{align}\" />");
        }
        out.raw("</w:pPr>");
        if std::mem::take(&mut flow.note) {
            out.raw("<w:r>\n  <w:rPr>\n    <w:rStyle w:val=\"");
            out.escaped(self.styles.id(Kind::Character, "FootnoteReference"));
            out.raw("\" />\n  </w:rPr>\n  <w:footnoteRef />\n</w:r>");
            flow.run.text(out, " ");
        }
        self.first = false;
    }

    fn list(
        &mut self,
        out: &mut Xml,
        attributes: Option<ListAttributes>,
        items: &[Vec<Block>],
        flow: Flow<'_>,
        depth: usize,
    ) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let tasks = attributes.is_none() && items.iter().all(|item| task(item).is_some());
        let id = if tasks {
            0
        } else {
            self.numbering.list(
                attributes.map_or(Marker::Bullet, Marker::Ordered),
                &self.budget,
            )?
        };
        for item in items {
            let task = tasks.then(|| task(item)).flatten();
            let id = if let Some((checked, _)) = task {
                self.numbering.list(Marker::Task(checked), &self.budget)?
            } else {
                id
            };
            self.depth(depth + 1)?;
            self.first = true;
            let mut item_flow = Flow {
                level: flow.level + 1,
                list: Some(id),
                ..flow
            };
            for (index, block) in item.iter().enumerate() {
                if item_flow.list.is_some()
                    && matches!(
                        block,
                        Block::BulletList(_)
                            | Block::OrderedList(..)
                            | Block::Table(_)
                            | Block::Figure(..)
                            | Block::Div(..)
                            | Block::BlockQuote(_)
                    )
                {
                    self.open_paragraph(out, &mut item_flow, Some("Compact"));
                    out.raw("</w:p>");
                }
                let mut paragraph = item_flow;
                if matches!(block, Block::Plain(_)) {
                    paragraph.style = Some("Compact");
                }
                if index == 0
                    && let Some((_, inlines)) = task
                {
                    self.paragraphs(out, inlines, &mut paragraph, depth + 1)?;
                } else {
                    self.block(out, block, &mut paragraph, depth + 1)?;
                }
                item_flow.list = paragraph.list;
                item_flow.note = paragraph.note;
                self.budget.check()?;
            }
            if item_flow.list.is_some() {
                self.open_paragraph(out, &mut item_flow, Some("Compact"));
                out.raw("</w:p>");
            }
        }
        self.first = true;
        Ok(())
    }
}

fn bibliography_spacing(out: &mut Xml, attr: &Attr) {
    let spacing = |key, minimum| {
        attr.get(key)
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value > minimum)
            .map(|value| (value * 240.0).round_ties_even() as i64)
    };
    let line = spacing("line-spacing", 1.0);
    let after = spacing("entry-spacing", 0.0);
    if line.is_some() || after.is_some() {
        out.raw("<w:spacing");
        if let Some(line) = line {
            let _ = write!(out, " w:line=\"{line}\" w:lineRule=\"auto\"");
        }
        if let Some(after) = after {
            let _ = write!(out, " w:after=\"{after}\"");
        }
        out.raw(" />");
    }
}

fn trim_breaks(inlines: &[Inline]) -> Option<&[Inline]> {
    let space = |inline: &Inline| matches!(inline, Inline::Space | Inline::SoftBreak);
    let start = inlines.iter().position(|inline| !space(inline))?;
    let end = inlines.iter().rposition(|inline| !space(inline))?;
    Some(&inlines[start..=end])
}

fn task(blocks: &[Block]) -> Option<(bool, &[Inline])> {
    if let Some(Block::Para(inlines) | Block::Plain(inlines)) = blocks.first()
        && let [Inline::Str(symbol), Inline::Space, rest @ ..] = inlines.as_slice()
    {
        return match symbol.as_str() {
            "☒" => Some((true, rest)),
            "☐" => Some((false, rest)),
            _ => None,
        };
    }
    None
}
