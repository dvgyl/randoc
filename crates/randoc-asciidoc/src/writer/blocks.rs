use randoc_ast::{Attr, Block, Caption, Inline, ListAttributes, ListNumberStyle, MathType};
use randoc_core::{Extension, shared};
use randoc_layout::{Doc, blankline, cr, empty, hcat, literal, nest, nowrap, vcat, vsep};

use super::{Dialect, Renderer};

impl Renderer<'_> {
    pub(super) fn blocks(&mut self, blocks: &[Block], list: usize, table: usize) -> Doc {
        vsep(
            blocks
                .iter()
                .map(|block| self.block(block, list, table))
                .filter(|doc| !doc.is_empty()),
        )
    }

    fn block(&mut self, block: &Block, list: usize, table: usize) -> Doc {
        match block {
            Block::Plain(inlines) | Block::Para(inlines) => {
                let mut body = self.inlines(inlines, false, table > 0);
                if starts_ordered(inlines) {
                    body = hcat([literal("{empty}"), body]);
                }
                if list > 0
                    && matches!(inlines.as_slice(), [Inline::Math(MathType::DisplayMath, _)])
                {
                    vcat([literal("+"), body])
                } else {
                    body
                }
            }
            Block::LineBlock(lines) => vcat(lines.iter().enumerate().map(|(i, line)| {
                let line = self.inlines(line, false, table > 0);
                if i + 1 < lines.len() {
                    hcat([line, literal(" +")])
                } else {
                    line
                }
            })),
            Block::Header(level, attr, title) => {
                let extension = self.options.extensions;
                let id = if (extension.contains(Extension::AutoIdentifiers)
                    || self.dialect != Dialect::Modern)
                    && !extension.contains(Extension::AsciiIdentifiers)
                {
                    let mut extensions = extension;
                    extensions.disable(Extension::GfmAutoIdentifiers);
                    let candidate = shared::auto_identifier(title, extensions);
                    self.ids.unique(&candidate)
                } else {
                    String::new()
                };
                let markers = "=".repeat((level.saturating_add(1)).clamp(1, 64) as usize);
                vcat([
                    anchor(if attr.id != id { &attr.id } else { "" }),
                    nowrap(hcat([
                        literal(format!("{markers} ")),
                        self.inlines(title, false, table > 0),
                    ])),
                ])
            }
            Block::HorizontalRule => literal("'''''"),
            Block::RawBlock(format, value) if format.0.eq_ignore_ascii_case("asciidoc") => {
                literal(value)
            }
            Block::RawBlock(..) => empty(),
            Block::CodeBlock(attr, code) => {
                let classes = attr
                    .classes
                    .iter()
                    .filter(|class| class.as_str() != "numberLines")
                    .cloned()
                    .collect::<Vec<_>>();
                let listing = !classes.is_empty() || attr.has_class("numberLines");
                let marker = if listing { "----" } else { "...." };
                let mut output = String::new();
                if !attr.id.is_empty() {
                    output.push_str(&format!("[[{}]]\n", attr.id));
                }
                if listing {
                    let numbers = if attr.has_class("numberLines") {
                        "%linesnum"
                    } else {
                        ""
                    };
                    let languages = if classes.is_empty() {
                        String::new()
                    } else {
                        format!(",{}", classes.join(","))
                    };
                    output.push_str(&format!("[source{numbers}{languages}]\n"));
                }
                output.push_str(&format!("{marker}\n{code}\n{marker}"));
                literal(output)
            }
            Block::BlockQuote(blocks) => {
                let nested = blocks
                    .iter()
                    .any(|block| matches!(block, Block::BlockQuote(_)));
                let body = self.blocks(blocks, list, table);
                let body = if nested {
                    vcat([literal("--"), body, blankline(), literal("--")])
                } else {
                    body
                };
                vcat([literal("____"), body, literal("____")])
            }
            Block::Div(attr, blocks) => self.division(attr, blocks, list, table),
            Block::BulletList(items) => self.list(items, None, list, table),
            Block::OrderedList(options, items) => self.list(items, Some(options), list, table),
            Block::DefinitionList(items) => {
                let marker = ["::", ";;", ":::", ":::;"][list.min(3)];
                vcat(items.iter().map(|(term, definitions)| {
                    let label = hcat([self.inlines(term, false, table > 0), literal(marker)]);
                    let definitions = definitions
                        .iter()
                        .map(|definition| self.definition(definition, list + 1, table));
                    let mut docs = vec![label];
                    for (index, definition) in definitions.enumerate() {
                        if index > 0 {
                            docs.push(literal("  +"));
                        }
                        docs.push(definition);
                    }
                    vcat(docs)
                }))
            }
            Block::Figure(attr, caption, blocks) => {
                if let [Block::Plain(inlines) | Block::Para(inlines)] = blocks.as_slice()
                    && let [Inline::Image(image_attr, label, target)] = inlines.as_slice()
                {
                    let caption = self.caption(caption, table);
                    let image = self.image(image_attr, label, target, true);
                    let id = if attr.id.is_empty() {
                        empty()
                    } else {
                        literal(format!("[#{}]", attr.id))
                    };
                    vcat([caption, id, image])
                } else {
                    let caption = self.caption(caption, table);
                    let body = self.blocks(blocks, list, table);
                    vcat([anchor(&attr.id), caption, body])
                }
            }
            Block::Table(value) => self.table(value, list, table),
        }
    }

    pub(super) fn caption(&self, caption: &Caption, table: usize) -> Doc {
        let mut lines = caption
            .long
            .iter()
            .filter_map(|block| match block {
                Block::Plain(inlines) | Block::Para(inlines) => {
                    Some(self.inlines(inlines, false, table > 0).render(None))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if lines.is_empty()
            && let Some(short) = &caption.short
        {
            lines.push(self.inlines(short, false, table > 0).render(None));
        }
        if lines.is_empty() {
            empty()
        } else {
            literal(format!(".{}", lines.join(" ").replace('\n', " ")))
        }
    }

    fn division(&mut self, attr: &Attr, blocks: &[Block], list: usize, table: usize) -> Doc {
        let style = attr.classes.iter().find(|class| {
            matches!(
                class.as_str(),
                "note" | "tip" | "important" | "warning" | "caution" | "sidebar" | "example"
            )
        });
        let Some(style) = style else {
            return vcat([anchor(&attr.id), self.blocks(blocks, list, table)]);
        };
        let mut body = blocks;
        let mut title = empty();
        if let Some((Block::Div(attr, title_blocks), rest)) = blocks.split_first()
            && attr.has_class("title")
        {
            body = rest;
            let value = shared::stringify_blocks(title_blocks);
            if !value.eq_ignore_ascii_case(style) {
                title = literal(format!(".{value}"));
            }
        }
        let marker = if style == "sidebar" { "****" } else { "====" };
        let style = if style == "example" {
            empty()
        } else {
            literal(format!("[{}]", style.to_ascii_uppercase()))
        };
        vcat([
            anchor(&attr.id),
            style,
            title,
            literal(marker),
            self.blocks(body, list, table),
            literal(marker),
        ])
    }

    fn list(
        &mut self,
        items: &[Vec<Block>],
        options: Option<&ListAttributes>,
        level: usize,
        table: usize,
    ) -> Doc {
        let marker = if options.is_some() { '.' } else { '*' };
        let mark = format!("{} ", marker.to_string().repeat(level + 1));
        let style = options.map_or_else(empty, |options| {
            let style = match options.style {
                ListNumberStyle::LowerAlpha => "loweralpha",
                ListNumberStyle::UpperAlpha => "upperalpha",
                ListNumberStyle::LowerRoman => "lowerroman",
                ListNumberStyle::UpperRoman => "upperroman",
                ListNumberStyle::DefaultStyle => {
                    return if options.start == 1 {
                        empty()
                    } else {
                        literal(format!("[start={}]", options.start))
                    };
                }
                _ => "arabic",
            };
            literal(if options.start == 1 {
                format!("[{style}]")
            } else {
                format!("[{style}, start={}]", options.start)
            })
        });
        let items = vcat(items.iter().map(|item| {
            let body = self.item(item, level + 1, table, options.is_some());
            hcat([literal(&mark), body])
        }));
        vcat([style, items])
    }

    fn definition(&mut self, blocks: &[Block], level: usize, table: usize) -> Doc {
        vcat(blocks.iter().enumerate().map(|(index, block)| {
            let body = match block {
                Block::OrderedList(options, items) => {
                    nest(2, self.list(items, Some(options), 0, table))
                }
                Block::BulletList(items) => nest(2, self.list(items, None, 0, table)),
                Block::CodeBlock(..) | Block::DefinitionList(_) => self.block(block, level, table),
                _ => nest(2, self.block(block, level, table)),
            };
            if index == 0 || matches!(block, Block::DefinitionList(_)) {
                body
            } else {
                vcat([literal("  +"), body])
            }
        }))
    }

    fn item(&mut self, blocks: &[Block], level: usize, table: usize, ordered: bool) -> Doc {
        hcat(blocks.iter().enumerate().map(|(index, block)| {
            let block_doc = match block {
                Block::Plain(inlines) | Block::Para(inlines) if index == 0 => {
                    if let Some(index) = inlines
                        .iter()
                        .position(|inline| matches!(inline, Inline::Math(MathType::DisplayMath, _)))
                        && index > 0
                    {
                        vcat([
                            self.inlines(&inlines[..index], false, table > 0),
                            literal("+"),
                            self.inlines(&inlines[index..], false, table > 0),
                        ])
                    } else if self.dialect != Dialect::Legacy
                        && let Some(Inline::Str(mark)) = inlines.first()
                        && matches!(mark.as_str(), "☒" | "☐")
                    {
                        hcat([
                            literal(if mark == "☒" { "[x]" } else { "[ ]" }),
                            self.inlines(&inlines[1..], false, table > 0),
                        ])
                    } else {
                        self.inlines(inlines, false, table > 0)
                    }
                }
                Block::BulletList(items) => {
                    self.list(items, None, level - usize::from(ordered), table)
                }
                Block::OrderedList(options, items) => {
                    self.list(items, Some(options), level - usize::from(!ordered), table)
                }
                _ => self.block(block, level, table),
            };
            if index == 0 {
                return block_doc;
            }
            match block {
                Block::BulletList(_) | Block::OrderedList(..) | Block::DefinitionList(_) => {
                    let plain = match block {
                        Block::OrderedList(options, _) => {
                            options.start == 1 && options.style == ListNumberStyle::DefaultStyle
                        }
                        _ => true,
                    };
                    hcat([
                        if index > 1 && plain {
                            blankline()
                        } else {
                            cr()
                        },
                        block_doc,
                    ])
                }
                Block::Para(inlines)
                    if matches!(inlines.as_slice(), [Inline::Math(MathType::DisplayMath, _)]) =>
                {
                    hcat([cr(), block_doc])
                }
                Block::Plain(_) | Block::Para(_) => hcat([cr(), literal("+"), cr(), block_doc]),
                _ => hcat([
                    cr(),
                    literal("+"),
                    if block_doc.is_empty() {
                        blankline()
                    } else {
                        cr()
                    },
                    block_doc,
                ]),
            }
        }))
    }
}

fn anchor(id: &str) -> Doc {
    if id.is_empty() {
        empty()
    } else {
        literal(format!("[[{id}]]"))
    }
}

fn starts_ordered(inlines: &[Inline]) -> bool {
    matches!(inlines, [Inline::Str(text), Inline::Space, ..] if text.strip_suffix('.').is_some_and(|prefix| !prefix.is_empty() && prefix.bytes().all(|ch| ch.is_ascii_digit())))
}
