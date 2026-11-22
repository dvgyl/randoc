use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Component, Path};

use randoc_ast::{Attr, Block, Format, Inline, MetaValue};
use randoc_core::{Error, LogMessage, Result, SourcePos};

use super::blocks::{
    Boundaries, block_mark, comment, fixed, heading, heading_tags, note_definition,
};
use super::{Footnote, Parser};

pub(super) fn directive(line: &str) -> Option<(&str, &str)> {
    let (name, value) = line.trim_start().strip_prefix("#+")?.split_once(':')?;
    (!name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .then(|| (name, value.trim()))
}

pub(super) fn document_directive(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "options"
            | "todo"
            | "seq_todo"
            | "typ_todo"
            | "macro"
            | "select_tags"
            | "export_select_tags"
            | "exclude_tags"
            | "link"
            | "title"
            | "subtitle"
            | "date"
            | "institute"
            | "keywords"
            | "description"
            | "nocite"
            | "author"
            | "email"
            | "language"
            | "creator"
            | "bibliography"
            | "html_head"
            | "html_head_extra"
            | "latex_header"
            | "latex_header_extra"
    )
}

pub(super) fn words(mut input: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        input = input.trim_start();
        let first = input.chars().next()?;
        let end = match first {
            '"' => input[1..].find('"').map(|i| i + 2),
            '(' => input.find(')').map(|i| i + 1),
            _ => input.find(char::is_whitespace),
        }
        .unwrap_or(input.len());
        let (word, rest) = input.split_at(end);
        input = rest;
        Some(word)
    })
}

pub(super) fn attributes(input: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut current = None::<(String, String)>;
    for word in words(input) {
        if let Some(key) = word.strip_prefix(':').filter(|key| !key.is_empty()) {
            if let Some((key, value)) = current.take() {
                pairs.push((
                    key,
                    if value.is_empty() {
                        "yes".into()
                    } else {
                        value
                    },
                ));
            }
            current = Some((key.into(), String::new()));
        } else if let Some((_, value)) = &mut current {
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(word.trim_matches('"'));
        }
    }
    if let Some((key, value)) = current {
        pairs.push((
            key,
            if value.is_empty() {
                "yes".into()
            } else {
                value
            },
        ));
    }
    pairs
}

#[derive(Default)]
pub(super) struct Affiliated<'a> {
    pub id: &'a str,
    pub caption: String,
    pub html: Vec<(String, String)>,
}

impl<'a> Affiliated<'a> {
    pub fn read(&mut self, line: &'a str) -> bool {
        let Some((key, value)) = directive(line) else {
            return false;
        };
        match key.to_ascii_lowercase().as_str() {
            "name" | "label" => self.id = value,
            "caption" => {
                if !self.caption.is_empty() {
                    self.caption.push(' ');
                }
                self.caption.push_str(value);
            }
            "attr_html" => self.html.extend(attributes(value)),
            _ => return false,
        }
        true
    }

    pub fn attr(&self) -> Attr {
        Attr::new(self.id, vec![], self.html.clone())
    }
}

