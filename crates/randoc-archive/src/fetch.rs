//! Resource lookup for documents and the media bag.

use std::io::ErrorKind;
use std::path::Path;

use randoc_ast::Text;
use randoc_core::error::io_error;
use randoc_core::{Context, Error, LogMessage, Result};

use crate::media::{base64_decode_percent_lenient, decode_percent_bytes, sha1_hex};

/// Reads a data URI, a URL, or a file from the resource path.
pub fn fetch_resource(
    context: &mut Context,
    source: &str,
    resource_path: &[Text],
    headers: &[(Text, Text)],
    max_bytes: u64,
) -> Result<(Vec<u8>, Text)> {
    let max_bytes = max_bytes.min(context.limits.max_resource_size);
    let limit = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    let too_large = || Error::App(format!("{source}: the resource exceeds the size limit"));
    if let Some(data) = data_payload(source) {
        let (header, payload) = data.split_once(',').unwrap_or((data, ""));
        let (mime, bytes) = if let Some(mime) = header.strip_suffix(";base64") {
            (mime, base64_decode_percent_lenient(payload, limit))
        } else {
            (header, decode_percent_bytes(payload, limit))
        };
        return Ok((bytes.ok_or_else(too_large)?, mime.to_owned()));
    }
    if let Some(item) = context.media.get(source) {
        if item.contents.len() > limit {
            return Err(too_large());
        }
        return Ok((item.contents.clone(), item.mime_type.clone()));
    }

    let absolute_url = source.starts_with("//").then(|| {
        let scheme = context
            .state
            .source_url
            .as_deref()
            .and_then(|url| url.split_once(':'))
            .map_or("http", |(scheme, _)| scheme);
        format!("{scheme}:{source}")
    });
    let location = absolute_url.as_deref().unwrap_or(source);
    let scheme = location.split_once(':').filter(|(scheme, _)| {
        let mut bytes = scheme.bytes();
        scheme.len() > 1
            && bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
    });
    let path = match scheme {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("file") => {
            let rest = rest.split(['?', '#']).next().unwrap_or(rest);
            let path = rest.strip_prefix("//").map_or(rest, |authority| {
                authority.find('/').map_or("", |i| &authority[i..])
            });
            path.replace(' ', "%20")
        }
        Some(_) => {
            let url = location
                .split('#')
                .next()
                .unwrap_or(location)
                .replace(' ', "%20");
            context.report(LogMessage::Fetching(url.clone()));
            let (bytes, mime) = context.host().fetch_url(&url, headers)?;
            if bytes.len() > limit {
                return Err(too_large());
            }
            return Ok((bytes, mime.unwrap_or_default()));
        }
        None => decode_percent_bytes(source, source.len())
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or_else(|| Error::ResourceNotFound(source.into()))?,
    };
    let missing = || {
        let name = if scheme.is_some() {
            path.as_str()
        } else {
            source
        };
        Error::ResourceNotFound(name.into())
    };
    let absolute = Path::new(&path).is_absolute();
    let candidates = if absolute || resource_path.is_empty() {
        std::slice::from_ref(&path)
    } else {
        resource_path
    };
    for directory in candidates {
        let candidate =
            if absolute || resource_path.is_empty() || matches!(directory.as_str(), "" | ".") {
                path.clone()
            } else {
                format!("{}/{path}", directory.trim_end_matches('/'))
            };
        match context.host().read_file_limited(&candidate, limit) {
            Ok(bytes) => {
                context.report(LogMessage::LoadedResource(path.clone(), candidate));
                let mime_path = path
                    .rsplit_once('.')
                    .filter(|(_, ext)| ext.eq_ignore_ascii_case("gz"))
                    .map_or(path.as_str(), |(base, _)| base);
                return Ok((
                    bytes,
                    crate::mime::by_path(mime_path).unwrap_or_default().into(),
                ));
            }
            Err(error) if error.kind() == ErrorKind::FileTooLarge => return Err(too_large()),
            Err(error) if absolute => return Err(io_error(&candidate, "withBinaryFile", &error)),
            Err(_) => {}
        }
    }
    Err(missing())
}

fn data_payload(source: &str) -> Option<&str> {
    let (scheme, rest) = source.split_once(':')?;
    if scheme.eq_ignore_ascii_case("data") {
        rest.split('#').next()
    } else {
        None
    }
}

