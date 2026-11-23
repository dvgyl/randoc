//! Differential tests for Pandoc 3.13 document conversion.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use randoc_archive::zip::{Archive, ArchiveLimits};
use randoc_ast::{Pandoc, json};
use randoc_core::{
    CaptionPosition, Context, FormatSpec, HighlightMethod, Reader, ReaderOptions, Sources,
    TopLevelDivision, WrapOption, WriterOptions,
};
use randoc_tail_writers::{reader, writer};
use randoc_templates::{Template, Val};

struct Oracle {
    pandoc: PathBuf,
    suite: PathBuf,
}

impl Oracle {
    fn new() -> Self {
        let pandoc =
            PathBuf::from(std::env::var_os("RANDOC_ORACLE_PANDOC").expect("Source env/env.sh."));
        let suite =
            PathBuf::from(std::env::var_os("RANDOC_PANDOC_SRC").expect("Source env/env.sh."))
                .join("test");
        assert_eq!(
            run(&pandoc, &suite, &["--version"], b"")
                .stdout
                .split(|byte| *byte == b'\n')
                .next(),
            Some(b"pandoc 3.13".as_slice())
        );
        Self { pandoc, suite }
    }

    fn run(&self, args: &[&str], input: &[u8]) -> Output {
        run(&self.pandoc, &self.suite, args, input)
    }

    fn ast(&self, format: &str, input: &[u8], standalone: bool) -> Pandoc {
        let mut args = vec!["-f", format, "-t", "json"];
        if standalone {
            args.push("-s");
        }
        let output = self.run(&args, input);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        json::from_json(std::str::from_utf8(&output.stdout).unwrap()).unwrap()
    }

    fn file(&self, name: &str) -> Vec<u8> {
        assert!(!name.ends_with(".hs"));
        std::fs::read(self.suite.join(name)).unwrap()
    }
}

fn run(program: &Path, dir: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut task = Command::new(program)
        .args(args)
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", "1700000000")
        .env("TZ", "UTC")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    task.stdin.take().unwrap().write_all(input).unwrap();
    task.wait_with_output().unwrap()
}

fn load_template(format: &str) -> Template {
    let source = randoc_data::default_template(format).unwrap_or("");
    Template::compile(format, source, 128, &mut |name: &str| {
        randoc_data::data_file(&format!("templates/{name}"))
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .map(str::to_owned)
            .ok_or(())
    })
    .unwrap()
}

fn write_format(
    oracle: &Oracle,
    format: &str,
    doc: &Pandoc,
    options: &WriterOptions,
) -> Result<Vec<u8>, String> {
    let mut context = Context::native();
    context.state.resource_path = vec![oracle.suite.display().to_string()];
    let output = writer(format)
        .unwrap()
        .write(doc, options, &mut context)
        .map_err(|e| e.to_string())?;
    let mut bytes = output.into_bytes();
    if format != "chunkedhtml" && !bytes.is_empty() && !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    Ok(bytes)
}

fn first_difference(actual: &[u8], expected: &[u8]) -> Option<String> {
    if actual == expected {
        return None;
    }
    let expected = String::from_utf8_lossy(expected);
    let actual = String::from_utf8_lossy(actual);
    let mut actual_lines = actual.split('\n');
    let mut expected_lines = expected.split('\n');
    let mut line = 1usize;
    loop {
        let a = actual_lines.next();
        let e = expected_lines.next();
        if a != e {
            let shorten =
                |text: Option<&str>| text.map(|line| line.chars().take(220).collect::<String>());
            return Some(format!(
                "line {line}: expected {:?}, got {:?}",
                shorten(e),
                shorten(a)
            ));
        }
        if a.is_none() {
            return Some("different output bytes".into());
        }
        line += 1;
    }
}

#[derive(Default)]
struct Counts {
    by_format: BTreeMap<String, (usize, usize)>,
    failures: Vec<String>,
}

impl Counts {
    fn check(&mut self, format: &str, name: &str, difference: Option<String>) {
        let counts = self.by_format.entry(format.into()).or_default();
        counts.1 += 1;
        if let Some(difference) = difference {
            self.failures.push(format!("{format} {name}: {difference}"));
        } else {
            counts.0 += 1;
        }
    }

