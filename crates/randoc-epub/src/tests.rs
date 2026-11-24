#![allow(clippy::unwrap_used, clippy::panic)]

use randoc_archive::zip::{Archive, ArchiveLimits, ArchiveWriter};
use randoc_ast::{Attr, Block, Inline, Meta, MetaValue, Pandoc, Target};
use randoc_core::{Context, Error, FormatSpec, MediaItem, PureHost, ReaderOptions, WriterOptions};

const CONTAINER: &str = r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OPS/content.opf"/></rootfiles></container>"#;
const OPF: &str = r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>Book</dc:title><dc:creator>A</dc:creator><dc:creator>B</dc:creator></metadata><manifest><item id="one" href="text/one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/><item id="cover" href="images/cover.png" media-type="image/png" properties="cover-image"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>"#;

fn fixture(package: &str, one: &str, two: &str) -> Vec<u8> {
    let mut zip = ArchiveWriter::new();
    for (name, bytes) in [
        ("META-INF/container.xml", CONTAINER.as_bytes()),
        ("OPS/content.opf", package.as_bytes()),
        ("OPS/text/one.xhtml", one.as_bytes()),
        ("OPS/two.xhtml", two.as_bytes()),
        ("OPS/images/cover.png", b"PNG".as_slice()),
        ("top.png", b"top image".as_slice()),
    ] {
        zip.write_stored(name, bytes).unwrap();
    }
    zip.finish().unwrap()
}

fn read(bytes: &[u8], context: &mut Context) -> randoc_core::Result<Pandoc> {
    let Some(randoc_core::Reader::Binary(reader)) = super::reader("epub") else {
        panic!("EPUB reader missing");
    };
    let mut options = ReaderOptions::default();
    options.extensions = FormatSpec::parse("epub").extensions().unwrap();
    reader.read(bytes, &options, context)
}

