//! Replays the resource matrix of Pandoc 3.13 (`tests/data/resources.json`,
//! made by `tools/resource_matrix.py`): include files, templates, the
//! resource path as a list, the search for a defaults file, named files that
//! are missing, the order of the reads, `--sandbox` and the log.

#![allow(
    clippy::expect_used,
    reason = "a broken fixture of the replay stops the test"
)]

use serde_json::Value;

const DOC: &str = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}"#;

fn text(record: &Value, key: &str) -> Option<String> {
    record.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hexadecimal file bytes"))
        .collect()
}

/// Removes the parts of a Pandoc error that are recorded as I-CLI-06: the
/// `pandoc: ` prefix and the GHC backtrace.
fn without_ghc_parts(stderr: &str) -> String {
    let stderr = stderr
        .split("HasCallStack backtrace:")
        .next()
        .unwrap_or_default();
    stderr
        .lines()
        .map(|line| line.strip_prefix("pandoc: ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn resource_reads_follow_the_matrix_of_pandoc() {
    replay(include_str!("data/resources.json"), "resources");
}

/// Each defaults file and metadata file text of the cases and the matrix
/// tools (`tools/yaml_texts.py`), read through `randoc-yaml`.
#[test]
fn defaults_and_metadata_texts_give_the_result_of_pandoc() {
    replay(include_str!("data/yaml-texts.json"), "yaml");
}

#[test]
fn utf8_errors_in_include_files_name_the_byte_of_pandoc() {
    replay(include_str!("data/utf8-offsets.json"), "utf8");
}

/// Runs each recorded case and compares exit code, standard output,
/// standard error and the log bytes with the record of Pandoc.
///
/// The cases with the same files share one folder: each file is slow on the
/// exFAT drive of the project. A case writes only the log, and the replay
/// removes the log after each case.
fn replay(data: &str, name: &str) {
    let records: Vec<Value> = serde_json::from_str(data).expect("the matrix is JSON");
    let mut by_files: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (index, record) in records.iter().enumerate() {
        by_files
            .entry(record["files"].to_string())
            .or_default()
            .push(index);
    }
    let groups: Vec<Vec<usize>> = by_files.into_values().collect();
    let base = std::env::temp_dir().join(format!("randoc-cli-{name}-{}", std::process::id()));
    let home = base.join("home");
    std::fs::create_dir_all(&home).expect("the home folder is made");
    let workers = 4;
    // A wrong rule fails many cases. The replay stops after this number of
    // differences, so that a failure does not need the time of a full run.
    let limit = 15;
    let found = std::sync::atomic::AtomicUsize::new(0);
    let differences: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                let (records, groups, base, home, found) =
                    (&records, &groups, &base, &home, &found);
                scope.spawn(move || {
                    use std::sync::atomic::Ordering;
                    let mut differences = Vec::new();
                    for group in groups.iter().skip(worker).step_by(workers) {
                        let root = base.join(group[0].to_string());
                        make_files(&root, &records[group[0]]["files"]);
                        for &index in group {
                            if found.load(Ordering::Relaxed) >= limit {
                                break;
                            }
                            if let Some(text) = difference(index, &records[index], &root, home) {
                                found.fetch_add(1, Ordering::Relaxed);
                                differences.push(text);
                            }
                            let _ = std::fs::remove_file(root.join("log.json"));
                        }
                        let _ = std::fs::remove_dir_all(&root);
                    }
                    differences
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("a replay worker ends"))
            .collect()
    });
    let _ = std::fs::remove_dir_all(&base);
    assert!(
        differences.is_empty(),
        "{} of {} cases differ (the replay stops after {limit}):\n{}",
        differences.len(),
        records.len(),
        differences.join("\n")
    );
}

/// Makes the files of a case in the folder `root`.
fn make_files(root: &std::path::Path, files: &Value) {
    std::fs::create_dir_all(root).expect("the case folder is made");
    for (path, bytes) in files.as_object().expect("files") {
        let full = root.join(path);
        std::fs::create_dir_all(full.parent().expect("a parent")).expect("a folder");
        // A file `keep` only makes its folder exist. The folder is sufficient.
        if full.file_name().is_some_and(|name| name == "keep") {
            continue;
        }
        std::fs::write(&full, hex(bytes.as_str().expect("hex"))).expect("a file");
    }
}

/// Runs one recorded case in the folder `root`, which holds the files of the
/// case, and describes how the result of Randoc differs.
fn difference(
    index: usize,
    record: &Value,
    root: &std::path::Path,
    home: &std::path::Path,
) -> Option<String> {
    let root_text = root.to_str().expect("a UTF-8 folder name").to_owned();
    let args: Vec<String> = record["args"]
        .as_array()
        .expect("args")
        .iter()
        .map(|a| a.as_str().expect("text").replace("{root}", &root_text))
        .collect();
    let uses_stdin = !args
        .iter()
        .any(|a| a.ends_with(".json") && !a.starts_with('-'));
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_randoc"));
    command
        .args(&args)
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("share"));
    // A case can name its own variables, for example a user data folder in the case folder.
    for (key, value) in record["env"].as_object().into_iter().flatten() {
        command.env(
            key,
            value.as_str().expect("text").replace("{root}", &root_text),
        );
    }
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the program starts");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("standard input");
        // A program that stops before it reads its input closes the pipe;
        // the comparison of the results judges that case.
        if uses_stdin {
            let _ = stdin.write_all(DOC.as_bytes());
        }
    }
    let output = child.wait_with_output().expect("the program ends");
    let log = std::fs::read(root.join("log.json"))
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).replace(&root_text, "{root}"));
    let stderr = String::from_utf8_lossy(&output.stderr).replace(&root_text, "{root}");
    let got = (
        output.status.code().map(i64::from),
        String::from_utf8_lossy(&output.stdout).replace(&root_text, "{root}"),
        without_ghc_parts(&stderr),
        log,
    );
    let expected = (
        record["exit"].as_i64(),
        text(record, "stdout").unwrap_or_default(),
        without_ghc_parts(&text(record, "stderr").unwrap_or_default()),
        text(record, "log"),
    );
    (got != expected).then(|| {
        format!(
            "case {index}: {args:?}\n  files {:?}\n  expected {expected:?}\n  got      {got:?}",
            record["files"]
                .as_object()
                .map(|f| f.keys().collect::<Vec<_>>())
        )
    })
}
