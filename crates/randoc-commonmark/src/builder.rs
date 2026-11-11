//! A list of inlines that joins neighbours as the `Inlines` monoid of `pandoc-types` does.

use randoc_ast::Inline;

/// Inline content under construction.
#[derive(Debug, Default, Clone)]
pub(crate) struct Inlines(pub(crate) Vec<Inline>);

impl Inlines {
    pub(crate) fn new() -> Self {
        Inlines(Vec::new())
    }

    pub(crate) fn into_vec(self) -> Vec<Inline> {
        self.0
    }

    /// Appends one inline and joins it with the last inline when the monoid does.
    pub(crate) fn push(&mut self, inline: Inline) {
        let Some(last) = self.0.pop() else {
            self.0.push(inline);
            return;
        };
        match (last, inline) {
            (Inline::Space, Inline::Space) => self.0.push(Inline::Space),
            (Inline::Space, Inline::SoftBreak) | (Inline::SoftBreak, Inline::Space) => {
                self.0.push(Inline::SoftBreak)
            }
            (Inline::SoftBreak, Inline::SoftBreak) => self.0.push(Inline::SoftBreak),
            (Inline::Space | Inline::SoftBreak, Inline::LineBreak)
            | (Inline::LineBreak, Inline::Space) => self.0.push(Inline::LineBreak),
            (Inline::Str(mut a), Inline::Str(b)) => {
                a.push_str(&b);
                self.0.push(Inline::Str(a));
            }
            (Inline::Emph(a), Inline::Emph(b)) => self.0.push(Inline::Emph(join(a, b))),
            (Inline::Strong(a), Inline::Strong(b)) => self.0.push(Inline::Strong(join(a, b))),
            (Inline::Strikeout(a), Inline::Strikeout(b)) => {
                self.0.push(Inline::Strikeout(join(a, b)))
            }
            (Inline::Superscript(a), Inline::Superscript(b)) => {
                self.0.push(Inline::Superscript(join(a, b)))
            }
            (Inline::Subscript(a), Inline::Subscript(b)) => {
                self.0.push(Inline::Subscript(join(a, b)))
            }
            (Inline::SmallCaps(a), Inline::SmallCaps(b)) => {
                self.0.push(Inline::SmallCaps(join(a, b)))
            }
            (Inline::Underline(a), Inline::Underline(b)) => {
                self.0.push(Inline::Underline(join(a, b)))
            }
            (a, b) => {
                self.0.push(a);
                self.0.push(b);
            }
        }
    }

    /// Appends each inline of a list.
    pub(crate) fn extend(&mut self, inlines: Vec<Inline>) {
        if self.0.is_empty() {
            self.0 = inlines;
            return;
        }
        for inline in inlines {
            self.push(inline);
        }
    }

    /// Appends text as `Builder.text` does: runs of spaces and tabs become `Space`,
    /// runs with a line ending become `SoftBreak`, other runs become `Str`.
    pub(crate) fn text(&mut self, text: &str) {
        let mut word_start = None;
        let mut space_start = None;
        let mut has_newline = false;
        for (index, ch) in text.char_indices() {
            let is_space = matches!(ch, ' ' | '\t' | '\n' | '\r');
            if is_space {
                if let Some(start) = word_start.take() {
                    self.push(Inline::Str(text.get(start..index).unwrap_or("").into()));
                }
                if space_start.is_none() {
                    space_start = Some(index);
                    has_newline = false;
                }
                has_newline |= ch == '\n' || ch == '\r';
            } else {
                if space_start.take().is_some() {
                    self.push(break_inline(has_newline));
                }
                if word_start.is_none() {
                    word_start = Some(index);
                }
            }
        }
        if let Some(start) = word_start {
            self.push(Inline::Str(text.get(start..).unwrap_or("").into()));
        }
        if space_start.is_some() {
            self.push(break_inline(has_newline));
        }
    }
}

fn break_inline(has_newline: bool) -> Inline {
    if has_newline {
        Inline::SoftBreak
    } else {
        Inline::Space
    }
}

/// Joins the content of two elements of one kind. The content is a plain list in
/// `pandoc-types`, so the two inlines at the join stay separate.
fn join(mut a: Vec<Inline>, b: Vec<Inline>) -> Vec<Inline> {
    a.extend(b);
    a
}

/// Splits text into inlines as `Builder.text` does.
pub(crate) fn text(text: &str) -> Vec<Inline> {
    let mut out = Inlines::new();
    out.text(text);
    out.into_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Inline {
        Inline::Str(text.into())
    }

    #[test]
    fn text_splits_spaces_and_line_endings() {
        assert_eq!(
            text("a b\t\tc \n d "),
            vec![
                s("a"),
                Inline::Space,
                s("b"),
                Inline::Space,
                s("c"),
                Inline::SoftBreak,
                s("d"),
                Inline::Space
            ]
        );
        assert_eq!(text("\u{a0}x"), vec![s("\u{a0}x")]);
    }

    #[test]
    fn neighbours_join() {
        let mut out = Inlines::new();
        out.push(Inline::Emph(vec![s("a")]));
        out.push(Inline::Emph(vec![s("b")]));
        out.push(Inline::Space);
        out.push(Inline::SoftBreak);
        out.push(Inline::Space);
        out.push(Inline::LineBreak);
        out.text("x");
        out.text("y");
        out.text("");
        assert_eq!(
            out.into_vec(),
            vec![
                Inline::Emph(vec![s("a"), s("b")]),
                Inline::LineBreak,
                s("xy")
            ]
        );
    }
}
