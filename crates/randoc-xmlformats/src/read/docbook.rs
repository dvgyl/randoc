use randoc_ast::{Block, Inline};
use randoc_core::Result;

use super::{Element, Read, append, attr, class, id, name};

pub(super) fn title_div(title: Vec<Inline>) -> Block {
    let blocks = if title.is_empty() {
        Vec::new()
    } else {
        vec![Block::Plain(title)]
    };
    Block::Div(class("title"), blocks)
}

pub(super) fn layout_text(out: &mut Vec<Inline>, value: &str) {
    for (index, line) in value.replace("\r\n", "\n").split(['\r', '\n']).enumerate() {
        if index > 0 {
            out.push(Inline::LineBreak);
        }
        append(
            out,
            [Inline::Str(
                line.replace(' ', "\u{a0}")
                    .replace('\t', "\u{a0}\u{a0}\u{a0}\u{a0}"),
            )],
        );
    }
}

impl<'a> Read<'a, '_> {
    pub(super) fn titled_div(
        &mut self,
        el: &'a Element,
        level: i64,
        depth: usize,
    ) -> Result<Block> {
        let mut attributes = id(el);
        attributes.classes.push(name(el).into());
        if let Some(role) = el.attr_local("role") {
            attributes.attributes.push(("role".into(), role.into()));
        }
        let mut children = Vec::new();
        if let Some(title) = el.child_local("title") {
            children.push(title_div(self.inlines(&title.children, depth)?));
        }
        children.extend(self.blocks_skipping(&el.children, level, depth, &["title"])?);
        Ok(Block::Div(attributes, children))
    }

    pub(super) fn literal_layout(&mut self, el: &'a Element, depth: usize) -> Result<Block> {
        if attr(el, "class") == "monospaced" {
            return Ok(super::code_block(el));
        }
        let previous = std::mem::replace(&mut self.literal_layout, true);
        let inlines = self.inline_content(&el.children, depth);
        self.literal_layout = previous;
        let mut lines = Vec::new();
        let mut line = Vec::new();
        for inline in inlines? {
            if inline == Inline::LineBreak {
                lines.push(std::mem::take(&mut line));
            } else {
                line.push(inline);
            }
        }
        lines.push(line);
        Ok(Block::LineBlock(lines))
    }

    pub(super) fn question_answer(
        &mut self,
        el: &'a Element,
        level: i64,
        depth: usize,
        out: &mut Vec<Block>,
    ) -> Result<()> {
        let mut blocks = self.blocks(&el.children, level, depth)?;
        blocks.retain(|block| !matches!(block, Block::Para(lines) if lines.is_empty()));
        if let Some(Block::Para(first)) = blocks.first_mut() {
            let label = if name(el) == "question" { "Q:" } else { "A:" };
            let mut lines = vec![
                Inline::Strong(vec![Inline::Str(label.into())]),
                Inline::Str(" ".into()),
            ];
            append(&mut lines, std::mem::take(first));
            *first = lines;
        }
        out.extend(blocks);
        Ok(())
    }
}
