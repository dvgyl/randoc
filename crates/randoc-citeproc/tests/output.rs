//! The rendering of CSL output against the oracle. `tests/replay/output.txt`
//! holds styles whose citation layout has only `text value` and `group`
//! elements, with the citation inlines that Pandoc 3.13 gave
//! (`tools/output.lua`). The test builds the output tree as evaluation does
//! for these elements, renders it and applies the last steps of `citeproc`.

#![allow(clippy::unwrap_used, clippy::panic)]

use randoc_ast::{Block, Inline};
use randoc_citeproc::output::{self, Output, RenderOptions, Tag};
use randoc_citeproc::types::{Element, ElementType, Formatting, Locale, TextType};
use randoc_core::Context;

fn plain(json: &str) -> Vec<Inline> {
    let doc = randoc_ast::json::from_json(json).unwrap();
    match doc.blocks.into_iter().next() {
        Some(Block::Plain(inlines)) => inlines,
        _ => Vec::new(),
    }
}

/// `eElement` for `text value` and `group` (no variables, so no group is
/// suppressed).
fn build(locale: &Locale, element: &Element) -> Output {
    match &element.kind {
        ElementType::Text(TextType::Value(value)) => output::formatted(
            element.formatting.clone(),
            vec![Output::Literal(
                output::parse_csl_json(locale, value, 128).unwrap(),
            )],
        ),
        ElementType::Group(_, elements) => output::formatted(
            element.formatting.clone(),
            elements.iter().map(|e| build(locale, e)).collect(),
        ),
        other => panic!("element {other:?}"),
    }
}

/// The `--citeproc` filter of Pandoc (unit 5, not in this module) gives a
/// `csl-quoted` span an empty class list after the quotes of the locale
/// went in; the oracle output shows that step.
fn drop_quoted_class(inlines: &mut [Inline]) {
    for inline in inlines {
        if let Inline::Span(attr, _) = inline
            && attr.classes == ["csl-quoted"]
        {
            attr.classes.clear();
        }
        match inline {
            Inline::Span(_, parts)
            | Inline::Emph(parts)
            | Inline::Strong(parts)
            | Inline::Underline(parts)
            | Inline::SmallCaps(parts)
            | Inline::Superscript(parts)
            | Inline::Subscript(parts) => drop_quoted_class(parts),
            _ => {}
        }
    }
}

/// The locale of the oracle for these cases: en-US moves punctuation into
/// quotes and has the default quotation marks.
fn locale() -> Locale {
    Locale {
        punctuation_in_quote: Some(true),
        ..Locale::default()
    }
}

#[test]
fn each_layout_renders_the_oracle_inlines() {
    let replay = include_str!("replay/output.txt");
    let locale = locale();
    let mut failures = Vec::new();
    let mut count = 0;
    for line in replay
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let Some((style_text, expected)) = line.split_once('\t') else {
            panic!("bad line: {line}")
        };
        count += 1;
        let style = randoc_citeproc::parse_style(&mut Context::pure(), style_text).unwrap();
        let layout = &style.citation;
        let elements = layout.elements.iter().map(|e| build(&locale, e)).collect();
        let item = Output::Tagged(
            Tag::Item {
                author_only: false,
                id: "a".into(),
            },
            Box::new(output::formatted(Formatting::default(), elements)),
        );
        let citation = output::formatted(layout.formatting.clone(), vec![item]);
        let rendered =
            output::render_output(RenderOptions::default(), &locale, &citation, 128).unwrap();
        let mut got = output::finish(&locale, rendered, 128).unwrap();
        drop_quoted_class(&mut got);
        let expected = plain(expected);
        if got != expected {
            failures.push(format!(
                "{style_text}\n  expected {expected:?}\n  got      {got:?}"
            ));
        }
    }
    assert!(count >= 50, "{count} cases");
    assert!(
        failures.is_empty(),
        "{} of {count} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
