//! The `pandoc` module: the Rust functions that the Lua prelude calls.

use std::cell::Cell;

use mlua::{Function, Lua, MultiValue, Table, Value, Variadic};
use randoc_ast::{Attr, Block, Inline, Text};
use randoc_core::shared::{stringify, stringify_blocks, stringify_meta};

use crate::construct::{CONSTRUCTORS, Call, construct, version};
use crate::object::{
    METAMETHODS, Node, TYPE_SPECS, cache_of, ensure_cache, new_object, set_node, with_node,
};
use crate::peek::{Cx, Peek, PeekError, is_none_or_nil, mismatch, none, retrieving};
use crate::properties::get_property;
use crate::push::{self, METATABLE_PREFIX};
use crate::runtime;
use crate::show::{show_blocks, show_inlines, show_node};
use crate::utils;
use crate::walk::{CALL_TRACED, Filter, Subject, apply_fully, check_node, walk_blocks_and_inlines};
use crate::{formats, json, path, text};

/// The Lua part of the module.
const PRELUDE: &str = include_str!("prelude.lua");

/// The chunk name of the prelude. A traceback leaves out the frames of
/// this chunk.
const PRELUDE_NAME: &str = "[randoc]";

/// The registry name of the table of Rust functions of the prelude.
pub(crate) const NATIVE: &str = "randoc.native";

/// The string constants of the module, for example `pandoc.AlignLeft`.
const CONSTANTS: [&str; 22] = [
    "AlignCenter",
    "AlignDefault",
    "AlignLeft",
    "AlignRight",
    "AuthorInText",
    "Decimal",
    "DefaultDelim",
    "DefaultStyle",
    "DisplayMath",
    "DoubleQuote",
    "Example",
    "InlineMath",
    "LowerAlpha",
    "LowerRoman",
    "NormalCitation",
    "OneParen",
    "Period",
    "SingleQuote",
    "SuppressAuthor",
    "TwoParens",
    "UpperAlpha",
    "UpperRoman",
];

/// The largest number of calls of `walk` that are active at one time. Each
/// active call uses the stack of the program, and Lua stops only at 200
/// nested calls of C functions.
const MAX_ACTIVE_WALKS: usize = 20;

/// The settings of one Lua state.
struct Settings {
    max_depth: usize,
    active_walks: Cell<usize>,
}

fn cx(lua: &Lua) -> Cx<'_> {
    let max_depth = lua
        .app_data_ref::<Settings>()
        .map_or(128, |settings| settings.max_depth);
    Cx { lua, max_depth }
}

/// Makes the `pandoc` module and its globals in a Lua state.
///
/// `max_depth` is the largest nesting depth of a document value.
pub(crate) fn install(lua: &Lua, max_depth: usize) -> mlua::Result<()> {
    lua.set_app_data(Settings {
        max_depth,
        active_walks: Cell::new(0),
    });
    lua.set_app_data(crate::peek::ActiveObjects::default());
    let native = lua.create_table()?;
    native.set("metatables", lua.create_table()?)?;
    native.set("constructors", lua.create_sequence_from(CONSTRUCTORS)?)?;
    native.set("constants", lua.create_sequence_from(CONSTANTS)?)?;
    native.set("types", type_tables(lua)?)?;
    native.set(
        "typename",
        lua.create_function(|_, value: Value| Ok(with_node(&value, Node::type_name)))?,
    )?;
    native.set(
        "cache",
        lua.create_function(|lua, object: mlua::AnyUserData| ensure_cache(lua, &object))?,
    )?;
    native.set("traceback", lua.create_function(traceback)?)?;
    native.set("json_null", json::null())?;
    native.set("os", system_os())?;
    native.set("arch", system_arch())?;
    lua.set_named_registry_value(NATIVE, native.clone())?;
    native.set(
        "call",
        lua.create_function(|lua, arguments: Variadic<Value>| {
            let cx = cx(lua);
            let mut results = Vec::new();
            match dispatch(&cx, &arguments) {
                Ok(values) => {
                    results.push(Value::Boolean(true));
                    results.extend(values);
                }
                Err(error) => {
                    results.push(Value::Boolean(false));
                    results.push(Value::String(lua.create_string(error.text())?));
                }
            }
            Ok(MultiValue::from_vec(results))
        })?,
    )?;
    lua.load(PRELUDE)
        .set_name(format!("={PRELUDE_NAME}"))
        .call::<()>(native.clone())?;
    let metatables: Table = native.get("metatables")?;
    for pair in metatables.pairs::<String, Table>() {
        let (name, metatable) = pair?;
        lua.set_named_registry_value(&format!("{METATABLE_PREFIX}{name}"), metatable)?;
    }
    lua.set_named_registry_value(METAMETHODS, native.get::<Table>("metamethods")?)?;
    lua.set_named_registry_value(CALL_TRACED, native.get::<Function>("call_traced")?)?;
    lua.globals().set(
        "PANDOC_VERSION",
        new_object(lua, Node::Version(vec![3, 13]))?,
    )?;
    lua.globals().set(
        "PANDOC_API_VERSION",
        new_object(lua, Node::Version(randoc_ast::API_VERSION.to_vec()))?,
    )?;
    Ok(())
}