impl Parser<'_> {
    pub(super) fn collect(&mut self, lines: &[&str], depth: usize) {
        if self.check_depth(depth).is_err() {
            return;
        }
        let boundaries = Boundaries::new(lines);
        let mut index = 0;
        while let Some(&line) = lines.get(index) {
            if let Some(end) = boundaries.block_end(index)
                && let Some((kind, _)) = block_mark(line, "#+begin_")
            {
                match kind.to_ascii_lowercase().as_str() {
                    "src" | "example" | "comment" | "export" | "html" | "latex" | "ascii" => {}
                    "verse" => {
                        for line in &lines[index + 1..end] {
                            self.collect_inline_definitions(line, depth + 1);
                        }
                    }
                    _ => self.collect(&lines[index + 1..end], depth + 1),
                }
                index = end + 1;
                continue;
            }
            if let Some(end) = boundaries.opaque_end(index) {
                index = end + 1;
                continue;
            }
            if let Some((name, value)) = directive(line) {
                self.global_keyword(&name.to_ascii_lowercase(), value);
            } else if let Some((label, first)) = note_definition(line) {
                let end = boundaries.note_end(lines, index + 1);
                let mut body = first.to_owned();
                index += 1;
                while index < end {
                    body.push('\n');
                    if let Some(last) = boundaries.opaque_end(index) {
                        for (offset, line) in lines[index..=last].iter().enumerate() {
                            if offset > 0 {
                                body.push('\n');
                            }
                            body.push_str(line);
                        }
                        index = last + 1;
                        continue;
                    }
                    let line = lines[index];
                    if let Some((name, value)) =
                        directive(line).filter(|(key, _)| document_directive(key))
                    {
                        self.global_keyword(&name.to_ascii_lowercase(), value);
                    } else {
                        body.push_str(line);
                    }
                    index += 1;
                }
                self.notes.insert(label.into(), Footnote::Blocks(body));
                continue;
            } else if let Some((_, title)) = heading(line) {
                self.collect_inline_definitions(heading_tags(title).0, depth + 1);
            } else if !line.trim().is_empty()
                && !comment(line)
                && fixed(line).is_none()
                && !line.trim_start().starts_with("#+")
            {
                let end = self.paragraph_end(lines, &boundaries, index);
                self.collect_inline_definitions(&lines[index..end].join("\n"), depth + 1);
                index = end;
                continue;
            }
            index += 1;
        }
    }

    pub(super) fn has_select_tags(&self, lines: &[&str]) -> bool {
        Boundaries::new(lines).headings(lines).any(|(_, _, title)| {
            heading_tags(title)
                .1
                .iter()
                .any(|tag| self.select.iter().any(|selected| selected == tag))
        })
    }

    fn global_keyword(&mut self, name: &str, value: &str) {
        match name {
            "options" => self.export_options(value, true),
            "select_tags" | "export_select_tags" => {
                self.select = value.split_whitespace().map(str::to_owned).collect();
            }
            "exclude_tags" => self.exclude = value.split_whitespace().map(str::to_owned).collect(),
            "link" => {
                if let Some((key, body)) = value.split_once(char::is_whitespace) {
                    self.abbreviations
                        .insert(key.into(), body.trim_start().into());
                }
            }
            _ => {}
        }
    }

    fn export_options(&mut self, input: &str, global: bool) {
        let mut words = words(input).peekable();
        while let Some(option) = words.next() {
            let Some((key, mut value)) = option.split_once(':') else {
                continue;
            };
            let mut list = String::new();
            if value.starts_with('(') && !value.ends_with(')') {
                list.push_str(value);
                for word in words.by_ref() {
                    list.push(' ');
                    list.push_str(word);
                    if word.ends_with(')') {
                        break;
                    }
                }
                value = &list;
            }
            if value.is_empty() && words.peek().is_some_and(|word| !word.contains(':')) {
                value = words.next().unwrap_or_default();
            }
            match key {
                "tags" | "todo" | "arch" | "d" | "p" | "H" | "author" | "email" if global => {
                    self.settings.insert(key.into(), value.into());
                }
                "^" | "*" | "'" | "-" | "e" | "f" | "tex" | "\\n" if !global => {
                    self.settings.insert(key.into(), value.into());
                }
                "tags" | "todo" | "arch" | "d" | "p" | "H" | "author" | "email" | "^" | "*"
                | "'" | "-" | "e" | "f" | "tex" | "\\n" | "toc" | "pri" | "c" | "stat"
                | "title" | "num" | "creator" | "date" | "inline" | "timestamp" | "prop" | "<" => {}
                _ if global => self
                    .context
                    .report(LogMessage::UnknownOrgExportOption(format!("{key}:{value}"))),
                _ => {}
            }
        }
    }

    fn todo_keywords(&mut self, value: &str) {
        if !self.custom_keywords {
            self.keywords.clear();
            self.custom_keywords = true;
        }
        let mut done = false;
        let mut words = value.split_whitespace().peekable();
        let explicit = value.split_whitespace().any(|word| word == "|");
        while let Some(word) = words.next() {
            if word == "|" {
                done = true;
            } else {
                let word = word.split('(').next().unwrap_or(word);
                self.keywords
                    .insert(word.into(), done || (!explicit && words.peek().is_none()));
            }
        }
    }

    pub(super) fn keyword(
        &mut self,
        line: &str,
        key: &str,
        value: &str,
        depth: usize,
        number: usize,
    ) -> Result<Vec<Block>> {
        let name = key.to_ascii_lowercase();
        match name.as_str() {
            "options" => self.export_options(value, false),
            "todo" | "seq_todo" | "typ_todo" => self.todo_keywords(value),
            "macro" => {
                if let Some((key, body)) = value.split_once(char::is_whitespace) {
                    self.macros.insert(key.into(), body.trim_start().into());
                }
            }
            "select_tags" | "export_select_tags" | "exclude_tags" | "link" => {}
            "title" | "subtitle" | "date" | "institute" | "keywords" | "description" | "nocite"
            | "author" => {
                if name != "author" || self.setting("author", "t") != "nil" {
                    let content = self.inlines(value, depth + 1)?;
                    if matches!(name.as_str(), "title" | "author")
                        && let Some(MetaValue::MetaInlines(previous)) = self.meta.0.get_mut(&name)
                    {
                        previous.push(Inline::SoftBreak);
                        previous.extend(content);
                    } else {
                        self.meta.insert(name, MetaValue::MetaInlines(content));
                    }
                }
            }
            "email" | "language" | "creator" => {
                if name != "email" || self.setting("email", "t") != "nil" {
                    let key = if name == "language" { "lang" } else { &name };
                    self.meta.insert(key, MetaValue::MetaString(value.into()));
                }
            }
            "bibliography" => {
                for path in words(value) {
                    self.append_meta(
                        "bibliography",
                        MetaValue::MetaString(path.trim_matches('"').into()),
                    );
                }
            }
            "html_head" | "html_head_extra" | "latex_header" | "latex_header_extra" => {
                let format = if name.starts_with("html") {
                    "html"
                } else {
                    "latex"
                };
                self.append_meta(
                    "header-includes",
                    MetaValue::MetaInlines(vec![Inline::RawInline(
                        Format::new(format),
                        value.into(),
                    )]),
                );
            }
            "html" | "latex" | "texinfo" | "beamer" => {
                return Ok(vec![Block::RawBlock(Format::new(name), value.into())]);
            }
            "include" => return self.include(line, value, depth, number),
            _ => {
                return Ok(vec![Block::RawBlock(
                    Format::new("org"),
                    line.trim_start().into(),
                )]);
            }
        }
        Ok(Vec::new())
    }

    fn append_meta(&mut self, key: &str, value: MetaValue) {
        let item = self
            .meta
            .0
            .entry(key.into())
            .or_insert_with(|| MetaValue::MetaList(Vec::new()));
        if let MetaValue::MetaList(values) = item {
            values.push(value);
        }
    }

    fn include(
        &mut self,
        line: &str,
        value: &str,
        depth: usize,
        number: usize,
    ) -> Result<Vec<Block>> {
        let raw = || {
            vec![Block::RawBlock(
                Format::new("org"),
                line.trim_start().into(),
            )]
        };
        let mut args = words(value).peekable();
        let Some(file) = args
            .next()
            .filter(|name| name.starts_with('"') && name.ends_with('"'))
        else {
            return Ok(raw());
        };
        let file = file.trim_matches('"');
        let kind = args
            .next_if(|word| !word.starts_with(':'))
            .unwrap_or_default();
        let lang = args
            .next_if(|word| !word.starts_with(':'))
            .unwrap_or_default();
        if (kind == "src" && !lang.bytes().all(|byte| byte.is_ascii_alphanumeric()))
            || (kind == "export" && lang.is_empty())
        {
            return Ok(raw());
        }
        let settings = attributes(&args.collect::<Vec<_>>().join(" "));
        let field = |name| {
            settings
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, val)| val.as_str())
        };
        if self.active_includes.len() >= self.context.limits.max_nesting_depth.min(32) {
            return Ok(Vec::new());
        }
        self.check_depth(depth + 1)?;
        if self.includes_exhausted {
            return Ok(Vec::new());
        }
        let literal = |body: String| match kind {
            "src" if !lang.is_empty() => {
                vec![Block::CodeBlock(
                    Attr::new("", vec![lang.into()], vec![]),
                    body,
                )]
            }
            "src" | "example" => vec![Block::CodeBlock(Attr::default(), body)],
            "export" => vec![Block::RawBlock(Format::new(lang), body)],
            _ => Vec::new(),
        };
        let resource = Path::new(file);
        let mut paths = Vec::new();
        if !resource.is_absolute() {
            if let Some(parent) = Path::new(&self.source)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
            {
                paths.push(parent.join(resource));
            }
            paths.extend(
                self.context
                    .state
                    .resource_path
                    .iter()
                    .map(|path| Path::new(path).join(resource)),
            );
        }
        paths.push(resource.to_owned());
        let limit = usize::try_from(self.remaining()).unwrap_or(usize::MAX);
        let mut visited = HashSet::new();
        let current = normalized(Path::new(&self.source));
        for path in paths {
            let path = path
                .components()
                .filter(|part| *part != Component::CurDir)
                .collect::<std::path::PathBuf>();
            let key = normalized(&path);
            if self.active_includes.contains(&key) || current == key {
                return Ok(Vec::new());
            }
            if !visited.insert(key.clone()) {
                continue;
            }
            let bytes = match self
                .context
                .host()
                .read_file_limited(&path.to_string_lossy(), limit)
            {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == ErrorKind::FileTooLarge => {
                    self.includes_exhausted = true;
                    return Ok(Vec::new());
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        ErrorKind::NotFound
                            | ErrorKind::PermissionDenied
                            | ErrorKind::Unsupported
                            | ErrorKind::InvalidData
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(Error::App(format!("{}: {error}", path.display()))),
            };
            self.charge(bytes.len() as u64)?;
            let Ok(source) = std::str::from_utf8(&bytes) else {
                break;
            };
            let mut slice = source;
            if let Some((start, end)) = field("lines").and_then(|value| value.split_once('-')) {
                let start = start.parse::<usize>().unwrap_or(1).saturating_sub(1);
                let end = end.parse::<usize>().ok().map(|line| line.saturating_sub(1));
                let mut offsets =
                    std::iter::once(0).chain(slice.match_indices('\n').map(|(pos, _)| pos + 1));
                let start_pos = offsets.nth(start).unwrap_or(slice.len());
                let end_pos = end.map_or(slice.len(), |end| {
                    if end <= start {
                        start_pos
                    } else {
                        offsets.nth(end - start - 1).unwrap_or(slice.len())
                    }
                });
                slice = &slice[start_pos..end_pos];
            }
            if matches!(kind, "src" | "example" | "export") {
                return Ok(literal(slice.to_owned()));
            }
            let lines = slice.lines().collect::<Vec<_>>();
            let shift = field("minlevel")
                .and_then(|v| v.parse::<i64>().ok())
                .map_or(0, |level| {
                    let minimum = Boundaries::new(&lines)
                        .headings(&lines)
                        .map(|(_, level, _)| level)
                        .min()
                        .unwrap_or(1);
                    level.saturating_sub(minimum)
                });
            let old_shift = std::mem::replace(&mut self.heading_shift, shift);
            let old_source =
                std::mem::replace(&mut self.source, path.to_string_lossy().into_owned());
            self.active_includes.push(current);
            self.collect(&lines, depth + 1);
            let select = self.has_select_tags(&lines);
            let result = self.source_blocks(&lines, depth + 1, select, true);
            self.active_includes.pop();
            self.source = old_source;
            self.heading_shift = old_shift;
            return result;
        }
        self.context.report(LogMessage::CouldNotLoadIncludeFile(
            file.into(),
            SourcePos::new(
                &self.source,
                u32::try_from(number).unwrap_or(u32::MAX).saturating_add(1),
                1,
            ),
        ));
        Ok(literal(String::new()))
    }
}

fn normalized(path: &Path) -> String {
    let mut parts = std::path::PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if parts.file_name().is_some_and(|name| name != "..") => {
                parts.pop();
            }
            Component::ParentDir if parts.has_root() => {}
            _ => parts.push(part),
        }
    }
    parts.to_string_lossy().into_owned()
}
