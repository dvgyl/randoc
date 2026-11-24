//! ZIP archive operations for Lua.

use std::path::{Component, Path};

use mlua::{AnyUserData, Lua, Table, UserData, UserDataFields, Value};
use randoc_archive::{
    media::is_safe_media_path,
    zip::{Archive, ArchiveLimits, ArchiveWriter},
};
use randoc_core::{Context, PathKind, error::io_error};

use crate::construct::Call;
use crate::host::Host;
use crate::mediabag::bytes;
use crate::object::ensure_cache;
use crate::peek::{Cx, Peek, PeekError, all_choices, is_none_or_nil, mismatch};
use crate::push;

const ARCHIVE: &str = "zip.Archive";
const ENTRY: &str = "zip.Entry";
const ENTRIES: &str = "ZipEntry list";

struct LuaArchive(Vec<EntryContents>);

struct LuaEntry(EntryContents);

impl UserData for LuaArchive {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        crate::object::typed_fields(fields, ARCHIVE, &[]);
    }
}

impl UserData for LuaEntry {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        crate::object::typed_fields(fields, ENTRY, &[]);
    }
}

fn thrown(error: impl std::fmt::Display) -> PeekError {
    PeekError::Thrown(error.to_string())
}

fn typed<T: UserData + 'static>(value: &Value, name: &str) -> Peek<AnyUserData> {
    match value {
        Value::UserData(data) if data.is::<T>() => Ok(data.clone()),
        _ => Err(mismatch(name, value)),
    }
}

fn new_entry(lua: &Lua, path: String, contents: mlua::LuaString, modtime: i64) -> Peek<Value> {
    Ok(Value::UserData(lua.create_userdata(LuaEntry(
        EntryContents {
            path,
            contents,
            modtime,
        },
    ))?))
}

fn new_archive(lua: &Lua, entries: Vec<EntryContents>) -> Peek<Value> {
    Ok(Value::UserData(lua.create_userdata(LuaArchive(entries))?))
}

fn archive_entries(lua: &Lua, data: &AnyUserData) -> Peek<Value> {
    let fields = ensure_cache(lua, data)?;
    let cached = fields.raw_get::<Value>("entries")?;
    if !matches!(cached, Value::Nil) {
        return Ok(cached);
    }
    let archive = data.borrow::<LuaArchive>()?;
    let values = archive
        .0
        .iter()
        .map(|entry| {
            new_entry(
                lua,
                entry.path.clone(),
                entry.contents.clone(),
                entry.modtime,
            )
        })
        .collect::<Peek<Vec<_>>>()?;
    let value = push::list_of(lua, ENTRIES, values)?;
    fields.raw_set("entries", value.clone())?;
    Ok(value)
}

fn from_bytes(lua: &Lua, bytes: &[u8], limits: ArchiveLimits) -> Peek<Vec<EntryContents>> {
    let mut archive = Archive::open(bytes, limits).map_err(thrown)?;
    let paths = archive
        .entry_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if paths.len() != archive.len() {
        return Err(thrown("ZIP entry name is not valid UTF-8"));
    }
    let mut entries = Vec::with_capacity(paths.len());
    for (index, path) in paths.into_iter().enumerate() {
        let time = archive.entry_modified(index).unwrap_or_default();
        let contents = archive.read_index(index).map_err(thrown)?;
        entries.push(EntryContents {
            path,
            contents: lua.create_string(contents)?,
            modtime: time,
        });
    }
    Ok(entries)
}

fn archive(host: &Host, call: &Call) -> Peek<Value> {
    let lua = call.cx.lua;
    let value = call.argument(0);
    let entries = if is_none_or_nil(&value) {
        vec![]
    } else if let Ok(bytes) = bytes(call.cx, &value) {
        let limits = host.context(|context| Ok(ArchiveLimits::from_limits(&context.limits)))?;
        from_bytes(lua, &bytes.as_bytes(), limits)?
    } else {
        call.required(0, "bytestring_or_entries", |v| {
            all_choices(call.cx.list(v, |entry| read_entry_value(call.cx, entry)))
        })?
    };
    new_archive(lua, entries)
}

