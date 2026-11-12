//! The macro definitions at the start of a formula (`\newcommand` and its
//! relatives) and their expansion.
//!
//! The oracle reads the definitions, removes them, and replaces each use in
//! the rest of the text by the body in braces. It reports a parse error at
//! its place in the expanded text.

use std::collections::HashMap;

/// The largest number of passes over the text. One pass expands each use
/// that the text holds at its start; a body that uses a macro needs one more.
const MAX_PASSES: usize = 32;

/// The largest expanded text: this number of characters plus 4 times the
/// input.
const MAX_GROWTH: usize = 65_536;

struct Macro {
    params: usize,
    default: Option<Vec<char>>,
    body: Vec<char>,
}

struct Reader<'a> {
    chars: &'a [char],
    pos: usize,
    /// The largest place that a search for a closing delimiter reached.
    reach: usize,
    /// The place after the last line end: a comment after it has no line end.
    last_line: usize,
}

impl<'a> Reader<'a> {
    fn new(chars: &'a [char]) -> Self {
        let last_line = chars
            .iter()
            .rposition(|c| *c == '\n')
            .map_or(0, |at| at + 1);
        Reader {
            chars,
            pos: 0,
            reach: 0,
            last_line,
        }
    }

    fn peek(&self) -> Option<char> {
        #[cfg(test)]
        crate::parse::PEEKS.with(|peeks| peeks.set(peeks.get() + 1));
        self.chars.get(self.pos).copied()
    }

    /// Skips white space and comments. A comment with no line end stays: the
    /// oracle does not read it as a comment here.
    fn skip_space(&mut self) {
        loop {
            match self.peek() {
                Some(c) if crate::parse::is_tex_space(c) => self.pos += 1,
                Some('%') if self.pos < self.last_line => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.pos += 1;
                    }
                    self.pos += 1;
                }
                _ => break,
            }
        }
        // A failed search after this skip counts the comment as read.
        self.reach = self.reach.max(self.pos);
    }

    fn eat(&mut self, c: char) -> bool {
        let found = self.peek() == Some(c);
        if found {
            self.pos += 1;
        }
        found
    }

    /// A control sequence at the cursor: the name without its backslash.
    fn control(&mut self) -> Option<Vec<char>> {
        if self.peek() != Some('\\') {
            return None;
        }
        let start = self.pos + 1;
        let mut end = start;
        while self.chars.get(end).is_some_and(char::is_ascii_alphabetic) {
            end += 1;
        }
        if end == start {
            self.chars.get(start)?;
            end = start + 1;
        }
        self.pos = end;
        self.chars.get(start..end).map(<[char]>::to_vec)
    }

    /// The content of a group in braces at the cursor.
    fn group(&mut self) -> Option<Vec<char>> {
        self.delimited('{', '}')
    }

    fn delimited(&mut self, open: char, close: char) -> Option<Vec<char>> {
        if self.peek() != Some(open) {
            return None;
        }
        let start = self.pos + 1;
        let mut level = 0usize;
        let mut i = start;
        loop {
            self.reach = self.reach.max(i);
            let c = *self.chars.get(i)?;
            // A comment runs to the line end, also after a backslash: the
            // oracle reads `\%` here as the start of a comment.
            if c == '%' || (c == '\\' && self.chars.get(i + 1) == Some(&'%')) {
                while self.chars.get(i).is_some_and(|c| *c != '\n') {
                    i += 1;
                }
                i += 1;
                continue;
            }
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == '{' {
                level += 1;
            } else if c == '}' && level > 0 {
                level -= 1;
            } else if c == close && level == 0 {
                break;
            } else if c == '}' {
                return None;
            }
            i += 1;
        }
        self.pos = i + 1;
        self.chars.get(start..i).map(<[char]>::to_vec)
    }

    /// The optional argument of a use at the cursor `[`: the text up to the
    /// first `]`. Braces do not count here.
    fn bracket_argument(&mut self) -> Option<Vec<char>> {
        let start = self.pos + 1;
        let mut i = start;
        loop {
            self.reach = self.reach.max(i);
            match *self.chars.get(i)? {
                '\\' => i += 2,
                ']' => break,
                _ => i += 1,
            }
        }
        self.pos = i + 1;
        self.chars.get(start..i).map(<[char]>::to_vec)
    }

    /// One argument of a macro: a group, a control sequence or a character.
    fn argument(&mut self) -> Option<Vec<char>> {
        self.skip_space();
        match self.peek()? {
            '{' => self.group(),
            '\\' => {
                let start = self.pos;
                self.control()?;
                self.chars.get(start..self.pos).map(<[char]>::to_vec)
            }
            '}' => None,
            c => {
                self.pos += 1;
                Some(vec![c])
            }
        }
    }

    /// One definition at the cursor. On `None` the cursor has no meaning.
    fn definition(&mut self) -> Option<(Vec<char>, Macro)> {
        let command: String = self.control()?.into_iter().collect();
        let operator = command == "DeclareMathOperator";
        if !operator
            && !matches!(
                command.as_str(),
                "newcommand" | "renewcommand" | "providecommand"
            )
        {
            return None;
        }
        let star = self.eat('*');
        self.skip_space();
        let name = if self.peek() == Some('{') {
            let inner = self.group()?;
            let mut reader = Reader::new(&inner);
            let mut name = reader.control()?;
            name.extend(inner.get(reader.pos..)?);
            name
        } else {
            self.control()?
        };
        self.skip_space();
        let mut params = 0;
        let mut default = None;
        // Braces do not protect a `]` in the brackets of a definition.
        if !operator && self.peek() == Some('[') {
            let count = self.bracket_argument()?;
            let mut digits = count.iter().filter(|c| !crate::parse::is_tex_space(**c));
            let (Some(digit), None) = (digits.next(), digits.next()) else {
                return None;
            };
            params = usize::try_from(digit.to_digit(10)?).ok()?;
            self.skip_space();
            if self.peek() == Some('[') {
                default = Some(self.bracket_argument()?);
                self.skip_space();
            }
        }
        let mut body = self.group()?;
        if operator {
            let prefix = if star {
                "\\operatorname*{"
            } else {
                "\\operatorname{"
            };
            let mut text: Vec<char> = prefix.chars().collect();
            text.append(&mut body);
            text.push('}');
            body = text;
        }
        let definition = Macro {
            params,
            default,
            body,
        };
        Some((name, definition))
    }
}