    fn compare(
        &mut self,
        format: &str,
        name: &str,
        actual: Result<Vec<u8>, String>,
        expected: &[u8],
    ) {
        self.check(
            format,
            name,
            match actual {
                Ok(actual) => first_difference(&actual, expected),
                Err(error) => Some(error),
            },
        );
    }

    fn finish(self) {
        for (format, (passed, total)) in &self.by_format {
            println!("{format}: {passed}/{total}");
        }
        for failure in &self.failures {
            println!("{failure}");
        }
        assert!(
            self.failures.is_empty(),
            "{} oracle comparisons failed.",
            self.failures.len()
        );
    }
}

fn old_cases(oracle: &Oracle, counts: &mut Counts) {
    for format in ["bbcode", "context", "icml", "texinfo"] {
        for (name, input, standalone) in [
            (format!("writer.{format}"), "testsuite.native", true),
            (format!("tables.{format}"), "tables.native", false),
        ] {
            let doc = oracle.ast("native", &oracle.file(input), standalone);
            let mut options = WriterOptions::default();
            options.columns = 80;
            options.template = standalone.then(|| load_template(format));
            options.variables.insert("pandoc-version", Val::text(""));
            counts.compare(
                format,
                &name,
                write_format(oracle, format, &doc, &options),
                &oracle.file(&name),
            );
        }
    }
    let doc = oracle.ast("markdown", &oracle.file("ansi-test.txt"), false);
    counts.compare(
        "ansi",
        "ansi-test.ansi",
        write_format(oracle, "ansi", &doc, &WriterOptions::default()),
        &oracle.file("ansi-test.ansi"),
    );
    let doc = oracle.ast("native", &oracle.file("writers-lang-and-dir.native"), true);
    let mut options = WriterOptions::default();
    options.template = Some(load_template("context"));
    counts.compare(
        "context",
        "writers-lang-and-dir.context",
        write_format(oracle, "context", &doc, &options),
        &oracle.file("writers-lang-and-dir.context"),
    );
}

fn command_cases(oracle: &Oracle, counts: &mut Counts) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = r#"
import json, shlex, sys
sys.path.insert(0, sys.argv[1])
from suite import Discovery, discover_commands
discovery = Discovery()
discover_commands(sys.argv[2], discovery)
json.dump([(case.id, shlex.split(case.command), case.stdin)
           for case in discovery.cases], sys.stdout)
