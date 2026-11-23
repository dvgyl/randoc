//! Check RTF behavior and conversion limits.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use randoc_ast::{
    Attr, Block, Cell, ColSpec, ColWidth, Inline, MathType, MetaValue, Pandoc, Row, Table,
    TableBody, Target,
};
use randoc_core::{
    Context, Error, LogMessage, MediaItem, Reader, ReaderOptions, Sources, WriterOptions,
};

fn read(text: &str, context: &mut Context) -> randoc_core::Result<Pandoc> {
    let Reader::Text(reader) = randoc_rtf::reader("rtf").unwrap() else {
        panic!("Text reader required");
    };
    reader.read(
        &Sources::from_text(text),
        &ReaderOptions::default(),
        context,
    )
}

fn write(doc: &Pandoc, context: &mut Context) -> randoc_core::Result<Vec<u8>> {
    randoc_rtf::writer("rtf")
        .unwrap()
        .write(doc, &WriterOptions::default(), context)
        .map(randoc_core::Output::into_bytes)
}

#[test]
fn reads_character_runs_and_unicode() {
    let input = r"{\rtf1 a{\b b{\i c}d}e\uc2\u55357??\u56832??\par X\u945\tab Y}";
    let doc = read(input, &mut Context::pure()).unwrap();
    assert_eq!(
        doc.blocks,
        vec![
            Block::Para(vec![
                Inline::Str("a".into()),
                Inline::Strong(vec![
                    Inline::Str("b".into()),
                    Inline::Emph(vec![Inline::Str("c".into())]),
                    Inline::Str("d".into()),
                ]),
                Inline::Str("e😀".into()),
            ]),
            Block::Para(vec![
                Inline::Str("Xα".into()),
                Inline::Space,
                Inline::Str("Y".into())
            ]),
        ]
    );
}

#[test]
fn writes_fragment_and_round_trips_punctuation() {
    let doc = Pandoc {
        blocks: vec![Block::Para(vec![Inline::Str("a’—é😀".into())])],
        ..Pandoc::default()
    };
    let mut context = Context::pure();
    let bytes = write(&doc, &mut context).unwrap();
    assert_eq!(bytes, b"{\\pard \\ql \\f0 \\fs24 \\sa180 \\li0 \\fi0 a\\u8217'\\u8212-\\u233 ?\\u55357 ?\\u56832 ?\\par}\n");
    assert_eq!(
        read(std::str::from_utf8(&bytes).unwrap(), &mut context).unwrap(),
        doc
    );
    assert!(randoc_rtf::writer("rtf").unwrap().renders_template());
    assert!(randoc_rtf::reader("other").is_none());
}

#[test]
fn code_blocks_omit_one_final_newline() {
    let cases = [
        ("", ""),
        ("x", "x"),
        ("x\n", "x"),
        ("x\ny\n", "x\\line\ny"),
        ("x\n\n", "x\\line\n"),
    ];
    for (input, body) in cases {
        let doc = Pandoc {
            blocks: vec![Block::CodeBlock(Attr::default(), input.into())],
            ..Pandoc::default()
        };
        let expected =
            format!("{{\\pard \\ql \\f0 \\fs24 \\sa180 \\li0 \\fi0 \\f1 \\fs24 {body}\\par}}\n");
        assert_eq!(
            write(&doc, &mut Context::pure()).unwrap(),
            expected.as_bytes(),
            "{input:?}"
        );
    }
}

#[test]
fn reports_parse_errors_at_the_bad_character() {
    let error = read(r"{\rtf1 \'zz}", &mut Context::pure()).unwrap_err();
    assert_eq!(error.exit_code(), 64);
    assert_eq!(
        error.to_string(),
        "Error at (line 1, column 10):\nunexpected 'z'\n{\\rtf1 \\'zz}\n         ^"
    );
    let error = read(r"{\rtf1 \bin999999999999999999999 x}", &mut Context::pure()).unwrap_err();
    assert_eq!(error.exit_code(), 64);
}

