//! Regression cases checked against Pandoc 3.13.

use randoc_ast::{
    Attr, Block, Caption, Cell, Format, Inline, Pandoc, Row, Table, TableBody, Target,
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
