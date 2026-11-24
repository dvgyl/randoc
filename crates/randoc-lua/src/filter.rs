//! The application of one Lua filter file to a document.

use mlua::{Function, Lua, MultiValue, Table, Value, Variadic};
use randoc_ast::{Pandoc, Text};
use randoc_core::{Context, Error, LogMessage, ReaderOptions, SourcePos, WriterOptions};

use std::cell::RefCell;

use crate::construct::Call;
use crate::convert::{self, ReaderLookup, WriterLookup};
use crate::host::{self, Host};
use crate::module;
use crate::options::{default_reader_options, default_writer_options};
use crate::os::Os;
use crate::peek::{Cx, LuaText, Peek, PeekError, mismatch, retrieving};
use crate::runtime::{self, LuaRuntime, Mode, RuntimeLimits};
use crate::simple_table::TableCellLimit;
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

/// Runs the citation filter with the conversion's resource lookup and reader options.
pub type Citeproc = fn(&mut Context, &mut Pandoc, &FilterEnvironment) -> randoc_core::Result<()>;

/// Runs a JSON filter program with its arguments and input document.
pub type JsonFilter =
    fn(&mut Context, &Pandoc, &str, &[Text], &FilterEnvironment) -> randoc_core::Result<Pandoc>;

/// Collects the references of a document for `pandoc.utils.references`.
pub type References = fn(
    &mut Context,
    &Pandoc,
    &FilterEnvironment,
) -> randoc_core::Result<
    Vec<randoc_citeproc::reference::Reference<Vec<randoc_citeproc::markup::Markup>>>,
>;

/// What a filter can see of the conversion that runs it.
#[derive(Debug, Clone)]
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
    /// `PANDOC_READER_OPTIONS`.
    pub reader_options: ReaderOptions,
    /// `PANDOC_WRITER_OPTIONS`.
    pub writer_options: WriterOptions,
    /// The readers of `pandoc.read` and `pandoc.readers`. The program gives
    /// `randoc::formats::reader`; the default knows no reader.
    pub readers: ReaderLookup,
    /// The writers of `pandoc.write` and `pandoc.writers`. The program
    /// gives `randoc::formats::writer`; the default knows no writer.
    pub writers: WriterLookup,
    /// The citation filter used by `pandoc.utils.citeproc`.
    pub citeproc: Option<Citeproc>,
    /// The runner for `pandoc.utils.run_json_filter`.
    pub json_filter: Option<JsonFilter>,
    /// The reference collector for `pandoc.utils.references`.
    pub references: Option<References>,
    /// The conversion uses a restricted host for `--sandbox`.
    pub sandbox: bool,
}

impl FilterEnvironment {
    /// Returns the environment of a conversion to `format` with the
    /// defaults of Pandoc: full mode, no input file, the resource path `.`,
    /// the request header `User-Agent: pandoc/3.13`, the verbosity
    /// `WARNING`, the reader options of the input format `json` and the
    /// writer options of `format`.
    pub fn new(format: impl Into<Text>) -> Self {
        let format = format.into();
        FilterEnvironment {
            reader_options: default_reader_options("json"),
            writer_options: default_writer_options(&format),
            format,
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
            readers: convert::no_reader,
            writers: convert::no_writer,
            citeproc: None,
            json_filter: None,
            references: None,
            sandbox: false,
        }
    }
}

/// Applies one Lua filter file to a document.
///
/// The filter runs in a new Lua state, as in Pandoc ([`LuaRuntime`] sets
/// the character locale of the process). An error of the
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
    let globals = Globals {
        format: Some(&environment.format),
        script_file: Some(filter.path),
        options: true,
    };
    with_state(environment, globals, context, |cx| {
        check_document(cx, doc)?;
        for filter in load_filters(cx, filter, environment.mode)? {
            apply_fully(cx, &filter, doc)?;
        }
        Ok(())
    })
    .map_err(|error| Error::Filter {
        filter: filter.path.to_string(),
        message: error.text(),
    })
}

