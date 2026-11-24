mod blocks;
mod inlines;
mod standalone;
mod tables;

use std::collections::HashMap;

use randoc_ast::{Attr, Block, Pandoc, Text};
use randoc_core::{Context, Error, Result, WrapOption, WriterOptions};
use randoc_layout::{Doc, literal, nest, vcat, vsep};

struct Writer<'a> {
    options: &'a WriterOptions,
    context: &'a mut Context,
    notes: Vec<(Vec<Block>, usize)>,
    links: Vec<Doc>,
    link_urls: HashMap<String, String>,
    images: Vec<Doc>,
    image_number: usize,
    image_ids: HashMap<String, Image>,
    raw_tex: bool,
    in_citation: bool,
}

#[derive(PartialEq)]
struct Image {
    attr: Attr,
    url: String,
    target: Option<String>,
}

impl Writer<'_> {
    fn check_depth(&self, depth: usize) -> Result<()> {
        if depth >= self.context.limits.max_nesting_depth.min(128) {
            return Err(Error::Parse(
                "The reStructuredText document is nested too deeply.".into(),
            ));
        }
        Ok(())
    }

    fn width(&self) -> Option<usize> {
        (self.options.wrap_text == WrapOption::WrapAuto)
            .then(|| usize::try_from(self.options.columns).ok())
            .flatten()
    }

    fn append_definitions(&mut self, body: Doc) -> Result<Doc> {
        let mut notes = Vec::new();
        while notes.len() < self.notes.len() {
            let number = notes.len() + 1;
            let (blocks, depth) = std::mem::take(&mut self.notes[number - 1]);
            let body = self.blocks(&blocks, depth)?;
            notes.push(literal(format!(".. [{number}]")).above(nest(3, body)));
        }
        self.notes.clear();
        Ok(vsep([
            body,
            vsep(notes),
            vcat(std::mem::take(&mut self.links)),
            vcat(std::mem::take(&mut self.images)),
        ]))
    }
}

pub(super) fn write(doc: &Pandoc, options: &WriterOptions, context: &mut Context) -> Result<Text> {
    let mut writer = Writer {
        options,
        context,
        notes: Vec::new(),
        links: Vec::new(),
        link_urls: HashMap::new(),
        images: Vec::new(),
        image_number: 0,
        image_ids: HashMap::new(),
        raw_tex: false,
        in_citation: false,
    };
    if options.template.is_some() {
        writer.standalone(doc)
    } else {
        let body = writer.blocks(&doc.blocks, 0)?;
        let mut output = writer.append_definitions(body)?.render(writer.width());
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        Ok(output)
    }
}
