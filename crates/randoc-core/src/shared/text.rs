//! The plain text of a document tree, as the Pandoc function `stringify` gives it.
//!
//! The rules come from the oracle (`pandoc.utils.stringify` of Pandoc 3.13):
//!
//! - `Str`, `Code` and `Math` give their text.
//! - `Space`, `SoftBreak` and `LineBreak` give one space.
//! - `Quoted` gives curly quotation marks around its content.
//! - `Note` gives no text. A `RawInline` gives one space when its format is
//!   exactly `html` and its text starts with `<br`, and no text in each other
//!   case. `Cite` gives its content only, not the prefix or the suffix of a
//!   citation.
//! - A block gives the text of all its inlines, with no separator. `CodeBlock`,
//!   `RawBlock` and `HorizontalRule` give no text. A caption comes before the
//!   content of a figure or a table.
//! - `MetaBool` gives `true` or `false`. A metadata map gives its values in
//!   key order.
//!
//! The traversal uses a stack on the heap, so a deep tree cannot overflow the
//! call stack. Time is linear in the size of the tree.

use super::emoji::replace_emoji_with_aliases;
use randoc_ast::{Attr, Block, Caption, Format, Inline, MetaValue, QuoteType, Row, Text};

enum Node<'a> {
    Inline(&'a Inline),
    Block(&'a Block),
    Meta(&'a MetaValue),
    Literal(&'static str),
}

/// Returns the plain text of inlines.
///
/// ```
/// use randoc_ast::Inline;
/// use randoc_core::shared::stringify;
///
/// let inlines = [Inline::Emph(vec![Inline::Str("a".into())]), Inline::LineBreak, Inline::Str("b".into())];
/// assert_eq!(stringify(&inlines), "a b");
/// ```
pub fn stringify(inlines: &[Inline]) -> Text {
    collect(
        inlines.iter().rev().map(Node::Inline).collect(),
        Purpose::Text,
    )
}

/// The use of a plain text, which decides the rules for emoji.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Purpose {
    /// The text of `stringify`.
    Text,
    /// Identifier text with the CommonMark rules: an emoji span gives its alias.
    /// The caller changes each other emoji in the whole text.
    CommonmarkIdentifier,
    /// Identifier text with the Pandoc rules: an emoji span gives its alias, and
    /// an emoji character in a `Str` gives its first alias. Emoji in code, math
    /// and alias values stay.
    PandocIdentifier,
}

/// Returns the plain text of inlines for a purpose.
///
/// An emoji span has no identifier, the one class `emoji` and the one attribute
/// `data-emoji`.
pub(super) fn identifier_text(inlines: &[Inline], purpose: Purpose) -> Text {
    collect(inlines.iter().rev().map(Node::Inline).collect(), purpose)
}

/// Returns the plain text of blocks: the text of each inline, with no separator.
///
/// ```
/// use randoc_ast::{Block, Inline};
/// use randoc_core::shared::stringify_blocks;
///
/// let blocks = [Block::Para(vec![Inline::Str("a".into())]), Block::Plain(vec![Inline::Str("b".into())])];
/// assert_eq!(stringify_blocks(&blocks), "ab");
/// ```
pub fn stringify_blocks(blocks: &[Block]) -> Text {
    collect(
        blocks.iter().rev().map(Node::Block).collect(),
        Purpose::Text,
    )
}

/// Returns the plain text of a metadata value.
///
/// ```
/// use randoc_ast::MetaValue;
/// use randoc_core::shared::stringify_meta;
///
/// assert_eq!(stringify_meta(&MetaValue::MetaBool(true)), "true");
/// ```
pub fn stringify_meta(value: &MetaValue) -> Text {
    collect(vec![Node::Meta(value)], Purpose::Text)
}

fn collect(mut stack: Vec<Node<'_>>, purpose: Purpose) -> Text {
    let mut out = Text::new();
    while let Some(node) = stack.pop() {
        match node {
            Node::Literal(text) => out.push_str(text),
            Node::Inline(inline) => push_inline(inline, purpose, &mut out, &mut stack),
            Node::Block(block) => push_block(block, &mut stack),
            Node::Meta(value) => push_meta(value, &mut out, &mut stack),
        }
    }
    out
}

fn push_inlines<'a>(inlines: &'a [Inline], stack: &mut Vec<Node<'a>>) {
    stack.extend(inlines.iter().rev().map(Node::Inline));
}

fn push_blocks<'a>(blocks: &'a [Block], stack: &mut Vec<Node<'a>>) {
    stack.extend(blocks.iter().rev().map(Node::Block));
}

fn emoji_alias(attr: &Attr) -> Option<&str> {
    match (
        attr.id.as_str(),
        attr.classes.as_slice(),
        attr.attributes.as_slice(),
    ) {
        ("", [class], [(key, alias)]) if class == "emoji" && key == "data-emoji" => Some(alias),
        _ => None,
    }
}

fn push_inline<'a>(
    inline: &'a Inline,
    purpose: Purpose,
    out: &mut Text,
    stack: &mut Vec<Node<'a>>,
) {
    match inline {
        Inline::Str(text) if purpose == Purpose::PandocIdentifier => {
            out.push_str(&replace_emoji_with_aliases(text));
        }
        Inline::Str(text) | Inline::Code(_, text) | Inline::Math(_, text) => out.push_str(text),
        Inline::Space | Inline::SoftBreak | Inline::LineBreak => out.push(' '),
        Inline::RawInline(Format(format), text) if format == "html" && text.starts_with("<br") => {
            out.push(' ');
        }
        Inline::RawInline(..) | Inline::Note(_) => {}
        Inline::Span(attr, content) => match emoji_alias(attr).filter(|_| purpose != Purpose::Text)
        {
            Some(alias) => out.push_str(alias),
            None => push_inlines(content, stack),
        },
        Inline::Quoted(kind, content) => {
            let (open, close) = match kind {
                QuoteType::SingleQuote => ("\u{2018}", "\u{2019}"),
                QuoteType::DoubleQuote => ("\u{201C}", "\u{201D}"),
            };
            out.push_str(open);
            stack.push(Node::Literal(close));
            push_inlines(content, stack);
        }
        Inline::Emph(content)
        | Inline::Underline(content)
        | Inline::Strong(content)
        | Inline::Strikeout(content)
        | Inline::Superscript(content)
        | Inline::Subscript(content)
        | Inline::SmallCaps(content)
        | Inline::Cite(_, content)
        | Inline::Link(_, content, _)
        | Inline::Image(_, content, _) => push_inlines(content, stack),
    }
}

