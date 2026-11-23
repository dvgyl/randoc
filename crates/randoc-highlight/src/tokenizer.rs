//! A port of `Skylighting.Tokenizer` from skylighting-core 0.15: it splits
//! code into typed tokens with the rules of a KDE syntax definition.
//!
//! The tokenizer works line by line, with a stack of contexts. In each step
//! it tries the rules of the top context at the current position. A rule that
//! fails leaves no trace: the line state goes back to its value before the
//! attempt. The context stack changes only after a rule has matched, so the
//! state that a failed attempt can change is the scalar line state.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use crate::regex;
use crate::registry::Registry;
use crate::syntax::{Context, ContextSwitch, KeywordList, Matcher, Re, Rule, Syntax, fold_case};
use crate::token::{SourceLine, Token, TokenType};

/// The limit of steps without progress (skylighting `loopLimit`). After it,
/// the rest of the line gets the attribute of the current context.
const LOOP_LIMIT: usize = 1024;

/// The deepest nesting of rule attempts through child rules and
/// `IncludeRules`, if `max_depth` is not smaller. A deeper attempt is an
/// error.
pub(crate) const MAX_RULE_DEPTH: usize = 32;

/// The regular expression work budget of a line, per byte of the line, in
/// matcher steps. It keeps the regular expression work of a line linear in
/// the line length, also for a pattern that scans the rest of the line at
/// each position. When the budget of a line is used up, `RegExpr` rules do
/// not match for the rest of that line.
pub(crate) const REGEX_STEPS_PER_BYTE: usize = 1_500;

/// The largest number of rule attempts for one token. Includes that fan out
/// in a bad definition could otherwise take exponential time. Above the
/// limit, tokenization stops with an error.
pub(crate) const MAX_RULE_ATTEMPTS: usize = 100_000;

/// Highlights `text` with `syntax`. `registry` supplies the contexts of
/// other syntaxes for context switches and `IncludeRules`. `max_depth` is
/// the largest size of the context stack; a deeper push is an error.
///
/// ```
/// use randoc_highlight::{Registry, highlight};
/// let registry = Registry::embedded();
/// let Some(rust) = registry.lookup("rust") else { return };
/// let lines = highlight(registry, rust, "let x = 1; // note\nx\n", 64);
/// let texts: Option<Vec<String>> = lines
///     .ok()
///     .map(|ls| ls.iter().map(|l| l.iter().map(|t| t.1.as_str()).collect()).collect());
/// assert_eq!(texts, Some(vec!["let x = 1; // note".to_string(), "x".to_string()]));
/// ```
///
/// # Errors
///
/// Returns the error text of skylighting, for example for a context switch
/// to an unknown context, a regular expression that does not compile, or a
/// context stack deeper than `max_depth`. Pandoc then shows plain code.
pub fn highlight(
    registry: &Registry,
    syntax: &Syntax,
    text: &str,
    max_depth: usize,
) -> Result<Vec<SourceLine>, String> {
    tokenize(registry, syntax, text, max_depth)
}

/// The crate-internal entry point; see [`highlight`].
pub(crate) fn tokenize(
    registry: &Registry,
    syntax: &Syntax,
    text: &str,
    max_depth: usize,
) -> Result<Vec<SourceLine>, String> {
    Tokenizer::new(registry, syntax, text, max_depth)?.run(text)
}

/// The largest regular expression work of one line of `text`, in matcher
/// steps per byte of the line. The budget tests of the clean-room languages
/// use it, with the context stack limit of the writers (128).
#[cfg(test)]
pub(crate) fn worst_regex_steps_per_byte(
    registry: &Registry,
    syntax: &Syntax,
    text: &str,
) -> Result<usize, String> {
    let mut tk = Tokenizer::new(registry, syntax, text, 128)?;
    let mut worst = 0;
    for (i, l) in lines(text).enumerate() {
        let before = tk.regex_steps;
        tk.tokenize_line(l, i + 1)?;
        worst = worst.max((tk.regex_steps - before) / (l.len() + 1));
    }
    Ok(worst)
}

/// The lines of a text as `Data.ByteString.lines` gives them: a final
/// newline gives no extra empty line, and `\r` stays in the line.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    let body = text.strip_suffix('\n').unwrap_or(text);
    let none = text.is_empty();
    body.split('\n').filter(move |_| !none)
}

/// `Data.Char.isSpace` on one byte of `Data.ByteString.Char8`.
fn is_space_byte(b: u8) -> bool {
    matches!(b, 9..=13 | 32 | 0xa0)
}

/// Haskell `lookupContext`: an empty name means the starting context.
pub(crate) fn lookup_context<'s>(syntax: &'s Syntax, name: &str) -> Option<&'s Context> {
    if name.is_empty() {
        if syntax.starting_context.is_empty() {
            None
        } else {
            syntax.context(&syntax.starting_context)
        }
    } else {
        syntax.context(name)
    }
}

/// `normalizeHighlighting`: drops empty tokens and joins a token with the
/// directly following tokens of the same type.
fn normalize(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    // True when the last token of `out` can take more text: the tokens
    // between it and the current one were all of its type.
    let mut open = false;
    for (tt, text) in tokens {
        match out.last_mut() {
            Some(last) if open && last.0 == tt => last.1.push_str(&text),
            _ if text.is_empty() => open = false,
            _ => {
                out.push((tt, text));
                open = true;
            }
        }
    }
    out
}

/// A capture of a regular expression: a range of a line. The line is
/// shared, so a copy of the captures does not copy text.
#[derive(Clone)]
struct Capture {
    line: Rc<str>,
    range: Range<usize>,
}

impl Capture {
    fn text(&self) -> &str {
        self.line.get(self.range.clone()).unwrap_or("")
    }
}

/// The captures of a regular expression, by group number.
type Captures = BTreeMap<usize, Capture>;

/// `Ok(None)` is a failed attempt; `Err` stops tokenization.
type Attempt<T> = Result<Option<T>, String>;

struct Frame<'s> {
    context: &'s Context,
    captures: Captures,
}

/// The line state that a failed attempt must restore.
struct Snapshot {
    pos: usize,
    endline: bool,
    prev_char: char,
    column: usize,
    loop_counter: usize,
    line_continuation: bool,
    captures: Captures,
}

struct Tokenizer<'s, 'l> {
    registry: &'s Registry,
    stack: Vec<Frame<'s>>,
    max_depth: usize,
    line: &'l str,
    /// Byte position in `line`.
    pos: usize,
    endline: bool,
    prev_char: char,
    /// Column in characters.
    column: usize,
    line_continuation: bool,
    /// A byte index, as skylighting computes it, compared with the
    /// character column.
    first_nonspace: Option<usize>,
    loop_counter: usize,
    captures: Captures,
    rule_depth: usize,
    /// The contexts whose rules an `IncludeRules` is trying now, to stop a
    /// cycle of includes.
    including: HashSet<*const Context>,
    /// The regular expression steps that the current line can still use.
    regex_budget: usize,
    /// The budget per byte of a line ([`REGEX_STEPS_PER_BYTE`]).
    regex_steps_per_byte: usize,
    /// The memory of the regular expression probe for the current line.
    dead_ends: regex::DeadEnds,
    /// All regular expression steps of the run, for the measurements.
    #[cfg(test)]
    regex_steps: usize,
    #[cfg(test)]
    regex_profile: HashMap<&'s str, (usize, usize)>,
    /// Runs the set search alone too for each call of a static pattern, and
    /// counts the calls where the two results differ.
    #[cfg(test)]
    compare_engines: Option<(usize, usize)>,
    /// The current line, shared by its captures; made at the first capture.
    shared_line: Option<Rc<str>>,
    attempts: usize,
    /// For `RangeDetect`: for each end character, the position of a search
    /// and its result in the current line.
    range_memo: HashMap<char, (usize, Option<usize>)>,
    /// Work counter for the linear-time test.
    #[cfg(test)]
    steps: usize,
}

impl<'s, 'l> Tokenizer<'s, 'l> {
    fn new(
        registry: &'s Registry,
        syntax: &'s Syntax,
        text: &'l str,
        max_depth: usize,
    ) -> Result<Self, String> {
        let start = lookup_context(syntax, &syntax.starting_context)
            .ok_or_else(|| "No starting context specified".to_string())?;
        Ok(Tokenizer {
            registry,
            stack: vec![Frame {
                context: start,
                captures: Captures::new(),
            }],
            max_depth: max_depth.max(1),
            line: "",
            pos: 0,
            endline: text.is_empty(),
            prev_char: '\n',
            column: 0,
            line_continuation: false,
            first_nonspace: None,
            loop_counter: 0,
            captures: Captures::new(),
            rule_depth: 0,
            including: HashSet::new(),
            shared_line: None,
            regex_budget: 0,
            regex_steps_per_byte: REGEX_STEPS_PER_BYTE,
            dead_ends: regex::DeadEnds::default(),
            #[cfg(test)]
            regex_steps: 0,
            #[cfg(test)]
            regex_profile: HashMap::new(),
            #[cfg(test)]
            compare_engines: None,
            attempts: 0,
            range_memo: HashMap::new(),
            #[cfg(test)]
            steps: 0,
        })
    }

