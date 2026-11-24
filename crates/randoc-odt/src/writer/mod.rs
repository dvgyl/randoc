//! Renders OpenDocument content and assembles ODT and FODT files.

mod inline;
mod package;
mod show;
mod tables;

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use randoc_archive::xml::escape_xml_attr;
use randoc_ast::{
    Attr, Block, Caption, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, MetaValue,
    Pandoc,
};
use randoc_citeproc::{Lang, parse_lang};
use randoc_core::shared::meta_context::meta_to_context;
use randoc_core::{
    BinaryWriter, CaptionPosition, Context, Error, Extension, LogMessage, Result, TemplateWriter,
    WrapOption, Writer, WriterOptions,
};
use randoc_layout::{Doc, cr, hcat, nest, text, vcat};
use randoc_templates::{Template, Val};

use self::package::Resources;

const BODY: &str = "Text_20_body";
const FIRST: &str = "First_20_paragraph";
const RTL: &str = "style:writing-mode=\"rl-tb\" fo:text-align=\"right\"";
const LTR: &str = "style:writing-mode=\"lr-tb\" fo:text-align=\"left\"";
const FRAME_STYLES: &str = concat!(
    "<style:style style:name=\"fr2\" style:family=\"graphic\" style:parent-style-name=\"Formula\"><style:graphic-properties style:vertical-pos=\"middle\" style:vertical-rel=\"text\" style:horizontal-pos=\"center\" style:horizontal-rel=\"paragraph-content\" style:wrap=\"none\" /></style:style>\n",
    "<style:style style:name=\"fr1\" style:family=\"graphic\" style:parent-style-name=\"Formula\"><style:graphic-properties style:vertical-pos=\"middle\" style:vertical-rel=\"text\" /></style:style>"
);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Fragment,
    Odt,
    Fodt,
}

pub(crate) fn writer(format: &str) -> Option<Writer> {
    match format {
        "opendocument" => Some(Writer::Text(Arc::new(TemplateWriter(write_opendocument)))),
        "odt" => Some(Writer::Binary(Arc::new(OdtWriter))),
        "fodt" => Some(Writer::Text(Arc::new(TemplateWriter(package::write_fodt)))),
        _ => None,
    }
}

fn write_opendocument(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    render(
        doc,
        options,
        context,
        Format::Fragment,
        Resources::default(),
    )
    .map(|(content, _)| content)
}

struct OdtWriter;

impl BinaryWriter for OdtWriter {
    fn write(&self, doc: &Pandoc, opts: &WriterOptions, ctx: &mut Context) -> Result<Vec<u8>> {
        package::write_odt(doc, opts, ctx)
    }

    fn renders_template(&self) -> bool {
        true
    }
}

struct Renderer<'a> {
    options: &'a WriterOptions,
    context: &'a mut Context,
    format: Format,
    resources: Resources,
    text_styles: Vec<String>,
    text_style_index: HashMap<(u8, BTreeSet<Lang>), usize>,
    /// The languages of the enclosing divs and spans. The greatest one
    /// gives the language of the text.
    languages: BTreeSet<Lang>,
    direction: Option<&'static str>,
    paragraph_styles: Vec<String>,
    paragraph_style_index: HashMap<(String, String), String>,
    list_styles: BTreeMap<(u8, u8), String>,
    list_index: HashMap<(ListNumberStyle, ListNumberDelim), usize>,
    table_styles: Vec<String>,
    table_count: usize,
    table_cells: usize,
    note_count: usize,
    image_count: usize,
    figure_count: usize,
    quote_depth: usize,
    references: HashMap<String, &'static str>,
    registry: Option<randoc_highlight::Registry>,
}

