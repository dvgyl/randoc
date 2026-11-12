//! Checks that the reading time grows about linearly with the number of
//! repeated delimiters, for each delimiter kind of the round 1 reviews.

use std::time::{Duration, Instant};

use randoc_core::{Context, ReaderOptions, default_extensions};

const PATTERNS: &[(&str, &str)] = &[
    ("single quotes", "'a "),
    ("double quotes", "\"a "),
    ("fenced div openers", "::: a\n"),
    ("div tags", "<div>"),
    ("div tag lines", "<div>\n"),
    ("dollars", "$a "),
    ("angle brackets", "<"),
    ("tags with attributes", "<a "),
    ("span tags", "<span>"),
    ("comments", "<!--"),
    ("block quote markers", "> "),
    ("list markers", "- "),
    ("brackets", "["),
    ("link openers", "[a]("),
    ("images", "!["),
    ("wiki brackets", "[["),
    ("backticks", "`"),
    ("code with attributes", "`a`{"),
    ("environments", "\\begin{a}"),
    ("citations", "[@a; "),
    ("at signs", "@"),
    ("inline notes", "^["),
    ("emphasis and brackets", "*[a"),
    ("tildes", "~a "),
    ("carets", "^a "),
    ("paragraphs", "x\n\n"),
    ("headings", "# a\n\n"),
    ("unclosed backtick fences", "```a\n"),
    ("unclosed tilde fences", "~~~a\n"),
    ("task items", "- [ ] - a\n"),
    ("definitions with list markers", "Term\n: x\n  - y\n"),
    (
        "indented block tags after paragraphs",
        "a\n    <p>x</p>\n\n",
    ),
    ("headings with reference links", "# A [t][ref] b [^h]\n\n"),
    ("block tags after text", "x <p>y</p> "),
    ("block tags after text lines", "x <p>y</p>\n"),
    ("div tags after text", "x <div>y "),
    ("block tags after backticks", "` x <p>"),
    ("block tags after backtick runs", "`` x ` <p>"),
    ("processing instructions", "a <?x "),
    ("undefined notes", "[^x] "),
    ("math environments", "\\begin{equation}\n"),
    ("block tags in link text", "[x <p>y</p>](u) "),
    ("block tags in nested brackets", "[x [b <p>y "),
    ("spans with spaces", "[ a ]{.c} <span> b </span>"),
    ("open comments before blank lines", "x <!--a\n\n"),
    (
        "open processing instructions before blank lines",
        "x <?p a\n\n",
    ),
    ("comments over blank lines", "x <!--a\n\nb--> y\n\n"),
    ("list markers after definitions", "q\n:   a\n- b\n\n"),
    ("fences after definitions", "q\n:   a\n```\n"),
    ("undefined reference labels", "[a][b](u) "),
    ("undefined spaced reference labels", "[a] [b] "),
    ("headings with citations", "@d\n\n# @t\n\n"),
    ("loose items before sublists", "- a\n\n  - s\n"),
    ("citations before note brackets", "@b [^x] "),
    ("div tags with code closers", "<div>`</div>` "),
    ("div tags with math closers", "<div>$</div>$ "),
    ("div tags with escaped closers", "<div>\\</div> "),
    ("div lines with fences", "<div>\n```\n"),
    ("span tags with code closers", "<span>`</span>` "),
    ("span tags with math closers", "<span>$</span>$ "),
];

/// Returns the work count of one read with the program depth limit.
fn read_steps(text: &str) -> u64 {
    read_steps_limited(text, 16384)
}

/// Returns the work count of one read with the depth limit `limit`.
fn read_steps_limited(text: &str, limit: usize) -> u64 {
    read_steps_in("markdown", text, limit)
}

/// Returns the work count of one read of `format` with the depth limit `limit`.
fn read_steps_in(format: &str, text: &str, limit: usize) -> u64 {
    let mut options = ReaderOptions::default();
    let mut parts = format.split('+');
    options.extensions = default_extensions(parts.next().unwrap_or("markdown"));
    for name in parts {
        if let Some(extension) = randoc_core::Extension::from_name(name) {
            options.extensions.enable(extension);
        }
    }
    let mut context = Context::pure();
    context.limits.max_nesting_depth = limit;
    crate::STEPS.with(|s| s.set(0));
    let _ = crate::read_markdown(text, "", &options, &mut context);
    crate::STEPS.with(|s| s.get())
}

