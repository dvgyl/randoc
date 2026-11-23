mod blocks;
mod inlines;
mod tables;

use std::cell::RefCell;

use randoc_ast::Pandoc;
use randoc_core::shared::meta_context::meta_to_context;
use randoc_core::{Context, Error, Extension, Result, WrapOption, WriterOptions};
use randoc_layout::{Doc, chomp, literal, vsep};
use randoc_templates::Val;

use crate::shared::STACK_LIMIT;

struct Render<'a> {
    options: &'a WriterOptions,
    context: &'a mut Context,
    notes: Vec<Doc>,
    table_cells: usize,
    table_bytes: u64,
}

pub(super) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let mut writer = Render {
        options,
        context,
        notes: Vec::new(),
        table_cells: 0,
        table_bytes: 0,
    };
    let mut fields = if options.template.is_some() {
        let max_depth = writer.context.limits.max_nesting_depth.min(STACK_LIMIT);
        let shared = RefCell::new(&mut writer);
        meta_to_context(
            &doc.meta,
            &options.variables,
            max_depth,
            &mut |blocks| shared.borrow_mut().blocks(blocks, 0),
            &mut |inlines| shared.borrow_mut().inlines(inlines, 0),
        )?
    } else {
        randoc_templates::Context::new()
    };
    let mut org_options = randoc_templates::Context::new();
    if writer.enabled(Extension::SmartQuotes) {
        org_options.insert("'", literal("t"));
    }
    if !writer.enabled(Extension::SpecialStrings) {
        org_options.insert("-", literal("nil"));
    }
    if !org_options.0.is_empty() && fields.get("options").is_none() {
        fields.insert("options", Val::Map(org_options));
    }
    let body = writer.blocks(&doc.blocks, 0)?;
    let body = chomp(vsep([body, vsep(std::mem::take(&mut writer.notes))]));
    if let Some(template) = &options.template {
        fields.insert("body", Val::Simple(body));
        Ok(template.render(&fields).render(writer.width()))
    } else {
        Ok(body.render(writer.width()))
    }
}

impl Render<'_> {
    fn depth(&self, depth: usize) -> Result<()> {
        if depth >= self.context.limits.max_nesting_depth.min(STACK_LIMIT) {
            return Err(Error::Parse("Org content is nested too deeply.".into()));
        }
        Ok(())
    }

    fn width(&self) -> Option<usize> {
        (self.options.wrap_text == WrapOption::WrapAuto)
            .then(|| usize::try_from(self.options.columns).ok())
            .flatten()
    }

    fn enabled(&self, ext: Extension) -> bool {
        self.options.extensions.contains(ext)
    }
}
