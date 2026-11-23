#![allow(clippy::unwrap_used)]

use randoc_ast::{Attr, Block, Format, Inline, Pandoc};
use randoc_core::{Context, Extension, ReaderOptions, Sources, WrapOption, WriterOptions};

fn read(source: &str, context: &mut Context) -> randoc_core::Result<Pandoc> {
    super::reader::read(
        &Sources::from_text(source),
        &ReaderOptions::default(),
        context,
    )
}

fn write(doc: &Pandoc, context: &mut Context) -> randoc_core::Result<String> {
    let mut options = WriterOptions::default();
    options.wrap_text = WrapOption::WrapNone;
    super::writer::write(doc, &options, context)
}

#[test]
fn section_ids_follow_postorder() {
    let doc = read("# Same\n\n## Same\n\n# Same\n", &mut Context::pure()).unwrap();
    let [Block::Div(first, children), Block::Div(last, _)] = doc.blocks.as_slice() else {
        unreachable!()
    };
    assert_eq!(first.id, "Same-1");
    assert_eq!(last.id, "Same-2");
    let [Block::Header(1, header, _), Block::Div(child, _)] = children.as_slice() else {
        unreachable!()
    };
    assert!(header.is_empty());
    assert_eq!(child.id, "Same");
}

#[test]
fn inline_attributes_and_delimiters() {
    let doc = read(
        "word{.w} [_em_]{.span} **strong** {+new+} `x`{=html}",
        &mut Context::pure(),
    )
    .unwrap();
    let [Block::Para(nodes)] = doc.blocks.as_slice() else {
        unreachable!()
    };
    assert!(
        matches!(&nodes[0], Inline::Span(attr, nodes) if attr.has_class("w") && attr.get("wrapper") == Some("1") && nodes.len() == 1)
    );
    assert!(
        matches!(&nodes[2], Inline::Span(attr, nodes) if attr.has_class("span") && matches!(&nodes[0], Inline::Emph(_)))
    );
    assert!(matches!(&nodes[4], Inline::Strong(nodes) if matches!(&nodes[0], Inline::Strong(_))));
    assert!(matches!(&nodes[6], Inline::Span(attr, _) if attr.has_class("inserted")));
    assert!(
        matches!(&nodes[8], Inline::RawInline(Format(format), payload) if format == "html" && payload == "x")
    );
}

#[test]
fn source_positions_and_heading_attributes() {
    let mut options = ReaderOptions::default();
    options.extensions.enable(Extension::Sourcepos);
    let sources = Sources::from_text("# Heading\n\nA *bold* word.\n");
    let doc = super::reader::read(&sources, &options, &mut Context::pure()).unwrap();
    let [Block::Div(attr, children)] = doc.blocks.as_slice() else {
        unreachable!()
    };
    assert_eq!(attr.get("data-pos"), Some("1:1-4:0"));
    let [Block::Header(1, attr, title), Block::Div(_, _)] = children.as_slice() else {
        unreachable!()
    };
    assert_eq!(attr.get("data-pos"), Some("1:1-2:0"));
    assert!(matches!(&title[0], Inline::Span(attr, _) if attr.get("data-pos") == Some("1:3-1:9")));
}

#[test]
fn repeated_footnotes_use_the_expansion_budget() {
    let source = format!("{}\n\n[^a]: {}\n", "[^a] ".repeat(100), "text ".repeat(100));
    let mut context = Context::pure();
    context.limits.max_input_size = source.len() as u64 + 50;
    assert!(read(&source, &mut context).is_err());
}

#[test]
fn reference_attributes_and_recursion() {
    let source = "[X][key]{.own k=own} and a[^a].\n\n{.ref k=ref}\n[key]: /url\n\n[^a]: self[^a]\n";
    let doc = read(source, &mut Context::pure()).unwrap();
    let [Block::Para(nodes)] = doc.blocks.as_slice() else {
        unreachable!()
    };
    assert!(
        matches!(&nodes[0], Inline::Link(attr, _, target) if attr.classes == ["own", "ref"] && attr.get("k") == Some("ref") && target.url == "/url")
    );
    assert!(
        matches!(&nodes[nodes.len() - 2], Inline::Note(blocks) if blocks == &[Block::Para(vec![Inline::Str("self".into())])])
    );
}

#[test]
fn table_cell_budget() {
    let mut context = Context::pure();
    context.limits.max_table_cells = 3;
    assert!(read("| a | b |\n| c | d |\n", &mut context).is_err());
}

