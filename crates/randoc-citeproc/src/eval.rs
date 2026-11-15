//! The evaluation of one citation: a port of the parts of `Citeproc.Eval`
//! (`citeproc` 0.14.1.1) that need no sorting, no disambiguation and no
//! cite positions: `evalLayout` and `evalItem` for one citation, `text`
//! (variables, values, terms and macros), `group` with its suppression
//! rule, `choose`, page ranges and term lookup.
//!
//! `number` and `label` are ported too. The elements `names` and `date`
//! are not ported yet: they give [`EvalError::NotPorted`], so no output is
//! silently wrong.

use std::collections::BTreeSet;

use randoc_ast::Inline;

use crate::lang::{Lang, parse_lang};
use crate::markup::TooDeep;
use crate::output::{self, Output, Tag};
use crate::reference::{self, Reference, Val};
use crate::types::{
    Condition, Element, ElementType, Formatting, Locale, Match, NumberForm, PageRangeFormat,
    Pluralize, Style, Term, TermForm, TermGender, TermMatch, TermNumber, TextCase, TextType,
    Variable, VariableForm,
};
use crate::unicode_class::is_letter;

/// An evaluation that cannot go on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    /// The style or the output is nested deeper than the depth limit.
    TooDeep,
    /// The style uses an element that this port does not evaluate yet.
    NotPorted(&'static str),
}

impl From<TooDeep> for EvalError {
    fn from(_: TooDeep) -> EvalError {
        EvalError::TooDeep
    }
}

/// The result of one citation: its output tree and the warnings.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluated {
    /// The output of the citation.
    pub output: Output,
    /// The warnings, in code point order (upstream keeps a set).
    pub warnings: Vec<String>,
}

/// Evaluates one citation of the given items with the citation layout of
/// `style` (`evalLayout` with no item prefixes, suffixes or locators).
/// `locale` is the merged locale of the style. An item with no reference
/// gives the bold identifier and a question mark, with a warning.
pub fn eval_citation(
    style: &Style,
    locale: &Locale,
    items: &[(&str, Option<&Reference<Vec<Inline>>>)],
    max_depth: usize,
) -> Result<Evaluated, EvalError> {
    let mut warnings = BTreeSet::new();
    let mut outputs = Vec::with_capacity(items.len());
    for (id, reference) in items {
        let output = match reference {
            Some(reference) => {
                let mut eval = Eval {
                    style,
                    locale,
                    reference,
                    reference_lang: reference_lang(reference),
                    var_count: VarCount::default(),
                    warnings: &mut warnings,
                };
                let mut xs = eval.elements(&style.citation.elements, max_depth)?;
                if let Some(lang) = &eval.reference_lang {
                    for x in &mut xs {
                        add_lang(x, lang);
                    }
                }
                xs
            }
            None => {
                warnings.insert(format!("citation {id} not found"));
                vec![Output::Literal(output::add_font_weight(
                    crate::types::FontWeight::Bold,
                    output::from_text(&format!("{id}?")),
                ))]
            }
        };
        let item = output::formatted(Formatting::default(), output);
        outputs.push(match item {
            Output::Null => Output::Null,
            item => Output::Tagged(
                Tag::Item {
                    author_only: false,
                    id: (*id).to_owned(),
                },
                Box::new(item),
            ),
        });
    }
    Ok(Evaluated {
        output: output::formatted(style.citation.formatting.clone(), outputs),
        warnings: warnings.into_iter().collect(),
    })
}

/// `stateReferenceLang`: the `language` variable of the reference as a tag.
fn reference_lang(reference: &Reference<Vec<Inline>>) -> Option<Lang> {
    let text = val_to_text(
        reference
            .variables
            .get(&reference::Variable::new("language"))?,
    )?;
    parse_lang(&text).ok()
}

