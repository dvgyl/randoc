use std::collections::{HashMap, HashSet};

use randoc_archive::media::{image_size_and_mime, media_key, mime_from_extension};
use randoc_archive::xml::{self, XmlElement, XmlError, XmlLimits, XmlOptions};
use randoc_archive::zip::{Archive, ArchiveError, ArchiveLimits};
use randoc_ast::walk::{VisitorMut, walk_blocks_mut, walk_inlines_mut, walk_mut};
use randoc_ast::{Attr, Block, Inline, Meta, MetaValue, Pandoc, Target};
use randoc_core::{
    BinaryReader, Context, Error, Extension, MediaItem, Reader, ReaderOptions, Result, Sources,
};

use crate::{path, tree, xhtml};

const CONTAINER: &str = "META-INF/container.xml";
const CONTAINER_NS: &str = "urn:oasis:names:tc:opendocument:xmlns:container";
const DC: &str = "http://purl.org/dc/elements/1.1/";

pub(crate) struct EpubReader;

impl BinaryReader for EpubReader {
    fn read(&self, bytes: &[u8], options: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
        read(bytes, options, context)
    }
}

#[derive(Default)]
struct Item<'a> {
    id: &'a str,
    href: &'a str,
    mime: &'a str,
    properties: &'a str,
}

fn archive_error(error: ArchiveError) -> Error {
    Error::Parse(format!("Couldn't extract ePub file: {error}"))
}

fn entry(archive: &mut Archive<'_>, name: &str) -> Result<Vec<u8>> {
    archive
        .read(name)
        .map_err(archive_error)?
        .ok_or_else(|| missing(name))
}

fn missing(name: &str) -> Error {
    Error::Parse(format!("No entry on path: {name}"))
}

fn utf8(bytes: &[u8]) -> Result<&str> {
    xml::decode_utf8(bytes).map_err(|err| match err {
        XmlError::NotUtf8 { byte, .. } => Error::Fatal(format!(
            "Cannot decode byte '\\x{byte:02x}': Data.Text.Internal.Encoding: Invalid UTF-8 stream"
        )),
        other => Error::Fatal(other.format_pandoc()),
    })
}

fn document(bytes: &[u8], name: &str, limits: &XmlLimits) -> Result<XmlElement> {
    xml::parse(utf8(bytes)?, limits, XmlOptions::default())
        .map(|doc| doc.root)
        .map_err(|err| Error::Xml {
            file: format!("'{name}'"),
            message: err.format_pandoc(),
        })
}

