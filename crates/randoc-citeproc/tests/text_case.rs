//! The text cases of CSL output against the oracle. `tests/replay/text-case.txt`
//! holds, for each text case and title, the inlines of the title and the
//! inlines of the citation that Pandoc 3.13 gave (`tools/text_case.lua`, with
//! a style whose layout is one `text` element with the `text-case`).

#![allow(clippy::unwrap_used, clippy::panic)]

use randoc_ast::{Block, Inline};
use randoc_citeproc::case_transform::add_text_case;
use randoc_citeproc::types::TextCase;

fn plain(json: &str) -> Vec<Inline> {
    let doc = randoc_ast::json::from_json(json).unwrap();
    match doc.blocks.into_iter().next() {
        Some(Block::Plain(inlines)) => inlines,
        _ => Vec::new(),
    }
}

fn text_case(name: &str) -> TextCase {
    match name {
        "lowercase" => TextCase::Lowercase,
        "uppercase" => TextCase::Uppercase,
        "capitalize-first" => TextCase::CapitalizeFirst,
        "capitalize-all" => TextCase::CapitalizeAll,
        "sentence" => TextCase::SentenceCase,
        "title" => TextCase::TitleCase,
        other => panic!("text case {other}"),
    }
}

#[test]
fn each_text_case_gives_the_oracle_inlines() {
    let replay = include_str!("replay/text-case.txt");
    let mut failures = Vec::new();
    let mut count = 0;
    for line in replay
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let mut fields = line.split('\t');
        let (Some(case), Some(input), Some(expected)) =
            (fields.next(), fields.next(), fields.next())
        else {
            panic!("bad line: {line}")
        };
        count += 1;
        let got = add_text_case(None, text_case(case), plain(input), 128).unwrap();
        let expected = plain(expected);
        if got != expected {
            failures.push(format!(
                "{case}: {input}\n  expected {expected:?}\n  got      {got:?}"
            ));
        }
    }
    assert!(count > 100, "{count} cases");
    assert!(
        failures.is_empty(),
        "{} of {count} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
