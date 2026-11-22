//! Grouping and collapse of the items of a citation: a port of
//! `groupAndCollapseCitations`, `takeSeq` and `groupSuccessive` of
//! `Citeproc.Eval` (`citeproc` 0.14.1.1).
//!
//! Upstream uses the raw `Formatted` constructor in some places and the
//! filtering `formatted` in others; this port keeps each choice.

use crate::disambiguate::{transform, universe};
use crate::eval::EvalError;
use crate::output::{self, Output, Tag};
use crate::reference::Date;
use crate::types::{Collapsing, Formatting, ItemType};

/// A rendered item and the properties that grouping reads (`GroupingItem`).
/// `keys` are hashes of the parts that `sameNames` compares: two items
/// with no equal key are never the same, so most pairs need no deep
/// comparison.
struct Item {
    output: Output,
    has_prefix: bool,
    has_suffix: bool,
    tagged: Option<Output>,
    lead: String,
    keys: [Option<u64>; 2],
}

fn hash_of(value: &impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// The keys of an item: for names, the names (when there are some) and the
/// rendered names; for a date, the text before it.
fn keys_of(tagged: Option<&Output>, lead: &str) -> [Option<u64>; 2] {
    match tagged {
        Some(Output::Tagged(
            Tag::Names {
                variable, names, ..
            },
            ws,
        )) => [
            (!names.is_empty()).then(|| hash_of(&(variable, names))),
            Some(hash_of(&(variable, format!("{ws:?}")))),
        ],
        Some(Output::Tagged(Tag::Date(_), _)) => [None, Some(hash_of(&("date", lead)))],
        _ => [None, None],
    }
}

fn raw(formatting: Formatting, parts: Vec<Output>) -> Output {
    Output::Formatted(Box::new(formatting), parts)
}

fn delimited(delimiter: Option<String>, parts: Vec<Output>) -> Output {
    raw(
        Formatting {
            delimiter,
            ..Formatting::default()
        },
        parts,
    )
}

fn literal(text: Option<&str>) -> Output {
    match text {
        Some(text) => Output::Literal(output::from_text(text)),
        None => Output::Null,
    }
}

/// `groupAndCollapseCitations` for one citation.
pub(crate) fn group_and_collapse(
    group_delimiter: &str,
    year_suffix_delimiter: Option<&str>,
    after_collapse_delimiter: Option<&str>,
    collapsing: Option<Collapsing>,
    citation: Output,
    depth: usize,
) -> Result<Output, EvalError> {
    let mut citation = citation;
    let Output::Formatted(f, xs) = &mut citation else {
        return Ok(citation);
    };
    let mut f = std::mem::take(f);
    let xs = std::mem::take(xs);
    let delimiter = f.delimiter.clone();
    let collapse = Collapse {
        group_delimiter,
        year_suffix_delimiter,
        after_collapse_delimiter,
        delimiter: delimiter.as_deref(),
        depth,
    };
    Ok(match collapsing {
        Some(Collapsing::CitationNumber) => {
            f.delimiter = None;
            let groups = group_successive(xs, is_adjacent_citation_number);
            let mut acc = Vec::new();
            for group in groups.into_iter().rev() {
                if !acc.is_empty() {
                    let between = if group.len() >= 3 {
                        after_collapse_delimiter
                    } else {
                        delimiter.as_deref()
                    };
                    acc.push(literal(between));
                }
                acc.push(collapse.range(group));
            }
            acc.reverse();
            Output::Formatted(f, acc)
        }
        Some(kind) => {
            f.delimiter = None;
            let groups = collapse.grouped(xs);
            let mut acc: Vec<Output> = Vec::new();
            for group in groups.into_iter().rev() {
                if let Some(x) = collapse.group(kind, group, !acc.is_empty())? {
                    acc.push(x);
                }
            }
            acc.reverse();
            Output::Formatted(f, acc)
        }
        None => {
            let groups = collapse.grouped(xs);
            let parts = groups
                .into_iter()
                .map(|group| delimited(Some(group_delimiter.to_owned()), group))
                .collect();
            Output::Formatted(f, parts)
        }
    })
}

struct Collapse<'a> {
    group_delimiter: &'a str,
    year_suffix_delimiter: Option<&'a str>,
    after_collapse_delimiter: Option<&'a str>,
    delimiter: Option<&'a str>,
    depth: usize,
}

