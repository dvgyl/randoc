//! Replays the oracle matrix of the options that a Lua filter sees
//! (`tests/data/lua-options.json`, made by `tools/lua_options_matrix.py`):
//! each field of `PANDOC_READER_OPTIONS` and `PANDOC_WRITER_OPTIONS` for
//! each command line of the matrix.

#![cfg(feature = "lua")]
#![allow(
    clippy::expect_used,
    reason = "a broken fixture of the replay stops the test"
)]

use serde_json::Value;

const DOC: &str = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}"#;

/// The start of the lines that differ for a known reason, parts of row
/// I-LUA-27 of `randoc-lua`: the abbreviation table of `randoc-core` has 76
/// of the 80 entries of Pandoc (5), and a style name in `highlight_method`
/// is a string, where Pandoc gives a table of the style (4).
const KNOWN: [&str; 2] = ["R abbreviations = ", "W highlight_method = \"kate\""];

/// The output formats whose `W extensions` line differs (row I-CLI-54): for
/// `gfm` Pandoc adds `auto_identifiers`, which `--list-extensions=gfm` does
/// not show.
const KNOWN_WRITER_EXTENSIONS: [&str; 1] = ["gfm"];

#[test]
fn lua_filters_see_the_options_of_pandoc() {
    let records: Vec<Value> =
        serde_json::from_str(include_str!("data/lua-options.json")).expect("the matrix is JSON");
    let base = std::env::temp_dir().join(format!("randoc-cli-lua-options-{}", std::process::id()));
    std::fs::create_dir_all(&base).expect("the folder is made");
    std::fs::write(
        base.join("dump.lua"),
        include_str!("data/lua-options-dump.lua"),
    )
    .expect("the filter is written");
    std::fs::write(base.join("t.html"), "$body$\n").expect("the template is written");
    std::fs::write(base.join("ab"), "Zz.\n").expect("the abbreviations are written");
    let dump = base.join("dump.txt");
    let mut differences = Vec::new();
    for record in &records {
        let args: Vec<&str> = record["args"]
            .as_array()
            .expect("args")
            .iter()
            .map(|a| a.as_str().expect("text"))
            .collect();
        let _ = std::fs::remove_file(&dump);
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_randoc"))
            .args(&args)
            .current_dir(&base)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &base)
            .env("OPTIONS_DUMP", &dump)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("the program starts");
        {
            use std::io::Write;
            let mut stdin = child.stdin.take().expect("standard input");
            let _ = stdin.write_all(DOC.as_bytes());
        }
        let output = child.wait_with_output().expect("the program ends");
        let text = std::fs::read_to_string(&dump).unwrap_or_default();
        let mut got: Vec<&str> = text.lines().collect();
        got.sort_unstable();
        let expected: Vec<&str> = record["lines"]
            .as_array()
            .expect("lines")
            .iter()
            .map(|l| l.as_str().expect("text"))
            .collect();
        let to = args
            .iter()
            .position(|a| *a == "-t")
            .and_then(|i| args.get(i + 1))
            .copied()
            .unwrap_or_default();
        let known = |line: &&str| {
            !KNOWN.iter().any(|start| line.starts_with(start))
                && *line != "W highlight_method = \"a style\""
                && !(KNOWN_WRITER_EXTENSIONS.contains(&to) && line.starts_with("W extensions = "))
        };
        let got_lines: Vec<&str> = got.iter().copied().filter(known).collect();
        let expected_lines: Vec<&str> = expected.iter().copied().filter(known).collect();
        let exit = output.status.code().map(i64::from);
        if exit != record["exit"].as_i64() || got_lines != expected_lines {
            let missing: Vec<&&str> = expected_lines
                .iter()
                .filter(|l| !got_lines.contains(l))
                .collect();
            let extra: Vec<&&str> = got_lines
                .iter()
                .filter(|l| !expected_lines.contains(l))
                .collect();
            differences.push(format!(
                "{args:?}: exit {exit:?}, stderr {:?}\n  Pandoc only: {missing:?}\n  Randoc only: {extra:?}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&base);
    assert!(
        differences.is_empty(),
        "{} of {} command lines differ:\n{}",
        differences.len(),
        records.len(),
        differences.join("\n")
    );
}
