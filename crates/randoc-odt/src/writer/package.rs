use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write;
use std::path::{Component, Path, PathBuf};

use randoc_archive::media::{
    base64_encode, image_size_and_mime, is_safe_media_path, mime_from_extension,
};
use randoc_archive::xml::{self, XmlDoc, XmlLimits, XmlNode, XmlOptions};
use randoc_archive::zip::{Archive, ArchiveError, ArchiveLimits, ArchiveWriter};
use randoc_ast::{Attr, Inline, MathType, Meta, MetaValue, Pandoc, Target};
use randoc_core::shared::stringify;
use randoc_core::{Context, Error, LogMessage, Result, WriterOptions};
use randoc_highlight::{Style, TokenType};
use randoc_layout::{Doc, text};

use crate::ns::{self, OFFICE, STYLE, is_el};

use super::{Format, Renderer, attribute, escaped, render, span};

const MIME: &str = "application/vnd.oasis.opendocument.text";
const FORMULA_MIME: &str = "application/vnd.oasis.opendocument.formula";
const XML: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n";
const MANIFEST_FILE: &str = "META-INF/manifest.xml";

#[derive(Default)]
pub(super) struct RelativeLinks {
    cwd: PathBuf,
    output_dir: Option<PathBuf>,
}

impl RelativeLinks {
    pub(super) fn new(format: Format, context: &mut Context) -> Self {
        if format != Format::Odt {
            return Self::default();
        }
        let Some(output) = context
            .state
            .output_file
            .as_deref()
            .and_then(|path| Path::new(path).parent())
            .map(Path::to_owned)
        else {
            return Self::default();
        };
        if output.as_os_str().is_empty() {
            return Self::default();
        }
        let Ok(cwd) = context.host().working_directory() else {
            return Self::default();
        };
        let cwd = PathBuf::from(cwd);
        let output_dir = normalize_path(&cwd.join(output));
        Self {
            cwd,
            output_dir: Some(output_dir),
        }
    }

    pub(super) fn resolve<'a>(&self, url: &'a str) -> Cow<'a, str> {
        let Some(output) = &self.output_dir else {
            return Cow::Borrowed(url);
        };
        if url.is_empty()
            || url.starts_with(['#', '?', '/'])
            || url
                .split('/')
                .next()
                .is_some_and(|prefix| prefix.contains(':'))
        {
            return Cow::Borrowed(url);
        }
        let suffix_start = url.find(['#', '?']).unwrap_or(url.len());
        let (path, suffix) = url.split_at(suffix_start);
        let path = normalize_path(&self.cwd.join(path));
        let common = output
            .components()
            .zip(path.components())
            .take_while(|(left, right)| left == right)
            .count();
        if common == 0 {
            return Cow::Borrowed(url);
        }
        let mut relative = PathBuf::new();
        for _ in output.components().skip(common) {
            relative.push("..");
        }
        for component in path.components().skip(common) {
            relative.push(component);
        }
        let mut relative = relative.to_string_lossy().into_owned();
        if relative.is_empty() {
            relative.push('.');
        }
        if url.ends_with('/') {
            relative.push('/');
        }
        relative.push_str(suffix);
        Cow::Owned(relative)
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

