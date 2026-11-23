//! Regression cases checked against Pandoc 3.13.

use randoc_ast::{
    Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, ColWidth, Format, Inline,
    MetaValue, Pandoc, Row, Table, TableBody, TableHead, Target,
};
use randoc_core::{Context, FormatSpec, Output, WrapOption, WriterOptions};

fn render(format: &str, doc: Pandoc, mut options: WriterOptions) -> String {
    let spec = FormatSpec::parse(format);
    options.extensions = spec.extensions().unwrap();
    let writer = crate::writer(&spec.name).unwrap();
    let Output::Text(text) = writer.write(&doc, &options, &mut Context::pure()).unwrap() else {
        unreachable!("the Markdown writer gives text");
    };
    text
}

fn document(blocks: Vec<Block>) -> Pandoc {
    Pandoc {
        blocks,
        ..Pandoc::default()
    }
}

fn write(format: &str, blocks: Vec<Block>) -> String {
    render(format, document(blocks), WriterOptions::default())
}

fn text(s: &str) -> Inline {
    Inline::Str(s.into())
}

fn para(s: &str) -> Block {
    Block::Para(vec![text(s)])
}

fn markdown_template() -> randoc_templates::Template {
    let source = randoc_data::default_template("markdown").unwrap();
    randoc_templates::Template::compile("markdown", source, 64, &mut |_| Err("no partials"))
        .unwrap()
}

fn citation(id: &str) -> Inline {
    Inline::Cite(
        vec![Citation {
            id: id.into(),
            prefix: vec![],
            suffix: vec![],
            mode: CitationMode::NormalCitation,
            note_num: 0,
            hash: 0,
        }],
        vec![text("fallback")],
    )
}

#[test]
fn wildcard_citations_in_metadata_and_body_use_a_bare_key() {
    let mut doc = document(vec![Block::Para(vec![citation("*")])]);
    doc.meta
        .0
        .insert("nocite".into(), MetaValue::MetaInlines(vec![citation("*")]));
    let mut options = WriterOptions::default();
    options.template = Some(markdown_template());
    assert_eq!(
        render("markdown", doc, options),
        "---\nnocite: \"[@*]\"\n---\n\n[@*]\n"
    );
}

#[test]
fn citation_keys_use_pandoc_brace_rules() {
    let cases = [
        ("", "{}"),
        ("*foo", "*foo"),
        ("**", "{**}"),
        ("a*b", "{a*b}"),
        ("foo:bar", "foo:bar"),
        ("foo:", "{foo:}"),
        (":foo", "{:foo}"),
        ("123", "123"),
        ("_", "_"),
        ("foo://bar/_baz", "foo://bar/_baz"),
        ("a//b", "a//b"),
        ("a--b", "{a--b}"),
        ("a!b", "{a!b}"),
        ("a b", "{a b}"),
        ("a ", "a "),
        ("a{b}", "{a{b}}"),
        ("éⅣ", "éⅣ"),
        ("a\u{0345}b", "{a\u{0345}b}"),
    ];
    for (id, key) in cases {
        assert_eq!(
            write("markdown", vec![Block::Para(vec![citation(id)])]),
            format!("[@{key}]"),
            "{id:?}"
        );
    }
}

