//! Main DOCX-to-Pandoc AST conversion pipeline.

use std::collections::{BTreeMap, HashMap, HashSet};

use randoc_archive::media::image_size_and_mime;
use randoc_archive::opc::{
    Relationship, media_name, parse_relationships, rels_part_for, resolve_target,
};
use randoc_archive::xml::{self, XmlDoc, XmlElement, XmlError, XmlLimits, XmlNode, XmlOptions};
use randoc_archive::zip::{Archive, ArchiveError, ArchiveLimits};
use randoc_ast::{
    Alignment, Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, ColWidth, Inline,
    ListAttributes, MathType, Meta, MetaValue, Pandoc, Row, Table, TableBody, TableFoot, TableHead,
    Target, Text,
};
use randoc_core::shared::{IdentifierSet, auto_identifier, stringify};
use randoc_core::{Context, Error, Extension, MediaItem, ReaderOptions, Result, TrackChanges};

use crate::ns::{DocxNs, is_a, is_mc, is_pic, is_v, is_w14, is_wp, is_wps};
use crate::numbering::{ListKind, Numbering};
use crate::omml::omml_to_tex;
use crate::styles::{ParaProps, RunProps, StyleSheet, VertAlign, heading_info, is_code_char_style};
use crate::symbol::decode_symbol;

const DOC_ERROR: &str = "couldn't parse docx file: DocxError";
const DEFAULT_PAGE_WIDTH: f64 = 9360.0;

/// Reads a DOCX binary archive into a [`Pandoc`] document.
pub fn read_docx(bytes: &[u8], options: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    if bytes.len() as u64 > context.limits.max_input_size {
        return Err(Error::Parse(format!(
            "DOCX input size {} exceeds limit of {} bytes",
            bytes.len(),
            context.limits.max_input_size
        )));
    }

    let arc_limits = ArchiveLimits::from_limits(&context.limits);
    let mut archive = Archive::open(bytes, arc_limits).map_err(map_open_error)?;
    let xml_limits = XmlLimits::from_limits(&context.limits);

    let root_rels_bytes = archive
        .read("_rels/.rels")
        .map_err(map_read_error)?
        .ok_or_else(|| Error::Some(DOC_ERROR.into()))?;
    let root_rels_doc = parse_xml_strict(&root_rels_bytes, &xml_limits)?;
    let root_rels = parse_relationships(&root_rels_doc);
    let main_target = root_rels
        .iter()
        .find(|r| !r.external && r.kind.ends_with("/officeDocument"))
        .map(|r| r.target.as_str())
        .ok_or_else(|| Error::Some(DOC_ERROR.into()))?;
    let main_part = resolve_target("", main_target).ok_or_else(|| Error::Some(DOC_ERROR.into()))?;

    let main_bytes = archive
        .read(&main_part)
        .map_err(map_read_error)?
        .ok_or_else(|| Error::Some(DOC_ERROR.into()))?;
    let main_doc = parse_xml_strict(&main_bytes, &xml_limits)?;
    let main_ns = DocxNs::from_root(&main_doc.root);

    // P7 rule: root must declare `xmlns:w` and root element must be `w:document`.
    if main_ns.w_uri.is_none()
        || !main_ns.is_w(&main_doc.root, "document")
        || main_doc.root.name.prefix.as_deref() != Some("w")
    {
        return Err(Error::Some(DOC_ERROR.into()));
    }

    let doc_rels = load_part_rels(&mut archive, &main_part, &xml_limits);
    let styles = load_optional_part(&mut archive, &main_part, &doc_rels, "/styles", &xml_limits)
        .map(|doc| StyleSheet::parse(&doc))
        .unwrap_or_default();
    let numbering = load_optional_part(
        &mut archive,
        &main_part,
        &doc_rels,
        "/numbering",
        &xml_limits,
    )
    .map(|doc| Numbering::parse(&doc))
    .unwrap_or_default();

    let footnotes = load_notes_part(
        &mut archive,
        &main_part,
        &doc_rels,
        "/footnotes",
        "footnote",
        &xml_limits,
    );
    let endnotes = load_notes_part(
        &mut archive,
        &main_part,
        &doc_rels,
        "/endnotes",
        "endnote",
        &xml_limits,
    );
    let comments = load_comments_part(&mut archive, &main_part, &doc_rels, &xml_limits);

    let body = main_ns
        .w_child(&main_doc.root, "body")
        .ok_or_else(|| Error::Some(DOC_ERROR.into()))?;

    let page_width = extract_page_width(body, &main_ns);

    let max_expanded_nodes = context.limits.max_table_cells.saturating_mul(2).max(50_000);
    let max_expanded_text_bytes = context.limits.max_archive_size;
    let mut conv = Converter {
        archive: &mut archive,
        context,
        options,
        styles: &styles,
        numbering: &numbering,
        footnotes: &footnotes,
        endnotes: &endnotes,
        comments: &comments,
        active_footnotes: HashSet::new(),
        active_endnotes: HashSet::new(),
        active_comments: HashSet::new(),
        expanded_nodes_used: 0,
        max_expanded_nodes,
        expanded_text_bytes_used: 0,
        max_expanded_text_bytes,
        media_cache: HashMap::new(),
        table_cells_used: 0,
        depth: 0,
        page_width,
        references: BTreeMap::new(),
    };

    let raw_items = conv.parse_body_children(body, &main_ns, &main_part, &doc_rels)?;
    let mut blocks = conv.build_blocks(raw_items, true, true)?;
    let references: Vec<MetaValue> = conv.references.into_values().collect();

    // Post-passes: metadata extraction, auto-identifiers, bookmark pruning/retargeting.
    let mut meta = extract_metadata(&mut blocks);
    if !references.is_empty() {
        meta.insert("references", MetaValue::MetaList(references));
    }
    apply_identifiers_and_bookmarks(&mut blocks, options);

    Ok(Pandoc { meta, blocks })
}

fn extract_page_width(body: &XmlElement, ns: &DocxNs) -> f64 {
    let Some(sect) = ns.w_child(body, "sectPr") else {
        return DEFAULT_PAGE_WIDTH;
    };
    let Some(pg_sz) = ns.w_child(sect, "pgSz") else {
        return DEFAULT_PAGE_WIDTH;
    };
    let Some(pg_mar) = ns.w_child(sect, "pgMar") else {
        return DEFAULT_PAGE_WIDTH;
    };
    let w = ns.w_attr(pg_sz, "w").and_then(|s| s.parse::<f64>().ok());
    let left = ns
        .w_attr(pg_mar, "left")
        .and_then(|s| s.parse::<f64>().ok());
    let right = ns
        .w_attr(pg_mar, "right")
        .and_then(|s| s.parse::<f64>().ok());
    let gutter = ns
        .w_attr(pg_mar, "gutter")
        .and_then(|s| s.parse::<f64>().ok());
    match (w, left, right, gutter) {
        (Some(w), Some(l), Some(r), Some(g)) if w > l + r + g => w - l - r - g,
        _ => DEFAULT_PAGE_WIDTH,
    }
}

fn map_open_error(err: ArchiveError) -> Error {
    match err {
        ArchiveError::NotEnoughBytes => {
            Error::Some("couldn't unpack docx container: not enough bytes".into())
        }
        ArchiveError::NoCentralDirectory => Error::Some(
            "couldn't unpack docx container: Did not find end of central directory signature"
                .into(),
        ),
        ArchiveError::InputTooLarge { .. }
        | ArchiveError::TooManyEntries { .. }
        | ArchiveError::DuplicateNames
        | ArchiveError::EntryTooLarge { .. }
        | ArchiveError::TotalTooLarge { .. }
        | ArchiveError::CompressionRatioExceeded { .. } => Error::Parse(err.to_string()),
        ArchiveError::Corrupt { .. } | ArchiveError::Write { .. } => {
            Error::Some(format!("couldn't unpack docx container: {err}"))
        }
    }
}

fn map_read_error(err: ArchiveError) -> Error {
    match err {
        ArchiveError::EntryTooLarge { .. }
        | ArchiveError::TotalTooLarge { .. }
        | ArchiveError::CompressionRatioExceeded { .. } => Error::Parse(err.to_string()),
        _ => Error::Some(DOC_ERROR.into()),
    }
}

fn parse_xml_strict(bytes: &[u8], limits: &XmlLimits) -> Result<XmlDoc> {
    let text = xml::decode_utf8(bytes).map_err(|err| match err {
        XmlError::NotUtf8 { byte, .. } => Error::Fatal(format!(
            "Cannot decode byte '\\x{byte:02x}': Data.Text.Internal.Encoding: Invalid UTF-8 stream"
        )),
        _ => Error::Some(DOC_ERROR.into()),
    })?;
    xml::parse(text, limits, XmlOptions::default()).map_err(|err| match err {
        XmlError::Depth(limit) => {
            Error::Parse(format!("DOCX XML nesting exceeded limit of {limit} levels"))
        }
        XmlError::EntityBudget => Error::Parse("DOCX XML entity expansion exceeded limit".into()),
        XmlError::DomBudget => Error::Parse("DOCX XML DOM allocation exceeded limit".into()),
        _ => Error::Some(DOC_ERROR.into()),
    })
}

fn parse_xml_optional(bytes: &[u8], limits: &XmlLimits) -> Option<XmlDoc> {
    let text = xml::decode_utf8(bytes).ok()?;
    xml::parse(text, limits, XmlOptions::default()).ok()
}

type RelsMap = HashMap<Text, Relationship>;

fn load_part_rels(archive: &mut Archive<'_>, part: &str, limits: &XmlLimits) -> RelsMap {
    let rels_path = rels_part_for(part);
    let Ok(Some(bytes)) = archive.read(&rels_path) else {
        return HashMap::new();
    };
    let Some(doc) = parse_xml_optional(&bytes, limits) else {
        return HashMap::new();
    };
    parse_relationships(&doc)
        .into_iter()
        .map(|r| (r.id.clone(), r))
        .collect()
}

fn find_rel_target(part: &str, rels: &RelsMap, suffix: &str) -> Option<String> {
    rels.values()
        .find(|r| !r.external && r.kind.ends_with(suffix))
        .and_then(|r| resolve_target(part, &r.target))
}

fn load_optional_part(
    archive: &mut Archive<'_>,
    parent_part: &str,
    rels: &RelsMap,
    kind_suffix: &str,
    limits: &XmlLimits,
) -> Option<XmlDoc> {
    let path = find_rel_target(parent_part, rels, kind_suffix)?;
    let bytes = archive.read(&path).ok()??;
    parse_xml_optional(&bytes, limits)
}

struct NotesPart {
    part_path: String,
    rels: RelsMap,
    ns: DocxNs,
    notes: HashMap<Text, XmlElement>,
}

impl Default for NotesPart {
    fn default() -> Self {
        Self {
            part_path: String::new(),
            rels: HashMap::new(),
            ns: DocxNs {
                w_uri: None,
                r_uri: None,
                m_uri: None,
            },
            notes: HashMap::new(),
        }
    }
}

fn load_notes_part(
    archive: &mut Archive<'_>,
    parent_part: &str,
    parent_rels: &RelsMap,
    kind_suffix: &str,
    elem_local: &str,
    limits: &XmlLimits,
) -> NotesPart {
    let Some(part_path) = find_rel_target(parent_part, parent_rels, kind_suffix) else {
        return NotesPart::default();
    };
    let Ok(Some(bytes)) = archive.read(&part_path) else {
        return NotesPart::default();
    };
    let Some(doc) = parse_xml_optional(&bytes, limits) else {
        return NotesPart::default();
    };
    let ns = DocxNs::from_root(&doc.root);
    let rels = load_part_rels(archive, &part_path, limits);
    let mut notes = HashMap::new();
    for el in ns.w_children(&doc.root, elem_local) {
        if let Some(ty) = ns.w_attr(el, "type")
            && matches!(
                ty,
                "separator" | "continuationSeparator" | "continuationNotice"
            )
        {
            continue;
        }
        if let Some(id) = ns.w_attr(el, "id") {
            notes.insert(Text::from(id), el.clone());
        }
    }
    NotesPart {
        part_path,
        rels,
        ns,
        notes,
    }
}

struct CommentDef {
    author: Text,
    date: Text,
    element: XmlElement,
}

struct CommentsPart {
    part_path: String,
    rels: RelsMap,
    ns: DocxNs,
    comments: HashMap<Text, CommentDef>,
}

impl Default for CommentsPart {
    fn default() -> Self {
        Self {
            part_path: String::new(),
            rels: HashMap::new(),
            ns: DocxNs {
                w_uri: None,
                r_uri: None,
                m_uri: None,
            },
            comments: HashMap::new(),
        }
    }
}

fn load_comments_part(
    archive: &mut Archive<'_>,
    parent_part: &str,
    parent_rels: &RelsMap,
    limits: &XmlLimits,
) -> CommentsPart {
    let Some(part_path) = find_rel_target(parent_part, parent_rels, "/comments") else {
        return CommentsPart::default();
    };
    let Ok(Some(bytes)) = archive.read(&part_path) else {
        return CommentsPart::default();
    };
    let Some(doc) = parse_xml_optional(&bytes, limits) else {
        return CommentsPart::default();
    };
    let ns = DocxNs::from_root(&doc.root);
    let rels = load_part_rels(archive, &part_path, limits);
    let mut comments = HashMap::new();
    for el in ns.w_children(&doc.root, "comment") {
        let Some(id) = ns.w_attr(el, "id") else {
            continue;
        };
        let author = ns.w_attr(el, "author").unwrap_or("").to_owned();
        let date = ns.w_attr(el, "date").unwrap_or("").to_owned();
        comments.insert(
            Text::from(id),
            CommentDef {
                author,
                date,
                element: el.clone(),
            },
        );
    }
    CommentsPart {
        part_path,
        rels,
        ns,
        comments,
    }
}

// ---------------------------------------------------------------------------
// Intermediate document model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ChangeInfo {
    author: Text,
    date: Text,
}

#[derive(Debug, Clone)]
enum ParaMarkChange {
    None,
    Inserted(ChangeInfo),
    Deleted(ChangeInfo),
}

