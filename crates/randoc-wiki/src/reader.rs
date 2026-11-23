use randoc_ast::{Attr, Block, Caption, Format, Inline, ListAttributes, MathType, Pandoc};
use randoc_core::shared::{IdentifierSet, auto_identifier};
use randoc_core::{Context, Error, ReaderOptions, Result, Sources};
use randoc_core::{Extension, Extensions};

use crate::inline::behavior_switch;
use crate::scan::{Region, Scan, Tag, attrs, line_end};
use crate::{Wiki, depth_error};

type ParsedBlock = Option<(Option<Block>, usize)>;

pub(crate) fn read(
    wiki: Wiki,
    sources: &Sources,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let size = sources.0.iter().try_fold(0u64, |total, source| {
        total.checked_add(u64::try_from(source.text.len()).ok()?)
    });
    if size.is_none_or(|size| size > context.limits.max_input_size) {
        return Err(Error::Parse("Wiki input size limit exceeded".into()));
    }
    let mut parser = Parser {
        wiki,
        extensions: options.extensions,
        limit: context.limits.max_nesting_depth,
        cells_left: context.limits.max_table_cells,
        external_count: 0,
        categories: Vec::new(),
        ids: IdentifierSet::new(),
        source: "",
        name: "",
    };
    let mut blocks = Vec::new();
    for source in &sources.0 {
        parser.source = &source.text;
        parser.name = &source.name;
        blocks.extend(parser.blocks(&source.text, 0)?);
    }
    if !parser.categories.is_empty() {
        let mut categories = Vec::new();
        for node in parser.categories {
            if !categories.is_empty() {
                categories.push(Inline::Space);
            }
            categories.push(node);
        }
        blocks.push(Block::Para(categories));
    }
    Ok(Pandoc {
        meta: Default::default(),
        blocks,
    })
}

pub(crate) struct Parser<'a> {
    pub(crate) wiki: Wiki,
    pub(crate) limit: usize,
    pub(crate) cells_left: usize,
    pub(crate) external_count: usize,
    pub(crate) categories: Vec<Inline>,
    extensions: Extensions,
    ids: IdentifierSet,
    source: &'a str,
    name: &'a str,
}

