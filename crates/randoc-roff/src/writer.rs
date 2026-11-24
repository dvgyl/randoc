mod highlight;
mod inline;
mod table;
mod uri;

use randoc_ast::{
    Attr, Block, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, MetaValue, Pandoc,
};
use randoc_core::shared::{stringify, stringify_meta};
use randoc_core::{Context, Error, Result, WrapOption, WriterOptions};
use randoc_layout::{Doc, literal};
use randoc_templates::{Context as Vars, Val};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Format {
    Man,
    Ms,
}

impl Format {
    fn name(self) -> &'static str {
        match self {
            Self::Man => "man",
            Self::Ms => "ms",
        }
    }
}

pub(crate) fn write(
    format: Format,
    doc: &Pandoc,
    options: &WriterOptions,
    ctx: &mut Context,
) -> Result<String> {
    let pdf = format == Format::Ms
        && options.variables.get("pdf-engine").is_some_and(
            |value| matches!(value, Val::Simple(text) if text.render(None) == "pdfroff"),
        );
    let mut renderer = Renderer {
        format,
        options,
        pdf,
        notes: Vec::new(),
        table: false,
        highlighting: false,
        small_caps: false,
        csl_entry: false,
        syntax: None,
        inline_math: false,
        first_para: true,
        cells: ctx.limits.max_table_cells,
        limit: ctx.limits.max_nesting_depth,
        ctx,
    };
    let mut out = String::new();
    renderer.blocks(&doc.blocks, &mut out, Lead::First, 0)?;
    if !renderer.notes.is_empty() {
        line(&mut out, ".SH NOTES");
        let mut index = 0;
        while let Some(note) = renderer.notes.get(index).copied() {
            index += 1;
            line(&mut out, &format!(".SS [{index}]"));
            renderer.blocks(note, &mut out, Lead::First, 1)?;
        }
    }
    if let Some(template) = &options.template {
        let vars = renderer.variables(doc, out.trim_end_matches('\n'))?;
        let max = usize::try_from(renderer.ctx.limits.max_input_size).unwrap_or(usize::MAX);
        template
            .render_limited(&vars, max.saturating_mul(4), max)
            .ok_or_else(|| Error::App("The roff template exceeds the output limit.".into()))
    } else {
        Ok(out.trim_end_matches('\n').into())
    }
}

struct Renderer<'a, 'doc> {
    format: Format,
    options: &'a WriterOptions,
    ctx: &'a mut Context,
    pdf: bool,
    notes: Vec<&'doc [Block]>,
    table: bool,
    highlighting: bool,
    small_caps: bool,
    csl_entry: bool,
    syntax: Option<randoc_highlight::Registry>,
    inline_math: bool,
    first_para: bool,
    cells: usize,
    limit: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Lead {
    First,
    After,
    Bare,
    CslEntry,
    Normal,
}

impl<'a, 'doc> Renderer<'a, 'doc> {
    fn check_depth(&self, level: usize) -> Result<()> {
        if level > self.limit {
            return Err(Error::App(
                "The roff nesting depth exceeds the writer limit.".into(),
            ));
        }
        Ok(())
    }

    fn blocks(
        &mut self,
        blocks: &'doc [Block],
        out: &mut String,
        lead: Lead,
        level: usize,
    ) -> Result<Lead> {
        self.check_depth(level)?;
        let mut lead = lead;
        for block in blocks {
            let next = self.block(block, out, lead, level)?;
            if lead != Lead::CslEntry {
                lead = next;
            }
        }
        Ok(lead)
    }

