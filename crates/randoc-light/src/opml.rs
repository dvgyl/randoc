use randoc_archive::xml::{self, QName, XmlElement, XmlLimits, XmlNode, XmlOptions};
use randoc_ast::{Attr, Block, Inline, Meta, MetaValue, Pandoc, Target};
use randoc_core::shared::{meta_context, stringify_meta};
use randoc_core::{Context, Error, Reader, ReaderOptions, Result, Sources, WriterOptions};
use randoc_layout::literal;
use randoc_templates::Val;

use crate::{check_depth, expansion_budget, inline, write};

pub(crate) fn read(input: &str, options: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    let mut limits = XmlLimits::from_limits(&context.limits);
    let budget = expansion_budget(input.len(), context.limits.max_input_size);
    limits.max_entity_bytes = limits.max_entity_bytes.min(budget);
    limits.max_dom_bytes = limits.max_dom_bytes.min(budget);
    let doc = xml::parse(input, &limits, XmlOptions::default()).map_err(|error| Error::Xml {
        file: String::new(),
        message: error.format_pandoc(),
    })?;
    let mut meta = Meta::default();
    let head = doc.root.child_local("head");
    for (field, tag) in [("title", "title"), ("date", "dateModified")] {
        let value = head
            .and_then(|h| h.child_local(tag))
            .map(XmlElement::all_text)
            .unwrap_or_default();
        meta.insert(field, MetaValue::MetaInlines(metadata_text(&value)));
    }
    let owners = head
        .and_then(|h| h.child_local("ownerName"))
        .map(|owner| MetaValue::MetaInlines(metadata_text(&owner.all_text())))
        .into_iter()
        .collect();
    meta.insert("author", MetaValue::MetaList(owners));

    let Some(Reader::Text(markdown)) = randoc_markdown::reader("markdown") else {
        return Err(Error::UnknownReader("markdown".into()));
    };
    let mut note_options = options.clone();
    note_options.standalone = false;
    let mut blocks = Vec::new();
    let mut stack = Vec::new();
    let mut remaining = budget;
    if let Some(body) = doc.root.child_local("body") {
        stack.extend(body.children.iter().rev().map(|node| (node, 0usize)));
    }
    while let Some((node, mut depth)) = stack.pop() {
        let XmlNode::Element(element) = node else {
            continue;
        };
        if element.name.local == "outline" {
            depth += 1;
            check_depth(depth, context.limits.max_nesting_depth)?;
            let text = element.attr_unqualified("text").unwrap_or("");
            let note = element.attr_unqualified("_note").unwrap_or("");
            remaining = remaining
                .checked_sub(text.len().saturating_add(note.len()))
                .ok_or_else(|| Error::App("The outline attributes are too large.".into()))?;
            let mut label_limits = limits;
            label_limits.max_depth = label_limits.max_depth.saturating_sub(depth);
            let mut xs = read_label(
                text,
                &label_limits,
                options.extensions.contains(randoc_core::Extension::Smart),
            )?;
            if element
                .attr_unqualified("type")
                .is_some_and(|kind| kind.eq_ignore_ascii_case("link"))
            {
                xs = vec![Inline::Link(
                    Attr::default(),
                    xs,
                    Target::new(element.attr_unqualified("url").unwrap_or(""), ""),
                )];
            }
            blocks.push(Block::Header(
                i64::try_from(depth).unwrap_or(i64::MAX),
                Attr::default(),
                xs,
            ));
            if !note.trim().is_empty() {
                let original = context.limits;
                context.limits.max_nesting_depth = original.max_nesting_depth.saturating_sub(depth);
                let parsed = markdown.read(&Sources::from_text(note), &note_options, context);
                context.limits = original;
                blocks.extend(parsed?.blocks);
            }
        }
        stack.extend(element.children.iter().rev().map(|child| (child, depth)));
    }
    Ok(Pandoc { meta, blocks })
}

