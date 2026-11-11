//! The error type, the source display and the line index.

use std::cell::Cell;
use std::fmt;

use crate::Mark;

/// A message at a position, with the source line of the position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// The position.
    pub mark: Mark,
    /// The message, with the path of the node in front when it has one.
    pub message: String,
    /// The text of the source line, without the line break.
    pub source: String,
}

/// A YAML error. Its `Display` text is the text of Pandoc without the first
/// line that the caller adds; [`YamlError::report`] gives the full text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlError {
    /// The error.
    pub error: Box<Located>,
    /// A second position that the error refers to, for example the first key
    /// of a duplicate key.
    pub note: Option<Box<Located>>,
}

/// The place where the YAML text comes from. It decides the first line of the
/// error text, the file name in front of each position and the line numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entrance<'a> {
    /// A metadata block in a document. `name` is the input file name, or
    /// empty for standard input. `line` is the line of the opening `---`
    /// delimiter. The YAML text that the caller parses starts with that
    /// delimiter line, as in Pandoc, and ends before the closing delimiter.
    MetadataBlock {
        /// The input file name.
        name: &'a str,
        /// The line of the opening delimiter, from 1.
        line: usize,
    },
    /// A file that `--metadata-file` names.
    MetadataFile {
        /// The file name as the user wrote it.
        name: &'a str,
    },
    /// A defaults file that `--defaults` names.
    DefaultsFile {
        /// The file name as the user wrote it.
        name: &'a str,
    },
}

impl YamlError {
    pub(crate) fn new(error: Located) -> Self {
        YamlError {
            error: Box::new(error),
            note: None,
        }
    }

    /// Makes an error at a mark of `text`. A caller uses it for its own
    /// check of a node, for example an option of the wrong type in a defaults
    /// file, so that the error has the source line and the caret.
    pub fn at(text: &str, mark: Mark, message: impl Into<String>) -> Self {
        let text = crate::clean(text);
        let lines = LineIndex::new(&text);
        YamlError::new(lines.located(lines.position(mark), message))
    }

    /// Returns the full error text of Pandoc for the entrance, without a
    /// final line break.
    pub fn report(&self, entrance: Entrance<'_>) -> String {
        let (header, name, offset) = match entrance {
            Entrance::MetadataBlock { name, line } => (
                format!(
                    "Error parsing YAML metadata at {}(line {line}, column 1):",
                    quoted(name)
                ),
                name,
                line.saturating_sub(1),
            ),
            Entrance::MetadataFile { name } => (
                format!(
                    "Error parsing YAML metadata at {}(line 1, column 1):",
                    quoted(name)
                ),
                name,
                0,
            ),
            Entrance::DefaultsFile { name } => (format!("Error parsing {name}:"), name, 0),
        };
        format!("{header}\n{}", self.display(name, offset))
    }

    fn display(&self, name: &str, offset: usize) -> String {
        let mut out = String::new();
        render(&mut out, &self.error, name, offset);
        if let Some(note) = &self.note {
            out.push('\n');
            render(&mut out, note, name, offset);
        }
        out
    }
}

fn quoted(name: &str) -> String {
    if name.is_empty() {
        String::new()
    } else {
        format!("\"{name}\" ")
    }
}

/// The number of characters of a source line that Pandoc shows.
const WINDOW: usize = 80;

fn render(out: &mut String, located: &Located, name: &str, offset: usize) {
    let line = located.mark.line.saturating_add(offset);
    let number = line.to_string();
    let gutter = " ".repeat(number.len());
    let chars: Vec<char> = located.source.chars().collect();
    let caret_at = located.mark.column.saturating_sub(1);
    // A long line shows a window of 80 characters. The window moves when
    // the caret is right of its middle, and `...` marks each cut end.
    let start = if chars.len() <= WINDOW || caret_at <= WINDOW / 2 {
        0
    } else {
        (caret_at - WINDOW / 2).min(chars.len() - WINDOW)
    };
    let end = (start + WINDOW).min(chars.len());
    let mut shown: String = chars.get(start..end).unwrap_or(&[]).iter().collect();
    let mut caret: String = chars
        .get(start..caret_at.min(end))
        .unwrap_or(&[])
        .iter()
        .map(|c| if *c == '\t' { '\t' } else { ' ' })
        .collect();
    caret.extend(std::iter::repeat_n(' ', caret_at.saturating_sub(end)));
    if start > 0 {
        shown.insert_str(0, "...");
        caret.insert_str(0, "   ");
    }
    if end < chars.len() {
        shown.push_str("...");
    }
    out.push_str(&format!(
        "{name}:{line}:{}: {}\n{gutter} |\n{number} | {shown}\n{gutter} | {caret}^",
        located.mark.column, located.message,
    ));
}

impl fmt::Display for YamlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display("", 0))
    }
}

impl std::error::Error for YamlError {}

/// Finds the line and the column of a byte position. Positions mostly come
/// in increasing order, so the index keeps the last answer and counts on from
/// it on the same line.
pub(crate) struct LineIndex<'a> {
    text: &'a str,
    starts: Vec<usize>,
    last: Cell<(usize, Mark)>,
}

impl<'a> LineIndex<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        LineIndex {
            text,
            starts,
            last: Cell::new((0, Mark { line: 1, column: 1 })),
        }
    }

    pub(crate) fn mark(&self, pos: usize) -> Mark {
        let pos = pos.min(self.text.len());
        let line = self.starts.partition_point(|start| *start <= pos);
        let start = line
            .checked_sub(1)
            .and_then(|i| self.starts.get(i))
            .copied()
            .unwrap_or(0);
        let (last_pos, last_mark) = self.last.get();
        let (from, column) = if last_mark.line == line && last_pos <= pos && last_pos >= start {
            (last_pos, last_mark.column)
        } else {
            (start, 1)
        };
        let counted = self.text.get(from..pos).map_or(0, |s| s.chars().count());
        crate::steps::add(counted + 1);
        let column = column + counted;
        let mark = Mark { line, column };
        self.last.set((pos, mark));
        mark
    }

    /// Returns the byte position of a mark.
    pub(crate) fn position(&self, mark: Mark) -> usize {
        let start = mark
            .line
            .checked_sub(1)
            .and_then(|i| self.starts.get(i))
            .copied()
            .unwrap_or(0);
        let rest = self.text.get(start..).unwrap_or("");
        let offset = rest
            .char_indices()
            .nth(mark.column.saturating_sub(1))
            .map_or(rest.len(), |(i, _)| i);
        start + offset
    }

    /// Returns the text of the line of a position.
    pub(crate) fn source(&self, pos: usize) -> String {
        let pos = pos.min(self.text.len());
        let line = self.starts.partition_point(|start| *start <= pos);
        let start = line
            .checked_sub(1)
            .and_then(|i| self.starts.get(i))
            .copied()
            .unwrap_or(0);
        let rest = self.text.get(start..).unwrap_or("");
        rest.split('\n').next().unwrap_or("").to_string()
    }

    pub(crate) fn located(&self, pos: usize, message: impl Into<String>) -> Located {
        Located {
            mark: self.mark(pos),
            message: message.into(),
            source: self.source(pos),
        }
    }

    pub(crate) fn error(&self, pos: usize, message: impl Into<String>) -> YamlError {
        YamlError::new(self.located(pos, message))
    }
}
