//! CSV and TSV readers.

#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::sync::Arc;

use randoc_ast::{Block, Cell, ColSpec, Inline, Pandoc, Row, Table, TableBody, TableHead};
use randoc_core::{Context, Error, Limits, Reader, ReaderOptions, Result, Sources, Writer};

/// The syntax of delimited text.
#[derive(Clone, Copy, Debug)]
pub struct CsvOptions {
    /// The ASCII field separator.
    pub delimiter: u8,
    /// The ASCII quote character, if the format supports quoted fields.
    pub quote: Option<u8>,
    /// Keep horizontal white space immediately after a separator.
    pub keep_space: bool,
}

/// Returns the reader for a supported format.
pub fn reader(format: &str) -> Option<Reader> {
    let (delimiter, quote) = match format {
        "csv" => (b',', Some(b'"')),
        "tsv" => (b'\t', None),
        _ => return None,
    };
    let options = CsvOptions {
        delimiter,
        quote,
        keep_space: false,
    };
    Some(Reader::Text(Arc::new(
        move |sources: &Sources, _: &ReaderOptions, context: &mut Context| {
            let mut remaining = context.limits.max_input_size;
            let mut input = String::new();
            for (index, source) in sources.0.iter().enumerate() {
                let size = source.text.len() as u64 + if index > 0 { 1 } else { 0 };
                remaining = remaining.checked_sub(size).ok_or_else(input_limit)?;
                if index > 0 {
                    input.push('\n');
                }
                let text = source.text.strip_prefix('\u{feff}').unwrap_or(&source.text);
                input.extend(text.chars().filter(|&ch| ch != '\r'));
                if !text.is_empty() && !input.ends_with('\n') {
                    remaining = remaining.checked_sub(1).ok_or_else(input_limit)?;
                    input.push('\n');
                }
            }
            let blocks = read_table(&input, options, &context.limits)?
                .map(|table| Block::Table(Box::new(table)))
                .into_iter()
                .collect();
            Ok(Pandoc {
                blocks,
                ..Pandoc::default()
            })
        },
    )))
}

/// Returns the writer for a supported format.
pub fn writer(_format: &str) -> Option<Writer> {
    None
}

/// Reads a table. The first record sets the table width.
///
/// The input uses line feeds as record endings and has no byte order mark.
/// The cell limit applies to the empty cells generated to pad short records.
pub fn read_table(input: &str, options: CsvOptions, limits: &Limits) -> Result<Option<Table>> {
    if input.len() as u64 > limits.max_input_size {
        return Err(input_limit());
    }
    if !options.delimiter.is_ascii()
        || options.delimiter == b'\n'
        || options
            .quote
            .is_some_and(|quote| !quote.is_ascii() || quote == b'\n' || quote == options.delimiter)
    {
        return Err(Error::App("invalid delimited text options".into()));
    }
    let mut scanner = Scanner {
        input,
        options,
        pos: 0,
    };
    let Some(header) = scanner.record(usize::MAX)? else {
        return Ok(None);
    };
    let width = header.cells.len();
    let mut padding = limits.max_table_cells;
    let mut body = TableBody::default();
    while let Some(mut row) = scanner.record(width)? {
        padding = padding
            .checked_sub(width - row.cells.len())
            .ok_or_else(|| Error::App("the table exceeds the padding cell limit".into()))?;
        row.cells.resize_with(width, Cell::default);
        body.body.push(row);
    }
    Ok(Some(Table {
        col_specs: vec![ColSpec::default(); width],
        head: TableHead {
            rows: vec![header],
            ..TableHead::default()
        },
        bodies: vec![body],
        ..Table::default()
    }))
}

fn input_limit() -> Error {
    Error::App("the table exceeds the input size limit".into())
}

struct Scanner<'a> {
    input: &'a str,
    options: CsvOptions,
    pos: usize,
}

