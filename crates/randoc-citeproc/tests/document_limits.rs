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
