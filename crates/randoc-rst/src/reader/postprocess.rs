use std::collections::{HashMap, HashSet};

use randoc_ast::{Attr, Block, Inline, Meta, MetaValue, Pandoc, Row, Target};
use randoc_core::shared::stringify;
use randoc_core::{Error, LogMessage, Result, SourcePos};

use super::inlines::words;
use super::{Parser, Reference, TargetDef, attr, key, str_inline, too_large};

#[derive(Default)]
struct Active {
    substitutions: HashSet<String>,
    notes: HashSet<String>,
}

fn reference_index(inline: &Inline) -> Option<usize> {
    match inline {
        Inline::Span(a, _) if a.has_class("\0rst-reference") => a.id.parse().ok(),
        _ => None,
    }
}

fn is_footnote(label: &str) -> bool {
    label == "*" || label.starts_with('#') || label.bytes().all(|b| b.is_ascii_digit())
}

fn pending_reference(reference: Reference, out: &mut Vec<Inline>) -> (Vec<Inline>, String) {
    match reference {
        Reference::Link(_, label) => {
            let url = format!("##REF##{}", stringify(&label));
            (label, url)
        }
        Reference::Anonymous(label) => (label, "##REF##_".into()),
        Reference::Note(label) if is_footnote(&label) => {
            if matches!(out.last(), Some(Inline::Space)) {
                out.pop();
            }
            let url = format!("##NOTE##{label}");
            (vec![str_inline(label)], url)
        }
        Reference::Note(label) => {
            let label = format!("[{label}]");
            let url = format!("##REF##{label}");
            (vec![str_inline(label)], url)
        }
        Reference::Substitution(name, _) => {
            let label = words(&format!("|{name}|"), false);
            let url = format!("##SUBST##{}", stringify(&label));
            (label, url)
        }
    }
}

impl Parser<'_> {
    pub(super) fn finish(&mut self, mut blocks: Vec<Block>) -> Result<Pandoc> {
        let citations = std::mem::take(&mut self.citations);
        let citation_names = citations.iter().map(|(k, _)| (key(k), k.clone())).collect();
        if !citations.is_empty() {
            let definitions = citations
                .into_iter()
                .map(|(label, blocks)| {
                    let term =
                        Inline::Span(attr(&label, &["citation-label"]), vec![str_inline(label)]);
                    (vec![term], vec![blocks])
                })
                .collect();
            blocks.push(Block::Div(
                attr("citations", &[]),
                vec![Block::DefinitionList(definitions)],
            ));
        }
        let mut resolver = Resolver {
            active: Active::default(),
            in_block_substitution: false,
            citation_names,
            remaining: usize::try_from(self.bytes)
                .unwrap_or(usize::MAX)
                .saturating_mul(32)
                .max(1024)
                .min(
                    usize::try_from(self.context.limits.max_input_size)
                        .unwrap_or(usize::MAX)
                        .saturating_mul(32),
                ),
            parser: self,
        };
        resolver.blocks(&mut blocks, 0)?;
        let meta = if self.options.standalone {
            promote(&mut blocks)
        } else {
            Meta::default()
        };
        randoc_ast::walk::walk_blocks_mut(&mut blocks, |block| {
            if let Block::Div(a, inner) = block
                && a.has_class("\0rst-fields")
                && let Some(field_list) = inner.pop()
            {
                *block = field_list;
            }
        });
        Ok(Pandoc { meta, blocks })
    }
}

struct Resolver<'p, 'a> {
    parser: &'p mut Parser<'a>,
    active: Active,
    in_block_substitution: bool,
    citation_names: HashMap<String, String>,
    remaining: usize,
}

impl Resolver<'_, '_> {
    fn charge(&mut self, amount: usize) -> Result<()> {
        self.remaining = self.remaining.checked_sub(amount).ok_or_else(too_large)?;
        Ok(())
    }

