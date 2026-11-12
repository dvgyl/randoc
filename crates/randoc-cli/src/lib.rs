//! The command-line program of Randoc.
//!
//! The program accepts the options of Pandoc 3.13. The steps are:
//! parse the arguments ([`opts`]), apply them to the [`settings::Settings`]
//! ([`cli`], [`defaults`]), then run an information command ([`info`]) or the
//! conversion ([`convert`]).

pub mod cli;
pub mod convert;
mod data_files;
pub mod defaults;
pub mod info;
pub mod opts;
pub mod settings;
pub mod subsystem;
pub mod template;
pub mod yaml;

use std::io::Write;

use randoc::{Error, Result};

use crate::cli::{Environment, Step};
use crate::settings::Settings;

/// The stack size of the conversion thread. Deep documents need a large stack.
/// The system gives memory to the stack only when the program uses it.
const CONVERSION_STACK_BYTES: usize = 1 << 30;

/// The nesting limit for the conversion thread. A debug build uses less than
/// 32 KB of stack for one level, so this depth fits in the stack.
pub const CLI_MAX_NESTING_DEPTH: usize = 16_384;

/// Runs the program with the given arguments and returns the exit code.
///
/// The first argument is the program name.
pub fn main_with_args(args: Vec<String>) -> i32 {
    let worker = std::thread::Builder::new()
        .stack_size(CONVERSION_STACK_BYTES)
        .spawn(move || run(&args));
    match worker.map(std::thread::JoinHandle::join) {
        Ok(Ok(code)) => code,
        // The thread did not start or it panicked. Pandoc uses 62 for an internal error.
        _ => 62,
    }
}

fn run(args: &[String]) -> i32 {
    match run_checked(args) {
        Ok(()) => 0,
        Err(error) => {
            // A failure to write the message must not change the exit code.
            let _ = writeln!(std::io::stderr(), "{error}");
            error.exit_code()
        }
    }
}

/// Returns the program name without the directory.
fn program_name(args: &[String]) -> &str {
    let name = args.first().map_or("randoc", String::as_str);
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// Returns the default user data directory: `$XDG_DATA_HOME/pandoc`, else
/// `$HOME/.local/share/pandoc`, else `$HOME/.pandoc` when only that exists.
pub fn default_user_data_dir() -> String {
    if let Some(xdg) = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|v| !v.is_empty())
    {
        return format!("{}/pandoc", defaults::collapse_separators(&xdg));
    }
    let home = defaults::collapse_separators(&std::env::var("HOME").unwrap_or_default());
    let xdg = format!("{home}/.local/share/pandoc");
    let legacy = format!("{home}/.pandoc");
    if !std::path::Path::new(&xdg).is_dir() && std::path::Path::new(&legacy).is_dir() {
        legacy
    } else {
        xdg
    }
}

/// The system services of the option phase.
struct NativeEnvironment {
    defaults_stack: Vec<String>,
}

impl Environment for NativeEnvironment {
    fn warn(&mut self, text: &str) {
        let _ = std::io::stderr().write_all(text.as_bytes());
    }

    fn apply_defaults(&mut self, settings: &mut Settings, name: &str) -> Result<()> {
        let mut stack = std::mem::take(&mut self.defaults_stack);
        let result = defaults::apply(settings, name, self, &mut stack);
        self.defaults_stack = stack;
        result
    }
}

impl defaults::Loader for NativeEnvironment {
    fn read(&mut self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn user_data_dir(&mut self, data_dir: Option<&str>) -> String {
        data_dir.map_or_else(default_user_data_dir, str::to_owned)
    }

    fn absolute(&mut self, path: &str) -> String {
        let path = if path.starts_with('/') {
            path.to_owned()
        } else {
            let cwd = std::env::current_dir().unwrap_or_default();
            format!("{}/{path}", cwd.to_string_lossy())
        };
        defaults::collapse_separators(&path)
    }

    fn env_var(&mut self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn warn(&mut self, text: &str) {
        Environment::warn(self, text);
    }

    fn max_depth(&self) -> usize {
        CLI_MAX_NESTING_DEPTH
    }
}

const TRY_HELP: &str = "Try pandoc --help for more information.";

/// Parses the arguments and returns the settings, or the information command.
pub fn settings_from_args(
    args: &[String],
    env: &mut dyn Environment,
) -> Result<(Settings, Vec<String>, Option<cli::InfoCommand>)> {
    let parsed = opts::parse(args);
    if !parsed.errors.is_empty() || !parsed.unknown.is_empty() {
        let mut message = parsed.errors.concat();
        for name in &parsed.unknown {
            message.push_str(&format!("Unknown option {name}.\n"));
        }
        return Err(Error::Option(format!("{message}{TRY_HELP}")));
    }
    let mut settings = Settings::default();
    for action in &parsed.actions {
        if let Step::Info(command) = cli::apply(&mut settings, action, env)? {
            return Ok((settings, parsed.positional, Some(command)));
        }
    }
    Ok((settings, parsed.positional, None))
}

fn run_checked(args: &[String]) -> Result<()> {
    let program = program_name(args);
    let rest = args.get(1..).unwrap_or_default();
    let before_end = rest.iter().take_while(|arg| *arg != "--");
    if before_end
        .into_iter()
        .any(|arg| arg == "-v" || arg == "--version")
    {
        return write_stdout(info::version_text(program, &default_user_data_dir()).as_bytes());
    }
    let mut env = NativeEnvironment {
        defaults_stack: Vec::new(),
    };
    let (settings, positional, command) = settings_from_args(rest, &mut env)?;
    if let Some(command) = command {
        let data_dir = settings
            .data_dir
            .clone()
            .unwrap_or_else(default_user_data_dir);
        if let cli::InfoCommand::PrintHighlightStyle(style) = &command {
            convert::check_highlight_style(style, |path| std::fs::read(path))?;
        }
        return write_stdout(&info::run(&command, program, &data_dir)?);
    }
    convert::run(settings, positional)
}

/// Writes bytes to standard output.
pub fn write_stdout(bytes: &[u8]) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(bytes)
        .and_then(|()| stdout.flush())
        .map_err(|error| Error::Io(format!("<stdout>: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoEnvironment;

    impl Environment for NoEnvironment {
        fn warn(&mut self, _: &str) {}
        fn apply_defaults(&mut self, _: &mut Settings, name: &str) -> Result<()> {
            Err(Error::Io(name.to_owned()))
        }
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn errors_come_before_unknown_options() {
        let error =
            settings_from_args(&args(&["--nosuch", "--to"]), &mut NoEnvironment).unwrap_err();
        assert_eq!(
            error.to_string(),
            "option `--to' requires an argument FORMAT\nUnknown option --nosuch.\nTry pandoc --help for more information."
        );
        let error = settings_from_args(&args(&["--zz", "-x"]), &mut NoEnvironment).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Unknown option --zz.\nUnknown option -x.\nTry pandoc --help for more information."
        );
        assert_eq!(error.exit_code(), 6);
    }

    #[test]
    fn an_information_command_stops_the_options() {
        let (_, _, command) = settings_from_args(
            &args(&["--list-input-formats", "--toc-depth=9"]),
            &mut NoEnvironment,
        )
        .unwrap();
        assert_eq!(command, Some(cli::InfoCommand::ListInputFormats));
        assert!(settings_from_args(&args(&["--toc-depth=9", "-h"]), &mut NoEnvironment).is_err());
    }

    #[test]
    fn program_names_lose_the_directory() {
        assert_eq!(program_name(&args(&["/a/b/pandoc"])), "pandoc");
        assert_eq!(program_name(&[]), "randoc");
    }
}
