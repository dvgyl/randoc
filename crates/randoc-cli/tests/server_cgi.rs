//! Tests of CGI dispatch and response bytes.

#![cfg(all(feature = "server", unix))]

use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

#[test]
fn cgi_program_names_serve_one_request() -> io::Result<()> {
    let body = br#"{"text":"A **test**."}"#;
    for name in ["pandoc-server.cgi", "randoc-server.cgi"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_randoc"))
            .arg0(name)
            .env_clear()
            .env("REQUEST_METHOD", "POST")
            .env("PATH_INFO", "/")
            .env("CONTENT_TYPE", "application/json")
            .env("CONTENT_LENGTH", body.len().to_string())
            .env("HTTP_ACCEPT", "text/plain")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        if let Some(mut input) = child.stdin.take() {
            input.write_all(&[body.as_slice(), b"ignored"].concat())?;
        }
        let response = child.wait_with_output()?;
        assert!(response.status.success());
        assert_eq!(
            response.stdout,
            b"Status: 200 OK\nContent-Type: text/plain;charset=utf-8\n\n<p>A <strong>test</strong>.</p>"
        );
    }
    Ok(())
}

#[test]
fn cgi_head_response_bytes() -> io::Result<()> {
    for (timeout, path, expected) in [
        (
            "2",
            "/%76ersion",
            "Status: 200 OK\nContent-Type: application/json;charset=utf-8\n\n",
        ),
        (
            "0",
            "/version",
            "Status: 503 Service Unavailable\nContent-Type: text/html; charset=utf-8\n\n",
        ),
        (
            "2",
            "/babelmark",
            "Status: 400 Bad Request\nContent-Type: text/html; charset=utf-8\n\nQuery parameter text is required",
        ),
    ] {
        let response = Command::new(env!("CARGO_BIN_EXE_randoc"))
            .arg0("pandoc-server.cgi")
            .env_clear()
            .env("REQUEST_METHOD", "HEAD")
            .env("PATH_INFO", path)
            .env("HTTP_ACCEPT", "application/json")
            .env("PANDOC_SERVER_TIMEOUT", timeout)
            .stdin(Stdio::null())
            .output()?;
        assert!(response.status.success());
        assert_eq!(response.stdout, expected.as_bytes());
    }
    Ok(())
}