/// `addLangToFormatting` on each formatted part (`transform`).
fn add_lang(output: &mut Output, lang: &Lang) {
    let mut pending = vec![output];
    while let Some(output) = pending.pop() {
        match output {
            Output::Formatted(formatting, parts) => {
                formatting.lang = Some(lang.clone());
                pending.extend(parts.iter_mut());
            }
            Output::Linked(_, parts) => pending.extend(parts.iter_mut()),
            Output::InNote(inner) | Output::Tagged(_, inner) => pending.push(inner),
            Output::Literal(_) | Output::Null => {}
        }
    }
}

/// `VarCount`: the variables that a group calls, and how many are not empty.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct VarCount {
    accessed: usize,
    nonempty: usize,
}

struct Eval<'a, 'w> {
    style: &'a Style,
    locale: &'a Locale,
    reference: &'a Reference<Vec<Inline>>,
    reference_lang: Option<Lang>,
    var_count: VarCount,
    warnings: &'w mut BTreeSet<String>,
}

impl Eval<'_, '_> {
    fn update_var_count(&mut self, accessed: usize, nonempty: usize) {
        self.var_count.accessed += accessed;
        self.var_count.nonempty += nonempty;
    }

    fn elements(&mut self, elements: &[Element], depth: usize) -> Result<Vec<Output>, EvalError> {
        let mut out = Vec::new();
        for element in elements {
            out.extend(self.element(element, depth)?);
        }
        Ok(out)
    }

    /// `eElement`.
    fn element(&mut self, element: &Element, depth: usize) -> Result<Vec<Output>, EvalError> {
        let depth = depth.checked_sub(1).ok_or(EvalError::TooDeep)?;
        let formatting = &element.formatting;
        Ok(match &element.kind {
            ElementType::Text(TextType::Macro(name)) => match self.style.macros.get(name) {
                None => {
                    self.warnings.insert(format!("undefined macro {name}"));
                    Vec::new()
                }
                Some(elements) => vec![self.group(true, formatting, elements, depth)?],
            },
            ElementType::Text(text) => {
                let output = self.text(text, depth)?;
                vec![self.with_formatting(formatting, output)]
            }
            ElementType::Group(is_macro, elements) => {
                vec![self.group(*is_macro, formatting, elements, depth)?]
            }
            ElementType::Choose(branches) => {
                for branch in branches {
                    if self.matches(branch.matching, &branch.conditions) {
                        return self.elements(&branch.elements, depth);
                    }
                }
                Vec::new()
            }
            ElementType::Number(variable, form) => {
                let output = self.number(variable.as_str(), *form, depth)?;
                vec![self.with_formatting(formatting, output)]
            }
            ElementType::Label(variable, form, pluralize) => {
                vec![self.label(variable.as_str(), *form, *pluralize, formatting, depth)?]
            }
            ElementType::Date { .. } => return Err(EvalError::NotPorted("date")),
            ElementType::Names(..) => return Err(EvalError::NotPorted("names")),
        })
    }

    /// `eGroup`: a group that calls variables, all of them empty, gives nothing.
    fn group(
        &mut self,
        is_macro: bool,
        formatting: &Formatting,
        elements: &[Element],
        depth: usize,
    ) -> Result<Output, EvalError> {
        let before = self.var_count;
        let xs = self.elements(elements, depth)?;
        let after = self.var_count;
        let empty = after.accessed != before.accessed && after.nonempty == before.nonempty;
        if is_macro && !empty {
            self.update_var_count(1, 1);
        }
        Ok(if empty {
            Output::Null
        } else {
            output::formatted(formatting.clone(), xs)
        })
    }

    /// `withFormatting`: title case applies only to English items.
    fn with_formatting(&self, formatting: &Formatting, output: Output) -> Output {
        if *formatting == Formatting::default() {
            return output;
        }
        let is_english = match &self.reference_lang {
            Some(lang) => lang.language == "en",
            None => self
                .locale
                .language
                .as_ref()
                .is_some_and(|lang| lang.language == "en"),
        };
        let mut formatting = formatting.clone();
        if formatting.text_case == Some(TextCase::TitleCase) && !is_english {
            formatting.text_case = None;
        }
        output::formatted(formatting, vec![output])
    }

