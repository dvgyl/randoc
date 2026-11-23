//! Perl Plain Old Documentation (POD) reader.

#![forbid(unsafe_code)]

mod inlines;

use std::sync::Arc;

use randoc_ast::{Attr, Block, Format, Inline, ListAttributes, Pandoc};
use randoc_core::{Context, Error, Extensions, Reader, ReaderOptions, Result, Sources, Writer};

const HORIZONTAL_SPACE: [char; 2] = [' ', '\t'];
const POD_SPACE: [char; 3] = [' ', '\t', '\n'];

/// Returns the reader for a supported format.
pub fn reader(format: &str) -> Option<Reader> {
    (format == "pod").then(|| Reader::Text(Arc::new(read)))
}

/// Returns the writer for a supported format.
pub fn writer(_format: &str) -> Option<Writer> {
    None
}

fn read(sources: &Sources, options: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    let mut remaining = context.limits.max_input_size;
    let mut input = String::new();
    let mut names = Vec::new();
    for source in &sources.0 {
        let size = source.text.len() as u64 + u64::from(!source.text.ends_with('\n')) + 1;
        remaining = remaining
            .checked_sub(size)
            .ok_or_else(|| Error::App("the POD document exceeds the input size limit".into()))?;
        names.push((input.len(), source.name.as_str()));
        let text = source.text.strip_prefix('\u{feff}').unwrap_or(&source.text);
        input.extend(text.chars().filter(|&ch| ch != '\r'));
        if !input.ends_with('\n') {
            input.push('\n');
        }
        input.push('\n');
    }
    let parser = Parser {
        input: &input,
        names,
        lines: input.lines().collect(),
        pos: 0,
        active: false,
        frames: vec![Frame::new(Region::Document)],
        limit: context.limits.max_nesting_depth,
        extensions: options.extensions,
    };
    Ok(Pandoc {
        blocks: parser.parse()?,
        ..Pandoc::default()
    })
}

fn class(name: &str) -> Attr {
    Attr::new("", vec![name.into()], Vec::new())
}

fn depth_error() -> Error {
    Error::App("the POD document is nested too deeply".into())
}

#[derive(PartialEq)]
enum Region<'a> {
    Document,
    List,
    Div(&'a str),
}

struct Item {
    term: Vec<Inline>,
    marker: Marker,
    blocks: Vec<Block>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Marker {
    Empty,
    Bullet,
    Number,
    One,
    Term,
}

struct Frame<'a> {
    region: Region<'a>,
    blocks: Vec<Block>,
    items: Vec<Item>,
}

impl<'a> Frame<'a> {
    fn new(region: Region<'a>) -> Self {
        Self {
            region,
            blocks: Vec::new(),
            items: Vec::new(),
        }
    }

    fn blocks(&mut self) -> &mut Vec<Block> {
        self.items
            .last_mut()
            .map_or(&mut self.blocks, |item| &mut item.blocks)
    }

    fn into_block(self) -> Block {
        match self.region {
            Region::Div(format) => Block::Div(
                class(format.strip_prefix(':').unwrap_or(format)),
                self.blocks,
            ),
            Region::List if !self.items.is_empty() => list(self.items),
            _ => Block::BlockQuote(self.blocks),
        }
    }
}

fn list(items: Vec<Item>) -> Block {
    if items
        .iter()
        .all(|item| matches!(item.marker, Marker::Empty | Marker::Bullet))
    {
        return Block::BulletList(items.into_iter().map(|item| item.blocks).collect());
    }
    if items.first().is_some_and(|item| item.marker == Marker::One)
        && items
            .iter()
            .all(|item| matches!(item.marker, Marker::One | Marker::Number))
    {
        return Block::OrderedList(
            ListAttributes::default(),
            items.into_iter().map(|item| item.blocks).collect(),
        );
    }
    Block::DefinitionList(
        items
            .into_iter()
            .map(|mut item| {
                if item.marker == Marker::Empty
                    && matches!(item.blocks.first(), Some(Block::Para(_)))
                    && let Block::Para(parts) = item.blocks.remove(0)
                {
                    item.term = parts;
                }
                (
                    vec![Inline::Span(Attr::default(), item.term)],
                    vec![item.blocks],
                )
            })
            .collect(),
    )
}

