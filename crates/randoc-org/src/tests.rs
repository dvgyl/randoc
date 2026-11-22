use std::io;

use randoc_ast::{Attr, Block, Inline, Pandoc};
use randoc_core::shared::stringify;
use randoc_core::{
    Context, Error, Host, LogMessage, PureHost, Reader, ReaderOptions, Result, Source, Sources,
    Writer, WriterOptions, default_extensions,
};
use randoc_templates::Template;

use crate::{reader, writer};

fn read(input: &str, context: &mut Context) -> Result<Pandoc> {
    read_sources(&Sources::from_text(input), context)
}

fn read_sources(sources: &Sources, context: &mut Context) -> Result<Pandoc> {
    let Some(Reader::Text(reader)) = reader("org") else {
        return Err(randoc_core::Error::App("Missing Org reader.".into()));
    };
    let mut options = ReaderOptions::default();
    options.extensions = default_extensions("org");
    reader.read(sources, &options, context)
}

fn write(doc: &Pandoc, options: &WriterOptions, context: &mut Context) -> Result<String> {
    let Some(Writer::Text(writer)) = writer("org") else {
        return Err(randoc_core::Error::App("Missing Org writer.".into()));
    };
    assert!(writer.renders_template());
    writer.write(doc, options, context)
}

#[test]
fn headings_tables_and_figures() -> Result<()> {
    let input = "#+TITLE: Example\n* TODO Heading :tag:\n:PROPERTIES:\n:CUSTOM_ID: chosen\n:END:\n\n#+caption: Picture\n[[file:p.png]]\n\n| a | b |\n|---+---|\n| 1 | 2 |";
    let doc = read(input, &mut Context::pure())?;
    assert_eq!(doc.blocks.len(), 3);
    assert!(
        matches!(&doc.blocks[0], Block::Header(1, attr, text) if attr.id == "chosen" && stringify(text) == "TODO Heading tag")
    );
    assert!(matches!(&doc.blocks[1], Block::Figure(_, caption, _) if caption.long.len() == 1));
    assert!(
        matches!(&doc.blocks[2], Block::Table(table) if table.head.rows.len() == 1 && table.bodies[0].body[0].cells.len() == 2)
    );
    assert!(doc.meta.get("title").is_some());
    Ok(())
}

#[test]
fn macro_cycles_and_growth_are_bounded() -> Result<()> {
    let mut context = Context::pure();
    let doc = read(
        "#+macro: a {{{b}}}\n#+macro: b {{{a}}}\n{{{a}}}",
        &mut context,
    )?;
    assert!(matches!(doc.blocks.as_slice(), [Block::Para(text)] if stringify(text) == "{{{b}}}"));
    let input = "#+macro: double $1$1\n{{{double(123456789)}}}";
    context.limits.max_input_size = input.len() as u64 + 10;
    assert!(read(input, &mut context).is_err());
    Ok(())
}

#[test]
fn include_cycles_use_the_host_and_stop() -> Result<()> {
    let mut host = PureHost::default();
    host.files.insert(
        "dir/b.org".into(),
        b"second\n\n#+include: \"./a.org\"".to_vec(),
    );
    host.files.insert(
        "dir/a.org".into(),
        b"first\n\n#+include: \"b.org\"".to_vec(),
    );
    let mut context = Context::new(Box::new(host));
    let source = Sources(vec![Source {
        name: "dir/a.org".into(),
        text: "first\n\n#+include: \"b.org\"".into(),
    }]);
    let doc = read_sources(&source, &mut context)?;
    assert_eq!(
        doc.blocks,
        vec![
            Block::Para(vec![Inline::Str("first".into())]),
            Block::Para(vec![Inline::Str("second".into())])
        ]
    );
    Ok(())
}

#[test]
fn input_and_table_limits_apply_across_the_document() {
    let mut context = Context::pure();
    context.limits.max_input_size = 4;
    assert!(read("words", &mut context).is_err());
    context.limits.max_input_size = 1024;
    context.limits.max_table_cells = 7;
    assert!(read("| a | b |\n\n| c | d |", &mut context).is_err());
}

