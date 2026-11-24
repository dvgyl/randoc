//! Temporary overrides for the shared conversion state.

use mlua::{FromLua, Function, MultiValue, Table, Value};
use randoc_core::Text;

use crate::construct::Call;
use crate::host::Host;
use crate::peek::{Peek, PeekError, mismatch};

#[derive(Default)]
struct Overrides {
    paths: Option<Vec<Text>>,
    directory: Option<Option<Text>>,
    headers: Option<Vec<(Text, Text)>>,
}

impl Overrides {
    fn swap(&mut self, state: &mut randoc_core::CommonState) {
        if let Some(paths) = &mut self.paths {
            std::mem::swap(paths, &mut state.resource_path);
        }
        if let Some(directory) = &mut self.directory {
            std::mem::swap(directory, &mut state.user_data_dir);
        }
        if let Some(headers) = &mut self.headers {
            std::mem::swap(headers, &mut state.request_headers);
        }
    }
}

pub(crate) fn call(host: &Host, call: &Call) -> Peek<Vec<Value>> {
    let cx = call.cx;
    const FIELDS: &str = "randoc.state.fields";
    let previous: Option<Table> = cx.lua.named_registry_value(FIELDS)?;
    if call.name == "get" {
        return previous.map_or_else(
            || Ok(vec![Value::Nil]),
            |fields| Ok(vec![fields.get::<Value>(call.argument(0))?]),
        );
    }

    let mut overrides = call.required(0, "options", |value| {
        let Value::Table(table) = value else {
            return Err(mismatch("table", value));
        };
        let mut options = Overrides::default();
        for pair in table.pairs::<Value, Value>() {
            let (key, value) = pair?;
            match cx.text(&key)?.as_str() {
                "resource_path" => options.paths = Some(cx.list(&value, |v| cx.text(v))?),
                "user_data_dir" => options.directory = Some(Some(cx.text(&value)?)),
                "request_headers" => {
                    options.headers = Some(cx.list(&value, |item| {
                        let Value::Table(item) = item else {
                            return Err(mismatch("table", item));
                        };
                        Ok((cx.text(&item.raw_get(1)?)?, cx.text(&item.raw_get(2)?)?))
                    })?);
                }
                key => {
                    return Err(PeekError::failure(format!(
                        "Unknown or unsupported state option: {key}"
                    )));
                }
            }
        }
        Ok(options)
    })?;
    let fields = cx.lua.create_table()?;
    if let Some(paths) = &overrides.paths {
        fields.raw_set("resource_path", crate::module::string_list(cx.lua, paths)?)?;
    }
    if let Some(Some(directory)) = &overrides.directory {
        fields.raw_set("user_data_dir", directory.as_str())?;
    }
    if let Some(headers) = &overrides.headers {
        let rows = headers
            .iter()
            .map(|(name, value)| cx.lua.create_sequence_from([name.as_str(), value.as_str()]))
            .collect::<mlua::Result<Vec<_>>>()?;
        fields.raw_set(
            "request_headers",
            crate::push::list(cx.lua, &rows, |_, row| Ok(Value::Table(row.clone())))?,
        )?;
    }
    if let Some(previous) = previous.as_ref() {
        let metatable = cx.lua.create_table()?;
        metatable.raw_set("__index", previous.clone())?;
        fields.set_metatable(Some(metatable))?;
    }
    let protected: Function = Function::from_lua(call.argument(1), cx.lua)?;
    cx.lua.set_named_registry_value(FIELDS, fields)?;
    host.context(|context| {
        overrides.swap(&mut context.state);
        Ok(())
    })?;
    let result = protected.call::<MultiValue>(call.argument(2));
    let restore_context = host.context(|context| {
        overrides.swap(&mut context.state);
        Ok(())
    });
    let restore_registry = cx.lua.set_named_registry_value(FIELDS, previous);
    restore_context?;
    restore_registry?;
    Ok(result?.into_iter().collect())
}