"#;
    let output = run(
        Path::new("python3"),
        &root,
        &[
            "-B",
            "-c",
            script,
            root.join("tools/pandoc-testsuite").to_str().unwrap(),
            oracle.suite.to_str().unwrap(),
        ],
        b"",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cases: Vec<(String, Vec<String>, String)> = serde_json::from_slice(&output.stdout).unwrap();
    for (name, argv, input) in cases {
        let mut from = "markdown";
        let mut to = "";
        let mut options = WriterOptions::default();
        let mut standalone = false;
        let mut args = argv.iter().skip(1);
        let mut unknown = Vec::new();
        while let Some(arg) = args.next() {
            let (key, attached) = arg
                .split_once('=')
                .map_or((arg.as_str(), None), |(key, value)| (key, Some(value)));
            let mut value = || {
                attached
                    .or_else(|| args.next().map(String::as_str))
                    .unwrap_or_default()
            };
            match key {
                "-f" | "-r" | "--from" | "--read" => from = value(),
                "-t" | "-w" | "--to" | "--write" => to = value(),
                "-s" | "--standalone" => standalone = true,
                "--columns" => options.columns = value().parse().unwrap(),
                "--wrap" => {
                    options.wrap_text = match value() {
                        "none" => WrapOption::WrapNone,
                        "preserve" => WrapOption::WrapPreserve,
                        _ => WrapOption::WrapAuto,
                    }
                }
                "--top-level-division" => {
                    options.top_level_division = match value() {
                        "chapter" => TopLevelDivision::TopLevelChapter,
                        "section" => TopLevelDivision::TopLevelSection,
                        "part" => TopLevelDivision::TopLevelPart,
                        _ => TopLevelDivision::TopLevelDefault,
                    }
                }
                "--figure-caption-position" => options.figure_caption_position = caption(value()),
                "--table-caption-position" => options.table_caption_position = caption(value()),
                "--no-highlight" => options.highlight_method = HighlightMethod::None,
                "--toc" | "--table-of-contents" => options.table_of_contents = true,
                "-N" | "--number-sections" => options.number_sections = true,
                "--dpi" => options.dpi = value().parse().unwrap(),
                "--quiet" => {}
                _ => unknown.push(arg.as_str()),
            }
        }
        let spec = FormatSpec::parse(to);
        if writer(&spec.name).is_none() || spec.name == "chunkedhtml" {
            continue;
        }
        let format = spec.name;
        if !unknown.is_empty() {
            counts.check(
                &format,
                &name,
                Some(format!("unhandled arguments {unknown:?}")),
            );
            continue;
        }
        options.extensions = FormatSpec::parse(to).extensions().unwrap();
        options.template = standalone.then(|| load_template(&format));
        let doc = oracle.ast(from, input.as_bytes(), standalone);
        let argv: Vec<_> = argv.iter().skip(1).map(String::as_str).collect();
        let expected = oracle.run(&argv, input.as_bytes());
        assert!(
            expected.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        counts.compare(
            &format,
            &name,
            write_format(oracle, &format, &doc, &options),
            &expected.stdout,
        );
    }
}

fn caption(value: &str) -> CaptionPosition {
    if value == "above" {
        CaptionPosition::CaptionAbove
    } else {
        CaptionPosition::CaptionBelow
    }
}

fn original_cases(oracle: &Oracle, counts: &mut Counts) {
    let markdown = b"# Example\n\n**Strong**, *italic*, and `code`.\n";
    let doc = oracle.ast("markdown", markdown, false);
    for format in [
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
    ] {
        let reference = oracle.run(&["-t", format], markdown);
        assert!(reference.status.success());
        counts.compare(
            format,
            "original format sample",
            write_format(oracle, format, &doc, &WriterOptions::default()),
            &reference.stdout,
        );
    }

    let source = b"---\ntitle: Guide\n---\n\nBefore.\n\n# One\n\n[Two](#two).\n\n# Two\n\nH~2~O.\n";
    let doc = oracle.ast("markdown", source, true);
    let reference = oracle.run(&["-t", "xml", "-s"], source);
    let mut options = WriterOptions::default();
    options.template = Some(load_template("xml"));
    counts.compare(
        "xml",
        "standalone",
        write_format(oracle, "xml", &doc, &options),
        &reference.stdout,
    );
    let Some(Reader::Text(reader)) = reader("xml") else {
        panic!("XML reader registration")
    };
    let text = String::from_utf8(reference.stdout.clone()).unwrap();
    let actual = reader
        .read(
            &Sources::from_text(text),
            &ReaderOptions::default(),
            &mut Context::pure(),
        )
        .unwrap();
    let expected = oracle.ast("xml", &reference.stdout, true);
    counts.check(
        "xml",
        "round trip",
        (actual != expected).then(|| "different abstract syntax trees".into()),
    );

    let reference = oracle.run(&["-t", "chunkedhtml"], source);
    compare_zip(
        oracle,
        counts,
        "section links",
        reference,
        &doc,
        &WriterOptions::default(),
    );
}

fn compare_zip(
    oracle: &Oracle,
    counts: &mut Counts,
    name: &str,
    reference: Output,
    doc: &Pandoc,
    options: &WriterOptions,
) {
    assert!(reference.status.success());
    let actual = write_format(oracle, "chunkedhtml", doc, options);
    let difference = match actual {
        Ok(bytes) => {
            let mut a =
                Archive::open(&bytes, ArchiveLimits::from_limits(&Context::pure().limits)).unwrap();
            let mut e = Archive::open(
                &reference.stdout,
                ArchiveLimits::from_limits(&Context::pure().limits),
            )
            .unwrap();
            if a.entry_names() != e.entry_names() {
                Some(format!(
                    "expected ZIP entries {:?}, got {:?}",
                    e.entry_names(),
                    a.entry_names()
                ))
            } else {
                let names: Vec<_> = a.entry_names().into_iter().map(str::to_owned).collect();
                for entry in names {
                    let actual = a.read(&entry).unwrap().unwrap();
                    let expected = e.read(&entry).unwrap().unwrap();
                    counts.compare(
                        "chunkedhtml",
                        &format!("{name}/{entry}"),
                        Ok(actual),
                        &expected,
                    );
                }
                None
            }
        }
        Err(error) => Some(error),
    };
    counts.check("chunkedhtml", name, difference);
}

fn file_cases(oracle: &Oracle, counts: &mut Counts) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let input = std::fs::read(root.join("conformance/fixtures/files/sample-cell.json")).unwrap();
    let doc = json::from_json(std::str::from_utf8(&input).unwrap()).unwrap();
    let options = WriterOptions::default();
    for format in [
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
        "chunkedhtml",
    ] {
        let reference = oracle.run(&["-f", "json", "-t", format], &input);
        assert!(reference.status.success());
        if format == "chunkedhtml" {
            compare_zip(
                oracle,
                counts,
                "file fixture entries",
                reference,
                &doc,
                &options,
            );
        } else {
            counts.compare(
                format,
                "file fixture",
                write_format(oracle, format, &doc, &options),
                &reference.stdout,
            );
        }
    }

    let input = b"# Start\n\n![Alt](lalune.jpg){width=40%}\n\nH~2~O, `code`, ~~strike~~.\n\n- Item\n\n---\n\n| A | B |\n|-|-|\n| C | D |\n";
    let doc = oracle.ast("markdown", input, false);
    for format in [
        "bbcode",
        "bbcode_fluxbb",
        "bbcode_hubzilla",
        "bbcode_phpbb",
        "bbcode_steam",
        "bbcode_xenforo",
    ] {
        let reference = oracle.run(&["-t", format, "-s"], input);
        let mut options = WriterOptions::default();
        options.template = Some(load_template(format));
        counts.compare(
            format,
            "dialect features",
            write_format(oracle, format, &doc, &options),
            &reference.stdout,
        );
    }
}

