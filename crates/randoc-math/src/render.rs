//! The two outputs: MathML text and the inline list.

use std::sync::OnceLock;

use randoc_ast::{Attr, Inline};

use crate::symbols::{ACCENT_SYMBOLS, SYMBOLS, VARIANTS};
use crate::tree::{Aligns, Alphabet, Class, Exp, Row, Wrap};

/// Large operators that take their scripts above and below in display math.
const LIMIT_OPS: &[&str] = &[
    "\u{2210}", "\u{220f}", "\u{2211}", "\u{22c0}", "\u{22c1}", "\u{22c2}", "\u{22c3}", "\u{2a06}",
];

/// The class that a neighbour sees.
fn class_of(exp: &Exp) -> Class {
    match exp {
        Exp::Sym(Class::Ident | Class::IdentUpright | Class::Text, _) => Class::Ord,
        Exp::Sym(Class::Func | Class::FuncLimits, _) => Class::Func,
        Exp::Sym(class, _) => *class,
        // A large operator with scripts is an operand for a binary operator.
        Exp::Scripts { base, .. } => match class_of(base) {
            Class::Op => Class::Ord,
            class => class,
        },
        _ => Class::Ord,
    }
}

/// The classes of the items of a list after the TeX rule for binary operators:
/// a binary operator with no left operand, or before a relation, a closing
/// delimiter or a punctuation mark, is an ordinary symbol.
fn classes(items: &[Exp]) -> Vec<Class> {
    let mut out: Vec<Class> = Vec::with_capacity(items.len());
    for (i, exp) in items.iter().enumerate() {
        let mut class = class_of(exp);
        if class == Class::Bin {
            let prev = out.last().copied();
            // A relation with scripts does not end the operand of the operator.
            let next = items.get(i + 1).map(|next| match next {
                Exp::Scripts { .. } => Class::Ord,
                other => class_of(other),
            });
            // A punctuation mark is a left end also where it takes no space.
            let after_punct = i
                .checked_sub(1)
                .and_then(|at| items.get(at))
                .is_some_and(|prev| class_of(prev) == Class::Punct);
            let no_left = after_punct
                || matches!(
                    prev,
                    None | Some(Class::Bin | Class::Rel | Class::Open | Class::Punct | Class::Op)
                );
            let no_right = matches!(next, Some(Class::Rel | Class::Close | Class::Punct));
            if no_left || no_right {
                class = Class::Ord;
            }
        }
        // A punctuation mark with no left operand takes no space after it.
        if class == Class::Punct
            && matches!(
                out.last(),
                Some(Class::Open | Class::Op | Class::Rel | Class::Punct)
            )
        {
            class = Class::Ord;
        }
        out.push(class);
    }
    out
}

fn escape_char(c: char, out: &mut String) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '\'' => out.push_str("&#39;"),
        '"' => out.push_str("&quot;"),
        c if c != '\n' && not_printable(c) => {
            out.push_str("&#");
            out.push_str(&u32::from(c).to_string());
            out.push(';');
        }
        c => out.push(c),
    }
}

/// Reports whether the oracle writes `c` as a character reference: a control
/// character, a format character, a line or paragraph separator, or a
/// private-use character.
fn not_printable(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{ad}'
                | '\u{600}'..='\u{605}'
                | '\u{61c}'
                | '\u{6dd}'
                | '\u{70f}'
                | '\u{890}'..='\u{891}'
                | '\u{8e2}'
                | '\u{180e}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{206f}'
                | '\u{e000}'..='\u{f8ff}'
                | '\u{feff}'
                | '\u{fff9}'..='\u{fffb}'
                | '\u{110bd}'
                | '\u{110cd}'
                | '\u{13430}'..='\u{1343f}'
                | '\u{1bca0}'..='\u{1bca3}'
                | '\u{1d173}'..='\u{1d17a}'
                | '\u{e0001}'
                | '\u{e0020}'..='\u{e007f}'
                | '\u{f0000}'..='\u{10ffff}'
        )
}

