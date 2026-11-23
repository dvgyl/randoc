//! The evaluation of a whole document: a port of `citeproc` (`Citeproc`),
//! `evalStyle`, `evalLayout`, `evalItem`, the sort keys and the cite history
//! of `Citeproc.Eval` (`citeproc` 0.14.1.1).
//!
//! [`citeproc`] takes the references and the citations of a document and
//! gives the inlines of each citation and of each bibliography entry.
//! [`evaluate`] gives the output trees before rendering.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};

use randoc_ast::Inline;

use crate::abbreviations::Abbreviations;
use crate::collate::Collator;
use crate::eval::{self, DisambiguationData, EvalError, ItemContext};
use crate::lang::Lang;
use crate::output::{self, Output, RenderOptions, Tag};
use crate::reference::{self, Reference, Val};
use crate::sort::{self, SortKeyValue};
use crate::types::{
    Formatting, ItemType, Layout, Locale, NameFormat, Position, SortDirection, SortKey, Style,
};

/// One cited item (`CitationItem`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CitationItem {
    /// The identifier of the item.
    pub id: String,
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

impl CitationItem {
    /// A normal item with no locator and no affixes (`basicItem`).
    pub fn new(id: &str) -> CitationItem {
        CitationItem {
            id: id.to_owned(),
            ..CitationItem::default()
        }
    }
}

/// One citation (`Citation`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Citation {
    /// An identifier of the citation.
    pub id: Option<String>,
    /// The cite positions start again with this citation.
    pub reset_position: bool,
    /// The number of the note that holds the citation.
    pub note_number: Option<i64>,
    /// The text before the first item.
    pub prefix: Option<Vec<Inline>>,
    /// The text after the last item.
    pub suffix: Option<Vec<Inline>>,
    /// The cited items.
    pub items: Vec<CitationItem>,
}

/// The input of an evaluation.
pub struct Input<'a> {
    /// The style.
    pub style: &'a Style,
    /// The merged locale of the style.
    pub locale: &'a Locale,
    /// The language of the document: it decides the sort order.
    pub lang: Option<&'a Lang>,
    /// The abbreviations for short forms.
    pub abbreviations: Option<&'a Abbreviations>,
    /// The references. Of two references with one identifier, the last wins.
    pub references: Vec<Reference<Vec<Inline>>>,
    /// The citations of the document, in document order.
    pub citations: &'a [Citation],
    /// The depth limit of the output trees.
    pub max_depth: usize,
}

/// The output trees of an evaluation (`evalStyle`).
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluated {
    /// One output for each citation of the input.
    pub citations: Vec<Output>,
    /// The bibliography entries with their identifiers, in bibliography order.
    pub bibliography: Vec<(String, Output)>,
    /// The warnings, in code point order.
    pub warnings: Vec<String>,
}

/// The result of [`citeproc`] (`Result`).
#[derive(Debug, Clone, PartialEq)]
pub struct CiteprocResult {
    /// The inlines of each citation.
    pub citations: Vec<Vec<Inline>>,
    /// The bibliography entries with their identifiers.
    pub bibliography: Vec<(String, Vec<Inline>)>,
    /// The warnings.
    pub warnings: Vec<String>,
}

/// `citeproc`: evaluates the document and renders each citation and each
/// bibliography entry.
pub fn citeproc(options: RenderOptions, input: Input) -> Result<CiteprocResult, EvalError> {
    let locale = input.locale;
    let max_depth = input.max_depth;
    let citations_in = input.citations;
    let evaluated = evaluate(input)?;
    let render = |options: RenderOptions, x: &Output| -> Result<Vec<Inline>, EvalError> {
        let inlines = output::render_output(options, locale, x, max_depth)?;
        Ok(output::finish(locale, inlines, max_depth)?)
    };
    let mut citations = Vec::with_capacity(evaluated.citations.len());
    for x in &evaluated.citations {
        citations.push(render(options, x)?);
    }
    let bib_options = RenderOptions {
        link_citations: false,
        ..options
    };
    let mut bibliography = Vec::with_capacity(evaluated.bibliography.len());
    for (id, x) in &evaluated.bibliography {
        bibliography.push((id.clone(), render(bib_options, x)?));
    }
    let mut no_form = BTreeSet::new();
    for (citation, inlines) in citations_in.iter().zip(&citations) {
        if inlines.is_empty() {
            let ids: Vec<&str> = citation.items.iter().map(|i| i.id.as_str()).collect();
            no_form.insert(format!("Citation with no printed form: {}", ids.join(",")));
        }
    }
    for (id, inlines) in &bibliography {
        if inlines.is_empty() {
            no_form.insert(format!("Bibliography entry with no printed form: {id}"));
        }
    }
    let mut warnings = evaluated.warnings;
    warnings.extend(no_form);
    Ok(CiteprocResult {
        citations,
        bibliography,
        warnings,
    })
}

