mod eval;
mod lower;
mod math;
mod parser;
mod symbols;

use std::collections::BTreeSet;

use randoc_ast::*;
use randoc_core::shared::{
    CommonmarkIdentifierSet, IdentifierSet, auto_identifier, commonmark_auto_identifier,
};
use randoc_core::{Context, Error, Extension, ReaderOptions, Result, Sources};

pub(super) fn read(
    sources: &Sources,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let limit = context.limits.max_input_size;
    let size = sources
        .0
        .iter()
        .try_fold(0usize, |sum, source| sum.checked_add(source.text.len()));
    if size.is_none_or(|n| n as u64 > limit) {
        return Err(parser::error(
            "",
            0,
            "The Typst input exceeds the size limit.",
        ));
    }
    let depth = context.limits.max_nesting_depth;
    let text = if let [source] = sources.0.as_slice() {
        source.text.as_str().into()
    } else {
        std::borrow::Cow::Owned(sources.concat())
    };
    let name = sources.0.first().map_or("", |source| source.name.as_str());
    let marks = parser::parse(&text, name, depth)?;
    let mut doc = eval::evaluate(&text, name, &marks, depth, options, context)?;
    let mut labels = BTreeSet::new();
    randoc_ast::walk::query_blocks(&doc, |block| match block {
        Block::Header(_, attr, _) | Block::Figure(attr, ..) | Block::Div(attr, _)
            if !attr.id.is_empty() =>
        {
            labels.insert(attr.id.clone());
        }
        _ => {}
    });
    randoc_ast::walk::query_inlines(&doc, |inline| {
        if let Inline::Span(attr, _) = inline
            && !attr.id.is_empty()
        {
            labels.insert(attr.id.clone());
        }
    });
    randoc_ast::walk::walk_mut(&mut doc, &mut References(&labels));
    if options.extensions.contains(Extension::AutoIdentifiers) {
        let mut pandoc = IdentifierSet::new();
        let mut gfm = CommonmarkIdentifierSet::with_explicit(labels.iter().map(String::as_str));
        randoc_ast::walk::walk_blocks_mut(&mut doc, |block| {
            if let Block::Header(_, attr, xs) = block {
                if attr.id.is_empty() {
                    attr.id = if options.extensions.contains(Extension::GfmAutoIdentifiers) {
                        gfm.unique(&commonmark_auto_identifier(xs, options.extensions))
                    } else {
                        pandoc.unique(&auto_identifier(xs, options.extensions))
                    };
                } else {
                    pandoc.register(&attr.id);
                }
            }
        });
    }
    Ok(doc)
}

struct References<'a>(&'a BTreeSet<String>);

impl randoc_ast::walk::VisitorMut for References<'_> {
    fn inline(&mut self, inline: &mut Inline) {
        if let Inline::Link(attr, _, target) = inline
            && attr.has_class("typst-ref")
        {
            let id = target.url.trim_start_matches('#');
            if self.0.contains(id) {
                attr.classes[0] = "ref".into();
            } else {
                *inline = lower::citation(id, CitationMode::NormalCitation, vec![]);
            }
        }
    }

    fn inlines(&mut self, inlines: &mut Vec<Inline>) {
        let mut merged = Vec::with_capacity(inlines.len());
        let mut adjacent = false;
        let mut whitespace = Vec::new();
        for inline in std::mem::take(inlines) {
            match inline {
                Inline::Cite(mut citations, mut fallback) if adjacent => {
                    if let Some(Inline::Cite(prior, text)) = merged.last_mut() {
                        prior.append(&mut citations);
                        text.append(&mut fallback);
                    }
                    whitespace.clear();
                }
                Inline::Space | Inline::SoftBreak if adjacent => whitespace.push(inline),
                inline => {
                    merged.append(&mut whitespace);
                    adjacent = matches!(inline, Inline::Cite(..));
                    merged.push(inline);
                }
            }
        }
        merged.append(&mut whitespace);
        *inlines = merged;
    }
}

fn limit_error(pos: usize, source: &str) -> Error {
    parser::error(source, pos, "The Typst evaluation limit was exceeded.")
}
