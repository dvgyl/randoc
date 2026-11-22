use std::collections::HashMap;

use randoc_ast::{Attr, Block, Caption, Format, Inline, ListAttributes, MetaValue};
use randoc_core::shared::auto_identifier;
use randoc_core::{Extension, Result};

use super::Parser;
use super::directives::{Affiliated, attributes, directive, words};
use super::lists::marker;
use crate::shared::code_line;

pub(super) fn heading(line: &str) -> Option<(i64, &str)> {
    let title = line.trim_start_matches('*');
    let level = line.len() - title.len();
    (level > 0 && title.starts_with([' ', '\t'])).then(|| (level as i64, title.trim()))
}

pub(super) fn heading_tags(title: &str) -> (&str, Vec<&str>) {
    let Some((text, tags)) = title
        .rsplit_once(char::is_whitespace)
        .or_else(|| title.starts_with(':').then_some(("", title)))
    else {
        return (title, vec![]);
    };
    let Some(tags) = tags
        .strip_prefix(':')
        .and_then(|tags| tags.strip_suffix(':'))
    else {
        return (title, vec![]);
    };
    let tags = tags.split(':').collect::<Vec<_>>();
    if tags.iter().all(|tag| {
        !tag.is_empty()
            && tag
                .chars()
                .all(|c| c.is_alphanumeric() || "_@#%".contains(c))
    }) {
        (text.trim_end(), tags)
    } else {
        (title, vec![])
    }
}

pub(super) fn block_mark<'a>(line: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let line = line.trim_start();
    if !line.get(..prefix.len())?.eq_ignore_ascii_case(prefix) {
        return None;
    }
    let tail = line.get(prefix.len()..)?;
    let (name, args) = tail.split_once(char::is_whitespace).unwrap_or((tail, ""));
    (!name.is_empty()).then(|| (name, args.trim()))
}

pub(super) fn note_definition(line: &str) -> Option<(&str, &str)> {
    let (label, body) = line.strip_prefix("[fn:")?.split_once(']')?;
    (!label.is_empty()
        && label
            .chars()
            .all(|c| c.is_alphanumeric() || "-_".contains(c))
        && (body.is_empty() || body.starts_with(char::is_whitespace)))
    .then(|| (label, body.trim_start()))
}

fn drawer(line: &str) -> Option<&str> {
    let name = line.trim().strip_prefix(':')?.strip_suffix(':')?;
    (!name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || "-_".contains(c)))
    .then_some(name)
}

pub(super) fn fixed(line: &str) -> Option<&str> {
    let text = line.trim_start().strip_prefix(':')?;
    if text.is_empty() {
        Some(text)
    } else {
        text.strip_prefix(' ')
    }
}

fn rule(line: &str) -> bool {
    let line = line.trim();
    line.len() >= 5 && line.bytes().all(|b| b == b'-')
}

pub(super) fn comment(line: &str) -> bool {
    line.trim_start()
        .strip_prefix('#')
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
}

fn latex_env(line: &str) -> Option<&str> {
    latex_name(line.trim_start().strip_prefix("\\begin{")?)
}

fn latex_name(text: &str) -> Option<&str> {
    let len = text
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || b"*-_".contains(byte))
        .count();
    (len > 0 && text.as_bytes().get(len) == Some(&b'}')).then(|| &text[..len])
}

fn planning_entries(mut line: &str) -> Vec<(&str, &str)> {
    let mut entries = Vec::new();
    while let Some((key, rest)) = line.trim_start().split_once(char::is_whitespace) {
        let rest = rest.trim_start();
        let end = match (key, rest.as_bytes().first()) {
            ("SCHEDULED:" | "DEADLINE:" | "CLOSED:", Some(b'<')) => rest.find('>'),
            ("SCHEDULED:" | "DEADLINE:" | "CLOSED:", Some(b'[')) => rest.find(']'),
            _ => None,
        };
        let Some(end) = end else { break };
        if !rest.get(1..11).is_some_and(|date| {
            date.bytes().enumerate().all(|(i, byte)| {
                if matches!(i, 4 | 7) {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit()
                }
            })
        }) {
            break;
        }
        entries.push((key, &rest[..=end]));
        line = &rest[end + 1..];
    }
    entries.sort_by_key(|(key, _)| *key);
    entries
}

