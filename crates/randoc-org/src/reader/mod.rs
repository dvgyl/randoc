mod blocks;
mod directives;
mod inlines;
mod lists;
mod tables;

use std::collections::{HashMap, HashSet};

use randoc_ast::{Meta, Pandoc};
use randoc_core::shared::IdentifierSet;
use randoc_core::{Context, Error, Extension, ReaderOptions, Result, Sources};

use crate::shared::STACK_LIMIT;

struct Parser<'a> {
    options: &'a ReaderOptions,
    context: &'a mut Context,
    meta: Meta,
    settings: HashMap<String, String>,
    macros: HashMap<String, String>,
    abbreviations: HashMap<String, String>,
    keywords: HashMap<String, bool>,
    custom_keywords: bool,
    select: Vec<String>,
    exclude: Vec<String>,
    targets: HashSet<String>,
    notes: HashMap<String, Footnote>,
    active_notes: Vec<String>,
    active_macros: Vec<String>,
    active_includes: Vec<String>,
    ids: IdentifierSet,
    empty_headings: usize,
    used_bytes: u64,
    includes_exhausted: bool,
    table_cells: usize,
    source: String,
    heading_shift: i64,
    select_active: bool,
}

#[derive(Clone)]
enum Footnote {
    Blocks(String),
    Inline(String),
}

impl Footnote {
    fn text(&self) -> &str {
        match self {
            Self::Blocks(text) | Self::Inline(text) => text,
        }
    }
}

pub(super) fn read(
    sources: &Sources,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let bytes = sources.0.iter().fold(0u64, |total, source| {
        total.saturating_add(source.text.len() as u64)
    });
    let mut parser = Parser {
        options,
        context,
        meta: Meta::default(),
        settings: HashMap::new(),
        macros: HashMap::new(),
        abbreviations: HashMap::new(),
        keywords: HashMap::from([("TODO".into(), false), ("DONE".into(), true)]),
        custom_keywords: false,
        select: vec!["export".into()],
        exclude: vec!["noexport".into()],
        targets: HashSet::new(),
        notes: HashMap::new(),
        active_notes: Vec::new(),
        active_macros: Vec::new(),
        active_includes: Vec::new(),
        ids: IdentifierSet::new(),
        empty_headings: 0,
        used_bytes: 0,
        includes_exhausted: false,
        table_cells: 0,
        source: String::new(),
        heading_shift: 0,
        select_active: false,
    };
    parser.charge(bytes)?;
    // Collect document-wide definitions before reading any inline references.
    for source in &sources.0 {
        parser.source.clone_from(&source.name);
        parser.collect(&source.text.lines().collect::<Vec<_>>(), 0);
    }
    parser.select_active = sources
        .0
        .iter()
        .any(|source| parser.has_select_tags(&source.text.lines().collect::<Vec<_>>()));
    let mut blocks = Vec::new();
    for source in &sources.0 {
        parser.source.clone_from(&source.name);
        blocks.extend(parser.blocks(&source.text.lines().collect::<Vec<_>>(), 0)?);
    }
    Ok(Pandoc {
        meta: parser.meta,
        blocks,
    })
}

impl Parser<'_> {
    fn remaining(&self) -> u64 {
        self.context
            .limits
            .max_input_size
            .saturating_sub(self.used_bytes)
    }

    fn charge(&mut self, bytes: u64) -> Result<()> {
        charge_bytes(
            &mut self.used_bytes,
            bytes,
            self.context.limits.max_input_size,
        )
    }

    fn check_depth(&self, depth: usize) -> Result<()> {
        if depth >= self.context.limits.max_nesting_depth.min(STACK_LIMIT) {
            return Err(Error::Parse("Org content is nested too deeply.".into()));
        }
        Ok(())
    }

    fn enabled(&self, extension: Extension) -> bool {
        self.options.extensions.contains(extension)
    }

    fn setting<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.settings.get(key).map_or(default, String::as_str)
    }
}

fn charge_bytes(used: &mut u64, bytes: u64, limit: u64) -> Result<()> {
    *used = used
        .checked_add(bytes)
        .filter(|&total| total <= limit)
        .ok_or_else(|| Error::Parse("Org input exceeds the size limit.".into()))?;
    Ok(())
}
