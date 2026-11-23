use super::{End, Parser, R, Tok, tokens_text};
use randoc_ast::walk::walk_blocks_mut;
use randoc_ast::{Attr, Block, Inline, Text};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Copy, Default)]
enum Style {
    #[default]
    Plain,
    Definition,
    Remark,
}

#[derive(Clone)]
struct Definition {
    title: Arc<[Tok]>,
    counter: Text,
    style: Style,
}

#[derive(Default)]
pub(super) struct Theorems {
    style: Style,
    definitions: HashMap<Text, Definition>,
    counters: HashMap<(u64, Text), u64>,
}

impl Theorems {
    pub(super) fn contains(&self, name: &str) -> bool {
        self.definitions.contains_key(name)
    }
}

impl Parser<'_> {
    pub(super) fn theorem_definition(&mut self, command: &str) -> R<()> {
        self.bump();
        self.star();
        let name = self.argument_text()?;
        if command == "theoremstyle" {
            self.theorems.style = match name.as_str() {
                "definition" => Style::Definition,
                "remark" => Style::Remark,
                _ => Style::Plain,
            };
            return Ok(());
        }
        let shared = self.bracket_tokens().map(|tokens| tokens_text(&tokens));
        let title = self.argument_tokens().unwrap_or_default().into();
        self.bracket_tokens();
        let counter = shared.map_or_else(
            || name.clone(),
            |other| {
                self.theorems
                    .definitions
                    .get(&other)
                    .map_or(other, |definition| definition.counter.clone())
            },
        );
        self.theorems.definitions.insert(
            name,
            Definition {
                title,
                counter,
                style: self.theorems.style,
            },
        );
        Ok(())
    }

    pub(super) fn theorem(&mut self, name: &str, blocks: &mut Vec<Block>) -> R<()> {
        let Some(definition) = self.theorems.definitions.get(name).cloned() else {
            return self.div_body(name, blocks);
        };
        let subtitle = self.bracket_inlines()?;
        self.skip_blank();
        let label = if self.peek_raw().is_some_and(|token| token.is_word("label")) {
            self.bump();
            self.argument_text()?
        } else {
            Text::new()
        };
        let chapter = self.sections[0];
        let count = if definition.counter == "chapter" {
            1
        } else {
            let count = self
                .theorems
                .counters
                .entry((chapter, definition.counter))
                .or_default();
            *count += 1;
            *count
        };
        let number = numbered(chapter, count);
        if !label.is_empty() {
            self.section_numbers.insert(label.clone(), number.clone());
        }
        let mut title = self.inlines_of(definition.title.to_vec())?;
        title.extend([Inline::Space, Inline::Str(number)]);
        let mut heading = vec![match definition.style {
            Style::Remark => Inline::Emph(title),
            Style::Plain | Style::Definition => Inline::Strong(title),
        }];
        if let Some(subtitle) = subtitle {
            heading.push(Inline::Space);
            let mut subtitle = super::Inlines(subtitle);
            if let Some(Inline::Str(first)) = subtitle.0.first_mut() {
                first.insert(0, '(');
            } else {
                subtitle.0.insert(0, Inline::Str("(".into()));
            }
            subtitle.push_str(").");
            heading.extend(subtitle.0);
        } else {
            heading.push(Inline::Str(".".into()));
        }
        let mut body = self.blocks(End::Environment(name))?;
        if matches!(definition.style, Style::Plain) {
            walk_blocks_mut(&mut body, |block| {
                if let Block::Para(text) = block {
                    *text = vec![Inline::Emph(std::mem::take(text))];
                }
            });
        }
        heading.push(Inline::Space);
        if let Some(Block::Para(first)) = body.first_mut() {
            heading.push(Inline::Space);
            heading.append(first);
            *first = heading;
        } else {
            if !label.is_empty() && matches!(definition.style, Style::Plain) {
                heading.extend([Inline::Space, Inline::Emph(Vec::new())]);
            }
            body.insert(0, Block::Para(heading));
        }
        blocks.push(Block::Div(
            Attr::new(label, vec![name.into()], vec![]),
            body,
        ));
        self.close_environment(name)
    }
}

pub(super) fn numbered(chapter: u64, count: u64) -> Text {
    if chapter == 0 {
        count.to_string()
    } else {
        format!("{chapter}.{count}")
    }
}
