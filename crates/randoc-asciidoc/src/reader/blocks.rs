use std::path::Path;

use randoc_ast::{Attr, Block, Caption, Inline, ListAttributes, ListNumberStyle, MathType};
use randoc_core::{Error, Result, shared};

use super::{State, attrs, include_key};

#[derive(Clone, Copy)]
pub(super) enum Sections<'a> {
    Top,
    Include(&'a str),
    Cell,
    Restricted,
}

impl State<'_> {
    pub(super) fn blocks(
        &mut self,
        lines: &[&str],
        path: &Path,
        depth: usize,
        sections: Sections<'_>,
    ) -> Result<Vec<Block>> {
        self.depth(depth)?;
        let mut output = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            if lines[i].trim().is_empty() || lines[i].starts_with("//") && !is_fence(lines[i], '/')
            {
                i += 1;
                continue;
            }
            if self.attribute_entry(lines, &mut i) {
                continue;
            }
            let attributes = pending(lines, &mut i);
            if i >= lines.len() {
                break;
            }
            let start = i;
            self.block(
                lines,
                &mut i,
                path,
                (depth, sections),
                attributes,
                &mut output,
            )?;
            if i == start {
                i += 1;
            }
        }
        Ok(output)
    }

    fn block(
        &mut self,
        lines: &[&str],
        i: &mut usize,
        path: &Path,
        scope: (usize, Sections<'_>),
        attributes: attrs::Attributes<'_>,
        output: &mut Vec<Block>,
    ) -> Result<()> {
        let text = lines[*i].trim_end();
        if let Some((target, "")) = attrs::macro_args(text, "include::") {
            *i += 1;
            let Some((path, content)) = self.include(target, path)? else {
                return Ok(());
            };
            let depth = scope.0 + 1;
            self.depth(depth)?;
            let key = include_key(&path);
            self.includes.insert(key.clone());
            let result = self.blocks(
                &content.lines().collect::<Vec<_>>(),
                &path,
                depth,
                Sections::Include(&content),
            );
            self.includes.remove(&key);
            output.push(Block::Div(
                Attr::new(
                    "",
                    vec!["included".into()],
                    vec![("path".into(), path.to_string_lossy().into_owned())],
                ),
                result?,
            ));
            return Ok(());
        }
        if let Some(fence) = fence(text) {
            let (_, content) = take_fence(lines, i, text);
            output.extend(self.fenced_block(content, path, scope.0, attributes, fence, text)?);
            return Ok(());
        }
        self.regular_block(lines, i, path, scope, attributes, output)
    }

    fn fenced_block(
        &mut self,
        content: &[&str],
        path: &Path,
        depth: usize,
        attributes: attrs::Attributes<'_>,
        fence: Fence,
        marker: &str,
    ) -> Result<Vec<Block>> {
        let title = attributes.title;
        let quote_attr = (fence == Fence::Quote).then(|| attributes.attr.clone());
        let mut blocks = match fence {
            Fence::Comment => Vec::new(),
            Fence::Literal | Fence::Listing => {
                let content = self.listing_text(content, path, depth)?;
                vec![self.code(&attributes, content, fence == Fence::Listing)]
            }
            Fence::Table => vec![self.table(content, marker, &attributes, path, depth + 1)?],
            Fence::Pass => {
                let mut content = content.join("\n");
                if !content.is_empty() {
                    content.push('\n');
                }
                if matches!(attributes.style(), "latexmath" | "asciimath" | "stem") {
                    vec![Block::Para(vec![Inline::Math(
                        MathType::DisplayMath,
                        content,
                    )])]
                } else {
                    self.html(&content, depth)?
                }
            }
            Fence::Quote => {
                let style = attributes.style();
                let mut body = if style == "verse" {
                    vec![Block::Para(self.paragraph(
                        &content.join("\n"),
                        depth + 1,
                        true,
                    )?)]
                } else {
                    self.blocks(content, path, depth + 1, Sections::Restricted)?
                };
                self.attribution(&attributes, &mut body, depth)?;
                vec![Block::BlockQuote(body)]
            }
            Fence::Example | Fence::Sidebar | Fence::Open => {
                let body = self.blocks(content, path, depth + 1, Sections::Restricted)?;
                let default = match fence {
                    Fence::Example => "example",
                    Fence::Sidebar => "sidebar",
                    _ => "",
                };
                let style = attributes.style().to_owned();
                let style = if style.is_empty() { default } else { &style };
                vec![self.styled(attributes, style, body, depth)?]
            }
        };
        if matches!(fence, Fence::Literal | Fence::Listing | Fence::Quote)
            && let Some(title) = title
        {
            blocks = vec![Block::Div(
                quote_attr.clone().unwrap_or_default(),
                self.add_title(Some(title), blocks, depth)?,
            )];
        }
        if fence == Fence::Quote && title.is_none() {
            blocks = self.with_title(attributes_for_wrapper(quote_attr), blocks, depth)?;
        }
        Ok(blocks)
    }

    fn regular_block(
        &mut self,
        lines: &[&str],
        i: &mut usize,
        path: &Path,
        (depth, sections): (usize, Sections<'_>),
        mut attributes: attrs::Attributes<'_>,
        output: &mut Vec<Block>,
    ) -> Result<()> {
        let line = lines[*i];
        let text = line.trim_end();
        let sections_allowed = !matches!(sections, Sections::Restricted);
        if sections_allowed && let Some((level, heading)) = heading(text) {
            if level == 0 {
                let (offset, remaining) = if let Sections::Include(content) = sections {
                    let start: usize = content.split_inclusive('\n').take(*i).map(str::len).sum();
                    (
                        content[..start].chars().count(),
                        content[start..].to_owned(),
                    )
                } else {
                    let offset = self.header_chars
                        + lines[..*i]
                            .iter()
                            .map(|line| line.chars().count() + 1)
                            .sum::<usize>();
                    (offset, format!("{}\n\n", lines[*i..].join("\n")))
                };
                return Err(Error::Parse(format!(
                    "Unexpected {} at {} char {offset}",
                    shared::show_string(&remaining),
                    shared::show_string(self.source_name),
                )));
            }
            let title = self.inlines(heading, depth)?;
            let discrete = attributes.style() == "discrete";
            let level = if discrete {
                level
            } else {
                while self
                    .sections
                    .last()
                    .is_some_and(|previous| *previous >= level)
                {
                    self.sections.pop();
                }
                self.sections.push(level);
                self.sections.len()
            };
            if let Some(anchor) = attributes.anchor {
                attributes.attr.id = anchor.split(',').next().unwrap_or(anchor).into();
            }
            let prefix = self.attributes.get("idprefix").map_or("_", String::as_str);
            let separator = self
                .attributes
                .get("idseparator")
                .map_or("_", String::as_str);
            let generated = attributes.attr.id.is_empty() && !discrete;
            let base = if generated {
                let title = shared::stringify(&title).to_lowercase();
                format!(
                    "{prefix}{}",
                    title
                        .split(|ch: char| ch.is_whitespace() || matches!(ch, '.' | '-'))
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join(separator)
                )
            } else {
                attributes.attr.id.clone()
            };
            // Record this heading without changing headings from HTML passthrough.
            attributes.attr.id = format!("\0{}", self.headings.len());
            self.headings
                .push((level, base, generated, separator.into()));
            output.push(Block::Header(level as i64, attributes.finish(), title));
            *i += 1;
            return Ok(());
        }
        if text == "toc::[]" {
            output.push(Block::Div(Attr::new("toc", vec![], vec![]), vec![]));
            *i += 1;
            return Ok(());
        }
        if let Some((target, value)) =
            attrs::macro_args(text, "video::").or_else(|| attrs::macro_args(text, "audio::"))
        {
            let image = self.image(
                target,
                if value.is_empty() { target } else { value },
                depth + 1,
            )?;
            output.push(Block::Plain(vec![image]));
            *i += 1;
            return Ok(());
        }
        if let Some((target, value)) = attrs::macro_args(text, "image::") {
            let (position, named): (Vec<_>, Vec<_>) =
                attrs::fields(value).partition(|field| !field.contains('='));
            let image = self.image(target, &position.join(","), depth + 1)?;
            let caption = match attributes.title {
                Some(title) => caption(self.inlines(title, depth + 1)?),
                None => Caption::default(),
            };
            let figure = Block::Figure(Attr::default(), caption, vec![Block::Plain(vec![image])]);
            for field in named {
                attributes.parse(field);
            }
            attributes.attr.attributes.sort();
            attributes.title = None;
            output.extend(self.with_title(attributes, vec![figure], depth)?);
            *i += 1;
            return Ok(());
        }
        if let Some((marker, language)) = code_fence(text)
            && !matches!(attributes.style(), "source" | "literal" | "listing")
        {
            let (_, lines) = take_fence(lines, i, marker);
            let mut attr = Attr::default();
            if !language.is_empty() {
                attr.classes.push(language.into());
            }
            output.push(Block::CodeBlock(attr, lines.join("\n")));
            return Ok(());
        }
        if text == "<<<" {
            attributes.attr.classes.push("page-break".into());
            if !attributes.options.is_empty() {
                attributes.attr.attributes.push((
                    "options".into(),
                    std::mem::take(&mut attributes.options).join(","),
                ));
            }
            attributes
                .attr
                .attributes
                .push(("wrapper".into(), "1".into()));
            output.push(Block::Div(attributes.finish(), vec![Block::HorizontalRule]));
            *i += 1;
            return Ok(());
        }
        if horizontal(text) {
            output.push(Block::HorizontalRule);
            *i += 1;
            return Ok(());
        }
        if let Some(marker) = marker(text) {
            let block = self.list(
                lines,
                i,
                path,
                &attributes,
                marker,
                depth + 1,
                ListScope::default(),
            )?;
            let style = attributes.style().to_owned();
            attributes.attr.attributes.retain(|(key, _)| key != "start");
            if !style.is_empty() && !ordered_style(&style) && attributes.title.is_some() {
                output.push(self.styled(attributes, &style, vec![block], depth)?);
            } else if attributes.title.is_some() {
                output.push(self.styled(attributes, "", vec![block], depth)?);
            } else {
                if let Some(anchor) = attributes.anchor.take() {
                    attributes.attr.id = anchor.split(',').next().unwrap_or(anchor).into();
                }
                output.extend(self.with_title(attributes, vec![block], depth)?);
            }
            return Ok(());
        }
        if let Some((_, count, _)) = definition(text) {
            let mut block = self.definitions(lines, i, path, count, depth + 1)?;
            if attributes.style() == "ordered"
                && let Block::DefinitionList(items) = block
            {
                let items = items
                    .into_iter()
                    .map(|item| vec![Block::DefinitionList(vec![item])])
                    .collect();
                block = Block::OrderedList(ListAttributes::default(), items);
            }
            attributes.attr.attributes.sort();
            output.extend(self.with_title(attributes, vec![block], depth)?);
            return Ok(());
        }
        if text.starts_with([' ', '\t']) && !text.trim().is_empty()
            || matches!(attributes.style(), "source" | "literal" | "listing")
        {
            let indented = text.starts_with([' ', '\t']);
            let start = *i;
            *i += 1;
            while let Some(next) = lines.get(*i).filter(|next| !next.trim().is_empty()) {
                if indented && !next.starts_with([' ', '\t']) {
                    break;
                }
                *i += 1;
            }
            let slice = &lines[start..*i];
            let content = if indented {
                let indent = slice
                    .iter()
                    .map(|line| line.len() - line.trim_start_matches([' ', '\t']).len())
                    .min()
                    .unwrap_or(0);
                slice
                    .iter()
                    .map(|line| &line[indent..])
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                self.listing_text(slice, path, depth)?
            };
            let block = self.code(
                &attributes,
                content,
                matches!(attributes.style(), "source" | "listing"),
            );
            output.extend(self.with_title(
                attrs::Attributes {
                    title: attributes.title,
                    ..attrs::Attributes::default()
                },
                vec![block],
                depth,
            )?);
            return Ok(());
        }
        let start = *i;
        *i += 1;
        let conditional = ["ifdef::", "ifndef::", "ifeval::"]
            .iter()
            .any(|prefix| line.starts_with(prefix));
        while lines
            .get(*i)
            .is_some_and(|line| !interrupt(line, sections_allowed && !conditional))
        {
            *i += 1;
        }
        let value = lines[start..*i]
            .iter()
            .filter(|line| !is_comment(line))
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        let (style, value) = value
            .split_once(": ")
            .filter(|(style, _)| admonition(style).is_some())
            .unwrap_or(("", &value));
        let mut inline = self.paragraph(
            value,
            depth,
            attributes.option("hardbreaks")
                || self.attributes.contains_key("hardbreaks")
                || attributes.style() == "verse",
        )?;
        if let Some(anchor) = attributes.anchor {
            let (id, label) = anchor.split_once(',').unwrap_or((anchor, anchor));
            let label = self.inlines(label, depth + 1)?;
            self.define_label(id, label);
            inline.insert(0, Inline::Span(Attr::new(id, vec![], vec![]), vec![]));
        }
        let block = Block::Para(inline);
        let style = if style.is_empty() {
            attributes.style().to_owned()
        } else {
            style.into()
        };
        if matches!(style.as_str(), "quote" | "verse") {
            let mut body = vec![block];
            self.attribution(&attributes, &mut body, depth)?;
            output.extend(self.with_title(attributes, vec![Block::BlockQuote(body)], depth)?);
        } else if admonition(&style).is_some() {
            output.push(self.styled(attributes, &style, vec![block], depth)?);
        } else if matches!(style.as_str(), "example" | "sidebar") {
            if attributes.title.is_some()
                || !attributes.attr.is_empty()
                || !attributes.options.is_empty()
            {
                output.push(self.styled(attributes, "", vec![block], depth)?);
            } else {
                output.push(block);
            }
        } else if attributes.title.is_some() && attributes.option("collapsible") {
            output.push(self.styled(attributes, "", vec![block], depth)?);
        } else {
            output.extend(self.with_title(attributes, vec![block], depth)?);
        }
        Ok(())
    }

    fn attribution(
        &mut self,
        attrs: &attrs::Attributes<'_>,
        body: &mut Vec<Block>,
        depth: usize,
    ) -> Result<()> {
        let attribution = attrs
            .position
            .iter()
            .skip(1)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        if !attribution.is_empty() {
            body.push(Block::Para(
                self.inlines(&format!("— {attribution}"), depth + 1)?,
            ));
        }
        Ok(())
    }

    fn with_title(
        &mut self,
        attributes: attrs::Attributes<'_>,
        blocks: Vec<Block>,
        depth: usize,
    ) -> Result<Vec<Block>> {
        let title = attributes.title;
        let mut attr = attributes.finish();
        if attr.is_empty() && title.is_none() {
            return Ok(blocks);
        }
        attr.attributes.insert(0, ("wrapper".into(), "1".into()));
        Ok(vec![Block::Div(
            attr,
            self.add_title(title, blocks, depth)?,
        )])
    }

    fn add_title(
        &mut self,
        title: Option<&str>,
        mut blocks: Vec<Block>,
        depth: usize,
    ) -> Result<Vec<Block>> {
        if let Some(title) = title {
            blocks.insert(
                0,
                Block::Div(
                    Attr::new("", vec!["title".into()], vec![]),
                    vec![Block::Para(self.inlines(title, depth + 1)?)],
                ),
            );
        }
        Ok(blocks)
    }

    fn styled(
        &mut self,
        attributes: attrs::Attributes<'_>,
        style: &str,
        blocks: Vec<Block>,
        depth: usize,
    ) -> Result<Block> {
        let title = attributes.title.or_else(|| admonition(style));
        let mut attr = attributes.finish();
        if !style.is_empty() {
            attr.classes.insert(0, style.to_ascii_lowercase());
        }
        Ok(Block::Div(attr, self.add_title(title, blocks, depth)?))
    }

    fn listing_text(&mut self, lines: &[&str], parent: &Path, depth: usize) -> Result<String> {
        self.depth(depth)?;
        let mut output = String::new();
        let mut first = true;
        for line in lines {
            let included;
            let text = if let Some((target, _)) = attrs::macro_args(line.trim(), "include::") {
                let Some((path, value)) = self.include(target, parent)? else {
                    continue;
                };
                self.depth(depth + 1)?;
                let key = include_key(&path);
                self.includes.insert(key.clone());
                let result =
                    self.listing_text(&value.lines().collect::<Vec<_>>(), &path, depth + 1);
                self.includes.remove(&key);
                included = result?;
                &included
            } else {
                *line
            };
            if !first {
                output.push('\n');
            }
            first = false;
            output.push_str(text);
        }
        Ok(output)
    }

    fn code(
        &self,
        attributes: &attrs::Attributes<'_>,
        mut content: String,
        listing: bool,
    ) -> Block {
        let mut attr = attributes.attr.clone();
        if let Some(style) = attributes.position.first().filter(|value| {
            !value.is_empty() && !matches!(value.as_str(), "source" | "listing" | "literal")
        }) {
            attr.classes.insert(0, style.clone());
        }
        if listing {
            if attributes.option("linenums") && attributes.style() != "source" {
                attr.classes
                    .retain(|class| !attributes.position.contains(class));
            } else if let Some(language) = attributes
                .position
                .get(1)
                .filter(|language| !language.is_empty())
            {
                attr.classes.insert(0, language.clone());
            }
            if !attributes.options.is_empty() {
                attr.attributes
                    .push(("options".into(), attributes.options.join(",")));
            }
            attr.attributes.retain(|(key, _)| key != "start");
            content = callouts(&content);
        } else {
            content.push('\n');
        }
        Block::CodeBlock(attr, content)
    }

    #[allow(clippy::too_many_arguments)]
    fn list(
        &mut self,
        lines: &[&str],
        i: &mut usize,
        path: &Path,
        attributes: &attrs::Attributes<'_>,
        first: Marker<'_>,
        depth: usize,
        scope: ListScope<'_>,
    ) -> Result<Block> {
        self.depth(depth)?;
        let mut items = Vec::new();
        while let Some(current) = lines
            .get(*i)
            .and_then(|line| marker(line))
            .filter(|current| current.kind == first.kind && current.level == first.level)
        {
            let mut text = current.text.to_owned();
            *i += 1;
            while let Some(line) = lines
                .get(*i)
                .filter(|line| !interrupt(line, false) && line.trim() != "+")
            {
                if !is_comment(line.trim_start()) {
                    text.push('\n');
                    text.push_str(line.trim());
                }
                *i += 1;
            }
            for (task, mark) in [("[x] ", "☒ "), ("[*] ", "☒ "), ("[ ] ", "☐ ")] {
                if let Some(rest) = text.strip_prefix(task) {
                    text = format!("{mark}{rest}");
                    break;
                }
            }
            let mut item = vec![Block::Para(self.inlines(&text, depth)?)];
            loop {
                let mut next = *i;
                while lines.get(next).is_some_and(|line| line.trim().is_empty()) {
                    next += 1;
                }
                if lines.get(next).is_some_and(|line| line.trim() == "+") {
                    next += 1;
                    if self.attribute_entry(lines, &mut next) {
                        *i = next;
                        continue;
                    }
                    let attr = pending(lines, &mut next);
                    if next >= lines.len() {
                        *i = next;
                        break;
                    }
                    self.block(
                        lines,
                        &mut next,
                        path,
                        (depth, Sections::Restricted),
                        attr,
                        &mut item,
                    )?;
                    *i = next;
                    continue;
                }
                if next == *i
                    && let Some((term, count, "")) =
                        lines.get(next).and_then(|line| definition(line))
                {
                    if scope.definition == Some(count) && first.kind != ListKind::Bullet {
                        break;
                    }
                    if lines
                        .get(next + 1)
                        .and_then(|line| marker(line))
                        .is_some_and(|child| {
                            child.kind == first.kind
                                && child.level == first.level
                                && child.indent <= first.indent
                        })
                    {
                        let term = self.inlines(term, depth)?;
                        item.push(Block::DefinitionList(vec![(term, vec![vec![]])]));
                        *i += 1;
                    } else if scope.definition == Some(count) {
                        break;
                    } else if let Some((child, child_count, "")) =
                        lines.get(next + 1).and_then(|line| definition(line))
                        && child_count > count
                        && lines
                            .get(next + 2)
                            .and_then(|line| marker(line))
                            .is_some_and(|child| {
                                child.kind == first.kind && child.level == first.level
                            })
                    {
                        let name = self.inlines(term, depth)?;
                        let label = self.inlines(child, depth + 1)?;
                        let nested = Block::DefinitionList(vec![(label, vec![vec![]])]);
                        item.push(Block::DefinitionList(vec![(name, vec![vec![nested]])]));
                        *i += 2;
                    } else {
                        item.push(self.definitions(lines, i, path, count, depth + 1)?);
                    }
                    continue;
                }
                let mut nested = next;
                let attr = pending(lines, &mut nested);
                if let Some(child) = lines.get(nested).and_then(|line| marker(line)) {
                    if scope.parent.is_some_and(|parent| {
                        child.kind == parent.kind
                            && child.level == parent.level
                            && child.indent <= parent.indent
                    }) {
                        break;
                    }
                    if child.level > first.level
                        || child.kind != first.kind
                            && child.level >= first.level
                            && (child.indent > first.indent
                                || next == *i && child.indent == first.indent)
                    {
                        item.push(self.list(
                            lines,
                            &mut nested,
                            path,
                            &attr,
                            child,
                            depth + 1,
                            ListScope {
                                parent: Some(first),
                                ..scope
                            },
                        )?);
                        *i = nested;
                        continue;
                    }
                    if child.kind == first.kind
                        && child.level == first.level
                        && nested == next
                        && next == *i
                    {
                        *i = next;
                    }
                }
                break;
            }
            items.push(item);
        }
        let style = match attributes.style() {
            "arabic" => ListNumberStyle::Decimal,
            "loweralpha" => ListNumberStyle::LowerAlpha,
            "upperalpha" => ListNumberStyle::UpperAlpha,
            "lowerroman" => ListNumberStyle::LowerRoman,
            "upperroman" => ListNumberStyle::UpperRoman,
            _ => ListNumberStyle::DefaultStyle,
        };
        let options = ListAttributes {
            start: attributes
                .attr
                .get("start")
                .and_then(|value| value.parse().ok())
                .unwrap_or(first.start),
            style,
            ..ListAttributes::default()
        };
        Ok(match first.kind {
            ListKind::Bullet => Block::BulletList(items),
            ListKind::Ordered => Block::OrderedList(options, items),
            ListKind::Callout => Block::Div(
                Attr::new("", vec!["callout-list".into()], vec![]),
                vec![Block::OrderedList(options, items)],
            ),
        })
    }

    fn definitions(
        &mut self,
        lines: &[&str],
        i: &mut usize,
        path: &Path,
        count: usize,
        depth: usize,
    ) -> Result<Block> {
        self.depth(depth)?;
        let mut entries = Vec::new();
        while let Some((term, current, content)) = lines
            .get(*i)
            .and_then(|line| definition(line))
            .filter(|(_, current, _)| *current == count)
        {
            let _ = current;
            let term = self.inlines(term, depth)?;
            *i += 1;
            let mut body = Vec::new();
            if !content.is_empty() {
                let mut paragraph = content.to_owned();
                while let Some(line) = lines
                    .get(*i)
                    .filter(|line| !interrupt(line, false) && line.trim() != "+")
                {
                    paragraph.push('\n');
                    paragraph.push_str(line.trim());
                    *i += 1;
                }
                body.push(Block::Para(self.inlines(&paragraph, depth)?));
            }
            loop {
                let mut next = *i;
                while lines.get(next).is_some_and(|line| line.trim().is_empty()) {
                    next += 1;
                }
                let Some(line) = lines.get(next) else {
                    break;
                };
                if let Some((_, child, _)) = definition(line) {
                    if child > count {
                        body.push(self.definitions(lines, &mut next, path, child, depth + 1)?);
                        *i = next;
                        continue;
                    }
                    if child == count {
                        *i = next;
                    }
                    break;
                }
                if next == *i
                    && let Some(marker) = marker(line)
                {
                    body.push(self.list(
                        lines,
                        &mut next,
                        path,
                        &attrs::Attributes::default(),
                        marker,
                        depth + 1,
                        ListScope {
                            definition: Some(count),
                            ..ListScope::default()
                        },
                    )?);
                } else if line.trim() == "+" {
                    next += 1;
                    if self.attribute_entry(lines, &mut next) {
                        *i = next;
                        continue;
                    }
                    let attr = pending(lines, &mut next);
                    if next < lines.len() {
                        self.block(
                            lines,
                            &mut next,
                            path,
                            (depth, Sections::Restricted),
                            attr,
                            &mut body,
                        )?;
                    }
                } else if !interrupt(line, false)
                    && (matches!(body.last(), Some(Block::DefinitionList(_)))
                        || body.is_empty() && next == *i)
                {
                    let start = next;
                    next += 1;
                    while lines.get(next).is_some_and(|line| !interrupt(line, false)) {
                        next += 1;
                    }
                    let paragraph = lines[start..next]
                        .iter()
                        .filter(|line| !is_comment(line.trim_start()))
                        .copied()
                        .collect::<Vec<_>>()
                        .join("\n");
                    body.push(Block::Para(self.inlines(&paragraph, depth)?));
                } else {
                    break;
                }
                *i = next;
            }
            entries.push((term, vec![body]));
        }
        Ok(Block::DefinitionList(entries))
    }
}

pub(super) fn caption(inlines: Vec<Inline>) -> Caption {
    Caption {
        short: None,
        long: vec![Block::Plain(inlines)],
    }
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let first = line.as_bytes().first()?;
    if !matches!(first, b'=' | b'#') {
        return None;
    }
    let count = line.bytes().take_while(|byte| byte == first).count();
    let title = line[count..].strip_prefix(' ')?.trim();
    (!title.is_empty()).then_some((count - 1, title))
}

fn pending<'a>(lines: &[&'a str], i: &mut usize) -> attrs::Attributes<'a> {
    let mut attributes = attrs::Attributes::default();
    while let Some(text) = lines.get(*i).map(|line| line.trim_end()) {
        if let Some(value) = text
            .strip_prefix("[[")
            .and_then(|value| value.strip_suffix("]]"))
        {
            attributes.anchor = Some(value);
        } else if let Some(value) = attribute_line(text) {
            attributes.parse(value);
        } else if let Some(title) = title_line(text) {
            attributes.title = Some(title);
        } else {
            break;
        }
        *i += 1;
    }
    attributes
}

