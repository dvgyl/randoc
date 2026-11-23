#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use randoc_ast::{Block, Inline, Pandoc};
use randoc_core::{
    Context, Reader, ReaderOptions, Result, Sources, WriterOptions, default_extensions,
};

const READERS: [&str; 3] = ["tikiwiki", "twiki", "vimwiki"];
const WRITERS: [&str; 2] = ["xwiki", "zimwiki"];

fn read(format: &str, text: &str, context: &mut Context) -> Result<Pandoc> {
    let Some(Reader::Text(reader)) = crate::reader(format) else {
        panic!("Missing reader for {format}");
    };
    let mut options = ReaderOptions::default();
    options.extensions = default_extensions(format);
    reader.read(&Sources::from_text(text), &options, context)
}

fn write(format: &str, doc: &Pandoc, context: &mut Context) -> Result<String> {
    let writer = crate::writer(format).expect("Get wiki writer");
    let bytes = writer
        .write(doc, &WriterOptions::default(), context)?
        .into_bytes();
    Ok(String::from_utf8(bytes).expect("Decode wiki text"))
}

#[test]
fn readers_use_heap_frames_on_small_stacks() {
    for format in READERS {
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || {
                let mut text = String::new();
                use std::fmt::Write as _;
                for level in 1..=512 {
                    match format {
                        "tikiwiki" => writeln!(text, "{} Item", "*".repeat(level)).unwrap(),
                        "twiki" => writeln!(text, "{}* Item", " ".repeat(3 * level)).unwrap(),
                        _ => writeln!(text, "{}* Item", " ".repeat(level)).unwrap(),
                    }
                }
                let mut context = Context::pure();
                context.limits.max_nesting_depth = 511;
                assert!(read(format, &text, &mut context).is_err());
                context.limits.max_nesting_depth = 512;
                let doc = read(format, &text, &mut context).expect("Read the nested list");
                let mut count = 0;
                let mut current = doc.blocks.as_slice();
                while let Some(Block::BulletList(items)) =
                    current.iter().find(|b| matches!(b, Block::BulletList(_)))
                {
                    count += 1;
                    current = &items[0];
                }
                assert_eq!(count, 512);
                if format == "twiki" {
                    let text = format!("{}Item</i>", "<i>".repeat(512));
                    assert!(read(format, &text, &mut context).is_ok());
                    context.limits.max_nesting_depth = 511;
                    assert!(read(format, &text, &mut context).is_err());
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

#[test]
fn writers_use_heap_frames_on_small_stacks() {
    for format in WRITERS {
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || {
                let mut content = vec![Block::Plain(vec![Inline::Str("Item".into())])];
                for _ in 0..512 {
                    content = vec![Block::BulletList(vec![content])];
                }
                let doc = Pandoc {
                    blocks: content,
                    ..Pandoc::default()
                };
                let mut context = Context::pure();
                context.limits.max_nesting_depth = 511;
                assert!(write(format, &doc, &mut context).is_err());
                context.limits.max_nesting_depth = 512;
                let output = write(format, &doc, &mut context).expect("Write the nested list");
                assert!(output.contains("Item"));
                assert_eq!(output.lines().count(), 512);

                let mut item = Inline::Str("Item".into());
                for _ in 0..512 {
                    item = Inline::Emph(vec![item]);
                }
                let doc = Pandoc {
                    blocks: vec![Block::Para(vec![item])],
                    ..Pandoc::default()
                };
                assert!(write(format, &doc, &mut context).is_ok());
                context.limits.max_input_size = 32;
                assert!(write(format, &doc, &mut context).is_err());
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

#[test]
fn reader_table_limits_count_generated_cells() {
    for (format, input, generated) in [
        ("tikiwiki", "||A|B\nP|Q||", 2),
        ("twiki", "|*A*|*B*|\n|P|Q|", 0),
        ("vimwiki", "|A|B|\n|---|---|\n|P|Q|", 0),
        ("tikiwiki", "||A|B\nP||", 3),
        ("twiki", "|*A*|*B*|\n|P|", 1),
        ("vimwiki", "|A|B|\n|---|---|\n|P|", 1),
    ] {
        let mut context = Context::pure();
        if generated > 0 {
            context.limits.max_table_cells = generated - 1;
            assert!(read(format, input, &mut context).is_err(), "{format}");
        }
        context.limits.max_table_cells = generated;
        let doc = read(format, input, &mut context).unwrap();
        assert!(matches!(doc.blocks.as_slice(), [Block::Table(_)]));
    }
}

#[test]
fn writer_table_limits_count_generated_cells() {
    let mut context = Context::pure();
    context.limits.max_table_cells = 0;
    let mut doc = read(
        "vimwiki",
        "|A|B|\n|---|---|\n|P|Q|\n|R|S|\n|U|V|\n",
        &mut context,
    )
    .unwrap();
    for format in WRITERS {
        assert!(write(format, &doc, &mut context).is_ok(), "{format}");
    }
    let Block::Table(table) = &mut doc.blocks[0] else {
        panic!("Expected a table");
    };
    let rows = &mut table.bodies[0].body;
    rows[0].cells[0].row_span = 2;
    rows[1].cells.remove(0);
    rows[2].cells[0].col_span = 2;
    rows[2].cells.pop();

    for (format, expected) in [
        ("xwiki", "|=A |=B\n|P |Q\n| |S\n|U |\n"),
        ("zimwiki", "|A|B|\n|-|-|\n|P|Q|\n| |S|\n|U| |\n"),
    ] {
        context.limits.max_table_cells = 1;
        assert!(write(format, &doc, &mut context).is_err(), "{format}");
        context.limits.max_table_cells = 2;
        assert_eq!(write(format, &doc, &mut context).unwrap(), expected);
    }
}

#[test]
fn writers_check_output_size_at_the_limit() {
    use randoc_core::Error;

    let doc = Pandoc {
        blocks: vec![
            Block::Para(vec![
                Inline::Str("a".repeat(10 << 20)),
                Inline::Str("b".repeat(9 << 20)),
            ]),
            Block::BlockQuote(vec![Block::Para(vec![Inline::Str("c".repeat(1 << 20))])]),
        ],
        ..Pandoc::default()
    };
    for format in WRITERS {
        let mut context = Context::pure();
        let output = write(format, &doc, &mut context).unwrap();
        assert_eq!(
            output.len(),
            (20 << 20) + if format == "xwiki" { 6 } else { 5 }
        );
        context.limits.max_input_size = output.len() as u64;
        assert_eq!(write(format, &doc, &mut context).unwrap(), output);
        context.limits.max_input_size -= 1;
        assert!(matches!(
            write(format, &doc, &mut context),
            Err(Error::Parse(_))
        ));
    }
}

#[test]
fn tiki_emphasis_spans_lines() {
    let doc = read("tikiwiki", "__a\nb__\n''c\nd''", &mut Context::pure()).unwrap();
    let a = Inline::Strong(vec![
        Inline::Str("a".into()),
        Inline::Space,
        Inline::Str("b".into()),
    ]);
    let b = Inline::Emph(vec![
        Inline::Str("c".into()),
        Inline::Space,
        Inline::Str("d".into()),
    ]);
    assert_eq!(doc.blocks, vec![Block::Para(vec![a, Inline::Space, b])]);
}

#[test]
fn code_closers_keep_line_suffixes() {
    let mut context = Context::pure();
    for (format, input) in [
        ("vimwiki", "{{{\ncode\nfoo }}}\nafter"),
        ("vimwiki", "{{{\ncode\n}}} foo\nafter"),
        ("tikiwiki", "{CODE(colors=\"c\")}\ncode\n{CODE} tail"),
        ("tikiwiki", "{CODE()}a{CODE} tail\nb{CODE}"),
    ] {
        let doc = read(format, input, &mut context).unwrap();
        assert!(
            matches!(doc.blocks.as_slice(), [Block::Para(_)]),
            "{input:?}"
        );
    }

    let doc = read("vimwiki", "{{{\ncode\n  }}}  \nafter", &mut context).unwrap();
    assert!(
        matches!(doc.blocks.as_slice(), [Block::CodeBlock(_, code), Block::Para(_)] if code == "code")
    );

    let doc = read(
        "twiki",
        "<verbatim>\ncode\n</verbatim> tail\nafter",
        &mut context,
    )
    .unwrap();
    assert_eq!(
        doc.blocks,
        vec![
            Block::CodeBlock(Default::default(), "code".into()),
            Block::Para(vec![
                Inline::Str("tail".into()),
                Inline::Space,
                Inline::Str("after".into())
            ]),
        ]
    );
    let doc = read(
        "twiki",
        "<verbatim>a</verbatim><verbatim>b</verbatim> tail",
        &mut context,
    )
    .unwrap();
    assert!(
        matches!(doc.blocks.as_slice(), [Block::CodeBlock(_, a), Block::CodeBlock(_, b), Block::Para(_)] if a == "a" && b == "b")
    );
    let doc = read(
        "twiki",
        "<verbatim>a</verbatim> <verbatim>b</verbatim>",
        &mut context,
    )
    .unwrap();
    assert!(
        matches!(doc.blocks.as_slice(), [Block::CodeBlock(_, a), Block::Para(items)]
        if a == "a" && items == &[Inline::Str("<verbatim>b</verbatim>".into())])
    );

    let doc = read("vimwiki", "{{{\nfoo }}}\n}}}\nafter", &mut context).unwrap();
    assert!(
        matches!(doc.blocks.as_slice(), [Block::CodeBlock(_, code), Block::Para(_)]
        if code == "foo }}}")
    );
}

#[test]
fn twiki_macros_use_the_current_frame() {
    let mut context = Context::pure();
    let doc = read("twiki", "[[x][*%A{x]] %B{y}%", &mut context).unwrap();
    let [Block::Para(items)] = doc.blocks.as_slice() else {
        panic!("Expected a paragraph");
    };
    assert!(
        matches!(items.as_slice(), [Inline::Link(_, label, _), Inline::Space, Inline::Span(attr, body)]
        if label == &[Inline::Str("*%A{x".into())] && attr.classes == ["twiki-macro", "B"]
            && body == &[Inline::Str("y".into())])
    );

    for marker in ["_", "*", "__"] {
        let input = format!("{marker}%A{{x{marker} %B{{y}}%");
        let doc = read("twiki", &input, &mut context).unwrap();
        let [Block::Para(items)] = doc.blocks.as_slice() else {
            panic!("Expected a paragraph");
        };
        assert!(
            matches!(items.as_slice(), [Inline::Str(prefix), Inline::Span(attr, body)]
            if prefix == marker && attr.classes == ["twiki-macro", "A"]
                && body == &[Inline::Str(format!("x{marker} %B{{y"))])
        );
    }
    let doc = read("twiki", "_%A_x_%", &mut context).unwrap();
    let [Block::Para(items)] = doc.blocks.as_slice() else {
        panic!("Expected a paragraph");
    };
    assert!(
        matches!(items.as_slice(), [Inline::Str(prefix), Inline::Span(attr, body)]
        if prefix == "_" && attr.classes == ["twiki-macro", "A_x_"] && body.is_empty())
    );
}

#[test]
fn vimwiki_images_keep_single_classes() {
    for (attributes, expected) in [
        ("class=\"c\"", &["c"][..]),
        ("class=\"c\" id=\"i\"", &["c"][..]),
        ("class=\"c d\" id=\"i\"", &[][..]),
        ("class=\" c \"", &[][..]),
        ("class=c", &[][..]),
        ("class=\"c\" class=\"d\"", &["c"][..]),
    ] {
        for alt in ["alt", ""] {
            let input = format!("{{{{img.png|{alt}|{attributes}}}}}");
            let doc = read("vimwiki", &input, &mut Context::pure()).unwrap();
            let [Block::Para(items)] = doc.blocks.as_slice() else {
                panic!("Expected a paragraph");
            };
            assert!(
                matches!(items.as_slice(), [Inline::Image(attr, _, _)] if attr.classes == expected),
                "{input}"
            );
        }
    }
}

#[test]
fn vimwiki_local_targets_keep_wikilink_class() {
    let doc = read(
        "vimwiki",
        "[[local:f.txt]] [[file:f.txt]]",
        &mut Context::pure(),
    )
    .unwrap();
    let [Block::Para(items)] = doc.blocks.as_slice() else {
        panic!("Expected a paragraph");
    };
    assert!(
        matches!(items.as_slice(), [Inline::Link(local, _, a), Inline::Space, Inline::Link(file, _, b)]
        if local.classes == ["wikilink"] && file.classes.is_empty() && a.url == "file:f.txt" && a == b)
    );
}

#[test]
fn tiki_triple_pipe_is_literal() {
    for input in ["|||a|b||", "||a|||", "||a|||b||", "||||"] {
        let doc = read("tikiwiki", input, &mut Context::pure()).unwrap();
        assert_eq!(
            doc.blocks,
            vec![Block::Para(vec![Inline::Str(input.into())])]
        );
    }
}

#[test]
fn vimwiki_inner_style_uses_closing_marker_first() {
    let doc = read("vimwiki", "*a*b* c", &mut Context::pure()).unwrap();
    let [Block::Para(items)] = doc.blocks.as_slice() else {
        panic!("Expected a paragraph");
    };
    assert!(
        matches!(items.as_slice(), [Inline::Str(prefix), Inline::Span(attr, empty), Inline::Strong(body), Inline::Space, Inline::Str(suffix)]
        if prefix == "*a" && attr.id == "b" && empty.is_empty() && body == &[Inline::Str("b".into())] && suffix == "c")
    );

    let doc = read("vimwiki", "_a _b_ c_", &mut Context::pure()).unwrap();
    assert_eq!(
        doc.blocks,
        vec![Block::Para(vec![Inline::Emph(vec![
            Inline::Str("a".into()),
            Inline::Space,
            Inline::Emph(vec![Inline::Str("b".into())]),
            Inline::Space,
            Inline::Str("c".into()),
        ])])]
    );
}

#[test]
fn wiki_writers_escape_literal_markup() {
    let cases = [
        ("*", "*", "*"),
        ("**", "~*~*", "''**''"),
        ("***", "~*~**", "''**''*"),
        ("////", "~/~/~/~/", "''//''''//''"),
        ("///", "~/~//", "''//''/"),
        ("--", "~-~-", "--"),
        ("---", "~-~--", "---"),
        ("----", "~-~-~-~-", "----"),
        (",,,", "~,~,,", ",,,"),
        ("^^^", "~^~^^", "^^^"),
        ("~", "~~", "~"),
        ("~~~", "~~~~~~", "''~~''~"),
        ("___", "___", "''__''_"),
        ("[[", "[[", "[["),
        ("{{", "{{", "{{"),
        ("##", "##", "##"),
        ("||", "||", "||"),
        ("''", "''", "''"),
        ("http://x.com", "http:~/~/x.com", "http:''//''x.com"),
    ];
    let items = cases
        .iter()
        .enumerate()
        .flat_map(|(i, &(value, _, _))| {
            (i > 0)
                .then_some(Inline::Space)
                .into_iter()
                .chain([Inline::Str(value.into())])
        })
        .collect();
    let doc = Pandoc {
        blocks: vec![Block::Para(items)],
        ..Pandoc::default()
    };
    for (i, format) in WRITERS.into_iter().enumerate() {
        let tokens: Vec<_> = cases
            .iter()
            .map(|&(_, xwiki, zim)| if i == 0 { xwiki } else { zim })
            .collect();
        assert_eq!(
            write(format, &doc, &mut Context::pure()).unwrap(),
            format!("{}\n", tokens.join(" "))
        );
    }
}

#[test]
fn wiki_writer_links_use_original_labels() {
    use randoc_ast::{Attr, Target};

    let link = |label: &str, target| {
        Inline::Link(
            Attr::default(),
            vec![Inline::Str(label.into())],
            Target::new(target, ""),
        )
    };
    let doc = Pandoc {
        blocks: vec![Block::Para(vec![
            link("http://x.com", "http://x.com"),
            Inline::Space,
            link("m**", "mailto:me@x.org"),
        ])],
        ..Pandoc::default()
    };
    assert_eq!(
        write("xwiki", &doc, &mut Context::pure()).unwrap(),
        "http://x.com [[m~*~*>>mailto:me@x.org]]\n"
    );
    assert_eq!(
        write("zimwiki", &doc, &mut Context::pure()).unwrap(),
        "http://x.com <m**>\n"
    );
}
