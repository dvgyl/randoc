//! Starts the server adapter for the Pandoc server oracle suite.

use std::process::ExitCode;

use randoc_server::{ServerCliAction, parse_server_args, run_server};

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let args = if args.first().is_some_and(|arg| arg == "server") {
        &args[1..]
    } else {
        &args
    };
    match parse_server_args(args) {
        Ok(ServerCliAction::Help(text) | ServerCliAction::Version(text)) => print!("{text}"),
        Ok(ServerCliAction::Run(config)) => {
            if let Err(error) = run_server(&config) {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
        Err(error) => {
            eprint!("{error}");
            return ExitCode::from(error.exit_code() as u8);
        }
    }
    ExitCode::SUCCESS
}
