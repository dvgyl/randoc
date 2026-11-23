//! LaTeX document command and environment signatures.

use super::*;

const MAX_ARGUMENTS: usize = 9;

#[derive(Debug)]
pub(super) struct Signature(Vec<Parameter>);

#[derive(Debug)]
struct Parameter {
    kind: ParameterKind,
    long: bool,
    adjacent: bool,
    processors: Vec<Processor>,
}

#[derive(Debug)]
enum ParameterKind {
    Mandatory,
    Delimited(Kind, Kind, Option<Vec<Tok>>),
    Test(Kind),
    Verbatim,
    Body(bool),
    Embellishments(Vec<(Kind, Option<Vec<Tok>>)>),
}

#[derive(Debug)]
enum Processor {
    Trim,
    Reverse,
    Split(Kind, Option<usize>),
}

#[derive(Clone, Copy)]
pub(super) enum Definition {
    Command(Redefinition),
    Environment(Redefinition),
    CopyCommand,
    CopyEnvironment,
}

#[derive(Clone, Copy)]
pub(super) enum Redefinition {
    New,
    Provide,
    Replace,
}

pub(super) fn definition_kind(name: &str) -> Option<Definition> {
    use Definition::*;
    use Redefinition::*;
    Some(match name {
        "NewDocumentCommand" | "NewExpandableDocumentCommand" => Command(New),
        "ProvideDocumentCommand" | "ProvideExpandableDocumentCommand" => Command(Provide),
        "RenewDocumentCommand"
        | "DeclareDocumentCommand"
        | "RenewExpandableDocumentCommand"
        | "DeclareExpandableDocumentCommand" => Command(Replace),
        "NewDocumentEnvironment" => Environment(New),
        "ProvideDocumentEnvironment" => Environment(Provide),
        "RenewDocumentEnvironment" | "DeclareDocumentEnvironment" => Environment(Replace),
        "NewCommandCopy" | "RenewCommandCopy" | "DeclareCommandCopy" => CopyCommand,
        "NewEnvironmentCopy" | "RenewEnvironmentCopy" | "DeclareEnvironmentCopy" => CopyEnvironment,
        _ => return None,
    })
}

impl Signature {
    fn read(tokens: Vec<Tok>) -> Option<Self> {
        let mut tokens = spec_tokens(tokens).into_iter().peekable();
        let mut signature = Vec::new();
        let mut count = 0;
        let mut long = false;
        let mut adjacent = false;
        let mut processors = Vec::new();
        while let Some(token) = tokens.next() {
            if blank(&token) {
                continue;
            }
            let kind = match token.text.as_str() {
                "+" => {
                    long = true;
                    continue;
                }
                "!" => {
                    adjacent = true;
                    continue;
                }
                "=" => {
                    spec_argument(&mut tokens)?;
                    continue;
                }
                ">" => {
                    if let Some(processor) = Processor::read(spec_argument(&mut tokens)?) {
                        if processors.len() == MAX_OPTION_TOKENS {
                            return None;
                        }
                        processors.push(processor);
                    }
                    continue;
                }
                "m" => ParameterKind::Mandatory,
                "o" | "O" => ParameterKind::Delimited(
                    Kind::Other('['),
                    Kind::Other(']'),
                    if token.text == "O" {
                        Some(spec_argument(&mut tokens)?)
                    } else {
                        None
                    },
                ),
                "s" => ParameterKind::Test(Kind::Other('*')),
                "t" => ParameterKind::Test(tokens.next()?.kind),
                "d" | "D" | "r" | "R" => ParameterKind::Delimited(
                    tokens.next()?.kind,
                    tokens.next()?.kind,
                    if matches!(token.text.as_str(), "D" | "R") {
                        Some(spec_argument(&mut tokens)?)
                    } else {
                        None
                    },
                ),
                "v" => ParameterKind::Verbatim,
                "b" | "c" => ParameterKind::Body(token.text == "c"),
                "e" | "E" => {
                    let markers = spec_tokens(spec_argument(&mut tokens)?);
                    let mut defaults = if token.text == "E" {
                        spec_argument(&mut tokens)?
                    } else {
                        Vec::new()
                    }
                    .into_iter();
                    let entries = markers
                        .into_iter()
                        .filter(|token| !blank(token))
                        .take(MAX_ARGUMENTS + 1)
                        .map(|token| (token.kind, spec_argument(&mut defaults)))
                        .collect::<Vec<_>>();
                    count += entries.len().saturating_sub(1);
                    ParameterKind::Embellishments(entries)
                }
                _ => return None,
            };
            count += 1;
            if count > MAX_ARGUMENTS {
                return None;
            }
            signature.push(Parameter {
                kind,
                long: std::mem::take(&mut long),
                adjacent: std::mem::take(&mut adjacent),
                processors: std::mem::take(&mut processors),
            });
        }
        Some(Self(signature))
    }
}

