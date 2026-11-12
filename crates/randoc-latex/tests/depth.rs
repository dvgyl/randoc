//! Deep documents on a thread with a 2 MB stack and the default nesting
//! limit. The writer must return the depth error for very deep input, and it
//! must not overflow the stack for input at the limit.

// Test code: a failed thread must stop the test.
#![allow(clippy::unwrap_used)]

use randoc_ast::{
    Attr, Block, Caption, Cell, ColSpec, Inline, ListAttributes, Pandoc, Row, Table, TableBody,
    Target,
};
use randoc_core::{Context, Result, WriterOptions, default_extensions};

/// The container kinds that can nest without a limit in the document tree.
const KINDS: &[&str] = &[
    "bullet-list",
    "ordered-list",
    "block-quote",
    "div",
    "table-cell",
    "definition-list",
    "figure",
    "note",
    "emphasis",
    "span",
    "link",
    "image-description",
    "sections",
];

fn text() -> Inline {
    Inline::Str("x".into())
}

fn nested_inlines(kind: &str, depth: usize) -> Inline {
    let mut inline = text();
    for _ in 0..depth {
        inline = match kind {
            "emphasis" => Inline::Emph(vec![inline]),
            "span" => Inline::Span(Attr::default(), vec![inline]),
            "link" => Inline::Link(Attr::default(), vec![inline], Target::new("u", "")),
            _ => Inline::Note(vec![Block::Para(vec![inline])]),
        };
    }
    inline
}

fn one_cell(content: Block) -> Block {
    Block::Table(Box::new(Table {
        col_specs: vec![ColSpec::default()],
        bodies: vec![TableBody {
            body: vec![Row {
                attr: Attr::default(),
                cells: vec![Cell {
                    content: vec![content],
                    ..Default::default()
                }],
            }],
            ..Default::default()
        }],
        ..Default::default()
    }))
}

fn document(kind: &str, depth: usize) -> Pandoc {
    let blocks = match kind {
        "emphasis" | "span" | "link" | "note" => {
            vec![Block::Para(vec![nested_inlines(kind, depth)])]
        }
        "image-description" => vec![Block::Para(vec![Inline::Image(
            Attr::default(),
            vec![nested_inlines("emphasis", depth)],
            Target::new("a.png", ""),
        )])],
        "sections" => {
            let mut blocks: Vec<Block> = (1..=depth)
                .map(|level| Block::Header(level as i64, Attr::default(), vec![text()]))
                .collect();
            blocks.push(Block::Para(vec![text()]));
            blocks
        }
        _ => {
            let mut block = Block::Para(vec![text()]);
            for _ in 0..depth {
                block = match kind {
                    "bullet-list" => Block::BulletList(vec![vec![block]]),
                    "ordered-list" => {
                        Block::OrderedList(ListAttributes::default(), vec![vec![block]])
                    }
                    "block-quote" => Block::BlockQuote(vec![block]),
                    "div" => Block::Div(Attr::default(), vec![block]),
                    "table-cell" => one_cell(block),
                    "definition-list" => {
                        Block::DefinitionList(vec![(vec![text()], vec![vec![block]])])
                    }
                    _ => Block::Figure(Attr::default(), Caption::default(), vec![block]),
                };
            }
            vec![block]
        }
    };
    Pandoc {
        blocks,
        ..Default::default()
    }
}

/// Writes the document on a thread with a 2 MB stack. The input is not
/// dropped there, because the drop of the input tree is not under test.
fn write_on_small_stack(kind: &'static str, depth: usize, beamer: bool) -> Result<String> {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let doc = document(kind, depth);
            let mut options = WriterOptions::default();
            options.extensions = default_extensions("latex");
            let mut context = Context::pure();
            let result = if beamer {
                randoc_latex::write_beamer(&doc, &options, &mut context)
            } else {
                randoc_latex::write_latex(&doc, &options, &mut context)
            };
            std::mem::forget(doc);
            result
        })
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn very_deep_input_gives_the_depth_error() {
    for kind in KINDS {
        for depth in [10_000, 100_000] {
            for beamer in [false, true] {
                let result = write_on_small_stack(kind, depth, beamer);
                if *kind == "sections" && !beamer {
                    // LaTeX headings do not nest.
                    assert!(result.is_ok(), "flat headings at depth {depth}");
                    continue;
                }
                if *kind == "image-description" {
                    // The shared `stringify` walks the description with a heap stack.
                    let output = result.unwrap_or_default();
                    assert!(
                        output.contains("alt={x}"),
                        "image at depth {depth}, beamer {beamer}"
                    );
                    continue;
                }
                let error = result.err().map(|error| error.to_string());
                assert_eq!(
                    error.as_deref(),
                    Some("the document is nested too deeply for the LaTeX writer"),
                    "{kind} at depth {depth}, beamer {beamer}"
                );
            }
        }
    }
}

#[test]
fn input_at_the_default_limit_fits_on_the_stack() {
    let limit = Context::pure().limits.max_nesting_depth;
    for kind in KINDS {
        for depth in [limit / 2, limit - 2, limit - 1, limit, limit + 1] {
            for beamer in [false, true] {
                // An error is permitted here; a stack overflow ends the test process.
                let _ = write_on_small_stack(kind, depth, beamer);
            }
        }
    }
}

#[test]
fn moderate_depth_writes_output() {
    for kind in KINDS {
        for beamer in [false, true] {
            let result = write_on_small_stack(kind, 20, beamer);
            assert!(result.is_ok(), "{kind}, beamer {beamer}: {result:?}");
        }
    }
}
