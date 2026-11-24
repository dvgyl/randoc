use std::sync::{Arc, Mutex};

use randoc_ast::{Attr, Block, ColWidth, Inline, Meta, MetaValue, Pandoc, Target};
use randoc_core::{
    Context, Error, FormatSpec, Host, LogMessage, PureHost, ReaderOptions, Source, Sources,
    WriterOptions,
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
    let a = "Intro\n\n.. include:: b.rst\n\nA";
    host.files.insert("a.rst".into(), a.as_bytes().to_vec());
    host.files
        .insert("b.rst".into(), b".. include:: ./a.rst\n\nB".to_vec());
    let mut context = Context::new(Box::new(host));
    assert!(matches!(
        read(".. include:: a.rst", &mut context),
        Err(Error::Parse(message)) if message == "Include file loop at \"./a.rst\" (line 5, column 1)"
    ));

    let sources = Sources(vec![Source {
        name: "a.rst".into(),
        text: a.into(),
    }]);
    let error = super::reader::read(&sources, &ReaderOptions::default(), &mut context)
        .expect_err("input file cycle");
    assert_eq!(error.exit_code(), 64);
    assert_eq!(
        error.to_string(),
        "Include file loop at \"./a.rst\" (line 5, column 1)"
    );
}

#[test]
fn include_cycles_use_the_active_source() {
    let a = ".. include:: b.rst\n\nA";
    let mut host = PureHost::default();
    host.files.insert("b.rst".into(), b"B".to_vec());
    let sources = Sources(vec![
        Source {
            name: "a.rst".into(),
            text: a.into(),
        },
        Source {
            name: "b.rst".into(),
            text: "B".into(),
        },
    ]);
    let doc = super::reader::read(
        &sources,
        &ReaderOptions::default(),
        &mut Context::new(Box::new(host)),
    )
    .expect("include another input file");
    assert_eq!(doc.blocks, [para("B"), para("A"), para("B")]);

    let mut host = PureHost::default();
    host.files.insert(
        "a.rst".into(),
        b".. include:: a.rst\n   :literal:\n\nA".to_vec(),
    );
    let error = read(".. include:: a.rst", &mut Context::new(Box::new(host)))
        .expect_err("nested literal cycle");
    assert_eq!(
        error.to_string(),
        "Include file loop at \"a.rst\" (line 4, column 1)"
    );
}

#[test]
fn nested_includes_use_the_directory_of_the_source_name() {
    let first = ".. include:: sub/second.rst\n\n.. include:: sibling.rst";
    let mut host = PureHost::default();
    host.files.extend([
        ("docs/first.rst".into(), first.as_bytes().to_vec()),
        (
            "docs/sub/second.rst".into(),
            b".. include:: sibling.rst\n\n.. include:: fallback.rst".to_vec(),
        ),
        ("docs/sub/sibling.rst".into(), b"Wrong directory".to_vec()),
        ("sub/sibling.rst".into(), b"Inner".to_vec()),
        ("docs/sibling.rst".into(), b"Outer".to_vec()),
        ("resources/sibling.rst".into(), b"Wrong directory".to_vec()),
        ("resources/fallback.rst".into(), b"Fallback".to_vec()),
    ]);
    let mut context = Context::new(Box::new(host));
    context.state.resource_path = vec!["resources".into(), ".".into()];
    for source in [
        Source {
            name: "".into(),
            text: ".. include:: docs/first.rst".into(),
        },
        Source {
            name: "docs/first.rst".into(),
            text: first.into(),
        },
    ] {
        let doc = super::reader::read(
            &Sources(vec![source]),
            &ReaderOptions::default(),
            &mut context,
        )
        .expect("nested includes");
        assert_eq!(doc.blocks, [para("Inner"), para("Outer")]);
    }
    assert_eq!(
        context.log(),
        [(), ()].map(|()| LogMessage::CouldNotLoadIncludeFile(
            "fallback.rst".into(),
            randoc_core::SourcePos::new("sub/second.rst", 5, 1)
        ))
    );
}

fn para(text: &str) -> Block {
    Block::Para(vec![Inline::Str(text.into())])
}

#[test]
fn internal_targets_end_before_adjacent_name_separators() {
    let target = Inline::Span(
        Attr::new("a", vec![], vec![]),
        vec![Inline::Str("a".into())],
    );
    let doc = read("_a-_a-_-b", &mut Context::pure()).expect("internal targets");
    assert_eq!(
        doc.blocks,
        [Block::Para(vec![
            target.clone(),
            Inline::Str("-".into()),
            target,
            Inline::Str("-_-b".into()),
        ])]
    );
}

