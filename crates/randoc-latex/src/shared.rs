//! Utilities that other writers also need: URI decoding, number formatting
//! and image dimensions.

use randoc_ast::{Attr, Block, Caption, Inline, Row};

/// A test of one inline or of one block.
pub(crate) type Test<'t, T> = &'t dyn Fn(&T) -> bool;

/// Returns `true` when an element of the inlines passes a test, down to
/// `depth` levels. The search looks into notes, citations and each block.
pub(crate) fn inlines_any(
    inlines: &[Inline],
    depth: usize,
    inline_test: Test<'_, Inline>,
    block_test: Test<'_, Block>,
) -> bool {
    let Some(depth) = depth.checked_sub(1) else {
        return false;
    };
    let any = |items: &[Inline]| inlines_any(items, depth, inline_test, block_test);
    inlines.iter().any(|inline| {
        inline_test(inline)
            || match inline {
                Inline::Emph(items)
                | Inline::Underline(items)
                | Inline::Strong(items)
                | Inline::Strikeout(items)
                | Inline::Superscript(items)
                | Inline::Subscript(items)
                | Inline::SmallCaps(items)
                | Inline::Quoted(_, items)
                | Inline::Link(_, items, _)
                | Inline::Image(_, items, _)
                | Inline::Span(_, items) => any(items),
                Inline::Cite(citations, items) => {
                    any(items)
                        || citations
                            .iter()
                            .any(|citation| any(&citation.prefix) || any(&citation.suffix))
                }
                Inline::Note(blocks) => blocks_any(blocks, depth, inline_test, block_test),
                _ => false,
            }
    })
}

/// Returns `true` when an element of the blocks passes a test, down to
/// `depth` levels.
pub(crate) fn blocks_any(
    blocks: &[Block],
    depth: usize,
    inline_test: Test<'_, Inline>,
    block_test: Test<'_, Block>,
) -> bool {
    let Some(depth) = depth.checked_sub(1) else {
        return false;
    };
    let inlines = |items: &[Inline]| inlines_any(items, depth, inline_test, block_test);
    let any = |items: &[Block]| blocks_any(items, depth, inline_test, block_test);
    let caption =
        |caption: &Caption| caption.short.as_deref().is_some_and(inlines) || any(&caption.long);
    let rows = |rows: &[Row]| {
        rows.iter()
            .flat_map(|row| &row.cells)
            .any(|cell| any(&cell.content))
    };
    blocks.iter().any(|block| {
        block_test(block)
            || match block {
                Block::Plain(items) | Block::Para(items) | Block::Header(_, _, items) => {
                    inlines(items)
                }
                Block::LineBlock(lines) => lines.iter().any(|line| inlines(line)),
                Block::BlockQuote(items) | Block::Div(_, items) => any(items),
                Block::BulletList(items) | Block::OrderedList(_, items) => {
                    items.iter().any(|item| any(item))
                }
                Block::DefinitionList(items) => items.iter().any(|(term, definitions)| {
                    inlines(term) || definitions.iter().any(|definition| any(definition))
                }),
                Block::Figure(_, figure_caption, items) => caption(figure_caption) || any(items),
                Block::Table(table) => {
                    caption(&table.caption)
                        || rows(&table.head.rows)
                        || rows(&table.foot.rows)
                        || table
                            .bodies
                            .iter()
                            .any(|body| rows(&body.head) || rows(&body.body))
                }
                Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => false,
            }
    })
}

/// Decodes `%XX` escapes in a URI. Invalid escapes stay as they are.
pub fn unescape_uri(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(index + 1).and_then(|b| hex_value(*b)),
                bytes.get(index + 2).and_then(|b| hex_value(*b)),
            )
        {
            out.push(high * 16 + low);
            index += 3;
            continue;
        }
        out.push(byte);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|value| u8::try_from(value).ok())
}

/// Formats a number with at most five decimals and no trailing zeros, as
/// Pandoc's `showFl` does.
pub fn show_fl(value: f64) -> String {
    let text = format!("{value:.5}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        return "0".to_owned();
    }
    text.to_owned()
}

/// An image dimension from a `width` or `height` attribute.
#[derive(Debug, Clone, PartialEq)]
pub enum Dimension {
    /// Pixels: a number with no unit or with `px`.
    Pixel(f64),
    /// A percentage of the line width or the text height.
    Percent(f64),
    /// A number with a unit that the output keeps, for example `cm`.
    Unit(f64, &'static str),
}

/// The direction of a dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// The `width` attribute.
    Width,
    /// The `height` attribute.
    Height,
}

/// Reads the dimension of an image in one direction.
pub fn dimension(direction: Direction, attr: &Attr) -> Option<Dimension> {
    let key = match direction {
        Direction::Width => "width",
        Direction::Height => "height",
    };
    parse_dimension(attr.get(key)?)
}

/// Parses a dimension such as `50%`, `3cm` or `300`.
///
/// The grammar is the one of Pandoc 3.13: digits, an optional fraction (`.`
/// and digits), optional spaces and a unit. Pixels keep the integer part only.
pub fn parse_dimension(text: &str) -> Option<Dimension> {
    let integer_end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    if integer_end == 0 {
        return None;
    }
    let (integer, rest) = text.split_at(integer_end);
    let (fraction, rest) = match rest.strip_prefix('.') {
        Some(after) => {
            let end = after
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(after.len());
            if end == 0 {
                return None;
            }
            after.split_at(end)
        }
        None => ("", rest),
    };
    let value: f64 = if fraction.is_empty() {
        integer.parse().ok()?
    } else {
        format!("{integer}.{fraction}").parse().ok()?
    };
    match rest.trim_start_matches(' ') {
        "" | "px" => Some(Dimension::Pixel(integer.parse().ok()?)),
        "%" => Some(Dimension::Percent(value)),
        "cm" => Some(Dimension::Unit(value, "cm")),
        "mm" => Some(Dimension::Unit(value, "mm")),
        "in" | "inch" => Some(Dimension::Unit(value, "in")),
        "em" => Some(Dimension::Unit(value, "em")),
        "pt" => Some(Dimension::Unit(value, "pt")),
        "pc" => Some(Dimension::Unit(value, "pc")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescape_decodes_valid_escapes_only() {
        assert_eq!(unescape_uri("a%20b%zz%4"), "a b%zz%4");
        assert_eq!(unescape_uri("%C3%A9"), "é");
    }

    #[test]
    fn show_fl_drops_trailing_zeros() {
        assert_eq!(show_fl(3.125), "3.125");
        assert_eq!(show_fl(0.5), "0.5");
        assert_eq!(show_fl(3.0), "3");
        assert_eq!(show_fl(1.0 / 3.0), "0.33333");
    }

    #[test]
    fn dimensions_parse_units() {
        assert_eq!(parse_dimension("50%"), Some(Dimension::Percent(50.0)));
        assert_eq!(parse_dimension("300"), Some(Dimension::Pixel(300.0)));
        assert_eq!(parse_dimension("2inch"), Some(Dimension::Unit(2.0, "in")));
        assert_eq!(parse_dimension("2furlong"), None);
        assert_eq!(parse_dimension("cm"), None);
        // Oracle: these widths give no size.
        for bad in [".5in", "5.in", " 3cm", "3\tcm", "5PX", "+5"] {
            assert_eq!(parse_dimension(bad), None, "{bad}");
        }
        assert_eq!(parse_dimension("100.7px"), Some(Dimension::Pixel(100.0)));
        assert_eq!(parse_dimension("5 cm"), Some(Dimension::Unit(5.0, "cm")));
    }
}
