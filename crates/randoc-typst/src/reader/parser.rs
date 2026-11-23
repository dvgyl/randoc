use randoc_core::{Error, Result};

#[derive(Debug)]
pub(super) enum Mark<'a> {
    Text(&'a str),
    Space,
    Break(usize),
    Paragraph(usize),
    Expression(Expr<'a>),
    Styled(&'static str, Vec<Mark<'a>>),
    Heading(i64, Vec<Mark<'a>>, &'a str),
    Label(&'a str, bool),
    Reference(&'a str, Vec<Mark<'a>>),
    Raw(&'a str, &'a str, bool),
    Math(&'a str, usize),
    Link(&'a str),
    List(&'a str, usize, Vec<Mark<'a>>),
    Term(usize, Vec<Mark<'a>>, Vec<Mark<'a>>),
    Escape(char),
}

#[derive(Debug)]
pub(super) struct Expr<'a> {
    pub pos: usize,
    pub kind: Kind<'a>,
}

#[derive(Debug)]
pub(super) enum Kind<'a> {
    Name(&'a str),
    Literal(&'a str),
    Field(Box<Expr<'a>>, Vec<&'a str>),
    Args(Vec<Arg<'a>>),
    Call(Box<Expr<'a>>, Vec<Arg<'a>>),
    Unary(&'a str, Box<Expr<'a>>),
    Binary(Box<Expr<'a>>, Vec<(&'a str, Expr<'a>)>),
    Let(Box<Expr<'a>>, Option<Box<Expr<'a>>>),
    Closure(Box<Expr<'a>>, Box<Expr<'a>>),
    If(Box<Expr<'a>>, Box<Expr<'a>>, Option<Box<Expr<'a>>>),
    For(Box<Expr<'a>>, Box<Expr<'a>>, Box<Expr<'a>>),
    While(Box<Expr<'a>>, Box<Expr<'a>>),
    Set(Box<Expr<'a>>, Option<Box<Expr<'a>>>),
    Show(Option<Box<Expr<'a>>>, Box<Expr<'a>>),
    Include(Box<Expr<'a>>),
    Import(Box<Expr<'a>>, Vec<&'a str>),
    Break,
    Continue,
    Return(Option<Box<Expr<'a>>>),
    Block(Vec<Expr<'a>>),
    Content(Vec<Mark<'a>>),
    Raw(&'a str, &'a str, bool),
    Math(&'a str),
}

#[derive(Debug)]
pub(super) struct Arg<'a> {
    pub name: Option<&'a str>,
    pub spread: bool,
    pub expr: Expr<'a>,
}

pub(super) fn parse<'a>(source: &'a str, name: &str, depth: usize) -> Result<Vec<Mark<'a>>> {
    Parser {
        source,
        pos: 0,
        max_depth: depth,
    }
    .markup(None, depth)
    .map_err(|error| with_source_name(error, name))
}

pub(super) fn with_source_name(error: Error, name: &str) -> Error {
    match error {
        Error::Parse(message) if !name.is_empty() => Error::Parse(format!("{name:?} {message}")),
        error => error,
    }
}

pub(super) fn error(source: &str, pos: usize, message: &str) -> Error {
    let before = &source[..pos.min(source.len())];
    let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .map_or(1, |tail| tail.chars().count() + 1);
    Error::Parse(format!("(line {line}, column {column}):\n{message}"))
}

struct Parser<'a> {
    source: &'a str,
    pos: usize,
    max_depth: usize,
}

impl<'a> Parser<'a> {
    fn tail(&self) -> &'a str {
        &self.source[self.pos..]
    }
    fn peek(&self) -> Option<char> {
        self.tail().chars().next()
    }
    fn step(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }
    fn take(&mut self, token: &str) -> bool {
        if self.tail().starts_with(token) {
            self.pos += token.len();
            true
        } else {
            false
        }
    }
    fn scan(&mut self, predicate: impl Fn(char) -> bool) -> &'a str {
        let start = self.pos;
        while self.peek().is_some_and(&predicate) {
            self.step();
        }
        &self.source[start..self.pos]
    }
    fn fail(&self, message: &str) -> Error {
        if self.pos == self.source.len() {
            let line = self.source.lines().count() + 2;
            Error::Parse(format!("(line {line}, column 1):\n{message}"))
        } else {
            error(self.source, self.pos, message)
        }
    }
    fn expect(&mut self, token: &str) -> Result<()> {
        if self.take(token) {
            Ok(())
        } else {
            Err(self.fail(&format!("Expected {token:?}.")))
        }
    }
    fn comment(&mut self) -> Result<bool> {
        if self.take("//") {
            self.scan(|c| c != '\n');
            return Ok(true);
        }
        if !self.take("/*") {
            return Ok(false);
        }
        let mut nesting = 1;
        while nesting != 0 {
            if self.take("*/") {
                nesting -= 1;
            } else if self.take("/*") {
                nesting += 1;
                if nesting > self.max_depth {
                    return Err(self.fail("The Typst comment is nested too deeply."));
                }
            } else if self.step().is_none() {
                return Err(self.fail("Unclosed Typst comment."));
            }
        }
        Ok(true)
    }
    fn whitespace(&mut self, lines: bool) -> Result<()> {
        loop {
            self.scan(|c| c.is_whitespace() && (lines || c != '\n'));
            if !self.comment()? {
                return Ok(());
            }
        }
    }
    fn word(&mut self) -> &'a str {
        if self.peek().is_some_and(|c| c.is_alphabetic() || c == '_') {
            self.scan(is_name)
        } else {
            ""
        }
    }
    fn keyword(&mut self, name: &str) -> bool {
        if self
            .tail()
            .strip_prefix(name)
            .is_some_and(|tail| !tail.starts_with(is_name))
        {
            self.pos += name.len();
            true
        } else {
            false
        }
    }
    fn label(&mut self) -> Option<&'a str> {
        let start = self.pos;
        if self.take("<") {
            let id = self.scan(is_label);
            if !id.is_empty() && self.take(">") {
                return Some(id);
            }
        }
        self.pos = start;
        None
    }
    fn raw(&mut self) -> Result<(&'a str, &'a str, bool)> {
        let ticks = self.scan(|c| c == '`');
        if ticks.len() == 2 {
            return Ok(("", "", false));
        }
        let start = self.pos;
        let Some(end) = self.tail().find(ticks) else {
            self.pos = self.source.len();
            return Err(self.fail("Unclosed raw text."));
        };
        self.pos += end + ticks.len();
        let value = &self.source[start..start + end];
        if ticks.len() >= 3
            && let Some((lang, code)) = value.split_once('\n')
        {
            return Ok((
                lang.trim(),
                code.trim_end_matches(['\r', ' ', '\t'])
                    .strip_suffix('\n')
                    .unwrap_or(code)
                    .trim_end_matches('\r'),
                true,
            ));
        }
        Ok(("", value, false))
    }
    fn string(&mut self) -> Result<&'a str> {
        let start = self.pos;
        self.expect("\"")?;
        loop {
            match self.step() {
                Some('"') => return Ok(&self.source[start..self.pos]),
                Some('\\') => {
                    self.step();
                }
                None => return Err(self.fail("Unclosed string.")),
                _ => {}
            }
        }
    }
    fn math(&mut self) -> Result<&'a str> {
        self.expect("$")?;
        let start = self.pos;
        loop {
            match self.peek() {
                Some('$') => {
                    let math = &self.source[start..self.pos];
                    self.step();
                    return Ok(math);
                }
                Some('"') => {
                    self.string()?;
                }
                Some('\\') => {
                    self.step();
                    self.step();
                }
                Some(_) => {
                    self.step();
                }
                None => return Err(self.fail("Unclosed equation.")),
            }
        }
    }
    fn markup(&mut self, end: Option<char>, depth: usize) -> Result<Vec<Mark<'a>>> {
        if depth == 0 {
            return Err(self.fail("The Typst document is nested too deeply."));
        }
        let mut marks = Vec::new();
        let mut line_start = end.is_none() || end == Some(']');
        let mut indent = 0;
        while let Some(c) = self.peek() {
            if Some(c) == end || c == '\n' && end == Some('\r') || c == ']' && end == Some('\r') {
                if end != Some('\r') {
                    self.step();
                }
                if c == ']' && matches!(marks.last(), Some(Mark::Space)) {
                    marks.pop();
                }
                return Ok(marks);
            }
            if self.comment()? {
                continue;
            }
            if c == '\n' || c == '\r' {
                self.step();
                if c == '\r' {
                    self.take("\n");
                }
                indent = self.scan(|c| c == ' ' || c == '\t').len();
                if matches!(marks.last(), Some(Mark::Space)) {
                    marks.pop();
                }
                if matches!(marks.last(), Some(Mark::Break(_) | Mark::Paragraph(_))) {
                    marks.pop();
                    marks.push(Mark::Paragraph(indent));
                } else {
                    marks.push(Mark::Break(indent));
                }
                line_start = true;
                continue;
            }
            if c == ' ' || c == '\t' {
                let n = self.scan(|c| c == ' ' || c == '\t').len();
                if line_start {
                    indent += n;
                } else {
                    marks.push(Mark::Space);
                }
                continue;
            }
            if line_start && c == '=' {
                let start = self.pos;
                let level = self.scan(|c| c == '=').len();
                if self.take(" ") {
                    self.scan(|c| c == ' ' || c == '\t');
                    let mut title = self.markup(Some('\r'), depth - 1)?;
                    let mut id = "";
                    let mut separate = None;
                    if let Some(Mark::Label(label, tight)) = title.last() {
                        if *tight {
                            id = label;
                        } else {
                            separate = Some(*label);
                        }
                        title.pop();
                    } else {
                        let before = self.pos;
                        if self.take("\r\n") || self.take("\n") || self.take("\r") {
                            self.scan(|c| c == ' ' || c == '\t');
                            if let Some(label) = self.label() {
                                id = label;
                            } else {
                                self.pos = before;
                            }
                        }
                    }
                    marks.push(Mark::Heading(level as i64, title, id));
                    if let Some(label) = separate {
                        marks.push(Mark::Label(label, false));
                    }
                    line_start = false;
                    continue;
                }
                self.pos = start;
            }
            if line_start && matches!(c, '-' | '+' | '/' | '0'..='9') {
                let start = self.pos;
                let marker = if c.is_ascii_digit() {
                    self.scan(|c| c.is_ascii_digit());
                    if self.take(".") {
                        &self.source[start..self.pos]
                    } else {
                        ""
                    }
                } else {
                    self.step();
                    &self.source[start..self.pos]
                };
                if !marker.is_empty() && self.take(" ") {
                    if marker == "/" {
                        let term = self.markup(Some(':'), depth - 1)?;
                        self.scan(|c| c == ' ' || c == '\t');
                        let desc = self.markup(Some('\r'), depth - 1)?;
                        marks.push(Mark::Term(indent, term, desc));
                    } else {
                        marks.push(Mark::List(
                            marker,
                            indent,
                            self.markup(Some('\r'), depth - 1)?,
                        ));
                    }
                    line_start = false;
                    continue;
                }
                self.pos = start;
            }
            line_start = false;
            let start = self.pos;
            let mark = match c {
                '#' => {
                    self.step();
                    let expression = self.expr(depth, 0, false)?;
                    let statement = matches!(
                        expression.kind,
                        Kind::Let(..) | Kind::Set(..) | Kind::Show(..) | Kind::Import(..)
                    );
                    let consumes = self.take(";");
                    if statement && !consumes {
                        self.scan(|c| c == ' ' || c == '\t' || c == '\r');
                        self.comment()?;
                        if self.take("\n") {
                            line_start = true;
                        }
                    }
                    Mark::Expression(expression)
                }
                '`' => {
                    let (lang, source, block) = self.raw()?;
                    Mark::Raw(lang, source, block)
                }
                '$' => Mark::Math(self.math()?, start),
                '*' | '_' if !self.source[..start].ends_with(char::is_alphanumeric) => {
                    self.step();
                    if self.tail().contains(c) {
                        Mark::Styled(
                            if c == '*' { "strong" } else { "emph" },
                            self.markup(Some(c), depth - 1)?,
                        )
                    } else {
                        Mark::Text(&self.source[start..self.pos])
                    }
                }
                '<' => {
                    if let Some(id) = self.label() {
                        Mark::Label(id, !self.source[..start].ends_with(char::is_whitespace))
                    } else {
                        self.step();
                        Mark::Text(&self.source[start..self.pos])
                    }
                }
                '@' => {
                    self.step();
                    let value = self.scan(is_label).trim_end_matches(['.', ':']);
                    self.pos = start + 1 + value.len();
                    if value.is_empty() {
                        Mark::Text("@")
                    } else {
                        let body = if self.take("[") {
                            self.markup(Some(']'), depth - 1)?
                        } else {
                            Vec::new()
                        };
                        Mark::Reference(value, body)
                    }
                }
                '\\' => {
                    self.step();
                    if self.take("u{") {
                        let digits = self.scan(|c| c.is_ascii_hexdigit());
                        self.expect("}")?;
                        let value = u32::from_str_radix(digits, 16)
                            .ok()
                            .and_then(char::from_u32)
                            .ok_or_else(|| self.fail("Invalid Unicode escape."))?;
                        Mark::Escape(value)
                    } else if self.peek().is_none_or(char::is_whitespace) {
                        self.scan(|c| c.is_whitespace() && c != '\n');
                        if self.take("\n") {
                            line_start = true;
                        }
                        Mark::Styled("linebreak", Vec::new())
                    } else {
                        Mark::Escape(self.step().unwrap_or('\\'))
                    }
                }
                '(' if self.tail().starts_with("(http") => {
                    self.step();
                    Mark::Text("(")
                }
                'h' if self.tail().starts_with("http://")
                    || self.tail().starts_with("https://") =>
                {
                    let url = self
                        .scan(|c| !c.is_whitespace() && !matches!(c, '[' | ']' | '<' | '>' | '"'))
                        .trim_end_matches(['.', ',', ';', ')']);
                    self.pos = start + url.len();
                    Mark::Link(url)
                }
                _ => {
                    self.step();
                    self.scan(|c| {
                        !c.is_whitespace()
                            && !matches!(
                                c,
                                '#' | '`'
                                    | '$'
                                    | '*'
                                    | '_'
                                    | '<'
                                    | '@'
                                    | '\\'
                                    | '/'
                                    | ']'
                                    | ':'
                                    | '('
                            )
                            && Some(c) != end
                    });
                    Mark::Text(&self.source[start..self.pos])
                }
            };
            marks.push(mark);
        }
        if end.is_some_and(|c| c != '\r') {
            return Err(self.fail(&format!("Unclosed Typst markup. Expected {end:?}.")));
        }
        Ok(marks)
    }

    fn expr(&mut self, depth: usize, precedence: u8, code: bool) -> Result<Expr<'a>> {
        if depth == 0 {
            return Err(self.fail("The Typst expression is nested too deeply."));
        }
        let start = self.pos;
        let kind = self.atom(depth - 1)?;
        let mut left = Expr { pos: start, kind };
        let mut calls = 0;
        let mut operators = Vec::new();
        loop {
            let end = self.pos;
            if code {
                self.whitespace(false)?;
                if self.peek() == Some('\n') {
                    self.whitespace(true)?;
                    if !self.tail().starts_with('.')
                        && !self.tail().starts_with("and ")
                        && !self.tail().starts_with("or ")
                    {
                        self.pos = end;
                        break;
                    }
                }
            }
            if operators.is_empty() && self.take(".") {
                let field = self.word();
                if !field.is_empty() {
                    if let Kind::Field(_, fields) = &mut left.kind {
                        fields.push(field);
                    } else {
                        left = Expr {
                            pos: start,
                            kind: Kind::Field(Box::new(left), vec![field]),
                        };
                    }
                    continue;
                }
                self.pos = end;
            }
            if self.take("(") {
                calls += 1;
                if calls >= depth {
                    return Err(self.fail("The Typst expression is nested too deeply."));
                }
                let args = self.args(depth - 1)?;
                left = Expr {
                    pos: start,
                    kind: Kind::Call(Box::new(left), args),
                };
                continue;
            }
            if self.pos == end && self.take("[") {
                let pos = self.pos;
                let arg = Arg {
                    name: None,
                    spread: false,
                    expr: Expr {
                        pos,
                        kind: Kind::Content(self.markup(Some(']'), depth - 1)?),
                    },
                };
                match &mut left.kind {
                    Kind::Call(_, args) => args.push(arg),
                    _ => {
                        left = Expr {
                            pos: start,
                            kind: Kind::Call(Box::new(left), vec![arg]),
                        }
                    }
                }
                continue;
            }
            if !code {
                self.pos = end;
                break;
            }
            if precedence == 0 && self.take("=>") {
                self.whitespace(true)?;
                let body = self.expr(depth - 1, 0, true)?;
                if !operators.is_empty() {
                    left = Expr {
                        pos: start,
                        kind: Kind::Binary(Box::new(left), std::mem::take(&mut operators)),
                    };
                }
                left = Expr {
                    pos: start,
                    kind: Kind::Closure(Box::new(left), Box::new(body)),
                };
                break;
            }
            let Some((op, binding)) = self.operator() else {
                self.pos = end;
                break;
            };
            if binding < precedence {
                self.pos = end;
                break;
            }
            self.whitespace(true)?;
            let right = self.expr(depth - 1, binding + u8::from(binding != 1), true)?;
            operators.push((op, right));
        }
        Ok(if operators.is_empty() {
            left
        } else {
            Expr {
                pos: start,
                kind: Kind::Binary(Box::new(left), operators),
            }
        })
    }

    fn operator(&mut self) -> Option<(&'a str, u8)> {
        for (op, precedence) in [
            ("==", 4),
            ("!=", 4),
            ("<=", 4),
            (">=", 4),
            ("+=", 1),
            ("-=", 1),
            ("*=", 1),
            ("/=", 1),
            ("=", 1),
            ("<", 4),
            (">", 4),
            ("+", 5),
            ("-", 5),
            ("*", 6),
            ("/", 6),
        ] {
            if self.take(op) {
                return Some((op, precedence));
            }
        }
        let start = self.pos;
        for (word, binding) in [("or", 2), ("and", 3), ("in", 4), ("not", 4)] {
            if self.keyword(word) {
                if word == "not" {
                    self.scan(|c| c == ' ' || c == '\t');
                    if !self.keyword("in") {
                        self.pos = start;
                        return None;
                    }
                }
                return Some((&self.source[start..self.pos], binding));
            }
        }
        None
    }

    fn args(&mut self, depth: usize) -> Result<Vec<Arg<'a>>> {
        let mut args = Vec::new();
        self.whitespace(true)?;
        if self.take(":") {
            self.whitespace(true)?;
        }
        while !self.take(")") {
            let spread = self.take("..");
            let start = self.pos;
            let name = self.word();
            self.whitespace(false)?;
            let name = if !name.is_empty() && self.take(":") {
                Some(name)
            } else {
                self.pos = start;
                None
            };
            self.whitespace(true)?;
            let expr = self.expr(depth, 0, true)?;
            args.push(Arg { name, spread, expr });
            self.whitespace(true)?;
            if !self.take(",") {
                self.expect(")")?;
                break;
            }
            self.whitespace(true)?;
        }
        Ok(args)
    }

    fn branch(&mut self, depth: usize) -> Result<Expr<'a>> {
        let pos = self.pos;
        if self.take("[") {
            Ok(Expr {
                pos,
                kind: Kind::Content(self.markup(Some(']'), depth)?),
            })
        } else {
            self.block(depth)
        }
    }

    fn block(&mut self, depth: usize) -> Result<Expr<'a>> {
        let pos = self.pos;
        self.expect("{")?;
        let mut expressions = Vec::new();
        self.whitespace(true)?;
        while !self.take("}") {
            expressions.push(self.expr(depth, 0, true)?);
            self.whitespace(true)?;
            self.take(";");
            self.whitespace(true)?;
        }
        Ok(Expr {
            pos,
            kind: Kind::Block(expressions),
        })
    }

    fn atom(&mut self, depth: usize) -> Result<Kind<'a>> {
        let c = self
            .peek()
            .ok_or_else(|| self.fail("Expected a Typst expression."))?;
        let pos = self.pos;
        Ok(match c {
            '(' => {
                self.step();
                let args = self.args(depth)?;
                // A single value without a comma is a grouped expression.
                if let [arg] = args.as_slice()
                    && arg.name.is_none()
                    && !arg.spread
                    && !self.source[pos..self.pos - 1].trim_end().ends_with(',')
                {
                    let Some(Arg { expr, .. }) = args.into_iter().next() else {
                        return Err(self.fail("Expected an expression."));
                    };
                    expr.kind
                } else {
                    Kind::Args(args)
                }
            }
            '[' => {
                self.step();
                Kind::Content(self.markup(Some(']'), depth)?)
            }
            '{' => self.block(depth)?.kind,
            '"' => Kind::Literal(self.string()?),
            '$' => Kind::Math(self.math()?),
            '`' => {
                let (lang, source, block) = self.raw()?;
                Kind::Raw(lang, source, block)
            }
            '<' if self.label().is_some() => Kind::Literal(&self.source[pos..self.pos]),
            '+' | '-' => {
                self.step();
                Kind::Unary(
                    &self.source[pos..self.pos],
                    Box::new(self.expr(depth, 7, true)?),
                )
            }
            '0' if matches!(self.tail().as_bytes().get(1), Some(b'x' | b'o' | b'b')) => {
                let radix = match self.tail().as_bytes().get(1) {
                    Some(b'x') => 16,
                    Some(b'o') => 8,
                    _ => 2,
                };
                self.pos += 2;
                let digits = self.scan(|c| c.is_digit(radix));
                if digits.is_empty() {
                    return Err(self.fail("Invalid Typst integer."));
                }
                self.scan(|c| c.is_ascii_alphabetic() || c == '%');
                Kind::Literal(&self.source[pos..self.pos])
            }
            c if c.is_ascii_digit()
                || c == '.'
                    && self
                        .tail()
                        .as_bytes()
                        .get(1)
                        .is_some_and(u8::is_ascii_digit) =>
            {
                self.scan(|c| c.is_ascii_digit());
                if self
                    .tail()
                    .strip_prefix('.')
                    .is_some_and(|s| s.starts_with(|c: char| c.is_ascii_digit()))
                {
                    self.step();
                    self.scan(|c| c.is_ascii_digit());
                }
                self.scan(|c| c.is_ascii_alphabetic() || c == '%');
                Kind::Literal(&self.source[pos..self.pos])
            }
            _ => return self.keyword_expr(depth),
        })
    }

    fn keyword_expr(&mut self, depth: usize) -> Result<Kind<'a>> {
        let word = self.word();
        Ok(match word {
            "" => return Err(self.fail("Expected a Typst expression.")),
            "let" => return self.let_expr(depth),
            "if" => return self.if_expr(depth),
            "while" => {
                self.whitespace(true)?;
                let condition = self.expr(depth, 0, true)?;
                self.whitespace(true)?;
                Kind::While(Box::new(condition), Box::new(self.branch(depth)?))
            }
            "for" => return self.for_expr(depth),
            "context" => {
                self.whitespace(true)?;
                self.expr(depth, 0, true)?.kind
            }
            "not" => {
                self.whitespace(true)?;
                Kind::Unary(word, Box::new(self.expr(depth, 4, true)?))
            }
            "set" => return self.set_expr(depth),
            "show" => return self.show_expr(depth),
            "include" => {
                self.whitespace(true)?;
                Kind::Include(Box::new(self.expr(depth, 0, true)?))
            }
            "import" => return self.import_expr(depth),
            "break" => Kind::Break,
            "continue" => Kind::Continue,
            "return" => {
                self.whitespace(false)?;
                let expr = if matches!(self.peek(), None | Some(';' | '}' | '\n')) {
                    None
                } else {
                    Some(Box::new(self.expr(depth, 0, true)?))
                };
                Kind::Return(expr)
            }
            _ => Kind::Name(word),
        })
    }

    fn let_expr(&mut self, depth: usize) -> Result<Kind<'a>> {
        self.whitespace(true)?;
        let binding = self.expr(depth, 2, true)?;
        self.whitespace(false)?;
        let value = if self.take("=") {
            self.whitespace(true)?;
            Some(Box::new(self.expr(depth, 0, true)?))
        } else {
            None
        };
        Ok(Kind::Let(Box::new(binding), value))
    }

    fn if_expr(&mut self, depth: usize) -> Result<Kind<'a>> {
        self.whitespace(true)?;
        let condition = self.expr(depth, 0, true)?;
        self.whitespace(true)?;
        let yes = self.branch(depth)?;
        let end = self.pos;
        self.whitespace(true)?;
        let no = if self.keyword("else") {
            self.whitespace(true)?;
            Some(Box::new(if self.tail().starts_with("if ") {
                self.expr(depth, 0, true)?
            } else {
                self.branch(depth)?
            }))
        } else {
            self.pos = end;
            None
        };
        Ok(Kind::If(Box::new(condition), Box::new(yes), no))
    }

    fn for_expr(&mut self, depth: usize) -> Result<Kind<'a>> {
        self.whitespace(true)?;
        let pat = self.expr(depth, 5, true)?;
        self.whitespace(true)?;
        if !self.keyword("in") {
            return Err(self.fail("Expected in."));
        }
        self.whitespace(true)?;
        let items = self.expr(depth, 0, true)?;
        self.whitespace(true)?;
        Ok(Kind::For(
            Box::new(pat),
            Box::new(items),
            Box::new(self.branch(depth)?),
        ))
    }

    fn show_expr(&mut self, depth: usize) -> Result<Kind<'a>> {
        self.whitespace(true)?;
        let selector = if self.take(":") {
            None
        } else {
            let selector = self.expr(depth, 0, true)?;
            self.whitespace(true)?;
            self.expect(":")?;
            Some(Box::new(selector))
        };
        self.whitespace(true)?;
        Ok(Kind::Show(selector, Box::new(self.expr(depth, 0, true)?)))
    }

    fn set_expr(&mut self, depth: usize) -> Result<Kind<'a>> {
        self.whitespace(true)?;
        let rule = self.expr(depth, 0, true)?;
        let end = self.pos;
        self.whitespace(false)?;
        let condition = if self.keyword("if") {
            self.whitespace(true)?;
            Some(Box::new(self.expr(depth, 0, true)?))
        } else {
            self.pos = end;
            None
        };
        Ok(Kind::Set(Box::new(rule), condition))
    }

    fn import_expr(&mut self, depth: usize) -> Result<Kind<'a>> {
        self.whitespace(true)?;
        let source = self.expr(depth, 0, true)?;
        self.whitespace(false)?;
        let mut names = Vec::new();
        if self.take(":") {
            self.whitespace(true)?;
            if self.take("*") {
                names.push("*");
            } else {
                loop {
                    let name = self.word();
                    if name.is_empty() {
                        return Err(self.fail("Expected an import name."));
                    }
                    names.push(name);
                    self.whitespace(false)?;
                    if !self.take(",") {
                        break;
                    }
                    self.whitespace(true)?;
                }
            }
        }
        Ok(Kind::Import(Box::new(source), names))
    }
}

fn is_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-')
}
fn is_label(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | ':' | '.')
}
