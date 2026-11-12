//! The tree rules. Each rule was found with Pandoc 3.13 as a black box.

use std::collections::HashMap;

use randoc_ast::walk::walk_inlines_mut;
use randoc_ast::{
    Alignment, Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, ColWidth, Format,
    Inline, ListAttributes, ListNumberDelim, ListNumberStyle, MathType, Meta, MetaValue, QuoteType,
    Row, Table, TableBody, TableHead, Target, Text,
};
use randoc_core::shared::{IdentifierSet, auto_identifier, nfc};
use randoc_core::{Context, Extension, Extensions, LogMessage, SourcePos};

use crate::inlines::Inlines;
use crate::tokens::{Kind, Tok, tokenize};

/// Why the parser stopped before the end of the input.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Stop {
    /// The input is nested deeper than the limit of the context.
    TooDeep,
    /// A token that the grammar does not permit. The text is the message.
    Unexpected(Text),
    /// A macro expands without an end. The text is the macro name.
    Loop(Text),
}

type R<T> = Result<T, Stop>;

/// The expansion depth at which Randoc stops a macro as a loop.
pub(crate) const MAX_EXPANSION_DEPTH: u32 = 100;
/// The longest optional argument of `\cites` that the lookahead accepts.
const MAX_OPTION_TOKENS: usize = 256;
/// The largest number of tokens that all macro expansions together can make.
pub(crate) const MAX_EXPANDED_TOKENS: usize = 1_000_000;

#[derive(Debug, Clone)]
struct Macro {
    parameters: usize,
    default: Option<Vec<Tok>>,
    body: Vec<Tok>,
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
    /// At a paragraph break or at the start of a block.
    Paragraph,
    /// At the closing quotation mark.
    Quote(QuoteType),
    /// At the `]` of an optional argument.
    Bracket,
    /// At the end of a table cell.
    Cell,
}

/// The state of a figure or a table environment.
#[derive(Debug, Default)]
struct Float {
    figure: bool,
    caption: Option<(Option<Vec<Inline>>, Vec<Inline>)>,
    label: Option<Text>,
}