#[derive(Default)]
struct IncludeHost {
    files: PureHost,
    reads: Arc<Mutex<Vec<String>>>,
    sandboxed: bool,
}

impl Host for IncludeHost {
    fn is_sandboxed(&self) -> bool {
        self.sandboxed
    }

    fn working_directory(&mut self) -> std::io::Result<randoc_core::Text> {
        Ok("/workspace".into())
    }

    fn read_file_limited(&mut self, path: &str, max_bytes: usize) -> std::io::Result<Vec<u8>> {
        self.reads.lock().expect("read log").push(path.to_owned());
        self.files.read_file_limited(path, max_bytes)
    }
}

#[test]
fn diamond_includes_read_shared_text_once() {
    let mut host = IncludeHost::default();
    host.files.files.extend([
        ("left.rst".into(), b".. include:: shared.rst".to_vec()),
        ("right.rst".into(), b".. include:: ./shared.rst".to_vec()),
        ("shared.rst".into(), b"Shared".to_vec()),
    ]);
    let reads = Arc::clone(&host.reads);
    let mut context = Context::new(Box::new(host));
    let doc = read(
        ".. include:: left.rst\n\n.. include:: right.rst",
        &mut context,
    )
    .expect("diamond includes");
    assert_eq!(doc.blocks, [para("Shared"), para("Shared")]);
    assert_eq!(
        *reads.lock().expect("read log"),
        ["left.rst", "shared.rst", "right.rst"]
    );
}

#[test]
fn relative_and_absolute_includes_share_the_cache() {
    for (first, second) in [
        ("shared.rst", "/workspace/shared.rst"),
        ("/workspace/shared.rst", "shared.rst"),
    ] {
        let mut host = IncludeHost::default();
        host.files.files.insert(first.into(), b"Shared".to_vec());
        let reads = Arc::clone(&host.reads);
        let input = format!(".. include:: {first}\n\n.. include:: {second}");
        let doc = read(&input, &mut Context::new(Box::new(host))).expect("cached includes");
        assert_eq!(doc.blocks, [para("Shared"), para("Shared")]);
        assert_eq!(*reads.lock().expect("read log"), [first]);
    }
}

#[test]
fn missing_includes_warn_even_if_the_path_mentions_a_limit() {
    let mut context = Context::pure();
    let doc = read(
        ".. include:: exceeds.rst\n   :literal:\n\nAfter",
        &mut context,
    )
    .expect("missing include");
    assert_eq!(
        doc.blocks,
        [
            Block::CodeBlock(
                Attr::new("", vec![], vec![("literal".into(), "".into())]),
                "".into()
            ),
            para("After"),
        ]
    );
    assert_eq!(
        context.log(),
        &[LogMessage::CouldNotLoadIncludeFile(
            "exceeds.rst".into(),
            randoc_core::SourcePos::new("", 4, 1)
        )]
    );
}

#[test]
fn sandbox_directives_cannot_read_named_inputs() {
    let mut host = IncludeHost {
        sandboxed: true,
        ..IncludeHost::default()
    };
    host.files
        .files
        .insert("named.rst".into(), b"Named".to_vec());
    let reads = Arc::clone(&host.reads);
    let mut context = Context::new(Box::new(host));
    context.state.input_files.push("named.rst".into());
    let doc = read(".. include:: named.rst\n\nAfter", &mut context).expect("sandbox include");
    assert_eq!(doc.blocks, [para("After")]);
    assert!(matches!(
        context.log(),
        [LogMessage::CouldNotLoadIncludeFile(..)]
    ));
    let error =
        read(".. csv-table::\n   :file: named.rst", &mut context).expect_err("sandbox CSV file");
    assert_eq!(error.exit_code(), 99);
    assert!(reads.lock().expect("read log").is_empty());
}

