//! Checks each cache and index of the reader against the plain scan that it
//! replaces. In test builds each cached answer is compared with a new plain
//! scan, and each difference counts as a mismatch.

use randoc_core::{Context, ReaderOptions, default_extensions};

/// The parts of the generated inputs: HTML elements whose content is
/// Markdown, and the tokens of the content that can hold a literal tag.
const PARTS: &[&str] = &[
    "<div>",
    "</div>",
    "<DIV class=\"a\">",
    "<section>",
    "</section>",
    "<p>",
    "</p>",
    "<span>",
    "</span>",
    "<span title=\"</span>\">",
    "<b title=\"</div>\">",
    "`</div>`",
    "`</span>`",
    "``",
    "`",
    "$</div>$",
    "$",
    "\\</div>",
    "\\</span>",
    "<!-- </div> -->",
    "<!--",
    "-->",
    "x",
    "y z",
    " ",
    "\n",
    "\n",
    "\n\n",
    "\n```\n",
    "\n~~~\n",
    "\n> ",
    "\n- ",
    "\n    ",
    "\n  ",
    "\n::: c\n",
    "\n:::\n",
    "<div>x</div>",
    "<p></p>",
];

/// The formats of the runs: each one reads raw HTML in a different way.
const FORMATS: &[&str] = &[
    "markdown",
    "markdown-markdown_in_html_blocks",
    "markdown_phpextra",
    "markdown_github",
];

/// Returns `count` inputs of at most 14 parts each, from a fixed seed.
fn inputs(count: usize, seed: u64) -> Vec<String> {
    let mut state = seed;
    let mut next = move |bound: usize| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(state >> 33).unwrap_or(0) % bound
    };
    (0..count)
        .map(|_| {
            let parts = 1 + next(14);
            (0..parts).map(|_| PARTS[next(PARTS.len())]).collect()
        })
        .collect()
}

#[test]
fn caches_agree_with_the_plain_scans() {
    let cases = inputs(24_000, 20_261_112);
    let mut mismatches = Vec::new();
    for (k, text) in cases.iter().enumerate() {
        let format = FORMATS[k % FORMATS.len()];
        let mut options = ReaderOptions::default();
        options.extensions = default_extensions(format);
        let mut context = Context::pure();
        crate::CACHE_MISMATCHES.with(|m| m.set(Some(0)));
        let _ = crate::read_markdown(text, "", &options, &mut context);
        let count = crate::CACHE_MISMATCHES.with(|m| m.replace(None));
        if count != Some(0) {
            mismatches.push((format, text.clone()));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} inputs: {:?}",
        mismatches.len(),
        cases.len(),
        mismatches.get(..5)
    );
}
