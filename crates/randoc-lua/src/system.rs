//! The functions of `pandoc.system`, `pandoc.path.exists` and
//! `pandoc.pipe` that use the host: files, directories, the environment and
//! processes.
//!
//! Only full mode has them (ADR 11). They use a host of their own, not the
//! host of the conversion, because Pandoc permits each of them also with
//! `--sandbox` (row I-CONTRACTS-03). The texts of the errors are the texts
//! of GHC (oracle matrix `tests/replay/system.txt`).

use std::io;

use mlua::{Table, Value};
use randoc_core::{Host, PathKind, Timestamp};

use crate::construct::Call;
use crate::peek::{Cx, Peek, PeekError, is_none_or_nil};
use crate::push;

/// Runs the operation `name` of the prelude with its Lua arguments.
pub(crate) fn call(
    cx: &Cx,
    host: &mut dyn Host,
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
                .map_err(|e| ghc_error(&file, "withBinaryFile", &e, true))?;
            Ok(vec![Value::String(lua.create_string(bytes)?)])
        }
        "write_file" => {
            let file = path(0, "filepath")?;
            let contents = call.required(1, "contents", |v| bytes(cx, v))?;
            host.write_file(&file, &contents)
                .map_err(|e| ghc_error(&file, "withBinaryFile", &e, false))?;
            Ok(Vec::new())
        }
        "list_directory" => {
            let directory = call.optional(0, "directory", |v| cx.text(v))?;
            let directory = directory.unwrap_or_else(|| ".".to_string());
            let names = host.list_directory(&directory).map_err(|e| {
                ghc_error(&directory, "getDirectoryContents:openDirStream", &e, false)
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
                .map_err(|e| ghc_error(&directory, "createDirectory", &e, false))?;
            Ok(Vec::new())
        }
        "remove" => {
            let file = path(0, "filename")?;
            host.remove_file(&file)
                .map_err(|e| ghc_error(&file, "removeLink", &e, false))?;
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
                ghc_error(&directory, operation, &e, false)
            })?;
            Ok(Vec::new())
        }
        "rename" => {
            let old = path(0, "old")?;
            let new = path(1, "new")?;
            host.rename(&old, &new).map_err(|e| {
                if e.kind() == io::ErrorKind::IsADirectory {
                    ghc_error(&new, "renameFile", &e, true)
                } else {
                    ghc_error(
                        "",
                        &format!("renameFile:renamePath:rename '{old}' to '{new}'"),
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
            host.copy_file(&source, &target).map_err(|e| {
                let stem = "copyFile:atomicCopyFileContents:withReplacementFile";
                if e.kind() == io::ErrorKind::NotFound {
                    ghc_error(
                        &source,
                        &format!("{stem}:copyFileToHandle:openFdAt"),
                        &e,
                        false,
                    )
                } else {
                    ghc_error(&target, &format!("{stem}:renameFile"), &e, true)
                }
            })?;
            Ok(Vec::new())
        }
        "times" => {
            let file = path(0, "filepath")?;
            let times = host
                .path_times(&file)
                .map_err(|e| ghc_error(&file, "getModificationTime:getFileStatus", &e, false))?;
            Ok(vec![
                push::text(lua, &iso_time(&times.modified))?,
                push::text(lua, &iso_time(&times.accessed))?,
            ])
        }
        "get_working_directory" => {
            let directory = host
                .working_directory()
                .map_err(|e| ghc_error("", "getCurrentDirectory", &e, false))?;
            text(&directory)
        }
        "set_working_directory" => {
            let directory = path(0, "directory")?;
            host.set_working_directory(&directory)
                .map_err(|e| ghc_error(&directory, "changeWorkingDirectory", &e, false))?;
            Ok(Vec::new())
        }
        "environment" => {
            let table = lua.create_table()?;
            let variables = host
                .environment()
                .map_err(|e| ghc_error("", "getEnvironment", &e, false))?;
            for (variable, value) in variables {
                table.set(variable, value)?;
            }
            Ok(vec![Value::Table(table)])
        }
        "getenv" => {
            let variable = path(0, "name")?;
            Ok(vec![match host.environment_variable(&variable) {
                Some(value) => push::text(lua, &value)?,
                None => Value::Nil,
            }])
        }
        "set_environment" => {
            let Value::Table(wanted) = call.argument(0) else {
                return Err(PeekError::Thrown("table expected".into()));
            };
            set_environment(cx, host, &wanted)?;
            Ok(Vec::new())
        }
        "temporary_directory" => {
            let parent = match call.argument(0) {
                value if is_none_or_nil(&value) => host
                    .environment_variable("TMPDIR")
                    .unwrap_or_else(|| "/tmp".to_string()),
                value => cx.text(&value)?,
            };
            let template = path(1, "templ")?;
            let directory = host
                .create_temporary_directory(&parent, &template)
                .map_err(|e| ghc_error(&parent, "createTempDirectory", &e, false))?;
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
            let exists = match kind.as_deref() {
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
            let input = if name == "pipe" {
                call.required(2, "input", |v| bytes(cx, v))?
            } else {
                call.optional(2, "input", |v| bytes(cx, v))?
                    .unwrap_or_default()
            };
            let output = host
                .run_process(&program, &arguments, &input)
                .map_err(|e| {
                    ghc_error(
                        &program,
                        "readCreateProcessWithExitCode: posix_spawnp",
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
        "xdg" => xdg(cx, host, &call),
        other => Err(PeekError::Thrown(format!(
            "internal error: unknown function {other}"
        ))),
    }
}

/// The name of the Haskell function in the messages of argument errors.
fn haskell_name(name: &str) -> &str {
    match name {
        "make_directory" => "mkdir",
        "command" => "cmd",
        other => other,
    }
}

/// The bytes of a string or a number argument.
fn bytes(cx: &Cx, value: &Value) -> Peek<Vec<u8>> {
    match value {
        Value::String(text) => Ok(text.as_bytes().to_vec()),
        Value::Integer(_) | Value::Number(_) => Ok(cx.text(value)?.into_bytes()),
        _ => Err(crate::peek::mismatch("string", value)),
    }
}

/// Replaces the whole environment with the string pairs of a table, as
/// `with_environment` needs.
fn set_environment(cx: &Cx, host: &mut dyn Host, wanted: &Table) -> Peek<()> {
    let fail = |e: io::Error| PeekError::Thrown(e.to_string());
    for (variable, _) in host.environment().map_err(fail)? {
        host.set_environment_variable(&variable, None)
            .map_err(fail)?;
    }
    for pair in wanted.pairs::<Value, Value>() {
        let (variable, value) = pair?;
        let variable = cx.text(&variable)?;
        let value = cx.text(&value)?;
        host.set_environment_variable(&variable, Some(&value))
            .map_err(fail)?;
    }
    Ok(())
}

/// The message of an input or output error as GHC shows it:
/// `PATH: OPERATION: KIND (REASON)`. GHC writes its own reason `is a
/// directory` for some operations (`own_directory_text`); else the reason is
/// the text of the C library.
fn ghc_error(
    path: &str,
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
    let prefix = if path.is_empty() {
        String::new()
    } else {
        format!("{path}: ")
    };
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
fn xdg(cx: &Cx, host: &mut dyn Host, call: &Call) -> Peek<Vec<Value>> {
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
    let home = host.environment_variable("HOME").unwrap_or_default();
    let mut variable = |name: &str| host.environment_variable(name);
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
