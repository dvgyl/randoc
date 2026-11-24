//! Runs EPUB comparisons against the Pandoc 3.13 executable.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use randoc_archive::xml::{self, XmlElement, XmlLimits, XmlNode, XmlOptions};
use randoc_archive::zip::{Archive, ArchiveLimits, ArchiveWriter};
use randoc_ast::{Attr, Block, Inline, MathType, MetaValue, Pandoc, Target};
use randoc_core::{Context, FormatSpec, PureHost, Reader, ReaderOptions, WriterOptions};

fn oracle(args: &[&str], stdin: &[u8]) -> Vec<u8> {
    let mut child = Command::new(std::env::var_os("RANDOC_ORACLE_PANDOC").unwrap())
        .args(args)
        .env("SOURCE_DATE_EPOCH", "1700000000")
        .env("LC_ALL", "de_DE.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    std::thread::scope(|scope| {
        let writer = scope.spawn(move || input.write_all(stdin));
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        writer.join().unwrap().unwrap();
        output.stdout
    })
}

fn read_epub(bytes: &[u8]) -> Pandoc {
    let Some(Reader::Binary(reader)) = randoc_epub::reader("epub") else {
        panic!("EPUB reader missing");
    };
    let mut options = ReaderOptions::default();
    options.extensions = FormatSpec::parse("epub").extensions().unwrap();
    reader.read(bytes, &options, &mut Context::pure()).unwrap()
}

fn compare(label: &str, actual: &Pandoc, expected: &Pandoc) -> bool {
    let mut pass = true;
    if actual.meta != expected.meta {
        for key in actual
            .meta
            .0
            .keys()
            .chain(expected.meta.0.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            if actual.meta.get(key) != expected.meta.get(key) {
                eprintln!(
                    "{label} metadata {key}: actual {:.500?}, expected {:.500?}",
                    actual.meta.get(key),
                    expected.meta.get(key)
                );
            }
        }
        pass = false;
    }
    if actual.blocks != expected.blocks {
        eprintln!(
            "{label} block count: actual {}, expected {}",
            actual.blocks.len(),
            expected.blocks.len()
        );
        for (index, (a, b)) in actual.blocks.iter().zip(&expected.blocks).enumerate() {
            if a != b {
                let (a, b) = (format!("{a:?}"), format!("{b:?}"));
                let pos = a.bytes().zip(b.bytes()).take_while(|(a, b)| a == b).count();
                let display = |s: &str| {
                    String::from_utf8_lossy(
                        &s.as_bytes()[pos.saturating_sub(90)..s.len().min(pos + 260)],
                    )
                    .into_owned()
                };
                eprintln!(
                    "{label} block {index}: actual {}, expected {}",
                    display(&a),
                    display(&b)
                );
                break;
            }
        }
        pass = false;
    }
    pass
}

fn read_oracle(bytes: &[u8]) -> Pandoc {
    let json = oracle(&["-f", "epub", "-t", "json"], bytes);
    randoc_ast::json::from_json_with_limit(std::str::from_utf8(&json).unwrap(), 1024).unwrap()
}

