use std::collections::HashMap;
use std::io::ErrorKind;

use randoc_archive::media::{
    decode_data_uri, extension_for_mime, image_size_and_mime, mime_from_extension,
};
use randoc_core::{Context, Error, LogMessage, MediaItem, Result};
use randoc_templates::Val;

use crate::path;

const DEFAULT_CSS: &str = "body { line-height: 1.4; margin: 5%; }\nimg, svg { max-width: 100%; }\npre { white-space: pre-wrap; }\ntable { border-collapse: collapse; }\nth, td { padding: .25em; }\n.titlepage, #cover-image { text-align: center; }\nol.toc { list-style-type: none; padding-left: 1.25em; }\n";

pub(super) struct Resource {
    pub id: String,
    pub href: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub(super) struct Resources {
    pub media: Vec<Resource>,
    mapped: HashMap<String, String>,
    remaining: u64,
}

impl Resources {
    pub fn new(limit: u64) -> Self {
        Self {
            media: Vec::new(),
            mapped: HashMap::new(),
            remaining: limit,
        }
    }

    pub fn embed(&mut self, source: &str, context: &mut Context) -> Result<String> {
        if let Some(value) = self.mapped.get(source) {
            return Ok(value.clone());
        }
        let url = match fetch(source, context) {
            Ok(item) => {
                self.add_media(source, item)?;
                format!("../{}", self.media.last().map_or("", |file| &file.href))
            }
            Err(error @ (Error::ResourceNotFound(_) | Error::Http { .. })) => {
                let reason = match error {
                    Error::Http { url, message } => format!("PandocHttpError {url:?} {message:?}"),
                    _ => format!("PandocResourceNotFound {source:?}"),
                };
                context.report(LogMessage::CouldNotFetchResource(source.into(), reason));
                format!("../{source}")
            }
            Err(error) => return Err(error),
        };
        self.mapped.insert(source.into(), url.clone());
        Ok(url)
    }

    pub fn add_media(&mut self, source: &str, item: MediaItem) -> Result<()> {
        self.charge(item.contents.len())?;
        let extension = extension_for_mime(&item.mime_type).or_else(|| {
            let (_, ext) = path::basename(source).rsplit_once('.')?;
            ext.bytes()
                .all(|c| c.is_ascii_alphanumeric())
                .then_some(ext)
        });
        let n = self.media.len();
        let (id, href) = match extension {
            Some(ext) => (format!("file{n}_{ext}"), format!("media/file{n}.{ext}")),
            None => (format!("file{n}"), format!("media/file{n}")),
        };
        self.media.push(Resource {
            id,
            href,
            mime: item.mime_type,
            bytes: item.contents,
        });
        Ok(())
    }

