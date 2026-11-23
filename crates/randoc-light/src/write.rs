use std::cell::RefCell;

use randoc_ast::{
    Alignment, Attr, Block, ColWidth, Inline, MathType, MetaValue, Pandoc, QuoteType, Table, Target,
};
use randoc_core::shared::{meta_context, stringify};
use randoc_core::{Context, Error, Result, WrapOption, WriterOptions};
use randoc_layout::{self as layout, Doc, literal};
use randoc_templates::Val;

use crate::{Format, check_depth, expansion_budget};

pub(crate) fn write(
    format: Format,
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let budget = document_budget(doc, context)?;
    let columns = usize::try_from(options.columns).unwrap_or(1).max(1);
    if columns > budget.max(1) && matches!(format, Format::Vimdoc | Format::Haddock) {
        return Err(output_limit());
    }
    let vars = if options.template.is_some() {
        let max_depth = context.limits.max_nesting_depth;
        let shared = RefCell::new(&mut *context);
        Some(meta_context::meta_to_context(
            &doc.meta,
            &options.variables,
            max_depth,
            &mut |blocks| {
                State::new(format, options, &mut shared.borrow_mut(), columns, budget)
                    .blocks(blocks)
            },
            &mut |xs| {
                State::new(format, options, &mut shared.borrow_mut(), columns, budget).inlines(xs)
            },
        )?)
    } else {
        None
    };
    let mut state = State::new(format, options, context, columns, budget);
    if format == Format::Vimdoc
        && let Some(prefix) = doc
            .meta
            .get("vimdoc-prefix")
            .map(randoc_core::shared::stringify_meta)
        && !prefix.is_empty()
    {
        state.vim_prefix = format!("{prefix}-{}", state.vim_prefix);
    }
    state.top_level = true;
    let body = state.blocks(&doc.blocks)?;
    let notes = state.footnotes(doc.blocks.last())?;
    let mut body = state.render_body(&layout::chomp(body + notes))?;
    let trailing_lines = match format {
        Format::T2t if matches!(doc.blocks.last(), Some(Block::Figure(_, caption, _)) if !caption.long.is_empty()) => {
            2
        }
        Format::T2t
            if matches!(doc.blocks.last(), Some(Block::Div(_, blocks))
            if !body.is_empty() && !matches!(blocks.last(), Some(Block::Plain(_)))) =>
        {
            2
        }
        Format::Textile
            if state.notes.is_empty()
                && let Some(Block::CodeBlock(_, code)) = doc.blocks.last()
                && textile_native_code(code) =>
        {
            1 + usize::from(code.ends_with('\n')) + usize::from(options.template.is_none())
        }
        Format::Textile => state.notes.last().map_or(0, |note| {
            textile_note_spacing(note.blocks.last(), note.compact)
                + usize::from(options.template.is_none())
        }),
        _ => 0,
    };
    if body.len().saturating_add(trailing_lines) > budget {
        return Err(output_limit());
    }
    body.extend(std::iter::repeat_n('\n', trailing_lines));

    let (Some(template), Some(mut vars)) = (&options.template, vars) else {
        return Ok(body);
    };
    if format == Format::Vimdoc {
        vim_variables(doc, &mut vars, &mut state)?;
    }
    if format == Format::T2t && options.table_of_contents {
        vars.insert("toc", Val::Bool(true));
    }
    let body_is_empty = body.is_empty();
    vars.insert("body", Val::text(body));
    let mut text = template
        .render_limited(&vars, budget, budget)
        .ok_or_else(output_limit)?;
    if format == Format::Vimdoc
        && randoc_data::default_template("vimdoc").is_some_and(|source| source == template.source())
    {
        if let Some(Val::Simple(toc)) = vars.get("toc") {
            let toc = state.render_unwrapped(toc)?;
            let offset = text.find(&toc).map(|start| start + toc.len());
            if let Some(offset) = offset
                && text[offset..].starts_with("\n\n\n")
                && text[offset + 3..].starts_with(['=', '-'])
            {
                text.remove(offset);
            }
        } else if body_is_empty && let Some(offset) = text.rfind("\n\n\n\n\n vim:tw=") {
            text.replace_range(offset..offset + 3, "");
        }
    }
    Ok(text)
}

fn output_limit() -> Error {
    Error::App("The output is too large.".into())
}

struct State<'a, 'd> {
    format: Format,
    options: &'a WriterOptions,
    context: &'a mut Context,
    columns: usize,
    budget: usize,
    generated: usize,
    breaks: usize,
    wrap_prefix_bytes: usize,
    notes: Vec<Note<'d>>,
    list: Vec<&'static str>,
    vim_prefix: String,
    top_level: bool,
    textile_definition: bool,
}

#[derive(Clone, Copy)]
struct Note<'d> {
    blocks: &'d [Block],
    compact: bool,
}

impl<'a, 'd> State<'a, 'd> {
    fn new(
        format: Format,
        options: &'a WriterOptions,
        context: &'a mut Context,
        columns: usize,
        budget: usize,
    ) -> Self {
        Self {
            format,
            options,
            context,
            columns,
            budget,
            generated: 0,
            breaks: 0,
            wrap_prefix_bytes: 0,
            notes: Vec::new(),
            list: Vec::new(),
            vim_prefix: options.identifier_prefix.clone(),
            top_level: false,
            textile_definition: false,
        }
    }

    fn charge_generated(&mut self, bytes: usize) -> Result<()> {
        self.generated = self.generated.saturating_add(bytes);
        if self.generated > self.budget {
            return Err(output_limit());
        }
        Ok(())
    }

    fn render_body(&self, doc: &Doc) -> Result<String> {
        if self.options.wrap_text != WrapOption::WrapAuto
            || matches!(self.format, Format::Textile | Format::T2t)
        {
            return self.render_unwrapped(doc);
        }
        // Each breaking space can add one newline and repeat its enclosing prefixes.
        let allowance = self.breaks.saturating_add(self.wrap_prefix_bytes);
        let unwrapped_budget = self
            .budget
            .checked_sub(allowance)
            .ok_or_else(output_limit)?;
        doc.render_limited(unwrapped_budget)
            .ok_or_else(output_limit)?;
        let text = doc.render(Some(self.columns));
        if text.len() > self.budget {
            return Err(output_limit());
        }
        Ok(text)
    }

    fn render_unwrapped(&self, doc: &Doc) -> Result<String> {
        doc.render_limited(self.budget).ok_or_else(output_limit)
    }

    fn breaking_space(&mut self) -> Doc {
        self.breaks = self.breaks.saturating_add(1);
        layout::space()
    }

    fn charge_indent(&mut self, indent: usize, breaks_before: usize) -> Result<()> {
        self.wrap_prefix_bytes = self
            .wrap_prefix_bytes
            .saturating_add(indent.saturating_mul(self.breaks.saturating_sub(breaks_before)));
        self.charge_generated(indent)
    }

    fn inlines(&mut self, xs: &'d [Inline]) -> Result<Doc> {
        let mut out = Doc::default();
        for (index, x) in xs.iter().enumerate() {
            if self.format == Format::Muse
                && matches!(x, Inline::Space | Inline::SoftBreak)
                && matches!(xs.get(index + 1), Some(Inline::Note(_)))
            {
                out += literal(" ");
            } else {
                out += self.inline(x)?;
            }
        }
        Ok(out)
    }

