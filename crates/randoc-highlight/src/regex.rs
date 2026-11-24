//! A port of `Regex.KDE` from skylighting-core 0.15: the regular expression
//! engine for the patterns in KDE syntax definitions.
//!
//! The matcher is set based. It follows all alternatives at the same time.
//! Each partial match records its choices in a path. The match with the
//! smallest path is the match that a backtracking engine such as PCRE finds
//! first. A match always starts at byte 0 of the input (an implicit `^`).
//!
//! The tokenizer gives the matcher the rest of the current line, as
//! skylighting does. Look-behind therefore cannot see text before the
//! current position, and `^`, `\A` and `\G` all mean "byte 0 of the input".

// A `Path` holds a `Cell` (the child cache of its trie node), but its order
// and equality use only the node identity, depth and bit, which never
// change. Sets of matches are therefore safe.
#![allow(clippy::mutable_key_type)]

use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::{Rc, Weak};

/// Maximum nesting depth of parentheses in a pattern. A deeper pattern is a
/// compile error.
pub(crate) const MAX_NESTING: usize = 64;

/// Maximum depth of a compiled syntax tree. Large repetition counts such as
/// `a{1,5000}` expand to deep trees; a deeper tree is a compile error.
pub(crate) const MAX_TREE_DEPTH: usize = 512;

/// Maximum number of nodes of a compiled syntax tree. A larger tree is a
/// compile error.
pub(crate) const MAX_TREE_NODES: usize = 100_000;

/// Work budget of one match call in the unit tests: the sum of the sizes of
/// all match sets that the matcher processes. When the count passes the
/// limit, the call stops and returns no match. The tokenizer gives each
/// call the budget that its line has left.
#[cfg(test)]
pub(crate) const MAX_STEPS: usize = 1_000_000;

/// Maximum recursion depth of the matcher, which recurses on the syntax tree
/// and on subroutine calls. A deeper recursion stops the call with no match.
pub(crate) const MAX_EXEC_DEPTH: usize = 192;

/// Stack budget of one match call in bytes, a second bound after
/// [`MAX_EXEC_DEPTH`]. A match call stops with no match when its frames use
/// more stack than this, so that no pattern and no input can overflow a
/// 2 MiB thread stack, also in a debug build with larger frames.
pub(crate) const MAX_EXEC_STACK: usize = 384 * 1024;

/// The matcher keeps only this many preferred matches at a time, as
/// skylighting does (its `sizeLimit`).
const SIZE_LIMIT: usize = 2000;

/// Repetition counts above this value are not quantifiers (the PCRE2 limit).
const MAX_REPEAT: usize = 65535;

/// The direction in which a sub-pattern consumes the input. Look-behind
/// assertions match backward.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    /// From left to right.
    Forward,
    /// From right to left.
    Backward,
}

/// A compiled pattern: the abstract syntax tree of `Regex.KDE`.
///
/// `MatchConcat` holds a flat list where the Haskell type nests binary
/// nodes. The matcher flattens nested concatenations, so the two forms give
/// the same results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Regex {
    /// `.`: any one character.
    MatchAnyChar,
    /// `%N`: the text of capture N of the rule that started the context.
    /// Without substitution it matches the literal text `%N`.
    MatchDynamic(usize),
    /// One character that satisfies the predicate.
    MatchChar(CharPred),
    /// One or more repetitions (greedy).
    MatchSome(Box<Regex>),
    /// Two alternatives; the left one is preferred.
    MatchAlt(Box<Regex>, Box<Regex>),
    /// A sequence of sub-patterns.
    MatchConcat(Vec<Regex>),
    /// A capturing group with its number.
    MatchCapture(usize, Box<Regex>),
    /// A back-reference to a group; the flag is case sensitivity.
    MatchCaptured(usize, bool),
    /// `\b`.
    AssertWordBoundary,
    /// `^`, `\A` or `\G`.
    AssertBeginning,
    /// `$`.
    AssertEnd,
    /// `(?=...)` or `(?<=...)`.
    AssertPositive(Direction, Box<Regex>),
    /// `(?!...)` or `(?<!...)`.
    AssertNegative(Direction, Box<Regex>),
    /// A possessive quantifier or an atomic group `(?>...)`.
    Possessive(Box<Regex>),
    /// Marks a lazy repetition. It only wraps `MatchSome`.
    Lazy(Box<Regex>),
    /// `(?N)` or `(?R)` (group 0).
    Subroutine(usize),
    /// The empty pattern.
    MatchNull,
}

/// A character predicate. Literal characters are code points, because an
/// escape such as `\x{D800}` can name a surrogate, which matches nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CharPred {
    /// Equal to the code point.
    Is(u32),
    /// Equal to the code point after lower-case mapping of both sides.
    IsFold(u32),
    /// In the inclusive code point range.
    Range(u32, u32),
    /// One of the code points (`\Q...\E` in a bracket class), sorted.
    AnyOf(Vec<u32>),
    /// A named class.
    Class(CharClass),
    /// The inverse of the predicate.
    Not(Box<CharPred>),
    /// A bracket class: one of the items matches. With `fold`, an item can
    /// also match the lower-case or upper-case form of the character.
    Set {
        /// The items of the class.
        items: Vec<CharPred>,
        /// True for a case-insensitive class.
        fold: bool,
    },
}

/// A named character class, with the semantics of the Haskell `Data.Char`
/// predicates that skylighting uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CharClass {
    /// ASCII digits (`isDigit`).
    Digit,
    /// `isSpace`: ASCII white space, U+00A0 and category Zs.
    Space,
    /// PCRE `\h`.
    HorizSpace,
    /// Code points below 128.
    Ascii,
    /// `[:blank:]`: `isSpace` without `\n`, `\r`, `\f` and `\v`.
    Blank,
    /// `[:graph:]`: printable and not white space.
    Graph,
    /// ASCII hexadecimal digits.
    HexDigit,
    /// A set of Unicode general categories.
    Categories(CategorySet),
}

/// A set of Unicode general categories. Bit `i` is the category with index
/// `i` in the order `Lu Ll Lt Lm Lo Mn Mc Me Nd Nl No Pc Pd Ps Pe Pi Pf Po Sm
/// Sc Sk So Zs Zl Zp Cc Cf Cs Co Cn`, which is the Haskell
/// `GeneralCategory` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CategorySet(u32);

const LU: u32 = 0;
const LL: u32 = 1;
const LT: u32 = 2;
const PC: u32 = 11;
const ZS: u32 = 22;
const ZL: u32 = 23;
const ZP: u32 = 24;
const CC: u32 = 25;
const CF: u32 = 26;
const CS: u32 = 27;
const CO: u32 = 28;
const CN: u32 = 29;

const fn bit(cat: u32) -> u32 {
    1 << cat
}

const LETTER: u32 = 0b1_1111;
const MARK: u32 = 0b111 << 5;
const NUMBER: u32 = 0b111 << 8;
const PUNCTUATION: u32 = 0b111_1111 << 11;
const SYMBOL: u32 = 0b1111 << 18;
const SEPARATOR: u32 = 0b111 << 22;
const OTHER: u32 = 0b1_1111 << 25;
const NOT_PRINT: u32 = bit(ZL) | bit(ZP) | bit(CC) | bit(CF) | bit(CS) | bit(CO) | bit(CN);
const WORD: u32 = LETTER | NUMBER | bit(PC);

impl CategorySet {
    fn contains(self, c: char) -> bool {
        self.0 & bit(category(c)) != 0
    }
}

/// The general category index of a character (see [`CategorySet`]): a
/// table lookup for ASCII, a binary search in the range table otherwise.
fn category(c: char) -> u32 {
    match ASCII_CATEGORIES.get(c as usize) {
        Some(cat) => u32::from(*cat),
        None => category_by_search(c),
    }
}

/// The general category index from the range table [`GC_TABLE`].
fn category_by_search(c: char) -> u32 {
    let cp = c as u32;
    let i = GC_TABLE.partition_point(|&e| e >> 5 <= cp);
    i.checked_sub(1)
        .and_then(|j| GC_TABLE.get(j))
        .map_or(CN, |e| e & 0x1f)
}

/// The general category index of each ASCII character, from [`GC_TABLE`].
const ASCII_CATEGORIES: [u8; 128] = {
    let mut table = [CN as u8; 128];
    let mut entry = 0;
    while entry < GC_TABLE.len() {
        let first = (GC_TABLE[entry] >> 5) as usize;
        let end = if entry + 1 < GC_TABLE.len() {
            (GC_TABLE[entry + 1] >> 5) as usize
        } else {
            usize::MAX
        };
        let mut cp = first;
        while cp < end && cp < table.len() {
            table[cp] = (GC_TABLE[entry] & 0x1f) as u8;
            cp += 1;
        }
        if first >= table.len() {
            break;
        }
        entry += 1;
    }
    table
};

fn in_categories(c: char, mask: u32) -> bool {
    CategorySet(mask).contains(c)
}

/// True for a word character: a letter, a number or connector punctuation
/// (general categories L, N and Pc), as skylighting's `isWordChar`.
pub(crate) fn is_word_char(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric() || c == '_';
    }
    in_categories(c, WORD)
}

/// Haskell `isAlphaNum`: general categories L and N.
pub(crate) fn is_alpha_num(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric();
    }
    in_categories(c, LETTER | NUMBER)
}

/// Haskell `isSpace`.
fn is_space(c: char) -> bool {
    let cp = c as u32;
    if cp <= 0x377 {
        cp == 0x20 || (0x09..=0x0d).contains(&cp) || cp == 0xa0
    } else {
        category(c) == ZS
    }
}