/// A repeated text and an end text for each form.
const FORMS: &[(&str, &str, &str)] = &[
    ("single quotes, closer at the end", "'a ", "'b'"),
    ("double quotes, closer at the end", "\"a ", "\"b\""),
    ("superscripts, closer at the end", "^a ", "^b^"),
    ("subscripts, closer at the end", "~a ", "~b~"),
    ("strikeouts, closer at the end", "~~a ", "~~b~~"),
    ("emphasis, closer at the end", "*a ", "b*"),
    ("strong, closer at the end", "**a ", "b**"),
    ("attributes, closer at the end", "[a]{.c ", "}"),
    ("code attributes, closer at the end", "`a`{x=\"y\" ", "}"),
    ("attribute quotes, closer at the end", "[a]{x=\"", "}"),
    ("native spans, closer at the end", "<span>", "</span>"),
    ("link destinations, closer at the end", "[x](a(", "x)"),
    ("link titles, closer at the end", "[x](u \"", "x\")"),
    ("math, closer at the end", "$a ", "$b$"),
    ("inline notes, closer at the end", "^[a ", "]"),
    ("tag attributes, closer at the end", "<a title=\"", "\">"),
    ("single quotes, closer after each", "'a' ", ""),
    ("double quotes, closer after each", "\"a\" ", ""),
    ("emphasis, closer after each", "*a* ", ""),
    ("links, closer after each", "[a](u) ", ""),
    ("native spans, closer after each", "<span>a</span>", ""),
    ("code attributes, closer after each", "`a`{.c} ", ""),
    ("superscripts, closer after each", "^a^ ", ""),
    ("bracketed spans, closer after each", "[a]{.c} ", ""),
    ("math, closer after each", "$a$ ", ""),
    ("quotes alternating", "'a \"b ", ""),
    ("three quotes alternating", "'a \"b ^c ", ""),
    ("scripts alternating", "^a ~b ", ""),
    ("brackets and scripts alternating", "[a ^b ", ""),
    ("strikeout and subscript alternating", "~~a ^b ", ""),
    ("bare characters with URIs", "!", ""),
    ("two emphasis kinds alternating", "*a _b ", ""),
    ("emphasis and strong alternating", "*a **b ", ""),
    ("two strong kinds alternating", "**a __b ", ""),
    ("three emphasis kinds alternating", "*a _b **c ", ""),
    ("strikeout and superscript alternating", "~~a ^b ", ""),
    (
        "emphasis, subscript and superscript alternating",
        "*a ~b ^c ",
        "",
    ),
];

fn read_time(text: &str) -> Duration {
    let mut options = ReaderOptions::default();
    options.extensions = default_extensions("markdown");
    let mut best = Duration::MAX;
    for _ in 0..5 {
        let mut context = Context::pure();
        context.limits.max_nesting_depth = 16384;
        let start = Instant::now();
        let _ = crate::read_markdown(text, "", &options, &mut context);
        best = best.min(start.elapsed());
    }
    best
}

/// Returns `None` when the larger input (4 times as large) needs at most 6
/// times the work. Linear growth gives a factor of about 4 and quadratic
/// growth a factor of about 16. The work count does not depend on the load
/// of the machine.
fn compare(small_text: &str, large_text: &str) -> Option<String> {
    let small = read_steps(small_text);
    let large = read_steps(large_text);
    (large > small.saturating_mul(6)).then(|| format!("{small} steps then {large} steps"))
}

/// The inputs of review round 3 that grew faster than linear: a name, the
/// format, and the input for a number of repetitions.
type Round3Form = (&'static str, &'static str, fn(usize) -> String);

const ROUND_3_FORMS: &[Round3Form] = &[
    ("bare URI parentheses", "markdown+autolink_bare_uris", |n| {
        format!("https://x/{}", ")".repeat(n))
    }),
    (
        "unclosed backslash math",
        "markdown+tex_math_single_backslash",
        |n| format!("p {}x", "\\(".repeat(n)),
    ),
    ("simple table columns", "markdown", |n| {
        format!(
            "{}\n{}\n{}\n\n",
            "a ".repeat(n),
            "- ".repeat(n),
            "b ".repeat(n)
        )
    }),
    ("environment end names", "markdown", |n| {
        format!("p \\begin{{x}}{}}}", "\\end{".repeat(n))
    }),
    ("TeX command arguments without closers", "markdown", |n| {
        format!("p {}}}", "\\end{".repeat(n))
    }),
    ("reference groups without closers", "markdown", |n| {
        format!("[x]: u {}\n\n[x]\n", "(x ".repeat(n))
    }),
    (
        "reference attribute groups without closers",
        "markdown",
        |n| format!("[x]: u {}\n\n[x]\n", "{x ".repeat(n)),
    ),
    ("comments on one line", "markdown", |n| "<!--x-->".repeat(n)),
    ("processing instructions on one line", "markdown", |n| {
        "<?x>".repeat(n)
    }),
    ("closing tags on one line", "markdown", |n| "</p>".repeat(n)),
    ("unclosed comments in html divs", "markdown", |n| {
        "<div>\n<!--\n".repeat(n)
    }),
    ("comments before text on many lines", "markdown", |n| {
        "<!--x-->x\n\n".repeat(n)
    }),
    ("fenced divs in metadata", "markdown", |n| {
        format!(
            "---\nkey: \"{}x\\n{}\"\n---\n",
            "::: d\\n".repeat(n),
            ":::\\n".repeat(n)
        )
    }),
];

