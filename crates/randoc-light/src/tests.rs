use randoc_ast::{Attr, Block, Inline, Meta, MetaValue, Pandoc};
use randoc_core::{Context, Reader, ReaderOptions, Sources, WriterOptions, default_extensions};
use randoc_templates::Template;

use crate::{reader, writer};

fn read(name: &str, input: &str, context: &mut Context) -> randoc_core::Result<Pandoc> {
    let Some(Reader::Text(reader)) = reader(name) else {
        return Err(randoc_core::Error::UnknownReader(name.into()));
    };
    let mut options = ReaderOptions::default();
    options.extensions = default_extensions(name);
    reader.read(&Sources::from_text(input), &options, context)
}

#[test]
fn registry_and_templates() {
    assert!(reader("vimdoc").is_none());
    assert!(reader("unknown").is_none());
    assert!(writer("unknown").is_none());
    for name in ["textile", "muse", "t2t", "haddock", "opml", "vimdoc"] {
        let writer = writer(name).expect("Missing writer.");
        assert!(writer.renders_template());
        let mut meta = Meta::default();
        meta.insert("title", MetaValue::MetaString("Title".into()));
        let doc = Pandoc {
            meta,
            blocks: vec![Block::Header(
                1,
                Attr::default(),
                vec![Inline::Str("Section".into())],
            )],
        };
        let mut options = WriterOptions::default();
        options.template = Some(
            Template::compile(
                name,
                randoc_data::default_template(name).expect("Missing template."),
                128,
                &mut |_| Err(()),
            )
            .expect("Cannot compile template."),
        );
        let text = writer
            .write(&doc, &options, &mut Context::pure())
            .expect("Cannot write document.")
            .into_bytes();
        assert!(
            text.windows(b"Section".len())
                .any(|bytes| bytes == b"Section"),
            "{name}"
        );
    }
}

#[test]
fn textile_padding_limits_only_generated_cells() {
    let mut context = Context::pure();
    context.limits.max_table_cells = 0;
    assert!(read("textile", "|a|b|\n|c|d|", &mut context).is_ok());
    assert!(read("textile", "|a|b|\n|c|", &mut context).is_err());
}

#[test]
fn input_and_expansion_limits() {
    let mut context = Context::pure();
    context.limits.max_input_size = 8;
    assert!(read("muse", "an oversized document", &mut context).is_err());
    context.limits.max_input_size = 1024;
    let notes = format!("{}\n\nfn1. {}", "x[1] ".repeat(50), "note ".repeat(100));
    assert!(read("textile", &notes, &mut context).is_err());
    assert!(read("textile", "x[1]\n\nfn1. x[1]", &mut context).is_ok());
}

