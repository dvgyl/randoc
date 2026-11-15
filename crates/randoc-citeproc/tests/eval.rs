//! The evaluation of one citation against the oracle. `tests/replay/eval.txt`
//! holds, for each case, the item identifier, a style whose citation layout
//! uses `text`, `group` and `choose`, and the citation inlines that Pandoc
//! 3.13 gave with the bibliography `tests/replay/eval-refs.json`
//! (`tools/eval.lua`). The test reads the bibliography with the CSL JSON
//! reader of this crate, evaluates and renders the citation, and applies the
//! last steps of `citeproc`.

#![allow(clippy::unwrap_used, clippy::panic)]

use randoc_ast::{Block, Inline};
use randoc_citeproc::eval::eval_citation;
use randoc_citeproc::json::Json;
use randoc_citeproc::output::{self, RenderOptions};
use randoc_citeproc::reference::{Reference, Val, read_reference};
use randoc_citeproc::types::Locale;
use randoc_core::Context;

fn plain(json: &str) -> Vec<Inline> {
    let doc = randoc_ast::json::from_json(json).unwrap();
    match doc.blocks.into_iter().next() {
        Some(Block::Plain(inlines)) => inlines,
        _ => Vec::new(),
    }
}

/// The `--citeproc` filter of Pandoc (unit 5) gives a `csl-quoted` span an
/// empty class list; the oracle output shows that step.
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
            | Inline::Link(_, parts, _)
            | Inline::Underline(parts) => drop_quoted_class(parts),
            _ => {}
        }
    }
}

fn references(text: &str, locale: &Locale) -> Vec<Reference<Vec<Inline>>> {
    let tape = randoc_ast::json::parse_value(text, 128).unwrap();
    let Json::Array(items) = Json::from_value(tape.root()) else {
        panic!("array")
    };
    items
        .iter()
        .map(|item| {
            let reference = read_reference(item, 128).unwrap();
            Reference {
                id: reference.id,
                kind: reference.kind,
                variables: reference
                    .variables
                    .into_iter()
                    .map(|(k, v)| {
                        let v = match v {
                            Val::Fancy(markup) => {
                                Val::Fancy(output::markup_to_inlines(locale, &markup, 128).unwrap())
                            }
                            Val::Text(t) => Val::Text(t),
                            Val::Num(n) => Val::Num(n),
                            Val::Names(n) => Val::Names(n),
                            Val::Date(d) => Val::Date(d),
                        };
                        (k, v)
                    })
                    .collect(),
            }
        })
        .collect()
}

#[test]
fn each_citation_gives_the_oracle_inlines() {
    replay(
        include_str!("replay/eval.txt"),
        include_str!("replay/eval-refs.json"),
        30,
    );
}

#[test]
fn each_number_and_label_gives_the_oracle_inlines() {
    replay(
        include_str!("replay/number.txt"),
        include_str!("replay/number-refs.json"),
        100,
    );
}

#[test]
fn each_date_gives_the_oracle_inlines() {
    replay(
        include_str!("replay/date.txt"),
        include_str!("replay/date-refs.json"),
        300,
    );
}

fn replay(replay: &str, refs: &str, minimum: usize) {
    let base = Locale {
        language: Some(randoc_citeproc::parse_lang("en-US").unwrap()),
        punctuation_in_quote: Some(true),
        ..Locale::default()
    };
    let refs = references(refs, &base);
    let mut failures = Vec::new();
    let mut count = 0;
    for line in replay
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let mut fields = line.split('\t');
        let (Some(id), Some(style_text), Some(expected)) =
            (fields.next(), fields.next(), fields.next())
        else {
            panic!("bad line: {line}")
        };
        count += 1;
        let style = randoc_citeproc::parse_style(&mut Context::pure(), style_text).unwrap();
        let locale = style.locales[0].clone().merge(&base);
        let reference = refs.iter().find(|r| r.id == id);
        let evaluated = eval_citation(&style, &locale, &[(id, reference)], 128).unwrap();
        // Pandoc turns `link-bibliography` on by default.
        let options = RenderOptions {
            link_bibliography: true,
            ..RenderOptions::default()
        };
        let rendered = output::render_output(options, &locale, &evaluated.output, 128).unwrap();
        let mut got = output::finish(&locale, rendered, 128).unwrap();
        drop_quoted_class(&mut got);
        let expected = plain(expected);
        if got != expected {
            failures.push(format!(
                "{id} {style_text}\n  expected {expected:?}\n  got      {got:?}"
            ));
        }
    }
    assert!(count >= minimum, "{count} cases");
    assert!(
        failures.is_empty(),
        "{} of {count} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