struct Parser<'a> {
    input: &'a str,
    names: Vec<(usize, &'a str)>,
    lines: Vec<&'a str>,
    pos: usize,
    active: bool,
    frames: Vec<Frame<'a>>,
    limit: usize,
    extensions: Extensions,
}

impl<'a> Parser<'a> {
    fn parse(mut self) -> Result<Vec<Block>> {
        while let Some(&line) = self.lines.get(self.pos) {
            let initial = line.strip_prefix('=').and_then(|tail| tail.chars().next());
            if initial.is_some_and(|ch| ch.is_ascii_alphabetic()) {
                self.active = true;
                self.command(line)?;
            } else if initial.is_some_and(char::is_alphabetic) {
                return Err(self.unexpected(self.pos, 1, Some("Pod command")));
            } else if !self.active || blank(line) {
                self.pos += 1;
            } else if line.starts_with([' ', '\t']) {
                let block = self.verbatim();
                self.block(block);
            } else {
                let text = self.paragraph();
                let mut parts = self.inline(text.trim_end_matches(POD_SPACE), false)?;
                if matches!(parts.last(), Some(Inline::Space)) {
                    parts.pop();
                }
                if matches!(parts.first(), Some(Inline::Space)) {
                    parts.remove(0);
                }
                self.block(Block::Para(parts));
            }
        }
        if self.frames.len() > 1 {
            return Err(self.diagnostic(self.pos, 0, "end of input", Some("Pod paragraph")));
        }
        Ok(self
            .frames
            .pop()
            .map_or_else(Vec::new, |frame| frame.blocks))
    }

    fn block(&mut self, block: Block) {
        if let Some(frame) = self.frames.last_mut() {
            frame.blocks().push(block);
        }
    }

    fn enter(&mut self, region: Region<'a>) -> Result<()> {
        if self.frames.len() > self.limit {
            return Err(depth_error());
        }
        self.frames.push(Frame::new(region));
        Ok(())
    }

    fn leave(&mut self) {
        if let Some(frame) = self.frames.pop() {
            self.block(frame.into_block());
        }
    }

    fn paragraph(&mut self) -> String {
        let start = self.pos;
        while self.lines.get(self.pos).is_some_and(|line| !blank(line)) {
            self.pos += 1;
        }
        self.lines[start..self.pos].join("\n")
    }

    fn verbatim(&mut self) -> Block {
        let mut text = String::new();
        loop {
            while let Some(&line) = self.lines.get(self.pos).filter(|line| !blank(line)) {
                text.push_str(line);
                text.push('\n');
                self.pos += 1;
            }
            let start = self.pos;
            self.skip_blank();
            if !self
                .lines
                .get(self.pos)
                .is_some_and(|line| line.starts_with([' ', '\t']))
            {
                break;
            }
            for _ in start..self.pos {
                text.push('\n');
            }
        }
        Block::CodeBlock(Attr::default(), text)
    }

    fn skip_blank(&mut self) {
        while self.lines.get(self.pos).is_some_and(|line| blank(line)) {
            self.pos += 1;
        }
    }

