//! The module `pandoc.path`.
//!
//! A port of `HsLua.Module.Path` of `hslua-module-path` 1.2.0 (MIT) and of
//! the POSIX rules of `System.FilePath` of `filepath` 1.5.4.0
//! (BSD-3-Clause), which that module calls.

use mlua::Value;

use crate::construct::Call;
use crate::peek::{Peek, PeekError, is_none_or_nil};
use crate::push;

/// Runs the function `name` of `pandoc.path`.
pub(crate) fn call(call: &Call, name: &str) -> Peek<Vec<Value>> {
    let cx = call.cx;
    let lua = cx.lua;
    let path = |index: usize, parameter: &str| call.required(index, parameter, |v| cx.text(v));
    let text = |value: &str| -> Peek<Vec<Value>> { Ok(vec![push::text(lua, value)?]) };
    let list = |items: Vec<String>| -> Peek<Vec<Value>> {
        let values = items
            .iter()
            .map(|item| push::text(lua, item))
            .collect::<mlua::Result<Vec<_>>>()?;
        Ok(vec![Value::Table(lua.create_sequence_from(values)?)])
    };
    match name {
        "directory" => text(&take_directory(&path(0, "filepath")?)),
        "filename" => text(split_file_name(&path(0, "filepath")?).1),
        "is_absolute" => Ok(vec![Value::Boolean(path(0, "filepath")?.starts_with('/'))]),
        "is_relative" => Ok(vec![Value::Boolean(!path(0, "filepath")?.starts_with('/'))]),
        "join" => {
            let parts = call.required(0, "filepaths", |v| cx.list(v, |item| cx.text(item)))?;
            text(&join_path(&parts))
        }
        "make_relative" => {
            let target = path(0, "path")?;
            let root = path(1, "root")?;
            let unsafe_mode = call.argument(2);
            let unsafe_mode =
                !is_none_or_nil(&unsafe_mode) && !matches!(unsafe_mode, Value::Boolean(false));
            if unsafe_mode {
                text(&make_relative_unsafe(&target, &root))
            } else {
                text(&make_relative(&root, &target))
            }
        }
        "normalize" => text(&normalise(&path(0, "filepath")?)),
        "split" => list(split_directories(&path(0, "filepath")?)),
        "split_extension" => {
            let file = path(0, "filepath")?;
            let (base, extension) = split_extension(&file);
            Ok(vec![push::text(lua, base)?, push::text(lua, &extension)?])
        }
        "split_search_path" => {
            let search = call.required(0, "search_path", |v| cx.text(v))?;
            list(
                search
                    .split(':')
                    .map(|item| if item.is_empty() { "." } else { item }.to_string())
                    .collect(),
            )
        }
        "add_extension" => {
            let file = path(0, "filepath")?;
            let extension = call.required(1, "extension", |v| cx.text(v))?;
            text(&add_extension(&file, &extension))
        }
        "combine" => {
            let first = path(0, "filepath")?;
            let second = path(1, "filepath")?;
            text(&combine(&first, &second))
        }
        other => Err(PeekError::Thrown(format!(
            "internal error: unknown function {other}"
        ))),
    }
}

fn is_separator(c: char) -> bool {
    c == '/'
}

/// The drive of a POSIX path is its run of leading slashes.
fn split_drive(path: &str) -> (&str, &str) {
    let end = path.len() - path.trim_start_matches('/').len();
    path.split_at(end)
}

fn has_trailing_separator(path: &str) -> bool {
    path.ends_with('/')
}

fn drop_trailing_separator(path: &str) -> &str {
    let is_drive = !path.is_empty() && split_drive(path).1.is_empty();
    if has_trailing_separator(path) && !is_drive {
        let trimmed = path.trim_end_matches('/');
        if trimmed.is_empty() { "/" } else { trimmed }
    } else {
        path
    }
}

/// The directory part with its last separator, and the file name.
fn split_file_name_raw(path: &str) -> (&str, &str) {
    match path.rfind(is_separator) {
        Some(index) => path.split_at(index + 1),
        None => ("", path),
    }
}

fn split_file_name(path: &str) -> (&str, &str) {
    let (directory, file) = split_file_name_raw(path);
    (
        if directory.is_empty() {
            "./"
        } else {
            directory
        },
        file,
    )
}

fn take_directory(path: &str) -> String {
    drop_trailing_separator(split_file_name(path).0).to_string()
}

/// Each part with its separators, and the drive first.
fn split_path(path: &str) -> Vec<&str> {
    let (drive, mut rest) = split_drive(path);
    let mut parts = Vec::new();
    if !drive.is_empty() {
        parts.push(drive);
    }
    while !rest.is_empty() {
        let name = rest.find(is_separator).unwrap_or(rest.len());
        let after = rest.get(name..).unwrap_or_default();
        let end = name + (after.len() - after.trim_start_matches('/').len());
        let (part, next) = rest.split_at(end);
        parts.push(part);
        rest = next;
    }
    parts
}

