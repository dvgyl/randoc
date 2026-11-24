use std::collections::{HashMap, HashSet};

use randoc_ast::{
    Alignment, Attr, Block, Cell, ColSpec, Format, Inline, ListAttributes, ListNumberDelim,
    ListNumberStyle, Pandoc, Row, Table, TableBody, Target,
    walk::{VisitorMut, walk_mut},
};
use randoc_core::{
    Context, Extension, LogMessage, ReaderOptions, Result, SourcePos, Sources, shared::stringify,
};

use crate::{attrs, depth_error, identifier, inlines, size_error};

const EXPANSION_OVERHEAD: usize = 64;

fn charge(budget: &mut usize, size: usize) -> Result<()> {
    *budget = budget.checked_sub(size).ok_or_else(size_error)?;
    Ok(())
}

#[derive(Clone, Copy)]
struct Line<'a> {
    text: &'a str,
    number: usize,
    column: usize,
    newline: bool,
    quote_end: Option<(usize, usize)>,
}

impl<'a> Line<'a> {
    fn tail(self, bytes: usize) -> Self {
        Self {
            text: &self.text[bytes..],
            column: self.column + self.text[..bytes].chars().count(),
            ..self
        }
    }

    fn trim(self) -> Self {
        self.tail(self.text.len() - self.text.trim_start().len())
    }
}

struct State<'a, 's> {
    context: &'a mut Context,
    sourcepos: bool,
    east_asian: bool,
    ids: HashSet<String>,
    suffixes: HashMap<String, usize>,
    headings: HashMap<String, String>,
    references: HashMap<String, (Attr, String)>,
    notes: HashMap<String, Vec<Line<'s>>>,
    budget: usize,
    table_cells: usize,
}

pub(crate) fn read(
    sources: &Sources,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let size = sources.0.iter().fold(0u64, |size, source| {
        size.saturating_add(source.text.len() as u64)
    });
    if size > context.limits.max_input_size {
        return Err(size_error());
    }
    let source = expand_tabs(sources.concat(), options.tab_stop);
    let lines = source
        .split_inclusive('\n')
        .enumerate()
        .map(|(index, text)| {
            let body = text.strip_suffix('\n').unwrap_or(text);
            Line {
                text: body.strip_suffix('\r').unwrap_or(body),
                number: index + 1,
                column: 1,
                newline: text.ends_with('\n'),
                quote_end: None,
            }
        })
        .collect::<Vec<_>>();
    let mut state = State {
        sourcepos: options.extensions.contains(Extension::Sourcepos),
        east_asian: options.extensions.contains(Extension::EastAsianLineBreaks),
        budget: usize::try_from(
            context
                .limits
                .max_input_size
                .min(context.limits.max_archive_size)
                .min(size.max(1024).saturating_mul(64)),
        )
        .unwrap_or(usize::MAX),
        table_cells: context.limits.max_table_cells,
        context,
        ids: HashSet::new(),
        suffixes: HashMap::new(),
        headings: HashMap::new(),
        references: HashMap::new(),
        notes: HashMap::new(),
    };
    let mut blocks = state.blocks(&lines, 0)?;
    state.resolve(&mut blocks, 0)?;
    Ok(Pandoc {
        blocks,
        ..Pandoc::default()
    })
}

struct Positioned {
    block: Block,
    first: (usize, usize),
    last: (usize, usize),
    heading: Option<Attr>,
}

struct Section {
    level: i64,
    attr: Attr,
    title: String,
    first: (usize, usize),
    last: (usize, usize),
    blocks: Vec<Block>,
}

#[derive(Default)]
struct Frame {
    div: Option<(usize, Attr, (usize, usize))>,
    depth: usize,
    blocks: Vec<Block>,
    sections: Vec<Section>,
    pending: Attr,
}

impl Frame {
    fn append(&mut self, block: Block, last: (usize, usize)) {
        if let Some(section) = self.sections.last_mut() {
            section.last = last;
            section.blocks.push(block);
        } else {
            self.blocks.push(block);
        }
    }
}

struct Expansion {
    key: Option<String>,
    blocks: Vec<Block>,
    references: std::vec::IntoIter<(String, usize)>,
    notes: Vec<Option<Vec<Block>>>,
}

impl Expansion {
    fn new(blocks: Vec<Block>, depth: usize, limit: usize, key: Option<String>) -> Result<Self> {
        let mut references = NoteReferences {
            limit,
            items: Vec::new(),
        };
        references.blocks(&blocks, depth)?;
        Ok(Self {
            key,
            blocks,
            references: references.items.into_iter(),
            notes: Vec::new(),
        })
    }
}

struct NoteReferences {
    limit: usize,
    items: Vec<(String, usize)>,
}

