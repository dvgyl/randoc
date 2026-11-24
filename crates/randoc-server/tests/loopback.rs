//! Sends requests over a loopback connection to the server.

#![allow(clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use randoc_server::{
    ServerConfig, ServerHandle, ServerLimits, bind_server, bind_server_with_limits,
};
use serde_json::{Value, json};

fn config(timeout_secs: u64) -> ServerConfig {
    ServerConfig {
        port: 0,
        bind_addr: "127.0.0.1".into(),
        timeout_secs,
    }
}

fn server() -> ServerHandle {
    bind_server(&config(5)).unwrap()
}

fn connect(server: &ServerHandle) -> TcpStream {
    let stream = TcpStream::connect(server.local_addr()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    stream
}

struct Reply {
    status: u16,
    headers: String,
    body: Vec<u8>,
}

impl Reply {
    fn read(mut stream: impl BufRead) -> Self {
        let mut status_line = String::new();
        stream.read_line(&mut status_line).unwrap();
        let mut fields = status_line.split_whitespace();
        assert_eq!(fields.next(), Some("HTTP/1.1"));
        let status = fields.next().unwrap().parse().unwrap();
        let mut headers = String::new();
        loop {
            let old = headers.len();
            assert_ne!(stream.read_line(&mut headers).unwrap(), 0);
            if &headers[old..] == "\r\n" {
                break;
            }
        }
        let mut body = Vec::new();
        stream.read_to_end(&mut body).unwrap();
        Self {
            status,
            headers,
            body,
        }
    }

    fn body(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap()
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .lines()
            .filter_map(|field| field.split_once(':'))
            .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.trim()))
    }
}

fn request(server: &ServerHandle, bytes: impl AsRef<[u8]>) -> Reply {
    let mut stream = connect(server);
    stream.write_all(bytes.as_ref()).unwrap();
    Reply::read(BufReader::new(stream))
}

fn post(server: &ServerHandle, path: &str, accept: &str, body: Value) -> Reply {
    let body = body.to_string();
    request(
        server,
        format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json; charset=utf-8\r\nAccept: {accept}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len(),
        ),
    )
}

#[test]
fn version_and_head() {
    let server = server();
    for (method, body) in [("GET", "3.13"), ("HEAD", "")] {
        let reply = request(
            &server,
            format!("{method} /version HTTP/1.1\r\nHost: localhost\r\n\r\n"),
        );
        assert_eq!(reply.status, 200);
        assert_eq!(
            reply.header("Content-Type"),
            Some("text/plain;charset=utf-8")
        );
        assert_eq!(reply.header("Content-Length"), Some("4"));
        assert_eq!(reply.body(), body);
    }
    server.shutdown().unwrap();
}

#[test]
fn babelmark_query_order_and_decoding() {
    let server = server();
    let reply = request(
        &server,
        "GET /babelmark?text=caf%C3%A9+test%2B&text=no HTTP/1.1\r\nHost: localhost\r\n\r\n",
    );
    assert_eq!(
        reply.json(),
        json!({"html":"<p>café test+</p>","version":"3.13"})
    );
    let reply = request(
        &server,
        "GET /babelmark?text&text=hi HTTP/1.1\r\nHost: localhost\r\n\r\n",
    );
    assert_eq!(
        (reply.status, reply.body()),
        (400, "Query parameter text is required")
    );
    let reply = request(
        &server,
        "HEAD /babelmark?text=hi HTTP/1.1\r\nHost: localhost\r\n\r\n",
    );
    assert_eq!(reply.status, 200);
    assert!(reply.body.is_empty());
}

