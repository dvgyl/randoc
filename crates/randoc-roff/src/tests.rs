#![allow(clippy::unwrap_used, clippy::expect_used)]

use randoc_ast::{
    Alignment, Attr, Block, ColSpec, Inline, MathType, Pandoc, Row, Table, TableBody,
};
use randoc_core::{
    Context, Error, Host, LogMessage, Output, PureHost, Reader, ReaderOptions, Source, Sources,
    WriterOptions, default_extensions,
};
use randoc_templates::Template;

fn read(format: &str, sources: &Sources, ctx: &mut Context) -> randoc_core::Result<Pandoc> {
    let Some(Reader::Text(reader)) = crate::reader(format) else {
        unreachable!()
    };
    let mut options = ReaderOptions::default();
    options.extensions = default_extensions(format);
    reader.read(sources, &options, ctx)
}

fn parse(format: &str, text: &str) -> Pandoc {
    read(format, &Sources::from_text(text), &mut Context::pure()).unwrap()
}

fn render(
    format: &str,
    doc: &Pandoc,
    options: &WriterOptions,
    ctx: &mut Context,
) -> randoc_core::Result<String> {
    crate::writer(format)
        .unwrap()
        .write(doc, options, ctx)
        .map(|output| match output {
            Output::Text(text) => text,
            Output::Binary(_) => unreachable!(),
        })
}

#[test]
fn registers_roff_directions() {
    assert!(crate::reader("man").is_some());
    assert!(crate::reader("mdoc").is_some());
    assert!(crate::reader("ms").is_none());
    assert!(crate::writer("man").unwrap().renders_template());
    assert!(crate::writer("ms").unwrap().renders_template());
    assert!(crate::writer("mdoc").is_none());
}

#[test]
fn man_preserves_previous_font_and_mdoc_discards_fonts() {
    let text = "\\fBbold \\fIitalic\\fP bold\\fR.";
    let man = parse("man", text);
    let mdoc = parse("mdoc", text);
    assert_eq!(
        man.blocks,
        vec![Block::Para(vec![
            Inline::Strong(vec![Inline::Str("bold".into())]),
            Inline::Space,
            Inline::Emph(vec![Inline::Str("italic".into())]),
            Inline::Space,
            Inline::Strong(vec![Inline::Str("bold".into())]),
            Inline::Str(".".into()),
        ])]
    );
    assert_eq!(
        mdoc.blocks,
        vec![Block::Para(super::escape::words("bold italic bold."))]
    );
}

#[test]
fn man_expands_nested_strings_and_macro_parameters() {
    let doc = parse(
        "man",
        r#".ds key payload
.ds payload left
.de pair
\*[\*[key]] \\$1 \\$2
..
.pair "a b" c
"#,
    );
    assert_eq!(
        doc.blocks,
        vec![Block::Para(super::escape::words("left a b c"))]
    );
}

#[test]
fn man_reports_a_missing_include_at_its_source() {
    let sources = Sources(vec![Source {
        name: "main.1".into(),
        text: ".TH FILE\n.so missing.1\nbody\n".into(),
    }]);
    let mut ctx = Context::pure();
    let doc = read("man", &sources, &mut ctx).unwrap();
    assert_eq!(
        doc.blocks,
        vec![Block::Para(vec![Inline::Str("body".into())])]
    );
    assert!(
        matches!(ctx.log(), [LogMessage::CouldNotLoadIncludeFile(file, pos)]
        if file == "missing.1" && pos.source == "main.1" && pos.line == 3)
    );
}

#[test]
fn man_reads_include_from_resource_path() {
    let mut host = PureHost::default();
    host.files
        .insert("docs/section.1".into(), b".SH Heading\n".to_vec());
    let mut ctx = Context::new(Box::new(host));
    ctx.state.resource_path = vec!["docs".into()];
    let doc = read("man", &Sources::from_text(".so section.1\n"), &mut ctx).unwrap();
    assert!(matches!(&doc.blocks[..], [Block::Header(1, attr, _)] if attr.id == "heading"));
    assert!(ctx.log().is_empty());
}

#[test]
fn rejects_short_inputs_that_expand_past_the_input_cap() {
    let mut ctx = Context::pure();
    ctx.limits.max_input_size = 128;
    let doc = ".ds s 0123456789012345678901234567890\n.ds e \\*s\\*s\\*s\\*s\\*s\n\\*e\n";
    assert!(matches!(
        read("man", &Sources::from_text(doc), &mut ctx),
        Err(Error::Parse(_))
    ));
    assert!(matches!(
        read("mdoc", &Sources::from_text("a".repeat(129)), &mut ctx),
        Err(Error::Parse(_))
    ));
}

