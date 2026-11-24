//! Embeds external resources in rendered HTML.

#![forbid(unsafe_code)]

mod css;
mod data_uri;
mod fetch;
mod svg;

use randoc_core::{Context, Host, Result, Text};
use randoc_tagsoup::{RenderOptions, TokenKind, render, tokenize};

use crate::css::Base;
use crate::data_uri::{data_uri, to_text};
use crate::fetch::{Fetched, Fetcher, Resource};

const RENDER_OPTIONS: RenderOptions = RenderOptions {
    minimize: |name| {
        matches!(
            name,
            "br" | "col" | "hr" | "img" | "link" | "meta" | "path" | "rect" | "use"
        )
    },
    raw: |name| matches!(name, "script" | "style"),
};

/// Embeds supported resource references in HTML and reports unsuccessful fetches.
pub fn embed_resources(html: &str, context: &mut Context) -> Result<String> {
    embed(html, Fetcher::new(context, None))
}

/// Embeds HTML resources using a separate host for file and network access.
pub fn embed_resources_with_host(
    html: &str,
    context: &mut Context,
    host: &mut dyn Host,
) -> Result<String> {
    embed(html, Fetcher::new(context, Some(host)))
}

fn embed(html: &str, mut fetcher: Fetcher<'_>) -> Result<String> {
    fetcher.check_deadline()?;
    let mut tokens = tokenize(html)
        .into_iter()
        .map(|token| token.kind)
        .peekable();
    let mut rewritten = Vec::new();
    let mut svgs = svg::Inliner::default();
    while let Some(token) = tokens.next() {
        fetcher.check_deadline()?;
        let TokenKind::Open(name, mut attributes) = token else {
            rewritten.push(token);
            continue;
        };
        if name == "style"
            && let Some(TokenKind::Text(css)) = tokens.peek_mut()
        {
            let (processed, added) =
                css::process(css.as_bytes(), &Base::Document, None, &mut fetcher)?;
            fetcher.charge(added)?;
            // Pandoc 3.13 permits closing tags in the CSS of an existing style element.
            *css = to_text(&processed)?;
        } else if let Some(key) = match name.as_str() {
            "link" => Some("href"),
            "script" => Some("src"),
            _ => None,
        } {
            let has_close =
                matches!(tokens.peek(), Some(TokenKind::Close(close)) if close == &name);
            if let Some(inline) =
                rewrite_external(&name, key, &mut attributes, has_close, &mut fetcher)?
            {
                if has_close {
                    tokens.next();
                }
                rewritten.extend(inline);
                continue;
            }
        } else {
            if name == "img"
                && let Some(source) = svg::source(&attributes)
                && let Fetched::Item(item) =
                    fetcher.fetch(source, attribute(&attributes, "type"))?
                && item.mime == "image/svg+xml"
            {
                let has_close =
                    matches!(tokens.peek(), Some(TokenKind::Close(close)) if close == &name);
                if has_close {
                    tokens.next();
                }
                svgs.push(rewritten.len(), attributes, item, has_close);
                continue;
            }
            rewrite_attributes(&mut attributes, &mut fetcher)?;
        }
        rewritten.push(TokenKind::Open(name, attributes));
    }
    let rewritten = svgs.expand(rewritten, &mut fetcher)?;
    let mut output = String::with_capacity(html.len());
    render(&rewritten, &RENDER_OPTIONS, &mut output);
    Ok(output)
}

fn rewrite_external(
    name: &str,
    key: &str,
    attributes: &mut Vec<(Text, Text)>,
    has_close: bool,
    fetcher: &mut Fetcher<'_>,
) -> Result<Option<[TokenKind; 3]>> {
    let Some(url) = attribute(attributes, key) else {
        return Ok(None);
    };
    let hint = attribute(attributes, "type");
    let item = match fetcher.fetch(url, hint)? {
        Fetched::Failed | Fetched::Unchanged => return Ok(None),
        Fetched::AlreadyData => {
            move_first(attributes, key, url.to_owned());
            return Ok(None);
        }
        Fetched::Item(item) => item,
    };
    let uri = if name == "link"
        && (attribute(attributes, "rel") == Some("stylesheet") || item.mime.starts_with("text/css"))
    {
        let (processed, _) =
            css::process(&item.bytes, &item.base, Some(&item.import_key), fetcher)?;
        let text = to_text(&processed)?;
        if !text.contains("</")
            && !attribute(attributes, "media").is_some_and(|media| !media.is_empty())
        {
            fetcher.charge(text.len())?;
            return Ok(Some(inline_tag("style", Some("text/css"), text)));
        }
        if item.mime.starts_with("text/") {
            data_uri(&item.mime, &processed)?
        } else {
            data_uri(&item.mime, text.as_bytes())?
        }
    } else if name == "script" && has_close && item.mime.contains("javascript") {
        let text = to_text(&item.bytes)?;
        if !text
            .as_bytes()
            .windows(8)
            .any(|part| part.eq_ignore_ascii_case(b"</script"))
        {
            fetcher.charge(text.len())?;
            return Ok(Some(inline_tag("script", hint, text)));
        }
        data_uri(&item.mime, &item.bytes)?
    } else {
        encode_resource(&item, fetcher)?
    };
    fetcher.charge(uri.len())?;
    move_first(attributes, key, uri);
    Ok(None)
}

fn rewrite_attributes(attributes: &mut [(Text, Text)], fetcher: &mut Fetcher<'_>) -> Result<()> {
    let hint = attribute(attributes, "type").map(str::to_owned);
    for (name, value) in attributes {
        if matches!(
            name.as_str(),
            "src" | "poster" | "data-src" | "data-background-image"
        ) && let Fetched::Item(item) = fetcher.fetch(value, hint.as_deref())?
        {
            let uri = encode_resource(&item, fetcher)?;
            fetcher.charge(uri.len())?;
            *value = uri;
        }
    }
    Ok(())
}

fn encode_resource(item: &Resource, fetcher: &mut Fetcher<'_>) -> Result<Text> {
    if item.mime.starts_with("text/css") {
        let (processed, _) =
            css::process(&item.bytes, &item.base, Some(&item.import_key), fetcher)?;
        data_uri(&item.mime, &processed)
    } else {
        data_uri(&item.mime, &item.bytes)
    }
}

fn attribute<'a>(attributes: &'a [(Text, Text)], key: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value.as_str()))
}

fn move_first(attributes: &mut Vec<(Text, Text)>, key: &str, value: Text) {
    attributes.retain(|(name, _)| name != key);
    attributes.insert(0, (key.into(), value));
}

fn inline_tag(name: &str, mime: Option<&str>, text: Text) -> [TokenKind; 3] {
    let attributes = mime
        .into_iter()
        .map(|mime| ("type".into(), mime.into()))
        .collect();
    [
        TokenKind::Open(name.into(), attributes),
        TokenKind::Text(text),
        TokenKind::Close(name.into()),
    ]
}

#[cfg(test)]
mod tests;
