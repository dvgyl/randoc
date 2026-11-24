//! `pandoc.read`, `pandoc.write`, `pandoc.readers` and `pandoc.writers`.
//!
//! The crate does not know the readers and writers of the program: the
//! program gives two lookup functions in [`crate::FilterEnvironment`]
//! (decision b of the coordinator), so `randoc-lua` does not depend on the
//! crate `randoc`.

use mlua::{Lua, Table, Value};
use randoc_ast::Text;
use randoc_core::{
    Context, Error, Extension, Extensions, FormatSpec, PureHost, Reader, Source, Sources, Writer,
    default_extensions, supported_extensions,
};

use crate::runtime::Mode;

use crate::construct::Call;
use crate::options::{reader_options, writer_options};
use crate::peek::{Cx, LuaText, Peek, PeekError, described_type, mismatch, retrieving, within};
use crate::push;

/// Returns the reader for a format name without extension changes, as
/// `crates/randoc/src/formats.rs` has it.
pub type ReaderLookup = fn(&str) -> Option<Reader>;

/// Returns the writer for a format name without extension changes.
pub type WriterLookup = fn(&str) -> Option<Writer>;

/// The lookup of an environment that knows no reader.
pub(crate) fn no_reader(_: &str) -> Option<Reader> {
    None
}

/// The lookup of an environment that knows no writer.
pub(crate) fn no_writer(_: &str) -> Option<Writer> {
    None
}

/// The keys of `pandoc.readers` in Pandoc 3.13 (probed).
const READER_NAMES: [&str; 52] = [
    "asciidoc",
    "biblatex",
    "bibtex",
    "bits",
    "commonmark",
    "commonmark_x",
    "creole",
    "csljson",
    "csv",
    "djot",
    "docbook",
    "docx",
    "dokuwiki",
    "endnotexml",
    "epub",
    "fb2",
    "fodt",
    "gfm",
    "haddock",
    "html",
    "ipynb",
    "jats",
    "jira",
    "json",
    "latex",
    "man",
    "markdown",
    "markdown_github",
    "markdown_mmd",
    "markdown_phpextra",
    "markdown_strict",
    "mdoc",
    "mediawiki",
    "muse",
    "native",
    "odt",
    "opml",
    "org",
    "pod",
    "pptx",
    "ris",
    "rst",
    "rtf",
    "t2t",
    "textile",
    "tikiwiki",
    "tsv",
    "twiki",
    "typst",
    "vimwiki",
    "xlsx",
    "xml",
];

/// The keys of `pandoc.writers` in Pandoc 3.13 (probed).
const WRITER_NAMES: [&str; 76] = [
    "ansi",
    "asciidoc",
    "asciidoc_legacy",
    "asciidoctor",
    "bbcode",
    "bbcode_fluxbb",
    "bbcode_hubzilla",
    "bbcode_phpbb",
    "bbcode_steam",
    "bbcode_xenforo",
    "beamer",
    "biblatex",
    "bibtex",
    "chunkedhtml",
    "commonmark",
    "commonmark_x",
    "context",
    "csljson",
    "djot",
    "docbook4",
    "docbook5",
    "docbook",
    "docx",
    "dokuwiki",
    "dzslides",
    "epub2",
    "epub3",
    "epub",
    "fb2",
    "fodt",
    "gfm",
    "haddock",
    "html4",
    "html5",
    "html",
    "icml",
    "ipynb",
    "jats",
    "jats_archiving",
    "jats_articleauthoring",
    "jats_publishing",
    "jira",
    "json",
    "latex",
    "man",
    "markdown",
    "markdown_github",
    "markdown_mmd",
    "markdown_phpextra",
    "markdown_strict",
    "markua",
    "mediawiki",
    "ms",
    "muse",
    "native",
    "odt",
    "opendocument",
    "opml",
    "org",
    "plain",
    "pptx",
    "revealjs",
    "rst",
    "rtf",
    "s5",
    "slideous",
    "slidy",
    "t2t",
    "tei",
    "texinfo",
    "textile",
    "typst",
    "vimdoc",
    "xml",
    "xwiki",
    "zimwiki",
];

/// Runs `convert` with a context that has no system access, for
/// restricted mode (ADR 11): a reader or writer that reads a file, fetches
/// a URL or starts a process gets an error, and the host of the
/// conversion is never called. The messages and the media go to the
/// context of the conversion.
pub(crate) fn without_system_access<T>(
    mode: Mode,
    context: &mut Context,
    convert: impl FnOnce(&mut Context) -> T,
) -> T {
    if mode == Mode::Full {
        return convert(context);
    }
    let mut inner = Context::new(Box::new(PureHost {
        time: context.host().current_time(),
        ..PureHost::default()
    }));
    inner.state = context.state.clone();
    inner.limits = context.limits;
    inner.media = std::mem::take(&mut context.media);
    let result = convert(&mut inner);
    context.media = std::mem::take(&mut inner.media);
    for message in inner.log() {
        context.report(message.clone());
    }
    result
}