pub(super) struct Boundaries {
    ends: HashMap<usize, usize>,
    drawers: Vec<Option<usize>>,
    latex: HashMap<usize, usize>,
}

impl Boundaries {
    pub(super) fn new(lines: &[&str]) -> Self {
        let mut next_end = HashMap::new();
        let mut ends = HashMap::new();
        let mut drawers = vec![None; lines.len()];
        let mut end_drawer = None;
        let mut latex = HashMap::new();
        let mut latex_ends = HashMap::new();
        for (index, line) in lines.iter().enumerate().rev() {
            if let Some((name, _)) = block_mark(line, "#+end_") {
                next_end.insert(name.to_ascii_lowercase(), index);
            } else if let Some((name, args)) = block_mark(line, "#+begin_")
                && (!name.eq_ignore_ascii_case("export") || !args.is_empty())
                && let Some(&end) = next_end.get(&name.to_ascii_lowercase())
            {
                ends.insert(index, end);
            }
            for (offset, marker) in line.match_indices("\\end{") {
                if let Some(env) = latex_name(&line[offset + marker.len()..]) {
                    latex_ends.insert(env, index);
                }
            }
            if let Some(env) = latex_env(line)
                && let Some(&end) = latex_ends.get(env)
            {
                latex.insert(index, end);
            }
            if let Some(name) = drawer(line) {
                if name.eq_ignore_ascii_case("end") {
                    end_drawer = Some(index);
                } else {
                    drawers[index] = end_drawer;
                }
            }
            if heading(line).is_some() {
                end_drawer = None;
            }
        }
        Self {
            ends,
            drawers,
            latex,
        }
    }

    pub(super) fn block_end(&self, index: usize) -> Option<usize> {
        self.ends.get(&index).copied()
    }

    pub(super) fn opaque_end(&self, index: usize) -> Option<usize> {
        self.block_end(index)
            .or(self.drawers[index])
            .or_else(|| self.latex.get(&index).copied())
    }

    pub(super) fn headings<'a>(
        &self,
        lines: &'a [&'a str],
    ) -> impl Iterator<Item = (usize, i64, &'a str)> {
        let mut index = 0;
        std::iter::from_fn(move || {
            while let Some(line) = lines.get(index) {
                let position = index;
                index = self.opaque_end(index).unwrap_or(index) + 1;
                if let Some((level, title)) = heading(line) {
                    return Some((position, level, title));
                }
            }
            None
        })
    }

    pub(super) fn note_end(&self, lines: &[&str], mut index: usize) -> usize {
        while let Some(line) = lines.get(index) {
            if heading(line).is_some()
                || note_definition(line).is_some()
                || (line.trim().is_empty()
                    && lines
                        .get(index + 1)
                        .is_some_and(|line| line.trim().is_empty()))
            {
                break;
            }
            index = self.opaque_end(index).unwrap_or(index) + 1;
        }
        index
    }
}