/// Split letter runs between specifiers. Keep tokens inside groups intact.
fn spec_tokens(tokens: Vec<Tok>) -> Vec<Tok> {
    let mut level = 0usize;
    let mut out = Vec::new();
    for token in tokens {
        match &token.kind {
            Kind::Open => level += 1,
            Kind::Close => level = level.saturating_sub(1),
            Kind::Letters(text) if level == 0 => {
                // A parameter uses at most three letters. Keep one extra
                // letter so the parser can reject a longer run.
                out.extend(text.chars().take(3 * MAX_ARGUMENTS + 1).map(|ch| Tok {
                    kind: Kind::Letters(ch.to_string()),
                    text: ch.to_string(),
                    line: token.line,
                    column: token.column,
                    depth: token.depth,
                    frozen: token.frozen,
                }));
                continue;
            }
            _ => {}
        }
        out.push(token);
    }
    out
}

fn spec_argument(tokens: &mut impl Iterator<Item = Tok>) -> Option<Vec<Tok>> {
    let token = tokens.find(|tok| !blank(tok))?;
    if token.kind != Kind::Open {
        return Some(vec![token]);
    }
    let mut level = 0usize;
    let mut out = Vec::new();
    for token in tokens {
        match token.kind {
            Kind::Open => level += 1,
            Kind::Close if level == 0 => return Some(out),
            Kind::Close => level -= 1,
            _ => {}
        }
        out.push(token);
    }
    None
}

impl Processor {
    fn read(tokens: Vec<Tok>) -> Option<Self> {
        let mut tokens = tokens.into_iter().filter(|tok| !blank(tok));
        let Kind::Word(word) = tokens.next()?.kind else {
            return None;
        };
        match word.as_str() {
            "TrimSpaces" => Some(Self::Trim),
            "ReverseBoolean" => Some(Self::Reverse),
            "SplitList" | "SplitArgument" => {
                let count = if word == "SplitArgument" {
                    Some(
                        tokens_text(&spec_argument(&mut tokens)?)
                            .parse::<usize>()
                            .ok()?
                            .checked_add(1)?
                            .min(MAX_OPTION_TOKENS),
                    )
                } else {
                    None
                };
                let delimiter = spec_argument(&mut tokens)?;
                let [token] = delimiter.as_slice() else {
                    return None;
                };
                Some(Self::Split(token.kind.clone(), count))
            }
            _ => None,
        }
    }
}

fn blank(tok: &Tok) -> bool {
    matches!(
        tok.kind,
        Kind::Space | Kind::Newline | Kind::Comment | Kind::Par
    )
}

fn trim(tokens: &mut Vec<Tok>) {
    let end = tokens
        .iter()
        .rposition(|tok| !blank(tok))
        .map_or(0, |p| p + 1);
    tokens.truncate(end);
    let start = tokens.iter().take_while(|tok| blank(tok)).count();
    tokens.drain(..start);
}

enum Argument {
    Value(Vec<Tok>),
    Default(Vec<Tok>),
}

impl Argument {
    fn or_default(
        value: Option<Vec<Tok>>,
        default: &Option<Vec<Tok>>,
        parser: &Parser<'_>,
    ) -> Self {
        match (value, default) {
            (Some(value), _) => Self::Value(value),
            (None, Some(default)) => Self::Default(default.clone()),
            (None, None) => Self::Value(parser.no_value()),
        }
    }
}

