//! Helpers below the tree rules: the inline list with the join rules of
//! `pandoc-types`, the conversion of a text token to inlines, attributes and URLs.

use randoc_ast::{Attr, Format, Inline, MathType, QuoteType, Text};
use randoc_core::{Extension, Extensions};
use std::collections::HashMap;
use std::rc::Rc;

/// A list of inlines that joins neighbours as the `Inlines` monoid of
/// `pandoc-types` 1.23.1 does (`Text.Pandoc.Builder`, BSD-3-Clause).
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Inlines(pub(crate) Vec<Inline>);

impl Inlines {
    pub(crate) fn new() -> Self {
        Inlines(Vec::new())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Appends one inline and joins it with the last one where the monoid does.
    pub(crate) fn push(&mut self, inline: Inline) {
        let Some(last) = self.0.last_mut() else {
            self.0.push(inline);
            return;
        };
        match (last, inline) {
            (Inline::Space, Inline::Space)
            | (Inline::SoftBreak, Inline::Space | Inline::SoftBreak)
            | (Inline::LineBreak, Inline::Space | Inline::SoftBreak) => {}
            (last @ Inline::Space, Inline::SoftBreak) => *last = Inline::SoftBreak,
            (last @ (Inline::Space | Inline::SoftBreak), Inline::LineBreak) => {
                *last = Inline::LineBreak;
            }
            (Inline::Str(a), Inline::Str(b)) => a.push_str(&b),
            (Inline::Emph(a), Inline::Emph(b))
            | (Inline::Underline(a), Inline::Underline(b))
            | (Inline::Strong(a), Inline::Strong(b))
            | (Inline::Subscript(a), Inline::Subscript(b))
            | (Inline::Superscript(a), Inline::Superscript(b))
            | (Inline::Strikeout(a), Inline::Strikeout(b)) => a.extend(b),
            (_, inline) => self.0.push(inline),
        }
    }

    /// Appends a list. Only the two inlines at the border can join.
    pub(crate) fn append(&mut self, other: Inlines) {
        let mut rest = other.0.into_iter();
        if let Some(first) = rest.next() {
            self.push(first);
            self.0.extend(rest);
        }
    }

    /// Removes the spaces and soft breaks at both ends.
    pub(crate) fn trimmed(mut self) -> Self {
        while matches!(self.0.last(), Some(Inline::Space | Inline::SoftBreak)) {
            self.0.pop();
        }
        let lead = self
            .0
            .iter()
            .take_while(|inline| matches!(inline, Inline::Space | Inline::SoftBreak))
            .count();
        self.0.drain(..lead);
        self
    }

    /// Moves the space at each end out of an element, as Pandoc does for
    /// emphasis and links: `<b> x </b>` is a space, `Strong [x]` and a space.
    pub(crate) fn wrap_extracting_spaces(self, wrap: impl FnOnce(Vec<Inline>) -> Inline) -> Self {
        let edge = |inline: Option<&Inline>| match inline {
            Some(Inline::Space) => Some(Inline::Space),
            Some(Inline::SoftBreak) => Some(Inline::SoftBreak),
            _ => None,
        };
        let left = edge(self.0.first());
        let right = edge(self.0.last());
        let mut out = Inlines::new();
        if let Some(left) = left {
            out.push(left);
        }
        out.push(wrap(self.trimmed().0));
        if let Some(right) = right {
            out.push(right);
        }
        out
    }
}

/// The white space characters of HTML text for Pandoc. A form feed and a
/// no-break space are parts of a word.
pub(crate) fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

/// Returns true when the text has only white space.
pub(crate) fn is_blank(text: &str) -> bool {
    text.chars().all(is_space)
}

/// How a text token becomes inlines.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextMode {
    /// Words, spaces and soft breaks.
    Normal,
    /// Inside `pre`: a newline is a line break and a space is a no-break space.
    Pre,
    /// An attribute value: as `Normal`, but without `legacy_character`.
    Attribute,
}

/// Converts a text token to inlines.
pub(crate) fn text_inlines(text: &str, mode: TextMode, extensions: Extensions, out: &mut Inlines) {
    quoted_text_inlines(text, mode, extensions, None, out);
}

/// Converts a text token inside a quotation of the kind `context`. A
/// quotation mark of that kind does not open a quotation there.
pub(crate) fn quoted_text_inlines(
    text: &str,
    mode: TextMode,
    extensions: Extensions,
    context: Option<QuoteType>,
    out: &mut Inlines,
) {
    let chars: Rc<[char]> = text.chars().collect();
    let mut special = Special::new(extensions, Rc::clone(&chars), 0);
    special.context = context;
    special.mode = mode;
    convert(&chars, mode, &mut special, 0, out);
}

/// The largest number of quotations inside quotations that the `smart`
/// extension reads in one text. A deeper quotation mark is a plain mark.
const MAX_QUOTE_DEPTH: usize = 16;

/// The characters U+0080 to U+009F of text are the characters of Windows
/// code page 1252 with the same byte, as for Pandoc. A byte that the code page
/// does not define gives `?`.
fn legacy_character(c: char) -> char {
    #[rustfmt::skip]
    const CP1252: [char; 32] = [
        '\u{20ac}', '?', '\u{201a}', '\u{192}', '\u{201e}', '\u{2026}', '\u{2020}', '\u{2021}',
        '\u{2c6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '?', '\u{17d}', '?',
        '?', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}', '\u{2014}',
        '\u{2dc}', '\u{2122}', '\u{161}', '\u{203a}', '\u{153}', '?', '\u{17e}', '\u{178}',
    ];
    match u32::from(c).checked_sub(0x80) {
        Some(offset) => CP1252.get(offset as usize).copied().unwrap_or(c),
        None => c,
    }
}

