use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use flate2::{Compression, write::GzEncoder};
use randoc_core::{Context, Error, Host, LogMessage, MediaItem, PureHost, Result, Text};

use crate::embed_resources;

const PNG: &str = "data:image/png;base64,eA==";

fn files(entries: &[(&str, &[u8])]) -> Context {
    Context::new(Box::new(PureHost {
        files: entries
            .iter()
            .map(|(path, data)| ((*path).into(), data.to_vec()))
            .collect(),
        ..PureHost::default()
    }))
}

#[test]
fn embedding_stops_at_an_expired_deadline() {
    let mut context = files(&[("p.png", b"x")]);
    context.set_deadline(Some(std::time::Instant::now()));
    assert!(matches!(
        embed_resources("<img src='p.png'>", &mut context),
        Err(Error::App(_))
    ));
}

#[test]
fn renders_entities_raw_text_and_empty_tags() -> Result<()> {
    let mut ctx = Context::pure();
    let cases = [
        (
            "<?xml version='1.0'?><p alt='' title='&copy=2'>&nbsp;&apos;<![CDATA[<b>]]></p>\n",
            "<?xml version=\"1.0\" ?><p alt title=\"©=2\">\u{a0}&#39;&lt;b&gt;</p>\n",
        ),
        (
            "<IMG></IMG><path id='x'/><use href='#x'/><div/><meta></META>\n",
            "<IMG /><path id=\"x\" /><use href=\"#x\" /><div></div><meta></META>\n",
        ),
        (
            "<SCRIPT>if (a < b && b > c) {}</SCRIPT><style>f</STYLE>&amp;</style>&amp;\n",
            "<SCRIPT>if (a < b && b > c) {}</SCRIPT><style>f</STYLE>&</style>&amp;\n",
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(embed_resources(input, &mut ctx)?, expected);
    }
    assert!(ctx.log().is_empty());
    Ok(())
}

#[test]
fn rewrites_only_selected_tag_attributes() -> Result<()> {
    let mut ctx = files(&[("p.png", b"x")]);
    let attrs = "src poster data-src data-background-image srcset data-srcset data href xlink:href style background data-background-video data-background-iframe SRC";
    for tag in [
        "img", "IMG", "LINK", "SCRIPT", "object", "a", "image", "link", "script",
    ] {
        let mut input = format!("<{tag}");
        let mut expected = format!("<{tag}");
        let mut expected_attrs = Vec::new();
        for name in attrs.split_whitespace() {
            input.push_str(&format!(" {name}='p.png'"));
            let fetched = match tag {
                "link" => name == "href",
                "script" => name == "src",
                _ => matches!(
                    name,
                    "src" | "poster" | "data-src" | "data-background-image"
                ),
            };
            let value = if fetched { PNG } else { "p.png" };
            let attr = format!(" {name}=\"{value}\"");
            if (tag, name) == ("link", "href") {
                expected_attrs.insert(0, attr);
            } else {
                expected_attrs.push(attr);
            }
        }
        input.push('>');
        expected.push_str(&expected_attrs.concat());
        expected.push('>');
        assert_eq!(embed_resources(&input, &mut ctx)?, expected);
    }
    let input = "<!--<img src=p.png>--><noscript><img src=p.png></noscript>";
    let expected = format!("<!--<img src=p.png>--><noscript><img src=\"{PNG}\"></noscript>");
    assert_eq!(embed_resources(input, &mut ctx)?, expected);
    let input = "<link src=\"p.png\"><script poster=\"p.png\"></script>";
    assert_eq!(embed_resources(input, &mut ctx)?, input);
    Ok(())
}

#[test]
fn inlines_stylesheets_or_keeps_link_attributes() -> Result<()> {
    let mut ctx = files(&[
        ("theme.css", b"a{}"),
        ("weird.css", b"'</STYLE>'"),
        ("raw.unknown", b"a{}"),
        ("icon.png", b"x"),
    ]);
    let cases = [
        (
            "<link id=i rel=icon href=theme.css></link>",
            "<style type=\"text/css\">a{}</style>".into(),
        ),
        (
            "<link id=i media='' href=theme.css />",
            "<style type=\"text/css\">a{}</style>".into(),
        ),
        (
            "<link rel=stylesheet href=raw.unknown>",
            "<style type=\"text/css\">a{}</style>".into(),
        ),
        (
            "<link id=i media=print href=theme.css />",
            "<link href=\"data:text/css;charset=utf-8,a%7B%7D\" id=\"i\" media=\"print\" />".into(),
        ),
        (
            "<link href=weird.css>",
            "<link href=\"data:text/css;charset=utf-8,%27%3C%2FSTYLE%3E%27\">".into(),
        ),
        (
            "<link rel=STYLESHEET href=icon.png />",
            format!("<link href=\"{PNG}\" rel=\"STYLESHEET\" />"),
        ),
        (
            "<link id=i href=data:,a href=data:,b>",
            "<link href=\"data:,a\" id=\"i\">".into(),
        ),
        (
            "<link id=i href='#part'>",
            "<link id=\"i\" href=\"#part\">".into(),
        ),
        (
            "<link id=i href=missing.css>",
            "<link id=\"i\" href=\"missing.css\">".into(),
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(embed_resources(input, &mut ctx)?, expected);
    }
    Ok(())
}

#[test]
fn inlines_scripts_only_when_the_close_tag_is_next_and_safe() -> Result<()> {
    let mut ctx = files(&[
        ("plain.js", b"\xef\xbb\xbfvar x=1;\r\n"),
        ("unsafe.js", b"'</ScRiPtfoo>'"),
        ("script.x", b"a"),
    ]);
    let cases = [
        (
            "<script async id=a src=plain.js type=x></script>",
            "<script type=\"x\">var x=1;\n</script>",
        ),
        (
            "<script src=plain.js type='' />",
            "<script type>var x=1;\n</script>",
        ),
        (
            "<script src=unsafe.js></script>",
            "<script src=\"data:text/javascript;charset=utf-8,%27%3C%2FScRiPtfoo%3E%27\"></script>",
        ),
        (
            "<script defer src=plain.js>x</script>",
            "<script src=\"data:text/javascript;charset=utf-8,var%20x%3D1%3B%0A\" defer>x</script>",
        ),
        (
            "<script src=script.x type=text/ecmascript></script>",
            "<script src=\"data:text/ecmascript;charset=utf-8,a\" type=\"text/ecmascript\"></script>",
        ),
        (
            "<script id=a src=data:,a></script>",
            "<script src=\"data:,a\" id=\"a\"></script>",
        ),
        (
            "<SCRIPT id=a src=plain.js></SCRIPT>",
            "<SCRIPT id=\"a\" src=\"data:text/javascript;charset=utf-8,var%20x%3D1%3B%0A\"></SCRIPT>",
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(embed_resources(input, &mut ctx)?, expected);
    }
    Ok(())
}

#[test]
fn encodes_text_and_binary_data_uris() -> Result<()> {
    let mut ctx = files(&[
        ("t.txt", "\u{feff}é\r\n~-_.!*()'\u{feff} ".as_bytes()),
        ("t.x", b"x"),
        ("p%41.png", b"x"),
    ]);
    let input = "<img src='t.txt?x#f'><img type='text/plain; foo=bar' src=t.x><img type=TEXT/PLAIN src=t.x><img type='' src=t.x><img src='p%2541.png?x#f'><img src='DATA:,hi'><img src='DATA:text/plain;base64,YQ'><img src='DATA:text/plain;charset=x,hi%21'>";
    let expected = format!(
        concat!(
            "<img src=\"data:text/plain;charset=utf-8,%C3%A9%0A~-_.%21%2A%28%29%27%EF%BB%BF%20\">",
            "<img type=\"text/plain; foo=bar\" src=\"data:text/plain; foo=bar,x\">",
            "<img type=\"TEXT/PLAIN\" src=\"data:TEXT/PLAIN;base64,eA==\">",
            "<img type src=\"data:application/octet-stream;base64,eA==\">",
            "<img src=\"{}\"><img src=\"data:;base64,aGk=\">",
            "<img src=\"data:text/plain;charset=utf-8,a\"><img src=\"data:text/plain;charset=x,hi%21\">",
        ),
        PNG
    );
    assert_eq!(embed_resources(input, &mut ctx)?, expected);
    Ok(())
}

#[test]
fn processes_css_bytes_and_resolves_nested_imports() -> Result<()> {
    let mut ctx = files(&[
        ("assets/c/a.css", b"/* drop */a{ content:\"/*x*/q   w\"; }\r\n\t@import'../b.css';\n@import url('../b.css') print;\n/* open\n"),
        ("assets/c/../b.css", b".b{src:myurl( '../p.png' )}\n"),
        ("assets/c/../../p.png", b"x"),
        ("p.png", b"x"),
    ]);
    ctx.state.resource_path = vec!["missing".into(), "assets".into(), ".".into()];
    let html = "<link href=c/a.css><style>\t \nurl( \" missing \" ) url( #frag ) url( 'data:,x' ) Url(p.png) url(p.png) url(no-end\u{c}\u{b}</style><STYLE>url(p.png)</STYLE>";
    let expected = format!(
        concat!(
            "<style type=\"text/css\">a{{ content:\"q w\"; }}.b{{src:myurl({})}}\n\n",
            "@import url(data:text/css;charset=utf-8,.b%7Bsrc%3Amyurl%28data%3Aimage%2Fpng%3Bbase64%2CeA%3D%3D%29%7D%0A) print;\n",
            "/* open\n</style><style>\turl(\"missing\") url(#frag) url('data:,x') Url(p.png) url({}) url(no-end\u{c}\u{b}</style>",
            "<STYLE>url(p.png)</STYLE>",
        ),
        PNG, PNG
    );
    assert_eq!(embed_resources(html, &mut ctx)?, expected);
    Ok(())
}

#[test]
fn css_references_to_stylesheets_expand_and_stop_at_cycles() -> Result<()> {
    let mut ctx = files(&[
        ("a.css", b"a{}\r\n\t b{src:url(a.css)}"),
        ("./a.css", b"a{}\r\n\t b{src:url(a.css)}"),
    ]);
    assert_eq!(
        embed_resources("<img src=a.css>", &mut ctx)?,
        "<img src=\"data:text/css;charset=utf-8,a%7B%7Db%7Bsrc%3Aurl%28a.css%29%7D\">"
    );
    assert!(
        matches!(ctx.log().last(), Some(LogMessage::CouldNotFetchResource(url, detail))
        if url == "./a.css" && detail == "the CSS resources form a cycle")
    );
    Ok(())
}

#[test]
fn keeps_import_rules_when_a_cycle_or_depth_limit_stops_expansion() -> Result<()> {
    let mut ctx = files(&[
        ("c/self.css", b"@import './self.css';"),
        ("c/./self.css", b"@import './self.css';"),
        ("c/one.css", b"@import 'two.css';"),
        ("c/two.css", b"@import 'one.css';"),
    ]);
    assert_eq!(
        embed_resources("<link href=c/self.css><link href=c/one.css>", &mut ctx)?,
        "<style type=\"text/css\">@import './self.css';</style><style type=\"text/css\">@import 'one.css';</style>"
    );
    assert_eq!(ctx.log().iter().filter(|message| matches!(message,
        LogMessage::CouldNotFetchResource(_, detail) if detail == "the @import rules form a cycle"
    )).count(), 2);
    ctx.limits.max_nesting_depth = 0;
    assert_eq!(
        embed_resources("<link href=c/one.css>", &mut ctx)?,
        "<style type=\"text/css\">@import 'two.css';</style>"
    );
    assert!(
        matches!(ctx.log().last(), Some(LogMessage::CouldNotFetchResource(url, detail))
        if url == "c/two.css" && detail == "too many nested @import rules")
    );
    Ok(())
}

#[test]
fn stops_after_a_bounded_number_of_css_expansions() {
    let mut ctx = Context::pure();
    for level in 0..40 {
        let next = level + 1;
        ctx.media.insert(
            format!("c{level}.css"),
            MediaItem {
                mime_type: "text/css".into(),
                contents: format!("@import 'c{next}.css';@import 'c{next}.css';").into_bytes(),
            },
        );
    }
    ctx.media.insert(
        "c40.css",
        MediaItem {
            mime_type: "text/css".into(),
            contents: b"/* empty */".to_vec(),
        },
    );
    assert_eq!(
        embed_resources("<style>@import 'c0.css';</style>", &mut ctx),
        Err(Error::App(
            "The embedded resources exceed the size limit.".into()
        ))
    );
}

#[test]
fn preserves_unterminated_css_without_rescanning_its_suffix() -> Result<()> {
    let css = format!("{}{}", "/*x".repeat(50_000), "@import url(".repeat(50_000));
    let html = format!("<style>{css}</style>");
    assert_eq!(embed_resources(&html, &mut Context::pure())?, html);
    Ok(())
}

#[test]
fn caches_successes_and_failures_by_url_and_hint() -> Result<()> {
    let mut ctx = files(&[("p.png", b"x")]);
    embed_resources(
        "<img src=p.png poster=p.png><style>url(p.png) url(nope)</style><img src=nope type=''><link href=nope type=x><script src=nope type=x></script>",
        &mut ctx,
    )?;
    assert_eq!(
        ctx.log(),
        [
            LogMessage::LoadedResource("p.png".into(), "p.png".into()),
            LogMessage::CouldNotFetchResource("nope".into(), "".into()),
            LogMessage::CouldNotFetchResource("nope".into(), "".into()),
        ]
    );
    Ok(())
}

struct Sandbox;

impl Host for Sandbox {
    fn is_sandboxed(&self) -> bool {
        true
    }
}

#[test]
fn sandbox_allows_the_media_bag_and_data_uris() -> Result<()> {
    let mut ctx = Context::new(Box::new(Sandbox));
    ctx.media.insert(
        "css/bag.css",
        MediaItem {
            mime_type: "text/css".into(),
            contents: b"a{src:url(../p.png)}".to_vec(),
        },
    );
    ctx.media.insert(
        "p.png",
        MediaItem {
            mime_type: "image/png".into(),
            contents: b"x".to_vec(),
        },
    );
    let html = "<link href='css/./bag.css?q#f'><img src='sub/../p.png?q#f'><img src='DATA:,hi'><img src='data:,x'><img src='#f'><img src='http://host/a'><img src='ftp://host/a'><img src='file:///missing'><img src='file:missing'>";
    let expected = format!(
        "<style type=\"text/css\">a{{src:url({PNG})}}</style><img src=\"{PNG}\"><img src=\"data:;base64,aGk=\"><img src=\"data:,x\"><img src=\"#f\"><img src=\"http://host/a\"><img src=\"ftp://host/a\"><img src=\"file:///missing\"><img src=\"file:missing\">"
    );
    assert_eq!(embed_resources(html, &mut ctx)?, expected);
    assert_eq!(
        ctx.log(),
        ["http://host/a", "ftp://host/a", "/missing", "missing"]
            .map(|path| LogMessage::CouldNotFetchResource(path.into(), "".into()))
    );
    Ok(())
}

#[test]
fn invalid_utf8_and_unreadable_file_urls_are_fatal() {
    let mut ctx = files(&[
        ("bad.txt", b"abc\xe9"),
        ("bad.bin", b"\xe9"),
        ("bad.js", b"\xe9"),
    ]);
    let invalid =
        Error::Io("Cannot decode byte '\\xe9': Data.Text.Encoding: Invalid UTF-8 stream".into());
    for html in [
        "<img src=bad.txt>",
        "<link rel=stylesheet href=bad.bin>",
        "<script src=bad.js></script>",
    ] {
        assert_eq!(embed_resources(html, &mut ctx), Err(invalid.clone()));
    }
    assert_eq!(
        embed_resources(
            "<img src='file://localhost/path with space.png?x#f'>",
            &mut ctx
        ),
        Err(Error::Io(
            "/path%20with%20space.png: withBinaryFile: does not exist (No such file or directory)"
                .into()
        ))
    );
}

fn gzip(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes)?;
    encoder.finish()
}

#[test]
fn limits_individual_resources_and_the_total_embedded_size()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let bomb = gzip(&[b'x'; 10_000])?;
    let compressed = gzip(b"x")?;
    let mut ctx = files(&[
        ("big.png", b"123456789"),
        ("bomb.png.gz", &bomb),
        ("p.png.gz", &compressed),
    ]);
    ctx.limits.max_resource_size = 8;
    ctx.media.insert(
        "bag.png.gz",
        MediaItem {
            mime_type: "image/PNG".into(),
            contents: compressed,
        },
    );
    assert_eq!(
        embed_resources(
            "<img src=big.png><img src=bomb.png.gz><img src=p.png.gz><img src=bag.png.gz>",
            &mut ctx
        )?,
        format!(
            "<img src=\"big.png\"><img src=\"bomb.png.gz\"><img src=\"{PNG}\"><img src=\"data:image/PNG;base64,eA==\">"
        ),
    );
    assert_eq!(ctx.log().iter().filter(|message| matches!(message,
        LogMessage::CouldNotFetchResource(_, detail) if detail == "the resource exceeds the size limit"
    )).count(), 2);
    ctx.limits.max_embedded_size = PNG.len() as u64;
    assert_eq!(
        embed_resources("<img src=p.png.gz>", &mut ctx)?,
        format!("<img src=\"{PNG}\">")
    );
    let error = Err(Error::App(
        "The embedded resources exceed the size limit.".into(),
    ));
    assert_eq!(
        embed_resources("<img src=p.png.gz poster=p.png.gz>", &mut ctx),
        error
    );
    assert_eq!(
        embed_resources("<style>url(p.png.gz) url(p.png.gz)</style>", &mut ctx),
        error
    );
    Ok(())
}

#[test]
fn bounds_decoded_cache_bytes_even_when_svg_is_not_embedded()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let compressed = gzip(&[b'x'; 256])?;
    let mut ctx = files(&[("a.svgz", &compressed), ("b.svgz", &compressed)]);
    ctx.limits.max_embedded_size = 300;
    let image = "<img class='inline-svg' src=a.svgz>";
    assert_eq!(
        embed_resources(&image.repeat(2), &mut ctx)?,
        "<img class=\"inline-svg\">".repeat(2)
    );
    assert_eq!(
        embed_resources(
            &format!("{image}<img class='inline-svg' src=b.svgz>"),
            &mut ctx
        ),
        Err(Error::App(
            "The embedded resources exceed the size limit.".into()
        ))
    );
    Ok(())
}