    fn block(
        &mut self,
        block: &'doc Block,
        out: &mut String,
        lead: Lead,
        level: usize,
    ) -> Result<Lead> {
        match block {
            Block::Para(text) | Block::Plain(text) => {
                if lead == Lead::CslEntry && matches!(block, Block::Para(_)) {
                    let text = if let [
                        Inline::Span(label_attr, label),
                        Inline::Span(body_attr, _),
                        ..,
                    ] = text.as_slice()
                        && label_attr.has_class("csl-left-margin")
                        && body_attr.has_class("csl-right-inline")
                    {
                        let trailing_space =
                            matches!(label.last(), Some(Inline::Space | Inline::SoftBreak));
                        let label =
                            self.render_inlines(label, inline::Font::ROMAN, level, false)?;
                        let separator = if trailing_space { " " } else { "" };
                        line(out, &format!(".IP \"{label}{separator}\" 5"));
                        self.paragraph(&text[1..], level)?
                    } else {
                        line(out, ".CSLP");
                        self.paragraph(text, level)?
                    };
                    line(out, &text);
                    return Ok(Lead::CslEntry);
                }
                if matches!(block, Block::Para(_)) && lead != Lead::Bare {
                    match (self.format, lead) {
                        (Format::Man, Lead::After) => {}
                        (Format::Ms, _) if self.first_para => line(out, ".LP"),
                        _ => line(out, ".PP"),
                    }
                    self.first_para = false;
                }
                let text = self.paragraph(text, level)?;
                line(out, &text);
            }
            Block::Header(n, attr, text) => {
                let font = if self.format == Format::Ms {
                    inline::Font::BOLD
                } else {
                    inline::Font::ROMAN
                };
                let text_layout = self.render_inlines(text, font, level, false)?;
                if self.format == Format::Man {
                    let request = if *n == 1 { ".SH" } else { ".SS" };
                    line(out, &format!("{request} {text_layout}"));
                } else {
                    let request = if self.options.number_sections
                        && !attr.classes.iter().any(|c| c == "unnumbered")
                    {
                        ".NH"
                    } else {
                        ".SH"
                    };
                    line(out, &format!("{request} {n}"));
                    line(out, &text_layout);
                    if self.pdf {
                        line(
                            out,
                            &format!(".pdfhref O {n} \"{}\"", self.escape(&stringify(text))),
                        );
                        line(
                            out,
                            &format!(
                                ".pdfhref M \"{}{}\"",
                                self.options.identifier_prefix,
                                self.escape(&attr.id)
                            ),
                        );
                    }
                }
                self.first_para = true;
                return Ok(Lead::After);
            }
            Block::CodeBlock(attr, code) => {
                self.code_block(attr, code, out);
                self.first_para = true;
            }
            Block::RawBlock(format, text) => {
                if format == self.format.name() {
                    line(out, text);
                } else {
                    return Ok(lead);
                }
            }
            Block::BlockQuote(blocks) => {
                let (start, end) = if self.format == Format::Man {
                    (".RS", ".RE")
                } else {
                    (".QS", ".QE")
                };
                line(out, start);
                self.first_para = true;
                self.blocks(blocks, out, Lead::First, level + 1)?;
                line(out, end);
                self.first_para = true;
            }
            Block::BulletList(items) => {
                let indent = if self.format == Format::Man { 2 } else { 3 };
                for item in items {
                    self.list_item(item, "\\(bu", indent, out, level + 1)?;
                }
                self.first_para = true;
            }
            Block::OrderedList(attributes, items) => {
                let markers = (0..items.len())
                    .map(|index| marker(*attributes, index))
                    .collect::<Vec<_>>();
                let width = markers.iter().map(String::len).max().unwrap_or(0);
                for (marker, item) in markers.iter().zip(items) {
                    let extra = if self.format == Format::Man { 1 } else { 2 };
                    let mut tag = format!("{marker:>width$}");
                    if self.format == Format::Ms {
                        tag.insert(0, ' ');
                    }
                    self.list_item(item, &format!("\"{tag}\""), width + extra, out, level + 1)?;
                }
                self.first_para = true;
            }
            Block::DefinitionList(items) => {
                for (term, definitions) in items {
                    let ms = self.format == Format::Ms;
                    let font = if ms {
                        inline::Font::BOLD
                    } else {
                        inline::Font::ROMAN
                    };
                    let term = self.render_inlines(term, font, level + 1, false)?;
                    if ms {
                        line(out, &format!(".IP \"\\f[B]{term}\\f[R]\" 3"));
                        for definition in definitions {
                            if let Some((first, rest)) = definition.split_first() {
                                self.block(first, out, Lead::Bare, level + 1)?;
                                line(out, ".RS 3");
                                self.blocks(rest, out, Lead::Normal, level + 1)?;
                                line(out, ".RE");
                            }
                        }
                    } else {
                        line(out, ".TP");
                        line(out, &term);
                        let mut blocks = definitions.iter().flatten();
                        if let Some(first) = blocks.next() {
                            self.block(first, out, Lead::Bare, level + 1)?;
                        }
                        let mut rest = blocks.peekable();
                        if rest.peek().is_some() {
                            line(out, ".RS");
                            let mut lead = Lead::Normal;
                            for block in rest {
                                lead = self.block(block, out, lead, level + 1)?;
                            }
                            line(out, ".RE");
                        }
                    }
                }
                self.first_para = true;
            }
            Block::HorizontalRule => {
                if self.format == Format::Man {
                    line(out, ".PP\n   *   *   *   *   *");
                } else {
                    line(out, ".HLINE");
                }
            }
            Block::LineBlock(lines) => {
                if lead != Lead::Bare {
                    match (self.format, lead) {
                        (Format::Man, Lead::After) => {}
                        (Format::Ms, _) => line(out, ".LP"),
                        _ => line(out, ".PP"),
                    }
                    self.first_para = false;
                }
                for (i, inlines) in lines.iter().enumerate() {
                    if i > 0 {
                        line(out, self.hard_break());
                    }
                    line(
                        out,
                        &self.render_inlines(inlines, inline::Font::ROMAN, level + 1, false)?,
                    );
                }
            }
            Block::Div(attr, blocks) => {
                if self.format == Format::Ms {
                    if attr.has_class("csl-bib-body") {
                        csl_macros(attr, out);
                        return self.blocks(blocks, out, lead, level + 1);
                    }
                    if attr.has_class("csl-entry") {
                        line(out, ".CSLENTRY");
                        let was_entry = std::mem::replace(&mut self.csl_entry, true);
                        let result = self.blocks(blocks, out, Lead::CslEntry, level + 1);
                        self.csl_entry = was_entry;
                        result?;
                        return Ok(lead);
                    }
                    self.first_para = true;
                    self.blocks(blocks, out, Lead::First, level + 1)?;
                    self.first_para = true;
                    return Ok(Lead::Normal);
                }
                return self.blocks(blocks, out, lead, level + 1);
            }
            Block::Figure(_, caption, blocks) => {
                if let [Block::Plain(inlines) | Block::Para(inlines)] = blocks.as_slice()
                    && let [Inline::Image(_, label, target)] = inlines.as_slice()
                {
                    if self.format == Format::Ms {
                        line(
                            out,
                            &format!("\\\" .IMAGE \"{}\"", self.escape(&target.url)),
                        );
                        line(out, ".ce 1");
                    } else {
                        let alt =
                            self.render_inlines(label, inline::Font::ROMAN, level + 1, false)?;
                        line(out, &format!("[IMAGE: {alt}]"));
                    }
                    self.blocks(&caption.long, out, Lead::Bare, level + 1)?;
                    if self.format == Format::Ms {
                        line(out, ".sp 1");
                        self.first_para = true;
                    }
                } else {
                    self.blocks(blocks, out, Lead::Bare, level + 1)?;
                    self.blocks(&caption.long, out, Lead::Bare, level + 1)?;
                }
            }
            Block::Table(table) => {
                self.write_table(table, out, level + 1)?;
                self.first_para = true;
            }
        }
        Ok(Lead::Normal)
    }