    fn charge(&mut self, n: usize) {
        #[cfg(test)]
        {
            self.steps += n;
        }
        let _ = n;
    }

    fn run(&mut self, text: &'l str) -> Result<Vec<SourceLine>, String> {
        lines(text)
            .enumerate()
            .map(|(i, line)| self.tokenize_line(line, i + 1))
            .collect()
    }

    fn top(&self) -> Option<&Frame<'s>> {
        self.stack.last()
    }

    fn top_context(&self) -> Result<&'s Context, String> {
        self.top()
            .map(|f| f.context)
            .ok_or_else(|| "Empty context stack".to_string())
    }

    fn rest(&self) -> &'l str {
        self.line.get(self.pos..).unwrap_or("")
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            pos: self.pos,
            endline: self.endline,
            prev_char: self.prev_char,
            column: self.column,
            loop_counter: self.loop_counter,
            line_continuation: self.line_continuation,
            captures: self.captures.clone(),
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.pos = s.pos;
        self.endline = s.endline;
        self.prev_char = s.prev_char;
        self.column = s.column;
        self.loop_counter = s.loop_counter;
        self.line_continuation = s.line_continuation;
        self.captures = s.captures;
    }

    fn tokenize_line(&mut self, line: &'l str, linenum: usize) -> Result<SourceLine, String> {
        self.line = line;
        self.pos = 0;
        self.endline = line.is_empty();
        self.prev_char = '\n';
        self.line_continuation = false;
        self.column = 0;
        self.first_nonspace = line.bytes().position(|b| !is_space_byte(b));
        self.loop_counter = 0;
        self.range_memo.clear();
        self.dead_ends.clear();
        self.shared_line = None;
        self.regex_budget = self.regex_steps_per_byte.saturating_mul(line.len() + 1);
        if line.is_empty() {
            self.switch_until_stable(|c| &c.line_empty_context)?;
            return Ok(Vec::new());
        }
        let mut tokens = Vec::new();
        loop {
            let before = self.snapshot();
            match self.get_token()? {
                Some(Some(t)) => tokens.push(t),
                Some(None) => {}
                None => {
                    self.restore(before);
                    break;
                }
            }
        }
        let tokens = normalize(tokens);
        if self.endline {
            if !self.line_continuation {
                self.switch_until_stable(|c| &c.line_end_context)?;
            }
            Ok(tokens)
        } else {
            Err(format!(
                "Could not match anything at line {linenum} column {}",
                self.column
            ))
        }
    }

    /// `handleEmptyLine` and `checkLineEnd`: applies the switches of each
    /// new top context until the stack stays the same or a context has no
    /// switches, at most `LOOP_LIMIT` times.
    fn switch_until_stable(
        &mut self,
        pick: fn(&Context) -> &Vec<ContextSwitch>,
    ) -> Result<(), String> {
        for _ in 0..LOOP_LIMIT {
            let switches = pick(self.top_context()?);
            if switches.is_empty() {
                return Ok(());
            }
            // Only the frames that the pops can reach can change.
            let len = self.stack.len();
            let pops = switches
                .iter()
                .filter(|s| matches!(s, ContextSwitch::Pop))
                .count()
                .min(len);
            let saved: Vec<&'s Context> = self
                .stack
                .get(len - pops..)
                .unwrap_or(&[])
                .iter()
                .map(|f| f.context)
                .collect();
            self.switch(switches)?;
            let same = self.stack.len() == len
                && self
                    .stack
                    .get(len - pops..)
                    .unwrap_or(&[])
                    .iter()
                    .zip(&saved)
                    .all(|(f, c)| std::ptr::eq(f.context, *c));
            if same {
                return Ok(());
            }
        }
        Ok(())
    }

    fn switch(&mut self, switches: &[ContextSwitch]) -> Result<(), String> {
        for s in switches {
            match s {
                ContextSwitch::Pop => {
                    if self.stack.len() > 1 {
                        self.stack.pop();
                    }
                }
                ContextSwitch::Push((syn, name)) => {
                    let context = self
                        .registry
                        .get(syn)
                        .and_then(|s| lookup_context(s, name))
                        .ok_or_else(|| format!("Unknown syntax or context: ({syn:?},{name:?})"))?;
                    if self.stack.len() >= self.max_depth {
                        return Err(format!(
                            "Context stack deeper than {} at context {name}##{syn}",
                            self.max_depth
                        ));
                    }
                    self.stack.push(Frame {
                        context,
                        captures: Captures::new(),
                    });
                }
            }
        }
        Ok(())
    }

    fn add_captures(&mut self) {
        if !self.captures.is_empty() {
            let captures = self.captures.clone();
            if let Some(top) = self.stack.last_mut() {
                top.captures = captures;
            }
        }
    }

    fn get_capture(&self, n: usize) -> Option<String> {
        self.top()
            .and_then(|f| f.captures.get(&n))
            .map(|c| c.text().to_string())
    }

    /// `getToken`. `Ok(None)` is a failure; `Ok(Some(None))` is a step
    /// without a token.
    fn get_token(&mut self) -> Attempt<Option<Token>> {
        if self.endline {
            return Ok(None);
        }
        let context = self.top_context()?;
        if self.loop_counter > LOOP_LIMIT {
            let rest = self.rest();
            if let Some(last) = rest.chars().next_back() {
                self.prev_char = last;
            }
            self.column += rest.chars().count();
            self.charge(rest.len());
            self.pos = self.line.len();
            self.endline = true;
            return Ok(Some(Some((context.attribute, rest.to_string()))));
        }
        self.loop_counter += 1;
        self.attempts = 0;
        for rule in &context.rules {
            if let Some(result) = self.try_rule(rule)? {
                return Ok(Some(result));
            }
        }
        if !context.fallthrough_context.is_empty() {
            self.switch(&context.fallthrough_context)?;
            Ok(Some(None))
        } else if context.fallthrough {
            self.switch(&[ContextSwitch::Pop])?;
            Ok(Some(None))
        } else {
            Ok(self.normal_chunk().map(|t| Some((context.attribute, t))))
        }
    }

    /// `takeChars`: consumes `n` characters.
    fn take_chars(&mut self, n: usize) -> Option<String> {
        if n == 0 {
            return None;
        }
        let rest = self.rest();
        let end = rest.char_indices().nth(n).map_or(rest.len(), |(i, _)| i);
        let taken = rest.get(..end)?;
        let last = taken.chars().next_back()?;
        self.charge(end);
        self.pos += end;
        self.endline = self.pos >= self.line.len();
        self.prev_char = last;
        self.column += n;
        self.loop_counter = 0;
        Some(taken.to_string())
    }

    /// `tryRule`. A failure restores the line state.
    fn try_rule(&mut self, rule: &'s Rule) -> Attempt<Option<Token>> {
        self.attempts += 1;
        self.charge(1);
        if self.attempts > MAX_RULE_ATTEMPTS {
            return Err(format!(
                "More than {MAX_RULE_ATTEMPTS} rule attempts at one position"
            ));
        }
        let limit = MAX_RULE_DEPTH.min(self.max_depth);
        if self.rule_depth >= limit {
            return Err(format!(
                "Child rules or IncludeRules nest deeper than {limit}"
            ));
        }
        self.rule_depth += 1;
        let before = self.snapshot();
        let result = self.try_rule_inner(rule);
        self.rule_depth -= 1;
        if matches!(result, Ok(None)) {
            self.restore(before);
        }
        result
    }

    fn try_rule_inner(&mut self, rule: &'s Rule) -> Attempt<Option<Token>> {
        if self.pos >= self.line.len() {
            return Ok(None);
        }
        if rule.column.is_some_and(|n| n != self.column) {
            return Ok(None);
        }
        if rule.first_non_space && self.first_nonspace != Some(self.column) {
            return Ok(None);
        }
        let old = rule.lookahead.then(|| self.snapshot());
        self.captures.clear();
        let attr = rule.attribute;
        let token = match &rule.matcher {
            Matcher::IncludeRules((syn, name)) => {
                let include_attr = rule.include_attribute.then_some(attr);
                match self.include_rules(include_attr, syn, name)? {
                    Some(t) => t,
                    None => return Ok(None),
                }
            }
            matcher => match self.match_text(rule, matcher)? {
                Some(text) => (!text.is_empty()).then_some((attr, text)),
                None => return Ok(None),
            },
        };
        let mut child_token = None;
        for child in &rule.children {
            if let Some(t) = self.try_rule(child)? {
                child_token = t;
                break;
            }
        }
        let token = match token {
            None => None,
            Some(_) if rule.lookahead => {
                if let Some(old) = old {
                    self.pos = old.pos;
                    self.endline = old.endline;
                    self.prev_char = old.prev_char;
                    self.column = old.column;
                    self.loop_counter = old.loop_counter;
                }
                None
            }
            Some((tt, mut text)) => {
                if let Some((_, child_text)) = child_token {
                    text.push_str(&child_text);
                }
                Some((tt, text))
            }
        };
        self.switch(&rule.context_switch)?;
        self.add_captures();
        Ok(Some(token))
    }

    fn include_rules(
        &mut self,
        include_attr: Option<TokenType>,
        syn: &str,
        name: &str,
    ) -> Attempt<Option<Token>> {
        let Some(context) = self.registry.get(syn).and_then(|s| lookup_context(s, name)) else {
            let current = self.top_context()?;
            return Err(format!(
                "IncludeRules in {} requires undefined context {name}##{syn}",
                current.syntax
            ));
        };
        // An include of a context whose rules are being tried at this
        // position can only repeat the same attempts.
        let key: *const Context = context;
        if !self.including.insert(key) {
            return Ok(None);
        }
        let mut found = Ok(None);
        for rule in &context.rules {
            match self.try_rule(rule) {
                Ok(None) => {}
                other => {
                    found = other;
                    break;
                }
            }
        }
        self.including.remove(&key);
        let Some(token) = found? else {
            return Ok(None);
        };
        self.captures.clear();
        Ok(Some(match (token, include_attr) {
            (Some((TokenType::Normal, text)), Some(a)) => Some((a, text)),
            (t, _) => t,
        }))
    }

    /// The matchers that consume text: the matched text, or a failure.
    fn match_text(&mut self, rule: &'s Rule, matcher: &'s Matcher) -> Attempt<String> {
        let delims = &rule.word_delimiters;
        Ok(match matcher {
            Matcher::DetectChar(c) => self.detect_chars(rule.dynamic, &[*c]),
            Matcher::Detect2Chars(c, d) => self.detect_chars(rule.dynamic, &[*c, *d]),
            Matcher::AnyChar(set) => self
                .rest()
                .chars()
                .next()
                .filter(|c| set.contains(c))
                .and_then(|_| self.take_chars(1)),
            Matcher::RangeDetect(c, d) => self.range_detect(*c, *d),
            Matcher::RegExpr(re) => return self.reg_expr(rule.dynamic, re),
            Matcher::Int => self.number(delims, scan_int),
            Matcher::HlCOct => self.number(delims, scan_oct),
            Matcher::HlCHex => self.number(delims, scan_hex),
            Matcher::Float => self.number(delims, scan_float),
            Matcher::HlCStringChar => {
                let n = scan_cstring_char(self.rest().as_bytes());
                n.and_then(|n| self.take_chars(n))
            }
            Matcher::HlCChar => {
                let n = scan_cchar(self.rest().as_bytes());
                n.and_then(|n| self.take_chars(n))
            }
            Matcher::Keyword(attr, list) => match list {
                KeywordList::Unresolved(name) => {
                    return Err(format!("Keyword with unresolved list {name:?}"));
                }
                KeywordList::Resolved(words) => self.keyword(&attr.delims, |w| words.contains(w)),
            },
            Matcher::StringDetect(s) => self.string_detect(rule.dynamic, rule.case_sensitive, s),
            Matcher::WordDetect(s) => self.word_detect(rule.case_sensitive, delims, s),
            Matcher::LineContinue(c) => {
                let mut buf = [0u8; 4];
                if self.rest() == c.encode_utf8(&mut buf) {
                    self.line_continuation = true;
                    self.take_chars(1)
                } else {
                    None
                }
            }
            Matcher::DetectSpaces => {
                let n = self
                    .rest()
                    .bytes()
                    .take_while(|b| is_space_byte(*b))
                    .count();
                self.charge(n);
                self.take_chars(n)
            }
            Matcher::DetectIdentifier => {
                let bytes = self.rest().as_bytes();
                match bytes.first() {
                    Some(b) if b.is_ascii_alphabetic() || *b == b'_' => {
                        let n = 1 + bytes
                            .iter()
                            .skip(1)
                            .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
                            .count();
                        self.take_chars(n)
                    }
                    _ => None,
                }
            }
            Matcher::IncludeRules(_) => None,
        })
    }

    /// `detectChar` and `detect2Chars`. In a dynamic rule, a digit `N`
    /// stands for the first character of capture N.
    fn detect_chars(&mut self, dynamic: bool, chars: &[char]) -> Option<String> {
        let mut rest = self.rest().chars();
        for &c in chars {
            let want = if dynamic && c.is_ascii_digit() {
                let n = c.to_digit(10)? as usize;
                self.get_capture(n)?.chars().next()?
            } else {
                c
            };
            if rest.next()? != want {
                return None;
            }
        }
        self.take_chars(chars.len())
    }

    fn range_detect(&mut self, c: char, d: char) -> Option<String> {
        if self.rest().chars().next()? != c {
            return None;
        }
        let from = self.pos + c.len_utf8();
        let end = self.find_from(d, from)?;
        let inner = self.line.get(from..end)?.chars().count();
        self.take_chars(inner + 2)
    }

    /// The first position of `d` at or after `from`. The memo makes the
    /// searches of one line linear in total.
    fn find_from(&mut self, d: char, from: usize) -> Option<usize> {
        if let Some(&(start, found)) = self.range_memo.get(&d)
            && start <= from
            && found.is_none_or(|f| f >= from)
        {
            return found;
        }
        let tail = self.line.get(from..)?;
        let found = tail.find(d).map(|i| from + i);
        self.charge(found.map_or(tail.len(), |f| f - from));
        self.range_memo.insert(d, (from, found));
        found
    }

    fn reg_expr(&mut self, dynamic: bool, re: &'s Re) -> Attempt<String> {
        let rest = self.rest();
        if re.source.starts_with("\\b")
            && let Some(d) = rest.chars().next()
            && regex::is_word_char(self.prev_char) == regex::is_word_char(d)
        {
            return Ok(None);
        }
        let (compiled, groups) = match &re.compiled {
            Ok(x) => x,
            Err(e) => return Err(format!("Error compiling regex {}: {e}", re.source)),
        };
        if self.regex_budget == 0 {
            return Ok(None);
        }
        if !dynamic
            && let Some(first) = &re.first
            && !rest.chars().next().is_some_and(|c| first.allows(c))
        {
            return Ok(None);
        }
        let budget = self.regex_budget;
        // A pattern that lives for this call only gets its own memory: the
        // memory of the line holds addresses of compiled patterns.
        let plain = |dead_ends| regex::Search {
            max_steps: budget,
            ordered: false,
            probe: false,
            dead_ends,
            base: 0,
        };
        let (found, steps) = if dynamic {
            match regex::substitute_dynamic(compiled, &|n| self.get_capture(n)) {
                Some(sub) => regex::match_budgeted(
                    &regex::capturing_groups(&sub),
                    &sub,
                    rest,
                    plain(&mut regex::DeadEnds::default()),
                ),
                None => return Ok(None),
            }
        } else {
            let search = regex::Search {
                max_steps: budget,
                ordered: re.ordered,
                probe: re.probe,
                dead_ends: &mut self.dead_ends,
                base: self.pos,
            };
            let result = regex::match_budgeted(groups, compiled, rest, search);
            #[cfg(test)]
            if let Some((calls, diffs)) = &mut self.compare_engines {
                let set = regex::match_budgeted(
                    groups,
                    compiled,
                    rest,
                    plain(&mut regex::DeadEnds::default()),
                );
                *calls += 1;
                // The set search can stop at the budget where the ordered
                // search ends below it.
                if set.0 != result.0 && set.1 < budget {
                    *diffs += 1;
                    eprintln!("engines differ: {} on {rest:?}", re.source);
                }
            }
            result
        };
        self.regex_budget = self.regex_budget.saturating_sub(steps);
        #[cfg(test)]
        {
            self.regex_steps += steps;
            let e = self
                .regex_profile
                .entry(re.source.as_str())
                .or_insert((0, 0));
            e.0 += steps;
            e.1 += 1;
        }
        self.charge(steps);
        let Some(found) = found else {
            return Ok(None);
        };
        if !found.captures.is_empty() {
            let line = self
                .shared_line
                .get_or_insert_with(|| Rc::from(self.line))
                .clone();
            let base = self.pos;
            self.captures = found
                .captures
                .iter()
                .map(|(k, (off, len))| {
                    let start = base + off;
                    let capture = Capture {
                        line: line.clone(),
                        range: start..start + len,
                    };
                    (*k, capture)
                })
                .collect();
        }
        let n = rest.get(..found.len).map_or(0, |m| m.chars().count());
        Ok(self.take_chars(n))
    }

    /// `parseInt`, `parseOct`, `parseHex` and `parseFloat`: the previous
    /// character must be a word delimiter.
    fn number(
        &mut self,
        delims: &BTreeSet<char>,
        scan: fn(&[u8]) -> Option<usize>,
    ) -> Option<String> {
        if !delims.contains(&self.prev_char) {
            return None;
        }
        let n = scan(self.rest().as_bytes())?;
        self.charge(n);
        self.take_chars(n)
    }

    fn keyword(
        &mut self,
        delims: &BTreeSet<char>,
        contains: impl Fn(&str) -> bool,
    ) -> Option<String> {
        if !delims.contains(&self.prev_char) {
            return None;
        }
        let rest = self.rest();
        let end = rest.find(|c| delims.contains(&c)).unwrap_or(rest.len());
        self.charge(end);
        let word = rest.get(..end)?;
        if word.is_empty() || !contains(word) {
            return None;
        }
        self.take_chars(word.chars().count())
    }

    /// `subDynamicText`: replaces `%N` with capture N.
    fn sub_dynamic_text(&self, s: &str) -> Option<String> {
        let mut parts = s.split('%');
        let mut out = parts.next().unwrap_or("").to_string();
        for part in parts {
            match part.chars().next() {
                Some(c) if c.is_ascii_digit() => {
                    out.push_str(&self.get_capture(c.to_digit(10)? as usize)?);
                    out.push_str(part.get(1..).unwrap_or(""));
                }
                _ => {
                    out.push('%');
                    out.push_str(part);
                }
            }
        }
        Some(out)
    }

    /// The first `n` characters of the rest of the line.
    fn prefix_chars(&self, n: usize) -> &'l str {
        let rest = self.rest();
        let end = rest.char_indices().nth(n).map_or(rest.len(), |(i, _)| i);
        rest.get(..end).unwrap_or("")
    }

    fn string_detect(&mut self, dynamic: bool, case_sensitive: bool, s: &str) -> Option<String> {
        let target = if dynamic {
            self.sub_dynamic_text(s)?
        } else {
            s.to_string()
        };
        let n = target.chars().count();
        let ok = if case_sensitive {
            self.rest().starts_with(&target)
        } else {
            fold_case(&target) == fold_case(self.prefix_chars(n))
        };
        if !ok {
            return None;
        }
        self.take_chars(n)
    }

    fn word_detect(
        &mut self,
        case_sensitive: bool,
        delims: &BTreeSet<char>,
        s: &str,
    ) -> Option<String> {
        let n = s.chars().count();
        let text = if case_sensitive {
            if !self.rest().starts_with(s) {
                return None;
            }
            s
        } else {
            let t = self.prefix_chars(n);
            if fold_case(s) != fold_case(t) {
                return None;
            }
            t
        };
        let first = text.chars().next()?;
        let last = text.chars().next_back()?;
        let is_delim = |c: char| delims.contains(&c);
        if !(is_delim(self.prev_char) || is_delim(first)) {
            return None;
        }
        let after = self.rest().chars().nth(n).unwrap_or('\n');
        if !(is_delim(after) || is_delim(last)) {
            return None;
        }
        self.take_chars(text.chars().count())
    }

    /// The fallthrough of a context without a matching rule: a run of
    /// spaces, a word that starts with an ASCII letter or digit, or one
    /// character.
    fn normal_chunk(&mut self) -> Option<String> {
        let rest = self.rest();
        let first = rest.chars().next()?;
        let n = if first == ' ' {
            rest.bytes().take_while(|b| *b == b' ').count()
        } else if first.is_ascii_alphanumeric() {
            rest.chars().take_while(|c| regex::is_alpha_num(*c)).count()
        } else {
            1
        };
        self.take_chars(n)
    }
}

