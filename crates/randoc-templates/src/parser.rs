//! The template parser (`Text.DocTemplates.Parser`). It follows the Parsec
//! rules for consumed input, backtracking and error merging, so that an
//! error has the same text as in Pandoc.

use std::collections::HashMap;
use std::rc::Rc;

use randoc_layout::{literal, space};

use crate::parsec::{Msg, PErr, Pos, show_char, show_string};
use crate::render::{Align, Node, Pipe, Variable};

/// A Parsec reply: `Ok((value, consumed, error))` or
/// `Err((consumed, error))`.
type R<T> = Result<(T, bool, PErr), (bool, PErr)>;

/// The largest block width of a pipe. A larger width gives this width.
pub(crate) const MAX_BLOCK_WIDTH: usize = 100_000;

/// The deepest partial nesting before `doctemplates` inserts `(loop)`.
const MAX_PARTIAL_NESTING: usize = 50;

const RESERVED: [&str; 8] = [
    "if", "else", "endif", "elseif", "for", "endfor", "sep", "it",
];

/// An error that stops the parse at once.
pub(crate) enum Fatal<E> {
    Load(E),
    TooDeep,
}

#[derive(Clone)]
struct UserState {
    template_path: Rc<str>,
    partial_nesting: usize,
    breaking_spaces: bool,
    first_nonspace: Pos,
    nested_col: Option<u32>,
    inside_directive: bool,
}

struct Snapshot {
    input: Rc<[char]>,
    idx: usize,
    pos: Pos,
    st: UserState,
}

#[derive(Clone, Copy)]
enum Closer {
    Dollar,
    Brace,
}

/// Accumulates the consumed flag and the error of a sequence, as Parsec's
/// bind does.
struct Acc {
    consumed: bool,
    err: PErr,
}

impl Acc {
    fn new(pos: &Pos) -> Acc {
        Acc {
            consumed: false,
            err: PErr::unknown(pos.clone()),
        }
    }

    fn absorb(&mut self, consumed: bool, err: PErr) {
        let old = std::mem::replace(&mut self.err, PErr::unknown(err.pos.clone()));
        self.err = if consumed { err } else { old.merge_bind(err) };
        self.consumed |= consumed;
    }

    fn step<T>(&mut self, reply: R<T>) -> Result<T, (bool, PErr)> {
        match reply {
            Ok((value, consumed, err)) => {
                self.absorb(consumed, err);
                Ok(value)
            }
            Err((consumed, err)) => {
                self.absorb(consumed, err);
                Err((self.consumed, self.err.clone()))
            }
        }
    }

    fn ok<T>(self, value: T) -> R<T> {
        Ok((value, self.consumed, self.err))
    }
}

/// Haskell `isSpace`.
fn is_space(c: char) -> bool {
    matches!(
        c,
        '\t'..='\r'
            | ' '
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

fn is_spacy(c: char) -> bool {
    matches!(c, ' ' | '\n' | '\r' | '\t')
}

/// `toBreakable`: each run of spaces becomes a breakable space.
fn to_breakable(cs: &[char]) -> Node {
    let mut nodes = Vec::new();
    let mut rest = cs;
    while let Some(&first) = rest.first() {
        let spacy = is_spacy(first);
        let end = rest
            .iter()
            .position(|&c| is_spacy(c) != spacy)
            .unwrap_or(rest.len());
        let (run, tail) = rest.split_at(end);
        nodes.push(Node::Literal(if spacy {
            space()
        } else {
            literal(run.iter().collect::<String>())
        }));
        rest = tail;
    }
    Node::concat(nodes)
}

fn take_extension(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.rfind('.').map_or("", |i| &file[i..])
}

fn split_file_name(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(i) => (&path[..=i], &path[i + 1..]),
        None => ("", path),
    }
}

fn combine_always(dir: &str, file: &str) -> String {
    if dir.is_empty() {
        file.to_owned()
    } else if file.is_empty() || dir.ends_with('/') {
        format!("{dir}{file}")
    } else {
        format!("{dir}/{file}")
    }
}

/// The partial path of `doctemplates`: the partial name in the directory
/// of the template, with the extension of the template when the name has
/// none.
pub(crate) fn partial_path(template_path: &str, name: &str) -> String {
    let (dir, file) = split_file_name(template_path);
    if take_extension(name).is_empty() {
        combine_always(dir, &format!("{name}{}", take_extension(file)))
    } else if name.starts_with('/') {
        name.to_owned()
    } else {
        combine_always(dir, name)
    }
}

fn remove_final_newline(text: String) -> String {
    match text.strip_suffix('\n') {
        Some(t) => t.to_owned(),
        None => text,
    }
}

fn merge_option(first: Option<PErr>, second: PErr) -> PErr {
    match first {
        Some(e) => e.merge(second),
        None => second,
    }
}

pub(crate) struct Parser<'a, E> {
    input: Rc<[char]>,
    idx: usize,
    pos: Pos,
    st: UserState,
    load: &'a mut dyn FnMut(&str) -> Result<String, E>,
    fatal: Option<Fatal<E>>,
    depth: usize,
    max_depth: usize,
    /// Each partial that was parsed, by the state that its parse depends
    /// on, so that a partial used many times is read and parsed one time.
    partials: HashMap<PartialKey, (Node, bool, PErr)>,
}