/// An earlier item of the cite history (`stateLastCitedMap`).
#[derive(Debug, Clone)]
pub(crate) struct Cited {
    group: i64,
    note: Option<i64>,
    position_in_group: i64,
    alone_in_group: bool,
    label: Option<String>,
    locator: Option<String>,
}

/// How a layout evaluation finds the positions of its items.
pub(crate) enum Positions<'p> {
    /// From the cite history, which the evaluation updates.
    History,
    /// The positions of an earlier evaluation of the same citation: they
    /// depend only on the citations, which disambiguation does not change.
    Replay(&'p [Vec<Position>]),
    /// The bibliography: no positions and no update.
    Bibliography,
}

/// The state of an evaluation (`EvalState` and `Context`).
pub(crate) struct Engine<'a> {
    pub style: &'a Style,
    pub locale: &'a Locale,
    pub collator: Collator,
    pub abbreviations: Option<&'a Abbreviations>,
    /// The references, after the rule that the last one with an identifier wins.
    pub refs: Vec<Reference<Vec<Inline>>>,
    pub index: HashMap<String, usize>,
    pub disamb: HashMap<String, DisambiguationData>,
    last_cited: HashMap<String, Cited>,
    notes: HashMap<i64, HashSet<String>>,
    pub warnings: BTreeSet<String>,
    pub max_depth: usize,
}