/// PCRE `\h`.
fn is_horiz_space(c: char) -> bool {
    matches!(
        c,
        '\t' | ' ' | '\u{a0}' | '\u{1680}' | '\u{180e}' | '\u{2000}'
            ..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

/// Haskell `toLower`: the simple (one character) lower-case mapping.
fn simple_lower(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        // U+0130 is the only character with a longer full mapping.
        _ if c == '\u{130}' => 'i',
        _ => c,
    }
}

/// Haskell `toUpper`: the simple (one character) upper-case mapping.
fn simple_upper(c: char) -> char {
    let mut it = c.to_uppercase();
    match (it.next(), it.next()) {
        (Some(u), None) => u,
        _ => {
            // Of the characters with a longer full mapping, only these Greek
            // letters with ypogegrammeni have a simple mapping.
            let cp = c as u32;
            let mapped = match cp {
                0x1f80..=0x1f87 | 0x1f90..=0x1f97 | 0x1fa0..=0x1fa7 => cp + 8,
                0x1fb3 | 0x1fc3 | 0x1ff3 => cp + 9,
                _ => cp,
            };
            char::from_u32(mapped).unwrap_or(c)
        }
    }
}

fn lower_code_point(cp: u32) -> u32 {
    char::from_u32(cp).map_or(cp, |c| simple_lower(c) as u32)
}

impl CharClass {
    fn matches(self, c: char) -> bool {
        match self {
            CharClass::Digit => c.is_ascii_digit(),
            CharClass::Space => is_space(c),
            CharClass::HorizSpace => is_horiz_space(c),
            CharClass::Ascii => c.is_ascii(),
            CharClass::Blank => is_space(c) && !matches!(c, '\n' | '\r' | '\x0c' | '\x0b'),
            CharClass::Graph => !in_categories(c, NOT_PRINT) && !is_space(c),
            CharClass::HexDigit => c.is_ascii_hexdigit(),
            CharClass::Categories(set) => set.contains(c),
        }
    }
}

impl CharPred {
    /// The predicate "equal to `c`".
    pub(crate) fn is(c: char) -> CharPred {
        CharPred::Is(c as u32)
    }

    fn matches(&self, c: char) -> bool {
        match self {
            CharPred::Is(x) => c as u32 == *x,
            CharPred::IsFold(x) => simple_lower(c) as u32 == lower_code_point(*x),
            CharPred::Range(lo, hi) => (*lo..=*hi).contains(&(c as u32)),
            CharPred::AnyOf(cs) => cs.binary_search(&(c as u32)).is_ok(),
            CharPred::Class(class) => class.matches(c),
            CharPred::Not(p) => !p.matches(c),
            CharPred::Set { items, fold } => {
                let any = |d: char| items.iter().any(|p| p.matches(d));
                any(c) || (*fold && (any(simple_lower(c)) || any(simple_upper(c))))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Compiles a pattern. With `case_sensitive` false, literals and classes
/// ignore case. With `minimal` true, quantifiers are lazy by default and a
/// `?` suffix makes them greedy (PCRE `UNGREEDY`).
pub(crate) fn compile(pattern: &str, case_sensitive: bool, minimal: bool) -> Result<Regex, String> {
    let mut parser = Parser {
        src: pattern,
        pos: 0,
        capture: 0,
        case_sensitive,
        minimal,
        nesting: 0,
    };
    let re = parser.p_alternatives(None)?;
    if parser.pos == pattern.len() {
        Ok(re)
    } else {
        Err(format!("parse error at byte position {}", parser.pos))
    }
}

/// A hard error (`Err`) stops the compilation. A soft failure (`Ok(None)`)
/// makes the parser try the next alternative, as attoparsec's `<|>` does.
type PResult<T> = Result<Option<T>, String>;

#[derive(Clone, Copy)]
struct Snapshot {
    pos: usize,
    capture: usize,
    case_sensitive: bool,
    minimal: bool,
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    capture: usize,
    case_sensitive: bool,
    minimal: bool,
    nesting: usize,
}

/// Inline flags such as `i-U`.
#[derive(Clone, Copy)]
struct Flags {
    on_i: bool,
    on_u: bool,
    off_i: bool,
    off_u: bool,
}

impl Flags {
    fn apply(self, p: &mut Parser<'_>) {
        p.case_sensitive = if self.on_i && !self.off_i {
            false
        } else {
            self.off_i || p.case_sensitive
        };
        p.minimal = if self.on_u && !self.off_u {
            true
        } else {
            !self.off_u && p.minimal
        };
    }
}

/// What a group does with its contents.
#[derive(Clone, Copy)]
enum Wrap {
    Plain,
    Capture(usize),
    Positive(Direction),
    Negative(Direction),
    Subroutine(usize),
    Possessive,
}

impl Wrap {
    fn apply(self, contents: Regex) -> Regex {
        match self {
            Wrap::Plain => contents,
            Wrap::Capture(n) => Regex::MatchCapture(n, Box::new(contents)),
            Wrap::Positive(dir) => Regex::AssertPositive(dir, Box::new(contents)),
            Wrap::Negative(dir) => Regex::AssertNegative(dir, Box::new(contents)),
            Wrap::Subroutine(n) => Regex::Subroutine(n),
            Wrap::Possessive => Regex::Possessive(Box::new(contents)),
        }
    }
}

#[derive(Clone, Copy)]
enum Quant {
    Star,
    Plus,
    Opt,
    AtLeast(usize),
    AtMost(usize),
    Between(usize, usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flavor {
    Greedy,
    Lazy,
    Possessive,
}

fn too_large() -> String {
    format!(
        "pattern too large: more than {MAX_TREE_NODES} nodes or deeper than {MAX_TREE_DEPTH} levels"
    )
}

fn parse_number(digits: &str) -> Option<usize> {
    digits.bytes().try_fold(0usize, |n, b| {
        n.checked_mul(10)?
            .checked_add(usize::from(b.checked_sub(b'0')?))
    })
}

/// A repetition count: `None` for no digits, a saturated value on overflow
/// (which is then too big to be a quantifier).
fn parse_count(digits: &str) -> Option<usize> {
    if digits.is_empty() {
        None
    } else {
        Some(parse_number(digits).unwrap_or(usize::MAX))
    }
}

/// Haskell `read "'\xHHHH'" :: Char`: hexadecimal digits only, at most
/// U+10FFFF.
fn parse_hex_char(digits: &str) -> Option<u32> {
    if digits.is_empty() {
        return None;
    }
    digits.chars().try_fold(0u32, |n, c| {
        let v = n.checked_mul(16)?.checked_add(c.to_digit(16)?)?;
        (v <= 0x10_ffff).then_some(v)
    })
}

fn is_special(c: char) -> bool {
    matches!(
        c,
        '\\' | '?' | '*' | '+' | '[' | '%' | '(' | ')' | '|' | '.' | '$' | '^'
    )
}

fn literal(cp: u32, case_sensitive: bool) -> CharPred {
    if case_sensitive {
        CharPred::Is(cp)
    } else {
        CharPred::IsFold(cp)
    }
}

fn class(class: CharClass) -> Regex {
    Regex::MatchChar(CharPred::Class(class))
}

fn not_class(class: CharClass) -> Regex {
    Regex::MatchChar(CharPred::Not(Box::new(CharPred::Class(class))))
}

fn category_mask(name: &str) -> u32 {
    let names = [
        "Lu", "Ll", "Lt", "Lm", "Lo", "Mn", "Mc", "Me", "Nd", "Nl", "No", "Pc", "Pd", "Ps", "Pe",
        "Pi", "Pf", "Po", "Sm", "Sc", "Sk", "So", "Zs", "Zl", "Zp", "Cc", "Cf", "Cs", "Co", "Cn",
    ];
    match name {
        "L" => LETTER,
        "M" => MARK,
        "N" => NUMBER,
        "P" => PUNCTUATION,
        "S" => SYMBOL,
        "Z" => SEPARATOR,
        "C" => OTHER,
        _ => names
            .iter()
            .position(|n| *n == name)
            .and_then(|i| u32::try_from(i).ok())
            .map_or(0, bit),
    }
}

/// The parts of a sequence: the items of a `MatchConcat`, or the pattern.
fn flatten(re: Regex) -> Vec<Regex> {
    match re {
        Regex::MatchConcat(items) => items,
        other => vec![other],
    }
}

/// Haskell `a <> b` (`MatchConcat a b`).
fn append(a: Regex, b: Regex) -> Regex {
    let mut items = flatten(a);
    items.extend(flatten(b));
    Regex::MatchConcat(items)
}

/// Haskell `mconcat`: `p1 <> (p2 <> ... (pn <> MatchNull))`.
fn mconcat(parts: Vec<Regex>) -> Regex {
    if parts.is_empty() {
        return Regex::MatchNull;
    }
    let mut items: Vec<Regex> = parts.into_iter().flat_map(flatten).collect();
    items.push(Regex::MatchNull);
    Regex::MatchConcat(items)
}

/// Haskell `foldr1 MatchAlt`.
fn fold_alts(alts: Vec<Regex>) -> Regex {
    let mut it = alts.into_iter().rev();
    let mut acc = it.next().unwrap_or(Regex::MatchNull);
    for x in it {
        acc = Regex::MatchAlt(Box::new(x), Box::new(acc));
    }
    acc
}

fn children(re: &Regex) -> &[Regex] {
    match re {
        Regex::MatchConcat(items) => items,
        Regex::MatchSome(r)
        | Regex::MatchCapture(_, r)
        | Regex::AssertPositive(_, r)
        | Regex::AssertNegative(_, r)
        | Regex::Possessive(r)
        | Regex::Lazy(r) => std::slice::from_ref(r.as_ref()),
        _ => &[],
    }
}

/// The depth and the node count of a tree, without recursion.
fn measure(re: &Regex) -> (usize, usize) {
    let mut stack = vec![(re, 1usize)];
    let (mut depth, mut size) = (0, 0);
    while let Some((node, d)) = stack.pop() {
        size += 1;
        depth = depth.max(d);
        if let Regex::MatchAlt(a, b) = node {
            stack.push((a, d + 1));
            stack.push((b, d + 1));
        }
        stack.extend(children(node).iter().map(|c| (c, d + 1)));
    }
    (depth, size)
}

fn check_tree(re: &Regex) -> Result<(), String> {
    let (depth, size) = measure(re);
    if depth > MAX_TREE_DEPTH || size > MAX_TREE_NODES {
        Err(too_large())
    } else {
        Ok(())
    }
}

/// Haskell `atmost` and `atmostLazy`: `r(r(r)?)?`, nested so that the size
/// is linear in `n`.
fn at_most(n: usize, r: &Regex, lazy: bool) -> Regex {
    let mut acc = Regex::MatchNull;
    for _ in 0..n {
        let step = Box::new(append(r.clone(), acc));
        let null = Box::new(Regex::MatchNull);
        acc = if lazy {
            Regex::MatchAlt(null, step)
        } else {
            Regex::MatchAlt(step, null)
        };
    }
    acc
}

fn repeat(r: &Regex, n: usize) -> Regex {
    mconcat(vec![r.clone(); n])
}

fn some(r: Regex) -> Regex {
    Regex::MatchSome(Box::new(r))
}

fn alt(a: Regex, b: Regex) -> Regex {
    Regex::MatchAlt(Box::new(a), Box::new(b))
}

fn star(r: Regex, lazy: bool) -> Regex {
    if lazy {
        alt(Regex::MatchNull, Regex::Lazy(Box::new(some(r))))
    } else {
        alt(some(r), Regex::MatchNull)
    }
}

fn build_quantified(q: Quant, flavor: Flavor, r: Regex) -> Result<Regex, String> {
    let (depth, size) = measure(&r);
    let (copies, nesting) = match q {
        Quant::Star | Quant::Plus | Quant::Opt => (1, 1),
        Quant::AtLeast(n) => (n.saturating_add(1), 1),
        Quant::AtMost(n) => (n, n),
        Quant::Between(m, n) => (n, n - m),
    };
    let est_size = size
        .saturating_add(2)
        .saturating_mul(copies.max(1))
        .saturating_add(4);
    let est_depth = depth
        .saturating_add(nesting.saturating_mul(2))
        .saturating_add(3);
    if est_size > MAX_TREE_NODES || est_depth > MAX_TREE_DEPTH {
        return Err(too_large());
    }
    let lazy = flavor == Flavor::Lazy;
    let built = match q {
        Quant::Star => star(r, lazy),
        Quant::Plus if lazy => Regex::Lazy(Box::new(some(r))),
        Quant::Plus => some(r),
        Quant::Opt if lazy => alt(Regex::MatchNull, r),
        Quant::Opt => alt(r, Regex::MatchNull),
        Quant::AtLeast(n) => append(repeat(&r, n), star(r, lazy)),
        Quant::AtMost(n) | Quant::Between(0, n) => at_most(n, &r, lazy),
        Quant::Between(m, n) => append(repeat(&r, m), at_most(n - m, &r, lazy)),
    };
    Ok(if flavor == Flavor::Possessive {
        Regex::Possessive(Box::new(built))
    } else {
        built
    })
}

impl<'a> Parser<'a> {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            pos: self.pos,
            capture: self.capture,
            case_sensitive: self.case_sensitive,
            minimal: self.minimal,
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.pos = s.pos;
        self.capture = s.capture;
        self.case_sensitive = s.case_sensitive;
        self.minimal = s.minimal;
    }

    /// Runs `f`; on a soft failure, restores the position and the state.
    fn attempt<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        let s = self.snapshot();
        let r = f(self)?;
        if r.is_none() {
            self.restore(s);
        }
        Ok(r)
    }

    /// Like [`Parser::attempt`] for a parser without hard errors.
    fn attempt_opt<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        let s = self.snapshot();
        let r = f(self);
        if r.is_none() {
            self.restore(s);
        }
        r
    }

    fn rest(&self) -> &'a str {
        self.src.get(self.pos..).unwrap_or("")
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn next_char(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn eat_str(&mut self, s: &str) -> bool {
        if self.rest().starts_with(s) {
            self.pos += s.len();
            true
        } else {
            false
        }
    }

    fn take_while(&mut self, f: impl Fn(char) -> bool) -> &'a str {
        let rest = self.rest();
        let n = rest.find(|c: char| !f(c)).unwrap_or(rest.len());
        self.pos += n;
        rest.get(..n).unwrap_or("")
    }

    fn digits1(&mut self) -> Option<&'a str> {
        let d = self.take_while(|c| c.is_ascii_digit());
        (!d.is_empty()).then_some(d)
    }

    /// Exactly `n` characters (attoparsec `take`).
    fn take_chars(&mut self, n: usize) -> Option<&'a str> {
        let rest = self.rest();
        let mut end = 0;
        for _ in 0..n {
            end += rest.get(end..)?.chars().next()?.len_utf8();
        }
        self.pos += end;
        rest.get(..end)
    }

    /// `pRegex`, and the contents of a group. With `reset_to`, each
    /// alternative starts numbering at that capture number (`(?|...)`).
    fn p_alternatives(&mut self, reset_to: Option<usize>) -> Result<Regex, String> {
        let mut alts = vec![self.p_alt_part_or_null()?];
        let mut highest = self.capture;
        while self.eat('|') {
            if alts.len() >= MAX_TREE_DEPTH {
                return Err(too_large());
            }
            if let Some(n) = reset_to {
                self.capture = n;
            }
            alts.push(self.p_alt_part_or_null()?);
            highest = highest.max(self.capture);
        }
        if reset_to.is_some() {
            self.capture = highest;
        }
        let re = fold_alts(alts);
        check_tree(&re)?;
        Ok(re)
    }

    fn p_alt_part_or_null(&mut self) -> Result<Regex, String> {
        Ok(self.attempt(Self::p_alt_part)?.unwrap_or(Regex::MatchNull))
    }

    fn p_alt_part(&mut self) -> PResult<Regex> {
        let mut parts = Vec::new();
        let mut size = 0usize;
        while let Some(part) = self.attempt(Self::p_regex_part)? {
            size = size.saturating_add(measure(&part).1);
            if size > MAX_TREE_NODES {
                return Err(too_large());
            }
            parts.push(part);
        }
        Ok((!parts.is_empty()).then(|| mconcat(parts)))
    }

    fn p_regex_part(&mut self) -> PResult<Regex> {
        let atom = match self.attempt(|p| Ok(p.p_regex_char()))? {
            Some(a) => a,
            None => match self.attempt(Self::p_parenthesized)? {
                Some(a) => a,
                None => return Ok(None),
            },
        };
        self.p_suffix(atom).map(Some)
    }

    fn p_parenthesized(&mut self) -> PResult<Regex> {
        if !self.eat('(') {
            return Ok(None);
        }
        if self.nesting >= MAX_NESTING {
            return Err(format!("pattern nests deeper than {MAX_NESTING} groups"));
        }
        self.nesting += 1;
        let r = self.p_group_body();
        self.nesting -= 1;
        r
    }

    fn p_group_body(&mut self) -> PResult<Regex> {
        if let Some(r) = self.attempt(|p| Ok(p.p_inline_modifiers()))? {
            return Ok(Some(r));
        }
        let reset = self.eat_str("?|");
        let (wrap, flags) = if reset {
            (Wrap::Plain, None)
        } else {
            match self.attempt_opt(|p| {
                if p.eat('?') {
                    p.p_group_modifiers()
                } else {
                    None
                }
            }) {
                Some(x) => x,
                None => {
                    self.capture += 1;
                    (Wrap::Capture(self.capture), None)
                }
            }
        };
        let current = self.capture;
        let (old_case, old_minimal) = (self.case_sensitive, self.minimal);
        if let Some(f) = flags {
            f.apply(self);
        }
        let contents = self.p_alternatives(reset.then_some(current))?;
        if !self.eat(')') {
            return Ok(None);
        }
        self.case_sensitive = old_case;
        self.minimal = old_minimal;
        Ok(Some(wrap.apply(contents)))
    }

    /// `(?i)` without a colon: the flags hold to the end of the enclosing
    /// group, whose closing parenthesis restores them.
    fn p_inline_modifiers(&mut self) -> Option<Regex> {
        if !self.eat('?') {
            return None;
        }
        let flags = self.p_regex_modifier();
        if !self.eat(')') {
            return None;
        }
        flags.apply(self);
        Some(Regex::MatchNull)
    }

    fn p_group_modifiers(&mut self) -> Option<(Wrap, Option<Flags>)> {
        if let Some(f) = self.attempt_opt(|p| {
            let f = p.p_regex_modifier();
            p.eat(':').then_some(f)
        }) {
            return Some((Wrap::Plain, Some(f)));
        }
        if let Some(w) = self.attempt_opt(|p| {
            let dir = if p.eat('<') {
                Direction::Backward
            } else {
                Direction::Forward
            };
            if p.eat('=') {
                Some(Wrap::Positive(dir))
            } else if p.eat('!') {
                Some(Wrap::Negative(dir))
            } else {
                None
            }
        }) {
            return Some((w, None));
        }
        if let Some(n) = self.attempt_opt(|p| p.digits1().and_then(parse_number)) {
            return Some((Wrap::Subroutine(n), None));
        }
        if self.eat('R') {
            return Some((Wrap::Subroutine(0), None));
        }
        if self.eat('>') {
            return Some((Wrap::Possessive, None));
        }
        None
    }

    /// The flags `i` and `U` change the matching; `m` and `s` are accepted
    /// and do nothing, because the input is one line.
    fn p_regex_modifier(&mut self) -> Flags {
        let ons = self.take_while(|c| "imsU".contains(c));
        let offs = if self.eat('-') {
            self.take_while(|c| "imnsxU".contains(c))
        } else {
            ""
        };
        Flags {
            on_i: ons.contains('i'),
            on_u: ons.contains('U'),
            off_i: offs.contains('i'),
            off_u: offs.contains('U'),
        }
    }

    /// A quantifier after an anchor stays unconsumed, which then gives a
    /// parse error, as in PCRE.
    fn p_suffix(&mut self, atom: Regex) -> Result<Regex, String> {
        if matches!(
            atom,
            Regex::AssertBeginning | Regex::AssertEnd | Regex::AssertWordBoundary
        ) {
            return Ok(atom);
        }
        let start = self.snapshot();
        let Some(q) = self.p_quantifier() else {
            self.restore(start);
            return Ok(atom);
        };
        let flavor = if self.eat('+') {
            Flavor::Possessive
        } else if self.eat('?') == self.minimal {
            Flavor::Greedy
        } else {
            Flavor::Lazy
        };
        build_quantified(q, flavor, atom)
    }

    fn p_quantifier(&mut self) -> Option<Quant> {
        match self.next_char()? {
            '*' => Some(Quant::Star),
            '+' => Some(Quant::Plus),
            '?' => Some(Quant::Opt),
            '{' => {
                let min = parse_count(self.take_while(|c| c.is_ascii_digit()));
                let max = if self.eat(',') {
                    parse_count(self.take_while(|c| c.is_ascii_digit()))
                } else {
                    min
                };
                if !self.eat('}') {
                    return None;
                }
                if min.is_some_and(|n| n > MAX_REPEAT) || max.is_some_and(|n| n > MAX_REPEAT) {
                    return None;
                }
                match (min, max) {
                    (None, None) => None,
                    (Some(n), None) => Some(Quant::AtLeast(n)),
                    (None, Some(n)) => Some(Quant::AtMost(n)),
                    (Some(m), Some(n)) if m > n => None,
                    (Some(m), Some(n)) => Some(Quant::Between(m, n)),
                }
            }
            _ => None,
        }
    }

    fn p_regex_char(&mut self) -> Option<Regex> {
        let w = self.next_char()?;
        let cs = self.case_sensitive;
        match w {
            '.' => Some(Regex::MatchAnyChar),
            '%' => Some(
                self.attempt_opt(|p| p.digits1().and_then(parse_number))
                    .map_or(Regex::MatchChar(CharPred::is('%')), Regex::MatchDynamic),
            ),
            '\\' => self.p_regex_escaped_char(cs),
            '$' => Some(Regex::AssertEnd),
            '^' => Some(Regex::AssertBeginning),
            '[' => self.p_char_class(cs),
            // A `{` that starts a valid quantifier has nothing to repeat.
            '{' => match self.attempt_opt(Self::p_quantifier_shape) {
                Some(()) => None,
                None => Some(Regex::MatchChar(CharPred::is('{'))),
            },
            _ if is_special(w) => None,
            _ => Some(Regex::MatchChar(literal(w as u32, cs))),
        }
    }

    /// `{m}`, `{m,}`, `{m,n}` or `{,n}`, after the `{`.
    fn p_quantifier_shape(&mut self) -> Option<()> {
        if self.digits1().is_some() {
            if self.eat(',') {
                self.take_while(|c| c.is_ascii_digit());
            }
        } else if self.eat(',') {
            self.digits1()?;
        } else {
            return None;
        }
        self.eat('}').then_some(())
    }

    fn p_regex_escaped_char(&mut self, cs: bool) -> Option<Regex> {
        let c = self.next_char()?;
        if let Some(r) = self.attempt_opt(|p| p.p_escape_special(c, cs)) {
            return Some(r);
        }
        Some(Regex::MatchChar(literal(self.p_escaped(c)?, cs)))
    }

    fn p_escape_special(&mut self, c: char, cs: bool) -> Option<Regex> {
        match c {
            'b' => Some(Regex::AssertWordBoundary),
            'B' => Some(Regex::AssertNegative(
                Direction::Forward,
                Box::new(Regex::AssertWordBoundary),
            )),
            'G' | 'A' => Some(Regex::AssertBeginning),
            '{' => {
                let digits = self.digits1()?;
                if !self.eat('}') {
                    return None;
                }
                Some(Regex::MatchCaptured(parse_number(digits)?, cs))
            }
            'g' => {
                let digits = self
                    .attempt_opt(|p| {
                        if !p.eat('{') {
                            return None;
                        }
                        let d = p.digits1()?;
                        p.eat('}').then_some(d)
                    })
                    .or_else(|| self.digits1())?;
                Some(Regex::MatchCaptured(parse_number(digits)?, cs))
            }
            'd' => Some(class(CharClass::Digit)),
            'D' => Some(not_class(CharClass::Digit)),
            's' => Some(class(CharClass::Space)),
            'S' => Some(not_class(CharClass::Space)),
            'h' => Some(class(CharClass::HorizSpace)),
            'H' => Some(not_class(CharClass::HorizSpace)),
            'w' => Some(class(CharClass::Categories(CategorySet(WORD)))),
            'W' => Some(not_class(CharClass::Categories(CategorySet(WORD)))),
            'p' => Some(Regex::MatchChar(self.p_unicode_class()?)),
            'P' => Some(Regex::MatchChar(CharPred::Not(Box::new(
                self.p_unicode_class()?,
            )))),
            // `\0` is an octal escape, not a back-reference.
            '1'..='9' => Some(Regex::MatchCaptured(c.to_digit(10)? as usize, cs)),
            _ => None,
        }
    }

    /// The character that an escape `\c` names (Haskell `pEscaped`).
    fn p_escaped(&mut self, c: char) -> Option<u32> {
        match c {
            '\\' => Some(0x5c),
            'a' => Some(0x07),
            'b' => Some(0x08),
            'f' => Some(0x0c),
            'n' => Some(0x0a),
            'r' => Some(0x0d),
            't' => Some(0x09),
            'v' => Some(0x0b),
            '0' => Some(self.octal_digits(0)),
            '1'..='7' => Some(self.octal_digits(c.to_digit(8)?)),
            'z' => {
                let digits = self.take_chars(4)?;
                parse_hex_char(digits)
            }
            'x' => {
                let digits = self
                    .attempt_opt(|p| {
                        if !p.eat('{') {
                            return None;
                        }
                        let d = p.take_while(|c| c != '}');
                        p.eat('}').then_some(d)
                    })
                    .or_else(|| self.take_chars(2))?;
                parse_hex_char(digits)
            }
            _ if in_categories(c, PUNCTUATION | SYMBOL) || is_space(c) => Some(c as u32),
            _ => None,
        }
    }

    /// Up to two more octal digits after the first digit `init`.
    fn octal_digits(&mut self, init: u32) -> u32 {
        let mut value = init;
        for _ in 0..2 {
            match self.peek().and_then(|c| c.to_digit(8)) {
                Some(d) => {
                    self.pos += 1;
                    value = value * 8 + d;
                }
                None => break,
            }
        }
        value
    }

    /// A bracket class, after the `[`.
    fn p_char_class(&mut self, cs: bool) -> Option<Regex> {
        let negated = self.eat('^');
        let mut items = Vec::new();
        // A `]` in first position is a literal, or the start of a range.
        if self.eat(']') {
            let item = self
                .attempt_opt(|p| {
                    if !p.eat('-') {
                        return None;
                    }
                    p.class_char().map(|d| CharPred::Range(']' as u32, d))
                })
                .unwrap_or(CharPred::is(']'));
            items.push(item);
        }
        while let Some(item) = self.class_item() {
            items.push(item);
        }
        if !self.eat(']') {
            return None;
        }
        let set = CharPred::Set { items, fold: !cs };
        Some(Regex::MatchChar(if negated {
            CharPred::Not(Box::new(set))
        } else {
            set
        }))
    }

    fn class_item(&mut self) -> Option<CharPred> {
        if let Some(p) = self.attempt_opt(Self::class_quoted) {
            return Some(p);
        }
        if let Some(p) = self.attempt_opt(Self::class_escape) {
            return Some(p);
        }
        if let Some(p) = self.attempt_opt(Self::class_posix) {
            return Some(p);
        }
        if let Some(p) = self.attempt_opt(Self::class_range) {
            return Some(p);
        }
        if let Some(p) = self.attempt_opt(|p| {
            if !p.eat_str("\\p") {
                return None;
            }
            p.p_unicode_class()
        }) {
            return Some(p);
        }
        self.attempt_opt(|p| {
            if !p.eat_str("\\P") {
                return None;
            }
            p.p_unicode_class().map(|u| CharPred::Not(Box::new(u)))
        })
    }

    fn class_quoted(&mut self) -> Option<CharPred> {
        if !self.eat_str("\\Q") {
            return None;
        }
        let rest = self.rest();
        let end = rest.find("\\E")?;
        let mut chars: Vec<u32> = rest.get(..end)?.chars().map(|c| c as u32).collect();
        chars.sort_unstable();
        chars.dedup();
        self.pos += end + 2;
        Some(CharPred::AnyOf(chars))
    }

    fn class_escape(&mut self) -> Option<CharPred> {
        if !self.eat('\\') {
            return None;
        }
        let word = CharClass::Categories(CategorySet(WORD));
        let (cls, negated) = match self.next_char()? {
            'd' => (CharClass::Digit, false),
            'D' => (CharClass::Digit, true),
            's' => (CharClass::Space, false),
            'S' => (CharClass::Space, true),
            'h' => (CharClass::HorizSpace, false),
            'H' => (CharClass::HorizSpace, true),
            'w' => (word, false),
            'W' => (word, true),
            _ => return None,
        };
        let p = CharPred::Class(cls);
        Some(if negated {
            CharPred::Not(Box::new(p))
        } else {
            p
        })
    }

    fn class_posix(&mut self) -> Option<CharPred> {
        if !self.eat_str("[:") {
            return None;
        }
        let negated = self.eat('^');
        let cats = |mask| CharClass::Categories(CategorySet(mask));
        let table = [
            ("alnum", cats(LETTER | NUMBER)),
            ("alpha", cats(LETTER)),
            ("ascii", CharClass::Ascii),
            ("blank", CharClass::Blank),
            ("cntrl", cats(bit(CC))),
            ("digit", CharClass::Digit),
            ("graph", CharClass::Graph),
            ("lower", cats(bit(LL))),
            ("upper", cats(bit(LU) | bit(LT))),
            ("print", cats(!NOT_PRINT & 0x3fff_ffff)),
            ("punct", cats(PUNCTUATION)),
            ("space", CharClass::Space),
            ("word", cats(WORD)),
            ("xdigit", CharClass::HexDigit),
        ];
        let cls = table
            .iter()
            .find(|(name, _)| self.eat_str(name))
            .map(|(_, cls)| *cls)?;
        if !self.eat_str(":]") {
            return None;
        }
        let p = CharPred::Class(cls);
        Some(if negated {
            CharPred::Not(Box::new(p))
        } else {
            p
        })
    }

    fn class_range(&mut self) -> Option<CharPred> {
        let c = self.class_char()?;
        Some(
            self.attempt_opt(|p| {
                if !p.eat('-') {
                    return None;
                }
                p.class_char().map(|d| CharPred::Range(c, d))
            })
            .unwrap_or(CharPred::Is(c)),
        )
    }

    /// One character in a bracket class: an escape, or any character except
    /// `\` and `]`.
    fn class_char(&mut self) -> Option<u32> {
        if let Some(d) = self.attempt_opt(|p| {
            if !p.eat('\\') {
                return None;
            }
            let c = p.next_char()?;
            p.p_escaped(c)
        }) {
            return Some(d);
        }
        let c = self.peek()?;
        if c == '\\' || c == ']' {
            return None;
        }
        self.pos += c.len_utf8();
        Some(c as u32)
    }

    /// `{Lu}`, `{^Lu}` or `L`, after `\p`. An unknown name matches nothing.
    fn p_unicode_class(&mut self) -> Option<CharPred> {
        let braced = self.attempt_opt(|p| {
            if !p.eat('{') {
                return None;
            }
            let negated = p.eat('^');
            let name = p.take_while(|c| c != '}');
            p.eat('}').then_some((negated, name))
        });
        let (negated, name) = match braced {
            Some(x) => x,
            None => {
                let start = self.pos;
                let c = self.peek()?;
                if !in_categories(c, LETTER) {
                    return None;
                }
                self.pos += c.len_utf8();
                (false, self.src.get(start..self.pos)?)
            }
        };
        let p = CharPred::Class(CharClass::Categories(CategorySet(category_mask(name))));
        Some(if negated {
            CharPred::Not(Box::new(p))
        } else {
            p
        })
    }
}

// ---------------------------------------------------------------------------
// Capturing groups and dynamic substitution
// ---------------------------------------------------------------------------

/// The capturing groups of a pattern by number, with the whole pattern at
/// key 0. For a number that occurs more than once (`(?|...)`), the first
/// The characters that can start a match: each match of the pattern
/// consumes a first character that satisfies one of the predicates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FirstChars(Vec<CharPred>);

impl FirstChars {
    /// Tests whether a match can start with `c`.
    pub(crate) fn allows(&self, c: char) -> bool {
        self.0.iter().any(|p| p.matches(c))
    }
}

/// The deepest pattern tree that [`first_chars`] looks into.
const MAX_FIRST_DEPTH: usize = 64;

/// The first characters of the matches of `re`, or `None` when the pattern
/// can match the empty string, or when its first step is not one simple
/// character (a back-reference, `%N`, a subroutine, `.`).
pub(crate) fn first_chars(re: &Regex) -> Option<FirstChars> {
    let mut preds = Vec::new();
    first_into(re, &mut preds, 0).then_some(FirstChars(preds))
}

/// Adds the first characters of `re` to `out`; false when they are not known.
fn first_into(re: &Regex, out: &mut Vec<CharPred>, depth: usize) -> bool {
    if depth > MAX_FIRST_DEPTH {
        return false;
    }
    match re {
        Regex::MatchChar(p) => {
            out.push(p.clone());
            true
        }
        Regex::MatchAlt(a, b) => first_into(a, out, depth + 1) && first_into(b, out, depth + 1),
        Regex::MatchSome(r) | Regex::Lazy(r) | Regex::Possessive(r) | Regex::MatchCapture(_, r) => {
            first_into(r, out, depth + 1)
        }
        Regex::MatchConcat(items) => {
            for item in items {
                match item {
                    // Zero-width parts only restrict the match; the first
                    // character comes from a later part.
                    Regex::AssertBeginning
                    | Regex::AssertEnd
                    | Regex::AssertWordBoundary
                    | Regex::AssertPositive(..)
                    | Regex::AssertNegative(..) => continue,
                    other => return first_into(other, out, depth + 1),
                }
            }
            false
        }
        _ => false,
    }
}

/// group in pattern order wins, as with Haskell's left-biased union.
pub(crate) fn capturing_groups(re: &Regex) -> HashMap<usize, Regex> {
    let mut groups = HashMap::new();
    let mut stack = vec![re];
    while let Some(node) = stack.pop() {
        match node {
            Regex::MatchCapture(i, inner) => {
                groups.entry(*i).or_insert_with(|| inner.as_ref().clone());
                stack.push(inner);
            }
            Regex::MatchAlt(a, b) => {
                stack.push(b);
                stack.push(a);
            }
            _ => stack.extend(children(node).iter().rev()),
        }
    }
    groups.insert(0, re.clone());
    groups
}

/// Replaces each `%N` with the literal text of capture N. A missing
/// capture gives no match. The replacement is case sensitive and leaves
/// placeholders inside `Possessive` and `Lazy` unchanged.
///
/// Checks the node and byte budgets before cloning the tree or expanding
/// captures. The budgets include the copies made for the group table.
pub(crate) fn substitute_dynamic<'c>(
    re: &Regex,
    capture: &impl Fn(usize) -> Option<&'c str>,
) -> Result<Option<Regex>, String> {
    let mut nodes = 0;
    let mut bytes = 0;
    let mut replacements = Vec::new();
    // The result tree and group 0 each contain the full pattern.
    let mut pending = vec![(re, 1, 2, true)];
    while let Some((node, depth, copies, substitute)) = pending.pop() {
        let (size, depth) = match node {
            Regex::MatchDynamic(n) if substitute => {
                let Some(text) = capture(*n) else {
                    return Ok(None);
                };
                bytes += text.len().saturating_mul(copies).min(MAX_TREE_NODES + 1);
                if bytes > MAX_TREE_NODES {
                    return Err(format!("captured text exceeds {MAX_TREE_NODES} bytes"));
                }
                replacements.push(text);
                if text.is_empty() {
                    (1, depth)
                } else {
                    // A concatenation contains the characters and a null node.
                    (text.chars().count() + 2, depth + 1)
                }
            }
            Regex::MatchAlt(a, b) => {
                pending.push((b, depth + 1, copies, substitute));
                pending.push((a, depth + 1, copies, substitute));
                (1, depth)
            }
            _ => {
                let copies = copies + usize::from(matches!(node, Regex::MatchCapture(..)));
                let substitute =
                    substitute && !matches!(node, Regex::Possessive(_) | Regex::Lazy(_));
                pending.extend(
                    children(node)
                        .iter()
                        .rev()
                        .map(|child| (child, depth + 1, copies, substitute)),
                );
                (1, depth)
            }
        };
        nodes += size * copies;
        if nodes > MAX_TREE_NODES || depth > MAX_TREE_DEPTH {
            return Err(too_large());
        }
    }

    // An explicit stack instead of recursion: the tree can be
    // `MAX_TREE_DEPTH` levels deep.
    let mut out = re.clone();
    let mut replacements = replacements.into_iter();
    let mut stack = vec![&mut out];
    while let Some(node) = stack.pop() {
        if matches!(node, Regex::MatchDynamic(_)) {
            let Some(text) = replacements.next() else {
                return Ok(None);
            };
            *node = mconcat(
                text.chars()
                    .map(|c| Regex::MatchChar(CharPred::is(c)))
                    .collect(),
            );
            continue;
        }
        match node {
            Regex::MatchAlt(a, b) => {
                stack.push(b);
                stack.push(a);
            }
            Regex::MatchConcat(items) => stack.extend(items.iter_mut().rev()),
            Regex::MatchSome(r)
            | Regex::MatchCapture(_, r)
            | Regex::AssertPositive(_, r)
            | Regex::AssertNegative(_, r) => stack.push(r),
            _ => {}
        }
    }
    Ok(Some(out))
}

// ---------------------------------------------------------------------------
// Matcher
// ---------------------------------------------------------------------------

/// A successful match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegexMatch {
    /// The byte length of the matched prefix of the input.
    pub(crate) len: usize,
    /// Group number to (byte offset, byte length) in the input. A group
    /// captured inside a look-behind has length 0 (skylighting stores a
    /// negative length, which also slices to the empty string).
    pub(crate) captures: BTreeMap<usize, (usize, usize)>,
}