    fn command(&mut self, line: &'a str) -> Result<()> {
        let (command, tail) = line[1..]
            .split_once(HORIZONTAL_SPACE)
            .unwrap_or((&line[1..], ""));
        match command {
            "pod" | "encoding" | "cut" => {
                self.active = command != "cut";
                self.paragraph();
            }
            "head1" | "head2" | "head3" | "head4" | "head5" | "head6" => {
                let level = i64::from(command.as_bytes()[4] - b'0');
                let tail = tail.trim_start_matches(HORIZONTAL_SPACE);
                let text = if tail.is_empty() {
                    self.pos += 1;
                    let blank_line = self.lines.get(self.pos).is_some_and(|line| blank(line));
                    if blank_line {
                        self.pos += 1;
                    }
                    let paragraph = self.paragraph();
                    if blank_line {
                        format!(" {paragraph}")
                    } else {
                        paragraph
                    }
                } else {
                    let paragraph = self.paragraph();
                    paragraph[line.len() - tail.len()..].into()
                };
                let parts = self.inline(text.trim_end_matches('\n'), false)?;
                let parts = if text.is_empty() {
                    vec![Inline::Space]
                } else {
                    parts
                };
                self.block(Block::Header(level, Attr::default(), parts));
            }
            "over" => {
                self.end_command_line()?;
                self.enter(Region::List)?;
            }
            "item" => self.item(line, tail)?,
            "back" => {
                if !matches!(
                    self.frames.last().map(|frame| &frame.region),
                    Some(Region::List)
                ) {
                    return Err(self.diagnostic(self.pos, 5, "\"back\"", Some("Pod paragraph")));
                }
                self.end_command_line()?;
                self.leave();
            }
            "begin" => self.begin(line, tail)?,
            "for" => self.for_block(line, tail)?,
            "end" => {
                if let Some(Frame {
                    region: Region::Div(format),
                    ..
                }) = self.frames.last()
                {
                    self.check_end_format(line, tail, format)?;
                    self.end_command_line()?;
                    self.leave();
                } else {
                    return Err(self.diagnostic(self.pos, 4, "\"end\"", Some("Pod paragraph")));
                }
            }
            _ => return Err(self.unknown_command(command)),
        }
        Ok(())
    }

    fn end_command_line(&mut self) -> Result<()> {
        self.pos += 1;
        if self.lines.get(self.pos).is_some_and(|line| !blank(line)) {
            return Err(self.unexpected(self.pos, 0, None));
        }
        Ok(())
    }

