//! Compare DOCX output with the Pandoc 3.13 executable.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use randoc_archive::xml::{XmlElement, XmlLimits, XmlNode, XmlOptions, parse};
use randoc_archive::zip::{Archive, ArchiveLimits, ArchiveWriter};
use randoc_ast::{Block, Inline, Pandoc, json};
use randoc_core::{
    Context, Extension, HighlightMethod, Host, NativeHost, Output, PureHost, WriterOptions,
};

const EPOCH: &str = "1700000001";
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const GIF: &str = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";

fn oracle(dir: &Path, args: &[&str], input: &[u8]) -> Vec<u8> {
    let executable = std::env::var_os("RANDOC_ORACLE_PANDOC").expect("Set RANDOC_ORACLE_PANDOC.");
    let mut process = Command::new(executable)
        .current_dir(dir)
        .args(args)
        .env("SOURCE_DATE_EPOCH", EPOCH)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    process.stdin.take().unwrap().write_all(input).unwrap();
    let result = process.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}

fn writer(doc: &Pandoc, options: &WriterOptions, context: &mut Context) -> Vec<u8> {
    let writer = randoc_docx::writer("docx").unwrap();
    assert!(writer.renders_template());
    let Output::Binary(bytes) = writer.write(doc, options, context).unwrap() else {
        panic!("Expected DOCX bytes.");
    };
    bytes
}