    fn inline(&mut self, x: &'d Inline) -> Result<Doc> {
        use Format::*;
        let f = self.format;
        let doc = match x {
            Inline::Str(s) => literal(escape(f, s)),
            Inline::Space => self.breaking_space(),
            Inline::SoftBreak => {
                if f != Textile && self.options.wrap_text == WrapOption::WrapPreserve {
                    layout::cr()
                } else {
                    self.breaking_space()
                }
            }
            Inline::LineBreak => {
                literal(match f {
                    Muse => "<br>",
                    T2t => "\\\\",
                    _ => "",
                }) + layout::cr()
            }
            Inline::Emph(xs) => self.enclose(
                xs,
                match f {
                    Textile => ("_", "_"),
                    Muse => ("*", "*"),
                    T2t => ("//", "//"),
                    Haddock => ("/", "/"),
                    _ => ("", ""),
                },
            )?,
            Inline::Strong(xs) => self.enclose(
                xs,
                match f {
                    Textile => ("*", "*"),
                    Muse | T2t => ("**", "**"),
                    Haddock => ("__", "__"),
                    _ => ("", ""),
                },
            )?,
            Inline::Underline(xs) => self.enclose(
                xs,
                match f {
                    Textile => ("+", "+"),
                    T2t => ("__", "__"),
                    Muse => ("<u>", "</u>"),
                    _ => ("", ""),
                },
            )?,
            Inline::Strikeout(xs) => self.enclose(
                xs,
                match f {
                    Textile => ("-", "-"),
                    Muse => ("<del>", "</del>"),
                    T2t => ("--", "--"),
                    Haddock => ("~~", "~~"),
                    _ => ("", ""),
                },
            )?,
            Inline::Superscript(xs) => self.enclose(
                xs,
                match f {
                    Textile => ("[^", "^]"),
                    Muse | T2t => ("<sup>", "</sup>"),
                    _ => ("", ""),
                },
            )?,
            Inline::Subscript(xs) => self.enclose(
                xs,
                match f {
                    Textile => ("[~", "~]"),
                    Muse | T2t => ("<sub>", "</sub>"),
                    _ => ("", ""),
                },
            )?,
            Inline::SmallCaps(xs) => self.enclose(
                xs,
                if f == Muse {
                    ("<sc>", "</sc>")
                } else {
                    ("", "")
                },
            )?,
            Inline::Quoted(kind, xs) => {
                let quotes = match (f, kind) {
                    (Muse | T2t | Haddock, QuoteType::SingleQuote) => ("‘", "’"),
                    (Muse | T2t | Haddock, QuoteType::DoubleQuote) => ("“", "”"),
                    (_, QuoteType::SingleQuote) => ("'", "'"),
                    (_, QuoteType::DoubleQuote) => ("\"", "\""),
                };
                self.enclose(xs, quotes)?
            }
            Inline::Code(attr, s) => literal(match f {
                Vimdoc if vim_help(s).is_some() => format!("|{}|", vim_help(s).unwrap_or("")),
                Textile => format!("@{s}@"),
                Muse => {
                    if s.contains('=')
                        || s.starts_with(char::is_whitespace)
                        || s.ends_with(char::is_whitespace)
                    {
                        format!("<code>{s}</code>")
                    } else {
                        format!("={s}=")
                    }
                }
                T2t => format!("``{s}``"),
                Haddock if attr.has_class("identifier") => format!("'{s}'"),
                Haddock if attr.has_class("module") => format!("\"{s}\""),
                Haddock => format!("@{}@", escape(f, s)),
                Vimdoc => format!("`{s}`"),
                _ => s.clone(),
            }),
            Inline::Math(kind, s) if f == Muse => self.math(*kind, s)?,
            Inline::Math(kind, s) => literal(match f {
                Textile => format!(
                    "<span class=\"math\">{}</span>",
                    randoc_html::shared::escape_text(s, false)
                ),
                Haddock => {
                    if *kind == MathType::DisplayMath {
                        format!("\\[{s}\\]")
                    } else {
                        format!("\\({s}\\)")
                    }
                }
                Vimdoc => format!("`${s}$`"),
                T2t if *kind == MathType::DisplayMath => format!("$${s}$$"),
                T2t => format!("${s}$"),
                _ => s.clone(),
            }),
            Inline::RawInline(format, s) => {
                if f == Muse && format.as_str() != "muse" {
                    literal(format!(
                        "<literal style=\"{}\">{s}</literal>",
                        format.as_str()
                    ))
                } else if format.as_str() == "html" && f == Textile
                    || Format::get(format.as_str()) == Some(f)
                {
                    literal(s)
                } else {
                    Doc::default()
                }
            }
            Inline::Link(attr, xs, target) => self.link(attr, xs, target)?,
            Inline::Image(attr, xs, target) => self.image(attr, xs, target)?,
            Inline::Note(blocks) => {
                if f == T2t {
                    literal("(") + layout::chomp(self.blocks(blocks)?) + literal(")")
                } else {
                    self.notes.push(Note {
                        blocks,
                        compact: f == Textile && !self.list.is_empty(),
                    });
                    let n = self.notes.len();
                    literal(match f {
                        Haddock => format!("<#notes [{n}]>"),
                        Vimdoc => format!(" |footnote{n}|"),
                        _ => format!("[{n}]"),
                    })
                }
            }
            Inline::Span(attr, xs) if f == Textile && attr.has_class("underline") => {
                self.enclose(xs, ("+", "+"))?
            }
            Inline::Span(attr, xs) if f == Muse && !attr.is_empty() => {
                let mut span = Doc::default();
                if !attr.id.is_empty() {
                    span += literal(format!("#{}", attr.id));
                    if !xs.is_empty() {
                        span += literal(" ");
                    }
                }
                if attr.id.is_empty() || !xs.is_empty() {
                    let open = attr.classes.first().map_or_else(
                        || "<class>".into(),
                        |class| format!("<class name=\"{class}\">"),
                    );
                    span += literal(open) + self.inlines(xs)? + literal("</class>");
                }
                span
            }
            Inline::Span(attr, xs) if f == Vimdoc && attr.has_class("smallcaps") => {
                let doc = self.inlines(xs)?;
                literal(self.render_unwrapped(&doc)?.to_uppercase())
            }
            Inline::Span(attr, xs) if f == Haddock && !attr.id.is_empty() && xs.is_empty() => {
                literal(format!("#{}#", attr.id))
            }
            Inline::Span(_, xs) | Inline::Cite(_, xs) => self.inlines(xs)?,
        };
        Ok(doc)
    }

    fn enclose(&mut self, xs: &'d [Inline], (before, after): (&str, &str)) -> Result<Doc> {
        Ok(literal(before) + self.inlines(xs)? + literal(after))
    }

    fn math(&mut self, kind: MathType, tex: &str) -> Result<Doc> {
        use randoc_archive::xml::{XmlLimits, XmlNode, XmlOptions, parse_fragment};
        let mut options = self.options.clone();
        options.template = None;
        options.wrap_text = WrapOption::WrapNone;
        options.html_math_method = randoc_core::HtmlMathMethod::PlainMath;
        let html = randoc_html::render_blocks(
            &[Block::Plain(vec![Inline::Math(kind, tex.into())])],
            &options,
            self.context,
        )?;
        let xml = parse_fragment(
            &html,
            &XmlLimits::from_limits(&self.context.limits),
            XmlOptions {
                html_entities: true,
                ..Default::default()
            },
        );
        let Ok(xml) = xml else {
            return Ok(literal(tex));
        };
        let mut out = Doc::default();
        let mut pending = xml
            .nodes
            .iter()
            .rev()
            .map(|node| (Some(node), ""))
            .collect::<Vec<_>>();
        while let Some((node, close)) = pending.pop() {
            match node {
                None => out += literal(close),
                Some(XmlNode::Text(text)) => {
                    if self.format == Format::Muse && text.contains(['=', '*', '[', ']']) {
                        out += literal(format!("<verbatim>{text}</verbatim>"));
                    } else {
                        out += literal(text);
                    }
                }
                Some(XmlNode::Element(element)) => {
                    let (start, close) = match (self.format, element.name.local.as_str()) {
                        (Format::Muse, "em" | "i") => ("*", "*"),
                        (_, "strong" | "b") => ("**", "**"),
                        (_, "sub") => ("<sub>", "</sub>"),
                        (_, "sup") => ("<sup>", "</sup>"),
                        _ => ("", ""),
                    };
                    out += literal(start);
                    pending.push((None, close));
                    pending.extend(element.children.iter().rev().map(|node| (Some(node), "")));
                }
            }
        }
        Ok(out)
    }

    fn link(&mut self, attr: &Attr, xs: &'d [Inline], target: &Target) -> Result<Doc> {
        let url = &target.url;
        let plain = stringify(xs);
        let auto = plain == *url || url.strip_prefix("mailto:") == Some(plain.as_str());
        let label = if self.format == Format::Muse {
            let mut label = Doc::default();
            for x in xs {
                if let Inline::Str(text) = x
                    && text.contains(['[', ']'])
                {
                    label += literal(format!("<verbatim>{text}</verbatim>"));
                } else {
                    label += self.inline(x)?;
                }
            }
            label
        } else {
            self.inlines(xs)?
        };
        Ok(match self.format {
            Format::Textile => {
                let label = if plain == *url { literal("$") } else { label };
                let classes = if attr.classes.is_empty() || plain == *url && attr.classes == ["uri"]
                {
                    String::new()
                } else {
                    format!("({})", attr.classes.join(" "))
                };
                layout::nowrap(
                    literal(format!("\"{classes}")) + label + literal(format!("\":{url}")),
                )
            }
            Format::Muse if auto && !url.starts_with("mailto:") || xs.is_empty() => {
                literal(format!("[[{url}]]"))
            }
            Format::Muse => literal(format!("[[{url}][")) + label + literal("]]"),
            Format::T2t if auto && url.starts_with("mailto:") => literal(format!("<{plain}>")),
            Format::T2t if auto => literal(&plain),
            Format::T2t => {
                let url = url.strip_prefix('/').unwrap_or(url);
                layout::nowrap(literal("[") + label + literal(format!(" {url}]")))
            }
            Format::Haddock if auto && !url.starts_with("mailto:") => literal(format!("<{url}>")),
            Format::Haddock => layout::nowrap(literal(format!("<{url} ")) + label + literal(">")),
            Format::Vimdoc if vim_url(url).is_some() => {
                let help = literal(format!("|{}|", vim_url(url).unwrap_or_default()));
                if auto || xs.is_empty() {
                    help
                } else {
                    label + self.breaking_space() + help
                }
            }
            Format::Vimdoc if url.starts_with('#') && url[1..] == plain => {
                literal(format!("|{}|", &url[1..]))
            }
            Format::Vimdoc if auto || xs.is_empty() => {
                literal(url.strip_prefix("mailto:").unwrap_or(url))
            }
            Format::Vimdoc => {
                label + self.breaking_space() + literal(url.strip_prefix("mailto:").unwrap_or(url))
            }
            Format::Opml => label,
        })
    }