/// The globals that differ between the kinds of scripts.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Globals<'a> {
    /// `FORMAT`; a reader, a writer and the interpreter have none.
    pub(crate) format: Option<&'a str>,
    /// `PANDOC_SCRIPT_FILE`; the interpreter has none.
    pub(crate) script_file: Option<&'a str>,
    /// `PANDOC_READER_OPTIONS` and `PANDOC_WRITER_OPTIONS`.
    pub(crate) options: bool,
}

/// Runs `body` in a new Lua state with what Pandoc gives each script: the
/// `pandoc` module, the globals, `warn`, `pandoc.read`, `pandoc.write` and
/// (in full mode) the system functions.
pub(crate) fn with_state<T>(
    environment: &FilterEnvironment,
    globals: Globals,
    context: &mut Context,
    body: impl FnOnce(&Cx) -> Peek<T>,
) -> Peek<T> {
    let runtime = new_state(environment, globals, context.limits.max_nesting_depth)?;
    in_scope(&runtime, environment, context, body)
}

/// Makes a Lua state with the `pandoc` module and the globals.
pub(crate) fn new_state(
    environment: &FilterEnvironment,
    globals: Globals,
    max_depth: usize,
) -> Peek<LuaRuntime> {
    let runtime = LuaRuntime::with_limits(environment.mode, environment.limits)
        .map_err(|error| PeekError::Thrown(lua_error_text(&error)))?;
    module::install(runtime.lua(), max_depth)?;
    set_globals(runtime.lua(), globals, environment, max_depth)?;
    Ok(runtime)
}

