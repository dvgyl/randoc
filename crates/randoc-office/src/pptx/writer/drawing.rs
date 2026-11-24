use std::fmt::Write;

use randoc_archive::opc::Relationship;
use randoc_archive::xml::escape_xml_text;

use crate::package::{PartXml, xml_limits};
use randoc_ast::{
    Alignment, Attr, Block, Caption, ColWidth, Format, Inline, ListAttributes, ListNumberDelim,
    ListNumberStyle, MathType, QuoteType, Table, Target,
};
use randoc_core::shared::{stringify, stringify_blocks};
use randoc_core::{Error, LogMessage, Result};

use super::super::reference::{
    Bounds, COLOR_OVERRIDE, Layout, NAMESPACES, Reference, TREE, relationship, relative_target,
};
use super::{DocumentState, Heading, Slide, attr, check_depth};

const TABLE_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/table";

pub(super) struct SlideOutput {
    pub xml: String,
    pub relationships: Vec<Relationship>,
    pub notes: Option<String>,
}

struct Builder<'a, 'b, 'c> {
    state: &'a mut DocumentState<'b, 'c>,
    reference: &'a mut Reference,
    relationships: Vec<Relationship>,
    slide: usize,
    layout: Layout,
    next_shape: usize,
    animations: Vec<(usize, usize)>,
    suppress_notes: bool,
}

pub(super) fn draw_slide<'d>(
    state: &mut DocumentState<'_, 'd>,
    reference: &mut Reference,
    slide: &Slide<'d>,
    index: usize,
    metadata: &[String; 4],
    headings: &[Heading<'d>],
) -> Result<SlideOutput> {
    let mut builder = Builder::new(state, reference, index, slide.layout);
    let layout = slide.layout;
    let mut shapes = String::new();
    let background = if let Some(url) = slide.attr.and_then(|attr| attr.get("background-image")) {
        let id = builder.image_rel(url)?;
        format!(
            r#"<p:bg><p:bgPr><a:blipFill><a:blip r:embed="{id}" /><a:stretch><a:fillRect /></a:stretch></a:blipFill><a:effectLst /></p:bgPr></p:bg>"#
        )
    } else {
        String::new()
    };
    if matches!(layout, Layout::Title) {
        if !metadata[0].is_empty() {
            let para = builder.plain_paragraph(&metadata[0], "ctr", 3200)?;
            shapes.push_str(&builder.placeholder_shape(Some("ctrTitle"), 0, None, &para, &[]));
        }
        let mut subtitles = String::new();
        for line in std::iter::once(metadata[1].as_str())
            .chain(metadata[2].split('\n'))
            .filter(|line| !line.is_empty())
        {
            subtitles.push_str(&builder.plain_paragraph(line, "ctr", 2000)?);
        }
        if !subtitles.is_empty() {
            shapes.push_str(&builder.placeholder_shape(Some("subTitle"), 1, None, &subtitles, &[]));
        }
        if !metadata[3].is_empty() {
            let para = builder.plain_paragraph(&metadata[3], "ctr", 1600)?;
            shapes.push_str(&builder.placeholder_shape(Some("dt"), 10, None, &para, &[]));
        }
    } else if !slide.title.is_empty() || slide.toc {
        let title = if slide.toc {
            builder.plain_paragraph("Table of Contents", "", 3200)?
        } else {
            builder.paragraph(Paragraph::new(slide.title, 1), "", 3200)?
        };
        shapes.push_str(&builder.placeholder_shape(Some("title"), 0, None, &title, &[]));
    }
    if slide.toc {
        builder.suppress_notes = true;
        let mut paras = String::new();
        for (_, level, text, number) in headings
            .iter()
            .filter(|(_, level, _, _)| *level <= builder.state.options.toc_depth)
        {
            let id = builder.slide_link(*number);
            let run = builder.runs(
                text,
                1,
                Style {
                    link: Some(id),
                    ..Style::default()
                },
            )?;
            let level = level.saturating_sub(1).clamp(0, 8);
            let _ = write!(paras, r#"<a:p><a:pPr lvl="{level}" />{run}</a:p>"#);
            if paras.len() as u64 > super::output_limit(builder.state.context) {
                return Err(Error::App("PPTX exceeds the output limit".into()));
            }
        }
        if !builder.state.endnotes.is_empty() && builder.state.options.toc_depth >= 1 {
            let link = builder.slide_link(builder.state.endnote_slide);
            let run = make_run(
                "Notes",
                Style {
                    link: Some(link),
                    ..Style::default()
                },
            );
            let _ = write!(paras, r#"<a:p><a:pPr lvl="0" />{run}</a:p>"#);
        }
        builder.suppress_notes = false;
        if !paras.is_empty() {
            let bounds = builder
                .reference
                .placeholder(layout, 1)
                .is_none()
                .then(|| builder.reference.bounds(layout, 1));
            shapes.push_str(&builder.placeholder_shape(Some(""), 1, bounds, &paras, &[]));
        }
    }
    let mut notes = Vec::new();
    let mut items = flatten(
        slide.body.iter().copied(),
        builder.state.options.incremental,
        builder.state.context.limits.max_nesting_depth,
        &mut notes,
    )?;
    if matches!(layout, Layout::Columns | Layout::Comparison)
        && let Some(Item::Columns(columns, depth, incremental)) = items.first()
    {
        let columns = columns.iter().filter_map(|block| match block {
            Block::Div(attr, body) if attr.has_class("column") => Some(body),
            _ => None,
        });
        for (index, body) in columns.take(2).enumerate() {
            let nested = flatten_from(
                body.iter(),
                *depth + 1,
                *incremental,
                builder.state.context.limits.max_nesting_depth,
                &mut notes,
            )?;
            builder.items(layout, index + 1, &nested, &mut shapes, &mut notes)?;
        }
        items.remove(0);
    }
    if matches!(layout, Layout::Caption) && items.len() >= 2 {
        builder.items(layout, 2, &items[..1], &mut shapes, &mut notes)?;
        builder.items(layout, 1, &items[1..], &mut shapes, &mut notes)?;
    } else {
        builder.items(layout, 1, &items, &mut shapes, &mut notes)?;
    }
    let notes = builder.speaker_notes(&notes)?;
    builder.finish(&shapes, &background, notes)
}

pub(super) fn draw_endnotes(
    state: &mut DocumentState<'_, '_>,
    reference: &mut Reference,
) -> Result<SlideOutput> {
    let notes = std::mem::take(&mut state.endnotes);
    let layout = Layout::Content;
    let index = state.endnote_slide;
    let mut builder = Builder::new(state, reference, index, layout);
    builder.suppress_notes = true;
    let title = builder.plain_paragraph("Notes", "", 3200)?;
    let mut shapes = builder.placeholder_shape(Some("title"), 0, None, &title, &[]);
    let mut paras = String::new();
    let depth = builder.state.context.limits.max_nesting_depth;
    for (index, note) in notes.iter().enumerate() {
        let mut speaker_notes = Vec::new();
        let items = flatten(note.iter(), false, depth, &mut speaker_notes)?;
        let mut paragraphs = items.into_iter().filter_map(|item| match item {
            Item::Paragraph(para) => Some(para),
            _ => None,
        });
        let prefix = format!("{}. ", index + 1);
        if let Some(first) = paragraphs.next() {
            paras.push_str(&builder.paragraph_with_prefix(first, "", 2000, &prefix)?);
            for para in paragraphs {
                paras.push_str(&builder.paragraph(para, "", 2000)?);
            }
        } else {
            paras.push_str(&builder.plain_paragraph(prefix.trim_end(), "", 2000)?);
        }
    }
    let bounds = builder
        .reference
        .placeholder(layout, 1)
        .is_none()
        .then(|| builder.reference.bounds(layout, 1));
    shapes.push_str(&builder.placeholder_shape(Some(""), 1, bounds, &paras, &[]));
    builder.finish(&shapes, "", None)
}

#[derive(Clone, Copy)]
enum Bullet {
    None,
    Unordered,
    Ordered(ListAttributes, bool),
}

#[derive(Clone, Copy)]
struct Paragraph<'a> {
    code: Option<(&'a Attr, &'a str)>,
    inlines: &'a [Inline],
    depth: usize,
    indent: usize,
    bullet: Bullet,
    animate: bool,
    quote: bool,
    heading: bool,
}

impl<'a> Paragraph<'a> {
    fn new(inlines: &'a [Inline], depth: usize) -> Self {
        Self {
            code: None,
            inlines,
            depth,
            indent: 0,
            bullet: Bullet::None,
            animate: false,
            quote: false,
            heading: false,
        }
    }
}

type Notes<'a> = Vec<(&'a [Block], usize)>;

