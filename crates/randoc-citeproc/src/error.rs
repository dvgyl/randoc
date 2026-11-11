//! The errors of the style and locale parsers.

use std::fmt;

/// A CSL error, with the text of `prettyCiteprocError` of the upstream code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiteprocError {
    /// An XML error: the text after `CiteprocXMLError: `.
    Xml(String),
    /// A style or locale error: the text after `CiteprocParseError: `.
    Parse(String),
}

impl fmt::Display for CiteprocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CiteprocError::Xml(text) => write!(f, "CiteprocXMLError: {text}"),
            CiteprocError::Parse(text) => write!(f, "CiteprocParseError: {text}"),
        }
    }
}

impl std::error::Error for CiteprocError {}

/// The oracle exits with code 4 for a broken style, which is `Error::App`.
impl From<CiteprocError> for randoc_core::Error {
    fn from(error: CiteprocError) -> Self {
        randoc_core::Error::App(error.to_string())
    }
}