#[test]
fn figure_and_caption_use_the_div_syntax() {
    let figure = Block::Figure(
        Attr::new("f", vec!["k".into()], vec![("a".into(), "b".into())]),
        Caption {
            short: None,
            long: vec![Block::Plain(vec![text("cap")])],
        },
        vec![Block::Plain(vec![Inline::Image(
            Attr::new("im", Vec::new(), Vec::new()),
            vec![text("cap")],
            Target::new("i.png", ""),
        )])],
    );
    let cases = [
        (
            "markdown-implicit_figures-raw_html",
            ":::: {#f .figure .k a=\"b\"}\n![cap](i.png){#im}\n\n::: caption\ncap\n:::\n::::",
        ),
        (
            "commonmark_x-implicit_figures-raw_html",
            ":::: {#f .figure .k a=\"b\"}\n![cap](i.png){#im}\n\n::: {.caption}\ncap\n:::\n::::",
        ),
        (
            "markdown-raw_html-fenced_divs",
            "<div id=\"f\" class=\"figure k\" data-a=\"b\">\n\n![cap](i.png){#im}\n\n<div class=\"caption\">\n\ncap\n\n</div>\n\n</div>",
        ),
        ("markdown_mmd-raw_html", "![cap](i.png)"),
        ("markdown_phpextra-raw_html", "![cap](i.png){#im}\n\ncap"),
    ];
    for (format, expected) in cases {
        assert_eq!(
            write(format, vec![figure.clone(), para("after")]),
            format!("{expected}\n\nafter"),
            "{format}"
        );
    }
    let bare = Block::Figure(Attr::default(), Caption::default(), vec![para("body")]);
    assert_eq!(
        write("markdown-implicit_figures-raw_html", vec![bare]),
        "::: figure\nbody\n:::"
    );
}

#[test]
fn hard_line_breaks_collapse_and_need_no_marker() {
    let paragraph = Block::Para(vec![
        Inline::LineBreak,
        text("a"),
        Inline::LineBreak,
        Inline::LineBreak,
        text("b"),
        Inline::LineBreak,
    ]);
    for format in ["markdown+hard_line_breaks", "gfm+hard_line_breaks"] {
        assert_eq!(write(format, vec![paragraph.clone()]), "a\nb", "{format}");
    }
}

#[test]
fn simple_table_width_includes_final_spaces() {
    for (tail, dashes) in [
        (vec![], "-----"),
        (vec![Inline::Space], "------"),
        (vec![Inline::Space, Inline::Space], "-------"),
        (vec![Inline::SoftBreak], "------"),
    ] {
        let mut inlines = vec![text("abc")];
        inlines.extend(tail);
        let cell = Cell {
            content: vec![Block::Plain(inlines)],
            ..Cell::default()
        };
        let table = Table {
            col_specs: vec![Default::default()],
            bodies: vec![TableBody {
                body: vec![Row {
                    cells: vec![cell],
                    ..Row::default()
                }],
                ..TableBody::default()
            }],
            ..Table::default()
        };
        for format in ["markdown", "plain"] {
            assert_eq!(
                write(format, vec![Block::Table(Box::new(table.clone()))]),
                format!("  {dashes}\n  abc\n  {dashes}"),
                "{format}"
            );
        }
    }
}

#[test]
fn ascii_entities_apply_to_text() {
    let paragraph = Block::Para(vec![
        Inline::Code(Attr::default(), "é".into()),
        Inline::Space,
        text("©™®∀…εΩℝ→· æé—“😀中｡⃗¨Åˇ≪≫"),
        Inline::Space,
        Inline::Link(
            Attr::default(),
            vec![text("é")],
            Target::new("http://x/é", ""),
        ),
        Inline::Space,
        Inline::Span(
            Attr::new("", Vec::new(), vec![("title".into(), "é".into())]),
            vec![text("x")],
        ),
    ]);
    let mut opts = WriterOptions::default();
    opts.prefer_ascii = true;
    opts.wrap_text = WrapOption::WrapNone;
    assert_eq!(
        render("markdown-smart", document(vec![paragraph]), opts),
        concat!(
            "`é` &COPY;&TRADE;&REG;&ForAll;&mldr;&epsi;&ohm;&Ropf;&rarr;",
            "&middot;&nbsp;&aelig;&eacute;&mdash;&ldquo;&#128512;&#20013;&#65377;&#8407;",
            "&die;&angst;&caron;&ll;&gg; [&eacute;](http://x/é) [x]{title=\"é\"}"
        )
    );
}

