//! Named text sources for the Lua interface.

use std::cell::Cell;

use mlua::{UserData, UserDataFields, Value};
use randoc_core::Source;

use crate::construct::Call;
use crate::object::typed_fields;
use crate::peek::{Cx, Peek, PeekError, described_type, mismatch};
use crate::push;

struct LuaSource(Source);

impl UserData for LuaSource {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        typed_fields(fields, "Source", &[]);
    }
}

fn source(cx: &Cx, value: &Value) -> Peek<Source> {
    if let Value::UserData(data) = value
        && let Ok(source) = data.borrow::<LuaSource>()
    {
        return Ok(source.0.clone());
    }
    if let Ok(text) = cx.text(value) {
        return Ok(Source {
            name: String::new(),
            text,
        });
    }
    let Value::Table(table) = value else {
        return Err(PeekError::failure("all choices failed"));
    };
    let text_field = |key: &str, index: i64| -> Peek<String> {
        let value: Value = table.raw_get(key)?;
        let value = if matches!(value, Value::Nil) {
            table.raw_get(index)?
        } else {
            value
        };
        cx.text(&value)
            .map_err(|_| PeekError::failure("all choices failed"))
    };
    let name = text_field("name", 1)?;
    let text = text_field("text", 2)?
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        + "\n";
    Ok(Source { name, text })
}

pub(crate) fn read(value: &Value) -> Peek<Option<Source>> {
    match value {
        Value::UserData(data) if data.is::<LuaSource>() => {
            Ok(Some(data.borrow::<LuaSource>()?.0.clone()))
        }
        _ => Ok(None),
    }
}

fn charge(remaining: &Cell<usize>, source: &Source) -> Peek<()> {
    let length = source.name.len().saturating_add(source.text.len());
    let left = remaining
        .get()
        .checked_sub(length)
        .ok_or_else(|| PeekError::Thrown("sources exceed the size limit".into()))?;
    remaining.set(left);
    Ok(())
}

pub(crate) fn call(call: &Call) -> Peek<Value> {
    let cx = call.cx;
    let value = call.argument(0);
    let remaining = Cell::new(64 << 20);
    match call.name {
        "Sources" => {
            let sources = if matches!(value, Value::Table(_)) {
                call.required(0, "srcs", |value| {
                    cx.list(value, |item| {
                        let source = source(cx, item)?;
                        charge(&remaining, &source)?;
                        Ok(source)
                    })
                })?
            } else {
                let source = source(cx, &value).map_err(|_| {
                    PeekError::Thrown(format!("table expected, got {}", described_type(&value)))
                })?;
                charge(&remaining, &source)?;
                vec![source]
            };
            let table = cx.lua.create_table_with_capacity(sources.len(), 0)?;
            for (index, source) in sources.into_iter().enumerate() {
                table.raw_set(index + 1, cx.lua.create_userdata(LuaSource(source))?)?;
            }
            return Ok(Value::Table(table));
        }
        "concat" => {
            let parts = cx
                .list(&value, |item| {
                    let source = read(item)?.ok_or_else(|| mismatch("Source", item))?;
                    charge(&remaining, &source)?;
                    Ok(source)
                })
                .map_err(PeekError::thrown)?;
            let mut text = String::with_capacity(parts.iter().map(|part| part.text.len()).sum());
            for part in parts {
                text.push_str(&part.text);
            }
            return Ok(push::text(cx.lua, &text)?);
        }
        _ => {}
    }
    let source = read(&value)?.ok_or_else(|| mismatch("Source", &value))?;
    if call.name == "tostring" {
        return Ok(push::text(cx.lua, &source.text)?);
    }
    let name = cx.text(&call.argument(1)).unwrap_or_default();
    let text = match name.as_str() {
        "name" => source.name,
        "text" => source.text,
        _ if call.name == "get" => return Ok(Value::Nil),
        _ => return Err(PeekError::Thrown("Cannot set unknown property.".into())),
    };
    if call.name == "set" {
        return Err(PeekError::Thrown(format!(
            "'{name}' is a read-only property."
        )));
    }
    Ok(push::text(cx.lua, &text)?)
}
