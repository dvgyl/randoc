//! The evaluation of one citation against the oracle. `tests/replay/eval.txt`
//! holds, for each case, the item identifier, a style whose citation layout
//! uses `text`, `group` and `choose`, and the citation inlines that Pandoc
//! 3.13 gave with the bibliography `tests/replay/eval-refs.json`
//! (`tools/eval.lua`). The test reads the bibliography with the CSL JSON
//! reader of this crate, evaluates and renders the citation, and applies the
//! last steps of `citeproc`.

#![allow(clippy::unwrap_used, clippy::panic)]

use randoc_ast::{Block, Inline};
use randoc_citeproc::eval::{Citation, CitationItem, eval_citation};
use randoc_citeproc::json::Json;
use randoc_citeproc::output::{self, RenderOptions};
use randoc_citeproc::reference::{Reference, Val, read_reference};
use randoc_citeproc::types::{ItemType, Locale, Style};
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

/// 15 references (personal, literal, particle, Byzantine and other names) x
/// 30 `names` layouts (options of `name`, `et-al`, `label`, `substitute`,
/// `editortranslator`, the count form).
#[test]
fn each_name_gives_the_oracle_inlines() {
    replay(
        include_str!("replay/names.txt"),
        include_str!("replay/names-refs.json"),
        440,
    );
}

/// 6 quote sets of a style locale (none, German, French, Japanese, Danish,
/// two-character marks) x punctuation in quotes or not x 16 quoted texts in
/// values, affixes, terms and delimited groups, and reference titles
/// (`tools/quote_cases.py`, `tools/quotes.lua`; review citeproc-R3b-05).
#[test]
fn style_text_uses_the_quotes_of_the_locale() {
    replay(
        include_str!("replay/quotes.txt"),
        include_str!("replay/quotes-refs.json"),
        990,
    );
}

/// 4 references x 42 layouts: variable names in other letter cases in
/// `text`, `number`, `label` and conditions (Codex citeproc-R3b-02), dates
/// with `circa` and no parts (R3b-03), and letters that are new in Unicode
/// 16.0 in each text case (R3b-08) (`tools/variable_case_cases.py`).
#[test]
fn variable_names_compare_without_case() {
    replay(
        include_str!("replay/case.txt"),
        include_str!("replay/case-refs.json"),
        160,
    );
}

/// The locale that the merged locale of each style falls back to.
fn base_locale() -> Locale {
    Locale {
        language: Some(randoc_citeproc::parse_lang("en-US").unwrap()),
        punctuation_in_quote: Some(true),
        ..Locale::default()
    }
}

/// The rendered inlines of one citation, after the last steps of
/// `citeproc` and the class step of the filter.
fn render(style: &Style, locale: &Locale, citation: &Citation) -> Vec<Inline> {
    let evaluated = eval_citation(style, locale, citation, 128).unwrap();
    // Pandoc turns `link-bibliography` on by default.
    let options = RenderOptions {
        link_bibliography: true,
        ..RenderOptions::default()
    };
    let rendered = output::render_output(options, locale, &evaluated.output, 128).unwrap();
    let mut got = output::finish(locale, rendered, 128).unwrap();
    drop_quoted_class(&mut got);
    got
}

/// One item of the form `TYPE^ID^LABEL^LOCATOR^PREFIX^SUFFIX`; an empty
/// field is no value.
fn item<'a>(spec: &str, refs: &'a [Reference<Vec<Inline>>]) -> CitationItem<'a> {
    let fields: Vec<&str> = spec.split('^').collect();
    let [kind, id, label, locator, prefix, suffix] = fields[..] else {
        panic!("bad item: {spec}")
    };
    let text = |t: &str| (!t.is_empty()).then(|| t.to_owned());
    let inlines = |t: &str| (!t.is_empty()).then(|| output::from_text(t));
    CitationItem {
        kind: match kind {
            "A" => ItemType::AuthorOnly,
            "S" => ItemType::SuppressAuthor,
            _ => ItemType::NormalCite,
        },
        label: text(label),
        locator: text(locator),
        prefix: inlines(prefix),
        suffix: inlines(suffix),
        ..CitationItem::new(id, refs.iter().find(|r| r.id == id))
    }
}

/// 15 references x 30 citation layouts x 9 citation forms
/// (`tools/citation_cases.py`, `tools/citation.lua`). The oracle reads the
/// Markdown citation of the first field; the test gives the library the
/// items of the second field, the form that the `--citeproc` filter (a
/// later unit) makes from it.
#[test]
fn each_citation_form_gives_the_oracle_inlines() {
    let locale_xml = include_str!("replay/citation-locale.xml").trim_end();
    let base = base_locale();
    let refs = references(include_str!("replay/citation-refs.json"), &base);
    let mut failures = Vec::new();
    let mut count = 0;
    for line in include_str!("replay/citation.txt")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let fields: Vec<&str> = line.split('\t').collect();
        let [form, items, attrs, layout, expected] = fields[..] else {
            panic!("bad line: {line}")
        };
        count += 1;
        let style_text = format!(
            "<style xmlns=\"http://purl.org/net/xbiblio/csl\" class=\"in-text\" version=\"1.0\" default-locale=\"en-US\">{locale_xml}<macro name=\"m\"><text variable=\"publisher\" font-weight=\"bold\"/></macro><citation><layout {attrs}>{layout}</layout></citation></style>"
        );
        let style = randoc_citeproc::parse_style(&mut Context::pure(), &style_text).unwrap();
        let locale = style.locales[0].clone().merge(&base);
        let citation = Citation {
            items: items.split('|').map(|spec| item(spec, &refs)).collect(),
            ..Citation::default()
        };
        let got = render(&style, &locale, &citation);
        let expected = plain(expected);
        if got != expected {
            failures.push(format!(
                "{form} <layout {attrs}>{layout}\n  expected {expected:?}\n  got      {got:?}"
            ));
        }
    }
    assert!(count >= 4_000, "{count} cases");
    assert!(
        failures.is_empty(),
        "{} of {count} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn replay(replay: &str, refs: &str, minimum: usize) {
    let base = base_locale();
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
        let citation = Citation {
            items: vec![CitationItem::new(id, refs.iter().find(|r| r.id == id))],
            ..Citation::default()
        };
        let got = render(&style, &locale, &citation);
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