#[test]
fn fragment_bytes_and_alias_messages() {
    let server = server();
    for (params, expected) in [
        (
            json!({"text":"", "to":"native", "standalone":true}),
            "Pandoc Meta { unMeta = fromList [] } []\n",
        ),
        (
            json!({"text":"[Plain [Str \"a\tb\"]]", "from":"native","to":"native"}),
            "[ Plain [ Str \"a\\tb\" ] ]",
        ),
        (json!({"text":"é", "ascii":true}), "<p>&#xE9;</p>"),
        (
            json!({"text":"# x","to":"markdown","setext-headers":true}),
            "x\n=\n",
        ),
        (
            json!({"text":"[x](u)","to":"markdown","reference-links":true}),
            "[x]\n\n  [x]: u",
        ),
        (json!({"text":"x","to":"plain"}), "x\n"),
        (
            json!({"text":"[x](u)","to":"plain","reference-links":true}),
            "x\n\n  [x]: u",
        ),
        (json!({"text":"x","to":"asciidoc_legacy"}), "x\n"),
        (json!({"text":"x","to":"asciidoctor"}), "x\n"),
        (json!({"text":"x","to":"markdown_github"}), "x\n"),
        (
            json!({"text":"x^[y]","to":"ms"}),
            ".LP\nx\\**\n.FS\ny\n.FE\n",
        ),
        (
            json!({"text":"a![b](c.png)","to":"ms"}),
            ".LP\na\n[IMAGE: b] \\\" \"c.png\"\n",
        ),
        (
            json!({"text":"1. > q","to":"ansi"}),
            "1. \n   │ q\n\u{1b}[0m\u{1b}]8;;\u{1b}\\",
        ),
        (
            json!({"text":"- x","to":"texinfo"}),
            "@node Top\n@top Top\n\n@itemize\n@item\nx\n@end itemize\n",
        ),
        (
            json!({"text":"    x","to":"texinfo"}),
            "@node Top\n@top Top\n\n@verbatim\nx\n@end verbatim\n",
        ),
        (
            json!({"text":"[z \x60a,b\x60](https://a/b,c)","to":"texinfo"}),
            "@node Top\n@top Top\n\n@uref{https://a/b,c,z @code{a@comma{}b}}",
        ),
    ] {
        let reply = post(&server, "/", "application/json", params.clone());
        assert_eq!(reply.status, 200, "{params}: {}", reply.body());
        let result = reply.json();
        assert_eq!(result["output"], expected, "{params}");
        assert_eq!(result["messages"], json!([]), "{params}");
    }
}

#[test]
fn request_json_errors_preserve_the_input() {
    let server = server();
    for (format, prefix) in [("json", "JSON parse error: "), ("csljson", "")] {
        let reply = post(
            &server,
            "/",
            "text/plain",
            json!({"text":"bad","from":format}),
        );
        assert_eq!(reply.status, 500);
        assert_eq!(
            reply.body(),
            format!("{prefix}Unexpected \"bad\", expecting JSON value")
        );
    }
}

#[test]
fn reference_documents_use_the_request_files_and_resource_path() {
    let server = server();
    let params = json!({"text":"x","to":"odt","reference-doc":"x.odt","resource-path":["refs"]});
    let reply = post(&server, "/", "text/plain", params.clone());
    assert_eq!(reply.status, 500);
    assert_eq!(reply.body(), "File 'x.odt' not found in resource path");
    let reference = post(
        &server,
        "/",
        "application/json",
        json!({"text":"", "to":"odt"}),
    )
    .json();
    let mut params = params;
    params["files"] = json!({"refs/x.odt":reference["output"]});
    let reply = post(&server, "/", "application/json", params);
    assert_eq!(reply.status, 200, "{}", reply.body());
    assert_eq!(reply.json()["base64"], true);
}

#[test]
fn route_and_method_errors() {
    let server = server();
    for (method, path, status) in [
        ("GET", "/", 405),
        ("GET", "/batch", 405),
        ("POST", "/version", 405),
        ("OPTIONS", "/", 405),
        ("GET", "/convert", 404),
        ("GET", "/%76ersion", 200),
        ("GET", "/version%2F", 404),
        ("GET", "/%2576ersion", 404),
    ] {
        let reply = request(
            &server,
            format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
        );
        assert_eq!(reply.status, status);
    }
}

