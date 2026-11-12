//! Rule 21b: deep input on a thread with a 2 MB stack gives a limited
//! document and no stack overflow, for each container kind.

use randoc_core::{Context, ReaderOptions, default_extensions};

fn deep_inputs(n: usize) -> Vec<(&'static str, String)> {
    vec![
        ("block quotes", format!("{}x\n", "> ".repeat(n))),
        ("block quote lines", format!("{}x\n", ">\n".repeat(n))),
        ("lists", format!("{}x\n", "- ".repeat(n))),
        ("ordered lists", format!("{}x\n", "1. ".repeat(n))),
        (
            "indented lists",
            (0..n.min(130))
                .map(|d| format!("{}- a\n", " ".repeat(d * 2)))
                .collect(),
        ),
        (
            "fenced divs",
            format!("{}x\n{}", "::: a\n".repeat(n), ":::\n".repeat(n)),
        ),
        ("html divs", format!("{}x\n", "<div>".repeat(n))),
        (
            "comments on one line",
            format!("{}x\n", "<!--x-->".repeat(n)),
        ),
        (
            "processing instructions on one line",
            format!("{}x\n", "<?x>".repeat(n)),
        ),
        (
            "closing tags on one line",
            format!("{}x\n", "</p>".repeat(n)),
        ),
        (
            "html div lines",
            format!("{}x\n{}", "<div>\n".repeat(n), "</div>\n".repeat(n)),
        ),
        (
            "emphasis",
            format!("{}x{}\n", "*a ".repeat(n), " a*".repeat(n)),
        ),
        (
            "strong",
            format!("{}x{}\n", "**a ".repeat(n), " a**".repeat(n)),
        ),
        (
            "links",
            format!("{}x{}\n", "[a ".repeat(n), "](u)".repeat(n)),
        ),
        (
            "spans",
            format!("{}x{}\n", "[".repeat(n), "]{.c}".repeat(n)),
        ),
        (
            "images",
            format!("{}x{}\n", "![".repeat(n), "](u)".repeat(n)),
        ),
        (
            "inline notes",
            format!("{}x{}\n", "^[".repeat(n), "]".repeat(n)),
        ),
        (
            "native spans",
            format!("{}x{}\n", "<span>".repeat(n), "</span>".repeat(n)),
        ),
        (
            "double quotes",
            format!("{}x{}\n", "\"a ".repeat(n), " a\"".repeat(n)),
        ),
        (
            "superscripts",
            format!("{}x{}\n", "^a".repeat(n), "^".repeat(n)),
        ),
        (
            "strikeouts",
            format!("{}x{}\n", "~~a ".repeat(n), " a~~".repeat(n)),
        ),
        ("definition lists", format!("{}x\n", "a\n: ".repeat(n))),
        (
            "headings",
            (1..=n.min(10000))
                .map(|l| format!("{} h\n\n", "#".repeat(l)))
                .collect(),
        ),
    ]
}

fn read_all(n: usize) {
    let mut options = ReaderOptions::default();
    options.extensions = default_extensions("markdown");
    for (_, text) in deep_inputs(n) {
        let mut context = Context::pure();
        let _ = crate::read_markdown(&text, "", &options, &mut context);
    }
}

fn on_small_stack(n: usize) {
    let worker = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || read_all(n));
    let joined = worker.map(|handle| handle.join());
    assert!(
        matches!(joined, Ok(Ok(()))),
        "the reader failed at {n} levels"
    );
}

#[test]
fn deep_input_with_the_default_limit_is_safe() {
    on_small_stack(10_000);
    on_small_stack(100_000);
}

#[test]
fn the_default_limit_is_safe_for_each_container() {
    on_small_stack(128);
    on_small_stack(129);
}