/// Matches `re` at the start of `input` and returns the match that PCRE
/// finds first. Computes the capturing groups for subroutine calls.
#[cfg(test)]
pub(crate) fn match_regex(re: &Regex, input: &str) -> Option<RegexMatch> {
    match_with_groups(&capturing_groups(re), re, input)
}

/// Like [`match_regex`] with the groups from [`capturing_groups`], so that
/// a caller can compute them once per pattern.
#[cfg(test)]
pub(crate) fn match_with_groups(
    groups: &HashMap<usize, Regex>,
    re: &Regex,
    input: &str,
) -> Option<RegexMatch> {
    run_match(groups, re, input, MAX_STEPS)
}

/// How one match call searches.
pub(crate) struct Search<'a> {
    /// The work budget of the call in steps.
    pub(crate) max_steps: usize,
    /// From [`ordered_search_applies`]: the ordered search runs first.
    pub(crate) ordered: bool,
    /// From [`probe_applies`]: the probe runs on the whole pattern first.
    pub(crate) probe: bool,
    /// The memory of the probe for the line that holds the input.
    pub(crate) dead_ends: &'a mut DeadEnds,
    /// The byte offset of the input in that line.
    pub(crate) base: usize,
}

/// The memory of the probe for one line: the ends of a pass of a
/// repetition from which the rest of the pattern (or of the assertion that
/// holds the repetition) has no match, by (address of the repeated
/// pattern, byte offset in the line). The addresses belong to the compiled
/// patterns of the syntax definitions, so the memory is valid for each
/// call on the same line. A pattern that lives for one call only (a
/// dynamic rule) needs its own empty memory.
#[derive(Debug, Default)]
pub(crate) struct DeadEnds(HashSet<(usize, usize)>);

impl DeadEnds {
    /// Forgets all dead ends, for a new line.
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

/// Like [`match_with_groups`] with the budget and the search choices of
/// `search`; also returns the steps that the call used.
pub(crate) fn match_budgeted(
    groups: &HashMap<usize, Regex>,
    re: &Regex,
    input: &str,
    search: Search<'_>,
) -> (Option<RegexMatch>, usize) {
    let (found, stats) = run_match_stats(groups, re, input, search);
    (found, stats.steps)
}

/// Runs the set search alone, and then the probe and the ordered search
/// where they apply, and checks that both ways give the same match. Checks
/// each "no match" of the probe with the set search too.
#[cfg(test)]
fn run_match(
    groups: &HashMap<usize, Regex>,
    re: &Regex,
    input: &str,
    max_steps: usize,
) -> Option<RegexMatch> {
    let (set, _, wrong) = with_probe_check(|| {
        let set = run_plain(groups, re, input, max_steps, false).0;
        let fast = run_match_stats(
            groups,
            re,
            input,
            Search {
                max_steps,
                ordered: ordered_search_applies(re),
                probe: probe_applies(re),
                dead_ends: &mut DeadEnds::default(),
                base: 0,
            },
        )
        .0;
        assert_eq!(set, fast, "fast search differs for {re:?} on {input:?}");
        set
    });
    assert_eq!(wrong, 0, "the probe differs for {re:?} on {input:?}");
    set
}

/// What one match call used.
struct MatchStats {
    steps: usize,
    #[cfg(test)]
    peak_stack: usize,
}

/// A match call without the probe of the whole pattern, with a new memory.
#[cfg(test)]
fn run_plain(
    groups: &HashMap<usize, Regex>,
    re: &Regex,
    input: &str,
    max_steps: usize,
    ordered: bool,
) -> (Option<RegexMatch>, MatchStats) {
    let search = Search {
        max_steps,
        ordered,
        probe: false,
        dead_ends: &mut DeadEnds::default(),
        base: 0,
    };
    run_match_stats(groups, re, input, search)
}

fn run_match_stats(
    groups: &HashMap<usize, Regex>,
    re: &Regex,
    input: &str,
    search: Search<'_>,
) -> (Option<RegexMatch>, MatchStats) {
    let Search {
        max_steps,
        ordered,
        probe,
        dead_ends,
        base,
    } = search;
    let mut matcher = Matcher {
        input,
        groups,
        active: HashSet::new(),
        steps: 0,
        max_steps,
        depth: 0,
        stack_base: stack_address(),
        boundary_counts: HashMap::new(),
        alt_seen: HashMap::new(),
        dead_ends: &mut dead_ends.0,
        base,
        probe_log: Vec::new(),
        probe_bodies: HashMap::new(),
        #[cfg(test)]
        peak_stack: 0,
    };
    let start = Match {
        path: Path::root(),
        offset: 0,
        captures: Vec::new(),
    };
    let best = if probe && matcher.probe(Direction::Forward, re, &start) {
        None
    } else {
        let ordered_result = if ordered {
            matcher.search(re, start.clone(), &Cont::Done).ok()
        } else {
            None
        };
        match ordered_result {
            Some(found) => found,
            None => {
                matcher.depth = 0;
                matcher
                    .exec(Direction::Forward, re, single(start))
                    .ok()
                    .and_then(|ms| ms.into_iter().next())
            }
        }
    };
    let stats = MatchStats {
        steps: matcher.steps,
        #[cfg(test)]
        peak_stack: matcher.peak_stack,
    };
    (best.map(to_result), stats)
}

fn to_result(best: Match) -> RegexMatch {
    RegexMatch {
        len: best.offset,
        captures: best
            .captures
            .into_iter()
            .map(|(k, (off, len))| (k, (off, usize::try_from(len).unwrap_or(0))))
            .collect(),
    }
}

/// A sequence of binary choices, compared as a bit string, with a proper
/// prefix before its extensions (Haskell `Path`).
///
/// A path is a node of a trie that all paths of one match call share. Equal
/// paths are the same node, because a push reuses an existing child. A clone
/// and a push are O(1); a comparison is O(log n) with skew-binary jump
/// pointers. A path with its own copy of the bits makes a loop over n
/// characters cost O(n²) time and memory.
#[derive(Clone)]
struct Path(Rc<PathNode>);

struct PathNode {
    parent: Option<Rc<PathNode>>,
    /// An ancestor, for level-ancestor queries in O(log n) steps; `None`
    /// at the root.
    jump: Option<Rc<PathNode>>,
    depth: usize,
    bit: bool,
    /// The children for bit 0 and bit 1, if they exist.
    children: [Cell<Weak<PathNode>>; 2],
}

impl Drop for PathNode {
    /// Drops a long chain of unshared ancestors with a work list, not by
    /// recursion.
    fn drop(&mut self) {
        let mut work: Vec<Rc<PathNode>> = Vec::new();
        work.extend(self.parent.take());
        work.extend(self.jump.take());
        while let Some(rc) = work.pop() {
            if let Ok(mut node) = Rc::try_unwrap(rc) {
                work.extend(node.parent.take());
                work.extend(node.jump.take());
            }
        }
    }
}

impl PathNode {
    fn parent(&self) -> &PathNode {
        self.parent.as_deref().unwrap_or(self)
    }

    fn jump(&self) -> &PathNode {
        self.jump.as_deref().unwrap_or(self)
    }

    /// The ancestor at `depth` (at most the depth of `self`).
    fn ancestor(&self, depth: usize) -> &PathNode {
        let mut node = self;
        while node.depth > depth {
            let jump = node.jump();
            node = if jump.depth >= depth && jump.depth < node.depth {
                jump
            } else {
                node.parent()
            };
        }
        node
    }
}

impl Path {
    fn root() -> Path {
        Path(Rc::new(PathNode {
            parent: None,
            jump: None,
            depth: 0,
            bit: false,
            children: Default::default(),
        }))
    }

    fn depth(&self) -> usize {
        self.0.depth
    }

    fn push(&mut self, bit: bool) {
        let slot = &self.0.children[usize::from(bit)];
        let cached = slot.take();
        if let Some(child) = cached.upgrade() {
            slot.set(cached);
            *self = Path(child);
            return;
        }
        // Skew-binary jumps: the jump depth depends only on the depth.
        let parent = &self.0;
        let pj = parent.jump.clone().unwrap_or_else(|| parent.clone());
        let pjj = pj.jump.clone().unwrap_or_else(|| pj.clone());
        let jump = if parent.depth - pj.depth == pj.depth - pjj.depth {
            pjj
        } else {
            parent.clone()
        };
        let child = Rc::new(PathNode {
            parent: Some(parent.clone()),
            jump: Some(jump),
            depth: parent.depth + 1,
            bit,
            children: Default::default(),
        });
        slot.set(Rc::downgrade(&child));
        *self = Path(child);
    }
}

impl std::fmt::Debug for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Path(depth {})", self.depth())
    }
}

impl PartialEq for Path {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Path {}

impl Ord for Path {
    /// Lexicographic order of the bit strings, a prefix first: the first
    /// bits below the lowest common ancestor decide.
    fn cmp(&self, other: &Self) -> Ordering {
        let (a, b): (&PathNode, &PathNode) = (&self.0, &other.0);
        if std::ptr::eq(a, b) {
            return Ordering::Equal;
        }
        let depth = a.depth.min(b.depth);
        let mut x = a.ancestor(depth);
        let mut y = b.ancestor(depth);
        if std::ptr::eq(x, y) {
            return a.depth.cmp(&b.depth);
        }
        while !std::ptr::eq(x.parent(), y.parent()) {
            let (jx, jy) = (x.jump(), y.jump());
            if std::ptr::eq(jx, jy) || jx.depth >= x.depth {
                x = x.parent();
                y = y.parent();
            } else {
                x = jx;
                y = jy;
            }
        }
        x.bit.cmp(&y.bit)
    }
}

impl PartialOrd for Path {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Group number to (offset, length), sorted by group number. The length is
/// negative for a group captured backward. The derived order equals the
/// order of Haskell's `IntMap`.
type CaptureList = Vec<(usize, (usize, isize))>;

/// A partial match. The derived order compares the path, then the offset,
/// then the captures, as the Haskell `Ord Match` instance does.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Match {
    path: Path,
    offset: usize,
    captures: CaptureList,
}

/// A set of matches in the order of `Match`: a sorted vector without
/// duplicates. Most sets have one or two elements, where a vector needs
/// fewer allocations than a tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct MatchSet(Vec<Match>);

impl MatchSet {
    fn new() -> MatchSet {
        MatchSet(Vec::new())
    }

    fn from_vec(mut items: Vec<Match>) -> MatchSet {
        // The sort is adaptive, so input that is already sorted costs O(n).
        items.sort();
        items.dedup();
        MatchSet(items)
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn iter(&self) -> std::slice::Iter<'_, Match> {
        self.0.iter()
    }

    fn insert(&mut self, m: Match) {
        if self.0.last().is_none_or(|last| *last < m) {
            self.0.push(m);
        } else if let Err(i) = self.0.binary_search(&m) {
            self.0.insert(i, m);
        }
    }
}

impl Extend<Match> for MatchSet {
    fn extend<I: IntoIterator<Item = Match>>(&mut self, iter: I) {
        let mut items = std::mem::take(&mut self.0);
        let before = items.len();
        items.extend(iter);
        if items.len() > before {
            *self = MatchSet::from_vec(items);
        } else {
            self.0 = items;
        }
    }
}

impl FromIterator<Match> for MatchSet {
    fn from_iter<I: IntoIterator<Item = Match>>(iter: I) -> MatchSet {
        MatchSet::from_vec(iter.into_iter().collect())
    }
}

impl IntoIterator for MatchSet {
    type Item = Match;
    type IntoIter = std::vec::IntoIter<Match>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// The work budget or the depth bound was exhausted.
struct Abort;

type Exec = Result<MatchSet, Abort>;

fn single(m: Match) -> MatchSet {
    MatchSet(vec![m])
}

fn state_key(m: &Match) -> (usize, CaptureList) {
    (m.offset, m.captures.clone())
}

fn set_capture(captures: &mut CaptureList, group: usize, value: (usize, isize)) {
    match captures.binary_search_by_key(&group, |(k, _)| *k) {
        Ok(i) => {
            if let Some(slot) = captures.get_mut(i) {
                slot.1 = value;
            }
        }
        Err(i) => captures.insert(i, (group, value)),
    }
}

fn get_capture(captures: &CaptureList, group: usize) -> Option<(usize, isize)> {
    captures
        .binary_search_by_key(&group, |(k, _)| *k)
        .ok()
        .and_then(|i| captures.get(i))
        .map(|(_, v)| *v)
}

/// Appends a choice to the path of every match. The new set must be built
/// again, because appending a bit does not keep the order.
fn add_choice(ms: &MatchSet, bit: bool) -> MatchSet {
    ms.iter()
        .map(|m| {
            let mut m = m.clone();
            m.path.push(bit);
            m
        })
        .collect()
}

fn add_choice_owned(ms: MatchSet, bit: bool) -> MatchSet {
    ms.into_iter()
        .map(|mut m| {
            m.path.push(bit);
            m
        })
        .collect()
}

/// Drops each match whose state equals the state of a preferred match.
fn dedup(ms: MatchSet) -> MatchSet {
    if ms.len() < 2 {
        return ms;
    }
    let mut seen = HashSet::new();
    ms.into_iter()
        .filter(|m| seen.insert(state_key(m)))
        .collect()
}

fn prune(ms: MatchSet) -> MatchSet {
    if ms.len() > SIZE_LIMIT {
        ms.into_iter().take(SIZE_LIMIT).collect()
    } else {
        ms
    }
}

/// Moves each match to a new offset, or drops it.
fn map_offsets(ms: MatchSet, mut f: impl FnMut(&Match) -> Option<usize>) -> MatchSet {
    ms.into_iter()
        .filter_map(|mut m| {
            m.offset = f(&m)?;
            Some(m)
        })
        .collect()
}

fn ci_prefix_len(capture: &str, rest: &str) -> Option<usize> {
    let mut rest_chars = rest.chars();
    let mut len = 0;
    for c in capture.chars() {
        let d = rest_chars.next()?;
        if simple_lower(d) != simple_lower(c) {
            return None;
        }
        len += d.len_utf8();
    }
    Some(len)
}

fn ci_suffix_offset(capture: &str, input: &str, mut offset: usize) -> Option<usize> {
    for c in capture.chars().rev() {
        let d = input.get(..offset)?.chars().next_back()?;
        if simple_lower(d) != simple_lower(c) {
            return None;
        }
        offset -= d.len_utf8();
    }
    Some(offset)
}

/// `%N` without substitution: the literal text `%N`.
fn dynamic_literal(n: usize) -> Regex {
    let parts = std::iter::once('%')
        .chain(n.to_string().chars())
        .map(|c| Regex::MatchChar(CharPred::is(c)))
        .collect();
    mconcat(parts)
}

struct Matcher<'a> {
    input: &'a str,
    groups: &'a HashMap<usize, Regex>,
    /// The subroutine calls (group, offset) in progress; a call cannot
    /// enter itself again without progress.
    active: HashSet<(usize, usize)>,
    steps: usize,
    max_steps: usize,
    depth: usize,
    /// The stack address at the start of the match call.
    stack_base: usize,
    /// For the ordered search: the states that reached each item of a
    /// sequence after the first one, by (item list address, index).
    boundary_counts: HashMap<(usize, usize), usize>,
    /// For the ordered search: the states that left each alternation, by
    /// the address of the node.
    alt_seen: HashMap<usize, HashSet<(usize, CaptureList)>>,
    /// The memory of the probe (see [`DeadEnds`]).
    dead_ends: &'a mut HashSet<(usize, usize)>,
    /// The offset of the input in its line, for the keys of `dead_ends`.
    base: usize,
    /// The dead ends that the probe in progress added.
    probe_log: Vec<(usize, usize)>,
    /// Whether the probe applies to the body of an assertion, by address.
    probe_bodies: HashMap<usize, bool>,
    /// The largest stack distance from `stack_base` seen so far.
    #[cfg(test)]
    peak_stack: usize,
}

/// An address in the current stack frame. The distance between two such
/// addresses is the stack that the frames between them use.
#[inline(always)]
fn stack_address() -> usize {
    let marker = 0u8;
    std::ptr::addr_of!(marker) as usize
}

impl Matcher<'_> {
    /// Charges the work budget, checks the depth and stack bounds, and
    /// runs one node. The arms only call helpers, which keeps the stack
    /// frame of this recursive function small.
    fn exec(&mut self, dir: Direction, re: &Regex, ms: MatchSet) -> Exec {
        self.steps = self.steps.saturating_add(ms.len() + 1);
        let stack = self.stack_base.abs_diff(stack_address());
        #[cfg(test)]
        {
            self.peak_stack = self.peak_stack.max(stack);
        }
        if self.steps > self.max_steps || self.depth >= MAX_EXEC_DEPTH || stack > MAX_EXEC_STACK {
            return Err(Abort);
        }
        self.depth += 1;
        let out = match re {
            Regex::MatchNull => Ok(ms),
            Regex::Lazy(inner) => match inner.as_ref() {
                Regex::MatchSome(r) => self.some_loop(dir, r, true, ms),
                other => self.exec(dir, other, ms),
            },
            Regex::Possessive(r) => self.possessive(dir, r, ms),
            Regex::MatchDynamic(n) => self.exec(dir, &dynamic_literal(*n), ms),
            Regex::AssertPositive(d, r) => self.positive(*d, r, ms),
            Regex::AssertNegative(d, r) => self.negative(*d, r, ms),
            Regex::MatchConcat(items) => self.concat(dir, items, ms),
            Regex::MatchAlt(..) => self.alt(dir, re, ms),
            Regex::MatchSome(r) => self.some_loop(dir, r, false, ms),
            Regex::MatchCapture(i, r) => self.capture(dir, *i, r, ms),
            Regex::Subroutine(i) => self.subroutine(dir, *i, ms),
            Regex::AssertEnd
            | Regex::AssertBeginning
            | Regex::AssertWordBoundary
            | Regex::MatchAnyChar
            | Regex::MatchChar(_)
            | Regex::MatchCaptured(..) => Ok(self.leaf(dir, re, ms)),
        };
        self.depth -= 1;
        out
    }