/// Makes the table that tells the prelude the members of each object type.
fn type_tables(lua: &Lua) -> mlua::Result<Table> {
    let types = lua.create_table()?;
    for spec in &TYPE_SPECS {
        let table = lua.create_table()?;
        let getters = lua.create_table()?;
        let setters = lua.create_table()?;
        let readonly = lua.create_table()?;
        for name in spec.properties {
            getters.set(*name, true)?;
            setters.set(*name, true)?;
        }
        for name in spec.readonly {
            getters.set(*name, true)?;
            readonly.set(*name, true)?;
        }
        let aliases = lua.create_table()?;
        for (name, path) in spec.aliases {
            aliases.set(*name, lua.create_sequence_from(path.iter().copied())?)?;
        }
        table.set("getters", getters)?;
        table.set("setters", setters)?;
        table.set("readonly", readonly)?;
        table.set("aliases", aliases)?;
        table.set(
            "method_names",
            lua.create_sequence_from(spec.methods.iter().copied())?,
        )?;
        types.set(spec.name, table)?;
    }
    Ok(types)
}

/// The message handler of a filter function: the message of the error and
/// the traceback of Lua, from the caller of the function that raised the
/// error, without the frames of the prelude.
fn traceback(lua: &Lua, error: Value) -> mlua::Result<String> {
    let message = match &error {
        Value::String(text) => text.to_string_lossy(),
        Value::Error(error) => runtime::message(error),
        other => other.to_string()?,
    };
    let full = lua.traceback(Some(&message), 2)?.to_string_lossy();
    let Some(frames) = full.strip_prefix(message.as_str()) else {
        return Ok(full);
    };
    let prelude_frame = format!("\t{PRELUDE_NAME}:");
    let lines: Vec<&str> = frames.split('\n').collect();
    let is_prelude = |line: &str| line.starts_with(&prelude_frame);
    let mut kept = Vec::new();
    let mut after_prelude = false;
    for (index, line) in lines.iter().enumerate() {
        let next_is_prelude = lines.get(index + 1).is_some_and(|next| is_prelude(next));
        let drop = is_prelude(line)
            || (after_prelude && *line == "\t(...tail calls...)")
            || (line.starts_with("\t[C]: ") && next_is_prelude);
        after_prelude = is_prelude(line);
        if !drop {
            kept.push(*line);
        }
    }
    Ok(format!("{message}{}", kept.join("\n")))
}

fn argument(arguments: &[Value], index: usize) -> Value {
    arguments.get(index).cloned().unwrap_or_else(none)
}

fn name_argument(arguments: &[Value], index: usize) -> Peek<String> {
    match arguments.get(index) {
        Some(Value::String(name)) => Ok(name.to_string_lossy()),
        _ => Err(PeekError::Thrown("internal error: no name".to_string())),
    }
}