#[test]
#[ignore = "requires Pandoc 3.13"]
fn chapter_links_and_ids_match_the_oracle() {
    let container = r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#;
    let package = r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata/><manifest><item id="first" href="first.xhtml" media-type="application/xhtml+xml"/><item id="second" href="text/chapter one.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="first"/><itemref idref="second"/></spine></package>"#;
    let first = r##"<p><a href="text/chapter one.xhtml#sec">Section</a> <a href="chapter%20one.xhtml">Chapter</a> <a href="text/chapter%20one.xhtml#sec">Encoded</a></p>"##;
    let second = r##"<h1 id="sec">Section</h1><p><a href="#sec">Local</a></p><table id="tbl"><thead id="head"><tr id="row"><th id="cell">Data</th></tr></thead></table><figure id="fig1"><img src="data:image/png;base64,AQ=="/><figcaption>Figure</figcaption></figure>"##;
    for href in ["chapter one.xhtml", "chapter%20one.xhtml"] {
        let package = package.replace("chapter one.xhtml", href);
        let mut zip = ArchiveWriter::new();
        for (path, text) in [
            ("META-INF/container.xml", container),
            ("content.opf", package.as_str()),
            ("first.xhtml", first),
            ("text/chapter one.xhtml", second),
        ] {
            zip.write_stored(path, text.as_bytes()).unwrap();
        }
        let bytes = zip.finish().unwrap();
        assert!(compare(href, &read_epub(&bytes), &read_oracle(&bytes)));
    }
    let input = br#"<h1>First</h1><table id="tbl"><tr><td>Data</td></tr></table><figure id="fig1"><img src="data:image/png;base64,AQ=="/><figcaption>Figure</figcaption></figure>"#;
    let bytes = oracle(&["-f", "html", "-t", "epub3"], input);
    assert!(compare(
        "Pandoc EPUB",
        &read_epub(&bytes),
        &read_oracle(&bytes)
    ));
}

fn check_files(directory: &Path, names: &[&str]) {
    let mut exact = 0;
    let mut checked = 0;
    for name in names {
        let bytes = std::fs::read(directory.join(format!("{name}.epub"))).unwrap();
        let mut actual = read_epub(&bytes);
        let mut expected = read_oracle(&bytes);
        if actual == expected {
            exact += 1;
            checked += 1;
        } else if *name == "features" {
            let actual_math = math_text(&mut actual);
            let expected_math = math_text(&mut expected);
            assert_eq!(actual_math.len(), 12);
            assert_eq!(expected_math.len(), 12);
            let mut tex_differences = 0;
            let mut fallback_differences = 0;
            for (actual, expected) in actual_math.iter().zip(&expected_math) {
                match expected {
                    MathText::Tex(text) => {
                        tex_differences += usize::from(actual != &MathText::Tex(text.clone()));
                    }
                    MathText::Fallback => {
                        assert_eq!(actual, &MathText::RawLongDivision);
                        fallback_differences += 1;
                    }
                    MathText::RawLongDivision => panic!("unexpected raw MathML from Pandoc"),
                }
            }
            assert_eq!(tex_differences, 0);
            assert_eq!(fallback_differences, 1);
            eprintln!(
                "{name}: {tex_differences} TeX text differences and {fallback_differences} MathML fallback difference"
            );
            checked += usize::from(compare(name, &actual, &expected));
        } else {
            compare(name, &actual, &expected);
        }
    }
    eprintln!("EPUB reader exact AST parity: {exact}/{}", names.len());
    assert_eq!(checked, names.len());
}

#[derive(Debug, PartialEq)]
enum MathText {
    Tex(String),
    Fallback,
    RawLongDivision,
}

fn math_text(doc: &mut Pandoc) -> Vec<MathText> {
    let mut texts = Vec::new();
    randoc_ast::walk::walk_inlines_mut(doc, |inline| match inline {
        Inline::Math(_, text) => texts.push(MathText::Tex(std::mem::take(text))),
        Inline::Span(attr, _)
            if attr.classes == ["math"]
                && attr.get("xmlns") == Some("http://www.w3.org/1998/Math/MathML") =>
        {
            texts.push(MathText::Fallback);
            *inline = Inline::Math(MathType::InlineMath, String::new());
        }
        Inline::RawInline(format, text)
            if format.as_str() == "html"
                && text.starts_with("<math")
                && text.contains("<mlongdiv") =>
        {
            texts.push(MathText::RawLongDivision);
            *inline = Inline::Math(MathType::InlineMath, String::new());
        }
        _ => {}
    });
    texts
}

#[test]
#[ignore = "requires Pandoc 3.13 and its EPUB fixtures"]
fn old_epub() {
    let root = std::env::var_os("RANDOC_PANDOC_SRC").unwrap();
    check_files(
        &Path::new(&root).join("test/epub"),
        &["features", "formatting", "wasteland"],
    );
}