fn convert(chars: &[char], mode: TextMode, special: &mut Special, depth: usize, out: &mut Inlines) {
    let legacy = |c: char| {
        if mode == TextMode::Attribute {
            c
        } else {
            legacy_character(c)
        }
    };
    let mut index = 0;
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut Inlines| {
        if !word.is_empty() {
            out.push(Inline::Str(std::mem::take(word)));
        }
    };
    while let Some(&c) = chars.get(index) {
        if mode == TextMode::Pre && !special.starts(c) {
            index += 1;
            match c {
                '\n' => {
                    flush(&mut word, out);
                    out.push(Inline::LineBreak);
                }
                ' ' => word.push('\u{a0}'),
                _ => word.push(legacy(c)),
            }
            continue;
        }
        if is_space(c) {
            flush(&mut word, out);
            let mut newline = false;
            while let Some(&next) = chars.get(index).filter(|next| is_space(**next)) {
                newline |= next == '\n' || next == '\r';
                index += 1;
            }
            out.push(if newline {
                Inline::SoftBreak
            } else {
                Inline::Space
            });
            continue;
        }
        if special.starts(c)
            && let Some((inline, next)) = special.parse(chars, index, depth)
        {
            flush(&mut word, out);
            out.push(inline);
            index = next;
            continue;
        }
        word.push(legacy(c));
        index += 1;
    }
    flush(&mut word, out);
}

/// The extensions that give a character of a text token a special meaning,
/// and the searches that failed in this text. A search for a closing
/// delimiter that reaches the end of the text fails for each later start
/// too, so it is not made a second time.
struct Special {
    dollars: bool,
    single_backslash: bool,
    double_backslash: bool,
    raw_tex: bool,
    smart: bool,
    /// For each math delimiter kind: a content start before this index
    /// fails, because an earlier search failed at this index.
    math_failed_until: [usize; MATH_KINDS],
    /// The `\text` groups that failed in this text.
    group_failures: usize,
    /// For each `{` of the text, the index of its `}`. Made on first use.
    brace_ends: Option<Vec<Option<usize>>>,
    /// For each index of the text, the number of characters before it that
    /// end a `\text` group (see `group_breaks`). Made on first use.
    group_breaks: Option<Vec<usize>>,
    /// The end of each math search, by delimiter kind, index and whether an
    /// escape led to the index. A later search that reaches the same state
    /// has the same end, so each state is scanned once.
    math_ends: HashMap<(usize, usize, bool), MathEnd>,
    /// The `[` groups without an end that a `\ref` search found.
    bracket_failures: usize,
    no_environment_end: Vec<&'static str>,
    /// The close of each quotation of the whole text token. Made on first use.
    quote_closes: Option<Rc<QuoteCloses>>,
    /// The innermost quotation around this text.
    context: Option<QuoteType>,
    /// The character after the end of this text, in the text around it.
    next: Option<char>,
    /// The mode of the text, also for the text inside a quotation.
    mode: TextMode,
    /// The whole text token, and the index of this text in it. A quotation
    /// search reads the whole token, because a nested quotation can end
    /// after the end of the quotation around it.
    full: Rc<[char]>,
    offset: usize,
}

/// The math delimiter kinds: `$`, `$$`, then `(` and `[` with one and with two
/// backslashes.
const MATH_KINDS: usize = 6;

/// The `\text` groups that can fail in one text before the math search of a
/// kind stops: a failed group cannot be skipped by a later search.
const MAX_GROUP_FAILURES: usize = 16;

/// The result of a search for the end of math content.
#[derive(Clone, Copy)]
enum MathEnd {
    /// The content ends at the first index; the closer ends at the second.
    Found(usize, usize),
    /// No content here, and no content that starts before the index.
    FailedUntil(usize),
    /// No content here.
    Rejected,
}

impl Special {
    fn new(extensions: Extensions, full: Rc<[char]>, offset: usize) -> Self {
        Special {
            dollars: extensions.contains(Extension::TexMathDollars),
            single_backslash: extensions.contains(Extension::TexMathSingleBackslash),
            double_backslash: extensions.contains(Extension::TexMathDoubleBackslash),
            raw_tex: extensions.contains(Extension::RawTex),
            smart: extensions.contains(Extension::Smart),
            math_failed_until: [0; MATH_KINDS],
            group_failures: 0,
            brace_ends: None,
            group_breaks: None,
            math_ends: HashMap::new(),
            bracket_failures: 0,
            no_environment_end: Vec::new(),
            quote_closes: None,
            context: None,
            next: None,
            mode: TextMode::Normal,
            full,
            offset,
        }
    }

    fn starts(&self, c: char) -> bool {
        match c {
            '$' => self.dollars,
            '\\' => self.single_backslash || self.double_backslash || self.raw_tex,
            '-' | '.' | '\'' | '"' => self.smart,
            _ => false,
        }
    }

    /// Parses a special construct at `index`. Returns the inline and the index after it.
    fn parse(&mut self, chars: &[char], index: usize, depth: usize) -> Option<(Inline, usize)> {
        match chars.get(index)? {
            '$' => self.dollar_math(chars, index),
            '\\' => {
                let mut found = None;
                if self.double_backslash {
                    found = self.backslash_math(chars, index, 2);
                }
                if found.is_none() && self.single_backslash {
                    found = self.backslash_math(chars, index, 1);
                }
                if found.is_none() && self.raw_tex {
                    found = self.raw_tex(chars, index);
                }
                found
            }
            _ => self.smart_punctuation(chars, index, depth),
        }
    }

    /// `$x$` and `$$x$$`.
    fn dollar_math(&mut self, chars: &[char], index: usize) -> Option<(Inline, usize)> {
        if chars.get(index + 1) == Some(&'$') {
            let (body, next) = self.math_content(chars, index + 2, &['$', '$'], 1)?;
            return Some((Inline::Math(MathType::DisplayMath, trim_math(&body)), next));
        }
        let first = *chars.get(index + 1)?;
        if is_haskell_space(first) || first == '$' {
            return None;
        }
        let (body, next) = self.math_content(chars, index + 1, &['$'], 0)?;
        Some((Inline::Math(MathType::InlineMath, body), next))
    }

