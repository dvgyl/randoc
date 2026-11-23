use std::rc::Rc;

use randoc_core::Term;

use super::eval::{Element, Evaluator, Scope, Value};
use super::*;

#[derive(Clone, Default)]
pub(super) struct Flow {
    items: Vec<Item>,
    shape: Shape,
}

#[derive(Clone)]
pub(super) enum Item {
    Inline(Inline),
    Block(Block),
    Paragraph,
}

impl Flow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn depth(&self) -> usize {
        self.shape.depth
    }

    pub fn weight(&self) -> usize {
        self.shape.weight
    }

    pub fn as_slice(&self) -> &[Item] {
        &self.items
    }

    pub fn push(&mut self, item: Item) {
        let shape = match &item {
            Item::Inline(inline) => inline_shape(inline),
            Item::Block(block) => block_shape(block),
            Item::Paragraph => Shape::default(),
        };
        self.shape.add(shape.enclose(0, 32));
        self.items.push(item);
    }

    pub fn append(&mut self, mut flow: Self) {
        self.shape.add(flow.shape);
        self.items.append(&mut flow.items);
    }

    pub fn last(&self) -> Option<&Item> {
        self.items.last()
    }

    pub fn pop(&mut self) -> Option<Item> {
        // The recorded weight and depth remain upper bounds.
        self.items.pop()
    }

    pub fn attach_label(&mut self, id: &str) -> bool {
        let Some(Item::Block(
            Block::Header(_, attr, _) | Block::Figure(attr, ..) | Block::Div(attr, _),
        )) = self.items.last_mut()
        else {
            return false;
        };
        attr.id = id.into();
        self.shape.weight = self.shape.weight.saturating_add(id.len());
        true
    }

    pub fn definition(&mut self, term: Vec<Inline>, definition: Vec<Block>) {
        if let Some(Item::Block(Block::DefinitionList(items))) = self.items.last_mut() {
            let mut shape = shapes(term.iter().map(inline_shape));
            shape.add(shapes(definition.iter().map(block_shape)));
            self.shape.add(shape.enclose(1, 32));
            items.push((term, vec![definition]));
        } else {
            self.push(Item::Block(Block::DefinitionList(vec![(
                term,
                vec![definition],
            )])));
        }
    }
}

impl<const N: usize> From<[Item; N]> for Flow {
    fn from(items: [Item; N]) -> Self {
        items.into_iter().collect()
    }
}

impl FromIterator<Item> for Flow {
    fn from_iter<T: IntoIterator<Item = Item>>(items: T) -> Self {
        let mut flow = Self::new();
        for item in items {
            flow.push(item);
        }
        flow
    }
}

#[derive(Clone, Copy, Default)]
struct Shape {
    depth: usize,
    weight: usize,
}

impl Shape {
    fn add(&mut self, other: Self) {
        self.depth = self.depth.max(other.depth);
        self.weight = self.weight.saturating_add(other.weight);
    }

    fn enclose(self, depth: usize, weight: usize) -> Self {
        Self {
            depth: self.depth.saturating_add(depth),
            weight: self.weight.saturating_add(weight),
        }
    }
}

fn shapes(items: impl Iterator<Item = Shape>) -> Shape {
    let mut shape = Shape::default();
    for item in items {
        shape.add(item);
    }
    shape
}

fn attr_weight(attr: &Attr) -> usize {
    attr.classes
        .iter()
        .map(String::len)
        .chain(
            attr.attributes
                .iter()
                .flat_map(|(key, value)| [key.len(), value.len()]),
        )
        .fold(attr.id.len(), usize::saturating_add)
}

fn inline_weight(inline: &Inline) -> usize {
    let payload = match inline {
        Inline::Str(text) | Inline::Math(_, text) => text.len(),
        Inline::Code(attr, text) => attr_weight(attr).saturating_add(text.len()),
        Inline::RawInline(format, text) => format.0.len().saturating_add(text.len()),
        Inline::Link(attr, _, target) | Inline::Image(attr, _, target) => attr_weight(attr)
            .saturating_add(target.url.len())
            .saturating_add(target.title.len()),
        Inline::Span(attr, _) => attr_weight(attr),
        Inline::Cite(citations, _) => citations
            .iter()
            .map(|c| c.id.len().saturating_add(32))
            .fold(0usize, usize::saturating_add),
        _ => 0,
    };
    payload.saturating_add(32)
}

