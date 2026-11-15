//! Literal inline token boundaries used by the block and inline scans.

use std::cmp::Reverse;
use std::collections::HashMap;

use randoc_core::{Extension, Extensions};

use crate::block::{Ln, starts_comment_or_pi};

/// A line index and a byte offset in that line.
pub(crate) type Position = (usize, usize);
type Stored = (usize, Reverse<usize>);

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Mark {
    Ticks(usize),
    Byte(u8),
    Unescaped(u8),
    Dollar,
    DoubleDollar,
    CommentEnd,
    MathEnd(usize, u8),
    TitleEnd(u8),
    Visible,
}

/// The active inline syntax and the blank-line rule of the calling block.
#[derive(Clone, Copy, Default)]
pub(crate) struct TokenRules {
    pub math: bool,
    pub raw_html: bool,
    pub comment_over_blank: bool,
    pub single_backslash_math: bool,
    pub double_backslash_math: bool,
    pub attributes: bool,
    pub all_symbols_escapable: bool,
}

impl TokenRules {
    /// Takes the token syntax from the reader extensions.
    pub(crate) fn from_extensions(exts: Extensions, comment_over_blank: bool) -> Self {
        Self {
            math: exts.contains(Extension::TexMathDollars),
            raw_html: exts.contains(Extension::RawHtml),
            comment_over_blank,
            single_backslash_math: exts.contains(Extension::TexMathSingleBackslash),
            double_backslash_math: exts.contains(Extension::TexMathDoubleBackslash),
            attributes: exts.contains(Extension::BracketedSpans)
                || exts.contains(Extension::InlineCodeAttributes)
                || exts.contains(Extension::LinkAttributes),
            all_symbols_escapable: exts.contains(Extension::AllSymbolsEscapable),
        }
    }
}

/// The lines of a slice, starting at line index `at`.
#[derive(Clone, Copy)]
pub(crate) struct View<'a> {
    pub lines: &'a [Ln],
    pub at: usize,
}

impl<'a> View<'a> {
    fn text(self, j: usize) -> Option<&'a str> {
        self.lines.get(j.checked_sub(self.at)?).map(Ln::t)
    }

    fn byte(self, (j, i): Position) -> Option<u8> {
        self.text(j)?.as_bytes().get(i).copied()
    }

    fn stored(self, (j, i): Position) -> Stored {
        (
            j,
            Reverse(self.text(j).map_or(0, str::len).saturating_sub(i)),
        )
    }

    fn position(self, (j, Reverse(rest)): Stored) -> Position {
        (j, self.text(j).map_or(0, str::len).saturating_sub(rest))
    }

    fn advance(self, (j, i): Position, n: usize) -> Position {
        (j, i + n)
    }

    /// The index line after the last line of the view.
    fn end(self) -> usize {
        self.at + self.lines.len()
    }
}

/// Delimiter positions read once from a line slice. Failed lookups share the
/// same index, so an unmatched opener does not read the remaining text again.
/// A view of a part of the slice shares the index: an answer past the end
/// of the view is no answer for it.
#[derive(Default)]
pub(crate) struct TokenIndex {
    start: usize,
    read: usize,
    marks: HashMap<Mark, Vec<Stored>>,
    blanks: Vec<usize>,
    parens: HashMap<Stored, Stored>,
    paren_stack: Vec<Stored>,
    /// The unescaped quotes with only white space after them up to the
    /// lines read so far: the first visible byte after them decides whether
    /// they end a link title.
    open_titles: Vec<(u8, Stored)>,
}

impl TokenIndex {
    fn prepare(&mut self, view: View<'_>) {
        if view.at < self.start || self.read < view.at {
            *self = Self {
                start: view.at,
                read: view.at,
                ..Self::default()
            };
        }
    }

    fn push(&mut self, mark: Mark, at: Stored) {
        self.marks.entry(mark).or_default().push(at);
    }