/// The character pairs of a `mathvariant` value.
pub(crate) fn variant_chars(variant: &str) -> &'static [(char, char)] {
    VARIANTS
        .binary_search_by(|(name, _)| (*name).cmp(variant))
        .ok()
        .and_then(|i| VARIANTS.get(i))
        .map_or(&[], |(_, pairs)| pairs)
}

/// The form of `c` in an alphabet.
pub(crate) fn variant_char(pairs: &[(char, char)], c: char) -> char {
    pairs
        .binary_search_by(|(plain, _)| plain.cmp(&c))
        .ok()
        .and_then(|i| pairs.get(i))
        .map_or(c, |(_, styled)| *styled)
}

/// Writes the full `<math>` element.
pub(crate) fn mathml(items: &[Exp], display: bool, tex: &str) -> String {
    let mut out = String::new();
    out.push_str("<math display=\"");
    out.push_str(if display { "block" } else { "inline" });
    out.push_str("\" xmlns=\"http://www.w3.org/1998/Math/MathML\"><semantics>");
    let mut writer = MathMl {
        out,
        display,
        style: None,
    };
    writer.list(items);
    let mut out = writer.out;
    out.push_str("<annotation encoding=\"application/x-tex\">");
    for c in tex.chars() {
        escape_char(c, &mut out);
    }
    out.push_str("</annotation></semantics></math>");
    out
}

struct MathMl {
    out: String,
    display: bool,
    /// The math alphabet around the item.
    style: Option<&'static Alphabet>,
}

impl MathMl {
    fn open(&mut self, name: &str) {
        self.out.push('<');
        self.out.push_str(name);
        self.out.push('>');
    }

    fn close(&mut self, name: &str) {
        self.out.push_str("</");
        self.out.push_str(name);
        self.out.push('>');
    }

    /// Writes a token element. The text takes the characters of `variant`.
    fn token(&mut self, name: &str, attrs: &str, variant: Option<&str>, text: &str) {
        self.out.push('<');
        self.out.push_str(name);
        self.out.push_str(attrs);
        if let Some(variant) = variant {
            self.out.push_str(" mathvariant=\"");
            self.out.push_str(variant);
            self.out.push('"');
        }
        self.out.push('>');
        let pairs = variant.map_or(&[][..], variant_chars);
        for c in text.chars() {
            escape_char(variant_char(pairs, c), &mut self.out);
        }
        self.close(name);
    }