fn inline_shape(inline: &Inline) -> Shape {
    let children = match inline {
        Inline::Emph(xs)
        | Inline::Strong(xs)
        | Inline::Underline(xs)
        | Inline::Strikeout(xs)
        | Inline::Superscript(xs)
        | Inline::Subscript(xs)
        | Inline::SmallCaps(xs)
        | Inline::Quoted(_, xs)
        | Inline::Cite(_, xs)
        | Inline::Link(_, xs, _)
        | Inline::Image(_, xs, _)
        | Inline::Span(_, xs) => shapes(xs.iter().map(inline_shape)),
        Inline::Note(blocks) => shapes(blocks.iter().map(block_shape)),
        _ => Shape::default(),
    };
    children.enclose(1, inline_weight(inline))
}

fn block_shape(block: &Block) -> Shape {
    let blocks = |blocks: &[Block]| shapes(blocks.iter().map(block_shape));
    let inlines = |xs: &[Inline]| shapes(xs.iter().map(inline_shape));
    let (children, payload) = match block {
        Block::Plain(xs) | Block::Para(xs) => (inlines(xs), 0),
        Block::Header(_, attr, xs) => (inlines(xs), attr_weight(attr)),
        Block::LineBlock(lines) => (shapes(lines.iter().map(|line| inlines(line))), 0),
        Block::Div(attr, xs) => (blocks(xs), attr_weight(attr)),
        Block::BlockQuote(xs) => (blocks(xs), 0),
        Block::Figure(attr, caption, xs) => {
            let mut shape = blocks(&caption.long);
            shape.add(blocks(xs));
            (shape, attr_weight(attr))
        }
        Block::BulletList(items) | Block::OrderedList(_, items) => (
            shapes(items.iter().map(|item| blocks(item).enclose(0, 32))),
            0,
        ),
        Block::DefinitionList(items) => (
            shapes(items.iter().map(|(term, definitions)| {
                let mut shape = inlines(term);
                shape.add(shapes(definitions.iter().map(|item| blocks(item))));
                shape.enclose(0, 32)
            })),
            0,
        ),
        Block::Table(table) => {
            let mut shape = blocks(&table.caption.long);
            for row in table
                .head
                .rows
                .iter()
                .chain(
                    table
                        .bodies
                        .iter()
                        .flat_map(|body| body.head.iter().chain(&body.body)),
                )
                .chain(&table.foot.rows)
            {
                shape.add(Shape::default().enclose(0, attr_weight(&row.attr).saturating_add(32)));
                for cell in &row.cells {
                    shape.add(
                        blocks(&cell.content)
                            .enclose(0, attr_weight(&cell.attr).saturating_add(32)),
                    );
                }
            }
            (
                shape,
                attr_weight(&table.attr).saturating_add(table.col_specs.len().saturating_mul(32)),
            )
        }
        Block::CodeBlock(attr, text) => (
            Shape::default(),
            attr_weight(attr).saturating_add(text.len()),
        ),
        Block::RawBlock(format, text) => {
            (Shape::default(), format.0.len().saturating_add(text.len()))
        }
        _ => (Shape::default(), 0),
    };
    children.enclose(1, payload.saturating_add(32))
}

pub(super) fn inline_text(text: &str) -> Flow {
    let mut out = Flow::new();
    let mut start = 0;
    let mut space = false;
    for (end, c) in text.char_indices() {
        if c.is_whitespace()
            && c != '\u{a0}'
            && c != '\u{200a}'
            && c != '\u{2000}'
            && c != '\u{2001}'
        {
            if start < end {
                out.push(Item::Inline(Inline::Str(text[start..end].into())));
            }
            if !space {
                out.push(Item::Inline(Inline::Space));
            }
            space = true;
            start = end + c.len_utf8();
        } else {
            space = false;
        }
    }
    if start < text.len() {
        out.push(Item::Inline(Inline::Str(text[start..].into())));
    }
    out
}

pub(super) fn finish(flow: Flow) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut content = Vec::new();
    for item in flow.items {
        match item {
            Item::Inline(inline) => content.push(inline),
            Item::Paragraph => flush(&mut blocks, &mut content),
            Item::Block(block) => {
                flush(&mut blocks, &mut content);
                blocks.push(block);
            }
        }
    }
    flush(&mut blocks, &mut content);
    blocks
}

