use std::collections::HashMap;
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::rc::Rc;

use flate2::read::GzDecoder;
use randoc_archive::media::{base64_decode_lenient, decode_percent_bytes};
use randoc_archive::mime;
use randoc_core::error::io_error;
use randoc_core::{Context, Error, Host, LogMessage, Result, Text};

use crate::css::Base;

const MAX_CSS_EXPANSIONS: usize = 16_384;
const SIZE_LIMIT: &str = "the resource exceeds the size limit";
const GZIP_ERROR: &str = "could not decompress: Codec.Compression.Zlib: compressed data stream format error (incorrect header check)";

#[derive(Clone)]
pub(crate) enum Fetched {
    AlreadyData,
    Unchanged,
    Failed,
    Item(Rc<Resource>),
}

pub(crate) struct Resource {
    pub(crate) mime: Text,
    pub(crate) bytes: Vec<u8>,
    pub(crate) base: Base,
    pub(crate) import_key: Text,
}

pub(crate) struct Fetcher<'a> {
    context: &'a mut Context,
    host: Option<&'a mut dyn Host>,
    cache: HashMap<(Option<Text>, Text), Fetched>,
    remaining: u64,
    cache_remaining: u64,
    css_bytes_remaining: u64,
    css_expansions_remaining: usize,
}

impl<'a> Fetcher<'a> {
    pub(crate) fn new(context: &'a mut Context, host: Option<&'a mut dyn Host>) -> Self {
        Self {
            remaining: context.limits.max_embedded_size,
            cache_remaining: context.limits.max_embedded_size,
            css_bytes_remaining: context.limits.max_embedded_size,
            css_expansions_remaining: MAX_CSS_EXPANSIONS,
            context,
            host,
            cache: HashMap::new(),
        }
    }

    pub(crate) fn check_deadline(&self) -> Result<()> {
        self.context.check_deadline()
    }

    pub(crate) fn fetch(&mut self, url: &str, hint: Option<&str>) -> Result<Fetched> {
        self.check_deadline()?;
        if url.starts_with("data:") {
            return Ok(Fetched::AlreadyData);
        }
        if url.starts_with('#') {
            return Ok(Fetched::Unchanged);
        }
        let hint = hint.filter(|value| !value.is_empty());
        let key = (hint.map(str::to_owned), url.to_owned());
        if let Some(result) = self.cache.get(&key) {
            return Ok(result.clone());
        }
        let result = self.load(url, hint)?;
        if let Fetched::Item(item) = &result {
            self.cache_remaining = self
                .cache_remaining
                .checked_sub(item.bytes.len() as u64)
                .ok_or_else(embed_size_error)?;
        }
        self.cache.insert(key, result.clone());
        Ok(result)
    }

    pub(crate) fn warning(&mut self, url: &str, detail: &str) {
        self.context
            .report(LogMessage::CouldNotFetchResource(url.into(), detail.into()));
    }

    fn failed(&mut self, url: &str, detail: &str) -> Result<Fetched> {
        self.warning(url, detail);
        Ok(Fetched::Failed)
    }

    pub(crate) fn max_depth(&self) -> usize {
        self.context.limits.max_nesting_depth
    }

    fn host(&mut self) -> &mut dyn Host {
        match self.host.as_deref_mut() {
            Some(host) => host,
            None => self.context.host(),
        }
    }

    pub(crate) fn charge_css_expansion(&mut self, bytes: usize) -> Result<()> {
        self.css_expansions_remaining = self
            .css_expansions_remaining
            .checked_sub(1)
            .ok_or_else(embed_size_error)?;
        self.css_bytes_remaining = self
            .css_bytes_remaining
            .checked_sub(bytes as u64)
            .ok_or_else(embed_size_error)?;
        Ok(())
    }

    pub(crate) fn check_budget(&self, bytes: usize) -> Result<()> {
        if bytes as u64 > self.remaining {
            return Err(embed_size_error());
        }
        Ok(())
    }

    pub(crate) fn charge(&mut self, bytes: usize) -> Result<()> {
        self.check_budget(bytes)?;
        self.remaining -= bytes as u64;
        Ok(())
    }

