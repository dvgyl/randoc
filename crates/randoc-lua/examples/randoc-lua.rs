//! A small driver for the crate, for tests against the oracle.
//!
//! - `randoc-lua lua FILE` runs a Lua file in full mode.
//! - `randoc-lua restricted FILE` runs a Lua file in restricted mode.
//! - `randoc-lua filter FILE [FORMAT]` reads a document in the JSON form
//!   from standard input, applies the Lua filter `FILE`, and writes the
//!   result as `pandoc -f json -t json --lua-filter FILE` does.

use std::io::{Read, Write};
use std::process::ExitCode;

use randoc_ast::json;
use randoc_core::{Context, Error};
use randoc_lua::{FilterEnvironment, LuaFilter, LuaRuntime, Mode, apply_filter};

fn run(args: &[String]) -> Result<(), Error> {
    let read = |file: &String| std::fs::read(file).map_err(|e| Error::Io(format!("{file}: {e}")));
    match args {
        [command, file] if command == "lua" || command == "restricted" => {
            let mode = if command == "lua" {
                Mode::Full
            } else {
                Mode::Restricted
            };
            LuaRuntime::new(mode)?.run(&format!("@{file}"), &read(file)?)
        }
        [command, file, rest @ ..] if command == "filter" => {
            let source = read(file)?;
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|e| Error::Io(e.to_string()))?;
            let mut doc = json::from_json(&input).map_err(|e| Error::Parse(e.to_string()))?;
            let format = rest.first().map_or("json", String::as_str);
            let filter = LuaFilter {
                path: file,
                source: &source,
            };
            let mut context = Context::pure();
            apply_filter(
                &mut doc,
                &filter,
                &FilterEnvironment::new(format),
                &mut context,
            )?;
            let mut text = json::to_json(&doc);
            text.push('\n');
            std::io::stdout()
                .write_all(text.as_bytes())
                .map_err(|e| Error::Io(e.to_string()))
        }
        _ => Err(Error::Option(
            "usage: randoc-lua lua|restricted FILE, or randoc-lua filter FILE [FORMAT]".to_string(),
        )),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(1))
        }
    }
}
