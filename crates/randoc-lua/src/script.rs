//! The entrances for Lua scripts that are not filters: a custom reader
//! (`pandoc -f FILE.lua`), a custom writer (`pandoc -t FILE.lua`) and the
//! interpreter (`pandoc lua FILE ARGS`).
//!
//! A reader or writer object of `randoc-core` must be `Send` and `Sync`,
//! and a Lua state is neither. So each call runs the script in a new
//! state: [`custom_reader`] and [`custom_writer`] run it one time to find
//! its kind, and each read or write runs it again (row I-LUA-29).

use std::sync::Arc;

use mlua::{Function, MultiValue, Table, Value};
use randoc_ast::{Pandoc, Text};
use randoc_core::{
    BinaryReader, BinaryWriter, Context, Error, Reader, ReaderOptions, Sources, TextReader,
    TextWriter, Writer, WriterOptions,
};

use crate::filter::{FilterEnvironment, Globals, LuaFilter, load_chunk, open_error, with_state};
use crate::peek::{Cx, LuaText, Peek, PeekError};
use crate::walk::CALL_TRACED;
use crate::{options, push};

/// Reads a script file, with the error text of Pandoc for a file that
/// cannot be read (exit code 84).
pub fn read_script(path: &str) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(|error| Error::Lua(open_error(path, &error)))
}

/// The kinds of custom readers and writers, by the global that the script
/// defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Binary,
}

/// A custom reader or writer script: the path as the user gave it, the
/// content of the file and the environment of the conversion.
#[derive(Debug, Clone)]
struct Script {
    path: Text,
    source: Vec<u8>,
    environment: FilterEnvironment,
}

impl Script {
    fn new(script: &LuaFilter, environment: FilterEnvironment) -> Self {
        Script {
            path: script.path.into(),
            source: script.source.to_vec(),
            environment,
        }
    }

    /// Runs the script and then `body` with the state, as Pandoc runs a
    /// custom reader or writer: without `FORMAT` and the option globals.
    fn run<T>(&self, context: &mut Context, body: impl FnOnce(&Cx) -> Peek<T>) -> Result<T, Error> {
        let globals = Globals {
            format: None,
            script_file: Some(&self.path),
            options: false,
        };
        with_state(&self.environment, globals, context, |cx| {
            let chunk = load_chunk(cx, &self.path, &self.source, self.environment.mode)?;
            traced(cx, &chunk, MultiValue::new())?;
            body(cx)
        })
        .map_err(|error| Error::Lua(error.text()))
    }

    /// The kind of the script: the first of the globals `names` that is
    /// not `nil`, in the order text, binary.
    fn kind(&self, names: [&str; 2], context: &mut Context) -> Result<Option<Kind>, Error> {
        self.run(context, |cx| {
            let globals = cx.lua.globals();
            for (name, kind) in names.into_iter().zip([Kind::Text, Kind::Binary]) {
                if !matches!(globals.raw_get::<Value>(name)?, Value::Nil) {
                    return Ok(Some(kind));
                }
            }
            Ok(None)
        })
    }

    /// Calls the global `name` with `arguments` and a traceback for an
    /// error.
    fn call(cx: &Cx, name: &str, arguments: MultiValue) -> Peek<Value> {
        let function: Value = cx.lua.globals().raw_get(name)?;
        let results = traced_value(cx, function, arguments)?;
        Ok(results.into_iter().next().unwrap_or(Value::Nil))
    }
}

/// Calls `function` through the traceback handler of the prelude.
fn traced(cx: &Cx, function: &Function, arguments: MultiValue) -> Peek<MultiValue> {
    traced_value(cx, Value::Function(function.clone()), arguments)
}

fn traced_value(cx: &Cx, function: Value, arguments: MultiValue) -> Peek<MultiValue> {
    let call_traced: Function = cx.lua.named_registry_value(CALL_TRACED)?;
    let mut values = vec![function];
    values.extend(arguments);
    let mut results = call_traced
        .call::<MultiValue>(MultiValue::from_iter(values))?
        .into_iter();
    if results.next() == Some(Value::Boolean(true)) {
        return Ok(results.collect());
    }
    Err(PeekError::Thrown(match results.next() {
        Some(Value::String(message)) => message.text(),
        Some(other) => cx.text(&other).unwrap_or_default(),
        None => String::new(),
    }))
}

/// Returns the reader of a custom reader script: `Reader` gives a text
/// reader, `ByteStringReader` a binary reader.
///
/// The error for a script without either global is [`Error::App`] with
/// the text of Pandoc and the exit code 4; an error of the script is
/// [`Error::Lua`] (exit code 84).
pub fn custom_reader(
    script: &LuaFilter,
    environment: FilterEnvironment,
    context: &mut Context,
) -> Result<Reader, Error> {
    let script = Arc::new(Script::new(script, environment));
    match script.kind(["Reader", "ByteStringReader"], context)? {
        Some(Kind::Text) => Ok(Reader::Text(Arc::new(LuaReader(script)))),
        Some(Kind::Binary) => Ok(Reader::Binary(Arc::new(LuaReader(script)))),
        None => Err(Error::App(format!(
            "{} does not contain a custom reader",
            script.path
        ))),
    }
}

