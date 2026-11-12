//! JSON filters: external programs that read the document as JSON on
//! standard input and write the changed document as JSON on standard output.

use std::io::Write;

use randoc::ast::Pandoc;
use randoc::{Context, Error, LogMessage, ReaderOptions, Result, TrackChanges};

use crate::convert::data_path;

/// The interpreter that Pandoc selects by the file extension of a filter
/// that is not executable.
fn interpreter(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.').map(|(_, e)| e)?;
    Some(match extension {
        "py" => "python",
        "hs" => "runhaskell",
        "pl" => "perl",
        "rb" => "ruby",
        "php" => "php",
        "js" => "node",
        "r" => "Rscript",
        _ => return None,
    })
}

/// The paths that the search for the filter `path` checks, in order: the
/// path itself, `filters/PATH` of the user data folder, and for a name
/// without `/` each folder of `path_variable`.
pub fn candidates(path: &str, data_dir: &str, path_variable: &str) -> Vec<String> {
    let mut out = vec![
        path.to_owned(),
        data_path(data_dir, &format!("filters/{path}")),
    ];
    if !path.contains('/') {
        out.extend(
            path_variable
                .split(':')
                .filter(|dir| !dir.is_empty())
                .map(|dir| format!("{dir}/{path}")),
        );
    }
    out
}

/// A filter program that the search found.
struct Found {
    /// The name in the log and in an error: the path of a file, or the
    /// name that the `PATH` finds.
    label: String,
    /// The program to start.
    program: String,
}

fn find(path: &str, data_dir: &str, context: &mut Context) -> Option<Found> {
    let path_variable = context
        .host()
        .environment_variable("PATH")
        .unwrap_or_default();
    let list = candidates(path, data_dir, &path_variable);
    let index = list
        .iter()
        .position(|candidate| context.host().file_exists(candidate))?;
    let candidate = list.get(index)?.clone();
    Some(if index < 2 {
        // A relative file path starts with `./`, so that the system does not
        // search the `PATH` for it.
        let program = if candidate.starts_with('/') {
            candidate.clone()
        } else {
            format!("./{candidate}")
        };
        Found {
            label: candidate,
            program,
        }
    } else {
        Found {
            label: path.to_owned(),
            program: path.to_owned(),
        }
    })
}

/// Returns true when a program `name` is in a folder of the `PATH`.
fn on_path(name: &str, context: &mut Context) -> bool {
    let path_variable = context
        .host()
        .environment_variable("PATH")
        .unwrap_or_default();
    path_variable
        .split(':')
        .filter(|dir| !dir.is_empty())
        .any(|dir| context.host().file_exists(&format!("{dir}/{name}")))
}

/// The error status of a filter: the error of the system for a program that
/// cannot start, which `ENOEXEC` (8) marks for a file without a `#!` line.
const NOT_EXECUTABLE_FORMAT: i32 = 8;