fn attribute_line(text: &str) -> Option<&str> {
    let value = text.strip_prefix('[')?.strip_suffix(']')?;
    (!value.starts_with('[') && !value.contains(']')).then_some(value)
}

fn title_line(text: &str) -> Option<&str> {
    let value = text.strip_prefix('.')?;
    value
        .starts_with(|ch: char| !ch.is_whitespace() && ch != '.')
        .then_some(value)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fence {
    Comment,
    Literal,
    Listing,
    Pass,
    Quote,
    Example,
    Sidebar,
    Open,
    Table,
}

fn is_fence(line: &str, delimiter: char) -> bool {
    line.len() >= 4 && line.bytes().all(|ch| ch == delimiter as u8)
}

fn fence(line: &str) -> Option<Fence> {
    let first = line.chars().next()?;
    if line == "--" {
        return Some(Fence::Open);
    }
    if matches!(first, '|' | ':' | ',' | '!')
        && line[1..].starts_with("===")
        && line[1..].bytes().all(|ch| ch == b'=')
    {
        return Some(Fence::Table);
    }
    if !first.is_ascii() || !is_fence(line, first) {
        return None;
    }
    Some(match first {
        '/' => Fence::Comment,
        '.' => Fence::Literal,
        '-' => Fence::Listing,
        '+' => Fence::Pass,
        '_' => Fence::Quote,
        '=' => Fence::Example,
        '*' => Fence::Sidebar,
        _ => return None,
    })
}

fn take_fence<'a, 'b>(lines: &'a [&'b str], i: &mut usize, fence: &str) -> (usize, &'a [&'b str]) {
    let position = *i;
    let start = position + 1;
    let end = lines[start..]
        .iter()
        .position(|line| line.trim_end() == fence)
        .map_or(lines.len(), |end| end + start);
    *i = (end + 1).min(lines.len());
    (position, &lines[start..end])
}

