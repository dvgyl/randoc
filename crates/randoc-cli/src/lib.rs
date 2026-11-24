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
mod filter;
pub mod info;
mod lua;
mod node;
pub mod opts;
pub mod settings;
mod steps;
pub mod subsystem;
pub mod template;

use std::io::Write;

use randoc::{Error, Result};

use crate::cli::{Environment, Step};
use crate::settings::Settings;

/// The stack size of the conversion thread. Deep documents need a large stack.
/// The system gives memory to the stack only when the program uses it.
pub(crate) const CONVERSION_STACK_BYTES: usize = 1 << 30;

/// The nesting limit for the conversion thread. A debug build uses less than
/// 32 KB of stack for one level, so this depth fits in the stack.
pub const CLI_MAX_NESTING_DEPTH: usize = 16_384;

/// Runs the program with the given arguments and returns the exit code.
///
/// The first argument is the program name.
pub fn main_with_args(args: Vec<String>) -> i32 {
    #[cfg(feature = "server")]
    {
        let program = program_name(&args);
        match server_dispatch(&program, args.get(1..).unwrap_or_default()) {
            Some(ServerDispatch::Cgi(arguments)) => {
                let result = if version_requested(arguments) {
                    write_stdout(
                        info::version_text(server_program(&program), &default_user_data_dir())
                            .as_bytes(),
                    )
                } else {
                    randoc_server::run_cgi().map_err(|error| Error::App(error.to_string()))
                };
                return server_result(result);
            }
            Some(ServerDispatch::Http(arguments)) => return run_server(&program, arguments),
            None => {}
        }
    }
    let worker = std::thread::Builder::new()
        .stack_size(CONVERSION_STACK_BYTES)
        .spawn(move || run(&args));
    match worker.map(std::thread::JoinHandle::join) {
        Ok(Ok(code)) => code,
        // The thread did not start or it panicked. Pandoc uses 62 for an internal error.
        _ => 62,
    }
}

#[cfg(feature = "server")]
#[derive(Debug, PartialEq, Eq)]
enum ServerDispatch<'a> {
    Cgi(&'a [String]),
    Http(&'a [String]),
}

#[cfg(feature = "server")]
fn server_dispatch<'a>(program: &str, arguments: &'a [String]) -> Option<ServerDispatch<'a>> {
    match program {
        "randoc-server.cgi" | "pandoc-server.cgi" => Some(ServerDispatch::Cgi(arguments)),
        "randoc-server" | "pandoc-server" => Some(ServerDispatch::Http(arguments)),
        _ => {
            let (command, rest) = arguments.split_first()?;
            (command == "server").then_some(ServerDispatch::Http(rest))
        }
    }
}

#[cfg(feature = "server")]
fn server_program(program: &str) -> &str {
    if matches!(program, "pandoc" | "pandoc-server" | "pandoc-server.cgi") {
        "pandoc"
    } else {
        "randoc"
    }
}

#[cfg(feature = "server")]
fn run_server(program: &str, arguments: &[String]) -> i32 {
    use randoc_server::ServerCliAction;

    let result = match randoc_server::parse_server_args(arguments) {
        Ok(ServerCliAction::Run(config)) => randoc_server::run_server(&config),
        Ok(ServerCliAction::Help(text)) => write_stdout(text.as_bytes()),
        // Pandoc prints the text of `pandoc --version`.
        Ok(ServerCliAction::Version(_)) => write_stdout(
            info::version_text(server_program(program), &default_user_data_dir()).as_bytes(),
        ),
        Err(error) => {
            let _ = write!(std::io::stderr(), "{error}");
            return error.exit_code();
        }
    };
    server_result(result)
}

#[cfg(feature = "server")]
fn server_result(result: Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{error}");
            error.exit_code()
        }
    }
}

