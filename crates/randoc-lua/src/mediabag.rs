//! Access to the conversion's media bag.

use std::fmt::Write;
use std::path::Path;

use mlua::{Lua, Value};
use randoc_archive::{fetch, media, mime};
use randoc_ast::{Inline, Target, walk::walk_inlines_mut};
use randoc_core::{Context, LogMessage, MediaItem, error::io_error};

use crate::construct::Call;
use crate::host::Host;
use crate::peek::{Cx, Peek, PeekError, is_none, mismatch};
use crate::{push, walk::check_document};

pub(crate) fn bytes(cx: &Cx, value: &Value) -> Peek<mlua::LuaString> {
    cx.lua
        .coerce_string(value.clone())?
        .ok_or_else(|| mismatch("string", value))
}

pub(crate) fn call(host: &Host, call: &Call) -> Peek<Vec<Value>> {
    let cx = call.cx;
    let lua = cx.lua;
    let text = |index, name| call.required(index, name, |v| cx.text(v));
    match call.name {
        "insert" => {
            let path = text(0, "filepath")?;
            let mime_type = if is_none(&call.argument(1)) {
                Some(text(1, "mimetype")?)
            } else {
                call.optional(1, "mimetype", |v| cx.text(v))?
            }
            .unwrap_or_else(|| {
                mime::by_path(&path)
                    .unwrap_or("application/octet-stream")
                    .into()
            });
            let contents = call
                .required(2, "contents", |v| bytes(cx, v))?
                .as_bytes()
                .to_vec();
            host.context(|context| {
                context.media.insert(
                    path,
                    MediaItem {
                        mime_type,
                        contents,
                    },
                );
                Ok(vec![])
            })
        }
        "lookup" => {
            let path = text(0, "filepath")?;
            let item = host.context(|context| Ok(context.media.get(&path).cloned()))?;
            match item {
                Some(item) => pair(lua, &item.mime_type, &item.contents),
                None => Ok(vec![Value::Nil, Value::Nil]),
            }
        }
        "list" | "items" => {
            let snapshot = host.context(|context| {
                Ok(context
                    .media
                    .iter()
                    .map(|(path, item)| {
                        let contents = (call.name == "items").then(|| item.contents.clone());
                        (
                            path.clone(),
                            item.mime_type.clone(),
                            item.contents.len(),
                            contents,
                        )
                    })
                    .collect::<Vec<_>>())
            })?;
            let mut items = Vec::with_capacity(snapshot.len());
            for (path, mime, len, contents) in snapshot {
                let entry = lua.create_table()?;
                if let Some(contents) = contents {
                    entry.raw_set(1, path)?;
                    entry.raw_set(2, mime)?;
                    entry.raw_set(3, lua.create_string(contents)?)?;
                } else {
                    entry.raw_set("path", path)?;
                    entry.raw_set("type", mime)?;
                    entry.raw_set("length", len)?;
                }
                items.push(Value::Table(entry));
            }
            Ok(vec![push::list_of(lua, "List", items)?])
        }
        "delete" => {
            let path = text(0, "filepath")?;
            host.context(|context| {
                context.media.remove(&path);
                Ok(vec![])
            })
        }
        "empty" => host.context(|context| {
            context.media = Default::default();
            Ok(vec![])
        }),
        "make_data_uri" => {
            let mime = text(0, "mime_type")?;
            let data = call.required(1, "raw_data", |v| bytes(cx, v))?;
            Ok(vec![push::text(lua, &data_uri(&mime, &data.as_bytes()))?])
        }
        "fetch" => {
            let source = text(0, "source")?;
            let (bytes, mime) = host.system(|context| fetch_one(host, context, &source))?;
            pair(lua, &mime, &bytes)
        }
        "fill" => {
            let mut doc = call.required(0, "doc", |v| cx.pandoc(v))?;
            check_document(cx, &doc)?;
            host.system(|context| {
                walk_inlines_mut(&mut doc, |inline| fill_image(host, context, inline));
                Ok(())
            })?;
            Ok(vec![push::pandoc(lua, &doc)?])
        }
        "write" => {
            let dir = text(0, "dir")?;
            let path = call.optional(1, "fp", |v| cx.text(v))?;
            host.require_full("pandoc.mediabag.write")?;
            host.system(|context| write_items(context, &dir, path.as_deref()))?;
            Ok(vec![])
        }
        other => Err(crate::host::missing("pandoc.mediabag", other)),
    }
}

fn pair(lua: &Lua, mime: &str, bytes: &[u8]) -> Peek<Vec<Value>> {
    Ok(vec![
        push::text(lua, mime)?,
        Value::String(lua.create_string(bytes)?),
    ])
}

fn data_uri(mime: &str, data: &[u8]) -> String {
    if !mime.starts_with("text/") {
        return format!("data:{mime};base64,{}", media::base64_encode(data));
    }
    let charset = if mime.contains(';') {
        ""
    } else {
        ";charset=utf-8"
    };
    let mut result = format!("data:{mime}{charset},");
    for &byte in data {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            result.push(char::from(byte));
        } else {
            let _ = write!(result, "%{byte:02X}");
        }
    }
    result
}

fn fetch_one(host: &Host, context: &mut Context, source: &str) -> Peek<(Vec<u8>, String)> {
    fetch::fetch_resource(
        context,
        source,
        &host.environment.resource_path,
        &host.environment.request_headers,
        context.limits.max_resource_size,
    )
    .map_err(|error| PeekError::Thrown(error.to_string()))
}

fn fill_image(host: &Host, context: &mut Context, inline: &mut Inline) {
    let Inline::Image(attr, alt, Target { url: source, title }) = inline else {
        return;
    };
    if context.media.get(source).is_some() {
        return;
    }
    match fetch_one(host, context, source) {
        Ok((contents, mime_type)) => {
            let key = fetch::media_key(source, &mime_type, &contents);
            context.media.insert(
                key,
                MediaItem {
                    mime_type,
                    contents,
                },
            );
        }
        Err(_) => {
            context.report(LogMessage::CouldNotFetchResource(
                source.clone(),
                "replacing image with description".into(),
            ));
            attr.classes
                .splice(..0, ["image".into(), "placeholder".into()]);
            attr.attributes.splice(
                ..0,
                [
                    ("original-image-src".into(), source.clone()),
                    ("original-image-title".into(), title.clone()),
                ],
            );
            let attr = std::mem::take(attr);
            *inline = Inline::Span(attr, std::mem::take(alt));
        }
    }
}

fn write_items(context: &mut Context, dir: &str, selected: Option<&str>) -> Peek<()> {
    let paths = if let Some(path) = selected {
        if context.media.get(path).is_none() {
            return Err(PeekError::Thrown(format!(
                "Resource not in mediabag: {path}"
            )));
        }
        vec![path.to_owned()]
    } else {
        context.media.iter().map(|(path, _)| path.clone()).collect()
    };
    for path in paths {
        let Some(item) = context.media.get(&path).cloned() else {
            continue;
        };
        let safe = media::sanitize_media_path(&path, &item.contents);
        let target = Path::new(dir).join(&safe).to_string_lossy().into_owned();
        context.report(LogMessage::Extracting(target.clone()));
        context
            .host()
            .write_media_file(dir, &safe, &item.contents)
            .map_err(|error| {
                PeekError::Thrown(io_error(&target, "withBinaryFile", &error).to_string())
            })?;
    }
    Ok(())
}