fn flush(blocks: &mut Vec<Block>, inlines: &mut Vec<Inline>) {
    let text = normalize(std::mem::take(inlines), true);
    if !text.is_empty() {
        blocks.push(Block::Para(text));
    }
}

pub(super) fn inlines(flow: Flow, trim: bool) -> Vec<Inline> {
    normalize(
        flow.items
            .into_iter()
            .flat_map(|item| match item {
                Item::Inline(inline) => vec![inline],
                Item::Paragraph => vec![Inline::Space],
                Item::Block(
                    Block::Plain(inlines) | Block::Para(inlines) | Block::Header(_, _, inlines),
                ) => inlines,
                Item::Block(_) => Vec::new(),
            })
            .collect(),
        trim,
    )
}

fn normalize(inlines: Vec<Inline>, trim: bool) -> Vec<Inline> {
    let mut out = Vec::new();
    for i in inlines {
        match (out.last_mut(), i) {
            (Some(Inline::Str(last)), Inline::Str(text)) => last.push_str(&text),
            (Some(Inline::Space), Inline::Space) => {}
            (None, Inline::Space | Inline::SoftBreak) if trim => {}
            (_, i) => out.push(i),
        }
    }
    if trim {
        while matches!(out.last(), Some(Inline::Space | Inline::SoftBreak)) {
            out.pop();
        }
    }
    out
}

pub(super) fn wrap(flow: Flow, mut wrapper: impl FnMut(Vec<Inline>) -> Inline) -> Flow {
    let mut out = Flow {
        items: Vec::new(),
        shape: flow.shape,
    };
    let mut wrapped = false;
    let mut wrap_run = |run: Vec<Inline>, shape: &mut Shape| {
        wrapped = true;
        let inline = wrapper(run);
        shape.weight = shape
            .weight
            .saturating_add(inline_weight(&inline))
            .saturating_add(32);
        inline
    };
    let mut items = flow.items;
    if let [Item::Block(Block::Header(..))] = items.as_slice()
        && let Some(Item::Block(Block::Header(level, attr, text))) = items.pop()
    {
        let inline = wrap_run(text, &mut out.shape);
        out.items
            .push(Item::Block(Block::Header(level, attr, vec![inline])));
    } else {
        let mut run = Vec::new();
        let mut flush = |out: &mut Flow, run: &mut Vec<Inline>| {
            if run
                .iter()
                .any(|inline| !matches!(inline, Inline::SoftBreak | Inline::Space))
            {
                let inline = wrap_run(normalize(std::mem::take(run), false), &mut out.shape);
                out.items.push(Item::Inline(inline));
            } else {
                run.clear();
            }
        };
        for item in items {
            match item {
                Item::Inline(inline) => run.push(inline),
                boundary => {
                    flush(&mut out, &mut run);
                    out.items.push(boundary);
                }
            }
        }
        flush(&mut out, &mut run);
    }
    if wrapped {
        out.shape.depth = out.shape.depth.saturating_add(1);
    }
    out
}

pub(super) fn style(name: &str, xs: Vec<Inline>) -> Inline {
    match name {
        "emph" => Inline::Emph(xs),
        "strong" => Inline::Strong(xs),
        "underline" => Inline::Underline(xs),
        "strike" => Inline::Strikeout(xs),
        "sub" => Inline::Subscript(xs),
        "super" => Inline::Superscript(xs),
        "smallcaps" => Inline::SmallCaps(xs),
        "highlight" => Inline::Span(Attr::new("", vec!["mark".into()], vec![]), xs),
        "linebreak" => Inline::LineBreak,
        _ => Inline::Span(Attr::default(), xs),
    }
}

pub(super) fn raw(lang: &str, text: &str, block: bool) -> Item {
    let classes = if lang.is_empty() {
        vec![]
    } else {
        vec![lang.into()]
    };
    let attr = Attr::new("", classes, vec![]);
    if block {
        let margin = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                line.bytes()
                    .take_while(|b| matches!(*b, b' ' | b'\t'))
                    .count()
            })
            .min()
            .unwrap_or(0);
        let text = text
            .split('\n')
            .map(|line| line.get(margin..).unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        Item::Block(Block::CodeBlock(attr, text))
    } else {
        Item::Inline(Inline::Code(attr, text.into()))
    }
}

