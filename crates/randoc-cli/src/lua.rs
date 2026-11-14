//! Lua filters: `--lua-filter`, `-L` and a Lua item of `filters:`.
//!
//! The filter file is found and read with the file system of the process,
//! also under `--sandbox`, because Pandoc runs Lua filters outside the
//! sandbox (ADR 11).

use randoc::ast::Pandoc;
use randoc::{Context, Result};
#[cfg(feature = "lua")]
use randoc::{Host, LogMessage, NativeHost};

#[cfg(feature = "lua")]
use crate::convert::data_path;
use crate::settings::Settings;

/// The file that Pandoc runs for the Lua filter `path`: the path itself,
/// else `filters/PATH` of the user data folder. When neither exists, the
/// path stays, so that the error names it.
#[cfg(feature = "lua")]
fn find(path: &str, data_dir: &str) -> String {
    let in_data_dir = data_path(data_dir, &format!("filters/{path}"));
    if !NativeHost.file_exists(path) && NativeHost.file_exists(&in_data_dir) {
        in_data_dir
    } else {
        path.to_owned()
    }
}

/// The request headers that a filter sees: the headers of the command line,
/// then the default `User-Agent` when the command line gives none.
#[cfg(feature = "lua")]
fn request_headers(settings: &Settings) -> Vec<(String, String)> {
    let mut headers = settings.request_headers.clone();
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("User-Agent"))
    {
        headers.push((
            "User-Agent".to_owned(),
            format!("pandoc/{}", randoc::PANDOC_VERSION),
        ));
    }
    headers
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
    context: &mut Context,
) -> Result<()> {
    let label = find(path, data_dir);
    context.report(LogMessage::RunningFilter(label.clone()));
    let started = std::time::Instant::now();
    let mut environment = randoc_lua::FilterEnvironment::new(format);
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
    randoc_lua::apply_filter_files(doc, &[label.as_str()], &environment, context)?;
    let milliseconds = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    context.report(LogMessage::FilterCompleted(label, milliseconds));
    Ok(())
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
    _context: &mut Context,
) -> Result<()> {
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
}