#[test]
fn nested_content_stays_within_a_two_mib_stack() -> std::thread::Result<()> {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut context = Context::pure();
            let mut options = WriterOptions::default();
            options.extensions = default_extensions("org");
            for depth in [12, 24, 44, 400] {
                let names = (0..depth).map(|i| format!("quote{i}")).collect::<Vec<_>>();
                let mut input = String::new();
                for name in &names {
                    input.push_str(&format!("#+begin_{name}\n"));
                }
                input.push_str("base^{x}\n");
                for name in names.iter().rev() {
                    input.push_str(&format!("#+end_{name}\n"));
                }
                assert_eq!(read(&input, &mut context).is_ok(), depth + 2 < 48);

                let mut input = String::from("See [fn:n0].\n\n");
                for i in 0..depth {
                    input.push_str(&format!("[fn:n{i}] Note [fn:n{}].\n", i + 1));
                }
                assert_eq!(read(&input, &mut context).is_ok(), depth * 2 + 1 < 48);

                let mut inline = Inline::Str("text".into());
                for i in 0..depth {
                    inline = if i % 2 == 0 {
                        Inline::Strong(vec![inline])
                    } else {
                        Inline::Note(vec![Block::Para(vec![inline])])
                    };
                }
                let doc = Pandoc {
                    blocks: vec![Block::Plain(vec![inline])],
                    ..Pandoc::default()
                };
                assert_eq!(
                    write(&doc, &options, &mut context).is_ok(),
                    depth + depth / 2 + 1 < 48
                );

                let input = format!("{}x{}", "^{".repeat(depth), "}".repeat(depth));
                let input = format!("base{input}");
                assert_eq!(read(&input, &mut context).is_ok(), depth < 48);

                let mut blocks = vec![Block::Plain(vec![Inline::Str("text".into())])];
                for _ in 0..depth {
                    blocks = vec![Block::Div(Attr::default(), blocks)];
                }
                let doc = Pandoc {
                    blocks,
                    ..Pandoc::default()
                };
                assert_eq!(write(&doc, &options, &mut context).is_ok(), depth < 48);
            }
        })
        .map_err(|err| Box::new(err) as Box<dyn std::any::Any + Send>)?
        .join()
}

#[test]
fn standalone_template_and_notes() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut context = Context::pure();
    let doc = read(
        "#+TITLE: Example\n\nBody[fn:a].\n\n[fn:a] Note.",
        &mut context,
    )?;
    let mut options = WriterOptions::default();
    options.extensions = default_extensions("org");
    let template = randoc_data::default_template("org")
        .ok_or_else(|| io::Error::other("Missing Org template."))?;
    options.template = Some(Template::compile(
        "default.org",
        template,
        32,
        &mut |path| Err(io::Error::other(format!("Missing partial {path}."))),
    )?);
    let result = write(&doc, &options, &mut context)?;
    assert_eq!(result, "#+title: Example\n\nBody[fn:1].\n\n[fn:1] Note.\n");
    Ok(())
}

#[test]
fn export_options_apply_in_source_order() -> Result<()> {
    let input = r#"a_b \alpha

#+options: ^:nil e:nil tex:nil
a_b \alpha \textbf{hidden}

#+options: ^:t tex:verbatim
a_b \textbf{shown}
"#;
    let doc = read(input, &mut Context::pure())?;
    assert!(
        matches!(&doc.blocks[0], Block::Para(text) if matches!(text.as_slice(), [
        Inline::Str(base), Inline::Subscript(_), Inline::Space, Inline::Str(alpha),
    ] if base == "a" && alpha == "α"))
    );
    assert!(matches!(&doc.blocks[1], Block::Para(text) if stringify(text) == r"a_b \alpha"));
    assert!(
        matches!(&doc.blocks[2], Block::Para(text) if matches!(text.as_slice(), [
        Inline::Str(base), Inline::Subscript(_), Inline::Space, Inline::Str(command),
    ] if base == "a" && command == r"\textbf{shown}"))
    );
    Ok(())
}

