//! A part of `pandoc.template`: `compile` and `apply` (row I-LUA-27). A
//! custom writer gets the compiled `Template` userdata of `--template` or of
//! its `Template` global in `opts.template`, and `apply` renders it (keeping
//! any resolved partials intact).

use std::cell::Cell;
use std::rc::Rc;

use mlua::{Lua, MetaMethod, Table, UserData, UserDataMethods, Value};
use randoc_core::{Context, Error};
use randoc_templates::{Context as TemplateContext, Template, Val};

use crate::construct::Call;
use crate::peek::{Cx, LuaText, Peek, PeekError, depth_message, mismatch};
use crate::push;

/// Maximum total nodes visited when converting a Lua table to a
/// [`TemplateContext`] (prevents exponential DAG-to-tree blowup such as
/// `x = {x, x}`).
const MAX_CONTEXT_NODES: usize = 200_000;

/// Maximum cumulative string bytes allocated when converting a Lua table to a
/// [`TemplateContext`] (16 MiB).
const MAX_CONTEXT_BYTES: usize = 16 * 1024 * 1024;

const MAX_SOURCE_BYTES: usize = 1 << 20;
const MAX_LIVE_BYTES: usize = 64 << 20;
const MAX_RENDER_WORK_BYTES: usize = 64 << 20;
const MAX_OUTPUT_BYTES: usize = 16 << 20;

#[derive(Clone)]
struct LiveBytes {
    used: Rc<Cell<usize>>,
    limit: usize,
}

impl Default for LiveBytes {
    fn default() -> Self {
        Self {
            used: Rc::default(),
            limit: MAX_LIVE_BYTES,
        }
    }
}

/// A compiled template and its share of the Lua state's template budget.
pub(crate) struct LuaTemplate {
    template: Template,
    live: LiveBytes,
    bytes: usize,
}

impl Drop for LuaTemplate {
    fn drop(&mut self) {
        self.live.used.set(self.live.used.get() - self.bytes);
    }
}

pub(crate) fn userdata(lua: &Lua, template: Template) -> mlua::Result<Value> {
    let live = match lua.app_data_ref::<LiveBytes>() {
        Some(live) => LiveBytes::clone(&live),
        None => {
            let live = LiveBytes::default();
            lua.set_app_data(live.clone());
            live
        }
    };
    let bytes = template.estimated_size();
    let total = live.used.get().saturating_add(bytes);
    if total > live.limit {
        return Err(mlua::Error::RuntimeError(
            "compiled templates exceeded live size limit".into(),
        ));
    }
    live.used.set(total);
    Ok(Value::UserData(lua.create_userdata(LuaTemplate {
        template,
        live,
        bytes,
    })?))
}

impl UserData for LuaTemplate {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Template: {:p}", &this.template))
        });
    }
}

struct ContextBudget {
    nodes: usize,
    bytes: usize,
}

impl ContextBudget {
    fn new() -> Self {
        Self { nodes: 0, bytes: 0 }
    }

    fn charge(&mut self, nodes: usize, bytes: usize) -> Peek<()> {
        self.nodes = self.nodes.saturating_add(nodes);
        self.bytes = self.bytes.saturating_add(bytes);
        if self.nodes > MAX_CONTEXT_NODES || self.bytes > MAX_CONTEXT_BYTES {
            return Err(PeekError::Thrown(
                "template context exceeded size limit".to_owned(),
            ));
        }
        Ok(())
    }

    fn remaining_nodes(&self) -> usize {
        MAX_CONTEXT_NODES.saturating_sub(self.nodes)
    }
}

/// Runs the function `name` of `pandoc.template` with the arguments of
/// `call`: `compile` returns a compiled [`LuaTemplate`] userdata, `apply`
/// renders a compiled [`LuaTemplate`] userdata (or raw template text) with
/// a bounded context table and returns the rendered text.
pub(crate) fn call(call: &Call, name: &str) -> Peek<Value> {
    let cx = call.cx;
    match name {
        "apply" => {
            let template = call.required(0, "template", |v| extract_template(cx, v))?;
            let context = call.required(1, "context", |v| context(cx, v))?;
            let text = template
                .render_limited(&context, MAX_RENDER_WORK_BYTES, MAX_OUTPUT_BYTES)
                .ok_or_else(|| PeekError::Thrown("template render exceeded size limit".into()))?;
            Ok(push::text(cx.lua, &text)?)
        }
        other => Err(PeekError::Thrown(format!(
            "internal error: unknown function pandoc.template.{other}"
        ))),
    }
}

fn extract_template(cx: &Cx, value: &Value) -> Peek<Template> {
    if let Value::UserData(ud) = value
        && let Ok(tpl) = ud.borrow::<LuaTemplate>()
    {
        return Ok(tpl.template.clone());
    }
    let source = cx.text(value)?;
    compile(cx, &source, None, None)
}

pub(crate) fn compile_userdata(call: &Call, host: &mut Context) -> Peek<Value> {
    let source = call.required(0, "template", |v| call.cx.text(v))?;
    let path = call.optional(1, "path", |v| call.cx.text(v))?;
    let template = compile(call.cx, &source, path.as_deref(), Some(host))?;
    userdata(call.cx.lua, template).map_err(|error| match error {
        mlua::Error::RuntimeError(text) => PeekError::Thrown(text),
        other => other.into(),
    })
}

