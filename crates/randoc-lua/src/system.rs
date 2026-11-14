//! The functions of `pandoc.system`, `pandoc.path.exists` and
//! `pandoc.pipe` that use the host: files, directories, the environment and
//! processes.
//!
//! Only full mode has them (ADR 11). They use a host of their own, not the
//! host of the conversion, because Pandoc permits each of them also with
//! `--sandbox` (row I-CONTRACTS-03). The texts of the errors are the texts
//! of GHC (oracle matrix `tests/replay/system.txt`).

use std::io;

use mlua::Value;
use randoc_core::{Host, PathKind, Timestamp};

use crate::construct::Call;
use crate::os::{self, ErrorStream, Os, ProcessOptions, Variables};
use crate::peek::{Cx, Peek, PeekError, is_none_or_nil, mismatch, retrieving};
use crate::push;

/// Runs the operation `name` of the prelude with its Lua arguments. The
/// files use `host`; the copy, the environment and the processes use `os`.
pub(crate) fn call(
    cx: &Cx,
    host: &mut dyn Host,
    os: &mut Os,
    name: &str,
    arguments: &[Value],
) -> Peek<Vec<Value>> {
    let lua = cx.lua;
    let call = Call {
        cx,
        name: haskell_name(name),
        arguments,
    };
    let path = |index: usize, parameter: &str| call.required(index, parameter, |v| cx.text(v));
    let text = |value: &str| -> Peek<Vec<Value>> { Ok(vec![push::text(lua, value)?]) };
    let truthy = |index: usize| {
        let value = call.argument(index);
        !is_none_or_nil(&value) && value != Value::Boolean(false)
    };
    match name {
        "read_file" => {
            let file = path(0, "filepath")?;
            let bytes = host
                .read_file(&file)
                .map_err(|e| ghc_error(Some(&file), "withBinaryFile", &e, true))?;
            Ok(vec![Value::String(lua.create_string(bytes)?)])
        }
        "write_file" => {
            let file = path(0, "filepath")?;
            let contents = call.required(1, "contents", |v| bytes(cx, v))?;
            host.write_file(&file, &contents)
                .map_err(|e| ghc_error(Some(&file), "withBinaryFile", &e, false))?;
            Ok(Vec::new())
        }
        "list_directory" => {
            let directory = call.optional(0, "directory", |v| cx.text(v))?;
            let directory = directory.unwrap_or_else(|| ".".to_string());
            let names = host.list_directory(&directory).map_err(|e| {
                ghc_error(
                    Some(&directory),
                    "getDirectoryContents:openDirStream",
                    &e,
                    false,
                )
            })?;
            let names = names
                .iter()
                .map(|name| push::text(lua, name))
                .collect::<mlua::Result<Vec<_>>>()?;
            Ok(vec![Value::Table(lua.create_sequence_from(names)?)])
        }
        "make_directory" => {
            let directory = path(0, "dirname")?;
            host.create_directory(&directory, truthy(1))
                .map_err(|e| ghc_error(Some(&directory), "createDirectory", &e, false))?;
            Ok(Vec::new())
        }
        "remove" => {
            let file = path(0, "filename")?;
            host.remove_file(&file)
                .map_err(|e| ghc_error(Some(&file), "removeLink", &e, false))?;
            Ok(Vec::new())
        }
        "remove_directory" => {
            let directory = path(0, "dirname")?;
            let recursive = truthy(1);
            host.remove_directory(&directory, recursive).map_err(|e| {
                let operation = if recursive {
                    "removeDirectoryRecursive"
                } else {
                    "removeDirectory"
                };
                ghc_error(Some(&directory), operation, &e, false)
            })?;
            Ok(Vec::new())
        }
        "rename" => {
            let old = path(0, "old")?;
            let new = path(1, "new")?;
            // GHC names the operation by the kind of the source.
            let directory = host.path_kind(&old, false).ok() == Some(PathKind::Directory);
            host.rename(&old, &new).map_err(|e| {
                if e.kind() == io::ErrorKind::IsADirectory && !directory {
                    ghc_error(Some(&new), "renameFile", &e, true)
                } else {
                    let operation = if directory {
                        "renameDirectory"
                    } else {
                        "renameFile"
                    };
                    ghc_error(
                        None,
                        &format!("{operation}:renamePath:rename '{old}' to '{new}'"),
                        &e,
                        false,
                    )
                }
            })?;
            Ok(Vec::new())
        }
        "copy" => {
            let source = path(0, "source")?;
            let target = path(1, "target")?;
            os::copy_file(&source, &target).map_err(|failure| {
                ghc_error(
                    failure.path.as_deref(),
                    &failure.operation,
                    &failure.error,
                    true,
                )
            })?;
            Ok(Vec::new())
        }
        "times" => {
            let file = path(0, "filepath")?;
            let times = host.path_times(&file).map_err(|e| {
                ghc_error(Some(&file), "getModificationTime:getFileStatus", &e, false)
            })?;
            Ok(vec![
                push::text(lua, &iso_time(&times.modified))?,
                push::text(lua, &iso_time(&times.accessed))?,
            ])
        }
        "get_working_directory" => {
            let directory = host
                .working_directory()
                .map_err(|e| ghc_error(None, "getCurrentDirectory", &e, false))?;
            text(&directory)
        }
        "set_working_directory" => {
            let directory = path(0, "directory")?;
            host.set_working_directory(&directory)
                .map_err(|e| ghc_error(Some(&directory), "changeWorkingDirectory", &e, false))?;
            Ok(Vec::new())
        }
        "environment" => {
            let table = lua.create_table()?;
            for (variable, value) in os.variables() {
                table.set(variable, value)?;
            }
            Ok(vec![Value::Table(table)])
        }
        "getenv" => {
            let variable = path(0, "name")?;
            Ok(vec![match os.variable(&variable) {
                Some(value) => Value::String(lua.create_string(value)?),
                None => Value::Nil,
            }])
        }
        "shell_command" => text(&os.shell_command(&path(0, "command")?)),
        "with_working_directory" => {
            let directory = path(0, "directory")?;
            callback(&call, 1)?;
            let old = host
                .working_directory()
                .map_err(|e| ghc_error(None, "getCurrentDirectory", &e, false))?;
            host.set_working_directory(&directory)
                .map_err(|e| ghc_error(Some(&directory), "changeWorkingDirectory", &e, false))?;
            text(&old)
        }
        "with_environment" => {
            let wanted = call.required(0, "environment", |v| string_pairs(cx, v))?;
            callback(&call, 1)?;
            os.enter(set_env_semantics(wanted)?);
            Ok(Vec::new())
        }
        "leave_environment" => {
            os.leave();
            Ok(Vec::new())
        }
        "with_temporary_directory" => {
            // Without a third argument the parent is the temporary
            // directory of the system.
            let (parent, template, function) = if arguments.len() < 3 {
                (None, path(0, "templ")?, 1)
            } else {
                (Some(path(0, "parent_dir")?), path(1, "templ")?, 2)
            };
            callback(&call, function)?;
            let parent = parent.unwrap_or_else(|| {
                os.variable_text("TMPDIR")
                    .unwrap_or_else(|| "/tmp".to_string())
            });
            let directory = host
                .create_temporary_directory(&parent, &template)
                .map_err(|e| ghc_error(Some(&parent), "createTempDirectory", &e, false))?;
            text(&directory)
        }
        "remove_tree" => {
            // Pandoc ignores an error of the removal of a temporary directory.
            let directory = path(0, "directory")?;
            let _ = host.remove_directory(&directory, true);
            Ok(Vec::new())
        }
        "exists" => {
            let file = path(0, "path")?;
            let kind = call.optional(1, "type", |v| cx.text(v))?;
            let mut found =
                |follow: bool| host.path_kind(&file, follow).unwrap_or(PathKind::Missing);
            let exists = match kind.map(|kind| kind.to_lowercase()).as_deref() {
                None => found(true) != PathKind::Missing,
                Some("directory") => found(true) == PathKind::Directory,
                Some("file") => found(true) == PathKind::File,
                Some("symlink") => found(false) == PathKind::SymbolicLink,
                Some(other) => {
                    return Err(PeekError::Thrown(format!(
                        "Unsupported filesystem object type: {other}"
                    )));
                }
            };
            Ok(vec![Value::Boolean(exists)])
        }
        "command" | "pipe" => {
            let program = path(0, "command")?;
            let arguments = call.required(1, "args", |v| cx.list(v, |item| cx.text(item)))?;
            let (input, options, errors, operation) = if name == "pipe" {
                let input = call.required(2, "input", |v| bytes(cx, v))?;
                (
                    input,
                    ProcessOptions::default(),
                    ErrorStream::Inherit,
                    "createProcess",
                )
            } else {
                let input = call.optional(2, "input", |v| bytes(cx, v))?;
                let options = call.optional(3, "opts", |v| process_options(cx, v))?;
                (
                    input.unwrap_or_default(),
                    options.unwrap_or_default(),
                    ErrorStream::Capture,
                    "readCreateProcessWithExitCode",
                )
            };
            let output = os
                .run_process(&program, &arguments, &input, &options, errors)
                .map_err(|e| {
                    ghc_error(
                        Some(&program),
                        &format!("{operation}: posix_spawnp"),
                        &e,
                        false,
                    )
                })?;
            let code = if output.exit_code == 0 {
                Value::Boolean(false)
            } else {
                Value::Integer(i64::from(output.exit_code))
            };
            Ok(vec![
                code,
                Value::String(lua.create_string(&output.stdout)?),
                Value::String(lua.create_string(&output.stderr)?),
            ])
        }
        "cputime" => {
            let time = host
                .processor_time()
                .map_err(|e| PeekError::Thrown(e.to_string()))?;
            let picoseconds =
                i64::try_from(time.as_nanos().saturating_mul(1000)).unwrap_or(i64::MAX);
            Ok(vec![Value::Integer(picoseconds)])
        }
        "xdg" => xdg(cx, os, &call),
        other => Err(PeekError::Thrown(format!(
            "internal error: unknown function {other}"
        ))),
    }
}

