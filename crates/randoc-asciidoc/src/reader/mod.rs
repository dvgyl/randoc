mod attrs;
mod blocks;
mod inlines;
mod tables;

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use randoc_ast::{Attr, Block, Inline, Meta, MetaValue, Pandoc, Target};
use randoc_core::{Context, Error, LogMessage, Reader, ReaderOptions, Result, SourcePos, Sources};

use crate::limit;

pub(super) fn read(
    sources: &Sources,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let source_bytes = sources
        .0
        .iter()
        .try_fold(0_u64, |sum, source| {
            sum.checked_add(source.text.len() as u64)
        })
        .ok_or_else(|| limit("input size"))?;
    let expansion = expansion_allowance(source_bytes, context.limits.max_input_size);
    let mut state = State {
        remaining: context
            .limits
            .max_input_size
            .checked_sub(source_bytes)
            .ok_or_else(|| limit("input size"))?,
        expansion,
        expansion_allowance: expansion,
        input_bytes: source_bytes,
        meta: Meta::default(),
        source_name: "",
        context,
        options,
        attributes: HashMap::new(),
        labels: HashMap::new(),
        notes: HashMap::new(),
        includes: HashSet::new(),
        ids: HashSet::new(),
        suffixes: HashMap::new(),
        sections: Vec::new(),
        headings: Vec::new(),
        header_chars: 0,
    };
    let mut document = Pandoc::default();
    for (index, source) in sources.0.iter().enumerate() {
        state.source_name = &source.name;
        let path = Path::new(&source.name);
        let key = include_key(path);
        state.includes.insert(key.clone());
        let mut lines: Vec<_> = source.text.lines().collect();
        if let Some(first) = lines.first_mut() {
            *first = first.trim_start_matches('\u{feff}');
        }
        let start = if index == 0 { state.header(&lines)? } else { 0 };
        state.header_chars = lines[..start]
            .iter()
            .map(|line| line.chars().count() + 1)
            .sum();
        document
            .blocks
            .extend(state.blocks(&lines[start..], path, 0, blocks::Sections::Top)?);
        state.includes.remove(&key);
    }
    document.meta = std::mem::take(&mut state.meta);
    state.resolve(&mut document)?;
    Ok(document)
}

struct State<'a> {
    context: &'a mut Context,
    options: &'a ReaderOptions,
    attributes: HashMap<String, String>,
    labels: HashMap<String, (Vec<Inline>, u64)>,
    notes: HashMap<String, (Vec<Inline>, u64)>,
    includes: HashSet<PathBuf>,
    ids: HashSet<String>,
    suffixes: HashMap<String, usize>,
    sections: Vec<usize>,
    headings: Vec<(usize, String, bool, String)>,
    header_chars: usize,
    source_name: &'a str,
    meta: Meta,
    input_bytes: u64,
    expansion_allowance: u64,
    remaining: u64,
    expansion: u64,
}

