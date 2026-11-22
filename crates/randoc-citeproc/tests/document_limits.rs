//! Large and deep documents through the citation filter: the time of
//! inputs that repeat one construct (standing order 21c) and deep styles
//! on a thread with a 2 MB stack (standing order 21b).

#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::panic)]

use std::time::Duration;

use randoc_ast::{Block, Citation, CitationMode, Inline, Meta, MetaValue, Pandoc};
use randoc_citeproc::filter::{Hooks, process_citations};
use randoc_core::{Context, PureHost};

const NS: &str = "xmlns=\"http://purl.org/net/xbiblio/csl\"";

fn cite(ids: &[String]) -> Inline {
    let citations = ids
        .iter()
        .map(|id| Citation {
            id: id.clone(),
            prefix: Vec::new(),
            suffix: Vec::new(),
            mode: CitationMode::NormalCitation,
            note_num: 1,
            hash: 0,
        })
        .collect();
    Inline::Cite(citations, vec![Inline::Str("x".into())])
}

/// Runs the filter on one paragraph of citations.
fn run(
    style: &str,
    refs: &str,
    citations: Vec<Vec<String>>,
    max_depth: usize,
) -> Result<Pandoc, String> {
    let mut host = PureHost::default();
    let locale = std::fs::read("tests/replay/document-locale.xml").unwrap();
    host.files.insert("s.csl".into(), style.as_bytes().to_vec());
    host.files
        .insert("refs.json".into(), refs.as_bytes().to_vec());
    host.files
        .insert("dd/csl/locales/locales-en-US.xml".into(), locale);
    let mut context = Context::new(Box::new(host));
    context.state.user_data_dir = Some("dd".into());
    context.limits.max_nesting_depth = max_depth;
    let mut meta = Meta::default();
    meta.insert("csl", MetaValue::MetaString("s.csl".into()));
    meta.insert("bibliography", MetaValue::MetaString("refs.json".into()));
    let mut doc = Pandoc {
        meta,
        blocks: vec![Block::Para(citations.iter().map(|ids| cite(ids)).collect())],
    };
    let mut find = |context: &mut Context, name: &str| {
        context.host().file_exists(name).then(|| name.to_owned())
    };
    let mut yaml = |_: &mut Context, _: &str, _: &str| Ok(Vec::new());
    let hooks = Hooks {
        find_resource: &mut find,
        yaml_references: &mut yaml,
    };
    process_citations(&mut context, &mut doc, hooks).map_err(|e| e.to_string())?;
    Ok(doc)
}

fn author_date(options: &str) -> String {
    format!(
        "<style {NS} class=\"in-text\"><citation {options}><sort><key variable=\"author\"/></sort>\
         <layout delimiter=\"; \"><group delimiter=\" \"><names variable=\"author\"><name form=\"short\"/></names>\
         <date variable=\"issued\"><date-part name=\"year\"/></date></group></layout></citation>\
         <bibliography><sort><key variable=\"author\"/><key variable=\"title\"/></sort><layout>\
         <text variable=\"title\"/></layout></bibliography></style>"
    )
}

