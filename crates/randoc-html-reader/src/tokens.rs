//! The tag tokenizer: a port of `tagsoup` 0.14.8 (`Text.HTML.TagSoup.Specification`
//! and `Text.HTML.TagSoup.Implementation`), the library that Pandoc reads HTML with.
//!
//! The tokenizer is lenient. It never fails: broken markup becomes text, a
//! comment or a tag with odd attributes, exactly as `tagsoup` gives them,
//! because the tree that Pandoc builds depends on the exact token list.
//!
//! The port joins the two upstream steps (the character state machine and the
//! grouping of its output) into one pass, so that memory use is linear in the
//! number of tokens and not in the number of characters.

use randoc_ast::Text;
use randoc_core::shared::{named_entity, show_string};

/// One token of the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TokenKind {
    /// An open tag with its attributes in source order.
    Open(Text, Vec<(Text, Text)>),
    /// A close tag.
    Close(Text),
    /// Text. It is never empty.
    Text(Text),
    /// A comment.
    Comment(Text),
}

/// A token with the position of its first character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    /// The 1-based line.
    pub(crate) line: u32,
    /// The 1-based column. A tab moves to the next multiple of 8, plus 1.
    pub(crate) column: u32,
}

/// The kind of tag that the state machine reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TagType {
    /// `<name`
    Normal,
    /// `<?name`
    Xml,
    /// `<!name`
    Decl,
    /// `<script`
    Script,
}

/// A state inside a tag.
#[derive(Clone, Copy)]
enum TagState {
    Name,
    XmlClose,
    BeforeAttName,
    AttName,
    AfterAttName,
    BeforeAttValue,
    AttValueQuoted(char),
    AttValueUnquoted,
    AfterAttValueQuoted,
    SelfClosing,
}

/// A piece of the attribute list of one tag, in the order of the state machine.
enum Piece {
    Name(String),
    Value(String),
}

/// How a tag ends.
enum TagEnd {
    /// `>`
    Plain,
    /// `/>`
    SelfClose,
    /// The input ended inside the tag.
    Eof,
}

type Position = (u32, u32);

struct Lexer<'a> {
    chars: &'a [char],
    index: usize,
    line: u32,
    column: u32,
    tokens: Vec<Token>,
    text: String,
    text_start: Position,
}

/// Splits `input` into tokens.
pub(crate) fn tokenize(input: &str) -> Vec<Token> {
    let chars: Vec<char> = input.chars().collect();
    let mut lexer = Lexer {
        chars: &chars,
        index: 0,
        line: 1,
        column: 1,
        tokens: Vec::new(),
        text: String::new(),
        text_start: (1, 1),
    };
    lexer.data();
    lexer.tokens
}

fn is_white(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r')
}

