//! Lua scripts: filters (`--lua-filter`, `-L` and a Lua item of
//! `filters:`), custom readers (`-f FILE.lua`), custom writers
//! (`-t FILE.lua`) and the interpreter (`pandoc lua`).
//!
//! A script file is found and read with the file system of the process,
//! also under `--sandbox`, because Pandoc runs Lua scripts outside the
//! sandbox (ADR 11).

use randoc::ast::Pandoc;
use randoc::{Context, Output, ReaderOptions, Result, WriterOptions};
#[cfg(feature = "lua")]
use randoc::{Host, LogMessage, NativeHost};

#[cfg(feature = "lua")]
use crate::convert::data_file;
use crate::settings::Settings;

/// The file that Pandoc runs for the Lua filter `path`: the path itself,
/// else `filters/PATH` of the user data folder. When neither exists, the
/// path stays, so that the error names it.
#[cfg(feature = "lua")]
fn find(path: &str, data_dir: &str) -> String {
    let in_data_dir = data_file(data_dir, "filters", path);
    if !NativeHost.file_exists(path) && NativeHost.file_exists(&in_data_dir) {
        in_data_dir
    } else {
        path.to_owned()
    }
}

/// The request headers that a filter sees: the first value of each header
/// name of the command line, then the default `User-Agent` when the command
/// line has no header of that exact name. Names are compared with their case
/// (oracle probes of cli-R9-18).
#[cfg(feature = "lua")]
fn request_headers(settings: &Settings) -> Vec<(String, String)> {
    let mut seen = std::collections::HashSet::new();
    let mut headers: Vec<(String, String)> = settings
        .request_headers
        .iter()
        .filter(|(name, _)| seen.insert(name.as_str()))
        .cloned()
        .collect();
    if !seen.contains("User-Agent") {
        headers.push((
            "User-Agent".to_owned(),
            format!("pandoc/{}", randoc::PANDOC_VERSION),
        ));
    }
    headers
}

/// The options of the conversion that a Lua filter sees as
/// `PANDOC_READER_OPTIONS` and `PANDOC_WRITER_OPTIONS`.
#[derive(Clone, Copy)]
pub struct Options<'a> {
    /// The options of the reader.
    pub reader: &'a ReaderOptions,
    /// The options of the writer, with the template and the variables.
    pub writer: &'a WriterOptions,
}

/// The environment of a Lua script: the state of the conversion as the
/// globals `PANDOC_STATE`, `PANDOC_READER_OPTIONS` and
/// `PANDOC_WRITER_OPTIONS` show it, and the registry of formats for
/// `pandoc.read` and `pandoc.write`.
#[cfg(feature = "lua")]
fn environment(
    format: &str,
    settings: &Settings,
    data_dir: &str,
    options: Options<'_>,
    context: &Context,
) -> randoc_lua::FilterEnvironment {
    let mut environment = randoc_lua::FilterEnvironment::new(format);
    environment.readers = randoc::formats::reader;
    environment.writers = randoc::formats::writer;
    environment.citeproc = Some(crate::convert::lua_citeproc);
    environment.sandbox = settings.sandbox;
    // Standard input is the input file `-`.
    environment.input_files = if context.state.input_files.is_empty() {
        vec!["-".to_owned()]
    } else {
        context.state.input_files.clone()
    };
    environment.output_file = settings.output_file.clone();
    environment.resource_path = settings.resource_path.iter().cloned().collect();
    // A user data folder that the command line does not name counts only
    // when it exists.
    environment.user_data_dir = settings.data_dir.clone().or_else(|| {
        matches!(
            NativeHost.path_kind(data_dir, true),
            Ok(randoc::PathKind::Directory)
        )
        .then(|| data_dir.to_owned())
    });
    environment.request_headers = request_headers(settings);
    environment.trace = settings.trace;
    environment.verbosity = settings.verbosity.label().to_owned();
    environment.reader_options = options.reader.clone();
    environment.writer_options = options.writer.clone();
    environment
}

/// Runs the Lua filter `path` on the document. `format` is the output
/// format without extensions, the value of `FORMAT`.
#[cfg(feature = "lua")]
pub fn run(
    path: &str,
    doc: &mut Pandoc,
    format: &str,
    settings: &Settings,
    data_dir: &str,
    options: Options<'_>,
    context: &mut Context,
) -> Result<()> {
    let label = find(path, data_dir);
    crate::filter::report_progress(context, LogMessage::RunningFilter(label.clone()));
    let started = std::time::Instant::now();
    let environment = environment(format, settings, data_dir, options, context);
    randoc_lua::apply_filter_files(doc, &[label.as_str()], &environment, context)?;
    let milliseconds = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    crate::filter::report_progress(context, LogMessage::FilterCompleted(label, milliseconds));
    Ok(())
}

/// `true` when a format name is a custom reader or writer script.
pub fn is_script(format: &str) -> bool {
    format.ends_with(".lua")
}

