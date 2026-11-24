use randoc_ast::{Attr, Block, Caption, Inline, ListAttributes, ListNumberDelim, ListNumberStyle};
use randoc_core::shared::auto_identifier;
use randoc_core::{Extension, Result};
use randoc_layout::{Doc, hang, literal, nest, prefixed, real_length, vcat};

use super::Writer;
use super::inlines::image_options;

const ADMONITIONS: &[&str] = &[
    "attention",
    "caution",
    "danger",
    "error",
    "hint",
    "important",
    "note",
    "tip",
    "warning",
    "admonition",
];

impl Writer<'_> {
    pub(super) fn blocks(&mut self, blocks: &[Block], depth: usize) -> Result<Doc> {
        self.render_blocks(blocks, depth, false)
    }

    pub(super) fn render_blocks(
        &mut self,
        blocks: &[Block],
        depth: usize,
        compact_headings: bool,
    ) -> Result<Doc> {
        self.check_depth(depth)?;
        let mut out = Doc::default();
        let mut prev = None;
        let mut section_levels = Vec::new();
        for block in blocks {
            let level = if compact_headings && let Block::Header(level, _, _) = block {
                while section_levels.last().is_some_and(|last| last >= level) {
                    section_levels.pop();
                }
                section_levels.push(*level);
                Some(section_levels.len() as i64)
            } else {
                None
            };
            let mut doc = self.block(block, depth + 1, level)?;
            if matches!(block, Block::BlockQuote(_)) && prev.is_some_and(takes_indent) {
                doc = literal("..").above_blank(doc);
            }
            out = if matches!(prev, Some(Block::Plain(_)))
                && !matches!(
                    block,
                    Block::BulletList(_)
                        | Block::OrderedList(..)
                        | Block::RawBlock(..)
                        | Block::DefinitionList(_)
                        | Block::Table(_)
                )
                && !matches!(block, Block::CodeBlock(attr, _) if code_language(attr).is_some())
            {
                out.above(doc)
            } else {
                out.above_blank(doc)
            };
            prev = Some(block);
        }
        Ok(out)
    }

    fn block(&mut self, block: &Block, depth: usize, heading_level: Option<i64>) -> Result<Doc> {
        Ok(match block {
            Block::Plain(ils) | Block::Para(ils)
                if ils.iter().any(|i| matches!(i, Inline::LineBreak)) =>
            {
                vcat(
                    ils.split(|i| matches!(i, Inline::LineBreak))
                        .map(|line| self.inlines(line, depth).map(|doc| prefixed("| ", doc)))
                        .collect::<Result<Vec<_>>>()?,
                )
            }
            Block::Plain(ils) | Block::Para(ils) => self.inlines(ils, depth)?,
            Block::LineBlock(lines) => vcat(
                lines
                    .iter()
                    .map(|line| self.inlines(line, depth).map(|doc| prefixed("| ", doc)))
                    .collect::<Result<Vec<_>>>()?,
            ),
            Block::Header(level, attr, ils) => {
                if ils.is_empty() {
                    return Ok(Doc::default());
                }
                let title = self.inlines(ils, depth)?;
                if depth > 1 {
                    let directive = literal(".. rubric:: ") + title;
                    let name = if attr.id.is_empty() {
                        Doc::default()
                    } else {
                        literal(format!("   :name: {}", attr.id))
                    };
                    return Ok(directive.above(name));
                }
                let mark = match heading_level.unwrap_or(*level) {
                    1 => '=',
                    2 => '-',
                    3 => '~',
                    4 => '^',
                    5 => '\'',
                    _ => ' ',
                };
                let under = literal(mark.to_string().repeat(real_length(&title.render(None))));
                let heading = title.above(under);
                if !attr.id.is_empty() && attr.id != auto_identifier(ils, self.options.extensions) {
                    literal(format!(".. _{}:", attr.id)).above_blank(heading)
                } else {
                    heading
                }
            }
            Block::HorizontalRule => literal("--------------"),
            Block::BlockQuote(blocks) => nest(3, self.blocks(blocks, depth)?),
            Block::CodeBlock(attr, body) => self.code(attr, body),
            Block::RawBlock(format, body) => {
                if format == "rst" {
                    literal(body)
                } else {
                    let format = if format == "tex" {
                        "latex"
                    } else {
                        format.as_str()
                    };
                    literal(format!(".. raw:: {format}")).above_blank(nest(3, literal(body)))
                }
            }
            Block::BulletList(items) => self.list(items, depth, |_| "-".into())?,
            Block::OrderedList(a, items) => self.list(items, depth, |i| number_marker(a, i))?,
            Block::DefinitionList(items) => {
                let mut out = Doc::default();
                let mut blank = false;
                for (term, defs) in items {
                    let term = self.inlines(term, depth)?;
                    let mut definitions = Doc::default();
                    let mut loose = false;
                    for blocks in defs {
                        let doc = self.indented_blocks(blocks, depth)?;
                        definitions = if loose {
                            definitions.above_blank(doc)
                        } else {
                            definitions.above(doc)
                        };
                        loose = !matches!(blocks.last(), Some(Block::Plain(_)));
                    }
                    let doc = term.above(nest(3, definitions));
                    out = if blank {
                        out.above_blank(doc)
                    } else {
                        out.above(doc)
                    };
                    blank = loose;
                }
                out
            }
            Block::Div(attr, blocks) => self.div(attr, blocks, depth)?,
            Block::Figure(attr, cap, blocks) => self.figure(attr, cap, blocks, depth)?,
            Block::Table(table) => self.table(table, depth)?,
        })
    }

    fn code(&self, attr: &Attr, body: &str) -> Doc {
        if self.options.extensions.contains(Extension::LiterateHaskell)
            && attr.has_class("haskell")
            && attr.has_class("literate")
        {
            return prefixed("> ", literal(body));
        }
        let header = if let Some(lang) = code_language(attr) {
            let header = literal(format!(".. code:: {lang}"));
            if attr.has_class("numberLines") {
                let option = attr.get("startFrom").map_or_else(
                    || "   :number-lines:".into(),
                    |n| format!("   :number-lines: {n}"),
                );
                header.above(literal(option))
            } else {
                header
            }
        } else {
            literal("::")
        };
        header.above_blank(nest(3, literal(body)))
    }

    pub(super) fn indented_blocks(&mut self, blocks: &[Block], depth: usize) -> Result<Doc> {
        let body = self.blocks(blocks, depth)?;
        Ok(if matches!(blocks.first(), Some(Block::BlockQuote(_))) {
            literal("..").above_blank(body)
        } else {
            body
        })
    }

    fn list(
        &mut self,
        items: &[Vec<Block>],
        depth: usize,
        marker: impl Fn(usize) -> String,
    ) -> Result<Doc> {
        let markers: Vec<_> = (0..items.len()).map(marker).collect();
        let width = markers.iter().map(String::len).max().unwrap_or(0) + 1;
        let mut out = Doc::default();
        let mut blank = false;
        for (item, marker) in items.iter().zip(markers) {
            let body = self.indented_blocks(item, depth)?;
            let doc = hang(width, literal(format!("{marker:<width$}")), body);
            out = if blank {
                out.above_blank(doc)
            } else {
                out.above(doc)
            };
            blank = !matches!(item.last(), Some(Block::Plain(_)));
        }
        Ok(out)
    }

    fn div(&mut self, attr: &Attr, blocks: &[Block], depth: usize) -> Result<Doc> {
        let mut content = blocks;
        let mut header = if let Some(kind) = attr
            .classes
            .iter()
            .find(|c| ADMONITIONS.contains(&c.as_str()))
        {
            if kind != "admonition"
                && let Some((Block::Div(a, _), body)) = blocks.split_first()
                && a.has_class("title")
            {
                content = body;
            }
            literal(format!(".. {kind}::"))
        } else {
            let classes = attr
                .classes
                .iter()
                .filter(|c| *c != "container")
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(" ");
            if classes.is_empty() {
                literal(".. container::")
            } else {
                literal(format!(".. container:: {classes}"))
            }
        };
        if !attr.id.is_empty() {
            header = header.above(literal(format!("   :name: {}", attr.id)));
        }
        Ok(header.above_blank(nest(3, self.indented_blocks(content, depth)?)))
    }

    fn figure(
        &mut self,
        attr: &Attr,
        caption: &Caption,
        blocks: &[Block],
        depth: usize,
    ) -> Result<Doc> {
        if let [Block::Plain(ils) | Block::Para(ils)] = blocks
            && let [Inline::Image(a, alt, target)] = ils.as_slice()
        {
            let alt = self.inlines(alt, depth)?;
            let title = if !alt.is_empty() {
                Some(alt)
            } else if !target.title.is_empty() {
                Some(literal(&target.title))
            } else {
                None
            };
            let opts = image_options(a, Some(attr), title, None);
            let head = literal(format!(".. figure:: {}", target.url)).above(vcat(opts));
            return Ok(head.above_blank(nest(3, self.blocks(&caption.long, depth)?)));
        }
        let body = self.blocks(blocks, depth)?;
        let caption = self.blocks(&caption.long, depth)?;
        Ok(literal(".. container:: float").above_blank(nest(3, body.above_blank(caption))))
    }
}