    fn list_item(
        &mut self,
        item: &'doc [Block],
        tag: &str,
        indent: usize,
        out: &mut String,
        level: usize,
    ) -> Result<()> {
        self.check_depth(level)?;
        line(out, &format!(".IP {tag} {indent}"));
        if let Some((first, rest)) = item.split_first() {
            self.block(first, out, Lead::Bare, level)?;
            if !rest.is_empty() {
                let continuation = if self.format == Format::Man && tag.starts_with('"') {
                    indent.max(4)
                } else {
                    indent
                };
                line(out, &format!(".RS {continuation}"));
                self.blocks(rest, out, Lead::Normal, level)?;
                line(out, ".RE");
            }
        }
        Ok(())
    }

    fn hard_break(&self) -> &'static str {
        if self.format == Format::Man {
            ".PD 0\n.P\n.PD"
        } else {
            ".br"
        }
    }

    fn width(&self) -> Option<usize> {
        (self.options.wrap_text == WrapOption::WrapAuto)
            .then(|| usize::try_from(self.options.columns).unwrap_or(0))
    }

    fn variables(&mut self, doc: &'doc Pandoc, body: &str) -> Result<Vars> {
        let mut vars = Vars::new();
        for (key, value) in &doc.meta.0 {
            vars.insert(key, self.meta(value, 0)?);
        }
        if self.format == Format::Man {
            self.man_title(doc, &mut vars);
            vars.insert("has-tables", Val::Bool(self.table));
        } else {
            let title = doc
                .meta
                .get("title")
                .map(stringify_meta)
                .unwrap_or_default();
            let author = doc
                .meta
                .get("author")
                .map_or_else(String::new, |val| match val {
                    MetaValue::MetaList(authors) => authors
                        .iter()
                        .map(stringify_meta)
                        .collect::<Vec<_>>()
                        .join("; "),
                    val => stringify_meta(val),
                });
            vars.insert("title-meta", Val::text(self.escape(&title)));
            vars.insert("author-meta", Val::text(self.escape(&author)));
            vars.insert("has-inline-math", Val::Bool(self.inline_math));
            vars.0.entry("hyphenate".into()).or_insert(Val::Bool(true));
            vars.insert("toc", Val::Bool(self.options.table_of_contents));
            if self.highlighting
                && let Some(macros) = self.highlighting_macros()
            {
                vars.insert("highlighting-macros", Val::text(macros));
            }
        }
        vars.0.extend(self.options.variables.0.clone());
        vars.insert("body", Val::Simple(literal(body)));
        Ok(vars)
    }

