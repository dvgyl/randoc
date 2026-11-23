use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt::Write;
use std::path::Path;

use randoc_archive::media::{decode_percent_bytes, is_safe_media_path, mime_from_extension};
use randoc_archive::opc::{Relationship, parse_relationships, rels_part_for, resolve_target};
use randoc_archive::xml::{self, QName, XmlDoc, XmlElement, XmlLimits, XmlNode, XmlOptions};
use randoc_archive::zip::{Archive, ArchiveError, ArchiveLimits, ArchiveWriter};
use randoc_ast::Meta;
use randoc_core::{Context, Error, PathKind, Result, WriterOptions};

use super::Renderer;
use super::xml::{A, Budget, DECL, R, W, WORD_NAMESPACES, Xml, size_error};

const DOC: &str = "word/document.xml";
const TYPES: &str = "[Content_Types].xml";
const DOC_RELS: &str = "word/_rels/document.xml.rels";
const NOTE_RELS: &str = "word/_rels/footnotes.xml.rels";
const STYLES: &str = "word/styles.xml";
const NUMBERING: &str = "word/numbering.xml";
const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

const STANDARD_RELS: [(&str, &str); 8] = [
    ("comments", "comments.xml"),
    ("footnotes", "footnotes.xml"),
    ("theme", "theme/theme1.xml"),
    ("fontTable", "fontTable.xml"),
    ("webSettings", "webSettings.xml"),
    ("settings", "settings.xml"),
    ("styles", "styles.xml"),
    ("numbering", "numbering.xml"),
];

const TYPE_OVERRIDES: [(&str, &str); 12] = [
    (
        "word/webSettings.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.webSettings+xml",
    ),
    (
        NUMBERING,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml",
    ),
    (
        "word/settings.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml",
    ),
    (
        "word/theme/theme1.xml",
        "application/vnd.openxmlformats-officedocument.theme+xml",
    ),
    (
        "word/fontTable.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml",
    ),
    (
        "docProps/app.xml",
        "application/vnd.openxmlformats-officedocument.extended-properties+xml",
    ),
    (
        "docProps/core.xml",
        "application/vnd.openxmlformats-package.core-properties+xml",
    ),
    (
        "docProps/custom.xml",
        "application/vnd.openxmlformats-officedocument.custom-properties+xml",
    ),
    (
        STYLES,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
    ),
    (
        DOC,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
    ),
    (
        "word/comments.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml",
    ),
    (
        "word/footnotes.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml",
    ),
];

const COPIED: [&str; 5] = [
    "word/settings.xml",
    "docProps/app.xml",
    "word/theme/theme1.xml",
    "word/fontTable.xml",
    "word/webSettings.xml",
];

const FALLBACK_SECTION: &str =
    r#"<w:sectPr><w:footnotePr><w:numRestart w:val="eachSect"/></w:footnotePr></w:sectPr>"#;

pub(super) struct Reference {
    pub(super) section: String,
    pub(super) styles: Option<XmlDoc>,
    pub(super) numbering: Option<XmlDoc>,
    parts: BTreeMap<String, Vec<u8>>,
    relations: Vec<Relationship>,
    types: BTreeMap<String, String>,
    width: i64,
    used: usize,
}