    fn image(&mut self, attr: &Attr, xs: &'d [Inline], target: &Target) -> Result<Doc> {
        let url = &target.url;
        let doc = self.inlines(xs)?;
        let alt = self.render_unwrapped(&doc)?;
        let title = if target.title.is_empty() || self.format == Format::Haddock && !alt.is_empty()
        {
            &alt
        } else {
            &target.title
        };
        let dimension = |name| {
            attr.get(name)
                .and_then(randoc_html::shared::Dimension::parse)
        };
        Ok(literal(match self.format {
            Format::Textile => {
                let (width, height) = (dimension("width"), dimension("height"));
                let style = if width.is_none() && height.is_none() {
                    String::new()
                } else {
                    let css = |value: Option<randoc_html::shared::Dimension>| {
                        value.map_or_else(
                            || "auto".into(),
                            |value| {
                                dimension_pixels(&value, self.options.dpi)
                                    .map_or_else(|| value.css(), |pixels| format!("{pixels}px"))
                            },
                        )
                    };
                    format!("{{width:{};height:{};}}", css(width), css(height))
                };
                if title.is_empty() {
                    format!("!{style}{url}!")
                } else {
                    format!("!{style}{url}({title})!")
                }
            }
            Format::Muse => {
                if title.is_empty() {
                    format!("[[{url}]]")
                } else {
                    format!("[[{url}][{title}]]")
                }
            }
            Format::T2t => {
                let pixels = |name| {
                    dimension(name).and_then(|value| dimension_pixels(&value, self.options.dpi))
                };
                let size = match (pixels("width"), pixels("height")) {
                    (width, Some(height)) => {
                        format!("?{}x{height}", width.as_deref().unwrap_or("0"))
                    }
                    (Some(width), None) => format!("?{width}"),
                    (None, None) => String::new(),
                };
                if title.is_empty() {
                    format!("[{url}{size}]")
                } else {
                    format!("[{url}{size}|{title}]")
                }
            }
            Format::Haddock => {
                if title.is_empty() {
                    format!("<<{url}>>")
                } else {
                    format!("<<{url} {title}>>")
                }
            }
            _ => String::new(),
        }))
    }

    fn blocks(&mut self, blocks: &'d [Block]) -> Result<Doc> {
        let top_level = std::mem::take(&mut self.top_level);
        let mut out = Doc::default();
        let mut previous: Option<&Block> = None;
        let mut skipped_tex = false;
        let mut blocks = blocks.iter().peekable();
        while let Some(block) = blocks.next() {
            let next = if self.format == Format::T2t
                && let Block::RawBlock(format, text) = block
                && format.as_str() == "html"
            {
                let mut raw = text.trim_end_matches('\n').to_owned();
                while let Some(Block::RawBlock(format, extra)) = blocks.peek()
                    && format.as_str() == "html"
                {
                    raw.push('\n');
                    raw.push_str(extra.trim_end_matches('\n'));
                    blocks.next();
                }
                literal(format!("\"\"\"\n{raw}\n\"\"\""))
            } else {
                self.block(block)?
            };
            if next.is_empty() {
                skipped_tex |= matches!(self.format, Format::Textile | Format::T2t)
                    && matches!(block, Block::RawBlock(format, _) if matches!(format.as_str(), "tex" | "latex"));
                continue;
            }
            let extra_blank = usize::from(std::mem::take(&mut skipped_tex));
            let plain = if matches!(self.format, Format::Vimdoc | Format::Muse) {
                previous.is_some_and(ends_plain)
            } else {
                matches!(previous, Some(Block::Plain(_)))
            };
            out = if plain && !(self.format == Format::Muse && matches!(block, Block::RawBlock(..)))
                || (self.format == Format::Vimdoc && matches!(previous, Some(Block::CodeBlock(..))))
                || self.format == Format::Textile
                    && matches!(previous, Some(Block::RawBlock(..)))
                    && matches!(
                        block,
                        Block::RawBlock(..)
                            | Block::Plain(_)
                            | Block::Para(_)
                            | Block::HorizontalRule
                    )
            {
                out.above(next)
            } else {
                let blanks = match (self.format, previous, block) {
                    (Format::Textile, Some(Block::Para(_)), _) if self.textile_definition => 0,
                    (Format::Textile, Some(Block::CodeBlock(_, text)), _)
                        if textile_native_code(text) =>
                    {
                        2 + usize::from(text.ends_with('\n'))
                    }
                    (Format::Textile, Some(Block::BlockQuote(quote)), _)
                        if matches!(quote.as_slice(), [Block::Para(_)] | [Block::Plain(_)]) =>
                    {
                        3
                    }
                    (Format::T2t, Some(Block::Figure(_, caption, _)), _)
                        if !caption.long.is_empty() =>
                    {
                        2
                    }
                    (Format::T2t, Some(_), Block::HorizontalRule) => 2,
                    (Format::T2t, Some(Block::Div(..)), Block::Div(..)) => 3,
                    (Format::T2t, Some(Block::Div(_, children)), _)
                        if matches!(children.as_slice(), [Block::Div(..)]) =>
                    {
                        3
                    }
                    (Format::T2t, Some(Block::Div(_, children)), _)
                        if children
                            .iter()
                            .any(|child| matches!(child, Block::Div(..) | Block::Para(_))) =>
                    {
                        2
                    }
                    (Format::T2t, Some(Block::CodeBlock(..)), _)
                        if top_level && !matches!(block, Block::Para(_) | Block::CodeBlock(..)) =>
                    {
                        2
                    }
                    (Format::Muse, Some(Block::Table(table)), _)
                        if table
                            .col_specs
                            .iter()
                            .any(|col| col.width != ColWidth::ColWidthDefault) =>
                    {
                        0
                    }
                    _ => 1,
                };
                if out.is_empty() {
                    next
                } else {
                    out + layout::blanklines(blanks + extra_blank) + next
                }
            };
            previous = Some(block);
        }
        Ok(out)
    }

    fn block(&mut self, b: &'d Block) -> Result<Doc> {
        use Format::*;
        let f = self.format;
        if matches!(b, Block::HorizontalRule) && matches!(f, Vimdoc | Haddock) {
            self.charge_generated(self.columns)?;
        }
        Ok(match b {
            Block::Para(xs) if f == Textile && self.textile_definition => {
                literal("<p>") + self.inlines(xs)? + literal("</p>")
            }
            Block::Plain(xs) | Block::Para(xs) => self.inlines(xs)?,
            Block::Header(level, attr, xs) => self.header(*level, attr, xs)?,
            Block::HorizontalRule => literal(match f {
                Textile => "<hr />".into(),
                Muse => "----".into(),
                T2t => "--------------------".into(),
                Haddock => "_".repeat(self.columns),
                Vimdoc => self.vim_rule(),
                Opml => String::new(),
            }),
            Block::CodeBlock(attr, code) => self.code_block(attr, code),
            Block::RawBlock(format, text) => {
                if f == Muse && format.as_str() != "muse" {
                    literal(format!(
                        "<literal style=\"{}\">\n{}\n</literal>",
                        format.as_str(),
                        text.trim_end_matches('\n')
                    ))
                } else if format.as_str() == "html" && f == Textile
                    || Format::get(format.as_str()) == Some(f)
                {
                    literal(text)
                } else {
                    Doc::default()
                }
            }
            Block::LineBlock(lines) => {
                let mut docs = Vec::new();
                for xs in lines {
                    docs.push(self.inlines(xs)?);
                }
                let body = layout::vcat(docs);
                if f == Muse {
                    literal("<verse>").above(body).above(literal("</verse>"))
                } else {
                    body
                }
            }
            Block::BlockQuote(blocks) => {
                let breaks = self.breaks;
                let body = self.blocks(blocks)?;
                match f {
                    Textile
                        if matches!(blocks.as_slice(), [Block::Para(_)] | [Block::Plain(_)]) =>
                    {
                        literal("bq. ") + body
                    }
                    Textile => {
                        let close = if matches!(blocks.last(), Some(Block::BlockQuote(quote)) if matches!(quote.as_slice(), [Block::Para(_)] | [Block::Plain(_)]))
                        {
                            3
                        } else {
                            1
                        };
                        literal("<blockquote>").above_blank(body)
                            + layout::blanklines(close)
                            + literal("</blockquote>")
                    }
                    Muse => literal("<quote>").above(body).above(literal("</quote>")),
                    Vimdoc => {
                        self.charge_indent(2, breaks)?;
                        layout::nest(2, body)
                    }
                    _ => body,
                }
            }
            Block::OrderedList(attr, items)
                if f == Textile
                    && (self.textile_definition
                        || complex_list(items)
                        || needs_textile_list_style(*attr)) =>
            {
                self.html_list(b)?
            }
            Block::BulletList(items) if f == Textile && complex_list(items) => self.html_list(b)?,
            Block::BulletList(items) => self.list(items, None)?,
            Block::OrderedList(attr, items) => self.list(items, Some(*attr))?,
            Block::DefinitionList(items) => self.definitions(items)?,
            Block::Table(table) => self.table(table)?,
            Block::Figure(_, caption, contents) if f == Textile => {
                let doc = self.html(b)?;
                let html = self.render_unwrapped(&doc)?;
                let start = html.lines().next().unwrap_or("<figure>");
                let mut figure = literal(start);
                if !caption.long.is_empty() {
                    figure = figure
                        .above_blank(literal("<figcaption>"))
                        .above_blank(self.blocks(&caption.long)?)
                        .above_blank(literal("</figcaption>"));
                }
                let blanks = if matches!(contents.last(), Some(Block::Plain(_))) {
                    1
                } else {
                    2
                };
                figure.above_blank(self.blocks(contents)?)
                    + layout::blanklines(blanks)
                    + literal("</figure>")
            }
            Block::Figure(_, caption, blocks) if matches!(f, Haddock | Muse | T2t) => {
                self.blocks(blocks)?.above(self.blocks(&caption.long)?)
            }
            Block::Div(attr, blocks) if f == Textile => {
                let mut opening = String::from("<div");
                if !attr.id.is_empty() {
                    html_attribute(&mut opening, "id", &attr.id);
                }
                if !attr.classes.is_empty() {
                    html_attribute(&mut opening, "class", &attr.classes.join(" "));
                }
                for (name, value) in &attr.attributes {
                    html_attribute(&mut opening, name, value);
                }
                opening.push('>');
                let blanks = if matches!(blocks.as_slice(), [Block::Plain(_)]) {
                    1
                } else {
                    2
                };
                literal(opening).above_blank(self.blocks(blocks)?)
                    + layout::blanklines(blanks)
                    + literal("</div>")
            }
            Block::Figure(_, _, blocks) | Block::Div(_, blocks) => self.blocks(blocks)?,
        })
    }

