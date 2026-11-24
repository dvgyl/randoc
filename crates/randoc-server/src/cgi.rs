use std::env;
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use crate::http::{Deadline, Request, Response};
use crate::{ServerConfig, ServerLimits, routes};

const DEFAULT_CONTENT_TYPE: &str = "text/html; charset=utf-8";

/// Reads one Common Gateway Interface (CGI) request from the environment and
/// standard input. Writes its CGI response to standard output.
///
/// Returns an I/O error if the worker cannot start or the response cannot be written.
pub fn run_cgi() -> io::Result<()> {
    let timeout = number("PANDOC_SERVER_TIMEOUT").unwrap_or(ServerConfig::default().timeout_secs);
    let deadline = Deadline::new(Duration::from_secs(timeout));
    let method = env::var("REQUEST_METHOD").unwrap_or_default();
    let head = method == "HEAD";
    let preflight_headers = env::var_os("HTTP_ACCESS_CONTROL_REQUEST_HEADERS").is_some();

    let response = if timeout == 0 {
        Response::empty(503)
    } else {
        let (send, receive) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("randoc-cgi".into())
            .stack_size(8 << 20)
            .spawn(move || {
                let _ = send.send(handle(method, deadline));
            })?;
        match receive.recv_timeout(deadline.remaining()) {
            Ok(response) => response,
            Err(RecvTimeoutError::Timeout) => Response::empty(503),
            Err(RecvTimeoutError::Disconnected) => Response::empty(500),
        }
    };
    write_response(&mut io::stdout().lock(), response, head, preflight_headers)
}

fn number(name: &str) -> Option<u64> {
    env::var(name).ok()?.trim().parse().ok()
}

fn handle(method: String, deadline: Deadline) -> Response {
    let limits = ServerLimits::default();
    let length = number("CONTENT_LENGTH").unwrap_or(0);
    if length > limits.max_body_bytes as u64 {
        return Response::empty(413);
    }
    let mut body = Vec::new();
    if io::stdin()
        .lock()
        .take(length)
        .read_to_end(&mut body)
        .is_err()
    {
        return Response::empty(400);
    }
    let path = env::var("PATH_INFO").unwrap_or_default();
    let path = routes::percent_decode(path.strip_prefix('/').unwrap_or(&path), false);
    let path = format!("/{path}");
    let query = env::var("QUERY_STRING").unwrap_or_default();
    let headers = [
        ("accept", "HTTP_ACCEPT"),
        ("content-type", "CONTENT_TYPE"),
        ("origin", "HTTP_ORIGIN"),
        (
            "access-control-request-method",
            "HTTP_ACCESS_CONTROL_REQUEST_METHOD",
        ),
        (
            "access-control-request-headers",
            "HTTP_ACCESS_CONTROL_REQUEST_HEADERS",
        ),
    ]
    .into_iter()
    .filter_map(|(name, variable)| Some((name.into(), env::var(variable).ok()?)))
    .collect();
    let request = Request {
        method,
        target: String::new(),
        headers,
        body,
    };
    routes::route_path(&request, &path, &query, limits, deadline)
}

fn write_response(
    output: &mut impl Write,
    response: Response,
    head: bool,
    preflight_headers: bool,
) -> io::Result<()> {
    writeln!(output, "Status: {} {}", response.status, response.reason())?;
    let content_type = response
        .headers
        .iter()
        .find_map(|&(name, value)| (name == "Content-Type").then_some(value))
        .unwrap_or(DEFAULT_CONTENT_TYPE);
    writeln!(output, "Content-Type: {content_type}")?;
    for (name, value) in response.headers {
        if name != "Content-Type" && (name != "Access-Control-Allow-Headers" || preflight_headers) {
            writeln!(output, "{name}: {value}")?;
        }
    }
    writeln!(output)?;
    // The CGI route omits the body on a successful HEAD request.
    if !head || response.status != 200 {
        output.write_all(&response.body)?;
    }
    output.flush()
}