fn metadata_text(text: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    for word in text
        .split(inline::markup_space)
        .filter(|word| !word.is_empty())
    {
        if !out.is_empty() {
            out.push(Inline::Space);
        }
        out.push(Inline::Str(word.into()));
    }
    if text.starts_with(inline::markup_space) {
        out.insert(0, Inline::Space);
    }
    if text.ends_with(inline::markup_space) && !matches!(out.last(), Some(Inline::Space)) {
        out.push(Inline::Space);
    }
    out
}

fn read_label(text: &str, limits: &XmlLimits, smart: bool) -> Result<Vec<Inline>> {
    let normalized = close_void_tags(text);
    let parsed = xml::parse_fragment(
        &normalized,
        limits,
        XmlOptions {
            html_entities: true,
            ..Default::default()
        },
    );
    let mut xs = match parsed {
        Ok(fragment) => label_nodes(&fragment.nodes, 0, limits.max_depth, smart)?,
        Err(xml::XmlError::Depth(_)) => {
            return Err(Error::App("The outline label is nested too deeply.".into()));
        }
        Err(xml::XmlError::Syntax { .. } | xml::XmlError::UnknownEntity { .. }) => {
            let trimmed = text.trim();
            if let Some((tag, closing, token)) = label_tag(trimmed)
                && token == trimmed
                && !matches!(tag, "html" | "head" | "body")
                && (closing || !matches!(tag, "div" | "p"))
            {
                vec![Inline::RawInline(
                    randoc_ast::Format::new("html"),
                    token.into(),
                )]
            } else {
                label_nodes(&recover_label(text, limits)?, 0, limits.max_depth, smart)?
            }
        }
        Err(error) => return Err(Error::App(error.format_pandoc())),
    };
    while matches!(xs.last(), Some(Inline::Space | Inline::LineBreak)) {
        xs.pop();
    }
    let start = xs
        .iter()
        .take_while(|x| matches!(x, Inline::Space | Inline::LineBreak))
        .count();
    xs.drain(..start);
    for tag in ["meta", "link"] {
        let closing = format!("</{tag}>");
        let text = text.trim_end();
        let self_closing = text.ends_with("/>")
            && text
                .strip_prefix('<')
                .is_some_and(|text| text.starts_with(tag));
        if text.ends_with(&closing) || self_closing {
            xs.push(Inline::RawInline(randoc_ast::Format::new("html"), closing));
        }
    }
    Ok(xs)
}

fn close_void_tags(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start + 1]);
        rest = &rest[start + 1..];
        let name_end = rest
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(rest.len());
        if matches!(
            rest[..name_end].to_ascii_lowercase().as_str(),
            "meta" | "link" | "br" | "hr" | "img" | "input"
        ) && let Some(end) = rest.find(['<', '>'])
            && rest.as_bytes()[end] == b'>'
        {
            let tag = &rest[..end];
            out.push_str(tag);
            if !tag.trim_end().ends_with('/') {
                out.push('/');
            }
            out.push('>');
            rest = &rest[end + 1..];
        }
    }
    out.push_str(rest);
    out
}

