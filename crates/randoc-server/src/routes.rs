use std::io::{self, Write};

use randoc_core::Output;
use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ServerLimits;
use crate::convert::{self, Conversion, Message};
use crate::http::{Deadline, Request, Response};
use crate::params::Params;

const JSON: &str = "application/json;charset=utf-8";
const PLAIN: &str = "text/plain;charset=utf-8";
const RAW: &str = "application/octet-stream";
const CORS_ERROR: &str = "text/html; charset-utf-8";
const CORS_METHODS: &str = "GET, HEAD, POST";
const CORS_HEADERS: &str = "Content-Type, Accept, Accept-Language, Content-Language";
const MAX_BATCH: usize = 256;

pub(crate) fn route(request: Request, limits: ServerLimits, deadline: Deadline) -> Response {
    let (path, query) = request
        .target
        .split_once('?')
        .unwrap_or((&request.target, ""));
    route_path(&request, path, query, limits, deadline)
}

pub(crate) fn route_path(
    request: &Request,
    path: &str,
    query: &str,
    limits: ServerLimits,
    deadline: Deadline,
) -> Response {
    let limit = limits.max_response_bytes;
    if request.method == "OPTIONS"
        && request.header("origin").is_some()
        && let Some(method) = request.header("access-control-request-method")
    {
        return cap_response(preflight(request, method), limit);
    }

    let path = path
        .strip_suffix('/')
        .filter(|path| !path.is_empty())
        .unwrap_or(path);
    let path = percent_decode(path, false);
    let response = match (request.method.as_str(), path.as_str()) {
        ("GET" | "HEAD", "/version") => match negotiate(request, &[Media::Plain, Media::Json]) {
            Some(Media::Json) => json(&randoc::PANDOC_VERSION, limit),
            Some(Media::Plain) => Response::with_body(200, PLAIN, randoc::PANDOC_VERSION),
            _ => Response::empty(406),
        },
        ("GET" | "HEAD", "/babelmark") => {
            if negotiate(request, &[Media::Json]).is_some() {
                babelmark(query, limits, deadline)
            } else {
                Response::empty(406)
            }
        }
        ("POST", "/" | "/batch") => post(request, path == "/batch", limits, deadline),
        (_, "/" | "/batch" | "/version" | "/babelmark") => Response::empty(405),
        _ => Response::empty(404),
    };

    if deadline.remaining().is_zero() {
        return Response::empty(503);
    }
    let mut response = cap_response(response, limit);
    if request.header("origin").is_some() && response.status != 503 {
        response.allow_origin();
    }
    response
}

fn cap_response(response: Response, limit: usize) -> Response {
    if response.body.len() > limit {
        Response::empty(500)
    } else {
        response
    }
}

fn preflight(request: &Request, method: &str) -> Response {
    if !matches!(method, "GET" | "HEAD" | "POST") {
        return Response::with_body(
            400,
            CORS_ERROR,
            format!(
                "Method requested in Access-Control-Request-Method of CORS request is not supported; requested: {method}; supported are {CORS_METHODS}."
            ),
        );
    }
    let requested: Vec<_> = request
        .headers("access-control-request-headers")
        .flat_map(|value| value.split(',').map(str::trim))
        .filter(|header| !header.is_empty())
        .filter(|header| {
            !CORS_HEADERS
                .split(", ")
                .any(|allowed| allowed.eq_ignore_ascii_case(header))
        })
        .collect();
    if !requested.is_empty() {
        return Response::with_body(
            400,
            CORS_ERROR,
            format!(
                "HTTP header requested in Access-Control-Request-Headers of CORS request is not supported; requested: {}; supported are {CORS_HEADERS}.",
                requested.join(", "),
            ),
        );
    }
    let mut response = Response::empty(200);
    response.allow_origin();
    response
        .headers
        .push(("Access-Control-Allow-Methods", CORS_METHODS));
    response
        .headers
        .push(("Access-Control-Allow-Headers", CORS_HEADERS));
    response
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Media {
    Raw,
    Plain,
    Json,
}

impl Media {
    fn mime(self) -> &'static str {
        match self {
            Self::Raw => RAW,
            Self::Plain => "text/plain",
            Self::Json => "application/json",
        }
    }
}

#[derive(Clone, Copy)]
struct Accept<'a> {
    media: &'a str,
    quality: f32,
    utf8: bool,
}