/// Returns the language of an RST code directive.
pub(super) fn code_language(attr: &Attr) -> Option<&str> {
    attr.classes.iter().map(String::as_str).find(|class| {
        !matches!(
            *class,
            "numberLines" | "number-lines" | "number" | "literate"
        )
    })
}

fn takes_indent(block: &Block) -> bool {
    matches!(
        block,
        Block::BulletList(_)
            | Block::OrderedList(..)
            | Block::DefinitionList(_)
            | Block::CodeBlock(..)
            | Block::BlockQuote(_)
            | Block::Div(..)
    )
}

fn number_marker(attrs: &ListAttributes, index: usize) -> String {
    if attrs.style == ListNumberStyle::DefaultStyle && attrs.delim == ListNumberDelim::DefaultDelim
    {
        return "#.".into();
    }
    let n = attrs
        .start
        .saturating_add(i64::try_from(index).unwrap_or(i64::MAX));
    let number = match attrs.style {
        ListNumberStyle::LowerAlpha | ListNumberStyle::UpperAlpha if (1..=26).contains(&n) => {
            let first = if attrs.style == ListNumberStyle::LowerAlpha {
                b'a'
            } else {
                b'A'
            };
            char::from(first + n as u8 - 1).to_string()
        }
        ListNumberStyle::LowerRoman | ListNumberStyle::UpperRoman if (1..=3999).contains(&n) => {
            let mut n = n;
            let mut out = String::new();
            for (v, numeral) in [
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
                while n >= v {
                    out.push_str(numeral);
                    n -= v;
                }
            }
            if attrs.style == ListNumberStyle::LowerRoman {
                out.make_ascii_lowercase();
            }
            out
        }
        _ => n.to_string(),
    };
    match attrs.delim {
        ListNumberDelim::OneParen => format!("{number})"),
        ListNumberDelim::TwoParens => format!("({number})"),
        _ => format!("{number}."),
    }
}