/// The name of the Haskell function in the messages of argument errors.
fn haskell_name(name: &str) -> &str {
    match name {
        "command" => "cmd",
        "copy" => "cp",
        "list_directory" => "ls",
        "make_directory" => "mkdir",
        "remove" => "rm",
        "remove_directory" => "rmdir",
        "with_environment" => "with_env",
        "with_temporary_directory" => "with_tmpdir",
        "with_working_directory" => "with_wd",
        other => other,
    }
}

/// Checks that argument `index` is a function, as the callback of the
/// `with_*` functions must be.
fn callback(call: &Call, index: usize) -> Peek<()> {
    call.required(index, "callback", |value| match value {
        Value::Function(_) => Ok(()),
        other => Err(mismatch("function", other)),
    })
}

/// The bytes of a string or a number argument.
fn bytes(cx: &Cx, value: &Value) -> Peek<Vec<u8>> {
    match value {
        Value::String(text) => Ok(text.as_bytes().to_vec()),
        Value::Integer(_) | Value::Number(_) => Ok(cx.text(value)?.into_bytes()),
        _ => Err(mismatch("string", value)),
    }
}

/// The string keys and values of a table, as `peekKeyValuePairs` of HsLua
/// reads them.
fn string_pairs(cx: &Cx, value: &Value) -> Peek<Vec<(String, String)>> {
    let Value::Table(table) = value else {
        return Err(mismatch("table", value));
    };
    let mut pairs = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let read = || {
            let key = retrieving("key", cx.text(&key))?;
            let value = retrieving("value", cx.text(&value))?;
            Ok((key, value))
        };
        pairs.push(retrieving("key-value pair", read())?);
    }
    Ok(pairs)
}

