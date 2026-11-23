//! Replays the oracle results in `data/replay.txt` (made by
//! `tools/data.py oracle`) and compares them with the output of the crate.

use std::collections::BTreeMap;

use randoc_highlight::{
    FormatOptions, HtmlVersion, Registry, format_latex_block, highlight, html_block,
};

const SAMPLES: &str = include_str!("../data/samples.txt");
const REPLAY: &str = include_str!("../data/replay.txt");

/// The languages with differences in the full replay, from `replay(1)`:
/// language, sample differences, mutation differences, plain results. Each
/// difference has a row in `docs/ISSUES.md`. Change the table when a fix
/// removes differences.
const KNOWN_DIFFERENCES: &str = concat!(
    "cobol 1 3 4\n",
    "coffee 0 1 0\n",
    "dockerfile 12 78 90\n",
    "elixireex 17 124 141\n",
    "elixirheex 18 132 150\n",
    "javascriptreact 0 1 0\n",
    "mermaid 1 4 5\n",
    "mustache 1 7 8\n",
    "orgmode 8 59 67\n",
    "raku 1 14 15\n",
    "rdoc 4 27 31\n",
    "sas 2 15 17\n",
    "total 530\n",
);

/// The same table for the default test, which replays each eighth mutation.
const KNOWN_DIFFERENCES_SAMPLED: &str = concat!(
    "cobol 1 0 1\n",
    "coffee 0 1 0\n",
    "dockerfile 12 8 20\n",
    "elixireex 17 14 31\n",
    "elixirheex 18 14 32\n",
    "mermaid 1 0 1\n",
    "mustache 1 1 2\n",
    "orgmode 8 8 16\n",
    "raku 1 0 1\n",
    "rdoc 4 3 7\n",
    "sas 2 4 6\n",
    "total 118\n",
);

fn samples() -> Vec<(String, String)> {
    SAMPLES
        .split('\u{c}')
        .skip(1)
        .map(|part| {
            let (lang, code) = part.split_once('\n').unwrap_or((part, ""));
            let code = code.strip_suffix('\n').unwrap_or(code);
            (lang.to_string(), code.to_string())
        })
        .collect()
}

/// Applies the mutation operations of `tools/data.py`.
fn apply_ops(code: &str, ops: &str) -> String {
    let mut chars: Vec<char> = code.chars().collect();
    for op in ops.split('.') {
        let (kind, n) = op.split_at(1);
        let Ok(n) = n.parse::<usize>() else { continue };
        match kind {
            "d" | "u" => {
                if let Some(&c) = chars.get(n) {
                    if kind == "d" {
                        chars.remove(n);
                    } else {
                        chars.insert(n, c);
                    }
                }
            }
            _ => {
                let text: String = chars.iter().collect();
                let mut lines: Vec<&str> = text.split('\n').collect();
                if let Some(&line) = lines.get(n) {
                    if kind == "L" {
                        lines.insert(n, line);
                    } else {
                        lines.remove(n);
                    }
                }
                chars = lines.join("\n").chars().collect();
            }
        }
    }
    chars.into_iter().collect()
}

