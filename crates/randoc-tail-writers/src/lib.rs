//! Additional document formats for Randoc.

#![forbid(unsafe_code)]

mod ansi;
mod bbcode;
mod chunked;
mod context;
mod highlight;
mod icml;
mod resource;
mod texinfo;
mod xml;

use std::sync::Arc;

use randoc_ast::{Block, Inline, Pandoc};
use randoc_core::shared::{chunks::inspect_tree, meta_context::meta_to_context};
use randoc_core::{Context, Error, Reader, Result, TemplateWriter, Writer, WriterOptions};
use randoc_layout::Doc;
use randoc_templates::{Template, Val};

/// Returns a reader for the requested format, if available.
pub fn reader(format: &str) -> Option<Reader> {
    (format == "xml").then(|| Reader::Text(Arc::new(xml::read)))
}

/// Returns a writer for the requested format, if available.
pub fn writer(format: &str) -> Option<Writer> {
    type Render = fn(&Pandoc, &WriterOptions, &mut Context) -> Result<String>;
    let render: Render = match format {
        "xml" => xml::write,
        "chunkedhtml" => return Some(Writer::Binary(Arc::new(chunked::ChunkedHtml))),
        "ansi" => ansi::write,
        "context" => context::write,
        "icml" => icml::write,
        "texinfo" => texinfo::write,
        name => {
            let dialect = bbcode::Dialect::from_name(name)?;
            return Some(Writer::Text(Arc::new(TemplateWriter(
                move |doc: &Pandoc, options: &WriterOptions, context: &mut Context| {
                    validate(doc, context)?;
                    bbcode::write(dialect, doc, options, context)
                },
            ))));
        }
    };
    Some(Writer::Text(Arc::new(TemplateWriter(
        move |doc: &Pandoc, options: &WriterOptions, context: &mut Context| {
            validate(doc, context)?;
            let output = render(doc, options, context)?;
            if output.len() as u64 > context.limits.max_input_size {
                return Err(output_limit_error());
            }
            Ok(output)
        },
    ))))
}

fn validate(doc: &Pandoc, context: &Context) -> Result<()> {
    inspect_tree(
        &doc.blocks,
        doc.meta.0.values(),
        context.limits.max_nesting_depth,
        |_| {},
    )
}

fn columns(options: &WriterOptions) -> Option<usize> {
    (options.wrap_text == randoc_core::WrapOption::WrapAuto)
        .then(|| usize::try_from(options.columns).ok())
        .flatten()
}

fn output_limit_error() -> Error {
    Error::App("The output exceeds the size limit.".into())
}

fn charge_output(remaining: &mut u64, bytes: usize) -> Result<()> {
    *remaining = remaining
        .checked_sub(bytes as u64)
        .ok_or_else(output_limit_error)?;
    Ok(())
}

fn check_output(bytes: usize, mut limit: u64) -> Result<()> {
    charge_output(&mut limit, bytes)
}

fn append_output(output: &mut String, text: &str, limit: u64) -> Result<()> {
    check_output(output.len().saturating_add(text.len()), limit)?;
    output.push_str(text);
    Ok(())
}

fn join_output(
    parts: impl IntoIterator<Item = Result<String>>,
    separator: &str,
    limit: u64,
    skip_empty: bool,
) -> Result<String> {
    let mut output = String::new();
    let mut first = true;
    for part in parts {
        let part = part?;
        if skip_empty && part.is_empty() {
            continue;
        }
        if !first {
            append_output(&mut output, separator, limit)?;
        }
        append_output(&mut output, &part, limit)?;
        first = false;
    }
    Ok(output)
}

fn check_layout(doc: &Doc, limit: u64) -> Result<()> {
    check_output(doc.estimated_size(), limit)
}

fn render_template(
    doc: &Pandoc,
    options: &WriterOptions,
    limits: randoc_core::Limits,
    body: Doc,
    blocks: &mut dyn FnMut(&[Block]) -> Result<Doc>,
    inlines: &mut dyn FnMut(&[Inline]) -> Result<Doc>,
    fields: impl IntoIterator<Item = (&'static str, Val)>,
) -> Result<String> {
    render_template_with(doc, options, limits, body, blocks, inlines, |scope| {
        set_template_defaults(scope, fields);
    })
}

fn set_template_defaults(
    scope: &mut randoc_templates::Context,
    fields: impl IntoIterator<Item = (&'static str, Val)>,
) {
    for (name, value) in fields {
        if scope.get(name).is_none() {
            scope.insert(name, value);
        }
    }
}

fn render_template_with(
    doc: &Pandoc,
    options: &WriterOptions,
    limits: randoc_core::Limits,
    body: Doc,
    blocks: &mut dyn FnMut(&[Block]) -> Result<Doc>,
    inlines: &mut dyn FnMut(&[Inline]) -> Result<Doc>,
    set_fields: impl FnOnce(&mut randoc_templates::Context),
) -> Result<String> {
    let width = columns(options);
    let limit = usize::try_from(limits.max_input_size).unwrap_or(usize::MAX);
    let Some(template) = &options.template else {
        return if width.is_none() {
            body.render_limited(limit).ok_or_else(output_limit_error)
        } else {
            Ok(body.render(width))
        };
    };
    let mut scope = meta_to_context(
        &doc.meta,
        &options.variables,
        limits.max_nesting_depth,
        blocks,
        inlines,
    )?;
    set_fields(&mut scope);
    scope.insert("body", Val::Simple(body));
    if width.is_none() {
        return template
            .render_limited(&scope, limit, limit)
            .ok_or_else(output_limit_error);
    }
    let rendered = template.render(&scope);
    check_layout(&rendered, limits.max_input_size)?;
    Ok(rendered.render(width))
}

fn default_template(format: &str, depth: usize) -> Result<Template> {
    let source = randoc_data::default_template(format)
        .ok_or_else(|| Error::Template(format!("No template is available for {format}.")))?;
    Template::compile(format, source, depth, &mut |name: &str| {
        randoc_data::data_file(&format!("templates/{name}"))
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .map(str::to_owned)
            .ok_or_else(|| format!("The template partial {name} is unavailable."))
    })
    .map_err(|error| Error::Template(error.to_string()))
}

fn rows(table: &randoc_ast::Table) -> impl Iterator<Item = (bool, &randoc_ast::Row)> {
    table
        .head
        .rows
        .iter()
        .map(|row| (true, row))
        .chain(table.bodies.iter().flat_map(|body| {
            body.head
                .iter()
                .map(|row| (true, row))
                .chain(body.body.iter().map(|row| (false, row)))
        }))
        .chain(table.foot.rows.iter().map(|row| (false, row)))
}

#[cfg(test)]
mod tests;
