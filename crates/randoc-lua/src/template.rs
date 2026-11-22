//! A part of `pandoc.template`: `compile` and `apply` (row I-LUA-27). A
//! custom writer gets the template of `--template` or of its `Template`
//! function as text in `opts.template`, and `apply` renders it.

use mlua::{Table, Value};
use randoc_templates::{Context as TemplateContext, Template, Val};

use crate::construct::Call;
use crate::peek::{Cx, LuaText, Peek, PeekError, depth_message, mismatch};
use crate::push;

/// Runs the function `name` of `pandoc.template` with the arguments of
/// `call`: `compile` checks a template text and returns it, `apply`
/// renders a template text with a context table and returns the text.
pub(crate) fn call(call: &Call, name: &str) -> Peek<Value> {
    let cx = call.cx;
    let source = call.required(0, "template", |v| cx.text(v))?;
    let template = compile(cx, &source)?;
    match name {
        "compile" => Ok(push::text(cx.lua, &source)?),
        "apply" => {
            let context = call.required(1, "context", |v| context(cx, v))?;
            Ok(push::text(cx.lua, &template.render(&context).render(None))?)
        }
        other => Err(PeekError::Thrown(format!(
            "internal error: unknown function pandoc.template.{other}"
        ))),
    }
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
    let mut context = TemplateContext::new();
    for pair in table.clone().pairs::<Value, Value>() {
        let (key, value) = pair?;
        if let Value::String(key) = key {
            context.insert(key.text(), template_value(cx, &value, 1)?);
        }
    }
    Ok(context)
}

/// A template value: a string or a number gives text, a boolean a flag, a
/// list a list, a table with string keys a map, and a `Doc` of `apply`
/// its text. `level` counts the tables around the value.
fn template_value(cx: &Cx, value: &Value, level: usize) -> Peek<Val> {
    if level > cx.max_depth {
        return Err(PeekError::Thrown(depth_message(cx.max_depth)));
    }
    Ok(match value {
        Value::Nil => Val::Null,
        Value::Boolean(flag) => Val::Bool(*flag),
        Value::String(_) | Value::Integer(_) | Value::Number(_) => Val::text(cx.text(value)?),
        Value::Table(table) => {
            if let Some(text) = doc_text(table)? {
                return Ok(Val::text(text));
            }
            let length = table.raw_len();
            if length > 0 {
                let mut items = Vec::with_capacity(length);
                for index in 1..=length {
                    let item: Value = table.raw_get(index)?;
                    items.push(template_value(cx, &item, level + 1)?);
                }
                Val::List(items)
            } else {
                let mut map = TemplateContext::new();
                for pair in table.clone().pairs::<Value, Value>() {
                    let (key, item) = pair?;
                    if let Value::String(key) = key {
                        map.insert(key.text(), template_value(cx, &item, level + 1)?);
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
