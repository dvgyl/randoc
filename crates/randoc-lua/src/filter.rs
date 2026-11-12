//! The application of one Lua filter file to a document.

use mlua::{Function, Lua, MultiValue, Table, Value, Variadic};
use randoc_ast::{Pandoc, Text};
use randoc_core::{Context, Error};

use crate::module;
use crate::peek::{Cx, Peek, PeekError, mismatch, retrieving};
use crate::runtime::{self, LuaRuntime, Mode, RuntimeLimits};
use crate::system;
use crate::walk::{CALL_TRACED, Filter, apply_fully, check_document};

/// One Lua filter file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LuaFilter<'a> {
    /// The path as the user gave it. It is the value of
    /// `PANDOC_SCRIPT_FILE`, and Lua shows it in each error position.
    pub path: &'a str,
    /// The content of the file.
    pub source: &'a [u8],
}

/// What a filter can see of the conversion that runs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterEnvironment {
    /// The output format: the value of the global `FORMAT`.
    pub format: Text,
    /// The input files: `PANDOC_STATE.input_files`.
    pub input_files: Vec<Text>,
    /// The output file: `PANDOC_STATE.output_file`.
    pub output_file: Option<Text>,
    /// The resource path: `PANDOC_STATE.resource_path`.
    pub resource_path: Vec<Text>,
    /// The base URL of the input: `PANDOC_STATE.source_url`.
    pub source_url: Option<Text>,
    /// The user data directory: `PANDOC_STATE.user_data_dir`.
    pub user_data_dir: Option<Text>,
    /// The headers of each HTTP request: `PANDOC_STATE.request_headers`.
    pub request_headers: Vec<(Text, Text)>,
    /// The value of the option `--trace`: `PANDOC_STATE.trace`.
    pub trace: bool,
    /// The verbosity: `PANDOC_STATE.verbosity`, one of `INFO`, `WARNING`
    /// and `ERROR`.
    pub verbosity: Text,
    /// The Lua libraries and the limits of the state that runs the filter.
    pub mode: Mode,
    /// The limits of the Lua state.
    pub limits: RuntimeLimits,
}

impl FilterEnvironment {
    /// Returns the environment of a conversion to `format` with the
    /// defaults of Pandoc: full mode, no input file, the resource path `.`,
    /// the request header `User-Agent: pandoc/3.13` and the verbosity
    /// `WARNING`.
    pub fn new(format: impl Into<Text>) -> Self {
        FilterEnvironment {
            format: format.into(),
            input_files: Vec::new(),
            output_file: None,
            resource_path: vec![".".to_string()],
            source_url: None,
            user_data_dir: None,
            request_headers: vec![("User-Agent".to_string(), "pandoc/3.13".to_string())],
            trace: false,
            verbosity: "WARNING".to_string(),
            mode: Mode::Full,
            limits: RuntimeLimits::for_mode(Mode::Full),
        }
    }
}

/// Applies one Lua filter file to a document.
///
/// The filter runs in a new Lua state, as in Pandoc. An error of the
/// filter is [`Error::Filter`], which has the exit code 83. After an error
/// the document can hold a part of the changes.
///
/// The function takes the document by reference so that it never drops a
/// document: a document that is nested deeper than
/// `context.limits.max_nesting_depth` gives an error, and the caller owns
/// the tree.
pub fn apply_filter(
    doc: &mut Pandoc,
    filter: &LuaFilter,
    environment: &FilterEnvironment,
    context: &mut Context,
) -> Result<(), Error> {
    let max_depth = context.limits.max_nesting_depth;
    let runtime = LuaRuntime::with_limits(environment.mode, environment.limits)?;
    let lua = runtime.lua();
    let cx = Cx { lua, max_depth };
    let mut run = || -> Peek<()> {
        check_document(&cx, doc)?;
        module::install(lua, max_depth)?;
        set_globals(lua, filter, environment)?;
        lua.scope(|scope| {
            // ADR 11: restricted mode has no function with file access. In
            // full mode the system functions use a native host of their own,
            // also when the conversion has a restricted host, because Pandoc
            // permits them with `--sandbox` (row I-CONTRACTS-03).
            if environment.mode == Mode::Full {
                let native: Table = lua.named_registry_value(module::NATIVE)?;
                let system = scope.create_function(|lua, arguments: Variadic<Value>| {
                    let cx = Cx { lua, max_depth };
                    let (name, rest) = match arguments.split_first() {
                        Some((Value::String(name), rest)) => (name.to_string_lossy(), rest),
                        _ => (String::new(), &arguments[..0]),
                    };
                    let result = system::call(&cx, &mut randoc_core::NativeHost, &name, rest);
                    system_result(lua, result)
                })?;
                let add: Function = native.get("system_functions")?;
                add.call::<()>(system)?;
            }
            let result = (|| -> Peek<()> {
                for filter in load_filters(&cx, filter, environment.mode)? {
                    apply_fully(&cx, &filter, doc)?;
                }
                Ok(())
            })();
            Ok(result)
        })?
    };
    run().map_err(|error| Error::Filter {
        filter: filter.path.to_string(),
        message: error.text(),
    })
}

