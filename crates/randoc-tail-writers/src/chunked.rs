use std::collections::{BTreeSet, HashMap};
use std::fmt::Write;

use randoc_archive::{
    media::{hashed_media_path, is_safe_media_path},
    zip::ArchiveWriter,
};
use randoc_ast::walk::{query_inlines, walk_inlines_mut};
use randoc_ast::{Block, Inline, Pandoc};
use randoc_core::shared::{auto_identifier, chunks::split_into_chunks, stringify, stringify_meta};
use randoc_core::{BinaryWriter, Context, Error, Result, WriterOptions};
use randoc_html::shared::{escape_attribute, escape_text};
use randoc_templates::{Context as TemplateContext, Val};

pub(super) struct ChunkedHtml;

struct Section {
    id: String,
    title: String,
    level: i64,
    number: Option<String>,
    path: String,
    children: Vec<usize>,
    unlisted: bool,
}

struct Page {
    path: String,
    title: String,
    up: usize,
    blocks: Vec<Block>,
}

impl BinaryWriter for ChunkedHtml {
    fn renders_template(&self) -> bool {
        true
    }

    fn write(
        &self,
        doc: &Pandoc,
        options: &WriterOptions,
        context: &mut Context,
    ) -> Result<Vec<u8>> {
        super::validate(doc, context)?;
        let depth = context.limits.max_nesting_depth;
        let template = match &options.template {
            Some(template) => template.clone(),
            None => super::default_template("chunkedhtml", depth)?,
        };
        let writer =
            randoc_html::writer("html5").ok_or_else(|| Error::UnknownWriter("html5".into()))?;
        let title = doc
            .meta
            .get("title")
            .map_or_else(String::new, stringify_meta);
        let title_slug = auto_identifier(&[Inline::Str(title.clone())], options.extensions);
        // Limit the depth of recursive section and HTML processing.
        let mut chunks = split_into_chunks(
            doc,
            options.split_level,
            &options.chunk_template,
            true,
            depth.min(128),
        )?;
        let mut path_changes = HashMap::new();
        for chunk in &mut chunks {
            let path = if chunk.level == 0 {
                preamble_path(&options.chunk_template, &title_slug, &title)
            } else {
                safe_path(&chunk.path)
            };
            path_changes
                .entry(chunk.path.clone())
                .or_insert_with(|| path.clone());
            chunk.path = path;
        }

        let mut pages = vec![Page {
            path: "index.html".into(),
            title: title.clone(),
            up: 0,
            blocks: vec![],
        }];
        let mut sections = vec![Section {
            id: String::new(),
            title,
            level: 0,
            number: None,
            path: "index.html".into(),
            children: vec![],
            unlisted: false,
        }];
        let mut ancestors = vec![0];
        let mut chunk_ancestors: Vec<(i64, usize)> = Vec::new();
        let mut preamble = 0;
        let mut media_seen = BTreeSet::new();
        let mut media_order = Vec::new();

        for mut chunk in chunks {
            let page = pages.len();
            query_inlines(&chunk.blocks, |inline| {
                if let Inline::Image(_, _, target) = inline
                    && !target.url.contains(':')
                    && is_safe_media_path(&target.url)
                    && media_seen.insert(target.url.clone())
                {
                    media_order.push(target.url.clone());
                }
            });
            walk_inlines_mut(&mut chunk.blocks, |inline| {
                if let Inline::Link(_, _, target) = inline
                    && let Some((path, id)) = target.url.rsplit_once('#')
                    && let Some(replacement) = path_changes.get(path)
                {
                    target.url = format!("{replacement}#{}{id}", options.identifier_prefix);
                }
            });
            if chunk.level == 0 {
                preamble = page;
            }
            let mut parent = preamble;
            while let Some(&(level, _)) = chunk_ancestors.last() {
                if level < chunk.level {
                    break;
                }
                chunk_ancestors.pop();
            }
            if let Some(&(_, up)) = chunk_ancestors.last() {
                parent = up;
            }
            if preamble != page {
                chunk_ancestors.push((chunk.level, page));
            } else {
                parent = 0;
            }
            let mut blocks = Vec::new();
            flatten_sections(
                chunk.blocks,
                &chunk.path,
                options,
                &mut sections,
                &mut ancestors,
                &mut blocks,
            );
            let page_title = if chunk.level == 0 {
                pages[0].title.clone()
            } else {
                stringify(&chunk.title)
            };
            pages.push(Page {
                path: chunk.path,
                title: page_title,
                up: parent,
                blocks,
            });
        }

        let limits = context.limits;
        if pages
            .len()
            .saturating_add(1)
            .saturating_add(media_order.len())
            > limits.max_archive_entries
        {
            return Err(Error::App("The ZIP archive has too many entries.".into()));
        }
        let sitemap = sitemap(&sections);
        let mut remaining = limits.max_input_size;
        super::charge_output(&mut remaining, sitemap.len())?;
        let timestamp = context
            .host()
            .environment_variable("SOURCE_DATE_EPOCH")
            .and_then(|time| time.parse().ok())
            .unwrap_or_else(|| context.host().current_time().unix_seconds);
        let mut archive = ArchiveWriter::with_input_limit(limits.max_archive_size);
        let mut used = BTreeSet::new();
        add(
            &mut archive,
            &mut used,
            "sitemap.json",
            sitemap.as_bytes(),
            timestamp,
            limits.max_archive_entries,
        )?;
        let toc = options.table_of_contents.then(|| toc(&sections, options));
        let mut page_options = options.clone();
        page_options.template = Some(template);
        page_options.table_of_contents = false;
        page_options.number_sections = false;
        page_options.section_divs = false;
        if page_options.variables.get("lang").is_none() && doc.meta.get("lang").is_none() {
            page_options.variables.insert("lang", Val::text(""));
        }
        let mut media = Vec::new();
        let mut media_bytes = 0u64;
        for source in media_order {
            let bytes = super::resource::fetch(&source, context)?;
            media_bytes = media_bytes
                .checked_add(bytes.len() as u64)
                .filter(|&total| total <= limits.max_archive_size)
                .ok_or_else(|| Error::App("The ZIP resources exceed the size limit.".into()))?;
            media.push((source, bytes));
        }
        let mut bodies = Vec::new();
        for page in &mut pages {
            bodies.push(std::mem::take(&mut page.blocks));
        }
        for (index, blocks) in bodies.into_iter().enumerate() {
            let page = &pages[index];
            if used.contains(&page.path) {
                continue;
            }
            let mut current = page_options.clone();
            let title = if page.title.is_empty() {
                "-"
            } else {
                &page.title
            };
            current
                .variables
                .insert("pagetitle", Val::text(escape_attribute(title, false)));
            if index > 0 {
                current.variables.insert("up", nav(&pages[page.up]));
                current.variables.insert("top", nav(&pages[0]));
                current.variables.insert("previous", nav(&pages[index - 1]));
            } else if let Some(toc) = &toc {
                current.variables.insert("toc", Val::Bool(true));
                current
                    .variables
                    .insert("table-of-contents", Val::text(toc));
            }
            if let Some(next) = pages.get(index + 1) {
                current.variables.insert("next", nav(next));
            }
            if remaining == 0 {
                return Err(super::output_limit_error());
            }
            let page_doc = Pandoc {
                meta: doc.meta.clone(),
                blocks,
            };
            context.limits.max_nesting_depth = depth.saturating_add(1);
            context.limits.max_input_size = remaining;
            let rendered = writer.write(&page_doc, &current, context);
            context.limits = limits;
            let mut html = rendered?.into_bytes();
            if !html.ends_with(b"\n") {
                html.push(b'\n');
            }
            super::charge_output(&mut remaining, html.len())?;
            add(
                &mut archive,
                &mut used,
                &page.path,
                &html,
                timestamp,
                limits.max_archive_entries,
            )?;
        }
        for (path, bytes) in media {
            add(
                &mut archive,
                &mut used,
                &path,
                &bytes,
                timestamp,
                limits.max_archive_entries,
            )?;
        }
        let result = archive
            .finish()
            .map_err(|error| Error::App(error.to_string()))?;
        if result.len() as u64 > limits.max_archive_size {
            return Err(Error::App("The ZIP archive exceeds the size limit.".into()));
        }
        Ok(result)
    }
}

