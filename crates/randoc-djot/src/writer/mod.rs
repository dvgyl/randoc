mod tables;

use randoc_ast::{
    Attr, Block, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, MathType, Pandoc,
    QuoteType, Target,
};
use randoc_core::{
    Context, Result, WrapOption, WriterOptions,
    shared::{meta_context::meta_to_context, stringify},
};
use randoc_layout::{self as layout, Doc};
use randoc_templates::Val;

use crate::{depth_error, identifier, size_error};

pub(crate) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let mut render = Render::new(options, context);
    let body = render.blocks(&doc.blocks, 0)?;
    let mut notes = Vec::new();
    let mut index = 0;
    while let Some(&(blocks, depth)) = render.notes.get(index) {
        index += 1;
        let label = format!("[^{index}]: ");
        notes.push(layout::hang(
            4,
            layout::literal(label),
            render.blocks(blocks, depth)?,
        ));
    }
    let references = layout::vcat(std::mem::take(&mut render.references));
    let body = layout::vsep([body, references, layout::vsep(notes)]);
    let width = if options.wrap_text == WrapOption::WrapAuto {
        usize::try_from(options.columns)
            .ok()
            .filter(|&width| width > 0)
    } else {
        None
    };
    let doc = if let Some(template) = &options.template {
        let mut fields = meta_to_context(
            &doc.meta,
            &options.variables,
            context.limits.max_nesting_depth,
            &mut |blocks| Render::new(options, context).blocks(blocks, 0),
            &mut |inlines| Render::new(options, context).inlines(inlines, 0),
        )?;
        if fields.get("body").is_none() {
            fields.insert("body", Val::Simple(layout::literal(body.render(width))));
        }
        let budget = render.budget;
        return template
            .render_limited(&fields, budget, budget)
            .ok_or_else(size_error);
    } else {
        body
    };
    if doc.estimated_size() > render.budget {
        return Err(size_error());
    }
    let mut text = doc.render(width);
    if !text.is_empty() {
        text.push('\n');
    }
    if text.len() > render.budget {
        return Err(size_error());
    }
    Ok(text)
}

struct Render<'a> {
    options: &'a WriterOptions,
    limit: usize,
    table_cells: usize,
    budget: usize,
    notes: Vec<(&'a [Block], usize)>,
    references: Vec<Doc>,
    fence: usize,
}

impl<'a> Render<'a> {
    fn new(options: &'a WriterOptions, context: &Context) -> Self {
        Self {
            options,
            limit: context.limits.max_nesting_depth,
            table_cells: context.limits.max_table_cells,
            budget: usize::try_from(
                context
                    .limits
                    .max_input_size
                    .min(context.limits.max_archive_size),
            )
            .unwrap_or(usize::MAX),
            notes: Vec::new(),
            references: Vec::new(),
            fence: 2,
        }
    }

    fn depth(&self, depth: usize) -> Result<()> {
        if depth > self.limit {
            Err(depth_error())
        } else {
            Ok(())
        }
    }

    fn blocks(&mut self, blocks: &'a [Block], depth: usize) -> Result<Doc> {
        self.depth(depth)?;
        let mut out = Doc::default();
        let mut previous_bullet = false;
        let mut bullet = '-';
        for block in blocks {
            if previous_bullet {
                bullet = if bullet == '-' { '+' } else { '-' };
            } else {
                bullet = '-';
            }
            out = out.above_blank(self.block(block, depth, bullet)?);
            previous_bullet = matches!(block, Block::BulletList(_));
        }
        Ok(out)
    }