/// Runs the JSON filter `path` on the document. `format` is the name of the
/// output format without extensions, the only argument of the filter.
pub fn run(
    path: &str,
    doc: &Pandoc,
    format: &str,
    data_dir: &str,
    reader_options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let Some(found) = find(path, data_dir, context) else {
        context.report(LogMessage::RunningFilter(path.into()));
        return Err(failure(path, format!("Could not find executable {path}")));
    };
    let label = found.label.as_str();
    context.report(LogMessage::RunningFilter(label.into()));
    let started = std::time::Instant::now();
    let input = randoc::ast::json::to_json(doc).into_bytes();
    let host = context.host();
    for (name, value) in [
        ("PANDOC_VERSION", randoc::PANDOC_VERSION.to_owned()),
        ("PANDOC_READER_OPTIONS", reader_options_json(reader_options)),
    ] {
        host.set_environment_variable(name, Some(&value))
            .map_err(|error| failure(label, error.to_string()))?;
    }
    let output = match host.run_process(&found.program, &[format.into()], &input) {
        // The system does not run a file that is not executable; Pandoc
        // then starts the interpreter of its extension.
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            match interpreter(&found.program) {
                Some(interpreter) if on_path(interpreter, context) => context.host().run_process(
                    interpreter,
                    &[found.program.as_str().into(), format.into()],
                    &input,
                ),
                Some(interpreter) => {
                    return Err(failure(
                        label,
                        format!("Could not find executable {interpreter}"),
                    ));
                }
                None => Err(error),
            }
        }
        // A file without a `#!` line runs in the shell, as `execvp` does.
        Err(error) if error.raw_os_error() == Some(NOT_EXECUTABLE_FORMAT) => {
            context.host().run_process(
                "/bin/sh",
                &[found.program.as_str().into(), format.into()],
                &input,
            )
        }
        other => other,
    }
    .map_err(|error| failure(label, error.to_string()))?;
    // The filter shares the standard error of Pandoc.
    let _ = std::io::stderr().write_all(&output.stderr);
    if output.exit_code != 0 {
        return Err(failure(
            label,
            format!("Filter returned error status {}", output.exit_code),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let doc = randoc::ast::json::from_json_with_limit(&text, context.limits.max_nesting_depth)
        .map_err(|error| failure(label, error.to_string()))?;
    let milliseconds = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    context.report(LogMessage::FilterCompleted(label.into(), milliseconds));
    Ok(doc)
}

fn failure(label: &str, message: String) -> Error {
    Error::Filter {
        filter: label.into(),
        message,
    }
}

/// The JSON form of the reader options that a filter gets in
/// `PANDOC_READER_OPTIONS`, with the fields in the order of Pandoc.
fn reader_options_json(options: &ReaderOptions) -> String {
    use serde_json::Value;
    let texts = |items: Vec<String>| Value::Array(items.into_iter().map(Value::String).collect());
    let mut extensions: Vec<&str> = options.extensions.iter().map(|e| e.name()).collect();
    crate::info::sort_extensions(&mut extensions);
    let track_changes = match options.track_changes {
        TrackChanges::AcceptChanges => "accept-changes",
        TrackChanges::RejectChanges => "reject-changes",
        TrackChanges::AllChanges => "all-changes",
    };
    let fields = [
        (
            "extensions",
            texts(extensions.into_iter().map(str::to_owned).collect()),
        ),
        ("standalone", Value::Bool(options.standalone)),
        ("columns", Value::from(options.columns)),
        ("tab-stop", Value::from(options.tab_stop)),
        (
            "indented-code-classes",
            texts(
                options
                    .indented_code_classes
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            ),
        ),
        (
            "abbreviations",
            texts(
                options
                    .abbreviations
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            ),
        ),
        (
            "default-image-extension",
            Value::String(options.default_image_extension.to_string()),
        ),
        ("track-changes", Value::String(track_changes.to_owned())),
        ("strip-comments", Value::Bool(options.strip_comments)),
        ("typst-inputs", Value::Array(Vec::new())),
    ];
    let body: Vec<String> = fields
        .iter()
        .map(|(name, value)| format!("{}:{value}", Value::String((*name).to_owned())))
        .collect();
    format!("{{{}}}", body.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_search_checks_the_path_the_data_folder_and_the_path_variable() {
        assert_eq!(
            candidates("f.sh", "/d", "/a::/b"),
            ["f.sh", "/d/filters/f.sh", "/a/f.sh", "/b/f.sh"]
        );
        assert_eq!(
            candidates("./f.sh", "/d", "/a"),
            ["./f.sh", "/d/filters/./f.sh"]
        );
        assert_eq!(interpreter("x.py"), Some("python"));
        assert_eq!(interpreter("x.r"), Some("Rscript"));
        assert_eq!(interpreter("x.sh"), None);
    }

    #[test]
    fn reader_options_have_the_fields_of_pandoc() {
        // The start of `PANDOC_READER_OPTIONS` for `pandoc -f markdown-smart+east_asian_line_breaks`
        // is the extension list in the order of `--list-extensions`.
        let mut options = ReaderOptions::default();
        options.extensions = [
            randoc::Extension::Smart,
            randoc::Extension::AllSymbolsEscapable,
        ]
        .into_iter()
        .collect();
        options.abbreviations = ["Mr.".into(), "Dr.".into()].into_iter().collect();
        options.columns = 72;
        assert_eq!(
            reader_options_json(&options),
            "{\"extensions\":[\"all_symbols_escapable\",\"smart\"],\"standalone\":false,\
             \"columns\":72,\"tab-stop\":4,\"indented-code-classes\":[],\
             \"abbreviations\":[\"Dr.\",\"Mr.\"],\"default-image-extension\":\"\",\
             \"track-changes\":\"accept-changes\",\"strip-comments\":false,\"typst-inputs\":[]}"
        );
    }
}