impl Collapse<'_> {
    /// `collapseRange` without the delimiter that follows it.
    fn range(&self, ys: Vec<Output>) -> Output {
        if ys.len() >= 3 {
            let mut ys = ys;
            let last = ys.pop().unwrap_or(Output::Null);
            let first = ys.into_iter().next().unwrap_or(Output::Null);
            delimited(Some("\u{2013}".to_owned()), vec![first, last])
        } else {
            delimited(self.delimiter.map(str::to_owned), ys)
        }
    }

    /// `groupedItems`: `groupWith` over the annotated items. An item with a
    /// suffix stands alone, and no item moves past a prefix or a suffix.
    /// Upstream puts the unmatched items back before the rest; that order
    /// is the order of the items without the taken ones, so this port
    /// marks the taken items instead of moving the others.
    fn grouped(&self, xs: Vec<Output>) -> Vec<Vec<Output>> {
        let items: Vec<Item> = xs.into_iter().map(annotate).collect();
        let mut taken = vec![false; items.len()];
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for (i, z) in items.iter().enumerate() {
            if taken[i] {
                continue;
            }
            taken[i] = true;
            let mut group = vec![i];
            if !z.has_suffix {
                for (j, w) in items.iter().enumerate().skip(i + 1) {
                    if taken[j] {
                        continue;
                    }
                    if w.has_prefix || w.has_suffix {
                        break;
                    }
                    if same_names(z, w) {
                        taken[j] = true;
                        group.push(j);
                    }
                }
            }
            groups.push(group);
        }
        let mut outputs: Vec<Option<Output>> = items.into_iter().map(|i| Some(i.output)).collect();
        groups
            .into_iter()
            .map(|group| {
                group
                    .into_iter()
                    .filter_map(|n| outputs.get_mut(n).and_then(Option::take))
                    .collect()
            })
            .collect()
    }

    /// `collapseGroup` with `null zs` given as `!more`.
    fn group(
        &self,
        kind: Collapsing,
        group: Vec<Output>,
        more: bool,
    ) -> Result<Option<Output>, EvalError> {
        let mut group = group.into_iter();
        let Some(y) = group.next() else {
            return Ok(None);
        };
        let ys: Vec<Output> = group.collect();
        let mut without_names = vec![y.clone()];
        for u in &ys {
            let mut u = u.clone();
            transform(&mut u, self.depth, &mut |x| {
                if matches!(x, Output::Tagged(Tag::Names { .. }, _)) {
                    *x = Output::Null;
                }
            })?;
            if !output::output_to_text(&u).is_empty() {
                without_names.push(u);
            }
        }
        let ws = self.collapse_year_suffix(kind, without_names.clone())?;
        let mut original = vec![y];
        original.extend(ys);
        let no_collapse = ws == original;
        let no_year_suffix_collapse = ws == without_names;
        let any_locator = ws.iter().any(|u| {
            universe(u)
                .iter()
                .any(|x| matches!(x, Output::Tagged(Tag::Locator, _)))
        });
        let flipped = kind == Collapsing::Year;
        let after = self
            .after_collapse_delimiter
            .or(self.delimiter)
            .map(str::to_owned);
        let count = ws.len();
        let mut parts = Vec::with_capacity(count);
        for (n, u) in ws.into_iter().enumerate() {
            if n + 1 == count {
                parts.push(u);
            } else {
                let suffix =
                    if no_collapse || (no_year_suffix_collapse && !(flipped && any_locator)) {
                        Some(self.group_delimiter.to_owned())
                    } else {
                        after.clone()
                    };
                parts.push(raw(
                    Formatting {
                        suffix,
                        ..Formatting::default()
                    },
                    vec![u],
                ));
            }
        }
        let suffix = if !more {
            None
        } else if no_collapse && !flipped {
            self.delimiter.map(str::to_owned)
        } else {
            after
        };
        Ok(Some(raw(
            Formatting {
                suffix,
                ..Formatting::default()
            },
            parts,
        )))
    }

    /// `collapseYearSuffix`.
    fn collapse_year_suffix(
        &self,
        kind: Collapsing,
        zs: Vec<Output>,
    ) -> Result<Vec<Output>, EvalError> {
        let ranges = match kind {
            Collapsing::YearSuffix => false,
            Collapsing::YearSuffixRanged => true,
            _ => return Ok(zs),
        };
        let mut items = Vec::new();
        let mut current: Vec<Output> = Vec::new();
        for mut item in zs {
            match current.first() {
                None => current.push(item),
                Some(z) if years(z) == years(&item) => {
                    transform(&mut item, self.depth, &mut |x| {
                        if let Output::Tagged(Tag::Date(_), inner) = x {
                            let suffix = universe(inner)
                                .into_iter()
                                .find(|y| matches!(y, Output::Tagged(Tag::YearSuffix(_), _)))
                                .cloned()
                                .unwrap_or(Output::Null);
                            **inner = suffix;
                        }
                    })?;
                    current.push(item);
                }
                Some(_) => {
                    let group = std::mem::replace(&mut current, vec![item]);
                    items.push(self.year_suffix_group(ranges, group));
                }
            }
        }
        items.push(self.year_suffix_group(ranges, current));
        Ok(items)
    }

    /// `yearSuffixGroup`.
    fn year_suffix_group(&self, ranges: bool, mut zs: Vec<Output>) -> Output {
        if zs.len() == 1 {
            return zs.pop().unwrap_or(Output::Null);
        }
        let parts = if ranges {
            group_successive(zs, is_successive)
                .into_iter()
                .map(|group| self.rangify(group))
                .collect()
        } else {
            zs
        };
        delimited(self.year_suffix_delimiter.map(str::to_owned), parts)
    }

    /// `rangifyGroup`.
    fn rangify(&self, mut zs: Vec<Output>) -> Output {
        match zs.len() {
            1 => zs.pop().unwrap_or(Output::Null),
            n if n >= 3 => {
                let last = zs.pop().unwrap_or(Output::Null);
                let first = zs.into_iter().next().unwrap_or(Output::Null);
                delimited(Some("\u{2013}".to_owned()), vec![first, last])
            }
            _ => delimited(self.year_suffix_delimiter.map(str::to_owned), zs),
        }
    }
}

