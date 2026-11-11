//! Parses CSL styles or locales and prints one line for each file:
//! `PATH<TAB>ok` or `PATH<TAB>ERROR` with each newline of the error as `\n`.
//!
//! Usage: `csl_parse style|locale LIST_FILE`, where LIST_FILE has one path
//! on each line. A dependent style reads its parent as a file. The program
//! uses the nesting limit of the `randoc` program.

use std::io::Write as _;

use randoc_core::Context;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(kind), Some(list)) = (args.get(1), args.get(2)) else {
        return Err("usage: csl_parse style|locale LIST_FILE".into());
    };
    let mut out = std::io::stdout().lock();
    for path in std::fs::read_to_string(list)?
        .lines()
        .filter(|l| !l.is_empty())
    {
        let text = String::from_utf8_lossy(&std::fs::read(path)?).into_owned();
        let mut context = Context::native();
        context.limits.max_nesting_depth = 16384;
        let result = match kind.as_str() {
            "locale" => randoc_citeproc::parse_locale(&text, 16384)
                .map(|_| ())
                .map_err(|e| e.to_string()),
            _ => randoc_citeproc::parse_style(&mut context, &text)
                .map(|_| ())
                .map_err(|e| e.to_string()),
        };
        let shown = result
            .err()
            .unwrap_or_else(|| "ok".to_owned())
            .replace('\n', "\\n");
        writeln!(out, "{path}\t{shown}")?;
    }
    Ok(())
}
