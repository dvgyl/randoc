use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write;
use std::path::Path;

use randoc_archive::media::{
    base64_encode, decode_data_uri, decode_percent_bytes, extension_for_mime, image_size_and_mime,
    is_safe_media_path, mime_from_extension,
};
use randoc_archive::xml::{self, XmlDoc, XmlElement, XmlLimits, XmlNode, XmlOptions};
use randoc_archive::zip::{Archive, ArchiveError, ArchiveLimits, ArchiveWriter};
use randoc_ast::{Attr, Inline, MathType, Meta, MetaValue, Pandoc, Target};
use randoc_citeproc::{Lang, parse_lang};
use randoc_core::shared::stringify;
use randoc_core::{Context, Error, LogMessage, Result, WriterOptions};
use randoc_highlight::{Style, TokenType};
use randoc_layout::{Doc, text};

use crate::ns::{self, OFFICE, STYLE, is_el};

use super::{Format, Renderer, attribute, escaped, render, show, span};

const MIME: &str = "application/vnd.oasis.opendocument.text";
const FORMULA_MIME: &str = "application/vnd.oasis.opendocument.formula";
const XML: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n";
const MANIFEST_FILE: &str = "META-INF/manifest.xml";
const XLINK: &str = "http://www.w3.org/1999/xlink";
const OOO: &str = "http://openoffice.org/2004/office";
const GRDDL: &str = "http://www.w3.org/2003/g/data-view#";

#[derive(Default)]
pub(super) struct Resources {
    parts: BTreeMap<String, Part>,
    /// The parts that the conversion adds, in order. Pandoc puts each new
    /// part in front of the others, so the archive holds them in reverse.
    added: Vec<String>,
    /// The entries of the reference document in its order. Pandoc keeps
    /// this order for the entries that it does not add.
    layout: Vec<String>,
    next_index: usize,
    added_bytes: u64,
}

/// The entries of the reference document when the conversion has none.
/// Pandoc's own reference gives this order.
const DEFAULT_LAYOUT: [&str; 3] = ["manifest.rdf", "styles.xml", "meta.xml"];

struct Part {
    media_type: String,
    bytes: Vec<u8>,
}

impl Resources {
    /// Adds a part that the conversion makes, such as a picture or a formula.
    fn add(&mut self, path: String, media_type: impl Into<String>, bytes: Vec<u8>) {
        self.added.push(path.clone());
        self.insert(path, media_type, bytes);
    }

    fn insert(&mut self, path: String, media_type: impl Into<String>, bytes: Vec<u8>) {
        self.parts.insert(
            path,
            Part {
                media_type: media_type.into(),
                bytes,
            },
        );
    }

    fn charge(&mut self, context: &Context, bytes: usize, entries: usize) -> Result<()> {
        self.added_bytes = self.added_bytes.saturating_add(bytes as u64);
        if self.added_bytes > context.limits.max_archive_size
            || self.parts.len().saturating_add(entries) > context.limits.max_archive_entries
        {
            return Err(Error::App(
                "The OpenDocument package limit was exceeded.".into(),
            ));
        }
        Ok(())
    }
}

