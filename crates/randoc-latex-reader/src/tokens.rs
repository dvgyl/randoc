//! The token level: TeX category codes of a plain LaTeX document.
//!
//! The tokenizer keeps the source text of each token, so that the parser can
//! give raw TeX (`raw_tex`) and math as the text of the input.

use std::collections::HashMap;

use randoc_ast::Text;

/// The longest environment name that the tokenizer looks for after `\begin`.
const MAX_NAME_LENGTH: usize = 64;
/// The longest verbatim URL argument; a longer one is read as tokens.
const MAX_URL_LENGTH: usize = 8192;

/// The environments whose body is verbatim text.
pub(crate) const VERBATIM_ENVIRONMENTS: &[&str] =
    &["verbatim", "Verbatim", "lstlisting", "minted", "comment"];

/// One token.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    /// A control word, without the backslash: `\emph` is `Word("emph")`.
    Word(Text),
    /// A control symbol, without the backslash: `\%` is `Symbol('%')`.
    Symbol(char),
    /// A run of letters and digits.
    Letters(Text),
    /// One character with no special meaning.
    Other(char),
    /// Spaces and tabs.
    Space,
    /// One end of line.
    Newline,
    /// An end of line and one or more blank lines: a paragraph break.
    Par,
    /// A comment, from `%` to the end of the line.
    Comment,
    /// `{`
    Open,
    /// `}`
    Close,
    /// `$`
    Dollar,
    /// `&`
    Tab,
    /// `#` with a parameter number, or 0 for a `#` without a digit.
    Param(u8),
    /// `^`
    Sup,
    /// `_`
    Sub,
    /// `~`
    Tilde,
    /// The body of `\verb`: the text between the delimiters.
    Verb(Text),
    /// The body of a verbatim environment, up to its `\end`.
    Verbatim(Text),
}

/// A token with its source text, its position and its macro expansion depth.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Tok {
    pub(crate) kind: Kind,
    pub(crate) text: Text,
    pub(crate) line: u32,
    pub(crate) column: u32,
    /// The number of macro expansions that made this token. Source tokens have 0.
    pub(crate) depth: u32,
}

impl Tok {
    pub(crate) fn is_word(&self, name: &str) -> bool {
        matches!(&self.kind, Kind::Word(word) if word == name)
    }
}

/// Splits the input into tokens.
pub(crate) fn tokenize(input: &str) -> Vec<Tok> {
    Tokenizer {
        chars: input.chars().collect(),
        index: 0,
        line: 1,
        column: 1,
        out: Vec::new(),
        verb_failures: HashMap::new(),
    }
    .run()
}

struct Tokenizer {
    chars: Vec<char>,
    index: usize,
    line: u32,
    column: u32,
    out: Vec<Tok>,
    /// For each `\verb` delimiter, the line end before which the delimiter
    /// does not occur again. It keeps repeated `\verb` without an end linear.
    verb_failures: HashMap<char, usize>,
}