/// Runs `body` in a state of [`new_state`] with the functions that use the
/// context: `warn`, `pandoc.read`, `pandoc.write` and (in full mode) the
/// system functions. They work only during `body`; each call installs
/// them again, so one state can serve many calls.
pub(crate) fn in_scope<T>(
    runtime: &LuaRuntime,
    environment: &FilterEnvironment,
    context: &mut Context,
    body: impl FnOnce(&Cx) -> Peek<T>,
) -> Peek<T> {
    let max_depth = context.limits.max_nesting_depth;
    let lua = runtime.lua();
    let cx = Cx { lua, max_depth };
    let previous_limit = lua.set_app_data(TableCellLimit(context.limits.max_table_cells));
    // `warn`, `pandoc.read` and `pandoc.write` share the context. None of
    // them runs Lua code while it holds the context.
    let context = RefCell::new(context);
    let result = lua.scope(|scope| {
        let native: Table = lua.named_registry_value(module::NATIVE)?;
        let report = scope.create_function(|lua, (message, show): (mlua::LuaString, bool)| {
            if show {
                // The default warning function of Lua writes this line.
                let mut line = b"Lua warning: ".to_vec();
                line.extend_from_slice(&message.as_bytes());
                line.push(b'\n');
                let _ = std::io::Write::write_all(&mut std::io::stderr(), &line);
            }
            if let Ok(mut context) = context.try_borrow_mut() {
                context.report(LogMessage::ScriptingWarning(
                    message.text(),
                    caller_position(lua),
                ));
            }
            Ok(())
        })?;
        native.get::<Function>("install_warn")?.call::<()>(report)?;
        let read = scope.create_function(|lua, arguments: Variadic<Value>| {
            let cx = Cx { lua, max_depth };
            let call = Call {
                cx: &cx,
                name: "read",
                arguments: &arguments,
            };
            let result = match context.try_borrow_mut() {
                Ok(mut context) => {
                    convert::without_system_access(environment.mode, &mut context, |context| {
                        convert::read(
                            &call,
                            environment.readers,
                            &environment.reader_options,
                            context,
                        )
                    })
                }
                Err(_) => Err(PeekError::Thrown(BUSY.into())),
            };
            system_result(lua, result.map(|value| vec![value]))
        })?;
        let write = scope.create_function(|lua, arguments: Variadic<Value>| {
            let cx = Cx { lua, max_depth };
            let call = Call {
                cx: &cx,
                name: "write",
                arguments: &arguments,
            };
            let result = match context.try_borrow_mut() {
                Ok(mut context) => {
                    convert::without_system_access(environment.mode, &mut context, |context| {
                        convert::write(
                            &call,
                            environment.writers,
                            &environment.writer_options,
                            context,
                        )
                    })
                }
                Err(_) => Err(PeekError::Thrown(BUSY.into())),
            };
            system_result(lua, result.map(|value| vec![value]))
        })?;
        native.get::<Function>("convert_functions")?.call::<()>((
            read,
            write,
            convert::readers_table(lua, environment.readers)?,
            convert::writers_table(lua, environment.writers)?,
        ))?;
        let citeproc = scope.create_function(|lua, arguments: Variadic<Value>| {
            let cx = Cx { lua, max_depth };
            let call = Call {
                cx: &cx,
                name: "citeproc",
                arguments: &arguments,
            };
            let result = (|| {
                let mut doc = call.required(0, "doc", |value| cx.pandoc(value))?;
                let process = environment.citeproc.ok_or_else(|| {
                    PeekError::Thrown("pandoc.utils.citeproc is not available".into())
                })?;
                let mut context = context
                    .try_borrow_mut()
                    .map_err(|_| PeekError::Thrown(BUSY.into()))?;
                convert::without_system_access(environment.mode, &mut context, |context| {
                    process(context, &mut doc, environment)
                })
                .map_err(|error| PeekError::Thrown(error.to_string()))?;
                Ok(vec![crate::push::pandoc(lua, &doc)?])
            })();
            system_result(lua, result)
        })?;
        let compile_template = scope.create_function(|lua, arguments: Variadic<Value>| {
            let cx = Cx { lua, max_depth };
            let call = Call {
                cx: &cx,
                name: "compile",
                arguments: &arguments,
            };
            let host = Host::new(&cx, environment, &context);
            let result = host.system(|context| {
                crate::template::compile_userdata(&call, context).map(|value| vec![value])
            });
            system_result(lua, result)
        })?;
        native
            .get::<Function>("document_functions")?
            .call::<()>((citeproc, compile_template))?;

        let host_call = scope.create_function(|lua, arguments: Variadic<Value>| {
            let cx = Cx { lua, max_depth };
            let result = (|| {
                let module = module::name_argument(&arguments, 0)?;
                let name = module::name_argument(&arguments, 1)?;
                let call = Call {
                    cx: &cx,
                    name: &name,
                    arguments: arguments.get(2..).unwrap_or_default(),
                };
                host::call(&Host::new(&cx, environment, &context), &module, &call)
            })();
            system_result(lua, result)
        })?;
        native
            .get::<Function>("host_functions")?
            .call::<()>(host_call)?;

        // ADR 11: restricted mode has no function with file access. In
        // full mode the system functions use a native host of their own,
        // also when the conversion has a restricted host, because Pandoc
        // permits them with `--sandbox` (row I-CONTRACTS-03).
        if environment.mode == Mode::Full {
            let mut os = Os::default();
            let system = scope.create_function_mut(move |lua, arguments: Variadic<Value>| {
                let cx = Cx { lua, max_depth };
                let (name, rest) = match arguments.split_first() {
                    Some((Value::String(name), rest)) => (name.text(), rest),
                    _ => (String::new(), &arguments[..0]),
                };
                let result = system::call(&cx, &mut randoc_core::NativeHost, &mut os, &name, rest);
                system_result(lua, result)
            })?;
            let add: Function = native.get("system_functions")?;
            add.call::<()>(system)?;
        }
        Ok(body(&cx))
    });
    if let Some(limit) = previous_limit {
        lua.set_app_data(limit);
    } else {
        lua.remove_app_data::<TableCellLimit>();
    }
    result?
}

/// The error of `pandoc.read` or `pandoc.write` inside a reader or writer
/// that the same state runs.
pub(crate) const BUSY: &str =
    "pandoc.read and pandoc.write cannot run inside a reader or writer of the same script";

/// The message of an error of the runtime, without the prefix of its kind.
fn lua_error_text(error: &Error) -> String {
    match error {
        Error::Lua(message) => message.clone(),
        other => other.to_string(),
    }
}