    /// The nodes without sub-patterns. A separate function keeps the stack
    /// frame of the recursive `exec_node` small.
    #[inline(never)]
    fn leaf(&self, dir: Direction, re: &Regex, ms: MatchSet) -> MatchSet {
        match re {
            Regex::AssertEnd => {
                let len = self.input.len();
                ms.into_iter().filter(|m| m.offset == len).collect()
            }
            Regex::AssertBeginning => ms.into_iter().filter(|m| m.offset == 0).collect(),
            Regex::AssertWordBoundary => ms
                .into_iter()
                .filter(|m| self.at_word_boundary(m.offset))
                .collect(),
            Regex::MatchAnyChar => map_offsets(ms, |m| self.step_char(dir, m.offset, |_| true)),
            Regex::MatchChar(p) => {
                map_offsets(ms, |m| self.step_char(dir, m.offset, |c| p.matches(c)))
            }
            Regex::MatchCaptured(n, cs) => map_offsets(ms, |m| self.captured(dir, *n, *cs, m)),
            _ => ms,
        }
    }

    /// The offset after one character that satisfies `f`, in direction
    /// `dir`.
    fn step_char(&self, dir: Direction, offset: usize, f: impl Fn(char) -> bool) -> Option<usize> {
        match dir {
            Direction::Forward => {
                let c = self.input.get(offset..)?.chars().next()?;
                f(c).then(|| offset + c.len_utf8())
            }
            Direction::Backward => {
                let c = self.input.get(..offset)?.chars().next_back()?;
                f(c).then(|| offset - c.len_utf8())
            }
        }
    }

    /// At byte 0 and at the end of the input there is always a boundary,
    /// as in skylighting.
    fn at_word_boundary(&self, offset: usize) -> bool {
        let prev = self.input.get(..offset).and_then(|s| s.chars().next_back());
        let next = self.input.get(offset..).and_then(|s| s.chars().next());
        match (prev, next) {
            (Some(p), Some(n)) => is_word_char(p) != is_word_char(n),
            _ => true,
        }
    }

    fn possessive(&mut self, dir: Direction, r: &Regex, ms: MatchSet) -> Exec {
        let mut out = MatchSet::new();
        for m in ms {
            // Haskell keeps the path of the entry. The entries of one call
            // reach this node at the same point of the pattern, so no path
            // is a zero-padded prefix of another, and the path of `first`
            // would give the same order.
            let path = m.path.clone();
            if let Some(first) = self.exec(dir, r, single(m))?.into_iter().next() {
                out.insert(Match { path, ..first });
            }
        }
        Ok(out)
    }

    /// Assertions are atomic: only the captures of the preferred match of
    /// the assertion are kept.
    fn positive(&mut self, dir: Direction, r: &Regex, ms: MatchSet) -> Exec {
        let mut out = MatchSet::new();
        for m in ms {
            if self.cannot_match(dir, r, &m) {
                continue;
            }
            if let Some(first) = self.exec(dir, r, single(m.clone()))?.into_iter().next() {
                out.insert(Match {
                    captures: first.captures,
                    ..m
                });
            }
        }
        Ok(out)
    }

    fn negative(&mut self, dir: Direction, r: &Regex, ms: MatchSet) -> Exec {
        let mut out = MatchSet::new();
        for m in ms {
            if self.cannot_match(dir, r, &m) || self.exec(dir, r, single(m.clone()))?.is_empty() {
                out.insert(m);
            }
        }
        Ok(out)
    }

    /// Forward, the set is pruned between the parts and an empty set stops
    /// the sequence. Backward, the parts run from the last to the first.
    fn concat(&mut self, dir: Direction, items: &[Regex], ms: MatchSet) -> Exec {
        let mut ms = ms;
        match dir {
            Direction::Forward => {
                for (i, r) in items.iter().enumerate() {
                    if i > 0 {
                        if ms.is_empty() {
                            return Ok(ms);
                        }
                        ms = prune(ms);
                    }
                    ms = self.exec(dir, r, ms)?;
                }
            }
            Direction::Backward => {
                for r in items.iter().rev() {
                    ms = self.exec(dir, r, ms)?;
                }
            }
        }
        Ok(ms)
    }

    /// Runs the right spine of nested alternatives in a loop. Because
    /// `dedup (a ∪ dedup (b ∪ c)) = dedup (a ∪ b ∪ c)`, one `dedup` at the
    /// end gives the result of the nested Haskell calls.
    fn alt(&mut self, dir: Direction, re: &Regex, ms: MatchSet) -> Exec {
        let mut acc = MatchSet::new();
        let mut node = re;
        let mut ms = ms;
        loop {
            match node {
                Regex::MatchAlt(left, right) => {
                    acc.extend(self.exec(dir, left, add_choice(&ms, false))?);
                    ms = add_choice_owned(ms, true);
                    node = right;
                }
                last => {
                    acc.extend(self.exec(dir, last, ms)?);
                    return Ok(dedup(acc));
                }
            }
        }
    }

    /// One or more repetitions. A greedy loop appends 0 to continue and 1 to
    /// stop; a lazy loop the reverse.
    fn some_loop(&mut self, dir: Direction, r: &Regex, lazy: bool, ms: MatchSet) -> Exec {
        let (cont, stop) = (lazy, !lazy);
        let mut ms = dedup(self.exec(dir, r, ms)?);
        let mut seen: HashSet<(usize, CaptureList)> = ms.iter().map(state_key).collect();
        // The paths of different iterations have different lengths, so the
        // results are distinct: collect them first and sort them once.
        let mut out: Vec<Match> = Vec::new();
        while !ms.is_empty() {
            let next = dedup(prune(self.exec(dir, r, add_choice(&ms, cont))?));
            // A state seen before has no new futures; this also ends a loop
            // whose body matches the empty string. The states of `next` are
            // unique, so one insert both tests and records each state.
            let new: MatchSet = next
                .into_iter()
                .filter(|m| seen.insert(state_key(m)))
                .collect();
            out.extend(ms.into_iter().map(|mut m| {
                m.path.push(stop);
                m
            }));
            ms = new;
        }
        Ok(out.into_iter().collect())
    }

    fn capture(&mut self, dir: Direction, group: usize, r: &Regex, ms: MatchSet) -> Exec {
        let mut out = MatchSet::new();
        for m in ms {
            let start = m.offset;
            for mut end in self.exec(dir, r, single(m))? {
                let len = end.offset as isize - start as isize;
                set_capture(&mut end.captures, group, (start, len));
                out.insert(end);
            }
        }
        Ok(out)
    }

    fn captured(&self, dir: Direction, group: usize, cs: bool, m: &Match) -> Option<usize> {
        let (off, len) = get_capture(&m.captures, group)?;
        let text = match usize::try_from(len) {
            Ok(len) if len > 0 => {
                let tail = self.input.get(off..)?;
                tail.get(..len.min(tail.len()))?
            }
            _ => "",
        };
        match dir {
            Direction::Forward => {
                let rest = self.input.get(m.offset..)?;
                if cs {
                    rest.starts_with(text).then(|| m.offset + text.len())
                } else {
                    ci_prefix_len(text, rest).map(|n| m.offset + n)
                }
            }
            Direction::Backward => {
                if cs {
                    let before = self.input.get(..m.offset)?;
                    before.ends_with(text).then(|| m.offset - text.len())
                } else {
                    ci_suffix_offset(text, self.input, m.offset)
                }
            }
        }
    }

    fn subroutine(&mut self, dir: Direction, group: usize, ms: MatchSet) -> Exec {
        let groups = self.groups;
        let Some(body) = groups.get(&group) else {
            // A call of a group that does not exist is ignored.
            return Ok(ms);
        };
        let mut acc = MatchSet::new();
        for m in ms {
            let key = (group, m.offset);
            if !self.active.insert(key) {
                continue;
            }
            let result = self.exec(dir, body, single(m));
            self.active.remove(&key);
            acc.extend(result?);
        }
        Ok(dedup(acc))
    }
}

/// The rest of the pattern after a node, for the ordered search.
enum Cont<'r, 'k> {
    /// The end of the pattern: the state is the match.
    Done,
    /// The items of a sequence from `index` on, then `next`.
    Seq {
        items: &'r [Regex],
        index: usize,
        next: &'k Cont<'r, 'k>,
    },
    /// The end of an alternation, then `next`.
    AltEnd { node: usize, next: &'k Cont<'r, 'k> },
}

/// Whether the ordered search gives the match of the set search for `re`.
///
/// The set search computes all states of a node at once and takes the
/// first final state in path order. The ordered search follows one state at
/// a time in path order and stops at the first final state. Both agree
/// when no repetition outside a capture, an assertion or a possessive
/// group can receive more than one state: a repetition shares its seen
/// states between its entry states, which the ordered search cannot see.
/// The ordered search copies the other set effects: the size limit of a
/// sequence and the duplicate removal of an alternation.
pub(crate) fn ordered_search_applies(re: &Regex) -> bool {
    spine_output(re, true, 0).is_some()
}

/// The ordered search walks the spine: sequences, alternations and
/// repetitions outside captures, assertions and possessive groups. Returns
/// whether the node gives at most one state, or `None` when a repetition
/// can receive more than one state.
fn spine_output(re: &Regex, single_in: bool, depth: usize) -> Option<bool> {
    if depth > MAX_FIRST_DEPTH {
        return None;
    }
    match re {
        Regex::MatchConcat(items) => items
            .iter()
            .try_fold(single_in, |s, r| spine_output(r, s, depth + 1)),
        Regex::MatchAlt(a, b) => {
            let sa = spine_output(a, single_in, depth + 1)?;
            let sb = spine_output(b, single_in, depth + 1)?;
            Some(single_in && sa && sb && disjoint(a, b))
        }
        Regex::MatchSome(_) => single_in.then_some(false),
        Regex::Lazy(inner) => spine_output(inner, single_in, depth + 1),
        Regex::MatchCapture(_, r) => {
            Some(single_in && spine_output(r, true, depth + 1) == Some(true))
        }
        Regex::Subroutine(_) => Some(false),
        Regex::MatchAnyChar
        | Regex::MatchDynamic(_)
        | Regex::MatchChar(_)
        | Regex::MatchCaptured(..)
        | Regex::AssertWordBoundary
        | Regex::AssertBeginning
        | Regex::AssertEnd
        | Regex::AssertPositive(..)
        | Regex::AssertNegative(..)
        | Regex::Possessive(_)
        | Regex::MatchNull => Some(single_in),
    }
}

/// True when no character can start a match of both patterns, so that at
/// most one of them matches at an offset.
fn disjoint(a: &Regex, b: &Regex) -> bool {
    let (Some(fa), Some(fb)) = (first_chars(a), first_chars(b)) else {
        return false;
    };
    let literals = |f: &FirstChars| -> Option<Vec<char>> {
        f.0.iter()
            .map(|p| match p {
                CharPred::Is(x) => char::from_u32(*x),
                _ => None,
            })
            .collect()
    };
    let apart = |lits: Vec<char>, other: &FirstChars| lits.iter().all(|c| !other.allows(*c));
    literals(&fa).is_some_and(|l| apart(l, &fb)) || literals(&fb).is_some_and(|l| apart(l, &fa))
}

/// `Ok(None)` is no match; `Err` sends the call back to the set search.
type Found = Result<Option<Match>, Abort>;

impl<'r> Matcher<'_> {
    /// The ordered search: the first final state of `re` and then `k`,
    /// from the single state `m`. Only for a pattern where
    /// [`ordered_search_applies`] holds.
    fn search(&mut self, re: &'r Regex, m: Match, k: &Cont<'r, '_>) -> Found {
        self.steps = self.steps.saturating_add(1);
        let stack = self.stack_base.abs_diff(stack_address());
        #[cfg(test)]
        {
            self.peak_stack = self.peak_stack.max(stack);
        }
        if self.steps > self.max_steps || self.depth >= MAX_EXEC_DEPTH || stack > MAX_EXEC_STACK {
            return Err(Abort);
        }
        self.depth += 1;
        let out = match re {
            Regex::MatchConcat(items) => self.proceed(
                m,
                &Cont::Seq {
                    items,
                    index: 0,
                    next: k,
                },
            ),
            Regex::MatchAlt(a, b) => self.search_alt(re, a, b, m, k),
            Regex::MatchSome(body) => self.search_repeat(body, false, m, k),
            Regex::Lazy(inner) => match inner.as_ref() {
                Regex::MatchSome(body) => self.search_repeat(body, true, m, k),
                other => self.search(other, m, k),
            },
            other => self.search_node(other, m, k),
        };
        self.depth -= 1;
        out
    }

    /// Continues the state `m` with `k`.
    fn proceed(&mut self, m: Match, k: &Cont<'r, '_>) -> Found {
        match k {
            Cont::Done => Ok(Some(m)),
            Cont::Seq { items, index, next } => match items.get(*index) {
                None => self.proceed(m, next),
                Some(item) => {
                    if *index > 0 {
                        let n = self
                            .boundary_counts
                            .entry((items.as_ptr() as usize, *index))
                            .or_insert(0);
                        *n += 1;
                        if *n > SIZE_LIMIT {
                            return Ok(None);
                        }
                    }
                    let rest = Cont::Seq {
                        items,
                        index: index + 1,
                        next,
                    };
                    self.search(item, m, &rest)
                }
            },
            Cont::AltEnd { node, next } => {
                if self
                    .alt_seen
                    .entry(*node)
                    .or_default()
                    .insert(state_key(&m))
                {
                    self.proceed(m, next)
                } else {
                    Ok(None)
                }
            }
        }
    }

    #[inline(never)]
    fn search_alt(
        &mut self,
        re: &Regex,
        a: &'r Regex,
        b: &'r Regex,
        m: Match,
        k: &Cont<'r, '_>,
    ) -> Found {
        let end = Cont::AltEnd {
            node: std::ptr::from_ref(re) as usize,
            next: k,
        };
        let mut left = m.clone();
        left.path.push(false);
        if let Some(found) = self.search(a, left, &end)? {
            return Ok(Some(found));
        }
        let mut right = m;
        right.path.push(true);
        self.search(b, right, &end)
    }

