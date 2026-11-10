//! Parses the text of a Haskell value into a [`Value`], as the Haskell `Read`
//! class reads it.
//!
//! A constructor with arguments must be in parentheses when it is itself an
//! argument. A record, a nullary constructor and a negative number need no
//! parentheses there, as in `Read`.

use randoc_ast::Text;

use crate::value::Value;

/// The input is not a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParseError;

type Parsed<T> = Result<T, ParseError>;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Open,
    Close,
    OpenBracket,
    CloseBracket,
    OpenBrace,
    CloseBrace,
    Comma,
    Equals,
    Minus,
    Ident(Text),
    Str(Text),
    Char(char),
    Num(Text),
}

/// Returns `true` for the characters that the Haskell function `isSpace` accepts.
fn is_haskell_space(c: char) -> bool {
    matches!(c, ' ' | '\t'..='\r' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}')
        || matches!(c, '\u{202f}' | '\u{205f}' | '\u{3000}')
}

fn is_symbol(c: char) -> bool {
    "!#$%&*+./<=>?@\\^|-~:".contains(c)
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

/// The names of the ASCII control characters in escapes, longest first where
/// one name is a prefix of another.
const ASCII_ESCAPES: [(&str, u32); 34] = [
    ("NUL", 0),
    ("SOH", 1),
    ("STX", 2),
    ("ETX", 3),
    ("EOT", 4),
    ("ENQ", 5),
    ("ACK", 6),
    ("BEL", 7),
    ("BS", 8),
    ("HT", 9),
    ("LF", 10),
    ("VT", 11),
    ("FF", 12),
    ("CR", 13),
    ("SO", 14),
    ("SI", 15),
    ("DLE", 16),
    ("DC1", 17),
    ("DC2", 18),
    ("DC3", 19),
    ("DC4", 20),
    ("NAK", 21),
    ("SYN", 22),
    ("ETB", 23),
    ("CAN", 24),
    ("EM", 25),
    ("SUB", 26),
    ("ESC", 27),
    ("FS", 28),
    ("GS", 29),
    ("RS", 30),
    ("US", 31),
    ("SP", 32),
    ("DEL", 127),
];

struct Lexer<'a> {
    rest: &'a str,
}