    fn header(&mut self, level: i64, attr: &Attr, xs: &'d [Inline]) -> Result<Doc> {
        let marker_count = usize::try_from(level.clamp(1, 6)).unwrap_or(1);
        let doc = self.inlines(xs)?;
        let title = self.render_unwrapped(&doc)?;
        let id = &attr.id;
        Ok(literal(match self.format {
            Format::Textile => format!("h{level}{}. {title}", textile_attributes(attr)),
            Format::Muse => {
                let default = randoc_core::shared::auto_identifier(xs, self.options.extensions);
                let mut title = format!("{} {title}", "*".repeat(marker_count));
                if !id.is_empty() && *id != default {
                    title.insert_str(0, &format!("#{id}\n"));
                }
                title
            }
            Format::T2t => {
                let markers = "=".repeat(marker_count.min(5));
                format!("{markers} {} {markers}", stringify(xs))
            }
            Format::Haddock => {
                let mut title = format!("{} {title}", "=".repeat(marker_count));
                if !id.is_empty() {
                    title.push_str(&format!("\n#{id}#"));
                }
                title
            }
            Format::Vimdoc => {
                self.charge_generated(self.columns.saturating_mul(2))?;
                let default = randoc_core::shared::auto_identifier(xs, self.options.extensions);
                let id = if id.is_empty() { &default } else { id };
                let level = marker_count;
                let title = if level == 3 {
                    let mut title = Doc::default();
                    for x in xs {
                        let part = self.inline(x)?;
                        let part = match x {
                            Inline::Link(_, label, target) => {
                                let label = {
                                    let doc = self.inlines(label)?;
                                    vim_heading_case(&self.render_unwrapped(&doc)?)
                                };
                                literal(format!("{label} {}", target.url))
                            }
                            Inline::Space | Inline::SoftBreak | Inline::LineBreak => part,
                            _ => literal(vim_heading_case(&self.render_unwrapped(&part)?)),
                        };
                        title += part;
                    }
                    self.render_unwrapped(&title)?
                } else {
                    title
                };
                let body = self.vim_tag(&title, id, false);
                match level {
                    1 => format!("{}\n{body}", "=".repeat(self.columns)),
                    2 => format!("{}\n{body}", "-".repeat(self.columns)),
                    _ => body,
                }
            }
            Format::Opml => title,
        }))
    }

