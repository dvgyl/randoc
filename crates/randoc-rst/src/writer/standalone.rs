use std::cell::RefCell;

use randoc_ast::{Block, MetaValue, Pandoc};
use randoc_core::Result;
use randoc_core::shared::meta_context::meta_to_context;
use randoc_layout::{Doc, literal};
use randoc_templates::Val;

use super::{Writer, blocks::code_language};

impl Writer<'_> {
    pub(super) fn standalone(&mut self, doc: &Pandoc) -> Result<String> {
        let options = self.options;
        let max_depth = self.context.limits.max_nesting_depth.min(128);
        let shared = RefCell::new(&mut *self);
        let mut fields = meta_to_context(
            &doc.meta,
            &options.variables,
            max_depth,
            &mut |blocks| shared.borrow_mut().blocks(blocks, 0),
            &mut |inlines| shared.borrow_mut().inlines(inlines, 0),
        )?;
        if matches!(doc.meta.get("abstract"), Some(MetaValue::MetaBlocks(blocks))
            if matches!(blocks.last(), Some(Block::Para(_))))
            && abstract_needs_blank_line(doc.blocks.first())
            && options.variables.get("abstract").is_none()
            && let Some(Val::Simple(abstract_doc)) = fields.get("abstract")
        {
            fields.insert(
                "abstract",
                Val::Simple(abstract_doc.clone() + literal("\n\n")),
            );
        }
        let title = adorn(fields.get("title"), '=');
        let subtitle = adorn(fields.get("subtitle"), '-');
        let titleblock = title.above(subtitle);
        if !titleblock.is_empty() {
            fields.insert("titleblock", Val::Simple(titleblock));
        }
        let body = self.render_blocks(&doc.blocks, 0, true)?;
        fields.insert("body", Val::Simple(self.append_definitions(body)?));
        if self.raw_tex {
            fields.insert("rawtex", Val::Bool(true));
        }
        if options.table_of_contents {
            fields.insert("toc", Val::Bool(true));
        }
        if options.number_sections {
            fields.insert("number-sections", Val::Bool(true));
        }
        fields.insert("toc-depth", Val::text(options.toc_depth.to_string()));
        Ok(options
            .template
            .as_ref()
            .map_or_else(String::new, |t| t.render(&fields).render(self.width())))
    }
}

fn abstract_needs_blank_line(first_block: Option<&Block>) -> bool {
    match first_block {
        Some(Block::CodeBlock(attr, _)) => code_language(attr).is_none(),
        Some(Block::RawBlock(format, _)) => format == "rst",
        Some(
            Block::BulletList(_)
            | Block::OrderedList(..)
            | Block::DefinitionList(_)
            | Block::Table(_)
            | Block::Div(..)
            | Block::HorizontalRule,
        ) => false,
        _ => true,
    }
}

fn adorn(value: Option<&Val>, mark: char) -> Doc {
    let Some(Val::Simple(doc)) = value.filter(|v| matches!(v, Val::Simple(d) if !d.is_empty()))
    else {
        return Doc::default();
    };
    let rule = literal(mark.to_string().repeat(doc.offset()));
    rule.clone().above(doc.clone()).above(rule)
}
