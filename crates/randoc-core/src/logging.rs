use std::fmt;

use randoc_ast::Text;

/// How much the program reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Verbosity {
    /// Errors only. This is the level of `--quiet`.
    Error,
    /// Errors and warnings. This is the default level.
    #[default]
    Warning,
    /// Errors, warnings and information. This is the level of `--verbose`.
    Info,
}

impl Verbosity {
    /// Returns the label that Pandoc prints before a message, for example `WARNING`.
    pub fn label(self) -> &'static str {
        match self {
            Verbosity::Error => "ERROR",
            Verbosity::Warning => "WARNING",
            Verbosity::Info => "INFO",
        }
    }
}

/// A position in an input source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SourcePos {
    /// The source name. An empty string means standard input.
    pub source: Text,
    /// The 1-based line.
    pub line: u32,
    /// The 1-based column.
    pub column: u32,
}

impl SourcePos {
    /// Makes a position.
    pub fn new(source: impl Into<Text>, line: u32, column: u32) -> Self {
        SourcePos {
            source: source.into(),
            line,
            column,
        }
    }
}

impl fmt::Display for SourcePos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.source.is_empty() {
            write!(f, "{} ", self.source)?;
        }
        write!(f, "line {} column {}", self.line, self.column)
    }
}

/// A message that a reader, a writer or the application reports.
///
/// Each variant matches one Pandoc log message type. [`LogMessage::text`]
/// gives the text that Pandoc prints after the `[WARNING]` or `[INFO]` label.
///
/// `DuplicateLinkReference` is verified against Pandoc 3.13. Verify each other
/// text against the oracle when a workstream first reports it, and send a
/// difference to the coordinator.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum LogMessage {
    SkippedContent(Text, SourcePos),
    IgnoredElement(Text),
    DuplicateLinkReference(Text, SourcePos),
    DuplicateNoteReference(Text, SourcePos),
    NoteDefinedButNotUsed(Text, SourcePos),
    DuplicateIdentifier(Text, SourcePos),
    ReferenceNotFound(Text, SourcePos),
    CircularReference(Text, SourcePos),
    UndefinedToggle(Text, SourcePos),
    ParsingUnescaped(Text, SourcePos),
    CouldNotLoadIncludeFile(Text, SourcePos),
    CouldNotParseIncludeFile(Text, SourcePos),
    MacroAlreadyDefined(Text, SourcePos),
    /// The text is the Haskell `show` form of the inline.
    InlineNotRendered(Text),
    /// The text is the Haskell `show` form of the block.
    BlockNotRendered(Text),
    DocxParserWarning(Text),
    PowerpointTemplateWarning(Text),
    IgnoredIoError(Text),
    CouldNotFetchResource(Text, Text),
    CouldNotDetermineImageSize(Text, Text),
    CouldNotConvertImage(Text, Text),
    CouldNotDetermineMimeType(Text),
    CouldNotConvertTexMath(Text, Text),
    CouldNotParseCss(Text),
    Fetching(Text),
    Extracting(Text),
    LoadedResource(Text, Text),
    ScriptingInfo(Text, Option<SourcePos>),
    ScriptingWarning(Text, Option<SourcePos>),
    NoTitleElement(Text),
    NoLangSpecified,
    InvalidLang(Text),
    CouldNotHighlight(Text),
    MissingCharacter(Text),
    Deprecated(Text, Text),
    NoTranslation(Text),
    CouldNotLoadTranslations(Text, Text),
    UnusualConversion(Text),
    UnexpectedXmlElement(Text, Text),
    UnknownOrgExportOption(Text),
    CouldNotDeduceFormat(Vec<Text>, Text),
    RunningFilter(Text),
    FilterCompleted(Text, u64),
    CiteprocWarning(Text),
    AtxHeadingInLhs(i64, Text),
    EnvironmentVariableUndefined(Text),
    DuplicateAttribute(Text, Text),
    NotUtf8Encoded(Text),
    MakePdfInfo(Text, Text),
    MakePdfWarning(Text),
    UnclosedDiv(SourcePos, SourcePos),
    UnsupportedCodePage(i64),
    YamlWarning(SourcePos, Text),
}

impl LogMessage {
    /// Returns the level of the message.
    pub fn level(&self) -> Verbosity {
        use LogMessage::*;
        match self {
            SkippedContent(..)
            | IgnoredElement(..)
            | ParsingUnescaped(..)
            | InlineNotRendered(..)
            | BlockNotRendered(..)
            | Fetching(..)
            | Extracting(..)
            | LoadedResource(..)
            | ScriptingInfo(..)
            | NoLangSpecified
            | UnusualConversion(..)
            | UnexpectedXmlElement(..)
            | RunningFilter(..)
            | FilterCompleted(..)
            | MakePdfInfo(..) => Verbosity::Info,
            _ => Verbosity::Warning,
        }
    }

