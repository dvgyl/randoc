mod drawing;
mod highlight;
mod properties;

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use randoc_archive::media::{decode_data_uri, extension_for_mime, image_size_and_mime, sha1_hex};
use randoc_archive::xml::escape_xml_attr;
use randoc_archive::zip::ArchiveWriter;
use randoc_ast::{Attr, Block, Inline, MetaValue, Pandoc};
use randoc_core::shared::sections::{prepare_slides, slide_level};
use randoc_core::shared::stringify_meta;
use randoc_core::{BinaryWriter, Context, Error, Result, WriterOptions};

use crate::package::{P, R, rel_kind, resource_path, xml_limits};

use super::reference::{
    Layout, Reference, TYPES, append_main_children, relationship, relative_target,
};

pub(crate) struct PptxWriter;

impl BinaryWriter for PptxWriter {
    fn write(
        &self,
        doc: &Pandoc,
        options: &WriterOptions,
        context: &mut Context,
    ) -> Result<Vec<u8>> {
        let depth = context.limits.max_nesting_depth;
        let level = options
            .slide_level
            .unwrap_or_else(|| slide_level(&doc.blocks));
        if !(0..=6).contains(&level) {
            return Err(Error::Option("Slide level must be between 0 and 6".into()));
        }
        let mut slides = plan(&doc.blocks, level, depth)?;
        let metadata = slide_metadata(doc);
        if metadata.iter().any(|value| !value.is_empty()) {
            let mut title = Slide {
                layout: Layout::Title,
                ..Slide::default()
            };
            if let Some(first) = slides.first_mut()
                && first.attr.is_none()
            {
                let lead = first
                    .body
                    .iter()
                    .take_while(|block| is_notes(block))
                    .count();
                title.body = first.body.drain(..lead).collect();
                if first.body.is_empty() {
                    slides.remove(0);
                }
            }
            slides.insert(0, title);
        }
        let toc = options.table_of_contents;
        if toc {
            let position = usize::from(matches!(
                slides.first().map(|slide| slide.layout),
                Some(Layout::Title)
            ));
            slides.insert(
                position,
                Slide {
                    layout: Layout::Content,
                    toc,
                    ..Slide::default()
                },
            );
        }
        if slides.len() > context.limits.max_archive_entries {
            return Err(Error::App("PPTX exceeds the archive entry limit".into()));
        }
        let headings = index_headings(&slides, depth)?;
        let mut reference = Reference::load(options, context)?;
        let stamp = context
            .host()
            .environment_variable("SOURCE_DATE_EPOCH")
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| context.host().current_time().unix_seconds);
        let mut state = DocumentState {
            context,
            options,
            slide_paths: slide_paths(&reference, slides.len() + 1),
            heading_slides: headings
                .iter()
                .map(|(id, _, _, number)| (*id, *number))
                .filter(|(id, _)| !id.is_empty())
                .collect(),
            media_sources: HashMap::new(),
            media_hashes: HashMap::new(),
            endnotes: Vec::new(),
            highlight: None,
            table_cells: 0,
            assembly_bytes: 0,
            endnote_slide: slides.len(),
        };
        // Draw the contents slide after the other slides have collected the endnotes.
        let order = slides
            .iter()
            .enumerate()
            .filter(|(_, slide)| !slide.toc)
            .chain(slides.iter().enumerate().filter(|(_, slide)| slide.toc));
        for (index, slide) in order {
            let output = drawing::draw_slide(
                &mut state,
                &mut reference,
                slide,
                index,
                &metadata,
                &headings,
            )?;
            write_slide(&state, &mut reference, index, output);
        }
        let mut count = slides.len();
        if !state.endnotes.is_empty() {
            let output = drawing::draw_endnotes(&mut state, &mut reference)?;
            write_slide(&state, &mut reference, count, output);
            count += 1;
        }
        finish(
            reference,
            &state.slide_paths[..count],
            doc,
            stamp,
            state.context,
        )
    }
}

struct Slide<'a> {
    title: &'a [Inline],
    attr: Option<&'a Attr>,
    heading_level: i64,
    layout: Layout,
    toc: bool,
    body: Vec<&'a Block>,
}

