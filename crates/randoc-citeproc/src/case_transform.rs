//! Text case changes of CSL output: a port of `Citeproc.CaseTransform` and of
//! `caseTransform` in `Citeproc.Pandoc` (`citeproc` 0.14.1.1).
//!
//! The walk splits each `Str` into runs of letters and digits, of spaces and
//! of punctuation. A state machine tracks the place in the sentence, and
//! the transformer changes each run of letters and digits.

use randoc_ast::{Attr, Inline};

use crate::haskell;
use crate::lang::Lang;
use crate::markup::TooDeep;
use crate::types::TextCase;
use crate::unicode_class::{is_alpha_num, is_letter, is_lower, is_punctuation, is_upper};

/// The place of a run in the text (`CaseTransformState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Start,
    StartSentence,
    AfterWordEnd,
    AfterWordChar,
    AfterSentenceEndingPunctuation,
    BeforeLastWord,
}

/// Applies a text case to inlines (`addTextCase` of the `Inlines` output).
///
/// `lang` is the language of the formatting; Turkish changes the case of
/// the dotted and dotless i. Inlines nested deeper than `max_depth` give
/// [`TooDeep`].
pub fn add_text_case(
    lang: Option<&Lang>,
    case: TextCase,
    inlines: Vec<Inline>,
    max_depth: usize,
) -> Result<Vec<Inline>, TooDeep> {
    let turkish = lang.is_some_and(|lang| lang.language == "tr");
    let transform = match case {
        TextCase::PreserveCase => {
            return Ok(vec![Inline::Span(
                Attr::new("", vec!["nocase".into()], Vec::new()),
                inlines,
            )]);
        }
        TextCase::Lowercase => lowercase_all,
        TextCase::Uppercase => uppercase_all,
        TextCase::CapitalizeFirst => capitalize_first,
        TextCase::CapitalizeAll => capitalize_words,
        TextCase::SentenceCase => sentence_case,
        TextCase::TitleCase => title_case,
    };
    let mut walk = Walk {
        state: State::Start,
        turkish,
        transform,
    };
    walk.top(inlines, max_depth)
}

/// A transformer: the language is Turkish, the state, the run.
type Transform = fn(bool, State, &str) -> String;

struct Walk {
    state: State,
    turkish: bool,
    transform: Transform,
}

impl Walk {
    /// `caseTransform'`: a last `Str` with no word break is the last word.
    fn top(&mut self, mut inlines: Vec<Inline>, depth: usize) -> Result<Vec<Inline>, TooDeep> {
        let last_word = inlines.len() > 1
            && matches!(inlines.last(), Some(Inline::Str(t)) if !t.chars().any(is_word_break));
        let last = if last_word { inlines.pop() } else { None };
        let mut out = self.list(inlines, depth)?;
        if let Some(last) = last {
            if matches!(
                self.state,
                State::AfterWordEnd | State::StartSentence | State::Start
            ) {
                self.state = State::BeforeLastWord;
            }
            out.push(self.inline(last, depth)?);
        }
        Ok(out)
    }

    fn list(&mut self, inlines: Vec<Inline>, depth: usize) -> Result<Vec<Inline>, TooDeep> {
        inlines
            .into_iter()
            .map(|inline| self.inline(inline, depth))
            .collect()
    }