fn horizontal(line: &str) -> bool {
    (line.len() >= 3 && line.bytes().all(|ch| ch == b'\''))
        || matches!(line, "---" | "***" | "- - -" | "* * *" | "_ _ _")
}

fn interrupt(line: &str, sections: bool) -> bool {
    let text = line.trim_end();
    text.trim().is_empty()
        || sections && heading(text).is_some()
        || fence(text).is_some()
        || horizontal(text)
        || code_fence(text).is_some()
        || text == "<<<"
        || attribute_line(text).is_some()
        || text.starts_with("[[") && text.ends_with("]]")
        || title_line(text).is_some()
        || marker(text).is_some()
        || definition(text).is_some()
        || attrs::macro_args(text, "include::").is_some_and(|(_, value)| value.is_empty())
        || ["image::", "video::", "audio::", "toc::"]
            .into_iter()
            .any(|name| attrs::macro_args(text, name).is_some())
}

fn admonition(style: &str) -> Option<&'static str> {
    Some(match style {
        "NOTE" => "Note",
        "TIP" => "Tip",
        "IMPORTANT" => "Important",
        "WARNING" => "Warning",
        "CAUTION" => "Caution",
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListKind {
    Bullet,
    Ordered,
    Callout,
}

#[derive(Clone, Copy)]
struct Marker<'a> {
    kind: ListKind,
    level: usize,
    start: i64,
    text: &'a str,
    indent: usize,
}

#[derive(Clone, Copy, Default)]
struct ListScope<'a> {
    parent: Option<Marker<'a>>,
    definition: Option<usize>,
}