    fn read_line(&mut self, view: View<'_>) -> bool {
        let Some(t) = view.text(self.read) else {
            return false;
        };
        let j = self.read;
        self.read += 1;
        if t.trim_matches([' ', '\t', '\r']).is_empty() {
            self.blanks.push(j);
        }
        let bytes = t.as_bytes();
        let at = |i| (j, Reverse(t.len() - i));
        if let Some(first) = bytes.iter().find(|b| !b.is_ascii_whitespace()) {
            for (q, quote) in std::mem::take(&mut self.open_titles) {
                if *first == b')' {
                    self.push(Mark::TitleEnd(q), quote);
                }
            }
        }
        let mut i = 0;
        let mut slashes = 0usize;
        while let Some(&b) = bytes.get(i) {
            crate::step(1);
            let escaped = slashes % 2 == 1;
            self.push(Mark::Byte(b), at(i));
            if !escaped {
                self.push(Mark::Unescaped(b), at(i));
            }
            if !matches!(b, b' ' | b'\t' | b'\r') {
                self.push(Mark::Visible, at(i));
            }
            if b == b'`' {
                let n = bytes
                    .get(i..)
                    .unwrap_or(&[])
                    .iter()
                    .take_while(|c| **c == b'`')
                    .count();
                self.push(Mark::Ticks(n), at(i));
                // The byte index also serves quoted strings and link targets.
                for k in 1..n {
                    self.push(Mark::Byte(b), at(i + k));
                    self.push(Mark::Unescaped(b), at(i + k));
                    self.push(Mark::Visible, at(i + k));
                }
                crate::step(n.saturating_sub(1));
                i += n;
                slashes = 0;
                continue;
            }
            if b == b'$' && !escaped {
                self.push(Mark::Dollar, at(i));
                if bytes.get(i + 1) == Some(&b'$') {
                    self.push(Mark::DoubleDollar, at(i));
                }
            }
            if bytes.get(i..i + 3) == Some(b"-->") {
                self.push(Mark::CommentEnd, at(i));
            }
            if b == b'\\' {
                for n in [1, 2] {
                    if bytes
                        .get(i..i + n)
                        .is_some_and(|s| s.iter().all(|c| *c == b'\\'))
                        && let Some(&c @ (b')' | b']')) = bytes.get(i + n)
                    {
                        self.push(Mark::MathEnd(n, c), at(i));
                    }
                }
            }
            if !escaped {
                if matches!(b, b'"' | b'\'') {
                    let after = bytes.get(i + 1..).unwrap_or(&[]);
                    let gap = after.iter().take_while(|c| c.is_ascii_whitespace()).count();
                    crate::step(gap);
                    match after.get(gap) {
                        Some(b')') => self.push(Mark::TitleEnd(b), at(i)),
                        None => self.open_titles.push((b, at(i))),
                        Some(_) => {}
                    }
                }
                if b == b'(' {
                    self.paren_stack.push(at(i));
                }
                if b == b')'
                    && let Some(open) = self.paren_stack.pop()
                {
                    self.parens.insert(open, at(i));
                }
            }
            slashes = if b == b'\\' { slashes + 1 } else { 0 };
            i += 1;
        }
        true
    }

    fn next(
        &mut self,
        view: View<'_>,
        from: Position,
        mark: Mark,
        stop_at_blank: bool,
    ) -> Option<Position> {
        self.prepare(view);
        let after = view.stored(from);
        let result = loop {
            let list = self.marks.get(&mark).map_or(&[][..], Vec::as_slice);
            let k = list.partition_point(|p| *p < after);
            if let Some(&p) = list.get(k) {
                let blank = self.blanks.partition_point(|b| *b <= from.0);
                if p.0 >= view.end()
                    || (stop_at_blank && self.blanks.get(blank).is_some_and(|b| *b < p.0))
                {
                    break None;
                }
                let found = view.position(p);
                // A title end that a shorter view shares can have its
                // parenthesis after the end of the view.
                if matches!(mark, Mark::TitleEnd(_))
                    && self
                        .skip_space(view, view.advance(found, 1))
                        .is_none_or(|after| view.byte(after) != Some(b')'))
                {
                    break None;
                }
                break Some(found);
            }
            if stop_at_blank && self.blanks.last().is_some_and(|b| *b > from.0) {
                break None;
            }
            if !self.read_line(view) {
                break None;
            }
        };
        #[cfg(test)]
        crate::check_cache(&result, || plain_next(view, from, mark, stop_at_blank));
        result
    }

