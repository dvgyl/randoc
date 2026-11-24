use randoc_ast::{
    Alignment, Attr, Block, Cell, ColSpec, Inline, ListAttributes, ListNumberStyle, MathType, Meta,
    MetaValue, Pandoc, Row, Table, TableBody,
};
use randoc_core::shared::{IdentifierSet, auto_identifier, stringify};
use randoc_core::{Context, Error, Extension, Extensions, ReaderOptions, Result, Sources};

use crate::dialect_inline::{class, parse, pipe_fields};
use crate::scan::{attrs, tag_at};
use crate::{cell_error, depth_error};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Dialect {
    Tiki,
    TWiki,
    Vim,
}

pub(super) fn read(
    dialect: Dialect,
    sources: &Sources,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let limits = &context.limits;
    let size = sources.0.iter().try_fold(0u64, |total, source| {
        total.checked_add(source.text.len() as u64)
    });
    if size.is_none_or(|size| size > limits.max_input_size) {
        return Err(Error::Parse("Wiki input size limit exceeded".into()));
    }
    let mut parser = Parser {
        dialect,
        extensions: options.extensions,
        depth: limits.max_nesting_depth,
        cells: limits.max_table_cells,
        copied: limits.max_input_size.min(64 << 20),
        ids: IdentifierSet::new(),
        meta: Meta::default(),
        auto: true,
    };
    let mut blocks = Vec::new();
    for source in &sources.0 {
        let mut lines = Lines::new(&source.text, dialect);
        blocks.extend(parser.blocks(&mut lines)?);
    }
    Ok(Pandoc {
        meta: parser.meta,
        blocks,
    })
}

struct Lines<'a> {
    text: &'a str,
    lines: Vec<&'a str>,
    offsets: Vec<usize>,
    codes: Vec<usize>,
    pre_closes: Vec<usize>,
    tables: Vec<usize>,
    math: Vec<usize>,
    macros: Vec<usize>,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str, dialect: Dialect) -> Self {
        let lines: Vec<_> = text.lines().collect();
        let offsets: Vec<_> = lines
            .iter()
            .map(|line| line.as_ptr() as usize - text.as_ptr() as usize)
            .collect();
        let code_end = match dialect {
            Dialect::Tiki => "{CODE}",
            Dialect::TWiki => "</verbatim>",
            Dialect::Vim => "}}}",
        };
        let codes = if dialect == Dialect::Vim {
            lines
                .iter()
                .zip(&offsets)
                .filter_map(|(line, &offset)| (line.trim() == code_end).then_some(offset))
                .collect()
        } else {
            text.match_indices(code_end).map(|(at, _)| at).collect()
        };
        let tables = if dialect == Dialect::Tiki {
            text.match_indices("||").map(|(at, _)| at).collect()
        } else {
            Vec::new()
        };
        let math = if dialect == Dialect::Vim {
            lines
                .iter()
                .enumerate()
                .filter_map(|(index, line)| (line.trim() == "}}$").then_some(index))
                .collect()
        } else {
            Vec::new()
        };
        let macros = if dialect == Dialect::TWiki {
            lines
                .iter()
                .enumerate()
                .filter_map(|(index, line)| line.contains("}%").then_some(index))
                .collect()
        } else {
            Vec::new()
        };
        Self {
            text,
            lines,
            offsets,
            codes,
            pre_closes: if dialect == Dialect::TWiki {
                text.match_indices("</pre>").map(|(at, _)| at).collect()
            } else {
                Vec::new()
            },
            tables,
            math,
            macros,
        }
    }

    fn line(&self, at: usize) -> &'a str {
        self.lines.get(at).copied().unwrap_or("")
    }

    fn end_after(&self, at: usize) -> usize {
        self.offsets.partition_point(|offset| *offset <= at)
    }
}

struct Parser {
    dialect: Dialect,
    extensions: Extensions,
    depth: usize,
    cells: usize,
    copied: u64,
    ids: IdentifierSet,
    meta: Meta,
    auto: bool,
}

impl Parser {
    fn inlines(&mut self, text: &str, depth: usize, edges: bool) -> Result<Vec<Inline>> {
        self.inlines_with_tag_end(text, depth, edges, true)
    }