enum Item<'a> {
    Paragraph(Paragraph<'a>),
    Table(&'a Table, usize),
    Picture(
        &'a Attr,
        &'a [Inline],
        &'a Target,
        Option<&'a Caption>,
        usize,
    ),
    Columns(&'a [Block], usize, bool),
    Raw(&'a str),
}

#[derive(Clone, Copy)]
struct BlockStyle {
    depth: usize,
    indent: usize,
    bullet: Bullet,
    incremental: bool,
    quote: bool,
}

impl BlockStyle {
    fn paragraph(self, inlines: &[Inline]) -> Paragraph<'_> {
        Paragraph {
            indent: self.indent.saturating_sub(1),
            bullet: self.bullet,
            animate: self.incremental && !matches!(self.bullet, Bullet::None),
            quote: self.quote,
            ..Paragraph::new(inlines, self.depth)
        }
    }

    fn children(self) -> Self {
        Self {
            depth: self.depth + 1,
            ..self
        }
    }
}

enum Work<'a> {
    Block(&'a Block, BlockStyle),
    Term(&'a [Inline], BlockStyle),
}

fn flatten<'a>(
    blocks: impl DoubleEndedIterator<Item = &'a Block>,
    incremental: bool,
    max_depth: usize,
    notes: &mut Notes<'a>,
) -> Result<Vec<Item<'a>>> {
    flatten_from(blocks, 1, incremental, max_depth, notes)
}

fn flatten_from<'a>(
    blocks: impl DoubleEndedIterator<Item = &'a Block>,
    depth: usize,
    incremental: bool,
    max_depth: usize,
    notes: &mut Notes<'a>,
) -> Result<Vec<Item<'a>>> {
    let base = BlockStyle {
        depth,
        indent: 0,
        bullet: Bullet::None,
        incremental,
        quote: false,
    };
    let mut work: Vec<_> = blocks.rev().map(|block| Work::Block(block, base)).collect();
    let mut output = Vec::new();
    while let Some(task) = work.pop() {
        let (block, style) = match task {
            Work::Term(inlines, style) => {
                output.push(Item::Paragraph(style.paragraph(inlines)));
                continue;
            }
            Work::Block(block, style) => (block, style),
        };
        check_depth(style.depth, max_depth)?;
        let child = style.children();
        match block {
            Block::Para(inlines) | Block::Plain(inlines) | Block::Header(_, _, inlines) => {
                if let [Inline::Image(attr, alt, target)] = inlines.as_slice() {
                    output.push(Item::Picture(attr, alt, target, None, style.depth));
                } else {
                    output.push(Item::Paragraph(Paragraph {
                        heading: matches!(block, Block::Header(..)),
                        ..style.paragraph(inlines)
                    }));
                }
            }
            Block::LineBlock(lines) => {
                output.extend(
                    lines
                        .iter()
                        .map(|line| Item::Paragraph(style.paragraph(line))),
                );
            }
            Block::CodeBlock(attr, text) => {
                output.push(Item::Paragraph(Paragraph {
                    code: Some((attr, text)),
                    ..style.paragraph(&[])
                }));
            }
            Block::Div(attr, children) if attr.has_class("notes") => {
                notes.push((children, child.depth))
            }
            Block::Div(attr, children) if attr.has_class("columns") && style.depth == 1 => {
                output.push(Item::Columns(children, child.depth, style.incremental));
            }
            Block::Div(attr, children) => {
                let incremental = if attr.has_class("incremental") {
                    true
                } else if attr.has_class("nonincremental") {
                    false
                } else {
                    child.incremental
                };
                let child = BlockStyle {
                    incremental,
                    ..child
                };
                work.extend(children.iter().rev().map(|block| Work::Block(block, child)));
            }
            Block::BlockQuote(children) => {
                let child = BlockStyle {
                    quote: true,
                    ..child
                };
                work.extend(children.iter().rev().map(|block| Work::Block(block, child)));
            }
            Block::Figure(_, caption, children) => {
                if let [Block::Para(inlines) | Block::Plain(inlines)] = children.as_slice()
                    && let [Inline::Image(attr, alt, target)] = inlines.as_slice()
                {
                    output.push(Item::Picture(
                        attr,
                        alt,
                        target,
                        Some(caption),
                        style.depth + 1,
                    ));
                } else {
                    work.extend(children.iter().rev().map(|block| Work::Block(block, child)));
                }
            }
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                for (index, blocks) in items.iter().enumerate().rev() {
                    for (paragraph, item) in blocks.iter().enumerate().rev() {
                        let bullet = if paragraph != 0 {
                            Bullet::None
                        } else if let Block::OrderedList(attrs, _) = block {
                            Bullet::Ordered(*attrs, index == 0)
                        } else {
                            Bullet::Unordered
                        };
                        work.push(Work::Block(
                            item,
                            BlockStyle {
                                indent: style.indent + 1,
                                bullet,
                                ..child
                            },
                        ));
                    }
                }
            }
            Block::DefinitionList(items) => {
                for (term, definitions) in items.iter().rev() {
                    work.extend(
                        definitions
                            .iter()
                            .rev()
                            .flat_map(|blocks| blocks.iter().rev())
                            .map(|block| Work::Block(block, child)),
                    );
                    work.push(Work::Term(term, child));
                }
            }
            Block::Table(table) => output.push(Item::Table(table, child.depth)),
            Block::RawBlock(Format(format), raw) if format == "openxml" => {
                output.push(Item::Raw(raw))
            }
            Block::RawBlock(..) | Block::HorizontalRule => {}
        }
    }
    Ok(output)
}