pub(super) fn write_odt(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<Vec<u8>> {
    let (reference, resources) = reference(options, context, true)?;
    let (content, mut resources) = render(doc, options, context, Format::Odt, resources)?;
    let stamp = timestamp(context);
    let meta = meta_xml(&doc.meta, stamp);
    let lang = document_language(&doc.meta, context);
    let styles = styles_xml(reference, options, lang.as_ref(), context)?;
    resources.charge(
        context,
        content
            .len()
            .saturating_add(styles.len())
            .saturating_add(meta.len()),
        5,
    )?;
    resources.insert(
        "content.xml".into(),
        "application/xml",
        content.into_bytes(),
    );
    resources.insert("styles.xml".into(), "application/xml", styles.into_bytes());
    resources.insert("meta.xml".into(), "application/xml", meta.into_bytes());
    let manifest = manifest(&resources);
    resources.charge(context, manifest.len().saturating_add(MIME.len()), 2)?;

    let mut archive = ArchiveWriter::new();
    archive
        .write_stored_with_time("mimetype", MIME.as_bytes(), Some(stamp))
        .map_err(zip_error)?;
    let mut parts = resources.parts;
    let mut write = |name: &str, bytes: &[u8]| {
        archive
            .write_deflated_with_time(name, bytes, Some(stamp))
            .map_err(zip_error)
    };
    let meta = parts
        .remove("meta.xml")
        .map(|part| part.bytes)
        .unwrap_or_default();
    write("meta.xml", &meta)?;
    write(MANIFEST_FILE, manifest.as_bytes())?;
    for name in std::iter::once("content.xml")
        .chain(resources.added.iter().rev().map(String::as_str))
        .chain(resources.layout.iter().map(String::as_str))
    {
        if let Some(part) = parts.remove(name) {
            write(name, &part.bytes)?;
        }
    }
    for (name, part) in parts {
        write(&name, &part.bytes)?;
    }
    archive.finish().map_err(zip_error)
}

/// Writes `meta.xml` with one line for each element, as Pandoc does.
fn meta_xml(meta: &Meta, stamp: i64) -> String {
    let mut out = format!(
        "{XML}<office:document-meta xmlns:office=\"{OFFICE}\" xmlns:xlink=\"{XLINK}\" xmlns:dc=\"{}\" xmlns:meta=\"{}\" xmlns:ooo=\"{OOO}\" xmlns:grddl=\"{GRDDL}\" office:version=\"1.3\">\n  <office:meta>\n",
        ns::DC,
        ns::META
    );
    for element in metadata(meta, stamp) {
        let _ = writeln!(out, "    {element}");
    }
    out.push_str("  </office:meta>\n</office:document-meta>");
    out
}

pub(super) fn write_fodt(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let (reference, resources) = reference(options, context, false)?;
    let (content, _) = render(doc, options, context, Format::Fodt, resources)?;
    let lang = document_language(&doc.meta, context);
    let styles = styles_xml(reference, options, lang.as_ref(), context)?;
    let content = parse_xml(&content, "content.xml", context)?.root;
    if !is_el(&content, OFFICE, "document-content") && !is_el(&content, OFFICE, "document") {
        return Err(Error::App(
            "The FODT template must produce an OpenDocument root element.".into(),
        ));
    }
    let meta = meta_xml(&doc.meta, timestamp(context));
    let meta = parse_xml(&meta, "meta.xml", context)?.root;
    let styles = parse_xml(&styles, "styles.xml", context)?.root;
    Ok(flat_document(content, &styles, meta))
}

/// Joins the parts of an ODT package into one flat document, as Pandoc does.
/// The root has the attributes and namespace declarations of all parts in
/// the order of their names. The sections of `content.xml` come first in
/// each section, then the elements of the same section in `styles.xml`.
fn flat_document(content: XmlElement, styles: &XmlElement, meta: XmlElement) -> String {
    let mut attributes = BTreeMap::new();
    for root in [&content, styles, &meta] {
        for (prefix, uri) in &root.ns_decls {
            let name = prefix
                .as_ref()
                .map_or_else(|| "xmlns".to_owned(), |prefix| format!("xmlns:{prefix}"));
            attributes.entry(name).or_insert_with(|| uri.to_string());
        }
        for (name, value) in &root.attrs {
            attributes
                .entry(qualified_name(name))
                .or_insert_with(|| value.to_string());
        }
    }
    let office = content
        .name
        .prefix
        .as_ref()
        .map(|prefix| prefix.to_string())
        .or_else(|| {
            attributes.iter().find_map(|(name, uri)| {
                name.strip_prefix("xmlns:")
                    .filter(|_| uri == OFFICE)
                    .map(str::to_owned)
            })
        })
        .unwrap_or_else(|| {
            attributes.insert("xmlns:odf".into(), OFFICE.into());
            "odf".into()
        });
    attributes
        .entry(format!("{office}:mimetype"))
        .or_insert_with(|| MIME.into());
    let root = match &content.name.prefix {
        Some(prefix) => format!("{prefix}:document"),
        None => "document".into(),
    };

    let section = |root: &XmlElement, name: &str| {
        root.elements()
            .find(|element| is_el(element, OFFICE, name))
            .cloned()
    };
    let mut sections = vec![
        meta.elements()
            .find(|element| is_el(element, OFFICE, "meta"))
            .cloned(),
    ];
    for name in [
        "font-face-decls",
        "styles",
        "automatic-styles",
        "master-styles",
    ] {
        sections.push(match (section(&content, name), section(styles, name)) {
            (Some(mut generated), Some(inherited)) => {
                let fonts: HashSet<String> = generated
                    .elements()
                    .filter_map(|font| font.attr(STYLE, "name").map(str::to_owned))
                    .collect();
                generated.children.extend(
                    inherited
                        .elements()
                        .filter(|element| {
                            name != "font-face-decls"
                                || element
                                    .attr(STYLE, "name")
                                    .is_none_or(|font| !fonts.contains(font))
                        })
                        .cloned()
                        .map(XmlNode::Element),
                );
                Some(generated)
            }
            (generated, inherited) => generated.or(inherited),
        });
    }
    let merged = [
        "meta",
        "font-face-decls",
        "styles",
        "automatic-styles",
        "master-styles",
    ];
    let rest = content
        .elements()
        .filter(|element| !merged.iter().any(|name| is_el(element, OFFICE, name)))
        .cloned()
        .map(Some);

    let mut out = format!("{}<{root}", show::DECLARATION);
    for (name, value) in &attributes {
        let _ = write!(out, " {name}=\"");
        show::escape(&mut out, value);
        out.push('"');
    }
    out.push('>');
    for element in sections.into_iter().chain(rest).flatten() {
        show::element(&mut out, &element);
    }
    let _ = write!(out, "</{root}>");
    out
}

fn qualified_name(name: &xml::QName) -> String {
    match &name.prefix {
        Some(prefix) => format!("{prefix}:{}", name.local),
        None => name.local.to_string(),
    }
}

fn zip_error(error: ArchiveError) -> Error {
    Error::App(format!("Could not write ODT: {error}"))
}

fn parse_xml(value: &str, name: &str, context: &Context) -> Result<XmlDoc> {
    xml::parse(
        value,
        &XmlLimits::from_limits(&context.limits),
        XmlOptions::default(),
    )
    .map_err(|error| Error::Xml {
        file: format!("'{name}'"),
        message: error.format_pandoc(),
    })
}

fn reference(
    options: &WriterOptions,
    context: &mut Context,
    copy_parts: bool,
) -> Result<(Option<Vec<u8>>, Resources)> {
    let data_dir = context.state.user_data_dir.clone();
    let candidate = data_dir.as_ref().map(|dir| format!("{dir}/reference.odt"));
    let explicit = options.reference_doc.as_deref();
    let path = if let Some(name) = explicit {
        Some(reference_path(name, context)?)
    } else {
        candidate.filter(|path| is_file(context, path))
    };
    let mut resources = Resources::default();
    let limit = usize::try_from(context.limits.max_resource_size).unwrap_or(usize::MAX);
    if let Some(path) = path {
        let input = context.read_file_limited(&path, limit)?;
        let mut limits = ArchiveLimits::from_limits(&context.limits);
        limits.max_input = context.limits.max_resource_size;
        let mut archive = Archive::open(&input, limits).map_err(|error| {
            Error::Parse(format!("Could not parse reference-doc {path:?}: {error}"))
        })?;
        let styles = archive.read("styles.xml").map_err(zip_error)?;
        resources.layout = archive
            .entry_names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        if copy_parts {
            let names: Vec<_> = archive
                .entry_names()
                .into_iter()
                .filter(|name| is_safe_media_path(name) && !reserved_part(name))
                .map(str::to_owned)
                .collect();
            for name in names {
                if let Some(bytes) = archive.read(&name).map_err(zip_error)? {
                    resources.charge(context, bytes.len(), 1)?;
                    resources.insert(name.clone(), part_type(&name), bytes);
                }
            }
        }
        return Ok((styles, resources));
    }
    let mut styles = None;
    resources.layout = DEFAULT_LAYOUT.map(str::to_owned).to_vec();
    if let Some(dir) = data_dir {
        let path = format!("{dir}/odt/styles.xml");
        if is_file(context, &path) {
            styles = Some(context.read_file_limited(&path, limit)?);
        }
        if copy_parts {
            for name in ["manifest.rdf", "settings.xml"] {
                let path = format!("{dir}/odt/{name}");
                if is_file(context, &path) {
                    let bytes = context.read_file_limited(&path, limit)?;
                    resources.charge(context, bytes.len(), 1)?;
                    resources.insert(name.into(), part_type(name), bytes);
                    if !resources.layout.iter().any(|entry| entry == name) {
                        resources.layout.push(name.into());
                    }
                }
            }
        }
    }
    if copy_parts && !resources.parts.contains_key("manifest.rdf") {
        let rdf = concat!(
            "<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\" ",
            "xmlns:pkg=\"http://docs.oasis-open.org/ns/office/1.2/meta/pkg#\">",
            "<pkg:Document rdf:about=\"\"><pkg:hasPart rdf:resource=\"content.xml\" />",
            "<pkg:hasPart rdf:resource=\"styles.xml\" /></pkg:Document></rdf:RDF>",
        );
        resources.charge(context, rdf.len(), 1)?;
        resources.insert(
            "manifest.rdf".into(),
            "application/rdf+xml",
            rdf.as_bytes().to_vec(),
        );
    }
    Ok((styles, resources))
}

fn reference_path(name: &str, context: &mut Context) -> Result<String> {
    if let Some(uri) = name.strip_prefix("file://") {
        let path = uri
            .strip_prefix("localhost")
            .filter(|path| path.starts_with('/'))
            .unwrap_or(uri);
        let path = path.split(['?', '#']).next().unwrap_or_default();
        if path.starts_with('/')
            && valid_uri_component(path, b":@/")
            && let Some(bytes) = decode_percent_bytes(path, path.len())
            && !bytes.contains(&0)
            && let Ok(path) = String::from_utf8(bytes)
        {
            return Ok(path);
        }
        return Err(Error::ResourceNotFound(name.to_owned()));
    }
    if Path::new(name).is_absolute() {
        return Ok(name.to_owned());
    }
    for dir in context.state.resource_path.clone() {
        let path = if dir == "." {
            name.to_owned()
        } else {
            Path::new(&dir).join(name).to_string_lossy().into_owned()
        };
        if is_file(context, &path) {
            return Ok(path);
        }
    }
    Err(Error::ResourceNotFound(name.to_owned()))
}

fn reserved_part(path: &str) -> bool {
    matches!(
        path,
        "mimetype" | "content.xml" | "styles.xml" | "meta.xml" | MANIFEST_FILE
    )
}

fn is_file(context: &mut Context, path: &str) -> bool {
    matches!(
        context.host().path_kind(path, true),
        Ok(randoc_core::PathKind::File)
    )
}

fn part_type(path: &str) -> &'static str {
    if path.ends_with(".xml") {
        "application/xml"
    } else if path.ends_with(".rdf") {
        "application/rdf+xml"
    } else if path.ends_with(".txt") {
        "text/plain"
    } else {
        mime_from_extension(path).unwrap_or("application/octet-stream")
    }
}