    fn inline(&mut self, inline: Inline, depth: usize) -> Result<Inline, TooDeep> {
        let inner = depth.checked_sub(1).ok_or(TooDeep)?;
        Ok(match inline {
            Inline::Str(text) => Inline::Str(
                split_up(&text)
                    .into_iter()
                    .map(|run| self.run(run))
                    .collect(),
            ),
            Inline::Space => {
                self.run(" ");
                Inline::Space
            }
            Inline::SmallCaps(_) | Inline::Superscript(_) | Inline::Subscript(_) => {
                self.skip(&inline);
                inline
            }
            Inline::Span(attr, parts) if attr.classes.iter().any(|c| c == "nocase") => {
                let plain = self.state == State::AfterWordChar && attr.classes == ["nocase"];
                let span = if plain {
                    Inline::Span(Attr::default(), parts)
                } else {
                    Inline::Span(attr, parts)
                };
                self.skip(&span);
                span
            }
            Inline::Span(attr, parts) => Inline::Span(attr, self.list(parts, inner)?),
            Inline::Emph(parts) => Inline::Emph(self.list(parts, inner)?),
            Inline::Underline(parts) => Inline::Underline(self.list(parts, inner)?),
            Inline::Strong(parts) => Inline::Strong(self.list(parts, inner)?),
            Inline::Strikeout(parts) => Inline::Strikeout(self.list(parts, inner)?),
            Inline::Quoted(kind, parts) => Inline::Quoted(kind, self.list(parts, inner)?),
            Inline::Cite(citations, parts) => Inline::Cite(citations, self.list(parts, inner)?),
            Inline::Link(attr, parts, target) => {
                Inline::Link(attr, self.list(parts, inner)?, target)
            }
            Inline::Image(attr, parts, target) => {
                Inline::Image(attr, self.list(parts, inner)?, target)
            }
            other => other,
        })
    }

    /// `return'`: the state moves over the text of an inline that keeps its case.
    fn skip(&mut self, inline: &Inline) {
        self.run(&str_text(inline));
    }

    /// `g`: moves the state over a run and changes the run when it holds
    /// only letters and digits.
    fn run(&mut self, run: &str) -> String {
        let before = self.state;
        if let Some(c) = run.chars().next_back() {
            self.state = if matches!(c, '.' | '?' | '!' | ':') {
                State::AfterSentenceEndingPunctuation
            } else if is_alpha_num(c) {
                State::AfterWordChar
            } else if haskell::is_space(c) && before == State::AfterSentenceEndingPunctuation {
                State::StartSentence
            } else if is_word_break(c) {
                State::AfterWordEnd
            } else {
                before
            };
        }
        if run.chars().all(is_alpha_num) {
            (self.transform)(self.turkish, before, run)
        } else {
            run.to_owned()
        }
    }
}

/// The concatenated text of the `Str` elements in an inline (`query fromStr`).
fn str_text(inline: &Inline) -> String {
    let mut out = String::new();
    let mut pending = vec![inline];
    while let Some(inline) = pending.pop() {
        let parts = match inline {
            Inline::Str(text) => {
                out.push_str(text);
                continue;
            }
            Inline::Emph(parts)
            | Inline::Underline(parts)
            | Inline::Strong(parts)
            | Inline::Strikeout(parts)
            | Inline::Superscript(parts)
            | Inline::Subscript(parts)
            | Inline::SmallCaps(parts)
            | Inline::Quoted(_, parts)
            | Inline::Cite(_, parts)
            | Inline::Link(_, parts, _)
            | Inline::Image(_, parts, _)
            | Inline::Span(_, parts) => parts,
            // `query` also reaches the text of a note; CSL output has no notes.
            _ => continue,
        };
        pending.extend(parts.iter().rev());
    }
    out
}

fn is_word_break(c: char) -> bool {
    matches!(c, '-' | '/' | '\u{2013}' | '\u{2014}') || haskell::is_space(c)
}

/// `T.groupBy sameType`: each run is compared with its first character.
fn split_up(text: &str) -> Vec<&str> {
    let same = |c: char, d: char| {
        (is_alpha_num(c) && is_alpha_num(d))
            || (haskell::is_space(c) && haskell::is_space(d))
            || (is_punctuation(c) && is_punctuation(d))
    };
    let mut runs = Vec::new();
    let mut start = 0;
    let mut first: Option<char> = None;
    for (at, c) in text.char_indices() {
        match first {
            Some(f) if same(f, c) => {}
            Some(_) => {
                runs.push(&text[start..at]);
                start = at;
                first = Some(c);
            }
            None => first = Some(c),
        }
    }
    if start < text.len() {
        runs.push(&text[start..]);
    }
    runs
}

fn to_upper(turkish: bool, text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'i' if turkish => '\u{130}',
            '\u{131}' if turkish => 'I',
            c => c,
        })
        .flat_map(char::to_uppercase)
        .collect()
}

