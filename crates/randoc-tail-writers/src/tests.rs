#![allow(clippy::expect_used, clippy::panic)]

use randoc_archive::zip::{Archive, ArchiveLimits};
use randoc_ast::{Attr, Block, Inline, MetaValue, Pandoc, Target};
use randoc_core::{Context, Error, Reader, ReaderOptions, Sources, WriterOptions};

use super::{reader, writer};

fn write(
    format: &str,
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> randoc_core::Result<Vec<u8>> {
    writer(format)
        .expect("writer registration")
        .write(doc, options, context)
        .map(randoc_core::Output::into_bytes)
}

fn read_xml(text: &str, context: &mut Context) -> randoc_core::Result<Pandoc> {
    let Some(Reader::Text(reader)) = reader("xml") else {
        panic!("XML reader registration")
    };
    reader.read(
        &Sources::from_text(text),
        &ReaderOptions::default(),
        context,
    )
}

const TEXT_FORMATS: [&str; 11] = [
    "ansi",
    "bbcode",
    "bbcode_fluxbb",
    "bbcode_hubzilla",
    "bbcode_phpbb",
    "bbcode_steam",
    "bbcode_xenforo",
    "context",
    "icml",
    "texinfo",
    "xml",
];

fn deep_document(depth: usize) -> Pandoc {
    let mut inline = Inline::Str("test".into());
    for _ in 2..depth / 2 {
        inline = Inline::Emph(vec![inline]);
    }
    let mut block = Block::Para(vec![inline]);
    for _ in depth / 2..depth {
        block = Block::BlockQuote(vec![block]);
    }
    Pandoc {
        blocks: vec![block],
        ..Pandoc::default()
    }
}

#[test]
fn writers_render_at_depth_limit_on_two_mib_stacks() {
    for format in TEXT_FORMATS {
        std::thread::Builder::new()
            .name(format.into())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let mut context = Context::pure();
                let depth = context.limits.max_nesting_depth;
                let mut doc = deep_document(depth);
                let options = WriterOptions::default();
                let output =
                    write(format, &doc, &options, &mut context).expect("write at the limit");
                assert!(!output.is_empty());
                if format == "xml" {
                    let text = std::str::from_utf8(&output).expect("valid UTF-8");
                    let decoded = read_xml(text, &mut context).expect("read at the limit");
                    assert_eq!(decoded, doc);
                    context.limits.max_nesting_depth -= 1;
                    assert!(read_xml(text, &mut context).is_err());
                    context.limits.max_nesting_depth = depth;
                }
                doc.blocks = vec![Block::Div(Attr::default(), doc.blocks)];
                assert!(write(format, &doc, &options, &mut context).is_err());
            })
            .expect("test thread")
            .join()
            .expect("writer stack");
    }
}

#[test]
fn chunked_html_renders_at_depth_limit_on_two_mib_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut context = Context::pure();
            let depth = context.limits.max_nesting_depth;
            let mut doc = deep_document(depth);
            let options = WriterOptions::default();
            assert!(
                !write("chunkedhtml", &doc, &options, &mut context)
                    .expect("write at the limit")
                    .is_empty()
            );
            doc.blocks = vec![Block::Div(Attr::default(), doc.blocks)];
            assert!(write("chunkedhtml", &doc, &options, &mut context).is_err());
            context.limits.max_nesting_depth = 256;
            let error = write("chunkedhtml", &deep_document(256), &options, &mut context)
                .expect_err("shared chunking depth limit");
            assert!(error.to_string().contains("nested too deeply"));
        })
        .expect("test thread")
        .join()
        .expect("chunked HTML stack");
}

