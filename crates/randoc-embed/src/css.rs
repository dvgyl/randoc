use randoc_core::{Result, Text};

use crate::data_uri::data_uri;
use crate::fetch::{Fetched, Fetcher, Resource, http_parts, path_part, scheme_parts};

pub(crate) enum Base {
    Document,
    Path(Text),
    Url(Text),
    MediaBag(Text),
}

impl Base {
    fn resolve(&self, url: &str) -> Text {
        if url.starts_with('#') || scheme_parts(url).is_some() {
            return url.into();
        }
        match self {
            Self::Url(base) => resolve_http(base, url),
            _ if url.starts_with('/') => url.into(),
            Self::Document => url.into(),
            Self::Path(dir) | Self::MediaBag(dir) if url.is_empty() => dir.clone(),
            Self::Path(dir) | Self::MediaBag(dir) => {
                format!("{}/{url}", dir.trim_end_matches('/'))
            }
        }
    }
}

fn resolve_http(base: &str, relative: &str) -> Text {
    let Some((origin, rest)) = http_parts(base) else {
        return relative.into();
    };
    if relative.starts_with("//") {
        let scheme = scheme_parts(origin).map_or("http", |(scheme, _)| scheme);
        return format!("{scheme}:{relative}");
    }
    let path = path_part(relative);
    if path.is_empty() {
        return if relative.is_empty() {
            base.into()
        } else {
            format!("{origin}{}{relative}", path_part(rest))
        };
    }
    let directory = path_part(rest).rsplit_once('/').map_or("", |(dir, _)| dir);
    let full = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("{directory}/{path}")
    };
    let mut segments = Vec::new();
    let mut parts = full[1..].split('/').peekable();
    while let Some(part) = parts.next() {
        match part {
            ".." => {
                segments.pop();
            }
            "." => {}
            _ => {
                segments.push(part);
                continue;
            }
        }
        if parts.peek().is_none() {
            segments.push("");
        }
    }
    format!("{origin}/{}{}", segments.join("/"), &relative[path.len()..])
}

pub(crate) fn process(
    bytes: &[u8],
    base: &Base,
    import_key: Option<&str>,
    fetcher: &mut Fetcher<'_>,
) -> Result<(Vec<u8>, usize)> {
    let mut output = CssOutput::default();
    let mut stack = import_key.into_iter().map(str::to_owned).collect();
    scan(bytes, base, fetcher, &mut stack, 0, &mut output)?;
    Ok((output.bytes, output.added))
}

#[derive(Default)]
struct CssOutput {
    bytes: Vec<u8>,
    added: usize,
}

impl CssOutput {
    fn emit(&mut self, bytes: &[u8], added: bool, fetcher: &Fetcher<'_>) -> Result<()> {
        if added {
            let text_bytes = if self.bytes.is_empty() {
                bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes)
            } else {
                bytes
            };
            let count = text_bytes.iter().filter(|&&byte| byte != b'\r').count();
            self.added = self.added.saturating_add(count);
            fetcher.check_budget(self.added)?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}

fn scan(
    mut input: &[u8],
    base: &Base,
    fetcher: &mut Fetcher<'_>,
    stack: &mut Vec<Text>,
    depth: usize,
    output: &mut CssOutput,
) -> Result<()> {
    let external = !stack.is_empty();
    let start = output.bytes.len();
    let mut delimiters = Delimiters::default();
    let mut steps = 0usize;
    while let Some((&byte, rest)) = input.split_first() {
        if steps.is_multiple_of(1024) {
            fetcher.check_deadline()?;
        }
        steps = steps.wrapping_add(1);
        if let Some(comment) = input.strip_prefix(b"/*")
            && let Some(end) = delimiters.comment_end(comment)
        {
            input = &comment[end + 2..];
        } else if whitespace(byte) {
            // Imported CSS collapses whitespace at the boundaries of its own imports.
            let follows_whitespace = depth > 0
                && output.bytes.len() > start
                && output.bytes.last().is_some_and(|&byte| whitespace(byte));
            if !follows_whitespace {
                output.emit(&input[..1], external, fetcher)?;
            }
            input = trim_start(rest);
        } else if let Some(after) = input.strip_prefix(b"@import") {
            if let Some((target, tail)) = import_target(after, &mut delimiters) {
                let url = target.resolve(base);
                let expanded = match url {
                    Some(url) => import(&url, fetcher, stack, depth, output)?,
                    None => false,
                };
                if !expanded {
                    output.emit(&input[..input.len() - tail.len()], external, fetcher)?;
                }
                input = tail;
            } else {
                output.emit(b"@import", external, fetcher)?;
                input = after;
            }
        } else if let Some((target, tail)) = url_target(input, &mut delimiters) {
            output.emit(b"url(", external, fetcher)?;
            let uri = match target.resolve(base) {
                Some(url) => encode_url(&url, fetcher, stack, depth)?,
                None => None,
            };
            if let Some(uri) = uri {
                output.emit(uri.as_bytes(), true, fetcher)?;
            } else {
                if let Some(quote) = target.quote {
                    output.emit(&[quote], external, fetcher)?;
                }
                output.emit(target.value, external, fetcher)?;
                if let Some(quote) = target.quote {
                    output.emit(&[quote], external, fetcher)?;
                }
            }
            output.emit(b")", external, fetcher)?;
            input = tail;
        } else {
            // Copy text up to the next possible rule in one step.
            let end = rest
                .iter()
                .position(|&byte| whitespace(byte) || matches!(byte, b'/' | b'@' | b'u'))
                .map_or(input.len(), |index| index + 1);
            output.emit(&input[..end], external, fetcher)?;
            input = &input[end..];
        }
    }
    Ok(())
}

fn import(
    url: &str,
    fetcher: &mut Fetcher<'_>,
    stack: &mut Vec<Text>,
    depth: usize,
    output: &mut CssOutput,
) -> Result<bool> {
    if depth >= fetcher.max_depth() {
        fetcher.warning(url, "too many nested @import rules");
        return Ok(false);
    }
    let Fetched::Item(item) = fetcher.fetch(url, None)? else {
        return Ok(false);
    };
    if stack.contains(&item.import_key) {
        fetcher.warning(url, "the @import rules form a cycle");
        return Ok(false);
    }
    expand(&item, fetcher, stack, depth, output)?;
    Ok(true)
}

fn encode_url(
    url: &str,
    fetcher: &mut Fetcher<'_>,
    stack: &mut Vec<Text>,
    depth: usize,
) -> Result<Option<Text>> {
    let Fetched::Item(item) = fetcher.fetch(url, None)? else {
        return Ok(None);
    };
    if !item.mime.starts_with("text/css") {
        return data_uri(&item.mime, &item.bytes).map(Some);
    }
    if stack.contains(&item.import_key) {
        fetcher.warning(url, "the CSS resources form a cycle");
        return Ok(None);
    }
    if depth >= fetcher.max_depth() {
        fetcher.warning(url, "too many nested CSS resources");
        return Ok(None);
    }
    let mut output = CssOutput::default();
    expand(&item, fetcher, stack, depth, &mut output)?;
    data_uri(&item.mime, &output.bytes).map(Some)
}

fn expand(
    item: &Resource,
    fetcher: &mut Fetcher<'_>,
    stack: &mut Vec<Text>,
    depth: usize,
    output: &mut CssOutput,
) -> Result<()> {
    fetcher.charge_css_expansion(item.bytes.len())?;
    stack.push(item.import_key.clone());
    let result = scan(&item.bytes, &item.base, fetcher, stack, depth + 1, output);
    stack.pop();
    result
}

#[derive(Clone, Copy)]
struct Target<'a> {
    value: &'a [u8],
    quote: Option<u8>,
}