fn entry(host: &Host, call: &Call) -> Peek<Value> {
    let cx = call.cx;
    let path = call.required(0, "path", |v| cx.text(v))?;
    let contents = call.required(1, "contents", |v| bytes(cx, v))?;
    let time = match call.optional(2, "modtime", |v| cx.integral(v))? {
        Some(time) => time,
        None => host.system(|context| Ok(context.host().current_time().unix_seconds))?,
    };
    new_entry(cx.lua, path, contents, time)
}

struct EntryContents {
    path: String,
    contents: mlua::LuaString,
    modtime: i64,
}

fn entry_field(cx: &Cx, data: &AnyUserData, key: &str) -> Peek<Value> {
    let cached = ensure_cache(cx.lua, data)?.raw_get::<Value>(key)?;
    if !matches!(cached, Value::Nil) {
        return Ok(cached);
    }
    let entry = data.borrow::<LuaEntry>()?;
    Ok(match key {
        "path" => push::text(cx.lua, &entry.0.path)?,
        "modtime" => Value::Integer(entry.0.modtime),
        _ => Value::Nil,
    })
}

fn read_entry_value(cx: &Cx, value: &Value) -> Peek<EntryContents> {
    let data = typed::<LuaEntry>(value, ENTRY)?;
    let path = cx.text(&entry_field(cx, &data, "path")?)?;
    let modtime = cx.integral(&entry_field(cx, &data, "modtime")?)?;
    Ok(EntryContents {
        path,
        modtime,
        contents: data.borrow::<LuaEntry>()?.0.contents.clone(),
    })
}

fn entries(cx: &Cx, archive: &Value) -> Peek<Vec<EntryContents>> {
    let data = typed::<LuaArchive>(archive, ARCHIVE)?;
    cx.list(&archive_entries(cx.lua, &data)?, |item| {
        read_entry_value(cx, item)
    })
}

fn bytestring(host: &Host, call: &Call) -> Peek<Value> {
    let entries = entries(call.cx, &call.argument(0))?;
    let (limit, max_entries) = host.context(|context| {
        Ok((
            context.limits.max_archive_size,
            context.limits.max_archive_entries,
        ))
    })?;
    if entries.len() > max_entries {
        return Err(thrown("ZIP archive exceeds the entry limit"));
    }
    let mut writer = ArchiveWriter::with_input_limit(limit);
    for entry in entries {
        writer
            .write_smallest_with_time(&entry.path, &entry.contents.as_bytes(), Some(entry.modtime))
            .map_err(thrown)?;
    }
    let bytes = writer.finish().map_err(thrown)?;
    if bytes.len() as u64 > limit {
        return Err(thrown("ZIP archive exceeds the size limit"));
    }
    Ok(Value::String(call.cx.lua.create_string(bytes)?))
}

fn options(call: &Call, index: usize) -> Peek<Option<Table>> {
    let value = call.argument(index);
    match value {
        Value::Table(table) => Ok(Some(table)),
        v if is_none_or_nil(&v) => Ok(None),
        other => Err(mismatch("table", &other).thrown()),
    }
}

fn option(opts: Option<&Table>, key: &str) -> Peek<Value> {
    Ok(match opts {
        Some(opts) => opts.raw_get(key)?,
        None => Value::Nil,
    })
}

fn option_text(cx: &Cx, opts: Option<&Table>, key: &str, default: &str) -> Peek<String> {
    let value = option(opts, key)?;
    if matches!(value, Value::Nil) {
        Ok(default.into())
    } else {
        cx.text(&value).map_err(PeekError::thrown)
    }
}

fn make_directory(context: &mut Context, destination: &str, relative: &str) -> Peek<()> {
    let mut target = Path::new(destination).to_path_buf();
    for component in relative.split('/') {
        target.push(component);
        let name = target.to_string_lossy();
        let kind = context.host().path_kind(&name, false).map_err(thrown)?;
        match kind {
            PathKind::Directory => {}
            PathKind::Missing => context
                .host()
                .create_directory(&name, true)
                .map_err(thrown)?,
            _ => return Err(thrown(format!("UnsafePath {relative:?}"))),
        }
    }
    Ok(())
}