    fn blocks(&mut self, blocks: &mut Vec<Block>, depth: usize) -> Result<()> {
        self.parser.check_depth(depth)?;
        let mut out = Vec::with_capacity(blocks.len());
        for mut block in std::mem::take(blocks) {
            self.charge(1)?;
            if let Block::Para(inlines) | Block::Plain(inlines) = &block
                && let [inline] = inlines.as_slice()
                && let Some(index) = reference_index(inline)
                && let Some((Some(Reference::Substitution(name, false)), pos)) =
                    self.parser.references.get(index)
            {
                let (name, pos) = (name.clone(), pos.clone());
                // The first block reference opens the replacement as blocks.
                // Track the references inside it to detect a loop.
                let replacement =
                    if self.active.substitutions.is_empty() && !self.in_block_substitution {
                        let mut replacement = self.parser.substitutions.get(&key(&name)).cloned();
                        if let Some(blocks) = &mut replacement {
                            self.in_block_substitution = true;
                            self.blocks(blocks, depth + 1)?;
                            self.in_block_substitution = false;
                        }
                        replacement
                    } else {
                        self.substitution(&name, &pos, depth + 1)?
                    };
                if let Some(replacement) = replacement {
                    out.extend(replacement);
                } else {
                    self.missing(&format!("\"{}\"", key(&name)), &pos);
                    out.push(Block::Para(vec![str_inline(format!("|{name}|"))]));
                }
                continue;
            }
            match &mut block {
                Block::Para(ils) | Block::Plain(ils) | Block::Header(_, _, ils) => {
                    self.inlines(ils, depth)?
                }
                Block::CodeBlock(_, text) | Block::RawBlock(_, text) => self.charge(text.len())?,
                Block::LineBlock(lines) => {
                    for line in lines {
                        self.inlines(line, depth)?;
                    }
                }
                Block::BlockQuote(blocks) | Block::Div(_, blocks) => {
                    self.blocks(blocks, depth + 1)?
                }
                Block::Figure(_, caption, blocks) => {
                    self.blocks(&mut caption.long, depth + 1)?;
                    self.blocks(blocks, depth + 1)?;
                }
                Block::OrderedList(_, items) | Block::BulletList(items) => {
                    for item in items {
                        self.blocks(item, depth + 1)?;
                    }
                }
                Block::DefinitionList(items) => {
                    for (term, definitions) in items {
                        self.inlines(term, depth)?;
                        for definition in definitions {
                            self.blocks(definition, depth + 1)?;
                        }
                    }
                }
                Block::Table(table) => {
                    self.blocks(&mut table.caption.long, depth + 1)?;
                    self.rows(&mut table.head.rows, depth + 1)?;
                    for body in &mut table.bodies {
                        self.rows(&mut body.head, depth + 1)?;
                        self.rows(&mut body.body, depth + 1)?;
                    }
                    self.rows(&mut table.foot.rows, depth + 1)?;
                }
                Block::HorizontalRule => {}
            }
            out.push(block);
        }
        *blocks = out;
        Ok(())
    }

    fn rows(&mut self, rows: &mut [Row], depth: usize) -> Result<()> {
        for row in rows {
            for cell in &mut row.cells {
                self.blocks(&mut cell.content, depth)?;
            }
        }
        Ok(())
    }

    fn inlines(&mut self, inlines: &mut Vec<Inline>, depth: usize) -> Result<()> {
        self.resolve_inlines(inlines, depth, true)
    }

