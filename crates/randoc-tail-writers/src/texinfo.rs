use std::cell::RefCell;
use std::collections::HashMap;

use randoc_ast::{Block, Inline, ListNumberStyle, MathType, Pandoc, QuoteType, Table};
use randoc_core::shared::{sections::make_sections, stringify_meta};
use randoc_core::{Context, Result, WrapOption, WriterOptions};
use randoc_layout::{Doc, blankline, chomp, cr, hcat, inside, literal, space, text, vcat};
use randoc_templates::Val;

pub(super) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let depth = context.limits.max_nesting_depth;
    let sections = make_sections(doc.blocks.clone(), false, None, depth)?;
    let mut render = Renderer {
        options,
        names: HashMap::new(),
        strikeout: false,
    };

    let mut nodes = Vec::new();
    let mut roots = Vec::new();
    let mut pending = vec![(&sections[..], None)];
    while let Some((blocks, parent)) = pending.pop() {
        for block in blocks {
            if let Some((level, id, title, content)) = section(block) {
                let title = render.inlines(title);
                let base = if (1..=4).contains(&level) {
                    super::check_layout(&title, context.limits.max_input_size)?;
                    node_name(&title.render(None))
                } else {
                    String::new()
                };
                let index = nodes.len();
                nodes.push(Node {
                    id,
                    name: String::new(),
                    base,
                    level,
                    title,
                    content,
                    children: Vec::new(),
                });
                pending.push((content, Some(index)));
                if let Some(parent) = parent {
                    let parent_node: &mut Node<'_> = &mut nodes[parent];
                    parent_node.children.push(index);
                } else {
                    roots.push(index);
                }
            }
        }
    }

    let mut counters = HashMap::new();
    let mut pending: Vec<_> = roots.iter().rev().copied().collect();
    let mut order = Vec::new();
    while let Some(index) = pending.pop() {
        let node = &mut nodes[index];
        pending.extend(node.children.iter().rev().copied());
        order.push(index);
        if !(1..=4).contains(&node.level) {
            continue;
        }
        let count = counters.entry(node.base.clone()).or_insert(0usize);
        *count += 1;
        node.name = if *count == 1 {
            node.base.clone()
        } else {
            format!("{} {count}", node.base)
        };
        render.names.insert(node.id.to_owned(), node.name.clone());
    }

    let title = doc
        .meta
        .get("title")
        .map_or_else(|| "Top".into(), stringify_meta);
    let mut parts = vec![
        literal(format!("@node Top\n@top {}", escape(&title))),
        blankline(),
    ];
    let preamble = render.blocks(&sections);
    if !preamble.is_empty() {
        parts.push(preamble);
        parts.push(blankline());
    }
    parts.push(menu(&nodes, &roots, 1));
    for index in order {
        let node = &nodes[index];
        if !(1..=4).contains(&node.level) {
            parts.push(node.title.clone() + cr());
            parts.push(render.blocks(node.content));
            continue;
        }
        let command = ["chapter", "section", "subsection", "subsubsection"]
            [usize::try_from(node.level - 1).unwrap_or_default()];
        parts.push(blankline());
        parts.push(literal(format!("@node {}\n@{command} ", node.name)) + node.title.clone());
        parts.push(cr());
        let body = render.blocks(node.content);
        if !body.is_empty() {
            parts.push(body);
            parts.push(blankline());
        }
        parts.push(menu(&nodes, &node.children, node.level + 1));
    }
    let body = chomp(hcat(parts));
    super::check_layout(&body, context.limits.max_input_size)?;
    let strikeout = render.strikeout;
    let render = RefCell::new(render);
    super::render_template(
        doc,
        options,
        context,
        body,
        &mut |blocks| {
            let body = chomp(render.borrow_mut().blocks(blocks));
            super::check_layout(&body, context.limits.max_input_size)?;
            Ok(body)
        },
        &mut |items| {
            let body = render.borrow_mut().inlines(items);
            super::check_layout(&body, context.limits.max_input_size)?;
            Ok(body)
        },
        [
            ("toc", Val::Bool(options.table_of_contents)),
            ("strikeout", Val::Bool(strikeout)),
            ("titlepage", Val::Bool(doc.meta.get("title").is_some())),
        ],
    )
}

