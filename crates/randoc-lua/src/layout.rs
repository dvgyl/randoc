//! Layout documents and the Lua layout operations.

use std::{cell::Cell, rc::Rc};

use mlua::{Lua, Table, UserData, UserDataFields, Value};
use randoc_layout::{self as layout, Color, Doc};

use crate::construct::Call;
use crate::object::typed_fields;
use crate::peek::{Cx, Peek, PeekError, mismatch};
use crate::push;

const LIMIT: usize = 256 << 20;

#[derive(Clone)]
struct LiveBytes {
    used: Rc<Cell<usize>>,
    limit: usize,
}

impl Default for LiveBytes {
    fn default() -> Self {
        Self {
            used: Rc::default(),
            limit: LIMIT,
        }
    }
}

pub(crate) struct LuaDoc {
    doc: Doc,
    live: LiveBytes,
    bytes: usize,
}

impl Drop for LuaDoc {
    fn drop(&mut self) {
        self.live
            .used
            .set(self.live.used.get().saturating_sub(self.bytes));
    }
}

impl UserData for LuaDoc {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        typed_fields(
            fields,
            "Doc",
            &["__concat", "__add", "__div", "__idiv", "__eq"],
        );
    }
}

fn size_error() -> PeekError {
    PeekError::Thrown("layout document exceeded size limit".into())
}

pub(crate) fn userdata(lua: &Lua, doc: Doc) -> Peek<Value> {
    let live = match lua.app_data_ref::<LiveBytes>() {
        Some(live) => live.clone(),
        None => {
            let live = LiveBytes::default();
            lua.set_app_data(live.clone());
            live
        }
    };
    let bytes = doc.estimated_size();
    if bytes > live.limit {
        return Err(size_error());
    }
    let total = || live.used.get().saturating_add(bytes);
    if total() > live.limit {
        // Lua tracks the userdata allocation. It cannot see the Rust document tree.
        lua.gc_collect()?;
        lua.gc_collect()?;
    }
    let total = total();
    if total > live.limit {
        return Err(size_error());
    }
    live.used.set(total);
    Ok(Value::UserData(lua.create_userdata(LuaDoc {
        doc,
        live,
        bytes,
    })?))
}

pub(crate) fn install_constants(lua: &Lua, table: &Table) -> mlua::Result<()> {
    for (name, doc) in [
        ("empty", layout::empty()),
        ("space", layout::space()),
        ("cr", layout::cr()),
        ("blankline", layout::blankline()),
    ] {
        let value = userdata(lua, doc).map_err(|error| mlua::Error::RuntimeError(error.text()))?;
        table.raw_set(name, value)?;
    }
    Ok(())
}

pub(crate) fn read(cx: &Cx, value: &Value) -> Peek<Doc> {
    if let Value::UserData(value) = value
        && let Ok(doc) = value.borrow::<LuaDoc>()
    {
        Ok(doc.doc.clone())
    } else if matches!(
        value,
        Value::String(_) | Value::Integer(_) | Value::Number(_)
    ) {
        Ok(layout::literal(cx.text(value)?))
    } else {
        Err(mismatch("Doc", value))
    }
}

fn bounded_number(number: i64) -> usize {
    usize::try_from(number).unwrap_or(if number < 0 { 0 } else { usize::MAX })
}

fn magnitude(call: &Call, index: usize, name: &str) -> Peek<usize> {
    let value = bounded_number(call.required(index, name, |v| call.cx.integral(v))?);
    if value > LIMIT {
        return Err(size_error());
    }
    Ok(value)
}

fn color(cx: &Cx, value: &Value) -> Peek<Color> {
    let name = cx.text(value)?;
    Ok(match name.as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        _ => return Err(PeekError::failure(format!("Unknown color: {name}"))),
    })
}