impl Target<'_> {
    fn resolve(self, base: &Base) -> Option<Text> {
        let url = std::str::from_utf8(self.value).ok()?;
        if url.starts_with("data:") || url.starts_with('#') {
            return None;
        }
        Some(base.resolve(url))
    }
}

#[derive(Default)]
struct Delimiters {
    comment: Option<usize>,
    paren: Option<usize>,
    single_quote: Option<usize>,
    double_quote: Option<usize>,
}

impl Delimiters {
    fn comment_end(&mut self, input: &[u8]) -> Option<usize> {
        if self.comment.is_some_and(|len| input.len() <= len) {
            return None;
        }
        let end = input.windows(2).position(|pair| pair == b"*/");
        if end.is_none() {
            self.comment = Some(input.len());
        }
        end
    }

    fn search(input: &[u8], delimiter: u8, missing: &mut Option<usize>) -> Option<usize> {
        if missing.is_some_and(|len| input.len() <= len) {
            return None;
        }
        let end = input.iter().position(|&byte| byte == delimiter);
        if end.is_none() {
            *missing = Some(input.len());
        }
        end
    }
}

fn import_target<'a>(
    input: &'a [u8],
    delimiters: &mut Delimiters,
) -> Option<(Target<'a>, &'a [u8])> {
    let input = trim_start(input);
    let (target, rest) =
        quoted_target(input, delimiters).or_else(|| url_target(input, delimiters))?;
    Some((target, trim_start(rest).strip_prefix(b";")?))
}

fn url_target<'a>(input: &'a [u8], delimiters: &mut Delimiters) -> Option<(Target<'a>, &'a [u8])> {
    let input = trim_start(input.strip_prefix(b"url(")?);
    if matches!(input.first(), Some(b'\'' | b'"')) {
        let (target, tail) = quoted_target(input, delimiters)?;
        return Some((target, trim_start(tail).strip_prefix(b")")?));
    }
    let end = Delimiters::search(input, b')', &mut delimiters.paren)?;
    Some((
        Target {
            value: trim(&input[..end]),
            quote: None,
        },
        &input[end + 1..],
    ))
}

fn quoted_target<'a>(
    input: &'a [u8],
    delimiters: &mut Delimiters,
) -> Option<(Target<'a>, &'a [u8])> {
    let (&quote, rest) = input.split_first()?;
    let missing = match quote {
        b'\'' => &mut delimiters.single_quote,
        b'"' => &mut delimiters.double_quote,
        _ => return None,
    };
    let end = Delimiters::search(rest, quote, missing)?;
    Some((
        Target {
            value: trim(&rest[..end]),
            quote: Some(quote),
        },
        &rest[end + 1..],
    ))
}

fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

fn trim_start(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|&byte| !whitespace(byte))
        .unwrap_or(bytes.len());
    &bytes[start..]
}

fn trim(bytes: &[u8]) -> &[u8] {
    let bytes = trim_start(bytes);
    let end = bytes
        .iter()
        .rposition(|&byte| !whitespace(byte))
        .map_or(0, |index| index + 1);
    &bytes[..end]
}