#[test]
fn reader_on_two_mib_stack() {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            let mut context = Context::pure();
            context.limits.max_nesting_depth = 180;
            let headers = (1..=140)
                .map(|n| format!("{} Same\n\n", "#".repeat(n)))
                .collect::<String>();
            assert!(read(&headers, &mut context).is_ok());
            let quotes = format!("{}text", "> ".repeat(132));
            assert!(read(&quotes, &mut context).is_ok());
            let divs = (3..=132)
                .rev()
                .map(|n| format!("{}\n", ":".repeat(n)))
                .collect::<String>();
            assert!(read(&divs, &mut context).is_ok());
            let notes = format!(
                "Note[^0].\n\n{}[^132]: Last.\n",
                (0..132)
                    .map(|n| { format!("[^{n}]: Next[^{}].\n\n", n + 1) })
                    .collect::<String>()
            );
            assert!(read(&notes, &mut context).is_ok());
            let list = format!("{}text", "- ".repeat(132));
            assert!(read(&list, &mut context).is_ok());
            let list = (0..132)
                .map(|n| format!("{}- text\n", "  ".repeat(n)))
                .collect::<String>();
            assert!(read(&list, &mut context).is_ok());
            let brackets = "[".repeat(200);
            assert!(read(&brackets, &mut context).is_err());
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn writer_on_two_mib_stack() {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            let mut context = Context::pure();
            context.limits.max_nesting_depth = 180;
            let mut nodes = vec![Inline::Str("nested".into())];
            for _ in 0..130 {
                nodes = vec![Inline::Emph(nodes)];
            }
            let doc = Pandoc {
                blocks: vec![Block::Plain(nodes)],
                ..Pandoc::default()
            };
            assert!(write(&doc, &mut context).is_ok());
            let mut blocks = vec![Block::Plain(vec![Inline::Str("text".into())])];
            for _ in 0..130 {
                blocks = vec![Block::Div(Attr::default(), blocks)];
            }
            let doc = Pandoc {
                blocks,
                ..Pandoc::default()
            };
            assert!(write(&doc, &mut context).is_ok());
            context.limits.max_nesting_depth = 24;
            assert!(write(&doc, &mut context).is_err());
        })
        .unwrap()
        .join()
        .unwrap();
}

fn paragraph(source: &str) -> Vec<Inline> {
    let doc = read(source, &mut Context::pure()).unwrap();
    let [Block::Para(nodes)] = doc.blocks.as_slice() else {
        unreachable!()
    };
    nodes.clone()
}

fn plain(text: &str) -> Inline {
    Inline::Str(text.into())
}

#[test]
fn attributes_after_openers() {
    for (source, literal, id, classes) in [
        ("_{.a}", "_", "", vec!["a".into()]),
        ("*{#y}", "*", "y", vec![]),
    ] {
        let attr = Attr::new(id, classes, vec![("wrapper".into(), "1".into())]);
        assert_eq!(
            paragraph(source),
            [Inline::Span(attr, vec![plain(literal)])]
        );
    }
    assert_eq!(paragraph("[{.a}"), [plain("[")]);
    assert_eq!(paragraph("![{.a}"), [plain("![")]);
    assert!(matches!(
        paragraph("[{.a}some](x)").as_slice(),
        [Inline::Link(attr, nodes, target)] if attr.is_empty() && nodes == &[plain("some")] && target.url == "x"
    ));
    assert_eq!(paragraph("*{%comment%}bold*"), [plain("*bold*")]);
}

#[test]
fn doubling_footnotes_use_the_default_expansion_budget() {
    let source = format!(
        "[^0]\n\n{}[^40]: x\n",
        (0..40)
            .map(|n| format!("[^{n}]: [^{next}][^{next}]\n\n", next = n + 1))
            .collect::<String>()
    );
    assert_size_error(read(&source, &mut Context::pure()));
}

fn assert_size_error(result: randoc_core::Result<Pandoc>) {
    assert!(matches!(result, Err(error) if error.to_string() == super::size_error().to_string()));
}

#[test]
fn explicit_and_heading_references_use_the_expansion_budget() {
    for definition in ["[r]: u", "# r"] {
        let source = format!("{}\n\n{definition}\n", "[x][r] ".repeat(500));
        let mut context = Context::pure();
        context.limits.max_input_size = source.len() as u64;
        assert_size_error(read(&source, &mut context));
    }
}