pub(super) fn citation(id: &str, mode: CitationMode, _supplement: Vec<Inline>) -> Inline {
    Inline::Cite(
        vec![Citation {
            id: id.into(),
            prefix: vec![],
            suffix: vec![],
            mode,
            note_num: 0,
            hash: 0,
        }],
        vec![Inline::Str(format!("[{id}]"))],
    )
}

pub(super) fn reference(id: &str, supplement: Vec<Inline>) -> Inline {
    Inline::Link(
        Attr::new("", vec!["typst-ref".into()], vec![]),
        supplement,
        Target::new(format!("#{id}"), ""),
    )
}

pub(super) fn remove_east_asian_breaks(flow: &mut Flow) {
    let mut omit = vec![false; flow.items.len()];
    for (index, part) in flow.items.windows(3).enumerate() {
        if let [
            Item::Inline(Inline::Str(left)),
            Item::Inline(Inline::SoftBreak),
            Item::Inline(Inline::Str(right)),
        ] = part
        {
            omit[index + 1] = left
                .chars()
                .next_back()
                .is_some_and(crate::writer::east_asian)
                && right.chars().next().is_some_and(crate::writer::east_asian);
        }
    }
    let mut index = 0;
    flow.items.retain(|_| {
        let keep = !omit[index];
        index += 1;
        keep
    });
}