#[test]
fn xml_preserves_structured_metadata_tables_and_citations() {
    let mut context = Context::pure();
    let text = concat!(
        "<Pandoc><meta><entry key=\"title\"><MetaList><MetaString>Ω &amp; text</MetaString>",
        "<MetaBool value=\"true\" /></MetaList></entry></meta><blocks>",
        "<Para><Cite><citations><Citation id=\"key\"><prefix>see</prefix><suffix>3</suffix>",
        "</Citation></citations>ref</Cite><Space count=\"2\" /><SoftBreak /></Para>",
        "<Table><Caption><ShortCaption>Short</ShortCaption><Plain>Long</Plain></Caption>",
        "<colspecs><ColSpec alignment=\"AlignRight\" col-width=\"0.5\" /></colspecs>",
        "<TableHead /><TableBody row-head-columns=\"0\"><header /><body><Row><Cell><Plain>Cell</Plain>",
        "</Cell></Row></body></TableBody><TableFoot /></Table></blocks></Pandoc>"
    );
    let doc = read_xml(text, &mut context).expect("XML document");
    assert!(
        matches!(doc.meta.get("title"), Some(MetaValue::MetaList(values)) if values.len() == 2)
    );
    let mut options = WriterOptions::default();
    options.template = Some(super::default_template("bbcode", 128).expect("template marker"));
    let output = write("xml", &doc, &options, &mut context).expect("XML output");
    let text = std::str::from_utf8(&output).expect("valid UTF-8");
    assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert_eq!(read_xml(text, &mut context).expect("round trip"), doc);
}

#[test]
fn xml_rejects_unexpected_content_and_excessive_whitespace_counts() {
    let mut context = Context::pure();
    for source in [
        "<Unknown />",
        "<Para><Unknown /></Para>",
        "<Para>&nbsp;</Para>",
        "<Para><Space count=\"9223372036854775807\" /></Para>",
        "<Div>text</Div>",
    ] {
        assert!(
            matches!(read_xml(source, &mut context), Err(Error::Xml { .. })),
            "{source}"
        );
    }
}

#[test]
fn bbcode_dialects_use_their_own_heading_and_code_tags() {
    let doc = Pandoc {
        blocks: vec![Block::Header(
            1,
            Attr::default(),
            vec![Inline::Code(Attr::default(), "Example".into())],
        )],
        ..Pandoc::default()
    };
    for (format, prefix) in [
        ("bbcode", "[u][b]Example"),
        ("bbcode_fluxbb", "[u][b]Example"),
        ("bbcode_hubzilla", "[h1][code]Example"),
        ("bbcode_phpbb", "[u][b]Example"),
        ("bbcode_steam", "[h1][noparse]Example"),
        ("bbcode_xenforo", "[heading=1][icode]Example"),
    ] {
        let output = write(
            format,
            &doc,
            &WriterOptions::default(),
            &mut Context::pure(),
        )
        .expect("BBCode");
        assert!(output.starts_with(prefix.as_bytes()), "{format}");
    }
}

#[test]
fn texinfo_resolves_duplicate_node_names_in_document_order() {
    let title = |level, id| {
        Block::Header(
            level,
            Attr::new(id, vec![], vec![]),
            vec![Inline::Str("A".into())],
        )
    };
    let doc = Pandoc {
        blocks: vec![
            title(1, "a"),
            title(2, "b"),
            title(1, "c"),
            Block::Para(vec![Inline::Link(
                Attr::default(),
                vec![Inline::Str("first".into())],
                Target::new("#b", ""),
            )]),
        ],
        ..Pandoc::default()
    };
    let output = write(
        "texinfo",
        &doc,
        &WriterOptions::default(),
        &mut Context::pure(),
    )
    .expect("Texinfo");
    let output = String::from_utf8(output).expect("valid UTF-8");
    assert!(output.contains("@node A 2\n@section A"));
    assert!(output.contains("@node A 3\n@chapter A"));
    assert!(output.contains("@ref{A 2,,first}"));
}

