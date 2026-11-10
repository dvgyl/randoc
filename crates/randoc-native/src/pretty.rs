//! The pretty layout of a [`Value`], as the Haskell library `pretty-show` makes it.
//!
//! `pretty-show` builds a document with the combinators of the Haskell library
//! `pretty` and renders it with a line length and a ribbon length. This module
//! keeps only the three combinators that `pretty-show` uses: text, horizontal
//! composition and `sep`. A `sep` group goes on one line when the line has
//! space for the group and for the text that follows the group on that line.
//! Otherwise each item of the group starts a new line.

use crate::value::{Value, show_string};

/// A layout document. `width` is the length of the document on one line.
struct Doc {
    width: usize,
    kind: Kind,
}

enum Kind {
    /// Text without line breaks.
    Text(String),
    /// Documents side by side. The lines of a later document after its first
    /// line start at the column where that document starts.
    Cat(Vec<Doc>),
    /// Documents separated by a space, or one below the other. Each item has
    /// an indentation for the vertical layout.
    Sep(Vec<(usize, Doc)>),
}

fn text(content: impl Into<String>) -> Doc {
    let content = content.into();
    Doc {
        width: content.len(),
        kind: Kind::Text(content),
    }
}

fn cat(parts: Vec<Doc>) -> Doc {
    Doc {
        width: parts.iter().map(|part| part.width).sum(),
        kind: Kind::Cat(parts),
    }
}

fn sep(items: Vec<(usize, Doc)>) -> Doc {
    let width =
        items.iter().map(|(_, item)| item.width).sum::<usize>() + items.len().saturating_sub(1);
    Doc {
        width,
        kind: Kind::Sep(items),
    }
}

/// `hang head 2 body` of the library `pretty`.
fn hang(head: Doc, body: Doc) -> Doc {
    sep(vec![(0, head), (2, body)])
}

/// The bracketed sequence of `pretty-show`: `[ a , b ]`.
fn block(open: char, close: char, items: Vec<Doc>) -> Doc {
    if items.is_empty() {
        return text(format!("{open}{close}"));
    }
    let mut lines = Vec::with_capacity(items.len() + 1);
    for (index, item) in items.into_iter().enumerate() {
        let lead = if index == 0 { open } else { ',' };
        lines.push((0, cat(vec![text(format!("{lead} ")), item])));
    }
    lines.push((0, text(close)));
    sep(lines)
}

fn to_doc(value: &Value) -> Doc {
    match value {
        Value::Con(name, args) if args.is_empty() => text(name.as_str()),
        Value::Con(name, args) => hang(
            text(name.as_str()),
            sep(args.iter().map(|arg| (0, atom(arg))).collect()),
        ),
        Value::Rec(name, fields) => hang(
            text(name.as_str()),
            block(
                '{',
                '}',
                fields
                    .iter()
                    .map(|(field, value)| hang(text(format!("{field} =")), to_doc(value)))
                    .collect(),
            ),
        ),
        Value::List(items) => block('[', ']', items.iter().map(to_doc).collect()),
        Value::Tuple(items) => block('(', ')', items.iter().map(to_doc).collect()),
        Value::Str(content) => {
            let mut shown = String::new();
            show_string(content, &mut shown);
            text(shown)
        }
        Value::Char(c) => {
            let mut shown = String::new();
            show_string(&c.to_string(), &mut shown);
            text(shown)
        }
        Value::Num { negative, digits } => {
            text(format!("{}{digits}", if *negative { "-" } else { "" }))
        }
    }
}

fn atom(value: &Value) -> Doc {
    if value.is_atom() {
        to_doc(value)
    } else {
        cat(vec![text("("), to_doc(value), text(")")])
    }
}

/// The ribbon length that Pandoc uses: the line length divided by 1.2, rounded
/// half to even as the Haskell function `round` does. The library `pretty`
/// keeps the ribbons per line as a single-precision `Float`, and in single
/// precision some quotients fall just below a half, for example 33 / 1.2.
fn ribbon_length(line_length: usize) -> usize {
    (line_length as f32 / 1.2_f32).round_ties_even() as usize
}

struct Renderer {
    out: String,
    column: usize,
    indent: usize,
    line_length: usize,
    ribbon: usize,
}

impl Renderer {
    fn fits(&self, width: usize) -> bool {
        let used = self.column - self.indent;
        let limit = self
            .line_length
            .saturating_sub(self.indent)
            .min(self.ribbon);
        used + width <= limit
    }

    fn newline(&mut self, indent: usize) {
        self.out.push('\n');
        self.out.extend(std::iter::repeat_n(' ', indent));
        self.column = indent;
        self.indent = indent;
    }

    fn flat(&mut self, doc: &Doc) {
        match &doc.kind {
            Kind::Text(content) => self.out.push_str(content),
            Kind::Cat(parts) => parts.iter().for_each(|part| self.flat(part)),
            Kind::Sep(items) => {
                for (index, (_, item)) in items.iter().enumerate() {
                    if index > 0 {
                        self.out.push(' ');
                    }
                    self.flat(item);
                }
            }
        }
    }

    /// Lays out `doc` at the current column. `trailing` is the length of the
    /// text that follows `doc` on its last line.
    fn emit(&mut self, doc: &Doc, trailing: usize) {
        match &doc.kind {
            Kind::Text(content) => {
                self.out.push_str(content);
                self.column += content.len();
            }
            Kind::Cat(parts) => {
                let mut rest = doc.width;
                for part in parts {
                    rest -= part.width;
                    self.emit(part, rest + trailing);
                }
            }
            Kind::Sep(items) => {
                if self.fits(doc.width + trailing) {
                    self.flat(doc);
                    self.column += doc.width;
                    return;
                }
                let base = self.column;
                let last = items.len().saturating_sub(1);
                for (index, (nest, item)) in items.iter().enumerate() {
                    if index > 0 {
                        self.newline(base + nest);
                    }
                    self.emit(item, if index == last { trailing } else { 0 });
                }
            }
        }
    }
}

/// Renders a value as `pretty-show` does, for a line length of `columns`.
/// `trailing` is the length of the text that follows the last line.
pub(crate) fn render(value: &Value, columns: usize, trailing: usize) -> String {
    let doc = to_doc(value);
    let mut renderer = Renderer {
        out: String::new(),
        column: 0,
        indent: 0,
        line_length: columns,
        ribbon: ribbon_length(columns),
    };
    renderer.emit(&doc, trailing);
    renderer.out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ribbon_length_rounds_half_to_even() {
        assert_eq!(ribbon_length(72), 60);
        assert_eq!(ribbon_length(80), 67);
        assert_eq!(ribbon_length(100), 83);
        assert_eq!(ribbon_length(20), 17);
        assert_eq!(ribbon_length(1), 1);
    }

    #[test]
    fn ribbon_length_uses_single_precision() {
        // From `pandoc -f json -t native --columns N`: at 33 a 28-character line breaks.
        assert_eq!(ribbon_length(9), 7);
        assert_eq!(ribbon_length(33), 27);
        assert_eq!(ribbon_length(69), 57);
        assert_eq!(ribbon_length(153), 127);
        assert_eq!(ribbon_length(34), 28);
    }
}