impl<'a> Scanner<'a> {
    fn byte(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn rest(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn record(&mut self, width: usize) -> Result<Option<Row>> {
        if self.byte().is_none_or(|byte| byte == b'\n') {
            self.finish()?;
            return Ok(None);
        }
        let mut row = Row::default();
        let mut index = 0;
        loop {
            let keep = index < width;
            let text = match self.options.quote {
                Some(quote) if self.byte() == Some(quote) => {
                    self.quoted(quote, index == 0, keep)?
                }
                _ => self.unquoted(keep),
            };
            let delimiter = self.byte() == Some(self.options.delimiter);
            if let Some(text) = text {
                row.cells.push(cell(&text));
            }
            if !delimiter {
                if self.byte() == Some(b'\n') {
                    self.pos += 1;
                } else {
                    self.finish()?;
                }
                return Ok(Some(row));
            }
            self.pos += 1;
            if !self.options.keep_space {
                while self.byte().is_some_and(|byte| {
                    byte != self.options.delimiter && matches!(byte, b' ' | b'\t')
                }) {
                    self.pos += 1;
                }
            }
            index += 1;
        }
    }

    fn unquoted(&mut self, keep: bool) -> Option<Cow<'a, str>> {
        let start = self.pos;
        while self
            .byte()
            .is_some_and(|byte| byte != b'\n' && byte != self.options.delimiter)
        {
            self.pos += 1;
        }
        keep.then(|| Cow::Borrowed(&self.input[start..self.pos]))
    }

    fn quoted(&mut self, quote: u8, first: bool, keep: bool) -> Result<Option<Cow<'a, str>>> {
        self.pos += 1;
        let start = self.pos;
        let mut segment = start;
        let mut decoded = None::<String>;
        loop {
            let Some(end) = self
                .rest()
                .as_bytes()
                .iter()
                .position(|&byte| byte == quote)
            else {
                self.pos = self.input.len();
                return Err(self.error(None, true));
            };
            self.pos += end + 1;
            let end = self.pos - 1;
            if self.byte() != Some(quote) {
                let text = if let Some(mut text) = decoded {
                    text.push_str(&self.input[segment..end]);
                    Some(Cow::Owned(text))
                } else {
                    keep.then(|| Cow::Borrowed(&self.input[start..end]))
                };
                if self.byte() != Some(self.options.delimiter) {
                    if first && start == end {
                        return Err(self.error(None, false));
                    }
                    if self
                        .rest()
                        .chars()
                        .next()
                        .is_some_and(|ch| !ch.is_whitespace())
                    {
                        return Err(self.error(Some("white space or end of input"), false));
                    }
                }
                return Ok(text);
            }
            if keep {
                let text = decoded.get_or_insert_with(String::new);
                text.push_str(&self.input[segment..end]);
                text.push(char::from(quote));
            }
            self.pos += 1;
            segment = self.pos;
        }
    }

    fn finish(&mut self) -> Result<()> {
        self.pos = self.input.len() - self.rest().trim_start().len();
        if self.byte().is_none() {
            Ok(())
        } else {
            Err(self.error(Some("end of input"), false))
        }
    }

    fn error(&self, expected: Option<&str>, quoted_eof: bool) -> Error {
        let before = &self.input[..self.pos];
        let line = before.bytes().filter(|&byte| byte == b'\n').count() + 1;
        let line = line + usize::from(quoted_eof);
        let column = before
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .chars()
            .fold(0, |width, ch| {
                if ch == '\t' {
                    width + 8 - width % 8
                } else {
                    width + 1
                }
            })
            + 1;
        let unexpected = match self.rest().chars().next() {
            Some(ch) => quoted_character(ch),
            None if !quoted_eof && expected.is_none() => "'\\n'".into(),
            None => "end of input".into(),
        };
        let mut message =
            format!("Error at \"csv\" (line {line}, column {column}):\nunexpected {unexpected}");
        if let Some(expected) = expected {
            message.push_str("\nexpecting ");
            message.push_str(expected);
        }
        Error::Parse(message)
    }
}

