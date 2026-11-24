//! Replays the level matrix of Pandoc 3.13 (`tests/data/levels.json`, made
//! by `tools/level_matrix.py`): each probe provokes one log message kind, and
//! the level in the log of Randoc must be the level in the log of Pandoc.

#![allow(
    clippy::expect_used,
    reason = "a broken fixture of the replay stops the test"
)]

mod fixtures;

use serde_json::Value;
use std::collections::BTreeMap;

/// The level of each message type of a log. Pandoc and Randoc spell some
/// types with other capitals (`CouldNotConvertTeXMath`).
fn levels(pairs: impl Iterator<Item = (String, String)>) -> BTreeMap<String, String> {
    pairs
        .map(|(kind, level)| (kind.to_lowercase(), level))
        .collect()
}

#[test]
fn each_emitted_message_has_the_level_of_pandoc() {
    let records: Vec<Value> =
        serde_json::from_str(include_str!("data/levels.json")).expect("the matrix is JSON");
    let base = std::env::temp_dir().join(format!("randoc-cli-levels-{}", std::process::id()));
    let mut differences = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let root = base.join(index.to_string());
        std::fs::create_dir_all(&root).expect("the probe folder is made");
        for (name, bytes) in record["files"].as_object().expect("files") {
            fixtures::write_hex_file(&root.join(name), bytes.as_str().expect("hex"));
        }
        let args: Vec<&str> = record["args"]
            .as_array()
            .expect("args")
            .iter()
            .map(|a| a.as_str().expect("text"))
            .collect();
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_randoc"))
            .args(&args)
            .args(["--log=log.json", "-o", "out"])
            .current_dir(&root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the program starts");
        {
            use std::io::Write;
            let mut stdin = child.stdin.take().expect("standard input");
            let _ = stdin.write_all(record["stdin"].as_str().unwrap_or_default().as_bytes());
        }
        child.wait().expect("the program ends");
        let log: Vec<Value> = std::fs::read(root.join("log.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let got = levels(log.iter().map(|m| {
            (
                m["type"].as_str().unwrap_or_default().to_owned(),
                m["verbosity"].as_str().unwrap_or_default().to_owned(),
            )
        }));
        let expected = levels(
            record["oracle"]
                .as_array()
                .expect("oracle")
                .iter()
                .map(|p| {
                    (
                        p[0].as_str().unwrap_or_default().to_owned(),
                        p[1].as_str().unwrap_or_default().to_owned(),
                    )
                }),
        );
        let target = record["target"].as_str().expect("target").to_lowercase();
        if !got.contains_key(&target) {
            differences.push(format!("probe {index} {args:?}: no {target} in {got:?}"));
        }
        for (kind, level) in &got {
            if expected.get(kind).is_some_and(|wanted| wanted != level) {
                differences.push(format!(
                    "probe {index} {args:?}: {kind} is {level}, Pandoc {:?}",
                    expected[kind]
                ));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&base);
    assert!(
        differences.is_empty(),
        "{} differences in {} probes:\n{}",
        differences.len(),
        records.len(),
        differences.join("\n")
    );
}