fn recover_label(mut text: &str, limits: &XmlLimits) -> Result<Vec<XmlNode>> {
    let options = XmlOptions {
        html_entities: true,
        ..Default::default()
    };
    let mut root = Vec::new();
    let mut open: Vec<XmlElement> = Vec::new();
    let mut nodes_left = limits.max_nodes;
    while !text.is_empty() {
        nodes_left = nodes_left
            .checked_sub(1)
            .ok_or_else(|| Error::App("The outline label has too many nodes.".into()))?;
        if let Some((tag, closing, token)) = label_tag(text) {
            text = &text[token.len()..];
            let tag = tag.to_ascii_lowercase();
            if closing
                && let Some(index) = open
                    .iter()
                    .rposition(|node| node.name.local.eq_ignore_ascii_case(&tag))
            {
                while open.len() > index {
                    finish_label_tag(&mut open, &mut root);
                }
                continue;
            }
            let content = open
                .last_mut()
                .map_or(&mut root, |element| &mut element.children);
            let known = matches!(
                tag.as_str(),
                "b" | "strong"
                    | "em"
                    | "i"
                    | "u"
                    | "s"
                    | "del"
                    | "strike"
                    | "sup"
                    | "sub"
                    | "code"
                    | "br"
                    | "a"
                    | "img"
                    | "span"
                    | "p"
                    | "div"
                    | "body"
                    | "blockquote"
                    | "ul"
                    | "ol"
                    | "li"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "hr"
                    | "html"
                    | "head"
            );
            if !closing && known {
                let element = XmlElement {
                    name: QName {
                        ns: None,
                        prefix: None,
                        local: tag.clone(),
                    },
                    attrs: Vec::new(),
                    ns_decls: Vec::new(),
                    children: Vec::new(),
                };
                let start = token.trim_end_matches('>').trim_end_matches('/');
                let element = xml::parse_fragment(&format!("{start}/>"), limits, options)
                    .ok()
                    .and_then(|fragment| fragment.nodes.into_iter().next())
                    .and_then(|node| match node {
                        XmlNode::Element(node) => Some(node),
                        XmlNode::Text(_) => None,
                    })
                    .unwrap_or(element);
                if token.ends_with("/>") || matches!(tag.as_str(), "br" | "img" | "hr") {
                    content.push(XmlNode::Element(element));
                } else {
                    check_depth(open.len() + 1, limits.max_depth)?;
                    open.push(element);
                }
            } else if !matches!(tag.as_str(), "html" | "head" | "body" | "div" | "p") {
                content.push(XmlNode::Element(XmlElement {
                    name: QName {
                        ns: None,
                        prefix: None,
                        local: String::new(),
                    },
                    attrs: Vec::new(),
                    ns_decls: Vec::new(),
                    children: vec![XmlNode::Text(token.into())],
                }));
            }
        } else if let Some(comment) = text.strip_prefix("<!--") {
            text = comment.split_once("-->").map_or("", |(_, rest)| rest);
        } else {
            let end = text
                .char_indices()
                .skip(1)
                .find_map(|(index, ch)| (ch == '<').then_some(index))
                .unwrap_or(text.len());
            let (plain, rest) = text.split_at(end);
            text = rest;
            let content = open
                .last_mut()
                .map_or(&mut root, |element| &mut element.children);
            let escaped = plain.replace('<', "&lt;");
            let decoded = xml::parse_fragment(&escaped, limits, options).map_or_else(
                |_| plain.to_owned(),
                |fragment| {
                    fragment
                        .nodes
                        .into_iter()
                        .filter_map(|node| match node {
                            XmlNode::Text(text) => Some(text),
                            XmlNode::Element(_) => None,
                        })
                        .collect()
                },
            );
            if let Some(XmlNode::Text(previous)) = content.last_mut() {
                previous.push_str(&decoded);
            } else {
                content.push(XmlNode::Text(decoded));
            }
        }
    }
    while !open.is_empty() {
        finish_label_tag(&mut open, &mut root);
    }
    Ok(root)
}

fn label_tag(text: &str) -> Option<(&str, bool, &str)> {
    let body = text.strip_prefix('<')?;
    let (body, closing) = body
        .strip_prefix('/')
        .map_or((body, false), |body| (body, true));
    if !body.starts_with(|ch: char| ch.is_ascii_alphabetic()) {
        return None;
    }
    let name_end = body.find(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '-' | ':'))?;
    let mut quote = None;
    for (index, ch) in body[name_end..].char_indices() {
        match ch {
            ch if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            '<' if quote.is_none() => return None,
            '>' if quote.is_none() => {
                let length = 2 + usize::from(closing) + name_end + index;
                return Some((&body[..name_end], closing, &text[..length]));
            }
            _ => {}
        }
    }
    None
}

fn finish_label_tag(open: &mut Vec<XmlElement>, root: &mut Vec<XmlNode>) {
    if let Some(node) = open.pop() {
        open.last_mut()
            .map_or(root, |parent| &mut parent.children)
            .push(XmlNode::Element(node));
    }
}