fn extract(host: &Host, call: &Call) -> Peek<Vec<Value>> {
    host.require_full("pandoc.zip.Archive.extract")?;
    let opts = options(call, 1)?;
    let destination = option_text(call.cx, opts.as_ref(), "destination", ".")?;
    let entries = entries(call.cx, &call.argument(0))?;
    for entry in &entries {
        let path = entry.path.strip_suffix('/').unwrap_or(&entry.path);
        if !is_safe_media_path(path) {
            return Err(thrown(format!("UnsafePath {:?}", entry.path)));
        }
    }
    host.system(|context| {
        for entry in entries {
            if let Some(path) = entry.path.strip_suffix('/') {
                make_directory(context, &destination, path)?;
            } else {
                context
                    .host()
                    .write_media_file(&destination, &entry.path, &entry.contents.as_bytes())
                    .map_err(thrown)?;
            }
        }
        Ok(vec![])
    })
}

struct FileEntry {
    path: String,
    bytes: Vec<u8>,
    time: i64,
    directory: bool,
}

fn read_file(context: &mut Context, source: &str, location: &str) -> Peek<FileEntry> {
    let kind = context.host().path_kind(source, false).map_err(thrown)?;
    if kind == PathKind::Missing {
        return Err(thrown(format!(
            "{source}: getSymbolicLinkStatus: does not exist (No such file or directory)"
        )));
    }
    let directory = kind == PathKind::Directory;
    let time = context
        .host()
        .path_times(source)
        .map_err(thrown)?
        .modified
        .unix_seconds;
    let mut path = Path::new(source)
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if !location.is_empty() && !Path::new(source).is_absolute() {
        path = format!("{}/{path}", location.trim_end_matches('/'));
    }
    if directory && !path.ends_with('/') {
        path.push('/');
    }
    let bytes = if directory {
        vec![]
    } else {
        let limit = usize::try_from(context.limits.max_resource_size).unwrap_or(usize::MAX);
        context
            .host()
            .read_file_limited(source, limit)
            .map_err(|error| thrown(io_error(source, "withBinaryFile", &error)))?
    };
    Ok(FileEntry {
        path,
        bytes,
        time,
        directory,
    })
}

fn read_paths(host: &Host, call: &Call) -> Peek<Value> {
    let cx = call.cx;
    let zip = call.name == "zip";
    host.require_full(if zip {
        "pandoc.zip.zip"
    } else {
        "pandoc.zip.read_entry"
    })?;
    let paths = if zip {
        call.required(0, "filepaths", |v| cx.list(v, |v| cx.text(v)))?
    } else {
        vec![call.required(0, "filepath", |v| cx.text(v))?]
    };
    let opts = options(call, 1)?;
    let location = option_text(cx, opts.as_ref(), "location", "")?;
    let recursive = !matches!(
        option(opts.as_ref(), "recursive")?,
        Value::Nil | Value::Boolean(false)
    );
    let verbose = !matches!(
        option(opts.as_ref(), "verbose")?,
        Value::Nil | Value::Boolean(false)
    );
    let files = host.system(|context| {
        let mut files = Vec::new();
        let mut pending: Vec<_> = paths.into_iter().rev().collect();
        let mut remaining = context.limits.max_archive_size;
        while let Some(path) = pending.pop() {
            if files.len() >= context.limits.max_archive_entries {
                return Err(thrown("ZIP archive exceeds the entry limit"));
            }
            let file = read_file(context, &path, &location)?;
            remaining = remaining
                .checked_sub(file.bytes.len() as u64)
                .ok_or_else(|| thrown("ZIP archive exceeds the size limit"))?;
            if zip && recursive && file.directory {
                let children = context.host().list_directory(&path).map_err(thrown)?;
                if children
                    .len()
                    .saturating_add(pending.len())
                    .saturating_add(files.len())
                    .saturating_add(1)
                    > context.limits.max_archive_entries
                {
                    return Err(thrown("ZIP archive exceeds the entry limit"));
                }
                pending.extend(
                    children
                        .into_iter()
                        .rev()
                        .filter(|child| !child.starts_with("._"))
                        .map(|child| Path::new(&path).join(child).to_string_lossy().into_owned()),
                );
            }
            files.push(file);
        }
        Ok(files)
    })?;
    let mut entries = Vec::with_capacity(files.len());
    for file in files {
        if zip && verbose {
            show_entry(&file)?;
        }
        entries.push(EntryContents {
            path: file.path,
            contents: cx.lua.create_string(file.bytes)?,
            modtime: file.time,
        });
    }
    if zip {
        new_archive(cx.lua, entries)
    } else {
        let file = entries
            .pop()
            .ok_or_else(|| thrown("ZIP entry is missing"))?;
        new_entry(cx.lua, file.path, file.contents, file.modtime)
    }
}