impl Lexer<'_> {
    fn eof(&self) -> bool {
        self.index >= self.chars.len()
    }

    /// The current character, or NUL at the end of the input as upstream does.
    fn head(&self) -> char {
        self.chars.get(self.index).copied().unwrap_or('\0')
    }

    fn position(&self) -> Position {
        (self.line, self.column)
    }

    fn advance(&mut self) {
        let Some(&c) = self.chars.get(self.index) else {
            return;
        };
        self.index += 1;
        match c {
            '\n' => {
                self.line = self.line.saturating_add(1);
                self.column = 1;
            }
            '\t' => {
                let rest = self.column.saturating_sub(1) % 8;
                self.column = self.column.saturating_add(8 - rest);
            }
            _ => self.column = self.column.saturating_add(1),
        }
    }

    /// Returns true and moves over `word` when the input continues with it.
    fn eat(&mut self, word: &str) -> bool {
        let mut offset = 0;
        for expected in word.chars() {
            if self.chars.get(self.index + offset) != Some(&expected) {
                return false;
            }
            offset += 1;
        }
        for _ in 0..offset {
            self.advance();
        }
        true
    }

    fn push_text(&mut self, c: char, start: Position) {
        if self.text.is_empty() {
            self.text_start = start;
        }
        self.text.push(c);
    }

    fn push_text_str(&mut self, text: &str, start: Position) {
        if text.is_empty() {
            return;
        }
        if self.text.is_empty() {
            self.text_start = start;
        }
        self.text.push_str(text);
    }

    fn flush_text(&mut self) {
        if self.text.is_empty() {
            return;
        }
        let (line, column) = self.text_start;
        self.tokens.push(Token {
            kind: TokenKind::Text(std::mem::take(&mut self.text)),
            line,
            column,
        });
    }

    fn push_token(&mut self, kind: TokenKind, (line, column): Position) {
        self.flush_text();
        self.tokens.push(Token { kind, line, column });
    }

    /// The data state: text, character references and the start of markup.
    fn data(&mut self) {
        while !self.eof() {
            let mark = self.position();
            match self.head() {
                '&' => {
                    self.advance();
                    let text = self.char_ref(None);
                    self.push_text_str(&text, mark);
                }
                '<' => {
                    self.advance();
                    self.tag_open(mark);
                }
                c => {
                    self.push_text(c, mark);
                    self.advance();
                }
            }
        }
        self.flush_text();
    }

    /// The input after `<`.
    fn tag_open(&mut self, mark: Position) {
        let head = self.head();
        match head {
            '!' => {
                self.advance();
                self.markup_declaration(mark);
            }
            '/' => {
                self.advance();
                self.close_tag_open(mark);
            }
            _ if head.is_alphabetic() => {
                let tag_type = if self.at_script() {
                    TagType::Script
                } else {
                    TagType::Normal
                };
                self.advance();
                self.tag(mark, String::from(head), tag_type, false);
            }
            '>' => {
                self.advance();
                self.push_text_str("<>", mark);
            }
            '?' => {
                self.advance();
                let next = self.head();
                if next.is_alphabetic() {
                    self.advance();
                    let mut name = String::from('?');
                    name.push(next);
                    self.tag(mark, name, TagType::Xml, false);
                } else {
                    self.push_text_str("<?", mark);
                }
            }
            _ => self.push_text('<', mark),
        }
    }

    /// Returns true when the input continues with `script` and the end of a tag name.
    fn at_script(&self) -> bool {
        let word = "script";
        let matches = word.chars().enumerate().all(|(offset, expected)| {
            self.chars
                .get(self.index + offset)
                .is_some_and(|c| c.to_lowercase().eq(expected.to_lowercase()))
        });
        if !matches {
            return false;
        }
        match self.chars.get(self.index + word.len()) {
            None => true,
            Some(&c) => is_white(c) || matches!(c, '/' | '>' | '?'),
        }
    }

    /// The input after `</`.
    fn close_tag_open(&mut self, mark: Position) {
        let head = self.head();
        if head.is_alphabetic() || head == '?' || head == '!' {
            self.advance();
            self.tag(mark, String::from(head), TagType::Normal, true);
        } else if head == '>' {
            self.advance();
            self.push_text_str("</>", mark);
        } else if self.eof() {
            self.push_text_str("</", mark);
        } else {
            self.bogus_comment(mark);
        }
    }

    /// The input after `<!`.
    fn markup_declaration(&mut self, mark: Position) {
        if self.eat("--") {
            self.comment(mark);
            return;
        }
        let head = self.head();
        if head.is_alphabetic() {
            self.advance();
            let mut name = String::from('!');
            name.push(head);
            self.tag(mark, name, TagType::Decl, false);
        } else if self.eat("[CDATA[") {
            self.cdata();
        } else {
            self.bogus_comment(mark);
        }
    }

    fn bogus_comment(&mut self, mark: Position) {
        let mut text = String::new();
        while !self.eof() {
            let c = self.head();
            self.advance();
            if c == '>' {
                break;
            }
            text.push(c);
        }
        self.push_token(TokenKind::Comment(text), mark);
    }

    fn cdata(&mut self) {
        while !self.eof() {
            if self.eat("]]>") {
                return;
            }
            let mark = self.position();
            let c = self.head();
            self.push_text(c, mark);
            self.advance();
        }
    }

    /// The comment states of upstream, after `<!--`.
    fn comment(&mut self, mark: Position) {
        #[derive(Clone, Copy)]
        enum State {
            Start,
            StartDash,
            Comment,
            EndDash,
            End,
            EndBang,
            EndSpace,
        }
        let mut text = String::new();
        let mut state = State::Start;
        loop {
            if self.eof() {
                break;
            }
            let c = self.head();
            self.advance();
            state = match (state, c) {
                (State::Start, '-') => State::StartDash,
                (State::Start | State::StartDash, '>') => break,
                (State::Start, _) => {
                    text.push(c);
                    State::Comment
                }
                (State::StartDash | State::EndDash, '-') => State::End,
                (State::StartDash | State::EndDash, _) => {
                    text.push('-');
                    text.push(c);
                    State::Comment
                }
                (State::Comment, '-') => State::EndDash,
                (State::Comment, _) => {
                    text.push(c);
                    State::Comment
                }
                (State::End | State::EndBang | State::EndSpace, '>') => break,
                (State::End, '-') => {
                    text.push('-');
                    State::End
                }
                (State::End, '!') => State::EndBang,
                (State::End, _) => {
                    text.push_str("--");
                    text.push(c);
                    if is_white(c) {
                        State::EndSpace
                    } else {
                        State::Comment
                    }
                }
                (State::EndBang, '-') => {
                    text.push_str("--!");
                    State::EndDash
                }
                (State::EndBang, _) => {
                    text.push_str("--!");
                    text.push(c);
                    State::Comment
                }
                (State::EndSpace, '-') => State::EndDash,
                (State::EndSpace, _) => {
                    text.push(c);
                    if is_white(c) {
                        State::EndSpace
                    } else {
                        State::Comment
                    }
                }
            };
        }
        self.push_token(TokenKind::Comment(text), mark);
    }

    /// Reads the rest of a tag after the first character of its name.
    fn tag(&mut self, mark: Position, mut name: String, tag_type: TagType, is_close: bool) {
        let mut pieces: Vec<Piece> = Vec::new();
        let mut state = TagState::Name;
        let xml = tag_type == TagType::Xml;
        let normal = tag_type == TagType::Normal;
        let end = loop {
            if self.eof() {
                break TagEnd::Eof;
            }
            let c = self.head();
            match state {
                TagState::Name => {
                    self.advance();
                    match c {
                        _ if is_white(c) => state = TagState::BeforeAttName,
                        '/' => state = TagState::SelfClosing,
                        '>' => break TagEnd::Plain,
                        '?' if xml => state = TagState::XmlClose,
                        _ => name.push(c),
                    }
                }
                TagState::XmlClose => {
                    if c == '>' {
                        self.advance();
                        break TagEnd::Plain;
                    }
                    state = TagState::BeforeAttName;
                }
                TagState::BeforeAttName => match c {
                    _ if is_white(c) => self.advance(),
                    '/' => {
                        self.advance();
                        state = TagState::SelfClosing;
                    }
                    '>' => {
                        self.advance();
                        break TagEnd::Plain;
                    }
                    '?' if xml => {
                        self.advance();
                        state = TagState::XmlClose;
                    }
                    '\'' | '"' if !normal => state = TagState::BeforeAttValue,
                    _ => {
                        self.advance();
                        pieces.push(Piece::Name(String::from(c)));
                        state = TagState::AttName;
                    }
                },
                TagState::AttName => {
                    self.advance();
                    match c {
                        _ if is_white(c) => state = TagState::AfterAttName,
                        '/' => state = TagState::SelfClosing,
                        '=' => state = TagState::BeforeAttValue,
                        '>' => break TagEnd::Plain,
                        '?' if xml => state = TagState::XmlClose,
                        _ => {
                            if let Some(Piece::Name(text)) = pieces.last_mut() {
                                text.push(c);
                            }
                        }
                    }
                }
                TagState::AfterAttName => match c {
                    _ if is_white(c) => self.advance(),
                    '/' => {
                        self.advance();
                        state = TagState::SelfClosing;
                    }
                    '=' => {
                        self.advance();
                        state = TagState::BeforeAttValue;
                    }
                    '>' => {
                        self.advance();
                        break TagEnd::Plain;
                    }
                    '?' if xml => {
                        self.advance();
                        state = TagState::XmlClose;
                    }
                    '\'' | '"' if !normal => {
                        pieces.push(Piece::Value(String::new()));
                        state = TagState::BeforeAttValue;
                    }
                    _ => {
                        self.advance();
                        pieces.push(Piece::Name(String::from(c)));
                        state = TagState::AttName;
                    }
                },
                TagState::BeforeAttValue => match c {
                    _ if is_white(c) => self.advance(),
                    '"' | '\'' => {
                        self.advance();
                        pieces.push(Piece::Value(String::new()));
                        state = TagState::AttValueQuoted(c);
                    }
                    '&' => {
                        pieces.push(Piece::Value(String::new()));
                        state = TagState::AttValueUnquoted;
                    }
                    '>' => {
                        self.advance();
                        break TagEnd::Plain;
                    }
                    '?' if xml => {
                        self.advance();
                        state = TagState::XmlClose;
                    }
                    _ => {
                        self.advance();
                        pieces.push(Piece::Value(String::from(c)));
                        state = TagState::AttValueUnquoted;
                    }
                },
                TagState::AttValueQuoted(quote) => {
                    self.advance();
                    if c == quote {
                        state = TagState::AfterAttValueQuoted;
                    } else {
                        let text = if c == '&' {
                            self.char_ref(Some(quote))
                        } else {
                            String::from(c)
                        };
                        if let Some(Piece::Value(value)) = pieces.last_mut() {
                            value.push_str(&text);
                        }
                    }
                }
                TagState::AttValueUnquoted => {
                    self.advance();
                    match c {
                        _ if is_white(c) => state = TagState::BeforeAttName,
                        '>' => break TagEnd::Plain,
                        '?' if xml => state = TagState::XmlClose,
                        _ => {
                            let text = if c == '&' {
                                self.char_ref(None)
                            } else {
                                String::from(c)
                            };
                            if let Some(Piece::Value(value)) = pieces.last_mut() {
                                value.push_str(&text);
                            }
                        }
                    }
                }
                TagState::AfterAttValueQuoted => match c {
                    _ if is_white(c) => {
                        self.advance();
                        state = TagState::BeforeAttName;
                    }
                    '/' => {
                        self.advance();
                        state = TagState::SelfClosing;
                    }
                    '>' => {
                        self.advance();
                        break TagEnd::Plain;
                    }
                    '?' if xml => {
                        self.advance();
                        state = TagState::XmlClose;
                    }
                    _ => state = TagState::BeforeAttName,
                },
                TagState::SelfClosing => {
                    if c == '>' && !xml {
                        self.advance();
                        break TagEnd::SelfClose;
                    }
                    state = TagState::BeforeAttName;
                }
            }
        };
        if is_close {
            self.push_token(TokenKind::Close(name), mark);
            return;
        }
        let attributes = group_attributes(pieces);
        match end {
            TagEnd::SelfClose => {
                self.push_token(TokenKind::Open(name.clone(), attributes), mark);
                self.push_token(TokenKind::Close(name), mark);
            }
            TagEnd::Plain => {
                self.push_token(TokenKind::Open(name, attributes), mark);
                if tag_type == TagType::Script {
                    self.script_body();
                }
            }
            TagEnd::Eof => self.push_token(TokenKind::Open(name, attributes), mark),
        }
    }

    /// The content of a `script` element: text up to `</script`.
    fn script_body(&mut self) {
        while !self.eof() {
            if self.head() == '<' && self.chars.get(self.index + 1) == Some(&'/') {
                let start = self.index;
                self.index += 2;
                let closes = self.at_script();
                self.index = start;
                if closes {
                    return;
                }
            }
            let mark = self.position();
            let c = self.head();
            self.push_text(c, mark);
            self.advance();
        }
    }

    /// Reads a character reference after `&` and returns its text.
    ///
    /// `quote` is the quote character of an attribute value, which ends a reference.
    fn char_ref(&mut self, quote: Option<char>) -> String {
        let head = self.head();
        if self.eof()
            || matches!(head, '\t' | '\n' | '\x0c' | ' ' | '<' | '&')
            || quote == Some(head)
        {
            return String::from('&');
        }
        if head == '#' {
            let restart = (self.index, self.line, self.column);
            self.advance();
            let hex = matches!(self.head(), 'x' | 'X');
            if hex {
                self.advance();
            }
            let radix = if hex { 16 } else { 10 };
            let mut value: u32 = 0;
            let mut digits = 0usize;
            while let Some(digit) = self.head().to_digit(radix).filter(|_| !self.eof()) {
                value = value.saturating_mul(radix).saturating_add(digit);
                digits += 1;
                self.advance();
            }
            if digits == 0 {
                (self.index, self.line, self.column) = restart;
                return String::from('&');
            }
            if self.head() == ';' && !self.eof() {
                self.advance();
            }
            // A Haskell `Text` cannot hold a surrogate: it stores U+FFFD.
            let c = match value {
                0xD800..=0xDFFF => char::REPLACEMENT_CHARACTER,
                _ => char::from_u32(value).unwrap_or('?'),
            };
            return String::from(c);
        }
        if !head.is_alphabetic() {
            return String::from('&');
        }
        let mut name = String::new();
        while !self.eof() {
            let c = self.head();
            if !(c.is_alphanumeric() || matches!(c, ':' | '-' | '_')) {
                break;
            }
            name.push(c);
            self.advance();
        }
        if self.head() == ';' && !self.eof() {
            self.advance();
            name.push(';');
        }
        match named_entity(&name) {
            Some(text) => text.to_owned(),
            None => format!("&{name}"),
        }
    }
}