    /// `\(x\)` and `\[x\]`, with `slashes` backslashes before each bracket.
    fn backslash_math(
        &mut self,
        chars: &[char],
        index: usize,
        slashes: usize,
    ) -> Option<(Inline, usize)> {
        if !(0..slashes).all(|offset| chars.get(index + offset) == Some(&'\\')) {
            return None;
        }
        let (close, kind, slot) = match chars.get(index + slashes)? {
            '(' => (')', MathType::InlineMath, 2),
            '[' => (']', MathType::DisplayMath, 3),
            _ => return None,
        };
        let mut closer = vec!['\\'; slashes];
        closer.push(close);
        let slot = slot + 2 * (slashes - 1);
        let (body, next) = self.math_content(chars, index + slashes + 1, &closer, slot)?;
        Some((Inline::Math(kind, trim_math(&body)), next))
    }

    /// Returns the math content from `start` to `closer`, and the index after
    /// the closer. `kind` is the delimiter kind. The content has at least one
    /// character, no blank line and no carriage return. A backslash escapes
    /// the next character. `\text` with letters takes a balanced brace group,
    /// in which a closer is text.
    fn math_content(
        &mut self,
        chars: &[char],
        start: usize,
        closer: &[char],
        kind: usize,
    ) -> Option<(String, usize)> {
        if self
            .math_failed_until
            .get(kind)
            .is_none_or(|until| start < *until)
        {
            return None;
        }
        match self.find_math_end(chars, start, closer, kind) {
            MathEnd::Found(end, next) => Some((collect(chars, start, end), next)),
            MathEnd::Rejected => None,
            MathEnd::FailedUntil(until) => {
                let in_group = until < chars.len() && chars.get(until) == Some(&'\\');
                if in_group {
                    self.group_failures += 1;
                }
                let until = if self.group_failures > MAX_GROUP_FAILURES {
                    usize::MAX
                } else {
                    until
                };
                if let Some(slot) = self.math_failed_until.get_mut(kind) {
                    *slot = until;
                }
                None
            }
        }
    }

    /// A command whose name starts with `ref` or `eqref`, with its arguments,
    /// and a math environment
    /// `\begin{name}...\end{name}`, as raw TeX.
    fn raw_tex(&mut self, chars: &[char], index: usize) -> Option<(Inline, usize)> {
        let mut at = index + 1;
        while chars.get(at).is_some_and(char::is_ascii_alphabetic) {
            at += 1;
        }
        let name = collect(chars, index + 1, at);
        let end = if name == "begin" {
            self.environment(chars, at)?
        } else if name.starts_with("ref") || name.starts_with("eqref") {
            self.reference_arguments(chars, at)?
        } else {
            return None;
        };
        let raw = Inline::RawInline(Format::new("tex"), collect(chars, index, end));
        Some((raw, end))
    }

    /// Returns the end of the arguments of `\ref` after the name at `at`: an
    /// optional `*`, spaces, groups in brackets with spaces between, then
    /// adjacent groups in braces. A brace group without an end fails. After a
    /// tab, no group follows.
    fn reference_arguments(&mut self, chars: &[char], mut at: usize) -> Option<usize> {
        if chars.get(at) == Some(&'*') {
            at += 1;
        }
        let skip_spaces = |mut at: usize| {
            while matches!(chars.get(at), Some(' ' | '\t')) {
                at += 1;
            }
            at
        };
        let name_end = at;
        at = skip_spaces(at);
        if chars
            .get(name_end..at)
            .is_some_and(|run| run.contains(&'\t'))
        {
            return Some(at);
        }
        while chars.get(at) == Some(&'[') && self.bracket_failures < MAX_GROUP_FAILURES {
            match self.bracket_end(chars, at) {
                Some(end) => {
                    let next = skip_spaces(end);
                    at = if matches!(chars.get(next), Some('[' | '{')) {
                        next
                    } else {
                        end
                    };
                }
                None => {
                    self.bracket_failures += 1;
                    break;
                }
            }
        }
        while chars.get(at) == Some(&'{') {
            at = self.brace_end(chars, at)?;
        }
        Some(at)
    }

    /// Returns the index after the `}` that closes the `{` at `at`.
    fn brace_end(&mut self, chars: &[char], at: usize) -> Option<usize> {
        let ends = self.brace_ends.get_or_insert_with(|| brace_ends(chars));
        ends.get(at).copied().flatten().map(|end| end + 1)
    }

    /// Searches the end of math content of the delimiter kind `kind`. The
    /// kind `$` adds its rules: the closer has no unescaped space before it
    /// and no ASCII digit after it.
    fn find_math_end(
        &mut self,
        chars: &[char],
        start: usize,
        closer: &[char],
        kind: usize,
    ) -> MathEnd {
        let inline_dollar = kind == 0;
        let mut at = start;
        let mut escaped = false;
        let mut visited = Vec::new();
        let end = loop {
            // The start has its own rule (a closer there is no content), so
            // only a later state has an end that other searches share.
            if at != start {
                if let Some(end) = self.math_ends.get(&(kind, at, escaped)) {
                    break *end;
                }
                visited.push((at, escaped));
            }
            #[cfg(test)]
            MATH_STEPS.with(|steps| steps.set(steps.get() + 1));
            let Some(&c) = chars.get(at) else {
                break MathEnd::FailedUntil(at);
            };
            if chars.get(at..at + closer.len()) == Some(closer) {
                if at == start {
                    break MathEnd::Rejected;
                }
                if inline_dollar {
                    let space_before = chars.get(at - 1).is_some_and(|c| is_space(*c));
                    let digit_after = chars
                        .get(at + closer.len())
                        .is_some_and(char::is_ascii_digit);
                    if (space_before && !escaped) || digit_after {
                        break MathEnd::Rejected;
                    }
                }
                break MathEnd::Found(at, at + closer.len());
            }
            escaped = false;
            match c {
                '\r' => break MathEnd::FailedUntil(at),
                '\n' => {
                    if starts_blank_line(chars, at) {
                        break MathEnd::FailedUntil(at);
                    }
                    at += 1;
                }
                '\\' if chars.get(at + 1..at + 5) == Some(&['t', 'e', 'x', 't']) => {
                    match self.text_group_end(chars, at + 5) {
                        Some(end) => at = end,
                        None => break MathEnd::FailedUntil(at),
                    }
                }
                '\\' => {
                    if chars.get(at + 1).is_none_or(|c| *c == '\r') {
                        break MathEnd::FailedUntil(at);
                    }
                    at += 2;
                    escaped = true;
                }
                _ => at += 1,
            }
        };
        for (at, escaped) in visited {
            self.math_ends.insert((kind, at, escaped), end);
        }
        end
    }