    /// `askVariable`: the value of a variable, counted for the group rule.
    fn ask_variable(&mut self, name: &str) -> Option<Val<Vec<Inline>>> {
        if name == "page-first" {
            let first = |text: &str| text.chars().take_while(|c| !is_sep_punct(*c)).collect();
            return match self.ask_variable("page")? {
                Val::Text(t) => Some(Val::Text(first(&t))),
                Val::Fancy(x) => Some(Val::Text(first(&output::stringify(&x)))),
                Val::Num(n) => Some(Val::Num(n)),
                Val::Names(_) | Val::Date(_) => None,
            };
        }
        let value = lookup_variable(self.reference, name);
        match value {
            Some(value) if is_nonempty(&value) => {
                self.update_var_count(1, 1);
                Some(value)
            }
            _ => {
                self.update_var_count(1, 0);
                None
            }
        }
    }

    /// `eText`.
    fn text(&mut self, text: &TextType, depth: usize) -> Result<Output, EvalError> {
        Ok(match text {
            TextType::Value(value) => {
                Output::Literal(output::parse_csl_json(self.locale, value, depth)?)
            }
            TextType::Term(term) => Output::Tagged(Tag::Other, Box::new(self.term(term, depth)?)),
            TextType::Macro(name) => {
                self.warnings.insert(format!("unexpanded macro {name}"));
                Output::Null
            }
            TextType::Variable(form, variable) => self.variable(*form, variable, depth)?,
        })
    }

    fn variable(
        &mut self,
        form: VariableForm,
        variable: &Variable,
        depth: usize,
    ) -> Result<Output, EvalError> {
        let name = variable.as_str();
        Ok(match name {
            "id" => {
                self.update_var_count(1, 1);
                Output::Literal(output::from_text(&self.reference.id))
            }
            "type" => {
                self.update_var_count(1, 1);
                Output::Literal(output::from_text(&self.reference.kind))
            }
            "locator" => {
                self.update_var_count(1, 0);
                Output::Null
            }
            "year-suffix" => Output::Null,
            "citation-number" | "citation-label" => {
                let value = self.ask_variable(name);
                match (name, value) {
                    ("citation-number", Some(Val::Num(n))) => Output::Tagged(
                        Tag::Other,
                        Box::new(Output::Literal(output::from_text(&n.to_string()))),
                    ),
                    ("citation-label", Some(Val::Text(t))) => {
                        Output::Tagged(Tag::Other, Box::new(Output::Literal(output::from_text(&t))))
                    }
                    ("citation-label", Some(Val::Fancy(x))) => {
                        Output::Tagged(Tag::Other, Box::new(Output::Literal(x)))
                    }
                    _ => {
                        let what = if name == "citation-number" {
                            "citation-number not defined for "
                        } else {
                            "citation-label of unknown type for "
                        };
                        self.warnings.insert(format!("{what}{}", self.reference.id));
                        Output::Null
                    }
                }
            }
            "DOI" | "PMCID" | "PMID" | "URL" => {
                let value = self.ask_variable(name);
                match value.as_ref().and_then(val_to_text) {
                    None => Output::Null,
                    Some(text) => Output::Linked(
                        identifier_url(name, &text),
                        vec![Output::Literal(output::from_text(&text))],
                    ),
                }
            }
            _ => {
                let value = if form == VariableForm::Short {
                    let short = self.ask_variable(&format!("{name}-short"));
                    let long = self.ask_variable(name);
                    short.or(long)
                } else {
                    self.ask_variable(name)
                };
                let result = match value {
                    Some(Val::Text(x)) if name == "page" => self.page_range(&x, depth)?,
                    Some(Val::Text(x)) => Output::Literal(output::from_text(&x)),
                    Some(Val::Fancy(x)) if name == "page" => {
                        self.page_range(&output::stringify(&x), depth)?
                    }
                    Some(Val::Fancy(x)) => Output::Literal(x),
                    Some(Val::Num(n)) => Output::Literal(output::from_text(&n.to_string())),
                    _ => Output::Null,
                };
                if name == "title" && result != Output::Null {
                    Output::Tagged(Tag::Other, Box::new(result))
                } else {
                    result
                }
            }
        })
    }