fn show_entry(file: &FileEntry) -> Peek<()> {
    use std::io::Write;

    let size = file.bytes.len() as u64;
    let mut writer = ArchiveWriter::with_input_limit(size);
    writer
        .write_smallest_with_time(&file.path, &file.bytes, Some(file.time))
        .map_err(thrown)?;
    let archive = writer.finish().map_err(thrown)?;
    // The local ZIP header stores the method and compressed size.
    let method = archive.get(8..10);
    let compressed = archive
        .get(18..22)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .map(u64::from)
        .unwrap_or(size);
    let saved = size.saturating_sub(compressed);
    let percent = saved
        .saturating_mul(100)
        .saturating_add(size / 2)
        .checked_div(size)
        .unwrap_or_default();
    let method = if method == Some(&[8, 0]) {
        "deflated"
    } else {
        "stored"
    };
    writeln!(
        std::io::stdout().lock(),
        "  adding: {} ({method} {percent}%)",
        file.path
    )
    .map_err(thrown)
}

fn property(call: &Call) -> Peek<Vec<Value>> {
    let value = call.argument(0);
    let Value::UserData(data) = &value else {
        return Err(mismatch(ENTRY, &value));
    };
    let archive = data.is::<LuaArchive>();
    let key = call.argument(1);
    let known = match &key {
        Value::String(key) if archive => key.as_bytes() == b"entries",
        Value::String(key) => matches!(&*key.as_bytes(), b"path" | b"modtime"),
        _ => false,
    };
    match call.name {
        "get" if known && archive => Ok(vec![archive_entries(call.cx.lua, data)?]),
        "get" if known => Ok(vec![entry_field(call.cx, data, &call.cx.text(&key)?)?]),
        "get" => Ok(vec![Value::Nil]),
        "set" if known => {
            ensure_cache(call.cx.lua, data)?.raw_set(key, call.argument(2))?;
            Ok(vec![])
        }
        "set" => Err(thrown("Cannot set unknown property.")),
        _ => {
            let name = if archive { ARCHIVE } else { ENTRY };
            Ok(vec![push::text(
                call.cx.lua,
                &format!("{name}: {:p}", data.to_pointer()),
            )?])
        }
    }
}

pub(crate) fn call(host: &Host, call: &Call) -> Peek<Vec<Value>> {
    Ok(vec![match call.name {
        "Archive" => archive(host, call)?,
        "Entry" => entry(host, call)?,
        "read_entry" | "zip" => read_paths(host, call)?,
        "bytestring" => bytestring(host, call)?,
        "extract" => return extract(host, call),
        "get" | "set" | "tostring" => return property(call),
        "contents" => {
            let data = typed::<LuaEntry>(&call.argument(0), ENTRY)?;
            call.optional(1, "password", |v| bytes(call.cx, v))?;
            Value::String(data.borrow::<LuaEntry>()?.0.contents.clone())
        }
        "symlink" => Value::Nil,
        other => return Err(crate::host::missing("pandoc.zip", other)),
    }])
}