impl Parser<'_> {
    pub(super) fn document_definition(&mut self, definition: Definition, pos: SourcePos) -> R<()> {
        let level = self.group_level;
        match definition {
            Definition::Command(mode) => {
                let name = self.defined_name();
                let spec = self.argument_tokens().unwrap_or_default();
                let body = self.argument_tokens().unwrap_or_default();
                if self.field.is_some() || name.is_empty() {
                    return Ok(());
                }
                if self.macros.contains_key(&name) && !matches!(mode, Redefinition::Replace) {
                    if matches!(mode, Redefinition::New) {
                        self.context
                            .report(LogMessage::MacroAlreadyDefined(format!("\\{name}"), pos));
                    }
                    return Ok(());
                }
                if let Some(signature) = Signature::read(spec) {
                    self.macros.define(
                        name,
                        Macro {
                            parameters: 0,
                            default: None,
                            signature: Some(Arc::new(signature)),
                            body: body.into(),
                            delimiters: None,
                        },
                        level,
                        false,
                    );
                }
            }
            Definition::Environment(mode) => {
                let name = self.argument_source_text().trim().to_owned();
                let spec = self.argument_tokens().unwrap_or_default();
                let begin = self.argument_tokens().unwrap_or_default();
                let end = self.argument_tokens().unwrap_or_default();
                if self.field.is_some()
                    || name.is_empty()
                    || (self.environments.contains_key(&name)
                        && !matches!(mode, Redefinition::Replace))
                {
                    return Ok(());
                }
                if let Some(signature) = Signature::read(spec) {
                    self.environments.define(
                        name,
                        EnvironmentMacro {
                            parameters: 0,
                            default: None,
                            signature: Some(Arc::new(signature)),
                            begin,
                            end,
                        },
                        level,
                        false,
                    );
                }
            }
            Definition::CopyCommand => {
                let name = self.defined_name();
                let target = self.defined_name();
                if self.field.is_none() && !name.is_empty() && !target.is_empty() {
                    let definition = self.macros.get(&target).cloned().unwrap_or_else(|| Macro {
                        parameters: 0,
                        default: None,
                        signature: None,
                        body: vec![self.word_token(&target)].into(),
                        delimiters: None,
                    });
                    self.macros.define(name, definition, level, false);
                }
            }
            Definition::CopyEnvironment => {
                let name = self.argument_source_text().trim().to_owned();
                let target = self.argument_source_text().trim().to_owned();
                if self.field.is_none() && !name.is_empty() && !target.is_empty() {
                    let definition =
                        self.environments.get(&target).cloned().unwrap_or_else(|| {
                            let mut begin = vec![self.word_token("begin")];
                            begin.extend(self.braced(vec![
                                self.synthetic(Kind::Letters(target.clone()), &target),
                            ]));
                            let mut end = vec![self.word_token("end")];
                            end.extend(begin[1..].iter().cloned());
                            EnvironmentMacro {
                                parameters: 0,
                                default: None,
                                signature: None,
                                begin,
                                end,
                            }
                        });
                    self.environments.define(name, definition, level, false);
                }
            }
        }
        Ok(())
    }

    pub(super) fn expand_document_command(
        &mut self,
        name: &str,
        depth: u32,
        definition: &Macro,
        signature: &Signature,
    ) -> R<()> {
        let recorded: Vec<usize> = self.recorders.iter().map(Text::len).collect();
        let origin = self.expansion_call.take();
        let call = origin.or_else(|| self.peek_raw().cloned());
        self.bump();
        self.skip_word_spaces();
        let arguments = self.document_arguments(signature, None)?;
        for (record, len) in self.recorders.iter_mut().zip(recorded) {
            record.truncate(len);
        }
        if let Some(arguments) = arguments {
            self.push_macro(name, depth, &definition.body, &arguments, call)?;
        }
        Ok(())
    }

    pub(super) fn document_environment_arguments(
        &mut self,
        signature: &Signature,
        name: &str,
    ) -> R<(Vec<Vec<Tok>>, Vec<Tok>)> {
        let arguments = self
            .document_arguments(signature, Some(name))?
            .unwrap_or_default();
        let body = if signature
            .0
            .iter()
            .any(|parameter| matches!(parameter.kind, ParameterKind::Body(_)))
        {
            Vec::new()
        } else {
            self.environment_body_tokens(name).0
        };
        Ok((arguments, body))
    }

    fn document_arguments(
        &mut self,
        signature: &Signature,
        environment: Option<&str>,
    ) -> R<Option<Vec<Vec<Tok>>>> {
        let mut entries = Vec::new();
        for parameter in &signature.0 {
            let argument =
                match &parameter.kind {
                    ParameterKind::Mandatory => {
                        if parameter.long {
                            self.skip_blank();
                        }
                        let Some(tokens) = self.argument_tokens() else {
                            return Ok(None);
                        };
                        if !parameter.long && tokens.iter().any(|tok| tok.kind == Kind::Par) {
                            return Ok(None);
                        }
                        Argument::Value(tokens)
                    }
                    ParameterKind::Delimited(open, close, default) => {
                        let value = if self.take_marker(open, parameter.adjacent) {
                            self.document_delimited(open, close, parameter.long)
                        } else {
                            None
                        };
                        Argument::or_default(value, default, self)
                    }
                    ParameterKind::Test(marker) => {
                        let yes = self.take_marker(marker, parameter.adjacent);
                        Argument::Value(self.boolean(yes))
                    }
                    ParameterKind::Verbatim => Argument::Value(self.document_verbatim()),
                    ParameterKind::Body(verbatim) => {
                        let Some(name) = environment else {
                            return Ok(None);
                        };
                        let (mut body, _) = self.environment_body_tokens(name);
                        if *verbatim {
                            body = self.verbatim_body(&tokens_text(&body));
                        } else {
                            trim(&mut body);
                        }
                        Argument::Value(body)
                    }
                    ParameterKind::Embellishments(markers) => {
                        let mut values = vec![None; markers.len()];
                        loop {
                            let next = markers.iter().enumerate().position(|(index, (kind, _))| {
                                values[index].is_none()
                                    && self.marker_gap(kind, parameter.adjacent).is_some()
                            });
                            let Some(index) = next else {
                                break;
                            };
                            self.take_marker(&markers[index].0, parameter.adjacent);
                            values[index] = Some(self.argument_tokens().unwrap_or_default());
                        }
                        entries.extend(values.into_iter().zip(markers).map(
                            |(value, (_, default))| {
                                (
                                    Argument::or_default(value, default, self),
                                    &parameter.processors,
                                )
                            },
                        ));
                        continue;
                    }
                };
            entries.push((argument, &parameter.processors));
        }
        // A default can refer to another argument. Resolve each default once
        // against the arguments supplied by the caller.
        let mut arguments: Vec<_> = entries
            .iter_mut()
            .map(|(entry, _)| match entry {
                Argument::Value(value) => std::mem::take(value),
                Argument::Default(_) => Vec::new(),
            })
            .collect();
        for (index, (entry, processors)) in entries.into_iter().enumerate() {
            if let Argument::Default(default) = entry {
                arguments[index] = substitute(&default, &arguments, self.expansion_limit())
                    .ok_or_else(|| Stop::Loop("document argument".into()))?
                    .0;
            }
            for processor in processors.iter().rev() {
                self.process_argument(processor, &mut arguments[index])?;
            }
        }
        Ok(Some(arguments))
    }

    fn marker_gap(&self, marker: &Kind, adjacent: bool) -> Option<usize> {
        let max = if adjacent { 1 } else { MAX_OPTION_TOKENS };
        self.scan()
            .take(max)
            .enumerate()
            .find(|(_, tok)| !matches!(tok.kind, Kind::Space | Kind::Comment | Kind::Newline))
            .filter(|(_, tok)| {
                &tok.kind == marker
                    || matches!((&tok.kind, marker),
                        (Kind::Letters(run), Kind::Letters(letter)) if run.starts_with(letter)
                    )
            })
            .map(|(gap, _)| gap)
    }

    fn take_marker(&mut self, marker: &Kind, adjacent: bool) -> bool {
        let Some(gap) = self.marker_gap(marker, adjacent) else {
            return false;
        };
        for _ in 0..gap {
            self.bump();
        }
        if matches!(marker, Kind::Letters(_)) {
            self.argument_tokens();
        } else {
            self.bump();
        }
        true
    }

    fn document_delimited(&mut self, open: &Kind, close: &Kind, long: bool) -> Option<Vec<Tok>> {
        let mut braces = 0usize;
        let mut delimiters = 0usize;
        let mut out = Vec::new();
        while let Some(token) = self.peek_raw() {
            match token.kind {
                Kind::Par if !long => return None,
                Kind::Open => braces += 1,
                Kind::Close if braces == 0 => return None,
                Kind::Close => braces -= 1,
                _ if braces == 0 && &token.kind == close => {
                    if delimiters == 0 {
                        self.bump();
                        return Some(out);
                    }
                    delimiters -= 1;
                }
                _ if braces == 0 && &token.kind == open => delimiters += 1,
                _ => {}
            }
            out.extend(self.bump());
        }
        None
    }

    fn document_verbatim(&mut self) -> Vec<Tok> {
        self.skip_spaces();
        let Some(start) = self.peek_raw().cloned() else {
            return Vec::new();
        };
        let text = if start.kind == Kind::Open {
            self.argument_source_text()
        } else {
            let Some(delimiter) = start.text.chars().next() else {
                return Vec::new();
            };
            let mut text = Text::new();
            // The closing character can be inside a token, for example a
            // comment. Tokenize only the suffix that follows it.
            self.bump();
            let mut part = start.text[delimiter.len_utf8()..].to_owned();
            loop {
                if let Some((body, tail)) = part.split_once(delimiter) {
                    text.push_str(body);
                    self.pending.extend(tokenize(tail).into_iter().rev());
                    break;
                }
                text.push_str(&part);
                if self
                    .peek_raw()
                    .is_none_or(|tok| matches!(tok.kind, Kind::Newline | Kind::Par))
                {
                    break;
                }
                let Some(token) = self.bump() else { break };
                part = token.text;
            }
            text
        };
        vec![self.synthetic(Kind::Letters(text.clone()), &text)]
    }

    fn verbatim_body(&self, body: &str) -> Vec<Tok> {
        let mut tokens = Vec::new();
        for line in body.trim_matches(['\n', '\r']).lines() {
            if !tokens.is_empty() {
                tokens.push(self.synthetic(Kind::Symbol('\\'), "\\\\"));
            }
            let text = line.replace(' ', "␣");
            tokens.push(self.synthetic(Kind::Verb(text.clone()), &text));
        }
        tokens
    }

    fn process_argument(&self, processor: &Processor, argument: &mut Vec<Tok>) -> R<()> {
        match processor {
            Processor::Trim => trim(argument),
            Processor::Reverse => {
                if let Some(value) = boolean_value(argument) {
                    *argument = self.boolean(!value);
                }
            }
            Processor::Split(delimiter, count) => {
                let mut parts = vec![Vec::new()];
                let mut remaining = count.unwrap_or(usize::MAX).saturating_sub(1);
                let mut level = 0usize;
                for tok in std::mem::take(argument) {
                    match tok.kind {
                        Kind::Open => level += 1,
                        Kind::Close => level = level.saturating_sub(1),
                        _ => {}
                    }
                    if level == 0 && &tok.kind == delimiter && remaining > 0 {
                        parts.push(Vec::new());
                        remaining -= 1;
                    } else if let Some(part) = parts.last_mut() {
                        part.push(tok);
                    }
                }
                for part in &mut parts {
                    trim(part);
                }
                if let Some(count) = count {
                    parts.resize_with(*count, || self.no_value());
                }
                if parts.iter().map(|part| token_cost(part) + 2).sum::<usize>()
                    > self.expansion_limit()
                {
                    return Err(Stop::Loop("document argument".into()));
                }
                argument.extend(parts.into_iter().flat_map(|part| self.braced(part)));
            }
        }
        Ok(())
    }

    fn word_token(&self, word: &str) -> Tok {
        self.synthetic(Kind::Word(word.into()), &format!("\\{word}"))
    }

    fn boolean(&self, value: bool) -> Vec<Tok> {
        vec![self.word_token(if value { "BooleanTrue" } else { "BooleanFalse" })]
    }

    fn no_value(&self) -> Vec<Tok> {
        vec![self.synthetic(Kind::Letters("-NoValue-".into()), "-NoValue-")]
    }

    fn braced(&self, mut body: Vec<Tok>) -> Vec<Tok> {
        body.insert(0, self.synthetic(Kind::Open, "{"));
        body.push(self.synthetic(Kind::Close, "}"));
        body
    }

    pub(super) fn expand_document_helper(&mut self, name: &str, depth: u32) -> R<bool> {
        let (condition, true_branch, false_branch) = match name {
            "IfNoValueTF" => ("NoValue", true, true),
            "IfNoValueT" => ("NoValue", true, false),
            "IfNoValueF" => ("NoValue", false, true),
            "IfValueTF" => ("Value", true, true),
            "IfValueT" => ("Value", true, false),
            "IfValueF" => ("Value", false, true),
            "IfBooleanTF" => ("Boolean", true, true),
            "IfBooleanT" => ("Boolean", true, false),
            "IfBooleanF" => ("Boolean", false, true),
            "IfBlankTF" => ("Blank", true, true),
            "IfBlankT" => ("Blank", true, false),
            "IfBlankF" => ("Blank", false, true),
            _ => return self.expand_document_utility(name, depth),
        };
        self.bump();
        let test = self.argument_tokens().unwrap_or_default();
        let answer = match condition {
            "Boolean" => boolean_value(&test).unwrap_or(false),
            "Blank" => test.iter().all(blank),
            _ => (tokens_text(&test).trim() == "-NoValue-") == (condition == "NoValue"),
        };
        let yes = true_branch.then(|| self.argument_tokens().unwrap_or_default());
        let no = false_branch.then(|| self.argument_tokens().unwrap_or_default());
        let out = if answer { yes } else { no }.unwrap_or_default();
        self.push_tokens(name, depth, out)?;
        Ok(true)
    }

    fn expand_document_utility(&mut self, name: &str, depth: u32) -> R<bool> {
        match name {
            "ShowCommand" => {
                self.bump();
                self.argument_tokens();
            }
            "ifstrequal" => {
                self.bump();
                let left = self.argument_source_text();
                let right = self.argument_source_text();
                let yes = self.argument_tokens().unwrap_or_default();
                let no = self.argument_tokens().unwrap_or_default();
                self.push_tokens(name, depth, if left == right { yes } else { no })?;
            }
            "BooleanTrue" | "BooleanFalse" => {
                self.bump();
                self.skip_word_spaces();
            }
            "UseName" => {
                self.bump();
                let target = self.nested(Self::argument_text)?;
                self.push_tokens(name, depth, vec![self.word_token(target.trim())])?;
            }
            "ExpandArgs" => {
                self.bump();
                let spec = self.argument_source_text();
                let mut out = self.argument_tokens().unwrap_or_default();
                for kind in spec.chars().take(MAX_ARGUMENTS) {
                    match kind {
                        'c' => {
                            let target = self.nested(Self::argument_text)?;
                            out.push(self.word_token(target.trim()));
                        }
                        'N' => out.extend(self.argument_tokens().unwrap_or_default()),
                        _ => break,
                    }
                }
                self.push_tokens(name, depth, out)?;
            }
            "ProcessList" => {
                self.bump();
                let list = self.argument_tokens().unwrap_or_default();
                let command = self.argument_tokens().unwrap_or_default();
                let mut items = list.into_iter();
                let limit = self.expansion_limit();
                let mut out = Vec::new();
                let mut cost = 0;
                let command_cost = token_cost(&command);
                while let Some(item) = spec_argument(&mut items) {
                    cost += command_cost + token_cost(&item) + 2;
                    if cost > limit {
                        return Err(Stop::Loop(name.into()));
                    }
                    out.extend(command.iter().cloned());
                    out.extend(self.braced(item));
                }
                self.push_tokens(name, depth, out)?;
            }
            "inteval" | "fpeval" => {
                self.bump();
                let input = self.nested(Self::argument_text)?;
                let result = super::eval::evaluate(input.trim(), name == "inteval");
                let out = if let Some(result) = result {
                    vec![self.synthetic(Kind::Letters(result.clone()), &result)]
                } else {
                    tokenize(&input)
                };
                self.push_tokens(name, depth, out)?;
            }
            "dimeval" | "skipeval" => {
                self.bump();
                let out = self.argument_tokens().unwrap_or_default();
                self.push_tokens(name, depth, out)?;
            }
            "expandableinput" if !self.raw_tex() => {
                self.bump();
                self.nested(|parser| parser.include(depth))?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

fn boolean_value(tokens: &[Tok]) -> Option<bool> {
    let mut tokens = tokens.iter().filter(|tok| !blank(tok));
    let token = tokens.next()?;
    if tokens.next().is_some() {
        return None;
    }
    match &token.kind {
        Kind::Word(word) if word == "BooleanTrue" => Some(true),
        Kind::Word(word) if word == "BooleanFalse" => Some(false),
        _ => None,
    }
}
