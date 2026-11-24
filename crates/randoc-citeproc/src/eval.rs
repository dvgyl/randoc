//! The evaluation of the elements of a style for one item: a port of the
//! parts of `Citeproc.Eval` (`citeproc` 0.14.1.1) for `text` (variables,
//! values, terms and macros), `group` with its suppression rule, `choose`,
//! `number`, `label`, `date`, `names` (with `substitute`), page ranges and
//! term lookup, with the sort key mode, the bibliography mode and the hooks
//! of disambiguation. [`crate::citeproc`] evaluates a whole document;
//! [`eval_citation`] evaluates one citation alone.

use std::collections::{BTreeSet, HashSet};

use randoc_ast::Inline;

use crate::case_transform::{to_lower, to_upper};
use crate::lang::{Lang, parse_lang};
use crate::markup::TooDeep;
use crate::output::{self, Output, Tag};
use crate::reference::{self, DateParts, Name, Reference, Val};
use crate::types::{
    Condition, DatePart, DatePartForm, DatePartName, DateType, DelimiterPrecedes,
    DemoteNonDroppingParticle, Element, ElementType, Formatting, ItemType, Locale, Match,
    NameAsSortOrder, NameForm, NameFormat, NamesFormat, NumberForm, PageRangeFormat, Pluralize,
    Position, ShowDateParts, Style, Term, TermForm, TermGender, TermMatch, TermNumber, TextCase,
    TextType, Variable, VariableForm,
};
use crate::unicode_class::{is_letter, is_lower, is_upper};

/// An evaluation that cannot go on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    /// The style or the output is nested deeper than the depth limit.
    TooDeep,
    /// The evaluation used all available work units.
    WorkLimit,
    /// The conversion deadline passed.
    Deadline,
}

const MAX_EVALUATION_STEPS: usize = 1_000_000;

pub(crate) struct EvalBudget {
    pub(crate) remaining: usize,
    deadline: Option<std::time::Instant>,
}

impl EvalBudget {
    pub(crate) fn new(deadline: Option<std::time::Instant>) -> Self {
        Self {
            remaining: MAX_EVALUATION_STEPS,
            deadline,
        }
    }

    pub(crate) fn check_deadline(&self) -> Result<(), EvalError> {
        if self
            .deadline
            .is_some_and(|deadline| std::time::Instant::now() >= deadline)
        {
            return Err(EvalError::Deadline);
        }
        Ok(())
    }

    fn step(&mut self) -> Result<(), EvalError> {
        step();
        if (MAX_EVALUATION_STEPS - self.remaining).is_multiple_of(256) {
            self.check_deadline()?;
        }
        self.remaining = self.remaining.checked_sub(1).ok_or(EvalError::WorkLimit)?;
        Ok(())
    }
}

