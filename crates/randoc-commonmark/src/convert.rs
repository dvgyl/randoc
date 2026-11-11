//! Turns the block tree into Pandoc blocks and parses the inline content.

use std::collections::HashMap;

use randoc_ast::{
    Alignment, Attr, Block, Caption, Cell, ColSpec, ColWidth, Format, Inline, ListAttributes,
    ListNumberDelim, ListNumberStyle, Row, Table, TableBody, TableFoot, TableHead,
};

use crate::Config;
use crate::attrs;
use crate::blocks::{Align, Kind, ListKind, NumStyle, TableData, Tree};
use crate::inlines::{Env, LinkDef, parse_inlines};
use randoc_core::shared::{CommonmarkIdentifierSet, commonmark_auto_identifier};

struct Converter<'a> {
    config: &'a Config,
    tree: &'a Tree,
    refs: HashMap<String, LinkDef>,
    notes: HashMap<String, Vec<Block>>,
    /// The label of the footnote body that is converted.
    current_note: Option<String>,
    heading_ids: HashMap<usize, String>,
}

/// Converts the block tree of a document.
pub(crate) fn convert(tree: &Tree, config: &Config) -> Vec<Block> {
    let mut converter = Converter {
        config,
        tree,
        refs: tree.refs.clone(),
        notes: HashMap::new(),
        current_note: None,
        heading_ids: HashMap::new(),
    };
    let root: Vec<usize> = tree
        .nodes
        .first()
        .map(|n| n.children.clone())
        .unwrap_or_default();
    if config.footnotes {
        let mut labels: Vec<(String, usize)> = Vec::new();
        for &index in &tree.notes {
            if let Some(node) = tree.nodes.get(index)
                && let Kind::FootnoteDef(label) = &node.kind
                && !converter.notes.contains_key(label)
            {
                converter.notes.insert(label.clone(), Vec::new());
                labels.push((label.clone(), index));
            }
        }
        let mut notes = HashMap::new();
        for (label, index) in &labels {
            let children = tree
                .nodes
                .get(*index)
                .map(|n| n.children.clone())
                .unwrap_or_default();
            converter.current_note = Some(label.clone());
            let depth = tree.nodes.get(*index).map_or(1, |n| n.depth);
            notes.insert(label.clone(), converter.blocks(&children, depth));
        }
        converter.notes = notes;
        converter.current_note = None;
    }
    if config.auto_identifiers || config.implicit_refs {
        converter.assign_heading_ids();
    }
    let mut blocks = converter.blocks(&root, 1);
    if config.implicit_figures {
        randoc_ast::walk::walk_blocks_mut(&mut blocks, make_figure);
    }
    blocks
}

/// Turns the paragraphs of a tight list item into plain blocks, also inside the
/// div that attributes of a block line make (`wrapper="1"`). A plain block with
/// no content makes no block, as `plain` of `pandoc-types` gives nothing for it,
/// and attributes on no block make no div.
fn make_tight(blocks: &mut Vec<Block>) {
    fn plain_blocks(blocks: &mut Vec<Block>) {
        for block in blocks.iter_mut() {
            if let Block::Para(inlines) = block {
                *block = Block::Plain(std::mem::take(inlines));
            }
        }
        blocks.retain(|block| !matches!(block, Block::Plain(inlines) if inlines.is_empty()));
    }
    blocks.retain_mut(|block| match block {
        Block::Div(attr, inner) if attr.get("wrapper") == Some("1") => {
            let had_blocks = !inner.is_empty();
            plain_blocks(inner);
            !(had_blocks && inner.is_empty())
        }
        _ => true,
    });
    plain_blocks(blocks);
}

/// Turns a paragraph that holds only an image with a description into a figure.
fn make_figure(block: &mut Block) {
    let Block::Para(inlines) = block else {
        return;
    };
    let [Inline::Image(attr, alt, target)] = inlines.as_mut_slice() else {
        return;
    };
    if alt.is_empty() {
        return;
    }
    let mut image_attr = std::mem::take(attr);
    let id = std::mem::take(&mut image_attr.id);
    let alt = std::mem::take(alt);
    let target = std::mem::take(target);
    *block = Block::Figure(
        Attr::new(id, Vec::new(), Vec::new()),
        Caption {
            short: None,
            long: vec![Block::Plain(alt.clone())],
        },
        vec![Block::Plain(vec![Inline::Image(image_attr, alt, target)])],
    );
}

