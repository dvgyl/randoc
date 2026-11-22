//! Disambiguation and the subsequent author substitute: a port of
//! `disambiguateCitations`, its helpers and `subsequentAuthorSubstitutes`
//! of `Citeproc.Eval` (`citeproc` 0.14.1.1).
//!
//! The searches that upstream makes over all pairs of items use maps here,
//! with the same result.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use crate::citeproc::{Citation, CitationItem, Engine, Positions};
use crate::eval::{DisambiguationData, EvalError, NameHint, initialize_name};
use crate::output::{self, Output, Tag};
use crate::reference::{Date, Name};
use crate::sort::{self, SortKeyValue};
use crate::types::{
    GivenNameDisambiguationRule, ItemType, Position, SubsequentAuthorSubstituteRule,
};

/// `DisambData`: the parts of a rendered item that make it ambiguous.
/// The derived order is the order of upstream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct DisambData {
    item: String,
    names: Vec<Name>,
    dates: Vec<Date>,
    rendered: String,
}

/// `RenderedCitation`: a citation, the positions of its items and the
/// disambiguation data of its items.
struct Rendered {
    positions: Vec<Vec<Position>>,
    output: Output,
    data: Vec<DisambData>,
}

type States = HashMap<String, DisambiguationData>;

/// `disambiguateCitations`: the outputs of the citations after
/// disambiguation.
pub(crate) fn disambiguate_citations(
    engine: &mut Engine,
    bib_sort_keys: &HashMap<String, Vec<SortKeyValue>>,
    citations: &[Citation],
) -> Result<Vec<Output>, EvalError> {
    let mut ids: Vec<String> = engine.refs.iter().map(|r| r.id.clone()).collect();
    ids.sort();
    let clean = |citation: &Citation| {
        let remove_affixes = |item: &CitationItem| CitationItem {
            label: None,
            locator: None,
            prefix: None,
            suffix: None,
            ..item.clone()
        };
        let items = match citation.items.as_slice() {
            [i1, i2, rest @ ..]
                if i1.kind == ItemType::AuthorOnly && i2.kind == ItemType::SuppressAuthor =>
            {
                let mut items = vec![CitationItem {
                    kind: ItemType::NormalCite,
                    ..remove_affixes(i2)
                }];
                items.extend(rest.iter().map(remove_affixes));
                items
            }
            items => items.iter().map(remove_affixes).collect(),
        };
        Citation {
            items,
            ..citation.clone()
        }
    };
    let mut all: Vec<Citation> = citations.iter().map(clean).collect();
    all.extend(ids.iter().map(|id| Citation {
        items: vec![CitationItem::new(id)],
        ..Citation::default()
    }));
    let mut rendered = render_all(engine, &all)?;

    let lang = engine.locale.language.clone();
    let strategy = engine.style.options.disambiguation;
    if let Some(rule) = strategy.add_given_names
        && rule != GivenNameDisambiguationRule::ByCite
    {
        let names_map = Arc::new(global_names_map(&rendered, rule, lang.as_ref()));
        for id in &ids {
            engine.disamb.entry(id.clone()).or_default().name_map = Arc::clone(&names_map);
        }
        rendered = render_all(engine, &all)?;
    }

    let states = engine.disamb.clone();
    let ambiguities = group_ambiguities(rendered.iter().flat_map(|r| r.data.iter().cloned()));
    if !ambiguities.is_empty() {
        let mut disamb = Disambiguation {
            engine,
            lang: lang.as_ref(),
            bib_sort_keys,
            citations: &all,
            rendered,
            states,
        };
        disamb.analyze(ambiguities)?;
    }
    Ok(render_all(engine, citations)?
        .into_iter()
        .map(|r| r.output)
        .collect())
}

/// `renderAll`: each citation with an empty cite history at the start.
fn render_all(engine: &mut Engine, citations: &[Citation]) -> Result<Vec<Rendered>, EvalError> {
    engine.clear_history();
    let style = engine.style;
    let mut out = Vec::with_capacity(citations.len());
    for (group, citation) in (1..).zip(citations) {
        let (output, positions) =
            engine.eval_layout(&style.citation, group, citation, Positions::History)?;
        out.push(rendered(output, positions));
    }
    Ok(out)
}

fn rendered(output: Output, positions: Vec<Vec<Position>>) -> Rendered {
    let data = extract_tag_items(&output)
        .into_iter()
        .map(|(item, x)| to_disamb_data(item, x))
        .collect();
    Rendered {
        positions,
        output,
        data,
    }
}

