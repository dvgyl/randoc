//! Connects Lua log reads and holds to the conversion context.

use mlua::{UserData, UserDataFields, Value};
use randoc_core::LogMessage;

use crate::construct::Call;
use crate::host::{self, Host};
use crate::object::typed_fields;
use crate::peek::{Peek, mismatch};
use crate::push;

struct LuaLogMessage(LogMessage);

impl UserData for LuaLogMessage {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        typed_fields(fields, "LogMessage", &["__tojson"]);
    }
}

pub(crate) fn call(host: &Host, call: &Call) -> Peek<Vec<Value>> {
    let lua = host.cx.lua;
    let messages = match call.name {
        "hold" => {
            return host.context(|context| {
                context.hold_messages();
                Ok(Vec::new())
            });
        }
        "release" => {
            let mut messages = host.context(|context| Ok(context.release_messages()))?;
            messages.reverse();
            messages
        }
        "list" => host.context(|context| Ok(context.log().to_vec()))?,
        "get" => {
            let Some(Value::UserData(value)) = call.arguments.first() else {
                return Err(mismatch("LogMessage", &call.argument(0)));
            };
            let message = value.borrow::<LuaLogMessage>()?;
            return Ok(vec![push::text(lua, &message.0.text())?]);
        }
        "set" => {
            return Err(crate::peek::PeekError::Thrown(
                "Cannot set unknown property.".into(),
            ));
        }
        name => return Err(host::missing("pandoc.log", name)),
    };
    let list = push::list(lua, &messages, |lua, message| {
        Ok(Value::UserData(
            lua.create_userdata(LuaLogMessage(message.clone()))?,
        ))
    })?;
    Ok(vec![list])
}