    fn item(&mut self, line: &str, tail: &str) -> Result<()> {
        let start = self.pos;
        let Some(frame) = self.frames.last() else {
            return Err(depth_error());
        };
        if frame.region != Region::List {
            return Err(self.diagnostic(start, 5, "\"item\"", Some("Pod paragraph")));
        }
        if !frame.blocks.is_empty() {
            return Err(self.unexpected(start, 1, None));
        }
        let paragraph = self.paragraph();
        let raw = paragraph[command_length(line, tail)..].trim_matches(POD_SPACE);
        let digits = raw.strip_suffix('.').unwrap_or(raw);
        let marker = match raw {
            "" => Marker::Empty,
            "*" => Marker::Bullet,
            "1" | "1." => Marker::One,
            _ if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) => {
                Marker::Number
            }
            _ => Marker::Term,
        };
        let term = self.inline(raw, true)?;
        if let Some(frame) = self.frames.last_mut() {
            frame.items.push(Item {
                term,
                marker,
                blocks: Vec::new(),
            });
        }
        Ok(())
    }

    fn begin(&mut self, line: &str, tail: &'a str) -> Result<()> {
        let format = self.read_format(line, tail)?;
        self.end_command_line()?;
        self.skip_blank();
        if format.starts_with(':') {
            return self.enter(Region::Div(format));
        }
        let start = self.pos;
        while let Some(&line) = self.lines.get(self.pos) {
            if let Some(tail) = line.strip_prefix("=end") {
                let tail = tail
                    .strip_prefix(HORIZONTAL_SPACE)
                    .ok_or_else(|| self.unexpected(self.pos, "=end".len(), None))?;
                self.check_end_format(line, tail, format)?;
                let mut raw = self.lines[start..self.pos].join("\n");
                if start < self.pos {
                    raw.push('\n');
                }
                self.end_command_line()?;
                self.block(Block::RawBlock(Format::new(format), raw));
                return Ok(());
            }
            self.pos += 1;
        }
        Err(self.diagnostic(self.pos, 0, "end of input", None))
    }

    fn for_block(&mut self, line: &str, tail: &str) -> Result<()> {
        let start = self.pos;
        let format = self.read_format(line, tail)?;
        let column = line.len() - tail.trim_start_matches(HORIZONTAL_SPACE).len() + format.len();
        let paragraph = self.paragraph();
        let raw = &paragraph[column..];
        if let Some(name) = format.strip_prefix(':') {
            let text = raw.trim_matches(POD_SPACE);
            if text.is_empty() {
                return Err(self.unexpected(start, column, None));
            }
            if self.frames.len() > self.limit {
                return Err(depth_error());
            }
            let parts = self.inline(text, true)?;
            self.block(Block::Div(class(name), vec![Block::Para(parts)]));
        } else {
            self.block(Block::RawBlock(Format::new(format), format!("{raw}\n")));
        }
        Ok(())
    }

    fn read_format<'b>(&self, line: &str, tail: &'b str) -> Result<&'b str> {
        let tail = tail.trim_start_matches(HORIZONTAL_SPACE);
        let name = tail.strip_prefix(':').unwrap_or(tail);
        let length = name
            .find(|ch: char| !(ch.is_alphanumeric() || matches!(ch, '-' | '_')))
            .unwrap_or(name.len());
        if length == 0 {
            return Err(self.unexpected(self.pos, line.len() - name.len(), None));
        }
        Ok(&tail[..tail.len() - name.len() + length])
    }

    fn check_end_format(&self, line: &str, tail: &str, format: &str) -> Result<()> {
        if let Some(rest) = tail.strip_prefix(format) {
            let rest = rest.trim_start_matches(HORIZONTAL_SPACE);
            if rest.is_empty() {
                return Ok(());
            }
            return Err(self.unexpected(self.pos, line.len() - rest.len(), None));
        }
        let common = common_prefix_length(tail, format);
        Err(self.unexpected(self.pos, line.len() - tail.len() + common, None))
    }

    fn inline(&self, text: &str, extra_depth: bool) -> Result<Vec<Inline>> {
        let limit = self
            .limit
            .saturating_sub(self.frames.len() - 1 + usize::from(extra_depth));
        inlines::parse(text, limit, self.extensions).map_err(|error| match error {
            inlines::InlineError::Unclosed => {
                self.diagnostic(self.pos + 1, 0, "'\\n'", Some("Pod paragraph"))
            }
            inlines::InlineError::Parse(error) => error,
        })
    }

    fn unknown_command(&self, command: &str) -> Error {
        let commands = [
            "pod", "encoding", "cut", "head1", "head2", "head3", "head4", "head5", "head6", "over",
            "item", "back", "begin", "for", "end",
        ];
        let longest = commands
            .into_iter()
            .map(|known| common_prefix_length(known, command))
            .max()
            .unwrap_or(0);
        self.unexpected(
            self.pos,
            1 + longest,
            (longest == 0).then_some("Pod command"),
        )
    }

    fn unexpected(&self, line: usize, offset: usize, expected: Option<&str>) -> Error {
        let text = self.lines.get(line).copied().unwrap_or("");
        let character = text[text.floor_char_boundary(offset.min(text.len()))..]
            .chars()
            .next()
            .unwrap_or('\n');
        self.diagnostic(line, offset, &quoted_character(character), expected)
    }

    fn diagnostic(
        &self,
        line: usize,
        offset: usize,
        unexpected: &str,
        expected: Option<&str>,
    ) -> Error {
        let text = self.lines.get(line).copied().unwrap_or("");
        let offset = text.floor_char_boundary(offset.min(text.len()));
        let column = text[..offset].chars().count() + 1;
        let start = self
            .lines
            .iter()
            .take(line)
            .map(|line| line.len() + 1)
            .sum::<usize>()
            .min(self.input.len());
        let source = self
            .names
            .partition_point(|(position, _)| *position <= start);
        let (source_start, name) = source.checked_sub(1).map_or((0, ""), |i| self.names[i]);
        let row = line + 1
            - self.input[..source_start]
                .bytes()
                .filter(|&byte| byte == b'\n')
                .count();
        let name = if name.is_empty() {
            String::new()
        } else {
            format!("{name:?} ")
        };
        let mut message =
            format!("Error at {name}(line {row}, column {column}):\nunexpected {unexpected}");
        if let Some(expected) = expected {
            message.push_str("\nexpecting ");
            message.push_str(expected);
        }
        message.push_str(&format!("\n{text}\n{:>column$}", "^"));
        Error::Parse(message)
    }
}