/// The path of a partial and the parser state that its parse depends on.
#[derive(PartialEq, Eq, Hash)]
struct PartialKey {
    path: String,
    nesting: usize,
    depth: usize,
    breaking_spaces: bool,
    inside_directive: bool,
}

/// Compiles a template. `Err(Ok(text))` is a syntax error with the Parsec
/// text, `Err(Err(fatal))` is a loader error or a nesting error.
pub(crate) fn compile<E>(
    path: &str,
    source: &str,
    max_depth: usize,
    load: &mut dyn FnMut(&str) -> Result<String, E>,
) -> Result<Node, Result<String, Fatal<E>>> {
    let name: Rc<str> = Rc::from(path);
    let pos = Pos::initial(name.clone());
    let mut parser = Parser {
        input: source.chars().collect(),
        idx: 0,
        pos: pos.clone(),
        st: UserState {
            template_path: name,
            partial_nesting: 1,
            breaking_spaces: false,
            first_nonspace: pos,
            nested_col: None,
            inside_directive: false,
        },
        load,
        fatal: None,
        depth: 0,
        max_depth,
        partials: HashMap::new(),
    };
    let reply = parser.template_then_eof();
    match (reply, parser.fatal.take()) {
        (_, Some(fatal)) => Err(Err(fatal)),
        (Ok((node, _, _)), None) => Ok(node),
        (Err((_, err)), None) => Err(Ok(err.to_string())),
    }
}