    fn load(&mut self, url: &str, hint: Option<&str>) -> Result<Fetched> {
        let key = path_key(path_part(url));
        if let Some(item) = self.context.media.get(&key) {
            if item.contents.len() > self.read_limit(&key) {
                return self.failed(url, SIZE_LIMIT);
            }
            return self.finish(
                url,
                &key,
                item.mime_type.clone(),
                item.contents.clone(),
                Base::MediaBag(directory(&key).into()),
                key.clone(),
            );
        }

        if let Some((scheme, rest)) = scheme_parts(url) {
            if scheme.eq_ignore_ascii_case("data") {
                return self.decode_data(url, rest);
            }
            if scheme.eq_ignore_ascii_case("file") {
                if let Some(authority) = path_part(rest).strip_prefix("//") {
                    let path = authority.find('/').map_or("", |start| &authority[start..]);
                    return self.file_url(&path.replace(' ', "%20"), hint);
                }
                return self.local(rest, hint);
            }
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
                return self.http(url, hint);
            }
            if self.host().is_sandboxed() {
                return self.failed(url, "");
            }
            return self.invalid_url(url, "Invalid scheme");
        }

        if url.starts_with("//") {
            let scheme = self
                .context
                .state
                .source_url
                .as_deref()
                .and_then(scheme_parts)
                .map_or("http", |(scheme, _)| scheme);
            return self.http(&format!("{scheme}:{url}"), hint);
        }
        self.local(url, hint)
    }

    fn decode_data(&mut self, url: &str, rest: &str) -> Result<Fetched> {
        let rest = rest.split('#').next().unwrap_or(rest);
        let (header, payload) = rest.split_once(',').unwrap_or((rest, ""));
        let limit = usize::try_from(self.context.limits.max_resource_size).unwrap_or(usize::MAX);
        let (mime, bytes) = if let Some(header) = header.strip_suffix(";base64") {
            let Some(payload) = decode_percent_bytes(payload, payload.len()) else {
                return self.failed(url, SIZE_LIMIT);
            };
            (
                header.trim_end(),
                base64_decode_lenient(&String::from_utf8_lossy(&payload), limit),
            )
        } else {
            (header.trim(), decode_percent_bytes(payload, limit))
        };
        let Some(bytes) = bytes else {
            return self.failed(url, SIZE_LIMIT);
        };
        Ok(Fetched::Item(Rc::new(Resource {
            mime: mime.into(),
            bytes,
            base: Base::Document,
            import_key: url.into(),
        })))
    }

    fn http(&mut self, url: &str, hint: Option<&str>) -> Result<Fetched> {
        let escaped = url.contains(' ').then(|| url.replace(' ', "%20"));
        let url = escaped.as_deref().unwrap_or(url);
        if self.host().is_sandboxed() {
            return self.failed(url, "");
        }
        self.context.report(LogMessage::Fetching(url.into()));
        let request_url = url.split('#').next().unwrap_or(url);
        let Some((_, path)) = http_parts(request_url) else {
            return self.invalid_url(url, "URL must be absolute");
        };
        let mut headers = self.context.state.request_headers.clone();
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
        {
            headers.push(("User-Agent".into(), "pandoc/3.13".into()));
        }
        match self.host().fetch_url(request_url, &headers) {
            Ok((bytes, mime)) => self.finish(
                url,
                path_part(path),
                mime.unwrap_or_else(|| fallback_mime(hint).into()),
                bytes,
                Base::Url(request_url.into()),
                request_url.into(),
            ),
            Err(Error::Http { message, .. }) => self.failed(url, &message),
            Err(error) => self.failed(url, &error.to_string()),
        }
    }

    fn invalid_url(&mut self, url: &str, reason: &str) -> Result<Fetched> {
        self.failed(url, &format!("InvalidUrlException {url:?} {reason:?}"))
    }

    fn file_url(&mut self, path: &str, hint: Option<&str>) -> Result<Fetched> {
        if self.host().is_sandboxed() {
            return self.failed(path, "");
        }
        let limit = self.read_limit(path);
        match self.host().read_file_limited(path, limit) {
            Ok(bytes) => {
                self.context
                    .report(LogMessage::LoadedResource(path.into(), path.into()));
                self.local_item(path, path, hint, bytes)
            }
            Err(error) if error.kind() == ErrorKind::FileTooLarge => self.failed(path, SIZE_LIMIT),
            Err(error) => Err(io_error(path, "withBinaryFile", &error)),
        }
    }

    fn local(&mut self, url: &str, hint: Option<&str>) -> Result<Fetched> {
        let path = path_part(url);
        let Some(path) =
            decode_percent_bytes(path, path.len()).and_then(|bytes| String::from_utf8(bytes).ok())
        else {
            return self.failed(url, "");
        };
        if self.host().is_sandboxed() {
            return self.failed(&path, "");
        }
        let absolute = Path::new(&path).is_absolute();
        let candidates = if absolute {
            vec![path.clone()]
        } else {
            self.context
                .state
                .resource_path
                .iter()
                .map(|dir| {
                    if dir == "." || dir.is_empty() {
                        path.clone()
                    } else {
                        let dir = dir.trim_end_matches('/');
                        format!("{dir}/{path}")
                    }
                })
                .collect()
        };
        let limit = self.read_limit(&path);
        for candidate in candidates {
            self.check_deadline()?;
            match self.host().read_file_limited(&candidate, limit) {
                Ok(bytes) => {
                    self.context
                        .report(LogMessage::LoadedResource(path.clone(), candidate));
                    return self.local_item(url, &path, hint, bytes);
                }
                Err(error) if error.kind() == ErrorKind::FileTooLarge => {
                    return self.failed(url, SIZE_LIMIT);
                }
                Err(error) if absolute => {
                    return Err(io_error(&candidate, "withBinaryFile", &error));
                }
                Err(_) => {}
            }
        }
        self.failed(&path, "")
    }

    fn local_item(
        &mut self,
        url: &str,
        path: &str,
        hint: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<Fetched> {
        let mime_path = if let Some((name, ext)) = path.rsplit_once('.')
            && ext.eq_ignore_ascii_case("gz")
        {
            name
        } else {
            path
        };
        let mime = mime::by_path(mime_path).unwrap_or_else(|| fallback_mime(hint));
        self.finish(
            url,
            path,
            mime.into(),
            bytes,
            Base::Path(directory(path_part(url)).into()),
            path_key(path),
        )
    }

    fn read_limit(&self, path: &str) -> usize {
        let limit = self.context.limits.max_resource_size;
        let limit = if is_gzip(path) {
            // Allow gzip headers and framing for small compressed files.
            limit.saturating_add(65_536)
        } else {
            limit
        };
        usize::try_from(limit).unwrap_or(usize::MAX)
    }

    fn within_limit(&self, len: usize) -> bool {
        len as u64 <= self.context.limits.max_resource_size
    }

    fn finish(
        &mut self,
        url: &str,
        path: &str,
        mime: Text,
        mut bytes: Vec<u8>,
        base: Base,
        import_key: Text,
    ) -> Result<Fetched> {
        if bytes.len() > self.read_limit(path) {
            return self.failed(url, SIZE_LIMIT);
        }
        if is_gzip(path) {
            let mut decoded = Vec::new();
            let limit = self
                .context
                .limits
                .max_resource_size
                .min(self.cache_remaining)
                .saturating_add(1);
            if GzDecoder::new(bytes.as_slice())
                .take(limit)
                .read_to_end(&mut decoded)
                .is_err()
            {
                return self.failed(url, GZIP_ERROR);
            }
            bytes = decoded;
        }
        if !self.within_limit(bytes.len()) {
            return self.failed(url, SIZE_LIMIT);
        }
        Ok(Fetched::Item(Rc::new(Resource {
            mime,
            bytes,
            base,
            import_key,
        })))
    }
}

