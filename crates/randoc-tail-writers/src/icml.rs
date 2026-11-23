use std::collections::BTreeSet;
use std::fmt::Write;

use randoc_archive::{media::image_size_and_mime, xml::escape_xml_attr};
use randoc_ast::{
    Alignment, Attr, Block, Inline, ListAttributes, ListNumberStyle, Pandoc, QuoteType, Table,
};
use randoc_core::shared::sections::make_sections;
use randoc_core::{Context, LogMessage, Result, WriterOptions};
use randoc_layout::literal;
use randoc_templates::Val;

use super::{append_output, charge_output, check_output, join_output};

const NORMAL: &str = "$ID/NormalCharacterStyle";
const BASE: &str = "$ID/NormalParagraphStyle";
const PARAGRAPH_BREAK: &str = "\n<Br />\n";
const STYLE_NAMES: [&str; 9] = [
    "Bold",
    "Code",
    "Italic",
    "Link",
    "SmallCaps",
    "Strikeout",
    "Subscript",
    "Superscript",
    "Underline",
];

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Style(u16);

impl Style {
    fn with(self, index: usize) -> Self {
        Self(self.0 | (1 << index))
    }

    fn name(self) -> String {
        STYLE_NAMES
            .into_iter()
            .enumerate()
            .filter_map(|(i, name)| (self.with(i) == self).then_some(name))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub(super) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let limits = context.limits;
    let depth = limits.max_nesting_depth;
    let sections = make_sections(doc.blocks.clone(), false, None, depth)?;
    let mut state = Renderer {
        context,
        options,
        paragraphs: BTreeSet::new(),
        characters: BTreeSet::new(),
        links: Vec::new(),
        link_budget: limits.max_input_size,
        objects: BTreeSet::new(),
    };
    let body = state.blocks(&sections, "")?;
    if options.template.is_none() {
        return Ok(body);
    }
    let limit = limits.max_input_size;
    let paragraph_styles = join_output(
        state
            .paragraphs
            .iter()
            .map(|style| Ok(paragraph_style(style))),
        "\n",
        limit,
        false,
    )?;
    let character_styles = join_output(
        state
            .characters
            .iter()
            .map(|style| Ok(character_style(style))),
        "\n",
        limit,
        false,
    )?;
    let hyperlinks = state.hyperlinks()?;
    let objects = join_output(state.objects.iter().map(|style| {
        let name = xml(style);
        Ok(format!("<ObjectStyle Self=\"ObjectStyle/{name}\" Name=\"{name}\">\n  <Properties>\n    <BasedOn type=\"string\">$ID/None</BasedOn>\n  </Properties>\n</ObjectStyle>"))
    }), "\n", limit, false)?;
    super::render_template(
        doc,
        options,
        limits,
        literal(body),
        &mut |blocks| Ok(literal(randoc_core::shared::stringify_blocks(blocks))),
        &mut |inlines| Ok(literal(randoc_core::shared::stringify(inlines))),
        [
            ("parStyles", Val::text(paragraph_styles)),
            ("charStyles", Val::text(character_styles)),
            ("hyperlinks", Val::text(hyperlinks)),
            ("objectStyles", Val::text(objects)),
        ],
    )
}

fn heading_style(level: i64, attr: &Attr) -> String {
    let suffix = if attr.has_class("unnumbered") {
        " (unnumbered)"
    } else {
        ""
    };
    format!("Header{level}{suffix}")
}

fn xml(text: &str) -> String {
    let mut out = String::new();
    escape_xml_attr(text, &mut out);
    out
}

fn indent(text: &str, level: usize, limit: u64) -> Result<String> {
    let lines = text.bytes().filter(|&b| b == b'\n').count() + 1;
    check_output(
        text.len().saturating_add(lines.saturating_mul(level * 2)),
        limit,
    )?;
    let prefix = "  ".repeat(level);
    let mut output = String::new();
    for (index, line) in text.split('\n').enumerate() {
        if index != 0 {
            output.push('\n');
        }
        output.push_str(&prefix);
        output.push_str(line);
    }
    Ok(output)
}

fn combine(prefix: &str, style: &str) -> String {
    if prefix.is_empty() {
        style.into()
    } else if style.is_empty() {
        prefix.into()
    } else {
        format!("{prefix} > {style}")
    }
}

struct Renderer<'a> {
    context: &'a mut Context,
    options: &'a WriterOptions,
    paragraphs: BTreeSet<String>,
    characters: BTreeSet<String>,
    links: Vec<String>,
    link_budget: u64,
    objects: BTreeSet<String>,
}