fn smart_label(text: &str) -> String {
    text.replace("---", "—")
        .replace("--", "–")
        .replace("...", "…")
}

fn label_text(text: &str, smart: bool, limit: usize) -> Result<Vec<Inline>> {
    if !smart {
        return Ok(metadata_text(text));
    }
    let adjusted = smart_label(text);
    let mut options = ReaderOptions::default();
    options.extensions.enable(randoc_core::Extension::Smart);
    let mut xs = inline::Parser {
        format: crate::Format::Opml,
        options: &options,
        limit,
        notes: Default::default(),
        links: Default::default(),
        active: Default::default(),
        budget: 0,
        decode_entities: false,
        t2t_macros: None,
    }
    .parse(&adjusted, 0)?;
    if text.starts_with(inline::markup_space) {
        xs.insert(0, Inline::Space);
    }
    if text.ends_with(inline::markup_space) && !matches!(xs.last(), Some(Inline::Space)) {
        xs.push(Inline::Space);
    }
    Ok(xs)
}

fn push_label(out: &mut Vec<Inline>, inline: Inline) {
    match (&inline, out.last_mut()) {
        (Inline::Str(text), Some(Inline::Str(previous))) => previous.push_str(text),
        (Inline::Space, Some(Inline::Space | Inline::LineBreak)) => {}
        (Inline::Space, Some(Inline::Str(text))) if text.ends_with('\u{a0}') => {}
        _ => out.push(inline),
    }
}

fn label_break(out: &mut Vec<Inline>) {
    if matches!(out.last(), Some(Inline::Space)) {
        out.pop();
    }
    if !matches!(out.last(), Some(Inline::LineBreak)) {
        out.push(Inline::LineBreak);
    }
}

