#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use randoc_ast::{
    Attr, Block, Format, Inline, MetaValue, Pandoc, Target,
    json::{JsonKind, parse_value},
};
use randoc_core::{
    Context, Error, IpynbOutput, ReaderOptions, Sources, WrapOption, WriterOptions,
    default_extensions,
};
use serde_json::Value;

fn read(source: &str, context: &mut Context) -> randoc_core::Result<Pandoc> {
    let mut options = ReaderOptions::default();
    options.extensions = default_extensions("ipynb");
    super::reader::read(&Sources::from_text(source), &options, context)
}

fn write(doc: &Pandoc, context: &mut Context) -> randoc_core::Result<String> {
    let mut options = WriterOptions::default();
    options.extensions = default_extensions("ipynb");
    options.wrap_text = WrapOption::WrapNone;
    super::writer::write(doc, &options, context)
}

#[test]
fn exact_numbers_in_metadata_and_outputs() {
    // Pandoc 3.13 gives these number strings for the same JSON tokens.
    let expanded_400 = format!("1{}", "0".repeat(400));
    let expanded_1024 = format!("1{}", "0".repeat(1024));
    let expanded_1025 = format!("1{}", "0".repeat(1025));
    let cases = [
        ("0", "0", "0", "0"),
        ("0.0", "0", "0.0", "0.0"),
        ("-0", "0", "0", "0"),
        ("-0.0", "0", "0.0", "0.0"),
        ("0e10", "0", "0", "0"),
        ("0e1025", "0", "0.0", "0"),
        ("0e-400", "0", "0.0", "0.0"),
        ("1", "1", "1", "1"),
        ("1.0", "1", "1.0", "1.0"),
        ("2e0", "2", "2", "2"),
        ("1500", "1500", "1500", "1500"),
        ("1500.0", "1500", "1500.0", "1500.0"),
        ("10.50e1", "105", "105.0", "105.0"),
        ("1e6", "1000000", "1000000", "1000000"),
        ("1000000.0", "1000000", "1000000.0", "1000000.0"),
        ("10000000.0", "10000000", "1.0e7", "1.0e7"),
        ("1e7", "10000000", "10000000", "10000000"),
        ("1.0e7", "10000000", "10000000", "10000000"),
        ("1e8", "100000000", "100000000", "100000000"),
        ("1e10", "10000000000", "10000000000", "10000000000"),
        ("1.0e10", "10000000000", "10000000000", "10000000000"),
        ("0.1", "0.1", "0.1", "0.1"),
        ("0.01", "1.0e-2", "1.0e-2", "1.0e-2"),
        ("1e-8", "1.0e-8", "1.0e-8", "1.0e-8"),
        ("1.5e-7", "1.5e-7", "1.5e-7", "1.5e-7"),
        ("1e-400", "1.0e-400", "1.0e-400", "1.0e-400"),
        ("1e400", &expanded_400, &expanded_400, "1.0e400"),
        ("1e1024", &expanded_1024, &expanded_1024, "1.0e1024"),
        ("1e1025", &expanded_1025, "1.0e1025", "1.0e1025"),
        (
            "1e19",
            "10000000000000000000",
            "10000000000000000000",
            "10000000000000000000",
        ),
        (
            "1.0e19",
            "10000000000000000000",
            "10000000000000000000",
            "10000000000000000000",
        ),
        (
            "1.2e19",
            "12000000000000000000",
            "12000000000000000000",
            "1.2e19",
        ),
        (
            "-1.2e19",
            "-12000000000000000000",
            "-12000000000000000000",
            "-1.2e19",
        ),
        (
            "10000000000000000001",
            "10000000000000000001",
            "10000000000000000001",
            "1.0000000000000000001e19",
        ),
        (
            "9223372036854775808",
            "9223372036854775808",
            "9223372036854775808",
            "9223372036854775808",
        ),
        (
            "12345678901234567890",
            "12345678901234567890",
            "12345678901234567890",
            "1.234567890123456789e19",
        ),
    ];
    let raw = cases.map(|(raw, _, _, _)| raw).join(",");
    let compact = cases.map(|(_, _, compact, _)| compact).join(",");
    let source = format!(
        r#"{{"nbformat":4,"metadata":{{"x":[{raw}]}},"cells":[
            {{"id":"code","cell_type":"code","metadata":{{"x":[{raw}]}},"source":"","outputs":[
                {{"output_type":"display_data","metadata":{{"image/png":{{"x":[{raw}]}}}},
                 "data":{{"application/json":[{raw}],"image/png":"YQ=="}}}}]}}]}}"#
    );
    let mut options = ReaderOptions::default();
    options.ipynb_output = IpynbOutput::All;
    let mut context = Context::pure();
    let mut doc = super::reader::read(&Sources::from_text(source), &options, &mut context).unwrap();
    let Some(MetaValue::MetaMap(meta)) = doc.meta.get("jupyter") else {
        unreachable!()
    };
    let MetaValue::MetaList(values) = &meta["x"] else {
        unreachable!()
    };
    assert_eq!(
        values,
        &cases.map(|(_, meta, _, _)| MetaValue::MetaString(meta.into()))
    );
    let [Block::Div(cell, body)] = doc.blocks.as_mut_slice() else {
        unreachable!()
    };
    let [_, Block::Div(_, outputs)] = body.as_mut_slice() else {
        unreachable!()
    };
    let [Block::CodeBlock(_, json), Block::Para(image)] = outputs.as_mut_slice() else {
        unreachable!()
    };
    let [Inline::Image(image, _, _)] = image.as_mut_slice() else {
        unreachable!()
    };
    let compact = format!("[{compact}]");
    assert_eq!(cell.get("x"), Some(compact.as_str()));
    assert_eq!(image.get("x"), Some(compact.as_str()));
    assert_eq!(*json, compact);

    // Give the writer the original tokens to test its own number rules.
    let raw = format!("[{raw}]");
    cell.attributes[0].1.clone_from(&raw);
    image.attributes[0].1.clone_from(&raw);
    *json = raw;
    let written = write(&doc, &mut context).unwrap();
    let tape = parse_value(&written, 128).unwrap();
    let root = tape.root();
    let cell = root.lookup("cells").unwrap().items().next().unwrap();
    let output = cell.lookup("outputs").unwrap().items().next().unwrap();
    for value in [
        cell.lookup("metadata").unwrap().lookup("x").unwrap(),
        output.lookup("metadata").unwrap().lookup("x").unwrap(),
        output
            .lookup("data")
            .unwrap()
            .lookup("application/json")
            .unwrap(),
    ] {
        assert_eq!(
            value.items().map(|item| item.kind()).collect::<Vec<_>>(),
            cases.map(|(_, _, _, pretty)| JsonKind::Number(pretty))
        );
    }
    assert_eq!(
        root.lookup("metadata")
            .unwrap()
            .lookup("x")
            .unwrap()
            .items()
            .map(|item| item.kind())
            .collect::<Vec<_>>(),
        cases.map(|(_, meta, _, _)| JsonKind::String(meta))
    );
}