fn render(call: &Call, value: &Value) -> Peek<Value> {
    let borrow = if let Value::UserData(value) = value {
        value.borrow::<LuaDoc>().ok()
    } else {
        None
    };
    let owned;
    let doc = if let Some(doc) = &borrow {
        &doc.doc
    } else {
        owned = call.required(0, "doc", |value| read(call.cx, value))?;
        &owned
    };
    let width = call
        .optional(1, "colwidth", |v| call.cx.integral(v))?
        .map(bounded_number);
    let ansi = call
        .optional(2, "style", |v| {
            let style = call.cx.text(v)?;
            match style.as_str() {
                "plain" => Ok(false),
                "ansi" => Ok(true),
                _ => Err(PeekError::failure(format!(
                    "Unknown rendering style: {style}"
                ))),
            }
        })?
        .unwrap_or(false);
    let text = if ansi {
        doc.render_ansi_limited(width, LIMIT)
    } else {
        doc.render_limited_width(width, LIMIT)
    }
    .ok_or_else(size_error)?;
    Ok(push::text(call.cx.lua, &text)?)
}

fn equal(a: &Value, b: &Value) -> bool {
    if let (Value::UserData(a), Value::UserData(b)) = (a, b)
        && let (Ok(a), Ok(b)) = (a.borrow::<LuaDoc>(), b.borrow::<LuaDoc>())
    {
        a.doc == b.doc
    } else {
        false
    }
}

fn concat(call: &Call) -> Peek<Doc> {
    let cx = call.cx;
    let bytes = Cell::new(0usize);
    let docs = call.required(0, "docs", |v| {
        cx.list(v, |v| {
            let doc = read(cx, v)?;
            let total = bytes.get().saturating_add(doc.estimated_size());
            if total > LIMIT {
                return Err(size_error());
            }
            bytes.set(total);
            Ok(doc)
        })
    })?;
    let sep = call
        .optional(1, "sep", |v| read(cx, v))?
        .unwrap_or_else(layout::empty);
    let separators = docs
        .len()
        .saturating_sub(1)
        .saturating_mul(sep.estimated_size());
    if bytes.get().saturating_add(separators) > LIMIT {
        return Err(size_error());
    }
    Ok(layout::hcat(docs.into_iter().enumerate().flat_map(
        |(index, doc)| {
            [(index > 0).then(|| sep.clone()), Some(doc)]
                .into_iter()
                .flatten()
        },
    )))
}

