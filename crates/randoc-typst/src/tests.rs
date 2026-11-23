use randoc_ast::*;
use randoc_core::{
    Context, Error, ReaderOptions, Result, Sources, WriterOptions, default_extensions,
};

fn read(input: &str, options: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    super::reader::read(&Sources::from_text(input), options, context)
}

fn options() -> ReaderOptions {
    let mut options = ReaderOptions::default();
    options.extensions = default_extensions("typst");
    options
}

#[test]
fn metadata_is_available_in_fragments() -> Result<()> {
    let doc = read(
        "#set document(title: [Example title], author: (\"A\", \"B\"))\n#bibliography(\"list.bib\")",
        &options(),
        &mut Context::pure(),
    )?;
    assert_eq!(
        doc.meta.0.get("title"),
        Some(&MetaValue::MetaInlines(vec![
            Inline::Str("Example".into()),
            Inline::Space,
            Inline::Str("title".into())
        ]))
    );
    assert!(matches!(doc.meta.0.get("author"), Some(MetaValue::MetaList(xs)) if xs.len() == 2));
    assert_eq!(
        doc.meta.0.get("bibliography"),
        Some(&MetaValue::MetaString("list.bib".into()))
    );
    Ok(())
}

#[test]
fn typst_inputs_are_visible_to_closures() -> Result<()> {
    let mut options = options();
    options.typst_inputs = vec![("name".into(), "Randoc".into())];
    let doc = read(
        "#let greeting() = sys.inputs.at(\"name\")\n#greeting()",
        &options,
        &mut Context::pure(),
    )?;
    assert_eq!(
        doc.blocks,
        vec![Block::Para(vec![Inline::Str("Randoc".into())])]
    );
    Ok(())
}

#[test]
fn deferred_code_has_no_side_effect() -> Result<()> {
    let doc = read(
        "#let template(body) = { regex(\"x\"); body }",
        &options(),
        &mut Context::pure(),
    )?;
    assert!(doc.blocks.is_empty());
    Ok(())
}

#[test]
fn unsupported_functions_return_a_positioned_error() {
    for name in ["regex", "json", "csv", "yaml", "toml", "xml"] {
        let result = read(
            &format!("First line.\n#{name}(\"x\")"),
            &options(),
            &mut Context::pure(),
        );
        assert!(
            matches!(result, Err(Error::Parse(text)) if text.starts_with("(line 2, column 2):"))
        );
    }
}

#[test]
fn reader_enforces_size_and_table_limits() {
    let mut context = Context::pure();
    context.limits.max_input_size = 3;
    assert!(matches!(
        read("abcd", &options(), &mut context),
        Err(Error::Parse(_))
    ));
    context.limits.max_input_size = 1000;
    context.limits.max_table_cells = 10;
    assert!(matches!(
        read(
            "#table(columns: 3, table.cell(rowspan: 4)[X])",
            &options(),
            &mut context
        ),
        Err(Error::Parse(_))
    ));
}

#[test]
fn reader_rejects_unbounded_evaluation() {
    let math_text = format!("#let s = \"x\" * 15000000\n$ {} $", "#s ".repeat(1000));
    let math_content = "#let s = \"x\" * 7000000; #let c = [$#s$]; $#c #c$";
    let repeat_body = format!(
        "#let big = [{}]\n#for i in range(3000) [#big.body]",
        "a".repeat(100_000)
    );
    let repeat_header = format!(
        "#show heading: it => {{ for i in range(3000) [#it.body] }}\n= {}",
        "a".repeat(100_000)
    );
    for input in [
        &math_text,
        math_content,
        &repeat_body,
        &repeat_header,
        "#let f(n) = f(n + 1)\n#f(0)",
        "#while true {}",
        "#{ \"a\" * 1000000000 }",
        "#lorem(10000000)",
    ] {
        assert!(
            matches!(
                read(input, &options(), &mut Context::pure()),
                Err(Error::Parse(message)) if message.contains("evaluation limit")
            ),
            "{:.60}",
            input
        );
    }
    assert!(read_text("#let s = [$x$]; $#s #s$").is_ok());
}

