//! The inline list with the join rules of the `Inlines` monoid of
//! `pandoc-types` 1.23.1 (`Text.Pandoc.Builder`, BSD-3-Clause; see ADR 13).

use randoc_ast::{Block, Inline};

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Inlines(pub(crate) Vec<Inline>);

impl Inlines {
    pub(crate) fn push(&mut self, inline: Inline) {
        let Some(last) = self.0.last_mut() else {
            self.0.push(inline);
            return;
        };
        match (last, inline) {
            (Inline::Space, Inline::Space)
            | (Inline::SoftBreak, Inline::Space | Inline::SoftBreak)
            | (Inline::LineBreak, Inline::Space | Inline::SoftBreak) => {}
            (last @ Inline::Space, Inline::SoftBreak) => *last = Inline::SoftBreak,
            (last @ (Inline::Space | Inline::SoftBreak), Inline::LineBreak) => {
                *last = Inline::LineBreak;
            }
            (Inline::Str(a), Inline::Str(b)) => a.push_str(&b),
            (Inline::Emph(a), Inline::Emph(b))
            | (Inline::Underline(a), Inline::Underline(b))
            | (Inline::Strong(a), Inline::Strong(b))
            | (Inline::Subscript(a), Inline::Subscript(b))
            | (Inline::Superscript(a), Inline::Superscript(b))
            | (Inline::Strikeout(a), Inline::Strikeout(b)) => a.extend(b),
            (_, inline) => self.0.push(inline),
        }
    }

    pub(crate) fn push_str(&mut self, text: &str) {
        if !text.is_empty() {
            self.push(Inline::Str(text.to_owned()));
        }
    }

    /// Appends a list. Only the two inlines at the border can join.
    pub(crate) fn append(&mut self, other: Inlines) {
        let mut rest = other.0.into_iter();
        if let Some(first) = rest.next() {
            self.push(first);
            self.0.extend(rest);
        }
    }

    /// Removes the spaces and soft breaks at both ends.
    pub(crate) fn trimmed(mut self) -> Vec<Inline> {
        while matches!(self.0.last(), Some(Inline::Space | Inline::SoftBreak)) {
            self.0.pop();
        }
        let lead = self
            .0
            .iter()
            .take_while(|inline| matches!(inline, Inline::Space | Inline::SoftBreak))
            .count();
        self.0.drain(..lead);
        self.0
    }

    /// Moves the paragraph to `blocks` when it has content.
    pub(crate) fn flush_into(&mut self, blocks: &mut Vec<Block>) {
        let inlines = std::mem::take(self).trimmed();
        if !inlines.is_empty() {
            blocks.push(Block::Para(inlines));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_rules_of_the_monoid() {
        let mut list = Inlines::default();
        list.push_str("a");
        list.push_str("b");
        list.push(Inline::Space);
        list.push(Inline::SoftBreak);
        list.push(Inline::Space);
        list.push(Inline::Emph(vec![Inline::Str("c".into())]));
        list.push(Inline::Emph(vec![Inline::Str("d".into())]));
        assert_eq!(
            list.0,
            vec![
                Inline::Str("ab".into()),
                Inline::SoftBreak,
                Inline::Emph(vec![Inline::Str("c".into()), Inline::Str("d".into())]),
            ]
        );
    }
}
