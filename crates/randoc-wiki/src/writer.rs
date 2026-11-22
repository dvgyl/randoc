use std::cell::RefCell;
use std::fmt::Write as _;

use randoc_ast::{Alignment, Attr, Block, Inline, MathType, Pandoc, QuoteType, Row, Table, Target};
use randoc_core::Extension;
use randoc_core::shared::{meta_context::meta_to_context, stringify};
use randoc_core::{Context, Error, LogMessage, Result, WrapOption, WriterOptions};
use randoc_layout::literal;
use randoc_templates::Val;

use crate::inline::url_start;
use crate::{Wiki, cell_error, depth_error};

pub(crate) fn write(
    wiki: Wiki,
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let mut writer = Writer {
        wiki,
        options,
        depth_limit: context.limits.max_nesting_depth,
        output_limit: context.limits.max_input_size.min(64 << 20),
        cells_left: context.limits.max_table_cells,
        notes: Vec::new(),
        media_notes: false,
        in_link: false,
        in_list: false,
        in_figure: false,
        messages: Vec::new(),
    };
    let mut fields = if options.template.is_some() {
        let limit = writer.depth_limit;
        let writer = RefCell::new(&mut writer);
        Some(meta_to_context(
            &doc.meta,
            &options.variables,
            limit,
            &mut |blocks| writer.borrow_mut().blocks(blocks, 0).map(literal),
            &mut |inlines| writer.borrow_mut().inlines(inlines, 0).map(literal),
        )?)
    } else {
        None
    };
    let mut body = writer.blocks(&doc.blocks, 0)?;
    for message in writer.messages.drain(..) {
        context.report(message);
    }
    if writer.media_notes {
        body = format!("{}\n\n<references />\n", body.trim_end_matches('\n'));
    }
    if !writer.notes.is_empty() {
        for (index, note) in writer.notes.iter().enumerate() {
            body.push('\n');
            let _ = write!(
                body,
                "\\[{}] {}\n\n",
                index + 1,
                note.trim_end_matches('\n')
            );
        }
    }
    if let (Some(template), Some(fields)) = (&options.template, fields.as_mut()) {
        fields.insert("body", Val::text(body.trim_end_matches('\n')));
        if options.table_of_contents {
            fields.insert("toc", Val::Bool(true));
        }
        Ok(template.render(fields).render(None))
    } else if writer.notes.is_empty() {
        if body.trim_end_matches('\n').is_empty() {
            Ok(String::new())
        } else {
            body.truncate(body.trim_end_matches('\n').len());
            body.push('\n');
            if wiki == Wiki::Jira && doc.blocks.last().is_some_and(ends_with_display_math) {
                body.push('\n');
            }
            Ok(body)
        }
    } else {
        Ok(body)
    }
}

struct Writer<'a> {
    wiki: Wiki,
    options: &'a WriterOptions,
    depth_limit: usize,
    output_limit: u64,
    cells_left: usize,
    notes: Vec<String>,
    media_notes: bool,
    in_link: bool,
    in_list: bool,
    in_figure: bool,
    messages: Vec<LogMessage>,
}