/// The default format of both functions in Pandoc 3.13 (probed; the
/// manual says `html` for `pandoc.write`).
const DEFAULT_FORMAT: &str = "markdown";

/// A format name and the extensions that the reader or writer gets.
struct Flavored {
    name: Text,
    extensions: Extensions,
}

/// Returns `pandoc.readers`: each name of Pandoc that `lookup` knows, with
/// the value `text` or `bytestring`.
pub(crate) fn readers_table(lua: &Lua, lookup: ReaderLookup) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for name in READER_NAMES {
        match lookup(name) {
            Some(Reader::Text(_)) => table.set(name, "text")?,
            Some(Reader::Binary(_)) => table.set(name, "bytestring")?,
            None => {}
        }
    }
    Ok(table)
}

/// Returns `pandoc.writers`, as [`readers_table`] does for the readers.
pub(crate) fn writers_table(lua: &Lua, lookup: WriterLookup) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for name in WRITER_NAMES {
        match lookup(name) {
            Some(Writer::Text(_)) => table.set(name, "text")?,
            Some(Writer::Binary(_)) => table.set(name, "bytestring")?,
            None => {}
        }
    }
    Ok(table)
}

/// `pandoc.read (markup[, format[, reader_options[, read_env]]])`.
pub(crate) fn read(call: &Call, lookup: ReaderLookup, context: &mut Context) -> Peek<Value> {
    let cx = call.cx;
    let markup = markup(call, context.limits.max_input_size)?;
    let format = call
        .optional(1, "formatspec", |value| flavored(cx, value))?
        .map_or_else(|| from_spec(DEFAULT_FORMAT), Ok)?;
    let mut options = call
        .optional(2, "reader_options", |value| {
            retrieving("ReaderOptions", reader_options(cx, value))
        })?
        .unwrap_or_default();
    options.extensions = format.extensions;
    options.ipynb_output = randoc_core::IpynbOutput::Best;
    options.output_format = None;
    call.optional(3, "read_env", |value| read_env(cx, value))?;
    let Some(reader) = lookup(&format.name) else {
        return Err(thrown(&Error::UnknownReader(format.name)));
    };
    let doc = match reader {
        Reader::Text(reader) => {
            let sources = if let Value::Table(table) = call.argument(0)
                && crate::peek::metatable_name(&table).as_deref() == Some("Sources")
            {
                cx.list(&Value::Table(table), |value| {
                    crate::source::read(value)?.ok_or_else(|| mismatch("Source", value))
                })?
            } else {
                vec![Source {
                    name: Text::new(),
                    text: utf8(markup.concat())?,
                }]
            };
            reader.read(&Sources(sources), &options, context)
        }
        Reader::Binary(reader) => reader.read(&markup.concat(), &options, context),
    }
    .map_err(|error| thrown(&error))?;
    Ok(push::pandoc(cx.lua, &doc)?)
}

/// `pandoc.write (doc[, format[, writer_options]])`.
pub(crate) fn write(call: &Call, lookup: WriterLookup, context: &mut Context) -> Peek<Value> {
    let cx = call.cx;
    let doc = call.required(0, "doc", |value| cx.pandoc(value))?;
    let format = call
        .optional(1, "formatspec", |value| flavored(cx, value))?
        .map_or_else(|| from_spec(DEFAULT_FORMAT), Ok)?;
    let mut options = call
        .optional(2, "writer_options", |value| {
            retrieving("WriterOptions", writer_options(cx, value))
        })?
        .unwrap_or_default();
    options.extensions = format.extensions;
    let Some(writer) = lookup(&format.name) else {
        return Err(thrown(&Error::UnknownWriter(format.name)));
    };
    let bytes = match writer {
        Writer::Text(writer) => writer
            .write(&doc, &options, context)
            .map(String::into_bytes),
        Writer::Binary(writer) => writer.write(&doc, &options, context),
    }
    .map_err(|error| thrown(&error))?;
    Ok(Value::String(cx.lua.create_string(bytes)?))
}

fn thrown(error: &Error) -> PeekError {
    PeekError::Thrown(error.to_string())
}