#[test]
fn repeated_spine_chapters_consume_the_archive_budget() {
    let package = OPF.replace(r#"<itemref idref="two"/>"#, r#"<itemref idref="one"/>"#);
    let chapter = format!("<p>{}</p>", "Page text. ".repeat(240));
    let bytes = fixture(&package, &chapter, "");
    assert!(read(&bytes, &mut Context::pure()).is_ok());

    let mut context = Context::pure();
    context.limits.max_archive_size = (CONTAINER.len() + package.len() + chapter.len() + 3) as u64;
    let error = read(&bytes, &mut context).unwrap_err().to_string();
    assert!(error.contains("decompressed archive size"), "{error}");
}

#[test]
fn reads_spine_metadata_links_and_cover() {
    let bytes = fixture(
        OPF,
        r##"<p id="local"><a href="#local">one</a> <a href="../two.xhtml#other">two</a> <img src="../images/cover.png" alt="image" /></p>"##,
        r#"<h1 id="other">Two</h1>"#,
    );
    let mut context = Context::pure();
    let doc = read(&bytes, &mut context).unwrap();
    assert_eq!(
        doc.meta.get("author"),
        Some(&MetaValue::MetaList(
            ["B", "A"]
                .map(|name| MetaValue::MetaInlines(vec![Inline::Str(name.into())]))
                .to_vec()
        ))
    );
    let mut urls = Vec::new();
    randoc_ast::walk::query_inlines(&doc.blocks, |inline| match inline {
        Inline::Link(_, _, target) | Inline::Image(_, _, target) => urls.push(target.url.clone()),
        _ => {}
    });
    assert_eq!(
        urls,
        [
            "images/cover.png",
            "#one.xhtml_local",
            "#two.xhtml_other",
            "images/cover.png"
        ]
    );
    assert_eq!(context.media.len(), 1);
    assert!(
        matches!(doc.blocks.last(), Some(Block::Header(_, attr, _)) if attr.id == "two.xhtml_other")
    );
}

#[test]
fn leaves_images_outside_the_package_unloaded() {
    let bytes = fixture(
        OPF,
        r#"<p><img src="../../top.png" alt="Outside" /></p>"#,
        "",
    );
    let mut context = Context::pure();
    let doc = read(&bytes, &mut context).unwrap();
    let mut urls = Vec::new();
    randoc_ast::walk::query_inlines(&doc.blocks, |inline| {
        if let Inline::Image(_, _, target) = inline {
            urls.push(target.url.clone());
        }
    });
    assert_eq!(urls, ["images/cover.png", "../top.png"]);
    assert_eq!(context.media.len(), 1);
}

#[test]
fn uses_lang_for_the_package_language() {
    let writer = super::writer("epub3").unwrap();
    let mut doc = Pandoc::default();
    doc.meta
        .insert("date", MetaValue::MetaString("2020-02-30".into()));
    for (lang, expected) in [(None, "en-US"), (Some(""), ""), (Some("C.UTF-8"), "C")] {
        let mut host = PureHost::default();
        host.environment
            .insert("LC_ALL".into(), "de_DE.UTF-8".into());
        if let Some(lang) = lang {
            host.environment.insert("LANG".into(), lang.into());
        }
        let mut context = Context::new(Box::new(host));
        let bytes = writer
            .write(&doc, &WriterOptions::default(), &mut context)
            .unwrap()
            .into_bytes();
        let mut zip = Archive::open(&bytes, ArchiveLimits::default()).unwrap();
        let package = String::from_utf8(zip.read("EPUB/content.opf").unwrap().unwrap()).unwrap();
        assert!(package.contains(&format!("<dc:language>{expected}</dc:language>")));
        assert!(package.contains(&format!("xml:lang=\"{expected}\"")));
        assert!(package.contains("<dc:date id=\"epub-date\"></dc:date>"));
    }
}

#[test]
fn reports_epub_metadata_errors_at_original_xml_positions() {
    let writer = super::writer("epub3").unwrap();
    for (prefix, span) in [
        ("\n", "2:12 (12)-2:22 (22)"),
        ("<?xml version=\"1.0\"?>\n", "2:12 (33)-2:22 (43)"),
        ("\n\n", "3:12 (13)-3:22 (23)"),
    ] {
        let mut options = WriterOptions::default();
        options.epub_metadata = Some(format!("{prefix}<dc:title>A</dc:titl>"));
        let error = writer
            .write(&Pandoc::default(), &options, &mut Context::pure())
            .unwrap_err();
        assert_eq!(error.exit_code(), 44);
        assert_eq!(
            error.to_string(),
            format!("Invalid XML in 'epub metadata':\n{span}: Unexpected close tag dc:titl")
        );
    }
}

#[test]
fn reports_archive_and_xml_errors() {
    let context = &mut Context::pure();
    let error = read(b"no", context).unwrap_err();
    assert_eq!(error.exit_code(), 64);
    assert_eq!(
        error.to_string(),
        "Couldn't extract ePub file: not enough bytes"
    );
    let error = read(&fixture("<package><metadata></package>", "", ""), context).unwrap_err();
    assert!(
        matches!(error, Error::Xml { ref file, ref message } if file == "'OPS/content.opf'" && message.contains("Unexpected close tag package"))
    );
    assert_eq!(error.exit_code(), 44);
}

#[test]
fn charges_archive_and_table_budgets_across_chapters() {
    let table = "<table><tr><td colspan='3'>x</td></tr></table>";
    let bytes = fixture(OPF, table, table);
    let mut context = Context::pure();
    context.limits.max_table_cells = 6;
    assert!(
        read(&bytes, &mut context)
            .unwrap_err()
            .to_string()
            .contains("cells")
    );
    assert_eq!(context.limits.max_table_cells, 6);
    context.limits.max_table_cells = 100;
    context.limits.max_archive_size = (CONTAINER.len() + OPF.len() + table.len() + 3) as u64;
    assert!(
        read(&bytes, &mut context)
            .unwrap_err()
            .to_string()
            .contains("archive size")
    );
}

#[test]
fn allows_empty_subdirectory_and_rejects_non_alphanumeric_names() {
    let writer = super::writer("epub3").unwrap();
    let doc = Pandoc {
        meta: Meta::default(),
        blocks: vec![Block::HorizontalRule],
    };
    let mut options = WriterOptions::default();
    let mut context = Context::pure();
    for name in ["a_b", "a-b", "../bad", "é"] {
        options.epub_subdirectory = name.into();
        assert_eq!(
            writer.write(&doc, &options, &mut context).unwrap_err(),
            Error::EpubSubdirectory(name.into())
        );
    }
    options.epub_subdirectory.clear();
    let bytes = writer
        .write(&doc, &options, &mut context)
        .unwrap()
        .into_bytes();
    let zip = Archive::open(&bytes, ArchiveLimits::default()).unwrap();
    assert!(zip.contains("content.opf"));
    assert!(!zip.contains("EPUB/content.opf"));
}

#[test]
fn stores_mimetype_first_and_keeps_a_separate_cover_resource() {
    let mut doc = Pandoc::default();
    doc.blocks.push(Block::Para(
        ["picture.png", "second.png"]
            .into_iter()
            .map(|source| Inline::Image(Attr::default(), Vec::new(), Target::new(source, "")))
            .collect(),
    ));
    doc.meta
        .insert("title", MetaValue::MetaString("Book".into()));
    doc.meta
        .insert("cover-image", MetaValue::MetaString("picture.png".into()));
    let mut host = PureHost::default();
    host.files.insert("font.ttf".into(), b"first font".to_vec());
    host.files
        .insert("folder/font.ttf".into(), b"second font".to_vec());
    let mut context = Context::new(Box::new(host));
    for source in ["picture.png", "second.png"] {
        context.media.insert(
            source,
            MediaItem {
                mime_type: "image/png".into(),
                contents: source.as_bytes().to_vec(),
            },
        );
    }
    let mut options = WriterOptions::default();
    options.epub_fonts = vec!["font.ttf".into(), "folder/font.ttf".into()];
    for format in ["epub2", "epub3"] {
        let bytes = super::writer(format)
            .unwrap()
            .write(&doc, &options, &mut context)
            .unwrap()
            .into_bytes();
        assert_eq!(&bytes[8..10], &[0, 0]);
        assert_eq!(&bytes[28..30], &[0, 0]);
        let mut zip = Archive::open(&bytes, ArchiveLimits::default()).unwrap();
        assert_eq!(
            zip.entry_names(),
            [
                "mimetype",
                "META-INF/container.xml",
                "META-INF/com.apple.ibooks.display-options.xml",
                "EPUB/content.opf",
                "EPUB/toc.ncx",
                "EPUB/nav.xhtml",
                "EPUB/text/title_page.xhtml",
                "EPUB/styles/stylesheet1.css",
                "EPUB/media/file1.png",
                "EPUB/media/file0.png",
                "EPUB/media/file2.png",
                "EPUB/text/cover.xhtml",
                "EPUB/text/ch001.xhtml",
                "EPUB/fonts/font.ttf",
            ]
        );
        assert_eq!(
            zip.read("mimetype").unwrap().unwrap(),
            b"application/epub+zip"
        );
        assert_eq!(
            zip.read("EPUB/media/file0.png").unwrap().unwrap(),
            zip.read("EPUB/media/file2.png").unwrap().unwrap()
        );
        assert_eq!(
            zip.read("EPUB/fonts/font.ttf").unwrap().unwrap(),
            b"second font"
        );
        let package = String::from_utf8(zip.read("EPUB/content.opf").unwrap().unwrap()).unwrap();
        let cover = package.find("href=\"media/file2.png\"").unwrap();
        let second = package.find("href=\"media/file1.png\"").unwrap();
        let first = package.find("href=\"media/file0.png\"").unwrap();
        assert!(cover < second && second < first);
        assert_eq!(package.matches("id=\"font_ttf\"").count(), 2);
        if format == "epub3" {
            assert!(package.contains("<item properties=\"cover-image\" id=\"file2_png\""));
        }
        let page = String::from_utf8(zip.read("EPUB/text/cover.xhtml").unwrap().unwrap()).unwrap();
        assert!(page.contains("<title>Book</title>"));
    }
}

#[test]
fn enforces_nesting_on_a_two_mib_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut context = Context::pure();
            let depth = 96;
            let markup = format!(
                "{}<p>text</p>{}",
                "<div>".repeat(depth),
                "</div>".repeat(depth)
            );
            let bytes = fixture(OPF, &markup, "<p>two</p>");
            let doc = read(&bytes, &mut context).unwrap();
            let writer = super::writer("epub3").unwrap();
            writer
                .write(&doc, &WriterOptions::default(), &mut context)
                .unwrap();
            context.limits.max_nesting_depth = 20;
            assert!(read(&bytes, &mut context).is_err());
            assert!(
                writer
                    .write(&doc, &WriterOptions::default(), &mut context)
                    .is_err()
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn embeds_remote_images_and_warns_on_fetch_failure() {
    use randoc_ast::MathType;
    use randoc_core::{Host, HtmlMathMethod, Text};

    struct WebTex;
    impl Host for WebTex {
        fn fetch_url(
            &mut self,
            url: &str,
            _: &[(Text, Text)],
        ) -> randoc_core::Result<(Vec<u8>, Option<Text>)> {
            if url == "http://example.test/missing.png" {
                return Err(Error::Http {
                    url: url.into(),
                    message: "file missing".into(),
                });
            }
            assert!(
                url.starts_with("https://example.test/render?")
                    || url == "https://example.test/photo.png"
            );
            Ok((b"img".to_vec(), Some("image/png".into())))
        }
    }
    let doc = Pandoc {
        meta: Meta::default(),
        blocks: vec![Block::Para(vec![
            Inline::Math(MathType::InlineMath, "x=2".into()),
            Inline::Image(
                Attr::default(),
                Vec::new(),
                Target::new("https://example.test/photo.png", ""),
            ),
            Inline::Image(Attr::default(), Vec::new(), Target::new("//example.test/missing.png", "")),
        ]), Block::RawBlock(randoc_ast::Format::new("html"), r#"<svg><image xlink:href="untouched.png" /></svg><video src="video.mp4" poster="poster.png"><source src="movie.webm" /><track src="track.vtt" /></video><audio src="sound.mp3" />"#.into())],
    };
    let mut context = Context::new(Box::new(WebTex));
    let mut options = WriterOptions::default();
    options.html_math_method = HtmlMathMethod::WebTex("https://example.test/render?".into());
    let bytes = super::writer("epub3")
        .unwrap()
        .write(&doc, &options, &mut context)
        .unwrap()
        .into_bytes();
    let mut zip = Archive::open(&bytes, ArchiveLimits::default()).unwrap();
    let chapter = String::from_utf8(zip.read("EPUB/text/ch001.xhtml").unwrap().unwrap()).unwrap();
    let package = String::from_utf8(zip.read("EPUB/content.opf").unwrap().unwrap()).unwrap();
    assert_eq!(zip.read("EPUB/media/file0.png").unwrap().unwrap(), b"img");
    assert!(chapter.contains("src=\"../media/file0.png\""));
    assert_eq!(zip.read("EPUB/media/file1.png").unwrap().unwrap(), b"img");
    assert!(chapter.contains("src=\"../media/file1.png\""));
    assert!(chapter.contains("src=\"..///example.test/missing.png\""));
    assert!(chapter.contains("xlink:href=\"untouched.png\""));
    assert!(chapter.contains("src=\"track.vtt\""));
    for file in ["video.mp4", "poster.png", "movie.webm", "sound.mp3"] {
        assert!(chapter.contains(&format!("=\"../{file}\"")));
    }
    assert!(!package.contains("remote-resources"));
    assert_eq!(context.log().len(), 5);
    assert!(matches!(&context.log()[0],
        randoc_core::LogMessage::CouldNotFetchResource(source, reason)
        if source == "//example.test/missing.png" && reason == "PandocHttpError \"http://example.test/missing.png\" \"file missing\""
    ));
}

#[test]
fn skips_title_page_fields_when_title_page_is_disabled() {
    let mut doc = Pandoc::default();
    doc.meta.insert(
        "abstract",
        MetaValue::MetaBlocks(vec![Block::CodeBlock(
            Attr::new("", vec!["rust".into()], Vec::new()),
            "fn main() {}".into(),
        )]),
    );
    let writer = super::writer("epub3").unwrap();
    let mut options = WriterOptions::default();
    options.highlight_style = Some("missing.theme".into());
    assert!(writer.write(&doc, &options, &mut Context::pure()).is_err());
    options.epub_title_page = false;
    let bytes = writer
        .write(&doc, &options, &mut Context::pure())
        .unwrap()
        .into_bytes();
    let zip = Archive::open(&bytes, ArchiveLimits::default()).unwrap();
    assert!(!zip.contains("EPUB/text/title_page.xhtml"));
}

#[test]
fn many_chapters_with_large_unused_template_variables() {
    use std::time::{Duration, Instant};

    let mut doc = Pandoc::default();
    doc.meta
        .insert("identifier", MetaValue::MetaString("book".into()));
    for index in 0..256 {
        doc.blocks.push(Block::Header(
            1,
            Attr::new(format!("chapter-{index}"), Vec::new(), Vec::new()),
            vec![Inline::Str("Chapter".into())],
        ));
    }
    let writer = super::writer("epub3").unwrap();
    let mut options = WriterOptions::default();
    let start = Instant::now();
    let expected = writer
        .write(&doc, &options, &mut Context::pure())
        .unwrap()
        .into_bytes();
    let baseline = start.elapsed();
    options.variables.insert(
        "unused",
        randoc_templates::Val::List(vec![randoc_templates::Val::Bool(true); 200_000]),
    );
    let start = Instant::now();
    let actual = writer
        .write(&doc, &options, &mut Context::pure())
        .unwrap()
        .into_bytes();
    let elapsed = start.elapsed();
    assert_eq!(actual, expected);
    assert!(
        elapsed < baseline * 8 + Duration::from_millis(250),
        "{elapsed:?} with variables, {baseline:?} without variables"
    );
}

#[test]
fn selects_a_supported_switch_case_or_its_default() {
    let chapter = r#"<epub:switch>
        <epub:case required-namespace="https://example.test"><p>First</p></epub:case>
        <epub:default><p>Second</p></epub:default>
    </epub:switch>
    <epub:switch>
        <epub:case required-namespace="http://www.w3.org/1998/Math/MathML"><p>Third</p></epub:case>
        <epub:default><p>Fourth</p></epub:default>
    </epub:switch>"#;
    let bytes = fixture(OPF, chapter, "");
    let doc = read(&bytes, &mut Context::pure()).unwrap();
    let words: Vec<_> = doc
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Para(inlines) if matches!(inlines.as_slice(), [Inline::Str(_)]) => {
                Some(randoc_core::shared::stringify(inlines))
            }
            _ => None,
        })
        .collect();
    assert_eq!(words, ["Second", "Third"]);
}

#[test]
fn reads_stylesheets_as_plain_files_as_pandoc_does() {
    use randoc_core::Host;
    use randoc_templates::Val;

    struct Sandboxed(PureHost);
    impl Host for Sandboxed {
        fn is_sandboxed(&self) -> bool {
            true
        }
        fn read_file(&mut self, path: &str) -> std::io::Result<Vec<u8>> {
            self.0.read_file(path)
        }
    }
    let files = || {
        let mut host = PureHost::default();
        host.files
            .insert("sub/real.css".into(), b"p { color: red; }".to_vec());
        host
    };
    let write = |css: &[&str], meta: bool, host: Box<dyn Host>| {
        let mut doc = Pandoc::default();
        let mut options = WriterOptions::default();
        let list = Val::List(css.iter().map(|&path| Val::from(path)).collect());
        if meta {
            let paths = css.iter().map(|&path| MetaValue::MetaString(path.into()));
            doc.meta.insert("css", MetaValue::MetaList(paths.collect()));
        } else {
            options.variables.insert("css", list);
        }
        let mut context = Context::new(host);
        context.state.resource_path = vec!["sub".into()];
        super::writer("epub3")
            .unwrap()
            .write(&doc, &options, &mut context)
            .map(|output| output.into_bytes())
    };
    let open_error = |path: &str| {
        Error::Io(format!(
            "{path}: openBinaryFile: does not exist (No such file or directory)"
        ))
    };
    for meta in [false, true] {
        for (css, missing) in [
            (&["a.css"][..], "a.css"),
            (&["real.css"], "real.css"),
            (&["data:text/css,p{}"], "data:text/css,p{}"),
            (&["file:///sub/real.css"], "file:///sub/real.css"),
            (&["sub/real.css", "b.css"], "b.css"),
        ] {
            let error = write(css, meta, Box::new(files())).unwrap_err();
            assert_eq!(error, open_error(missing), "{css:?}");
            assert_eq!(error.exit_code(), 1);
        }
        let error = write(&["sub/real.css"], meta, Box::new(Sandboxed(files()))).unwrap_err();
        assert_eq!(error, Error::ResourceNotFound("sub/real.css".into()));
        assert_eq!(error.exit_code(), 99);
        let bytes = write(&["sub/real.css"], meta, Box::new(files())).unwrap();
        let mut zip = Archive::open(&bytes, ArchiveLimits::default()).unwrap();
        assert_eq!(
            zip.read("EPUB/styles/stylesheet1.css").unwrap().unwrap(),
            b"p { color: red; }"
        );
    }
}