/// Applies Lua filter files to a document in their order, as
/// `pandoc --lua-filter FILE ...` does. This is the entrance of the command
/// line.
///
/// Each file runs in a new Lua state with the globals of `environment`;
/// `PANDOC_SCRIPT_FILE` is the path as given. The function reads each file
/// with the file system of the process, not with the host of `context`,
/// because Pandoc reads a filter file also with `--sandbox`. A file that is
/// missing, that does not compile or that fails at run time gives
/// [`Error::Filter`] with the text of Pandoc and the exit code 83; the
/// filters after it do not run.
///
/// `PANDOC_READER_OPTIONS` and `PANDOC_WRITER_OPTIONS` are not implemented
/// yet (row I-LUA-05).
pub fn apply_filter_files(
    doc: &mut Pandoc,
    paths: &[&str],
    environment: &FilterEnvironment,
    context: &mut Context,
) -> Result<(), Error> {
    for path in paths {
        let source = std::fs::read(path).map_err(|error| Error::Filter {
            filter: (*path).to_string(),
            message: open_error(path, &error),
        })?;
        let filter = LuaFilter {
            path,
            source: &source,
        };
        apply_filter(doc, &filter, environment, context)?;
    }
    Ok(())
}

/// The message of Lua (`luaL_loadfilex`) for a filter file that it cannot
/// read.
fn open_error(path: &str, error: &std::io::Error) -> String {
    let shown = error.to_string();
    let reason = shown.split(" (os error").next().unwrap_or_default();
    if error.kind() == std::io::ErrorKind::IsADirectory {
        format!("cannot read {path}: {reason}")
    } else {
        format!("cannot open {path}: {reason}")
    }
}

/// The result of a system function for the Lua wrapper of the prelude:
/// `true` and the values, or `false` and the message.
fn system_result(lua: &Lua, result: Peek<Vec<Value>>) -> mlua::Result<MultiValue> {
    Ok(match result {
        Ok(values) => std::iter::once(Value::Boolean(true))
            .chain(values)
            .collect(),
        Err(error) => [
            Value::Boolean(false),
            Value::String(lua.create_string(error.text())?),
        ]
        .into_iter()
        .collect(),
    })
}

/// Sets the globals that Pandoc gives a filter.
fn set_globals(lua: &Lua, filter: &LuaFilter, environment: &FilterEnvironment) -> mlua::Result<()> {
    let globals = lua.globals();
    globals.set("FORMAT", environment.format.as_str())?;
    globals.set("PANDOC_SCRIPT_FILE", filter.path)?;
    let state = lua.create_table()?;
    state.set(
        "input_files",
        module::string_list(lua, &environment.input_files)?,
    )?;
    state.set("output_file", environment.output_file.as_deref())?;
    state.set("log", module::string_list(lua, &[])?)?;
    let headers = environment
        .request_headers
        .iter()
        .map(|(name, value)| lua.create_sequence_from([name.as_str(), value.as_str()]))
        .collect::<mlua::Result<Vec<_>>>()?;
    let headers = lua.create_sequence_from(headers)?;
    headers.set_metatable(Some(crate::push::metatable(lua, "List")?))?;
    state.set("request_headers", headers)?;
    state.set(
        "resource_path",
        module::string_list(lua, &environment.resource_path)?,
    )?;
    state.set("source_url", environment.source_url.as_deref())?;
    state.set("user_data_dir", environment.user_data_dir.as_deref())?;
    state.set("trace", environment.trace)?;
    state.set("verbosity", environment.verbosity.as_str())?;
    // Pandoc makes each value again at each read, so a change to a list
    // that a script read does not change the state.
    let metatable = crate::push::metatable(lua, "CommonStateInterface")?;
    let index: Function = lua
        .load(STATE_INDEX)
        .set_name("=[randoc]")
        .call((state, lua.create_sequence_from(STATE_KEYS)?))?;
    metatable.set("__index", index)?;
    let proxy = lua.create_table()?;
    proxy.set_metatable(Some(metatable))?;
    globals.set("PANDOC_STATE", proxy)
}