    fn inlines_with_tag_end(
        &mut self,
        text: &str,
        depth: usize,
        edges: bool,
        tag_at_end: bool,
    ) -> Result<Vec<Inline>> {
        let limit = self.depth.checked_sub(depth).ok_or_else(depth_error)?;
        let tail = if edges { text } else { text.trim() };
        let mut items = parse(
            tail,
            self.dialect,
            limit,
            self.auto,
            self.dialect == Dialect::TWiki && self.extensions.contains(Extension::Smart),
            tag_at_end,
            &mut self.copied,
        )?;
        if !edges {
            items.truncate(
                items
                    .iter()
                    .rposition(|node| !matches!(node, Inline::Space | Inline::SoftBreak))
                    .map_or(0, |at| at + 1),
            );
        }
        Ok(items)
    }

    fn blocks(&mut self, source: &mut Lines<'_>) -> Result<Vec<Block>> {
        let mut quotes = vec![Vec::new()];
        let mut at = 0;
        while at < source.lines.len() {
            let line = source.line(at);
            let trimmed = line.trim();
            let padded_suffix = self.dialect == Dialect::TWiki
                && line.starts_with(char::is_whitespace)
                && line.as_ptr() as usize - source.text.as_ptr() as usize != source.offsets[at];
            if self.dialect == Dialect::TWiki {
                if quotes.len() > 1
                    && let Some(tail) = line.trim_start().strip_prefix("</blockquote>")
                {
                    close_quote(&mut quotes);
                    source.lines[at] = tail;
                    continue;
                }
                if !padded_suffix && let Some((_, tail)) = twiki_open(line, "blockquote") {
                    if quotes.len() > self.depth {
                        return Err(depth_error());
                    }
                    quotes.push(Vec::new());
                    source.lines[at] = tail;
                    continue;
                }
                if !padded_suffix && matches!(trimmed, "<noautolink>" | "</noautolink>") {
                    self.auto = trimmed.starts_with("</");
                    at += 1;
                    continue;
                }
            }
            if trimmed.is_empty() || self.comment(line) || self.directive(line, quotes.len() - 1)? {
                at += 1;
                continue;
            }
            let depth = quotes.len() - 1;
            let block = if let Some((level, label)) = self.heading(line) {
                at += 1;
                self.header(level, label, line, depth)?
            } else if self.rule(line) {
                at += 1;
                Block::HorizontalRule
            } else if !padded_suffix && let Some(block) = self.special(source, &mut at, depth)? {
                block
            } else if self.marker(line).is_some() {
                self.list(source, &mut at, depth)?
            } else if self.definition(line).is_some() {
                self.definitions(source, &mut at, depth)?
            } else if self.dialect == Dialect::Vim && indent(line) >= 4 {
                let start = at;
                at += 1;
                while indent(source.line(at)) >= 4 && !source.line(at).trim().is_empty() {
                    at += 1;
                }
                let text = join_lines(&source.lines[start..at]);
                Block::BlockQuote(vec![Block::Plain(self.inlines(&text, depth + 1, false)?)])
            } else {
                let items = self.paragraph(source, &mut at, depth)?;
                if items.is_empty() {
                    continue;
                }
                Block::Para(items)
            };
            if let Some(blocks) = quotes.last_mut() {
                blocks.push(block);
            }
        }
        while quotes.len() > 1 {
            close_quote(&mut quotes);
        }
        Ok(quotes.pop().unwrap_or_default())
    }

    fn paragraph(
        &mut self,
        source: &mut Lines<'_>,
        at: &mut usize,
        depth: usize,
    ) -> Result<Vec<Inline>> {
        let mut text = String::new();
        let mut macro_open = false;
        while *at < source.lines.len() {
            let line = source.line(*at);
            if !text.is_empty() {
                if !macro_open {
                    if line.is_empty() || self.block_start(line) {
                        break;
                    }
                    if self.dialect == Dialect::Vim && line.trim().is_empty()
                        || self.comment(line)
                        || self.directive(line, depth)?
                    {
                        *at += 1;
                        continue;
                    }
                    if line.trim().is_empty() {
                        break;
                    }
                }
                text.push('\n');
            }
            if self.dialect == Dialect::TWiki {
                macro_open = (macro_open
                    || line.contains('%')
                        && line.contains('{')
                        && next_at(&source.macros, *at + 1).is_some())
                    && !line.contains("}%");
            }
            if self.dialect == Dialect::TWiki
                && depth > 0
                && let Some((body, _)) = line.split_once("</blockquote>")
            {
                text.push_str(body.trim());
                source.lines[*at] = &line[body.len()..];
                break;
            }
            text.push_str(line.trim());
            *at += 1;
        }
        self.inlines(&text, depth, false)
    }

