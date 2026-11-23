mod chapters;
mod metadata;
mod nav;
mod resources;

use std::collections::HashMap;
use std::fmt::Write as _;

use randoc_archive::media::image_size_and_mime;
use randoc_archive::zip::ArchiveWriter;
use randoc_ast::{Block, Meta, Pandoc};
use randoc_core::{BinaryWriter, Context, Error, Result, WriterOptions};
use randoc_html::{EpubChapter, EpubVersion, write_epub_chapter};
use randoc_templates::{Context as TemplateContext, Template, Val};

use self::metadata::{DC, Metadata, OPF};
use self::resources::{Resource, Resources, value_text};
use crate::{path, tree, xhtml};

const XHTML: &str = "application/xhtml+xml";
const XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
const IBOOKS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<display_options>\n  <platform name=\"*\">\n    <option name=\"specified-fonts\">true</option>\n  </platform>\n</display_options>\n";

pub(crate) struct EpubWriter(pub EpubVersion);

impl BinaryWriter for EpubWriter {
    fn renders_template(&self) -> bool {
        true
    }

    fn write(
        &self,
        doc: &Pandoc,
        options: &WriterOptions,
        context: &mut Context,
    ) -> Result<Vec<u8>> {
        write(doc, options, context, self.0)
    }
}

fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
    version: EpubVersion,
) -> Result<Vec<u8>> {
    let subdir = &options.epub_subdirectory;
    if !subdir.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(Error::EpubSubdirectory(subdir.clone()));
    }
    let depth = context.limits.max_nesting_depth.min(tree::STACK_DEPTH);
    tree::inspect(&doc.blocks, doc.meta.0.values(), depth)?;
    let modern = version == EpubVersion::Epub3;
    let mut metadata = Metadata::new(&doc.meta, options, context, modern)?;
    let template = match &options.template {
        Some(template) => template.clone(),
        None => default_template(version, depth)?,
    };
    let (chapters, headings) =
        chapters::split(doc.blocks.clone(), &metadata.title_inlines, options, depth)?;
    let mut resources = Resources::new(context.limits.max_archive_size);
    let css_var = options.variables.get("css");
    let css_meta = doc.meta.get("css").map(|value| {
        Val::from_meta_value(
            value,
            &mut |blocks| randoc_layout::literal(randoc_core::shared::stringify_blocks(blocks)),
            &mut |inlines| randoc_layout::literal(randoc_core::shared::stringify(inlines)),
        )
    });
    let styles = resources::styles(css_var.or(css_meta.as_ref()), &mut resources, context)?;
    let mut fonts = Vec::new();
    for source in &options.epub_fonts {
        let font = resources::font(source, context)?;
        resources.charge(font.bytes.len())?;
        fonts.push(font);
    }
    let mut base = options.variables.clone();
    base.0.remove("epub-cover-image");
    base.insert("lang", Val::text(path::escape(&metadata.language)));
    for key in ["dir", "csl-css"] {
        if !base.0.contains_key(key)
            && let Some(value) = metadata::text(&doc.meta, key)
        {
            base.insert(
                key,
                if key == "csl-css" {
                    Val::Bool(true)
                } else {
                    Val::text(path::escape(&value))
                },
            );
        }
    }
    base.insert("abstract-title", Val::text("Abstract"));
    let page_limit = usize::try_from(context.limits.max_archive_size).unwrap_or(usize::MAX);
    let pages = Pages {
        template,
        fields: base,
        styles: &styles,
        page_limit,
    };
    let mut rendered = Vec::new();
    let mut chapter_fields = pages.context("", None, "../");
    for (index, chapter) in chapters.iter().enumerate() {
        let html = write_epub_chapter(&chapter.blocks, options, context, version)?;
        let (body, remote) = embed_markup(&html.body, &mut resources, context)?;
        chapter_fields.insert("pagetitle", Val::text(chapters::name(index)));
        chapter_fields.insert("body-type", Val::text(chapter.matter));
        chapter_fields.insert("body", Val::text(body));
        let saved_css = html.highlighting_css.as_ref().map(|css| {
            chapter_fields
                .0
                .insert("highlighting-css".into(), Val::text(css))
        });
        let output = pages.render(&chapter_fields)?;
        if let Some(saved_css) = saved_css {
            if let Some(css) = saved_css {
                chapter_fields.insert("highlighting-css", css);
            } else {
                chapter_fields.0.remove("highlighting-css");
            }
        }
        resources.charge(output.len())?;
        rendered.push((chapter_resource(index, output), properties(&html, remote)));
    }

    let cover_source = options
        .variables
        .get("epub-cover-image")
        .and_then(value_text)
        .or_else(|| metadata::text(&doc.meta, "cover-image"));
    let cover_page = if let Some(source) = cover_source.as_deref() {
        let item = resources::fetch(source, context)?;
        let (width, height) = image_size_and_mime(&item.contents, Some(source))
            .0
            .unwrap_or((600, 800));
        resources.add_media(source, item)?;
        let mut fields = pages.context(&metadata.title, None, "../");
        fields.insert("coverpage", Val::Bool(true));
        if let Some(resource) = resources.media.last() {
            fields.insert("cover-image", Val::text(path::basename(&resource.href)));
        }
        fields.insert("cover-image-width", Val::text(width.to_string()));
        fields.insert("cover-image-height", Val::text(height.to_string()));
        Some(pages.render(&fields)?)
    } else {
        None
    };

    let title_page = if options.epub_title_page {
        let title_content = title_fields(&doc.meta, options, context, version)?;
        let has_title = title_content.get("title").is_some();
        let mut fields = pages.context(&metadata.title, Some("frontmatter"), "../");
        fields.insert("titlepage", Val::Bool(true));
        fields.0.extend(title_content.0);
        Some((pages.render(&fields)?, has_title))
    } else {
        None
    };

    let (nav_body, ncx) = nav::documents(
        &headings,
        &metadata,
        options,
        context,
        version,
        cover_source.as_deref(),
    )?;
    let nav_title = metadata
        .document_title
        .as_deref()
        .filter(|title| !title.is_empty())
        .unwrap_or("-");
    let mut nav_fields = pages.context(nav_title, Some("frontmatter"), "");
    for key in ["include-before", "include-after"] {
        nav_fields.0.remove(key);
    }
    nav_fields.insert("body", Val::text(nav_body));
    let nav = pages.render(&nav_fields)?;
    let (cover, body_media) = if cover_page.is_some() {
        resources
            .media
            .split_last()
            .map_or((None, &[][..]), |(cover, media)| (Some(cover), media))
    } else {
        (None, resources.media.as_slice())
    };
    metadata.append_package_metadata(&doc.meta, cover.map(|item| item.id.as_str()), modern);
    let mut opf = PackageDocument::new(&metadata, modern);
    opf.item("ncx", "toc.ncx", "application/x-dtbncx+xml", "");
    opf.item("nav", "nav.xhtml", XHTML, "nav");
    for entry in &styles {
        opf.resource(entry, "");
    }
    if let Some(cover) = &cover_page {
        opf.item(
            "cover_xhtml",
            "text/cover.xhtml",
            XHTML,
            if cover.contains("<svg") { "svg" } else { "" },
        );
        opf.spine("cover_xhtml", "");
    }
    if let Some((_, has_title)) = &title_page {
        opf.item("title_page_xhtml", "text/title_page.xhtml", XHTML, "");
        opf.spine(
            "title_page_xhtml",
            if *has_title {
                " linear=\"yes\""
            } else {
                " linear=\"no\""
            },
        );
    }
    if options.table_of_contents {
        opf.spine("nav", "");
    }
    for (entry, props) in &rendered {
        opf.resource(entry, props);
        opf.spine(&entry.id, "");
    }
    if let Some(entry) = cover {
        opf.resource(entry, "cover-image");
    }
    for entry in body_media.iter().rev() {
        opf.resource(entry, "");
    }
    for entry in &fonts {
        opf.resource(entry, "");
    }
    let opf = opf.finish(
        metadata::text(&doc.meta, "page-progression-direction").as_deref(),
        cover_page.is_some(),
    );
    let container = format!(
        "{XML}<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n  <rootfiles>\n    <rootfile full-path=\"{}\" media-type=\"application/oebps-package+xml\" />\n  </rootfiles>\n</container>\n",
        path::join(subdir, "content.opf")
    );

    let mut zip = PackageZip::new(context, metadata.timestamp)?;
    zip.add("META-INF/container.xml", container.as_bytes())?;
    zip.add(
        "META-INF/com.apple.ibooks.display-options.xml",
        IBOOKS.as_bytes(),
    )?;
    let mut add = |name: &str, bytes: &[u8]| zip.add(&path::join(subdir, name), bytes);
    add("content.opf", opf.as_bytes())?;
    add("toc.ncx", ncx.as_bytes())?;
    add("nav.xhtml", nav.as_bytes())?;
    if let Some((page, _)) = title_page {
        add("text/title_page.xhtml", page.as_bytes())?;
    }
    for entry in styles.iter().chain(body_media.iter().rev()).chain(cover) {
        add(&entry.href, &entry.bytes)?;
    }
    if let Some(page) = cover_page {
        add("text/cover.xhtml", page.as_bytes())?;
    }
    for (entry, _) in rendered {
        add(&entry.href, &entry.bytes)?;
    }
    let last_font: HashMap<_, _> = fonts
        .iter()
        .enumerate()
        .map(|(index, entry)| (&entry.href, index))
        .collect();
    for (index, entry) in fonts.iter().enumerate() {
        if last_font.get(&entry.href) == Some(&index) {
            add(&entry.href, &entry.bytes)?;
        }
    }
    zip.archive
        .finish()
        .map_err(|error| Error::App(error.to_string()))
}