    /// Reads the lines of `view` up to line `j`.
    pub(crate) fn read_through(&mut self, view: View<'_>, j: usize) {
        self.prepare(view);
        while self.read <= j && self.read_line(view) {}
    }

    /// Returns the first line that the index has not read.
    pub(crate) fn frontier(&self) -> usize {
        self.read
    }

    /// Returns the exclusive end of a literal token at `at`.
    pub(crate) fn end(
        &mut self,
        view: View<'_>,
        at: Position,
        rules: TokenRules,
    ) -> Option<Position> {
        self.prepare(view);
        let (j, i) = at;
        let bytes = view.text(j)?.as_bytes();
        match *bytes.get(i)? {
            b'`' => {
                let n = bytes.get(i..)?.iter().take_while(|b| **b == b'`').count();
                for len in (1..=n).rev() {
                    if let Some(end) = self.next(view, (j, i + n), Mark::Ticks(len), true) {
                        let end = view.advance(end, len);
                        return Some(self.after_attributes(view, end, rules));
                    }
                }
                Some((j, i + n))
            }
            b'$' if rules.math => {
                let double = bytes.get(i + 1) == Some(&b'$');
                let n = if double { 2 } else { 1 };
                if !double && bytes.get(i + 1).is_none_or(|b| matches!(b, b' ' | b'\t')) {
                    return None;
                }
                let mark = if double {
                    Mark::DoubleDollar
                } else {
                    Mark::Dollar
                };
                let end = self.next(view, (j, i + n), mark, true)?;
                if double {
                    let visible = self.next(view, (j, i + n), Mark::Visible, false)?;
                    if visible >= end {
                        return None;
                    }
                } else {
                    let text = view.text(end.0)?.as_bytes();
                    if end.1 == 0
                        || matches!(text.get(end.1 - 1), None | Some(b' ' | b'\t'))
                        || text.get(end.1 + 1).is_some_and(u8::is_ascii_digit)
                    {
                        return None;
                    }
                }
                Some(view.advance(end, n))
            }
            b'\\' => {
                let n = if rules.double_backslash_math
                    && bytes.get(i + 1) == Some(&b'\\')
                    && matches!(bytes.get(i + 2), Some(b'(' | b'['))
                {
                    2
                } else if rules.single_backslash_math
                    && matches!(bytes.get(i + 1), Some(b'(' | b'['))
                {
                    1
                } else {
                    return None;
                };
                let close = if bytes.get(i + n) == Some(&b'(') {
                    b')'
                } else {
                    b']'
                };
                let end = self.next(view, (j, i + n + 1), Mark::MathEnd(n, close), true)?;
                Some(view.advance(end, n + 1))
            }
            b'<' if rules.raw_html => {
                let rest = view.text(j)?.get(i..)?;
                if starts_comment_or_pi(rest) {
                    let comment = rest.starts_with("<!--");
                    let n = if comment { 4 } else { 2 };
                    let mark = if comment {
                        Mark::CommentEnd
                    } else {
                        Mark::Byte(b'>')
                    };
                    let end = self.next(view, (j, i + n), mark, !rules.comment_over_blank)?;
                    Some(view.advance(end, if comment { 3 } else { 1 }))
                } else {
                    None
                }
            }
            b']' if bytes.get(i + 1) == Some(&b'(') => self.link_end(view, at, rules),
            b'{' if rules.attributes
                && i > 0
                && matches!(bytes.get(i - 1), Some(b']' | b')' | b'`')) =>
            {
                self.attributes_end(view, at, rules)
            }
            _ => None,
        }
    }