#[test]
fn reader_checks_depth_on_a_two_mib_stack() -> std::io::Result<()> {
    let thread = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut context = Context::pure();
            let emph = format!("{}x{}", "#emph[".repeat(60), "]".repeat(60));
            let function = "#let f(n) = if n == 0 { 0 } else { 1 + f(n - 1) }";
            assert!(read(&emph, &options(), &mut context).is_ok());
            for count in [30, 120] {
                assert_eq!(
                    read_text(&format!("{function}\n#f({count})"))?,
                    count.to_string()
                );
            }
            assert_eq!(read_text(&format!("#(1{})", " + 1".repeat(119)))?, "120");
            assert_eq!(
                read_text(&format!("#(1{})", " + 1".repeat(10_000)))?,
                "10001"
            );
            let functions = "#let f(x) = x\n".repeat(6000);
            assert_eq!(read_text(&format!("{functions}#f[ok]"))?, "ok");
            for input in [
                format!("{}x{}", "#emph[".repeat(10_000), "]".repeat(10_000)),
                format!("#{}x{}", "(".repeat(10_000), ")".repeat(10_000)),
                format!("{}x{}", "/*".repeat(10_000), "*/".repeat(10_000)),
                format!("$ {}x{} $", "(".repeat(10_000), ")".repeat(10_000)),
                "#let x = [a]; #for i in range(10000) { x = [#strong(x)] }".into(),
                "#let x = (); #for i in range(10000) { x = (x,) }".into(),
                format!("#sym{}", ".x".repeat(10_000)),
                format!("{function}\n#f(200)"),
                "#let f(n) = f(n + 1)\n#f(0)".into(),
            ] {
                assert!(matches!(
                    read(&input, &options(), &mut context),
                    Err(Error::Parse(_))
                ));
            }
            Ok::<(), Error>(())
        })?;
    assert!(matches!(thread.join(), Ok(Ok(()))));
    Ok(())
}

#[test]
fn writer_checks_depth_on_a_two_mib_stack() -> std::io::Result<()> {
    let thread = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut context = Context::pure();
            let mut block = Block::Para(vec![Inline::Str("x".into())]);
            for _ in 0..10_000 {
                block = Block::BlockQuote(vec![block]);
            }
            let mut doc = Pandoc {
                meta: Meta::default(),
                blocks: vec![block],
            };
            let result = super::writer::write(&doc, &WriterOptions::default(), &mut context);
            // Drop one level at a time so that the test exercises the writer stack.
            while let Some(Block::BlockQuote(child)) = doc.blocks.pop() {
                doc.blocks = child;
            }
            assert!(matches!(result, Err(Error::App(_))));
            let mut inline = Inline::Str("x".into());
            for _ in 0..10_000 {
                inline = Inline::Emph(vec![inline]);
            }
            doc.blocks = vec![Block::Para(vec![inline])];
            let result = super::writer::write(&doc, &WriterOptions::default(), &mut context);
            if let Some(Block::Para(mut xs)) = doc.blocks.pop() {
                while let Some(Inline::Emph(child)) = xs.pop() {
                    xs = child;
                }
            }
            assert!(matches!(result, Err(Error::App(_))));
        })?;
    assert!(thread.join().is_ok());
    Ok(())
}

#[test]
fn writer_uses_typst_math_and_highlight_options() -> Result<()> {
    let doc = Pandoc {
        meta: Meta::default(),
        blocks: vec![
            Block::Para(vec![Inline::Math(
                MathType::InlineMath,
                "\\frac{a}{b}".into(),
            )]),
            Block::CodeBlock(
                Attr::new("", vec!["rust".into()], vec![]),
                "let x = 2;".into(),
            ),
        ],
    };
    let mut options = WriterOptions::default();
    let render =
        |options: &WriterOptions| super::writer::write(&doc, options, &mut Context::pure());
    assert_eq!(render(&options)?, "$a / b$\n\n```rust\nlet x = 2;\n```\n");
    options.highlight_method = randoc_core::HighlightMethod::None;
    assert_eq!(render(&options)?, "$a / b$\n\n```\nlet x = 2;\n```\n");
    options.highlight_method = randoc_core::HighlightMethod::Style("pygments".into());
    assert_eq!(
        render(&options)?,
        "$a / b$\n\n#Skylighting(([#KeywordTok(\"let\");#NormalTok(\" x \");#OperatorTok(\"=\");#NormalTok(\" \");#DecValTok(\"2\");#OperatorTok(\";\");],));\n"
    );
    Ok(())
}

fn read_text(input: &str) -> Result<String> {
    let doc = read(input, &options(), &mut Context::pure())?;
    Ok(randoc_core::shared::stringify_blocks(&doc.blocks))
}