impl<'a> Evaluator<'a, '_> {
    pub(super) fn content(&mut self, value: Value<'a>, depth: usize, pos: usize) -> Result<Flow> {
        if depth == 0 {
            return Err(self.fail(pos, "The Typst content is nested too deeply."));
        }
        Ok(match value {
            Value::None | Value::Bool(_) | Value::Function(_) => Flow::new(),
            Value::Content(content) => match Rc::try_unwrap(content) {
                Ok(content) => content,
                Err(content) => {
                    self.spend(content.weight(), pos)?;
                    content.as_ref().clone()
                }
            },
            Value::Element(element) => self.element(&element, depth, pos)?,
            Value::Text(text) => {
                self.spend(text.len(), pos)?;
                inline_text(&text)
            }
            Value::Number(_, _) | Value::Float(_) => inline_text(&self.text(&value, pos)?),
            Value::Label(id) => Flow::from([Item::Inline(Inline::Span(
                Attr::new(id, vec![], vec![]),
                vec![],
            ))]),
            Value::Builtin(name) if name.starts_with("sym.") || name.starts_with("emoji.") => {
                let symbol = symbols::lookup(&name)
                    .ok_or_else(|| self.fail(pos, &format!("Symbol {name:?} not found")))?;
                Flow::from([Item::Inline(Inline::Str(symbol.into()))])
            }
            _ => Flow::new(),
        })
    }

    pub(super) fn document(&mut self, named: &Scope<'a>, depth: usize, pos: usize) -> Result<()> {
        for (key, val) in named {
            let value = match key.as_ref() {
                "title" | "date" => {
                    MetaValue::MetaInlines(inlines(self.content(val.clone(), depth, pos)?, true))
                }
                "author" | "keywords" => {
                    let values = match val {
                        Value::Array(values) => values.as_slice(),
                        _ => std::slice::from_ref(val),
                    };
                    let mut list = Vec::new();
                    for val in values {
                        list.push(MetaValue::MetaInlines(inlines(
                            self.content(val.clone(), depth, pos)?,
                            true,
                        )));
                    }
                    MetaValue::MetaList(list)
                }
                _ => continue,
            };
            self.meta.0.insert(key.to_string(), value);
        }
        Ok(())
    }

    fn element(&mut self, element: &Element<'a>, depth: usize, pos: usize) -> Result<Flow> {
        if depth == 0 {
            return Err(self.fail(pos, "The Typst content is nested too deeply."));
        }
        let depth = depth - 1;
        let name = element.name.as_str();
        if matches!(
            name,
            "emph"
                | "strong"
                | "underline"
                | "strike"
                | "sub"
                | "super"
                | "smallcaps"
                | "highlight"
        ) {
            let body = element.pos.last().cloned().unwrap_or(Value::None);
            let mut flow = self.content(body, depth, pos)?;
            if matches!(flow.last(), Some(Item::Inline(Inline::SoftBreak))) {
                flow.pop();
            }
            return Ok(wrap(flow, |xs| style(name, xs)));
        }
        self.element_other(element, depth, pos)
    }

    fn element_other(&mut self, element: &Element<'a>, depth: usize, pos: usize) -> Result<Flow> {
        let name = element.name.as_str();
        let first = || element.pos.first().cloned().unwrap_or(Value::None);
        let body = || element.pos.last().cloned().unwrap_or(Value::None);
        let defaults = self.settings.get(name).cloned();
        let setting = |key: &str| {
            element
                .named
                .get(key)
                .or_else(|| defaults.as_ref()?.get(key))
                .cloned()
        };
        let inline = |x| Flow::from([Item::Inline(x)]);
        let block = |x| Flow::from([Item::Block(x)]);
        Ok(match name {
            "parbreak" => Flow::from([Item::Paragraph]),
            "linebreak" => inline(Inline::LineBreak),
            "divider" | "line" => block(Block::HorizontalRule),
            "pagebreak" => block(Block::Div(
                Attr::new(
                    "",
                    vec!["page-break".into()],
                    vec![("wrapper".into(), "1".into())],
                ),
                vec![Block::HorizontalRule],
            )),
            "heading" => {
                let level = setting("level").and_then(|v| v.number()).unwrap_or(1.);
                let offset = setting("offset").and_then(|v| v.number()).unwrap_or(0.);
                block(Block::Header(
                    (level + offset) as i64,
                    Attr::default(),
                    inlines(self.content(body(), depth, pos)?, true),
                ))
            }
            "title" => {
                if !element.pos.is_empty() {
                    let title = inlines(self.content(body(), depth, pos)?, true);
                    self.meta
                        .0
                        .insert("title".into(), MetaValue::MetaInlines(title));
                }
                Flow::new()
            }
            "text" => {
                let weight = setting("weight").map(|v| self.text(&v, pos)).transpose()?;
                let style_value = setting("style").map(|v| self.text(&v, pos)).transpose()?;
                let mut body = self.content(body(), depth, pos)?;
                if weight.as_deref() == Some("bold") {
                    body = wrap(body, Inline::Strong);
                }
                if style_value.as_deref() == Some("italic") {
                    body = wrap(body, Inline::Emph);
                }
                body
            }
            "link" => {
                let dest = first();
                let mut url = self.text(&dest, pos)?;
                if matches!(dest, Value::Label(_)) {
                    url.insert(0, '#');
                }
                let target = Target::new(url.clone(), "");
                if element.pos.len() <= 1 {
                    inline(Inline::Link(
                        Attr::default(),
                        vec![Inline::Str(url)],
                        target,
                    ))
                } else {
                    let mut body = self.content(body(), depth, pos)?;
                    let trailing = if matches!(body.last(), Some(Item::Inline(Inline::SoftBreak))) {
                        body.pop()
                    } else {
                        None
                    };
                    let mut result =
                        wrap(body, |xs| Inline::Link(Attr::default(), xs, target.clone()));
                    if let Some(trailing) = trailing {
                        result.push(trailing);
                    }
                    result
                }
            }
            "ref" => {
                let supp = setting("supplement").unwrap_or(Value::None);
                inline(reference(
                    &self.text(&first(), pos)?,
                    inlines(self.content(supp, depth, pos)?, true),
                ))
            }
            "cite" => {
                let mode = match setting("form")
                    .map(|v| self.text(&v, pos))
                    .transpose()?
                    .as_deref()
                {
                    Some("prose") => CitationMode::AuthorInText,
                    Some("year") => CitationMode::SuppressAuthor,
                    _ => CitationMode::NormalCitation,
                };
                inline(citation(&self.text(&first(), pos)?, mode, vec![]))
            }
            "bibliography" => {
                let path = first();
                let value = match path {
                    Value::Array(paths) => MetaValue::MetaList(
                        paths
                            .iter()
                            .map(|path| self.text(path, pos).map(MetaValue::MetaString))
                            .collect::<Result<Vec<_>>>()?,
                    ),
                    path => MetaValue::MetaString(self.text(&path, pos)?),
                };
                self.meta.0.insert("bibliography".into(), value);
                let title = setting("title");
                let title = title
                    .unwrap_or_else(|| Value::Text(self.context.term(Term::References).into()));
                Flow::from([
                    Item::Block(Block::Header(
                        1,
                        Attr::default(),
                        inlines(self.content(title, depth, pos)?, true),
                    )),
                    Item::Block(Block::Div(Attr::new("refs", vec![], vec![]), vec![])),
                ])
            }
            "raw" => {
                let lang = setting("lang")
                    .map(|v| self.text(&v, pos))
                    .transpose()?
                    .unwrap_or_default();
                let block = matches!(setting("block"), Some(Value::Bool(true)));
                Flow::from([raw(&lang, &self.text(&first(), pos)?, block)])
            }
            "quote" => {
                let block_quote = matches!(setting("block"), Some(Value::Bool(true)));
                let author = setting("attribution");
                let text = self.content(body(), depth, pos)?;
                if block_quote {
                    let mut blocks = finish(text);
                    if let Some(author) = author {
                        let mut text = vec![Inline::Str("—\u{a0}".into())];
                        text.extend(inlines(self.content(author, depth, pos)?, true));
                        blocks.push(Block::Para(normalize(text, true)));
                    }
                    block(Block::BlockQuote(blocks))
                } else {
                    wrap(text, |xs| Inline::Quoted(QuoteType::DoubleQuote, xs))
                }
            }
            "footnote" => inline(Inline::Note(finish(self.content(body(), depth, pos)?))),
            "image" => {
                let mut attrs = Vec::new();
                let alt = setting("alt").unwrap_or(Value::None);
                for key in ["width", "height"] {
                    if let Some(Value::Number(size, unit)) = setting(key) {
                        let value = if unit == "%" {
                            format!("{size}%")
                        } else {
                            format!("{size:?}{unit}")
                        };
                        attrs.push((key.into(), value));
                    }
                }
                inline(Inline::Image(
                    Attr::new("", vec![], attrs),
                    inlines(self.content(alt, depth, pos)?, true),
                    Target::new(self.text(&first(), pos)?, ""),
                ))
            }
            "figure" => {
                let caption = setting("caption").unwrap_or(Value::None);
                let content = finish(self.content(first(), depth, pos)?);
                let caption = Caption {
                    short: None,
                    long: finish(self.content(caption, depth, pos)?),
                };
                if let [Block::Table(_)] = content.as_slice() {
                    let mut content = content;
                    if let Some(Block::Table(mut table)) = content.pop() {
                        table.caption = caption;
                        block(Block::Table(table))
                    } else {
                        Flow::new()
                    }
                } else {
                    block(Block::Figure(Attr::default(), caption, content))
                }
            }
            "box" | "align" => {
                let attr = if name == "box" {
                    Attr::new("", vec!["box".into()], vec![])
                } else {
                    Attr::new(
                        "",
                        vec![],
                        vec![("align".into(), self.text(&first(), pos)?)],
                    )
                };
                let body = self.content(body(), depth, pos)?;
                if body.as_slice().iter().any(|i| matches!(i, Item::Block(_))) {
                    block(Block::Div(attr, finish(body)))
                } else {
                    wrap(body, |xs| Inline::Span(attr.clone(), xs))
                }
            }
            "block" => {
                let mut result = Flow::from([Item::Paragraph]);
                result.append(self.content(body(), depth, pos)?);
                result.push(Item::Paragraph);
                result
            }
            "columns" | "rect" | "circle" | "square" => {
                let attr = if name == "columns" {
                    Attr::new(
                        "",
                        vec!["columns-flow".into()],
                        vec![("count".into(), self.text(&first(), pos)?)],
                    )
                } else {
                    Attr::new("", vec![name.into()], vec![])
                };
                block(Block::Div(attr, finish(self.content(body(), depth, pos)?)))
            }
            "lower" | "upper" => {
                let mut text = self.content(body(), depth, pos)?;
                for item in &mut text.items {
                    if let Item::Inline(Inline::Str(s)) = item {
                        let previous = s.len();
                        *s = if name == "lower" {
                            s.to_lowercase()
                        } else {
                            s.to_uppercase()
                        };
                        text.shape.weight = text
                            .shape
                            .weight
                            .saturating_add(s.len().saturating_sub(previous));
                    }
                }
                text
            }
            "label" => inline(Inline::Span(
                Attr::new(self.text(&first(), pos)?, vec![], vec![]),
                vec![],
            )),
            "table" | "grid" => {
                let cols = setting("columns");
                let align = setting("align");
                block(Block::Table(Box::new(
                    self.table(element, cols, align, depth, pos)?,
                )))
            }
            "list" | "enum" => {
                let mut items = Vec::new();
                for value in &element.pos {
                    items.push(finish(self.content(value.clone(), depth, pos)?));
                }
                if name == "list" {
                    block(Block::BulletList(items))
                } else {
                    let attr = self.list_attr(Some(&element.named), defaults.as_deref(), pos)?;
                    block(Block::OrderedList(attr, items))
                }
            }
            "terms" => {
                let mut entries = Vec::new();
                for value in &element.pos {
                    if let Value::Array(pair) = value
                        && let [term, body] = pair.as_slice()
                    {
                        let term = inlines(self.content(term.clone(), depth, pos)?, true);
                        let body = finish(self.content(body.clone(), depth, pos)?);
                        entries.push((term, vec![body]));
                    }
                }
                block(Block::DefinitionList(entries))
            }
            "pad" | "place" | "par" => self.content(body(), depth, pos)?,
            _ => Flow::new(),
        })
    }

    pub(super) fn list_attr(
        &mut self,
        args: Option<&Scope<'a>>,
        defaults: Option<&Scope<'a>>,
        pos: usize,
    ) -> Result<ListAttributes> {
        let mut attr = ListAttributes::default();
        {
            let setting = |key| {
                args.and_then(|args| args.get(key))
                    .or_else(|| defaults.and_then(|args| args.get(key)))
            };
            if let Some(start) = setting("start").and_then(Value::number) {
                attr.start = start as i64;
            }
            if let Some(value) = setting("numbering") {
                let pattern = self.text(value, pos)?;
                attr.style = match pattern
                    .find(['a', 'A', 'i', 'I'])
                    .and_then(|n| pattern.as_bytes().get(n))
                {
                    Some(b'a') => ListNumberStyle::LowerAlpha,
                    Some(b'A') => ListNumberStyle::UpperAlpha,
                    Some(b'i') => ListNumberStyle::LowerRoman,
                    Some(b'I') => ListNumberStyle::UpperRoman,
                    _ => ListNumberStyle::Decimal,
                };
                attr.delim = if pattern.starts_with('(') {
                    ListNumberDelim::TwoParens
                } else if pattern.ends_with(')') {
                    ListNumberDelim::OneParen
                } else {
                    ListNumberDelim::Period
                };
            }
        }
        Ok(attr)
    }

    fn table(
        &mut self,
        element: &Element<'a>,
        cols: Option<Value<'a>>,
        align: Option<Value<'a>>,
        depth: usize,
        pos: usize,
    ) -> Result<Table> {
        let count = match &cols {
            Some(Value::Array(widths)) => widths.len(),
            Some(value) => value.number().unwrap_or(1.) as usize,
            None => 1,
        }
        .max(1);
        let mut body = Vec::new();
        let mut head = Vec::new();
        let mut foot = Vec::new();
        for value in &element.pos {
            if let Value::Element(item) = value
                && let Some((_, name)) = item.name.split_once('.')
            {
                match name {
                    "header" => {
                        head.extend(item.pos.iter().cloned());
                        continue;
                    }
                    "footer" => {
                        foot.extend(item.pos.iter().cloned());
                        continue;
                    }
                    "hline" | "vline" => continue,
                    _ => {}
                }
            }
            body.push(value.clone());
        }
        let limit = self.context.limits.max_table_cells;
        let real_cells = head
            .len()
            .saturating_add(body.len())
            .saturating_add(foot.len());
        let extra_columns = count.saturating_sub(real_cells);
        if real_cells > 0 && extra_columns > limit {
            return Err(self.fail(pos, "The Typst table exceeds the cell limit."));
        }
        self.spend(
            extra_columns.saturating_mul(std::mem::size_of::<ColSpec>()),
            pos,
        )?;
        let mut specs = vec![ColSpec::default(); count];
        if let Some(Value::Array(widths)) = cols {
            let total: f64 = widths
                .iter()
                .filter_map(|w| match w {
                    Value::Number(n, "fr") => Some(n),
                    _ => None,
                })
                .sum();
            if total > 0. && widths.iter().all(|w| matches!(w, Value::Number(_, "fr"))) {
                for (col, w) in specs.iter_mut().zip(widths.iter()) {
                    col.width = ColWidth::ColWidth(w.number().unwrap_or(0.) / total);
                }
            }
        }
        if let Some(align) = align {
            if let Value::Array(aligns) = align {
                for (col, value) in specs.iter_mut().zip(aligns.iter()) {
                    col.align = alignment(&self.text(value, pos)?);
                }
            } else {
                let align = alignment(&self.text(&align, pos)?);
                for col in &mut specs {
                    col.align = align;
                }
            }
        }
        let mut remaining = limit;
        let mut head = self.table_rows(head, count, &mut remaining, depth, pos)?;
        let mut body = self.table_rows(body, count, &mut remaining, depth, pos)?;
        let mut foot = self.table_rows(foot, count, &mut remaining, depth, pos)?;
        if head
            .iter()
            .chain(&body)
            .chain(&foot)
            .flat_map(|row| &row.cells)
            .all(|cell| matches!(cell.content.as_slice(), [] | [Block::Para(_)]))
        {
            for cell in head
                .iter_mut()
                .chain(&mut body)
                .chain(&mut foot)
                .flat_map(|row| &mut row.cells)
            {
                if let [Block::Para(xs)] = cell.content.as_mut_slice() {
                    cell.content = vec![Block::Plain(std::mem::take(xs))];
                }
            }
        }
        Ok(Table {
            col_specs: specs,
            head: TableHead {
                rows: head,
                ..TableHead::default()
            },
            bodies: vec![TableBody {
                body,
                ..TableBody::default()
            }],
            foot: TableFoot {
                rows: foot,
                ..TableFoot::default()
            },
            ..Table::default()
        })
    }

    fn table_rows(
        &mut self,
        items: Vec<Value<'a>>,
        cols: usize,
        remaining: &mut usize,
        depth: usize,
        pos: usize,
    ) -> Result<Vec<Row>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let mut occupied = vec![0; cols];
        let mut rows = Vec::new();
        let mut row = 0usize;
        let mut column = 0usize;
        let too_large = || Error::Parse("The Typst table exceeds the cell limit.".into());
        let mut charge = |count| {
            *remaining = remaining.checked_sub(count).ok_or_else(too_large)?;
            Ok::<(), Error>(())
        };
        for value in items {
            let (value, width, height, align) = if let Value::Element(cell) = &value
                && cell.name.ends_with(".cell")
            {
                let span = |key| {
                    cell.named
                        .get(key)
                        .and_then(Value::number)
                        .unwrap_or(1.)
                        .max(1.) as usize
                };
                (
                    cell.pos.last().cloned().unwrap_or(Value::None),
                    span("colspan"),
                    span("rowspan"),
                    match cell.named.get("align") {
                        Some(value) => alignment(&self.text(value, pos)?),
                        None => Alignment::AlignDefault,
                    },
                )
            } else {
                (value, 1, 1, Alignment::AlignDefault)
            };
            if width > cols {
                return Err(too_large());
            }
            charge(width.checked_mul(height).ok_or_else(too_large)? - 1)?;
            loop {
                if column <= cols - width {
                    self.work(width.div_ceil(16), pos)?;
                    if occupied[column..column + width]
                        .iter()
                        .all(|end| *end <= row)
                    {
                        break;
                    }
                }
                if column < cols && occupied[column] <= row {
                    charge(1)?;
                    rows.resize_with(rows.len().max(row + 1), Row::default);
                    rows[row].cells.push(Cell::default());
                }
                column += 1;
                if column >= cols {
                    column = 0;
                    row += 1;
                }
            }
            rows.resize_with(rows.len().max(row + height), Row::default);
            let content = finish(self.content(value, depth, pos)?);
            rows[row].cells.push(Cell {
                content,
                align,
                row_span: height as i64,
                col_span: width as i64,
                ..Cell::default()
            });
            occupied[column..column + width].fill(row + height);
            column += width;
        }
        for (index, remaining_row) in rows.iter_mut().enumerate().skip(row) {
            let start = if index == row { column } else { 0 };
            for end in &occupied[start..] {
                if *end <= index {
                    charge(1)?;
                    remaining_row.cells.push(Cell::default());
                }
            }
        }
        Ok(rows)
    }
}

fn alignment(text: &str) -> Alignment {
    match text
        .split(" + ")
        .find(|s| matches!(*s, "left" | "right" | "center"))
    {
        Some("left") => Alignment::AlignLeft,
        Some("right") => Alignment::AlignRight,
        Some("center") => Alignment::AlignCenter,
        _ => Alignment::AlignDefault,
    }
}