impl Default for Slide<'_> {
    fn default() -> Self {
        Self {
            title: &[],
            attr: None,
            heading_level: 0,
            layout: Layout::Blank,
            toc: false,
            body: Vec::new(),
        }
    }
}

fn plan(blocks: &[Block], level: i64, depth: usize) -> Result<Vec<Slide<'_>>> {
    let mut source: Vec<_> = blocks.iter().collect();
    // A leading wrapper can contain the first slide heading.
    while let Some(Block::Div(attr, inner)) = source.first()
        && attr.is_empty()
        && matches!(inner.first(), Some(Block::Header(rank, ..)) if *rank <= level)
    {
        source.splice(..1, inner);
    }
    let boundaries = source
        .iter()
        .map(|block| match block {
            Block::Header(rank, ..) => Block::Header(*rank, Attr::default(), Vec::new()),
            Block::HorizontalRule => Block::HorizontalRule,
            _ => Block::Plain(Vec::new()),
        })
        .collect();
    let prepared = prepare_slides(boundaries, level, depth)?;
    let mut slides = Vec::new();
    let mut current = Slide::default();
    let mut source = source.into_iter();
    let inserted = prepared.len() > source.len();
    for boundary in prepared.iter().skip(usize::from(inserted)) {
        let Some(block) = source.next() else { break };
        if matches!(boundary, Block::Header(rank, ..) if *rank <= level) {
            push_slide(&mut slides, std::mem::take(&mut current));
            if let Block::Header(rank, attr, title) = block {
                current.title = title;
                current.attr = Some(attr);
                current.heading_level = *rank;
                current.layout = if *rank < level {
                    Layout::Section
                } else {
                    Layout::Content
                };
            }
        } else {
            if matches!(current.layout, Layout::Section) && !is_notes(block) {
                push_slide(&mut slides, std::mem::take(&mut current));
            }
            current.body.push(block);
        }
    }
    push_slide(&mut slides, current);
    Ok(slides)
}

fn is_notes(block: &Block) -> bool {
    matches!(block, Block::Div(attr, _) if attr.has_class("notes"))
}

fn push_slide<'a>(slides: &mut Vec<Slide<'a>>, mut slide: Slide<'a>) {
    if slide.body.is_empty() && slide.title.is_empty() && slide.attr.is_none() {
        return;
    }
    let mut body = slide.body.iter().copied().filter(|block| !is_notes(block));
    let first = body.next();
    if !matches!(slide.layout, Layout::Section) {
        slide.layout = match first {
            Some(Block::Div(attr, columns)) if attr.has_class("columns") && columns.iter().filter(|block| matches!(block, Block::Div(attr, _) if attr.has_class("column"))).count() == 2 => {
                if columns.iter().all(|column| matches!(column, Block::Div(_, body) if body.first().is_some_and(is_text) && body.iter().any(is_picture))) {
                    Layout::Comparison
                } else {
                    Layout::Columns
                }
            }
            Some(block) if is_text(block) && body.next().is_some_and(is_captioned_picture) => Layout::Caption,
            Some(_) => Layout::Content,
            None if slide.attr.is_some() => Layout::Content,
            None => Layout::Blank,
        };
    }
    slides.push(slide);
}

fn is_text(block: &Block) -> bool {
    matches!(block, Block::Para(_) | Block::Plain(_)) && !is_picture(block)
}

fn is_picture(block: &Block) -> bool {
    match block {
        Block::Para(xs) | Block::Plain(xs) => matches!(xs.as_slice(), [Inline::Image(..)]),
        Block::Figure(_, _, blocks) => {
            blocks.len() == 1
                && matches!(&blocks[0], Block::Para(xs) | Block::Plain(xs) if matches!(xs.as_slice(), [Inline::Image(..)]))
        }
        _ => false,
    }
}

fn is_captioned_picture(block: &Block) -> bool {
    match block {
        Block::Para(xs) | Block::Plain(xs) => {
            matches!(xs.as_slice(), [Inline::Image(_, caption, _)] if !caption.is_empty())
        }
        Block::Figure(_, caption, _) => {
            is_picture(block) && (!caption.long.is_empty() || caption.short.is_some())
        }
        _ => false,
    }
}

