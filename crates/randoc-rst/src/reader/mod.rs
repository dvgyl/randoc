mod blocks;
mod directives;
mod inlines;
mod postprocess;
mod tables;

use std::collections::{HashMap, HashSet, VecDeque};

use randoc_ast::{Block, Inline, Pandoc};
use randoc_core::shared::IdentifierSet;
use randoc_core::{Context, Error, Extension, ReaderOptions, Result, SourcePos, Sources};

type Options = Vec<(String, String)>;

#[derive(Clone, Copy)]
struct Line<'a> {
    text: &'a str,
    source: &'a str,
    number: u32,
    column: u32,
}

impl<'a> Line<'a> {
    fn with(self, text: &'a str) -> Self {
        Self { text, ..self }
    }

    fn cut(self, bytes: usize) -> Self {
        Self {
            text: self.text.get(bytes..).unwrap_or_default(),
            column: self
                .column
                .saturating_add(u32::try_from(bytes).unwrap_or(u32::MAX)),
            ..self
        }
    }

    fn pos(self) -> SourcePos {
        SourcePos::new(self.source, self.number, self.column)
    }

    fn indent(self) -> usize {
        self.text.len() - self.text.trim_start_matches(' ').len()
    }

    fn blank(self) -> bool {
        self.text.trim().is_empty()
    }
}

fn lines<'a>(text: &'a str, source: &'a str, start: u32) -> impl Iterator<Item = Line<'a>> {
    text.lines().enumerate().map(move |(i, text)| Line {
        text: text.trim_end_matches('\r'),
        source,
        number: start.saturating_add(u32::try_from(i).unwrap_or(u32::MAX)),
        column: 1,
    })
}

fn expand_tabs(text: &str, stop: i64) -> String {
    let stop = usize::try_from(stop).unwrap_or(4).clamp(1, 256);
    if !text.contains('\t') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut column = 0;
    for c in text.chars() {
        match c {
            '\t' => {
                out.extend(std::iter::repeat_n(' ', stop - column));
                column = 0;
            }
            '\n' => {
                out.push(c);
                column = 0;
            }
            _ => {
                out.push(c);
                column = (column + 1) % stop;
            }
        }
    }
    out
}

#[derive(Clone)]
enum TargetDef {
    Url(String),
    Indirect(String),
}

#[derive(Clone)]
enum Reference {
    Link(String, Vec<Inline>),
    Anonymous(Vec<Inline>),
    Note(String),
    Substitution(String),
}

#[derive(Clone)]
struct Role {
    base: String,
    fields: Options,
}

struct Parser<'a> {
    options: &'a ReaderOptions,
    context: &'a mut Context,
    adornments: Vec<(char, bool)>,
    ids: IdentifierSet,
    targets: HashMap<String, TargetDef>,
    anonymous: VecDeque<TargetDef>,
    substitutions: HashMap<String, Vec<Block>>,
    notes: HashMap<String, Vec<Block>>,
    auto_notes: HashMap<String, VecDeque<Vec<Block>>>,
    citations: Vec<(String, Vec<Block>)>,
    roles: HashMap<String, Role>,
    default_role: String,
    highlight: Option<String>,
    end_pos: SourcePos,
    references: Vec<(Option<Reference>, SourcePos)>,
    includes: HashSet<std::path::PathBuf>,
    bytes: u64,
    cells: usize,
}

fn key(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn attr(id: impl Into<String>, classes: &[&str]) -> randoc_ast::Attr {
    randoc_ast::Attr::new(
        id,
        classes.iter().map(ToString::to_string).collect(),
        Vec::new(),
    )
}

fn str_inline(text: impl Into<String>) -> Inline {
    Inline::Str(text.into())
}

fn too_large() -> Error {
    Error::Parse("The reStructuredText input exceeds the byte limit.".into())
}

impl Parser<'_> {
    fn check_depth(&self, depth: usize) -> Result<()> {
        if depth >= self.context.limits.max_nesting_depth.min(128) {
            return Err(Error::Parse(
                "The reStructuredText document is nested too deeply.".into(),
            ));
        }
        Ok(())
    }

    fn enabled(&self, extension: Extension) -> bool {
        self.options.extensions.contains(extension)
    }

    fn reference(&mut self, reference: Reference, _pos: &SourcePos) -> Inline {
        let index = self.references.len();
        self.references
            .push((Some(reference), self.end_pos.clone()));
        Inline::Span(attr(index.to_string(), &["\0rst-reference"]), Vec::new())
    }
}

pub(super) fn read(
    sources: &Sources,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let bytes = sources.0.iter().fold(0_u64, |total, source| {
        total.saturating_add(source.text.len() as u64)
    });
    if bytes > context.limits.max_input_size {
        return Err(too_large());
    }

    let expanded: Vec<_> = sources
        .0
        .iter()
        .map(|s| expand_tabs(&s.text, options.tab_stop))
        .collect();
    let mut input = Vec::new();
    for (source, text) in sources.0.iter().zip(&expanded) {
        input.extend(lines(text, &source.name, 1));
        input.push(Line {
            text: "",
            source: &source.name,
            number: 0,
            column: 1,
        });
    }

    let end_pos = input
        .iter()
        .rev()
        .find(|line| line.number != 0)
        .map_or_else(SourcePos::default, |l| {
            SourcePos::new(l.source, l.number.saturating_add(2), 1)
        });
    let mut parser = Parser {
        options,
        context,
        bytes,
        cells: 0,
        adornments: Vec::new(),
        ids: IdentifierSet::new(),
        targets: HashMap::new(),
        anonymous: VecDeque::new(),
        substitutions: HashMap::new(),
        notes: HashMap::new(),
        auto_notes: HashMap::new(),
        citations: Vec::new(),
        roles: HashMap::new(),
        highlight: None,
        end_pos,
        default_role: "title-reference".into(),
        references: Vec::new(),
        includes: HashSet::new(),
    };
    let blocks = parser.blocks(&input, 0)?;
    parser.finish(blocks)
}