impl Parser<'_> {
    pub(super) fn blocks(&mut self, lines: &[&str], depth: usize) -> Result<Vec<Block>> {
        self.source_blocks(lines, depth, depth == 0 && self.select_active, false)
    }

    pub(super) fn source_blocks(
        &mut self,
        lines: &[&str],
        depth: usize,
        filter: bool,
        final_plain: bool,
    ) -> Result<Vec<Block>> {
        self.check_depth(depth)?;
        let boundaries = Boundaries::new(lines);
        let mut output = Vec::new();
        let mut index = 0;
        let mut excluded = None;
        let selection = filter.then(|| self.selected_headings(lines, &boundaries));
        let leading = (depth == 0)
            .then(|| lines.iter().position(|line| !comment(line)))
            .flatten();
        let mut selected = selection.is_none();
        while let Some(&line) = lines.get(index) {
            if let Some((level, title)) = heading(line) {
                if excluded.is_some_and(|old| level > old) {
                    index += 1;
                    continue;
                }
                excluded = None;
                let (text, tags) = heading_tags(title);
                if text == "COMMENT"
                    || text.starts_with("COMMENT ")
                    || tags
                        .iter()
                        .any(|tag| self.exclude.iter().any(|excluded| excluded == tag))
                    || (tags.contains(&"ARCHIVE") && self.setting("arch", "headline") == "nil")
                {
                    excluded = Some(level);
                    index += 1;
                    continue;
                }
                selected = selection.as_ref().is_none_or(|entries| entries[index]);
                if !selected {
                    index += 1;
                    continue;
                }
                index += 1;
                let planning = lines
                    .get(index)
                    .map_or_else(Vec::new, |line| planning_entries(line));
                index += usize::from(!planning.is_empty());
                let mut attr = Attr::default();
                if lines
                    .get(index)
                    .and_then(|line| drawer(line))
                    .is_some_and(|name| name.eq_ignore_ascii_case("properties"))
                    && let Some(end) = boundaries.drawers[index]
                {
                    attr = heading_properties(&lines[index + 1..end]);
                    index = end + 1;
                }
                output.push(self.header(level, text, &tags, attr, depth)?);
                if !planning.is_empty() && self.setting("p", "nil") == "t" {
                    output.push(planning_block(planning));
                }
                if tags.contains(&"ARCHIVE") && self.setting("arch", "headline") == "headline" {
                    excluded = Some(level);
                }
                continue;
            }
            if excluded.is_some() || (!selected && directive(line).is_none()) {
                index = boundaries.opaque_end(index).unwrap_or(index) + 1;
                continue;
            }
            if line.trim().is_empty() || comment(line) {
                index += 1;
                continue;
            }
            if note_definition(line).is_some() {
                let end = boundaries.note_end(lines, index + 1);
                index += 1;
                while index < end {
                    if let Some((key, value)) = directive(lines[index])
                        && super::directives::document_directive(key)
                    {
                        self.keyword(lines[index], key, value, depth, index + 1)?;
                    }
                    index = boundaries.opaque_end(index).unwrap_or(index) + 1;
                }
                continue;
            }
            let start = index;
            let mut affiliated = Affiliated::default();
            while lines.get(index).is_some_and(|line| affiliated.read(line)) {
                index += 1;
            }
            let Some(&line) = lines.get(index) else {
                output.extend(lines[start..index].iter().map(|line| raw_org(line)));
                break;
            };
            if line.trim_start().starts_with('|') {
                let first = index;
                while lines
                    .get(index)
                    .is_some_and(|line| line.trim_start().starts_with('|'))
                {
                    index += 1;
                }
                output.push(self.table(&lines[first..index], &affiliated, depth)?);
                continue;
            }
            if let Some((name, args)) = block_mark(line, "#+begin_")
                && let Some(end) = boundaries.block_end(index)
            {
                let parsed =
                    self.greater(name, args, &lines[index + 1..end], &affiliated, depth)?;
                index = end + 1;
                output.extend(parsed);
                if name.eq_ignore_ascii_case("src")
                    && let Some((start, finish)) = result_range(
                        lines,
                        &boundaries,
                        index,
                        self.enabled(Extension::FancyLists),
                    )
                {
                    let attr = code_attributes("src", args, &affiliated).map(|(attr, _)| attr);
                    if matches!(
                        attr.as_ref().and_then(|attr| attr.get("exports")),
                        Some("results" | "both")
                    ) {
                        output.extend(self.blocks(&lines[start..finish], depth + 1)?);
                    }
                    index = finish;
                }
                continue;
            }
            if start < index {
                if line.trim_start().starts_with("[[") && line.trim_end().ends_with("]]") {
                    let inlines = self.inlines(line.trim(), depth + 1)?;
                    if let [Inline::Image(_, _, _)] = inlines.as_slice() {
                        output.push(self.figure(inlines, &affiliated, depth)?);
                        index += 1;
                        continue;
                    }
                }
                output.extend(lines[start..index].iter().map(|line| raw_org(line)));
                continue;
            }
            if let Some((key, value)) = directive(line) {
                output.extend(self.keyword(line, key, value, depth, index + 1)?);
                index += 1;
                continue;
            }
            if line.trim_start().starts_with("#+") {
                output.push(raw_org(line));
                index += 1;
                continue;
            }
            if let Some(name) = drawer(line)
                && let Some(end) = boundaries.drawers[index]
                && !name.eq_ignore_ascii_case("end")
            {
                let name = name.to_uppercase();
                let settings = self.setting("d", "t");
                let names = settings
                    .trim_matches(['(', ')'])
                    .split_whitespace()
                    .map(|name| name.trim_matches('"'));
                let has_name = names
                    .into_iter()
                    .any(|item| item.eq_ignore_ascii_case(&name));
                if name == "PROPERTIES" {
                    let properties = if leading == Some(index) {
                        &lines[index + 1..end]
                    } else {
                        &[]
                    };
                    for line in properties {
                        if let Some((key, value)) = property(line) {
                            self.meta.insert(
                                key.to_ascii_lowercase(),
                                MetaValue::MetaString(value.into()),
                            );
                        }
                    }
                } else if settings == "t"
                    || (settings.starts_with('(') && (settings.starts_with("(not ") != has_name))
                {
                    let contents = self.blocks(&lines[index + 1..end], depth + 1)?;
                    output.push(Block::Div(
                        Attr::new("", vec![name, "drawer".into()], vec![]),
                        contents,
                    ));
                }
                index = end + 1;
                continue;
            }
            if fixed(line).is_some() {
                let mut text = String::new();
                while let Some(value) = lines.get(index).and_then(|line| fixed(line)) {
                    text.push_str(value);
                    text.push('\n');
                    index += 1;
                }
                output.push(Block::CodeBlock(Attr::default(), text));
                continue;
            }
            if rule(line) {
                output.push(Block::HorizontalRule);
                index += 1;
                continue;
            }
            if let Some(item) = marker(line, self.enabled(Extension::FancyLists)) {
                let (block, count) = self.list(&lines[index..], item, depth)?;
                output.push(block);
                index += count;
                continue;
            }
            if self.setting("tex", "t") != "verbatim"
                && let Some(&end) = boundaries.latex.get(&index)
                && let Some(env) = latex_env(line)
            {
                if self.setting("tex", "t") != "nil" {
                    let text = lines[index..=end].join("\n");
                    let format = Format::new("latex");
                    let math = matches!(
                        env.trim_end_matches('*'),
                        "equation"
                            | "align"
                            | "gather"
                            | "multline"
                            | "flalign"
                            | "alignat"
                            | "eqnarray"
                            | "displaymath"
                            | "math"
                    );
                    output.push(if math {
                        Block::Para(vec![Inline::RawInline(format, text)])
                    } else {
                        Block::RawBlock(format, format!("{text}\n"))
                    });
                }
                index = end + 1;
                continue;
            }
            index = self.paragraph_end(lines, &boundaries, index);
            let paragraph = lines[start..index].join("\n");
            let inlines = self.inlines(&paragraph, depth + 1)?;
            if !inlines.is_empty() {
                output.push(if final_plain && index == lines.len() {
                    Block::Plain(inlines)
                } else {
                    Block::Para(inlines)
                });
            }
        }
        let maximum = self.setting("H", "3").parse().unwrap_or(3);
        let mut blocks = output.into_iter().peekable();
        let mut flattened = Vec::new();
        while let Some(block) = blocks.next() {
            if let Block::Header(level, _, _) = &block {
                if *level > maximum {
                    if let Block::Header(_, _, text) = block {
                        flattened.push(Block::Para(text));
                    }
                    continue;
                }
                let can_list = *level == maximum;
                flattened.push(block);
                if can_list {
                    while blocks
                        .peek()
                        .is_some_and(|block| !matches!(block, Block::Header(_, _, _)))
                    {
                        flattened.extend(blocks.next());
                    }
                    if matches!(blocks.peek(), Some(Block::Header(level, _, _)) if *level > maximum)
                    {
                        flattened.push(self.section_list(&mut blocks, maximum, depth + 1)?);
                    }
                }
            } else {
                flattened.push(block);
            }
        }
        Ok(flattened)
    }

    pub(super) fn paragraph_end(
        &self,
        lines: &[&str],
        boundaries: &Boundaries,
        start: usize,
    ) -> usize {
        let mut index = start + 1;
        while lines.get(index).is_some_and(|line| {
            !line.trim().is_empty()
                && heading(line).is_none()
                && !comment(line)
                && !line.trim_start().starts_with(['|'])
                && !line.trim_start().starts_with("#+")
                && marker(line, self.enabled(Extension::FancyLists)).is_none()
                && note_definition(line).is_none()
                && fixed(line).is_none()
                && !rule(line)
                && boundaries.drawers[index].is_none()
                && (self.setting("tex", "t") == "verbatim"
                    || !boundaries.latex.contains_key(&index))
        }) {
            index += 1;
        }
        index
    }

    fn section_list(
        &self,
        blocks: &mut std::iter::Peekable<std::vec::IntoIter<Block>>,
        parent: i64,
        depth: usize,
    ) -> Result<Block> {
        self.check_depth(depth)?;
        let mut items = Vec::new();
        while matches!(blocks.peek(), Some(Block::Header(level, _, _)) if *level > parent) {
            let Some(Block::Header(level, _, title)) = blocks.next() else {
                break;
            };
            let mut item = vec![Block::Para(title)];
            while blocks
                .peek()
                .is_some_and(|block| !matches!(block, Block::Header(_, _, _)))
            {
                item.extend(blocks.next());
            }
            if matches!(blocks.peek(), Some(Block::Header(child, _, _)) if *child > level) {
                item.push(self.section_list(blocks, level, depth + 1)?);
            }
            items.push(item);
        }
        Ok(Block::OrderedList(ListAttributes::default(), items))
    }

    fn selected_headings(&self, lines: &[&str], boundaries: &Boundaries) -> Vec<bool> {
        let mut entries = vec![false; lines.len()];
        let mut ancestors = Vec::<(i64, usize, bool)>::new();
        for (position, level, title) in boundaries.headings(lines) {
            while ancestors
                .last()
                .is_some_and(|(previous, _, _)| *previous >= level)
            {
                ancestors.pop();
            }
            let tagged = heading_tags(title)
                .1
                .iter()
                .any(|tag| self.select.iter().any(|item| item == tag));
            if tagged {
                for (_, index, _) in ancestors.iter().rev() {
                    if entries[*index] {
                        break;
                    }
                    entries[*index] = true;
                }
            }
            let active = tagged || ancestors.last().is_some_and(|(_, _, active)| *active);
            entries[position] = active;
            ancestors.push((level, position, active));
        }
        entries
    }

    fn header(
        &mut self,
        level: i64,
        title: &str,
        tags: &[&str],
        mut attr: Attr,
        depth: usize,
    ) -> Result<Block> {
        let (word, rest) = title.split_once(char::is_whitespace).unwrap_or((title, ""));
        let keyword = self
            .keywords
            .get(word)
            .copied()
            .filter(|_| !rest.is_empty() || !tags.is_empty());
        let title = if keyword.is_some() {
            rest.trim_start()
        } else {
            title
        };
        let text = self.inlines(title, depth + 1)?;
        if attr.id.is_empty() && self.enabled(Extension::AutoIdentifiers) {
            let base = auto_identifier(&text, self.options.extensions);
            if base.is_empty() {
                self.empty_headings += 1;
                attr.id = self.ids.unique(&format!("section-{}", self.empty_headings));
            } else {
                attr.id = self.ids.unique(&base);
            }
        } else if !attr.id.is_empty() {
            self.ids.register(&attr.id);
        }
        let mut content = Vec::new();
        if let Some(done) = keyword.filter(|_| self.setting("todo", "t") != "nil") {
            content.push(Inline::Span(
                Attr::new(
                    "",
                    vec![if done { "done" } else { "todo" }.into(), word.into()],
                    vec![],
                ),
                vec![Inline::Str(word.into())],
            ));
            if !text.is_empty() {
                content.push(Inline::Space);
            }
        }
        content.extend(text);
        if !tags.is_empty() && self.setting("tags", "t") != "nil" {
            content.push(Inline::Space);
            for (i, tag) in tags.iter().enumerate() {
                if i > 0 {
                    content.push(Inline::Str("\u{a0}".into()));
                }
                content.push(Inline::Span(
                    Attr::new(
                        "",
                        vec!["tag".into()],
                        vec![("tag-name".into(), (*tag).into())],
                    ),
                    vec![Inline::SmallCaps(vec![Inline::Str((*tag).into())])],
                ));
            }
        }
        Ok(Block::Header(
            level.saturating_add(self.heading_shift).max(1),
            attr,
            content,
        ))
    }

    pub(super) fn caption(&mut self, affiliated: &Affiliated<'_>, depth: usize) -> Result<Caption> {
        let text = self.inlines(&affiliated.caption, depth + 1)?;
        Ok(Caption {
            short: None,
            long: if text.is_empty() {
                vec![]
            } else {
                vec![Block::Plain(text)]
            },
        })
    }

    fn figure(
        &mut self,
        mut inlines: Vec<Inline>,
        affiliated: &Affiliated<'_>,
        depth: usize,
    ) -> Result<Block> {
        if !affiliated.caption.is_empty() {
            return Ok(Block::Figure(
                affiliated.attr(),
                self.caption(affiliated, depth)?,
                vec![Block::Plain(inlines)],
            ));
        }
        if let Some(Inline::Image(attr, _, _)) = inlines.first_mut() {
            *attr = affiliated.attr();
        }
        Ok(Block::Para(inlines))
    }

    fn greater(
        &mut self,
        name: &str,
        args: &str,
        lines: &[&str],
        affiliated: &Affiliated<'_>,
        depth: usize,
    ) -> Result<Vec<Block>> {
        let kind = name.to_ascii_lowercase();
        let block = match kind.as_str() {
            "comment" => return Ok(vec![]),
            "quote" => Block::BlockQuote(self.blocks(lines, depth + 1)?),
            "export" | "html" | "latex" | "ascii" => {
                let format = if kind == "export" {
                    args.split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                } else {
                    kind
                };
                Block::RawBlock(Format::new(format), block_text(lines, false, false))
            }
            "src" | "example" => {
                let (mut attr, preserve) =
                    code_attributes(&kind, args, affiliated).unwrap_or_default();
                if matches!(attr.get("exports"), Some("none" | "results")) {
                    return Ok(vec![]);
                }
                if attr.id.is_empty() {
                    attr.id = affiliated.id.into();
                }
                let code = Block::CodeBlock(attr, block_text(lines, preserve, true));
                if affiliated.caption.is_empty() {
                    code
                } else {
                    let title = Block::Div(
                        Attr::new("", vec!["caption".into()], vec![]),
                        self.caption(affiliated, depth)?.long,
                    );
                    Block::Div(
                        Attr::new("", vec!["captioned-content".into()], vec![]),
                        vec![title, code],
                    )
                }
            }
            "verse" => {
                let mut verse = Vec::new();
                let common = common_indent(lines);
                for line in lines {
                    let text = line.trim_start_matches([' ', '\t']);
                    let mut inlines = self.inlines(text, depth + 1)?;
                    let indent = (line.len() - text.len()).saturating_sub(common);
                    if indent > 0 && !inlines.is_empty() {
                        match inlines.first_mut() {
                            Some(Inline::Str(str)) => str.insert_str(0, &"\u{a0}".repeat(indent)),
                            _ => inlines.insert(0, Inline::Str("\u{a0}".repeat(indent))),
                        }
                    }
                    verse.push(inlines);
                }
                Block::LineBlock(verse)
            }
            _ => {
                let mut body = self.blocks(lines, depth + 1)?;
                let mut attr = affiliated.attr();
                if let Some(class) = attr.get("class") {
                    attr.classes = class.split_whitespace().map(str::to_owned).collect();
                    attr.attributes.retain(|(key, _)| key != "class");
                }
                if matches!(
                    kind.as_str(),
                    "note" | "tip" | "important" | "caution" | "warning"
                ) {
                    let title = format!("{}{}", kind[..1].to_ascii_uppercase(), &kind[1..]);
                    body.insert(
                        0,
                        Block::Div(
                            Attr::new("", vec!["title".into()], vec![]),
                            vec![Block::Para(vec![Inline::Str(title)])],
                        ),
                    );
                    attr.classes.push(kind);
                } else {
                    attr.classes.push(name.into());
                }
                Block::Div(attr, body)
            }
        };
        Ok(vec![block])
    }
}