type Requests = Arc<Mutex<Vec<(Text, Vec<(Text, Text)>)>>>;

struct MockHttp {
    requests: Requests,
    compressed: Vec<u8>,
}

impl Host for MockHttp {
    fn fetch_url(
        &mut self,
        url: &str,
        headers: &[(Text, Text)],
    ) -> Result<(Vec<u8>, Option<Text>)> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((url.into(), headers.to_vec()));
        let (bytes, mime) = match url {
            "https://example.test/a/b/redirect.css" => (
                b"a{src:url(../../rel?q=1#f)}".to_vec(),
                Some("text/css; charset=utf-8"),
            ),
            "https://example.test/rel?q=1" => (b"x".to_vec(), Some("image/PNG")),
            "https://example.test/no-type.png" => (b"hi".to_vec(), None),
            "https://example.test/zip.svgz" => (self.compressed.clone(), None),
            _ => {
                return Err(Error::Http {
                    url: url.into(),
                    message: "Mock request failed.".into(),
                });
            }
        };
        Ok((bytes, mime.map(str::to_owned)))
    }
}

#[test]
fn http_preserves_response_mime_and_resolves_css_from_the_request_url()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let requests = Requests::default();
    let mut ctx = Context::new(Box::new(MockHttp {
        requests: requests.clone(),
        compressed: gzip(b"<svg/>")?,
    }));
    ctx.state.source_url = Some("https://example.test/input.md".into());
    ctx.state.request_headers = vec![("X-Test".into(), "value".into())];
    let html = "<link href='https://example.test/a/b/redirect.css#f'><img src='//example.test/no-type.png'><img src='https://example.test/no-type.png' type='text/plain'><img src='https://example.test/zip.svgz'>";
    let expected = concat!(
        "<style type=\"text/css\">a{src:url(data:image/PNG;base64,eA==)}</style>",
        "<img src=\"data:application/octet-stream;base64,aGk=\">",
        "<img src=\"data:text/plain;charset=utf-8,hi\" type=\"text/plain\">",
        "<img src=\"data:application/octet-stream;base64,PHN2Zy8+\">",
    );
    assert_eq!(embed_resources(html, &mut ctx)?, expected);
    let requests = requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        requests
            .iter()
            .map(|(url, _)| url.as_str())
            .collect::<Vec<_>>(),
        [
            "https://example.test/a/b/redirect.css",
            "https://example.test/rel?q=1",
            "https://example.test/no-type.png",
            "https://example.test/no-type.png",
            "https://example.test/zip.svgz",
        ]
    );
    for (_, headers) in requests.iter() {
        assert_eq!(
            headers,
            &[
                ("X-Test".into(), "value".into()),
                ("User-Agent".into(), "pandoc/3.13".into())
            ]
        );
    }
    assert!(
        ctx.log()
            .iter()
            .all(|message| matches!(message, LogMessage::Fetching(_)))
    );
    Ok(())
}

