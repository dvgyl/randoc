//! Prints the default template of one format. Exits with 1 when there is none.
//! `tests/check_default.sh` uses this program.

use std::io::Write;

fn main() {
    let format = std::env::args().nth(1).unwrap_or_default();
    match randoc_data::default_template(&format) {
        Some(text) => {
            if std::io::stdout().write_all(text.as_bytes()).is_err() {
                std::process::exit(2);
            }
        }
        None => std::process::exit(1),
    }
}