    fn block(&mut self, block: &'a Block, depth: usize, bullet: char) -> Result<Doc> {
        self.depth(depth)?;
        let next = depth + 1;
        Ok(match block {
            Block::Plain(nodes) | Block::Para(nodes) => self.inlines(nodes, next)?,
            Block::LineBlock(lines) => layout::vcat(
                lines
                    .iter()
                    .map(|line| self.inlines(line, next))
                    .collect::<Result<Vec<_>>>()?,
            ),
            Block::CodeBlock(attr, content) => block_attr(attr).above(code_block(content, "")),
            Block::RawBlock(format, content) => code_block(content, &format!("={}", format.0)),
            Block::Header(level, attr, nodes) => self.heading(*level, attr, nodes, next)?,
            Block::HorizontalRule => layout::literal("* * * *"),
            Block::BlockQuote(blocks) => layout::prefixed("> ", self.blocks(blocks, next)?),
            Block::BulletList(items) => self.list(items, None, bullet, next)?,
            Block::OrderedList(attr, items) => self.list(items, Some(*attr), bullet, next)?,
            Block::DefinitionList(items) => {
                let tight = items
                    .iter()
                    .flat_map(|(_, defs)| defs)
                    .all(|def| !matches!(def.first(), Some(Block::Para(_))));
                let mut out = Vec::new();
                for (term, defs) in items {
                    let term = layout::hang(2, layout::literal(": "), self.inlines(term, next)?);
                    let defs = defs
                        .iter()
                        .map(|def| self.blocks(def, next))
                        .collect::<Result<Vec<_>>>()?;
                    out.push(term.above_blank(layout::nest(2, layout::vsep(defs))));
                }
                if tight {
                    layout::vcat(out)
                } else {
                    layout::vsep(out)
                }
            }
            Block::Table(table) => self.table(table, next)?,
            Block::Div(attr, children) => {
                if attr.get("wrapper") == Some("1") && children.len() == 1 {
                    let mut attr = attr.clone();
                    attr.attributes.retain(|(k, _)| k != "wrapper");
                    block_attr(&attr).above(self.blocks(children, next)?)
                } else if attr.has_class("section")
                    && let Some((Block::Header(level, head_attr, nodes), rest)) =
                        children.split_first()
                    && !rest
                        .iter()
                        .any(|block| matches!(block, Block::Header(child, ..) if child <= level))
                {
                    let mut attr = attr.clone();
                    attr.classes.retain(|class| class != "section");
                    crate::attrs::merge(&mut attr, head_attr.clone());
                    let header = self.heading(*level, &attr, nodes, next)?;
                    header.above_blank(self.blocks(rest, next)?)
                } else {
                    let saved = self.fence;
                    self.fence = 2;
                    let contents = self.blocks(children, next)?;
                    self.div(attr, contents, saved)
                }
            }
            Block::Figure(attr, caption, children) => {
                let saved = self.fence;
                self.fence = 2;
                let body = self.blocks(children, next)?;
                let contents = if caption.long.is_empty() {
                    body
                } else {
                    let saved = self.fence;
                    self.fence = 2;
                    let caption = self.blocks(&caption.long, next)?;
                    let caption = self.div(
                        &Attr::new("", vec!["caption".into()], vec![]),
                        caption,
                        saved,
                    );
                    body.above_blank(caption)
                };
                self.div(attr, contents, saved)
            }
        })
    }

    fn div(&mut self, attr: &Attr, contents: Doc, old: usize) -> Doc {
        let width = self.fence + 1;
        self.fence = old.saturating_add(width.saturating_sub(2));
        let fence = layout::literal(":".repeat(width));
        block_attr(attr)
            .above(fence.clone())
            .above(contents)
            .above_blank(fence)
    }

    fn heading(
        &mut self,
        level: i64,
        attr: &Attr,
        nodes: &'a [Inline],
        depth: usize,
    ) -> Result<Doc> {
        let mut attr = attr.clone();
        if attr.id == identifier(&stringify(nodes)) {
            attr.id.clear();
        }
        let level = usize::try_from(level.max(1)).map_err(|_| size_error())?;
        if level > self.budget {
            return Err(size_error());
        }
        let body = self.inlines(nodes, depth)?;
        Ok(block_attr(&attr)
            .above(layout::literal(format!("{} ", "#".repeat(level))) + layout::nowrap(body)))
    }

    fn list(
        &mut self,
        items: &'a [Vec<Block>],
        attrs: Option<ListAttributes>,
        bullet: char,
        depth: usize,
    ) -> Result<Doc> {
        let mut rendered = Vec::new();
        let tight = items
            .iter()
            .all(|item| !item.iter().any(|block| matches!(block, Block::Para(_))));
        for (i, item) in items.iter().enumerate() {
            let mark = attrs.map_or_else(
                || bullet.to_string(),
                |attr| number_marker(attr, attr.start.saturating_add(i as i64)),
            );
            let start = format!("{mark} ");
            let contents = self.blocks(item, depth)?;
            rendered.push(layout::hang(start.len(), layout::literal(start), contents));
        }
        Ok(if tight {
            layout::vcat(rendered)
        } else {
            layout::vsep(rendered)
        })
    }

