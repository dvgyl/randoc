//! Converts JSON documents at the nesting limit of the program.

use std::io::Write;
use std::process::{Command, Stdio};

fn nested_block_quotes(depth: usize) -> String {
    let mut text = String::from(r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":["#);
    text.push_str(&r#"{"t":"BlockQuote","c":["#.repeat(depth));
    text.push_str(&"]}".repeat(depth));
    text.push_str("]}");
    text
}

fn convert(input: &str) -> std::io::Result<std::process::Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_randoc"))
        .args(["-f", "json", "-t", "json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }
    child.wait_with_output()
}

#[test]
fn a_document_at_the_depth_limit_converts() {
    // Pandoc 3.13 gives the same bytes. 16383 block quotes use 32768 JSON levels.
    let input = nested_block_quotes(16_383);
    let output = convert(&input).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{input}\n")
    );
}

#[test]
fn a_document_over_the_depth_limit_is_refused() {
    let output = convert(&nested_block_quotes(16_384)).unwrap();
    assert_eq!(output.status.code(), Some(64));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "JSON parse error: the document is nested too deeply\n"
    );
}