fn split_directories(path: &str) -> Vec<String> {
    split_path(path)
        .into_iter()
        .map(|part| drop_trailing_separator(part).to_string())
        .collect()
}

fn combine_always(first: &str, second: &str) -> String {
    if first.is_empty() {
        second.to_string()
    } else if second.is_empty() {
        first.to_string()
    } else if has_trailing_separator(first) {
        format!("{first}{second}")
    } else {
        format!("{first}/{second}")
    }
}

fn combine(first: &str, second: &str) -> String {
    if second.starts_with('/') {
        second.to_string()
    } else {
        combine_always(first, second)
    }
}

/// `foldr combine ""` in linear time: the pieces are collected from the
/// end, and a part that starts with `/` drops each part before it.
fn join_path(parts: &[String]) -> String {
    let mut pieces: Vec<&str> = Vec::new();
    let mut rest_is_empty = true;
    for part in parts.iter().rev() {
        let rest_starts_with_slash = pieces.last().is_some_and(|piece| piece.starts_with('/'));
        if rest_starts_with_slash || part.is_empty() {
            continue;
        }
        if !rest_is_empty && !has_trailing_separator(part) {
            pieces.push("/");
        }
        pieces.push(part);
        rest_is_empty = false;
    }
    pieces.iter().rev().copied().collect()
}

fn normalise(path: &str) -> String {
    let (drive, rest) = split_drive(path);
    let parts: Vec<String> = split_directories(rest)
        .into_iter()
        .filter(|part| part != ".")
        .collect();
    let joined = join_path(&parts);
    let drive = if drive.is_empty() { "" } else { "/" };
    let result = if drive.is_empty() && joined.is_empty() {
        ".".to_string()
    } else {
        combine_always(drive, &joined)
    };
    let is_directory =
        has_trailing_separator(rest) || rest.strip_suffix('.').is_some_and(has_trailing_separator);
    if is_directory && !has_trailing_separator(&result) {
        result + "/"
    } else {
        result
    }
}

fn equal_file_path(first: &str, second: &str) -> bool {
    let first = normalise(first);
    let second = normalise(second);
    drop_trailing_separator(&first) == drop_trailing_separator(&second)
}

/// `makeRelative root path` of `filepath`.
fn make_relative(root: &str, path: &str) -> String {
    if equal_file_path(root, path) {
        return ".".to_string();
    }
    if split_drive(root).0 != split_drive(path).0 {
        return path.to_string();
    }
    // Slices of the inputs: each step copies only one directory name.
    fn next(x: &str) -> (&str, &str) {
        let x = x.trim_start_matches('/');
        let end = x.find(is_separator).unwrap_or(x.len());
        let (a, b) = x.split_at(end);
        (a, b.trim_start_matches('/'))
    }
    let mut x = split_drive(root).1;
    let mut y = split_drive(path).1;
    loop {
        if x.is_empty() {
            return y.trim_start_matches('/').to_string();
        }
        let (x1, x2) = next(x);
        let (y1, y2) = next(y);
        if !equal_file_path(x1, y1) {
            return path.to_string();
        }
        x = x2;
        y = y2;
    }
}

/// `makeRelative` of `hslua-module-path` with `unsafe` set: the result can
/// start with `..`.
fn make_relative_unsafe(path: &str, root: &str) -> String {
    if equal_file_path(root, path) {
        return ".".to_string();
    }
    if split_drive(root).0 != split_drive(path).0 {
        return path.to_string();
    }
    let path_parts = split_directories(&normalise(path));
    let root_parts = split_directories(&normalise(root));
    let common = path_parts
        .iter()
        .zip(&root_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts = vec!["..".to_string(); root_parts.len() - common];
    parts.extend(path_parts.into_iter().skip(common));
    join_path(&parts)
}

fn split_extension(path: &str) -> (&str, String) {
    match path.rfind('.') {
        Some(index) => {
            let extension = path.get(index + 1..).unwrap_or_default();
            if extension.contains(is_separator) {
                (path, String::new())
            } else {
                (
                    path.get(..index).unwrap_or_default(),
                    format!(".{extension}"),
                )
            }
        }
        None => (path, String::new()),
    }
}

fn add_extension(file: &str, extension: &str) -> String {
    if extension.is_empty() {
        return file.to_string();
    }
    let (drive, rest) = split_drive(file);
    let result = if extension.starts_with('.') {
        format!("{rest}{extension}")
    } else {
        format!("{rest}.{extension}")
    };
    combine_always(drive, &result)
}