fn planning_block(entries: Vec<(&str, &str)>) -> Block {
    let mut content = Vec::new();
    for (key, timestamp) in entries {
        if !content.is_empty() {
            content.push(Inline::Space);
        }
        content.push(Inline::Strong(vec![Inline::Str(key.into())]));
        content.push(Inline::Space);
        content.push(Inline::Emph(vec![Inline::Str(timestamp.into())]));
    }
    Block::Plain(content)
}

fn raw_org(line: &str) -> Block {
    Block::RawBlock(Format::new("org"), line.trim_start().into())
}

fn result_range(
    lines: &[&str],
    boundaries: &Boundaries,
    index: usize,
    fancy: bool,
) -> Option<(usize, usize)> {
    let offset = lines[index..]
        .iter()
        .position(|line| !line.trim().is_empty())?;
    let mark = index + offset;
    let (key, value) = directive(lines[mark])?;
    if !key.eq_ignore_ascii_case("results") || !value.is_empty() {
        return None;
    }
    let start = mark + 1;
    let first = lines.get(start).copied().unwrap_or_default();
    let end = if fixed(first).is_some() {
        lines[start..]
            .iter()
            .take_while(|line| fixed(line).is_some())
            .count()
            + start
    } else if first.trim_start().starts_with('|') {
        lines[start..]
            .iter()
            .take_while(|line| line.trim_start().starts_with('|'))
            .count()
            + start
    } else if let Some(end) = boundaries.ends.get(&start) {
        end + 1
    } else {
        lines[start..]
            .iter()
            .take_while(|line| {
                !line.trim().is_empty()
                    && directive(line).is_none()
                    && heading(line).is_none()
                    && marker(line, fancy).is_none()
            })
            .count()
            + start
    };
    Some((start, end))
}