/// `universe`: the output and each part inside it, in pre-order.
pub(crate) fn universe(x: &Output) -> Vec<&Output> {
    let mut out = Vec::new();
    let mut pending = vec![x];
    while let Some(x) = pending.pop() {
        out.push(x);
        match x {
            Output::Formatted(_, parts) | Output::Linked(_, parts) => {
                pending.extend(parts.iter().rev());
            }
            Output::Tagged(_, inner) | Output::InNote(inner) => pending.push(inner),
            Output::Literal(_) | Output::Null => {}
        }
    }
    out
}

/// `extractTagItems`: the normal items, without the items that hold the
/// term `ibid`.
fn extract_tag_items(x: &Output) -> Vec<(&str, &Output)> {
    universe(x)
        .into_iter()
        .filter_map(|y| match y {
            Output::Tagged(
                Tag::Item {
                    kind: ItemType::NormalCite,
                    id,
                },
                inner,
            ) if !has_ibid(inner) => Some((id.as_str(), inner.as_ref())),
            _ => None,
        })
        .collect()
}

fn has_ibid(x: &Output) -> bool {
    universe(x)
        .iter()
        .any(|y| matches!(y, Output::Tagged(Tag::Term(term), _) if term.name == "ibid"))
}

/// `toDisambData`.
fn to_disamb_data(item: &str, x: &Output) -> DisambData {
    let mut names = Vec::new();
    let mut dates = Vec::new();
    for y in universe(x) {
        match y {
            Output::Tagged(Tag::Names { names: ns, .. }, _) => names.extend(ns.iter().cloned()),
            Output::Tagged(Tag::Date(date), _) => dates.push((**date).clone()),
            _ => {}
        }
    }
    DisambData {
        item: item.to_owned(),
        names,
        dates,
        rendered: output::output_to_text(x),
    }
}

/// `groupAmbiguities`: the groups of at least two items with the same
/// rendered text, each item once.
fn group_ambiguities(data: impl Iterator<Item = DisambData>) -> Vec<Vec<DisambData>> {
    let mut data: Vec<DisambData> = data.collect();
    data.sort_by(|a, b| a.rendered.cmp(&b.rendered));
    let mut out = Vec::new();
    let mut start = 0;
    while start < data.len() {
        let end = data[start..]
            .iter()
            .position(|d| d.rendered != data[start].rendered)
            .map_or(data.len(), |n| start + n);
        let run = &data[start..end];
        start = end;
        if run.len() < 2 || run[0].rendered.is_empty() {
            continue;
        }
        let mut seen = HashSet::new();
        let unique: Vec<DisambData> = run
            .iter()
            .filter(|d| seen.insert(d.item.clone()))
            .cloned()
            .collect();
        if unique.len() >= 2 {
            out.push(unique);
        }
    }
    out
}

/// The names map of a global given-name rule: the hints of the names
/// whose short forms are equal.
fn global_names_map(
    rendered: &[Rendered],
    rule: GivenNameDisambiguationRule,
    lang: Option<&crate::lang::Lang>,
) -> BTreeMap<Name, NameHint> {
    let mut groups: Vec<&Vec<Name>> = Vec::new();
    for r in rendered {
        for y in universe(&r.output) {
            if let Output::Tagged(Tag::Names { names, .. }, _) = y {
                groups.push(names);
            }
        }
    }
    let mut seen = HashSet::new();
    let relevant: Vec<&Name> = match rule {
        GivenNameDisambiguationRule::PrimaryName
        | GivenNameDisambiguationRule::PrimaryNameWithInitials => groups
            .iter()
            .filter_map(|names| names.first())
            .filter(|n| seen.insert(*n))
            .collect(),
        _ => groups
            .iter()
            .flat_map(|names| names.iter())
            .filter(|n| seen.insert(*n))
            .collect(),
    };
    let mut by_short: Vec<(&Option<String>, &Option<String>)> = Vec::new();
    let mut members: HashMap<(&Option<String>, &Option<String>), Vec<&Name>> = HashMap::new();
    for name in &relevant {
        let short = (&name.non_dropping_particle, &name.family);
        if name.family.is_some() && !members.contains_key(&short) {
            by_short.push(short);
        }
        if name.family.is_some() {
            members.entry(short).or_default().push(name);
        }
    }
    let mut map = BTreeMap::new();
    for short in by_short {
        let Some(names) = members.get(&short) else {
            continue;
        };
        if names.len() <= 1 {
            continue;
        }
        // The names of a group are distinct, so another name with the same
        // initials exists when two names of the group have them.
        let initials: Vec<Option<String>> = names.iter().map(|n| initials(lang, n)).collect();
        let mut by_initials: HashMap<&str, usize> = HashMap::new();
        for text in initials.iter().flatten() {
            *by_initials.entry(text).or_default() += 1;
        }
        for (name, initials) in names.iter().zip(&initials) {
            let initials_match = initials
                .as_deref()
                .is_some_and(|text| by_initials.get(text).copied().unwrap_or(0) > 1);
            let hint = if initials_match {
                match rule {
                    GivenNameDisambiguationRule::AllNamesWithInitials
                    | GivenNameDisambiguationRule::PrimaryNameWithInitials => None,
                    GivenNameDisambiguationRule::PrimaryName => {
                        Some(NameHint::AddGivenNameIfPrimary)
                    }
                    _ => Some(NameHint::AddGivenName),
                }
            } else {
                match rule {
                    GivenNameDisambiguationRule::PrimaryNameWithInitials
                    | GivenNameDisambiguationRule::PrimaryName => {
                        Some(NameHint::AddInitialsIfPrimary)
                    }
                    _ => Some(NameHint::AddInitials),
                }
            };
            if let Some(hint) = hint {
                map.insert((*name).clone(), hint);
            }
        }
    }
    map
}