fn read(bytes: &[u8], options: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    let mut archive =
        Archive::open(bytes, ArchiveLimits::from_limits(&context.limits)).map_err(archive_error)?;
    let mut xml_limits = XmlLimits::from_limits(&context.limits);
    xml_limits.max_depth = xml_limits.max_depth.min(tree::STACK_DEPTH);
    let container = document(&entry(&mut archive, CONTAINER)?, CONTAINER, &xml_limits)?;
    let rootfile = container
        .child(CONTAINER_NS, "rootfiles")
        .and_then(|files| files.child(CONTAINER_NS, "rootfile"))
        .ok_or_else(|| Error::Parse(format!(
            "Unable to find element: QName {{qName = \"rootfile\", qURI = Just \"{CONTAINER_NS}\", qPrefix = Nothing}}"
        )))?;
    let opf = rootfile.attr_unqualified("full-path").unwrap_or_default();
    let opf = path::resolve("", opf).ok_or_else(|| missing(opf))?;
    let package = document(&entry(&mut archive, &opf)?, &opf, &xml_limits)?;
    let dir = path::parent(&opf);
    let metadata = package.child_local("metadata");
    let items: Vec<_> = package
        .child_local("manifest")
        .into_iter()
        .flat_map(|manifest| manifest.children_local("item"))
        .map(|item| Item {
            id: item.attr_unqualified("id").unwrap_or_default(),
            href: item.attr_unqualified("href").unwrap_or_default(),
            mime: item.attr_unqualified("media-type").unwrap_or_default(),
            properties: item.attr_unqualified("properties").unwrap_or_default(),
        })
        .collect();
    let by_id: HashMap<_, _> = items.iter().map(|item| (item.id, item)).collect();
    let spine: Vec<_> = package
        .child_local("spine")
        .into_iter()
        .flat_map(|spine| spine.children_local("itemref"))
        .filter_map(|item| {
            by_id
                .get(item.attr_unqualified("idref")?)
                .copied()
                .map(|entry| (entry, item))
        })
        .collect();
    let basenames: HashSet<_> = spine
        .iter()
        .map(|(item, _)| path::basename(item.href).replace(' ', "%20"))
        .collect();
    let mut media = Media {
        archive: &mut archive,
        dir,
        mime: items
            .iter()
            .filter(|item| !item.mime.is_empty())
            .map(|item| (path::decode(item.href), item.mime))
            .collect(),
        loaded: HashSet::new(),
    };
    let mut blocks = Vec::new();
    if let Some(cover) = cover(&items, metadata) {
        let resolved = media.load(cover.href, context)?;
        blocks.push(Block::Para(vec![Inline::Image(
            Attr::default(),
            Vec::new(),
            Target {
                url: resolved,
                title: String::new(),
            },
        )]));
    }
    let Some(Reader::Text(html)) = randoc_html_reader::reader("html") else {
        return Err(Error::App("The HTML reader is unavailable.".into()));
    };
    let table_limit = context.limits.max_table_cells;
    let mut remaining = table_limit;
    for (item, itemref) in spine {
        context.check_deadline()?;
        if itemref
            .attr_unqualified("linear")
            .is_some_and(|value| value.eq_ignore_ascii_case("no"))
        {
            continue;
        }
        let name = path::basename(item.href);
        blocks.push(Block::Para(vec![Inline::Span(
            Attr::new(name, Vec::new(), Vec::new()),
            Vec::new(),
        )]));
        let href = path::decode(item.href);
        // Keep the manifest spelling on the chapter marker. The fragment
        // prefix uses the decoded name with spaces escaped.
        let chapter_id = path::decode(name).replace(' ', "%20");
        let archive_path =
            path::resolve(dir, &href).ok_or_else(|| missing(&path::join(dir, &href)))?;
        // Each occurrence consumes the cumulative archive budget because it
        // also contributes a separate chapter to the document.
        let bytes = entry(media.archive, &archive_path)?;
        if item.mime != "application/xhtml+xml" {
            continue;
        }
        let original = context.limits;
        let depth = original.max_nesting_depth.min(tree::STACK_DEPTH);
        let text = utf8(&bytes)?;
        let sources = if options.extensions.contains(Extension::EpubHtmlExts) {
            Sources::from_text(xhtml::select_switches(text, depth)?.into_owned())
        } else {
            Sources::from_text(text)
        };
        context.limits.max_table_cells = remaining;
        context.limits.max_nesting_depth = depth;
        let chapter = html.read(&sources, options, context);
        context.limits = original;
        let mut chapter = chapter?.blocks;
        let used = tree::inspect(&chapter, std::iter::empty(), original.max_nesting_depth)?;
        remaining = remaining.checked_sub(used).ok_or_else(|| {
            Error::Parse(format!(
                "the tables need more than {table_limit} cells that the input does not write"
            ))
        })?;
        if options.extensions.contains(Extension::EpubHtmlExts) {
            remove_title_pages(&mut chapter);
        }
        let rename = |attr: &mut Attr| {
            if !attr.id.is_empty() {
                attr.id = format!("{chapter_id}_{}", attr.id);
            }
        };
        walk_blocks_mut(&mut chapter, |block| {
            let table = matches!(block, Block::Table(..));
            let prefix_id = !matches!(block, Block::Table(..) | Block::Figure(..));
            tree::block_attrs(block, |attr| {
                if prefix_id {
                    rename(attr);
                }
                if !table {
                    attr.attributes.retain(|(key, _)| key != "epub:type");
                }
            });
        });
        let mut result = Ok(());
        walk_inlines_mut(&mut chapter, |inline| {
            let is_image = matches!(inline, Inline::Image(..));
            if let Some(attr) = tree::inline_attr(inline) {
                rename(attr);
                if !is_image {
                    attr.attributes.retain(|(key, _)| key != "epub:type");
                }
            }
            if result.is_err() {
                return;
            }
            match inline {
                Inline::Link(_, _, target) => {
                    rewrite_link(&mut target.url, &chapter_id, &basenames)
                }
                Inline::Image(_, _, target)
                    if !path::external(&target.url) && !target.url.starts_with('/') =>
                {
                    match media.load(&path::join(path::parent(item.href), &target.url), context) {
                        Ok(url) => target.url = url,
                        Err(error) => result = Err(error),
                    }
                }
                _ => {}
            }
        });
        result?;
        blocks.extend(chapter);
    }
    Ok(Pandoc {
        meta: read_metadata(metadata),
        blocks,
    })
}