impl Renderer<'_> {
    fn limit(&self) -> u64 {
        self.context.limits.max_input_size
    }

    fn blocks(&mut self, blocks: &[Block], prefix: &str) -> Result<String> {
        let limit = self.limit();
        join_paragraphs(blocks.iter().map(|block| self.block(block, prefix)), limit)
    }

    fn block(&mut self, block: &Block, prefix: &str) -> Result<String> {
        match block {
            Block::Plain(items) => self.paragraph(prefix, "", items, "", None),
            Block::Para(items) => {
                self.paragraph(&combine(prefix, "Paragraph"), "", items, "", None)
            }
            Block::Header(level, attr, items) => self.paragraph(
                &combine(prefix, &heading_style(*level, attr)),
                &attr.id,
                items,
                "",
                None,
            ),
            Block::CodeBlock(_, code) => {
                let payload = if code.ends_with('\n') {
                    format!("{code}    ")
                } else {
                    code.clone()
                };
                self.paragraph(&combine(prefix, "CodeBlock"), "", &[], &payload, None)
            }
            Block::LineBlock(lines) => {
                let mut items = Vec::new();
                for line in lines {
                    if !items.is_empty() {
                        items.push(Inline::LineBreak);
                    }
                    items.extend_from_slice(line);
                }
                self.paragraph(&combine(prefix, "Paragraph"), "", &items, "", None)
            }
            Block::BlockQuote(blocks) => self.blocks(blocks, &combine(prefix, "Blockquote")),
            Block::Div(attr, blocks)
                if attr.classes.first().is_some_and(|class| class == "section") =>
            {
                if let Some((Block::Header(level, _, items), rest)) = blocks.split_first() {
                    let heading = self.paragraph(
                        &combine(prefix, &heading_style(*level, attr)),
                        &attr.id,
                        items,
                        "",
                        None,
                    );
                    let rest = self.blocks(rest, prefix);
                    return join_paragraphs([heading, rest], self.limit());
                }
                self.blocks(blocks, prefix)
            }
            Block::Div(_, blocks) => self.blocks(blocks, prefix),
            Block::BulletList(items) => self.list(prefix, None, items),
            Block::OrderedList(attr, items) => self.list(prefix, Some(attr), items),
            Block::DefinitionList(items) => {
                let limit = self.limit();
                join_paragraphs(
                    items.iter().map(|(term, defs)| {
                        let title =
                            self.paragraph(&combine(prefix, "DefListTerm"), "", term, "", None)?;
                        let defs = join_paragraphs(
                            defs.iter()
                                .map(|def| self.blocks(def, &combine(prefix, "DefListDef"))),
                            limit,
                        )?;
                        join_paragraphs([Ok(title), Ok(defs)], limit)
                    }),
                    limit,
                )
            }
            Block::Figure(_, caption, blocks) => {
                let body = self.blocks_as(blocks, &combine(prefix, "Figure"))?;
                let caption = self.blocks_as(&caption.long, &combine(prefix, "Caption"))?;
                join_paragraphs([Ok(body), Ok(caption)], self.limit())
            }
            Block::Table(table) => self.table(table, prefix),
            Block::RawBlock(format, raw) if format == "icml" => {
                check_output(raw.len(), self.limit())?;
                Ok(raw.clone())
            }
            Block::RawBlock(..) | Block::HorizontalRule => Ok(String::new()),
        }
    }

    fn blocks_as(&mut self, blocks: &[Block], style: &str) -> Result<String> {
        let limit = self.limit();
        join_paragraphs(
            blocks.iter().map(|block| match block {
                Block::Plain(items) | Block::Para(items) => {
                    self.paragraph(style, "", items, "", None)
                }
                _ => self.block(block, style),
            }),
            limit,
        )
    }

    fn list(
        &mut self,
        prefix: &str,
        attrs: Option<&ListAttributes>,
        items: &[Vec<Block>],
    ) -> Result<String> {
        let base = combine(
            prefix,
            if attrs.is_some() {
                "NumList"
            } else {
                "BulList"
            },
        );
        let start = attrs.map_or(1, |attr| attr.start);
        let number_style = match attrs.map(|attr| attr.style) {
            Some(ListNumberStyle::LowerRoman) => "lowerRoman",
            Some(ListNumberStyle::UpperRoman) => "upperRoman",
            Some(ListNumberStyle::LowerAlpha) => "lowerAlpha",
            Some(ListNumberStyle::UpperAlpha) => "upperAlpha",
            _ => "",
        };
        let limit = self.limit();
        join_paragraphs(
            items.iter().enumerate().map(|(index, blocks)| {
                let mut label = if index == 0 {
                    combine(&base, "first")
                } else {
                    base.clone()
                };
                if start != 1 {
                    label = combine(&label, &format!("beginsWith-{start}"));
                }
                label = combine(&label, number_style);
                let mut rendered = String::new();
                let mut first = true;
                for block in blocks {
                    let text = match block {
                        Block::Plain(items) | Block::Para(items) if first => {
                            let style = if matches!(block, Block::Para(_)) {
                                combine(&label, "Paragraph")
                            } else {
                                label.clone()
                            };
                            let text = self.paragraph(
                                &style,
                                "",
                                items,
                                "",
                                (index == 0).then_some(start),
                            )?;
                            first = false;
                            text
                        }
                        Block::Plain(_) | Block::Para(_) => {
                            self.block(block, &combine(&base, "subParagraph"))?
                        }
                        _ => self.block(block, &base)?,
                    };
                    append_paragraph(&mut rendered, &text, limit)?;
                }
                Ok(rendered)
            }),
            limit,
        )
    }

    fn paragraph(
        &mut self,
        style: &str,
        id: &str,
        items: &[Inline],
        literal_text: &str,
        start: Option<i64>,
    ) -> Result<String> {
        let limit = self.limit();
        check_output(style.len().saturating_add(id.len()), limit)?;
        self.paragraphs.insert(style.into());
        let mut runs = Runs::new(limit);
        if (style.contains("subParagraph")
            || style.contains("Footnote") && style.ends_with("Paragraph"))
            && !items.is_empty()
        {
            runs.push(Run::Text(Style::default(), "\t".into()))?;
        }
        if !literal_text.is_empty() {
            check_output(literal_text.len(), limit)?;
            runs.push(Run::Text(Style::default(), xml(literal_text)))?;
        }
        self.collect(items, Style::default(), &mut runs, id)?;
        let style = xml(style);
        let mut attrs = if style.is_empty() {
            String::from("AppliedParagraphStyle=\"\"")
        } else {
            format!("AppliedParagraphStyle=\"ParagraphStyle/{style}\"")
        };
        if let Some(start) = start {
            attrs.push_str(" NumberingContinue=\"false\"");
            if start != 1 {
                attrs = format!("NumberingStartAt=\"{start}\" {attrs}");
            }
        }
        let body = self.runs(runs, id)?;
        let output = if body.is_empty() {
            format!("<ParagraphStyleRange {attrs}>\n</ParagraphStyleRange>")
        } else {
            format!("<ParagraphStyleRange {attrs}>\n{body}\n</ParagraphStyleRange>")
        };
        check_output(output.len(), limit)?;
        Ok(output)
    }

    fn collect(
        &mut self,
        items: &[Inline],
        style: Style,
        runs: &mut Runs,
        anchor: &str,
    ) -> Result<()> {
        for item in items {
            match item {
                Inline::Str(text) => {
                    check_output(text.len(), runs.remaining)?;
                    runs.text(style, &xml(text))?;
                }
                Inline::Space | Inline::SoftBreak => runs.text(style, " ")?,
                Inline::LineBreak => runs.push(Run::Text(style, "&#x2028;".into()))?,
                Inline::Strong(items) => self.collect(items, style.with(0), runs, anchor)?,
                Inline::Emph(items) => self.collect(items, style.with(2), runs, anchor)?,
                Inline::SmallCaps(items) => self.collect(items, style.with(4), runs, anchor)?,
                Inline::Strikeout(items) => self.collect(items, style.with(5), runs, anchor)?,
                Inline::Subscript(items) => self.collect(items, style.with(6), runs, anchor)?,
                Inline::Superscript(items) => self.collect(items, style.with(7), runs, anchor)?,
                Inline::Underline(items) => self.collect(items, style.with(8), runs, anchor)?,
                Inline::Code(_, text) => {
                    check_output(text.len(), runs.remaining)?;
                    runs.text(style.with(1), &xml(text))?;
                }
                Inline::Cite(_, items) => self.collect(items, style, runs, anchor)?,
                Inline::Span(attr, items) => {
                    if !attr.id.is_empty() {
                        check_output(attr.id.len(), runs.remaining)?;
                        runs.push(Run::Destination(attr.id.clone()))?;
                        if let Some(image) = span_image(items)
                            && items.iter().all(|item| {
                                matches!(
                                    item,
                                    Inline::Image(..) | Inline::Space | Inline::SoftBreak
                                )
                            })
                        {
                            runs.push(Run::Text(style, " ".into()))?;
                            self.collect(std::slice::from_ref(image), style, runs, anchor)?;
                            runs.push(Run::Destination(attr.id.clone()))?;
                            runs.push(Run::Text(style, " ".into()))?;
                        } else {
                            self.collect(items, style, runs, anchor)?;
                        }
                        runs.push(Run::Boundary)?;
                    } else {
                        self.collect(items, style, runs, anchor)?;
                    }
                }
                Inline::Quoted(kind, items) => {
                    let (left, right) = if *kind == QuoteType::SingleQuote {
                        ("‘", "’")
                    } else {
                        ("“", "”")
                    };
                    runs.push(Run::Boundary)?;
                    runs.text(style, left)?;
                    self.collect(items, style, runs, anchor)?;
                    runs.text(style, right)?;
                    runs.push(Run::Boundary)?;
                }
                Inline::Math(kind, tex) => self.collect_math(*kind, tex, style, runs, anchor)?,
                Inline::Link(_, label, target) => {
                    self.collect_link(label, target, style, runs, anchor)?
                }
                Inline::Note(blocks) => self.collect_note(blocks, runs)?,
                Inline::Image(attr, _, target) => {
                    check_output(target.url.len(), runs.remaining)?;
                    check_output(attr.get("object-style").map_or(0, str::len), runs.remaining)?;
                    runs.push(Run::Markup(self.image(attr, &target.url)))?;
                }
                Inline::RawInline(format, raw) if format == "icml" => {
                    check_output(raw.len(), runs.remaining)?;
                    runs.push(Run::Markup(raw.clone()))?;
                }
                Inline::RawInline(..) => {}
            }
        }
        Ok(())
    }

    fn collect_math(
        &mut self,
        kind: randoc_ast::MathType,
        tex: &str,
        style: Style,
        runs: &mut Runs,
        anchor: &str,
    ) -> Result<()> {
        check_output(tex.len(), self.limit())?;
        match randoc_math::to_inlines(kind, tex, self.context.limits.max_nesting_depth) {
            Ok(items) => {
                let mut math = Runs::new(runs.remaining);
                for item in &items {
                    if let Inline::Str(text) = item {
                        math_text_runs(text, style, &mut math)?;
                    } else {
                        self.collect(std::slice::from_ref(item), style, &mut math, anchor)?;
                    }
                    math.push(Run::Boundary)?;
                }
                let combined = self
                    .runs(math, anchor)?
                    .replace("\n  <CharacterStyleRange", "<CharacterStyleRange");
                runs.push(Run::Markup(combined))?;
            }
            Err(error) => {
                self.context.report(LogMessage::CouldNotConvertTexMath(
                    tex.to_owned(),
                    error.detail(tex),
                ));
                let delimiter = if kind == randoc_ast::MathType::DisplayMath {
                    "$$"
                } else {
                    "$"
                };
                runs.push(Run::Text(
                    style,
                    xml(&format!("{delimiter}{tex}{delimiter}")),
                ))?;
                runs.push(Run::Boundary)?;
            }
        }
        Ok(())
    }

    fn collect_link(
        &mut self,
        label: &[Inline],
        target: &randoc_ast::Target,
        style: Style,
        runs: &mut Runs,
        anchor: &str,
    ) -> Result<()> {
        check_output(
            target.url.len().saturating_add(target.title.len()),
            self.limit(),
        )?;
        if self.options.template.is_some() {
            charge_output(&mut self.link_budget, target.url.len().saturating_add(1))?;
        }
        self.links.push(if self.options.template.is_some() {
            target.url.clone()
        } else {
            String::new()
        });
        let index = self.links.len();
        let mut linked = Runs::new(runs.remaining);
        self.collect(label, style.with(3), &mut linked, anchor)?;
        let linked = self.runs(linked, anchor)?;
        let title = xml(&target.title);
        runs.push(Run::Markup(format!("  <HyperlinkTextSource Self=\"htss-{index}\" Name=\"{title}\" Hidden=\"false\">\n{}\n  </HyperlinkTextSource>", indent(&linked, 1, runs.remaining)?)))?;
        Ok(())
    }

    fn collect_note(&mut self, blocks: &[Block], runs: &mut Runs) -> Result<()> {
        let note = self.blocks(blocks, "Footnote")?;
        runs.push(Run::Markup(format!("  <CharacterStyleRange AppliedCharacterStyle=\"{NORMAL}\" Position=\"Superscript\">\n    <Footnote>\n      <ParagraphStyleRange>\n        <CharacterStyleRange>\n          <Content><?ACE 4?></Content>\n        </CharacterStyleRange>\n      </ParagraphStyleRange>\n{}\n    </Footnote>\n  </CharacterStyleRange>", indent(&note, 3, runs.remaining)?)))?;
        Ok(())
    }

    fn runs(&mut self, runs: Runs, anchor: &str) -> Result<String> {
        let anchor = if anchor.is_empty() {
            String::new()
        } else {
            destination(anchor)
        };
        let mut output = String::new();
        let mut first = true;
        let mut destination = String::new();
        for run in runs.items {
            let text = match run {
                Run::Text(style, text) => {
                    let name = if style == Style::default() {
                        NORMAL.into()
                    } else {
                        let name = style.name();
                        self.characters.insert(name.clone());
                        format!("CharacterStyle/{name}")
                    };
                    let rendered = format!(
                        "  <CharacterStyleRange AppliedCharacterStyle=\"{name}\">\n{anchor}{destination}    <Content>{text}</Content>\n  </CharacterStyleRange>"
                    );
                    destination.clear();
                    rendered
                }
                Run::Markup(text) => text,
                Run::Boundary => continue,
                Run::Destination(id) => {
                    append_output(&mut destination, &self::destination(&id), self.limit())?;
                    continue;
                }
            };
            if !first {
                append_output(&mut output, "\n", self.limit())?;
            }
            append_output(&mut output, &text, self.limit())?;
            first = false;
        }
        if !destination.is_empty() {
            if !first {
                append_output(&mut output, "\n", self.limit())?;
            }
            append_output(
                &mut output,
                &format!(
                    "  <CharacterStyleRange AppliedCharacterStyle=\"{NORMAL}\">\n{destination}  </CharacterStyleRange>"
                ),
                self.limit(),
            )?;
        }
        Ok(output)
    }

    fn hyperlinks(&self) -> Result<String> {
        join_output(self.links.iter().enumerate().rev().map(|(index, url)| {
            let i = index + 1;
            let name = xml(url);
            let (declaration, target) = if url.starts_with('#') {
                (String::new(), format!("HyperlinkTextDestination/{name}"))
            } else {
                let target = format!("HyperlinkURLDestination/{}", name.replace(':', "%3a"));
                (format!("<HyperlinkURLDestination Self=\"{target}\" Name=\"link\" DestinationURL=\"{name}\" DestinationUniqueKey=\"1\" />\n"), target)
            };
            Ok(format!("{declaration}<Hyperlink Self=\"uf-{i}\" Name=\"{name}\" Source=\"htss-{i}\" Visible=\"false\" DestinationUniqueKey=\"1\">\n  <Properties>\n    <BorderColor type=\"enumeration\">Black</BorderColor>\n    <Destination type=\"object\">{target}</Destination>\n  </Properties>\n</Hyperlink>"))
        }), "\n", self.limit(), false)
    }

    fn table(&mut self, table: &Table, prefix: &str) -> Result<String> {
        let head = table.head.rows.len();
        let rows: Vec<_> = super::rows(table).collect();
        let columns = table.col_specs.len();
        let table_style = xml(table.attr.get("custom-style").unwrap_or("Table"));
        let mut output = format!(
            "<Table AppliedTableStyle=\"TableStyle/{table_style}\" HeaderRowCount=\"{head}\" BodyRowCount=\"{}\" ColumnCount=\"{columns}\">",
            rows.len().saturating_sub(head)
        );
        let limit = self.limit();
        check_output(output.len(), limit)?;
        for i in 0..columns {
            let column_width = match table.col_specs[i].width {
                randoc_ast::ColWidth::ColWidth(fraction) => format!(
                    " SingleColumnWidth=\"{}\"",
                    randoc_ast::json::show_double(fraction * 500.0)
                ),
                randoc_ast::ColWidth::ColWidthDefault => String::new(),
            };
            append_output(
                &mut output,
                &format!("\n  <Column Name=\"{i}\"{column_width} />"),
                limit,
            )?;
        }
        for (index, (head, row)) in rows.into_iter().enumerate() {
            let mut column = 0i64;
            for cell in &row.cells {
                let mut style = combine(prefix, "TablePar");
                if head {
                    style = combine(&style, "TableHeader");
                }
                let align = if cell.align == Alignment::AlignDefault {
                    usize::try_from(column)
                        .ok()
                        .and_then(|col| table.col_specs.get(col))
                        .map_or(Alignment::AlignDefault, |s| s.align)
                } else {
                    cell.align
                };
                style = combine(
                    &style,
                    match align {
                        Alignment::AlignRight => "RightAlign",
                        Alignment::AlignLeft => "LeftAlign",
                        Alignment::AlignCenter => "CenterAlign",
                        _ => "",
                    },
                );
                let text = self.blocks_as(&cell.content, &style)?;
                let text = indent(&text, 2, limit.saturating_sub(output.len() as u64))?;
                append_output(
                    &mut output,
                    &format!(
                        "\n  <Cell Name=\"{column}:{index}\" AppliedCellStyle=\"CellStyle/Cell\">\n{text}\n  </Cell>"
                    ),
                    limit,
                )?;
                column = column.saturating_add(cell.col_span.max(1));
            }
        }
        append_output(&mut output, "\n</Table>", limit)?;
        let caption = self.blocks_as(&table.caption.long, &combine(prefix, "TableCaption"))?;
        let caption = if caption.is_empty() {
            self.paragraph(&combine(prefix, "TableCaption"), "", &[], "", None)?
        } else {
            caption
        };
        append_output(&mut output, "\n", limit)?;
        append_output(&mut output, &caption, limit)?;
        Ok(output)
    }

    fn image(&mut self, attr: &Attr, url: &str) -> String {
        self.characters.insert(String::new());
        let object = attr.get("object-style").map_or_else(String::new, |style| {
            self.objects.insert(style.to_owned());
            format!(" AppliedObjectStyle=\"ObjectStyle/{}\"", xml(style))
        });
        let (width, height) = match super::resource::fetch(url, self.context) {
            Ok(bytes) => {
                let (size, mime) = image_size_and_mime(&bytes, Some(url));
                let size = size.map(|(w, h)| (f64::from(w), f64::from(h)));
                if mime == Some("image/svg+xml") {
                    svg_size(&bytes, &self.context.limits)
                        .or_else(|| size.map(|(w, h)| (w * 0.75, h * 0.75)))
                        .unwrap_or((300.0, 200.0))
                } else {
                    let (w, h) = size.unwrap_or((300.0, 200.0));
                    if mime == Some("image/jpeg") {
                        jpeg_point_size(&bytes, w, h)
                    } else {
                        (w, h)
                    }
                }
            }
            Err(error) => {
                let detail = if matches!(error, randoc_core::Error::ResourceNotFound(_)) {
                    format!(
                        "PandocResourceNotFound {}",
                        randoc_ast::json::show_string(url)
                    )
                } else {
                    error.to_string()
                };
                self.context
                    .report(LogMessage::CouldNotFetchResource(url.into(), detail));
                (300.0, 200.0)
            }
        };
        let dpi = self.options.dpi.max(1) as f64;
        let dims = |name: &str| {
            attr.get(name)
                .and_then(|text| {
                    if let Some(inches) = text.strip_suffix("in") {
                        inches.parse::<f64>().ok().map(|inches| inches * 72.0)
                    } else {
                        text.strip_suffix("px")
                            .unwrap_or(text)
                            .parse::<f64>()
                            .ok()
                            .map(|pixels| pixels * 72.0 / dpi)
                    }
                })
                .filter(|dim| dim.is_finite() && *dim > 0.0)
        };
        let w = width.max(1.0);
        let h = height.max(1.0);
        let x = w / 2.0;
        let y = h / 2.0;
        let sx = dims("width").map(|dim| dim / w);
        let sy = dims("height").map(|dim| dim / h);
        let scale_x = sx.or(sy).unwrap_or(1.0);
        let scale_y = sy.or(sx).unwrap_or(1.0);
        let decimals = short_number;
        let mut out = format!(
            "  <CharacterStyleRange AppliedCharacterStyle=\"{NORMAL}\">\n    <Rectangle Self=\"uec\" StrokeWeight=\"0\" ItemTransform=\"{} 0 0 {} {x} -{y}\"{object}>\n      <Properties>\n        <PathGeometry>\n          <GeometryPathType PathOpen=\"false\">\n            <PathPointArray>",
            decimals(scale_x),
            decimals(scale_y)
        );
        for (x, y) in [(-x, -y), (-x, y), (x, y), (x, -y)] {
            let _ = write!(
                out,
                "\n              <PathPointType Anchor=\"{x} {y}\" LeftDirection=\"{x} {y}\" RightDirection=\"{x} {y}\" />"
            );
        }
        let (contents, link) = if let Some((_, encoded)) = url
            .strip_prefix("data:")
            .and_then(|data| data.split_once(";base64,"))
        {
            let encoded = encoded.replace("]]>", "]]]]><![CDATA[>");
            (
                format!(
                    "\n          <Contents>\n            <![CDATA[{encoded}]]>\n          </Contents>"
                ),
                String::new(),
            )
        } else {
            let uri = if url.contains("://") {
                xml(url)
            } else {
                format!("file:{}", xml(url))
            };
            (
                String::new(),
                format!("\n        <Link Self=\"ueb\" LinkResourceURI=\"{uri}\" />"),
            )
        };
        let _ = write!(
            out,
            "\n            </PathPointArray>\n          </GeometryPathType>\n        </PathGeometry>\n      </Properties>\n      <Image Self=\"ue6\" ItemTransform=\"{} 0 0 {} -{x} -{y}\">\n        <Properties>\n          <Profile type=\"string\">\n            $ID/Embedded\n          </Profile>\n          <GraphicBounds Left=\"0\" Top=\"0\" Right=\"{}\" Bottom=\"{}\" />{contents}\n        </Properties>{link}\n      </Image>\n    </Rectangle>\n  </CharacterStyleRange>",
            decimals(scale_x),
            decimals(scale_y),
            decimals(w / scale_x),
            decimals(h / scale_y)
        );
        out
    }
}