#[test]
fn checks_input_group_and_cell_limits() {
    let mut ctx = Context::pure();
    ctx.limits.max_input_size = 3;
    assert!(read("abc", &mut ctx).is_ok());
    assert!(matches!(read("abcd", &mut ctx), Err(Error::App(_))));
    ctx.limits.max_input_size = 1024;
    ctx.limits.max_nesting_depth = 2;
    assert!(read("{{x}}", &mut ctx).is_ok());
    assert!(matches!(
        read(r"{\rtf1{\*\skip{x}}}", &mut ctx),
        Err(Error::App(_))
    ));
    assert!(matches!(
        read(r"{\rtf1\intbl\itap9223372036854775807 x}", &mut ctx),
        Err(Error::App(_))
    ));
    assert!(matches!(
        read(r"{\rtf1\ilvl9223372036854775807 x}", &mut ctx),
        Err(Error::App(_))
    ));

    ctx.limits.max_nesting_depth = 128;
    let rows = r"{\rtf1\trowd\intbl a\cell\row\trowd\intbl b\cell c\cell\row}";
    ctx.limits.max_table_cells = 6;
    let doc = read(rows, &mut ctx).unwrap();
    assert!(write(&doc, &mut ctx).is_ok());
    ctx.limits.max_table_cells = 5;
    assert!(matches!(read(rows, &mut ctx), Err(Error::App(_))));
    assert!(matches!(write(&doc, &mut ctx), Err(Error::App(_))));
}

#[test]
fn stages_pictures_and_checks_decoded_size() {
    let mut ctx = Context::pure();
    ctx.limits.max_resource_size = 3;
    let input = r"{\rtf1{\pict\jpegblip\bin3 a}b}{\pict\pngblip 01020304}}";
    assert!(matches!(read(input, &mut ctx), Err(Error::App(_))));
    assert!(ctx.media.is_empty());

    let doc = read(r"{\rtf1{\pict\jpegblip\bin3 a}b}}", &mut ctx).unwrap();
    let (key, item) = ctx.media.iter().next().unwrap();
    assert_eq!(item.contents, b"a}b");
    assert_eq!(item.mime_type, "image/jpeg");
    assert_eq!(
        key,
        &format!("{}.jpg", randoc_archive::media::sha1_hex(b"a}b"))
    );

    ctx.limits.max_resource_size = 2;
    assert!(matches!(write(&doc, &mut ctx), Err(Error::App(_))));
    assert_eq!(ctx.media.len(), 1);
}

#[test]
fn renders_embedded_png_with_dimensions() {
    let mut ctx = Context::pure();
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend(96u32.to_be_bytes());
    png.extend(48u32.to_be_bytes());
    ctx.media.insert(
        "p.png",
        MediaItem {
            mime_type: "image/png".into(),
            contents: png,
        },
    );
    let doc = Pandoc {
        blocks: vec![Block::Para(vec![Inline::Image(
            Attr::new("", vec![], vec![("width".into(), "2in".into())]),
            vec![],
            Target::new("p.png", ""),
        )])],
        ..Pandoc::default()
    };
    let rtf = String::from_utf8(write(&doc, &mut ctx).unwrap()).unwrap();
    assert!(rtf.contains(r"\pict\pngblip\picw96\pich48\picwgoal2880\pichgoal1440 89504e47"));
}

#[test]
fn handles_deep_groups_notes_tables_and_writer_in_two_mib() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut ctx = Context::pure();
            let input = format!("{}x{}", r"{\b ".repeat(128), "}".repeat(128));
            assert!(read(&input, &mut ctx).is_ok());
            let notes = format!("{}x{}", r"{\footnote ".repeat(120), "}".repeat(120));
            let doc = read(&notes, &mut ctx).unwrap();
            assert!(matches!(write(&doc, &mut ctx), Err(Error::App(_))));

            let mut table = String::from(r"{\rtf1");
            for depth in 1..=128 {
                table.push_str(&format!(r"\pard\intbl\itap{depth} X\par"));
            }
            for depth in (2..=128).rev() {
                table.push_str(&format!(r"\pard\intbl\itap{depth}\nestcell\nestrow"));
            }
            table.push_str(r"\pard\intbl\itap1\cell\row}");
            assert!(read(&table, &mut ctx).is_ok());
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn reads_picture_hex_across_lines() {
    let mut ctx = Context::pure();
    ctx.limits.max_resource_size = 3;
    read("{\\rtf1{\\pict\\pngblip abc\ndef}}", &mut ctx).unwrap();
    let (_, item) = ctx.media.iter().next().unwrap();
    assert_eq!(item.contents, [0xab, 0xcd, 0xef]);

    ctx.limits.max_resource_size = 2;
    assert!(matches!(
        read("{\\rtf1{\\pict\\pngblip abc\ndef}}", &mut ctx),
        Err(Error::App(_))
    ));
}

