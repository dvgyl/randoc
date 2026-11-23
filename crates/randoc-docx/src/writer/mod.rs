mod blocks;
mod inlines;
mod limits;
mod media;
mod metadata;
mod notes;
mod numbering;
mod package;
mod styles;
mod tables;
mod xml;

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write;

use randoc_archive::media::sha1_hex;
use randoc_archive::opc::Relationship;
use randoc_ast::Pandoc;
use randoc_core::{BinaryWriter, Context, Error, Result, WriterOptions};
use randoc_highlight::Registry;

use self::xml::{Budget, Xml};

pub(crate) struct DocxWriter;

impl BinaryWriter for DocxWriter {
    fn renders_template(&self) -> bool {
        true
    }

    fn write(
        &self,
        doc: &Pandoc,
        options: &WriterOptions,
        context: &mut Context,
    ) -> Result<Vec<u8>> {
        limits::validate_depth(doc, context.limits.max_nesting_depth)?;
        let reference = package::Reference::load(options, context)?;
        let clock = package::timestamp(context);
        let budget = Budget::new(reference.remaining_bytes(context));
        let (relations, next_id) = reference.document_relationships();
        let mut renderer = Renderer {
            options,
            context,
            styles: styles::Styles::new(reference.styles.as_ref(), options, &budget)?,
            numbering: numbering::Numbering::new(),
            registry: styles::registry(options)?,
            notes: budget.xml(),
            comments: budget.xml(),
            budget,
            relations,
            next_id,
            links: HashMap::new(),
            comment_ids: HashMap::new(),
            used_comment_ids: HashSet::new(),
            media: BTreeMap::new(),
            inherited_parts: reference.part_names(),
            first: true,
            table_cells: 0,
            headings: [0; 10],
            figure_count: 0,
            table_count: 0,
            width: reference.text_width(),
        };
        let document = renderer.document(doc, &reference.section)?;
        renderer.finish(reference, document, &doc.meta, clock)
    }
}

struct Renderer<'a> {
    options: &'a WriterOptions,
    context: &'a mut Context,
    budget: Budget,
    styles: styles::Styles,
    numbering: numbering::Numbering,
    registry: Cow<'static, Registry>,
    notes: Xml,
    comments: Xml,
    relations: Vec<Relationship>,
    next_id: u64,
    links: HashMap<String, u64>,
    comment_ids: HashMap<String, u64>,
    used_comment_ids: HashSet<u64>,
    media: BTreeMap<String, media::Image>,
    inherited_parts: HashSet<String>,
    first: bool,
    table_cells: usize,
    headings: [i64; 10],
    figure_count: usize,
    table_count: usize,
    width: i64,
}

impl Renderer<'_> {
    fn depth(&self, depth: usize) -> Result<()> {
        if depth >= self.context.limits.max_nesting_depth {
            return Err(Error::Parse("DOCX content is nested too deeply.".into()));
        }
        self.budget.check()
    }

    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn relation(&mut self, id: u64, kind: &str, target: &str, external: bool) -> Result<()> {
        if self.relations.len() >= self.context.limits.max_archive_entries {
            return Err(Error::Parse("DOCX relationship limit exceeded.".into()));
        }
        self.budget.charge(target.len().saturating_add(128))?;
        self.relations
            .push(package::relationship(id, kind, target, external));
        Ok(())
    }

    fn bookmark(&mut self, out: &mut Xml, name: &str) -> Option<u64> {
        if name.is_empty() {
            return None;
        }
        let id = self.id();
        let _ = write!(
            out,
            "<w:bookmarkStart w:id=\"{id}\" w:name=\"{}\" />",
            bookmark_name(name)
        );
        Some(id)
    }
}

impl Renderer<'_> {
    /// Starts a bookmark that surrounds whole blocks: a top-level element.
    fn block_bookmark(&mut self, out: &mut Xml, name: &str) -> Option<u64> {
        if !name.is_empty() {
            out.element();
        }
        self.bookmark(out, name)
    }
}

fn bookmark_name(name: &str) -> String {
    if name.chars().count() < 40 && name.chars().all(|ch| ch.is_alphanumeric() || ch == '_') {
        format!("_{name}")
    } else {
        let hash = sha1_hex(name.as_bytes());
        format!("_{}", &hash[1..])
    }
}