#[test]
#[ignore = "requires Pandoc 3.13 and the EPUB corpus"]
fn epub_corpus() {
    let root = std::env::var_os("RANDOC_CORPUS").unwrap();
    check_files(
        Path::new(&root),
        &[
            "tika-testepub-epub",
            "tika-testepub-multi-metadata-vals-epub",
            "se-pride-and-prejudice",
            "se-frankenstein",
            "se-the-time-machine",
            "se-the-strange-case-of-dr-jekyll-and-mr-hyde",
            "se-the-importance-of-being-earnest",
        ],
    );
}

fn write_epub(
    format: &str,
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Vec<u8> {
    randoc_epub::writer(format)
        .unwrap()
        .write(doc, options, context)
        .unwrap()
        .into_bytes()
}

fn json(doc: &Pandoc) -> Vec<u8> {
    randoc_ast::json::to_json(doc).into_bytes()
}

fn xml_root(source: &str) -> XmlElement {
    let mut root = xml::parse(source, &XmlLimits::default(), XmlOptions::default())
        .unwrap()
        .root;
    fn normalize(element: &mut XmlElement) {
        element
            .attrs
            .sort_by(|(a, _), (b, _)| a.local.cmp(&b.local));
        element.ns_decls.sort();
        element
            .children
            .retain(|node| !matches!(node, XmlNode::Text(text) if text.trim().is_empty()));
        for node in &mut element.children {
            if let XmlNode::Element(child) = node {
                normalize(child);
            }
        }
    }
    normalize(&mut root);
    root
}

#[test]
#[ignore = "requires Pandoc 3.13"]
fn epub_writer_structure() {
    let mut doc = Pandoc::default();
    for (key, value) in [
        ("title", "Example"),
        ("identifier", "example-book"),
        ("lang", "en-US"),
    ] {
        doc.meta.insert(key, MetaValue::MetaString(value.into()));
    }
    doc.blocks = vec![
        Block::Header(
            1,
            Attr::new("1st", Vec::new(), Vec::new()),
            vec![Inline::Str("First".into())],
        ),
        Block::Para(vec![
            Inline::Math(MathType::InlineMath, "x^2".into()),
            Inline::Note(vec![Block::Para(vec![Inline::Str("Note".into())])]),
        ]),
        Block::Header(
            2,
            Attr::new("sub", Vec::new(), Vec::new()),
            vec![Inline::Str("Sub".into())],
        ),
        Block::Header(
            1,
            Attr::new("second", Vec::new(), Vec::new()),
            vec![Inline::Str("Second".into())],
        ),
        Block::Para(vec![Inline::Link(
            Attr::default(),
            vec![Inline::Str("First".into())],
            Target {
                url: "#1st".into(),
                title: String::new(),
            },
        )]),
    ];
    let mut options = WriterOptions::default();
    options.table_of_contents = true;
    options.number_sections = true;
    options.number_offset = vec![2, 1];
    for format in ["epub2", "epub3"] {
        let mut host = PureHost::default();
        host.environment
            .insert("SOURCE_DATE_EPOCH".into(), "1700000000".into());
        let actual = write_epub(format, &doc, &options, &mut Context::new(Box::new(host)));
        let expected = oracle(
            &[
                "-f",
                "json",
                "-t",
                format,
                "--toc",
                "--number-sections",
                "--number-offset=2,1",
                "--metadata=identifier:example-book",
            ],
            &json(&doc),
        );
        let mut actual_zip = Archive::open(&actual, ArchiveLimits::default()).unwrap();
        let mut expected_zip = Archive::open(&expected, ArchiveLimits::default()).unwrap();
        assert_eq!(
            actual_zip.entry_names(),
            expected_zip.entry_names(),
            "{format}"
        );
        for member in [
            "mimetype",
            "EPUB/content.opf",
            "EPUB/nav.xhtml",
            "EPUB/toc.ncx",
            "EPUB/text/ch001.xhtml",
            "EPUB/text/ch002.xhtml",
        ] {
            let left = String::from_utf8(actual_zip.read(member).unwrap().unwrap()).unwrap();
            let right = String::from_utf8(expected_zip.read(member).unwrap().unwrap()).unwrap();
            if std::env::var_os("RANDOC_EPUB_TRACE").is_some() {
                eprintln!("{format} {member} Randoc:\n{left}\nPandoc:\n{right}");
            }
            if member.starts_with("EPUB/text/") {
                assert_eq!(xml_root(&left), xml_root(&right), "{format} {member}");
            } else {
                assert_eq!(left, right, "{format} {member}");
            }
            if member.ends_with("ch001.xhtml") {
                assert!(left.contains("<math ") && right.contains("<math "));
                assert!(left.contains("href=\"#fn1\"") && right.contains("href=\"#fn1\""));
            }
            if member.ends_with("ch002.xhtml") {
                assert!(
                    left.contains("ch001.xhtml#id_1st") && right.contains("ch001.xhtml#id_1st")
                );
            }
        }
        let actual = read_epub(&actual);
        let expected = read_epub(&expected);
        assert!(
            compare(format, &actual, &expected),
            "{format} reader round-trip"
        );
        eprintln!(
            "{format}: ZIP member order, MathML, footnote link, cross-chapter link, and identifier match"
        );
    }
}

#[test]
#[ignore = "requires Pandoc 3.13"]
fn epub_writer_merges_package_metadata() {
    let metadata = concat!(
        "<?xml version=\"1.0\"?>\n",
        "<dc:identifier opf:scheme=\"URN\">extra-id</dc:identifier>",
        "<dc:title opf:file-as=\"Sort\" opf:type=\"main\">Extra title</dc:title>",
        "<dc:creator opf:file-as=\"Ed, E\" opf:role=\"edt\">E Ed</dc:creator>",
        "<dc:contributor opf:role=\"ill\">Extra contributor</dc:contributor>",
        "<dc:subject>Extra subject</dc:subject>",
        "<dc:rights>Extra rights</dc:rights>",
        "<meta name=\"unknown\" content=\"ignored\" />",
    );
    let path = Path::new(&std::env::var_os("TMPDIR").unwrap())
        .join(format!("randoc-epub-metadata-{}.xml", std::process::id()));
    std::fs::write(&path, metadata).unwrap();
    let meta_arg = format!("--epub-metadata={}", path.display());
    let mut doc = Pandoc::default();
    doc.blocks.push(Block::Header(
        1,
        Attr::new("one", Vec::new(), Vec::new()),
        vec![Inline::Str("One".into())],
    ));
    for (key, value) in [
        ("title", "Book & \"Writer's notes\""),
        ("identifier", "example-book"),
        ("date", "January 5, 2020"),
        ("author", "Author"),
        ("creator", "Creator"),
        ("contributor", "Contributor"),
        ("subject", "Subject"),
        ("description", "Description"),
        ("type", "Type"),
        ("format", "Format"),
        ("publisher", "Publisher"),
        ("source", "Source"),
        ("relation", "Relation"),
        ("coverage", "Coverage"),
        ("rights", "Rights"),
        ("page-progression-direction", "rtl"),
    ] {
        doc.meta.insert(key, MetaValue::MetaString(value.into()));
    }
    let mut options = WriterOptions::default();
    for use_extra in [false, true] {
        options.epub_metadata = use_extra.then(|| metadata.into());
        for format in ["epub2", "epub3"] {
            let mut host = PureHost::default();
            for (name, value) in [
                ("SOURCE_DATE_EPOCH", "1700000000"),
                ("LANG", "en_US.UTF-8"),
                ("LC_ALL", "de_DE.UTF-8"),
            ] {
                host.environment.insert(name.into(), value.into());
            }
            let actual = write_epub(format, &doc, &options, &mut Context::new(Box::new(host)));
            let mut args = vec![
                "-f",
                "json",
                "-t",
                format,
                "--metadata=identifier:example-book",
            ];
            if use_extra {
                args.push(&meta_arg);
            }
            let expected = oracle(&args, &json(&doc));
            for member in ["EPUB/content.opf", "EPUB/toc.ncx", "EPUB/nav.xhtml"] {
                assert_eq!(
                    member_text(&actual, member),
                    member_text(&expected, member),
                    "{format} {member} with external metadata = {use_extra}"
                );
            }
        }
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires Pandoc 3.13"]
fn epub_writer_orders_cover_media_and_duplicate_fonts() {
    let root = Path::new(&std::env::var_os("TMPDIR").unwrap())
        .join(format!("randoc-epub-resources-{}", std::process::id()));
    let first_font_dir = root.join("first");
    let last_font_dir = root.join("last");
    for dir in [&first_font_dir, &last_font_dir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let names = [
        root.join("first.png"),
        root.join("second.png"),
        root.join("cover.png"),
        first_font_dir.join("same.ttf"),
        last_font_dir.join("same.ttf"),
    ];
    let contents: [&[u8]; 5] = [
        b"first image",
        b"second image",
        b"cover",
        b"first font",
        b"last font",
    ];
    let mut host = PureHost::default();
    host.environment
        .insert("SOURCE_DATE_EPOCH".into(), "1700000000".into());
    host.environment.insert("LANG".into(), "en_US.UTF-8".into());
    for (path, bytes) in names.iter().zip(contents) {
        std::fs::write(path, bytes).unwrap();
        host.files
            .insert(path.display().to_string(), bytes.to_vec());
    }
    let mut context = Context::new(Box::new(host));
    let mut doc = Pandoc::default();
    doc.meta
        .insert("identifier", MetaValue::MetaString("example-book".into()));
    doc.meta
        .insert("author", MetaValue::MetaString("Author".into()));
    doc.blocks = vec![
        Block::Header(
            1,
            Attr::new("one", Vec::new(), Vec::new()),
            vec![Inline::Str("One".into())],
        ),
        Block::Para(
            names[..2]
                .iter()
                .map(|path| {
                    Inline::Image(
                        Attr::default(),
                        Vec::new(),
                        Target::new(path.display().to_string(), ""),
                    )
                })
                .collect(),
        ),
    ];
    let mut options = WriterOptions::default();
    options.variables.insert(
        "epub-cover-image",
        randoc_templates::Val::text(names[2].display().to_string()),
    );
    options.epub_fonts = names[3..]
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    let cover = format!("--epub-cover-image={}", names[2].display());
    let first_font = format!("--epub-embed-font={}", names[3].display());
    let last_font = format!("--epub-embed-font={}", names[4].display());
    for format in ["epub2", "epub3"] {
        let actual = write_epub(format, &doc, &options, &mut context);
        let expected = oracle(
            &[
                "-f",
                "json",
                "-t",
                format,
                "--metadata=identifier:example-book",
                &cover,
                &first_font,
                &last_font,
            ],
            &json(&doc),
        );
        let mut actual_zip = Archive::open(&actual, ArchiveLimits::default()).unwrap();
        let mut expected_zip = Archive::open(&expected, ArchiveLimits::default()).unwrap();
        assert_eq!(
            actual_zip.entry_names(),
            expected_zip.entry_names(),
            "{format}"
        );
        for member in ["EPUB/content.opf", "EPUB/toc.ncx", "EPUB/nav.xhtml"] {
            assert_eq!(
                member_text(&actual, member),
                member_text(&expected, member),
                "{format} {member}"
            );
        }
        assert_eq!(
            actual_zip.read("EPUB/fonts/same.ttf").unwrap().unwrap(),
            expected_zip.read("EPUB/fonts/same.ttf").unwrap().unwrap(),
            "{format}"
        );
        for bytes in [&actual, &expected] {
            assert!(
                member_text(bytes, "EPUB/text/cover.xhtml").contains("<title>UNTITLED</title>")
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

fn member_text(epub: &[u8], name: &str) -> String {
    let mut zip = Archive::open(epub, ArchiveLimits::default()).unwrap();
    String::from_utf8(zip.read(name).unwrap().unwrap()).unwrap()
}

#[test]
#[ignore = "requires Pandoc 3.13"]
fn epub_commands() {
    for (id, raw_format) in [
        ("8880.md#1", "epub"),
        ("8880.md#2", "epub2"),
        ("8880.md#3", "epub3"),
    ] {
        let mut doc = Pandoc::default();
        doc.meta
            .insert("title", MetaValue::MetaString("Raw".into()));
        doc.blocks.push(Block::RawBlock(
            randoc_ast::Format::new(raw_format),
            "<p>ok</p>".into(),
        ));
        let options = WriterOptions::default();
        let actual = write_epub("epub3", &doc, &options, &mut Context::pure());
        let expected = oracle(&["-f", "json", "-t", "epub3"], &json(&doc));
        let has_content = |bytes: &[u8]| {
            let mut zip = Archive::open(bytes, ArchiveLimits::default()).unwrap();
            let chapter = zip.read("EPUB/text/ch001.xhtml").unwrap().unwrap();
            String::from_utf8(chapter).unwrap().contains("<p>ok</p>")
        };
        assert_eq!(has_content(&actual), has_content(&expected), "{id}");
    }
    let mut doc = Pandoc::default();
    doc.meta
        .insert("title", MetaValue::MetaString("Foo".into()));
    doc.blocks.push(Block::Header(
        1,
        Attr::new("hi", Vec::new(), Vec::new()),
        vec![Inline::Str("Hi".into())],
    ));
    let mut options = WriterOptions::default();
    options.number_sections = true;
    options.table_of_contents = true;
    let actual = write_epub("epub", &doc, &options, &mut Context::pure());
    let expected = oracle(
        &["-f", "json", "-t", "epub", "--number-sections", "--toc"],
        &json(&doc),
    );
    let chapter = |bytes: &[u8]| {
        let mut zip = Archive::open(bytes, ArchiveLimits::default()).unwrap();
        String::from_utf8(zip.read("EPUB/text/ch001.xhtml").unwrap().unwrap()).unwrap()
    };
    for bytes in [&actual, &expected] {
        assert!(chapter(bytes).contains("header-section-number\">1</span>"));
    }
    eprintln!("EPUB command cases: 4/4");
}

fn oracle_in(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(std::env::var_os("RANDOC_ORACLE_PANDOC").unwrap())
        .current_dir(dir)
        .args(args)
        .env("SOURCE_DATE_EPOCH", "1700000000")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
#[ignore = "requires Pandoc 3.13"]
fn nav_title_uses_the_first_source_name() {
    let dir = std::env::temp_dir().join(format!("randoc-epub-nav-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("chapters")).unwrap();
    let first = "chapters/Field notes.v2.md";
    std::fs::write(dir.join(first), "# First\n").unwrap();
    std::fs::write(dir.join("second.md"), "# Second\n").unwrap();
    for format in ["epub2", "epub3"] {
        let expected = oracle_in(&dir, &[first, "second.md", "-t", format]);
        assert!(expected.status.success());
        let mut context = Context::pure();
        context.state.input_files = vec![first.into(), "second.md".into()];
        let actual = write_epub(
            format,
            &Pandoc::default(),
            &WriterOptions::default(),
            &mut context,
        );
        for epub in [&actual, &expected.stdout] {
            assert!(member_text(epub, "EPUB/nav.xhtml").contains("<title>Field notes.v2</title>"));
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
#[ignore = "requires Pandoc 3.13"]
fn epub_writer_entries_match_with_a_shared_css_file() {
    let dir = std::env::temp_dir().join(format!("randoc-epub-css-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let css = b"p { color: red; }\n".to_vec();
    std::fs::write(dir.join("style.css"), &css).unwrap();
    let source = "---\ntitle: Book\nsubtitle: Sub\nauthor: [Ann, Bob]\nlang: en-US\nrights: CC0\nidentifier: urn:uuid:7f6b2c1e-3a5d-4e8f-9b0a-1c2d3e4f5a6b\n---\n\n# One\n\nText with a [link](#two) and a note.[^n]\n\n## One sub\n\n# Two {#two}\n\nFinal.\n\n[^n]: Note.\n";
    std::fs::write(dir.join("book.md"), source).unwrap();
    let json = oracle_in(&dir, &["book.md", "-t", "json"]).stdout;
    let doc = randoc_ast::json::from_json(std::str::from_utf8(&json).unwrap()).unwrap();
    let mut differences = Vec::new();
    for format in ["epub2", "epub3"] {
        let output = oracle_in(
            &dir,
            &[
                "book.md",
                "-t",
                format,
                "--toc",
                "-N",
                "--css=style.css",
                "-o",
                "-",
            ],
        );
        assert!(output.status.success());
        let mut options = WriterOptions::default();
        options.table_of_contents = true;
        options.number_sections = true;
        options
            .variables
            .insert("css", randoc_templates::Val::List(vec!["style.css".into()]));
        let mut host = PureHost::default();
        host.files.insert("style.css".into(), css.clone());
        host.environment
            .insert("SOURCE_DATE_EPOCH".into(), "1700000000".into());
        let actual = write_epub(format, &doc, &options, &mut Context::new(Box::new(host)));
        let mut a = Archive::open(&actual, ArchiveLimits::default()).unwrap();
        let mut b = Archive::open(&output.stdout, ArchiveLimits::default()).unwrap();
        assert_eq!(a.entry_names(), b.entry_names(), "{format}");
        let names: Vec<String> = b.entry_names().into_iter().map(str::to_owned).collect();
        for name in names {
            if a.read(&name).unwrap() != b.read(&name).unwrap() {
                differences.push(format!("{format}: {name}"));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(differences.is_empty(), "{differences:?}");
}

#[test]
#[ignore = "requires Pandoc 3.13"]
fn missing_stylesheets_fail_as_in_pandoc() {
    let dir = std::env::temp_dir().join(format!("randoc-epub-nocss-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/real.css"), "p {}").unwrap();
    std::fs::write(dir.join("in.md"), "hi").unwrap();
    for (args, path, sandbox) in [
        (&["--css=a.css"][..], "a.css", false),
        (&["-V", "css=a.css"], "a.css", false),
        (
            &["--css=real.css", "--resource-path=sub"],
            "real.css",
            false,
        ),
        (&["--css=data:text/css,p{}"], "data:text/css,p{}", false),
        (&["--css=sub/real.css", "--sandbox"], "sub/real.css", true),
    ] {
        let mut all = vec!["in.md", "-t", "epub", "-o", "out.epub"];
        all.extend(args);
        let output = oracle_in(&dir, &all);
        let expected = if sandbox {
            format!("File '{path}' not found in resource path\n")
        } else {
            format!("{path}: openBinaryFile: does not exist (No such file or directory)\n")
        };
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            expected,
            "{args:?}"
        );
        assert_eq!(
            output.status.code(),
            Some(if sandbox { 99 } else { 1 }),
            "{args:?}"
        );
        // The unit test `reads_stylesheets_as_plain_files_as_pandoc_does`
        // covers the sandbox with a sandboxed host.
        if sandbox {
            continue;
        }
        let mut options = WriterOptions::default();
        options
            .variables
            .insert("css", randoc_templates::Val::List(vec![path.into()]));
        let mut host = PureHost::default();
        host.files.insert("sub/real.css".into(), b"p {}".to_vec());
        let mut context = Context::new(Box::new(host));
        context.state.resource_path = vec!["sub".into()];
        let error = randoc_epub::writer("epub3")
            .unwrap()
            .write(&Pandoc::default(), &options, &mut context)
            .unwrap_err();
        assert_eq!(format!("{error}\n"), expected, "{args:?}");
        assert_eq!(error.exit_code(), 1);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
