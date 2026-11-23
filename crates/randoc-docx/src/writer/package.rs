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

const SETTINGS: &str = "word/settings.xml";

/// The children of `w:settings` that Pandoc takes from the reference, in its
/// order: the order of the schema without `footnotePr`, `endnotePr`, `mathPr`
/// and `schemaLibrary`. Pandoc takes each missing child from its own
/// reference document, a GPL data file (I-DATA-01); Randoc leaves it out.
const SETTINGS_CHILDREN: [&str; 94] = [
    "writeProtection",
    "view",
    "zoom",
    "removePersonalInformation",
    "removeDateAndTime",
    "doNotDisplayPageBoundaries",
    "displayBackgroundShape",
    "printPostScriptOverText",
    "printFractionalCharacterWidth",
    "printFormsData",
    "embedTrueTypeFonts",
    "embedSystemFonts",
    "saveSubsetFonts",
    "saveFormsData",
    "mirrorMargins",
    "alignBordersAndEdges",
    "bordersDoNotSurroundHeader",
    "bordersDoNotSurroundFooter",
    "gutterAtTop",
    "hideSpellingErrors",
    "hideGrammaticalErrors",
    "activeWritingStyle",
    "proofState",
    "formsDesign",
    "attachedTemplate",
    "linkStyles",
    "stylePaneFormatFilter",
    "stylePaneSortMethod",
    "documentType",
    "mailMerge",
    "revisionView",
    "trackRevisions",
    "doNotTrackMoves",
    "doNotTrackFormatting",
    "documentProtection",
    "autoFormatOverride",
    "styleLockTheme",
    "styleLockQFSet",
    "defaultTabStop",
    "autoHyphenation",
    "consecutiveHyphenLimit",
    "hyphenationZone",
    "doNotHyphenateCaps",
    "showEnvelope",
    "summaryLength",
    "clickAndTypeStyle",
    "defaultTableStyle",
    "evenAndOddHeaders",
    "bookFoldRevPrinting",
    "bookFoldPrinting",
    "bookFoldPrintingSheets",
    "drawingGridHorizontalSpacing",
    "drawingGridVerticalSpacing",
    "displayHorizontalDrawingGridEvery",
    "displayVerticalDrawingGridEvery",
    "doNotUseMarginsForDrawingGridOrigin",
    "drawingGridHorizontalOrigin",
    "drawingGridVerticalOrigin",
    "doNotShadeFormData",
    "noPunctuationKerning",
    "characterSpacingControl",
    "printTwoOnOne",
    "strictFirstAndLastChars",
    "noLineBreaksAfter",
    "noLineBreaksBefore",
    "savePreviewPicture",
    "doNotValidateAgainstSchema",
    "saveInvalidXml",
    "ignoreMixedContent",
    "alwaysShowPlaceholderText",
    "doNotDemarcateInvalidXml",
    "saveXmlDataOnly",
    "useXSLTWhenSaving",
    "saveThroughXslt",
    "showXMLTags",
    "alwaysMergeEmptyNamespace",
    "updateFields",
    "hdrShapeDefaults",
    "compat",
    "docVars",
    "rsids",
    "attachedSchema",
    "themeFontLang",
    "clrSchemeMapping",
    "doNotIncludeSubdocsInStats",
    "doNotAutoCompressPictures",
    "forceUpgrade",
    "captions",
    "readModeInkLockDown",
    "smartTagType",
    "shapeDefaults",
    "doNotEmbedSmartTags",
    "decimalSymbol",
    "listSeparator",
];

const SETTINGS_NAMESPACES: &str = concat!(
    r#"xmlns:o="urn:schemas-microsoft-com:office:office" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" "#,
    r#"xmlns:v="urn:schemas-microsoft-com:vml" "#,
    r#"xmlns:w10="urn:schemas-microsoft-com:office:word" "#,
    r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
    r#"xmlns:sl="http://schemas.openxmlformats.org/schemaLibrary/2006/main""#,
);