    fn after_attributes(&mut self, view: View<'_>, end: Position, rules: TokenRules) -> Position {
        if rules.attributes && view.byte(end) == Some(b'{') {
            self.attributes_end(view, end, rules).unwrap_or(end)
        } else {
            end
        }
    }

    /// Returns the exclusive end of an HTML tag, including quoted values.
    pub(crate) fn tag_end(&mut self, view: View<'_>, at: Position) -> Option<Position> {
        let rest = view.text(at.0)?.get(at.1..)?;
        let body = rest
            .strip_prefix('<')?
            .strip_prefix('/')
            .unwrap_or(rest.get(1..)?);
        let name_end = body
            .char_indices()
            .take_while(|(_, c)| c.is_alphanumeric() || matches!(c, '-' | ':'))
            .last()
            .map_or(0, |(i, c)| i + c.len_utf8());
        let name = body.get(..name_end)?;
        if !name.starts_with(char::is_alphabetic)
            || name.ends_with(':')
            || !matches!(
                body.get(name_end..)?.chars().next(),
                None | Some(' ' | '>' | '/' | '\n')
            )
        {
            return None;
        }
        let mut from = view.advance(at, 1);
        loop {
            let gt = self.next(view, from, Mark::Byte(b'>'), false)?;
            let lt = self.next(view, from, Mark::Byte(b'<'), false);
            let dq = self.next(view, from, Mark::Byte(b'"'), false);
            let sq = self.next(view, from, Mark::Byte(b'\''), false);
            let quote = dq.into_iter().chain(sq).min();
            if let Some(q) = quote.filter(|q| *q < gt && lt.is_none_or(|lt| *q < lt)) {
                let mark = Mark::Byte(view.byte(q)?);
                let end = self.next(view, view.advance(q, 1), mark, false)?;
                from = view.advance(end, 1);
            } else if lt.is_some_and(|lt| lt < gt) {
                return None;
            } else {
                return Some(view.advance(gt, 1));
            }
        }
    }

    fn skip_space(&self, view: View<'_>, mut p: Position) -> Option<Position> {
        loop {
            let t = view.text(p.0)?;
            while t
                .as_bytes()
                .get(p.1)
                .is_some_and(|b| b.is_ascii_whitespace())
            {
                p.1 += 1;
                crate::step(1);
            }
            if p.1 < t.len() {
                return Some(p);
            }
            p = (p.0 + 1, 0);
        }
    }

    fn char_at(view: View<'_>, p: Position) -> Option<char> {
        view.text(p.0)?.get(p.1..)?.chars().next()
    }

    fn attributes_end(
        &mut self,
        view: View<'_>,
        at: Position,
        rules: TokenRules,
    ) -> Option<Position> {
        // A missing brace ends the attempt before the attribute grammar runs.
        self.next(view, view.advance(at, 1), Mark::Byte(b'}'), true)?;
        let ident =
            |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.') || !c.is_ascii();
        let mut p = view.advance(at, 1);
        loop {
            p = self.skip_space(view, p)?;
            let c = Self::char_at(view, p)?;
            match c {
                '}' => return Some(view.advance(p, 1)),
                '#' | '.' => {
                    p.1 += 1;
                    let start = p;
                    while let Some(c) = Self::char_at(view, p).filter(|c| ident(*c)) {
                        p.1 += c.len_utf8();
                        crate::step(1);
                    }
                    if p == start {
                        return None;
                    }
                }
                '-' if view
                    .byte(view.advance(p, 1))
                    .is_some_and(|b| b.is_ascii_whitespace() || b == b'}') =>
                {
                    p.1 += 1
                }
                _ => {
                    let start = p;
                    while let Some(c) = Self::char_at(view, p).filter(|c| ident(*c)) {
                        p.1 += c.len_utf8();
                        crate::step(1);
                    }
                    if p == start || view.byte(p) != Some(b'=') {
                        return None;
                    }
                    p.1 += 1;
                    if let Some(q @ (b'"' | b'\'')) = view.byte(p) {
                        let mark = if rules.all_symbols_escapable {
                            Mark::Unescaped(q)
                        } else {
                            Mark::Byte(q)
                        };
                        let end = self.next(view, view.advance(p, 1), mark, true)?;
                        p = view.advance(end, 1);
                    } else {
                        while let Some(c) =
                            Self::char_at(view, p).filter(|c| !c.is_whitespace() && *c != '}')
                        {
                            p.1 += c.len_utf8();
                            if c == '\\'
                                && let Some(c) =
                                    Self::char_at(view, p).filter(|c| c.is_ascii_punctuation())
                            {
                                p.1 += c.len_utf8();
                            }
                            crate::step(1);
                        }
                    }
                }
            }
        }
    }