fn fnv64(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The HTML and LaTeX output of Pandoc for one code block with the class
/// `lang`, the identifier `c` in HTML and no identifier in LaTeX.
fn render(registry: &Registry, lang: &str, code: &str) -> (String, String) {
    let classes = [lang.to_string()];
    let highlighted = registry.lookup_classes(&classes).and_then(|syntax| {
        let lines = highlight(registry, syntax, code, 1000).ok()?;
        Some((syntax.short_name().to_lowercase(), lines))
    });
    match highlighted {
        Some((name, lines)) => {
            let opts = FormatOptions::from_attr("c", &classes, &[], &name);
            let mut block = html_block(&opts, &lines, HtmlVersion::Html5);
            block.div.push(("id".to_string(), "c".to_string()));
            let latex_opts = FormatOptions::from_attr("", &classes, &[], &name);
            (block.render(), format_latex_block(&latex_opts, &lines))
        }
        None => (
            format!(
                "<pre id=\"c\" class=\"{}\"><code>{}</code></pre>",
                escape_html(lang),
                escape_html(code)
            ),
            format!("\\begin{{verbatim}}\n{code}\n\\end{{verbatim}}"),
        ),
    }
}

#[derive(Default)]
struct Count {
    samples: usize,
    sample_diffs: usize,
    mutations: usize,
    mutation_diffs: usize,
    /// Differences where the crate gives plain code, because a definition
    /// that the language includes is not embedded.
    plain_samples: usize,
    plain_mutations: usize,
}

/// Compares each sample and each `stride`-th mutation with the oracle and
/// returns the number of differences.
fn replay(stride: usize) -> String {
    replay_with(Registry::embedded(), stride)
}

fn replay_with(registry: &Registry, stride: usize) -> String {
    let samples = samples();
    let mut table: BTreeMap<String, Count> = BTreeMap::new();
    let mut first_diffs = Vec::new();
    let mut mutation_number = 0usize;
    for entry in REPLAY.split('\u{c}').skip(1) {
        let (head, body) = entry.split_once('\n').unwrap_or((entry, ""));
        let fields: Vec<&str> = head.split(' ').collect();
        match fields.as_slice() {
            ["s", index, lang] => {
                let Some((_, code)) = index.parse::<usize>().ok().and_then(|i| samples.get(i))
                else {
                    unreachable!("bad sample index {index}");
                };
                let (html, latex) = body.split_once("\n\u{b}\n").unwrap_or((body, ""));
                let latex = latex.strip_suffix('\n').unwrap_or(latex);
                let (got_html, got_latex) = render(registry, lang, code);
                let count = table.entry((*lang).to_string()).or_default();
                count.samples += 1;
                if got_html != html || got_latex != latex {
                    count.sample_diffs += 1;
                    if got_html.starts_with("<pre") {
                        count.plain_samples += 1;
                    } else if count.sample_diffs - count.plain_samples == 1 {
                        let which = if got_html != html { "html" } else { "latex" };
                        let (want, got) = if got_html != html {
                            (html, got_html.as_str())
                        } else {
                            (latex, got_latex.as_str())
                        };
                        first_diffs.push(format!(
                            "{lang} sample {index} ({which}):\nwant {want}\ngot  {got}"
                        ));
                    }
                }
            }
            ["m", base, lang, ops, html_hash, latex_hash] => {
                mutation_number += 1;
                if !mutation_number.is_multiple_of(stride) {
                    continue;
                }
                let Some((_, code)) = base.parse::<usize>().ok().and_then(|i| samples.get(i))
                else {
                    unreachable!("bad sample index {base}");
                };
                let code = apply_ops(code, ops);
                let (got_html, got_latex) = render(registry, lang, &code);
                let count = table.entry((*lang).to_string()).or_default();
                count.mutations += 1;
                if fnv64(&got_html) != *html_hash || fnv64(&got_latex) != *latex_hash {
                    count.mutation_diffs += 1;
                    if got_html.starts_with("<pre") {
                        count.plain_mutations += 1;
                    } else if count.mutation_diffs - count.plain_mutations == 1 {
                        first_diffs.push(format!(
                            "{lang} mutation of {base} ({ops}):\n{code}\ngot  {got_html}"
                        ));
                    }
                }
            }
            _ => unreachable!("bad replay line {head}"),
        }
    }
    let mut report = String::from("language samples diffs mutations diffs plain\n");
    let mut total = 0;
    for (lang, c) in &table {
        report.push_str(&format!(
            "{lang} {} {} {} {} {}\n",
            c.samples,
            c.sample_diffs,
            c.mutations,
            c.mutation_diffs,
            c.plain_samples + c.plain_mutations
        ));
        total += c.sample_diffs + c.mutation_diffs;
    }
    println!("{report}total differences {total}");
    for d in &first_diffs {
        println!("{d}\n");
    }
    assert_eq!(table.len(), 63);
    let mut known = String::new();
    for (lang, c) in &table {
        if c.sample_diffs + c.mutation_diffs > 0 {
            known.push_str(&format!(
                "{lang} {} {} {}\n",
                c.sample_diffs,
                c.mutation_diffs,
                c.plain_samples + c.plain_mutations
            ));
        }
    }
    known.push_str(&format!("total {total}\n"));
    known
}

#[test]
fn replay_samples_and_some_mutations() {
    assert_eq!(replay(8), KNOWN_DIFFERENCES_SAMPLED);
}

/// The full replay takes minutes in a debug build. Run it with
/// `cargo test -p randoc-highlight --test replay -- --ignored --nocapture`.
#[test]
#[ignore]
fn replay_everything() {
    assert_eq!(replay(1), KNOWN_DIFFERENCES);
}

/// Replays everything with all definitions of a skylighting `xml/` folder
/// loaded at run time, from the variable `RANDOC_HIGHLIGHT_XML`. The
/// definitions that the crate does not embed then close the gaps I-HL-01
/// and I-HL-02.
#[test]
#[ignore]
fn replay_with_all_definitions() {
    let Ok(dir) = std::env::var("RANDOC_HIGHLIGHT_XML") else {
        return;
    };
    let mut registry = Registry::embedded().clone();
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        names.push(entry.unwrap().file_name().to_string_lossy().into_owned());
    }
    let loaded = registry.load_dir(&dir, names, |p| std::fs::read(p).map_err(|e| e.to_string()));
    assert_eq!(loaded, Ok(()));
    assert_eq!(replay_with(&registry, 1), "total 0\n");
}

const LONG: &str = include_str!("../data/long.txt");

/// One long input of `data/long.txt` (made by `tools/data.py long`).
struct Long {
    name: String,
    lang: String,
    /// `ok`, or why the oracle gave no result (`timeout`).
    status: String,
    html_hash: String,
    latex_hash: String,
    code: String,
}

