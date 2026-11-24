//! Document structure operations.

use std::rc::Rc;

use mlua::{Lua, ObjectLike, Table, UserData, UserDataFields, Value};
use randoc_ast::{Inline, Meta, MetaValue};
use randoc_core::shared::chunks::{ChunkOptions, ChunkedDoc, split_into_chunked_doc};
use randoc_core::shared::sections::{make_sections, prepare_slides};
use randoc_core::shared::toc::{TocEntry, TocOptions, toc_entries, toc_list};
use randoc_core::{Extensions, WriterOptions};

use crate::construct::Call;
use crate::object::{cache_of, ensure_cache, typed_fields};
use crate::peek::{Peek, PeekError, is_none, mismatch, retrieving, within};
use crate::push;
use crate::show::{show_blocks, show_inlines, show_text};
use crate::utils::{document_blocks, is_truthy};
use crate::walk::{check_blocks, check_document};

struct LuaChunkedDoc {
    doc: Rc<ChunkedDoc>,
    meta: Meta,
}

#[derive(Clone, Copy)]
enum View {
    Full,
    UpOnly,
    UpPrev,
}

struct LuaChunk {
    doc: Rc<ChunkedDoc>,
    index: usize,
    view: View,
}

impl UserData for LuaChunkedDoc {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        typed_fields(fields, "ChunkedDoc", &[]);
    }
}

impl UserData for LuaChunk {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        typed_fields(fields, "Chunk", &[]);
    }
}

pub(crate) fn call(call: &Call) -> Peek<Value> {
    match call.name {
        "make_sections" => sections(call),
        "slide_level" | "unique_identifier" => crate::utils::call(call, call.name),
        "split_into_chunks" => split(call),
        "table_of_contents" => table_of_contents(call),
        "get" | "set" | "tostring" => property(call),
        _ => Err(crate::host::missing("pandoc.structure", call.name)),
    }
}

fn core_error(error: randoc_core::Error) -> PeekError {
    PeekError::Thrown(error.to_string())
}

fn sections(call: &Call) -> Peek<Value> {
    let cx = call.cx;
    let mut blocks = call.required(0, "blocks", |v| document_blocks(cx, v))?;
    let options = call.argument(1);
    let field = |name: &str| -> Peek<Value> {
        match &options {
            Value::Table(table) => Ok(table.get::<Value>(name)?),
            Value::UserData(userdata) => Ok(userdata.get::<Value>(name)?),
            value if is_none(value) || matches!(value, Value::Nil | Value::String(_)) => {
                Ok(Value::Nil)
            }
            other => Err(PeekError::Thrown(format!(
                "attempt to index a {} value",
                crate::peek::lua_type_name(other)
            ))),
        }
    };
    let number = is_truthy(&field("number_sections")?);
    let base = cx.integral(&field("base_level")?).ok();
    let slide = field("slide_level")?;
    if let Ok(level) = cx.integral(&slide) {
        let level = if level < 1 { 6 } else { level };
        blocks = prepare_slides(blocks, level, cx.max_depth).map_err(core_error)?;
    }
    let sections = make_sections(blocks, number, base, cx.max_depth).map_err(core_error)?;
    check_blocks(cx, &sections)?;
    Ok(push::blocks(cx.lua, &sections)?)
}

fn split(call: &Call) -> Peek<Value> {
    let cx = call.cx;
    let doc = call.required(0, "doc", |v| cx.pandoc(v))?;
    check_document(cx, &doc)?;
    let defaults = ChunkOptions::default();
    let mut path = defaults.path_template.to_owned();
    let mut options = defaults;
    if let Some((level, template, number, base)) = call.optional(1, "opts", |v| {
        let Value::Table(table) = v else {
            return Err(mismatch("table", v).thrown());
        };
        let field = |name| table.raw_get::<Value>(name).map_err(PeekError::from);
        let template = field("path_template")?;
        let template = if matches!(template, Value::Nil) {
            None
        } else {
            within(
                || "retrieving raw field 'path_template'".into(),
                cx.text(&template),
            )
            .map(Some)?
        };
        let level = field("chunk_level")?;
        let level = if matches!(level, Value::Nil) {
            defaults.level
        } else {
            within(
                || "retrieving raw field 'chunk_level'".into(),
                cx.integral(&level),
            )?
        };
        Ok((
            level,
            template,
            is_truthy(&field("number_sections")?),
            cx.integral(&field("base_heading_level")?).ok(),
        ))
    })? {
        if let Some(template) = template {
            path = template;
        }
        options = ChunkOptions {
            level,
            path_template: &path,
            number_sections: number,
            base_heading_level: base,
        };
    }
    let mut chunked = split_into_chunked_doc(&doc, &options, cx.max_depth).map_err(core_error)?;
    if let Some(intro) = chunked.chunks.first_mut().filter(|chunk| chunk.level == 0)
        && let Some(title) = doc.meta.0.get("title")
    {
        let title = match title {
            MetaValue::MetaString(text) => vec![Inline::Str(text.clone())],
            MetaValue::MetaInlines(inlines) => inlines.clone(),
            MetaValue::MetaBlocks(blocks) => {
                crate::utils::blocks_to_inlines(blocks, &[Inline::Space])
            }
            _ => Vec::new(),
        };
        if !title.is_empty() {
            let id = randoc_core::shared::auto_identifier(&title, Extensions::empty());
            intro.chunk.id = format!("{id}-1");
            let heading = randoc_core::shared::stringify(&title);
            let mut parts = options.path_template.split('%');
            let mut path = parts.next().unwrap_or_default().to_owned();
            for part in parts {
                match part.as_bytes().first() {
                    Some(b'n') => path.push_str("001"),
                    Some(b's') => path.push('0'),
                    Some(b'i') => path.push_str(&intro.chunk.id),
                    Some(b'h') => path.push_str(&heading),
                    _ => {
                        path.push('%');
                        path.push_str(part);
                        continue;
                    }
                }
                path.push_str(&part[1..]);
            }
            intro.chunk.path = path;
            intro.chunk.title = title;
        }
    }
    Ok(Value::UserData(cx.lua.create_userdata(LuaChunkedDoc {
        doc: Rc::new(chunked),
        meta: doc.meta,
    })?))
}

