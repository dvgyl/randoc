use std::cell::Cell;
use std::collections::HashMap;

use randoc_ast::MathType;

const PATTERNS: [&str; 11] = [
    ">>", ">", "@@", "\\)", "\\]", "$$", "$", "]", "[", "\n", "}",
];

pub(super) struct Index {
    markup: HashMap<usize, usize>,
    quotes: HashMap<usize, usize>,
    citations: HashMap<usize, usize>,
    groups: HashMap<usize, usize>,
    macros: HashMap<usize, usize>,
    matches: [Vec<usize>; PATTERNS.len()],
    math_close: HashMap<usize, usize>,
    environment_ends: HashMap<String, Vec<usize>>,
    offsets: [Cell<usize>; PATTERNS.len()],
    word: Cell<(usize, usize)>,
}

impl Index {
    pub fn new(text: &str, following: Option<char>) -> Self {
        let mut result = Self {
            markup: HashMap::new(),
            quotes: HashMap::new(),
            citations: HashMap::new(),
            groups: HashMap::new(),
            macros: HashMap::new(),
            matches: std::array::from_fn(|_| Vec::new()),
            math_close: HashMap::new(),
            environment_ends: HashMap::new(),
            offsets: std::array::from_fn(|_| Cell::new(0)),
            word: Cell::new((0, 0)),
        };
        let mut closing = [(); 6].map(|()| Vec::new());
        let mut quote_end = [None; 2];
        let mut math_end = None;
        let mut next = following;
        let mut stacks = [Vec::new(), Vec::new(), Vec::new()];
        let mut macros = Vec::new();
        let mut line = 0usize;
        let mut citation_at = None;
        let mut invalid_at = None;
        let mut separator = text.len();
        let mut chars = text.char_indices().rev().peekable();
        while let Some((pos, c)) = chars.next() {
            let before = chars.peek().map(|(_, c)| *c);
            let tail = &text[pos..];
            if c == '\n' {
                line += 1;
            }
            if let Some(k) = "*/_+=~".find(c) {
                let code = matches!(c, '=' | '~');
                if next.is_some_and(|c: char| !c.is_whitespace() && (code || c != '\\'))
                    && let Some(&(end, close_line)) = closing[k].last()
                    && end > pos + 1
                    && line - close_line <= 1
                {
                    result.markup.insert(pos, end);
                    if !code && before.is_none_or(pre_char) {
                        closing[k].pop();
                    }
                }
                if next.is_none_or(post_char)
                    && before.is_some_and(|c: char| {
                        !c.is_whitespace() && (code || !matches!(c, '^' | '_'))
                    })
                {
                    closing[k].push((pos, line));
                }
            }
            if let Some(k) = "'\"".find(c) {
                if next.is_some_and(|c: char| !c.is_whitespace())
                    && let Some(end) = quote_end[k]
                    && end > pos + 1
                {
                    result.quotes.insert(pos, end);
                }
                if before.is_some_and(|c| !c.is_whitespace())
                    && (c == '"' || next.is_none_or(post_char))
                {
                    quote_end[k] = Some(pos);
                }
            }
            if c == '$' {
                if let Some(end) = math_end {
                    result.math_close.insert(pos, end);
                }
                if before.is_some_and(|c| !c.is_whitespace())
                    && next.is_none_or(|c| c.is_whitespace() || ",.;:?-\'\")".contains(c))
                {
                    math_end = Some(pos);
                }
            }
            // Each citation part uses only its first at sign.
            if c == '@' {
                citation_at = Some((pos, next.is_some_and(citation_key_char)));
            }
            if c == ';' {
                if let Some((at, false)) = citation_at.filter(|(at, _)| *at < separator) {
                    invalid_at = Some(at);
                }
                separator = pos;
            }
            if before != Some('\\') {
                if let Some(k) = "]})".find(c) {
                    stacks[k].push(pos);
                }
                if let Some(k) = "[{(".find(c)
                    && let Some(end) = stacks[k].pop()
                {
                    result.groups.insert(pos, end);
                    if (tail.starts_with("[cite:") || tail.starts_with("[cite/"))
                        && let Some((at, true)) = citation_at
                        && at < end
                        && invalid_at.is_none_or(|bad| bad >= end)
                    {
                        result.citations.insert(pos, end);
                    }
                }
            }
            if c == '}' && tail.starts_with("}}}") && before != Some('}') {
                let run = tail.bytes().take_while(|&b| b == b'}').count();
                for offset in (0..run / 3).rev() {
                    macros.push(pos + offset * 3);
                }
            }
            if c == '{'
                && tail.starts_with("{{{")
                && before != Some('{')
                && let Some(end) = macros.pop()
            {
                result.macros.insert(pos, end);
            }
            if let Some(name) = tail
                .strip_prefix("\\end{")
                .and_then(crate::reader::blocks::latex_name)
            {
                result
                    .environment_ends
                    .entry(name.to_owned())
                    .or_default()
                    .push(pos + 6 + name.len());
            }
            for (pattern, matches) in PATTERNS.iter().zip(&mut result.matches) {
                if tail.starts_with(pattern) {
                    matches.push(pos);
                }
            }
            next = Some(c);
        }
        for (positions, offset) in result.matches.iter().zip(&result.offsets) {
            offset.set(positions.len());
        }
        result
    }