fn digits(bytes: &[u8], f: impl Fn(u8) -> bool) -> usize {
    bytes.iter().take_while(|b| f(**b)).count()
}

/// `Int`: one or more decimal digits.
fn scan_int(bytes: &[u8]) -> Option<usize> {
    let n = digits(bytes, |b| b.is_ascii_digit());
    (n > 0).then_some(n)
}

/// `HlCOct`: `0` and one or more octal digits.
fn scan_oct(bytes: &[u8]) -> Option<usize> {
    if bytes.first() != Some(&b'0') {
        return None;
    }
    let n = digits(bytes.get(1..)?, |b| (b'0'..=b'7').contains(&b));
    (n > 0).then_some(1 + n)
}

/// `HlCHex`: `0x` or `0X` and one or more hexadecimal digits.
fn scan_hex(bytes: &[u8]) -> Option<usize> {
    if bytes.first() != Some(&b'0') || !matches!(bytes.get(1), Some(b'x' | b'X')) {
        return None;
    }
    let n = digits(bytes.get(2..)?, |b| b.is_ascii_hexdigit());
    (n > 0).then_some(2 + n)
}

/// `Float`: digits, a dot and digits (one side not empty), and an
/// optional complete exponent.
fn scan_float(bytes: &[u8]) -> Option<usize> {
    let before = digits(bytes, |b| b.is_ascii_digit());
    if bytes.get(before) != Some(&b'.') {
        return None;
    }
    let after = digits(bytes.get(before + 1..)?, |b| b.is_ascii_digit());
    if before == 0 && after == 0 {
        return None;
    }
    let mut n = before + 1 + after;
    if matches!(bytes.get(n), Some(b'e' | b'E')) {
        let mut m = n + 1;
        if matches!(bytes.get(m), Some(b'+' | b'-')) {
            m += 1;
        }
        let exp = digits(bytes.get(m..).unwrap_or(&[]), |b| b.is_ascii_digit());
        if exp > 0 {
            n = m + exp;
        }
    }
    Some(n)
}