fn default_template(version: EpubVersion, depth: usize) -> Result<Template> {
    let format = if version == EpubVersion::Epub3 {
        "epub3"
    } else {
        "epub2"
    };
    let source =
        randoc_data::default_template(format).ok_or_else(|| Error::NoTemplate(format.into()))?;
    Template::compile(&format!("default.{format}"), source, depth, &mut |name| {
        randoc_data::data_file(&format!("templates/{name}"))
            .and_then(|data| std::str::from_utf8(data).ok())
            .map(str::to_owned)
            .ok_or_else(|| Error::CouldNotFindDataFile(name.to_owned()))
    })
    .map_err(|error| Error::Template(error.to_string()))
}

struct Pages<'a> {
    template: Template,
    fields: TemplateContext,
    styles: &'a [Resource],
    page_limit: usize,
}

impl Pages<'_> {
    fn context(&self, title: &str, matter: Option<&str>, prefix: &str) -> TemplateContext {
        let mut fields = self.fields.clone();
        fields.insert("pagetitle", Val::text(path::escape(title)));
        if let Some(matter) = matter {
            fields.insert("body-type", Val::text(matter));
        }
        fields.insert(
            "css",
            Val::List(
                self.styles
                    .iter()
                    .map(|style| Val::text(format!("{prefix}{}", style.href)))
                    .collect(),
            ),
        );
        fields
    }

    fn render(&self, fields: &TemplateContext) -> Result<String> {
        self.template
            .render_limited(fields, self.page_limit, self.page_limit)
            .map(|mut page| {
                if !page.ends_with('\n') {
                    page.push('\n');
                }
                page
            })
            .ok_or_else(|| Error::App("The EPUB page exceeds the size limit.".into()))
    }
}

fn title_fields(
    meta: &Meta,
    options: &WriterOptions,
    context: &mut Context,
    version: EpubVersion,
) -> Result<TemplateContext> {
    let mut fields = TemplateContext::new();
    for key in [
        "title",
        "subtitle",
        "author",
        "creator",
        "publisher",
        "date",
        "rights",
        "abstract",
    ] {
        if let Some(value) = options.variables.get(key) {
            fields.insert(key, value.clone());
        } else if let Some(value) = meta.get(key) {
            fields.insert(key, metadata_value(value, options, context, version)?);
        }
    }
    Ok(fields)
}