#[test]
fn post_media_selection_and_errors() {
    let server = server();
    for (accept, media) in [
        ("*/*", "application/octet-stream"),
        ("application/*", "application/octet-stream"),
        (
            "application/json;q=0.7, text/plain;q=0.9",
            "text/plain;charset=utf-8",
        ),
    ] {
        let reply = post(&server, "/", accept, json!({"text":"# Hi"}));
        assert_eq!(reply.header("Content-Type"), Some(media));
        assert_eq!(reply.body(), "<h1 id=\"hi\">Hi</h1>");
    }
    assert_eq!(
        post(&server, "/", "text/html", json!({"text":"hi"})).status,
        406
    );
    let reply = post(
        &server,
        "/",
        "application/json",
        json!({"text":"hi", "to":"unknown"}),
    );
    assert_eq!(
        reply.json(),
        json!({"error":"Unknown output format 'unknown'"})
    );
    let reply = post(&server, "/", "*/*", json!({"text":"hi", "from":"unknown"}));
    assert_eq!(
        (reply.status, reply.body()),
        (500, "Unknown input format 'unknown'")
    );
    for (accept, status) in [("*/*", 415), ("application/json", 406)] {
        let reply = request(
            &server,
            format!(
                "POST / HTTP/1.1\r\nHost: localhost\r\nAccept: {accept}\r\nOrigin: https://example.test\r\n\r\n"
            ),
        );
        assert_eq!(reply.status, status);
        assert_eq!(reply.header("Access-Control-Allow-Origin"), Some("*"));
    }
}

#[test]
fn cors_preflight() {
    let server = server();
    for (headers, status, origin) in [
        ("content-type, ACCEPT", 200, Some("*")),
        ("X-Custom", 400, None),
    ] {
        let reply = request(
            &server,
            format!(
                "OPTIONS / HTTP/1.1\r\nHost: localhost\r\nOrigin: https://example.test\r\nAccess-Control-Request-Method: POST\r\nAccess-Control-Request-Headers: {headers}\r\n\r\n"
            ),
        );
        assert_eq!(reply.status, status);
        assert_eq!(reply.header("Access-Control-Allow-Origin"), origin);
        if status == 200 {
            assert_eq!(
                reply.header("Access-Control-Allow-Methods"),
                Some("GET, HEAD, POST")
            );
            assert_eq!(
                reply.header("Access-Control-Allow-Headers"),
                Some("Content-Type, Accept, Accept-Language, Content-Language")
            );
            assert!(reply.body.is_empty());
        }
    }
}

#[test]
fn batch_validation_and_order() {
    let server = server();
    let path = "/batch";
    let accept = "application/json";
    assert_eq!(post(&server, path, accept, json!([])).json(), json!([]));
    let reply = post(
        &server,
        path,
        accept,
        json!([{"text":"hi"}, {"text":"hi","table-of-contents":"true"}]),
    );
    assert_eq!(reply.status, 400);
    assert_eq!(
        reply.body(),
        "Error in $[1]['table-of-contents']: expected Bool, but encountered String"
    );
    let reply = post(
        &server,
        path,
        accept,
        json!([{"text":"first"}, {"text":"hi","to":"bad"}, {"text":"last"}]),
    );
    let result = reply.json();
    assert_eq!(result[0]["output"], "<p>first</p>");
    assert_eq!(result[1]["error"], "Unknown output format 'bad'");
    assert_eq!(result[2]["output"], "<p>last</p>");
}

#[test]
fn batch_virtual_files_and_embedding_are_isolated() {
    let server = server();
    let reply = post(
        &server,
        "/batch",
        "application/json",
        json!([
            {"text":"Look ![x](x.png).","self-contained":true,"resource-path":["assets"],"files":{"assets/x.png":"!eA"}},
            {"text":"Look ![x](x.png).","embed-resources":true,"resource-path":["assets"]}
        ]),
    );
    let result = reply.json();
    assert_eq!(
        result[0]["output"],
        "<p>Look <img src=\"data:image/png;base64,eA==\" alt=\"x\" />.</p>"
    );
    assert_eq!(result[0]["messages"][0]["verbosity"], "INFO");
    assert_eq!(
        result[1]["output"],
        "<p>Look <img src=\"x.png\" alt=\"x\" />.</p>"
    );
    assert_eq!(
        result[1]["messages"][0],
        json!({"verbosity":"WARNING","message":"Could not fetch resource x.png"})
    );
}