/// Writes the manifest as Pandoc does: `content.xml`, the added parts in
/// reverse order, the other parts in the order of the reference, and then the
/// folder of each formula part, one time for each part in it.
fn manifest(resources: &Resources) -> String {
    let mut out = format!(
        "{XML}<manifest:manifest xmlns:manifest=\"{}\" manifest:version=\"1.3\">\n  <manifest:file-entry manifest:media-type=\"{MIME}\" manifest:full-path=\"/\" manifest:version=\"1.3\" />\n",
        ns::MANIFEST
    );
    let mut seen = HashSet::new();
    let listed: Vec<&str> = std::iter::once("content.xml")
        .chain(resources.added.iter().rev().map(String::as_str))
        .chain(resources.layout.iter().map(String::as_str))
        .chain(resources.parts.keys().map(String::as_str))
        .filter(|path| {
            *path != "mimetype"
                && *path != MANIFEST_FILE
                && resources.parts.contains_key(*path)
                && seen.insert(*path)
        })
        .collect();
    for path in &listed {
        let _ = writeln!(
            out,
            "  <manifest:file-entry manifest:media-type=\"{}\" manifest:full-path=\"{}\" />",
            escaped(&resources.parts[*path].media_type),
            escaped(path)
        );
    }
    for path in &listed {
        if path.starts_with("Formula-")
            && let Some((folder, _)) = path.split_once('/')
        {
            let _ = writeln!(
                out,
                "  <manifest:file-entry manifest:media-type=\"{FORMULA_MIME}\" manifest:full-path=\"{}/\" />",
                escaped(folder)
            );
        }
    }
    out.push_str("</manifest:manifest>");
    out
}

fn document_language(meta: &Meta, context: &mut Context) -> Option<Lang> {
    let value = meta.0.get("lang").and_then(meta_text)?;
    match parse_lang(&value) {
        Ok(lang) => Some(lang),
        Err(_) => {
            context.report(LogMessage::InvalidLang(value));
            None
        }
    }
}

fn localize_styles(element: &mut xml::XmlElement, lang: &Lang) {
    if is_el(element, STYLE, "text-properties") {
        for (name, value) in &mut element.attrs {
            if name.ns.as_deref() == Some(ns::FO) {
                match name.local.as_str() {
                    "language" => value.clone_from(&lang.language),
                    "country" => value.replace_range(.., lang.region.as_deref().unwrap_or("")),
                    _ => {}
                }
            }
        }
    }
    for child in &mut element.children {
        if let XmlNode::Element(child) = child {
            localize_styles(child, lang);
        }
    }
}

fn styles_xml(
    reference: Option<Vec<u8>>,
    options: &WriterOptions,
    lang: Option<&Lang>,
    context: &Context,
) -> Result<String> {
    let reference = reference
        .map(String::from_utf8)
        .transpose()
        .map_err(|error| Error::Xml {
            file: "'styles.xml'".into(),
            message: error.to_string(),
        })?;
    let additions = extra_styles(options);
    if let Some(reference) = reference {
        let mut root = parse_xml(&reference, "styles.xml", context)?.root;
        if let Some(lang) = lang {
            localize_styles(&mut root, lang);
        }
        let index =
            match root.children.iter().position(
                |node| matches!(node, XmlNode::Element(el) if is_el(el, OFFICE, "styles")),
            ) {
                Some(index) => index,
                None => {
                    let root_xml = format!("<office:styles xmlns:office=\"{OFFICE}\" />");
                    root.children.insert(
                        0,
                        XmlNode::Element(parse_xml(&root_xml, "styles.xml", context)?.root),
                    );
                    0
                }
            };
        if let Some(XmlNode::Element(styles)) = root.children.get_mut(index) {
            // Pandoc appends the styles also when the reference has a style
            // of the same name.
            for definition in additions {
                let definition = definition.replacen(
                    "<style:style",
                    &format!(
                        "<style:style xmlns:style=\"{STYLE}\" xmlns:fo=\"{}\"",
                        ns::FO
                    ),
                    1,
                );
                let mut element = parse_xml(&definition, "styles.xml", context)?.root;
                // The declarations only serve the parse: the root of the
                // reference declares these prefixes.
                element.ns_decls.clear();
                styles.children.push(XmlNode::Element(element));
            }
        }
        let mut out = show::DECLARATION.to_owned();
        show::element(&mut out, &root);
        return Ok(out);
    }

    let mut out = default_styles(lang);
    for style in additions {
        out.push_str(&style);
    }
    out.push_str("</office:styles><office:automatic-styles><style:page-layout style:name=\"PageLayout\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" fo:margin=\"2cm\" style:print-orientation=\"portrait\" /></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"PageLayout\" /></office:master-styles></office:document-styles>");
    Ok(out)
}