    fn inlines(&mut self, nodes: &'a [Inline], depth: usize) -> Result<Doc> {
        self.depth(depth)?;
        let mut docs = Vec::with_capacity(nodes.len());
        let mut after_markup = false;
        let mut groups = nodes
            .chunk_by(|a, b| matches!((a, b), (Inline::Str(_), Inline::Str(_))))
            .peekable();
        while let Some(group) = groups.next() {
            let Some(node) = group.first() else { continue };
            let trailing_dollar = !matches!(
                groups.peek().and_then(|group| group.first()),
                Some(Inline::Space | Inline::SoftBreak | Inline::LineBreak)
            );
            let doc = if group.len() > 1 {
                let text = group
                    .iter()
                    .filter_map(|node| match node {
                        Inline::Str(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>();
                layout::literal(escape(&text, after_markup, trailing_dollar))
            } else {
                self.inline(node, depth, after_markup, trailing_dollar)?
            };
            docs.push(doc);
            after_markup = matches!(
                node,
                Inline::Code(..) | Inline::Link(..) | Inline::Image(..)
            );
        }
        Ok(layout::hcat(docs))
    }

    fn inline(
        &mut self,
        node: &'a Inline,
        depth: usize,
        after_markup: bool,
        trailing_dollar: bool,
    ) -> Result<Doc> {
        let next = depth + 1;
        let wrapped = |start: &str, end: &str, text: Doc| {
            layout::literal(start) + text + layout::literal(end)
        };
        Ok(match node {
            Inline::Str(text) => layout::literal(escape(text, after_markup, trailing_dollar)),
            Inline::Space => layout::space(),
            Inline::SoftBreak if self.options.wrap_text == WrapOption::WrapPreserve => layout::cr(),
            Inline::SoftBreak => layout::space(),
            Inline::LineBreak => layout::literal("\\") + layout::cr(),
            Inline::Emph(nodes) => wrapped("_", "_", self.inlines(nodes, next)?),
            Inline::Strong(nodes) => wrapped("*", "*", self.inlines(nodes, next)?),
            Inline::Strikeout(nodes) => wrapped("{-", "-}", self.inlines(nodes, next)?),
            Inline::Superscript(nodes) => wrapped("^", "^", self.inlines(nodes, next)?),
            Inline::Subscript(nodes) => wrapped("~", "~", self.inlines(nodes, next)?),
            Inline::SmallCaps(nodes) => wrapped("[", "]{.smallcaps}", self.inlines(nodes, next)?),
            Inline::Underline(nodes) => wrapped("[", "]{.underline}", self.inlines(nodes, next)?),
            Inline::Quoted(kind, nodes) => {
                let q = if *kind == QuoteType::SingleQuote {
                    "'"
                } else {
                    "\""
                };
                wrapped(q, q, self.inlines(nodes, next)?)
            }
            Inline::Cite(_, nodes) => self.inlines(nodes, next)?,
            Inline::Code(attr, text) => code_inline(text) + layout::literal(attributes(attr)),
            Inline::Math(kind, text) => {
                layout::literal(if *kind == MathType::InlineMath {
                    "$"
                } else {
                    "$$"
                }) + code_inline(text)
            }
            Inline::RawInline(format, text) => {
                code_inline(text) + layout::literal(format!("{{={}}}", format.0))
            }
            Inline::Span(attr, nodes) => {
                let contents = self.inlines(nodes, next)?;
                if attr.get("wrapper") == Some("1") {
                    let mut attr = attr.clone();
                    attr.attributes.retain(|(key, _)| key != "wrapper");
                    contents + layout::literal(attributes(&attr))
                } else if attr.id.is_empty()
                    && attr.attributes.is_empty()
                    && attr.classes.as_slice() == ["mark"]
                {
                    wrapped("{=", "=}", contents)
                } else if attr.is_empty() {
                    contents
                } else {
                    wrapped("[", &format!("]{}", attributes(attr)), contents)
                }
            }
            Inline::Link(attr, nodes, target) => self.link(false, attr, nodes, target, next)?,
            Inline::Image(attr, nodes, target) => self.link(true, attr, nodes, target, next)?,
            Inline::Note(blocks) => {
                self.depth(next)?;
                self.notes.push((blocks, next));
                layout::literal(format!("[^{}]", self.notes.len()))
            }
        })
    }

    fn link(
        &mut self,
        image: bool,
        attr: &Attr,
        nodes: &'a [Inline],
        target: &Target,
        depth: usize,
    ) -> Result<Doc> {
        let mut attr = attr.clone();
        let url = target
            .url
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        if !image && attr.has_class("uri") && stringify(nodes) == target.url {
            attr.classes.retain(|class| class != "uri");
            return Ok(layout::literal(format!("<{}>", target.url))
                + link_attributes(&attr, &target.title));
        }
        if !image
            && attr.has_class("email")
            && target
                .url
                .strip_prefix("mailto:")
                .is_some_and(|mail| stringify(nodes) == mail)
        {
            attr.classes.retain(|class| class != "email");
            return Ok(layout::literal(format!("<{}>", &target.url[7..]))
                + link_attributes(&attr, &target.title));
        }
        let attr = link_attributes(&attr, &target.title);
        let start = if image { "![" } else { "[" };
        let label = self.inlines(nodes, depth)?;
        let tail = if self.options.reference_links {
            let index = self.references.len() + 1;
            self.references
                .push(layout::literal(format!("[{index}]: {url}")));
            format!("][{index}]")
        } else {
            format!("]({url})")
        };
        Ok(layout::literal(start) + label + layout::literal(tail) + attr)
    }
}

fn block_attr(attr: &Attr) -> Doc {
    layout::literal(attributes(attr))
}

fn link_attributes(attr: &Attr, title: &str) -> Doc {
    let mut parts = attribute_parts(attr);
    if !title.is_empty() {
        parts.insert(0, format!("title=\"{}\"", escape_attribute(title)));
    }
    if parts.is_empty() {
        layout::empty()
    } else {
        layout::literal("{")
            + layout::hsep(parts.into_iter().map(layout::literal))
            + layout::literal("}")
    }
}

fn attributes(attr: &Attr) -> String {
    if attr.is_empty() {
        String::new()
    } else {
        format!("{{{}}}", attribute_parts(attr).join(" "))
    }
}

fn attribute_parts(attr: &Attr) -> Vec<String> {
    let mut parts = Vec::new();
    if !attr.id.is_empty() {
        parts.push(format!("#{}", attr.id));
    }
    parts.extend(attr.classes.iter().map(|class| format!(".{class}")));
    parts.extend(
        attr.attributes
            .iter()
            .map(|(k, v)| format!("{k}=\"{}\"", escape_attribute(v))),
    );
    parts
}

fn escape_attribute(text: &str) -> String {
    let mut result = String::new();
    for c in text.chars() {
        if matches!(c, '\\' | '"' | ':') {
            result.push('\\');
        }
        result.push(c);
    }
    result
}

fn escape(text: &str, after_markup: bool, trailing_dollar: bool) -> String {
    let mut result = String::new();
    let mut after_bracket = after_markup;
    for (offset, c) in text.char_indices() {
        let rest = &text[offset + c.len_utf8()..];
        match c {
            '…' => result.push_str("..."),
            '–' => result.push_str("--"),
            '—' => result.push_str("---"),
            '’' => result.push('\''),
            '‘' => result.push('\''),
            '“' | '”' => result.push('"'),
            '\\' | '`' | '*' | '_' | '^' | '~' | '[' | ']' | '{' | '}' | '<' | '>' | '|' | '\''
            | '"' => {
                result.push('\\');
                result.push(c);
            }
            '!' if rest.starts_with('[') || text == "!" => result.push_str("\\!"),
            '$' if (trailing_dollar && rest.is_empty()) || rest.starts_with('`') => {
                result.push_str("\\$")
            }
            '-' if rest.starts_with('-') => result.push_str("\\-"),
            '.' if rest.starts_with("..") => result.push_str("\\."),
            ':' if after_bracket || rest.chars().next().is_some_and(|c| !c.is_whitespace()) => {
                result.push_str("\\:");
            }
            _ => result.push(c),
        }
        after_bracket = c == ']';
    }
    result
}

fn max_ticks(text: &str) -> usize {
    let mut longest = 0;
    let mut ticks = 0;
    for b in text.bytes() {
        if b == b'`' {
            ticks += 1;
            longest = longest.max(ticks);
        } else {
            ticks = 0;
        }
    }
    longest
}

fn code_inline(text: &str) -> Doc {
    let fence = "`".repeat(max_ticks(text) + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') {
        " "
    } else {
        ""
    };
    layout::literal(format!("{fence}{pad}{text}{pad}{fence}"))
}

fn code_block(text: &str, info: &str) -> Doc {
    let fence = "`".repeat((max_ticks(text) + 1).max(3));
    layout::literal(format!(
        "{fence}{info}\n{}{fence}",
        if text.is_empty() || text.ends_with('\n') {
            text.into()
        } else {
            format!("{text}\n")
        }
    ))
}

fn number_marker(attr: ListAttributes, number: i64) -> String {
    let value = match attr.style {
        ListNumberStyle::LowerAlpha | ListNumberStyle::UpperAlpha if (1..=26).contains(&number) => {
            let a = if attr.style == ListNumberStyle::LowerAlpha {
                b'a'
            } else {
                b'A'
            };
            char::from(a + number as u8 - 1).to_string()
        }
        ListNumberStyle::LowerRoman | ListNumberStyle::UpperRoman
            if (1..=3999).contains(&number) =>
        {
            let mut number = number;
            let mut roman = String::new();
            for (value, symbol) in [
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
                while number >= value {
                    roman.push_str(symbol);
                    number -= value;
                }
            }
            if attr.style == ListNumberStyle::LowerRoman {
                roman.make_ascii_lowercase();
            }
            roman
        }
        _ => number.to_string(),
    };
    match attr.delim {
        ListNumberDelim::OneParen => format!("{value})"),
        ListNumberDelim::TwoParens => format!("({value})"),
        _ => format!("{value}."),
    }
}