#[cfg(test)]
thread_local! {
    /// The work units of an evaluation: elements, names, name parts and the
    /// characters of a given name.
    static STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts work units of an evaluation (test builds only).
pub(crate) fn step() {
    #[cfg(test)]
    STEPS.with(|steps| steps.set(steps.get() + 1));
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

/// `CitationItem`: one cited item and its reference.
#[derive(Debug, Clone, PartialEq)]
pub struct CitationItem<'a> {
    /// The identifier of the item.
    pub id: String,
    /// The reference with this identifier, if the bibliography has one.
    pub reference: Option<&'a Reference<Vec<Inline>>>,
    /// The form of the item.
    pub kind: ItemType,
    /// The locator label, for example `page`.
    pub label: Option<String>,
    /// The locator, for example `33`.
    pub locator: Option<String>,
    /// The text before the item.
    pub prefix: Option<Vec<Inline>>,
    /// The text after the item.
    pub suffix: Option<Vec<Inline>>,
}

impl<'a> CitationItem<'a> {
    /// A normal item with no locator and no affixes.
    pub fn new(id: &str, reference: Option<&'a Reference<Vec<Inline>>>) -> CitationItem<'a> {
        CitationItem {
            id: id.to_owned(),
            reference,
            kind: ItemType::NormalCite,
            label: None,
            locator: None,
            prefix: None,
            suffix: None,
        }
    }
}

/// `Citation`: the items of one citation and the affixes of all of them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Citation<'a> {
    /// The number of the note that holds the citation.
    pub note_number: Option<i64>,
    /// The text before the first item.
    pub prefix: Option<Vec<Inline>>,
    /// The text after the last item.
    pub suffix: Option<Vec<Inline>>,
    /// The cited items.
    pub items: Vec<CitationItem<'a>>,
}

/// Evaluates one citation with the citation layout of `style`: the
/// pipeline of [`crate::citeproc::evaluate`] for a document with this one
/// citation and the references of its items. `locale` is the merged locale
/// of the style. An item with no reference gives the bold identifier and a
/// question mark, with a warning.
pub fn eval_citation(
    style: &Style,
    locale: &Locale,
    citation: &Citation,
    max_depth: usize,
) -> Result<Evaluated, EvalError> {
    let mut seen = HashSet::new();
    let mut references = Vec::new();
    for item in &citation.items {
        if let Some(reference) = item.reference
            && seen.insert(item.id.as_str())
        {
            let mut reference = reference.clone();
            reference.id.clone_from(&item.id);
            references.push(reference);
        }
    }
    let cite = crate::citeproc::Citation {
        id: None,
        note_number: citation.note_number,
        reset_position: false,
        prefix: citation.prefix.clone(),
        suffix: citation.suffix.clone(),
        items: citation
            .items
            .iter()
            .map(|item| crate::citeproc::CitationItem {
                id: item.id.clone(),
                kind: item.kind,
                label: item.label.clone(),
                locator: item.locator.clone(),
                prefix: item.prefix.clone(),
                suffix: item.suffix.clone(),
            })
            .collect(),
    };
    let input = crate::citeproc::Input {
        style,
        locale,
        lang: None,
        abbreviations: None,
        references,
        citations: std::slice::from_ref(&cite),
        max_depth,
    };
    let evaluated = crate::citeproc::evaluate(input)?;
    Ok(Evaluated {
        output: evaluated
            .citations
            .into_iter()
            .next()
            .unwrap_or(Output::Null),
        warnings: evaluated.warnings,
    })
}

/// `addCitationAffixes`: the prefix of the citation goes before the prefix
/// of the first item, the suffix after the suffix of the last item.
pub(crate) fn add_citation_affixes(
    citation: &crate::citeproc::Citation,
) -> Vec<crate::citeproc::CitationItem> {
    let mut items = citation.items.clone();
    if let (Some(prefix), Some(first)) = (&citation.prefix, items.first_mut()) {
        let mut joined = crate::csljson::Inlines(prefix.clone());
        joined.extend(first.prefix.take().unwrap_or_default());
        first.prefix = Some(joined.0);
    }
    if let (Some(suffix), Some(last)) = (&citation.suffix, items.last_mut()) {
        let mut joined = crate::csljson::Inlines(last.suffix.take().unwrap_or_default());
        joined.extend(suffix.clone());
        last.suffix = Some(joined.0);
    }
    items
}

/// The steps of `evalItem'` after the evaluation of the layout.
pub(crate) fn wrap_item(
    item: &crate::citeproc::CitationItem,
    mut xs: Vec<Output>,
    is_note: bool,
    second_field_align: bool,
) -> Output {
    if second_field_align {
        xs = align_second_field(xs);
    }
    let whole_sentence = |prefix: &Vec<Inline>| {
        let text = output::stringify(prefix);
        text.ends_with(". ") && text.matches(' ').count() > 1
    };
    if is_note && item.prefix.as_ref().is_some_and(whole_sentence) {
        capitalize_initial_term(&mut xs);
    }
    if item.kind == ItemType::AuthorOnly {
        xs = vec![get_authors(&output::formatted(Formatting::default(), xs))];
    }
    let mut x = match output::formatted(Formatting::default(), xs) {
        Output::Null => Output::Null,
        x => Output::Tagged(
            Tag::Item {
                kind: item.kind,
                id: item.id.clone(),
            },
            Box::new(x),
        ),
    };
    if let Some(suffix) = &item.suffix {
        x = output::formatted(
            Formatting::default(),
            vec![
                x,
                Output::Tagged(Tag::Suffix, Box::new(Output::Literal(suffix.clone()))),
            ],
        );
    }
    if let Some(prefix) = &item.prefix {
        x = output::formatted(
            Formatting::default(),
            vec![
                Output::Tagged(Tag::Prefix, Box::new(Output::Literal(prefix.clone()))),
                x,
            ],
        );
    }
    x
}

/// `secondFieldAlign`: the first part goes in the left margin, the rest
/// inline on the right. A link around all parts gives its first part up.
fn align_second_field(mut xs: Vec<Output>) -> Vec<Output> {
    if let [Output::Linked(url, parts)] = xs.as_mut_slice()
        && !parts.is_empty()
    {
        let first = parts.remove(0);
        let rest = Output::Linked(std::mem::take(url), std::mem::take(parts));
        return align_second_field(vec![first, rest]);
    }
    if xs.is_empty() {
        return xs;
    }
    let first = xs.remove(0);
    let display = |display, parts| {
        output::formatted(
            Formatting {
                display: Some(display),
                ..Formatting::default()
            },
            parts,
        )
    };
    vec![
        display(crate::types::DisplayStyle::LeftMargin, vec![first]),
        display(crate::types::DisplayStyle::RightInline, xs),
    ]
}

/// `capitalizeInitialTerm`: the first term of the first part, along the
/// first parts, starts with a capital letter.
fn capitalize_initial_term(xs: &mut [Output]) {
    let mut current = xs.first_mut();
    while let Some(x) = current {
        current = match x {
            Output::Tagged(Tag::Term(_), inner) => {
                let term = std::mem::replace(inner.as_mut(), Output::Null);
                **inner = output::formatted(
                    Formatting {
                        text_case: Some(TextCase::CapitalizeFirst),
                        ..Formatting::default()
                    },
                    vec![term],
                );
                None
            }
            Output::Formatted(_, parts) => parts.first_mut(),
            Output::Tagged(_, inner) => Some(inner.as_mut()),
            _ => None,
        };
    }
}

/// `getAuthors`: the first names output in pre-order, or nothing.
pub(crate) fn get_authors(x: &Output) -> Output {
    let mut pending = vec![x];
    while let Some(x) = pending.pop() {
        match x {
            Output::Tagged(Tag::Names { .. }, _) => return x.clone(),
            Output::Tagged(_, inner) | Output::InNote(inner) => pending.push(inner),
            Output::Formatted(_, parts) | Output::Linked(_, parts) => {
                pending.extend(parts.iter().rev());
            }
            Output::Literal(_) | Output::Null => {}
        }
    }
    Output::Null
}

/// `moveSuffixInsideDisplay`: the suffix of the layout goes into the last
/// part with a `display` attribute, along the last parts.
pub(crate) fn move_suffix_inside_display(
    zs: &[Output],
    suffix: &str,
    depth: usize,
) -> Result<Option<Vec<Output>>, EvalError> {
    let depth = depth.checked_sub(1).ok_or(EvalError::TooDeep)?;
    let Some((last, init)) = zs.split_last() else {
        return Ok(None);
    };
    let moved = match last {
        Output::Tagged(tag @ Tag::Item { .. }, inner) => match inner.as_ref() {
            Output::Formatted(f, ys) => move_suffix_inside_display(ys, suffix, depth)?
                .map(|ys| Output::Tagged(tag.clone(), Box::new(Output::Formatted(f.clone(), ys)))),
            _ => None,
        },
        Output::Formatted(f, ys) if f.display.is_some() => {
            let mut f = f.clone();
            f.suffix = Some(format!("{}{suffix}", f.suffix.as_deref().unwrap_or("")));
            Some(Output::Formatted(f, ys.clone()))
        }
        Output::Formatted(f, ys) => move_suffix_inside_display(ys, suffix, depth)?
            .map(|ys| Output::Formatted(f.clone(), ys)),
        _ => None,
    };
    Ok(moved.map(|moved| {
        let mut zs = init.to_vec();
        zs.push(moved);
        zs
    }))
}

/// `handleAuthorOnly`: an author-only item at the start of the citation
/// stands apart from the other items.
pub(crate) fn handle_author_only(cite: Output, is_note: bool) -> Output {
    let is_author_only = |x: &Output| {
        matches!(
            x,
            Output::Tagged(
                Tag::Item {
                    kind: ItemType::AuthorOnly,
                    ..
                },
                _
            )
        )
    };
    let apart =
        |x: Output, others: Option<Output>, joined_by: fn(Vec<Output>) -> Output| match others {
            None => joined_by(vec![x]),
            Some(others) if is_note => joined_by(vec![x, Output::InNote(Box::new(others))]),
            Some(others) => joined_by(vec![x, Output::Literal(output::from_text(" ")), others]),
        };
    let formatted: fn(Vec<Output>) -> Output =
        |parts| output::formatted(Formatting::default(), parts);
    let plain: fn(Vec<Output>) -> Output = |parts| Output::Formatted(Box::default(), parts);
    let mut cite = cite;
    if let Output::Formatted(f, xs) = &mut cite
        && xs.first().is_some_and(is_author_only)
    {
        let f = std::mem::take(&mut **f);
        let mut xs = std::mem::take(xs);
        let x = xs.remove(0);
        let others = (!xs.is_empty()).then(|| output::formatted(f, xs));
        return apart(x, others, formatted);
    }
    if let Output::Formatted(f, ys) = &mut cite
        && let Some(Output::Formatted(f2, xs)) = ys.first_mut()
        && xs.first().is_some_and(is_author_only)
    {
        let f2 = std::mem::take(&mut **f2);
        let mut xs = std::mem::take(xs);
        let f = std::mem::take(&mut **f);
        let mut ys = std::mem::take(ys);
        ys.remove(0);
        let x = xs.remove(0);
        let others = (!xs.is_empty() || !ys.is_empty()).then(|| {
            let mut parts = vec![output::formatted(f2, xs)];
            parts.extend(ys);
            output::formatted(f, parts)
        });
        return apart(x, others, if is_note { formatted } else { plain });
    }
    if is_note {
        Output::InNote(Box::new(cite))
    } else {
        cite
    }
}

/// `removeNamesIfSuppressAuthor` on each part (`transform`): in a
/// suppress-author item, each part equal to the first names is removed.
pub(crate) fn remove_names_if_suppress_author(
    x: &mut Output,
    depth: usize,
) -> Result<(), EvalError> {
    let depth = depth.checked_sub(1).ok_or(EvalError::TooDeep)?;
    for child in children_mut(x) {
        remove_names_if_suppress_author(child, depth)?;
    }
    if let Output::Tagged(
        Tag::Item {
            kind: ItemType::SuppressAuthor,
            ..
        },
        inner,
    ) = x
    {
        let authors = get_authors(inner);
        remove_equal(inner, &authors, depth)?;
    }
    Ok(())
}

/// `transform (removeIfEqual y)`: each part equal to `y` becomes nothing.
fn remove_equal(x: &mut Output, y: &Output, depth: usize) -> Result<(), EvalError> {
    let depth = depth.checked_sub(1).ok_or(EvalError::TooDeep)?;
    for child in children_mut(x) {
        remove_equal(child, y, depth)?;
    }
    if x == y {
        *x = Output::Null;
    }
    Ok(())
}

/// The direct output parts of an output (the children of `uniplate`).
pub(crate) fn children_mut(x: &mut Output) -> Vec<&mut Output> {
    match x {
        Output::Formatted(_, parts) | Output::Linked(_, parts) => parts.iter_mut().collect(),
        Output::Tagged(_, inner) | Output::InNote(inner) => vec![inner.as_mut()],
        Output::Literal(_) | Output::Null => Vec::new(),
    }
}

/// `citationLabel`: a trigraph from the names and the year of the reference.
pub(crate) fn citation_label(reference: &Reference<Vec<Inline>>) -> String {
    let names = |variable: &reference::Variable| match reference.variables.get(variable) {
        Some(Val::Names(names)) => {
            let take = match names.len() {
                1 => 4,
                n if n >= 4 => 1,
                _ => 2,
            };
            names
                .iter()
                .take(4)
                .map(|name| {
                    name.family
                        .as_deref()
                        .unwrap_or("")
                        .chars()
                        .take(take)
                        .collect::<String>()
                })
                .collect()
        }
        _ => String::new(),
    };
    let author = reference::Variable::new("author");
    let name_part = if reference.variables.contains_key(&author) {
        names(&author)
    } else {
        match reference
            .variables
            .keys()
            .find(|v| v.kind() == reference::VariableType::Name)
        {
            Some(variable) => names(variable),
            None => "Xyz".to_owned(),
        }
    };
    let date_part = match reference.variables.get(&reference::Variable::new("issued")) {
        Some(Val::Date(date)) => match date.parts.first().and_then(|p| p.0.first()) {
            Some(year) => format!("{:02}", year.rem_euclid(100)),
            None => String::new(),
        },
        _ => String::new(),
    };
    name_part + &date_part
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
    accessed: i64,
    nonempty: i64,
}

/// A variable of the reference as the evaluation sees it. A variable that a
/// `substitute` element has used is `SubstitutedVal` upstream.
enum Found {
    Substituted,
    Value(Val<Vec<Inline>>),
}

/// The year, the month and the day of one date (`bindDateParts`).
type Ymd = [Option<i64>; 3];

/// The disambiguation data of a reference (`DisambiguationData`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DisambiguationData {
    /// The year suffix, 1 for `a`.
    pub year_suffix: Option<i64>,
    /// The hints for names. The references share one map of a global
    /// given-name rule; a reference gets its own copy at its first change.
    pub name_map: std::sync::Arc<std::collections::BTreeMap<Name, NameHint>>,
    /// The number of names to show before `et-al`.
    pub et_al_names: Option<i64>,
    /// The value of the `disambiguate` condition.
    pub condition: bool,
}

/// `NameHints`: how a name shows more of its given name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs)]
pub enum NameHint {
    AddInitials,
    AddGivenName,
    AddInitialsIfPrimary,
    AddGivenNameIfPrimary,
}

/// The context of the evaluation of one item.
pub(crate) struct ItemContext<'a> {
    pub style: &'a Style,
    pub locale: &'a Locale,
    pub reference: &'a Reference<Vec<Inline>>,
    pub disamb: Option<&'a DisambiguationData>,
    pub abbreviations: Option<&'a crate::abbreviations::Abbreviations>,
    pub name_format: NameFormat,
    pub locator: Option<&'a str>,
    pub label: Option<&'a str>,
    pub positions: Vec<Position>,
    pub in_sort_key: bool,
    pub in_bibliography: bool,
}

/// What the evaluation of an item used (`stateUsedIdentifier`, `stateUsedTitle`).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Used {
    pub identifier: bool,
    pub title: bool,
}

/// `eElements` for one item: the outputs, with the language of the item on
/// each formatted part.
pub(crate) fn eval_elements(
    context: ItemContext,
    elements: &[Element],
    warnings: &mut BTreeSet<String>,
    depth: usize,
    budget: &mut EvalBudget,
) -> Result<(Vec<Output>, Used), EvalError> {
    let mut eval = Eval {
        budget,
        style: context.style,
        locale: context.locale,
        reference: context.reference,
        reference_lang: reference_lang(context.reference),
        locator: context.locator,
        label: context.label,
        positions: context.positions,
        var_count: VarCount::default(),
        in_substitute: false,
        substitute_names_form: None,
        name_format: context.name_format,
        substituted: BTreeSet::new(),
        warnings,
        in_sort_key: context.in_sort_key,
        in_bibliography: context.in_bibliography,
        disamb: context.disamb,
        abbreviations: context.abbreviations,
        used_year_suffix: false,
        used: Used::default(),
    };
    let xs = eval.elements(elements, depth)?;
    Ok((xs, eval.used))
}