#[test]
fn charges_empty_macro_replays() {
    let mut ctx = Context::pure();
    let doc = format!(
        ".de empty\n..\n.de a\n{}..\n.de b\n{}..\n.b\n",
        ".empty\n".repeat(80),
        ".a\n".repeat(300)
    );
    assert!(matches!(
        read("man", &Sources::from_text(doc), &mut ctx),
        Err(Error::Parse(_))
    ));
}

#[test]
fn rejects_ragged_tables_that_exceed_the_cell_limit() {
    let mut ctx = Context::pure();
    ctx.limits.max_table_cells = 5;
    for (format, text) in [
        (
            "mdoc",
            ".Bl -column\n.It a Ta b Ta c Ta d\n.It a\n.It b\n.El\n",
        ),
        ("man", ".TS\nl.\na\tb\tc\td\na\nb\n.TE\n"),
    ] {
        assert!(matches!(
            read(format, &Sources::from_text(text), &mut ctx),
            Err(Error::Parse(_))
        ));
    }
}

#[test]
fn writers_apply_the_cell_limit_before_padding_tables() {
    let doc = Pandoc {
        blocks: vec![Block::Table(Box::new(Table {
            col_specs: vec![ColSpec::default(); 4],
            bodies: vec![TableBody {
                body: vec![Row::default(); 3],
                ..TableBody::default()
            }],
            ..Table::default()
        }))],
        ..Pandoc::default()
    };
    let mut ctx = Context::pure();
    ctx.limits.max_table_cells = 8;
    for format in ["man", "ms"] {
        assert!(matches!(
            render(format, &doc, &WriterOptions::default(), &mut ctx),
            Err(Error::Parse(_))
        ));
    }
}

#[test]
fn man_writer_uses_math_inlines_and_restores_the_font() {
    let doc = Pandoc {
        blocks: vec![Block::Para(vec![Inline::Strong(vec![
            Inline::Code(Attr::default(), "-n".into()),
            Inline::Space,
            Inline::Math(MathType::InlineMath, "x^2".into()),
        ])])],
        ..Pandoc::default()
    };
    assert_eq!(
        render("man", &doc, &WriterOptions::default(), &mut Context::pure()).unwrap(),
        ".PP\n\\f[B]\\f[CB]\\-n\\f[B] \\f[BI]x\\f[B]^2^\\f[R]"
    );
}

#[test]
fn ms_writer_renders_eqn_and_highlighting() {
    let doc = Pandoc {
        blocks: vec![
            Block::Para(vec![Inline::Math(MathType::InlineMath, "x^2".into())]),
            Block::Para(vec![Inline::Math(MathType::DisplayMath, "y^2".into())]),
            Block::CodeBlock(super::escape::attr("rust"), "let n = 1;".into()),
        ],
        ..Pandoc::default()
    };
    let mut options = WriterOptions::default();
    options.template = Some(
        Template::compile::<()>(
            "default.ms",
            randoc_data::default_template("ms").unwrap(),
            64,
            &mut |_| Err(()),
        )
        .unwrap(),
    );
    let text = render("ms", &doc, &options, &mut Context::pure()).unwrap();
    assert!(text.contains(".EQ\ndelim @@\n.EN"));
    assert!(text.contains("@x sup 2@"));
    assert!(text.contains(".EQ\ny sup 2\n.EN"));
    assert!(text.contains("\\*[KeywordTok \"let\"]"), "{text}");
    assert!(text.contains(".ds KeywordTok "));
}

#[test]
fn man_reader_limits_recursion_on_a_two_mib_stack() {
    on_small_stack(|| {
        let mut host = PureHost::default();
        host.files
            .insert("docs/cycle.1".into(), b".so cycle.1\n".to_vec());
        let mut ctx = Context::new(Box::new(host));
        ctx.state.resource_path = vec!["docs".into()];
        for text in [
            ".de a\n.a\n..\n.a\n".into(),
            ".de a\n.b\n..\n.de b\n.a\n..\n.a\n".into(),
            ".ds a \\*b\n.ds b \\*a\n\\*a\n".into(),
            ".so cycle.1\n".into(),
            format!("{}text\n{}", ".RS\n".repeat(200), ".RE\n".repeat(200)),
        ] {
            assert!(matches!(
                read("man", &Sources::from_text(text), &mut ctx),
                Err(Error::Parse(_))
            ));
        }
    });
}