const COPIED: [&str; 5] = [
    SETTINGS,
    "docProps/app.xml",
    "word/theme/theme1.xml",
    "word/fontTable.xml",
    "word/webSettings.xml",
];

const FALLBACK_SECTION: &str = "<w:sectPr>\n  <w:footnotePr>\n    <w:numRestart w:val=\"eachSect\" />\n  </w:footnotePr>\n</w:sectPr>";

pub(super) struct Reference {
    pub(super) section: String,
    pub(super) styles: Option<XmlDoc>,
    pub(super) numbering: Option<XmlDoc>,
    settings: Option<XmlDoc>,
    parts: BTreeMap<String, Vec<u8>>,
    /// The names of `parts` in the order of discovery: Pandoc writes the
    /// parts of the reference in this order.
    order: Vec<String>,
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
            settings: None,
            parts: BTreeMap::new(),
            order: Vec::new(),
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
                    let xml = parse_part(&bytes, &path, context)?;
                    if name == SETTINGS {
                        self.settings = Some(xml);
                    }
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
            // Pandoc keeps only the child elements of the section properties.
            let section = XmlElement {
                children: section
                    .children
                    .iter()
                    .filter(|node| matches!(node, XmlNode::Element(_)))
                    .cloned()
                    .collect(),
                ..section.clone()
            };
            let budget = Budget::new(to_usize(context.limits.max_archive_size));
            let mut out = budget.xml();
            write_pretty(&mut out, &section);
            let mut text = out.into_string()?;
            text.truncate(text.trim_end_matches('\n').len());
            self.section = text;
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
        if self.parts.insert(name.clone(), bytes).is_none() {
            self.order.push(name);
        }
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

/// Writes a relationships part. The package relationships and those of the
/// reference document give `Id` first; the relationships that Pandoc makes
/// for the document and the notes give `Type` first.
fn relationships<'a>(
    budget: &Budget,
    relations: impl IntoIterator<Item = &'a Relationship>,
    type_first: impl Fn(&Relationship) -> bool,
) -> Result<String> {
    let mut out = budget.xml();
    out.raw(DECL);
    let _ = write!(out, "<Relationships xmlns=\"{RELS_NS}\"");
    let mut relations = relations.into_iter().peekable();
    if relations.peek().is_none() {
        out.raw(" />");
        return out.into_string();
    }
    out.raw(">");
    for rel in relations {
        let (first, second) = if type_first(rel) {
            (("Type", &rel.kind), ("Id", &rel.id))
        } else {
            (("Id", &rel.id), ("Type", &rel.kind))
        };
        for (index, (name, value)) in [first, second].into_iter().enumerate() {
            out.raw(if index == 0 { "<Relationship " } else { "\" " });
            out.raw(name);
            out.raw("=\"");
            out.escaped(value);
        }
        out.raw("\" Target=\"");
        out.escaped(&rel.target);
        if rel.external {
            out.raw("\" TargetMode=\"External");
        }
        out.raw("\" />");
    }
    out.raw("</Relationships>");
    out.into_string()
}

/// Writes the start tag of `element`: its name, its namespace declarations
/// and its attributes, as Pandoc writes an element of a reference document.
pub(super) fn start_tag(out: &mut Xml, element: &XmlElement) {
    out.raw("<");
    name(out, &element.name);
    for (prefix, uri) in &element.ns_decls {
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
}

pub(super) fn end_tag(out: &mut Xml, element: &XmlElement) {
    out.raw("</");
    name(out, &element.name);
    out.raw(">");
}

enum Step<'a> {
    Element(&'a XmlElement),
    Text(&'a str),
    Close(&'a XmlElement),
}

impl<'a> From<&'a XmlNode> for Step<'a> {
    fn from(node: &'a XmlNode) -> Self {
        match node {
            XmlNode::Element(element) => Step::Element(element),
            XmlNode::Text(text) => Step::Text(text),
        }
    }
}

/// Writes nodes of a reference document on one line, text included.
pub(super) fn write_nodes<'a>(out: &mut Xml, nodes: impl IntoIterator<Item = &'a XmlNode>) {
    write_steps(out, nodes.into_iter().map(Step::from).collect());
}

/// Writes elements of a reference document on one line.
pub(super) fn write_elements<'a>(
    out: &mut Xml,
    elements: impl IntoIterator<Item = &'a XmlElement>,
) {
    write_steps(out, elements.into_iter().map(Step::Element).collect());
}

