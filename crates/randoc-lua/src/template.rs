//! A part of `pandoc.template`: `compile` and `apply` (row I-LUA-27). A
//! custom writer gets the compiled `Template` userdata of `--template` or of
//! its `Template` global in `opts.template`, and `apply` renders it (keeping
//! any resolved partials intact).

use mlua::{MetaMethod, Table, UserData, UserDataMethods, Value};
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

/// A compiled template exposed to Lua as opaque `userdata`, matching Pandoc's
/// `pandoc Template` userdata (`type(opts.template) == "userdata"`).
#[derive(Clone)]
pub(crate) struct LuaTemplate(pub(crate) Template);

impl UserData for LuaTemplate {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Template: {:p}", &this.0))
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
        "compile" => {
            let source = call.required(0, "template", |v| cx.text(v))?;
            let template = compile(cx, &source)?;
            Ok(Value::UserData(
                cx.lua.create_userdata(LuaTemplate(template))?,
            ))
        }
        "apply" => {
            let template = call.required(0, "template", |v| extract_template(cx, v))?;
            let context = call.required(1, "context", |v| context(cx, v))?;
            Ok(push::text(cx.lua, &template.render(&context).render(None))?)
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
        return Ok(tpl.0.clone());
    }
    let source = cx.text(value)?;
    compile(cx, &source)
}

fn compile(cx: &Cx, source: &str) -> Peek<Template> {
    Template::compile("template", source, cx.max_depth, &mut |path| {
        Err(format!("Could not find template partial {path}"))
    })
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