#[derive(Default)]
pub(super) struct Resources {
    parts: BTreeMap<String, Part>,
    pub(super) custom_styles: BTreeSet<(String, &'static str)>,
    pub(super) highlighted: bool,
    formulas: BTreeSet<String>,
    next_index: usize,
    added_bytes: u64,
}

struct Part {
    media_type: String,
    bytes: Vec<u8>,
}

impl Resources {
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
    let meta = format!(
        "{XML}<office:document-meta xmlns:office=\"{OFFICE}\" xmlns:dc=\"{}\" xmlns:meta=\"{}\" office:version=\"1.3\">{}</office:document-meta>",
        ns::DC,
        ns::META,
        metadata(&doc.meta, stamp)
    );
    let styles = styles_xml(reference, options, &resources, context)?;
    if !resources.parts.contains_key("manifest.rdf") {
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
    resources.charge(
        context,
        content
            .len()
            .saturating_add(styles.len())
            .saturating_add(meta.len()),
        5,
    )?;
    resources.insert("content.xml".into(), "text/xml", content.into_bytes());
    resources.insert("styles.xml".into(), "text/xml", styles.into_bytes());
    resources.insert("meta.xml".into(), "text/xml", meta.into_bytes());
    let manifest = manifest(&resources);
    resources.charge(context, manifest.len().saturating_add(MIME.len()), 2)?;

    let mut archive = ArchiveWriter::new();
    archive
        .write_stored_with_time("mimetype", MIME.as_bytes(), Some(stamp))
        .map_err(zip_error)?;
    archive
        .write_deflated_with_time(MANIFEST_FILE, manifest.as_bytes(), Some(stamp))
        .map_err(zip_error)?;
    for (name, part) in resources.parts {
        archive
            .write_deflated_with_time(&name, &part.bytes, Some(stamp))
            .map_err(zip_error)?;
    }
    archive.finish().map_err(zip_error)
}

pub(super) fn write_fodt(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let (reference, resources) = reference(options, context, false)?;
    let (content, resources) = render(doc, options, context, Format::Fodt, resources)?;
    let styles = styles_xml(reference, options, &resources, context)?;
    let mut content = parse_xml(&content, "content.xml", context)?.root;
    if !is_el(&content, OFFICE, "document-content") && !is_el(&content, OFFICE, "document") {
        return Err(Error::App(
            "The FODT template must produce an OpenDocument root element.".into(),
        ));
    }
    content.name.local = "document".into();
    if content.attr(OFFICE, "mimetype").is_none() {
        let prefix = content.name.prefix.clone().unwrap_or_else(|| {
            let mut prefix = "odf".to_owned();
            while content
                .ns_decls
                .iter()
                .any(|(key, _)| key.as_deref() == Some(prefix.as_str()))
            {
                prefix.push('_');
            }
            content
                .ns_decls
                .push((Some(prefix.clone().into()), OFFICE.into()));
            prefix.into()
        });
        content.attrs.push((
            xml::QName {
                ns: Some(OFFICE.into()),
                prefix: Some(prefix),
                local: "mimetype".into(),
            },
            MIME.into(),
        ));
    }
    let meta = format!(
        "<office:meta xmlns:office=\"{OFFICE}\" xmlns:dc=\"{}\" xmlns:meta=\"{}\">",
        ns::DC,
        ns::META
    );
    let meta = metadata(&doc.meta, timestamp(context)).replacen("<office:meta>", &meta, 1);
    let meta = parse_xml(&meta, "meta.xml", context)?.root;
    let styles = parse_xml(&styles, "styles.xml", context)?.root;
    let mut style_nodes = styles
        .children
        .into_iter()
        .filter_map(|node| {
            let XmlNode::Element(mut child) = node else {
                return None;
            };
            if child.name.ns.as_deref() != Some(OFFICE) {
                return None;
            }
            for (prefix, ns) in &styles.ns_decls {
                if !child.ns_decls.iter().any(|(key, _)| key == prefix) {
                    child.ns_decls.push((prefix.clone(), ns.clone()));
                }
            }
            Some(child)
        })
        .collect::<Vec<_>>();

    content
        .children
        .retain(|node| !matches!(node, XmlNode::Element(el) if is_el(el, OFFICE, "meta")));
    let mut output_nodes = vec![XmlNode::Element(meta)];
    for name in [
        "font-face-decls",
        "styles",
        "automatic-styles",
        "master-styles",
    ] {
        let existing = content.children.iter().position(
            |node| matches!(node, XmlNode::Element(element) if is_el(element, OFFICE, name)),
        );
        let inherited = style_nodes
            .iter()
            .position(|node| is_el(node, OFFICE, name))
            .map(|index| style_nodes.remove(index));
        let current = existing.and_then(|index| match content.children.remove(index) {
            XmlNode::Element(el) => Some(el),
            XmlNode::Text(_) => None,
        });
        match (inherited, current) {
            (Some(mut base), Some(mut generated)) => {
                // Keep the namespace bindings of both files on their own nodes.
                for child in &mut base.children {
                    if let XmlNode::Element(child) = child {
                        for (prefix, ns) in &base.ns_decls {
                            if !child.ns_decls.iter().any(|(key, _)| key == prefix) {
                                child.ns_decls.push((prefix.clone(), ns.clone()));
                            }
                        }
                    }
                }
                if name == "font-face-decls" {
                    let fonts: HashSet<_> = base
                        .elements()
                        .filter_map(|child| child.attr(STYLE, "name"))
                        .collect();
                    generated.children.retain(|node| {
                        !matches!(node, XmlNode::Element(child) if child.attr(STYLE, "name").is_some_and(|font| fonts.contains(font)))
                    });
                }
                base.children.append(&mut generated.children);
                generated.children = base.children;
                output_nodes.push(XmlNode::Element(generated));
            }
            (Some(node), None) | (None, Some(node)) => output_nodes.push(XmlNode::Element(node)),
            (None, None) => {}
        }
    }
    output_nodes.append(&mut content.children);
    content.children = output_nodes;
    let mut out = XML.to_owned();
    content.write_to(&mut out);
    Ok(out)
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
        message: error.format_conduit(),
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
    let path = explicit.or_else(|| candidate.as_deref().filter(|path| is_file(context, path)));
    let mut resources = Resources::default();
    let limit = usize::try_from(context.limits.max_input_size).unwrap_or(usize::MAX);
    if let Some(path) = path {
        let input = context.read_file_limited(path, limit)?;
        let mut archive = Archive::open(&input, ArchiveLimits::from_limits(&context.limits))
            .map_err(|error| {
                Error::Parse(format!("Could not parse reference-doc {path:?}: {error}"))
            })?;
        let styles = archive.read("styles.xml").map_err(zip_error)?;
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
    if let Some(dir) = data_dir {
        let limit = usize::try_from(context.limits.max_resource_size).unwrap_or(usize::MAX);
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
                }
            }
        }
    }
    Ok((styles, resources))
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
        "text/xml"
    } else if path.ends_with(".rdf") {
        "application/rdf+xml"
    } else {
        mime_from_extension(path).unwrap_or("application/octet-stream")
    }
}