fn review_cases(oracle: &Oracle, counts: &mut Counts) {
    for (name, from, to, standalone, input) in [
        (
            "Str content and literal whitespace",
            "native",
            "xml",
            false,
            r#"[Plain [Str "a b"],Plain [Str ""],Plain [Str "a\tb"],Plain [Str "a\160b"],Plain [Str "a\nb"]]"#,
        ),
        (
            "attribute order",
            "native",
            "xml",
            false,
            r#"[Header 7 ("h",["c"],[("k","v")]) [],Plain [Link ("i",["c"],[("k","v")]) [Str "x"] ("u","t"),Image ("i",["c"],[("k","v")]) [] ("s","t")]]"#,
        ),
        (
            "empty literals",
            "native",
            "xml",
            true,
            r#"Pandoc (Meta {unMeta = fromList [("x",MetaString "")]}) [CodeBlock ("i",["py"],[("k","v")]) "",RawBlock (Format "html") "",Plain [],Plain [Code ("",[],[]) "",Math InlineMath "",RawInline (Format "html") "",Note [],Span ("",[],[]) []],BulletList [[]],Figure ("",[],[]) (Caption Nothing []) []]"#,
        ),
        (
            "URL escaping and labels",
            "markdown",
            "context",
            false,
            "[a](http://x.com/a%20b#frag) <http://y.com/a#b> <http://y.com/p%41#q>",
        ),
        (
            "URL punctuation",
            "native",
            "context",
            false,
            r##"[Plain [Link ("",[],[]) [Str "x"] ("https://x/a%#&{}_~^$[]()|<>",""),Link ("",[],[]) [Str "x"] ("#a%20b","")]]"##,
        ),
        (
            "inline code delimiters",
            "native",
            "context",
            false,
            r#"[Plain [Code ("",[],[]) "{x}",Space,Code ("",[],[]) "{\"x}",Space,Code ("",[],[]) "{x}\"'",Space,Code ("",[],[]) "{x}\"'`(",Space,Code ("",[],[]) "{x}\"'`()-%+=,.:;"]]"#,
        ),
        (
            "image dimensions",
            "markdown",
            "context",
            false,
            "![x](a.png){width=50px} ![x](a.png){width=40px height=3in} ![x](a.png){width=33.3% height=39.123456%} ![x](a.png){width=1.234567cm}",
        ),
        (
            "table column fractions",
            "native",
            "texinfo",
            false,
            r#"[Table ("",[],[]) (Caption Nothing []) [(AlignDefault,ColWidth 0.333),(AlignDefault,ColWidth 0.5)] (TableHead ("",[],[]) []) [TableBody ("",[],[]) (RowHeadColumns 0) [] [Row ("",[],[]) [Cell ("",[],[]) AlignDefault (RowSpan 1) (ColSpan 1) [Plain [Str "a"]], Cell ("",[],[]) AlignDefault (RowSpan 1) (ColSpan 1) [Plain [Str "b"]]]]] (TableFoot ("",[],[]) [])]"#,
        ),
        (
            "deep headings",
            "markdown",
            "texinfo",
            false,
            "# C\n\n##### Deep\n\n# Deep\n\n[x](#deep) [y](#deep-1)\n",
        ),
        (
            "Hubzilla definitions and quotes",
            "markdown",
            "bbcode_hubzilla",
            false,
            "Term\n\n:   def1\n\n    def1b\n\n:   def2\n\nSecond\n\n:   def3\n\n![Alt \"q\"](a.png \"T\")",
        ),
    ] {
        let input = input.as_bytes();
        let doc = oracle.ast(from, input, standalone);
        let mut args = vec!["-f", from, "-t", to];
        let mut options = WriterOptions::default();
        if standalone {
            args.push("-s");
            options.template = Some(load_template(to));
        }
        let expected = oracle.run(&args, input);
        assert!(expected.status.success(), "{name}: {:?}", expected.stderr);
        counts.compare(
            to,
            name,
            write_format(oracle, to, &doc, &options),
            &expected.stdout,
        );
    }

    let Some(Reader::Text(xml)) = reader("xml") else {
        panic!("XML reader registration")
    };
    for (name, input) in [
        (
            "explicit Str content",
            "<Plain><Str content=\"a b\" />x<Str>y z</Str><Str/></Plain>",
        ),
        (
            "Unicode spaces",
            "<Plain>a&#160;b&#8195;c a&#9;b&#13;c&#10;d</Plain>&#160;<Plain>&#160;</Plain>",
        ),
    ] {
        let actual = xml
            .read(
                &Sources::from_text(input),
                &ReaderOptions::default(),
                &mut Context::pure(),
            )
            .unwrap();
        let expected = oracle.ast("xml", input.as_bytes(), true);
        counts.check(
            "xml",
            name,
            (actual != expected).then(|| "different abstract syntax trees".into()),
        );
    }
    for input in ["<Para>x<Foo/></Para>", "junk<Para>x</Para>", "<Foo/>"] {
        let error = xml
            .read(
                &Sources::from_text(input),
                &ReaderOptions::default(),
                &mut Context::pure(),
            )
            .unwrap_err();
        let expected = oracle.run(&["-f", "xml", "-t", "native"], input.as_bytes());
        assert_eq!(Some(error.exit_code()), expected.status.code());
        counts.compare(
            "xml",
            input,
            Ok(format!("{error}\n").into_bytes()),
            &expected.stderr,
        );
    }

    let source = b"# A\n\n![s](/etc/hosts)\n\n![r](http://127.0.0.1:9/x.png)\n\n![p](sub/../../nothere.gif)\n\n![d](data:image/png;base64,iVBORw0KGgo=)\n";
    let doc = oracle.ast("markdown", source, true);
    compare_zip(
        oracle,
        counts,
        "unsafe image targets",
        oracle.run(&["-t", "chunkedhtml"], source),
        &doc,
        &WriterOptions::default(),
    );
}