impl Writer<'_> {
    fn depth(&self, depth: usize) -> Result<()> {
        if depth > self.depth_limit {
            Err(depth_error())
        } else {
            Ok(())
        }
    }

    fn output_size(&self, bytes: usize) -> Result<()> {
        if bytes as u64 > self.output_limit {
            return Err(Error::Parse("Wiki output size limit exceeded".into()));
        }
        Ok(())
    }

    fn blocks(&mut self, blocks: &[Block], depth: usize) -> Result<String> {
        self.depth(depth)?;
        let mut out = String::new();
        let mut skipped_raw = false;
        for block in blocks {
            let text = self.block(block, depth)?;
            if self.wiki == Wiki::Doku && text.is_empty() && matches!(block, Block::RawBlock(..)) {
                if !skipped_raw {
                    out.push('\n');
                    skipped_raw = true;
                }
                continue;
            }
            skipped_raw = false;
            if matches!(block, Block::HorizontalRule)
                && !out.is_empty()
                && (self.wiki == Wiki::Doku || self.wiki == Wiki::Media && !out.ends_with("\n\n"))
            {
                out.push('\n');
            }
            out.push_str(&text);
        }
        Ok(out)
    }

    fn block(&mut self, block: &Block, depth: usize) -> Result<String> {
        match block {
            Block::Table(table) => self.table(table, depth + 1),
            Block::BlockQuote(blocks) => self.quote(blocks, depth + 1),
            _ => self.ordinary_block(block, depth),
        }
    }

    fn ordinary_block(&mut self, block: &Block, depth: usize) -> Result<String> {
        Ok(match block {
            Block::Plain(items) | Block::Para(items) => {
                let mut body = self.inlines(items, depth)?;
                if self.wiki == Wiki::Media && body.starts_with(['*', '#', ':', ';', ' ']) {
                    body.insert_str(0, "<nowiki></nowiki>");
                }
                if !body.is_empty() {
                    body.push_str(
                        if matches!(block, Block::Plain(_)) && self.wiki != Wiki::Jira
                            || self.in_list && self.wiki == Wiki::Jira
                        {
                            "\n"
                        } else {
                            "\n\n"
                        },
                    );
                }
                body
            }
            Block::Header(level, attr, items) => {
                let body = self.inlines(items, depth)?;
                match self.wiki {
                    Wiki::Media => {
                        let marks = "=".repeat((*level).clamp(1, 6) as usize);
                        let implicit = stringify(items).replace(' ', "_");
                        let id = if attr.id == implicit || attr.id.is_empty() {
                            String::new()
                        } else {
                            format!("<span id=\"{}\"></span>\n", escape_html(&attr.id))
                        };
                        format!("{id}{marks} {body} {marks}\n\n")
                    }
                    Wiki::Doku => {
                        let marks = "=".repeat((7 - (*level).clamp(1, 6)) as usize);
                        format!("{marks} {} {marks}\n\n", escape_doku(&stringify(items)))
                    }
                    Wiki::Jira => format!("h{level}. {{anchor:{}}}{body}\n", attr.id),
                    Wiki::Creole => String::new(),
                }
            }
            Block::HorizontalRule => match self.wiki {
                Wiki::Media => "-----\n\n".into(),
                Wiki::Jira => "----\n".into(),
                _ => "----\n\n".into(),
            },
            Block::CodeBlock(attr, code) => self.code(attr, code),
            Block::RawBlock(format, text)
                if format.0 == self.wiki.name()
                    || self.wiki == Wiki::Media && format.0 == "html" =>
            {
                format!("{}\n", text.trim_end_matches('\n'))
            }
            Block::RawBlock(format, text)
                if self.wiki == Wiki::Doku
                    && format.0 == "html"
                    && self.options.extensions.contains(Extension::RawHtml) =>
            {
                format!("<HTML>{text}</HTML>\n\n")
            }
            Block::RawBlock(_, _) => String::new(),
            Block::BulletList(_) | Block::OrderedList(_, _) => {
                let previous = self.in_list;
                self.in_list = true;
                let mut output = if self.wiki == Wiki::Media && needs_html_list(block) {
                    self.html_list(block, depth + 1)?
                } else {
                    self.list(block, "", depth + 1)?
                };
                self.in_list = previous;
                if !output.is_empty() && !output.ends_with("\n\n") {
                    output.push('\n');
                }
                output
            }
            Block::DefinitionList(terms) => self.definitions(terms, depth + 1)?,
            Block::Table(_) | Block::BlockQuote(_) => String::new(),
            Block::Figure(attr, caption, blocks) => {
                let previous = std::mem::replace(&mut self.in_figure, true);
                let result = self.figure(attr, &caption.long, blocks, depth + 1);
                self.in_figure = previous;
                result?
            }
            Block::Div(attr, blocks) => {
                let content = self.blocks(blocks, depth + 1)?;
                match self.wiki {
                    Wiki::Media => format!(
                        "<div{}>\n{}</div>\n\n",
                        html_attr(attr),
                        ensure_newline(content)
                    ),
                    Wiki::Jira if attr.has_class("panel") => format!(
                        "{{panel{}}}\n{}{{panel}}\n",
                        jira_args(attr),
                        ensure_newline(content.trim_end_matches('\n').into())
                    ),
                    Wiki::Jira if let Some(color) = attr.get("color") => format!(
                        "{{color:{color}}}\n{}\n{{color}}\n",
                        content.trim_end_matches('\n')
                    ),
                    Wiki::Doku => content + "\n",
                    _ => content,
                }
            }
            Block::LineBlock(lines) => {
                let lines = lines
                    .iter()
                    .map(|line| self.inlines(line, depth + 1))
                    .collect::<Result<Vec<_>>>()?;
                format!("{}\n\n", lines.join(self.line_break()))
            }
        })
    }

    fn definitions(
        &mut self,
        terms: &[(Vec<Inline>, Vec<Vec<Block>>)],
        depth: usize,
    ) -> Result<String> {
        self.depth(depth)?;
        let html = self.wiki == Wiki::Media
            && terms.iter().any(|(_, definitions)| {
                definitions.iter().any(|body| {
                    body.len() > 1
                        && body.iter().any(|block| list_items(block).is_none())
                        && body
                            .iter()
                            .filter(|block| list_items(block).is_none())
                            .count()
                            > 1
                })
            });
        let mut out = if html {
            "<dl>\n".to_owned()
        } else {
            String::new()
        };
        for (term, definitions) in terms {
            let term = self.inlines(term, depth)?;
            if html {
                let _ = writeln!(out, "<dt>{term}</dt>");
                for body in definitions {
                    let content = self.html_blocks(body, depth)?;
                    let _ = writeln!(out, "<dd>{}</dd>", content.trim_end_matches('\n'));
                }
                continue;
            }
            let mut rendered = Vec::new();
            for body in definitions {
                let text = self.definition_body(body, depth)?;
                rendered.push(text.trim_end_matches('\n').to_owned());
            }
            match self.wiki {
                Wiki::Doku => {
                    let _ = writeln!(out, "  * **{term}** {}", rendered.join("; "));
                }
                Wiki::Jira => {
                    let _ = writeln!(out, "* *{term}*\n{}", rendered.join("\n"));
                }
                _ => {
                    let _ = writeln!(out, "; {}", term.replace(':', "<nowiki>:</nowiki>"));
                    for definition in rendered {
                        let _ = writeln!(out, ": {definition}");
                    }
                }
            }
        }
        if html {
            out.truncate(out.trim_end_matches('\n').len());
            out.push_str("</dl>\n");
        }
        out.push('\n');
        Ok(out)
    }

    fn definition_body(&mut self, blocks: &[Block], depth: usize) -> Result<String> {
        self.depth(depth)?;
        if self.wiki == Wiki::Media {
            let mut text = String::new();
            for block in blocks {
                if list_items(block).is_some() {
                    text.push_str(&self.list(block, ";", depth + 1)?);
                } else {
                    text.push_str(&ensure_newline(
                        self.block(block, depth)?.trim_end_matches('\n').into(),
                    ));
                }
            }
            return Ok(text);
        }
        let wrap = self.wiki == Wiki::Doku
            && blocks.len() > 1
            && blocks
                .iter()
                .skip(1)
                .any(|block| list_items(block).is_none());
        let mut text = if wrap {
            "<WRAP>\n".to_owned()
        } else {
            String::new()
        };
        for block in blocks {
            if list_items(block).is_some() {
                text.push_str(&self.list(
                    block,
                    if self.wiki == Wiki::Doku { "  " } else { "*" },
                    depth + 1,
                )?);
            } else {
                let rendered = self.block(block, depth)?;
                if self.wiki == Wiki::Doku && matches!(block, Block::CodeBlock(_, _)) {
                    text.push_str(&rendered);
                } else {
                    text.push_str(&ensure_newline(rendered.trim_end_matches('\n').into()));
                }
            }
        }
        if wrap {
            text.push_str("</WRAP>\n");
        }
        Ok(text)
    }

    fn code(&self, attr: &Attr, code: &str) -> String {
        let lang = attr
            .classes
            .iter()
            .find(|name| *name != "numberLines" && *name != "numberSource");
        match self.wiki {
            Wiki::Media => {
                if let Some(lang) = lang {
                    let line = if attr.has_class("numberLines") || attr.has_class("numberSource") {
                        " line"
                    } else {
                        ""
                    };
                    let start = attr.get("startFrom").map_or_else(String::new, |start| {
                        format!(" start=\"{}\"", escape_html(start))
                    });
                    format!(
                        "<syntaxhighlight lang=\"{}\"{line}{start}>{code}</syntaxhighlight>\n",
                        escape_html(lang)
                    )
                } else {
                    format!("<pre>{}</pre>\n", escape_html(code))
                }
            }
            Wiki::Doku => {
                let lang = lang.map_or_else(String::new, |lang| format!(" {lang}"));
                format!("<code{lang}>\n{code}\n</code>\n\n")
            }
            Wiki::Jira => {
                if let Some(lang) = lang {
                    format!("{{code:{lang}}}\n{code}\n{{code}}\n")
                } else {
                    format!("{{noformat}}\n{code}{{noformat}}\n")
                }
            }
            Wiki::Creole => String::new(),
        }
    }

    fn quote(&mut self, blocks: &[Block], depth: usize) -> Result<String> {
        self.depth(depth)?;
        let content = if self.wiki == Wiki::Doku {
            String::new()
        } else {
            self.blocks(blocks, depth)?
        };
        let body = content.trim_end_matches('\n');
        Ok(match self.wiki {
            Wiki::Media => format!("<blockquote>{body}\n</blockquote>\n"),
            Wiki::Jira if matches!(blocks, [Block::Para(_) | Block::Plain(_)]) => {
                format!("bq. {body}\n")
            }
            Wiki::Jira => format!("{{quote}}\n{body}{{quote}}\n"),
            Wiki::Doku => {
                let mut out = String::new();
                for block in blocks {
                    let content = self.block(block, depth)?;
                    match block {
                        Block::Plain(_) => out.push_str(&content),
                        Block::CodeBlock(_, _) => {
                            out.push_str("> ");
                            out.push_str(&content);
                        }
                        Block::BlockQuote(_) => {
                            for line in content.split_inclusive('\n') {
                                if line.starts_with('>') {
                                    out.push('>');
                                }
                                out.push_str(line);
                            }
                        }
                        Block::Para(_) if !content.is_empty() => {
                            out.push_str("> ");
                            out.push_str(&content);
                        }
                        _ => {
                            for line in content.split_inclusive('\n') {
                                if line != "\n" {
                                    out.push_str("> ");
                                }
                                out.push_str(line);
                            }
                        }
                    }
                }
                out
            }
            Wiki::Creole => content,
        })
    }

    fn list(&mut self, block: &Block, prefix: &str, depth: usize) -> Result<String> {
        self.depth(depth)?;
        let Some((ordered, items)) = list_items(block) else {
            return Ok(String::new());
        };
        let marker = match (self.wiki, ordered) {
            (Wiki::Doku, true) => "-",
            (_, true) => "#",
            _ => "*",
        };
        let (line_prefix, nested_prefix) = if self.wiki == Wiki::Doku {
            (format!("{prefix}  {marker} "), format!("{prefix}  "))
        } else {
            (format!("{prefix}{marker} "), format!("{prefix}{marker}"))
        };
        let mut out = String::new();
        for item in items {
            let start = out.len();
            out.push_str(&line_prefix);
            let mut first = true;
            if self.wiki == Wiki::Doku
                && item
                    .iter()
                    .filter(|child| matches!(child, Block::Plain(_) | Block::Para(_)))
                    .count()
                    > 1
            {
                out.push_str("<WRAP>\n");
                for child in item {
                    if list_items(child).is_some() {
                        out.push_str(&self.list(child, &nested_prefix, depth + 1)?);
                    } else {
                        out.push_str(&ensure_newline(
                            self.block(child, depth)?.trim_end_matches('\n').into(),
                        ));
                    }
                }
                out.push_str("</WRAP>\n");
                continue;
            }
            for child in item {
                if list_items(child).is_some() {
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str(&self.list(child, &nested_prefix, depth + 1)?);
                } else {
                    let text = self.block(child, depth)?;
                    if !first && self.wiki != Wiki::Doku && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    if self.wiki == Wiki::Doku && !first && matches!(child, Block::CodeBlock(_, _))
                    {
                        out.push_str(&text);
                    } else if self.wiki == Wiki::Media && !first {
                        let _ = write!(out, "{nested_prefix}: {}", text.trim_end_matches('\n'));
                    } else if self.wiki == Wiki::Jira && ends_with_display_math(child) {
                        out.push_str(&text);
                    } else {
                        out.push_str(text.trim_end_matches('\n'));
                    }
                }
                first = false;
            }
            if self.wiki == Wiki::Media && out.len() == start + line_prefix.len() {
                out.pop();
            }
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        Ok(out)
    }

    fn html_list(&mut self, block: &Block, depth: usize) -> Result<String> {
        self.depth(depth)?;
        let Some((ordered, items)) = list_items(block) else {
            return Ok(String::new());
        };
        let tag = if ordered { "ol" } else { "ul" };
        let mut out = format!("<{tag}");
        if let Block::OrderedList(attr, _) = block {
            if attr.start != 1 {
                let _ = write!(out, " start=\"{}\"", attr.start);
            }
            let style = match attr.style {
                randoc_ast::ListNumberStyle::Decimal => Some("decimal"),
                randoc_ast::ListNumberStyle::LowerRoman => Some("lower-roman"),
                randoc_ast::ListNumberStyle::UpperRoman => Some("upper-roman"),
                randoc_ast::ListNumberStyle::LowerAlpha => Some("lower-alpha"),
                randoc_ast::ListNumberStyle::UpperAlpha => Some("upper-alpha"),
                _ => None,
            };
            if let Some(style) = style {
                let _ = write!(out, " style=\"list-style-type: {style};\"");
            }
        }
        out.push_str(">\n");
        for item in items {
            out.push_str("<li>");
            let content = self.html_blocks(item, depth)?;
            out.push_str(content.trim_end_matches('\n'));
            out.push_str("</li>\n");
        }
        out.truncate(out.trim_end_matches('\n').len());
        let _ = writeln!(out, "</{tag}>");
        Ok(out)
    }

    fn html_blocks(&mut self, blocks: &[Block], depth: usize) -> Result<String> {
        self.depth(depth)?;
        let mut out = String::new();
        for block in blocks {
            match block {
                Block::Para(items) => {
                    let _ = writeln!(out, "<p>{}</p>", self.inlines(items, depth)?);
                }
                Block::BlockQuote(children) => {
                    let _ = writeln!(
                        out,
                        "<blockquote>{}</blockquote>",
                        self.html_blocks(children, depth + 1)?
                    );
                }
                Block::BulletList(_) | Block::OrderedList(_, _) => {
                    out.push_str(&self.html_list(block, depth + 1)?);
                }
                _ => out.push_str(&ensure_newline(
                    self.block(block, depth)?.trim_end_matches('\n').into(),
                )),
            }
        }
        Ok(out)
    }

    fn figure(
        &mut self,
        attr: &Attr,
        caption: &[Block],
        blocks: &[Block],
        depth: usize,
    ) -> Result<String> {
        self.depth(depth)?;
        match self.wiki {
            Wiki::Media => {
                if let [Block::Plain(items) | Block::Para(items)] = blocks
                    && let [Inline::Image(image_attr, alt, target)] = items.as_slice()
                {
                    let mut figure_attr = attr.clone();
                    figure_attr.classes.insert(0, "figure".into());
                    if url_start(&target.url)
                        && caption.is_empty()
                        && alt.is_empty()
                        && attr.is_empty()
                    {
                        return Ok(format!(
                            "{}\n\n",
                            self.image(image_attr, alt, target, false, depth)?
                        ));
                    }
                    let thumb = matches!(blocks, [Block::Para(_)]);
                    let image = self.image(image_attr, alt, target, thumb, depth)?;
                    Ok(format!(
                        "<div{}>\n{image}\n</div>\n\n",
                        html_attr(&figure_attr)
                    ))
                } else {
                    let mut attr = attr.clone();
                    attr.classes.insert(0, "figure".into());
                    Ok(format!(
                        "<div{}>\n{}</div>\n\n",
                        html_attr(&attr),
                        self.blocks(blocks, depth)?
                    ))
                }
            }
            Wiki::Doku => {
                let mut body = self.blocks(blocks, depth)?;
                if caption.is_empty() {
                    body.push('\n');
                } else {
                    body.push_str(&self.blocks(caption, depth)?);
                    body.push_str("\n\n");
                }
                Ok(body)
            }
            Wiki::Jira if attr.get("title").is_some() => {
                let body = self.blocks(blocks, depth)?;
                let title = attr.get("title").unwrap_or_default();
                Ok(format!(
                    "{{panel:title={title}}}\n{{anchor:{}}}\n{body}{{panel}}\n",
                    attr.id
                ))
            }
            _ => self.blocks(blocks, depth),
        }
    }

    fn inlines(&mut self, items: &[Inline], depth: usize) -> Result<String> {
        self.depth(depth)?;
        let mut out = String::new();
        for (index, item) in items.iter().enumerate() {
            let adjacent = self.wiki == Wiki::Jira
                && (out.ends_with(|ch: char| {
                    ch.is_alphanumeric() || matches!(ch, '\u{2004}' | '\u{2005}')
                }) || matches!(items.get(index + 1), Some(Inline::Str(s)) if s.starts_with(char::is_alphanumeric)));
            if self.wiki == Wiki::Media
                && out.ends_with('[')
                && matches!(item, Inline::Link(..) | Inline::Image(..))
            {
                out.push_str("<nowiki/>");
            }
            out.push_str(&self.inline(item, adjacent, depth)?);
        }
        Ok(out)
    }

    fn inline(&mut self, item: &Inline, adjacent: bool, depth: usize) -> Result<String> {
        Ok(match item {
            Inline::Str(text) => match self.wiki {
                Wiki::Media if text.contains("://") => {
                    format!("<nowiki>{}</nowiki>", escape_html(text))
                }
                Wiki::Media => escape_html(text),
                Wiki::Doku => escape_doku(text),
                Wiki::Jira => escape_jira(text),
                Wiki::Creole => text.clone(),
            },
            Inline::Space => " ".into(),
            Inline::SoftBreak
                if self.options.wrap_text == WrapOption::WrapPreserve && !self.in_list =>
            {
                "\n".into()
            }
            Inline::SoftBreak => " ".into(),
            Inline::LineBreak => self.line_break().into(),
            Inline::Emph(items) => self.mark(items, ["''", "//", "_"], adjacent, depth)?,
            Inline::Strong(items) => self.mark(items, ["'''", "**", "*"], adjacent, depth)?,
            Inline::Underline(items) => self.mark(items, ["<u>", "__", "+"], adjacent, depth)?,
            Inline::Strikeout(items) => self.mark(items, ["<s>", "<del>", "-"], adjacent, depth)?,
            Inline::Superscript(items) => {
                self.mark(items, ["<sup>", "<sup>", "^"], adjacent, depth)?
            }
            Inline::Subscript(items) => {
                self.mark(items, ["<sub>", "<sub>", "~"], adjacent, depth)?
            }
            Inline::SmallCaps(items) | Inline::Cite(_, items) => self.inlines(items, depth + 1)?,
            Inline::Quoted(kind, items) => {
                let body = self.inlines(items, depth + 1)?;
                match kind {
                    QuoteType::DoubleQuote if self.wiki == Wiki::Jira => format!("\"{body}\""),
                    QuoteType::SingleQuote if self.wiki == Wiki::Jira => format!("'{body}'"),
                    QuoteType::DoubleQuote => format!("“{body}”"),
                    QuoteType::SingleQuote => format!("‘{body}’"),
                }
            }
            Inline::Code(_, text) => match self.wiki {
                Wiki::Media => format!("<code>{}</code>", escape_html(text)),
                Wiki::Doku => format!("''%%{text}%%''"),
                Wiki::Jira => format!("{{{{{}}}}}", text.replace('\\', "&bsol;")),
                Wiki::Creole => text.clone(),
            },
            Inline::Math(kind, text) => match self.wiki {
                Wiki::Media => format!(
                    "<math display=\"{}\">{text}</math>",
                    if *kind == MathType::DisplayMath {
                        "block"
                    } else {
                        "inline"
                    }
                ),
                Wiki::Doku => {
                    let delim = if *kind == MathType::DisplayMath {
                        "$$"
                    } else {
                        "$"
                    };
                    format!("{delim}{text}{delim}")
                }
                Wiki::Jira => self.jira_math(*kind, text, depth)?,
                Wiki::Creole => text.clone(),
            },
            Inline::RawInline(format, text)
                if format.0 == self.wiki.name()
                    || self.wiki == Wiki::Media && format.0 == "html" =>
            {
                text.clone()
            }
            Inline::RawInline(format, text)
                if self.wiki == Wiki::Doku
                    && format.0 == "html"
                    && self.options.extensions.contains(Extension::RawHtml) =>
            {
                format!("<html>{text}</html>")
            }
            Inline::RawInline(_, _) => String::new(),
            Inline::Link(attr, items, target) => self.link(attr, items, target, depth + 1)?,
            Inline::Image(attr, items, target) => {
                self.image(attr, items, target, !self.in_figure, depth + 1)?
            }
            Inline::Note(blocks) => match self.wiki {
                Wiki::Media => {
                    self.media_notes = true;
                    format!(
                        "<ref>{}</ref>",
                        self.blocks(blocks, depth + 1)?.trim_end_matches('\n')
                    )
                }
                Wiki::Doku => {
                    let mut content = if self.in_list {
                        self.depth(depth + 1)?;
                        let mut content = String::new();
                        for block in blocks {
                            let text = self.block(block, depth + 1)?;
                            content.push_str(text.trim_end_matches('\n'));
                            content.push('\n');
                        }
                        content
                    } else {
                        self.blocks(blocks, depth + 1)?
                    };
                    content.truncate(content.trim_end_matches('\n').len());
                    if matches!(blocks.last(), Some(Block::Para(_))) && !self.in_list
                        || matches!(blocks.last(), Some(Block::CodeBlock(..))) && self.in_list
                    {
                        content.push('\n');
                    }
                    format!("(({content}))")
                }
                Wiki::Jira => {
                    let index = self.notes.len();
                    self.notes.push(String::new());
                    let content = self.blocks(blocks, depth + 1)?;
                    self.notes[index] = content;
                    format!("[{}]", index + 1)
                }
                Wiki::Creole => String::new(),
            },
            Inline::Span(attr, items) => {
                let body = self.inlines(items, depth + 1)?;
                match self.wiki {
                    Wiki::Media if !attr.is_empty() => {
                        format!("<span{}>{body}</span>", html_attr(attr))
                    }
                    Wiki::Jira if !attr.id.is_empty() => format!("{{anchor:{}}}{body}", attr.id),
                    Wiki::Jira if let Some(color) = attr.get("color") => {
                        format!("{{color:{color}}}{body}{{color}}")
                    }
                    _ => body,
                }
            }
        })
    }

    fn line_break(&self) -> &'static str {
        match self.wiki {
            Wiki::Media => "<br />\n",
            Wiki::Doku => "\\\\\n",
            _ => "\n",
        }
    }

    fn mark(
        &mut self,
        items: &[Inline],
        marks: [&str; 3],
        adjacent: bool,
        depth: usize,
    ) -> Result<String> {
        let body = self.inlines(items, depth + 1)?;
        let mark = match self.wiki {
            Wiki::Media => marks[0],
            Wiki::Doku => marks[1],
            _ => marks[2],
        };
        Ok(if let Some(tag) = mark.strip_prefix('<') {
            format!("{mark}{body}</{tag}")
        } else if adjacent {
            format!("{{{mark}}}{body}{{{mark}}}")
        } else {
            format!("{mark}{body}{mark}")
        })
    }

    fn link(
        &mut self,
        attr: &Attr,
        items: &[Inline],
        target: &Target,
        depth: usize,
    ) -> Result<String> {
        let nested = self.in_link;
        self.in_link = true;
        let body = self.inlines(items, depth)?;
        self.in_link = nested;
        let url = if matches!(self.wiki, Wiki::Media | Wiki::Doku) {
            target.url.strip_prefix('/').unwrap_or(&target.url)
        } else {
            &target.url
        };
        let label_target = if self.wiki == Wiki::Doku {
            &target.url
        } else {
            url
        };
        let matching = matches!(items, [Inline::Str(text)] if text == label_target);
        Ok(match self.wiki {
            Wiki::Media if nested => format!("<span>{body}</span>"),
            Wiki::Media if url_start(url) => {
                if matching {
                    url.to_owned()
                } else if body.is_empty() {
                    format!("[{url}]")
                } else {
                    format!("[{url} {body}]")
                }
            }
            Wiki::Media
                if matching
                    || attr.has_class("wikilink")
                        && !body.is_empty()
                        && body.replace(' ', "_") == url =>
            {
                format!("[[{url}]]")
            }
            Wiki::Media => format!("[[{url}|{body}]]"),
            Wiki::Doku
                if let Some(email) = url.strip_prefix("mailto:")
                    && matches!(items, [Inline::Str(text)] if text == email) =>
            {
                format!("<{email}>")
            }
            Wiki::Doku if matching => target.url.clone(),
            Wiki::Doku => format!("[[{url}|{body}]]"),
            Wiki::Jira if attr.has_class("attachment") => format!("[^{url}]"),
            Wiki::Jira if matching => url.to_owned(),
            Wiki::Jira => {
                let prefix = if !url_start(url) && !url.starts_with(['/', '#', '~']) {
                    "/"
                } else {
                    ""
                };
                format!("[{body}|{prefix}{url}]")
            }
            Wiki::Creole => body,
        })
    }

    fn jira_math(&mut self, kind: MathType, tex: &str, depth: usize) -> Result<String> {
        let display = kind == MathType::DisplayMath;
        let remaining = self.depth_limit.saturating_sub(depth);
        let body = match randoc_math::to_inlines(kind, tex, remaining) {
            Ok(inlines) => self.inlines(&inlines, depth)?,
            Err(error) => {
                self.messages.push(LogMessage::CouldNotConvertTexMath(
                    tex.to_owned(),
                    error.detail(tex),
                ));
                let delim = if display { "$$" } else { "$" };
                let escaped = escape_jira_math(tex);
                format!("{delim}{escaped}{delim}")
            }
        };
        Ok(if display { format!("\n{body}\n") } else { body })
    }

    fn image(
        &mut self,
        attr: &Attr,
        alt: &[Inline],
        target: &Target,
        thumb: bool,
        depth: usize,
    ) -> Result<String> {
        let body = self.inlines(alt, depth)?;
        let url = &target.url;
        let width = attr.get("width").unwrap_or("").trim_end_matches("px");
        let height = attr.get("height").unwrap_or("").trim_end_matches("px");
        let size = if height.is_empty() {
            width.into()
        } else {
            format!("{width}x{height}")
        };
        Ok(match self.wiki {
            Wiki::Media if url_start(url) => format!("<nowiki></nowiki>{url}<nowiki></nowiki>"),
            Wiki::Media => {
                let mut out = format!("[[File:{url}");
                let thumb_title = thumb.then(|| target.title.strip_prefix("fig:")).flatten();
                if thumb_title.is_some() {
                    out.push_str("|thumb|none");
                }
                if !size.is_empty() {
                    let _ = write!(out, "|{size}px");
                }
                if let Some(title) = thumb_title {
                    let alt = if title.is_empty() { &body } else { title };
                    let bytes = out
                        .len()
                        .saturating_add(alt.len())
                        .saturating_add(body.len())
                        .saturating_add(5 * usize::from(!alt.is_empty()))
                        .saturating_add(usize::from(!body.is_empty()))
                        .saturating_add(2);
                    self.output_size(bytes)?;
                    if !alt.is_empty() {
                        let _ = write!(out, "|alt={alt}");
                    }
                    if !body.is_empty() {
                        let _ = write!(out, "|{body}");
                    }
                } else {
                    let label = if alt.is_empty() { &target.title } else { &body };
                    if !label.is_empty() {
                        let _ = write!(out, "|{label}");
                    }
                }
                out.push_str("]]");
                out
            }
            Wiki::Doku => {
                let query = if size.is_empty() {
                    String::new()
                } else {
                    format!("?{size}")
                };
                let label = if target.title.is_empty() {
                    &body
                } else {
                    &target.title
                };
                let label = if label.is_empty() {
                    String::new()
                } else {
                    format!("|{label}")
                };
                format!("{{{{{url}{query}{label}}}}}")
            }
            Wiki::Jira => {
                let mut args = attr.classes.clone();
                args.extend(
                    attr.attributes
                        .iter()
                        .map(|(key, value)| format!("{key}={value}")),
                );
                if !target.title.is_empty() {
                    args.push(format!("title={}", target.title));
                }
                if !body.is_empty() {
                    args.push(format!("alt={body}"));
                }
                if args.is_empty() {
                    format!("!{url}!")
                } else {
                    format!("!{url}|{}!", args.join(", "))
                }
            }
            Wiki::Creole => body,
        })
    }

    fn charge(&mut self, cells: usize) -> Result<()> {
        self.cells_left = self.cells_left.checked_sub(cells).ok_or_else(cell_error)?;
        Ok(())
    }

    fn table(&mut self, table: &Table, depth: usize) -> Result<String> {
        self.depth(depth)?;
        let width = table.col_specs.len();
        let mut rows = Vec::new();
        for row in &table.head.rows {
            rows.push((row, width));
        }
        for body in &table.bodies {
            for row in &body.head {
                rows.push((row, width));
            }
            for row in &body.body {
                rows.push((row, usize::try_from(body.row_head_columns).unwrap_or(0)));
            }
        }
        for row in &table.foot.rows {
            rows.push((row, width));
        }
        for (row, _) in &rows {
            for cell in &row.cells {
                let cols = usize::try_from(cell.col_span.max(1)).map_err(|_| cell_error())?;
                let lines = usize::try_from(cell.row_span.max(1)).map_err(|_| cell_error())?;
                self.charge(cols.checked_mul(lines).ok_or_else(cell_error)? - 1)?;
            }
        }
        let caption = if self.wiki != Wiki::Jira {
            self.blocks(&table.caption.long, depth)?
        } else {
            String::new()
        };
        if self.wiki == Wiki::Media {
            let mut attr = table.attr.clone();
            attr.classes.insert(0, "wikitable".into());
            let mut out = format!("{{|{}\n", html_attr(&attr));
            if !caption.trim().is_empty() {
                let _ = writeln!(
                    out,
                    "|+ {}",
                    caption.split_whitespace().collect::<Vec<_>>().join(" ")
                );
            }
            let mut occupancy = vec![0; width];
            for (index, (row, heads)) in rows.iter().enumerate() {
                let _ = writeln!(out, "|-{}", html_attr(&row.attr));
                self.media_row(&mut out, table, row, *heads, &mut occupancy, index, depth)?;
            }
            out.push_str("|}\n\n");
            return Ok(out);
        }
        self.pipe_table(table, &rows, &caption, depth)
    }

    #[allow(clippy::too_many_arguments)]
    fn media_row(
        &mut self,
        out: &mut String,
        table: &Table,
        row: &Row,
        head_columns: usize,
        occupancy: &mut [usize],
        index: usize,
        depth: usize,
    ) -> Result<()> {
        let mut column = 0;
        for cell in &row.cells {
            while column < occupancy.len() && occupancy[column] > index {
                column += 1;
            }
            let marker = if column < head_columns { '!' } else { '|' };
            let align = if cell.align == Alignment::AlignDefault {
                table
                    .col_specs
                    .get(column)
                    .map_or(Alignment::AlignDefault, |c| c.align)
            } else {
                cell.align
            };
            let mut attr = cell.attr.clone();
            if cell.row_span > 1 {
                attr.attributes
                    .push(("rowspan".into(), cell.row_span.to_string()));
            }
            if cell.col_span > 1 {
                attr.attributes
                    .push(("colspan".into(), cell.col_span.to_string()));
            }
            if let Some(align) = alignment_name(align) {
                attr.attributes
                    .push(("style".into(), format!("text-align: {align};")));
            }
            let body = self.blocks(&cell.content, depth)?;
            let body = body.trim_end_matches('\n');
            out.push(marker);
            if !attr.is_empty() {
                out.push_str(&html_attr(&attr));
                out.push('|');
            }
            if !body.is_empty() {
                out.push(' ');
                out.push_str(body);
            }
            out.push('\n');
            let next = column.saturating_add(cell.col_span.max(1) as usize);
            let last = next.min(occupancy.len());
            if column < last {
                occupancy[column..last].fill(index.saturating_add(cell.row_span.max(1) as usize));
            }
            column = next;
        }
        Ok(())
    }

    fn pipe_table(
        &mut self,
        table: &Table,
        rows: &[(&Row, usize)],
        caption: &str,
        depth: usize,
    ) -> Result<String> {
        let width = table.col_specs.len();
        let mut widths = vec![0; width];
        let mut rendered = Vec::new();
        for (index, (row, _)) in rows.iter().enumerate() {
            let mut cells = Vec::new();
            for cell in &row.cells {
                let text = self.blocks(&cell.content, depth)?;
                let body = text.trim_end_matches('\n');
                let body = if self.wiki == Wiki::Doku {
                    body.lines()
                        .map(|line| line.trim().trim_end_matches("\\\\"))
                        .collect::<Vec<_>>()
                        .join("\\\\ ")
                } else {
                    body.replace('\n', " ")
                };
                let offset = literal(&body).offset();
                let column = cells.len();
                if let Some(spec) = table.col_specs.get(column) {
                    let pad = match spec.align {
                        Alignment::AlignCenter => 4,
                        Alignment::AlignDefault => 0,
                        _ => 2,
                    };
                    widths[column] = widths[column].max(offset + pad);
                }
                cells.push((body, offset));
                let next = column
                    .saturating_add(cell.col_span.max(1) as usize)
                    .min(width);
                cells.resize(next, (String::new(), 0));
            }
            self.charge(width.saturating_sub(cells.len()))?;
            cells.resize(width, (String::new(), 0));
            rendered.push((index < table.head.rows.len(), cells));
        }
        let mut out = String::new();
        if !caption.trim().is_empty() {
            let _ = writeln!(out, "{}", caption.trim_end());
        }
        for (heading, row) in rendered {
            let mark = match (self.wiki, heading) {
                (Wiki::Doku, true) => "^",
                (Wiki::Jira, true) => "||",
                _ => "|",
            };
            out.push_str(mark);
            for (column, (body, offset)) in row.into_iter().enumerate() {
                if self.wiki == Wiki::Jira {
                    if !body.is_empty() {
                        out.push(' ');
                        out.push_str(&body);
                        out.push(' ');
                    }
                } else if !body.is_empty() {
                    let gap = widths[column].saturating_sub(offset);
                    let left = match table.col_specs[column].align {
                        Alignment::AlignRight => gap,
                        Alignment::AlignCenter => gap / 2,
                        _ => 0,
                    };
                    if self.wiki == Wiki::Doku {
                        self.output_size(
                            out.len()
                                .saturating_add(body.len())
                                .saturating_add(gap)
                                .saturating_add(mark.len() + 2),
                        )?;
                    }
                    out.push_str(&" ".repeat(left));
                    out.push_str(&body);
                    out.push_str(&" ".repeat(gap - left));
                }
                out.push_str(mark);
            }
            out.push('\n');
        }
        out.push('\n');
        Ok(out)
    }
}