fn property(line: &str) -> Option<(&str, &str)> {
    line.trim()
        .strip_prefix(':')?
        .split_once(':')
        .map(|(key, value)| (key, value.trim()))
}

fn heading_properties(lines: &[&str]) -> Attr {
    let mut attr = Attr::default();
    let mut id = "";
    for line in lines {
        if let Some((key, value)) = property(line) {
            match key.to_ascii_lowercase().as_str() {
                "custom_id" => attr.id = value.into(),
                "id" => id = value,
                "class" => attr
                    .classes
                    .extend(value.split_whitespace().map(str::to_owned)),
                "unnumbered" if value != "nil" => attr.classes.push("unnumbered".into()),
                "unnumbered" => {}
                key => attr.attributes.push((key.into(), value.into())),
            }
        }
    }
    if attr.id.is_empty() {
        attr.id = id.into();
    }
    attr
}

fn common_indent(lines: &[&str]) -> usize {
    lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start_matches([' ', '\t']).len())
        .min()
        .unwrap_or(0)
}

fn block_text(lines: &[&str], preserve: bool, unescape: bool) -> String {
    let indent = if preserve { 0 } else { common_indent(lines) };
    let mut text = String::new();
    for line in lines {
        let leading = line.len() - line.trim_start_matches([' ', '\t']).len();
        let line = &line[leading.min(indent)..];
        if unescape {
            text.push_str(&code_line(line, false));
        } else {
            text.push_str(line);
        }
        text.push('\n');
    }
    text
}