#[test]
fn exponent_expansion_uses_the_budget() {
    let source = r#"{"nbformat":4,"metadata":{"x":1e2000000000},"cells":[]}"#;
    assert!(matches!(
        read(source, &mut Context::pure()),
        Err(Error::IpynbDecoding(message)) if message == "Notebook exceeds the conversion size limit"
    ));
    let source =
        r#"{"nbformat":4,"cells":[{"cell_type":"raw","metadata":{"x":1e1024},"source":""}]}"#;
    let mut context = Context::pure();
    context.limits.max_archive_size = 1024;
    assert!(matches!(
        read(source, &mut context),
        Err(Error::IpynbDecoding(message)) if message == "Notebook exceeds the conversion size limit"
    ));
}

#[test]
fn exact_json_decoding_errors() {
    for (input, expected) in [
        ("x", r#"Unexpected "x\n\n", expecting JSON value"#),
        ("x\n", r#"Unexpected "x\n\n", expecting JSON value"#),
        (
            "{",
            "Unexpected end-of-input, expecting record key literal or }",
        ),
        (
            r#"{"a":1,}"#,
            r#"Unexpected ",}\n\n", expecting key literal"#,
        ),
        (r#"{"a":[1,2"#, "Unexpected end-of-input, expecting , or ]"),
        (r#"{"a" 1}"#, r#"Unexpected "1}\n\n", expecting :"#),
        (
            r#"{"nbformat":4,"#,
            "Unexpected end-of-input, expecting key literal",
        ),
        (
            r#"{"nbformat":4,"cells":[]} x"#,
            "Error in $: expected nbformat <= 3",
        ),
        (
            r#"{"a": éyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy"#,
            r#"Unexpected "\195\169yyyyyyyyyyyyyyyyyyyyyyyyyyyy", expecting JSON value"#,
        ),
    ] {
        let Err(Error::IpynbDecoding(message)) = read(input, &mut Context::pure()) else {
            panic!("Expected an ipynb decoding error for {input:?}");
        };
        assert_eq!(message, expected, "{input:?}");
    }
}

#[test]
fn duplicate_fields_keep_the_first_value() {
    let source = r#"{"nbformat":4,"nbformat":5,"cells":[],"metadata":{"x":{"y":1,"y":2}}}"#;
    let doc = read(source, &mut Context::pure()).unwrap();
    let Some(MetaValue::MetaMap(meta)) = doc.meta.get("jupyter") else {
        unreachable!()
    };
    assert_eq!(
        meta["x"],
        MetaValue::MetaMap([("y".into(), MetaValue::MetaString("1".into()))].into())
    );
}

#[test]
fn legacy_schema() {
    let input = r#"{"nbformat":3,"worksheets":[{"cells":[
        {"cell_type":"heading","level":2,"source":["An H","2"]},
        {"cell_type":"code","language":"python","input":"x","prompt_number":0,
         "outputs":[{"output_type":"pyout","prompt_number":1,"text":["x","\n"]}]}
    ]}]}"#;
    let doc = read(input, &mut Context::pure()).unwrap();
    let [Block::Div(_, heading), Block::Div(attr, code)] = doc.blocks.as_slice() else {
        unreachable!()
    };
    assert!(matches!(heading.as_slice(), [Block::Header(2, ..)]));
    assert_eq!(attr.get("execution_count"), Some("0"));
    let [Block::CodeBlock(_, _), Block::Div(attr, blocks)] = code.as_slice() else {
        unreachable!()
    };
    assert_eq!(attr.get("execution_count"), Some("1"));
    assert_eq!(blocks, &[Block::CodeBlock(Attr::default(), "x\n".into())]);
}