#[test]
fn template_variables_override_tagged_metadata() {
    let server = server();
    let reply = post(
        &server,
        "/",
        "application/json",
        json!({
            "text":"hi","standalone":true,"template":"$x$|$card()$",
            "files":{"card.html":"JGJvZHkk"},
            "metadata":{"x":{"t":"MetaString","c":"meta"}},"variables":{"x":"variable"}
        }),
    );
    assert_eq!(reply.json()["output"], "variable|<p>hi</p>");
    let reply = post(
        &server,
        "/",
        "application/json",
        json!({"text":"hi","standalone":true,"template":"$if(x)$"}),
    );
    assert!(
        reply.json()["error"]
            .as_str()
            .unwrap()
            .contains("\"custom.html\" (line 1, column 8):")
    );
}

#[test]
fn citeproc_resolves_virtual_bibliography() {
    let server = server();
    let refs = b"@book{source, title={Example}, author={Smith, Alice}, year={2024}}";
    let reply = post(
        &server,
        "/",
        "application/json",
        json!({
            "text":"[@source]","citeproc":true,
            "metadata":{"bibliography":{"t":"MetaString","c":"refs.bib"}},
            "files":{"refs.bib":randoc_archive::media::base64_encode(refs)}
        }),
    );
    let result = reply.json();
    assert!(result["output"].as_str().unwrap().contains("Smith 2024"));
    assert_eq!(
        result["messages"],
        json!([{"verbosity":"INFO","message":"Loaded refs.bib from refs.bib"}])
    );
}

#[test]
fn docx_round_trip_and_track_changes_validation() {
    let server = server();
    let reply = post(
        &server,
        "/",
        "application/json",
        json!({"text":"DOCX marker","to":"docx"}),
    );
    let result = reply.json();
    assert_eq!(result["base64"], true);
    let text = format!(
        " !{}",
        result["output"].as_str().unwrap().trim_end_matches('=')
    );
    let reply = post(
        &server,
        "/",
        "application/json",
        json!({"text":text,"from":"docx","track-changes":"reject"}),
    );
    assert_eq!(reply.json()["output"], "<p>DOCX marker</p>");
    let reply = post(
        &server,
        "/",
        "application/json",
        json!({"text":text,"from":"docx","track-changes":"bogus"}),
    );
    assert_eq!(reply.status, 400);
    assert_eq!(
        reply.body(),
        "Error in $['track-changes']: Unknown track changes method \"bogus\""
    );
}

#[test]
fn native_files_and_subprocess_filters_are_inaccessible() {
    let server = server();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let reply = post(
        &server,
        "/",
        "application/json",
        json!({
            "text":"Hi","citeproc":true,"metadata":{"csl":{"t":"MetaString","c":path}}
        }),
    );
    assert!(
        reply.json()["error"]
            .as_str()
            .unwrap()
            .contains("does not exist")
    );
    let reply = post(
        &server,
        "/",
        "application/json",
        json!({"text":"Hi","filters":[path],"output-file":path}),
    );
    assert_eq!(reply.json()["output"], "<p>Hi</p>");
}

#[test]
fn valid_chunk_extensions_trailer_and_continue() {
    let server = server();
    let mut stream = BufReader::new(connect(&server));
    stream.get_mut().write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nExpect: 100-continue\r\n\r\n").unwrap();
    let mut interim = [0; 25];
    stream.read_exact(&mut interim).unwrap();
    assert_eq!(&interim, b"HTTP/1.1 100 Continue\r\n\r\n");
    stream
        .get_mut()
        .write_all(
            b"8; label = \"x\\\"y\"\r\n{\"text\":\r\n5\r\n\"hi\"}\r\n0;end\r\nX-Trace: yes\r\n\r\n",
        )
        .unwrap();
    let reply = Reply::read(stream);
    assert_eq!((reply.status, reply.body()), (200, "<p>hi</p>"));
}

#[test]
fn rejects_ambiguous_or_invalid_framing() {
    let server = server();
    for headers in [
        "Content-Length: 1\r\nContent-Length: 2\r\n",
        "Content-Length: 0, 1\r\n",
        "Content-Length: +0\r\n",
        "Content-Length: 0\r\nTransfer-Encoding: chunked\r\n",
        "Transfer-Encoding: gzip, chunked\r\n",
        "Host: second\r\n",
        "Content-Length : 0\r\n",
    ] {
        let reply = request(
            &server,
            format!("GET /version HTTP/1.1\r\nHost: localhost\r\n{headers}\r\n"),
        );
        assert_eq!(reply.status, 400, "{headers}");
        assert_eq!(reply.header("Connection"), Some("close"));
    }
    let reply = request(&server, "GET /version HTTP/1.1\r\n\r\n");
    assert_eq!(reply.status, 400);
    let reply = request(
        &server,
        "GET /version HTTP/1.1\r\nHost: localhost\r\nContent-Length: 00, 0\r\n\r\n",
    );
    assert_eq!(reply.status, 200);
}