fn ends_with_display_math(mut block: &Block) -> bool {
    while let Some((_, items)) = list_items(block) {
        let Some(last) = items.last().and_then(|item| item.last()) else {
            return false;
        };
        block = last;
    }
    matches!(block, Block::Plain(items) | Block::Para(items)
        if matches!(items.last(), Some(Inline::Math(MathType::DisplayMath, _))))
}

fn list_items(block: &Block) -> Option<(bool, &[Vec<Block>])> {
    match block {
        Block::BulletList(items) => Some((false, items)),
        Block::OrderedList(_, items) => Some((true, items)),
        _ => None,
    }
}

fn needs_html_list(block: &Block) -> bool {
    if matches!(block, Block::OrderedList(attr, _) if attr.start != 1 || attr.style != randoc_ast::ListNumberStyle::DefaultStyle && attr.style != randoc_ast::ListNumberStyle::Decimal)
    {
        return true;
    }
    list_items(block).is_some_and(|(_, items)| {
        items.iter().any(|item| {
            item.iter()
                .filter(|block| list_items(block).is_some())
                .count()
                > 1
                || item
                    .iter()
                    .filter(|block| matches!(block, Block::Para(_)))
                    .count()
                    > 1
        })
    })
}

fn ensure_newline(mut text: String) -> String {
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn alignment_name(align: Alignment) -> Option<&'static str> {
    match align {
        Alignment::AlignLeft => Some("left"),
        Alignment::AlignRight => Some("right"),
        Alignment::AlignCenter => Some("center"),
        Alignment::AlignDefault => None,
    }
}