#[test]
fn mdoc_reader_limits_recursion_on_a_two_mib_stack() {
    on_small_stack(|| {
        let mut ctx = Context::pure();
        for text in [
            format!(
                "{}text\n{}",
                ".Bd -filled\n".repeat(250),
                ".Ed\n".repeat(250)
            ),
            format!(".No {}x\n", "Op ".repeat(250)),
        ] {
            assert!(matches!(
                read("mdoc", &Sources::from_text(text), &mut ctx),
                Err(Error::Parse(_))
            ));
        }
    });
}

#[test]
fn man_and_ms_writers_limit_recursion_on_a_two_mib_stack() {
    on_small_stack(|| {
        let mut ctx = Context::pure();
        let nested_blocks = (0..200)
            .fold(Block::Para(vec![Inline::Str("a".into())]), |block, _| {
                Block::BlockQuote(vec![block])
            });
        let nested_inlines = (0..200).fold(Inline::Str("a".into()), |inline, _| {
            Inline::Emph(vec![inline])
        });
        for blocks in [vec![nested_blocks], vec![Block::Para(vec![nested_inlines])]] {
            let doc = Pandoc {
                blocks,
                ..Pandoc::default()
            };
            for format in ["man", "ms"] {
                assert!(matches!(
                    render(format, &doc, &WriterOptions::default(), &mut ctx),
                    Err(Error::App(_))
                ));
            }
        }
    });
}