fn metadata_value(
    value: &randoc_ast::MetaValue,
    options: &WriterOptions,
    context: &mut Context,
    version: EpubVersion,
) -> Result<Val> {
    use randoc_ast::MetaValue::*;
    Ok(match value {
        MetaString(value) => Val::text(path::escape(value)),
        MetaBool(value) => Val::Bool(*value),
        MetaList(items) => Val::List(
            items
                .iter()
                .map(|item| metadata_value(item, options, context, version))
                .collect::<Result<_>>()?,
        ),
        MetaMap(items) => Val::Map(TemplateContext(
            items
                .iter()
                .map(|(key, item)| {
                    Ok((
                        key.clone(),
                        metadata_value(item, options, context, version)?,
                    ))
                })
                .collect::<Result<_>>()?,
        )),
        MetaInlines(inlines) => Val::text(
            write_epub_chapter(&[Block::Plain(inlines.clone())], options, context, version)?.body,
        ),
        MetaBlocks(blocks) => {
            Val::text(write_epub_chapter(blocks, options, context, version)?.body)
        }
    })
}

fn chapter_resource(index: usize, page: String) -> Resource {
    let name = chapters::name(index);
    Resource {
        id: name.replace('.', "_"),
        href: format!("text/{name}"),
        mime: XHTML.into(),
        bytes: page.into_bytes(),
    }
}

fn properties(html: &EpubChapter, remote: bool) -> String {
    [
        (html.has_math, "mathml"),
        (html.has_svg, "svg"),
        (html.has_script, "scripted"),
        (remote, "remote-resources"),
    ]
    .into_iter()
    .filter_map(|(has, name)| has.then_some(name))
    .collect::<Vec<_>>()
    .join(" ")
}

struct PackageZip {
    archive: ArchiveWriter,
    timestamp: Option<i64>,
    remaining: usize,
}

impl PackageZip {
    fn new(context: &Context, timestamp: i64) -> Result<Self> {
        let remaining = context
            .limits
            .max_archive_entries
            .checked_sub(1)
            .ok_or_else(|| Error::App("The EPUB exceeds the archive entry limit.".into()))?;
        let mut archive = ArchiveWriter::with_input_limit(context.limits.max_archive_size);
        let timestamp = Some(timestamp);
        archive
            .write_stored_with_time("mimetype", b"application/epub+zip", timestamp)
            .map_err(|error| Error::App(error.to_string()))?;
        Ok(Self {
            archive,
            timestamp,
            remaining,
        })
    }

    fn add(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or_else(|| Error::App("The EPUB exceeds the archive entry limit.".into()))?;
        self.archive
            .write_deflated_with_time(name, bytes, self.timestamp)
            .map_err(|error| Error::App(error.to_string()))
    }
}

fn escape_package(text: &str) -> String {
    path::escape(text).replace('\'', "&#39;")
}

struct PackageDocument<'a> {
    metadata: &'a Metadata,
    modern: bool,
    manifest: String,
    spine: String,
}

impl<'a> PackageDocument<'a> {
    fn new(metadata: &'a Metadata, modern: bool) -> Self {
        Self {
            metadata,
            modern,
            manifest: String::new(),
            spine: String::new(),
        }
    }

    fn item(&mut self, id: &str, href: &str, mime: &str, props: &str) {
        self.manifest.push_str("    <item");
        if self.modern && props == "cover-image" {
            self.manifest.push_str(" properties=\"cover-image\"");
        }
        let _ = write!(
            self.manifest,
            " id=\"{}\" href=\"{}\" media-type=\"{}\"",
            escape_package(id),
            escape_package(href),
            escape_package(mime)
        );
        if self.modern && !props.is_empty() && props != "cover-image" {
            let _ = write!(self.manifest, " properties=\"{props}\"");
        }
        self.manifest.push_str(" />\n");
    }

    fn resource(&mut self, entry: &Resource, properties: &str) {
        self.item(&entry.id, &entry.href, &entry.mime, properties);
    }

    fn spine(&mut self, id: &str, linear: &str) {
        let _ = writeln!(self.spine, "    <itemref idref=\"{id}\"{linear} />");
    }

