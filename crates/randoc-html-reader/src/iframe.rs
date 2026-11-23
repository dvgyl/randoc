//! Resource loading for empty iframe elements.

use randoc_archive::media::decode_data_uri;
use randoc_ast::{Attr, Block, Inline, Target, Text};
use randoc_core::{Context, Error, LogMessage, ReaderOptions, Result};

/// The frames of an iframe include use more stack than one HTML block.
const MAX_IFRAME_DEPTH: usize = 16;

pub(crate) struct Outcome {
    pub(crate) block: Option<Block>,
    pub(crate) logs: Vec<LogMessage>,
}

pub(crate) struct Iframes<'a> {
    context: &'a mut Context,
    remaining_bytes: u64,
    active: Vec<Text>,
}

impl<'a> Iframes<'a> {
    pub(crate) fn new(context: &'a mut Context) -> Self {
        Self {
            remaining_bytes: context.limits.max_input_size,
            context,
            active: Vec::new(),
        }
    }

    pub(crate) fn read(&mut self, src: &str, options: &ReaderOptions, depth: usize) -> Outcome {
        let mut logs = Vec::new();
        let block = match self.content(src, options, depth, &mut logs) {
            Ok(block) => Some(block),
            Err(error) => {
                logs.push(LogMessage::CouldNotFetchResource(
                    src.into(),
                    error.to_string(),
                ));
                None
            }
        };
        Outcome { block, logs }
    }

    fn content(
        &mut self,
        src: &str,
        options: &ReaderOptions,
        depth: usize,
        logs: &mut Vec<LogMessage>,
    ) -> Result<Block> {
        if self.context.host().is_sandboxed() {
            return Err(Error::ResourceNotFound(src.into()));
        }
        if depth == 0 || self.active.len() >= MAX_IFRAME_DEPTH {
            return Err(Error::Parse("the document is nested too deeply".into()));
        }
        if self.active.iter().any(|url| url == src) {
            return Err(Error::Parse("the iframe includes itself".into()));
        }
        let mut limits = self.context.limits;
        let limit = limits.max_resource_size.min(self.remaining_bytes);
        let (bytes, mime) = if let Some(data) = src.strip_prefix("data:") {
            if data.contains(',') {
                let (mime, bytes) = decode_data_uri(src, limit).ok_or_else(|| {
                    logs.push(LogMessage::Fetching(src.into()));
                    invalid_url(src, "Invalid URL")
                })?;
                (bytes, Some(mime))
            } else {
                (Vec::new(), None)
            }
        } else {
            logs.push(LogMessage::Fetching(src.into()));
            if !src.starts_with("http://") && !src.starts_with("https://") {
                let reason = if src.split_once(':').is_some_and(|(scheme, _)| {
                    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                        && scheme
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"+-.".contains(&c))
                }) {
                    "Invalid scheme"
                } else {
                    "Invalid URL"
                };
                return Err(invalid_url(src, reason));
            }
            if limit == 0 {
                return Err(size_error(src));
            }
            let headers = self.context.state.request_headers.clone();
            self.context.host().fetch_url(src, &headers)?
        };
        if bytes.len() as u64 > limit {
            return Err(size_error(src));
        }
        self.remaining_bytes -= bytes.len() as u64;

        let mut attr = Attr::new("", vec!["iframe".into()], Vec::new());
        let mime = mime
            .as_deref()
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim();
        let blocks = if mime.eq_ignore_ascii_case("text/html") {
            let mut input = String::from_utf8_lossy(&bytes).into_owned();
            if !input.is_empty() {
                input.push('\n');
            }
            limits.max_nesting_depth = depth;
            self.active.push(src.into());
            let parsed = crate::read_document(&input, crate::SOURCE_NAME, options, limits, self);
            self.active.pop();
            let (doc, messages) = parsed?;
            logs.extend(messages);
            doc.blocks
        } else if mime.to_ascii_lowercase().starts_with("image/") {
            vec![Block::Plain(vec![Inline::Image(
                Attr::default(),
                Vec::new(),
                Target::new(src, ""),
            )])]
        } else {
            attr.attributes.push(("src".into(), src.into()));
            Vec::new()
        };
        Ok(Block::Div(attr, blocks))
    }
}

fn invalid_url(src: &str, reason: &str) -> Error {
    Error::Http {
        url: src.into(),
        message: format!("InvalidUrlException {:?} \"{reason}\"", src.trim_start()),
    }
}

fn size_error(src: &str) -> Error {
    Error::App(format!("The resource '{src}' exceeds the size limit."))
}
