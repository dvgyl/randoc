//! The tree rules. Each rule was found with Pandoc 3.13 as a black box.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use randoc_ast::walk::{walk_blocks_mut, walk_inlines_mut};
use randoc_ast::{
    Alignment, Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, ColWidth, Format,
    Inline, ListAttributes, ListNumberDelim, ListNumberStyle, MathType, Meta, MetaValue, QuoteType,
    Row, Table, TableBody, TableHead, Target, Text,
};
use randoc_core::shared::{IdentifierSet, auto_identifier, nfc};
use randoc_core::{Context, Extension, Extensions, LogMessage, SourcePos};

use crate::FieldOptions;
use crate::inlines::Inlines;
use crate::scope::Scoped;
use crate::tokens::{Kind, Tok, listings_language, tokenize};

/// Why the parser stopped before the end of the input.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Stop {
    /// The input is nested deeper than the limit of the context.
    TooDeep,
    /// A token that the grammar does not permit. The text is the message.
    Unexpected(Text),
    /// A nested document ends the enclosing list or table.
    NestedDocument,
    /// The table exceeds the configured cell limit.
    TableLimit,
    /// A macro expands without an end. The text is the macro name.
    Loop(Text),
    /// A parser loop made no progress, or the parser used its step budget.
    /// Both are defects of the reader. The text is the message.
    Stuck(Text),
}

type R<T> = Result<T, Stop>;

#[cfg(test)]
thread_local! {
    /// In tests: true makes each cache and index give way to its plain
    /// scan, so that a test can compare both.
    pub(crate) static PLAIN_SCANS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// True when the caches must give way to the plain scans (tests only).
pub(crate) fn plain_scans() -> bool {
    #[cfg(test)]
    {
        PLAIN_SCANS.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        false
    }
}

/// The expansion depth at which Randoc stops a macro as a loop.
pub(crate) const MAX_EXPANSION_DEPTH: u32 = 100;
/// The longest optional argument of `\cites` that the lookahead accepts.
const MAX_OPTION_TOKENS: usize = 256;
/// The number of tokens that all macro expansions together can make:
/// `MAX_EXPANDED_TOKENS` and `EXPANDED_PER_INPUT_TOKEN` for each token of
/// the input, so that a long document with a common macro reads, and a
/// macro that doubles stops in time linear in the input.
pub(crate) const MAX_EXPANDED_TOKENS: usize = 1_000_000;
pub(crate) const EXPANDED_PER_INPUT_TOKEN: usize = 20;
/// The step budget: the rounds of all parser loops together stay below
/// `ROUNDS_PER_TOKEN` rounds for each token of the input and of the
/// expansions, plus `BASE_ROUNDS`.
pub(crate) const ROUNDS_PER_TOKEN: usize = 64;
pub(crate) const BASE_ROUNDS: usize = 100_000;
/// The tokens that all live copies of rules that read a copy (a table
/// cell, an argument) hold together stay below `COPIES_PER_TOKEN` for each
/// token of the input and of the expansions, plus `BASE_COPIES`. A deeper
/// nesting of copies stops with the depth error, so that the memory stays
/// linear in the input.
pub(crate) const COPIES_PER_TOKEN: usize = 1;
pub(crate) const BASE_COPIES: usize = 50_000;

#[derive(Debug, Clone)]
struct Macro {
    parameters: usize,
    default: Option<Vec<Tok>>,
    body: Arc<[Tok]>,
    /// The parameter text of a `\def` with delimiters: the tokens before
    /// the first parameter, then the delimiter after each parameter (empty
    /// for an undelimited one). Empty for a macro without delimiters.
    delimiters: Option<Arc<Delimiters>>,
}

#[derive(Debug)]
struct Delimiters {
    id: usize,
    chunks: Vec<Delimiter>,
}

#[derive(Debug)]
struct Delimiter {
    tokens: Vec<Tok>,
    prefix: Vec<usize>,
}

impl Delimiter {
    fn new(tokens: Vec<Tok>) -> Self {
        let mut prefix = vec![0; tokens.len()];
        let mut length = 0;
        for index in 1..tokens.len() {
            let kind = &tokens[index].kind;
            while length > 0 && tokens[length].kind != *kind {
                length = prefix[length - 1];
            }
            if tokens[length].kind == *kind {
                length += 1;
            }
            prefix[index] = length;
        }
        Self { tokens, prefix }
    }

    fn advance(&self, mut length: usize, kind: &Kind) -> usize {
        while length > 0 && self.tokens[length].kind != *kind {
            length = self.prefix[length - 1];
        }
        length + usize::from(self.tokens[length].kind == *kind)
    }
}

struct DelimitedMatch {
    arguments: Vec<Vec<Tok>>,
    consumed: usize,
    carry: Option<Tok>,
}

enum DelimiterMiss {
    Mismatch,
    End(usize),
    Paragraph,
}

#[derive(Debug, Clone)]
struct EnvironmentMacro {
    parameters: usize,
    default: Option<Vec<Tok>>,
    begin: Vec<Tok>,
    end: Vec<Tok>,
}

/// Where a sequence of blocks ends.
#[derive(Debug, Clone, Copy, PartialEq)]
enum End<'a> {
    Input,
    Close,
    Environment(&'a str),
    Item(&'a str),
}

/// Where a sequence of inlines ends.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Until {
    /// At the `}` of the group.
    Close,
    /// At the end of a required inline argument.
    Argument,
    /// At a paragraph break or at the start of a block.
    Paragraph,
    /// At the closing quotation mark.
    Quote(QuoteType),
    /// At the `]` of an optional argument.
    Bracket,
}

/// The state of the guard of one parser loop: the consumed count at the
/// start of its last round.
#[derive(Default)]
struct Rounds(Option<usize>);

/// The longest pending macro expansion that an environment with a
/// fallback copies, so that the copies stay linear in the input.
const MAX_SNAPSHOT_PENDING: usize = 4096;

/// The parser state at the `\begin` of an environment with a fallback.
#[derive(Clone)]
struct Snapshot {
    position: usize,
    pending: Vec<Tok>,
    recorders: Vec<usize>,
    quote_kinds: usize,
    sections: [u64; 6],
    /// The source line and column of the `\begin`; `None` for a token of a
    /// macro expansion. Copies of tokens keep the source positions.
    source_position: Option<(u32, u32)>,
}

/// The state of a figure or a table environment.
#[derive(Debug, Default)]
struct Float {
    caption: Option<(Option<Vec<Inline>>, Vec<Inline>)>,
    label: Option<Text>,
}

pub(crate) struct Parser<'c> {
    tokens: Vec<Tok>,
    position: usize,
    /// Tokens of macro expansions, in reverse order: the last one is next.
    pending: Vec<Tok>,
    /// Source token for the first token of an active macro replacement.
    expansion_call: Option<Tok>,
    /// For the pending tokens from the bottom: the `\fi` count minus the
    /// count of words that start with `if` below each token, and the
    /// minimum of that count up to the token. With it, a conditional in
    /// `pending` finds in constant time that no `\fi` closes it.
    fi_profile: Vec<(i64, i64)>,
    /// The entries of `fi_profile` below this length are current.
    profile_valid: usize,
    macros: Scoped<Macro>,
    environments: Scoped<EnvironmentMacro>,
    /// The depth of TeX groups at the current position.
    group_level: u32,
    /// True after `\global`: the next definition is global.
    global: bool,
    /// The number of `\texttt` arguments around the current position:
    /// there, punctuation gives no dashes and no quotation marks.
    code_text: usize,
    extensions: Extensions,
    depth: usize,
    max_depth: usize,
    expanded: usize,
    recorders: Vec<Text>,
    floats: Vec<Float>,
    level_offset: i64,
    identifiers: IdentifierSet,
    /// The counters of the numbered sections, from `\chapter` to
    /// `\subparagraph`, and the number of each section label.
    sections: [u64; 6],
    section_numbers: HashMap<Text, Text>,
    pub(crate) meta: Meta,
    pub(crate) context: &'c mut Context,
    finished: bool,
    input: Text,
    line_offsets: Vec<usize>,
    /// The number of tokens consumed, a measure of the work.
    bumps: usize,
    /// The number of consumed tokens that a rule put back into the input.
    /// `bumps - unread` grows in each round of a loop that makes progress.
    unread: usize,
    /// The rounds of all parser loops, for the step budget.
    rounds: usize,
    /// The number of tokens of the input.
    input_tokens: usize,
    /// The name of the input file for error messages; empty for standard input.
    pub(crate) source_name: Text,
    /// The depth of `with_tokens`: rules that read a copy of tokens.
    copies: usize,
    /// The number of tokens in the live copies of `with_tokens`.
    copied: usize,
    /// The source positions of the tables that failed as tables.
    failed_tables: HashSet<(u32, u32)>,
    /// The input positions of the `\begin` tokens that have their `\end`.
    closed_begins: HashSet<usize>,
    /// For each `{` of the input: the position of its `}`.
    group_ends: HashMap<usize, usize>,
    /// The last failed search for the arguments of a macro in the input:
    /// its start, the position where the search stopped, and the count of
    /// arguments that it found. A later search between the two positions
    /// finds no more arguments.
    failed_arguments: Option<(usize, usize, usize)>,
    /// For each delimiter text: the last range in which a search for it
    /// failed at the end of the input.
    failed_delimiters: HashMap<(usize, bool), (usize, usize)>,
    next_delimiter_id: usize,
    restricted_inlines: bool,
    field: Option<FieldOptions>,
    /// The copy depth of the table cell that is read now, or 0. An
    /// environment that a cell ends inside is an error there.
    cell_copies: usize,
    /// The depth at which a `}` is a stray closer: 0, or the depth of the
    /// table cell that is read now.
    top_depth: usize,
    /// True in the preamble. Pandoc gives no parse error for a missing
    /// argument group there, but it does for a definition without a name.
    in_preamble: bool,
    /// The flags of `\newif`, without the `if`.
    flags: Scoped<bool>,
    /// For each word of the input that starts with `if`: the index of its
    /// `\else` and of its `\fi`, when it has a `\fi`.
    conditionals: HashMap<usize, (Option<usize>, usize)>,
    /// Jumps over the branches of conditionals that are not chosen: from a
    /// position of the input to the position after the `\fi`.
    skips: HashMap<usize, usize>,
    /// The kinds of the `\enquote` commands around the current position.
    quote_kinds: Vec<QuoteType>,
    /// The number of tokens that the lookahead rules examined.
    scanned: std::cell::Cell<usize>,
}

const NBSP: &str = "\u{a0}";

impl<'c> Parser<'c> {
    pub(crate) fn new(input: &str, extensions: Extensions, context: &'c mut Context) -> Self {
        let tokens = tokenize(input);
        let conditionals = match_conditionals(&tokens);
        let closed_begins = match_environments(&tokens);
        let group_ends = match_groups(&tokens);
        let has = |name: &str| tokens.iter().any(|tok| tok.is_word(name));
        let level_offset = if has("part") {
            2
        } else if has("chapter") {
            1
        } else {
            0
        };
        Parser {
            input_tokens: tokens.len(),
            tokens,
            position: 0,
            pending: Vec::new(),
            expansion_call: None,
            fi_profile: Vec::new(),
            profile_valid: 0,
            macros: Scoped::default(),
            environments: Scoped::default(),
            group_level: 0,
            global: false,
            code_text: 0,
            extensions,
            depth: 0,
            max_depth: context.limits.max_nesting_depth,
            expanded: 0,
            recorders: Vec::new(),
            floats: Vec::new(),
            level_offset,
            identifiers: IdentifierSet::new(),
            sections: [0; 6],
            section_numbers: HashMap::new(),
            meta: Meta::default(),
            context,
            finished: false,
            input: input.to_owned(),
            line_offsets: std::iter::once(0)
                .chain(input.match_indices('\n').map(|(index, _)| index + 1))
                .collect(),
            bumps: 0,
            unread: 0,
            rounds: 0,
            source_name: Text::new(),
            copies: 0,
            copied: 0,
            failed_tables: HashSet::new(),
            closed_begins,
            group_ends,
            failed_arguments: None,
            failed_delimiters: HashMap::new(),
            next_delimiter_id: 0,
            restricted_inlines: false,
            field: None,
            cell_copies: 0,
            top_depth: 0,
            in_preamble: false,
            scanned: std::cell::Cell::new(0),
            quote_kinds: Vec::new(),
            flags: Scoped::default(),
            skips: HashMap::new(),
            conditionals,
        }
    }

    /// The number of tokens that the parser consumed, a measure of its work.
    #[cfg(test)]
    pub(crate) fn steps(&self) -> usize {
        self.bumps + self.scanned.get()
    }

    pub(crate) fn field_inlines(&mut self, options: FieldOptions) -> R<Vec<Inline>> {
        self.field = Some(options);
        self.skip_spaces();
        while let Some(name) = self.field_definition_name() {
            self.consume_field_definition(&name)?;
            self.skip_spaces();
        }
        let result = self.inlines(Until::Close)?.trimmed();
        if self.peek_raw().is_some() {
            return Err(self.unexpected());
        }
        Ok(result)
    }

    pub(crate) fn document(&mut self) -> R<Vec<Block>> {
        let mut blocks = self.blocks(End::Input)?;
        if !self.section_numbers.is_empty() {
            let numbers = &self.section_numbers;
            walk_inlines_mut(&mut blocks, |inline| {
                if let Inline::Link(attr, text, _) = inline
                    && let Some(number) = section_reference(attr, numbers)
                {
                    *text = vec![Inline::Str(number)];
                }
            });
        }
        Ok(blocks)
    }

    // The token stream.

    fn peek_raw(&self) -> Option<&Tok> {
        if self.finished {
            return None;
        }
        self.pending
            .last()
            .or_else(|| self.tokens.get(self.raw_position()))
    }

    fn bump(&mut self) -> Option<Tok> {
        if self.finished {
            return None;
        }
        let tok = match self.pending.pop() {
            Some(tok) => {
                self.profile_valid = self.profile_valid.min(self.pending.len());
                tok
            }
            None => {
                self.position = self.raw_position();
                let tok = self.tokens.get(self.position)?.clone();
                self.position += 1;
                tok
            }
        };
        self.expansion_call = None;
        self.bumps += 1;
        for record in &mut self.recorders {
            record.push_str(&tok.text);
        }
        Some(tok)
    }

    /// Puts consumed tokens back into the input: the first one is next.
    fn unread(&mut self, tokens: Vec<Tok>) {
        self.unread += tokens.len();
        self.pending.extend(tokens.into_iter().rev());
    }

    /// The guard of each parser loop that calls other rules: call it at the
    /// start of each round. It stops the parser when the last round of the
    /// loop consumed no token, or when all rounds together pass the step
    /// budget, so that no loop can run without an end.
    fn round(&mut self, rounds: &mut Rounds) -> R<()> {
        let consumed = self.bumps - self.unread;
        if rounds.0 == Some(consumed) {
            return Err(self.stuck("made no progress"));
        }
        rounds.0 = Some(consumed);
        self.rounds += 1;
        let budget = ROUNDS_PER_TOKEN * (self.input_tokens + self.expanded) + BASE_ROUNDS;
        if self.rounds > budget {
            return Err(self.stuck("used its step budget"));
        }
        Ok(())
    }

    /// Runs `count` rounds of one loop that consumes nothing.
    #[cfg(test)]
    pub(crate) fn rounds_of_one_loop(&mut self, count: usize) -> R<()> {
        let mut rounds = Rounds::default();
        (0..count).try_for_each(|_| self.round(&mut rounds))
    }

    /// Runs `count` loops of one round each.
    #[cfg(test)]
    pub(crate) fn loops_of_one_round(&mut self, count: usize) -> R<()> {
        (0..count).try_for_each(|_| self.round(&mut Rounds::default()))
    }

    fn stuck(&self, what: &str) -> Stop {
        let SourcePos { line, column, .. } = self.position_of_next();
        Stop::Stuck(format!(
            "the latex reader {what} at line {line}, column {column}"
        ))
    }

    /// Expands macros until the next token is not a macro.
    fn expand(&mut self) -> R<()> {
        let macros = self.extensions.contains(Extension::LatexMacros);
        let mut rounds = Rounds::default();
        loop {
            self.round(&mut rounds)?;
            let Some(tok) = self.peek_raw() else {
                return Ok(());
            };
            let depth = tok.depth;
            let name = match &tok.kind {
                Kind::Word(name) => name.clone(),
                Kind::Symbol(c) if macros && self.macros.contains_key(&c.to_string()) => {
                    c.to_string()
                }
                _ => return Ok(()),
            };
            if is_stream_noop(&name) {
                self.bump();
                self.skip_word_spaces();
                continue;
            }
            if let Some(value) = self.condition_value(&name, macros) {
                if !self.conditional(value, depth)? {
                    return Ok(());
                }
                continue;
            }
            if !macros {
                return Ok(());
            }
            if self.flag_command(&name) {
                continue;
            }
            if name == "begin" {
                if !self.expand_environment()? {
                    return Ok(());
                }
                continue;
            }
            let Some(definition) = self.macros.get(&name).cloned() else {
                return Ok(());
            };
            if let Some(delimiters) = &definition.delimiters {
                let Some(DelimitedMatch {
                    arguments,
                    consumed,
                    carry,
                }) = self.delimited_arguments(delimiters)?
                else {
                    return Ok(());
                };
                let recorded: Vec<usize> = self.recorders.iter().map(Text::len).collect();
                let origin = self.expansion_call.take();
                let token = self.bump();
                let call = origin.or(token);
                for _ in 0..consumed {
                    self.bump();
                }
                if let Some(tok) = carry {
                    self.pending.push(tok);
                }
                for (record, len) in self.recorders.iter_mut().zip(recorded) {
                    record.truncate(len);
                }
                self.push_macro(&name, depth, &definition.body, &arguments, call)?;
                continue;
            }
            let required = definition
                .parameters
                .saturating_sub(usize::from(definition.default.is_some()));
            if !self.arguments_ahead(required) {
                return Ok(());
            }
            let recorded: Vec<usize> = self.recorders.iter().map(Text::len).collect();
            let origin = self.expansion_call.take();
            let token = self.bump();
            if token
                .as_ref()
                .is_some_and(|tok| matches!(tok.kind, Kind::Word(_)))
            {
                self.skip_word_spaces();
            }
            let call = origin.or(token);
            let arguments =
                self.macro_arguments(definition.parameters, definition.default, true)?;
            for (record, len) in self.recorders.iter_mut().zip(recorded) {
                record.truncate(len);
            }
            self.push_macro(&name, depth, &definition.body, &arguments, call)?;
        }
    }

    /// The arguments of a macro with delimited parameters, as TeX matches
    /// its parameter text: the arguments, the number of tokens after the
    /// macro name that they take, and the rest of a letter run that the
    /// last undelimited argument split. `None` when the input does not
    /// match: Pandoc then reads the macro as an unknown command.
    fn delimited_arguments(&mut self, delimiters: &Delimiters) -> R<Option<DelimitedMatch>> {
        let in_input = self.pending.is_empty() && self.skips.is_empty() && !plain_scans();
        let start = self.raw_position();
        let key = (delimiters.id, self.restricted_inlines);
        if in_input
            && let Some(&(from, to)) = self.failed_delimiters.get(&key)
            && from <= start
            && start < to
        {
            return Ok(None);
        }
        match self.match_delimited(&delimiters.chunks) {
            Ok(found) => Ok(Some(found)),
            Err(DelimiterMiss::End(used)) => {
                self.remember_failed_delimiter(in_input, key, start, used);
                Ok(None)
            }
            Err(DelimiterMiss::Mismatch) => Ok(None),
            Err(DelimiterMiss::Paragraph) => {
                if let Some(tok) = self.peek_raw() {
                    return Err(self.error_at(tok, "unexpected ()"));
                }
                Ok(None)
            }
        }
    }

    /// The plain match of `delimited_arguments`. The error holds the
    /// number of tokens examined when the input ends before a delimiter.
    fn match_delimited(&self, delimiters: &[Delimiter]) -> Result<DelimitedMatch, DelimiterMiss> {
        let mut tokens = self.scan().skip(1).peekable();
        let mut used = 0usize;
        while tokens.next_if(|tok| tok.kind == Kind::Space).is_some() {
            used += 1;
        }
        let (prefix, rest) = delimiters.split_first().ok_or(DelimiterMiss::Mismatch)?;
        for want in &prefix.tokens {
            used += 1;
            if tokens.next().is_none_or(|tok| tok.kind != want.kind) {
                return Err(DelimiterMiss::Mismatch);
            }
        }
        let mut carry: Option<Tok> = None;
        let mut arguments = Vec::new();
        for delimiter in rest {
            let mut argument = Vec::new();
            if delimiter.tokens.is_empty() {
                if carry.is_none() {
                    while tokens.next_if(|tok| tok.kind == Kind::Space).is_some() {
                        used += 1;
                    }
                }
                let tok = match carry.take() {
                    Some(tok) => tok,
                    None => {
                        used += 1;
                        tokens.next().ok_or(DelimiterMiss::Mismatch)?.clone()
                    }
                };
                match &tok.kind {
                    Kind::Open => {
                        let mut nesting = 0usize;
                        loop {
                            used += 1;
                            let inner = tokens.next().ok_or(DelimiterMiss::Mismatch)?;
                            match inner.kind {
                                Kind::Open => nesting += 1,
                                Kind::Close if nesting == 0 => break,
                                Kind::Close => nesting -= 1,
                                _ => {}
                            }
                            argument.push(inner.clone());
                        }
                    }
                    Kind::Close => return Err(DelimiterMiss::Mismatch),
                    Kind::Letters(text) if text.chars().nth(1).is_some() => {
                        let mut chars = text.chars();
                        let first: Text = chars.next().map(String::from).unwrap_or_default();
                        let rest: Text = chars.collect();
                        argument.push(Tok {
                            kind: Kind::Letters(first.clone()),
                            text: first,
                            ..tok.clone()
                        });
                        carry = Some(Tok {
                            kind: Kind::Letters(rest.clone()),
                            text: rest,
                            column: tok.column + 1,
                            ..tok.clone()
                        });
                    }
                    _ => argument.push(tok),
                }
            } else {
                let mut nesting = 0usize;
                if let Some(tok) = carry.take() {
                    argument.push(tok);
                }
                let mut matched = 0;
                loop {
                    used += 1;
                    let Some(tok) = tokens.next() else {
                        return Err(DelimiterMiss::End(used));
                    };
                    if self.restricted_inlines && tok.kind == Kind::Par {
                        return Err(DelimiterMiss::Paragraph);
                    }
                    if nesting == 0 {
                        matched = delimiter.advance(matched, &tok.kind);
                    }
                    match tok.kind {
                        Kind::Open => nesting += 1,
                        Kind::Close if nesting == 0 => return Err(DelimiterMiss::Mismatch),
                        Kind::Close => nesting -= 1,
                        _ => {}
                    }
                    argument.push(tok.clone());
                    if matched == delimiter.tokens.len() {
                        argument.truncate(argument.len() - matched);
                        break;
                    }
                }
                // Pandoc drops the braces of the groups at the top of a
                // delimited argument.
                let mut depth = 0usize;
                argument.retain(|tok| match tok.kind {
                    Kind::Open => {
                        depth += 1;
                        depth > 1
                    }
                    Kind::Close => {
                        depth = depth.saturating_sub(1);
                        depth > 0
                    }
                    _ => true,
                });
            }
            arguments.push(argument);
        }
        Ok(DelimitedMatch {
            arguments,
            consumed: used,
            carry,
        })
    }

