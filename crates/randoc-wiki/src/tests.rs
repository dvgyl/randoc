#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use randoc_ast::{Attr, Block, Format, Inline, Pandoc, Target};
use randoc_core::{Context, Reader, ReaderOptions, Sources, WriterOptions, default_extensions};
use randoc_templates::{Template, Val};

fn read(format: &str, text: &str, context: &mut Context) -> randoc_core::Result<Pandoc> {
    let Some(Reader::Text(reader)) = super::reader(format) else {
        panic!("missing wiki reader");
    };
    let mut options = ReaderOptions::default();
    options.extensions = default_extensions(format);
    reader.read(&Sources::from_text(text), &options, context)
}

#[test]
fn mediawiki_keeps_unique_heading_ids_before_replacing_hyphens() {
    let doc = read(
        "mediawiki",
        "== A B ==\n== A-B ==\n== A_B ==\n== A B ==\n",
        &mut Context::pure(),
    )
    .unwrap();
    let ids: Vec<_> = doc
        .blocks
        .iter()
        .filter_map(|block| {
            if let Block::Header(_, attr, _) = block {
                Some(attr.id.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(ids, ["a_b", "a_b_1", "a_b", "a_b_2"]);
}

#[test]
fn named_mediawiki_references_report_an_unexpected_closer() {
    for (input, line, column) in [
        ("<ref name=\"part/chapter\">note</ref>", 1, 36),
        ("* first<ref>note\n</ref> rest", 2, 7),
    ] {
        let error = read("mediawiki", input, &mut Context::pure()).unwrap_err();
        assert_eq!(error.exit_code(), 64);
        assert!(
            error
                .to_string()
                .contains(&format!("line {line}, column {column}")),
            "{error}"
        );
        assert!(
            error
                .to_string()
                .contains("unexpected (TagClose \"ref\",\"</ref>\")")
        );
    }
}

#[test]
fn mediawiki_note_definition_passes_over_a_closing_tag_on_its_line() {
    let input = "A<ref>{{topic}}: one</ref> ignored\n\nTwo<ref>nested</ref> three</ref> End";
    let doc = read("mediawiki", input, &mut Context::pure()).unwrap();
    let [Block::Para(items)] = doc.blocks.as_slice() else {
        panic!("expected one paragraph");
    };
    let [
        Inline::Str(start),
        Inline::Note(note),
        Inline::Space,
        Inline::Str(end),
    ] = items.as_slice()
    else {
        panic!("expected a note between two words");
    };
    assert_eq!((start.as_str(), end.as_str()), ("A", "End"));
    assert!(matches!(
        note.as_slice(),
        [Block::RawBlock(..), Block::DefinitionList(_), Block::Para(items)]
            if items == &[
                Inline::Str("Two".into()),
                Inline::Note(vec![Block::Plain(vec![Inline::Str("nested".into())])]),
                Inline::Space,
                Inline::Str("three".into())
            ]
    ));
    let input = "A<ref>{{topic}}: one</ref> ignored";
    assert!(read("mediawiki", input, &mut Context::pure()).is_err());
}

#[test]
fn mediawiki_cell_markers_split_code_and_comments() {
    let input = concat!(
        "{|\n|<syntaxhighlight lang=\"txt\">code\n",
        "!text <abbr>T</abbr>\n</syntaxhighlight>\n",
        "<!--\n|-\n|<pre>visible</pre>\n -->\n|}"
    );
    let doc = read("mediawiki", input, &mut Context::pure()).unwrap();
    let [Block::Table(table)] = doc.blocks.as_slice() else {
        panic!("expected a table");
    };
    let rows = &table.bodies[0].body;
    assert_eq!(rows.len(), 2);
    assert!(rows[0].cells[0].content.is_empty());
    assert!(matches!(
        rows[0].cells[1].content.as_slice(),
        [Block::Para(items), Block::RawBlock(_, close), Block::Para(_)]
            if close == "</syntaxhighlight>"
                && matches!(&items[2], Inline::RawInline(_, tag) if tag == "<abbr>")
    ));
    assert!(matches!(
        rows[1].cells[0].content.first(),
        Some(Block::CodeBlock(_, code)) if code == "visible"
    ));
}

#[test]
fn mediawiki_bare_links_encode_internal_pipes() {
    let doc = read(
        "mediawiki",
        "File:image.png|caption http://example.org|[[Next]]",
        &mut Context::pure(),
    )
    .unwrap();
    assert!(matches!(
        doc.blocks.as_slice(),
        [Block::Para(items)] if matches!(items.as_slice(),
            [Inline::Link(_, _, first), Inline::Space, Inline::Link(_, _, second), Inline::Str(pipe), Inline::Link(..)]
                if first.url == "File:image.png%7Ccaption" && second.url == "http://example.org" && pipe == "|")
    ));
}

#[test]
fn mediawiki_nested_table_and_span_limits_share_one_budget() {
    let input = "{|\n! Head\n|-\n|\n{|\n! Nested\n|-\n| Value\n|}\n|}\n";
    let doc = read("mediawiki", input, &mut Context::pure()).unwrap();
    let [Block::Table(outer)] = doc.blocks.as_slice() else {
        panic!("expected a table")
    };
    assert!(matches!(
        outer.head.rows[0].cells[0].content.as_slice(),
        [Block::Para(_)]
    ));
    assert!(matches!(
        outer.bodies[0].body[0].cells[0].content.as_slice(),
        [Block::Table(_)]
    ));
    let mut context = Context::pure();
    context.limits.max_table_cells = 0;
    assert!(read("mediawiki", input, &mut context).is_ok());
    let input = "{|\n| rowspan=999999999999999 colspan=99999999999999 | x\n|}\n";
    assert!(read("mediawiki", input, &mut context).is_err());
}

#[test]
fn table_cell_limits_charge_generated_cells() {
    for (format, head, body) in [
        ("mediawiki", "{|\n! H !! J\n|-\n", "| a || b\n|}\n"),
        ("dokuwiki", "^ H ^ J ^\n", "| a | b |\n"),
        ("jira", "||H||J||\n", "|a|b|\n"),
        ("creole", "|=H|=J|\n", "|a|b|\n"),
    ] {
        let mut context = Context::pure();
        context.limits.max_table_cells = 0;
        let mut doc = read(format, &format!("{head}{body}"), &mut context).unwrap();
        if let Some(writer) = super::writer(format) {
            let options = WriterOptions::default();
            assert!(writer.write(&doc, &options, &mut context).is_ok());
            let Block::Table(table) = &mut doc.blocks[0] else {
                panic!("expected a table")
            };
            table.bodies[0].body[0].cells[0].col_span = 2;
            assert!(writer.write(&doc, &options, &mut context).is_err());
        }
    }
    let mut context = Context::pure();
    context.limits.max_table_cells = 0;
    for (format, input) in [
        ("mediawiki", "{|\n! H !! J\n|-\n| a\n|}\n"),
        ("dokuwiki", "^ H ^ J ^\n| a |\n"),
        ("jira", "||H||J||\n|a|\n"),
        ("creole", "|=H|=J|\n|a|\n"),
    ] {
        assert!(read(format, input, &mut context).is_err());
    }
    assert!(read("creole", "|a|\n", &mut context).is_err());
    let mut doc = read("mediawiki", "{|\n| x\n|}\n", &mut Context::pure()).unwrap();
    let Block::Table(table) = &mut doc.blocks[0] else {
        panic!("expected a table")
    };
    table.bodies[0].body[0].cells[0].col_span = i64::MAX;
    assert!(
        super::writer("mediawiki")
            .unwrap()
            .write(&doc, &WriterOptions::default(), &mut Context::pure())
            .is_err()
    );
}

#[test]
fn limits_use_bytes_and_include_nested_inline_and_block_content() {
    let mut context = Context::pure();
    context.limits.max_input_size = 5;
    assert!(read("creole", "ééé", &mut context).is_err());
    let mut context = Context::pure();
    context.limits.max_nesting_depth = 2;
    assert!(read("mediawiki", "* text<ref><sup>x</sup></ref>", &mut context).is_err());
    let template = format!("{}<!-- c -->{}", "{{a|".repeat(200), "}}".repeat(200));
    let doc = read("mediawiki", &template, &mut context).unwrap();
    assert!(matches!(doc.blocks.as_slice(), [Block::RawBlock(_, _)]));
    assert!(
        read(
            "jira",
            "{quote}\n{panel:title=x}\n{color:red}y{color}\n{panel}\n{quote}",
            &mut context
        )
        .is_err()
    );
}

#[test]
fn writer_templates_render_the_body_and_includes() {
    let doc = Pandoc {
        blocks: vec![Block::Para(vec![Inline::Str("hello".into())])],
        ..Default::default()
    };
    for (name, expected) in [
        ("mediawiki", "BEFORE\n\n__TOC__\n\nhello\n\nAFTER\n"),
        ("dokuwiki", "BEFORE\n\n__TOC__\n\nhello\n\nAFTER\n"),
        ("jira", "BEFORE\n\nhello\n\nAFTER\n"),
    ] {
        let mut opts = WriterOptions::default();
        opts.table_of_contents = true;
        opts.variables
            .insert("include-before", Val::List(vec![Val::text("BEFORE")]));
        opts.variables
            .insert("include-after", Val::List(vec![Val::text("AFTER")]));
        opts.template = Some(
            Template::compile(
                name,
                randoc_data::default_template(name).unwrap(),
                128,
                &mut |_: &str| -> Result<String, ()> { Err(()) },
            )
            .unwrap(),
        );
        let writer = super::writer(name).unwrap();
        assert!(writer.renders_template());
        let bytes = writer
            .write(&doc, &opts, &mut Context::pure())
            .unwrap()
            .into_bytes();
        assert_eq!(bytes, expected.as_bytes());
    }
    assert!(super::writer("creole").is_none());
}

#[test]
fn jira_writes_div_anchors_before_the_content() {
    let paragraph = || Block::Para(vec![Inline::Str("Text".into())]);
    let div = |id, children| Block::Div(Attr::new(id, vec![], vec![]), children);
    let code = || Block::CodeBlock(Attr::default(), "Code".into());
    let panel = Block::Div(
        Attr::new("panel", vec![], vec![("color".into(), "red".into())]),
        vec![paragraph()],
    );
    let code_panel = Block::Div(
        Attr::new("code", vec!["panel".into()], vec![]),
        vec![code()],
    );
    for (block, expected) in [
        (div("empty", vec![]), "{anchor:empty}\n\n"),
        (div("para", vec![paragraph()]), "{anchor:para}Text\n\n"),
        (
            div("outer", vec![div("inner", vec![paragraph()])]),
            "{anchor:outer}{anchor:inner}Text\n\n",
        ),
        (
            div("outer", vec![div("", vec![code()])]),
            "{anchor:outer}\n\n{noformat}\nCode{noformat}\n",
        ),
        (panel, "{panel:color=red}\n{anchor:panel}Text\n{panel}\n"),
        (
            code_panel,
            "{panel}\n{anchor:code}\n\n{noformat}\nCode{noformat}{panel}\n",
        ),
        (
            div(
                "raw",
                vec![Block::RawBlock(Format("jira".into()), "Raw\n".into())],
            ),
            "{anchor:raw}Raw\n\n\n",
        ),
    ] {
        let doc = Pandoc {
            blocks: vec![block, paragraph()],
            ..Pandoc::default()
        };
        let actual = super::writer("jira")
            .unwrap()
            .write(&doc, &WriterOptions::default(), &mut Context::pure())
            .unwrap()
            .into_bytes();
        assert_eq!(actual, format!("{expected}Text\n").as_bytes());
    }
}

#[test]
fn jira_writes_code_and_note_boundaries() {
    let document = Pandoc {
        blocks: vec![Block::Para(vec![
            Inline::Str("a".into()),
            Inline::Note(vec![Block::Para(vec![Inline::Str("b".into())])]),
            Inline::Str("c".into()),
            Inline::Note(vec![Block::Plain(vec![Inline::Str("d".into())])]),
        ])],
        ..Default::default()
    };
    let writer = super::writer("jira").unwrap();
    let output = writer
        .write(&document, &WriterOptions::default(), &mut Context::pure())
        .unwrap();
    assert_eq!(
        output.into_bytes(),
        b"a[1]c[2]\n\n\n\\[1] b\n\n\n\\[2] d\n\n"
    );

    let document = read(
        "jira",
        "{code:python|title=Example}\nx\n{code}\n",
        &mut Context::pure(),
    )
    .unwrap();
    let output = writer
        .write(&document, &WriterOptions::default(), &mut Context::pure())
        .unwrap();
    assert_eq!(output.into_bytes(), b"{code:python}\nx\n\n{code}\n");
}

#[test]
fn deep_inputs_and_asts_fit_a_two_mib_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut context = Context::pure();
            let tables = format!("{}x\n{}", "{|\n|\n".repeat(160), "|}\n".repeat(160));
            let quotes = format!("{} x", ">".repeat(160));
            let lists = format!("{} x", "*".repeat(160));
            let macros = format!(
                "{}x\n{}",
                "{panel:title=x}\n".repeat(160),
                "{panel}\n".repeat(160)
            );
            assert!(read("mediawiki", &tables, &mut context).is_err());
            assert!(read("dokuwiki", &quotes, &mut context).is_err());
            assert!(read("mediawiki", &lists, &mut context).is_err());
            assert!(read("jira", &macros, &mut context).is_err());
            let mut block = Block::BlockQuote(vec![]);
            for _ in 0..160 {
                block = Block::BlockQuote(vec![block]);
            }
            let doc = Pandoc {
                blocks: vec![block],
                ..Default::default()
            };
            for format in ["dokuwiki", "jira"] {
                assert!(
                    super::writer(format)
                        .unwrap()
                        .write(&doc, &WriterOptions::default(), &mut context)
                        .is_err()
                );
            }
            for depth in [1, 128] {
                let table = format!("{}x\n{}", "{|\n|\n".repeat(depth), "|}\n".repeat(depth));
                let document = read("mediawiki", &table, &mut context).unwrap();
                assert!(
                    super::writer("mediawiki")
                        .unwrap()
                        .write(&document, &WriterOptions::default(), &mut context)
                        .is_ok()
                );
                let quote = format!("{} x", ">".repeat(depth));
                assert!(read("dokuwiki", &quote, &mut context).is_ok());
                let panels = format!(
                    "{}x\n{}",
                    "{panel:title=x}\n".repeat(depth - 1),
                    "{panel}\n".repeat(depth - 1)
                );
                assert!(read("jira", &panels, &mut context).is_ok());
                let notes = format!("{}x{}", "<ref>".repeat(depth), "</ref>".repeat(depth));
                assert!(read("mediawiki", &notes, &mut context).is_ok());
                let superscripts = format!("{}x{}", "<sup>".repeat(depth), "</sup>".repeat(depth));
                assert!(read("mediawiki", &superscripts, &mut context).is_ok());
                let links = format!("{}x{}", "[[a|".repeat(depth), "]]".repeat(depth));
                assert!(read("dokuwiki", &links, &mut context).is_ok());
                assert!(read("mediawiki", &links, &mut context).is_ok());

                let html_list = format!(
                    "{}x{}",
                    "<ul><li>".repeat(depth),
                    "</li></ul>".repeat(depth)
                );
                let document = read("mediawiki", &html_list, &mut context).unwrap();
                let mut blocks = document.blocks.as_slice();
                for _ in 0..depth {
                    let [Block::BulletList(items)] = blocks else {
                        panic!("expected a bullet list");
                    };
                    blocks = &items[0];
                }
                assert_eq!(blocks, [Block::Plain(vec![Inline::Str("x".into())])]);

                let notes = format!("{}x{}", "((".repeat(depth), "))".repeat(depth));
                let document = read("dokuwiki", &notes, &mut context).unwrap();
                let mut blocks = document.blocks.as_slice();
                for _ in 0..depth {
                    let [Block::Para(items) | Block::Plain(items)] = blocks else {
                        panic!("expected note content");
                    };
                    let [Inline::Note(nested)] = items.as_slice() else {
                        panic!("expected a note");
                    };
                    blocks = nested;
                }
                assert!(matches!(blocks, [Block::Para(items) | Block::Plain(items)]
                    if items == &[Inline::Str("x".into())]));
                let mixed = format!(
                    "{{|\n| <ref>{}x</ref>\n|}}",
                    "*".repeat(depth.saturating_sub(2).max(1))
                );
                assert!(read("mediawiki", &mixed, &mut context).is_ok());
            }
            for (format, text) in [("creole", "**//x//**"), ("jira", "{*}{_}x{_}{*}")] {
                let document = read(format, text, &mut context).unwrap();
                assert!(matches!(
                    document.blocks.as_slice(),
                    [Block::Para(items)]
                        if matches!(items.as_slice(), [Inline::Strong(items)]
                            if matches!(items.as_slice(), [Inline::Emph(_)]))
                ));
                if let Some(writer) = super::writer(format) {
                    let mut item = Inline::Str("x".into());
                    for _ in 0..128 {
                        item = Inline::Emph(vec![item]);
                    }
                    let document = Pandoc {
                        blocks: vec![Block::Para(vec![item])],
                        ..Default::default()
                    };
                    assert!(
                        writer
                            .write(&document, &WriterOptions::default(), &mut context)
                            .is_ok()
                    );
                }
            }
            context.limits.max_nesting_depth = 160;
            assert!(read("mediawiki", &lists, &mut context).is_ok());
            assert!(read("dokuwiki", &quotes, &mut context).is_ok());
            assert!(
                super::writer("dokuwiki")
                    .unwrap()
                    .write(&doc, &WriterOptions::default(), &mut context)
                    .is_err()
            );
            context.limits.max_nesting_depth = 161;
            assert!(
                super::writer("dokuwiki")
                    .unwrap()
                    .write(&doc, &WriterOptions::default(), &mut context)
                    .is_ok()
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn mediawiki_thumbnail_duplication_respects_the_output_limit() {
    let mut block = Block::Plain(vec![Inline::Str("x".into())]);
    for _ in 0..40 {
        block = Block::Plain(vec![Inline::Image(
            Attr::default(),
            vec![Inline::Note(vec![block])],
            Target::new("a.png", "fig:"),
        )]);
    }
    let doc = Pandoc {
        blocks: vec![block],
        ..Default::default()
    };
    let mut context = Context::pure();
    context.limits.max_input_size = 4096;
    let error = super::writer("mediawiki")
        .unwrap()
        .write(&doc, &WriterOptions::default(), &mut context)
        .unwrap_err();
    assert!(error.to_string().contains("output size limit"));
}

#[test]
fn dokuwiki_table_padding_respects_the_output_limit() {
    let doc = read(
        "dokuwiki",
        &format!("| {} |\n{}", "x".repeat(80), "| y |\n".repeat(4)),
        &mut Context::pure(),
    )
    .unwrap();
    let mut context = Context::pure();
    context.limits.max_input_size = 150;
    let error = super::writer("dokuwiki")
        .unwrap()
        .write(&doc, &WriterOptions::default(), &mut context)
        .unwrap_err();
    assert!(error.to_string().contains("output size limit"));
}

#[test]
fn mediawiki_image_options_preserve_captions_and_fall_back_to_links() {
    for field in ["thumb x", "leftover", "frame |Cap"] {
        let doc = read(
            "mediawiki",
            &format!("x [[File:a.png|{field}]]"),
            &mut Context::pure(),
        )
        .unwrap();
        assert!(matches!(
            doc.blocks.as_slice(),
            [Block::Para(items)] if matches!(
                items.last(), Some(Inline::Link(attr, _, target))
                    if attr.has_class("wikilink") && target.title == field
            )
        ));
    }
    let doc = read(
        "mediawiki",
        "x [[File:a.png| thumb|Cap ]]",
        &mut Context::pure(),
    )
    .unwrap();
    assert!(matches!(
        doc.blocks.as_slice(),
        [Block::Para(items)] if matches!(
            items.last(), Some(Inline::Image(_, alt, target))
                if alt.first() == Some(&Inline::Space)
                    && alt.last() == Some(&Inline::Space)
                    && target.title == " thumb|Cap "
        )
    ));
}

#[test]
fn mediawiki_links_can_extend_over_line_breaks() {
    let doc = read(
        "mediawiki",
        "[https://example.org a\nb]\n\n[[a\n\nb]]\n\n[[x|a\n\nb]]",
        &mut Context::pure(),
    )
    .unwrap();
    let [
        Block::Para(external),
        Block::Para(wiki),
        Block::Para(first),
        Block::Para(last),
    ] = doc.blocks.as_slice()
    else {
        panic!("expected four paragraphs");
    };
    assert!(matches!(external.as_slice(), [Inline::Link(_, items, _)]
        if items.contains(&Inline::SoftBreak)));
    assert!(matches!(wiki.as_slice(), [Inline::Link(_, _, target)]
        if target.url == "a_b"));
    assert_eq!(first, &[Inline::Str("[[x|a".into())]);
    assert_eq!(last, &[Inline::Str("b]]".into())]);
}

#[test]
fn mediawiki_table_splits_same_line_regions_and_keeps_attribute_continuations() {
    let doc = read(
        "mediawiki",
        "{|\n! <nowiki>a!!b</nowiki>\n|-\n| {{x|a||b}}\n| rowspan=2 more\n|}",
        &mut Context::pure(),
    )
    .unwrap();
    let [Block::Table(table)] = doc.blocks.as_slice() else {
        panic!("expected a table");
    };
    assert_eq!(table.head.rows[0].cells.len(), 2);
    let [left, right] = table.bodies[0].body[0].cells.as_slice() else {
        panic!("expected two cells");
    };
    assert_eq!(
        left.content,
        [Block::Plain(vec![Inline::Str("{{x|a".into())])]
    );
    assert!(matches!(right.content.as_slice(), [Block::Plain(items)]
        if items.get(1) == Some(&Inline::SoftBreak)));
    assert!(read("mediawiki", "{|\n| a=b more\n|}", &mut Context::pure()).is_err());
}

#[test]
fn jira_code_macros_end_at_the_first_literal_closer() {
    let doc = read(
        "jira",
        "{code:python}\n{code:java}\n{color:red}[x]{color}\n{code}\n\n{noformat}\n{noformat:x}\n{noformat}",
        &mut Context::pure(),
    )
    .unwrap();
    let [
        Block::CodeBlock(code_attr, code),
        Block::CodeBlock(literal_attr, literal),
    ] = doc.blocks.as_slice()
    else {
        panic!("expected two code blocks");
    };
    assert!(code_attr.has_class("python"));
    assert_eq!(code, "{code:java}\n{color:red}[x]{color}\n");
    assert!(literal_attr.is_empty());
    assert_eq!(literal, "{noformat:x}\n");
}
