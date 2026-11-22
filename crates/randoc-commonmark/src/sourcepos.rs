//! Source coordinates for the block and inline parsers.

use std::fmt::Write;
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Point {
    pub(crate) line: usize,
    pub(crate) col: usize,
}

impl Point {
    pub(crate) fn next_line(self) -> Self {
        Self {
            line: self.line + 1,
            col: 1,
        }
    }
}

/// A block starts a new range each time a line starts after column one.
#[derive(Clone, Debug, Default)]
pub(crate) struct BlockPos {
    starts: Vec<Point>,
    end: Point,
    /// A setext underline or table divider has its own range before the block range.
    pub(crate) marker: Option<Point>,
}

impl BlockPos {
    pub(crate) fn add_line(&mut self, point: Point) {
        if self.end.line > point.line {
            return;
        }
        if self.starts.is_empty() || point.col > 1 {
            self.starts.push(point);
        }
        self.end = point.next_line();
    }

    pub(crate) fn finish_at(&mut self, line: usize) {
        self.starts.retain(|point| point.line < line);
        self.end = Point { line, col: 1 };
    }

    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        if let Some(marker) = self.marker {
            push_range(&mut out, marker, marker.next_line());
        }
        for (i, &start) in self.starts.iter().enumerate() {
            let end = self.starts.get(i + 1).map_or(self.end, |next| Point {
                line: next.line,
                col: 1,
            });
            push_range(&mut out, start, end);
        }
        out
    }
}

/// The origin of each line in the text passed to the inline parser.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Anchor {
    pub(crate) offset: usize,
    pub(crate) point: Point,
}

/// Source information for a table cell after the table parser removes pipe escapes.
#[derive(Debug)]
pub(crate) struct CellPos {
    pub(crate) anchor: Anchor,
    pub(crate) escapes: Vec<usize>,
}

#[derive(Clone, Copy)]
pub(crate) struct InlineSource<'a> {
    pub(crate) anchors: &'a [Anchor],
    pub(crate) origin: usize,
    pub(crate) escapes: &'a [usize],
}

/// Maps inline byte ranges to the original line and character columns.
pub(crate) struct InlinePos<'a> {
    lines: Vec<Anchor>,
    /// Empty for ASCII text without tabs. Otherwise indexed by byte offset.
    columns: Vec<usize>,
    text_start: usize,
    origin: usize,
    escapes: &'a [usize],
}

impl<'a> InlinePos<'a> {
    pub(crate) fn new(text: &str, source: InlineSource<'a>, trim: usize) -> Self {
        let InlineSource {
            anchors,
            origin,
            escapes,
        } = source;
        let lines: Vec<_> = anchors
            .iter()
            .copied()
            .filter(|a| a.offset >= origin && a.offset <= origin + text.len())
            .collect();
        let columns = if text.is_ascii() && !text.contains('\t') {
            Vec::new()
        } else {
            let mut columns = vec![0; text.len() + 1];
            let mut col = lines.first().map_or(1, |a| a.point.col);
            let mut anchors = lines.iter().peekable();
            for (offset, ch) in text.char_indices() {
                if anchors.peek().is_some_and(|a| a.offset == origin + offset)
                    && let Some(anchor) = anchors.next()
                {
                    col = anchor.point.col;
                }
                columns[offset] = col;
                col = match ch {
                    '\n' => 1,
                    '\t' => col + 4 - (col - 1) % 4,
                    _ => col + 1,
                };
            }
            columns[text.len()] = col;
            columns
        };
        Self {
            lines,
            columns,
            text_start: origin + trim,
            origin,
            escapes,
        }
    }

    pub(crate) fn render(&self, range: Range<usize>) -> String {
        if range.is_empty() {
            return String::new();
        }
        let start = self.text_start + range.start;
        let end = self.text_start + range.end;
        let Some(first) = self
            .lines
            .partition_point(|a| a.offset <= start)
            .checked_sub(1)
        else {
            return String::new();
        };
        let mut out = String::new();
        let mut line = self.lines[first];
        let mut from = self.point(line, start);
        for &next in &self.lines[first + 1..] {
            if next.offset >= end {
                break;
            }
            if next.point.col != 1 {
                push_range(&mut out, from, self.point(line, next.offset - 1));
                from = next.point;
            }
            line = next;
        }
        push_range(&mut out, from, self.point(line, end));
        out
    }

    fn point(&self, line: Anchor, offset: usize) -> Point {
        let escapes = self
            .escapes
            .partition_point(|&at| at <= offset - self.origin);
        Point {
            line: line.point.line,
            col: self
                .columns
                .get(offset - self.origin)
                .copied()
                .unwrap_or(line.point.col + offset - line.offset)
                + escapes,
        }
    }
}

fn push_range(out: &mut String, start: Point, end: Point) {
    if start == end {
        return;
    }
    if !out.is_empty() {
        out.push(';');
    }
    let _ = write!(out, "{}:{}-{}:{}", start.line, start.col, end.line, end.col);
}
