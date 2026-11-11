//! Replays the oracle results of `tests/oracle.tsv` (written by `tools/probe.py`).
//!
//! `tests/known-differences.txt` lists each output of each formula that is
//! different from the oracle now (one line: output, kind, escaped formula).
//! The test fails when an output that is not in the list is different, and
//! when an output in the list became equal, so that the list stays exact.
//! Set `RANDOC_MATH_DIFFS=N` to print the first N differences, and
//! `RANDOC_MATH_FILTER=TEXT` to print only the formulas that contain TEXT.
//! Set `RANDOC_MATH_KNOWN_OUT=PATH` to write the list of the current
//! differences to PATH.
//!
//! The ignored test `replay_other_file` replays the file `RANDOC_MATH_TSV`
//! and writes each difference to `RANDOC_MATH_OUT` (one line: output, kind,
//! formula, oracle result, Randoc result, all escaped). `tools/diffrun.py`
//! uses it.

use randoc_ast::{Inline, MathType};
use std::collections::BTreeSet;

use randoc_math::{MathError, to_inlines, to_mathml};

struct Row {
    kind: MathType,
    tex: String,
    mathml: String,
    inline: String,
    mathml_warning: String,
    inline_warning: String,
}

fn unescape(field: &str) -> String {
    let mut out = String::new();
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn escape_field(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn rows(data: &str) -> Vec<Row> {
    data.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let f: Vec<String> = line.split('\t').map(unescape).collect();
            assert_eq!(f.len(), 6, "a row has six fields: {line}");
            Row {
                kind: if f[0] == "D" {
                    MathType::DisplayMath
                } else {
                    MathType::InlineMath
                },
                tex: f[1].clone(),
                mathml: f[2].clone(),
                inline: f[3].clone(),
                mathml_warning: f[4].clone(),
                inline_warning: f[5].clone(),
            }
        })
        .collect()
}

fn escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

/// Writes inlines as the HTML writer of the oracle does at 1 column: each
/// `Space` is a line break.
fn html(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        let (tag, inner) = match inline {
            Inline::Str(s) => {
                escape(s, out);
                continue;
            }
            Inline::Space => {
                out.push('\n');
                continue;
            }
            Inline::Code(_, text) => {
                out.push_str("<code>");
                escape(text, out);
                out.push_str("</code>");
                continue;
            }
            Inline::Emph(xs) => ("em", xs),
            Inline::Strong(xs) => ("strong", xs),
            Inline::Subscript(xs) => ("sub", xs),
            Inline::Superscript(xs) => ("sup", xs),
            other => {
                out.push_str(&format!("<?{other:?}?>"));
                continue;
            }
        };
        out.push_str(&format!("<{tag}>"));
        html(inner, out);
        out.push_str(&format!("</{tag}>"));
    }
}

fn inline_expected(row: &Row) -> Option<String> {
    if !row.inline_warning.is_empty() {
        return None;
    }
    let class = if row.kind == MathType::DisplayMath {
        "display"
    } else {
        "inline"
    };
    let body = row.inline.strip_suffix("</span>")?;
    for sep in [" ", "\n"] {
        if let Some(rest) = body.strip_prefix(&format!("<span{sep}class=\"math {class}\">")) {
            return Some(rest.to_string());
        }
    }
    None
}

/// The full warning text, as `LogMessage::CouldNotConvertTexMath` writes it.
fn warning(err: &MathError, tex: &str) -> String {
    // The oracle indents each continuation line of a warning by two spaces.
    let head = format!(
        "Could not convert TeX math {}, rendering as TeX",
        tex.replace('\n', "\n  ")
    );
    let detail = err.detail(tex);
    if detail.is_empty() {
        head
    } else {
        format!("{head}:\n{detail}")
    }
}

/// One output of one formula that is different from the oracle.
struct Difference {
    output: &'static str,
    kind: &'static str,
    tex: String,
    want: String,
    got: String,
}

impl Difference {
    /// The line of `tests/known-differences.txt`.
    fn key(&self) -> String {
        format!(
            "{}\t{}\t{}",
            self.output,
            self.kind,
            escape_field(&self.tex)
        )
    }
}

/// The numbers of equal outputs, and each difference.
#[derive(Default)]
struct Replay {
    mathml_eq: usize,
    inline_eq: usize,
    warn_eq: usize,
    warn_total: usize,
    differences: Vec<Difference>,
}

