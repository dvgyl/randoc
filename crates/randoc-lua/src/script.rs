//! The entrances for Lua scripts that are not filters: a custom reader
//! (`pandoc -f FILE.lua`), a custom writer (`pandoc -t FILE.lua`) and the
//! interpreter (`pandoc lua FILE ARGS`).
//!
//! A [`CustomScript`] keeps one Lua state, as Pandoc does: the top-level
//! code of the script runs one time, and the globals of the script stay
//! from one read or write to the next. A Lua state is neither `Send` nor
//! `Sync`, so the program uses the script directly and not as a reader or
//! writer object of `randoc-core`.

use mlua::{Function, MultiValue, Table, Value};
use randoc_ast::{Pandoc, Text};
use std::collections::BTreeSet;

use randoc_core::{
    Context, Error, Extension, Extensions, Output, ReaderOptions, Sources, WriterOptions,
};

use crate::filter::{
    FilterEnvironment, Globals, LuaFilter, in_scope, load_chunk, new_state, open_error, with_state,
};
use crate::peek::{Cx, LuaText, Peek, PeekError};
use crate::runtime::LuaRuntime;
use crate::walk::CALL_TRACED;
use crate::{options, push};

/// Reads a script file of at most `max_bytes` bytes, with the error text
/// of Pandoc for a file that cannot be read (exit code 84). The read stops
/// after the limit, so a large file or an endless stream does not fill the
/// memory.
pub fn read_script(path: &str, max_bytes: usize) -> Result<Vec<u8>, Error> {
    randoc_core::read_real_file_limited(path, max_bytes).map_err(|error| {
        let message = if error.kind() == std::io::ErrorKind::FileTooLarge {
            format!("cannot read {path}: {error}")
        } else {
            open_error(path, &error)
        };
        Error::Lua(message)
    })
}

/// The kind of a custom reader or writer, by the global that the script
/// defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptKind {
    /// `Reader` or `Writer`: the script reads or writes text.
    Text,
    /// `ByteStringReader` or `ByteStringWriter`: the script reads or
    /// writes bytes.
    Binary,
}

/// A custom reader or writer script with its Lua state.
pub struct CustomScript {
    runtime: LuaRuntime,
    path: Text,
    environment: FilterEnvironment,
}

impl CustomScript {
    /// Runs the top-level code of a script in a new state, as Pandoc loads
    /// a custom reader or writer: without `FORMAT` and the option globals.
    /// An error of the script is [`Error::Lua`] (exit code 84).
    pub fn load(
        script: &LuaFilter,
        environment: FilterEnvironment,
        context: &mut Context,
    ) -> Result<Self, Error> {
        let globals = Globals {
            format: None,
            script_file: Some(script.path),
            options: false,
        };
        let runtime = new_state(&environment, globals, context.limits.max_nesting_depth)
            .map_err(|error| Error::Lua(error.text()))?;
        in_scope(&runtime, &environment, context, |cx| {
            let chunk = load_chunk(cx, script.path, script.source, environment.mode)?;
            traced(cx, &chunk, MultiValue::new())?;
            Ok(())
        })
        .map_err(|error| Error::Lua(error.text()))?;
        Ok(CustomScript {
            runtime,
            path: script.path.into(),
            environment,
        })
    }

    /// The path of the script as the user gave it.
    pub fn path(&self) -> &str {
        &self.path
    }

    fn global(&self, name: &str) -> Result<Value, Error> {
        self.runtime
            .lua()
            .globals()
            .raw_get(name)
            .map_err(|error| Error::Lua(error.to_string()))
    }

    fn kind(&self, names: [&str; 2]) -> Result<Option<ScriptKind>, Error> {
        for (name, kind) in names
            .into_iter()
            .zip([ScriptKind::Text, ScriptKind::Binary])
        {
            if !matches!(self.global(name)?, Value::Nil) {
                return Ok(Some(kind));
            }
        }
        Ok(None)
    }

    /// The reader kind of the script: `Reader` before `ByteStringReader`.
    /// A script without either global gives [`Error::App`] with the text
    /// of Pandoc (exit code 4).
    pub fn reader_kind(&self) -> Result<ScriptKind, Error> {
        self.kind(["Reader", "ByteStringReader"])?
            .ok_or_else(|| Error::App(format!("{} does not contain a custom reader", self.path)))
    }