fn on_small_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn roff_honors_depth_limits_above_128() {
    std::thread::Builder::new()
        .stack_size(32 << 20)
        .spawn(|| {
            let mut ctx = Context::pure();
            ctx.limits.max_nesting_depth = 16_384;
            for depth in [200, 1_000] {
                let text = format!("{}text\n{}", ".RS\n".repeat(depth), ".RE\n".repeat(depth));
                let doc = read("man", &Sources::from_text(text), &mut ctx).unwrap();
                let mut blocks = doc.blocks.as_slice();
                for _ in 0..depth {
                    let [Block::BlockQuote(inner)] = blocks else {
                        panic!("Expected a block quote.");
                    };
                    blocks = inner;
                }
                assert_eq!(blocks, [Block::Para(vec![Inline::Str("text".into())])]);
                for format in ["man", "ms"] {
                    let output = render(format, &doc, &WriterOptions::default(), &mut ctx).unwrap();
                    let start = if format == "man" { ".RS" } else { ".QS" };
                    assert_eq!(output.lines().filter(|line| *line == start).count(), depth);
                }
            }
            let inline = (0..1_000).fold(Inline::Str("text".into()), |inner, _| {
                Inline::Emph(vec![inner])
            });
            let doc = Pandoc {
                blocks: vec![Block::Para(vec![inline])],
                ..Pandoc::default()
            };
            for format in ["man", "ms"] {
                assert!(
                    render(format, &doc, &WriterOptions::default(), &mut ctx)
                        .unwrap()
                        .contains("text")
                );
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn man_stub_reads_a_long_include() {
    let page = "one line of text\n".repeat(300);
    assert_eq!(page.len(), 5_100);
    let mut host = PureHost::default();
    host.files
        .insert("./big.man".into(), page.as_bytes().to_vec());
    let mut ctx = Context::new(Box::new(host));
    let stub = ".so big.man\n";
    assert_eq!(stub.len(), 12);
    ctx.limits.max_input_size = (stub.len() + page.len()) as u64;
    let doc = read("man", &Sources::from_text(stub), &mut ctx).unwrap();
    assert_eq!(doc, parse("man", &page));
    assert!(ctx.log().is_empty());
}

#[test]
fn man_limits_include_reads_per_resource_and_in_total() {
    let mut host = PureHost::default();
    host.files.insert("./page.man".into(), b"text\n".to_vec());
    let mut ctx = Context::new(Box::new(host));
    let stub = ".so page.man\n";

    ctx.limits.max_resource_size = 4;
    assert!(matches!(
        read("man", &Sources::from_text(stub), &mut ctx),
        Err(Error::Io(_))
    ));

    let repeated = stub.repeat(2);
    ctx.limits.max_resource_size = 5;
    ctx.limits.max_input_size = repeated.len() as u64 + 9;
    assert!(matches!(
        read("man", &Sources::from_text(&repeated), &mut ctx),
        Err(Error::Io(_))
    ));
    ctx.limits.max_input_size += 1;
    assert_eq!(
        read("man", &Sources::from_text(repeated), &mut ctx).unwrap(),
        parse("man", "text\ntext\n")
    );
}

#[test]
fn man_detects_include_size_errors_by_kind() {
    struct TestHost(std::io::ErrorKind, &'static str);

    impl Host for TestHost {
        fn read_file_limited(&mut self, _: &str, _: usize) -> std::io::Result<Vec<u8>> {
            Err(std::io::Error::new(self.0, self.1))
        }
    }

    let source = Sources::from_text(".so page.man\ntext\n");
    let mut ctx = Context::new(Box::new(TestHost(
        std::io::ErrorKind::FileTooLarge,
        "Resource size limit reached.",
    )));
    assert!(matches!(read("man", &source, &mut ctx), Err(Error::Io(_))));

    let mut ctx = Context::new(Box::new(TestHost(
        std::io::ErrorKind::NotFound,
        "the file is larger than the available disk space",
    )));
    assert!(read("man", &source, &mut ctx).is_ok());
    assert!(matches!(
        ctx.log(),
        [LogMessage::CouldNotLoadIncludeFile(..)]
    ));
}

#[test]
fn man_names_the_include_cycle() {
    let mut host = PureHost::default();
    host.files.insert("./a.man".into(), b".so b.man\n".to_vec());
    host.files.insert("./b.man".into(), b".so a.man\n".to_vec());
    let mut ctx = Context::new(Box::new(host));
    let error = read("man", &Sources::from_text(".so a.man\n"), &mut ctx).unwrap_err();
    assert!(
        matches!(error, Error::Parse(message) if message.contains("include cycle") && message.contains("a.man"))
    );
}

#[test]
fn man_tbl_uses_the_first_format_line() {
    let doc = parse("man", ".TH X 1\n.TS\nc c\nl l.\nA\tB\n1\t2\n.TE\n");
    let [Block::Table(table)] = &doc.blocks[..] else {
        panic!("Expected a table.");
    };
    assert_eq!(
        table
            .col_specs
            .iter()
            .map(|col| col.align)
            .collect::<Vec<_>>(),
        [Alignment::AlignCenter; 2]
    );
    assert!(table.bodies[0].body.iter().all(|row| row.cells.len() == 2));
}

#[test]
fn ms_keeps_first_paragraph_state_across_blocks() {
    let p = |text: &str| Block::Para(vec![Inline::Str(text.into())]);
    for (blocks, expected) in [
        (
            vec![p("p"), Block::HorizontalRule, p("q")],
            ".LP\np\n.HLINE\n.PP\nq",
        ),
        (
            vec![Block::OrderedList(
                randoc_ast::ListAttributes::default(),
                vec![vec![p("one"), p("continued")], vec![p("two")]],
            )],
            ".IP \" 1.\" 4\none\n.RS 4\n.LP\ncontinued\n.RE\n.IP \" 2.\" 4\ntwo",
        ),
        (
            vec![
                Block::RawBlock(randoc_ast::Format::new("ms"), ".sp".into()),
                p("q"),
            ],
            ".sp\n.LP\nq",
        ),
    ] {
        let doc = Pandoc {
            blocks,
            ..Pandoc::default()
        };
        assert_eq!(
            render("ms", &doc, &WriterOptions::default(), &mut Context::pure()).unwrap(),
            expected
        );
    }
}

#[test]
fn ms_uses_plain_code_for_an_unknown_language() {
    let doc = Pandoc {
        blocks: vec![Block::CodeBlock(
            super::escape::attr("unknown-language"),
            "print(1)".into(),
        )],
        ..Pandoc::default()
    };
    let text = render("ms", &doc, &WriterOptions::default(), &mut Context::pure()).unwrap();
    assert_eq!(text, ".IP\n.nf\n\\f[C]\nprint(1)\n\\f[]\n.fi");
}

#[test]
fn mdoc_accepts_a_final_synopsis() {
    for body in ["", ".Nm x\n", ".Nm x\n.Bl -bullet\n.It\nitem\n.El\n"] {
        let input = format!(".Sh SYNOPSIS\n{body}");
        let at_end = parse("mdoc", &input);
        let followed = parse("mdoc", &format!("{input}.Sh DESCRIPTION\n"));
        assert_eq!(at_end.blocks, followed.blocks[..followed.blocks.len() - 1]);
        if body.starts_with(".Nm") {
            assert!(matches!(at_end.blocks.get(1), Some(Block::LineBlock(_))));
        }
    }
}

#[test]
fn mdoc_rejects_a_subsection_in_synopsis() {
    for input in [
        ".Sh SYNOPSIS\n.Nm x\n.Ss Sub\ntext\n",
        ".Sh SYNOPSIS\n.Nm x\n.Bl -bullet\n.It\nitem\n.El\n.Ss Sub\ntext\n",
    ] {
        let err = read("mdoc", &Sources::from_text(input), &mut Context::pure()).unwrap_err();
        assert_eq!(err.exit_code(), 64);
        assert!(
            matches!(err, Error::Parse(message) if message.contains("unexpected Macro \"Ss\""))
        );
    }
}

#[test]
fn roff_accepts_tables_with_real_cells_above_the_padding_limit() {
    let row = format!("{}x\n", "x\t".repeat(249));
    let doc = parse("man", &format!(".TS\nl.\n{}.TE\n", row.repeat(1_000)));
    let [Block::Table(table)] = doc.blocks.as_slice() else {
        panic!("Expected a table.");
    };
    assert_eq!(table.bodies[0].body.len(), 1_000);
    assert!(
        table.bodies[0]
            .body
            .iter()
            .all(|row| row.cells.len() == 250)
    );
    for format in ["man", "ms"] {
        let out = render(
            format,
            &doc,
            &WriterOptions::default(),
            &mut Context::pure(),
        )
        .unwrap();
        assert_eq!(out.matches("T{\n").count(), 250_000);
    }

    let mut ctx = Context::pure();
    ctx.limits.max_table_cells = 1;
    for (format, input) in [
        ("man", ".TS\nl l.\na\tb\nc\n.TE\n"),
        ("mdoc", ".Bl -column\n.It a Ta b\n.It c\n.El\n"),
    ] {
        let doc = read(format, &Sources::from_text(input), &mut ctx).unwrap();
        for writer in ["man", "ms"] {
            assert!(render(writer, &doc, &WriterOptions::default(), &mut ctx).is_ok());
        }
    }
}

#[test]
fn mdoc_matches_nested_enclosures_and_preserves_punctuation() {
    let depth = 1_000;
    let text = format!(".No Oo {}value Oc tail .\n", "Dq ".repeat(depth));
    std::thread::Builder::new()
        .stack_size(32 << 20)
        .spawn(move || {
            let mut ctx = Context::pure();
            ctx.limits.max_nesting_depth = 16_384;
            let doc = read("mdoc", &Sources::from_text(text), &mut ctx).unwrap();
            let [Block::Para(content)] = doc.blocks.as_slice() else {
                panic!("Expected a paragraph.");
            };
            assert_eq!(content[0], Inline::Str("[".into()));
            assert_eq!(content[2], Inline::Str("]".into()));
            assert_eq!(content[4], Inline::Str("tail.".into()));
            let mut value = &content[1];
            for _ in 0..depth {
                let Inline::Quoted(randoc_ast::QuoteType::DoubleQuote, inner) = value else {
                    panic!("Expected a quote.");
                };
                assert_eq!(inner.len(), 1);
                value = &inner[0];
            }
            assert_eq!(value, &Inline::Str("value".into()));
        })
        .unwrap()
        .join()
        .unwrap();

    let doc = parse("mdoc", ".No Oo Op Oo Cm inner Oc Cm outer Oc tail .\n");
    let [Block::Para(content)] = doc.blocks.as_slice() else {
        panic!("Expected a paragraph.");
    };
    assert_eq!(content[0], Inline::Str("[[[".into()));
    assert!(content.contains(&Inline::Str("]]".into())));
    assert_eq!(content.last(), Some(&Inline::Str("tail.".into())));
}

#[test]
fn man_writer_links_images_only_for_known_schemes() {
    let image = |alt: &str, url: &str| {
        let alt = if alt.is_empty() {
            Vec::new()
        } else {
            vec![Inline::Str(alt.into())]
        };
        Inline::Image(Attr::default(), alt, randoc_ast::Target::new(url, ""))
    };
    let man = |inline: Inline| {
        let doc = Pandoc {
            blocks: vec![Block::Para(vec![inline])],
            ..Pandoc::default()
        };
        render("man", &doc, &WriterOptions::default(), &mut Context::pure()).unwrap()
    };
    assert_eq!(
        man(image("", "http://x-y.org/a-b.png")),
        ".PP\n[IMAGE: \\c\n.UR http://x-y.org/a-b.png\nimage\n.UE \\c\n]"
    );
    assert_eq!(
        man(image("alt", "mailto:a-b@c.org")),
        ".PP\n[IMAGE: \\c\n.MT a-b@c.org\nalt\n.ME \\c\n]"
    );
    assert_eq!(man(image("alt", "foo:b-ar.png")), ".PP\n[IMAGE: alt]");
    assert_eq!(man(image("", "a.png")), ".PP\n[IMAGE: image]");
    let link = Inline::Link(
        Attr::default(),
        vec![Inline::Str("a".into())],
        randoc_ast::Target::new("foo:bar", ""),
    );
    assert_eq!(man(link), ".PP\na");
}