fn initials(lang: Option<&crate::lang::Lang>, name: &Name) -> Option<String> {
    name.given
        .as_deref()
        .map(|given| initialize_name(lang, true, false, "", given))
}

/// The state of `analyzeAmbiguities`.
struct Disambiguation<'e, 'a, 'k> {
    engine: &'e mut Engine<'a>,
    lang: Option<&'k crate::lang::Lang>,
    bib_sort_keys: &'k HashMap<String, Vec<SortKeyValue>>,
    citations: &'k [Citation],
    rendered: Vec<Rendered>,
    states: States,
}

impl Disambiguation<'_, '_, '_> {
    /// `analyzeAmbiguities`.
    fn analyze(&mut self, ambiguities: Vec<Vec<DisambData>>) -> Result<(), EvalError> {
        let strategy = self.engine.style.options.disambiguation;
        let mut ambiguities = ambiguities;
        if !ambiguities.is_empty() && strategy.add_names {
            for group in &ambiguities {
                self.try_add_names(strategy.add_given_names, group);
            }
            ambiguities = self.refresh()?;
        }
        if strategy.add_given_names == Some(GivenNameDisambiguationRule::ByCite)
            && !ambiguities.is_empty()
        {
            for group in &ambiguities {
                self.try_add_given_names(group);
            }
            ambiguities = self.refresh()?;
        }
        if !ambiguities.is_empty() && strategy.add_year_suffix {
            self.add_year_suffixes(&ambiguities);
            ambiguities = self.refresh()?;
        }
        for group in &ambiguities {
            for d in group {
                self.engine
                    .disamb
                    .entry(d.item.clone())
                    .or_default()
                    .condition = true;
            }
        }
        Ok(())
    }

    /// `refreshAmbiguities` with `refreshCitations`: the citations with a
    /// changed item render again with their earlier positions.
    fn refresh(&mut self) -> Result<Vec<Vec<DisambData>>, EvalError> {
        let new = &self.engine.disamb;
        let changed: HashSet<String> = new
            .keys()
            .chain(self.states.keys())
            .filter(|id| new.get(*id) != self.states.get(*id))
            .cloned()
            .collect();
        let style = self.engine.style;
        for (group, (citation, cached)) in (1..).zip(self.citations.iter().zip(&mut self.rendered))
        {
            if citation.items.iter().any(|i| changed.contains(&i.id)) {
                let (output, _) = self.engine.eval_layout(
                    &style.citation,
                    group,
                    citation,
                    Positions::Replay(&cached.positions),
                )?;
                let positions = std::mem::take(&mut cached.positions);
                *cached = rendered(output, positions);
            }
        }
        self.states = self.engine.disamb.clone();
        Ok(group_ambiguities(
            self.rendered.iter().flat_map(|r| r.data.iter().cloned()),
        ))
    }

    fn set(&mut self, id: &str, f: impl FnOnce(&mut DisambiguationData)) {
        if self.engine.index.contains_key(id) {
            f(self.engine.disamb.entry(id.to_owned()).or_default());
        }
    }

    /// `tryAddNames`. The key of an item stands for its first `n` names in
    /// the form of the rule: each step extends the key of the step before
    /// by one name, so a step costs one name for each item.
    fn try_add_names(&mut self, rule: Option<GivenNameDisambiguationRule>, group: &[DisambData]) {
        if rule == Some(GivenNameDisambiguationRule::ByCite) {
            self.try_add_given_names(group);
        }
        // Key 0 is the empty list of names.
        let mut remaining: Vec<(&DisambData, usize)> = group.iter().map(|d| (d, 0)).collect();
        let mut keys: HashMap<(usize, Name), usize> = HashMap::new();
        let mut n = 1usize;
        loop {
            let max_names = remaining
                .iter()
                .map(|(d, _)| d.names.len())
                .max()
                .unwrap_or(0);
            if n > max_names {
                return;
            }
            for (d, key) in &mut remaining {
                if let Some(name) = d.names.get(n - 1) {
                    let name = name_in_form(self.lang, rule, n - 1, name);
                    let next = keys.len() + 1;
                    *key = *keys.entry((*key, name)).or_insert(next);
                }
            }
            let done = disambiguated(&remaining);
            if done.iter().any(|d| *d) {
                let count = i64::try_from(n).unwrap_or(i64::MAX);
                for (d, _) in &remaining {
                    let id = d.item.clone();
                    self.set(&id, |data| data.et_al_names = Some(count));
                }
                remaining = remaining
                    .into_iter()
                    .zip(done)
                    .filter(|(_, done)| !done)
                    .map(|(d, _)| d)
                    .collect();
            }
            n += 1;
        }
    }

    /// `tryAddGivenNames`: for each name position, the names that another
    /// name of the position shares the family name with get a hint.
    fn try_add_given_names(&mut self, group: &[DisambData]) {
        let mut remaining: Vec<&str> = group.iter().map(|d| d.item.as_str()).collect();
        let longest = group.iter().map(|d| d.names.len()).max().unwrap_or(0);
        for position in 0..longest {
            if remaining.is_empty() {
                return;
            }
            // `transpose` drops the missing names; `zip` pairs the column
            // with the items by place.
            let column: Vec<&Name> = group.iter().filter_map(|d| d.names.get(position)).collect();
            let pairs: Vec<(&str, &Name)> = group
                .iter()
                .map(|d| d.item.as_str())
                .zip(column.iter().copied())
                .collect();
            let hinted = self.add_name_hints(&column, &pairs);
            remaining.retain(|id| !hinted.contains(*id));
        }
    }

    /// `addNameHint` for each pair of a column.
    fn add_name_hints(&mut self, column: &[&Name], pairs: &[(&str, &Name)]) -> HashSet<String> {
        type Family<'n> = (&'n Option<String>, &'n Option<String>);
        let mut count: HashMap<Family, usize> = HashMap::new();
        let mut equal: HashMap<&Name, usize> = HashMap::new();
        let mut by_initials: HashMap<(Family, String), usize> = HashMap::new();
        for name in column {
            let family = (&name.family, &name.non_dropping_particle);
            *count.entry(family).or_default() += 1;
            *equal.entry(name).or_default() += 1;
            if let Some(initials) = initials(self.lang, name) {
                *by_initials.entry((family, initials)).or_default() += 1;
            }
        }
        let mut hinted = HashSet::new();
        for (item, name) in pairs {
            let family = (&name.family, &name.non_dropping_particle);
            let same = equal.get(name).copied().unwrap_or(0);
            if count.get(&family).copied().unwrap_or(0) <= same {
                continue;
            }
            let initials_match = initials(self.lang, name).is_some_and(|initials| {
                by_initials.get(&(family, initials)).copied().unwrap_or(0) > same
            });
            let hint = if initials_match {
                NameHint::AddGivenName
            } else {
                NameHint::AddInitials
            };
            let name = (*name).clone();
            self.set(item, |data| {
                Arc::make_mut(&mut data.name_map).insert(name, hint);
            });
            hinted.insert((*item).to_owned());
        }
        hinted
    }

    /// `addYearSuffixes`: the items of each set of companion groups in the
    /// order of the bibliography, the groups in the order of upstream's set.
    fn add_year_suffixes(&mut self, groups: &[Vec<DisambData>]) {
        let mut membership: HashMap<&DisambData, Vec<usize>> = HashMap::new();
        for (n, group) in groups.iter().enumerate() {
            for d in group {
                let entry = membership.entry(d).or_default();
                if entry.last() != Some(&n) {
                    entry.push(n);
                }
            }
        }
        let keys = self.bib_sort_keys;
        let empty = Vec::new();
        let mut companions: BTreeSet<Vec<DisambData>> = BTreeSet::new();
        let mut done: HashSet<&Vec<usize>> = HashSet::new();
        for indices in membership.values() {
            if !done.insert(indices) {
                continue;
            }
            let mut list: Vec<DisambData> = indices
                .iter()
                .filter_map(|n| groups.get(*n))
                .flatten()
                .cloned()
                .collect();
            list.sort_by(|a, b| {
                sort::compare_values(
                    keys.get(&a.item).unwrap_or(&empty),
                    keys.get(&b.item).unwrap_or(&empty),
                )
            });
            companions.insert(list);
        }
        for list in companions {
            for (suffix, d) in (1..).zip(&list) {
                self.set(&d.item, |data| data.year_suffix = Some(suffix));
            }
        }
    }
}