/// `evalStyle`: the output trees of the citations and of the bibliography.
pub fn evaluate(input: Input) -> Result<Evaluated, EvalError> {
    let style = input.style;
    let (refs, index) = reference_map(input.references);
    let mut engine = Engine {
        style,
        locale: input.locale,
        collator: Collator::for_lang(input.lang),
        abbreviations: input.abbreviations,
        refs,
        index,
        disamb: HashMap::new(),
        last_cited: HashMap::new(),
        notes: HashMap::new(),
        warnings: BTreeSet::new(),
        max_depth: input.max_depth,
    };
    let citations = input.citations;
    let mut order: HashMap<&str, usize> = HashMap::new();
    for (n, item) in citations.iter().flat_map(|c| &c.items).enumerate() {
        order.entry(item.id.as_str()).or_insert(n);
    }
    let mut sorted_cite_ids: Vec<String> = engine.refs.iter().map(|r| r.id.clone()).collect();
    sorted_cite_ids.sort_by_key(|id| order.get(id.as_str()).copied().unwrap_or(usize::MAX));
    engine.assign_citation_numbers(&sorted_cite_ids);

    let mut bib_citations = Vec::new();
    let mut bib_sort_keys: HashMap<String, Vec<SortKeyValue>> = HashMap::new();
    if let Some(layout) = &style.bibliography {
        let ids: Vec<String> = engine.refs.iter().map(|r| r.id.clone()).collect();
        for id in &ids {
            let keys = engine.sort_keys(layout, id)?;
            bib_sort_keys.insert(id.clone(), keys);
        }
        let sorted_ids = if layout.sort_keys.is_empty() {
            sorted_cite_ids.clone()
        } else {
            let mut ids = ids;
            ids.sort_by(|x, y| compare_keys(&bib_sort_keys, x, y));
            ids
        };
        let reversed = match layout.sort_keys.first() {
            Some(SortKey::Variable(SortDirection::Descending, v)) => {
                v.as_str() == "citation-number"
            }
            Some(SortKey::Macro(SortDirection::Descending, _, _)) => true,
            _ => false,
        };
        if reversed {
            let mut ids = sorted_ids.clone();
            ids.reverse();
            engine.assign_citation_numbers(&ids);
        } else {
            engine.assign_citation_numbers(&sorted_ids);
        }
        bib_citations = sorted_ids
            .iter()
            .map(|id| Citation {
                id: Some(id.clone()),
                items: vec![CitationItem::new(id)],
                ..Citation::default()
            })
            .collect();
    }

    let mut cite_sort_keys: HashMap<String, Vec<SortKeyValue>> = HashMap::new();
    for id in order.keys() {
        let keys = engine.sort_keys(&style.citation, id)?;
        cite_sort_keys.insert((*id).to_owned(), keys);
    }
    let sorted: Vec<Citation> = citations
        .iter()
        .map(|citation| sort_citation_items(citation, &cite_sort_keys))
        .collect();

    let outputs =
        crate::disambiguate::disambiguate_citations(&mut engine, &bib_sort_keys, &sorted)?;
    let layout = &style.citation.options;
    let group_delimiter = style
        .options
        .cite_group_delimiter
        .clone()
        .or_else(|| layout.collapse.map(|_| ", ".to_owned()));
    let is_note = style.options.is_note_style;
    let mut out = Vec::with_capacity(outputs.len());
    for x in outputs {
        let x = match &group_delimiter {
            None => x,
            Some(delimiter) => crate::collapse::group_and_collapse(
                delimiter,
                layout.year_suffix_delimiter.as_deref(),
                layout.after_collapse_delimiter.as_deref(),
                layout.collapse,
                x,
                engine.max_depth,
            )?,
        };
        let mut x = eval::handle_author_only(x, is_note);
        eval::remove_names_if_suppress_author(&mut x, engine.max_depth)?;
        out.push(x);
    }

    let mut bibliography = Vec::with_capacity(bib_citations.len());
    if let Some(layout) = &style.bibliography {
        for (group, citation) in (1..).zip(&bib_citations) {
            let (x, _) = engine.eval_layout(layout, group, citation, Positions::Bibliography)?;
            bibliography.push(x);
        }
        if let Some((text, rule)) = &style.options.subsequent_author_substitute {
            bibliography = crate::disambiguate::subsequent_author_substitutes(
                text,
                *rule,
                bibliography,
                engine.max_depth,
            )?;
        }
    }
    let bibliography = bib_citations
        .iter()
        .map(|c| c.id.clone().unwrap_or_default())
        .zip(bibliography)
        .collect();
    Ok(Evaluated {
        citations: out,
        bibliography,
        warnings: engine.warnings.into_iter().collect(),
    })
}

/// `makeReferenceMap`: the last reference with an identifier wins and
/// takes that place.
fn reference_map(
    references: Vec<Reference<Vec<Inline>>>,
) -> (Vec<Reference<Vec<Inline>>>, HashMap<String, usize>) {
    let mut seen = HashSet::new();
    let mut kept: Vec<Reference<Vec<Inline>>> = Vec::with_capacity(references.len());
    for mut reference in references.into_iter().rev() {
        if seen.insert(reference.id.clone()) {
            for value in reference.variables.values_mut() {
                if let crate::reference::Val::Date(date) = value
                    && date.parts.is_empty()
                    && let Some(literal) = &date.literal
                    && let Some((start, end)) = literal.split_once('_')
                    && let Some(range) = crate::reference::raw_date_edtf(&format!("{start}/{end}"))
                {
                    *date = range;
                }
            }
            kept.push(reference);
        }
    }
    kept.reverse();
    let index = kept
        .iter()
        .enumerate()
        .map(|(n, r)| (r.id.clone(), n))
        .collect();
    (kept, index)
}

fn compare_keys(keys: &HashMap<String, Vec<SortKeyValue>>, x: &str, y: &str) -> Ordering {
    let empty = Vec::new();
    sort::compare_values(keys.get(x).unwrap_or(&empty), keys.get(y).unwrap_or(&empty))
}