#[test]
fn missing_csv_file_is_a_resource_error() {
    let error = read(
        ".. csv-table::\n   :file: missing.csv",
        &mut Context::pure(),
    )
    .expect_err("missing CSV file");
    assert!(matches!(&error, Error::ResourceNotFound(file) if file == "missing.csv"));
    assert_eq!(error.exit_code(), 99);
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

#[test]
fn empty_targets_consume_depth_only_when_they_wrap_blocks() {
    let mut input = (0..130)
        .map(|i| format!(".. _target-{i}:\n"))
        .collect::<String>();
    let mut context = Context::pure();
    assert!(
        read(&input, &mut context)
            .expect("empty targets")
            .blocks
            .is_empty()
    );
    let with_heading = format!("{input}\nHeading\n=======");
    let doc = read(&with_heading, &mut context).expect("heading targets");
    assert!(
        matches!(doc.blocks.as_slice(), [Block::Header(_, attr, label)]
        if attr.id == "target-129" && label.len() == 130)
    );

    input.push_str("\nBody");
    assert!(matches!(read(&input, &mut context), Err(Error::Parse(_))));
}

#[test]
fn the_last_definition_of_a_heading_target_wins() {
    let heading = "Paper\n=====\n\n";
    let target = ".. _Paper: https://example.com\n\n";
    for (definitions, expected) in [
        (format!("{target}{heading}"), "#paper"),
        (format!("{heading}{heading}"), "#paper-1"),
        (format!("{heading}{target}{heading}"), "#paper-1"),
        (format!("{heading}{heading}{target}"), "https://example.com"),
    ] {
        let input = format!("{definitions}Paper_");
        let doc = read(&input, &mut Context::pure()).expect("section references");
        assert!(matches!(doc.blocks.last(), Some(Block::Para(inlines))
            if matches!(inlines.as_slice(), [Inline::Link(_, _, reference)] if reference.url == expected)));
    }
}

#[test]
fn identifier_styles_require_auto_identifiers() {
    for style in ["ascii_identifiers", "gfm_auto_identifiers"] {
        let mut options = ReaderOptions::default();
        options.extensions = FormatSpec::parse(&format!("rst-auto_identifiers+{style}"))
            .extensions()
            .expect("RST extensions");
        let doc = super::reader::read(
            &Sources::from_text("Title\n====="),
            &options,
            &mut Context::pure(),
        )
        .expect("heading");
        assert!(matches!(doc.blocks.as_slice(), [Block::Header(_, attr, _)] if attr.id.is_empty()));
    }
}

fn write(blocks: Vec<Block>, options: &WriterOptions) -> String {
    let doc = Pandoc {
        meta: Meta::default(),
        blocks,
    };
    super::writer::write(&doc, options, &mut Context::pure()).expect("RST output")
}

#[test]
fn standalone_headings_close_gaps_in_the_section_hierarchy() {
    let mut options = WriterOptions::default();
    options.template =
        Some(
            randoc_templates::Template::compile("body.rst", "$body$", 8, &mut |_| {
                Err::<String, ()>(())
            })
            .expect("template"),
        );
    let headings = [("A", 2), ("B", 6), ("C", 3), ("D", 5)].map(|(title, level)| {
        Block::Header(level, Attr::default(), vec![Inline::Str(title.into())])
    });
    assert_eq!(
        write(headings.to_vec(), &options).trim_end(),
        "A\n=\n\nB\n-\n\nC\n-\n\nD\n~"
    );
}

#[test]
fn generic_admonition_title_stays_in_the_body() {
    let doc = read(".. admonition:: Custom\n\n   Text", &mut Context::pure()).expect("admonition");
    assert_eq!(
        write(doc.blocks, &WriterOptions::default()),
        ".. admonition::\n\n   Custom\n\n   Text\n"
    );
}

#[test]
fn smart_writer_escapes_literal_punctuation_runs() {
    let mut options = WriterOptions::default();
    options.extensions = FormatSpec::parse("rst+smart")
        .extensions()
        .expect("RST extensions");
    let punctuation = ["a--b", "a---b", "a...b", "a....b", "a–b", "a—b", "a…b"].map(para);
    assert_eq!(
        write(punctuation.to_vec(), &options),
        "a\\--b\n\na\\-\\--b\n\na\\...b\n\na\\.\\...b\n\na--b\n\na---b\n\na...b\n"
    );
}

#[test]
fn image_scale_formats_percent_and_drops_invalid_dimensions() {
    let doc = read(
        ".. image:: picture.png\n   :width: 50%\n   :height: 1e3px\n   :scale: 50 %\n\n.. image:: picture.png\n   :width: 3.9px\n   :height: 1.234567inch\n   :scale: 0.5",
        &mut Context::pure(),
    ).expect("scaled images");
    let sizes = |block: &Block| match block {
        Block::Para(inlines) => match inlines.as_slice() {
            [Inline::Image(attr, _, _)] => attr.attributes.clone(),
            _ => unreachable!("image"),
        },
        _ => unreachable!("paragraph"),
    };
    assert_eq!(sizes(&doc.blocks[0]), [("width".into(), "25.0%".into())]);
    assert_eq!(
        sizes(&doc.blocks[1]),
        [
            ("width".into(), "2px".into()),
            ("height".into(), "0.61728in".into()),
        ]
    );
}

#[test]
fn table_widths_use_columns_and_zip_with_the_table() {
    let mut options = ReaderOptions::default();
    options.columns = 42;
    for (widths, expected) in [
        ("10", vec![ColWidth::ColWidth(10.0 / 42.0)]),
        (
            "10, 20 30",
            vec![
                ColWidth::ColWidth(10.0 / 42.0),
                ColWidth::ColWidth(20.0 / 42.0),
            ],
        ),
        (
            "bad 0",
            vec![ColWidth::ColWidthDefault, ColWidth::ColWidthDefault],
        ),
        ("auto", vec![]),
    ] {
        let input =
            format!(".. table::\n   :widths: {widths}\n\n   === ===\n   abc xyz\n   === ===");
        let doc = super::reader::read(&Sources::from_text(input), &options, &mut Context::pure())
            .expect("table");
        let [Block::Table(table)] = doc.blocks.as_slice() else {
            unreachable!("table");
        };
        assert_eq!(
            table.col_specs.iter().map(|s| s.width).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(table.bodies[0].body[0].cells.len(), 2);
    }
}

#[test]
fn reference_link_labels_keep_their_case() {
    let mut options = WriterOptions::default();
    options.reference_links = true;
    let links = [("a", "one"), ("A", "two")].map(|(label, url)| {
        Inline::Link(
            Attr::default(),
            vec![Inline::Str(label.into())],
            Target::new(url, ""),
        )
    });
    assert_eq!(
        write(vec![Block::Para(links.to_vec())], &options),
        "`a`_\\ `A`_\n\n.. _a: one\n.. _A: two\n"
    );
}

#[test]
fn image_number_counts_generated_names() {
    let image = |label: &str, url| {
        Inline::Image(
            Attr::default(),
            (!label.is_empty())
                .then(|| Inline::Str(label.into()))
                .into_iter()
                .collect(),
            Target::new(url, ""),
        )
    };
    let doc = write(
        vec![Block::Para(vec![
            image("label", "one"),
            Inline::Space,
            image("", "two"),
            Inline::Space,
            image("label", "three"),
        ])],
        &WriterOptions::default(),
    );
    assert_eq!(
        doc,
        "|label| |image1| |image2|\n\n.. |label| image:: one\n.. |image1| image:: two\n.. |image2| image:: three\n"
    );
}

#[test]
fn substitution_and_note_loops_keep_the_placeholder() {
    let mut context = Context::pure();
    let doc = read(
        ".. |a| replace:: |a|\n\n|a|\n\n[1]_\n\n.. [1] note [1]_",
        &mut context,
    )
    .expect("circular references");
    let expected_note = Inline::Note(vec![Block::Para(vec![
        Inline::Str("note".into()),
        Inline::Link(
            Attr::default(),
            vec![Inline::Str("1".into())],
            Target::new("##NOTE##1", ""),
        ),
    ])]);
    assert_eq!(
        doc.blocks,
        [
            Block::Para(vec![Inline::Span(
                Attr::default(),
                vec![Inline::Str("|a|".into())]
            )]),
            Block::Para(vec![expected_note]),
        ]
    );
    assert!(matches!(context.log(), [LogMessage::CircularReference(label, _)] if label == "a"));
}

#[test]
fn inline_substitutions_keep_nested_links_without_warnings() {
    let span = |text: &str, name: &str| {
        Inline::Span(
            Attr::default(),
            vec![
                Inline::Str(text.into()),
                Inline::Space,
                Inline::Link(
                    Attr::default(),
                    vec![Inline::Str(format!("|{name}|"))],
                    Target::new(format!("##SUBST##|{name}|"), ""),
                ),
            ],
        )
    };
    for (definitions, name, expected) in [
        (
            ".. |a| replace:: begin |a|",
            "a",
            [span("begin", "a"), span("begin", "a")],
        ),
        (
            ".. |a| replace:: begin |b|\n\n.. |b| replace:: end |a|",
            "b",
            [span("begin", "b"), span("end", "a")],
        ),
    ] {
        let input = format!("{definitions}\n\nText |a|.\n\nText |{name}|.");
        let mut context = Context::pure();
        let doc = read(&input, &mut context).expect("inline substitutions");
        assert_eq!(
            doc.blocks,
            expected.map(|replacement| Block::Para(vec![
                Inline::Str("Text".into()),
                Inline::Space,
                replacement,
                Inline::Str(".".into()),
            ]))
        );
        assert!(context.log().is_empty());
    }
}

#[test]
fn bird_tracks_require_a_blank_line_before_prose() {
    let mut options = ReaderOptions::default();
    options.extensions = FormatSpec::parse("rst+literate_haskell")
        .extensions()
        .expect("RST extensions");
    let doc = super::reader::read(
        &Sources::from_text("> x\n> y\nz\n\n> code\n\nText"),
        &options,
        &mut Context::pure(),
    )
    .expect("literate Haskell");
    assert!(matches!(doc.blocks.as_slice(),
        [Block::Para(text), Block::CodeBlock(attr, code), Block::Para(_)]
        if text.contains(&Inline::SoftBreak) && attr.has_class("literate") && code == "code"));
}

#[test]
fn a_role_and_literal_can_follow_a_backtick_delimiter() {
    let code = |text: &str| Inline::Code(Attr::default(), text.into());
    let input = "word``x``:doc:`a```, joined\n\nwith ``rest.";
    assert_eq!(
        read(input, &mut Context::pure()).unwrap().blocks,
        vec![Block::Para(vec![
            Inline::Str("word".into()),
            code("x"),
            Inline::Code(
                Attr::new(
                    "",
                    vec!["interpreted-text".into()],
                    vec![("role".into(), "doc".into())]
                ),
                "a".into(),
            ),
            code(", joined  with"),
            Inline::Str("rest.".into())
        ])]
    );
}

#[test]
fn a_single_backtick_can_close_before_a_symbol() {
    let input = "`with spaces `%s` and";
    assert_eq!(
        read(input, &mut Context::pure()).unwrap().blocks,
        vec![Block::Para(vec![
            Inline::Span(
                Attr::new("", vec!["title-ref".into()], vec![]),
                vec![
                    Inline::Str("with".into()),
                    Inline::Space,
                    Inline::Str("spaces".into()),
                    Inline::Space
                ],
            ),
            Inline::Str("%s`".into()),
            Inline::Space,
            Inline::Str("and".into())
        ])]
    );
}

#[test]
#[ignore = "requires local corpus files and Pandoc reference fixtures"]
fn wave8_fixture_replay() {
    let root = std::path::PathBuf::from(std::env::var_os("RANDOC_W8_REPLAY").unwrap());
    let corpus = std::path::PathBuf::from(std::env::var_os("RANDOC_CORPUS").unwrap());
    let mut failures = Vec::new();
    for id in ["sphinx-usage-domains-python", "sphinx-usage-referencing"] {
        let filename = corpus.join(format!("{id}.rst"));
        let file = std::fs::read_to_string(&filename).unwrap();
        let mut context = Context::pure();
        let actual = read(&file, &mut context).unwrap();
        let expected = randoc_ast::json::from_json(
            &std::fs::read_to_string(root.join(format!("{id}.json"))).unwrap(),
        )
        .unwrap();
        if actual != expected || !context.log().is_empty() {
            failures.push(id);
        }
    }
    println!("RST corpus: {}/2", 2 - failures.len());
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn inset_overlined_title() {
    let doc = read("=======\n  Title\n=======\n\nBody.", &mut Context::pure()).expect("RST");
    assert!(matches!(
        doc.blocks.as_slice(),
        [Block::Header(1, attr, title), Block::Para(_)]
            if attr.id == "title" && *title == [Inline::Str("Title".into())]
    ));
}

#[test]
fn formatted_alt_uses_the_substitution_name() {
    let output = write(
        vec![Block::Para(vec![
            Inline::Str("See".into()),
            Inline::Space,
            Inline::Image(
                Attr::default(),
                vec![
                    Inline::Str("alt".into()),
                    Inline::Space,
                    Inline::Emph(vec![Inline::Str("text".into())]),
                ],
                Target::new("image.png", ""),
            ),
            Inline::Str(".".into()),
        ])],
        &WriterOptions::default(),
    );
    assert!(output.contains("See |alt text|."));
    assert!(output.contains(".. |alt text| image:: image.png"));
}

#[test]
fn grid_cells_keep_paragraphs_if_any_cell_has_a_list() {
    let input = "+-------+------+\n| A     | B    |\n+=======+======+\n| - one | Text |\n| - two |      |\n+-------+------+";
    let doc = read(input, &mut Context::pure()).expect("RST");
    assert!(matches!(doc.blocks.as_slice(), [Block::Table(table)]
        if matches!(table.head.rows[0].cells[0].content.as_slice(), [Block::Para(_)])
        && matches!(table.bodies[0].body[0].cells[0].content.as_slice(), [Block::BulletList(_)])
        && matches!(table.bodies[0].body[0].cells[1].content.as_slice(), [Block::Para(_)])));
}

#[test]
fn sphinx_citation_roles() {
    use randoc_ast::CitationMode;
    let doc = read(
        ":cite:t:`{see}smith{p. 1},jones` and :cite:p:`smith`.\n\n.. bibliography:: sources.bib",
        &mut Context::pure(),
    )
    .expect("RST");
    let [Block::Para(nodes), Block::Div(attr, bibliography)] = doc.blocks.as_slice() else {
        panic!("Expected a paragraph and bibliography");
    };
    assert_eq!(attr.id, "refs");
    assert!(bibliography.is_empty());
    let Inline::Cite(citations, label) = &nodes[0] else {
        panic!("Expected a citation");
    };
    assert_eq!(citations[0].id, "smith");
    assert_eq!(citations[0].mode, CitationMode::AuthorInText);
    assert_eq!(citations[0].prefix, [Inline::Str("see".into())]);
    assert_eq!(
        citations[0].suffix,
        [
            Inline::Str("p.".into()),
            Inline::Space,
            Inline::Str("1".into())
        ]
    );
    assert_eq!(citations[1].id, "jones");
    assert_eq!(citations[1].mode, CitationMode::NormalCitation);
    assert_eq!(label, &[Inline::Str("{see}smith{p. 1},jones".into())]);
    assert!(matches!(&nodes[4], Inline::Cite(citations, _)
        if citations[0].mode == CitationMode::NormalCitation));
}

#[test]
#[ignore = "Requires the Pandoc oracle."]
fn longtail_rst_oracle_parity() {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let oracle = std::env::var_os("RANDOC_ORACLE_PANDOC").expect("Pandoc path");
    let convert = |from, to, input: &str| {
        let mut child = Command::new(&oracle)
            .args(["-f", from, "-t", to, "--columns=80"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("Pandoc");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(input.as_bytes())
            .expect("input");
        let output = child.wait_with_output().expect("output");
        assert!(output.status.success());
        String::from_utf8(output.stdout).expect("UTF-8")
    };

    for source in [
        "=======\n  Title\n=======\n\nBody.",
        "+-------+------+\n| A     | B    |\n+=======+======+\n| - one | Text |\n| - two |      |\n+-------+------+",
        ":cite:t:`{see}smith{p. 1},jones` and :cite:p:`smith`.\n\n.. bibliography:: sources.bib",
    ] {
        let expected = convert("rst", "json", source);
        let actual = read(source, &mut Context::pure()).expect("RST");
        assert_eq!(
            randoc_ast::json::from_json(&expected).expect("Pandoc JSON"),
            actual,
            "{source}"
        );
    }

    let source = "See ![alt *text*](image.png).";
    let ast = convert("markdown", "json", source);
    let expected = convert("json", "rst", &ast);
    let doc = randoc_ast::json::from_json(&ast).expect("Pandoc JSON");
    let actual = write(doc.blocks, &WriterOptions::default());
    assert_eq!(expected.trim_end(), actual.trim_end());
}

#[test]
fn rst_quote_boundaries_and_citation_styles() {
    let cite = Inline::Cite(
        Vec::new(),
        vec![Inline::Emph(vec![Inline::Str("book".into())])],
    );
    let link = Inline::Link(
        Attr::default(),
        vec![Inline::Str("title".into())],
        Target::new("https://x.test", ""),
    );
    assert_eq!(
        write(
            vec![Block::Para(vec![
                Inline::Str("“".into()),
                link,
                Inline::Str("”".into()),
                Inline::Space,
                cite
            ])],
            &WriterOptions::default()
        ),
        "“\\ `title <https://x.test>`__\\ ” book\n",
    );
}