/// Returns the source, or a SHA-1 content key for a data URI.
pub fn media_key(source: &str, mime: &str, contents: &[u8]) -> Text {
    if data_payload(source).is_none() {
        return source.into();
    }
    let digest = sha1_hex(contents);
    match crate::mime::preferred_extension(mime) {
        Some(extension) => format!("{digest}.{extension}"),
        None => digest,
    }
}

#[cfg(test)]
mod tests {
    use randoc_core::Host;

    use super::*;

    #[test]
    fn reads_data_uris_and_applies_a_byte_limit() -> Result<()> {
        let mut context = Context::pure();
        for (source, mime, contents) in [
            ("data:,a+b", "", &b"a+b"[..]),
            (
                "data:text/plain;charset=utf-8,a%20b#fragment",
                "text/plain;charset=utf-8",
                &b"a b"[..],
            ),
            ("DATA:image/png;base64,YWI%3D", "image/png", &b"ab"[..]),
            ("data:image/png;base64,YWI", "image/png", &b"ab"[..]),
            ("data:foo", "foo", &b""[..]),
        ] {
            assert_eq!(
                fetch_resource(&mut context, source, &[], &[], 8)?,
                (contents.to_vec(), mime.into())
            );
        }
        assert!(fetch_resource(&mut context, "data:,abc", &[], &[], 2).is_err());
        assert_eq!(
            media_key("data:image/png,abc", "image/png", b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d.png"
        );
        for (mime, extension) in [
            ("application/octet-stream", ".so"),
            ("text/html;charset=utf-8", ".shtml"),
            ("image/tiff", ".tiff"),
            ("Text/Plain", ""),
        ] {
            assert_eq!(
                media_key("data:,abc", mime, b"abc"),
                format!("a9993e364706816aba3e25717850c26c9cd0d89d{extension}")
            );
        }
        assert_eq!(media_key("/a.png", "image/png", b"abc"), "/a.png");
        Ok(())
    }

    #[test]
    fn searches_the_resource_path_in_order() -> Result<()> {
        let mut context = Context::pure();
        for (path, bytes) in [("first/a.dat", b"12"), ("second/a.dat", b"34")] {
            context
                .host()
                .write_file(path, bytes)
                .map_err(|error| Error::Io(error.to_string()))?;
        }
        let paths = ["absent".into(), "first".into(), "second".into()];
        assert_eq!(
            fetch_resource(&mut context, "a%2edat", &paths, &[], 2)?,
            (b"12".to_vec(), "chemical/x-mopac-input".into())
        );
        for (path, mime) in [("a.unknown", ""), ("a.dat.gz", "chemical/x-mopac-input")] {
            context
                .host()
                .write_file(path, b"bytes")
                .map_err(|error| Error::Io(error.to_string()))?;
            assert_eq!(
                fetch_resource(&mut context, path, &[], &[], 8)?,
                (b"bytes".to_vec(), mime.into())
            );
        }
        assert!(fetch_resource(&mut context, "a.dat", &paths, &[], 1).is_err());
        assert!(matches!(
            fetch_resource(&mut context, "absent.jpg", &paths, &[], 10),
            Err(Error::ResourceNotFound(path)) if path == "absent.jpg"
        ));
        Ok(())
    }

    struct TestHost;

    impl Host for TestHost {
        fn fetch_url(
            &mut self,
            url: &str,
            headers: &[(Text, Text)],
        ) -> Result<(Vec<u8>, Option<Text>)> {
            assert_eq!(headers, [("X-Test".into(), "value".into())]);
            let mime = match url {
                "https://example.com/a" => Some("text/plain".into()),
                "https://example.com/unknown" => None,
                _ => return Err(Error::App(format!("Unexpected test URL {url}"))),
            };
            Ok((b"web".to_vec(), mime))
        }

        fn read_file(&mut self, path: &str) -> std::io::Result<Vec<u8>> {
            Err(std::io::Error::new(ErrorKind::NotFound, path.to_owned()))
        }
    }

    #[test]
    fn fetches_urls_through_the_host() -> Result<()> {
        let mut context = Context::new(Box::new(TestHost));
        let headers = [("X-Test".into(), "value".into())];
        let url = "https://example.com/a#anchor";
        assert_eq!(
            fetch_resource(&mut context, url, &[], &headers, 8)?,
            (b"web".to_vec(), "text/plain".into())
        );
        context.state.source_url = Some("https://example.com/document".into());
        assert_eq!(
            fetch_resource(&mut context, "//example.com/unknown", &[], &headers, 8)?,
            (b"web".to_vec(), String::new())
        );
        assert!(fetch_resource(&mut context, url, &[], &headers, 2).is_err());
        Ok(())
    }
}