#[test]
fn inline_footnote_labels_can_follow_references() -> Result<()> {
    let doc = read(
        "Before [fn:x] [fn:x: *bold* and ~code~] [fn:x].",
        &mut Context::pure(),
    )?;
    let [Block::Para(text)] = doc.blocks.as_slice() else {
        return Err(randoc_core::Error::App("Expected one paragraph.".into()));
    };
    let notes = text
        .iter()
        .filter_map(|inline| match inline {
            Inline::Note(note) => Some(note),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(notes.len(), 3);
    assert_eq!(notes[0], notes[1]);
    assert_eq!(notes[1], notes[2]);
    assert!(matches!(notes[0].as_slice(), [Block::Para(text)] if
        matches!(text.first(), Some(Inline::Strong(_)))
        && matches!(text.last(), Some(Inline::Str(code)) if code == "~code~")));
    Ok(())
}

#[test]
fn include_byte_limit_applies_before_line_slicing() -> Result<()> {
    let mut host = PureHost::default();
    host.files
        .insert("one".into(), b"line one\nline two\nline three\n".to_vec());
    let mut context = Context::new(Box::new(host));
    let input = "#+include: \"one\" src text :lines \"2-3\"";
    let doc = read(input, &mut context)?;
    assert!(
        matches!(doc.blocks.as_slice(), [Block::CodeBlock(attr, text)] if
        attr.classes == ["text"] && text == "line two\n")
    );
    context.limits.max_input_size = input.len() as u64 + 10;
    assert!(read(input, &mut context)?.blocks.is_empty());
    Ok(())
}

#[test]
fn writer_limits_multiline_table_projection() -> Result<()> {
    use randoc_ast::{Cell, ColSpec, Row, Table, TableBody};

    let lines = vec![
        Block::Para(vec![Inline::Str("First.".into())]),
        Block::Para(vec![Inline::Str("Second.".into())]),
    ];
    let table = Table {
        col_specs: vec![ColSpec::default()],
        bodies: vec![TableBody {
            body: vec![Row {
                cells: vec![Cell {
                    content: lines,
                    ..Cell::default()
                }],
                ..Row::default()
            }],
            ..TableBody::default()
        }],
        ..Table::default()
    };
    let doc = Pandoc {
        blocks: vec![Block::Table(Box::new(table))],
        ..Pandoc::default()
    };
    let mut context = Context::pure();
    context.limits.max_table_cells = 3;
    assert!(write(&doc, &WriterOptions::default(), &mut context).is_err());
    Ok(())
}

#[test]
fn link_expansion_charges_encoded_bytes_before_allocation() -> Result<()> {
    let input = "#+link: a https://x/%h%h\n[[a:é][A]] [[a:é][B]]";
    let mut context = Context::pure();
    let destination = "https://x/%C3%A9%h";
    context.limits.max_input_size = (input.len() + destination.len() * 2) as u64;
    let doc = read(input, &mut context)?;
    assert!(matches!(doc.blocks.as_slice(), [Block::Para(text)] if
        matches!(text.as_slice(), [Inline::Link(_, _, a), Inline::Space, Inline::Link(_, _, b)]
        if a.url == destination && b.url == destination)));
    context.limits.max_input_size -= 1;
    assert!(matches!(read(input, &mut context), Err(Error::Parse(_))));
    Ok(())
}

#[test]
fn oversized_includes_exhaust_the_budget_and_skip_repeated_reads() -> Result<()> {
    use std::sync::{Arc, Mutex};

    struct CountingHost(Arc<Mutex<Vec<String>>>);

    impl Host for CountingHost {
        fn read_file_limited(&mut self, path: &str, max_bytes: usize) -> io::Result<Vec<u8>> {
            self.0.lock().unwrap().push(path.into());
            match path {
                "dir/one" => Err(io::ErrorKind::NotFound.into()),
                "one" if max_bytes == 10 => Err(io::ErrorKind::FileTooLarge.into()),
                "other" => Err(io::Error::other("Host read failure.")),
                _ => Err(io::ErrorKind::NotFound.into()),
            }
        }
    }

    let reads = Arc::new(Mutex::new(Vec::new()));
    let mut context = Context::new(Box::new(CountingHost(reads.clone())));
    context.state.resource_path = vec!["./dir/.".into(), "dir".into()];
    let input = "#+include: \"one\" src text\n#+include: \"one\" src text";
    context.limits.max_input_size = input.len() as u64 + 10;
    assert!(read(input, &mut context)?.blocks.is_empty());
    assert_eq!(*reads.lock().unwrap(), ["dir/one", "one"]);
    assert!(context.log().is_empty());

    assert!(matches!(
        read("#+include: \"other\" src text", &mut context),
        Err(Error::App(message)) if message.contains("Host read failure.")
    ));
    Ok(())
}

#[test]
fn invalid_and_missing_includes_warn_and_keep_literal_blocks() -> Result<()> {
    use randoc_ast::Format;

    let mut host = PureHost::default();
    host.files
        .insert("invalid".into(), vec![b'a', 0xff, b'b', b'\n']);
    let mut context = Context::new(Box::new(host));
    let doc = read(
        "#+include: \"absent\" src sh\n#+include: \"invalid\" example\n#+include: \"invalid\" export html\n#+include: \"invalid\"",
        &mut context,
    )?;
    assert_eq!(
        doc.blocks,
        [
            Block::CodeBlock(Attr::new("", vec!["sh".into()], vec![]), String::new()),
            Block::CodeBlock(Attr::default(), String::new()),
            Block::RawBlock(Format::new("html"), String::new()),
        ]
    );
    assert_eq!(context.log().len(), 4);
    assert!(
        context
            .log()
            .iter()
            .all(|message| matches!(message, LogMessage::CouldNotLoadIncludeFile(_, _)))
    );
    Ok(())
}

#[test]
fn padded_table_output_has_a_cumulative_byte_limit() -> Result<()> {
    let mut context = Context::pure();
    let doc = read("| wide |\n| x |\n| x |", &mut context)?;
    let options = WriterOptions::default();
    const TABLE: &str = "| wide |\n| x    |\n| x    |";
    context.limits.max_input_size = (TABLE.len() + 1) as u64;
    assert_eq!(write(&doc, &options, &mut context)?, TABLE);
    context.limits.max_input_size -= 1;
    assert!(matches!(
        write(&doc, &options, &mut context),
        Err(Error::Parse(_))
    ));

    let mut twice = doc.clone();
    twice.blocks.extend(doc.blocks);
    context.limits.max_input_size = (TABLE.len() + 1) as u64;
    assert!(matches!(
        write(&twice, &options, &mut context),
        Err(Error::Parse(_))
    ));
    let mut context = Context::pure();
    let narrow_and_wide = format!("| {} |\n{}", "x".repeat(100_000), "| y |\n".repeat(1_000));
    let large = read(&narrow_and_wide, &mut context)?;
    assert!(matches!(
        write(&large, &options, &mut context),
        Err(Error::Parse(_))
    ));
    Ok(())
}

#[test]
fn ragged_table_charges_only_retained_cells() -> Result<()> {
    let mut context = Context::pure();
    context.limits.max_table_cells = 3;
    let input = format!("| <r> |\n| a |\n| b {}|", "|discarded".repeat(10_000));
    let doc = read(&input, &mut context)?;
    assert!(matches!(doc.blocks.as_slice(), [Block::Table(table)] if
        table.col_specs.len() == 1 && table.bodies[0].body.len() == 2
        && table.bodies[0].body.iter().all(|row| row.cells.len() == 1)));
    context.limits.max_table_cells -= 1;
    assert!(matches!(read(&input, &mut context), Err(Error::Parse(_))));
    Ok(())
}
