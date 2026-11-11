//! Parse errors with the semantics and the text of Parsec 3.1, which
//! `doctemplates` uses. Pandoc prints these texts for a template error.

use std::fmt;
use std::rc::Rc;

/// A source position. Equality compares the name too, as Parsec's
/// `SourcePos` does. The error merge compares only the line and the column.
/// The numbers are 32 bits wide to keep the parser frames small; they stop
/// at the largest value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pos {
    pub(crate) name: Rc<str>,
    pub(crate) line: u32,
    pub(crate) column: u32,
}

impl Pos {
    pub(crate) fn initial(name: Rc<str>) -> Pos {
        Pos {
            name,
            line: 1,
            column: 1,
        }
    }

    /// Parsec `updatePosChar`.
    pub(crate) fn advance(&mut self, c: char) {
        match c {
            '\n' => {
                self.line = self.line.saturating_add(1);
                self.column = 1;
            }
            '\t' => self.column = self.column.saturating_add(8 - ((self.column - 1) % 8)),
            _ => self.column = self.column.saturating_add(1),
        }
    }
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.name.is_empty() {
            write!(f, "\"{}\" ", self.name)?;
        }
        write!(f, "(line {}, column {})", self.line, self.column)
    }
}

/// A Parsec error message. The kinds sort in the order of the variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Msg {
    SysUnExpect(String),
    UnExpect(String),
    Expect(String),
    Message(String),
}

impl Msg {
    fn rank(&self) -> u8 {
        match self {
            Msg::SysUnExpect(_) => 0,
            Msg::UnExpect(_) => 1,
            Msg::Expect(_) => 2,
            Msg::Message(_) => 3,
        }
    }

    fn text(&self) -> &str {
        match self {
            Msg::SysUnExpect(s) | Msg::UnExpect(s) | Msg::Expect(s) | Msg::Message(s) => s,
        }
    }
}

/// A Parsec `ParseError`. The messages are on the heap, and an error with
/// no messages (the most frequent kind) allocates nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PErr {
    pub(crate) pos: Pos,
    #[expect(
        clippy::box_collection,
        reason = "one pointer keeps each parser reply small on the stack"
    )]
    msgs: Option<Box<Vec<Msg>>>,
}

impl PErr {
    pub(crate) fn unknown(pos: Pos) -> PErr {
        PErr { pos, msgs: None }
    }

    pub(crate) fn new(pos: Pos, msg: Msg) -> PErr {
        PErr {
            pos,
            msgs: Some(Box::new(vec![msg])),
        }
    }

    pub(crate) fn msgs(&self) -> &[Msg] {
        self.msgs.as_deref().map_or(&[], Vec::as_slice)
    }

    #[cfg(test)]
    pub(crate) fn push(&mut self, msg: Msg) {
        self.msgs.get_or_insert_with(Box::default).push(msg);
    }

    pub(crate) fn is_unknown(&self) -> bool {
        self.msgs().is_empty()
    }

    /// Parsec `setErrorMessage (Expect label)`: all expect messages are
    /// replaced by the label.
    pub(crate) fn set_expect(mut self, label: &str) -> PErr {
        let msgs = self.msgs.get_or_insert_with(Box::default);
        msgs.retain(|m| m.rank() != 2);
        msgs.insert(0, Msg::Expect(label.to_owned()));
        self
    }

    /// Parsec 3.1.17 `mergeError`. Positions compare by line and column
    /// only (`compareErrorPos`); for equal positions the first error keeps
    /// its position and its name.
    pub(crate) fn merge(self, other: PErr) -> PErr {
        if other.is_unknown() && !self.is_unknown() {
            return self;
        }
        if self.is_unknown() && !other.is_unknown() {
            return other;
        }
        let first = (self.pos.line, self.pos.column);
        let second = (other.pos.line, other.pos.column);
        match first.cmp(&second) {
            std::cmp::Ordering::Equal => {
                let mut msgs = self.msgs;
                if let Some(more) = other.msgs {
                    msgs.get_or_insert_with(Box::default).extend(*more);
                }
                PErr {
                    pos: self.pos,
                    msgs,
                }
            }
            std::cmp::Ordering::Greater => self,
            std::cmp::Ordering::Less => other,
        }
    }