/// The script of a custom reader or writer: the path itself, else
/// `custom/PATH` of the user data folder. When neither exists, the path
/// stays, so that the error names it.
#[cfg(feature = "lua")]
fn find_custom(path: &str, data_dir: &str) -> String {
    let in_data_dir = data_file(data_dir, "custom", path);
    if !NativeHost.file_exists(path) && NativeHost.file_exists(&in_data_dir) {
        in_data_dir
    } else {
        path.to_owned()
    }
}

/// The largest script file that the program reads: the input limit of the
/// conversion.
#[cfg(feature = "lua")]
fn max_script_bytes(context: &Context) -> usize {
    usize::try_from(context.limits.max_input_size).unwrap_or(usize::MAX)
}

/// A custom reader or writer script (`-f FILE.lua`, `-t FILE.lua`) with
/// its Lua state: the program uses one state for all calls, as Pandoc does.
#[cfg(feature = "lua")]
pub struct CustomScript(Box<randoc_lua::CustomScript>);

/// The kind of a custom reader or writer script.
#[cfg(feature = "lua")]
pub use randoc_lua::ScriptKind;

/// Loads the custom reader or writer script `path`: it runs the top-level
/// code one time.
#[cfg(feature = "lua")]
pub fn load_custom(
    path: &str,
    settings: &Settings,
    data_dir: &str,
    options: Options<'_>,
    context: &mut Context,
) -> Result<CustomScript> {
    let file = find_custom(path, data_dir);
    let source = randoc_lua::read_script(&file, max_script_bytes(context))?;
    let script = randoc_lua::LuaFilter {
        path: &file,
        source: &source,
    };
    let environment = environment("", settings, data_dir, options, context);
    randoc_lua::CustomScript::load(&script, environment, context)
        .map(|script| CustomScript(Box::new(script)))
}

#[cfg(feature = "lua")]
impl CustomScript {
    /// The kind of the reader; an error when the script has no reader.
    pub fn reader_kind(&self) -> Result<ScriptKind> {
        self.0.reader_kind()
    }

    /// The kind of the writer; an error when the script has no writer.
    pub fn writer_kind(&self) -> Result<ScriptKind> {
        self.0.writer_kind()
    }

    /// The names that `opts.extensions` shows and the known extensions,
    /// for the extension changes of the format.
    pub fn extensions(
        &self,
        spec: &randoc::FormatSpec,
    ) -> Result<(Vec<String>, randoc::Extensions)> {
        self.0.extensions(&spec.changes, &spec.name)
    }

    /// The text of the global `Template`, or `None` without it.
    pub fn template(&mut self, context: &mut Context) -> Result<Option<String>> {
        self.0.template(context)
    }

    /// Reads text sources with `Reader`.
    pub fn read_text(
        &mut self,
        sources: &randoc::Sources,
        options: &ReaderOptions,
        extensions: &[String],
        context: &mut Context,
    ) -> Result<Pandoc> {
        self.0.read_text(sources, options, extensions, context)
    }

    /// Reads the bytes of one input with `ByteStringReader`.
    pub fn read_binary(
        &mut self,
        bytes: &[u8],
        options: &ReaderOptions,
        extensions: &[String],
        context: &mut Context,
    ) -> Result<Pandoc> {
        self.0.read_binary(bytes, options, extensions, context)
    }

    /// Writes the document with `Writer` or `ByteStringWriter`.
    pub fn write(
        &mut self,
        kind: ScriptKind,
        doc: &Pandoc,
        options: &WriterOptions,
        extensions: &[String],
        context: &mut Context,
    ) -> Result<Output> {
        self.0.write(kind, doc, options, extensions, context)
    }
}

/// Runs the Lua interpreter as `pandoc lua SCRIPT ARGS` does. `program` is
/// `pandoc lua`, or `pandoc-lua` for a program of that name. The script `-`,
/// and no script, read standard input, which error messages name `stdin`.
#[cfg(feature = "lua")]
pub fn interpreter(program: &str, options: &[String], args: &[String]) -> Result<()> {
    let mut context = Context::native();
    context.limits.max_nesting_depth = crate::CLI_MAX_NESTING_DEPTH;
    let max_bytes = max_script_bytes(&context);
    let script_argument = args.first().map(String::as_str);
    let (name, source) = match script_argument {
        Some(path) if path != "-" => (path.to_owned(), randoc_lua::read_script(path, max_bytes)?),
        _ => {
            let mut bytes = Vec::new();
            let limit = u64::try_from(max_bytes)
                .unwrap_or(u64::MAX)
                .saturating_add(1);
            std::io::Read::read_to_end(
                &mut std::io::Read::take(std::io::stdin(), limit),
                &mut bytes,
            )
            .map_err(|error| randoc::Error::Io(format!("<stdin>: {error}")))?;
            if bytes.len() > max_bytes {
                return Err(randoc::Error::Lua(format!(
                    "cannot read stdin: the file is larger than {max_bytes} bytes"
                )));
            }
            ("stdin".to_owned(), bytes)
        }
    };
    let script = randoc_lua::LuaFilter {
        path: &name,
        source: &source,
    };
    let invocation = randoc_lua::Invocation {
        program,
        options,
        script: script_argument,
        arguments: args.get(1..).unwrap_or_default(),
    };
    let settings = Settings::default();
    let data_dir = crate::default_user_data_dir();
    context.state.user_data_dir = Some(data_dir.clone());
    let reader = ReaderOptions::default();
    let writer = WriterOptions::default();
    let options = Options {
        reader: &reader,
        writer: &writer,
    };
    let environment = environment("", &settings, &data_dir, options, &context);
    randoc_lua::run_script(&script, &invocation, &environment, &mut context)
}