fn default_styles(lang: Option<&Lang>) -> String {
    let mut out = format!(
        "{XML}<office:document-styles xmlns:office=\"{OFFICE}\" xmlns:style=\"{STYLE}\" xmlns:text=\"{}\" xmlns:fo=\"{}\" xmlns:svg=\"{}\" office:version=\"1.3\"><office:font-face-decls><style:font-face style:name=\"Courier New\" svg:font-family=\"'Courier New'\" style:font-pitch=\"fixed\" /></office:font-face-decls><office:styles>",
        ns::TEXT,
        ns::FO,
        ns::SVG
    );
    let language = lang.map_or_else(String::new, |lang| attribute("fo:language", &lang.language));
    if let Some(lang) = lang {
        let country = attribute("fo:country", lang.region.as_deref().unwrap_or(""));
        let locale = format!("{language}{country}");
        let _ = write!(
            out,
            "<style:default-style style:family=\"paragraph\"><style:text-properties{locale} /></style:default-style>"
        );
    }
    for (name, props) in [
        ("Emphasis", "fo:font-style=\"italic\""),
        ("Strong_20_Emphasis", "fo:font-weight=\"bold\""),
        ("Strikeout", "style:text-line-through-style=\"solid\""),
        ("Superscript", "style:text-position=\"super 58%\""),
        ("Subscript", "style:text-position=\"sub 58%\""),
        ("Source_20_Text", "style:font-name=\"Courier New\""),
        ("Definition", ""),
        (
            "Internet_20_link",
            "fo:color=\"#000080\" style:text-underline-style=\"solid\"",
        ),
        ("Numbering_20_Symbols", ""),
    ] {
        let lang = if name == "Source_20_Text" {
            language.as_str()
        } else {
            ""
        };
        let _ = write!(
            out,
            "<style:style style:name=\"{name}\" style:family=\"text\"><style:text-properties {props}{lang}/></style:style>"
        );
    }
    for name in [
        "Standard",
        "Text_20_body",
        "First_20_paragraph",
        "Title",
        "Subtitle",
        "Author",
        "Date",
        "Abstract",
        "Footnote",
        "Quotations",
        "Preformatted_20_Text",
        "Horizontal_20_Line",
        "Definition_20_Term",
        "Definition_20_Definition",
        "Definition_20_Term_20_Tight",
        "Definition_20_Definition_20_Tight",
        "Table_20_Contents",
        "Table_20_Heading",
        "List_20_Number",
        "List_20_Number_20_Tight",
        "List_20_Bullet",
        "List_20_Bullet_20_Tight",
        "Figure",
        "FigureWithCaption",
        "Caption",
        "FigureCaption",
        "TableCaption",
        "Contents_20_Heading",
    ] {
        let parent = if name == "FigureCaption" {
            " style:parent-style-name=\"Caption\""
        } else {
            ""
        };
        let _ = write!(
            out,
            "<style:style style:name=\"{name}\" style:family=\"paragraph\"{parent}>"
        );
        out.push_str(match name {
            "Quotations" => "<style:paragraph-properties fo:margin-left=\"0.25in\" />",
            "Table_20_Heading" | "Title" => "<style:text-properties fo:font-weight=\"bold\" />",
            "Preformatted_20_Text" => "<style:text-properties style:font-name=\"Courier New\" />",
            "Horizontal_20_Line" => {
                "<style:paragraph-properties fo:border-bottom=\"0.5pt solid #000000\" />"
            }
            _ => "",
        });
        out.push_str("</style:style>");
    }
    out.push_str("<style:style style:name=\"Formula\" style:family=\"graphic\" />");
    for level in 1..=10 {
        let _ = write!(
            out,
            "<style:style style:name=\"Heading_20_{level}\" style:family=\"paragraph\" style:default-outline-level=\"{level}\"><style:text-properties fo:font-weight=\"bold\" /></style:style><style:style style:name=\"Contents_20_{level}\" style:family=\"paragraph\" />"
        );
    }
    for (name, kind, attrs) in [
        (
            "Numbering_20_1",
            "number",
            "style:num-format=\"1\" style:num-suffix=\".\"",
        ),
        ("List_20_1", "bullet", "text:bullet-char=\"•\""),
    ] {
        let _ = write!(out, "<text:list-style style:name=\"{name}\">");
        for level in 1..=10 {
            let indent = level * 5;
            let _ = write!(
                out,
                "<text:list-level-style-{kind} text:level=\"{level}\" {attrs}><style:list-level-properties text:space-before=\"{indent}mm\" text:min-label-width=\"5mm\" /></text:list-level-style-{kind}>"
            );
        }
        out.push_str("</text:list-style>");
    }
    out
}

/// Returns the styles that Pandoc adds to `styles.xml`: one text style for
/// each token type, in this order, when highlighting is on.
fn extra_styles(options: &WriterOptions) -> Vec<String> {
    let mut styles = Vec::new();
    let palette = options
        .highlight_theme
        .as_ref()
        .and_then(|file| std::str::from_utf8(&file.bytes).ok())
        .and_then(|value| Style::from_json(value).ok())
        .or_else(|| options.highlight_style.as_deref().and_then(Style::builtin));
    if let Some(palette) = palette {
        for kind in TokenType::ALL {
            let name = format!("{}Tok", kind.name());
            let style = palette.token_styles.get(&kind).copied().unwrap_or_default();
            let mut props = String::new();
            if let Some(color) = style.color.or(palette.default_color) {
                let _ = write!(props, " fo:color=\"{color}\"");
            }
            if let Some(color) = style.background {
                let _ = write!(props, " fo:background-color=\"{color}\"");
            }
            if style.italic {
                props.push_str(" fo:font-style=\"italic\"");
            }
            if style.bold {
                props.push_str(" fo:font-weight=\"bold\"");
            }
            if style.underline {
                props.push_str(" style:text-underline-style=\"solid\"");
            }
            styles.push(format!("<style:style style:name=\"{name}\" style:family=\"text\"><style:text-properties{props} /></style:style>"));
        }
    }
    styles
}

fn timestamp(context: &mut Context) -> i64 {
    context
        .host()
        .environment_variable("SOURCE_DATE_EPOCH")
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| context.host().current_time().unix_seconds)
}