#[test]
fn schema_errors() {
    for (input, expected) in [
        ("{}", "key \"worksheets\" not found"),
        (
            r#"{"nbformat":4,"cells":[{"cell_type":"bad","metadata":{},"source":""}]}"#,
            "expected nbformat <= 3",
        ),
        (
            r#"{"nbformat":4,"cells":[{"cell_type":"markdown","metadata":{},"source":[1]}]}"#,
            "expected nbformat <= 3",
        ),
    ] {
        let error = read(input, &mut Context::pure()).unwrap_err();
        assert_eq!(error.exit_code(), 93);
        assert!(error.to_string().ends_with(expected));
    }
}

#[test]
fn raw_precedence_and_separate_cells() {
    let doc = read(r##"{"nbformat":4,"cells":[
        {"cell_type":"raw","metadata":{"format":"text/html","raw_mimetype":"application/pdf"},"source":""},
        {"cell_type":"markdown","metadata":{},"source":"# Hi"},
        {"cell_type":"markdown","metadata":{},"source":"# Hi"}
    ]}"##, &mut Context::pure()).unwrap();
    let [
        Block::Div(_, raw),
        Block::Div(_, first),
        Block::Div(_, second),
    ] = doc.blocks.as_slice()
    else {
        unreachable!()
    };
    assert_eq!(
        raw,
        &[Block::RawBlock(Format("latex".into()), String::new())]
    );
    assert_eq!(first, second);
}

