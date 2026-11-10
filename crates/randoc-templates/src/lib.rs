//! The Pandoc template language for Randoc.
//!
//! This crate is a port of the Haskell library `doctemplates` 0.11.1, which
//! Pandoc 3.13 uses for standalone output. [`Template::compile`] parses a
//! template, and [`Template::render`] fills it with the values of a
//! [`Context`]. The result is a [`Doc`], which the writer renders at its
//! line width.

use std::collections::BTreeMap;
use std::fmt;

use randoc_ast::{Block, Inline, Meta, MetaValue};
use randoc_layout::{Doc, literal};

mod parsec;
mod parser;
mod render;

/// A value that a template can use.
#[derive(Debug, PartialEq, Default)]
pub enum Val {
    /// Text, which can contain layout information.
    Simple(Doc),
    /// A list of values.
    List(Vec<Val>),
    /// A map from a name to a value.
    Map(Context),
    /// A Boolean value.
    Bool(bool),
    /// No value.
    #[default]
    Null,
}

/// Counts the work of the renderer in test builds, so that a test can
/// compare the work for two input sizes without a clock.
#[cfg(test)]
pub(crate) mod steps {
    use std::cell::Cell;

    thread_local! {
        static STEPS: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) fn step() {
        STEPS.with(|s| s.set(s.get() + 1));
    }

    /// Runs `f` and returns the number of steps that it took.
    pub(crate) fn count(f: impl FnOnce()) -> usize {
        let before = STEPS.with(Cell::get);
        f();
        STEPS.with(Cell::get) - before
    }
}

impl Clone for Val {
    fn clone(&self) -> Val {
        #[cfg(test)]
        steps::step();
        match self {
            Val::Simple(d) => Val::Simple(d.clone()),
            Val::List(xs) => Val::List(xs.clone()),
            Val::Map(m) => Val::Map(m.clone()),
            Val::Bool(b) => Val::Bool(*b),
            Val::Null => Val::Null,
        }
    }
}

impl Val {
    /// Makes a text value from literal text, as Pandoc does for a `-V`
    /// value.
    pub fn text(text: impl Into<String>) -> Val {
        Val::Simple(literal(text))
    }

    /// Converts a metadata value. `blocks` and `inlines` are the writer
    /// functions for block and inline content. A `MetaString` goes through
    /// `inlines`, split into words as the Pandoc builder `text` does.
    pub fn from_meta_value(
        value: &MetaValue,
        blocks: &mut dyn FnMut(&[Block]) -> Doc,
        inlines: &mut dyn FnMut(&[Inline]) -> Doc,
    ) -> Val {
        match value {
            MetaValue::MetaMap(map) => Val::Map(Context(
                map.iter()
                    .map(|(k, v)| (k.clone(), Val::from_meta_value(v, blocks, inlines)))
                    .collect(),
            )),
            MetaValue::MetaList(items) => Val::List(
                items
                    .iter()
                    .map(|v| Val::from_meta_value(v, blocks, inlines))
                    .collect(),
            ),
            MetaValue::MetaBool(b) => Val::Bool(*b),
            MetaValue::MetaString(s) => Val::Simple(inlines(&text_inlines(s))),
            MetaValue::MetaInlines(xs) => Val::Simple(inlines(xs)),
            MetaValue::MetaBlocks(xs) => Val::Simple(blocks(xs)),
        }
    }
}

/// The Pandoc builder function `text`: runs of spaces become `Space`, or
/// `SoftBreak` when the run has a line break, and other runs become `Str`.
fn text_inlines(s: &str) -> Vec<Inline> {
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r');
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(first) = rest.chars().next() {
        let spacy = is_space(first);
        let end = rest
            .find(|c: char| is_space(c) != spacy)
            .unwrap_or(rest.len());
        let (run, tail) = rest.split_at(end);
        out.push(if !spacy {
            Inline::Str(run.to_owned())
        } else if run.contains(['\n', '\r']) {
            Inline::SoftBreak
        } else {
            Inline::Space
        });
        rest = tail;
    }
    out
}