/// `HlCStringChar`: a C escape sequence.
fn scan_cstring_char(bytes: &[u8]) -> Option<usize> {
    if bytes.first() != Some(&b'\\') {
        return None;
    }
    let next = *bytes.get(1)?;
    let tail = bytes.get(2..).unwrap_or(&[]);
    match next {
        b'x' | b'X' => {
            let n = digits(tail, |b| b.is_ascii_hexdigit());
            (n > 0).then_some(2 + n)
        }
        b'0' => Some(2 + digits(tail, |b| (b'0'..=b'7').contains(&b))),
        _ if b"abefnrtv\"'?\\".contains(&next) => Some(2),
        _ => None,
    }
}

/// `HlCChar`: a C character literal such as `'a'` or `'\n'`.
fn scan_cchar(bytes: &[u8]) -> Option<usize> {
    if bytes.first() != Some(&b'\'') {
        return None;
    }
    let body = bytes.get(1..)?;
    let n = match scan_cstring_char(body) {
        Some(n) => n,
        None => match body.first() {
            Some(b) if *b != b'\'' && *b != b'\\' => 1,
            _ => return None,
        },
    };
    (body.get(n) == Some(&b'\'')).then_some(n + 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"fn main() { let x = 0x1F + 1.5e3; // comment with words
    /* block */ println!("a {} b\n", x); if a == b && c != d { return; }
    <div class="x" id='y'>Text &amp; <b>bold</b> <!-- c --></div>
    # Heading *emph* **strong** `code` [link](http://x.y/z) - item
    SELECT a, b FROM t WHERE c = 'd' AND e > 10; -- sql
    def f(x): return x ** 2  # py; import os.path; @decorator
    key: value, list: [1, 2, 3], "json": {"a": null, "b": true}
    $var = "string $interp ${expr}"; echo $HOME | grep -E '^a.*b$' > /dev/null
"#;

    const ITEMS: &str = r#"<itemDatas>
      <itemData name="Normal Text" defStyleNum="dsNormal"/>
      <itemData name="Comment" defStyleNum="dsComment"/>
      <itemData name="Keyword" defStyleNum="dsKeyword"/>
      <itemData name="String" defStyleNum="dsString"/>
      <itemData name="Alert" defStyleNum="dsAlert"/>
      <itemData name="Dec" defStyleNum="dsDecVal"/>
      <itemData name="Float" defStyleNum="dsFloat"/>
      <itemData name="BaseN" defStyleNum="dsBaseN"/>
      <itemData name="Char" defStyleNum="dsChar"/>
    </itemDatas>"#;

    fn registry(lists: &str, contexts: &str) -> Result<Registry, String> {
        let xml = format!(
            r#"<language name="Tiny" extensions="*.tiny"><highlighting>{lists}
            <contexts>{contexts}</contexts>{ITEMS}</highlighting></language>"#
        );
        let mut reg = Registry::new();
        reg.add_definition("tiny.xml", &xml)?;
        Ok(reg)
    }

    type Lines = Vec<Vec<(TokenType, String)>>;

    fn run_depth(lists: &str, contexts: &str, text: &str, depth: usize) -> Result<Lines, String> {
        let reg = registry(lists, contexts)?;
        let syntax = reg.get("Tiny").ok_or("no syntax")?;
        tokenize(&reg, syntax, text, depth)
    }

    fn run(contexts: &str, text: &str) -> Result<Lines, String> {
        run_depth("", contexts, text, 64)
    }

    fn line(tokens: &[(TokenType, &str)]) -> Vec<(TokenType, String)> {
        tokens.iter().map(|(t, s)| (*t, s.to_string())).collect()
    }

    use TokenType::{
        Alert, BaseN, Char, Comment, DecVal, Float as FloatTok, Keyword as Kw, Normal,
        String as Str,
    };

    const COMMENT: &str = r##"
      <context name="Normal" attribute="Normal Text" lineEndContext="#stay">
        <DetectChar char="#" attribute="Comment" context="Comment"/>
      </context>
      <context name="Comment" attribute="Comment" lineEndContext="#pop">
        <LineContinue attribute="Comment"/>
      </context>"##;

    #[test]
    fn context_push_and_line_end_pop() {
        assert_eq!(
            run(COMMENT, "a #x\nb"),
            Ok(vec![
                line(&[(Normal, "a "), (Comment, "#x")]),
                line(&[(Normal, "b")])
            ])
        );
    }

    #[test]
    fn lines_like_bytestring_lines() {
        assert_eq!(run(COMMENT, ""), Ok(vec![]));
        assert_eq!(run(COMMENT, "a\n"), Ok(vec![line(&[(Normal, "a")])]));
        assert_eq!(
            run(COMMENT, "a\n\nb\r"),
            Ok(vec![
                line(&[(Normal, "a")]),
                vec![],
                line(&[(Normal, "b\r")])
            ])
        );
    }

    #[test]
    fn line_continuation_keeps_context() {
        assert_eq!(
            run(COMMENT, "#a\\\nb\nc"),
            Ok(vec![
                line(&[(Comment, "#a\\")]),
                line(&[(Comment, "b")]),
                line(&[(Normal, "c")])
            ])
        );
        // An empty line applies the line-empty switches (here the default:
        // the line-end switches).
        assert_eq!(
            run(COMMENT, "#a\\\n\nc"),
            Ok(vec![
                line(&[(Comment, "#a\\")]),
                vec![],
                line(&[(Normal, "c")])
            ])
        );
    }

    #[test]
    fn keywords_need_delimiters() {
        let lists = "<list name=\"kw\"><item>if</item><item>then</item></list>";
        let ctx = r#"<context name="Normal" attribute="Normal Text">
            <keyword String="kw" attribute="Keyword"/></context>"#;
        assert_eq!(
            run_depth(lists, ctx, "if x then", 64),
            Ok(vec![line(&[(Kw, "if"), (Normal, " x "), (Kw, "then")])])
        );
        assert_eq!(
            run_depth(lists, ctx, "xif", 64),
            Ok(vec![line(&[(Normal, "xif")])])
        );
    }

    #[test]
    fn range_detect() {
        let ctx = r#"<context name="Normal" attribute="Normal Text">
            <RangeDetect char="&quot;" char1="&quot;" attribute="String"/></context>"#;
        assert_eq!(
            run(ctx, "a \"b\" c \"d"),
            Ok(vec![line(&[
                (Normal, "a "),
                (Str, "\"b\""),
                (Normal, " c \"d")
            ])])
        );
    }

    #[test]
    fn lookahead_switches_without_consuming() {
        let ctx = r##"<context name="Normal" attribute="Normal Text">
            <RegExpr String="x" lookAhead="true" context="X"/></context>
            <context name="X" attribute="Keyword">
            <DetectChar char="x" attribute="Keyword" context="#pop"/></context>"##;
        assert_eq!(
            run(ctx, "-x-"),
            Ok(vec![line(&[(Normal, "-"), (Kw, "x"), (Normal, "-")])])
        );
    }

    #[test]
    fn dynamic_rules_use_captures() {
        let ctx = r##"<context name="Normal" attribute="Normal Text">
            <RegExpr String="&lt;&lt;(\w+)" attribute="Keyword" context="Here"/></context>
            <context name="Here" attribute="String" dynamic="true">
            <StringDetect String="%1" dynamic="true" attribute="Keyword" context="#pop"/>
            <RegExpr String="=(%1)" dynamic="true" attribute="Alert"/>
            <DetectChar char="1" dynamic="true" attribute="Char"/>
            </context>"##;
        assert_eq!(
            run(ctx, "<<END a=END b E END x"),
            Ok(vec![line(&[
                (Kw, "<<END"),
                (Str, " a"),
                (Alert, "=END"),
                (Str, " b "),
                (Char, "E"),
                (Str, " "),
                (Kw, "END"),
                (Normal, " x")
            ])])
        );
    }

    #[test]
    fn include_rules_with_include_attrib() {
        let ctx = r#"<context name="Normal" attribute="Normal Text">
            <IncludeRules context="Inc" includeAttrib="true" attribute="Alert"/></context>
            <context name="Inc" attribute="Normal Text">
            <DetectChar char="@" attribute="Normal Text"/>
            <DetectChar char="!" attribute="Keyword"/></context>"#;
        assert_eq!(
            run(ctx, "@!a"),
            Ok(vec![line(&[(Alert, "@"), (Kw, "!"), (Normal, "a")])])
        );
    }

    #[test]
    fn include_cycles_end() {
        let ctx = r#"<context name="Normal" attribute="Normal Text">
            <IncludeRules context="B"/><IncludeRules context="B"/></context>
            <context name="B" attribute="Normal Text">
            <IncludeRules context="Normal"/><IncludeRules context="B"/>
            <DetectChar char="!" attribute="Keyword"/></context>"#;
        assert_eq!(run(ctx, "x!"), Ok(vec![line(&[(Normal, "x"), (Kw, "!")])]));
    }

    #[test]
    fn errors_have_skylighting_text() {
        let unknown_include = r#"<context name="Normal" attribute="Normal Text">
            <IncludeRules context="Nope"/></context>"#;
        assert_eq!(
            run(unknown_include, "x"),
            Err("IncludeRules in Tiny requires undefined context Nope##Tiny".to_string())
        );
        let unknown_push = r#"<context name="Normal" attribute="Normal Text">
            <DetectChar char="x" context="Nope"/></context>"#;
        assert_eq!(
            run(unknown_push, "x"),
            Err("Unknown syntax or context: (\"Tiny\",\"Nope\")".to_string())
        );
        let bad_regex = r#"<context name="Normal" attribute="Normal Text">
            <RegExpr String="a(" attribute="Keyword"/></context>"#;
        assert!(run(bad_regex, "b").is_err_and(|e| e.starts_with("Error compiling regex a(:")));
        // The rule is tried only on a non-empty line.
        assert_eq!(run(bad_regex, "\n"), Ok(vec![vec![]]));
    }

    #[test]
    fn push_loop_hits_loop_limit_or_depth_limit() {
        let ctx = r#"<context name="Normal" attribute="Normal Text">
            <DetectChar char="x" lookAhead="true" context="Normal"/></context>"#;
        assert_eq!(
            run_depth("", ctx, "xy", 2000),
            Ok(vec![line(&[(Normal, "xy")])])
        );
        assert!(
            run_depth("", ctx, "xy", 100).is_err_and(|e| e.starts_with("Context stack deeper"))
        );
    }

    #[test]
    fn numbers() {
        let ctx = r#"<context name="Normal" attribute="Normal Text">
            <HlCHex attribute="BaseN"/><Float attribute="Float"/>
            <HlCOct attribute="BaseN"/><Int attribute="Dec"/></context>"#;
        assert_eq!(
            run(ctx, "0x1F 1.5e3 017 42 a1 1.5e .5"),
            Ok(vec![line(&[
                (BaseN, "0x1F"),
                (Normal, " "),
                (FloatTok, "1.5e3"),
                (Normal, " "),
                (BaseN, "017"),
                (Normal, " "),
                (DecVal, "42"),
                (Normal, " a1 "),
                (FloatTok, "1.5"),
                (Normal, "e "),
                (FloatTok, ".5")
            ])])
        );
    }

    #[test]
    fn c_chars() {
        let ctx = r#"<context name="Normal" attribute="Normal Text">
            <HlCChar attribute="Char"/><HlCStringChar attribute="String"/></context>"#;
        assert_eq!(
            run(ctx, "'a' '\\n' \\x41 'ab'"),
            Ok(vec![line(&[
                (Char, "'a'"),
                (Normal, " "),
                (Char, "'\\n'"),
                (Normal, " "),
                (Str, "\\x41"),
                (Normal, " 'ab'")
            ])])
        );
    }

    #[test]
    fn word_and_string_detect() {
        let ctx = r#"<context name="Normal" attribute="Normal Text">
            <WordDetect String="end" attribute="Keyword"/>
            <StringDetect String="BEGIN" insensitive="true" attribute="Alert"/></context>"#;
        assert_eq!(
            run(ctx, "end endx begin"),
            Ok(vec![line(&[
                (Kw, "end"),
                (Normal, " endx "),
                (Alert, "begin")
            ])])
        );
    }

    #[test]
    fn column_and_first_non_space() {
        let ctx = r##"<context name="Normal" attribute="Normal Text">
            <DetectChar char="#" firstNonSpace="true" attribute="Comment"/>
            <RegExpr String="^!" attribute="Alert"/></context>"##;
        assert_eq!(
            run(ctx, "  #x\na #\n!!"),
            Ok(vec![
                line(&[(Normal, "  "), (Comment, "#"), (Normal, "x")]),
                line(&[(Normal, "a #")]),
                line(&[(Alert, "!"), (Normal, "!")])
            ])
        );
    }

    #[test]
    fn word_boundary_prefix_check() {
        let ctx = r#"<context name="Normal" attribute="Normal Text">
            <RegExpr String="\bfoo" attribute="Keyword"/></context>"#;
        assert_eq!(run(ctx, "_foo"), Ok(vec![line(&[(Normal, "_foo")])]));
        assert_eq!(
            run(ctx, "-foo"),
            Ok(vec![line(&[(Normal, "-"), (Kw, "foo")])])
        );
    }

    #[test]
    fn child_rules_extend_the_token() {
        let ctx = r#"<context name="Normal" attribute="Normal Text">
            <DetectChar char="a" attribute="Keyword"><DetectChar char="b" attribute="Alert"/></DetectChar>
            </context>"#;
        assert_eq!(
            run(ctx, "ab ac"),
            Ok(vec![line(&[
                (Kw, "ab"),
                (Normal, " "),
                (Kw, "a"),
                (Normal, "c")
            ])])
        );
    }

    #[test]
    fn fallthrough_context() {
        let ctx = r##"<context name="Normal" attribute="Normal Text">
            <DetectChar char="(" attribute="Keyword" context="Inner"/></context>
            <context name="Inner" attribute="String" fallthrough="true" fallthroughContext="#pop">
            <DetectChar char="x" attribute="Alert"/></context>"##;
        assert_eq!(
            run(ctx, "(xxy"),
            Ok(vec![line(&[(Kw, "("), (Alert, "xx"), (Normal, "y")])])
        );
    }

    #[test]
    fn normalize_joins_only_direct_neighbours() {
        let tokens = vec![
            (Normal, "a".to_string()),
            (Normal, "b".to_string()),
            (Kw, String::new()),
            (Normal, "c".to_string()),
        ];
        assert_eq!(normalize(tokens), line(&[(Normal, "ab"), (Normal, "c")]));
    }

    fn steps_for(contexts: &str, lists: &str, text: &str) -> Option<usize> {
        let reg = registry(lists, contexts).ok()?;
        let syntax = reg.get("Tiny")?;
        let mut tk = Tokenizer::new(&reg, syntax, text, 64).ok()?;
        tk.run(text).ok()?;
        Some(tk.steps)
    }

    /// One input class: the rules of the context "Normal" (and more
    /// contexts), the keyword lists, and the unit that the input repeats.
    struct Construct {
        name: &'static str,
        lists: &'static str,
        contexts: &'static str,
        unit: &'static str,
    }

    const NORMAL: &str =
        r##"<context name="Normal" attribute="Normal Text" lineEndContext="#stay">"##;

    fn constructs() -> Vec<Construct> {
        let c = |name, contexts, unit| Construct {
            name,
            lists: "",
            contexts,
            unit,
        };
        vec![
            c("normal text", "</context>", "abc é "),
            c(
                "DetectChar push and line end",
                r##"<DetectChar char="#" attribute="Comment" context="Comment"/></context>
                <context name="Comment" attribute="Comment" lineEndContext="#pop"/>"##,
                "x #c\n",
            ),
            c(
                "Detect2Chars push and pop",
                r##"<Detect2Chars char="/" char1="*" attribute="Comment" context="Block"/></context>
                <context name="Block" attribute="Comment"><Detect2Chars char="*" char1="/" attribute="Comment" context="#pop"/></context>"##,
                "/*x*/",
            ),
            c(
                "AnyChar",
                r#"<AnyChar String="$@" attribute="Alert"/></context>"#,
                "$@ ",
            ),
            c(
                "RangeDetect closed",
                r#"<RangeDetect char="&quot;" char1="&quot;" attribute="String"/></context>"#,
                "\"s\" ",
            ),
            c(
                "RangeDetect never closed",
                r#"<RangeDetect char="{" char1="}" attribute="String"/></context>"#,
                "{ ",
            ),
            c(
                "StringDetect",
                r#"<StringDetect String="begin" attribute="Keyword"/>
                <StringDetect String="END" insensitive="true" attribute="Keyword"/></context>"#,
                "begin end ",
            ),
            c(
                "WordDetect",
                r#"<WordDetect String="word" attribute="Keyword"/></context>"#,
                "word words ",
            ),
            c(
                "RegExpr",
                r#"<RegExpr String="[A-Z]+[0-9]" attribute="BaseN"/></context>"#,
                "AB1 AB ",
            ),
            c(
                "RegExpr dynamic",
                r##"<RegExpr String="&lt;&lt;(\w+)" attribute="Keyword" context="Here"/></context>
                <context name="Here" attribute="String" dynamic="true">
                <StringDetect String="%1" dynamic="true" attribute="Keyword" context="#pop"/>
                <RegExpr String="x%1" dynamic="true" attribute="Alert"/></context>"##,
                "<<E xE E ",
            ),
            c(
                "numbers",
                r#"<HlCHex attribute="BaseN"/><Float attribute="Float"/>
                <HlCOct attribute="BaseN"/><Int attribute="Dec"/></context>"#,
                "0x1F 1.5 017 42 1.5e ",
            ),
            c(
                "C characters",
                r#"<HlCChar attribute="Char"/><HlCStringChar attribute="String"/></context>"#,
                "'a' \\n '\\t' ",
            ),
            c(
                "DetectSpaces and DetectIdentifier",
                r#"<DetectSpaces/><DetectIdentifier attribute="Keyword"/></context>"#,
                "  ab_1\t",
            ),
            c(
                "IncludeRules and lookahead",
                r##"<IncludeRules context="Inc" includeAttrib="true" attribute="Alert"/></context>
                <context name="Inc" attribute="Normal Text"><DetectChar char="!"/>
                <DetectChar char="?" lookAhead="true" context="Look"/></context>
                <context name="Look" attribute="Keyword"><DetectChar char="?" attribute="Keyword" context="#pop"/></context>"##,
                "! ? ",
            ),
            c(
                "fallthrough",
                r##"<DetectChar char="[" attribute="Keyword" context="Fall"/></context>
                <context name="Fall" attribute="String" fallthrough="true" fallthroughContext="#pop">
                <DetectChar char="]" attribute="Alert"/></context>"##,
                "[]]x ",
            ),
            c(
                "line continuation",
                r##"<DetectChar char="#" attribute="Comment" context="Comment"/></context>
                <context name="Comment" attribute="Comment" lineEndContext="#pop"><LineContinue attribute="Comment"/></context>"##,
                "#a\\\nb\n",
            ),
            c(
                "empty lines",
                r##"<DetectChar char="#" attribute="Comment" context="Comment"/></context>
                <context name="Comment" attribute="Comment" lineEndContext="#pop"/>"##,
                "#\n\n",
            ),
            c(
                "child rules",
                r#"<DetectChar char="a" attribute="Keyword"><DetectChar char="b" attribute="Alert"/></DetectChar></context>"#,
                "ab ac ",
            ),
            Construct {
                name: "keyword",
                lists: "<list name=\"kw\"><item>if</item><item>then</item></list>",
                contexts: r#"<keyword String="kw" attribute="Keyword"/>
                    <keyword String="kw" insensitive="true" attribute="Alert"/></context>"#,
                unit: "if THEN iff x@y ",
            },
        ]
    }

    fn construct_steps(k: &Construct, reps: usize) -> Option<(usize, usize)> {
        let text = k.unit.repeat(reps);
        let contexts = format!("{NORMAL}{}", k.contexts);
        let steps = steps_for(&contexts, k.lists, &text)?;
        Some((steps, text.len()))
    }

    /// Standing orders 21a and 21c: each construct repeated 5,000 and 10,000
    /// times (measured once at 25,000 and 50,000); the step count grows
    /// linearly.
    fn check_linear(range: std::ops::Range<usize>) {
        for k in constructs().get(range).unwrap_or(&[]) {
            let small = construct_steps(k, 5_000);
            let large = construct_steps(k, 10_000);
            eprintln!("STEPS {}: {small:?} {large:?}", k.name);
            let linear = match (small, large) {
                (Some((a, _)), Some((b, bytes))) => b <= a * 21 / 10 && b <= 40 * bytes,
                _ => false,
            };
            assert!(linear, "{}: {small:?} {large:?}", k.name);
        }
    }

    #[test]
    fn constructs_are_linear_part_1() {
        check_linear(0..7);
    }

    #[test]
    fn constructs_are_linear_part_2() {
        check_linear(7..14);
    }

    #[test]
    fn constructs_are_linear_part_3() {
        check_linear(14..usize::MAX);
    }

    /// Runs `f` on a thread with the default 2 MiB stack.
    fn on_default_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(f)
            .ok()?
            .join()
            .ok()
    }

    fn is_depth_error(r: &Result<Lines, String>) -> bool {
        r.as_ref()
            .is_err_and(|e| e.contains("deeper than") || e.contains("nested too deeply"))
    }

    const NESTING: &str = r##"<context name="Normal" attribute="Normal Text" lineEndContext="#stay">
        <DetectChar char="(" attribute="Keyword" context="Normal"/>
        <DetectChar char=")" attribute="Keyword" context="#pop"/></context>"##;

    /// Standing order 21b: deep input gives the depth error or a correct
    /// result on a 2 MiB stack.
    #[test]
    fn deep_context_stacks() {
        let got = on_default_stack(|| {
            let mut ok = true;
            for n in [10_000, 100_000] {
                ok &= is_depth_error(&run_depth("", NESTING, &"(".repeat(n), 128));
                ok &= is_depth_error(&run_depth("", NESTING, &"(\n".repeat(n), 128));
            }
            // The default limit itself: 127 pushes on the starting context.
            let text = format!("{}{}", "(".repeat(127), ")".repeat(127));
            ok &= run_depth("", NESTING, &text, 128) == Ok(vec![line(&[(Kw, &text)])]);
            ok
        });
        assert_eq!(got, Some(true));
    }

    fn include_chain(n: usize) -> String {
        let mut xml = String::new();
        for i in 0..n {
            let name = if i == 0 {
                "Normal".to_string()
            } else {
                format!("C{i}")
            };
            xml.push_str(&format!(
                "<context name=\"{name}\" attribute=\"Normal Text\"><IncludeRules context=\"C{}\"/></context>",
                i + 1
            ));
        }
        xml.push_str(&format!(
            "<context name=\"C{n}\" attribute=\"Normal Text\"><DetectChar char=\"x\" attribute=\"Keyword\"/>\
             <RegExpr String=\"a(?R)?\" attribute=\"Alert\"/></context>"
        ));
        xml
    }

    #[test]
    fn deep_include_chains() {
        let got = on_default_stack(|| {
            let mut ok = true;
            for n in [10_000, 40_000] {
                ok &= is_depth_error(&run_depth("", &include_chain(n), "x", 128));
            }
            // 31 includes and the rule of the last context: depth 32.
            let deep_ok = run_depth("", &include_chain(31), "x", 128);
            ok &= deep_ok == Ok(vec![line(&[(Kw, "x")])]);
            // The regular expression at the end of the chain recurses as
            // deep as its own bound allows.
            let long = "a".repeat(5000);
            ok &=
                run_depth("", &include_chain(31), &long, 128) == Ok(vec![line(&[(Normal, &long)])]);
            ok
        });
        assert_eq!(got, Some(true));
    }

    #[test]
    fn include_cycles_between_definitions() {
        let got = on_default_stack(|| {
            let def = |name: &str, other: &str, extra: &str| {
                format!(
                    r###"<language name="{name}"><highlighting><contexts>
                    <context name="Normal" attribute="Normal Text">
                    <IncludeRules context="##{other}"/>{extra}</context></contexts>{ITEMS}
                    </highlighting></language>"###
                )
            };
            let mut reg = Registry::new();
            let a = reg.add_definition("a.xml", &def("A", "B", ""));
            let b = reg.add_definition(
                "b.xml",
                &def("B", "A", r#"<DetectChar char="!" attribute="Keyword"/>"#),
            );
            let syntax = reg.get("A")?;
            let lines = tokenize(&reg, syntax, "x!", 128);
            Some((a, b, lines))
        });
        assert_eq!(
            got,
            Some(Some((
                Ok(()),
                Ok(()),
                Ok(vec![line(&[(Normal, "x"), (Kw, "!")])])
            )))
        );
    }

    #[test]
    fn deep_child_rules() {
        let got = on_default_stack(|| {
            let nested = |n: usize| {
                format!(
                    "{}{}</context>",
                    "<DetectChar char=\"a\" attribute=\"Keyword\">".repeat(n),
                    "</DetectChar>".repeat(n)
                )
            };
            let mut ok = true;
            for n in [200, 10_000, 100_000] {
                let r = run_depth("", &format!("{NORMAL}{}", nested(n)), &"a".repeat(n), 128);
                ok &= is_depth_error(&r);
            }
            let r = run_depth("", &format!("{NORMAL}{}", nested(32)), &"a".repeat(32), 128);
            ok &= r == Ok(vec![line(&[(Kw, &"a".repeat(32))])]);
            ok
        });
        assert_eq!(got, Some(true));
    }

    /// A pattern that scans the rest of the line at each position costs
    /// O(n²) steps without the line budget; with it, the work is linear.
    #[test]
    fn regex_budget_keeps_scans_linear() {
        let contexts = format!(
            "{NORMAL}{}",
            r#"<RegExpr String="[a-z ]*;" attribute="Alert"/></context>"#
        );
        let steps = |n: usize| {
            let reg = registry("", &contexts).ok()?;
            let syntax = reg.get("Tiny")?;
            let text = "ab ".repeat(n);
            let mut tk = Tokenizer::new(&reg, syntax, &text, 128).ok()?;
            tk.regex_steps_per_byte = 4;
            let lines = tk.run(&text).ok()?;
            let plain = lines == vec![line(&[(Normal, &text)])];
            Some((plain, tk.regex_steps, text.len()))
        };
        let (small, large) = (steps(5_000), steps(10_000));
        let linear = match (small, large) {
            (Some((true, a, _)), Some((true, b, bytes))) => b <= a * 21 / 10 && b <= 5 * bytes,
            _ => false,
        };
        assert!(linear, "{small:?} {large:?}");
    }

    #[test]
    fn regex_budget_has_a_margin() {
        let reg = Registry::embedded();
        let mut rows = Vec::new();
        for syntax in reg.syntaxes() {
            // A mix of code, comments, strings and markup.
            let text = SAMPLE;
            let Ok(mut tk) = Tokenizer::new(reg, syntax, text, 128) else {
                continue;
            };
            let mut worst = 0f64;
            let mut ok = true;
            for (i, l) in lines(text).enumerate() {
                let before = tk.regex_steps;
                if tk.tokenize_line(l, i + 1).is_err() {
                    ok = false;
                    break;
                }
                worst = worst.max((tk.regex_steps - before) as f64 / (l.len() + 1) as f64);
            }
            if ok {
                rows.push((worst, syntax.name().to_string()));
            }
        }
        // The most regular expression work of one line, per byte, is far
        // below the budget (measured: 328 for AsciiDoc).
        rows.sort_by(|a, b| b.0.total_cmp(&a.0));
        let limit = (REGEX_STEPS_PER_BYTE / 4) as f64;
        assert!(
            rows.len() > 50 && rows.iter().all(|r| r.0 <= limit),
            "{:?}",
            rows.get(..5)
        );
    }

    #[test]
    fn every_embedded_definition_runs() {
        let code = "x = 1 + 2.5 // c\n\t\"s\" 'c' /* d */ <a> # e\n\n";
        let mut failures = Vec::new();
        for syntax in Registry::embedded().syntaxes() {
            if let Err(e) = tokenize(Registry::embedded(), syntax, code, 64) {
                failures.push(format!("{}: {e}", syntax.name()));
            }
        }
        assert_eq!(failures, Vec::<String>::new());
    }

    #[test]
    fn embedded_definitions_tokenize() {
        let reg = Registry::embedded();
        let samples = [
            (
                "rust",
                "fn main() {\n    let x = 0x1F; // y\n    println!(\"{x} \\n\");\n}\n",
            ),
            ("swift", "let s: String = \"v\\(x)\" /* c */ + 1.5e3\n"),
            (
                "zig",
                "const std = @import(\"std\");\npub fn main() void {}\n",
            ),
            ("nix", "{ pkgs ? import <nixpkgs> {} }:\npkgs.hello # c\n"),
            ("dart", "var x = 'a$b'; // c\n"),
            ("zsh", "cat <<EOF\nhello $X\nEOF\necho done\n"),
        ];
        for (lang, code) in samples {
            let syntax = reg.lookup(lang);
            assert!(syntax.is_some(), "no syntax {lang}");
            let Some(syntax) = syntax else { continue };
            let got = tokenize(reg, syntax, code, 64);
            let joined: Option<Vec<String>> = got.as_ref().ok().map(|ls| {
                ls.iter()
                    .map(|l| l.iter().map(|t| t.1.as_str()).collect())
                    .collect()
            });
            let expected: Vec<String> = lines(code).map(str::to_string).collect();
            assert_eq!(joined, Some(expected), "{lang}: {got:?}");
        }
    }

    #[test]
    #[ignore]
    fn profile_rules() {
        let reg = Registry::embedded();
        let cases: Vec<(&str, String)> = vec![
            ("asciidoc", "é".repeat(2000)),
            ("asciidoc", "[".repeat(2000)),
            ("julia", "+".repeat(2000)),
            ("odin", "0".repeat(2000)),
            ("mustache", "%".repeat(2000)),
            (
                "asciidoc",
                "Lorem ipsum dolor sit amet, see `code` and *strong* text. ".repeat(40),
            ),
            (
                "asciidoc",
                "Lorem ipsum dolor sit amet, see `code` and *strong* text. ".repeat(80),
            ),
            (
                "julia",
                format!("values = [{}0]", "123,456,789,".repeat(300)),
            ),
            (
                "julia",
                format!("values = [{}0]", "123,456,789,".repeat(600)),
            ),
            (
                "rust",
                format!("let data = [{}0];", "123,456,789,".repeat(300)),
            ),
            (
                "rust",
                format!("let data = [{}0];", "123,456,789,".repeat(600)),
            ),
            (
                "julia",
                format!(
                    "function f();x=0;y=0;{}end",
                    "x=foo(12,34);y=x+1;".repeat(300)
                ),
            ),
            (
                "logfile",
                "2024-01-01 12:00:00 INFO the quick brown fox jumps over the lazy dog ".repeat(100),
            ),
            (
                "orgmode",
                "Lorem ipsum dolor sit amet, see =code= and *strong* text. ".repeat(100),
            ),
            (
                "nix",
                "Lorem ipsum dolor sit amet, see `code` and *strong* text. ".repeat(100),
            ),
        ];
        for (lang, text) in cases {
            let Some(syntax) = reg.lookup(lang) else {
                continue;
            };
            let Ok(mut tk) = Tokenizer::new(reg, syntax, &text, 128) else {
                continue;
            };
            tk.regex_steps_per_byte = usize::MAX / 4;
            let _ = tk.run(&text);
            let mut rows: Vec<_> = tk
                .regex_profile
                .iter()
                .map(|(k, v)| (v.0, v.1, *k))
                .collect();
            rows.sort_by(|a, b| b.cmp(a));
            eprintln!(
                "== {lang} {} bytes, {} steps/byte",
                text.len(),
                tk.regex_steps / text.len().max(1)
            );
            for r in rows.iter().take(4) {
                eprintln!(
                    "  {:>10} steps {:>6} calls  {}",
                    r.0,
                    r.1,
                    r.2.chars().take(150).collect::<String>()
                );
            }
        }
    }

    fn compare_engines_on(reg: &Registry, lang: &str, text: &str) -> Option<(usize, usize)> {
        let syntax = reg.lookup(lang)?;
        let mut tk = Tokenizer::new(reg, syntax, text, 128).ok()?;
        tk.compare_engines = Some((0, 0));
        let _ = tk.run(text);
        tk.compare_engines
    }

    /// The ordered search gives the match of the set search for each
    /// regular expression call of the replay samples and of long lines.
    /// In the same calls, each "no match" of the probe for an assertion is
    /// an empty set of the set search.
    #[test]
    fn ordered_search_equals_set_search() {
        let reg = Registry::embedded();
        let samples = include_str!("../data/samples.txt");
        let mut long = Vec::new();
        for unit in [
            "Lorem ipsum dolor sit amet, see `code` and *strong* text. ",
            "Lorem ipsum, see =code= and *strong* and ~x~ and /it/ text. ",
            "x=foo(12,34);y=x+1;",
            "123,456,789,",
            "\"a\\\"b\" 'c' `d` <e> [f] {g} (h) /*i*/ #j ",
        ] {
            for lang in [
                "asciidoc", "orgmode", "julia", "rust", "nix", "logfile", "typst", "zig",
            ] {
                long.push((lang.to_string(), unit.repeat(40)));
            }
        }
        let parts = samples.split('\u{c}').skip(1).filter_map(|part| {
            let (lang, code) = part.split_once('\n')?;
            Some((lang.to_string(), code.to_string()))
        });
        let (total, probes, wrong) = regex::with_probe_check(|| {
            let mut total = (0, 0);
            for (lang, code) in parts.chain(long) {
                if let Some((calls, diffs)) = compare_engines_on(reg, &lang, &code) {
                    total.0 += calls;
                    total.1 += diffs;
                }
            }
            total
        });
        assert!(total.0 > 10_000 && total.1 == 0, "{total:?}");
        assert!(
            probes > 10_000 && wrong == 0,
            "{probes} probe answers, {wrong} wrong"
        );
    }

    fn regex_steps_of(lang: &str, text: &str) -> Option<(usize, Lines)> {
        let reg = Registry::embedded();
        let syntax = reg.lookup(lang)?;
        let mut tk = Tokenizer::new(reg, syntax, text, 128).ok()?;
        let lines = tk.run(text).ok()?;
        Some((tk.regex_steps, lines))
    }

    const PROSE: &str = "Lorem ipsum dolor sit amet, see `code` and *strong* text. ";

    /// The regular expression work of an AsciiDoc paragraph line grows
    /// linearly with the line, and stays far below the line budget.
    #[test]
    fn asciidoc_paragraph_lines_are_linear() {
        let steps = |reps: usize| regex_steps_of("asciidoc", &PROSE.repeat(reps));
        let (Some((small, _)), Some((large, lines))) = (steps(25), steps(100)) else {
            unreachable!("asciidoc tokenizes");
        };
        let bytes = PROSE.len() * 100;
        assert!(large <= small * 9 / 2, "{small} {large}");
        assert!(
            large * 4 <= REGEX_STEPS_PER_BYTE * bytes,
            "{large} steps for {bytes} bytes"
        );
        let code = lines.first().map_or(0, |l| {
            l.iter()
                .filter(|t| t.1 == "`code`" && t.0 != TokenType::Normal)
                .count()
        });
        assert_eq!(code, 100);
    }

    /// The paragraphs of a long AsciiDoc text all get the tokens of the
    /// first one, so the text has the oracle tokens of one paragraph.
    #[test]
    fn asciidoc_paragraphs_repeat_their_tokens() {
        let text = vec![PROSE.repeat(100); 20].join("\n\n");
        let Some((steps, lines)) = regex_steps_of("asciidoc", &text) else {
            unreachable!("asciidoc tokenizes");
        };
        assert!(steps * 4 <= REGEX_STEPS_PER_BYTE * text.len(), "{steps}");
        let first = lines.first().cloned();
        let same = lines.iter().step_by(2).all(|l| Some(l) == first.as_ref());
        assert!(same && lines.len() == 39);
    }

    fn start_strings(rules: &[Rule], out: &mut BTreeSet<String>) {
        for rule in rules {
            let s = match &rule.matcher {
                Matcher::DetectChar(c) | Matcher::RangeDetect(c, _) | Matcher::LineContinue(c) => {
                    c.to_string()
                }
                Matcher::Detect2Chars(c, d) => format!("{c}{d}"),
                Matcher::StringDetect(s) | Matcher::WordDetect(s) => s.clone(),
                Matcher::AnyChar(set) => set.iter().next().map(char::to_string).unwrap_or_default(),
                Matcher::RegExpr(re) => {
                    for c in re.source.chars().filter(|c| !c.is_alphanumeric()).take(3) {
                        out.insert(c.to_string());
                    }
                    String::new()
                }
                _ => String::new(),
            };
            if !s.is_empty() && s.chars().count() <= 8 {
                out.insert(s);
            }
            start_strings(&rule.children, out);
        }
    }

    /// The table of HL-R1-06: for each embedded language, the slowest line
    /// of 50,000 characters made of one start string of the definition.
    /// Run it in a release build:
    /// `cargo test --release -p randoc-highlight --lib worst_lines -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn worst_lines() {
        let reg = Registry::embedded();
        let mut rows = Vec::new();
        for syntax in reg.syntaxes() {
            let mut starts = BTreeSet::new();
            for context in syntax.contexts.values() {
                start_strings(&context.rules, &mut starts);
            }
            for s in [
                "a", " ", "0", "\"", "'", "(", "[", "{", "<", "\\", "`", "#", "é",
            ] {
                starts.insert(s.to_string());
            }
            let mut worst = (0f64, String::new(), 0usize);
            let language_start = std::time::Instant::now();
            let mut skipped = 0;
            for start in &starts {
                // A language that needs more than 60 seconds is a finding;
                // the other strings would only make the run longer.
                if language_start.elapsed().as_secs() > 60 {
                    skipped += 1;
                    continue;
                }
                let reps = 50_000 / start.chars().count().max(1);
                let line = start.repeat(reps);
                let t = std::time::Instant::now();
                let Ok(mut tk) = Tokenizer::new(reg, syntax, &line, 1000) else {
                    continue;
                };
                let _ = tk.run(&line);
                let secs = t.elapsed().as_secs_f64();
                if secs > worst.0 {
                    worst = (secs, start.clone(), tk.regex_steps / line.len().max(1));
                }
            }
            eprintln!(
                "{} {:.3} {:?} {} {} {skipped}",
                syntax.short_name(),
                worst.0,
                worst.1,
                worst.2,
                starts.len()
            );
            rows.push((
                worst.0,
                syntax.short_name().to_string(),
                worst.1,
                worst.2,
                starts.len(),
                skipped,
            ));
        }
        rows.sort_by(|a, b| b.0.total_cmp(&a.0));
        eprintln!(
            "| Language | Worst start string | Seconds | Steps per byte | Strings | Skipped |"
        );
        for (secs, name, start, spb, n, skipped) in &rows {
            eprintln!("| {name} | `{start:?}` | {secs:.3} | {spb} | {n} | {skipped} |");
        }
    }
}