    fn ident_variant(&self) -> Option<&'static str> {
        self.style.map(|style| style.ident)
    }

    fn other_variant(&self) -> Option<&'static str> {
        self.style.map(|style| style.other)
    }

    fn list(&mut self, items: &[Exp]) {
        let classes = classes(items);
        if items.len() == 1 {
            if let (Some(exp), Some(class)) = (items.first(), classes.first()) {
                self.exp(exp, *class, true);
            }
            return;
        }
        self.open("mrow");
        self.items(items, &classes);
        self.close("mrow");
    }

    fn items(&mut self, items: &[Exp], classes: &[Class]) {
        let last = items.len().saturating_sub(1);
        for (i, (exp, class)) in items.iter().zip(classes).enumerate() {
            self.exp(exp, *class, i == last);
        }
    }

    fn exp(&mut self, exp: &Exp, class: Class, last: bool) {
        match exp {
            Exp::Num(n) => self.token("mn", "", self.other_variant(), n),
            Exp::Sym(Class::Func | Class::FuncLimits, name) if !last => self.func(name),
            Exp::Sym(sym, text) => self.sym(*sym, class, text),
            Exp::Group(items) | Exp::PaddedText(items) => self.list(items),
            Exp::Scripts {
                base,
                sub,
                sup,
                limits,
            } => self.scripts(base, sub.as_deref(), sup.as_deref(), *limits),
            Exp::Frac { num, den, line } => {
                self.out.push_str(if *line {
                    "<mfrac>"
                } else {
                    "<mfrac linethickness=\"0\">"
                });
                self.arg(num);
                self.arg(den);
                self.close("mfrac");
            }
            Exp::Sqrt(body) => self.wrapped("msqrt", "", body),
            Exp::Root(body, index) => {
                self.open("mroot");
                self.arg(body);
                self.arg(index);
                self.close("mroot");
            }
            Exp::Fenced { open, body, close } => {
                self.open("mrow");
                if let Some(open) = open {
                    self.delimiter("prefix", open);
                }
                // The oracle adds no function application inside delimiters.
                for (item, class) in body.iter().zip(classes(body)) {
                    self.exp(item, class, true);
                }
                if let Some(close) = close {
                    self.delimiter("postfix", close);
                }
                self.close("mrow");
            }
            Exp::Middle(text) => self.delimiter("infix", text),
            Exp::Accent { body, mark } => {
                let tag = if mark.under { "munder" } else { "mover" };
                self.open(tag);
                self.arg(body);
                self.token("mo", " accent=\"true\"", None, mark.mathml);
                self.close(tag);
            }
            Exp::Stack { base, under, over } => {
                let tag = match (under.is_some(), over.is_some()) {
                    (true, true) => "munderover",
                    (true, false) => "munder",
                    (false, _) => "mover",
                };
                self.open(tag);
                self.arg(base);
                for script in [under, over].into_iter().flatten() {
                    self.limit(script);
                }
                self.close(tag);
            }
            Exp::Space(space) => {
                self.out.push_str("<mspace width=\"");
                self.out.push_str(space.width);
                self.out.push_str("\"></mspace>");
            }
            Exp::Text(style, text) => self.text(*style, text),
            Exp::Styled(alphabet, items) => {
                let outer = self.style.replace(alphabet);
                self.list(items);
                self.style = outer;
            }
            Exp::Sized(percent, delimiter) => self.sized(percent, delimiter),
            Exp::Display(on, body) => {
                let attrs = if *on {
                    " displaystyle=\"true\""
                } else {
                    " displaystyle=\"false\""
                };
                self.wrapped("mstyle", attrs, body);
            }
            Exp::Enclose(notation, body) => {
                self.out.push_str("<menclose notation=\"");
                self.out.push_str(notation);
                self.out.push_str("\">");
                self.arg(body);
                self.close("menclose");
            }
            Exp::Phantom(body) => self.wrapped("mphantom", "", body),
            Exp::Table {
                aligns,
                rows,
                cells_in_mrow,
            } => self.table(aligns, rows, *cells_in_mrow),
        }
    }

    fn wrapped(&mut self, name: &str, attrs: &str, body: &Exp) {
        self.out.push('<');
        self.out.push_str(name);
        self.out.push_str(attrs);
        self.out.push('>');
        self.arg(body);
        self.close(name);
    }

    /// An argument of a construct. One binary operator stays an operator.
    fn arg(&mut self, exp: &Exp) {
        match exp {
            Exp::Sym(Class::Bin, text) => self.token("mo", "", self.other_variant(), text),
            other => self.list(std::slice::from_ref(other)),
        }
    }

    /// An expression above or below a base. A symbol that is not a letter
    /// is an operator with no alphabet, and the table tells if it is an
    /// accent.
    fn limit(&mut self, exp: &Exp) {
        match exp {
            Exp::Sym(class, text)
                if !matches!(
                    class,
                    Class::Ident
                        | Class::IdentUpright
                        | Class::Func
                        | Class::FuncLimits
                        | Class::Text
                ) =>
            {
                let attrs = if ACCENT_SYMBOLS.binary_search(&text.as_str()).is_ok() {
                    " accent=\"true\""
                } else {
                    " accent=\"false\""
                };
                self.token("mo", attrs, None, text);
            }
            other => self.arg(other),
        }
    }

    fn delimiter(&mut self, form: &str, text: &str) {
        let attrs = format!(" stretchy=\"true\" form=\"{form}\"");
        self.token("mo", &attrs, self.other_variant(), text);
    }

    fn func(&mut self, name: &str) {
        self.open("mrow");
        self.func_name(name);
        self.out.push_str("<mo");
        if let Some(variant) = self.other_variant() {
            self.out.push_str(" mathvariant=\"");
            self.out.push_str(variant);
            self.out.push('"');
        }
        self.out.push_str(">&#8289;</mo>");
        self.close("mrow");
    }

    fn func_name(&mut self, name: &str) {
        let variant = self.ident_variant().unwrap_or("normal");
        self.token("mi", "", Some(variant), name);
    }

    fn text(&mut self, style: Option<&'static Alphabet>, text: &str) {
        let variant = style
            .or(self.style)
            .map_or("normal", |alphabet| alphabet.other);
        self.token("mtext", "", Some(variant), text);
    }

    fn sym(&mut self, sym: Class, class: Class, text: &str) {
        let ident = self.ident_variant();
        let other = self.other_variant();
        match (sym, class) {
            (Class::Bin, Class::Ord) | (Class::Ident | Class::Ord, _) => {
                self.token("mi", "", ident, text);
            }
            (Class::IdentUpright, _) => self.token("mi", "", other.or(Some("normal")), text),
            (Class::Func | Class::FuncLimits, _) => self.func_name(text),
            (Class::Text, _) => self.text(None, text),
            (Class::Open, _) => {
                self.token("mo", " stretchy=\"false\" form=\"prefix\"", other, text)
            }
            (Class::Close, _) => {
                self.token("mo", " stretchy=\"false\" form=\"postfix\"", other, text);
            }
            (Class::Bin | Class::Rel | Class::Punct | Class::Op, _) => {
                self.token("mo", "", other, text);
            }
        }
    }

    /// A delimiter with a fixed size. With no `form`, the oracle writes
    /// `mathvariant` before `stretchy`.
    fn sized(&mut self, percent: &str, delimiter: &Exp) {
        let Exp::Sym(class, text) = delimiter else {
            return self.arg(delimiter);
        };
        let (name, variant) = if *class == Class::Ord {
            ("mi", self.ident_variant())
        } else {
            ("mo", self.other_variant())
        };
        let size = format!(" minsize=\"{percent}\" maxsize=\"{percent}\"");
        let form = match class {
            Class::Open => "prefix",
            Class::Close => "postfix",
            _ => {
                let variant = variant.map_or(String::new(), |v| format!(" mathvariant=\"{v}\""));
                let attrs = format!("{size}{variant} stretchy=\"true\"");
                return self.token(name, &attrs, None, text);
            }
        };
        let attrs = format!("{size} stretchy=\"true\" form=\"{form}\"");
        self.token(name, &attrs, variant, text);
    }

    fn scripts(&mut self, base: &Exp, sub: Option<&Exp>, sup: Option<&Exp>, limits: Option<bool>) {
        let braces = matches!(base, Exp::Accent { mark, .. } if matches!(mark.mathml, "\u{23de}" | "\u{23df}"));
        let limits = limits.unwrap_or(braces || (self.display && has_limits(base)));
        let name = match (limits, sub.is_some(), sup.is_some()) {
            (true, true, true) => "munderover",
            (true, true, false) => "munder",
            (true, false, _) => "mover",
            (false, true, true) => "msubsup",
            (false, true, false) => "msub",
            (false, false, _) => "msup",
        };
        self.open(name);
        match base {
            Exp::Sym(Class::Func | Class::FuncLimits, text) if !limits => self.func(text),
            other => self.arg(other),
        }
        for (script, last) in [(sub, sup.is_none()), (sup, true)] {
            match script {
                None => {}
                Some(script) if limits => self.limit(script),
                Some(Exp::Sym(Class::Func | Class::FuncLimits, name)) if !last => self.func(name),
                Some(script) => self.arg(script),
            }
        }
        self.close(name);
    }

    fn table(&mut self, aligns: &Aligns, rows: &[Row], cells_in_mrow: bool) {
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let pairs_padded = columns % 2 == 0;
        let first_row = rows.first().map_or(0, Vec::len);
        self.open("mtable");
        for row in rows {
            self.open("mtr");
            for (i, cell) in row.iter().enumerate() {
                let (align, padded) = match aligns {
                    Aligns::All(align) => (*align, false),
                    Aligns::FirstRow(align) if i < first_row => (*align, false),
                    Aligns::FirstRow(_) => ("center", false),
                    Aligns::Pairs if i % 2 == 0 => ("right", pairs_padded),
                    Aligns::Pairs => ("left", pairs_padded),
                    Aligns::Cycle(turns) => {
                        let align = turns.get(i % turns.len().max(1)).copied();
                        (align.unwrap_or("center"), false)
                    }
                    Aligns::Columns(columns) => {
                        columns.get(i).copied().unwrap_or(("center", false))
                    }
                };
                let padding = if padded {
                    format!("; padding-{align}: 0")
                } else {
                    String::new()
                };
                self.out.push_str(&format!(
                    "<mtd columnalign=\"{align}\" style=\"text-align: {align}{padding}\">"
                ));
                if cells_in_mrow {
                    self.list(cell);
                } else {
                    let classes = classes(cell);
                    self.items(cell, &classes);
                }
                self.close("mtd");
            }
            self.close("mtr");
        }
        self.close("mtable");
    }
}