    /// Returns the message text without the level label.
    pub fn text(&self) -> Text {
        use LogMessage::*;
        let with_detail = |head: String, detail: &Text| {
            if detail.is_empty() {
                head
            } else {
                format!("{head}: {detail}")
            }
        };
        match self {
            SkippedContent(content, pos) => format!("Skipped '{content}' at {pos}"),
            IgnoredElement(element) => format!("Ignored element {element}"),
            DuplicateLinkReference(key, pos) => {
                format!("Duplicate link reference '{key}' at {pos}")
            }
            DuplicateNoteReference(key, pos) => {
                format!("Duplicate note reference '{key}' at {pos}")
            }
            NoteDefinedButNotUsed(key, pos) => {
                format!("Note with key '{key}' defined at {pos} but not used.")
            }
            DuplicateIdentifier(id, pos) => format!("Duplicate identifier '{id}' at {pos}"),
            ReferenceNotFound(key, pos) => format!("Reference not found for '{key}' at {pos}"),
            CircularReference(key, pos) => format!("Circular reference '{key}' at {pos}"),
            UndefinedToggle(name, pos) => format!("Undefined toggle '{name}' at {pos}"),
            ParsingUnescaped(text, pos) => format!("Parsing unescaped '{text}' at {pos}"),
            CouldNotLoadIncludeFile(file, pos) => {
                format!("Could not load include file {file} at {pos}")
            }
            CouldNotParseIncludeFile(file, pos) => {
                format!("Parsing error in include file {file} at {pos}")
            }
            MacroAlreadyDefined(name, pos) => format!("Macro '{name}' already defined at {pos}"),
            InlineNotRendered(shown) | BlockNotRendered(shown) => format!("Not rendering {shown}"),
            DocxParserWarning(detail) => format!("Docx parser warning: {detail}"),
            PowerpointTemplateWarning(detail) => format!("Powerpoint template warning: {detail}"),
            IgnoredIoError(detail) => format!("IO Error (ignored): {detail}"),
            CouldNotFetchResource(resource, detail) => {
                with_detail(format!("Could not fetch resource {resource}"), detail)
            }
            CouldNotDetermineImageSize(image, detail) => with_detail(
                format!("Could not determine image size for {image}"),
                detail,
            ),
            CouldNotConvertImage(image, detail) => {
                with_detail(format!("Could not convert image {image}"), detail)
            }
            CouldNotDetermineMimeType(file) => format!("Could not determine mime type for {file}"),
            CouldNotConvertTexMath(math, detail) => {
                let head = format!("Could not convert TeX math {math}, rendering as TeX");
                if detail.is_empty() {
                    head
                } else {
                    format!("{head}:\n{detail}")
                }
            }
            CouldNotParseCss(detail) => format!("Could not parse CSS:\n{detail}"),
            Fetching(resource) => format!("Fetching {resource}..."),
            Extracting(file) => format!("Extracting {file}..."),
            LoadedResource(original, found) => format!("Loaded {original} from {found}"),
            ScriptingInfo(detail, pos) | ScriptingWarning(detail, pos) => {
                let label = if matches!(self, ScriptingInfo(..)) {
                    "Scripting info"
                } else {
                    "Scripting warning"
                };
                match pos {
                    Some(pos) => format!("{label} at {pos}: {detail}"),
                    None => format!("{label}: {detail}"),
                }
            }
            NoTitleElement(fallback) => format!(
                "This document format requires a nonempty <title> element.\n\
                 Defaulting to '{fallback}' as the title.\n\
                 To specify a title, use 'title' in metadata or --metadata title=\"...\"."
            ),
            NoLangSpecified => "No value for 'lang' was specified in the metadata.\n\
                 It is recommended that lang be specified for this format."
                .to_owned(),
            InvalidLang(lang) => {
                format!("Invalid 'lang' value '{lang}'.\nUse an IETF language tag like 'en-US'.")
            }
            CouldNotHighlight(detail) => format!("Could not highlight code block:\n{detail}"),
            MissingCharacter(detail) => format!("Missing character: {detail}"),
            Deprecated(thing, detail) if detail.is_empty() => format!("Deprecated: {thing}"),
            Deprecated(thing, detail) => format!("Deprecated: {thing}. {detail}"),
            NoTranslation(term) => format!("The term {term} has no translation defined."),
            CouldNotLoadTranslations(lang, detail) => {
                let head = format!("Could not load translations for {lang}");
                if detail.is_empty() {
                    head
                } else {
                    format!("{head}\n{detail}")
                }
            }
            UnusualConversion(detail) => format!("Unusual conversion: {detail}"),
            UnexpectedXmlElement(element, parent) => {
                format!("Unexpected XML element {element} in {parent}")
            }
            UnknownOrgExportOption(option) => {
                format!("Ignoring unknown Org export option: {option}")
            }
            CouldNotDeduceFormat(extensions, format) => format!(
                "Could not deduce format from file extension {}\nDefaulting to {format}",
                extensions.join(" or ")
            ),
            RunningFilter(filter) => format!("Running filter {filter}"),
            FilterCompleted(filter, milliseconds) => {
                format!("Completed filter {filter} in {milliseconds} ms")
            }
            CiteprocWarning(detail) => format!("Citeproc: {detail}"),
            AtxHeadingInLhs(level, contents) => format!(
                "Rendering heading '{contents}' as a paragraph.\n\
                 ATX headings cannot be used in literate Haskell, because '#' is not\n\
                 allowed in column 1.{}",
                if *level < 3 {
                    " Consider using --markdown-headings=setext."
                } else {
                    ""
                }
            ),
            EnvironmentVariableUndefined(name) => {
                format!("Undefined environment variable {name} in defaults file.")
            }
            DuplicateAttribute(name, value) => {
                format!("Ignoring duplicate attribute {name}={value:?}.")
            }
            NotUtf8Encoded(source) => {
                format!("{source} is not UTF-8 encoded: falling back to latin1.")
            }
            MakePdfInfo(head, detail) => {
                if detail.is_empty() {
                    head.clone()
                } else {
                    format!("{head}\n{detail}")
                }
            }
            MakePdfWarning(detail) => detail.clone(),
            UnclosedDiv(open, close) => {
                format!("Div at {open} unclosed at {close}, closing implicitly.")
            }
            UnsupportedCodePage(page) => {
                format!("Unsupported code page {page}. Text will likely be garbled.")
            }
            YamlWarning(pos, detail) => format!("YAML warning ({pos}): {detail}"),
        }
    }