fn long_inputs() -> Vec<Long> {
    LONG.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let [name, lang, status, html_hash, latex_hash, spec @ ..] = f.as_slice() else {
                return None;
            };
            let spec: Vec<serde_json::Value> = spec
                .iter()
                .map(|s| serde_json::from_str(s))
                .collect::<Result<_, _>>()
                .ok()?;
            let text = |i: usize| spec.get(i).and_then(|v| v.as_str()).map(str::to_string);
            let number = |i: usize| {
                spec.get(i)
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
            };
            let row = format!("{}{}{}", text(0)?, text(1)?.repeat(number(2)?), text(3)?);
            let code = vec![row; number(4)?].join(&text(5)?) + &text(6)?;
            Some(Long {
                name: (*name).to_string(),
                lang: (*lang).to_string(),
                status: (*status).to_string(),
                html_hash: (*html_hash).to_string(),
                latex_hash: (*latex_hash).to_string(),
                code,
            })
        })
        .collect()
}

/// Compares the long inputs of at most `max_bytes` bytes with the oracle
/// and returns the names of the inputs that differ.
fn replay_long(max_bytes: usize) -> (usize, Vec<String>) {
    let inputs = long_inputs();
    assert!(inputs.len() >= 18, "{} long inputs", inputs.len());
    let mut compared = 0;
    let mut differ = Vec::new();
    for input in inputs
        .iter()
        .filter(|i| i.status == "ok" && i.code.len() <= max_bytes)
    {
        let (html, latex) = render(Registry::embedded(), &input.lang, &input.code);
        compared += 1;
        if fnv64(&html) != input.html_hash || fnv64(&latex) != input.latex_hash {
            differ.push(input.name.clone());
        }
    }
    (compared, differ)
}

/// The samples of the reviews up to 60,000 bytes give the oracle output.
#[test]
fn long_inputs_match_the_oracle() {
    let (compared, differ) = replay_long(60_000);
    assert_eq!((compared, differ), (6, Vec::<String>::new()));
}

/// All long inputs with an oracle result. Minutes in a debug build.
#[test]
#[ignore]
fn all_long_inputs_match_the_oracle() {
    let (compared, differ) = replay_long(usize::MAX);
    assert!(compared >= 10, "{compared}");
    assert_eq!(differ, Vec::<String>::new());
}

#[test]
fn mutation_rules_match_the_tool() {
    assert_eq!(apply_ops("ab\ncd\nef", "d0.L1.u3.K0"), "cdd\ncd\nef");
    assert_eq!(apply_ops("ab", "d9.K9.x1.u"), "ab");
}

/// The languages of the benchmark: the five of the first report and the
/// four with long prose lines of review round 1.
const BENCH_LANGUAGES: [&str; 9] = [
    "rust",
    "zsh",
    "typescript",
    "raku",
    "julia",
    "asciidoc",
    "logfile",
    "orgmode",
    "nix",
];

/// Sample code of `lang`, repeated to `size` bytes.
fn bench_input(lang: &str, size: usize) -> String {
    let samples = samples();
    let parts: Vec<&str> = samples
        .iter()
        .filter(|(l, _)| l == lang)
        .map(|(_, c)| c.as_str())
        .collect();
    let mut code = String::new();
    while code.len() < size && !parts.is_empty() {
        for p in &parts {
            code.push_str(p);
            code.push('\n');
        }
    }
    code
}

/// The minimum time of 5 runs of `highlight` on `code`, in seconds.
fn time_highlight(lang: &str, code: &str) -> f64 {
    let registry = Registry::embedded();
    let Some(syntax) = registry.lookup(lang) else {
        return 0.0;
    };
    (0..5)
        .map(|_| {
            let start = std::time::Instant::now();
            let _ = highlight(registry, syntax, code, 1000);
            start.elapsed().as_secs_f64()
        })
        .fold(f64::INFINITY, f64::min)
}

/// A benchmark: run it with `cargo test --release -p randoc-highlight
/// --test replay -- --ignored --nocapture one_megabyte`.
#[test]
#[ignore]
fn one_megabyte_is_fast_and_linear() {
    let _ = Registry::embedded();
    let prose = "Lorem ipsum dolor sit amet, see `code` and *strong* text. ".repeat(100);
    let mut inputs: Vec<(String, String, String)> = Vec::new();
    for lang in BENCH_LANGUAGES {
        inputs.push((
            lang.to_string(),
            bench_input(lang, 256 * 1024),
            bench_input(lang, 1024 * 1024),
        ));
    }
    for lang in ["asciidoc", "logfile", "orgmode", "nix"] {
        let rows = |n: usize| vec![prose.clone(); n].join("\n\n");
        inputs.push((format!("{lang} prose"), rows(45), rows(180)));
    }
    let mut ok = true;
    for (name, small, large) in &inputs {
        let lang = name.split(' ').next().unwrap_or(name);
        let (a, b) = (time_highlight(lang, small), time_highlight(lang, large));
        println!(
            "| {name} | {} | {a:.3} | {} | {b:.3} | {:.2} |",
            small.len(),
            large.len(),
            b / a.max(1e-9)
        );
        ok &= b < 10.0 * a + 0.1;
    }
    assert!(ok);
}