    /// Remembers that a delimiter does not follow `start` up to the end of
    /// the input, when no group is in that range: a later search from a
    /// position in it fails too.
    fn remember_failed_delimiter(
        &mut self,
        in_input: bool,
        key: (usize, bool),
        start: usize,
        used: usize,
    ) {
        let grouped = self
            .tokens
            .get(start..start + used)
            .is_none_or(|range| range.iter().any(|tok| tok.kind == Kind::Open));
        if in_input && !grouped {
            self.failed_delimiters.insert(key, (start, start + used));
        }
    }

    /// True when `count` required arguments follow the macro name. Pandoc
    /// reads a macro without them as an unknown command. An argument is a
    /// group or one token after blank space; a `}` or the end of the input
    /// is none.
    fn arguments_ahead(&mut self, count: usize) -> bool {
        if plain_scans() {
            return self.count_arguments(count).0;
        }
        let in_input = self.pending.is_empty() && self.skips.is_empty();
        let start = self.raw_position();
        if in_input
            && let Some((from, to, found)) = self.failed_arguments
            && from < start
            && start < to
            && found < count
        {
            return false;
        }
        let (ahead, found, examined) = self.count_arguments(count);
        // Without a group in the range, a search from a later position in
        // it sees a part of the same tokens, so it finds fewer arguments.
        let grouped = self
            .tokens
            .get(start..start + examined)
            .is_none_or(|range| range.iter().any(|tok| tok.kind == Kind::Open));
        if !ahead && in_input && !grouped {
            self.failed_arguments = Some((start, start + examined, found));
        }
        ahead
    }

    /// The plain search of `arguments_ahead`: whether `count` arguments
    /// follow, the arguments found, and the number of tokens examined.
    fn count_arguments(&self, count: usize) -> (bool, usize, usize) {
        let examined = std::cell::Cell::new(1usize);
        let mut tokens = self
            .scan()
            .skip(1)
            .inspect(|_| examined.set(examined.get() + 1));
        let mut found = 0usize;
        while found < count {
            let Some(tok) = tokens.next() else {
                return (false, found, examined.get());
            };
            match &tok.kind {
                Kind::Space | Kind::Newline | Kind::Comment | Kind::Par => {}
                Kind::Close => return (false, found, examined.get()),
                Kind::Open => {
                    let mut nesting = 1usize;
                    while nesting > 0 {
                        match tokens.next().map(|tok| &tok.kind) {
                            None => return (false, found, examined.get()),
                            Some(Kind::Open) => nesting += 1,
                            Some(Kind::Close) => nesting -= 1,
                            Some(_) => {}
                        }
                    }
                    found += 1;
                }
                Kind::Letters(text) => found += text.chars().count(),
                _ => found += 1,
            }
        }
        (true, found, examined.get())
    }

    /// The arguments of a macro use. The option of an environment must
    /// follow its name directly; the option of a command can follow spaces.
    fn macro_arguments(
        &mut self,
        count: usize,
        default: Option<Vec<Tok>>,
        option_after_spaces: bool,
    ) -> R<Vec<Vec<Tok>>> {
        let mut arguments = Vec::new();
        if let Some(default) = default {
            let gap = self.bracket_gap().is_some_and(|(skip, _)| skip > 0);
            let option = if gap && !option_after_spaces {
                None
            } else {
                self.bracket_tokens_on_line()
            };
            arguments.push(option.unwrap_or(default));
        }
        while arguments.len() < count {
            self.skip_blank();
            let Some(argument) = self.argument_tokens() else {
                break;
            };
            arguments.push(argument);
        }
        Ok(arguments)
    }

    fn push_macro(
        &mut self,
        name: &str,
        depth: u32,
        body: &[Tok],
        arguments: &[Vec<Tok>],
        call: Option<Tok>,
    ) -> R<()> {
        let limit = self.expansion_limit();
        let (replacement, missing) =
            substitute(body, arguments, limit).ok_or_else(|| Stop::Loop(name.into()))?;
        if let Some(call) = call.as_ref().filter(|_| missing) {
            if replacement.is_empty() {
                return Err(self.macro_not_found(call, false));
            }
            let mut nesting = 0usize;
            for token in &replacement {
                match token.kind {
                    Kind::Open => nesting += 1,
                    Kind::Close => nesting = nesting.saturating_sub(1),
                    _ => {}
                }
            }
            if nesting != 0 {
                let width = u32::try_from(call.text.chars().count()).unwrap_or(1);
                return Err(self.error_at_position(
                    call.line,
                    call.column + width.saturating_sub(1),
                    "unexpected end of input",
                ));
            }
        }
        if missing {
            self.pending.clear();
            self.position = self.tokens.len();
            self.profile_valid = 0;
        }
        let has_tokens = !replacement.is_empty();
        self.push_tokens(name, depth, replacement)?;
        if has_tokens {
            self.expansion_call = call;
        }
        Ok(())
    }

    fn expansion_limit(&self) -> usize {
        (MAX_EXPANDED_TOKENS + EXPANDED_PER_INPUT_TOKEN * self.input_tokens)
            .saturating_sub(self.expanded)
    }

    fn push_expansion(
        &mut self,
        name: &str,
        depth: u32,
        body: &[Tok],
        arguments: &[Vec<Tok>],
    ) -> R<()> {
        let (replacement, _) = substitute(body, arguments, self.expansion_limit())
            .ok_or_else(|| Stop::Loop(name.into()))?;
        self.push_tokens(name, depth, replacement)
    }

    /// Puts the tokens of an expansion into the input.
    fn push_tokens(&mut self, name: &str, depth: u32, mut out: Vec<Tok>) -> R<()> {
        if depth + 1 > MAX_EXPANSION_DEPTH {
            return Err(Stop::Loop(name.to_owned()));
        }
        if out.len() > self.expansion_limit() {
            return Err(Stop::Loop(name.to_owned()));
        }
        self.expanded += out.len();
        for tok in &mut out {
            tok.depth = depth + 1;
        }
        self.pending.extend(out.into_iter().rev());
        Ok(())
    }

    /// Expands `\begin{NAME}` of an environment that the input defines.
    /// Returns false when NAME is not such an environment.
    fn expand_environment(&mut self) -> R<bool> {
        let Some(name) = self.environment_name_ahead() else {
            return Ok(false);
        };
        let Some(definition) = self.environments.get(&name).cloned() else {
            return Ok(false);
        };
        let depth = self.peek_raw().map_or(0, |tok| tok.depth);
        self.bump();
        self.argument_tokens();
        let arguments = self.macro_arguments(definition.parameters, definition.default, false)?;
        let (body, _) = self.environment_body_tokens(&name);
        let limit = self
            .expansion_limit()
            .saturating_sub(body.len())
            .saturating_sub(2);
        let (begin, _) = substitute(&definition.begin, &arguments, limit)
            .ok_or_else(|| Stop::Loop(name.clone()))?;
        let (end, _) = substitute(&definition.end, &arguments, limit - begin.len())
            .ok_or_else(|| Stop::Loop(name.clone()))?;
        let mut all = vec![self.synthetic(Kind::Open, "{")];
        all.extend(begin);
        all.extend(body);
        all.extend(end);
        all.push(self.synthetic(Kind::Close, "}"));
        self.push_tokens(&name, depth, all)?;
        Ok(true)
    }

    fn synthetic(&self, kind: Kind, text: &str) -> Tok {
        Tok {
            kind,
            text: text.to_owned(),
            line: 0,
            column: 0,
            depth: 0,
        }
    }