#[test]
fn opml_uses_reader_extensions_and_reports_invalid_xml() {
    let mut context = Context::pure();
    let doc = read("opml", r#"<opml><body><other><outline text="&lt;b&gt;label&lt;/b&gt;" _note="A *note*"/></other></body></opml>"#, &mut context).expect("Cannot read outline.");
    assert!(
        matches!(doc.blocks.as_slice(), [Block::Header(1, _, label), Block::Para(_)] if matches!(label.as_slice(), [Inline::Strong(_)]))
    );
    let error = read("opml", "<opml><body>", &mut context).expect_err("Expected an XML error.");
    assert_eq!(error.exit_code(), 44);
}

fn on_small_stack(action: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(action)
        .expect("Cannot start test thread.")
        .join()
        .expect("Test thread failed.");
}

#[test]
fn readers_fit_a_two_mib_stack() {
    on_small_stack(|| {
        let mut context = Context::pure();
        let depth = context.limits.max_nesting_depth;
        let textile = (1..=depth)
            .map(|i| format!("{} item\n", "*".repeat(i)))
            .collect::<String>();
        assert!(read("textile", &textile, &mut context).is_ok());
        assert!(
            read(
                "textile",
                &format!("{textile}{} item", "*".repeat(depth + 1)),
                &mut context
            )
            .is_err()
        );
        for (name, prefix) in [("muse", " - "), ("t2t", "- ")] {
            let input = (0..depth)
                .map(|i| format!("{}{prefix}item\n", " ".repeat(i * 2)))
                .collect::<String>();
            assert!(read(name, &input, &mut context).is_ok(), "{name}");
            assert!(
                read(
                    name,
                    &format!("{input}{}{prefix}item", " ".repeat(depth * 2)),
                    &mut context
                )
                .is_err(),
                "{name}"
            );
        }
        assert!(
            read(
                "haddock",
                &format!("{}x", "/a ".repeat(10_000)),
                &mut context
            )
            .is_ok()
        );
        let nested_labels = |count: usize| {
            format!(
                "<opml><body>{}{}</body></opml>",
                "<outline text=\"x\">".repeat(count),
                "</outline>".repeat(count)
            )
        };
        assert!(read("opml", &nested_labels(depth - 2), &mut context).is_ok());
        assert!(read("opml", &nested_labels(depth - 1), &mut context).is_err());
        let markup = format!(
            "<opml><body><outline text=\"{}text{}\"/></body></opml>",
            "&lt;em&gt;".repeat(depth - 1),
            "&lt;/em&gt;".repeat(depth - 1)
        );
        assert!(read("opml", &markup, &mut context).is_ok());
    });
}

#[test]
fn readers_use_the_configured_depth() {
    on_small_stack(|| {
        let mut context = Context::pure();
        context.limits.max_nesting_depth = 160;
        let muse = |depth| {
            format!(
                "{}body{}",
                "<strong>".repeat(depth),
                "</strong>".repeat(depth)
            )
        };
        assert!(read("muse", &muse(160), &mut context).is_ok());
        assert!(read("muse", &muse(161), &mut context).is_err());
        let quotes = |depth| {
            format!(
                "{}body\n{}",
                "<quote>\n".repeat(depth),
                "</quote>\n".repeat(depth)
            )
        };
        assert!(read("muse", &quotes(160), &mut context).is_ok());
        assert!(read("muse", &quotes(161), &mut context).is_err());

        let textile = |depth: usize| {
            let mut text = String::from("Start[1]\n\n");
            for i in 1..depth {
                text.push_str(&format!("fn{i}. Note[{}]\n", i + 1));
            }
            text.push_str(&format!("fn{depth}. End."));
            text
        };
        assert!(read("textile", &textile(160), &mut context).is_ok());
        assert!(read("textile", &textile(161), &mut context).is_err());

        let quote = |depth| format!("{}Quoted", "\t".repeat(depth));
        assert!(read("t2t", &quote(160), &mut context).is_ok());
        assert!(read("t2t", &quote(161), &mut context).is_err());
    });
}

#[test]
fn unmatched_and_crossed_delimiters_make_progress() {
    on_small_stack(|| {
        for (format, input) in [
            ("textile", "*x "),
            ("muse", "<strong>x "),
            ("t2t", "[ a "),
            ("haddock", "< a "),
        ] {
            let text = format!("{}é]", input.repeat(20_000));
            assert!(
                read(format, &text, &mut Context::pure()).is_ok(),
                "{format}"
            );
        }
    });
}

#[test]
fn writers_fit_a_two_mib_stack() {
    on_small_stack(|| {
        let mut context = Context::pure();
        let mut blocks = vec![Block::Para(vec![Inline::Str("Body".into())])];
        for _ in 0..context.limits.max_nesting_depth {
            blocks = vec![Block::BlockQuote(blocks)];
        }
        let mut xs = vec![Inline::Str("Inline".into())];
        for _ in 0..context.limits.max_nesting_depth {
            xs = vec![Inline::Emph(xs)];
        }
        blocks.push(Block::Para(xs));
        let doc = Pandoc {
            meta: Meta::default(),
            blocks,
        };
        for name in ["textile", "muse", "t2t", "haddock", "opml", "vimdoc"] {
            let writer = writer(name).expect("Missing writer.");
            let options = WriterOptions::default();
            assert!(writer.write(&doc, &options, &mut context).is_ok(), "{name}");
            context.limits.max_nesting_depth -= 1;
            assert!(
                writer.write(&doc, &options, &mut context).is_err(),
                "{name}"
            );
            context.limits.max_nesting_depth += 1;
        }
    });
}

#[test]
fn definition_bodies_fit_a_two_mib_stack() {
    on_small_stack(|| {
        let mut context = Context::pure();
        let muse = |count| format!(" a{}", " :: a".repeat(count));
        let doc = read("muse", &muse(127), &mut context).expect("Cannot read definitions.");
        let mut blocks = doc.blocks.as_slice();
        for _ in 0..127 {
            let [Block::DefinitionList(items)] = blocks else {
                panic!("Expected a definition list.");
            };
            blocks = &items[0].1[0];
        }
        assert!(matches!(blocks, [Block::Para(_)]));
        let error = read("muse", &muse(130), &mut context).expect_err("Expected a depth error.");
        assert!(error.to_string().contains("nested too deeply"));

        let haddock = format!("{}x", "[a] ".repeat(127));
        let doc = read("haddock", &haddock, &mut context).expect("Cannot read definition.");
        let [Block::DefinitionList(items)] = doc.blocks.as_slice() else {
            panic!("Expected a definition list.");
        };
        assert!(matches!(items[0].1[0].as_slice(), [Block::Para(xs)] if xs.len() == 253));
    });
}

#[test]
fn textile_multiline_cells_fit_a_two_mib_stack() {
    on_small_stack(|| {
        let mut context = Context::pure();
        let depth = context.limits.max_nesting_depth;
        let table = |count| {
            let rows = (1..=count)
                .map(|level| format!("{} item", "*".repeat(level)))
                .collect::<Vec<_>>();
            format!("|{}|", rows.join("\n"))
        };
        let doc = read("textile", &table(depth - 1), &mut context).expect("Cannot read table.");
        let [Block::Table(table_block)] = doc.blocks.as_slice() else {
            panic!("Expected a table.");
        };
        assert!(matches!(
            table_block.bodies[0].body[0].cells[0].content.as_slice(),
            [Block::BulletList(_)]
        ));
        let error =
            read("textile", &table(depth), &mut context).expect_err("Expected a depth error.");
        assert!(error.to_string().contains("nested too deeply"));
    });
}

fn check_scaling(format: &str, pattern: &str, large: usize, check: impl Fn(&Pandoc, usize)) {
    use std::time::{Duration, Instant};
    let sample = |count| {
        let input = pattern.repeat(count);
        let start = Instant::now();
        let doc = read(format, &input, &mut Context::pure()).expect("Cannot read input.");
        let elapsed = start.elapsed();
        check(&doc, count);
        elapsed
    };
    let short = sample(large / 4);
    let long = sample(large);
    eprintln!(
        "{format}: {short:?} for {} repeats, {long:?} for {large} repeats.",
        large / 4
    );
    assert!(long < Duration::from_secs(1), "{format}: {long:?}");
    assert!(
        long < short * 10 + Duration::from_millis(30),
        "{format}: {short:?}, {long:?}"
    );
}

#[test]
fn t2t_autolinks_scan_at_word_starts() {
    check_scaling("t2t", "a(", 40_000, |doc, count| {
        assert!(matches!(
            doc.blocks.as_slice(),
            [Block::Para(xs)] if matches!(xs.as_slice(), [Inline::Str(text)] if text.len() == count * 2)
        ));
    });
}

#[test]
fn textile_unclosed_rows_scan_once() {
    check_scaling("textile", "|a\n", 20_000, |doc, count| {
        assert!(matches!(doc.blocks.as_slice(), [Block::Para(xs)] if xs.len() == count * 2 - 1));
    });
}

#[test]
fn writer_limits_indented_blocks_and_cells() {
    let mut context = Context::pure();
    context.limits.max_input_size = 16_384;
    let vimdoc = writer("vimdoc").expect("Missing writer.");
    let check_limit = |blocks, context: &mut Context| {
        let error = vimdoc
            .write(
                &Pandoc {
                    meta: Meta::default(),
                    blocks,
                },
                &WriterOptions::default(),
                context,
            )
            .expect_err("Expected an output limit error.");
        assert!(error.to_string().contains("output is too large"));
    };
    let quote = |block| (0..70).fold(block, |block, _| Block::BlockQuote(vec![block]));
    check_limit(
        vec![quote(Block::Para(
            (0..400)
                .flat_map(|_| [Inline::Str("x".into()), Inline::Space])
                .collect(),
        ))],
        &mut context,
    );
    check_limit(
        vec![Block::Table(Box::new(randoc_ast::Table {
            col_specs: vec![randoc_ast::ColSpec::default()],
            bodies: vec![randoc_ast::TableBody {
                body: vec![randoc_ast::Row {
                    cells: vec![randoc_ast::Cell {
                        content: vec![quote(Block::CodeBlock(Attr::default(), "x\n".repeat(400)))],
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }))],
        &mut context,
    );
}

#[test]
fn t2t_path_macros_use_context() {
    let mut context = Context::pure();
    context.state.input_files = vec!["src/doc.t2t".into()];
    context.state.output_file = Some("out/doc.native".into());
    let doc =
        read("t2t", "Wrote %%infile to %%outfile.", &mut context).expect("Cannot read macros.");
    assert!(matches!(
        doc.blocks.as_slice(),
        [Block::Para(xs)] if matches!(
            xs.as_slice(),
            [Inline::Str(_), Inline::Space, Inline::Str(input), Inline::Space, Inline::Str(_),
                Inline::Space, Inline::Str(output)]
                if input == "src/doc.t2t" && output == "out/doc.native."
        )
    ));
}

#[test]
fn t2t_date_macros_use_the_host_clock() -> randoc_core::Result<()> {
    let host = randoc_core::PureHost {
        environment: [("SOURCE_DATE_EPOCH".into(), "86400".into())].into(),
        ..Default::default()
    };
    let doc = read(
        "t2t",
        "%%date\n\nx %%date(%Y) y\n\n%%mtime\n\n%%mtime(%H)\n",
        &mut Context::new(Box::new(host)),
    )?;
    let date = Inline::Str("1970-01-02".into());
    assert_eq!(
        doc.blocks,
        [
            Block::Para(vec![date.clone()]),
            Block::Para(vec![
                Inline::Str("x".into()),
                Inline::Space,
                date,
                Inline::Space,
                Inline::Str("y".into())
            ]),
            Block::Para(vec![Inline::Str(String::new())]),
            Block::Para(vec![Inline::Str(String::new())]),
        ]
    );
    Ok(())
}