#[derive(Debug, Clone)]
struct ParsedPara {
    props: ParaProps,
    style_names: Vec<Text>,
    inlines: Vec<Inline>,
    mark_change: ParaMarkChange,
    has_seq_table: bool,
    has_seq_any: bool,
    is_bibl_start: bool,
    is_bibl_end: bool,
    extra_blocks: Vec<Block>,
    caption_blocks: Vec<Block>,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum BodyItem {
    Para(ParsedPara),
    Table(Block),
}

#[derive(Debug, Clone)]
enum RawInline {
    Run {
        props: RunProps,
        items: Vec<RunAtom>,
    },
    Inline(Inline),
    Tracked {
        class: &'static str,
        info: ChangeInfo,
        items: Vec<RawInline>,
    },
    Link {
        target: Text,
        title: Text,
        items: Vec<RawInline>,
    },
    FldBegin,
    FldSeparate,
    FldEnd,
    FldInstr(Text),
}

#[derive(Debug, Clone)]
enum RunAtom {
    Text(Text),
    Tab,
    LineBreak,
    Inline(Inline),
}

struct Converter<'a, 'b> {
    archive: &'a mut Archive<'b>,
    context: &'a mut Context,
    options: &'a ReaderOptions,
    styles: &'a StyleSheet,
    numbering: &'a Numbering,
    footnotes: &'a NotesPart,
    endnotes: &'a NotesPart,
    comments: &'a CommentsPart,
    active_footnotes: HashSet<Text>,
    active_endnotes: HashSet<Text>,
    active_comments: HashSet<Text>,
    expanded_nodes_used: usize,
    max_expanded_nodes: usize,
    expanded_text_bytes_used: u64,
    max_expanded_text_bytes: u64,
    media_cache: HashMap<String, Option<Text>>,
    table_cells_used: usize,
    depth: usize,
    page_width: f64,
    references: BTreeMap<Text, MetaValue>,
}

impl<'a, 'b> Converter<'a, 'b> {
    fn in_expanded_note_or_comment(&self) -> bool {
        !self.active_footnotes.is_empty()
            || !self.active_endnotes.is_empty()
            || !self.active_comments.is_empty()
    }

    fn charge_expanded_node(&mut self) -> Result<()> {
        self.expanded_nodes_used = self.expanded_nodes_used.saturating_add(1);
        if self.expanded_nodes_used > self.max_expanded_nodes {
            return Err(Error::Parse(format!(
                "DOCX note/comment expansion exceeded limit of {} nodes",
                self.max_expanded_nodes
            )));
        }
        Ok(())
    }
    fn run_text(&mut self, element: &XmlElement) -> Result<Text> {
        if self.in_expanded_note_or_comment() {
            for node in &element.children {
                if let XmlNode::Text(text) = node {
                    self.expanded_text_bytes_used = self
                        .expanded_text_bytes_used
                        .saturating_add(text.len() as u64);
                    if self.expanded_text_bytes_used > self.max_expanded_text_bytes {
                        return Err(Error::Parse(format!(
                            "DOCX note/comment expansion exceeded limit of {} text bytes",
                            self.max_expanded_text_bytes
                        )));
                    }
                }
            }
        }
        Ok(element.text())
    }

    fn keep_styles(&self) -> bool {
        self.options.extensions.contains(Extension::Styles)
    }

    fn empty_paragraphs(&self) -> bool {
        self.options.extensions.contains(Extension::EmptyParagraphs)
    }

    fn citations_enabled(&self) -> bool {
        self.options.extensions.contains(Extension::Citations)
    }

    fn parse_body_children(
        &mut self,
        parent: &XmlElement,
        ns: &DocxNs,
        part_path: &str,
        rels: &RelsMap,
    ) -> Result<Vec<BodyItem>> {
        if self.depth > self.context.limits.max_nesting_depth {
            return Err(Error::Parse(format!(
                "DOCX nesting exceeded limit of {} levels",
                self.context.limits.max_nesting_depth
            )));
        }
        if self.in_expanded_note_or_comment() {
            self.charge_expanded_node()?;
        }
        self.depth += 1;
        let mut items = Vec::new();
        for child in parent.elements() {
            self.parse_body_element(child, ns, part_path, rels, &mut items)?;
        }
        self.depth -= 1;
        Ok(items)
    }

    fn parse_body_element(
        &mut self,
        el: &XmlElement,
        ns: &DocxNs,
        part_path: &str,
        rels: &RelsMap,
        out: &mut Vec<BodyItem>,
    ) -> Result<()> {
        if self.in_expanded_note_or_comment() {
            self.charge_expanded_node()?;
        }
        if ns.is_w(el, "p") {
            let p = self.parse_paragraph(el, ns, part_path, rels)?;
            out.push(BodyItem::Para(p));
        } else if ns.is_w(el, "tbl") {
            let tbl = self.parse_table(el, ns, part_path, rels)?;
            out.push(BodyItem::Table(tbl));
        } else if ns.is_w(el, "sdt") {
            if let Some(content) = ns.w_child(el, "sdtContent") {
                let inner = self.parse_body_children(content, ns, part_path, rels)?;
                out.extend(inner);
            }
        } else if ns.is_w(el, "smartTag") || ns.is_w(el, "customXml") {
            let inner = self.parse_body_children(el, ns, part_path, rels)?;
            out.extend(inner);
        } else if ns.is_w(el, "ins") || ns.is_w(el, "moveTo") {
            if self.options.track_changes != TrackChanges::RejectChanges {
                let inner = self.parse_body_children(el, ns, part_path, rels)?;
                out.extend(inner);
            }
        } else if ns.is_w(el, "del") || ns.is_w(el, "moveFrom") {
            if self.options.track_changes != TrackChanges::AcceptChanges {
                let inner = self.parse_body_children(el, ns, part_path, rels)?;
                out.extend(inner);
            }
        } else if is_mc(el, "AlternateContent")
            && let Some(chosen) = choose_alternate_content(el)
        {
            let inner = self.parse_body_children(chosen, ns, part_path, rels)?;
            out.extend(inner);
        }
        Ok(())
    }