/// `stateReferenceLang` of a reference.
pub(crate) fn language_of(reference: &Reference<Vec<Inline>>) -> Option<Lang> {
    reference_lang(reference)
}

/// `addLangToFormatting` on each formatted part of the outputs.
pub(crate) fn add_language(outputs: &mut [Output], lang: &Lang) {
    for x in outputs {
        add_lang(x, lang);
    }
}

struct Eval<'a, 'w> {
    budget: &'w mut EvalBudget,
    style: &'a Style,
    locale: &'a Locale,
    reference: &'a Reference<Vec<Inline>>,
    reference_lang: Option<Lang>,
    /// `contextLocator`.
    locator: Option<&'a str>,
    /// `contextLabel`.
    label: Option<&'a str>,
    /// `contextPosition`.
    positions: Vec<Position>,
    var_count: VarCount,
    /// `contextInSubstitute`.
    in_substitute: bool,
    /// `contextSubstituteNamesForm`: the options of the `names` element
    /// whose `substitute` runs.
    substitute_names_form: Option<NamesFormat>,
    /// `contextNameFormat`: the name options of the layout and the style.
    name_format: NameFormat,
    /// The variables that a `substitute` element has used.
    substituted: BTreeSet<reference::Variable>,
    warnings: &'w mut BTreeSet<String>,
    /// `contextInSortKey`.
    in_sort_key: bool,
    /// `contextInBibliography`.
    in_bibliography: bool,
    /// `referenceDisambiguation` of the reference.
    disamb: Option<&'a DisambiguationData>,
    /// `contextAbbreviations`.
    abbreviations: Option<&'a crate::abbreviations::Abbreviations>,
    /// `stateUsedYearSuffix`.
    used_year_suffix: bool,
    used: Used,
}

