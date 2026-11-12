//! The TeX math parser.

use crate::macros;
use crate::render::{variant_char, variant_chars};
use crate::symbols::{CHARS, DELIMITERS, NEGATIONS, SIZED, SYMBOLS};
use crate::tree::{Aligns, Alphabet, Class, Exp, Mark, Row, Space, Wrap};

/// A parse failure: the character offset of the problem in the formula after
/// macro expansion, and its description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParseFailure {
    pub(crate) column: usize,
    pub(crate) message: String,
}

type Parsed<T> = Result<T, ParseFailure>;

/// Where a list of expressions ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    End,
    Brace,
    Bracket,
    Right,
    /// The end of a table cell: `&`, `\\` or `\end`.
    Cell,
    /// The `$` that ends math in text.
    Dollar,
    /// The `\)` that ends math in text.
    MathClose,
    /// The `\rVert` that closes `\lVert`.
    Bars,
}

/// The line that the oracle adds to the message for a control sequence that
/// it does not know.
const EXPECTING_SPACE: &str =
    r#"expecting "%", "\\label", "\\tag", "\\nonumber", whitespace or "\\allowbreak""#;

/// The line that the oracle adds when `\left` has no delimiter.
const EXPECTING_DELIMITER: &str = r#"expecting "%", "\\label", "\\tag", "\\nonumber", whitespace, "\\allowbreak", "\\", ".", "<" or ">""#;

/// The line that the oracle adds when the column description of `array` is
/// missing.
const EXPECTING_COLUMNS: &str = r#"expecting "%", "\\label", "\\tag", "\\nonumber", whitespace, "\\allowbreak", white space or "{""#;

/// The line that the oracle adds for eof in a group after an item.
const EXPECTING_BRACE_AFTER: &str = r#"expecting "\\", "{", "\\operatorname", letter, digit, ".", "!", "'", "''", "'''", "''''", "*", "+", ",", "-", ".", "/", ":", ":=", ";", "<", "=", ">", "?", "@", "~", "_", "^", "'", "\\left", "\\lVert", "\\hyperref", "``", "''", "`", "~", "\\ ", "\\\"", "\\#", "\\$", "\\%", "\\&", "\\'", "\\.", "\\=", "\\AA", "\\AE", "\\L", "\\O", "\\OE", "\\^", "\\_", "\\`", "\\aa", "\\ae", "\\c", "\\char", "\\l", "\\ldots", "\\o", "\\oe", "\\quad", "\\ss", "\\textasciicircum", "\\textasciitilde", "\\textbackslash", "\\u", "\\v", "\\{", "\\}", "\\~", white space or "}""#;
/// The line that the oracle adds for eof in a group with no item.
const EXPECTING_BRACE_START: &str = r#"expecting white space, "%", "\\label", "\\tag", "\\nonumber", whitespace, "\\allowbreak", "\\", "{", "\\operatorname", letter, digit, ".", "!", "'", "''", "'''", "''''", "*", "+", ",", "-", ".", "/", ":", ":=", ";", "<", "=", ">", "?", "@", "~", "_", "^", "'", "\\left", "\\lVert", "\\hyperref", "``", "''", "`", "~", "\\ ", "\\\"", "\\#", "\\$", "\\%", "\\&", "\\'", "\\.", "\\=", "\\AA", "\\AE", "\\L", "\\O", "\\OE", "\\^", "\\_", "\\`", "\\aa", "\\ae", "\\c", "\\char", "\\l", "\\ldots", "\\o", "\\oe", "\\quad", "\\ss", "\\textasciicircum", "\\textasciitilde", "\\textbackslash", "\\u", "\\v", "\\{", "\\}", "\\~" or "}""#;
/// The line that the oracle adds for an unexpected character at the top level after an item.
const EXPECTING_END_AFTER: &str = r#"expecting "\\", "{", "\\operatorname", letter, digit, ".", "!", "'", "''", "'''", "''''", "*", "+", ",", "-", ".", "/", ":", ":=", ";", "<", "=", ">", "?", "@", "~", "_", "^", "'", "\\left", "\\lVert", "\\hyperref", "``", "''", "`", "~", "\\ ", "\\\"", "\\#", "\\$", "\\%", "\\&", "\\'", "\\.", "\\=", "\\AA", "\\AE", "\\L", "\\O", "\\OE", "\\^", "\\_", "\\`", "\\aa", "\\ae", "\\c", "\\char", "\\l", "\\ldots", "\\o", "\\oe", "\\quad", "\\ss", "\\textasciicircum", "\\textasciitilde", "\\textbackslash", "\\u", "\\v", "\\{", "\\}", "\\~" or end of input"#;
/// The line that the oracle adds for an unexpected character at the top level with no item.
const EXPECTING_END_START: &str = r#"expecting "%", "\\label", "\\tag", "\\nonumber", whitespace, "\\allowbreak", "\\", "{", "\\operatorname", letter, digit, ".", "!", "'", "''", "'''", "''''", "*", "+", ",", "-", ".", "/", ":", ":=", ";", "<", "=", ">", "?", "@", "~", "_", "^", "'", "\\left", "\\lVert", "\\hyperref", "``", "''", "`", "~", "\\ ", "\\\"", "\\#", "\\$", "\\%", "\\&", "\\'", "\\.", "\\=", "\\AA", "\\AE", "\\L", "\\O", "\\OE", "\\^", "\\_", "\\`", "\\aa", "\\ae", "\\c", "\\char", "\\l", "\\ldots", "\\o", "\\oe", "\\quad", "\\ss", "\\textasciicircum", "\\textasciitilde", "\\textbackslash", "\\u", "\\v", "\\{", "\\}", "\\~" or end of input"#;
/// The line that the oracle adds for eof in a cell of an environment.
const EXPECTING_CELL: &str = r#"expecting "\\", "\\end", "\\\\", "{", "\\operatorname", letter, digit, ".", "!", "'", "''", "'''", "''''", "*", "+", ",", "-", ".", "/", ":", ":=", ";", "<", "=", ">", "?", "@", "~", "_", "^", "'", "\\left", "\\lVert", "\\hyperref", "``", "''", "`", "~", "\\ ", "\\\"", "\\#", "\\$", "\\%", "\\&", "\\'", "\\.", "\\=", "\\AA", "\\AE", "\\L", "\\O", "\\OE", "\\^", "\\_", "\\`", "\\aa", "\\ae", "\\c", "\\char", "\\l", "\\ldots", "\\o", "\\oe", "\\quad", "\\ss", "\\textasciicircum", "\\textasciitilde", "\\textbackslash", "\\u", "\\v", "\\{", "\\}", "\\~", "&" or white space"#;
/// The line that the oracle adds for an unexpected character in math in text.
const EXPECTING_DOLLAR: &str = r#"expecting "\\", "{", "\\operatorname", letter, digit, ".", "!", "'", "''", "'''", "''''", "*", "+", ",", "-", ".", "/", ":", ":=", ";", "<", "=", ">", "?", "@", "~", "_", "^", "'", "\\left", "\\lVert", "\\hyperref", "``", "''", "`", "~", "\\ ", "\\\"", "\\#", "\\$", "\\%", "\\&", "\\'", "\\.", "\\=", "\\AA", "\\AE", "\\L", "\\O", "\\OE", "\\^", "\\_", "\\`", "\\aa", "\\ae", "\\c", "\\char", "\\l", "\\ldots", "\\o", "\\oe", "\\quad", "\\ss", "\\textasciicircum", "\\textasciitilde", "\\textbackslash", "\\u", "\\v", "\\{", "\\}", "\\~" or "$""#;
/// The line that the oracle adds for eof in brackets.
const EXPECTING_BRACKET: &str = r#"expecting "\\", "]", "{", "\\operatorname", letter, digit, ".", "!", "'", "''", "'''", "''''", "*", "+", ",", "-", ".", "/", ":", ":=", ";", "<", "=", ">", "?", "@", "~", "_", "^", "'", "\\left", "\\lVert", "\\hyperref", "``", "''", "`", "~", "\\ ", "\\\"", "\\#", "\\$", "\\%", "\\&", "\\'", "\\.", "\\=", "\\AA", "\\AE", "\\L", "\\O", "\\OE", "\\^", "\\_", "\\`", "\\aa", "\\ae", "\\c", "\\char", "\\l", "\\ldots", "\\o", "\\oe", "\\quad", "\\ss", "\\textasciicircum", "\\textasciitilde", "\\textbackslash", "\\u", "\\v", "\\{", "\\}", "\\~" or white space"#;
/// The line that the oracle adds for eof where a command needs an argument.
const EXPECTING_ARGUMENT: &str = r#"expecting "%", "\\label", "\\tag", "\\nonumber", whitespace, "\\allowbreak", "!", "'", "''", "'''", "''''", "*", "+", ",", "-", ".", "/", ":", ":=", ";", "<", "=", ">", "?", "@", "~", "\\" or "{""#;
/// The line that the oracle adds for eof where a script needs its argument.
const EXPECTING_SCRIPT: &str = r#"expecting "%", "\\label", "\\tag", "\\nonumber", whitespace, "\\allowbreak", "{", letter, digit, ".", "!", "'", "''", "'''", "''''", "*", "+", ",", "-", ".", "/", ":", ":=", ";", "<", "=", ">", "?", "@", "~", "_", "^", "'", "\\left", "\\lVert", "\\", "\\hyperref", "``", "''", "`", "~", "\\ ", "\\\"", "\\#", "\\$", "\\%", "\\&", "\\'", "\\.", "\\=", "\\AA", "\\AE", "\\L", "\\O", "\\OE", "\\^", "\\_", "\\`", "\\aa", "\\ae", "\\c", "\\char", "\\l", "\\ldots", "\\o", "\\oe", "\\quad", "\\ss", "\\textasciicircum", "\\textasciitilde", "\\textbackslash", "\\u", "\\v", "\\{", "\\}", "\\~", white space or "\\operatorname""#;
/// The line that the oracle adds for eof in text.
const EXPECTING_TEXT: &str = r#"expecting text, "}", "\\", "{", "$", "$$", "\\(" or "\\[""#;