fn reference() -> Vec<u8> {
    let doc = format!(
        r#"<w:document xmlns:w="{W}" xmlns:r="{R}"><w:body><w:p><w:r><w:t>Reference body</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rId1"/><w:footerReference w:type="default" r:id="rId2"/><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="2160" w:bottom="1440" w:left="2160" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    );
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{R}/header" Target="header1.xml"/><Relationship Id="rId2" Type="{R}/footer" Target="footer1.xml"/></Relationships>"#
    );
    let header =
        format!(r#"<w:hdr xmlns:w="{W}"><w:p><w:r><w:t>Example Header</w:t></w:r></w:p></w:hdr>"#);
    let footer =
        format!(r#"<w:ftr xmlns:w="{W}"><w:p><w:r><w:t>Example Footer</w:t></w:r></w:p></w:ftr>"#);
    let mut styles = format!(r#"<w:styles xmlns:w="{W}">"#);
    for (kind, id, name) in [
        ("paragraph", "LocalStyle", "Local Style"),
        ("paragraph", "Title", "Title"),
        ("paragraph", "Subtitle", "Subtitle"),
        ("paragraph", "Author", "Author"),
        ("paragraph", "Date", "Date"),
        ("paragraph", "Abstract", "Abstract"),
        ("paragraph", "AbstractTitle", "Abstract Title"),
        ("paragraph", "FirstParagraph", "First Paragraph"),
        ("paragraph", "BodyText", "Body Text"),
        ("paragraph", "Compact", "Compact"),
        ("paragraph", "BlockText", "Block Text"),
        ("paragraph", "Heading1", "heading 1"),
        ("paragraph", "DefinitionTerm", "Definition Term"),
        ("paragraph", "Definition", "Definition"),
        ("paragraph", "SourceCode", "Source Code"),
        ("paragraph", "FootnoteText", "Footnote Text"),
        ("paragraph", "CommentText", "Comment Text"),
        ("paragraph", "TableCaption", "Table Caption"),
        ("paragraph", "ImageCaption", "Image Caption"),
        ("paragraph", "CaptionedFigure", "Captioned Figure"),
        ("paragraph", "TOCHeading", "TOC Heading"),
        ("character", "BodyTextChar", "Body Text Char"),
        ("character", "AccentText", "Accent Text"),
        ("character", "Hyperlink", "Hyperlink"),
        ("character", "FootnoteReference", "Footnote Reference"),
        ("character", "CommentReference", "Comment Reference"),
        ("character", "SectionNumber", "Section Number"),
        ("table", "Table", "Table"),
    ] {
        use std::fmt::Write as _;
        write!(
            styles,
            r#"<w:style w:type="{kind}" w:styleId="{id}"><w:name w:val="{name}"/></w:style>"#
        )
        .unwrap();
    }
    styles.push_str("</w:styles>");
    let types = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#;
    let mut archive = ArchiveWriter::new();
    for (name, xml) in [
        ("[Content_Types].xml", types),
        ("word/document.xml", &doc),
        ("word/_rels/document.xml.rels", &rels),
        ("word/header1.xml", &header),
        ("word/footer1.xml", &footer),
        ("word/styles.xml", &styles),
    ] {
        archive.write_deflated(name, xml.as_bytes()).unwrap();
    }
    archive.finish().unwrap()
}

fn read_part(archive: &mut Archive<'_>, name: &str) -> Vec<u8> {
    archive.read(name).unwrap().unwrap()
}

#[test]
fn writer_selects_reference_and_retains_header_and_footer() {
    let host = PureHost {
        files: BTreeMap::from([
            ("user/reference.docx".into(), reference()),
            ("user/docx/word/styles.xml".into(), b"broken".to_vec()),
            (
                "resources/empty.docx".into(),
                ArchiveWriter::new().finish().unwrap(),
            ),
            ("resources/bad.docx".into(), b"invalid".to_vec()),
            ("resources/refdir/item".into(), Vec::new()),
        ]),
        environment: BTreeMap::from([("SOURCE_DATE_EPOCH".into(), EPOCH.into())]),
        ..PureHost::default()
    };
    let mut context = Context::new(Box::new(host));
    context.state.user_data_dir = Some("user".into());
    context.state.resource_path = vec!["resources".into()];
    let mut doc = Pandoc::default();
    doc.blocks
        .push(randoc_ast::Block::Para(vec![randoc_ast::Inline::Str(
            "Output body".into(),
        )]));
    let bytes = writer(&doc, &WriterOptions::default(), &mut context);
    assert_eq!(bytes, writer(&doc, &WriterOptions::default(), &mut context));
    let mut archive = Archive::open(&bytes, ArchiveLimits::default()).unwrap();
    for (name, expected) in [
        ("word/document.xml", "Output body"),
        ("word/header1.xml", "Example Header"),
        ("word/footer1.xml", "Example Footer"),
        ("docProps/core.xml", "2023-11-14T22:13:21Z"),
    ] {
        assert!(
            String::from_utf8(read_part(&mut archive, name))
                .unwrap()
                .contains(expected)
        );
    }
    let mut options = WriterOptions::default();
    options.reference_doc = Some("empty.docx".into());
    let bytes = writer(&doc, &options, &mut context);
    let archive = Archive::open(&bytes, ArchiveLimits::default()).unwrap();
    assert_eq!(archive.entry_names().len(), 16);
    assert!(!archive.contains("word/header1.xml"));
    for (name, exit) in [
        ("missing.docx", 99),
        ("refdir", 99),
        ("/missing.docx", 1),
        ("file://localhost/missing.docx", 1),
        ("file:///bad%00.docx", 99),
        ("file:///bad%2.docx", 99),
        ("https://example.test/missing.docx", 61),
        ("bad.docx", 64),
    ] {
        options.reference_doc = Some(name.into());
        let error = randoc_docx::writer("docx")
            .unwrap()
            .write(&doc, &options, &mut context)
            .unwrap_err();
        assert_eq!(error.exit_code(), exit, "{name}");
        if name == "bad.docx" {
            assert_eq!(
                error.to_string(),
                r#"Could not parse reference-doc "bad.docx": Did not find end of central directory signature"#
            );
        }
    }
    options.reference_doc = None;
    context.state.user_data_dir = Some("unpacked".into());
    context
        .host()
        .write_file("unpacked/docx/word/styles.xml", b"broken")
        .unwrap();
    let error = randoc_docx::writer("docx")
        .unwrap()
        .write(&doc, &options, &mut context)
        .unwrap_err();
    assert_eq!(error.exit_code(), 44);
}

#[test]
fn uses_reference_from_host_and_checks_response_size() {
    const URL: &str = "https://example.test/reference.docx";

    struct WebHost {
        bytes: Vec<u8>,
    }

    impl Host for WebHost {
        fn fetch_url(
            &mut self,
            url: &str,
            headers: &[(String, String)],
        ) -> randoc_core::Result<(Vec<u8>, Option<String>)> {
            assert_eq!(url, URL);
            assert_eq!(
                headers,
                [("Accept".into(), "application/octet-stream".into())]
            );
            Ok((self.bytes.clone(), None))
        }
    }

    let bytes = reference();
    let size = bytes.len() as u64;
    let mut context = Context::new(Box::new(WebHost { bytes }));
    context.state.request_headers = vec![("Accept".into(), "application/octet-stream".into())];
    let mut options = WriterOptions::default();
    options.reference_doc = Some(URL.into());
    options.highlight_style = Some("zenburn".into());
    let doc = Pandoc::default();
    let output = writer(&doc, &options, &mut context);
    let mut archive = Archive::open(&output, ArchiveLimits::default()).unwrap();
    assert!(archive.contains("word/header1.xml"));
    let styles = String::from_utf8(read_part(&mut archive, "word/styles.xml")).unwrap();
    // Pandoc 3.13 keeps the `SourceCode` style of the reference unchanged and
    // adds the token styles of the theme.
    assert!(styles.contains(
        r#"<w:style w:type="paragraph" w:styleId="SourceCode"><w:name w:val="Source Code" /></w:style>"#
    ));
    assert!(styles.contains(r#"w:styleId="KeywordTok"><w:name w:val="KeywordTok" /><w:basedOn w:val="VerbatimChar" /><w:rPr><w:color w:val="f0dfaf" /><w:shd w:val="clear" w:fill="303030" /></w:rPr>"#));

    context.limits.max_resource_size = size - 1;
    let error = randoc_docx::writer("docx")
        .unwrap()
        .write(&doc, &options, &mut context)
        .unwrap_err();
    assert_eq!(error.exit_code(), 61);
    assert!(error.to_string().contains("The response exceeds the limit"));
}

#[derive(Debug, PartialEq, Eq)]
struct Element {
    name: String,
    attrs: BTreeMap<String, String>,
    children: Vec<Element>,
    text: String,
}

fn normalize(element: &XmlElement) -> Element {
    let preserve = matches!(element.name.local.as_str(), "t" | "instrText" | "lpwstr");
    Element {
        name: format!(
            "{}:{}",
            element.name.ns.as_deref().unwrap_or_default(),
            element.name.local
        ),
        attrs: element
            .attrs
            .iter()
            .map(|(name, value)| {
                (
                    format!("{}:{}", name.ns.as_deref().unwrap_or_default(), name.local),
                    value.clone(),
                )
            })
            .collect(),
        children: element.elements().map(normalize).collect(),
        text: element
            .children
            .iter()
            .filter_map(|node| match node {
                XmlNode::Text(text) if preserve || !text.trim().is_empty() => Some(text.as_str()),
                _ => None,
            })
            .collect(),
    }
}

fn part_xml(archive: &mut Archive<'_>, part: &str) -> Element {
    let bytes = read_part(archive, part);
    let xml = parse(
        std::str::from_utf8(&bytes).unwrap(),
        &XmlLimits::default(),
        XmlOptions::default(),
    )
    .unwrap_or_else(|error| panic!("{part}: {error}"));
    normalize(&xml.root)
}

fn diff(actual: &Element, expected: &Element, path: &str) -> Option<String> {
    if actual.name != expected.name
        || actual.attrs != expected.attrs
        || actual.text != expected.text
    {
        return Some(format!(
            "{path}: actual {} {:?} {:?}, expected {} {:?} {:?}",
            actual.name, actual.attrs, actual.text, expected.name, expected.attrs, expected.text
        ));
    }
    if actual.children.len() != expected.children.len() {
        let names = |node: &Element| {
            node.children
                .iter()
                .map(|child| child.name.rsplit(':').next().unwrap())
                .collect::<Vec<_>>()
                .join(", ")
        };
        return Some(format!(
            "{path}: actual [{}], expected [{}]",
            names(actual),
            names(expected)
        ));
    }
    actual
        .children
        .iter()
        .zip(&expected.children)
        .enumerate()
        .find_map(|(index, (a, b))| {
            diff(
                a,
                b,
                &format!("{path}/{}[{index}]", b.name.rsplit(':').next().unwrap()),
            )
        })
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("randoc-docx-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ast(bytes: &[u8]) -> Pandoc {
    json::from_json(std::str::from_utf8(bytes).unwrap()).unwrap()
}

#[test]
#[ignore = "Requires Pandoc 3.13."]
fn same_reference_xml_and_json_round_trip() {
    let scratch = Scratch::new("parity");
    let dir = &scratch.0;
    let reference = reference();
    std::fs::write(dir.join("reference.docx"), &reference).unwrap();
    let mut errors = Vec::new();
    let mut xml_count = 0;
    let mut round_trips = 0;
    let cases = [
        ("empty", String::new()),
        ("paragraphs and links", "# Start {#a-b}\n\n[Visit](u \"Tip\") and [return](#a-b).\n\n> Quoted **text**.\n\nNext.".into()),
        ("metadata", "---\ntitle: A *Title*\nsubtitle: The subtitle\nauthor: [First, Second]\ndate: Today\nabstract: An abstract\nlang: en-US\nsubject: Topic\nkeywords: [a, b]\n---\n\nText.".into()),
        ("lists", "- one\n  - nested\n\n3. ordered\n4. next\n\nTerm\n: Definition\n\n- [x] done".into()),
        ("table", "| Left | Right |\n|:-----|------:|\n| One  | Two   |\n\n: A table".into()),
        ("notes and comments", "Text[^a] with [a note.]{.comment-start id=\"7\" author=\"Alex\" date=\"2020-01-01T00:00:00Z\"}a comment[]{.comment-end id=\"7\"}.\n\n[^a]: A [link](u).".into()),
        ("math", "$a^2$.\n\nBefore $$\\frac{x}{y}$$ after.\n\n$x$".into()),
        ("styles and code", "::: {custom-style=\"Local Style\"}\nA [custom span]{custom-style=\"Accent Text\"}.\n:::\n\n```rust\nlet x = 123;\n```".into()),
        ("image", format!("A ![dot]({GIF} \"Image title\"){{width=24px}} here.")),
        ("figure", format!("![A dot]({GIF} \"Image title\"){{#my-figure}}")),
        ("merged table", r#"<table><caption>Spanning cells</caption><colgroup><col style="width: 25%"><col style="width: 25%"><col style="width: 50%"></colgroup><thead><tr><th colspan="2">Left</th><th>Right</th></tr></thead><tbody><tr><td rowspan="2" colspan="2">Wide</td><td>Top</td></tr><tr><td>Bottom</td></tr></tbody></table>"#.into()),
        ("repeated image", format!("A ![dot]({GIF}) and ![dot]({GIF}).")),
        ("native captions", format!("| Left | Right |\n|:-----|------:|\n| One  | Two   |\n\n: A table\n\n![A dot]({GIF}){{#chart}}\n\n[Table](#table1) and [Figure](#ref_chart)")),
        ("direction and language", "::: {dir=rtl lang=ar}\nHi [arabic text]{lang=ar dir=rtl}.\n:::\n\nA 中文 B".into()),
        ("page break and fields", "# Heading\n\n```{=openxml}\n<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\n```\n".into()),
    ];
    let case_count = cases.len();
    for (label, source) in cases {
        let input = if label == "merged table" {
            "html"
        } else {
            "markdown+task_lists"
        };
        let json = oracle(dir, &["-f", input, "-t", "json"], source.as_bytes());
        let mut args = vec![
            "-f",
            "json",
            "-t",
            "docx",
            "--reference-doc=reference.docx",
            "-o",
            "-",
        ];
        let mut options = WriterOptions::default();
        options.reference_doc = Some("reference.docx".into());
        if label == "page break and fields" {
            args.extend([
                "--toc",
                "--lof",
                "--lot",
                "--number-sections",
                "--toc-depth=2",
            ]);
            options.table_of_contents = true;
            options.list_of_figures = true;
            options.list_of_tables = true;
            options.number_sections = true;
            options.toc_depth = 2;
        }
        if label == "native captions" {
            args[3] = "docx+native_numbering";
            args.extend([
                "--figure-caption-position=above",
                "--table-caption-position=below",
            ]);
            options.extensions.enable(Extension::NativeNumbering);
            options.figure_caption_position = randoc_core::CaptionPosition::CaptionAbove;
            options.table_caption_position = randoc_core::CaptionPosition::CaptionBelow;
        }
        let expected = oracle(dir, &args, &json);
        let host = PureHost {
            files: BTreeMap::from([("reference.docx".into(), reference.clone())]),
            environment: BTreeMap::from([("SOURCE_DATE_EPOCH".into(), EPOCH.into())]),
            ..PureHost::default()
        };
        let actual = writer(&ast(&json), &options, &mut Context::new(Box::new(host)));
        let mut a = Archive::open(&actual, ArchiveLimits::default()).unwrap();
        let mut b = Archive::open(&expected, ArchiveLimits::default()).unwrap();
        for part in [
            "word/document.xml",
            "word/numbering.xml",
            "word/footnotes.xml",
            "word/comments.xml",
            "word/_rels/document.xml.rels",
            "word/_rels/footnotes.xml.rels",
            "docProps/core.xml",
            "docProps/custom.xml",
            "word/header1.xml",
            "word/footer1.xml",
        ] {
            let a = part_xml(&mut a, part);
            let b = part_xml(&mut b, part);
            if let Some(difference) = diff(&a, &b, part) {
                errors.push(format!("{label}: {difference}"));
            } else {
                xml_count += 1;
            }
        }
        let a = oracle(
            dir,
            &["-f", "docx", "-t", "json", "--track-changes=all"],
            &actual,
        );
        let b = oracle(
            dir,
            &["-f", "docx", "-t", "json", "--track-changes=all"],
            &expected,
        );
        if a == b {
            round_trips += 1;
        } else {
            errors.push(format!(
                "{label}: JSON round trip\n{}\n{}",
                String::from_utf8_lossy(&a),
                String::from_utf8_lossy(&b)
            ));
        }
    }
    eprintln!(
        "{xml_count}/{} XML part comparisons, {round_trips}/{case_count} JSON round trips",
        case_count * 10
    );
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
#[ignore = "Requires Pandoc 3.13 and its command test folder."]
fn discovered_command_cases() {
    let suite =
        PathBuf::from(std::env::var_os("RANDOC_PANDOC_SRC").expect("Set RANDOC_PANDOC_SRC."))
            .join("test");
    let mut cases = Vec::new();
    for entry in std::fs::read_dir(suite.join("command")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|value| value.to_str()) != Some("md")
            || path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("._"))
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for (index, line) in text.lines().enumerate() {
            if let Some(command) = line.strip_prefix("% pandoc ")
                && command
                    .split_ascii_whitespace()
                    .collect::<Vec<_>>()
                    .windows(2)
                    .any(|pair| {
                        matches!(pair[0], "-t" | "--to")
                            && (pair[1] == "docx" || pair[1].starts_with("docx+"))
                    })
            {
                let data = text
                    .lines()
                    .skip(index + 1)
                    .take_while(|line| *line != "^D")
                    .collect::<Vec<_>>()
                    .join("\n");
                cases.push((
                    path.file_name().unwrap().to_owned(),
                    index + 1,
                    command.to_owned(),
                    data,
                ));
            }
        }
    }
    cases.sort();
    assert!(!cases.is_empty());
    let mut passed = 0;
    let mut differences = Vec::new();
    for (file, line, command, data) in &cases {
        let (first, second) = command
            .split_once(" | pandoc ")
            .expect("Expected a DOCX round trip.");
        let args: Vec<_> = first.split_ascii_whitespace().collect();
        let target_index = args
            .iter()
            .position(|arg| matches!(*arg, "-t" | "--to"))
            .unwrap()
            + 1;
        let target = args[target_index];
        let mut json_args = args.clone();
        json_args[target_index] = "json";
        let json = oracle(&suite, &json_args, data.as_bytes());
        let mut options = WriterOptions::default();
        if target.starts_with("docx+empty_paragraphs") {
            options.extensions.enable(Extension::EmptyParagraphs);
        }
        let mut context = Context::new(Box::new(NativeHost));
        context.state.resource_path = vec![suite.to_string_lossy().into_owned()];
        let actual = writer(&ast(&json), &options, &mut context);
        let expected = oracle(&suite, &args, data.as_bytes());
        let output: Vec<_> = second.split_ascii_whitespace().collect();
        let original = oracle(&suite, &output, &expected);
        let replacement = oracle(&suite, &output, &actual);
        if replacement == original {
            passed += 1;
        } else {
            differences.push(format!(
                "{file:?}:{line}\nactual: {}\nexpected: {}",
                String::from_utf8_lossy(&replacement),
                String::from_utf8_lossy(&original)
            ));
        }
    }
    eprintln!(
        "{passed}/{} upstream DOCX writer command cases",
        cases.len()
    );
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
#[ignore = "Requires Pandoc 3.13."]
fn repaired_tables_and_figure_layout_match_oracle() {
    use randoc_ast::{
        Alignment, Attr, Caption, Cell, ColSpec, ColWidth, Row, Table, TableBody, TableHead, Target,
    };

    let scratch = Scratch::new("layout");
    let plain = |text: &str| Block::Plain(vec![Inline::Str(text.into())]);
    let cell = |text, cols, rows| Cell {
        col_span: cols,
        row_span: rows,
        content: vec![plain(text)],
        ..Cell::default()
    };
    let row = |cells| Row {
        cells,
        ..Row::default()
    };
    let table = |head, body_head, body, cols| {
        Block::Table(Box::new(Table {
            col_specs: vec![
                ColSpec {
                    align: Alignment::AlignDefault,
                    width: ColWidth::ColWidthDefault
                };
                cols
            ],
            head: TableHead {
                rows: head,
                ..TableHead::default()
            },
            bodies: vec![TableBody {
                head: body_head,
                body,
                ..TableBody::default()
            }],
            ..Table::default()
        }))
    };
    let figure = |blocks| {
        Block::Figure(
            Attr::default(),
            Caption {
                short: None,
                long: vec![plain("Caption")],
            },
            blocks,
        )
    };
    let cases = [
        (
            "extra cells",
            table(
                vec![],
                vec![],
                vec![row(vec![
                    cell("A", 1, 1),
                    cell("B", 1, 1),
                    cell("Discarded", 1, 1),
                ])],
                2,
            ),
        ),
        (
            "wide span",
            table(
                vec![],
                vec![],
                vec![row(vec![cell("Wide", 5, 1), cell("Discarded", 1, 1)])],
                2,
            ),
        ),
        (
            "tall span",
            table(
                vec![],
                vec![],
                vec![
                    row(vec![cell("Tall", 1, 9), cell("B", 1, 1)]),
                    row(vec![cell("C", 1, 1)]),
                ],
                2,
            ),
        ),
        (
            "overlap",
            table(
                vec![],
                vec![],
                vec![
                    row(vec![cell("A", 1, 1), cell("Tall", 1, 2)]),
                    row(vec![
                        cell("Clipped", 2, 1),
                        cell("C", 1, 1),
                        cell("Discarded", 1, 1),
                    ]),
                ],
                3,
            ),
        ),
        (
            "section boundaries",
            table(
                vec![row(vec![cell("Head", 1, 9), cell("B", 1, 1)])],
                vec![row(vec![cell("Intermediate head", 1, 9), cell("C", 1, 1)])],
                vec![row(vec![cell("Body", 1, 1), cell("D", 1, 1)])],
                2,
            ),
        ),
        (
            "missing image",
            figure(vec![Block::Plain(vec![Inline::Image(
                Attr::default(),
                vec![Inline::Str("Description".into())],
                Target {
                    url: "missing-image.png".into(),
                    title: String::new(),
                },
            )])]),
        ),
        (
            "multiple blocks",
            figure(vec![
                plain("Left"),
                Block::Para(vec![Inline::Str("Right".into())]),
            ]),
        ),
    ];
    let mut differences = Vec::new();
    for (label, block) in cases {
        let doc = Pandoc {
            blocks: vec![block],
            ..Pandoc::default()
        };
        let input = json::to_json(&doc);
        let expected = oracle(&scratch.0, &["-f", "json", "-t", "docx"], input.as_bytes());
        let actual = writer(&doc, &WriterOptions::default(), &mut Context::pure());
        let part = |bytes: &[u8]| {
            let mut archive = Archive::open(bytes, ArchiveLimits::default()).unwrap();
            part_xml(&mut archive, "word/document.xml")
        };
        if let Some(difference) = diff(&part(&actual), &part(&expected), "word/document.xml") {
            differences.push(format!("{label}: {difference}"));
        }
    }
    eprintln!(
        "{}/7 repaired table and figure comparisons",
        7 - differences.len()
    );
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

/// Replaces entries of a ZIP archive, keeping the entry order.
fn replace_entries(archive: &[u8], replacements: &[(&str, &str)]) -> Vec<u8> {
    let mut source = Archive::open(archive, ArchiveLimits::default()).unwrap();
    let mut output = ArchiveWriter::new();
    for name in source
        .entry_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>()
    {
        let bytes = replacements
            .iter()
            .find(|(entry, _)| *entry == name)
            .map_or_else(
                || source.read(&name).unwrap().unwrap(),
                |(_, xml)| xml.as_bytes().to_vec(),
            );
        output.write_deflated(&name, &bytes).unwrap();
    }
    output.finish().unwrap()
}

/// Returns each difference between two DOCX archives, entry by entry.
/// Pandoc takes each copied part that `reference` lacks, and each settings
/// element that it lacks, from its own reference document, a GPL data file
/// (I-DATA-01). Only `settings.docx` has each of those settings elements.
fn entry_differences(label: &str, actual: &[u8], expected: &[u8], reference: &[u8]) -> Vec<String> {
    let reference = Archive::open(reference, ArchiveLimits::default()).unwrap();
    let licensed = |name: &str| {
        (name == "word/settings.xml" && !label.starts_with("settings.docx"))
            || ([
                "docProps/app.xml",
                "word/theme/theme1.xml",
                "word/fontTable.xml",
                "word/webSettings.xml",
            ]
            .contains(&name)
                && !reference.contains(name))
    };
    compare_entries(label, actual, expected, licensed)
}

/// Returns each difference between two DOCX archives, entry by entry, for
/// each entry that `skip` does not name.
fn compare_entries(
    label: &str,
    actual: &[u8],
    expected: &[u8],
    skip: impl Fn(&str) -> bool,
) -> Vec<String> {
    let mut a = Archive::open(actual, ArchiveLimits::default()).unwrap();
    let mut b = Archive::open(expected, ArchiveLimits::default()).unwrap();
    let names: Vec<String> = b.entry_names().into_iter().map(str::to_owned).collect();
    let mut differences = Vec::new();
    if a.entry_names() != b.entry_names() {
        differences.push(format!(
            "{label}: entry order\n  randoc {:?}\n  pandoc {:?}",
            a.entry_names(),
            b.entry_names()
        ));
    }
    for name in names {
        if skip(&name) || !a.contains(&name) {
            continue;
        }
        let left = a.read(&name).unwrap().unwrap();
        let right = b.read(&name).unwrap().unwrap();
        if left != right {
            let at = left.iter().zip(&right).take_while(|(x, y)| x == y).count();
            let show = |bytes: &[u8]| {
                String::from_utf8_lossy(&bytes[at.saturating_sub(80)..bytes.len().min(at + 160)])
                    .into_owned()
            };
            differences.push(format!(
                "{label}: {name} at byte {at}\n  randoc {:?}\n  pandoc {:?}",
                show(&left),
                show(&right)
            ));
        }
    }
    differences
}

#[test]
#[ignore = "Requires Pandoc 3.13."]
fn shared_reference_entries_match_oracle() {
    let scratch = Scratch::new("entries");
    let dir = &scratch.0;
    // A project-owned reference document: Randoc's output for an empty document.
    let epoch = PureHost {
        environment: BTreeMap::from([("SOURCE_DATE_EPOCH".into(), EPOCH.into())]),
        ..PureHost::default()
    };
    let base = writer(
        &Pandoc::default(),
        &WriterOptions::default(),
        &mut Context::new(Box::new(epoch)),
    );
    let numbering = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="{W}" xmlns:x="urn:x">
  <w:numPicBullet w:numPicBulletId="0"><w:pict/></w:numPicBullet>
  <w:abstractNum w:abstractNumId="5" x:a="2"><w:nsid w:val="00000005"/></w:abstractNum>
  <w:abstractNum w:abstractNumId="991"><w:nsid w:val="ABCDEF01"/></w:abstractNum>
  <w:num w:numId="7"><w:abstractNumId w:val="5"/></w:num>
  <w:num w:numId="1001"><w:abstractNumId w:val="991"/></w:num>
  <w:numIdMacAtCleanup w:val="3"/>
</w:numbering>"#
    );
    let mut base_archive = Archive::open(&base, ArchiveLimits::default()).unwrap();
    let styles = String::from_utf8(read_part(&mut base_archive, "word/styles.xml"))
        .unwrap()
        .replacen(
            &format!(r#"<w:styles xmlns:w="{W}">"#),
            &format!(
                "<w:styles xmlns:mc=\"urn:mc\" xmlns:w=\"{W}\" mc:Ignorable=\"w14\">\n  <!-- c -->\n  "
            ),
            1,
        )
        .replacen("</w:style><w:style", "</w:style>\n  <w:style", 3);
    let section = "<w:sectPr xmlns:x=\"urn:x\" w:rsidR=\"00\"><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\n      <w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/><w:footnotePr>\n        <w:numRestart w:val=\"eachSect\"/>\n      </w:footnotePr><w:cols w:space=\"720\">t&apos;x</w:cols></w:sectPr>";
    let document = String::from_utf8(read_part(&mut base_archive, "word/document.xml")).unwrap();
    let start = document.find("<w:sectPr").unwrap();
    let end = document.find("</w:sectPr>").unwrap() + "</w:sectPr>".len();
    let document = format!("{}{section}{}", &document[..start], &document[end..]);
    // A reference without the token styles of highlighting.
    let plain_styles = {
        let styles = String::from_utf8(read_part(&mut base_archive, "word/styles.xml")).unwrap();
        let tokens = styles
            .find(r#"<w:style w:type="character" w:customStyle="1" w:styleId="KeywordTok">"#)
            .unwrap();
        format!("{}</w:styles>", &styles[..tokens])
    };
    // Project-owned values for each element of the settings of Pandoc's own
    // reference document, and one element that Pandoc drops.
    let settings = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<w:settings xmlns:w="{W}"><w:listSeparator w:val=";"/><w:zoom w:percent="90"/><w:embedSystemFonts w:val="0"/><w:proofState w:spelling="dirty" w:grammar="dirty"/><w:stylePaneFormatFilter w:val="0001"/><w:doNotTrackMoves w:val="0"/><w:defaultTabStop w:val="700"/><w:drawingGridHorizontalSpacing w:val="200"/><w:drawingGridVerticalSpacing w:val="200"/><w:displayHorizontalDrawingGridEvery w:val="2"/><w:displayVerticalDrawingGridEvery w:val="2"/><w:characterSpacingControl w:val="compressPunctuation"/><w:savePreviewPicture w:val="0"/><w:rsids><w:rsidRoot w:val="00A1B2C3"/></w:rsids><w:themeFontLang w:val="fr-FR"/><w:clrSchemeMapping w:bg1="light1" w:t1="dark1"/><w:decimalSymbol w:val=","/><w:footnotePr><w:numFmt w:val="lowerRoman"/></w:footnotePr><w:evenAndOddHeaders/></w:settings>"#
    );
    let references = [
        ("base.docx", base.clone()),
        (
            "settings.docx",
            replace_entries(&base, &[("word/settings.xml", &settings)]),
        ),
        (
            "plain.docx",
            replace_entries(&base, &[("word/styles.xml", &plain_styles)]),
        ),
        (
            "numbering.docx",
            replace_entries(&base, &[("word/numbering.xml", &numbering)]),
        ),
        ("headers.docx", reference()),
        (
            "layout.docx",
            replace_entries(
                &base,
                &[
                    ("word/styles.xml", &styles),
                    ("word/document.xml", &document),
                ],
            ),
        ),
    ];
    for (name, bytes) in &references {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    let cases: &[(&str, String, &[&str])] = &[
        ("core", format!("---\ntitle: The Title\nauthor: Ann Author\ndate: 2024-01-02\n---\n\n# Heading one\n\nSome *emphasis*, **strong**, `code`, a [link](https://example.com) and a note.[^1]\n\n## Heading two\n\n- item one\n- item two\n\n1. first\n2. second\n\n> A quote.\n\n```\nprint(\"hi\")\n```\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n![An image]({GIF})\n\n$x^2$\n\n[^1]: The note."), &[]),
        ("runs", "Lead a b \"quoted *text*\" 'single' a\nsoft  break.\n\nA 中文 B and A中 B.\n\n `code`, a [x y](u) z.".into(), &[]),
        ("blocks", format!("---\ntitle: T\nabstract: |\n  Abs one.\n\n  Abs two.\n---\n\n::: {{#d1}}\nIn div.\n:::\n\n---\n\n```{{=openxml}}\n<w:p><w:r><w:t>raw1</w:t></w:r></w:p><w:p><w:r><w:t>raw2</w:t></w:r></w:p>\n```\n\nTerm\n: Def one.\n\n    Def two.\n\n- a\n    - b\n\n> q1\n>\n> q2\n\n| x\n| y\n\nNote.[^1]\n\n+---+---+\n| a | b |\n|   |   |\n| c |   |\n+---+---+\n\n: Cap {{#tbl}}\n\n![Fig]({GIF}){{#fig}}\n\n[^1]: N1.\n\n    N2."), &["--toc"]),
        ("numbering", "1. one\n2. two\n\n(a) alpha\n(b) beta\n\niv) roman\nv) next\n\n- [ ] task\n- [x] done\n\n(@) example\n(@) again\n\n3. three\n    - inner".into(), &[]),
        ("escaping", "---\ntitle: \"T ' \\\" < &\"\n---\n\nit's \"q\" <a> & b\n\n![alt ' \" < &](GIFSRC \"ti'tle\")".replace("GIFSRC", GIF), &[]),
        ("highlight", "```rust\nlet x = 1; // c\n```\n\nText.".into(), &[]),
        ("zenburn", "```rust\nfn f() {}\n```".into(), &["--syntax-highlighting=zenburn"]),
        ("custom styles", "::: {custom-style=\"Local Style\"}\nA [span]{custom-style=\"Accent Text\"}.\n:::".into(), &[]),
        ("comments", "Text [note]{.comment-start id=\"7\" author=\"Alex\" date=\"2020-01-01T00:00:00Z\"}a comment[]{.comment-end id=\"7\"} end.".into(), &[]),
    ];
    let mut differences = Vec::new();
    let mut matched = 0;
    let mut total = 0;
    for (reference, _) in &references {
        for (label, source, extra) in cases {
            let label = format!("{reference}/{label}");
            let json = oracle(dir, &["-f", "markdown", "-t", "json"], source.as_bytes());
            let reference_arg = format!("--reference-doc={reference}");
            let mut args = vec!["-f", "json", "-t", "docx", &reference_arg, "-o", "-"];
            args.extend(extra.iter());
            let expected = oracle(dir, &args, &json);
            let mut options = WriterOptions::default();
            options.reference_doc = Some((*reference).into());
            options.table_of_contents = extra.contains(&"--toc");
            if extra.contains(&"--syntax-highlighting=zenburn") {
                options.highlight_style = Some("zenburn".into());
            }
            let host = PureHost {
                files: references
                    .iter()
                    .map(|(name, bytes)| ((*name).into(), bytes.clone()))
                    .collect(),
                environment: BTreeMap::from([("SOURCE_DATE_EPOCH".into(), EPOCH.into())]),
                ..PureHost::default()
            };
            let actual = writer(&ast(&json), &options, &mut Context::new(Box::new(host)));
            if let Some(dump) = std::env::var_os("RANDOC_DOCX_DUMP") {
                let stem = label.replace(['/', '.'], "-");
                std::fs::write(
                    Path::new(&dump).join(format!("randoc-{stem}.docx")),
                    &actual,
                )
                .unwrap();
                std::fs::write(
                    Path::new(&dump).join(format!("oracle-{stem}.docx")),
                    &expected,
                )
                .unwrap();
            }
            let reference_bytes = &references
                .iter()
                .find(|(name, _)| name == reference)
                .unwrap()
                .1;
            let found = entry_differences(&label, &actual, &expected, reference_bytes);
            total += 1;
            if found.is_empty() {
                matched += 1;
            }
            differences.extend(found);
        }
    }
    eprintln!("{matched}/{total} DOCX archives match entry by entry");
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

/// The DOCX units of the writer baseline of the conformance harness: each
/// archive must match the oracle entry by entry, `word/settings.xml` included.
#[test]
#[ignore = "Requires Pandoc 3.13."]
fn writer_baseline_harness_units_match_oracle() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cases =
        std::fs::read_to_string(root.join("conformance/cases/writer-baseline/docx.toml")).unwrap();
    let reference =
        std::fs::read(root.join("conformance/fixtures/writer-baseline/reference.docx")).unwrap();
    let scratch = Scratch::new("baseline");
    std::fs::write(scratch.0.join("reference.docx"), &reference).unwrap();
    let ids = cases
        .lines()
        .filter_map(|line| line.strip_prefix("id = \""))
        .map(|id| id.trim_end_matches('"'));
    let inputs = cases
        .split("input = '''\n")
        .skip(1)
        .map(|rest| rest.split("'''").next().unwrap());
    let mut differences = Vec::new();
    let mut total = 0;
    for (id, input) in ids.zip(inputs) {
        total += 1;
        let json = oracle(
            &scratch.0,
            &["-f", "native", "-t", "json"],
            input.as_bytes(),
        );
        let expected = oracle(
            &scratch.0,
            &[
                "-f",
                "json",
                "-t",
                "docx",
                "--reference-doc=reference.docx",
                "--syntax-highlighting=none",
                "-o",
                "-",
            ],
            &json,
        );
        let mut options = WriterOptions::default();
        options.reference_doc = Some("reference.docx".into());
        options.highlight_method = HighlightMethod::None;
        options.highlight_style = None;
        let host = PureHost {
            files: BTreeMap::from([("reference.docx".into(), reference.clone())]),
            environment: BTreeMap::from([("SOURCE_DATE_EPOCH".into(), EPOCH.into())]),
            ..PureHost::default()
        };
        let actual = writer(&ast(&json), &options, &mut Context::new(Box::new(host)));
        differences.extend(compare_entries(id, &actual, &expected, |_| false));
    }
    assert_eq!(total, 8);
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}