    fn resolve_inlines(
        &mut self,
        inlines: &mut Vec<Inline>,
        depth: usize,
        resolve_references: bool,
    ) -> Result<()> {
        self.parser.check_depth(depth)?;
        let mut out = Vec::with_capacity(inlines.len());
        for mut inline in std::mem::take(inlines) {
            self.charge(1)?;
            if let Some(index) = reference_index(&inline) {
                let Some((Some(reference), pos)) = self.parser.references.get(index).cloned()
                else {
                    continue;
                };
                if !resolve_references {
                    let (label, url) = pending_reference(reference, &mut out);
                    self.charge(url.len())?;
                    out.push(Inline::Link(Attr::default(), label, Target::new(url, "")));
                    continue;
                }
                match reference {
                    Reference::Link(key, label) => {
                        let url = self.url(Some(&key), None, &pos);
                        inline = Inline::Link(Attr::default(), label, Target::new(url, ""));
                    }
                    Reference::Anonymous(label) => {
                        let Some(target) = self.parser.anonymous.pop_front() else {
                            let location = if pos.source.is_empty() {
                                String::new()
                            } else {
                                format!("{:?} ", pos.source)
                            };
                            return Err(Error::Parse(format!(
                                "Error at {location}(line {}, column 1):\nunexpected end of input\nexpecting end of input\n\n^",
                                pos.line
                            )));
                        };
                        let url = self.url(None, Some(target), &pos);
                        inline = Inline::Link(Attr::default(), label, Target::new(url, ""));
                    }
                    Reference::Note(label) => {
                        let name = key(&label);
                        if let Some(id) = self.citation_names.get(&name) {
                            inline = Inline::Link(
                                attr("", &["citation"]),
                                vec![str_inline(format!("[{label}]"))],
                                Target::new(format!("#{id}"), ""),
                            );
                        } else if is_footnote(&label) {
                            if matches!(out.last(), Some(Inline::Space)) {
                                out.pop();
                            }
                            let repeated = self.active.notes.contains(&name);
                            let note = if repeated {
                                None
                            } else if name == "#" || name == "*" {
                                self.parser
                                    .auto_notes
                                    .get_mut(&name)
                                    .and_then(|notes| notes.pop_front())
                            } else {
                                self.parser.notes.get(&name).cloned()
                            };
                            inline = if let Some(mut note) = note {
                                self.active.notes.insert(name.clone());
                                self.blocks(&mut note, depth + 1)?;
                                self.active.notes.remove(&name);
                                Inline::Note(note)
                            } else {
                                if !repeated {
                                    self.missing(&label, &pos);
                                }
                                Inline::Link(
                                    Attr::default(),
                                    vec![str_inline(&label)],
                                    Target::new(format!("##NOTE##{label}"), ""),
                                )
                            };
                        } else {
                            self.missing(&name, &pos);
                            inline = Inline::Link(
                                Attr::default(),
                                vec![str_inline(format!("[{label}]"))],
                                Target::new("", ""),
                            );
                        }
                    }
                    Reference::Substitution(name, linked) => {
                        let content = self.substitution(&name, &pos, depth + 1)?;
                        let mut inlines = match content.and_then(|blocks| blocks.into_iter().next())
                        {
                            Some(Block::Para(ils) | Block::Plain(ils)) => ils,
                            Some(_) => Vec::new(),
                            None => {
                                self.missing(&format!("\"{}\"", key(&name)), &pos);
                                inline = Inline::Span(
                                    Attr::default(),
                                    vec![str_inline(format!("|{name}|"))],
                                );
                                out.push(inline);
                                continue;
                            }
                        };
                        let content = if inlines.len() == 1 {
                            inlines.remove(0)
                        } else {
                            Inline::Span(Attr::default(), inlines)
                        };
                        inline = if linked {
                            let url = self.url(Some(&key(&name)), None, &pos);
                            Inline::Link(Attr::default(), vec![content], Target::new(url, ""))
                        } else {
                            content
                        };
                    }
                }
            } else {
                match &mut inline {
                    Inline::Str(s)
                    | Inline::Code(_, s)
                    | Inline::RawInline(_, s)
                    | Inline::Math(_, s) => self.charge(s.len())?,
                    Inline::Emph(ils)
                    | Inline::Underline(ils)
                    | Inline::Strong(ils)
                    | Inline::Strikeout(ils)
                    | Inline::Superscript(ils)
                    | Inline::Subscript(ils)
                    | Inline::SmallCaps(ils)
                    | Inline::Quoted(_, ils)
                    | Inline::Span(_, ils)
                    | Inline::Image(_, ils, _)
                    | Inline::Link(_, ils, _) => {
                        self.resolve_inlines(ils, depth + 1, resolve_references)?
                    }
                    _ => {}
                }
            }
            out.push(inline);
        }
        *inlines = out;
        Ok(())
    }

    fn missing(&mut self, key: &str, pos: &SourcePos) {
        self.parser
            .context
            .report(LogMessage::ReferenceNotFound(key.into(), pos.clone()));
    }

