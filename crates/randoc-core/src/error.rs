use std::fmt;

use randoc_ast::Text;

/// The result type of Randoc operations.
pub type Result<T> = std::result::Result<T, Error>;

/// A conversion error.
///
/// Each variant matches one Pandoc error type. [`Error::exit_code`] gives the
/// exit code from the "Exit codes" section of the Pandoc manual. The `Display`
/// form is the text that Pandoc prints to standard error.
///
/// The messages of `UnknownReader`, `UnknownWriter`, `UnsupportedExtension`,
/// `Parse`, `CouldNotFindDataFile`, `CouldNotFindMetadataFile` and `Filter` are
/// verified against Pandoc 3.13. Verify each other message
/// against the oracle when a workstream first produces it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// An input or output operation failed. The text is the full message.
    Io(Text),
    /// A warning occurred and `--fail-if-warnings` is set.
    FailOnWarning,
    /// A general application error.
    App(Text),
    /// A template did not compile or did not render.
    Template(Text),
    /// A command-line option or a defaults file is not valid.
    Option(Text),
    /// A format specification has a syntax error. The text is the full message.
    /// The Pandoc manual does not list this error. Pandoc 3.13 exits with 20.
    Format(Text),
    /// The input format is not known.
    UnknownReader(Text),
    /// The output format is not known.
    UnknownWriter(Text),
    /// The format does not accept the extension.
    UnsupportedExtension {
        /// The extension name.
        extension: Text,
        /// The format name.
        format: Text,
    },
    /// Citation processing failed.
    Citeproc(Text),
    /// A bibliography file is not valid.
    Bibliography {
        /// The bibliography file.
        file: Text,
        /// The details.
        message: Text,
    },
    /// The EPUB subdirectory name has characters that are not permitted.
    EpubSubdirectory(Text),
    /// The PDF engine failed.
    Pdf(Text),
    /// An XML document is not valid.
    Xml {
        /// The file that holds the XML, or an empty string.
        file: Text,
        /// The details.
        message: Text,
    },
    /// The PDF engine program is not installed.
    PdfProgramNotFound(Text),
    /// A network request failed.
    Http {
        /// The URL.
        url: Text,
        /// The details.
        message: Text,
    },
    /// An internal condition that must not occur.
    ShouldNeverHappen(Text),
    /// An error with no more specific type.
    Some(Text),
    /// The input has a syntax error. The text is the full message.
    Parse(Text),
    /// PDF production failed.
    MakePdf(Text),
    /// A syntax definition for highlighting is not valid.
    SyntaxMap(Text),
    /// A filter failed.
    Filter {
        /// The filter name.
        filter: Text,
        /// The details.
        message: Text,
    },
    /// A Lua script failed.
    Lua(Text),
    /// This build has no Lua support.
    NoScriptingEngine,
    /// A macro expands to itself without end.
    MacroLoop(Text),
    /// The input is not valid UTF-8.
    Utf8Decoding {
        /// The input name.
        source: Text,
        /// The byte offset of the first bad byte.
        offset: u64,
        /// The bad byte.
        byte: u8,
    },
    /// A Jupyter notebook is not valid.
    IpynbDecoding(Text),
    /// The character set is not supported.
    UnsupportedCharset(Text),
    /// A text reader got binary input.
    InputNotText(Text),
    /// A data file is not in the data directory or in the built-in data.
    CouldNotFindDataFile(Text),
    /// A metadata file is not in the working directory or the data directory.
    CouldNotFindMetadataFile(Text),
    /// A resource is not in the resource path.
    ResourceNotFound(Text),
}

