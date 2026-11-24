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
    /// Citation processing (`--citeproc`, `--bibliography`).
    Citeproc,
    /// `--extract-media`.
    ExtractMedia,
}

/// Returns the error for a feature that needs a missing subsystem.
pub fn missing(subsystem: Subsystem) -> Error {
    let feature = match subsystem {
        Subsystem::DataFiles(name) => format!("the default data file '{name}'"),
        Subsystem::Templates => "templates and standalone output".to_owned(),
        Subsystem::Citeproc => "citation processing".to_owned(),
        Subsystem::ExtractMedia => "--extract-media".to_owned(),
    };
    Error::App(format!("Randoc does not support {feature} yet."))
}

/// Returns a default data file of Pandoc.
///
/// Only the templates of `randoc-data` are embedded. A name that Pandoc does not know gives the
/// error of Pandoc.
pub fn default_data_file(name: &str) -> Result<Vec<u8>> {
    let normal = crate::defaults::normalize(name);
    if normal == randoc_citeproc::DEFAULT_STYLE_FILE {
        return Ok(randoc_data::csl::DEFAULT_STYLE.as_bytes().to_vec());
    }
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
    fn the_default_csl_file_contains_the_builtin_style() {
        assert_eq!(
            default_data_file("default.csl").unwrap(),
            randoc_data::csl::DEFAULT_STYLE.as_bytes()
        );
    }

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
    }
}
