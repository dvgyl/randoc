//! Template lookup, compilation, and rendering for Lua.

use std::cell::Cell;
use std::rc::Rc;

use mlua::{Lua, MetaMethod, Table, UserData, UserDataMethods, Value};
use randoc_ast::{Inline, MetaValue};
use randoc_core::{Context, Error};
use randoc_templates::{Context as TemplateContext, Template, Val};

use crate::construct::Call;
use crate::layout::userdata as doc_userdata;
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
        self.live
            .used
            .set(self.live.used.get().saturating_sub(self.bytes));
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
/// a bounded context table and returns the rendered document.
pub(crate) fn call(call: &Call, name: &str) -> Peek<Value> {
    let cx = call.cx;
    match name {
        "apply" => {
            let template = call.required(0, "template", |v| extract_template(cx, v))?;
            let context = call.required(1, "context", |v| context(cx, v))?;
            let text = template
                .render_limited(&context, MAX_RENDER_WORK_BYTES, MAX_OUTPUT_BYTES)
                .ok_or_else(|| PeekError::Thrown("template render exceeded size limit".into()))?;
            doc_userdata(cx.lua, randoc_layout::literal(text))
        }
        "meta_to_context" => {
            let meta = call.required(0, "meta", |value| cx.meta(value))?;
            let render_blocks = call.argument(1);
            let render_inlines = call.argument(2);
            let table = context_table(cx.lua)?;
            for (key, value) in meta.0 {
                table.raw_set(key, meta_value(cx, value, &render_blocks, &render_inlines)?)?;
            }
            Ok(Value::Table(table))
        }
        other => Err(PeekError::Thrown(format!(
            "internal error: unknown function pandoc.template.{other}"
        ))),
    }
}

pub(crate) fn extract_template(cx: &Cx, value: &Value) -> Peek<Template> {
    if let Value::UserData(ud) = value
        && let Ok(tpl) = ud.borrow::<LuaTemplate>()
    {
        return Ok(tpl.template.clone());
    }
    let source = cx.text(value)?;
    compile(cx, &source, None, None)
}

pub(crate) fn file(call: &Call, host: &mut Context) -> Peek<Value> {
    let cx = call.cx;
    let name = call.required(
        0,
        if call.name == "default" {
            "format"
        } else {
            "filename"
        },
        |v| cx.text(v),
    )?;
    let limit = usize::try_from(host.limits.max_resource_size).unwrap_or(usize::MAX);
    let data_name = if call.name == "default" {
        if matches!(
            name.as_str(),
            "csljson" | "fb2" | "ipynb" | "json" | "native" | "pptx" | "xml"
        ) {
            return Ok(push::text(cx.lua, "")?);
        }
        format!("default.{}", template_format(&name))
    } else {
        if let Some(text) = read_text(host, &name, limit)? {
            return Ok(push::text(cx.lua, &text)?);
        }
        name.rsplit('/').next().unwrap_or(&name).to_owned()
    };
    Ok(push::text(
        cx.lua,
        &data_template(host, &data_name, limit)?,
    )?)
}

fn template_format(format: &str) -> &str {
    match format {
        "asciidoc_legacy" | "asciidoctor" => "asciidoc",
        "bbcode_fluxbb" | "bbcode_hubzilla" | "bbcode_phpbb" | "bbcode_steam"
        | "bbcode_xenforo" => "bbcode",
        "commonmark_x" | "gfm" => "commonmark",
        "docbook" => "docbook5",
        "docx" => "openxml",
        "epub" => "epub3",
        "fodt" | "odt" => "opendocument",
        "html" => "html5",
        "jats" => "jats_archiving",
        "markdown_github" | "markdown_mmd" | "markdown_phpextra" | "markdown_strict" => "markdown",
        _ => format,
    }
}