#[test]
fn underline_and_small_caps_follow_the_span_extensions() {
    let paragraph = Block::Para(vec![
        Inline::Underline(vec![text("u")]),
        Inline::Space,
        Inline::SmallCaps(vec![text("Sc")]),
    ]);
    for (format, expected) in [
        ("markdown", "[u]{.underline} [Sc]{.smallcaps}"),
        (
            "markdown-bracketed_spans-raw_html",
            "<span class=\"underline\">u</span> <span class=\"smallcaps\">Sc</span>",
        ),
        (
            "markdown-bracketed_spans-native_spans",
            "<u>u</u> <span class=\"smallcaps\">Sc</span>",
        ),
        ("markdown-bracketed_spans-native_spans-raw_html", "*u* SC"),
        ("markdown-native_spans-raw_html", "[u]{.underline} SC"),
        ("plain", "u SC"),
    ] {
        assert_eq!(write(format, vec![paragraph.clone()]), expected, "{format}");
    }
    let paragraph = vec![Block::Para(vec![
        text("a"),
        Inline::Underline(vec![Inline::Space, text("u"), Inline::Space]),
        text("b"),
    ])];
    assert_eq!(write("markdown", paragraph.clone()), "a [u]{.underline} b");
    assert_eq!(
        write("markdown-bracketed_spans", paragraph),
        "a<span class=\"underline\"> u </span>b"
    );
}

#[test]
fn mark_spans_use_delimiters_and_text_escapes_them() {
    let span = |id| {
        Inline::Span(
            Attr::new(id, vec!["mark".into()], Vec::new()),
            vec![text("m")],
        )
    };
    assert_eq!(
        write(
            "markdown+mark",
            vec![
                Block::Para(vec![span(""), Inline::Space, text("==marked==")]),
                Block::Para(vec![span("i")]),
                Block::Para(vec![
                    text("a"),
                    Inline::Span(
                        Attr::new("", vec!["mark".into()], Vec::new()),
                        vec![Inline::Space, text("m"), Inline::Space],
                    ),
                    text("b"),
                ]),
            ],
        ),
        "==m== \\==marked\\==\n\n[m]{#i .mark}\n\na== m ==b"
    );
}

#[test]
fn a_note_label_precedes_a_list_on_its_own_line() {
    let paragraphs = vec![
        Block::Para(vec![
            text("x"),
            Inline::Note(vec![Block::BulletList(vec![
                vec![Block::Plain(vec![text("i")])],
                vec![Block::Plain(vec![text("j")])],
            ])]),
        ]),
        Block::Para(vec![text("y"), Inline::Note(vec![para("text")])]),
    ];
    for format in [
        "plain",
        "commonmark",
        "markdown-footnotes",
        "markdown_strict",
    ] {
        assert_eq!(
            write(format, paragraphs.clone()),
            "x[1]\n\ny[2]\n\n[1]\n- i\n- j\n\n[2] text",
            "{format}"
        );
    }
}

#[test]
fn raw_blocks_keep_final_blank_lines() {
    for format in ["markdown", "markdown_strict"] {
        let blocks = vec![
            para("z"),
            Block::RawBlock(Format::new("html"), "<i>\n\n".into()),
        ];
        assert_eq!(write(format, blocks.clone()), "z\n\n<i>\n\n", "{format}");
        let mut blocks = blocks;
        blocks.push(Block::RawBlock(Format::new("tex"), "<drop>".into()));
        assert_eq!(
            write(format, blocks),
            if format == "markdown" {
                "z\n\n<i>\n\n\n<drop>"
            } else {
                "z\n\n<i>\n\n"
            }
        );
    }
}

#[test]
fn commonmark_passes_through_only_its_raw_markdown_formats() {
    let paragraph = Block::Para(vec![
        text("x"),
        Inline::RawInline(Format::new("markdown_strict"), "_s_".into()),
        Inline::RawInline(Format::new("markdown"), "_m_".into()),
        Inline::RawInline(Format::new("gfm"), "_g_".into()),
        text("x"),
    ]);
    for (format, expected) in [
        ("markdown", "x_s__m_`_g_`{=gfm}x"),
        ("commonmark", "x_m__g_x"),
        ("gfm", "x_m__g_x"),
        ("commonmark_x", "x`_s_`{=markdown_strict}_m__g_x"),
    ] {
        assert_eq!(write(format, vec![paragraph.clone()]), expected, "{format}");
    }
}