impl Reference {
    pub(super) fn load(options: &WriterOptions, context: &mut Context) -> Result<Self> {
        let data_dir = context.state.user_data_dir.clone();
        let explicit = options.reference_doc.as_deref();
        let path = if let Some(path) = explicit {
            Some(if is_web_url(path) {
                path.to_owned()
            } else {
                resource_path(path, context).ok_or_else(|| Error::ResourceNotFound(path.into()))?
            })
        } else {
            data_dir
                .as_deref()
                .map(|dir| format!("{dir}/reference.docx"))
                .filter(|path| is_file(context, path))
        };
        let mut reference = Self {
            section: FALLBACK_SECTION.into(),
            styles: None,
            numbering: None,
            parts: BTreeMap::new(),
            relations: Vec::new(),
            types: BTreeMap::new(),
            width: 7920,
            used: 0,
        };
        if let Some(path) = path {
            let mut limits = ArchiveLimits::from_limits(&context.limits);
            limits.max_input = limits.max_input.min(context.limits.max_resource_size);
            let bytes = if is_web_url(&path) {
                fetch_url(context, &path, limits.max_input)?.0
            } else {
                context.read_file_limited(&path, to_usize(limits.max_input))?
            };
            let mut archive = Archive::open(&bytes, limits).map_err(|error| {
                let name = explicit.unwrap_or(&path);
                Error::Parse(format!("Could not parse reference-doc {name:?}: {error}"))
            })?;
            let names = archive
                .entry_names()
                .into_iter()
                .filter(|name| name.starts_with("word/media/") && is_safe_media_path(name))
                .map(str::to_owned)
                .collect::<Vec<_>>();
            reference.read_parts(
                context,
                |name, _| archive.read(name).map_err(zip_error),
                names,
            )?;
        } else if let Some(dir) = data_dir {
            reference.read_parts(
                context,
                |name, context| {
                    let path = format!("{dir}/docx/{name}");
                    if is_file(context, &path) {
                        let limit = context
                            .limits
                            .max_resource_size
                            .min(context.limits.max_archive_size);
                        context.read_file_limited(&path, to_usize(limit)).map(Some)
                    } else {
                        Ok(None)
                    }
                },
                Vec::new(),
            )?;
        }
        Ok(reference)
    }

    fn read_parts(
        &mut self,
        context: &mut Context,
        mut read: impl FnMut(&str, &mut Context) -> Result<Option<Vec<u8>>>,
        media: Vec<String>,
    ) -> Result<()> {
        let input_budget = Budget::new(to_usize(context.limits.max_archive_size));
        let mut read = |name: &str, context: &mut Context| {
            let bytes = read(name, context)?;
            if let Some(bytes) = &bytes {
                input_budget.charge(bytes.len())?;
            }
            Ok::<_, Error>(bytes)
        };
        let mut part = |name: &str, context: &mut Context| {
            read(name, context)?
                .map(|bytes| parse_part(&bytes, name, context))
                .transpose()
        };
        let root_rel = part("_rels/.rels", context)?;
        let doc_path = root_rel
            .as_ref()
            .and_then(|rels| {
                parse_relationships(rels)
                    .into_iter()
                    .find(|rel| is_kind(rel, "officeDocument"))
            })
            .filter(|rel| !rel.external)
            .and_then(|rel| resolve_target("", &rel.target))
            .filter(|path| is_safe_media_path(path))
            .unwrap_or_else(|| DOC.into());
        if let Some(doc) = part(&doc_path, context)? {
            self.read_section(doc, context)?;
        }
        if let Some(rels) = part(&rels_part_for(&doc_path), context)? {
            self.relations = parse_relationships(&rels);
        }
        for (name, kind) in [(STYLES, "styles"), (NUMBERING, "numbering")] {
            let path = self.part_path(&doc_path, name, kind);
            let xml = part(&path, context)?;
            if name == STYLES {
                self.styles = xml;
            } else {
                self.numbering = xml;
            }
        }
        let mut defaults = BTreeMap::new();
        if let Some(types) = part(TYPES, context)? {
            for element in types.root.elements() {
                if let Some(mime) = element.attr_unqualified("ContentType") {
                    if let Some(name) = element
                        .attr_unqualified("PartName")
                        .and_then(|name| name.strip_prefix('/'))
                    {
                        self.types.insert(name.to_owned(), mime.to_owned());
                    } else if let Some(extension) = element.attr_unqualified("Extension") {
                        defaults.insert(extension.to_ascii_lowercase(), mime.to_owned());
                    }
                }
            }
        }
        for name in COPIED {
            let kind = STANDARD_RELS
                .iter()
                .find(|(_, target)| name.strip_prefix("word/") == Some(*target))
                .map_or("", |(kind, _)| *kind);
            let path = self.part_path(&doc_path, name, kind);
            if let Some(bytes) = read(&path, context)? {
                if name.ends_with(".xml") {
                    parse_part(&bytes, &path, context)?;
                }
                self.copy(name.to_owned(), bytes, context)?;
            }
        }
        self.relations
            .retain(|rel| is_kind(rel, "header") || is_kind(rel, "footer"));
        for rel in &mut self.relations {
            if !rel.external {
                let target = checked_target(&doc_path, &rel.target)?;
                rel.target = target
                    .strip_prefix("word/")
                    .map_or_else(|| format!("/{target}"), str::to_owned);
            }
        }
        let mut queue = VecDeque::new();
        for rel in &self.relations {
            if !rel.external {
                queue.push_back(checked_target(DOC, &rel.target)?);
            }
        }
        queue.extend(media);
        let mut seen = HashSet::new();
        while let Some(path) = queue.pop_front() {
            if self.parts.contains_key(&path) || is_generated(&path) || !seen.insert(path.clone()) {
                continue;
            }
            let Some(bytes) = read(&path, context)? else {
                continue;
            };
            let rels_path = rels_part_for(&path);
            if let Some(rels) = read(&rels_path, context)? {
                let doc = parse_part(&rels, &rels_path, context)?;
                for rel in parse_relationships(&doc) {
                    if !rel.external {
                        queue.push_back(checked_target(&path, &rel.target)?);
                    }
                }
                if !is_generated(&rels_path) && !self.parts.contains_key(&rels_path) {
                    self.copy(rels_path, rels, context)?;
                }
            }
            if let Some(extension) = path.rsplit('.').next()
                && let Some(mime) = defaults.get(&extension.to_ascii_lowercase())
            {
                self.types
                    .entry(path.clone())
                    .or_insert_with(|| mime.clone());
            }
            self.copy(path, bytes, context)?;
        }
        Ok(())
    }