impl Tokenizer {
    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.index + offset).copied()
    }

    /// Takes `count` characters as one token.
    fn take(&mut self, kind: Kind, count: usize) {
        let end = (self.index + count).min(self.chars.len());
        let text: Text = self.chars[self.index..end].iter().collect();
        let (line, column) = (self.line, self.column);
        for &c in &self.chars[self.index..end] {
            if c == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }
        self.index = end;
        self.out.push(Tok {
            kind,
            text,
            line,
            column,
            depth: 0,
        });
    }

    fn count_while(&self, from: usize, test: impl Fn(char) -> bool) -> usize {
        self.count_while_max(from, usize::MAX, test)
    }

    fn count_while_max(&self, from: usize, max: usize, test: impl Fn(char) -> bool) -> usize {
        self.chars
            .get(self.index + from..)
            .unwrap_or_default()
            .iter()
            .take(max)
            .take_while(|&&c| test(c))
            .count()
    }

    fn run(mut self) -> Vec<Tok> {
        while let Some(c) = self.peek(0) {
            match c {
                '\\' => self.control_sequence(),
                '{' => self.take(Kind::Open, 1),
                '}' => self.take(Kind::Close, 1),
                '$' => self.take(Kind::Dollar, 1),
                '&' => self.take(Kind::Tab, 1),
                '^' => self.take(Kind::Sup, 1),
                '_' => self.take(Kind::Sub, 1),
                '~' => self.take(Kind::Tilde, 1),
                '#' => match self.peek(1).and_then(|d| d.to_digit(10)) {
                    Some(digit) => self.take(Kind::Param(digit as u8), 2),
                    None => self.take(Kind::Param(0), 1),
                },
                '%' => {
                    let length = self.count_while(0, |c| c != '\n');
                    self.take(Kind::Comment, length);
                }
                ' ' | '\t' => {
                    let length = self.count_while(0, |c| c == ' ' || c == '\t');
                    self.take(Kind::Space, length);
                }
                '\n' => self.newline(),
                c if c.is_alphanumeric() => {
                    let length = self.count_while(0, char::is_alphanumeric);
                    let text = self.chars[self.index..self.index + length].iter().collect();
                    self.take(Kind::Letters(text), length);
                }
                c => self.take(Kind::Other(c), 1),
            }
        }
        self.out
    }

    /// A newline, or a paragraph break when blank lines follow it.
    fn newline(&mut self) {
        let mut length = 1;
        let mut breaks = 0;
        loop {
            let blank = self.count_while(length, |c| c == ' ' || c == '\t');
            if self.peek(length + blank) == Some('\n') {
                length += blank + 1;
                breaks += 1;
            } else {
                break;
            }
        }
        let at_end = self.index + length + self.count_while(length, |c| c == ' ' || c == '\t')
            >= self.chars.len();
        let kind = if breaks == 0 && !at_end {
            Kind::Newline
        } else {
            Kind::Par
        };
        self.take(kind, length);
    }

    fn control_sequence(&mut self) {
        let letters = self.count_while(1, |c| c.is_ascii_alphabetic());
        if letters == 0 {
            match self.peek(1) {
                Some(symbol) => self.take(Kind::Symbol(symbol), 2),
                None => self.take(Kind::Other('\\'), 1),
            }
            return;
        }
        let name: Text = self.chars[self.index + 1..self.index + 1 + letters]
            .iter()
            .collect();
        self.take(Kind::Word(name.clone()), letters + 1);
        match name.as_str() {
            "verb" => self.verb(),
            "begin" => self.verbatim_environment(),
            "url" | "href" => self.verbatim_argument(),
            _ => {}
        }
    }

    /// `\verb` takes the text up to the next delimiter on the same line.
    fn verb(&mut self) {
        let star = usize::from(self.peek(0) == Some('*'));
        let Some(delimiter) = self.peek(star) else {
            return;
        };
        if delimiter.is_alphabetic() || delimiter == ' ' || delimiter == '\n' {
            return;
        }
        if self
            .verb_failures
            .get(&delimiter)
            .is_some_and(|&line_end| self.index < line_end)
        {
            return;
        }
        let body = self.count_while(star + 1, |c| c != delimiter && c != '\n');
        if self.peek(star + 1 + body) != Some(delimiter) {
            self.verb_failures
                .insert(delimiter, self.index + star + 1 + body);
            return;
        }
        let text = self.chars[self.index + star + 1..self.index + star + 1 + body]
            .iter()
            .collect();
        self.take(Kind::Verb(text), star + body + 2);
    }

    /// The argument of `\url` and the first argument of `\href` are
    /// verbatim: `%` and `#` are characters there.
    fn verbatim_argument(&mut self) {
        let spaces = self.count_while(0, |c| c == ' ');
        if self.peek(spaces) != Some('{') {
            return;
        }
        let length = self.count_while_max(spaces + 1, MAX_URL_LENGTH, |c| c != '}' && c != '\n');
        if self.peek(spaces + 1 + length) != Some('}') {
            return;
        }
        if spaces > 0 {
            self.take(Kind::Space, spaces);
        }
        self.take(Kind::Open, 1);
        let text = self.chars[self.index..self.index + length].iter().collect();
        self.take(Kind::Verb(text), length);
        self.take(Kind::Close, 1);
    }

    /// After `\begin{NAME}` of a verbatim environment, the text up to
    /// `\end{NAME}` is one token.
    fn verbatim_environment(&mut self) {
        let name_length = self.count_while_max(1, MAX_NAME_LENGTH, |c| c != '}' && c != '\n');
        if self.peek(0) != Some('{') || self.peek(1 + name_length) != Some('}') {
            return;
        }
        let name: Text = self.chars[self.index + 1..self.index + 1 + name_length]
            .iter()
            .collect();
        if !VERBATIM_ENVIRONMENTS.contains(&name.as_str()) {
            return;
        }
        self.take(Kind::Open, 1);
        self.take(Kind::Letters(name.clone()), name_length);
        self.take(Kind::Close, 1);
        let end: Vec<char> = format!("\\end{{{name}}}").chars().collect();
        let rest = &self.chars[self.index..];
        let length = rest
            .windows(end.len())
            .position(|window| window == end.as_slice())
            .unwrap_or(rest.len());
        let body = rest[..length].iter().collect();
        self.take(Kind::Verbatim(body), length);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(input: &str) -> Vec<Kind> {
        tokenize(input).into_iter().map(|tok| tok.kind).collect()
    }

    #[test]
    fn control_words_symbols_and_groups() {
        assert_eq!(
            kinds("\\emph{a1}\\% x"),
            vec![
                Kind::Word("emph".into()),
                Kind::Open,
                Kind::Letters("a1".into()),
                Kind::Close,
                Kind::Symbol('%'),
                Kind::Space,
                Kind::Letters("x".into()),
            ]
        );
    }

    #[test]
    fn blank_lines_make_one_paragraph_break() {
        assert_eq!(
            kinds("a\n \n\nb\nc"),
            vec![
                Kind::Letters("a".into()),
                Kind::Par,
                Kind::Letters("b".into()),
                Kind::Newline,
                Kind::Letters("c".into()),
            ]
        );
    }

    #[test]
    fn verbatim_forms_keep_their_text() {
        assert_eq!(kinds("\\verb|a %b|")[1], Kind::Verb("a %b".into()));
        let tokens = kinds("\\begin{verbatim}x}%\n\\end{verbatim}");
        assert_eq!(tokens[4], Kind::Verbatim("x}%\n".into()));
        assert_eq!(tokens[5], Kind::Word("end".into()));
    }

    #[test]
    fn positions_count_lines_and_columns() {
        let tokens = tokenize("a\n \\b");
        let last = &tokens[tokens.len() - 1];
        assert_eq!((last.line, last.column), (2, 2));
    }
}