#[test]
fn chunked_html_has_navigation_and_resolves_local_links() {
    let doc = Pandoc {
        blocks: vec![
            Block::Header(
                1,
                Attr::new("first", vec![], vec![]),
                vec![Inline::Str("First".into())],
            ),
            Block::Para(vec![Inline::Link(
                Attr::default(),
                vec![Inline::Str("Second".into())],
                Target::new("#second", ""),
            )]),
            Block::Header(
                1,
                Attr::new("second", vec![], vec![]),
                vec![Inline::Str("Second".into())],
            ),
        ],
        ..Pandoc::default()
    };
    let mut context = Context::pure();
    let mut options = WriterOptions::default();
    options.chunk_template = "chapters/%n-%i.html".into();
    options.identifier_prefix = "p-".into();
    let bytes = write("chunkedhtml", &doc, &options, &mut context).expect("ZIP output");
    let mut archive =
        Archive::open(&bytes, ArchiveLimits::from_limits(&context.limits)).expect("ZIP archive");
    assert_eq!(
        archive.entry_names(),
        [
            "sitemap.json",
            "index.html",
            "chapters001-first.html",
            "chapters002-second.html"
        ]
    );
    let data = archive
        .read("chapters001-first.html")
        .expect("read page")
        .expect("page");
    let html = String::from_utf8(data).expect("HTML");
    assert!(html.contains("chapters002-second.html#p-second"), "{html}");
    let map = archive
        .read("sitemap.json")
        .expect("read sitemap")
        .expect("sitemap");
    let parsed: serde_json::Value = serde_json::from_slice(&map).expect("sitemap JSON");
    assert_eq!(parsed["subsections"][1]["section"]["id"], "second");
    context.limits.max_archive_entries = 3;
    assert!(write("chunkedhtml", &doc, &options, &mut context).is_err());
}

#[test]
fn ansi_charges_table_padding_without_charging_existing_cells() {
    use randoc_ast::{Cell, ColSpec, Row, Table, TableBody};

    let cell = || Cell {
        content: vec![Block::Plain(vec![Inline::Str("data".into())])],
        ..Cell::default()
    };
    let table = |cells| {
        let table = Table {
            col_specs: vec![ColSpec::default(); 2],
            bodies: vec![TableBody {
                body: vec![Row {
                    attr: Attr::default(),
                    cells,
                }],
                ..TableBody::default()
            }],
            ..Table::default()
        };
        Pandoc {
            blocks: vec![Block::Table(Box::new(table))],
            ..Pandoc::default()
        }
    };
    let options = WriterOptions::default();
    let mut context = Context::pure();
    context.limits.max_table_cells = 0;
    assert!(write("ansi", &table(vec![cell(), cell()]), &options, &mut context).is_ok());
    let err =
        write("ansi", &table(vec![cell()]), &options, &mut context).expect_err("padding limit");
    assert!(err.to_string().contains("padding cells"));
}

#[test]
fn chunked_html_adds_resources_and_reports_missing_files() {
    let img = Block::Plain(vec![Inline::Image(
        Attr::default(),
        vec![],
        Target::new("photo.png", ""),
    )]);
    let doc = Pandoc {
        blocks: vec![img],
        ..Pandoc::default()
    };
    let mut context = Context::pure();
    let options = WriterOptions::default();
    assert!(matches!(
        write("chunkedhtml", &doc, &options, &mut context),
        Err(Error::ResourceNotFound(_))
    ));
    context.media.insert(
        "photo.png",
        randoc_core::MediaItem {
            mime_type: "image/png".into(),
            contents: vec![1, 2, 3],
        },
    );
    let bytes = write("chunkedhtml", &doc, &options, &mut context).expect("ZIP output");
    let mut archive =
        Archive::open(&bytes, ArchiveLimits::from_limits(&context.limits)).expect("ZIP archive");
    assert_eq!(
        archive.entry_names(),
        ["sitemap.json", "index.html", "0--1.html", "photo.png"]
    );
    assert_eq!(
        archive.read("photo.png").expect("read image"),
        Some(vec![1, 2, 3])
    );
    context.limits.max_archive_size = 256;
    assert!(write("chunkedhtml", &doc, &options, &mut context).is_err());
}

