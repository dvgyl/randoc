//! Tests of the program with arguments that unit tests cannot give.

#[cfg(unix)]
#[test]
fn an_argument_that_is_not_utf8_becomes_a_replacement_character() {
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;
    // Pandoc 3.13 gives the metadata value "a\u{fffd}b" and exit code 0.
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_randoc"))
        .args(["-f", "json", "-t", "json", "-M"])
        .arg(std::ffi::OsStr::from_bytes(b"t=a\xffb"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("the program starts");
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(br#"{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[]}"#)
            .expect("the input is written");
    }
    let output = child.wait_with_output().expect("the program ends");
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).expect("the output is UTF-8");
    assert!(
        text.contains(r#""t":{"t":"MetaString","c":"a\u{fffd}b"}"#) || text.contains("a\u{fffd}b")
    );
}

#[cfg(unix)]
#[test]
fn a_format_name_that_is_not_utf8_is_an_unknown_format() {
    use std::os::unix::ffi::OsStrExt;
    // Pandoc 3.13: exit 21 and `Unknown input format '\u{fffd}'`.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_randoc"))
        .args(["-t", "json", "-f"])
        .arg(std::ffi::OsStr::from_bytes(b"\xff"))
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the program runs");
    assert_eq!(output.status.code(), Some(21));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "Unknown input format '\u{fffd}'\n"
    );
}