fn to_lower(turkish: bool, text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{130}' if turkish => 'i',
            'I' if turkish => '\u{131}',
            c => c,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

fn capitalize(turkish: bool, text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => to_upper(turkish, first.encode_utf8(&mut [0; 4])) + chars.as_str(),
        None => String::new(),
    }
}

fn is_capitalized(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(is_upper) && chars.all(is_lower)
}

fn is_mixed_case(text: &str) -> bool {
    text.chars().any(is_upper) && text.chars().any(is_lower)
}

fn uppercase_all(turkish: bool, _: State, run: &str) -> String {
    to_upper(turkish, run)
}

fn lowercase_all(turkish: bool, _: State, run: &str) -> String {
    to_lower(turkish, run)
}

fn capitalize_words(turkish: bool, state: State, run: &str) -> String {
    let at_word = matches!(
        state,
        State::Start | State::StartSentence | State::AfterWordEnd | State::BeforeLastWord
    );
    if !is_mixed_case(run) && at_word && run.chars().all(is_lower) {
        capitalize(turkish, run)
    } else {
        run.to_owned()
    }
}

fn capitalize_first(turkish: bool, state: State, run: &str) -> String {
    if !is_mixed_case(run) && state == State::Start && run.chars().all(is_lower) {
        capitalize(turkish, run)
    } else {
        run.to_owned()
    }
}

fn sentence_case(turkish: bool, state: State, run: &str) -> String {
    let at_start = matches!(state, State::Start | State::StartSentence);
    if is_capitalized(run) && !at_start {
        to_lower(turkish, run)
    } else if (is_capitalized(run) || run.chars().all(is_lower)) && at_start {
        capitalize(turkish, &to_lower(turkish, run))
    } else {
        run.to_owned()
    }
}

fn title_case(turkish: bool, state: State, run: &str) -> String {
    let long = run.chars().nth(1).is_some();
    let change = if is_mixed_case(run) || run.chars().all(|c| is_upper(c) || !is_letter(c)) {
        false
    } else {
        match state {
            State::StartSentence | State::Start => true,
            State::AfterWordEnd => !is_stop_word(run) && long,
            State::BeforeLastWord => long,
            _ => false,
        }
    };
    if change {
        capitalize(turkish, &to_lower(turkish, run))
    } else {
        run.to_owned()
    }
}

/// The stop words of title case (`isStopWord`). A run holds only letters
/// and digits, so the upstream words with a space, an apostrophe or a
/// period never match and are left out.
fn is_stop_word(run: &str) -> bool {
    STOP_WORDS.binary_search(&run).is_ok()
}

const STOP_WORDS: [&str; 98] = [
    "a",
    "about",
    "across",
    "afore",
    "after",
    "against",
    "along",
    "alongside",
    "amid",
    "amidst",
    "among",
    "amongst",
    "an",
    "and",
    "anenst",
    "apropos",
    "apud",
    "around",
    "as",
    "aside",
    "astride",
    "at",
    "athwart",
    "atop",
    "barring",
    "before",
    "behind",
    "below",
    "beneath",
    "beside",
    "besides",
    "between",
    "beyond",
    "but",
    "by",
    "c",
    "ca",
    "circa",
    "d",
    "de",
    "despite",
    "down",
    "during",
    "et",
    "except",
    "for",
    "forenenst",
    "from",
    "given",
    "in",
    "inside",
    "into",
    "l",
    "lest",
    "like",
    "modulo",
    "near",
    "next",
    "nor",
    "notwithstanding",
    "of",
    "off",
    "on",
    "onto",
    "or",
    "out",
    "over",
    "per",
    "plus",
    "pro",
    "qua",
    "sans",
    "since",
    "so",
    "than",
    "the",
    "through",
    "throughout",
    "thru",
    "thruout",
    "till",
    "to",
    "toward",
    "towards",
    "under",
    "underneath",
    "until",
    "unto",
    "up",
    "upon",
    "van",
    "versus",
    "via",
    "von",
    "with",
    "within",
    "without",
    "yet",
];