    /// Returns the index after the letters, the white space and the adjacent
    /// balanced brace groups that follow `\text`. `None` when a group has no
    /// end, a blank line or a carriage return.
    fn text_group_end(&mut self, chars: &[char], from: usize) -> Option<usize> {
        let mut at = from;
        while chars.get(at).is_some_and(char::is_ascii_alphabetic) {
            at += 1;
        }
        let letters_end = at;
        while chars.get(at).is_some_and(|c| is_space(*c)) {
            at += 1;
        }
        if chars.get(at) != Some(&'{') {
            return Some(letters_end);
        }
        let breaks = self.group_breaks.get_or_insert_with(|| group_breaks(chars));
        let ends = self.brace_ends.get_or_insert_with(|| brace_ends(chars));
        loop {
            #[cfg(test)]
            MATH_STEPS.with(|steps| steps.set(steps.get() + 1));
            let end = ends.get(at).copied().flatten()?;
            if breaks.get(end) != breaks.get(at) {
                return None;
            }
            if chars.get(end + 1) != Some(&'{') {
                return Some(end + 1);
            }
            at = end + 1;
        }
    }

    /// Returns the index after the `]` that closes the `[` at `at`. A brace
    /// group and an escaped character inside are skipped.
    fn bracket_end(&mut self, chars: &[char], at: usize) -> Option<usize> {
        let mut at = at + 1;
        loop {
            match chars.get(at)? {
                ']' => return Some(at + 1),
                '{' => at = self.brace_end(chars, at)?,
                '\\' => at += 2,
                _ => at += 1,
            }
        }
    }

    /// Returns the end of a math environment after `\begin` at `at`: spaces,
    /// `{name}` with a name of `MATH_ENVIRONMENTS`, and the content up to the
    /// first `\end{name}`, in which a backslash escapes the next character.
    fn environment(&mut self, chars: &[char], mut at: usize) -> Option<usize> {
        while matches!(chars.get(at), Some(' ' | '\t')) {
            at += 1;
        }
        if chars.get(at) != Some(&'{') {
            return None;
        }
        let name_end = at + chars.get(at..)?.iter().position(|c| *c == '}')?;
        let name = collect(chars, at + 1, name_end);
        let name = *MATH_ENVIRONMENTS.iter().find(|known| **known == name)?;
        if self.no_environment_end.contains(&name) {
            return None;
        }
        let closer: Vec<char> = name.chars().collect();
        let mut at = name_end + 1;
        while let Some(&c) = chars.get(at) {
            if c != '\\' {
                at += 1;
                continue;
            }
            if chars.get(at + 1..at + 4) == Some(&['e', 'n', 'd']) {
                let mut brace = at + 4;
                while chars.get(brace).is_some_and(|c| is_space(*c)) {
                    brace += 1;
                }
                let name_start = brace + 1;
                let name_end = name_start + closer.len();
                if chars.get(brace) == Some(&'{')
                    && chars.get(name_start..name_end) == Some(closer.as_slice())
                    && chars.get(name_end) == Some(&'}')
                {
                    return Some(name_end + 1);
                }
            }
            at += 2;
        }
        self.no_environment_end.push(name);
        None
    }

    /// Dashes, ellipses and quotation marks of the `smart` extension.
    fn smart_punctuation(
        &mut self,
        chars: &[char],
        index: usize,
        depth: usize,
    ) -> Option<(Inline, usize)> {
        let at = |offset: usize| chars.get(index + offset).copied();
        let text = |text: &str, length: usize| Some((Inline::Str(text.to_owned()), index + length));
        match at(0)? {
            '-' if at(1) == Some('-') && at(2) == Some('-') => text("\u{2014}", 3),
            '-' if at(1) == Some('-') => text("\u{2013}", 2),
            '.' if at(1) == Some('.') && at(2) == Some('.') => text("\u{2026}", 3),
            quote @ ('\'' | '"') => self.smart_quote(chars, index, quote, depth),
            _ => None,
        }
    }