#[test]
fn plain_smart_reverses_typographic_punctuation() {
    assert_eq!(
        write("plain+smart", vec![para("“q” ‘s’ a–b — c…")]),
        "\"q\" 's' a--b --- c..."
    );
}

#[test]
fn only_tex_math_dollars_escapes_text_dollars() {
    for (format, expected) in [
        ("gfm-tex_math_dollars", "$x$ a $5"),
        ("commonmark+tex_math_gfm", "$x$ a $5"),
        ("gfm-tex_math_gfm", "\\$x\\$ a \\$5"),
    ] {
        assert_eq!(write(format, vec![para("$x$ a $5")]), expected, "{format}");
    }
}

#[test]
fn four_space_rule_pads_the_bullet_marker() {
    let list = Block::BulletList(vec![vec![para("x"), para("y")]]);
    assert_eq!(
        write("markdown+four_space_rule", vec![list]),
        "-   x\n\n    y"
    );
}

#[test]
fn only_full_uris_with_known_schemes_become_autolinks() {
    for (url, label, expected) in [
        ("ftp://foo", "ftp://foo", "<ftp://foo>"),
        ("irc://host", "irc://host", "<irc://host>"),
        ("urn:isbn:1", "urn:isbn:1", "<urn:isbn:1>"),
        (
            "HTTPS://example.org",
            "HTTPS://example.org",
            "<HTTPS://example.org>",
        ),
        (
            "mailto:reader@example.org",
            "reader@example.org",
            "<reader@example.org>",
        ),
        (
            "M-x:doom/help",
            "M-x:doom/help",
            "[M-x:doom/help](M-x:doom/help)",
        ),
        (r"Example:\", r"Example:\", r"[Example:\\](Example:\)"),
        ("http:A B", "http:A B", "[http:A B](<http:A B>)"),
        (
            "https://a/%XX",
            "https://a/%XX",
            "[https://a/%XX](https://a/%XX)",
        ),
        (
            "https://a:port",
            "https://a:port",
            "[https://a:port](https://a:port)",
        ),
    ] {
        let link = Inline::Link(Attr::default(), vec![text(label)], Target::new(url, ""));
        assert_eq!(
            write("markdown", vec![Block::Para(vec![link])]),
            expected,
            "{url}"
        );
    }
}

#[test]
fn citations_remove_only_the_final_top_level_refs_div() {
    let refs = Block::Div(Attr::new("refs", vec![], vec![]), vec![para("refs")]);
    let inner = Block::Div(Attr::new("parent", vec![], vec![]), vec![refs.clone()]);
    assert_eq!(write("markdown", vec![para("body"), refs.clone()]), "body");
    assert!(write("markdown", vec![refs.clone(), para("body")]).contains("#refs"));
    assert!(write("markdown", vec![inner]).contains("#refs"));
    assert!(write("markdown-citations", vec![refs]).contains("#refs"));
}

#[test]
fn images_omit_a_plain_label_equal_to_the_full_url() {
    let image = |url, label| {
        Block::Para(vec![Inline::Image(
            Attr::new("", vec![], vec![("width".into(), "30".into())]),
            vec![text(label)],
            Target::new(url, "tip"),
        )])
    };
    assert_eq!(
        write(
            "markdown",
            vec![
                image("folder/a.jpg", "folder/a.jpg"),
                image("folder/a.jpg", "a.jpg")
            ]
        ),
        "![](folder/a.jpg \"tip\"){width=\"30\"}\n\n![a.jpg](folder/a.jpg \"tip\"){width=\"30\"}"
    );
}

#[test]
fn a_period_after_nine_digits_is_escaped_at_the_start_of_a_line() {
    assert_eq!(
        write(
            "markdown",
            vec![
                para("12345678.foo"),
                para("123456789.foo"),
                para("1234567890.foo"),
                Block::Para(vec![text("number"), Inline::Space, text("123456789.foo")])
            ]
        ),
        "12345678.foo\n\n123456789\\.foo\n\n1234567890.foo\n\nnumber 123456789.foo"
    );
}

fn table_cell(content: Vec<Block>) -> Cell {
    Cell {
        content,
        ..Cell::default()
    }
}

fn table_row(cells: Vec<Cell>) -> Row {
    Row {
        cells,
        ..Row::default()
    }
}

#[test]
fn table_forms_keep_headers_with_empty_text() {
    let empty_cell = table_cell(vec![Block::Plain(vec![text("")])]);
    let table = Table {
        col_specs: vec![ColSpec::default(); 2],
        head: TableHead {
            rows: vec![table_row(vec![empty_cell.clone(); 2])],
            ..TableHead::default()
        },
        bodies: vec![TableBody {
            body: vec![table_row(vec![empty_cell; 2]); 2],
            ..TableBody::default()
        }],
        ..Table::default()
    };
    let actual = write("markdown", vec![Block::Table(Box::new(table))]);
    assert_eq!(actual.lines().filter(|line| line.contains("--")).count(), 1);

    let table = Table {
        col_specs: vec![ColSpec {
            width: ColWidth::ColWidth(0.999),
            ..ColSpec::default()
        }],
        head: TableHead {
            rows: vec![table_row(vec![table_cell(vec![Block::Plain(vec![text(
                "header",
            )])])])],
            ..TableHead::default()
        },
        ..Table::default()
    };
    let actual = write("markdown", vec![Block::Table(Box::new(table.clone()))]);
    assert!(actual.starts_with('+'));
    assert!(actual.lines().last().unwrap().contains('='));

    let actual = write(
        "markdown-grid_tables",
        vec![Block::Table(Box::new(table.clone()))],
    );
    assert!(actual.trim_start().starts_with('|'));

    let mut table = table;
    table.bodies.push(TableBody {
        body: vec![table_row(vec![table_cell(vec![Block::Plain(vec![text(
            "body",
        )])])])],
        ..TableBody::default()
    });
    let actual = write("markdown", vec![Block::Table(Box::new(table))]);
    assert!(actual.trim_start().starts_with("---"), "{actual:?}");
    assert!(actual.lines().any(|line| line.trim() == "body"));
}

#[test]
fn grid_widths_share_unused_space_and_keep_indivisible_text() {
    let words = |word: &str, n| {
        let mut inlines = vec![text(word)];
        for _ in 1..n {
            inlines.extend([Inline::Space, text(word)]);
        }
        table_cell(vec![Block::Para(inlines)])
    };
    let format = "markdown-simple_tables-multiline_tables-pipe_tables";
    for (left, right, widths) in [
        (words("word", 18), words("x", 22), (34, 34)),
        (words("word", 6), words("x", 30), (31, 38)),
        (words(&"x".repeat(41), 1), words("word", 22), (43, 34)),
    ] {
        for spans in [false, true] {
            let mut left = left.clone();
            left.row_span = if spans { 2 } else { 1 };
            let mut rows = vec![table_row(vec![left, right.clone()])];
            if spans {
                rows.push(table_row(vec![table_cell(vec![para("second")])]));
            }
            let table = Table {
                col_specs: vec![ColSpec::default(); 2],
                bodies: vec![TableBody {
                    body: rows,
                    ..TableBody::default()
                }],
                ..Table::default()
            };
            let actual = write(format, vec![Block::Table(Box::new(table))]);
            let expected = format!("+{}+{}+", "-".repeat(widths.0), "-".repeat(widths.1));
            assert_eq!(actual.lines().next(), Some(expected.as_str()));
            if spans {
                assert!(actual.contains(&format!(
                    "|{}+{}+",
                    " ".repeat(widths.0),
                    "-".repeat(widths.1)
                )));
            }
        }
    }
}

#[test]
fn definition_terms_keep_trailing_space_and_empty_definitions_are_loose() {
    let code = |classes, value: &str| Inline::Code(Attr::new("", classes, vec![]), value.into());
    let block = Block::DefinitionList(vec![
        (
            vec![
                Inline::Span(Attr::default(), vec![code(vec![], "m")]),
                Inline::Space,
            ],
            vec![vec![]],
        ),
        (
            vec![text("+"), code(vec!["variable".into()], "number")],
            vec![vec![]],
        ),
    ]);
    assert_eq!(
        write("markdown", vec![block]),
        "`m` \n\n: \n\n\\+`number`{.variable}\n\n: "
    );
}

#[test]
fn escaped_markers_allow_wrapping() {
    assert_eq!(
        write(
            "markdown",
            vec![Block::Para(vec![
                Inline::Space,
                text("*"),
                Inline::Space,
                text("x")
            ])],
        ),
        r"\* x"
    );
    for (marker, escaped) in [("*", r"\*"), (">", r"\>")] {
        let mut options = WriterOptions::default();
        options.columns = 14;
        let blocks = vec![Block::Para(vec![
            text("abcdefghijkl"),
            Inline::Space,
            text(marker),
            Inline::Space,
            text("tail"),
            Inline::Space,
            text("words"),
        ])];
        assert_eq!(
            render("markdown", document(blocks), options),
            format!("abcdefghijkl\n{escaped} tail words")
        );
    }
}

#[test]
fn list_wrapping_keeps_an_ordered_marker_with_the_previous_word() {
    let blocks = vec![
        Block::BulletList(vec![vec![Block::Plain(vec![
            text("xxxxxxxxxxxxxxxxxxx"),
            Inline::Space,
            text("(28)"),
            Inline::Space,
            text("end"),
        ])]]),
        Block::Para(vec![text("C."), Inline::Space, text("Author")]),
    ];
    let mut options = WriterOptions::default();
    options.columns = 20;
    assert_eq!(
        render("markdown", document(blocks), options),
        "- xxxxxxxxxxxxxxxxxxx (28)\n  end\n\nC. Author"
    );
}

#[test]
fn image_and_figure_labels_keep_the_first_marker_and_repeat_the_note() {
    let note = |body| Inline::Note(vec![para(body)]);
    let image = |alt| Inline::Image(Attr::default(), alt, Target::new("a.jpg", ""));
    let blocks = vec![
        Block::Para(vec![
            note("before"),
            Inline::Space,
            image(vec![text("label"), note("image")]),
        ]),
        Block::Figure(
            Attr::default(),
            Caption {
                long: vec![Block::Para(vec![text("caption"), note("figure")])],
                ..Caption::default()
            },
            vec![Block::Plain(vec![image(vec![])])],
        ),
        Block::Para(vec![note("after")]),
    ];
    assert_eq!(
        write("markdown", blocks),
        concat!(
            "[^1] ![label[^2]](a.jpg)\n\n![caption[^4]](a.jpg)\n\n[^6]\n\n",
            "[^1]: before\n\n[^2]: image\n\n[^3]: image\n\n[^4]: figure\n\n[^5]: figure\n\n[^6]: after"
        )
    );
}

#[test]
fn image_label_note_repeats_have_depth_and_document_limits() {
    let image_note = |body| {
        Inline::Image(
            Attr::default(),
            vec![Inline::Note(vec![body])],
            Target::new("a.jpg", ""),
        )
    };
    let nested = (0..5).fold(para("nested note"), |body, _| {
        Block::Para(vec![image_note(body)])
    });
    let actual = write("markdown", vec![nested]);
    assert_eq!(actual.matches(": nested note").count(), 8);

    let labels = vec![Block::Para(vec![image_note(para("separate note"))]); 70];
    let actual = write("markdown", labels);
    assert_eq!(actual.matches(": separate note").count(), 70 + 64);
}

#[test]
fn quoted_yaml_wraps_at_the_requested_width() {
    let mut doc = document(vec![]);
    let value = MetaValue::MetaInlines(
        vec![
            text("Survey:"),
            Inline::Space,
            Inline::Quoted(randoc_ast::QuoteType::DoubleQuote, vec![text("quoted")]),
            Inline::Space,
            text(r"C:\Tools"),
            Inline::Space,
            Inline::Span(
                Attr::new("", vec!["nocase".into()], vec![]),
                vec![text("CamelCase")],
            ),
            Inline::Space,
            text("a\u{a0}b"),
        ]
        .into_iter()
        .chain(
            "and several more words that wrap in the reference"
                .split(' ')
                .flat_map(|word| [Inline::Space, text(word)]),
        )
        .collect(),
    );
    doc.meta.0.insert(
        "references".into(),
        MetaValue::MetaList(vec![MetaValue::MetaMap(
            [
                ("id".into(), MetaValue::MetaString("x".into())),
                ("title".into(), value),
            ]
            .into_iter()
            .collect(),
        )]),
    );
    doc.meta.0.insert(
        "multiline".into(),
        MetaValue::MetaInlines(vec![text("one"), Inline::LineBreak, text("two")]),
    );
    let prefix = "---\nmultiline: |\n  one\\\n  two\nreferences:\n- id: x\n";
    let title = r#"  title: "Survey: \"quoted\" C:\\\\Tools"#;
    for (columns, wrap, tail) in [
        (
            72,
            WrapOption::WrapAuto,
            " [CamelCase]{.nocase} a\u{a0}b and\n    several more words that wrap in the reference",
        ),
        (
            45,
            WrapOption::WrapAuto,
            "\n    [CamelCase]{.nocase} a\u{a0}b and several more\n    words that wrap in the reference",
        ),
        (
            45,
            WrapOption::WrapNone,
            " [CamelCase]{.nocase} a\u{a0}b and several more words that wrap in the reference",
        ),
    ] {
        let mut options = WriterOptions::default();
        options.columns = columns;
        options.wrap_text = wrap;
        options.template = Some(markdown_template());
        assert_eq!(
            render("markdown", doc.clone(), options),
            format!("{prefix}{title}{tail}\"\n---\n\n"),
            "{columns}, {wrap:?}"
        );
    }
}

#[test]
#[ignore = "requires local Pandoc reference fixtures"]
fn wave8_fixture_replay() {
    let root = std::path::PathBuf::from(std::env::var_os("RANDOC_W8_REPLAY").unwrap());
    let writer = crate::writer("markdown").unwrap();
    let mut failures = Vec::new();
    for (list, total, standalone) in [("corpus-ids", 35, false), ("bib-ids", 32, true)] {
        let ids = std::fs::read_to_string(root.join(list)).unwrap();
        assert_eq!(ids.lines().count(), total);
        let mut passed = 0;
        for id in ids.lines() {
            let doc = randoc_ast::json::from_json(
                &std::fs::read_to_string(root.join(format!("{id}.json"))).unwrap(),
            )
            .unwrap();
            let mut options = WriterOptions::default();
            options.extensions = FormatSpec::parse("markdown").extensions().unwrap();
            if standalone {
                options.template = Some(markdown_template());
            }
            let mut context = Context::pure();
            let Output::Text(mut actual) = writer.write(&doc, &options, &mut context).unwrap()
            else {
                unreachable!("the Markdown writer gives text")
            };
            if !standalone && !actual.ends_with('\n') {
                actual.push('\n');
            }
            let expected = std::fs::read_to_string(root.join(format!("{id}.md"))).unwrap();
            if expected == actual && context.log().is_empty() {
                passed += 1;
            } else {
                failures.push(id.to_owned());
            }
        }
        println!("{list}: {passed}/{total}");
    }
    assert!(failures.is_empty(), "{failures:?}");
}