/// Adds attributes to a block as `commonmark-pandoc` does. A block without
/// attributes goes into a div with the attribute `wrapper="1"`.
fn add_block_attrs(pairs: &attrs::Pairs, block: &mut Block) {
    match block {
        Block::Header(_, attr, _) | Block::CodeBlock(attr, _) | Block::Div(attr, _) => {
            *attr = attrs::merge(pairs.clone(), std::mem::take(attr));
        }
        Block::Table(table) => {
            table.attr = attrs::merge(pairs.clone(), std::mem::take(&mut table.attr));
        }
        other => {
            let mut all = vec![("wrapper".to_owned(), "1".to_owned())];
            all.extend(pairs.iter().cloned());
            let inner = std::mem::replace(other, Block::HorizontalRule);
            *other = Block::Div(attrs::merge(all, Attr::default()), vec![inner]);
        }
    }
}

/// The node indexes of the tree in document order.
fn document_order(tree: &Tree) -> Vec<usize> {
    let mut order = Vec::new();
    let mut stack = vec![0];
    while let Some(index) = stack.pop() {
        order.push(index);
        if let Some(node) = tree.nodes.get(index) {
            stack.extend(node.children.iter().rev());
        }
    }
    order
}

/// The plain text of inlines, as the `stringify` of `commonmark-pandoc` gives it.
pub(crate) fn plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    collect_text(inlines, &mut out);
    out
}

fn collect_text(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Str(s) => out.push_str(s),
            Inline::Space | Inline::SoftBreak | Inline::LineBreak => out.push(' '),
            Inline::Code(_, s) | Inline::Math(_, s) => out.push_str(s),
            Inline::RawInline(f, s) if f == "html" && s.starts_with("<br") => out.push(' '),
            Inline::Span(attr, children)
                if attr.classes == ["emoji"] && attr.attributes.len() == 1 =>
            {
                match attr.get("data-emoji") {
                    Some(alias) => {
                        out.push(':');
                        out.push_str(alias);
                        out.push(':');
                    }
                    None => collect_text(children, out),
                }
            }
            Inline::Quoted(q, children) => {
                let (open, close) = match q {
                    randoc_ast::QuoteType::SingleQuote => ('\u{2018}', '\u{2019}'),
                    randoc_ast::QuoteType::DoubleQuote => ('\u{201C}', '\u{201D}'),
                };
                out.push(open);
                collect_text(children, out);
                out.push(close);
            }
            Inline::Emph(c)
            | Inline::Underline(c)
            | Inline::Strong(c)
            | Inline::Strikeout(c)
            | Inline::Superscript(c)
            | Inline::Subscript(c)
            | Inline::SmallCaps(c)
            | Inline::Span(_, c)
            | Inline::Link(_, c, _)
            | Inline::Image(_, c, _)
            | Inline::Cite(_, c) => collect_text(c, out),
            _ => {}
        }
    }
}