    /// A quotation inside one text token, or a single typographic quote
    /// mark. A mark opens a quotation after the start, white space or other
    /// punctuation that `smart` reads, before a character that is not white
    /// space, and outside a quotation of its kind. The first double mark
    /// after it closes; the first single mark that no letter or digit follows
    /// closes, but not directly after the open mark.
    /// An open mark without a close is a left double mark or an apostrophe.
    fn smart_quote(
        &mut self,
        chars: &[char],
        index: usize,
        quote: char,
        depth: usize,
    ) -> Option<(Inline, usize)> {
        let before = index.checked_sub(1).and_then(|at| chars.get(at)).copied();
        let after = chars.get(index + 1).copied().or(self.next);
        let kind = if quote == '"' {
            QuoteType::DoubleQuote
        } else {
            QuoteType::SingleQuote
        };
        let after_punctuation = before.is_none_or(|c| {
            is_space(c)
                || matches!(
                    c,
                    '-' | '.' | '\'' | '"' | '\u{201c}' | '\u{201d}' | '\u{2018}' | '\u{2019}'
                )
        });
        let opens =
            after_punctuation && after.is_some_and(|c| !is_space(c)) && self.context != Some(kind);
        if opens && depth < MAX_QUOTE_DEPTH {
            let table = Rc::clone(
                self.quote_closes
                    .get_or_insert_with(|| Rc::new(QuoteCloses::new(&self.full))),
            );
            if let Some(end) = table
                .close(self.offset + index, quote)
                .map(|close| close - self.offset)
                .filter(|end| *end < chars.len())
            {
                let mut inner = Inlines::new();
                let body = chars.get(index + 1..end).unwrap_or_default();
                let mut special = Special::new(
                    Extensions::from_list(&[Extension::Smart]),
                    Rc::clone(&self.full),
                    self.offset + index + 1,
                );
                special.quote_closes = Some(table);
                special.context = Some(kind);
                special.next = Some(quote);
                special.mode = self.mode;
                convert(body, self.mode, &mut special, depth + 1, &mut inner);
                return Some((Inline::Quoted(kind, inner.0), end + 1));
            }
        }
        let mark = match (quote, opens) {
            ('"', true) => "\u{201c}",
            ('"', false) => "\u{201d}",
            _ => "\u{2019}",
        };
        Some((Inline::Str(mark.to_owned()), index + 1))
    }
}

/// Whether the mark at `at` can open a quotation of its kind inside a
/// quotation of the other kind: after the start, white space or punctuation
/// that `smart` reads, and before a character that is not white space.
fn opens_inside(chars: &[char], at: usize) -> bool {
    let before = at
        .checked_sub(1)
        .and_then(|before| chars.get(before))
        .copied();
    let after_punctuation = before.is_none_or(|c| {
        is_space(c)
            || matches!(
                c,
                '-' | '.' | '\'' | '"' | '\u{201c}' | '\u{201d}' | '\u{2018}' | '\u{2019}'
            )
    });
    after_punctuation && chars.get(at + 1).is_some_and(|c| !is_space(*c))
}

/// For each mark of a text that opens a quotation, the index of its close
/// mark. A quotation of the other kind inside it is skipped, so that its
/// marks do not close the outer quotation; such a nested quotation can end
/// after the end of the quotation around it, and then that one has no close.
pub(crate) struct QuoteCloses {
    /// `walk[kind][at]`: the close mark of kind `kind` (0 double, 1 single)
    /// that a search from `at` finds.
    walk: [Vec<Option<usize>>; 2],
    chars_len: usize,
    single_closes: Vec<bool>,
}

impl QuoteCloses {
    fn new(chars: &[char]) -> Self {
        let length = chars.len();
        let closes = |at: usize, quote: char| {
            chars.get(at) == Some(&quote)
                && (quote == '"' || !chars.get(at + 1).is_some_and(|c| c.is_alphanumeric()))
        };
        let single_closes: Vec<bool> = (0..=length).map(|at| closes(at, '\'')).collect();
        let mut walk = [vec![None; length + 1], vec![None; length + 1]];
        for at in (0..length).rev() {
            for (kind, quote, other) in [(0, '"', 1), (1, '\'', 0)] {
                let found = if closes(at, quote) {
                    Some(at)
                } else {
                    let other_mark = if other == 0 { '"' } else { '\'' };
                    let nested = (chars.get(at) == Some(&other_mark) && opens_inside(chars, at))
                        .then(|| Self::close_in(&walk, &single_closes, at, other))
                        .flatten();
                    let next = nested.map_or(at + 1, |close| close + 1);
                    walk[kind].get(next).copied().flatten()
                };
                walk[kind][at] = found;
            }
        }
        QuoteCloses {
            walk,
            chars_len: length,
            single_closes,
        }
    }

    fn close_in(
        walk: &[Vec<Option<usize>>; 2],
        single_closes: &[bool],
        at: usize,
        kind: usize,
    ) -> Option<usize> {
        // A single quotation is not empty.
        if kind == 1 && single_closes.get(at + 1).copied().unwrap_or(false) {
            return None;
        }
        walk[kind].get(at + 1).copied().flatten()
    }

    /// The index of the close mark of the quotation that the mark `quote` at
    /// `at` opens.
    fn close(&self, at: usize, quote: char) -> Option<usize> {
        let kind = usize::from(quote == '\'');
        (at < self.chars_len)
            .then(|| Self::close_in(&self.walk, &self.single_closes, at, kind))
            .flatten()
    }
}

/// The environments that `raw_tex` reads. Found with the oracle.
const MATH_ENVIRONMENTS: &[&str] = &[
    "align",
    "align*",
    "alignat",
    "alignat*",
    "darray",
    "darray*",
    "dgroup",
    "dgroup*",
    "displaymath",
    "dmath",
    "dmath*",
    "eqnarray",
    "eqnarray*",
    "equation",
    "equation*",
    "flalign",
    "flalign*",
    "gather",
    "gather*",
    "math",
    "multline",
    "multline*",
    "subequations",
];

/// Returns, for each `{` of the text, the index of the `}` that closes it.
/// A backslash escapes the next character.
fn brace_ends(chars: &[char]) -> Vec<Option<usize>> {
    let mut ends = vec![None; chars.len()];
    let mut open: Vec<usize> = Vec::new();
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        match c {
            '\\' => at += 1,
            '{' => open.push(at),
            '}' => {
                if let Some(start) = open.pop()
                    && let Some(slot) = ends.get_mut(start)
                {
                    *slot = Some(at);
                }
            }
            _ => {}
        }
        at += 1;
    }
    ends
}