pub(crate) fn call(call: &Call) -> Peek<Value> {
    let cx = call.cx;
    let text = |index, name| call.required(index, name, |v| cx.text(v));
    let doc = match call.name {
        "get" => return Ok(Value::Nil),
        "set" => return Err(PeekError::Thrown("Cannot set unknown property.".into())),
        "__eq" => return Ok(Value::Boolean(equal(&call.argument(0), &call.argument(1)))),
        "literal" => layout::literal(text(0, "text")?),
        "after_break" => layout::after_break(&text(0, "text")?),
        "blanklines" => layout::blanklines(magnitude(call, 0, "n")?),
        "concat" => concat(call)?,
        "render" => return render(call, &call.argument(0)),
        "real_length" => return Ok(Value::Integer(layout::real_length(&text(0, "str")?) as i64)),
        op @ ("__concat" | "__add" | "__div" | "__idiv") => {
            let operands = Call { name: "", ..*call };
            let a = operands.required(0, "a", |v| read(cx, v))?;
            let b = operands.required(1, "b", |v| read(cx, v))?;
            match op {
                "__add" => a.spaced(b),
                "__div" => a.above(b),
                "__idiv" => a.above_blank(b),
                _ => a + b,
            }
        }
        name => {
            let parameter = match name {
                "inside" => "contents",
                "vfill" => "border",
                _ => "doc",
            };
            if name == "vfill" {
                return userdata(cx.lua, layout::vfill(&text(0, parameter)?));
            }
            let doc = call.required(0, parameter, |v| read(cx, v))?;
            match name {
                "before_non_blank" => layout::before_non_blank(doc),
                "bold" => layout::bold(doc),
                "italic" => layout::italic(doc),
                "underlined" => layout::underlined(doc),
                "strikeout" => layout::strikeout(doc),
                "fg" => layout::fg(call.required(1, "color", |v| color(cx, v))?, doc),
                "bg" => layout::bg(call.required(1, "color", |v| color(cx, v))?, doc),
                "braces" => layout::braces(doc),
                "brackets" => layout::brackets(doc),
                "parens" => layout::parens(doc),
                "quotes" => layout::quotes(doc),
                "double_quotes" => layout::double_quotes(doc),
                "cblock" | "lblock" | "rblock" => {
                    let width = magnitude(call, 1, "width")?;
                    match name {
                        "cblock" => layout::cblock_limited(width, &doc, LIMIT),
                        "lblock" => layout::lblock_limited(width, &doc, LIMIT),
                        _ => layout::rblock_limited(width, &doc, LIMIT),
                    }
                    .ok_or_else(size_error)?
                }
                "chomp" => layout::chomp(doc),
                "flush" => layout::flush(doc),
                "hang" => layout::hang(
                    magnitude(call, 1, "ind")?,
                    call.required(2, "start", |v| read(cx, v))?,
                    doc,
                ),
                "height" => {
                    let text = doc.render_limited(LIMIT).ok_or_else(size_error)?;
                    return Ok(Value::Integer(
                        text.bytes().filter(|&b| b == b'\n').count() as i64 + 1,
                    ));
                }
                "inside" => layout::inside(
                    call.required(1, "start", |v| read(cx, v))?,
                    call.required(2, "end", |v| read(cx, v))?,
                    doc,
                ),
                "is_empty" => return Ok(Value::Boolean(doc.is_empty())),
                "min_offset" => return Ok(Value::Integer(doc.min_offset() as i64)),
                "offset" => return Ok(Value::Integer(doc.offset() as i64)),
                "nest" => layout::nest(magnitude(call, 1, "ind")?, doc),
                "nestle" => layout::nestle(doc),
                "nowrap" => layout::nowrap(doc),
                "prefixed" => layout::prefixed(&text(1, "prefix")?, doc),
                "tostring" => {
                    return Ok(push::text(
                        cx.lua,
                        &doc.render_limited(LIMIT).ok_or_else(size_error)?,
                    )?);
                }
                "update_column" => {
                    let column = call.required(1, "i", |v| cx.integral(v))?;
                    if column < 0
                        && let Some(text) = doc.render_limited(LIMIT)
                        && !text.contains('\n')
                        && doc == layout::literal(text)
                    {
                        return Ok(Value::Integer(column.saturating_add(doc.offset() as i64)));
                    }
                    return Ok(Value::Integer(
                        doc.update_column(bounded_number(column)) as i64
                    ));
                }
                _ => {
                    return Err(PeekError::Thrown(format!(
                        "unknown layout function: {name}"
                    )));
                }
            }
        }
    };
    userdata(cx.lua, doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concatenation_reclaims_unreachable_documents() -> mlua::Result<()> {
        let lua = Lua::new();
        lua.set_app_data(LiveBytes {
            used: Rc::default(),
            limit: 256 << 10,
        });
        crate::module::install(&lua, 128)?;
        lua.load(
            r#"
            local layout = pandoc.layout
            local acc = layout.empty
            local line = ('x'):rep(50)
            collectgarbage('stop')
            for _ = 1, 300 do
                acc = acc .. layout.literal(line)
            end
            assert(#layout.render(acc) == 15000)
            "#,
        )
        .exec()
    }

    #[test]
    fn collection_keeps_charging_live_documents() -> mlua::Result<()> {
        let lua = Lua::new();
        lua.set_app_data(LiveBytes {
            used: Rc::default(),
            limit: 256 << 10,
        });
        crate::module::install(&lua, 128)?;
        lua.load(
            r#"
            local layout = pandoc.layout
            local live = {}
            for i = 1, 10 do
                local ok, doc = pcall(layout.nest, layout.literal('x'), 2 ^ 16)
                if not ok then
                    assert(doc:find('layout document exceeded size limit', 1, true))
                    assert(i > 1)
                    return
                end
                live[i] = doc
            end
            error('The live document limit did not reject an allocation.')
            "#,
        )
        .exec()
    }
}