/// The items of a citation sorted inside each run of items with no
/// affixes. The runs are Haskell `groupBy` runs: each item is compared with
/// the first item of its run.
fn sort_citation_items(citation: &Citation, keys: &HashMap<String, Vec<SortKeyValue>>) -> Citation {
    let can_group = |a: &CitationItem, b: &CitationItem| {
        a.suffix.is_none() && b.prefix.is_none() && b.suffix.is_none() && a.prefix.is_none()
    };
    let mut items = Vec::with_capacity(citation.items.len());
    let mut run: Vec<CitationItem> = Vec::new();
    for item in &citation.items {
        if let Some(first) = run.first()
            && !can_group(first, item)
        {
            run.sort_by(|a, b| compare_keys(keys, &a.id, &b.id));
            items.append(&mut run);
        }
        run.push(item.clone());
    }
    run.sort_by(|a, b| compare_keys(keys, &a.id, &b.id));
    items.append(&mut run);
    Citation {
        items,
        ..citation.clone()
    }
}

impl Engine<'_> {
    fn reference(&self, id: &str) -> Option<&Reference<Vec<Inline>>> {
        self.index.get(id).and_then(|n| self.refs.get(*n))
    }

    /// `assignCitationNumbers`.
    fn assign_citation_numbers(&mut self, ids: &[String]) {
        for (number, id) in (1..).zip(ids) {
            let Some(&n) = self.index.get(id) else {
                continue;
            };
            let Some(reference) = self.refs.get_mut(n) else {
                continue;
            };
            let label = reference::Variable::new("citation-label");
            if !reference.variables.contains_key(&label) {
                let trigraph = eval::citation_label(reference);
                reference.variables.insert(label, Val::Text(trigraph));
            }
            reference.variables.insert(
                reference::Variable::new("citation-number"),
                Val::Num(number),
            );
            reference.variables.insert(
                reference::Variable::new("citation-key"),
                Val::Text(id.clone()),
            );
        }
    }

    /// `evalSortKeys`: the keys of a layout for one item, with the name
    /// options of the layout alone.
    fn sort_keys(&mut self, layout: &Layout, id: &str) -> Result<Vec<SortKeyValue>, EvalError> {
        let mut out = Vec::with_capacity(layout.sort_keys.len());
        for key in &layout.sort_keys {
            out.push(self.sort_key(layout, key, id)?);
        }
        Ok(out)
    }

    /// `evalSortKey`.
    fn sort_key(
        &mut self,
        layout: &Layout,
        key: &SortKey,
        id: &str,
    ) -> Result<SortKeyValue, EvalError> {
        let collator = self.collator;
        match key {
            SortKey::Macro(direction, name_format, name) => {
                let elements = match self.style.macros.get(name) {
                    Some(elements) => elements.as_slice(),
                    None => {
                        self.warnings.insert(format!("undefined macro {name}"));
                        &[]
                    }
                };
                let Some(reference) = self.index.get(id).and_then(|n| self.refs.get(*n)) else {
                    return Ok(SortKeyValue::new(&collator, *direction, None));
                };
                let context = ItemContext {
                    style: self.style,
                    locale: self.locale,
                    reference,
                    disamb: self.disamb.get(id),
                    abbreviations: self.abbreviations,
                    name_format: name_format.combine(&layout.options.name_format),
                    locator: None,
                    label: None,
                    positions: Vec::new(),
                    in_sort_key: true,
                    in_bibliography: false,
                };
                let (xs, _) =
                    eval::eval_elements(context, elements, &mut self.warnings, self.max_depth)?;
                let inlines = output::render_output(
                    RenderOptions::default(),
                    self.locale,
                    &output::formatted(Formatting::default(), xs),
                    self.max_depth,
                )?;
                let words = sort::normalize_sort_key(&output::stringify(&inlines));
                Ok(SortKeyValue::new(&collator, *direction, Some(words)))
            }
            SortKey::Variable(direction, variable) => {
                let value = self
                    .reference(id)
                    .and_then(|r| eval::lookup_variable(r, variable.as_str()));
                let words = match value {
                    None => None,
                    Some(Val::Text(t)) => Some(sort::normalize_sort_key(&t)),
                    Some(Val::Num(n)) => Some(vec![format!("{n:09}")]),
                    Some(Val::Fancy(x)) => Some(sort::normalize_sort_key(&output::stringify(&x))),
                    Some(Val::Names(names)) => {
                        let demote = self.style.options.demote_non_dropping_particle;
                        let joined: Vec<String> = names
                            .iter()
                            .map(|n| name_sort_order(demote, n).join(" "))
                            .collect();
                        Some(sort::normalize_sort_key(&joined.join(",")))
                    }
                    Some(Val::Date(date)) => Some(vec![sort::date_to_text(&date).to_lowercase()]),
                };
                Ok(SortKeyValue::new(&collator, *direction, words))
            }
        }
    }

    /// `getPosition` outside the bibliography.
    fn position(
        &self,
        group: i64,
        note: Option<i64>,
        item: &CitationItem,
        position_in_group: i64,
    ) -> Vec<Position> {
        let options = &self.style.options;
        let Some(previous) = self.last_cited.get(&item.id) else {
            return vec![Position::First];
        };
        let near_note_distance = options.near_note_distance.unwrap_or(5);
        let note_number = note.unwrap_or(group);
        let previous_note_number = previous.note.unwrap_or(previous.group);
        let alone_in_note = self
            .notes
            .get(&previous_note_number)
            .is_none_or(|ids| ids.len() <= 1);
        let alone = previous.alone_in_group && alone_in_note;
        let mut positions = Vec::with_capacity(4);
        // Haskell `Int` arithmetic wraps, also for extreme note numbers.
        if options.is_note_style
            && note_number.wrapping_sub(previous_note_number) < near_note_distance
        {
            positions.push(Position::NearNote);
        }
        // Haskell compares `Maybe Int` values: `Nothing` is below each `Just`.
        let note_distance_at_most_one = match (note, previous.note) {
            (Some(n), Some(p)) => n.wrapping_sub(p) <= 1,
            _ => true,
        };
        let follows = (group == previous.group
            && position_in_group == previous.position_in_group + 1)
            || (group == previous.group + 1
                && note_distance_at_most_one
                && position_in_group == 1
                && alone);
        if follows {
            match (&previous.locator, &item.locator) {
                (None, Some(_)) => positions.extend([Position::IbidWithLocator, Position::Ibid]),
                (None, None) => positions.push(Position::Ibid),
                (Some(_), None) => {}
                (Some(l1), Some(l2)) if l1 == l2 && item.label == previous.label => {
                    positions.push(Position::Ibid);
                }
                (Some(_), Some(_)) => {
                    positions.extend([Position::IbidWithLocator, Position::Ibid]);
                }
            }
        }
        positions.push(Position::Subsequent);
        positions
    }

    /// `updateRefMap` and `updateLastCitedMap`.
    fn update_history(
        &mut self,
        group: i64,
        citation: &Citation,
        item: &CitationItem,
        position_in_group: i64,
    ) {
        if let Some(note) = citation.note_number {
            self.notes.entry(note).or_default().insert(item.id.clone());
        }
        if self.style.options.is_note_style
            && !self.last_cited.contains_key(&item.id)
            && let Some(&n) = self.index.get(&item.id)
            && let Some(reference) = self.refs.get_mut(n)
        {
            reference.variables.insert(
                reference::Variable::new("first-reference-note-number"),
                Val::Num(citation.note_number.unwrap_or(group)),
            );
        }
        if item.kind == ItemType::AuthorOnly {
            return;
        }
        let alone_in_group = match citation.items.as_slice() {
            [_] => true,
            [x, y] => {
                x.id == y.id && x.kind == ItemType::AuthorOnly && y.kind == ItemType::SuppressAuthor
            }
            _ => false,
        };
        self.last_cited.insert(
            item.id.clone(),
            Cited {
                group,
                note: citation.note_number,
                position_in_group,
                alone_in_group,
                label: item.label.clone(),
                locator: item.locator.clone(),
            },
        );
    }

    /// Clears the cite history (`renderAll`, `citationResetPosition`).
    pub(crate) fn clear_history(&mut self) {
        self.last_cited.clear();
        self.notes.clear();
    }

    /// `evalLayout`: one citation, or one bibliography entry. The second
    /// value holds the positions of the items.
    pub(crate) fn eval_layout(
        &mut self,
        layout: &Layout,
        group: i64,
        citation: &Citation,
        positions: Positions,
    ) -> Result<(Output, Vec<Vec<Position>>), EvalError> {
        let style = self.style;
        let name_format = layout
            .options
            .name_format
            .combine(&style.options.name_format);
        let first_position = match citation.items.first() {
            Some(item) if item.kind == ItemType::AuthorOnly => 0,
            _ => 1,
        };
        if citation.reset_position {
            self.clear_history();
        }
        let in_bibliography = matches!(positions, Positions::Bibliography);
        let is_note = style.options.is_note_style;
        let second_field_align = in_bibliography && style.options.second_field_align.is_some();
        let items = eval::add_citation_affixes(citation);
        let mut outputs = Vec::with_capacity(items.len());
        let mut seen = Vec::with_capacity(items.len());
        for (n, (position_in_group, item)) in (first_position..).zip(&items).enumerate() {
            let item_positions = match &positions {
                Positions::History => {
                    self.position(group, citation.note_number, item, position_in_group)
                }
                Positions::Replay(known) => known.get(n).cloned().unwrap_or_default(),
                Positions::Bibliography => Vec::new(),
            };
            let xs = self.eval_item(
                layout,
                &name_format,
                item_positions.clone(),
                item,
                in_bibliography,
            )?;
            if matches!(positions, Positions::History) {
                self.update_history(group, citation, item, position_in_group);
            }
            seen.push(item_positions);
            outputs.push(eval::wrap_item(item, xs, is_note, second_field_align));
        }
        let formatting = &layout.formatting;
        let output = match &formatting.suffix {
            None => output::formatted(formatting.clone(), outputs),
            Some(suffix) => {
                match eval::move_suffix_inside_display(&outputs, suffix, self.max_depth)? {
                    None => output::formatted(formatting.clone(), outputs),
                    Some(moved) => output::formatted(
                        Formatting {
                            suffix: None,
                            ..formatting.clone()
                        },
                        moved,
                    ),
                }
            }
        };
        Ok((output, seen))
    }

    /// `evalItem`.
    fn eval_item(
        &mut self,
        layout: &Layout,
        name_format: &NameFormat,
        positions: Vec<Position>,
        item: &CitationItem,
        in_bibliography: bool,
    ) -> Result<Vec<Output>, EvalError> {
        let Some(&n) = self.index.get(&item.id) else {
            self.warnings
                .insert(format!("citation {} not found", item.id));
            return Ok(vec![Output::Literal(output::add_font_weight(
                crate::types::FontWeight::Bold,
                output::from_text(&format!("{}?", item.id)),
            ))]);
        };
        let Some(reference) = self.refs.get(n) else {
            return Ok(Vec::new());
        };
        let context = ItemContext {
            style: self.style,
            locale: self.locale,
            reference,
            disamb: self.disamb.get(&item.id),
            abbreviations: self.abbreviations,
            name_format: name_format.clone(),
            locator: item.locator.as_deref(),
            label: item.label.as_deref(),
            positions,
            in_sort_key: false,
            in_bibliography,
        };
        let (mut xs, used) = eval::eval_elements(
            context,
            &layout.elements,
            &mut self.warnings,
            self.max_depth,
        )?;
        if !used.identifier && in_bibliography {
            let url = ["DOI", "PMCID", "PMID", "URL"]
                .into_iter()
                .find_map(|name| {
                    let value = eval::lookup_variable(reference, name)?;
                    let text = eval::val_to_text(&value)?;
                    Some(eval::identifier_url(&name.to_ascii_lowercase(), &text))
                });
            if let Some(url) = url {
                if used.title {
                    for x in &mut xs {
                        link_titles(x, &url, self.max_depth)?;
                    }
                } else {
                    xs = vec![Output::Linked(url, xs)];
                }
            }
        }
        if let Some(lang) = eval::language_of(reference) {
            eval::add_language(&mut xs, &lang);
        }
        Ok(xs)
    }
}