fn write_steps(out: &mut Xml, mut stack: Vec<Step<'_>>) {
    stack.reverse();
    while let Some(step) = stack.pop() {
        match step {
            Step::Text(text) => out.escaped(text),
            Step::Element(element) => {
                start_tag(out, element);
                if element.children.is_empty() {
                    out.raw(" />");
                } else {
                    out.raw(">");
                    stack.push(Step::Close(element));
                    stack.extend(element.children.iter().rev().map(Step::from));
                }
            }
            Step::Close(element) => end_tag(out, element),
        }
    }
}

/// Writes `element` with one line for each child element and two spaces of
/// indentation for each level, as Pandoc writes the section properties.
fn write_pretty(out: &mut Xml, element: &XmlElement) {
    enum Step<'a> {
        Element(&'a XmlElement, usize),
        Text(&'a str, usize),
        Close(&'a XmlElement, usize),
    }
    let indent = |out: &mut Xml, level: usize| {
        for _ in 0..level {
            out.raw("  ");
        }
    };
    let mut stack = vec![Step::Element(element, 0)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Text(text, level) => {
                for (index, line) in text.split('\n').enumerate() {
                    if index > 0 {
                        out.raw("\n");
                    }
                    indent(out, level);
                    out.escaped(line);
                }
                out.raw("\n");
            }
            Step::Element(element, level) => {
                indent(out, level);
                start_tag(out, element);
                match element.children.as_slice() {
                    [] => out.raw(" />\n"),
                    [XmlNode::Text(text)] => {
                        out.raw(">");
                        out.escaped(text);
                        end_tag(out, element);
                        out.raw("\n");
                    }
                    children => {
                        out.raw(">\n");
                        stack.push(Step::Close(element, level));
                        stack.extend(children.iter().rev().map(|child| match child {
                            XmlNode::Element(child) => Step::Element(child, level + 1),
                            XmlNode::Text(text) => Step::Text(text, level + 1),
                        }));
                    }
                }
            }
            Step::Close(element, level) => {
                indent(out, level);
                end_tag(out, element);
                out.raw("\n");
            }
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
    let _ = write!(out, "<w:{tag} {WORD_NAMESPACES}");
    if prefix.is_empty() && inner.is_empty() {
        out.raw(" />");
    } else {
        out.raw(">");
        out.raw(prefix);
        out.append(inner);
        let _ = write!(out, "</w:{tag}>");
    }
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
        write(
            "_rels/.rels",
            relationships(budget, &root_rels, |_| false)?.as_bytes(),
        )?;
        write(DOC, document.as_bytes())?;
        let inherited: HashSet<&str> = reference
            .relations
            .iter()
            .map(|rel| rel.id.as_str())
            .collect();
        write(
            DOC_RELS,
            relationships(budget, &self.relations, |rel| {
                !inherited.contains(rel.id.as_str())
            })?
            .as_bytes(),
        )?;
        write(
            NOTE_RELS,
            relationships(
                budget,
                self.relations
                    .iter()
                    .filter(|rel| is_kind(rel, "hyperlink")),
                |_| true,
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

        // Pandoc writes the media after the settings part.
        let settings = match reference.settings.take() {
            Some(settings) => {
                reference.parts.remove(SETTINGS);
                settings_part(budget, &settings)?.into_bytes()
            }
            None => reference
                .parts
                .remove(SETTINGS)
                .unwrap_or_else(|| fallback(SETTINGS).into_bytes()),
        };
        write(SETTINGS, &settings)?;
        for (name, bytes) in &media {
            write(name, bytes)?;
        }
        for name in COPIED.iter().filter(|name| **name != SETTINGS) {
            let bytes = reference
                .parts
                .remove(*name)
                .unwrap_or_else(|| fallback(name).into_bytes());
            write(name, &bytes)?;
        }
        for name in &reference.order {
            if let Some(bytes) = reference.parts.get(name) {
                write(name, bytes)?;
            }
        }
        let bytes = archive.finish().map_err(zip_error)?;
        if bytes.len() as u64 > total {
            return Err(size_error());
        }
        Ok(bytes)
    }
}

fn settings_part(budget: &Budget, reference: &XmlDoc) -> Result<String> {
    let mut out = budget.xml();
    out.raw(DECL);
    out.raw("<w:settings ");
    out.raw(SETTINGS_NAMESPACES);
    let children: Vec<_> = SETTINGS_CHILDREN
        .iter()
        .flat_map(|name| reference.root.children_ns(W, name))
        .collect();
    if children.is_empty() {
        out.raw(" />");
    } else {
        out.raw(">");
        write_elements(&mut out, children);
        out.raw("</w:settings>");
    }
    out.into_string()
}

fn content_types(
    budget: &Budget,
    reference: &Reference,
    media: &BTreeMap<String, Vec<u8>>,
) -> Result<String> {
    let mut out = budget.xml();
    out.raw(DECL);
    out.raw(r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml" /><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" /><Default Extension="odttf" ContentType="application/vnd.openxmlformats-officedocument.obfuscatedFont" />"#);
    for (name, mime) in TYPE_OVERRIDES {
        content_type(&mut out, name, mime);
    }
    let parts = reference
        .order
        .iter()
        .filter(|name| reference.parts.contains_key(*name));
    for name in parts.chain(media.keys()) {
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
    out.raw("\" />");
}

fn fallback(name: &str) -> String {
    match name {
        "word/settings.xml" => format!(
            r#"{DECL}<w:settings xmlns:w="{W}"><w:footnotePr><w:footnote w:id="-1" /><w:footnote w:id="0" /></w:footnotePr></w:settings>"#
        ),
        "docProps/app.xml" => format!(
            r#"{DECL}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" />"#
        ),
        "word/theme/theme1.xml" => theme(),
        "word/fontTable.xml" => format!(
            r#"{DECL}<w:fonts xmlns:w="{W}"><w:font w:name="Courier New" /><w:font w:name="Symbol" /></w:fonts>"#
        ),
        _ => format!(r#"{DECL}<w:webSettings xmlns:w="{W}" />"#),
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
        let _ = write!(out, "<a:{name}><a:srgbClr val=\"{color}\" /></a:{name}>");
    }
    out.push_str("</a:clrScheme><a:fontScheme name=\"Default\">");
    for (tag, font) in [("majorFont", "Arial"), ("minorFont", "Arial")] {
        let _ = write!(
            out,
            "<a:{tag}><a:latin typeface=\"{font}\" /><a:ea typeface=\"\" /><a:cs typeface=\"\" /></a:{tag}>"
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
                "lnStyleLst" => out.push_str(r#"<a:ln w="9525"><a:solidFill><a:schemeClr val="phClr" /></a:solidFill></a:ln>"#),
                "effectStyleLst" => out.push_str("<a:effectStyle><a:effectLst /></a:effectStyle>"),
                _ => out.push_str(r#"<a:solidFill><a:schemeClr val="phClr" /></a:solidFill>"#),
            }
        }
        let _ = write!(out, "</a:{tag}>");
    }
    out.push_str("</a:fmtScheme></a:themeElements></a:theme>");
    out
}