fn iso_time(unix_seconds: i64) -> String {
    // Convert days since 1970 to the Gregorian calendar.
    let days = unix_seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day = days.rem_euclid(146_097);
    let year = (day - day / 1460 + day / 36_524 - day / 146_096) / 365;
    let day = day - (365 * year + year / 4 - year / 100);
    let month = (5 * day + 2) / 153;
    let day = day - (153 * month + 2) / 5 + 1;
    let month = if month < 10 { month + 3 } else { month - 9 };
    let year = year + era * 400 + i64::from(month <= 2);
    let seconds = unix_seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// Returns the child elements of `office:meta`.
fn metadata(meta: &Meta, timestamp: i64) -> Vec<String> {
    let mut out = vec![format!(
        "<meta:generator>Randoc/{}</meta:generator>",
        env!("CARGO_PKG_VERSION")
    )];
    for (field, tag) in [
        ("title", "dc:title"),
        ("description", "dc:description"),
        ("subject", "dc:subject"),
        ("keywords", "meta:keyword"),
        ("lang", "dc:language"),
        ("author", "meta:initial-creator"),
        ("author", "dc:creator"),
    ] {
        let value = meta
            .0
            .get(field)
            .map(|field_value| match field_value {
                MetaValue::MetaList(values) => values
                    .iter()
                    .filter_map(meta_text)
                    .collect::<Vec<_>>()
                    .join(if field == "author" { "; " } else { ", " }),
                _ => meta_text(field_value).unwrap_or_default(),
            })
            .unwrap_or_default();
        if field != "lang" || !value.is_empty() {
            out.push(format!("<{tag}>{}</{tag}>", escaped(&value)));
        }
    }
    let date = iso_time(timestamp);
    out.push(format!("<meta:creation-date>{date}</meta:creation-date>"));
    out.push(format!("<dc:date>{date}</dc:date>"));
    for (name, value) in &meta.0 {
        if matches!(
            name.as_str(),
            "title" | "description" | "subject" | "keywords" | "lang" | "author"
        ) {
            continue;
        }
        if let Some(value) = meta_text(value) {
            out.push(format!(
                "<meta:user-defined meta:name=\"{}\" meta:value-type=\"string\">{}</meta:user-defined>",
                escaped(name),
                escaped(&value)
            ));
        }
    }
    out
}

pub(super) fn meta_text(value: &MetaValue) -> Option<String> {
    match value {
        MetaValue::MetaString(value) => Some(value.clone()),
        MetaValue::MetaBool(value) => Some(value.to_string()),
        MetaValue::MetaInlines(value) => Some(stringify(value)),
        MetaValue::MetaBlocks(value) => Some(randoc_core::shared::stringify_blocks(value)),
        _ => None,
    }
}

impl Renderer<'_> {
    pub(super) fn formula(&mut self, kind: MathType, math: String) -> Result<Doc> {
        let is_inline = kind == MathType::InlineMath;
        let style = if is_inline { "fr1" } else { "fr2" };
        let anchor = if is_inline { "as-char" } else { "paragraph" };
        let object = if self.format == Format::Fodt {
            format!("<draw:object>{math}</draw:object>")
        } else {
            let mut index = self.resources.next_index;
            let path = loop {
                let path = format!("Formula-{index}");
                if !self
                    .resources
                    .parts
                    .contains_key(&format!("{path}/content.xml"))
                    && !self
                        .resources
                        .parts
                        .contains_key(&format!("{path}/settings.xml"))
                {
                    break path;
                }
                index += 1;
            };
            let content = format!("<?xml version='1.0' ?>\n{math}\n");
            let settings = format!(
                "{XML}<office:document-settings xmlns:office=\"{OFFICE}\" xmlns:xlink=\"{XLINK}\" xmlns:config=\"{}\" xmlns:ooo=\"{OOO}\" office:version=\"1.3\">\n  <office:settings><config:config-item-set config:name=\"ooo:configuration-settings\"><config:config-item config:name=\"IsTextMode\" config:type=\"boolean\">{is_inline}</config:config-item></config:config-item-set></office:settings>\n</office:document-settings>",
                ns::CONFIG
            );
            self.resources.charge(
                self.context,
                content.len().saturating_add(settings.len()),
                2,
            )?;
            self.resources.add(
                format!("{path}/content.xml"),
                "application/xml",
                content.into_bytes(),
            );
            self.resources.add(
                format!("{path}/settings.xml"),
                "application/xml",
                settings.into_bytes(),
            );
            self.resources.next_index = index + 2;
            format!(
                "<draw:object xlink:href=\"{path}/\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\" />"
            )
        };
        Ok(text(format!(
            "<draw:frame draw:style-name=\"{style}\" text:anchor-type=\"{anchor}\">{object}</draw:frame>"
        )))
    }

    pub(super) fn href<'a>(&self, url: &'a str) -> Cow<'a, str> {
        if self.format != Format::Odt {
            return Cow::Borrowed(url);
        }
        let path = url.split(['?', '#']).next().unwrap_or_default();
        if path.is_empty()
            || path
                .split('/')
                .next()
                .is_some_and(|part| part.contains(':'))
            || !valid_uri_reference(url)
        {
            return Cow::Borrowed(url);
        }
        let prefix = path.strip_prefix("//").map_or(0, |rest| {
            rest.find('/').map_or(path.len(), |index| index + 2)
        });
        if prefix == path.len() {
            return Cow::Borrowed(url);
        }
        let (authority, rest) = url.split_at(prefix);
        Cow::Owned(format!("{authority}../{rest}"))
    }

    pub(super) fn image(
        &mut self,
        attr: &Attr,
        alt: &[Inline],
        target: &Target,
        depth: usize,
    ) -> Result<Doc> {
        self.image_count += 1;
        let mut width = attr.get("width").map(str::to_owned);
        let mut height = attr.get("height").map(str::to_owned);
        let mut reference = self.href(&target.url).into_owned();
        let mut embedded = None;
        if self.format != Format::Fragment {
            match fetch(self.context, &target.url) {
                Some((bytes, mime)) => {
                    let (size, detected) = image_size_and_mime(&bytes, Some(&target.url));
                    let mime = detected
                        .map(str::to_owned)
                        .or(mime)
                        .unwrap_or_else(|| "application/octet-stream".into());
                    if let Some((w, h)) = size.filter(|&(w, h)| w > 0 && h > 0) {
                        let ratio = f64::from(w) / f64::from(h);
                        let (x_dpi, y_dpi) = image_density(&bytes).unwrap_or((72.0, 72.0));
                        match (&width, &height) {
                            (None, None) => {
                                width = Some(format!(
                                    "{:.1}pt",
                                    (f64::from(w) * 72.0 / x_dpi).max(1.0)
                                ));
                                height = Some(format!(
                                    "{:.1}pt",
                                    (f64::from(h) * 72.0 / y_dpi).max(1.0)
                                ));
                            }
                            (Some(w), None) => height = scaled(w, 1.0 / ratio),
                            (None, Some(h)) => width = scaled(h, ratio),
                            _ => {}
                        }
                    }
                    if !self.options.link_images && self.format == Format::Fodt {
                        let encoded_size = bytes.len().saturating_add(2) / 3 * 4;
                        self.resources.charge(self.context, encoded_size, 0)?;
                        embedded = Some(format!(
                            "<draw:image draw:mime-type=\"{}\"><office:binary-data>{}</office:binary-data></draw:image>",
                            escaped(&mime),
                            base64_encode(&bytes)
                        ));
                    } else if !self.options.link_images {
                        let extension = extension_for_mime(&mime).unwrap_or("bin");
                        loop {
                            let index = self.resources.next_index;
                            self.resources.next_index += 1;
                            reference = format!("Pictures/{index}.{extension}");
                            if !self.resources.parts.contains_key(&reference) {
                                break;
                            }
                        }
                        self.resources.charge(self.context, bytes.len(), 1)?;
                        self.resources.add(reference.clone(), mime, bytes);
                    }
                }
                None => {
                    self.context.report(LogMessage::CouldNotFetchResource(
                        target.url.clone(),
                        format!("PandocResourceNotFound {:?}", target.url),
                    ));
                    return Ok(if alt.is_empty() {
                        Doc::default()
                    } else {
                        span("Emphasis", self.inlines(alt, depth + 1)?)
                    });
                }
            }
        }
        let image = embedded.unwrap_or_else(|| format!("<draw:image xlink:href=\"{}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\" />", escaped(&reference)));
        let mut attrs = format!(
            "draw:name=\"img{}\" text:anchor-type=\"as-char\"",
            self.image_count
        );
        if let Some(width) = width {
            attrs.push_str(&attribute("svg:width", &width));
        }
        if let Some(height) = height {
            attrs.push_str(&attribute("svg:height", &height));
        }
        Ok(text(format!("<draw:frame {attrs}>{image}</draw:frame>")))
    }
}

