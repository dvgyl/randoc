//! Whole documents through the citation filter against the oracle.
//! `tests/replay/document.txt` holds, for each case, a style, the input
//! document (Pandoc JSON from the oracle's Markdown reader) and the
//! document that Pandoc 3.13 gave with `--citeproc`
//! (`tools/document_cases.py`). The cases cover sorting, disambiguation,
//! collapse, cite positions in a note style, the bibliography, links,
//! locators, `nocite` and inline references.

#![allow(clippy::unwrap_used, clippy::panic)]

use randoc_citeproc::filter::{Hooks, process_citations};
use randoc_core::{Context, PureHost};

fn context(style: &str) -> Context {
    let mut host = PureHost::default();
    let file = |name: &str| std::fs::read(format!("tests/replay/{name}")).unwrap();
    host.files.insert("s.csl".into(), style.as_bytes().to_vec());
    host.files
        .insert("refs.json".into(), file("document-refs.json"));
    host.files
        .insert("ab.json".into(), file("document-abbreviations.json"));
    host.files.insert(
        "dd/csl/locales/locales-en-US.xml".into(),
        file("document-locale.xml"),
    );
    let mut context = Context::new(Box::new(host));
    context.state.user_data_dir = Some("dd".into());
    context
}

#[test]
fn each_document_gives_the_oracle_output() {
    let text = std::fs::read_to_string("tests/replay/document.txt").unwrap();
    let mut failures = Vec::new();
    let mut count = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let [id, style, input, expected] = line.split('\t').collect::<Vec<_>>()[..] else {
            panic!("bad line");
        };
        count += 1;
        let mut doc = randoc_ast::json::from_json(input).unwrap();
        let expected = randoc_ast::json::from_json(expected).unwrap();
        let mut context = context(style);
        let mut find = |context: &mut Context, name: &str| {
            context.host().file_exists(name).then(|| name.to_owned())
        };
        let mut yaml = |_: &mut Context, _: &str, _: &str| Ok(Vec::new());
        let hooks = Hooks {
            find_resource: &mut find,
            yaml_references: &mut yaml,
        };
        process_citations(&mut context, &mut doc, hooks).unwrap();
        if doc != expected {
            failures.push(format!(
                "{id}\n  got:      {}\n  expected: {}",
                randoc_ast::json::to_json(&doc),
                randoc_ast::json::to_json(&expected)
            ));
        }
    }
    assert!(count >= 24);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