fn run(args: &[String]) -> i32 {
    match run_checked(args) {
        Ok(()) => 0,
        Err(error) => {
            // A failure to write the message must not change the exit code.
            let hint = match &error {
                Error::UnknownWriter(format) if format == "pdf" => {
                    "To create a pdf using pandoc, use -t latex|beamer|context|ms|html5|typst\n\
                     and specify an output file with .pdf extension (-o filename.pdf)."
                }
                _ => "",
            };
            let _ = writeln!(std::io::stderr(), "{error}{hint}");
            error.exit_code()
        }
    }
}

/// Returns the program name without the directory and without a `.exe`
/// suffix, in ASCII lower case.
fn program_name(args: &[String]) -> String {
    let path = args.first().map_or("randoc", String::as_str);
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let name = name.to_ascii_lowercase();
    match name.strip_suffix(".exe") {
        Some(stem) => stem.to_owned(),
        None => name,
    }
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
    let program = &program_name(args);
    let rest = args.get(1..).unwrap_or_default();
    // `pandoc lua ARGS` and a program called `pandoc-lua` run the Lua
    // interpreter.
    if program == "pandoc-lua" {
        return lua_interpreter(program, rest);
    }
    if let Some(("lua", arguments)) = rest.split_first().map(|(a, r)| (a.as_str(), r)) {
        return lua_interpreter("pandoc lua", arguments);
    }
    if version_requested(rest) {
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
        return write_stdout(&info::run(&command, program, &data_dir)?);
    }
    convert::run_on_this_thread(settings, positional)
}

fn version_requested(arguments: &[String]) -> bool {
    arguments
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "-v" || arg == "--version")
}

/// Runs the Lua interpreter with the arguments after `pandoc lua`. The
/// options of the interpreter are not implemented (I-LUA-29); `--` before
/// the script is permitted, and the script sees it in `arg[-1]`.
fn lua_interpreter(program: &str, arguments: &[String]) -> Result<()> {
    let (options, arguments) = match arguments.split_first() {
        Some((first, _)) if first == "--" => arguments.split_at(1),
        Some((first, _)) if first.len() > 1 && first.starts_with('-') => {
            return Err(Error::App(format!(
                "Randoc does not support the option {first} of pandoc lua yet."
            )));
        }
        _ => (&[][..], arguments),
    };
    lua::interpreter(program, options, arguments)
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

    #[cfg(feature = "server")]
    #[test]
    fn server_dispatch_uses_the_executable_name_first() {
        for program in ["/bin/randoc-server", "/bin/pandoc-server"] {
            let named = args(&[program, "server", "--help"]);
            assert_eq!(
                server_dispatch(&program_name(&named), &named[1..]),
                Some(ServerDispatch::Http(&named[1..]))
            );
        }
        for program in ["/bin/randoc-server.cgi", "C:\\bin\\Pandoc-Server.CGI.EXE"] {
            let named = args(&[program, "server", "--help"]);
            assert_eq!(
                server_dispatch(&program_name(&named), &named[1..]),
                Some(ServerDispatch::Cgi(&named[1..]))
            );
        }
        let subcommand = args(&["server", "--port=0"]);
        for program in ["randoc", "", ".."] {
            assert_eq!(
                server_dispatch(program, &subcommand),
                Some(ServerDispatch::Http(&subcommand[1..]))
            );
        }
        for ordinary in [&["--server"][..], &["--", "server"], &[]] {
            assert_eq!(server_dispatch("randoc", &args(ordinary)), None);
        }
    }

    #[test]
    fn program_names_ignore_the_directory_the_suffix_and_the_case() {
        for (path, name) in [
            ("/usr/bin/pandoc", "pandoc"),
            ("C:\\bin\\Pandoc-Lua.Exe", "pandoc-lua"),
            ("pandoc.exe", "pandoc"),
            ("", ""),
        ] {
            assert_eq!(program_name(&args(&[path])), name);
        }
        assert_eq!(program_name(&[]), "randoc");
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
}