    fn meta(&mut self, value: &'doc MetaValue, level: usize) -> Result<Val> {
        self.check_depth(level)?;
        Ok(match value {
            MetaValue::MetaBool(value) => Val::Bool(*value),
            MetaValue::MetaString(value) => Val::text(self.escape(value)),
            MetaValue::MetaInlines(value) => {
                Val::text(self.render_inlines(value, inline::Font::ROMAN, level, false)?)
            }
            MetaValue::MetaBlocks(value) => {
                let mut text = String::new();
                self.blocks(value, &mut text, Lead::Bare, level + 1)?;
                Val::text(text.trim_end_matches('\n'))
            }
            MetaValue::MetaList(value) => Val::List(
                value
                    .iter()
                    .map(|value| self.meta(value, level + 1))
                    .collect::<Result<_>>()?,
            ),
            MetaValue::MetaMap(value) => Val::Map(Vars(
                value
                    .iter()
                    .map(|(key, value)| {
                        self.meta(value, level + 1)
                            .map(|value| (key.clone(), value))
                    })
                    .collect::<Result<_>>()?,
            )),
        })
    }

    fn man_title(&self, doc: &Pandoc, vars: &mut Vars) {
        let Some(title) = doc.meta.get("title").map(stringify_meta) else {
            return;
        };
        if let Some((before, after)) = title.split_once('(')
            && !before.trim().is_empty()
            && !before.trim().contains(' ')
            && let Some((section, tail)) = after.split_once(')')
            && section.starts_with(|ch: char| ch.is_ascii_digit())
        {
            vars.insert("title", Val::text(self.escape(before.trim())));
            vars.0
                .entry("section".into())
                .or_insert_with(|| Val::text(section));
            let (footer, header) = tail.split_once('|').unwrap_or((tail, ""));
            vars.0
                .entry("footer".into())
                .or_insert_with(|| Val::text(self.escape(footer.trim())));
            vars.0
                .entry("header".into())
                .or_insert_with(|| Val::text(self.escape(header.trim())));
        }
    }
}

fn line(out: &mut String, text: &str) {
    if !text.is_empty() {
        out.push_str(text);
        if !text.ends_with('\n') {
            out.push('\n');
        }
    }
}

fn marker(
    ListAttributes {
        start,
        style,
        delim,
    }: ListAttributes,
    index: usize,
) -> String {
    use ListNumberStyle::{LowerAlpha, LowerRoman, UpperAlpha, UpperRoman};
    let number = start.saturating_add(i64::try_from(index).unwrap_or(i64::MAX));
    let mut text = match style {
        LowerAlpha | UpperAlpha if number > 0 => {
            let mut n = number;
            let mut letters = Vec::new();
            while n > 0 {
                n -= 1;
                letters.push(char::from(b'a' + (n % 26) as u8));
                n /= 26;
            }
            letters.into_iter().rev().collect::<String>()
        }
        LowerRoman | UpperRoman if (1..=3999).contains(&number) => {
            let mut n = number;
            let mut roman = String::new();
            for (value, digit) in [
                (1000, "m"),
                (900, "cm"),
                (500, "d"),
                (400, "cd"),
                (100, "c"),
                (90, "xc"),
                (50, "l"),
                (40, "xl"),
                (10, "x"),
                (9, "ix"),
                (5, "v"),
                (4, "iv"),
                (1, "i"),
            ] {
                while n >= value {
                    roman.push_str(digit);
                    n -= value;
                }
            }
            roman
        }
        _ => number.to_string(),
    };
    if matches!(style, UpperAlpha | UpperRoman) {
        text.make_ascii_uppercase();
    }
    match delim {
        ListNumberDelim::TwoParens => format!("({text})"),
        ListNumberDelim::OneParen => format!("{text})"),
        _ => format!("{text}."),
    }
}

fn protect(text: &str) -> String {
    if text.starts_with('.') {
        format!("\\&{text}")
    } else {
        text.into()
    }
}

fn render_parts(parts: Vec<Doc>, width: Option<usize>) -> String {
    randoc_layout::hcat(parts).render(width)
}

fn csl_macros(attr: &Attr, out: &mut String) {
    line(
        out,
        ".nr PI 3n
.de CSLENTRY",
    );
    if attr
        .get("entry-spacing")
        .and_then(|n| n.parse::<i64>().ok())
        .is_some_and(|n| n > 0)
    {
        line(out, ".sp");
    }
    line(
        out,
        "..
.de CSLP",
    );
    line(
        out,
        if attr.has_class("hanging-indent") {
            ".XP"
        } else {
            ".LP"
        },
    );
    line(out, "..");
}
