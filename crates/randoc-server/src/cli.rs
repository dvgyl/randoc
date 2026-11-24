use std::fmt;

use crate::ServerConfig;

/// The action selected by the server arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerCliAction {
    /// Start the server with these settings.
    Run(ServerConfig),
    /// Write this help text to standard output.
    Help(String),
    /// Write this version text to standard output.
    Version(String),
}

/// A server argument error. The message ends with a newline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerArgError(String);

impl ServerArgError {
    /// Returns the Pandoc exit code for an option error.
    pub const fn exit_code(&self) -> i32 {
        6
    }
}

impl fmt::Display for ServerArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ServerArgError {}

const HELP: &str = "Usage: pandoc [OPTION...]\n  -p NUMBER  --port=NUMBER     port number\n  -t NUMBER  --timeout=NUMBER  timeout (seconds)\n  -h         --help            help message\n  -v         --version         version info\n\n";
const TRY_HELP: &str = "Try --help for more information.\n";

enum OptionValue<'a> {
    Port(&'a str),
    Timeout(&'a str),
    Help,
    Version,
}

fn long_option(name: &str) -> Option<&'static str> {
    let mut matches = ["port", "timeout", "help", "version"]
        .into_iter()
        .filter(|option| !name.is_empty() && option.starts_with(name));
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn push_option<'a>(
    name: &str,
    spelling: &str,
    attached: Option<&'a str>,
    args: &mut impl Iterator<Item = &'a str>,
    options: &mut Vec<OptionValue<'a>>,
    errors: &mut String,
) {
    let value = match name {
        "port" | "timeout" => {
            let Some(value) = attached.or_else(|| args.next()) else {
                errors.push_str(&format!(
                    "option `{spelling}' requires an argument NUMBER\n"
                ));
                return;
            };
            if name == "port" {
                OptionValue::Port(value)
            } else {
                OptionValue::Timeout(value)
            }
        }
        _ => {
            if attached.is_some() {
                errors.push_str(&format!("option `{spelling}' doesn't allow an argument\n"));
                return;
            }
            if name == "help" {
                OptionValue::Help
            } else {
                OptionValue::Version
            }
        }
    };
    options.push(value);
}

/// Parses server options after the executable name or the `server` subcommand.
pub fn parse_server_args(args: &[String]) -> Result<ServerCliAction, ServerArgError> {
    let mut args = args.iter().map(String::as_str);
    let mut options = Vec::new();
    let mut positionals = Vec::new();
    let mut errors = String::new();

    while let Some(arg) = args.next() {
        if arg == "--" {
            positionals.extend(args);
            break;
        }
        if let Some(long) = arg.strip_prefix("--") {
            let (name, attached) = long
                .split_once('=')
                .map_or((long, None), |(name, value)| (name, Some(value)));
            if let Some(option) = long_option(name) {
                push_option(
                    option,
                    &format!("--{name}"),
                    attached,
                    &mut args,
                    &mut options,
                    &mut errors,
                );
            } else {
                errors.push_str(&format!("Unknown option: {arg}\n"));
            }
        } else if let Some(short) = arg.strip_prefix('-').filter(|short| !short.is_empty()) {
            for (index, key) in short.char_indices() {
                let option = match key {
                    'p' => "port",
                    't' => "timeout",
                    'h' => "help",
                    'v' => "version",
                    _ => {
                        errors.push_str(&format!("Unknown option: -{key}\n"));
                        continue;
                    }
                };
                let takes_value = matches!(key, 'p' | 't');
                let attached = &short[index + key.len_utf8()..];
                let attached = (takes_value && !attached.is_empty()).then_some(attached);
                push_option(
                    option,
                    &format!("-{key}"),
                    attached,
                    &mut args,
                    &mut options,
                    &mut errors,
                );
                if takes_value {
                    break;
                }
            }
        } else {
            positionals.push(arg);
        }
    }

    if !errors.is_empty() {
        errors.push_str(TRY_HELP);
        return Err(ServerArgError(errors));
    }
    if !positionals.is_empty() {
        return Err(ServerArgError(format!(
            "Unknown arguments: {}\n",
            positionals.join(" ")
        )));
    }

    let mut config = ServerConfig::default();
    for option in options {
        match option {
            OptionValue::Help => return Ok(ServerCliAction::Help(HELP.into())),
            OptionValue::Version => {
                return Ok(ServerCliAction::Version(format!(
                    "randoc {} (compatible with Pandoc {})\nFeatures: +server\n",
                    randoc::RANDOC_VERSION,
                    randoc::PANDOC_VERSION,
                )));
            }
            OptionValue::Port(value) => config.port = number(value)?,
            OptionValue::Timeout(value) => config.timeout_secs = number(value)?,
        }
    }
    Ok(ServerCliAction::Run(config))
}

fn number<T: std::str::FromStr>(value: &str) -> Result<T, ServerArgError> {
    value
        .trim()
        .parse()
        .map_err(|_| ServerArgError(format!("{value} is not a number\n")))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn parse(args: &[&str]) -> Result<ServerCliAction, ServerArgError> {
        parse_server_args(&args.iter().map(|arg| (*arg).into()).collect::<Vec<_>>())
    }

    #[test]
    fn defaults_and_attached_values() {
        assert_eq!(
            parse(&[]),
            Ok(ServerCliAction::Run(ServerConfig::default()))
        );
        assert_eq!(
            parse(&["-p9342", "--timeout=0", "--por", "42"]),
            Ok(ServerCliAction::Run(ServerConfig {
                port: 42,
                timeout_secs: 0,
                ..ServerConfig::default()
            }))
        );
    }

    #[test]
    fn validates_option_names_before_processing_values() {
        assert!(matches!(
            parse(&["--help", "-t", "abc"]),
            Ok(ServerCliAction::Help(_))
        ));
        for (args, diagnostic) in [
            (&["-t", "abc", "--help"][..], "abc is not a number\n"),
            (
                &["--help", "--bogus"],
                "Unknown option: --bogus\nTry --help for more information.\n",
            ),
            (&["file.md"], "Unknown arguments: file.md\n"),
            (
                &["--port"],
                "option `--port' requires an argument NUMBER\nTry --help for more information.\n",
            ),
        ] {
            let error = parse(args).unwrap_err();
            assert_eq!(error.exit_code(), 6);
            assert_eq!(error.to_string(), diagnostic);
        }
    }
}