/// Makes the attribute list from the pieces, as the upstream function `atts` does.
fn group_attributes(pieces: Vec<Piece>) -> Vec<(Text, Text)> {
    let mut attributes: Vec<(Text, Text)> = Vec::new();
    let mut open_name = false;
    for piece in pieces {
        match piece {
            Piece::Name(name) => {
                attributes.push((name, Text::new()));
                open_name = true;
            }
            Piece::Value(value) => {
                match attributes.last_mut() {
                    Some(last) if open_name => last.1 = value,
                    _ => attributes.push((Text::new(), value)),
                }
                open_name = false;
            }
        }
    }
    attributes
}

/// Renders tokens as HTML text, as the `renderTags` function of upstream does
/// with the options that Pandoc uses.
pub(crate) fn render_open(name: &str, attributes: &[(Text, Text)], out: &mut String) {
    out.push('<');
    out.push_str(name);
    for (key, value) in attributes {
        match (key.is_empty(), value.is_empty()) {
            (true, true) => out.push_str(" \"\""),
            (false, true) => {
                out.push(' ');
                out.push_str(key);
            }
            (true, false) => {
                out.push_str(" \"");
                escape_into(value, out);
                out.push('"');
            }
            (false, false) => {
                out.push(' ');
                out.push_str(key);
                out.push_str("=\"");
                escape_into(value, out);
                out.push('"');
            }
        }
    }
    if name.starts_with('?') {
        out.push_str(" ?");
    }
    out.push('>');
}