/// `transform (linkTitle url)`: each title goes into a link.
fn link_titles(x: &mut Output, url: &str, depth: usize) -> Result<(), EvalError> {
    let depth = depth.checked_sub(1).ok_or(EvalError::TooDeep)?;
    for child in eval::children_mut(x) {
        link_titles(child, url, depth)?;
    }
    if matches!(x, Output::Tagged(Tag::Title, _)) {
        let title = std::mem::replace(x, Output::Null);
        *x = Output::Linked(url.to_owned(), vec![title]);
    }
    Ok(())
}

/// `getNamePartSortOrder`: the parts of a name for a sort key. Upstream
/// joins two optional parts with `<>`: the text of both, without a space.
fn name_sort_order(
    demote: crate::types::DemoteNonDroppingParticle,
    name: &reference::Name,
) -> Vec<String> {
    let text = |x: &Option<String>| x.clone().unwrap_or_default();
    let join = |x: &Option<String>, y: &Option<String>| match (x, y) {
        (None, None) => String::new(),
        _ => format!("{}{}", text(x), text(y)),
    };
    if let Some(literal) = &name.literal {
        return vec![literal.clone()];
    }
    if is_byzantine_name(name) {
        match demote {
            crate::types::DemoteNonDroppingParticle::Never => vec![
                join(&name.non_dropping_particle, &name.family),
                text(&name.dropping_particle),
                text(&name.given),
                text(&name.suffix),
            ],
            _ => vec![
                text(&name.family),
                join(&name.dropping_particle, &name.non_dropping_particle),
                text(&name.given),
                text(&name.suffix),
            ],
        }
    } else {
        vec![text(&name.family), text(&name.given)]
    }
}