#[test]
fn reader_closures_use_the_bindings_at_definition() -> Result<()> {
    use std::fmt::Write as _;

    let mut input = String::from("#let a = [old]\n");
    for i in 0..64 {
        let _ = writeln!(input, "#let f{i}(x) = a + x");
    }
    input.push_str("#let a = [new]\n#f63[value]");
    assert_eq!(read_text(&input)?, "oldvalue");
    Ok(())
}

#[test]
fn reader_reuses_large_bindings_and_settings_in_nested_scopes() -> Result<()> {
    use std::fmt::Write as _;

    let mut input = String::new();
    for i in 0..3000 {
        let _ = writeln!(input, "#let a{i} = 1");
    }
    input.push_str("#set text(");
    for i in 0..2500 {
        let _ = write!(input, "k{i}: 1,");
    }
    input.push_str(")\n#let f(x) = { [#x] }\n#for i in range(2000) { f(\"a\") }");
    assert_eq!(read_text(&input)?, "a".repeat(2000));
    Ok(())
}

#[test]
fn reader_limits_text_conversions_and_show_scans() {
    let content = "x".repeat(100_000);
    for expr in [
        "big == [x]",
        "big != [x]",
        "big in (x: 1)",
        "str(big)",
        "repr(big)",
    ] {
        let input = format!("#let big = [{content}]; #for i in range(5000) {{ {expr}; none }}");
        assert!(
            matches!(read_text(&input), Err(Error::Parse(message))
            if message.contains("evaluation limit")),
            "{expr}"
        );
    }

    use std::fmt::Write as _;
    let mut input = String::new();
    for i in 0..150 {
        let _ = writeln!(input, "#show \"selector{i}\": x => x");
    }
    input.push_str(&"word ".repeat(8000));
    assert!(matches!(read_text(&input), Err(Error::Parse(message))
        if message.contains("evaluation limit")));
}

#[test]
fn reader_propagates_loop_control_and_return() -> Result<()> {
    for (input, expected) in [
        (
            "#let i = 0\n#while true { i += 1; if i > 3 { break } }\n#i",
            "4",
        ),
        (
            "#for i in range(4) { [x]; if i == 1 { break }; [y] }",
            "xyx",
        ),
        (
            "#for i in range(3) { [x]; if i == 1 { continue }; [y] }",
            "xyxxy",
        ),
        (
            "#let f(x) = { if x { return [yes] }; [no] }\n#f(true)",
            "yes",
        ),
        (
            "#let f() = { [x]; if true { [y]; return }; [z] }\n#f()",
            "xy",
        ),
        (
            "#let f() = { for i in range(4) { if i == 2 { return i } }; 0 }\n#f()",
            "2",
        ),
    ] {
        assert_eq!(read_text(input)?, expected, "{input}");
    }
    Ok(())
}

#[test]
fn reader_iterates_strings_and_dictionaries() -> Result<()> {
    assert_eq!(read_text("#for c in \"aé😀\" [#c,]")?, "a,é,😀,");
    assert_eq!(
        read_text("#let d = (z: 1, a: 2)\n#for (k, v) in (..d, b: 3) [#k=#v,]")?,
        "z=1,a=2,b=3,"
    );
    Ok(())
}

#[test]
fn reader_links_to_labels_after_crlf_headings() -> Result<()> {
    let doc = read("= H\r\n<x>\r\nsee @x", &options(), &mut Context::pure())?;
    assert!(matches!(doc.blocks.first(), Some(Block::Header(_, attr, _)) if attr.id == "x"));
    assert!(matches!(doc.blocks.last(), Some(Block::Para(xs))
        if matches!(xs.last(), Some(Inline::Link(attr, text, target))
            if attr.has_class("ref")
                && text == &[Inline::Str("[x]".into())]
                && target.url == "#x")));
    Ok(())
}

#[test]
fn reader_reports_named_and_end_of_input_parse_errors() {
    use randoc_core::Source;

    for (input, location) in [
        ("a\n#let x = )", "(line 2, column 10)"),
        ("a\n#let x = (", "(line 4, column 1)"),
        ("aé\n#(1 +\n", "(line 4, column 1)"),
        ("#emph[x", "(line 3, column 1)"),
    ] {
        let sources = Sources(vec![Source {
            name: "e.typ".into(),
            text: input.into(),
        }]);
        let result = super::reader::read(&sources, &options(), &mut Context::pure());
        assert!(
            matches!(result, Err(Error::Parse(message))
            if message.starts_with(&format!("\"e.typ\" {location}:"))),
            "{input}"
        );
    }
}