fn push_block<'a>(block: &'a Block, stack: &mut Vec<Node<'a>>) {
    match block {
        Block::Plain(inlines) | Block::Para(inlines) | Block::Header(_, _, inlines) => {
            push_inlines(inlines, stack);
        }
        Block::LineBlock(lines) => {
            for line in lines.iter().rev() {
                push_inlines(line, stack);
            }
        }
        Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => {}
        Block::BlockQuote(blocks) | Block::Div(_, blocks) => push_blocks(blocks, stack),
        Block::OrderedList(_, items) | Block::BulletList(items) => {
            for item in items.iter().rev() {
                push_blocks(item, stack);
            }
        }
        Block::DefinitionList(items) => {
            for (term, definitions) in items.iter().rev() {
                for definition in definitions.iter().rev() {
                    push_blocks(definition, stack);
                }
                push_inlines(term, stack);
            }
        }
        Block::Figure(_, caption, blocks) => {
            push_blocks(blocks, stack);
            push_caption(caption, stack);
        }
        Block::Table(table) => {
            push_rows(&table.foot.rows, stack);
            for body in table.bodies.iter().rev() {
                push_rows(&body.body, stack);
                push_rows(&body.head, stack);
            }
            push_rows(&table.head.rows, stack);
            push_caption(&table.caption, stack);
        }
    }
}

fn push_caption<'a>(caption: &'a Caption, stack: &mut Vec<Node<'a>>) {
    push_blocks(&caption.long, stack);
    if let Some(short) = &caption.short {
        push_inlines(short, stack);
    }
}

fn push_rows<'a>(rows: &'a [Row], stack: &mut Vec<Node<'a>>) {
    for row in rows.iter().rev() {
        for cell in row.cells.iter().rev() {
            push_blocks(&cell.content, stack);
        }
    }
}

fn push_meta<'a>(value: &'a MetaValue, out: &mut Text, stack: &mut Vec<Node<'a>>) {
    match value {
        MetaValue::MetaMap(map) => stack.extend(map.values().rev().map(Node::Meta)),
        MetaValue::MetaList(values) => stack.extend(values.iter().rev().map(Node::Meta)),
        MetaValue::MetaBool(true) => out.push_str("true"),
        MetaValue::MetaBool(false) => out.push_str("false"),
        MetaValue::MetaString(text) => out.push_str(text),
        MetaValue::MetaInlines(inlines) => push_inlines(inlines, stack),
        MetaValue::MetaBlocks(blocks) => push_blocks(blocks, stack),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // pandoc.utils.stringify of Pandoc 3.13.
    #[test]
    fn a_raw_html_line_break_gives_a_space() {
        let raw = |format: &str, text: &str| {
            vec![
                Inline::Str("a".into()),
                Inline::RawInline(Format(format.into()), text.into()),
                Inline::Str("b".into()),
            ]
        };
        assert_eq!(stringify(&raw("html", "<br>")), "a b");
        assert_eq!(stringify(&raw("html", "<br class=x>")), "a b");
        assert_eq!(stringify(&raw("html", "<BR>")), "ab");
        assert_eq!(stringify(&raw("html5", "<br>")), "ab");
        assert_eq!(stringify(&raw("html", " <br>")), "ab");
    }

    // Pandoc 3.13, `-f markdown+gfm_auto_identifiers`: `[x]{.emoji data-emoji=foo}` gives `foo`,
    // and a second class or attribute keeps the text `x`.
    #[test]
    fn only_an_exact_emoji_span_gives_its_alias() {
        let span = |classes: &[&str], attributes: &[(&str, &str)]| {
            Inline::Span(
                Attr {
                    id: String::new(),
                    classes: classes.iter().map(|c| c.to_string()).collect(),
                    attributes: attributes
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                },
                vec![Inline::Str("x".into())],
            )
        };
        assert_eq!(
            identifier_text(
                &[span(&["emoji"], &[("data-emoji", "foo")])],
                Purpose::CommonmarkIdentifier
            ),
            "foo"
        );
        assert_eq!(
            identifier_text(
                &[span(&["emoji", "b"], &[("data-emoji", "foo")])],
                Purpose::CommonmarkIdentifier
            ),
            "x"
        );
        assert_eq!(
            identifier_text(
                &[span(&["emoji"], &[("data-emoji", "foo"), ("k", "v")])],
                Purpose::CommonmarkIdentifier
            ),
            "x"
        );
        assert_eq!(
            identifier_text(
                &[span(&["emoji"], &[("data-emojo", "foo")])],
                Purpose::CommonmarkIdentifier
            ),
            "x"
        );
        assert_eq!(
            identifier_text(
                &[span(&["emojo"], &[("data-emoji", "foo")])],
                Purpose::CommonmarkIdentifier
            ),
            "x"
        );
        assert_eq!(
            stringify(&[span(&["emoji"], &[("data-emoji", "foo")])]),
            "x"
        );
    }
}
