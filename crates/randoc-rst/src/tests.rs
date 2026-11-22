use randoc_ast::{Attr, Block, Inline, Meta, MetaValue, Pandoc, Target};
use randoc_core::{
    Context, Error, FormatSpec, LogMessage, PureHost, ReaderOptions, Sources, WriterOptions,
};

fn read(input: &str, context: &mut Context) -> randoc_core::Result<Pandoc> {
    let mut options = ReaderOptions::default();
    options.extensions = FormatSpec::parse("rst")
        .extensions()
        .expect("RST extensions");
    super::reader::read(&Sources::from_text(input), &options, context)
}

#[test]
fn explicit_targets_and_section_links_use_their_own_identifiers() {
    let pandoc = read(
        ".. _first:\n.. _second:\n\nSection title\n-------------\n\nfirst_ second_ `Section title`_",
        &mut Context::pure(),
    )
    .expect("RST document");
    let text = |s: &str| Inline::Str(s.into());
    let link = |label: Vec<Inline>, target: &str| {
        Inline::Link(Attr::default(), label, Target::new(target, ""))
    };
    let label = vec![text("Section"), Inline::Space, text("title")];
    let mut heading = label.clone();
    heading.push(Inline::Span(Attr::new("first", vec![], vec![]), vec![]));
    assert_eq!(
        pandoc.blocks,
        vec![
            Block::Header(1, Attr::new("second", vec![], vec![]), heading),
            Block::Para(vec![
                link(vec![text("first")], "#first"),
                Inline::Space,
                link(vec![text("second")], "#second"),
                Inline::Space,
                link(label, "#section-title"),
            ]),
        ]
    );
}

#[test]
fn standalone_metadata_and_controls_use_the_default_template() {
    let mut meta = Meta::default();
    meta.0.insert(
        "title".into(),
        MetaValue::MetaInlines(vec![Inline::Strong(vec![Inline::Str("Title".into())])]),
    );
    meta.0
        .insert("subtitle".into(), MetaValue::MetaString("Sub".into()));
    let template = randoc_templates::Template::compile(
        "default.rst",
        include_str!("../../randoc-data/data/templates/default.rst"),
        128,
        &mut |_| Err::<String, ()>(()),
    )
    .expect("default template");
    let mut options = WriterOptions::default();
    options.template = Some(template);
    options.table_of_contents = true;
    options.number_sections = true;
    let doc = Pandoc {
        meta,
        blocks: vec![Block::Para(vec![Inline::Str("Body.".into())])],
    };
    let result = super::writer::write(&doc, &options, &mut Context::pure()).expect("RST output");
    assert_eq!(
        result.trim_end(),
        "=========\n**Title**\n=========\n---\nSub\n---\n\n\n.. contents::\n   :depth: 3\n..\n\n.. section-numbering::\n\nBody."
    );
}

#[test]
fn includes_use_the_host_and_stop_on_a_cycle() {
    let mut host = PureHost::default();
    host.files
        .insert("a.rst".into(), b".. include:: ./b.rst\n\nA".to_vec());
    host.files
        .insert("b.rst".into(), b".. include:: ./a.rst\n\nB".to_vec());
    let mut context = Context::new(Box::new(host));
    let document = read(".. include:: a.rst", &mut context).expect("bounded include");
    assert_eq!(document.blocks.len(), 2);
    assert!(
        context
            .log()
            .iter()
            .any(|log| matches!(log, LogMessage::CircularReference(..)))
    );
}

#[test]
fn includes_share_the_input_budget() {
    let input = ".. include:: a.rst\n\n.. include:: a.rst\n";
    let mut host = PureHost::default();
    host.files.insert("a.rst".into(), b"Hello".to_vec());
    let mut context = Context::new(Box::new(host));
    context.limits.max_input_size = input.len() as u64 + 5;
    assert!(matches!(read(input, &mut context), Err(Error::Parse(_))));
}

#[test]
fn substitution_and_target_cycles_complete() {
    let mut context = Context::pure();
    let text = ".. |a| replace:: |b|\n\n.. |b| replace:: |a|\n\n|a|\n\n.. _a: b_\n.. _b: a_\n\na_";
    let document = read(text, &mut context).expect("bounded references");
    assert_eq!(document.blocks.len(), 2);
    assert_eq!(
        context
            .log()
            .iter()
            .filter(|log| matches!(log, LogMessage::CircularReference(..)))
            .count(),
        2
    );
}

#[test]
fn nesting_fails_on_a_two_mebibyte_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut input = String::new();
            for depth in 0..200 {
                input.push_str(&"  ".repeat(depth));
                input.push_str("- item\n\n");
            }
            assert!(matches!(
                read(&input, &mut Context::pure()),
                Err(Error::Parse(_))
            ));
        })
        .expect("test thread")
        .join()
        .expect("bounded parser stack");
}

#[test]
fn table_cells_share_a_budget_including_spans_and_padding() {
    for input in [
        "=== ===\na   b\nc   d\n=== ===",
        "+---+---+\n| a | b |\n+---+---+\n| c     |\n+-------+",
        ".. csv-table::\n\n   a,b\n   c",
        ".. list-table::\n\n   * - a\n     - b\n   * - c",
    ] {
        let mut context = Context::pure();
        context.limits.max_table_cells = 3;
        assert!(
            matches!(read(input, &mut context), Err(Error::Parse(_))),
            "{input}"
        );
    }
}