    fn substitution(
        &mut self,
        name: &str,
        pos: &SourcePos,
        depth: usize,
    ) -> Result<Option<Vec<Block>>> {
        let key = key(name);
        if !self.active.substitutions.insert(key.clone()) {
            self.parser
                .context
                .report(LogMessage::CircularReference(name.into(), pos.clone()));
            return Ok(Some(vec![Block::Para(vec![Inline::Span(
                Attr::default(),
                vec![str_inline(format!("|{name}|"))],
            )])]));
        }
        let content = if let Some(mut blocks) = self.parser.substitutions.get(&key).cloned() {
            if let [Block::Para(inlines) | Block::Plain(inlines)] = blocks.as_mut_slice()
                && !matches!(inlines.as_slice(), [inline] if reference_index(inline).is_some())
            {
                // An inline replacement resolves a reference only when it is the entire replacement.
                self.charge(1)?;
                self.resolve_inlines(inlines, depth, false)?;
            } else {
                self.blocks(&mut blocks, depth)?;
            }
            Some(blocks)
        } else {
            None
        };
        self.active.substitutions.remove(&key);
        Ok(content)
    }

    fn url(&mut self, name: Option<&str>, first: Option<TargetDef>, pos: &SourcePos) -> String {
        let mut seen = HashSet::new();
        let mut next = name.map(str::to_owned);
        let mut target = first;
        for _ in 0..self.parser.context.limits.max_nesting_depth.min(128) {
            if let Some(key) = next.take() {
                if !seen.insert(key.clone()) {
                    self.parser
                        .context
                        .report(LogMessage::CircularReference(key, pos.clone()));
                    return String::new();
                }
                target = self.parser.targets.get(&key).cloned();
            }
            match target.take() {
                Some(TargetDef::Indirect(key)) => next = Some(key),
                Some(TargetDef::Url(url)) => return url,
                None => break,
            }
        }
        self.missing(name.unwrap_or("_"), pos);
        String::new()
    }
}

fn promote(blocks: &mut Vec<Block>) -> Meta {
    let mut meta = Meta::default();
    let mut counts = HashMap::new();
    for block in blocks.iter() {
        if let Block::Header(level, _, _) = block {
            *counts.entry(*level).or_insert(0) += 1;
        }
    }
    let mut promoted = 0;
    for name in ["title", "subtitle"] {
        if let Some(Block::Header(level, _, _)) = blocks.first()
            && counts.get(level) == Some(&1)
            && let Block::Header(_, _, inlines) = blocks.remove(0)
        {
            meta.0.insert(name.into(), MetaValue::MetaInlines(inlines));
            promoted += 1;
        } else {
            break;
        }
    }
    if matches!(blocks.first(), Some(Block::Div(a, _)) if a.has_class("\0rst-fields"))
        && let Block::Div(_, mut fields) = blocks.remove(0)
        && let Some(Block::DefinitionList(items)) = fields.pop()
    {
        let mut authors = Vec::new();
        for (name, mut defs) in items {
            let name = key(&stringify(&name));
            let mut body = defs.pop().unwrap_or_default();
            let value = if body.len() == 1 && matches!(body[0], Block::Plain(_) | Block::Para(_)) {
                match body.pop() {
                    Some(Block::Plain(ils) | Block::Para(ils)) => MetaValue::MetaInlines(ils),
                    _ => MetaValue::MetaInlines(Vec::new()),
                }
            } else {
                MetaValue::MetaBlocks(body)
            };
            if name == "author" {
                authors.push(value);
            } else if name == "authors" {
                if let MetaValue::MetaInlines(inlines) = value {
                    let mut current = Vec::new();
                    for inline in inlines {
                        if let Inline::Str(text) = &inline
                            && text.contains(';')
                        {
                            for (i, part) in text.split(';').enumerate() {
                                if i > 0 {
                                    authors.push(author(std::mem::take(&mut current)));
                                }
                                if !part.is_empty() {
                                    current.push(str_inline(part));
                                }
                            }
                        } else {
                            current.push(inline);
                        }
                    }
                    if !current.is_empty() {
                        authors.push(author(current));
                    }
                }
            } else {
                meta.0.insert(name, value);
            }
        }
        if !authors.is_empty() {
            meta.0.insert("author".into(), MetaValue::MetaList(authors));
        }
    }
    for block in blocks {
        if let Block::Header(level, _, _) = block {
            *level = (*level - promoted).max(1);
        }
    }
    meta
}

fn author(mut inlines: Vec<Inline>) -> MetaValue {
    if matches!(inlines.first(), Some(Inline::Space)) {
        inlines.remove(0);
    }
    if matches!(inlines.last(), Some(Inline::Space)) {
        inlines.pop();
    }
    MetaValue::MetaInlines(inlines)
}