impl State<'_> {
    fn depth(&self, depth: usize) -> Result<()> {
        if depth > self.context.limits.max_nesting_depth {
            return Err(limit("nesting depth"));
        }
        Ok(())
    }

    fn charge(&mut self, size: u64) -> Result<()> {
        self.expansion = self
            .expansion
            .checked_sub(size)
            .ok_or_else(|| limit("reference expansion"))?;
        Ok(())
    }

    fn header(&mut self, lines: &[&str]) -> Result<usize> {
        let mut i = 0;
        while lines.get(i).is_some_and(|line| {
            line.trim().is_empty() || line.starts_with("//") && !line.starts_with("////")
        }) {
            i += 1;
        }
        if let Some(title) = lines
            .get(i)
            .and_then(|line| line.strip_prefix("= ").or_else(|| line.strip_prefix("# ")))
        {
            let title = self.inlines(title, 0)?;
            self.meta.insert("title", MetaValue::MetaInlines(title));
            i += 1;
            if let Some(authors) = lines
                .get(i)
                .filter(|line| !line.trim().is_empty() && !line.starts_with([':', '=', '[']))
            {
                let mut values = Vec::new();
                for author in authors.split(';') {
                    let (name, email) = author
                        .trim()
                        .split_once(" <")
                        .unwrap_or((author.trim(), ""));
                    let mut name = self.inlines(name, 0)?;
                    if let Some(email) = email.strip_suffix('>') {
                        name.extend([
                            Inline::Space,
                            Inline::Str("(".into()),
                            Inline::Link(
                                Attr::default(),
                                vec![Inline::Str(email.into())],
                                Target::new(format!("mailto:{email}"), ""),
                            ),
                            Inline::Str(")".into()),
                        ]);
                    }
                    values.push(MetaValue::MetaInlines(name));
                }
                self.meta.insert("author", MetaValue::MetaList(values));
                i += 1;
                if let Some(revision) = lines.get(i).filter(|line| {
                    !line.trim().is_empty() && !line.starts_with([':', '=', '[', '/'])
                }) {
                    let (revision, remark) = revision.split_once(':').unwrap_or((revision, ""));
                    let (version, date) = revision.split_once(',').unwrap_or((revision, ""));
                    self.meta.insert(
                        "version",
                        MetaValue::MetaString(version.trim().trim_start_matches('v').into()),
                    );
                    if !date.is_empty() {
                        self.meta
                            .insert("date", MetaValue::MetaString(date.trim().into()));
                    }
                    if !remark.is_empty() {
                        self.meta
                            .insert("remark", MetaValue::MetaString(remark.trim().into()));
                    }
                    i += 1;
                }
            }
        }
        while let Some(line) = lines.get(i) {
            if line.starts_with("//") && !line.starts_with("////") {
                i += 1;
                continue;
            }
            if !self.attribute_entry(lines, &mut i) {
                break;
            }
        }
        Ok(i)
    }

    fn attribute_entry(&mut self, lines: &[&str], i: &mut usize) -> bool {
        let Some((key, value)) = lines.get(*i).and_then(|line| attrs::entry(line)) else {
            return false;
        };
        let mut value = value.to_owned();
        *i += 1;
        while let Some(prefix) = value.strip_suffix(" \\") {
            let Some(next) = lines.get(*i) else { break };
            let truncate = prefix.len();
            value.truncate(truncate);
            value.push(' ');
            value.push_str(next.trim());
            *i += 1;
        }
        self.attributes.insert(key.into(), value.clone());
        if key != "stem" {
            self.meta.insert(
                key,
                if value.is_empty() {
                    MetaValue::MetaBool(true)
                } else {
                    MetaValue::MetaString(value)
                },
            );
        }
        true
    }

    fn html(&mut self, text: &str, depth: usize) -> Result<Vec<Block>> {
        let Some(Reader::Text(reader)) = randoc_html_reader::reader("html") else {
            return Err(Error::Parse("Missing HTML reader".into()));
        };
        let mut options = self.options.clone();
        for ext in [
            randoc_core::Extension::RawHtml,
            randoc_core::Extension::NativeDivs,
            randoc_core::Extension::NativeSpans,
        ] {
            options.extensions.disable(ext);
        }
        options.extensions.enable(randoc_core::Extension::RawHtml);
        self.depth(depth)?;
        let max_depth = self.context.limits.max_nesting_depth;
        self.context.limits.max_nesting_depth = max_depth - depth;
        let result = reader.read(&Sources::from_text(text), &options, self.context);
        self.context.limits.max_nesting_depth = max_depth;
        Ok(result?.blocks)
    }

    fn include(&mut self, target: &str, parent: &Path) -> Result<Option<(PathBuf, String)>> {
        let path = parent.parent().unwrap_or(Path::new("")).join(target);
        let display = path.to_string_lossy();
        if self.context.host().is_sandboxed() {
            return Err(Error::ResourceNotFound(display.into_owned()));
        }
        if self.includes.contains(&include_key(&path)) {
            self.context.report(LogMessage::CouldNotLoadIncludeFile(
                display.into_owned(),
                SourcePos::default(),
            ));
            return Ok(None);
        }
        let max = self.remaining.min(self.context.limits.max_resource_size);
        let bytes = self
            .context
            .host()
            .read_file_limited(&display, usize::try_from(max).unwrap_or(usize::MAX))
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::FileTooLarge {
                    limit("input size")
                } else {
                    randoc_core::error::io_error(&display, "withBinaryFile", &err)
                }
            })?;
        self.remaining = self
            .remaining
            .checked_sub(bytes.len() as u64)
            .ok_or_else(|| limit("input size"))?;
        self.input_bytes += bytes.len() as u64;
        let allowance = expansion_allowance(self.input_bytes, self.context.limits.max_input_size);
        self.expansion += allowance - self.expansion_allowance;
        self.expansion_allowance = allowance;
        Ok(Some((path, String::from_utf8_lossy(&bytes).into_owned())))
    }
}

fn include_key(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if result.file_name().is_some_and(|part| part != "..") => {
                result.pop();
            }
            _ => result.push(component),
        }
    }
    result
}

fn expansion_allowance(input_bytes: u64, max_input_size: u64) -> u64 {
    // Allow for the memory cost of AST nodes in copied content.
    input_bytes.saturating_mul(32).min(max_input_size)
}