/// `groupSuccessive`: the runs of items where each item follows the one
/// before it.
fn group_successive(zs: Vec<Output>, adjacent: fn(&Output, &Output) -> bool) -> Vec<Vec<Output>> {
    let mut groups: Vec<Vec<Output>> = Vec::new();
    for z in zs {
        match groups.last_mut() {
            Some(group) if group.last().is_some_and(|last| adjacent(last, &z)) => group.push(z),
            _ => groups.push(vec![z]),
        }
    }
    groups
}

fn year_suffixes(x: &Output) -> Vec<i64> {
    universe(x)
        .into_iter()
        .filter_map(|y| match y {
            Output::Tagged(Tag::YearSuffix(n), _) => Some(*n),
            _ => None,
        })
        .collect()
}

/// `isSuccessive`: two items with one year suffix each, the second one next.
fn is_successive(x: &Output, y: &Output) -> bool {
    matches!((year_suffixes(x).as_slice(), year_suffixes(y).as_slice()), ([c], [d]) if *d == c + 1)
}

/// `isAdjacentCitationNumber`.
fn is_adjacent_citation_number(x: &Output, y: &Output) -> bool {
    fn number(x: &Output) -> Option<i64> {
        let Output::Tagged(Tag::Item { .. }, inner) = x else {
            return None;
        };
        match inner.as_ref() {
            Output::Formatted(_, parts) => match parts.as_slice() {
                [Output::Tagged(Tag::CitationNumber(n), _)] => Some(*n),
                _ => None,
            },
            Output::Tagged(Tag::CitationNumber(n), _) => Some(*n),
            _ => None,
        }
    }
    matches!((number(x), number(y)), (Some(a), Some(b)) if b == a + 1)
}