    fn finish(self, direction: Option<&str>, cover: bool) -> String {
        let version = if self.modern { "3.0" } else { "2.0" };
        let prefix = if self.modern {
            " prefix=\"ibooks: http://vocabulary.itunes.apple.com/rdf/ibooks/vocabulary-extensions-1.0/\""
        } else {
            ""
        };
        let language = if self.modern {
            format!(" xml:lang=\"{}\"", escape_package(&self.metadata.language))
        } else {
            String::new()
        };
        let direction = match direction.filter(|_| self.modern) {
            Some("ltr") => " page-progression-direction=\"ltr\"",
            Some("rtl") => " page-progression-direction=\"rtl\"",
            _ => "",
        };
        let cover = if cover {
            "    <reference type=\"cover\" title=\"Cover\" href=\"text/cover.xhtml\" />\n"
        } else {
            ""
        };
        let title = escape_package(&self.metadata.title);
        format!(
            "{XML}<package version=\"{version}\" xmlns=\"{OPF}\"{language} unique-identifier=\"epub-id-1\"{prefix}>\n  <metadata xmlns:dc=\"{DC}\" xmlns:opf=\"{OPF}\">\n{}  </metadata>\n  <manifest>\n{}  </manifest>\n  <spine toc=\"ncx\"{direction}>\n{}  </spine>\n  <guide>\n    <reference type=\"toc\" title=\"{title}\" href=\"nav.xhtml\" />\n{cover}  </guide>\n</package>\n",
            self.metadata.xml, self.manifest, self.spine
        )
    }
}

/// Rewrites resource URLs in generated tags and in raw XHTML.
fn embed_markup(
    body: &str,
    resources: &mut Resources,
    context: &mut Context,
) -> Result<(String, bool)> {
    let mut output = String::new();
    let mut rest = body;
    let mut remote = false;
    while let Some((text, tail)) = rest.split_once('<') {
        output.push_str(text);
        output.push('<');
        if tail.starts_with("!--") {
            let Some((comment, after)) = tail.split_once("-->") else {
                rest = tail;
                break;
            };
            output.push_str(comment);
            output.push_str("-->");
            rest = after;
            continue;
        }
        let Some(end) = xhtml::end_of_tag(tail) else {
            rest = tail;
            break;
        };
        let (tag, after) = (&tail[..end], &tail[end + 1..]);
        let name = tag
            .split_ascii_whitespace()
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        if matches!(name, "img" | "audio" | "video" | "source") {
            let mut start = 0;
            for (index, _) in tag.match_indices(['"', '\'']) {
                if index < start {
                    continue;
                }
                let key = tag[start..index]
                    .trim_end()
                    .strip_suffix('=')
                    .and_then(|s| s.split_ascii_whitespace().next_back());
                let quote = tag.as_bytes()[index] as char;
                let Some(end) = tag[index + 1..].find(quote).map(|end| index + end + 1) else {
                    break;
                };
                output.push_str(&tag[start..index + 1]);
                let value = &tag[index + 1..end];
                if key == Some("src") || (name == "video" && key == Some("poster")) {
                    let url = value
                        .replace("&quot;", "\"")
                        .replace("&apos;", "'")
                        .replace("&lt;", "<")
                        .replace("&gt;", ">")
                        .replace("&amp;", "&");
                    let rewritten = resources.embed(&url, context)?;
                    remote |= rewritten.starts_with("http://")
                        || rewritten.starts_with("https://")
                        || rewritten.starts_with("//");
                    let rewritten = path::escape(&rewritten);
                    if quote == '\'' {
                        output.push_str(&rewritten.replace('\'', "&apos;"));
                    } else {
                        output.push_str(&rewritten);
                    }
                } else {
                    output.push_str(value);
                }
                output.push(quote);
                start = end + 1;
            }
            output.push_str(&tag[start..]);
        } else {
            output.push_str(tag);
        }
        output.push('>');
        rest = after;
    }
    output.push_str(rest);
    Ok((output, remote))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn embed_markup_scales_linearly_with_unterminated_comments() {
        let mut context = Context::pure();
        let mut resources = Resources::new(1_000_000);
        let mut run = |count| {
            let input = "<!-- > ".repeat(count);
            let start = Instant::now();
            let (output, remote) = embed_markup(&input, &mut resources, &mut context).unwrap();
            let elapsed = start.elapsed();
            assert_eq!(output, input);
            assert!(!remote);
            elapsed
        };
        let small = run(16_384);
        let large = run(262_144);
        assert!(
            large < small * 32 + Duration::from_millis(150),
            "{small:?} for the small input, {large:?} for the large input"
        );
    }
}
