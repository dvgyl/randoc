use randoc_archive::media::{decode_data_uri, decode_percent_bytes};
use randoc_core::{Context, Error, Result};

pub(super) fn fetch(source: &str, context: &mut Context) -> Result<Vec<u8>> {
    let limit = context.limits.max_resource_size;
    let missing = || Error::ResourceNotFound(source.into());
    let bytes = if let Some(item) = context.media.get(source) {
        if item.contents.len() as u64 > limit {
            return Err(oversize(source));
        }
        item.contents.clone()
    } else if source.starts_with("data:") {
        decode_data_uri(source, limit)
            .map(|(_, bytes)| bytes)
            .ok_or_else(missing)?
    } else if source.starts_with("https://")
        || source.starts_with("http://")
        || source.starts_with("//")
    {
        let url = if source.starts_with("//") {
            let https = context
                .state
                .source_url
                .as_ref()
                .is_some_and(|url| url.starts_with("https:"));
            format!("{}:{source}", if https { "https" } else { "http" })
        } else {
            source.to_owned()
        };
        let headers = context.state.request_headers.clone();
        context.host().fetch_url(&url, &headers)?.0
    } else {
        let path = source.strip_prefix("file://").unwrap_or(source);
        let decoded =
            decode_percent_bytes(path, path.len()).and_then(|bytes| String::from_utf8(bytes).ok());
        let path = decoded.as_deref().unwrap_or(path);
        let directories = if path.starts_with('/') || context.state.resource_path.is_empty() {
            vec![String::new()]
        } else {
            context.state.resource_path.clone()
        };
        let mut bytes = None;
        for dir in directories {
            let full = if dir.is_empty() || dir == "." {
                path.to_owned()
            } else {
                format!("{dir}/{path}")
            };
            match context
                .host()
                .read_file_limited(&full, usize::try_from(limit).unwrap_or(usize::MAX))
            {
                Ok(data) => {
                    bytes = Some(data);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::FileTooLarge => {
                    return Err(oversize(source));
                }
                Err(_) => {}
            }
        }
        bytes.ok_or_else(missing)?
    };
    if bytes.len() as u64 > limit {
        return Err(oversize(source));
    }
    Ok(bytes)
}

fn oversize(source: &str) -> Error {
    Error::App(format!("The resource '{source}' exceeds the size limit."))
}
