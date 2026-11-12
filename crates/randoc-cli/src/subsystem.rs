//! The connection points for the subsystems that Randoc does not have yet.
//!
//! Each feature that needs a missing subsystem returns the error of
//! [`missing`]. A later workstream connects the subsystem here.

use randoc::{Error, Result};

use crate::data_files::DATA_FILES;

/// A subsystem that Randoc does not have yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subsystem {
    /// The default data files (`--print-default-data-file`, templates, translations).
    DataFiles(String),
    /// Templates and `--standalone` output.
    Templates,
    /// Syntax highlighting (`--print-highlight-style`, `--syntax-definition`).
    Highlighting(String),
    /// Lua and JSON filters.
    Filters(String),
    /// Citation processing (`--citeproc`, `--bibliography`).
    Citeproc,
    /// PDF output with a PDF engine.
    Pdf,
    /// `--extract-media`.
    ExtractMedia,
    /// `--embed-resources` and `--self-contained`.
    EmbedResources,
}

/// Returns the error for a feature that needs a missing subsystem.
pub fn missing(subsystem: Subsystem) -> Error {
    let feature = match subsystem {
        Subsystem::DataFiles(name) => format!("the default data file '{name}'"),
        Subsystem::Templates => "templates and standalone output".to_owned(),
        Subsystem::Highlighting(name) => format!("syntax highlighting ('{name}')"),
        Subsystem::Filters(name) => format!("filters ('{name}')"),
        Subsystem::Citeproc => "citation processing".to_owned(),
        Subsystem::Pdf => "PDF output".to_owned(),
        Subsystem::ExtractMedia => "--extract-media".to_owned(),
        Subsystem::EmbedResources => "--embed-resources".to_owned(),
    };
    Error::App(format!("Randoc does not support {feature} yet."))
}

/// The highlighting styles as `--print-highlight-style` prints them.
const HIGHLIGHT_STYLES: &[u8] = include_bytes!("../data/highlight-styles.bundle");

/// Finds an entry in a bundle: a sequence of `NAME\nLENGTH\nBYTES`.
fn bundle_entry<'a>(mut bundle: &'a [u8], name: &str) -> Option<&'a [u8]> {
    loop {
        let line_end = bundle.iter().position(|&b| b == b'\n')?;
        let (entry_name, rest) = bundle.split_at(line_end);
        let rest = rest.get(1..)?;
        let length_end = rest.iter().position(|&b| b == b'\n')?;
        let length: usize = std::str::from_utf8(rest.get(..length_end)?)
            .ok()?
            .parse()
            .ok()?;
        let body = rest.get(length_end + 1..)?;
        let contents = body.get(..length)?;
        if entry_name == name.as_bytes() {
            return Some(contents);
        }
        bundle = body.get(length..)?;
    }
}

/// Returns the JSON form of a highlighting style.
pub fn highlight_style(name: &str) -> Option<Vec<u8>> {
    bundle_entry(HIGHLIGHT_STYLES, &name.to_ascii_lowercase()).map(<[u8]>::to_vec)
}

/// Returns a default data file of Pandoc.
///
/// Only the templates of `randoc-data` are embedded. A name that Pandoc does not know gives the
/// error of Pandoc.
pub fn default_data_file(name: &str) -> Result<Vec<u8>> {
    let normal = crate::defaults::normalize(name);
    if let Some(contents) = randoc_data::data_file(&normal) {
        return Ok(contents.to_vec());
    }
    if DATA_FILES.contains(&normal.as_str()) {
        Err(missing(Subsystem::DataFiles(name.to_owned())))
    } else {
        Err(Error::CouldNotFindDataFile(name.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_data_files_give_the_pandoc_error() {
        assert_eq!(
            default_data_file("nosuch").unwrap_err().to_string(),
            "Could not find data file 'nosuch'"
        );
        assert_eq!(
            default_data_file("abbreviations").unwrap_err().exit_code(),
            4
        );
        let plain = default_data_file("templates/default.plain").unwrap_or_default();
        assert!(plain.starts_with(b"$if(titleblock)$"));
        assert!(highlight_style("Tango").is_some_and(|s| s.starts_with(b"{")));
        assert!(highlight_style("nosuch").is_none());
        assert_eq!(bundle_entry(b"a\n9\nxy", "a"), None);
    }
}