    /// `eNumber`: each number part of the value in the form.
    fn number(&mut self, name: &str, form: NumberForm, depth: usize) -> Result<Output, EvalError> {
        let value = self.ask_variable(name);
        let gender = self
            .lookup_term_entries(&term_named(name))
            .into_iter()
            .next()
            .and_then(|(term, _)| term.gender);
        let parts = match value {
            Some(Val::Num(n)) => vec![Val::Num(n)],
            Some(Val::Fancy(x)) => split_nums(&output::stringify(&x)),
            Some(Val::Text(t)) => split_nums(&t),
            _ => Vec::new(),
        };
        let mut outputs = Vec::with_capacity(parts.len());
        for part in parts {
            outputs.push(self.eval_number(form, gender, &part, depth)?);
        }
        Ok(output::formatted(Formatting::default(), outputs))
    }

    /// `evalNumber`.
    fn eval_number(
        &mut self,
        form: NumberForm,
        gender: Option<TermGender>,
        value: &Val<Vec<Inline>>,
        depth: usize,
    ) -> Result<Output, EvalError> {
        let i = match value {
            Val::Num(i) => *i,
            Val::Text(t) => return Ok(Output::Literal(output::from_text(t))),
            Val::Fancy(x) => return Ok(Output::Literal(x.clone())),
            Val::Names(_) | Val::Date(_) => return Ok(Output::Null),
        };
        let numbered = |name: String| Term {
            gender_form: gender,
            ..term_named(&name)
        };
        let decimal = i.to_string();
        let with_suffix = |this: &Self, suffix: &str| -> Result<Output, EvalError> {
            let suffix = output::parse_csl_json(this.locale, suffix, depth)?;
            Ok(Output::Literal(output::concat(vec![
                output::from_text(&decimal),
                suffix,
            ])))
        };
        match form {
            NumberForm::Numeric => Ok(Output::Literal(output::from_text(&decimal))),
            NumberForm::Roman => Ok(Output::Literal(output::from_text(&roman(i)))),
            NumberForm::LongOrdinal if (1..=10).contains(&i) => {
                match self
                    .lookup_term(&numbered(format!("long-ordinal-{i:02}")))
                    .into_iter()
                    .next()
                {
                    Some(text) => Ok(Output::Literal(output::from_text(&text))),
                    None => self.eval_number(NumberForm::Ordinal, gender, value, depth),
                }
            }
            NumberForm::LongOrdinal | NumberForm::Ordinal => {
                let two = numbered(format!("ordinal-{:02}", i.rem_euclid(100)));
                let found = self
                    .lookup_term_entries(&two)
                    .into_iter()
                    .find(|(term, _)| i <= 99 || term.matching != Some(TermMatch::WholeNumber));
                if let Some((_, suffix)) = found {
                    return with_suffix(self, &suffix);
                }
                let one = numbered(format!("ordinal-{:02}", i.rem_euclid(10)));
                let found = self
                    .lookup_term_entries(&one)
                    .into_iter()
                    .find(|(term, _)| {
                        i <= 10
                            || term.matching.is_none()
                            || term.matching == Some(TermMatch::LastDigit)
                    });
                if let Some((_, suffix)) = found {
                    return with_suffix(self, &suffix);
                }
                if let Some(suffix) = self.lookup_term(&term_named("ordinal")).into_iter().next() {
                    return with_suffix(self, &suffix);
                }
                self.warnings
                    .insert(format!("no ordinal suffix found for {decimal}"));
                Ok(Output::Literal(output::from_text(&decimal)))
            }
        }
    }