fn accept_fields(request: &Request) -> Vec<Accept<'_>> {
    request
        .headers("accept")
        .flat_map(|value| value.split(','))
        .filter_map(|item| {
            let mut parts = item.split(';');
            let media = parts.next()?.trim();
            let mut quality = 1.0;
            let mut utf8 = false;
            for parameter in parts {
                let (name, value) = parameter.trim().split_once('=')?;
                if name.trim().eq_ignore_ascii_case("q") {
                    quality = value.trim().parse().ok()?;
                    break;
                }
                if !name.trim().eq_ignore_ascii_case("charset")
                    || !value.trim().trim_matches('"').eq_ignore_ascii_case("utf-8")
                {
                    return None;
                }
                utf8 = true;
            }
            (0.0..=1.0).contains(&quality).then_some(Accept {
                media,
                quality,
                utf8,
            })
        })
        .collect()
}

fn specificity(media: &str, offer: &str) -> Option<u8> {
    if media.eq_ignore_ascii_case(offer) {
        Some(2)
    } else if media == "*/*" {
        Some(0)
    } else {
        let (kind, "*") = media.split_once('/')? else {
            return None;
        };
        offer
            .split_once('/')
            .filter(|(major, _)| kind.eq_ignore_ascii_case(major))
            .map(|_| 1)
    }
}

fn acceptable(fields: &[Accept<'_>], offer: Media) -> bool {
    fields
        .iter()
        .filter(|accept| !accept.utf8 || offer != Media::Raw)
        .filter_map(|accept| {
            let rank = specificity(accept.media, offer.mime())?;
            Some(((rank, accept.utf8), accept.quality))
        })
        .max_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.total_cmp(&right.1))
        })
        .is_some_and(|(_, quality)| quality > 0.0)
}

fn negotiate(request: &Request, offers: &[Media]) -> Option<Media> {
    if request.header("accept").is_none() {
        return offers.first().copied();
    }
    // Pandoc tries its representations in server order. A quality of zero
    // excludes a representation, even when a broader range accepts it.
    let fields = accept_fields(request);
    offers
        .iter()
        .copied()
        .find(|&offer| acceptable(&fields, offer))
}

fn post(request: &Request, batch: bool, limits: ServerLimits, deadline: Deadline) -> Response {
    let limit = limits.max_response_bytes;
    let has_json = request
        .header("content-type")
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
    let offers: &[_] = if batch {
        &[Media::Json]
    } else {
        &[Media::Raw, Media::Plain, Media::Json]
    };
    let Some(media) = negotiate(request, offers) else {
        return Response::empty(406);
    };
    if !has_json {
        let accepts_all = request.header("accept").is_none() || {
            let fields = accept_fields(request);
            offers.iter().all(|&offer| acceptable(&fields, offer))
        };
        return Response::empty(if accepts_all { 415 } else { 406 });
    }
    if batch && request.body.iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b'[') {
        let items = match serde_json::from_slice::<Batch>(&request.body) {
            Ok(batch) => batch.0,
            Err(error) if error.to_string().starts_with(BATCH_LIMIT_ERROR) => {
                return error_response(400, BATCH_LIMIT_ERROR);
            }
            Err(error) => return error_response(400, json_error(&request.body, &error)),
        };
        return batch_response(&items, limits, deadline);
    }
    let value = match request_json(&request.body) {
        Ok(value) => value,
        Err(error) => return error_response(400, json_error(&request.body, &error)),
    };
    if batch {
        return error_response(
            400,
            format!(
                "Error in $: parsing [] failed, expected Array, but encountered {}",
                crate::params::kind(&value)
            ),
        );
    }
    let params = match Params::parse(&value, "$") {
        Ok(params) => params,
        Err(error) => return error_response(400, error),
    };
    let output = convert::convert(
        params,
        &mut convert::file_budget(),
        limit,
        limits.max_body_bytes,
        deadline,
    );
    match media {
        Media::Json => result_json(output, limit),
        Media::Raw | Media::Plain => match output {
            Ok(result) => output_bytes(result, media, limit),
            Err(error) => error_response(500, error.to_string()),
        },
    }
}

fn json_error(body: &[u8], error: &serde_json::Error) -> String {
    // Keep the detailed syntax diagnostic for short requests.
    if error.is_data() || body.len() > 64 << 10 {
        return error.to_string();
    }
    std::str::from_utf8(body)
        .ok()
        .and_then(|text| randoc_ast::json::parse_value(text, 128).err())
        .map_or_else(|| error.to_string(), |error| error.to_string())
}