/// Renders a close tag.
pub(crate) fn render_close(name: &str, out: &mut String) {
    out.push_str("</");
    out.push_str(name);
    out.push('>');
}

/// Renders a comment. A `-->` inside the text becomes `-- >`.
pub(crate) fn render_comment(text: &str, out: &mut String) {
    out.push_str("<!--");
    out.push_str(&text.replace("-->", "-- >"));
    out.push_str("-->");
}

/// Escapes text as the `escapeXML` function of upstream does.
pub(crate) fn escape_into(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
}

/// Shows a token as the Haskell function `show` does for a `tagsoup` tag.
/// Pandoc gives this text as its error message when it cannot read a token.
pub(crate) fn show_token(kind: &TokenKind) -> String {
    let mut out = String::new();
    match kind {
        TokenKind::Open(name, attributes) => {
            out.push_str("TagOpen ");
            out.push_str(&show_string(name));
            out.push_str(" [");
            for (index, (key, value)) in attributes.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push('(');
                out.push_str(&show_string(key));
                out.push(',');
                out.push_str(&show_string(value));
                out.push(')');
            }
            out.push(']');
        }
        TokenKind::Close(name) => {
            out.push_str("TagClose ");
            out.push_str(&show_string(name));
        }
        TokenKind::Text(text) => {
            out.push_str("TagText ");
            out.push_str(&show_string(text));
        }
        TokenKind::Comment(text) => {
            out.push_str("TagComment ");
            out.push_str(&show_string(text));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pandoc 3.13 prints this text for `<dfn><button rowspan="2" data-k="v"></button></dfn>`.
    #[test]
    fn shows_a_token_as_haskell_does() {
        let token = open("button", &[("rowspan", "2"), ("data-k", "v")]);
        assert_eq!(
            show_token(&token),
            r#"TagOpen "button" [("rowspan","2"),("data-k","v")]"#
        );
        assert_eq!(show_token(&close("div")), r#"TagClose "div""#);
        assert_eq!(
            show_token(&text("a\"b\n\u{e9}1")),
            r#"TagText "a\"b\n\233\&1""#
        );
    }

    fn kinds(input: &str) -> Vec<TokenKind> {
        tokenize(input)
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    fn open(name: &str, attributes: &[(&str, &str)]) -> TokenKind {
        TokenKind::Open(
            name.to_owned(),
            attributes
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
        )
    }

    fn text(value: &str) -> TokenKind {
        TokenKind::Text(value.to_owned())
    }

    fn close(name: &str) -> TokenKind {
        TokenKind::Close(name.to_owned())
    }

    #[test]
    fn reads_tags_text_and_positions() {
        let tokens = tokenize("<p>a\n\t<b x=1>c</b>");
        let positions: Vec<(u32, u32)> = tokens.iter().map(|t| (t.line, t.column)).collect();
        assert_eq!(positions, vec![(1, 1), (1, 4), (2, 9), (2, 16), (2, 17)]);
        assert_eq!(
            kinds("<p>a\n\t<b x=1>c</b>"),
            vec![
                open("p", &[]),
                text("a\n\t"),
                open("b", &[("x", "1")]),
                text("c"),
                close("b"),
            ]
        );
    }

    // The expected values come from Pandoc 3.13 with `-f html+raw_html`.
    #[test]
    fn reads_attribute_forms() {
        assert_eq!(
            kinds("<foo a=\"1\" b='2' c=3 d e=\"\" \"q\" x=y/>"),
            vec![open(
                "foo",
                &[
                    ("a", "1"),
                    ("b", "2"),
                    ("c", "3"),
                    ("d", ""),
                    ("e", ""),
                    ("\"q\"", ""),
                    ("x", "y/")
                ]
            )]
        );
        assert_eq!(kinds("<b =x>"), vec![open("b", &[("=x", "")])]);
        assert_eq!(kinds("<b x= >"), vec![open("b", &[("x", "")])]);
        assert_eq!(
            kinds("<b x=a\"b y=c'd>"),
            vec![open("b", &[("x", "a\"b"), ("y", "c'd")])]
        );
        assert_eq!(kinds("<bar/>"), vec![open("bar", &[]), close("bar")]);
        assert_eq!(
            kinds("</b c>x</b/>"),
            vec![close("b"), text("x"), close("b")]
        );
        assert_eq!(kinds("<a</p>"), vec![open("a<", &[("p", "")])]);
        assert_eq!(kinds("<a href=\"x"), vec![open("a", &[("href", "x")])]);
    }

    #[test]
    fn reads_declarations_and_processing_instructions() {
        assert_eq!(
            kinds("<!DOCTYPE html PUBLIC \"a b\">"),
            vec![open(
                "!DOCTYPE",
                &[("html", ""), ("PUBLIC", ""), ("", "a b")]
            )]
        );
        assert_eq!(
            kinds("<?xml version=\"1.0\"?>x"),
            vec![open("?xml", &[("version", "1.0")]), text("x")]
        );
        assert_eq!(kinds("a<![CDATA[c<b>]]>d"), vec![text("ac<b>d")]);
        assert_eq!(kinds("<?> <? k ?>"), vec![text("<?> <? k ?>")]);
    }

    #[test]
    fn reads_broken_markup_as_text() {
        assert_eq!(
            kinds("a < b <> c </> d <1"),
            vec![text("a < b <> c </> d <1")]
        );
        assert_eq!(kinds("x</"), vec![text("x</")]);
        assert_eq!(
            kinds("</ f><!><!x>"),
            vec![
                TokenKind::Comment(" f".to_owned()),
                TokenKind::Comment(String::new()),
                open("!x", &[]),
            ]
        );
    }

    #[test]
    fn reads_comments() {
        let comment = |text: &str| TokenKind::Comment(text.to_owned());
        assert_eq!(kinds("<!----->"), vec![comment("-")]);
        assert_eq!(kinds("<!--->"), vec![comment("")]);
        assert_eq!(kinds("<!-->"), vec![comment("")]);
        assert_eq!(kinds("<!-- x -- y -->"), vec![comment(" x -- y ")]);
        assert_eq!(kinds("<!-- x --!>"), vec![comment(" x ")]);
        assert_eq!(kinds("<!--x-- >"), vec![comment("x-- ")]);
        assert_eq!(kinds("<!-- open</p>"), vec![comment(" open</p>")]);
    }

    #[test]
    fn reads_character_references() {
        assert_eq!(
            kinds("&amp; &copy &nosuch; &#65; &#x41 x"),
            vec![text("& \u{a9} &nosuch; A A x")]
        );
        assert_eq!(kinds("&#0;&#x110000;&#xD800;"), vec![text("\0?\u{fffd}")]);
        assert_eq!(
            kinds("&#x; &# &#xZ; &#x41g"),
            vec![text("&#x; &# &#xZ; Ag")]
        );
        assert_eq!(
            kinds("&ampx; &amp= &copy1 &lt&gt & &; &&amp;"),
            vec![text("&ampx; &= &copy1 <> & &; &&")]
        );
        assert_eq!(
            kinds("<a href=\"a?b=1&copy=2&reg;&c=3\" t='&quot;&#65;'>"),
            vec![open(
                "a",
                &[("href", "a?b=1\u{a9}=2\u{ae}&c=3"), ("t", "\"A")]
            )]
        );
    }

    #[test]
    fn reads_a_script_body_as_one_text() {
        assert_eq!(
            kinds("<script type=\"x\">if (a<b) { \"</p>\" }</SCRIPT>z"),
            vec![
                open("script", &[("type", "x")]),
                text("if (a<b) { \"</p>\" }"),
                close("SCRIPT"),
                text("z"),
            ]
        );
        assert_eq!(
            kinds("<script/>a<b>"),
            vec![
                open("script", &[]),
                close("script"),
                text("a"),
                open("b", &[])
            ]
        );
    }

    #[test]
    fn renders_tags() {
        let mut out = String::new();
        render_open(
            "b",
            &[
                ("x".into(), "a\"b".into()),
                ("y".into(), "c'd".into()),
                ("e".into(), String::new()),
            ],
            &mut out,
        );
        render_close("b", &mut out);
        render_comment("x-->y", &mut out);
        assert_eq!(out, "<b x=\"a&quot;b\" y=\"c&#39;d\" e></b><!--x-- >y-->");
    }
}