    /// The writer kind of the script: `Writer` before `ByteStringWriter`.
    /// A classic custom writer (a global function for each element) is not
    /// implemented (row I-LUA-29). The errors are those of
    /// [`CustomScript::reader_kind`].
    pub fn writer_kind(&self) -> Result<ScriptKind, Error> {
        self.kind(["Writer", "ByteStringWriter"])?
            .ok_or_else(|| Error::App(format!("{} does not contain a custom writer", self.path)))
    }

    /// The extensions of the format `FILE.lua+a-b`: the defaults of the
    /// global `Extensions` table with the `changes` of the format applied.
    /// The result is the names that `opts.extensions` shows, the known
    /// ones in the order of Pandoc and then the others in sorted order, and
    /// the known ones as a set. A change of a name that the table does not
    /// have is [`Error::UnsupportedExtension`] for `format` (exit code 23);
    /// a script without the table has no extensions.
    pub fn extensions(
        &self,
        changes: &[(bool, Text)],
        format: &str,
    ) -> Result<(Vec<Text>, Extensions), Error> {
        let mut supported = BTreeSet::new();
        let mut enabled = BTreeSet::new();
        if let Value::Table(table) = self.global("Extensions")? {
            for pair in table.pairs::<Value, Value>() {
                let (name, on) = pair.map_err(|error| Error::Lua(error.to_string()))?;
                if let Value::String(name) = name {
                    let name = name.text();
                    if !matches!(on, Value::Nil | Value::Boolean(false)) {
                        enabled.insert(name.clone());
                    }
                    supported.insert(name);
                }
            }
        }
        for (on, name) in changes {
            if !supported.contains(name) {
                return Err(Error::UnsupportedExtension {
                    extension: name.clone(),
                    format: format.into(),
                });
            }
            if *on {
                enabled.insert(name.clone());
            } else {
                enabled.remove(name);
            }
        }
        let mut known = Extensions::empty();
        let mut custom = Vec::new();
        for name in enabled {
            match Extension::from_name(&name) {
                Some(extension) => known.enable(extension),
                None => custom.push(name),
            }
        }
        let mut names: Vec<Text> = crate::formats::extension_names(known)
            .into_iter()
            .map(Text::from)
            .collect();
        names.extend(custom);
        Ok((names, known))
    }

    /// Calls the global `Template` and returns its text, or `None` when the
    /// script has no `Template`.
    pub fn template(&mut self, context: &mut Context) -> Result<Option<Text>, Error> {
        if matches!(self.global("Template")?, Value::Nil) {
            return Ok(None);
        }
        self.run(context, |cx| {
            let result = call(cx, "Template", MultiValue::new())?;
            Ok(Some(cx.text(&result)?))
        })
    }

    /// Calls `Reader` with the sources. `extensions` are the names that
    /// `opts.extensions` shows.
    pub fn read_text(
        &mut self,
        sources: &Sources,
        options: &ReaderOptions,
        extensions: &[Text],
        context: &mut Context,
    ) -> Result<Pandoc, Error> {
        self.read(options, extensions, context, "Reader", |cx| {
            push_sources(cx, sources)
        })
    }

    /// Calls `ByteStringReader` with the bytes of one input.
    pub fn read_binary(
        &mut self,
        bytes: &[u8],
        options: &ReaderOptions,
        extensions: &[Text],
        context: &mut Context,
    ) -> Result<Pandoc, Error> {
        self.read(options, extensions, context, "ByteStringReader", |cx| {
            Ok(Value::String(cx.lua.create_string(bytes)?))
        })
    }