    /// `eLabel` with no locator: the term of the variable in the number
    /// that its value asks for.
    fn label(
        &mut self,
        name: &str,
        form: TermForm,
        pluralize: Pluralize,
        formatting: &Formatting,
        depth: usize,
    ) -> Result<Output, EvalError> {
        let lookup_name = if name == "editortranslator" {
            "editor"
        } else {
            name
        };
        let term = match lookup_variable(self.reference, lookup_name) {
            None => Output::Null,
            Some(value) => {
                let number = match pluralize {
                    Pluralize::Always => TermNumber::Plural,
                    Pluralize::Never => TermNumber::Singular,
                    Pluralize::Contextual => match &value {
                        Val::Text(t) => determine_plural(name, t),
                        Val::Num(n) => determine_plural(name, &n.to_string()),
                        Val::Fancy(x) => determine_plural(name, &output::stringify(x)),
                        Val::Names(names) if names.len() > 1 => TermNumber::Plural,
                        _ => TermNumber::Singular,
                    },
                };
                let term = Term {
                    form,
                    number: Some(number),
                    ..term_named(name)
                };
                self.term(&term, depth)?
            }
        };
        if let Some(suffix) = &formatting.suffix
            && suffix.starts_with('.')
            && let Output::Literal(x) = &term
            && output::stringify(x).ends_with('.')
            && !formatting.strip_periods
        {
            let mut formatting = formatting.clone();
            formatting.suffix = (suffix.chars().count() > 1).then(|| suffix[1..].to_owned());
            return Ok(output::formatted(formatting, vec![term]));
        }
        Ok(output::formatted(formatting.clone(), vec![term]))
    }

    /// `pageRange`.
    fn page_range(&self, text: &str, depth: usize) -> Result<Output, EvalError> {
        let delimiter = self
            .lookup_term(&term_named("page-range-delimiter"))
            .into_iter()
            .next()
            .unwrap_or_default();
        let format = self.style.options.page_range_format;
        let mut parts = Vec::new();
        for range in group_ranges(text) {
            let formatted = format_page_range(format, &delimiter, crate::haskell::strip(range));
            parts.push(Output::Literal(output::parse_csl_json(
                self.locale,
                &formatted,
                depth,
            )?));
        }
        Ok(output::formatted(
            Formatting {
                delimiter: Some(" ".into()),
                ..Formatting::default()
            },
            parts,
        ))
    }

    /// `lookupTerm`: the texts of the matching terms of the locale.
    fn lookup_term(&self, term: &Term) -> Vec<String> {
        self.lookup_term_entries(term)
            .into_iter()
            .map(|(_, text)| text)
            .collect()
    }