fn marker(line: &str) -> Option<Marker<'_>> {
    let indent = line.len() - line.trim_start().len();
    let line = line.trim_start();
    let split = line.find([' ', '\t'])?;
    let tag = &line[..split];
    let text = line[split..].trim_start();
    let (kind, level, start) = match tag.as_bytes().first()? {
        b'*' if tag.bytes().all(|ch| ch == b'*') => (ListKind::Bullet, tag.len(), 1),
        b'-' if tag == "-" => (ListKind::Bullet, 1, 1),
        b'.' if tag.bytes().all(|ch| ch == b'.') => (ListKind::Ordered, tag.len(), 1),
        b'<' if tag == "<.>" => (ListKind::Callout, 1, 1),
        b'<' => (
            ListKind::Callout,
            1,
            tag[1..].strip_suffix('>')?.parse().ok()?,
        ),
        b'0'..=b'9' => (ListKind::Ordered, 1, tag.strip_suffix('.')?.parse().ok()?),
        _ => return None,
    };
    Some(Marker {
        kind,
        level,
        start,
        text,
        indent,
    })
}

fn definition(line: &str) -> Option<(&str, usize, &str)> {
    let line = line.trim();
    let (term, rest) = line.split_once("::")?;
    let content = rest.trim_start_matches(':');
    let count = 2 + rest.len() - content.len();
    (!term.is_empty()
        && !term.contains([':', '[', ']'])
        && (content.is_empty() || content.starts_with([' ', '\t'])))
    .then(|| (term, count, content.trim()))
}