fn read_text(host: &mut Context, path: &str, limit: usize) -> Peek<Option<String>> {
    match host.host().read_file_limited(path, limit) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(|text| Some(text.trim_start_matches('\u{feff}').replace('\r', "")))
            .map_err(|error| PeekError::Thrown(error.to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(PeekError::Thrown(format!(
            "Could not read template {path}: {error}"
        ))),
    }
}

fn data_template(host: &mut Context, name: &str, limit: usize) -> Peek<String> {
    let path = format!("templates/{name}");
    if let Some(directory) = host.state.user_data_dir.as_ref() {
        let file = format!("{directory}/{path}");
        if let Some(text) = read_text(host, &file, limit)? {
            return Ok(text);
        }
    }
    let text = builtin_template(name)
        .ok_or_else(|| PeekError::Thrown(Error::CouldNotFindDataFile(path).to_string()))?;
    if text.len() > limit {
        return Err(PeekError::Thrown(
            "template source exceeded size limit".into(),
        ));
    }
    Ok(text.to_owned())
}

// Use the workspace's BSD-3-Clause templates without a second copy.
fn builtin_template(name: &str) -> Option<&'static str> {
    macro_rules! templates {
        ($($name:literal),* $(,)?) => {
            match name {
                $($name => Some(include_str!(concat!("../../randoc-data/data/templates/", $name))),)*
                _ => None,
            }
        };
    }
    templates! {
        "affiliations.jats",
        "after-header-includes.latex",
        "article.jats_publishing",
        "common.latex",
        "default.ansi",
        "default.asciidoc",
        "default.bbcode",
        "default.beamer",
        "default.biblatex",
        "default.bibtex",
        "default.chunkedhtml",
        "default.commonmark",
        "default.context",
        "default.djot",
        "default.docbook4",
        "default.docbook5",
        "default.dokuwiki",
        "default.dzslides",
        "default.epub2",
        "default.epub3",
        "default.haddock",
        "default.html4",
        "default.html5",
        "default.icml",
        "default.jats_archiving",
        "default.jats_articleauthoring",
        "default.jats_publishing",
        "default.jira",
        "default.latex",
        "default.man",
        "default.markdown",
        "default.markua",
        "default.mediawiki",
        "default.ms",
        "default.muse",
        "default.opendocument",
        "default.openxml",
        "default.opml",
        "default.org",
        "default.plain",
        "default.revealjs",
        "default.rst",
        "default.rtf",
        "default.s5",
        "default.slideous",
        "default.slidy",
        "default.t2t",
        "default.tei",
        "default.texinfo",
        "default.textile",
        "default.typst",
        "default.vimdoc",
        "default.xwiki",
        "default.zimwiki",
        "document-metadata.latex",
        "font-settings.latex",
        "fonts.latex",
        "hypersetup.latex",
        "passoptions.latex",
        "styles.citations.html",
        "styles.html",
        "template.typst",
    }
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
            let name = partial.rsplit('/').next().unwrap_or(partial);
            let local = if path.is_some() {
                read_text(context, partial, remaining).map_err(|error| error.text())?
            } else {
                None
            };
            let text = match local {
                Some(text) => text,
                None => data_template(context, name, remaining).map_err(|error| error.text())?,
            };
            remaining = remaining.checked_sub(text.len()).ok_or(limit_error)?;
            Ok(text)
        },
    )
    .map_err(|error| PeekError::Thrown(error.to_string()))
}

/// The template context of a table: each string key with its value.
pub(crate) fn context(cx: &Cx, value: &Value) -> Peek<TemplateContext> {
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
        other => {
            let doc = crate::layout::read(cx, other)?;
            budget.charge(0, doc.estimated_size())?;
            Val::Simple(doc)
        }
    })
}

pub(crate) fn context_table(lua: &Lua) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let metatable = lua.create_table()?;
    metatable.raw_set("__name", "pandoc Context")?;
    table.set_metatable(Some(metatable))?;
    Ok(table)
}

fn meta_value(cx: &Cx, value: MetaValue, blocks: &Value, inlines: &Value) -> Peek<Value> {
    let render = |callback: &Value, value: Value| -> Peek<Value> {
        let Value::Function(callback) = callback else {
            return Err(PeekError::Thrown(format!(
                "attempt to call a {} value",
                crate::peek::lua_type_name(callback)
            )));
        };
        let rendered = callback.call::<Value>(value)?;
        doc_userdata(cx.lua, crate::layout::read(cx, &rendered)?)
    };
    match value {
        MetaValue::MetaBool(flag) => Ok(Value::Boolean(flag)),
        MetaValue::MetaString(text) => {
            render(inlines, push::inlines(cx.lua, &[Inline::Str(text)])?)
        }
        MetaValue::MetaInlines(items) => render(inlines, push::inlines(cx.lua, &items)?),
        MetaValue::MetaBlocks(items) => render(blocks, push::blocks(cx.lua, &items)?),
        MetaValue::MetaList(items) => {
            let table = cx.lua.create_table_with_capacity(items.len(), 0)?;
            for (index, item) in items.into_iter().enumerate() {
                table.raw_set(index + 1, meta_value(cx, item, blocks, inlines)?)?;
            }
            Ok(Value::Table(table))
        }
        MetaValue::MetaMap(items) => {
            let table = context_table(cx.lua)?;
            for (key, item) in items {
                table.raw_set(key, meta_value(cx, item, blocks, inlines)?)?;
            }
            Ok(Value::Table(table))
        }
    }
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