    fn link_end(&mut self, view: View<'_>, at: Position, rules: TokenRules) -> Option<Position> {
        while self.read_line(view) {}
        let outer = view.advance(at, 1);
        let mut p = self.skip_space(view, view.advance(at, 2))?;
        if view.byte(p) != Some(b'<') && !self.parens.contains_key(&view.stored(outer)) {
            return None;
        }
        if view.byte(p) == Some(b'<') {
            let end = self.next(view, view.advance(p, 1), Mark::Byte(b'>'), true)?;
            if end.0 != p.0 {
                return None;
            }
            p = view.advance(end, 1);
        } else {
            let mut depth = 0usize;
            loop {
                let Some(c) = view.byte(p) else {
                    let next = self.skip_space(view, (p.0 + 1, 0))?;
                    if matches!(view.byte(next), Some(b'"' | b'\'' | b')')) {
                        p = next;
                        break;
                    }
                    p = next;
                    continue;
                };
                crate::step(1);
                match c {
                    b'\\' => p.1 += 2,
                    b'(' => {
                        depth += 1;
                        p.1 += 1;
                    }
                    b')' if depth == 0 => break,
                    b')' => {
                        depth -= 1;
                        p.1 += 1;
                    }
                    b' ' => {
                        let next = self.skip_space(view, p)?;
                        if matches!(view.byte(next), Some(b'"' | b'\'' | b')')) {
                            p = next;
                            break;
                        }
                        p = next;
                    }
                    _ => p.1 += 1,
                }
            }
        }
        p = self.skip_space(view, p)?;
        if let Some(q @ (b'"' | b'\'')) = view.byte(p) {
            let end = self.next(view, view.advance(p, 1), Mark::TitleEnd(q), true)?;
            p = self.skip_space(view, view.advance(end, 1))?;
        }
        if view.byte(p) != Some(b')') {
            return None;
        }
        let blank = self.blanks.partition_point(|j| *j <= at.0);
        if self.blanks.get(blank).is_some_and(|j| *j < p.0) {
            return None;
        }
        Some(self.after_attributes(view, view.advance(p, 1), rules))
    }

    /// Returns the last line reached by complete tokens that start on line `j`.
    pub(crate) fn reach(&mut self, view: View<'_>, mut j: usize, rules: TokenRules) -> usize {
        self.prepare(view);
        let mut i = 0;
        loop {
            let Some(t) = view.text(j) else {
                return j;
            };
            while i < t.len() {
                crate::step(1);
                if let Some(end) = self.end(view, (j, i), rules) {
                    (j, i) = end;
                    break;
                }
                if rules.raw_html
                    && t.as_bytes().get(i) == Some(&b'<')
                    && let Some(end) = self.tag_end(view, (j, i))
                {
                    (j, i) = end;
                    break;
                }
                i += if t.as_bytes().get(i) == Some(&b'\\') {
                    2
                } else {
                    1
                };
            }
            if view.text(j).is_none_or(|t| i >= t.len()) {
                return j;
            }
        }
    }
}