impl<'a> Lexer<'a> {
    fn take_while(&mut self, keep: impl Fn(char) -> bool) -> &'a str {
        let end = self
            .rest
            .char_indices()
            .find(|&(_, c)| !keep(c))
            .map_or(self.rest.len(), |(index, _)| index);
        let (taken, rest) = self.rest.split_at(end);
        self.rest = rest;
        taken
    }

    fn next_char(&mut self) -> Option<char> {
        let mut chars = self.rest.chars();
        let c = chars.next()?;
        self.rest = chars.as_str();
        Some(c)
    }

    fn tokens(mut self) -> Parsed<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            self.take_while(is_haskell_space);
            let Some(c) = self.rest.chars().next() else {
                return Ok(tokens);
            };
            let token = match c {
                '(' | ')' | '[' | ']' | '{' | '}' | ',' => {
                    self.next_char();
                    match c {
                        '(' => Token::Open,
                        ')' => Token::Close,
                        '[' => Token::OpenBracket,
                        ']' => Token::CloseBracket,
                        '{' => Token::OpenBrace,
                        '}' => Token::CloseBrace,
                        _ => Token::Comma,
                    }
                }
                '\'' => {
                    self.next_char();
                    let c = match self.next_char().ok_or(ParseError)? {
                        '\\' => self.escape()?.ok_or(ParseError)?,
                        '\'' => return Err(ParseError),
                        c => c,
                    };
                    if self.next_char() != Some('\'') {
                        return Err(ParseError);
                    }
                    Token::Char(c)
                }
                '"' => {
                    self.next_char();
                    Token::Str(self.string()?)
                }
                '0'..='9' => Token::Num(self.number()),
                _ if is_symbol(c) => match self.take_while(is_symbol) {
                    "-" => Token::Minus,
                    "=" => Token::Equals,
                    _ => return Err(ParseError),
                },
                _ if c.is_alphabetic() || c == '_' => {
                    Token::Ident(Text::from(self.take_while(is_ident_char)))
                }
                _ => return Err(ParseError),
            };
            tokens.push(token);
        }
    }

    fn number(&mut self) -> Text {
        let rest = self.rest;
        let bytes = rest.as_bytes();
        let radix_digits = |prefix: u8, keep: fn(&u8) -> bool| {
            (bytes.first() == Some(&b'0')
                && bytes.get(1).map(u8::to_ascii_lowercase) == Some(prefix)
                && bytes.get(2).is_some_and(keep))
            .then(|| 2 + bytes[2..].iter().take_while(|b| keep(b)).count())
        };
        let end = radix_digits(b'x', u8::is_ascii_hexdigit)
            .or_else(|| radix_digits(b'o', |b| (b'0'..=b'7').contains(b)))
            .unwrap_or_else(|| {
                let digits = |from: usize| {
                    bytes[from..]
                        .iter()
                        .take_while(|b| b.is_ascii_digit())
                        .count()
                };
                let mut end = digits(0);
                if bytes.get(end) == Some(&b'.')
                    && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)
                {
                    end += 1 + digits(end + 1);
                }
                if matches!(bytes.get(end), Some(b'e' | b'E')) {
                    let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
                    if bytes.get(end + 1 + sign).is_some_and(u8::is_ascii_digit) {
                        end += 1 + sign + digits(end + 1 + sign);
                    }
                }
                end
            });
        let (taken, rest) = rest.split_at(end);
        self.rest = rest;
        Text::from(taken)
    }

    /// Reads a string literal after its opening quote.
    fn string(&mut self) -> Parsed<Text> {
        let mut out = Text::new();
        loop {
            match self.next_char().ok_or(ParseError)? {
                '"' => return Ok(out),
                '\\' => {
                    if let Some(c) = self.escape()? {
                        out.push(c);
                    }
                }
                c => out.push(c),
            }
        }
    }

    /// Reads an escape after its backslash. `None` is the empty escape `\&` or a gap.
    fn escape(&mut self) -> Parsed<Option<char>> {
        let c = self.next_char().ok_or(ParseError)?;
        let simple = match c {
            'a' => Some('\u{7}'),
            'b' => Some('\u{8}'),
            'f' => Some('\u{c}'),
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            'v' => Some('\u{b}'),
            '\\' | '"' | '\'' => Some(c),
            _ => None,
        };
        if simple.is_some() {
            return Ok(simple);
        }
        let code = match c {
            '&' => return Ok(None),
            _ if is_haskell_space(c) => {
                self.take_while(is_haskell_space);
                return match self.next_char() {
                    Some('\\') => Ok(None),
                    _ => Err(ParseError),
                };
            }
            '^' => match self.next_char() {
                Some(control @ '@'..='_') => control as u32 - 64,
                _ => return Err(ParseError),
            },
            '0'..='9' => {
                let digits = format!("{c}{}", self.take_while(|d| d.is_ascii_digit()));
                code_point(&digits, 10)?
            }
            'x' | 'X' => code_point(self.take_while(|d| d.is_ascii_hexdigit()), 16)?,
            'o' | 'O' => code_point(self.take_while(|d| ('0'..='7').contains(&d)), 8)?,
            _ => {
                let (tail, code) = ASCII_ESCAPES
                    .iter()
                    .filter_map(|(name, code)| Some((name.strip_prefix(c)?, *code)))
                    .filter(|(tail, _)| self.rest.starts_with(tail))
                    .max_by_key(|(tail, _)| tail.len())
                    .ok_or(ParseError)?;
                self.rest = self.rest.get(tail.len()..).ok_or(ParseError)?;
                code
            }
        };
        // Data.Text replaces a surrogate code point with U+FFFD.
        Ok(Some(char::from_u32(code).unwrap_or('\u{fffd}')))
    }
}