type Heading<'a> = (&'a str, i64, &'a [Inline], usize);

fn index_headings<'a>(slides: &[Slide<'a>], depth: usize) -> Result<Vec<Heading<'a>>> {
    let mut headings = Vec::new();
    for (index, slide) in slides.iter().enumerate() {
        if let Some(attr) = slide.attr {
            headings.push((attr.id.as_str(), slide.heading_level, slide.title, index));
        }
        let mut work: Vec<_> = slide.body.iter().rev().map(|block| (*block, 1)).collect();
        while let Some((block, level)) = work.pop() {
            check_depth(level, depth)?;
            match block {
                Block::Header(rank, attr, inlines) => {
                    headings.push((attr.id.as_str(), *rank, inlines, index))
                }
                Block::Div(_, blocks) | Block::BlockQuote(blocks) | Block::Figure(_, _, blocks) => {
                    work.extend(blocks.iter().rev().map(|block| (block, level + 1)));
                }
                Block::BulletList(items) | Block::OrderedList(_, items) => {
                    work.extend(items.iter().rev().flatten().map(|block| (block, level + 1)));
                }
                Block::DefinitionList(items) => {
                    work.extend(
                        items
                            .iter()
                            .rev()
                            .flat_map(|(_, defs)| {
                                defs.iter().rev().flat_map(|blocks| blocks.iter().rev())
                            })
                            .map(|block| (block, level + 1)),
                    );
                }
                _ => {}
            }
        }
    }
    Ok(headings)
}

fn check_depth(depth: usize, max: usize) -> Result<()> {
    if depth > max {
        return Err(Error::App(format!(
            "the value is nested deeper than the limit of {max} levels"
        )));
    }
    Ok(())
}

fn meta_value(doc: &Pandoc, key: &str, separator: &str) -> String {
    match doc.meta.get(key) {
        Some(MetaValue::MetaList(values)) => values
            .iter()
            .map(stringify_meta)
            .collect::<Vec<_>>()
            .join(separator),
        Some(value) => stringify_meta(value),
        None => String::new(),
    }
}

fn slide_metadata(doc: &Pandoc) -> [String; 4] {
    [
        meta_value(doc, "title", " "),
        meta_value(doc, "subtitle", " "),
        meta_value(doc, "author", "\n"),
        meta_value(doc, "date", " "),
    ]
}

#[derive(Clone)]
struct Media {
    path: String,
    size: (u32, u32),
}

struct DocumentState<'a, 'b> {
    context: &'a mut Context,
    options: &'a WriterOptions,
    slide_paths: Vec<String>,
    heading_slides: HashMap<&'b str, usize>,
    media_sources: HashMap<String, Media>,
    media_hashes: HashMap<String, Media>,
    endnotes: Vec<&'b [Block]>,
    table_cells: usize,
    highlight: Option<highlight::Highlight>,
    assembly_bytes: u64,
    endnote_slide: usize,
}