    fn parse_paragraph(
        &mut self,
        el: &XmlElement,
        ns: &DocxNs,
        part_path: &str,
        rels: &RelsMap,
    ) -> Result<ParsedPara> {
        let p_pr_el = ns.w_child(el, "pPr");
        let direct_p = p_pr_el.map(|p| ParaProps::parse(p, ns)).unwrap_or_default();
        let (mut props, style_names) = self.styles.resolve_para(&direct_p);

        if props.num_pr.is_none()
            && let Some(sid) = props.style_id.as_deref()
            && let Some((nid, ilvl)) = self.numbering.by_style_id(sid)
        {
            props.num_pr = Some(crate::styles::NumPr {
                num_id: Text::from(nid),
                ilvl,
            });
        }

        let mark_change = p_pr_el
            .and_then(|p| ns.w_child(p, "rPr"))
            .map(|rpr| {
                if let Some(ins) = ns.w_child(rpr, "ins") {
                    ParaMarkChange::Inserted(parse_change_info(ins, ns))
                } else if let Some(del) = ns.w_child(rpr, "del") {
                    ParaMarkChange::Deleted(parse_change_info(del, ns))
                } else {
                    ParaMarkChange::None
                }
            })
            .unwrap_or(ParaMarkChange::None);

        let is_code_para = is_source_code_style(&style_names);
        let mut raw_inlines = Vec::new();
        let mut extra_blocks = Vec::new();
        let mut caption_blocks = Vec::new();

        for child in el.elements() {
            if ns.is_w(child, "pPr") {
                continue;
            }
            if ns.is_m(child, "oMathPara") {
                for sub in child.elements() {
                    if ns.is_m(sub, "oMath") {
                        let tex = omml_to_tex(sub, ns, self.context.limits.max_nesting_depth);
                        raw_inlines
                            .push(RawInline::Inline(Inline::Math(MathType::DisplayMath, tex)));
                    }
                }
                continue;
            }
            self.parse_inline_element(
                child,
                ns,
                part_path,
                rels,
                false,
                false,
                &mut raw_inlines,
                &mut extra_blocks,
                &mut caption_blocks,
            )?;
        }

        let (resolved, seq_info) = self.resolve_fields(raw_inlines)?;
        let mut inlines = if is_code_para {
            flatten_code_inlines(&resolved)
        } else {
            self.render_raw_inlines(resolved)
        };

        if self.options.track_changes == TrackChanges::AllChanges {
            match &mark_change {
                ParaMarkChange::Inserted(info) => {
                    inlines.push(make_change_span("paragraph-insertion", info, Vec::new()));
                }
                ParaMarkChange::Deleted(info) => {
                    inlines.push(make_change_span("paragraph-deletion", info, Vec::new()));
                }
                ParaMarkChange::None => {}
            }
        }

        Ok(ParsedPara {
            props,
            style_names,
            inlines,
            mark_change,
            has_seq_table: seq_info.has_table,
            has_seq_any: seq_info.has_any,
            is_bibl_start: seq_info.is_bibl_start,
            is_bibl_end: seq_info.is_bibl_end,
            extra_blocks,
            caption_blocks,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_inline_element(
        &mut self,
        el: &XmlElement,
        ns: &DocxNs,
        part_path: &str,
        rels: &RelsMap,
        in_deleted: bool,
        in_link: bool,
        out: &mut Vec<RawInline>,
        extra_blocks: &mut Vec<Block>,
        caption_blocks: &mut Vec<Block>,
    ) -> Result<()> {
        if self.depth > self.context.limits.max_nesting_depth {
            return Err(Error::Parse(format!(
                "DOCX nesting exceeded limit of {} levels",
                self.context.limits.max_nesting_depth
            )));
        }
        if self.in_expanded_note_or_comment() {
            self.charge_expanded_node()?;
        }

        if ns.is_w(el, "r") {
            self.parse_run(
                el,
                ns,
                part_path,
                rels,
                in_deleted,
                in_link,
                out,
                extra_blocks,
                caption_blocks,
            )?;
        } else if ns.is_w(el, "hyperlink") {
            let base_url = ns
                .r_attr(el, "id")
                .and_then(|rid| rels.get(rid))
                .map(|r| r.target.as_str());
            let anchor = ns.w_attr(el, "anchor");
            let target = match (base_url, anchor) {
                (Some(u), Some(a)) if !a.is_empty() => format!("{u}#{a}"),
                (Some(u), _) => u.to_owned(),
                (None, Some(a)) if !a.is_empty() => format!("#{a}"),
                _ => String::new(),
            };
            let title = ns.w_attr(el, "tooltip").unwrap_or("").to_owned();
            let mut inner = Vec::new();
            self.depth += 1;
            for c in el.elements() {
                self.parse_inline_element(
                    c,
                    ns,
                    part_path,
                    rels,
                    in_deleted,
                    true,
                    &mut inner,
                    extra_blocks,
                    caption_blocks,
                )?;
            }
            self.depth -= 1;
            if target.is_empty() {
                out.extend(inner);
            } else {
                out.push(RawInline::Link {
                    target,
                    title,
                    items: inner,
                });
            }
        } else if ns.is_w(el, "fldSimple") {
            let instr = ns.w_attr(el, "instr").unwrap_or("").to_owned();
            let is_hl = instr.trim().to_ascii_uppercase().starts_with("HYPERLINK ");
            out.push(RawInline::FldBegin);
            out.push(RawInline::FldInstr(instr));
            out.push(RawInline::FldSeparate);
            self.depth += 1;
            for c in el.elements() {
                self.parse_inline_element(
                    c,
                    ns,
                    part_path,
                    rels,
                    in_deleted,
                    in_link || is_hl,
                    out,
                    extra_blocks,
                    caption_blocks,
                )?;
            }
            self.depth -= 1;
            out.push(RawInline::FldEnd);
        } else if ns.is_w(el, "bookmarkStart") {
            if let Some(name) = ns.w_attr(el, "name")
                && !name.is_empty()
                && !name.eq_ignore_ascii_case("_GoBack")
            {
                out.push(RawInline::Inline(Inline::Span(
                    Attr::new(name, vec!["anchor".into()], vec![]),
                    Vec::new(),
                )));
            }
        } else if ns.is_w(el, "ins") || ns.is_w(el, "moveTo") {
            match self.options.track_changes {
                TrackChanges::RejectChanges => {}
                TrackChanges::AcceptChanges => {
                    let mut inner = Vec::new();
                    self.depth += 1;
                    for c in el.elements() {
                        self.parse_inline_element(
                            c,
                            ns,
                            part_path,
                            rels,
                            false,
                            in_link,
                            &mut inner,
                            extra_blocks,
                            caption_blocks,
                        )?;
                    }
                    self.depth -= 1;
                    // Do not let standalone `w:instrText` inside a tracked change
                    // hijack an enclosing paragraph-level field instruction.
                    let has_fld_begin = inner.iter().any(|r| matches!(r, RawInline::FldBegin));
                    if has_fld_begin {
                        out.extend(inner);
                    } else {
                        out.extend(
                            inner
                                .into_iter()
                                .filter(|r| !matches!(r, RawInline::FldInstr(_))),
                        );
                    }
                }
                TrackChanges::AllChanges => {
                    let info = parse_change_info(el, ns);
                    let mut inner = Vec::new();
                    self.depth += 1;
                    for c in el.elements() {
                        self.parse_inline_element(
                            c,
                            ns,
                            part_path,
                            rels,
                            false,
                            in_link,
                            &mut inner,
                            extra_blocks,
                            caption_blocks,
                        )?;
                    }
                    self.depth -= 1;
                    out.push(RawInline::Tracked {
                        class: "insertion",
                        info,
                        items: inner,
                    });
                }
            }
        } else if ns.is_w(el, "del") || ns.is_w(el, "moveFrom") {
            match self.options.track_changes {
                TrackChanges::AcceptChanges => {}
                TrackChanges::RejectChanges => {
                    self.depth += 1;
                    for c in el.elements() {
                        self.parse_inline_element(
                            c,
                            ns,
                            part_path,
                            rels,
                            true,
                            in_link,
                            out,
                            extra_blocks,
                            caption_blocks,
                        )?;
                    }
                    self.depth -= 1;
                }
                TrackChanges::AllChanges => {
                    let info = parse_change_info(el, ns);
                    let mut inner = Vec::new();
                    self.depth += 1;
                    for c in el.elements() {
                        self.parse_inline_element(
                            c,
                            ns,
                            part_path,
                            rels,
                            true,
                            in_link,
                            &mut inner,
                            extra_blocks,
                            caption_blocks,
                        )?;
                    }
                    self.depth -= 1;
                    out.push(RawInline::Tracked {
                        class: "deletion",
                        info,
                        items: inner,
                    });
                }
            }
        } else if ns.is_w(el, "commentRangeStart") {
            if self.options.track_changes == TrackChanges::AllChanges
                && let Some(id) = ns.w_attr(el, "id")
                && let Some(comment) = self.comments.comments.get(id)
            {
                let author = comment.author.clone();
                let date = comment.date.clone();
                let comment_inlines = self.expand_comment(id)?;
                let mut kvs = vec![("comment-id".into(), Text::from(id))];
                if !author.is_empty() {
                    kvs.push(("author".into(), author));
                }
                if !date.is_empty() {
                    kvs.push(("date".into(), date));
                }
                out.push(RawInline::Inline(Inline::Span(
                    Attr::new("", vec!["comment-start".into()], kvs),
                    comment_inlines,
                )));
            }
        } else if ns.is_w(el, "commentRangeEnd") {
            if self.options.track_changes == TrackChanges::AllChanges
                && let Some(id) = ns.w_attr(el, "id")
                && self.comments.comments.contains_key(id)
            {
                out.push(RawInline::Inline(Inline::Span(
                    Attr::new(
                        "",
                        vec!["comment-end".into()],
                        vec![("comment-id".into(), Text::from(id))],
                    ),
                    Vec::new(),
                )));
            }
        } else if ns.is_w(el, "sdt") {
            let checkbox = ns
                .w_child(el, "sdtPr")
                .and_then(|pr| pr.elements().find(|c| is_w14(c, "checkbox")))
                .and_then(|cb| cb.elements().find(|c| is_w14(c, "checked")))
                .and_then(|ch| ch.attr_local("val"));
            if let Some(val) = checkbox {
                let sym = if val == "1" { "\u{2612}" } else { "\u{2610}" };
                out.push(RawInline::Run {
                    props: RunProps::default(),
                    items: vec![RunAtom::Text(sym.into())],
                });
                return Ok(());
            }
            if let Some(content) = ns.w_child(el, "sdtContent") {
                self.depth += 1;
                for c in content.elements() {
                    self.parse_inline_element(
                        c,
                        ns,
                        part_path,
                        rels,
                        in_deleted,
                        in_link,
                        out,
                        extra_blocks,
                        caption_blocks,
                    )?;
                }
                self.depth -= 1;
            }
        } else if ns.is_w(el, "smartTag") || ns.is_w(el, "customXml") {
            self.depth += 1;
            for c in el.elements() {
                self.parse_inline_element(
                    c,
                    ns,
                    part_path,
                    rels,
                    in_deleted,
                    in_link,
                    out,
                    extra_blocks,
                    caption_blocks,
                )?;
            }
            self.depth -= 1;
        } else if ns.is_m(el, "oMath") {
            let tex = omml_to_tex(el, ns, self.context.limits.max_nesting_depth);
            out.push(RawInline::Inline(Inline::Math(MathType::InlineMath, tex)));
        } else if ns.is_m(el, "oMathPara") {
            for sub in el.elements() {
                if ns.is_m(sub, "oMath") {
                    let tex = omml_to_tex(sub, ns, self.context.limits.max_nesting_depth);
                    out.push(RawInline::Inline(Inline::Math(MathType::DisplayMath, tex)));
                }
            }
        } else if is_mc(el, "AlternateContent")
            && let Some(chosen) = choose_alternate_content(el)
        {
            self.depth += 1;
            for c in chosen.elements() {
                self.parse_inline_element(
                    c,
                    ns,
                    part_path,
                    rels,
                    in_deleted,
                    in_link,
                    out,
                    extra_blocks,
                    caption_blocks,
                )?;
            }
            self.depth -= 1;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_run(
        &mut self,
        el: &XmlElement,
        ns: &DocxNs,
        part_path: &str,
        rels: &RelsMap,
        in_deleted: bool,
        in_link: bool,
        out: &mut Vec<RawInline>,
        extra_blocks: &mut Vec<Block>,
        caption_blocks: &mut Vec<Block>,
    ) -> Result<()> {
        let direct_r = ns
            .w_child(el, "rPr")
            .map(|p| RunProps::parse(p, ns))
            .unwrap_or_default();
        let props = self
            .styles
            .resolve_run(&direct_r, self.keep_styles(), in_link);

        let mut atoms = Vec::new();
        for child in el.elements() {
            if ns.is_w(child, "rPr") {
                continue;
            }
            if self.in_expanded_note_or_comment() {
                self.charge_expanded_node()?;
            }
            if ns.is_w(child, "t") || (in_deleted && ns.is_w(child, "delText")) {
                let raw_text = self.run_text(child)?;
                if !raw_text.is_empty() {
                    let decoded = decode_pua_text(&raw_text, props.font.as_deref());
                    atoms.push(RunAtom::Text(decoded));
                }
            } else if ns.is_w(child, "tab") {
                atoms.push(RunAtom::Tab);
            } else if ns.is_w(child, "br") || ns.is_w(child, "cr") {
                let br_type = ns.w_attr(child, "type").unwrap_or("textWrapping");
                if br_type != "page" && br_type != "column" {
                    atoms.push(RunAtom::LineBreak);
                }
            } else if ns.is_w(child, "softHyphen") {
                atoms.push(RunAtom::Text("\u{00AD}".into()));
            } else if ns.is_w(child, "noBreakHyphen") {
                atoms.push(RunAtom::Text("\u{2011}".into()));
            } else if ns.is_w(child, "sym") {
                let font = ns.w_attr(child, "font");
                if let Some(hex) = ns.w_attr(child, "char")
                    && let Some(decoded) = decode_symbol(font, hex)
                {
                    atoms.push(RunAtom::Text(decoded));
                }
            } else if ns.is_w(child, "fldChar") {
                flush_atoms(&props, &mut atoms, out);
                match ns.w_attr(child, "fldCharType") {
                    Some("begin") => out.push(RawInline::FldBegin),
                    Some("separate") => out.push(RawInline::FldSeparate),
                    Some("end") => out.push(RawInline::FldEnd),
                    _ => {}
                }
            } else if ns.is_w(child, "instrText")
                || (in_deleted
                    && self.options.track_changes == TrackChanges::RejectChanges
                    && ns.is_w(child, "delInstrText"))
            {
                flush_atoms(&props, &mut atoms, out);
                out.push(RawInline::FldInstr(self.run_text(child)?));
            } else if ns.is_w(child, "footnoteReference") {
                if let Some(id) = ns.w_attr(child, "id") {
                    flush_atoms(&props, &mut atoms, out);
                    let note = self.expand_footnote(id)?;
                    out.push(RawInline::Inline(note));
                }
            } else if ns.is_w(child, "endnoteReference") {
                if let Some(id) = ns.w_attr(child, "id") {
                    flush_atoms(&props, &mut atoms, out);
                    let note = self.expand_endnote(id)?;
                    out.push(RawInline::Inline(note));
                }
            } else if ns.is_w(child, "drawing") {
                self.parse_drawing(
                    child,
                    ns,
                    part_path,
                    rels,
                    &mut atoms,
                    extra_blocks,
                    caption_blocks,
                )?;
            } else if ns.is_w(child, "pict") || ns.is_w(child, "object") {
                self.parse_vml_pict(
                    child,
                    ns,
                    part_path,
                    rels,
                    &mut atoms,
                    extra_blocks,
                    caption_blocks,
                )?;
            } else if is_mc(child, "AlternateContent")
                && let Some(chosen) = choose_alternate_content(child)
            {
                for sub in chosen.elements() {
                    if ns.is_w(sub, "drawing") {
                        self.parse_drawing(
                            sub,
                            ns,
                            part_path,
                            rels,
                            &mut atoms,
                            extra_blocks,
                            caption_blocks,
                        )?;
                    } else if ns.is_w(sub, "pict") {
                        self.parse_vml_pict(
                            sub,
                            ns,
                            part_path,
                            rels,
                            &mut atoms,
                            extra_blocks,
                            caption_blocks,
                        )?;
                    }
                }
            }
        }
        flush_atoms(&props, &mut atoms, out);
        Ok(())
    }

    fn expand_footnote(&mut self, id: &str) -> Result<Inline> {
        self.charge_expanded_node()?;
        if self.active_footnotes.contains(id) {
            return Ok(Inline::Note(Vec::new()));
        }
        let footnotes = self.footnotes;
        let Some(note_el) = footnotes.notes.get(id) else {
            return Ok(Inline::Note(Vec::new()));
        };

        self.active_footnotes.insert(Text::from(id));
        let res = self.parse_body_children(
            note_el,
            &footnotes.ns,
            &footnotes.part_path,
            &footnotes.rels,
        );
        self.active_footnotes.remove(id);
        let items = res?;

        let mut blocks = self.build_blocks(items, false, false)?;
        trim_all_block_edges(&mut blocks);
        Ok(Inline::Note(blocks))
    }

    fn expand_endnote(&mut self, id: &str) -> Result<Inline> {
        self.charge_expanded_node()?;
        if self.active_endnotes.contains(id) {
            return Ok(Inline::Note(Vec::new()));
        }
        let endnotes = self.endnotes;
        let Some(note_el) = endnotes.notes.get(id) else {
            return Ok(Inline::Note(Vec::new()));
        };

        self.active_endnotes.insert(Text::from(id));
        let res =
            self.parse_body_children(note_el, &endnotes.ns, &endnotes.part_path, &endnotes.rels);
        self.active_endnotes.remove(id);
        let items = res?;

        let mut blocks = self.build_blocks(items, false, false)?;
        trim_all_block_edges(&mut blocks);
        Ok(Inline::Note(blocks))
    }

    fn expand_comment(&mut self, id: &str) -> Result<Vec<Inline>> {
        self.charge_expanded_node()?;
        if self.active_comments.contains(id) {
            return Ok(Vec::new());
        }
        let comments = self.comments;
        let Some(comment) = comments.comments.get(id) else {
            return Ok(Vec::new());
        };

        self.active_comments.insert(Text::from(id));
        let res = self.parse_body_children(
            &comment.element,
            &comments.ns,
            &comments.part_path,
            &comments.rels,
        );
        self.active_comments.remove(id);
        let items = res?;

        let mut blocks = self.build_blocks(items, false, true)?;
        trim_all_block_edges(&mut blocks);
        let mut out = Vec::new();
        for (idx, block) in blocks.into_iter().enumerate() {
            if idx > 0 && !out.is_empty() {
                out.push(Inline::LineBreak);
            }
            match block {
                Block::Para(inlines) | Block::Plain(inlines) => out.extend(inlines),
                _ => {}
            }
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_drawing(
        &mut self,
        drawing: &XmlElement,
        ns: &DocxNs,
        part_path: &str,
        rels: &RelsMap,
        atoms: &mut Vec<RunAtom>,
        extra_blocks: &mut Vec<Block>,
        caption_blocks: &mut Vec<Block>,
    ) -> Result<()> {
        for container in drawing.elements() {
            if !is_wp(container, "inline") && !is_wp(container, "anchor") {
                continue;
            }
            let (width, height) = container
                .elements()
                .find(|c| is_wp(c, "extent"))
                .map(|ext| {
                    let cx = ext
                        .attr_local("cx")
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|&v| v > 0.0)
                        .map(emu_to_inches);
                    let cy = ext
                        .attr_local("cy")
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|&v| v > 0.0)
                        .map(emu_to_inches);
                    (cx, cy)
                })
                .unwrap_or((None, None));

            let doc_pr = container.elements().find(|c| is_wp(c, "docPr"));
            let descr = doc_pr.and_then(|d| d.attr_local("descr")).unwrap_or("");
            let title = doc_pr.and_then(|d| d.attr_local("title")).unwrap_or("");
            let link_rid = doc_pr
                .and_then(|d| d.elements().find(|c| is_a(c, "hlinkClick")))
                .and_then(|h| ns.r_attr(h, "id"));

            let blip_rid = find_blip_rid(container, ns);
            if let Some(rid) = blip_rid
                && let Some(img) =
                    self.load_image(rid, part_path, rels, width, height, descr, title)?
            {
                let wrapped = if let Some(lrid) = link_rid
                    && let Some(rel) = rels.get(lrid)
                {
                    Inline::Link(
                        Attr::default(),
                        vec![img],
                        Target::new(rel.target.clone(), ""),
                    )
                } else {
                    img
                };
                atoms.push(RunAtom::Inline(wrapped));
                continue;
            }

            if let Some(txbx) = find_txbx_content(container, ns) {
                let items = self.parse_body_children(txbx, ns, part_path, rels)?;
                let is_cap = items.iter().any(|it| {
                    matches!(it, BodyItem::Para(p) if is_caption_style(&p.style_names) || p.has_seq_any)
                });
                let blocks = self.build_blocks(items, false, true)?;
                if is_cap {
                    caption_blocks.extend(blocks);
                } else {
                    extra_blocks.extend(blocks);
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_vml_pict(
        &mut self,
        pict: &XmlElement,
        ns: &DocxNs,
        part_path: &str,
        rels: &RelsMap,
        atoms: &mut Vec<RunAtom>,
        extra_blocks: &mut Vec<Block>,
        caption_blocks: &mut Vec<Block>,
    ) -> Result<()> {
        let mut stack = vec![pict];
        while let Some(el) = stack.pop() {
            if is_v(el, "imagedata") {
                if let Some(rid) = ns.r_attr(el, "id") {
                    let title = el.attr_local("title").unwrap_or("");
                    if let Some(img) =
                        self.load_image(rid, part_path, rels, None, None, "", title)?
                    {
                        atoms.push(RunAtom::Inline(img));
                    }
                }
                continue;
            }
            if ns.is_w(el, "txbxContent") {
                let items = self.parse_body_children(el, ns, part_path, rels)?;
                let is_cap = items.iter().any(|it| {
                    matches!(it, BodyItem::Para(p) if is_caption_style(&p.style_names) || p.has_seq_any)
                });
                let blocks = self.build_blocks(items, false, true)?;
                if is_cap {
                    caption_blocks.extend(blocks);
                } else {
                    extra_blocks.extend(blocks);
                }
                continue;
            }
            for child in el.elements().collect::<Vec<_>>().into_iter().rev() {
                stack.push(child);
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn load_image(
        &mut self,
        rid: &str,
        part_path: &str,
        rels: &RelsMap,
        width: Option<String>,
        height: Option<String>,
        descr: &str,
        title: &str,
    ) -> Result<Option<Inline>> {
        let Some(rel) = rels.get(rid) else {
            return Ok(None);
        };
        let mut kvs = Vec::new();
        if let Some(w) = width {
            kvs.push(("width".into(), w));
        }
        if let Some(h) = height {
            kvs.push(("height".into(), h));
        }
        let alt = tokenize_plain_inlines(descr);

        if rel.external {
            return Ok(Some(Inline::Image(
                Attr::new("", vec![], kvs),
                alt,
                Target::new(rel.target.clone(), title),
            )));
        }

        let Some(resolved) = resolve_target(part_path, &rel.target) else {
            return Ok(None);
        };
        if let Some(cached) = self.media_cache.get(&resolved) {
            return Ok(cached.as_ref().map(|key| {
                Inline::Image(
                    Attr::new("", vec![], kvs),
                    alt,
                    Target::new(key.clone(), title),
                )
            }));
        }
        let bytes = match self.archive.read(&resolved) {
            Ok(Some(bytes)) => bytes,
            Ok(None) | Err(ArchiveError::Corrupt { .. }) => {
                self.media_cache.insert(resolved, None);
                return Ok(None);
            }
            Err(err) => {
                self.media_cache.insert(resolved, None);
                return Err(map_read_error(err));
            }
        };
        let (_, mime) = image_size_and_mime(&bytes, Some(&resolved));
        let key = media_name(&resolved, "word/", &bytes);
        let mime_type = Text::from(mime.unwrap_or("application/octet-stream"));
        self.context.media.insert(
            key.clone(),
            MediaItem {
                mime_type,
                contents: bytes,
            },
        );
        self.media_cache.insert(resolved, Some(key.clone()));
        Ok(Some(Inline::Image(
            Attr::new("", vec![], kvs),
            alt,
            Target::new(key, title),
        )))
    }

    // -----------------------------------------------------------------------
    // Fields state machine (`begin` / `instrText` / `separate` / `end`)
    // -----------------------------------------------------------------------

    fn resolve_fields(&mut self, items: Vec<RawInline>) -> Result<(Vec<RawInline>, SeqInfo)> {
        let mut seq = SeqInfo::default();
        let resolved = self.resolve_fields_inner(items, &mut seq, 0)?;
        Ok((resolved, seq))
    }

    fn resolve_fields_inner(
        &mut self,
        items: Vec<RawInline>,
        seq: &mut SeqInfo,
        field_depth: usize,
    ) -> Result<Vec<RawInline>> {
        let max_depth = self.context.limits.max_nesting_depth;
        if field_depth > max_depth {
            return Err(Error::Parse(format!(
                "DOCX field nesting exceeded limit of {max_depth} levels"
            )));
        }
        let mut out: Vec<RawInline> = Vec::new();
        let mut stack: Vec<ActiveFieldSlot> = Vec::new();

        for item in items {
            match item {
                RawInline::FldBegin => {
                    if stack.len() >= max_depth {
                        return Err(Error::Parse(format!(
                            "DOCX field nesting exceeded limit of {max_depth} levels"
                        )));
                    }
                    stack.push(ActiveFieldSlot {
                        instr: String::new(),
                        in_result: false,
                        result: Vec::new(),
                    });
                }
                RawInline::FldSeparate => {
                    if let Some(top) = stack.last_mut() {
                        top.in_result = true;
                    }
                }
                RawInline::FldInstr(text) => {
                    if let Some(top) = stack.last_mut()
                        && !top.in_result
                    {
                        top.instr.push_str(&text);
                    }
                }
                RawInline::FldEnd => {
                    if let Some(field) = stack.pop() {
                        let produced = self.finish_field(field.instr, field.result, seq);
                        if let Some(parent) = stack.last_mut() {
                            if parent.in_result {
                                parent.result.extend(produced);
                            }
                        } else {
                            out.extend(produced);
                        }
                    } else {
                        // Closing a field that started in an earlier paragraph (e.g. ZOTERO_BIBL).
                        seq.is_bibl_end = true;
                    }
                }
                RawInline::Tracked { class, info, items } => {
                    let inner = self.resolve_fields_inner(items, seq, field_depth + 1)?;
                    let wrapped = RawInline::Tracked {
                        class,
                        info,
                        items: inner,
                    };
                    push_field_item(&mut stack, &mut out, wrapped);
                }
                RawInline::Link {
                    target,
                    title,
                    items,
                } => {
                    let inner = self.resolve_fields_inner(items, seq, field_depth + 1)?;
                    let wrapped = RawInline::Link {
                        target,
                        title,
                        items: inner,
                    };
                    push_field_item(&mut stack, &mut out, wrapped);
                }
                other => {
                    push_field_item(&mut stack, &mut out, other);
                }
            }
        }
        for field in stack {
            let upper = field.instr.trim().to_ascii_uppercase();
            if self.citations_enabled()
                && (upper.starts_with("ADDIN ZOTERO_BIBL")
                    || upper.starts_with("ADDIN MENDELEY BIBLIOGRAPHY")
                    || upper.contains("CSL_BIBLIOGRAPHY"))
            {
                seq.is_bibl_start = true;
            } else {
                out.extend(field.result);
            }
        }
        Ok(out)
    }

    fn finish_field(
        &mut self,
        instr: String,
        result: Vec<RawInline>,
        seq: &mut SeqInfo,
    ) -> Vec<RawInline> {
        let trimmed = instr.trim();
        let upper = trimmed.to_ascii_uppercase();

        if upper.starts_with("SEQ ") {
            seq.has_any = true;
            let rest = trimmed[4..].trim_start();
            let ident = rest.split_whitespace().next().unwrap_or("");
            if ident.eq_ignore_ascii_case("Table") {
                seq.has_table = true;
            }
            return result;
        }

        if upper.starts_with("XE ") {
            let args = split_field_args(trimmed[3..].trim());
            if let Some(entry) = args.first() {
                return vec![RawInline::Inline(Inline::Span(
                    Attr::new(
                        "",
                        vec!["indexref".into()],
                        vec![("entry".into(), entry.clone())],
                    ),
                    Vec::new(),
                ))];
            }
            return Vec::new();
        }

        if upper.starts_with("HYPERLINK ") {
            if let Some((target, title)) = parse_hyperlink_instr(trimmed) {
                let stripped = strip_hyperlink_underline(result);
                return vec![RawInline::Link {
                    target,
                    title,
                    items: stripped,
                }];
            }
            return result;
        }

        if (upper.starts_with("PAGEREF ") || upper.starts_with("REF "))
            && let Some(anchor) = parse_ref_instr(trimmed)
        {
            return vec![RawInline::Link {
                target: format!("#{anchor}"),
                title: Text::new(),
                items: result,
            }];
        }

        if self.citations_enabled() && upper.starts_with("ADDIN ") {
            if upper.starts_with("ADDIN ZOTERO_BIBL")
                || upper.starts_with("ADDIN MENDELEY BIBLIOGRAPHY")
                || upper.contains("CSL_BIBLIOGRAPHY")
            {
                return Vec::new();
            }
            if let Some(cite) = self.parse_csl_citation(trimmed, result.clone()) {
                return vec![RawInline::Inline(cite)];
            }
        }

        result
    }

    fn parse_csl_citation(&mut self, instr: &str, result: Vec<RawInline>) -> Option<Inline> {
        let json_start = instr.find('{')?;
        let val: serde_json::Value = serde_json::from_str(&instr[json_start..]).ok()?;
        let items = val.get("citationItems")?.as_array()?;
        let mut citations = Vec::new();
        for item in items {
            let id = item
                .get("id")
                .and_then(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .or_else(|| v.as_i64().map(|n| n.to_string()))
                })
                .or_else(|| {
                    item.get("itemData")
                        .and_then(|d| d.get("id"))
                        .and_then(|v| {
                            v.as_str()
                                .map(str::to_owned)
                                .or_else(|| v.as_i64().map(|n| n.to_string()))
                        })
                })
                .unwrap_or_default();
            if id.is_empty() {
                continue;
            }
            // Record `itemData` into `meta["references"]` using `randoc_citeproc::csljson`
            // for exact Pandoc CSL-JSON -> MetaValue conversion.
            if let Some(item_data) = item.get("itemData") {
                let mut obj = item_data.clone();
                if let Some(map) = obj.as_object_mut() {
                    // Pandoc keys each reference by the citationItem's `id`.
                    map.insert("id".to_owned(), serde_json::Value::String(id.clone()));
                }
                let arr_json = serde_json::Value::Array(vec![obj]).to_string();
                if let Ok(refs) = randoc_citeproc::csljson::read_references(
                    &arr_json,
                    self.context.limits.max_nesting_depth,
                ) {
                    for r in refs {
                        let ref_id = r.id.clone();
                        self.references
                            .insert(ref_id, randoc_citeproc::csljson::reference_to_meta(&r));
                    }
                }
            }
            let prefix = item
                .get("prefix")
                .and_then(|v| v.as_str())
                .map(|s| normalize_inline_list(tokenize_text_preserving_edges(s)))
                .unwrap_or_default();
            let mut suffix_inlines = Vec::new();
            if let Some(locator) = item.get("locator").and_then(|v| v.as_str())
                && !locator.is_empty()
            {
                suffix_inlines.push(Inline::Str(",".into()));
                suffix_inlines.push(Inline::Space);
                if let Some(label) = item.get("label").and_then(|v| v.as_str())
                    && !label.is_empty()
                {
                    suffix_inlines.extend(tokenize_text_preserving_edges(label));
                    suffix_inlines.push(Inline::Space);
                }
                suffix_inlines.extend(tokenize_text_preserving_edges(locator));
                suffix_inlines.push(Inline::Space);
            }
            if let Some(suf) = item.get("suffix").and_then(|v| v.as_str())
                && !suf.is_empty()
            {
                suffix_inlines.extend(tokenize_text_preserving_edges(suf));
            }
            let suffix = normalize_inline_list(suffix_inlines);
            let mode = if item
                .get("suppress-author")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                CitationMode::SuppressAuthor
            } else {
                CitationMode::NormalCitation
            };
            citations.push(Citation {
                id,
                prefix,
                suffix,
                mode,
                note_num: 0,
                hash: 0,
            });
        }
        if citations.is_empty() {
            return None;
        }
        let display_inlines = self.render_raw_inlines(result);
        Some(Inline::Cite(citations, display_inlines))
    }

    // -----------------------------------------------------------------------
    // RawInline -> Pandoc Inline rendering + normalization
    // -----------------------------------------------------------------------

    fn render_raw_inlines(&self, items: Vec<RawInline>) -> Vec<Inline> {
        let mut merged: Vec<RawInline> = Vec::with_capacity(items.len());
        for item in items {
            match (merged.last_mut(), item) {
                (
                    Some(RawInline::Run {
                        props: prev_props,
                        items: prev_items,
                    }),
                    RawInline::Run {
                        props,
                        items: next_items,
                    },
                ) if *prev_props == props => {
                    prev_items.extend(next_items);
                }
                (_, item) => merged.push(item),
            }
        }

        let mut out = Vec::new();
        for item in merged {
            match item {
                RawInline::Run { props, items } => {
                    out.extend(self.render_run_keeping_spaces(&props, items));
                }
                RawInline::Inline(inl) => out.push(inl),
                RawInline::Tracked { class, info, items } => {
                    let inner = self.render_raw_inlines(items);
                    out.push(make_change_span(class, &info, inner));
                }
                RawInline::Link {
                    target,
                    title,
                    items,
                } => {
                    let inner = self.render_raw_inlines(items);
                    emit_wrapped_moving_spaces(inner, &mut out, |core| {
                        Inline::Link(
                            Attr::default(),
                            core,
                            Target::new(target.clone(), title.clone()),
                        )
                    });
                }
                RawInline::FldBegin
                | RawInline::FldSeparate
                | RawInline::FldEnd
                | RawInline::FldInstr(_) => {}
            }
        }
        let merged_fmt = normalize_inline_list(out);
        lift_edge_spaces_out_of_formatting(merged_fmt)
    }

    fn render_run_keeping_spaces(&self, props: &RunProps, atoms: Vec<RunAtom>) -> Vec<Inline> {
        let is_code = props.style_names.iter().any(|n| is_code_char_style(n));
        if is_code {
            let mut text = String::new();
            let mut trailing = Vec::new();
            for atom in atoms {
                match atom {
                    RunAtom::Text(t) => text.push_str(&t),
                    RunAtom::Tab => text.push('\t'),
                    RunAtom::LineBreak => text.push('\n'),
                    RunAtom::Inline(inl) => trailing.push(inl),
                }
            }
            let mut res = Vec::new();
            if !text.is_empty() {
                res.push(Inline::Code(Attr::default(), text));
            }
            res.extend(trailing);
            match props.vert_align {
                Some(VertAlign::Superscript) => res = vec![Inline::Superscript(res)],
                Some(VertAlign::Subscript) => res = vec![Inline::Subscript(res)],
                _ => {}
            }
            if props.highlight == Some(true) {
                res = vec![Inline::Span(
                    Attr::new("", vec!["mark".into()], vec![]),
                    res,
                )];
            }
            return res;
        }

        let mut inlines = Vec::new();
        for atom in atoms {
            match atom {
                RunAtom::Text(t) => inlines.extend(tokenize_text_preserving_edges(&t)),
                RunAtom::Tab => inlines.push(Inline::Space),
                RunAtom::LineBreak => inlines.push(Inline::LineBreak),
                RunAtom::Inline(inl) => inlines.push(inl),
            }
        }
        inlines = normalize_inline_list(inlines);
        if inlines.is_empty() {
            return inlines;
        }

        // Exact Pandoc 3.13 nesting order (innermost -> outermost), keeping spaces
        // inside so adjacent runs sharing an outer wrapper merge cleanly before
        // edge spaces are lifted out.
        if props.underline == Some(true) {
            inlines = vec![Inline::Underline(inlines)];
        }
        match props.vert_align {
            Some(VertAlign::Superscript) => inlines = vec![Inline::Superscript(inlines)],
            Some(VertAlign::Subscript) => inlines = vec![Inline::Subscript(inlines)],
            _ => {}
        }
        if props.is_strikeout() {
            inlines = vec![Inline::Strikeout(inlines)];
        }
        if props.small_caps == Some(true) {
            inlines = vec![Inline::SmallCaps(inlines)];
        }
        if props.highlight == Some(true) {
            inlines = vec![Inline::Span(
                Attr::new("", vec!["mark".into()], vec![]),
                inlines,
            )];
        }
        if props.bold == Some(true) {
            inlines = vec![Inline::Strong(inlines)];
        }
        if props.italic == Some(true) {
            inlines = vec![Inline::Emph(inlines)];
        }
        if self.keep_styles()
            && let Some(style_name) = &props.style_name
            && !is_default_char_style(style_name)
        {
            inlines = vec![Inline::Span(
                Attr::new(
                    "",
                    vec![],
                    vec![("custom-style".into(), style_name.clone())],
                ),
                inlines,
            )];
        }

        inlines
    }

    // -----------------------------------------------------------------------
    // Tables (`w:tbl`)
    // -----------------------------------------------------------------------

    fn parse_table(
        &mut self,
        tbl: &XmlElement,
        ns: &DocxNs,
        part_path: &str,
        rels: &RelsMap,
    ) -> Result<Block> {
        let tbl_pr = ns.w_child(tbl, "tblPr");
        let tbl_look_first_row = tbl_pr
            .and_then(|p| ns.w_child(p, "tblLook"))
            .map(|look| {
                if let Some(fr) = ns.w_attr(look, "firstRow") {
                    fr == "1" || fr.eq_ignore_ascii_case("true")
                } else if let Some(hex) = ns.w_attr(look, "val") {
                    u32::from_str_radix(hex, 16)
                        .map(|bits| (bits & 0x0020) != 0)
                        .unwrap_or(false)
                } else {
                    false
                }
            })
            .unwrap_or(false);

        let mut grid_widths: Vec<f64> = Vec::new();
        if let Some(grid) = ns.w_child(tbl, "tblGrid") {
            let max_cells = self.context.limits.max_table_cells;
            for col in ns.w_children(grid, "gridCol") {
                if grid_widths.len() >= max_cells {
                    return Err(Error::Parse(format!(
                        "DOCX table column count exceeded limit of {max_cells}"
                    )));
                }
                let w = ns
                    .w_attr(col, "w")
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                grid_widths.push(w);
            }
        }

        struct RawCell {
            blocks: Vec<Block>,
            col_span: usize,
            v_merge: VMerge,
            align: Alignment,
        }
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum VMerge {
            None,
            Restart,
            Continue,
        }
        struct RawRow {
            is_header: bool,
            cells: Vec<RawCell>,
        }

        let mut raw_rows: Vec<RawRow> = Vec::new();
        let mut max_cols = grid_widths.len();
        let mut table_explicit_cells = 0usize;

        let tr_elems = collect_table_children(tbl, ns, "tr");
        for tr in tr_elems {
            let tr_pr = ns.w_child(tr, "trPr");
            let explicit_hdr = tr_pr
                .and_then(|p| ns.w_child(p, "tblHeader"))
                .is_some_and(|h| ns.toggle_val(h));
            let grid_before = tr_pr
                .and_then(|p| ns.w_child(p, "gridBefore"))
                .and_then(|g| ns.w_attr(g, "val"))
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let grid_after = tr_pr
                .and_then(|p| ns.w_child(p, "gridAfter"))
                .and_then(|g| ns.w_attr(g, "val"))
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);

            let mut cells = Vec::new();
            let mut row_cols = 0usize;

            if grid_before > 0 {
                self.charge_table_cells(grid_before)?;
                table_explicit_cells = table_explicit_cells.saturating_add(grid_before);
                for _ in 0..grid_before {
                    cells.push(RawCell {
                        blocks: Vec::new(),
                        col_span: 1,
                        v_merge: VMerge::None,
                        align: Alignment::AlignDefault,
                    });
                }
                row_cols = row_cols.saturating_add(grid_before);
            }

            let tc_elems = collect_table_children(tr, ns, "tc");
            for tc in tc_elems {
                let tc_pr = ns.w_child(tc, "tcPr");
                let col_span = tc_pr
                    .and_then(|p| ns.w_child(p, "gridSpan"))
                    .and_then(|g| ns.w_attr(g, "val"))
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(1)
                    .max(1);

                self.charge_table_cells(col_span)?;
                table_explicit_cells = table_explicit_cells.saturating_add(col_span);

                let v_merge = tc_pr
                    .and_then(|p| ns.w_child(p, "vMerge"))
                    .map(|vm| match ns.w_attr(vm, "val") {
                        Some("restart") => VMerge::Restart,
                        _ => VMerge::Continue,
                    })
                    .unwrap_or(VMerge::None);

                let items = self.parse_body_children(tc, ns, part_path, rels)?;
                let cell_align = infer_cell_alignment(&items);
                let blocks = self.build_blocks(items, false, true)?;

                row_cols = row_cols.saturating_add(col_span);
                cells.push(RawCell {
                    blocks,
                    col_span,
                    v_merge,
                    align: cell_align,
                });
            }

            if grid_after > 0 {
                self.charge_table_cells(grid_after)?;
                table_explicit_cells = table_explicit_cells.saturating_add(grid_after);
                for _ in 0..grid_after {
                    cells.push(RawCell {
                        blocks: Vec::new(),
                        col_span: 1,
                        v_merge: VMerge::None,
                        align: Alignment::AlignDefault,
                    });
                }
                row_cols = row_cols.saturating_add(grid_after);
            }

            max_cols = max_cols.max(row_cols);
            raw_rows.push(RawRow {
                is_header: explicit_hdr,
                cells,
            });
        }

        // Convert single-Para cells to Plain ONLY when every cell in the entire
        // table has at most one simple paragraph (oracle-verified).
        let all_simple_cells = raw_rows.iter().all(|r| {
            r.cells.iter().all(|c| {
                c.blocks.is_empty()
                    || (c.blocks.len() == 1
                        && matches!(c.blocks[0], Block::Para(_) | Block::Plain(_)))
            })
        });
        if all_simple_cells {
            for row in &mut raw_rows {
                for cell in &mut row.cells {
                    if let Some(Block::Para(inlines)) = cell.blocks.first_mut() {
                        let taken = std::mem::take(inlines);
                        cell.blocks[0] = Block::Plain(taken);
                    }
                }
            }
        }

        let any_explicit_header = raw_rows.iter().any(|r| r.is_header);
        if !any_explicit_header && tbl_look_first_row && !raw_rows.is_empty() {
            raw_rows[0].is_header = true;
        }

        let num_rows = raw_rows.len();
        let grid_slots = num_rows.saturating_mul(max_cols);
        let implicit_grid_cells = grid_slots.saturating_sub(table_explicit_cells);
        if implicit_grid_cells > 0 {
            self.charge_table_cells(implicit_grid_cells)?;
        }
        let mut grid_map: Vec<Vec<Option<usize>>> = vec![vec![None; max_cols]; num_rows];
        let mut row_spans: Vec<Vec<usize>> = raw_rows
            .iter()
            .map(|r| vec![1usize; r.cells.len()])
            .collect();
        let mut skip_cell: Vec<Vec<bool>> = raw_rows
            .iter()
            .map(|r| vec![false; r.cells.len()])
            .collect();

        for (r_idx, row) in raw_rows.iter().enumerate() {
            let mut col = 0usize;
            for (c_idx, cell) in row.cells.iter().enumerate() {
                for offset in 0..cell.col_span {
                    if col + offset < max_cols {
                        grid_map[r_idx][col + offset] = Some(c_idx);
                    }
                }
                col += cell.col_span;
            }
        }

        // Determine header row count; extend header if a header cell's vMerge continues into the next row.
        let mut header_count = raw_rows.iter().take_while(|r| r.is_header).count();
        while header_count > 0 && header_count < num_rows {
            let next_continues = raw_rows[header_count]
                .cells
                .iter()
                .any(|c| c.v_merge == VMerge::Continue);
            if next_continues {
                header_count += 1;
            } else {
                break;
            }
        }

        for &(start_r, end_r) in &[(0usize, header_count), (header_count, num_rows)] {
            for c_pos in 0..max_cols {
                let mut anchor: Option<(usize, usize)> = None;
                for r_idx in start_r..end_r {
                    let Some(c_idx) = grid_map[r_idx][c_pos] else {
                        anchor = None;
                        continue;
                    };
                    if c_pos > 0 && grid_map[r_idx][c_pos - 1] == Some(c_idx) {
                        continue;
                    }
                    match raw_rows[r_idx].cells[c_idx].v_merge {
                        VMerge::Restart => {
                            anchor = Some((r_idx, c_idx));
                        }
                        VMerge::Continue => {
                            if let Some((ar, ac)) = anchor {
                                row_spans[ar][ac] += 1;
                                skip_cell[r_idx][c_idx] = true;
                            }
                        }
                        VMerge::None => {
                            anchor = None;
                        }
                    }
                }
            }
        }

        // Column alignments come from the first body row (if any), else AlignDefault;
        // each Cell preserves its own alignment.
        let mut col_aligns = vec![Alignment::AlignDefault; max_cols];
        if header_count < num_rows {
            for (c_pos, slot) in col_aligns.iter_mut().enumerate() {
                if let Some(c_idx) = grid_map[header_count][c_pos] {
                    *slot = raw_rows[header_count].cells[c_idx].align;
                }
            }
        }

        // Exact Pandoc 3.13 ColWidth formula:
        // fracs = [w / (page_width - 10*(n-1))], normalized by sum(fracs) if sum(fracs) > 1.0.
        let col_widths: Vec<ColWidth> = if grid_widths.len() == max_cols
            && !grid_widths.is_empty()
            && grid_widths.iter().all(|&w| w > 0.0)
        {
            let base = (self.page_width - 10.0 * ((max_cols.saturating_sub(1)) as f64)).max(1.0);
            let mut fracs: Vec<f64> = grid_widths.iter().map(|&w| w / base).collect();
            let total: f64 = fracs.iter().sum();
            if total > 1.0 {
                for f in &mut fracs {
                    *f /= total;
                }
            }
            fracs.into_iter().map(ColWidth::ColWidth).collect()
        } else {
            vec![ColWidth::ColWidthDefault; max_cols]
        };

        let col_specs: Vec<ColSpec> = col_aligns
            .into_iter()
            .zip(col_widths)
            .map(|(align, width)| ColSpec { align, width })
            .collect();

        // Check if any cell in the table has a list; in that case Pandoc keeps Para in cells.
        let mut head_rows = Vec::new();
        let mut body_rows = Vec::new();
        for (r_idx, row) in raw_rows.into_iter().enumerate() {
            let mut out_cells = Vec::new();
            for (c_idx, cell) in row.cells.into_iter().enumerate() {
                if skip_cell[r_idx][c_idx] {
                    continue;
                }
                out_cells.push(Cell {
                    attr: Attr::default(),
                    align: cell.align,
                    row_span: row_spans[r_idx][c_idx] as i64,
                    col_span: cell.col_span as i64,
                    content: cell.blocks,
                });
            }
            let ast_row = Row {
                attr: Attr::default(),
                cells: out_cells,
            };
            if r_idx < header_count {
                head_rows.push(ast_row);
            } else {
                body_rows.push(ast_row);
            }
        }

        let head = TableHead {
            attr: Attr::default(),
            rows: head_rows,
        };
        let bodies = vec![TableBody {
            attr: Attr::default(),
            row_head_columns: 0,
            head: Vec::new(),
            body: body_rows,
        }];

        Ok(Block::Table(Box::new(Table {
            attr: Attr::default(),
            caption: Caption::default(),
            col_specs,
            head,
            bodies,
            foot: TableFoot::default(),
        })))
    }

    fn charge_table_cells(&mut self, count: usize) -> Result<()> {
        self.table_cells_used = self.table_cells_used.saturating_add(count);
        if self.table_cells_used > self.context.limits.max_table_cells {
            return Err(Error::Parse(format!(
                "DOCX table cell count exceeded limit of {}",
                self.context.limits.max_table_cells
            )));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Block assembly: paragraph-mark merging, dropcaps, code blocks, blockquotes, lists, captions
    // -----------------------------------------------------------------------

    fn build_blocks(
        &self,
        items: Vec<BodyItem>,
        allow_meta: bool,
        allow_lists: bool,
    ) -> Result<Vec<Block>> {
        let merged = self.merge_tracked_and_dropcap_paragraphs(items);
        let mut blocks = Vec::new();
        let mut list_counters: HashMap<Text, BTreeMap<i64, i64>> = HashMap::new();
        let mut in_bibl = false;
        let mut idx = 0usize;

        while idx < merged.len() {
            if in_bibl {
                if let BodyItem::Para(p) = &merged[idx]
                    && p.is_bibl_end
                {
                    in_bibl = false;
                }
                idx += 1;
                continue;
            }
            match &merged[idx] {
                BodyItem::Para(p) if p.is_bibl_start => {
                    in_bibl = !p.is_bibl_end;
                    idx += 1;
                    continue;
                }
                BodyItem::Table(tbl) => {
                    let mut tbl = tbl.clone();
                    // Check for trailing caption paragraph immediately after table.
                    if let Some(BodyItem::Para(next_p)) = merged.get(idx + 1)
                        && (is_caption_style(&next_p.style_names) || next_p.has_seq_table)
                        && !next_p.inlines.is_empty()
                    {
                        if let Block::Table(table) = &mut tbl {
                            table.caption.long = vec![Block::Para(next_p.inlines.clone())];
                        }
                        blocks.push(tbl);
                        blocks.extend(next_p.extra_blocks.clone());
                        idx += 2;
                        continue;
                    }
                    blocks.push(tbl);
                    idx += 1;
                }
                BodyItem::Para(p) => {
                    let extra = p.extra_blocks.clone();

                    // 1. Table caption immediately before a table.
                    if (is_caption_style(&p.style_names) || p.has_seq_table)
                        && let Some(BodyItem::Table(mut tbl)) = merged.get(idx + 1).cloned()
                    {
                        if let Block::Table(table) = &mut tbl {
                            table.caption.long = vec![Block::Para(p.inlines.clone())];
                        }
                        blocks.push(tbl);
                        blocks.extend(extra);
                        idx += 2;
                        continue;
                    }

                    // 2. Image with caption (either in textbox in same paragraph or in next paragraph) -> Figure.
                    if !p.caption_blocks.is_empty()
                        && let Some(fig) = try_make_figure(&p.inlines, &p.caption_blocks)
                    {
                        blocks.push(fig);
                        blocks.extend(extra);
                        idx += 1;
                        continue;
                    }
                    if is_image_only_inlines(&p.inlines)
                        && let Some(BodyItem::Para(next_p)) = merged.get(idx + 1)
                        && is_caption_style(&next_p.style_names)
                        && !next_p.inlines.is_empty()
                        && let Some(fig) =
                            try_make_figure(&p.inlines, &[Block::Para(next_p.inlines.clone())])
                    {
                        blocks.extend(extra);
                        blocks.push(fig);
                        blocks.extend(next_p.extra_blocks.clone());
                        idx += 2;
                        continue;
                    }

                    // 3. Code blocks (`Source Code`), including empty Source Code lines.
                    if is_source_code_style(&p.style_names) {
                        if self.keep_styles() {
                            let code = stringify(&p.inlines);
                            let name = primary_style_name(&p.style_names).unwrap_or("Source Code");
                            blocks.push(Block::Div(
                                Attr::new("", vec![], vec![("custom-style".into(), name.into())]),
                                vec![Block::CodeBlock(Attr::default(), code)],
                            ));
                            blocks.extend(extra);
                            idx += 1;
                        } else {
                            let mut lines = vec![stringify(&p.inlines)];
                            let mut end = idx + 1;
                            while let Some(BodyItem::Para(next_p)) = merged.get(end) {
                                if is_source_code_style(&next_p.style_names) {
                                    lines.push(stringify(&next_p.inlines));
                                    end += 1;
                                } else {
                                    break;
                                }
                            }
                            // Trim trailing empty lines if any.
                            let joined = lines.join("\n");
                            if !joined.is_empty() || self.empty_paragraphs() {
                                blocks.push(Block::CodeBlock(Attr::default(), joined));
                            }
                            blocks.extend(extra);
                            idx = end;
                        }
                        continue;
                    }

                    // 4. Empty paragraph check (unless +empty_paragraphs or extra blocks).
                    let effective_inlines = trim_para_edges(p.inlines.clone());
                    if effective_inlines.is_empty() {
                        blocks.extend(extra);
                        if self.empty_paragraphs() {
                            blocks.push(Block::Para(Vec::new()));
                        }
                        idx += 1;
                        continue;
                    }

                    // 5. Metadata styles (Title, Subtitle, Author, Date, Abstract).
                    if allow_meta && let Some(meta_key) = meta_style_key(&p.style_names) {
                        blocks.push(Block::Div(
                            Attr::new(
                                "",
                                vec!["__docx_meta".into()],
                                vec![("key".into(), meta_key.into())],
                            ),
                            vec![Block::Para(p.inlines.clone())],
                        ));
                        blocks.extend(extra);
                        idx += 1;
                        continue;
                    }

                    // 6. Headings (`Heading 1..9`, or `outlineLvl` 0..=8).
                    if let Some((level, custom_class)) =
                        heading_info(&p.style_names, p.props.outline_lvl)
                    {
                        let classes = custom_class.into_iter().collect();
                        blocks.push(Block::Header(
                            level,
                            Attr::new("", classes, vec![]),
                            p.inlines.clone(),
                        ));
                        blocks.extend(extra);
                        idx += 1;
                        continue;
                    }

                    // 7. Definition lists (`Definition Term` + `Definition`).
                    if is_def_term_style(&p.style_names) {
                        let (dl, next_idx) = self.consume_definition_list(&merged, idx);
                        blocks.push(dl);
                        idx = next_idx;
                        continue;
                    }

                    // 8. List items (`numPr` resolving to a valid list in `numbering.xml`).
                    if allow_lists && let Some((num_id, ilvl, kind)) = self.para_list_info(p) {
                        let (list_block, next_idx) = self.consume_list(
                            &merged,
                            idx,
                            &num_id,
                            ilvl,
                            kind,
                            &mut list_counters,
                            0,
                        )?;
                        blocks.push(list_block);
                        idx = next_idx;
                        continue;
                    }

                    // 9. BlockQuotes (`Block Text`, `Quote`, or relative direct left indent).
                    if is_blockquote_para(p) {
                        let mut quote_paras = vec![Block::Para(p.inlines.clone())];
                        let style_name = primary_style_name(&p.style_names).map(str::to_owned);
                        let explicit = p.props.has_explicit_style;
                        let mut end = idx + 1;
                        if !self.keep_styles() {
                            while let Some(BodyItem::Para(next_p)) = merged.get(end) {
                                if is_blockquote_para(next_p) && !next_p.inlines.is_empty() {
                                    quote_paras.push(Block::Para(next_p.inlines.clone()));
                                    end += 1;
                                } else {
                                    break;
                                }
                            }
                        }
                        let bq = Block::BlockQuote(quote_paras);
                        if self.keep_styles()
                            && explicit
                            && let Some(sn) = style_name
                            && !is_compact_style_name(&sn)
                        {
                            blocks.push(Block::Div(
                                Attr::new("", vec![], vec![("custom-style".into(), sn)]),
                                vec![bq],
                            ));
                        } else {
                            blocks.push(bq);
                        }
                        blocks.extend(extra);
                        idx = end;
                        continue;
                    }

                    // 10. Regular paragraph (`Compact` -> `Plain`, else `Para`; under `+styles` wrap explicit styles).
                    blocks.extend(extra);
                    let is_compact = is_compact_style(&p.style_names);
                    let para = if is_compact {
                        Block::Plain(p.inlines.clone())
                    } else {
                        Block::Para(p.inlines.clone())
                    };
                    if self.keep_styles()
                        && p.props.has_explicit_style
                        && !is_compact
                        && let Some(sn) = primary_style_name(&p.style_names)
                    {
                        blocks.push(Block::Div(
                            Attr::new("", vec![], vec![("custom-style".into(), sn.into())]),
                            vec![para],
                        ));
                    } else {
                        blocks.push(para);
                    }
                    idx += 1;
                }
            }
        }
        Ok(blocks)
    }

    /// Merges adjacent paragraphs for tracked paragraph-mark changes (with a Space)
    /// and drop-cap frames (without a Space), in O(n).
    fn merge_tracked_and_dropcap_paragraphs(&self, items: Vec<BodyItem>) -> Vec<BodyItem> {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum MergeMode {
            None,
            WithSpace,
            Direct,
        }
        let mut out: Vec<BodyItem> = Vec::with_capacity(items.len());
        let mut mode = MergeMode::None;

        for item in items {
            match item {
                BodyItem::Para(mut p) => {
                    let next_mode = if p.props.drop_cap {
                        MergeMode::Direct
                    } else {
                        match (&self.options.track_changes, &p.mark_change) {
                            (TrackChanges::RejectChanges, ParaMarkChange::Inserted(_))
                            | (TrackChanges::AcceptChanges, ParaMarkChange::Deleted(_)) => {
                                MergeMode::WithSpace
                            }
                            _ => MergeMode::None,
                        }
                    };
                    if mode != MergeMode::None
                        && let Some(BodyItem::Para(prev)) = out.last_mut()
                    {
                        if mode == MergeMode::WithSpace
                            && !prev.inlines.is_empty()
                            && !p.inlines.is_empty()
                            && !matches!(prev.inlines.last(), Some(Inline::Space))
                            && !matches!(p.inlines.first(), Some(Inline::Space))
                        {
                            prev.inlines.push(Inline::Space);
                        }
                        if mode == MergeMode::Direct {
                            // Keep target paragraph's properties (dropcap inherits body para props).
                            prev.props = p.props;
                            prev.style_names = p.style_names;
                        }
                        for inl in p.inlines {
                            push_normalized_inline(&mut prev.inlines, inl);
                        }
                        prev.extra_blocks.extend(p.extra_blocks);
                        prev.caption_blocks.extend(p.caption_blocks);
                        mode = next_mode;
                    } else {
                        p.inlines = normalize_inline_list(p.inlines);
                        out.push(BodyItem::Para(p));
                        mode = next_mode;
                    }
                }
                other => {
                    mode = MergeMode::None;
                    out.push(other);
                }
            }
        }
        out
    }

    fn para_list_info(&self, p: &ParsedPara) -> Option<(Text, i64, ListKind)> {
        if heading_info(&p.style_names, p.props.outline_lvl).is_some() {
            return None;
        }
        let num_pr = p.props.num_pr.as_ref()?;
        let kind = self.numbering.resolve(&num_pr.num_id, num_pr.ilvl)?;
        Some((num_pr.num_id.clone(), num_pr.ilvl, kind))
    }

    fn consume_definition_list(&self, items: &[BodyItem], start_idx: usize) -> (Block, usize) {
        let mut entries: Vec<(Vec<Inline>, Vec<Vec<Block>>)> = Vec::new();
        let mut idx = start_idx;
        while idx < items.len() {
            let BodyItem::Para(p) = &items[idx] else {
                break;
            };
            if !is_def_term_style(&p.style_names) {
                break;
            }
            let term = p.inlines.clone();
            idx += 1;
            let mut def_blocks: Vec<Block> = Vec::new();
            while let Some(BodyItem::Para(dp)) = items.get(idx) {
                if is_def_body_style(&dp.style_names) {
                    let b = if is_compact_style(&dp.style_names) {
                        Block::Plain(dp.inlines.clone())
                    } else {
                        Block::Para(dp.inlines.clone())
                    };
                    def_blocks.push(b);
                    idx += 1;
                } else {
                    break;
                }
            }
            let defs = if def_blocks.is_empty() {
                Vec::new()
            } else {
                vec![def_blocks]
            };
            entries.push((term, defs));
        }
        (Block::DefinitionList(entries), idx)
    }

    #[allow(clippy::too_many_arguments)]
    fn consume_list(
        &self,
        items: &[BodyItem],
        start_idx: usize,
        root_num_id: &str,
        root_ilvl: i64,
        root_kind: ListKind,
        counters: &mut HashMap<Text, BTreeMap<i64, i64>>,
        list_depth: usize,
    ) -> Result<(Block, usize)> {
        let max_depth = self.context.limits.max_nesting_depth;
        if list_depth >= max_depth {
            return Err(Error::Parse(format!(
                "DOCX list nesting exceeded limit of {max_depth} levels"
            )));
        }
        let mut list_items: Vec<Vec<Block>> = Vec::new();
        let mut idx = start_idx;

        let effective_start = match &root_kind {
            ListKind::Ordered { start, .. } => {
                if self.numbering.has_start_override(root_num_id, root_ilvl) {
                    *start
                } else if let Some(&prev) = counters
                    .get(root_num_id)
                    .and_then(|levels| levels.get(&root_ilvl))
                {
                    prev + 1
                } else {
                    *start
                }
            }
            ListKind::Bullet { .. } => 1,
        };

        let mut item_count = 0i64;
        while idx < items.len() {
            let BodyItem::Para(p) = &items[idx] else {
                break;
            };
            if let Some((num_id, ilvl, kind)) = self.para_list_info(p) {
                if ilvl < root_ilvl {
                    break;
                }
                if ilvl == root_ilvl {
                    let compatible = match (&root_kind, &kind) {
                        (ListKind::Bullet { checkbox: c1 }, ListKind::Bullet { checkbox: c2 }) => {
                            num_id == root_num_id || (c1.is_some() && c2.is_some())
                        }
                        (
                            ListKind::Ordered {
                                style: s1,
                                delim: d1,
                                ..
                            },
                            ListKind::Ordered {
                                style: s2,
                                delim: d2,
                                ..
                            },
                        ) => {
                            s1 == s2
                                && d1 == d2
                                && (num_id == root_num_id
                                    || !self.numbering.has_start_override(&num_id, ilvl))
                        }
                        _ => false,
                    };
                    if !compatible && idx > start_idx {
                        break;
                    }
                    // Reset deeper sublist counters for this `numId` in O(log L + removed).
                    if let Some(levels) = counters.get_mut(root_num_id) {
                        let _ = levels.split_off(&root_ilvl.saturating_add(1));
                    }
                    let mut inlines = p.inlines.clone();
                    if let ListKind::Bullet {
                        checkbox: Some(box_sym),
                    } = kind
                    {
                        let mut prefixed = vec![Inline::Str(box_sym.into()), Inline::Space];
                        prefixed.extend(inlines);
                        inlines = prefixed;
                    }
                    let block = if is_compact_style(&p.style_names) {
                        Block::Plain(inlines)
                    } else {
                        Block::Para(inlines)
                    };
                    let mut entry = vec![block];
                    entry.extend(p.extra_blocks.clone());
                    list_items.push(entry);
                    item_count += 1;
                    idx += 1;
                } else {
                    let (nested, next_idx) = self.consume_list(
                        items,
                        idx,
                        &num_id,
                        ilvl,
                        kind,
                        counters,
                        list_depth + 1,
                    )?;
                    if let Some(last) = list_items.last_mut() {
                        last.push(nested);
                    } else {
                        list_items.push(vec![nested]);
                    }
                    idx = next_idx;
                }
            } else if !p.inlines.is_empty()
                && heading_info(&p.style_names, p.props.outline_lvl).is_none()
                && (p
                    .props
                    .num_pr
                    .as_ref()
                    .is_some_and(|np| np.num_id == "0" || np.ilvl >= root_ilvl)
                    || p.props.direct_indent_left.is_some_and(|l| l > 0))
                && let Some(last) = list_items.last_mut()
            {
                // Continuation paragraph inside a list item (deleted bullet, dummy bullet, or indented follow-up).
                let cont = if is_compact_style(&p.style_names) {
                    Block::Plain(p.inlines.clone())
                } else {
                    Block::Para(p.inlines.clone())
                };
                last.push(cont);
                last.extend(p.extra_blocks.clone());
                idx += 1;
            } else {
                break;
            }
        }

        let block = match root_kind {
            ListKind::Bullet { .. } => Block::BulletList(list_items),
            ListKind::Ordered { style, delim, .. } => {
                if item_count > 0 {
                    counters
                        .entry(Text::from(root_num_id))
                        .or_default()
                        .insert(root_ilvl, effective_start + item_count - 1);
                }
                Block::OrderedList(
                    ListAttributes {
                        start: effective_start,
                        style,
                        delim,
                    },
                    list_items,
                )
            }
        };
        Ok((block, idx))
    }
}

#[derive(Default)]
struct SeqInfo {
    has_table: bool,
    has_any: bool,
    is_bibl_start: bool,
    is_bibl_end: bool,
}

struct ActiveFieldSlot {
    instr: String,
    in_result: bool,
    result: Vec<RawInline>,
}

fn push_field_item(stack: &mut [ActiveFieldSlot], out: &mut Vec<RawInline>, item: RawInline) {
    if let Some(top) = stack.last_mut() {
        if top.in_result {
            top.result.push(item);
        }
    } else {
        out.push(item);
    }
}

fn flush_atoms(props: &RunProps, atoms: &mut Vec<RunAtom>, out: &mut Vec<RawInline>) {
    if !atoms.is_empty() {
        out.push(RawInline::Run {
            props: props.clone(),
            items: std::mem::take(atoms),
        });
    }
}

fn strip_hyperlink_underline(items: Vec<RawInline>) -> Vec<RawInline> {
    items
        .into_iter()
        .map(|item| match item {
            RawInline::Run { mut props, items } => {
                if props.style_names.iter().any(|n| {
                    n.eq_ignore_ascii_case("Hyperlink")
                        || n.eq_ignore_ascii_case("FollowedHyperlink")
                }) {
                    props.underline = None;
                }
                RawInline::Run { props, items }
            }
            other => other,
        })
        .collect()
}

fn decode_pua_text(text: &str, font: Option<&str>) -> Text {
    if !text
        .chars()
        .any(|c| (0xF000..=0xF0FF).contains(&u32::from(c)))
    {
        return Text::from(text);
    }
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        let cp = u32::from(ch);
        if (0xF000..=0xF0FF).contains(&cp) {
            let f = font.or(Some("Symbol"));
            if let Some(s) = decode_symbol(f, &format!("{cp:X}")) {
                out.push_str(&s);
            } else if let Some(s) = decode_symbol(Some("Symbol"), &format!("{cp:X}")) {
                out.push_str(&s);
            } else {
                out.push(ch);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn is_image_only_inlines(inlines: &[Inline]) -> bool {
    let trimmed = trim_para_edges(inlines.to_vec());
    trimmed.len() == 1 && matches!(trimmed[0], Inline::Image(..))
}

fn try_make_figure(inlines: &[Inline], caption_blocks: &[Block]) -> Option<Block> {
    let trimmed = trim_para_edges(inlines.to_vec());
    let has_image = trimmed.iter().any(|i| matches!(i, Inline::Image(..)));
    if !has_image {
        return None;
    }
    Some(Block::Figure(
        Attr::default(),
        Caption {
            short: None,
            long: caption_blocks.to_vec(),
        },
        vec![Block::Plain(trimmed)],
    ))
}

fn parse_change_info(el: &XmlElement, ns: &DocxNs) -> ChangeInfo {
    ChangeInfo {
        author: ns.w_attr(el, "author").unwrap_or("").to_owned(),
        date: ns.w_attr(el, "date").unwrap_or("").to_owned(),
    }
}

fn make_change_span(class: &str, info: &ChangeInfo, inner: Vec<Inline>) -> Inline {
    let mut kvs = Vec::new();
    if !info.author.is_empty() {
        kvs.push(("author".into(), info.author.clone()));
    }
    if !info.date.is_empty() {
        kvs.push(("date".into(), info.date.clone()));
    }
    Inline::Span(Attr::new("", vec![class.into()], kvs), inner)
}

fn choose_alternate_content(el: &XmlElement) -> Option<&XmlElement> {
    el.elements()
        .find(|c| is_mc(c, "Choice"))
        .or_else(|| el.elements().find(|c| is_mc(c, "Fallback")))
}

fn find_blip_rid<'a>(el: &'a XmlElement, ns: &DocxNs) -> Option<&'a str> {
    let mut stack = vec![el];
    while let Some(cur) = stack.pop() {
        if is_a(cur, "blip")
            && let Some(rid) = ns.r_attr(cur, "embed").or_else(|| ns.r_attr(cur, "link"))
        {
            return Some(rid);
        }
        for c in cur.elements().collect::<Vec<_>>().into_iter().rev() {
            if !is_wps(c, "txbx") {
                stack.push(c);
            }
        }
    }
    None
}

fn find_txbx_content<'a>(el: &'a XmlElement, ns: &DocxNs) -> Option<&'a XmlElement> {
    let mut stack = vec![el];
    while let Some(cur) = stack.pop() {
        if ns.is_w(cur, "txbxContent") {
            return Some(cur);
        }
        if is_pic(cur, "pic") {
            continue;
        }
        for c in cur.elements().collect::<Vec<_>>().into_iter().rev() {
            stack.push(c);
        }
    }
    None
}

fn emu_to_inches(emu: f64) -> String {
    let inches = emu / 914_400.0;
    format!("{}in", format_haskell_double(inches))
}

fn format_haskell_double(v: f64) -> String {
    let s = format!("{v}");
    if !s.contains('.') && !s.contains('e') && !s.contains('E') {
        format!("{s}.0")
    } else {
        s
    }
}

fn tokenize_plain_inlines(text: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    for (idx, word) in text.split_whitespace().enumerate() {
        if idx > 0 {
            out.push(Inline::Space);
        }
        out.push(Inline::Str(word.to_owned()));
    }
    out
}

fn tokenize_text_preserving_edges(text: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut in_space = false;

    for ch in text.chars() {
        if matches!(ch, ' ' | '\t' | '\n' | '\r') {
            if !buf.is_empty() {
                out.push(Inline::Str(std::mem::take(&mut buf)));
            }
            if !in_space {
                out.push(Inline::Space);
                in_space = true;
            }
        } else {
            in_space = false;
            buf.push(ch);
        }
    }
    if !buf.is_empty() {
        out.push(Inline::Str(buf));
    }
    out
}

fn flatten_code_inlines(items: &[RawInline]) -> Vec<Inline> {
    let mut text = String::new();
    fn collect(items: &[RawInline], out: &mut String) {
        for item in items {
            match item {
                RawInline::Run { items, .. } => {
                    for atom in items {
                        match atom {
                            RunAtom::Text(t) => out.push_str(t),
                            RunAtom::Tab => out.push('\t'),
                            RunAtom::LineBreak => out.push('\n'),
                            RunAtom::Inline(_) => {}
                        }
                    }
                }
                RawInline::Tracked { items, .. } | RawInline::Link { items, .. } => {
                    collect(items, out);
                }
                _ => {}
            }
        }
    }
    collect(items, &mut text);
    if text.is_empty() {
        Vec::new()
    } else {
        vec![Inline::Str(text)]
    }
}

/// Normalizes an inline list by merging adjacent `Str`, collapsing `Space`,
/// and merging adjacent identical formatting wrappers (including across a
/// single `Space`, commuting nested single-child formatting wrappers when
/// both sides share an outer style).
fn normalize_inline_list(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    for inl in inlines {
        push_normalized_inline(&mut out, inl);
    }
    out
}

fn push_normalized_inline(out: &mut Vec<Inline>, inl: Inline) {
    // Fast in-place merge for adjacent formatting wrappers to avoid O(n^2) re-normalization.
    if let Some(prev) = out.last_mut()
        && let Some(tag_a) = format_tag(prev)
    {
        if format_tag(&inl) == Some(tag_a) {
            if let Some(prev_children) = unwrap_formatted_mut(prev) {
                for child in into_formatted_children(inl) {
                    push_normalized_inline(prev_children, child);
                }
                return;
            }
        } else if let Some(peeled_b) = peel_format_tag(&inl, tag_a)
            && let Some(prev_children) = unwrap_formatted_mut(prev)
        {
            push_normalized_inline(prev_children, peeled_b);
            return;
        }
    }
    if let Some(prev) = out.last()
        && let Some(tag_b) = format_tag(&inl)
        && let Some(peeled_a) = peel_format_tag(prev, tag_b)
    {
        out.pop();
        let mut combined = vec![peeled_a];
        for child in into_formatted_children(inl) {
            push_normalized_inline(&mut combined, child);
        }
        push_normalized_inline(out, rewrap_formatted(tag_b, combined));
        return;
    }

    match (out.last_mut(), inl) {
        (Some(Inline::Str(prev)), Inline::Str(next)) => prev.push_str(&next),
        (Some(Inline::Space), Inline::Space) => {}
        (Some(Inline::Code(a1, s1)), Inline::Code(a2, s2)) if *a1 == a2 => {
            s1.push_str(&s2);
        }
        (_, other) => out.push(other),
    }
}

fn unwrap_formatted_mut(inl: &mut Inline) -> Option<&mut Vec<Inline>> {
    match inl {
        Inline::Emph(v)
        | Inline::Strong(v)
        | Inline::Underline(v)
        | Inline::Strikeout(v)
        | Inline::SmallCaps(v)
        | Inline::Superscript(v)
        | Inline::Subscript(v)
        | Inline::Span(_, v) => Some(v),
        _ => None,
    }
}

fn into_formatted_children(inl: Inline) -> Vec<Inline> {
    match inl {
        Inline::Emph(v)
        | Inline::Strong(v)
        | Inline::Underline(v)
        | Inline::Strikeout(v)
        | Inline::SmallCaps(v)
        | Inline::Superscript(v)
        | Inline::Subscript(v)
        | Inline::Span(_, v) => v,
        _ => Vec::new(),
    }
}

/// Recursively lifts leading and trailing `Space`s outside formatting wrappers
/// (keeping spaces inside tracked-change spans untouched).
fn lift_edge_spaces_out_of_formatting(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out = Vec::with_capacity(inlines.len());
    for inl in inlines {
        if let Some(tag) = format_tag(&inl) {
            let inner = lift_edge_spaces_out_of_formatting(unwrap_formatted(&inl).to_vec());
            let lifted = wrap_moving_spaces(inner, |core| rewrap_formatted(tag, core));
            for item in lifted {
                match (out.last_mut(), item) {
                    (Some(Inline::Space), Inline::Space) => {}
                    (_, other) => out.push(other),
                }
            }
        } else if matches!(&inl, Inline::Span(attr, _) if attr.get("custom-style").is_some()) {
            if let Inline::Span(attr, inner) = inl {
                let inner = lift_edge_spaces_out_of_formatting(inner);
                let lifted = wrap_moving_spaces(inner, |core| Inline::Span(attr, core));
                for item in lifted {
                    match (out.last_mut(), item) {
                        (Some(Inline::Space), Inline::Space) => {}
                        (_, other) => out.push(other),
                    }
                }
            }
        } else {
            match (out.last_mut(), inl) {
                (Some(Inline::Space), Inline::Space) => {}
                (_, other) => out.push(other),
            }
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FmtTag {
    Emph,
    Strong,
    Underline,
    Strikeout,
    SmallCaps,
    Superscript,
    Subscript,
    Mark,
}

fn format_tag(inl: &Inline) -> Option<FmtTag> {
    match inl {
        Inline::Emph(_) => Some(FmtTag::Emph),
        Inline::Strong(_) => Some(FmtTag::Strong),
        Inline::Underline(_) => Some(FmtTag::Underline),
        Inline::Strikeout(_) => Some(FmtTag::Strikeout),
        Inline::SmallCaps(_) => Some(FmtTag::SmallCaps),
        Inline::Superscript(_) => Some(FmtTag::Superscript),
        Inline::Subscript(_) => Some(FmtTag::Subscript),
        Inline::Span(attr, _)
            if attr.id.is_empty()
                && attr.attributes.is_empty()
                && attr.classes.as_slice() == ["mark"] =>
        {
            Some(FmtTag::Mark)
        }
        _ => None,
    }
}

fn unwrap_formatted(inl: &Inline) -> &[Inline] {
    match inl {
        Inline::Emph(v)
        | Inline::Strong(v)
        | Inline::Underline(v)
        | Inline::Strikeout(v)
        | Inline::SmallCaps(v)
        | Inline::Superscript(v)
        | Inline::Subscript(v)
        | Inline::Span(_, v) => v,
        _ => &[],
    }
}

fn rewrap_formatted(tag: FmtTag, inner: Vec<Inline>) -> Inline {
    match tag {
        FmtTag::Emph => Inline::Emph(inner),
        FmtTag::Strong => Inline::Strong(inner),
        FmtTag::Underline => Inline::Underline(inner),
        FmtTag::Strikeout => Inline::Strikeout(inner),
        FmtTag::SmallCaps => Inline::SmallCaps(inner),
        FmtTag::Superscript => Inline::Superscript(inner),
        FmtTag::Subscript => Inline::Subscript(inner),
        FmtTag::Mark => Inline::Span(Attr::new("", vec!["mark".into()], vec![]), inner),
    }
}

/// If `inl` is a nested single-child chain of formatting wrappers containing
/// `target`, removes `target` from inside the chain and returns the remaining wrapper.
fn peel_format_tag(inl: &Inline, target: FmtTag) -> Option<Inline> {
    let outer = format_tag(inl)?;
    let children = unwrap_formatted(inl);
    if children.len() != 1 {
        return None;
    }
    let child = &children[0];
    if format_tag(child) == Some(target) {
        return Some(rewrap_formatted(outer, unwrap_formatted(child).to_vec()));
    }
    let deeper = peel_format_tag(child, target)?;
    Some(rewrap_formatted(outer, vec![deeper]))
}

fn trim_para_edges(mut inlines: Vec<Inline>) -> Vec<Inline> {
    let start = inlines
        .iter()
        .take_while(|i| matches!(i, Inline::Space | Inline::LineBreak))
        .count();
    if start > 0 {
        inlines.drain(..start);
    }
    while matches!(inlines.last(), Some(Inline::Space | Inline::LineBreak)) {
        inlines.pop();
    }
    inlines
}

fn wrap_moving_spaces(
    mut inlines: Vec<Inline>,
    wrap: impl FnOnce(Vec<Inline>) -> Inline,
) -> Vec<Inline> {
    let mut out = Vec::new();
    let lead = inlines
        .iter()
        .take_while(|i| matches!(i, Inline::Space))
        .count();
    let leading: Vec<Inline> = inlines.drain(..lead).collect();
    let mut trailing = Vec::new();
    while matches!(inlines.last(), Some(Inline::Space)) {
        if let Some(sp) = inlines.pop() {
            trailing.push(sp);
        }
    }
    out.extend(leading);
    if !inlines.is_empty() {
        out.push(wrap(inlines));
    }
    out.extend(trailing);
    out
}

fn emit_wrapped_moving_spaces(
    inlines: Vec<Inline>,
    out: &mut Vec<Inline>,
    wrap: impl FnOnce(Vec<Inline>) -> Inline,
) {
    out.extend(wrap_moving_spaces(inlines, wrap));
}

fn primary_style_name(names: &[Text]) -> Option<&str> {
    names.first().map(String::as_str)
}

fn is_compact_style_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("Compact")
}

fn is_compact_style(names: &[Text]) -> bool {
    names.first().is_some_and(|n| is_compact_style_name(n))
}

fn is_default_char_style(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "default paragraph font" | "hyperlink" | "footnote reference" | "endnote reference"
    )
}

fn is_source_code_style(names: &[Text]) -> bool {
    names
        .iter()
        .any(|n| n.eq_ignore_ascii_case("Source Code") || n.eq_ignore_ascii_case("SourceCode"))
}

fn is_caption_style(names: &[Text]) -> bool {
    names.iter().any(|n| {
        let l = n.to_ascii_lowercase();
        l == "caption" || l == "table caption" || l == "image caption"
    })
}

fn is_def_term_style(names: &[Text]) -> bool {
    names
        .first()
        .is_some_and(|n| n.eq_ignore_ascii_case("Definition Term"))
}

fn is_def_body_style(names: &[Text]) -> bool {
    names
        .first()
        .is_some_and(|n| n.eq_ignore_ascii_case("Definition"))
}

fn is_blockquote_para(p: &ParsedPara) -> bool {
    if p.style_names.iter().any(|n| {
        matches!(
            n.to_ascii_lowercase().as_str(),
            "block text" | "quote" | "intense quote" | "quotation" | "blockquote" | "cita"
        )
    }) {
        return true;
    }
    // Direct paragraph left indent relative to the style's own indent (oracle-verified):
    // Only triggers when `w:pPr/w:ind/@w:left` is directly present on the paragraph,
    // `direct_left - direct_hanging > 0`, and `direct_left - direct_hanging > style_indent_left`.
    if p.props.num_pr.is_none()
        && let Some(direct_left) = p.props.direct_indent_left
    {
        let eff = direct_left - p.props.direct_indent_hanging.unwrap_or(0);
        if eff > 0 && eff > p.props.style_indent_left {
            return true;
        }
    }
    false
}

fn meta_style_key(names: &[Text]) -> Option<&'static str> {
    for n in names {
        match n.to_ascii_lowercase().as_str() {
            "title" => return Some("title"),
            "subtitle" => return Some("subtitle"),
            "author" => return Some("author"),
            "date" => return Some("date"),
            "abstract" => return Some("abstract"),
            _ => {}
        }
    }
    None
}

fn collect_table_children<'a>(
    parent: &'a XmlElement,
    ns: &DocxNs,
    local: &str,
) -> Vec<&'a XmlElement> {
    let mut out = Vec::new();
    for child in parent.elements() {
        if ns.is_w(child, local) {
            out.push(child);
        } else if ns.is_w(child, "sdt") {
            if let Some(content) = ns.w_child(child, "sdtContent") {
                out.extend(collect_table_children(content, ns, local));
            }
        } else if ns.is_w(child, "customXml") || ns.is_w(child, "smartTag") {
            out.extend(collect_table_children(child, ns, local));
        }
    }
    out
}

fn infer_cell_alignment(items: &[BodyItem]) -> Alignment {
    let mut align: Option<Alignment> = None;
    for item in items {
        if let BodyItem::Para(p) = item
            && let Some(jc) = p.props.direct_jc.as_deref()
        {
            let a = match jc {
                "left" | "both" => Alignment::AlignLeft,
                "center" => Alignment::AlignCenter,
                "right" => Alignment::AlignRight,
                _ => Alignment::AlignDefault,
            };
            if a != Alignment::AlignDefault {
                match align {
                    None => align = Some(a),
                    Some(prev) if prev == a => {}
                    Some(_) => return Alignment::AlignDefault,
                }
            }
        }
    }
    align.unwrap_or(Alignment::AlignDefault)
}

fn extract_metadata(blocks: &mut Vec<Block>) -> Meta {
    let mut meta = Meta::default();
    let mut authors: Vec<MetaValue> = Vec::new();
    let mut consumed = 0usize;

    for block in blocks.iter() {
        let Block::Div(attr, inner) = block else {
            break;
        };
        if !attr.classes.iter().any(|c| c == "__docx_meta") {
            break;
        }
        let Some((_, key)) = attr.attributes.iter().find(|(k, _)| k == "key") else {
            break;
        };
        let inlines = match inner.first() {
            Some(Block::Para(inls)) => trim_para_edges(inls.clone()),
            _ => Vec::new(),
        };
        match key.as_str() {
            "author" => authors.push(MetaValue::MetaInlines(inlines)),
            other => {
                meta.insert(other, MetaValue::MetaInlines(inlines));
            }
        }
        consumed += 1;
    }

    if !authors.is_empty() {
        if authors.len() == 1 {
            if let Some(a) = authors.pop() {
                meta.insert("author", a);
            }
        } else {
            meta.insert("author", MetaValue::MetaList(authors));
        }
    }

    blocks.drain(..consumed);

    for block in blocks.iter_mut() {
        let replace = if let Block::Div(attr, inner) = block
            && attr.classes.iter().any(|c| c == "__docx_meta")
            && let Some(Block::Para(inls)) = inner.first()
        {
            Some(Block::Para(trim_para_edges(inls.clone())))
        } else {
            None
        };
        if let Some(p) = replace {
            *block = p;
        }
    }

    trim_all_block_edges(blocks);
    meta
}

fn trim_all_block_edges(blocks: &mut [Block]) {
    for block in blocks.iter_mut() {
        match block {
            Block::Para(inls) | Block::Plain(inls) | Block::Header(_, _, inls) => {
                *inls = trim_para_edges(std::mem::take(inls));
            }
            Block::BlockQuote(bs) | Block::Div(_, bs) => trim_all_block_edges(bs),
            Block::Figure(_, cap, bs) => {
                trim_all_block_edges(&mut cap.long);
                trim_all_block_edges(bs);
            }
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                for item in items {
                    trim_all_block_edges(item);
                }
            }
            Block::DefinitionList(entries) => {
                for (term, defs) in entries {
                    *term = trim_para_edges(std::mem::take(term));
                    for def in defs {
                        trim_all_block_edges(def);
                    }
                }
            }
            Block::Table(table) => {
                trim_all_block_edges(&mut table.caption.long);
                for row in &mut table.head.rows {
                    for cell in &mut row.cells {
                        trim_all_block_edges(&mut cell.content);
                    }
                }
                for body in &mut table.bodies {
                    for row in &mut body.head {
                        for cell in &mut row.cells {
                            trim_all_block_edges(&mut cell.content);
                        }
                    }
                    for row in &mut body.body {
                        for cell in &mut row.cells {
                            trim_all_block_edges(&mut cell.content);
                        }
                    }
                }
                for row in &mut table.foot.rows {
                    for cell in &mut row.cells {
                        trim_all_block_edges(&mut cell.content);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Assigns header identifiers, absorbs bookmarks inside headings, collapses
/// adjacent overlapping anchor spans, retargets links, and prunes unused anchors.
fn apply_identifiers_and_bookmarks(blocks: &mut [Block], options: &ReaderOptions) {
    // Pandoc's DOCX reader always assigns heading identifiers (oracle-verified:
    // even with `docx-auto_identifiers`, headings receive auto-generated IDs).
    let auto_ids = true;
    let mut id_set = IdentifierSet::new();
    let mut retarget: HashMap<Text, Text> = HashMap::new();

    // 1. Absorb bookmarks inside headings & assign auto-identifiers.
    assign_heading_ids(blocks, options, auto_ids, &mut id_set, &mut retarget);

    // 2. Collapse consecutive empty anchor spans anywhere in the document.
    collapse_adjacent_anchors(blocks, &mut retarget);

    // 3. Apply retargeting (following transitive chains if any).
    if !retarget.is_empty() {
        retarget_links(blocks, &retarget);
    }

    // 4. Collect all referenced internal `#id` targets and prune unreferenced anchor spans.
    let referenced = collect_internal_link_targets(blocks);
    prune_unreferenced_anchors(blocks, &referenced);
}

fn is_anchor_span(inl: &Inline) -> Option<&str> {
    if let Inline::Span(attr, inner) = inl
        && inner.is_empty()
        && attr.classes.as_slice() == ["anchor"]
        && !attr.id.is_empty()
    {
        Some(&attr.id)
    } else {
        None
    }
}

fn assign_heading_ids(
    blocks: &mut [Block],
    options: &ReaderOptions,
    auto_ids: bool,
    id_set: &mut IdentifierSet,
    retarget: &mut HashMap<Text, Text>,
) {
    for block in blocks.iter_mut() {
        randoc_ast::walk::walk_blocks_mut(block, |b| {
            if let Block::Header(_, attr, inlines) = b {
                let mut found_anchors = Vec::new();
                strip_anchors_recursive(inlines, &mut found_anchors);
                *inlines = trim_para_edges(std::mem::take(inlines));

                if auto_ids && attr.id.is_empty() {
                    let base = auto_identifier(inlines, options.extensions);
                    attr.id = id_set.unique(&base);
                } else if attr.id.is_empty()
                    && let Some(first) = found_anchors
                        .iter()
                        .find(|a| !a.eq_ignore_ascii_case("_GoBack"))
                {
                    attr.id = first.clone();
                }

                if !attr.id.is_empty() {
                    for bm in found_anchors {
                        if bm != attr.id && !bm.eq_ignore_ascii_case("_GoBack") {
                            retarget.insert(bm, attr.id.clone());
                        }
                    }
                }
            }
        });
    }
}

fn strip_anchors_recursive(inlines: &mut Vec<Inline>, out: &mut Vec<Text>) {
    inlines.retain_mut(|inl| {
        if let Some(id) = is_anchor_span(inl) {
            out.push(Text::from(id));
            return false;
        }
        match inl {
            Inline::Emph(v)
            | Inline::Strong(v)
            | Inline::Underline(v)
            | Inline::Strikeout(v)
            | Inline::Superscript(v)
            | Inline::Subscript(v)
            | Inline::SmallCaps(v)
            | Inline::Quoted(_, v)
            | Inline::Link(_, v, _)
            | Inline::Span(_, v) => {
                strip_anchors_recursive(v, out);
                true
            }
            _ => true,
        }
    });
}

fn collapse_adjacent_anchors(blocks: &mut [Block], retarget: &mut HashMap<Text, Text>) {
    struct Collapser<'a>(&'a mut HashMap<Text, Text>);
    impl randoc_ast::walk::VisitorMut for Collapser<'_> {
        fn inlines(&mut self, inlines: &mut Vec<Inline>) {
            let mut prev_anchor: Option<Text> = None;
            inlines.retain(|inl| {
                if let Some(id) = is_anchor_span(inl) {
                    if id.eq_ignore_ascii_case("_GoBack") {
                        return false;
                    }
                    if let Some(keep) = &prev_anchor {
                        if id != keep.as_str() {
                            self.0.insert(Text::from(id), keep.clone());
                        }
                        return false;
                    }
                    prev_anchor = Some(Text::from(id));
                    true
                } else {
                    prev_anchor = None;
                    true
                }
            });
        }
    }
    let mut visitor = Collapser(retarget);
    for block in blocks.iter_mut() {
        randoc_ast::walk::walk_mut(block, &mut visitor);
    }
}

fn retarget_links(blocks: &mut [Block], retarget: &HashMap<Text, Text>) {
    struct Retargeter<'a>(&'a HashMap<Text, Text>);
    impl randoc_ast::walk::VisitorMut for Retargeter<'_> {
        fn inline(&mut self, inline: &mut Inline) {
            if let Inline::Link(_, _, target) = inline
                && let Some(frag) = target.url.strip_prefix('#')
            {
                let mut cur = frag;
                let mut steps = 0usize;
                while let Some(next) = self.0.get(cur) {
                    cur = next.as_str();
                    steps += 1;
                    if steps > 16 {
                        break;
                    }
                }
                if cur != frag {
                    target.url = format!("#{cur}");
                }
            }
        }
    }
    let mut visitor = Retargeter(retarget);
    for block in blocks.iter_mut() {
        randoc_ast::walk::walk_mut(block, &mut visitor);
    }
}

fn collect_internal_link_targets(blocks: &mut [Block]) -> HashSet<Text> {
    struct Collector(HashSet<Text>);
    impl randoc_ast::walk::VisitorMut for Collector {
        fn inline(&mut self, inline: &mut Inline) {
            if let Inline::Link(_, _, target) = inline
                && let Some(frag) = target.url.strip_prefix('#')
            {
                self.0.insert(Text::from(frag));
            }
        }
    }
    let mut collector = Collector(HashSet::new());
    for block in blocks.iter_mut() {
        randoc_ast::walk::walk_mut(block, &mut collector);
    }
    collector.0
}

fn prune_unreferenced_anchors(blocks: &mut [Block], referenced: &HashSet<Text>) {
    struct Pruner<'a>(&'a HashSet<Text>);
    impl randoc_ast::walk::VisitorMut for Pruner<'_> {
        fn inlines(&mut self, inlines: &mut Vec<Inline>) {
            inlines.retain(|inl| {
                if let Some(id) = is_anchor_span(inl) {
                    self.0.contains(id)
                } else {
                    true
                }
            });
        }
    }
    let mut pruner = Pruner(referenced);
    for block in blocks.iter_mut() {
        randoc_ast::walk::walk_mut(block, &mut pruner);
    }
    trim_all_block_edges(blocks);
}

// ---------------------------------------------------------------------------
// Field instruction parsers (HYPERLINK, REF/PAGEREF)
// ---------------------------------------------------------------------------

fn parse_hyperlink_instr(instr: &str) -> Option<(Text, Text)> {
    let rest = instr.trim()[9..].trim();
    let tokens = split_field_args(rest);
    let mut url: Option<String> = None;
    let mut anchor: Option<String> = None;
    let mut tooltip: Option<String> = None;
    let mut i = 0usize;
    while i < tokens.len() {
        match tokens[i].as_str() {
            "\\l" => {
                if let Some(a) = tokens.get(i + 1) {
                    anchor = Some(a.clone());
                    i += 2;
                    continue;
                }
            }
            "\\o" => {
                if let Some(t) = tokens.get(i + 1) {
                    tooltip = Some(t.clone());
                    i += 2;
                    continue;
                }
            }
            s if s.starts_with('\\') => {
                i += 1;
                continue;
            }
            other => {
                if url.is_none() {
                    url = Some(other.to_owned());
                }
            }
        }
        i += 1;
    }
    let target = match (url, anchor) {
        (Some(u), Some(a)) => format!("{u}#{a}"),
        (Some(u), None) => u,
        (None, Some(a)) => format!("#{a}"),
        (None, None) => return None,
    };
    Some((target, tooltip.unwrap_or_default()))
}

fn parse_ref_instr(instr: &str) -> Option<String> {
    let rest = instr.split_once(char::is_whitespace)?.1.trim();
    let tokens = split_field_args(rest);
    // Only emit a Link when the `\h` (create hyperlink) switch is present.
    if !tokens.iter().any(|t| t == "\\h") {
        return None;
    }
    tokens
        .into_iter()
        .find(|t| !t.is_empty() && !t.starts_with('\\'))
}

fn split_field_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }
        if ch == '"' {
            chars.next();
            let mut arg = String::new();
            while let Some(c) = chars.next() {
                if c == '"' {
                    break;
                }
                if c == '\\'
                    && let Some(&next) = chars.peek()
                    && (next == '"' || next == '\\')
                {
                    arg.push(next);
                    chars.next();
                    continue;
                }
                arg.push(c);
            }
            out.push(arg);
        } else {
            let mut arg = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                arg.push(c);
                chars.next();
            }
            out.push(arg);
        }
    }
    out
}