    fn block_start(&self, line: &str) -> bool {
        self.heading(line).is_some()
            || self.rule(line)
            || self.marker(line).is_some()
            || self.definition(line).is_some()
            || match self.dialect {
                Dialect::Tiki => line.starts_with("{CODE(") || tiki_table_start(line).is_some(),
                Dialect::TWiki => {
                    let line = line.trim();
                    matches!(
                        line,
                        "<noautolink>" | "</noautolink>" | "<blockquote>" | "</blockquote>"
                    ) || line.starts_with("<verbatim")
                        || twiki_open(line, "pre").is_some()
                        || twiki_open(line, "blockquote").is_some()
                        || line.starts_with("</blockquote>")
                        || pipe_line(line)
                }
                Dialect::Vim => {
                    let line = line.trim();
                    line.starts_with("{{{") || line.starts_with("{{$") || pipe_line(line)
                }
            }
    }

    fn comment(&self, line: &str) -> bool {
        let line = line.trim();
        self.dialect == Dialect::Vim && line.starts_with("%%")
            || self.dialect == Dialect::TWiki && line.starts_with("<!--") && line.ends_with("-->")
    }

    fn directive(&mut self, line: &str, depth: usize) -> Result<bool> {
        if self.dialect != Dialect::Vim {
            return Ok(false);
        }
        if let Some((key, value)) = line
            .strip_prefix('%')
            .and_then(|s| s.split_once(char::is_whitespace))
            && matches!(key, "title" | "date")
        {
            if self.meta.get(key).is_none() {
                let items = self.inlines(value, depth, false)?;
                self.meta.insert(key, MetaValue::MetaInlines(items));
            }
            return Ok(true);
        }
        Ok(line == "%nohtml" || line.starts_with("%template "))
    }