fn code_point(digits: &str, radix: u32) -> Parsed<u32> {
    if digits.is_empty() {
        return Err(ParseError);
    }
    let mut value: u32 = 0;
    for digit in digits.chars() {
        let digit = digit.to_digit(radix).ok_or(ParseError)?;
        value = value
            .checked_mul(radix)
            .and_then(|v| v.checked_add(digit))
            .filter(|v| *v <= 0x10ffff)
            .ok_or(ParseError)?;
    }
    Ok(value)
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
    depth: usize,
    max_depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn next(&mut self) -> Parsed<Token> {
        let token = self.tokens.get(self.position).cloned().ok_or(ParseError)?;
        self.position += 1;
        Ok(token)
    }

    fn expect(&mut self, expected: &Token) -> Parsed<()> {
        if self.next()? == *expected {
            Ok(())
        } else {
            Err(ParseError)
        }
    }

    fn enter(&mut self) -> Parsed<()> {
        self.depth += 1;
        if self.depth > self.max_depth {
            Err(ParseError)
        } else {
            Ok(())
        }
    }

    /// A value at precedence 0: a constructor with its arguments, or an atom.
    fn expression(&mut self) -> Parsed<Value> {
        self.enter()?;
        let value = match self.peek() {
            Some(Token::Ident(_))
                if !matches!(self.tokens.get(self.position + 1), Some(Token::OpenBrace)) =>
            {
                let Token::Ident(name) = self.next()? else {
                    return Err(ParseError);
                };
                let mut args = Vec::new();
                while self.starts_atom() {
                    args.push(self.atom()?);
                }
                Value::Con(name, args)
            }
            _ => self.atom()?,
        };
        self.depth -= 1;
        Ok(value)
    }

    fn starts_atom(&self) -> bool {
        matches!(
            self.peek(),
            Some(
                Token::Open
                    | Token::OpenBracket
                    | Token::Ident(_)
                    | Token::Str(_)
                    | Token::Char(_)
                    | Token::Num(_)
                    | Token::Minus
            )
        )
    }

    fn atom(&mut self) -> Parsed<Value> {
        self.enter()?;
        let value = match self.next()? {
            Token::Open => {
                let mut items = vec![self.expression()?];
                while self.peek() == Some(&Token::Comma) {
                    self.position += 1;
                    items.push(self.expression()?);
                }
                self.expect(&Token::Close)?;
                match items.len() {
                    1 => items.pop().ok_or(ParseError)?,
                    _ => Value::Tuple(items),
                }
            }
            Token::OpenBracket => {
                let mut items = Vec::new();
                if self.peek() == Some(&Token::CloseBracket) {
                    self.position += 1;
                } else {
                    items.push(self.expression()?);
                    while self.next()? == Token::Comma {
                        items.push(self.expression()?);
                    }
                    if self.tokens.get(self.position - 1) != Some(&Token::CloseBracket) {
                        return Err(ParseError);
                    }
                }
                Value::List(items)
            }
            Token::Str(text) => Value::Str(text),
            Token::Char(c) => Value::Char(c),
            Token::Num(digits) => Value::Num {
                negative: false,
                digits,
            },
            Token::Minus => match self.next()? {
                Token::Num(digits) | Token::Ident(digits) => Value::Num {
                    negative: true,
                    digits,
                },
                _ => return Err(ParseError),
            },
            Token::Ident(name) => {
                if self.peek() == Some(&Token::OpenBrace) {
                    self.position += 1;
                    Value::Rec(name, self.fields()?)
                } else {
                    Value::Con(name, Vec::new())
                }
            }
            _ => return Err(ParseError),
        };
        self.depth -= 1;
        Ok(value)
    }

    fn fields(&mut self) -> Parsed<Vec<(Text, Value)>> {
        let mut fields = Vec::new();
        if self.peek() == Some(&Token::CloseBrace) {
            self.position += 1;
            return Ok(fields);
        }
        loop {
            let Token::Ident(name) = self.next()? else {
                return Err(ParseError);
            };
            self.expect(&Token::Equals)?;
            fields.push((name, self.expression()?));
            match self.next()? {
                Token::Comma => {}
                Token::CloseBrace => return Ok(fields),
                _ => return Err(ParseError),
            }
        }
    }
}

/// Parses one value that fills the whole text. `max_depth` limits the nesting.
pub(crate) fn parse(text: &str, max_depth: usize) -> Parsed<Value> {
    let tokens = Lexer { rest: text }.tokens()?;
    let mut parser = Parser {
        tokens,
        position: 0,
        depth: 0,
        max_depth,
    };
    let value = parser.expression()?;
    if parser.position == parser.tokens.len() {
        Ok(value)
    } else {
        Err(ParseError)
    }
}