fn html_attr(attr: &Attr) -> String {
    let mut out = String::new();
    if !attr.id.is_empty() {
        let _ = write!(out, " id=\"{}\"", escape_html(&attr.id));
    }
    if !attr.classes.is_empty() {
        let _ = write!(out, " class=\"{}\"", escape_html(&attr.classes.join(" ")));
    }
    for (key, value) in &attr.attributes {
        let key = if key == "source" { "data-source" } else { key };
        let _ = write!(out, " {}=\"{}\"", escape_html(key), escape_html(value));
    }
    out
}

fn jira_args(attr: &Attr) -> String {
    if attr.attributes.is_empty() {
        return String::new();
    }
    let args: Vec<_> = attr
        .attributes
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    format!(":{}", args.join("|"))
}

fn escape_html(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_doku(text: &str) -> String {
    let url = text.contains("://");
    let mut out = String::new();
    let mut at = 0;
    while at < text.len() {
        let tail = &text[at..];
        if ["**", "//", "__", "''", "[[", "{{", "%%", "(("]
            .iter()
            .any(|mark| tail.starts_with(mark))
            && !url
        {
            let _ = write!(out, "%%{}%%", &tail[..2]);
            at += 2;
        } else if let Some(ch) = tail.chars().next() {
            out.push(ch);
            at += ch.len_utf8();
        }
    }
    out
}

fn escape_jira_math(tex: &str) -> String {
    let mut out = String::new();
    for (index, ch) in tex.char_indices() {
        let rest = &tex[index..];
        let previous = tex[..index].chars().next_back();
        let follows_escape =
            previous.is_some_and(|c| c.is_whitespace() || matches!(c, '{' | '}' | '('));
        if ch == '\\' && follows_escape {
            out.push_str("&bsol;");
            continue;
        }
        let mark = matches!(ch, '_' | '+' | '-' | '^' | '*' | '~' | '!' | '&' | '|');
        let emoticon = [
            "()", "(x)", "(X)", "(/)", "(y)", "(n)", "(i)", "(a)", "(-)", "(+)", "(!)", "(?)",
            ":P", ":D", ":)", ":(", ";)",
        ]
        .iter()
        .any(|face| rest.starts_with(face));
        let next = rest[ch.len_utf8()..].chars().next();
        if matches!(ch, '{' | '}' | '[')
            || ch == ']' && !previous.is_some_and(char::is_whitespace)
            || mark
                && (follows_escape && !next.is_some_and(char::is_whitespace)
                    || previous.is_some_and(|c| "_+-^*~!&|".contains(c))
                    || next.is_some_and(|c| "_+-^*~!&|{}".contains(c)))
            || emoticon
        {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn escape_jira(text: &str) -> String {
    let mut out = String::new();
    if [":P", ":D", ":)", ":(", ";)", "(/)", "(x)"]
        .iter()
        .any(|face| {
            text.strip_prefix(face)
                .is_some_and(|rest| !rest.starts_with(char::is_alphanumeric))
        })
    {
        out.push('\\');
    }
    if text.starts_with('-') {
        out.push('\\');
    }
    for ch in text.chars() {
        if matches!(ch, '_' | '[' | ']' | '{' | '}' | '!' | '\\') || ch == '*' && text.len() > 1 {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}