#[test]
fn xml_respects_a_limit_above_128_on_a_two_mib_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut context = Context::pure();
            context.limits.max_nesting_depth = 256;
            let options = WriterOptions::default();
            let doc = deep_document(256);
            let bytes = write("xml", &doc, &options, &mut context).expect("write at the limit");
            let input = std::str::from_utf8(&bytes).expect("valid UTF-8");
            let decoded = read_xml(input, &mut context).expect("read at the limit");
            assert_eq!(decoded, doc);
            assert!(read_xml(&format!("{input}<Unknown />"), &mut context).is_err());
            assert!(read_xml(&format!("<Div>{input}</Div>"), &mut context).is_err());
        })
        .expect("test thread")
        .join()
        .expect("XML reader and writer stack");
}

#[test]
fn chunked_html_uses_source_date_epoch_for_zip_entries() {
    let host = randoc_core::PureHost {
        files: Default::default(),
        time: randoc_core::Timestamp {
            unix_seconds: 0,
            nanoseconds: 0,
            utc_offset_minutes: 0,
        },
        environment: [("SOURCE_DATE_EPOCH".into(), "1700000000".into())].into(),
    };
    let mut context = Context::new(Box::new(host));
    let archive = write(
        "chunkedhtml",
        &Pandoc::default(),
        &WriterOptions::default(),
        &mut context,
    )
    .expect("ZIP output");
    assert!(archive.starts_with(b"PK\x03\x04"));
    let mut dos_time = [0; 2];
    let mut dos_date = [0; 2];
    dos_time.copy_from_slice(&archive[10..12]);
    dos_date.copy_from_slice(&archive[12..14]);
    assert_eq!(u16::from_le_bytes(dos_time), (22 << 11) | (13 << 5) | 10);
    assert_eq!(u16::from_le_bytes(dos_date), (43 << 9) | (11 << 5) | 14);
}

#[test]
fn chunked_html_splits_nested_sections_and_keeps_the_first_duplicate_path() {
    let heading = |level, id: &str, title: &str| {
        Block::Header(
            level,
            Attr::new(id, vec![], vec![]),
            vec![Inline::Str(title.into())],
        )
    };
    let doc = Pandoc {
        blocks: vec![
            heading(1, "first", "First"),
            heading(2, "nested", "Nested"),
            heading(1, "last", "Last"),
        ],
        ..Pandoc::default()
    };
    let mut context = Context::pure();
    let mut options = WriterOptions::default();
    options.split_level = 2;
    let bytes = write("chunkedhtml", &doc, &options, &mut context).expect("ZIP output");
    let archive =
        Archive::open(&bytes, ArchiveLimits::from_limits(&context.limits)).expect("ZIP archive");
    assert_eq!(
        archive.entry_names(),
        [
            "sitemap.json",
            "index.html",
            "1-first.html",
            "1.1-nested.html",
            "2-last.html"
        ]
    );

    options.chunk_template = "../same.html".into();
    let bytes = write("chunkedhtml", &doc, &options, &mut context).expect("ZIP output");
    let mut archive =
        Archive::open(&bytes, ArchiveLimits::from_limits(&context.limits)).expect("ZIP archive");
    assert_eq!(
        archive.entry_names(),
        ["sitemap.json", "index.html", "..same.html"]
    );
    let page = archive
        .read("..same.html")
        .expect("read page")
        .expect("page");
    let page = String::from_utf8(page).expect("valid UTF-8");
    assert!(page.contains(">First</h1>"));
}

#[test]
fn xml_writes_str_and_preserves_explicit_content() {
    let mut context = Context::pure();
    let input = concat!(
        "<Plain><Str content=\"a b\" />x<Str>y z</Str><Str /><Str content=\"a&#9;b\" />",
        "<Str content=\"c\"><Ignored /></Str></Plain>",
    );
    let doc = read_xml(input, &mut context).expect("read Str");
    assert_eq!(
        doc.blocks,
        vec![Block::Plain(
            ["a b", "x", "", "", "a\tb", "c"]
                .into_iter()
                .map(|text| Inline::Str(text.into()))
                .collect(),
        )],
    );
    let doc = read_xml(
        "<Plain><Str content=\"a b\" /><Str /><Str content=\" \" /></Plain>",
        &mut context,
    )
    .expect("read round-trip input");
    let bytes = write("xml", &doc, &WriterOptions::default(), &mut context).expect("write Str");
    let xml = std::str::from_utf8(&bytes).expect("UTF-8");
    assert_eq!(
        xml,
        "<Plain><Str content=\"a b\" /><Str /><Str content=\" \" /></Plain>"
    );
    assert_eq!(read_xml(xml, &mut context).expect("read output"), doc);
}