/// The position of the Lua code that called `warn`, as Pandoc shows it:
/// the chunk name without its `@`, the line, and column 1. Level 0 is the
/// Rust function and level 1 the `warn` of the prelude.
pub(crate) fn caller_position(lua: &Lua) -> Option<SourcePos> {
    lua.inspect_stack(2, |debug| {
        let source = debug.source().source?.into_owned();
        let line = u32::try_from(debug.current_line()?).ok()?;
        let source = source.strip_prefix('@').unwrap_or(&source).to_string();
        Some(SourcePos::new(source, line, 1))
    })
    .flatten()
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
pub(crate) fn open_error(path: &str, error: &std::io::Error) -> String {
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
fn set_globals(
    lua: &Lua,
    kind: Globals,
    environment: &FilterEnvironment,
    max_depth: usize,
) -> mlua::Result<()> {
    let globals = lua.globals();
    globals.set("FORMAT", kind.format)?;
    if kind.options {
        let native: Table = lua.named_registry_value(module::NATIVE)?;
        crate::options::set_globals(
            lua,
            &native,
            &environment.reader_options,
            &environment.writer_options,
            max_depth,
        )?;
    } else {
        globals.set("PANDOC_READER_OPTIONS", Value::Nil)?;
        globals.set("PANDOC_WRITER_OPTIONS", Value::Nil)?;
    }
    globals.set("PANDOC_SCRIPT_FILE", kind.script_file)?;
    let state = lua.create_table()?;
    state.set(
        "input_files",
        module::string_list(lua, &environment.input_files)?,
    )?;
    state.set("output_file", environment.output_file.as_deref())?;
    let getters = lua.create_table()?;
    let native: Table = lua.named_registry_value(module::NATIVE)?;
    getters.set("log", native.get::<Function>("state_log")?)?;
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
    let index: Function = lua.load(STATE_INDEX).set_name("=[randoc]").call((
        state,
        lua.create_sequence_from(STATE_KEYS)?,
        getters,
    ))?;
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
/// the list of field names, and the getter functions.
const STATE_INDEX: &str = r#"
local state, names, getters = ...
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
  local getter = getters[key]
  if getter then return getter() end
  return copy(state[key])
end
"#;

/// Compiles a script file as Pandoc does: without a byte order mark and
/// without a first line that starts with `#`, so that a script can be a
/// program of a Unix shell. The line end stays for the line count.
pub(crate) fn load_chunk(cx: &Cx, path: &str, source: &[u8], mode: Mode) -> Peek<Function> {
    let source = source.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(source);
    let source = match source {
        [b'#', ..] => {
            let end = source
                .iter()
                .position(|byte| *byte == b'\n')
                .unwrap_or(source.len());
            source.get(end..).unwrap_or_default()
        }
        other => other,
    };
    runtime::load_code(cx.lua, mode, source)
        .set_name(format!("@{path}"))
        .into_function()
        .map_err(|error| PeekError::Thrown(runtime::message(&error)))
}

/// Runs the filter file and returns its filters: the tables of the list
/// that the file returns, the one table that it returns, or the global
/// functions when it returns nothing.
fn load_filters(cx: &Cx, filter: &LuaFilter, mode: Mode) -> Peek<Vec<Filter>> {
    let chunk = load_chunk(cx, filter.path, filter.source, mode)?;
    run_chunk(cx, chunk, cx.lua.globals())
}

pub(crate) fn run_chunk(cx: &Cx, chunk: Function, env: Table) -> Peek<Vec<Filter>> {
    let call_traced: Function = cx.lua.named_registry_value(CALL_TRACED)?;
    let mut results = call_traced.call::<MultiValue>(chunk)?.into_iter();
    if results.next() != Some(Value::Boolean(true)) {
        return Err(PeekError::Thrown(match results.next() {
            Some(Value::String(message)) => message.text(),
            _ => "the filter failed with an error that is not a string".to_string(),
        }));
    }
    // A file that returns no value defines global functions.
    let Some(last) = results.last() else {
        return Ok(vec![Filter::peek(&Value::Table(env))?]);
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