#[test]
#[ignore = "requires the Pandoc 3.13 executable"]
fn chunked_html_default_template_and_table_of_contents_match_pandoc() {
    let oracle = Oracle::new();
    let mut counts = Counts::default();
    let input = br#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}"#;
    for (name, metadata, variables, toc) in [
        ("empty", &[][..], &[][..], false),
        (
            "title",
            &[
                "title=A Title",
                "author=Ann Author",
                "author=Bob Writer",
                "date=2024-01-02",
            ][..],
            &[][..],
            false,
        ),
        (
            "many",
            &[
                "title=A Title",
                "subtitle=Sub",
                "author=Ann Author",
                "date=2024-01-02",
                "abstract=The abstract.",
                "keywords=k1",
                "keywords=k2",
                "lang=en-US",
                "institute=Inst",
                "subject=Subj",
                "description=Desc",
            ][..],
            &[
                ("header-includes", "<!-- h1 -->"),
                ("include-before", "Before."),
                ("include-after", "After."),
                ("css", "a.css"),
                ("fontsize", "11pt"),
                ("geometry", "margin=1in"),
                ("classoption", "twocolumn"),
                ("mainfont", "Serif"),
                ("linkcolor", "blue"),
                ("papersize", "a4"),
            ][..],
            true,
        ),
    ] {
        let mut args = vec!["-f", "json", "-t", "json", "-s"];
        for value in metadata {
            args.extend(["-M", value]);
        }
        let output = oracle.run(&args, input);
        assert!(output.status.success());
        let doc = json::from_json(std::str::from_utf8(&output.stdout).unwrap()).unwrap();

        args[3] = "chunkedhtml";
        let mut options = WriterOptions::default();
        options.table_of_contents = toc;
        options.number_sections = toc;
        if toc {
            args.extend(["--toc", "-N"]);
        }
        let variable_args: Vec<_> = variables
            .iter()
            .map(|&(key, value)| {
                options.variables.insert(key, Val::text(value));
                format!("{key}={value}")
            })
            .collect();
        for value in &variable_args {
            args.extend(["-V", value]);
        }
        compare_zip(
            &oracle,
            &mut counts,
            name,
            oracle.run(&args, input),
            &doc,
            &options,
        );
    }

    for (name, source, toc) in [
        ("body with TOC", b"Body.\n".as_slice(), true),
        ("headings with TOC", b"# A\n\nBody.\n\n## B\n\n# C\n", true),
        (
            "headings without TOC",
            b"# A\n\nBody.\n\n## B\n\n# C\n",
            false,
        ),
    ] {
        let doc = oracle.ast("markdown", source, true);
        let mut args = vec!["-f", "markdown", "-t", "chunkedhtml"];
        if toc {
            args.push("--toc");
        }
        let mut options = WriterOptions::default();
        options.table_of_contents = toc;
        compare_zip(
            &oracle,
            &mut counts,
            name,
            oracle.run(&args, source),
            &doc,
            &options,
        );
    }
    counts.finish();
}