    fn read_section(&mut self, doc: XmlDoc, context: &Context) -> Result<()> {
        if let Some(body) = doc.root.child(W, "body")
            && let Some(section) = body.children_ns(W, "sectPr").last()
        {
            let num = |tag, attr| {
                section
                    .child(W, tag)
                    .and_then(|el| el.attr(W, attr))
                    .and_then(|n| n.parse::<i64>().ok())
            };
            let page = num("pgSz", "w").unwrap_or(12_240);
            let left = num("pgMar", "left").unwrap_or(2160);
            let right = num("pgMar", "right").unwrap_or(2160);
            self.width = page
                .saturating_sub(left)
                .saturating_sub(right)
                .clamp(1, 31_680);
            let budget = Budget::new(to_usize(context.limits.max_archive_size));
            let mut out = budget.xml();
            let namespaces = [&body.ns_decls[..], &doc.root.ns_decls[..]];
            write_element(&mut out, section, &namespaces);
            self.section = out.into_string()?;
        }
        Ok(())
    }

    fn part_path(&self, doc: &str, fallback: &str, kind: &str) -> String {
        self.relations
            .iter()
            .find(|rel| !rel.external && is_kind(rel, kind))
            .and_then(|rel| resolve_target(doc, &rel.target))
            .filter(|name| is_safe_media_path(name))
            .unwrap_or_else(|| fallback.to_owned())
    }

    fn copy(&mut self, name: String, bytes: Vec<u8>, context: &Context) -> Result<()> {
        if self.parts.len() >= context.limits.max_archive_entries {
            return Err(Error::Parse("DOCX archive entry limit exceeded.".into()));
        }
        self.used = self
            .used
            .checked_add(bytes.len())
            .filter(|size| *size as u64 <= context.limits.max_archive_size)
            .ok_or_else(size_error)?;
        self.parts.insert(name, bytes);
        Ok(())
    }

    pub(super) fn remaining_bytes(&self, context: &Context) -> usize {
        to_usize(context.limits.max_archive_size).saturating_sub(self.used)
    }

    pub(super) fn text_width(&self) -> i64 {
        self.width
    }