    /// The tokens from the next one on, without macro expansion.
    fn ahead(&self) -> impl Iterator<Item = &Tok> + Clone + '_ {
        let rest = self.tokens.get(self.raw_position()..).unwrap_or_default();
        self.pending.iter().rev().chain(rest)
    }

    /// True when the group that the next token opens holds a paragraph
    /// break or a block at any depth. Pandoc reads such a group as blocks.
    /// The scan stops at the first block, so a run of such groups is linear.
    fn group_holds_block(&self) -> bool {
        let mut nesting = 0usize;
        let mut tokens = self.scan();
        while let Some(tok) = tokens.next() {
            match &tok.kind {
                Kind::Open => nesting += 1,
                Kind::Close => {
                    nesting = nesting.saturating_sub(1);
                    if nesting == 0 {
                        return false;
                    }
                }
                Kind::Par => return true,
                Kind::Word(name) if name == "begin" => {
                    let environment = name_in(tokens.clone().take(64));
                    if environment.is_some_and(|name| !is_inline_environment(&name)) {
                        return true;
                    }
                }
                Kind::Word(name)
                    if sectioning_level(name).is_some()
                        || matches!(name.as_str(), "title" | "author" | "date") =>
                {
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// True when the group that the next token opens holds one group and
    /// spaces before it, nothing else. Pandoc reads the inner group as the
    /// argument: `\emph{{x}}` is `\emph{x}`, `\emph{{x} }` keeps a group.
    fn whole_group_ahead(&self) -> bool {
        self.whole_group_after(1)
    }

    /// `whole_group_ahead` for the group that opens `skip` tokens before the
    /// next token: 0 after its `{` was consumed.
    fn whole_group_after(&self, skip: usize) -> bool {
        if self.pending.is_empty() && self.skips.is_empty() && !plain_scans() {
            let mut at = self.raw_position() + skip;
            while self
                .tokens
                .get(at)
                .is_some_and(|tok| matches!(tok.kind, Kind::Space | Kind::Newline))
            {
                at += 1;
            }
            self.scanned
                .set(self.scanned.get() + at - self.raw_position());
            let Some(&end) = self.group_ends.get(&at) else {
                return false;
            };
            return self
                .tokens
                .get(end + 1)
                .is_some_and(|tok| tok.kind == Kind::Close);
        }
        let mut tokens = self
            .scan()
            .skip(skip)
            .skip_while(|tok| matches!(tok.kind, Kind::Space | Kind::Newline));
        if tokens.next().is_none_or(|tok| tok.kind != Kind::Open) {
            return false;
        }
        let mut nesting = 1usize;
        for tok in tokens.by_ref() {
            match tok.kind {
                Kind::Open => nesting += 1,
                Kind::Close => {
                    nesting -= 1;
                    if nesting == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        nesting == 0 && tokens.next().is_some_and(|tok| tok.kind == Kind::Close)
    }

    /// The value of a TeX conditional: `\iftrue`, `\iffalse`, and with
    /// `latex_macros` a conditional of `\newif`. Other `\if` words are not
    /// conditionals for Pandoc.
    fn condition_value(&self, name: &str, macros: bool) -> Option<bool> {
        match name {
            "iftrue" => Some(true),
            "iffalse" => Some(false),
            _ if macros => name
                .strip_prefix("if")
                .and_then(|flag| self.flags.get(flag).copied()),
            _ => None,
        }
    }

    /// `\newif\ifNAME`, `\NAMEtrue` and `\NAMEfalse`. Returns false for
    /// other words.
    fn flag_command(&mut self, name: &str) -> bool {
        if name == "newif" {
            let flag = self
                .ahead()
                .skip(1)
                .find(|tok| tok.kind != Kind::Space)
                .and_then(|tok| match &tok.kind {
                    Kind::Word(word) => word.strip_prefix("if").map(Text::from),
                    _ => None,
                });
            let Some(flag) = flag else {
                return false;
            };
            self.bump();
            self.skip_word_spaces();
            self.bump();
            self.skip_word_spaces();
            self.flags.define(flag, false, self.group_level, false);
            return true;
        }
        match self.flag_setting(name) {
            Some((flag, value)) if self.flags.contains_key(flag) => {
                let global = std::mem::take(&mut self.global);
                self.flags
                    .define(flag.to_owned(), value, self.group_level, global);
                self.bump();
                true
            }
            _ => false,
        }
    }

    /// The flag and the value of `\NAMEtrue` or `\NAMEfalse`.
    fn flag_setting<'n>(&self, name: &'n str) -> Option<(&'n str, bool)> {
        [("true", true), ("false", false)]
            .into_iter()
            .find_map(|(suffix, value)| Some((name.strip_suffix(suffix)?, value)))
    }

    fn is_flag_setter(&self, name: &str) -> bool {
        self.flag_setting(name)
            .is_some_and(|(flag, _)| self.flags.contains_key(flag))
    }

    /// Replaces the conditional at the next token with the branch that
    /// `value` selects. Returns false, and consumes nothing, when the
    /// conditional has no `\fi`: Pandoc then reads its words as unknown.
    /// The branch counts as an expansion: a conditional nested deeper than
    /// `MAX_EXPANSION_DEPTH` stops with the depth error.
    fn conditional(&mut self, value: bool, depth: u32) -> R<bool> {
        if self.pending.is_empty() {
            return Ok(self.conditional_in_input(value));
        }
        if !self.fi_in_pending() {
            return Ok(false);
        }
        let Some((else_at, fi_at)) = self.branches_ahead() else {
            return Ok(false);
        };
        if depth + 1 > MAX_EXPANSION_DEPTH {
            return Err(Stop::TooDeep);
        }
        self.bump();
        let mut chosen = Vec::new();
        let mut after_word = true;
        for offset in 1..fi_at {
            let Some(tok) = self.bump() else {
                break;
            };
            let in_true = else_at.is_none_or(|at| offset < at);
            let in_false = else_at.is_some_and(|at| offset > at);
            let skipped = after_word && tok.kind == Kind::Space;
            after_word = Some(offset) == else_at || skipped;
            if !skipped && ((value && in_true) || (!value && in_false)) {
                chosen.push(Tok {
                    depth: depth + 1,
                    ..tok
                });
            }
        }
        self.bump();
        self.skip_word_spaces();
        self.unread(chosen);
        Ok(true)
    }

    /// A conditional of the input itself: the branches stay in place, and a
    /// jump in `skips` passes over the branch that is not chosen. So nested
    /// conditionals copy no tokens.
    fn conditional_in_input(&mut self, value: bool) -> bool {
        let at = self.raw_position();
        let Some(&(else_at, fi_at)) = self.conditionals.get(&at) else {
            return false;
        };
        let mut after_fi = fi_at + 1;
        while self
            .tokens
            .get(after_fi)
            .is_some_and(|tok| tok.kind == Kind::Space)
        {
            after_fi += 1;
        }
        self.bump();
        match (value, else_at) {
            (true, _) => {
                self.skips.insert(else_at.unwrap_or(fi_at), after_fi);
            }
            (false, Some(else_at)) => {
                self.position = else_at + 1;
                self.skips.insert(fi_at, after_fi);
            }
            (false, None) => self.position = after_fi,
        }
        self.skip_word_spaces();
        true
    }

    /// The position of the next input token after the jumps of `skips`.
    fn raw_position(&self) -> usize {
        let mut position = self.position;
        while let Some(&to) = self.skips.get(&position) {
            position = to;
        }
        position
    }

    /// The offsets of the `\else` and the `\fi` of the conditional at the
    /// next token, counted from it, inside the expanded tokens. Each word
    /// that starts with `if` nests.
    fn branches_ahead(&self) -> Option<(Option<usize>, usize)> {
        let mut nesting = 0usize;
        let mut else_at = None;
        let expanded = self.pending.len();
        for (offset, tok) in self.scan().enumerate().take(expanded).skip(1) {
            let Kind::Word(word) = &tok.kind else {
                continue;
            };
            match word.as_str() {
                "fi" if nesting == 0 => return Some((else_at, offset)),
                "fi" => nesting -= 1,
                "else" if nesting == 0 && else_at.is_none() => else_at = Some(offset),
                _ if word.starts_with("if") => nesting += 1,
                _ => {}
            }
        }
        None
    }

    /// False when no `\fi` in `pending` closes the conditional on its top.
    /// It updates the profile of the tokens pushed since the last call, so
    /// that repeated conditionals without `\fi` take linear time.
    fn fi_in_pending(&mut self) -> bool {
        if plain_scans() {
            return true;
        }
        let weight = |tok: &Tok| match &tok.kind {
            Kind::Word(word) if word == "fi" => 1,
            Kind::Word(word) if word.starts_with("if") => -1,
            _ => 0,
        };
        let keep = self.profile_valid.min(self.pending.len());
        self.fi_profile.truncate(keep);
        for index in keep..self.pending.len() {
            let entry = match index
                .checked_sub(1)
                .and_then(|below| Some((self.fi_profile.get(below)?, self.pending.get(below)?)))
            {
                Some((&(count, minimum), tok)) => {
                    let count = count + weight(tok);
                    (count, minimum.min(count))
                }
                None => (0, 0),
            };
            self.fi_profile.push(entry);
        }
        self.profile_valid = self.pending.len();
        let Some(top) = self.pending.len().checked_sub(1) else {
            return false;
        };
        match (
            top.checked_sub(1)
                .and_then(|below| self.fi_profile.get(below)),
            self.fi_profile.get(top),
        ) {
            (Some(&(_, minimum)), Some(&(count, _))) => minimum < count,
            _ => false,
        }
    }

    /// The tokens from the next one on, counted as the work of a lookahead rule.
    fn scan(&self) -> impl Iterator<Item = &Tok> + Clone + '_ {
        self.ahead()
            .inspect(|_| self.scanned.set(self.scanned.get() + 1))
    }

    /// The name of `\begin{NAME}` when the next token is `\begin`.
    fn environment_name_ahead(&self) -> Option<Text> {
        name_in(self.ahead().skip(1).take(64))
    }

    /// The name of `{NAME}` after a `\begin` that was consumed.
    fn environment_name_ahead_of_argument(&self) -> Option<Text> {
        name_in(self.ahead().take(64))
    }

    /// The tokens up to the `\end{NAME}` that closes the environment, which
    /// it consumes, and false when the input ends before that `\end`.
    fn environment_body_tokens(&mut self, name: &str) -> (Vec<Tok>, bool) {
        let mut nesting = 0usize;
        let mut body = Vec::new();
        while let Some(tok) = self.peek_raw() {
            let begins = tok.is_word("begin");
            let ends = tok.is_word("end");
            if begins || ends {
                let ahead = self.environment_name_ahead();
                if ahead.as_deref() == Some(name) {
                    if ends && nesting == 0 {
                        self.bump();
                        self.argument_tokens();
                        return (body, true);
                    }
                    if begins {
                        nesting += 1;
                    } else {
                        nesting -= 1;
                    }
                }
            }
            if let Some(tok) = self.bump() {
                body.push(tok);
            }
        }
        (body, false)
    }

    /// The tokens of a math environment up to its `\end{NAME}`, which it
    /// consumes, and false when the input ends before that `\end`. Macros
    /// expand, also into the `\end`, as in Pandoc.
    fn math_body_tokens(&mut self, name: &str) -> R<(Vec<Tok>, bool)> {
        let mut nesting = 0usize;
        let mut body = Vec::new();
        let mut rounds = Rounds::default();
        while let Some(kind) = self.peek_kind()? {
            self.round(&mut rounds)?;
            if let Kind::Word(word) = &kind
                && (word == "begin" || word == "end")
                && self.environment_name_ahead().as_deref() == Some(name)
            {
                if word == "end" && nesting == 0 {
                    self.bump();
                    self.argument_tokens();
                    return Ok((body, true));
                }
                if word == "begin" {
                    nesting += 1;
                } else {
                    nesting -= 1;
                }
            }
            if let Some(tok) = self.bump() {
                body.push(tok);
            }
        }
        Ok((body, false))
    }

    fn peek_kind(&mut self) -> R<Option<Kind>> {
        self.expand()?;
        Ok(self.peek_raw().map(|tok| tok.kind.clone()))
    }

    fn skip_spaces(&mut self) {
        while matches!(
            self.peek_raw().map(|tok| &tok.kind),
            Some(Kind::Space | Kind::Newline | Kind::Comment)
        ) {
            self.bump();
        }
    }

    fn skip_blank(&mut self) {
        while matches!(
            self.peek_raw().map(|tok| &tok.kind),
            Some(Kind::Space | Kind::Newline | Kind::Comment | Kind::Par)
        ) {
            self.bump();
        }
    }

    /// Skips the spaces after a control word, as TeX does.
    fn skip_word_spaces(&mut self) {
        while matches!(self.peek_raw().map(|tok| &tok.kind), Some(Kind::Space)) {
            self.bump();
        }
    }

    fn star(&mut self) -> bool {
        if matches!(self.peek_raw().map(|tok| &tok.kind), Some(Kind::Other('*'))) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// The tokens of a `{...}` group without its braces, or of one token.
    /// One letter of a run of letters is one token, as in TeX.
    fn argument_tokens(&mut self) -> Option<Vec<Tok>> {
        self.skip_spaces();
        match &self.peek_raw()?.kind {
            Kind::Open => {
                self.bump();
                Some(self.tokens_until_close())
            }
            Kind::Close | Kind::Par => None,
            Kind::Letters(text) if text.chars().nth(1).is_some() => {
                let tok = self.bump()?;
                let mut letters: Vec<Tok> = tok
                    .text
                    .chars()
                    .zip(tok.column..)
                    .map(|(letter, column)| Tok {
                        kind: Kind::Letters(letter.to_string()),
                        text: letter.to_string(),
                        column,
                        ..tok.clone()
                    })
                    .collect();
                let first = letters.remove(0);
                self.pending.extend(letters.into_iter().rev());
                Some(vec![first])
            }
            _ => self.bump().map(|tok| vec![tok]),
        }
    }

    /// The tokens up to the `}` that closes the current group, which it consumes.
    fn tokens_until_close(&mut self) -> Vec<Tok> {
        self.tokens_until_close_or_end().0
    }

    /// `tokens_until_close`, and false when the input ends before the `}`.
    fn tokens_until_close_or_end(&mut self) -> (Vec<Tok>, bool) {
        let mut nesting = 0usize;
        let mut out = Vec::new();
        while let Some(tok) = self.bump() {
            match tok.kind {
                Kind::Open => nesting += 1,
                Kind::Close if nesting == 0 => return (out, true),
                Kind::Close => nesting -= 1,
                _ => {}
            }
            out.push(tok);
        }
        (out, false)
    }

    /// Skips the spaces, comments and line ends before a `[`: Pandoc
    /// reads an optional argument after them. Consumes nothing when no `[`
    /// follows.
    fn skip_to_bracket(&mut self) {
        if let Some((skip, _)) = self.bracket_gap() {
            for _ in 0..skip {
                self.bump();
            }
        }
    }

    /// The number of tokens before the `[` of an optional argument, and
    /// true when a line end is among them.
    fn bracket_gap(&self) -> Option<(usize, bool)> {
        let mut tokens = self.scan().peekable();
        let gap = skip_option_gap(&mut tokens, usize::MAX);
        tokens
            .next()
            .filter(|tok| tok.kind == Kind::Other('['))
            .map(|_| gap)
    }

    /// The `[...]` argument of `\caption` and of a macro: Pandoc reads it
    /// after spaces, but not after a line end.
    fn bracket_tokens_on_line(&mut self) -> Option<Vec<Tok>> {
        if self.bracket_gap().is_some_and(|(_, line_end)| line_end) {
            return None;
        }
        self.bracket_tokens()
    }

    /// Skips each `[...]` argument that follows, as Pandoc does after a
    /// footnote, a section, a table and most environments.
    fn skip_options(&mut self) -> R<()> {
        let mut rounds = Rounds::default();
        loop {
            self.round(&mut rounds)?;
            if self.bracket_tokens().is_none() {
                return Ok(());
            }
        }
    }

    /// The tokens of a `[...]` argument without its brackets.
    fn bracket_tokens(&mut self) -> Option<Vec<Tok>> {
        self.skip_to_bracket();
        let start = self.peek_raw()?;
        if start.kind != Kind::Other('[') {
            return None;
        }
        self.bump();
        let mut nesting = 0usize;
        let mut out = Vec::new();
        while let Some(tok) = self.bump() {
            match tok.kind {
                Kind::Open => nesting += 1,
                Kind::Close => nesting = nesting.saturating_sub(1),
                Kind::Other(']') if nesting == 0 => break,
                _ => {}
            }
            out.push(tok);
        }
        Some(out)
    }

    /// The source text of a required argument, without macro expansion.
    fn argument_source_text(&mut self) -> Text {
        self.argument_tokens()
            .map(|tokens| tokens_text(&tokens))
            .unwrap_or_default()
    }

    /// The text of a required argument after macro expansion, as Pandoc
    /// reads keys, labels, paths and names.
    fn argument_text(&mut self) -> R<Text> {
        self.skip_spaces();
        if !self.peek_raw().is_some_and(|tok| tok.kind == Kind::Open) {
            return Ok(self.argument_source_text());
        }
        self.bump();
        let mut nesting = 0usize;
        let mut text = Text::new();
        let mut rounds = Rounds::default();
        while let Some(kind) = self.peek_kind()? {
            self.round(&mut rounds)?;
            match kind {
                Kind::Open => nesting += 1,
                Kind::Close if nesting == 0 => {
                    self.bump();
                    break;
                }
                Kind::Close => nesting -= 1,
                _ => {}
            }
            if let Some(tok) = self.bump() {
                text.push_str(&tok.text);
            }
        }
        Ok(text)
    }

    /// Runs a nested rule in a TeX group.
    fn group<T>(&mut self, run: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        self.scope(|parser| parser.nested(run))
    }

    /// Runs a rule in a TeX group: the local definitions of the rule end
    /// with it.
    fn scope<T>(&mut self, run: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        let level = self.group_level;
        self.group_level += 1;
        let result = run(self);
        self.group_level = level;
        self.macros.end_groups(level);
        self.environments.end_groups(level);
        self.flags.end_groups(level);
        result
    }

    fn nested<T>(&mut self, run: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        if self.depth >= self.max_depth {
            return Err(Stop::TooDeep);
        }
        self.depth += 1;
        let result = run(self);
        self.depth -= 1;
        result
    }

    /// Runs a rule on a token list instead of the input.
    fn with_tokens<T>(&mut self, tokens: Vec<Tok>, run: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        let size = tokens.len();
        if self.copied + size > COPIES_PER_TOKEN * (self.input_tokens + self.expanded) + BASE_COPIES
        {
            return Err(Stop::TooDeep);
        }
        self.copied += size;
        let result = self.with_copy(tokens, run);
        self.copied -= size;
        result
    }

    fn with_copy<T>(&mut self, tokens: Vec<Tok>, run: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        self.nested(|parser| {
            let saved_tokens = std::mem::replace(&mut parser.tokens, tokens);
            let saved_position = std::mem::replace(&mut parser.position, 0);
            let saved_pending = std::mem::take(&mut parser.pending);
            let saved_expansion_call = parser.expansion_call.take();
            let saved_profile = std::mem::take(&mut parser.fi_profile);
            let saved_valid = std::mem::replace(&mut parser.profile_valid, 0);
            let saved_finished = std::mem::replace(&mut parser.finished, false);
            let saved_conditionals =
                std::mem::replace(&mut parser.conditionals, match_conditionals(&parser.tokens));
            let saved_skips = std::mem::take(&mut parser.skips);
            let saved_begins = std::mem::replace(
                &mut parser.closed_begins,
                match_environments(&parser.tokens),
            );
            let saved_groups =
                std::mem::replace(&mut parser.group_ends, match_groups(&parser.tokens));
            let saved_failed = parser.failed_arguments.take();
            let saved_delimiters = std::mem::take(&mut parser.failed_delimiters);
            parser.copies += 1;
            let result = run(parser);
            parser.copies -= 1;
            parser.conditionals = saved_conditionals;
            parser.skips = saved_skips;
            parser.closed_begins = saved_begins;
            parser.group_ends = saved_groups;
            parser.failed_arguments = saved_failed;
            parser.failed_delimiters = saved_delimiters;
            parser.tokens = saved_tokens;
            parser.position = saved_position;
            parser.pending = saved_pending;
            parser.expansion_call = saved_expansion_call;
            parser.fi_profile = saved_profile;
            parser.profile_valid = saved_valid;
            parser.finished = saved_finished;
            result
        })
    }

    fn inlines_of(&mut self, tokens: Vec<Tok>) -> R<Vec<Inline>> {
        self.scope(|parser| {
            parser.with_tokens(tokens, |parser| Ok(parser.inlines(Until::Close)?.trimmed()))
        })
    }

    fn blocks_of(&mut self, tokens: Vec<Tok>) -> R<Vec<Block>> {
        self.scope(|parser| parser.with_tokens(tokens, |parser| parser.blocks(End::Input)))
    }

    /// The inlines of a required argument, without the spaces at the ends.
    fn argument_inlines(&mut self) -> R<Vec<Inline>> {
        Ok(Inlines(self.argument_inlines_untrimmed()?).trimmed())
    }

    /// The inlines of a required argument. A group is read in place, so
    /// that nested arguments are not copied.
    fn argument_inlines_untrimmed(&mut self) -> R<Vec<Inline>> {
        self.skip_spaces();
        if self.peek_raw().is_some_and(|tok| tok.kind == Kind::Open) {
            self.bump();
            self.skip_stream_noops();
            let inlines = if self.whole_group_after(0) {
                self.group(Self::argument_inlines_untrimmed)?
            } else {
                self.group(|parser| parser.inlines(Until::Argument))?.0
            };
            self.close_group()?;
            return Ok(inlines);
        }
        let tokens = self.argument_tokens().unwrap_or_default();
        if tokens
            .first()
            .is_some_and(|tok| matches!(tok.kind, Kind::Word(_)))
        {
            self.skip_word_spaces();
        }
        self.inlines_of(tokens)
    }

    /// The inlines of an optional `[...]` argument, read in place.
    fn skip_stream_noops(&mut self) {
        while self
            .peek_raw()
            .is_some_and(|tok| matches!(&tok.kind, Kind::Word(name) if is_stream_noop(name)))
        {
            self.bump();
            self.skip_word_spaces();
        }
    }

    fn bracket_inlines(&mut self) -> R<Option<Vec<Inline>>> {
        self.skip_to_bracket();
        if !self
            .peek_raw()
            .is_some_and(|tok| tok.kind == Kind::Other('['))
        {
            return Ok(None);
        }
        self.bump();
        let inlines = self.nested(|parser| parser.inlines(Until::Bracket))?;
        if self
            .peek_raw()
            .is_some_and(|tok| tok.kind == Kind::Other(']'))
        {
            self.bump();
        }
        Ok(Some(inlines.trimmed()))
    }

    /// The blocks of a required argument, read in place.
    fn argument_blocks(&mut self) -> R<Vec<Block>> {
        self.skip_spaces();
        if self.peek_raw().is_some_and(|tok| tok.kind == Kind::Open) {
            let whole = self.whole_group_ahead();
            self.bump();
            let blocks = if whole {
                self.group(Self::argument_blocks)?
            } else {
                self.group(|parser| parser.blocks(End::Close))?
            };
            self.close_group()?;
            return Ok(blocks);
        }
        let tokens = self.argument_tokens().unwrap_or_default();
        self.blocks_of(tokens)
    }

    /// Skips spaces and one `{...}` group after them, as Pandoc does after
    /// `\noindent` and `\maketitle`.
    fn skip_group(&mut self) {
        self.skip_spaces();
        if self.peek_raw().is_some_and(|tok| tok.kind == Kind::Open) {
            self.bump();
            self.tokens_until_close();
        }
    }

    /// Consumes the `}` of a group. Pandoc stops when the input ends inside
    /// the group.
    fn close_group(&mut self) -> R<()> {
        match self.peek_raw() {
            Some(tok) if tok.kind == Kind::Close => {
                self.bump();
                Ok(())
            }
            None => self.ends_inside("unexpected ()"),
            Some(_) => Ok(()),
        }
    }

    /// The position of the next token, or the end of the input.
    fn position_of_next(&self) -> SourcePos {
        let (line, column) = match self.peek_raw() {
            Some(tok) => (tok.line, tok.column),
            None => self.tokens.last().map_or((1, 1), |tok| {
                let width = u32::try_from(tok.text.chars().count()).unwrap_or(0);
                (tok.line, tok.column + width)
            }),
        };
        SourcePos::new(self.source_name.as_str(), line, column)
    }

    // Blocks.

    fn blocks(&mut self, end: End) -> R<Vec<Block>> {
        let mut blocks = Vec::new();
        self.blocks_into(&mut blocks, end)?;
        Ok(blocks)
    }

    /// The blocks up to `end`. On an error, `blocks` holds the blocks before it.
    fn blocks_into(&mut self, blocks: &mut Vec<Block>, end: End) -> R<()> {
        let mut rounds = Rounds::default();
        while let Some(kind) = self.peek_kind()? {
            self.round(&mut rounds)?;
            if let Kind::Word(name) = &kind {
                self.check_argument(name, 1)?;
            }
            match kind {
                Kind::Par if self.peek_raw().is_some_and(|tok| tok.text == "\\par") => {
                    self.bump();
                    self.star();
                    self.skip_options()?;
                }
                Kind::Par | Kind::Space | Kind::Newline | Kind::Comment => {
                    self.bump();
                }
                Kind::Close if end == End::Close => break,
                Kind::Close if let End::Environment(name) | End::Item(name) = end => {
                    let message = format!("unexpected }}\nexpecting \\end{{{name}}}");
                    return Err(self
                        .peek_raw()
                        .map_or(Stop::Unexpected(Text::new()), |tok| {
                            self.error_at(tok, &message)
                        }));
                }
                Kind::Close if self.depth == self.top_depth => return Err(self.unexpected()),
                Kind::Close => {
                    self.bump();
                }
                Kind::Word(name) if name == "end" => {
                    let ahead = self.environment_name_ahead();
                    if matches!(end, End::Environment(n) | End::Item(n) if Some(n) == ahead.as_deref())
                    {
                        break;
                    }
                    return Err(self.stray_end(end));
                }
                Kind::Word(name) if name == "item" && matches!(end, End::Item(_)) => break,
                Kind::Word(name) if name == "begin" => {
                    let environment = self.environment_name_ahead().unwrap_or_default();
                    if environment == "document"
                        && (matches!(end, End::Item(_))
                            || self.copies == self.cell_copies && self.copies > 0)
                    {
                        return Err(Stop::NestedDocument);
                    }
                    if is_inline_environment(&environment) {
                        self.paragraph(blocks)?;
                    } else {
                        let mut parsed = Vec::new();
                        self.group(|parser| {
                            parser.environment(
                                &environment,
                                &mut parsed,
                                matches!(end, End::Item(_)),
                            )
                        })?;
                        blocks.extend(parsed);
                    }
                }
                Kind::Word(name) if self.block_command(&name, blocks)? => {}
                Kind::Open if self.group_holds_block() => {
                    self.bump();
                    let inner = self.group(|parser| parser.blocks(End::Close))?;
                    self.close_group()?;
                    blocks.extend(inner);
                }
                _ => self.paragraph(blocks)?,
            }
        }
        Ok(())
    }

    /// The parse error of Pandoc for an `\end` that closes no open
    /// environment: inside an environment it names the environment after
    /// the `\end{...}`; at the top level it shows the token.
    fn stray_end(&mut self, end: End) -> Stop {
        match end {
            End::Environment(name) | End::Item(name) => {
                self.bump();
                let expecting = format!("expecting \\end{{{name}}}");
                self.skip_spaces();
                match self.peek_raw() {
                    Some(tok) if tok.kind != Kind::Open => {
                        self.error_at(tok, &format!("unexpected {}\n{expecting}", tok.text))
                    }
                    _ => {
                        self.argument_tokens();
                        let SourcePos { line, column, .. } = self.position_of_next();
                        self.error_at_position(line, column, &expecting)
                    }
                }
            }
            End::Input if self.copies == 0 => self.unexpected(),
            End::Input | End::Close => self.unexpected_word(),
        }
    }

    /// `unexpected \NAME` at the next token.
    fn unexpected_word(&self) -> Stop {
        self.peek_raw()
            .map_or(Stop::Unexpected(Text::new()), |tok| {
                let spelling = self.diagnostic_spelling(tok, self.scan().nth(1));
                self.error_at(tok, &format!("unexpected {spelling}"))
            })
    }

    fn diagnostic_spelling(&self, tok: &Tok, next: Option<&Tok>) -> Text {
        match tok.kind {
            Kind::Par | Kind::Newline if tok.text.starts_with('\n') => "\n".into(),
            Kind::Word(_) | Kind::Par => match next.filter(|next| next.kind == Kind::Space) {
                Some(space) => format!("{}{}", tok.text, space.text),
                None => tok.text.clone(),
            },
            _ => tok.text.clone(),
        }
    }

    /// A paragraph. It consumes one token at least, so that each loop
    /// that calls it ends.
    fn paragraph(&mut self, blocks: &mut Vec<Block>) -> R<()> {
        let before = self.bumps;
        let mut inlines = Inlines::default();
        let read = self.inlines_into(&mut inlines, Until::Paragraph);
        inlines.flush_into(blocks);
        read?;
        if self.bumps == before {
            self.bump();
        }
        Ok(())
    }

    /// Handles a command that makes a block. Returns false for other commands.
    fn block_command(&mut self, name: &str, blocks: &mut Vec<Block>) -> R<bool> {
        if let Some(base) = sectioning_level(name) {
            self.bump();
            self.skip_word_spaces();
            let star = self.star();
            self.skip_options()?;
            let title = self.argument_inlines_untrimmed()?;
            self.heading(
                base + self.level_offset,
                (!star).then_some(base),
                title,
                blocks,
            )?;
            return Ok(true);
        }
        match name {
            "hypertarget" => {
                self.bump();
                self.star();
                let label = self.argument_text()?;
                let body = self.argument_blocks()?;
                blocks.push(Block::Div(Attr::new(label, vec![], vec![]), body));
                Ok(true)
            }
            "title" | "author" | "date" => {
                self.bump();
                self.star();
                self.skip_options()?;
                let tokens = self.argument_tokens().unwrap_or_default();
                let value = if name == "author" {
                    let mut authors = Vec::new();
                    for part in split_on_word(tokens, "and") {
                        authors.push(MetaValue::MetaInlines(self.inlines_of(part)?));
                    }
                    MetaValue::MetaList(authors)
                } else {
                    MetaValue::MetaInlines(self.inlines_of(tokens)?)
                };
                self.append_meta(name, value);
                Ok(true)
            }
            "caption" => {
                self.bump();
                self.star();
                let short = if self.bracket_gap().is_some_and(|(_, line_end)| line_end) {
                    None
                } else {
                    self.bracket_inlines()?
                };
                self.skip_blank();
                let long = self.argument_inlines()?;
                if let Some(float) = self.floats.last_mut() {
                    float.caption = Some((short, long));
                }
                Ok(true)
            }
            "item" | "raggedright" => {
                self.bump();
                self.skip_word_spaces();
                self.star();
                if name == "item" {
                    self.skip_options()?;
                }
                Ok(true)
            }
            "bibliography" | "addbibresource" => {
                self.bump();
                self.star();
                self.skip_options()?;
                let names = self.argument_text()?;
                for name in names
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                {
                    let filename = std::path::Path::new(name).with_extension("bib");
                    let value = MetaValue::MetaInlines(vec![Inline::Str(
                        filename.to_string_lossy().into_owned(),
                    )]);
                    self.meta
                        .0
                        .entry("bibliography".into())
                        .and_modify(|entries| {
                            if let MetaValue::MetaList(entries) = entries {
                                entries.push(value.clone());
                            }
                        })
                        .or_insert_with(|| MetaValue::MetaList(vec![value]));
                }
                Ok(true)
            }
            "parbox" => {
                self.bump();
                self.star();
                self.skip_options()?;
                self.argument_tokens();
                blocks.extend(self.argument_blocks()?);
                Ok(true)
            }
            "include" if !self.raw_tex() => {
                let depth = self.bump().map_or(0, |tok| tok.depth);
                self.star();
                self.skip_options()?;
                self.include(depth)?;
                Ok(true)
            }
            "maketitle" | "bibliographystyle" | "usepackage" | "listoffigures" | "listoftables"
            | "newtheorem" | "theoremstyle" | "addtocounter" | "graphicspath" | "include" => {
                let raw = self.unknown_command_raw()?;
                if self.raw_tex()
                    && (!self.in_preamble || name == "maketitle")
                    && !matches!(name, "newtheorem" | "theoremstyle" | "graphicspath")
                {
                    blocks.push(Block::RawBlock(Format::new("latex"), raw));
                }
                Ok(true)
            }
            "documentclass" => {
                self.unknown_command_raw()?;
                if !self.in_preamble {
                    self.preamble()?;
                }
                Ok(true)
            }
            _ if self.is_active_definition(name) => {
                if let Some(tok) = self.bump() {
                    let pos = SourcePos::new(self.source_name.as_str(), tok.line, tok.column);
                    self.definition(name, pos)?;
                }
                Ok(true)
            }
            _ if is_definition(name) => {
                let raw = self.definition_raw();
                blocks.push(Block::RawBlock(Format::new("latex"), raw));
                Ok(true)
            }
            _ if self.raw_tex() && is_unknown_command(name) && !self.is_active_definition(name) => {
                self.start_record();
                let mut consumed = Vec::new();
                self.unknown_command_tokens(&mut consumed);
                let raw = self.stop_record();
                let alone = matches!(
                    self.peek_raw().map(|tok| &tok.kind),
                    None | Some(Kind::Par | Kind::Newline)
                );
                if alone {
                    let raw = if consumed
                        .iter()
                        .skip(1)
                        .any(|tok| self.expands_raw_token(tok))
                    {
                        tokens_text(&self.expand_tokens(consumed)?)
                    } else {
                        raw
                    };
                    blocks.push(Block::RawBlock(Format::new("latex"), raw));
                    return Ok(true);
                }
                self.unread(consumed);
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn append_meta(&mut self, name: &str, value: MetaValue) {
        use std::collections::btree_map::Entry;
        match self.meta.0.entry(name.into()) {
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
            Entry::Occupied(mut entry) => match entry.get_mut() {
                MetaValue::MetaList(values) => match value {
                    MetaValue::MetaList(more) => values.extend(more),
                    value => values.push(value),
                },
                current => {
                    let previous = std::mem::replace(current, MetaValue::MetaList(Vec::new()));
                    *current = MetaValue::MetaList(vec![previous, value]);
                }
            },
        }
    }

    /// A heading. `numbered` is the LaTeX level of a numbered section
    /// (`-1` for `\part`, `0` for `\chapter`); a label directly after it
    /// gets the section number for `\ref`.
    fn heading(
        &mut self,
        level: i64,
        numbered: Option<i64>,
        title: Vec<Inline>,
        blocks: &mut Vec<Block>,
    ) -> R<()> {
        let mut attr = Attr::default();
        if numbered.is_none() {
            attr.classes.push("unnumbered".into());
        }
        let number = numbered.and_then(|unit| self.count_section(unit));
        self.skip_blank();
        if self.peek_raw().is_some_and(|tok| tok.is_word("label")) {
            self.bump();
            attr.id = self.argument_text()?;
            self.identifiers.register(&attr.id);
            if let Some(number) = number {
                self.section_numbers.insert(attr.id.clone(), number);
            }
        }
        if attr.id.is_empty() && self.extensions.contains(Extension::AutoIdentifiers) {
            attr.id = self
                .identifiers
                .unique(&auto_identifier(&title, self.extensions));
        }
        blocks.push(Block::Header(level, attr, title));
        Ok(())
    }

    /// Counts a numbered section of LaTeX level `unit` and gives its number,
    /// as Pandoc prints it for `\ref`: from the chapter when a chapter was
    /// counted, else from the section (`0.1` for a first `\subsection`).
    /// A part has no number.
    fn count_section(&mut self, unit: i64) -> Option<Text> {
        let unit = usize::try_from(unit).ok()?.min(self.sections.len() - 1);
        self.sections[unit] = self.sections[unit].saturating_add(1);
        self.sections[unit + 1..].fill(0);
        let first = usize::from(self.sections[0] == 0);
        let parts: Vec<Text> = self.sections[first.min(unit)..=unit]
            .iter()
            .map(u64::to_string)
            .collect();
        Some(parts.join("."))
    }

    /// `\input` and `\include` put the tokens of the file in the input.
    fn include(&mut self, depth: u32) -> R<()> {
        let name = self.argument_text()?;
        if self.field.is_some() {
            return Ok(());
        }
        let pos = self.position_of_next();
        let path = if name.contains('.') {
            name.clone()
        } else {
            format!("{name}.tex")
        };
        let limit = usize::try_from(self.context.limits.max_input_size).unwrap_or(usize::MAX);
        let Ok(bytes) = self.context.read_file_limited(&path, limit) else {
            let warning = LogMessage::CouldNotLoadIncludeFile(path, pos);
            if self.cell_copies > 0 {
                self.context.report(warning.clone());
            }
            self.context.report(warning);
            return Ok(());
        };
        let text = String::from_utf8_lossy(&bytes).replace('\r', "");
        let tokens = tokenize(&text);
        self.push_expansion(&path, depth, &tokens, &[])
    }

    /// `\newcommand` and the other macro definitions.
    fn definition(&mut self, command: &str, pos: SourcePos) -> R<()> {
        self.skip_word_spaces();
        if command == "global" {
            // Pandoc accepts only a `\def` or a `\let` form after `\global`.
            return match self.peek_raw().map(|tok| tok.kind.clone()) {
                Some(Kind::Word(word)) if is_def_word(&word) || word == "let" => {
                    let tok = self.bump();
                    let pos = tok.map_or(pos, |tok| {
                        SourcePos::new(self.source_name.as_str(), tok.line, tok.column)
                    });
                    self.global = true;
                    self.definition(&word, pos)
                }
                Some(Kind::Word(word)) if self.is_flag_setter(&word) => {
                    self.global = true;
                    Ok(())
                }
                _ => Err(self.unexpected_word()),
            };
        }
        let global = std::mem::take(&mut self.global) || matches!(command, "gdef" | "xdef");
        let level = self.group_level;
        self.star();
        match command {
            "let" => {
                let name = self.defined_name();
                self.skip_spaces();
                if matches!(self.peek_raw().map(|t| &t.kind), Some(Kind::Other('='))) {
                    self.bump();
                    self.skip_spaces();
                }
                let Some(target) = self.bump() else {
                    return Ok(());
                };
                if self.field.is_some() {
                    return Ok(());
                }
                let definition = match &target.kind {
                    Kind::Word(word) => self.macros.get(word).cloned(),
                    _ => None,
                }
                .unwrap_or(Macro {
                    parameters: 0,
                    default: None,
                    body: vec![target].into(),
                    delimiters: None,
                });
                self.macros.define(name, definition, level, global);
            }
            _ if is_def_word(command) => {
                let name = self.defined_name();
                let mut parameters = 0;
                let mut delimiters = vec![Vec::new()];
                while let Some(tok) = self.peek_raw() {
                    match tok.kind {
                        Kind::Open => break,
                        Kind::Newline | Kind::Par => return Err(self.unexpected_word()),
                        Kind::Param(n) => {
                            parameters = usize::from(n).max(parameters);
                            if self.field.is_none() {
                                delimiters.push(Vec::new());
                            }
                        }
                        _ => {
                            if self.field.is_none()
                                && let Some(part) = delimiters.last_mut()
                            {
                                part.push(tok.clone());
                            }
                        }
                    }
                    self.bump();
                }
                let body = self.argument_tokens().unwrap_or_default();
                if self.field.is_some() {
                    return Ok(());
                }
                // Delimiters of other characters only; others keep the
                // rule of undelimited parameters.
                let delimited = delimiters.iter().flatten().next().is_some()
                    && delimiters
                        .iter()
                        .flatten()
                        .all(|tok| matches!(tok.kind, Kind::Other(_)));
                let delimiters = delimited.then(|| {
                    self.next_delimiter_id += 1;
                    Arc::new(Delimiters {
                        id: self.next_delimiter_id,
                        chunks: delimiters.into_iter().map(Delimiter::new).collect(),
                    })
                });
                self.macros.define(
                    name,
                    Macro {
                        parameters,
                        default: None,
                        body: body.into(),
                        delimiters,
                    },
                    level,
                    global,
                );
            }
            "newenvironment" | "renewenvironment" => {
                let name = self.argument_source_text();
                let (parameters, default) = self.parameter_specification();
                let begin = self.argument_tokens().unwrap_or_default();
                let end = self.argument_tokens().unwrap_or_default();
                if self.field.is_some() {
                    return Ok(());
                }
                if command == "newenvironment" && self.environments.contains_key(&name) {
                    return Ok(());
                }
                self.environments.define(
                    name,
                    EnvironmentMacro {
                        parameters,
                        default,
                        begin,
                        end,
                    },
                    level,
                    global,
                );
            }
            _ => {
                let name = self.defined_name();
                let (parameters, default) = self.parameter_specification();
                let mut body = self.argument_tokens().unwrap_or_default();
                if self.field.is_some() {
                    return Ok(());
                }
                if command == "DeclareMathOperator" {
                    let op = tokens_text(&body);
                    body = tokenize(&format!("\\mathop{{\\mathrm{{{op}}}}}"));
                }
                let exists = self.macros.contains_key(&name);
                if exists && command == "newcommand" {
                    self.context
                        .report(LogMessage::MacroAlreadyDefined(format!("\\{name}"), pos));
                    return Ok(());
                }
                if exists && command == "providecommand" {
                    return Ok(());
                }
                self.macros.define(
                    name,
                    Macro {
                        parameters,
                        default,
                        body: body.into(),
                        delimiters: None,
                    },
                    level,
                    global,
                );
            }
        }
        Ok(())
    }

    fn field_definition_name(&self) -> Option<Text> {
        if self.field.is_some()
            && let Some(Kind::Word(name)) = self.peek_raw().map(|tok| &tok.kind)
            && is_definition(name)
        {
            return Some(name.clone());
        }
        None
    }

    fn consume_field_definition(&mut self, name: &str) -> R<()> {
        if let Some(tok) = self.bump() {
            let pos = SourcePos::new(self.source_name.as_str(), tok.line, tok.column);
            self.definition(name, pos)?;
        }
        Ok(())
    }

    /// The name of the macro in `\newcommand{\name}` or `\def\name`.
    fn defined_name(&mut self) -> Text {
        let tokens = self.argument_tokens().unwrap_or_default();
        tokens
            .iter()
            .find_map(|tok| match &tok.kind {
                Kind::Word(word) => Some(word.clone()),
                Kind::Symbol(c) => Some(c.to_string()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// `[n][default]` of a definition.
    fn parameter_specification(&mut self) -> (usize, Option<Vec<Tok>>) {
        self.skip_spaces();
        let count = self
            .bracket_tokens()
            .and_then(|tokens| parameter_count(tokens_text(&tokens).trim()))
            .unwrap_or(0);
        self.skip_spaces();
        let default = self.bracket_tokens();
        (count, default)
    }

    // Environments.

    /// An environment. For a list, a table and an environment that Pandoc
    /// does not know, a parse error inside does not stop the document, as
    /// in Pandoc: the reader reads the environment again with the next rule
    /// of a chain (a table as an unknown environment, then the source text
    /// as raw TeX, which only `raw_tex` keeps).
    fn environment(&mut self, name: &str, blocks: &mut Vec<Block>, in_item: bool) -> R<()> {
        let raw_tex = self.extensions.contains(Extension::RawTex);
        if raw_tex && !is_known_environment(name) {
            return self.raw_environment(name, blocks);
        }
        if !falls_back(name) {
            return self.known_environment(name, blocks, in_item);
        }
        let start = self.snapshot();
        let failed = start.as_ref().and_then(|s| s.source_position);
        let mut tabular_failed =
            failed.is_some_and(|p| self.failed_tables.contains(&p)) && !plain_scans();
        if !tabular_failed {
            let Some(snapshot) = start.clone() else {
                return self.known_environment(name, blocks, in_item);
            };
            let mut parsed = Vec::new();
            match self.known_environment(name, &mut parsed, in_item) {
                Ok(()) => {
                    blocks.extend(parsed);
                    return Ok(());
                }
                Err(Stop::Unexpected(_)) => self.restore(snapshot),
                Err(other) => return Err(other),
            }
            tabular_failed = is_tabular(name);
            if let Some(position) = failed.filter(|_| tabular_failed) {
                self.failed_tables.insert(position);
            }
        }
        if tabular_failed && !raw_tex {
            // The memory of failed tables keeps nested failed tables from
            // reading their content once for each rule of the chain.
            if let Some(snapshot) = start.filter(|s| s.source_position.is_some()) {
                let mut parsed = Vec::new();
                match self.div_environment(name, &mut parsed) {
                    Ok(()) => {
                        blocks.extend(parsed);
                        return Ok(());
                    }
                    Err(Stop::Unexpected(_)) => self.restore(snapshot),
                    Err(other) => return Err(other),
                }
            }
        }
        let mut raw = Vec::new();
        self.raw_environment(name, &mut raw)?;
        if raw_tex {
            blocks.extend(raw);
        }
        Ok(())
    }

    /// The source text of an environment as a `RawBlock`.
    fn raw_environment(&mut self, name: &str, blocks: &mut Vec<Block>) -> R<()> {
        self.start_record();
        self.bump();
        self.argument_tokens();
        // Pandoc keeps the text after macro expansion.
        let closed = self.math_body_tokens(name);
        let raw = self.stop_record();
        let (_, closed) = closed?;
        if !closed {
            self.unclosed(name)?;
        }
        blocks.push(Block::RawBlock(Format::new("latex"), raw));
        Ok(())
    }

    /// The state that a failed environment must give back, or `None` when
    /// it is too large to copy (a long macro expansion is pending).
    fn snapshot(&self) -> Option<Snapshot> {
        if self.pending.len() > MAX_SNAPSHOT_PENDING {
            return None;
        }
        Some(Snapshot {
            position: self.position,
            pending: self.pending.clone(),
            recorders: self.recorders.iter().map(Text::len).collect(),
            quote_kinds: self.quote_kinds.len(),
            sections: self.sections,
            source_position: self
                .peek_raw()
                .filter(|tok| tok.depth == 0)
                .map(|tok| (tok.line, tok.column)),
        })
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.position = snapshot.position;
        self.pending = snapshot.pending;
        self.expansion_call = None;
        self.profile_valid = 0;
        self.recorders.truncate(snapshot.recorders.len());
        for (record, len) in self.recorders.iter_mut().zip(snapshot.recorders) {
            record.truncate(len);
        }
        self.quote_kinds.truncate(snapshot.quote_kinds);
        self.sections = snapshot.sections;
    }

    /// An environment by the rule of its name.
    fn known_environment(&mut self, name: &str, blocks: &mut Vec<Block>, in_item: bool) -> R<()> {
        self.bump();
        self.argument_tokens();
        match name {
            "document" => {
                blocks.extend(self.blocks(End::Environment(name))?);
                if self.peek_raw().is_none() {
                    self.unclosed(name)?;
                }
                self.finished = true;
                return Ok(());
            }
            "abstract" | "proof" | "sloppypar" | "otherlanguage" | "alltt" => {
                self.semantic_environment(name, blocks)?;
            }
            "itemize" | "enumerate" | "description" => {
                return self.list_environment(name, blocks, in_item);
            }
            "quote" | "quotation" | "verse" => {
                blocks.push(Block::BlockQuote(self.blocks(End::Environment(name))?));
            }
            "verbatim" | "Verbatim" | "lstlisting" | "minted" | "comment" => {
                if let Some(block) = self.verbatim(name) {
                    blocks.push(block);
                }
            }
            "figure" | "figure*" | "subfigure" => blocks.push(self.figure(name)?),
            "tikzpicture" => {
                let (_, closed) = self.environment_body_tokens(name);
                if !closed {
                    self.unclosed(name)?;
                }
                return Ok(());
            }
            "table" => blocks.extend(self.table_float(name)?),
            _ if is_tabular(name) => return self.tabular_environment(name, blocks),
            _ => return self.div_body(name, blocks),
        }
        self.close_environment(name)
    }

    fn semantic_environment(&mut self, name: &str, blocks: &mut Vec<Block>) -> R<()> {
        match name {
            "abstract" => {
                let content = MetaValue::MetaBlocks(self.blocks(End::Environment(name))?);
                if self.cell_copies > 0 {
                    self.append_meta("abstract", content.clone());
                }
                self.append_meta("abstract", content);
            }
            "proof" => blocks.push(self.proof(name)?),
            "sloppypar" => blocks.extend(self.blocks(End::Environment(name))?),
            "otherlanguage" => {
                self.skip_options()?;
                let language = babel_language(&self.argument_text()?);
                let content = self.blocks(End::Environment(name))?;
                if let Some(language) = language {
                    blocks.push(Block::Div(language_attr(language), content));
                } else {
                    blocks.extend(content);
                }
            }
            "alltt" => {
                let mut content = self.blocks(End::Environment(name))?;
                walk_inlines_mut(&mut content, |inline| {
                    let replacement = match inline {
                        Inline::Str(text) => Inline::Code(Attr::default(), std::mem::take(text)),
                        Inline::Space => Inline::RawInline(Format::new("latex"), "\\ ".into()),
                        Inline::SoftBreak => Inline::LineBreak,
                        _ => return,
                    };
                    *inline = replacement;
                });
                blocks.extend(content);
            }
            _ => return Err(self.stuck("has no environment handler")),
        }
        Ok(())
    }

    fn list_environment(&mut self, name: &str, blocks: &mut Vec<Block>, in_item: bool) -> R<()> {
        match self.list(name, in_item) {
            Ok(Block::Div(..)) if self.raw_tex() => {
                return Err(Stop::Unexpected("Expected list items".into()));
            }
            Ok(list) => blocks.push(list),
            Err(Stop::NestedDocument) => {
                let (_, closed) = self.environment_body_tokens(name);
                if !closed {
                    self.unclosed(name)?;
                }
                return Ok(());
            }
            Err(other) => return Err(other),
        }
        self.close_environment(name)
    }

    fn tabular_environment(&mut self, name: &str, blocks: &mut Vec<Block>) -> R<()> {
        if name != "tabular" && name != "longtable" {
            self.argument_tokens();
        }
        if name == "tabular*" {
            self.skip_spaces();
            if !self.peek_raw().is_some_and(|tok| tok.kind == Kind::Open) {
                return Err(Stop::Unexpected(
                    "Expected table column specification".into(),
                ));
            }
        }
        match self.tabular(name) {
            Ok(table) => blocks.push(table),
            Err(Stop::NestedDocument) => {
                let (_, closed) = self.environment_body_tokens(name);
                if !closed {
                    self.unclosed(name)?;
                }
                return Ok(());
            }
            Err(other) => return Err(other),
        }
        self.close_environment(name)
    }

    /// An environment read as a `Div` with its name as the class, from its
    /// `\begin`.
    fn proof(&mut self, name: &str) -> R<Block> {
        let mut heading = Inlines(
            self.bracket_inlines()?
                .unwrap_or_else(|| vec![Inline::Str("Proof".into())]),
        );
        heading.push_str(".");
        let mut content = self.blocks(End::Environment(name))?;
        let mut intro = vec![Inline::Emph(heading.0)];
        if let Some(Block::Para(first)) = content.first_mut() {
            if !first.is_empty() {
                intro.push(Inline::Space);
            }
            intro.append(first);
            *first = intro;
        } else {
            content.insert(0, Block::Para(intro));
        }
        let qed = Inline::Str("\u{a0}◻".into());
        if let Some(Block::Para(last)) = content.last_mut() {
            last.push(qed);
        } else {
            content.push(Block::Para(vec![qed]));
        }
        Ok(Block::Div(
            Attr::new("", vec![name.into()], vec![]),
            content,
        ))
    }

    fn div_environment(&mut self, name: &str, blocks: &mut Vec<Block>) -> R<()> {
        self.bump();
        self.argument_tokens();
        self.div_body(name, blocks)
    }

    fn div_body(&mut self, name: &str, blocks: &mut Vec<Block>) -> R<()> {
        if name == "minipage" {
            let comments = self
                .scan()
                .take_while(|tok| matches!(tok.kind, Kind::Space | Kind::Comment | Kind::Newline))
                .filter(|tok| tok.kind == Kind::Comment)
                .count();
            if comments < 2 {
                if self.bracket_gap().is_some() {
                    self.skip_options()?;
                    self.skip_blank();
                } else {
                    self.skip_spaces();
                }
                if self.peek_raw().is_some_and(|tok| tok.kind == Kind::Open) {
                    self.argument_tokens();
                }
            }
        } else if name != "center" {
            self.skip_options()?;
        }
        let content = self.blocks(End::Environment(name))?;
        blocks.push(Block::Div(
            Attr::new("", vec![name.to_owned()], vec![]),
            content,
        ));
        self.close_environment(name)
    }

    fn close_environment(&mut self, name: &str) -> R<()> {
        if self.peek_raw().is_none() {
            self.unclosed(name)?;
        }
        self.end_environment();
        Ok(())
    }

    /// The error of Pandoc for an environment that the input ends inside.
    /// Pandoc gives it at column 2 of the line after the input. A rule that
    /// reads a copy of tokens, such as a title, has no such error: its
    /// copy ends before the input does.
    fn unclosed(&self, name: &str) -> R<()> {
        self.ends_inside(&format!(
            "unexpected end of input\nexpecting \\end{{{name}}}"
        ))
    }

    /// The error of Pandoc for a construct that the input ends inside.
    fn ends_inside(&self, message: &str) -> R<()> {
        let lines = self.line_offsets.len() - usize::from(self.input.ends_with('\n'));
        let line = u32::try_from(lines + 1).unwrap_or(u32::MAX);
        self.stop_at(line, 2, message)
    }

    /// The parse error at a position of the input. A rule that reads a copy
    /// of tokens has no such error, as in [`Parser::ends_inside`].
    fn stop_at(&self, line: u32, column: u32, message: &str) -> R<()> {
        if self.copies > self.cell_copies {
            return Ok(());
        }
        Err(self.error_at_position(line, column, message))
    }

    /// Pandoc stops when the argument of a text style command is not
    /// there. Oracle matrices `errors/eofarg-*` and `errors/argap-*`: the
    /// command reads its argument after spaces, line ends and blank lines,
    /// and stops only at the end of the input.
    fn require_text_argument(&mut self) -> R<()> {
        self.skip_blank();
        if self.peek_raw().is_none() {
            self.ends_inside("unexpected end of input")?;
        }
        Ok(())
    }

    fn require_link_text(&mut self) -> R<()> {
        self.skip_spaces();
        let paragraph_gap = self
            .peek_raw()
            .is_some_and(|tok| tok.kind == Kind::Par && tok.text.matches('\n').count() > 1);
        self.skip_blank();
        if self.peek_raw().is_none() {
            self.ends_inside(if paragraph_gap {
                "unexpected end of input"
            } else {
                "unexpected ()"
            })?;
        }
        Ok(())
    }

    fn end_environment(&mut self) {
        if self.peek_raw().is_some_and(|tok| tok.is_word("end")) {
            self.bump();
            self.argument_tokens();
        }
    }

    /// A list. A description item without a term, or an option of
    /// `description`, makes a `Div` with the class of the environment, as in
    /// Pandoc. A list without items is a list only when nothing stands
    /// before its end: for `itemize` and `description` not even a space;
    /// `enumerate` permits its option, spaces and a counter command of an
    /// `enum` counter.
    fn list(&mut self, name: &str, in_item: bool) -> R<Block> {
        let mut attributes = ListAttributes {
            start: 1,
            style: ListNumberStyle::DefaultStyle,
            delim: ListNumberDelim::DefaultDelim,
        };
        let opened = self.bumps;
        self.skip_spaces();
        if name == "enumerate"
            && self
                .scan()
                .find(|tok| {
                    !matches!(
                        tok.kind,
                        Kind::Par | Kind::Space | Kind::Newline | Kind::Comment
                    )
                })
                .is_some_and(|tok| tok.kind == Kind::Other('['))
        {
            self.skip_blank();
        }
        let option = self.bracket_tokens();
        let mut valid = true;
        match &option {
            Some(_) if name == "description" => valid = false,
            Some(label) => {
                (attributes.style, attributes.delim) = list_label(&tokens_text(label));
            }
            None => {}
        }
        self.skip_blank();
        let mut bare = self.bumps == opened;
        if name == "enumerate" {
            match self.counter_start()? {
                Counter::Absent => bare = true,
                Counter::Enum(start) => {
                    bare = true;
                    if let Some(start) = start {
                        attributes.start = start;
                    }
                }
                Counter::Other => bare = false,
            }
        }
        let counted = self.bumps;
        let mut items = Vec::new();
        let mut terms = Vec::new();
        let mut lead = Vec::new();
        let mut rounds = Rounds::default();
        loop {
            self.round(&mut rounds)?;
            self.skip_blank();
            let Some(kind) = self.peek_kind()? else {
                break;
            };
            if kind != Kind::Word("item".into()) {
                if kind == Kind::Word("end".into()) {
                    break;
                }
                let before = self.bumps;
                let blocks = self.blocks(End::Item(name))?;
                if items.is_empty() {
                    lead.extend(blocks);
                }
                if self.bumps == before {
                    self.bump();
                }
                continue;
            }
            self.bump();
            self.skip_word_spaces();
            let term = self.bracket_inlines()?;
            if name == "description" && term.is_none() && in_item {
                let mut gap = self.scan().peekable();
                skip_option_gap(&mut gap, usize::MAX);
                if gap.peek().is_some_and(|tok| tok.kind == Kind::Par) {
                    return Err(Stop::Unexpected("Expected description term".into()));
                }
            }
            if name == "description" && term.is_none() {
                valid = false;
            }
            if name != "description" {
                self.skip_options()?;
            }
            let term = if name == "description" {
                term.unwrap_or_default()
            } else {
                Vec::new()
            };
            let content = self.blocks(End::Item(name))?;
            terms.push(term);
            items.push(content);
        }
        if items.is_empty() && bare && self.bumps == counted {
            return Ok(match name {
                "itemize" => Block::BulletList(items),
                "enumerate" => Block::OrderedList(attributes, items),
                _ => Block::DefinitionList(Vec::new()),
            });
        }
        if items.is_empty() || !valid {
            let mut content = lead;
            content.extend(items.into_iter().flatten());
            return Ok(Block::Div(
                Attr::new("", vec![name.to_owned()], vec![]),
                content,
            ));
        }
        Ok(match name {
            "itemize" => Block::BulletList(items),
            "enumerate" => Block::OrderedList(attributes, items),
            _ => Block::DefinitionList(
                terms
                    .into_iter()
                    .zip(items)
                    .map(|(term, content)| (term, vec![content]))
                    .collect(),
            ),
        })
    }

    /// The counter command at the start of an `enumerate`:
    /// `\setcounter{enumN}{VALUE}` gives the start VALUE + 1. Pandoc examines only the first
    /// command of the list, and only a counter name of `enum` and the
    /// letters `i` and `v`, and only a decimal VALUE (64-bit wrap).
    fn counter_start(&mut self) -> R<Counter> {
        if self.peek_kind()? != Some(Kind::Word("setcounter".into()))
            || self.next_is_other_after('*')
        {
            return Ok(Counter::Absent);
        }
        self.bump();
        let mut texts = Vec::new();
        for _ in 0..2 {
            self.skip_spaces();
            if !self.peek_raw().is_some_and(|tok| tok.kind == Kind::Open) {
                return Ok(Counter::Other);
            }
            texts.push(self.argument_text()?);
        }
        let counter_ok = texts[0]
            .strip_prefix("enum")
            .is_some_and(|rest| rest.chars().all(|c| matches!(c, 'i' | 'v')));
        if !counter_ok {
            return Ok(Counter::Other);
        }
        Ok(Counter::Enum(
            decimal_wrapping(texts[1].trim()).map(|value| value.wrapping_add(1)),
        ))
    }

    fn verbatim(&mut self, name: &str) -> Option<Block> {
        let body = match self.peek_raw().map(|tok| tok.kind.clone()) {
            Some(Kind::Verbatim(body)) => {
                self.bump();
                body
            }
            _ => Text::new(),
        };
        if name == "comment" {
            return None;
        }
        let (attr, body) = code_attributes(name, &body);
        let trimmed = body.trim_start_matches([' ', '\t']);
        let body = trimmed.strip_prefix('\n').unwrap_or(body);
        let body = body.strip_suffix('\n').unwrap_or(body);
        Some(Block::CodeBlock(attr, body.to_owned()))
    }

    fn figure(&mut self, name: &str) -> R<Block> {
        let mut placement = self.bracket_tokens().map(|tokens| tokens_text(&tokens));
        if name == "subfigure" {
            self.argument_tokens();
            placement = None;
        }
        self.floats.push(Float::default());
        let content = self.blocks(End::Environment(name));
        let float = self.floats.pop().unwrap_or_default();
        let content = content?;
        let mut attr = Attr {
            id: float.label.unwrap_or_default(),
            ..Attr::default()
        };
        if let Some(placement) = placement {
            attr.attributes.push(("latex-placement".into(), placement));
        }
        let caption = match float.caption {
            Some((short, long)) => Caption {
                short,
                long: vec![Block::Plain(long)],
            },
            None => Caption::default(),
        };
        let mut content = content;
        walk_blocks_mut(&mut content, |block| {
            if let Block::Para(inlines) = block
                && let [Inline::Image(attr, _, target)] = inlines.as_slice()
            {
                *block = Block::Plain(vec![Inline::Image(
                    attr.clone(),
                    Vec::new(),
                    target.clone(),
                )]);
            }
        });
        Ok(Block::Figure(attr, caption, content))
    }

    fn table_float(&mut self, name: &str) -> R<Vec<Block>> {
        self.skip_options()?;
        self.floats.push(Float::default());
        let content = self.blocks(End::Environment(name));
        let float = self.floats.pop().unwrap_or_default();
        let mut content = content?;
        for block in &mut content {
            if let Block::Table(table) = block {
                if let Some((short, long)) = float.caption.clone() {
                    table.caption = Caption {
                        short,
                        long: vec![Block::Plain(long)],
                    };
                }
                if let Some(label) = &float.label {
                    table.attr.id = label.clone();
                }
            }
        }
        Ok(content)
    }

    /// A tabular environment, read in place up to its `\end`. As in Pandoc,
    /// the rows end at `\\` and the cells at `&` also inside a group, but
    /// not inside a nested environment, and each cell is read alone.
    fn tabular(&mut self, name: &str) -> R<Block> {
        self.skip_options()?;
        let spec = self.argument_tokens().unwrap_or_default();
        let aligns = column_alignments(&spec);
        let columns = aligns.len();
        let cell_limit = self.context.limits.max_table_cells;
        let mut rows = Vec::new();
        let mut head_rows = None;
        let mut rounds = Rounds::default();
        loop {
            self.round(&mut rounds)?;
            self.skip_blank();
            let plain_empty = self
                .peek_raw()
                .is_some_and(|tok| tok.kind == Kind::Symbol('\\'));
            let rule = self.skip_rules()?;
            if rule && !rows.is_empty() && head_rows.is_none() {
                head_rows = Some(rows.len());
            }
            if self.peek_raw().is_none() || self.at_end_of(name) {
                break;
            }
            let available = cell_limit.saturating_sub(rows.len().saturating_mul(columns));
            if available < columns.max(1) {
                return Err(Stop::TableLimit);
            }
            let row = self.row_tokens(name, available)?;
            let empty_candidate = row.len() == 1
                && (plain_empty && row[0].is_empty()
                    || row[0]
                        .iter()
                        .find(|tok| {
                            !matches!(
                                tok.kind,
                                Kind::Space | Kind::Newline | Kind::Par | Kind::Comment
                            )
                        })
                        .is_some_and(|tok| tok.is_word("caption")));
            let mut cells = Vec::new();
            for tokens in row {
                cells.push(self.cell(tokens)?);
            }
            if !rule && rows.is_empty() && empty_candidate && cells[0].content.is_empty() {
                continue;
            }
            rows.push(cells);
        }
        if rows.iter().any(|cells| cells.len() > columns) {
            return Err(Stop::Unexpected(Text::from(
                "a row has more cells than the table has columns",
            )));
        }
        let simple = rows
            .iter()
            .flatten()
            .all(|cell| matches!(cell.content.as_slice(), [] | [Block::Para(_)]));
        let mut rows: Vec<Row> = rows
            .into_iter()
            .map(|cells| Row {
                attr: Attr::default(),
                cells: fit_row(cells, columns, simple),
            })
            .collect();
        // Pandoc makes a head only of one row before the first rule.
        let head = match head_rows {
            Some(1) => rows.drain(..1).collect(),
            _ => Vec::new(),
        };
        let col_specs = aligns
            .into_iter()
            .map(|align| ColSpec {
                align,
                width: ColWidth::ColWidthDefault,
            })
            .collect();
        Ok(Block::Table(Box::new(Table {
            col_specs,
            head: TableHead {
                attr: Attr::default(),
                rows: head,
            },
            bodies: vec![TableBody {
                body: rows,
                ..TableBody::default()
            }],
            ..Table::default()
        })))
    }

    /// True when the next tokens are `\end{NAME}`.
    fn at_end_of(&self, name: &str) -> bool {
        self.peek_raw().is_some_and(|tok| tok.is_word("end"))
            && self.environment_name_ahead().as_deref() == Some(name)
    }

    /// The tokens of the cells of one row, up to its `\\` (which it
    /// consumes with a star and an option) or the `\end` of the table.
    /// An environment that has its `\end` is one unit of a cell.
    fn row_tokens(&mut self, name: &str, cell_limit: usize) -> R<Vec<Vec<Tok>>> {
        let mut cells = vec![Vec::new()];
        let mut open: Vec<Text> = Vec::new();
        while let Some(tok) = self.peek_raw() {
            match &tok.kind {
                Kind::Word(word) if word == "begin" && self.begin_is_closed() => {
                    open.push(self.environment_name_ahead().unwrap_or_default());
                }
                Kind::Word(word) if word == "end" => {
                    let ahead = self.environment_name_ahead();
                    if open.is_empty() && ahead.as_deref() == Some(name) {
                        break;
                    }
                    if open.last() == ahead.as_ref() {
                        open.pop();
                    }
                }
                Kind::Tab if open.is_empty() => {
                    if cells.len() == cell_limit {
                        return Err(Stop::TableLimit);
                    }
                    self.bump();
                    cells.push(Vec::new());
                    continue;
                }
                Kind::Symbol('\\') if open.is_empty() => {
                    self.bump();
                    let starred = self.star();
                    self.bracket_tokens();
                    if !starred
                        && cells.len() == 1
                        && cells[0].is_empty()
                        && self.peek_raw().is_some_and(|tok| tok.kind == Kind::Open)
                    {
                        continue;
                    }
                    break;
                }
                _ => {}
            }
            if let (Some(tok), Some(cell)) = (self.bump(), cells.last_mut()) {
                cell.push(tok);
            }
        }
        Ok(cells)
    }

    /// True when the next `\begin` has its `\end` in the input. A token of a
    /// macro expansion counts as closed.
    fn begin_is_closed(&self) -> bool {
        !self.pending.is_empty() || self.closed_begins.contains(&self.raw_position())
    }

    /// One cell. A parse error inside ends the cell: the cell keeps the
    /// blocks before the error, as in Pandoc.
    fn cell(&mut self, tokens: Vec<Tok>) -> R<Cell> {
        self.scope(|parser| parser.cell_in_group(tokens))
    }

    fn cell_in_group(&mut self, tokens: Vec<Tok>) -> R<Cell> {
        self.with_tokens(tokens, |parser| {
            let saved = (parser.cell_copies, parser.top_depth);
            (parser.cell_copies, parser.top_depth) = (parser.copies, parser.depth);
            let cell = parser.cell_content();
            (parser.cell_copies, parser.top_depth) = saved;
            cell
        })
    }

    fn cell_content(&mut self) -> R<Cell> {
        let mut cell = Cell::default();
        self.skip_spaces();
        let first = self.peek_kind()?;
        if first == Some(Kind::Word("label".into())) && !self.raw_tex() {
            self.check_argument("label", 1)?;
            self.bump();
            self.star();
            let label = self.argument_text()?;
            if let Some(float) = self.floats.last_mut() {
                float.label = Some(label);
            }
        } else if first == Some(Kind::Word("multirow".into())) {
            self.bump();
            self.skip_options()?;
            self.argument_tokens();
            self.argument_tokens();
            self.skip_options()?;
            let content = self.argument_tokens().unwrap_or_default();
            self.unread(content);
        } else if first == Some(Kind::Word("tabularnewline".into())) {
            self.bump();
        } else if first == Some(Kind::Word("multicolumn".into())) {
            self.bump();
            let span = self.argument_text()?;
            let spec = self.argument_tokens().unwrap_or_default();
            cell.col_span = span.trim().parse().unwrap_or(1);
            cell.align = column_alignments(&spec)
                .first()
                .copied()
                .unwrap_or(Alignment::AlignDefault);
            let content = self.argument_tokens().unwrap_or_default();
            self.unread(content);
        }
        match self.blocks_into(&mut cell.content, End::Input) {
            Ok(()) | Err(Stop::Unexpected(_)) => Ok(cell),
            Err(other) => Err(other),
        }
    }

    /// Skips the spaces and the rules between rows. Returns true when a
    /// rule was there.
    fn skip_rules(&mut self) -> R<bool> {
        let mut rule = false;
        let mut rounds = Rounds::default();
        loop {
            self.round(&mut rounds)?;
            self.skip_blank();
            let Some(Kind::Word(word)) = self.peek_kind()? else {
                return Ok(rule);
            };
            match word.as_str() {
                "hline" | "toprule" | "midrule" | "bottomrule" => {
                    self.bump();
                    rule = true;
                }
                "cline" | "cmidrule" => {
                    self.bump();
                    return Ok(true);
                }
                _ => return Ok(rule),
            }
        }
    }

    // Inlines.

    fn inlines(&mut self, until: Until) -> R<Inlines> {
        let mut out = Inlines::default();
        let previous = self.restricted_inlines;
        self.restricted_inlines |= matches!(until, Until::Argument | Until::Bracket);
        let result = self.inlines_into(&mut out, until);
        self.restricted_inlines = previous;
        result?;
        Ok(out)
    }

    /// The inlines up to `until`. On an error, `out` holds the inlines before it.
    fn inlines_into(&mut self, out: &mut Inlines, until: Until) -> R<()> {
        let mut rounds = Rounds::default();
        while let Some(kind) = self.peek_kind()? {
            self.round(&mut rounds)?;
            match (until, &kind) {
                (
                    Until::Close | Until::Argument | Until::Quote(_) | Until::Bracket,
                    Kind::Close,
                ) => {
                    break;
                }
                (Until::Bracket, Kind::Other(']')) => break,
                (Until::Paragraph, Kind::Close) if self.depth == self.top_depth => {
                    return Err(self.unexpected());
                }
                (Until::Paragraph, Kind::Close) => break,
                (Until::Quote(QuoteType::DoubleQuote), Kind::Other('\''))
                    if self.next_is_other_after('\'') =>
                {
                    break;
                }
                (Until::Quote(QuoteType::SingleQuote), Kind::Other('\''))
                    if !self.next_is_other_after('\'') =>
                {
                    break;
                }
                (Until::Paragraph | Until::Quote(_), Kind::Par) => break,
                (Until::Argument | Until::Bracket, Kind::Par) => {
                    if let Some(tok) = self.bump() {
                        if tok.text == "\\par" {
                            self.skip_word_spaces();
                            return Err(self.unexpected_word());
                        }
                        if tok.text.bytes().filter(|&byte| byte == b'\n').count() > 1 {
                            self.stop_at(tok.line + 2, 1, "unexpected ()")?;
                        }
                    }
                    continue;
                }
                (Until::Paragraph | Until::Quote(_), Kind::Open) if self.group_holds_block() => {
                    break;
                }
                (Until::Paragraph | Until::Quote(_), Kind::Word(name))
                    if self.starts_block(name) =>
                {
                    break;
                }
                _ => {}
            }
            self.inline(out, until)?;
        }
        Ok(())
    }

    fn next_is_other_after(&self, c: char) -> bool {
        self.ahead()
            .nth(1)
            .is_some_and(|tok| tok.kind == Kind::Other(c))
    }

    fn starts_block(&self, name: &str) -> bool {
        match name {
            "begin" => self
                .environment_name_ahead()
                .is_none_or(|environment| !is_inline_environment(&environment)),
            "end" => true,
            _ => is_block_only(name),
        }
    }

    fn inline(&mut self, out: &mut Inlines, until: Until) -> R<()> {
        if let Some(name) = self.field_definition_name() {
            self.start_record();
            let result = self.consume_field_definition(&name);
            let raw = self.stop_record();
            result?;
            out.push(Inline::RawInline(Format::new("latex"), raw));
            return Ok(());
        }
        if let Some(Tok {
            kind: Kind::Word(name),
            ..
        }) = self.peek_raw()
        {
            if matches!(until, Until::Argument | Until::Bracket) && is_block_only(name) {
                return Err(self.block_in_argument());
            }
            self.check_argument(name, 1)?;
        }
        let Some(tok) = self.bump() else {
            return Ok(());
        };
        match tok.kind {
            Kind::Letters(text) => out.push_str(&text),
            Kind::Space => out.push(Inline::Space),
            Kind::Newline | Kind::Par => out.push(Inline::SoftBreak),
            Kind::Comment | Kind::Close => {}
            Kind::Tilde => out.push_str(NBSP),
            Kind::Open => {
                let inner = if self.whole_group_after(0) {
                    self.group(Self::argument_inlines_untrimmed)?
                } else {
                    self.group(|parser| {
                        parser.inlines(if until == Until::Argument {
                            Until::Argument
                        } else {
                            Until::Close
                        })
                    })?
                    .0
                };
                self.close_group()?;
                if !inner.is_empty() {
                    match self.field {
                        Some(FieldOptions {
                            keep_brace_spans: false,
                        }) => out.append(Inlines(inner)),
                        Some(FieldOptions {
                            keep_brace_spans: true,
                        }) => {
                            let mut attr = Attr::default();
                            if inner.iter().any(|inline| matches!(inline, Inline::Str(text)
                                if text.split([' ', '-']).any(|word| word.starts_with(char::is_lowercase))))
                            {
                                attr.classes.push("nocase".into());
                            }
                            out.push(Inline::Span(attr, inner));
                        }
                        None => out.push(Inline::Span(Attr::default(), inner)),
                    }
                } else if tok.text.starts_with("\\begingroup") {
                    self.star();
                    self.raw_inline_suffix()?;
                }
            }
            Kind::Dollar => self.dollar_math(out)?,
            Kind::Tab => out.push_str("&"),
            Kind::Param(_) if self.field.is_some() && tok.text.len() > 1 => {
                return Err(self.error_at(&tok, "unexpected macro parameter"));
            }
            Kind::Param(_) => out.push_str("#"),
            Kind::Sup => out.push_str("^"),
            Kind::Sub => out.push_str("_"),
            Kind::Other(c) => self.punctuation(c, out, until)?,
            Kind::Symbol('!' | ':' | ';') => {
                self.unknown_inline(&tok.text, out)?;
            }
            Kind::Symbol(c) => self.symbol(c, out)?,
            Kind::Verb(text) => out.push(Inline::Code(Attr::default(), text)),
            Kind::InlineCode(language, text) => out.push(Inline::Code(
                Attr::new("", language.into_iter().collect(), vec![]),
                text,
            )),
            Kind::Verbatim(text) => out.push(Inline::Code(Attr::default(), text)),
            Kind::Word(ref name) if name == "end" => {
                return Err(self.error_at(&tok, "unexpected \\end"));
            }
            Kind::Word(ref name)
                if name == "begin"
                    && matches!(until, Until::Close | Until::Argument | Until::Bracket)
                    && self
                        .environment_name_ahead_of_argument()
                        .is_none_or(|environment| !is_inline_environment(&environment)) =>
            {
                return Err(self.error_at(&tok, "unexpected \\begin"));
            }
            Kind::Word(name) if self.is_active_definition(&name) => {
                self.definition(
                    &name,
                    SourcePos::new(self.source_name.as_str(), tok.line, tok.column),
                )?;
            }
            Kind::Word(name) => {
                self.nested(|parser| parser.command(&name, &tok.text, tok.depth, out, until))?
            }
        }
        Ok(())
    }

    fn block_in_argument(&mut self) -> Stop {
        self.bump();
        self.skip_word_spaces();
        if self.peek_raw().is_some_and(|tok| tok.kind == Kind::Open) {
            let mut depth = self.depth;
            for tok in self.scan() {
                match tok.kind {
                    Kind::Open => {
                        depth += 1;
                        if depth > self.max_depth {
                            return Stop::TooDeep;
                        }
                    }
                    Kind::Close => {
                        depth -= 1;
                        if depth == self.depth {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        self.unexpected_word()
    }

    fn punctuation(&mut self, c: char, out: &mut Inlines, until: Until) -> R<()> {
        if self.code_text > 0 {
            out.push_str(&c.to_string());
            return Ok(());
        }
        let next_is = |parser: &Self, wanted: char| {
            parser
                .peek_raw()
                .is_some_and(|tok| tok.kind == Kind::Other(wanted))
        };
        match c {
            '-' if next_is(self, '-') => {
                self.bump();
                if next_is(self, '-') {
                    self.bump();
                    out.push_str("\u{2014}");
                } else {
                    out.push_str("\u{2013}");
                }
            }
            '`' if self.extensions.contains(Extension::Smart) => {
                let kind = if next_is(self, '`') {
                    self.bump();
                    QuoteType::DoubleQuote
                } else {
                    QuoteType::SingleQuote
                };
                let inner = self.nested(|parser| parser.inlines(Until::Quote(kind)))?;
                if next_is(self, '\'') {
                    self.bump();
                    if kind == QuoteType::DoubleQuote {
                        self.bump();
                    }
                    out.push(Inline::Quoted(kind, inner.0));
                } else {
                    out.push_str(if kind == QuoteType::DoubleQuote {
                        "\u{201c}"
                    } else {
                        "\u{2018}"
                    });
                    out.append(inner);
                }
            }
            '\'' if next_is(self, '\'') && !matches!(until, Until::Quote(_)) => {
                self.bump();
                out.push_str("\u{201d}");
            }
            '\'' => out.push_str("\u{2019}"),
            c => out.push_str(&c.to_string()),
        }
        Ok(())
    }

    fn symbol(&mut self, c: char, out: &mut Inlines) -> R<()> {
        match c {
            '%' | '&' | '$' | '#' | '_' | '{' | '}' => out.push_str(&c.to_string()),
            ',' => out.push_str("\u{2006}"),
            '-' => out.push_str("\u{ad}"),
            ' ' | '\n' | '\t' => {
                out.push_str(NBSP);
                while self
                    .peek_raw()
                    .is_some_and(|tok| matches!(tok.kind, Kind::Space | Kind::Newline))
                {
                    self.bump();
                }
            }
            '\\' => {
                self.skip_spaces();
                if self
                    .peek_raw()
                    .is_some_and(|tok| tok.kind == Kind::Par && tok.text.matches('\n').count() > 1)
                {
                    self.bump();
                    self.skip_spaces();
                } else {
                    self.bracket_tokens();
                }
                out.push(Inline::LineBreak);
            }
            '(' => {
                let text = self.math_until(|kind| *kind == Kind::Symbol(')'))?;
                out.push(Inline::Math(MathType::InlineMath, text));
            }
            '[' => {
                let text = self.math_until(|kind| *kind == Kind::Symbol(']'))?;
                out.push(Inline::Math(MathType::DisplayMath, text));
            }
            '\'' | '`' | '^' | '"' | '~' | '=' | '.' => self.accent(c, out)?,
            _ => {}
        }
        Ok(())
    }

    fn dollar_math(&mut self, out: &mut Inlines) -> R<()> {
        if self.peek_raw().is_some_and(|tok| tok.kind == Kind::Dollar) {
            self.bump();
            let text = self.math_until(|kind| *kind == Kind::Dollar)?;
            if self.peek_raw().is_some_and(|tok| tok.kind == Kind::Dollar) {
                self.bump();
            } else if self.field.is_some() {
                return Err(self.unexpected());
            }
            out.push(Inline::Math(MathType::DisplayMath, text));
        } else {
            let text = self.math_until(|kind| *kind == Kind::Dollar)?;
            out.push(Inline::Math(MathType::InlineMath, text));
        }
        Ok(())
    }

    /// The source text up to a closing token, which it consumes.
    fn math_until(&mut self, closes: impl Fn(&Kind) -> bool) -> R<Text> {
        let mut text = Text::new();
        let mut rounds = Rounds::default();
        loop {
            self.round(&mut rounds)?;
            self.expand()?;
            let Some(tok) = self.bump() else {
                if self.field.is_some() {
                    return Err(self.unexpected());
                }
                break;
            };
            if closes(&tok.kind) {
                break;
            }
            text.push_str(&tok.text);
        }
        Ok(text.trim().to_owned())
    }

    fn accent(&mut self, accent: char, out: &mut Inlines) -> R<()> {
        let combining = match accent {
            '\'' => '\u{301}',
            '`' => '\u{300}',
            '^' => '\u{302}',
            '"' => '\u{308}',
            '~' => '\u{303}',
            '=' => '\u{304}',
            '.' => '\u{307}',
            'c' => '\u{327}',
            'v' => '\u{30c}',
            'u' => '\u{306}',
            'H' => '\u{30b}',
            'r' => '\u{30a}',
            'k' => '\u{328}',
            'd' => '\u{323}',
            't' => '\u{361}',
            _ => '\u{331}',
        };
        self.skip_spaces();
        self.expand()?;
        let grouped = self.peek_raw().is_some_and(|tok| tok.kind == Kind::Open);
        let tokens = self.argument_tokens().unwrap_or_default();
        let tokens = self.expand_tokens(tokens)?;
        let ends = match_groups(&tokens);
        let mut from = 0;
        let mut to = tokens.len();
        while to > from && ends.get(&from) == Some(&(to - 1)) {
            from += 1;
            to -= 1;
        }
        let tokens: Vec<_> = tokens.into_iter().skip(from).take(to - from).collect();
        let markup = |tok: &Tok| {
            matches!(
                tok.kind,
                Kind::Word(_) | Kind::Symbol(_) | Kind::Open | Kind::Tilde
            )
        };
        if grouped && tokens.len() > 1 && tokens.iter().any(markup) {
            // Oracle probes: the argument is read as inlines, and the accent
            // goes on the first character of a leading `Str` only.
            let mut inlines = self.inlines_of(tokens)?;
            if let Some(Inline::Str(text)) = inlines.first_mut() {
                let mut chars = text.chars();
                if let Some(base) = chars.next() {
                    *text = format!("{}{}", nfc(&format!("{base}{combining}")), chars.as_str());
                }
            }
            out.append(Inlines(inlines));
            return Ok(());
        }
        let text = match tokens.as_slice() {
            [
                Tok {
                    kind: Kind::Word(word),
                    ..
                },
            ] => {
                if !grouped {
                    self.skip_word_spaces();
                }
                letter_symbol(word).map_or_else(|| tokens_text(&tokens), Text::from)
            }
            _ => tokens_text(&tokens),
        };
        let mut chars = text.chars();
        let Some(base) = chars
            .next()
            .filter(|_| !(grouped && text.trim().is_empty()))
        else {
            // Pandoc gives the character of a symbol accent, and the
            // combining mark of the other accents.
            if grouped || self.field.is_some() {
                match accent {
                    '~' | '\'' | '`' | '^' => out.push_str(&accent.to_string()),
                    _ => out.push_str(&combining.to_string()),
                }
            }
            return Ok(());
        };
        out.push_str(&nfc(&format!("{base}{combining}")));
        out.push_str(chars.as_str());
        Ok(())
    }

    /// The tokens after macro expansion, read in a copy. Tokens without a
    /// defined macro stay as they are.
    fn expand_tokens(&mut self, tokens: Vec<Tok>) -> R<Vec<Tok>> {
        let defined = |tok: &Tok| match &tok.kind {
            Kind::Word(name) => self.macros.contains_key(name),
            Kind::Symbol(c) => self.macros.contains_key(&c.to_string()),
            _ => false,
        };
        if !tokens.iter().any(defined) {
            return Ok(tokens);
        }
        self.with_tokens(tokens, |parser| {
            let mut out = Vec::new();
            let mut rounds = Rounds::default();
            while parser.peek_kind()?.is_some() {
                parser.round(&mut rounds)?;
                if let Some(tok) = parser.bump() {
                    out.push(tok);
                }
            }
            Ok(out)
        })
    }

    /// The text of a verbatim argument after macro expansion, as Pandoc
    /// reads a URL.
    fn expand_text(&mut self, text: Text) -> R<Text> {
        if !text.contains('\\') {
            return Ok(text);
        }
        let tokens = tokenize(&text);
        let expanded = self.expand_tokens(tokens.clone())?;
        if expanded.len() == tokens.len()
            && expanded.iter().zip(&tokens).all(|(a, b)| a.text == b.text)
        {
            return Ok(text);
        }
        Ok(tokens_text(&expanded))
    }

    fn start_record(&mut self) {
        self.recorders.push(Text::new());
    }

    fn stop_record(&mut self) -> Text {
        self.recorders.pop().unwrap_or_default()
    }

    /// Consumes an unknown command after its name was peeked: the name, a
    /// star, and each option and argument group. Returns the source text.
    fn unknown_command_raw(&mut self) -> R<Text> {
        self.start_record();
        self.unknown_command_tokens(&mut Vec::new());
        Ok(self.stop_record())
    }

    /// Consumes an unknown command and keeps its tokens in `consumed`.
    fn unknown_command_tokens(&mut self, consumed: &mut Vec<Tok>) {
        let mut take = |parser: &mut Self| {
            if let Some(tok) = parser.bump() {
                consumed.push(tok);
            }
        };
        take(self);
        while matches!(self.peek_raw().map(|tok| &tok.kind), Some(Kind::Space)) {
            take(self);
        }
        if matches!(self.peek_raw().map(|tok| &tok.kind), Some(Kind::Other('*'))) {
            take(self);
        }
        loop {
            let close = match self.peek_raw().map(|tok| &tok.kind) {
                Some(Kind::Other('[')) => Kind::Other(']'),
                Some(Kind::Open) => Kind::Close,
                _ => break,
            };
            let mut nesting = 0usize;
            while let Some(tok) = self.peek_raw() {
                let kind = tok.kind.clone();
                take(self);
                match kind {
                    Kind::Open => nesting += 1,
                    Kind::Close => nesting = nesting.saturating_sub(1),
                    _ => {}
                }
                if kind == close && nesting == 0 {
                    break;
                }
            }
        }
    }

    /// A macro definition as raw TeX, when `latex_macros` is off.
    fn definition_raw(&mut self) -> Text {
        self.start_record();
        let is_def = self
            .peek_raw()
            .is_some_and(|tok| matches!(&tok.kind, Kind::Word(word) if is_def_word(word)));
        if is_def {
            self.bump();
            while let Some(tok) = self.peek_raw() {
                if tok.kind == Kind::Open {
                    break;
                }
                self.bump();
            }
            self.bump();
            self.tokens_until_close();
        } else {
            self.unknown_command_tokens(&mut Vec::new());
        }
        self.stop_record()
    }

    /// The preamble after `\documentclass`: definitions and metadata count,
    /// other content does not.
    fn preamble(&mut self) -> R<()> {
        let outer = std::mem::replace(&mut self.in_preamble, true);
        let preamble = self.preamble_items();
        self.in_preamble = outer;
        preamble
    }

    fn preamble_items(&mut self) -> R<()> {
        let mut rounds = Rounds::default();
        loop {
            self.round(&mut rounds)?;
            let Some(kind) = self.peek_kind()? else {
                return Ok(());
            };
            if kind == Kind::Word("begin".into())
                && self.environment_name_ahead().as_deref() == Some("document")
            {
                return Ok(());
            }
            let mut ignored = Vec::new();
            match kind {
                Kind::Word(name) if self.block_command(&name, &mut ignored)? => {}
                _ => {
                    self.nested(|parser| parser.inline(&mut Inlines::default(), Until::Paragraph))?;
                }
            }
        }
    }

    /// Wraps the rest of the group in an element, for a declaration such as `\bf`.
    fn declaration(
        &mut self,
        until: Until,
        out: &mut Inlines,
        wrap: impl FnOnce(Vec<Inline>) -> Inline,
    ) -> R<()> {
        self.skip_word_spaces();
        let rest = self.inlines(until)?;
        push_extracting_spaces(out, rest.0, wrap);
        Ok(())
    }

    fn command(
        &mut self,
        name: &str,
        source: &str,
        depth: u32,
        out: &mut Inlines,
        until: Until,
    ) -> R<()> {
        let star_is_a_mode = citation_mode(name).is_some()
            || multi_citation_mode(name).is_some()
            || matches!(name, "enquote" | "foreignquote");
        if !is_unknown_command(name) && !star_is_a_mode && name != "verb" && self.star() {
            self.skip_word_spaces();
        }
        let wrapper: Option<fn(Vec<Inline>) -> Inline> = match name {
            "emph" | "textit" | "textsl" => Some(Inline::Emph),
            "textbf" => Some(Inline::Strong),
            "underline" | "uline" => Some(Inline::Underline),
            "sout" => Some(Inline::Strikeout),
            "textsc" => Some(Inline::SmallCaps),
            "textsuperscript" => Some(Inline::Superscript),
            "textsubscript" => Some(Inline::Subscript),
            "textsf" => Some(|content| {
                Inline::Span(Attr::new("", vec!["sans-serif".into()], vec![]), content)
            }),
            "textrm" => {
                Some(|content| Inline::Span(Attr::new("", vec!["roman".into()], vec![]), content))
            }
            _ => None,
        };
        if let Some(wrap) = wrapper {
            self.require_text_argument()?;
            let content = self.argument_inlines_untrimmed()?;
            if matches!(name, "underline" | "uline") {
                out.push(wrap(content));
            } else {
                push_extracting_spaces(out, content, wrap);
            }
            return Ok(());
        }
        let declaration: Option<fn(Vec<Inline>) -> Inline> = match name {
            "em" | "it" | "itshape" | "sl" | "slshape" => Some(Inline::Emph),
            "bf" | "bfseries" => Some(Inline::Strong),
            "scshape" => Some(Inline::SmallCaps),
            _ => None,
        };
        if let Some(wrap) = declaration {
            return self.declaration(until, out, wrap);
        }
        match name {
            name if is_size_or_fill(name) => self.text_command(name, source, out, until),
            "tt" | "ttfamily" | "textup" | "textmd" | "textnormal" | "vbox" | "texttt" | "mbox"
            | "hbox" | "LaTeX" | "TeX" | "ldots" | "dots" | "newline" | "NeedsTeXFormat" => {
                self.text_command(name, source, out, until)
            }
            "url" | "nolinkurl" | "hyperlink" | "hypertarget" | "hyperref" | "href" | "label"
            | "ref" | "eqref" | "autoref" | "cref" | "Cref" | "includegraphics" => {
                self.reference_command(name, source, out)
            }
            "footnote" | "thanks" => {
                self.skip_options()?;
                let content = self.argument_blocks()?;
                out.push(Inline::Note(content));
                Ok(())
            }
            "enquote" | "foreignquote" => self.quoted_argument(name, out),
            "textcolor" | "colorbox" | "gls" | "Gls" | "glspl" | "ac" | "MakeUppercase"
            | "uppercase" | "MakeLowercase" | "lowercase" | "SI" | "qty" | "si" | "num" | "ang"
            | "ensuremath" => self.semantic_command(name, out),
            _ => self.other_inline_command(name, source, depth, out),
        }
    }

    fn text_command(&mut self, name: &str, source: &str, out: &mut Inlines, until: Until) -> R<()> {
        match name {
            name if is_size_or_fill(name) => {
                if self.raw_tex() && name != "rm" {
                    self.start_record();
                    self.skip_word_spaces();
                    let gap = self.stop_record();
                    out.push(Inline::RawInline(
                        Format::new("latex"),
                        format!("{source}{gap}"),
                    ));
                } else {
                    self.skip_word_spaces();
                }
            }
            "tt" | "ttfamily" => {
                self.skip_word_spaces();
                let rest = self.inlines(until)?;
                let content = if name == "ttfamily" {
                    rest.trimmed()
                } else {
                    rest.0
                };
                out.append(code_inlines(content));
            }
            "textup" | "textmd" | "textnormal" => {
                let class = match name {
                    "textup" => "upright",
                    "textmd" => "medium",
                    _ => "nodecor",
                };
                let content = self.argument_inlines_untrimmed()?;
                out.push(Inline::Span(
                    Attr::new("", vec![class.into()], vec![]),
                    content,
                ));
            }
            "vbox" if self.raw_tex() => self.unknown_inline(source, out)?,
            "vbox" => {
                self.skip_word_spaces();
                if self.peek_raw().is_some_and(|tok| tok.kind == Kind::Open) {
                    out.append(Inlines(self.argument_inlines_untrimmed()?));
                }
            }
            "texttt" => {
                self.require_text_argument()?;
                self.code_text += 1;
                let content = self.argument_inlines_untrimmed();
                self.code_text -= 1;
                out.append(code_inlines(content?));
            }
            "mbox" | "hbox" if self.raw_tex() => self.unknown_inline(source, out)?,
            "mbox" | "hbox" => {
                self.require_text_argument()?;
                let mut content = self.argument_inlines_untrimmed()?;
                walk_inlines_mut(&mut content, |inline| match inline {
                    Inline::Space | Inline::SoftBreak => *inline = Inline::Str(NBSP.into()),
                    Inline::LineBreak => *inline = Inline::Str(Text::new()),
                    _ => {}
                });
                out.append(Inlines(content));
            }
            "LaTeX" => {
                self.skip_word_spaces();
                out.push_str("LaTeX");
            }
            "TeX" => {
                self.skip_word_spaces();
                out.push_str("TeX");
            }
            "ldots" | "dots" => {
                self.skip_word_spaces();
                out.push_str("\u{2026}");
            }
            "newline" => {
                self.skip_word_spaces();
                out.push(Inline::LineBreak);
            }
            "NeedsTeXFormat" => {
                let argument = self.argument_source_text();
                if self.raw_tex() {
                    out.push(Inline::RawInline(
                        Format::new("latex"),
                        format!("{source}{{{argument}}}"),
                    ));
                }
            }
            _ => return Err(self.stuck("has no text command handler")),
        }
        Ok(())
    }

    fn reference_command(&mut self, name: &str, source: &str, out: &mut Inlines) -> R<()> {
        match name {
            "url" => {
                let url = self.argument_text()?;
                let url = unescape(&self.expand_text(url)?);
                out.push(Inline::Link(
                    Attr::new("", vec!["uri".into()], vec![]),
                    vec![Inline::Str(url.clone())],
                    Target::new(url, ""),
                ));
            }
            "nolinkurl" => {
                let url = unescape(&self.argument_text()?);
                out.push(Inline::Code(Attr::default(), url));
            }
            "hyperlink" | "hypertarget" | "hyperref" => {
                let target = if name == "hyperref" {
                    if let Some(label) = self.bracket_tokens() {
                        format!("#{}", tokens_text(&label))
                    } else {
                        let url = self.argument_text()?;
                        self.argument_tokens();
                        self.argument_tokens();
                        url
                    }
                } else {
                    let label = self.argument_text()?;
                    if name == "hyperlink" {
                        format!("#{label}")
                    } else {
                        label
                    }
                };
                let text = self.argument_inlines_untrimmed()?;
                if name == "hypertarget" {
                    out.push(Inline::Span(Attr::new(target, vec![], vec![]), text));
                } else {
                    out.push(Inline::Link(Attr::default(), text, Target::new(target, "")));
                }
            }
            "href" => {
                let url = self.argument_text()?;
                let url = unescape(&self.expand_text(url)?);
                self.require_link_text()?;
                let text = self.argument_inlines_untrimmed()?;
                out.push(Inline::Link(Attr::default(), text, Target::new(url, "")));
            }
            "label"
                if self.raw_tex()
                    && (self.floats.is_empty()
                        || !self
                            .scan()
                            .find(|tok| tok.kind != Kind::Space)
                            .is_some_and(|tok| tok.kind == Kind::Open)) =>
            {
                self.unknown_inline(source, out)?
            }
            "label" => {
                let label = self.argument_text()?;
                if let Some(float) = self.floats.last_mut() {
                    float.label = Some(label);
                } else if self.extensions.contains(Extension::RawTex) {
                    out.push(Inline::RawInline(
                        Format::new("latex"),
                        format!("{source}{{{label}}}"),
                    ));
                } else {
                    out.push(Inline::Span(
                        Attr::new(label.clone(), vec![], vec![("label".into(), label)]),
                        vec![],
                    ));
                }
            }
            "ref" | "eqref" | "autoref" | "cref" | "Cref" if self.raw_tex() => {
                self.unknown_inline(source, out)?;
            }
            "ref" | "eqref" | "autoref" | "cref" | "Cref" => {
                let label = self.argument_text()?;
                let kind = match name {
                    "autoref" | "cref" => "ref+label",
                    "Cref" => "ref+Label",
                    other => other,
                };
                out.push(Inline::Link(
                    Attr::new(
                        "",
                        vec![],
                        vec![
                            ("reference-type".into(), kind.to_owned()),
                            ("reference".into(), label.clone()),
                        ],
                    ),
                    vec![Inline::Str(format!("[{label}]"))],
                    Target::new(format!("#{label}"), ""),
                ));
            }
            "includegraphics" => {
                self.star();
                let options = self.bracket_tokens().map(|tokens| tokens_text(&tokens));
                let path = self.argument_text()?;
                let mut attr = Attr::default();
                for option in options.iter().flat_map(|text| text.split(',')) {
                    if let Some((key, value)) = option.split_once('=') {
                        let key = key.trim();
                        if key == "width" || key == "height" {
                            attr.attributes
                                .push((key.to_owned(), dimension(value.trim())));
                        }
                    }
                }
                let alt = vec![Inline::Str("image".into())];
                out.push(Inline::Image(attr, alt, Target::new(path, "")));
            }
            _ => return Err(self.stuck("has no reference command handler")),
        }
        Ok(())
    }

    fn quoted_argument(&mut self, name: &str, out: &mut Inlines) -> R<()> {
        let starred = self.star();
        let language = if name == "foreignquote" {
            babel_language(&self.argument_text()?)
        } else {
            None
        };
        let single = starred != (self.quote_kinds.last() == Some(&QuoteType::DoubleQuote));
        if name == "enquote" {
            self.skip_options()?;
        }
        let kind = if single {
            QuoteType::SingleQuote
        } else {
            QuoteType::DoubleQuote
        };
        self.require_link_text()?;
        self.quote_kinds.push(kind);
        let content = self.argument_inlines_untrimmed();
        self.quote_kinds.pop();
        let mut content = content?;
        if let Some(language) = language {
            content = vec![Inline::Span(language_attr(language), content)];
        }
        out.push(Inline::Quoted(kind, content));
        Ok(())
    }

    fn semantic_command(&mut self, name: &str, out: &mut Inlines) -> R<()> {
        match name {
            "textcolor" | "colorbox" => {
                self.skip_options()?;
                let value = self.argument_text()?;
                let property = if name == "textcolor" {
                    "color"
                } else {
                    "background-color"
                };
                let attr = Attr::new(
                    "",
                    vec![],
                    vec![("style".into(), format!("{property}: {value}"))],
                );
                out.push(Inline::Span(attr, self.argument_inlines_untrimmed()?));
            }
            "gls" | "Gls" | "glspl" | "ac" => {
                let label = self.argument_text()?;
                let (form, display) = if name == "glspl" {
                    ("plural+short", format!("{label}s"))
                } else {
                    ("singular+short", label.clone())
                };
                let attr = Attr::new(
                    "",
                    vec![],
                    vec![
                        ("acronym-label".into(), label),
                        ("acronym-form".into(), form.into()),
                    ],
                );
                out.push(Inline::Span(attr, vec![Inline::Str(display)]));
            }
            "MakeUppercase" | "uppercase" | "MakeLowercase" | "lowercase" => {
                self.skip_options()?;
                let mut content = self.argument_inlines_untrimmed()?;
                let upper = matches!(name, "MakeUppercase" | "uppercase");
                walk_inlines_mut(&mut content, |inline| {
                    if let Inline::Str(text) = inline {
                        *text = if upper {
                            text.to_uppercase()
                        } else {
                            text.to_lowercase()
                        };
                    }
                });
                out.append(Inlines(content));
            }
            "SI" | "qty" | "si" | "num" | "ang" => {
                self.skip_options()?;
                if name == "ang" {
                    let angle = self.argument_text()?;
                    for (part, suffix) in angle.split(';').zip(["°", "′", "″"]) {
                        out.push_str(&part.trim().replace('-', "−"));
                        out.push_str(suffix);
                    }
                } else {
                    if name != "si" {
                        let value = self.argument_text()?;
                        append_measurement_number(out, value.trim());
                    }
                    if name != "num" {
                        if name != "si" {
                            out.push_str(NBSP);
                        }
                        let unit = self.argument_tokens().unwrap_or_default();
                        append_measurement_unit(out, &self.expand_tokens(unit)?);
                    }
                }
            }
            "ensuremath" => {
                let text = self.argument_text()?;
                out.push(Inline::Math(MathType::InlineMath, text.trim().into()));
            }
            _ => return Err(self.stuck("has no semantic command handler")),
        }
        Ok(())
    }

    fn other_inline_command(
        &mut self,
        name: &str,
        source: &str,
        depth: u32,
        out: &mut Inlines,
    ) -> R<()> {
        match name {
            "centering" | "noindent" if !self.raw_tex() => {
                self.star();
                self.skip_options()?;
                self.skip_group();
            }
            "footnotemark" => {
                self.skip_options()?;
                out.push(Inline::Str(Text::new()));
            }
            "footnotetext" => {
                self.skip_options()?;
                self.argument_blocks()?;
            }
            "c" | "v" | "u" | "H" | "r" | "k" | "d" | "b" | "t" => {
                let accent = name.chars().next().unwrap_or('b');
                self.accent(accent, out)?;
            }
            "verb" => match self.peek_raw() {
                Some(Tok {
                    kind: Kind::Verb(_),
                    ..
                }) => {}
                Some(tok)
                    if matches!(tok.kind, Kind::Newline | Kind::Par)
                        && tok.text.starts_with('\n') =>
                {
                    return Err(self.error_at(tok, "unexpected \n"));
                }
                Some(tok) => {
                    let line = tok.line;
                    self.stop_at(line + 1, 1, "unexpected ()")?;
                }
                None => self.ends_inside("unexpected ()")?,
            },
            "lstinline" | "mintinline" => {}
            "hskip" | "vskip" | "kern" => self.skip_dimension(name, source, out)?,
            "include" if self.field.is_some() => self.include(depth)?,
            "today" if self.field.is_some() => self.unknown_inline(source, out)?,
            "input" if self.raw_tex() => self.unknown_inline(source, out)?,
            "input" => {
                self.skip_options()?;
                self.include(depth)?;
            }
            "today" => {
                self.skip_word_spaces();
                let fixed = self
                    .context
                    .host()
                    .environment_variable("SOURCE_DATE_EPOCH")
                    .and_then(|text| text.trim().parse::<i64>().ok());
                let seconds = fixed.unwrap_or_else(|| {
                    let time = self.context.host().current_time();
                    time.unix_seconds + i64::from(time.utc_offset_minutes) * 60
                });
                out.push_str(&iso_date(seconds));
            }
            "nocite" => {
                let mut cite = Inlines::default();
                self.citation(source, CitationMode::NormalCitation, false, &mut cite)?;
                let mut inlines = match self.meta.0.remove("nocite") {
                    Some(MetaValue::MetaInlines(inlines)) => Inlines(inlines),
                    _ => Inlines::default(),
                };
                inlines.append(cite);
                self.meta
                    .insert(Text::from("nocite"), MetaValue::MetaInlines(inlines.0));
            }
            "texorpdfstring" => {
                let content = self.argument_inlines()?;
                self.argument_tokens();
                out.append(Inlines(content));
            }
            "char" if self.raw_tex() => {
                self.start_record();
                self.char_code();
                let rest = self.stop_record();
                out.push(Inline::RawInline(
                    Format::new("latex"),
                    format!("{source}{rest}"),
                ));
            }
            "char" => {
                self.skip_options()?;
                self.char_code();
            }
            "footcite" => {
                let mut cite = Inlines::default();
                self.citation(source, CitationMode::NormalCitation, true, &mut cite)?;
                cite.push_str(".");
                out.push(Inline::Note(vec![Block::Para(cite.0)]));
            }
            "begin" => {
                let environment = self.argument_text()?;
                if is_inline_environment(&environment) {
                    self.group(|parser| parser.inline_environment(&environment, out))?;
                } else {
                    self.block_environment_in_inlines(source, &environment, out)?;
                }
            }
            _ => {
                if let Some(text) = letter_symbol(name) {
                    self.skip_word_spaces();
                    out.push_str(text);
                } else if let Some(mode) = citation_mode(name) {
                    let starred = self
                        .peek_raw()
                        .is_some_and(|tok| tok.kind == Kind::Other('*'));
                    let star = citation_star(name);
                    let mode = if starred && star == Star::SuppressAuthor {
                        CitationMode::SuppressAuthor
                    } else {
                        mode
                    };
                    self.citation(source, mode, star == Star::NotInRaw, out)?;
                } else if let Some(first_mode) = multi_citation_mode(name) {
                    self.multi_citation(source, name, first_mode, out)?;
                } else {
                    self.unknown_inline(source, out)?;
                }
            }
        }
        Ok(())
    }

    /// `\char65` drops the command and its decimal code, as Pandoc does.
    /// A code with letters after it (`\char65x`) stays text.
    fn skip_dimension(&mut self, name: &str, source: &str, out: &mut Inlines) -> R<()> {
        self.start_record();
        let result = (|| {
            if name == "kern" {
                self.skip_options()?;
            }
            self.tex_length()?;
            if name != "kern" {
                for keyword in ["plus", "minus"] {
                    let next = self.scan().find(|tok| tok.kind != Kind::Space);
                    if next.is_some_and(|tok| tok.text == keyword) {
                        self.skip_word_spaces();
                        self.bump();
                        self.tex_length()?;
                    }
                }
            }
            Ok(())
        })();
        let captured = self.stop_record();
        result?;
        if self.raw_tex() {
            out.push(Inline::RawInline(
                Format::new("latex"),
                format!("{source}{captured}"),
            ));
        }
        Ok(())
    }

    fn tex_length(&mut self) -> R<()> {
        self.skip_spaces();
        if self
            .peek_raw()
            .is_some_and(|tok| matches!(tok.kind, Kind::Other('+' | '-')))
        {
            self.bump();
        }
        let digits = |tok: &Tok| {
            matches!(tok.kind, Kind::Letters(_)) && tok.text.bytes().all(|b| b.is_ascii_digit())
        };
        let integer = self.peek_raw().is_some_and(digits);
        if integer {
            self.bump();
        }
        let decimal = self
            .peek_raw()
            .is_some_and(|tok| tok.kind == Kind::Other('.'));
        if decimal {
            self.bump();
        }
        if (integer || decimal) && self.peek_raw().is_some_and(digits) {
            self.bump();
        }
        self.skip_word_spaces();
        let Some(tok) = self.peek_raw() else {
            return self.ends_inside("unexpected ()");
        };
        let unit = tok.text.trim_start_matches(|c: char| c.is_ascii_digit());
        if (!integer && unit == tok.text)
            || !matches!(
                unit,
                "pt" | "pc" | "in" | "bp" | "cm" | "mm" | "dd" | "cc" | "sp" | "em" | "ex" | "mu"
            )
        {
            return Err(self.unexpected_word());
        }
        self.bump();
        Ok(())
    }

    fn char_code(&mut self) {
        let Some(Tok {
            kind: Kind::Letters(text),
            ..
        }) = self.peek_raw()
        else {
            return;
        };
        if text.chars().all(|c| c.is_ascii_digit()) {
            self.bump();
        }
    }

    fn is_active_definition(&self, name: &str) -> bool {
        self.extensions.contains(Extension::LatexMacros) && is_definition(name)
    }

    /// The parse error of Pandoc for the next token.
    fn unexpected(&self) -> Stop {
        let Some(tok) = self.peek_raw() else {
            return Stop::Unexpected(Text::new());
        };
        let shown = match &tok.kind {
            Kind::Word(name) => {
                let spelling = self.diagnostic_spelling(tok, self.scan().nth(1));
                format!(
                    "(CtrlSeq \"{name}\") {}",
                    randoc_ast::json::show_string(&spelling)
                )
            }
            _ => format!("Symbol \"{}\"", tok.text),
        };
        let (line, column) = (tok.line, tok.column);
        let source = if self.source_name.is_empty() {
            Text::new()
        } else {
            format!("\"{}\" ", self.source_name)
        };
        let message = format!(
            "unexpected Tok {source}(line {line}, column {column}) {shown}\nexpecting end of input"
        );
        // Pandoc names no file and shows no source line for an error at the
        // first token of the input.
        if (line, column) == (1, 1) && self.pending.is_empty() {
            return Stop::Unexpected(format!(
                "Error at \"source\" (line 1, column 1):\n{message}"
            ));
        }
        self.error_at(tok, &message)
    }

    /// A parse error at `tok`, with the source line and a caret, as Pandoc
    /// prints it.
    fn error_at(&self, tok: &Tok, message: &str) -> Stop {
        self.error_at_position(tok.line, tok.column, message)
    }

    fn error_at_position(&self, line: u32, column: u32, message: &str) -> Stop {
        let index = usize::try_from(line).unwrap_or(1).saturating_sub(1);
        let source_line = self.line_offsets.get(index).map_or("", |&start| {
            let end = self
                .line_offsets
                .get(index + 1)
                .copied()
                .unwrap_or(self.input.len());
            self.input[start..end].trim_end_matches(['\r', '\n'])
        });
        let caret = " ".repeat(usize::try_from(column).unwrap_or(1).saturating_sub(1));
        let source = if self.source_name.is_empty() {
            Text::new()
        } else {
            format!("\"{}\" ", self.source_name)
        };
        Stop::Unexpected(format!(
            "Error at {source}(line {line}, column {column}):\n{message}\n{source_line}\n{caret}^"
        ))
    }

    fn macro_not_found(&self, call: &Tok, paragraph: bool) -> Stop {
        let name = match &call.kind {
            Kind::Word(name) => name.as_str(),
            _ => call.text.as_str(),
        };
        let prefix = if paragraph { "() or " } else { "" };
        let (line, column) = (call.line, call.column);
        let spelling = randoc_ast::json::show_string(&call.text);
        self.error_at(call, &format!(
            "unexpected {prefix}Tok (line {line}, column {column}) (CtrlSeq \"{name}\") {spelling}\nexpecting end of input\nMacro not found"
        ))
    }

    /// Pandoc stops with a parse error when a command that needs an
    /// argument group, or a definition that needs a name, has another
    /// token there. `skip` is 1 when the next token is the command itself.
    /// An argument that a macro or a conditional can give is not checked.
    /// The end of the input, or a paragraph break, is `unexpected ()`.
    fn check_argument(&self, name: &str, skip: usize) -> R<()> {
        if name == "label" && self.raw_tex() {
            return Ok(());
        }
        let Some(rule) = argument_rule(name) else {
            return Ok(());
        };
        if self.in_preamble && rule == ArgumentRule::Group {
            return Ok(());
        }
        let mut tokens = self.scan().skip(skip).peekable();
        let verbatim = takes_verbatim_argument(name);
        let blank = |tok: &&Tok| {
            matches!(tok.kind, Kind::Space)
                || (!verbatim && matches!(tok.kind, Kind::Newline | Kind::Comment))
        };
        if !matches!(name, "begin" | "let")
            && !is_def_word(name)
            && tokens
                .peek()
                .is_some_and(|tok| tok.kind == Kind::Other('*'))
        {
            tokens.next();
        }
        // Pandoc reads an option after one line end only, also when a
        // comment line follows it (matrix `errors/optcomment-*`).
        if !verbatim && rule == ArgumentRule::Group && takes_options(name) {
            skip_option_gap(&mut tokens, 1);
        } else {
            while tokens.next_if(blank).is_some() {}
        }
        if multi_citation_mode(name).is_some() {
            for _ in 0..2 {
                if tokens.next_if(|tok| tok.kind == Kind::Other('(')).is_none() {
                    break;
                }
                if !tokens
                    .by_ref()
                    .take(MAX_OPTION_TOKENS)
                    .any(|tok| tok.kind == Kind::Other(')'))
                {
                    return Ok(());
                }
                while tokens.next_if(blank).is_some() {}
            }
        }
        let mut options = 0;
        if rule == ArgumentRule::Group && takes_options(name) {
            if name == "hyperref"
                && tokens
                    .peek()
                    .is_some_and(|tok| tok.kind == Kind::Other('['))
            {
                return Ok(());
            }
            while let Some(open) = tokens.next_if(|tok| tok.kind == Kind::Other('[')) {
                options += 1;
                if name == "includegraphics" && options > 1 {
                    return Err(self.error_at(open, "unexpected ["));
                }
                let mut nesting = 0usize;
                let closed = tokens
                    .by_ref()
                    .take(MAX_OPTION_TOKENS)
                    .any(|tok| match tok.kind {
                        Kind::Open => {
                            nesting += 1;
                            false
                        }
                        Kind::Close => {
                            nesting = nesting.saturating_sub(1);
                            false
                        }
                        Kind::Other(']') => nesting == 0,
                        _ => false,
                    });
                if !closed {
                    return Ok(());
                }
                if verbatim {
                    skip_option_gap(&mut tokens, 1);
                } else {
                    while tokens.next_if(blank).is_some() {}
                }
            }
        }
        let Some(mut tok) = tokens.next() else {
            if let Some(call) = &self.expansion_call {
                return Err(self.macro_not_found(call, false));
            }
            return self.ends_inside("unexpected ()");
        };
        if tok.kind == Kind::Par && (!verbatim || (name == "includegraphics" && options > 0)) {
            if !tok.text.contains("\n\n") && tokens.next().is_none() {
                return self.ends_inside("unexpected ()");
            }
            if let Some(call) = &self.expansion_call {
                return Err(self.macro_not_found(call, true));
            }
            // Pandoc stops at the line after the first blank line.
            return self.stop_at(tok.line + 2, 1, "unexpected ()");
        }
        let is_name = |tok: &Tok| matches!(tok.kind, Kind::Word(_) | Kind::Symbol(_));
        match rule {
            ArgumentRule::Group if tok.kind == Kind::Open => return Ok(()),
            ArgumentRule::Name if is_name(tok) => return Ok(()),
            ArgumentRule::NameOrGroup if is_name(tok) => return Ok(()),
            ArgumentRule::NameOrGroup if tok.kind == Kind::Open => {
                while tokens.next_if(blank).is_some() {}
                let Some(inner) = tokens.next() else {
                    return Ok(());
                };
                tok = inner;
                // A name with `@` is one name after `\makeatletter`.
                if tok.kind == Kind::Symbol('@') {
                    return Ok(());
                }
                if is_name(tok) {
                    while tokens.next_if(blank).is_some() {}
                    let Some(after) = tokens.next() else {
                        return Ok(());
                    };
                    if matches!(after.kind, Kind::Close | Kind::Other('@')) {
                        return Ok(());
                    }
                    tok = after;
                }
            }
            _ => {}
        }
        if self.expands(tok) || (matches!(tok.kind, Kind::Par) && !verbatim) {
            return Ok(());
        }
        if let Some(call) = &self.expansion_call {
            return Err(self.macro_not_found(call, false));
        }
        let mut shown = match tok.kind {
            Kind::Newline | Kind::Par => Text::from("\n"),
            Kind::Comment => tok.text.trim_end_matches('\n').into(),
            _ => tok.text.clone(),
        };
        if matches!(tok.kind, Kind::Word(_))
            && let Some(space) = tokens.next().filter(|next| next.kind == Kind::Space)
        {
            shown.push_str(&space.text);
        }
        Err(self.error_at(tok, &format!("unexpected {shown}")))
    }

    /// True when `tok` is a macro or a conditional that the parser expands.
    fn expands(&self, tok: &Tok) -> bool {
        let Kind::Word(word) = &tok.kind else {
            return false;
        };
        let macros = self.extensions.contains(Extension::LatexMacros);
        self.condition_value(word, macros).is_some()
            || (macros && (self.macros.contains_key(word) || word == "newif"))
    }

    fn expands_raw_token(&self, tok: &Tok) -> bool {
        match &tok.kind {
            Kind::Symbol(c) => self.macros.contains_key(&c.to_string()),
            _ => self.expands(tok) || matches!(&tok.kind, Kind::Word(name) if is_stream_noop(name)),
        }
    }

    fn raw_tex(&self) -> bool {
        self.extensions.contains(Extension::RawTex)
    }

    fn unknown_inline(&mut self, source: &str, out: &mut Inlines) -> R<()> {
        self.start_record();
        if source
            .as_bytes()
            .get(1)
            .is_some_and(u8::is_ascii_alphabetic)
        {
            self.skip_word_spaces();
        }
        self.star();
        let result = self.raw_inline_suffix();
        let rest = self.stop_record();
        result?;
        if self.raw_tex() {
            out.push(Inline::RawInline(
                Format::new("latex"),
                format!("{source}{rest}"),
            ));
        }
        Ok(())
    }

    fn raw_inline_suffix(&mut self) -> R<()> {
        let mut rounds = Rounds::default();
        let initial_gap = if self.raw_tex() {
            self.bracket_gap().map_or(0, |(gap, _)| gap)
        } else {
            0
        };
        if initial_gap > 0 && self.closed_raw_option(initial_gap) {
            for _ in 0..initial_gap {
                self.bump();
            }
        }
        loop {
            self.round(&mut rounds)?;
            match self.peek_raw().map(|tok| &tok.kind) {
                Some(Kind::Other('[')) => {
                    if self.raw_tex() && !self.closed_raw_option(0) {
                        break;
                    }
                    self.bracket_tokens();
                }
                Some(Kind::Open) => {
                    self.bump();
                    let recorded = self.recorders.iter().map(Text::len).collect::<Vec<_>>();
                    let (body, closed) = self.tokens_until_close_or_end();
                    if !closed {
                        self.ends_inside("unexpected end of input")?;
                    }
                    if self.raw_tex() && body.iter().any(|tok| self.expands_raw_token(tok)) {
                        let body = self.expand_tokens(body)?;
                        let body = tokens_text(&body);
                        for (recorder, length) in self.recorders.iter_mut().zip(recorded) {
                            recorder.truncate(length);
                            recorder.push_str(&body);
                            if closed {
                                recorder.push('}');
                            }
                        }
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn closed_raw_option(&self, offset: usize) -> bool {
        let mut depth = 0usize;
        self.scan()
            .skip(offset + 1)
            .take(MAX_OPTION_TOKENS)
            .take_while(|tok| !matches!(tok.kind, Kind::Newline | Kind::Par))
            .any(|tok| match tok.kind {
                Kind::Open => {
                    depth += 1;
                    false
                }
                Kind::Close => {
                    depth = depth.saturating_sub(1);
                    false
                }
                Kind::Other(']') => depth == 0,
                _ => false,
            })
    }

    /// A block environment inside inline content: its paragraphs become inlines.
    fn block_environment_in_inlines(
        &mut self,
        source: &str,
        name: &str,
        out: &mut Inlines,
    ) -> R<()> {
        let mut begin = vec![self.synthetic(Kind::Word("begin".into()), source)];
        begin.push(self.synthetic(Kind::Open, "{"));
        begin.extend(tokenize(name));
        begin.push(self.synthetic(Kind::Close, "}"));
        self.pending.extend(begin.into_iter().rev());
        let mut blocks = Vec::new();
        self.group(|parser| parser.environment(name, &mut blocks, false))?;
        for block in blocks {
            if let Block::Para(inlines) | Block::Plain(inlines) = block {
                out.push(Inline::Space);
                out.append(Inlines(inlines));
            }
        }
        Ok(())
    }

    fn inline_environment(&mut self, name: &str, out: &mut Inlines) -> R<()> {
        let (body, closed) = self.math_body_tokens(name)?;
        if !closed {
            self.unclosed(name)?;
        }
        let text = tokens_text(&body);
        // Pandoc keeps the leading spaces of a body, except in `displaymath`.
        let text = if name == "displaymath" {
            text.trim()
        } else {
            text.trim_start_matches('\n').trim_end()
        };
        let math = match name {
            "math" => Inline::Math(MathType::InlineMath, text.to_owned()),
            "displaymath" | "subequations" => Inline::Math(MathType::DisplayMath, text.to_owned()),
            _ => Inline::Math(
                MathType::DisplayMath,
                format!("\\begin{{{name}}}\n{text}\n\\end{{{name}}}"),
            ),
        };
        out.push(math);
        Ok(())
    }

    /// A citation command. With `star_not_in_raw`, a star is not part of
    /// the raw text, as in Pandoc for `\textcite*`.
    fn citation(
        &mut self,
        source: &str,
        mode: CitationMode,
        star_not_in_raw: bool,
        out: &mut Inlines,
    ) -> R<()> {
        self.skip_word_spaces();
        self.start_record();
        let recorders = std::mem::take(&mut self.recorders);
        let starred = self
            .peek_raw()
            .is_some_and(|tok| tok.kind == Kind::Other('*'));
        if starred && !star_not_in_raw {
            self.recorders = recorders;
            self.star();
            let recorders = std::mem::take(&mut self.recorders);
            self.skip_word_spaces();
            self.recorders = recorders;
        } else {
            self.star();
            self.skip_word_spaces();
            self.recorders = recorders;
        }
        let first = self.bracket_inlines();
        let second = match first {
            Ok(Some(_)) => self.bracket_inlines(),
            _ => Ok(None),
        };
        let keys = self.argument_text()?;
        let raw = format!("{source}{}", self.stop_record());
        let citations = key_group(mode, (first?, second?), &keys);
        out.push(Inline::Cite(
            citations,
            vec![Inline::RawInline(Format::new("latex"), raw)],
        ));
        Ok(())
    }

    /// `\cites(pre)(post)[p][s]{a}[p][s]{b}` and the other biblatex
    /// commands with several key groups. One `(...)` is the global suffix.
    fn multi_citation(
        &mut self,
        source: &str,
        name: &str,
        first_mode: CitationMode,
        out: &mut Inlines,
    ) -> R<()> {
        self.skip_word_spaces();
        if name == "cites" {
            self.star();
        }
        self.start_record();
        let first = self.paren_inlines()?;
        let second = match first {
            Some(_) => self.paren_inlines()?,
            None => None,
        };
        let (global_prefix, global_suffix) = match (first, second) {
            (Some(prefix), Some(suffix)) => (Some(prefix), Some(suffix)),
            (Some(suffix), None) => (None, Some(suffix)),
            _ => (None, None),
        };
        let mut citations: Vec<Citation> = Vec::new();
        let mut rounds = Rounds::default();
        while self.key_group_ahead() {
            self.round(&mut rounds)?;
            self.skip_spaces();
            let options = self.bracket_inlines()?;
            self.skip_spaces();
            let options = match options {
                Some(first) => (Some(first), self.bracket_inlines()?),
                None => (None, None),
            };
            let keys = self.argument_text()?;
            citations.extend(key_group(CitationMode::NormalCitation, options, &keys));
        }
        if let Some(first) = citations.first_mut() {
            first.mode = first_mode;
        }
        let raw = format!("{source}{}", self.stop_record());
        if let (Some(prefix), Some(first)) = (global_prefix, citations.first_mut()) {
            let mut joined = prefix;
            if !first.prefix.is_empty() {
                joined.push(Inline::Space);
            }
            joined.append(&mut first.prefix);
            first.prefix = joined;
        }
        if let (Some(suffix), Some(last)) = (global_suffix, citations.last_mut()) {
            last.suffix.push(Inline::Str(",".into()));
            last.suffix.push(Inline::Space);
            last.suffix.extend(suffix);
        }
        let cite = Inline::Cite(
            citations,
            vec![Inline::RawInline(Format::new("latex"), raw)],
        );
        if name == "footcites" {
            out.push(Inline::Note(vec![Block::Para(vec![
                cite,
                Inline::Str(".".into()),
            ])]));
        } else {
            out.push(cite);
        }
        Ok(())
    }

    /// The inlines of a `(...)` argument of `\cites`.
    fn paren_inlines(&mut self) -> R<Option<Vec<Inline>>> {
        if !self
            .peek_raw()
            .is_some_and(|tok| tok.kind == Kind::Other('('))
        {
            return Ok(None);
        }
        self.bump();
        let mut tokens = Vec::new();
        while let Some(tok) = self.bump() {
            if tok.kind == Kind::Other(')') {
                break;
            }
            tokens.push(tok);
        }
        let recorders = std::mem::take(&mut self.recorders);
        let inlines = self.inlines_of(tokens);
        self.recorders = recorders;
        Ok(Some(inlines?))
    }

    /// True when spaces, at most two `[...]` and a `{` come next. An option
    /// longer than `MAX_OPTION_TOKENS` ends the command, so that a run of
    /// unclosed options is linear.
    fn key_group_ahead(&self) -> bool {
        let mut tokens = self
            .scan()
            .skip_while(|tok| matches!(tok.kind, Kind::Space | Kind::Newline | Kind::Comment));
        for _ in 0..3 {
            match tokens.next().map(|tok| &tok.kind) {
                Some(Kind::Open) => return true,
                Some(Kind::Other('[')) => {
                    let mut nesting = 0usize;
                    let closed =
                        tokens
                            .by_ref()
                            .take(MAX_OPTION_TOKENS)
                            .any(|tok| match tok.kind {
                                Kind::Open => {
                                    nesting += 1;
                                    false
                                }
                                Kind::Close => {
                                    nesting = nesting.saturating_sub(1);
                                    false
                                }
                                Kind::Other(']') => nesting == 0,
                                _ => false,
                            });
                    if !closed {
                        return false;
                    }
                    let mut rest = tokens.clone().peekable();
                    while rest
                        .peek()
                        .is_some_and(|tok| matches!(tok.kind, Kind::Space | Kind::Newline))
                    {
                        rest.next();
                        tokens.next();
                    }
                }
                _ => return false,
            }
        }
        false
    }
}

/// The citations of one key group: `[prefix][suffix]{keys}`. One optional
/// argument is the suffix.
fn key_group(
    mode: CitationMode,
    options: (Option<Vec<Inline>>, Option<Vec<Inline>>),
    keys: &str,
) -> Vec<Citation> {
    let (prefix, suffix) = match options {
        (Some(prefix), Some(suffix)) => (prefix, suffix),
        (Some(suffix), None) => (Vec::new(), suffix),
        _ => (Vec::new(), Vec::new()),
    };
    let keys: Vec<Text> = keys
        .split(',')
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .collect();
    let count = keys.len();
    keys.into_iter()
        .enumerate()
        .map(|(index, id)| Citation {
            id,
            prefix: if index == 0 {
                prefix.clone()
            } else {
                Vec::new()
            },
            suffix: if index + 1 == count {
                suffix.clone()
            } else {
                Vec::new()
            },
            mode: if index > 0 && mode == CitationMode::AuthorInText {
                CitationMode::NormalCitation
            } else {
                mode
            },
            note_num: 0,
            hash: 0,
        })
        .collect()
}

/// Moves the spaces at the ends of `content` out of the element, as the
/// `extractSpaces` rule of Pandoc does for `\emph{ x }`.
fn push_extracting_spaces(
    out: &mut Inlines,
    mut content: Vec<Inline>,
    wrap: impl FnOnce(Vec<Inline>) -> Inline,
) {
    let is_space = |inline: &Inline| matches!(inline, Inline::Space | Inline::SoftBreak);
    let lead = content.first().filter(|inline| is_space(inline)).cloned();
    if lead.is_some() {
        content.remove(0);
    }
    let trail = if content.last().is_some_and(is_space) {
        content.pop()
    } else {
        None
    };
    if let Some(space) = lead {
        out.push(space);
    }
    out.push(wrap(content));
    if let Some(space) = trail {
        out.push(space);
    }
}

/// The `\else` and the `\fi` of each word that starts with `if`, in one
/// pass with a stack.
/// The position of the `}` of each `{` of the input, in one pass.
fn match_groups(tokens: &[Tok]) -> HashMap<usize, usize> {
    let mut open = Vec::new();
    let mut ends = HashMap::new();
    for (index, tok) in tokens.iter().enumerate() {
        match tok.kind {
            Kind::Open => open.push(index),
            Kind::Close => {
                if let Some(start) = open.pop() {
                    ends.insert(start, index);
                }
            }
            _ => {}
        }
    }
    ends
}

/// The positions of the `\begin{NAME}` tokens that have their `\end{NAME}`.
fn match_environments(tokens: &[Tok]) -> HashSet<usize> {
    let mut open: HashMap<Text, Vec<usize>> = HashMap::new();
    let mut closed = HashSet::new();
    for (index, tok) in tokens.iter().enumerate() {
        let begins = tok.is_word("begin");
        if !begins && !tok.is_word("end") {
            continue;
        }
        let Some(name) = name_in(tokens[index + 1..].iter().take(64)) else {
            continue;
        };
        let stack = open.entry(name).or_default();
        if begins {
            stack.push(index);
        } else if let Some(begin) = stack.pop() {
            closed.insert(begin);
        }
    }
    closed
}

/// The cells of a row with the column count of the table: Pandoc drops
/// the cells after the last column and adds empty cells to a short row.
/// In a table where each cell holds one paragraph at most, the paragraphs
/// are plain.
fn fit_row(cells: Vec<Cell>, columns: usize, simple: bool) -> Vec<Cell> {
    let mut used = 0usize;
    let mut fitted = Vec::with_capacity(columns);
    for mut cell in cells {
        if used >= columns {
            break;
        }
        let span = usize::try_from(cell.col_span.max(1))
            .unwrap_or(1)
            .min(columns - used);
        cell.col_span = i64::try_from(span).unwrap_or(1);
        used += span;
        if simple && let [Block::Para(inlines)] = cell.content.as_mut_slice() {
            cell.content = vec![Block::Plain(std::mem::take(inlines))];
        }
        fitted.push(cell);
    }
    fitted.resize_with(fitted.len() + columns.saturating_sub(used), Cell::default);
    fitted
}

/// The tokens of a macro body with its parameters replaced by arguments.
fn substitute(body: &[Tok], arguments: &[Vec<Tok>], limit: usize) -> Option<(Vec<Tok>, bool)> {
    let mut tokens = body.iter().peekable();
    let mut out = Vec::new();
    while let Some(tok) = tokens.next() {
        if tok.kind == Kind::Param(0)
            && tok.text == "#"
            && let Some(next) = tokens.next_if(|tok| matches!(tok.kind, Kind::Param(_)))
        {
            if out.len() == limit {
                return None;
            }
            out.push(next.clone());
        } else if let Kind::Param(n @ 1..) = tok.kind {
            let Some(argument) = arguments.get(usize::from(n) - 1) else {
                return Some((out, true));
            };
            if argument.len() > limit - out.len() {
                return None;
            }
            out.extend(argument.iter().cloned());
        } else {
            if out.len() == limit {
                return None;
            }
            out.push(tok.clone());
        }
    }
    Some((out, false))
}

fn match_conditionals(tokens: &[Tok]) -> HashMap<usize, (Option<usize>, usize)> {
    let mut open: Vec<(usize, Option<usize>)> = Vec::new();
    let mut matches = HashMap::new();
    for (index, tok) in tokens.iter().enumerate() {
        let Kind::Word(word) = &tok.kind else {
            continue;
        };
        match word.as_str() {
            "fi" => {
                if let Some((start, else_at)) = open.pop() {
                    matches.insert(start, (else_at, index));
                }
            }
            "else" => {
                if let Some((_, else_at @ None)) = open.last_mut() {
                    *else_at = Some(index);
                }
            }
            _ if word.starts_with("if") => open.push((index, None)),
            _ => {}
        }
    }
    matches
}

fn name_in<'a>(mut tokens: impl Iterator<Item = &'a Tok>) -> Option<Text> {
    let mut first = tokens.next()?;
    while first.kind == Kind::Space {
        first = tokens.next()?;
    }
    if first.kind != Kind::Open {
        return None;
    }
    let mut name = Text::new();
    for tok in tokens {
        if tok.kind == Kind::Close {
            return Some(name);
        }
        name.push_str(&tok.text);
    }
    None
}

fn tokens_text(tokens: &[Tok]) -> Text {
    tokens.iter().map(|tok| tok.text.as_str()).collect()
}

fn unescape(text: &str) -> Text {
    let mut out = Text::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A dimension of `\includegraphics`: a fraction of the text width is a percentage.
fn dimension(value: &str) -> Text {
    let plain_number = |number: &str| {
        let (whole, fraction) = number.split_once('.').unwrap_or((number, "0"));
        !whole.is_empty()
            && !fraction.is_empty()
            && whole
                .chars()
                .chain(fraction.chars())
                .all(|c| c.is_ascii_digit())
    };
    for unit in ["\\textwidth", "\\linewidth"] {
        if let Some(fraction) = value
            .strip_suffix(unit)
            .map(str::trim)
            .filter(|number| plain_number(number))
            .and_then(|number| number.parse::<f64>().ok())
        {
            let percent = fraction * 100.0;
            return format!("{}%", trim_number(percent));
        }
    }
    value.to_owned()
}

/// The date `YYYY-MM-DD` of a time in seconds since 1970.
fn iso_date(seconds: i64) -> Text {
    let days = seconds.div_euclid(86_400);
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn trim_number(value: f64) -> Text {
    let text = format!("{value:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.to_owned()
}

fn split_on_word(tokens: Vec<Tok>, word: &str) -> Vec<Vec<Tok>> {
    let mut parts = vec![Vec::new()];
    for tok in tokens {
        if tok.is_word(word) {
            parts.push(Vec::new());
        } else if let Some(part) = parts.last_mut() {
            part.push(tok);
        }
    }
    parts
}

fn column_alignments(spec: &[Tok]) -> Vec<Alignment> {
    let mut aligns = Vec::new();
    let mut iter = spec.iter();
    while let Some(tok) = iter.next() {
        let letters: Vec<char> = match &tok.kind {
            Kind::Letters(text) => text.chars().collect(),
            Kind::Other('@' | '>' | '<' | '!') => {
                skip_group(&mut iter);
                continue;
            }
            _ => continue,
        };
        for letter in letters {
            match letter {
                'l' => aligns.push(Alignment::AlignLeft),
                'c' => aligns.push(Alignment::AlignCenter),
                'r' => aligns.push(Alignment::AlignRight),
                'p' | 'm' | 'b' | 'X' => {
                    aligns.push(Alignment::AlignLeft);
                    if letter != 'X' {
                        skip_group(&mut iter);
                    }
                }
                _ => {}
            }
        }
    }
    aligns
}

fn skip_group<'a>(iter: &mut impl Iterator<Item = &'a Tok>) {
    let mut nesting = 0usize;
    for tok in iter {
        match tok.kind {
            Kind::Open => nesting += 1,
            Kind::Close => {
                nesting = nesting.saturating_sub(1);
                if nesting == 0 {
                    return;
                }
            }
            _ if nesting == 0 => return,
            _ => {}
        }
    }
}

/// `\texttt`: text becomes code, also inside each container; a quotation
/// becomes its quotation marks in the code text.
fn code_inlines(content: Vec<Inline>) -> Inlines {
    let mut out = Inlines::default();
    let mut code = Text::new();
    code_into(content, &mut code, &mut out);
    flush_code(&mut code, &mut out);
    out
}

fn flush_code(code: &mut Text, out: &mut Inlines) {
    if !code.is_empty() {
        out.push(Inline::Code(Attr::default(), std::mem::take(code)));
    }
}

fn code_into(content: Vec<Inline>, code: &mut Text, out: &mut Inlines) {
    let container = |code: &mut Text, out: &mut Inlines, inline: Inline| {
        flush_code(code, out);
        out.push(inline);
    };
    for inline in content {
        match inline {
            Inline::Str(text) => code.push_str(&text),
            Inline::Space | Inline::SoftBreak => code.push(' '),
            Inline::Quoted(kind, inner) => {
                let (open, close) = match kind {
                    QuoteType::DoubleQuote => ('\u{201c}', '\u{201d}'),
                    QuoteType::SingleQuote => ('\u{2018}', '\u{2019}'),
                };
                code.push(open);
                code_into(inner, code, out);
                code.push(close);
            }
            Inline::Emph(inner) => container(code, out, Inline::Emph(code_inlines(inner).0)),
            Inline::Strong(inner) => container(code, out, Inline::Strong(code_inlines(inner).0)),
            Inline::SmallCaps(inner) => {
                container(code, out, Inline::SmallCaps(code_inlines(inner).0));
            }
            Inline::Underline(inner) => {
                container(code, out, Inline::Underline(code_inlines(inner).0));
            }
            Inline::Strikeout(inner) => {
                container(code, out, Inline::Strikeout(code_inlines(inner).0));
            }
            Inline::Superscript(inner) => {
                container(code, out, Inline::Superscript(code_inlines(inner).0));
            }
            Inline::Subscript(inner) => {
                container(code, out, Inline::Subscript(code_inlines(inner).0));
            }
            Inline::Span(attr, inner) => {
                container(code, out, Inline::Span(attr, code_inlines(inner).0));
            }
            Inline::Link(attr, inner, target) => {
                container(code, out, Inline::Link(attr, code_inlines(inner).0, target));
            }
            Inline::Image(attr, inner, target) => {
                container(
                    code,
                    out,
                    Inline::Image(attr, code_inlines(inner).0, target),
                );
            }
            Inline::Note(blocks) => {
                let blocks = blocks
                    .into_iter()
                    .map(|block| match block {
                        Block::Para(inner) => Block::Para(code_inlines(inner).0),
                        Block::Plain(inner) => Block::Plain(code_inlines(inner).0),
                        other => other,
                    })
                    .collect();
                container(code, out, Inline::Note(blocks));
            }
            other => container(code, out, other),
        }
    }
}

/// The options of `lstlisting` (`[language=X]`) and of `minted` (`{X}`) at
/// the start of the verbatim body.
fn code_attributes<'a>(name: &str, body: &'a str) -> (Attr, &'a str) {
    let mut attr = Attr::default();
    if !matches!(name, "Verbatim" | "lstlisting" | "minted") {
        return (attr, body);
    }
    let mut body = body;
    if let Some((options, rest)) = body.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
        body = rest;
        for option in options
            .split(',')
            .map(str::trim)
            .filter(|option| !option.is_empty())
        {
            let (key, value) = option.split_once('=').unwrap_or((option, ""));
            let (key, value) = (key.trim(), value.trim());
            match (name, key, value) {
                ("lstlisting", "language", _) => attr.classes.push(listings_language(value)),
                ("lstlisting", "numbers", "left") => attr.classes.insert(0, "numberLines".into()),
                ("lstlisting", "label", _) => attr.id = value.to_owned(),
                _ => {}
            }
            attr.attributes.push((key.to_owned(), value.to_owned()));
        }
    }
    if name == "minted"
        && let Some((language, rest)) = body
            .trim_start_matches([' ', '\t'])
            .strip_prefix('{')
            .and_then(|rest| rest.split_once('}'))
    {
        body = rest;
        attr.classes.push(language.trim().to_owned());
    }
    (attr, body)
}

fn is_block_only(name: &str) -> bool {
    sectioning_level(name).is_some()
        || matches!(
            name,
            "item"
                | "title"
                | "author"
                | "date"
                | "maketitle"
                | "caption"
                | "documentclass"
                | "bibliography"
                | "addbibresource"
                | "bibliographystyle"
                | "usepackage"
                | "listoffigures"
                | "listoftables"
                | "newtheorem"
                | "theoremstyle"
                | "addtocounter"
                | "graphicspath"
                | "raggedright"
                | "parbox"
                | "include"
        )
}

fn is_stream_noop(name: &str) -> bool {
    matches!(name, "xspace" | "expandafter")
}

fn is_size_or_fill(name: &str) -> bool {
    matches!(
        name,
        "small"
            | "footnotesize"
            | "large"
            | "Large"
            | "tiny"
            | "scriptsize"
            | "normalsize"
            | "huge"
            | "LARGE"
            | "Huge"
            | "rm"
            | "hfill"
            | "vfill"
    )
}

fn babel_language(name: &str) -> Option<&'static str> {
    match name {
        "french" => Some("fr"),
        "german" => Some("de-DE-1901"),
        _ => None,
    }
}

fn language_attr(language: &str) -> Attr {
    Attr::new("", vec![], vec![("lang".into(), language.into())])
}

fn append_measurement_number(out: &mut Inlines, value: &str) {
    let normalized = value.replace('-', "−");
    if let Some((mantissa, exponent)) = value.split_once(['e', 'E'])
        && mantissa.parse::<f64>().is_ok()
        && let Ok(exponent) = exponent.parse::<i64>()
    {
        out.push_str(&mantissa.replace('-', "−"));
        out.push_str("\u{a0}×\u{a0}10");
        out.push(Inline::Superscript(vec![Inline::Str(
            exponent.to_string().replace('-', "−"),
        )]));
    } else {
        out.push_str(&normalized);
    }
}

fn append_measurement_unit(out: &mut Inlines, tokens: &[Tok]) {
    let mut per = false;
    let mut exponent = None;
    let mut unit = Text::new();
    for tok in tokens {
        if let Kind::Word(name) = &tok.kind {
            let symbol = match name.as_str() {
                "kilo" => {
                    unit.push('k');
                    continue;
                }
                "metre" | "meter" => "m",
                "second" => "s",
                "degreeCelsius" => "°C",
                "per" => {
                    per = true;
                    continue;
                }
                "squared" => {
                    exponent = Some(2);
                    continue;
                }
                "cubed" => {
                    exponent = Some(3);
                    continue;
                }
                _ => tok.text.as_str(),
            };
            if per && !unit.is_empty() {
                out.push_str(&unit);
                out.push_str(NBSP);
                unit.clear();
            }
            unit.push_str(symbol);
        } else if !matches!(tok.kind, Kind::Space | Kind::Newline | Kind::Comment) {
            unit.push_str(&tok.text);
        }
    }
    out.push_str(&unit);
    if let Some(power) = exponent.or(per.then_some(1)) {
        let prefix = if per { "−" } else { "" };
        out.push(Inline::Superscript(vec![Inline::Str(format!(
            "{prefix}{power}"
        ))]));
    }
}

fn letter_symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "ss" => "\u{df}",
        "o" => "\u{f8}",
        "O" => "\u{d8}",
        "ae" => "\u{e6}",
        "AE" => "\u{c6}",
        "oe" => "\u{153}",
        "OE" => "\u{152}",
        "aa" => "\u{e5}",
        "AA" => "\u{c5}",
        "l" => "\u{142}",
        "L" => "\u{141}",
        "i" => "\u{131}",
        "j" => "\u{237}",
        "textbackslash" => "\\",
        "copyright" | "textcopyright" => "\u{a9}",
        "textregistered" => "®",
        "textdegree" => "°",
        "textasciitilde" => "~",
        "textasciicircum" => "^",
        "textless" => "<",
        "textgreater" => ">",
        "textquoteleft" => "‘",
        "textquoteright" => "’",
        "textquotedblleft" => "“",
        "textquotedblright" => "”",
        "guillemotleft" => "«",
        "guillemotright" => "»",
        "S" => "§",
        "P" => "¶",
        "pounds" => "£",
        "euro" => "€",
        "textbullet" => "•",
        "slash" => "/",
        _ => return None,
    })
}

/// The number style and the delimiter of an `enumerate` label such as `(a)`.
fn list_label(label: &str) -> (ListNumberStyle, ListNumberDelim) {
    let default = (ListNumberStyle::DefaultStyle, ListNumberDelim::DefaultDelim);
    let (number, delim) = if let Some(inner) = label
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
    {
        (inner, ListNumberDelim::TwoParens)
    } else if let Some(inner) = label.strip_suffix(')') {
        (inner, ListNumberDelim::OneParen)
    } else if let Some(inner) = label.strip_suffix('.') {
        (inner, ListNumberDelim::Period)
    } else {
        return default;
    };
    let style = match number {
        "1" => ListNumberStyle::Decimal,
        "a" => ListNumberStyle::LowerAlpha,
        "A" => ListNumberStyle::UpperAlpha,
        "i" => ListNumberStyle::LowerRoman,
        "I" => ListNumberStyle::UpperRoman,
        _ => return default,
    };
    (style, delim)
}

/// An optional `-` and decimal digits, read as a 64-bit number that wraps.
fn decimal_wrapping(text: &str) -> Option<i64> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value = digits.bytes().fold(0i64, |value, b| {
        value.wrapping_mul(10).wrapping_add(i64::from(b - b'0'))
    });
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

/// The first command of an `enumerate`, as its start number reads it.
enum Counter {
    /// No `\setcounter` comes first.
    Absent,
    /// `\setcounter` of an `enum` counter, with VALUE + 1 when VALUE is
    /// decimal. It is a part of the list, also without items.
    Enum(Option<i64>),
    /// `\setcounter` of another counter, or without its two groups.
    Other,
}

/// The section number for the text of a `\ref`, `\autoref` or `\cref` link.
fn section_reference(attr: &Attr, numbers: &HashMap<Text, Text>) -> Option<Text> {
    let value = |key: &str| {
        attr.attributes
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    };
    if !matches!(
        value("reference-type")?.as_str(),
        "ref" | "ref+label" | "ref+Label"
    ) {
        return None;
    }
    numbers.get(value("reference")?).cloned()
}

/// What a star after a citation command does.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Star {
    /// The citation suppresses the author: `\parencite*`.
    SuppressAuthor,
    /// The star is not part of the raw text: `\textcite*`.
    NotInRaw,
    /// The star stays in the raw text and changes nothing: `\citep*`.
    Kept,
}

fn citation_star(name: &str) -> Star {
    match name {
        "cite" | "Cite" | "parencite" | "Parencite" | "autocite" | "Autocite" => {
            Star::SuppressAuthor
        }
        "textcite" | "Textcite" | "citeauthor" | "citeyear" => Star::NotInRaw,
        _ => Star::Kept,
    }
}

fn citation_mode(name: &str) -> Option<CitationMode> {
    Some(match name {
        "cite" | "citep" | "citealp" | "parencite" | "autocite" | "Cite" | "Parencite"
        | "Autocite" => CitationMode::NormalCitation,
        "citet" | "citealt" | "textcite" | "Textcite" | "Citet" | "Citep" | "citeauthor" => {
            CitationMode::AuthorInText
        }
        "citeyear" | "citeyearpar" => CitationMode::SuppressAuthor,
        _ => return None,
    })
}

/// The heading level of a sectioning command before the shift of `\part`
/// and `\chapter`.
fn sectioning_level(name: &str) -> Option<i64> {
    Some(match name {
        "part" => -1,
        "chapter" => 0,
        "section" => 1,
        "subsection" => 2,
        "subsubsection" => 3,
        "paragraph" => 4,
        "subparagraph" => 5,
        _ => return None,
    })
}

/// The mode of the first citation of a biblatex command with several key
/// groups, such as `\cites`. The other citations are normal.
fn multi_citation_mode(name: &str) -> Option<CitationMode> {
    Some(match name {
        "cites" | "Cites" | "parencites" | "Parencites" | "autocites" | "Autocites"
        | "supercites" | "footcites" => CitationMode::NormalCitation,
        "textcites" | "Textcites" => CitationMode::AuthorInText,
        _ => return None,
    })
}

/// What must come after a command, after its star and options.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ArgumentRule {
    /// A `{...}` group.
    Group,
    /// A control sequence.
    Name,
    /// A control sequence, alone or in a group.
    NameOrGroup,
}

/// The commands for which Pandoc stops with a parse error when the
/// argument is not there. Found with probes of each command of
/// `tools/names.tsv` (matrix `braced` of `rules.toml`).
fn argument_rule(name: &str) -> Option<ArgumentRule> {
    if citation_mode(name).is_some()
        || multi_citation_mode(name).is_some()
        || name == "footcite"
        || sectioning_level(name).is_some()
    {
        return Some(ArgumentRule::Group);
    }
    match name {
        "def" | "gdef" | "edef" | "xdef" | "let" => Some(ArgumentRule::Name),
        "newcommand" | "renewcommand" | "providecommand" | "DeclareMathOperator" => {
            Some(ArgumentRule::NameOrGroup)
        }
        "label" | "ref" | "eqref" | "autoref" | "cref" | "Cref" | "nocite" | "begin"
        | "usepackage" | "includegraphics" | "footnote" | "footnotetext" | "documentclass"
        | "url" | "href" | "nolinkurl" | "hyperlink" | "hypertarget" | "hyperref" | "title"
        | "author" | "thanks" | "bibliography" | "addbibresource" | "input" | "include"
        | "newenvironment" | "newtheorem" | "theoremstyle" | "textcolor" | "colorbox"
        | "foreignquote" | "SI" | "si" | "num" | "qty" | "ang" | "ensuremath" | "gls" | "Gls"
        | "glspl" | "ac" | "parbox" | "graphicspath" => Some(ArgumentRule::Group),
        _ => None,
    }
}

/// Skips the tokens that Pandoc skips around an optional argument:
/// spaces, comments and at most `line_ends` line ends. Gives the number
/// of tokens skipped, and true when a line end is among them.
fn skip_option_gap<'t>(
    tokens: &mut std::iter::Peekable<impl Iterator<Item = &'t Tok>>,
    line_ends: usize,
) -> (usize, bool) {
    let mut seen = 0;
    let mut skip = 0;
    while let Some(tok) = tokens.next_if(|tok| match tok.kind {
        Kind::Space | Kind::Comment => true,
        Kind::Newline => seen < line_ends,
        _ => false,
    }) {
        seen += usize::from(tok.kind == Kind::Newline);
        skip += 1;
    }
    (skip, seen > 0)
}

/// The commands whose argument group must follow after spaces only: a
/// line end or a comment there is the parse error of Pandoc (matrix
/// `errors/eofarg-*`).
fn takes_verbatim_argument(name: &str) -> bool {
    matches!(
        name,
        "label"
            | "ref"
            | "eqref"
            | "autoref"
            | "cref"
            | "Cref"
            | "includegraphics"
            | "nolinkurl"
            | "ensuremath"
            | "url"
            | "href"
            | "hypertarget"
            | "hyperlink"
            | "foreignquote"
    )
}

/// The number of `[n]`. A number too large for `usize` is `usize::MAX`:
/// no input has so many arguments.
fn parameter_count(text: &str) -> Option<usize> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(text.parse().unwrap_or(usize::MAX))
}

/// False for a command of `argument_rule` for which Pandoc reads no
/// `[...]` option: a `[` there is the parse error.
fn takes_options(name: &str) -> bool {
    !matches!(
        name,
        "label"
            | "ref"
            | "eqref"
            | "autoref"
            | "cref"
            | "Cref"
            | "begin"
            | "url"
            | "href"
            | "nolinkurl"
            | "hyperlink"
            | "hypertarget"
            | "newenvironment"
            | "newtheorem"
            | "theoremstyle"
            | "foreignquote"
            | "ensuremath"
            | "gls"
            | "Gls"
            | "glspl"
            | "ac"
            | "graphicspath"
    )
}

fn is_definition(name: &str) -> bool {
    matches!(
        name,
        "newcommand"
            | "renewcommand"
            | "providecommand"
            | "def"
            | "gdef"
            | "edef"
            | "xdef"
            | "global"
            | "let"
            | "newenvironment"
            | "renewenvironment"
            | "DeclareMathOperator"
    )
}

/// The TeX primitives that define a macro with a parameter text.
fn is_def_word(name: &str) -> bool {
    matches!(name, "def" | "gdef" | "edef" | "xdef")
}

fn is_tabular(name: &str) -> bool {
    matches!(name, "tabular" | "tabular*" | "tabularx" | "longtable")
}

/// The math environments. Pandoc reads them as inline math.
pub(crate) const INLINE_ENVIRONMENTS: &[&str] = &[
    "math",
    "displaymath",
    "subequations",
    "equation",
    "equation*",
    "align",
    "align*",
    "gather",
    "gather*",
    "multline",
    "multline*",
    "eqnarray",
    "eqnarray*",
    "flalign",
    "flalign*",
    "alignat",
    "alignat*",
];

/// The block environments that the reader knows. With `raw_tex`, each
/// other environment is raw TeX.
pub(crate) const BLOCK_ENVIRONMENTS: &[&str] = &[
    "proof",
    "alltt",
    "sloppypar",
    "subfigure",
    "otherlanguage",
    "document",
    "abstract",
    "itemize",
    "enumerate",
    "description",
    "quote",
    "quotation",
    "verse",
    "center",
    "minipage",
    "verbatim",
    "Verbatim",
    "lstlisting",
    "minted",
    "comment",
    "figure",
    "figure*",
    "table",
    "tabular",
    "tabular*",
    "tabularx",
    "longtable",
];

/// The environments that Pandoc reads again with a fallback rule after a
/// parse error inside: lists, tables and each environment that Pandoc does
/// not know. A parse error in other environments stops the document.
fn falls_back(name: &str) -> bool {
    matches!(name, "itemize" | "enumerate" | "description")
        || is_tabular(name)
        || !matches!(
            name,
            "document"
                | "abstract"
                | "quote"
                | "quotation"
                | "verse"
                | "center"
                | "minipage"
                | "figure"
                | "figure*"
                | "table"
                | "verbatim"
                | "Verbatim"
                | "lstlisting"
                | "minted"
                | "comment"
                | "proof"
                | "subfigure"
                | "alltt"
                | "otherlanguage"
                | "sloppypar"
                | "letter"
        )
}

fn is_inline_environment(name: &str) -> bool {
    INLINE_ENVIRONMENTS.contains(&name)
}

fn is_known_environment(name: &str) -> bool {
    BLOCK_ENVIRONMENTS.contains(&name) || is_inline_environment(name)
}

/// A command that the reader does not know. With `raw_tex`, it is raw TeX.
fn is_unknown_command(name: &str) -> bool {
    !matches!(
        name,
        "emph"
            | "textit"
            | "textsl"
            | "textbf"
            | "underline"
            | "uline"
            | "sout"
            | "textsc"
            | "textsuperscript"
            | "textsubscript"
            | "textsf"
            | "textrm"
            | "em"
            | "it"
            | "itshape"
            | "sl"
            | "slshape"
            | "bf"
            | "bfseries"
            | "texttt"
            | "tt"
            | "ttfamily"
            | "scshape"
            | "textup"
            | "textmd"
            | "textnormal"
            | "vbox"
            | "nolinkurl"
            | "hyperlink"
            | "hyperref"
            | "hypertarget"
            | "textcolor"
            | "colorbox"
            | "gls"
            | "Gls"
            | "glspl"
            | "ac"
            | "foreignquote"
            | "MakeUppercase"
            | "MakeLowercase"
            | "uppercase"
            | "lowercase"
            | "SI"
            | "si"
            | "qty"
            | "ang"
            | "num"
            | "ensuremath"
            | "footnotemark"
            | "footnotetext"
            | "t"
            | "lstinline"
            | "mintinline"
            | "hskip"
            | "vskip"
            | "kern"
            | "mbox"
            | "hbox"
            | "LaTeX"
            | "TeX"
            | "ldots"
            | "newline"
            | "NeedsTeXFormat"
            | "dots"
            | "footnote"
            | "thanks"
            | "url"
            | "href"
            | "label"
            | "caption"
            | "ref"
            | "eqref"
            | "autoref"
            | "cref"
            | "Cref"
            | "pageref"
            | "includegraphics"
            | "c"
            | "v"
            | "u"
            | "H"
            | "r"
            | "k"
            | "d"
            | "b"
            | "verb"
            | "input"
            | "include"
            | "today"
            | "nocite"
            | "enquote"
            | "texorpdfstring"
            | "char"
            | "footcite"
            | "begin"
            | "end"
            | "item"
    ) && !is_size_or_fill(name)
        && !is_block_only(name)
        && letter_symbol(name).is_none()
        && citation_mode(name).is_none()
        && multi_citation_mode(name).is_none()
}