/// The environment that `setEnv` of GHC makes from the pairs: an empty
/// value removes the variable, a NUL character ends a name or a value, and
/// an empty name or a name with `=` is an error.
fn set_env_semantics(pairs: Vec<(String, String)>) -> Peek<Variables> {
    let mut variables = Variables::new();
    for (name, value) in pairs {
        if name.is_empty() || name.contains('=') {
            return Err(PeekError::Thrown("setEnv: invalid argument".into()));
        }
        if value.is_empty() {
            continue;
        }
        let before_nul = |text: &str| text.split('\0').next().unwrap_or_default().to_string();
        variables.insert(before_nul(&name), before_nul(&value));
    }
    Ok(variables)
}

/// The options table of `pandoc.system.command`: `env` and `cwd`.
fn process_options(cx: &Cx, value: &Value) -> Peek<ProcessOptions> {
    let Value::Table(table) = value else {
        return Err(mismatch("table", value));
    };
    let field = |name: &str| -> Peek<Value> { Ok(table.raw_get(name)?) };
    let environment = match field("env")? {
        Value::Nil => None,
        value => Some(within_field("env", string_pairs(cx, &value))?),
    };
    let cwd = match field("cwd")? {
        Value::Nil => None,
        value => Some(within_field("cwd", cx.text(&value))?),
    };
    Ok(ProcessOptions {
        cwd,
        environment: environment.map(|pairs| pairs.into_iter().collect()),
    })
}

fn within_field<T>(name: &str, result: Peek<T>) -> Peek<T> {
    retrieving(&format!("raw field '{name}'"), result)
}