fn has_limits(base: &Exp) -> bool {
    match base {
        Exp::Sym(Class::Op, text) => LIMIT_OPS.contains(&text.as_str()),
        Exp::Sym(Class::FuncLimits, _) => true,
        _ => false,
    }
}

/// The inline list cannot show this construct.
pub(crate) struct Unsupported;

/// Writes the inline list.
pub(crate) fn inlines(items: &[Exp]) -> Result<Vec<Inline>, Unsupported> {
    let mut out = Vec::new();
    list(items, None, &mut out)?;
    Ok(merge(out))
}

fn str_(text: impl Into<String>) -> Inline {
    Inline::Str(text.into())
}

/// Appends one item of text in the alphabet `style`.
fn leaf(text: &str, style: Option<&Alphabet>, out: &mut Vec<Inline>) {
    let Some(style) = style else {
        out.push(str_(text));
        return;
    };
    let text: String = if style.inline_chars {
        let pairs = variant_chars(style.other);
        text.chars().map(|c| variant_char(pairs, c)).collect()
    } else {
        text.to_string()
    };
    wrap(style.wrap, vec![str_(text)], out);
}

/// Appends `inner` in the element of `kind`.
fn wrap(kind: Wrap, mut inner: Vec<Inline>, out: &mut Vec<Inline>) {
    out.push(match kind {
        Wrap::Plain => {
            out.append(&mut inner);
            return;
        }
        Wrap::Emph => Inline::Emph(inner),
        Wrap::Strong => Inline::Strong(inner),
        Wrap::StrongEmph => Inline::Strong(vec![Inline::Emph(inner)]),
        Wrap::Code => {
            let mut text = String::new();
            for item in &inner {
                match item {
                    Inline::Str(s) => text.push_str(s),
                    _ => text.push(' '),
                }
            }
            Inline::Code(Attr::default(), text)
        }
    });
}