    /// Returns the lines that Pandoc prints to standard error for the message.
    ///
    /// The first line starts with the level label. Each later line of the
    /// message is indented by two spaces. The result ends with a newline.
    pub fn render(&self) -> Text {
        let text = self.text();
        let mut out = format!("[{}] ", self.level().label());
        for (index, line) in text.split('\n').enumerate() {
            if index > 0 {
                out.push_str("\n  ");
            }
            out.push_str(line);
        }
        out.push('\n');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_link_reference_matches_pandoc_3_13() {
        // Standard error of: printf '[x]: a\n[x]: b\n' | pandoc -f markdown -t html
        let message = LogMessage::DuplicateLinkReference("[x]".into(), SourcePos::new("", 2, 1));
        assert_eq!(
            message.render(),
            "[WARNING] Duplicate link reference '[x]' at line 2 column 1\n"
        );
        assert_eq!(message.level(), Verbosity::Warning);
    }

    #[test]
    fn a_position_in_a_file_starts_with_the_file_name() {
        assert_eq!(
            SourcePos::new("a.md", 3, 7).to_string(),
            "a.md line 3 column 7"
        );
        assert_eq!(SourcePos::new("", 3, 7).to_string(), "line 3 column 7");
    }

    #[test]
    fn later_lines_are_indented() {
        let message = LogMessage::NoLangSpecified;
        assert_eq!(
            message.render(),
            "[INFO] No value for 'lang' was specified in the metadata.\n  \
             It is recommended that lang be specified for this format.\n"
        );
    }

    #[test]
    fn levels_are_ordered_from_quiet_to_verbose() {
        assert!(Verbosity::Error < Verbosity::Warning);
        assert!(Verbosity::Warning < Verbosity::Info);
        assert_eq!(Verbosity::default(), Verbosity::Warning);
        assert_eq!(Verbosity::Error.label(), "ERROR");
        assert_eq!(LogMessage::Fetching("x".into()).level(), Verbosity::Info);
    }

    #[test]
    fn optional_details_are_left_out_when_empty() {
        assert_eq!(
            LogMessage::CouldNotFetchResource("a.png".into(), String::new()).text(),
            "Could not fetch resource a.png"
        );
        assert_eq!(
            LogMessage::CouldNotFetchResource("a.png".into(), "gone".into()).text(),
            "Could not fetch resource a.png: gone"
        );
        assert_eq!(
            LogMessage::CouldNotConvertTexMath("x".into(), String::new()).text(),
            "Could not convert TeX math x, rendering as TeX"
        );
        assert_eq!(
            LogMessage::Deprecated("a".into(), String::new()).text(),
            "Deprecated: a"
        );
        assert_eq!(
            LogMessage::Deprecated("a".into(), "b".into()).text(),
            "Deprecated: a. b"
        );
    }
}