impl From<&str> for Val {
    fn from(text: &str) -> Val {
        Val::text(text)
    }
}

impl From<bool> for Val {
    fn from(b: bool) -> Val {
        Val::Bool(b)
    }
}

impl From<Doc> for Val {
    fn from(doc: Doc) -> Val {
        Val::Simple(doc)
    }
}

/// The variables that a template can use.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Context(pub BTreeMap<String, Val>);

impl Context {
    /// Makes an empty context.
    pub fn new() -> Context {
        Context::default()
    }

    /// Sets a variable and returns the value that it replaces.
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<Val>) -> Option<Val> {
        self.0.insert(name.into(), value.into())
    }

    /// Returns the value of a variable.
    pub fn get(&self, name: &str) -> Option<&Val> {
        self.0.get(name)
    }

    /// Converts document metadata, as Pandoc's `metaToContext` does
    /// before the writer adds its own variables.
    pub fn from_meta(
        meta: &Meta,
        blocks: &mut dyn FnMut(&[Block]) -> Doc,
        inlines: &mut dyn FnMut(&[Inline]) -> Doc,
    ) -> Context {
        Context(
            meta.0
                .iter()
                .map(|(k, v)| (k.clone(), Val::from_meta_value(v, blocks, inlines)))
                .collect(),
        )
    }
}

/// The reason that a template does not compile.
#[derive(Debug, Clone, PartialEq)]
pub enum CompileError<E> {
    /// A syntax error. The text is the text of `doctemplates`, for example
    /// `"t.tpl" (line 1, column 6):` and the Parsec messages. Pandoc
    /// prints it as `Error compiling template 'TEXT'` and exits with code 5.
    Syntax(String),
    /// The loader returned an error for a partial.
    Partial(E),
    /// The directives are nested more deeply than `max_depth`.
    TooDeep,
}

impl<E: fmt::Display> fmt::Display for CompileError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::Syntax(text) => f.write_str(text),
            CompileError::Partial(e) => e.fmt(f),
            CompileError::TooDeep => f.write_str("the template is nested too deeply"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for CompileError<E> {}

/// A compiled template.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Template {
    source: String,
    node: render::Node,
}

impl Template {
    /// Returns the template source text.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Compiles a template.
    ///
    /// `path` is the template path. It is the name in error messages, and
    /// it gives the directory and the default extension of a partial:
    /// `$name()$` in `dir/t.html` asks `load` for `dir/name.html`. `load`
    /// returns the text of a partial; the caller decides where partials
    /// come from.
    ///
    /// Pandoc removes each carriage return (CR) and a byte order mark from
    /// the text of a template file and of each partial file. The caller
    /// does the same for `source` and in `load`. The crate removes one
    /// final newline of a partial.
    ///
    /// `max_depth` limits the nesting of directives (`if`, `elseif`, `for`,
    /// `$^$` and partials). A deeper template gives
    /// [`CompileError::TooDeep`]. Compilation needs about 10 KB of stack for
    /// each level in a debug build, so the depth 128 runs on a thread with
    /// a 2 MB stack. Rendering, cloning and dropping need less.
    pub fn compile<E>(
        path: &str,
        source: &str,
        max_depth: usize,
        load: &mut dyn FnMut(&str) -> Result<String, E>,
    ) -> Result<Template, CompileError<E>> {
        match parser::compile(path, source, max_depth, load) {
            Ok(node) => Ok(Template {
                source: source.to_owned(),
                node,
            }),
            Err(Ok(text)) => Err(CompileError::Syntax(text)),
            Err(Err(parser::Fatal::Load(e))) => Err(CompileError::Partial(e)),
            Err(Err(parser::Fatal::TooDeep)) => Err(CompileError::TooDeep),
        }
    }

    /// Renders the template with the values of a context.
    pub fn render(&self, context: &Context) -> Doc {
        render::render(&self.node, context)
    }
}

#[cfg(test)]
mod tests;