/// Reads the definitions at the start of `tex` and returns the rest of the
/// text with each macro use expanded.
pub(crate) fn expand(tex: &str) -> Vec<char> {
    let chars: Vec<char> = tex.chars().collect();
    if !tex.contains("command") && !tex.contains("\\DeclareMathOperator") {
        return chars;
    }
    let mut macros: HashMap<Vec<char>, Macro> = HashMap::new();
    let mut reader = Reader::new(&chars);
    let mut rest = 0;
    loop {
        reader.skip_space();
        let Some((name, definition)) = reader.definition() else {
            break;
        };
        macros.insert(name, definition);
        reader.skip_space();
        rest = reader.pos;
    }
    let mut text = chars.get(rest..).map(<[char]>::to_vec).unwrap_or_default();
    if macros.is_empty() {
        return text;
    }
    let limit = MAX_GROWTH + 4 * chars.len();
    let original = text.clone();
    for _ in 0..MAX_PASSES {
        match pass(&text, &macros, limit) {
            Some((next, true)) => text = next,
            Some((next, false)) => return next,
            None => break,
        }
    }
    // The oracle gives up on a macro that uses itself, and reads the text
    // with no expansion. The crate does the same at its limits.
    original
}

/// Expands each macro use of `text` one time. `None`: the result, or the
/// text that the pass read after the ends of the uses, is larger than `limit`.
fn pass(
    text: &[char],
    macros: &HashMap<Vec<char>, Macro>,
    limit: usize,
) -> Option<(Vec<char>, bool)> {
    let mut out = Vec::with_capacity(text.len());
    let mut changed = false;
    let mut reader = Reader::new(text);
    // The characters that a search for a closing delimiter read after the
    // end of its macro use. The limit keeps the time linear when many uses
    // search to the end of the text.
    let mut wasted = 0usize;
    while let Some(c) = reader.peek() {
        if out.len() > limit || wasted > limit {
            return None;
        }
        if c != '\\' {
            out.push(c);
            reader.pos += 1;
            continue;
        }
        let start = reader.pos;
        let found = reader.control().and_then(|name| macros.get(&name));
        let after_name = reader.pos.max(start + 1);
        reader.reach = after_name;
        if let Some(found) = found
            && let Some(args) = arguments(&mut reader, found)
        {
            out.push('{');
            substitute(&found.body, &args, &mut out, limit)?;
            out.push('}');
            changed = true;
        } else {
            reader.pos = after_name;
            out.extend(text.get(start..after_name).unwrap_or_default());
        }
        wasted += reader.reach.saturating_sub(reader.pos);
    }
    Some((out, changed))
}