impl DocumentState<'_, '_> {
    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.assembly_bytes = self
            .assembly_bytes
            .checked_add(bytes as u64)
            .filter(|total| *total <= output_limit(self.context))
            .ok_or_else(|| Error::App("PPTX exceeds the output limit".into()))?;
        Ok(())
    }

    fn media(&mut self, reference: &mut Reference, source: &str) -> Result<Media> {
        if let Some(media) = self.media_sources.get(source) {
            return Ok(media.clone());
        }
        let limit = self.context.limits.max_resource_size;
        let (bytes, declared) = if let Some(item) = self.context.media.get(source) {
            if item.contents.len() as u64 > limit {
                return Err(Error::App("Image exceeds the resource limit".into()));
            }
            (item.contents.clone(), Some(item.mime_type.clone()))
        } else if source.starts_with("data:") {
            let (mime, bytes) = decode_data_uri(source, limit)
                .ok_or_else(|| Error::ResourceNotFound(source.chars().take(80).collect()))?;
            (bytes, Some(mime))
        } else if source.starts_with("https://") || source.starts_with("http://") {
            let headers = self.context.state.request_headers.clone();
            self.context.host().fetch_url(source, &headers)?
        } else {
            let path = resource_path(self.context, source)
                .ok_or_else(|| Error::ResourceNotFound(source.into()))?;
            (
                self.context
                    .read_file_limited(&path, usize::try_from(limit).unwrap_or(usize::MAX))?,
                None,
            )
        };
        if bytes.len() as u64 > limit {
            return Err(Error::App("Image exceeds the resource limit".into()));
        }
        let hash = sha1_hex(&bytes);
        if let Some(media) = self.media_hashes.get(&hash) {
            self.media_sources.insert(source.into(), media.clone());
            return Ok(media.clone());
        }
        let (size, detected) = image_size_and_mime(&bytes, Some(source));
        let mime = detected
            .or(declared.as_deref())
            .unwrap_or("application/octet-stream");
        let ext = extension_for_mime(mime).unwrap_or("bin");
        self.charge(bytes.len())?;
        let path = reference.unused_path("ppt/media/image", ext);
        reference.overrides.insert(path.clone(), mime.into());
        reference.parts.insert(path.clone(), bytes);
        let media = Media {
            path,
            size: size.unwrap_or((4, 3)),
        };
        self.media_hashes.insert(hash, media.clone());
        self.media_sources.insert(source.into(), media.clone());
        Ok(media)
    }
}

fn write_slide(
    state: &DocumentState<'_, '_>,
    reference: &mut Reference,
    index: usize,
    output: drawing::SlideOutput,
) {
    let path = &state.slide_paths[index];
    let mut relationships = output.relationships;
    if let Some(notes) = output.notes {
        reference.add_notes_master();
        let note_path = reference.unused_path("ppt/notesSlides/notesSlide", "xml");
        relationships.push(relationship(
            relationships.len() + 1,
            "notesSlide",
            relative_target(path, &note_path),
            false,
        ));
        let note_rels = [
            relationship(
                1,
                "notesMaster",
                relative_target(&note_path, &reference.notes_master),
                false,
            ),
            relationship(2, "slide", relative_target(&note_path, path), false),
        ];
        reference.add_rels(&note_path, &note_rels);
        reference.add_xml(note_path, "presentationml.notesSlide", notes);
    }
    reference.add_rels(path, &relationships);
    reference.add_xml(path.clone(), "presentationml.slide", output.xml);
}

fn slide_paths(reference: &Reference, count: usize) -> Vec<String> {
    let mut paths = Vec::new();
    for number in 1.. {
        let path = format!("ppt/slides/slide{number}.xml");
        if !reference.parts.contains_key(&path) && reference.main_path != path {
            paths.push(path);
            if paths.len() == count {
                break;
            }
        }
    }
    paths
}

fn output_limit(context: &Context) -> u64 {
    context
        .limits
        .max_input_size
        .min(context.limits.max_archive_size)
}

