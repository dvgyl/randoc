use randoc_ast::{
    Alignment, Attr, Block, Cell, ColSpec, Format as RawFormat, Inline, ListAttributes,
    ListNumberDelim, ListNumberStyle, Meta, MetaValue, Pandoc, Row, Table, TableBody,
};
use randoc_core::shared::{IdentifierSet, auto_identifier, default_abbreviations};
use randoc_core::{Extension, Limits, ReaderOptions, Result};

use crate::inline::{Parser, attributes};
use crate::{Format, check_depth, expansion_budget};

pub(crate) fn read(
    format: Format,
    input: &str,
    options: &ReaderOptions,
    limits: &Limits,
    t2t_paths: Option<(&str, &str)>,
) -> Result<Pandoc> {
    let mut state = State {
        inline: Parser {
            format,
            options,
            limit: limits.max_nesting_depth,
            notes: Default::default(),
            links: Default::default(),
            active: Default::default(),
            budget: expansion_budget(input.len(), limits.max_input_size),
            decode_entities: true,
            t2t_paths,
        },
        ids: IdentifierSet::new(),
        padding: limits.max_table_cells,
        meta: Meta::default(),
    };
    let mut lines: Vec<_> = input.lines().collect();
    let start = state.metadata(&lines)?;
    lines.drain(..start);
    if format == Format::T2t {
        strip_t2t_comments(&mut lines);
    }
    let amuse = options.extensions.contains(Extension::Amuse);
    let mut current = 0;
    let mut content = Vec::new();
    while let Some(&line) = lines.get(current) {
        if let Some((id, body)) = note_definition(format, line) {
            let mut end = current + 1;
            while let Some(next) = lines.get(end)
                && note_definition(format, next).is_none()
                && (format == Format::Muse && !amuse || !next.trim().is_empty())
            {
                end += 1;
            }
            let start = body.as_ptr() as usize - input.as_ptr() as usize;
            let last = lines[end - 1];
            let finish = last.as_ptr() as usize - input.as_ptr() as usize + last.len();
            if let Some(body) = input.get(start..finish) {
                state.inline.notes.insert(id, body.trim_end());
            }
            current = end;
        } else {
            if format == Format::Textile
                && let Some((id, url)) = reference(line)
            {
                state.inline.links.insert(id, url);
            } else {
                content.push(line);
            }
            current += 1;
        }
    }
    let blocks = state.blocks(&content, 0)?;
    Ok(Pandoc {
        meta: state.meta,
        blocks,
    })
}

struct State<'a> {
    inline: Parser<'a>,
    ids: IdentifierSet,
    padding: usize,
    meta: Meta,
}