/// A program without the Cargo feature `lua` (for example a WebAssembly
/// build) gives the error of a Pandoc without Lua.
#[cfg(not(feature = "lua"))]
pub fn run(
    _path: &str,
    _doc: &mut Pandoc,
    _format: &str,
    _settings: &Settings,
    _data_dir: &str,
    _options: Options<'_>,
    _context: &mut Context,
) -> Result<()> {
    Err(randoc::Error::NoScriptingEngine)
}

/// Without Lua, a script has no kinds.
#[cfg(not(feature = "lua"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(dead_code, reason = "without Lua no script exists")]
pub enum ScriptKind {
    /// A text reader or writer.
    Text,
    /// A binary reader or writer.
    Binary,
}

/// Without Lua, no custom script can exist.
#[cfg(not(feature = "lua"))]
pub enum CustomScript {}

/// Without Lua, a custom reader or writer gives the error of a Pandoc
/// without Lua.
#[cfg(not(feature = "lua"))]
pub fn load_custom(
    _path: &str,
    _settings: &Settings,
    _data_dir: &str,
    _options: Options<'_>,
    _context: &mut Context,
) -> Result<CustomScript> {
    Err(randoc::Error::NoScriptingEngine)
}

#[cfg(not(feature = "lua"))]
impl CustomScript {
    /// No script exists without Lua.
    pub fn reader_kind(&self) -> Result<ScriptKind> {
        match *self {}
    }

    /// No script exists without Lua.
    pub fn writer_kind(&self) -> Result<ScriptKind> {
        match *self {}
    }

    /// No script exists without Lua.
    pub fn extensions(
        &self,
        _spec: &randoc::FormatSpec,
    ) -> Result<(Vec<String>, randoc::Extensions)> {
        match *self {}
    }

    /// No script exists without Lua.
    pub fn template(&mut self, _context: &mut Context) -> Result<Option<String>> {
        match *self {}
    }

    /// No script exists without Lua.
    pub fn read_text(
        &mut self,
        _sources: &randoc::Sources,
        _options: &ReaderOptions,
        _extensions: &[String],
        _context: &mut Context,
    ) -> Result<Pandoc> {
        match *self {}
    }

    /// No script exists without Lua.
    pub fn read_binary(
        &mut self,
        _bytes: &[u8],
        _options: &ReaderOptions,
        _extensions: &[String],
        _context: &mut Context,
    ) -> Result<Pandoc> {
        match *self {}
    }

    /// No script exists without Lua.
    pub fn write(
        &mut self,
        _kind: ScriptKind,
        _doc: &Pandoc,
        _options: &WriterOptions,
        _extensions: &[String],
        _context: &mut Context,
    ) -> Result<Output> {
        match *self {}
    }
}

/// Without Lua, `pandoc lua` gives the error of a Pandoc without Lua.
#[cfg(not(feature = "lua"))]
pub fn interpreter(_program: &str, _options: &[String], _args: &[String]) -> Result<()> {
    Err(randoc::Error::NoScriptingEngine)
}

#[cfg(all(test, feature = "lua"))]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_user_agent_replaces_the_default() {
        let mut settings = Settings {
            request_headers: vec![("A".to_owned(), "b".to_owned())],
            ..Settings::default()
        };
        assert_eq!(
            request_headers(&settings),
            [
                ("A".to_owned(), "b".to_owned()),
                ("User-Agent".to_owned(), "pandoc/3.13".to_owned())
            ]
        );
        settings.request_headers = vec![("User-Agent".to_owned(), "me".to_owned())];
        assert_eq!(
            request_headers(&settings),
            [("User-Agent".to_owned(), "me".to_owned())]
        );
    }

    /// Pandoc 3.13 for `--request-header` with `A:1 B:2 A:3 user-agent:x`:
    /// `A=1;B=2;user-agent=x;User-Agent=pandoc/3.13`.
    #[test]
    fn the_first_value_of_each_exact_name_stays() {
        let pairs = [("A", "1"), ("B", "2"), ("A", "3"), ("user-agent", "x")];
        let settings = Settings {
            request_headers: pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            ..Settings::default()
        };
        let text: Vec<String> = request_headers(&settings)
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        assert_eq!(
            text.join(";"),
            "A=1;B=2;user-agent=x;User-Agent=pandoc/3.13"
        );
    }
}