fn valid_uri_reference(url: &str) -> bool {
    let (url, fragment) = url.split_once('#').unwrap_or((url, ""));
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let path = if let Some(rest) = path.strip_prefix("//") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        if !valid_uri_authority(authority) {
            return false;
        }
        path
    } else {
        path
    };
    valid_uri_component(path, b":@/")
        && valid_uri_component(query, b":@/?")
        && valid_uri_component(fragment, b":@/?")
}

fn valid_uri_authority(authority: &str) -> bool {
    let (user, host) = authority.split_once('@').unwrap_or(("", authority));
    if !valid_uri_component(user, b":") {
        return false;
    }
    if let Some(literal) = host.strip_prefix('[') {
        let Some((ip, port)) = literal.split_once(']') else {
            return false;
        };
        let future = ip
            .strip_prefix(['v', 'V'])
            .and_then(|rest| rest.split_once('.'));
        let valid_ip = ip.parse::<std::net::Ipv6Addr>().is_ok()
            || future.is_some_and(|(version, address)| {
                !version.is_empty()
                    && version.bytes().all(|b| b.is_ascii_hexdigit())
                    && !address.is_empty()
                    && !address.contains('%')
                    && valid_uri_component(address, b":")
            });
        return valid_ip && (port.is_empty() || port.strip_prefix(':').is_some_and(ascii_digits));
    }
    let (host, port) = host.split_once(':').unwrap_or((host, ""));
    valid_uri_component(host, b"") && ascii_digits(port)
}

fn ascii_digits(text: &str) -> bool {
    text.bytes().all(|b| b.is_ascii_digit())
}

fn valid_uri_component(value: &str, extra: &[u8]) -> bool {
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            if !bytes.next().is_some_and(|b| b.is_ascii_hexdigit())
                || !bytes.next().is_some_and(|b| b.is_ascii_hexdigit())
            {
                return false;
            }
        } else if !byte.is_ascii_alphanumeric()
            && !b"-._~!$&'()*+,;=".contains(&byte)
            && !extra.contains(&byte)
        {
            return false;
        }
    }
    true
}

fn image_density(bytes: &[u8]) -> Option<(f64, f64)> {
    let (x, y, scale) = match bytes {
        [
            0xff,
            0xd8,
            0xff,
            0xe0,
            _,
            _,
            b'J',
            b'F',
            b'I',
            b'F',
            0,
            _,
            _,
            unit,
            x1,
            x2,
            y1,
            y2,
            ..,
        ] => {
            let scale = match unit {
                1 => 1.0,
                2 => 2.54,
                _ => return None,
            };
            (
                u32::from(u16::from_be_bytes([*x1, *x2])),
                u32::from(u16::from_be_bytes([*y1, *y2])),
                scale,
            )
        }
        [137, b'P', b'N', b'G', 13, 10, 26, 10, chunks @ ..] => {
            let mut chunks = chunks;
            loop {
                let [l1, l2, l3, l4, kind @ ..] = chunks else {
                    return None;
                };
                let length = u32::from_be_bytes([*l1, *l2, *l3, *l4]) as usize;
                if let [
                    b'p',
                    b'H',
                    b'Y',
                    b's',
                    x1,
                    x2,
                    x3,
                    x4,
                    y1,
                    y2,
                    y3,
                    y4,
                    1,
                    ..,
                ] = kind
                    && length == 9
                {
                    break (
                        u32::from_be_bytes([*x1, *x2, *x3, *x4]),
                        u32::from_be_bytes([*y1, *y2, *y3, *y4]),
                        0.0254,
                    );
                }
                if kind.starts_with(b"IDAT") {
                    return None;
                }
                chunks = chunks.get(length.checked_add(12)?..)?;
            }
        }
        _ => return None,
    };
    (x > 0 && y > 0).then_some((f64::from(x) * scale, f64::from(y) * scale))
}

fn scaled(dimension: &str, ratio: f64) -> Option<String> {
    let unit_start = dimension
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(dimension.len());
    let (number, unit) = dimension.split_at(unit_start);
    if unit == "%" {
        return None;
    }
    let value = number.parse::<f64>().ok()? * ratio;
    value.is_finite().then(|| format!("{value:.1}{unit}"))
}

