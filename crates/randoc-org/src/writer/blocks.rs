use randoc_ast::{Attr, Block, Caption, Inline, ListAttributes, ListNumberDelim};
use randoc_core::{Error, Extension, LogMessage, Result};
use randoc_layout::{Doc, blankline, chomp, cr, hang, literal, nest, real_length};

use super::Render;
use crate::shared::{code_line, raw_format};

impl Render<'_> {
    pub(super) fn blocks(&mut self, blocks: &[Block], depth: usize) -> Result<Doc> {
        self.depth(depth)?;
        let mut output = Doc::default();
        for block in blocks {
            output += self.block(block, depth)?;
        }
        Ok(output)
    }

    fn block(&mut self, block: &Block, depth: usize) -> Result<Doc> {
        Ok(match block {
            Block::Plain(text) => self.inlines(text, depth + 1)? + cr(),
            Block::Para(text) => self.inlines(text, depth + 1)? + blankline(),
            Block::Header(level, attr, content) => {
                self.header(*level, attr, content, depth)? + cr()
            }
            Block::HorizontalRule => blankline() + literal("--------------") + blankline(),
            Block::BlockQuote(body) => {
                blankline() + fenced("quote", &chomp(self.blocks(body, depth + 1)?)) + blankline()
            }
            Block::CodeBlock(attr, code) => code_block(attr, code, "") + blankline(),
            Block::RawBlock(format, text) if raw_format(format) => literal(text) + cr(),
            Block::RawBlock(format, text) if format == "html" => {
                blankline()
                    + literal("#+begin_export html")
                    + cr()
                    + nest(2, literal(text.trim_end_matches('\n')))
                    + cr()
                    + literal("#+end_export")
                    + blankline()
            }
            Block::RawBlock(_, _) => {
                self.context
                    .report(LogMessage::BlockNotRendered(randoc_native::show_block(
                        block,
                        self.context
                            .limits
                            .max_nesting_depth
                            .min(crate::shared::STACK_LIMIT),
                    )));
                Doc::default()
            }
            Block::LineBlock(lines) => {
                let mut body = String::new();
                let end = lines.iter().rposition(|line| !line.is_empty());
                for line in &lines[..end.map_or(0, |index| index + 1)] {
                    if !line.is_empty() {
                        body.push_str("  ");
                        body.push_str(&self.inlines(line, depth + 1)?.render(None));
                    }
                    body.push('\n');
                }
                blankline() + literal(format!("#+begin_verse\n{body}#+end_verse")) + blankline()
            }
            Block::BulletList(items) => self.list(items, None, depth)? + blankline(),
            Block::OrderedList(attr, items) => self.list(items, Some(*attr), depth)? + blankline(),
            Block::DefinitionList(items) => self.definitions(items, depth)? + blankline(),
            Block::Div(attr, body) => self.div(attr, body, depth)?,
            Block::Figure(attr, caption, body) => {
                let caption = self.caption(caption, depth)?;
                caption + anchor(attr) + self.blocks(body, depth + 1)? + blankline()
            }
            Block::Table(table) => self.table(table, depth)? + blankline(),
        })
    }

    fn header(&mut self, level: i64, attr: &Attr, content: &[Inline], depth: usize) -> Result<Doc> {
        let level = usize::try_from(level.max(1)).unwrap_or(usize::MAX);
        if level as u64 > self.context.limits.max_input_size || level > 1_000_000 {
            return Err(Error::Parse(
                "The Org heading exceeds the size limit.".into(),
            ));
        }
        let mut end = content.len();
        let mut tags = Vec::new();
        while let Some(Inline::Span(attr, _)) = end.checked_sub(1).and_then(|pos| content.get(pos))
        {
            let Some(tag) = attr.get("tag-name").filter(|_| attr.has_class("tag")) else {
                break;
            };
            tags.push(tag);
            end -= 1;
            if end > 0 && matches!(&content[end - 1], Inline::Str(space) if space == "\u{a0}") {
                end -= 1;
            }
        }
        if !tags.is_empty() && end > 0 && matches!(content[end - 1], Inline::Space) {
            end -= 1;
        }
        let text = self.inlines(&content[..end], depth + 1)?.render(None);
        let mut title = format!("{} {text}", "*".repeat(level));
        if !tags.is_empty() {
            tags.reverse();
            let tags = format!(":{}:", tags.join(":"));
            let width = real_length(&title).saturating_add(real_length(&tags));
            let columns = usize::try_from(self.options.columns).unwrap_or(0);
            title.push_str(&" ".repeat(columns.saturating_sub(width).max(1)));
            title.push_str(&tags);
        }
        if !attr.is_empty() {
            title.push_str("\n:PROPERTIES:");
            if !attr.id.is_empty() {
                title.push_str(&format!("\n:CUSTOM_ID: {}", attr.id));
            }
            if !attr.classes.is_empty() {
                title.push_str(&format!("\n:CLASS: {}", attr.classes.join(" ")));
            }
            for (key, value) in &attr.attributes {
                title.push_str(&format!("\n:{key}: {value}"));
            }
            title.push_str("\n:END:");
        }
        Ok(literal(title))
    }

    pub(super) fn caption(&mut self, caption: &Caption, depth: usize) -> Result<Doc> {
        let body = chomp(self.blocks(&caption.long, depth + 1)?);
        if body.is_empty() {
            Ok(body)
        } else {
            Ok(literal("#+caption: ") + body + cr())
        }
    }

    fn list(
        &mut self,
        items: &[Vec<Block>],
        attr: Option<ListAttributes>,
        depth: usize,
    ) -> Result<Doc> {
        let loose = items.iter().any(|item| {
            !ends_with_plain(item) || item.iter().any(|block| matches!(block, Block::Para(_)))
        });
        let mut output = Doc::default();
        for (i, item) in items.iter().enumerate() {
            let marker = if let Some(attr) = attr {
                let mark = if matches!(
                    attr.delim,
                    ListNumberDelim::OneParen | ListNumberDelim::TwoParens
                ) {
                    ')'
                } else {
                    '.'
                };
                format!("{}{mark} ", attr.start.saturating_add(i as i64))
            } else {
                "- ".into()
            };
            let indent = marker.len();
            let mut prefix = marker;
            if let Some(attr) = attr.filter(|attr| i == 0 && attr.start != 1) {
                prefix.push_str(&format!("[@{}] ", attr.start));
            }
            let content = if self.enabled(Extension::TaskLists)
                && let Some(Block::Para(text) | Block::Plain(text)) = item.first()
                && let Some(Inline::Str(mark)) = text.first()
                && matches!(mark.as_str(), "☐" | "☒")
            {
                prefix.push_str(if mark == "☐" { "[ ]" } else { "[X]" });
                if text.len() > 1 {
                    prefix.push(' ');
                }
                let start = if matches!(text.get(1), Some(Inline::Space)) {
                    2
                } else {
                    1
                };
                let tail = self.inlines(&text[start..], depth + 1)?;
                let sep = if matches!(item.first(), Some(Block::Para(_))) {
                    blankline()
                } else {
                    cr()
                };
                tail + sep + self.blocks(&item[1..], depth + 1)?
            } else if item
                .first()
                .is_none_or(|block| matches!(block, Block::Plain(_) | Block::Para(_)))
            {
                self.blocks(item, depth + 1)?
            } else {
                // A marker line of its own: the first block cannot follow the marker.
                cr() + self.blocks(item, depth + 1)?
            };
            output += hang(indent, literal(prefix), chomp(content));
            output += if loose { blankline() } else { cr() };
        }
        Ok(chomp(output))
    }

    fn definitions(
        &mut self,
        items: &[(Vec<Inline>, Vec<Vec<Block>>)],
        depth: usize,
    ) -> Result<Doc> {
        let mut output = Doc::default();
        for (term, definitions) in items {
            let term = self.inlines(term, depth + 1)?;
            let mut content = Doc::default();
            for definition in definitions {
                content += self.blocks(definition, depth + 1)?;
            }
            let loose = definitions
                .iter()
                .flatten()
                .any(|block| matches!(block, Block::Para(_)));
            output += hang(2, literal("- "), term + literal(" :: ") + chomp(content));
            output += if loose || !definitions.last().is_none_or(|item| ends_with_plain(item)) {
                blankline()
            } else {
                cr()
            };
        }
        Ok(chomp(output))
    }

    fn div(&mut self, attr: &Attr, blocks: &[Block], depth: usize) -> Result<Doc> {
        // Jupyter notebook cells: the source of a code cell and the result of its
        // execution.
        match (attr.classes.as_slice(), blocks) {
            ([cell, code], [Block::CodeBlock(code_attr, text), rest @ ..])
                if cell == "cell" && code == "code" && !code_attr.classes.is_empty() =>
            {
                return Ok(code_block(code_attr, text, "jupyter-")
                    + blankline()
                    + self.blocks(rest, depth + 1)?);
            }
            ([output, result], [Block::CodeBlock(_, text)])
                if output == "output" && result == "execute_result" =>
            {
                let mut lines = String::from("#+RESULTS:");
                for line in text.lines().filter(|line| !line.is_empty()) {
                    lines.push_str("\n: ");
                    lines.push_str(line);
                }
                return Ok(literal(lines) + cr());
            }
            _ => {}
        }
        match attr.classes.as_slice() {
            [name, class] if class == "drawer" => {
                let body = chomp(self.blocks(blocks, depth + 1)?);
                Ok(literal(format!(":{name}:"))
                    + blankline()
                    + body
                    + blankline()
                    + literal(":END:")
                    + blankline())
            }
            [class]
                if matches!(
                    class.as_str(),
                    "note"
                        | "tip"
                        | "important"
                        | "caution"
                        | "warning"
                        | "center"
                        | "CENTER"
                        | "quote"
                        | "QUOTE"
                ) =>
            {
                let admonition = matches!(
                    class.as_str(),
                    "note" | "tip" | "important" | "caution" | "warning"
                );
                let blocks = if admonition
                    && matches!(blocks.first(), Some(Block::Div(attr, _)) if attr.classes == ["title"])
                {
                    &blocks[1..]
                } else {
                    blocks
                };
                let mut prefix = Doc::default();
                if !attr.id.is_empty() {
                    prefix += literal(format!("#+name: {}", attr.id)) + cr();
                }
                if !attr.attributes.is_empty() {
                    let pairs = attr
                        .attributes
                        .iter()
                        .map(|(key, val)| format!(":{key} {val}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    prefix += literal(format!("#+attr_html: {pairs}")) + cr();
                }
                Ok(blankline()
                    + prefix
                    + fenced(class, &chomp(self.blocks(blocks, depth + 1)?))
                    + blankline())
            }
            _ => Ok(blankline() + anchor(attr) + self.blocks(blocks, depth + 1)? + blankline()),
        }
    }
}

/// Returns `true` when a list item ends with plain text, so that the next item
/// can follow without a blank line.
fn ends_with_plain(item: &[Block]) -> bool {
    match item.last() {
        None | Some(Block::Plain(_)) => true,
        Some(Block::BulletList(items) | Block::OrderedList(_, items)) => {
            items.last().is_none_or(|item| ends_with_plain(item))
        }
        Some(Block::DefinitionList(items)) => items
            .last()
            .and_then(|(_, definitions)| definitions.last())
            .is_none_or(|item| ends_with_plain(item)),
        Some(_) => false,
    }
}

fn anchor(attr: &Attr) -> Doc {
    if attr.id.is_empty() {
        Doc::default()
    } else {
        literal(format!("<<{}>>", attr.id)) + cr()
    }
}

fn fenced(class: &str, body: &Doc) -> Doc {
    literal(format!("#+begin_{class}"))
        + cr()
        + body.clone()
        + cr()
        + literal(format!("#+end_{class}"))
}

fn code_block(attr: &Attr, code: &str, language_prefix: &str) -> Doc {
    let mut head = if attr.id.is_empty() {
        String::new()
    } else {
        format!("#+name: {}\n", attr.id)
    };
    let lang = attr
        .classes
        .iter()
        .find(|name| !matches!(name.as_str(), "numberLines" | "continuedSourceBlock"));
    let kind = if let Some(lang) = lang {
        let lang = match lang.as_str() {
            "c" => "C",
            "r" => "R",
            "commonlisp" => "lisp",
            "bash" => "sh",
            lang => lang,
        };
        head.push_str(&format!("#+begin_src {language_prefix}{lang}"));
        "src"
    } else {
        head.push_str("#+begin_example");
        "example"
    };
    if kind == "src" {
        if attr.has_class("numberLines") {
            head.push_str(if attr.has_class("continuedSourceBlock") {
                " +n"
            } else {
                " -n"
            });
            if let Some(start) = attr.get("startFrom").filter(|start| !start.is_empty()) {
                head.push(' ');
                head.push_str(start);
            }
        }
        for (key, value) in &attr.attributes {
            if !matches!(key.as_str(), "startFrom" | "org-language") {
                head.push_str(&format!(" :{key} {value}"));
            }
        }
    }
    head.push('\n');
    for line in code.lines() {
        head.push_str(&code_line(line, true));
        head.push('\n');
    }
    head.push_str(&format!("#+end_{kind}"));
    literal(head)
}