impl Parser<'_> {
    pub(crate) fn enabled(&self, extension: Extension) -> bool {
        self.extensions.contains(extension)
    }

    pub(crate) fn depth(&self, depth: usize) -> Result<()> {
        if depth > self.limit {
            Err(depth_error())
        } else {
            Ok(())
        }
    }

    pub(crate) fn ref_error(&self, fragment: &str, at: usize) -> Error {
        self.unexpected(fragment, at, r#"(TagClose "ref","</ref>")"#)
    }

    pub(crate) fn unexpected(&self, fragment: &str, at: usize, token: &str) -> Error {
        let source_at = (fragment.as_ptr() as usize)
            .checked_sub(self.source.as_ptr() as usize)
            .and_then(|start| start.checked_add(at))
            .filter(|&end| end <= self.source.len() && self.source.is_char_boundary(end));
        let (text, end) = source_at.map_or((fragment, at), |end| (self.source, end));
        let start = text[..end].rfind('\n').map_or(0, |start| start + 1);
        let line = text[..start].bytes().filter(|&c| c == b'\n').count() + 1;
        let column = text[start..end].chars().fold(1, |column, ch| {
            if ch == '\t' {
                column + 8 - (column - 1) % 8
            } else {
                column + 1
            }
        });
        let name = if self.name.is_empty() {
            String::new()
        } else {
            format!("{:?} ", self.name)
        };
        let content = &text[start..line_end(text, end)];
        Error::Parse(format!(
            "Error at {name}(line {line}, column {column}):\nunexpected {token}\n{content}\n{}",
            " ".repeat(column - 1) + "^"
        ))
    }

    pub(crate) fn blocks(&mut self, text: &str, depth: usize) -> Result<Vec<Block>> {
        self.depth(depth)?;
        let scan = Scan::new(text, self.wiki);
        self.block_range(&scan, 0, text.len(), depth)
    }

    pub(crate) fn block_range(
        &mut self,
        scan: &Scan<'_>,
        start: usize,
        end: usize,
        depth: usize,
    ) -> Result<Vec<Block>> {
        self.depth(depth)?;
        let mut blocks = Vec::new();
        let mut at = start;
        let mut line_end_at = line_end(scan.text, at).min(end);
        while at < end {
            if at > line_end_at {
                line_end_at = line_end(scan.text, at).min(end);
            }
            let new_line = at == start || scan.text.as_bytes()[at - 1] == b'\n';
            let line = scan.text[at..line_end_at].trim_end_matches('\r');
            let trim = line.trim_start();
            let first = at + line.len() - trim.len();
            let after = line_end_at.saturating_add(1).min(end);
            if trim.is_empty() || self.wiki == Wiki::Media && behavior_switch(trim) == Some(trim) {
                at = after;
                continue;
            }
            if self.wiki == Wiki::Media
                && let Some(&close) = scan.tables.get(&first).filter(|&&close| close < end)
            {
                if let Some(table) = self.media_table(scan, first, line_end_at, close, depth + 1)? {
                    blocks.push(table);
                }
                at = close + 2;
                continue;
            }
            if self.wiki == Wiki::Media
                && line.starts_with(':')
                && let Some((block, next)) = self.display_math(scan, at, end)
            {
                blocks.push(block);
                at = next;
                continue;
            }
            if (self.wiki != Wiki::Media || at == first || !trim.starts_with("{{"))
                && let Some((block, next)) = self.special_block(scan, first, end, depth)?
            {
                if let Some(block) = block {
                    blocks.push(block);
                }
                at = next;
                continue;
            }
            if let Some(next) =
                self.regular_block(scan, &mut blocks, at, line_end_at, end, depth, new_line)?
            {
                at = next;
            } else {
                at = self.paragraph(scan, &mut blocks, at, end, depth)?;
            }
        }
        Ok(blocks)
    }

    #[allow(clippy::too_many_arguments)]
    fn regular_block(
        &mut self,
        scan: &Scan<'_>,
        blocks: &mut Vec<Block>,
        at: usize,
        line_end_at: usize,
        end: usize,
        depth: usize,
        new_line: bool,
    ) -> Result<Option<usize>> {
        let line = scan.text[at..line_end_at].trim_end_matches('\r');
        let trim = line.trim_start();
        let after = line_end_at.saturating_add(1).min(end);
        let next_at;
        if let Some((level, label)) = heading(self.wiki, line) {
            blocks.push(self.header(scan, at, line, level, label, depth)?);
            next_at = after;
        } else if trim.trim_end().bytes().all(|c| c == b'-') && trim.trim_end().len() >= 4 {
            blocks.push(Block::HorizontalRule);
            next_at = after;
        } else if self.wiki != Wiki::Media && pipe_start(self.wiki, trim) {
            if let Some((block, next)) = self.pipe_table(scan, at, end, depth + 1)? {
                blocks.push(block);
                next_at = next;
            } else {
                let content = self.inlines(scan, at, line_end_at, depth)?;
                append_paragraph(self.wiki, blocks, content);
                next_at = after;
            }
        } else if new_line && let Some(marker) = list_start(self.wiki, line) {
            let (lists, next) = self.lists(scan, at, end, depth, marker);
            blocks.extend(lists?);
            next_at = next;
        } else if self.wiki == Wiki::Media
            && (line.starts_with(':') || new_line && line.starts_with(';'))
        {
            let (block, next) = self.definitions(scan, at, end, depth + 1)?;
            blocks.push(block);
            next_at = next;
        } else if self.wiki == Wiki::Doku && line.starts_with('>') {
            let (block, next) = self.doku_quote(scan, at, end, depth + 1)?;
            blocks.push(block);
            next_at = next;
        } else if new_line
            && line.starts_with(' ')
            && (self.wiki == Wiki::Media || self.wiki == Wiki::Doku && line.starts_with("  "))
        {
            let (block, next) = self.indented(scan, at, end, depth)?;
            blocks.push(block);
            next_at = next;
        } else {
            return Ok(None);
        }
        Ok(Some(next_at))
    }

    fn paragraph(
        &mut self,
        scan: &Scan<'_>,
        blocks: &mut Vec<Block>,
        at: usize,
        end: usize,
        depth: usize,
    ) -> Result<usize> {
        let mut stop = self.paragraph_line(scan, at, end);
        while stop < end && scan.text.as_bytes()[stop] == b'\n' {
            let next = stop + 1;
            let next_end = line_end(scan.text, next).min(end);
            let line = &scan.text[next..next_end];
            if self.block_start(scan, next, end, line) {
                break;
            }
            stop = self.paragraph_line(scan, next, end);
        }
        let content = self.inlines(scan, at, stop, depth)?;
        append_paragraph(self.wiki, blocks, content);
        Ok(if scan.text.as_bytes().get(stop) == Some(&b'\n') {
            (stop + 1).min(end)
        } else {
            stop
        })
    }

    fn header(
        &mut self,
        scan: &Scan<'_>,
        at: usize,
        line: &str,
        level: i64,
        label: &str,
        depth: usize,
    ) -> Result<Block> {
        let start = at + (label.as_ptr() as usize - line.as_ptr() as usize);
        let content = if self.wiki == Wiki::Creole {
            vec![Inline::Str(label.into())]
        } else {
            self.inlines(scan, start, start + label.len(), depth)?
        };
        let mut attr = Attr::default();
        if self.enabled(Extension::AutoIdentifiers) {
            let base = auto_identifier(&content, self.extensions);
            let base = if base.is_empty() { "section" } else { &base };
            attr.id = self.ids.unique(base);
            if self.wiki == Wiki::Media {
                attr.id = attr.id.replace('-', "_");
            }
        }
        Ok(Block::Header(level, attr, content))
    }

    fn paragraph_line(&self, scan: &Scan<'_>, start: usize, end: usize) -> usize {
        if self.wiki != Wiki::Media {
            return self.logical_line(scan, start, end);
        }
        let mut at = start;
        while at < end && scan.text.as_bytes()[at] != b'\n' {
            if at > start
                && scan.tags.get(&at).is_some_and(|tag| {
                    is_raw_block_tag(tag.name)
                        || tag.name.eq_ignore_ascii_case("p")
                        || tag.name.eq_ignore_ascii_case("gallery")
                        || is_code_block_tag(tag.name)
                        || tag.close && tag.name.eq_ignore_ascii_case("blockquote")
                        || !tag.close
                            && tag.name.eq_ignore_ascii_case("blockquote")
                            && scan.region(at, end).is_some()
                })
            {
                break;
            }
            at = scan.skip(at, end);
        }
        at
    }

    pub(crate) fn logical_line(&self, scan: &Scan<'_>, start: usize, end: usize) -> usize {
        let mut at = start;
        while at < end && scan.text.as_bytes()[at] != b'\n' {
            at = scan.skip(at, end);
        }
        at
    }

    fn block_start(&self, scan: &Scan<'_>, at: usize, end: usize, line: &str) -> bool {
        let trim = line.trim();
        let first = at + line.len() - line.trim_start().len();
        if trim.is_empty()
            || heading(self.wiki, line).is_some()
            || list_start(self.wiki, line).is_some()
            || (trim.len() >= 4 && trim.bytes().all(|c| c == b'-'))
            || (self.wiki != Wiki::Media && pipe_start(self.wiki, trim))
        {
            return true;
        }
        match self.wiki {
            Wiki::Media => {
                (line.starts_with([';', ':'])
                    || line.starts_with(' ') && !line.starts_with(" <!--"))
                    || scan.tables.contains_key(&first)
                    || scan.tags.get(&first).is_some_and(|tag| {
                        matches!(
                            tag.name.to_ascii_lowercase().as_str(),
                            "blockquote"
                                | "div"
                                | "pre"
                                | "source"
                                | "syntaxhighlight"
                                | "p"
                                | "references"
                                | "gallery"
                                | "ol"
                                | "ul"
                        )
                    })
            }
            Wiki::Doku => {
                line.starts_with(">")
                    || line.starts_with("  ")
                    || scan
                        .tags
                        .get(&first)
                        .is_some_and(|tag| matches!(tag.name, "code" | "file" | "HTML"))
            }
            Wiki::Jira => {
                line.starts_with("bq.")
                    || scan.region(first, end).is_some()
                        && ["{code", "{noformat", "{quote", "{panel", "{color:"]
                            .iter()
                            .any(|mark| line.starts_with(mark))
            }
            Wiki::Creole => trim == "{{{" && scan.region(first, end).is_some(),
        }
    }

    fn special_block(
        &mut self,
        scan: &Scan<'_>,
        at: usize,
        end: usize,
        depth: usize,
    ) -> Result<ParsedBlock> {
        let tail = &scan.text[at..end];
        let region = scan.region(at, end);
        if self.wiki == Wiki::Media {
            if tail.starts_with("<!--")
                && let Some(r) = region
            {
                let eol = line_end(scan.text, r.end).min(end);
                if scan.text[r.end..eol].trim().is_empty() {
                    return Ok(Some((None, r.end)));
                }
            }
            if tail.starts_with("{{")
                && !tail.starts_with("{{{")
                && let Some(r) = region
            {
                return Ok(Some((
                    Some(Block::RawBlock(
                        Format("mediawiki".into()),
                        scan.text[at..r.end].into(),
                    )),
                    r.end,
                )));
            }
        }
        if self.wiki == Wiki::Creole
            && tail.starts_with("{{{\n")
            && let Some(r) = region
        {
            let body = &scan.text[r.body + 1..r.close];
            let text = body.strip_suffix('\n').unwrap_or(body);
            return Ok(Some((
                Some(Block::CodeBlock(Attr::default(), text.into())),
                r.end,
            )));
        }
        if self.wiki == Wiki::Jira {
            return self.jira_block(scan, at, end, depth, region);
        }
        if matches!(self.wiki, Wiki::Media | Wiki::Doku)
            && let Some(tag) = scan.tags.get(&at)
        {
            return self.tag_block(scan, at, depth, *tag, region);
        }
        Ok(None)
    }

    fn jira_block(
        &mut self,
        scan: &Scan<'_>,
        at: usize,
        end: usize,
        depth: usize,
        region: Option<Region>,
    ) -> Result<ParsedBlock> {
        let tail = &scan.text[at..end];
        if let Some(body) = tail.strip_prefix("bq. ") {
            let stop = line_end(scan.text, at).min(end);
            let content = self.inlines(scan, at + 4, stop, depth + 1)?;
            return Ok(Some((
                Some(Block::BlockQuote(vec![Block::Para(content)])),
                stop.min(at + 4 + body.len()),
            )));
        }
        if let Some(r) = region
            && tail.starts_with('{')
        {
            let spec = &scan.text[at + 1..r.body - 1];
            let (name, args) = spec.split_once(':').unwrap_or((spec, ""));
            let body = &scan.text[r.body..r.close];
            let block = match name {
                "code" | "noformat" => {
                    let mut attr = Attr::default();
                    if name == "code" {
                        attr.classes.push("java".into());
                    }
                    for (index, part) in args.split('|').enumerate() {
                        if let Some((key, value)) = part.split_once('=') {
                            attr.attributes.push((key.into(), value.into()));
                        } else if index == 0 && !part.is_empty() {
                            attr.classes = vec![part.into()];
                        }
                    }
                    Block::CodeBlock(attr, body.strip_prefix('\n').unwrap_or(body).into())
                }
                "quote" => Block::BlockQuote(self.block_range(scan, r.body, r.close, depth + 1)?),
                "panel" => {
                    let mut attr = Attr::new("", vec!["panel".into()], vec![]);
                    let mut title = None;
                    for (key, value) in args.split('|').filter_map(|arg| arg.split_once('=')) {
                        if key == "title" {
                            title = Some(value);
                        } else {
                            attr.attributes.push((key.into(), value.into()));
                        }
                    }
                    let mut blocks = self.block_range(scan, r.body, r.close, depth + 1)?;
                    if let Some(title) = title {
                        let title_scan = Scan::new(title, self.wiki);
                        let title = self.inlines(&title_scan, 0, title.len(), depth + 1)?;
                        blocks.insert(
                            0,
                            Block::Div(
                                Attr::new("", vec!["panelheader".into()], vec![]),
                                vec![Block::Plain(vec![Inline::Strong(title)])],
                            ),
                        );
                    }
                    Block::Div(attr, blocks)
                }
                "color" if body.starts_with('\n') => {
                    let mut blocks = self.block_range(scan, r.body, r.close, depth + 1)?;
                    if let Some(Block::Para(inlines)) = blocks.first_mut() {
                        inlines.insert(0, Inline::LineBreak);
                    }
                    Block::Div(
                        Attr::new("", vec![], vec![("color".into(), args.into())]),
                        blocks,
                    )
                }
                _ => return Ok(None),
            };
            return Ok(Some((Some(block), r.end)));
        }
        Ok(None)
    }

    fn tag_block(
        &mut self,
        scan: &Scan<'_>,
        at: usize,
        depth: usize,
        tag: Tag<'_>,
        region: Option<Region>,
    ) -> Result<ParsedBlock> {
        let name = tag.name.to_ascii_lowercase();
        if self.wiki == Wiki::Media && matches!(name.as_str(), "p" | "gallery") {
            return Ok(Some((None, tag.end)));
        }
        if self.wiki == Wiki::Media
            && (is_raw_block_tag(&name)
                || tag.close && (is_code_block_tag(&name) || name == "blockquote"))
        {
            return Ok(Some((
                Some(Block::RawBlock(
                    Format("html".into()),
                    scan.text[at..tag.end].into(),
                )),
                tag.end,
            )));
        }
        if !tag.close
            && let Some(r) = region
        {
            let body = &scan.text[r.body..r.close];
            let block = if self.wiki == Wiki::Media {
                match name.as_str() {
                    "pre" | "source" | "syntaxhighlight" | "haskell" => {
                        let tags = attrs(tag.attrs);
                        let mut attr = Attr::default();
                        if let Some(lang) = tags
                            .get("lang")
                            .or((name == "haskell").then_some("haskell"))
                        {
                            attr.classes.push(lang.into());
                        }
                        if tags.get("line").is_some() {
                            attr.classes.push("numberLines".into());
                        }
                        if let Some(start) = tags.get("start") {
                            attr.attributes.push(("startFrom".into(), start.into()));
                        }
                        Block::CodeBlock(attr, body.trim_matches('\n').into())
                    }
                    "blockquote" => {
                        Block::BlockQuote(self.block_range(scan, r.body, r.close, depth + 1)?)
                    }
                    "ol" | "ul" => {
                        self.html_list(scan, r.body, r.close, depth + 1, &name, tag.attrs)?
                    }
                    _ => return Ok(None),
                }
            } else {
                match name.as_str() {
                    "code" | "file" => {
                        let mut attr = Attr::default();
                        if let Some(lang) = tag.attrs.split_whitespace().next() {
                            attr.classes.push(lang.into());
                        }
                        Block::CodeBlock(attr, body.strip_prefix('\n').unwrap_or(body).into())
                    }
                    "html" if tag.name == "HTML" => {
                        Block::RawBlock(Format("html".into()), body.into())
                    }
                    _ => return Ok(None),
                }
            };
            return Ok(Some((Some(block), r.end)));
        }
        Ok(None)
    }

    fn html_list(
        &mut self,
        scan: &Scan<'_>,
        start: usize,
        end: usize,
        depth: usize,
        name: &str,
        properties: &str,
    ) -> Result<Block> {
        self.depth(depth)?;
        let mut items = Vec::new();
        let mut at = start;
        while at < end {
            if scan
                .tags
                .get(&at)
                .is_some_and(|tag| !tag.close && tag.name.eq_ignore_ascii_case("li"))
                && let Some(r) = scan.region(at, end)
            {
                let mut blocks = self.block_range(scan, r.body, r.close, depth)?;
                for index in 0..blocks.len() {
                    if (blocks.len() == 1
                        || matches!(
                            blocks.get(index + 1),
                            Some(
                                Block::BulletList(_)
                                    | Block::OrderedList(_, _)
                                    | Block::DefinitionList(_)
                            )
                        ))
                        && let Block::Para(inlines) = &mut blocks[index]
                    {
                        blocks[index] = Block::Plain(std::mem::take(inlines));
                    }
                }
                items.push(blocks);
                at = r.end;
            } else {
                at = scan.skip(at, end);
            }
        }
        if items.is_empty() {
            let mut blocks = self.block_range(scan, start, end, depth)?;
            if blocks.len() == 1 {
                return Ok(blocks.remove(0));
            }
        }
        Ok(if name == "ol" {
            let start = attrs(properties)
                .get("start")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);
            Block::OrderedList(
                ListAttributes {
                    start,
                    ..Default::default()
                },
                items,
            )
        } else {
            Block::BulletList(items)
        })
    }

    fn indented(
        &mut self,
        scan: &Scan<'_>,
        start: usize,
        end: usize,
        depth: usize,
    ) -> Result<(Block, usize)> {
        let mut at = start;
        let mut lines = Vec::new();
        let prefix = if self.wiki == Wiki::Media { " " } else { "  " };
        while at < end {
            let stop = self.logical_line(scan, at, end);
            let Some(line) = scan.text[at..stop].strip_prefix(prefix) else {
                break;
            };
            if line.trim().is_empty() {
                break;
            }
            lines.push((at + prefix.len(), stop));
            at = stop.saturating_add(1).min(end);
        }
        let block = if self.wiki == Wiki::Doku {
            let mut text = String::new();
            for (start, end) in lines {
                text.push_str(&scan.text[start..end]);
                text.push('\n');
            }
            Block::CodeBlock(Attr::default(), text)
        } else {
            let mut items = Vec::new();
            for (start, end) in lines {
                if !items.is_empty() {
                    items.push(Inline::LineBreak);
                }
                items.extend(self.preformatted_line(scan, start, end, depth)?);
            }
            Block::Para(items)
        };
        if at == start {
            at = line_end(scan.text, at).saturating_add(1).min(end);
        }
        Ok((block, at))
    }

    fn display_math(&self, scan: &Scan<'_>, at: usize, end: usize) -> Option<(Block, usize)> {
        let tail = &scan.text[at..end];
        let body = tail.trim_start_matches(':');
        let start = end - body.len();
        let tag = scan.tags.get(&start)?;
        if !tag.name.eq_ignore_ascii_case("math") {
            return None;
        }
        let region = scan.region(start, end)?;
        let text = scan.text[region.body..region.close].trim();
        Some((
            Block::Para(vec![Inline::Math(MathType::DisplayMath, text.into())]),
            region.end,
        ))
    }

    fn definitions(
        &mut self,
        scan: &Scan<'_>,
        start: usize,
        end: usize,
        depth: usize,
    ) -> Result<(Block, usize)> {
        self.depth(depth)?;
        let mut lines = Vec::new();
        let mut at = start;
        while at < end && scan.text[at..end].starts_with([':', ';']) {
            let stop = self.logical_line(scan, at, end);
            lines.push((at, stop));
            at = stop.saturating_add(1).min(end);
        }
        Ok((self.definition_block(scan, &mut lines, depth)?, at))
    }

    fn definition_block(
        &mut self,
        scan: &Scan<'_>,
        lines: &mut [(usize, usize)],
        depth: usize,
    ) -> Result<Block> {
        self.depth(depth)?;
        let mut terms = Vec::new();
        let mut current = 0;
        while let Some(&(start, end)) = lines.get(current) {
            let mut term = Vec::new();
            if scan.text[start..end].starts_with(';') {
                let fields = scan.split(start + 1, end, ":");
                term = self.inlines(scan, start + 1, fields[0].1, depth)?;
                if fields.len() > 1 {
                    lines[current].0 = fields[0].1;
                } else {
                    current += 1;
                }
            }
            let items = self.definition_items(scan, lines, &mut current, 1, depth)?;
            terms.push((term, items));
        }
        Ok(Block::DefinitionList(terms))
    }

    fn definition_items(
        &mut self,
        scan: &Scan<'_>,
        lines: &[(usize, usize)],
        current: &mut usize,
        level: usize,
        depth: usize,
    ) -> Result<Vec<Vec<Block>>> {
        self.depth(depth)?;
        let mut items = Vec::new();
        while let Some(&(start, end)) = lines.get(*current) {
            let (run, content_start) = definition_marker(scan.text, start, end);
            if run < level {
                break;
            }
            let mut content = Vec::new();
            if run == level {
                content = if scan.text[content_start..end].starts_with("{{")
                    && scan.region(content_start, end).is_some()
                {
                    self.block_range(scan, content_start, end, depth)?
                } else {
                    vec![Block::Plain(self.inlines(
                        scan,
                        content_start,
                        end,
                        depth,
                    )?)]
                };
                *current += 1;
            }
            if let Some(&(start, end)) = lines.get(*current) {
                let (next, _) = definition_marker(scan.text, start, end);
                if next > level {
                    let nested_level = if run > level { run } else { level + 1 };
                    let nested =
                        self.definition_items(scan, lines, current, nested_level, depth + 1)?;
                    content.push(Block::DefinitionList(vec![(vec![], nested)]));
                }
            }
            items.push(content);
        }
        Ok(items)
    }

    fn lists(
        &mut self,
        scan: &Scan<'_>,
        start: usize,
        end: usize,
        depth: usize,
        first: Marker<'_>,
    ) -> (Result<Vec<Block>>, usize) {
        let mut at = start;
        let mut entries = Vec::new();
        while at < end {
            let stop = self.logical_line(scan, at, end);
            let line = &scan.text[at..stop];
            let next = stop.saturating_add(1).min(end);
            if let Some(marker) = list_marker(self.wiki, line) {
                let level = if self.wiki == Wiki::Doku {
                    marker.level.saturating_sub(first.level).saturating_add(1)
                } else {
                    marker.level
                };
                if let Err(error) = self.depth(depth + level) {
                    return (Err(error), next);
                }
                let item_start = stop - marker.body.len();
                if self.wiki == Wiki::Media
                    && let Err(error) = self.check_list_refs(scan, item_start, stop)
                {
                    return (Err(error), next);
                }
                entries.push(ListEntry {
                    extras: vec![],
                    marker,
                    level,
                    start: item_start,
                    end: stop,
                });
            } else if self.wiki == Wiki::Creole && line.trim_start().starts_with("{{{\n") {
                break;
            } else if self.wiki == Wiki::Jira
                && let Some(entry) = entries.last_mut()
                && (line.starts_with("bq. ")
                    || line.starts_with(['{']) && scan.region(at, end).is_some())
            {
                entry.extras.push((at, stop));
            } else if matches!(self.wiki, Wiki::Jira | Wiki::Creole)
                && !self.block_start(scan, at, end, line)
                && let Some(entry) = entries.last_mut()
            {
                entry.end = stop;
            } else {
                break;
            }
            at = next;
        }
        (self.list_level(scan, &entries, &mut 0, 1, depth), at)
    }

    fn check_list_refs(&self, scan: &Scan<'_>, start: usize, end: usize) -> Result<()> {
        let mut at = start;
        while at < end {
            if scan
                .tags
                .get(&at)
                .is_some_and(|tag| tag.name.eq_ignore_ascii_case("ref"))
                && let Some(region) = scan.region(at, end)
                && self.logical_line(scan, region.body, region.close) < region.close
            {
                return Err(self.ref_error(scan.text, region.end));
            }
            at = scan.skip(at, end);
        }
        Ok(())
    }

    fn list_level(
        &mut self,
        scan: &Scan<'_>,
        entries: &[ListEntry<'_>],
        current: &mut usize,
        level: usize,
        depth: usize,
    ) -> Result<Vec<Block>> {
        self.depth(depth + level)?;
        let mut lists = Vec::new();
        while let Some(entry) = entries.get(*current).filter(|e| e.level >= level) {
            let ordered = entry.marker.ordered_at(level);
            let mut items = Vec::new();
            while let Some(entry) = entries
                .get(*current)
                .filter(|e| e.level >= level && e.marker.ordered_at(level) == ordered)
            {
                let mut body = Vec::new();
                if entry.level == level {
                    if self.wiki == Wiki::Media
                        && scan.text[entry.start..entry.end].starts_with("{{")
                        && !scan.text[entry.start..entry.end].starts_with("{{{")
                        && scan.region(entry.start, entry.end).is_some()
                    {
                        body = self.block_range(scan, entry.start, entry.end, depth + level)?;
                    } else {
                        let content = self.inlines(scan, entry.start, entry.end, depth + level)?;
                        body.push(if self.wiki == Wiki::Jira {
                            Block::Para(content)
                        } else {
                            Block::Plain(content)
                        });
                    }
                    for &(start, end) in &entry.extras {
                        body.extend(self.block_range(scan, start, end, depth + level)?);
                    }
                    *current += 1;
                }
                while let Some(entry) = entries
                    .get(*current)
                    .filter(|e| e.level > level && e.marker.ordered_at(level) == ordered)
                {
                    if self.wiki == Wiki::Media
                        && matches!(entry.marker.run.as_bytes().get(level), Some(b':' | b';'))
                    {
                        body.extend(self.list_definitions(scan, entries, current, level, depth)?);
                    } else {
                        body.extend(self.list_level(scan, entries, current, level + 1, depth)?);
                    }
                }
                items.push(body);
            }
            lists.push(if ordered {
                Block::OrderedList(ListAttributes::default(), items)
            } else {
                Block::BulletList(items)
            });
        }
        Ok(lists)
    }

    fn list_definitions(
        &mut self,
        scan: &Scan<'_>,
        entries: &[ListEntry<'_>],
        current: &mut usize,
        level: usize,
        depth: usize,
    ) -> Result<Vec<Block>> {
        let mut lines = Vec::new();
        let mut blocks = Vec::new();
        let prefix = &entries[*current].marker.run.as_bytes()[..level];
        while let Some(entry) = entries.get(*current) {
            let run = entry.marker.run.as_bytes();
            if !run.starts_with(prefix) || !matches!(run.get(level), Some(b':' | b';')) {
                break;
            }
            let start = entry.marker.run.as_ptr() as usize - scan.text.as_ptr() as usize + level;
            if let Some((math, next)) = self.display_math(scan, start, entry.end)
                && next == entry.end
            {
                if !lines.is_empty() {
                    blocks.push(self.definition_block(scan, &mut lines, depth + level + 1)?);
                    lines.clear();
                }
                blocks.push(math);
            } else {
                lines.push((start, entry.end));
            }
            *current += 1;
        }
        if !lines.is_empty() {
            blocks.push(self.definition_block(scan, &mut lines, depth + level + 1)?);
        }
        Ok(blocks)
    }

    fn doku_quote(
        &mut self,
        scan: &Scan<'_>,
        start: usize,
        end: usize,
        depth: usize,
    ) -> Result<(Block, usize)> {
        self.depth(depth)?;
        let mut at = start;
        let mut text = String::new();
        while at < end && scan.text.as_bytes()[at] == b'>' {
            let stop = self.logical_line(scan, at, end);
            let line = &scan.text[at + 1..stop];
            text.push_str(line.strip_prefix(' ').unwrap_or(line));
            text.push('\n');
            at = stop.saturating_add(1).min(end);
        }
        let mut blocks = self.blocks(&text, depth)?;
        for block in &mut blocks {
            if let Block::Para(items) = block {
                let mut items = std::mem::take(items);
                for node in &mut items {
                    if *node == Inline::SoftBreak {
                        *node = Inline::LineBreak;
                    }
                }
                *block = Block::Plain(items);
            }
        }
        Ok((Block::BlockQuote(blocks), at))
    }
}

pub(crate) fn append_paragraph(wiki: Wiki, blocks: &mut Vec<Block>, mut inlines: Vec<Inline>) {
    if inlines.is_empty() {
        return;
    }
    if wiki == Wiki::Media
        && let [Inline::Image(_, alt, _)] = inlines.as_mut_slice()
    {
        let caption = std::mem::take(alt);
        blocks.push(Block::Figure(
            Attr::default(),
            Caption {
                short: None,
                long: if caption.is_empty() {
                    vec![]
                } else {
                    vec![Block::Plain(caption)]
                },
            },
            vec![Block::Plain(inlines)],
        ));
    } else {
        blocks.push(Block::Para(inlines));
    }
}

fn heading(wiki: Wiki, line: &str) -> Option<(i64, &str)> {
    if wiki == Wiki::Jira {
        let [b'h', level @ b'1'..=b'6', b'.', b' ', ..] = line.as_bytes() else {
            return None;
        };
        return Some((i64::from(level - b'0'), line[4..].trim()));
    }
    let line = line.trim_end();
    let open = line.bytes().take_while(|&c| c == b'=').count();
    if open == 0 || open > 6 || wiki == Wiki::Doku && open == 1 {
        return None;
    }
    let close = line.bytes().rev().take_while(|&c| c == b'=').count();
    if open + close >= line.len() || wiki != Wiki::Creole && open != close {
        return None;
    }
    let level = if wiki == Wiki::Doku { 7 - open } else { open };
    Some((level as i64, line[open..line.len() - close].trim()))
}

fn is_raw_block_tag(name: &str) -> bool {
    [
        "references",
        "div",
        "center",
        "poem",
        "table",
        "tbody",
        "thead",
        "tfoot",
        "tr",
        "td",
        "th",
        "hr",
    ]
    .iter()
    .any(|tag| name.eq_ignore_ascii_case(tag))
}

pub(crate) fn is_code_block_tag(name: &str) -> bool {
    ["pre", "source", "syntaxhighlight", "haskell"]
        .iter()
        .any(|tag| name.eq_ignore_ascii_case(tag))
}

fn definition_marker(text: &str, start: usize, end: usize) -> (usize, usize) {
    let mut line = &text[start..end];
    let mut level = 0;
    while let Some(body) = line.strip_prefix(':') {
        level += 1;
        line = body.trim_start_matches([' ', '\t']);
    }
    (level, end - line.len())
}

fn pipe_start(wiki: Wiki, line: &str) -> bool {
    line.starts_with('|') || wiki == Wiki::Doku && line.starts_with('^')
}

#[derive(Clone, Copy)]
struct Marker<'a> {
    run: &'a str,
    level: usize,
    body: &'a str,
}