#[test]
fn duplicate_inline_svg_points_to_the_last_generated_id() -> Result<()> {
    let resource = br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" version="1.1" viewBox="-.5 0 40 25"><path /></svg>"#;
    let mut ctx = files(&[("art.svg", resource), ("copy.svg", resource)]);
    let digest = randoc_archive::media::sha1_hex(resource);
    let id = format!("svg_{}", &digest[..20]);
    let input = r#"<img src="art.svg" class="inline-svg" alt="Art"><img src="copy.svg" class="inline-svg" alt="Art">"#;
    assert_eq!(
        embed_resources(input, &mut ctx)?,
        format!(
            concat!(
                "<svg role=\"img\" aria-label=\"Art\" alt=\"Art\" viewBox=\"-.5 0 40 25\" width=\"40\" height=\"25\">",
                "<use href=\"#{0}\" width=\"100%\" height=\"100%\" /></svg>",
                "<svg id=\"{0}\" role=\"img\" aria-label=\"Art\" alt=\"Art\" viewBox=\"-.5 0 40 25\" width=\"40\" height=\"25\">",
                "<path /></svg>"
            ),
            id
        )
    );
    Ok(())
}

#[test]
fn duplicate_inline_svg_prefers_the_last_image_id() -> Result<()> {
    let mut ctx = files(&[("art.svg", br#"<svg width="40" height="25"><g /></svg>"#)]);
    let input = r#"<img id="first" src="art.svg" class="inline-svg" alt="A"><img id="last" src="art.svg" class="inline-svg" alt="B">"#;
    assert_eq!(
        embed_resources(input, &mut ctx)?,
        concat!(
            "<svg role=\"img\" aria-label=\"A\" alt=\"A\" width=\"40\" height=\"25\" viewBox=\"0 0 40 25\">",
            "<use href=\"#last\" width=\"100%\" height=\"100%\" /></svg>",
            "<svg id=\"last\" role=\"img\" aria-label=\"B\" alt=\"B\" width=\"40\" height=\"25\" viewBox=\"0 0 40 25\"><g></g></svg>"
        )
    );
    Ok(())
}

#[test]
fn inline_svg_prefixes_ids_fragment_links_and_paint_references() -> Result<()> {
    let mut ctx = files(&[("art.svg", br##"<svg width="50pt" height="25pt" viewBox="0 0 50 25"><defs><clipPath id="clip"><path /></clipPath></defs><use href="#clip" xlink:href="#clip" clip-path="url(#clip)" /></svg>"##)]);
    let input = r#"<img src="art.svg" class="inline-svg" id="art">"#;
    assert_eq!(
        embed_resources(input, &mut ctx)?,
        concat!(
            "<svg id=\"art\" role=\"img\" width=\"50pt\" height=\"25pt\" viewBox=\"0 0 50 25\">",
            "<defs><clipPath id=\"art_clip\"><path /></clipPath></defs>",
            "<use href=\"#art_clip\" xlink:href=\"#art_clip\" clip-path=\"url(#art_clip)\" /></svg>"
        )
    );
    Ok(())
}

#[test]
fn inline_svg_keeps_the_svg_class_and_merges_image_attributes() -> Result<()> {
    let mut ctx = files(&[(
        "art.svg",
        br#"<svg id="art" class="artwork" width="50pt" height="25pt" viewBox="0 0 50 25"></svg>"#,
    )]);
    let input =
        r#"<img class="extra inline-svg" src="art.svg" width="40" alt="A" aria-label="Label" />"#;
    assert_eq!(
        embed_resources(input, &mut ctx)?,
        concat!(
            "<svg id=\"art\" role=\"img\" width=\"40\" alt=\"A\" aria-label=\"Label\" height=\"25pt\"",
            " viewBox=\"0 0 50 25\" class=\"extra inline-svg artwork\"></svg>"
        )
    );
    Ok(())
}

#[test]
fn embeds_dat_and_shtml_with_oracle_mime_types() -> Result<()> {
    let mut ctx = files(&[("a.dat", b"hello"), ("a.shtml", b"hello world")]);
    assert_eq!(
        embed_resources(r#"<img src="a.dat"><img src="a.shtml">"#, &mut ctx)?,
        concat!(
            "<img src=\"data:chemical/x-mopac-input;base64,aGVsbG8=\">",
            "<img src=\"data:text/html;charset=utf-8,hello%20world\">"
        )
    );
    Ok(())
}

#[test]
fn missing_absolute_path_returns_io_error_after_relative_warning() {
    let mut ctx = Context::pure();
    let error = embed_resources(
        r#"<img src=" img/pixel.png "><img src="/img/pixel.png">"#,
        &mut ctx,
    );
    assert!(matches!(error, Err(Error::Io(message))
        if message == "/img/pixel.png: withBinaryFile: does not exist (No such file or directory)"));
    assert!(
        matches!(ctx.log(), [LogMessage::CouldNotFetchResource(url, detail)]
        if url == " img/pixel.png " && detail.is_empty())
    );
}