    pub(super) fn document_relationships(&self) -> (Vec<Relationship>, u64) {
        let mut id = self
            .relations
            .iter()
            .filter_map(|rel| {
                rel.id
                    .strip_prefix("rId")?
                    .parse::<u32>()
                    .ok()
                    .map(u64::from)
            })
            .max()
            .unwrap_or_default()
            + 1;
        let mut result = Vec::new();
        for (kind, target) in STANDARD_RELS {
            result.push(relationship(id, kind, target, false));
            id += 1;
        }
        result.reverse();
        result.extend(self.relations.iter().cloned());
        (result, id)
    }

    pub(super) fn part_names(&self) -> HashSet<String> {
        self.parts.keys().cloned().collect()
    }
}

fn to_usize(limit: u64) -> usize {
    usize::try_from(limit).unwrap_or(usize::MAX)
}

pub(super) fn resource_path(name: &str, context: &mut Context) -> Option<String> {
    if let Some(uri) = name.strip_prefix("file://") {
        let uri = uri
            .strip_prefix("localhost")
            .filter(|path| path.starts_with('/'))
            .unwrap_or(uri);
        if !uri.starts_with('/') {
            return None;
        }
        let path = uri.split(['?', '#']).next()?;
        if !valid_uri_path(path) {
            return None;
        }
        let bytes = decode_percent_bytes(path, path.len())?;
        if bytes.contains(&0) {
            return None;
        }
        return String::from_utf8(bytes).ok();
    }
    if Path::new(name).is_absolute() {
        return Some(name.to_owned());
    }
    for dir in context.state.resource_path.clone() {
        let path = if dir == "." {
            name.into()
        } else {
            Path::new(&dir).join(name).to_string_lossy().into_owned()
        };
        if is_file(context, &path) {
            return Some(path);
        }
    }
    None
}

fn is_file(context: &mut Context, path: &str) -> bool {
    matches!(context.host().path_kind(path, true), Ok(PathKind::File))
}

fn valid_uri_path(path: &str) -> bool {
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            if !bytes.next().is_some_and(|b| b.is_ascii_hexdigit())
                || !bytes.next().is_some_and(|b| b.is_ascii_hexdigit())
            {
                return false;
            }
        } else if !byte.is_ascii_alphanumeric() && !b"-._~!$&'()*+,;=:@/".contains(&byte) {
            return false;
        }
    }
    true
}

pub(super) fn is_web_url(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

pub(super) fn fetch_url(
    context: &mut Context,
    source: &str,
    limit: u64,
) -> Result<(Vec<u8>, Option<String>)> {
    let headers = context.state.request_headers.clone();
    let response = context.host().fetch_url(source, &headers)?;
    if response.0.len() as u64 > limit {
        return Err(Error::Http {
            url: source.to_owned(),
            message: format!("The response exceeds the limit of {limit} bytes."),
        });
    }
    Ok(response)
}

fn checked_target(owner: &str, target: &str) -> Result<String> {
    resolve_target(owner, target)
        .filter(|path| is_safe_media_path(path))
        .ok_or_else(|| Error::Parse(format!("Invalid DOCX relationship target: {target}")))
}

fn is_generated(path: &str) -> bool {
    matches!(
        path,
        TYPES
            | DOC
            | DOC_RELS
            | NOTE_RELS
            | STYLES
            | NUMBERING
            | "_rels/.rels"
            | "word/comments.xml"
            | "word/footnotes.xml"
            | "docProps/core.xml"
            | "docProps/custom.xml"
    )
}

fn parse_part(bytes: &[u8], name: &str, context: &Context) -> Result<XmlDoc> {
    let error = |message| Error::Xml {
        file: format!("'{name}'"),
        message,
    };
    let source = xml::decode_utf8(bytes).map_err(|err| error(err.format_pandoc()))?;
    let limits = XmlLimits::from_limits(&context.limits);
    xml::parse(source, &limits, XmlOptions::default()).map_err(|err| error(err.format_pandoc()))
}

fn zip_error(error: ArchiveError) -> Error {
    Error::Parse(format!("Could not process DOCX archive: {error}"))
}

fn is_kind(relation: &Relationship, kind: &str) -> bool {
    relation
        .kind
        .strip_prefix(R)
        .and_then(|suffix| suffix.strip_prefix('/'))
        == Some(kind)
}

pub(super) fn relationship(id: u64, kind: &str, target: &str, external: bool) -> Relationship {
    Relationship {
        id: format!("rId{id}"),
        kind: format!("{R}/{kind}"),
        target: target.into(),
        external,
    }
}

fn relationships<'a>(
    budget: &Budget,
    relations: impl IntoIterator<Item = &'a Relationship>,
) -> Result<String> {
    let mut out = budget.xml();
    out.raw(DECL);
    let _ = write!(out, "<Relationships xmlns=\"{RELS_NS}\">");
    for rel in relations {
        out.raw("<Relationship Id=\"");
        out.escaped(&rel.id);
        out.raw("\" Type=\"");
        out.escaped(&rel.kind);
        out.raw("\" Target=\"");
        out.escaped(&rel.target);
        if rel.external {
            out.raw("\" TargetMode=\"External");
        }
        out.raw("\"/>");
    }
    out.raw("</Relationships>");
    out.into_string()
}