/// The line that the oracle adds for the end of a list right after a style
/// command.
const EXPECTING_STYLE: &str = r#"expecting letter, "%", "\\label", "\\tag", "\\nonumber", whitespace, "\\allowbreak", "{", "\\operatorname", digit, ".", "!", "'", "''", "'''", "''''", "*", "+", ",", "-", ".", "/", ":", ":=", ";", "<", "=", ">", "?", "@", "~", "_", "^", "'", "\\left", "\\lVert", "\\", "\\hyperref", "``", "''", "`", "~" or "\\ ", "\\\"", "\\#", "\\$", "\\%", "\\&", "\\'", "\\.", "\\=", "\\AA", "\\AE", "\\L", "\\O", "\\OE", "\\^", "\\_", "\\`", "\\aa", "\\ae", "\\c", "\\char", "\\l", "\\ldots", "\\o", "\\oe", "\\quad", "\\ss", "\\textasciicircum", "\\textasciitilde", "\\textbackslash", "\\u", "\\v", "\\{", "\\}", "\\~""#;

/// The line that the oracle adds for an environment that it does not know.
const EXPECTING_ENVIRONMENT: &str = r#"expecting white space or "Bmatrix", "Vmatrix", "align", "alignat", "aligned", "alignedat", "array", "bmatrix", "cases", "eqnarray", "equation", "flalign", "flaligned", "gather", "gathered", "matrix", "multline", "multlined", "pmatrix", "smallmatrix", "split", "vmatrix""#;

const fn space(width: &'static str, text: &'static str) -> Space {
    Space { width, text }
}

const THIN: Space = space("0.167em", "\u{2006}");
const MEDIUM: Space = space("0.222em", "\u{a0}");
const THICK: Space = space("0.278em", "\u{2005}");
const NEGATIVE: Space = space("-0.167em", "\u{200a}");
/// The space that the oracle puts next to text with a space at its edge.
const TEXT_EDGE: Space = space("0.333em", "");

const SPACES: &[(&str, Space)] = &[
    (" ", MEDIUM),
    ("!", NEGATIVE),
    (",", THIN),
    (":", MEDIUM),
    (";", THICK),
    (">", MEDIUM),
    ("enspace", space("0.5em", "\u{2000}")),
    ("qquad", space("2.0em", "\u{200a}\u{2001}\u{2001}")),
    ("quad", space("1.0em", "\u{200a}\u{2001}")),
];

const fn mark(mathml: &'static str, inline: Option<&'static str>, under: bool) -> Mark {
    Mark {
        mathml,
        inline,
        under,
    }
}

const ACCENTS: &[(&str, Mark)] = &[
    ("acute", mark("\u{301}", Some("\u{301}"), false)),
    ("bar", mark("\u{203e}", Some("\u{304}"), false)),
    ("breve", mark("\u{306}", Some("\u{306}"), false)),
    ("check", mark("\u{30c}", Some("\u{30c}"), false)),
    ("Dot", mark("\u{307}", Some("\u{307}"), false)),
    ("Vec", mark("\u{2192}", Some("\u{20d7}"), false)),
    ("ddddot", mark("\u{20dc}", None, false)),
    ("dddot", mark("\u{20db}", None, false)),
    ("ddot", mark("\u{308}", None, false)),
    ("dot", mark("\u{307}", Some("\u{307}"), false)),
    ("grave", mark("\u{300}", Some("\u{300}"), false)),
    ("hat", mark("\u{302}", Some("\u{302}"), false)),
    ("mathring", mark("\u{30a}", Some("\u{30a}"), false)),
    ("overbrace", mark("\u{23de}", None, false)),
    ("overleftarrow", mark("\u{20d6}", Some("\u{20d6}"), false)),
    ("overleftrightarrow", mark("\u{20e1}", None, false)),
    ("overbracket", mark("\u{23b4}", None, false)),
    ("overline", mark("\u{af}", None, false)),
    ("overparen", mark("\u{23dc}", None, false)),
    ("overrightarrow", mark("\u{2192}", Some("\u{20d7}"), false)),
    ("tilde", mark("\u{303}", Some("\u{303}"), false)),
    ("underbrace", mark("\u{23df}", None, true)),
    ("underbracket", mark("\u{23b5}", None, true)),
    ("underleftarrow", mark("\u{20ee}", None, false)),
    ("underline", mark("_", None, true)),
    ("underparen", mark("\u{23dd}", None, true)),
    ("underrightarrow", mark("\u{20ef}", None, false)),
    ("utilde", mark("\u{330}", None, false)),
    ("vec", mark("\u{2192}", Some("\u{20d7}"), false)),
    ("widehat", mark("\u{302}", Some("\u{302}"), false)),
    ("widetilde", mark("\u{303}", Some("\u{303}"), false)),
];

const fn alphabet(
    ident: &'static str,
    other: &'static str,
    wrap: Wrap,
    inline_chars: bool,
) -> Alphabet {
    Alphabet {
        ident,
        other,
        wrap,
        inline_chars,
    }
}

const NORMAL: Alphabet = alphabet("normal", "normal", Wrap::Plain, false);
const BOLD: Alphabet = alphabet("bold-italic", "bold", Wrap::Strong, false);
const SCRIPT: Alphabet = alphabet("script", "script", Wrap::Plain, true);

/// The math alphabets, sorted by name.
static ALPHABETS: &[(&str, Alphabet)] = &[
    ("bm", BOLD),
    ("boldsymbol", BOLD),
    (
        "mathbb",
        alphabet("double-struck", "double-struck", Wrap::Plain, true),
    ),
    ("mathbf", BOLD),
    (
        "mathbfcal",
        alphabet("bold-script", "bold-script", Wrap::Strong, true),
    ),
    (
        "mathbffrak",
        alphabet("bold-fraktur", "bold-fraktur", Wrap::Strong, true),
    ),
    (
        "mathbfit",
        alphabet("bold-italic", "bold-italic", Wrap::StrongEmph, false),
    ),
    ("mathcal", SCRIPT),
    (
        "mathfrak",
        alphabet("fraktur", "fraktur", Wrap::Plain, true),
    ),
    ("mathit", alphabet("italic", "italic", Wrap::Emph, false)),
    ("mathrm", NORMAL),
    ("mathscr", SCRIPT),
    (
        "mathsf",
        alphabet("sans-serif", "sans-serif", Wrap::Plain, false),
    ),
    (
        "mathsfit",
        alphabet("sans-serif-italic", "sans-serif-italic", Wrap::Emph, false),
    ),
    (
        "mathtt",
        alphabet("monospace", "monospace", Wrap::Code, false),
    ),
    ("mathup", NORMAL),
    ("pmb", BOLD),
];

/// The text commands that set a style, sorted by name. `\text`, `\textrm`
/// and `\mbox` set no style.
static TEXT_STYLES: &[(&str, Alphabet)] = &[
    ("textbf", BOLD),
    ("textit", alphabet("italic", "italic", Wrap::Emph, false)),
    (
        "textsf",
        alphabet("sans-serif", "sans-serif", Wrap::Plain, false),
    ),
    (
        "texttt",
        alphabet("monospace", "monospace", Wrap::Code, false),
    ),
];

/// The size commands: the name without `l` or `r` at its end, and the size.
const SIZES: &[(&str, &str)] = &[
    ("Big", "180%"),
    ("Bigg", "300%"),
    ("big", "120%"),
    ("bigg", "240%"),
];

/// The delimiters and the column alignment of an environment.
struct Environment {
    open: Option<&'static str>,
    close: Option<&'static str>,
    aligns: Aligns,
}

fn environment(name: &str) -> Option<Environment> {
    let (open, close, aligns) = match name {
        "matrix" | "smallmatrix" | "gather" | "gathered" | "multline" => {
            (None, None, Aligns::All("center"))
        }
        "pmatrix" => (Some("("), Some(")"), Aligns::All("center")),
        "bmatrix" => (Some("["), Some("]"), Aligns::All("center")),
        "Bmatrix" => (Some("{"), Some("}"), Aligns::All("center")),
        "vmatrix" => (Some("\u{2223}"), Some("\u{2223}"), Aligns::All("center")),
        "Vmatrix" => (Some("\u{2225}"), Some("\u{2225}"), Aligns::All("center")),
        "cases" => (Some("{"), None, Aligns::FirstRow("left")),
        "aligned" | "align" | "split" | "alignat" | "alignedat" => (None, None, Aligns::Pairs),
        "eqnarray" => (None, None, Aligns::Cycle(&["right", "center", "left"])),
        "flalign" => (None, None, Aligns::Cycle(&["left", "right"])),
        _ => return None,
    };
    Some(Environment {
        open,
        close,
        aligns,
    })
}