fn replay(rows: &[Row]) -> Replay {
    let mut result = Replay::default();
    for row in rows {
        let kind = if row.kind == MathType::DisplayMath {
            "D"
        } else {
            "I"
        };
        let mut differ = |output: &'static str, want: &str, got: String| {
            result.differences.push(Difference {
                output,
                kind,
                tex: row.tex.clone(),
                want: want.to_string(),
                got,
            });
        };
        let mathml = to_mathml(row.kind, &row.tex, 128);
        let mathml_ok = row.mathml.starts_with("<math");
        match (&mathml, mathml_ok) {
            (Ok(got), true) if *got == row.mathml => result.mathml_eq += 1,
            (Err(_), false) => result.mathml_eq += 1,
            (got, _) => differ("mathml", &row.mathml, format!("{got:?}")),
        }
        let inline = to_inlines(row.kind, &row.tex, 128);
        match (&inline, inline_expected(row)) {
            (Ok(got), Some(want)) => {
                let mut text = String::new();
                html(got, &mut text);
                if text == want {
                    result.inline_eq += 1;
                } else {
                    differ("inline", &want, text);
                }
            }
            (Err(_), None) => result.inline_eq += 1,
            (got, want) => differ("inline", &format!("{want:?}"), format!("{got:?}")),
        }
        for (output, want, got) in [
            ("mathml-warning", &row.mathml_warning, mathml.err()),
            ("inline-warning", &row.inline_warning, inline.err()),
        ] {
            if want.is_empty() {
                continue;
            }
            result.warn_total += 1;
            let text = got
                .as_ref()
                .map(|e: &MathError| warning(e, &row.tex))
                .unwrap_or_default();
            if text == *want {
                result.warn_eq += 1;
            } else {
                differ(output, want, text);
            }
        }
    }
    result
}

#[test]
fn replay_oracle_results() {
    let rows = rows(include_str!("oracle.tsv"));
    let result = replay(&rows);
    eprintln!(
        "replay: {} formulas; MathML equal {}; inline equal {}; warnings equal {} of {}",
        rows.len(),
        result.mathml_eq,
        result.inline_eq,
        result.warn_eq,
        result.warn_total
    );
    let show: usize = std::env::var("RANDOC_MATH_DIFFS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let filter = std::env::var("RANDOC_MATH_FILTER").unwrap_or_default();
    for d in result
        .differences
        .iter()
        .filter(|d| d.tex.contains(&filter))
        .take(show)
    {
        eprintln!(
            "DIFF {} {:?}\n  want {:?}\n  got  {:?}",
            d.output, d.tex, d.want, d.got
        );
    }
    let found: BTreeSet<String> = result.differences.iter().map(Difference::key).collect();
    if let Ok(path) = std::env::var("RANDOC_MATH_KNOWN_OUT") {
        let mut text: String = found.iter().map(|key| format!("{key}\n")).collect();
        if text.is_empty() {
            text.push('\n');
        }
        std::fs::write(&path, text).unwrap_or_else(|e| panic!("cannot write {path}: {e}"));
    }
    let known: BTreeSet<String> = include_str!("known-differences.txt")
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    let new: Vec<&String> = found.difference(&known).collect();
    let gone: Vec<&String> = known.difference(&found).collect();
    assert!(
        new.is_empty(),
        "{} outputs are different from the oracle and not in tests/known-differences.txt, for example {:?}",
        new.len(),
        &new[..new.len().min(5)]
    );
    assert!(
        gone.is_empty(),
        "{} outputs of tests/known-differences.txt are equal now; remove them, for example {:?}",
        gone.len(),
        &gone[..gone.len().min(5)]
    );
}

#[test]
#[ignore = "a tool for tools/diffrun.py: set RANDOC_MATH_TSV and RANDOC_MATH_OUT"]
fn replay_other_file() {
    let (Ok(input), Ok(output)) = (
        std::env::var("RANDOC_MATH_TSV"),
        std::env::var("RANDOC_MATH_OUT"),
    ) else {
        return;
    };
    let data =
        std::fs::read_to_string(&input).unwrap_or_else(|e| panic!("cannot read {input}: {e}"));
    let rows = rows(&data);
    let result = replay(&rows);
    eprintln!(
        "replay: {} formulas; MathML equal {}; inline equal {}; warnings equal {} of {}",
        rows.len(),
        result.mathml_eq,
        result.inline_eq,
        result.warn_eq,
        result.warn_total
    );
    let text: String = result
        .differences
        .iter()
        .map(|d| {
            format!(
                "{}\t{}\t{}\t{}\t{}\n",
                d.output,
                d.kind,
                escape_field(&d.tex),
                escape_field(&d.want),
                escape_field(&d.got)
            )
        })
        .collect();
    std::fs::write(&output, text).unwrap_or_else(|e| panic!("cannot write {output}: {e}"));
}