type Namespace = (Option<std::sync::Arc<str>>, std::sync::Arc<str>);

pub(super) fn reference_children(out: &mut Xml, root: XmlElement) {
    write_children(out, root.children, &root.ns_decls);
}

pub(super) fn write_children(out: &mut Xml, children: Vec<XmlNode>, namespaces: &[Namespace]) {
    for child in children {
        if let XmlNode::Element(element) = child {
            write_element(out, &element, &[namespaces]);
        }
    }
}

fn write_element(out: &mut Xml, element: &XmlElement, namespaces: &[&[Namespace]]) {
    enum Step<'a> {
        Open(&'a XmlElement, &'a [&'a [Namespace]]),
        Close(&'a QName),
        Text(&'a str),
    }
    let mut stack = vec![Step::Open(element, namespaces)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Open(element, namespaces) => {
                out.raw("<");
                name(out, &element.name);
                let mut seen = HashSet::new();
                for (prefix, uri) in element
                    .ns_decls
                    .iter()
                    .chain(namespaces.iter().flat_map(|decls| decls.iter()))
                {
                    if !seen.insert(prefix.as_deref()) {
                        continue;
                    }
                    out.raw(" xmlns");
                    if let Some(prefix) = prefix {
                        out.raw(":");
                        out.raw(prefix);
                    }
                    out.raw("=\"");
                    out.escaped(uri);
                    out.raw("\"");
                }
                for (attribute, value) in &element.attrs {
                    out.raw(" ");
                    name(out, attribute);
                    out.raw("=\"");
                    out.escaped(value);
                    out.raw("\"");
                }
                if element.children.is_empty() {
                    out.raw("/>");
                } else {
                    out.raw(">");
                    stack.push(Step::Close(&element.name));
                    stack.extend(element.children.iter().rev().map(|child| match child {
                        XmlNode::Element(child) => Step::Open(child, &[]),
                        XmlNode::Text(text) => Step::Text(text),
                    }));
                }
            }
            Step::Close(tag) => {
                out.raw("</");
                name(out, tag);
                out.raw(">");
            }
            Step::Text(text) => out.escaped(text),
        }
    }
}

fn name(out: &mut Xml, name: &QName) {
    if let Some(prefix) = &name.prefix {
        out.raw(prefix);
        out.raw(":");
    }
    out.raw(&name.local);
}

pub(super) fn timestamp(context: &mut Context) -> i64 {
    context
        .host()
        .environment_variable("SOURCE_DATE_EPOCH")
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| context.host().current_time().unix_seconds)
}

fn wrapped(budget: &Budget, tag: &str, prefix: &str, inner: Xml) -> Result<String> {
    let mut out = budget.xml();
    out.raw(DECL);
    let _ = write!(out, "<w:{tag} {WORD_NAMESPACES}>");
    out.raw(prefix);
    out.append(inner);
    let _ = write!(out, "</w:{tag}>");
    out.into_string()
}

impl Renderer<'_> {
    pub(super) fn finish(
        mut self,
        mut reference: Reference,
        document: String,
        meta: &Meta,
        stamp: i64,
    ) -> Result<Vec<u8>> {
        let remaining = reference.remaining_bytes(self.context);
        if document.len() > remaining {
            return Err(size_error());
        }
        let base_rel_count = STANDARD_RELS.len() + reference.relations.len();
        let sources: BTreeMap<_, _> = self
            .media
            .iter()
            .map(|(source, image)| (image.target.as_str(), source.as_str()))
            .collect();
        self.relations[base_rel_count..].sort_by(|a, b| {
            let key = |rel: &Relationship| {
                // Image relationships precede hyperlinks.
                (
                    usize::from(rel.external),
                    sources.get(rel.target.as_str()).copied(),
                )
            };
            key(a).cmp(&key(b)).then_with(|| a.target.cmp(&b.target))
        });
        let budget = &self.budget;
        let mut media = BTreeMap::new();
        for image in self.media.into_values() {
            let path = format!("word/{}", image.target);
            reference.types.insert(path.clone(), image.mime);
            media.insert(path, image.bytes);
        }
        let root_rels = [
            relationship(1, "officeDocument", DOC, false),
            relationship(4, "extended-properties", "docProps/app.xml", false),
            Relationship {
                kind: "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties".into(),
                ..relationship(3, "core-properties", "docProps/core.xml", false)
            },
            relationship(5, "custom-properties", "docProps/custom.xml", false),
        ];
        let total = self.context.limits.max_archive_size;
        let max_entries = self.context.limits.max_archive_entries;
        if 16usize
            .saturating_add(
                reference
                    .parts
                    .keys()
                    .filter(|key| !COPIED.contains(&key.as_str()))
                    .count(),
            )
            .saturating_add(media.len())
            > max_entries
        {
            return Err(Error::Parse("DOCX archive entry limit exceeded.".into()));
        }
        let mut archive = ArchiveWriter::with_input_limit(total);
        let mut write = |name: &str, bytes: &[u8]| {
            archive
                .write_deflated_with_time(name, bytes, Some(stamp))
                .map_err(zip_error)
        };
        let types = content_types(budget, &reference, &media)?;
        write(TYPES, types.as_bytes())?;
        write("_rels/.rels", relationships(budget, &root_rels)?.as_bytes())?;
        write(DOC, document.as_bytes())?;
        write(DOC_RELS, relationships(budget, &self.relations)?.as_bytes())?;
        write(
            NOTE_RELS,
            relationships(
                budget,
                self.relations
                    .iter()
                    .filter(|rel| is_kind(rel, "hyperlink")),
            )?
            .as_bytes(),
        )?;
        write(
            NUMBERING,
            self.numbering
                .finish(reference.numbering.take(), budget)?
                .as_bytes(),
        )?;
        write(
            STYLES,
            self.styles
                .finish(reference.styles.take(), budget)?
                .as_bytes(),
        )?;
        write(
            "word/footnotes.xml",
            wrapped(budget, "footnotes", super::notes::SEPARATORS, self.notes)?.as_bytes(),
        )?;
        write(
            "word/comments.xml",
            wrapped(budget, "comments", "", self.comments)?.as_bytes(),
        )?;
        write(
            "docProps/core.xml",
            super::metadata::core(budget, meta, stamp)?.as_bytes(),
        )?;
        write(
            "docProps/custom.xml",
            super::metadata::custom(budget, meta)?.as_bytes(),
        )?;

        for name in COPIED {
            if let Some(bytes) = reference.parts.remove(name) {
                write(name, &bytes)?;
            } else {
                write(name, fallback(name).as_bytes())?;
            }
        }
        for (name, bytes) in reference.parts.into_iter().chain(media) {
            write(&name, &bytes)?;
        }
        let bytes = archive.finish().map_err(zip_error)?;
        if bytes.len() as u64 > total {
            return Err(size_error());
        }
        Ok(bytes)
    }
}