fn finish(
    mut reference: Reference,
    slides: &[String],
    doc: &Pandoc,
    stamp: i64,
    context: &Context,
) -> Result<Vec<u8>> {
    let mut next = if reference.imported {
        reference
            .main_rels
            .iter()
            .filter(|rel| rel_kind(rel, "slideMaster"))
            .filter_map(|rel| rel.id.strip_prefix("rId")?.parse::<usize>().ok())
            .max()
            .and_then(|id| id.checked_add(1))
            .unwrap_or(1)
    } else {
        1
    };
    let mut trailing = Vec::new();
    let mut used = HashSet::new();
    for (index, rel) in reference.main_rels.iter().enumerate() {
        if reference.imported
            && ["presProps", "viewProps", "tableStyles"]
                .iter()
                .any(|kind| rel_kind(rel, kind))
            && let Some(number) = rel
                .id
                .strip_prefix("rId")
                .and_then(|id| id.parse::<usize>().ok())
            && number >= next
        {
            trailing.push((index, number));
        } else {
            used.insert(rel.id.clone());
        }
    }
    let mut allocate = || {
        loop {
            let number = next;
            next = next.checked_add(1).unwrap_or(1);
            if used.insert(format!("rId{number}")) {
                return number;
            }
        }
    };
    let mut make_rel = |kind: &str, target: String| relationship(allocate(), kind, target, false);
    let mut slide_rels = Vec::new();
    if reference.imported {
        slide_rels.extend(
            slides
                .iter()
                .map(|path| make_rel("slide", relative_target(&reference.main_path, path))),
        );
    }
    let mut notes_xml = String::new();
    if !reference.notes_master.is_empty() {
        let id = if let Some(rel) = reference
            .main_rels
            .iter()
            .find(|rel| rel_kind(rel, "notesMaster"))
        {
            rel.id.clone()
        } else {
            let rel = make_rel(
                "notesMaster",
                relative_target(&reference.main_path, &reference.notes_master),
            );
            let id = rel.id.clone();
            if reference.imported {
                slide_rels.push(rel);
            } else {
                reference.main_rels.push(rel);
            }
            id
        };
        let id = attr(&id);
        let _ = write!(
            notes_xml,
            r#"<p:notesMasterIdLst xmlns:p="{P}" xmlns:r="{R}"><p:notesMasterId r:id="{id}" /></p:notesMasterIdLst>"#
        );
    }
    let mut slide_xml = format!(r#"<p:sldIdLst xmlns:p="{P}" xmlns:r="{R}">"#);
    for (index, path) in slides.iter().enumerate() {
        let rel = if let Some(rel) = slide_rels.get(index) {
            rel.clone()
        } else {
            make_rel("slide", relative_target(&reference.main_path, path))
        };
        let _ = write!(
            slide_xml,
            r#"<p:sldId id="{}" r:id="{}" />"#,
            index + 256,
            rel.id
        );
        if !reference.imported {
            reference.main_rels.push(rel);
        }
    }
    if reference.imported {
        // Allocate the property IDs in numeric order and keep their XML order.
        trailing.sort_by_key(|&(_, number)| number);
        for (index, _) in trailing {
            reference.main_rels[index].id = format!("rId{}", allocate());
        }
        slide_rels.append(&mut reference.main_rels);
        reference.main_rels = slide_rels;
    }
    slide_xml.push_str("</p:sldIdLst>");
    let xml_limits = xml_limits(&context.limits);
    append_main_children(&mut reference, &slide_xml, &notes_xml, &xml_limits)?;
    let rels = super::reference::relationship_xml(&reference.main_rels);
    reference.parts.insert(
        randoc_archive::opc::rels_part_for(&reference.main_path),
        rels.into_bytes(),
    );
    properties::add(&mut reference, doc, stamp, slides.len());
    let root_rels = std::mem::take(&mut reference.root_rels);
    reference.add_rels("", &root_rels);
    let mut types = format!(
        r#"{}<Types xmlns="{TYPES}">"#,
        super::reference::DECLARATION
    );
    let mut defaults: Vec<_> = reference.defaults.iter().collect();
    let mut overrides: Vec<_> = reference.overrides.iter().collect();
    if reference.imported {
        defaults.sort_by_key(|(ext, _)| match ext.as_str() {
            "xml" => 0,
            "rels" => 1,
            _ => 2,
        });
        overrides.sort_by_key(|(path, mime)| reference.manifest_order(path, mime));
    }
    for (ext, mime) in defaults {
        let _ = write!(
            types,
            r#"<Default Extension="{}" ContentType="{}" />"#,
            attr(ext),
            attr(mime)
        );
    }
    for (part, mime) in overrides {
        let _ = write!(
            types,
            r#"<Override PartName="/{}" ContentType="{}" />"#,
            attr(part),
            attr(mime)
        );
    }
    types.push_str("</Types>");
    reference
        .parts
        .insert("[Content_Types].xml".into(), types.into_bytes());
    if reference.parts.len() > context.limits.max_archive_entries {
        return Err(Error::App("PPTX exceeds the archive entry limit".into()));
    }
    let mut archive = ArchiveWriter::with_input_limit(output_limit(context));
    let error = |error| Error::App(format!("Could not write PPTX: {error}"));
    for (name, bytes) in reference.parts {
        archive
            .write_deflated_with_time(&name, &bytes, Some(stamp))
            .map_err(error)?;
    }
    archive.finish().map_err(error)
}

fn attr(text: &str) -> String {
    let mut out = String::new();
    escape_xml_attr(text, &mut out);
    out
}