fn code_attributes(kind: &str, args: &str, affiliated: &Affiliated<'_>) -> Option<(Attr, bool)> {
    let mut attr = Attr::default();
    let mut args = words(args).peekable();
    if kind == "src"
        && let Some(lang) = args.next()
    {
        if !lang
            .chars()
            .all(|c| c.is_alphanumeric() || "-_".contains(c))
        {
            return None;
        }
        if let Some(lang) = lang.strip_prefix("jupyter-") {
            attr.classes.extend([lang.into(), "code".into()]);
        } else {
            let mapped = match lang {
                "C" => "c",
                "R" => "r",
                "emacs-lisp" | "lisp" => "commonlisp",
                "js" => "javascript",
                "sh" => "bash",
                "sqlite" => "sql",
                _ => lang,
            };
            attr.classes.push(mapped.into());
            if mapped != lang {
                attr.attributes.push(("org-language".into(), lang.into()));
            }
        }
    }
    let mut preserve = false;
    if args.next_if_eq(&"-i").is_some() {
        preserve = true;
    }
    if args.next_if_eq(&"-l").is_some() {
        args.next()?;
    }
    if let Some(flag) = args.next_if(|word| matches!(*word, "-n" | "+n")) {
        let start = args
            .next_if(|word| word.parse::<i64>().is_ok())
            .unwrap_or_default();
        if !start.is_empty() && args.peek().is_some() {
            return None;
        }
        attr.classes.push("numberLines".into());
        if flag == "+n" {
            attr.classes.push("continuedSourceBlock".into());
        }
        attr.attributes.push(("startFrom".into(), start.into()));
    }
    if args.peek().is_some_and(|word| !word.starts_with(':')) {
        return None;
    }
    for (key, value) in &affiliated.html {
        if key == "id" {
            attr.id.clone_from(value);
        } else {
            attr.attributes.push((key.clone(), value.clone()));
        }
    }
    if !affiliated.id.is_empty() {
        attr.id = affiliated.id.into();
    }
    attr.attributes
        .extend(attributes(&args.collect::<Vec<_>>().join(" ")));
    Some((attr, preserve))
}