impl<'a, 'b, 'c> Builder<'a, 'b, 'c> {
    fn new(
        state: &'a mut DocumentState<'b, 'c>,
        reference: &'a mut Reference,
        slide: usize,
        layout: Layout,
    ) -> Self {
        let link = relative_target(&state.slide_paths[slide], reference.layout_path(layout));
        Self {
            state,
            reference,
            slide,
            layout,
            relationships: vec![relationship(1, "slideLayout", link, false)],
            next_shape: 2,
            animations: Vec::new(),
            suppress_notes: false,
        }
    }

    fn add_rel(&mut self, kind: &str, target: String, external: bool) -> usize {
        let id = self.relationships.len() + 1;
        self.relationships
            .push(relationship(id, kind, target, external));
        id
    }

    fn slide_link(&mut self, target: usize) -> (usize, bool) {
        let path = relative_target(
            &self.state.slide_paths[self.slide],
            &self.state.slide_paths[target],
        );
        (self.add_rel("slide", path, false), true)
    }

    fn image_rel(&mut self, source: &str) -> Result<String> {
        let media = self.state.media(self.reference, source)?;
        let path = relative_target(&self.state.slide_paths[self.slide], &media.path);
        Ok(format!("rId{}", self.add_rel("image", path, false)))
    }

    fn shape_id(&mut self) -> usize {
        while self
            .reference
            .layout_has_shape_id(self.layout, self.next_shape)
        {
            self.next_shape += 1;
        }
        let id = self.next_shape;
        self.next_shape += 1;
        id
    }

    fn finish(self, shapes: &str, background: &str, notes: Option<String>) -> Result<SlideOutput> {
        let timing = timing(&self.animations);
        let group = self.reference.slide_group(self.layout);
        let (namespaces, color) = if self.reference.imported {
            (self.reference.slide_namespaces(self.layout, shapes), "")
        } else {
            (NAMESPACES.to_owned(), COLOR_OVERRIDE)
        };
        let xml = format!(
            r#"<p:sld {namespaces}><p:cSld>{background}<p:spTree>{group}{shapes}</p:spTree></p:cSld>{color}{timing}</p:sld>"#
        );
        self.state
            .charge(xml.len() + notes.as_ref().map_or(0, String::len))?;
        Ok(SlideOutput {
            xml,
            relationships: self.relationships,
            notes,
        })
    }