    /// A node that the set search runs on the single state `m`.
    #[inline(never)]
    fn search_node(&mut self, re: &'r Regex, m: Match, k: &Cont<'r, '_>) -> Found {
        for state in self.exec(Direction::Forward, re, single(m))? {
            if let Some(found) = self.proceed(state, k)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    /// One step of a repetition: the state after the body, or `Err` when
    /// the body gives more than one state.
    fn body_step(&mut self, body: &Regex, m: Match) -> Result<Option<Match>, Abort> {
        let mut out = dedup(self.exec(Direction::Forward, body, single(m))?).into_iter();
        let first = out.next();
        if out.next().is_some() {
            return Err(Abort);
        }
        Ok(first)
    }

    /// A repetition with one entry state, as `some_loop` computes it: the
    /// chain of states after 1, 2, ... iterations, each with the stop bit.
    /// A lazy loop prefers fewer iterations, a greedy loop more.
    #[inline(never)]
    fn search_repeat(&mut self, body: &Regex, lazy: bool, m: Match, k: &Cont<'r, '_>) -> Found {
        let (cont, stop) = (lazy, !lazy);
        let Some(first) = self.body_step(body, m)? else {
            return Ok(None);
        };
        let mut seen = HashSet::new();
        seen.insert(state_key(&first));
        if lazy {
            let mut current = first;
            loop {
                let mut out = current.clone();
                out.path.push(stop);
                if let Some(found) = self.proceed(out, k)? {
                    return Ok(Some(found));
                }
                current.path.push(cont);
                match self.body_step(body, current)? {
                    Some(next) if seen.insert(state_key(&next)) => current = next,
                    _ => return Ok(None),
                }
            }
        }
        let mut chain = vec![first];
        while let Some(last) = chain.last() {
            let mut next = last.clone();
            next.path.push(cont);
            match self.body_step(body, next)? {
                Some(n) if seen.insert(state_key(&n)) => chain.push(n),
                _ => break,
            }
        }
        for mut out in chain.into_iter().rev() {
            out.path.push(stop);
            if let Some(found) = self.proceed(out, k)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }
}

/// The rest of the body of an assertion after a node, for the probe.
enum Rest<'r, 'k> {
    /// The end of the body.
    End,
    /// The items of a sequence from `index` on, then `next`.
    Seq {
        items: &'r [Regex],
        index: usize,
        next: &'k Rest<'r, 'k>,
    },
    /// The end of a pass of a repetition of `body` that started at
    /// `entry`, then `next`.
    Pass {
        body: &'r Regex,
        entry: usize,
        next: &'k Rest<'r, 'k>,
    },
}

/// Whether the probe can prove that a pattern, or the body of an assertion,
/// has no match.
///
/// The probe follows each way through the pattern and looks only at
/// offsets. It ignores the paths, the captures, the size limit and the
/// duplicate removal of the set search. It lets `^`, `\b`, each
/// look-behind and each negative look-ahead pass, so that its answer for
/// an offset of the line does not change with the start of the call. A
/// positive look-ahead passes when the probe can match its body. Each
/// state of the set search is thus a state of the probe: when the probe
/// reaches no end, the set search gives the empty set. The reverse does
/// not hold, so a pattern that the probe can match goes to the ordered
/// search or the set search.
///
/// A back-reference, a subroutine call and `%N` consume text that the
/// offsets alone do not give, so the probe does not apply to them.
pub(crate) fn probe_applies(re: &Regex) -> bool {
    probe_applies_at(re, 0)
}

fn probe_applies_at(re: &Regex, depth: usize) -> bool {
    if depth > MAX_FIRST_DEPTH {
        return false;
    }
    let mut node = re;
    while let Regex::MatchAlt(a, b) = node {
        if !probe_applies_at(a, depth + 1) {
            return false;
        }
        node = b;
    }
    match node {
        Regex::MatchCaptured(..) | Regex::Subroutine(_) | Regex::MatchDynamic(_) => false,
        Regex::AssertPositive(..) | Regex::AssertNegative(..) => true,
        other => children(other)
            .iter()
            .all(|r| probe_applies_at(r, depth + 1)),
    }
}

#[cfg(test)]
thread_local! {
    /// When set, the set search checks each "no match" of the probe:
    /// (answers checked, answers that the set search contradicts).
    static PROBE_CHECK: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

/// Runs `f` with the check of the probe on, and returns its result with
/// the number of probe answers that the set search checked and the number
/// that it contradicted.
#[cfg(test)]
pub(crate) fn with_probe_check<T>(f: impl FnOnce() -> T) -> (T, usize, usize) {
    let before = PROBE_CHECK.replace(Some((0, 0)));
    let out = f();
    let (checked, wrong) = PROBE_CHECK.replace(before).unwrap_or((0, 0));
    if let Some((outer_checked, outer_wrong)) = before {
        PROBE_CHECK.set(Some((outer_checked + checked, outer_wrong + wrong)));
    }
    (out, checked, wrong)
}

impl Matcher<'_> {
    /// True when the probe proves that the assertion body `body` has no
    /// match from the state `m` (see [`probe_applies`]).
    fn cannot_match(&mut self, dir: Direction, body: &Regex, m: &Match) -> bool {
        dir == Direction::Forward && self.probe_applies_to(body) && self.probe(dir, body, m)
    }

    /// [`probe_applies`] for the body of an assertion, computed one time
    /// for each call.
    fn probe_applies_to(&mut self, body: &Regex) -> bool {
        let id = std::ptr::from_ref(body) as usize;
        match self.probe_bodies.get(&id) {
            Some(applies) => *applies,
            None => {
                let applies = probe_applies(body);
                self.probe_bodies.insert(id, applies);
                applies
            }
        }
    }

    /// Forgets the dead ends that the probe added after the log had
    /// `mark` entries. A dead end is certain only when the probe that
    /// found it fails: a pass that consumed nothing was not followed
    /// again, because the pass in progress follows the same states.
    fn forget_from(&mut self, mark: usize) {
        for key in self.probe_log.drain(mark..) {
            self.dead_ends.remove(&key);
        }
    }

    /// True when the probe proves that `re` has no match from the state
    /// `m`. Only for a pattern where [`probe_applies`] holds. The memory
    /// of dead ends keeps a scan of the same text from each start position
    /// linear.
    fn probe(&mut self, dir: Direction, re: &Regex, m: &Match) -> bool {
        self.probe_log.clear();
        let depth = self.depth;
        let dead = matches!(self.reach(re, m.offset, &Rest::End), Ok(false));
        self.depth = depth;
        if !dead {
            self.forget_from(0);
        }
        #[cfg(test)]
        if dead && let Some((checked, wrong)) = PROBE_CHECK.get() {
            let steps = self.steps;
            let found = self.exec(dir, re, single(m.clone()));
            self.steps = steps;
            self.depth = depth;
            if let Ok(ms) = found {
                PROBE_CHECK.set(Some((checked + 1, wrong + usize::from(!ms.is_empty()))));
            }
        }
        let _ = dir;
        dead
    }

    /// Whether a state at `offset` can pass `re` and then `k`. `Err` when
    /// the budget, the depth bound or the stack bound stops the probe.
    fn reach<'r>(&mut self, re: &'r Regex, offset: usize, k: &Rest<'r, '_>) -> Result<bool, Abort> {
        self.steps = self.steps.saturating_add(1);
        let stack = self.stack_base.abs_diff(stack_address());
        #[cfg(test)]
        {
            self.peak_stack = self.peak_stack.max(stack);
        }
        if self.steps > self.max_steps || self.depth >= MAX_EXEC_DEPTH || stack > MAX_EXEC_STACK {
            return Err(Abort);
        }
        self.depth += 1;
        let out = self.reach_node(re, offset, k);
        self.depth -= 1;
        out
    }

    fn reach_node<'r>(
        &mut self,
        re: &'r Regex,
        offset: usize,
        k: &Rest<'r, '_>,
    ) -> Result<bool, Abort> {
        let after_char =
            |this: &Self, f: &dyn Fn(char) -> bool| this.step_char(Direction::Forward, offset, f);
        let next = match re {
            Regex::AssertPositive(Direction::Forward, body) if self.probe_applies_to(body) => {
                let mark = self.probe_log.len();
                if !self.reach(body, offset, &Rest::End)? {
                    return Ok(false);
                }
                self.forget_from(mark);
                Some(offset)
            }
            Regex::MatchNull
            | Regex::AssertBeginning
            | Regex::AssertWordBoundary
            | Regex::AssertPositive(..)
            | Regex::AssertNegative(..) => Some(offset),
            Regex::MatchAnyChar => after_char(self, &|_| true),
            Regex::MatchChar(p) => after_char(self, &|c| p.matches(c)),
            Regex::AssertEnd => (offset == self.input.len()).then_some(offset),
            Regex::MatchConcat(items) => {
                let rest = Rest::Seq {
                    items,
                    index: 0,
                    next: k,
                };
                return self.resume(offset, &rest);
            }
            Regex::MatchAlt(..) => {
                let mut node = re;
                while let Regex::MatchAlt(a, b) = node {
                    if self.reach(a, offset, k)? {
                        return Ok(true);
                    }
                    node = b;
                }
                return self.reach(node, offset, k);
            }
            Regex::MatchSome(body) => return self.reach_repeat(body, offset, k),
            Regex::Lazy(inner) => {
                return match inner.as_ref() {
                    Regex::MatchSome(body) => self.reach_repeat(body, offset, k),
                    other => self.reach(other, offset, k),
                };
            }
            Regex::MatchCapture(_, r) | Regex::Possessive(r) => return self.reach(r, offset, k),
            Regex::MatchCaptured(..) | Regex::Subroutine(_) | Regex::MatchDynamic(_) => {
                return Err(Abort);
            }
        };
        match next {
            Some(offset) => self.resume(offset, k),
            None => Ok(false),
        }
    }

    /// Continues a state at `offset` with `k`.
    fn resume<'r>(&mut self, offset: usize, k: &Rest<'r, '_>) -> Result<bool, Abort> {
        match k {
            Rest::End => Ok(true),
            Rest::Seq { items, index, next } => match items.get(*index) {
                None => self.resume(offset, next),
                Some(item) => {
                    let rest = Rest::Seq {
                        items,
                        index: index + 1,
                        next,
                    };
                    self.reach(item, offset, &rest)
                }
            },
            Rest::Pass { body, entry, next } => {
                let key = (
                    std::ptr::from_ref::<Regex>(body) as usize,
                    self.base + offset,
                );
                if self.dead_ends.contains(&key) {
                    return Ok(false);
                }
                if self.resume(offset, next)? {
                    return Ok(true);
                }
                // A pass that consumed nothing leads to the states of the
                // pass in progress again.
                if offset != *entry {
                    let again = Rest::Pass {
                        body,
                        entry: offset,
                        next,
                    };
                    if self.reach(body, offset, &again)? {
                        return Ok(true);
                    }
                }
                self.dead_ends.insert(key);
                self.probe_log.push(key);
                Ok(false)
            }
        }
    }

    /// One or more passes of `body`, then `k`.
    fn reach_repeat<'r>(
        &mut self,
        body: &'r Regex,
        offset: usize,
        k: &Rest<'r, '_>,
    ) -> Result<bool, Abort> {
        let test: &dyn Fn(char) -> bool = match body {
            Regex::MatchChar(p) => &|c| p.matches(c),
            Regex::MatchAnyChar => &|_| true,
            _ => {
                let pass = Rest::Pass {
                    body,
                    entry: offset,
                    next: k,
                };
                return self.reach(body, offset, &pass);
            }
        };
        // A repetition of one character test runs in a loop, so that a
        // long run of characters does not use the stack. The shortest pass
        // comes first, so that a match near the start ends the scan. The
        // run stops at a dead end: no longer pass has a match from there.
        let id = std::ptr::from_ref(body) as usize;
        let mut end = offset;
        while let Some(next) = self.step_char(Direction::Forward, end, test) {
            if self.dead_ends.contains(&(id, self.base + next)) {
                break;
            }
            end = next;
            self.steps = self.steps.saturating_add(1);
            if self.steps > self.max_steps {
                return Err(Abort);
            }
            if self.resume(end, k)? {
                return Ok(true);
            }
        }
        let passed = self.input.get(offset..end).unwrap_or("");
        for (at, c) in passed.char_indices() {
            let key = (id, self.base + offset + at + c.len_utf8());
            self.dead_ends.insert(key);
            self.probe_log.push(key);
        }
        Ok(false)
    }
}