fn fetch(context: &mut Context, url: &str) -> Option<(Vec<u8>, Option<String>)> {
    let max = context.limits.max_resource_size;
    if let Some(item) = context.media.get(url) {
        return (item.contents.len() as u64 <= max)
            .then(|| (item.contents.clone(), Some(item.mime_type.clone())));
    }
    if url.starts_with("data:") {
        let (mime, bytes) = decode_data_uri(url, max)?;
        return Some((bytes, Some(mime)));
    }
    if url.starts_with("https://") || url.starts_with("http://") {
        let headers = context.state.request_headers.clone();
        let (body, mime) = context.host().fetch_url(url, &headers).ok()?;
        return (body.len() as u64 <= max).then_some((body, mime));
    }
    let limit = usize::try_from(max).unwrap_or(usize::MAX);
    let path = Path::new(url);
    for dir in context.state.resource_path.clone() {
        if let Some(path) = Path::new(&dir).join(path).to_str()
            && let Ok(bytes) = context.read_file_limited(path, limit)
        {
            return Some((bytes, None));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use randoc_archive::media::base64_decode;
    use randoc_ast::Block;
    use randoc_core::{MediaItem, PureHost, ReaderOptions, Sources};

    #[test]
    fn packaged_formula_and_image_round_trip() -> Result<()> {
        let mut host = PureHost::default();
        host.environment
            .insert("SOURCE_DATE_EPOCH".into(), "1700000000".into());
        let mut context = Context::new(Box::new(host));
        let gif = base64_decode("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7")
            .ok_or_else(|| Error::App("The test image is invalid.".into()))?;
        context.media.insert(
            "image.gif",
            MediaItem {
                contents: gif.clone(),
                mime_type: "image/gif".into(),
            },
        );
        let doc = Pandoc {
            blocks: vec![
                Block::Para(vec![Inline::Math(MathType::InlineMath, "x^2".into())]),
                Block::Para(vec![Inline::Image(
                    Attr::default(),
                    Vec::new(),
                    Target::new("image.gif", ""),
                )]),
            ],
            ..Pandoc::default()
        };
        let options = WriterOptions::default();
        let odt = write_odt(&doc, &options, &mut context)?;
        assert_eq!(odt.get(8..10), Some(&[0u8, 0][..]));
        assert_eq!(odt.get(28..38), Some(&b"\0\0mimetype"[..]));
        assert_eq!(write_odt(&doc, &options, &mut context)?, odt);
        let mut zip =
            Archive::open(&odt, ArchiveLimits::from_limits(&context.limits)).map_err(zip_error)?;
        assert_eq!(
            zip.read("mimetype").map_err(zip_error)?.as_deref(),
            Some(MIME.as_bytes())
        );
        assert_eq!(
            zip.read("Pictures/2.gif").map_err(zip_error)?,
            Some(gif.clone())
        );
        assert!(zip.contains("Formula-0/content.xml") && zip.contains("Formula-0/settings.xml"));
        let metadata = zip.read("meta.xml").map_err(zip_error)?.unwrap_or_default();
        assert!(String::from_utf8_lossy(&metadata).contains("2023-11-14T22:13:20Z"));

        let fodt = write_fodt(&doc, &options, &mut context)?;
        assert!(fodt.contains("office:binary-data"));
        let reader_options = ReaderOptions::default();
        let odt = crate::reader::read_odt(&odt, &reader_options, &mut context)?;
        let fodt =
            crate::reader::read_fodt(&Sources::from_text(fodt), &reader_options, &mut context)?;
        for (doc, path) in [(odt, "Pictures/2.gif"), (fodt, "Pictures/image1.gif")] {
            assert_eq!(
                doc.blocks,
                vec![
                    Block::Para(vec![Inline::Math(MathType::DisplayMath, "x^{2}".into())]),
                    Block::Para(vec![Inline::Image(
                        Attr::new(
                            "",
                            Vec::new(),
                            vec![
                                ("width".into(), "1.0pt".into()),
                                ("height".into(), "1.0pt".into())
                            ]
                        ),
                        Vec::new(),
                        Target::new(path, ""),
                    )]),
                ]
            );
            assert_eq!(
                context.media.get(path).map(|item| &item.contents),
                Some(&gif)
            );
        }
        Ok(())
    }
    fn xml_part(archive: &mut Archive<'_>, name: &str) -> Result<String> {
        let bytes = archive
            .read(name)
            .map_err(zip_error)?
            .ok_or_else(|| Error::App(format!("Missing {name}")))?;
        String::from_utf8(bytes).map_err(|error| Error::App(error.to_string()))
    }

    #[test]
    fn odt_prefixes_relative_links_at_every_output_path() -> Result<()> {
        let links = [
            ("file.txt", "../file.txt"),
            ("a[b].html", "a[b].html"),
            ("a%5Bb%5D.html", "../a%5Bb%5D.html"),
            ("a.html?x=[b]", "a.html?x=[b]"),
            ("//[::1]/a.html", "//[::1]..//a.html"),
            ("./file.txt", ".././file.txt"),
            ("../file.txt", "../../file.txt"),
            ("/file.txt", "..//file.txt"),
            ("https://example.org/file", "https://example.org/file"),
            ("#section", "#section"),
            ("?query", "?query"),
        ];
        let doc = Pandoc {
            blocks: vec![Block::Para(
                links
                    .iter()
                    .map(|(href, _)| {
                        Inline::Link(
                            Attr::default(),
                            vec![Inline::Str("link".into())],
                            Target::new(*href, ""),
                        )
                    })
                    .collect(),
            )],
            ..Pandoc::default()
        };
        for output in [None, Some("out.odt"), Some("a/b/out.odt")] {
            let mut context = Context::pure();
            context.state.output_file = output.map(str::to_owned);
            let odt = write_odt(&doc, &WriterOptions::default(), &mut context)?;
            let mut zip = Archive::open(&odt, ArchiveLimits::default()).map_err(zip_error)?;
            let content = xml_part(&mut zip, "content.xml")?;
            for (_, expected) in links {
                assert!(
                    content.contains(&format!("xlink:href=\"{expected}\"")),
                    "{content}"
                );
            }
        }
        let fodt = write_fodt(&doc, &WriterOptions::default(), &mut Context::pure())?;
        for (expected, _) in links {
            assert!(
                fodt.contains(&format!("xlink:href=\"{expected}\"")),
                "{fodt}"
            );
        }
        Ok(())
    }

    #[test]
    fn data_uri_images_use_intrinsic_size_without_description() -> Result<()> {
        // A 20-by-10 PNG header with no density chunk.
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\0\x14\0\0\0\x0a";
        let data = format!(
            "data:image/png;base64,{}",
            base64_encode(png).replace('/', "%2F")
        );
        let doc = Pandoc {
            blocks: vec![Block::Para(vec![Inline::Image(
                Attr::default(),
                vec![Inline::Str("Alt text".into())],
                Target::new(data, ""),
            )])],
            ..Pandoc::default()
        };
        let mut options = WriterOptions::default();
        options.dpi = 300;
        let mut context = Context::pure();
        let odt = write_odt(&doc, &options, &mut context)?;
        let mut zip = Archive::open(&odt, ArchiveLimits::default()).map_err(zip_error)?;
        assert_eq!(
            zip.read("Pictures/0.png").map_err(zip_error)?.as_deref(),
            Some(png.as_slice())
        );
        let content = xml_part(&mut zip, "content.xml")?;
        let fodt = write_fodt(&doc, &options, &mut context)?;
        assert!(content.contains("xlink:href=\"Pictures/0.png\""));
        assert!(fodt.contains("<office:binary-data>"));
        for xml in [content, fodt] {
            assert!(xml.contains("svg:width=\"20.0pt\" svg:height=\"10.0pt\""));
            assert!(!xml.contains("<svg:desc"));
        }
        Ok(())
    }

    #[test]
    fn pdf_data_uri_has_a_pdf_entry() -> Result<()> {
        let doc = Pandoc {
            blocks: vec![Block::Para(vec![Inline::Image(
                Attr::default(),
                vec![Inline::Str("PDF".into())],
                Target::new("data:application/pdf;base64,JVBERg==", ""),
            )])],
            ..Pandoc::default()
        };
        let odt = write_odt(&doc, &WriterOptions::default(), &mut Context::pure())?;
        let mut zip = Archive::open(&odt, ArchiveLimits::default()).map_err(zip_error)?;
        assert_eq!(
            zip.read("Pictures/0.pdf").map_err(zip_error)?.as_deref(),
            Some(&b"%PDF"[..])
        );
        assert!(xml_part(&mut zip, "content.xml")?.contains(r#"xlink:href="Pictures/0.pdf""#));
        Ok(())
    }

    #[test]
    fn language_without_a_region_adds_an_empty_country_to_default_styles() -> Result<()> {
        let mut doc = Pandoc::default();
        doc.meta
            .0
            .insert("lang".into(), MetaValue::MetaString("de".into()));
        let odt = write_odt(&doc, &WriterOptions::default(), &mut Context::pure())?;
        let mut zip = Archive::open(&odt, ArchiveLimits::default()).map_err(zip_error)?;
        let styles = xml_part(&mut zip, "styles.xml")?;
        assert!(styles.contains(concat!(
            r#"<style:default-style style:family="paragraph">"#,
            r#"<style:text-properties fo:language="de" fo:country="" />"#
        )));
        Ok(())
    }

    #[test]
    fn reference_document_file_uri_reads_the_absolute_path() -> Result<()> {
        let reference_styles = format!(
            r#"<office:document-styles xmlns:office="{OFFICE}" xmlns:style="{STYLE}"><office:styles><style:style style:name="ReferenceStyle" style:family="paragraph"/></office:styles></office:document-styles>"#
        );
        let mut zip = ArchiveWriter::new();
        zip.write_stored("styles.xml", reference_styles.as_bytes())
            .map_err(zip_error)?;
        let mut host = PureHost::default();
        let reference = zip.finish().map_err(zip_error)?;
        host.files.insert("/refs/ref.odt".into(), reference.clone());
        host.files.insert("/refs/ref doc.odt".into(), reference);
        let mut context = Context::new(Box::new(host));
        let mut options = WriterOptions::default();
        for uri in [
            "file:///refs/ref.odt",
            "file://localhost/refs/ref%20doc.odt",
        ] {
            options.reference_doc = Some(uri.into());
            let odt = write_odt(&Pandoc::default(), &options, &mut context)?;
            let mut zip = Archive::open(&odt, ArchiveLimits::default()).map_err(zip_error)?;
            assert!(xml_part(&mut zip, "styles.xml")?.contains(r#"style:name="ReferenceStyle""#));
        }
        Ok(())
    }

    #[test]
    fn manifest_uses_application_xml_and_styles_use_document_language() -> Result<()> {
        let mut doc = Pandoc::default();
        doc.meta
            .0
            .insert("lang".into(), MetaValue::MetaString("de-DE".into()));
        let mut context = Context::pure();
        let odt = write_odt(&doc, &WriterOptions::default(), &mut context)?;
        let mut zip = Archive::open(&odt, ArchiveLimits::default()).map_err(zip_error)?;
        let manifest = xml_part(&mut zip, MANIFEST_FILE)?;
        for part in ["styles.xml", "content.xml", "meta.xml"] {
            assert!(manifest.contains(&format!(
                r#"manifest:media-type="application/xml" manifest:full-path="{part}""#
            )));
        }
        let styles = xml_part(&mut zip, "styles.xml")?;
        let fodt = write_fodt(&doc, &WriterOptions::default(), &mut context)?;
        for xml in [styles, fodt] {
            assert!(xml.contains(r#"fo:language="de" fo:country="DE""#));
        }
        Ok(())
    }

    #[test]
    fn reference_documents_use_resource_lookup_and_size_limit() -> Result<()> {
        let doc = Pandoc::default();
        let mut options = WriterOptions::default();
        options.reference_doc = Some("reference.odt".into());
        let err = write_odt(&doc, &options, &mut Context::pure());
        assert!(matches!(err, Err(Error::ResourceNotFound(name)) if name == "reference.odt"));

        let styles = format!(
            r#"<office:document-styles xmlns:office="{OFFICE}" xmlns:style="{STYLE}" xmlns:fo="{}"><office:styles><style:default-style style:family="paragraph"><style:text-properties fo:language="fr" fo:country="CA" /></style:default-style></office:styles></office:document-styles>"#,
            ns::FO
        );
        let mut zip = ArchiveWriter::new();
        zip.write_stored("styles.xml", styles.as_bytes())
            .map_err(zip_error)?;
        let reference = zip.finish().map_err(zip_error)?;
        let size = reference.len() as u64;
        let mut host = PureHost::default();
        host.files
            .insert("resources/reference.odt".into(), reference);
        let mut context = Context::new(Box::new(host));
        context.state.resource_path = vec!["resources".into()];
        context.limits.max_input_size = 1;
        let mut doc = doc;
        doc.meta
            .0
            .insert("lang".into(), MetaValue::MetaString("de-DE".into()));
        let odt = write_odt(&doc, &options, &mut context)?;
        let mut zip = Archive::open(&odt, ArchiveLimits::default()).map_err(zip_error)?;
        assert!(xml_part(&mut zip, "styles.xml")?.contains(r#"fo:language="de" fo:country="DE""#));
        context.limits.max_input_size = size;
        context.limits.max_resource_size = size - 1;
        assert!(matches!(
            write_odt(&doc, &options, &mut context),
            Err(Error::Io(_))
        ));
        Ok(())
    }

    #[test]
    fn flat_document_sorts_root_attributes_and_merges_font_faces() -> Result<()> {
        let doc = Pandoc {
            blocks: vec![Block::Para(vec![Inline::Code(Attr::default(), "x".into())])],
            ..Pandoc::default()
        };
        let fodt = write_fodt(&doc, &WriterOptions::default(), &mut Context::pure())?;
        assert!(fodt.starts_with(concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document ",
            "office:mimetype=\"application/vnd.oasis.opendocument.text\" ",
            "office:version=\"1.3\" xmlns:chart="
        )));
        assert!(fodt.contains("xmlns:grddl="), "{fodt}");
        assert_eq!(fodt.matches("<style:font-face ").count(), 1, "{fodt}");
        assert!(
            fodt.contains("</office:meta><office:font-face-decls>"),
            "{fodt}"
        );
        Ok(())
    }

    #[test]
    fn languages_and_formats_style_each_run_of_words() -> Result<()> {
        let attr = |lang: &str| {
            Attr::new(
                String::new(),
                Vec::new(),
                vec![("lang".into(), lang.into())],
            )
        };
        let word = |text: &str| Inline::Str(text.into());
        let doc = Pandoc {
            blocks: vec![Block::Div(
                attr("fr-CA"),
                vec![Block::Para(vec![
                    Inline::Span(Attr::new("s", Vec::new(), Vec::new()), vec![word("a")]),
                    Inline::Emph(vec![word("b"), Inline::Code(Attr::default(), "c".into())]),
                    Inline::Note(vec![Block::Para(vec![word("n")])]),
                ])],
            )],
            ..Pandoc::default()
        };
        let fodt = write_fodt(&doc, &WriterOptions::default(), &mut Context::pure())?;
        assert!(fodt.contains(concat!(
            "<text:bookmark-start text:name=\"s\" /><text:span text:style-name=\"T1\">a</text:span>",
            "<text:bookmark-end text:name=\"s\" /><text:span text:style-name=\"T2\">b</text:span>",
            "<text:span text:style-name=\"Source_20_Text\">c</text:span>"
        )), "{fodt}");
        assert!(
            fodt.contains("<text:p text:style-name=\"Footnote\">n</text:p>"),
            "{fodt}"
        );
        assert!(fodt.contains(concat!(
            "<style:style style:name=\"T2\" style:family=\"text\"><style:text-properties ",
            "fo:country=\"CA\" fo:font-style=\"italic\" fo:language=\"fr\" ",
            "style:font-style-asian=\"italic\" style:font-style-complex=\"italic\" /></style:style>"
        )), "{fodt}");
        Ok(())
    }

    #[test]
    fn a_final_line_break_in_code_adds_no_paragraph() -> Result<()> {
        let doc = Pandoc {
            blocks: vec![Block::CodeBlock(Attr::default(), "a\n\nb\n".into())],
            ..Pandoc::default()
        };
        let mut options = WriterOptions::default();
        options.highlight_style = None;
        let fodt = write_fodt(&doc, &options, &mut Context::pure())?;
        assert_eq!(
            fodt.matches("<text:p text:style-name=\"Preformatted_20_Text\"")
                .count(),
            3,
            "{fodt}"
        );
        Ok(())
    }
}