/// Returns the writer of a custom writer script: `Writer` gives a text
/// writer, `ByteStringWriter` a binary writer.
///
/// A classic custom writer (a global function for each element) is not
/// implemented (row I-LUA-29). The errors are those of [`custom_reader`].
pub fn custom_writer(
    script: &LuaFilter,
    environment: FilterEnvironment,
    context: &mut Context,
) -> Result<Writer, Error> {
    let script = Arc::new(Script::new(script, environment));
    match script.kind(["Writer", "ByteStringWriter"], context)? {
        Some(Kind::Text) => Ok(Writer::Text(Arc::new(LuaWriter(script)))),
        Some(Kind::Binary) => Ok(Writer::Binary(Arc::new(LuaWriter(script)))),
        None => Err(Error::App(format!(
            "{} does not contain a custom writer",
            script.path
        ))),
    }
}

struct LuaReader(Arc<Script>);

struct LuaWriter(Arc<Script>);

/// The `Sources` value of a custom reader: a list of tables with the
/// fields `name` and `text`; `tostring` gives the texts together.
fn push_sources(cx: &Cx, sources: &Sources) -> Peek<Value> {
    let lua = cx.lua;
    let list = lua.create_table()?;
    for source in &sources.0 {
        let item = lua.create_table()?;
        item.set("name", push::text(lua, &source.name)?)?;
        item.set("text", push::text(lua, &source.text)?)?;
        list.push(item)?;
    }
    let all: String = sources
        .0
        .iter()
        .map(|source| source.text.as_str())
        .collect();
    let all = lua.create_string(all)?;
    let metatable: Table = lua.create_table()?;
    metatable.set(
        "__tostring",
        lua.create_function(move |_, _: Value| Ok(all.clone()))?,
    )?;
    metatable.set("__name", "Sources")?;
    list.set_metatable(Some(metatable))?;
    Ok(Value::Table(list))
}

impl LuaReader {
    fn run(
        &self,
        options: &ReaderOptions,
        context: &mut Context,
        name: &str,
        input: impl FnOnce(&Cx) -> Peek<Value>,
    ) -> randoc_core::Result<Pandoc> {
        let mut script = (*self.0).clone();
        script.environment.reader_options = options.clone();
        script.run(context, |cx| {
            let input = input(cx)?;
            let options = options::reader_object(cx, options)?;
            let result = Script::call(cx, name, MultiValue::from_iter([input, options]))?;
            cx.pandoc(&result).map_err(PeekError::thrown)
        })
    }
}

impl TextReader for LuaReader {
    fn read(
        &self,
        sources: &Sources,
        options: &ReaderOptions,
        context: &mut Context,
    ) -> randoc_core::Result<Pandoc> {
        self.run(options, context, "Reader", |cx| push_sources(cx, sources))
    }
}

impl BinaryReader for LuaReader {
    fn read(
        &self,
        bytes: &[u8],
        options: &ReaderOptions,
        context: &mut Context,
    ) -> randoc_core::Result<Pandoc> {
        self.run(options, context, "ByteStringReader", |cx| {
            Ok(Value::String(cx.lua.create_string(bytes)?))
        })
    }
}

impl LuaWriter {
    fn run(
        &self,
        doc: &Pandoc,
        options: &WriterOptions,
        context: &mut Context,
        name: &str,
    ) -> randoc_core::Result<Vec<u8>> {
        let mut script = (*self.0).clone();
        script.environment.writer_options = options.clone();
        script.run(context, |cx| {
            let doc = push::pandoc(cx.lua, doc)?;
            let options = options::writer_object(cx, options)?;
            let result = Script::call(cx, name, MultiValue::from_iter([doc, options]))?;
            match &result {
                Value::String(text) => Ok(text.as_bytes().to_vec()),
                Value::Integer(_) | Value::Number(_) => Ok(cx.text(&result)?.into_bytes()),
                other => Err(crate::peek::mismatch("string", other).thrown()),
            }
        })
    }
}

impl TextWriter for LuaWriter {
    fn write(
        &self,
        doc: &Pandoc,
        options: &WriterOptions,
        context: &mut Context,
    ) -> randoc_core::Result<Text> {
        let bytes = self.run(doc, options, context, "Writer")?;
        Ok(crate::peek::text_from_bytes(&bytes))
    }
}

impl BinaryWriter for LuaWriter {
    fn write(
        &self,
        doc: &Pandoc,
        options: &WriterOptions,
        context: &mut Context,
    ) -> randoc_core::Result<Vec<u8>> {
        self.run(doc, options, context, "ByteStringWriter")
    }
}

/// Runs a script as `pandoc lua SCRIPT ARGS` does: `arg[-1]` is
/// `pandoc lua`, `arg[0]` the script, `arg[1]` and later the arguments,
/// which the chunk also gets as `...`. The options `-e`, `-i`, `-l`, `-v`
/// and `-W` of the interpreter are not implemented (row I-LUA-29).
///
/// An error is [`Error::Lua`] with the text of Pandoc (exit code 84).
pub fn run_script(
    script: &LuaFilter,
    arguments: &[String],
    environment: &FilterEnvironment,
    context: &mut Context,
) -> Result<(), Error> {
    let globals = Globals {
        format: None,
        script_file: None,
        options: false,
    };
    with_state(environment, globals, context, |cx| {
        let lua = cx.lua;
        let arg = lua.create_table()?;
        arg.raw_set(-1, "pandoc lua")?;
        arg.raw_set(0, script.path)?;
        for (index, argument) in arguments.iter().enumerate() {
            arg.raw_set(index + 1, argument.as_str())?;
        }
        lua.globals().raw_set("arg", arg)?;
        let chunk = load_chunk(cx, script.path, script.source, environment.mode)?;
        let values = arguments
            .iter()
            .map(|argument| push::text(lua, argument))
            .collect::<mlua::Result<Vec<_>>>()?;
        traced(cx, &chunk, MultiValue::from_iter(values))?;
        Ok(())
    })
    .map_err(|error| Error::Lua(error.text()))
}