/// Appends an identifier: `Emph` when no alphabet is set.
fn ident(text: &str, style: Option<&Alphabet>, out: &mut Vec<Inline>) {
    match style {
        None => out.push(Inline::Emph(vec![str_(text)])),
        Some(_) => leaf(text, style, out),
    }
}

fn list(items: &[Exp], style: Option<&Alphabet>, out: &mut Vec<Inline>) -> Result<(), Unsupported> {
    let classes = classes(items);
    let last = items.len().saturating_sub(1);
    for (i, (exp, class)) in items.iter().zip(&classes).enumerate() {
        match (exp, class) {
            (Exp::Sym(Class::Bin | Class::Rel, text), Class::Bin | Class::Rel) => {
                let space = if *class == Class::Bin {
                    "\u{2005}"
                } else {
                    "\u{2004}"
                };
                // A space command or a punctuation mark after the operator
                // replaces its spaces.
                if matches!(items.get(i + 1), Some(Exp::Space(_)))
                    || items.get(i + 1).map(class_of) == Some(Class::Punct)
                {
                    leaf(inline_text(text), style, out);
                    continue;
                }
                if i != last {
                    out.push(str_(space));
                }
                leaf(inline_text(text), style, out);
                if i != last {
                    out.push(str_(space));
                }
            }
            (Exp::Sym(Class::Punct, text), class) => {
                leaf(text, style, out);
                let next = items.get(i + 1);
                let spaced = matches!(next, Some(Exp::Space(_)));
                if *class == Class::Punct
                    && next.is_some()
                    && next.map(class_of) != Some(Class::Punct)
                    && !spaced
                {
                    out.push(str_("\u{2006}"));
                }
            }
            _ => exp_inlines(exp, style, out)?,
        }
    }
    Ok(())
}