/// An infix fraction command, for example `\\over`.
struct Infix {
    name: &'static str,
    /// The delimiters around the fraction.
    fence: Option<(&'static str, &'static str)>,
    /// The fraction has a line.
    line: bool,
}

const fn infix(
    name: &'static str,
    fence: Option<(&'static str, &'static str)>,
    line: bool,
) -> Infix {
    Infix { name, fence, line }
}

static INFIX: &[Infix] = &[
    infix("over", None, true),
    infix("atop", None, false),
    infix("choose", Some(("(", ")")), false),
    infix("brace", Some(("{", "}")), false),
    infix("brack", Some(("[", "]")), false),
];

/// The style commands. In a list they change nothing in the output.
const STYLES: &[&str] = &[
    "displaystyle",
    "scriptscriptstyle",
    "scriptstyle",
    "textstyle",
];

/// Function names that take their scripts above and below in display math.
const LIMIT_FUNCS: &[&str] = &[
    "Pr", "det", "gcd", "inf", "lim", "liminf", "limsup", "max", "min", "sup",
];

/// The nesting levels that one command counts. The function that reads a
/// command uses much more stack than the function that reads a group. With
/// this number, the deepest nesting at the default limit of 128 levels fits
/// in a stack of 1 MB in a debug build.
const COMMAND_LEVELS: usize = 4;

#[cfg(test)]
thread_local! {
    /// The number of characters that the parser read on this thread: the
    /// step count of the time tests.
    pub(crate) static PEEKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Parses a TeX math formula. Nesting deeper than `max_depth` is a failure.
pub(crate) fn parse(tex: &str, max_depth: usize) -> Parsed<Vec<Exp>> {
    let mut parser = Parser {
        chars: macros::expand(tex),
        pos: 0,
        depth: 0,
        max_depth,
        paired_bars: None,
    };
    parser.list(Stop::End)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    depth: usize,
    max_depth: usize,
    /// The place of each `\lVert` that has its `\rVert`, in rising order.
    /// The parser finds the places when it reads the first `\lVert`.
    paired_bars: Option<Vec<usize>>,
}

/// Reports whether `chars` holds `\NAME` at `at`, and no more letters.
fn word_at(chars: &[char], at: usize, name: &str) -> bool {
    let mut i = at + 1;
    for c in name.chars() {
        if chars.get(i) != Some(&c) {
            return false;
        }
        i += 1;
    }
    !chars.get(i).is_some_and(char::is_ascii_alphabetic)
}

/// The place of each `\lVert` that has its `\rVert` in the same group.
fn paired_bars(chars: &[char]) -> Vec<usize> {
    let mut open: Vec<(usize, usize)> = Vec::new();
    let mut paired = Vec::new();
    let mut level = 0usize;
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        match c {
            '{' => level += 1,
            '}' => {
                level = level.saturating_sub(1);
                while open.last().is_some_and(|(_, at)| *at > level) {
                    open.pop();
                }
            }
            '%' => {
                while chars.get(i).is_some_and(|c| *c != '\n') {
                    i += 1;
                }
            }
            '\\' if word_at(chars, i, "lVert") => open.push((i, level)),
            '\\' if word_at(chars, i, "rVert") => {
                if open.last().is_some_and(|(_, at)| *at == level)
                    && let Some((place, _)) = open.pop()
                {
                    paired.push(place);
                }
            }
            '\\' => i += 1,
            _ => {}
        }
        i += 1;
    }
    paired.sort_unstable();
    paired
}

/// The items of the text of an operator name at one group level.
struct TextShape {
    items: usize,
    /// A group of other than one item stands beside other items.
    grouped: bool,
}

/// Reports whether the oracle reads `c` as white space: the line and
/// paragraph separators and U+0085 are not white space there.
pub(crate) fn is_tex_space(c: char) -> bool {
    c.is_whitespace() && !matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// A character as the oracle shows it in a message.
fn quoted(c: char) -> String {
    match c {
        '\\' => "\"\\\\\"".to_string(),
        '"' => "\"\\\"\"".to_string(),
        c => format!("\"{c}\""),
    }
}

/// The class and the text of the delimiter command `name`, when the sorted
/// list `accepted` holds it.
fn delimiter_command(name: &str, accepted: &[&str]) -> Option<(Class, &'static str)> {
    match name {
        "{" => Some((Class::Open, "{")),
        "}" => Some((Class::Close, "}")),
        "|" => Some((Class::Close, "\u{2225}")),
        _ => accepted
            .binary_search(&name)
            .ok()
            .and_then(|_| symbol(name)),
    }
}

fn symbol(name: &str) -> Option<(Class, &'static str)> {
    let i = SYMBOLS
        .binary_search_by(|(n, _, _, _)| (*n).cmp(name))
        .ok()?;
    let (_, class, text, _) = SYMBOLS.get(i)?;
    let class = match class {
        Class::Func if LIMIT_FUNCS.contains(&name) => Class::FuncLimits,
        other => *other,
    };
    Some((class, text))
}

fn lookup<T: 'static>(table: &'static [(&str, T)], name: &str) -> Option<&'static T> {
    table.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
}

fn sym(class: Class, text: &str) -> Exp {
    Exp::Sym(class, text.to_string())
}

fn boxed(exp: Exp) -> Box<Exp> {
    Box::new(exp)
}

fn primes(count: usize) -> String {
    let mut text = "\u{2057}".repeat(count / 4);
    text.push_str(match count % 4 {
        1 => "\u{2032}",
        2 => "\u{2033}",
        3 => "\u{2034}",
        _ => "",
    });
    text
}

impl Parser {
    fn fail<T>(&self, message: impl Into<String>) -> Parsed<T> {
        Err(ParseFailure {
            column: self.pos,
            message: message.into(),
        })
    }

    fn unexpected<T>(&self) -> Parsed<T> {
        match self.peek() {
            None => self.fail("unexpected eof"),
            Some(c) => self.fail(format!("unexpected '{c}'")),
        }
    }

    fn text(&self, start: usize) -> String {
        self.chars
            .get(start..self.pos)
            .map(|cs| cs.iter().collect())
            .unwrap_or_default()
    }

    fn peek(&self) -> Option<char> {
        #[cfg(test)]
        PEEKS.with(|peeks| peeks.set(peeks.get() + 1));
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        #[cfg(test)]
        PEEKS.with(|peeks| peeks.set(peeks.get() + 1));
        self.chars.get(self.pos + offset).copied()
    }