enum Run {
    Text(Style, String),
    Destination(String),
    Markup(String),
    Boundary,
}

fn destination(id: &str) -> String {
    let id = xml(id);
    format!(
        "    <HyperlinkTextDestination Self=\"HyperlinkTextDestination/#{id}\" Name=\"Destination\" DestinationUniqueKey=\"1\" />\n"
    )
}

fn span_image(items: &[Inline]) -> Option<&Inline> {
    let mut content = items
        .iter()
        .filter(|item| !matches!(item, Inline::Space | Inline::SoftBreak));
    match (content.next(), content.next()) {
        (Some(image @ Inline::Image(..)), None) => Some(image),
        _ => None,
    }
}

struct Runs {
    items: Vec<Run>,
    remaining: u64,
}

impl Runs {
    fn new(limit: u64) -> Self {
        Self {
            items: Vec::new(),
            remaining: limit,
        }
    }

    fn push(&mut self, run: Run) -> Result<()> {
        let bytes = match &run {
            Run::Text(_, text) => {
                let tags = "  <CharacterStyleRange AppliedCharacterStyle=\"\">\n    <Content></Content>\n  </CharacterStyleRange>";
                tags.len().saturating_add(text.len())
            }
            Run::Destination(id) | Run::Markup(id) => id.len(),
            Run::Boundary if matches!(self.items.last(), None | Some(Run::Boundary)) => {
                return Ok(());
            }
            Run::Boundary => 0,
        };
        charge_output(&mut self.remaining, bytes)?;
        self.items.push(run);
        Ok(())
    }