#[test]
fn output_modes_and_media() {
    let source = r#"{"nbformat":4,"cells":[{"cell_type":"code","metadata":{},"source":"x",
        "outputs":[{"output_type":"display_data","metadata":{},"data":{"image/png":"YQ==","text/html":"<b>H</b>","text/plain":"P"}}]}]}"#;
    for (mode, format, len) in [
        (IpynbOutput::All, "json", 3),
        (IpynbOutput::None, "json", 0),
        (IpynbOutput::Best, "json", 1),
        (IpynbOutput::Best, "html", 1),
        (IpynbOutput::Best, "ipynb", 3),
    ] {
        let mut context = Context::pure();
        let mut options = ReaderOptions::default();
        options.ipynb_output = mode;
        options.output_format = Some(format.into());
        let doc = super::reader::read(&Sources::from_text(source), &options, &mut context).unwrap();
        let [Block::Div(_, blocks)] = doc.blocks.as_slice() else {
            unreachable!()
        };
        let Block::Div(_, output) = &blocks[1] else {
            unreachable!()
        };
        assert_eq!(output.len(), len);
        if format == "html" {
            assert!(matches!(output.as_slice(), [Block::RawBlock(_, _)]));
        } else if len != 0 {
            assert_eq!(
                context
                    .media
                    .get("86f7e437faa5a7fce15d1ddcb9eaeaea377667b8.png")
                    .unwrap()
                    .contents,
                b"a"
            );
        }
    }
}

#[test]
fn attachment_round_trip_leaves_links_unchanged() {
    let mut context = Context::pure();
    let doc = read(
        r#"{"nbformat":4,"cells":[{"id":"m","cell_type":"markdown",
        "metadata":{},"source":"![image](attachment:a%20b.png) [link](attachment:a%20b.png)",
        "attachments":{"a b.png":{"image/png":"YQ=="}}}]}"#,
        &mut context,
    )
    .unwrap();
    assert_eq!(context.media.get("m-a%20b.png").unwrap().contents, b"a");
    let written: Value = serde_json::from_str(&write(&doc, &mut context).unwrap()).unwrap();
    let cell = &written["cells"][0];
    assert_eq!(cell["id"], "m");
    assert_eq!(
        cell["source"][0],
        "![image](attachment:m-a%20b.png) [link](attachment:a%20b.png)"
    );
    assert_eq!(cell["attachments"]["m-a%20b.png"]["image/png"], "YQ==\n");
}

#[test]
fn attachment_names_without_ids_use_the_last_payload() {
    let cell = |data: &str| {
        format!(
            r#"{{"cell_type":"markdown","source":"![x](attachment:a.png)",
        "metadata":{{}},"attachments":{{"a.png":{{"image/png":"{data}"}}}}}}"#
        )
    };
    let source = format!(
        r#"{{"nbformat":4,"cells":[{},{}]}}"#,
        cell("YQ=="),
        cell("Yg==")
    );
    let mut context = Context::pure();
    let doc = read(&source, &mut context).unwrap();
    assert_eq!(context.media.get("a.png").unwrap().contents, b"b");
    assert_eq!(doc.blocks[0], doc.blocks[1]);
}

#[test]
fn writer_keeps_base64_images_and_traceback_lines() {
    let source = r#"{"nbformat":4,"cells":[{"cell_type":"code","metadata":{},"source":"","outputs":[
       {"output_type":"display_data","metadata":{},"data":{"image/png":"YQ==","image/svg+xml":"<svg/>"}},
       {"output_type":"error","ename":"A","evalue":"B","traceback":["one","two\n"]}]}]}"#;
    let mut context = Context::pure();
    let mut options = ReaderOptions::default();
    options.output_format = Some("ipynb".into());
    let doc = super::reader::read(&Sources::from_text(source), &options, &mut context).unwrap();
    let written: Value = serde_json::from_str(&write(&doc, &mut context).unwrap()).unwrap();
    let outputs = &written["cells"][0]["outputs"];
    assert_eq!(outputs[0]["data"]["image/png"], "YQ==\n");
    assert_eq!(outputs[0]["data"]["image/svg+xml"], "PHN2Zy8+\n");
    assert_eq!(
        outputs[1]["traceback"],
        serde_json::json!(["one\n", "two\n", "\n"])
    );
}