fn add(
    archive: &mut ArchiveWriter,
    paths: &mut BTreeSet<String>,
    path: &str,
    bytes: &[u8],
    time: i64,
    max: usize,
) -> Result<()> {
    if paths.contains(path) {
        return Ok(());
    }
    if paths.len() >= max {
        return Err(Error::App("The ZIP archive has too many entries.".into()));
    }
    archive
        .write_deflated_with_time(path, bytes, Some(time))
        .map_err(|error| Error::App(error.to_string()))?;
    paths.insert(path.into());
    Ok(())
}

fn nav(page: &Page) -> Val {
    let mut map = TemplateContext::new();
    map.insert("url", Val::text(escape_attribute(&page.path, false)));
    map.insert("title", Val::text(escape_text(&page.title, false)));
    Val::Map(map)
}

fn preamble_path(template: &str, id: &str, title: &str) -> String {
    let mut rest = template;
    let mut path = String::new();
    while let Some((before, after)) = rest.split_once('%') {
        path.push_str(before);
        match after.as_bytes().first() {
            Some(b'i') => {
                path.push_str(id);
                path.push_str("-1");
            }
            Some(b'n') => path.push_str("001"),
            Some(b's') => path.push('0'),
            Some(b'h') => path.push_str(title),
            _ => {
                path.push('%');
                rest = after;
                continue;
            }
        }
        rest = &after[1..];
    }
    path.push_str(rest);
    safe_path(&path)
}

fn safe_path(path: &str) -> String {
    let clean = path.replace(['/', '\\'], "");
    if is_safe_media_path(&clean) {
        clean
    } else {
        hashed_media_path(path, path.as_bytes())
    }
}