    fn code_block(&self, attr: &Attr, code: &str) -> Doc {
        let lang = attr.classes.first().map_or("", String::as_str);
        literal(match self.format {
            Format::Textile if !textile_native_code(code) => {
                let class = if lang.is_empty() {
                    String::new()
                } else {
                    format!(
                        " class=\"{}\"",
                        randoc_html::shared::escape_attribute(lang, false)
                    )
                };
                let body = randoc_html::shared::escape_attribute(code, false);
                format!("<pre{class}>\n{body}\n</pre>")
            }
            Format::Textile => {
                let class = if lang.is_empty() {
                    String::new()
                } else {
                    format!("({lang})")
                };
                format!("bc{class}. {code}")
            }
            Format::Muse | Format::T2t => {
                let (open, close) = if self.format == Format::Muse {
                    ("<example>", "</example>")
                } else {
                    ("```", "```")
                };
                let newline = if code.ends_with('\n') { "" } else { "\n" };
                format!("{open}\n{code}{newline}{close}")
            }
            Format::Haddock => code
                .lines()
                .map(|s| {
                    if s.is_empty() {
                        ">".into()
                    } else {
                        format!("> {s}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Format::Vimdoc => {
                return literal(format!(">{lang}\n{}", indent_text(code, 4)))
                    .above(layout::flush(literal("<")));
            }
            _ => code.into(),
        })
    }

    fn list(
        &mut self,
        items: &'d [Vec<Block>],
        ordered: Option<randoc_ast::ListAttributes>,
    ) -> Result<Doc> {
        use Format::*;
        let mut out = Doc::default();
        let kind = if ordered.is_some() { "#" } else { "*" };
        self.list.push(kind);
        let loose = matches!(self.format, Haddock | Vimdoc)
            && items
                .iter()
                .any(|item| matches!(item.first(), Some(Block::Para(_))));
        for (index, blocks) in items.iter().enumerate() {
            let prefix = match self.format {
                Textile => format!("{} ", self.list.concat()),
                Muse | Haddock | Vimdoc => {
                    let pad = if self.format == Muse && self.list.len() == 1 {
                        " "
                    } else {
                        ""
                    };
                    if let Some(attr) = ordered {
                        let n = attr.start.saturating_add(index as i64);
                        let marker = list_marker(attr, n, self.format);
                        if self.format == Haddock {
                            format!("{marker:4}")
                        } else {
                            format!("{pad}{marker} ")
                        }
                    } else if self.format == Haddock {
                        "-   ".into()
                    } else {
                        format!("{pad}- ")
                    }
                }
                T2t => {
                    if ordered.is_some() {
                        "+ ".into()
                    } else {
                        "- ".into()
                    }
                }
                Opml => "- ".into(),
            };
            let mut item = Doc::default();
            for (i, block) in blocks.iter().enumerate() {
                let breaks = self.breaks;
                let body = self.block(block)?;
                let nested = matches!(block, Block::BulletList(_) | Block::OrderedList(..));
                if i == 0 {
                    item = if nested {
                        literal(prefix.trim_end()).above(body)
                    } else if self.format == Textile {
                        literal(prefix.as_str()) + body
                    } else {
                        self.charge_indent(prefix.len(), breaks)?;
                        layout::hang(prefix.len(), literal(prefix.as_str()), body)
                    };
                } else {
                    let indent = if self.format == Textile || self.format == T2t && !nested {
                        0
                    } else {
                        prefix.len()
                    };
                    self.charge_indent(indent, breaks)?;
                    let body = layout::nest(indent, body);
                    item = if self.format == T2t
                        || nested
                            && !(matches!(self.format, Muse | Haddock | Vimdoc)
                                && matches!(blocks.first(), Some(Block::Para(_))))
                    {
                        item.above(body)
                    } else {
                        item.above_blank(body)
                    };
                }
            }
            if blocks.is_empty() {
                item = literal(prefix.trim_end());
            }
            out = if loose {
                out.above_blank(item)
            } else {
                out.above(item)
            };
        }
        self.list.pop();
        Ok(out)
    }

    fn definitions(&mut self, items: &'d [(Vec<Inline>, Vec<Vec<Block>>)]) -> Result<Doc> {
        let mut out = Doc::default();
        let loose = matches!(self.format, Format::Haddock | Format::Vimdoc)
            && items
                .iter()
                .flat_map(|(_, definitions)| definitions)
                .any(|blocks| matches!(blocks.first(), Some(Block::Para(_))));
        for (term, definitions) in items {
            let term = match (self.format, term.as_slice()) {
                (Format::Vimdoc, [Inline::Code(attr, code)]) if !attr.id.is_empty() => {
                    self.charge_generated(self.columns)?;
                    literal(self.vim_tag(code, &attr.id, true))
                }
                (Format::Vimdoc, [Inline::Span(attr, xs)]) if !attr.id.is_empty() => {
                    let doc = self.inlines(xs)?;
                    let label = self.render_unwrapped(&doc)?;
                    self.charge_generated(self.columns)?;
                    literal(self.vim_tag(&label, &attr.id, true))
                }
                _ => self.inlines(term)?,
            };
            let mut row = match self.format {
                Format::Textile => literal("<dt>") + term + literal("</dt>"),
                Format::T2t => literal(": **") + term + literal("**"),
                Format::Haddock => literal("[") + term + literal("]"),
                Format::Muse => literal(" ") + term + literal(" :: "),
                _ => term,
            };
            let muse_indent = row.offset();
            for (index, blocks) in definitions.iter().enumerate() {
                let breaks = self.breaks;
                if self.format == Format::Muse {
                    self.list.push("");
                }
                let prior_definition = self.textile_definition;
                self.textile_definition |= self.format == Format::Textile;
                let body = if self.format == Format::T2t {
                    let mut body = Doc::default();
                    for (i, block) in blocks.iter().enumerate() {
                        let rendered = self.block(block)?;
                        body = if i == 0 {
                            body.above(layout::nest(2, rendered))
                        } else if matches!(block, Block::BlockQuote(_)) {
                            body.above_blank(rendered)
                        } else if matches!(block, Block::OrderedList(..) | Block::BulletList(_)) {
                            body.above(layout::nest(2, rendered))
                        } else {
                            body.above(rendered)
                        };
                    }
                    body
                } else {
                    self.blocks(blocks)?
                };
                self.textile_definition = prior_definition;
                if self.format == Format::Muse {
                    self.list.pop();
                }
                let indent = match self.format {
                    Format::Muse => muse_indent,
                    Format::Haddock | Format::Vimdoc => 4,
                    _ => 0,
                };
                self.charge_indent(indent, breaks)?;
                row = match self.format {
                    Format::Textile => {
                        let body = if matches!(blocks.last(), Some(Block::BlockQuote(_))) {
                            body + layout::blankline()
                        } else if matches!(
                            blocks.last(),
                            Some(Block::OrderedList(..) | Block::BulletList(_))
                        ) {
                            body + layout::cr()
                        } else {
                            body
                        };
                        row.above(literal("<dd>") + body + literal("</dd>"))
                    }
                    Format::Muse if index == 0 => row + layout::nest(muse_indent, body),
                    Format::Muse => row.above(layout::hang(
                        muse_indent,
                        literal(format!("{}:: ", " ".repeat(muse_indent.saturating_sub(3)))),
                        body,
                    )),
                    Format::T2t => row.above(body),
                    _ if loose && index > 0 => row.above_blank(layout::nest(4, body)),
                    _ => row.above(layout::nest(4, body)),
                };
            }
            out = if loose {
                out.above_blank(row)
            } else {
                out.above(row)
            };
        }
        Ok(if self.format == Format::Textile {
            literal("<dl>").above(out).above(literal("</dl>"))
        } else {
            out
        })
    }

    fn footnotes(&mut self, last_block: Option<&Block>) -> Result<Doc> {
        let mut out = Doc::default();
        let mut index = 0;
        let mut textile_spacing = textile_note_spacing(last_block, false);
        while let Some(&note) = self.notes.get(index) {
            let breaks = self.breaks;
            let text = if note.compact {
                let mut doc = Doc::default();
                for block in note.blocks {
                    doc = doc.above(self.block(block)?);
                }
                doc
            } else {
                self.blocks(note.blocks)?
            };
            let n = index + 1;
            let entry = match self.format {
                Format::Textile => literal(format!("fn{n}. ")) + text,
                Format::Muse => {
                    let marker = format!("[{n}] ");
                    self.charge_indent(marker.len(), breaks)?;
                    layout::hang(marker.len(), literal(marker), text)
                }
                Format::Haddock => {
                    self.charge_indent(4, breaks)?;
                    layout::hang(4, literal(format!("{n}.  ")), text)
                }
                Format::Vimdoc => {
                    self.charge_generated(self.columns)?;
                    literal(self.vim_tag("", &format!("footnote{n}"), false)).above(text)
                }
                _ => text,
            };
            out = if self.format == Format::Textile {
                let doc = out + layout::blanklines(textile_spacing) + entry;
                textile_spacing = textile_note_spacing(note.blocks.last(), note.compact);
                doc
            } else {
                out.above_blank(entry)
            };
            index += 1;
        }
        Ok(if out.is_empty() {
            out
        } else {
            match self.format {
                Format::Textile => out,
                Format::Haddock => layout::blankline() + literal("#notes#").above_blank(out),
                Format::Vimdoc => layout::blankline() + literal(self.vim_rule()).above_blank(out),
                _ => layout::blankline() + out,
            }
        })
    }

    fn html(&mut self, block: &Block) -> Result<Doc> {
        let mut options = self.options.clone();
        options.template = None;
        options.wrap_text = WrapOption::WrapNone;
        Ok(literal(randoc_html::render_blocks(
            std::slice::from_ref(block),
            &options,
            self.context,
        )?))
    }

    fn html_list(&mut self, block: &Block) -> Result<Doc> {
        let mut block = block.clone();
        randoc_ast::walk::walk_inlines_mut(&mut block, |inline| {
            if let Inline::Str(text) = inline {
                *text = text.replace('’', "'");
            }
        });
        let doc = self.html(&block)?;
        let source = self.render_unwrapped(&doc)?;
        let out = source
            .lines()
            .map(|line| {
                let line = line.trim_start();
                if !line.starts_with("<ol ") {
                    return line.to_owned();
                }
                line.replace("type=\"1\"", "style=\"list-style-type: decimal;\"")
                    .replace("type=\"i\"", "style=\"list-style-type: lower-roman;\"")
                    .replace("type=\"I\"", "style=\"list-style-type: upper-roman;\"")
                    .replace("type=\"a\"", "style=\"list-style-type: lower-alpha;\"")
                    .replace("type=\"A\"", "style=\"list-style-type: upper-alpha;\"")
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(literal(
            out.replace("</ol></li>", "</ol>\n</li>")
                .replace("</ul></li>", "</ul>\n</li>"),
        ))
    }

    fn vim_tag(&self, label: &str, id: &str, wrap: bool) -> String {
        if id.is_empty() {
            return label.into();
        }
        let tag = format!(
            "*{}{}*",
            if id.starts_with(':') {
                ""
            } else {
                &self.vim_prefix
            },
            id
        );
        let gap = self
            .columns
            .saturating_add(2)
            .saturating_sub(display_width(label).saturating_add(display_width(&tag)));
        if gap == 0 && wrap {
            let label =
                layout::hsep(label.split_whitespace().map(literal)).render(Some(self.columns));
            return format!("{:>width$}\n{label}", tag, width = self.columns + 2);
        }
        let gap = if label.is_empty() { gap } else { gap.max(1) };
        format!("{label}{}{tag}", " ".repeat(gap))
    }

    fn vim_rule(&self) -> String {
        " *".repeat(self.columns / 2)
    }

    fn table(&mut self, table: &'d Table) -> Result<Doc> {
        let columns = table.col_specs.len();
        if columns == 0 {
            return Ok(Doc::default());
        }
        if self.format == Format::Textile && !native_textile_table(table) {
            return self.html_table(table);
        }
        let rows = table
            .head
            .rows
            .iter()
            .map(|row| (row, true))
            .chain(table.bodies.iter().flat_map(|body| {
                body.head
                    .iter()
                    .map(|row| (row, true))
                    .chain(body.body.iter().map(|row| (row, false)))
            }))
            .chain(table.foot.rows.iter().map(|row| (row, false)));
        let mut cells = Vec::new();
        let mut headers = Vec::new();
        let mut widths = vec![0; columns];
        let mut generated = 0usize;
        let mut remaining = self.budget;
        for (row, header) in rows {
            generated = generated.saturating_add(columns.saturating_sub(row.cells.len()));
            if generated > self.context.limits.max_table_cells {
                return Err(output_limit());
            }
            let mut rendered = Vec::new();
            for (col, cell) in row.cells.iter().take(columns).enumerate() {
                let doc = self.blocks(&cell.content)?;
                let content = doc.render_limited(remaining).ok_or_else(output_limit)?;
                remaining -= content.len();
                widths[col] = widths[col].max(display_width(&content));
                rendered.push(content);
            }
            rendered.resize_with(columns, String::new);
            headers.push(header);
            cells.push(rendered);
        }
        let caption = self
            .blocks(&table.caption.long)?
            .render_limited(remaining)
            .ok_or_else(output_limit)?;
        let aligns = table
            .col_specs
            .iter()
            .map(|spec| spec.align)
            .collect::<Vec<_>>();
        let result = match self.format {
            Format::Textile => cells
                .iter()
                .zip(&headers)
                .map(|(row, header)| {
                    let mut line = String::from("|");
                    for (i, field) in row.iter().enumerate() {
                        let prefix = if *header {
                            "_. "
                        } else {
                            match aligns[i] {
                                Alignment::AlignLeft => "<. ",
                                Alignment::AlignCenter => "=. ",
                                Alignment::AlignRight => ">. ",
                                Alignment::AlignDefault => "",
                            }
                        };
                        line.push_str(prefix);
                        line.push_str(field);
                        line.push('|');
                    }
                    line
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Format::T2t => {
                for (width, align) in widths.iter_mut().zip(&aligns) {
                    *width += if *align == Alignment::AlignDefault {
                        0
                    } else if *align == Alignment::AlignCenter {
                        4
                    } else {
                        2
                    };
                }
                self.check_table_width(&widths, cells.len(), 1)?;
                let mut lines = Vec::new();
                if !caption.is_empty() {
                    lines.push(caption);
                }
                for row in cells {
                    lines.push(format!(
                        "|{}|",
                        row.iter()
                            .enumerate()
                            .map(|(i, s)| {
                                let single = s.split_whitespace().collect::<Vec<_>>().join(" ");
                                pad(&single, widths[i], aligns[i])
                            })
                            .collect::<Vec<_>>()
                            .join("|")
                    ));
                }
                lines.join("\n")
            }
            Format::Haddock => self.grid_table(table, &cells, &headers, &caption, false)?,
            Format::Muse
                if table
                    .col_specs
                    .iter()
                    .any(|col| col.width != ColWidth::ColWidthDefault)
                    || cells.iter().flatten().any(|s| s.contains('\n')) =>
            {
                self.grid_table(table, &cells, &headers, "", true)?
            }
            Format::Muse => {
                self.check_table_width(&widths, cells.len(), 4)?;
                let mut lines = Vec::new();
                for (row, header) in cells.iter().zip(&headers) {
                    let rendered = row
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            if i + 1 == columns {
                                s.clone()
                            } else {
                                pad(s, widths[i], Alignment::AlignLeft)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(if *header {
                            " || "
                        } else if headers.first() == Some(&true) {
                            " |  "
                        } else {
                            " | "
                        });
                    lines.push(format!(" {rendered}"));
                }
                if !caption.is_empty() {
                    lines.push(format!(" |+ {caption} +|"));
                }
                lines.join("\n")
            }
            Format::Vimdoc => self.vim_table(table, &cells, &headers, &caption, widths)?,
            Format::Opml => String::new(),
        };
        if result.len() > self.budget {
            return Err(output_limit());
        }
        Ok(literal(result))
    }

    fn check_table_width(&self, widths: &[usize], rows: usize, separator: usize) -> Result<()> {
        let bytes = widths
            .iter()
            .copied()
            .fold(1usize, usize::saturating_add)
            .saturating_add(widths.len().saturating_mul(separator))
            .saturating_mul(rows);
        if bytes > self.budget {
            return Err(output_limit());
        }
        Ok(())
    }

    fn vim_table(
        &self,
        table: &Table,
        rows: &[Vec<String>],
        headers: &[bool],
        caption: &str,
        mut widths: Vec<usize>,
    ) -> Result<String> {
        let relative = table
            .col_specs
            .iter()
            .any(|col| col.width != ColWidth::ColWidthDefault);
        for (i, (width, spec)) in widths.iter_mut().zip(&table.col_specs).enumerate() {
            let padding = usize::from(spec.align != Alignment::AlignDefault) * 2;
            if let ColWidth::ColWidth(fraction) = spec.width {
                let minimum = rows
                    .iter()
                    .flat_map(|row| row[i].split_whitespace())
                    .map(display_width)
                    .max()
                    .unwrap_or(0);
                let bound = relative_width(fraction, self.columns);
                let last = usize::from(i + 1 == table.col_specs.len()) * 2;
                *width = bound.saturating_sub(last).max(minimum + padding);
            } else {
                *width = width.saturating_add(padding);
            }
        }
        let line_width = widths
            .iter()
            .copied()
            .fold(4usize, usize::saturating_add)
            .saturating_add(widths.len());
        if line_width > self.budget {
            return Err(output_limit());
        }
        let mut out = String::new();
        for (index, (row, header)) in rows.iter().zip(headers).enumerate() {
            if index > 0 {
                out.push('\n');
                if relative {
                    out.push('\n');
                }
            }
            let wrapped = row
                .iter()
                .zip(&widths)
                .map(|(text, &width)| {
                    if relative {
                        wrap_cell(text, width)
                    } else {
                        vec![text.clone()]
                    }
                })
                .collect::<Vec<_>>();
            let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
            for y in 0..height {
                if out.len() > self.budget {
                    return Err(output_limit());
                }
                if y > 0 {
                    out.push('\n');
                }
                out.push_str("    ");
                let line = wrapped
                    .iter()
                    .enumerate()
                    .map(|(i, lines)| {
                        pad(
                            lines.get(y).map_or("", String::as_str),
                            widths[i],
                            table.col_specs[i].align,
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                out.push_str(line.trim_end());
                if *header && y + 1 == height {
                    out.push_str(if wrapped.last().is_some_and(|lines| lines.len() <= y) {
                        "  ~"
                    } else {
                        " ~"
                    });
                }
            }
        }
        if !caption.is_empty() {
            out.push_str("\n\n");
            out.push_str(&indent_text(caption, 4));
        }
        Ok(out)
    }

    fn html_table(&mut self, table: &Table) -> Result<Doc> {
        let mut table = table.clone();
        table.attr.id.clear();
        if table
            .col_specs
            .iter()
            .any(|column| column.align != Alignment::AlignDefault)
        {
            for column in &mut table.col_specs {
                if column.align == Alignment::AlignDefault {
                    column.align = Alignment::AlignLeft;
                }
            }
        }
        let mut block = Block::Table(Box::new(table));
        randoc_ast::walk::walk_inlines_mut(&mut block, |x| {
            if let Inline::Str(text) = x {
                *text = text.replace('’', "'");
            }
        });
        let doc = self.html(&block)?;
        let html = self.render_unwrapped(&doc)?;
        let mut result = String::new();
        for line in html.lines() {
            let line = line.trim_start();
            if matches!(line, "<colgroup>" | "</colgroup>") {
                continue;
            }
            if line.starts_with("<table style=\"width:") {
                result.push_str("<table>\n");
                continue;
            }
            if let Some(width) = line
                .strip_prefix("<col style=\"width:")
                .and_then(|s| s.split_once('%'))
                .map(|(width, _)| width.trim())
            {
                result.push_str(&format!("<col width=\"{width}%\" />\n"));
                continue;
            }
            let line = line
                .replace(" class=\"header\"", "")
                .replace(" class=\"odd\"", "")
                .replace(" class=\"even\"", "")
                .replace("style=\"text-align: right;\"", "align=\"right\"")
                .replace("style=\"text-align: left;\"", "align=\"left\"")
                .replace("style=\"text-align: center;\"", "align=\"center\"");
            result.push_str(&line);
            result.push('\n');
        }
        result.pop();
        Ok(literal(result))
    }

    fn grid_table(
        &self,
        table: &Table,
        rows: &[Vec<String>],
        headers: &[bool],
        caption: &str,
        muse: bool,
    ) -> Result<String> {
        let mut widths: Vec<_> = table
            .col_specs
            .iter()
            .map(|spec| match spec.width {
                ColWidth::ColWidth(fraction) if fraction.is_finite() && fraction > 0.0 => {
                    ((fraction * self.columns as f64)
                        .floor()
                        .clamp(1.0, self.columns as f64) as usize)
                        .saturating_sub(3)
                        .max(1)
                }
                _ => 1,
            })
            .collect();
        for row in rows {
            for (i, s) in row.iter().enumerate() {
                if table.col_specs[i].width == ColWidth::ColWidthDefault {
                    widths[i] = widths[i].max(s.lines().map(display_width).max().unwrap_or(0));
                } else {
                    widths[i] =
                        widths[i].max(s.split_whitespace().map(display_width).max().unwrap_or(0));
                }
            }
        }
        self.check_table_width(&widths, 1, 3)?;
        let border = |mark: char, alignment: bool| {
            let mut line = String::from("+");
            for (i, &width) in widths.iter().enumerate() {
                let (left, right) = if alignment {
                    match table.col_specs[i].align {
                        Alignment::AlignDefault
                            if table.col_specs[i].width != ColWidth::ColWidthDefault
                                && headers.first() == Some(&true) =>
                        {
                            (true, false)
                        }
                        Alignment::AlignLeft => (true, false),
                        Alignment::AlignRight => (false, true),
                        Alignment::AlignCenter => (true, true),
                        _ => (false, false),
                    }
                } else {
                    (false, false)
                };
                line.push(if left { ':' } else { mark });
                line.extend(std::iter::repeat_n(mark, width));
                line.push(if right { ':' } else { mark });
                line.push('+');
            }
            line
        };
        let base = border('-', false);
        let mut out = if muse || headers.first() != Some(&true) {
            border('-', true)
        } else {
            base.clone()
        };
        for (row, header) in rows.iter().zip(headers) {
            let wrapped = row
                .iter()
                .zip(&widths)
                .map(|(s, &width)| wrap_cell(s, width))
                .collect::<Vec<_>>();
            let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
            for row in 0..height {
                if out.len() > self.budget {
                    return Err(output_limit());
                }
                out.push_str("\n|");
                for (i, lines) in wrapped.iter().enumerate() {
                    out.push(' ');
                    out.push_str(&pad(
                        lines.get(row).map_or("", String::as_str),
                        widths[i],
                        Alignment::AlignLeft,
                    ));
                    out.push_str(" |");
                }
            }
            out.push('\n');
            if *header && !muse {
                out.push_str(&border('=', true));
            } else {
                out.push_str(&base);
            }
        }
        if !caption.is_empty() {
            out.push_str(&format!("\n\n{caption}"));
        }
        Ok(out)
    }
}

fn wrap_cell(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        let doc = layout::hsep(line.split_whitespace().map(literal));
        let line = doc.render(Some(width));
        out.extend(line.split('\n').map(str::to_owned));
    }
    out
}

fn textile_note_spacing(last: Option<&Block>, compact: bool) -> usize {
    match last {
        Some(Block::Plain(_)) => 1,
        Some(Block::Para(_)) if compact => 1,
        Some(Block::CodeBlock(_, code)) if !code.contains("\n\n") => 3,
        Some(Block::BlockQuote(quote))
            if matches!(quote.as_slice(), [Block::Plain(_)] | [Block::Para(_)]) =>
        {
            4
        }
        _ => 2,
    }
}

fn textile_attributes(attr: &Attr) -> String {
    let mut text = String::new();
    if !attr.classes.is_empty() || !attr.id.is_empty() {
        text.push('(');
        text.push_str(&attr.classes.join(" "));
        if !attr.id.is_empty() {
            text.push('#');
            text.push_str(&attr.id);
        }
        text.push(')');
    }
    if let Some(style) = attr.get("style") {
        text.push_str(&format!("{{{style}}}"));
    }
    if let Some(lang) = attr.get("lang") {
        text.push_str(&format!("[{lang}]"));
    }
    text
}

fn textile_native_code(code: &str) -> bool {
    !code.starts_with('\n') && !code.contains("\n\n")
}

fn escape(format: Format, s: &str) -> String {
    if format == Format::Muse && s.contains(['*', '=', '#', '<', '|']) {
        let text = s.replace("</verbatim>", "<</verbatim><verbatim>/verbatim>");
        return format!("<verbatim>{text}</verbatim>");
    }
    let mut out = String::new();
    for c in s.chars() {
        let replacement = match (format, c) {
            (Format::Textile, '<') => "&lt;",
            (Format::Textile, '>') => "&gt;",
            (Format::Textile, '&') => "&amp;",
            (Format::Textile, '*' | '+' | '-' | '@' | '_' | '|') => {
                out.push_str(&format!("&#{};", c as u32));
                continue;
            }
            (Format::Textile | Format::Vimdoc, '’') => "'",
            (Format::Vimdoc, '—') => "---",
            (Format::Vimdoc, '–') => "--",
            (Format::Vimdoc, '…') => "...",
            (Format::Textile, '—') => " -- ",
            (Format::Textile, '–') => " - ",
            (Format::Textile, '…') => "...",
            (Format::Haddock, '/' | '\'' | '`' | '"' | '@' | '<' | '\\') => {
                out.push('\\');
                out.push(c);
                continue;
            }
            _ => {
                out.push(c);
                continue;
            }
        };
        out.push_str(replacement);
    }
    out
}

fn display_width(text: &str) -> usize {
    layout::real_length(text)
}

fn pad(text: &str, width: usize, align: Alignment) -> String {
    let gap = width.saturating_sub(display_width(text));
    let left = match align {
        Alignment::AlignRight => gap,
        Alignment::AlignCenter => gap / 2,
        _ => 0,
    };
    format!("{}{text}{}", " ".repeat(left), " ".repeat(gap - left))
}

fn indent_text(text: &str, width: usize) -> String {
    let prefix = " ".repeat(width);
    text.split('\n')
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn section_number(sections: &[i64]) -> String {
    let mut out = sections
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(".");
    if sections.len() == 1 {
        out.push('.');
    }
    out
}

fn vim_variables(
    doc: &Pandoc,
    vars: &mut randoc_templates::Context,
    state: &mut State<'_, '_>,
) -> Result<()> {
    let text = |name| {
        doc.meta
            .get(name)
            .map(randoc_core::shared::stringify_meta)
            .unwrap_or_default()
    };
    let author = match doc.meta.get("author") {
        Some(MetaValue::MetaList(authors)) => authors
            .iter()
            .map(randoc_core::shared::stringify_meta)
            .collect::<Vec<_>>()
            .join(", "),
        _ => text("author"),
    };
    let title = text("title");
    let title = if !title.is_empty() && !author.is_empty() {
        format!("{title} by {author}")
    } else {
        title
    };
    if !title.is_empty() {
        vars.insert(
            "combined-title",
            Val::text(format!(
                "{}{title}",
                " ".repeat(state.columns.saturating_sub(display_width(&title)) / 2)
            )),
        );
    }
    vars.insert(
        "modeline",
        Val::text(format!(
            "vim:tw={}:sw={}:ts={}:ft=help:norl:et:",
            state.columns, state.options.tab_stop, state.options.tab_stop
        )),
    );
    vars.insert(
        "toc-reminder",
        Val::text(format!(
            "{:>width$}",
            "Type |gO| to see the table of contents.",
            width = state.columns
        )),
    );
    if state.options.table_of_contents {
        let mut lines = Vec::new();
        let base = doc
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Header(level, _, _) => Some(*level),
                _ => None,
            })
            .min()
            .unwrap_or(1);
        let mut sections: Vec<i64> = Vec::new();
        let mut unnumbered = None;
        for block in &doc.blocks {
            let Block::Header(level, attr, xs) = block else {
                continue;
            };
            if *level > state.options.toc_depth {
                continue;
            }
            let id = &attr.id;
            if id.is_empty() {
                continue;
            }
            if unnumbered.is_some_and(|ancestor| ancestor >= *level) {
                unnumbered = None;
            }
            if attr.has_class("unnumbered") {
                unnumbered = Some(*level);
            }
            let level = level.saturating_sub(base).clamp(0, 5) as usize + 1;
            let base_indent = (level - 1) * 2;
            let indent = " ".repeat(base_indent);
            let label = stringify(xs);
            let prefix = if state.options.number_sections && unnumbered.is_none() {
                sections.resize(level, 0);
                sections[level - 1] = sections[level - 1].saturating_add(1);
                let extra = if level >= 3 { 2 } else { 0 };
                format!(
                    "{}{} ",
                    " ".repeat(base_indent + extra),
                    section_number(&sections)
                )
            } else {
                indent
            };
            let label = format!("{prefix}{label}");
            let target = format!("|{}{}|", state.vim_prefix, id);
            let available = state
                .columns
                .saturating_add(1)
                .saturating_sub(display_width(&target));
            let line = if display_width(&label) + 4 > available {
                let take = available.saturating_sub(3).max(prefix.len());
                let mut short = label.chars().take(take).collect::<String>();
                if short.ends_with(' ') && short.len() > prefix.len() {
                    short.pop();
                    short.push('.');
                }
                format!("{short}... {target}")
            } else {
                format!(
                    "{label} {} {target}",
                    ".".repeat(available - display_width(&label) - 1)
                )
            };
            lines.push(line);
        }
        if !lines.is_empty() {
            vars.insert("toc", Val::text(lines.join("\n")));
        }
    }
    Ok(())
}

fn dimension_pixels(dimension: &randoc_html::shared::Dimension, dpi: i64) -> Option<String> {
    use randoc_html::shared::{Dimension, Unit};
    let pixels = match dimension {
        Dimension::Pixel(value) => return Some(value.clone()),
        Dimension::Length(value, unit) => match unit {
            Unit::Inch => value * dpi as f64,
            Unit::Centimeter => value / 2.54 * dpi as f64,
            Unit::Millimeter => value / 25.4 * dpi as f64,
            Unit::Point => value / 72.0 * dpi as f64,
            Unit::Pica => value / 6.0 * dpi as f64,
            Unit::Em => value * (16.5 / 96.0) * dpi as f64,
            Unit::Percent => return None,
        },
    };
    Some(format!("{}", pixels.floor()))
}

enum Content<'a> {
    Blocks(&'a [Block]),
    Inlines(&'a [Inline]),
    Meta(&'a MetaValue),
}

fn attr_size(attr: &Attr) -> usize {
    attr.id
        .len()
        .saturating_add(attr.classes.iter().map(String::len).sum::<usize>())
        .saturating_add(
            attr.attributes
                .iter()
                .map(|(key, value)| key.len() + value.len())
                .sum::<usize>(),
        )
}

pub(crate) fn document_budget(doc: &Pandoc, context: &Context) -> Result<usize> {
    use Content::*;
    let mut todo = vec![(0, Blocks(&doc.blocks))];
    todo.extend(doc.meta.0.values().map(|value| (0, Meta(value))));
    let mut size = 0usize;
    while let Some((depth, node)) = todo.pop() {
        check_depth(depth, context.limits.max_nesting_depth)?;
        let child = depth + 1;
        size = size.saturating_add(match node {
            Blocks(blocks) => {
                let mut bytes = blocks.len();
                for block in blocks {
                    bytes = bytes.saturating_add(match block {
                        Block::Plain(xs) | Block::Para(xs) => {
                            todo.push((depth, Inlines(xs)));
                            0
                        }
                        Block::Header(_, attr, xs) => {
                            todo.push((depth, Inlines(xs)));
                            attr_size(attr)
                        }
                        Block::LineBlock(lines) => {
                            todo.extend(lines.iter().map(|line| (depth, Inlines(line))));
                            0
                        }
                        Block::HorizontalRule => 0,
                        Block::CodeBlock(attr, text) => attr_size(attr).saturating_add(text.len()),
                        Block::RawBlock(format, text) => {
                            format.as_str().len().saturating_add(text.len())
                        }
                        Block::BlockQuote(contents) => {
                            todo.push((child, Blocks(contents)));
                            0
                        }
                        Block::Div(attr, contents) => {
                            todo.push((child, Blocks(contents)));
                            attr_size(attr)
                        }
                        Block::Figure(attr, caption, contents) => {
                            todo.push((child, Blocks(contents)));
                            todo.push((child, Blocks(&caption.long)));
                            if let Some(short) = &caption.short {
                                todo.push((child, Inlines(short)));
                            }
                            attr_size(attr)
                        }
                        Block::BulletList(items) | Block::OrderedList(_, items) => {
                            todo.extend(items.iter().map(|item| (child, Blocks(item))));
                            items.len()
                        }
                        Block::DefinitionList(items) => {
                            for (term, definitions) in items {
                                todo.push((child, Inlines(term)));
                                todo.extend(definitions.iter().map(|item| (child, Blocks(item))));
                            }
                            items.len()
                        }
                        Block::Table(table) => {
                            let mut bytes =
                                attr_size(&table.attr).saturating_add(table.col_specs.len());
                            todo.push((child, Blocks(&table.caption.long)));
                            if let Some(short) = &table.caption.short {
                                todo.push((child, Inlines(short)));
                            }
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
                                bytes = bytes
                                    .saturating_add(attr_size(&row.attr))
                                    .saturating_add(row.cells.len());
                                for cell in &row.cells {
                                    bytes = bytes.saturating_add(attr_size(&cell.attr));
                                    todo.push((child, Blocks(&cell.content)));
                                }
                            }
                            bytes
                        }
                    });
                }
                bytes
            }
            Inlines(xs) => {
                let mut bytes = xs.len();
                for x in xs {
                    bytes = bytes.saturating_add(match x {
                        Inline::Str(text) | Inline::Math(_, text) => text.len(),
                        Inline::RawInline(format, text) => {
                            format.as_str().len().saturating_add(text.len())
                        }
                        Inline::Code(attr, text) => attr_size(attr).saturating_add(text.len()),
                        Inline::Emph(xs)
                        | Inline::Strong(xs)
                        | Inline::Underline(xs)
                        | Inline::Strikeout(xs)
                        | Inline::Subscript(xs)
                        | Inline::Superscript(xs)
                        | Inline::SmallCaps(xs)
                        | Inline::Quoted(_, xs) => {
                            todo.push((child, Inlines(xs)));
                            0
                        }
                        Inline::Cite(citations, xs) => {
                            todo.push((child, Inlines(xs)));
                            for citation in citations {
                                todo.push((child, Inlines(&citation.prefix)));
                                todo.push((child, Inlines(&citation.suffix)));
                                bytes = bytes.saturating_add(citation.id.len());
                            }
                            0
                        }
                        Inline::Span(attr, xs) => {
                            todo.push((child, Inlines(xs)));
                            attr_size(attr)
                        }
                        Inline::Link(attr, xs, target) | Inline::Image(attr, xs, target) => {
                            todo.push((child, Inlines(xs)));
                            attr_size(attr)
                                .saturating_add(target.url.len())
                                .saturating_add(target.title.len())
                        }
                        Inline::Note(blocks) => {
                            todo.push((child, Blocks(blocks)));
                            0
                        }
                        Inline::Space | Inline::SoftBreak | Inline::LineBreak => 0,
                    });
                }
                bytes
            }
            Meta(value) => match value {
                MetaValue::MetaMap(values) => {
                    todo.extend(values.values().map(|value| (child, Meta(value))));
                    values.keys().map(String::len).sum()
                }
                MetaValue::MetaList(values) => {
                    todo.extend(values.iter().map(|value| (child, Meta(value))));
                    values.len()
                }
                MetaValue::MetaBool(_) => 1,
                MetaValue::MetaString(text) => text.len(),
                MetaValue::MetaInlines(xs) => {
                    todo.push((depth, Inlines(xs)));
                    0
                }
                MetaValue::MetaBlocks(blocks) => {
                    todo.push((depth, Blocks(blocks)));
                    0
                }
            },
        });
        if size as u64 > context.limits.max_input_size {
            return Err(Error::App("The input is too large.".into()));
        }
    }
    Ok(expansion_budget(size, context.limits.max_input_size))
}

fn vim_help(input: &str) -> Option<&str> {
    let topic = input
        .strip_prefix(":h ")
        .or_else(|| input.strip_prefix(":help "))?
        .trim();
    (!topic.is_empty() && !topic.contains(char::is_whitespace)).then_some(topic)
}

fn vim_url(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let path = rest
        .strip_prefix("vimhelp.org/")
        .or_else(|| rest.strip_prefix("neo.vimhelp.org/"))
        .or_else(|| rest.strip_prefix("neovim.io/doc/user/"))?;
    let (file, topic) = path.split_once('#').unwrap_or((path, ""));
    let file = file.strip_suffix(".html")?;
    if !topic.is_empty() {
        Some(topic.into())
    } else if file.ends_with(".txt") {
        Some(file.into())
    } else {
        Some(format!("{file}.txt"))
    }
}

fn vim_heading_case(text: &str) -> String {
    let mut keep_case = false;
    let mut result = String::new();
    for ch in text.chars() {
        if ch == '`' {
            keep_case = !keep_case;
            result.push(ch);
        } else if keep_case {
            result.push(ch);
        } else {
            result.extend(ch.to_uppercase());
        }
    }
    result
}

fn list_marker(attr: randoc_ast::ListAttributes, value: i64, format: Format) -> String {
    use randoc_ast::{ListNumberDelim as Delim, ListNumberStyle as Style};
    let label = match (format, attr.style) {
        (Format::Muse | Format::Vimdoc, Style::LowerAlpha | Style::UpperAlpha)
            if (1..=26).contains(&value) =>
        {
            let first = if attr.style == Style::LowerAlpha {
                b'a'
            } else {
                b'A'
            };
            char::from(first + value as u8 - 1).to_string()
        }
        (Format::Muse | Format::Vimdoc, Style::LowerRoman | Style::UpperRoman)
            if (1..=3999).contains(&value) =>
        {
            let mut value = value;
            let mut roman = String::new();
            for (number, letters) in [
                (1000, "M"),
                (900, "CM"),
                (500, "D"),
                (400, "CD"),
                (100, "C"),
                (90, "XC"),
                (50, "L"),
                (40, "XL"),
                (10, "X"),
                (9, "IX"),
                (5, "V"),
                (4, "IV"),
                (1, "I"),
            ] {
                while value >= number {
                    roman.push_str(letters);
                    value -= number;
                }
            }
            if attr.style == Style::LowerRoman {
                roman.to_ascii_lowercase()
            } else {
                roman
            }
        }
        _ => value.to_string(),
    };
    match (format, attr.delim) {
        (Format::Muse, _) => format!("{label}."),
        (_, Delim::TwoParens) => format!("({label})"),
        (_, Delim::OneParen) => format!("{label})"),
        _ => format!("{label}."),
    }
}

fn complex_list(items: &[Vec<Block>]) -> bool {
    items.iter().any(|item| {
        item.iter()
            .filter(|block| !matches!(block, Block::BulletList(_) | Block::OrderedList(..)))
            .take(2)
            .count()
            > 1
    })
}

fn native_textile_table(table: &Table) -> bool {
    table.caption.long.is_empty()
        && table.attr.is_empty()
        && table.head.rows.len() <= 1
        && table.foot.rows.is_empty()
        && table.bodies.len() <= 1
        && table
            .col_specs
            .iter()
            .all(|col| col.width == ColWidth::ColWidthDefault)
        && table
            .head
            .rows
            .iter()
            .chain(
                table
                    .bodies
                    .iter()
                    .flat_map(|body| body.head.iter().chain(&body.body)),
            )
            .all(|row| {
                row.attr.is_empty()
                    && row.cells.iter().all(|cell| {
                        cell.attr.is_empty()
                            && cell.row_span == 1
                            && cell.col_span == 1
                            && matches!(
                                cell.content.as_slice(),
                                [] | [Block::Plain(_)] | [Block::Para(_)]
                            )
                    })
            })
}

fn relative_width(fraction: f64, columns: usize) -> usize {
    if !fraction.is_finite() || fraction <= 0.0 {
        return 1;
    }
    ((fraction * columns as f64)
        .floor()
        .clamp(1.0, columns as f64) as usize)
        .saturating_sub(3)
        .max(1)
}

fn needs_textile_list_style(attr: randoc_ast::ListAttributes) -> bool {
    use randoc_ast::ListNumberStyle as Style;
    attr.start != 1 || !matches!(attr.style, Style::DefaultStyle | Style::Decimal)
}

fn ends_plain(mut block: &Block) -> bool {
    while let Block::Div(_, blocks) = block {
        let Some(last) = blocks.last() else {
            return false;
        };
        block = last;
    }
    matches!(block, Block::Plain(_))
        || matches!(block, Block::Figure(_, caption, _) if matches!(caption.long.last(), Some(Block::Plain(_))))
}

fn html_attribute(text: &mut String, name: &str, value: &str) {
    if !name.is_empty()
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b":_-".contains(&c))
    {
        text.push(' ');
        text.push_str(name);
        text.push_str("=\"");
        text.push_str(&randoc_html::shared::escape_attribute(value, false));
        text.push('"');
    }
}