fn exp_inlines(
    exp: &Exp,
    style: Option<&Alphabet>,
    out: &mut Vec<Inline>,
) -> Result<(), Unsupported> {
    match exp {
        Exp::Num(n) => leaf(n, style, out),
        Exp::Sym(Class::Ident | Class::IdentUpright, text) => ident(text, style, out),
        Exp::Sym(Class::Func | Class::FuncLimits, name) => {
            leaf(name, style, out);
            out.push(str_("\u{2006}"));
        }
        Exp::Sym(Class::Text, text) => out.push(str_(text.as_str())),
        Exp::Sym(_, text) => {
            if !text.is_empty() {
                leaf(inline_text(text), style, out);
            }
        }
        Exp::Group(items) | Exp::PaddedText(items) => list(items, style, out)?,
        Exp::Scripts {
            base,
            sub,
            sup,
            limits,
        } => {
            let both = sub.is_some() && sup.is_some();
            if *limits == Some(true) || (limits.is_none() && both && has_limits(base)) {
                return Err(Unsupported);
            }
            match base.as_ref() {
                Exp::Sym(Class::Func | Class::FuncLimits, name) => leaf(name, style, out),
                other => exp_inlines(other, style, out)?,
            }
            if let Some(sub) = sub {
                out.push(Inline::Subscript(script(sub, style)?));
            }
            if let Some(sup) = sup {
                out.push(Inline::Superscript(script(sup, style)?));
            }
        }
        Exp::Fenced { open, body, close } => {
            if let Some(open) = open {
                leaf(open, style, out);
            }
            list(body, style, out)?;
            if let Some(close) = close {
                leaf(close, style, out);
            }
        }
        Exp::Middle(text) => leaf(text, style, out),
        Exp::Accent { body, mark } => match (single_ident(body), mark.inline) {
            (Some(text), Some(m)) => ident(&format!("{text}{m}"), style, out),
            _ => return Err(Unsupported),
        },
        Exp::Space(space) => {
            if !space.text.is_empty() {
                out.push(str_(space.text));
            }
        }
        Exp::Text(own, text) => {
            if own.is_some_and(|alphabet| alphabet.wrap == Wrap::Code) {
                if !text.is_empty() {
                    out.push(Inline::Code(Attr::default(), text.clone()));
                }
                return Ok(());
            }
            // Each line feed is a `Space`; a space after a `Space` adds none,
            // also after the `Space` of an earlier piece of text.
            let plain = matches!(own.map(|alphabet| alphabet.wrap), None | Some(Wrap::Plain));
            let mut after_space = plain && ends_with_space(out);
            let mut words = Vec::new();
            let mut word = String::new();
            for c in text.chars() {
                if c != ' ' && c != '\n' {
                    word.push(c);
                    after_space = false;
                    continue;
                }
                if !word.is_empty() {
                    words.push(str_(std::mem::take(&mut word)));
                }
                // A line feed after a line feed of the same piece stays.
                if !after_space || (c == '\n' && !words.is_empty()) {
                    words.push(Inline::Space);
                }
                after_space = true;
            }
            if !word.is_empty() {
                words.push(str_(word));
            }
            match own.map(|alphabet| alphabet.wrap) {
                None | Some(Wrap::Plain) => out.append(&mut words),
                Some(_) if words.is_empty() => {}
                Some(kind) => wrap(kind, words, out),
            }
        }
        Exp::Styled(alphabet, items) => list(items, Some(alphabet), out)?,
        Exp::Sized(_, delimiter) => match delimiter.as_ref() {
            Exp::Sym(_, text) => leaf(text, style, out),
            _ => return Err(Unsupported),
        },
        Exp::Frac { .. }
        | Exp::Sqrt(_)
        | Exp::Root(..)
        | Exp::Stack { .. }
        | Exp::Display(..)
        | Exp::Enclose(..)
        | Exp::Phantom(_)
        | Exp::Table { .. } => return Err(Unsupported),
    }
    Ok(())
}