#[test]
fn framing_and_body_caps_apply_before_conversion() {
    let server = bind_server_with_limits(
        &config(5),
        ServerLimits {
            max_body_bytes: 16,
            ..ServerLimits::default()
        },
    )
    .unwrap();
    for (framing, status) in [
        ("Content-Length: 17\r\n\r\n", 413),
        ("Transfer-Encoding: chunked\r\n\r\n11\r\n", 413),
        ("Transfer-Encoding: chunked\r\n\r\n1;=bad\r\n", 400),
        (
            "Transfer-Encoding: chunked\r\n\r\n0\r\nContent-Length: 2\r\n\r\n",
            400,
        ),
    ] {
        let reply = request(
            &server,
            format!("POST / HTTP/1.1\r\nHost: localhost\r\n{framing}"),
        );
        assert_eq!(reply.status, status);
    }
    for headers in [
        format!("X-Pad: {}\r\n", "x".repeat(65536)),
        "X: y\r\n".repeat(128),
    ] {
        let reply = request(
            &server,
            format!("GET /version HTTP/1.1\r\nHost: localhost\r\n{headers}\r\n"),
        );
        assert_eq!(reply.status, 431);
    }
    let reply = request(
        &server,
        format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n0;x={}\r\n\r\n",
            "x".repeat(65536)
        ),
    );
    assert_eq!(reply.status, 431);
}

#[test]
fn aggregate_batch_response_limit() {
    let server = bind_server_with_limits(
        &config(5),
        ServerLimits {
            max_response_bytes: 75,
            ..ServerLimits::default()
        },
    )
    .unwrap();
    let value = json!({"text":"hi"});
    let reply = post(&server, "/", "application/json", value.clone());
    assert_eq!(reply.status, 200);
    let reply = post(&server, "/batch", "application/json", json!([value, value]));
    assert_eq!(reply.status, 500);
    assert!(reply.body.is_empty());
}

#[test]
fn zero_and_stalled_requests_timeout_without_cors() {
    let zero = bind_server(&config(0)).unwrap();
    for (method, path) in [("GET", "/version"), ("POST", "/"), ("POST", "/batch")] {
        let reply = request(
            &zero,
            format!(
                "{method} {path} HTTP/1.1\r\nHost: localhost\r\nOrigin: https://example.test\r\n\r\n"
            ),
        );
        assert_eq!(reply.status, 503);
        assert_eq!(reply.header("Access-Control-Allow-Origin"), None);
        assert!(reply.body.is_empty());
    }
    let slow = bind_server(&config(1)).unwrap();
    let reply = request(
        &slow,
        "POST / HTTP/1.1\r\nHost: localhost\r\nOrigin: https://example.test\r\nContent-Type: application/json\r\nContent-Length: 40\r\n\r\n{\"text\":",
    );
    assert_eq!(reply.status, 503);
    assert_eq!(reply.header("Access-Control-Allow-Origin"), None);
    assert!(reply.body.is_empty());
}

#[test]
fn archive_reader_uses_restricted_entry_limit() {
    let server = server();
    let limit = randoc_core::Limits::restricted().max_archive_entries;
    let mut archive = randoc_archive::zip::ArchiveWriter::new();
    for index in 0..=limit {
        archive.write_stored(&format!("part-{index}"), b"").unwrap();
    }
    let data = randoc_archive::media::base64_encode(&archive.finish().unwrap());
    let reply = post(
        &server,
        "/",
        "application/json",
        json!({"text":data,"from":"docx"}),
    );
    let value = reply.json();
    let error = value["error"].as_str().unwrap();
    assert!(
        error.contains(&format!("exceeding limit of {limit}")),
        "{error}"
    );
}