#[test]
fn xml_preserves_non_ascii_spaces_and_reports_context_errors() {
    let mut context = Context::pure();
    let doc = read_xml(
        "<Plain>a&#160;b&#8195;c a&#9;b&#13;c&#10;d</Plain>&#160;<Plain>&#160;</Plain>",
        &mut context,
    )
    .expect("read whitespace");
    assert_eq!(
        doc.blocks,
        vec![
            Block::Plain(vec![
                Inline::Str("a\u{a0}b\u{2003}c".into()),
                Inline::Space,
                Inline::Str("a".into()),
                Inline::Space,
                Inline::Str("b".into()),
                Inline::SoftBreak,
                Inline::Str("c".into()),
                Inline::SoftBreak,
                Inline::Str("d".into()),
            ]),
            Block::Plain(vec![Inline::Str("\u{a0}".into())]),
        ]
    );
    for (input, message) in [
        (
            "<Para>x<Foo/></Para>",
            "unexpected element \"Foo\" in inline context",
        ),
        ("<Foo/>", "unexpected element \"Foo\" in blocks context"),
        (
            "junk<Para>x</Para>",
            "non-space characters out of inline context",
        ),
    ] {
        let error = read_xml(input, &mut context).expect_err("invalid XML");
        assert_eq!(error.exit_code(), 44);
        assert_eq!(error.to_string(), format!("Invalid XML:\n{message}"));
    }
}

#[test]
fn xml_formats_whitespace_runs() {
    let a = || Inline::Str("a".into());
    let b = || Inline::Str("b".into());
    use Inline::{LineBreak, SoftBreak, Space};
    for (items, expected) in [
        (
            vec![SoftBreak, SoftBreak, SoftBreak],
            "<SoftBreak /><SoftBreak /><SoftBreak />",
        ),
        (
            vec![a(), Space, Space, SoftBreak, b()],
            "a<Space count=\"2\" />\nb",
        ),
        (
            vec![a(), SoftBreak, Space, Space, b()],
            "a\n<Space count=\"2\" />b",
        ),
        (vec![Space, a(), Space], "<Space />a<Space />"),
        (vec![SoftBreak], "\n"),
        (vec![a(), LineBreak, Space, b()], "a<LineBreak /> b"),
        (vec![a(), Space, SoftBreak, b()], "a<Space /><SoftBreak />b"),
        (vec![Space, SoftBreak, Space], "<Space />\n<Space />"),
        (
            vec![Inline::Emph(vec![Space, a(), Space])],
            "<Emph><Space />a<Space /></Emph>",
        ),
    ] {
        let doc = Pandoc {
            blocks: vec![Block::Plain(items)],
            ..Pandoc::default()
        };
        let actual =
            write("xml", &doc, &WriterOptions::default(), &mut Context::pure()).expect("write XML");
        assert_eq!(actual, format!("<Plain>{expected}</Plain>").into_bytes());
    }
}

#[test]
fn chunked_html_ignores_unsafe_image_targets() {
    let urls = [
        "/etc/hosts",
        "sub/../../nothere.gif",
        "http://127.0.0.1:9/x.png",
        "data:image/png;base64,iVBORw0KGgo=",
    ];
    let mut context = Context::pure();
    let doc = Pandoc {
        blocks: urls
            .iter()
            .map(|url| {
                Block::Plain(vec![Inline::Image(
                    Attr::default(),
                    vec![],
                    Target::new(*url, ""),
                )])
            })
            .collect(),
        ..Pandoc::default()
    };
    let bytes =
        write("chunkedhtml", &doc, &WriterOptions::default(), &mut context).expect("write ZIP");
    let mut archive =
        Archive::open(&bytes, ArchiveLimits::from_limits(&context.limits)).expect("open ZIP");
    assert_eq!(
        archive.entry_names(),
        ["sitemap.json", "index.html", "0--1.html"]
    );
    let html = String::from_utf8(archive.read("0--1.html").expect("read page").expect("page"))
        .expect("UTF-8");
    for url in urls {
        assert!(html.contains(&format!("src=\"{url}\"")), "{html}");
    }
}