/// The markup: a string, a number, or a list of them. Pandoc reads the
/// items of a list as one text (probed: `{'*a', 'b*'}` gives emphasis).
fn markup(call: &Call, max_bytes: u64) -> Peek<Vec<Vec<u8>>> {
    let cx = call.cx;
    let item = |value: &Value| {
        if let Some(source) = crate::source::read(value)? {
            return Ok(source.text.into_bytes());
        }
        match value {
            Value::String(text) => Ok(text.as_bytes().to_vec()),
            Value::Integer(_) | Value::Number(_) => Ok(cx.text(value)?.into_bytes()),
            Value::Table(_) => Err(PeekError::failure("all choices failed")),
            other => Err(PeekError::Thrown(format!(
                "table expected, got {}",
                described_type(other)
            ))),
        }
    };
    let remaining = std::cell::Cell::new(max_bytes);
    let bounded_item = |value: &Value| {
        let bytes = item(value)?;
        let left = remaining
            .get()
            .checked_sub(bytes.len() as u64)
            .ok_or_else(|| PeekError::Thrown("input exceeds the size limit".into()))?;
        remaining.set(left);
        Ok(bytes)
    };
    match call.argument(0) {
        Value::Table(_) => call.required(0, "content", |value| cx.list(value, bounded_item)),
        value @ (Value::String(_) | Value::Integer(_) | Value::Number(_)) => {
            Ok(vec![bounded_item(&value)?])
        }
        value => Err(PeekError::Thrown(format!(
            "table expected, got {}",
            described_type(&value)
        ))),
    }
}

/// The text of a source, or the error of Pandoc for invalid UTF-8.
fn utf8(bytes: Vec<u8>) -> Peek<Text> {
    String::from_utf8(bytes).map_err(|error| {
        let start = error.utf8_error().valid_up_to();
        let byte = error.as_bytes().get(start).copied().unwrap_or_default();
        PeekError::Thrown(format!(
            "Cannot decode byte '\\x{byte:x}': Data.Text.Encoding: Invalid UTF-8 stream"
        ))
    })
}

/// `read_env`: Randoc runs each reader with the host of the conversion, so
/// only an empty list has the effect of Pandoc.
fn read_env(cx: &Cx, value: &Value) -> Peek<()> {
    let items = cx.list(value, |item| Ok(item.clone()))?;
    if items.is_empty() {
        Ok(())
    } else {
        Err(PeekError::Thrown(not_implemented(
            "the argument read_env of pandoc.read",
        )))
    }
}

fn not_implemented(what: &str) -> String {
    format!("{what} is not implemented in Randoc")
}

/// A format specification: a string such as `markdown+smart-raw_html`, or
/// a table with the fields `format` and `extensions`.
fn flavored(cx: &Cx, value: &Value) -> Peek<Flavored> {
    let read = || match value {
        Value::String(text) => from_spec(&text.text()),
        Value::Table(table) => from_table(cx, table),
        _ => Err(mismatch("string or table", value)),
    };
    retrieving("flavored format", read())
}

/// Reads a format specification string with the syntax rule of the
/// command line (`format_spec` of `crates/randoc-cli/src/convert.rs`).
fn from_spec(spec: &str) -> Peek<Flavored> {
    let unexpected = match spec.chars().next() {
        None => Some("end of input".to_owned()),
        Some(c @ ('+' | '-')) => Some(format!("'{c}'")),
        Some(_) => None,
    };
    if let Some(unexpected) = unexpected {
        return Err(PeekError::Thrown(format!(
            "Error parsing format '{spec}': (line 1, column 1):\nunexpected {unexpected}"
        )));
    }
    let spec = FormatSpec::parse(spec);
    let extensions = spec.extensions().map_err(|error| thrown(&error))?;
    Ok(Flavored {
        name: spec.name,
        extensions,
    })
}

/// Reads `{format = NAME, extensions = LIST or MAP}`. A list names all
/// enabled extensions; a map enables an extension for `true` or `enable`
/// and disables it for each other value.
fn from_table(cx: &Cx, table: &Table) -> Peek<Flavored> {
    let format: Value = table.raw_get("format")?;
    let name = within(|| "retrieving raw field 'format'".into(), cx.text(&format))?;
    let mut extensions = default_extensions(&name);
    let changes = match table.raw_get::<Value>("extensions")? {
        Value::Nil => None,
        Value::Table(changes) => Some(changes),
        other => {
            return within(
                || "retrieving raw field 'extensions'".into(),
                Err(mismatch("table", &other)),
            );
        }
    };
    if let Some(changes) = changes {
        if changes.raw_len() > 0 {
            extensions = Extensions::empty();
            for item in changes.sequence_values::<Value>() {
                extensions.enable(extension(&name, &cx.text(&item?)?)?);
            }
        } else {
            for pair in changes.pairs::<Value, Value>() {
                let (key, setting) = pair?;
                let extension = extension(&name, &cx.text(&key)?)?;
                let enable = match &setting {
                    Value::Boolean(on) => *on,
                    Value::String(text) => &*text.as_bytes() == b"enable",
                    _ => false,
                };
                if enable {
                    extensions.enable(extension);
                } else {
                    extensions.disable(extension);
                }
            }
        }
    }
    Ok(Flavored { name, extensions })
}

fn extension(format: &str, name: &str) -> Peek<Extension> {
    Extension::from_name(name)
        .filter(|extension| supported_extensions(format).contains(*extension))
        .ok_or_else(|| {
            thrown(&Error::UnsupportedExtension {
                extension: name.into(),
                format: format.into(),
            })
        })
}