/// Runs one operation of the prelude. The first argument is its name.
fn dispatch(cx: &Cx, arguments: &[Value]) -> Peek<Vec<Value>> {
    let operation = name_argument(arguments, 0)?;
    let rest = arguments.get(1..).unwrap_or_default();
    let lua = cx.lua;
    let one = |value: Value| Ok(vec![value]);
    match operation.as_str() {
        "construct" => {
            let name = name_argument(rest, 0)?;
            let call = Call {
                cx,
                name: &name,
                arguments: rest.get(1..).unwrap_or_default(),
            };
            let value = construct(&call)?;
            if let Some(result) = with_node(&value, |node| check_node(cx, node)) {
                result?;
            }
            one(value)
        }
        "get" => {
            let object = argument(rest, 0);
            let key = name_argument(rest, 1)?;
            let cached = match &object {
                Value::UserData(userdata) => cache_of(userdata).is_some_and(|t| !t.is_empty()),
                _ => false,
            };
            // The getter of Pandoc reads the object with its cached
            // properties first, so a bad cached value gives its error here.
            let value = if cached {
                let type_name = with_node(&object, Node::type_name).unwrap_or("");
                let node = cx.object(type_name, &object).map_err(PeekError::thrown)?;
                get_property(lua, &node, &key)?
            } else {
                with_node(&object, |node| get_property(lua, node, &key))
                    .transpose()?
                    .unwrap_or(Value::Nil)
            };
            one(value)
        }
        "method" => {
            let type_name = name_argument(rest, 0)?;
            let name = name_argument(rest, 1)?;
            method(cx, &type_name, &name, rest.get(2..).unwrap_or_default()).and_then(one)
        }
        "walk" => {
            let type_name = name_argument(rest, 0)?;
            method(cx, &type_name, "walk", rest.get(1..).unwrap_or_default()).and_then(one)
        }
        "eq" => {
            let (first, second) = (argument(rest, 0), argument(rest, 1));
            let type_name = with_node(&first, Node::type_name)
                .or_else(|| with_node(&second, Node::type_name))
                .unwrap_or("");
            let read = |value: &Value| match cx.object(type_name, value) {
                Ok(node) => Ok(Some(node)),
                Err(PeekError::Failure { .. }) => Ok(None),
                Err(thrown) => Err(thrown),
            };
            let (first, second) = (read(&first)?, read(&second)?);
            one(Value::Boolean(first.is_some() && first == second))
        }
        "tostring" => {
            let value = argument(rest, 0);
            let type_name = with_node(&value, Node::type_name).unwrap_or("");
            let node = cx.object(type_name, &value).map_err(PeekError::thrown)?;
            let text = show_node(&node, cx.max_depth).unwrap_or_default();
            one(push::text(lua, &text)?)
        }
        "concat" => {
            let call = Call {
                cx,
                name: "",
                arguments: rest,
            };
            let mut first = call.required(0, "a", |v| cx.pandoc(v))?;
            let second = call.required(1, "b", |v| cx.pandoc(v))?;
            first.meta.0.extend(second.meta.0);
            first.blocks.extend(second.blocks);
            one(push::pandoc(lua, &first)?)
        }
        "attributes" => attributes(cx, rest),
        "version" => versions(cx, rest),
        "list_walk" | "list_clone" | "list_show" => {
            let kind = name_argument(rest, 0)?;
            let call = Call {
                cx,
                name: "",
                arguments: rest.get(1..).unwrap_or_default(),
            };
            let node = if kind == "Inlines" {
                ListNode::Inlines(call.required(0, "self", |v| cx.inlines_fuzzy(v))?)
            } else {
                ListNode::Blocks(call.required(0, "self", |v| cx.blocks_fuzzy(v))?)
            };
            match (operation.as_str(), node) {
                ("list_show", ListNode::Inlines(items)) => {
                    one(push::text(lua, &show_inlines(&items, cx.max_depth))?)
                }
                ("list_show", ListNode::Blocks(items)) => {
                    one(push::text(lua, &show_blocks(&items, cx.max_depth))?)
                }
                ("list_clone", ListNode::Inlines(items)) => one(push::inlines(lua, &items)?),
                ("list_clone", ListNode::Blocks(items)) => one(push::blocks(lua, &items)?),
                (_, ListNode::Inlines(mut items)) => {
                    let filter = Filter::peek(&call.argument(1))?;
                    walking(cx, || {
                        walk_blocks_and_inlines(cx, &filter, Subject::Inlines(&mut items))
                    })?;
                    one(push::inlines(lua, &items)?)
                }
                (_, ListNode::Blocks(mut items)) => {
                    let filter = Filter::peek(&call.argument(1))?;
                    walking(cx, || {
                        walk_blocks_and_inlines(cx, &filter, Subject::Blocks(&mut items))
                    })?;
                    one(push::blocks(lua, &items)?)
                }
            }
        }
        "stringify" => one(push::text(lua, &stringify_value(cx, &argument(rest, 0))?)?),
        "utils" => {
            let name = name_argument(rest, 0)?;
            let call = Call {
                cx,
                name: name.strip_prefix("structure_").unwrap_or(&name),
                arguments: rest.get(1..).unwrap_or_default(),
            };
            utils::call(&call, &name).and_then(one)
        }
        "path" | "text" | "json" | "format" => {
            let name = name_argument(rest, 0)?;
            let call = Call {
                cx,
                name: &name,
                arguments: rest.get(1..).unwrap_or_default(),
            };
            match operation.as_str() {
                "path" => path::call(&call, &name),
                "text" => text::call(&call, &name),
                "format" => formats::call(&call, &name),
                _ => json::call(&call, &name),
            }
        }
        other => Err(PeekError::Thrown(format!(
            "internal error: unknown operation {other}"
        ))),
    }
}