/// A name in the form that a given-name rule compares: the name at
/// `position` (0 for the first name) with all, some or none of its given
/// name.
fn name_in_form(
    lang: Option<&crate::lang::Lang>,
    rule: Option<GivenNameDisambiguationRule>,
    position: usize,
    name: &Name,
) -> Name {
    let without_given = || Name {
        given: None,
        ..name.clone()
    };
    let with_initials = || Name {
        given: name
            .given
            .as_deref()
            .map(|g| initialize_name(lang, true, false, "", g)),
        ..name.clone()
    };
    match rule {
        Some(GivenNameDisambiguationRule::AllNames | GivenNameDisambiguationRule::ByCite) => {
            name.clone()
        }
        Some(GivenNameDisambiguationRule::AllNamesWithInitials) => with_initials(),
        Some(GivenNameDisambiguationRule::PrimaryName) if position == 0 => name.clone(),
        Some(GivenNameDisambiguationRule::PrimaryNameWithInitials) if position == 0 => {
            with_initials()
        }
        Some(
            GivenNameDisambiguationRule::PrimaryName
            | GivenNameDisambiguationRule::PrimaryNameWithInitials,
        )
        | None => without_given(),
    }
}

/// `isDisambiguated` for each item: an item is disambiguated when each
/// other item with the same key (the same first names in the form of the
/// rule) is equal to it.
fn disambiguated(items: &[(&DisambData, usize)]) -> Vec<bool> {
    let mut first: HashMap<usize, usize> = HashMap::new();
    let mut uniform: HashMap<usize, bool> = HashMap::new();
    for (n, (d, key)) in items.iter().enumerate() {
        match first.get(key) {
            None => {
                first.insert(*key, n);
                uniform.insert(*key, true);
            }
            Some(&m) => {
                if items[m].0 != *d {
                    uniform.insert(*key, false);
                }
            }
        }
    }
    items
        .iter()
        .map(|(_, key)| uniform.get(key).copied().unwrap_or(true))
        .collect()
}