    fn text(&mut self, style: Style, text: &str) -> Result<()> {
        if let Some(Run::Text(previous, value)) = self.items.last_mut()
            && *previous == style
            && value != "&#x2028;"
        {
            charge_output(&mut self.remaining, text.len())?;
            value.push_str(text);
        } else if !text.is_empty() {
            self.push(Run::Text(style, text.into()))?;
        }
        Ok(())
    }
}

fn math_text_runs(text: &str, style: Style, runs: &mut Runs) -> Result<()> {
    let math_space = |c| matches!(c, '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}');
    for part in text.split_inclusive(math_space) {
        if let Some((index, space)) = part
            .char_indices()
            .next_back()
            .filter(|&(_, c)| math_space(c))
        {
            if let Some(word) = part.get(..index).filter(|word| !word.is_empty()) {
                runs.push(Run::Text(style, xml(word)))?;
            }
            runs.push(Run::Text(style, space.to_string()))?;
        } else {
            runs.push(Run::Text(style, xml(part)))?;
        }
    }
    Ok(())
}

fn join_paragraphs(parts: impl IntoIterator<Item = Result<String>>, limit: u64) -> Result<String> {
    join_output(parts, PARAGRAPH_BREAK, limit, true)
}

fn append_paragraph(output: &mut String, text: &str, limit: u64) -> Result<()> {
    if !text.is_empty() {
        if !output.is_empty() {
            append_output(output, PARAGRAPH_BREAK, limit)?;
        }
        append_output(output, text, limit)?;
    }
    Ok(())
}

