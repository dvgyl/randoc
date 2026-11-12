//! Depth and time limits of the writer.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::time::{Duration, Instant};

use randoc_ast::{
    Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, Inline, ListAttributes, Pandoc,
    QuoteType, Row, Table, TableBody, Target,
};
use randoc_core::{Context, FormatSpec, WriterOptions};

const FORMATS: &[&str] = &[
    "markdown",
    "gfm",
    "commonmark",
    "plain",
    "markdown_strict",
    "markua",
];

fn write(doc: &Pandoc, format: &str, context: &mut Context) -> randoc_core::Result<()> {
    let spec = FormatSpec::parse(format);
    let mut options = WriterOptions::default();
    options.extensions = spec.extensions().unwrap();
    options.reference_links = true;
    let writer = randoc_markdown_writer::writer(&spec.name).unwrap();
    writer.write(doc, &options, context).map(|_| ())
}

fn cell(blocks: Vec<Block>) -> Cell {
    Cell {
        attr: Attr::default(),
        align: randoc_ast::Alignment::AlignDefault,
        row_span: 1,
        col_span: 1,
        content: blocks,
    }
}

fn table(blocks: Vec<Block>) -> Block {
    Block::Table(Box::new(Table {
        attr: Attr::default(),
        caption: Caption::default(),
        col_specs: vec![ColSpec::default()],
        head: Default::default(),
        bodies: vec![TableBody {
            attr: Attr::default(),
            row_head_columns: 0,
            head: Vec::new(),
            body: vec![Row {
                attr: Attr::default(),
                cells: vec![cell(blocks)],
            }],
        }],
        foot: Default::default(),
    }))
}

/// Each kind of inline container, as a function that wraps its content.
type InlineKind = (&'static str, fn(Vec<Inline>) -> Inline);
type BlockKind = (&'static str, fn(Vec<Block>) -> Block);

fn inline_kinds() -> Vec<InlineKind> {
    vec![
        ("emph", Inline::Emph),
        ("strong", Inline::Strong),
        ("strikeout", Inline::Strikeout),
        ("superscript", Inline::Superscript),
        ("subscript", Inline::Subscript),
        ("smallcaps", Inline::SmallCaps),
        ("underline", Inline::Underline),
        ("quoted", |c| Inline::Quoted(QuoteType::DoubleQuote, c)),
        ("span", |c| Inline::Span(Attr::new("i", vec![], vec![]), c)),
        ("link", |c| {
            Inline::Link(Attr::default(), c, Target::new("u", ""))
        }),
        ("image", |c| {
            Inline::Image(Attr::default(), c, Target::new("u", ""))
        }),
        ("note", |c| Inline::Note(vec![Block::Para(c)])),
        ("cite", |c| {
            Inline::Cite(
                vec![Citation {
                    id: "k".into(),
                    prefix: Vec::new(),
                    suffix: Vec::new(),
                    mode: CitationMode::NormalCitation,
                    note_num: 0,
                    hash: 0,
                }],
                c,
            )
        }),
    ]
}

/// Each kind of block container, as a function that wraps its content.
fn block_kinds() -> Vec<BlockKind> {
    vec![
        ("blockquote", Block::BlockQuote),
        ("div", |c| Block::Div(Attr::new("d", vec![], vec![]), c)),
        ("bullet", |c| Block::BulletList(vec![c])),
        ("ordered", |c| {
            Block::OrderedList(ListAttributes::default(), vec![c])
        }),
        ("definition", |c| {
            Block::DefinitionList(vec![(vec![Inline::Str("t".into())], vec![c])])
        }),
        ("figure", |c| {
            Block::Figure(Attr::default(), Caption::default(), c)
        }),
        ("table", table),
        ("note", |c| Block::Para(vec![Inline::Note(c)])),
    ]
}

fn nested_inlines(wrap: fn(Vec<Inline>) -> Inline, depth: usize) -> Pandoc {
    let mut inner = vec![Inline::Str("x".into())];
    for _ in 0..depth {
        inner = vec![wrap(inner)];
    }
    Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Para(inner)],
    }
}

fn nested_blocks(wrap: fn(Vec<Block>) -> Block, depth: usize) -> Pandoc {
    let mut inner = vec![Block::Para(vec![Inline::Str("x".into())])];
    for _ in 0..depth {
        inner = vec![wrap(inner)];
    }
    Pandoc {
        meta: Default::default(),
        blocks: inner,
    }
}

