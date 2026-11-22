//! Lua scripts: filters (`--lua-filter`, `-L` and a Lua item of
//! `filters:`), custom readers (`-f FILE.lua`), custom writers
//! (`-t FILE.lua`) and the interpreter (`pandoc lua`).
//!
//! A script file is found and read with the file system of the process,
//! also under `--sandbox`, because Pandoc runs Lua scripts outside the
//! sandbox (ADR 11).

use randoc::ast::Pandoc;
use randoc::{Context, Reader, ReaderOptions, Result, Writer, WriterOptions};
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

/// Returns the reader of the custom reader script `path` (`-f FILE.lua`).
#[cfg(feature = "lua")]
pub fn custom_reader(
    path: &str,
    settings: &Settings,
    data_dir: &str,
    options: Options<'_>,
    context: &mut Context,
) -> Result<Reader> {
    let file = find_custom(path, data_dir);
    let source = randoc_lua::read_script(&file)?;
    let script = randoc_lua::LuaFilter {
        path: &file,
        source: &source,
    };
    let environment = environment("", settings, data_dir, options, context);
    randoc_lua::custom_reader(&script, environment, context)
}

/// Returns the writer of the custom writer script `path` (`-t FILE.lua`).
#[cfg(feature = "lua")]
pub fn custom_writer(
    path: &str,
    settings: &Settings,
    data_dir: &str,
    options: Options<'_>,
    context: &mut Context,
) -> Result<Writer> {
    let file = find_custom(path, data_dir);
    let source = randoc_lua::read_script(&file)?;
    let script = randoc_lua::LuaFilter {
        path: &file,
        source: &source,
    };
    let environment = environment("", settings, data_dir, options, context);
    randoc_lua::custom_writer(&script, environment, context)
}

/// Runs the Lua interpreter as `pandoc lua SCRIPT ARGS` does. `program` is
/// `pandoc lua`, or `pandoc-lua` for a program of that name. The script `-`,
/// and no script, read standard input, which error messages name `stdin`.
#[cfg(feature = "lua")]
pub fn interpreter(program: &str, args: &[String]) -> Result<()> {
    let script_argument = args.first().map(String::as_str);
    let (name, source) = match script_argument {
        Some(path) if path != "-" => (path.to_owned(), randoc_lua::read_script(path)?),
        _ => {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut std::io::stdin(), &mut bytes)
                .map_err(|error| randoc::Error::Io(format!("<stdin>: {error}")))?;
            ("stdin".to_owned(), bytes)
        }
    };
    let script = randoc_lua::LuaFilter {
        path: &name,
        source: &source,
    };
    let invocation = randoc_lua::Invocation {
        program,
        script: script_argument,
        arguments: args.get(1..).unwrap_or_default(),
    };
    let settings = Settings::default();
    let data_dir = crate::default_user_data_dir();
    let mut context = Context::native();
    context.limits.max_nesting_depth = crate::CLI_MAX_NESTING_DEPTH;
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

/// Without Lua, a custom reader gives the error of a Pandoc without Lua.
#[cfg(not(feature = "lua"))]
pub fn custom_reader(
    _path: &str,
    _settings: &Settings,
    _data_dir: &str,
    _options: Options<'_>,
    _context: &mut Context,
) -> Result<Reader> {
    Err(randoc::Error::NoScriptingEngine)
}

/// Without Lua, a custom writer gives the error of a Pandoc without Lua.
#[cfg(not(feature = "lua"))]
pub fn custom_writer(
    _path: &str,
    _settings: &Settings,
    _data_dir: &str,
    _options: Options<'_>,
    _context: &mut Context,
) -> Result<Writer> {
    Err(randoc::Error::NoScriptingEngine)
}

/// Without Lua, `pandoc lua` gives the error of a Pandoc without Lua.
#[cfg(not(feature = "lua"))]
pub fn interpreter(_program: &str, _args: &[String]) -> Result<()> {
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