enum ListNode {
    Inlines(Vec<Inline>),
    Blocks(Vec<Block>),
}

/// Runs `walk` and counts the calls of `walk` that are active.
fn walking(cx: &Cx, walk: impl FnOnce() -> Peek<()>) -> Peek<()> {
    let active = |change: fn(usize) -> usize| {
        cx.lua.app_data_ref::<Settings>().map(|settings| {
            let count = change(settings.active_walks.get());
            settings.active_walks.set(count);
            count
        })
    };
    if active(|count| count + 1).is_some_and(|count| count > MAX_ACTIVE_WALKS) {
        active(|count| count.saturating_sub(1));
        return Err(PeekError::Thrown(format!(
            "more than {MAX_ACTIVE_WALKS} calls of walk are active"
        )));
    }
    let result = walk();
    active(|count| count.saturating_sub(1));
    result
}

/// Returns the name of the parameter `self` of a method, which is part of
/// the error text.
fn self_parameter(type_name: &str, method: &str) -> &'static str {
    match (type_name, method) {
        ("Block", "clone") => "block",
        ("Inline", "clone") => "Inline",
        ("Attr", "clone") => "Attr",
        ("Pandoc", "clone") => "doc",
        ("Citation", "clone") => "obj",
        ("ListAttributes", "clone") => "a",
        ("Caption", "clone") => "capt",
        _ => "self",
    }
}

/// Runs a method of an object type.
fn method(cx: &Cx, type_name: &str, name: &str, arguments: &[Value]) -> Peek<Value> {
    let lua = cx.lua;
    let call = Call {
        cx,
        name,
        arguments,
    };
    let parameter = self_parameter(type_name, name);
    let mut node = call.required(0, parameter, |value| {
        let node = cx.object(type_name, value);
        if type_name == "Pandoc" {
            retrieving("Pandoc", node)
        } else {
            node
        }
    })?;
    match name {
        "clone" => {}
        "show" => {
            let text = show_node(&node, cx.max_depth).unwrap_or_default();
            return Ok(push::text(lua, &text)?);
        }
        "normalize" => {
            if let Node::Pandoc(doc) = &mut node {
                crate::normalize::normalize(doc);
            }
        }
        "walk" => {
            let filter = Filter::peek(&call.argument(1))?;
            walking(cx, || match &mut node {
                Node::Pandoc(doc) => apply_fully(cx, &filter, doc),
                Node::Inline(inline) => {
                    walk_blocks_and_inlines(cx, &filter, Subject::Inline(inline))
                }
                Node::Block(block) => walk_blocks_and_inlines(cx, &filter, Subject::Block(block)),
                Node::Cell(cell) => walk_blocks_and_inlines(cx, &filter, Subject::Cell(cell)),
                Node::Row(row) => walk_blocks_and_inlines(cx, &filter, Subject::Row(row)),
                _ => Ok(()),
            })?;
        }
        other => {
            return Err(PeekError::Thrown(format!(
                "internal error: unknown method {other}"
            )));
        }
    }
    Ok(new_object(lua, node)?)
}

