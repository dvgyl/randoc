use randoc_archive::media::decode_percent_bytes;

pub(crate) fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub(crate) fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

pub(crate) fn join(dir: &str, path: &str) -> String {
    if dir.is_empty() {
        path.to_owned()
    } else {
        format!("{dir}/{path}")
    }
}

pub(crate) fn external(url: &str) -> bool {
    url.starts_with("//")
        || url.split_once(':').is_some_and(|(scheme, _)| {
            scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"+-.".contains(&c))
        })
}

pub(crate) fn decode(path: &str) -> String {
    decode_percent_bytes(path, path.len())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_else(|| path.to_owned())
}

/// Resolves a relative path within the archive root.
pub(crate) fn resolve(dir: &str, path: &str) -> Option<String> {
    if path.starts_with('/') || path.contains(['\\', ':']) || path.chars().any(char::is_control) {
        return None;
    }
    let parts = normalize_parts(dir.split('/').chain(path.split('/')));
    (!parts.is_empty() && parts.first() != Some(&"..")).then(|| parts.join("/"))
}

pub(crate) fn normalize_relative(path: &str) -> String {
    normalize_parts(path.split('/')).join("/")
}

fn normalize_parts<'a>(parts: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut output = Vec::new();
    for part in parts {
        match part {
            "" | "." => {}
            ".." if output.last().is_some_and(|last| *last != "..") => {
                output.pop();
            }
            _ => output.push(part),
        }
    }
    output
}

pub(crate) fn escape(text: &str) -> String {
    let mut result = String::new();
    randoc_archive::xml::escape_xml_attr(text, &mut result);
    result
}