/// Runs a check on a thread with a 2 MB stack.
fn on_small_stack(check: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(check)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn deep_documents_give_the_depth_error_on_a_small_stack() {
    on_small_stack(|| {
        for depth in [10_000, 100_000] {
            for (name, wrap) in inline_kinds() {
                let doc = nested_inlines(wrap, depth);
                for format in FORMATS {
                    let result = write(&doc, format, &mut Context::pure());
                    // A citation in a format with citations does not write
                    // its fallback content, so the result is correct output.
                    let uses_fallback = name != "cite" || !format.starts_with("markdown");
                    assert!(result.is_err() || !uses_fallback, "{name} {depth} {format}");
                }
                // The tree type has no iterative drop, and a drop at this
                // depth needs a large stack.
                std::mem::forget(doc);
            }
            for (name, wrap) in block_kinds() {
                let doc = nested_blocks(wrap, depth);
                for format in FORMATS {
                    let result = write(&doc, format, &mut Context::pure());
                    assert!(result.is_err(), "{name} {depth} {format}");
                }
                std::mem::forget(doc);
            }
        }
    });
}

#[test]
fn the_default_limit_is_safe_on_a_small_stack() {
    on_small_stack(|| {
        let depth = Context::pure().limits.max_nesting_depth;
        for (name, wrap) in inline_kinds() {
            let doc = nested_inlines(wrap, depth - 3);
            for format in FORMATS {
                let result = write(&doc, format, &mut Context::pure());
                assert!(
                    result.is_ok() || name == "note" || name == "table",
                    "{name} {format}"
                );
            }
        }
        for (name, wrap) in block_kinds() {
            let doc = nested_blocks(wrap, depth - 3);
            for format in FORMATS {
                let result = write(&doc, format, &mut Context::pure());
                // A note in a paragraph, and a table or a figure in HTML
                // form, count two levels for each nesting level.
                assert!(
                    result.is_ok() || matches!(name, "note" | "table" | "figure"),
                    "{name} {format}"
                );
            }
        }
    });
}

/// The inputs that repeat one construct `n` times.
fn repeated(kind: &str, n: usize) -> Pandoc {
    let s = |t: &str| Inline::Str(t.into());
    let para = |inlines: Vec<Inline>| Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Para(inlines)],
    };
    match kind {
        "ampersands" => para(vec![s(&"&".repeat(n))]),
        "specials" => para(vec![s(&"*_`[]<>$^~|'\"-.@#\\".repeat(n / 16 + 1))]),
        "words" => para((0..n).flat_map(|_| [s("w"), Inline::Space]).collect()),
        "links" => para(
            (0..n)
                .map(|i| {
                    Inline::Link(
                        Attr::default(),
                        vec![s(&format!("l{i}"))],
                        Target::new(format!("u{i}"), ""),
                    )
                })
                .collect(),
        ),
        "notes" => para(
            (0..n)
                .map(|_| Inline::Note(vec![Block::Para(vec![s("n")])]))
                .collect(),
        ),
        "emph" => para((0..n).map(|_| Inline::Emph(vec![s("e")])).collect()),
        "attributes" => para(
            (0..n)
                .map(|i| {
                    Inline::Span(
                        Attr::new(
                            format!("i{i}"),
                            vec!["c".into()],
                            vec![("k".into(), "v".into())],
                        ),
                        vec![s("x")],
                    )
                })
                .collect(),
        ),
        "lists" => Pandoc {
            meta: Default::default(),
            blocks: (0..n)
                .map(|_| Block::BulletList(vec![vec![Block::Plain(vec![s("i")])]]))
                .collect(),
        },
        "items" => Pandoc {
            meta: Default::default(),
            blocks: vec![Block::OrderedList(
                ListAttributes::default(),
                (0..n).map(|_| vec![Block::Plain(vec![s("i")])]).collect(),
            )],
        },
        "headings" => Pandoc {
            meta: Default::default(),
            blocks: (0..n)
                .map(|i| {
                    Block::Header(
                        i64::try_from(i % 6 + 1).unwrap(),
                        Attr::new(format!("h{i}"), vec![], vec![]),
                        vec![s("h")],
                    )
                })
                .collect(),
        },
        "table rows" => {
            let Block::Table(mut t) = table(vec![Block::Plain(vec![s("c")])]) else {
                unreachable!()
            };
            let row = t.bodies[0].body[0].clone();
            t.bodies[0].body = vec![row; n];
            Pandoc {
                meta: Default::default(),
                blocks: vec![Block::Table(t)],
            }
        }
        "divs" => nested_blocks(|c| Block::Div(Attr::default(), c), n),
        "quotes" => nested_blocks(Block::BlockQuote, n),
        "nested emph" => nested_inlines(Inline::Emph, n),
        "nested superscript" => nested_inlines(Inline::Superscript, n),
        _ => unreachable!(),
    }
}

const KINDS: &[&str] = &[
    "ampersands",
    "specials",
    "words",
    "links",
    "notes",
    "emph",
    "attributes",
    "lists",
    "items",
    "headings",
    "table rows",
    "divs",
    "quotes",
    "nested emph",
    "nested superscript",
];

/// Returns the shortest time of three runs of the writer on the input.
fn best_time(doc: &Pandoc, format: &str) -> Duration {
    let mut context = Context::pure();
    context.limits.max_nesting_depth = 1_000_000;
    (0..3)
        .map(|_| {
            let start = Instant::now();
            let _ = write(doc, format, &mut context);
            start.elapsed()
        })
        .min()
        .unwrap_or_default()
}

fn time_ratios(small: usize, print: bool) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    for kind in KINDS {
        // Nested inputs do not grow the stack of the renderer, but the
        // writer recurses once for each level.
        let (small, large) = if kind.starts_with("nested") || *kind == "divs" || *kind == "quotes" {
            (small / 10, small * 4 / 10)
        } else {
            (small, small * 4)
        };
        let small_doc = repeated(kind, small);
        let large_doc = repeated(kind, large);
        for format in ["markdown", "gfm"] {
            let a = best_time(&small_doc, format);
            let b = best_time(&large_doc, format);
            let ratio = b.as_secs_f64() / a.as_secs_f64().max(1e-6);
            if print {
                println!(
                    "{kind:20} {format:9} n={small:6}: {a:>10.2?}  n={large:6}: {b:>10.2?}  ratio {ratio:5.1}"
                );
            }
            out.push((format!("{kind} {format}"), ratio));
        }
        std::mem::forget(small_doc);
        std::mem::forget(large_doc);
    }
    out
}

fn on_large_stack(check: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(check)
        .unwrap()
        .join()
        .unwrap();
}

/// The measurement for the report: 12,500 and 50,000 repetitions.
#[test]
#[ignore]
fn measure_time_at_50000_repetitions() {
    on_large_stack(|| {
        time_ratios(12_500, true);
    });
}
