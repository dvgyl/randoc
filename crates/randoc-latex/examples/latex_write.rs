//! Writes a JSON document from standard input as LaTeX or Beamer with writer
//! options, for `tools/randtrees.py`. Usage: `latex_write -t FORMAT [--wrap=none|preserve|auto] [--columns=N]`.

use std::io::Read;

use randoc_core::{Context, FormatSpec, WrapOption, WriterOptions};

fn main() {
    let mut format = String::from("latex");
    let mut options = WriterOptions::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-t" => format = args.next().unwrap_or_default(),
            "--wrap=none" => options.wrap_text = WrapOption::WrapNone,
            "--wrap=preserve" => options.wrap_text = WrapOption::WrapPreserve,
            "--wrap=auto" => options.wrap_text = WrapOption::WrapAuto,
            other => {
                if let Some(columns) = other.strip_prefix("--columns=") {
                    options.columns = columns.parse().unwrap_or(72);
                }
            }
        }
    }
    let spec = FormatSpec::parse(&format);
    options.extensions = spec.extensions().unwrap_or_default();
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        std::process::exit(1);
    }
    let Ok(doc) = randoc_ast::json::from_json(&input) else {
        std::process::exit(1);
    };
    let mut context = Context::pure();
    let result = if spec.name == "beamer" {
        randoc_latex::write_beamer(&doc, &options, &mut context)
    } else {
        randoc_latex::write_latex(&doc, &options, &mut context)
    };
    match result {
        Ok(text) => println!("{text}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