/// The fields of `PANDOC_STATE`. A field without a value reads as nil.
const STATE_KEYS: [&str; 9] = [
    "input_files",
    "output_file",
    "log",
    "request_headers",
    "resource_path",
    "source_url",
    "user_data_dir",
    "trace",
    "verbosity",
];

/// The `__index` function of `PANDOC_STATE`. It gets the table of values
/// and the list of the field names.
const STATE_INDEX: &str = r#"
local state, names = ...
local known = {}
for _, name in ipairs(names) do known[name] = true end
local error, pairs, type, setmetatable, getmetatable = error, pairs, type, setmetatable, getmetatable
local function copy(value)
  if type(value) ~= 'table' then return value end
  local result = {}
  for key, item in pairs(value) do result[key] = copy(item) end
  return setmetatable(result, getmetatable(value))
end
return function(_, key)
  if type(key) ~= 'string' and type(key) ~= 'number' then
    error('string expected, got ' .. type(key), 0)
  end
  if not known[key] then error('Unknown key', 0) end
  local value = state[key]
  return copy(value)
end
"#;

/// Runs the filter file and returns its filters: the tables of the list
/// that the file returns, the one table that it returns, or the global
/// functions when it returns nothing.
fn load_filters(cx: &Cx, filter: &LuaFilter, mode: Mode) -> Peek<Vec<Filter>> {
    let lua = cx.lua;
    // Lua ignores a first line that starts with `#`, so that a script can
    // be a program of a Unix shell. The line end stays for the line count.
    let source = filter
        .source
        .strip_prefix(b"\xEF\xBB\xBF")
        .unwrap_or(filter.source);
    let source = match source {
        [b'#', ..] => {
            let end = filter
                .source
                .iter()
                .position(|byte| *byte == b'\n')
                .unwrap_or(filter.source.len());
            filter.source.get(end..).unwrap_or_default()
        }
        other => other,
    };
    let chunk: Function = runtime::load_code(lua, mode, source)
        .set_name(format!("@{}", filter.path))
        .into_function()
        .map_err(|error| PeekError::Thrown(runtime::message(&error)))?;
    let call_traced: Function = lua.named_registry_value(CALL_TRACED)?;
    let mut results = call_traced.call::<MultiValue>(chunk)?.into_iter();
    if results.next() != Some(Value::Boolean(true)) {
        return Err(PeekError::Thrown(match results.next() {
            Some(Value::String(message)) => message.to_string_lossy(),
            _ => "the filter failed with an error that is not a string".to_string(),
        }));
    }
    // A file that returns no value defines global functions.
    let Some(last) = results.last() else {
        return Ok(vec![Filter::peek(&Value::Table(lua.globals()))?]);
    };
    // Pandoc reads the last value as a list of filters when its raw
    // length is not 0, and as one filter in each other case. The raw
    // length of a string is its size, and that of a userdata value is the
    // size of its memory, so these two always give the error of a list.
    match &last {
        Value::Table(table) if table.raw_len() > 0 => {
            let mut filters = Vec::new();
            for index in 1..=table.raw_len() {
                filters.push(Filter::peek(&table.raw_get::<Value>(index)?)?);
            }
            Ok(filters)
        }
        Value::String(text) if text.as_bytes().is_empty() => Ok(vec![Filter::peek(&last)?]),
        Value::String(_) | Value::UserData(_) => Err(retrieving::<Vec<Filter>>(
            "list",
            Err(mismatch("table", &last)),
        )
        .err()
        .unwrap_or_else(|| PeekError::failure("table expected"))),
        other => Ok(vec![Filter::peek(other)?]),
    }
}
