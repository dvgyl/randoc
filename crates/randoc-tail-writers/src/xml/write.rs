use std::collections::BTreeMap;

use randoc_archive::xml::{escape_xml_attr, escape_xml_text_quot};
use randoc_ast::*;
use randoc_core::{Context, Error, Result, WriterOptions};

type Attributes<'a> = Vec<(&'a str, String)>;

enum Event<'a> {
    Block(&'a Block),
    Inlines(&'a [Inline]),
    Inline(&'a Inline),
    Meta(&'a MetaValue),
    Entry(&'a str, &'a MetaValue),
    ListItem(&'a [Block]),
    Definition(&'a [Inline], &'a [Vec<Block>]),
    Line(&'a [Inline]),
    Caption(&'a Caption),
    ShortCaption(&'a [Inline]),
    Columns(&'a [ColSpec]),
    Column(&'a ColSpec),
    Rows(&'static str, Option<&'a Attr>, &'a [Row]),
    Body(&'a TableBody),
    Row(&'a Row),
    Cell(&'a Cell),
    Citations(&'a [Citation]),
    Citation(&'a Citation),
    CitationPart(&'static str, &'a [Inline]),
    Newline,
    EndElements(&'static str),
    DefinitionBody(&'a [Block]),
    Whitespace(&'static str, usize),
    Close(&'static str),
}

struct XmlWriter<'a> {
    output: String,
    stack: Vec<Event<'a>>,
    level: usize,
    limit: u64,
}

pub(crate) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let mut writer = XmlWriter {
        output: String::new(),
        stack: Vec::new(),
        level: 0,
        limit: context.limits.max_input_size,
    };
    if options.template.is_some() {
        writer.push("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;
        writer.open("Pandoc", vec![("api-version", "1,23,1,2".into())])?;
        writer.level = 1;
        writer.newline()?;
        writer.entries("meta", &doc.meta.0)?;
        writer.run()?;
        writer.newline()?;
        writer.elements("blocks", vec![], doc.blocks.iter().map(Event::Block))?;
        writer.run()?;
        writer.level = 0;
        writer.push("\n</Pandoc>")?;
    } else {
        writer
            .stack
            .extend(doc.blocks.iter().rev().map(Event::Block));
        writer.run()?;
    }
    Ok(writer.output)
}

fn attributes<'a>(attr: &'a Attr, properties: Attributes<'a>) -> Attributes<'a> {
    let mut attrs = Vec::new();
    if !attr.id.is_empty() {
        attrs.push(("id", attr.id.clone()));
    }
    if !attr.classes.is_empty() {
        attrs.push(("class", attr.classes.join(" ")));
    }
    attrs.extend(properties);
    attrs.extend(
        attr.attributes
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone())),
    );
    attrs
}

impl<'a> XmlWriter<'a> {
    fn check(&self, bytes: usize) -> Result<()> {
        if self.output.len().saturating_add(bytes) as u64 > self.limit {
            return Err(Error::App("The XML output exceeds the size limit.".into()));
        }
        Ok(())
    }

    fn push(&mut self, text: &str) -> Result<()> {
        self.check(text.len())?;
        self.output.push_str(text);
        Ok(())
    }

    fn newline(&mut self) -> Result<()> {
        let spaces = self.level.saturating_mul(2);
        self.check(spaces.saturating_add(1))?;
        self.output.push('\n');
        self.output.extend(std::iter::repeat_n(' ', spaces));
        Ok(())
    }

    fn tag(&mut self, name: &str, attrs: Attributes<'_>, suffix: &str) -> Result<()> {
        self.push("<")?;
        self.push(name)?;
        for (key, value) in attrs {
            self.push(" ")?;
            self.push(key)?;
            self.push("=\"")?;
            self.escaped(&value, true)?;
            self.push("\"")?;
        }
        self.push(suffix)
    }

    fn escaped(&mut self, value: &str, attr: bool) -> Result<()> {
        for part in value.split_inclusive(['&', '<', '>', '"', '\'']) {
            self.check(part.len())?;
            let (text, apostrophe) = part
                .strip_suffix('\'')
                .map_or((part, false), |text| (text, true));
            if attr {
                escape_xml_attr(text, &mut self.output);
            } else {
                escape_xml_text_quot(text, &mut self.output);
            }
            if apostrophe {
                self.push("&#39;")?;
            }
            self.check(0)?;
        }
        Ok(())
    }

    fn open(&mut self, name: &str, attrs: Attributes<'_>) -> Result<()> {
        self.tag(name, attrs, ">")
    }

    fn close(&mut self, name: &str) -> Result<()> {
        self.push("</")?;
        self.push(name)?;
        self.push(">")
    }

    fn leaf(&mut self, name: &str, attrs: Attributes<'_>, text: &str) -> Result<()> {
        if text.is_empty()
            && !matches!(
                name,
                "CodeBlock" | "RawBlock" | "Code" | "Math" | "RawInline" | "MetaString"
            )
        {
            return self.tag(name, attrs, " />");
        }
        self.open(name, attrs)?;
        self.escaped(text, false)?;
        self.close(name)
    }

    fn mixed(
        &mut self,
        name: &'static str,
        attrs: Attributes<'_>,
        items: &'a [Inline],
    ) -> Result<()> {
        if items.is_empty() {
            return self.leaf(name, attrs, "");
        }
        self.open(name, attrs)?;
        self.stack.push(Event::Close(name));
        self.stack.push(Event::Inlines(items));
        Ok(())
    }

    fn elements(
        &mut self,
        name: &'static str,
        attrs: Attributes<'_>,
        events: impl IntoIterator<Item = Event<'a>>,
    ) -> Result<()> {
        let start = self.stack.len();
        self.stack.push(Event::EndElements(name));
        for event in events {
            self.stack.push(Event::Newline);
            self.stack.push(event);
        }
        if self.stack.len() == start + 1 {
            self.stack.pop();
            return self.leaf(name, attrs, "");
        }
        self.stack[start + 1..].reverse();
        self.level += 1;
        self.open(name, attrs)
    }

    fn entries(
        &mut self,
        name: &'static str,
        entries: &'a BTreeMap<String, MetaValue>,
    ) -> Result<()> {
        self.elements(
            name,
            vec![],
            entries.iter().map(|(key, value)| Event::Entry(key, value)),
        )
    }

    fn blocks(
        &mut self,
        name: &'static str,
        attrs: Attributes<'_>,
        blocks: &'a [Block],
    ) -> Result<()> {
        self.elements(name, attrs, blocks.iter().map(Event::Block))
    }

    fn run(&mut self) -> Result<()> {
        while let Some(event) = self.stack.pop() {
            match event {
                Event::Newline => self.newline()?,
                Event::Close(name) => self.close(name)?,
                Event::EndElements(name) => {
                    self.level -= 1;
                    self.newline()?;
                    self.close(name)?;
                }
                Event::Whitespace(name, count) => {
                    let attrs = if count == 1 {
                        vec![]
                    } else {
                        vec![("count", count.to_string())]
                    };
                    self.leaf(name, attrs, "")?;
                }
                Event::DefinitionBody(blocks) => self.blocks("def", vec![], blocks)?,
                Event::Block(block) => self.block(block)?,
                Event::Inlines(items) => self.inlines(items),
                Event::Inline(inline) => self.inline(inline)?,
                Event::Meta(value) => self.meta(value)?,
                Event::Entry(key, value) => {
                    self.elements("entry", vec![("key", key.into())], [Event::Meta(value)])?
                }
                Event::ListItem(items) => self.blocks("item", vec![], items)?,
                Event::Definition(term, definitions) => self.elements(
                    "item",
                    vec![],
                    std::iter::once(Event::CitationPart("term", term)).chain(
                        definitions
                            .iter()
                            .map(|blocks| Event::DefinitionBody(blocks)),
                    ),
                )?,
                Event::Line(line) => self.mixed("line", vec![], line)?,
                Event::Caption(caption) => self.elements(
                    "Caption",
                    vec![],
                    caption
                        .short
                        .as_deref()
                        .map(Event::ShortCaption)
                        .into_iter()
                        .chain(caption.long.iter().map(Event::Block)),
                )?,
                Event::ShortCaption(items) => self.mixed("ShortCaption", vec![], items)?,
                Event::Columns(cols) => {
                    self.elements("colspecs", vec![], cols.iter().map(Event::Column))?
                }
                Event::Column(col) => self.leaf(
                    "ColSpec",
                    vec![
                        ("alignment", format!("{:?}", col.align)),
                        (
                            "col-width",
                            match col.width {
                                ColWidth::ColWidth(width) => randoc_ast::json::show_double(width),
                                ColWidth::ColWidthDefault => "0".into(),
                            },
                        ),
                    ],
                    "",
                )?,
                Event::Rows(name, attr, rows) => self.elements(
                    name,
                    attr.map_or_else(Vec::new, |attr| attributes(attr, vec![])),
                    rows.iter().map(Event::Row),
                )?,
                Event::Body(body) => self.elements(
                    "TableBody",
                    attributes(
                        &body.attr,
                        vec![("row-head-columns", body.row_head_columns.to_string())],
                    ),
                    [
                        Event::Rows("header", None, &body.head),
                        Event::Rows("body", None, &body.body),
                    ],
                )?,
                Event::Row(row) => self.elements(
                    "Row",
                    attributes(&row.attr, vec![]),
                    row.cells.iter().map(Event::Cell),
                )?,
                Event::Cell(cell) => {
                    let mut properties = Vec::new();
                    if cell.col_span != 1 {
                        properties.push(("col-span", cell.col_span.to_string()));
                    }
                    if cell.row_span != 1 {
                        properties.push(("row-span", cell.row_span.to_string()));
                    }
                    properties.push(("alignment", format!("{:?}", cell.align)));
                    self.blocks("Cell", attributes(&cell.attr, properties), &cell.content)?;
                }
                Event::Citations(citations) => {
                    self.elements("citations", vec![], citations.iter().map(Event::Citation))?
                }
                Event::Citation(citation) => {
                    let properties = vec![
                        ("id", citation.id.clone()),
                        ("mode", format!("{:?}", citation.mode)),
                        ("note-num", citation.note_num.to_string()),
                        ("hash", citation.hash.to_string()),
                    ];
                    let parts = [
                        ("prefix", citation.prefix.as_slice()),
                        ("suffix", citation.suffix.as_slice()),
                    ];
                    self.elements(
                        "Citation",
                        properties,
                        parts
                            .into_iter()
                            .filter(|(_, items)| !items.is_empty())
                            .map(|(name, items)| Event::CitationPart(name, items)),
                    )?;
                }
                Event::CitationPart(name, items) => self.mixed(name, vec![], items)?,
            }
        }
        Ok(())
    }

    fn block(&mut self, block: &'a Block) -> Result<()> {
        match block {
            Block::Plain(items) => self.mixed("Plain", vec![], items),
            Block::Para(items) => self.mixed("Para", vec![], items),
            Block::Header(level, attr, items) => self.mixed(
                "Header",
                attributes(attr, vec![])
                    .into_iter()
                    .chain([("level", level.to_string())])
                    .collect(),
                items,
            ),
            Block::LineBlock(lines) => self.elements(
                "LineBlock",
                vec![],
                lines.iter().map(|line| Event::Line(line)),
            ),
            Block::CodeBlock(attr, text) => self.leaf("CodeBlock", attributes(attr, vec![]), text),
            Block::RawBlock(format, text) => {
                self.leaf("RawBlock", vec![("format", format.0.clone())], text)
            }
            Block::BlockQuote(blocks) => self.blocks("BlockQuote", vec![], blocks),
            Block::Div(attr, blocks) => self.blocks("Div", attributes(attr, vec![]), blocks),
            Block::BulletList(items) => self.elements(
                "BulletList",
                vec![],
                items.iter().map(|item| Event::ListItem(item)),
            ),
            Block::OrderedList(attr, items) => self.elements(
                "OrderedList",
                vec![
                    ("start", attr.start.to_string()),
                    ("number-style", format!("{:?}", attr.style)),
                    ("number-delim", format!("{:?}", attr.delim)),
                ],
                items.iter().map(|item| Event::ListItem(item)),
            ),
            Block::DefinitionList(items) => self.elements(
                "DefinitionList",
                vec![],
                items
                    .iter()
                    .map(|(term, defs)| Event::Definition(term, defs)),
            ),
            Block::HorizontalRule => self.leaf("HorizontalRule", vec![], ""),
            Block::Figure(attr, caption, items) => self.elements(
                "Figure",
                attributes(attr, vec![]),
                std::iter::once(Event::Caption(caption)).chain(items.iter().map(Event::Block)),
            ),
            Block::Table(table) => self.elements(
                "Table",
                attributes(&table.attr, vec![]),
                [
                    Event::Caption(&table.caption),
                    Event::Columns(&table.col_specs),
                    Event::Rows("TableHead", Some(&table.head.attr), &table.head.rows),
                ]
                .into_iter()
                .chain(table.bodies.iter().map(Event::Body))
                .chain([Event::Rows(
                    "TableFoot",
                    Some(&table.foot.attr),
                    &table.foot.rows,
                )]),
            ),
        }
    }

    fn inlines(&mut self, items: &'a [Inline]) {
        let start = self.stack.len();
        let mut i = 0;
        let is_space = |index| matches!(items.get(index), Some(Inline::Space));
        let text_whitespace = |index: usize| match items.get(index) {
            Some(Inline::SoftBreak) => true,
            Some(Inline::Space) => {
                index > 0 && index + 1 < items.len() && !is_space(index - 1) && !is_space(index + 1)
            }
            _ => false,
        };
        while let Some(item) = items.get(i) {
            let count = if is_space(i) {
                items[i..]
                    .iter()
                    .take_while(|item| matches!(item, Inline::Space))
                    .count()
            } else {
                1
            };
            let adjacent =
                text_whitespace(i + count) || i.checked_sub(1).is_some_and(text_whitespace);
            match item {
                Inline::Space if !text_whitespace(i) || adjacent => {
                    self.stack.push(Event::Whitespace("Space", count));
                }
                Inline::SoftBreak if adjacent => {
                    self.stack.push(Event::Whitespace("SoftBreak", 1));
                }
                _ => self.stack.push(Event::Inline(item)),
            }
            i += count;
        }
        self.stack[start..].reverse();
    }

    fn inline(&mut self, inline: &'a Inline) -> Result<()> {
        match inline {
            Inline::Str(text) if text.is_empty() => self.tag("Str", vec![], " />"),
            Inline::Str(text) if text.contains(' ') => {
                self.push("<Str content=\"")?;
                self.escaped(text, true)?;
                self.push("\" />")
            }
            Inline::Str(text) => self.escaped(text, false),
            Inline::Space => self.push(" "),
            Inline::SoftBreak => self.push("\n"),
            Inline::LineBreak => self.leaf("LineBreak", vec![], ""),
            Inline::Emph(items) => self.mixed("Emph", vec![], items),
            Inline::Strong(items) => self.mixed("Strong", vec![], items),
            Inline::Underline(items) => self.mixed("Underline", vec![], items),
            Inline::Strikeout(items) => self.mixed("Strikeout", vec![], items),
            Inline::Superscript(items) => self.mixed("Superscript", vec![], items),
            Inline::Subscript(items) => self.mixed("Subscript", vec![], items),
            Inline::SmallCaps(items) => self.mixed("SmallCaps", vec![], items),
            Inline::Quoted(kind, items) => {
                self.mixed("Quoted", vec![("quote-type", format!("{kind:?}"))], items)
            }
            Inline::Code(attr, text) => self.leaf("Code", attributes(attr, vec![]), text),
            Inline::Math(kind, text) => {
                self.leaf("Math", vec![("math-type", format!("{kind:?}"))], text)
            }
            Inline::RawInline(format, text) => {
                self.leaf("RawInline", vec![("format", format.0.clone())], text)
            }
            Inline::Link(attr, items, target) | Inline::Image(attr, items, target) => {
                let (name, key) = if matches!(inline, Inline::Link(..)) {
                    ("Link", "href")
                } else {
                    ("Image", "src")
                };
                let mut props = Vec::new();
                if !target.url.is_empty() {
                    props.push((key, target.url.clone()));
                }
                if !target.title.is_empty() {
                    props.push(("title", target.title.clone()));
                }
                self.mixed(
                    name,
                    attributes(attr, vec![]).into_iter().chain(props).collect(),
                    items,
                )
            }
            Inline::Span(attr, items) => self.mixed("Span", attributes(attr, vec![]), items),
            Inline::Note(blocks) => self.blocks("Note", vec![], blocks),
            Inline::Cite(cites, items) => {
                self.open("Cite", vec![])?;
                self.stack.push(Event::Close("Cite"));
                self.stack.push(Event::Inlines(items));
                self.stack.push(Event::Citations(cites));
                Ok(())
            }
        }
    }

    fn meta(&mut self, value: &'a MetaValue) -> Result<()> {
        match value {
            MetaValue::MetaString(text) => self.leaf("MetaString", vec![], text),
            MetaValue::MetaBool(value) => {
                self.leaf("MetaBool", vec![("value", value.to_string())], "")
            }
            MetaValue::MetaInlines(items) => self.mixed("MetaInlines", vec![], items),
            MetaValue::MetaBlocks(blocks) => self.blocks("MetaBlocks", vec![], blocks),
            MetaValue::MetaList(items) => {
                self.elements("MetaList", vec![], items.iter().map(Event::Meta))
            }
            MetaValue::MetaMap(entries) => self.entries("MetaMap", entries),
        }
    }
}