    fn items(
        &mut self,
        layout: Layout,
        slot: usize,
        items: &[Item<'c>],
        shapes: &mut String,
        notes: &mut Notes<'c>,
    ) -> Result<()> {
        let count = items
            .iter()
            .enumerate()
            .filter(|(index, item)| {
                !matches!(item, Item::Paragraph(_))
                    || *index == 0
                    || !matches!(items[index - 1], Item::Paragraph(_))
            })
            .count()
            .max(1);
        let area = self.reference.bounds(layout, slot);
        let mut index = 0;
        let mut shape = 0;
        let mut parts = 0usize;
        while index < items.len() {
            let mut bounds = area;
            bounds.y += area.h * shape as i64 / count as i64;
            bounds.h = (area.h / count as i64).max(1);
            let initial = shapes.len();
            match &items[index] {
                Item::Paragraph(_) => {
                    let mut paras = String::new();
                    let mut animation = Vec::new();
                    let mut paragraph = 0;
                    while let Some(Item::Paragraph(para)) = items.get(index) {
                        if para.animate {
                            animation.push(paragraph);
                        }
                        paras.push_str(&self.paragraph(*para, "", 2000)?);
                        index += 1;
                        paragraph += 1;
                    }
                    shapes.push_str(
                        &self.placeholder_shape(
                            Some(""),
                            slot,
                            (count != 1 || self.reference.placeholder(layout, slot).is_none())
                                .then_some(bounds),
                            &paras,
                            &animation,
                        ),
                    );
                }
                Item::Table(table, depth) => {
                    shapes.push_str(&self.table(table, *depth, bounds, slot)?);
                    index += 1;
                }
                Item::Picture(attr, alt, target, caption, depth) => {
                    let caption = caption
                        .map(|caption| {
                            if let Some(short) = &caption.short {
                                stringify(short)
                            } else {
                                stringify_blocks(&caption.long)
                            }
                        })
                        .unwrap_or_else(|| stringify(alt));
                    check_depth(*depth, self.state.context.limits.max_nesting_depth)?;
                    if !caption.is_empty() {
                        bounds.h = (bounds.h * 4 / 5).max(1);
                    }
                    shapes.push_str(&self.picture(attr, target, &caption, bounds)?);
                    if !caption.is_empty() {
                        let para = self.plain_paragraph(&caption, "ctr", 1400)?;
                        bounds.y += bounds.h;
                        bounds.h = (bounds.h / 4).max(1);
                        shapes.push_str(&self.text_shape(None, 0, Some(bounds), &para, &[]));
                    }
                    index += 1;
                }
                Item::Columns(blocks, depth, incremental) => {
                    let nested = flatten_from(
                        blocks.iter(),
                        *depth + 1,
                        *incremental,
                        self.state.context.limits.max_nesting_depth,
                        notes,
                    )?;
                    self.items(layout, slot, &nested, shapes, notes)?;
                    index += 1;
                }
                Item::Raw(raw) => {
                    self.raw(raw)?;
                    shapes.push_str(raw);
                    index += 1;
                }
            }
            parts = parts.saturating_add(shapes.len() - initial);
            if parts as u64 > super::output_limit(self.state.context) {
                return Err(Error::App("PPTX exceeds the output limit".into()));
            }
            shape += 1;
        }
        Ok(())
    }

    fn code_runs(&mut self, attr: &Attr, code: &str) -> Result<String> {
        let base = Style {
            code: true,
            ..Style::default()
        };
        let options = self.state.options;
        if let Some(name) = super::highlight::style_name(options)
            && !attr.classes.is_empty()
        {
            if self.state.highlight.is_none() {
                self.state.highlight = Some(super::highlight::Highlight::new(options, name)?);
            }
            if let Some(highlight) = &self.state.highlight {
                let depth = self.state.context.limits.max_nesting_depth;
                match highlight
                    .registry
                    .highlight_classes(&attr.classes, code, depth)
                {
                    Some(Ok(code)) => {
                        let mut xml = String::new();
                        for (index, line) in code.lines.into_iter().enumerate() {
                            if index != 0 {
                                xml.push_str("<a:br />");
                            }
                            for (kind, text) in line {
                                let theme = &highlight.theme;
                                let mut style = Style {
                                    color: theme.default_color,
                                    ..base
                                };
                                if let Some(token) = theme.token_styles.get(&kind) {
                                    style.color = token.color.or(style.color);
                                    style.bold = token.bold;
                                    style.italic = token.italic;
                                    style.underline = token.underline;
                                }
                                xml.push_str(&make_run(&text, style));
                            }
                        }
                        return Ok(xml);
                    }
                    Some(Err(error)) => self
                        .state
                        .context
                        .report(LogMessage::CouldNotHighlight(error)),
                    None => {}
                }
            }
        }
        Ok(make_run(code, base))
    }

    fn paragraph(&mut self, para: Paragraph<'c>, alignment: &str, size: u32) -> Result<String> {
        self.paragraph_with_prefix(para, alignment, size, "")
    }

    fn paragraph_with_prefix(
        &mut self,
        para: Paragraph<'c>,
        alignment: &str,
        size: u32,
        prefix: &str,
    ) -> Result<String> {
        let imported = self.reference.imported && size != 1200;
        let style = Style {
            size: if imported {
                if para.quote { 2000 } else { 0 }
            } else {
                size
            },
            bold: imported && para.heading,
            compact: imported,
            ..Style::default()
        };
        let body = if let Some((attr, text)) = para.code {
            let code = self.code_runs(attr, text)?;
            if prefix.is_empty() {
                code
            } else {
                make_run(prefix, style) + &code
            }
        } else {
            self.runs_with_prefix(para.inlines, para.depth, style, prefix)?
        };
        let alignment = if alignment.is_empty() {
            String::new()
        } else {
            format!(r#" algn="{alignment}""#)
        };
        let level = para.indent.min(8);
        let space = if imported && para.heading {
            r#"<a:spcBef><a:spcPts val="3000" /></a:spcBef>"#
        } else {
            ""
        };
        let margin = if imported && para.quote { 1_270_000 } else { 0 };
        let properties = match para.bullet {
            Bullet::None if para.code.is_some() => {
                format!(r#"<a:pPr lvl="{level}" indent="0"{alignment}><a:buNone /></a:pPr>"#)
            }
            Bullet::None => format!(
                r#"<a:pPr lvl="{level}" indent="0" marL="{margin}"{alignment}>{space}<a:buNone /></a:pPr>"#
            ),
            Bullet::Unordered => format!(r#"<a:pPr lvl="{level}"{alignment} />"#),
            Bullet::Ordered(list, start) => {
                let kind = number_style(list);
                let number = list.start.clamp(1, 32767);
                let start = if (imported || start) && number != 1 {
                    format!(r#" startAt="{number}""#)
                } else {
                    String::new()
                };
                if imported {
                    format!(
                        r#"<a:pPr lvl="{level}"{alignment}><a:buAutoNum{start} type="{kind}" /></a:pPr>"#
                    )
                } else {
                    let margin = 342_900 * (level + 1);
                    format!(
                        r#"<a:pPr lvl="{level}" indent="-342900" marL="{margin}"{alignment}><a:buAutoNum type="{kind}"{start} /></a:pPr>"#
                    )
                }
            }
        };
        Ok(format!("<a:p>{properties}{body}</a:p>"))
    }

    fn plain_paragraph(&mut self, text: &str, alignment: &str, size: u32) -> Result<String> {
        let alignment = if alignment.is_empty() {
            String::new()
        } else {
            format!(r#" algn="{alignment}""#)
        };
        let run = make_run(
            text,
            Style {
                size: if self.reference.imported && size != 1200 {
                    0
                } else {
                    size
                },
                compact: self.reference.imported && size != 1200,
                ..Style::default()
            },
        );
        Ok(format!(
            r#"<a:p><a:pPr{alignment}><a:buNone /></a:pPr>{run}</a:p>"#
        ))
    }

    fn placeholder_shape(
        &mut self,
        kind: Option<&str>,
        slot: usize,
        bounds: Option<Bounds>,
        paras: &str,
        animate: &[usize],
    ) -> String {
        if let Some(placeholder) = self.reference.placeholder(self.layout, slot) {
            // Pandoc 3.13 reuses the layout ID for each text shape in the slot.
            let id = placeholder.id;
            let xml = placeholder.draw(paras, bounds, kind == Some(""));
            self.animations
                .extend(animate.iter().map(|paragraph| (id, *paragraph)));
            math_shape(xml, paras.contains("<a14:m>"))
        } else {
            self.text_shape(kind, slot, bounds, paras, animate)
        }
    }

    fn text_shape(
        &mut self,
        kind: Option<&str>,
        slot: usize,
        bounds: Option<Bounds>,
        paras: &str,
        animate: &[usize],
    ) -> String {
        let id = self.shape_id();
        self.animations
            .extend(animate.iter().map(|paragraph| (id, *paragraph)));
        let placeholder = match (kind, slot) {
            (Some(kind), 0) => format!(r#"<p:ph type="{kind}" />"#),
            (Some(""), _) => format!(r#"<p:ph idx="{slot}" />"#),
            (Some(kind), _) => format!(r#"<p:ph type="{kind}" idx="{slot}" />"#),
            (None, _) => String::new(),
        };
        let bounds = bounds.map_or_else(String::new, |bounds| bounds.transform("a"));
        let box_kind = if kind.is_none() { r#" txBox="1""# } else { "" };
        let shape = if kind.is_none() {
            r#"<a:prstGeom prst="rect"><a:avLst /></a:prstGeom><a:noFill />"#
        } else {
            ""
        };
        let xml = format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Text {id}" /><p:cNvSpPr{box_kind} /><p:nvPr>{placeholder}</p:nvPr></p:nvSpPr><p:spPr>{bounds}{shape}</p:spPr><p:txBody><a:bodyPr /><a:lstStyle />{paras}</p:txBody></p:sp>"#
        );
        math_shape(xml, paras.contains("<a14:m>"))
    }

    fn table(
        &mut self,
        table: &'c Table,
        depth: usize,
        area: Bounds,
        slot: usize,
    ) -> Result<String> {
        check_depth(depth, self.state.context.limits.max_nesting_depth)?;
        let rows = table
            .head
            .rows
            .iter()
            .chain(
                table
                    .bodies
                    .iter()
                    .flat_map(|body| body.head.iter().chain(&body.body)),
            )
            .chain(&table.foot.rows);
        let height = rows.clone().count();
        let width = rows
            .clone()
            .map(|row| {
                row.cells.iter().try_fold(0usize, |total, cell| {
                    total.checked_add(usize::try_from(cell.col_span.max(1)).ok()?)
                })
            })
            .try_fold(table.col_specs.len(), |max, value| {
                value.map(|value| max.max(value))
            })
            .ok_or_else(|| Error::App("Table exceeds the cell limit".into()))?;
        if width == 0 || height == 0 {
            return Ok(String::new());
        }
        let real_cells: usize = rows.clone().map(|row| row.cells.len()).sum();
        let area_cells = height.checked_mul(width).ok_or_else(table_limit)?;
        let remaining = self.state.context.limits.max_table_cells - self.state.table_cells;
        if area_cells.saturating_sub(real_cells) > remaining {
            return Err(table_limit());
        }
        let has_caption = !table.caption.long.is_empty() || table.caption.short.is_some();
        let imported = self.reference.imported;
        const EMU_PER_POINT: i64 = 12_700;
        const CAPTION_HEIGHT: i64 = 40 * EMU_PER_POINT;
        let area = if imported {
            let whole_points = |value| (value / EMU_PER_POINT) * EMU_PER_POINT;
            Bounds {
                x: whole_points(area.x),
                y: whole_points(area.y),
                w: whole_points(area.w).max(1),
                h: whole_points(area.h).max(1),
            }
        } else {
            area
        };
        let frame = if imported && has_caption {
            Bounds {
                h: (area.h - CAPTION_HEIGHT).max(1),
                ..area
            }
        } else if has_caption {
            Bounds {
                h: (area.h * 4 / 5).max(1),
                ..area
            }
        } else {
            area
        };
        let id = self.shape_id();
        let position = frame.transform("p");
        let first = usize::from(!table.head.rows.is_empty());
        let frame_properties = if imported {
            // Pandoc 3.13 uses ID 6 for each table frame.
            r#"<p:cNvPr id="6" name="Content Placeholder 5" /><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1" /></p:cNvGraphicFramePr>"#.to_owned()
        } else {
            format!(r#"<p:cNvPr id="{id}" name="Table {id}" /><p:cNvGraphicFramePr />"#)
        };
        let table_properties = if imported {
            let mut style = String::new();
            escape_content(&self.reference.table_style, &mut style);
            format!(
                r#"<a:tblPr firstRow="{first}" bandRow="1"><a:tableStyleId>{style}</a:tableStyleId></a:tblPr>"#
            )
        } else {
            format!(r#"<a:tblPr firstRow="{first}" bandRow="1" />"#)
        };
        let mut xml = format!(
            r#"<p:graphicFrame><p:nvGraphicFramePr>{frame_properties}<p:nvPr><p:ph idx="{slot}" /></p:nvPr></p:nvGraphicFramePr>{position}<a:graphic><a:graphicData uri="{TABLE_URI}"><a:tbl>{table_properties}<a:tblGrid>"#
        );
        let equal = (area.w / width as i64).max(1);
        let custom = table.col_specs.len() == width && table.col_specs.iter().all(|spec| matches!(spec.width, ColWidth::ColWidth(frac) if frac.is_finite() && frac > 0.0 && frac <= 1.0));
        for col in 0..width {
            let width = match table.col_specs.get(col).map(|spec| spec.width) {
                Some(ColWidth::ColWidth(fraction)) if custom => {
                    ((area.w as f64 * fraction) as i64).max(1)
                }
                _ => equal,
            };
            let _ = write!(xml, r#"<a:gridCol w="{width}" />"#);
        }
        xml.push_str("</a:tblGrid>");
        let row_height = if imported {
            0
        } else {
            (frame.h / height as i64).clamp(1, 381_000)
        };
        let mut covering = vec![0usize; width];
        let mut has_math = false;
        for (row_index, row) in rows.enumerate() {
            let _ = write!(xml, r#"<a:tr h="{row_height}">"#);
            let mut col = 0;
            for cell in &row.cells {
                while col < width && covering[col] > row_index {
                    self.padding(&mut xml, 1)?;
                    col += 1;
                }
                if col == width {
                    break;
                }
                let span = (cell.col_span.max(1) as usize).min(width - col);
                let rowspan = (cell.row_span.max(1) as usize).min(height - row_index);
                for until in &mut covering[col..col + span] {
                    *until = (*until).max(row_index + rowspan);
                }
                let alignment = match cell.align {
                    Alignment::AlignDefault => table
                        .col_specs
                        .get(col)
                        .map_or(Alignment::AlignDefault, |column| column.align),
                    align => align,
                };
                let alignment = match alignment {
                    Alignment::AlignLeft => "l",
                    Alignment::AlignRight => "r",
                    Alignment::AlignCenter => "ctr",
                    Alignment::AlignDefault => "",
                };
                xml.push_str("<a:tc><a:txBody><a:bodyPr /><a:lstStyle />");
                let mut notes = Vec::new();
                let paragraphs = flatten_from(
                    cell.content.iter(),
                    depth,
                    false,
                    self.state.context.limits.max_nesting_depth,
                    &mut notes,
                )?;
                let mut written = false;
                for item in paragraphs {
                    if let Item::Paragraph(para) = item {
                        let paragraph = self.paragraph(para, alignment, 1600)?;
                        has_math |= paragraph.contains("<a14:m>");
                        xml.push_str(&paragraph);
                        written = true;
                    }
                }
                if !written {
                    xml.push_str("<a:p />");
                }
                xml.push_str("</a:txBody>");
                if !imported || row_index < table.head.rows.len() {
                    xml.push_str("<a:tcPr />");
                }
                xml.push_str("</a:tc>");
                self.padding(&mut xml, span - 1)?;
                col += span;
            }
            self.padding(&mut xml, width - col)?;
            xml.push_str("</a:tr>");
            if xml.len() as u64 > super::output_limit(self.state.context) {
                return Err(Error::App("PPTX exceeds the output limit".into()));
            }
        }
        xml.push_str("</a:tbl></a:graphicData></a:graphic></p:graphicFrame>");
        let mut xml = math_shape(xml, has_math);
        if has_caption {
            let caption = table.caption.short.as_ref().map_or_else(
                || stringify_blocks(&table.caption.long),
                |text| stringify(text),
            );
            let bounds = Bounds {
                y: area.y + frame.h,
                h: (area.h - frame.h).max(1),
                ..area
            };
            if imported {
                // Pandoc 3.13 uses the group ID 1 for each table caption.
                let position = bounds.transform("a");
                let run = make_run(
                    &caption,
                    Style {
                        compact: true,
                        ..Style::default()
                    },
                );
                let _ = write!(
                    xml,
                    r#"<p:sp><p:nvSpPr><p:cNvPr id="1" name="TextBox 3" /><p:cNvSpPr txBox="1" /><p:nvPr><p:ph idx="{slot}" /></p:nvPr></p:nvSpPr><p:spPr>{position}<a:prstGeom prst="rect"><a:avLst /></a:prstGeom><a:noFill /></p:spPr><p:txBody><a:bodyPr /><a:lstStyle /><a:p><a:pPr lvl="0" indent="0" marL="0" algn="ctr"><a:buNone /></a:pPr>{run}</a:p></p:txBody></p:sp>"#
                );
            } else {
                let para = self.plain_paragraph(&caption, "ctr", 1400)?;
                xml.push_str(&self.text_shape(None, 0, Some(bounds), &para, &[]));
            }
        }
        Ok(xml)
    }

    fn padding(&mut self, xml: &mut String, count: usize) -> Result<()> {
        self.state.table_cells = self
            .state
            .table_cells
            .checked_add(count)
            .filter(|total| *total <= self.state.context.limits.max_table_cells)
            .ok_or_else(table_limit)?;
        for _ in 0..count {
            xml.push_str(
                "<a:tc><a:txBody><a:bodyPr /><a:lstStyle /><a:p /></a:txBody><a:tcPr /></a:tc>",
            );
        }
        Ok(())
    }

    fn picture(
        &mut self,
        attributes: &Attr,
        target: &Target,
        caption: &str,
        mut area: Bounds,
    ) -> Result<String> {
        let media = self.state.media(self.reference, &target.url)?;
        let path = relative_target(&self.state.slide_paths[self.slide], &media.path);
        let rel = self.add_rel("image", path, false);
        let id = self.shape_id();
        let name = if target.title.is_empty() {
            format!("Picture {id}")
        } else {
            attr(&target.title)
        };
        let caption = attr(caption);
        let ratio = f64::from(media.size.0.max(1)) / f64::from(media.size.1.max(1));
        let max_width =
            length(attributes.get("width"), area.w, self.state.options.dpi).unwrap_or(area.w);
        let max_height =
            length(attributes.get("height"), area.h, self.state.options.dpi).unwrap_or(area.h);
        let w = max_width.min((max_height as f64 * ratio) as i64).max(1);
        let h = max_height.min((w as f64 / ratio) as i64).max(1);
        area.x += (area.w - w) / 2;
        area.y += (area.h - h) / 2;
        area.w = w;
        area.h = h;
        let position = area.transform("a");
        Ok(format!(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="{name}" descr="{caption}" /><p:cNvPicPr><a:picLocks noChangeAspect="1" /></p:cNvPicPr><p:nvPr /></p:nvPicPr><p:blipFill><a:blip r:embed="rId{rel}" /><a:stretch><a:fillRect /></a:stretch></p:blipFill><p:spPr>{position}<a:prstGeom prst="rect"><a:avLst /></a:prstGeom></p:spPr></p:pic>"#
        ))
    }

    fn speaker_notes(&mut self, notes: &Notes<'c>) -> Result<Option<String>> {
        if notes.is_empty() {
            return Ok(None);
        }
        self.suppress_notes = true;
        let mut paragraphs = String::new();
        let mut pending: Vec<_> = notes.iter().rev().copied().collect();
        let max_depth = self.state.context.limits.max_nesting_depth;
        let relationship_count = self.relationships.len();
        while let Some((blocks, depth)) = pending.pop() {
            let mut children = Vec::new();
            let items = flatten_from(blocks.iter(), depth, false, max_depth, &mut children)?;
            for item in items {
                if let Item::Paragraph(para) = item {
                    paragraphs.push_str(&self.paragraph(para, "", 1200)?);
                }
            }
            pending.extend(children.into_iter().rev());
        }
        self.relationships.truncate(relationship_count);
        let body = self.text_shape(Some("body"), 1, None, &paragraphs, &[]);
        let number = self.plain_paragraph(&(self.slide + 1).to_string(), "", 1200)?;
        let number = self.text_shape(Some("sldNum"), 2, None, &number, &[]);
        Ok(Some(format!(
            r#"<p:notes {NAMESPACES}><p:cSld><p:spTree>{TREE}{body}{number}</p:spTree></p:cSld>{COLOR_OVERRIDE}</p:notes>"#
        )))
    }

    fn raw(&self, raw: &str) -> Result<()> {
        let limits = xml_limits(&self.state.context.limits);
        if raw.len() as u64 > super::output_limit(self.state.context) {
            return Err(Error::App("PPTX exceeds the output limit".into()));
        }
        let input = format!("<root {NAMESPACES}>{raw}</root>");
        PartXml::parse(&input, &limits)
            .map(|_| ())
            .map_err(|error| Error::App(format!("Invalid Open XML: {}", error.format_pandoc())))
    }
}

fn table_limit() -> Error {
    Error::App("Table exceeds the cell limit".into())
}

fn math_shape(xml: String, has_math: bool) -> String {
    if has_math {
        format!(
            r#"<mc:AlternateContent><mc:Choice Requires="a14">{xml}</mc:Choice></mc:AlternateContent>"#
        )
    } else {
        xml
    }
}

fn length(value: Option<&str>, area: i64, dpi: i64) -> Option<i64> {
    let value = value?;
    let index = value
        .find(|ch: char| ch != '.' && !ch.is_ascii_digit())
        .unwrap_or(value.len());
    let (value, unit) = value.split_at(index);
    let value = value.parse::<f64>().ok()?;
    let scale = match unit {
        "%" => area as f64 / 100.0,
        "in" => 914_400.0,
        "cm" => 360_000.0,
        "mm" => 36_000.0,
        "pt" => 12_700.0,
        "" | "px" => 914_400.0 / dpi.max(1) as f64,
        _ => return None,
    };
    Some(((value * scale) as i64).clamp(1, area))
}

fn number_style(list: ListAttributes) -> String {
    let style = match list.style {
        ListNumberStyle::LowerRoman => "romanLc",
        ListNumberStyle::UpperRoman => "romanUc",
        ListNumberStyle::LowerAlpha => "alphaLc",
        ListNumberStyle::UpperAlpha => "alphaUc",
        _ => "arabic",
    };
    let delim = match list.delim {
        ListNumberDelim::OneParen => "ParenR",
        ListNumberDelim::TwoParens => "ParenBoth",
        _ => "Period",
    };
    format!("{style}{delim}")
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Style {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    caps: bool,
    code: bool,
    color: Option<randoc_highlight::Color>,
    baseline: i32,
    size: u32,
    link: Option<(usize, bool)>,
    compact: bool,
}

enum Run<'a> {
    Inline(&'a Inline, usize, Style),
    Text(&'a str, Style),
}

impl<'c> Builder<'_, '_, 'c> {
    fn runs(&mut self, inlines: &'c [Inline], depth: usize, base: Style) -> Result<String> {
        self.runs_with_prefix(inlines, depth, base, "")
    }

    fn runs_with_prefix(
        &mut self,
        inlines: &'c [Inline],
        depth: usize,
        base: Style,
        prefix: &str,
    ) -> Result<String> {
        let mut work: Vec<_> = inlines
            .iter()
            .rev()
            .map(|inline| Run::Inline(inline, depth, base))
            .collect();
        let mut xml = String::new();
        let mut run = prefix.to_owned();
        let mut previous = base;
        let limit = super::output_limit(self.state.context);
        let max_depth = self.state.context.limits.max_nesting_depth;
        while let Some(item) = work.pop() {
            if xml.len().saturating_add(run.len()) as u64 > limit {
                return Err(Error::App("PPTX exceeds the output limit".into()));
            }
            let (text, style) = match item {
                Run::Text(text, style) => (text, style),
                Run::Inline(inline, level, mut style) => {
                    check_depth(level, max_depth)?;
                    let children = match inline {
                        Inline::Str(text) => {
                            push_text(&mut xml, &mut run, &mut previous, text, style);
                            continue;
                        }
                        Inline::Code(_, text) => {
                            style.code = true;
                            push_text(&mut xml, &mut run, &mut previous, text, style);
                            continue;
                        }
                        Inline::Space | Inline::SoftBreak => {
                            push_text(&mut xml, &mut run, &mut previous, " ", style);
                            continue;
                        }
                        Inline::LineBreak => {
                            flush_run(&mut xml, &mut run, previous);
                            xml.push_str("<a:br />");
                            continue;
                        }
                        Inline::Emph(children) => {
                            style.italic = true;
                            children
                        }
                        Inline::Strong(children) => {
                            style.bold = true;
                            children
                        }
                        Inline::Underline(children) => {
                            style.underline = true;
                            children
                        }
                        Inline::Strikeout(children) => {
                            style.strike = true;
                            children
                        }
                        Inline::SmallCaps(children) => {
                            style.caps = true;
                            children
                        }
                        Inline::Superscript(children) => {
                            style.baseline = 30000;
                            children
                        }
                        Inline::Subscript(children) => {
                            style.baseline = -25000;
                            children
                        }
                        Inline::Image(_, children, _)
                        | Inline::Span(_, children)
                        | Inline::Cite(_, children) => children,
                        Inline::Link(_, children, target) => {
                            if !self.suppress_notes {
                                style.link = if let Some(name) = target.url.strip_prefix('#') {
                                    self.state
                                        .heading_slides
                                        .get(name)
                                        .copied()
                                        .map(|slide| self.slide_link(slide))
                                } else {
                                    Some((
                                        self.add_rel("hyperlink", target.url.clone(), true),
                                        false,
                                    ))
                                };
                            }
                            children
                        }
                        Inline::Quoted(kind, children) => {
                            let (left, right) = match kind {
                                QuoteType::SingleQuote => ("‘", "’"),
                                QuoteType::DoubleQuote => ("“", "”"),
                            };
                            work.push(Run::Text(right, style));
                            work.extend(
                                children
                                    .iter()
                                    .rev()
                                    .map(|child| Run::Inline(child, level + 1, style)),
                            );
                            push_text(&mut xml, &mut run, &mut previous, left, style);
                            continue;
                        }
                        Inline::Math(kind, text) => {
                            match randoc_math::to_omml(*kind, text, max_depth) {
                                Ok(math) => {
                                    flush_run(&mut xml, &mut run, previous);
                                    xml.push_str("<a14:m>");
                                    if *kind == MathType::DisplayMath {
                                        xml.push_str(r#"<m:oMathPara><m:oMathParaPr><m:jc m:val="center" /></m:oMathParaPr>"#);
                                    }
                                    xml.push_str(&math);
                                    if *kind == MathType::DisplayMath {
                                        xml.push_str("</m:oMathPara>");
                                    }
                                    xml.push_str("</a14:m>");
                                }
                                Err(error) => {
                                    self.state
                                        .context
                                        .report(LogMessage::CouldNotConvertTexMath(
                                            text.clone(),
                                            error.detail(text),
                                        ));
                                    let delimiter = if *kind == MathType::DisplayMath {
                                        "$$"
                                    } else {
                                        "$"
                                    };
                                    push_text(
                                        &mut xml,
                                        &mut run,
                                        &mut previous,
                                        &format!("{delimiter}{text}{delimiter}"),
                                        style,
                                    );
                                }
                            }
                            continue;
                        }
                        Inline::Note(blocks) => {
                            if !self.suppress_notes {
                                self.state.endnotes.push(blocks);
                                let number = self.state.endnotes.len().to_string();
                                style.baseline = 30000;
                                style.link = Some(self.slide_link(self.state.endnote_slide));
                                push_text(&mut xml, &mut run, &mut previous, &number, style);
                            }
                            continue;
                        }
                        Inline::RawInline(Format(format), raw) => {
                            if format == "openxml" {
                                self.raw(raw)?;
                                flush_run(&mut xml, &mut run, previous);
                                xml.push_str(raw);
                            }
                            continue;
                        }
                    };
                    work.extend(
                        children
                            .iter()
                            .rev()
                            .map(|child| Run::Inline(child, level + 1, style)),
                    );
                    continue;
                }
            };
            push_text(&mut xml, &mut run, &mut previous, text, style);
        }
        flush_run(&mut xml, &mut run, previous);
        if xml.len() as u64 > limit {
            return Err(Error::App("PPTX exceeds the output limit".into()));
        }
        Ok(xml)
    }
}

fn push_text(xml: &mut String, run: &mut String, previous: &mut Style, text: &str, style: Style) {
    if *previous != style {
        flush_run(xml, run, *previous);
        *previous = style;
    }
    run.push_str(text);
}

fn flush_run(xml: &mut String, run: &mut String, style: Style) {
    if !run.is_empty() {
        xml.push_str(&make_run(run, style));
        run.clear();
    }
}

fn make_run(text: &str, style: Style) -> String {
    let mut xml = String::from("<a:r><a:rPr");
    for (enabled, attribute) in [
        (style.bold, r#" b="1""#),
        (style.italic, r#" i="1""#),
        (style.underline, r#" u="sng""#),
        (style.strike, r#" strike="sngStrike""#),
        (style.caps, r#" cap="small""#),
    ] {
        if enabled {
            xml.push_str(attribute);
        }
    }
    if style.baseline != 0 {
        let _ = write!(xml, r#" baseline="{}""#, style.baseline);
    }
    if style.size != 0 {
        let _ = write!(xml, r#" sz="{}""#, style.size);
    }
    if style.compact && style.color.is_none() && !style.code && style.link.is_none() {
        xml.push_str(" /><a:t>");
        escape_content(text, &mut xml);
        xml.push_str("</a:t></a:r>");
        return xml;
    }
    xml.push('>');
    if let Some(color) = style.color {
        let randoc_highlight::Color { r, g, b } = color;
        let _ = write!(
            xml,
            r#"<a:solidFill><a:srgbClr val="{r:02X}{g:02X}{b:02X}" /></a:solidFill>"#
        );
    }
    if style.code {
        xml.push_str(r#"<a:latin typeface="Courier" />"#);
    }
    if let Some((id, slide)) = style.link {
        let action = if slide {
            r#" action="ppaction://hlinksldjump""#
        } else {
            ""
        };
        let _ = write!(xml, r#"<a:hlinkClick r:id="rId{id}"{action} />"#);
    }
    xml.push_str("</a:rPr><a:t>");
    escape_content(text, &mut xml);
    xml.push_str("</a:t></a:r>");
    xml
}

pub(super) fn escape_content(text: &str, xml: &mut String) {
    if text.chars().any(invalid_xml_char) {
        let clean: String = text.chars().filter(|ch| !invalid_xml_char(*ch)).collect();
        escape_xml_text(&clean, xml);
    } else {
        escape_xml_text(text, xml);
    }
}

fn invalid_xml_char(ch: char) -> bool {
    matches!(ch, '\0'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}')
}

fn timing(animations: &[(usize, usize)]) -> String {
    if animations.is_empty() {
        return String::new();
    }
    let mut xml = String::from(
        r#"<p:timing><p:tnLst><p:par><p:cTn id="1" dur="indefinite" restart="never" nodeType="tmRoot"><p:childTnLst><p:seq concurrent="1" nextAc="seek"><p:cTn id="2" dur="indefinite" nodeType="mainSeq"><p:childTnLst>"#,
    );
    for (index, (shape, paragraph)) in animations.iter().copied().enumerate() {
        let event = 3 + index * 4;
        let click = event + 1;
        let effect = event + 2;
        let behavior = event + 3;
        let _ = write!(
            xml,
            concat!(
                r#"<p:par><p:cTn id="{event}" fill="hold"><p:stCondLst><p:cond delay="indefinite" /></p:stCondLst><p:childTnLst>"#,
                r#"<p:par><p:cTn id="{click}" fill="hold"><p:stCondLst><p:cond delay="0" /></p:stCondLst><p:childTnLst>"#,
                r#"<p:par><p:cTn id="{effect}" presetID="1" presetClass="entr" presetSubtype="0" fill="hold" grpId="0" nodeType="clickEffect"><p:stCondLst><p:cond delay="0" /></p:stCondLst><p:childTnLst>"#,
                r#"<p:set><p:cBhvr><p:cTn id="{behavior}" dur="1" fill="hold"><p:stCondLst><p:cond delay="0" /></p:stCondLst></p:cTn>"#,
                r#"<p:tgtEl><p:spTgt spid="{shape}"><p:txEl><p:pRg st="{paragraph}" end="{paragraph}" /></p:txEl></p:spTgt></p:tgtEl>"#,
                r#"<p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val="visible" /></p:to></p:set>"#,
                "</p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>"
            ),
            event = event,
            click = click,
            effect = effect,
            behavior = behavior,
            shape = shape,
            paragraph = paragraph
        );
    }
    xml.push_str("</p:childTnLst></p:cTn>");
    for (condition, event) in [("prevCondLst", "onPrev"), ("nextCondLst", "onNext")] {
        let _ = write!(
            xml,
            r#"<p:{condition}><p:cond evt="{event}" delay="0"><p:tgtEl><p:sldTgt /></p:tgtEl></p:cond></p:{condition}>"#
        );
    }
    xml.push_str("</p:seq></p:childTnLst></p:cTn></p:par></p:tnLst><p:bldLst>");
    let mut previous = None;
    for (shape, _) in animations {
        if previous != Some(*shape) {
            let _ = write!(
                xml,
                r#"<p:bldP spid="{shape}" grpId="0" uiExpand="1" build="p" />"#
            );
            previous = Some(*shape);
        }
    }
    xml.push_str("</p:bldLst></p:timing>");
    xml
}