/// The message of an input or output error as GHC shows it:
/// `PATH: OPERATION: KIND (REASON)`. GHC writes its own reason `is a
/// directory` for some operations (`own_directory_text`); else the reason is
/// the text of the C library.
fn ghc_error(
    path: Option<&str>,
    operation: &str,
    error: &io::Error,
    own_directory_text: bool,
) -> PeekError {
    use io::ErrorKind;
    let kind = match error.kind() {
        ErrorKind::NotFound => "does not exist",
        ErrorKind::AlreadyExists => "already exists",
        ErrorKind::NotADirectory | ErrorKind::IsADirectory => "inappropriate type",
        ErrorKind::DirectoryNotEmpty => "unsatisfied constraints",
        ErrorKind::PermissionDenied => "permission denied",
        ErrorKind::InvalidInput => "invalid argument",
        ErrorKind::ResourceBusy => "resource busy",
        _ => "failed",
    };
    let reason = if own_directory_text && error.kind() == ErrorKind::IsADirectory {
        "is a directory".to_string()
    } else {
        let shown = error.to_string();
        match shown.find(" (os error") {
            Some(end) => shown.get(..end).unwrap_or_default().to_string(),
            None => shown,
        }
    };
    let prefix = path.map(|path| format!("{path}: ")).unwrap_or_default();
    PeekError::Thrown(format!("{prefix}{operation}: {kind} ({reason})"))
}

/// A time in the ISO 8601 form of Pandoc, in UTC, with the fraction of a
/// second without its final zeros: `2026-11-12T02:35:45.93Z`.
fn iso_time(time: &Timestamp) -> String {
    let days = time.unix_seconds.div_euclid(86_400);
    let seconds = time.unix_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let mut text = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    if time.nanoseconds > 0 {
        let fraction = format!("{:09}", time.nanoseconds);
        text.push('.');
        text.push_str(fraction.trim_end_matches('0'));
    }
    text.push('Z');
    text
}

/// The date of a day number, with day 0 at 1970-01-01 (the algorithm of
/// Howard Hinnant, `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `pandoc.system.xdg`: the directories of the XDG Base Directory
/// Specification from the environment.
fn xdg(cx: &Cx, os: &Os, call: &Call) -> Peek<Vec<Value>> {
    let lua = cx.lua;
    let key = call.required(0, "xdg_directory_type", |v| {
        let wanted: String = cx
            .text(v)?
            .chars()
            .filter(|c| *c != '_')
            .flat_map(char::to_lowercase)
            .collect();
        let key = wanted.strip_prefix("xdg").unwrap_or(&wanted).to_string();
        match key.as_str() {
            "config" | "data" | "cache" | "state" | "datadirs" | "configdirs" => Ok(key),
            _ => Err(PeekError::failure(format!(
                "Expected 'cache', 'config', 'data', or 'state', got: {key}"
            ))),
        }
    })?;
    let file = call.optional(1, "filepath", |v| cx.text(v))?;
    let home = os.variable_text("HOME").unwrap_or_default();
    let mut variable = |name: &str| os.variable_text(name);
    let single = |name: &str, default: &str, variable: &mut dyn FnMut(&str) -> Option<String>| {
        let base = match variable(name) {
            Some(value) if value.starts_with('/') => value,
            _ => format!("{home}/{default}"),
        };
        match &file {
            Some(file) if file.starts_with('/') => file.clone(),
            Some(file) if !file.is_empty() => format!("{}/{file}", base.trim_end_matches('/')),
            _ => base,
        }
    };
    let list = |name: &str, defaults: &[&str], variable: &mut dyn FnMut(&str) -> Option<String>| {
        match variable(name) {
            Some(value) if !value.is_empty() => value
                .split(':')
                .map(|part| {
                    if part.is_empty() {
                        ".".to_string()
                    } else {
                        part.to_string()
                    }
                })
                .collect(),
            _ => defaults.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
        }
    };
    let result = match key.as_str() {
        "config" => single("XDG_CONFIG_HOME", ".config", &mut variable),
        "data" => single("XDG_DATA_HOME", ".local/share", &mut variable),
        "cache" => single("XDG_CACHE_HOME", ".cache", &mut variable),
        "datadirs" => {
            let dirs = list(
                "XDG_DATA_DIRS",
                &["/usr/local/share/", "/usr/share/"],
                &mut variable,
            );
            return Ok(vec![push::list(lua, &dirs, |lua, d| push::text(lua, d))?]);
        }
        "configdirs" => {
            let dirs = list("XDG_CONFIG_DIRS", &["/etc/xdg"], &mut variable);
            return Ok(vec![push::list(lua, &dirs, |lua, d| push::text(lua, d))?]);
        }
        _ => single("XDG_STATE_HOME", ".local/state", &mut variable),
    };
    Ok(vec![push::text(lua, &result)?])
}