fn references(n: usize, same: bool) -> String {
    let items: Vec<String> = (0..n)
        .map(|i| {
            let family = if same {
                "Smith".to_owned()
            } else {
                format!("F{}", (i * 7919) % n)
            };
            format!(
                "{{\"id\":\"r{i}\",\"type\":\"book\",\"author\":[{{\"family\":\"{family}\"}}],\
                 \"issued\":{{\"date-parts\":[[2000]]}},\"title\":\"T{i}\"}}"
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn ids(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("r{i}")).collect()
}

fn cpu_time() -> Duration {
    nix::time::clock_gettime(nix::time::ClockId::CLOCK_PROCESS_CPUTIME_ID)
        .unwrap()
        .into()
}

fn check(name: &str, sizes: (usize, usize), make: &dyn Fn(usize)) {
    let time = |n| {
        let start = cpu_time();
        make(n);
        cpu_time().saturating_sub(start)
    };
    let small = time(sizes.0);
    let large = time(sizes.1);
    eprintln!("{name}: {} in {small:?}, {} in {large:?}", sizes.0, sizes.1);
    assert!(
        large < small * 10 + Duration::from_millis(200),
        "{name}: {small:?} then {large:?}"
    );
}

#[test]
fn large_documents_take_linear_time() {
    // References with the same author and year get year suffixes past `zz`.
    let suffixes = author_date("disambiguate-add-year-suffix=\"true\" collapse=\"year-suffix\"");
    check("year suffixes", (300, 1_200), &|n| {
        let doc = run(&suffixes, &references(n, true), vec![ids(n)], 128).unwrap();
        assert!(
            randoc_ast::json::to_json(&doc).contains("2000a")
                && randoc_ast::json::to_json(&doc).contains("aa")
        );
    });
    // One citation with many items to group and collapse.
    let collapse = author_date("collapse=\"year\" cite-group-delimiter=\", \"");
    check("one long citation", (500, 2_000), &|n| {
        run(&collapse, &references(n, false), vec![ids(n)], 128).unwrap();
    });
    // Many citations and a sorted bibliography.
    let plain = author_date("");
    check("many citations", (500, 2_000), &|n| {
        let citations = ids(n).into_iter().map(|id| vec![id]).collect();
        run(&plain, &references(n, false), citations, 128).unwrap();
    });
}

fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn deep_styles_give_the_depth_error_on_a_small_stack() {
    let nested = |levels: usize| {
        format!(
            "<style {NS} class=\"in-text\"><citation disambiguate-add-year-suffix=\"true\" collapse=\"year\">\
             <layout>{}<names variable=\"author\"/><date variable=\"issued\"><date-part name=\"year\"/></date>{}</layout>\
             </citation><bibliography><layout>{}<text variable=\"title\"/>{}</layout></bibliography></style>",
            "<group>".repeat(levels),
            "</group>".repeat(levels),
            "<group>".repeat(levels),
            "</group>".repeat(levels)
        )
    };
    let shallow = nested(20);
    let result = on_small_stack(move || run(&shallow, &references(3, true), vec![ids(3)], 64));
    assert!(result.is_ok(), "{result:?}");
    let deep = nested(5_000);
    let result = on_small_stack(move || run(&deep, &references(3, true), vec![ids(3)], 64));
    assert!(result.is_err());
}

/// References with the same family name and different given names, so
/// that each name of the global given-name rule needs a hint.
fn same_family(n: usize, authors: usize) -> String {
    let items: Vec<String> = (0..n)
        .map(|i| {
            let names: Vec<String> = (0..authors)
                .map(|a| format!("{{\"family\":\"Smith\",\"given\":\"G{i}x{a}\"}}"))
                .collect();
            format!(
                "{{\"id\":\"r{i}\",\"type\":\"book\",\"author\":[{}],\
                 \"issued\":{{\"date-parts\":[[2000]]}},\"title\":\"T{i}\"}}",
                names.join(",")
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// GPT 6.2 Sol r-b98611b-16bf516 findings 1 and 6: the global given-name
/// rule shares one names map (each reference had a copy of the map of
/// all names), and the hints count the initials once.
#[test]
fn the_global_given_name_rule_takes_linear_time() {
    let style = author_date(
        "disambiguate-add-givenname=\"true\" givenname-disambiguation-rule=\"all-names\"",
    );
    check("same family names", (400, 1_600), &|n| {
        let citations = ids(n).into_iter().map(|id| vec![id]).collect();
        let doc = run(&style, &same_family(n, 1), citations, 128).unwrap();
        let json = randoc_ast::json::to_json(&doc);
        assert!(json.contains("G0x0") && json.contains(&format!("G{}x0", n - 1)));
    });
}

/// Finding 6: `disambiguate-add-names` extends the key of each item by one
/// name in each step (each step compared the first `n` names again).
#[test]
fn adding_names_takes_linear_time_in_the_number_of_names() {
    let style =
        author_date("disambiguate-add-names=\"true\" et-al-min=\"2\" et-al-use-first=\"1\"");
    check("long author lists", (200, 1_600), &|authors| {
        // The two items differ only in the last author.
        let refs: Vec<String> = ["Alpha", "Beta"]
            .iter()
            .enumerate()
            .map(|(i, last)| {
                let mut names: Vec<String> = (1..authors)
                    .map(|a| format!("{{\"family\":\"Smith\",\"given\":\"G{a}\"}}"))
                    .collect();
                names.push(format!("{{\"family\":\"{last}\"}}"));
                format!(
                    "{{\"id\":\"r{i}\",\"type\":\"book\",\"author\":[{}],\
                     \"issued\":{{\"date-parts\":[[2000]]}},\"title\":\"T{i}\"}}",
                    names.join(",")
                )
            })
            .collect();
        let refs = format!("[{}]", refs.join(","));
        let doc = run(&style, &refs, vec![ids(2)], 128).unwrap();
        let json = randoc_ast::json::to_json(&doc);
        assert!(json.contains("Alpha") && json.contains("Beta"));
    });
}

/// Takes a chain of `Emph` apart on the heap; the derived `Drop` recurses.
fn take_apart(mut inlines: Vec<Inline>) {
    while let Some(inline) = inlines.pop() {
        if let Inline::Emph(inner) = inline {
            inlines = inner;
        }
    }
}

fn deep_emph(levels: usize) -> Vec<Inline> {
    let mut inlines = vec![cite(&ids(1))];
    for _ in 0..levels {
        inlines = vec![Inline::Emph(inlines)];
    }
    inlines
}

/// Finding 2: the filter checks the depth of the document before its walks
/// recurse, in the blocks and in the metadata, on a thread with a 2 MiB
/// stack.
#[test]
fn a_deep_document_gives_the_depth_error_on_a_small_stack() {
    for in_meta in [false, true] {
        let result = on_small_stack(move || {
            let mut host = PureHost::default();
            let locale = std::fs::read("tests/replay/document-locale.xml").unwrap();
            host.files
                .insert("s.csl".into(), author_date("").into_bytes());
            host.files
                .insert("refs.json".into(), references(1, false).into_bytes());
            host.files
                .insert("dd/csl/locales/locales-en-US.xml".into(), locale);
            let mut context = Context::new(Box::new(host));
            context.state.user_data_dir = Some("dd".into());
            context.limits.max_nesting_depth = 128;
            let mut meta = Meta::default();
            meta.insert("csl", MetaValue::MetaString("s.csl".into()));
            meta.insert("bibliography", MetaValue::MetaString("refs.json".into()));
            let mut blocks = Vec::new();
            if in_meta {
                meta.insert("title", MetaValue::MetaInlines(deep_emph(16_384)));
            } else {
                blocks.push(Block::Para(deep_emph(16_384)));
            }
            let mut doc = Pandoc { meta, blocks };
            let mut find = |context: &mut Context, name: &str| {
                context.host().file_exists(name).then(|| name.to_owned())
            };
            let mut yaml = |_: &mut Context, _: &str, _: &str| Ok(Vec::new());
            let hooks = Hooks {
                find_resource: &mut find,
                yaml_references: &mut yaml,
            };
            let result =
                process_citations(&mut context, &mut doc, hooks).map_err(|e| e.to_string());
            if let Some(MetaValue::MetaInlines(inlines)) = doc.meta.0.remove("title") {
                take_apart(inlines);
            }
            for block in std::mem::take(&mut doc.blocks) {
                if let Block::Para(inlines) = block {
                    take_apart(inlines);
                }
            }
            result
        });
        assert_eq!(
            result,
            Err("the document is nested too deeply for the citations".to_owned()),
            "in the metadata: {in_meta}"
        );
    }
}