impl Converter<'_> {
    /// Gives each heading without an identifier a unique identifier, and adds
    /// the implicit heading references.
    fn assign_heading_ids(&mut self) {
        let order = document_order(self.tree);
        let explicit: Vec<&str> = order
            .iter()
            .filter_map(|&index| self.tree.nodes.get(index))
            .filter(|node| matches!(node.kind, Kind::Heading { .. }) && !node.attr.id.is_empty())
            .map(|node| node.attr.id.as_str())
            .collect();
        let mut set = CommonmarkIdentifierSet::with_explicit(explicit);
        let extensions = if self.config.ascii_identifiers {
            randoc_core::Extensions::from_list(&[randoc_core::Extension::AsciiIdentifiers])
        } else {
            randoc_core::Extensions::empty()
        };
        for &index in &order {
            let Some(node) = self.tree.nodes.get(index) else {
                continue;
            };
            if !matches!(node.kind, Kind::Heading { .. }) {
                continue;
            }
            let id = if node.attr.id.is_empty() && self.config.auto_identifiers {
                let inlines = self.inlines(&node.content, node.depth);
                let base = commonmark_auto_identifier(&inlines, extensions);
                let id = set.unique(&base);
                self.heading_ids.insert(index, id.clone());
                id
            } else {
                node.attr.id.clone()
            };
            if self.config.implicit_refs {
                let label = crate::scan::normalize_label(node.content.trim());
                if !label.is_empty() {
                    self.refs.entry(label).or_insert(LinkDef {
                        url: format!("#{id}"),
                        title: String::new(),
                        attr: Attr::default(),
                    });
                }
            }
        }
    }

    /// Parses inlines of the block that starts at `line`. An inline link or image
    /// gets the folder of that block's file before a relative target.
    fn inlines_at(&self, text: &str, depth: usize, line: usize) -> Vec<Inline> {
        self.inlines_in(text, depth, self.config.source_dir(line))
    }

    fn inlines(&self, text: &str, depth: usize) -> Vec<Inline> {
        self.inlines_in(text, depth, None)
    }

    fn inlines_in(&self, text: &str, depth: usize, dir: Option<&str>) -> Vec<Inline> {
        let env = Env {
            config: self.config,
            refs: &self.refs,
            notes: &self.notes,
            current_note: self.current_note.as_deref(),
            depth_budget: self.config.max_depth.saturating_sub(depth),
            dir,
        };
        parse_inlines(text, &env)
    }

    fn blocks(&self, children: &[usize], depth: usize) -> Vec<Block> {
        let mut out = Vec::new();
        for &index in children {
            let before = out.len();
            self.block(index, depth, &mut out);
            let Some(node) = self.tree.nodes.get(index) else {
                continue;
            };
            if let Some(pairs) = &node.block_attrs {
                for block in out.iter_mut().skip(before) {
                    add_block_attrs(pairs, block);
                }
            }
        }
        out
    }

    /// The blocks of a list item or a definition. The attributes of the item go to
    /// each of its blocks, as `renderChildren` of `Commonmark/Blocks.hs` adds them.
    fn item_blocks(&self, item: usize, depth: usize) -> Vec<Block> {
        let Some(node) = self.tree.nodes.get(item) else {
            return Vec::new();
        };
        let mut blocks = self.blocks(&node.children, depth);
        if let Some(pairs) = &node.block_attrs {
            for block in &mut blocks {
                add_block_attrs(pairs, block);
            }
        }
        blocks
    }

    fn block(&self, index: usize, depth: usize, out: &mut Vec<Block>) {
        let Some(node) = self.tree.nodes.get(index) else {
            return;
        };
        match &node.kind {
            Kind::Document
            | Kind::Item(_)
            | Kind::FootnoteDef(_)
            | Kind::DefItem { .. }
            | Kind::Definition { .. } => {}
            Kind::DefList => {
                let mut items = Vec::new();
                for &item in &node.children {
                    let Some(item_node) = self.tree.nodes.get(item) else {
                        continue;
                    };
                    let tight = matches!(item_node.kind, Kind::DefItem { tight: true });
                    let term = self.inlines_at(&item_node.content, depth + 1, item_node.start_line);
                    let mut definitions = Vec::new();
                    for &definition in &item_node.children {
                        let mut blocks = self.item_blocks(definition, depth + 2);
                        if tight {
                            make_tight(&mut blocks);
                        }
                        definitions.push(blocks);
                    }
                    items.push((term, definitions));
                }
                out.push(Block::DefinitionList(items));
            }
            Kind::Paragraph => {
                let inlines = self.inlines_at(&node.content, depth, node.start_line);
                out.push(Block::Para(inlines));
            }
            Kind::Heading { level, .. } => {
                let inlines = self.inlines_at(&node.content, depth, node.start_line);
                let mut attr = node.attr.clone();
                if let Some(id) = self.heading_ids.get(&index) {
                    attr.id = id.clone();
                }
                out.push(Block::Header(*level, attr, inlines));
            }
            Kind::ThematicBreak => out.push(Block::HorizontalRule),
            Kind::CodeBlock { fenced, info, .. } => {
                let text = node.content.strip_suffix('\n').unwrap_or(&node.content);
                if *fenced {
                    out.push(self.fenced_code(info, text));
                } else {
                    out.push(Block::CodeBlock(Attr::default(), text.to_owned()));
                }
            }
            Kind::HtmlBlock(_) => {
                out.push(Block::RawBlock(
                    Format::new("html"),
                    self.config.raw_text("html", node.content.clone()),
                ));
            }
            Kind::BlockQuote => {
                if self.config.alerts
                    && let Some(block) = self.alert(&node.content, &node.children, depth)
                {
                    out.push(block);
                    return;
                }
                out.push(Block::BlockQuote(self.blocks(&node.children, depth + 1)));
            }
            Kind::Div { .. } => {
                out.push(Block::Div(
                    node.attr.clone(),
                    self.blocks(&node.children, depth + 1),
                ));
            }
            Kind::List(data) => {
                let mut items = Vec::new();
                for &item in &node.children {
                    let Some(item_node) = self.tree.nodes.get(item) else {
                        continue;
                    };
                    let mut blocks = self.item_blocks(item, depth + 1);
                    if data.tight {
                        make_tight(&mut blocks);
                    }
                    if let Kind::Item(item_data) = &item_node.kind
                        && let Some(checked) = item_data.task
                    {
                        let mark =
                            Inline::Str(if checked { "\u{2612}" } else { "\u{2610}" }.into());
                        match blocks.first_mut() {
                            Some(Block::Plain(inlines) | Block::Para(inlines)) => {
                                inlines.insert(0, Inline::Space);
                                inlines.insert(0, mark);
                            }
                            _ => blocks.insert(0, Block::Plain(vec![mark])),
                        }
                    }
                    items.push(blocks);
                }
                match data.kind {
                    ListKind::Bullet(_) => out.push(Block::BulletList(items)),
                    ListKind::Ordered(style, delim) => out.push(Block::OrderedList(
                        ListAttributes {
                            start: data.start,
                            style: match style {
                                NumStyle::Decimal => ListNumberStyle::Decimal,
                                NumStyle::LowerAlpha => ListNumberStyle::LowerAlpha,
                                NumStyle::UpperAlpha => ListNumberStyle::UpperAlpha,
                                NumStyle::LowerRoman => ListNumberStyle::LowerRoman,
                                NumStyle::UpperRoman => ListNumberStyle::UpperRoman,
                            },
                            delim: match delim {
                                b')' => ListNumberDelim::OneParen,
                                b'(' => ListNumberDelim::TwoParens,
                                _ => ListNumberDelim::Period,
                            },
                        },
                        items,
                    )),
                }
            }
            Kind::Table(table) => {
                let Some(TableData {
                    aligns,
                    header,
                    rows,
                    ..
                }) = self.tree.tables.get(*table)
                else {
                    return;
                };
                let cell = |text: &String| {
                    let inlines = self.inlines_at(text, depth + 1, node.start_line);
                    Cell {
                        content: if inlines.is_empty() {
                            Vec::new()
                        } else {
                            vec![Block::Plain(inlines)]
                        },
                        ..Cell::default()
                    }
                };
                let row = |cells: &Vec<String>| Row {
                    attr: Attr::default(),
                    cells: cells.iter().map(cell).collect(),
                };
                let col_specs = aligns
                    .iter()
                    .map(|a| ColSpec {
                        align: match a {
                            Align::Left => Alignment::AlignLeft,
                            Align::Right => Alignment::AlignRight,
                            Align::Center => Alignment::AlignCenter,
                            Align::Default => Alignment::AlignDefault,
                        },
                        width: ColWidth::ColWidthDefault,
                    })
                    .collect();
                out.push(Block::Table(Box::new(Table {
                    attr: Attr::default(),
                    caption: Caption::default(),
                    col_specs,
                    head: TableHead {
                        attr: Attr::default(),
                        rows: vec![row(header)],
                    },
                    bodies: vec![TableBody {
                        attr: Attr::default(),
                        row_head_columns: 0,
                        head: Vec::new(),
                        body: rows.iter().map(row).collect(),
                    }],
                    foot: TableFoot::default(),
                })));
            }
        }
    }

    fn fenced_code(&self, info: &str, text: &str) -> Block {
        if self.config.raw_attribute
            && let Some(format) = raw_format(info)
        {
            return Block::RawBlock(
                Format::new(format),
                self.config.raw_text(format, format_raw_text(text)),
            );
        }
        if self.config.attributes {
            let lang_end = info.find(['{']).unwrap_or(info.len());
            if let Some(open) = info.find('{')
                && let Some((end, pairs)) = attrs::parse_attributes(info, open)
                && info.get(end..).is_some_and(|r| r.trim().is_empty())
            {
                let lang =
                    crate::scan::unescape(info.get(..lang_end).unwrap_or("").trim().as_bytes());
                let mut attr = Attr::default();
                if !lang.is_empty() {
                    attr.classes.push(lang);
                }
                return Block::CodeBlock(attrs::merge(pairs, attr), text.to_owned());
            }
        }
        let info = crate::scan::unescape(info.as_bytes());
        let lang: String = info.chars().take_while(|c| !c.is_whitespace()).collect();
        let classes = if lang.is_empty() {
            Vec::new()
        } else {
            vec![lang]
        };
        if self.config.math_gfm && classes.first().is_some_and(|c| c == "math") {
            return Block::Para(vec![Inline::Math(
                randoc_ast::MathType::DisplayMath,
                text.to_owned(),
            )]);
        }
        Block::CodeBlock(Attr::new("", classes, Vec::new()), text.to_owned())
    }

    fn alert(&self, marker: &str, children: &[usize], depth: usize) -> Option<Block> {
        let title = crate::blocks::alert_title(marker)?;
        let mut blocks = vec![Block::Div(
            Attr::new("", vec!["title".into()], Vec::new()),
            vec![Block::Para(vec![Inline::Str(title.into())])],
        )];
        blocks.extend(self.blocks(children, depth + 1));
        Some(Block::Div(
            Attr::new("", vec![title.to_ascii_lowercase()], Vec::new()),
            blocks,
        ))
    }
}

fn raw_format(info: &str) -> Option<&str> {
    let inner = info.trim().strip_prefix('{')?.strip_suffix('}')?.trim();
    let format = inner.strip_prefix('=')?;
    (!format.is_empty() && format.chars().all(crate::inlines::is_word_char)).then_some(format)
}

fn format_raw_text(text: &str) -> String {
    let mut out = text.to_owned();
    if !out.is_empty() {
        out.push('\n');
    }
    out
}