pub(crate) struct Parser<'c> {
    tokens: Vec<Tok>,
    position: usize,
    /// Tokens of macro expansions, in reverse order: the last one is next.
    pending: Vec<Tok>,
    macros: HashMap<Text, Macro>,
    environments: HashMap<Text, EnvironmentMacro>,
    extensions: Extensions,
    depth: usize,
    max_depth: usize,
    expanded: usize,
    recorders: Vec<Text>,
    floats: Vec<Float>,
    level_offset: i64,
    identifiers: IdentifierSet,
    pub(crate) meta: Meta,
    pub(crate) context: &'c mut Context,
    finished: bool,
    input: Text,
    /// The number of tokens consumed. A loop checks it for progress.
    bumps: usize,
    /// The flags of `\newif`, without the `if`.
    flags: HashMap<Text, bool>,
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
        let has = |name: &str| tokens.iter().any(|tok| tok.is_word(name));
        let level_offset = if has("part") {
            2
        } else if has("chapter") {
            1
        } else {
            0
        };
        Parser {
            tokens,
            position: 0,
            pending: Vec::new(),
            macros: HashMap::new(),
            environments: HashMap::new(),
            extensions,
            depth: 0,
            max_depth: context.limits.max_nesting_depth,
            expanded: 0,
            recorders: Vec::new(),
            floats: Vec::new(),
            level_offset,
            identifiers: IdentifierSet::new(),
            meta: Meta::default(),
            context,
            finished: false,
            input: input.to_owned(),
            bumps: 0,
            scanned: std::cell::Cell::new(0),
            quote_kinds: Vec::new(),
            flags: HashMap::new(),
            skips: HashMap::new(),
            conditionals,
        }
    }

    /// The number of tokens that the parser consumed, a measure of its work.
    #[cfg(test)]
    pub(crate) fn steps(&self) -> usize {
        self.bumps + self.scanned.get()
    }

    pub(crate) fn document(&mut self) -> R<Vec<Block>> {
        self.blocks(End::Input)
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
            Some(tok) => tok,
            None => {
                self.position = self.raw_position();
                let tok = self.tokens.get(self.position)?.clone();
                self.position += 1;
                tok
            }
        };
        self.bumps += 1;
        for record in &mut self.recorders {
            record.push_str(&tok.text);
        }
        Some(tok)
    }

    /// Expands macros until the next token is not a macro.
    fn expand(&mut self) -> R<()> {
        let macros = self.extensions.contains(Extension::LatexMacros);
        loop {
            let Some(Tok {
                kind: Kind::Word(name),
                depth,
                ..
            }) = self.peek_raw()
            else {
                return Ok(());
            };
            let (name, depth) = (name.clone(), *depth);
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
            self.bump();
            self.skip_word_spaces();
            let arguments = self.macro_arguments(definition.parameters, definition.default)?;
            self.push_expansion(&name, depth, &definition.body, &arguments)?;
        }
    }

    fn macro_arguments(&mut self, count: usize, default: Option<Vec<Tok>>) -> R<Vec<Vec<Tok>>> {
        let mut arguments = Vec::new();
        if let Some(default) = default {
            arguments.push(self.bracket_tokens().unwrap_or(default));
        }
        while arguments.len() < count {
            arguments.push(self.argument_tokens().unwrap_or_default());
        }
        Ok(arguments)
    }

    fn push_expansion(
        &mut self,
        name: &str,
        depth: u32,
        body: &[Tok],
        arguments: &[Vec<Tok>],
    ) -> R<()> {
        if depth + 1 > MAX_EXPANSION_DEPTH {
            return Err(Stop::Loop(name.to_owned()));
        }
        let mut out = Vec::new();
        for tok in body {
            match tok.kind {
                Kind::Param(n) if n >= 1 => {
                    if let Some(argument) = arguments.get(usize::from(n) - 1) {
                        out.extend(argument.iter().cloned());
                    }
                }
                _ => out.push(tok.clone()),
            }
        }
        self.expanded += out.len();
        if self.expanded > MAX_EXPANDED_TOKENS {
            return Err(Stop::Loop(name.to_owned()));
        }
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
        let arguments = self.macro_arguments(definition.parameters, definition.default)?;
        let body = self.environment_body_tokens(&name);
        let mut all = vec![self.synthetic(Kind::Open, "{")];
        all.extend(definition.begin.iter().cloned());
        all.extend(body);
        all.extend(definition.end.iter().cloned());
        all.push(self.synthetic(Kind::Close, "}"));
        self.push_expansion(&name, depth, &all, &arguments)?;
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
                Kind::Word(name) if sectioning_level(name).is_some() => return true,
                _ => {}
            }
        }
        false
    }

    /// True when the group that the next token opens holds one group and
    /// spaces before it, nothing else. Pandoc reads the inner group as the
    /// argument: `\emph{{x}}` is `\emph{x}`, `\emph{{x} }` keeps a group.
    fn whole_group_ahead(&self) -> bool {
        let mut tokens = self
            .scan()
            .skip(1)
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
            self.flags.insert(flag, false);
            return true;
        }
        let setting = [("true", true), ("false", false)]
            .into_iter()
            .find_map(|(suffix, value)| Some((name.strip_suffix(suffix)?, value)));
        match setting {
            Some((flag, value)) if self.flags.contains_key(flag) => {
                self.flags.insert(flag.to_owned(), value);
                self.bump();
                true
            }
            _ => false,
        }
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
        self.pending.extend(chosen.into_iter().rev());
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

    /// The tokens from the next one on, counted as the work of a lookahead rule.
    fn scan(&self) -> impl Iterator<Item = &Tok> + Clone + '_ {
        self.ahead()
            .inspect(|_| self.scanned.set(self.scanned.get() + 1))
    }

    /// The name of `\begin{NAME}` when the next token is `\begin`.
    fn environment_name_ahead(&self) -> Option<Text> {
        name_in(self.ahead().skip(1).take(64))
    }

    /// The tokens up to the `\end{NAME}` that closes the environment, which
    /// it consumes.
    fn environment_body_tokens(&mut self, name: &str) -> Vec<Tok> {
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
                        return body;
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
        body
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
                let mut tok = self.bump()?;
                let mut chars = text_chars(&tok.text);
                let first: Text = chars.next().map(String::from).unwrap_or_default();
                let rest: Text = chars.collect();
                let mut rest_tok = tok.clone();
                rest_tok.kind = Kind::Letters(rest.clone());
                rest_tok.text = rest;
                rest_tok.column += 1;
                self.pending.push(rest_tok);
                tok.kind = Kind::Letters(first.clone());
                tok.text = first;
                Some(vec![tok])
            }
            _ => self.bump().map(|tok| vec![tok]),
        }
    }

    /// The tokens up to the `}` that closes the current group, which it consumes.
    fn tokens_until_close(&mut self) -> Vec<Tok> {
        let mut nesting = 0usize;
        let mut out = Vec::new();
        while let Some(tok) = self.bump() {
            match tok.kind {
                Kind::Open => nesting += 1,
                Kind::Close if nesting == 0 => break,
                Kind::Close => nesting -= 1,
                _ => {}
            }
            out.push(tok);
        }
        out
    }

    /// The tokens of a `[...]` argument without its brackets.
    fn bracket_tokens(&mut self) -> Option<Vec<Tok>> {
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

    fn argument_text(&mut self) -> Text {
        self.argument_tokens()
            .map(|tokens| tokens_text(&tokens))
            .unwrap_or_default()
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
        self.nested(|parser| {
            let saved_tokens = std::mem::replace(&mut parser.tokens, tokens);
            let saved_position = std::mem::replace(&mut parser.position, 0);
            let saved_pending = std::mem::take(&mut parser.pending);
            let saved_finished = std::mem::replace(&mut parser.finished, false);
            let saved_conditionals =
                std::mem::replace(&mut parser.conditionals, match_conditionals(&parser.tokens));
            let saved_skips = std::mem::take(&mut parser.skips);
            let result = run(parser);
            parser.conditionals = saved_conditionals;
            parser.skips = saved_skips;
            parser.tokens = saved_tokens;
            parser.position = saved_position;
            parser.pending = saved_pending;
            parser.finished = saved_finished;
            result
        })
    }

    fn inlines_of(&mut self, tokens: Vec<Tok>) -> R<Vec<Inline>> {
        self.with_tokens(tokens, |parser| Ok(parser.inlines(Until::Close)?.trimmed()))
    }

    fn blocks_of(&mut self, tokens: Vec<Tok>) -> R<Vec<Block>> {
        self.with_tokens(tokens, |parser| parser.blocks(End::Input))
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
            let whole = self.whole_group_ahead();
            self.bump();
            let inlines = if whole {
                self.nested(Self::argument_inlines_untrimmed)?
            } else {
                self.nested(|parser| parser.inlines(Until::Close))?.0
            };
            self.close_group();
            return Ok(inlines);
        }
        let tokens = self.argument_tokens().unwrap_or_default();
        self.inlines_of(tokens)
    }

    /// The inlines of an optional `[...]` argument, read in place.
    fn bracket_inlines(&mut self) -> R<Option<Vec<Inline>>> {
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
                self.nested(Self::argument_blocks)?
            } else {
                self.nested(|parser| parser.blocks(End::Close))?
            };
            self.close_group();
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

    fn close_group(&mut self) {
        if self.peek_raw().is_some_and(|tok| tok.kind == Kind::Close) {
            self.bump();
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
        SourcePos::new("", line, column)
    }

    // Blocks.

    fn blocks(&mut self, end: End) -> R<Vec<Block>> {
        let mut blocks = Vec::new();
        while let Some(kind) = self.peek_kind()? {
            match kind {
                Kind::Par | Kind::Space | Kind::Newline | Kind::Comment => {
                    self.bump();
                }
                Kind::Close if end == End::Close => break,
                Kind::Close if self.depth == 0 => return Err(self.unexpected()),
                Kind::Close => {
                    self.bump();
                }
                Kind::Word(name) if name == "end" => {
                    let ahead = self.environment_name_ahead();
                    if matches!(end, End::Environment(n) | End::Item(n) if Some(n) == ahead.as_deref())
                    {
                        break;
                    }
                    self.bump();
                    self.argument_tokens();
                }
                Kind::Word(name) if name == "item" && matches!(end, End::Item(_)) => break,
                Kind::Word(name) if name == "begin" => {
                    let environment = self.environment_name_ahead().unwrap_or_default();
                    if is_inline_environment(&environment) {
                        self.paragraph(&mut blocks)?;
                    } else {
                        self.nested(|parser| parser.environment(&environment, &mut blocks))?;
                    }
                }
                Kind::Word(name) if self.block_command(&name, &mut blocks)? => {}
                Kind::Open if self.group_holds_block() => {
                    self.bump();
                    let inner = self.nested(|parser| parser.blocks(End::Close))?;
                    self.close_group();
                    blocks.extend(inner);
                }
                _ => self.paragraph(&mut blocks)?,
            }
        }
        Ok(blocks)
    }

    /// A paragraph. It consumes one token at least, so that each loop
    /// that calls it ends.
    fn paragraph(&mut self, blocks: &mut Vec<Block>) -> R<()> {
        let before = self.bumps;
        let mut inlines = self.inlines(Until::Paragraph)?;
        inlines.flush_into(blocks);
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
            let _short = self.bracket_tokens();
            let title = self.argument_inlines_untrimmed()?;
            self.heading(base + self.level_offset, star, title, blocks);
            return Ok(true);
        }
        match name {
            "title" | "author" | "date" => {
                self.bump();
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
                self.meta.insert(name.to_owned(), value);
                Ok(true)
            }
            "maketitle" => {
                self.start_record();
                self.bump();
                let raw = self.stop_record();
                let alone = matches!(
                    self.peek_raw().map(|tok| &tok.kind),
                    None | Some(Kind::Par | Kind::Newline)
                );
                if self.raw_tex() && alone {
                    blocks.push(Block::RawBlock(Format::new("latex"), raw));
                } else {
                    self.skip_group();
                }
                Ok(true)
            }
            "documentclass" => {
                self.unknown_command_raw()?;
                self.preamble()?;
                Ok(true)
            }
            _ if is_definition(name) && !self.extensions.contains(Extension::LatexMacros) => {
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
                    blocks.push(Block::RawBlock(Format::new("latex"), raw));
                    return Ok(true);
                }
                self.pending.extend(consumed.into_iter().rev());
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn heading(&mut self, level: i64, star: bool, title: Vec<Inline>, blocks: &mut Vec<Block>) {
        let mut attr = Attr::default();
        if star {
            attr.classes.push("unnumbered".into());
        }
        self.skip_blank();
        if self.peek_raw().is_some_and(|tok| tok.is_word("label")) {
            self.bump();
            attr.id = self.argument_text();
            self.identifiers.register(&attr.id);
        }
        if attr.id.is_empty() && self.extensions.contains(Extension::AutoIdentifiers) {
            attr.id = self
                .identifiers
                .unique(&auto_identifier(&title, self.extensions));
        }
        blocks.push(Block::Header(level, attr, title));
    }

    /// `\input` and `\include` put the tokens of the file in the input.
    fn include(&mut self, depth: u32) -> R<()> {
        let name = self.argument_text();
        let pos = self.position_of_next();
        let path = if name.contains('.') {
            name.clone()
        } else {
            format!("{name}.tex")
        };
        let Ok(bytes) = self.context.read_file(&path) else {
            self.context
                .report(LogMessage::CouldNotLoadIncludeFile(path, pos));
            return Ok(());
        };
        let text = String::from_utf8_lossy(&bytes).replace('\r', "");
        let tokens = tokenize(&text);
        self.push_expansion(&path, depth, &tokens, &[])
    }

    /// `\newcommand` and the other macro definitions.
    fn definition(&mut self, command: &str, pos: SourcePos) -> R<()> {
        self.skip_word_spaces();
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
                let definition = match &target.kind {
                    Kind::Word(word) => self.macros.get(word).cloned(),
                    _ => None,
                }
                .unwrap_or(Macro {
                    parameters: 0,
                    default: None,
                    body: vec![target],
                });
                self.macros.insert(name, definition);
            }
            "def" => {
                let name = self.defined_name();
                let mut parameters = 0;
                while let Some(tok) = self.peek_raw() {
                    match tok.kind {
                        Kind::Open => break,
                        Kind::Param(n) => parameters = usize::from(n).max(parameters),
                        _ => {}
                    }
                    self.bump();
                }
                let body = self.argument_tokens().unwrap_or_default();
                self.macros.insert(
                    name,
                    Macro {
                        parameters,
                        default: None,
                        body,
                    },
                );
            }
            "newenvironment" | "renewenvironment" => {
                let name = self.argument_text();
                let (parameters, default) = self.parameter_specification();
                let begin = self.argument_tokens().unwrap_or_default();
                let end = self.argument_tokens().unwrap_or_default();
                if command == "newenvironment" && self.environments.contains_key(&name) {
                    return Ok(());
                }
                self.environments.insert(
                    name,
                    EnvironmentMacro {
                        parameters,
                        default,
                        begin,
                        end,
                    },
                );
            }
            _ => {
                let name = self.defined_name();
                let (parameters, default) = self.parameter_specification();
                let body = self.argument_tokens().unwrap_or_default();
                let exists = self.macros.contains_key(&name);
                if exists && command == "newcommand" {
                    self.context
                        .report(LogMessage::MacroAlreadyDefined(format!("\\{name}"), pos));
                    return Ok(());
                }
                if exists && command == "providecommand" {
                    return Ok(());
                }
                self.macros.insert(
                    name,
                    Macro {
                        parameters,
                        default,
                        body,
                    },
                );
            }
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
            .and_then(|tokens| tokens_text(&tokens).trim().parse().ok())
            .unwrap_or(0);
        self.skip_spaces();
        let default = self.bracket_tokens();
        (count, default)
    }

    // Environments.

    fn environment(&mut self, name: &str, blocks: &mut Vec<Block>) -> R<()> {
        if self.extensions.contains(Extension::RawTex) && !is_known_environment(name) {
            self.start_record();
            self.bump();
            self.argument_tokens();
            self.environment_body_tokens(name);
            let raw = self.stop_record();
            blocks.push(Block::RawBlock(Format::new("latex"), raw));
            return Ok(());
        }
        self.bump();
        self.argument_tokens();
        match name {
            "document" => {
                blocks.extend(self.blocks(End::Environment(name))?);
                self.finished = true;
                return Ok(());
            }
            "abstract" => {
                let content = self.blocks(End::Environment(name))?;
                self.meta
                    .insert(Text::from("abstract"), MetaValue::MetaBlocks(content));
            }
            "itemize" | "enumerate" | "description" => {
                let mut attributes = ListAttributes {
                    start: 1,
                    style: ListNumberStyle::DefaultStyle,
                    delim: ListNumberDelim::DefaultDelim,
                };
                if name != "description" {
                    self.skip_spaces();
                    if let Some(label) = self.bracket_tokens() {
                        (attributes.style, attributes.delim) = list_label(&tokens_text(&label));
                    }
                }
                blocks.push(self.list(name, attributes)?);
            }
            "quote" | "quotation" | "verse" => {
                blocks.push(Block::BlockQuote(self.blocks(End::Environment(name))?));
            }
            "verbatim" | "Verbatim" | "lstlisting" | "minted" | "comment" => {
                if let Some(block) = self.verbatim(name) {
                    blocks.push(block);
                }
            }
            "figure" | "figure*" => blocks.push(self.figure(name)?),
            "table" | "table*" => blocks.extend(self.table_float(name)?),
            _ if is_tabular(name) => {
                if name != "tabular" && name != "longtable" {
                    self.argument_tokens();
                }
                blocks.push(self.tabular()?);
            }
            _ => {
                self.bracket_tokens();
                if name == "minipage" {
                    self.argument_tokens();
                }
                let content = self.blocks(End::Environment(name))?;
                blocks.push(Block::Div(
                    Attr::new("", vec![name.to_owned()], vec![]),
                    content,
                ));
            }
        }
        self.end_environment();
        Ok(())
    }

    fn end_environment(&mut self) {
        if self.peek_raw().is_some_and(|tok| tok.is_word("end")) {
            self.bump();
            self.argument_tokens();
        }
    }

    /// A list. A list without items, or a description item without a
    /// term, makes a `Div` with the class of the environment, as in Pandoc.
    fn list(&mut self, name: &str, attributes: ListAttributes) -> R<Block> {
        let mut items = Vec::new();
        let mut terms = Vec::new();
        let mut lead = Vec::new();
        let mut valid = true;
        loop {
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
            if name == "description" && term.is_none() {
                valid = false;
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
        let body = body.strip_prefix('\n').unwrap_or(body);
        let body = body.strip_suffix('\n').unwrap_or(body);
        Some(Block::CodeBlock(attr, body.to_owned()))
    }

    fn figure(&mut self, name: &str) -> R<Block> {
        let placement = self.bracket_tokens().map(|tokens| tokens_text(&tokens));
        self.floats.push(Float {
            figure: true,
            ..Float::default()
        });
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
        Ok(Block::Figure(attr, caption, plain_paragraphs(content)))
    }

    fn table_float(&mut self, name: &str) -> R<Vec<Block>> {
        self.bracket_tokens();
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

    /// A tabular environment, read in place up to its `\end`.
    fn tabular(&mut self) -> R<Block> {
        self.bracket_tokens();
        let spec = self.argument_tokens().unwrap_or_default();
        let aligns = column_alignments(&spec);
        let mut rows = Vec::new();
        let mut head_rows = None;
        loop {
            if self.skip_rules()? && !rows.is_empty() && head_rows.is_none() {
                head_rows = Some(rows.len());
            }
            if self.peek_raw().is_none_or(|tok| tok.is_word("end")) {
                break;
            }
            let before = self.bumps;
            let mut cells = Vec::new();
            loop {
                cells.push(self.cell()?);
                match self.peek_raw().map(|tok| &tok.kind) {
                    Some(Kind::Tab) => {
                        self.bump();
                    }
                    Some(Kind::Symbol('\\')) => {
                        self.bump();
                        self.star();
                        self.bracket_tokens();
                        break;
                    }
                    _ => break,
                }
            }
            if self.bumps == before {
                // A token that ends a cell but no row, such as a stray `}`.
                self.bump();
                continue;
            }
            rows.push(Row {
                attr: Attr::default(),
                cells,
            });
        }
        let head = match head_rows {
            Some(count) if count < rows.len() => rows.drain(..count).collect(),
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

    /// Skips the spaces and the rules between rows. Returns true when a
    /// rule was there.
    fn skip_rules(&mut self) -> R<bool> {
        let mut rule = false;
        loop {
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
                    self.argument_tokens();
                    rule = true;
                }
                _ => return Ok(rule),
            }
        }
    }

    /// One cell, up to `&`, `\\` or `\end`.
    fn cell(&mut self) -> R<Cell> {
        self.skip_spaces();
        let mut cell = Cell::default();
        let mut inlines = Inlines::default();
        if self.peek_kind()? == Some(Kind::Word("multicolumn".into())) {
            self.bump();
            let span = self.argument_text();
            let spec = self.argument_tokens().unwrap_or_default();
            cell.col_span = span.trim().parse().unwrap_or(1);
            cell.align = column_alignments(&spec)
                .first()
                .copied()
                .unwrap_or(Alignment::AlignDefault);
            inlines.append(Inlines(self.argument_inlines()?));
        }
        inlines.append(self.nested(|parser| parser.inlines(Until::Cell))?);
        let inlines = inlines.trimmed();
        if !inlines.is_empty() {
            cell.content = vec![Block::Plain(inlines)];
        }
        Ok(cell)
    }

    // Inlines.

    fn inlines(&mut self, until: Until) -> R<Inlines> {
        let mut out = Inlines::default();
        while let Some(kind) = self.peek_kind()? {
            match (until, &kind) {
                (Until::Close | Until::Quote(_) | Until::Bracket | Until::Cell, Kind::Close) => {
                    break;
                }
                (Until::Bracket, Kind::Other(']')) => break,
                (Until::Cell, Kind::Tab | Kind::Symbol('\\')) => break,
                (Until::Cell, Kind::Word(name)) if name == "end" => break,
                (Until::Paragraph, Kind::Close) if self.depth == 0 => {
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
            self.inline(&mut out, until)?;
        }
        Ok(out)
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
            "end" | "item" | "part" | "chapter" | "section" | "subsection" | "subsubsection"
            | "paragraph" | "subparagraph" | "title" | "author" | "date" | "maketitle" => true,
            _ => false,
        }
    }

    fn inline(&mut self, out: &mut Inlines, until: Until) -> R<()> {
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
                let inner = self.nested(|parser| parser.inlines(Until::Close))?;
                if self.peek_raw().is_some_and(|tok| tok.kind == Kind::Close) {
                    self.bump();
                }
                if !inner.0.is_empty() {
                    out.push(Inline::Span(Attr::default(), inner.0));
                }
            }
            Kind::Dollar => self.dollar_math(out)?,
            Kind::Tab => out.push_str("&"),
            Kind::Param(_) => out.push_str("#"),
            Kind::Sup => out.push_str("^"),
            Kind::Sub => out.push_str("_"),
            Kind::Other(c) => self.punctuation(c, out, until)?,
            Kind::Symbol(c) => self.symbol(c, out)?,
            Kind::Verb(text) => out.push(Inline::Code(Attr::default(), text)),
            Kind::Verbatim(text) => out.push(Inline::Code(Attr::default(), text)),
            Kind::Word(name) if self.is_active_definition(&name) => {
                self.definition(&name, SourcePos::new("", tok.line, tok.column))?;
            }
            Kind::Word(name) => {
                self.nested(|parser| parser.command(&name, &tok.text, tok.depth, out, until))?
            }
        }
        Ok(())
    }

    fn punctuation(&mut self, c: char, out: &mut Inlines, until: Until) -> R<()> {
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
            ' ' | '\n' | '\t' => out.push_str(NBSP),
            '\\' => {
                self.skip_spaces();
                self.bracket_tokens();
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
            '\'' | '`' | '^' | '"' | '~' | '=' | '.' => self.accent(c, out),
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
        loop {
            self.expand()?;
            let Some(tok) = self.bump() else {
                break;
            };
            if closes(&tok.kind) {
                break;
            }
            text.push_str(&tok.text);
        }
        Ok(text.trim().to_owned())
    }

    fn accent(&mut self, accent: char, out: &mut Inlines) {
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
            _ => '\u{331}',
        };
        self.skip_spaces();
        let grouped = self.peek_raw().is_some_and(|tok| tok.kind == Kind::Open);
        let tokens = self.argument_tokens().unwrap_or_default();
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
        let Some(base) = chars.next() else {
            return;
        };
        out.push_str(&nfc(&format!("{base}{combining}")));
        out.push_str(chars.as_str());
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
        let is_def = self.peek_raw().is_some_and(|tok| tok.is_word("def"));
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
        loop {
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
        let rest = self.inlines(if until == Until::Paragraph {
            Until::Paragraph
        } else {
            Until::Close
        })?;
        out.push(wrap(rest.0));
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
            || name == "enquote";
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
            _ => None,
        };
        if let Some(wrap) = declaration {
            return self.declaration(until, out, wrap);
        }
        match name {
            "texttt" => {
                let content = self.argument_inlines_untrimmed()?;
                out.append(code_inlines(content));
            }
            "mbox" | "hbox" if self.raw_tex() => self.unknown_inline(source, out)?,
            "mbox" | "hbox" => {
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
            "footnote" => {
                self.bracket_tokens();
                let content = self.argument_blocks()?;
                out.push(Inline::Note(content));
            }
            "url" => {
                let url = unescape(&self.argument_text());
                out.push(Inline::Link(
                    Attr::new("", vec!["uri".into()], vec![]),
                    vec![Inline::Str(url.clone())],
                    Target::new(url, ""),
                ));
            }
            "href" => {
                let url = unescape(&self.argument_text());
                let text = self.argument_inlines_untrimmed()?;
                out.push(Inline::Link(Attr::default(), text, Target::new(url, "")));
            }
            "label" => {
                let label = self.argument_text();
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
            "caption" => {
                let short = self.bracket_inlines()?;
                let long = self.argument_inlines()?;
                if let Some(float) = self.floats.last_mut() {
                    float.caption = Some((short, long));
                }
            }
            "ref" | "eqref" | "autoref" | "cref" | "Cref" => {
                let label = self.argument_text();
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
                let path = self.argument_text();
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
                let in_figure = self.floats.last().is_some_and(|float| float.figure);
                let alt = if in_figure {
                    Vec::new()
                } else {
                    vec![Inline::Str("image".into())]
                };
                out.push(Inline::Image(attr, alt, Target::new(path, "")));
            }
            "centering" | "noindent" if !self.raw_tex() => self.skip_group(),
            "par" if !self.raw_tex() => {
                self.skip_word_spaces();
            }
            "c" | "v" | "u" | "H" | "r" | "k" | "d" | "b" => {
                let accent = name.chars().next().unwrap_or('b');
                self.accent(accent, out);
            }
            "verb" => {}
            "input" | "include" => self.include(depth)?,
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
            "enquote" => {
                let single =
                    self.star() != (self.quote_kinds.last() == Some(&QuoteType::DoubleQuote));
                let kind = if single {
                    QuoteType::SingleQuote
                } else {
                    QuoteType::DoubleQuote
                };
                self.quote_kinds.push(kind);
                let content = self.argument_inlines_untrimmed();
                self.quote_kinds.pop();
                out.push(Inline::Quoted(kind, content?));
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
            "char" => self.char_code(),
            "footcite" => {
                let mut cite = Inlines::default();
                self.citation(source, CitationMode::NormalCitation, true, &mut cite)?;
                cite.push_str(".");
                out.push(Inline::Note(vec![Block::Para(cite.0)]));
            }
            "begin" => {
                let environment = self.argument_text();
                if is_inline_environment(&environment) {
                    self.inline_environment(&environment, out)?;
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
        let (line, column) = (tok.line, tok.column);
        let shown = match &tok.kind {
            Kind::Word(name) => format!("CtrlSeq \"{name}\""),
            _ => format!("Symbol \"{}\"", tok.text),
        };
        let source_line = self
            .input
            .lines()
            .nth(usize::try_from(line).unwrap_or(1).saturating_sub(1))
            .unwrap_or_default();
        let caret = " ".repeat(usize::try_from(column).unwrap_or(1).saturating_sub(1));
        Stop::Unexpected(format!(
            "Error at (line {line}, column {column}):\nunexpected Tok (line {line}, column {column}) {shown}\nexpecting end of input\n{source_line}\n{caret}^"
        ))
    }

    fn raw_tex(&self) -> bool {
        self.extensions.contains(Extension::RawTex)
    }

    fn unknown_inline(&mut self, source: &str, out: &mut Inlines) -> R<()> {
        self.start_record();
        self.skip_word_spaces();
        self.star();
        loop {
            match self.peek_raw().map(|tok| &tok.kind) {
                Some(Kind::Other('[')) => {
                    self.bracket_tokens();
                }
                Some(Kind::Open) => {
                    self.bump();
                    self.tokens_until_close();
                }
                _ => break,
            }
        }
        let rest = self.stop_record();
        if self.raw_tex() {
            out.push(Inline::RawInline(
                Format::new("latex"),
                format!("{source}{rest}"),
            ));
        }
        Ok(())
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
        self.environment(name, &mut blocks)?;
        for block in blocks {
            if let Block::Para(inlines) | Block::Plain(inlines) = block {
                out.push(Inline::Space);
                out.append(Inlines(inlines));
            }
        }
        Ok(())
    }

    fn inline_environment(&mut self, name: &str, out: &mut Inlines) -> R<()> {
        let body = self.environment_body_tokens(name);
        let text = tokens_text(&body);
        let text = text.trim();
        let math = match name {
            "math" => Inline::Math(MathType::InlineMath, text.to_owned()),
            "displaymath" => Inline::Math(MathType::DisplayMath, text.to_owned()),
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
        self.start_record();
        self.skip_word_spaces();
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
        let keys = self.argument_text();
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
        self.start_record();
        self.skip_word_spaces();
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
        while self.key_group_ahead() {
            self.skip_spaces();
            let options = self.bracket_inlines()?;
            self.skip_spaces();
            let options = match options {
                Some(first) => (Some(first), self.bracket_inlines()?),
                None => (None, None),
            };
            let keys = self.argument_text();
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
            mode,
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

fn text_chars(text: &str) -> std::str::Chars<'_> {
    text.chars()
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
    for unit in ["\\textwidth", "\\linewidth", "\\columnwidth"] {
        if let Some(fraction) = value
            .strip_suffix(unit)
            .and_then(|number| number.trim().parse::<f64>().ok())
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

/// `\texttt`: text becomes code, other elements keep code inside.
fn code_inlines(content: Vec<Inline>) -> Inlines {
    let mut out = Inlines::default();
    let mut code = Text::new();
    let flush = |code: &mut Text, out: &mut Inlines| {
        if !code.is_empty() {
            out.push(Inline::Code(Attr::default(), std::mem::take(code)));
        }
    };
    for inline in content {
        match inline {
            Inline::Str(text) => code.push_str(&text),
            Inline::Space | Inline::SoftBreak => code.push(' '),
            Inline::Emph(inner) => {
                flush(&mut code, &mut out);
                out.push(Inline::Emph(code_inlines(inner).0));
            }
            Inline::Strong(inner) => {
                flush(&mut code, &mut out);
                out.push(Inline::Strong(code_inlines(inner).0));
            }
            Inline::Span(attr, inner) => {
                flush(&mut code, &mut out);
                out.push(Inline::Span(attr, code_inlines(inner).0));
            }
            other => {
                flush(&mut code, &mut out);
                out.push(other);
            }
        }
    }
    flush(&mut code, &mut out);
    out
}

/// The options of `lstlisting` (`[language=X]`) and of `minted` (`{X}`) at
/// the start of the verbatim body.
fn code_attributes<'a>(name: &str, body: &'a str) -> (Attr, &'a str) {
    let mut attr = Attr::default();
    let (open, close) = match name {
        "lstlisting" => ('[', ']'),
        "minted" => ('{', '}'),
        _ => return (attr, body),
    };
    let Some(rest) = body.strip_prefix(open) else {
        return (attr, body);
    };
    let Some((options, rest)) = rest.split_once(close) else {
        return (attr, body);
    };
    if name == "minted" {
        attr.classes.push(options.trim().to_owned());
        return (attr, rest);
    }
    for option in options.split(',') {
        if let Some((key, value)) = option.split_once('=') {
            let (key, value) = (key.trim(), value.trim());
            if key == "language" {
                attr.classes.push(value.to_lowercase());
            }
            attr.attributes.push((key.to_owned(), value.to_owned()));
        }
    }
    (attr, rest)
}

fn plain_paragraphs(blocks: Vec<Block>) -> Vec<Block> {
    blocks
        .into_iter()
        .map(|block| match block {
            Block::Para(inlines) => Block::Plain(inlines),
            block => block,
        })
        .collect()
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
        _ => return None,
    })
}

/// The number style and the delimiter of an `enumerate` label such as `(a)`.
fn list_label(label: &str) -> (ListNumberStyle, ListNumberDelim) {
    let label = label.trim();
    let style = label.chars().find_map(|c| match c {
        '1' => Some(ListNumberStyle::Decimal),
        'a' => Some(ListNumberStyle::LowerAlpha),
        'A' => Some(ListNumberStyle::UpperAlpha),
        'i' => Some(ListNumberStyle::LowerRoman),
        'I' => Some(ListNumberStyle::UpperRoman),
        _ => None,
    });
    let delim = if label.starts_with('(') && label.ends_with(')') {
        ListNumberDelim::TwoParens
    } else if label.ends_with(')') {
        ListNumberDelim::OneParen
    } else if label.ends_with('.') {
        ListNumberDelim::Period
    } else {
        ListNumberDelim::DefaultDelim
    };
    match style {
        Some(style) => (style, delim),
        None => (ListNumberStyle::DefaultStyle, ListNumberDelim::DefaultDelim),
    }
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

fn is_definition(name: &str) -> bool {
    matches!(
        name,
        "newcommand"
            | "renewcommand"
            | "providecommand"
            | "def"
            | "let"
            | "newenvironment"
            | "renewenvironment"
    )
}

fn is_tabular(name: &str) -> bool {
    matches!(name, "tabular" | "tabular*" | "tabularx" | "longtable")
}

fn is_inline_environment(name: &str) -> bool {
    matches!(
        name,
        "math"
            | "displaymath"
            | "equation"
            | "equation*"
            | "align"
            | "align*"
            | "gather"
            | "gather*"
            | "multline"
            | "multline*"
            | "eqnarray"
            | "eqnarray*"
            | "flalign"
            | "flalign*"
            | "alignat"
            | "alignat*"
    )
}

fn is_known_environment(name: &str) -> bool {
    matches!(
        name,
        "document"
            | "abstract"
            | "itemize"
            | "enumerate"
            | "description"
            | "quote"
            | "quotation"
            | "verse"
            | "center"
            | "minipage"
            | "verbatim"
            | "Verbatim"
            | "lstlisting"
            | "minted"
            | "comment"
            | "figure"
            | "figure*"
            | "table"
            | "table*"
            | "tabular"
            | "tabular*"
            | "tabularx"
            | "longtable"
    ) || is_inline_environment(name)
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
            | "mbox"
            | "hbox"
            | "LaTeX"
            | "TeX"
            | "ldots"
            | "dots"
            | "footnote"
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
    ) && letter_symbol(name).is_none()
        && citation_mode(name).is_none()
        && multi_citation_mode(name).is_none()
}