    /// Skips white space, comments and the commands that the oracle reads as
    /// white space.
    fn skip_space(&mut self) {
        loop {
            match self.peek() {
                Some(c) if is_tex_space(c) => self.pos += 1,
                Some('%') => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.pos += 1;
                    }
                }
                Some('\\') if self.at_word("nonumber") => self.pos += 9,
                Some('\\') if self.at_word("allowbreak") => self.pos += 11,
                Some('\\') if self.at_word("label") || self.at_word("tag") => {
                    let start = self.pos;
                    self.pos += if self.at_word("tag") { 4 } else { 6 };
                    if self.raw_group().is_err() {
                        self.pos = start;
                        return;
                    }
                }
                _ => return,
            }
        }
    }

    fn enter(&mut self) -> Parsed<()> {
        if self.depth >= self.max_depth {
            return self.fail("maximum nesting depth exceeded");
        }
        self.depth += 1;
        Ok(())
    }

    /// Reports whether the input at the cursor is `\NAME` and no more letters.
    fn at_word(&self, name: &str) -> bool {
        word_at(&self.chars, self.pos, name)
    }

    /// The length of the style command at the cursor, with its backslash.
    fn style_command(&self) -> Option<usize> {
        STYLES
            .iter()
            .find(|name| self.at_word(name))
            .map(|name| name.len() + 1)
    }

    /// Reads a style command at the cursor in a list. The oracle writes no
    /// element for it, and the rest of the list is read as if it were not
    /// there. A second style command, or the end of the list, directly after
    /// it is a failure. `false`: no style command is at the cursor.
    fn style_switch(&mut self, stop: Stop) -> Parsed<bool> {
        let Some(len) = self.style_command() else {
            return Ok(false);
        };
        self.pos += len;
        let after_name = self.pos;
        self.skip_space();
        if let Some(len) = self.style_command() {
            let name = self
                .chars
                .get(self.pos + 1..self.pos + len)
                .unwrap_or_default();
            let name: String = name.iter().collect();
            self.pos += len;
            return self.unknown(&name);
        }
        if self.at_list_end(stop) {
            return match self.peek() {
                _ if self.pos != after_name => self.unexpected(),
                None => self.fail(format!("unexpected eof\n  {EXPECTING_STYLE}")),
                Some(c) => self.fail(format!("unexpected \"{c}\"\n  {EXPECTING_STYLE}")),
            };
        }
        Ok(true)
    }

    /// Reports whether the list that ends at `stop` ends at the cursor, or
    /// an infix fraction command comes.
    fn at_list_end(&self, stop: Stop) -> bool {
        let row_end = self.peek_at(1) == Some('\\');
        match self.peek() {
            None | Some('}' | '&') => true,
            Some(']') => stop == Stop::Bracket,
            Some('$') => stop == Stop::Dollar,
            Some('\\') => {
                self.infix().is_some()
                    || (stop == Stop::Cell && (row_end || self.at_word("end")))
                    || (stop == Stop::Right && (self.at_word("right") || self.at_word("middle")))
                    || (stop == Stop::MathClose && self.peek_at(1) == Some(')'))
                    || (stop == Stop::Bars && self.at_word("rVert"))
            }
            Some(_) => false,
        }
    }

    /// The infix fraction command at the cursor.
    fn infix(&self) -> Option<&'static Infix> {
        INFIX.iter().find(|infix| self.at_word(infix.name))
    }

    fn list(&mut self, stop: Stop) -> Parsed<Vec<Exp>> {
        self.enter()?;
        let mut items = Vec::new();
        let mut over = None;
        loop {
            self.skip_space();
            match self.peek() {
                None if matches!(stop, Stop::End | Stop::Right) => break,
                None => {
                    let start = items.is_empty() && over.is_none();
                    let expecting = match stop {
                        Stop::Brace if start => EXPECTING_BRACE_START,
                        Stop::Brace => EXPECTING_BRACE_AFTER,
                        Stop::Bracket => EXPECTING_BRACKET,
                        Stop::Cell => EXPECTING_CELL,
                        _ => return self.fail("unexpected eof"),
                    };
                    return self.fail(format!("unexpected eof\n  {expecting}"));
                }
                Some('}') if stop == Stop::Brace => {
                    self.pos += 1;
                    break;
                }
                Some(']') if stop == Stop::Bracket => {
                    self.pos += 1;
                    break;
                }
                Some('$') if stop == Stop::Dollar => {
                    self.pos += 1;
                    break;
                }
                Some('&') if stop == Stop::Cell => break,
                Some(c @ ('}' | '&' | '$' | '#' | '"')) => {
                    let start = items.is_empty() && over.is_none();
                    return match stop {
                        Stop::End if start => {
                            self.fail(format!("unexpected '{c}'\n  {EXPECTING_END_START}"))
                        }
                        Stop::End => {
                            self.fail(format!("unexpected '{c}'\n  {EXPECTING_END_AFTER}"))
                        }
                        Stop::Dollar => {
                            self.fail(format!("unexpected \"{c}\"\n  {EXPECTING_DOLLAR}"))
                        }
                        _ => self.unexpected(),
                    };
                }
                Some('\\') => {
                    if stop == Stop::Cell && (self.peek_at(1) == Some('\\') || self.at_word("end"))
                    {
                        break;
                    }
                    if stop == Stop::Right && self.at_word("right") {
                        break;
                    }
                    if stop == Stop::Bars && self.at_word("rVert") {
                        self.pos += 6;
                        break;
                    }
                    if stop == Stop::MathClose && self.peek_at(1) == Some(')') {
                        self.pos += 2;
                        break;
                    }
                    if stop == Stop::Cell && self.at_word("hline") {
                        self.pos += 6;
                    } else if stop == Stop::Right && self.at_word("middle") {
                        self.pos += 7;
                        let after = self.pos;
                        let Some(delimiter) = self.delimiter(false)? else {
                            self.pos = after;
                            return self.unknown("middle");
                        };
                        items.push(Exp::Middle(delimiter.unwrap_or_default()));
                    } else if self.style_switch(stop)? {
                        continue;
                    } else if let Some(infix) = self.infix() {
                        if over.is_some() {
                            return self.fail("unexpected control sequence");
                        }
                        self.pos += infix.name.len() + 1;
                        over = Some((std::mem::take(&mut items), infix));
                    } else {
                        items.push(self.scripted()?);
                    }
                }
                Some(_) => items.push(self.scripted()?),
            }
        }
        self.depth -= 1;
        let Some((num, infix)) = over else {
            // A list of one group is the list of the group.
            return Ok(match items.pop() {
                Some(Exp::Group(inner)) if items.is_empty() => inner,
                Some(other) => {
                    items.push(other);
                    items
                }
                None => items,
            });
        };
        let frac = Exp::Frac {
            num: boxed(single(num)),
            den: boxed(single(items)),
            line: infix.line,
        };
        Ok(vec![match infix.fence {
            None => frac,
            Some((open, close)) => Exp::Fenced {
                open: Some(open.to_string()),
                body: vec![frac],
                close: Some(close.to_string()),
            },
        }])
    }

    fn scripted(&mut self) -> Parsed<Exp> {
        self.enter()?;
        let base = match self.peek() {
            Some('^' | '_') => sym(Class::Ord, ""),
            _ => self.atom()?,
        };
        let mut limits = None;
        self.skip_space();
        if self.peek() == Some('\\') && (self.at_word("limits") || self.at_word("nolimits")) {
            let on = self.at_word("limits");
            self.pos += if on { 7 } else { 9 };
            limits = Some(on);
            self.skip_space();
        }
        let (mut sub, mut sup) = (None, None);
        match self.peek() {
            Some('\'') => {
                sup = Some(self.prime_marks());
                self.skip_space();
                if self.peek() == Some('_') {
                    self.pos += 1;
                    sub = Some(self.script_argument()?);
                }
            }
            Some(first @ ('^' | '_')) => {
                self.pos += 1;
                let arg = self.script_argument()?;
                self.skip_space();
                let (same, other) = if first == '^' {
                    (&mut sup, &mut sub)
                } else {
                    (&mut sub, &mut sup)
                };
                match self.peek() {
                    Some('\'') if first == '_' => {
                        *same = Some(arg);
                        *other = Some(self.prime_marks());
                    }
                    Some(c @ ('^' | '_')) if c != first => {
                        self.pos += 1;
                        *same = Some(arg);
                        *other = Some(self.script_argument()?);
                    }
                    _ => *same = Some(self.nested_script(arg, first)?),
                }
            }
            _ => {}
        }
        self.depth -= 1;
        if sub.is_none() && sup.is_none() {
            return Ok(base);
        }
        Ok(Exp::Scripts {
            base: boxed(base),
            sub: sub.map(boxed),
            sup: sup.map(boxed),
            limits,
        })
    }

    /// The prime marks at the cursor, as the superscript that they make.
    fn prime_marks(&mut self) -> Exp {
        let start = self.pos;
        while self.peek() == Some('\'') {
            self.pos += 1;
        }
        Exp::Sym(Class::Op, primes(self.pos - start))
    }

    /// The argument of a script character: a group, a number, one atom with
    /// its own arguments, or scripts with no base.
    fn script_argument(&mut self) -> Parsed<Exp> {
        self.skip_space();
        if self.peek().is_none() {
            return self.fail(format!("unexpected eof\n  {EXPECTING_SCRIPT}"));
        }
        let decimal =
            self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit());
        match self.peek() {
            Some(c) if c.is_ascii_digit() || decimal => Ok(self.number()),
            Some('^' | '_') => self.scripted(),
            _ => self.expression(),
        }
    }

    /// The single script `arg` of the kind `kind` with the scripts of the
    /// same kind after it: `x^2^3` is `x^{2^3}`, and `x^2'` is `x^{2'}`.
    fn nested_script(&mut self, arg: Exp, kind: char) -> Parsed<Exp> {
        self.enter()?;
        self.skip_space();
        let scripts = match self.peek() {
            Some(c) if c == kind => {
                self.pos += 1;
                let next = self.script_argument()?;
                self.skip_space();
                let deeper =
                    self.peek() == Some(kind) || (kind == '^' && self.peek() == Some('\''));
                // The script of the other kind after the chain belongs to
                // the innermost level: `x_a_b^2` is `x_{a_b^2}`.
                let (inner, other) = if deeper {
                    (self.nested_script(next, kind)?, None)
                } else {
                    (next, self.other_script(kind)?)
                };
                Some((inner, other))
            }
            Some('\'') if kind == '^' => {
                let marks = self.prime_marks();
                self.skip_space();
                Some((marks, self.other_script(kind)?))
            }
            _ => None,
        };
        self.depth -= 1;
        let Some((inner, other)) = scripts else {
            return Ok(arg);
        };
        let (sub, sup) = if kind == '^' {
            (other, Some(inner))
        } else {
            (Some(inner), other)
        };
        Ok(Exp::Scripts {
            base: boxed(arg),
            sub: sub.map(boxed),
            sup: sup.map(boxed),
            limits: None,
        })
    }

    /// The script of the kind that is not `kind` at the cursor, if any.
    fn other_script(&mut self, kind: char) -> Parsed<Option<Exp>> {
        Ok(match (kind, self.peek()) {
            ('^', Some('_')) | ('_', Some('^')) => {
                self.pos += 1;
                Some(self.script_argument()?)
            }
            ('_', Some('\'')) => Some(self.prime_marks()),
            _ => None,
        })
    }

    /// One argument of a command: a group, one digit, one character or one
    /// control sequence that takes no argument.
    fn argument(&mut self) -> Parsed<Exp> {
        self.skip_space();
        let start = self.pos;
        if self.peek().is_none() {
            // Right after a command name the oracle names the command.
            let letters = self.chars[..start]
                .iter()
                .rev()
                .take_while(|c| c.is_ascii_alphabetic())
                .count();
            let name_start = start - letters;
            let found =
                if letters > 0 && name_start > 0 && self.chars.get(name_start - 1) == Some(&'\\') {
                    let name: String = self.chars[name_start..start].iter().collect();
                    format!("unexpected control sequence \\{name}")
                } else {
                    "unexpected eof".to_string()
                };
            return self.fail(format!("{found}\n  {EXPECTING_ARGUMENT}"));
        }
        let exp = self.expression()?;
        if self.chars.get(start) == Some(&'\\') {
            let mut end = start + 1;
            while self.chars.get(end).is_some_and(char::is_ascii_alphabetic) {
                end += 1;
            }
            if end > start + 1 && self.pos != end {
                let name: String = self
                    .chars
                    .get(start + 1..end)
                    .unwrap_or_default()
                    .iter()
                    .collect();
                self.pos = end;
                return self.unknown(&name);
            }
        }
        Ok(exp)
    }

    /// A group, one digit or one atom with its own arguments.
    fn expression(&mut self) -> Parsed<Exp> {
        self.skip_space();
        match self.peek() {
            None => self.fail("unexpected eof"),
            Some('{') => {
                self.pos += 1;
                Ok(single(self.list(Stop::Brace)?))
            }
            Some('^' | '_' | '}' | '&') => self.unexpected(),
            Some(c) if c.is_ascii_digit() => {
                self.pos += 1;
                Ok(Exp::Num(c.to_string()))
            }
            // One argument is one character: `.5` is a point and a digit.
            Some('.') => {
                self.pos += 1;
                Ok(sym(Class::Ord, "."))
            }
            Some(_) => self.atom(),
        }
    }

    /// The text of a group in braces, without parsing.
    fn raw_group(&mut self) -> Parsed<String> {
        self.skip_plain_space();
        if self.peek() != Some('{') {
            return self.unexpected();
        }
        self.pos += 1;
        let mut level = 0usize;
        let start = self.pos;
        loop {
            match self.peek() {
                None => return self.fail("unexpected eof"),
                Some('}') if level == 0 => {
                    let text = self.text(start);
                    self.pos += 1;
                    return Ok(text);
                }
                Some(c) => {
                    if c == '{' {
                        level += 1;
                    } else if c == '}' {
                        level -= 1;
                    } else if c == '\\' {
                        self.pos += 1;
                    }
                    self.pos += 1;
                }
            }
        }
    }

    fn skip_plain_space(&mut self) {
        while self.peek().is_some_and(is_tex_space) {
            self.pos += 1;
        }
    }

    fn atom(&mut self) -> Parsed<Exp> {
        let Some(c) = self.peek() else {
            return self.fail("unexpected eof");
        };
        if c.is_ascii_digit() || (c == '.' && self.peek_at(1).is_some_and(|d| d.is_ascii_digit())) {
            return Ok(self.number());
        }
        if c == '\'' {
            let start = self.pos;
            while self.peek() == Some('\'') {
                self.pos += 1;
            }
            return Ok(Exp::Sym(Class::Ord, primes(self.pos - start)));
        }
        self.pos += 1;
        let exp = match c {
            '{' => single(self.list(Stop::Brace)?),
            '\\' => return self.command(),
            '~' => Exp::Space(MEDIUM),
            '+' => sym(Class::Bin, "+"),
            '-' => sym(Class::Bin, "\u{2212}"),
            '*' => sym(Class::Bin, "*"),
            ':' if self.peek() == Some('=') => {
                self.pos += 1;
                sym(Class::Rel, ":=")
            }
            '=' | '<' | '>' | ':' => Exp::Sym(Class::Rel, c.to_string()),
            ',' | ';' => Exp::Sym(Class::Punct, c.to_string()),
            '(' | '[' | '|' => Exp::Sym(Class::Open, c.to_string()),
            ')' | ']' => Exp::Sym(Class::Close, c.to_string()),
            '.' | '/' | '!' | '?' | '@' => Exp::Sym(Class::Ord, c.to_string()),
            '`' => Exp::Text(None, "\u{2018}".into()),
            c if !c.is_ascii() => match char_class(c) {
                Some(class) => Exp::Sym(class, c.to_string()),
                None => {
                    self.pos -= 1;
                    return self.fail(format!("the character '{c}' has no conversion"));
                }
            },
            c if c.is_ascii_alphabetic() => Exp::Sym(Class::Ident, c.to_string()),
            _ => {
                self.pos -= 1;
                return self.unexpected();
            }
        };
        Ok(exp)
    }

    fn number(&mut self) -> Exp {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        Exp::Num(self.text(start))
    }

    /// The name of the control sequence after its backslash.
    fn control_name(&mut self) -> Parsed<String> {
        let start = self.pos;
        match self.peek() {
            None => self.fail("unexpected eof"),
            Some(c) if c.is_ascii_alphabetic() => {
                while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                    self.pos += 1;
                }
                Ok(self.text(start))
            }
            Some(c) => {
                self.pos += 1;
                Ok(c.to_string())
            }
        }
    }

    /// The failure for a control sequence that the oracle does not know. The
    /// oracle reports it after the white space that follows the name.
    fn unknown<T>(&mut self, name: &str) -> Parsed<T> {
        self.skip_plain_space();
        self.fail(format!(
            "unexpected control sequence \\{name}\n  {EXPECTING_SPACE}"
        ))
    }

    /// A command after its backslash.
    fn command(&mut self) -> Parsed<Exp> {
        let name = self.control_name()?;
        let after_name = self.pos;
        if self.depth + COMMAND_LEVELS > self.max_depth {
            return self.fail("maximum nesting depth exceeded");
        }
        self.depth += COMMAND_LEVELS;
        let result = self.known_command(&name);
        self.depth -= COMMAND_LEVELS;
        match result {
            Ok(Some(exp)) => Ok(exp),
            Ok(None) => {
                self.pos = after_name;
                self.unknown(&name)
            }
            Err(failure) => Err(failure),
        }
    }

    /// The expression of the command `name`. `None`: the oracle does not
    /// know the command.
    fn known_command(&mut self, name: &str) -> Parsed<Option<Exp>> {
        if let Some(space) = lookup(SPACES, name) {
            return Ok(Some(Exp::Space(*space)));
        }
        if let Some(mark) = lookup(ACCENTS, name) {
            let body = boxed(self.argument()?);
            return Ok(Some(Exp::Accent { body, mark }));
        }
        if let Some(alphabet) = lookup(ALPHABETS, name) {
            let arg = self.argument()?;
            return Ok(Some(Exp::Styled(alphabet, flatten(arg))));
        }
        if let Some(percent) = self.size(name) {
            return self.sized(percent);
        }
        let exp = match name {
            "{" => sym(Class::Open, "{"),
            "}" => sym(Class::Close, "}"),
            "|" => sym(Class::Close, "\u{2225}"),
            "$" | "%" | "#" | "&" | "_" | "^" => sym(Class::Ord, name),
            "frac" => self.fraction(true)?,
            "dfrac" => Exp::Display(true, boxed(self.fraction(true)?)),
            "tfrac" => Exp::Display(false, boxed(self.fraction(true)?)),
            "binom" => Exp::Fenced {
                open: Some("(".into()),
                body: vec![self.fraction(false)?],
                close: Some(")".into()),
            },
            "genfrac" => self.genfrac()?,
            "sqrt" => {
                self.skip_space();
                if self.peek() == Some('[') {
                    self.pos += 1;
                    let index = self.list(Stop::Bracket)?;
                    let body = self.argument()?;
                    Exp::Root(boxed(body), boxed(single(index)))
                } else if self.peek().is_none() {
                    sym(Class::Op, "\u{221a}")
                } else {
                    Exp::Sqrt(boxed(self.argument()?))
                }
            }
            "surd" => Exp::Sqrt(boxed(self.argument()?)),
            "left" => match self.fenced()? {
                Some(exp) => exp,
                None => {
                    return self.fail(format!(
                        "unexpected control sequence \\left\n  {EXPECTING_DELIMITER}"
                    ));
                }
            },
            "text" | "mbox" | "textrm" => self.text_command(None)?,
            "textbf" | "textit" | "textsf" | "texttt" => {
                self.text_command(lookup(TEXT_STYLES, name))?
            }
            "operatorname" => {
                let limits = self.peek() == Some('*');
                if limits {
                    self.pos += 1;
                }
                let class = if limits {
                    Class::FuncLimits
                } else {
                    Class::Func
                };
                self.operator(class, true)?
            }
            "mathop" => {
                let (start, depth) = (self.pos, self.depth);
                let arg = self.argument().ok();
                match arg.as_ref().and_then(|arg| classed(Class::Op, arg)) {
                    Some(exp) => exp,
                    // A name of letters is a function name.
                    None => {
                        (self.pos, self.depth) = (start, depth);
                        self.operator(Class::Func, false)?
                    }
                }
            }
            "lVert" => {
                let place = self.pos - 6;
                let paired = self
                    .paired_bars
                    .get_or_insert_with(|| paired_bars(&self.chars))
                    .binary_search(&place)
                    .is_ok();
                if !paired {
                    return Ok(symbol(name).map(|(class, text)| sym(class, text)));
                }
                let body = self.list(Stop::Bars)?;
                Exp::Fenced {
                    open: Some("\u{2016}".into()),
                    body,
                    close: Some("\u{2016}".into()),
                }
            }
            "end" if self.peek() == Some('{') => {
                return self.fail(format!("unexpected \"{{\"\n  {EXPECTING_SPACE}"));
            }
            "mathbin" | "mathrel" | "mathord" | "mathpunct" | "mathopen" | "mathclose" => {
                self.class_command(name)?
            }
            "displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle" => {
                self.expression()?
            }
            "color" => {
                self.skip_plain_space();
                if self.peek() != Some('{') {
                    return Ok(None);
                }
                self.argument()?;
                self.argument()?
            }
            "boxed" => Exp::Enclose("box", boxed(self.argument()?)),
            "cancel" => Exp::Enclose("updiagonalstrike", boxed(self.argument()?)),
            "bcancel" => Exp::Enclose("downdiagonalstrike", boxed(self.argument()?)),
            "xcancel" => Exp::Enclose(
                "updiagonalstrike downdiagonalstrike",
                boxed(self.argument()?),
            ),
            "phantom" => Exp::Phantom(boxed(self.argument()?)),
            "stackrel" | "overset" | "underset" => {
                let script = Some(boxed(self.argument()?));
                let base = boxed(self.argument()?);
                let (under, over) = if name == "underset" {
                    (script, None)
                } else {
                    (None, script)
                };
                Exp::Stack { base, under, over }
            }
            "xrightarrow" | "xleftarrow" => {
                let arrow = if name == "xrightarrow" {
                    "\u{2192}"
                } else {
                    "\u{2190}"
                };
                self.skip_space();
                let mut under = None;
                if self.peek() == Some('[') {
                    self.pos += 1;
                    under = Some(boxed(single(self.list(Stop::Bracket)?)));
                }
                let over = Some(boxed(self.argument()?));
                Exp::Stack {
                    base: boxed(sym(Class::Op, arrow)),
                    under,
                    over,
                }
            }
            "substack" => {
                self.skip_space();
                if self.peek() != Some('{') {
                    return self.unexpected();
                }
                self.pos += 1;
                let rows = self.rows(Stop::Brace, true)?;
                Exp::Table {
                    aligns: Aligns::All("center"),
                    rows,
                    cells_in_mrow: true,
                }
            }
            "bmod" | "mod" | "pmod" | "pod" => self.modulo(name)?,
            "not" => return self.negation(),
            "begin" => return self.begin(),
            _ => match symbol(name) {
                Some((class, text)) => sym(class, text),
                None => return Ok(None),
            },
        };
        Ok(Some(exp))
    }

    fn fraction(&mut self, line: bool) -> Parsed<Exp> {
        let num = boxed(self.argument()?);
        let den = boxed(self.argument()?);
        Ok(Exp::Frac { num, den, line })
    }

    /// `\genfrac{open}{close}{thickness}{style}{num}{den}`.
    fn genfrac(&mut self) -> Parsed<Exp> {
        let open = self.raw_group()?;
        let close = self.raw_group()?;
        let thickness = self.raw_group()?;
        let style = self.raw_group()?;
        let known = [&open, &close]
            .iter()
            .all(|d| matches!(d.as_str(), "" | "(" | ")" | "[" | "]" | "|" | "."));
        if !known
            || !matches!(thickness.as_str(), "" | "0pt" | "1pt" | "2pt")
            || !matches!(style.as_str(), "" | "0" | "1" | "2" | "3")
        {
            return self.fail("this form of \\genfrac has no conversion");
        }
        let mut frac = self.fraction(thickness != "0pt")?;
        if style == "0" {
            frac = Exp::Display(true, boxed(frac));
        }
        Ok(Exp::Fenced {
            open: Some(open).filter(|d| !d.is_empty()),
            body: vec![frac],
            close: Some(close).filter(|d| !d.is_empty()),
        })
    }

    fn size(&self, name: &str) -> Option<&'static str> {
        let base = name
            .strip_suffix(['l', 'r'])
            .filter(|base| lookup(SIZES, base).is_some())
            .unwrap_or(name);
        lookup(SIZES, base).copied()
    }

    /// The delimiter after a size command.
    fn sized(&mut self, percent: &'static str) -> Parsed<Option<Exp>> {
        self.skip_plain_space();
        let delimiter = match self.peek() {
            Some('(' | ')' | '[' | ']' | '|' | '/' | '.' | '+' | '<' | '>') => self.atom()?,
            Some('\\') => {
                self.pos += 1;
                let name = self.control_name()?;
                match delimiter_command(&name, SIZED) {
                    Some((class, text)) => sym(class, text),
                    None => return self.not_a_delimiter(),
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(Exp::Sized(percent, boxed(delimiter))))
    }

    /// The failure after a control sequence that is not a delimiter. The
    /// oracle reports the character after it.
    fn not_a_delimiter<T>(&mut self) -> Parsed<T> {
        self.skip_plain_space();
        let found = match self.peek() {
            None => "unexpected eof".to_string(),
            Some(c) => format!("unexpected {}", quoted(c)),
        };
        self.fail(format!("{found}\n  {EXPECTING_SPACE}"))
    }

    /// The delimiter after `\left`, `\right` or `\middle`: `Some(None)` is
    /// `.`, and `None` is no delimiter. `strict`: a control sequence that is
    /// not a delimiter is a failure at the character after it.
    fn delimiter(&mut self, strict: bool) -> Parsed<Option<Option<String>>> {
        self.skip_space();
        let start = self.pos;
        let Some(c) = self.peek() else {
            return Ok(None);
        };
        self.pos += 1;
        let text = match c {
            '.' => return Ok(Some(None)),
            '(' | ')' | '[' | ']' | '|' => c.to_string(),
            '<' => "\u{27e8}".to_string(),
            '>' => "\u{27e9}".to_string(),
            '\\' => {
                let name = self.control_name()?;
                match delimiter_command(&name, DELIMITERS) {
                    Some((_, text)) => text.to_string(),
                    None if strict => return self.not_a_delimiter(),
                    None => {
                        self.pos = start;
                        return Ok(None);
                    }
                }
            }
            _ => {
                self.pos = start;
                return Ok(None);
            }
        };
        Ok(Some(Some(text)))
    }

    /// `\left` with its delimiter, the body and `\right` with its delimiter.
    /// `None`: no delimiter follows `\left`.
    fn fenced(&mut self) -> Parsed<Option<Exp>> {
        let Some(open) = self.delimiter(true)? else {
            return Ok(None);
        };
        let body = self.list(Stop::Right)?;
        let close = if self.peek().is_some() {
            self.pos += "\\right".len();
            match self.delimiter(true)? {
                Some(close) => close,
                None => {
                    let found = if self.peek().is_none() {
                        "unexpected eof"
                    } else {
                        "unexpected control sequence \\right"
                    };
                    return self.fail(format!("{found}\n  {EXPECTING_DELIMITER}"));
                }
            }
        } else {
            None
        };
        Ok(Some(Exp::Fenced { open, body, close }))
    }

    /// The content of `\operatorname{...}` or of `\mathop{...}`. Content
    /// that is text gives a function name of the class `class`. One
    /// construct that is not text is upright when `upright` is set.
    fn operator(&mut self, class: Class, upright: bool) -> Parsed<Exp> {
        self.skip_plain_space();
        if self.peek() != Some('{') {
            return match self.peek() {
                Some(c) if c.is_ascii_alphanumeric() => {
                    self.pos += 1;
                    Ok(Exp::Sym(class, c.to_string()))
                }
                _ => self.unexpected(),
            };
        }
        let (start, depth) = (self.pos, self.depth);
        self.pos += 1;
        let mut name = String::new();
        match self.operator_text(&mut name)? {
            Some(shape) if !(upright && shape.grouped) => {
                // The name of a function with limits keeps its limits.
                let class = if LIMIT_FUNCS.contains(&name.as_str()) {
                    Class::FuncLimits
                } else {
                    class
                };
                return Ok(Exp::Sym(class, name));
            }
            // `\operatorname` with a group of other than one item beside
            // other items is upright math, as `\mathrm` is.
            Some(_) => {
                (self.pos, self.depth) = (start, depth);
                let arg = self.argument()?;
                let styled = Exp::Styled(&NORMAL, flatten(arg));
                return Ok(if class == Class::FuncLimits {
                    Exp::Limits(boxed(styled))
                } else {
                    styled
                });
            }
            None => {}
        }
        (self.pos, self.depth) = (start, depth);
        if !upright {
            return self.fail("this operator has no conversion");
        }
        let arg = self.argument()?;
        let construct = matches!(
            arg,
            Exp::Sqrt(_)
                | Exp::Root(..)
                | Exp::Frac { .. }
                | Exp::Fenced { .. }
                | Exp::Accent { .. }
                | Exp::Table { .. }
        );
        if construct {
            return Ok(Exp::Styled(&NORMAL, vec![arg]));
        }
        self.fail("this operator name has no conversion")
    }

    /// Reads the content of a group as the text of an operator name, up to
    /// the closing brace. `None`: the content is not text, and the cursor
    /// has no meaning.
    fn operator_text(&mut self, out: &mut String) -> Parsed<Option<TextShape>> {
        self.enter()?;
        let mut shape = TextShape {
            items: 0,
            grouped: false,
        };
        let mut odd_group = false;
        loop {
            let Some(c) = self.peek() else {
                return self.fail("unexpected eof");
            };
            self.pos += 1;
            shape.items += 1;
            match c {
                '}' => {
                    shape.items -= 1;
                    break;
                }
                '{' => {}
                c if is_tex_space(c) => {
                    shape.items -= 1;
                    continue;
                }
                '\\' => {
                    let name = self.control_name()?;
                    let alphabet = lookup(ALPHABETS, &name).map(|alphabet| alphabet.other);
                    match (name.as_str(), alphabet, symbol(&name)) {
                        (",", _, _) => out.push('\u{2006}'),
                        (" ", _, _) => out.push('\u{a0}'),
                        ("text" | "textrm" | "mbox", _, _) | (_, Some(_), _) => {
                            // The crate has oracle results only for letters
                            // and digits in a nested group.
                            if self.peek() != Some('{') {
                                return Ok(None);
                            }
                            self.pos += 1;
                            let pairs = alphabet.map_or(&[][..], variant_chars);
                            loop {
                                match self.peek() {
                                    Some('}') => break,
                                    Some(c) if c.is_ascii_alphanumeric() => {
                                        out.push(variant_char(pairs, c));
                                    }
                                    _ => return Ok(None),
                                }
                                self.pos += 1;
                            }
                            self.pos += 1;
                        }
                        (_, _, Some((_, text))) => out.push_str(text),
                        _ => return Ok(None),
                    }
                    continue;
                }
                '^' | '_' | '&' | '$' | '#' | '%' | '~' | '"' | '\0'..='\u{1f}' | '\u{7f}' => {
                    return Ok(None);
                }
                c => {
                    out.push(c);
                    continue;
                }
            }
            let Some(inner) = self.operator_text(out)? else {
                return Ok(None);
            };
            shape.grouped |= inner.grouped;
            odd_group |= inner.items != 1;
        }
        shape.grouped |= odd_group && shape.items > 1;
        self.depth -= 1;
        Ok(Some(shape))
    }

    /// `\mathbin` and the other commands that set the class of their
    /// argument.
    fn class_command(&mut self, name: &str) -> Parsed<Exp> {
        let class = match name {
            "mathbin" => Class::Bin,
            "mathrel" => Class::Rel,
            "mathpunct" => Class::Punct,
            "mathopen" => Class::Open,
            "mathclose" => Class::Close,
            _ => Class::Ord,
        };
        let arg = self.argument()?;
        if let Some(exp) = classed(class, &arg) {
            return Ok(exp);
        }
        match arg {
            Exp::Num(_) | Exp::Sym(..) | Exp::Group(_) | Exp::Styled(..) => Ok(arg),
            _ => self.fail("this argument of a class command has no conversion"),
        }
    }

    /// `\bmod`, `\mod`, `\pmod` and `\pod` with their argument.
    fn modulo(&mut self, name: &str) -> Parsed<Exp> {
        let arg = self.argument()?;
        let wide = space("0.444em", "\u{2000}");
        let mut items = vec![Exp::Space(if name == "mod" { wide } else { MEDIUM })];
        let parens = matches!(name, "pmod" | "pod");
        if parens {
            items.push(sym(Class::Open, "("));
        }
        if name != "pod" {
            items.push(sym(Class::Func, "mod"));
            items.push(Exp::Space(MEDIUM));
        }
        items.push(arg);
        if parens {
            items.push(sym(Class::Close, ")"));
        }
        Ok(Exp::Group(items))
    }

    /// `\not` with the symbol after it.
    fn negation(&mut self) -> Parsed<Option<Exp>> {
        const SLASH: char = '\u{338}';
        self.skip_space();
        let start = self.pos;
        let exp = match self.argument() {
            Ok(Exp::Sym(Class::Rel, text)) if self.chars.get(start) != Some(&'{') => {
                let negated = NEGATIONS
                    .binary_search_by(|(base, _)| (*base).cmp(text.as_str()))
                    .ok()
                    .and_then(|i| NEGATIONS.get(i))
                    .map_or_else(|| format!("{text}{SLASH}"), |(_, n)| (*n).to_string());
                Exp::Sym(Class::Rel, negated)
            }
            Ok(Exp::Sym(Class::Ident, text)) if self.chars.get(start) != Some(&'{') => {
                Exp::Sym(Class::Ident, format!("{text}{SLASH}"))
            }
            // An upright Greek letter is negated as a command, not as a
            // character.
            Ok(Exp::Sym(Class::IdentUpright, text)) if self.chars.get(start) == Some(&'\\') => {
                Exp::Sym(Class::Ident, format!("{text}{SLASH}"))
            }
            Ok(Exp::Num(text)) if self.chars.get(start) != Some(&'{') => {
                Exp::Num(format!("{text}{SLASH}"))
            }
            _ => {
                self.pos = start;
                return self.fail("this use of \\not has no conversion");
            }
        };
        Ok(Some(exp))
    }

    /// `\text{...}` and its relatives: text with math in `$...$` or `\(...\)`.
    fn text_command(&mut self, style: Option<&'static Alphabet>) -> Parsed<Exp> {
        self.skip_plain_space();
        match self.peek() {
            Some('{') => self.pos += 1,
            Some(c) if c.is_alphanumeric() => {
                self.pos += 1;
                return Ok(Exp::Text(style, c.to_string()));
            }
            _ => return self.unexpected(),
        }
        let mut pieces = Vec::new();
        self.text_pieces(style, &mut pieces)?;
        if pieces.is_empty() {
            pieces.push(Exp::Text(style, String::new()));
        }
        Ok(single(pieces))
    }

    /// Reads text up to the closing brace and appends its pieces.
    fn text_pieces(
        &mut self,
        style: Option<&'static Alphabet>,
        pieces: &mut Vec<Exp>,
    ) -> Parsed<()> {
        self.enter()?;
        let mut text = String::new();
        let flush = |text: &mut String, pieces: &mut Vec<Exp>| {
            if text.is_empty() {
                return;
            }
            let (before, after) = (text.starts_with(' '), text.ends_with(' '));
            let piece = Exp::Text(style, std::mem::take(text));
            if !before && !after {
                pieces.push(piece);
                return;
            }
            let mut items = Vec::new();
            if before {
                items.push(Exp::Space(TEXT_EDGE));
            }
            items.push(piece);
            if after {
                items.push(Exp::Space(TEXT_EDGE));
            }
            pieces.push(Exp::PaddedText(items));
        };
        loop {
            let Some(c) = self.peek() else {
                return self.fail(format!("unexpected eof\n  {EXPECTING_TEXT}"));
            };
            self.pos += 1;
            match c {
                '}' => break,
                '{' => {
                    flush(&mut text, pieces);
                    self.text_pieces(style, pieces)?;
                }
                '$' => {
                    flush(&mut text, pieces);
                    // Math in text cannot start with white space or a comment.
                    if let Some(c) = self.peek().filter(|c| *c == '%' || is_tex_space(*c)) {
                        return self.fail(format!("unexpected \"{c}\"\n  {EXPECTING_DOLLAR}"));
                    }
                    // Nor with a command that the oracle reads as white space.
                    if ["label", "tag", "nonumber", "allowbreak"]
                        .iter()
                        .any(|name| self.at_word(name))
                    {
                        self.pos += 1;
                        let name = self.control_name()?;
                        return self.unknown(&name);
                    }
                    let math = self.list(Stop::Dollar)?;
                    if math.is_empty() {
                        self.pos -= 1;
                        return self.unexpected();
                    }
                    pieces.push(single(math));
                }
                '\\' => {
                    let Some(next) = self.peek() else {
                        return self.fail("unexpected eof");
                    };
                    let width = match next {
                        '$' | '%' | '{' | '}' | '&' | '_' | '#' | ' ' => {
                            text.push(next);
                            None
                        }
                        '(' => {
                            self.pos += 1;
                            flush(&mut text, pieces);
                            pieces.push(single(self.list(Stop::MathClose)?));
                            continue;
                        }
                        ',' => Some(THIN),
                        '!' => Some(NEGATIVE),
                        ':' | '>' => Some(MEDIUM),
                        ';' => Some(THICK),
                        _ => {
                            let shown = if next == '\\' {
                                "\\\\".to_string()
                            } else {
                                next.to_string()
                            };
                            return self.fail(format!(
                                "unexpected \"{shown}\"\n  expecting \" \", \"!\", \",\", \":\", \";\" or \">\""
                            ));
                        }
                    };
                    self.pos += 1;
                    if let Some(width) = width {
                        flush(&mut text, pieces);
                        pieces.push(Exp::Space(width));
                    } else {
                        // The oracle reads an escaped character as a
                        // control word: the white space after it goes.
                        while self.peek().is_some_and(is_tex_space) {
                            self.pos += 1;
                        }
                    }
                }
                '~' => text.push('\u{a0}'),
                '`' if self.peek() == Some('`') => {
                    self.pos += 1;
                    text.push('\u{201c}');
                }
                '`' => text.push('\u{2018}'),
                '\'' if self.peek() == Some('\'') => {
                    self.pos += 1;
                    text.push('\u{201d}');
                }
                '\'' => text.push('\u{2019}'),
                '-' if self.peek() == Some('-') => {
                    self.pos += 1;
                    if self.peek() == Some('-') {
                        return self.fail("this dash in text has no conversion");
                    }
                    text.push('\u{2013}');
                }
                '%' => {
                    self.pos -= 1;
                    return self.fail("a comment in text has no conversion");
                }
                // A run of white space that starts with a space or a tab is
                // one space; a line feed alone stays.
                ' ' | '\t' => {
                    text.push(' ');
                    while matches!(self.peek(), Some(' ' | '\t' | '\n')) {
                        self.pos += 1;
                    }
                }
                c => text.push(c),
            }
        }
        flush(&mut text, pieces);
        self.depth -= 1;
        Ok(())
    }

    /// The name in the braces after `\begin` or `\end`.
    fn environment_name(&mut self) -> Parsed<String> {
        let name = self.raw_group()?;
        Ok(name.trim().to_string())
    }

    /// `\begin{name} ... \end{name}` after `\begin`.
    fn begin(&mut self) -> Parsed<Option<Exp>> {
        let start = self.pos;
        let name = self.environment_name()?;
        let base = name.strip_suffix('*').unwrap_or(&name);
        if base == "equation" {
            let items = self.list(Stop::Cell)?;
            if items.is_empty() {
                return self.unexpected();
            }
            self.end(&name)?;
            return Ok(Some(single(items)));
        }
        let (environment, array) = match (base, environment(base)) {
            ("array", _) => (
                Environment {
                    open: None,
                    close: None,
                    aligns: self.array_columns()?,
                },
                true,
            ),
            (_, Some(environment)) => (environment, false),
            (_, None) => {
                self.pos = start;
                self.skip_plain_space();
                self.pos += 1;
                self.skip_plain_space();
                let first = self.peek().unwrap_or(' ');
                return self.fail(format!("unexpected \"{first}\"\n  {EXPECTING_ENVIRONMENT}"));
            }
        };
        if !array && matches!(base, "alignat" | "alignedat") {
            self.expect_brace()?;
            self.raw_group()?;
        }
        let spaced_size = array || base == "eqnarray";
        let rows = self.rows(Stop::Cell, spaced_size)?;
        self.end(&name)?;
        let table = Exp::Table {
            aligns: environment.aligns,
            rows,
            cells_in_mrow: false,
        };
        if environment.open.is_none() && environment.close.is_none() {
            return Ok(Some(table));
        }
        Ok(Some(Exp::Fenced {
            open: environment.open.map(str::to_string),
            body: vec![table],
            close: environment.close.map(str::to_string),
        }))
    }

    /// Fails when the argument in braces of an environment is missing.
    fn expect_brace(&mut self) -> Parsed<()> {
        self.skip_space();
        let found = match self.peek() {
            Some('{') => return Ok(()),
            None => "unexpected eof".to_string(),
            Some(c) => format!("unexpected {}", quoted(c)),
        };
        self.fail(format!("{found}\n  {EXPECTING_COLUMNS}"))
    }

    /// The column alignments in the argument of `array`.
    fn array_columns(&mut self) -> Parsed<Aligns> {
        self.expect_brace()?;
        let start = self.pos;
        let spec: Vec<char> = self.raw_group()?.chars().collect();
        let mut columns: Vec<(&'static str, bool)> = Vec::new();
        let mut i = 0;
        while let Some(&c) = spec.get(i) {
            i += 1;
            match c {
                'l' => columns.push(("left", false)),
                'c' => columns.push(("center", false)),
                'r' => columns.push(("right", false)),
                'p' | '@' => {
                    if c == 'p' {
                        columns.push(("left", false));
                    }
                    if spec.get(i) == Some(&'{') {
                        while spec.get(i).is_some_and(|c| *c != '}') {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                '*' | '{' | '}' | '\\' | '0'..='9' => {
                    self.pos = start;
                    return self.fail("this column description has no conversion");
                }
                _ => {}
            }
        }
        // Columns that are right and left by turns have no padding inside a pair.
        let pairs = !columns.is_empty()
            && columns.len().is_multiple_of(2)
            && columns
                .iter()
                .enumerate()
                .all(|(i, (align, _))| *align == if i % 2 == 0 { "right" } else { "left" });
        for column in &mut columns {
            column.1 = pairs;
        }
        Ok(Aligns::Columns(columns))
    }

    /// The rows of a table up to `\end` (for `Stop::Cell`) or to the closing
    /// brace. The size `[...]` of a row end can come after comments, and
    /// after white space when `spaced_size` is set (`array`, `eqnarray`).
    fn rows(&mut self, stop: Stop, spaced_size: bool) -> Parsed<Vec<Row>> {
        let mut rows = Vec::new();
        let mut row = Vec::new();
        loop {
            let cell = if stop == Stop::Cell {
                self.list(Stop::Cell)?
            } else {
                self.substack_line()?
            };
            row.push(cell);
            match self.peek() {
                Some('&') if stop == Stop::Cell => self.pos += 1,
                Some('\\') if self.peek_at(1) == Some('\\') => {
                    self.pos += 2;
                    let after = self.pos;
                    loop {
                        match self.peek() {
                            Some('%') => {
                                while self.peek().is_some_and(|c| c != '\n') {
                                    self.pos += 1;
                                }
                                self.pos += 1;
                            }
                            Some(c) if spaced_size && is_tex_space(c) => self.pos += 1,
                            _ => break,
                        }
                    }
                    if self.peek() != Some('[') {
                        self.pos = after;
                    } else {
                        while self.peek().is_some_and(|c| c != ']') {
                            self.pos += 1;
                        }
                        self.pos += 1;
                    }
                    rows.push(std::mem::take(&mut row));
                }
                Some('}') if stop == Stop::Brace => {
                    self.pos += 1;
                    break;
                }
                _ => break,
            }
        }
        rows.push(row);
        Ok(rows)
    }

    /// One line of `\substack`: the items up to `\\` or the closing brace.
    fn substack_line(&mut self) -> Parsed<Vec<Exp>> {
        self.enter()?;
        let mut items = Vec::new();
        loop {
            self.skip_space();
            match self.peek() {
                None => return self.fail("unexpected eof"),
                Some('}') => break,
                Some('\\') if self.peek_at(1) == Some('\\') => break,
                Some('&') => return self.unexpected(),
                Some('\\') if self.style_switch(Stop::Cell)? => {}
                Some(_) => items.push(self.scripted()?),
            }
        }
        self.depth -= 1;
        Ok(items)
    }

    /// `\end{name}` at the cursor.
    fn end(&mut self, name: &str) -> Parsed<()> {
        if !self.at_word("end") {
            return self.unexpected();
        }
        self.pos += 4;
        let start = self.pos;
        let found = self.environment_name()?;
        if found != name.trim() {
            self.pos = start + 1;
            self.skip_plain_space();
            let first = found.chars().next().unwrap_or('}');
            return self.fail(format!(
                "unexpected \"{first}\"\n  expecting white space or \"{}\"",
                name.trim()
            ));
        }
        Ok(())
    }
}

/// The class of a character that is not ASCII. `None`: the crate has no
/// oracle result for the character.
fn char_class(c: char) -> Option<Class> {
    let known = CHARS
        .binary_search_by(|(known, _)| known.cmp(&c))
        .ok()
        .and_then(|i| CHARS.get(i))
        .map(|(_, class)| *class);
    match known {
        Some(class) => Some(class),
        None if c.is_alphabetic() => Some(Class::Ident),
        None if c.is_numeric() => Some(Class::Ord),
        None => None,
    }
}

/// The expression for a list: the item itself when the list has one item.
pub(crate) fn single(mut items: Vec<Exp>) -> Exp {
    if items.len() == 1
        && let Some(exp) = items.pop()
    {
        return exp;
    }
    Exp::Group(items)
}

/// The argument of a class command as one symbol of `class`: a symbol, or
/// text with its edge spaces and its style. `None`: the argument keeps its
/// own items.
fn classed(class: Class, arg: &Exp) -> Option<Exp> {
    match arg {
        Exp::Sym(old, text) if !matches!(old, Class::Func | Class::FuncLimits) => {
            Some(Exp::Sym(class, text.clone()))
        }
        Exp::Text(None, text) => Some(Exp::Sym(class, text.clone())),
        Exp::Text(Some(style), text) => {
            Some(Exp::Styled(style, vec![Exp::Sym(class, text.clone())]))
        }
        Exp::PaddedText(items) => items.iter().find_map(|item| match item {
            Exp::Text(..) => classed(class, item),
            _ => None,
        }),
        _ => None,
    }
}

fn flatten(exp: Exp) -> Vec<Exp> {
    match exp {
        Exp::Group(items) => items,
        other => vec![other],
    }
}