const BATCH_LIMIT_ERROR: &str = "The batch has more than 256 requests.";

struct Batch(Vec<Value>);

impl<'de> Deserialize<'de> for Batch {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BatchVisitor;

        impl<'de> Visitor<'de> for BatchVisitor {
            type Value = Batch;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an array of conversion requests")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Batch, A::Error> {
                let mut items = Vec::new();
                let mut remaining = MAX_JSON_VALUES;
                while items.len() < MAX_BATCH {
                    let Some(item) = seq.next_element_seed(JsonValue(&mut remaining))? else {
                        return Ok(Batch(items));
                    };
                    items.push(item);
                }
                if seq.next_element::<IgnoredAny>()?.is_some() {
                    return Err(de::Error::custom(BATCH_LIMIT_ERROR));
                }
                Ok(Batch(items))
            }
        }

        deserializer.deserialize_seq(BatchVisitor)
    }
}

// Preserve the option diagnostics from Params while limiting the size of
// the intermediate JSON tree. A long text field uses only one work unit.
const MAX_JSON_VALUES: usize = 256 << 10;

fn request_json(bytes: &[u8]) -> serde_json::Result<Value> {
    let mut remaining = MAX_JSON_VALUES;
    let mut json = serde_json::Deserializer::from_slice(bytes);
    let value = JsonValue(&mut remaining).deserialize(&mut json)?;
    json.end()?;
    Ok(value)
}

struct JsonValue<'a>(&'a mut usize);

impl<'de> DeserializeSeed<'de> for JsonValue<'_> {
    type Value = Value;

    fn deserialize<D: serde::Deserializer<'de>>(self, json: D) -> Result<Value, D::Error> {
        *self.0 = self
            .0
            .checked_sub(1)
            .ok_or_else(|| de::Error::custom("The JSON request has too many values."))?;
        json.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for JsonValue<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(value.into())
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(value.into())
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(value.into())
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        Ok(value.into())
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(value.into())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(value) = seq.next_element_seed(JsonValue(self.0))? {
            items.push(value);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut fields = serde_json::Map::new();
        while let Some(key) = map.next_key()? {
            fields.insert(key, map.next_value_seed(JsonValue(self.0))?);
        }
        Ok(Value::Object(fields))
    }
}

fn batch_response(values: &[Value], limits: ServerLimits, deadline: Deadline) -> Response {
    let params = values
        .iter()
        .enumerate()
        .map(|(index, value)| Params::parse(value, &format!("$[{index}]")))
        .collect::<Result<Vec<_>, _>>();
    let params = match params {
        Ok(params) => params,
        Err(error) => return error_response(400, error),
    };
    let mut bytes = BoundedBytes::new(limits.max_response_bytes);
    let mut budget = convert::file_budget();
    let write = || -> io::Result<()> {
        bytes.write_all(b"[")?;
        for (index, params) in params.into_iter().enumerate() {
            if deadline.remaining().is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            if index > 0 {
                bytes.write_all(b",")?;
            }
            let output = convert::convert(
                params,
                &mut budget,
                bytes.remaining(),
                limits.max_body_bytes,
                deadline,
            );
            let result = json_result(output, bytes.remaining()).ok_or_else(response_limit)?;
            serde_json::to_writer(&mut bytes, &result)?;
        }
        bytes.write_all(b"]")
    };
    match write() {
        Ok(()) => Response::with_body(200, JSON, bytes.body),
        Err(error) if error.kind() == io::ErrorKind::TimedOut => Response::empty(503),
        Err(_) => Response::empty(500),
    }
}

