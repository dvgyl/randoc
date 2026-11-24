use std::borrow::Cow;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv6Addr, TcpStream};
use std::time::{Duration, Instant};

const MAX_HEADER_BYTES: usize = 64 << 10;
const MAX_HEADER_COUNT: usize = 128;
const MAX_FRAMING_BYTES: usize = 64 << 10;
const MAX_TRAILER_COUNT: usize = 128;

#[derive(Clone, Copy)]
pub(crate) struct Deadline {
    started: Instant,
    timeout: Duration,
}

impl Deadline {
    pub fn new(timeout: Duration) -> Self {
        Self {
            started: Instant::now(),
            timeout,
        }
    }

    pub fn remaining(self) -> Duration {
        self.timeout.saturating_sub(self.started.elapsed())
    }

    pub fn instant(self) -> Option<Instant> {
        self.started.checked_add(self.timeout)
    }

    fn socket_timeout(self) -> io::Result<Duration> {
        let timeout = self.remaining();
        if timeout.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(timeout.min(Duration::from_secs(3600)))
    }
}

pub(crate) struct Request {
    pub method: String,
    pub target: String,
    pub(crate) headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    #[cfg(test)]
    pub fn test_get(target: &str) -> Self {
        Self {
            method: "GET".into(),
            target: target.into(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn header<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        self.headers(name).next()
    }

    pub fn headers<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.headers
            .iter()
            .filter_map(move |(key, value)| (key == name).then_some(value.as_str()))
    }
}

pub(crate) struct Response {
    pub status: u16,
    pub headers: Vec<(&'static str, &'static str)>,
    pub body: Vec<u8>,
}

impl Response {
    pub(crate) fn reason(&self) -> &'static str {
        match self.status {
            200 => "OK",
            400 => "Bad Request",
            404 => "Not Found",
            405 => "Method Not Allowed",
            406 => "Not Acceptable",
            413 => "Content Too Large",
            415 => "Unsupported Media Type",
            417 => "Expectation Failed",
            431 => "Request Header Fields Too Large",
            500 => "Internal Server Error",
            503 => "Service Unavailable",
            _ => "Unknown",
        }
    }

    pub fn empty(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn with_body(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type", content_type)],
            body: body.into(),
        }
    }

    pub fn allow_origin(&mut self) {
        self.headers.push(("Access-Control-Allow-Origin", "*"));
    }
}

struct Socket<'a> {
    stream: &'a TcpStream,
    deadline: Deadline,
}

impl Read for Socket<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(
            self.deadline.socket_timeout()?.min(Duration::from_secs(1)),
        ))?;
        self.stream.read(buf)
    }
}

impl Write for Socket<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream
            .set_write_timeout(Some(self.deadline.socket_timeout()?))?;
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReadError {
    BadRequest,
    HeaderLimit,
    BodyLimit,
    Expectation,
    Timeout,
    Closed,
}

impl ReadError {
    pub fn status(&self) -> Option<u16> {
        Some(match self {
            Self::BadRequest => 400,
            Self::HeaderLimit => 431,
            Self::BodyLimit => 413,
            Self::Expectation => 417,
            Self::Timeout => 503,
            Self::Closed => return None,
        })
    }
}

impl From<io::Error> for ReadError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => Self::Timeout,
            _ => Self::Closed,
        }
    }
}

type ReadResult<T> = Result<T, ReadError>;

pub(crate) struct RequestReader<'a>(BufReader<Socket<'a>>);

impl<'a> RequestReader<'a> {
    pub fn new(stream: &'a TcpStream, deadline: Deadline) -> Self {
        Self(BufReader::new(Socket { stream, deadline }))
    }

    pub fn head(&mut self) -> ReadResult<Request> {
        read_head(&mut self.0)
    }

    pub fn body(&mut self, request: &Request, limit: usize) -> ReadResult<Vec<u8>> {
        let framing = framing(request, limit)?;
        let mut expect = request.headers("expect");
        if let Some(value) = expect.next() {
            if !value.eq_ignore_ascii_case("100-continue") || expect.next().is_some() {
                return Err(ReadError::Expectation);
            }
            if framing != Framing::Length(0) {
                self.0
                    .get_mut()
                    .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
            }
        }
        match framing {
            Framing::Length(length) => {
                let mut body = Vec::new();
                append_bytes(&mut self.0, &mut body, length)?;
                Ok(body)
            }
            Framing::Chunked => read_chunks(&mut self.0, limit),
        }
    }
}

fn read_line(
    reader: &mut impl BufRead,
    line: &mut Vec<u8>,
    remaining: &mut usize,
    limit_error: ReadError,
) -> ReadResult<()> {
    line.clear();
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            return Err(ReadError::Closed);
        }
        let newline = bytes.iter().position(|&byte| byte == b'\n');
        let take = newline.map_or(bytes.len(), |index| index + 1);
        if take > *remaining {
            return Err(limit_error);
        }
        line.extend_from_slice(&bytes[..take]);
        reader.consume(take);
        *remaining -= take;
        if newline.is_some() {
            if !line.ends_with(b"\r\n") {
                return Err(ReadError::BadRequest);
            }
            line.truncate(line.len() - 2);
            return Ok(());
        }
    }
}