impl State<'_> {
    fn metadata(&mut self, lines: &[&str]) -> Result<usize> {
        let mut start = 0;
        if self.inline.format == Format::T2t && self.inline.options.standalone {
            if lines.first().is_some_and(|line| line.is_empty()) {
                start = 1;
            } else {
                for (name, line) in ["title", "author", "date"].into_iter().zip(lines.iter()) {
                    let inlines = self.inline.parse(line, 0)?;
                    if !inlines.is_empty() {
                        let value = MetaValue::MetaInlines(inlines);
                        self.meta.insert(
                            name,
                            if name == "author" {
                                MetaValue::MetaList(vec![value])
                            } else {
                                value
                            },
                        );
                    }
                    start += 1;
                }
            }
            while let Some(line) = lines.get(start) {
                if let Some((name, value)) = line
                    .strip_prefix("%!")
                    .and_then(|text| text.split_once(':'))
                {
                    self.meta
                        .insert(name.trim(), MetaValue::MetaString(value.trim().into()));
                } else if !line.trim().is_empty() && !line.starts_with('%') {
                    break;
                }
                start += 1;
            }
        } else if self.inline.format == Format::Muse {
            for &line in lines {
                if let Some((key, value)) = line
                    .strip_prefix('#')
                    .and_then(|text| text.split_once(char::is_whitespace))
                    && key.starts_with(char::is_alphabetic)
                    && !value.trim().is_empty()
                {
                    self.meta.insert(
                        key,
                        MetaValue::MetaInlines(self.inline.parse(value.trim(), 0)?),
                    );
                } else if !line.trim().is_empty() {
                    break;
                }
                start += 1;
            }
        }
        Ok(start)
    }

    fn blocks(&mut self, lines: &[&str], depth: usize) -> Result<Vec<Block>> {
        check_depth(depth, self.inline.limit)?;
        let format = self.inline.format;
        let mut out = Vec::new();
        let mut regions = Vec::new();
        let mut last_pre_close = None;
        let mut i = 0;
        while let Some(&line) = lines.get(i) {
            let trim = line.trim();
            let depth = depth.saturating_add(regions.len());
            if trim.is_empty() {
                i += 1;
                continue;
            }
            if format == Format::T2t && line.starts_with('%') {
                i += 1;
                if trim == "%%%" {
                    while i < lines.len() && lines[i].trim_end() != "%%%" {
                        i += 1;
                    }
                    i = (i + 1).min(lines.len());
                }
                continue;
            }
            if format == Format::Muse && line.starts_with("; ") {
                i += 1;
                continue;
            }
            if format == Format::Muse {
                if let Some(close) = muse_region(trim) {
                    check_depth(depth + 1, self.inline.limit)?;
                    regions.push((close, std::mem::take(&mut out)));
                    i += 1;
                    continue;
                }
                if regions.last().is_some_and(|(close, _)| *close == trim)
                    && let Some((_, mut outer)) = regions.pop()
                {
                    outer.push(Block::BlockQuote(out));
                    out = outer;
                    i += 1;
                    continue;
                }
            }
            if format == Format::Muse
                && let Some(rest) = trim.strip_prefix("<comment>")
            {
                let (_, end) = collect_delimited(lines, i, rest, "</comment>");
                i = end.unwrap_or(lines.len());
                continue;
            }
            if horizontal_rule(format, trim) {
                out.push(Block::HorizontalRule);
                i += 1;
                continue;
            }
            if let Some((level, mut attr, title)) = heading(format, line) {
                let extensions = self.inline.options.extensions;
                i += 1;
                let xs = if format == Format::Muse && extensions.contains(Extension::Amuse) {
                    let start = i;
                    while let Some(next) = lines.get(i)
                        && !next.trim().is_empty()
                        && heading(format, next).is_none()
                    {
                        i += 1;
                    }
                    let text = std::iter::once(title.trim())
                        .chain(lines[start..i].iter().map(|line| line.trim()))
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.inline.parse(&text, depth)?
                } else {
                    self.inline.parse(title.trim(), depth)?
                };
                if matches!(format, Format::Muse | Format::Textile) {
                    if attr.id.is_empty() && extensions.contains(Extension::AutoIdentifiers) {
                        attr.id = self.ids.unique(&auto_identifier(&xs, extensions));
                    } else if !attr.id.is_empty() {
                        self.ids.register(&attr.id);
                    }
                }
                out.push(Block::Header(level, attr, xs));
                continue;
            }
            if format == Format::Textile {
                if let Some((name, attr, extended, body)) = textile_block(line) {
                    i += 1;
                    if name == "table" {
                        continue;
                    }
                    let start = i;
                    while i < lines.len()
                        && (extended || !lines[i].trim().is_empty())
                        && (!extended || textile_block(lines[i]).is_none())
                    {
                        i += 1;
                    }
                    if name == "###" {
                        continue;
                    }
                    let text = joined(body, &lines[start..i]);
                    let text = text.trim_end_matches('\n');
                    let block = match name {
                        "bc" | "pre" => Block::CodeBlock(
                            Attr::default(),
                            expand_tabs(text, self.inline.options.tab_stop),
                        ),
                        "bq" => Block::BlockQuote(vec![Block::Para(
                            self.inline.parse(text, depth + 1)?,
                        )]),
                        _ => {
                            let para = Block::Para(self.inline.parse(text, depth)?);
                            if attr.is_empty() {
                                para
                            } else {
                                Block::Div(attr, vec![para])
                            }
                        }
                    };
                    out.push(block);
                    continue;
                }
                if let Some(body) = line.strip_prefix("<pre>") {
                    let last = last_pre_close.get_or_insert_with(|| {
                        lines.iter().rposition(|line| line.contains("</pre>"))
                    });
                    if last.is_some_and(|last| last >= i) {
                        let (text, end) = collect_delimited(lines, i, body, "</pre>");
                        out.push(Block::CodeBlock(
                            Attr::default(),
                            expand_tabs(text.trim_matches('\n'), self.inline.options.tab_stop),
                        ));
                        i = end.unwrap_or(lines.len());
                    } else {
                        out.push(Block::RawBlock(RawFormat::new("html"), "<pre>".into()));
                        i += 1;
                        if !body.trim().is_empty() {
                            let start = i;
                            i = paragraph_end(format, lines, i);
                            let text = joined(body.trim(), &lines[start..i]);
                            split_textile_html(self.inline.parse(&text, depth)?, &mut out);
                        }
                    }
                    continue;
                }
                if let Some(body) = line.strip_prefix("<notextile>") {
                    let (text, end) = collect_delimited(lines, i, body, "</notextile>");
                    out.push(Block::Para(vec![Inline::Str(text)]));
                    i = end.unwrap_or(lines.len());
                    continue;
                }
                if block_html(trim)
                    && let Some(end) = line.find('>')
                {
                    out.push(Block::RawBlock(RawFormat::new("html"), line[..=end].into()));
                    if !line[end + 1..].trim().is_empty() {
                        out.push(Block::Para(self.inline.parse(&line[end + 1..], depth)?));
                    }
                    i += 1;
                    continue;
                }
            }
            if format == Format::Muse
                && let Some(tag) = ["example", "verse"].into_iter().find(|tag| {
                    trim.strip_prefix('<').and_then(|s| s.strip_suffix('>')) == Some(*tag)
                })
            {
                let close = format!("</{tag}>");
                let (text, end) = collect_delimited(lines, i, "", &close);
                let text = text.trim_matches('\n');
                let block = match tag {
                    "example" => Block::CodeBlock(Attr::default(), strip_shared_margin(text)),
                    "verse" => Block::LineBlock(
                        text.lines()
                            .map(|line| self.inline.parse(line, depth + 1))
                            .collect::<Result<_>>()?,
                    ),
                    _ => Block::Para(self.inline.parse(text, depth)?),
                };
                out.push(block);
                i = end.unwrap_or(lines.len());
                continue;
            }
            if format == Format::Muse
                && let Some((tag, rest)) = trim
                    .strip_prefix("<literal")
                    .and_then(|text| text.split_once('>'))
                && rest.is_empty()
            {
                let raw_format = tag
                    .split_once("style=\"")
                    .and_then(|(_, value)| value.split_once('"'))
                    .map_or("html", |(value, _)| value);
                let (text, end) = collect_delimited(lines, i, "", "</literal>");
                out.push(Block::RawBlock(
                    RawFormat::new(raw_format),
                    text.trim_matches('\n').into(),
                ));
                i = end.unwrap_or(lines.len());
                continue;
            }
            if format == Format::Muse && line.starts_with("> ") {
                let mut verse = Vec::new();
                while let Some(text) = lines.get(i).and_then(|line| line.strip_prefix("> ")) {
                    let spaces = text.len() - text.trim_start_matches(' ').len();
                    let text = format!("{}{}", "\u{a0}".repeat(spaces), &text[spaces..]);
                    verse.push(self.inline.parse(&text, depth)?);
                    i += 1;
                }
                out.push(Block::LineBlock(verse));
                continue;
            }
            if format == Format::T2t
                && let Some((fence, raw)) = [("```", false), ("\"\"\"", true), ("'''", true)]
                    .into_iter()
                    .find(|(fence, _)| {
                        line.strip_prefix(fence)
                            .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
                    })
            {
                let start = i;
                i += 1;
                let text = if line[3..].trim().is_empty() {
                    while i < lines.len() && lines[i].trim_end() != fence {
                        i += 1;
                    }
                    let mut text = lines[start + 1..i].join("\n");
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    i = (i + 1).min(lines.len());
                    text
                } else {
                    format!("{}\n", line[4..].trim_end())
                };
                out.push(if raw {
                    Block::Para(vec![Inline::Str(text)])
                } else {
                    Block::CodeBlock(Attr::default(), text)
                });
                continue;
            }
            if format == Format::T2t && line.starts_with('\t') {
                let (end, block) = self.t2t_quote(lines, i, depth)?;
                out.push(block);
                i = end;
                continue;
            }
            if format == Format::Haddock {
                if let Some(expr) = trim.strip_prefix(">>> ") {
                    let mut xs = vec![
                        classed_code(&["prompt"], ">>>"),
                        Inline::Space,
                        classed_code(&["haskell", "expr"], expr),
                    ];
                    i += 1;
                    while let Some(result) = lines
                        .get(i)
                        .filter(|s| !s.trim().is_empty() && !s.trim_start().starts_with(">>> "))
                    {
                        xs.push(Inline::LineBreak);
                        xs.push(classed_code(&["result"], result.trim()));
                        i += 1;
                    }
                    out.push(Block::Para(xs));
                    continue;
                }
                if trim.starts_with('>') {
                    let mut code = Vec::new();
                    while let Some(rest) =
                        lines.get(i).and_then(|s| s.trim_start().strip_prefix('>'))
                    {
                        code.push(rest.strip_prefix(' ').unwrap_or(rest));
                        i += 1;
                    }
                    out.push(Block::CodeBlock(Attr::default(), code.join("\n")));
                    continue;
                }
                if let Some(property) = trim.strip_prefix("prop>") {
                    out.push(Block::CodeBlock(
                        Attr::new("", vec!["property".into(), "haskell".into()], vec![]),
                        property.trim_start().into(),
                    ));
                    i += 1;
                    continue;
                }
                if trim == "@"
                    && let (text, Some(end)) = collect_delimited(lines, i, "", "\n@")
                {
                    let text = text.strip_prefix('\n').unwrap_or(&text);
                    let mut xs = Vec::new();
                    let mut has_markup = false;
                    for line in text.lines() {
                        let contents = self.haddock_code(line, depth)?;
                        has_markup |= !matches!(contents.as_slice(), [Inline::Code(attr, _)] if attr.is_empty());
                        let module = matches!(contents.first(), Some(Inline::Code(attr, _)) if attr.has_class("module"));
                        if !xs.is_empty() && !module {
                            xs.push(Inline::LineBreak);
                        }
                        xs.extend(contents);
                    }
                    out.push(if has_markup {
                        Block::Para(xs)
                    } else {
                        Block::CodeBlock(Attr::default(), format!("{text}\n"))
                    });
                    i = end;
                    continue;
                }
            }
            if table_start(format, line)
                && let Some((end, block)) = self.table(lines, i, depth)?
            {
                out.push(block);
                i = end;
                continue;
            }
            if definition(format, line).is_some() {
                let (end, block) = self.definitions(lines, i, depth)?;
                out.push(block);
                i = end;
                continue;
            }
            if list_marker(format, line).is_some() {
                let (end, blocks) = self.lists(lines, i, depth)?;
                out.extend(blocks);
                i = end;
                continue;
            }
            let quoted = format == Format::Muse
                && (2..6).contains(&(line.len() - line.trim_start_matches(' ').len()));
            let start = i;
            i = paragraph_end(format, lines, i + 1);
            let text = lines[start..i]
                .iter()
                .filter(|line| format != Format::T2t || !line.starts_with('%'))
                .map(|line| line.trim())
                .collect::<Vec<_>>()
                .join("\n");
            let xs = self.inline.parse(&text, depth)?;
            if format == Format::Textile {
                split_textile_html(xs, &mut out);
            } else {
                let para = if format == Format::T2t
                    && lines
                        .get(i)
                        .and_then(|line| list_marker(format, line))
                        .is_some()
                    || format == Format::Haddock
                        && matches!(xs.as_slice(), [Inline::Span(attr, children)]
                            if attr.has_class("anchor") && children.is_empty())
                {
                    Block::Plain(xs)
                } else {
                    Block::Para(xs)
                };
                if quoted {
                    out.push(Block::BlockQuote(vec![para]));
                } else {
                    out.push(para);
                }
            }
        }
        while let Some((_, mut outer)) = regions.pop() {
            outer.push(Block::BlockQuote(out));
            out = outer;
        }
        Ok(out)
    }

    fn haddock_code(&mut self, line: &str, depth: usize) -> Result<Vec<Inline>> {
        let xs = self.inline.parse(line, depth + 1)?;
        let mut out = Vec::new();
        let mut text = String::new();
        for x in xs {
            match x {
                Inline::Str(s) => text.push_str(&s),
                Inline::Space => text.push(' '),
                Inline::Emph(xs) => {
                    out.push(Inline::Code(Attr::default(), std::mem::take(&mut text)));
                    out.push(Inline::Emph(vec![Inline::Code(
                        Attr::default(),
                        randoc_core::shared::stringify(&xs),
                    )]));
                }
                other => {
                    if !text.is_empty() {
                        out.push(Inline::Code(Attr::default(), std::mem::take(&mut text)));
                    }
                    out.push(other);
                }
            }
        }
        out.push(Inline::Code(Attr::default(), text));
        Ok(out)
    }

    fn t2t_quote(&mut self, lines: &[&str], start: usize, depth: usize) -> Result<(usize, Block)> {
        let mut frames: Vec<Vec<Block>> = vec![Vec::new()];
        let mut i = start;
        while let Some(line) = lines.get(i).filter(|s| s.starts_with('\t')) {
            let tabs = line.bytes().take_while(|&c| c == b'\t').count();
            if line.trim().is_empty() {
                i += 1;
                continue;
            }
            check_depth(depth.saturating_add(tabs), self.inline.limit)?;
            while frames.len() > tabs {
                close_quote(&mut frames);
            }
            frames.resize_with(tabs, Vec::new);
            let begin = i;
            i += 1;
            while let Some(next) = lines.get(i).filter(|s| !s.trim().is_empty()) {
                if next.bytes().take_while(|&c| c == b'\t').count() != tabs {
                    break;
                }
                i += 1;
            }
            let text = lines[begin..i]
                .iter()
                .map(|s| s.trim())
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(frame) = frames.last_mut() {
                frame.push(Block::Para(self.inline.parse(&text, depth + tabs)?));
            }
        }
        while frames.len() > 1 {
            close_quote(&mut frames);
        }
        Ok((i, Block::BlockQuote(frames.pop().unwrap_or_default())))
    }

    fn definitions(
        &mut self,
        lines: &[&str],
        start: usize,
        depth: usize,
    ) -> Result<(usize, Block)> {
        let format = self.inline.format;
        let mut i = start;
        let mut items = Vec::new();
        while let Some((term, definition)) = lines.get(i).and_then(|line| definition(format, line))
        {
            let xs = self.inline.parse(term.trim(), depth + 1)?;
            i += 1;
            let begin = i;
            while i < lines.len()
                && (lines[i].starts_with(' ')
                    && !lines[i].trim().is_empty()
                    && crate::read::definition(format, lines[i]).is_none()
                    || format == Format::Textile
                        && definition.trim().is_empty()
                        && crate::read::definition(format, lines[i]).is_none())
            {
                let close = lines[i].trim_end().ends_with("=:");
                i += 1;
                if close {
                    break;
                }
            }
            let mut text = joined(definition.trim(), &lines[begin..i]);
            if definition.trim().is_empty() && format == Format::Textile && text.ends_with("=:") {
                text.truncate(text.len() - 2);
            }
            let blocks = match format {
                Format::Textile if !definition.trim().is_empty() => {
                    vec![Block::Plain(self.inline.parse(&text, depth + 1)?)]
                }
                Format::Haddock => vec![Block::Para(self.inline.parse(&text, depth + 1)?)],
                Format::Muse => self.muse_definition_body(&text, depth + 1)?,
                _ => {
                    let mut blocks = self.blocks(&text.lines().collect::<Vec<_>>(), depth + 1)?;
                    if format == Format::T2t
                        && let [Block::Para(xs)] = blocks.as_mut_slice()
                    {
                        blocks[0] = Block::Plain(std::mem::take(xs));
                    }
                    blocks
                }
            };
            items.push((xs, vec![blocks]));
            if matches!(format, Format::Haddock | Format::Muse)
                && lines.get(i).is_some_and(|s| s.trim().is_empty())
                && lines
                    .get(i + 1)
                    .and_then(|s| crate::read::definition(format, s))
                    .is_some()
            {
                i += 1;
            }
        }
        Ok((i, Block::DefinitionList(items)))
    }

    fn muse_definition_body(&mut self, text: &str, mut depth: usize) -> Result<Vec<Block>> {
        let mut lines = text.lines().collect::<Vec<_>>();
        let mut terms = Vec::new();
        while let Some((term, body)) = lines
            .first()
            .and_then(|line| definition(Format::Muse, line))
        {
            terms.push(self.inline.parse(term.trim(), depth + 1)?);
            lines[0] = body.trim();
            depth += 1;
        }
        let mut blocks = self.blocks(&lines, depth)?;
        for term in terms.into_iter().rev() {
            blocks = vec![Block::DefinitionList(vec![(term, vec![blocks])])];
        }
        Ok(blocks)
    }

    fn lists(&mut self, lines: &[&str], start: usize, depth: usize) -> Result<(usize, Vec<Block>)> {
        let mut frames: Vec<ListFrame> = Vec::new();
        let mut out = Vec::new();
        let mut i = start;
        let format = self.inline.format;
        while let Some(&line) = lines.get(i) {
            if let Some(mut marker) = list_marker(format, line) {
                while let Some(frame) = frames.last().filter(|frame| frame.indent >= marker.indent)
                {
                    if format == Format::T2t && frame.ordered == marker.ordered {
                        if frames.len() == 1 {
                            frames[0].indent = marker.indent;
                            break;
                        }
                        if frames[frames.len() - 2].indent < marker.indent {
                            marker.indent = frame.indent;
                            break;
                        }
                    }
                    if frame.indent == marker.indent && frame.ordered == marker.ordered {
                        break;
                    }
                    self.close_list(&mut frames, &mut out, depth)?;
                }
                if format == Format::T2t
                    && let Some(frame) = frames.last_mut()
                    && frame.ordered == marker.ordered
                    && marker.indent == frame.indent + 1
                {
                    frame.indent = marker.indent;
                }
                if frames
                    .last()
                    .is_some_and(|frame| frame.indent < marker.indent)
                {
                    self.finish_item(&mut frames, depth)?;
                }
                if frames
                    .last()
                    .is_none_or(|frame| frame.indent < marker.indent)
                {
                    frames.push(ListFrame {
                        indent: marker.indent,
                        ordered: marker.ordered,
                        start: marker.start,
                        items: Vec::new(),
                        current: Vec::new(),
                        text: Vec::new(),
                        loose: false,
                    });
                    check_depth(depth.saturating_add(frames.len()), self.inline.limit)?;
                } else {
                    self.finish_item(&mut frames, depth)?;
                    if let Some(frame) = frames.last_mut() {
                        frame.items.push(std::mem::take(&mut frame.current));
                    }
                }
                let item_depth = depth + frames.len();
                if let Some(frame) = frames.last_mut() {
                    if format == Format::T2t
                        && let Some(block) = self.t2t_inline_list(marker.body, item_depth)?
                    {
                        frame.current.push(block);
                    } else {
                        frame.text.push(marker.body);
                    }
                }
            } else if line.trim().is_empty() {
                let next = lines.get(i + 1);
                if matches!(format, Format::Muse | Format::T2t)
                    && next.is_some_and(|next| {
                        let indent = next.len() - next.trim_start_matches(' ').len();
                        !next.trim().is_empty()
                            && frames.last().is_some_and(|frame| indent > frame.indent)
                            && list_marker(format, next).is_none()
                    })
                {
                    self.finish_item(&mut frames, depth)?;
                    if let Some(frame) = frames.last_mut() {
                        frame.loose = true;
                    }
                } else if format == Format::Textile
                    || next.is_none_or(|s| s.trim().is_empty() || list_marker(format, s).is_none())
                {
                    break;
                } else if format == Format::T2t
                    && let Some(frame) = frames.last_mut()
                    && next
                        .and_then(|line| list_marker(format, line))
                        .is_some_and(|marker| marker.indent >= frame.indent)
                {
                    frame.loose = true;
                }
            } else if format == Format::T2t && matches!(line.trim(), "-" | "+" | ":") {
                let indent = line.len() - line.trim_start_matches(' ').len();
                if frames.len() == 1 && frames.last().is_some_and(|frame| frame.indent == indent) {
                    break;
                }
                if frames.len() > 1 && frames.last().is_some_and(|frame| frame.indent == indent) {
                    self.close_list(&mut frames, &mut out, depth)?;
                }
                if let Some(frame) = frames.last_mut() {
                    frame.text.push(line.trim());
                }
            } else if format == Format::Textile
                && let Some(first) = line.strip_prefix("<pre>")
            {
                let (code, end) = collect_delimited(lines, i, first, "</pre>");
                let Some(end) = end else { break };
                self.finish_item(&mut frames, depth)?;
                if let Some(frame) = frames.last_mut() {
                    frame.current.push(Block::CodeBlock(
                        Attr::default(),
                        expand_tabs(code.trim_matches('\n'), self.inline.options.tab_stop),
                    ));
                }
                i = end;
                continue;
            } else if paragraph_ends(format, line) || format == Format::T2t && line.starts_with('%')
            {
                break;
            } else {
                if format == Format::T2t {
                    let indent = line.len() - line.trim_start_matches(' ').len();
                    while frames.len() > 1
                        && frames.last().is_some_and(|frame| frame.indent >= indent)
                    {
                        self.close_list(&mut frames, &mut out, depth)?;
                    }
                }
                if let Some(frame) = frames.last_mut() {
                    frame.text.push(line.trim());
                }
            }
            i += 1;
        }
        while !frames.is_empty() {
            self.close_list(&mut frames, &mut out, depth)?;
        }
        Ok((i, out))
    }

    fn t2t_inline_list(&mut self, mut text: &str, depth: usize) -> Result<Option<Block>> {
        let mut lists = Vec::new();
        while let Some(marker) = list_marker(Format::T2t, text) {
            lists.push(marker.ordered);
            check_depth(depth + lists.len(), self.inline.limit)?;
            text = marker.body;
        }
        let mut item = if let Some((term, _)) = definition(Format::T2t, text) {
            vec![Block::DefinitionList(vec![(
                self.inline.parse(term, depth + lists.len() + 1)?,
                vec![vec![]],
            )])]
        } else if lists.is_empty() {
            return Ok(None);
        } else {
            vec![Block::Plain(self.inline.parse(text, depth + lists.len())?)]
        };
        for ordered in lists.into_iter().rev() {
            item = vec![if ordered {
                Block::OrderedList(ListAttributes::default(), vec![item])
            } else {
                Block::BulletList(vec![item])
            }];
        }
        Ok(item.pop())
    }

    fn finish_item(&mut self, frames: &mut [ListFrame<'_>], depth: usize) -> Result<()> {
        let level = depth + frames.len();
        if let Some(frame) = frames.last_mut()
            && !frame.text.is_empty()
        {
            let mut xs = self.inline.parse(&frame.text.join("\n"), level)?;
            if self.inline.format == Format::Textile
                && frame.text.last().is_some_and(|text| text.ends_with(' '))
            {
                xs.push(Inline::Space);
            }
            frame.text.clear();
            frame.current.push(
                if matches!(self.inline.format, Format::Muse | Format::Haddock) {
                    Block::Para(xs)
                } else {
                    Block::Plain(xs)
                },
            );
        }
        Ok(())
    }

    fn close_list(
        &mut self,
        frames: &mut Vec<ListFrame<'_>>,
        out: &mut Vec<Block>,
        depth: usize,
    ) -> Result<()> {
        self.finish_item(frames, depth)?;
        if let Some(mut frame) = frames.pop() {
            frame.items.push(frame.current);
            if frame.loose {
                for block in frame.items.iter_mut().flatten() {
                    if let Block::Plain(xs) = block {
                        *block = Block::Para(std::mem::take(xs));
                    }
                }
            }
            let block = if frame.ordered {
                let mut attr = ListAttributes {
                    start: frame.start,
                    ..Default::default()
                };
                if self.inline.format == Format::Muse {
                    attr.style = ListNumberStyle::Decimal;
                    attr.delim = ListNumberDelim::Period;
                }
                Block::OrderedList(attr, frame.items)
            } else {
                Block::BulletList(frame.items)
            };
            frames
                .last_mut()
                .map_or(out, |parent| &mut parent.current)
                .push(block);
        }
        Ok(())
    }

    fn table(
        &mut self,
        lines: &[&str],
        start: usize,
        depth: usize,
    ) -> Result<Option<(usize, Block)>> {
        let format = self.inline.format;
        let mut i = start;
        let mut rows = Vec::new();
        let mut cols = Vec::<ColSpec>::new();
        let mut caption = None;
        let mut header = false;
        let mut width = 0;
        let textile_rows = if format == Format::Textile {
            let Some(rows) = textile_rows(lines, start) else {
                return Ok(None);
            };
            rows
        } else {
            Vec::new()
        };
        while let Some(&line) = lines.get(i) {
            if let Some(text) = muse_caption(line).filter(|_| format == Format::Muse) {
                caption = Some(Block::Plain(self.inline.parse(text, depth + 1)?));
                i += 1;
                continue;
            }
            if format == Format::Textile && rows.len() == textile_rows.len() {
                break;
            }
            let mut multiline = None;
            let first = rows.is_empty();
            let row_index = rows.len();
            let end = textile_rows.get(row_index).copied().unwrap_or(i + 1);
            if end > i + 1 {
                multiline = Some(lines[i..end].join("\n"));
            }
            let Some((text, heading, separator)) =
                table_row(format, multiline.as_deref().unwrap_or(line))
            else {
                break;
            };
            if heading && !first && format == Format::T2t {
                break;
            }
            let mut row = Row::default();
            let fields = if format == Format::T2t {
                let Some(fields) = t2t_fields(text) else {
                    break;
                };
                fields
            } else {
                text.split(separator).collect()
            };
            i = end;
            for (col, field) in fields.into_iter().enumerate() {
                let (align, value) = match format {
                    Format::Textile => {
                        let (heading, align, value) = textile_field(field.trim());
                        header |= heading && first;
                        (align, value)
                    }
                    Format::T2t => {
                        let left = field.len() - field.trim_start_matches(' ').len();
                        let right = field.len() - field.trim_end_matches(' ').len();
                        let align = match left.cmp(&right) {
                            std::cmp::Ordering::Less => Alignment::AlignLeft,
                            std::cmp::Ordering::Greater => Alignment::AlignRight,
                            std::cmp::Ordering::Equal => Alignment::AlignCenter,
                        };
                        (align, field.trim())
                    }
                    _ => (Alignment::AlignDefault, field.trim()),
                };
                if !(format == Format::T2t && heading) {
                    if let Some(spec) = cols.get_mut(col) {
                        if format == Format::T2t && spec.align != align {
                            spec.align = Alignment::AlignDefault;
                        }
                    } else {
                        cols.push(ColSpec {
                            align: if first || format == Format::T2t {
                                align
                            } else {
                                Alignment::AlignDefault
                            },
                            ..Default::default()
                        });
                    }
                }
                let blocks = if value.is_empty() {
                    vec![]
                } else if format == Format::Textile && value.contains('\n') {
                    let mut blocks = self.blocks(&value.lines().collect::<Vec<_>>(), depth + 1)?;
                    if let [Block::Para(xs)] = blocks.as_mut_slice() {
                        blocks[0] = Block::Plain(std::mem::take(xs));
                    }
                    blocks
                } else {
                    vec![Block::Plain(self.inline.parse(value, depth + 1)?)]
                };
                row.cells.push(Cell {
                    content: blocks,
                    ..Default::default()
                });
            }
            header |= heading && first;
            width = width.max(row.cells.len());
            rows.push(row);
            if format == Format::T2t {
                while lines.get(i).is_some_and(|s| s.starts_with('%')) {
                    i += 1;
                }
            }
        }
        if format == Format::T2t && header && rows.len() == 1 {
            return Ok(None);
        }
        cols.resize(width, ColSpec::default());
        for row in &mut rows {
            self.padding = self
                .padding
                .checked_sub(width - row.cells.len())
                .ok_or_else(|| {
                    randoc_core::Error::App("The table needs too many padding cells.".into())
                })?;
            row.cells.resize_with(width, Cell::default);
        }
        if rows.is_empty() && caption.is_none() {
            return Ok(None);
        }
        let mut table = Table {
            col_specs: cols,
            ..Default::default()
        };
        table.caption.long.extend(caption);
        if header && !rows.is_empty() {
            table.head.rows.push(rows.remove(0));
        }
        table.bodies.push(TableBody {
            body: rows,
            ..Default::default()
        });
        Ok(Some((i, Block::Table(Box::new(table)))))
    }
}