#[test]
#[ignore = "requires the Pandoc 3.13 executable"]
fn ansi_standalone_title_block_matches_pandoc() {
    let oracle = Oracle::new();
    let input = b"% A Title\n% Ann Author; Bob Writer\n% 2024-01-02\n\nBody.\n";
    let doc = oracle.ast("markdown", input, true);
    let template = load_template("ansi");
    for (wrap, wrap_text) in [
        ("auto", WrapOption::WrapAuto),
        ("none", WrapOption::WrapNone),
    ] {
        let mut options = WriterOptions::default();
        options.template = Some(template.clone());
        options.columns = 24;
        options.wrap_text = wrap_text;
        let expected = oracle.run(
            &[
                "-f",
                "markdown",
                "-t",
                "ansi",
                "-s",
                "--columns",
                "24",
                "--wrap",
                wrap,
            ],
            input,
        );
        assert!(expected.status.success(), "{:?}", expected.stderr);
        let actual = writer("ansi")
            .unwrap()
            .write(&doc, &options, &mut Context::native())
            .unwrap()
            .into_bytes();
        assert_eq!(first_difference(&actual, &expected.stdout), None, "{wrap}");
    }
}

#[test]
#[ignore = "requires the Pandoc 3.13 executable and non-Haskell fixtures"]
fn pandoc_3_13_parity() {
    let oracle = Oracle::new();
    let mut counts = Counts::default();
    old_cases(&oracle, &mut counts);
    command_cases(&oracle, &mut counts);
    original_cases(&oracle, &mut counts);
    file_cases(&oracle, &mut counts);
    review_cases(&oracle, &mut counts);
    counts.finish();
}