fn read_head(reader: &mut impl BufRead) -> ReadResult<Request> {
    let mut line = Vec::new();
    let mut remaining = MAX_HEADER_BYTES;
    read_line(reader, &mut line, &mut remaining, ReadError::HeaderLimit)?;
    let request_line = std::str::from_utf8(&line).map_err(|_| ReadError::BadRequest)?;
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target), Some(version @ ("HTTP/1.0" | "HTTP/1.1")), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(ReadError::BadRequest);
    };
    if !is_token(method.as_bytes()) {
        return Err(ReadError::BadRequest);
    }
    let target = request_target(method, target).ok_or(ReadError::BadRequest)?;
    let require_host = version == "HTTP/1.1";
    let mut request = Request {
        method: method.to_owned(),
        target: target.into_owned(),
        headers: Vec::new(),
        body: Vec::new(),
    };
    loop {
        read_line(reader, &mut line, &mut remaining, ReadError::HeaderLimit)?;
        if line.is_empty() {
            break;
        }
        if request.headers.len() == MAX_HEADER_COUNT {
            return Err(ReadError::HeaderLimit);
        }
        let (name, value) = parse_field(&line)?;
        request.headers.push((name, value));
    }
    {
        let mut host = request.headers("host");
        let first = host.next();
        if (require_host && first.is_none())
            || first.is_some_and(|value| !valid_host(value))
            || host.next().is_some()
        {
            return Err(ReadError::BadRequest);
        }
    }
    Ok(request)
}

fn request_target<'a>(method: &str, target: &'a str) -> Option<Cow<'a, str>> {
    if !target
        .bytes()
        .all(|byte| matches!(byte, b'!'..=b'~') && byte != b'#')
    {
        return None;
    }
    if method == "CONNECT" {
        let port = target.rsplit_once(':')?.1;
        return (valid_host(target) && valid_port(port)).then_some(Cow::Borrowed(target));
    }
    if target.starts_with('/') || (target == "*" && method == "OPTIONS") {
        return Some(Cow::Borrowed(target));
    }
    let (scheme, absolute) = target.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let (authority, rest) = absolute.split_at(absolute.find(['/', '?']).unwrap_or(absolute.len()));
    if !valid_host(authority) {
        return None;
    }
    Some(match rest.as_bytes().first() {
        Some(b'/') => Cow::Borrowed(rest),
        Some(b'?') => Cow::Owned(format!("/{rest}")),
        _ => Cow::Borrowed("/"),
    })
}

fn valid_host(host: &str) -> bool {
    let (name, port) = if let Some(literal) = host.strip_prefix('[') {
        let Some((ip, rest)) = literal.split_once(']') else {
            return false;
        };
        return ip.parse::<Ipv6Addr>().is_ok()
            && (rest.is_empty() || rest.strip_prefix(':').is_some_and(valid_port));
    } else {
        host.split_once(':')
            .map_or((host, None), |(name, port)| (name, Some(port)))
    };
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._~!$&'()*+;=%".contains(&byte))
        && port.is_none_or(valid_port)
}

fn valid_port(port: &str) -> bool {
    !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_token(value: &[u8]) -> bool {
    !value.is_empty() && value.iter().copied().all(token_byte)
}

fn token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn parse_field(line: &[u8]) -> ReadResult<(String, String)> {
    let colon = line
        .iter()
        .position(|&byte| byte == b':')
        .ok_or(ReadError::BadRequest)?;
    let name = &line[..colon];
    let value = &line[colon + 1..];
    if !is_token(name)
        || value
            .iter()
            .any(|&byte| byte != b'\t' && byte.is_ascii_control())
    {
        return Err(ReadError::BadRequest);
    }
    Ok((
        String::from_utf8_lossy(name).to_ascii_lowercase(),
        String::from_utf8_lossy(value)
            .trim_matches([' ', '\t'])
            .to_owned(),
    ))
}

#[derive(Debug, PartialEq, Eq)]
enum Framing {
    Length(usize),
    Chunked,
}

fn framing(request: &Request, limit: usize) -> ReadResult<Framing> {
    let has_length = request.header("content-length").is_some();
    let mut transfer = request.headers("transfer-encoding");
    if let Some(coding) = transfer.next() {
        if has_length || transfer.next().is_some() || !coding.eq_ignore_ascii_case("chunked") {
            return Err(ReadError::BadRequest);
        }
        return Ok(Framing::Chunked);
    }
    let mut lengths = request
        .headers("content-length")
        .flat_map(|value| value.split(','));
    let Some(length) = lengths.next() else {
        return Ok(Framing::Length(0));
    };
    let length = decimal_length(length)?;
    for other in lengths {
        if decimal_length(other)? != length {
            return Err(ReadError::BadRequest);
        }
    }
    if length.is_empty() {
        return Ok(Framing::Length(0));
    }
    let length: usize = length.parse().map_err(|_| ReadError::BodyLimit)?;
    if length > limit {
        return Err(ReadError::BodyLimit);
    }
    Ok(Framing::Length(length))
}

fn decimal_length(value: &str) -> ReadResult<&str> {
    let value = value.trim_matches([' ', '\t']);
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ReadError::BadRequest);
    }
    Ok(value.trim_start_matches('0'))
}