    fn heading<'a>(&self, line: &'a str) -> Option<(i64, &'a str)> {
        let (text, marker) = match self.dialect {
            Dialect::Tiki => (line, '!'),
            Dialect::TWiki => (line.strip_prefix("---")?, '+'),
            Dialect::Vim => (line.trim(), '='),
        };
        let level = text.bytes().take_while(|&b| b == marker as u8).count();
        if !(1..=6).contains(&level) {
            return None;
        }
        let text = &text[level..];
        if self.dialect == Dialect::Vim {
            let text = text.strip_prefix([' ', '\t'])?;
            let end = text.trim_end_matches('=');
            let label = end.strip_suffix([' ', '\t'])?;
            if text.len() - end.len() != level
                || label.trim().is_empty()
                || label.ends_with([' ', '\t'])
            {
                return None;
            }
            return Some((level as i64, label.trim_start()));
        }
        Some((level as i64, text.trim()))
    }

    fn header(&mut self, level: i64, label: &str, line: &str, depth: usize) -> Result<Block> {
        let items = self.inlines(label, depth, false)?;
        let mut attr = Attr::default();
        if self.dialect == Dialect::Vim {
            attr.id = stringify(&items);
            if line.starts_with([' ', '\t']) {
                attr.classes.push("justcenter".into());
            }
        } else if self.extensions.contains(Extension::AutoIdentifiers) {
            let base = auto_identifier(&items, self.extensions);
            attr.id = self
                .ids
                .unique(if base.is_empty() { "section" } else { &base });
        }
        Ok(Block::Header(level, attr, items))
    }

    fn rule(&self, line: &str) -> bool {
        let min = if self.dialect == Dialect::TWiki { 3 } else { 4 };
        line.len() >= min && line.bytes().all(|ch| ch == b'-')
    }

    fn special(
        &mut self,
        source: &mut Lines<'_>,
        at: &mut usize,
        depth: usize,
    ) -> Result<Option<Block>> {
        let line = source.line(*at);
        match self.dialect {
            Dialect::Tiki => {
                if let Some(rest) = line.strip_prefix("{CODE(")
                    && let Some((options, body)) = rest.split_once(")}")
                {
                    let start = body.as_ptr() as usize - source.text.as_ptr() as usize;
                    if let Some(end) = next_at(&source.codes, start) {
                        let stop = source.end_after(end);
                        let closing = source.line(stop - 1);
                        if end + "{CODE}".len() != source.offsets[stop - 1] + closing.len() {
                            return Ok(None);
                        }
                        *at = stop;
                        let mut attr = attrs(options);
                        if let Some(lang) = attr.get("colors") {
                            attr.classes.push(lang.to_owned());
                        }
                        if !options.is_empty() && attr.get("ln") != Some("0") {
                            attr.classes.push("numberLines".into());
                        }
                        return Ok(Some(Block::CodeBlock(attr, source.text[start..end].into())));
                    }
                }
                if let Some(body) = tiki_table_start(line) {
                    let start = body.as_ptr() as usize - source.text.as_ptr() as usize;
                    if let Some(end) = next_at(&source.tables, start)
                        && !source.text[end..].starts_with("|||")
                    {
                        *at = source.end_after(end);
                        return self
                            .table(source.text[start..end].lines(), false, depth)
                            .map(Some);
                    }
                }
                if line.starts_with("::") && line.ends_with("::") && line.len() > 4 {
                    *at += 1;
                    let mut items = vec![Inline::Str(format!(
                        " NOT SUPPORTED: :: (centered) BEGIN: {line} :END "
                    ))];
                    let next_line = source.line(*at);
                    let next = if next_line.trim().is_empty() || self.block_start(next_line) {
                        Vec::new()
                    } else {
                        self.paragraph(source, at, depth)?
                    };
                    if !next.is_empty() {
                        items.push(Inline::Space);
                        items.extend(next);
                    }
                    return Ok(Some(Block::Para(items)));
                }
            }
            Dialect::TWiki | Dialect::Vim => {
                let trimmed = line.trim_start();
                if self.dialect == Dialect::TWiki
                    && let Some((info, body)) = twiki_open(line, "pre")
                {
                    let start = body.as_ptr() as usize - source.text.as_ptr() as usize;
                    let end = next_at(&source.pre_closes, start);
                    let finish = end.unwrap_or(source.text.len());
                    *at = source.lines.len();
                    if let Some(end) = end {
                        let stop = source.end_after(end);
                        *at = stop;
                        let closing = source.line(stop - 1);
                        let start = closing.as_ptr() as usize - source.text.as_ptr() as usize;
                        let tail = &closing[end - start + "</pre>".len()..];
                        if !tail.trim().is_empty() {
                            source.lines[stop - 1] = tail;
                            *at = stop - 1;
                        }
                    }
                    let code = source.text[start..finish].trim_matches(['\r', '\n']);
                    return Ok(Some(Block::CodeBlock(attrs(info), code.into())));
                }
                let info = if self.dialect == Dialect::TWiki {
                    trimmed
                        .strip_prefix("<verbatim")
                        .and_then(|s| s.split_once('>'))
                        .map(|(attr, body)| {
                            (attr, body.as_ptr() as usize - source.text.as_ptr() as usize)
                        })
                } else {
                    trimmed.strip_prefix("{{{").map(|info| {
                        (
                            info,
                            source
                                .offsets
                                .get(*at + 1)
                                .copied()
                                .unwrap_or(source.text.len()),
                        )
                    })
                };
                if let Some((info, start)) = info
                    && let Some(end) = next_at(&source.codes, start)
                {
                    let stop = source.end_after(end);
                    let mut attr = attrs(info.trim());
                    if self.dialect == Dialect::Vim && !info.is_empty() && !info.contains('=') {
                        attr = class(info.trim());
                    }
                    let body = source.text[start..end].trim_matches(['\r', '\n']);
                    *at = stop;
                    if self.dialect == Dialect::TWiki {
                        let closing = source.line(stop - 1);
                        let closing_start =
                            closing.as_ptr() as usize - source.text.as_ptr() as usize;
                        let tail = &closing[end - closing_start + "</verbatim>".len()..];
                        if !tail.trim().is_empty() {
                            source.lines[stop - 1] = tail;
                            *at = stop - 1;
                        }
                    }
                    return Ok(Some(Block::CodeBlock(attr, body.into())));
                }
                if self.dialect == Dialect::Vim
                    && let Some(info) = trimmed.strip_prefix("{{$")
                    && let Some(end) = next_at(&source.math, *at + 1)
                {
                    let content = join_lines(&source.lines[*at + 1..end]);
                    *at = end + 1;
                    let body = if let Some((tag, first)) =
                        info.strip_prefix('%').and_then(|s| s.rsplit_once('%'))
                    {
                        let env = match tag {
                            "align" => "aligned",
                            "multline" => "gathered",
                            tag => tag,
                        };
                        let first = if first.is_empty() {
                            String::new()
                        } else {
                            format!("{first}\n")
                        };
                        format!("\\begin{{{env}}}\n{first}{content}\n\\end{{{env}}}")
                    } else {
                        content
                    };
                    return Ok(Some(Block::Para(vec![Inline::Math(
                        MathType::DisplayMath,
                        body,
                    )])));
                }
                if pipe_line(line) {
                    let start = *at;
                    while pipe_line(source.line(*at)) {
                        *at += 1;
                    }
                    return self
                        .table(
                            source.lines[start..*at].iter().map(|line| line.trim()),
                            self.dialect == Dialect::Vim && indent(line) > 0,
                            depth,
                        )
                        .map(Some);
                }
            }
        }
        Ok(None)
    }

    fn table<'a>(
        &mut self,
        lines: impl Iterator<Item = &'a str>,
        centered: bool,
        depth: usize,
    ) -> Result<Block> {
        let tiki = self.dialect == Dialect::Tiki;
        let mut table = Table::default();
        let mut body = TableBody::default();
        for (index, line) in lines.enumerate() {
            let line = if tiki { line } else { &line[1..line.len() - 1] };
            let raw = pipe_fields(line, self.dialect);
            if index == 1
                && self.dialect == Dialect::Vim
                && raw.len() == table.col_specs.len()
                && raw
                    .iter()
                    .all(|field| !field.is_empty() && field.bytes().all(|b| b == b'-'))
            {
                table.head.rows.append(&mut body.body);
                continue;
            }
            let head = index == 0
                && self.dialect == Dialect::TWiki
                && raw.iter().all(|field| twiki_header_cell(field).is_some());
            if index == 0 {
                table.col_specs = vec![
                    ColSpec {
                        align: if head {
                            Alignment::AlignLeft
                        } else {
                            Alignment::AlignDefault
                        },
                        ..ColSpec::default()
                    };
                    raw.len()
                ];
            }
            let width = table.col_specs.len();
            let mut row = Row::default();
            for (column, field) in raw.into_iter().take(width).enumerate() {
                let field = if head {
                    let (text, align) =
                        twiki_header_cell(field).unwrap_or((field, Alignment::AlignLeft));
                    table.col_specs[column].align = align;
                    text
                } else {
                    field
                };
                let items = self.inlines_with_tag_end(
                    field,
                    depth,
                    tiki,
                    self.dialect != Dialect::Vim || field.ends_with(char::is_whitespace),
                )?;
                row.cells.push(Cell {
                    content: if items.is_empty() && !tiki {
                        Vec::new()
                    } else {
                        vec![Block::Plain(items)]
                    },
                    ..Cell::default()
                });
            }
            self.charge(width - row.cells.len())?;
            row.cells.resize_with(width, Cell::default);
            if head {
                table.head.rows.push(row);
            } else {
                body.body.push(row);
            }
        }
        if table.head.rows.is_empty() {
            let width = table.col_specs.len();
            self.charge(width)?;
            let cells = (0..width)
                .map(|_| Cell {
                    content: if tiki {
                        vec![Block::Plain(vec![Inline::Str(String::new())])]
                    } else {
                        Vec::new()
                    },
                    ..Cell::default()
                })
                .collect();
            table.head.rows.push(Row {
                cells,
                ..Row::default()
            });
        }
        table.bodies.push(body);
        let block = Block::Table(Box::new(table));
        Ok(if centered {
            Block::Div(class("center"), vec![block])
        } else {
            block
        })
    }

    fn charge(&mut self, cells: usize) -> Result<()> {
        self.cells = self.cells.checked_sub(cells).ok_or_else(cell_error)?;
        Ok(())
    }

    fn marker<'a>(&self, line: &'a str) -> Option<Marker<'a>> {
        let text = line.trim_start_matches([' ', '\t']);
        let indentation = indent(line);
        let mut kind = None;
        let (level, rest) = match self.dialect {
            Dialect::Tiki => {
                let first = *line.as_bytes().first()?;
                if !matches!(first, b'*' | b'#') {
                    return None;
                }
                let depth = line.bytes().take_while(|&b| b == first).count();
                if first == b'#' {
                    kind = Some(ListNumberStyle::DefaultStyle);
                }
                (depth, &line[depth..])
            }
            Dialect::TWiki => {
                if indentation < 3 || !indentation.is_multiple_of(3) {
                    return None;
                }
                if let Some(rest) = text.strip_prefix('*') {
                    (
                        indentation,
                        rest.strip_prefix([' ', '\t'])
                            .or_else(|| rest.is_empty().then_some(rest))?,
                    )
                } else {
                    let (prefix, rest) = text.split_once(". ")?;
                    kind = Some(match prefix {
                        "1" => ListNumberStyle::DefaultStyle,
                        "a" => ListNumberStyle::LowerAlpha,
                        "A" => ListNumberStyle::UpperAlpha,
                        "i" => ListNumberStyle::LowerRoman,
                        "I" => ListNumberStyle::UpperRoman,
                        _ => return None,
                    });
                    (indentation, rest)
                }
            }
            Dialect::Vim => {
                let (prefix, rest) = text.split_once(char::is_whitespace)?;
                match prefix {
                    "*" | "-" => {}
                    "#" => kind = Some(ListNumberStyle::DefaultStyle),
                    _ => {
                        if !vim_ordered_marker(prefix) {
                            return None;
                        }
                        kind = Some(ListNumberStyle::DefaultStyle);
                    }
                }
                (indentation, rest)
            }
        };
        Some(Marker {
            level,
            kind,
            text: rest.trim(),
        })
    }

    fn list(&mut self, source: &mut Lines<'_>, at: &mut usize, depth: usize) -> Result<Block> {
        let Some(first) = self.marker(source.line(*at)) else {
            return Ok(Block::BulletList(Vec::new()));
        };
        let mut frames: Vec<ListFrame> = Vec::new();
        let mut limit = first.level;
        let mut vim_root = first.level;
        while *at < source.lines.len() {
            let line = source.line(*at);
            if let Some(mark) = self.marker(line) {
                if mark.level < vim_root {
                    break;
                }
                let mut outdented = false;
                while frames.len() > 1
                    && frames.last().is_some_and(|frame| frame.level > mark.level)
                {
                    close_list(&mut frames);
                    outdented = true;
                }
                let vim_sibling = self.dialect == Dialect::Vim && outdented;
                if vim_sibling && frames.len() == 1 {
                    vim_root = mark.level;
                }
                let sibling = frames
                    .last()
                    .is_some_and(|frame| mark.level <= frame.level || vim_sibling);
                let new_kind = self.dialect != Dialect::Vim
                    && frames.last().is_some_and(|frame| frame.kind != mark.kind);
                if sibling && new_kind {
                    let item_depth = depth + frames.len();
                    if self.dialect == Dialect::TWiki
                        && let Some(item) =
                            frames.last_mut().and_then(|frame| frame.items.last_mut())
                    {
                        let items = self.inlines(line, item_depth, false)?;
                        append_item(item, items, self.dialect);
                        *at += 1;
                        continue;
                    }
                    if frames.len() == 1 {
                        break;
                    }
                    close_list(&mut frames);
                }
                if !sibling || new_kind {
                    if frames.len() >= self.depth.saturating_sub(depth) {
                        return Err(depth_error());
                    }
                    frames.push(ListFrame {
                        level: mark.level,
                        kind: mark.kind,
                        items: Vec::new(),
                    });
                }
                limit = frames.last().map_or(mark.level, |frame| frame.level);
                let mut text = mark.text;
                let mut status = None;
                if self.dialect == Dialect::Vim {
                    (status, text) = todo(text);
                }
                let mut content = self.inlines(text, depth + frames.len(), false)?;
                if let Some(status) = status {
                    content.insert(0, status);
                }
                if let Some(frame) = frames.last_mut() {
                    frame.items.push(if content.is_empty() {
                        Vec::new()
                    } else {
                        vec![Block::Plain(content)]
                    });
                }
                *at += 1;
                continue;
            }
            let item_depth = depth + frames.len();
            let Some(frame) = frames.last_mut() else {
                break;
            };
            let Some(item) = frame.items.last_mut() else {
                break;
            };
            if line.trim().is_empty() {
                break;
            }
            if self.dialect == Dialect::Vim && self.comment(line) {
                *at += 1;
                continue;
            }
            if self.dialect == Dialect::TWiki
                && indent(line) > limit
                && self.definition(line).is_some()
            {
                item.push(self.definitions(source, at, item_depth)?);
                continue;
            }
            if self.dialect == Dialect::Vim
                && ((line.trim_start().starts_with("{{{") || line.trim_start().starts_with("{{$"))
                    || pipe_line(line))
                && let Some(block) = self.special(source, at, item_depth)?
            {
                item.push(block);
                continue;
            }
            let continuation = match self.dialect {
                Dialect::Tiki => line.strip_prefix('+'),
                Dialect::TWiki => (indent(line) > limit
                    || indent(line) == limit && self.definition(line).is_some())
                .then_some(line),
                Dialect::Vim => (indent(line) > first.level).then_some(line),
            };
            let Some(text) = continuation else { break };
            let items = self.inlines(text, item_depth, false)?;
            append_item(item, items, self.dialect);
            *at += 1;
        }
        while frames.len() > 1 {
            close_list(&mut frames);
        }
        Ok(frames
            .pop()
            .map_or(Block::BulletList(Vec::new()), ListFrame::block))
    }

    fn definition<'a>(&self, line: &'a str) -> Option<(&'a str, &'a str)> {
        match self.dialect {
            Dialect::Tiki => line.strip_prefix(';')?.split_once(':'),
            Dialect::TWiki if indent(line) >= 3 => {
                line.trim_start().strip_prefix("$ ")?.split_once(':')
            }
            Dialect::TWiki => None,
            Dialect::Vim if indent(line) < 4 => {
                let line = line.trim_end();
                for (at, _) in line.rmatch_indices("::") {
                    let end = at + 2;
                    if end == line.len() || line[end..].starts_with(char::is_whitespace) {
                        if line[..at].ends_with(':') && line.as_bytes().get(at + 2) == Some(&b':') {
                            continue;
                        }
                        return Some((&line[..at], &line[end..]));
                    }
                }
                None
            }
            Dialect::Vim => None,
        }
    }

    fn definitions(&mut self, source: &Lines<'_>, at: &mut usize, depth: usize) -> Result<Block> {
        let mut out: Vec<(Vec<Inline>, Vec<Vec<Block>>)> = Vec::new();
        while let Some((term, value)) = self.definition(source.line(*at)) {
            let line = source.line(*at);
            let indentation = indent(line);
            let mut items = self.inlines(value, depth, false)?;
            *at += 1;
            if self.dialect != Dialect::Vim {
                loop {
                    let next = source.line(*at);
                    let tail = if self.dialect == Dialect::Tiki {
                        next.strip_prefix('+')
                    } else {
                        (indent(next) > indentation
                            && !next.trim().is_empty()
                            && self.definition(next).is_none()
                            && self.marker(next).is_none())
                        .then_some(next)
                    };
                    let Some(tail) = tail else { break };
                    items.push(Inline::Space);
                    items.extend(self.inlines(tail, depth, false)?);
                    *at += 1;
                }
            }
            let blocks = if items.is_empty() {
                Vec::new()
            } else {
                vec![Block::Plain(items)]
            };
            if self.dialect == Dialect::Vim && term.trim().is_empty() {
                if let Some((_, values)) = out.last_mut() {
                    if !blocks.is_empty() {
                        values.push(blocks);
                    }
                } else {
                    out.push((Vec::new(), vec![blocks]));
                }
            } else {
                out.push((self.inlines(term, depth, false)?, vec![blocks]));
            }
        }
        for (_, definitions) in &mut out {
            if definitions.len() > 1 && definitions.first().is_some_and(Vec::is_empty) {
                definitions.remove(0);
            }
        }
        Ok(Block::DefinitionList(out))
    }
}