fn embed_size_error() -> Error {
    Error::App("The embedded resources exceed the size limit.".into())
}

pub(crate) fn scheme_parts(url: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = url.split_once(':')?;
    let mut bytes = scheme.bytes();
    if scheme.len() < 2
        || !bytes.next()?.is_ascii_alphabetic()
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
    {
        return None;
    }
    Some((scheme, rest))
}

pub(crate) fn path_part(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

pub(crate) fn http_parts(url: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = scheme_parts(url)?;
    let authority = rest.strip_prefix("//")?;
    let end = authority.find(['/', '?', '#']).unwrap_or(authority.len());
    if end == 0 {
        return None;
    }
    Some((&url[..scheme.len() + 3 + end], &authority[end..]))
}

fn fallback_mime(hint: Option<&str>) -> &str {
    hint.unwrap_or("application/octet-stream")
}

fn is_gzip(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gz") || ext.eq_ignore_ascii_case("svgz"))
}

fn directory(path: &str) -> &str {
    path.rsplit_once('/')
        .map_or(".", |(dir, _)| if dir.is_empty() { "/" } else { dir })
}

fn path_key(path: &str) -> Text {
    let mut keys = Vec::new();
    for part in path.split('/') {
        match part {
            "." | "" => {}
            ".." if keys.last().is_some_and(|last| *last != "..") => {
                keys.pop();
            }
            ".." if path.starts_with('/') => {}
            _ => keys.push(part),
        }
    }
    if path.starts_with('/') {
        format!("/{}", keys.join("/"))
    } else {
        keys.join("/")
    }
}