#[test]
fn unclosed_verbatim_ends_with_the_paragraph() {
    for (source, expected) in [
        ("`abc", Inline::Code(Attr::default(), "abc".into())),
        ("``abc` d", Inline::Code(Attr::default(), "abc` d".into())),
        (
            "` abc\n  line \n",
            Inline::Code(Attr::default(), " abc\nline".into()),
        ),
        (
            "`abc \nline \n",
            Inline::Code(Attr::default(), "abc \nline".into()),
        ),
        (
            "$`x",
            Inline::Math(randoc_ast::MathType::InlineMath, "x".into()),
        ),
        (
            "$$`x",
            Inline::Math(randoc_ast::MathType::DisplayMath, "x".into()),
        ),
    ] {
        assert_eq!(paragraph(source), [expected], "{source:?}");
    }
}

#[test]
fn symbol_names_and_emoji_aliases() {
    let emoji = |alias: &str, glyph: &str| {
        Inline::Span(
            Attr::new(
                "",
                vec!["emoji".into()],
                vec![("data-emoji".into(), alias.into())],
            ),
            vec![plain(glyph)],
        )
    };
    let symbol = |name| {
        Inline::Span(
            Attr::new("", vec!["symbol".into()], vec![]),
            vec![plain(name)],
        )
    };
    assert_eq!(paragraph(":smile:"), [emoji("smile", "😄")]);
    assert_eq!(
        paragraph("a:b:c"),
        [plain("a"), emoji("b", "🅱️"), plain("c")]
    );
    assert_eq!(
        paragraph(":smile:{.x data-emoji=z}{.y}"),
        [Inline::Span(
            Attr::new(
                "",
                vec!["x".into(), "y".into(), "emoji".into()],
                vec![("data-emoji".into(), "smile".into())]
            ),
            vec![plain("😄")],
        )]
    );
    assert_eq!(paragraph(":+1:"), [emoji("+1", "👍")]);
    for name in ["notanemoji", "a-b", "a_b", "a+b", "A", "0"] {
        assert_eq!(paragraph(&format!(":{name}:")), [symbol(name)]);
    }
    for text in [":é:", ":a.b:", "::", ":foo/bar:", r"\:smile:"] {
        assert_eq!(paragraph(text), [plain(text.trim_start_matches('\\'))]);
    }
}