impl Marker<'_> {
    fn ordered_at(self, level: usize) -> bool {
        let marker = self
            .run
            .as_bytes()
            .get(level - 1)
            .or(self.run.as_bytes().first());
        matches!(marker, Some(b'#')) || self.run == "d-"
    }
}

struct ListEntry<'a> {
    extras: Vec<(usize, usize)>,
    marker: Marker<'a>,
    level: usize,
    start: usize,
    end: usize,
}

fn list_start(wiki: Wiki, line: &str) -> Option<Marker<'_>> {
    list_marker(wiki, line).filter(|marker| wiki != Wiki::Creole || marker.level == 1)
}

fn list_marker(wiki: Wiki, line: &str) -> Option<Marker<'_>> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let leading = line.len() - trimmed.len();
    let initial = *trimmed.as_bytes().first()?;
    if wiki == Wiki::Doku {
        if leading < 2
            || !matches!(initial, b'*' | b'-')
            || !trimmed[1..].starts_with(char::is_whitespace)
        {
            return None;
        }
        return Some(Marker {
            run: if initial == b'-' { "d-" } else { "*" },
            level: leading / 2,
            body: trimmed[1..].trim_start(),
        });
    }
    if wiki != Wiki::Creole && leading != 0
        || !matches!(initial, b'*' | b'#') && !(wiki == Wiki::Jira && initial == b'-')
    {
        return None;
    }
    let run = if wiki == Wiki::Creole {
        trimmed.bytes().take_while(|&b| b == initial).count()
    } else if initial == b'-' {
        1
    } else {
        trimmed
            .bytes()
            .take_while(|b| {
                matches!(b, b'*' | b'#') || wiki == Wiki::Media && matches!(b, b':' | b';')
            })
            .count()
    };
    let body = &trimmed[run..];
    if wiki == Wiki::Jira && !body.starts_with(char::is_whitespace) {
        return None;
    }
    if wiki == Wiki::Creole && matches!(body.as_bytes().first(), Some(b'}' | b',' | b'.')) {
        return None;
    }
    Some(Marker {
        run: &trimmed[..run],
        level: run,
        body: body.trim_start(),
    })
}