/// The operations of an `AttributeList` object.
fn attributes(cx: &Cx, arguments: &[Value]) -> Peek<Vec<Value>> {
    let lua = cx.lua;
    let operation = name_argument(arguments, 0)?;
    let object = argument(arguments, 1);
    let pairs = match with_node(&object, |node| match node {
        Node::AttributeList(pairs) => Some(pairs.clone()),
        _ => None,
    }) {
        Some(Some(pairs)) => pairs,
        _ => return Err(mismatch("AttributeList", &object).thrown()),
    };
    let key = argument(arguments, 2);
    let pair_value = |pair: &(Text, Text)| -> Peek<Value> {
        let items = [push::text(lua, &pair.0)?, push::text(lua, &pair.1)?];
        Ok(Value::Table(lua.create_sequence_from(items)?))
    };
    match operation.as_str() {
        "len" => Ok(vec![Value::Integer(pairs.len() as i64)]),
        "pairs" => {
            let items = pairs.iter().map(pair_value).collect::<Peek<Vec<Value>>>()?;
            Ok(vec![Value::Table(lua.create_sequence_from(items)?)])
        }
        "index" => match &key {
            Value::String(name) => {
                let name = name.to_string_lossy();
                match pairs.iter().find(|(k, _)| *k == name) {
                    Some((_, value)) => Ok(vec![push::text(lua, value)?]),
                    None => Ok(vec![Value::Nil]),
                }
            }
            Value::Integer(_) | Value::Number(_) => {
                let index = cx.integral(&key).map_err(PeekError::thrown)?;
                let item = usize::try_from(index.wrapping_sub(1))
                    .ok()
                    .and_then(|index| pairs.get(index));
                match item {
                    Some(pair) => Ok(vec![pair_value(pair)?]),
                    None => Ok(vec![Value::Nil]),
                }
            }
            _ => Ok(vec![Value::Nil]),
        },
        "newindex" => {
            let value = argument(arguments, 3);
            let updated = set_attribute(cx, pairs, &key, &value)?;
            set_node(&object, Node::AttributeList(updated));
            Ok(Vec::new())
        }
        other => Err(PeekError::Thrown(format!(
            "internal error: unknown operation {other}"
        ))),
    }
}

/// The new value of an attribute: a string, or a pair of a key and a value.
enum Attribute {
    Value(Text),
    Pair(Text, Text),
}

/// Sets, replaces or removes one attribute, as the `__newindex` function
/// of `AttributeList` in `pandoc-lua-marshal` does.
fn set_attribute(
    cx: &Cx,
    mut pairs: Vec<(Text, Text)>,
    key: &Value,
    value: &Value,
) -> Peek<Vec<(Text, Text)>> {
    let call = Call {
        cx,
        name: "",
        arguments: std::slice::from_ref(value),
    };
    let new = call.optional(0, "value", |v| match cx.text(v) {
        Ok(text) => Ok(Attribute::Value(text)),
        Err(PeekError::Failure { .. }) => {
            let table = match v {
                Value::Table(table) => table,
                other => return Err(mismatch("table", other).thrown()),
            };
            let key = cx.text(&table.raw_get::<Value>(1)?)?;
            let item = cx.text(&table.raw_get::<Value>(2)?)?;
            Ok(Attribute::Pair(key, item))
        }
        Err(thrown) => Err(thrown),
    })?;
    let fail = |message: &str| Err(PeekError::Thrown(message.to_string()));
    match key {
        Value::String(name) => {
            let name = name.to_string_lossy();
            let position = pairs.iter().position(|(k, _)| *k == name);
            match (position, new) {
                (Some(index), None) => {
                    pairs.remove(index);
                }
                (Some(index), Some(Attribute::Value(text))) => {
                    if let Some(pair) = pairs.get_mut(index) {
                        pair.1 = text;
                    }
                }
                (None, None) => {}
                (None, Some(Attribute::Value(text))) => pairs.push((name, text)),
                (_, Some(Attribute::Pair(..))) => return fail("invalid attribute value"),
            }
        }
        Value::Integer(_) | Value::Number(_) => {
            let index = cx.integral(key).map_err(PeekError::thrown)?;
            // The index wraps around as an `Int` of Haskell does, and an
            // index below 1 means the first pair.
            let position = usize::try_from(index.wrapping_sub(1).max(0))
                .ok()
                .filter(|index| *index < pairs.len());
            if let Some(index) = position {
                match new {
                    None => {
                        pairs.remove(index);
                    }
                    Some(Attribute::Pair(k, v)) => {
                        if let Some(pair) = pairs.get_mut(index) {
                            *pair = (k, v);
                        }
                    }
                    Some(Attribute::Value(text)) => {
                        if let Some(pair) = pairs.get_mut(index) {
                            pair.1 = text;
                        }
                    }
                }
            } else {
                match new {
                    None => {}
                    Some(Attribute::Pair(k, v)) => pairs.push((k, v)),
                    Some(Attribute::Value(_)) => {
                        return fail(
                            "trying to set an attribute key-value pair, \
                             but got a single string instead.",
                        );
                    }
                }
            }
        }
        _ => return fail("invalid attribute key"),
    }
    Ok(pairs)
}