fn command_length(line: &str, tail: &str) -> usize {
    line.len() - tail.len()
}

fn common_prefix_length(left: &str, right: &str) -> usize {
    left.bytes()
        .zip(right.bytes())
        .take_while(|(a, b)| a == b)
        .count()
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

fn blank(line: &str) -> bool {
    line.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn pod(input: &str, depth: usize) -> Result<Pandoc> {
        let mut context = Context::pure();
        context.limits.max_nesting_depth = depth;
        read(
            &Sources::from_text(input),
            &ReaderOptions::default(),
            &mut context,
        )
    }

    #[test]
    fn keeps_nested_codes_and_literal_angles() {
        let doc = pod("=pod\n\nB<< I<word> and <x> >>", 2).unwrap();
        assert_eq!(
            doc.blocks,
            vec![Block::Para(vec![Inline::Strong(vec![
                Inline::Emph(vec![Inline::Str("word".into())]),
                Inline::Space,
                Inline::Str("and".into()),
                Inline::Space,
                Inline::Str("<x>".into()),
            ])])]
        );
    }

    #[test]
    fn treats_equals_without_a_letter_as_text() {
        let doc = pod(
            "=pod\n\n= foo\n\n=1x\n\n=\n\n=_x\n\n=cut\n\n= not pod\n\n==x\n\n=1x\n\n=\n\n=_x\n",
            128,
        )
        .unwrap();
        assert_eq!(
            doc.blocks,
            vec![
                Block::Para(vec![
                    Inline::Str("=".into()),
                    Inline::Space,
                    Inline::Str("foo".into())
                ]),
                Block::Para(vec![Inline::Str("=1x".into())]),
                Block::Para(vec![Inline::Str("=".into())]),
                Block::Para(vec![Inline::Str("=_x".into())]),
            ]
        );
    }

    #[test]
    fn classifies_item_markers_without_converting_numbers() {
        for marker in ["+2", "7..", "2\u{a0}"] {
            let input = format!("=over\n\n=item 1\n\nx\n\n=item {marker}\n\ny\n\n=back\n");
            let doc = pod(&input, 128).unwrap();
            assert!(
                matches!(&doc.blocks[..], [Block::DefinitionList(_)]),
                "{marker}: {:?}",
                doc.blocks
            );
        }
        for marker in ["99999999999999999999", "02", "2."] {
            let input = format!("=over\n\n=item 1\n\nx\n\n=item {marker}\n\ny\n\n=back\n");
            let doc = pod(&input, 128).unwrap();
            assert!(
                matches!(&doc.blocks[..], [Block::OrderedList(_, _)]),
                "{marker}: {:?}",
                doc.blocks
            );
        }
    }

    #[test]
    fn resolves_entities_from_parsed_content() {
        for (input, expected) in [
            ("E< lt >", "<"),
            ("E<I<lt>>", "<"),
            ("E<< foo >>", "E<foo>"),
            ("E<foo  bar>", "E<foo bar>"),
            ("E<B<x>>", "E<x>"),
            ("E<lt >", "E<lt >"),
            ("E<0>", "\u{fffd}"),
            ("E<00>", "\u{fffd}"),
            ("E<0x0>", "\u{fffd}"),
            ("E<0x00>", "\u{fffd}"),
        ] {
            let doc = pod(&format!("=pod\n\n{input}\n"), 128).unwrap();
            assert_eq!(
                doc.blocks,
                vec![Block::Para(vec![Inline::Str(expected.into())])],
                "{input}"
            );
        }
    }

    #[test]
    fn reports_unknown_codes_before_unclosed_delimiters() {
        for input in ["Y<bar", "B<Y<bar", "Y<Q<bar>>"] {
            assert_parse_error(
                &format!("=pod\n\n{input}\n"),
                "unknown Pod formatting code Y",
            );
        }
    }

    #[test]
    fn reports_bare_end_in_raw_block() {
        assert_parse_error(
            "=begin html\n\nx\n\n=end\n",
            "Error at (line 5, column 5):\nunexpected '\\n'\n=end\n    ^",
        );
    }

    #[test]
    fn reports_the_first_mismatched_character_in_end_format() {
        assert_parse_error(
            "=begin :note\n\nx\n\n=end :other\n",
            "Error at (line 5, column 7):\nunexpected 'o'\n=end :other\n      ^",
        );
        assert_parse_error(
            "=begin :note\n\nx\n\n=end :note extra\n",
            "Error at (line 5, column 12):\nunexpected 'e'\n=end :note extra\n           ^",
        );
    }

    #[test]
    fn preserves_non_ascii_whitespace_and_escapes_error_characters() {
        let doc = pod("=pod\n\nx\u{a0}\u{3000}\n", 128).unwrap();
        assert_eq!(
            doc.blocks,
            vec![Block::Para(vec![Inline::Str("x\u{a0}\u{3000}".into())])]
        );
        let doc = pod("=head1 \u{a0}Title\n\n=for html\u{a0} x\n", 128).unwrap();
        assert_eq!(
            doc.blocks,
            vec![
                Block::Header(1, Attr::default(), vec![Inline::Str("\u{a0}Title".into())]),
                Block::RawBlock(Format::new("html"), "\u{a0} x\n".into()),
            ]
        );
        assert_parse_error(
            "=head1\u{a0}Title\n",
            "Error at (line 1, column 7):\nunexpected '\\160'\n=head1\u{a0}Title\n      ^",
        );
        assert_parse_error(
            "=pod\n\n=éx\n",
            "Error at (line 3, column 2):\nunexpected '\\233'\nexpecting Pod command\n=éx\n ^",
        );
    }

    fn assert_parse_error(input: &str, expected: &str) {
        let error = pod(input, 128).unwrap_err();
        assert_eq!(error.exit_code(), 64);
        assert_eq!(error.to_string(), expected);
    }

    #[test]
    fn rejects_unclosed_openers_in_one_scan() {
        let input = format!("=pod\n\n{}text", "B<< ".repeat(30_000));
        let error = pod(&input, 128).unwrap_err();
        assert_eq!(error.exit_code(), 64);
        assert_eq!(
            error.to_string(),
            "Error at (line 5, column 1):\nunexpected '\\n'\nexpecting Pod paragraph\n\n^"
        );
    }

    #[test]
    fn limits_input_size() {
        let mut context = Context::pure();
        context.limits.max_input_size = 32;
        let input = Sources::from_text(format!("=head1 {}", "x".repeat(32)));
        assert!(matches!(
            read(&input, &ReaderOptions::default(), &mut context),
            Err(Error::App(_))
        ));
    }

    #[test]
    fn nested_blocks_and_inlines_fit_on_a_two_mebibyte_stack() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let input = format!(
                    "{}{}word{}\n\n{}",
                    "=over\n\n".repeat(160),
                    "B<".repeat(160),
                    ">".repeat(160),
                    "=back\n\n".repeat(160),
                );
                assert!(pod(&input, 320).is_ok());
                assert!(matches!(pod(&input, 319), Err(Error::App(_))));
                let invalid = format!("{input}=over\n\n");
                assert!(matches!(pod(&invalid, 320), Err(Error::Parse(_))));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