fn strip_shared_margin(text: &str) -> String {
    let margin = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);
    text.lines()
        .map(|line| line.get(margin..).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn close_quote(frames: &mut Vec<Vec<Block>>) {
    if let Some(frame) = frames.pop()
        && let Some(parent) = frames.last_mut()
    {
        parent.push(Block::BlockQuote(frame));
    }
}

fn joined(first: &str, other: &[&str]) -> String {
    if other.is_empty() {
        first.into()
    } else if first.is_empty() {
        other.join("\n")
    } else {
        format!("{first}\n{}", other.join("\n"))
    }
}

fn collect_delimited(
    lines: &[&str],
    start: usize,
    first: &str,
    close: &str,
) -> (String, Option<usize>) {
    let mut out = String::new();
    for (offset, line) in std::iter::once(first)
        .chain(lines[start + 1..].iter().copied())
        .enumerate()
    {
        if close == "\n@" && offset > 0 && line.trim() == "@" {
            return (out, Some(start + offset + 1));
        }
        if let Some((body, _)) = line.split_once(close) {
            if offset > 0 {
                out.push('\n');
            }
            out.push_str(body);
            return (out, Some(start + offset + 1));
        }
        if offset > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    (out, None)
}

fn expand_tabs(text: &str, tab_stop: i64) -> String {
    let tab_stop = usize::try_from(tab_stop).unwrap_or(4).clamp(1, 32);
    let mut out = String::new();
    let mut column = 0;
    for c in text.chars() {
        if c == '\t' {
            let spaces = tab_stop - column % tab_stop;
            out.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else {
            out.push(c);
            column = if c == '\n' { 0 } else { column + 1 };
        }
    }
    out
}

fn strip_t2t_comments(lines: &mut Vec<&str>) {
    let mut comment = false;
    let mut fence = None;
    lines.retain_mut(|line| {
        let trimmed = line.trim_end();
        if let Some(current) = fence {
            if trimmed == current {
                fence = None;
            }
            return true;
        }
        if trimmed == "%%%" {
            comment = !comment;
            *line = "%";
            return comment;
        }
        if comment {
            return false;
        }
        if ["```", "\"\"\"", "'''"].contains(&trimmed) {
            fence = Some(trimmed);
        }
        true
    });
}

fn note_definition(format: Format, line: &str) -> Option<(&str, &str)> {
    let (id, body) = match format {
        Format::Textile => {
            let (id, body) = line.strip_prefix("fn")?.split_once('.')?;
            (id.trim_end_matches('^'), body)
        }
        Format::Muse => line.strip_prefix('[')?.split_once(']')?,
        _ => return None,
    };
    (!id.is_empty()
        && id.bytes().all(|c| c.is_ascii_digit())
        && body.starts_with(char::is_whitespace))
    .then_some((id, body.trim_start()))
}

fn reference(line: &str) -> Option<(&str, &str)> {
    let (id, url) = line.strip_prefix('[')?.split_once(']')?;
    let url = url.trim_end();
    if !id.is_empty()
        && id.chars().all(|c| c.is_alphanumeric() || "-_".contains(c))
        && !url.contains(char::is_whitespace)
        && url.starts_with(|c: char| c.is_alphanumeric() || "/#".contains(c))
    {
        Some((id, url))
    } else {
        None
    }
}

fn heading(format: Format, line: &str) -> Option<(i64, Attr, &str)> {
    match format {
        Format::Textile => {
            let rest = line.strip_prefix('h')?;
            let level = rest.bytes().next()?.checked_sub(b'0')?;
            if !(1..=6).contains(&level) {
                return None;
            }
            let (attr, rest) = attributes(&rest[1..]);
            let text = rest.strip_prefix('.')?;
            text.starts_with(char::is_whitespace)
                .then_some((i64::from(level), attr, text))
        }
        Format::T2t => {
            let line = line.trim();
            let marker = line.chars().next()?;
            if !matches!(marker, '=' | '+') {
                return None;
            }
            let count = line.bytes().take_while(|&c| c == marker as u8).count();
            if count > 5 {
                return None;
            }
            let (text, anchor) =
                if let Some((text, id)) = line.strip_suffix(']').and_then(|s| s.rsplit_once('[')) {
                    if id.chars().all(|c| c.is_alphanumeric() || "_-".contains(c)) {
                        (text, id)
                    } else {
                        return None;
                    }
                } else {
                    (line, "")
                };
            let body = text.get(count..)?.strip_suffix(&line[..count])?.trim();
            if body.is_empty() || body.starts_with(marker) || body.ends_with(marker) {
                return None;
            }
            Some((count as i64, Attr::new(anchor, vec![], vec![]), body))
        }
        Format::Muse | Format::Haddock => {
            let marker = if format == Format::Muse { b'*' } else { b'=' };
            let count = line.bytes().take_while(|&c| c == marker).count();
            if count == 0 || count > 6 || !line[count..].starts_with(char::is_whitespace) {
                return None;
            }
            Some((count as i64, Attr::default(), &line[count..]))
        }
        _ => None,
    }
}

fn horizontal_rule(format: Format, line: &str) -> bool {
    match format {
        Format::Textile | Format::Muse => {
            line.len() >= if format == Format::Textile { 3 } else { 4 }
                && line.bytes().all(|c| c == b'-')
        }
        Format::T2t => line.len() >= 20 && line.bytes().all(|c| matches!(c, b'-' | b'_' | b'=')),
        Format::Haddock => line.len() >= 3 && line.bytes().all(|c| c == b'_'),
        _ => false,
    }
}

fn textile_block(line: &str) -> Option<(&str, Attr, bool, &str)> {
    for name in ["p", "bc", "pre", "bq", "###", "table"] {
        if let Some(rest) = line.strip_prefix(name) {
            let (attr, tail) = attributes(rest);
            if matches!(name, "bc" | "pre") && !attr.is_empty() {
                continue;
            }
            if let Some(body) = tail.strip_prefix('.') {
                let (extended, body) = body.strip_prefix('.').map_or((false, body), |s| (true, s));
                if extended && !matches!(name, "bc" | "pre" | "p") {
                    continue;
                }
                if body.is_empty() || body.starts_with(char::is_whitespace) {
                    return Some((name, attr, extended, body.trim_start()));
                }
            }
        }
    }
    None
}

fn block_html(line: &str) -> bool {
    line.strip_prefix('<')
        .map(|s| s.trim_start_matches('/'))
        .is_some_and(|s| {
            let name = s.split([' ', '>', '\t']).next().unwrap_or("");
            matches!(
                name,
                "div"
                    | "p"
                    | "section"
                    | "table"
                    | "ul"
                    | "ol"
                    | "blockquote"
                    | "hr"
                    | "script"
                    | "style"
            )
        })
}

fn split_textile_html(xs: Vec<Inline>, out: &mut Vec<Block>) {
    let mut para = Vec::new();
    for x in xs {
        if let Inline::RawInline(tag, raw) = &x
            && block_html(raw)
        {
            while matches!(para.last(), Some(Inline::Space | Inline::LineBreak)) {
                para.pop();
            }
            if !para.is_empty() {
                out.push(Block::Para(std::mem::take(&mut para)));
            }
            out.push(Block::RawBlock(tag.clone(), raw.clone()));
        } else if !para.is_empty() || !matches!(x, Inline::Space | Inline::LineBreak) {
            para.push(x);
        }
    }
    if !para.is_empty() {
        out.push(Block::Para(para));
    }
}

fn paragraph_end(format: Format, lines: &[&str], mut i: usize) -> usize {
    while let Some(line) = lines.get(i) {
        if line.trim().is_empty() || paragraph_ends(format, line) {
            break;
        }
        i += 1;
    }
    i
}

fn paragraph_ends(format: Format, line: &str) -> bool {
    match format {
        Format::Textile => {
            list_marker(format, line).is_some()
                || definition(format, line).is_some()
                || line.starts_with("<pre>")
                || block_html(line.trim())
        }
        Format::T2t => {
            horizontal_rule(format, line.trim())
                || line.starts_with('\t')
                || line.starts_with("```")
                || line.starts_with("\"\"\"")
                || heading(format, line).is_some()
                || table_start(format, line)
                || list_marker(format, line).is_some()
        }
        Format::Muse => {
            heading(format, line).is_some()
                || table_start(format, line)
                || list_marker(format, line).is_some()
                || line.starts_with("<example>")
                || muse_region(line.trim()).is_some()
                || matches!(line.trim(), "</quote>" | "</center>" | "</right>")
                || line.starts_with("> ")
        }
        _ => false,
    }
}

struct ListMarker<'a> {
    indent: usize,
    ordered: bool,
    start: i64,
    body: &'a str,
}
struct ListFrame<'a> {
    indent: usize,
    ordered: bool,
    start: i64,
    items: Vec<Vec<Block>>,
    current: Vec<Block>,
    text: Vec<&'a str>,
    loose: bool,
}

fn list_marker(format: Format, line: &str) -> Option<ListMarker<'_>> {
    let trim = line.trim_start_matches(' ');
    let indent = line.len() - trim.len();
    let marker = trim.as_bytes().first().copied()?;
    if format == Format::Textile {
        if indent != 0 || !matches!(marker, b'*' | b'#') {
            return None;
        }
        let indent = trim.bytes().take_while(|&c| c == marker).count();
        let (start, tail) =
            if marker == b'#' && trim[indent..].starts_with(|c: char| c.is_ascii_digit()) {
                let (start, tail) = trim[indent..].split_once(' ')?;
                (start.parse().ok()?, tail)
            } else {
                (1, trim[indent..].strip_prefix(" ")?)
            };
        return Some(ListMarker {
            indent,
            ordered: marker == b'#',
            start,
            body: tail.trim_start(),
        });
    }
    if format == Format::Muse && indent == 0 {
        return None;
    }
    if let Some(body) = match format {
        Format::T2t | Format::Muse => trim.strip_prefix("- "),
        Format::Haddock => trim.strip_prefix("* ").or_else(|| trim.strip_prefix("- ")),
        _ => None,
    } {
        if format == Format::T2t && body.starts_with(char::is_whitespace) {
            return None;
        }
        return Some(ListMarker {
            indent,
            ordered: false,
            start: 1,
            body: body.trim_start(),
        });
    }
    let (number, body) = if format == Format::T2t {
        ("1", trim.strip_prefix("+ ")?)
    } else if format == Format::Haddock && trim.starts_with('(') {
        trim[1..].split_once(") ")?
    } else {
        trim.split_once(". ")?
    };
    let start = number.parse().ok()?;
    Some(ListMarker {
        indent,
        ordered: true,
        start,
        body: body.trim_start(),
    })
}