fn check_all() -> Vec<String> {
    let mut slow = Vec::new();
    for (name, format, make) in ROUND_3_FORMS {
        let small = read_steps_in(format, &make(250), 16384);
        let large = read_steps_in(format, &make(1000), 16384);
        if large > small.saturating_mul(6) {
            slow.push(format!("{name}: {small} steps then {large} steps"));
        }
    }
    for (name, pattern) in PATTERNS {
        let small = format!("{}x\n", pattern.repeat(1000));
        let large = format!("{}x\n", pattern.repeat(4000));
        if let Some(times) = compare(&small, &large) {
            slow.push(format!("{name}: {times}"));
        }
    }
    // Nested containers over many lines: the depth stays the same, and the
    // number of lines grows.
    let quotes = |lines: usize| "> ".repeat(50) + &"x\n".repeat(lines);
    if let Some(times) = compare(&quotes(200), &quotes(800)) {
        slow.push(format!("nested block quotes: {times}"));
    }
    let lists = |lines: usize| {
        let mut text = String::new();
        for depth in 0..30 {
            text.push_str(&" ".repeat(depth * 2));
            text.push_str("- a\n");
        }
        text.push_str(&format!("{}b\n", " ".repeat(60)).repeat(lines));
        text
    };
    if let Some(times) = compare(&lists(200), &lists(800)) {
        slow.push(format!("nested lists: {times}"));
    }
    // Lazy lines of one list item, each a fence opener with no closer.
    let fences = |lines: usize| format!("- x\n{}", "```a\n".repeat(lines));
    if let Some(times) = compare(&fences(1000), &fences(4000)) {
        slow.push(format!("unclosed fences as lazy lines: {times}"));
    }
    // Lists nested by indentation: each line is one level deeper. Twice the
    // lines give four times the bytes, so linear growth gives a factor of 4.
    for (name, marker, step) in [
        ("bullet lists nested by indentation", "- a\n", 2),
        ("ordered lists nested by indentation", "1. a\n", 3),
        ("bullet lists nested by four spaces", "- a\n", 4),
    ] {
        let stairs = |lines: usize| -> String {
            (0..lines)
                .map(|depth| " ".repeat(step * depth) + marker)
                .collect()
        };
        if let Some(times) = compare(&stairs(100), &stairs(200)) {
            slow.push(format!("{name}: {times}"));
        }
    }
    // Each construct with one closer at the very end, with a closer after
    // each opener, and two constructs that alternate.
    for (name, repeated, end) in FORMS {
        let small = format!("{}{end}\n", repeated.repeat(1000));
        let large = format!("{}{end}\n", repeated.repeat(4000));
        if let Some(times) = compare(&small, &large) {
            slow.push(format!("{name}: {times}"));
        }
    }
    let nested = |n: usize, open: &str, inner: &str, close: &str| {
        format!("{}{inner}{}", open.repeat(n), close.repeat(n))
    };
    for (name, open, inner, close) in [
        ("nested links", "[a ", "x", "](u)"),
        ("nested images", "![a ", "x", "](u)"),
        ("nested bracketed spans", "[a ", "x", "]{.c}"),
        ("nested native spans", "<span>a ", "x", "</span>"),
        ("nested inline notes", "^[a ", "x", "]"),
        ("nested fenced divs with closers", "::: d\n", "x\n", ":::\n"),
        (
            "nested HTML divs with closers",
            "<div>\n",
            "a\n",
            "</div>\n",
        ),
        (
            "nested raw elements with indented lines",
            " <section>\n",
            " a\n",
            " </section>\n",
        ),
        (
            "nested raw elements with indented lines and no closer",
            " <section>\n",
            " a\n",
            "",
        ),
    ] {
        if let Some(times) = compare(
            &nested(1000, open, inner, close),
            &nested(4000, open, inner, close),
        ) {
            slow.push(format!("{name}: {times}"));
        }
    }
    let envs = |n: usize| -> String {
        let mut text = String::from("p ");
        for i in 0..n {
            text.push_str(&format!("\\begin{{env{i}}}"));
        }
        text
    };
    if let Some(times) = compare(&envs(1000), &envs(4000)) {
        slow.push(format!("different environment names: {times}"));
    }
    let defs: String = (0..1000).map(|i| format!("[r{i}]: u{i}\n")).collect();
    let more: String = (0..4000).map(|i| format!("[r{i}]: u{i}\n")).collect();
    if let Some(times) = compare(&defs, &more) {
        slow.push(format!("reference definitions: {times}"));
    }
    slow
}