impl<'a> Eval<'a, '_> {
    fn update_var_count(&mut self, accessed: i64, nonempty: i64) {
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
        self.budget.step()?;
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
            ElementType::Date {
                variable,
                date_type,
                show_date_parts,
                parts,
            } => vec![self.date(
                variable.as_str(),
                *date_type,
                *show_date_parts,
                parts,
                formatting,
                depth,
            )?],
            ElementType::Names(variables, names_format, substitute) => {
                vec![self.names(variables, names_format, substitute, formatting, depth)?]
            }
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
        if same(name, "page-first") {
            let first = |text: &str| text.chars().take_while(|c| !is_sep_punct(*c)).collect();
            return match self.ask_variable("page")? {
                Val::Text(t) => Some(Val::Text(first(&t))),
                Val::Fancy(x) => Some(Val::Text(first(&output::stringify(&x)))),
                Val::Num(n) => Some(Val::Num(n)),
                Val::Names(_) | Val::Date(_) => None,
            };
        }
        match self.find(name) {
            Some(Found::Value(value)) if is_nonempty(&value) => {
                self.update_var_count(1, 1);
                Some(value)
            }
            _ => {
                self.update_var_count(1, 0);
                None
            }
        }
    }

    /// `lookupVariable` on the reference of the state.
    fn find(&self, name: &str) -> Option<Found> {
        if !self.substituted.is_empty()
            && !same(name, "id")
            && !same(name, "type")
            && self.substituted.contains(&reference::Variable::new(name))
        {
            return Some(Found::Substituted);
        }
        lookup_variable(self.reference, name).map(Found::Value)
    }

    /// `deleteSubstitutedVariables`: inside `substitute`, a variable that
    /// gave output is not used again.
    fn delete_substituted(&mut self, name: &str) {
        if self.in_substitute {
            self.substituted.insert(reference::Variable::new(name));
        }
    }

    /// `getYearSuffix`: the automatic year suffix, once per item, when the
    /// style does not use the variable `year-suffix`.
    fn year_suffix(&mut self) -> Option<Output> {
        let n = self.disamb?.year_suffix?;
        if self.style.options.uses_year_suffix_variable || self.used_year_suffix {
            return None;
        }
        self.used_year_suffix = true;
        Some(Output::Tagged(
            Tag::YearSuffix(n),
            Box::new(Output::Literal(output::from_text(&show_year_suffix(n)))),
        ))
    }

    /// `eText`.
    fn text(&mut self, text: &TextType, depth: usize) -> Result<Output, EvalError> {
        Ok(match text {
            TextType::Value(value) => {
                Output::Literal(output::parse_csl_json(self.locale, value, depth)?)
            }
            TextType::Term(term) => {
                let mut output = self.term(term, depth)?;
                if term.name == "no date"
                    && let Some(suffix) = self.year_suffix()
                {
                    output = if term.form == TermForm::Long {
                        output::formatted(
                            Formatting::default(),
                            vec![output, Output::Literal(output::from_text(" ")), suffix],
                        )
                    } else {
                        output::formatted(Formatting::default(), vec![output, suffix])
                    };
                }
                Output::Tagged(Tag::Term(Box::new(term.clone())), Box::new(output))
            }
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
        // The special variables compare without case (Codex citeproc-R3b-02).
        let special = [
            "id",
            "type",
            "locator",
            "year-suffix",
            "citation-number",
            "citation-label",
            "doi",
            "pmcid",
            "pmid",
            "url",
        ]
        .into_iter()
        .find(|key| same(name, key))
        .unwrap_or("");
        Ok(match special {
            "id" => {
                self.update_var_count(1, 1);
                Output::Literal(output::from_text(&self.reference.id))
            }
            "type" => {
                self.update_var_count(1, 1);
                Output::Literal(output::from_text(&self.reference.kind))
            }
            "locator" => match self.locator {
                None => {
                    self.update_var_count(1, 0);
                    Output::Null
                }
                Some(locator) => {
                    let mut locator = locator.to_owned();
                    if locator.contains('&')
                        && let Some(and) = self.lookup_term(&Term {
                            form: TermForm::Symbol,
                            ..term_named("and")
                        })
                    {
                        locator = locator.replace('&', and);
                    }
                    self.update_var_count(1, 1);
                    let inner = if self.label.is_none_or(|label| label == "page") {
                        self.page_range(&locator, depth)?
                    } else {
                        Output::Literal(output::parse_csl_json(
                            self.locale,
                            &format_page_range(None, "\u{2013}", &locator),
                            depth,
                        )?)
                    };
                    Output::Tagged(Tag::Locator, Box::new(inner))
                }
            },
            // The year suffix does not count as a variable.
            "year-suffix" => match self.disamb.and_then(|d| d.year_suffix) {
                Some(n) => Output::Tagged(
                    Tag::YearSuffix(n),
                    Box::new(Output::Literal(output::from_text(&show_year_suffix(n)))),
                ),
                None => Output::Null,
            },
            "citation-number" | "citation-label" => {
                let value = self.ask_variable(name);
                let suffix = if special == "citation-label" {
                    self.year_suffix()
                } else {
                    None
                };
                let label = |x: Vec<Inline>, suffix: Option<Output>| {
                    let mut parts = vec![Output::Literal(x)];
                    parts.extend(suffix);
                    Output::Tagged(
                        Tag::CitationLabel,
                        Box::new(output::formatted(Formatting::default(), parts)),
                    )
                };
                match (special, value) {
                    ("citation-number", Some(Val::Num(n))) => Output::Tagged(
                        Tag::CitationNumber(n),
                        Box::new(Output::Literal(output::from_text(&n.to_string()))),
                    ),
                    ("citation-label", Some(Val::Text(t))) => label(output::from_text(&t), suffix),
                    ("citation-label", Some(Val::Fancy(x))) => label(x, suffix),
                    _ => {
                        let what = if special == "citation-number" {
                            "citation-number not defined for "
                        } else {
                            "citation-label of unknown type for "
                        };
                        self.warnings.insert(format!("{what}{}", self.reference.id));
                        Output::Null
                    }
                }
            }
            "doi" | "pmcid" | "pmid" | "url" => {
                let value = self.ask_variable(name);
                self.delete_substituted(name);
                match value.as_ref().and_then(val_to_text) {
                    None => Output::Null,
                    Some(text) => {
                        self.used.identifier = true;
                        Output::Linked(
                            identifier_url(special, &text),
                            vec![Output::Literal(output::from_text(&text))],
                        )
                    }
                }
            }
            _ => {
                let value = if form == VariableForm::Short {
                    let short = self.ask_variable(&format!("{name}-short"));
                    let long = self.ask_variable(name);
                    short.or(long).map(|value| {
                        self.abbreviations
                            .and_then(|a| a.lookup(name, &value))
                            .unwrap_or(value)
                    })
                } else {
                    self.ask_variable(name)
                };
                if value.is_some() {
                    self.delete_substituted(name);
                    if form == VariableForm::Short {
                        self.delete_substituted(&format!("{name}-short"));
                    }
                }
                let result = match value {
                    Some(Val::Text(x)) if same(name, "page") => self.page_range(&x, depth)?,
                    Some(Val::Text(x)) => Output::Literal(output::from_text(&x)),
                    Some(Val::Fancy(x)) if same(name, "page") => {
                        self.page_range(&output::stringify(&x), depth)?
                    }
                    Some(Val::Fancy(x)) => Output::Literal(x),
                    Some(Val::Num(n)) => Output::Literal(output::from_text(&n.to_string())),
                    _ => Output::Null,
                };
                if same(name, "title") && result != Output::Null {
                    self.used.title = true;
                    Output::Tagged(Tag::Title, Box::new(result))
                } else {
                    result
                }
            }
        })
    }

    /// `eNumber`: each number part of the value in the form.
    fn number(&mut self, name: &str, form: NumberForm, depth: usize) -> Result<Output, EvalError> {
        let value = self.ask_variable(name);
        let value_given = value.is_some();
        let gender = lookup_term_entries(self.locale, &term_named(name))
            .next()
            .and_then(|(term, _)| term.gender);
        let parts = match value {
            Some(Val::Num(n)) => vec![Val::Num(n)],
            Some(Val::Fancy(x)) => split_nums(&output::stringify(&x)),
            Some(Val::Text(t)) => split_nums(&t),
            _ => Vec::new(),
        };
        if value_given {
            self.delete_substituted(name);
        }
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
                    Some(text) => Ok(Output::Literal(output::from_text(text))),
                    None => self.eval_number(NumberForm::Ordinal, gender, value, depth),
                }
            }
            NumberForm::LongOrdinal | NumberForm::Ordinal => {
                let two = numbered(format!("ordinal-{:02}", i.rem_euclid(100)));
                let found = lookup_term_entries(self.locale, &two)
                    .into_iter()
                    .find(|(term, _)| i <= 99 || term.matching != Some(TermMatch::WholeNumber));
                if let Some((_, suffix)) = found {
                    return with_suffix(self, suffix);
                }
                let one = numbered(format!("ordinal-{:02}", i.rem_euclid(10)));
                let found = lookup_term_entries(self.locale, &one)
                    .into_iter()
                    .find(|(term, _)| {
                        i <= 10
                            || term.matching.is_none()
                            || term.matching == Some(TermMatch::LastDigit)
                    });
                if let Some((_, suffix)) = found {
                    return with_suffix(self, suffix);
                }
                if let Some(suffix) = self.lookup_term(&term_named("ordinal")).into_iter().next() {
                    return with_suffix(self, suffix);
                }
                self.warnings
                    .insert(format!("no ordinal suffix found for {decimal}"));
                Ok(Output::Literal(output::from_text(&decimal)))
            }
        }
    }

    /// `eLabel`: the term of the variable, or of the locator label, in the
    /// number that its value asks for.
    fn label(
        &mut self,
        name: &str,
        form: TermForm,
        pluralize: Pluralize,
        formatting: &Formatting,
        depth: usize,
    ) -> Result<Output, EvalError> {
        let lookup_name = if same(name, "editortranslator") {
            "editor"
        } else {
            name
        };
        let (term_name, found) = match (name, self.locator, self.label) {
            ("locator", Some(locator), Some(label)) => {
                (label, Some(Found::Value(Val::Text(locator.to_owned()))))
            }
            ("locator", Some(locator), None)
                if begins_with_space(locator)
                    || locator.trim_start_matches(is_letter).starts_with(". ") =>
            {
                (name, None)
            }
            ("locator", Some(locator), None) => {
                ("page", Some(Found::Value(Val::Text(locator.to_owned()))))
            }
            _ => (name, self.find(lookup_name)),
        };
        let term = match found {
            None => Output::Null,
            Some(found) => {
                let number = match (pluralize, found) {
                    (Pluralize::Always, _) => TermNumber::Plural,
                    (Pluralize::Never, _) => TermNumber::Singular,
                    (Pluralize::Contextual, Found::Value(value)) => match &value {
                        Val::Text(t) => determine_plural(name, t),
                        Val::Num(n) => determine_plural(name, &n.to_string()),
                        Val::Fancy(x) => determine_plural(name, &output::stringify(x)),
                        Val::Names(names) if names.len() > 1 => TermNumber::Plural,
                        _ => TermNumber::Singular,
                    },
                    (Pluralize::Contextual, Found::Substituted) => TermNumber::Singular,
                };
                let term = Term {
                    form,
                    number: Some(number),
                    ..term_named(term_name)
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

    /// `eDate`. A date form of the locale gives the parts; the parts of the
    /// element change their forms.
    fn date(
        &mut self,
        name: &str,
        date_type: DateType,
        show: Option<ShowDateParts>,
        parts: &[DatePart],
        formatting: &Formatting,
        depth: usize,
    ) -> Result<Output, EvalError> {
        if name.is_empty() {
            self.warnings
                .insert("skipping date element with no variable attribute set".into());
            return Ok(Output::Null);
        }
        let value = self.ask_variable(name);
        if value.is_some() {
            self.delete_substituted(name);
        }
        let shown = |part: &DatePart| match show {
            Some(ShowDateParts::Year) => part.name == DatePartName::Year,
            Some(ShowDateParts::YearMonth) => part.name != DatePartName::Day,
            Some(ShowDateParts::YearMonthDay) | None => true,
        };
        let mut first_part: [Option<&DatePart>; 3] = [None; 3];
        for part in parts {
            first_part[date_part_index(part.name)].get_or_insert(part);
        }
        let (parts, formatting): (Vec<DatePart>, Formatting) =
            match self.locale.dates.get(&date_type) {
                Some(Element {
                    kind:
                        ElementType::Date {
                            parts: locale_parts,
                            ..
                        },
                    formatting: locale_formatting,
                }) => (
                    locale_parts
                        .iter()
                        .map(|old| match first_part[date_part_index(old.name)] {
                            Some(new) => DatePart {
                                formatting: old.formatting.combine(&new.formatting),
                                ..new.clone()
                            },
                            None => old.clone(),
                        })
                        .filter(shown)
                        .collect(),
                    formatting.combine(locale_formatting),
                ),
                _ => (
                    parts.iter().filter(|p| shown(p)).cloned().collect(),
                    formatting.clone(),
                ),
            };
        let date = match value {
            None => return Ok(Output::Null),
            Some(Val::Date(date)) => date,
            Some(_) => {
                self.warnings.insert(format!(
                    "date element for variable with non-date value {name}"
                ));
                return Ok(Output::Null);
            }
        };
        if let Some(text) = &date.literal {
            // A literal stands for the year: parts with no year leave it out.
            if parts.is_empty() || parts.iter().any(|p| p.name == DatePartName::Year) {
                return Ok(output::formatted(
                    formatting,
                    vec![Output::Literal(output::from_text(text))],
                ));
            }
            self.update_var_count(0, -1);
            return Ok(Output::Null);
        }
        let mut dates = date.parts.clone();
        if let (Some(season), [DateParts(year)]) = (date.season, dates.as_slice())
            && let [year] = year.as_slice()
        {
            dates = vec![DateParts(vec![*year, season.wrapping_add(12)])];
        }
        let mut xs = if self.in_sort_key {
            let sort_date = |ymd: Ymd| format_sort_date(&parts, ymd);
            match (dates.first(), dates.get(1)) {
                (first, None) => vec![sort_date(bind_date_parts(first))],
                (first, Some(next)) => vec![
                    sort_date(bind_date_parts(first)),
                    Output::Literal(output::from_text("-")),
                    sort_date(bind_date_parts(Some(next))),
                ],
            }
        } else {
            self.date_parts(&parts, dates.first(), dates.get(1), depth)?
        };
        if xs.iter().all(|x| *x == Output::Null) {
            self.update_var_count(0, -1);
        }
        xs.extend(self.year_suffix());
        Ok(Output::Tagged(
            Tag::Date(Box::new(date)),
            Box::new(output::formatted(formatting, xs)),
        ))
    }

    /// `formatDateParts`: one date, or a range that shows the parts that
    /// are equal once.
    fn date_parts(
        &mut self,
        specs: &[DatePart],
        date: Option<&DateParts>,
        next: Option<&DateParts>,
        depth: usize,
    ) -> Result<Vec<Output>, EvalError> {
        let ymd = bind_date_parts(date);
        let Some(next) = next.map(|next| bind_date_parts(Some(next))) else {
            let mut out = Vec::with_capacity(specs.len());
            for spec in specs {
                out.push(self.date_part(ymd, spec, depth)?);
            }
            return Ok(out);
        };
        let index = date_part_index;
        let mut names: Vec<DatePartName> = specs.iter().map(|p| p.name).collect();
        names.sort();
        let mut same = [false; 3];
        for name in names {
            if ymd[index(name)] != next[index(name)] {
                break;
            }
            same[index(name)] = true;
        }
        let is_same = |p: &DatePart| same[index(p.name)];
        let split = specs
            .iter()
            .position(|p| !is_same(p))
            .unwrap_or(specs.len());
        let (sames1, rest) = specs.split_at(split);
        let split = rest.iter().position(is_same).unwrap_or(rest.len());
        let (diffs, sames2) = rest.split_at(split);
        let sames1 = self.date_parts_present(ymd, sames1, depth)?;
        let mut left = self.date_parts_present(ymd, diffs, depth)?;
        let mut right = self.date_parts_present(next, diffs, depth)?;
        let sames2 = self.date_parts_present(ymd, sames2, depth)?;
        let range_delimiter = diffs
            .iter()
            .min_by_key(|p| p.name)
            .map(|p| p.range_delimiter.clone());
        let open_range = next == [Some(0), None, None];
        if open_range {
            let mut parts = sames1;
            parts.extend(left);
            remove_last_suffix(&mut parts);
            return Ok(vec![Output::Formatted(
                Box::new(Formatting {
                    suffix: range_delimiter,
                    ..Formatting::default()
                }),
                parts,
            )]);
        }
        remove_last_suffix(&mut left);
        if let Some(Output::Formatted(f, _)) = right.first_mut() {
            f.prefix = None;
        }
        let mut out = sames1;
        match left.pop() {
            Some(last) => {
                out.extend(left);
                let mut right = right.into_iter();
                let first = right.next().unwrap_or(Output::Literal(Vec::new()));
                out.push(Output::Formatted(
                    Box::new(Formatting {
                        delimiter: range_delimiter,
                        ..Formatting::default()
                    }),
                    vec![last, first],
                ));
                out.extend(right);
            }
            None => out.append(&mut right),
        }
        out.extend(sames2);
        remove_last_suffix(&mut out);
        Ok(out)
    }

    /// The parts of a date that give output.
    fn date_parts_present(
        &mut self,
        ymd: Ymd,
        specs: &[DatePart],
        depth: usize,
    ) -> Result<Vec<Output>, EvalError> {
        let mut out = Vec::with_capacity(specs.len());
        for spec in specs {
            let part = self.date_part(ymd, spec, depth)?;
            if part != Output::Null {
                out.push(part);
            }
        }
        Ok(out)
    }

    /// `eDP`: one part of a date.
    fn date_part(&mut self, ymd: Ymd, spec: &DatePart, depth: usize) -> Result<Output, EvalError> {
        let is_year = spec.name == DatePartName::Year;
        let Some(n) = ymd[date_part_index(spec.name)] else {
            return Ok(Output::Null);
        };
        if n == 0 && is_year {
            // The end of an open range.
            return Ok(Output::Literal(Vec::new()));
        }
        let era = match n {
            _ if !is_year => None,
            n if n < 0 => Some("bc"),
            1..=999 => Some("ad"),
            _ => None,
        };
        let suffix = match era {
            Some(era) => self.term(&term_named(era), depth)?,
            None => Output::Null,
        };
        let n = if is_year { n.wrapping_abs() } else { n };
        let text = |text: String| Ok(Output::Literal(output::from_text(&text)));
        let main = match spec.form {
            DatePartForm::Numeric => text(n.to_string()),
            DatePartForm::NumericLeadingZeros => text(format!("{n:02}")),
            DatePartForm::Ordinal => {
                if self.locale.limit_day_ordinals_to_day_1 == Some(true) && n != 1 {
                    text(n.to_string())
                } else {
                    self.eval_number(NumberForm::Ordinal, None, &Val::Num(n), depth)
                }
            }
            form @ (DatePartForm::Long | DatePartForm::Short) => {
                let term = |name: String| Term {
                    form: if form == DatePartForm::Short {
                        TermForm::Short
                    } else {
                        TermForm::Long
                    },
                    ..term_named(&name)
                };
                match spec.name {
                    DatePartName::Month if n <= 0 => Ok(Output::Null),
                    DatePartName::Month if n <= 12 => {
                        self.term(&term(format!("month-{n:02}")), depth)
                    }
                    DatePartName::Month => {
                        let season = match n {
                            13..=16 => n - 12,
                            17..=20 => n - 16,
                            _ => n - 20,
                        };
                        self.term(&term(format!("season-{season:02}")), depth)
                    }
                    _ => text(n.to_string()),
                }
            }
        }?;
        Ok(output::formatted(
            spec.formatting.clone(),
            vec![main, suffix],
        ))
    }

    /// `eNames`: the names of the variables, or the first `substitute`
    /// element that gives output when all variables are empty.
    fn names(
        &mut self,
        variables: &[Variable],
        own_format: &NamesFormat,
        substitute: &[Element],
        element_formatting: &Formatting,
        depth: usize,
    ) -> Result<Output, EvalError> {
        let mut formatting = element_formatting.clone();
        if formatting.delimiter.is_none() {
            formatting.delimiter.clone_from(&self.name_format.delimiter);
        }
        let names_format = match &self.substitute_names_form {
            None => own_format.clone(),
            Some(outer) => NamesFormat {
                // `sort_DropNameLabelInSort.txt`.
                label: if self.in_sort_key {
                    None
                } else {
                    own_format.label.clone().or_else(|| outer.label.clone())
                },
                et_al: own_format.et_al.clone().or_else(|| outer.et_al.clone()),
                name: own_format.name.clone().or_else(|| outer.name.clone()),
                label_before_name: if own_format.name.is_some() && own_format.label.is_some() {
                    own_format.label_before_name
                } else {
                    outer.label_before_name
                },
            },
        };
        let mut names: Vec<&str> = variables.iter().map(Variable::as_str).collect();
        if names.contains(&"editor") && names.contains(&"translator") {
            let editor = self.ask_variable("editor");
            let translator = self.ask_variable("translator");
            let form = match &names_format.label {
                Some((form, _, _)) => *form,
                None => TermForm::Long,
            };
            let term = self.term(
                &Term {
                    form,
                    ..term_named("editortranslator")
                },
                depth,
            )?;
            if editor == translator && term != Output::Null {
                names.retain(|name| *name != "editor" && *name != "translator");
                names.insert(0, "editortranslator");
            }
        }
        let (name_format, name_formatting) = match &names_format.name {
            None => (self.name_format.clone(), Formatting::default()),
            Some((own, f)) => (own.combine(&self.name_format), f.clone()),
        };
        let name_formatting = name_formatting.combine(&Formatting {
            prefix: None,
            suffix: None,
            delimiter: None,
            ..formatting.clone()
        });
        let mut contributions = Vec::with_capacity(names.len());
        for name in names {
            let asked = if same(name, "editortranslator") {
                "editor"
            } else {
                name
            };
            contributions.push((name, self.ask_variable(asked)));
        }
        if contributions.iter().all(|(_, value)| value.is_none()) {
            if substitute.is_empty() || self.in_substitute {
                return Ok(Output::Null);
            }
            let outer_in = std::mem::replace(&mut self.in_substitute, true);
            let outer_form = self.substitute_names_form.replace(names_format.clone());
            let found = self.substitute(substitute, depth);
            self.in_substitute = outer_in;
            self.substitute_names_form = outer_form;
            let found = found?;
            return Ok(match found {
                Some(found @ Output::Tagged(Tag::Names { .. }, _)) => {
                    output::formatted(formatting, vec![found])
                }
                found => output::formatted(
                    formatting,
                    vec![Output::Tagged(
                        Tag::Names {
                            variable: String::new(),
                            format: Box::new(names_format),
                            names: Vec::new(),
                        },
                        Box::new(found.unwrap_or(Output::Null)),
                    )],
                ),
            });
        }
        let mut xs = Vec::with_capacity(contributions.len());
        let mut count = 0;
        for (name, value) in &contributions {
            let (x, shown) = self.format_names(
                &names_format,
                &name_format,
                &name_formatting,
                name,
                value.as_ref(),
                depth,
            )?;
            xs.push(x);
            count += shown;
        }
        for (name, value) in &contributions {
            if value.is_some() {
                self.delete_substituted(name);
            }
        }
        Ok(if name_format.form == Some(NameForm::Count) {
            Output::Literal(output::from_text(&count.to_string()))
        } else {
            output::formatted(
                Formatting {
                    prefix: formatting.prefix,
                    suffix: formatting.suffix,
                    delimiter: formatting.delimiter,
                    ..Formatting::default()
                },
                xs,
            )
        })
    }

    /// `eSubstitute`: the first output of the first element that gives one.
    fn substitute(
        &mut self,
        elements: &[Element],
        depth: usize,
    ) -> Result<Option<Output>, EvalError> {
        for element in elements {
            let found = self
                .element(element, depth)?
                .into_iter()
                .find(|x| *x != Output::Null);
            if found.is_some() {
                return Ok(found);
            }
        }
        Ok(None)
    }

    /// `formatNames`: the names of one variable with their delimiters, `and`,
    /// `et-al` and the label. The second value counts the names shown.
    fn format_names(
        &mut self,
        names_format: &NamesFormat,
        name_format: &NameFormat,
        formatting: &Formatting,
        variable: &str,
        value: Option<&Val<Vec<Inline>>>,
        depth: usize,
    ) -> Result<(Output, usize), EvalError> {
        let names = match value {
            None => return Ok((Output::Null, 0)),
            Some(Val::Names(names)) => names,
            Some(_) => {
                self.warnings
                    .insert(format!("ignoring non-name value for variable {variable}"));
                return Ok((Output::Null, 0));
            }
        };
        let mut formatted_names = Vec::with_capacity(names.len());
        for (order, name) in (1..).zip(names) {
            formatted_names.push(self.format_name(name_format, formatting, order, name));
        }
        let subsequent = !self.in_bibliography && self.positions.contains(&Position::Subsequent);
        let (et_al_min, et_al_use_first) = if subsequent {
            (
                name_format.et_al_subsequent_min.or(name_format.et_al_min),
                name_format
                    .et_al_subsequent_use_first
                    .or(name_format.et_al_use_first),
            )
        } else {
            (name_format.et_al_min, name_format.et_al_use_first)
        };
        let delimiter = formatting
            .delimiter
            .as_deref()
            .or(name_format.delimiter.as_deref())
            .unwrap_or(", ");
        let suffix_space = formatting.suffix.as_deref().is_some_and(begins_with_space);
        let prefix_space = formatting.prefix.as_deref().is_some_and(ends_with_space);
        let delimiter = match (suffix_space, prefix_space) {
            (true, true) => crate::haskell::strip(delimiter),
            (true, false) => delimiter.trim_start_matches(crate::haskell::is_space),
            (false, true) => delimiter.trim_end_matches(crate::haskell::is_space),
            (false, false) => delimiter,
        }
        .to_owned();
        let count = i64::try_from(names.len()).unwrap_or(i64::MAX);
        let label = match &names_format.label {
            Some((form, pluralize, label_formatting)) if !self.in_sort_key => {
                Some(self.label(variable, *form, *pluralize, label_formatting, depth)?)
            }
            _ => None,
        };
        let and_term = match name_format.and_style {
            Some(TermForm::Symbol) => Some(
                self.lookup_term(&Term {
                    form: TermForm::Symbol,
                    ..term_named("and")
                })
                .unwrap_or("&")
                .to_owned(),
            ),
            Some(_) => self.lookup_term(&term_named("and")).map(str::to_owned),
            None => None,
        };
        let final_is_others = names
            .last()
            .is_some_and(|name| name.literal.as_deref() == Some("others"));
        let use_last = name_format.et_al_use_last.unwrap_or(false);
        let threshold = match et_al_min {
            Some(min) if count >= min => {
                match (self.disamb.and_then(|d| d.et_al_names), et_al_use_first) {
                    (Some(n), Some(m)) => Some(m.max(n)),
                    (_, y) => y,
                }
            }
            Some(_) if final_is_others => Some(count - 1),
            _ => None,
        };
        let inverted = |i: i64| {
            usize::try_from(i - 1)
                .ok()
                .and_then(|i| names.get(i))
                .is_some_and(|name| name.family.is_some())
                && match name_format.as_sort_order {
                    Some(NameAsSortOrder::All) => true,
                    Some(NameAsSortOrder::First) => i == 1,
                    None => false,
                }
        };
        let before_last = match and_term {
            None => delimiter.as_str(),
            Some(_) => {
                let precedes = name_format
                    .delimiter_precedes_last
                    .unwrap_or(DelimiterPrecedes::Contextual);
                let with_delimiter = match precedes {
                    DelimiterPrecedes::Contextual => count > 2,
                    DelimiterPrecedes::AfterInvertedName => inverted(count - 1),
                    DelimiterPrecedes::Always => true,
                    DelimiterPrecedes::Never => false,
                };
                if with_delimiter {
                    delimiter.as_str()
                } else {
                    ""
                }
            }
        };
        let suffix_ends_space = formatting.suffix.as_deref().is_some_and(ends_with_space);
        let and_pre_space = if before_last.is_empty() {
            if suffix_ends_space { "" } else { " " }
        } else if ends_with_space(before_last) {
            ""
        } else {
            " "
        };
        let and_post_space = if formatting.prefix.as_deref().is_some_and(begins_with_space) {
            ""
        } else {
            " "
        };
        let and_delimiter = and_term
            .map(|term| format!("{and_pre_space}{term}{and_post_space}"))
            .unwrap_or_default();
        let et_al_pre_space = if suffix_ends_space { "" } else { " " };
        let before_et_al = match name_format
            .delimiter_precedes_et_al
            .unwrap_or(DelimiterPrecedes::Contextual)
        {
            DelimiterPrecedes::Contextual if count > 2 && threshold.is_some_and(|t| t > 1) => {
                delimiter.as_str()
            }
            DelimiterPrecedes::AfterInvertedName if threshold.is_some_and(inverted) => {
                delimiter.as_str()
            }
            DelimiterPrecedes::Always => delimiter.as_str(),
            _ => et_al_pre_space,
        }
        .to_owned();
        let et_al_term = match &names_format.et_al {
            Some((term, _)) => term.as_str(),
            None => "et-al",
        };
        let et_al = match &names_format.et_al {
            Some((term, f)) => {
                let prefix = format!("{before_et_al}{}", f.prefix.as_deref().unwrap_or(""));
                let f = Formatting {
                    prefix: Some(remove_double_spaces(prefix)),
                    ..f.clone()
                };
                let term = self.term(&term_named(term), depth)?;
                self.with_formatting(&f, term)
            }
            None if use_last && !final_is_others => Output::Formatted(
                Box::new(Formatting {
                    prefix: Some(before_et_al),
                    ..Formatting::default()
                }),
                vec![Output::Literal(output::from_text("\u{2026} "))],
            ),
            None => Output::Formatted(
                Box::new(Formatting {
                    prefix: Some(before_et_al),
                    ..Formatting::default()
                }),
                vec![self.term(&term_named("et-al"), depth)?],
            ),
        };
        let et_al = Output::Tagged(Tag::Term(Box::new(term_named(et_al_term))), Box::new(et_al));
        let with_prefix = |prefix: String, name: Output| {
            output::formatted(
                Formatting {
                    prefix: Some(prefix),
                    ..Formatting::default()
                },
                vec![name],
            )
        };
        let mut shown = 0;
        let mut parts = Vec::with_capacity(formatted_names.len() + 1);
        for (index, name) in (1..).zip(formatted_names) {
            self.budget.step()?;
            let after_threshold = |by: i64| threshold.is_some_and(|t| index - 1 - by >= t);
            let part = if threshold == Some(0) {
                Output::Null
            } else if index == 1 {
                shown += 1;
                name
            } else if threshold == Some(index - 1) {
                if self.in_sort_key {
                    Output::Null
                } else {
                    et_al.clone()
                }
            } else if index == count && use_last && !final_is_others && after_threshold(0) {
                shown += 1;
                name
            } else if after_threshold(1) {
                Output::Null
            } else if self.in_sort_key {
                shown += 1;
                name
            } else if index == count {
                shown += 1;
                with_prefix(format!("{before_last}{and_delimiter}"), name)
            } else {
                shown += 1;
                with_prefix(delimiter.clone(), name)
            };
            parts.push(part);
        }
        if let Some(label) = label {
            if names_format.label_before_name {
                parts.insert(0, label);
            } else {
                parts.push(label);
            }
        }
        Ok((
            Output::Tagged(
                Tag::Names {
                    variable: variable.to_owned(),
                    format: Box::new(names_format.clone()),
                    names: names.clone(),
                },
                Box::new(output::formatted(Formatting::default(), parts)),
            ),
            shown,
        ))
    }

    /// `formatName`: a literal name, or the parts of a personal name.
    fn format_name(
        &self,
        name_format: &NameFormat,
        formatting: &Formatting,
        order: i64,
        name: &Name,
    ) -> Output {
        let hinted;
        let hint = self.disamb.and_then(|d| d.name_map.get(name));
        let name_format = match hint {
            None => name_format,
            Some(hint) => {
                let primary = order == 1;
                let (form, full) = match hint {
                    NameHint::AddInitials => (true, false),
                    NameHint::AddInitialsIfPrimary => (primary, false),
                    NameHint::AddGivenName => (true, true),
                    NameHint::AddGivenNameIfPrimary => (primary, primary),
                };
                hinted = NameFormat {
                    form: if form {
                        Some(NameForm::Long)
                    } else {
                        name_format.form
                    },
                    initialize: if full {
                        Some(false)
                    } else {
                        name_format.initialize
                    },
                    ..name_format.clone()
                };
                &hinted
            }
        };
        let output = match &name.literal {
            Some(text) => {
                let literal = Output::Literal(output::from_text(text));
                output::formatted(
                    formatting.clone(),
                    vec![match &name_format.family_formatting {
                        Some(f) => Output::Formatted(Box::new(f.clone()), vec![literal]),
                        None => literal,
                    }],
                )
            }
            None => self.display_name(name_format, formatting, order, name),
        };
        Output::Tagged(Tag::Name(Box::new(name.clone())), Box::new(output))
    }

    /// `getDisplayName`: the parts of a personal name in the order of the
    /// name options.
    fn display_name(
        &self,
        name_format: &NameFormat,
        formatting: &Formatting,
        order: i64,
        name: &Name,
    ) -> Output {
        let options = &self.style.options;
        step();
        let byzantine = name.family.as_deref().is_some_and(output::is_byzantine);
        let lang = self.locale.language.as_ref();
        let separator = name_format.sort_separator.as_deref().unwrap_or(", ");
        let join = |x: Output, y: Output| join_space(byzantine, x, y);
        let sort_join = |x: Output, y: Output| join_separator(separator, x, y);
        let affixes = |f: &Option<Formatting>, parts: Vec<Output>| {
            let f = f.as_ref().map_or_else(Formatting::default, |f| Formatting {
                prefix: f.prefix.clone(),
                suffix: f.suffix.clone(),
                ..Formatting::default()
            });
            output::formatted(f, parts)
        };
        let part_formatting = |f: &Option<Formatting>, text: &Option<String>, initialize: bool| {
            let Some(text) = text else {
                return Output::Null;
            };
            let text = match (&name.family, &name_format.initialize_with, initialize) {
                (Some(_), Some(with), true) => initialize_name(
                    lang,
                    name_format.initialize.unwrap_or(true),
                    options.initialize_with_hyphen,
                    with,
                    text,
                ),
                _ => text.clone(),
            };
            let f = f.as_ref().map_or_else(Formatting::default, |f| Formatting {
                prefix: None,
                suffix: None,
                ..f.clone()
            });
            output::formatted(f, vec![Output::Literal(output::from_text(&text))])
        };
        let family_affixes = |parts| affixes(&name_format.family_formatting, parts);
        let given_affixes = |parts| affixes(&name_format.given_formatting, parts);
        let non_dropping = || {
            part_formatting(
                &name_format.family_formatting,
                &name.non_dropping_particle,
                false,
            )
        };
        let dropping = || {
            part_formatting(
                &name_format.given_formatting,
                &name.dropping_particle,
                false,
            )
        };
        let given = || part_formatting(&name_format.given_formatting, &name.given, true);
        let family = || part_formatting(&name_format.family_formatting, &name.family, false);
        let suffix = || match &name.suffix {
            Some(text) => Output::Literal(output::from_text(text)),
            None => Output::Null,
        };
        let as_sort = match name_format.as_sort_order {
            Some(NameAsSortOrder::All) => true,
            Some(NameAsSortOrder::First) => order == 1,
            None => false,
        };
        let form = name_format.form.unwrap_or(NameForm::Long);
        let demote = options.demote_non_dropping_particle;
        let sort_key = self.in_sort_key;
        let output = if byzantine {
            match form {
                NameForm::Long
                    if (demote == DemoteNonDroppingParticle::Never && (sort_key || as_sort))
                        || (demote == DemoteNonDroppingParticle::SortOnly
                            && !sort_key
                            && as_sort) =>
                {
                    sort_join(
                        sort_join(
                            family_affixes(vec![join(non_dropping(), family())]),
                            given_affixes(vec![join(given(), dropping())]),
                        ),
                        suffix(),
                    )
                }
                NameForm::Long
                    if (demote == DemoteNonDroppingParticle::SortOnly && sort_key)
                        || (demote == DemoteNonDroppingParticle::DisplayAndSort
                            && (sort_key || as_sort)) =>
                {
                    sort_join(
                        sort_join(
                            family_affixes(vec![family()]),
                            given_affixes(vec![join(join(given(), dropping()), non_dropping())]),
                        ),
                        suffix(),
                    )
                }
                NameForm::Long if name.comma_suffix => join(
                    given_affixes(vec![given()]),
                    family_affixes(vec![sort_join(
                        join(join(dropping(), non_dropping()), family()),
                        suffix(),
                    )]),
                ),
                NameForm::Long => join(
                    given_affixes(vec![given()]),
                    family_affixes(vec![join(
                        join(join(dropping(), non_dropping()), family()),
                        suffix(),
                    )]),
                ),
                NameForm::Short => family_affixes(vec![join(non_dropping(), family())]),
                NameForm::Count => Output::Null,
            }
        } else {
            match form {
                NameForm::Long => output::formatted(
                    Formatting::default(),
                    vec![family_affixes(vec![family()]), given_affixes(vec![given()])],
                ),
                NameForm::Short => family_affixes(vec![family()]),
                NameForm::Count => Output::Null,
            }
        };
        output::formatted(formatting.clone(), vec![output])
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
            let formatted = format_page_range(format, delimiter, crate::haskell::strip(range));
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

    /// `lookupTerm`: the text of the first matching term of the locale.
    fn lookup_term(&self, term: &Term) -> Option<&'a str> {
        lookup_term_entries(self.locale, term)
            .next()
            .map(|(_, text)| text.as_str())
    }

    /// `lookupTerm'`: a missing form falls back to another form.
    fn term(&self, term: &Term, depth: usize) -> Result<Output, EvalError> {
        let mut term = term.clone();
        loop {
            match self.lookup_term(&term).into_iter().next() {
                Some("") => return Ok(Output::Null),
                Some(text) => {
                    return Ok(Output::Literal(output::parse_csl_json(
                        self.locale,
                        text,
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

    /// The test of a `choose` branch.
    fn matches(&self, matching: Match, conditions: &[Condition]) -> bool {
        let test = |condition: &Condition| match condition {
            Condition::HasVariable(v) if same(v.as_str(), "locator") => self.locator.is_some(),
            Condition::HasVariable(v) => match self.find(v.as_str()) {
                Some(Found::Value(x)) => is_nonempty(&x),
                Some(Found::Substituted) => true,
                None => false,
            },
            Condition::HasType(types) => {
                !self.reference.kind.is_empty() && types.contains(&self.reference.kind)
            }
            Condition::IsUncertainDate(v) => matches!(
                self.find(v.as_str()),
                Some(Found::Value(Val::Date(date))) if date.circa
            ),
            Condition::IsNumeric(v) if same(v.as_str(), "locator") => {
                self.locator.is_some_and(is_numeric)
            }
            Condition::IsNumeric(v) => match self.find(v.as_str()) {
                Some(Found::Value(Val::Num(_))) => true,
                Some(Found::Value(Val::Text(x))) => is_numeric(&x),
                Some(Found::Value(Val::Fancy(x))) => is_numeric(&output::stringify(&x)),
                _ => false,
            },
            Condition::HasLocatorType(t) => match self.label {
                Some("sub verbo") => same(t.as_str(), "sub-verbo"),
                Some(label) => crate::types::Variable::new(label) == *t,
                None => same(t.as_str(), "page") && self.locator.is_some(),
            },
            Condition::HasPosition(position) => self.positions.contains(position),
            Condition::WouldDisambiguate => self.disamb.is_some_and(|d| d.condition),
        };
        match matching {
            Match::All => conditions.iter().all(test),
            Match::Any => conditions.iter().any(test),
            Match::None => !conditions.iter().any(test),
        }
    }
}

/// `<+>` of `getDisplayName`: two name parts with a space between them,
/// except after an apostrophe, a hyphen, an en dash or a no-break space.
fn join_space(byzantine: bool, mut x: Output, y: Output) -> Output {
    if y == Output::Null {
        return x;
    }
    if matches!(x, Output::Linked(..)) {
        return output::formatted(
            Formatting {
                delimiter: Some(" ".into()),
                ..Formatting::default()
            },
            vec![x, y],
        );
    }
    match &mut x {
        Output::Null => y,
        Output::Literal(x) => {
            let x = std::mem::take(x);
            let joined = output::stringify(&x)
                .chars()
                .last()
                .is_some_and(|c| matches!(c, '\u{2019}' | '\'' | '-' | '\u{2013}' | '\u{a0}'));
            let delimiter = (byzantine && !joined).then(|| " ".to_owned());
            output::formatted(
                Formatting {
                    delimiter,
                    ..Formatting::default()
                },
                vec![Output::Literal(x), y],
            )
        }
        Output::Formatted(f, x) => {
            let f = std::mem::take(&mut **f);
            let x = std::mem::take(x);
            let delimiter =
                (!f.suffix.as_deref().is_some_and(ends_with_space)).then(|| " ".to_owned());
            output::formatted(
                Formatting {
                    delimiter,
                    ..Formatting::default()
                },
                vec![output::formatted(f, x), y],
            )
        }
        Output::Linked(..) => Output::Null,
        Output::Tagged(_, x) | Output::InNote(x) => {
            join_space(byzantine, std::mem::replace(&mut **x, Output::Null), y)
        }
    }
}

/// `<:>` of `getDisplayName`: two name parts with the sort separator.
fn join_separator(separator: &str, mut x: Output, y: Output) -> Output {
    match (&mut x, y) {
        (_, Output::Null) => x,
        (Output::Null, y) => y,
        (Output::Tagged(_, inner) | Output::InNote(inner), y) => {
            join_separator(separator, std::mem::replace(&mut **inner, Output::Null), y)
        }
        (_, y) => output::formatted(
            Formatting {
                delimiter: Some(separator.to_owned()),
                ..Formatting::default()
            },
            vec![x, y],
        ),
    }
}

/// A word of a given name: `Left` upstream when it is an initial already.
enum Word {
    Initial(String),
    Full(String),
}

/// `initialize`: the given name as initials with `initialize-with` after
/// each one.
pub(crate) fn initialize_name(
    lang: Option<&crate::lang::Lang>,
    make_initials: bool,
    use_hyphen: bool,
    with: &str,
    text: &str,
) -> String {
    let turkish = lang.is_some_and(|lang| lang.language == "tr");
    let mut words = Vec::new();
    let mut current = String::new();
    let take = |current: &mut String| std::mem::take(current);
    for c in text.chars() {
        step();
        match c {
            '.' if current.is_empty() => {}
            '.' => words.push(Word::Initial(take(&mut current))),
            '-' if current.is_empty() => current.push('-'),
            '-' => {
                words.push(Word::Full(take(&mut current)));
                current.push('-');
            }
            ' ' if current.is_empty() => {}
            ' ' if current.chars().count() == 1 => words.push(Word::Initial(take(&mut current))),
            ' ' => words.push(Word::Full(take(&mut current))),
            c => current.push(c),
        }
    }
    match current.chars().count() {
        0 => {}
        1 => words.push(Word::Initial(current)),
        _ => words.push(Word::Full(current)),
    }
    let add_suffix = |t: String| if t.is_empty() { t } else { t + with };
    let caseless = |c: char| {
        is_letter(c)
            && !is_upper(c)
            && !is_lower(c)
            && output::is_byzantine(c.encode_utf8(&mut [0; 4]))
    };
    let to_initial = |t: &str| -> String {
        let mut chars = t.chars();
        match chars.next() {
            Some('-') => match chars.next() {
                Some(c) if is_upper(c) && use_hyphen => {
                    format!("-{}", to_upper(turkish, c.encode_utf8(&mut [0; 4])))
                }
                Some(c) if is_upper(c) => to_upper(turkish, c.encode_utf8(&mut [0; 4])),
                _ => String::new(),
            },
            Some(c) if caseless(c) => c.to_string(),
            Some(c) if is_upper(c) => {
                let rest = chars.as_str();
                let mut tail = rest.chars();
                match tail.next() {
                    Some(d)
                        if is_upper(d)
                            && !tail.as_str().is_empty()
                            && tail.as_str().chars().all(is_lower) =>
                    {
                        format!("{c}{}", to_lower(turkish, d.encode_utf8(&mut [0; 4])))
                    }
                    _ => c.to_string(),
                }
            }
            _ => t.to_owned(),
        }
    };
    let mut out = String::new();
    for word in words {
        step();
        match word {
            Word::Initial(t) => out.push_str(&add_suffix(t)),
            Word::Full(t) if t.chars().all(is_lower) => {
                if ends_with_space(with) {
                    out.push_str(&t);
                    out.push(' ');
                } else {
                    out.push(' ');
                    out.push_str(&t);
                    out.push(' ');
                }
            }
            Word::Full(t) if !output::is_byzantine(&t) || !make_initials => {
                out.push_str(&t);
                out.push(' ');
            }
            Word::Full(t) => out.push_str(&add_suffix(to_initial(&t))),
        }
    }
    out.replace(" -", "-").trim_matches(' ').to_owned()
}

/// `showYearSuffix`: 1 is `a`, 26 is `z`, 27 is `aa` (bijective base 26).
pub(crate) fn show_year_suffix(n: i64) -> String {
    let mut letters = Vec::new();
    let mut x = n;
    while x >= 1 {
        let r = (x - 1).rem_euclid(26);
        x = (x - 1).div_euclid(26);
        letters.push(char::from(b'a' + u8::try_from(r).unwrap_or(0)));
    }
    letters.iter().rev().collect()
}

/// `formatSortDate`: the parts of the date that the element shows, as
/// digits that sort (the year plus 5000).
fn format_sort_date(specs: &[DatePart], ymd: Ymd) -> Output {
    let has = |name: DatePartName| specs.iter().any(|p| p.name == name);
    let mut text = String::new();
    if let (Some(year), true) = (ymd[0], has(DatePartName::Year)) {
        text.push_str(&format!("{:04}", year.saturating_add(5000)));
    }
    if let (Some(month), true) = (ymd[1], has(DatePartName::Month)) {
        text.push_str(&format!("{month:02}"));
    }
    if let (Some(day), true) = (ymd[2], has(DatePartName::Day)) {
        text.push_str(&format!("{day:02}"));
    }
    Output::Literal(output::from_text(&text))
}

/// `removeDoubleSpaces`: each run of spaces becomes one space.
fn remove_double_spaces(text: String) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if !(c == ' ' && out.ends_with(' ')) {
            out.push(c);
        }
    }
    out
}

/// `endsWithSpace`.
fn ends_with_space(text: &str) -> bool {
    text.chars().last().is_some_and(crate::haskell::is_space)
}

/// `beginsWithSpace`.
fn begins_with_space(text: &str) -> bool {
    text.chars().next().is_some_and(crate::haskell::is_space)
}

/// The place of a date part in [`Ymd`].
fn date_part_index(name: DatePartName) -> usize {
    match name {
        DatePartName::Year => 0,
        DatePartName::Month => 1,
        DatePartName::Day => 2,
    }
}

/// `bindDateParts`.
fn bind_date_parts(date: Option<&DateParts>) -> Ymd {
    match date.map(|d| d.0.as_slice()) {
        Some([y, m, d, ..]) => [Some(*y), Some(*m), Some(*d)],
        Some([y, m]) => [Some(*y), Some(*m), None],
        Some([y]) => [Some(*y), None, None],
        _ => [None; 3],
    }
}

/// `removeLastSuffix`: the last part loses its suffix when it is formatted.
fn remove_last_suffix(parts: &mut [Output]) {
    if let Some(Output::Formatted(f, _)) = parts.last_mut() {
        f.suffix = None;
    }
}

/// `lookupTerm`: the terms of the locale that match, in order. The callers
/// take the first match (Codex citeproc-R3b-07: no copy of the others).
fn lookup_term_entries<'l>(
    locale: &'l Locale,
    term: &Term,
) -> impl Iterator<Item = &'l (Term, String)> + use<'l> {
    let name = if term.name == "sub verbo" {
        "sub-verbo"
    } else {
        term.name.as_str()
    };
    let wanted = Term {
        name: name.to_owned(),
        ..term.clone()
    };
    locale
        .terms
        .get(name)
        .into_iter()
        .flatten()
        .filter(move |(found, _)| {
            step();
            term_matches(&wanted, found)
        })
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
pub(crate) fn lookup_variable(
    reference: &Reference<Vec<Inline>>,
    name: &str,
) -> Option<Val<Vec<Inline>>> {
    match name {
        _ if same(name, "id") => {
            (!reference.id.is_empty()).then(|| Val::Text(reference.id.clone()))
        }
        _ if same(name, "type") => {
            (!reference.kind.is_empty()).then(|| Val::Text(reference.kind.clone()))
        }
        _ => {
            let found = reference
                .variables
                .get(&reference::Variable::new(name))
                .cloned();
            if !same(name, "page-first") || found.is_some() {
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
    if same(variable, "number-of-volumes") && text != "1" && text != "0" {
        return TermNumber::Plural;
    }
    if text.contains("\\-") {
        return TermNumber::Singular;
    }
    let ungrouped = if same(variable, "page") || same(variable, "locator") {
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
        // `isNonEmpty`: `circa` alone does not count (Codex citeproc-R3b-03).
        Val::Date(date) => {
            !(date.parts.is_empty() && date.season.is_none() && date.literal.is_none())
        }
        Val::Num(_) => true,
    }
}

/// `valToText`.
pub(crate) fn val_to_text(value: &Val<Vec<Inline>>) -> Option<String> {
    match value {
        Val::Text(t) => Some(t.clone()),
        Val::Fancy(x) => Some(output::stringify(x)),
        Val::Num(n) => Some(n.to_string()),
        Val::Names(_) | Val::Date(_) => None,
    }
}

/// A variable name equals a folded name, with the full case folding of
/// `CI Text`.
fn same(name: &str, key: &str) -> bool {
    if name.is_ascii() {
        name.eq_ignore_ascii_case(key)
    } else {
        crate::case_fold::fold(name) == key
    }
}

/// `identifierToURL`.
pub(crate) fn identifier_url(variable: &str, text: &str) -> String {
    let prefix = match variable {
        "doi" => "https://doi.org/",
        "pmcid" => "https://www.ncbi.nlm.nih.gov/pmc/articles/",
        "pmid" => "https://www.ncbi.nlm.nih.gov/pubmed/",
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Citation, CitationItem, EvalBudget, EvalError, STEPS, eval_citation};
    use crate::output::{fix_punct, from_text};
    use crate::reference::{Name, Reference, Val, Variable};

    fn person(given: &str) -> Name {
        Name {
            family: Some("F".into()),
            given: Some(given.into()),
            ..Name::default()
        }
    }

    /// The steps of the citation of one reference with `variables` in a style
    /// whose citation layout is `layout`.
    fn steps_of(layout: &str, variables: Vec<(&str, Val<Vec<randoc_ast::Inline>>)>) -> usize {
        let style = crate::parse_style(
            &mut randoc_core::Context::pure(),
            &format!(
                "<style xmlns=\"http://purl.org/net/xbiblio/csl\"><locale><terms><term name=\"and\">and</term><term name=\"et-al\">et al.</term></terms></locale><citation><layout>{layout}</layout></citation></style>"
            ),
        )
        .unwrap();
        let reference = Reference {
            id: "a".into(),
            kind: "book".into(),
            variables: variables
                .into_iter()
                .map(|(k, v)| (Variable::new(k), v))
                .collect::<BTreeMap<_, _>>(),
        };
        STEPS.with(|steps| steps.set(0));
        let citation = Citation {
            items: vec![CitationItem::new("a", Some(&reference))],
            ..Citation::default()
        };
        eval_citation(&style, &style.locales[0], &citation, 128).unwrap();
        STEPS.with(std::cell::Cell::get)
    }

    #[test]
    fn evaluation_stops_when_the_work_budget_is_used() {
        let mut budget = EvalBudget {
            remaining: 2,
            deadline: None,
        };
        assert_eq!(budget.step(), Ok(()));
        assert_eq!(budget.step(), Ok(()));
        assert_eq!(budget.step(), Err(EvalError::WorkLimit));
    }

    #[test]
    fn evaluation_stops_at_an_expired_deadline() {
        let mut budget = EvalBudget::new(Some(std::time::Instant::now()));
        assert_eq!(budget.step(), Err(EvalError::Deadline));
    }

    #[test]
    fn names_evaluate_in_linear_steps() {
        type Class = (&'static str, fn(usize) -> usize);
        let classes: [Class; 4] = [
            ("authors", |n| {
                steps_of(
                    "<names variable=\"author\"><name and=\"text\" initialize-with=\". \" name-as-sort-order=\"all\"/></names>",
                    vec![("author", Val::Names(vec![person("G H"); n]))],
                )
            }),
            ("authors with et al. and the last name", |n| {
                steps_of(
                    "<names variable=\"author\"><name et-al-min=\"3\" et-al-use-first=\"2\" et-al-use-last=\"true\"/></names>",
                    vec![("author", Val::Names(vec![person("G"); n]))],
                )
            }),
            ("words of a given name", |n| {
                steps_of(
                    "<names variable=\"author\"><name initialize-with=\".\"/></names>",
                    vec![("author", Val::Names(vec![person(&"Ab-cd E. f ".repeat(n))]))],
                )
            }),
            ("substitute elements", |n| {
                steps_of(
                    &format!(
                        "<names variable=\"author\"><substitute>{}<text variable=\"title\"/></substitute></names>",
                        "<names variable=\"editor\"/><text variable=\"note\"/>".repeat(n)
                    ),
                    vec![("title", Val::Text("T".into()))],
                )
            }),
        ];
        for (name, steps) in classes {
            assert_linear(name, steps);
        }
    }

    fn assert_linear(name: &str, steps: fn(usize) -> usize) {
        {
            let (small, large) = (steps(1_000), steps(4_000));
            assert!(small >= 1_000, "{name}: {small} steps");
            assert!(large <= 5 * small, "{name}: {small} then {large} steps");
        }
    }

    /// The steps of `fixPunct` on `x` then `y` (Codex citeproc-R3b-06).
    fn punct_steps(x: &str, y: &str) -> usize {
        STEPS.with(|steps| steps.set(0));
        let fixed = fix_punct(vec![from_text(x), from_text(y)]);
        assert!(!fixed.is_empty());
        STEPS.with(std::cell::Cell::get)
    }

    #[test]
    fn punctuation_and_terms_take_linear_steps() {
        let fixed = fix_punct(vec![from_text(&";:".repeat(3)), from_text("!")]);
        assert_eq!(fixed, vec![Vec::new(), from_text("!")]);
        let fixed = fix_punct(vec![from_text("a!"), from_text(&".:".repeat(3))]);
        assert_eq!(fixed, vec![from_text("a!"), Vec::new()]);
        type Class = (&'static str, fn(usize) -> usize);
        let classes: [Class; 3] = [
            ("end punctuation before a mark", |n| {
                punct_steps(&";:".repeat(n), "!")
            }),
            ("start punctuation after a mark", |n| {
                punct_steps("a!", &".:".repeat(n))
            }),
            ("terms with one name, used n times", |n| {
                // Codex citeproc-R3b-07: n equal terms and n uses.
                let terms = "<term name=\"x\">x</term>".repeat(n);
                let style = crate::parse_style(
                    &mut randoc_core::Context::pure(),
                    &format!(
                        "<style xmlns=\"http://purl.org/net/xbiblio/csl\"><locale><terms>{terms}</terms></locale><citation><layout>{}</layout></citation></style>",
                        "<text term=\"x\"/>".repeat(n)
                    ),
                )
                .unwrap();
                let reference = Reference {
                    id: "a".into(),
                    kind: "book".into(),
                    variables: BTreeMap::new(),
                };
                STEPS.with(|steps| steps.set(0));
                let citation = Citation {
                    items: vec![CitationItem::new("a", Some(&reference))],
                    ..Citation::default()
                };
                eval_citation(&style, &style.locales[0], &citation, 128).unwrap();
                STEPS.with(std::cell::Cell::get)
            }),
        ];
        for (name, steps) in classes {
            assert_linear(name, steps);
        }
    }
}