    /// Calls `Writer` or `ByteStringWriter`, as `kind` gives: text output
    /// or bytes. `extensions` are the names that `opts.extensions` shows.
    pub fn write(
        &mut self,
        kind: ScriptKind,
        doc: &Pandoc,
        options: &WriterOptions,
        extensions: &[Text],
        context: &mut Context,
    ) -> Result<Output, Error> {
        let name = match kind {
            ScriptKind::Text => "Writer",
            ScriptKind::Binary => "ByteStringWriter",
        };
        self.environment.writer_options = options.clone();
        let bytes = self.run(context, |cx| {
            let doc = push::pandoc(cx.lua, doc)?;
            let options = options::writer_object(cx, options, Some(extensions))?;
            let result = call(cx, name, MultiValue::from_iter([doc, options]))?;
            match &result {
                Value::String(text) => Ok(text.as_bytes().to_vec()),
                Value::Integer(_) | Value::Number(_) => Ok(cx.text(&result)?.into_bytes()),
                other => Err(crate::peek::mismatch("string", other).thrown()),
            }
        })?;
        Ok(match kind {
            ScriptKind::Text => Output::Text(crate::peek::text_from_bytes(&bytes)),
            ScriptKind::Binary => Output::Binary(bytes),
        })
    }

    fn read(
        &mut self,
        options: &ReaderOptions,
        extensions: &[Text],
        context: &mut Context,
        name: &str,
        input: impl FnOnce(&Cx) -> Peek<Value>,
    ) -> Result<Pandoc, Error> {
        self.environment.reader_options = options.clone();
        self.run(context, |cx| {
            let input = input(cx)?;
            let options = options::reader_object(cx, options, Some(extensions))?;
            let result = call(cx, name, MultiValue::from_iter([input, options]))?;
            cx.pandoc(&result).map_err(PeekError::thrown)
        })
    }

    fn run<T>(&self, context: &mut Context, body: impl FnOnce(&Cx) -> Peek<T>) -> Result<T, Error> {
        in_scope(&self.runtime, &self.environment, context, body)
            .map_err(|error| Error::Lua(error.text()))
    }
}

/// Calls the global `name` with `arguments` and a traceback for an error.
fn call(cx: &Cx, name: &str, arguments: MultiValue) -> Peek<Value> {
    let function: Value = cx.lua.globals().raw_get(name)?;
    let results = traced_value(cx, function, arguments)?;
    Ok(results.into_iter().next().unwrap_or(Value::Nil))
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

/// The command line of `pandoc lua`, which gives the global `arg`.
#[derive(Debug, Clone, Copy)]
pub struct Invocation<'a> {
    /// The program name: `pandoc lua`, or the name of a program called
    /// `pandoc-lua`.
    pub program: &'a str,
    /// The arguments before the script, for example `--`.
    pub options: &'a [String],
    /// The script argument as the user gave it (`-` for standard input),
    /// or `None` when the command line names no script.
    pub script: Option<&'a str>,
    /// The arguments after the script.
    pub arguments: &'a [String],
}

/// Runs a script as `pandoc lua SCRIPT ARGS` does. `script.path` names the
/// chunk in error messages (Pandoc names standard input `stdin`). `arg`
/// is the table of the standalone Lua interpreter: `arg[0]` is the script
/// argument, the arguments before it have negative indices and the program
/// comes first (oracle: `pandoc lua -- a.lua x` gives `arg[-2]` `pandoc
/// lua`, `arg[-1]` `--` and `arg[1]` `x`). The arguments after the script
/// are also `...` of the chunk. Without a script argument, `arg[0]` is the
/// program. The options `-e`, `-i`, `-l`, `-v` and `-W` of the interpreter
/// are not implemented (row I-LUA-29).
///
/// An error is [`Error::Lua`] with the text of Pandoc (exit code 84).
pub fn run_script(
    script: &LuaFilter,
    invocation: &Invocation,
    environment: &FilterEnvironment,
    context: &mut Context,
) -> Result<(), Error> {
    let arguments = invocation.arguments;
    let globals = Globals {
        format: None,
        script_file: None,
        options: false,
    };
    with_state(environment, globals, context, |cx| {
        let lua = cx.lua;
        let arg = lua.create_table()?;
        let mut all: Vec<&str> = vec![invocation.program];
        all.extend(invocation.options.iter().map(String::as_str));
        let script_index = match invocation.script {
            Some(name) => {
                all.push(name);
                all.len() - 1
            }
            None => 0,
        };
        all.extend(arguments.iter().map(String::as_str));
        for (index, text) in all.into_iter().enumerate() {
            let key =
                i64::try_from(index).unwrap_or(i64::MAX) - i64::try_from(script_index).unwrap_or(0);
            arg.raw_set(key, text)?;
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