fn quoted_character(ch: char) -> String {
    const CONTROL_NAMES: [&str; 32] = [
        "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "a", "b", "t", "n", "v", "f", "r", "SO",
        "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC",
        "FS", "GS", "RS", "US",
    ];
    match ch {
        '\0'..='\u{1f}' => format!("'\\{}'", CONTROL_NAMES[ch as usize]),
        '\u{7f}' => "'\\DEL'".into(),
        '\\' | '\'' => format!("'\\{ch}'"),
        ch if !ch.is_ascii() => format!("'\\{}'", u32::from(ch)),
        ch => format!("'{ch}'"),
    }
}

fn cell(text: &str) -> Cell {
    let mut parts = Vec::new();
    let mut lines = text.split('\n').peekable();
    let mut after_break = false;
    while let Some(mut line) = lines.next() {
        if after_break {
            line = line.trim_start_matches([' ', '\t']);
        }
        if lines.peek().is_some() {
            line = line.trim_end_matches([' ', '\t']);
        }
        let mut space = false;
        let mut start = 0;
        for (pos, ch) in line.char_indices() {
            let horizontal = matches!(ch, ' ' | '\t');
            if horizontal != space {
                if space {
                    parts.push(Inline::Space);
                } else if pos > start {
                    parts.push(Inline::Str(line[start..pos].into()));
                }
                start = pos;
                space = horizontal;
            }
        }
        if space {
            parts.push(Inline::Space);
        } else if start < line.len() {
            parts.push(Inline::Str(line[start..].into()));
        }
        if lines.peek().is_some() {
            parts.push(Inline::LineBreak);
            after_break = true;
        }
    }
    if text.ends_with('\n') {
        parts.pop();
    }
    Cell {
        content: if parts.is_empty() {
            Vec::new()
        } else {
            vec![Block::Plain(parts)]
        },
        ..Cell::default()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn csv(input: &str, limits: Limits) -> Result<Pandoc> {
        let mut context = Context::pure();
        context.limits = limits;
        let Reader::Text(reader) = reader("csv").unwrap() else {
            unreachable!();
        };
        reader.read(
            &Sources::from_text(input),
            &ReaderOptions::default(),
            &mut context,
        )
    }

    #[test]
    fn counts_only_padding_cells() {
        let limits = Limits {
            max_table_cells: 1,
            ..Limits::default()
        };
        assert!(csv("a,b\n1,2,3\n4\n5,6", limits).is_ok());
        assert!(matches!(csv("a,b\n1\n2", limits), Err(Error::App(_))));
    }

    #[test]
    fn scans_extra_fields_for_errors() {
        assert_eq!(
            csv("a\n1,\"x\"bad", Limits::default())
                .unwrap_err()
                .to_string(),
            "Error at \"csv\" (line 2, column 6):\nunexpected 'b'\nexpecting white space or end of input"
        );
    }

    #[test]
    fn reports_empty_quoted_record_and_trailing_input() {
        for (input, expected) in [
            (
                "\"\"",
                "Error at \"csv\" (line 1, column 3):\nunexpected '\\n'",
            ),
            (
                "a,b\n\n1,2",
                "Error at \"csv\" (line 3, column 1):\nunexpected '1'\nexpecting end of input",
            ),
            (
                "\"a",
                "Error at \"csv\" (line 3, column 1):\nunexpected end of input",
            ),
        ] {
            let error = csv(input, Limits::default()).unwrap_err();
            assert_eq!(error.exit_code(), 64);
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn wide_and_ragged_records_fit_on_a_two_mebibyte_stack() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let input = format!("a{}\n1{}\n2", ",a".repeat(8_000), ",2".repeat(32_000));
                let mut limits = Limits {
                    max_table_cells: 8_000,
                    ..Limits::default()
                };
                assert!(csv(&input, limits).is_ok());
                limits.max_table_cells -= 1;
                assert!(matches!(csv(&input, limits), Err(Error::App(_))));
                limits.max_input_size = 8;
                assert!(matches!(csv(&input, limits), Err(Error::App(_))));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