fn definition(format: Format, line: &str) -> Option<(&str, &str)> {
    match format {
        Format::Textile => line.strip_prefix("- ")?.split_once(" :="),
        Format::Muse => line.trim_start().split_once(" :: "),
        Format::Haddock => line.trim_start().strip_prefix('[')?.split_once("] "),
        Format::T2t => Some((line.trim_start().strip_prefix(": ")?, "")),
        _ => None,
    }
}

fn textile_rows(lines: &[&str], start: usize) -> Option<Vec<usize>> {
    let mut ends = Vec::new();
    let mut i = start;
    while let Some(line) = lines.get(i).filter(|line| !line.trim().is_empty()) {
        let start = line.trim_start().strip_prefix('|')?;
        if start.trim_start().starts_with("^.") || start.trim_start().starts_with("~.") {
            return None;
        }
        while !lines.get(i)?.trim_end().ends_with('|') || lines[i].trim() == "|" {
            i += 1;
            if lines.get(i)?.trim().is_empty() {
                return None;
            }
        }
        i += 1;
        ends.push(i);
    }
    Some(ends)
}

fn textile_field(field: &str) -> (bool, Alignment, &str) {
    let (header, value) = field
        .strip_prefix('_')
        .map_or((false, field), |rest| (true, rest));
    let (align, value) = match value.as_bytes().first() {
        Some(b'>') => (Alignment::AlignRight, &value[1..]),
        Some(b'<') => (Alignment::AlignLeft, &value[1..]),
        Some(b'=') => (Alignment::AlignCenter, &value[1..]),
        _ => (Alignment::AlignDefault, value),
    };
    let mut rest = value;
    loop {
        let suffix = match rest.as_bytes().first() {
            Some(b'{') => rest.split_once('}').map(|(_, rest)| rest),
            Some(b'(') => rest.split_once(')').map(|(_, rest)| rest),
            Some(b'\\' | b'/') => {
                let digits = rest[1..].bytes().take_while(u8::is_ascii_digit).count();
                (digits != 0).then(|| &rest[1 + digits..])
            }
            _ => None,
        };
        let Some(suffix) = suffix else { break };
        rest = suffix;
    }
    rest.strip_prefix('.')
        .filter(|text| {
            (header || align != Alignment::AlignDefault || rest != field)
                && (text.is_empty() || text.starts_with(char::is_whitespace))
        })
        .map_or((false, Alignment::AlignDefault, field), |text| {
            (header, align, text.trim_start())
        })
}