fn ends_with_space(out: &[Inline]) -> bool {
    out.last() == Some(&Inline::Space)
}

/// The content of a script. A function name alone has no space after it.
fn script(exp: &Exp, style: Option<&Alphabet>) -> Result<Vec<Inline>, Unsupported> {
    let mut inner = Vec::new();
    match exp {
        Exp::Sym(Class::Func | Class::FuncLimits, name) => leaf(name, style, &mut inner),
        other => list(std::slice::from_ref(other), style, &mut inner)?,
    }
    Ok(inner)
}

/// The text of a symbol in the inline list. A few arrows carry the variation
/// selector U+FE0E there.
fn inline_text(text: &str) -> &str {
    static FORMS: OnceLock<Vec<(&str, &str)>> = OnceLock::new();
    if text.is_ascii() {
        return text;
    }
    FORMS
        .get_or_init(|| {
            SYMBOLS
                .iter()
                .filter(|(_, _, mathml, inline)| mathml != inline)
                .map(|(_, _, mathml, inline)| (*mathml, *inline))
                .collect()
        })
        .iter()
        .find(|(mathml, _)| *mathml == text)
        .map_or(text, |(_, inline)| inline)
}

/// The text of an identifier of one character, also in a group of one item.
fn single_ident(exp: &Exp) -> Option<&str> {
    match exp {
        Exp::Sym(Class::Ident | Class::IdentUpright, text) if text.chars().count() == 1 => {
            Some(text)
        }
        Exp::Group(items) if items.len() == 1 => items.first().and_then(single_ident),
        _ => None,
    }
}

/// Joins adjacent `Str` elements, in each container too. An empty `Str` goes.
fn merge(items: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(items.len());
    for item in items {
        let item = match item {
            Inline::Emph(xs) => Inline::Emph(merge(xs)),
            Inline::Strong(xs) => Inline::Strong(merge(xs)),
            Inline::Subscript(xs) => Inline::Subscript(merge(xs)),
            Inline::Superscript(xs) => Inline::Superscript(merge(xs)),
            Inline::Str(s) if s.is_empty() => continue,
            other => other,
        };
        if let (Some(Inline::Str(prev)), Inline::Str(next)) = (out.last_mut(), &item) {
            prev.push_str(next);
            continue;
        }
        out.push(item);
    }
    out
}