fn jpeg_point_size(bytes: &[u8], width: f64, height: f64) -> (f64, f64) {
    let Some(header) = bytes.get(2..18) else {
        return (width, height);
    };
    if !header.starts_with(&[0xff, 0xe0]) || header.get(4..9) != Some(b"JFIF\0") {
        return (width, height);
    }
    let factor = match header[11] {
        1 => 1.0,
        2 => 2.54,
        _ => return (width, height),
    };
    let x = u16::from_be_bytes([header[12], header[13]]);
    let y = u16::from_be_bytes([header[14], header[15]]);
    if x == 0 || y == 0 {
        (width, height)
    } else {
        (
            width * 72.0 / (f64::from(x) * factor),
            height * 72.0 / (f64::from(y) * factor),
        )
    }
}

fn svg_size(bytes: &[u8], limits: &randoc_core::Limits) -> Option<(f64, f64)> {
    use randoc_archive::xml::{XmlLimits, XmlOptions, parse};
    let text = std::str::from_utf8(bytes).ok()?;
    let limits = XmlLimits::from_limits(limits);
    let root = parse(text, &limits, XmlOptions::default()).ok()?.root;
    let pixels = |value: &str| {
        let (number, factor) = if let Some(value) = value.strip_suffix("pt") {
            (value, 1.0)
        } else if let Some(value) = value.strip_suffix("in") {
            (value, 72.0)
        } else if let Some(value) = value.strip_suffix("mm") {
            (value, 72.0 / 25.4)
        } else {
            (value.strip_suffix("px").unwrap_or(value), 0.75)
        };
        number
            .parse::<f64>()
            .ok()
            .map(|n| (n * factor / 0.75).floor() * 0.75)
            .filter(|n| n.is_finite() && *n > 0.0)
    };
    let view_box = root.attr_unqualified("viewBox").and_then(|value| {
        let mut fields = value
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .skip(2);
        Some((pixels(fields.next()?)?, pixels(fields.next()?)?))
    });
    let width = root.attr_unqualified("width").and_then(pixels);
    let height = root.attr_unqualified("height").and_then(pixels);
    match (width, height) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => view_box,
    }
}