    pub fn charge(&mut self, size: usize) -> Result<()> {
        self.remaining = self
            .remaining
            .checked_sub(size as u64)
            .ok_or_else(|| Error::App("The EPUB exceeds the archive size limit.".into()))?;
        Ok(())
    }
}

pub(super) fn fetch(source: &str, context: &mut Context) -> Result<MediaItem> {
    let limit = context.limits.max_resource_size;
    if let Some(item) = context.media.get(source) {
        check_size(source, item.contents.len(), limit)?;
        return Ok(item.clone());
    }
    let (contents, mime) = if source.starts_with("data:") {
        let (mime, contents) =
            decode_data_uri(source, limit).ok_or_else(|| Error::ResourceNotFound(source.into()))?;
        (contents, Some(mime))
    } else if source.starts_with("http://")
        || source.starts_with("https://")
        || source.starts_with("//")
    {
        let url = if source.starts_with("//") {
            let scheme = if context
                .state
                .source_url
                .as_deref()
                .is_some_and(|base| base.starts_with("https://"))
            {
                "https"
            } else {
                "http"
            };
            format!("{scheme}:{source}")
        } else {
            source.into()
        };
        let headers = context.state.request_headers.clone();
        context.host().fetch_url(&url, &headers)?
    } else {
        let decoded = path::decode(source.strip_prefix("file://").unwrap_or(source));
        (local(&decoded, context)?, None)
    };
    check_size(source, contents.len(), limit)?;
    let mime_type = mime
        .map(|mime| mime.split(';').next().unwrap_or_default().trim().to_owned())
        .or_else(|| {
            image_size_and_mime(&contents, Some(source))
                .1
                .map(str::to_owned)
        })
        .or_else(|| mime_from_extension(source).map(str::to_owned))
        .unwrap_or_else(|| "application/octet-stream".into());
    Ok(MediaItem {
        mime_type,
        contents,
    })
}

fn check_size(source: &str, size: usize, limit: u64) -> Result<()> {
    if size as u64 > limit {
        return Err(Error::App(format!(
            "The resource '{source}' exceeds the size limit."
        )));
    }
    Ok(())
}

fn local(source: &str, context: &mut Context) -> Result<Vec<u8>> {
    let mut dirs = context.state.resource_path.clone();
    if source.starts_with('/') || dirs.is_empty() {
        dirs = vec![String::new()];
    }
    let limit = usize::try_from(context.limits.max_resource_size).unwrap_or(usize::MAX);
    for dir in dirs {
        let full = if dir == "." {
            source.into()
        } else {
            path::join(&dir, source)
        };
        match context.host().read_file_limited(&full, limit) {
            Ok(bytes) => return Ok(bytes),
            Err(error) if error.kind() == ErrorKind::FileTooLarge => {
                return Err(Error::App(error.to_string()));
            }
            Err(_) => {}
        }
    }
    Err(Error::ResourceNotFound(source.into()))
}

pub(super) fn value_text(value: &Val) -> Option<String> {
    match value {
        Val::Simple(doc) => Some(doc.render(None)),
        _ => None,
    }
}

pub(super) fn styles(
    css: Option<&Val>,
    resources: &mut Resources,
    context: &mut Context,
) -> Result<Vec<Resource>> {
    let mut paths: Vec<_> = match css {
        Some(Val::List(items)) => items.iter().filter_map(value_text).collect(),
        Some(value) => value_text(value).into_iter().collect(),
        None => Vec::new(),
    };
    let defaults = paths.is_empty();
    if defaults {
        paths.push("epub.css".into());
    }
    paths
        .into_iter()
        .enumerate()
        .map(|(index, source)| {
            let bytes = if defaults {
                let limit = usize::try_from(context.limits.max_resource_size).unwrap_or(usize::MAX);
                let from_data = context.state.user_data_dir.clone().map(|dir| {
                    context
                        .host()
                        .read_file_limited(&path::join(&dir, &source), limit)
                });
                match from_data {
                    Some(Ok(bytes)) => bytes,
                    Some(Err(error)) if error.kind() == ErrorKind::FileTooLarge => {
                        return Err(Error::App(error.to_string()));
                    }
                    _ => DEFAULT_CSS.as_bytes().to_vec(),
                }
            } else {
                fetch(&source, context)?.contents
            };
            resources.charge(bytes.len())?;
            let id = format!("stylesheet{}", index + 1);
            Ok(Resource {
                href: format!("styles/{id}.css"),
                id,
                mime: "text/css".into(),
                bytes,
            })
        })
        .collect()
}

pub(super) fn font(source: &str, context: &mut Context) -> Result<Resource> {
    let name = path::basename(source);
    if path::resolve("", name).as_deref() != Some(name) {
        return Err(Error::ResourceNotFound(source.into()));
    }
    let item = fetch(source, context)?;
    let id = file_id(name);
    let mime = match name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => &item.mime_type,
    };
    Ok(Resource {
        id,
        href: format!("fonts/{name}"),
        mime: mime.into(),
        bytes: item.contents,
    })
}

pub(super) fn file_id(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
