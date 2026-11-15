//! The libraries `lpeg` and `re` of LPeg 1.1.0 (ADR 19).
//!
//! Pandoc gives both modules to each Lua state: as the globals `lpeg` and
//! `re`, in `package.loaded`, and through a fifth entry of
//! `package.searchers` that finds them again after a script removes them
//! from `package.loaded`. LPeg has no file, network or process access, so
//! restricted mode gets the two globals too. A match is one call of a C
//! function, so the instruction limit does not stop a long match; the
//! memory limit counts the stacks of LPeg, which are Lua userdata.

use mlua::{Function, Lua, Table, Value};

use crate::runtime::Mode;

/// The text of `re.lua`. Pandoc loads it as a string chunk, so its chunk
/// name is the text itself, and an error of `re` starts with
/// `[string "--..."]:LINE:`.
const RE_SOURCE: &str = include_str!("../lpeg-1.1.0/re.lua");

#[allow(unsafe_code)]
mod ffi {
    unsafe extern "C-unwind" {
        /// The entry function of LPeg in `lptree.c`.
        pub(super) fn luaopen_lpeg(state: *mut mlua::ffi::lua_State) -> std::ffi::c_int;
    }
}

/// Returns `luaopen_lpeg` as a Lua function.
///
/// This function holds the second `unsafe` call of the crate (ADR 19).
#[allow(unsafe_code)]
fn lpeg_opener(lua: &Lua) -> mlua::Result<Function> {
    // SAFETY: `create_c_function` is `unsafe` because Lua runs the C
    // function without a check. The argument for `luaopen_lpeg` has four
    // parts.
    // 1. The function is the unchanged `luaopen_lpeg` of LPeg 1.1.0 (the
    //    hash test of `sources.rs`), compiled by `build.rs` against the
    //    headers of the same Lua 5.4.8 that this state runs, so the
    //    `lua_State` layout and the `lua_CFunction` convention agree.
    // 2. LPeg uses only the Lua C API. It allocates through the allocator
    //    of the state (userdata), so the memory limit of `mlua` applies,
    //    and it reports each error with `lua_error`. Lua calls each C
    //    function inside a protected call of `mlua`, so a `longjmp` of
    //    LPeg never crosses a Rust frame.
    // 3. A pattern can call a Lua or Rust function (a function capture).
    //    That call goes through `lua_call` as from any C library, for
    //    example `table.sort`, and `mlua` converts each Rust error at its
    //    own boundary.
    // 4. LPeg reads no file, starts no process and loads no code, so the
    //    function adds no access to restricted mode.
    unsafe { lua.create_c_function(ffi::luaopen_lpeg) }
}

/// Gives `lua` the modules `lpeg` and `re` as Pandoc has them.
pub(crate) fn install(lua: &Lua, mode: Mode) -> mlua::Result<()> {
    let open_lpeg = lpeg_opener(lua)?;
    let lpeg: Table = open_lpeg.call("lpeg")?;
    let globals = lua.globals();
    globals.set("lpeg", &lpeg)?;
    let re = match mode {
        Mode::Full => {
            let package: Table = globals.get("package")?;
            let loaded: Table = package.get("loaded")?;
            loaded.set("lpeg", &lpeg)?;
            let re: Table = re_loader(lua)?.call("re")?;
            loaded.set("re", &re)?;
            let searchers: Table = package.get("searchers")?;
            searchers.push(searcher(lua, open_lpeg)?)?;
            re
        }
        Mode::Restricted => restricted_re(lua, lpeg)?,
    };
    globals.set("re", re)
}

/// Loads `re.lua` as a function that returns the module.
fn re_loader(lua: &Lua) -> mlua::Result<Function> {
    lua.load(RE_SOURCE).set_name(RE_SOURCE).into_function()
}

/// Loads `re` without `require`, which restricted mode does not have: the
/// chunk sees the globals and a `require` that gives `lpeg`.
fn restricted_re(lua: &Lua, lpeg: Table) -> mlua::Result<Table> {
    let environment = lua.create_table()?;
    environment.set(
        "require",
        lua.create_function(move |_, _: mlua::MultiValue| Ok(lpeg.clone()))?,
    )?;
    let meta = lua.create_table()?;
    meta.set("__index", lua.globals())?;
    environment.set_metatable(Some(meta))?;
    lua.load(RE_SOURCE)
        .set_name(RE_SOURCE)
        .set_environment(environment)
        .call("re")
}

/// The fifth searcher of Pandoc: a loader for `lpeg` and `re`, nil for any
/// other name.
fn searcher(lua: &Lua, open_lpeg: Function) -> mlua::Result<Function> {
    lua.create_function(move |lua, name: Value| {
        let Value::String(name) = name else {
            return Ok(Value::Nil);
        };
        match &*name.as_bytes() {
            b"lpeg" => Ok(Value::Function(open_lpeg.clone())),
            b"re" => re_loader(lua).map(Value::Function),
            _ => Ok(Value::Nil),
        }
    })
}