fn manifest(resources: &Resources) -> String {
    let mut out = format!(
        "{XML}<manifest:manifest xmlns:manifest=\"{}\" manifest:version=\"1.3\">\n  <manifest:file-entry manifest:media-type=\"{MIME}\" manifest:full-path=\"/\" manifest:version=\"1.3\" />\n",
        ns::MANIFEST
    );
    for (path, kind) in resources
        .parts
        .iter()
        .map(|(path, part)| (path.as_str(), part.media_type.as_str()))
    {
        let _ = writeln!(
            out,
            "  <manifest:file-entry manifest:media-type=\"{}\" manifest:full-path=\"{}\" />",
            escaped(kind),
            escaped(path)
        );
    }
    for path in &resources.formulas {
        let _ = writeln!(
            out,
            "  <manifest:file-entry manifest:media-type=\"{FORMULA_MIME}\" manifest:full-path=\"{path}/\" />"
        );
    }
    out.push_str("</manifest:manifest>\n");
    out
}

fn styles_xml(
    reference: Option<Vec<u8>>,
    options: &WriterOptions,
    resources: &Resources,
    context: &Context,
) -> Result<String> {
    let reference = reference
        .map(String::from_utf8)
        .transpose()
        .map_err(|error| Error::Xml {
            file: "'styles.xml'".into(),
            message: error.to_string(),
        })?;
    let additions = extra_styles(options, resources);
    if let Some(reference) = reference {
        let mut root = parse_xml(&reference, "styles.xml", context)?.root;
        if additions.is_empty() {
            return Ok(reference);
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
            let names = styles
                .elements()
                .filter_map(|element| element.attr(STYLE, "name").map(str::to_owned))
                .collect::<HashSet<_>>();
            for (name, definition) in additions {
                if !names.contains(&name) {
                    let definition = definition.replacen(
                        "<style:style",
                        &format!(
                            "<style:style xmlns:style=\"{STYLE}\" xmlns:fo=\"{}\"",
                            ns::FO
                        ),
                        1,
                    );
                    styles.children.push(XmlNode::Element(
                        parse_xml(&definition, "styles.xml", context)?.root,
                    ));
                }
            }
        }
        let mut out = XML.to_owned();
        root.write_to(&mut out);
        return Ok(out);
    }

    let mut out = default_styles();
    for (_, style) in additions {
        out.push_str(&style);
    }
    out.push_str("</office:styles><office:automatic-styles><style:page-layout style:name=\"PageLayout\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" fo:margin=\"2cm\" style:print-orientation=\"portrait\" /></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"PageLayout\" /></office:master-styles></office:document-styles>");
    Ok(out)
}