#[test]
fn missing_resources_and_uuid() {
    let doc = Pandoc {
        blocks: vec![Block::Para(vec![Inline::Image(
            Attr::default(),
            vec![],
            Target::new("missing.png", ""),
        )])],
        ..Pandoc::default()
    };
    assert_eq!(
        write(&doc, &mut Context::pure()).unwrap_err().exit_code(),
        99
    );
    let doc = Pandoc {
        blocks: vec![Block::Para(vec![Inline::Str("text".into())])],
        ..Pandoc::default()
    };
    let mut result: Value =
        serde_json::from_str(&write(&doc, &mut Context::pure()).unwrap()).unwrap();
    let id = result["cells"][0]["id"].take();
    assert_eq!(id, "00000000-0000-4000-8000-000000000000");
}

#[test]
fn reader_on_two_mib_stack() {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            let source = |depth| {
                let nested = format!("{}null{}", "[".repeat(depth), "]".repeat(depth));
                format!(r#"{{"nbformat":4,"metadata":{{"nested":{nested}}},"cells":[]}}"#)
            };
            let mut context = Context::pure();
            assert!(read(&source(126), &mut context).is_ok());
            assert!(matches!(
                read(&source(127), &mut context),
                Err(Error::IpynbDecoding(message)) if message == "Notebook exceeds the nesting limit"
            ));
            context.limits.max_nesting_depth = 1000;
            let doc = read(&source(998), &mut context).unwrap();
            let Some(MetaValue::MetaMap(meta)) = doc.meta.get("jupyter") else {
                unreachable!()
            };
            let mut value = &meta["nested"];
            for _ in 0..998 {
                let MetaValue::MetaList(items) = value else {
                    unreachable!()
                };
                value = &items[0];
            }
            assert_eq!(*value, MetaValue::MetaString(String::new()));
            assert!(read(&source(999), &mut context).is_err());
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn writer_on_two_mib_stack() {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            let mut blocks = vec![Block::Plain(vec![Inline::Str("text".into())])];
            for _ in 0..96 {
                blocks = vec![Block::BlockQuote(blocks)];
            }
            let doc = Pandoc {
                blocks,
                ..Pandoc::default()
            };
            let mut context = Context::pure();
            assert!(write(&doc, &mut context).is_ok());
            context.limits.max_nesting_depth = 95;
            assert!(matches!(write(&doc, &mut context), Err(Error::App(_))));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn writer_json_depth_on_two_mib_stack() {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            for depth in [128, 1000] {
                let raw = format!("{}1.0{}", "[".repeat(depth), "]".repeat(depth));
                let doc = Pandoc {
                    blocks: vec![Block::Div(
                        Attr::new(
                            "code",
                            vec!["cell".into(), "code".into()],
                            vec![("x".into(), raw.clone())],
                        ),
                        vec![
                            Block::CodeBlock(Attr::default(), String::new()),
                            Block::Div(
                                Attr::new("", vec!["output".into(), "display_data".into()], vec![]),
                                vec![Block::CodeBlock(
                                    Attr::new("", vec!["json".into()], vec![]),
                                    raw,
                                )],
                            ),
                        ],
                    )],
                    ..Pandoc::default()
                };
                let mut context = Context::pure();
                context.limits.max_nesting_depth = depth;
                let text = write(&doc, &mut context).unwrap();
                let tape = parse_value(&text, depth + 10).unwrap();
                let cell = tape.root().lookup("cells").unwrap().items().next().unwrap();
                let output = cell.lookup("outputs").unwrap().items().next().unwrap();
                for mut value in [
                    cell.lookup("metadata").unwrap().lookup("x").unwrap(),
                    output
                        .lookup("data")
                        .unwrap()
                        .lookup("application/json")
                        .unwrap(),
                ] {
                    for _ in 0..depth {
                        value = value.items().next().unwrap();
                    }
                    assert_eq!(value.kind(), JsonKind::Number("1.0"));
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