fn babelmark(query: &str, limits: ServerLimits, deadline: Deadline) -> Response {
    let limit = limits.max_response_bytes;
    let mut known = std::collections::BTreeMap::new();
    for pair in query.split('&') {
        let (key, value) = pair
            .split_once('=')
            .map_or((pair, None), |(key, value)| (key, Some(value)));
        let key = form_decode(key);
        if matches!(key.as_str(), "text" | "from" | "to" | "standalone") {
            known.entry(key).or_insert_with(|| value.map(form_decode));
        }
    }
    let Some(text) = known.get("text").and_then(Option::as_deref) else {
        return error_response(400, "Query parameter text is required");
    };
    let mut params = Params::new(text);
    if let Some(Some(from)) = known.get("from") {
        params.from = from;
    }
    if let Some(Some(to)) = known.get("to") {
        params.to = to;
    }
    params.reader.standalone = known
        .get("standalone")
        .is_some_and(|value| matches!(value.as_deref(), None | Some("" | "1" | "true")));
    match convert::convert(
        params,
        &mut convert::file_budget(),
        limit,
        limits.max_body_bytes,
        deadline,
    ) {
        Ok(result) if fits(&result.output, true, limit) => {
            #[derive(Serialize)]
            struct Babelmark {
                html: String,
                version: &'static str,
            }
            json(
                &Babelmark {
                    html: result.encoded().0,
                    version: randoc::PANDOC_VERSION,
                },
                limit,
            )
        }
        Ok(_) => Response::empty(500),
        Err(error) => error_response(500, error.to_string()),
    }
}

fn form_decode(value: &str) -> String {
    percent_decode(value, true)
}

pub(crate) fn percent_decode(value: &str, plus_as_space: bool) -> String {
    let mut decoded = Vec::with_capacity(value.len());
    let mut bytes = value.as_bytes();
    while let Some((&byte, tail)) = bytes.split_first() {
        bytes = tail;
        match byte {
            b'+' if plus_as_space => decoded.push(b' '),
            b'%' if tail.len() >= 2 => {
                let high = (tail[0] as char).to_digit(16);
                let low = (tail[1] as char).to_digit(16);
                if let (Some(high), Some(low)) = (high, low) {
                    decoded.push((high * 16 + low) as u8);
                    bytes = &tail[2..];
                } else {
                    decoded.push(byte);
                }
            }
            _ => decoded.push(byte),
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn output_bytes(result: Conversion, media: Media, limit: usize) -> Response {
    if !fits(&result.output, media != Media::Raw, limit) {
        return Response::empty(500);
    }
    match media {
        Media::Raw => Response::with_body(200, RAW, result.output.into_bytes()),
        Media::Plain => Response::with_body(200, PLAIN, result.encoded().0),
        Media::Json => Response::empty(500),
    }
}

fn fits(output: &Output, encode: bool, limit: usize) -> bool {
    match output {
        Output::Text(text) => text.len() <= limit,
        Output::Binary(bytes) if encode => bytes.len().div_ceil(3).saturating_mul(4) <= limit,
        Output::Binary(bytes) => bytes.len() <= limit,
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum JsonResult {
    Success {
        output: String,
        base64: bool,
        messages: Vec<Message>,
    },
    Error {
        error: String,
    },
}

fn json_result(output: randoc_core::Result<Conversion>, limit: usize) -> Option<JsonResult> {
    match output {
        Ok(result) if fits(&result.output, true, limit) => {
            let (output, base64, messages) = result.encoded();
            Some(JsonResult::Success {
                output,
                base64,
                messages,
            })
        }
        Ok(_) => None,
        Err(error) => Some(JsonResult::Error {
            error: error.to_string(),
        }),
    }
}

fn result_json(output: randoc_core::Result<Conversion>, limit: usize) -> Response {
    json_result(output, limit).map_or_else(|| Response::empty(500), |result| json(&result, limit))
}

fn error_response(status: u16, body: impl Into<Vec<u8>>) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.into(),
    }
}

fn json(value: &impl Serialize, limit: usize) -> Response {
    let mut bytes = BoundedBytes::new(limit);
    if serde_json::to_writer(&mut bytes, value).is_ok() {
        Response::with_body(200, JSON, bytes.body)
    } else {
        Response::empty(500)
    }
}

fn response_limit() -> io::Error {
    io::Error::other("The response exceeds the byte limit.")
}

struct BoundedBytes {
    body: Vec<u8>,
    limit: usize,
}

impl BoundedBytes {
    fn new(limit: usize) -> Self {
        Self {
            body: Vec::new(),
            limit,
        }
    }

    fn remaining(&self) -> usize {
        self.limit - self.body.len()
    }
}

impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining() {
            return Err(response_limit());
        }
        self.body.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[test]
fn json_value_budget_applies_to_nested_fields() {
    let mut json = serde_json::Deserializer::from_str(r#"{"fields":[1,2,3]}"#);
    let result = JsonValue(&mut 4).deserialize(&mut json);
    assert!(
        matches!(result, Err(error) if error.to_string().starts_with("The JSON request has too many values."))
    );
}