fn default_styles() -> String {
    let mut out = format!(
        "{XML}<office:document-styles xmlns:office=\"{OFFICE}\" xmlns:style=\"{STYLE}\" xmlns:text=\"{}\" xmlns:fo=\"{}\" xmlns:svg=\"{}\" office:version=\"1.3\"><office:font-face-decls><style:font-face style:name=\"Courier New\" svg:font-family=\"'Courier New'\" style:font-pitch=\"fixed\" /></office:font-face-decls><office:styles>",
        ns::TEXT,
        ns::FO,
        ns::SVG
    );
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
        let _ = write!(
            out,
            "<style:style style:name=\"{name}\" style:family=\"text\"><style:text-properties {props}/></style:style>"
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

fn extra_styles(options: &WriterOptions, resources: &Resources) -> BTreeMap<String, String> {
    let mut styles = BTreeMap::new();
    for (name, family) in &resources.custom_styles {
        styles.insert(
            name.clone(),
            format!(
                "<style:style style:name=\"{}\" style:family=\"{family}\" />",
                escaped(name)
            ),
        );
    }
    if resources.highlighted {
        let palette = options
            .highlight_theme
            .as_ref()
            .and_then(|file| std::str::from_utf8(&file.bytes).ok())
            .and_then(|value| Style::from_json(value).ok())
            .or_else(|| options.highlight_style.as_deref().and_then(Style::builtin))
            .unwrap_or_default();
        for kind in TokenType::ALL {
            let name = format!("{}Tok", kind.name());
            let style = palette.token_styles.get(&kind).copied().unwrap_or_default();
            let mut props = String::new();
            if let Some(color) = style.color.or(palette.default_color) {
                let _ = write!(props, " fo:color=\"{color}\"");
            }
            if let Some(color) = style.background.or(palette.background_color) {
                let _ = write!(props, " fo:background-color=\"{color}\"");
            }
            if style.bold {
                props.push_str(" fo:font-weight=\"bold\"");
            }
            if style.italic {
                props.push_str(" fo:font-style=\"italic\"");
            }
            if style.underline {
                props.push_str(" style:text-underline-style=\"solid\"");
            }
            styles.insert(name.clone(), format!("<style:style style:name=\"{name}\" style:family=\"text\"><style:text-properties{props} /></style:style>"));
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

fn metadata(meta: &Meta, timestamp: i64) -> String {
    let mut out = format!(
        "<office:meta><meta:generator>Randoc/{}</meta:generator>",
        env!("CARGO_PKG_VERSION")
    );
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
            let _ = write!(out, "<{tag}>{}</{tag}>", escaped(&value));
        }
    }
    let date = iso_time(timestamp);
    let _ = write!(
        out,
        "<meta:creation-date>{date}</meta:creation-date><dc:date>{date}</dc:date>"
    );
    for (name, value) in &meta.0 {
        if matches!(
            name.as_str(),
            "title" | "description" | "subject" | "keywords" | "lang" | "author"
        ) {
            continue;
        }
        if let Some(value) = meta_text(value) {
            let _ = write!(
                out,
                "<meta:user-defined meta:name=\"{}\" meta:value-type=\"string\">{}</meta:user-defined>",
                escaped(name),
                escaped(&value)
            );
        }
    }
    out.push_str("</office:meta>");
    out
}

fn meta_text(value: &MetaValue) -> Option<String> {
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
            let content = format!("{XML}{math}");
            let settings = format!(
                "{XML}<office:document-settings xmlns:office=\"{OFFICE}\" xmlns:config=\"{}\" office:version=\"1.3\"><office:settings><config:config-item-set config:name=\"ooo:configuration-settings\"><config:config-item config:name=\"IsTextMode\" config:type=\"boolean\">{is_inline}</config:config-item></config:config-item-set></office:settings></office:document-settings>",
                ns::CONFIG
            );
            self.resources.charge(
                self.context,
                content.len().saturating_add(settings.len()),
                2,
            )?;
            self.resources.insert(
                format!("{path}/content.xml"),
                "text/xml",
                content.into_bytes(),
            );
            self.resources.insert(
                format!("{path}/settings.xml"),
                "text/xml",
                settings.into_bytes(),
            );
            self.resources.next_index = index + 2;
            self.resources.formulas.insert(path.clone());
            format!(
                "<draw:object xlink:href=\"{path}/\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\" />"
            )
        };
        Ok(text(format!(
            "<draw:frame draw:style-name=\"{style}\" text:anchor-type=\"{anchor}\">{object}</draw:frame>"
        )))
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
        let mut reference = self.links.resolve(&target.url).into_owned();
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
                        let default_dpi = self.options.dpi.max(1) as f64;
                        let (x_dpi, y_dpi) =
                            image_density(&bytes).unwrap_or((default_dpi, default_dpi));
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
                        let extension = match mime.as_str() {
                            "image/png" => "png",
                            "image/jpeg" => "jpg",
                            "image/gif" => "gif",
                            "image/bmp" => "bmp",
                            "image/svg+xml" => "svg",
                            "image/webp" => "webp",
                            "image/tiff" => "tif",
                            _ => "bin",
                        };
                        loop {
                            let index = self.resources.next_index;
                            self.resources.next_index += 1;
                            reference = format!("Pictures/{index}.{extension}");
                            if !self.resources.parts.contains_key(&reference) {
                                break;
                            }
                        }
                        self.resources.charge(self.context, bytes.len(), 1)?;
                        self.resources.insert(reference.clone(), mime, bytes);
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
        let description = if alt.is_empty() || self.format == Format::Fragment {
            String::new()
        } else {
            format!("<svg:desc>{}</svg:desc>", escaped(&stringify(alt)))
        };
        Ok(text(format!(
            "<draw:frame {attrs}>{image}{description}</draw:frame>"
        )))
    }
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
}