#[test]
fn finds_images_in_resource_paths_and_bounds_file_reads() {
    let mut host = randoc_core::PureHost::default();
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend(96u32.to_be_bytes());
    png.extend(48u32.to_be_bytes());
    let size = png.len() as u64;
    host.files.insert("images/p.png".into(), png);
    let mut ctx = Context::new(Box::new(host));
    ctx.state.resource_path = vec!["images".into()];
    ctx.limits.max_resource_size = size;
    let doc = Pandoc {
        blocks: vec![Block::Para(vec![Inline::Image(
            Attr::default(),
            vec![],
            Target::new("p.png", ""),
        )])],
        ..Pandoc::default()
    };
    let rtf = String::from_utf8(write(&doc, &mut ctx).unwrap()).unwrap();
    assert!(rtf.contains(r"\pict\pngblip\picw96\pich48"));
    ctx.limits.max_resource_size = size - 1;
    let fallback = String::from_utf8(write(&doc, &mut ctx).unwrap()).unwrap();
    assert!(fallback.contains(r"{\cf1 [image: p.png]\cf0}"));
}

#[test]
fn empty_table_of_contents_adds_no_line_before_an_include() {
    let doc = Pandoc::default();
    let mut options = WriterOptions::default();
    options.template = Some(
        randoc_templates::Template::compile::<String>(
            "default.rtf",
            randoc_data::default_template("rtf").unwrap(),
            128,
            &mut |_| Ok(String::new()),
        )
        .unwrap(),
    );
    options.variables.insert("include-before", "Before.");
    let writer = randoc_rtf::writer("rtf").unwrap();
    let expected = writer
        .write(&doc, &options, &mut Context::pure())
        .unwrap()
        .into_bytes();
    assert!(String::from_utf8_lossy(&expected).contains("\\hyphauto\n\nBefore.\n"));
    options.table_of_contents = true;
    assert_eq!(
        writer
            .write(&doc, &options, &mut Context::pure())
            .unwrap()
            .into_bytes(),
        expected
    );
}