fn append_bytes(reader: &mut impl Read, body: &mut Vec<u8>, mut count: usize) -> ReadResult<()> {
    let mut buffer = [0; 8192];
    while count > 0 {
        let length = count.min(buffer.len());
        reader.read_exact(&mut buffer[..length])?;
        body.extend_from_slice(&buffer[..length]);
        count -= length;
    }
    Ok(())
}

fn read_chunks(reader: &mut impl BufRead, limit: usize) -> ReadResult<Vec<u8>> {
    let mut body = Vec::new();
    let mut line = Vec::new();
    let mut remaining = MAX_FRAMING_BYTES;
    loop {
        read_line(reader, &mut line, &mut remaining, ReadError::HeaderLimit)?;
        let size = chunk_size(&line)?;
        if size == 0 {
            read_trailers(reader, &mut remaining, &mut line)?;
            return Ok(body);
        }
        if size > limit.saturating_sub(body.len()) {
            return Err(ReadError::BodyLimit);
        }
        append_bytes(reader, &mut body, size)?;
        remaining = remaining.checked_sub(2).ok_or(ReadError::HeaderLimit)?;
        let mut end = [0; 2];
        reader.read_exact(&mut end)?;
        if end != *b"\r\n" {
            return Err(ReadError::BadRequest);
        }
    }
}

fn chunk_size(line: &[u8]) -> ReadResult<usize> {
    let (digits, extensions) = line.split_at(
        line.iter()
            .take_while(|byte| byte.is_ascii_hexdigit())
            .count(),
    );
    if digits.is_empty() || !valid_chunk_extensions(extensions) {
        return Err(ReadError::BadRequest);
    }
    let digits = std::str::from_utf8(digits).map_err(|_| ReadError::BadRequest)?;
    usize::from_str_radix(digits, 16).map_err(|_| ReadError::BodyLimit)
}

fn valid_chunk_extensions(mut bytes: &[u8]) -> bool {
    while !bytes.is_empty() {
        let Some(rest) = skip_space(bytes).strip_prefix(b";") else {
            return false;
        };
        let rest = skip_space(rest);
        let name_len = rest.iter().take_while(|&&b| token_byte(b)).count();
        if name_len == 0 {
            return false;
        }
        bytes = &rest[name_len..];
        if let Some(value) = skip_space(bytes).strip_prefix(b"=") {
            let value = skip_space(value);
            if let Some(quoted) = value.strip_prefix(b"\"") {
                let Some(tail) = quoted_tail(quoted) else {
                    return false;
                };
                bytes = tail;
            } else {
                let length = value.iter().take_while(|&&b| token_byte(b)).count();
                if length == 0 {
                    return false;
                }
                bytes = &value[length..];
            }
        }
    }
    true
}

fn skip_space(bytes: &[u8]) -> &[u8] {
    &bytes[bytes
        .iter()
        .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
        .count()..]
}

fn quoted_tail(mut bytes: &[u8]) -> Option<&[u8]> {
    while let Some((&byte, tail)) = bytes.split_first() {
        bytes = tail;
        match byte {
            b'"' => return Some(tail),
            b'\\' => {
                let (&escaped, tail) = tail.split_first()?;
                if escaped.is_ascii_control() && escaped != b'\t' {
                    return None;
                }
                bytes = tail;
            }
            b'\t' => {}
            byte if byte.is_ascii_control() => return None,
            _ => {}
        }
    }
    None
}

fn read_trailers(
    reader: &mut impl BufRead,
    remaining: &mut usize,
    line: &mut Vec<u8>,
) -> ReadResult<()> {
    for index in 0..=MAX_TRAILER_COUNT {
        read_line(reader, line, remaining, ReadError::HeaderLimit)?;
        if line.is_empty() {
            return Ok(());
        }
        if index == MAX_TRAILER_COUNT {
            break;
        }
        let (name, _) = parse_field(line)?;
        if matches!(
            name.as_str(),
            "transfer-encoding" | "content-length" | "host"
        ) {
            return Err(ReadError::BadRequest);
        }
    }
    Err(ReadError::HeaderLimit)
}

pub(crate) fn write_response(
    stream: &TcpStream,
    response: Response,
    head: bool,
    deadline: Deadline,
) -> io::Result<()> {
    let mut output = Socket { stream, deadline };
    let mut headers = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        response.reason(),
        response.body.len(),
    );
    for (name, value) in response.headers {
        headers.push_str(&format!("{name}: {value}\r\n"));
    }
    headers.push_str("\r\n");
    output.write_all(headers.as_bytes())?;
    if !head {
        output.write_all(&response.body)?;
    }
    output.flush()
}