#[test]
fn chunked_html_flattens_chunk_names_and_hashes_unsafe_names() {
    let doc = Pandoc {
        blocks: vec![
            Block::Plain(vec![Inline::Str("pre".into())]),
            Block::Header(
                1,
                Attr::new("a", vec![], vec![]),
                vec![Inline::Str("A".into())],
            ),
            Block::Header(
                1,
                Attr::new("b", vec![], vec![]),
                vec![Inline::Str("B".into())],
            ),
        ],
        ..Pandoc::default()
    };
    let mut context = Context::pure();
    for (template, expected) in [
        ("../%i.html", ["..-1.html", "..a.html", "..b.html"]),
        ("x/../%i.html", ["x..-1.html", "x..a.html", "x..b.html"]),
        ("/abs/%i.html", ["abs-1.html", "absa.html", "absb.html"]),
        (
            "d/%n-%s.html",
            ["d001-0.html", "d002-1.html", "d003-2.html"],
        ),
        ("a\\b%i.html", ["ab-1.html", "aba.html", "abb.html"]),
    ] {
        let mut options = WriterOptions::default();
        options.chunk_template = template.into();
        let zip = write("chunkedhtml", &doc, &options, &mut context).expect("write ZIP");
        let archive =
            Archive::open(&zip, ArchiveLimits::from_limits(&context.limits)).expect("open ZIP");
        assert_eq!(&archive.entry_names()[2..], expected);
    }
    for template in ["/", ".", ".."] {
        let mut options = WriterOptions::default();
        options.chunk_template = template.into();
        let zip = write("chunkedhtml", &doc, &options, &mut context).expect("write ZIP");
        let archive =
            Archive::open(&zip, ArchiveLimits::from_limits(&context.limits)).expect("open ZIP");
        let names = archive.entry_names();
        assert_eq!(names.len(), 3);
        assert_eq!(names[2].len(), 40);
        assert!(names[2].bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}

#[test]
fn chunked_html_checks_entry_and_cumulative_output_limits() {
    let mut context = Context::pure();
    let mut doc = Pandoc {
        blocks: vec![Block::Plain(vec![Inline::Image(
            Attr::default(),
            vec![],
            Target::new("missing.png", ""),
        )])],
        ..Pandoc::default()
    };
    let options = WriterOptions::default();
    context.limits.max_archive_entries = 3;
    let error =
        write("chunkedhtml", &doc, &options, &mut context).expect_err("entry limit before fetch");
    assert!(error.to_string().contains("too many entries"));
    doc.blocks = vec![Block::Plain(vec![Inline::Str("text".into())])];
    let bytes = write("chunkedhtml", &doc, &options, &mut context).expect("write ZIP");
    let mut archive =
        Archive::open(&bytes, ArchiveLimits::from_limits(&context.limits)).expect("open ZIP");
    let total: u64 = ["sitemap.json", "index.html", "0--1.html"]
        .into_iter()
        .map(|name| {
            archive
                .read(name)
                .expect("read entry")
                .expect("entry")
                .len() as u64
        })
        .sum();
    context.limits.max_input_size = total;
    assert!(write("chunkedhtml", &doc, &options, &mut context).is_ok());
    context.limits.max_input_size = total - 1;
    let error =
        write("chunkedhtml", &doc, &options, &mut context).expect_err("cumulative output limit");
    assert!(error.to_string().contains("output exceeds"));
    assert_eq!(context.limits.max_input_size, total - 1);
}

#[test]
fn text_writers_limit_high_amplification() {
    for (format, mut items) in [
        ("context", vec![Inline::Str("\\".repeat(130))]),
        ("texinfo", vec![Inline::Str("@".repeat(130))]),
        (
            "icml",
            (0..80)
                .map(|i| {
                    let text = vec![Inline::Str("x".into())];
                    if i % 2 == 0 {
                        Inline::Emph(text)
                    } else {
                        Inline::Strong(text)
                    }
                })
                .collect(),
        ),
        (
            "bbcode",
            vec![Inline::Emph(vec![Inline::Str("x".into())]); 80],
        ),
    ] {
        if format == "icml" {
            items.push(Inline::Image(
                Attr::default(),
                vec![],
                Target::new("missing-after-limit.png", ""),
            ));
        }
        let doc = Pandoc {
            blocks: vec![Block::Para(items)],
            ..Pandoc::default()
        };
        let mut context = Context::pure();
        context.limits.max_input_size = 256;
        let error = write(format, &doc, &WriterOptions::default(), &mut context).expect_err(format);
        assert!(
            error.to_string().contains("output exceeds"),
            "{format}: {error}"
        );
    }
}

#[test]
fn xml_drops_partial_trees_on_a_two_mib_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut context = Context::pure();
            context.limits.max_nesting_depth = 256;
            let chain = |depth: usize| {
                format!(
                    "{}<Plain>x</Plain>{}",
                    "<Div>".repeat(depth),
                    "</Div>".repeat(depth)
                )
            };
            let input = format!("{}{}", chain(255), chain(256));
            let error = read_xml(&input, &mut context).expect_err("AST depth limit");
            assert!(error.to_string().contains("nested too deeply"));
            let input = format!("{}{}", chain(255), "<Div>".repeat(2048));
            let error = read_xml(&input, &mut context).expect_err("parser depth limit");
            assert!(matches!(error, Error::Xml { .. }));
        })
        .expect("XML test thread")
        .join()
        .expect("XML stack");
}