fn short_number(value: f64) -> String {
    format!("{value:.5}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .into()
}

fn character_style(name: &str) -> String {
    let escaped = xml(name);
    let has = |style| name.split_whitespace().any(|word| word == style);
    let mut props = String::new();
    let font = match (has("Bold"), has("Italic")) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        _ => "",
    };
    if !font.is_empty() {
        let _ = write!(props, " FontStyle=\"{font}\"");
    }
    for (style, value) in [
        ("Strikeout", "StrikeThru=\"true\""),
        ("Underline", "Underline=\"true\""),
        ("SmallCaps", "Capitalization=\"SmallCaps\""),
        ("Subscript", "Position=\"Subscript\""),
        ("Superscript", "Position=\"Superscript\""),
    ] {
        if has(style) {
            let _ = write!(props, " {value}");
        }
    }
    let font = if has("Code") {
        "\n    <AppliedFont type=\"string\">Courier New</AppliedFont>"
    } else {
        ""
    };
    format!(
        "<CharacterStyle Self=\"CharacterStyle/{escaped}\" Name=\"{escaped}\"{props}>\n  <Properties>\n    <BasedOn type=\"object\">{NORMAL}</BasedOn>{font}\n  </Properties>\n</CharacterStyle>"
    )
}

fn paragraph_style(style: &str) -> String {
    let name = xml(style);
    let mut props = String::new();
    let list_types = style
        .split(" > ")
        .filter(|part| matches!(*part, "NumList" | "BulList"));
    let list_level = list_types.clone().count();
    let list_type = list_types.last();
    let continuation = style.contains("subParagraph");
    if list_type == Some("BulList") && !continuation {
        props.push_str(" BulletsAndNumberingListType=\"BulletList\"");
    }
    if list_type == Some("NumList") {
        let _ = write!(
            props,
            " NumberingExpression=\"^#.^t\" NumberingLevel=\"{list_level}\""
        );
        if !continuation {
            props.push_str(" BulletsAndNumberingListType=\"NumberedList\"");
        }
    }
    let quotes = style
        .split(" > ")
        .filter(|part| *part == "Blockquote")
        .count();
    let definition = usize::from(style.contains("DefListDef"));
    let indentation = if list_level > 0 {
        20 * (quotes + definition) + 10 * (list_level - 1)
    } else if quotes > 0 && definition == 0 {
        20 * quotes - 10
    } else {
        20 * quotes + 10 * definition
    };
    let _ = write!(props, " LeftIndent=\"{indentation}\"");
    if style.contains("DefListTerm") {
        props.push_str(" BulletsAndNumberingListType=\"BulletList\" FontStyle=\"Bold\"");
    }
    if let Some(level) = style.split(" > ").find_map(|part| {
        part.strip_prefix("Header")
            .and_then(|i| i.parse::<usize>().ok())
    }) {
        let point_size = [36, 30, 24, 18, 14, 12]
            .get(level.saturating_sub(1))
            .copied()
            .unwrap_or(12);
        let _ = write!(props, " PointSize=\"{point_size}\"");
    }
    if style.contains("RightAlign") {
        props.push_str(" Justification=\"RightAlign\"");
    }
    if style.contains("CenterAlign") {
        props.push_str(" Justification=\"CenterAlign\"");
    }
    let font = if style.contains("CodeBlock") {
        "\n    <AppliedFont type=\"string\">Courier New</AppliedFont>"
    } else {
        ""
    };
    let tabs = if list_type == Some("BulList") && !continuation {
        let tab = indentation + 10;
        format!(
            "\n    <TabList type=\"list\">\n      <ListItem type=\"record\">\n        <Alignment type=\"enumeration\">LeftAlign</Alignment>\n        <AlignmentCharacter type=\"string\">.</AlignmentCharacter>\n        <Leader type=\"string\" />\n        <Position type=\"unit\">{tab}</Position>\n      </ListItem>\n    </TabList>"
        )
    } else {
        String::new()
    };
    let numbers = [
        ("lowerRoman", "i, ii, iii, iv..."),
        ("upperRoman", "I, II, III, IV..."),
        ("lowerAlpha", "a, b, c, d..."),
        ("upperAlpha", "A, B, C, D..."),
    ]
    .into_iter()
    .find(|(name, _)| style.split(" > ").any(|part| part == *name))
    .map_or_else(String::new, |(_, format)| {
        format!("\n    <NumberingFormat type=\"string\">{format}</NumberingFormat>")
    });
    format!(
        "<ParagraphStyle Self=\"ParagraphStyle/{name}\" Name=\"{name}\"{props}>\n  <Properties>\n    <BasedOn type=\"object\">{BASE}</BasedOn>{font}{tabs}{numbers}\n  </Properties>\n</ParagraphStyle>"
    )
}
