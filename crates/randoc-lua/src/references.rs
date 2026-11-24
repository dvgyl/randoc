//! Converts the cited references of a document to Lua values.

use std::collections::HashSet;

use mlua::{Lua, Value};
use randoc_ast::{Inline, Pandoc, walk::walk_inlines_mut};
use randoc_citeproc::{
    markup::Markup,
    output::markup_to_inlines,
    reference::{Date, Name, Reference, Val},
    types::Locale,
};

use crate::construct::Call;
use crate::host::Host;
use crate::peek::{Cx, Peek, PeekError, depth_message};
use crate::push;
use crate::walk::check_document;

pub(crate) fn call(host: &Host, call: &Call) -> Peek<Vec<Value>> {
    let cx = call.cx;
    let mut doc = call.required(0, "doc", |v| cx.pandoc(v))?;
    check_document(cx, &doc)?;
    let hook = host
        .environment
        .references
        .ok_or_else(|| PeekError::Thrown("pandoc.utils.references is not available".into()))?;
    let references = host.system(|context| {
        hook(context, &doc, host.environment).map_err(|error| PeekError::Thrown(error.to_string()))
    })?;
    let (cited, all) = cited_ids(&mut doc);
    let mut result = Vec::new();
    for reference in &references {
        if !reference.id.is_empty() && (all || cited.contains(&reference.id)) {
            result.push(push_reference(cx, reference)?);
        }
    }
    Ok(vec![push::list_of(cx.lua, "List", result)?])
}

fn cited_ids(doc: &mut Pandoc) -> (HashSet<String>, bool) {
    let mut cited = HashSet::new();
    let mut all = false;
    if let Some(mut nocite) = doc.meta.0.remove("nocite") {
        walk_inlines_mut(&mut nocite, |inline| {
            if let Inline::Cite(citations, _) = inline {
                for item in citations {
                    all |= item.id == "*";
                    cited.insert(item.id.clone());
                }
            }
        });
        doc.meta.insert("nocite", nocite);
    }
    walk_inlines_mut(doc, |inline| {
        if let Inline::Cite(citations, _) = inline {
            cited.extend(citations.iter().map(|c| c.id.clone()));
        }
    });
    (cited, all)
}

fn push_reference(cx: &Cx, reference: &Reference<Vec<Markup>>) -> Peek<Value> {
    let lua = cx.lua;
    let table = lua.create_table()?;
    table.raw_set("id", reference.id.as_str())?;
    table.raw_set("type", reference.kind.as_str())?;
    for (key, value) in &reference.variables {
        let value = match value {
            Val::Text(text) => push::text(lua, text)?,
            Val::Fancy(parts) => {
                let inlines = markup_to_inlines(&Locale::default(), parts, cx.max_depth)
                    .map_err(|_| PeekError::Thrown(depth_message(cx.max_depth)))?;
                push::inlines(lua, &inlines)?
            }
            Val::Num(number) => Value::Integer(*number),
            Val::Names(names) => push::list(lua, names, push_name)?,
            Val::Date(date) => push_date(lua, date)?,
        };
        table.raw_set(key.as_str(), value)?;
    }
    Ok(Value::Table(table))
}

fn push_name(lua: &Lua, name: &Name) -> mlua::Result<Value> {
    let table = lua.create_table()?;
    for (key, text) in [
        ("family", &name.family),
        ("given", &name.given),
        ("dropping-particle", &name.dropping_particle),
        ("non-dropping-particle", &name.non_dropping_particle),
        ("suffix", &name.suffix),
        ("literal", &name.literal),
    ] {
        if let Some(text) = text {
            table.raw_set(key, text.as_str())?;
        }
    }
    if name.comma_suffix {
        table.raw_set("comma-suffix", true)?;
    }
    if name.static_ordering {
        table.raw_set("static-ordering", true)?;
    }
    Ok(Value::Table(table))
}

fn push_date(lua: &Lua, date: &Date) -> mlua::Result<Value> {
    let table = lua.create_table()?;
    let parts = push::list(lua, &date.parts, |lua, part| {
        push::list(lua, &part.0, |_, n| Ok(Value::Integer(*n)))
    })?;
    table.raw_set("date-parts", parts)?;
    if date.circa {
        table.raw_set("circa", true)?;
    }
    if let Some(season) = date.season {
        table.raw_set("season", season)?;
    }
    if let Some(literal) = &date.literal {
        table.raw_set("literal", literal.as_str())?;
    }
    Ok(Value::Table(table))
}