#[test]
fn default_body_limit_accepts_eight_mib_json() {
    let server = server();
    let mut body = String::from(r#"{"text":"hi"}"#);
    body.extend(std::iter::repeat_n(' ', (8 << 20) - body.len()));
    let reply = request(
        &server,
        format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len(),
        ),
    );
    assert_eq!((reply.status, reply.body()), (200, "<p>hi</p>"));
}

#[test]
fn markdown_indentation_and_fragment_line_endings() {
    let server = server();
    for (tab_stop, fragment) in [(4, "<p>x</p>"), (2, "<pre><code>\tx</code></pre>")] {
        let reply = post(
            &server,
            "/",
            "*/*",
            json!({"text":"  \tx","tab-stop":tab_stop}),
        );
        assert_eq!((reply.status, reply.body()), (200, fragment));
    }
    for (to, text, expected) in [
        ("markdown", "Hi", "Hi\n"),
        ("plain", "Hi", "Hi\n"),
        ("html", "Hi", "<p>Hi</p>"),
        ("markdown", "", ""),
        ("html", "", ""),
    ] {
        let reply = post(&server, "/", "*/*", json!({"text":text,"to":to}));
        assert_eq!((reply.status, reply.body()), (200, expected));
    }
}

#[test]
fn image_extension_applies_to_markdown_images() {
    let server = server();
    let reply = post(
        &server,
        "/",
        "*/*",
        json!({
            "text":"![a](dir.name/x) ![b](y.png)","default-image-extension":"png"
        }),
    );
    assert_eq!(
        reply.body(),
        "<p><img src=\"dir.name/x.png\" alt=\"a\" /> <img src=\"y.png\" alt=\"b\" /></p>"
    );
}

#[test]
fn http_10_accepts_requests_without_host() {
    let server = server();
    let reply = request(&server, "GET /version HTTP/1.0\r\n\r\n");
    assert_eq!((reply.status, reply.body()), (200, "3.13"));
}

#[test]
fn early_body_limit_response_arrives_with_body_in_flight() {
    let server = bind_server_with_limits(
        &config(5),
        ServerLimits {
            max_body_bytes: 1024,
            ..ServerLimits::default()
        },
    )
    .unwrap();
    let body = "a".repeat(128 << 10);
    let mut stream = connect(&server);
    write!(
        stream,
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let reply = Reply::read(BufReader::new(stream));
    assert_eq!(reply.status, 413);
}

#[test]
fn idle_read_timeout_is_shorter_than_request_timeout() {
    let server = bind_server(&config(10)).unwrap();
    let start = std::time::Instant::now();
    let reply = request(&server, "GET /version HTTP/1.1\r\n");
    assert_eq!(reply.status, 503);
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn batch_item_limit_is_reported_before_reading_later_items() {
    let server = server();
    let body = format!("[{},null,", "null,".repeat(255) + "null");
    let reply = request(
        &server,
        format!(
            "POST /batch HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len(),
        ),
    );
    assert_eq!(
        (reply.status, reply.body()),
        (400, "The batch has more than 256 requests.")
    );
}

#[test]
fn standalone_template_respects_the_output_limit() {
    let server = bind_server_with_limits(
        &config(5),
        ServerLimits {
            max_response_bytes: 4096,
            ..ServerLimits::default()
        },
    )
    .unwrap();
    let reply = post(
        &server,
        "/",
        "application/json",
        json!({
            "text": "Test", "standalone": true,
            "template": "$for(items)$$items$$sep$ $endfor$",
            "variables": {"items": ["x".repeat(2048), "y".repeat(2048)]}
        }),
    );
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.json()["error"],
        "The template exceeds the output limit."
    );
    assert_eq!(
        request(&server, "GET /version HTTP/1.0\r\n\r\n").status,
        200
    );
}

#[test]
fn markdown_references_respect_the_output_limit() {
    let server = bind_server_with_limits(
        &config(5),
        ServerLimits {
            max_response_bytes: 1024,
            ..ServerLimits::default()
        },
    )
    .unwrap();
    let text = format!("[label]: /{}\n\n[label] and [label]", "a".repeat(600));
    let reply = post(&server, "/", "application/json", json!({"text": text}));
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.json()["error"],
        "Markdown references exceed the expansion limit."
    );
    assert_eq!(
        request(&server, "GET /version HTTP/1.0\r\n\r\n").status,
        200
    );
}
