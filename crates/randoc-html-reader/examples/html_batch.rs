//! A tool for differential tests: reads a file of cases and prints the JSON
//! document and the messages of the `html` reader for each case.
//!
//! The cases are separated by a line `%%%%`. A first line `@FORMAT` gives the
//! format with extension changes, for example `@html+raw_html`. The tool
//! `tools/differential.py` compares the output with the output of Pandoc.

use std::io::Write;

use randoc_ast::json;
use randoc_core::{Context, FormatSpec, Reader, ReaderOptions, Sources};

fn main() -> std::io::Result<()> {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: html_batch FILE");
        std::process::exit(2);
    };
    let data = std::fs::read_to_string(path)?;
    let Some(Reader::Text(reader)) = randoc_html_reader::reader("html") else {
        eprintln!("the html reader is not registered");
        std::process::exit(2);
    };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for (index, case) in data.split("\n%%%%\n").enumerate() {
        let (format, input) = match case
            .strip_prefix('@')
            .and_then(|rest| rest.split_once('\n'))
        {
            Some((format, input)) => (format, input),
            None => ("html", case),
        };
        writeln!(out, "--- {}", index + 1)?;
        let mut options = ReaderOptions::default();
        match FormatSpec::parse(format).extensions() {
            Ok(extensions) => options.extensions = extensions,
            Err(error) => {
                writeln!(out, "ERROR: {error}")?;
                continue;
            }
        }
        let mut context = Context::pure();
        context.limits.max_nesting_depth = 16384;
        match reader.read(&Sources::from_text(input), &options, &mut context) {
            Ok(doc) => writeln!(out, "{}", json::to_json(&doc))?,
            Err(error) => writeln!(out, "ERROR: {error}")?,
        }
        for message in context.log() {
            writeln!(
                out,
                "LOG: {} {}",
                message.level().label(),
                message.text().replace('\n', "\\n")
            )?;
        }
    }
    Ok(())
}