impl<E> Parser<'_, E> {
    fn save(&self) -> Snapshot {
        #[cfg(test)]
        crate::steps::step();
        Snapshot {
            input: self.input.clone(),
            idx: self.idx,
            pos: self.pos.clone(),
            st: self.st.clone(),
        }
    }

    fn restore(&mut self, snap: Snapshot) {
        self.input = snap.input;
        self.idx = snap.idx;
        self.pos = snap.pos;
        self.st = snap.st;
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.idx).copied()
    }

    fn ret<T>(&self, value: T) -> R<T> {
        Ok((value, false, PErr::unknown(self.pos.clone())))
    }

    fn guard(&self, ok: bool) -> R<()> {
        if ok {
            self.ret(())
        } else {
            Err((false, PErr::unknown(self.pos.clone())))
        }
    }

    fn fail<T>(&self, msg: String) -> R<T> {
        Err((false, PErr::new(self.pos.clone(), Msg::Message(msg))))
    }

    fn fatal_error<T>(&mut self, fatal: Fatal<E>) -> R<T> {
        self.fatal = Some(fatal);
        Err((true, PErr::unknown(self.pos.clone())))
    }

    fn catchable(&self, reply: &(bool, PErr)) -> bool {
        !reply.0 && self.fatal.is_none()
    }

    fn satisfy(&mut self, f: impl Fn(char) -> bool) -> R<char> {
        #[cfg(test)]
        crate::steps::step();
        match self.peek() {
            Some(c) if f(c) => {
                self.idx += 1;
                self.pos.advance(c);
                Ok((c, true, PErr::unknown(self.pos.clone())))
            }
            Some(c) => Err((
                false,
                PErr::new(
                    self.pos.clone(),
                    Msg::SysUnExpect(show_string(&c.to_string())),
                ),
            )),
            None => Err((
                false,
                PErr::new(self.pos.clone(), Msg::SysUnExpect(String::new())),
            )),
        }
    }

    fn label<T>(reply: R<T>, label: &str) -> R<T> {
        match reply {
            Ok((v, false, e)) if !e.is_unknown() => Ok((v, false, e.set_expect(label))),
            Err((false, e)) => Err((false, e.set_expect(label))),
            other => other,
        }
    }

    fn char_(&mut self, c: char) -> R<char> {
        let reply = self.satisfy(|x| x == c);
        Self::label(reply, &show_string(&c.to_string()))
    }

    fn letter(&mut self) -> R<char> {
        let reply = self.satisfy(char::is_alphabetic);
        Self::label(reply, "letter")
    }

    /// Parsec `string`: the error is at the start, and a mismatch after the
    /// first character is a consumed error.
    fn string(&mut self, s: &str) -> R<String> {
        #[cfg(test)]
        crate::steps::step();
        let start = self.pos.clone();
        let error = |found: Option<char>| {
            let unexpected = found.map_or(String::new(), |c| show_string(&c.to_string()));
            PErr::new(start.clone(), Msg::SysUnExpect(unexpected)).set_expect(&show_string(s))
        };
        for (i, expected) in s.chars().enumerate() {
            match self.peek() {
                Some(c) if c == expected => {
                    self.idx += 1;
                    self.pos.advance(c);
                }
                found => return Err((i > 0, error(found))),
            }
        }
        Ok((s.to_owned(), true, PErr::unknown(self.pos.clone())))
    }

    fn attempt<T>(&mut self, p: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        match p(self) {
            Err((true, e)) if self.fatal.is_none() => Err((false, e)),
            other => other,
        }
    }

    fn or<T>(
        &mut self,
        p: impl FnOnce(&mut Self) -> R<T>,
        q: impl FnOnce(&mut Self) -> R<T>,
    ) -> R<T> {
        let snap = self.save();
        match p(self) {
            Err(fail) if self.catchable(&fail) => {
                self.restore(snap);
                match q(self) {
                    Ok((v, false, e2)) => Ok((v, false, fail.1.merge(e2))),
                    Err((false, e2)) => Err((false, fail.1.merge(e2))),
                    other => other,
                }
            }
            other => other,
        }
    }

    fn option<T>(&mut self, default: T, p: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        self.or(p, |s| s.ret(default))
    }

    /// Parsec `many`: the final error is the error of the failed step.
    fn many<T>(&mut self, p: impl FnMut(&mut Self) -> R<T>) -> R<Vec<T>> {
        self.repeat(p, false)
    }

    /// The `many` of `Control.Applicative`, built from `<|>` and `<*>`:
    /// the error of the last successful step is merged into the final
    /// error. `doctemplates` uses it for the template body and for the
    /// pipes after a partial.
    fn many_alt<T>(&mut self, p: impl FnMut(&mut Self) -> R<T>) -> R<Vec<T>> {
        self.repeat(p, true)
    }

    fn repeat<T>(&mut self, mut p: impl FnMut(&mut Self) -> R<T>, merge_last: bool) -> R<Vec<T>> {
        let mut items = Vec::new();
        let mut consumed = false;
        let mut last: Option<PErr> = None;
        loop {
            let snap = self.save();
            match p(self) {
                Ok((v, true, e)) => {
                    items.push(v);
                    consumed = true;
                    last = Some(e);
                }
                Ok((_, false, e)) => return Ok((items, consumed, e)),
                Err(fail) if self.catchable(&fail) => {
                    let start = snap.pos.clone();
                    self.restore(snap);
                    let err = match last {
                        Some(last) if merge_last => {
                            last.merge_bind(fail.1.merge(PErr::unknown(start)))
                        }
                        _ => fail.1,
                    };
                    return Ok((items, consumed, err));
                }
                Err((c, e)) => return Err((c || consumed, e)),
            }
        }
    }

    /// `<|>` over a list of parsers, in one stack frame.
    fn choice<T>(&mut self, alternatives: &[fn(&mut Self) -> R<T>]) -> R<T> {
        let mut err: Option<PErr> = None;
        for alternative in alternatives {
            let snap = self.save();
            match alternative(self) {
                Err(fail) if self.catchable(&fail) => {
                    self.restore(snap);
                    err = Some(match err {
                        Some(e) => e.merge(fail.1),
                        None => fail.1,
                    });
                }
                Ok((v, false, e2)) => return Ok((v, false, merge_option(err, e2))),
                Err((false, e2)) => return Err((false, merge_option(err, e2))),
                other => return other,
            }
        }
        Err((
            false,
            err.unwrap_or_else(|| PErr::unknown(self.pos.clone())),
        ))
    }

    fn many1<T>(&mut self, mut p: impl FnMut(&mut Self) -> R<T>) -> R<Vec<T>> {
        let mut a = Acc::new(&self.pos);
        let first = a.step(p(self))?;
        let mut items = a.step(self.many(p))?;
        items.insert(0, first);
        a.ok(items)
    }

    fn skip_many<T>(&mut self, p: impl FnMut(&mut Self) -> R<T>) -> R<()> {
        self.many(p).map(|(_, c, e)| ((), c, e))
    }

    fn look_ahead<T>(&mut self, p: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        let snap = self.save();
        let value = p(self)?.0;
        let pos = snap.pos.clone();
        self.restore(snap);
        Ok((value, false, PErr::unknown(pos)))
    }

    fn not_followed_by<T>(
        &mut self,
        p: impl FnOnce(&mut Self) -> R<T>,
        show: impl Fn(&T) -> String,
    ) -> R<()> {
        let snap = self.save();
        match p(self) {
            Ok((v, _, _)) => {
                let err = PErr::new(self.pos.clone(), Msg::UnExpect(show(&v)));
                self.restore(snap);
                Err((false, err))
            }
            Err(fail) if self.fatal.is_none() => {
                let pos = snap.pos.clone();
                self.restore(snap);
                Ok(((), false, fail.1.merge(PErr::unknown(pos))))
            }
            Err(fail) => Err(fail),
        }
    }

    fn eof(&mut self) -> R<()> {
        let pos = self.pos.clone();
        match self.peek() {
            Some(c) => Err((
                false,
                PErr::new(pos, Msg::UnExpect(show_char(c))).set_expect("end of input"),
            )),
            None => Ok((
                (),
                false,
                PErr::new(pos, Msg::SysUnExpect(String::new())).set_expect("end of input"),
            )),
        }
    }

    fn space_or_tab(&mut self) -> R<char> {
        self.satisfy(|c| c == ' ' || c == '\t')
    }

    fn space_or_tab_char(&mut self) -> R<char> {
        self.or(|s| s.char_(' '), |s| s.char_('\t'))
    }

    fn template_then_eof(&mut self) -> R<Node> {
        let mut a = Acc::new(&self.pos);
        let node = a.step(self.p_template())?;
        a.step(self.eof())?;
        a.ok(node)
    }

    /// Runs a parser one nesting level deeper.
    /// The depth check comes before the recursive call, so that excess
    /// nesting gives `TooDeep` before it uses more stack.
    fn p_template(&mut self) -> R<Node> {
        if self.depth >= self.max_depth {
            return self.fatal_error(Fatal::TooDeep);
        }
        self.depth += 1;
        let reply = self.p_template_body();
        self.depth -= 1;
        reply
    }

    fn p_template_body(&mut self) -> R<Node> {
        let mut a = Acc::new(&self.pos);
        a.step(self.skip_many(Self::p_comment))?;
        let items = a.step(self.many_alt(Self::p_piece))?;
        a.ok(Node::concat(items))
    }

    /// `pLit <|> pNewline <|> pDirective <|> pEscape`, then comments.
    fn p_piece(&mut self) -> R<Node> {
        let mut a = Acc::new(&self.pos);
        let node = a.step(self.choice(&[
            Self::p_lit,
            Self::p_newline,
            Self::p_conditional,
            Self::p_for_loop,
            Self::p_reflow_toggle,
            Self::p_nested,
            Self::p_interpolate,
            Self::p_bare_partial,
            Self::p_escape,
        ]))?;
        a.step(self.skip_many(Self::p_comment))?;
        a.ok(node)
    }

    fn p_comment(&mut self) -> R<()> {
        let column = self.pos.column;
        let mut a = Acc::new(&self.pos);
        a.step(self.attempt(|s| s.string("$--")))?;
        a.step(self.skip_many(|s| s.satisfy(|c| c != '\n')))?;
        if column == 1 {
            a.step(self.p_newline_or_eof())?;
        }
        a.ok(())
    }

    fn p_line_ending(&mut self) -> R<String> {
        self.or(
            |s| s.string("\n"),
            |s| s.or(|s| s.attempt(|s| s.string("\r\n")), |s| s.string("\r")),
        )
    }

    fn p_newline_or_eof(&mut self) -> R<()> {
        self.or(|s| s.p_line_ending().map(|(_, c, e)| ((), c, e)), Self::eof)
    }

    fn p_endline(&mut self) -> R<String> {
        self.attempt(|s| {
            let mut a = Acc::new(&s.pos);
            let nls = a.step(s.p_line_ending())?;
            if let Some(col) = s.st.nested_col {
                a.step(s.skip_many(|s| {
                    let mut b = Acc::new(&s.pos);
                    b.step(s.guard(s.pos.column < col))?;
                    b.step(s.space_or_tab_char())?;
                    b.ok(())
                }))?;
                let ok = s.st.inside_directive || s.pos.column >= col;
                a.step(s.guard(ok))?;
            }
            a.ok(nls)
        })
    }

    fn p_blank_line(&mut self) -> R<Node> {
        self.attempt(|s| {
            let mut a = Acc::new(&s.pos);
            let nls = a.step(s.p_line_ending())?;
            a.step(s.look_ahead(Self::p_newline_or_eof))?;
            a.ok(Node::Literal(literal(nls)))
        })
    }

    fn p_newline(&mut self) -> R<Node> {
        self.attempt(|s| {
            let mut a = Acc::new(&s.pos);
            let nls = a.step(s.p_endline())?;
            let sps = a.step(s.many(Self::space_or_tab_char))?;
            s.st.first_nonspace = s.pos.clone();
            let doc = if s.st.breaking_spaces {
                space()
            } else {
                literal(nls + &sps.into_iter().collect::<String>())
            };
            a.ok(Node::Literal(doc))
        })
    }

    fn p_lit(&mut self) -> R<Node> {
        let mut a = Acc::new(&self.pos);
        let cs = a.step(self.many1(|s| s.satisfy(|c| c != '$' && c != '\n' && c != '\r')))?;
        if cs.iter().all(|&c| c == ' ' || c == '\t') && self.pos.line == 1 {
            self.st.first_nonspace = self.pos.clone();
        }
        let node = if self.st.breaking_spaces {
            to_breakable(&cs)
        } else {
            Node::Literal(literal(cs.into_iter().collect::<String>()))
        };
        a.ok(node)
    }

    fn p_escape(&mut self) -> R<Node> {
        self.attempt(|s| {
            let mut a = Acc::new(&s.pos);
            a.step(s.string("$$"))?;
            s.pos.column = s.pos.column.saturating_sub(1);
            a.ok(Node::Literal(literal("$")))
        })
    }

    fn p_open(&mut self) -> R<Closer> {
        self.or(
            |s| {
                s.attempt(|s| {
                    let mut a = Acc::new(&s.pos);
                    a.step(s.char_('$'))?;
                    a.step(s.not_followed_by(
                        |s| s.or(|s| s.char_('$'), |s| s.char_('{')),
                        |c| show_char(*c),
                    ))?;
                    a.ok(Closer::Dollar)
                })
            },
            |s| {
                s.attempt(|s| {
                    let mut a = Acc::new(&s.pos);
                    a.step(s.string("${"))?;
                    a.step(s.not_followed_by(|s| s.char_('}'), |c| show_char(*c)))?;
                    a.ok(Closer::Brace)
                })
            },
        )
    }

    fn p_close(&mut self, closer: Closer) -> R<()> {
        let reply = match closer {
            Closer::Dollar => self.char_('$'),
            Closer::Brace => self.attempt(|s| s.char_('}')),
        };
        reply.map(|(_, c, e)| ((), c, e))
    }

    fn p_enclosed<T>(&mut self, p: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        self.attempt(|s| {
            let mut a = Acc::new(&s.pos);
            let closer = a.step(s.p_open())?;
            a.step(s.skip_many(Self::space_or_tab))?;
            let result = a.step(p(s))?;
            a.step(s.skip_many(Self::space_or_tab))?;
            a.step(s.p_close(closer))?;
            a.ok(result)
        })
    }

    fn p_keyword_var(&mut self, keyword: &str) -> R<Variable> {
        self.p_enclosed(|s| {
            s.attempt(|s| {
                let mut a = Acc::new(&s.pos);
                a.step(s.string(keyword))?;
                a.step(s.char_('('))?;
                let var = a.step(s.p_var())?;
                a.step(s.char_(')'))?;
                a.ok(var)
            })
        })
    }

    fn p_keyword(&mut self, keyword: &str) -> R<String> {
        self.p_enclosed(|s| s.string(keyword))
    }

    fn p_multiline(&mut self) -> R<bool> {
        self.option(false, |s| s.skip_endline().map(|(_, c, e)| (true, c, e)))
    }

    fn p_optional_endline(&mut self, multiline: bool) -> R<()> {
        if multiline {
            self.option((), Self::skip_endline)
        } else {
            self.ret(())
        }
    }

    fn p_conditional(&mut self) -> R<Node> {
        let mut a = Acc::new(&self.pos);
        let var = a.step(self.p_keyword_var("if"))?;
        let old = std::mem::replace(&mut self.st.inside_directive, true);
        let node = self.p_conditional_rest(var);
        self.st.inside_directive = old;
        let node = a.step(node)?;
        a.ok(node)
    }

    fn p_conditional_rest(&mut self, var: Variable) -> R<Node> {
        let mut b = Acc::new(&self.pos);
        let multiline = b.step(self.p_multiline())?;
        let then = b.step(self.p_template())?;
        let other = b.step(self.p_else_part(multiline))?;
        b.step(self.p_keyword("endif"))?;
        b.step(self.p_optional_endline(multiline))?;
        b.ok(Node::conditional(var, then, other))
    }

    /// `option mempty (pElse multiline <|> pElseIf)`, written out so that
    /// the recursion through it needs no closure frames.
    fn p_else_part(&mut self, multiline: bool) -> R<Node> {
        let start = self.save();
        let reply = match self.p_else(multiline) {
            Err(fail) if self.catchable(&fail) => {
                self.restore(start);
                let again = self.save();
                match self.p_else_if() {
                    Ok((v, false, e2)) => Ok((v, false, fail.1.merge(e2))),
                    Err(fail2) if self.catchable(&fail2) => {
                        self.restore(again);
                        Err((false, fail.1.merge(fail2.1)))
                    }
                    other => other,
                }
            }
            other => other,
        };
        self.or_empty(reply)
    }

    /// `option mempty (pEnclosed (string "sep") *> ... *> pTemplate)`.
    fn p_sep_part(&mut self, multiline: bool) -> R<Node> {
        let start = self.save();
        let reply = self.p_sep_body(multiline);
        match reply {
            Err(fail) if self.catchable(&fail) => {
                self.restore(start);
                self.or_empty(Err(fail))
            }
            other => other,
        }
    }

    fn p_sep_body(&mut self, multiline: bool) -> R<Node> {
        let mut c = Acc::new(&self.pos);
        c.step(self.p_keyword("sep"))?;
        c.step(self.p_optional_endline(multiline))?;
        let sep = c.step(self.p_template())?;
        c.ok(sep)
    }

    /// `p <|> return mempty`, after `p` failed and the state is restored.
    fn or_empty(&self, reply: R<Node>) -> R<Node> {
        match reply {
            Err(fail) if self.catchable(&fail) => Ok((
                Node::Empty,
                false,
                fail.1.merge(PErr::unknown(self.pos.clone())),
            )),
            other => other,
        }
    }

    fn p_else(&mut self, multiline: bool) -> R<Node> {
        let mut a = Acc::new(&self.pos);
        a.step(self.p_keyword("else"))?;
        a.step(self.p_optional_endline(multiline))?;
        let node = a.step(self.p_template())?;
        a.ok(node)
    }

    fn p_else_if(&mut self) -> R<Node> {
        if self.depth >= self.max_depth {
            return self.fatal_error(Fatal::TooDeep);
        }
        self.depth += 1;
        let reply = self.p_else_if_body();
        self.depth -= 1;
        reply
    }

    fn p_else_if_body(&mut self) -> R<Node> {
        let mut a = Acc::new(&self.pos);
        let var = a.step(self.p_keyword_var("elseif"))?;
        let multiline = a.step(self.p_multiline())?;
        let then = a.step(self.p_template())?;
        let other = a.step(self.p_else_part(multiline))?;
        a.ok(Node::conditional(var, then, other))
    }

    fn skip_endline(&mut self) -> R<()> {
        let mut a = Acc::new(&self.pos);
        a.step(self.p_endline())?;
        let pos = a.step(self.look_ahead(|s| {
            let mut b = Acc::new(&s.pos);
            b.step(s.skip_many(Self::space_or_tab_char))?;
            b.ok(s.pos.clone())
        }))?;
        self.st.first_nonspace = pos;
        a.ok(())
    }

    fn p_reflow_toggle(&mut self) -> R<Node> {
        let mut a = Acc::new(&self.pos);
        a.step(self.p_enclosed(|s| s.char_('~')))?;
        self.st.breaking_spaces = !self.st.breaking_spaces;
        a.ok(Node::Empty)
    }

    fn p_nested(&mut self) -> R<Node> {
        let col = self.pos.column;
        let mut a = Acc::new(&self.pos);
        a.step(self.p_enclosed(|s| s.char_('^')))?;
        let old = self.st.nested_col;
        self.st.nested_col = Some(col);
        let first = a.step(self.p_template())?;
        let rest = a.step(self.many(|s| {
            s.attempt(|s| {
                let mut b = Acc::new(&s.pos);
                let blanks = b.step(s.many1(Self::p_blank_line))?;
                let next = b.step(s.p_template())?;
                b.ok(Node::concat(blanks).append(next))
            })
        }))?;
        self.st.nested_col = old;
        a.ok(Node::Nested(Box::new(first.append(Node::concat(rest)))))
    }

    fn p_for_loop(&mut self) -> R<Node> {
        let mut a = Acc::new(&self.pos);
        let var = a.step(self.p_keyword_var("for"))?;
        let old = std::mem::replace(&mut self.st.inside_directive, true);
        let node = self.p_for_loop_rest(var);
        self.st.inside_directive = old;
        let node = a.step(node)?;
        a.ok(node)
    }

    fn p_for_loop_rest(&mut self, var: Variable) -> R<Node> {
        let mut b = Acc::new(&self.pos);
        let multiline = b.step(self.p_multiline())?;
        let body = b.step(self.p_template())?;
        let sep = b.step(self.p_sep_part(multiline))?;
        b.step(self.p_keyword("endfor"))?;
        b.step(self.p_optional_endline(multiline))?;
        b.ok(Node::iterate(var, body, sep))
    }

    fn p_interpolate(&mut self) -> R<Node> {
        let pos = self.pos.clone();
        let mut a = Acc::new(&self.pos);
        let (closer, var) = a.step(self.attempt(|s| {
            let mut b = Acc::new(&s.pos);
            let closer = b.step(s.p_open())?;
            b.step(s.skip_many(Self::space_or_tab))?;
            let var = b.step(s.p_var())?;
            b.step(s.not_followed_by(|s| s.char_('('), |c| show_char(*c)))?;
            b.ok((closer, var))
        }))?;
        let node = a.step(self.or(
            |s| {
                let mut b = Acc::new(&s.pos);
                b.step(s.char_(':'))?;
                let name = b.step(s.p_partial_name())?;
                let node = b.step(s.p_partial(Some(var.clone()), &name))?;
                b.ok(node)
            },
            |s| {
                s.or(
                    |s| {
                        let mut b = Acc::new(&s.pos);
                        let sep = b.step(s.p_sep())?;
                        let it = Node::interpolate(Variable {
                            parts: vec!["it".to_owned()],
                            pipes: Vec::new(),
                        });
                        b.ok(Node::iterate(var.clone(), it, sep))
                    },
                    |s| s.ret(Node::interpolate(var.clone())),
                )
            },
        ))?;
        a.step(self.skip_many(Self::space_or_tab))?;
        a.step(self.p_close(closer))?;
        let node = a.step(self.handle_nesting(false, &pos, node))?;
        a.ok(node)
    }

    fn handle_nesting(&mut self, eat_endline: bool, pos: &Pos, node: Node) -> R<Node> {
        let mut a = Acc::new(&self.pos);
        let begin_line = self.st.first_nonspace == *pos;
        let end_of_line = a.step(self.or(
            |s| {
                s.look_ahead(Self::p_newline_or_eof)
                    .map(|(_, c, e)| (true, c, e))
            },
            |s| s.ret(false),
        ))?;
        if eat_endline && begin_line {
            a.step(self.option((), Self::skip_endline))?;
        }
        let nested_col = self.st.nested_col;
        let node = if begin_line && end_of_line {
            match node {
                Node::Nested(_) => node,
                _ if pos.column == 1 || Some(pos.column) == nested_col => node,
                _ => Node::Nested(Box::new(node)),
            }
        } else {
            node
        };
        a.ok(node)
    }

    fn p_bare_partial(&mut self) -> R<Node> {
        let pos = self.pos.clone();
        let mut a = Acc::new(&self.pos);
        let (closer, name) = a.step(self.attempt(|s| {
            let mut b = Acc::new(&s.pos);
            let closer = b.step(s.p_open())?;
            b.step(s.skip_many(Self::space_or_tab))?;
            let name = b.step(s.p_partial_name())?;
            b.ok((closer, name))
        }))?;
        let node = a.step(self.p_partial(None, &name))?;
        a.step(self.skip_many(Self::space_or_tab))?;
        a.step(self.p_close(closer))?;
        let node = a.step(self.handle_nesting(true, &pos, node))?;
        a.ok(node)
    }

    fn p_partial_name(&mut self) -> R<String> {
        self.attempt(|s| {
            let mut a = Acc::new(&s.pos);
            let name = a.step(s.many1(|s| {
                s.or(
                    |s| {
                        let reply = s.satisfy(char::is_alphanumeric);
                        Self::label(reply, "letter or digit")
                    },
                    |s| s.satisfy(|c| matches!(c, '_' | '-' | '.' | '/' | '\\')),
                )
            }))?;
            a.step(s.string("()"))?;
            a.ok(name.into_iter().collect())
        })
    }

    fn p_partial(&mut self, var: Option<Variable>, name: &str) -> R<Node> {
        let old_state = self.st.clone();
        let mut a = Acc::new(&self.pos);
        let sep = a.step(self.option(Node::Empty, Self::p_sep))?;
        let path = partial_path(&self.st.template_path, name);
        let nesting = self.st.partial_nesting;
        let key = PartialKey {
            path,
            nesting,
            depth: self.depth,
            breaking_spaces: self.st.breaking_spaces,
            inside_directive: self.st.inside_directive,
        };
        let body = if let Some(parsed) = self.partials.get(&key) {
            a.step(Ok(parsed.clone()))?
        } else {
            a.step(self.p_partial_body(&key))?
        };
        self.st = old_state;
        let pipes = a.step(self.many_alt(Self::p_pipe))?;
        let partial = Node::Partial(pipes, Box::new(body));
        a.ok(match var {
            Some(var) => Node::iterate(var, partial, sep),
            None => partial,
        })
    }

    /// Reads and parses a partial, and keeps the result for the next use.
    fn p_partial_body(&mut self, key: &PartialKey) -> R<Node> {
        let path = key.path.clone();
        let nesting = key.nesting;
        let text = match (self.load)(&path) {
            Ok(text) => remove_final_newline(text),
            Err(e) => return self.fatal_error(Fatal::Load(e)),
        };
        let reply = if nesting > MAX_PARTIAL_NESTING {
            self.ret(Node::Literal(literal("(loop)")))
        } else {
            let old_input = std::mem::replace(&mut self.input, text.chars().collect());
            let old_idx = std::mem::replace(&mut self.idx, 0);
            let old_pos = std::mem::replace(&mut self.pos, Pos::initial(Rc::from(path.as_str())));
            self.st.partial_nesting = nesting + 1;
            self.st.nested_col = None;
            let reply = self.template_then_eof();
            if let Err(fail) = reply {
                return Err(fail);
            }
            self.st.partial_nesting = nesting;
            self.input = old_input;
            self.idx = old_idx;
            self.pos = old_pos;
            reply
        };
        if let Ok(parsed) = &reply {
            self.partials.insert(
                PartialKey {
                    path,
                    nesting,
                    depth: key.depth,
                    breaking_spaces: key.breaking_spaces,
                    inside_directive: key.inside_directive,
                },
                parsed.clone(),
            );
        }
        reply
    }

    fn p_sep(&mut self) -> R<Node> {
        let mut a = Acc::new(&self.pos);
        a.step(self.char_('['))?;
        let text = a.step(self.many(|s| s.satisfy(|c| c != ']')))?;
        a.step(self.char_(']'))?;
        a.ok(Node::Literal(literal(text.into_iter().collect::<String>())))
    }

    fn p_var(&mut self) -> R<Variable> {
        let mut a = Acc::new(&self.pos);
        let first = a.step(self.or(Self::p_ident_part, |s| s.attempt(|s| s.string("it"))))?;
        let rest = a.step(self.many(|s| {
            let mut b = Acc::new(&s.pos);
            b.step(s.char_('.'))?;
            let part = b.step(s.p_ident_part())?;
            b.ok(part)
        }))?;
        let pipes = a.step(self.many(Self::p_pipe))?;
        let mut parts = vec![first];
        parts.extend(rest);
        a.ok(Variable { parts, pipes })
    }

    fn p_ident_part(&mut self) -> R<String> {
        self.attempt(|s| {
            let mut a = Acc::new(&s.pos);
            let first = a.step(s.letter())?;
            let rest =
                a.step(s.many(|s| s.satisfy(|c| c.is_alphanumeric() || c == '_' || c == '-')))?;
            let part: String = std::iter::once(first).chain(rest).collect();
            a.step(s.guard(!RESERVED.contains(&part.as_str())))?;
            a.ok(part)
        })
    }

    fn p_pipe(&mut self) -> R<Pipe> {
        let mut a = Acc::new(&self.pos);
        a.step(self.char_('/'))?;
        let name: String = a.step(self.many1(Self::letter))?.into_iter().collect();
        a.step(self.not_followed_by(Self::letter, |c| show_char(*c)))?;
        let pipe = match name.as_str() {
            "uppercase" => Pipe::Uppercase,
            "lowercase" => Pipe::Lowercase,
            "pairs" => Pipe::Pairs,
            "length" => Pipe::Length,
            "alpha" => Pipe::Alpha,
            "roman" => Pipe::Roman,
            "reverse" => Pipe::Reverse,
            "first" => Pipe::First,
            "rest" => Pipe::Rest,
            "last" => Pipe::Last,
            "allbutlast" => Pipe::AllButLast,
            "chomp" => Pipe::Chomp,
            "nowrap" => Pipe::NoWrap,
            "left" | "right" | "center" => {
                let align = match name.as_str() {
                    "left" => Align::Left,
                    "right" => Align::Right,
                    _ => Align::Center,
                };
                let width = a.step(self.p_block_width())?;
                a.step(self.skip_many(Self::p_space))?;
                let left = a.step(self.p_border())?;
                let right = a.step(self.p_border())?;
                Pipe::Block(align, width, left, right)
            }
            _ => {
                a.step(self.fail::<()>(format!("Unknown pipe {name}")))?;
                Pipe::Chomp
            }
        };
        a.ok(pipe)
    }

    fn p_space(&mut self) -> R<char> {
        let reply = self.satisfy(is_space);
        Self::label(reply, "space")
    }

    fn p_block_width(&mut self) -> R<usize> {
        let reply = self.attempt(|s| {
            let mut a = Acc::new(&s.pos);
            a.step(s.many1(Self::p_space))?;
            let digits = a.step(s.many1(|s| {
                let reply = s.satisfy(|c| c.is_ascii_digit());
                Self::label(reply, "digit")
            }))?;
            let value = digits.iter().fold(0i64, |acc, d| {
                acc.wrapping_mul(10)
                    .wrapping_add(i64::from(d.to_digit(10).unwrap_or(0)))
            });
            let width = usize::try_from(value).unwrap_or(0).min(MAX_BLOCK_WIDTH);
            a.ok(width)
        });
        Self::label(reply, "integer parameter for pipe")
    }

    /// One optional border of a block pipe, with the spaces after it.
    fn p_border(&mut self) -> R<String> {
        self.option(String::new(), |s| {
            let mut b = Acc::new(&s.pos);
            b.step(s.char_('"'))?;
            let chars = b.step(s.many(|s| {
                s.or(
                    |s| s.satisfy(|c| c != '"' && c != '\\'),
                    |s| {
                        let mut c = Acc::new(&s.pos);
                        c.step(s.char_('\\'))?;
                        let ch = c.step(s.satisfy(|_| true))?;
                        c.ok(ch)
                    },
                )
            }))?;
            b.step(s.char_('"'))?;
            b.step(s.skip_many(Self::p_space))?;
            b.ok(chars.into_iter().collect::<String>())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_paths() {
        assert_eq!(partial_path("test/foobar.txt", "bad"), "test/bad.txt");
        assert_eq!(partial_path("", "bad"), "bad");
        assert_eq!(partial_path("dir/x.txt", "a.tex"), "dir/a.tex");
        assert_eq!(partial_path("x.txt", "sub/p"), "sub/p.txt");
        assert_eq!(partial_path("dir/x", "p"), "dir/p");
        assert_eq!(partial_path("dir/x.txt", "/abs.tex"), "/abs.tex");
    }

    #[test]
    fn breakable_runs() {
        let chars: Vec<char> = "a  b ".chars().collect();
        let Node::Concat(items) = to_breakable(&chars) else {
            unreachable!("four runs give a concatenation")
        };
        assert_eq!(items.len(), 4);
        assert_eq!(to_breakable(&[]), Node::Empty);
    }

    #[test]
    fn haskell_spaces() {
        assert!(is_space('\u{b}') && is_space('\u{3000}') && is_space('\u{a0}'));
        assert!(!is_space('\u{85}') && !is_space('\u{2028}') && !is_space('a'));
    }
}