/// `getYears`: the first parts of the dates that are not literal.
fn years(x: &Output) -> Vec<Vec<Option<i64>>> {
    universe(x)
        .into_iter()
        .filter_map(|y| match y {
            Output::Tagged(Tag::Date(date), _) if date.literal.is_none() => Some(date_years(date)),
            _ => None,
        })
        .collect()
}

fn date_years(date: &Date) -> Vec<Option<i64>> {
    date.parts.iter().map(|p| p.0.first().copied()).collect()
}

fn annotate(x: Output) -> Item {
    let all = universe(&x);
    let has_prefix = all
        .iter()
        .any(|y| matches!(y, Output::Tagged(Tag::Prefix, _)));
    let has_suffix = all
        .iter()
        .any(|y| matches!(y, Output::Tagged(Tag::Suffix, _)));
    let tagged = extract_tagged(&all);
    let mut words = Vec::new();
    leading(&x, &mut words);
    let lead = words.join(" ");
    let keys = keys_of(tagged.as_ref(), &lead);
    Item {
        has_prefix,
        has_suffix,
        tagged,
        lead,
        keys,
        output: x,
    }
}

/// `extractTagged`: the first names of the items that are not author-only,
/// else their first date.
fn extract_tagged(all: &[&Output]) -> Option<Output> {
    let items: Vec<&Output> = all
        .iter()
        .copied()
        .filter(|y| {
            matches!(y, Output::Tagged(Tag::Item { kind, .. }, _) if *kind != ItemType::AuthorOnly)
        })
        .collect();
    if items.is_empty() {
        return None;
    }
    let inside: Vec<&Output> = items.iter().flat_map(|i| universe(i)).collect();
    inside
        .iter()
        .find(|y| matches!(y, Output::Tagged(Tag::Names { .. }, _)))
        .or_else(|| {
            inside
                .iter()
                .find(|y| matches!(y, Output::Tagged(Tag::Date(_), _)))
        })
        .map(|y| (*y).clone())
}

/// `leadingText`: the texts before the first date, without the prefix.
/// The result is true when a date ends the walk.
fn leading(x: &Output, words: &mut Vec<String>) -> bool {
    match x {
        Output::Tagged(Tag::Date(_), _) => true,
        Output::Tagged(Tag::Prefix, _) => false,
        Output::Tagged(_, y) | Output::InNote(y) => leading(y, words),
        Output::Formatted(_, ys) | Output::Linked(_, ys) => ys.iter().any(|y| leading(y, words)),
        Output::Literal(t) => {
            words.push(output::stringify(t));
            false
        }
        Output::Null => false,
    }
}

/// `sameNames`.
fn same_names(x: &Item, y: &Item) -> bool {
    let key = |n: usize| x.keys[n].is_some() && x.keys[n] == y.keys[n];
    if !key(0) && !key(1) {
        return false;
    }
    match (&x.tagged, &y.tagged) {
        (
            Some(Output::Tagged(
                Tag::Names {
                    variable: t1,
                    names: ns1,
                    ..
                },
                ws1,
            )),
            Some(Output::Tagged(
                Tag::Names {
                    variable: t2,
                    names: ns2,
                    ..
                },
                ws2,
            )),
        ) => {
            t1 == t2
                && if ns1 == ns2 {
                    !ns1.is_empty() || ws1 == ws2
                } else {
                    ws1 == ws2
                }
        }
        (Some(Output::Tagged(Tag::Date(_), _)), Some(Output::Tagged(Tag::Date(_), _))) => {
            x.lead == y.lead
        }
        _ => false,
    }
}