#[test]
fn writers_render_mixed_trees_on_two_mib_stacks() {
    use randoc_ast::{Caption, Cell, ColSpec, Row, Table, TableBody};

    for format in TEXT_FORMATS.into_iter().chain(["chunkedhtml"]) {
        std::thread::Builder::new()
            .name(format.into())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let mut context = Context::pure();
                let depth = context.limits.max_nesting_depth;
                let mut doc = deep_document(depth - 5);
                doc.blocks = vec![Block::Figure(
                    Attr::default(),
                    Caption::default(),
                    vec![Block::Table(Box::new(Table {
                        col_specs: vec![ColSpec::default()],
                        bodies: vec![TableBody {
                            body: vec![Row {
                                cells: vec![Cell {
                                    content: vec![Block::BulletList(vec![vec![Block::Para(
                                        vec![Inline::Str("note".into()), Inline::Note(doc.blocks)],
                                    )]])],
                                    ..Cell::default()
                                }],
                                ..Row::default()
                            }],
                            ..TableBody::default()
                        }],
                        ..Table::default()
                    }))],
                )];
                let mut metadata = MetaValue::MetaString("title".into());
                for _ in 1..depth {
                    metadata = MetaValue::MetaList(vec![metadata]);
                }
                doc.meta.0.insert("nested".into(), metadata);
                let mut options = WriterOptions::default();
                if format == "xml" {
                    options.template =
                        Some(super::default_template("bbcode", depth).expect("template"));
                }
                let bytes = write(format, &doc, &options, &mut context).expect("write mixed tree");
                if format == "xml" {
                    let xml = std::str::from_utf8(&bytes).expect("XML");
                    assert_eq!(read_xml(xml, &mut context).expect("read mixed tree"), doc);
                }
            })
            .expect("test thread")
            .join()
            .expect("writer stack");
    }
}

#[test]
fn text_writers_render_depth_256_on_two_mib_stacks() {
    for format in TEXT_FORMATS {
        std::thread::Builder::new()
            .name(format.into())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let mut context = Context::pure();
                context.limits.max_nesting_depth = 256;
                let doc = deep_document(256);
                write(format, &doc, &WriterOptions::default(), &mut context)
                    .expect("write at depth 256");
            })
            .expect("test thread")
            .join()
            .expect("writer stack");
    }
}