/// Whether the newline at `at` starts a blank line: only spaces and tabs
/// follow it before the next line break.
fn starts_blank_line(chars: &[char], at: usize) -> bool {
    let mut next = at + 1;
    while chars.get(next).is_some_and(|c| matches!(c, ' ' | '\t')) {
        next += 1;
    }
    matches!(chars.get(next), Some('\n' | '\r'))
}

/// For each index of the text, the number of characters before it that end
/// a `\text` group: a carriage return, a backslash before a carriage return
/// or at the end, and a newline that starts a blank line. A backslash escapes
/// the next character, as in `brace_ends`.
fn group_breaks(chars: &[char]) -> Vec<usize> {
    let mut breaks = Vec::with_capacity(chars.len() + 1);
    let mut count = 0;
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        breaks.push(count);
        let breaks_group = match c {
            '\r' => true,
            '\n' => starts_blank_line(chars, at),
            '\\' => chars.get(at + 1).is_none_or(|c| *c == '\r'),
            _ => false,
        };
        count += usize::from(breaks_group);
        if c == '\\' && at + 1 < chars.len() {
            at += 1;
            breaks.push(count);
        }
        at += 1;
    }
    breaks.push(count);
    breaks
}

#[cfg(test)]
thread_local! {
    /// The steps of the math searches on this thread.
    pub(crate) static MATH_STEPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Removes the white space at both ends of math content. A space that a
/// backslash escapes stays.
fn trim_math(body: &str) -> String {
    let body = body.trim_start();
    let mut end = body.len();
    while let Some(c) = body[..end].chars().next_back() {
        let before = body[..end - c.len_utf8()].chars().next_back();
        if !c.is_whitespace() || before == Some('\\') {
            break;
        }
        end -= c.len_utf8();
    }
    body[..end].to_owned()
}

fn collect(chars: &[char], from: usize, to: usize) -> String {
    chars.get(from..to).unwrap_or_default().iter().collect()
}

/// The attribute names of HTML that keep a `data-` prefix: Pandoc removes the
/// prefix from each other name. The list is sorted. Found with the oracle.
#[rustfmt::skip]
const HTML_ATTRIBUTES: &[&str] = &[
    "abbr", "accept", "accept-charset", "accesskey", "action", "align", "alink", "allow",
    "allowfullscreen", "alt", "archive", "as", "async", "autocapitalize", "autocomplete",
    "autofocus", "autoplay", "axis", "background", "bgcolor", "border", "cellpadding",
    "cellspacing", "char", "charoff", "charset", "checked", "cite", "class", "classid", "clear",
    "code", "codebase", "codetype", "color", "cols", "colspan", "compact", "content",
    "contenteditable", "controls", "coords", "crossorigin", "data", "datetime", "declare",
    "decoding", "default", "defer", "dir", "dirname", "disabled", "download", "draggable",
    "enctype", "enterkeyhint", "face", "for", "form", "formaction", "formenctype", "formmethod",
    "formnovalidate", "formtarget", "frame", "frameborder", "headers", "height", "hidden", "high",
    "href", "hreflang", "hspace", "http-equiv", "id", "inputmode", "integrity", "is", "ismap",
    "itemid", "itemprop", "itemref", "itemscope", "itemtype", "kind", "label", "lang", "language",
    "link", "list", "loading", "longdesc", "loop", "low", "manifest", "marginheight",
    "marginwidth", "max", "maxlength", "media", "method", "min", "minlength", "multiple", "muted",
    "name", "nohref", "nomodule", "nonce", "noresize", "noshade", "novalidate", "nowrap", "object",
    "onabort", "onblur", "onchange", "onclick", "ondblclick", "onerror", "onfocus", "onkeydown",
    "onkeypress", "onkeyup", "onload", "onmousedown", "onmousemove", "onmouseout", "onmouseover",
    "onmouseup", "onreset", "onselect", "onsubmit", "onunload", "open", "optimum", "pattern",
    "ping", "placeholder", "playsinline", "poster", "preload", "profile", "prompt", "readonly",
    "referrerpolicy", "rel", "required", "rev", "reversed", "role", "rows", "rowspan", "rules",
    "sandbox", "scheme", "scope", "scrolling", "selected", "shape", "size", "sizes", "slot",
    "span", "spellcheck", "src", "srcdoc", "srclang", "srcset", "standby", "start", "step",
    "style", "summary", "tabindex", "target", "text", "title", "translate", "type",
    "typemustmatch", "usemap", "valign", "value", "valuetype", "version", "vlink", "vspace",
    "width", "wrap",
];

/// Maps the attribute names as Pandoc does and keeps the first of each name.
pub(crate) fn clean_attributes(raw: &[(Text, Text)]) -> Vec<(Text, Text)> {
    let mut out: Vec<(Text, Text)> = Vec::with_capacity(raw.len());
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (key, value) in raw {
        let name: &str = match key.strip_prefix("data-") {
            Some(rest) if HTML_ATTRIBUTES.binary_search(&rest).is_err() => rest,
            _ if key == "xml:lang" => "lang",
            _ => key,
        };
        if seen.insert(name) {
            out.push((name.to_owned(), value.clone()));
        }
    }
    out
}

/// Returns the value of an attribute.
pub(crate) fn attribute<'a>(attributes: &'a [(Text, Text)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// Makes the attributes of a tree element from cleaned attributes.
pub(crate) fn to_attr(attributes: &[(Text, Text)]) -> Attr {
    let id = attribute(attributes, "id").unwrap_or_default().to_owned();
    let mut classes: Vec<Text> = attribute(attributes, "class")
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if let Some(types) = attribute(attributes, "epub:type") {
        classes.extend(types.split_whitespace().map(str::to_owned));
    }
    let rest = attributes
        .iter()
        .filter(|(key, _)| key != "id" && key != "class")
        .cloned()
        .collect();
    Attr::new(id, classes, rest)
}

/// Escapes the characters that Pandoc escapes in a URL.
pub(crate) fn escape_uri(url: &str) -> Text {
    let mut out = Text::with_capacity(url.len());
    for c in url.chars() {
        if is_haskell_space(c)
            || matches!(c, '<' | '>' | '|' | '"' | '{' | '}' | '[' | ']' | '^' | '`')
        {
            let mut buffer = [0u8; 4];
            for byte in c.encode_utf8(&mut buffer).bytes() {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// White space as `isSpace` of Haskell: the Latin-1 spaces, and the space
/// separators above U+0377.
fn is_haskell_space(c: char) -> bool {
    matches!(c, ' ' | '\t'..='\r' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}')
        || matches!(c, '\u{202f}' | '\u{205f}' | '\u{3000}')
}

/// Reads a whole attribute value as an integer, as Pandoc does for `start`,
/// `rowspan` and `colspan`. The oracle accepts white space around the value,
/// parentheses, a minus sign before the digits (also after white space),
/// decimal digits and the prefixes `0x` and `0o`. A value outside 64 bits
/// wraps. Each other value, also one with a plus sign, gives `None`.
pub(crate) fn read_integer(text: &str) -> Option<i64> {
    let mut chars = text.chars().peekable();
    let skip_space = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>| {
        while chars.next_if(|c| is_haskell_space(*c)).is_some() {}
    };
    let mut open = 0usize;
    skip_space(&mut chars);
    while chars.next_if_eq(&'(').is_some() {
        open += 1;
        skip_space(&mut chars);
    }
    let negative = chars.next_if_eq(&'-').is_some();
    if negative {
        skip_space(&mut chars);
    }
    let mut radix = 10;
    let mut value: i64 = 0;
    let mut digits = 0usize;
    if chars.next_if_eq(&'0').is_some() {
        digits = 1;
        if let Some(prefix) = chars.next_if(|c| matches!(c, 'x' | 'X' | 'o' | 'O')) {
            radix = if matches!(prefix, 'x' | 'X') { 16 } else { 8 };
            digits = 0;
        }
    }
    while let Some(digit) = chars.peek().and_then(|c| c.to_digit(radix)) {
        chars.next();
        digits += 1;
        value = value
            .wrapping_mul(i64::from(radix))
            .wrapping_add(i64::from(digit));
    }
    if digits == 0 {
        return None;
    }
    skip_space(&mut chars);
    while open > 0 && chars.next_if_eq(&')').is_some() {
        open -= 1;
        skip_space(&mut chars);
    }
    (open == 0 && chars.next().is_none()).then(|| {
        if negative {
            value.wrapping_neg()
        } else {
            value
        }
    })
}

/// Reads a whole value as a decimal number, as the oracle reads a width in
/// percent: white space around it, an optional minus sign, digits, an
/// optional fraction with digits on both sides of the point, and an optional
/// exponent.
pub(crate) fn read_decimal(text: &str) -> Option<f64> {
    let text = text.trim_matches(is_haskell_space);
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let mantissa_end = unsigned.find(['e', 'E']).unwrap_or(unsigned.len());
    let (mantissa, exponent) = unsigned.split_at(mantissa_end);
    let digits = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
    let mantissa_ok = match mantissa.split_once('.') {
        Some((whole, fraction)) => digits(whole) && digits(fraction),
        None => digits(mantissa),
    };
    let exponent_ok = exponent.is_empty() || {
        let rest = &exponent[1..];
        digits(rest.strip_prefix(['+', '-']).unwrap_or(rest))
    };
    if !(mantissa_ok && exponent_ok) {
        return None;
    }
    text.parse::<f64>().ok()
}

/// Returns the value of one property of a `style` attribute.
pub(crate) fn style_property(style: &str, property: &str) -> Option<Text> {
    style.split(';').find_map(|declaration| {
        let (name, value) = declaration.split_once(':')?;
        (name.trim().eq_ignore_ascii_case(property)).then(|| value.trim().to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inlines(text: &str, extensions: &[Extension]) -> Vec<Inline> {
        let mut out = Inlines::new();
        text_inlines(
            text,
            TextMode::Normal,
            Extensions::from_list(extensions),
            &mut out,
        );
        out.0
    }

    fn word(text: &str) -> Inline {
        Inline::Str(text.to_owned())
    }

    #[test]
    fn splits_text_at_spaces_and_newlines() {
        assert_eq!(
            inlines("tab\there  nl\n\n  two \u{c} \u{a0}", &[]),
            vec![
                word("tab"),
                Inline::Space,
                word("here"),
                Inline::Space,
                word("nl"),
                Inline::SoftBreak,
                word("two"),
                Inline::Space,
                word("\u{c}"),
                Inline::Space,
                word("\u{a0}"),
            ]
        );
    }

    #[test]
    fn joins_neighbours_as_the_builder_does() {
        let mut list = Inlines::new();
        for inline in [
            word("a"),
            word("b"),
            Inline::Space,
            Inline::LineBreak,
            Inline::Space,
            Inline::Strikeout(vec![word("s")]),
            Inline::Strikeout(vec![word("k")]),
            Inline::SoftBreak,
            Inline::Space,
        ] {
            list.push(inline);
        }
        assert_eq!(
            list.0,
            vec![
                word("ab"),
                Inline::LineBreak,
                Inline::Strikeout(vec![word("s"), word("k")]),
                Inline::SoftBreak,
            ]
        );
    }

    #[test]
    fn extracts_spaces_from_an_element() {
        let content = Inlines(vec![Inline::Space, word("x"), Inline::SoftBreak]);
        assert_eq!(
            content.wrap_extracting_spaces(Inline::Strong).0,
            vec![
                Inline::Space,
                Inline::Strong(vec![word("x")]),
                Inline::SoftBreak
            ]
        );
        let only_space = Inlines(vec![Inline::Space]);
        assert_eq!(
            only_space.wrap_extracting_spaces(Inline::Emph).0,
            vec![Inline::Space, Inline::Emph(vec![]), Inline::Space]
        );
    }

    // Pandoc 3.13: an apostrophe inside a nested quotation does not close the
    // quotation around it. In the second input the nested single quotation
    // ends at the last mark, after the double quotation, so the double marks
    // are plain marks.
    #[test]
    fn a_nested_quotation_is_read_before_the_outer_close() {
        let smart = |text: &str| inlines(text, &[Extension::Smart]);
        let single = |inner| Inline::Quoted(QuoteType::SingleQuote, inner);
        assert_eq!(
            smart(r#"'He said "dogs' names".'"#),
            vec![single(vec![
                word("He"),
                Inline::Space,
                word("said"),
                Inline::Space,
                Inline::Quoted(
                    QuoteType::DoubleQuote,
                    vec![word("dogs\u{2019}"), Inline::Space, word("names")]
                ),
                word("."),
            ])]
        );
        assert_eq!(
            smart(r#"'x "'a" y'"#),
            vec![single(vec![
                word("x"),
                Inline::Space,
                word("\u{201c}\u{2019}a\u{201d}"),
                Inline::Space,
                word("y"),
            ])]
        );
    }

    fn rejected_groups(count: usize) -> String {
        format!("{}{}$2", r"$\text{".repeat(count), "}".repeat(count))
    }

    fn math_steps(text: &str) -> u64 {
        MATH_STEPS.with(|steps| steps.set(0));
        inlines(text, &[Extension::TexMathDollars]);
        MATH_STEPS.with(std::cell::Cell::get)
    }

    // Each dollar starts a search that the digit after the last dollar
    // rejects. Pandoc 3.13 gives one `Str` for each count.
    #[test]
    fn a_rejected_math_search_is_not_scanned_again() {
        let text = rejected_groups(3);
        assert_eq!(
            inlines(&text, &[Extension::TexMathDollars]),
            vec![word(&text)]
        );
        let (small, large) = (
            math_steps(&rejected_groups(500)),
            math_steps(&rejected_groups(2000)),
        );
        assert!(
            large <= small * 6,
            "{small} steps for 500 groups, {large} for 2000"
        );
        let escapes = |count: usize| format!("${}$2", r"\$".repeat(count));
        let (small, large) = (math_steps(&escapes(500)), math_steps(&escapes(2000)));
        assert!(
            large <= small * 6,
            "{small} steps for 500 escapes, {large} for 2000"
        );
    }

    #[test]
    fn reads_math_with_the_extensions() {
        let math = |kind, text: &str| Inline::Math(kind, text.to_owned());
        assert_eq!(
            inlines("$x^2$ $$y$$ $ no $ $5", &[Extension::TexMathDollars]),
            vec![
                math(MathType::InlineMath, "x^2"),
                Inline::Space,
                math(MathType::DisplayMath, "y"),
                Inline::Space,
                word("$"),
                Inline::Space,
                word("no"),
                Inline::Space,
                word("$"),
                Inline::Space,
                word("$5"),
            ]
        );
        assert_eq!(
            inlines("\\( z \\) \\[y\\]", &[Extension::TexMathSingleBackslash]),
            vec![
                math(MathType::InlineMath, "z"),
                Inline::Space,
                math(MathType::DisplayMath, "y")
            ]
        );
        assert_eq!(
            inlines("\\\\(x\\\\)", &[Extension::TexMathDoubleBackslash]),
            vec![math(MathType::InlineMath, "x")]
        );
    }

    #[test]
    fn cleans_attributes() {
        let raw: Vec<(Text, Text)> = [
            ("data-foo", "1"),
            ("data-id", "2"),
            ("xml:lang", "en"),
            ("lang", "de"),
            ("id", "a"),
            ("id", "b"),
            ("class", " x  y "),
        ]
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
        let clean = clean_attributes(&raw);
        let attr = to_attr(&clean);
        assert_eq!(attr.id, "a");
        assert_eq!(attr.classes, vec!["x", "y"]);
        assert_eq!(
            attr.attributes,
            vec![
                ("foo".to_owned(), "1".to_owned()),
                ("data-id".to_owned(), "2".to_owned()),
                ("lang".to_owned(), "en".to_owned()),
            ]
        );
    }

    // Pandoc 3.13, `<ol start="VALUE">`: the start number, or 1 for `None`.
    #[test]
    fn reads_integers_as_the_oracle() {
        let cases: &[(&str, Option<i64>)] = &[
            ("0x2", Some(2)),
            ("0X3", Some(3)),
            ("0o7", Some(7)),
            ("0xA", Some(10)),
            ("00012", Some(12)),
            ("- 2", Some(-2)),
            ("((2))", Some(2)),
            (" ( 2 ) ", Some(2)),
            ("(- 2)", Some(-2)),
            ("\u{a0}2", Some(2)),
            ("\u{2003}2", Some(2)),
            ("\n2", Some(2)),
            ("-0x2", Some(-2)),
            ("0xffffffffffffffff", Some(-1)),
            ("99999999999999999999", Some(7_766_279_631_452_241_919)),
            ("9223372036854775808", Some(i64::MIN)),
            ("-9223372036854775809", Some(i64::MAX)),
            ("+2", None),
            ("0b11", None),
            ("0x", None),
            ("1_0", None),
            ("2.0", None),
            ("2e0", None),
            ("\u{662}", None),
            ("--2", None),
            ("-(2)", None),
            ("(2", None),
            ("3 4", None),
            ("0x1G", None),
            ("-", None),
            ("", None),
        ];
        for (text, expected) in cases {
            assert_eq!(read_integer(text), *expected, "{text:?}");
        }
    }

    #[test]
    fn escapes_urls_and_reads_styles() {
        assert_eq!(escape_uri(" sp ace "), "%20sp%20ace%20");
        assert_eq!(
            style_property("color:red; Width: 25% ;", "width").as_deref(),
            Some("25%")
        );
        assert_eq!(style_property("color:red", "width"), None);
    }
}