/// The arguments of one use. `None`: the text does not hold all of them.
fn arguments(reader: &mut Reader, found: &Macro) -> Option<Vec<Vec<char>>> {
    let mut args = Vec::with_capacity(found.params);
    if found.params > 0
        && let Some(default) = &found.default
    {
        reader.skip_space();
        let given = if reader.peek() == Some('[') {
            reader.bracket_argument()
        } else {
            None
        };
        args.push(given.unwrap_or_else(|| default.clone()));
    }
    while args.len() < found.params {
        args.push(reader.argument()?);
    }
    Some(args)
}

/// Writes `body` with each `#n` replaced by its argument. `None`: the text
/// would become larger than `limit`; the check comes before each copy.
fn substitute(body: &[char], args: &[Vec<char>], out: &mut Vec<char>, limit: usize) -> Option<()> {
    let mut i = 0;
    while let Some(&c) = body.get(i) {
        i += 1;
        let arg = (c == '#')
            .then(|| body.get(i).and_then(|d| d.to_digit(10)))
            .flatten()
            .and_then(|n| usize::try_from(n).ok())
            .and_then(|n| n.checked_sub(1))
            .and_then(|n| args.get(n));
        let piece = match arg {
            Some(arg) => {
                i += 1;
                arg.as_slice()
            }
            None => std::slice::from_ref(&c),
        };
        if out.len() + piece.len() > limit {
            return None;
        }
        #[cfg(test)]
        COPIED.with(|copied| copied.set(copied.get() + piece.len()));
        out.extend_from_slice(piece);
    }
    Some(())
}

#[cfg(test)]
thread_local! {
    /// The number of characters that `substitute` copied on this thread.
    static COPIED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One use of a macro whose body holds its argument `n` times, with an
    /// argument of `n` characters: the result has `n * n` characters.
    fn square(n: usize) -> String {
        format!(
            "\\newcommand{{\\f}}[1]{{{}}}\\f{{{}}}",
            "#1".repeat(n),
            "x".repeat(n)
        )
    }

    fn copied_by_expand(tex: &str) -> usize {
        COPIED.with(|copied| copied.set(0));
        let _ = expand(tex);
        COPIED.with(std::cell::Cell::get)
    }

    #[test]
    fn substitution_stops_at_the_limit_before_it_copies() {
        for n in [263, 1_000, 4_000, 16_000] {
            let tex = square(n);
            let limit = MAX_GROWTH + 4 * tex.chars().count();
            let copied = copied_by_expand(&tex);
            assert!(
                copied <= limit,
                "n = {n}: {copied} characters copied, limit {limit}"
            );
        }
    }

    #[test]
    fn substitution_can_reach_the_limit_and_not_pass_it() {
        let (body, args) = (['#', '1'], [vec!['a', 'b']]);
        let mut out = Vec::new();
        assert!(substitute(&body, &args, &mut out, 2).is_some());
        assert_eq!(out, ['a', 'b']);
        assert!(substitute(&body, &args, &mut Vec::new(), 1).is_none());
    }

    #[test]
    fn a_space_before_the_optional_argument_is_skipped() {
        let text: String = expand("\\newcommand\\f[1][x]{#1}\\f \n [y]")
            .into_iter()
            .collect();
        assert_eq!(text, "{y}");
    }
}