/// The operations of a `Version` object.
fn versions(cx: &Cx, arguments: &[Value]) -> Peek<Vec<Value>> {
    let lua = cx.lua;
    let operation = name_argument(arguments, 0)?;
    let first = argument(arguments, 1);
    match operation.as_str() {
        "construct" => {
            let call = Call {
                cx,
                name: "Version",
                arguments: arguments.get(1..).unwrap_or_default(),
            };
            let parts = call.required(0, "version_specifier", |v| version(cx, v))?;
            Ok(vec![new_object(lua, Node::Version(parts))?])
        }
        "new" => {
            let parts = version(cx, &first).map_err(PeekError::thrown)?;
            Ok(vec![new_object(lua, Node::Version(parts))?])
        }
        "compare" => {
            let left = version(cx, &first).map_err(PeekError::thrown)?;
            let right = version(cx, &argument(arguments, 2)).map_err(PeekError::thrown)?;
            Ok(vec![Value::Integer(match left.cmp(&right) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })])
        }
        "len" | "list" | "index" => {
            let parts = version(cx, &first).map_err(PeekError::thrown)?;
            match operation.as_str() {
                "len" => Ok(vec![Value::Integer(parts.len() as i64)]),
                "list" => Ok(vec![Value::Table(lua.create_sequence_from(parts)?)]),
                _ => {
                    let index = match argument(arguments, 2) {
                        Value::Integer(index) => usize::try_from(index.wrapping_sub(1)).ok(),
                        _ => None,
                    };
                    let part = index.and_then(|index| parts.get(index));
                    Ok(vec![part.map_or(Value::Nil, |part| Value::Integer(*part))])
                }
            }
        }
        other => Err(PeekError::Thrown(format!(
            "internal error: unknown operation {other}"
        ))),
    }
}

/// Returns the text of a value without its formatting, as
/// `pandoc.utils.stringify` does.
fn stringify_value(cx: &Cx, value: &Value) -> Peek<String> {
    let node = with_node(value, Node::type_name)
        .map(|type_name| cx.object(type_name, value).map_err(PeekError::thrown))
        .transpose()?;
    match node {
        Some(Node::Inline(inline)) => Ok(stringify(&[inline])),
        Some(Node::Block(block)) => Ok(stringify_blocks(&[block])),
        Some(Node::Pandoc(doc)) => Ok(stringify_blocks(&doc.blocks)),
        Some(Node::Cell(cell)) => Ok(stringify_blocks(&cell.content)),
        Some(Node::Citation(_) | Node::Attr(_)) => Ok(String::new()),
        Some(_) => Err(PeekError::Thrown(
            "Cannot get Attr from TypeUserdata".to_string(),
        )),
        None if is_none_or_nil(value) || matches!(value, Value::Function(_) | Value::Thread(_)) => {
            // The last form that Pandoc tries is an attribute set.
            cx.attr(value).map(|_: Attr| String::new())
        }
        None => {
            let meta = cx.meta_value(value, 1).map_err(PeekError::thrown)?;
            Ok(stringify_meta(&meta))
        }
    }
}

/// Makes the list `PANDOC_STATE.FIELD` from strings.
pub(crate) fn string_list(lua: &Lua, items: &[Text]) -> mlua::Result<Value> {
    push::list(lua, items, |lua, item| push::text(lua, item))
}

/// The operating system name of GHC (`System.Info.os`).
fn system_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "mingw32",
        other => other,
    }
}

/// The architecture name of GHC (`System.Info.arch`).
fn system_arch() -> &'static str {
    std::env::consts::ARCH
}