    /// `lookupTerm` with the terms of the locale that match.
    fn lookup_term_entries(&self, term: &Term) -> Vec<(Term, String)> {
        let name = if term.name == "sub verbo" {
            "sub-verbo"
        } else {
            term.name.as_str()
        };
        let wanted = Term {
            name: name.to_owned(),
            ..term.clone()
        };
        self.locale
            .terms
            .get(name)
            .map(|terms| {
                terms
                    .iter()
                    .filter(|(found, _)| term_matches(&wanted, found))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `lookupTerm'`: a missing form falls back to another form.
    fn term(&self, term: &Term, depth: usize) -> Result<Output, EvalError> {
        let mut term = term.clone();
        loop {
            match self.lookup_term(&term).into_iter().next() {
                Some(text) if text.is_empty() => return Ok(Output::Null),
                Some(text) => {
                    return Ok(Output::Literal(output::parse_csl_json(
                        self.locale,
                        &text,
                        depth,
                    )?));
                }
                None => {
                    term.form = match term.form {
                        TermForm::VerbShort => TermForm::Verb,
                        TermForm::Symbol => TermForm::Short,
                        TermForm::Verb | TermForm::Short => TermForm::Long,
                        TermForm::Long => return Ok(Output::Null),
                    };
                }
            }
        }
    }

    /// The test of a `choose` branch. No locator, no position and no
    /// disambiguation exist in this port.
    fn matches(&self, matching: Match, conditions: &[Condition]) -> bool {
        let test = |condition: &Condition| match condition {
            Condition::HasVariable(v) if v.as_str() == "locator" => false,
            Condition::HasVariable(v) => {
                lookup_variable(self.reference, v.as_str()).is_some_and(|x| is_nonempty(&x))
            }
            Condition::HasType(types) => {
                !self.reference.kind.is_empty() && types.contains(&self.reference.kind)
            }
            Condition::IsUncertainDate(v) => matches!(
                lookup_variable(self.reference, v.as_str()),
                Some(Val::Date(date)) if date.circa
            ),
            Condition::IsNumeric(v) if v.as_str() == "locator" => false,
            Condition::IsNumeric(v) => match lookup_variable(self.reference, v.as_str()) {
                Some(Val::Num(_)) => true,
                Some(Val::Text(x)) => is_numeric(&x),
                Some(Val::Fancy(x)) => is_numeric(&output::stringify(&x)),
                _ => false,
            },
            Condition::HasLocatorType(_)
            | Condition::HasPosition(_)
            | Condition::WouldDisambiguate => false,
        };
        match matching {
            Match::All => conditions.iter().all(test),
            Match::Any => conditions.iter().any(test),
            Match::None => !conditions.iter().any(test),
        }
    }
}

fn term_named(name: &str) -> Term {
    Term {
        name: name.to_owned(),
        form: TermForm::Long,
        number: None,
        gender: None,
        gender_form: None,
        matching: None,
    }
}

/// `termMatches`: equal names and forms; each other field matches when one
/// side has no value.
fn term_matches(a: &Term, b: &Term) -> bool {
    fn optional<T: PartialEq>(x: &Option<T>, y: &Option<T>) -> bool {
        x.is_none() || y.is_none() || x == y
    }
    a.name == b.name
        && a.form == b.form
        && optional(&a.number, &b.number)
        && optional(&a.gender, &b.gender)
        && optional(&a.gender_form, &b.gender_form)
        && optional(&a.matching, &b.matching)
}

/// `lookupVariable`: `id` and `type` come from the reference fields, and
/// `page-first` from the digits at the start of `page`.
fn lookup_variable(reference: &Reference<Vec<Inline>>, name: &str) -> Option<Val<Vec<Inline>>> {
    match name {
        "id" => (!reference.id.is_empty()).then(|| Val::Text(reference.id.clone())),
        "type" => (!reference.kind.is_empty()).then(|| Val::Text(reference.kind.clone())),
        _ => {
            let found = reference
                .variables
                .get(&reference::Variable::new(name))
                .cloned();
            if name != "page-first" || found.is_some() {
                return found;
            }
            let digits = |text: &str| -> Option<Val<Vec<Inline>>> {
                let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
                digits.parse().ok().map(Val::Num)
            };
            match reference
                .variables
                .get(&reference::Variable::new("pages"))?
            {
                Val::Num(n) => Some(Val::Num(*n)),
                Val::Text(t) => digits(t),
                Val::Fancy(x) => digits(&output::stringify(x)),
                _ => None,
            }
        }
    }
}

/// The number of a label term for a value (`determinePlural`).
fn determine_plural(variable: &str, text: &str) -> TermNumber {
    if variable == "number-of-volumes" && text != "1" && text != "0" {
        return TermNumber::Plural;
    }
    if text.contains("\\-") {
        return TermNumber::Singular;
    }
    let ungrouped = if variable == "page" || variable == "locator" {
        text.to_owned()
    } else {
        drop_digit_grouping(text)
    };
    if is_one_identifier(&ungrouped) {
        TermNumber::Singular
    } else if split_nums(text).len() > 1 {
        TermNumber::Plural
    } else {
        TermNumber::Singular
    }
}

/// `isOneIdentifier`: one word with digits, no range or list punctuation.
fn is_one_identifier(text: &str) -> bool {
    let words: Vec<&str> = text
        .split(crate::haskell::is_space)
        .filter(|w| !w.is_empty())
        .collect();
    !text.chars().any(is_sep_punct)
        && !text.contains('&')
        && !words.contains(&"and")
        && words
            .iter()
            .filter(|w| w.chars().any(|c| c.is_ascii_digit()))
            .count()
            == 1
}

/// `dropDigitGrouping`: a comma between a digit and exactly three digits goes.
fn drop_digit_grouping(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let digit = |i: usize| chars.get(i).is_some_and(char::is_ascii_digit);
    let mut out = String::with_capacity(text.len());
    for (i, c) in chars.iter().enumerate() {
        let grouping = *c == ','
            && i > 0
            && digit(i - 1)
            && digit(i + 1)
            && digit(i + 2)
            && digit(i + 3)
            && !digit(i + 4);
        if !grouping {
            out.push(*c);
        }
    }
    out
}

/// `splitNums`: runs of separators and runs of other characters; a run
/// that reads as an integer is a number, and `-` alone is an en dash.
fn split_nums(text: &str) -> Vec<Val<Vec<Inline>>> {
    let class = |c: char| is_sep_punct(c) || crate::haskell::is_space(c);
    let mut runs: Vec<&str> = Vec::new();
    let mut start = 0;
    let mut first: Option<char> = None;
    for (at, c) in text.char_indices() {
        match first {
            Some(f) if class(f) == class(c) => {}
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
    runs.into_iter()
        .map(|run| match crate::types::read_as_int(run) {
            Some(n) => Val::Num(n),
            None if run == "-" => Val::Text("\u{2013}".into()),
            None => Val::Text(run.to_owned()),
        })
        .collect()
}

/// `toRomanNumeral`: lower-case roman numerals below 4000.
fn roman(x: i64) -> String {
    if !(0..4000).contains(&x) {
        return x.to_string();
    }
    const TABLE: [(i64, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut rest = x;
    let mut out = String::new();
    for (value, letters) in TABLE {
        while rest >= value {
            out.push_str(letters);
            rest -= value;
        }
    }
    out
}

/// `isSepPunct`: the punctuation between the pages of a range.
fn is_sep_punct(c: char) -> bool {
    matches!(c, ',' | ';' | '-' | '\u{2013}')
}

/// `isNonEmpty`.
fn is_nonempty(value: &Val<Vec<Inline>>) -> bool {
    match value {
        Val::Text(t) => !t.is_empty(),
        Val::Fancy(x) => !x.is_empty(),
        Val::Names(names) => !names.is_empty(),
        Val::Date(date) => {
            !(date.parts.is_empty()
                && !date.circa
                && date.season.is_none()
                && date.literal.is_none())
        }
        Val::Num(_) => true,
    }
}

/// `valToText`.
fn val_to_text(value: &Val<Vec<Inline>>) -> Option<String> {
    match value {
        Val::Text(t) => Some(t.clone()),
        Val::Fancy(x) => Some(output::stringify(x)),
        Val::Num(n) => Some(n.to_string()),
        Val::Names(_) | Val::Date(_) => None,
    }
}

/// `identifierToURL`.
fn identifier_url(variable: &str, text: &str) -> String {
    let prefix = match variable {
        "DOI" => "https://doi.org/",
        "PMCID" => "https://www.ncbi.nlm.nih.gov/pmc/articles/",
        "PMID" => "https://www.ncbi.nlm.nih.gov/pubmed/",
        _ => "https://",
    };
    if text.is_empty() || text.contains("://") {
        text.to_owned()
    } else {
        format!("{prefix}{text}")
    }
}

/// The numeric test of `is-numeric`: each part between `,`, `-`, `&` and
/// `–`, without letters at its ends, is digits.
fn is_numeric(text: &str) -> bool {
    text.split([',', '-', '&', '\u{2013}']).all(|chunk| {
        let core = crate::haskell::strip(chunk)
            .trim_end_matches(is_letter)
            .trim_start_matches(is_letter);
        !core.is_empty() && core.chars().all(|c| c.is_ascii_digit())
    })
}

/// `T.groupBy`: runs of characters split before and after `,` and `&`.
fn group_ranges(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut first: Option<char> = None;
    for (at, c) in text.char_indices() {
        if let Some(f) = first {
            let together = !(f == ',' || f == '&' || c == ',' || c == '&');
            if !together {
                out.push(&text[start..at]);
                start = at;
                first = Some(c);
            }
        } else {
            first = Some(c);
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// `formatPageRange`.
pub fn format_page_range(format: Option<PageRangeFormat>, delimiter: &str, text: &str) -> String {
    if text == "&" || text == "," {
        return text.to_owned();
    }
    let parts: Vec<String> = if text.contains("\\-") {
        vec![text.replace("\\-", "-")]
    } else {
        text.split(['-', '\u{2013}'])
            .map(|p| crate::haskell::strip(p).to_owned())
            .collect()
    };
    let in_range = |prefix: &str, xs: &[&str]| -> String {
        let joined = xs.join(delimiter);
        format!("{prefix}{joined}")
    };
    match parts.as_slice() {
        [] => String::new(),
        [w] => w.clone(),
        [w, v] => {
            let Some(format) = format else {
                return in_range("", &[w, v]);
            };
            let w_prefix = w.trim_end_matches(|c: char| c.is_ascii_digit());
            let v_prefix = v.trim_end_matches(|c: char| c.is_ascii_digit());
            if w_prefix != v_prefix {
                return in_range("", &[w, v]);
            }
            let prefix = w_prefix;
            let x: Vec<char> = w[w_prefix.len()..].chars().collect();
            let y: Vec<char> = v[v_prefix.len()..].chars().collect();
            let y2: Vec<char> = if y.len() < x.len() {
                x[..x.len() - y.len()]
                    .iter()
                    .chain(y.iter())
                    .copied()
                    .collect()
            } else {
                y.clone()
            };
            let xs: String = x.iter().collect();
            let ys: String = y2.iter().collect();
            let second_last_zero = |s: &[char]| s.len() >= 2 && s[s.len() - 2] == '0';
            let minimal = |threshold: usize| -> String {
                let common = xs
                    .chars()
                    .zip(ys.chars())
                    .take_while(|(a, b)| a == b)
                    .count();
                if common == 0 {
                    return in_range(prefix, &[&xs, &ys]);
                }
                let rest_x = x.len() - common;
                let rest_y: String = y2[common..].iter().collect();
                if rest_x == y2.len() - common {
                    if rest_y.chars().count() < threshold && y2.len() >= threshold {
                        let tail: String = y2[y2.len() - threshold..].iter().collect();
                        in_range(prefix, &[&xs, &tail])
                    } else {
                        in_range(prefix, &[&xs, &rest_y])
                    }
                } else {
                    in_range(prefix, &[&xs, &ys])
                }
            };
            let changed = || {
                x.iter()
                    .copied()
                    .chain(std::iter::repeat(' '))
                    .zip(y2.iter())
                    .filter(|(a, b)| a != *b)
                    .count()
            };
            match format {
                PageRangeFormat::Chicago15 | PageRangeFormat::Chicago16 => {
                    if x.len() < 3 || xs.ends_with("00") {
                        in_range(prefix, &[&xs, &ys])
                    } else if second_last_zero(&x) && second_last_zero(&y2) {
                        minimal(1)
                    } else if format == PageRangeFormat::Chicago15 && x.len() == 4 && changed() >= 3
                    {
                        in_range(prefix, &[&xs, &ys])
                    } else {
                        minimal(2)
                    }
                }
                PageRangeFormat::Expanded => {
                    in_range("", &[&format!("{prefix}{xs}"), &format!("{prefix}{ys}")])
                }
                PageRangeFormat::Minimal => minimal(1),
                PageRangeFormat::MinimalTwo => minimal(2),
            }
        }
        _ => text.to_owned(),
    }
}