#[cfg(test)]
fn plain_next(view: View<'_>, from: Position, mark: Mark, stop_at_blank: bool) -> Option<Position> {
    for (offset, line) in view.lines.iter().enumerate() {
        let j = view.at + offset;
        if j < from.0 {
            continue;
        }
        let t = line.t();
        if j > from.0 && stop_at_blank && t.trim_matches([' ', '\t', '\r']).is_empty() {
            return None;
        }
        let bytes = t.as_bytes();
        let mut i = 0;
        let mut slashes = 0usize;
        while let Some(&b) = bytes.get(i) {
            let escaped = slashes % 2 == 1;
            let run = if b == b'`' {
                bytes
                    .get(i..)
                    .unwrap_or(&[])
                    .iter()
                    .take_while(|c| **c == b'`')
                    .count()
            } else {
                0
            };
            let matches = match mark {
                Mark::Ticks(n) => run == n && n > 0 && (i == 0 || bytes.get(i - 1) != Some(&b'`')),
                Mark::Byte(c) => b == c,
                Mark::Unescaped(c) => b == c && !escaped,
                Mark::Dollar => b == b'$' && !escaped,
                Mark::DoubleDollar => b == b'$' && bytes.get(i + 1) == Some(&b'$') && !escaped,
                Mark::CommentEnd => bytes.get(i..i + 3) == Some(b"-->"),
                Mark::MathEnd(n, c) => {
                    bytes
                        .get(i..i + n)
                        .is_some_and(|s| s.iter().all(|b| *b == b'\\'))
                        && bytes.get(i + n) == Some(&c)
                }
                Mark::TitleEnd(q) => {
                    b == q && !escaped && {
                        let mut p = (j, i + 1);
                        loop {
                            let Some(line) = view.text(p.0) else {
                                break false;
                            };
                            while line
                                .as_bytes()
                                .get(p.1)
                                .is_some_and(|b| b.is_ascii_whitespace())
                            {
                                p.1 += 1;
                            }
                            if p.1 < line.len() {
                                break view.byte(p) == Some(b')');
                            }
                            p = (p.0 + 1, 0);
                        }
                    }
                }
                Mark::Visible => !matches!(b, b' ' | b'\t' | b'\r'),
            };
            if (j, i) >= from && matches {
                return Some((j, i));
            }
            slashes = if b == b'\\' { slashes + 1 } else { 0 };
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_delimiters_match_plain_scans() {
        let parts = [
            "a", "`", "``", "```", "$", "$$", "\\", "\\)", "\\]", "<!--", "-->", "<b", "</div>",
            "\\\\)", "\\\\]", "\\\"", "\"", "'", "(", ")", "{", "}", " ", "\n", "\n\n",
        ];
        let mut state = 20_261_115_u64;
        for _ in 0..5_000 {
            let mut text = String::new();
            for _ in 0..12 {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                text.push_str(parts[(state >> 33) as usize % parts.len()]);
            }
            let lines: Vec<Ln> = text
                .split('\n')
                .enumerate()
                .map(|(j, t)| Ln::new(t, j as u32))
                .collect();
            let view = View {
                lines: &lines,
                at: 0,
            };
            let mut index = TokenIndex::default();
            // A shorter view shares the index that the full view read.
            let shorter = View {
                lines: lines.get(..lines.len() - 1).unwrap_or(&[]),
                at: 0,
            };
            for (j, line) in lines.iter().enumerate() {
                for i in [0, line.t().len() / 2, line.t().len()] {
                    for mark in [
                        Mark::Ticks(1),
                        Mark::Ticks(2),
                        Mark::Ticks(3),
                        Mark::Dollar,
                        Mark::DoubleDollar,
                        Mark::CommentEnd,
                        Mark::Byte(b'>'),
                        Mark::Byte(b'"'),
                        Mark::Unescaped(b'"'),
                        Mark::MathEnd(1, b')'),
                        Mark::MathEnd(2, b']'),
                        Mark::TitleEnd(b'"'),
                        Mark::Visible,
                    ] {
                        for stop in [false, true] {
                            for v in [view, shorter] {
                                if j >= v.end() {
                                    continue;
                                }
                                let actual = index.next(v, (j, i), mark, stop);
                                let plain = plain_next(v, (j, i), mark, stop);
                                assert_eq!(
                                    actual,
                                    plain,
                                    "{text:?} at {j}:{i}, {mark:?}, blank {stop}, {} lines",
                                    v.lines.len()
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