fn remove_title_pages(blocks: &mut Vec<Block>) {
    struct TitlePages;
    impl VisitorMut for TitlePages {
        fn blocks(&mut self, blocks: &mut Vec<Block>) {
            blocks.retain(|block| !matches!(block, Block::Div(attr, _) if matches!(attr.get("epub:type"), Some("titlepage" | "halftitlepage"))));
        }
    }
    walk_mut(blocks, &mut TitlePages);
}

fn read_metadata(metadata: Option<&XmlElement>) -> Meta {
    let mut values: HashMap<String, Vec<_>> = HashMap::new();
    for element in metadata.into_iter().flat_map(XmlElement::elements) {
        if element.name.ns.as_deref() != Some(DC) {
            continue;
        }
        let name = match element.name.local.as_str() {
            "creator" => "author",
            "title" | "language" | "contributor" | "coverage" | "date" | "description"
            | "format" | "identifier" | "publisher" | "relation" | "rights" | "source"
            | "subject" | "type" => &element.name.local,
            _ => continue,
        };
        values
            .entry(name.to_owned())
            .or_default()
            .push(MetaValue::MetaInlines(vec![Inline::Str(
                element.all_text(),
            )]));
    }
    Meta(
        values
            .into_iter()
            .map(|(key, mut entries)| {
                let value = if entries.len() == 1 {
                    entries
                        .pop()
                        .unwrap_or_else(|| MetaValue::MetaList(Vec::new()))
                } else {
                    entries.reverse();
                    MetaValue::MetaList(entries)
                };
                (key, value)
            })
            .collect(),
    )
}

fn cover<'a>(items: &'a [Item<'a>], metadata: Option<&XmlElement>) -> Option<&'a Item<'a>> {
    items
        .iter()
        .find(|item| {
            item.properties
                .split_whitespace()
                .any(|p| p == "cover-image")
        })
        .or_else(|| {
            let name = metadata?
                .children_local("meta")
                .find(|meta| meta.attr_unqualified("name") == Some("cover"))?
                .attr_unqualified("content")?;
            items
                .iter()
                .find(|item| item.id == name)
                .or_else(|| items.iter().find(|item| item.href == name))
        })
}

fn rewrite_link(url: &mut String, chapter: &str, basenames: &HashSet<String>) {
    if path::external(url) {
        return;
    }
    if url == "#" {
        url.clear();
    } else if let Some(fragment) = url.strip_prefix('#') {
        *url = format!("#{chapter}_{fragment}");
    } else if let Some((target, fragment)) = url.split_once('#') {
        let target = path::basename(target);
        let prefix = if basenames.contains(target) { "#" } else { "" };
        *url = format!("{prefix}{target}_{fragment}");
    } else if basenames.contains(url.as_str()) {
        url.insert(0, '#');
    }
}

struct Media<'a, 'b> {
    archive: &'a mut Archive<'b>,
    dir: &'a str,
    mime: HashMap<String, &'a str>,
    loaded: HashSet<String>,
}

impl Media<'_, '_> {
    fn load(&mut self, href: &str, context: &mut Context) -> Result<String> {
        let display = path::normalize_relative(href);
        let decoded = path::decode(href);
        let Some(relative) = path::resolve("", &decoded) else {
            return Ok(display);
        };
        let full_path = path::join(self.dir, &relative);
        if self.loaded.contains(&display) {
            return Ok(display);
        }
        let Some(contents) = self.archive.read(&full_path).map_err(archive_error)? else {
            return Ok(display);
        };
        let mime = self
            .mime
            .get(&relative)
            .copied()
            .or_else(|| image_size_and_mime(&contents, Some(&display)).1)
            .or_else(|| mime_from_extension(&display))
            .unwrap_or("application/octet-stream");
        let key = media_key(&display, Some(mime), &contents);
        context.media.insert(
            key,
            MediaItem {
                mime_type: mime.into(),
                contents,
            },
        );
        self.loaded.insert(display.clone());
        Ok(display)
    }
}