fn vim_ordered_marker(text: &str) -> bool {
    let Some(marker) = text.strip_suffix(['.', ')']).filter(|s| !s.is_empty()) else {
        return false;
    };
    marker.bytes().all(|b| b.is_ascii_digit())
        || text.ends_with(')')
            && (marker.len() == 1 && marker.bytes().all(|b| b.is_ascii_alphabetic())
                || marker.bytes().all(|b| b"ivxlcdm".contains(&b))
                || marker.bytes().all(|b| b"IVXLCDM".contains(&b)))
}

fn twiki_open<'a>(line: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let line = line.trim_start();
    let body = line.strip_prefix('<')?;
    if !body.starts_with(name) {
        return None;
    }
    let tag = tag_at(line, 0)?;
    (tag.name == name && !tag.single).then_some((tag.attrs, &line[tag.end..]))
}

fn next_at(positions: &[usize], start: usize) -> Option<usize> {
    positions
        .get(positions.partition_point(|&at| at < start))
        .copied()
}

fn close_quote(quotes: &mut Vec<Vec<Block>>) {
    if let Some(blocks) = quotes.pop()
        && let Some(parent) = quotes.last_mut()
    {
        parent.push(Block::BlockQuote(blocks));
    }
}

fn indent(line: &str) -> usize {
    line.bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .fold(0, |at, ch| {
            if ch == b'\t' {
                at + 4 - (at % 4)
            } else {
                at + 1
            }
        })
}