#[test]
fn time_grows_about_linearly() {
    let worker = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(check_all);
    // A failure to start the worker or a panic in it fails the test.
    let joined = worker.map(|handle| handle.join());
    assert!(matches!(&joined, Ok(Ok(_))), "the time test worker failed");
    if let Ok(Ok(slow)) = joined {
        assert!(slow.is_empty(), "{slow:?}");
    }
}

#[test]
#[ignore]
fn print_times() {
    for (name, pattern) in PATTERNS {
        for n in [1000, 4000, 16000] {
            let text = format!("{}x\n", pattern.repeat(n));
            println!("{name} {n} {:?} {}", read_time(&text), read_steps(&text));
        }
    }
}

/// Prints the work count of each form of the rule 21c table at 12,500 and
/// 50,000 repetitions with the depth limit of the program.
#[test]
#[ignore]
fn print_rule_21c_steps() {
    let worker = std::thread::Builder::new().stack_size(1 << 30).spawn(|| {
        for form in ["__a ~~b ", "~~a _b ", "_a \"b ", "\"a __b ", "*a _b "] {
            for n in [4000, 8000, 12_500, 16_000, 32_000, 50_000, 64_000] {
                let text = format!("{}\n", form.repeat(n));
                let start = Instant::now();
                let steps = read_steps(&text);
                println!(
                    "{form:?} {n} {steps} steps, {} per unit, {:?}",
                    steps / n as u64,
                    start.elapsed()
                );
            }
        }
    });
    assert!(matches!(worker.map(|handle| handle.join()), Ok(Ok(()))));
}

/// The openers of rule 21a, each followed by a word.
const OPENERS: &[&str] = &[
    "*a ", "_a ", "**a ", "__a ", "~~a ", "^a ", "~a ", "==a ", "\"a ", "'a ", "[a ", "![a ",
    "<span>a ",
];

/// The text after the repeated pair: no closer, and one closer of each kind.
/// With the closers, each opener has a possible closer at the end. The
/// tests with a small depth limit use only the first end: past the limit,
/// each failed level reads the literal rest again for its closer, so the
/// work for each pair is larger there by a constant factor (I-MD-35).
const PAIR_ENDS: &[&str] = &[
    "x\n",
    "x~~ y** z__ w* v_ \"u\" 't' s^ r~ q== ]p](u) </span>\n",
];

fn pair_ratios(limit: usize, ends: &[&str]) -> Vec<(String, String, u64, u64)> {
    let read = |pair: &str, end: &str, n: usize| {
        read_steps_limited(&format!("{}{end}", pair.repeat(n)), limit)
    };
    let mut out = Vec::new();
    for a in OPENERS {
        for b in OPENERS {
            if a == b {
                continue;
            }
            for end in ends {
                let small = read(&format!("{a}{b}"), end, 150);
                let large = read(&format!("{a}{b}"), end, 600);
                out.push(((*a).to_string(), format!("{b}{end}"), small, large));
            }
        }
    }
    out
}

#[test]
fn pairs_of_openers_grow_linearly() {
    check_pairs(16384, PAIR_ENDS);
}

/// 150 pairs nest less deep than 300 levels, and 600 pairs nest deeper. The
/// part past the limit must not need more work for each pair than the part
/// before it.
#[test]
fn pairs_of_openers_grow_linearly_across_the_limit() {
    check_pairs(300, &PAIR_ENDS[..1]);
}

/// 600 pairs nest deeper than the default limit of 128 levels.
#[test]
fn pairs_of_openers_grow_linearly_past_the_default_limit() {
    check_pairs(128, &PAIR_ENDS[..1]);
}

fn check_pairs(limit: usize, ends: &'static [&'static str]) {
    let worker = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(move || pair_ratios(limit, ends));
    let joined = worker.map(|handle| handle.join());
    assert!(matches!(&joined, Ok(Ok(_))), "the time test worker failed");
    let Ok(Ok(rows)) = joined else {
        return;
    };
    let mut wrong = Vec::new();
    for (a, b, small, large) in rows {
        // Linear growth gives 4 and quadratic growth gives 16.
        if large > small.saturating_mul(6) {
            wrong.push(format!("{a:?} {b:?}: {small} then {large} steps"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}