fn render(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
    format: Format,
    resources: Resources,
) -> Result<(String, Resources)> {
    let depth = context.limits.max_nesting_depth;
    let metadata = |key| doc.meta.0.get(key).and_then(package::meta_text);
    let language = || match options.variables.0.get("lang") {
        Some(Val::Simple(value)) => Some(value.render(None)),
        Some(Val::Bool(_)) => None,
        _ => metadata("lang"),
    };
    let direction = match metadata("dir").as_deref() {
        Some("rtl") => Some(RTL),
        None if language()
            .and_then(|value| parse_lang(&value).ok())
            .is_some_and(|lang| {
                matches!(
                    lang.language.as_str(),
                    "ar" | "ckb" | "dv" | "fa" | "he" | "sd" | "ur" | "yi"
                )
            }) =>
        {
            Some(RTL)
        }
        _ => None,
    };
    let mut renderer = Renderer {
        options,
        context,
        format,
        resources,
        text_styles: Vec::new(),
        text_style_index: HashMap::new(),
        languages: BTreeSet::new(),
        direction,
        paragraph_styles: Vec::new(),
        paragraph_style_index: HashMap::new(),
        list_styles: BTreeMap::new(),
        list_index: HashMap::new(),
        table_styles: Vec::new(),
        table_count: 0,
        table_cells: 0,
        note_count: 0,
        image_count: 0,
        figure_count: 0,
        quote_depth: 0,
        references: HashMap::new(),
        registry: highlight_registry(options),
    };
    if renderer.has(Extension::XrefsName) || renderer.has(Extension::XrefsNumber) {
        renderer.collect_references(&doc.blocks, 0)?;
    }
    let body = renderer.blocks(&doc.blocks, None, 0, true)?;
    let width = (format == Format::Fragment && options.wrap_text == WrapOption::WrapAuto)
        .then_some(options.columns.max(1) as usize);
    let content = if options.template.is_some() || format != Format::Fragment {
        let abstract_value = doc.meta.0.get("abstract");
        let title_value = doc.meta.0.get("title");
        let cell = RefCell::new(&mut renderer);
        let mut fields = meta_to_context(
            &doc.meta,
            &options.variables,
            depth,
            &mut |blocks| {
                if matches!(title_value, Some(MetaValue::MetaBlocks(value)) if std::ptr::eq(value.as_slice(), blocks))
                {
                    let mut renderer = cell.borrow_mut();
                    let mut title = Vec::new();
                    renderer.title_inlines(blocks, &mut title, 0)?;
                    return renderer.inlines(&title, 0);
                }
                let abstract_blocks = matches!(abstract_value, Some(MetaValue::MetaBlocks(value)) if std::ptr::eq(value.as_slice(), blocks));
                let style = abstract_blocks.then_some("Abstract");
                cell.borrow_mut().blocks(blocks, style, 0, !abstract_blocks)
            },
            &mut |inlines| cell.borrow_mut().inlines(inlines, 0),
        )?;
        if !options.variables.0.contains_key("abstract")
            && matches!(
                abstract_value,
                Some(MetaValue::MetaInlines(_) | MetaValue::MetaString(_))
            )
            && let Some(Val::Simple(content)) = fields.0.remove("abstract")
        {
            fields.insert("abstract", Val::Simple(para("Abstract", content)));
        }
        let styles = vcat(
            renderer
                .text_styles
                .iter()
                .map(text)
                .chain([text(FRAME_STYLES)])
                .chain(
                    renderer
                        .paragraph_styles
                        .iter()
                        .chain(&renderer.table_styles)
                        .chain(renderer.list_styles.values())
                        .map(text),
                ),
        );
        fields.insert("automatic-styles", Val::Simple(styles));
        fields.insert("body", Val::Simple(body));
        fields
            .0
            .entry("toc".into())
            .or_insert(Val::Bool(options.table_of_contents));
        fields
            .0
            .entry("toc-depth".into())
            .or_insert_with(|| Val::text(options.toc_depth.to_string()));
        let template = if let Some(template) = &options.template {
            template.clone()
        } else {
            let source = randoc_data::default_template("opendocument")
                .ok_or_else(|| Error::App("The OpenDocument template is missing.".into()))?;
            Template::compile("default.opendocument", source, depth, &mut |path| {
                Err(Error::App(format!(
                    "The OpenDocument template partial {path} is missing."
                )))
            })
            .map_err(|error| Error::App(error.to_string()))?
        };
        renderer
            .context
            .render_template(&template, &fields, width)?
    } else {
        body.render(width)
    };
    Ok((content, renderer.resources))
}