fn table_of_contents(call: &Call) -> Peek<Value> {
    let cx = call.cx;
    let source = call.argument(0);
    let chunked = match &source {
        Value::UserData(data) if data.is::<LuaChunkedDoc>() => {
            Some(data.borrow::<LuaChunkedDoc>()?)
        }
        _ => None,
    };
    let blocks = if chunked.is_none() {
        call.required(0, "toc_source", |value| {
            document_blocks(cx, value).map_err(|error| match error {
                PeekError::Failure { .. } => mismatch("ChunkedDoc", value),
                thrown => thrown,
            })
        })?
    } else {
        Vec::new()
    };
    let defaults = WriterOptions::default();
    let options = call
        .optional(1, "opts", |v| {
            let result = crate::options::writer_options(cx, v);
            if matches!(v, Value::Table(_)) {
                result.map_err(PeekError::thrown)
            } else {
                retrieving("WriterOptions", result)
            }
        })?
        .unwrap_or(defaults);
    let filter = TocOptions {
        depth: options.toc_depth,
        number_sections: options.number_sections,
    };
    let list = if let Some(chunked) = chunked {
        toc_list(&chunked.doc.toc, filter)
    } else {
        let sections = make_sections(blocks, options.number_sections, None, cx.max_depth)
            .map_err(core_error)?;
        let entries =
            toc_entries(&sections, &|id| format!("#{id}"), cx.max_depth).map_err(core_error)?;
        toc_list(&entries, filter)
    };
    check_blocks(cx, std::slice::from_ref(&list))?;
    Ok(push::block(cx.lua, &list)?)
}

fn chunk(lua: &Lua, doc: &Rc<ChunkedDoc>, index: Option<usize>, view: View) -> mlua::Result<Value> {
    let Some(index) = index.filter(|&i| i < doc.chunks.len()) else {
        return Ok(Value::Nil);
    };
    Ok(Value::UserData(lua.create_userdata(LuaChunk {
        doc: Rc::clone(doc),
        index,
        view,
    })?))
}

fn toc_tree(lua: &Lua, entry: Option<&TocEntry>, children: &[TocEntry]) -> mlua::Result<Table> {
    let tree = lua.create_table_with_capacity(children.len(), 1)?;
    let fields = lua.create_table()?;
    if let Some(entry) = entry {
        fields.raw_set("id", entry.id.as_str())?;
        fields.raw_set("level", entry.level)?;
        fields.raw_set("number", entry.number.as_deref())?;
        fields.raw_set("path", entry.target.as_str())?;
        fields.raw_set("title", push::inlines(lua, &entry.title)?)?;
    } else {
        fields.raw_set("id", "")?;
        fields.raw_set("level", 0)?;
        fields.raw_set("path", "#")?;
        fields.raw_set("title", push::inlines(lua, &[])?)?;
    }
    tree.raw_set(0, fields)?;
    for (i, child) in children.iter().enumerate() {
        tree.raw_set(i + 1, toc_tree(lua, Some(child), &child.children)?)?;
    }
    Ok(tree)
}

const DOC_FIELDS: &[&str] = &["chunks", "meta", "toc"];
const CHUNK_FIELDS: &[&str] = &[
    "contents",
    "heading",
    "id",
    "level",
    "next",
    "number",
    "path",
    "prev",
    "section_number",
    "unlisted",
    "up",
];

