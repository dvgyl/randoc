//! Checks each cache and index of the reader against the plain scan that it
//! replaces. In test builds each cached answer is compared with a new plain
//! scan, and each difference counts as a mismatch.

use randoc_core::{Context, Extension, FormatSpec, ReaderOptions, default_extensions};

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
    "<!--x-->",
    "<?p",
    ">",
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
    "\n---\nk: |\n  ::: c\n  ::: d\n  x\n  :::\n---\n",
    "\n::: c\n```\n:::\n```\n",
    "`a\n</div>\nb`",
    "``a\n</span>\nb``",
    "q ```a\n</section>\nb```",
    "$a\n</div>\nb$",
    "$$a\n</div>\nb$$",
    "\\(a\n</div>\nb\\)",
    "\\[a\n</span>\nb\\]",
    "\\\\(a\n</section>\nb\\\\)",
    "\\\\[a\n</div>\nb\\\\]",
    "<!-- --> <!--\n</div>\n-->",
    "<b title=\"a\n</div>\nb\">",
    "[a]{title=\"b\n</div>\nc\"}",
    "[a](route</div>tail)",
    "[a](route \"b\n</div>\nc\")",
    "`a\n</div>\nb`{=html}",
];

/// The formats of the runs: each one reads raw HTML in a different way.
const FORMATS: &[&str] = &[
    "markdown",
    "markdown-markdown_in_html_blocks",
    "markdown_phpextra",
    "markdown_github",
    "markdown+tex_math_single_backslash+tex_math_double_backslash",
    "markdown-native_divs-native_spans",
    "markdown-native_divs+native_spans",
    "markdown+native_divs-native_spans",
    "markdown-markdown_in_html_blocks-native_divs-native_spans",
    "markdown-markdown_in_html_blocks-native_divs+native_spans",
    "markdown-markdown_in_html_blocks+native_divs-native_spans",
    "markdown-markdown_in_html_blocks+native_divs+native_spans",
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
        let spec = FormatSpec::parse(format);
        let mut options = ReaderOptions::default();
        options.extensions = default_extensions(&spec.name);
        for (enabled, name) in &spec.changes {
            if let Some(extension) = Extension::from_name(name) {
                if *enabled {
                    options.extensions.enable(extension);
                } else {
                    options.extensions.disable(extension);
                }
            }
        }
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
        mismatches.iter().take(5).collect::<Vec<_>>()
    );
}