impl Renderer<'_> {
    fn title_inlines(&self, blocks: &[Block], out: &mut Vec<Inline>, depth: usize) -> Result<()> {
        self.check_depth(depth)?;
        for (index, block) in blocks.iter().enumerate() {
            if index > 0 {
                out.push(Inline::LineBreak);
            }
            match block {
                Block::Plain(text) | Block::Para(text) | Block::Header(_, _, text) => {
                    out.extend_from_slice(text);
                }
                Block::LineBlock(lines) => {
                    for (index, line) in lines.iter().enumerate() {
                        if index > 0 {
                            out.push(Inline::LineBreak);
                        }
                        out.extend_from_slice(line);
                    }
                }
                Block::CodeBlock(attr, text) => out.push(Inline::Code(attr.clone(), text.clone())),
                Block::RawBlock(format, text) => {
                    out.push(Inline::RawInline(format.clone(), text.clone()))
                }
                Block::BlockQuote(blocks) | Block::Div(_, blocks) | Block::Figure(_, _, blocks) => {
                    self.title_inlines(blocks, out, depth + 1)?;
                }
                Block::BulletList(items) | Block::OrderedList(_, items) => {
                    for item in items {
                        self.title_inlines(item, out, depth + 1)?;
                    }
                }
                Block::DefinitionList(items) => {
                    for (term, definitions) in items {
                        out.extend_from_slice(term);
                        out.extend([Inline::Str(":".into()), Inline::Space]);
                        for definition in definitions {
                            self.title_inlines(definition, out, depth + 1)?;
                        }
                    }
                }
                Block::Table(table) => {
                    let rows = table
                        .head
                        .rows
                        .iter()
                        .chain(
                            table
                                .bodies
                                .iter()
                                .flat_map(|body| body.head.iter().chain(&body.body)),
                        )
                        .chain(&table.foot.rows);
                    for (index, row) in rows.enumerate() {
                        if index > 0 {
                            out.push(Inline::LineBreak);
                        }
                        for (index, cell) in row.cells.iter().enumerate() {
                            if index > 0 {
                                out.push(Inline::Space);
                            }
                            self.title_inlines(&cell.content, out, depth + 1)?;
                        }
                    }
                }
                Block::HorizontalRule => {}
            }
        }
        Ok(())
    }

    fn has(&self, extension: Extension) -> bool {
        self.options.extensions.contains(extension)
    }

    fn check_depth(&self, depth: usize) -> Result<()> {
        if depth >= self.context.limits.max_nesting_depth {
            return Err(Error::App(
                "The OpenDocument content is nested too deeply.".into(),
            ));
        }
        Ok(())
    }

    fn collect_references(&mut self, blocks: &[Block], depth: usize) -> Result<()> {
        self.check_depth(depth)?;
        for block in blocks {
            match block {
                Block::Header(_, attr, _) => {
                    self.references.insert(attr.id.clone(), "bookmark");
                }
                Block::Figure(attr, caption, children) => {
                    self.references.insert(attr.id.clone(), "sequence");
                    self.collect_references(children, depth + 1)?;
                    self.collect_references(&caption.long, depth + 1)?;
                }
                Block::Table(table) => {
                    self.references.insert(table.attr.id.clone(), "sequence");
                }
                Block::BlockQuote(children) | Block::Div(_, children) => {
                    self.collect_references(children, depth + 1)?
                }
                Block::BulletList(items) | Block::OrderedList(_, items) => {
                    for item in items {
                        self.collect_references(item, depth + 1)?;
                    }
                }
                Block::DefinitionList(items) => {
                    for (_, definitions) in items {
                        for definition in definitions {
                            self.collect_references(definition, depth + 1)?;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn blocks(
        &mut self,
        blocks: &[Block],
        style: Option<&str>,
        depth: usize,
        separated: bool,
    ) -> Result<Doc> {
        self.check_depth(depth)?;
        let mut first = false;
        let mut out = Vec::with_capacity(blocks.len());
        for block in blocks {
            let paragraph_style = style.unwrap_or(if first { FIRST } else { BODY });
            out.push(self.block(block, paragraph_style, depth)?);
            first = matches!(
                block,
                Block::Header(..)
                    | Block::CodeBlock(..)
                    | Block::BlockQuote(..)
                    | Block::OrderedList(..)
                    | Block::BulletList(..)
                    | Block::DefinitionList(..)
                    | Block::Table(..)
                    | Block::HorizontalRule
            );
        }
        Ok(if separated { vcat(out) } else { hcat(out) })
    }

    fn block(&mut self, block: &Block, style: &str, depth: usize) -> Result<Doc> {
        Ok(match block {
            Block::Plain(inlines) | Block::Para(inlines) => {
                if inlines.is_empty() {
                    if self.has(Extension::EmptyParagraphs) || !matches!(style, BODY | FIRST) {
                        para(&self.directional_style(style), Doc::default())
                    } else {
                        Doc::default()
                    }
                } else {
                    para(
                        &self.directional_style(style),
                        self.inlines(inlines, depth + 1)?,
                    )
                }
            }
            Block::Header(level, attr, inlines) => {
                let level = level.clamp(&1, &10);
                let parent = format!("Heading_20_{level}");
                let style = self.directional_style(&parent);
                let content = self.inlines(inlines, depth + 1)?;
                tag(
                    "text:h",
                    &format!(" text:style-name=\"{style}\" text:outline-level=\"{level}\""),
                    bookmark(&attr.id, content),
                )
            }
            Block::Div(attr, blocks) => {
                let custom = attr.get("custom-style");
                let inner_style = custom.or_else(|| (style != BODY).then_some(style));
                let previous = self.direction;
                self.direction = match attr.get("dir") {
                    Some("rtl") => Some(RTL),
                    Some("ltr") => Some(LTR),
                    _ => previous,
                };
                let body = self.with_language(attr, |renderer| {
                    renderer.blocks(blocks, inner_style, depth + 1, custom.is_none())
                });
                self.direction = previous;
                let body = body?;
                if attr.id.is_empty() {
                    body
                } else {
                    tag("text:section", &attribute("text:name", &attr.id), body)
                }
            }
            Block::BlockQuote(blocks) => {
                let style = if self.quote_depth == 0 {
                    "Quotations".into()
                } else {
                    let margin = self.quote_depth as f64 * 0.5;
                    self.paragraph_style("Quotations", &format!("fo:margin-left=\"{margin}in\" fo:margin-right=\"0in\" fo:text-indent=\"0in\" style:auto-text-indent=\"false\""))
                };
                self.quote_depth += 1;
                let body = self.blocks(blocks, Some(&style), depth + 1, true);
                self.quote_depth -= 1;
                body?
            }
            Block::HorizontalRule => text("<text:p text:style-name=\"Horizontal_20_Line\" />"),
            Block::RawBlock(format, raw) if format.0 == "opendocument" => text(raw),
            Block::RawBlock(..) => Doc::default(),
            Block::BulletList(items) => self.list(items, None, depth + 1)?,
            Block::OrderedList(attributes, items) => {
                self.list(items, Some(attributes), depth + 1)?
            }
            Block::DefinitionList(items) => self.definitions(items, depth + 1)?,
            Block::CodeBlock(attr, code) => self.code_block(attr, code),
            Block::LineBlock(lines) => {
                let mut body = Doc::default();
                for (index, line) in lines.iter().enumerate() {
                    if index > 0 {
                        body += text("<text:line-break />");
                    }
                    body += self.inlines(line, depth + 1)?;
                }
                para(style, body)
            }
            Block::Table(table) => self.table(table, depth + 1)?,
            Block::Figure(attr, caption, blocks) => {
                self.figure_count += 1;
                let number = self.figure_count;
                let style = if caption.long.is_empty() {
                    "Figure"
                } else {
                    "FigureWithCaption"
                };
                let content = self.blocks(blocks, Some(style), depth + 1, true)?;
                let caption = self.caption(
                    caption,
                    "Figure",
                    "Illustration",
                    &attr.id,
                    number,
                    depth + 1,
                )?;
                match self.options.figure_caption_position {
                    CaptionPosition::CaptionAbove => vcat([caption, content]),
                    CaptionPosition::CaptionBelow => vcat([content, caption]),
                }
            }
        })
    }

    fn caption(
        &mut self,
        caption: &Caption,
        label: &str,
        category: &str,
        id: &str,
        index: usize,
        depth: usize,
    ) -> Result<Doc> {
        let style = format!("{label}Caption");
        if self.has(Extension::NativeNumbering)
            && let Some((Block::Para(inlines) | Block::Plain(inlines), rest)) =
                caption.long.split_first()
        {
            let name = if id.is_empty() {
                format!("ref{category}{}", index - 1)
            } else {
                escaped(id)
            };
            let prefix = text(format!(
                "{label} <text:sequence text:ref-name=\"{name}\" text:name=\"{category}\" text:formula=\"ooow:{category}+1\" style:num-format=\"1\">{index}</text:sequence>: "
            ));
            return Ok(vcat([
                para(&style, prefix + self.inlines(inlines, depth)?),
                self.blocks(rest, Some(&style), depth, true)?,
            ]));
        }
        self.blocks(&caption.long, Some(&style), depth, true)
    }

    fn directional_style<'a>(&mut self, parent: &'a str) -> Cow<'a, str> {
        match self.direction {
            Some(properties) => Cow::Owned(self.paragraph_style(parent, properties)),
            None => Cow::Borrowed(parent),
        }
    }

    fn paragraph_style(&mut self, parent: &str, properties: &str) -> String {
        let key = (parent.to_owned(), properties.to_owned());
        if let Some(name) = self.paragraph_style_index.get(&key) {
            return name.clone();
        }
        let name = self.add_paragraph_style(parent, properties);
        self.paragraph_style_index.insert(key, name.clone());
        name
    }

    fn add_paragraph_style(&mut self, parent: &str, properties: &str) -> String {
        let name = format!("P{}", self.paragraph_styles.len() + 1);
        let parent = escaped(parent);
        self.paragraph_styles.push(format!("<style:style style:name=\"{name}\" style:family=\"paragraph\" style:parent-style-name=\"{parent}\">\n  <style:paragraph-properties {properties} />\n</style:style>"));
        name
    }

    fn list(
        &mut self,
        items: &[Vec<Block>],
        attrs: Option<&ListAttributes>,
        depth: usize,
    ) -> Result<Doc> {
        self.check_depth(depth)?;
        let style = attrs.map_or_else(
            || "List_20_1".to_owned(),
            |attrs| self.numbering_style(attrs),
        );
        let mut attributes = attribute("text:style-name", &style);
        if let Some(attrs) = attrs.filter(|attrs| attrs.start != 1) {
            attributes.push_str(&attribute("text:start-value", &attrs.start.to_string()));
        }
        let family = if attrs.is_some() {
            "List_20_Number"
        } else {
            "List_20_Bullet"
        };
        let mut rendered = Vec::with_capacity(items.len());
        for blocks in items {
            let mut item = Doc::default();
            for block in blocks {
                let style = if matches!(block, Block::Plain(_)) {
                    format!("{family}_20_Tight")
                } else {
                    family.to_owned()
                };
                if !item.is_empty()
                    && (attrs.is_some()
                        || !matches!(block, Block::BulletList(_) | Block::OrderedList(..)))
                {
                    item += cr();
                }
                item += self.block(block, &style, depth)?;
            }
            rendered.push(structure("text:list-item", "", item));
        }
        Ok(structure("text:list", &attributes, vcat(rendered)))
    }

    fn numbering_style(&mut self, attrs: &ListAttributes) -> String {
        use ListNumberDelim::{DefaultDelim, OneParen, Period, TwoParens};
        use ListNumberStyle::{
            Decimal, DefaultStyle, Example, LowerAlpha, LowerRoman, UpperAlpha, UpperRoman,
        };
        if matches!(attrs.style, Decimal | DefaultStyle)
            && matches!(attrs.delim, Period | DefaultDelim)
        {
            return "Numbering_20_1".into();
        }
        let next = self.list_index.len() + 1;
        let index = *self
            .list_index
            .entry((attrs.style, attrs.delim))
            .or_insert(next);
        let name = format!("Pandoc_5f_Numbering_5f_{index}");
        if index == next {
            let number = match attrs.style {
                LowerAlpha => "a",
                UpperAlpha => "A",
                LowerRoman => "i",
                UpperRoman => "I",
                Decimal | DefaultStyle | Example => "1",
            };
            let suffix = match attrs.delim {
                OneParen | TwoParens => ")",
                Period | DefaultDelim => ".",
            };
            let prefix = if attrs.delim == TwoParens {
                attribute("style:num-prefix", "(")
            } else {
                String::new()
            };
            let mut levels = Vec::new();
            for level in 1..=10 {
                let margin = 0.1972 * f64::from(level - 1);
                let attrs = format!(
                    " text:level=\"{level}\" text:style-name=\"Numbering_20_Symbols\" style:num-format=\"{number}\"{prefix} style:num-suffix=\"{suffix}\""
                );
                let properties = text(format!(
                    "<style:list-level-properties text:space-before=\"{margin:.4}in\" text:min-label-width=\"0.1965in\" text:min-label-distance=\"0.1in\" />"
                ));
                levels.push(structure(
                    "text:list-level-style-number",
                    &attrs,
                    properties,
                ));
            }
            self.list_styles.insert(
                (attrs.style as u8, attrs.delim as u8),
                structure(
                    "text:list-style",
                    &attribute("style:name", &name),
                    vcat(levels),
                )
                .render(None),
            );
        }
        name
    }

    fn definitions(
        &mut self,
        items: &[(Vec<randoc_ast::Inline>, Vec<Vec<Block>>)],
        depth: usize,
    ) -> Result<Doc> {
        let mut out = Vec::new();
        for (term, definitions) in items {
            let tight = definitions
                .iter()
                .flatten()
                .all(|block| !matches!(block, Block::Para(_)));
            let (term_style, def_style) = if tight {
                (
                    "Definition_20_Term_20_Tight",
                    "Definition_20_Definition_20_Tight",
                )
            } else {
                ("Definition_20_Term", "Definition_20_Definition")
            };
            out.push(para(term_style, self.inlines(term, depth)?));
            self.quote_depth += 1;
            let rendered = definitions
                .iter()
                .map(|definition| self.blocks(definition, Some(def_style), depth, false))
                .collect::<Result<Vec<_>>>();
            self.quote_depth -= 1;
            out.extend(rendered?);
        }
        Ok(vcat(out))
    }

    fn code_block(&mut self, attr: &Attr, code: &str) -> Doc {
        let tab_stop = self.options.tab_stop.clamp(1, 1024) as usize;
        let highlighter = self
            .registry
            .as_ref()
            .unwrap_or_else(|| randoc_highlight::Registry::embedded());
        let highlighted = self.options.highlight_style.as_ref().and_then(|_| {
            highlighter.highlight_classes(
                &attr.classes,
                code,
                self.context.limits.max_nesting_depth,
            )
        });
        match highlighted {
            Some(Ok(code)) => {
                vcat(code.lines.iter().map(|line| {
                    // Pandoc escapes the text of each token on its own.
                    let spans = hcat(line.iter().map(|(kind, value)| {
                        let content = code_text(value, tab_stop, true);
                        span(&format!("{}Tok", kind.name()), text(content))
                    }));
                    para("Preformatted_20_Text", cr() + spans + cr())
                }))
            }
            Some(Err(message)) => {
                self.context.report(LogMessage::CouldNotHighlight(message));
                plain_code(code, tab_stop)
            }
            None => plain_code(code, tab_stop),
        }
    }
}

fn highlight_registry(options: &WriterOptions) -> Option<randoc_highlight::Registry> {
    if options.syntax_definitions.is_empty() || options.highlight_style.is_none() {
        return None;
    }
    let mut registry = randoc_highlight::Registry::embedded().clone();
    for file in &options.syntax_definitions {
        if let Ok(source) = std::str::from_utf8(&file.bytes) {
            let _ = registry.add_definition(&file.path, source);
        }
    }
    Some(registry)
}

/// Writes one paragraph for each line. As with Haskell's `lines`, a final
/// line break does not start one more line.
fn plain_code(code: &str, tab_stop: usize) -> Doc {
    vcat(code.split_terminator('\n').map(|line| {
        para(
            "Preformatted_20_Text",
            text(code_text(line, tab_stop, true)),
        )
    }))
}

fn code_text(value: &str, tab_stop: usize, at_start: bool) -> String {
    let mut out = String::new();
    let mut spaces = 0usize;
    let mut at_start = at_start;
    let mut column = 0;
    for ch in value.chars().chain(['\0']) {
        if ch == ' ' || ch == '\t' {
            let advance = if ch == '\t' {
                tab_stop - column % tab_stop
            } else {
                1
            };
            column += advance;
            spaces += advance;
            continue;
        }
        if spaces > 0 {
            if !at_start {
                out.push(' ');
                spaces -= 1;
            }
            if spaces > 0 {
                out.push_str(&format!("<text:s text:c=\"{spaces}\" />"));
            }
            spaces = 0;
        }
        at_start = false;
        column += 1;
        if ch != '\0' {
            push_escaped(&mut out, ch);
        }
    }
    out
}

fn escaped(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    escape_xml_attr(value, &mut out);
    out
}

fn push_escaped(out: &mut String, ch: char) {
    let mut buf = [0; 4];
    escape_xml_attr(ch.encode_utf8(&mut buf), out);
}

fn attribute(name: &str, value: &str) -> String {
    format!(" {name}=\"{}\"", escaped(value))
}

fn tag(name: &str, attributes: &str, content: Doc) -> Doc {
    text(format!("<{name}{attributes}>")) + content + text(format!("</{name}>"))
}

fn structure(name: &str, attributes: &str, content: Doc) -> Doc {
    tag(name, attributes, cr() + nest(2, content) + cr())
}

fn para(style: &str, content: Doc) -> Doc {
    tag("text:p", &attribute("text:style-name", style), content)
}

fn span(style: &str, content: Doc) -> Doc {
    tag("text:span", &attribute("text:style-name", style), content)
}

fn bookmark(id: &str, content: Doc) -> Doc {
    if id.is_empty() {
        return content;
    }
    let id = escaped(id);
    text(format!("<text:bookmark-start text:name=\"{id}\" />"))
        + content
        + text(format!("<text:bookmark-end text:name=\"{id}\" />"))
}

#[cfg(test)]
mod wave9_tests;