fn property(call: &Call) -> Peek<Value> {
    let lua = call.cx.lua;
    let value = call.argument(0);
    let Value::UserData(data) = &value else {
        return Err(mismatch("ChunkedDoc or Chunk", &value));
    };
    let (type_name, fields) = if data.is::<LuaChunkedDoc>() {
        ("ChunkedDoc", DOC_FIELDS)
    } else if data.is::<LuaChunk>() {
        ("Chunk", CHUNK_FIELDS)
    } else {
        return Err(mismatch("ChunkedDoc or Chunk", &value));
    };
    if call.name == "tostring" {
        let text = if let Ok(chunk) = data.borrow::<LuaChunk>() {
            chunk_string(&chunk.doc, chunk.index, chunk.view, call.cx.max_depth).unwrap_or_default()
        } else {
            format!("{type_name}: {:p}", data.to_pointer())
        };
        return Ok(push::text(lua, &text)?);
    }
    let key = call.argument(1);
    let name = match &key {
        Value::String(name) => {
            let name = name.as_bytes();
            fields
                .iter()
                .copied()
                .find(|field| field.as_bytes() == &*name)
        }
        _ => None,
    };
    let Some(name) = name else {
        return if call.name == "get" {
            Ok(Value::Nil)
        } else {
            Err(PeekError::Thrown("Cannot set unknown property.".into()))
        };
    };
    if call.name == "set" {
        if data.is::<LuaChunkedDoc>() {
            return Err(PeekError::Thrown(format!(
                "'{name}' is a read-only property."
            )));
        }
        ensure_cache(lua, data)?.raw_set(key, call.argument(2))?;
        return Ok(Value::Nil);
    }
    if let Some(cache) = cache_of(data) {
        let cached = cache.raw_get::<Value>(key.clone())?;
        if !matches!(cached, Value::Nil) {
            return Ok(cached);
        }
    }
    let value = if let Ok(doc) = data.borrow::<LuaChunkedDoc>() {
        doc_field(lua, &doc, name)?
    } else {
        let view = data.borrow::<LuaChunk>()?;
        chunk_field(lua, &view, name)?
    };
    if !matches!(value, Value::Nil) {
        ensure_cache(lua, data)?.raw_set(key, value.clone())?;
    }
    Ok(value)
}

fn doc_field(lua: &Lua, value: &LuaChunkedDoc, name: &str) -> mlua::Result<Value> {
    match name {
        "chunks" => {
            let chunks = lua.create_table_with_capacity(value.doc.chunks.len(), 0)?;
            for i in 0..value.doc.chunks.len() {
                chunks.raw_set(i + 1, chunk(lua, &value.doc, Some(i), View::Full)?)?;
            }
            Ok(Value::Table(chunks))
        }
        "meta" => push::meta(lua, &value.meta),
        "toc" => Ok(Value::Table(toc_tree(lua, None, &value.doc.toc)?)),
        _ => Ok(Value::Nil),
    }
}

fn chunk_field(lua: &Lua, value: &LuaChunk, name: &str) -> mlua::Result<Value> {
    let doc = &value.doc;
    let index = value.index;
    let Some(entry) = doc.chunks.get(index) else {
        return Ok(Value::Nil);
    };
    match name {
        "heading" => push::inlines(lua, &entry.title),
        "id" => push::text(lua, &entry.id),
        "level" => Ok(Value::Integer(entry.level)),
        "number" => Ok(Value::Integer(i64::try_from(index + 1).unwrap_or(i64::MAX))),
        "section_number" => entry
            .number
            .as_deref()
            .map_or(Ok(Value::Nil), |n| push::text(lua, n)),
        "path" => push::text(lua, &entry.path),
        "unlisted" => Ok(Value::Boolean(entry.unlisted)),
        "contents" => push::blocks(lua, &entry.blocks),
        "up" => chunk(lua, doc, entry.up, View::UpOnly),
        "prev" if matches!(value.view, View::Full | View::UpPrev) => {
            chunk(lua, doc, index.checked_sub(1), View::UpOnly)
        }
        "next" if matches!(value.view, View::Full) => {
            chunk(lua, doc, index.checked_add(1), View::UpPrev)
        }
        _ => Ok(Value::Nil),
    }
}

fn chunk_string(doc: &ChunkedDoc, index: usize, view: View, depth: usize) -> Option<String> {
    let entry = doc.chunks.get(index)?;
    let quote = |value: &str| show_text(value, depth);
    let link = |index: Option<usize>, view| {
        index
            .and_then(|index| chunk_string(doc, index, view, depth.checked_sub(1)?))
            .map_or_else(|| "Nothing".to_owned(), |value| format!("Just ({value})"))
    };
    let up = link(entry.up, View::UpOnly);
    let prev = link(
        index
            .checked_sub(1)
            .filter(|_| !matches!(view, View::UpOnly)),
        View::UpOnly,
    );
    let next = link(
        index.checked_add(1).filter(|_| matches!(view, View::Full)),
        View::UpPrev,
    );
    let section = entry.number.as_deref().map_or_else(
        || "Nothing".to_owned(),
        |value| format!("Just {}", quote(value)),
    );
    Some(format!(
        "Chunk {{chunkHeading = {}, chunkId = {}, chunkLevel = {}, chunkNumber = {}, chunkSectionNumber = {section}, chunkPath = {}, chunkUp = {up}, chunkPrev = {prev}, chunkNext = {next}, chunkUnlisted = {}, chunkContents = {}}}",
        show_inlines(&entry.title, depth),
        quote(&entry.id),
        entry.level,
        index + 1,
        quote(&entry.path),
        if entry.unlisted { "True" } else { "False" },
        show_blocks(&entry.blocks, depth),
    ))
}