impl Error {
    /// Returns the process exit code that Pandoc uses for this error.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Io(_) => 1,
            Error::FailOnWarning => 3,
            Error::App(_) => 4,
            Error::Template(_) => 5,
            Error::Option(_) => 6,
            Error::Format(_) => 20,
            Error::UnknownReader(_) => 21,
            Error::UnknownWriter(_) => 22,
            Error::UnsupportedExtension { .. } => 23,
            Error::Citeproc(_) => 24,
            Error::Bibliography { .. } => 25,
            Error::EpubSubdirectory(_) => 31,
            Error::Pdf(_) => 43,
            Error::Xml { .. } => 44,
            Error::PdfProgramNotFound(_) => 47,
            Error::Http { .. } => 61,
            Error::ShouldNeverHappen(_) => 62,
            Error::Some(_) => 63,
            Error::Parse(_) => 64,
            Error::MakePdf(_) => 66,
            Error::SyntaxMap(_) => 67,
            Error::Filter { .. } => 83,
            Error::Lua(_) => 84,
            Error::NoScriptingEngine => 89,
            Error::MacroLoop(_) => 91,
            Error::Utf8Decoding { .. } => 92,
            Error::IpynbDecoding(_) => 93,
            Error::UnsupportedCharset(_) => 94,
            Error::InputNotText(_) => 95,
            Error::CouldNotFindDataFile(_) => 97,
            Error::CouldNotFindMetadataFile(_) => 98,
            Error::ResourceNotFound(_) => 99,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(message)
            | Error::Format(message)
            | Error::App(message)
            | Error::Option(message)
            | Error::Some(message)
            | Error::Parse(message)
            | Error::MakePdf(message)
            | Error::SyntaxMap(message)
            | Error::Citeproc(message) => f.write_str(message),
            Error::FailOnWarning => f.write_str("Failing because there were warnings."),
            Error::Template(message) => write!(f, "Error compiling template {message}"),
            Error::UnknownReader(format) => write!(f, "Unknown input format '{format}'"),
            Error::UnknownWriter(format) => write!(f, "Unknown output format '{format}'"),
            Error::UnsupportedExtension { extension, format } => write!(
                f,
                "The extension '{extension}' is not supported for {format}.\n\
                 Use --list-extensions={format} to list supported extensions."
            ),
            Error::Bibliography { file, message } => {
                write!(f, "Error reading bibliography file {file}:\n{message}")
            }
            Error::EpubSubdirectory(name) => {
                write!(
                    f,
                    "EPUB subdirectory name '{name}' contains illegal characters"
                )
            }
            Error::Pdf(message) => write!(f, "Error producing PDF.\n{message}"),
            Error::Xml { file, message } if file.is_empty() => write!(f, "Invalid XML:\n{message}"),
            Error::Xml { file, message } => write!(f, "Invalid XML in {file}:\n{message}"),
            Error::PdfProgramNotFound(program) => write!(
                f,
                "{program} not found. Please select a different --pdf-engine or install {program}"
            ),
            Error::Http { url, message } => write!(f, "Could not fetch {url}\n{message}"),
            Error::ShouldNeverHappen(message) => write!(
                f,
                "Something we thought was impossible happened!\n\
                 Please report this to pandoc's developers: {message}"
            ),
            Error::Filter { filter, message } => {
                write!(f, "Error running filter '{filter}':\n{message}")
            }
            Error::Lua(message) => write!(f, "Error running Lua:\n{message}"),
            Error::NoScriptingEngine => {
                f.write_str("This version of pandoc has been compiled without Lua support.")
            }
            Error::MacroLoop(name) => write!(f, "Loop encountered in expanding macro {name}"),
            Error::Utf8Decoding {
                source,
                offset,
                byte,
            } => write!(
                f,
                "UTF-8 decoding error in {source} at byte offset {offset} ({byte:02x}).\n\
                 The input must be a UTF-8 encoded text."
            ),
            Error::IpynbDecoding(message) => write!(f, "ipynb decoding error: {message}"),
            Error::UnsupportedCharset(charset) => write!(f, "Unsupported charset {charset}"),
            Error::InputNotText(source) => write!(
                f,
                "Expected text as an input, but received binary data from {source}.\n\
                 If you intended to convert from binary format, verify that it's supported and use\n\
                 explicit -f FORMAT."
            ),
            Error::CouldNotFindDataFile(name) => write!(f, "Could not find data file '{name}'"),
            Error::CouldNotFindMetadataFile(name) => {
                write!(f, "Could not find metadata file '{name}'")
            }
            Error::ResourceNotFound(name) => {
                write!(f, "File '{name}' not found in resource path")
            }
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_match_pandoc_3_13() {
        // Each expected string is the standard error output of Pandoc 3.13.
        assert_eq!(
            Error::UnknownReader("nosuch".into()).to_string(),
            "Unknown input format 'nosuch'"
        );
        assert_eq!(
            Error::UnknownWriter("nosuch".into()).to_string(),
            "Unknown output format 'nosuch'"
        );
        assert_eq!(
            Error::CouldNotFindDataFile("nosuch.x".into()).to_string(),
            "Could not find data file 'nosuch.x'"
        );
        assert_eq!(
            Error::CouldNotFindMetadataFile("nosuch.yaml".into()).to_string(),
            "Could not find metadata file 'nosuch.yaml'"
        );
        assert_eq!(
            Error::Filter {
                filter: "nosuch.lua".into(),
                message: "cannot open nosuch.lua: No such file or directory".into()
            }
            .to_string(),
            "Error running filter 'nosuch.lua':\ncannot open nosuch.lua: No such file or directory"
        );
        assert_eq!(
            Error::UnsupportedExtension {
                extension: "smart".into(),
                format: "json".into()
            }
            .to_string(),
            "The extension 'smart' is not supported for json.\n\
             Use --list-extensions=json to list supported extensions."
        );
    }