/// `subsequentAuthorSubstitutes`: an entry whose names match the names of
/// the entry before it shows the substitute text in their place.
pub(crate) fn subsequent_author_substitutes(
    text: &str,
    rule: SubsequentAuthorSubstituteRule,
    entries: Vec<Output>,
    depth: usize,
) -> Result<Vec<Output>, EvalError> {
    let replacement = output::from_text(text);
    let mut out = Vec::with_capacity(entries.len());
    let mut entries = entries.into_iter().peekable();
    while let Some(x) = entries.next() {
        let names = get_names(&x).unwrap_or_else(|| (Vec::new(), Output::Null));
        out.push(x);
        while let Some(next) = entries.peek() {
            let mut candidate = next.clone();
            if !replace_match(rule, &replacement, &names, &mut candidate, depth)? {
                break;
            }
            entries.next();
            out.push(candidate);
        }
    }
    Ok(out)
}

/// The names and the rendered output of the first names of an entry.
type FirstNames = (Vec<Name>, Output);

/// `getNames`: the first names inside the first part of an entry.
fn get_names(x: &Output) -> Option<FirstNames> {
    let Output::Formatted(_, parts) = x else {
        return None;
    };
    let first = parts.first()?;
    universe(first).into_iter().find_map(|y| match y {
        Output::Tagged(Tag::Names { names, .. }, inner) => Some((names.clone(), (**inner).clone())),
        _ => None,
    })
}