/// `isByzantineName`: the family name is in a byzantine script.
pub(crate) fn is_byzantine_name(name: &reference::Name) -> bool {
    name.family.as_deref().is_some_and(output::is_byzantine)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::Variable;
    use randoc_core::Context;

    const NS: &str = "xmlns=\"http://purl.org/net/xbiblio/csl\"";

    fn reference(id: &str, title: &str) -> Reference<Vec<Inline>> {
        let mut r = Reference {
            id: id.to_owned(),
            kind: "book".to_owned(),
            ..Reference::default()
        };
        r.variables
            .insert(Variable::new("title"), Val::Text(title.to_owned()));
        r
    }

    fn texts(
        style: &str,
        refs: Vec<Reference<Vec<Inline>>>,
        citations: &[Citation],
    ) -> Vec<String> {
        let style = crate::loader::parse_style(&mut Context::pure(), style).unwrap();
        let locale = Locale::default();
        let input = Input {
            style: &style,
            locale: &locale,
            lang: None,
            abbreviations: None,
            references: refs,
            citations,
            max_depth: 100,
        };
        let result = citeproc(RenderOptions::default(), input).unwrap();
        let mut out: Vec<String> = result
            .citations
            .iter()
            .map(|c| output::stringify(c))
            .collect();
        out.extend(
            result
                .bibliography
                .iter()
                .map(|(id, c)| format!("{id}={}", output::stringify(c))),
        );
        out
    }

    fn citation(ids: &[&str]) -> Citation {
        Citation {
            items: ids.iter().map(|id| CitationItem::new(id)).collect(),
            ..Citation::default()
        }
    }

    #[test]
    fn literal_date_ranges_render_in_citations_and_the_bibliography() {
        let style = format!(
            r#"<style {NS} version="1.0"><citation><layout delimiter="; "><date variable="issued"><date-part name="year"/></date></layout></citation><bibliography><layout><date variable="issued"><date-part name="year"/></date></layout></bibliography></style>"#
        );
        let dates = [
            ("range", "1996_2004", vec![]),
            ("open-end", "1996_", vec![]),
            ("open-start", "_2004", vec![]),
            ("text", "ca_2004", vec![]),
            ("slash", "1996/2004", vec![]),
            (
                "parts",
                "1996_2004",
                vec![crate::reference::DateParts(vec![2010])],
            ),
        ];
        let ids: Vec<_> = dates.iter().map(|(id, _, _)| *id).collect();
        let refs = dates
            .into_iter()
            .map(|(id, literal, parts)| {
                let mut r = reference(id, "");
                r.variables.insert(
                    Variable::new("issued"),
                    Val::Date(crate::reference::Date {
                        literal: Some(literal.to_owned()),
                        parts,
                        ..Default::default()
                    }),
                );
                r
            })
            .collect();
        assert_eq!(
            texts(&style, refs, &[citation(&ids)]),
            [
                "1996–2004; 1996–; 2004; ca_2004; 1996/2004; 1996_2004",
                "range=1996–2004",
                "open-end=1996–",
                "open-start=2004",
                "text=ca_2004",
                "slash=1996/2004",
                "parts=1996_2004"
            ]
        );
    }

    #[test]
    fn the_last_reference_with_an_identifier_wins_in_its_place() {
        let style = format!(
            "<style {NS}><citation><layout delimiter=\";\"><text variable=\"title\"/></layout></citation>\
             <bibliography><layout><text variable=\"title\"/></layout></bibliography></style>"
        );
        let refs = vec![
            reference("a", "A1"),
            reference("b", "B"),
            reference("a", "A2"),
        ];
        assert_eq!(texts(&style, refs, &[]), ["b=B", "a=A2"]);
    }

    #[test]
    fn items_sort_inside_runs_without_affixes() {
        let style = format!(
            "<style {NS}><citation><sort><key variable=\"title\"/></sort><layout delimiter=\";\">\
             <text variable=\"title\"/></layout></citation></style>"
        );
        let refs = vec![
            reference("a", "A"),
            reference("b", "B"),
            reference("c", "C"),
            reference("d", "D"),
        ];
        let mut cite = citation(&["d", "c", "b", "a"]);
        cite.items[2].prefix = Some(vec![Inline::Str("p".into())]);
        assert_eq!(texts(&style, refs, &[cite]), ["C;D;pB;A"]);
    }

    #[test]
    fn a_descending_macro_key_reverses_the_citation_numbers() {
        let style = format!(
            "<style {NS}><macro name=\"t\"><text variable=\"title\"/></macro>\
             <citation><layout delimiter=\",\"><text variable=\"citation-number\"/></layout></citation>\
             <bibliography><sort><key macro=\"t\" sort=\"descending\"/></sort><layout>\
             <text variable=\"citation-number\"/></layout></bibliography></style>"
        );
        let refs = vec![
            reference("a", "A"),
            reference("b", "B"),
            reference("c", "C"),
        ];
        // The bibliography order (C, B, A) is not reversed; the numbers are.
        assert_eq!(
            texts(&style, refs, &[citation(&["a", "b", "c"])]),
            ["1,2,3", "c=3", "b=2", "a=1"]
        );
    }

    #[test]
    fn positions_follow_the_citations_of_the_document() {
        let style = format!(
            "<style {NS} class=\"note\"><citation><layout><choose><if position=\"ibid\"><text value=\"I\"/></if>\
             <else-if position=\"subsequent\"><text value=\"S\"/></else-if><else><text value=\"F\"/></else></choose>\
             </layout></citation></style>"
        );
        let refs = vec![reference("a", "A"), reference("b", "B")];
        let cites: Vec<Citation> = [("a", 1), ("a", 2), ("b", 3), ("a", 4)]
            .into_iter()
            .map(|(id, note)| Citation {
                note_number: Some(note),
                ..citation(&[id])
            })
            .collect();
        assert_eq!(texts(&style, refs, &cites), ["F", "I", "F", "S"]);
    }
}