fn label_nodes(nodes: &[XmlNode], depth: usize, limit: usize, smart: bool) -> Result<Vec<Inline>> {
    check_depth(depth, limit)?;
    let mut out = Vec::new();
    for node in nodes {
        match node {
            XmlNode::Text(text) => {
                for x in label_text(text, smart, limit.saturating_sub(depth))? {
                    push_label(&mut out, x);
                }
            }
            XmlNode::Element(element) => {
                let tag = element.name.local.to_ascii_lowercase();
                if tag.is_empty() {
                    out.push(Inline::RawInline(
                        randoc_ast::Format::new("html"),
                        element.all_text(),
                    ));
                    continue;
                }
                let mut xs = if matches!(tag.as_str(), "ul" | "ol") {
                    let mut xs = Vec::new();
                    for node in &element.children {
                        if let XmlNode::Element(item) = node
                            && item.name.local.eq_ignore_ascii_case("li")
                        {
                            for inline in label_nodes(&item.children, depth + 2, limit, smart)? {
                                push_label(&mut xs, inline);
                            }
                        }
                    }
                    xs
                } else {
                    label_nodes(&element.children, depth + 1, limit, smart)?
                };
                match tag.as_str() {
                    "b" | "strong" => out.push(Inline::Strong(xs)),
                    "em" | "i" => out.push(Inline::Emph(xs)),
                    "u" => out.push(Inline::Underline(xs)),
                    "s" | "del" | "strike" => out.push(Inline::Strikeout(xs)),
                    "sup" => out.push(Inline::Superscript(xs)),
                    "sub" => out.push(Inline::Subscript(xs)),
                    "code" => out.push(Inline::Code(Attr::default(), element.all_text())),
                    "br" => out.push(Inline::LineBreak),
                    "a" if element.attr_local("href").is_some() => out.push(Inline::Link(
                        label_attributes(element, &["href", "title"]),
                        xs,
                        Target::new(
                            element.attr_local("href").unwrap_or(""),
                            element.attr_local("title").unwrap_or(""),
                        ),
                    )),
                    "img" => out.push(Inline::Image(
                        label_attributes(element, &["src", "title", "alt"]),
                        inline::words(element.attr_local("alt").unwrap_or("")),
                        Target::new(
                            element.attr_local("src").unwrap_or(""),
                            element.attr_local("title").unwrap_or(""),
                        ),
                    )),
                    "span" | "a" => {
                        out.push(Inline::Span(label_attributes(element, &[]), xs));
                    }
                    "p" | "div" | "body" | "blockquote" | "ul" | "ol" | "h1" | "h2" | "h3"
                    | "h4" | "h5" | "h6" => {
                        label_break(&mut out);
                        for inline in xs {
                            push_label(&mut out, inline);
                        }
                        label_break(&mut out);
                    }
                    "hr" => {
                        label_break(&mut out);
                        out.push(Inline::LineBreak);
                    }
                    "script" | "style" => {
                        let (start, end) = label_tags(element);
                        let content = element.all_text();
                        let end = end.unwrap_or_default();
                        out.push(Inline::RawInline(
                            randoc_ast::Format::new("html"),
                            format!("{start}{content}{end}"),
                        ));
                    }
                    "html" | "head" => out.append(&mut xs),
                    _ => {
                        if !tag.is_empty() {
                            let (opening, closing) = label_tags(element);
                            out.push(Inline::RawInline(randoc_ast::Format::new("html"), opening));
                            if matches!(tag.as_str(), "title" | "li") {
                                label_break(&mut out);
                            }
                            out.append(&mut xs);
                            if matches!(tag.as_str(), "title" | "li") {
                                label_break(&mut out);
                            }
                            if let Some(closing) = closing {
                                out.push(Inline::RawInline(
                                    randoc_ast::Format::new("html"),
                                    closing,
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(out)
}

pub(crate) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let budget = write::document_budget(doc, context)?;
    let mut output = String::new();
    let mut stack = Vec::new();
    let mut blocks = doc.blocks.iter().collect::<Vec<_>>();
    let mut heading_divs = std::collections::BTreeSet::new();
    let mut scan = vec![(None, &doc.blocks[..])];
    let mut parents = Vec::new();
    while let Some((parent, children)) = scan.pop() {
        for block in children {
            match block {
                Block::Div(_, contents) => {
                    let id = parents.len();
                    parents.push((parent, std::ptr::from_ref(block)));
                    scan.push((Some(id), contents));
                }
                Block::Header(..) => {
                    let mut current = parent;
                    while let Some(index) = current {
                        let (parent, ptr) = parents[index];
                        if !heading_divs.insert(ptr) {
                            break;
                        }
                        current = parent;
                    }
                }
                _ => {}
            }
        }
    }
    blocks.reverse();
    let mut heading = None;
    let mut note = Vec::new();
    while let Some(block) = blocks.pop() {
        match block {
            Block::Div(_, children) if heading_divs.contains(&std::ptr::from_ref(block)) => {
                blocks.extend(children.iter().rev())
            }
            Block::Header(level, _, xs) => {
                if let Some(label) = heading.take() {
                    open_outline(
                        &mut output,
                        stack.len() - 1,
                        label,
                        std::mem::take(&mut note),
                        options,
                        context,
                    )?;
                }
                while stack.last().is_some_and(|previous| previous >= level) {
                    stack.pop();
                    close_outline(&mut output, stack.len());
                }
                check_depth(stack.len() + 1, context.limits.max_nesting_depth)?;
                stack.push(*level);
                heading = Some(xs);
            }
            _ if heading.is_some() => note.push(block),
            _ => {}
        }
        if output.len() > budget {
            return Err(Error::App("The outline output is too large.".into()));
        }
    }
    if let Some(label) = heading {
        open_outline(&mut output, stack.len() - 1, label, note, options, context)?;
    }
    while stack.pop().is_some() {
        close_outline(&mut output, stack.len());
    }
    if output.len() > budget {
        return Err(Error::App("The outline output is too large.".into()));
    }
    output.pop();
    let Some(template) = &options.template else {
        return Ok(output);
    };
    let mut vars = meta_context::meta_to_context(
        &doc.meta,
        &options.variables,
        context.limits.max_nesting_depth,
        &mut |blocks| Ok(literal(randoc_core::shared::stringify_blocks(blocks))),
        &mut |xs| Ok(literal(randoc_core::shared::stringify(xs))),
    )?;
    let date = doc
        .meta
        .get("date")
        .and_then(|date| meta_context::normalize_date(&stringify_meta(date)))
        .and_then(|date| opml_date(&date))
        .unwrap_or_default();
    vars.insert("date", Val::text(date));
    vars.insert("body", Val::text(output));
    template
        .render_limited(&vars, budget, budget)
        .ok_or_else(|| Error::App("The outline output is too large.".into()))
}

fn open_outline(
    output: &mut String,
    depth: usize,
    label: &[Inline],
    note: Vec<&Block>,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<()> {
    let mut fragment = options.clone();
    fragment.template = None;
    fragment.wrap_text = randoc_core::WrapOption::WrapNone;
    let label = randoc_html::render_blocks(&[Block::Plain(label.to_vec())], &fragment, context)?;
    indent(output, depth);
    output.push_str("<outline text=\"");
    xml::escape_xml_attr(&label, output);
    output.push('"');
    if !note.is_empty() {
        let Some(markdown) = randoc_markdown_writer::writer("markdown") else {
            return Err(Error::UnknownWriter("markdown".into()));
        };
        fragment.wrap_text = options.wrap_text;
        fragment.extensions = randoc_core::default_extensions("markdown");
        fragment.table_of_contents = false;
        let doc = Pandoc {
            meta: Meta::default(),
            blocks: note.into_iter().cloned().collect(),
        };
        let note = String::from_utf8(markdown.write(&doc, &fragment, context)?.into_bytes())
            .map_err(|_| Error::App("The note is not valid UTF-8.".into()))?;
        if !note.trim().is_empty() {
            output.push_str(" _note=\"");
            let mut escaped = String::new();
            xml::escape_xml_attr(note.trim_matches('\n'), &mut escaped);
            output.push_str(&escaped.replace('\n', "&#10;"));
            output.push('"');
        }
    }
    output.push_str(">\n");
    Ok(())
}

fn indent(output: &mut String, depth: usize) {
    output.extend(std::iter::repeat_n(' ', depth.saturating_mul(2)));
}

fn close_outline(output: &mut String, depth: usize) {
    indent(output, depth);
    output.push_str("</outline>\n");
}

fn opml_date(date: &str) -> Option<String> {
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: usize = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let offset = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let index = month.checked_sub(1)?;
    let adjusted = year - i64::from(month < 3);
    let weekday =
        (adjusted + adjusted / 4 - adjusted / 100 + adjusted / 400 + offset.get(index)? + day)
            .rem_euclid(7);
    let weekdays = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    Some(format!(
        "{}, {day:02} {} {year:04} 00:00:00 UTC",
        weekdays.get(weekday as usize)?,
        months.get(index)?
    ))
}

fn label_tags(element: &XmlElement) -> (String, Option<String>) {
    let mut opening = format!("<{}", element.name.local);
    for (name, value) in &element.attrs {
        opening.push(' ');
        opening.push_str(&name.local);
        opening.push_str("=\"");
        xml::escape_xml_attr(value, &mut opening);
        opening.push('"');
    }
    opening.push('>');
    let closing = (!matches!(
        element.name.local.as_str(),
        "meta" | "link" | "hr" | "input"
    ))
    .then(|| format!("</{}>", element.name.local));
    (opening, closing)
}

fn label_attributes(element: &XmlElement, consumed: &[&str]) -> Attr {
    let mut attr = Attr {
        id: element
            .attr_local("id")
            .or_else(|| element.attr_local("name"))
            .unwrap_or("")
            .into(),
        ..Attr::default()
    };
    for (name, value) in &element.attrs {
        match name.local.as_str() {
            "id" | "name" => {}
            "class" => attr
                .classes
                .extend(value.split_whitespace().map(str::to_owned)),
            key if !consumed.contains(&key) => attr.attributes.push((
                key.strip_prefix("data-").unwrap_or(key).into(),
                value.clone(),
            )),
            _ => {}
        }
    }
    attr
}