/// `replaceMatch` for one entry: true when the entry matches and changed.
fn replace_match(
    rule: SubsequentAuthorSubstituteRule,
    replacement: &[randoc_ast::Inline],
    first: &FirstNames,
    z: &mut Output,
    depth: usize,
) -> Result<bool, EvalError> {
    let depth = depth.checked_sub(1).ok_or(EvalError::TooDeep)?;
    match z {
        Output::Tagged(Tag::Item { .. }, inner) => {
            replace_match(rule, replacement, first, inner, depth)
        }
        Output::Formatted(_, parts) if !parts.is_empty() => {
            replace_match(rule, replacement, first, &mut parts[0], depth)
        }
        Output::Tagged(Tag::Names { .. }, _) => replace_names(rule, replacement, first, z, depth),
        _ => Ok(false),
    }
}

/// `replaceMatch` at the names of an entry.
fn replace_names(
    rule: SubsequentAuthorSubstituteRule,
    replacement: &[randoc_ast::Inline],
    (names, raw): &FirstNames,
    z: &mut Output,
    depth: usize,
) -> Result<bool, EvalError> {
    let Output::Tagged(Tag::Names { names: ns, .. }, r) = z else {
        return Ok(false);
    };
    let ns = ns.clone();
    let rule = if names.is_empty() {
        SubsequentAuthorSubstituteRule::CompleteAll
    } else {
        rule
    };
    let literal = || Output::Literal(replacement.to_vec());
    match rule {
        SubsequentAuthorSubstituteRule::CompleteAll => {
            let rendered_names = |x: &Output| -> Vec<String> {
                universe(x)
                    .into_iter()
                    .filter_map(|y| match y {
                        Output::Tagged(Tag::Name(_), o) => Some(output::output_to_text(o)),
                        _ => None,
                    })
                    .collect()
            };
            let (a, b) = (rendered_names(raw), rendered_names(r));
            let same = if a.is_empty() && b.is_empty() {
                output::output_to_text(r) == output::output_to_text(raw)
            } else {
                a == b
            };
            if !same {
                return Ok(false);
            }
            if ns.is_empty() {
                **r = literal();
            } else {
                let mut x = std::mem::replace(&mut **r, Output::Null);
                transform(&mut x, depth, &mut |y| {
                    if matches!(y, Output::Tagged(Tag::Name(_) | Tag::Term(_), _)) {
                        *y = Output::Null;
                    }
                })?;
                **r = match &mut x {
                    Output::Formatted(f, xs) => {
                        let mut xs = std::mem::take(xs);
                        xs.insert(0, literal());
                        Output::Formatted(std::mem::take(f), xs)
                    }
                    _ => literal(),
                };
            }
            Ok(true)
        }
        SubsequentAuthorSubstituteRule::CompleteEach => {
            if ns != *names {
                return Ok(false);
            }
            transform(z, depth, &mut |y| {
                if let Output::Tagged(Tag::Name(n), inner) = y
                    && names.contains(&**n)
                {
                    **inner = literal();
                }
            })?;
            Ok(true)
        }
        SubsequentAuthorSubstituteRule::PartialEach
        | SubsequentAuthorSubstituteRule::PartialFirst => {
            let matches = ns.iter().zip(names).take_while(|(a, b)| a == b).count();
            if matches < 1 {
                return Ok(false);
            }
            let count = if rule == SubsequentAuthorSubstituteRule::PartialFirst {
                1
            } else {
                matches
            };
            // `foldr (transform . replaceName)`: the last name first.
            for target in ns.iter().take(count).rev() {
                transform(z, depth, &mut |y| {
                    if let Output::Tagged(Tag::Name(n), inner) = y
                        && **n == *target
                    {
                        **inner = literal();
                    }
                })?;
            }
            Ok(true)
        }
    }
}

/// `transform`: `f` on each part, the inner parts first.
pub(crate) fn transform(
    x: &mut Output,
    depth: usize,
    f: &mut dyn FnMut(&mut Output),
) -> Result<(), EvalError> {
    let depth = depth.checked_sub(1).ok_or(EvalError::TooDeep)?;
    for child in crate::eval::children_mut(x) {
        transform(child, depth, f)?;
    }
    f(x);
    Ok(())
}