#[test]
fn includes_div_headings_in_the_table_of_contents() {
    let doc = Pandoc {
        blocks: vec![Block::Div(
            Attr::default(),
            vec![Block::Header(
                1,
                Attr::new("test", vec![], vec![]),
                vec![Inline::Str("Heading".into())],
            )],
        )],
        ..Pandoc::default()
    };
    let template = randoc_templates::Template::compile::<String>(
        "default.rtf",
        randoc_data::default_template("rtf").unwrap(),
        128,
        &mut |_| Ok(String::new()),
    )
    .unwrap();
    let mut opts = WriterOptions::default();
    opts.table_of_contents = true;
    opts.template = Some(template);
    let bytes = randoc_rtf::writer("rtf")
        .unwrap()
        .write(&doc, &opts, &mut Context::pure())
        .unwrap()
        .into_bytes();
    let rendered = String::from_utf8(bytes).unwrap();
    assert!(rendered.starts_with(r"{\rtf1"));
    assert!(rendered.contains(r##"{\field{\*\fldinst{HYPERLINK "#test"}}"##));
}

fn text(value: &str) -> Inline {
    Inline::Str(value.into())
}

fn para(value: &str) -> Block {
    Block::Para(vec![text(value)])
}

fn fragment(blocks: Vec<Block>, context: &mut Context) -> String {
    let doc = Pandoc {
        blocks,
        ..Pandoc::default()
    };
    String::from_utf8(write(&doc, context).unwrap()).unwrap()
}

fn table(widths: &[ColWidth], content: Vec<Block>) -> Block {
    Block::Table(Box::new(Table {
        col_specs: widths
            .iter()
            .map(|width| ColSpec {
                width: *width,
                ..ColSpec::default()
            })
            .collect(),
        bodies: vec![TableBody {
            body: vec![Row {
                cells: (0..widths.len())
                    .map(|index| Cell {
                        content: if index == 0 { content.clone() } else { vec![] },
                        ..Cell::default()
                    })
                    .collect(),
                ..Row::default()
            }],
            ..TableBody::default()
        }],
        ..Table::default()
    }))
}

#[test]
fn keeps_lists_beside_tables_and_checks_container_depth_on_two_mib() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut context = Context::pure();
            context.limits.max_nesting_depth = 2;
            let input = r"{\rtf1 {\*\pn\pnlvlblt\ls1}\pard\intbl\itap1\ls1 A\par\pard\intbl\itap2 B\nestcell\nestrow\pard\intbl\itap1\cell\row}";
            let default = [ColWidth::ColWidthDefault];
            let inner = table(&default, vec![para("B")]);
            assert_eq!(
                read(input, &mut context).unwrap().blocks,
                [Block::BulletList(vec![vec![para("A")]]), table(&default, vec![inner])]
            );

            let input = r"{\rtf1 {\*\pn\pnlvlblt\ls1}\intbl\ls1 A\par\cell\row}";
            assert_eq!(
                read(input, &mut context).unwrap().blocks,
                [table(&default, vec![]), Block::BulletList(vec![vec![para("A")]])]
            );

            let with_table_note = r"{\rtf1 {\*\pn\pnlvlblt\ls1}\intbl\ls1 A\par\pard\intbl\itap2 B{\footnote N}\nestcell\nestrow\pard\intbl\cell\row}";
            assert!(matches!(read(with_table_note, &mut context), Err(Error::App(_))));
            let with_list_note = r"{\rtf1 {\*\pn\pnlvlblt\ls1}\ls1 A\par\ilvl1 B{\footnote N}}";
            assert!(matches!(read(with_list_note, &mut context), Err(Error::App(_))));

            context.limits.max_nesting_depth = 128;
            let mut deep = String::from(r"{\rtf1 {\*\pn\pnlvlblt\ls1}");
            for depth in 1..=128 {
                deep.push_str(&format!(
                    r"\pard\intbl\itap{depth}\ls1\ilvl{} A\par",
                    depth - 1
                ));
            }
            deep.push_str(r"\pard\intbl\itap128 B\par");
            for depth in (2..=128).rev() {
                deep.push_str(&format!(r"\pard\intbl\itap{depth}\nestcell\nestrow"));
            }
            deep.push_str(r"\pard\intbl\cell\row}");
            let document = read(&deep, &mut context).unwrap();
            assert!(matches!(&document.blocks[..], [Block::BulletList(_), Block::Table(_)]));
            assert!(matches!(
                read(&deep.replace(" B\\par", r" B{\footnote N}\par"), &mut context),
                Err(Error::App(_))
            ));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn writes_markers_for_empty_items_and_first_paragraphs() {
    let rtf = fragment(
        vec![Block::BulletList(vec![
            vec![],
            vec![Block::BlockQuote(vec![
                Block::Header(2, Attr::default(), vec![text("A")]),
                para("B"),
            ])],
        ])],
        &mut Context::pure(),
    );
    assert_eq!(
        rtf,
        concat!(
            r"{\pard \ql \f0 \fs24 \sa0 \li360 \fi-360 \ls1\ilvl0\tx360 {\listtext\tab \bullet \tab}\par}",
            "\n",
            r"{\pard \ql \f0 \fs24 \sa180 \li1080 \fi-360\ls1\ilvl0\tx360 {\listtext\tab \bullet \tab} \outlinelevel1 \b \fs32 A\par}",
            "\n",
            r"{\pard \ql \f0 \fs24 \sa180 \li1080 \fi0 B\sa180\par}",
            "\n",
        )
    );
}

#[test]
fn writes_mixed_fractional_and_default_column_boundaries() {
    use randoc_ast::ColWidth::{ColWidth as Width, ColWidthDefault};
    for (widths, boundaries) in [
        (vec![Width(0.3), ColWidthDefault], r"\cellx2592\cellx2592"),
        (vec![Width(0.3333); 3], r"\cellx2879\cellx5758\cellx8637"),
        (vec![ColWidthDefault; 2], r"\cellx4320\cellx8640"),
    ] {
        let rtf = fragment(vec![table(&widths, vec![])], &mut Context::pure());
        assert!(rtf.lines().any(|line| line == boundaries), "{rtf}");
    }
}

#[test]
fn writes_nested_cell_indentation_and_block_spacing() {
    let widths = [ColWidth::ColWidthDefault];
    let nested = table(&widths, vec![para("B")]);
    let rtf = fragment(
        vec![Block::BlockQuote(vec![table(
            &widths,
            vec![para("A"), Block::Plain(vec![text("P")]), nested],
        )])],
        &mut Context::pure(),
    );
    for paragraph in [
        r"\pard\intbl \ql \f0 \fs24 \sa180 \li720 \fi0 A\par",
        r"\pard\intbl \ql \f0 \fs24 \sa0 \li720 \fi0 P\par",
        r"\pard\intbl\intbl \ql \f0 \fs24 \sa180 \li720 \fi0 B\par",
    ] {
        assert!(rtf.contains(paragraph), "{rtf}");
    }
}

#[test]
fn reads_field_targets_and_preserves_result_run_spaces() {
    let mut context = Context::pure();
    for (instruction, target) in [
        (r#"HYPERLINK "https://e.test/a  b""#, "https://e.test/a  b"),
        (r#"HYPERLINK \l "mark""#, ""),
        (r#"HYPERLINK \\l "mark""#, ""),
        (r#"{HYPERLINK \\l "mark"}"#, "#mark"),
    ] {
        let rtf =
            format!(r"{{\rtf1 X{{\field{{\*\fldinst {instruction}}}{{\fldrslt  label }}}}Y}}");
        assert_eq!(
            read(&rtf, &mut context).unwrap().blocks,
            [Block::Para(vec![
                text("X"),
                Inline::Link(
                    Attr::default(),
                    vec![Inline::Space, text("label"), Inline::Space],
                    Target::new(target, ""),
                ),
                text("Y"),
            ])]
        );
    }
    let ungrouped = r#"{\rtf1 {\field\fldinst{HYPERLINK "https://e.test"}{\fldrslt A}}}"#;
    assert!(read(ungrouped, &mut context).unwrap().blocks.is_empty());
}

#[test]
fn reads_selected_picture_bytes_and_final_dimensions() {
    for (picture, expected) in [
        (r"01020304\bin2 AB", &b"AB"[..]),
        (r"\bin2 AB01020304", &b"AB"[..]),
        (r"\bin1 A01\bin1 B", &b"AB"[..]),
        (r"a\picw100 bc", &[0xbc][..]),
        (r"01020304\picw100 bc", &[0xbc][..]),
        ("abc\ndef", &[0xab, 0xcd, 0xef][..]),
    ] {
        let mut context = Context::pure();
        context.limits.max_resource_size = expected.len() as u64;
        let input = format!(
            r"{{\rtf1{{\pict\pngblip\picwgoal1440\picwgoal720\pichgoal720\pichgoal1440 {picture}}}}}"
        );
        let doc = read(&input, &mut context).unwrap();
        let [Block::Para(inlines)] = &doc.blocks[..] else {
            panic!("Expected a picture paragraph");
        };
        let [Inline::Image(attr, _, target)] = &inlines[..] else {
            panic!("Expected a picture");
        };
        assert_eq!(
            attr.attributes,
            [
                ("width".into(), "0.5in".into()),
                ("height".into(), "1.0in".into())
            ]
        );
        assert_eq!(context.media.get(&target.url).unwrap().contents, expected);
    }
}

#[test]
fn writes_math_signs_symbols_and_scripts_and_warns_for_tex_fallback() {
    let mut context = Context::pure();
    let rtf = fragment(
        [
            r"-x",
            r"a-b",
            r"\theta",
            r"x^2_i",
            r"\frac{1}{2}",
            r"\unknown{x}",
        ]
        .into_iter()
        .map(|tex| Block::Para(vec![Inline::Math(MathType::InlineMath, tex.into())]))
        .collect(),
        &mut context,
    );
    for expression in [
        r"\u8722 ?{\i x}",
        r"{\i a}\u8197 ?\u8722 ?\u8197 ?{\i b}",
        r"{\i \u952 ?}",
        r"{\i x}{\sub {\i i}}{\super 2}",
        r"$\\frac\{1\}\{2\}$",
        r"$\\unknown\{x\}$",
    ] {
        assert!(rtf.contains(&format!(r"\fi0 {expression}\par")), "{rtf}");
    }
    assert!(matches!(
        context.log(),
        [LogMessage::CouldNotConvertTexMath(fraction, _), LogMessage::CouldNotConvertTexMath(unknown, _)]
            if fraction == r"\frac{1}{2}" && unknown == r"\unknown{x}"
    ));
}

#[test]
fn metadata_lists_use_simple_markers_and_body_ids_start_at_one() {
    let mut doc = Pandoc {
        blocks: vec![Block::BulletList(vec![vec![para("A")]])],
        ..Pandoc::default()
    };
    doc.meta.insert(
        "title",
        MetaValue::MetaBlocks(vec![Block::BulletList(vec![vec![para("title")]])]),
    );
    let source = randoc_data::default_template("rtf").unwrap();
    let mut options = WriterOptions::default();
    options.template = Some(
        randoc_templates::Template::compile::<String>("default.rtf", source, 128, &mut |_| {
            Ok(String::new())
        })
        .unwrap(),
    );
    let render = |doc: &Pandoc| {
        String::from_utf8(
            randoc_rtf::writer("rtf")
                .unwrap()
                .write(doc, &options, &mut Context::pure())
                .unwrap()
                .into_bytes(),
        )
        .unwrap()
    };
    let rtf = render(&doc);
    assert_eq!(rtf.matches(r"\listtemplateid").count(), 1);
    assert!(rtf.contains(r"\fs12 \sa180 \li360 \fi-360 \bullet \tx360\tab title\sa180\par"));
    assert!(rtf.contains(r"\ls1\ilvl0\tx360 {\listtext\tab \bullet \tab} A"));
    let spacer = r"{\pard \ql \f0 \sa180 \li0 \fi0 \par}";
    assert!(!rtf.contains(spacer));

    doc.meta
        .insert("title", MetaValue::MetaBlocks(vec![para("title")]));
    let rtf = render(&doc);
    assert!(rtf.contains(r"\b \fs36 title\par"));
    assert!(rtf.contains(spacer));
}

#[test]
fn preserves_unicode_fallback_count_and_surrogate_boundaries() {
    let mut context = Context::pure();
    for (input, expected) in [
        (r"{\rtf1 \uc2\uc\u945??X}", vec![text("αX")]),
        (
            "{\\rtf1 \\uc2\\u945?\t?X}",
            vec![text("α"), Inline::Space, text("?X")],
        ),
        (r"{\rtf1 \uc0\u55357\emdash\u56832}", vec![text("—")]),
        (r"{\rtf1 \uc0\u55357{}\u56832}", vec![text("😀")]),
        (
            r"{\rtf1 \uc0\u55357{\b\u56832}}",
            vec![Inline::Strong(vec![text("😀")])],
        ),
        (
            r"{\rtf1 \uc0\u55357\line\u56832 X}",
            vec![Inline::LineBreak, text("X")],
        ),
        (r"{\rtf1 \uc0\u55357{\pard}\u56832}", vec![text("😀")]),
        (r"{\rtf1 \uc0\u55357{\v hidden}\u56832 X}", vec![text("X")]),
        (r"{\rtf1 \uc0\u9223372036854775808}", vec![text("\0")]),
        (
            r"{\rtf1 \uc0\u-9223372036854775809}",
            vec![text("\u{ffff}")],
        ),
    ] {
        assert_eq!(
            read(input, &mut context).unwrap().blocks,
            [Block::Para(expected)],
            "{input}"
        );
    }
}

#[test]
fn skips_hidden_footnotes_before_reading_the_body() {
    let mut context = Context::pure();
    let note = r"{\rtf1 A{\v {\footnote\ansicpg932\intbl\itap1000 secret}}B}";
    assert_eq!(read(note, &mut context).unwrap().blocks, [para("AB")]);
    assert!(context.log().is_empty());
    let visible = r"{\rtf1 {\b A{\footnote N}B}}";
    assert_eq!(
        read(visible, &mut context).unwrap().blocks,
        [Block::Para(vec![
            Inline::Strong(vec![text("A")]),
            Inline::Note(vec![Block::Para(vec![Inline::Strong(vec![text("N")])])]),
            Inline::Strong(vec![text("B")]),
        ])],
    );
}

#[test]
fn escapes_literal_tabs_and_aligns_the_error_caret() {
    let rtf = fragment(
        vec![
            Block::CodeBlock(Attr::default(), "A\tB".into()),
            para("C\tD"),
        ],
        &mut Context::pure(),
    );
    assert!(rtf.contains(r"\f1 \fs24 A\tab B\par"));
    assert!(rtf.contains(r"\fi0 C\tab D\par"));
    assert!(!rtf.contains('\t'));

    let error = read("{\\rtf1 \t\\'zz}", &mut Context::pure()).unwrap_err();
    assert_eq!(error.exit_code(), 64);
    assert_eq!(
        error.to_string(),
        "Error at (line 1, column 11):\nunexpected 'z'\n{\\rtf1  \\'zz}\n          ^"
    );
}