#[test]
fn writer_escapes_inline_text() {
    let code = Inline::Code(Attr::default(), "x".into());
    let cases = [
        (vec![plain(r#"a"b'c"#)], r#"a\"b\'c"#),
        (vec![plain("a$")], r"a\$"),
        (vec![plain("$"), code], "\\$`x`"),
        (vec![plain("a--b...c")], r"a\--b\...c"),
        (vec![plain("a---b....c")], r"a\-\--b\.\...c"),
        (
            vec![plain("a:b http://z :: ::: ]:")],
            r"a\:b http\://z \:: \:\:: \]\:",
        ),
        (vec![plain("a-"), plain("-b")], r"a\--b"),
        (vec![plain(":s"), plain("mile:")], r"\:smile:"),
        (vec![plain("a$"), Inline::Space, plain("b")], "a$ b"),
        (vec![plain(":smile:")], r"\:smile:"),
    ];
    for (nodes, expected) in cases {
        let doc = Pandoc {
            blocks: vec![Block::Para(nodes)],
            ..Pandoc::default()
        };
        let actual = write(&doc, &mut Context::pure()).unwrap();
        assert_eq!(actual, format!("{expected}\n"));
        let reparsed = read(&actual, &mut Context::pure()).unwrap();
        let [Block::Para(before)] = doc.blocks.as_slice() else {
            unreachable!()
        };
        let [Block::Para(after)] = reparsed.blocks.as_slice() else {
            unreachable!()
        };
        assert_eq!(
            randoc_core::shared::stringify(before),
            randoc_core::shared::stringify(after)
        );
        if before.iter().any(|node| matches!(node, Inline::Code(..))) {
            assert_eq!(before, after);
        }
    }
}

#[test]
fn writer_omits_only_unsuffixed_heading_ids() {
    for (source, expected) in [
        ("# A\n\n# A\n", "# A\n\n{#A-1}\n# A\n"),
        (
            "# Same\n\n## Same\n\n# Same\n",
            "{#Same-1}\n# Same\n\n## Same\n\n{#Same-2}\n# Same\n",
        ),
    ] {
        let mut context = Context::pure();
        let doc = read(source, &mut context).unwrap();
        assert_eq!(write(&doc, &mut context).unwrap(), expected);
    }
}

#[test]
fn link_destinations_keep_spaces() {
    for (source, url) in [
        ("[a](b c)", "b c"),
        ("[a](b\nc d)", "bc d"),
        ("[a](b \n   c d)", "b c d"),
        ("[a](  b c  )", "  b c  "),
        ("[a](b\r\nc d)", "bc d"),
        ("[a](b\\\nc d)", "bc d"),
        ("[a](b\tc)", "b   c"),
    ] {
        assert!(
            matches!(
                paragraph(source).as_slice(),
                [Inline::Link(_, _, target)] if target.url == url
            ),
            "{source:?}"
        );
    }
}

#[test]
fn long_dash_runs_follow_djot_rules() {
    for (count, text) in [
        (6, "——"),
        (8, "––––"),
        (11, "———–"),
        (13, "———––"),
        (17, "—————–"),
    ] {
        assert_eq!(
            paragraph(&format!("a{}b", "-".repeat(count))),
            [plain(&format!("a{text}b"))]
        );
    }
}

#[test]
fn unmatched_single_quotes_use_a_right_apostrophe() {
    assert_eq!(paragraph("'tis"), [plain("’tis")]);
    assert_eq!(paragraph("{'tis"), [plain("’tis")]);
    assert_eq!(
        paragraph("'yes'"),
        [Inline::Quoted(
            randoc_ast::QuoteType::SingleQuote,
            vec![plain("yes")]
        )]
    );
    assert_eq!(paragraph("\"yes"), [plain("“yes")]);
}

#[test]
fn attributes_and_comments_keep_one_space() {
    for source in ["a {.c} b", "a {% c %} b", "a {% c %} {.c} b", "a {.c}b"] {
        assert_eq!(paragraph(source), [plain("a"), Inline::Space, plain("b")]);
    }
    assert_eq!(
        paragraph("a {% c %}\nb"),
        [plain("a"), Inline::SoftBreak, plain("b")]
    );
}

#[test]
fn east_asian_breaks_only_disappear_between_east_asian_text() {
    let mut options = ReaderOptions::default();
    options.extensions.enable(Extension::EastAsianLineBreaks);
    let count = 4096;
    let sources = Sources::from_text(format!("{}日\nword\n本", "日\n".repeat(count)));
    let doc = super::reader::read(&sources, &options, &mut Context::pure()).unwrap();
    let [Block::Para(nodes)] = doc.blocks.as_slice() else {
        unreachable!()
    };
    assert_eq!(nodes.len(), count + 5);
    assert!(nodes[..count].iter().all(|node| node == &plain("日")));
    assert_eq!(
        &nodes[count..],
        [
            plain("日"),
            Inline::SoftBreak,
            plain("word"),
            Inline::SoftBreak,
            plain("本")
        ]
    );
}

#[test]
fn source_positions_track_unicode_columns_and_lines() {
    let mut options = ReaderOptions::default();
    options.extensions.enable(Extension::Sourcepos);
    let prefix = "é".repeat(400);
    let sources = Sources::from_text(format!("> {prefix} *é*\n> 日\n"));
    let doc = super::reader::read(&sources, &options, &mut Context::pure()).unwrap();
    let mut positions = Vec::new();
    randoc_ast::walk::query_inlines(&doc, |node| {
        if let Inline::Span(attr, _) = node
            && let Some(position) = attr.get("data-pos")
        {
            positions.push(position.to_string());
        }
    });
    assert_eq!(
        positions,
        [
            "1:3-1:403",
            "1:3-1:403",
            "1:404-1:406",
            "1:405-1:405",
            "1:407-2:0",
            "2:3-2:3"
        ]
    );
}

#[test]
fn footnote_expansion_includes_reference_depth() {
    for reference in [
        format!("{}[^a]", "> ".repeat(8)),
        format!("{}[^a]{}", "*".repeat(8), "*".repeat(8)),
    ] {
        let source = format!("{reference}\n\n[^a]: {}text\n", "> ".repeat(8));
        let mut context = Context::pure();
        context.limits.max_nesting_depth = 20;
        assert!(read(&source, &mut context).is_ok());
        context.limits.max_nesting_depth = 12;
        let error = read(&source, &mut context).unwrap_err();
        assert_eq!(error.to_string(), super::depth_error().to_string());
    }
}