fn join_lines(lines: &[&str]) -> String {
    lines
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join("\n")
}

fn tiki_table_start(line: &str) -> Option<&str> {
    line.strip_prefix("||")
        .filter(|body| !body.starts_with('|'))
}

fn pipe_line(line: &str) -> bool {
    let line = line.trim();
    line.len() > 1 && line.starts_with('|') && line.ends_with('|')
}

fn twiki_header_cell(text: &str) -> Option<(&str, Alignment)> {
    let lead = text.len() - text.trim_start().len();
    let tail = text.len() - text.trim_end().len();
    let align = match (lead >= 2, tail >= 2) {
        (true, true) => Alignment::AlignCenter,
        (true, false) => Alignment::AlignRight,
        _ => Alignment::AlignLeft,
    };
    let value = text.trim().strip_prefix('*')?.strip_suffix('*')?;
    (!value.is_empty()).then_some((value, align))
}

struct Marker<'a> {
    level: usize,
    kind: Option<ListNumberStyle>,
    text: &'a str,
}

struct ListFrame {
    level: usize,
    kind: Option<ListNumberStyle>,
    items: Vec<Vec<Block>>,
}

impl ListFrame {
    fn block(self) -> Block {
        if let Some(style) = self.kind {
            Block::OrderedList(
                ListAttributes {
                    style,
                    ..ListAttributes::default()
                },
                self.items,
            )
        } else {
            Block::BulletList(self.items)
        }
    }
}

fn close_list(frames: &mut Vec<ListFrame>) {
    if let Some(frame) = frames.pop()
        && let Some(item) = frames.last_mut().and_then(|parent| parent.items.last_mut())
    {
        item.push(frame.block());
    }
}

fn append_item(item: &mut Vec<Block>, items: Vec<Inline>, dialect: Dialect) {
    if let Some(Block::Plain(last)) = item.last_mut() {
        if !matches!(last.last(), Some(Inline::LineBreak)) {
            last.push(if dialect == Dialect::Vim {
                Inline::SoftBreak
            } else {
                Inline::Space
            });
        }
        last.extend(items);
    } else {
        item.push(Block::Plain(items));
    }
}

fn todo(text: &str) -> (Option<Inline>, &str) {
    for (marker, class_name) in [
        ("[ ]", "done0"),
        ("[.]", "done1"),
        ("[o]", "done2"),
        ("[O]", "done3"),
        ("[X]", "done4"),
    ] {
        if let Some(rest) = text.strip_prefix(marker)
            && (rest.is_empty() || rest.starts_with(char::is_whitespace))
        {
            return (
                Some(Inline::Span(class(class_name), Vec::new())),
                rest.trim_start(),
            );
        }
    }
    (None, text)
}