impl NoteReferences {
    fn blocks(&mut self, blocks: &[Block], depth: usize) -> Result<()> {
        let next = depth + 1;
        for block in blocks {
            if depth > self.limit {
                return Err(depth_error());
            }
            match block {
                Block::Plain(nodes) | Block::Para(nodes) | Block::Header(_, _, nodes) => {
                    self.inlines(nodes, depth)?;
                }
                Block::Div(_, blocks) | Block::BlockQuote(blocks) => self.blocks(blocks, next)?,
                Block::BulletList(items) | Block::OrderedList(_, items) => {
                    for item in items {
                        self.blocks(item, next)?;
                    }
                }
                Block::DefinitionList(items) => {
                    for (term, definitions) in items {
                        self.inlines(term, next)?;
                        for blocks in definitions {
                            self.blocks(blocks, next)?;
                        }
                    }
                }
                Block::Table(table) => {
                    self.blocks(&table.caption.long, next)?;
                    let body = table
                        .bodies
                        .iter()
                        .flat_map(|body| body.head.iter().chain(&body.body));
                    for row in table.head.rows.iter().chain(body).chain(&table.foot.rows) {
                        for cell in &row.cells {
                            self.blocks(&cell.content, next)?;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn inlines(&mut self, nodes: &[Inline], depth: usize) -> Result<()> {
        for node in nodes {
            if depth > self.limit {
                return Err(depth_error());
            }
            match node {
                Inline::RawInline(format, key) if format.0 == inlines::NOTE => {
                    self.items.push((key.clone(), depth + 1));
                }
                Inline::Emph(nodes)
                | Inline::Strong(nodes)
                | Inline::Subscript(nodes)
                | Inline::Superscript(nodes)
                | Inline::Span(_, nodes)
                | Inline::Link(_, nodes, _)
                | Inline::Image(_, nodes, _)
                | Inline::Quoted(_, nodes) => self.inlines(nodes, depth + 1)?,
                _ => {}
            }
        }
        Ok(())
    }
}

impl<'s> State<'_, 's> {
    fn check_depth(&self, depth: usize) -> Result<()> {
        if depth > self.context.limits.max_nesting_depth {
            Err(depth_error())
        } else {
            Ok(())
        }
    }

    fn blocks(&mut self, lines: &[Line<'s>], depth: usize) -> Result<Vec<Block>> {
        self.check_depth(depth)?;
        let mut frames = vec![Frame {
            depth,
            ..Frame::default()
        }];
        let mut cursor = 0;
        while let Some(line) = lines.get(cursor).copied() {
            let Some(frame) = frames.last_mut() else {
                break;
            };
            let trimmed = line.trim();
            if trimmed.text.is_empty() {
                cursor += 1;
                continue;
            }
            if let Some((attr, rest)) = attrs::parse(trimmed.text)
                && rest.trim().is_empty()
            {
                attrs::merge(&mut frame.pending, attr);
                cursor += 1;
                continue;
            }
            let level_depth = frame.depth + frame.sections.len();
            if let Some((width, name)) = fence(trimmed.text, ':') {
                if name.is_empty()
                    && frame
                        .div
                        .as_ref()
                        .is_some_and(|(open, _, _)| width >= *open)
                {
                    self.close_div(&mut frames, end(line))?;
                } else {
                    let mut attr = std::mem::take(&mut frame.pending);
                    if !name.is_empty() {
                        attr.classes.insert(0, name.into());
                    }
                    self.check_depth(level_depth + 1)?;
                    frames.push(Frame {
                        depth: level_depth + 1,
                        div: Some((width, attr, (trimmed.number, trimmed.column))),
                        ..Frame::default()
                    });
                }
                cursor += 1;
                continue;
            }
            if let Some((key, body, skip)) = definition(trimmed.text) {
                cursor += 1;
                if let Some(key) = key.strip_prefix('^') {
                    let mut note = vec![trimmed.tail(skip)];
                    let mut paragraph = !body.is_empty();
                    while let Some(next) = lines.get(cursor).copied() {
                        if next.text.trim().is_empty() {
                            paragraph = false;
                            note.push(next);
                        } else if next.text.starts_with(char::is_whitespace) {
                            note.push(next.trim());
                            paragraph = true;
                        } else if paragraph && !block_start(next.trim().text) {
                            note.push(next);
                        } else {
                            break;
                        }
                        cursor += 1;
                    }
                    while note.last().is_some_and(|line| line.text.trim().is_empty()) {
                        note.pop();
                    }
                    self.notes.insert(key.into(), note);
                } else {
                    let mut url = body.split_whitespace().collect::<String>();
                    while let Some(next) = lines.get(cursor).filter(|line| {
                        !line.text.trim().is_empty() && line.text.starts_with(char::is_whitespace)
                    }) {
                        url.push_str(next.text.trim());
                        cursor += 1;
                    }
                    self.references
                        .insert(key.into(), (std::mem::take(&mut frame.pending), url));
                }
                continue;
            }
            let (node, next) = self.one(
                lines,
                cursor,
                level_depth,
                std::mem::take(&mut frame.pending),
            )?;
            cursor = next;
            if let (Block::Header(level, _, title), Some(attr)) = (&node.block, node.heading) {
                let level = *level;
                let text = stringify(title);
                self.close_sections(&mut frame.sections, &mut frame.blocks, level)?;
                self.check_depth(frame.depth + frame.sections.len() + 1)?;
                frame.sections.push(Section {
                    level,
                    attr,
                    title: text,
                    first: node.first,
                    last: node.last,
                    blocks: vec![node.block],
                });
            } else {
                frame.append(node.block, node.last);
            }
        }
        while frames.len() > 1 {
            let last = lines.last().copied().map_or((1, 0), end);
            self.close_div(&mut frames, last)?;
        }
        let Some(mut root) = frames.pop() else {
            return Ok(Vec::new());
        };
        self.close_sections(&mut root.sections, &mut root.blocks, 0)?;
        Ok(root.blocks)
    }

    fn close_div(&mut self, frames: &mut Vec<Frame>, last: (usize, usize)) -> Result<()> {
        if let Some(mut frame) = frames.pop()
            && let Some((_, mut attr, first)) = frame.div.take()
            && let Some(parent) = frames.last_mut()
        {
            self.close_sections(&mut frame.sections, &mut frame.blocks, 0)?;
            if self.sourcepos {
                set_pos(&mut attr, first, last);
            }
            parent.append(Block::Div(attr, frame.blocks), last);
        }
        Ok(())
    }

    fn close_sections(
        &mut self,
        sections: &mut Vec<Section>,
        base: &mut Vec<Block>,
        level: i64,
    ) -> Result<()> {
        while sections
            .last()
            .is_some_and(|section| section.level >= level)
        {
            let Some(mut section) = sections.pop() else {
                break;
            };
            if section.attr.id.is_empty() {
                let id = identifier(&section.title);
                section.attr.id = if self.ids.insert(id.clone()) {
                    id
                } else {
                    let next = self.suffixes.entry(id.clone()).or_insert(1);
                    loop {
                        let unique = format!("{id}-{next}");
                        *next += 1;
                        if self.ids.insert(unique.clone()) {
                            break unique;
                        }
                    }
                };
            } else {
                self.ids.insert(section.attr.id.clone());
            }
            if !section.attr.has_class("section") {
                section.attr.classes.push("section".into());
            }
            charge(
                &mut self.budget,
                section.title.len().saturating_add(section.attr.id.len()),
            )?;
            self.headings.insert(section.title, section.attr.id.clone());
            if self.sourcepos {
                set_pos(&mut section.attr, section.first, section.last);
            }
            let block = Block::Div(section.attr, section.blocks);
            if let Some(parent) = sections.last_mut() {
                parent.last = section.last;
                parent.blocks.push(block);
            } else {
                base.push(block);
            }
        }
        Ok(())
    }

    fn one(
        &mut self,
        lines: &[Line<'s>],
        start: usize,
        depth: usize,
        mut attr: Attr,
    ) -> Result<(Positioned, usize)> {
        let line = lines[start].trim();
        let mut cursor = start + 1;
        let mut heading = None;
        let block = if let Some((width, info)) = fence(line.text, '`') {
            let mut payload = String::new();
            while let Some(body) = lines.get(cursor).copied() {
                if fence(body.trim().text, '`')
                    .is_some_and(|(n, name)| n >= width && name.is_empty())
                {
                    cursor += 1;
                    break;
                }
                payload.push_str(
                    remove_indent(body, line.column.saturating_sub(lines[start].column)).text,
                );
                if body.newline {
                    payload.push('\n');
                }
                cursor += 1;
            }
            if let Some(format) = info.strip_prefix('=') {
                Block::RawBlock(Format(format.into()), payload)
            } else {
                let classes = if info.is_empty() {
                    vec![]
                } else {
                    vec![info.into()]
                };
                Block::CodeBlock(Attr::new("", classes, vec![]), payload)
            }
        } else if let Some((level, text)) = headline(line.text) {
            let mut parts = vec![line.tail(line.text.len() - text.len())];
            while let Some(next) = lines.get(cursor).copied() {
                let next = next.trim();
                if next.text.is_empty() || block_start(next.text) {
                    break;
                }
                parts.push(next);
                cursor += 1;
            }
            heading = Some(std::mem::take(&mut attr));
            Block::Header(level, Attr::default(), self.inline_lines(&parts, depth)?)
        } else if horizontal(line.text) {
            Block::HorizontalRule
        } else if let Some(offset) = quote(line.text) {
            let mut quotes = vec![quote_line(line, offset)];
            let mut lazy = !quotes[0].text.trim().is_empty() && !block_start(quotes[0].text.trim());
            while let Some(next) = lines.get(cursor).copied() {
                let next = next.trim();
                if let Some(offset) = quote(next.text) {
                    if let Some(previous) = quotes.last_mut() {
                        previous.quote_end = Some((next.number, next.column));
                    }
                    let inside = quote_line(next, offset);
                    lazy = !inside.text.trim().is_empty() && !block_start(inside.text.trim());
                    quotes.push(inside);
                } else if lazy && !next.text.is_empty() && !block_start(next.text) {
                    if let Some(previous) = quotes.last_mut() {
                        previous.quote_end = Some((next.number, 0));
                    }
                    quotes.push(Line {
                        quote_end: Some(end(next)),
                        ..next
                    });
                } else {
                    break;
                }
                cursor += 1;
            }
            let mut chain = Vec::new();
            while !quotes.is_empty() && quotes.iter().all(|line| quote(line.trim().text).is_some())
            {
                self.check_depth(depth + 2 + chain.len())?;
                let first = quotes[0].trim();
                let last = quotes[quotes.len() - 1];
                chain.push(self.position(first, last));
                for index in 0..quotes.len() {
                    let trimmed = quotes[index].trim();
                    if let Some(offset) = quote(trimmed.text) {
                        let mut inside = quote_line(trimmed, offset);
                        if let Some(next) = quotes.get(index + 1) {
                            inside.quote_end = Some((next.number, next.trim().column));
                        }
                        quotes[index] = inside;
                    }
                }
            }
            let mut children = self.blocks(&quotes, depth + 1 + chain.len())?;
            for attr in chain.into_iter().rev() {
                children = vec![attrs::block(attr, Block::BlockQuote(children))];
            }
            Block::BlockQuote(children)
        } else if let Some(marker) = list_marker(line.text) {
            let (list, end) = self.list(lines, start, marker, depth)?;
            cursor = end;
            list
        } else if table_parts(line.text).is_some() {
            let (table, end) = self.table(lines, start, depth)?;
            cursor = end;
            Block::Table(Box::new(table))
        } else {
            let mut parts = vec![line];
            while let Some(next) = lines.get(cursor).copied() {
                let next = next.trim();
                if next.text.is_empty() || (block_start(next.text) && !horizontal(next.text)) {
                    break;
                }
                parts.push(next);
                cursor += 1;
            }
            Block::Para(self.inline_lines(&parts, depth)?)
        };
        let last_line = lines[cursor - 1];
        let positioned_end = if matches!(
            block,
            Block::Header(..)
                | Block::Para(..)
                | Block::BulletList(_)
                | Block::OrderedList(..)
                | Block::DefinitionList(_)
                | Block::Table(_)
        ) {
            line_boundary(last_line)
        } else {
            end(last_line)
        };
        if self.sourcepos {
            set_pos(&mut attr, (line.number, line.column), positioned_end);
        }
        let block = attrs::block(attr, block);
        Ok((
            Positioned {
                block,
                first: (line.number, line.column),
                last: positioned_end,
                heading,
            },
            cursor,
        ))
    }

    fn position(&self, first: Line<'_>, last: Line<'_>) -> Attr {
        let mut attr = Attr::default();
        if self.sourcepos {
            set_pos(&mut attr, (first.number, first.column), end(last));
        }
        attr
    }

    fn inline_lines(&self, lines: &[Line<'_>], depth: usize) -> Result<Vec<Inline>> {
        self.inline_content(lines, depth, false)
    }

    fn inline_content(&self, lines: &[Line<'_>], depth: usize, table: bool) -> Result<Vec<Inline>> {
        let source = lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                if index + 1 == lines.len() && !(table && self.sourcepos) {
                    line.text.trim_end()
                } else {
                    line.text
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let positions = self.sourcepos.then(|| {
            inlines::Positions::new(
                &source,
                lines
                    .iter()
                    .map(|line| (line.number, line.column, line.quote_end.is_some())),
            )
        });
        let mut nodes = inlines::parse(
            &source,
            self.context.limits.max_nesting_depth.saturating_sub(depth),
            positions.as_ref(),
        )?;
        if table && matches!(nodes.last().map(unposition), Some(Inline::Space)) {
            nodes.pop();
        }
        if self.east_asian {
            let mut kept = 0;
            for index in 0..nodes.len() {
                let join = kept > 0
                    && matches!(unposition(&nodes[index]), Inline::SoftBreak)
                    && matches!(unposition(&nodes[kept - 1]), Inline::Str(s) if s.chars().next_back().is_some_and(east_asian))
                    && matches!(nodes.get(index + 1).map(unposition), Some(Inline::Str(s)) if s.chars().next().is_some_and(east_asian));
                if !join {
                    nodes.swap(kept, index);
                    kept += 1;
                }
            }
            nodes.truncate(kept);
        }
        Ok(nodes)
    }

    fn list(
        &mut self,
        lines: &[Line<'s>],
        start: usize,
        marker: Marker,
        depth: usize,
    ) -> Result<(Block, usize)> {
        self.check_depth(depth + 1)?;
        let mut cursor = start;
        let mut items = Vec::new();
        let mut definitions = Vec::new();
        let mut loose = false;
        let base_column = lines[start].trim().column;
        while let Some(line) = lines.get(cursor).copied() {
            let line = line.trim();
            let Some(current) = list_marker(line.text)
                .filter(|m| line.column == base_column && m.same_list(marker))
            else {
                break;
            };
            let first = line.tail(current.skip);
            cursor += 1;
            let mut body = vec![first];
            let mut blank = false;
            let mut has_definition = false;
            while let Some(next) = lines.get(cursor).copied() {
                let trimmed = next.trim();
                if trimmed.text.is_empty() {
                    blank = true;
                    body.push(next);
                } else if trimmed.column > base_column {
                    if blank && marker.kind == ':' && has_definition {
                        loose = true;
                    }
                    if blank {
                        has_definition = true;
                    }
                    body.push(remove_indent(
                        next,
                        base_column
                            .saturating_add(current.skip)
                            .saturating_sub(next.column),
                    ));
                    blank = false;
                } else if !blank && !block_start(trimmed.text) {
                    body.push(trimmed);
                } else {
                    break;
                }
                cursor += 1;
            }
            let another = lines.get(cursor).is_some_and(|line| {
                line.trim().column == base_column
                    && list_marker(line.trim().text).is_some_and(|m| m.same_list(marker))
            });
            if blank && another {
                loose = true;
            }
            while body.last().is_some_and(|line| line.text.trim().is_empty()) {
                body.pop();
            }
            if marker.kind == ':' {
                let split = body
                    .iter()
                    .position(|line| line.text.trim().is_empty())
                    .unwrap_or(body.len());
                let term = self.inline_lines(&body[..split], depth + 1)?;
                let definition = self.blocks(&body[split..], depth + 1)?;
                definitions.push((term, vec![definition]));
            } else if current.task {
                if let Some(line) = body.first_mut() {
                    let checked = line.text.starts_with("[x]") || line.text.starts_with("[X]");
                    *line = line.tail(3).trim();
                    let mut item = self.blocks(&body, depth + 1)?;
                    let label = Inline::Str(if checked { "[X]" } else { "[ ]" }.into());
                    match item.first_mut() {
                        Some(Block::Para(inlines) | Block::Plain(inlines)) => {
                            inlines.splice(..0, [label, Inline::Space]).for_each(drop)
                        }
                        _ => item.insert(0, Block::Plain(vec![label])),
                    }
                    items.push(item);
                } else {
                    items.push(vec![]);
                }
            } else {
                items.push(self.list_item(&body, depth + 1)?);
            }
        }
        if marker.kind == ':' {
            if !loose {
                for (_, defs) in &mut definitions {
                    for def in defs {
                        make_plain(def);
                    }
                }
            }
            return Ok((Block::DefinitionList(definitions), cursor));
        }
        loose |= items.iter().any(|item| {
            item.windows(2)
                .any(|pair| matches!(pair, [Block::Para(_), Block::Para(_)]))
        });
        if !loose {
            for item in &mut items {
                make_plain(item);
            }
        }
        Ok((list_block(marker, items), cursor))
    }

    fn list_item(&mut self, lines: &[Line<'s>], depth: usize) -> Result<Vec<Block>> {
        let mut chain = Vec::new();
        let mut lines = lines.to_vec();
        let mut start = 0;
        let mut leading = Vec::new();
        while let Some(first) = lines.get(start).copied() {
            let first = first.trim();
            if first.text.is_empty() {
                start += 1;
                continue;
            }
            if !block_start(first.text) {
                let stop = lines[start..]
                    .iter()
                    .position(|line| line.text.trim().is_empty())
                    .map_or(lines.len(), |offset| start + offset);
                for line in &mut lines[start..stop] {
                    *line = line.trim();
                }
                let para = self.inline_lines(&lines[start..stop], depth + chain.len())?;
                let mut attr = Attr::default();
                if self.sourcepos {
                    set_pos(
                        &mut attr,
                        (first.number, first.column),
                        line_boundary(lines[stop - 1]),
                    );
                }
                leading.push(attrs::block(attr, Block::Para(para)));
                start = stop;
                break;
            }
            if horizontal(first.text) {
                break;
            }
            let Some(marker) =
                list_marker(first.text).filter(|marker| marker.kind != ':' && !marker.task)
            else {
                break;
            };
            if lines[start + 1..]
                .iter()
                .any(|line| !line.text.trim().is_empty() && line.trim().column <= first.column)
            {
                break;
            }
            self.check_depth(depth + chain.len() + 1)?;
            let last = lines.last().copied().unwrap_or(first);
            let mut attr = Attr::default();
            if self.sourcepos {
                set_pos(&mut attr, (first.number, first.column), line_boundary(last));
            }
            chain.push((marker, attr));
            lines[start] = first.tail(marker.skip);
            for line in &mut lines[start + 1..] {
                let width = (first.column + marker.skip).saturating_sub(line.column);
                *line = remove_indent(*line, width);
            }
        }
        leading.extend(self.blocks(&lines[start..], depth + chain.len())?);
        let mut blocks = leading;
        for (marker, attr) in chain.into_iter().rev() {
            if !blocks
                .windows(2)
                .any(|pair| matches!(pair, [Block::Para(_), Block::Para(_)]))
            {
                make_plain(&mut blocks);
            }
            blocks = vec![attrs::block(attr, list_block(marker, vec![blocks]))];
        }
        Ok(blocks)
    }

    fn table(&mut self, lines: &[Line<'s>], start: usize, depth: usize) -> Result<(Table, usize)> {
        let mut table = Table::default();
        let mut cursor = start;
        let mut rows: Vec<Row> = Vec::new();
        while let Some(line) = lines.get(cursor).map(|line| line.trim()) {
            let Some(parts) = table_parts(line.text) else {
                break;
            };
            self.table_cells = self
                .table_cells
                .checked_sub(parts.len())
                .ok_or_else(size_error)?;
            let alignments = parts
                .iter()
                .map(|(_, part)| alignment(part.trim()))
                .collect::<Option<Vec<_>>>();
            if let Some(alignments) = alignments {
                if table.col_specs.is_empty()
                    || table
                        .col_specs
                        .iter()
                        .all(|spec| spec.align == Alignment::AlignDefault)
                {
                    table.col_specs = alignments
                        .iter()
                        .map(|&align| ColSpec {
                            align,
                            ..ColSpec::default()
                        })
                        .collect();
                }
                if let Some(mut head) = rows.pop() {
                    for (cell, align) in head.cells.iter_mut().zip(&alignments) {
                        cell.align = *align;
                    }
                    if table.head.rows.is_empty() && rows.is_empty() && table.bodies.is_empty() {
                        table.head.rows.push(head);
                    } else {
                        if !rows.is_empty() {
                            table.bodies.push(TableBody {
                                body: std::mem::take(&mut rows),
                                ..TableBody::default()
                            });
                        }
                        table.bodies.push(TableBody {
                            head: vec![head],
                            ..TableBody::default()
                        });
                    }
                }
            } else {
                if table.col_specs.len() < parts.len() {
                    table.col_specs.resize(parts.len(), ColSpec::default());
                }
                let mut previous = 0;
                let mut column = line.column;
                let cells = parts
                    .into_iter()
                    .enumerate()
                    .map(|(index, (start, part))| {
                        let trimmed = part.trim_start();
                        let offset = start + part.len() - trimmed.len();
                        column += line.text[previous..offset].chars().count();
                        previous = offset;
                        let cell = Line {
                            text: trimmed,
                            column,
                            ..line
                        };
                        Ok(Cell {
                            align: table.col_specs[index].align,
                            content: vec![Block::Plain(self.inline_content(
                                &[cell],
                                depth + 1,
                                true,
                            )?)],
                            ..Cell::default()
                        })
                    })
                    .collect::<Result<_>>()?;
                rows.push(Row {
                    cells,
                    ..Row::default()
                });
            }
            cursor += 1;
        }
        if let Some(body) = table.bodies.last_mut().filter(|body| !body.head.is_empty()) {
            body.body = rows;
        } else {
            table.bodies.push(TableBody {
                body: rows,
                ..TableBody::default()
            });
        }
        let cap_start = cursor
            + usize::from(
                lines
                    .get(cursor)
                    .is_some_and(|line| line.text.trim().is_empty()),
            );
        if let Some(line) = lines.get(cap_start).map(|line| line.trim())
            && line.text.starts_with("^ ")
        {
            cursor = cap_start + 1;
            let mut caption = vec![line.tail(2)];
            while let Some(next) = lines.get(cursor).map(|line| line.trim()) {
                if next.text.is_empty() || block_start(next.text) {
                    break;
                }
                caption.push(next);
                cursor += 1;
            }
            table
                .caption
                .long
                .push(Block::Plain(self.inline_lines(&caption, depth + 1)?));
        }
        Ok((table, cursor))
    }

    fn resolve(&mut self, blocks: &mut Vec<Block>, depth: usize) -> Result<()> {
        let limit = self.context.limits.max_nesting_depth;
        let mut frames = vec![Expansion::new(
            std::mem::take(blocks),
            depth,
            usize::MAX,
            None,
        )?];
        let mut active = HashSet::new();
        while !frames.is_empty() {
            let Some(frame) = frames.last_mut() else {
                break;
            };
            if let Some((key, next_depth)) = frame.references.next() {
                if let Some(lines) = self.notes.get(&key)
                    && !active.contains(&key)
                {
                    let bytes = lines.iter().fold(EXPANSION_OVERHEAD, |size, line| {
                        size.saturating_add(line.text.len() + 1)
                    });
                    self.check_depth(next_depth)?;
                    charge(&mut self.budget, bytes)?;
                    let lines = lines.clone();
                    let body = self.blocks(&lines, next_depth)?;
                    active.insert(key.clone());
                    frames.push(Expansion::new(body, next_depth, limit, Some(key))?);
                    continue;
                }
                frame.notes.push(None);
            } else if let Some(frame) = frames.pop() {
                let (key, body) = self.finish_expansion(frame)?;
                if let Some(key) = key {
                    active.remove(&key);
                }
                if let Some(parent) = frames.last_mut() {
                    parent.notes.push(Some(body));
                } else {
                    *blocks = body;
                }
            }
        }
        Ok(())
    }

    fn finish_expansion(&mut self, frame: Expansion) -> Result<(Option<String>, Vec<Block>)> {
        struct Resolve<'a, 'b, 's> {
            state: &'a mut State<'b, 's>,
            status: Result<()>,
            notes: std::vec::IntoIter<Option<Vec<Block>>>,
        }
        impl VisitorMut for Resolve<'_, '_, '_> {
            fn inline(&mut self, node: &mut Inline) {
                if matches!(node, Inline::RawInline(format, _) if format.0 == inlines::NOTE) {
                    if let Some(Some(blocks)) = self.notes.next() {
                        *node = Inline::Note(blocks);
                    }
                } else if self.status.is_ok() {
                    self.status = self.state.resolve_link(node);
                }
            }

            fn inlines(&mut self, nodes: &mut Vec<Inline>) {
                nodes.retain(|node| !matches!(node, Inline::RawInline(format, _) if format.0 == inlines::NOTE));
            }
        }
        let mut visitor = Resolve {
            state: self,
            status: Ok(()),
            notes: frame.notes.into_iter(),
        };
        let mut blocks = frame.blocks;
        walk_mut(&mut blocks, &mut visitor);
        visitor.status?;
        Ok((frame.key, blocks))
    }

    fn resolve_link(&mut self, node: &mut Inline) -> Result<()> {
        match node {
            Inline::Link(attr, _, target) | Inline::Image(attr, _, target) => {
                if let Some(index) = attr
                    .attributes
                    .iter()
                    .position(|(key, _)| key == inlines::REFERENCE)
                {
                    let (_, label) = attr.attributes.remove(index);
                    if let Some((ref_attr, url)) = self.references.get(&label) {
                        let size = url
                            .len()
                            .saturating_add(ref_attr.id.len())
                            .saturating_add(ref_attr.classes.iter().map(String::len).sum::<usize>())
                            .saturating_add(
                                ref_attr
                                    .attributes
                                    .iter()
                                    .map(|(k, v)| k.len().saturating_add(v.len()))
                                    .sum::<usize>(),
                            );
                        charge(&mut self.budget, size.saturating_add(EXPANSION_OVERHEAD))?;
                        attrs::merge(attr, ref_attr.clone());
                        *target = Target::new(url.clone(), "");
                    } else if let Some(id) = self.headings.get(&label) {
                        charge(
                            &mut self.budget,
                            id.len().saturating_add(1 + EXPANSION_OVERHEAD),
                        )?;
                        target.url = format!("#{id}");
                    } else {
                        self.context.report(LogMessage::ReferenceNotFound(
                            label,
                            SourcePos::new("", 0, 0),
                        ));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn end(line: Line<'_>) -> (usize, usize) {
    (
        line.number,
        line.column + line.text.chars().count().saturating_sub(1),
    )
}

fn quote_line(line: Line<'_>, offset: usize) -> Line<'_> {
    Line {
        quote_end: Some(end(line)),
        ..line.tail(offset)
    }
}

fn line_boundary(line: Line<'_>) -> (usize, usize) {
    if let Some(end) = line.quote_end {
        end
    } else if line.newline {
        (line.number + 1, 0)
    } else {
        end(line)
    }
}

fn set_pos(attr: &mut Attr, first: (usize, usize), last: (usize, usize)) {
    attr.attributes.push((
        "data-pos".into(),
        format!("{}:{}-{}:{}", first.0, first.1, last.0, last.1),
    ));
}

fn headline(text: &str) -> Option<(i64, &str)> {
    let count = text.bytes().take_while(|b| *b == b'#').count();
    if count > 0 && text[count..].starts_with(char::is_whitespace) {
        Some((count as i64, text[count..].trim_start()))
    } else {
        None
    }
}

fn fence(text: &str, marker: char) -> Option<(usize, &str)> {
    let count = text.bytes().take_while(|b| *b == marker as u8).count();
    (count >= 3).then(|| (count, text[count..].trim()))
}

fn horizontal(text: &str) -> bool {
    let Some(marker @ ('*' | '-')) = text.chars().next() else {
        return false;
    };
    let mut count = 0;
    for c in text.chars() {
        if c == marker {
            count += 1;
        } else if !c.is_whitespace() {
            return false;
        }
    }
    count >= 3
}

fn quote(text: &str) -> Option<usize> {
    let rest = text.strip_prefix('>')?;
    if rest.is_empty() {
        Some(1)
    } else if rest.starts_with([' ', '\t']) {
        Some(2)
    } else {
        None
    }
}

fn definition(text: &str) -> Option<(&str, &str, usize)> {
    let (key, body) = text.strip_prefix('[')?.split_once("]:")?;
    if key.is_empty() || key.contains('[') {
        return None;
    }
    let body = body.trim_start();
    Some((key, body, text.len() - body.len()))
}

fn block_start(text: &str) -> bool {
    headline(text).is_some()
        || fence(text, '`').is_some()
        || fence(text, ':').is_some()
        || quote(text).is_some()
        || list_marker(text).is_some()
        || horizontal(text)
        || definition(text).is_some()
        || table_parts(text).is_some()
        || attrs::parse(text).is_some_and(|(_, rest)| rest.trim().is_empty())
}

fn remove_indent(line: Line<'_>, width: usize) -> Line<'_> {
    let bytes = line
        .text
        .bytes()
        .take(width)
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count();
    line.tail(bytes)
}

fn make_plain(blocks: &mut [Block]) {
    for block in blocks {
        if let Block::Para(nodes) = block {
            *block = Block::Plain(std::mem::take(nodes));
        }
    }
}

#[derive(Clone, Copy)]
struct Marker {
    kind: char,
    ordered: Option<ListAttributes>,
    skip: usize,
    task: bool,
}

impl Marker {
    fn same_list(self, other: Self) -> bool {
        self.kind == other.kind
            && self.task == other.task
            && self.ordered.map(|m| (m.style, m.delim)) == other.ordered.map(|m| (m.style, m.delim))
    }
}

fn list_block(marker: Marker, items: Vec<Vec<Block>>) -> Block {
    if let Some(attr) = marker.ordered {
        Block::OrderedList(attr, items)
    } else {
        Block::BulletList(items)
    }
}

fn list_marker(text: &str) -> Option<Marker> {
    let split = text.find([' ', '\t']).unwrap_or(text.len());
    let token = &text[..split];
    let skip = if split < text.len() { split + 1 } else { split };
    let body = &text[skip..];
    if let "-" | "+" | "*" | ":" = token {
        let task = token != ":"
            && ["[x]", "[X]", "[ ]"].iter().any(|mark| {
                body.strip_prefix(mark)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
            });
        return Some(Marker {
            kind: token.as_bytes()[0] as char,
            ordered: None,
            skip,
            task,
        });
    }
    let (value, delim) = if let Some(value) = token
        .strip_prefix('(')
        .and_then(|text| text.strip_suffix(')'))
    {
        (value, ListNumberDelim::TwoParens)
    } else if let Some(value) = token.strip_suffix(')') {
        (value, ListNumberDelim::OneParen)
    } else {
        (token.strip_suffix('.')?, ListNumberDelim::Period)
    };
    let (start, style) = if let Ok(number) = value.parse::<i64>() {
        (number, ListNumberStyle::Decimal)
    } else {
        let lower = value.bytes().all(|b| b.is_ascii_lowercase());
        if !lower && !value.bytes().all(|b| b.is_ascii_uppercase()) {
            return None;
        }
        if let Some(roman) = roman_value(value)
            .filter(|_| value.len() > 1 || matches!(value, "i" | "I" | "v" | "V" | "x" | "X"))
        {
            (
                roman,
                if lower {
                    ListNumberStyle::LowerRoman
                } else {
                    ListNumberStyle::UpperRoman
                },
            )
        } else if value.len() == 1 && value.as_bytes()[0].is_ascii_alphabetic() {
            (
                (value.as_bytes()[0].to_ascii_lowercase() - b'a' + 1) as i64,
                if lower {
                    ListNumberStyle::LowerAlpha
                } else {
                    ListNumberStyle::UpperAlpha
                },
            )
        } else {
            return None;
        }
    };
    Some(Marker {
        kind: '1',
        ordered: Some(ListAttributes {
            start,
            style,
            delim,
        }),
        skip,
        task: false,
    })
}

fn roman_value(text: &str) -> Option<i64> {
    let mut total = 0;
    let mut previous = 0;
    for c in text.bytes().rev() {
        let value = match c.to_ascii_lowercase() {
            b'i' => 1,
            b'v' => 5,
            b'x' => 10,
            b'l' => 50,
            b'c' => 100,
            b'd' => 500,
            b'm' => 1000,
            _ => return None,
        };
        if value >= previous {
            total += value;
        } else {
            total -= value;
        }
        previous = value;
    }
    (total > 0).then_some(total)
}

fn table_parts(text: &str) -> Option<Vec<(usize, &str)>> {
    let body = text.trim_end().strip_prefix('|')?;
    let mut out = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    let mut ticks = 0;
    let mut cursor = 0;
    while cursor < body.len() {
        let byte = body.as_bytes()[cursor];
        if escaped {
            escaped = false;
        } else if byte == b'`' {
            let count = body[cursor..].bytes().take_while(|b| *b == b'`').count();
            if ticks == 0 {
                ticks = count;
            } else if ticks == count {
                ticks = 0;
            }
            cursor += count;
            continue;
        } else if byte == b'\\' && ticks == 0 {
            escaped = true;
        } else if byte == b'|' && ticks == 0 {
            out.push((start + 1, &body[start..cursor]));
            start = cursor + 1;
        }
        cursor += 1;
    }
    if start == body.len() && !out.is_empty() {
        Some(out)
    } else {
        None
    }
}

fn alignment(text: &str) -> Option<Alignment> {
    let lines = text.trim_matches(':');
    if lines.is_empty() || !lines.bytes().all(|b| b == b'-') {
        return None;
    }
    Some(match (text.starts_with(':'), text.ends_with(':')) {
        (true, true) => Alignment::AlignCenter,
        (true, false) => Alignment::AlignLeft,
        (false, true) => Alignment::AlignRight,
        (false, false) => Alignment::AlignDefault,
    })
}

fn unposition(node: &Inline) -> &Inline {
    if let Inline::Span(attr, nodes) = node
        && attr.get("wrapper") == Some("1")
        && attr.get("data-pos").is_some()
        && let [node] = nodes.as_slice()
    {
        node
    } else {
        node
    }
}

fn east_asian(c: char) -> bool {
    matches!(c, '\u{2e80}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}' | '\u{f900}'..='\u{faff}' | '\u{20000}'..='\u{323af}')
}

fn expand_tabs(source: String, tab_stop: i64) -> String {
    if !source.contains('\t') {
        return source;
    }
    let stop = usize::try_from(tab_stop)
        .ok()
        .filter(|width| (1..=16).contains(width))
        .unwrap_or(4);
    let mut out = String::with_capacity(source.len());
    let mut column = 0;
    for c in source.chars() {
        if c == '\t' {
            let width = stop - column % stop;
            out.extend(std::iter::repeat_n(' ', width));
            column += width;
        } else {
            out.push(c);
            if c == '\n' {
                column = 0;
            } else {
                column += 1;
            }
        }
    }
    out
}
