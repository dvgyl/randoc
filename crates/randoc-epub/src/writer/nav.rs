use std::fmt::Write as _;

use randoc_ast::Block;
use randoc_core::{Context, Result, WriterOptions};
use randoc_html::{EpubVersion, write_epub_chapter};

use super::{chapters::Heading, escape_package as escape, metadata::Metadata, resources::file_id};

pub(super) fn documents(
    headings: &[Heading],
    metadata: &Metadata,
    options: &WriterOptions,
    context: &mut Context,
    version: EpubVersion,
    cover: Option<&str>,
) -> Result<(String, String)> {
    let modern = version == EpubVersion::Epub3;
    let title = escape(&metadata.title);
    let uid = escape(&metadata.identifier);
    let nav_tag = if modern { "nav" } else { "div" };
    let toc_attrs = if modern {
        " epub:type=\"toc\" role=\"doc-toc\""
    } else {
        ""
    };
    let mut nav = format!("<{nav_tag}{toc_attrs} id=\"toc\"><h1 id=\"toc-title\">{title}</h1>");
    let cover_meta = cover.map_or_else(String::new, |source| {
        let id = escape(&file_id(crate::path::basename(source)));
        format!("    <meta name=\"cover\" content=\"{id}\" />\n")
    });
    let mut ncx = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ncx version=\"2005-1\" xmlns=\"http://www.daisy.org/z3986/2005/ncx/\">\n  <head>\n    <meta name=\"dtb:uid\" content=\"{uid}\" />\n    <meta name=\"dtb:depth\" content=\"1\" />\n    <meta name=\"dtb:totalPageCount\" content=\"0\" />\n    <meta name=\"dtb:maxPageNumber\" content=\"0\" />\n{cover_meta}  </head>\n  <docTitle>\n    <text>{title}</text>\n  </docTitle>\n  <navMap>\n"
    );
    if options.epub_title_page {
        let label = escape(metadata.document_title.as_deref().unwrap_or_default());
        nav_point(&mut ncx, 0, 0, &label, "text/title_page.xhtml");
        close_nav_point(&mut ncx, 0);
    }
    nav.push_str("<ol class=\"toc\">");
    let mut levels = Vec::new();
    let mut fragment_options = options.clone();
    fragment_options.highlight_style = None;
    for (index, heading) in headings.iter().enumerate() {
        while levels.last().is_some_and(|level| *level >= heading.level) {
            levels.pop();
            nav.push_str("</li>");
            close_nav_point(&mut ncx, levels.len());
            if levels.last().is_some_and(|level| *level >= heading.level) {
                nav.push_str("</ol>");
            }
        }
        if !levels.is_empty() && (index == 0 || headings[index - 1].level < heading.level) {
            nav.push_str("<ol class=\"toc\">");
        }
        levels.push(heading.level);
        let id = index + 1;
        let href = escape(&heading.href);
        let label = write_epub_chapter(
            &[Block::Plain(without_links(&heading.inlines))],
            &fragment_options,
            context,
            version,
        )?
        .body;
        let _ = write!(nav, "<li id=\"toc-li-{id}\"><a href=\"{href}\"");
        if heading.number.is_none() && label.is_empty() {
            nav.push_str(" />");
        } else {
            nav.push('>');
            if let Some(number) = &heading.number {
                let _ = write!(
                    nav,
                    "<span class=\"section-header-number\">{}</span>\u{a0}",
                    escape(number)
                );
            }
            let _ = write!(nav, "{label}</a>");
        }
        let label = match &heading.number {
            Some(number) => format!("{number} {}", heading.label),
            None => heading.label.clone(),
        };
        nav_point(&mut ncx, levels.len() - 1, id, &escape(&label), &href);
    }
    while levels.pop().is_some() {
        nav.push_str("</li>");
        close_nav_point(&mut ncx, levels.len());
        if !levels.is_empty() {
            nav.push_str("</ol>");
        }
    }
    let _ = writeln!(nav, "</ol></{nav_tag}>");
    ncx.push_str("  </navMap>\n</ncx>\n");
    if modern && (options.epub_title_page || cover.is_some() || options.table_of_contents) {
        nav.push_str("<nav epub:type=\"landmarks\" id=\"landmarks\" hidden=\"hidden\">\n  <ol>\n");
        if options.epub_title_page {
            nav.push_str("    <li>\n      <a href=\"text/title_page.xhtml\" epub:type=\"titlepage\">Title Page</a>\n    </li>\n");
        }
        if cover.is_some() {
            nav.push_str("    <li>\n      <a href=\"text/cover.xhtml\" epub:type=\"cover\">Cover</a>\n    </li>\n");
        }
        if options.table_of_contents {
            nav.push_str("    <li>\n      <a href=\"#toc\" epub:type=\"toc\">Table of Contents</a>\n    </li>\n");
        }
        nav.push_str("  </ol>\n</nav>\n");
    }
    Ok((nav, ncx))
}

fn without_links(inlines: &[randoc_ast::Inline]) -> Vec<randoc_ast::Inline> {
    use randoc_ast::Inline;
    let mut inlines = inlines.to_vec();
    randoc_ast::walk::walk_inlines_mut(&mut inlines, |inline| match inline {
        Inline::Link(_, text, _) => {
            *inline = Inline::Span(Default::default(), std::mem::take(text))
        }
        Inline::Note(_) => *inline = Inline::Str(String::new()),
        _ => {}
    });
    inlines
}

fn nav_point(ncx: &mut String, depth: usize, id: usize, label: &str, href: &str) {
    let indent = "  ".repeat(depth + 2);
    let _ = writeln!(
        ncx,
        "{indent}<navPoint id=\"navPoint-{id}\">\n{indent}  <navLabel>\n{indent}    <text>{label}</text>\n{indent}  </navLabel>\n{indent}  <content src=\"{href}\" />"
    );
}

fn close_nav_point(ncx: &mut String, depth: usize) {
    let indent = "  ".repeat(depth + 2);
    let _ = writeln!(ncx, "{indent}</navPoint>");
}