struct Node<'a> {
    id: &'a str,
    name: String,
    base: String,
    level: i64,
    title: Doc,
    content: &'a [Block],
    children: Vec<usize>,
}

fn section(block: &Block) -> Option<(i64, &str, &[Inline], &[Block])> {
    if let Block::Div(attr, blocks) = block
        && attr.classes.first().is_some_and(|class| class == "section")
        && let Some((Block::Header(level, _, title), rest)) = blocks.split_first()
    {
        Some((*level, &attr.id, title, rest))
    } else {
        None
    }
}

fn node_name(title: &str) -> String {
    let mut level = 0usize;
    let mut out = String::new();
    for c in title.chars() {
        match c {
            '{' => level += 1,
            '}' => level = level.saturating_sub(1),
            ',' | ':' | '.' | '(' | ')' if level == 0 => continue,
            _ => {}
        }
        out.push(c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn menu(nodes: &[Node<'_>], entries: &[usize], level: i64) -> Doc {
    let entries: Vec<_> = entries
        .iter()
        .filter_map(|&i| {
            let item = &nodes[i];
            (level <= 4 && item.level == level).then(|| {
                if item.name == item.base {
                    format!("* {}::", item.base)
                } else {
                    format!("* {}: {}.", item.base, item.name)
                }
            })
        })
        .collect();
    if entries.is_empty() {
        Doc::default()
    } else {
        blankline() + literal(format!("@menu\n{}\n@end menu", entries.join("\n"))) + blankline()
    }
}

fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c == '\u{a0}' {
            out.push_str("@ ");
            continue;
        }
        let c = match c {
            '’' => '\'',
            '—' => {
                out.push_str("---");
                continue;
            }
            '–' => {
                out.push_str("--");
                continue;
            }
            '…' => {
                out.push_str("@dots{}");
                continue;
            }
            _ => c,
        };
        if matches!(c, '@' | '{' | '}') {
            out.push('@');
        }
        out.push(c);
    }
    out
}

fn argument(text: &str) -> String {
    text.replace(',', "@comma{}")
}

struct Renderer<'a> {
    options: &'a WriterOptions,
    names: HashMap<String, String>,
    strikeout: bool,
}

impl Renderer<'_> {
    fn blocks(&mut self, blocks: &[Block]) -> Doc {
        hcat(
            blocks
                .iter()
                .filter(|block| section(block).is_none())
                .map(|block| self.block(block)),
        )
    }

    fn block(&mut self, block: &Block) -> Doc {
        match block {
            Block::Plain(items) => self.inlines(items) + cr(),
            Block::Para(items) if is_example(items) => {
                let lines = items.iter().filter_map(|inline| match inline {
                    Inline::Code(_, text) => Some(literal(text)),
                    _ => None,
                });
                text("@example") + cr() + vcat(lines) + cr() + text("@end example") + blankline()
            }
            Block::Para(items) => self.inlines(items) + blankline(),
            Block::LineBlock(lines) => {
                vcat(lines.iter().map(|line| self.inlines(line))) + blankline()
            }
            Block::Header(_, _, items) => self.inlines(items) + cr(),
            Block::CodeBlock(_, code) => {
                let code = code.strip_suffix('\n').unwrap_or(code);
                literal(format!("@verbatim\n{code}\n@end verbatim")) + blankline()
            }
            Block::RawBlock(format, raw) if format == "texinfo" => literal(raw.clone()) + cr(),
            Block::RawBlock(format, raw) if format == "tex" || format == "latex" => tex(raw) + cr(),
            Block::RawBlock(..) => Doc::default(),
            Block::BlockQuote(blocks) => {
                text("@quotation")
                    + cr()
                    + chomp(self.blocks(blocks))
                    + cr()
                    + text("@end quotation")
                    + cr()
            }
            Block::Div(_, blocks) => chomp(self.blocks(blocks)) + cr(),
            Block::HorizontalRule => {
                literal(
                    "@iftex\n@bigskip@hrule@bigskip\n@end iftex\n@ifnottex\n------------------------------------------------------------------------\n@end ifnottex",
                ) + blankline()
            }
            Block::BulletList(items) => self.list("@itemize", "@end itemize", items),
            Block::OrderedList(attrs, items) => {
                let start = match attrs.style {
                    ListNumberStyle::LowerAlpha => alpha(attrs.start, b'a'),
                    ListNumberStyle::UpperAlpha => alpha(attrs.start, b'A'),
                    _ if attrs.start == 1 => String::new(),
                    _ => attrs.start.to_string(),
                };
                self.list(&format!("@enumerate {start}"), "@end enumerate", items)
            }
            Block::DefinitionList(items) => {
                let mut out = vec![text("@table @asis"), cr()];
                for (term, defs) in items {
                    out.extend([text("@item ") + self.inlines(term), blankline()]);
                    for def in defs {
                        out.push(self.blocks(def));
                    }
                }
                out.extend([text("@end table"), blankline()]);
                hcat(out)
            }
            Block::Table(table) => self.table(table),
            Block::Figure(_, caption, blocks) => {
                let title = chomp(self.blocks(&caption.long));
                if let [Block::Table(table)] = blocks.as_slice() {
                    return self.table_with_caption(table, Some(title));
                }
                let float = if !title.is_empty()
                    && matches!(blocks.as_slice(),
                    [Block::Para(items) | Block::Plain(items)] if matches!(items.as_slice(), [Inline::Image(_, _, target)] if target.title == "fig:"))
                {
                    "@float Figure"
                } else {
                    "@float"
                };
                let caption = if title.is_empty() {
                    Doc::default()
                } else {
                    command("caption", title) + cr()
                };
                text(float)
                    + cr()
                    + chomp(self.blocks(blocks))
                    + cr()
                    + caption
                    + text("@end float")
                    + cr()
            }
        }
    }

    fn list(&mut self, open: &str, close: &str, items: &[Vec<Block>]) -> Doc {
        text(open)
            + cr()
            + hcat(
                items
                    .iter()
                    .map(|item| text("@item") + cr() + self.blocks(item)),
            )
            + text(close)
            + blankline()
    }

    fn table(&mut self, table: &Table) -> Doc {
        self.table_with_caption(table, None)
    }

    fn table_with_caption(&mut self, table: &Table, caption: Option<Doc>) -> Doc {
        let rows: Vec<_> = super::rows(table)
            .map(|(head, row)| {
                (
                    head,
                    row.cells
                        .iter()
                        .map(|cell| chomp(self.blocks(&cell.content)))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let mut prototypes = vec![""; table.col_specs.len()];
        let rendered: Vec<Vec<_>> = rows
            .iter()
            .map(|(_, cells)| cells.iter().map(|cell| cell.render(None)).collect())
            .collect();
        for cells in &rendered {
            for (cell, longest) in cells.iter().zip(&mut prototypes) {
                if randoc_layout::real_length(cell) >= randoc_layout::real_length(longest) {
                    *longest = cell;
                }
            }
        }
        let caption = caption.unwrap_or_else(|| chomp(self.blocks(&table.caption.long)));
        let mut out = Vec::new();
        if !caption.is_empty() {
            out.extend([text("@float Table"), cr()]);
        }
        out.push(text("@multitable "));
        if table
            .col_specs
            .iter()
            .any(|spec| matches!(spec.width, randoc_ast::ColWidth::ColWidth(_)))
        {
            out.push(text("@columnfractions "));
            for spec in &table.col_specs {
                let width = match spec.width {
                    randoc_ast::ColWidth::ColWidth(width) => width,
                    randoc_ast::ColWidth::ColWidthDefault => 0.0,
                };
                out.push(text(format!("{width:.2} ")));
            }
        } else {
            for prototype in prototypes {
                out.push(text(format!("{{{}}} ", prototype.replace('\n', " "))));
            }
        }
        out.push(cr());
        for (head, cells) in rows {
            out.extend([text(if head { "@headitem " } else { "@item " }), cr()]);
            let mut cells = cells.into_iter().filter(|cell| !cell.is_empty());
            if let Some(cell) = cells.next() {
                out.push(cell);
                out.push(cr());
            }
            out.push(vcat(cells.map(|cell| text(" @tab ") + cell)));
            out.push(cr());
        }
        out.extend([text("@end multitable"), cr()]);
        if !caption.is_empty() {
            out.extend([command("caption", caption), cr(), text("@end float"), cr()]);
        } else {
            out.push(blankline());
        }
        hcat(out)
    }

    fn inlines(&mut self, items: &[Inline]) -> Doc {
        hcat(items.iter().map(|item| self.inline(item)))
    }

    fn inline(&mut self, inline: &Inline) -> Doc {
        match inline {
            Inline::Str(value) => text(escape(value)),
            Inline::Space => space(),
            Inline::SoftBreak if self.options.wrap_text == WrapOption::WrapPreserve => cr(),
            Inline::SoftBreak => space(),
            Inline::LineBreak => text("@*") + cr(),
            Inline::Emph(items) | Inline::Underline(items) => command("emph", self.inlines(items)),
            Inline::Strong(items) => command("strong", self.inlines(items)),
            Inline::Strikeout(items) => {
                self.strikeout = true;
                command("textstrikeout", self.inlines(items))
            }
            Inline::Subscript(items) => command("sub", self.inlines(items)),
            Inline::Superscript(items) => command("sup", self.inlines(items)),
            Inline::SmallCaps(items) => command("sc", self.inlines(items)),
            Inline::Quoted(kind, items) => {
                let (open, close) = if *kind == QuoteType::SingleQuote {
                    ("`", "'")
                } else {
                    ("``", "''")
                };
                inside(text(open), text(close), self.inlines(items))
            }
            Inline::Span(_, items) | Inline::Cite(_, items) => self.inlines(items),
            Inline::Code(attr, code) => {
                let body = text(escape(code));
                command(
                    "code",
                    if attr.has_class("variable") {
                        command("var", body)
                    } else {
                        body
                    },
                )
            }
            Inline::Math(kind, math) => {
                let body = command("math", literal(math));
                if *kind == MathType::DisplayMath {
                    cr() + body
                } else {
                    body
                }
            }
            Inline::RawInline(format, raw) if format == "texinfo" => literal(raw.clone()),
            Inline::RawInline(format, raw) if format == "tex" || format == "latex" => tex(raw),
            Inline::RawInline(..) => Doc::default(),
            Inline::Note(blocks) => command("footnote", chomp(self.blocks(blocks))),
            Inline::Link(_, items, target) => {
                let label = self.inlines(items).map_text(&argument);
                let url = escape(&target.url);
                if let Some(id) = target.url.strip_prefix('#') {
                    let name = self.names.get(id).map_or(url.as_str(), String::as_str);
                    command("ref", text(format!("{name},,")) + label)
                } else if label.render(None) == url {
                    command("url", text(url))
                } else {
                    command("uref", text(format!("{},", argument(&url))) + label)
                }
            }
            Inline::Image(attr, items, target) => {
                let (base, ext) = target
                    .url
                    .rsplit_once('.')
                    .filter(|(_, suffix)| !suffix.contains('/'))
                    .unwrap_or((&target.url, ""));
                let dimension = |key| {
                    attr.get(key)
                        .filter(|value| !value.ends_with('%'))
                        .and_then(|value| super::context::dimension(value, self.options.dpi))
                        .unwrap_or_default()
                };
                let mut alt = items.to_vec();
                randoc_ast::walk::walk_inlines_mut(&mut alt, |item| {
                    if let Inline::Str(text) = item
                        && text.contains(',')
                    {
                        *item = Inline::RawInline(
                            randoc_ast::Format("texinfo".into()),
                            argument(&escape(text)),
                        );
                    }
                });
                let alt = self.inlines(&alt);
                let width = dimension("width");
                let height = dimension("height");
                command(
                    "image",
                    text(format!("{},{width},{height},", escape(base)))
                        + alt
                        + text(format!(",{}", escape(ext))),
                )
            }
        }
    }
}

fn is_example(items: &[Inline]) -> bool {
    items.len() > 1
        && items.iter().enumerate().all(|(i, item)| {
            if i % 2 == 0 {
                matches!(item, Inline::Code(..))
            } else {
                matches!(item, Inline::LineBreak)
            }
        })
}

fn tex(raw: &str) -> Doc {
    literal(format!("@tex\n{raw}\n@end tex"))
}

fn command(name: &str, body: Doc) -> Doc {
    inside(text(format!("@{name}{{")), text("}"), body)
}

fn alpha(start: i64, base: u8) -> String {
    u8::try_from(start.saturating_sub(1))
        .ok()
        .filter(|n| *n < 26)
        .map_or_else(|| start.to_string(), |n| char::from(base + n).to_string())
}