fn muse_caption(line: &str) -> Option<&str> {
    Some(line.trim().strip_prefix("|+ ")?.strip_suffix(" +|")?.trim())
}

fn t2t_fields(mut text: &str) -> Option<Vec<&str>> {
    let mut fields = Vec::new();
    while let Some(pipe) = text.find(" |") {
        let (field, tail) = text.split_at(pipe + 1);
        fields.push(field);
        text = tail.trim_start_matches('|');
        if text.is_empty() {
            return Some(fields);
        }
        if !text.starts_with(' ') {
            return None;
        }
    }
    fields.push(text);
    Some(fields)
}

fn table_start(format: Format, line: &str) -> bool {
    if format == Format::Textile {
        line.trim_start().starts_with('|')
    } else if format == Format::Muse {
        muse_caption(line).is_some() || table_row(format, line).is_some()
    } else if format == Format::T2t {
        table_row(format, line).is_some_and(|(text, _, _)| t2t_fields(text).is_some())
    } else {
        false
    }
}

fn table_row(format: Format, line: &str) -> Option<(&str, bool, &'static str)> {
    match format {
        Format::Textile => Some((
            line.trim().strip_prefix('|')?.strip_suffix('|')?,
            false,
            "|",
        )),
        Format::Muse => {
            let line = line.trim();
            if line.starts_with("| ") {
                Some((line, false, " |"))
            } else if line.contains(" ||| ") {
                Some((line, false, " ||| "))
            } else if line.contains(" || ") {
                Some((line, true, " || "))
            } else if line.contains(" | ") {
                Some((line, false, " | "))
            } else {
                None
            }
        }
        Format::T2t => {
            let line = line.trim_start_matches(' ');
            let (line, header) = if line.starts_with("|| ") {
                (&line[2..], true)
            } else {
                (line.strip_prefix('|')?, false)
            };
            let trimmed = line.trim_end_matches(' ');
            let text = if trimmed.ends_with('|') {
                trimmed
            } else {
                line
            };
            line.starts_with(' ').then_some((text, header, "|"))
        }
        _ => None,
    }
}

fn classed_code(classes: &[&str], text: &str) -> Inline {
    Inline::Code(
        Attr::new("", classes.iter().map(|s| (*s).into()).collect(), vec![]),
        text.into(),
    )
}

pub(crate) fn is_abbreviation(word: &str, options: &ReaderOptions) -> bool {
    options.abbreviations.contains(word) || default_abbreviations().contains(&word)
}

fn muse_region(line: &str) -> Option<&'static str> {
    match line {
        "<quote>" => Some("</quote>"),
        "<center>" => Some("</center>"),
        "<right>" => Some("</right>"),
        _ => None,
    }
}