    /// The error merge of Parsec's bind, when the second parser consumed
    /// no input.
    pub(crate) fn merge_bind(self, other: PErr) -> PErr {
        if self.is_unknown() {
            other
        } else {
            self.merge(other)
        }
    }
}

impl fmt::Display for PErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:", self.pos)?;
        let mut msgs = self.msgs().to_vec();
        msgs.sort_by_key(Msg::rank);
        if msgs.is_empty() {
            return write!(f, "unknown parse error");
        }
        let of = |rank: u8| -> Vec<&str> {
            msgs.iter()
                .filter(|m| m.rank() == rank)
                .map(Msg::text)
                .collect()
        };
        let sys = of(0);
        let unexpect = of(1);
        let mut lines = Vec::new();
        match sys.first() {
            Some(first) if unexpect.is_empty() && first.is_empty() => {
                lines.push("unexpected end of input".to_owned());
            }
            Some(first) if unexpect.is_empty() => lines.push(format!("unexpected {first}")),
            _ => {}
        }
        lines.push(show_many("unexpected", &unexpect));
        lines.push(show_many("expecting", &of(2)));
        lines.push(show_many("", &of(3)));
        for line in clean(lines.iter().map(String::as_str)) {
            write!(f, "\n{line}")?;
        }
        Ok(())
    }
}

fn clean<'a>(items: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::new();
    for item in items {
        if !item.is_empty() && !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

fn show_many(prefix: &str, msgs: &[&str]) -> String {
    let items = clean(msgs.iter().copied());
    let text = match items.as_slice() {
        [] => return String::new(),
        [one] => (*one).to_owned(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    };
    if prefix.is_empty() {
        text
    } else {
        format!("{prefix} {text}")
    }
}

#[cfg(test)]
mod tests {
    use randoc_ast::json::{show_char, show_string};

    use super::*;

    #[test]
    fn haskell_show() {
        assert_eq!(show_string("a\"b\\"), "\"a\\\"b\\\\\"");
        assert_eq!(show_string("\u{e9}1"), "\"\\233\\&1\"");
        assert_eq!(
            show_string("\u{e}H\n\t\u{7f}\u{1}"),
            "\"\\SO\\&H\\n\\t\\DEL\\SOH\""
        );
        assert_eq!(show_char('\''), "'\\''");
        assert_eq!(show_char('"'), "'\"'");
        assert_eq!(show_char('\u{e9}'), "'\\233'");
    }

    #[test]
    fn tab_column() {
        let mut pos = Pos::initial(Rc::from(""));
        pos.advance('a');
        pos.advance('\t');
        assert_eq!(pos.column, 9);
        pos.advance('\t');
        assert_eq!(pos.column, 17);
    }

    fn at(column: u32) -> Pos {
        Pos {
            name: Rc::from("t"),
            line: 1,
            column,
        }
    }

    #[test]
    fn merge_prefers_meaningful_errors() {
        let meaningful = PErr::new(at(2), Msg::Expect("x".to_owned()));
        let later_unknown = PErr::unknown(at(5));
        assert_eq!(meaningful.clone().merge(later_unknown.clone()), meaningful);
        assert_eq!(later_unknown.merge(meaningful.clone()), meaningful);
        let earlier = PErr::new(at(1), Msg::Expect("y".to_owned()));
        assert_eq!(earlier.merge(meaningful.clone()), meaningful);
    }

    #[test]
    fn unexpect_hides_system_unexpect() {
        let mut err = PErr::new(at(3), Msg::SysUnExpect("\"a\"".to_owned()));
        err.push(Msg::UnExpect("'b'".to_owned()));
        assert_eq!(err.to_string(), "\"t\" (line 1, column 3):\nunexpected 'b'");
        let sys = PErr::new(at(3), Msg::SysUnExpect("\"a\"".to_owned()));
        assert_eq!(
            sys.to_string(),
            "\"t\" (line 1, column 3):\nunexpected \"a\""
        );
    }

    #[test]
    fn unknown_error_text() {
        let err = PErr::unknown(Pos::initial(Rc::from("x")));
        assert_eq!(
            err.to_string(),
            "\"x\" (line 1, column 1):unknown parse error"
        );
    }
}