fn flatten_sections(
    blocks: Vec<Block>,
    path: &str,
    options: &WriterOptions,
    sections: &mut Vec<Section>,
    ancestors: &mut Vec<usize>,
    output: &mut Vec<Block>,
) {
    let mut work: Vec<_> = blocks.into_iter().rev().collect();
    while let Some(block) = work.pop() {
        let Block::Div(mut section, blocks) = block else {
            output.push(block);
            continue;
        };
        let mut iter = blocks.into_iter();
        let Some(mut first) = iter.next() else {
            output.push(Block::Div(section, vec![]));
            continue;
        };
        if let Block::Header(level, attr, inlines) = &mut first
            && section
                .classes
                .first()
                .is_some_and(|class| class == "section")
        {
            let level = *level;
            while ancestors.len() > 1
                && ancestors
                    .last()
                    .is_some_and(|index| sections[*index].level >= level)
            {
                ancestors.pop();
            }
            let number = section.get("number").map(str::to_owned);
            let id = std::mem::take(&mut section.id);
            let title = stringify(inlines);
            let index = sections.len();
            if let Some(&parent) = ancestors.last() {
                sections[parent].children.push(index);
            }
            sections.push(Section {
                id: id.clone(),
                title,
                level,
                number: number.clone(),
                path: format!("{path}#{}{id}", options.identifier_prefix),
                children: vec![],
                unlisted: attr.has_class("unlisted") || section.has_class("unlisted"),
            });
            ancestors.push(index);
            attr.id = id;
            for class in section.classes.into_iter().skip(1) {
                if !attr.classes.contains(&class) {
                    attr.classes.push(class);
                }
            }
            if let Some(number) = number {
                attr.attributes.retain(|(key, _)| key != "number");
                attr.attributes.insert(0, ("number".into(), number.clone()));
                if options.number_sections {
                    let number = Inline::RawInline(
                        randoc_ast::Format::new("html"),
                        format!(
                            "<span class=\"header-section-number\">{}</span>",
                            escape_text(&number, false)
                        ),
                    );
                    inlines.splice(..0, [number, Inline::Space]);
                }
            }
            output.push(first);
            work.extend(iter.rev());
        } else {
            output.push(Block::Div(
                section,
                std::iter::once(first).chain(iter).collect(),
            ));
        }
    }
}

fn json_string(text: &str) -> String {
    randoc_ast::json::value_to_json(&text.to_owned())
}

fn sitemap(sections: &[Section]) -> String {
    enum Event {
        Section(usize),
        End,
        Comma,
    }
    let mut work = vec![Event::Section(0)];
    let mut output = String::new();
    while let Some(event) = work.pop() {
        match event {
            Event::Section(index) => {
                let section = &sections[index];
                let number = section
                    .number
                    .as_deref()
                    .map_or_else(|| "null".into(), json_string);
                let _ = write!(
                    output,
                    "{{\"section\":{{\"id\":{},\"level\":\"{}\",\"number\":{number},\"path\":{},\"title\":{}}},\"subsections\":[",
                    json_string(&section.id),
                    section.level,
                    json_string(&section.path),
                    json_string(&section.title)
                );
                work.push(Event::End);
                for (index, &child) in section.children.iter().enumerate().rev() {
                    work.push(Event::Section(child));
                    if index != 0 {
                        work.push(Event::Comma);
                    }
                }
            }
            Event::Comma => output.push(','),
            Event::End => output.push_str("]}"),
        }
    }
    output
}

fn toc(sections: &[Section], options: &WriterOptions) -> String {
    enum Event {
        List(usize),
        Item(usize),
        EndList,
        EndItem,
    }
    let mut out = String::new();
    let mut work = vec![Event::List(0)];
    while let Some(event) = work.pop() {
        match event {
            Event::List(index) => {
                let start = work.len();
                work.extend(
                    sections[index]
                        .children
                        .iter()
                        .rev()
                        .copied()
                        .filter(|&i| {
                            let section = &sections[i];
                            !section.unlisted && section.level <= options.toc_depth
                        })
                        .map(Event::Item),
                );
                if work.len() != start {
                    out.push_str("\n<ul>\n");
                    work.insert(start, Event::EndList);
                }
            }
            Event::Item(index) => {
                let section = &sections[index];
                let _ = write!(
                    out,
                    "<li><a href=\"{}\" id=\"{}toc-{}\">",
                    escape_attribute(&section.path, false),
                    options.identifier_prefix,
                    escape_attribute(&section.id, false)
                );
                if options.number_sections
                    && let Some(number) = &section.number
                {
                    let _ = write!(out, "<span class=\"toc-section-number\">{number}</span> ");
                }
                let _ = write!(out, "{}</a>", escape_text(&section.title, false));
                work.push(Event::EndItem);
                work.push(Event::List(index));
            }
            Event::EndItem => out.push_str("</li>\n"),
            Event::EndList => out.push_str("</ul>"),
        }
    }
    if out.is_empty() {
        return "<ul class=\"task-list\">\n</ul>".into();
    }
    out.trim_start_matches('\n').into()
}
