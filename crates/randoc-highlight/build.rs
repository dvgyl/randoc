//! Builds the lookup index for the bundled syntax definitions.

use std::env;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

#[expect(
    dead_code,
    reason = "The build script uses the XML parser and reads only root attributes."
)]
#[path = "src/xml.rs"]
mod xml;

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=data/syntax-definitions.txt");
    println!("cargo:rerun-if-changed=src/xml.rs");
    let source = include_str!("data/syntax-definitions.txt");
    let output = env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("OUT_DIR is missing"))?;
    let output = File::create(PathBuf::from(output).join("syntax-index.rs"))?;
    let mut output = BufWriter::new(output);
    writeln!(output, "[")?;
    let mut offset = 0;
    for part in source.split('\u{c}') {
        let end = offset + part.len();
        offset = end + 1;
        let Some((path, text)) = part.split_once('\n') else {
            continue;
        };
        let start = end - text.len();
        let root = xml::parse_definition(text)
            .map_err(|error| io::Error::other(format!("{path}: {error}")))?;
        if root.name != "language" {
            return Err(io::Error::other(format!(
                "{path}: expected a language element"
            )));
        }
        writeln!(
            output,
            "XmlMetadata {{ path: {path:?}, name: {:?}, extensions: {:?}, range: {start}..{end} }},",
            root.attr("name"),
            root.attr("extensions"),
        )?;
    }
    writeln!(output, "]")?;
    output.flush()
}