fn content_types(
    budget: &Budget,
    reference: &Reference,
    media: &BTreeMap<String, Vec<u8>>,
) -> Result<String> {
    let mut out = budget.xml();
    out.raw(DECL);
    out.raw(r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="odttf" ContentType="application/vnd.openxmlformats-officedocument.obfuscatedFont"/>"#);
    for (name, mime) in TYPE_OVERRIDES {
        content_type(&mut out, name, mime);
    }
    for name in reference.parts.keys().chain(media.keys()) {
        if COPIED.contains(&name.as_str()) || name.ends_with(".rels") {
            continue;
        }
        let mime = reference
            .types
            .get(name)
            .map(String::as_str)
            .or_else(|| mime_from_extension(name))
            .unwrap_or_else(|| {
                if name.starts_with("word/header") {
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"
                } else if name.starts_with("word/footer") {
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"
                } else if name.ends_with(".xml") {
                    "application/xml"
                } else {
                    "application/octet-stream"
                }
            });
        content_type(&mut out, name, mime);
    }
    out.raw("</Types>");
    out.into_string()
}

fn content_type(out: &mut Xml, name: &str, mime: &str) {
    out.raw("<Override PartName=\"/");
    out.escaped(name);
    out.raw("\" ContentType=\"");
    out.escaped(mime);
    out.raw("\"/>");
}

fn fallback(name: &str) -> String {
    match name {
        "word/settings.xml" => format!(
            r#"{DECL}<w:settings xmlns:w="{W}"><w:footnotePr><w:footnote w:id="-1"/><w:footnote w:id="0"/></w:footnotePr></w:settings>"#
        ),
        "docProps/app.xml" => format!(
            r#"{DECL}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"/>"#
        ),
        "word/theme/theme1.xml" => theme(),
        "word/fontTable.xml" => format!(
            r#"{DECL}<w:fonts xmlns:w="{W}"><w:font w:name="Courier New"/><w:font w:name="Symbol"/></w:fonts>"#
        ),
        _ => format!(r#"{DECL}<w:webSettings xmlns:w="{W}"/>"#),
    }
}

fn theme() -> String {
    let mut out = format!(
        r#"{DECL}<a:theme xmlns:a="{A}" name="Default"><a:themeElements><a:clrScheme name="Default">"#
    );
    for (name, color) in [
        ("dk1", "000000"),
        ("lt1", "FFFFFF"),
        ("dk2", "222222"),
        ("lt2", "EEEEEE"),
        ("accent1", "0066AA"),
        ("accent2", "AA4422"),
        ("accent3", "557733"),
        ("accent4", "664488"),
        ("accent5", "228888"),
        ("accent6", "AA7733"),
        ("hlink", "0563C1"),
        ("folHlink", "7030A0"),
    ] {
        let _ = write!(out, "<a:{name}><a:srgbClr val=\"{color}\"/></a:{name}>");
    }
    out.push_str("</a:clrScheme><a:fontScheme name=\"Default\">");
    for (tag, font) in [("majorFont", "Arial"), ("minorFont", "Arial")] {
        let _ = write!(
            out,
            "<a:{tag}><a:latin typeface=\"{font}\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:{tag}>"
        );
    }
    out.push_str("</a:fontScheme><a:fmtScheme name=\"Default\">");
    for tag in [
        "fillStyleLst",
        "lnStyleLst",
        "effectStyleLst",
        "bgFillStyleLst",
    ] {
        let _ = write!(out, "<a:{tag}>");
        for _ in 0..3 {
            match tag {
                "lnStyleLst" => out.push_str(r#"<a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>"#),
                "effectStyleLst" => out.push_str("<a:effectStyle><a:effectLst/></a:effectStyle>"),
                _ => out.push_str(r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#),
            }
        }
        let _ = write!(out, "</a:{tag}>");
    }
    out.push_str("</a:fmtScheme></a:themeElements></a:theme>");
    out
}