    pub fn find(&self, start: usize, pattern: &str) -> Option<usize> {
        let id = PATTERNS.iter().position(|p| *p == pattern)?;
        let positions = &self.matches[id];
        let mut cursor = self.offsets[id].get();
        while cursor > 0 && positions[cursor - 1] < start {
            cursor -= 1;
        }
        self.offsets[id].set(cursor);
        positions.get(cursor.checked_sub(1)?).copied()
    }

    pub fn link<'a>(
        &self,
        text: &'a str,
        start: usize,
    ) -> Option<(usize, &'a str, Option<&'a str>)> {
        let destination_end = self.next_match(start, "]")?;
        if destination_end == start
            || self
                .next_match(start, "[")
                .is_some_and(|pos| pos < destination_end)
            || self
                .next_match(start, "\n")
                .is_some_and(|pos| pos < destination_end)
        {
            return None;
        }
        let destination = &text[start..destination_end];
        let rest = &text[destination_end + 1..];
        if rest.starts_with(']') {
            return Some((destination_end + 2, destination, None));
        }
        if !rest.starts_with('[') {
            return None;
        }
        let start = destination_end + 2;
        let end = self.next_match(start, "]")?;
        (end > start && text[end..].starts_with("]]"))
            .then(|| (end + 2, destination, Some(&text[start..end])))
    }

    pub fn next_match(&self, start: usize, pattern: &str) -> Option<usize> {
        let id = PATTERNS.iter().position(|p| *p == pattern)?;
        let positions = &self.matches[id];
        let offset = positions.partition_point(|&pos| pos >= start);
        positions.get(offset.checked_sub(1)?).copied()
    }

    /// Returns the end of the run of letters, digits, `-` and `_` at `start`.
    /// A start inside the last run gives the same end without a new scan.
    pub fn word_end(&self, text: &str, start: usize) -> usize {
        let (first, end) = self.word.get();
        if first <= start && start <= end && end > first {
            return end;
        }
        let end = start
            + text[start..]
                .find(|c: char| !(c.is_alphanumeric() || "-_".contains(c)))
                .unwrap_or(text.len() - start);
        self.word.set((start, end));
        end
    }

    /// Returns the end of the first `\\end{NAME}` after `pos`.
    pub fn environment_end(&self, name: &str, pos: usize) -> Option<usize> {
        let ends = self.environment_ends.get(name)?;
        // The positions are in falling order.
        let count = ends.partition_point(|&end| end > pos);
        ends.get(count.checked_sub(1)?).copied()
    }

    pub fn citation(&self, pos: usize) -> Option<usize> {
        self.citations.get(&pos).copied()
    }

    pub fn code_end(&self, char: char, pos: usize, can_open: bool) -> Option<usize> {
        (matches!(char, '=' | '~') && can_open)
            .then(|| self.markup(pos))
            .flatten()
    }

    pub fn markup(&self, pos: usize) -> Option<usize> {
        self.markup.get(&pos).copied()
    }

    pub fn group(&self, pos: usize) -> Option<usize> {
        self.groups.get(&pos).copied()
    }

    pub fn macro_end(&self, pos: usize) -> Option<usize> {
        self.macros.get(&pos).copied()
    }

    pub fn quote(&self, pos: usize) -> Option<usize> {
        self.quotes.get(&pos).copied()
    }

    pub fn script<'a>(&self, text: &'a str, pos: usize, setting: &str) -> Option<(usize, &'a str)> {
        if setting == "nil" {
            return None;
        }
        let start = pos + 1;
        let tail = text.get(start..)?;
        let first = tail.chars().next()?;
        if first == '{' || (first == '(' && setting == "t") {
            let end = self.group(start)?;
            let body = if first == '{' {
                &text[start + 1..end]
            } else {
                &text[start..=end]
            };
            return Some((end + 1, body));
        }
        if setting != "t" {
            return None;
        }
        if first == '*' {
            return Some((start + 1, "*"));
        }
        let offset = usize::from(matches!(first, '+' | '-'));
        let signed = &tail[offset..];
        let len = signed
            .find(|c: char| !c.is_alphanumeric())
            .unwrap_or(signed.len());
        (len > 0).then(|| (start + offset + len, &tail[..offset + len]))
    }

    pub fn math<'a>(&self, text: &'a str, pos: usize) -> Option<(usize, &'a str, MathType)> {
        let tail = text.get(pos..)?;
        if tail.starts_with("$$") {
            let end = self.find(pos + 2, "$$")?;
            return Some((end + 2, &text[pos + 2..end], MathType::DisplayMath));
        }
        if tail[1..].starts_with(|c: char| c.is_whitespace() || matches!(c, '.' | ',')) {
            return None;
        }
        let end = *self.math_close.get(&pos)?;
        (end > pos + 1).then(|| (end + 1, &text[pos + 1..end], MathType::InlineMath))
    }
}

pub(super) fn pre_char(c: char) -> bool {
    c.is_whitespace() || "(\"'-{\u{200b}".contains(c)
}

fn post_char(c: char) -> bool {
    c.is_whitespace() || "!\"'),-.:;?[}\u{200b}".contains(c)
}

pub(super) fn citation_key_char(c: char) -> bool {
    c.is_alphanumeric() || "_-.:/#$%&+?<>~".contains(c)
}
