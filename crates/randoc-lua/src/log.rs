//! Log operations and the Lua representation of a log message.

use mlua::{Lua, MetaMethod, UserData, UserDataFields, UserDataMethods, Value};
use randoc_core::LogMessage;

use crate::construct::Call;
use crate::filter::caller_position;
use crate::host::Host;
use crate::object::typed_fields;
use crate::peek::{Peek, PeekError, mismatch};
use crate::{json, push};

struct LuaLogMessage(LogMessage);

impl UserData for LuaLogMessage {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        typed_fields(fields, "LogMessage", &["__tojson"]);
    }
}

struct LuaError(mlua::LuaString);

impl UserData for LuaError {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__name", "PandocError");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(this.0.clone()));
    }
}

pub(crate) fn call(host: &Host, call: &Call) -> Peek<Vec<Value>> {
    let lua = host.cx.lua;
    match call.name {
        "error" => {
            let text = call.required(0, "message", |value| call.cx.text(value))?;
            Ok(vec![Value::UserData(
                lua.create_userdata(LuaError(lua.create_string(text)?))?,
            )])
        }
        "info" | "warn" => {
            let text = call.required(0, "message", |v| call.cx.text(v))?;
            let position = caller_position(lua);
            let message = if call.name == "info" {
                LogMessage::ScriptingInfo(text, position)
            } else {
                LogMessage::ScriptingWarning(text, position)
            };
            host.context(|context| {
                context.report(message);
                Ok(vec![])
            })
        }
        "hold" => host.context(|context| {
            context.hold_messages();
            Ok(vec![])
        }),
        "release" | "list" => {
            let messages = host.context(|context| {
                Ok(if call.name == "release" {
                    context.release_messages().into_iter().rev().collect()
                } else {
                    context.log().to_vec()
                })
            })?;
            let values = messages
                .into_iter()
                .map(|message| {
                    lua.create_userdata(LuaLogMessage(message))
                        .map(Value::UserData)
                })
                .collect::<mlua::Result<Vec<_>>>()?;
            Ok(vec![push::list_of(lua, "List", values)?])
        }
        "set" => Err(PeekError::Thrown("Cannot set unknown property.".into())),
        operation => {
            let value = call.argument(0);
            let Value::UserData(data) = &value else {
                return Err(mismatch("LogMessage", &value));
            };
            let message = data.borrow::<LuaLogMessage>()?;
            match operation {
                "get" => Ok(vec![push::text(lua, &message.0.text())?]),
                "tostring" => Ok(vec![push::text(
                    lua,
                    &format!("LogMessage: {:p}", data.to_pointer()),
                )?]),
                "__tojson" => {
                    let args = [Value::Table(fields(lua, &message.0)?)];
                    json::call(
                        &Call {
                            arguments: &args,
                            ..*call
                        },
                        "encode",
                    )
                }
                other => Err(crate::host::missing("pandoc.log", other)),
            }
        }
    }
}

fn fields(lua: &Lua, message: &LogMessage) -> mlua::Result<mlua::Table> {
    use LogMessage::*;
    let fields = lua.create_table()?;
    let debug = format!("{message:?}");
    let kind = debug
        .split_once('(')
        .map_or(debug.as_str(), |(name, _)| name);
    fields.raw_set(
        "type",
        if kind == "NotUtf8Encoded" {
            "NotUTF8Encoded"
        } else {
            kind
        },
    )?;
    fields.raw_set("verbosity", message.level().label())?;
    fields.raw_set("pretty", message.text())?;
    match message {
        ScriptingInfo(text, position) | ScriptingWarning(text, position) => {
            fields.raw_set("message", text.as_str())?;
            if let Some(position) = position {
                fields.raw_set("source", position.source.as_str())?;
                fields.raw_set("line", position.line)?;
                fields.raw_set("column", position.column)?;
            }
        }
        CouldNotFetchResource(path, message) => {
            fields.raw_set("path", path.as_str())?;
            fields.raw_set("message", message.as_str())?;
        }
        Extracting(path) => fields.raw_set("path", path.as_str())?,
        Fetching(url) => fields.raw_set("path", url.as_str())?,
        RunningFilter(filter) => fields.raw_set("path", filter.as_str())?,
        FilterCompleted(filter, time) => {
            fields.raw_set("path", filter.as_str())?;
            fields.raw_set("milliseconds", time.to_string())?;
        }
        LoadedResource(original, found) => {
            fields.raw_set("for", original.as_str())?;
            fields.raw_set("from", found.as_str())?;
        }
        NotUtf8Encoded(source) => fields.raw_set("source", source.as_str())?,
        CouldNotDeduceFormat(extensions, format) => {
            fields.raw_set("extensions", crate::module::string_list(lua, extensions)?)?;
            fields.raw_set("format", format.as_str())?;
        }
        _ => {}
    }
    Ok(fields)
}