#[test]
fn reader_handles_radix_integers_and_reports_division_by_zero() -> Result<()> {
    assert_eq!(read_text("#(0x1F + 0o17 + 0b101)")?, "51");
    assert_eq!(read_text("#(0xF-1)")?, "14");
    assert_eq!(read_text("#(1.0/0)")?, "Infinity");
    assert!(matches!(read_text("#(1/0)"), Err(Error::Parse(message))
        if message == "(line 1, column 2):\nCan't / VInteger 1 and VInteger 0"));
    Ok(())
}

#[test]
fn reader_reports_incomplete_equations_and_disallowed_paths() {
    assert!(matches!(read_text("$1/$"), Err(Error::Parse(message))
        if message.contains("equation is incomplete")));
    for directive in ["#include", "#import", "#read"] {
        for path in ["/x.typ", "../x.typ", "@local/x:1.0.0"] {
            let input = if directive == "#read" {
                format!("{directive}(\"{path}\")")
            } else {
                format!("{directive} \"{path}\"")
            };
            assert!(
                matches!(read_text(&input), Err(Error::Parse(message))
                if message.contains("outside the permitted scope")),
                "{input}"
            );
        }
    }
}

#[test]
fn table_limits_count_only_generated_cells() -> Result<()> {
    let mut context = Context::pure();
    context.limits.max_table_cells = 0;
    let input = format!("#table(columns: 3, {})", "[a],".repeat(70_000 * 3));
    let doc = read(&input, &options(), &mut context)?;
    assert!(matches!(doc.blocks.as_slice(), [Block::Table(table)]
        if table.bodies.first().is_some_and(|body| body.body.len() == 70_000)));
    let output = super::writer::write(&doc, &WriterOptions::default(), &mut context)?;
    assert_eq!(output.matches("[a],").count(), 210_000);
    assert!(read("#table()", &options(), &mut context).is_ok());
    assert!(read("#table(columns: 2, [a])", &options(), &mut context).is_err());
    context.limits.max_table_cells = 1;
    assert!(read("#table(columns: 2, [a])", &options(), &mut context).is_ok());
    assert!(
        read(
            "#table(columns: 2, table.cell(colspan: 2)[a])",
            &options(),
            &mut context
        )
        .is_ok()
    );
    assert!(
        read(
            "#table(columns: 2, table.cell(colspan: 2, rowspan: 2)[a])",
            &options(),
            &mut context
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn reader_uses_the_configured_nesting_limit() -> Result<()> {
    let input = format!("{}x{}", "#emph[".repeat(160), "]".repeat(160));
    let mut context = Context::pure();
    assert!(read(&input, &options(), &mut context).is_err());
    context.limits.max_nesting_depth = 256;
    let doc = read(&input, &options(), &mut context)?;
    assert_eq!(randoc_core::shared::stringify_blocks(&doc.blocks), "x");
    Ok(())
}

#[test]
fn writer_uses_the_configured_nesting_limit() -> Result<()> {
    let mut inline = Inline::Str("x".into());
    for _ in 0..200 {
        inline = Inline::Emph(vec![inline]);
    }
    let doc = Pandoc {
        blocks: vec![Block::Para(vec![inline])],
        ..Pandoc::default()
    };
    let mut context = Context::pure();
    let opts = WriterOptions::default();
    assert!(matches!(
        super::writer::write(&doc, &opts, &mut context),
        Err(Error::App(_))
    ));
    context.limits.max_nesting_depth = 256;
    let output = super::writer::write(&doc, &opts, &mut context)?;
    assert_eq!(
        output.trim(),
        format!("{}x{}", "#emph[".repeat(200), "]".repeat(200))
    );
    Ok(())
}

#[test]
fn writer_escapes_markup_after_line_breaks() -> Result<()> {
    use randoc_core::WrapOption;

    for marker in ["+", "-", "=", "/", "12.", ";"] {
        for (separator, wrap, columns) in [
            (Inline::SoftBreak, WrapOption::WrapPreserve, 72),
            (Inline::Space, WrapOption::WrapAuto, 3),
        ] {
            let doc = Pandoc {
                blocks: vec![Block::Para(vec![
                    Inline::Str("aaa".into()),
                    separator,
                    Inline::Str(marker.into()),
                    Inline::Space,
                    Inline::Str("b".into()),
                ])],
                ..Pandoc::default()
            };
            let mut opts = WriterOptions::default();
            opts.wrap_text = wrap;
            opts.columns = columns;
            let output = super::writer::write(&doc, &opts, &mut Context::pure())?;
            assert!(output.contains(&format!("\n\\{marker}")), "{output}");
        }
    }
    Ok(())
}