fn compile(
    cx: &Cx,
    source: &str,
    path: Option<&str>,
    mut context: Option<&mut Context>,
) -> Peek<Template> {
    let limit = context.as_ref().map_or(MAX_SOURCE_BYTES, |context| {
        usize::try_from(context.limits.max_resource_size)
            .unwrap_or(usize::MAX)
            .min(MAX_SOURCE_BYTES)
    });
    let limit_error = "template source exceeded size limit";
    let mut remaining = limit
        .checked_sub(source.len())
        .ok_or_else(|| PeekError::Thrown(limit_error.into()))?;
    Template::compile(
        path.unwrap_or("template"),
        source,
        cx.max_depth,
        &mut |partial| {
            let Some(context) = context.as_deref_mut() else {
                return Err(format!("Could not find template partial {partial}"));
            };
            let name = format!(
                "templates/{}",
                partial.rsplit('/').next().unwrap_or(partial)
            );
            let file = context
                .state
                .user_data_dir
                .as_ref()
                .map(|dir| format!("{dir}/{name}"));
            let from_path = if path.is_some() {
                match context.host().read_file_limited(partial, remaining) {
                    Ok(bytes) => Some(bytes),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => {
                        return Err(format!(
                            "Could not read template partial {partial}: {error}"
                        ));
                    }
                }
            } else {
                None
            };
            let bytes =
                match (from_path, file) {
                    (Some(bytes), _) => bytes,
                    (None, Some(file)) => context
                        .host()
                        .read_file_limited(&file, remaining)
                        .map_err(|error| {
                            if error.kind() == std::io::ErrorKind::NotFound {
                                Error::CouldNotFindDataFile(name.clone()).to_string()
                            } else {
                                format!("Could not read template partial {file}: {error}")
                            }
                        })?,
                    (None, None) => return Err(Error::CouldNotFindDataFile(name).to_string()),
                };
            remaining = remaining.checked_sub(bytes.len()).ok_or(limit_error)?;
            let text = String::from_utf8(bytes).map_err(|error| error.to_string())?;
            Ok(text.trim_start_matches('\u{feff}').replace('\r', ""))
        },
    )
    .map_err(|error| PeekError::Thrown(error.to_string()))
}

/// The template context of a table: each string key with its value.
fn context(cx: &Cx, value: &Value) -> Peek<TemplateContext> {
    let Value::Table(table) = value else {
        return Err(mismatch("table", value));
    };
    let mut budget = ContextBudget::new();
    let mut context = TemplateContext::new();
    for pair in table.clone().pairs::<Value, Value>() {
        let (key, value) = pair?;
        if let Value::String(key) = key {
            let key_text = key.text();
            budget.charge(1, key_text.len())?;
            context.insert(key_text, template_value(cx, &value, 1, &mut budget)?);
        }
    }
    Ok(context)
}

/// A template value: a string or a number gives text, a boolean a flag, a
/// list a list, a table with string keys a map, and a `Doc` of `apply`
/// its text. `level` counts the tables around the value.
fn template_value(cx: &Cx, value: &Value, level: usize, budget: &mut ContextBudget) -> Peek<Val> {
    if level > cx.max_depth {
        return Err(PeekError::Thrown(depth_message(cx.max_depth)));
    }
    budget.charge(1, 0)?;
    Ok(match value {
        Value::Nil => Val::Null,
        Value::Boolean(flag) => Val::Bool(*flag),
        Value::String(_) | Value::Integer(_) | Value::Number(_) => {
            let text = cx.text(value)?;
            budget.charge(0, text.len())?;
            Val::text(text)
        }
        Value::Table(table) => {
            if let Some(text) = doc_text(table)? {
                budget.charge(0, text.len())?;
                return Ok(Val::text(text));
            }
            let length = table.raw_len();
            if length > 0 {
                let mut items = Vec::with_capacity(length.min(budget.remaining_nodes()));
                for index in 1..=length {
                    let item: Value = table.raw_get(index)?;
                    items.push(template_value(cx, &item, level + 1, budget)?);
                }
                Val::List(items)
            } else {
                let mut map = TemplateContext::new();
                for pair in table.clone().pairs::<Value, Value>() {
                    let (key, item) = pair?;
                    if let Value::String(key) = key {
                        let key_text = key.text();
                        budget.charge(1, key_text.len())?;
                        map.insert(key_text, template_value(cx, &item, level + 1, budget)?);
                    }
                }
                Val::Map(map)
            }
        }
        other => return Err(mismatch("string, boolean or table", other)),
    })
}

/// The text of a `Doc` value of `pandoc.template.apply`.
fn doc_text(table: &Table) -> Peek<Option<String>> {
    let Some(metatable) = table.metatable() else {
        return Ok(None);
    };
    if metatable.raw_get::<Option<String>>("__name")?.as_deref() != Some("Doc") {
        return Ok(None);
    }
    Ok(table.raw_get::<Option<String>>("text")?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_templates_share_a_live_budget_and_gc_releases_it() {
        let lua = Lua::new();
        let tpl = Template::compile("test", "a test", 16, &mut |_| {
            Err::<String, _>("no partials")
        })
        .unwrap();
        let limit = tpl.estimated_size() * 2;
        let budget = LiveBytes {
            used: Rc::default(),
            limit,
        };
        lua.set_app_data(budget.clone());
        let first = userdata(&lua, tpl.clone()).unwrap();
        let second = userdata(&lua, tpl.clone()).unwrap();
        assert_eq!(budget.used.get(), limit);
        assert!(userdata(&lua, tpl.clone()).is_err());

        drop(first);
        lua.gc_collect().unwrap();
        assert_eq!(budget.used.get(), limit / 2);
        assert!(userdata(&lua, tpl).is_ok());
        drop(second);
        lua.gc_collect().unwrap();
        assert_eq!(budget.used.get(), 0);
    }
}
