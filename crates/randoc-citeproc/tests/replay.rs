//! Compares the parser with the oracle results of `tests/replay/parse-errors.txt`.

#![allow(clippy::unwrap_used, clippy::panic)]

use randoc_core::Context;

fn unescape(field: &str) -> String {
    let mut out = String::new();
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn each_style_gives_the_oracle_result_and_exit_code() {
    // The program runs on a thread with a 1 GiB stack (docs/ARCHITECTURE.md).
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(compare)
        .unwrap()
        .join()
        .unwrap();
}

fn compare() {
    let replay = include_str!("replay/parse-errors.txt");
    let mut failures = Vec::new();
    let mut count = 0;
    for line in replay
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let fields: Vec<&str> = line.split('\t').collect();
        let [name, exit, style, expected] = fields.as_slice() else {
            panic!("a replay line has not four fields: {line}");
        };
        count += 1;
        let mut context = Context::pure();
        // The program limit (docs/ARCHITECTURE.md).
        context.limits.max_nesting_depth = 16384;
        let (code, text) = match randoc_citeproc::parse_style(&mut context, &unescape(style)) {
            Ok(_) => ("0".to_owned(), "ok".to_owned()),
            Err(error) => (error.exit_code().to_string(), error.to_string()),
        };
        let expected = unescape(expected);
        if code != *exit || text != expected {
            failures.push(format!(
                "{name}: expected {exit} {expected:?}\n  got {code} {text:?}"
            ));
        }
    }
    assert!(count >= 60, "the replay file has {count} cases");
    assert!(
        failures.is_empty(),
        "{} of {count} cases differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