fn callouts(text: &str) -> String {
    let mut result = String::new();
    let mut tail = text;
    let mut next = 1;
    while let Some((prefix, rest)) = tail.split_once('<') {
        result.push_str(prefix);
        if let Some(end) = rest.as_bytes().iter().take(4).position(|ch| *ch == b'>') {
            let value = &rest[..end];
            let number = if value == "." {
                Some(next)
            } else {
                value.parse::<u32>().ok()
            };
            if let Some(number) = number.filter(|number| (1..=20).contains(number)) {
                if let Some(mark) = char::from_u32(0x245f + number) {
                    result.push(mark);
                }
                next = number + 1;
                tail = &rest[end + 1..];
                continue;
            }
        }
        result.push('<');
        tail = rest;
    }
    result.push_str(tail);
    result
}

fn ordered_style(style: &str) -> bool {
    matches!(
        style,
        "arabic" | "loweralpha" | "upperalpha" | "lowerroman" | "upperroman"
    )
}

fn attributes_for_wrapper(attr: Option<Attr>) -> attrs::Attributes<'static> {
    attrs::Attributes {
        attr: attr.unwrap_or_default(),
        ..attrs::Attributes::default()
    }
}

fn code_fence(line: &str) -> Option<(&str, &str)> {
    let count = line.bytes().take_while(|ch| *ch == b'`').count();
    (count >= 3).then(|| (&line[..count], line[count..].trim()))
}

fn is_comment(line: &str) -> bool {
    line == "//" || line.starts_with("// ")
}