    #[test]
    fn exit_codes_match_the_pandoc_manual() {
        // The pairs are from the "Exit codes" section of the Pandoc 3.13 manual.
        let text = || Text::from("x");
        let cases: Vec<(Error, i32)> = vec![
            (Error::Io(text()), 1),
            (Error::FailOnWarning, 3),
            (Error::App(text()), 4),
            (Error::Template(text()), 5),
            (Error::Option(text()), 6),
            (Error::Format(text()), 20),
            (Error::UnknownReader(text()), 21),
            (Error::UnknownWriter(text()), 22),
            (
                Error::UnsupportedExtension {
                    extension: text(),
                    format: text(),
                },
                23,
            ),
            (Error::Citeproc(text()), 24),
            (
                Error::Bibliography {
                    file: text(),
                    message: text(),
                },
                25,
            ),
            (Error::EpubSubdirectory(text()), 31),
            (Error::Pdf(text()), 43),
            (
                Error::Xml {
                    file: text(),
                    message: text(),
                },
                44,
            ),
            (Error::PdfProgramNotFound(text()), 47),
            (
                Error::Http {
                    url: text(),
                    message: text(),
                },
                61,
            ),
            (Error::ShouldNeverHappen(text()), 62),
            (Error::Some(text()), 63),
            (Error::Parse(text()), 64),
            (Error::MakePdf(text()), 66),
            (Error::SyntaxMap(text()), 67),
            (
                Error::Filter {
                    filter: text(),
                    message: text(),
                },
                83,
            ),
            (Error::Lua(text()), 84),
            (Error::NoScriptingEngine, 89),
            (Error::MacroLoop(text()), 91),
            (
                Error::Utf8Decoding {
                    source: text(),
                    offset: 0,
                    byte: 0,
                },
                92,
            ),
            (Error::IpynbDecoding(text()), 93),
            (Error::UnsupportedCharset(text()), 94),
            (Error::InputNotText(text()), 95),
            (Error::CouldNotFindDataFile(text()), 97),
            (Error::CouldNotFindMetadataFile(text()), 98),
            (Error::ResourceNotFound(text()), 99),
        ];
        for (error, code) in cases {
            assert_eq!(error.exit_code(), code, "{error:?}");
        }
    }
}