/// Unicode general categories (Unicode 16.0, from Python `unicodedata`).
/// Each entry is `first_code_point << 5 | category_index`; a range ends
/// where the next entry starts.
const GC_TABLE: &[u32] = &[
    25, 1046, 1073, 1171, 1201, 1293, 1326, 1361, 1394, 1425, 1452, 1489, 1544, 1873, 1938, 2033,
    2080, 2925, 2961, 2990, 3028, 3051, 3092, 3105, 3949, 3986, 4014, 4050, 4089, 5142, 5169, 5203,
    5333, 5361, 5396, 5429, 5444, 5487, 5522, 5562, 5589, 5620, 5653, 5682, 5706, 5780, 5793, 5841,
    5908, 5930, 5956, 6000, 6026, 6129, 6144, 6898, 6912, 7137, 7922, 7937, 8192, 8225, 8256, 8289,
    8320, 8353, 8384, 8417, 8448, 8481, 8512, 8545, 8576, 8609, 8640, 8673, 8704, 8737, 8768, 8801,
    8832, 8865, 8896, 8929, 8960, 8993, 9024, 9057, 9088, 9121, 9152, 9185, 9216, 9249, 9280, 9313,
    9344, 9377, 9408, 9441, 9472, 9505, 9536, 9569, 9600, 9633, 9664, 9697, 9728, 9761, 9792, 9825,
    9856, 9889, 9920, 9953, 10016, 10049, 10080, 10113, 10144, 10177, 10208, 10241, 10272, 10305,
    10336, 10369, 10400, 10433, 10464, 10497, 10560, 10593, 10624, 10657, 10688, 10721, 10752,
    10785, 10816, 10849, 10880, 10913, 10944, 10977, 11008, 11041, 11072, 11105, 11136, 11169,
    11200, 11233, 11264, 11297, 11328, 11361, 11392, 11425, 11456, 11489, 11520, 11553, 11584,
    11617, 11648, 11681, 11712, 11745, 11776, 11809, 11840, 11873, 11904, 11937, 11968, 12001,
    12032, 12097, 12128, 12161, 12192, 12225, 12320, 12385, 12416, 12449, 12480, 12545, 12576,
    12673, 12736, 12865, 12896, 12961, 12992, 13089, 13184, 13249, 13280, 13345, 13376, 13409,
    13440, 13473, 13504, 13569, 13600, 13633, 13696, 13729, 13760, 13825, 13856, 13953, 13984,
    14017, 14048, 14113, 14180, 14208, 14241, 14340, 14464, 14498, 14529, 14560, 14594, 14625,
    14656, 14690, 14721, 14752, 14785, 14816, 14849, 14880, 14913, 14944, 14977, 15008, 15041,
    15072, 15105, 15136, 15169, 15200, 15233, 15296, 15329, 15360, 15393, 15424, 15457, 15488,
    15521, 15552, 15585, 15616, 15649, 15680, 15713, 15744, 15777, 15808, 15841, 15904, 15938,
    15969, 16000, 16033, 16064, 16161, 16192, 16225, 16256, 16289, 16320, 16353, 16384, 16417,
    16448, 16481, 16512, 16545, 16576, 16609, 16640, 16673, 16704, 16737, 16768, 16801, 16832,
    16865, 16896, 16929, 16960, 16993, 17024, 17057, 17088, 17121, 17152, 17185, 17216, 17249,
    17280, 17313, 17344, 17377, 17408, 17441, 17472, 17505, 17536, 17569, 17600, 17633, 17664,
    17697, 17728, 17761, 17792, 17825, 17856, 17889, 17920, 17953, 17984, 18017, 18240, 18305,
    18336, 18401, 18464, 18497, 18528, 18657, 18688, 18721, 18752, 18785, 18816, 18849, 18880,
    18913, 21124, 21153, 22019, 22612, 22723, 23124, 23555, 23732, 23939, 23988, 24003, 24052,
    24581, 28160, 28193, 28224, 28257, 28291, 28340, 28352, 28385, 28445, 28483, 28513, 28625,
    28640, 28701, 28820, 28864, 28913, 28928, 29053, 29056, 29117, 29120, 29185, 29216, 29789,
    29792, 30081, 31200, 31233, 31296, 31393, 31488, 31521, 31552, 31585, 31616, 31649, 31680,
    31713, 31744, 31777, 31808, 31841, 31872, 31905, 31936, 31969, 32000, 32033, 32064, 32097,
    32128, 32161, 32192, 32225, 32384, 32417, 32466, 32480, 32513, 32544, 32609, 32672, 34305,
    35840, 35873, 35904, 35937, 35968, 36001, 36032, 36065, 36096, 36129, 36160, 36193, 36224,
    36257, 36288, 36321, 36352, 36385, 36416, 36449, 36480, 36513, 36544, 36577, 36608, 36641,
    36672, 36705, 36736, 36769, 36800, 36833, 36864, 36897, 36949, 36965, 37127, 37184, 37217,
    37248, 37281, 37312, 37345, 37376, 37409, 37440, 37473, 37504, 37537, 37568, 37601, 37632,
    37665, 37696, 37729, 37760, 37793, 37824, 37857, 37888, 37921, 37952, 37985, 38016, 38049,
    38080, 38113, 38144, 38177, 38208, 38241, 38272, 38305, 38336, 38369, 38400, 38433, 38464,
    38497, 38528, 38561, 38592, 38625, 38656, 38689, 38720, 38753, 38784, 38817, 38848, 38881,
    38912, 38977, 39008, 39041, 39072, 39105, 39136, 39169, 39200, 39233, 39264, 39297, 39328,
    39361, 39424, 39457, 39488, 39521, 39552, 39585, 39616, 39649, 39680, 39713, 39744, 39777,
    39808, 39841, 39872, 39905, 39936, 39969, 40000, 40033, 40064, 40097, 40128, 40161, 40192,
    40225, 40256, 40289, 40320, 40353, 40384, 40417, 40448, 40481, 40512, 40545, 40576, 40609,
    40640, 40673, 40704, 40737, 40768, 40801, 40832, 40865, 40896, 40929, 40960, 40993, 41024,
    41057, 41088, 41121, 41152, 41185, 41216, 41249, 41280, 41313, 41344, 41377, 41408, 41441,
    41472, 41505, 41536, 41569, 41600, 41633, 41664, 41697, 41728, 41761, 41792, 41825, 41856,
    41889, 41920, 41953, 41984, 42017, 42048, 42081, 42112, 42145, 42176, 42209, 42240, 42273,
    42304, 42337, 42368, 42401, 42432, 42465, 42525, 42528, 43773, 43811, 43857, 44033, 45361,
    45388, 45437, 45493, 45555, 45597, 45605, 47052, 47077, 47121, 47141, 47217, 47237, 47313,
    47333, 47389, 47620, 48509, 48612, 48753, 48829, 49178, 49362, 49457, 49523, 49553, 49621,
    49669, 50033, 50074, 50097, 50180, 51203, 51236, 51557, 52232, 52561, 52676, 52741, 52772,
    55953, 55972, 56005, 56250, 56277, 56293, 56483, 56549, 56629, 56645, 56772, 56840, 57156,
    57269, 57316, 57361, 57821, 57850, 57860, 57893, 57924, 58885, 59773, 59812, 62661, 63012,
    63069, 63496, 63812, 64869, 65155, 65237, 65265, 65347, 65405, 65445, 65491, 65540, 66245,
    66371, 66405, 66691, 66725, 66819, 66853, 67037, 67089, 67581, 67588, 68389, 68509, 68561,
    68605, 68612, 68989, 69124, 69908, 69924, 70141, 70170, 70237, 70373, 70660, 71971, 72005,
    72794, 72805, 73830, 73860, 75589, 75622, 75653, 75684, 75718, 75813, 76070, 76197, 76230,
    76292, 76325, 76548, 76869, 76945, 77000, 77329, 77347, 77380, 77861, 77894, 77981, 77988,
    78269, 78308, 78397, 78436, 79165, 79172, 79421, 79428, 79485, 79556, 79709, 79749, 79780,
    79814, 79909, 80061, 80102, 80189, 80230, 80293, 80324, 80381, 80614, 80669, 80772, 80861,
    80868, 80965, 81053, 81096, 81412, 81491, 81546, 81749, 81779, 81796, 81841, 81861, 81917,
    81957, 82022, 82077, 82084, 82301, 82404, 82493, 82532, 83261, 83268, 83517, 83524, 83613,
    83620, 83709, 83716, 83805, 83845, 83901, 83910, 84005, 84093, 84197, 84285, 84325, 84445,
    84517, 84573, 84772, 84925, 84932, 84989, 85192, 85509, 85572, 85669, 85713, 85757, 86053,
    86118, 86173, 86180, 86493, 86500, 86621, 86628, 87357, 87364, 87613, 87620, 87709, 87716,
    87901, 87941, 87972, 88006, 88101, 88285, 88293, 88358, 88413, 88422, 88485, 88541, 88580,
    88637, 89092, 89157, 89245, 89288, 89617, 89651, 89693, 89892, 89925, 90141, 90149, 90182,
    90269, 90276, 90557, 90596, 90685, 90724, 91453, 91460, 91709, 91716, 91805, 91812, 91997,
    92037, 92068, 92102, 92133, 92166, 92197, 92349, 92390, 92477, 92518, 92581, 92637, 92837,
    92902, 92957, 93060, 93149, 93156, 93253, 93341, 93384, 93717, 93732, 93770, 93981, 94277,
    94308, 94365, 94372, 94589, 94660, 94781, 94788, 94941, 95012, 95101, 95108, 95165, 95172,
    95261, 95332, 95421, 95492, 95613, 95684, 96093, 96198, 96261, 96294, 96381, 96454, 96573,
    96582, 96677, 96733, 96772, 96829, 96998, 97053, 97480, 97802, 97909, 98099, 98133, 98173,
    98309, 98342, 98437, 98468, 98749, 98756, 98877, 98884, 99645, 99652, 100189, 100229, 100260,
    100293, 100390, 100541, 100549, 100669, 100677, 100829, 101029, 101117, 101124, 101245, 101284,
    101341, 101380, 101445, 101533, 101576, 101917, 102129, 102154, 102389, 102404, 102437, 102470,
    102545, 102564, 102845, 102852, 102973, 102980, 103741, 103748, 104093, 104100, 104285, 104325,
    104356, 104390, 104421, 104454, 104637, 104645, 104678, 104765, 104774, 104837, 104925, 105126,
    105213, 105380, 105469, 105476, 105541, 105629, 105672, 106013, 106020, 106086, 106141, 106501,
    106566, 106628, 106941, 106948, 107069, 107076, 108389, 108452, 108486, 108581, 108733, 108742,
    108861, 108870, 108965, 108996, 109045, 109085, 109188, 109286, 109322, 109540, 109637, 109725,
    109768, 110090, 110389, 110404, 110621, 110629, 110662, 110749, 110756, 111357, 111428, 112221,
    112228, 112541, 112548, 112605, 112644, 112893, 112965, 113021, 113126, 113221, 113341, 113349,
    113405, 113414, 113693, 113864, 114205, 114246, 114321, 114365, 114724, 116261, 116292, 116357,
    116605, 116723, 116740, 116931, 116965, 117233, 117256, 117585, 117661, 118820, 118909, 118916,
    118973, 118980, 119165, 119172, 119965, 119972, 120029, 120036, 120357, 120388, 120453, 120740,
    120797, 120836, 121021, 121027, 121085, 121093, 121341, 121352, 121693, 121732, 121885, 122884,
    122933, 123025, 123509, 123537, 123573, 123653, 123733, 123912, 124234, 124565, 124581, 124629,
    124645, 124693, 124709, 124749, 124782, 124813, 124846, 124870, 124932, 125213, 125220, 126397,
    126501, 126950, 126981, 127153, 127173, 127236, 127397, 127773, 127781, 128957, 128981, 129221,
    129269, 129469, 129493, 129553, 129717, 129841, 129917, 131076, 132454, 132517, 132646, 132677,
    132870, 132901, 132966, 133029, 133092, 133128, 133457, 133636, 133830, 133893, 133956, 134085,
    134180, 134214, 134308, 134374, 134596, 134693, 134820, 135237, 135270, 135333, 135398, 135589,
    135620, 135654, 135688, 136006, 136101, 136149, 136192, 137437, 137440, 137501, 137632, 137693,
    137729, 139121, 139139, 139169, 139268, 149821, 149828, 149981, 150020, 150269, 150276, 150333,
    150340, 150493, 150532, 151869, 151876, 152029, 152068, 153149, 153156, 153309, 153348, 153597,
    153604, 153661, 153668, 153821, 153860, 154365, 154372, 156221, 156228, 156381, 156420, 158589,
    158629, 158737, 159018, 159677, 159748, 160277, 160605, 160768, 163549, 163585, 163805, 163852,
    163876, 183733, 183761, 183780, 184342, 184356, 185197, 185230, 185277, 185348, 187761, 187849,
    187940, 188221, 188420, 188997, 189094, 189149, 189412, 190021, 190086, 190129, 190205, 190468,
    191045, 191133, 191492, 191933, 191940, 192061, 192069, 192157, 192516, 194181, 194246, 194277,
    194502, 194757, 194790, 194853, 195217, 195299, 195345, 195443, 195460, 195493, 195549, 195592,
    195933, 196106, 196445, 196625, 196812, 196849, 196965, 197082, 197093, 197128, 197469, 197636,
    198755, 198788, 200509, 200708, 200869, 200932, 202021, 202052, 202109, 202244, 204509, 204804,
    205821, 205829, 205926, 206053, 206118, 206237, 206342, 206405, 206438, 206629, 206749, 206869,
    206909, 206993, 207048, 207364, 208349, 208388, 208573, 208900, 210333, 210436, 211293, 211464,
    211786, 211837, 211925, 212996, 213733, 213798, 213861, 213917, 213969, 214020, 215718, 215749,
    215782, 215813, 216061, 216069, 216102, 216133, 216166, 216229, 216486, 216677, 217021, 217061,
    217096, 217437, 217608, 217949, 218129, 218339, 218385, 218589, 218629, 219079, 219109, 219645,
    221189, 221318, 221348, 222853, 222886, 222917, 223078, 223109, 223142, 223301, 223334, 223396,
    223677, 223697, 223752, 224081, 224309, 224613, 224917, 225201, 225285, 225350, 225380, 226342,
    226373, 226502, 226565, 226630, 226661, 226756, 226824, 227140, 228549, 228582, 228613, 228678,
    228773, 228806, 228837, 228934, 229021, 229265, 229380, 230534, 230789, 231046, 231109, 231197,
    231281, 231432, 231773, 231844, 231944, 232260, 233219, 233425, 233473, 233760, 233793, 233853,
    233984, 235389, 235424, 235537, 235805, 236037, 236145, 236165, 236582, 236613, 236836, 236965,
    236996, 237189, 237220, 237286, 237317, 237380, 237437, 237569, 238979, 240993, 241411, 241441,
    242531, 243717, 245760, 245793, 245824, 245857, 245888, 245921, 245952, 245985, 246016, 246049,
    246080, 246113, 246144, 246177, 246208, 246241, 246272, 246305, 246336, 246369, 246400, 246433,
    246464, 246497, 246528, 246561, 246592, 246625, 246656, 246689, 246720, 246753, 246784, 246817,
    246848, 246881, 246912, 246945, 246976, 247009, 247040, 247073, 247104, 247137, 247168, 247201,
    247232, 247265, 247296, 247329, 247360, 247393, 247424, 247457, 247488, 247521, 247552, 247585,
    247616, 247649, 247680, 247713, 247744, 247777, 247808, 247841, 247872, 247905, 247936, 247969,
    248000, 248033, 248064, 248097, 248128, 248161, 248192, 248225, 248256, 248289, 248320, 248353,
    248384, 248417, 248448, 248481, 248512, 248545, 248576, 248609, 248640, 248673, 248704, 248737,
    248768, 248801, 248832, 248865, 248896, 248929, 248960, 248993, 249024, 249057, 249088, 249121,
    249152, 249185, 249216, 249249, 249280, 249313, 249344, 249377, 249408, 249441, 249472, 249505,
    249536, 249569, 249600, 249633, 249664, 249697, 249728, 249761, 249792, 249825, 249856, 249889,
    249920, 249953, 249984, 250017, 250048, 250081, 250112, 250145, 250176, 250209, 250240, 250273,
    250304, 250337, 250368, 250401, 250432, 250465, 250496, 250529, 250816, 250849, 250880, 250913,
    250944, 250977, 251008, 251041, 251072, 251105, 251136, 251169, 251200, 251233, 251264, 251297,
    251328, 251361, 251392, 251425, 251456, 251489, 251520, 251553, 251584, 251617, 251648, 251681,
    251712, 251745, 251776, 251809, 251840, 251873, 251904, 251937, 251968, 252001, 252032, 252065,
    252096, 252129, 252160, 252193, 252224, 252257, 252288, 252321, 252352, 252385, 252416, 252449,
    252480, 252513, 252544, 252577, 252608, 252641, 252672, 252705, 252736, 252769, 252800, 252833,
    252864, 252897, 252928, 252961, 252992, 253025, 253056, 253089, 253120, 253153, 253184, 253217,
    253248, 253281, 253312, 253345, 253376, 253409, 253440, 253473, 253504, 253537, 253568, 253601,
    253632, 253665, 253696, 253729, 253760, 253793, 253824, 253857, 253888, 253921, 254208, 254465,
    254685, 254720, 254941, 254977, 255232, 255489, 255744, 256001, 256221, 256256, 256477, 256513,
    256797, 256800, 256861, 256864, 256925, 256928, 256989, 256992, 257025, 257280, 257537, 258013,
    258049, 258306, 258561, 258818, 259073, 259330, 259585, 259773, 259777, 259840, 259970, 260020,
    260033, 260084, 260161, 260285, 260289, 260352, 260482, 260532, 260609, 260765, 260801, 260864,
    261021, 261044, 261121, 261376, 261556, 261661, 261697, 261821, 261825, 261888, 262018, 262068,
    262141, 262166, 262522, 262668, 262865, 262927, 262960, 262989, 263023, 263088, 263117, 263151,
    263185, 263447, 263480, 263514, 263670, 263697, 263983, 264016, 264049, 264171, 264241, 264338,
    264365, 264398, 264433, 264786, 264817, 264843, 264881, 265206, 265242, 265405, 265434, 265738,
    265763, 265821, 265866, 266066, 266157, 266190, 266211, 266250, 266578, 266669, 266702, 266749,
    266755, 267197, 267283, 268349, 268805, 269223, 269349, 269383, 269477, 269885, 270357, 270400,
    270453, 270560, 270613, 270657, 270688, 270785, 270848, 270945, 270997, 271008, 271061, 271122,
    271136, 271317, 271488, 271541, 271552, 271605, 271616, 271669, 271680, 271829, 271841, 271872,
    272001, 272036, 272161, 272213, 272257, 272320, 272402, 272544, 272577, 272725, 272754, 272789,
    272833, 272885, 272906, 273417, 274528, 274561, 274601, 274730, 274773, 274845, 274962, 275125,
    275282, 275349, 275474, 275509, 275570, 275605, 275666, 275701, 275922, 275957, 276946, 277013,
    277074, 277109, 277138, 277173, 278162, 286741, 286989, 287022, 287053, 287086, 287125, 287762,
    287829, 288045, 288078, 288117, 290706, 290741, 291698, 292501, 293778, 293973, 296285, 296981,
    297341, 297994, 299925, 302410, 303125, 308978, 309013, 309298, 309333, 311058, 311317, 314866,
    314901, 322829, 322862, 322893, 322926, 322957, 322990, 323021, 323054, 323085, 323118, 323149,
    323182, 323213, 323246, 323274, 324245, 325650, 325805, 325838, 325874, 326861, 326894, 326925,
    326958, 326989, 327022, 327053, 327086, 327117, 327150, 327186, 327701, 335890, 340077, 340110,
    340141, 340174, 340205, 340238, 340269, 340302, 340333, 340366, 340397, 340430, 340461, 340494,
    340525, 340558, 340589, 340622, 340653, 340686, 340717, 340750, 340786, 342797, 342830, 342861,
    342894, 342930, 343949, 343982, 344018, 352277, 353810, 354485, 354546, 354741, 355997, 356053,
    357085, 357109, 360448, 361985, 363520, 363553, 363584, 363681, 363744, 363777, 363808, 363841,
    363872, 363905, 363936, 364065, 364096, 364129, 364192, 364225, 364419, 364480, 364577, 364608,
    364641, 364672, 364705, 364736, 364769, 364800, 364833, 364864, 364897, 364928, 364961, 364992,
    365025, 365056, 365089, 365120, 365153, 365184, 365217, 365248, 365281, 365312, 365345, 365376,
    365409, 365440, 365473, 365504, 365537, 365568, 365601, 365632, 365665, 365696, 365729, 365760,
    365793, 365824, 365857, 365888, 365921, 365952, 365985, 366016, 366049, 366080, 366113, 366144,
    366177, 366208, 366241, 366272, 366305, 366336, 366369, 366400, 366433, 366464, 366497, 366528,
    366561, 366592, 366625, 366656, 366689, 366720, 366753, 366784, 366817, 366848, 366881, 366912,
    366945, 366976, 367009, 367040, 367073, 367104, 367137, 367168, 367201, 367232, 367265, 367296,
    367329, 367360, 367393, 367424, 367457, 367488, 367521, 367552, 367585, 367616, 367649, 367680,
    367713, 367797, 367968, 368001, 368032, 368065, 368101, 368192, 368225, 368285, 368433, 368554,
    368593, 368641, 369885, 369889, 369949, 370081, 370141, 370180, 371997, 372195, 372241, 372285,
    372709, 372740, 373501, 373764, 374013, 374020, 374269, 374276, 374525, 374532, 374781, 374788,
    375037, 375044, 375293, 375300, 375549, 375556, 375805, 375813, 376849, 376911, 376944, 376975,
    377008, 377041, 377135, 377168, 377201, 377231, 377264, 377297, 377580, 377617, 377676, 377713,
    377743, 377776, 377809, 377871, 377904, 377933, 377966, 377997, 378030, 378061, 378094, 378125,
    378158, 378193, 378339, 378385, 378700, 378769, 378892, 378929, 378957, 378993, 379413, 379473,
    379565, 379598, 379629, 379662, 379693, 379726, 379757, 379790, 379820, 379869, 380949, 381789,
    381813, 384669, 385045, 391901, 392725, 393238, 393265, 393365, 393379, 393412, 393449, 393485,
    393518, 393549, 393582, 393613, 393646, 393677, 393710, 393741, 393774, 393813, 393869, 393902,
    393933, 393966, 393997, 394030, 394061, 394094, 394124, 394157, 394190, 394261, 394281, 394565,
    394694, 394764, 394787, 394965, 395017, 395107, 395140, 395185, 395221, 395293, 395300, 398077,
    398117, 398196, 398243, 398308, 398348, 398372, 401265, 401283, 401380, 401437, 401572, 402973,
    402980, 406013, 406037, 406090, 406229, 406532, 407573, 408797, 409077, 409092, 409621, 410621,
    410634, 410965, 411914, 412181, 412202, 412693, 413706, 414037, 415274, 415765, 425988, 636949,
    638980, 1311395, 1311428, 1348029, 1348117, 1349885, 1350148, 1351427, 1351633, 1351684,
    1360259, 1360305, 1360388, 1360904, 1361220, 1361309, 1361920, 1361953, 1361984, 1362017,
    1362048, 1362081, 1362112, 1362145, 1362176, 1362209, 1362240, 1362273, 1362304, 1362337,
    1362368, 1362401, 1362432, 1362465, 1362496, 1362529, 1362560, 1362593, 1362624, 1362657,
    1362688, 1362721, 1362752, 1362785, 1362816, 1362849, 1362880, 1362913, 1362944, 1362977,
    1363008, 1363041, 1363072, 1363105, 1363136, 1363169, 1363200, 1363233, 1363264, 1363297,
    1363328, 1363361, 1363396, 1363429, 1363463, 1363569, 1363589, 1363921, 1363939, 1363968,
    1364001, 1364032, 1364065, 1364096, 1364129, 1364160, 1364193, 1364224, 1364257, 1364288,
    1364321, 1364352, 1364385, 1364416, 1364449, 1364480, 1364513, 1364544, 1364577, 1364608,
    1364641, 1364672, 1364705, 1364736, 1364769, 1364800, 1364833, 1364867, 1364933, 1364996,
    1367241, 1367557, 1367633, 1367837, 1368084, 1368803, 1369108, 1369152, 1369185, 1369216,
    1369249, 1369280, 1369313, 1369344, 1369377, 1369408, 1369441, 1369472, 1369505, 1369536,
    1369569, 1369664, 1369697, 1369728, 1369761, 1369792, 1369825, 1369856, 1369889, 1369920,
    1369953, 1369984, 1370017, 1370048, 1370081, 1370112, 1370145, 1370176, 1370209, 1370240,
    1370273, 1370304, 1370337, 1370368, 1370401, 1370432, 1370465, 1370496, 1370529, 1370560,
    1370593, 1370624, 1370657, 1370688, 1370721, 1370752, 1370785, 1370816, 1370849, 1370880,
    1370913, 1370944, 1370977, 1371008, 1371041, 1371072, 1371105, 1371136, 1371169, 1371200,
    1371233, 1371264, 1371297, 1371328, 1371361, 1371392, 1371425, 1371456, 1371489, 1371520,
    1371553, 1371584, 1371617, 1371651, 1371681, 1371936, 1371969, 1372000, 1372033, 1372064,
    1372129, 1372160, 1372193, 1372224, 1372257, 1372288, 1372321, 1372352, 1372385, 1372419,
    1372468, 1372512, 1372545, 1372576, 1372609, 1372644, 1372672, 1372705, 1372736, 1372769,
    1372864, 1372897, 1372928, 1372961, 1372992, 1373025, 1373056, 1373089, 1373120, 1373153,
    1373184, 1373217, 1373248, 1373281, 1373312, 1373345, 1373376, 1373409, 1373440, 1373473,
    1373504, 1373665, 1373696, 1373857, 1373888, 1373921, 1373952, 1373985, 1374016, 1374049,
    1374080, 1374113, 1374144, 1374177, 1374208, 1374241, 1374272, 1374305, 1374336, 1374465,
    1374496, 1374529, 1374560, 1374625, 1374685, 1374720, 1374753, 1374813, 1374817, 1374877,
    1374881, 1374912, 1374945, 1374976, 1375009, 1375040, 1375073, 1375104, 1375165, 1375811,
    1375904, 1375937, 1375972, 1376003, 1376065, 1376100, 1376325, 1376356, 1376453, 1376484,
    1376613, 1376644, 1377382, 1377445, 1377510, 1377557, 1377669, 1377725, 1377802, 1378005,
    1378067, 1378101, 1378141, 1378308, 1379985, 1380125, 1380358, 1380420, 1382022, 1382533,
    1382621, 1382865, 1382920, 1383261, 1383429, 1384004, 1384209, 1384292, 1384337, 1384356,
    1384421, 1384456, 1384772, 1385669, 1385937, 1385988, 1386725, 1387078, 1387165, 1387505,
    1387524, 1388477, 1388549, 1388646, 1388676, 1390181, 1390214, 1390277, 1390406, 1390469,
    1390534, 1390641, 1391069, 1391075, 1391112, 1391453, 1391569, 1391620, 1391781, 1391811,
    1391844, 1392136, 1392452, 1392637, 1392644, 1393957, 1394150, 1394213, 1394278, 1394341,
    1394429, 1394692, 1394789, 1394820, 1395077, 1395110, 1395165, 1395208, 1395549, 1395601,
    1395716, 1396227, 1396260, 1396469, 1396548, 1396582, 1396613, 1396646, 1396676, 1398277,
    1398308, 1398341, 1398436, 1398501, 1398564, 1398725, 1398788, 1398821, 1398852, 1398909,
    1399652, 1399715, 1399761, 1399812, 1400166, 1400197, 1400262, 1400337, 1400388, 1400419,
    1400486, 1400517, 1400573, 1400868, 1401085, 1401124, 1401341, 1401380, 1401597, 1401860,
    1402109, 1402116, 1402365, 1402369, 1403764, 1403779, 1403905, 1404195, 1404244, 1404317,
    1404417, 1406980, 1408102, 1408165, 1408198, 1408261, 1408294, 1408369, 1408390, 1408421,
    1408477, 1408520, 1408861, 1409028, 1766557, 1766916, 1767677, 1767780, 1769373, 1769499,
    1835036, 2039812, 2051549, 2051588, 2055005, 2056193, 2056445, 2056801, 2056989, 2057124,
    2057157, 2057188, 2057522, 2057540, 2057981, 2057988, 2058173, 2058180, 2058237, 2058244,
    2058333, 2058340, 2058429, 2058436, 2061908, 2062461, 2062948, 2074574, 2074605, 2074645,
    2075140, 2077213, 2077252, 2079005, 2079221, 2079261, 2080260, 2080659, 2080693, 2080773,
    2081297, 2081517, 2081550, 2081585, 2081629, 2081797, 2082321, 2082348, 2082411, 2082477,
    2082510, 2082541, 2082574, 2082605, 2082638, 2082669, 2082702, 2082733, 2082766, 2082797,
    2082830, 2082861, 2082894, 2082925, 2082958, 2082993, 2083053, 2083086, 2083121, 2083243,
    2083345, 2083453, 2083473, 2083596, 2083629, 2083662, 2083693, 2083726, 2083757, 2083790,
    2083825, 2083922, 2083948, 2083986, 2084093, 2084113, 2084147, 2084177, 2084253, 2084356,
    2084541, 2084548, 2088893, 2088954, 2088989, 2089009, 2089107, 2089137, 2089229, 2089262,
    2089297, 2089330, 2089361, 2089388, 2089425, 2089480, 2089809, 2089874, 2089969, 2090016,
    2090861, 2090897, 2090926, 2090964, 2090987, 2091028, 2091041, 2091885, 2091922, 2091950,
    2091986, 2092013, 2092046, 2092081, 2092109, 2092142, 2092177, 2092228, 2092547, 2092580,
    2094019, 2094084, 2095101, 2095172, 2095389, 2095428, 2095645, 2095684, 2095901, 2095940,
    2096061, 2096147, 2096210, 2096244, 2096277, 2096307, 2096381, 2096405, 2096434, 2096565,
    2096637, 2096954, 2097045, 2097117, 2097156, 2097565, 2097572, 2098429, 2098436, 2099069,
    2099076, 2099165, 2099172, 2099677, 2099716, 2100189, 2101252, 2105213, 2105361, 2105469,
    2105578, 2107037, 2107125, 2107401, 2109098, 2109237, 2109770, 2109845, 2109949, 2109973,
    2110397, 2110485, 2110525, 2112021, 2113445, 2113501, 2117636, 2118589, 2118660, 2120253,
    2120709, 2120746, 2121629, 2121732, 2122762, 2122909, 2123172, 2123817, 2123844, 2124105,
    2124157, 2124292, 2125509, 2125693, 2125828, 2126813, 2126833, 2126852, 2128029, 2128132,
    2128401, 2128425, 2128605, 2129920, 2131201, 2132484, 2135005, 2135048, 2135389, 2135552,
    2136733, 2136833, 2138013, 2138116, 2139421, 2139652, 2141341, 2141681, 2141696, 2142077,
    2142080, 2142589, 2142592, 2142845, 2142848, 2142941, 2142945, 2143325, 2143329, 2143837,
    2143841, 2144093, 2144097, 2144189, 2144260, 2145949, 2146308, 2156285, 2156548, 2157277,
    2157572, 2157853, 2158595, 2158813, 2158819, 2160189, 2160195, 2160509, 2162692, 2162909,
    2162948, 2163005, 2163012, 2164445, 2164452, 2164541, 2164612, 2164669, 2164708, 2165469,
    2165489, 2165514, 2165764, 2166517, 2166570, 2166788, 2167805, 2168042, 2168349, 2169860,
    2170493, 2170500, 2170589, 2170730, 2170884, 2171594, 2171805, 2171889, 2171908, 2172765,
    2172913, 2172957, 2174980, 2176797, 2176906, 2176964, 2177034, 2177565, 2177610, 2179076,
    2179109, 2179229, 2179237, 2179325, 2179461, 2179588, 2179741, 2179748, 2179869, 2179876,
    2180829, 2180869, 2180989, 2181093, 2181130, 2181437, 2181649, 2181949, 2182148, 2183082,
    2183153, 2183172, 2184106, 2184221, 2185220, 2185493, 2185508, 2186405, 2186493, 2186602,
    2186769, 2187005, 2187268, 2189021, 2189105, 2189316, 2190045, 2190090, 2190340, 2190973,
    2191114, 2191364, 2191965, 2192177, 2192317, 2192682, 2192925, 2195460, 2197821, 2199552,
    2201213, 2201601, 2203261, 2203466, 2203652, 2204805, 2204957, 2205192, 2205533, 2205704,
    2206020, 2206147, 2206180, 2206208, 2206941, 2207013, 2207180, 2207203, 2207233, 2207965,
    2208210, 2208285, 2214922, 2215933, 2215940, 2217309, 2217317, 2217388, 2217437, 2217476,
    2217565, 2218052, 2218173, 2219909, 2220036, 2220970, 2221284, 2221341, 2221572, 2222277,
    2222634, 2222769, 2222941, 2223620, 2224197, 2224337, 2224477, 2225668, 2226346, 2226589,
    2227204, 2227965, 2228230, 2228261, 2228294, 2228324, 2230021, 2230513, 2230749, 2230858,
    2231496, 2231813, 2231844, 2231909, 2231972, 2232029, 2232293, 2232390, 2232420, 2233862,
    2233957, 2234086, 2234149, 2234225, 2234298, 2234321, 2234437, 2234493, 2234810, 2234845,
    2234884, 2235709, 2235912, 2236253, 2236421, 2236516, 2237669, 2237830, 2237861, 2238141,
    2238152, 2238481, 2238596, 2238630, 2238692, 2238749, 2238980, 2240101, 2240145, 2240196,
    2240253, 2240517, 2240582, 2240612, 2242150, 2242245, 2242534, 2242596, 2242737, 2242853,
    2242993, 2243014, 2243045, 2243080, 2243396, 2243441, 2243460, 2243505, 2243613, 2243626,
    2244285, 2244612, 2245213, 2245220, 2246022, 2246117, 2246214, 2246277, 2246310, 2246341,
    2246417, 2246597, 2246628, 2246693, 2246749, 2248708, 2248957, 2248964, 2249021, 2249028,
    2249181, 2249188, 2249693, 2249700, 2250033, 2250077, 2250244, 2251749, 2251782, 2251877,
    2252157, 2252296, 2252637, 2252805, 2252870, 2252957, 2252964, 2253245, 2253284, 2253373,
    2253412, 2254141, 2254148, 2254397, 2254404, 2254493, 2254500, 2254685, 2254693, 2254756,
    2254790, 2254853, 2254886, 2255037, 2255078, 2255165, 2255206, 2255325, 2255364, 2255421,
    2255590, 2255645, 2255780, 2255942, 2256029, 2256069, 2256317, 2256389, 2256573, 2256900,
    2257245, 2257252, 2257309, 2257348, 2257405, 2257412, 2258653, 2258660, 2258694, 2258789,
    2259005, 2259014, 2259069, 2259110, 2259165, 2259174, 2259325, 2259334, 2259397, 2259430,
    2259461, 2259492, 2259525, 2259556, 2259601, 2259677, 2259697, 2259773, 2260005, 2260093,
    2260996, 2262694, 2262789, 2263046, 2263109, 2263206, 2263237, 2263268, 2263409, 2263560,
    2263889, 2263965, 2263985, 2264005, 2264036, 2264157, 2265092, 2266630, 2266725, 2266918,
    2266949, 2266982, 2267109, 2267174, 2267205, 2267268, 2267345, 2267364, 2267421, 2267656,
    2267997, 2273284, 2274790, 2274885, 2275037, 2275078, 2275205, 2275270, 2275301, 2275377,
    2276100, 2276229, 2276317, 2277380, 2278918, 2279013, 2279270, 2279333, 2279366, 2279397,
    2279473, 2279556, 2279613, 2279944, 2280285, 2280465, 2280893, 2281476, 2282853, 2282886,
    2282917, 2282950, 2283013, 2283206, 2283237, 2283268, 2283313, 2283357, 2283528, 2283869,
    2284040, 2284701, 2285572, 2286461, 2286501, 2286534, 2286565, 2286598, 2286661, 2286790,
    2286821, 2287005, 2287112, 2287434, 2287505, 2287605, 2287620, 2287869, 2293764, 2295174,
    2295269, 2295558, 2295589, 2295665, 2295709, 2298880, 2299905, 2300936, 2301258, 2301565,
    2301924, 2302205, 2302244, 2302301, 2302340, 2302621, 2302628, 2302717, 2302724, 2303494,
    2303709, 2303718, 2303805, 2303845, 2303910, 2303941, 2303972, 2304006, 2304036, 2304070,
    2304101, 2304145, 2304253, 2304520, 2304861, 2307076, 2307357, 2307396, 2308646, 2308741,
    2308893, 2308933, 2308998, 2309125, 2309156, 2309201, 2309220, 2309254, 2309309, 2310148,
    2310181, 2310500, 2311781, 2311974, 2312004, 2312037, 2312177, 2312421, 2312477, 2312708,
    2312741, 2312934, 2312997, 2313092, 2314565, 2314982, 2315013, 2315089, 2315172, 2315217,
    2315389, 2315780, 2318141, 2318353, 2318685, 2324484, 2325553, 2325597, 2326024, 2326365,
    2326532, 2326845, 2326852, 2328038, 2328069, 2328317, 2328325, 2328518, 2328549, 2328580,
    2328625, 2328797, 2329096, 2329418, 2330045, 2330129, 2330180, 2331165, 2331205, 2331933,
    2331942, 2331973, 2332198, 2332229, 2332294, 2332325, 2332413, 2334724, 2334973, 2334980,
    2335069, 2335076, 2336293, 2336509, 2336581, 2336637, 2336645, 2336733, 2336741, 2336964,
    2336997, 2337053, 2337288, 2337629, 2337796, 2338013, 2338020, 2338109, 2338116, 2339142,
    2339325, 2339333, 2339421, 2339430, 2339493, 2339526, 2339557, 2339588, 2339645, 2339848,
    2340189, 2350084, 2350693, 2350758, 2350833, 2350909, 2351109, 2351172, 2351206, 2351236,
    2351677, 2351684, 2352774, 2352837, 2353021, 2353094, 2353157, 2353190, 2353221, 2353265,
    2353672, 2353989, 2354045, 2356740, 2356797, 2357258, 2357941, 2358195, 2358325, 2358877,
    2359281, 2359300, 2388829, 2392073, 2395645, 2395665, 2395837, 2396164, 2402461, 2486788,
    2489905, 2489981, 2490372, 2524698, 2525189, 2525220, 2525413, 2525917, 2526212, 2654077,
    2654212, 2672893, 2891780, 2892741, 2893126, 2893221, 2893320, 2893661, 2949124, 2967357,
    2967556, 2968573, 2968584, 2968925, 2969041, 2969092, 2971645, 2971656, 2971997, 2972164,
    2973149, 2973189, 2973361, 2973405, 2973700, 2975237, 2975473, 2975637, 2975747, 2975889,
    2975925, 2975965, 2976264, 2976605, 2976618, 2976861, 2976868, 2977565, 2977700, 2978333,
    2992131, 2992228, 2993507, 2993585, 2993672, 2994013, 3000320, 3001345, 3002378, 3003121,
    3003261, 3006468, 3008893, 3008997, 3009028, 3009062, 3010845, 3011045, 3011171, 3011613,
    3013635, 3013713, 3013731, 3013765, 3013821, 3014150, 3014237, 3014660, 3211037, 3211268,
    3250909, 3252196, 3252541, 3538435, 3538589, 3538595, 3538845, 3538851, 3538941, 3538948,
    3548285, 3548740, 3548797, 3549700, 3549821, 3549860, 3549917, 3550340, 3550493, 3550724,
    3563421, 3637252, 3640701, 3640836, 3641277, 3641348, 3641661, 3641860, 3642205, 3642261,
    3642277, 3642353, 3642394, 3642525, 3768341, 3776008, 3776349, 3776533, 3790493, 3792901,
    3794397, 3794437, 3795197, 3795477, 3799197, 3801109, 3808989, 3809301, 3810557, 3810613,
    3812518, 3812581, 3812693, 3812774, 3812986, 3813221, 3813493, 3813541, 3813781, 3814725,
    3814869, 3816829, 3817493, 3819589, 3819701, 3819741, 3823626, 3824285, 3824650, 3825309,
    3825685, 3828477, 3828746, 3829565, 3833856, 3834689, 3835520, 3836353, 3836605, 3836609,
    3837184, 3838017, 3838848, 3838909, 3838912, 3839005, 3839040, 3839101, 3839136, 3839229,
    3839264, 3839421, 3839424, 3839681, 3839837, 3839841, 3839901, 3839905, 3840157, 3840161,
    3840512, 3841345, 3842176, 3842269, 3842272, 3842429, 3842464, 3842749, 3842752, 3843005,
    3843009, 3843840, 3843933, 3843936, 3844093, 3844096, 3844285, 3844288, 3844349, 3844416,
    3844669, 3844673, 3845504, 3846337, 3847168, 3848001, 3848832, 3849665, 3850496, 3851329,
    3852160, 3852993, 3853824, 3854657, 3855581, 3855616, 3856434, 3856449, 3857266, 3857281,
    3857472, 3858290, 3858305, 3859122, 3859137, 3859328, 3860146, 3860161, 3860978, 3860993,
    3861184, 3862002, 3862017, 3862834, 3862849, 3863040, 3863858, 3863873, 3864690, 3864705,
    3864896, 3864929, 3864989, 3865032, 3866645, 3883013, 3884789, 3884901, 3886517, 3886757,
    3886805, 3887237, 3887285, 3887345, 3887517, 3887973, 3888157, 3888165, 3888669, 3923969,
    3924292, 3924321, 3924989, 3925153, 3925373, 3932165, 3932413, 3932421, 3932989, 3933029,
    3933277, 3933285, 3933373, 3933381, 3933565, 3933699, 3935709, 3936741, 3936797, 3940356,
    3941821, 3941893, 3942115, 3942365, 3942408, 3942749, 3942852, 3942901, 3942941, 3953156,
    3954117, 3954173, 3954692, 3956101, 3956232, 3956573, 3956723, 3956765, 3971588, 3972451,
    3972485, 3972616, 3972957, 3979780, 3980741, 3980804, 3980840, 3981181, 3981297, 3981341,
    3996676, 3996925, 3996932, 3997085, 3997092, 3997181, 3997188, 3997693, 3997700, 4004029,
    4004074, 4004357, 4004605, 4005888, 4006977, 4008069, 4008291, 4008349, 4008456, 4008797,
    4008913, 4008989, 4034090, 4035989, 4036010, 4036115, 4036138, 4036285, 4038698, 4040149,
    4040170, 4040669, 4046852, 4047005, 4047012, 4047901, 4047908, 4047997, 4048004, 4048061,
    4048100, 4048157, 4048164, 4048509, 4048516, 4048669, 4048676, 4048733, 4048740, 4048797,
    4048964, 4049021, 4049124, 4049181, 4049188, 4049245, 4049252, 4049309, 4049316, 4049437,
    4049444, 4049533, 4049540, 4049597, 4049636, 4049693, 4049700, 4049757, 4049764, 4049821,
    4049828, 4049885, 4049892, 4049949, 4049956, 4050045, 4050052, 4050109, 4050148, 4050301,
    4050308, 4050557, 4050564, 4050717, 4050724, 4050877, 4050884, 4050941, 4050948, 4051293,
    4051300, 4051869, 4052004, 4052125, 4052132, 4052317, 4052324, 4052893, 4054546, 4054621,
    4063253, 4064669, 4064789, 4067997, 4068373, 4068861, 4068917, 4069405, 4069429, 4069917,
    4069941, 4071133, 4071434, 4071861, 4077021, 4078805, 4079741, 4080149, 4081565, 4081685,
    4081981, 4082197, 4082269, 4082709, 4082909, 4087829, 4095860, 4096021, 4119325, 4119445,
    4119997, 4120085, 4120509, 4120597, 4124413, 4124533, 4127581, 4127765, 4128157, 4128277,
    4128317, 4128789, 4129181, 4129301, 4131101, 4131349, 4131677, 4131861, 4133149, 4133397,
    4134365, 4134421, 4134813, 4134933, 4135005, 4136981, 4147869, 4148245, 4148701, 4148757,
    4149181, 4149269, 4149597, 4149749, 4151549, 4151765, 4152253, 4152309, 4152669, 4152853,
    4153149, 4153365, 4158077, 4158101, 4161032, 4161373, 4194308, 5561373, 5562372, 5695325,
    5695492, 5702621, 5702660, 5887069, 5887492, 6126653, 6127108, 6147037, 6225924, 6243293,
    6291460, 6449533, 6449668, 6583837, 29360186, 29360221, 29361178, 29364253, 29368325, 29376029,
    31457308, 33554397, 33554460, 35651549,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The length of the match of `pattern` at the start of `input`.
    fn category_match(pattern: &str, input: &str) -> Option<usize> {
        let re = compile(pattern, true, false).ok()?;
        match_regex(&re, input).map(|m| m.len)
    }

    #[test]
    fn general_category_groups() {
        let cases: [(&str, &str, bool); 20] = [
            ("\\p{M}", "\u{301}", true),
            ("\\p{M}", "a", false),
            ("\\p{S}", "+", true),
            ("\\p{S}", "$", true),
            ("\\p{S}", "^", true),
            ("\\p{S}", "a", false),
            ("\\p{Z}", " ", true),
            ("\\p{Z}", "\u{2028}", true),
            ("\\p{Z}", "\u{2029}", true),
            ("\\p{Z}", "_", false),
            ("\\p{C}", "\u{1}", true),
            ("\\p{C}", "\u{ad}", true),
            ("\\p{C}", "a", false),
            ("[[:print:]]", "a", true),
            ("[[:print:]]", " ", true),
            ("[[:print:]]", "\u{2028}", false),
            ("[[:print:]]", "\u{2029}", false),
            ("[[:print:]]", "\u{1}", false),
            ("[[:print:]]", "\u{ad}", false),
            ("[[:print:]]", "\u{e000}", false),
        ];
        for (pattern, input, matches) in cases {
            assert_eq!(
                category_match(pattern, input).is_some(),
                matches,
                "{pattern} on {input:?}"
            );
        }
        assert_eq!(category_match("[[:print:]]", "\u{378}"), None);
        assert_eq!(category_match("[[:graph:]]", "\u{378}"), None);
        assert_eq!(category_match("[[:graph:]]", "x"), Some(1));
    }

    type Outcome = Result<Option<String>, String>;

    fn run(pat: &str, cs: bool, minimal: bool, input: &str) -> Outcome {
        let re = compile(pat, cs, minimal)?;
        Ok(match_regex(&re, input).and_then(|m| input.get(..m.len).map(str::to_string)))
    }

    fn m(pat: &str, input: &str) -> Outcome {
        run(pat, true, false, input)
    }

    fn mi(pat: &str, input: &str) -> Outcome {
        run(pat, false, false, input)
    }

    fn yes(s: &str) -> Outcome {
        Ok(Some(s.to_string()))
    }

    fn caps(pat: &str, input: &str) -> Result<Option<Vec<(usize, String)>>, String> {
        let re = compile(pat, true, false)?;
        Ok(match_regex(&re, input).map(|m| {
            m.captures
                .iter()
                .map(|(k, (off, len))| {
                    let text = input.get(*off..off + len).unwrap_or("?");
                    (*k, text.to_string())
                })
                .collect()
        }))
    }

    fn cap_list(items: &[(usize, &str)]) -> Result<Option<Vec<(usize, String)>>, String> {
        Ok(Some(
            items.iter().map(|(k, s)| (*k, s.to_string())).collect(),
        ))
    }

    #[test]
    fn literal_is_anchored_at_start() {
        assert_eq!(m("abc", "abcd"), yes("abc"));
        assert_eq!(m("bc", "abc"), Ok(None));
    }

    #[test]
    fn alternation_prefers_left() {
        assert_eq!(m("a|ab", "ab"), yes("a"));
        assert_eq!(m("ab|a", "ab"), yes("ab"));
        assert_eq!(m("(?:|a)b", "ab"), yes("ab"));
    }

    #[test]
    fn greedy_and_lazy_star() {
        assert_eq!(m("a*", "aaab"), yes("aaa"));
        assert_eq!(m("a*?", "aaa"), yes(""));
        assert_eq!(m("a*?b", "aaab"), yes("aaab"));
        assert_eq!(m("a+?", "aaa"), yes("a"));
    }

    #[test]
    fn optional_quantifier() {
        assert_eq!(m("ab?", "ab"), yes("ab"));
        assert_eq!(m("ab??", "ab"), yes("a"));
    }

    #[test]
    fn counted_repetition() {
        assert_eq!(m("a{2}", "aaa"), yes("aa"));
        assert_eq!(m("a{2,}", "aaaa"), yes("aaaa"));
        assert_eq!(m("a{,2}", "aaa"), yes("aa"));
        assert_eq!(m("a{1,2}?", "aaa"), yes("a"));
        assert_eq!(m("a{2,3}", "a"), Ok(None));
    }

    #[test]
    fn brace_literals_and_errors() {
        assert_eq!(m("a{}", "a{}"), yes("a{}"));
        assert_eq!(m("x{,}", "x{,}"), yes("x{,}"));
        assert_eq!(m("a{b}", "a{b}"), yes("a{b}"));
        assert!(compile("a{3,1}", true, false).is_err());
        assert!(compile("{2}", true, false).is_err());
        assert!(compile("a{70000}", true, false).is_err());
    }

    #[test]
    fn quantifier_after_anchor_is_error() {
        assert!(compile("^*", true, false).is_err());
        assert!(compile("\\b+", true, false).is_err());
        assert!(compile("a**", true, false).is_err());
        assert!(compile("\\B*", true, false).is_ok());
    }

    #[test]
    fn possessive_quantifiers() {
        assert_eq!(m("a*+a", "aaa"), Ok(None));
        assert_eq!(m("a++b", "aab"), yes("aab"));
        assert_eq!(m("a?+a", "a"), Ok(None));
    }

    #[test]
    fn atomic_group() {
        assert_eq!(m("(?>a|ab)c", "abc"), Ok(None));
        assert_eq!(m("(?:a|ab)c", "abc"), yes("abc"));
    }

    #[test]
    fn capture_offsets() {
        assert_eq!(caps("(a)(b)", "ab"), cap_list(&[(1, "a"), (2, "b")]));
        let re = compile("x(a+)", true, false);
        let got = re.ok().and_then(|re| match_regex(&re, "xaa!"));
        assert_eq!(
            got.map(|g| (g.len, g.captures.get(&1).copied())),
            Some((3, Some((1, 2))))
        );
    }

    #[test]
    fn preferred_path_decides_captures() {
        assert_eq!(
            caps("(a|ab)(c|bcd)(d*)", "abcd"),
            cap_list(&[(1, "a"), (2, "bcd"), (3, "")])
        );
    }

    #[test]
    fn back_references() {
        assert_eq!(m("(a+)b\\1", "aabaa"), yes("aabaa"));
        assert_eq!(m("(a+)b\\1", "aaba"), Ok(None));
        assert_eq!(m("(a)\\g1\\g{1}\\{1}", "aaaa"), yes("aaaa"));
        assert_eq!(mi("(a)\\1", "aA"), yes("aA"));
        assert_eq!(m("(a)\\1", "aA"), Ok(None));
        assert_eq!(m("\\1", "a"), Ok(None));
    }

    #[test]
    fn case_insensitive_mode() {
        assert_eq!(mi("abc", "ABC"), yes("ABC"));
        assert_eq!(mi("[a-c]+", "ABCd"), yes("ABC"));
        assert_eq!(mi("[^a-c]", "B"), Ok(None));
        assert_eq!(mi("\\x41", "a"), yes("a"));
    }

    #[test]
    fn simple_case_mapping() {
        assert_eq!(mi("i", "\u{130}"), yes("\u{130}"));
        assert_eq!(mi("\u{1c5}", "\u{1c4}"), yes("\u{1c4}"));
        assert_eq!(mi("ss", "\u{df}"), Ok(None));
    }

    #[test]
    fn inline_modifiers() {
        assert_eq!(m("(?i)abc", "ABC"), yes("ABC"));
        assert_eq!(m("a(?i)b", "aB"), yes("aB"));
        assert_eq!(m("a(?i)b", "AB"), Ok(None));
        assert_eq!(m("(?i:a)b", "Ab"), yes("Ab"));
        assert_eq!(m("(?i:a)b", "AB"), Ok(None));
        assert_eq!(m("(?i)a(?-i)b", "Ab"), yes("Ab"));
        assert_eq!(m("(?i)a(?-i)b", "AB"), Ok(None));
        assert_eq!(m("(?:(?i)a)b", "AB"), Ok(None));
        assert_eq!(m("(?ms)a", "a"), yes("a"));
        assert!(compile("(?x)a", true, false).is_err());
    }

    #[test]
    fn minimal_mode() {
        assert_eq!(run("a+", true, true, "aaa"), yes("a"));
        assert_eq!(run("a+?", true, true, "aaa"), yes("aaa"));
        assert_eq!(m("(?U)a+", "aaa"), yes("a"));
        assert_eq!(m("(?U:a+)", "aaa"), yes("a"));
        assert_eq!(m("(?U:x)a+", "xaaa"), yes("xaaa"));
        assert_eq!(run("a++", true, true, "aaa"), yes("aaa"));
    }

    #[test]
    fn look_ahead() {
        assert_eq!(m("a(?=b)", "ab"), yes("a"));
        assert_eq!(m("a(?=b)", "ac"), Ok(None));
        assert_eq!(m("a(?!b)", "ac"), yes("a"));
        assert_eq!(m("a(?!b)", "ab"), Ok(None));
    }

    #[test]
    fn look_behind() {
        assert_eq!(m("a(?<=a)b", "ab"), yes("ab"));
        assert_eq!(m("a(?<!a)b", "ab"), Ok(None));
        assert_eq!(m("(?<!a)b", "b"), yes("b"));
        assert_eq!(m("(?<=x)a", "a"), Ok(None));
        assert_eq!(m("é(?<=é)", "é"), yes("é"));
        assert_eq!(m("(a)b(?<=\\1b)", "ab"), yes("ab"));
    }

    #[test]
    fn capture_inside_look_behind_is_empty() {
        assert_eq!(caps("a(?<=(a))", "a"), cap_list(&[(1, "")]));
    }

    #[test]
    fn word_boundary() {
        assert_eq!(m("\\bfoo\\b", "foo bar"), yes("foo"));
        assert_eq!(m("foo\\b", "foobar"), Ok(None));
        assert_eq!(m("foo\\b", "foo"), yes("foo"));
        assert_eq!(m("a\\B", "ab"), yes("a"));
        // skylighting treats the end of the input as a boundary.
        assert_eq!(m("-\\b", "-"), yes("-"));
        assert_eq!(m("-\\b", "--"), Ok(None));
    }

    #[test]
    fn anchors() {
        assert_eq!(m("a$", "a"), yes("a"));
        assert_eq!(m("a$", "ab"), Ok(None));
        assert_eq!(m("^a", "a"), yes("a"));
        assert_eq!(m("\\Aa\\G", "a"), Ok(None));
        assert_eq!(m("\\Ga", "a"), yes("a"));
    }

    #[test]
    fn any_char_is_utf8_aware() {
        let re = compile("..", true, false);
        let got = re.ok().and_then(|re| match_regex(&re, "éa!"));
        assert_eq!(got.map(|g| g.len), Some(3));
    }

    #[test]
    fn unicode_classes() {
        assert_eq!(m("\\p{Lu}+", "ÄBc"), yes("ÄB"));
        assert_eq!(m("\\p{^Lu}", "a"), yes("a"));
        assert_eq!(m("\\P{Lu}", "A"), Ok(None));
        assert_eq!(m("\\pL", "ж"), yes("ж"));
        assert_eq!(m("\\p{N}+", "٣Ⅳ½x"), yes("٣Ⅳ½"));
        assert_eq!(m("\\p{So}", "\u{1f600}"), yes("\u{1f600}"));
        assert_eq!(m("\\p{Greek}", "α"), Ok(None));
        assert_eq!(m("\\P{Greek}", "α"), yes("α"));
        assert_eq!(m("[\\p{Nd}x]+", "1x2"), yes("1x2"));
    }

    #[test]
    fn posix_classes() {
        assert_eq!(m("[[:digit:]]+", "123a"), yes("123"));
        assert_eq!(m("[[:^alpha:]]", "1"), yes("1"));
        assert_eq!(m("[[:^alpha:]]", "a"), Ok(None));
        assert_eq!(m("[[:word:]]+", "a_1-"), yes("a_1"));
        assert_eq!(m("[[:upper:]]", "\u{1c5}"), yes("\u{1c5}"));
        assert_eq!(m("[[:blank:]]", "\n"), Ok(None));
        assert_eq!(m("[[:xdigit:]]+", "fF0g"), yes("fF0"));
        assert_eq!(m("[[:graph:]]", " "), Ok(None));
    }

    #[test]
    fn bracket_classes() {
        assert_eq!(m("[^a-c]+", "xyzab"), yes("xyz"));
        assert_eq!(m("[]a]+", "]a]b"), yes("]a]"));
        assert_eq!(m("[]-a]+", "^a"), yes("^a"));
        assert_eq!(m("[a-]+", "a-a"), yes("a-a"));
        assert_eq!(m("[\\d.]+", "1.5x"), yes("1.5"));
        assert_eq!(m("[\\Qa]\\E]+", "a]a"), yes("a]a"));
        assert_eq!(m("[\\101]", "A"), yes("A"));
        assert_eq!(m("[\\b]", "\u{8}"), yes("\u{8}"));
        assert!(compile("[abc", true, false).is_err());
    }

    #[test]
    fn escapes() {
        assert_eq!(m("\\x41", "A"), yes("A"));
        assert_eq!(m("\\x{263A}", "☺"), yes("☺"));
        assert_eq!(m("\\z0041", "A"), yes("A"));
        assert_eq!(m("\\t\\n", "\t\n"), yes("\t\n"));
        assert_eq!(m("\\0101", "\u{8}1"), yes("\u{8}1"));
        assert_eq!(m("\\.\\-\\ ", ".- "), yes(".- "));
        assert_eq!(m("\\{", "{"), yes("{"));
        assert!(compile("\\q", true, false).is_err());
        assert!(compile("\\x4g", true, false).is_err());
        assert!(compile("\\x{110000}", true, false).is_err());
    }

    #[test]
    fn white_space_classes() {
        assert_eq!(m("\\h+", " \t\u{3000}x"), yes(" \t\u{3000}"));
        assert_eq!(m("\\s", "\u{85}"), Ok(None));
        assert_eq!(m("\\s", "\u{2028}"), Ok(None));
        assert_eq!(m("\\s+", " \u{a0}\u{2003}"), yes(" \u{a0}\u{2003}"));
        assert_eq!(m("\\S", "x"), yes("x"));
    }

    #[test]
    fn word_class_and_is_word_char() {
        assert_eq!(m("\\w+", "é_1\u{203f}-"), yes("é_1\u{203f}"));
        assert_eq!(m("\\W", "-"), yes("-"));
        assert!(is_word_char('_'));
        assert!(is_word_char('\u{ff3f}'));
        assert!(is_word_char('Ⅳ'));
        assert!(is_word_char('٣'));
        assert!(!is_word_char('-'));
        assert!(!is_word_char('\u{301}'));
    }

    #[test]
    fn subroutines() {
        assert_eq!(m("(a|b(?1))", "bba"), yes("bba"));
        assert_eq!(m("a(?R)?b", "aabb"), yes("aabb"));
        assert_eq!(m("x|(?R)", "y"), Ok(None));
        assert_eq!(m("(?2)a", "a"), yes("a"));
    }

    #[test]
    fn branch_reset_groups() {
        assert_eq!(m("(?|(a)|(b))\\1", "bb"), yes("bb"));
        assert_eq!(
            caps("(?|(a)|(b)(c))(d)", "ad"),
            cap_list(&[(1, "a"), (3, "d")])
        );
    }

    #[test]
    fn dynamic_placeholders() {
        let re = compile("%1x", true, false);
        assert_eq!(
            re.as_ref()
                .ok()
                .and_then(|r| match_regex(r, "%1x"))
                .map(|g| g.len),
            Some(3)
        );
        let capture = |n: usize| (n == 1).then_some("a.b");
        let sub = re
            .ok()
            .and_then(|r| substitute_dynamic(&r, &capture).ok())
            .flatten();
        let lens = sub.map(|s| {
            (
                match_regex(&s, "a.bx").map(|g| g.len),
                match_regex(&s, "aXbx"),
            )
        });
        assert_eq!(lens, Some((Some(4), None)));
        let missing = compile("%2", true, false).and_then(|r| substitute_dynamic(&r, &capture));
        assert_eq!(missing, Ok(None));
        assert_eq!(m("%a", "%a"), yes("%a"));
    }

    #[test]
    fn dynamic_budget_counts_repeated_captures_and_group_copies() -> Result<(), String> {
        let re = compile("(%1%1)", true, false)?;
        let tag = "x".repeat(MAX_TREE_NODES / 6);
        assert!(substitute_dynamic(&re, &|_| Some(&tag)).is_err());

        // A multibyte capture can exceed the byte budget within the node budget.
        let re = compile("%1", true, false)?;
        let tag = "界".repeat(MAX_TREE_NODES / 5);
        assert!(substitute_dynamic(&re, &|_| Some(&tag)).is_err());
        Ok(())
    }

    #[test]
    fn dynamic_inside_lazy_is_not_substituted() {
        let capture = |_: usize| Some("z");
        let sub = compile("%1+?", true, false)
            .ok()
            .and_then(|r| substitute_dynamic(&r, &capture).ok())
            .flatten();
        let got = sub.and_then(|s| match_regex(&s, "%1%1"));
        assert_eq!(got.map(|g| g.len), Some(2));
    }

    #[test]
    fn nesting_limit() {
        let deep = format!("{}a{}", "(".repeat(65), ")".repeat(65));
        assert!(compile(&deep, true, false).is_err());
        let ok = format!("{}a{}", "(".repeat(64), ")".repeat(64));
        let got = compile(&ok, true, false)
            .ok()
            .and_then(|r| match_regex(&r, "a"));
        assert_eq!(got.map(|g| (g.len, g.captures.len())), Some((1, 64)));
    }

    #[test]
    fn tree_size_limit() {
        assert!(compile("a{1,60000}", true, false).is_err());
        assert!(compile("(?:a{100}){100}{100}", true, false).is_err());
        assert!(compile("a{0,30}", true, false).is_ok());
    }

    #[test]
    fn step_budget_stops_matching() {
        let re = compile("(a|b)*c", true, false);
        let got = re.ok().map(|r| {
            let groups = capturing_groups(&r);
            let input = "ab".repeat(50) + "c";
            (
                run_match(&groups, &r, &input, MAX_STEPS).map(|g| g.len),
                run_match(&groups, &r, &input, 50),
            )
        });
        assert_eq!(got, Some((Some(101), None)));
    }

    #[test]
    fn depth_bound_stops_recursion() {
        let re = compile("a(?R)?", true, false);
        let got = re.ok().map(|r| {
            (
                match_regex(&r, &"a".repeat(50)).map(|g| g.len),
                match_regex(&r, &"a".repeat(5000)),
            )
        });
        assert_eq!(got, Some((Some(50), None)));
    }

    /// Worst cases for the stack: the deepest group nesting, the deepest
    /// tree, the longest alternation and unbounded subroutine recursion.
    fn stack_cases() -> Vec<(String, String)> {
        let nested = format!("{}a{}", "(?:(".repeat(32), ")?)?".repeat(32));
        let alts: Vec<String> = (0..511).map(|i| format!("w{i}")).collect();
        let behind = format!("a{}", "(?<=a".repeat(63) + &")".repeat(63));
        vec![
            (nested, "a".to_string()),
            ("a{0,250}".to_string(), "a".repeat(250)),
            (alts.join("|"), "w510".to_string()),
            ("a(?R)?".to_string(), "a".repeat(5000)),
            ("(a|b(?1))".to_string(), "b".repeat(5000) + "a"),
            (behind, "a".to_string()),
        ]
    }

    #[test]
    fn worst_cases_fit_a_small_stack() {
        // 512 KiB is a quarter of the default 2 MiB thread stack.
        let worker = std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                let mut peaks = Vec::new();
                for (pat, input) in stack_cases() {
                    let Ok(re) = compile(&pat, true, false) else {
                        return Err(pat);
                    };
                    let groups = capturing_groups(&re);
                    let (_, stats) = run_plain(&groups, &re, &input, MAX_STEPS, false);
                    let copy = re.clone();
                    let text = format!("{copy:?}");
                    let capture = |_: usize| Some("");
                    let same = substitute_dynamic(&re, &capture) == Ok(Some(copy));
                    if !same || text.is_empty() {
                        return Err(pat);
                    }
                    peaks.push(stats.peak_stack);
                }
                Ok(peaks)
            });
        let peaks = worker.ok().and_then(|h| h.join().ok());
        let within = peaks
            .as_ref()
            .and_then(|p| p.as_ref().ok())
            .is_some_and(|p| p.iter().all(|&b| b <= MAX_EXEC_STACK + 4096));
        assert!(within, "{peaks:?}");
    }

    #[test]
    fn steps_grow_linearly() {
        let steps = |pat: &str, input: &str| {
            compile(pat, true, false).ok().map(|re| {
                let groups = capturing_groups(&re);
                run_plain(&groups, &re, input, MAX_STEPS, false).1.steps
            })
        };
        for n in [500, 1000, 2000] {
            let word = "a".repeat(n);
            let line = "x = ".repeat(n / 4);
            let cases = [
                ("[a-z]+", word.clone()),
                ("\\b\\w+\\b", word.clone()),
                ("(?:x|=| )*", line.clone()),
                (".*?;", line.clone()),
            ];
            for (pat, input) in cases {
                let got = steps(pat, &input);
                assert!(got.is_some_and(|s| s <= 20 * n + 64), "{pat} {n} {got:?}");
            }
        }
    }

    #[test]
    fn repetition_shares_seen_states() {
        // skylighting's `someLoop` shares one set of seen states for all
        // matches that enter the loop. Offset 3 is first reached from the
        // less preferred two-character prefix, so the preferred route to
        // offset 3 is dropped. PCRE would match "aaa".
        assert_eq!(m("a+?a*", "aaa"), yes("aa"));
    }

    #[test]
    fn many_alternatives() {
        let words: Vec<String> = (0..300).map(|i| format!("w{i}x")).collect();
        let pat = words.join("|");
        assert_eq!(m(&pat, "w299x"), yes("w299x"));
    }

    #[test]
    fn path_order_matches_haskell() {
        let root = Path::root();
        let path = |bits: &[bool]| {
            let mut p = root.clone();
            for b in bits {
                p.push(*b);
            }
            p
        };
        assert!(path(&[false]) < path(&[false, false]));
        assert!(path(&[false, true]) < path(&[true]));
        assert!(path(&[false, true]) > path(&[false, false, true]));
        assert!(path(&[true, false]) == path(&[true, false]));
        let long: Vec<bool> = (0..70).map(|i| i == 69).collect();
        let longer: Vec<bool> = (0..71).map(|i| i == 64).collect();
        assert!(path(&long) < path(&longer));
        // Compare with the old packed order on many pairs of bit strings.
        let bits = |n: u32, len: u32| -> Vec<bool> { (0..len).map(|i| n >> i & 1 == 1).collect() };
        let mut all = Vec::new();
        for len in 0..9 {
            for n in 0..(1u32 << len) {
                all.push(bits(n, len));
            }
        }
        let paths: Vec<Path> = all.iter().map(|b| path(b)).collect();
        for (a, pa) in all.iter().zip(&paths) {
            for (b, pb) in all.iter().zip(&paths) {
                assert_eq!(pa.cmp(pb), a.cmp(b), "{a:?} {b:?}");
            }
        }
        // A deep path: the order of two long paths that differ early.
        let deep_a: Vec<bool> = (0..5000).map(|i| i == 3).collect();
        let deep_b: Vec<bool> = (0..4000).map(|i| i == 2).collect();
        assert_eq!(path(&deep_a).cmp(&path(&deep_b)), deep_a.cmp(&deep_b));
    }

    #[test]
    fn groups_take_first_definition() {
        let groups = compile("(?|(a)|(b))", true, false).map(|r| capturing_groups(&r));
        let first = groups.ok().and_then(|g| g.get(&1).cloned());
        assert_eq!(
            first,
            Some(mconcat(vec![Regex::MatchChar(CharPred::is('a'))]))
        );
    }

    /// The speed of one class test and of one matcher step, for an ASCII
    /// text and for a text outside ASCII. "Search" is the class test before
    /// the ASCII table: a binary search for each character. Run it in a
    /// release build:
    /// `cargo test --release -p randoc-highlight --lib class_steps_per_second -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn class_steps_per_second() {
        let rate = |f: &dyn Fn(char) -> bool, c: char| {
            let tests = 20_000_000u32;
            let t = std::time::Instant::now();
            let hits = (0..tests).filter(|_| f(std::hint::black_box(c))).count();
            (f64::from(tests) / t.elapsed().as_secs_f64(), hits)
        };
        for (pattern, mask, unit) in [
            ("\\w+", WORD, "a"),
            ("\\w+", WORD, "é"),
            ("\\p{L}+", LETTER, "a"),
            ("\\p{L}+", LETTER, "é"),
        ] {
            let Ok(re) = compile(pattern, true, false) else {
                unreachable!("the pattern compiles");
            };
            let c = unit.chars().next().unwrap_or('a');
            let (search, search_hits) = rate(&|c| mask & bit(category_by_search(c)) != 0, c);
            let (table, table_hits) = rate(&|c| in_categories(c, mask), c);
            assert_eq!(search_hits, table_hits);
            let input = unit.repeat(20_000);
            let groups = capturing_groups(&re);
            let t = std::time::Instant::now();
            let mut steps = 0;
            for _ in 0..20 {
                steps += run_plain(&groups, &re, &input, usize::MAX, false).1.steps;
            }
            let step_rate = steps as f64 / t.elapsed().as_secs_f64();
            eprintln!(
                "{pattern} on {unit:?}: class tests per second {search:.0} (search) \
                 {table:.0} (table); matcher steps per second {step_rate:.0}"
            );
        }
    }

    /// The probe answers "no match" only where the set search gives the
    /// empty set (`run_match` checks each answer), and it answers for
    /// these assertions.
    #[test]
    fn probe_agrees_with_the_set_search() {
        let cases = [
            ("(?:(?!\\w[\\w.]*@x).)+", "ab.cd ef@x gh", "ab.cd "),
            ("(?:(?!(?:a?)+b).)+", "aaacab", "aaac"),
            ("(?:(?!(?:\\w+\\s)*x).)+", "ab cd x", ""),
            ("(?:(?!(?:\\w+\\s)*=).)+", "ab cd, =e", "ab cd, "),
            (
                "(?:(?!a*?b|(c)d|(?>e+)f|\\bg$).)+",
                "aacxeeh gaab",
                "aacxeeh g",
            ),
            ("(?=.*z)a+", "aaa", ""),
            ("(?=.*z)a+", "aaaz", "aaa"),
            ("(?!(?=a)b|(?<=a)c)..", "ab", "ab"),
            ("[^ ]*!?(?=\\.?\\()", "++++ f(", ""),
            ("[^ ]*!?(?=\\.?\\()", "f!.(x", "f!."),
            ("(?:(?=(?:a?)+b)a|c)+", "caabc", "caa"),
            ("(a)(?!\\1)..", "aab", ""),
            ("(a)(?!\\1)..", "abb", "abb"),
        ];
        let mut checked = 0;
        for (pattern, input, expected) in cases {
            let ((), n, wrong) = with_probe_check(|| {
                let found = m(pattern, input);
                let expected = (!expected.is_empty()).then(|| expected.to_string());
                assert_eq!(found, Ok(expected), "{pattern} on {input:?}");
            });
            assert_eq!(wrong, 0, "{pattern} on {input:?}");
            checked += n;
        }
        assert!(checked >= 20, "{checked}");
    }

    /// A look-ahead that scans a long word from each start position uses a
    /// number of steps that grows linearly with the word.
    #[test]
    fn repeated_look_ahead_scans_are_linear() {
        let steps = |n: usize| {
            let Ok(re) = compile(
                "(?:(?!\\w[\\w.%+-]*@[a-z]+\\.[a-z]{2,4}\\b|\\.\\.\\.).)++",
                true,
                false,
            ) else {
                unreachable!("the pattern compiles");
            };
            let input = "é".repeat(n);
            let (found, stats) = run_plain(&capturing_groups(&re), &re, &input, usize::MAX, false);
            assert_eq!(found.map(|m| m.len), Some(input.len()));
            stats.steps
        };
        let (small, large) = (steps(500), steps(2000));
        assert!(large <= small * 9 / 2, "{small} {large}");
        assert!(large <= 2000 * 100, "{large}");
    }

    /// The ordered search applies to a spine of 64 levels and not to a
    /// spine of 65 levels, for each node kind that makes a level. The
    /// deeper pattern goes to the set search, which gives the same match
    /// with few steps.
    #[test]
    fn spine_depth_guard_is_exact() {
        let leaf = || Regex::MatchChar(CharPred::is('a'));
        // Each alternative starts with its own character, so that an
        // alternation gives one state.
        let other = |level: usize| Regex::MatchChar(CharPred::Is(0x100 + level as u32));
        type Wrapper<'a> = (&'a str, &'a dyn Fn(Regex, usize) -> Regex);
        let wrappers: [Wrapper<'_>; 5] = [
            ("sequence", &|r, _| Regex::MatchConcat(vec![r])),
            ("left alternative", &|r, level| alt(r, other(level))),
            ("right alternative", &|r, level| alt(other(level), r)),
            ("lazy", &|r, _| Regex::Lazy(Box::new(r))),
            ("capture", &|r, _| Regex::MatchCapture(1, Box::new(r))),
        ];
        for (name, wrap) in wrappers {
            // The outer sequence is level 0, so the leaf is at level
            // `levels + 1`. The repetition after it needs one state.
            let pattern = |levels: usize| {
                let nested = (0..levels).fold(leaf(), wrap);
                Regex::MatchConcat(vec![nested, some(leaf())])
            };
            let (at_limit, above) = (pattern(MAX_FIRST_DEPTH - 1), pattern(MAX_FIRST_DEPTH));
            assert!(ordered_search_applies(&at_limit), "{name}");
            assert!(!ordered_search_applies(&above), "{name}");
            for re in [&at_limit, &above] {
                let groups = capturing_groups(re);
                let (set, set_stats) = run_plain(&groups, re, "aaab", MAX_STEPS, false);
                let (ordered, _) = run_plain(&groups, re, "aaab", MAX_STEPS, true);
                assert_eq!(set.as_ref().map(|m| m.len), Some(3), "{name}");
                assert_eq!(set, ordered, "{name}");
                assert!(set_stats.steps < 400, "{name}: {}", set_stats.steps);
            }
        }
    }

    /// The ASCII table holds the category that the range table gives.
    #[test]
    fn ascii_table_equals_the_range_table() {
        for c in '\0'..='\u{ff}' {
            assert_eq!(category(c), category_by_search(c), "{c:?}");
        }
        assert_eq!(category('a'), LL);
        assert_eq!(category('A'), LU);
        assert_eq!(category('_'), PC);
        assert_eq!(category(' '), ZS);
        assert_eq!(category('\u{7f}'), CC);
    }

    #[test]
    fn unmatched_parenthesis_is_error() {
        assert!(compile("(a", true, false).is_err());
        assert!(compile("a)", true, false).is_err());
        assert_eq!(m("a]", "a]"), yes("a]"));
    }
}
