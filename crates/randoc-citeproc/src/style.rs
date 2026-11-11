//! The style parser: a port of `Citeproc.Style` (citeproc 0.14.1.1,
//! BSD-2-Clause, see `NOTICE.md`).

use std::collections::{BTreeMap, BTreeSet};

use crate::element::{date, formatting, locale, term_form};
use crate::error::CiteprocError;
use crate::lang::parse_lang;
use crate::types::{
    ChooseBranch, Collapsing, Condition, DelimiterPrecedes, DemoteNonDroppingParticle,
    DisambiguationStrategy, Element, ElementType, GivenNameDisambiguationRule, Layout,
    LayoutOptions, Match, NameAsSortOrder, NameForm, NameFormat, NamesFormat, NumberForm,
    PageRangeFormat, Pluralize, Position, SecondFieldAlign, SortDirection, SortKey, Style,
    StyleOptions, SubsequentAuthorSubstituteRule, Term, TermForm, TermNumber, TextType, Variable,
    VariableForm, read_as_int,
};
use crate::xml::{XmlElement, parse_document};
use randoc_core::shared::show_string;

/// Parses a style. `parent` returns the text of the independent parent of a
/// dependent style, given the `href` of its link.
pub(crate) fn parse_style<E>(
    text: &str,
    max_depth: usize,
    parent: &mut dyn FnMut(&str) -> Result<String, E>,
) -> Result<Result<Style, CiteprocError>, E> {
    let root = match parse_document(text, max_depth) {
        Ok(root) => root,
        Err(error) => return Ok(Err(CiteprocError::Xml(error.0))),
    };
    let default_locale = root
        .attr("default-locale")
        .and_then(|tag| parse_lang(tag).ok());
    let link = root
        .children_named("info")
        .flat_map(|info| info.children_named("link"))
        .find(|link| link.attr("rel") == Some("independent-parent"));
    let Some(link) = link else {
        return Ok(StyleParser { max_depth }.style(default_locale, &root));
    };
    let Some(href) = link.attr("href") else {
        return Ok(Err(CiteprocError::Xml(
            "No href attribute on link to parent style".to_owned(),
        )));
    };
    let parent_text = parent(href)?;
    Ok(match parse_document(&parent_text, max_depth) {
        Ok(parent_root) => StyleParser { max_depth }.style(default_locale, &parent_root),
        Err(error) => Err(CiteprocError::Xml(error.0)),
    })
}

struct StyleParser {
    max_depth: usize,
}

fn fail<T>(message: impl Into<String>) -> Result<T, CiteprocError> {
    Err(CiteprocError::Parse(message.into()))
}

impl StyleParser {
    fn style(
        &self,
        default_locale: Option<crate::lang::Lang>,
        node: &XmlElement,
    ) -> Result<Style, CiteprocError> {
        let mut macros = BTreeMap::new();
        for macro_node in node.children_named("macro") {
            let Some(name) = macro_node.attr("name") else {
                return fail("macro element missing name attribute");
            };
            let elements = self.elements(macro_node)?;
            macros.insert(name.to_owned(), elements);
        }
        let citation_nodes: Vec<&XmlElement> = node.children_named("citation").collect();
        let (citation_node, citation) = match citation_nodes.as_slice() {
            [one] => (*one, self.layout(one)?),
            [] => return fail("No citation element present"),
            _ => return fail("More than one citation element present"),
        };
        let bibliography_nodes: Vec<&XmlElement> = node.children_named("bibliography").collect();
        let (bibliography_node, bibliography) = match bibliography_nodes.as_slice() {
            [one] => (Some(*one), Some(self.layout(one)?)),
            [] => (None, None),
            _ => return fail("More than one bibliography element present"),
        };
        let cattr = |name: &str| citation_node.attr(name);
        let battr = |name: &str| bibliography_node.and_then(|b| b.attr(name));
        let rule = match cattr("givenname-disambiguation-rule") {
            Some("all-names") => GivenNameDisambiguationRule::AllNames,
            Some("all-names-with-initials") => GivenNameDisambiguationRule::AllNamesWithInitials,
            Some("primary-name") => GivenNameDisambiguationRule::PrimaryName,
            Some("primary-name-with-initials") => {
                GivenNameDisambiguationRule::PrimaryNameWithInitials
            }
            _ => GivenNameDisambiguationRule::ByCite,
        };
        let disambiguation = DisambiguationStrategy {
            add_names: cattr("disambiguate-add-names") == Some("true"),
            add_given_names: (cattr("disambiguate-add-givenname") == Some("true")).then_some(rule),
            add_year_suffix: cattr("disambiguate-add-year-suffix") == Some("true"),
        };
        let mut search = YearSuffixSearch {
            macros: &macros,
            visited: BTreeSet::new(),
            max_depth: self.max_depth,
        };
        let mut uses_year_suffix_variable = false;
        for element in citation
            .elements
            .iter()
            .chain(bibliography.iter().flat_map(|b| b.elements.iter()))
        {
            if search.element(element, 0)? {
                uses_year_suffix_variable = true;
                break;
            }
        }
        let attr = |name: &str| node.attr(name);
        let options = StyleOptions {
            is_note_style: matches!(attr("class"), Some("note") | None),
            default_locale,
            demote_non_dropping_particle: match attr("demote-non-dropping-particle") {
                Some("never") => DemoteNonDroppingParticle::Never,
                Some("sort-only") => DemoteNonDroppingParticle::SortOnly,
                _ => DemoteNonDroppingParticle::DisplayAndSort,
            },
            initialize_with_hyphen: attr("initialize-with-hyphen").is_none_or(|v| v == "true"),
            page_range_format: match attr("page-range-format") {
                Some("chicago" | "chicago-15") => Some(PageRangeFormat::Chicago15),
                Some("chicago-16") => Some(PageRangeFormat::Chicago16),
                Some("expanded") => Some(PageRangeFormat::Expanded),
                Some("minimal") => Some(PageRangeFormat::Minimal),
                Some("minimal-two") => Some(PageRangeFormat::MinimalTwo),
                _ => None,
            },
            page_range_delimiter: attr("page-range-delimiter").map(str::to_owned),
            disambiguation,
            near_note_distance: attr("near-note-distance").and_then(read_as_int),
            cite_group_delimiter: cattr("cite-group-delimiter")
                .map(str::to_owned)
                .or_else(|| cattr("collapse").map(|_| ", ".to_owned())),
            line_spacing: battr("line-spacing").and_then(read_as_int),
            entry_spacing: battr("entry-spacing").and_then(read_as_int),
            hanging_indent: battr("hanging-indent") == Some("true"),
            second_field_align: match battr("second-field-align") {
                Some("flush") => Some(SecondFieldAlign::Flush),
                Some("margin") => Some(SecondFieldAlign::Margin),
                _ => None,
            },
            subsequent_author_substitute: battr("subsequent-author-substitute").map(|text| {
                let rule = match battr("subsequent-author-substitute-rule") {
                    Some("complete-each") => SubsequentAuthorSubstituteRule::CompleteEach,
                    Some("partial-each") => SubsequentAuthorSubstituteRule::PartialEach,
                    Some("partial-first") => SubsequentAuthorSubstituteRule::PartialFirst,
                    _ => SubsequentAuthorSubstituteRule::CompleteAll,
                };
                (text.to_owned(), rule)
            }),
            uses_year_suffix_variable,
            name_format: inheritable_name_format(&|name: &str| node.attr(name)),
        };
        let locales = node
            .children_named("locale")
            .map(locale)
            .collect::<Result<Vec<_>, _>>()?;
        let numbers: Vec<Option<i64>> =
            attr("version").map_or_else(Vec::new, |v| v.split('.').map(read_as_int).collect());
        let csl_version = match numbers.as_slice() {
            [Some(x), Some(y), Some(z), ..] => (*x, *y, *z),
            [Some(x), Some(y), ..] => (*x, *y, 0),
            [Some(x), ..] => (*x, 0, 0),
            _ => (0, 0, 0),
        };
        Ok(Style {
            csl_version,
            options,
            citation,
            bibliography,
            locales,
            macros,
        })
    }

    fn elements(&self, node: &XmlElement) -> Result<Vec<Element>, CiteprocError> {
        node.elements().map(|child| self.element(child)).collect()
    }

    fn element(&self, node: &XmlElement) -> Result<Element, CiteprocError> {
        match node.name.local.as_str() {
            "date" => Ok(date(node)),
            "text" => text(node),
            "group" => Ok(Element {
                kind: ElementType::Group(false, self.elements(node)?),
                formatting: formatting(node),
            }),
            "choose" => self.choose(node),
            "number" => number(node),
            "label" => Ok(label(node)),
            "names" => self.names(node),
            name => fail(format!("unknown element {}", show_string(name))),
        }
    }

    fn choose(&self, node: &XmlElement) -> Result<Element, CiteprocError> {
        let mut branches = Vec::new();
        for name in ["if", "else-if", "else"] {
            for branch in node.children_named(name) {
                branches.push(ChooseBranch {
                    matching: match branch.attr("match") {
                        Some("any") => Match::Any,
                        Some("none") => Match::None,
                        _ => Match::All,
                    },
                    conditions: conditions(branch),
                    elements: self.elements(branch)?,
                });
            }
        }
        Ok(Element {
            kind: ElementType::Choose(branches),
            formatting: Formatting::default(),
        })
    }

    fn names(&self, node: &XmlElement) -> Result<Element, CiteprocError> {
        let mut names = NamesFormat::default();
        let mut substitute = Vec::new();
        for child in node.elements() {
            match child.name.local.as_str() {
                "label" => {
                    names.label = Some((
                        term_form(child.attr("form")),
                        pluralize(child),
                        formatting(child),
                    ));
                    names.label_before_name = names.name.is_none();
                }
                "substitute" => substitute = self.elements(child)?,
                "et-al" => {
                    names.et_al = Some((
                        child.attr("term").unwrap_or("et-al").to_owned(),
                        formatting(child),
                    ));
                }
                "name" => names.name = Some((name_format(child), formatting(child))),
                other => {
                    return fail(format!(
                        "element {} not a valid child of names",
                        show_string(other)
                    ));
                }
            }
        }
        let variables = node.attr("variable").map(split_vars).unwrap_or_default();
        Ok(Element {
            kind: ElementType::Names(variables, Box::new(names), substitute),
            formatting: formatting(node),
        })
    }

    fn layout(&self, node: &XmlElement) -> Result<Layout, CiteprocError> {
        let layouts: Vec<&XmlElement> = node.children_named("layout").collect();
        let layout = match layouts.as_slice() {
            [one] => *one,
            [] => return fail(format!("No layout element present in {}", node.name.local)),
            _ => {
                return fail(format!(
                    "Multiple layout elements present in {}",
                    node.name.local
                ));
            }
        };
        let layout_formatting = formatting(layout);
        let elements = self.elements(layout)?;
        let attr = |name: &str| node.attr(name);
        let options = LayoutOptions {
            collapse: match attr("collapse") {
                Some("citation-number") => Some(Collapsing::CitationNumber),
                Some("year") => Some(Collapsing::Year),
                Some("year-suffix") => Some(Collapsing::YearSuffix),
                Some("year-suffix-ranged") => Some(Collapsing::YearSuffixRanged),
                _ => None,
            },
            year_suffix_delimiter: attr("year-suffix-delimiter")
                .or_else(|| attr("cite-group-delimiter"))
                .map(str::to_owned)
                .or_else(|| layout_formatting.delimiter.clone()),
            after_collapse_delimiter: attr("after-collapse-delimiter")
                .map(str::to_owned)
                .or_else(|| layout_formatting.delimiter.clone()),
            name_format: inheritable_name_format(&|name: &str| node.attr(name)),
        };
        let sort_keys = node
            .children_named("sort")
            .flat_map(|sort| sort.children_named("key"))
            .map(sort_key)
            .collect();
        Ok(Layout {
            options,
            formatting: Formatting {
                affixes_inside: true,
                ..layout_formatting
            },
            elements,
            sort_keys,
        })
    }
}

use crate::types::Formatting;

/// Searches the layouts for the variable `year-suffix`, also through macros.
/// Each macro is searched once, so a macro that calls itself ends the search.
struct YearSuffixSearch<'a> {
    macros: &'a BTreeMap<String, Vec<Element>>,
    visited: BTreeSet<&'a str>,
    max_depth: usize,
}

impl<'a> YearSuffixSearch<'a> {
    fn element(&mut self, element: &'a Element, depth: usize) -> Result<bool, CiteprocError> {
        if depth > self.max_depth {
            return Err(CiteprocError::Parse(format!(
                "macros are nested deeper than {} levels (a Randoc limit)",
                self.max_depth
            )));
        }
        match &element.kind {
            ElementType::Text(TextType::Variable(_, variable)) => {
                Ok(*variable == Variable::new("year-suffix"))
            }
            ElementType::Text(TextType::Macro(name)) => {
                match self.macros.get_key_value(name.as_str()) {
                    Some((key, elements)) if self.visited.insert(key.as_str()) => {
                        self.any(elements, depth + 1)
                    }
                    _ => Ok(false),
                }
            }
            ElementType::Group(_, elements) => self.any(elements, depth + 1),
            ElementType::Choose(branches) => {
                for branch in branches {
                    if self.any(&branch.elements, depth + 1)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn any(&mut self, elements: &'a [Element], depth: usize) -> Result<bool, CiteprocError> {
        for element in elements {
            if self.element(element, depth)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// `T.words`: words separated by Haskell `isSpace` characters (the ASCII
/// spaces and the general category Zs), not by each Unicode white space.
pub(crate) fn haskell_words(text: &str) -> impl Iterator<Item = &str> {
    let is_space = |c: char| {
        matches!(
            c,
            ' ' | '\t' | '\n' | '\r' | '\u{B}' | '\u{C}' | '\u{A0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
        )
    };
    text.split(is_space).filter(|word| !word.is_empty())
}

/// `splitVars`: variable names, which compare without regard to case.
fn split_vars(text: &str) -> Vec<Variable> {
    haskell_words(text).map(Variable::new).collect()
}

fn conditions(node: &XmlElement) -> Vec<Condition> {
    let mut out = Vec::new();
    if node.attr("disambiguate") == Some("true") {
        out.push(Condition::WouldDisambiguate);
    }
    let vars = |name: &str| node.attr(name).map(split_vars).unwrap_or_default();
    out.extend(vars("is-numeric").into_iter().map(Condition::IsNumeric));
    out.extend(
        vars("is-uncertain-date")
            .into_iter()
            .map(Condition::IsUncertainDate),
    );
    out.extend(vars("locator").into_iter().map(Condition::HasLocatorType));
    if let Some(positions) = node.attr("position") {
        // Upstream matches the `Variable` of each word, so letter case does not count.
        const POSITIONS: [(&str, Position); 5] = [
            ("first", Position::First),
            ("ibid", Position::Ibid),
            ("ibid-with-locator", Position::IbidWithLocator),
            ("subsequent", Position::Subsequent),
            ("near-note", Position::NearNote),
        ];
        out.extend(split_vars(positions).into_iter().filter_map(|word| {
            POSITIONS
                .iter()
                .find(|(name, _)| Variable::new(name) == word)
                .map(|(_, position)| Condition::HasPosition(*position))
        }));
    }
    if let Some(types) = node.attr("type") {
        out.push(Condition::HasType(
            haskell_words(types).map(str::to_owned).collect(),
        ));
    }
    out.extend(vars("variable").into_iter().map(Condition::HasVariable));
    out
}

fn number(node: &XmlElement) -> Result<Element, CiteprocError> {
    let form = match node.attr("form") {
        Some("ordinal") => NumberForm::Ordinal,
        Some("long-ordinal") => NumberForm::LongOrdinal,
        Some("roman") => NumberForm::Roman,
        _ => NumberForm::Numeric,
    };
    match node.attr("variable") {
        Some(variable) => Ok(Element {
            kind: ElementType::Number(Variable::new(variable), form),
            formatting: formatting(node),
        }),
        None => fail("number element without required variable attribute"),
    }
}

fn pluralize(node: &XmlElement) -> Pluralize {
    match node.attr("plural") {
        Some("always") => Pluralize::Always,
        Some("never") => Pluralize::Never,
        _ => Pluralize::Contextual,
    }
}

fn label(node: &XmlElement) -> Element {
    Element {
        kind: ElementType::Label(
            Variable::new(node.attr("variable").unwrap_or("")),
            term_form(node.attr("form")),
            pluralize(node),
        ),
        formatting: formatting(node),
    }
}

fn text(node: &XmlElement) -> Result<Element, CiteprocError> {
    let attr = |name| node.attr(name);
    let kind = if let Some(variable) = attr("variable") {
        let form = if attr("form") == Some("short") {
            VariableForm::Short
        } else {
            VariableForm::Long
        };
        TextType::Variable(form, Variable::new(variable))
    } else if let Some(name) = attr("macro") {
        TextType::Macro(name.to_owned())
    } else if let Some(name) = attr("term") {
        TextType::Term(Term {
            form: term_form(attr("form")),
            number: match attr("plural") {
                Some("true") => Some(TermNumber::Plural),
                Some("false") => Some(TermNumber::Singular),
                _ => None,
            },
            ..Term::named(name)
        })
    } else if let Some(value) = attr("value") {
        TextType::Value(value.to_owned())
    } else {
        return fail("text element lacks needed attribute");
    };
    Ok(Element {
        kind: ElementType::Text(kind),
        formatting: formatting(node),
    })
}

fn and_style(value: Option<&str>) -> Option<TermForm> {
    match value {
        Some("text") => Some(TermForm::Long),
        Some("symbol") => Some(TermForm::Symbol),
        _ => None,
    }
}

fn precedes(value: Option<&str>) -> Option<DelimiterPrecedes> {
    match value {
        Some("after-inverted-name") => Some(DelimiterPrecedes::AfterInvertedName),
        Some("always") => Some(DelimiterPrecedes::Always),
        Some("never") => Some(DelimiterPrecedes::Never),
        Some("contextual") => Some(DelimiterPrecedes::Contextual),
        _ => None,
    }
}

fn true_false(value: Option<&str>) -> Option<bool> {
    match value {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    }
}

fn name_form(value: Option<&str>) -> Option<NameForm> {
    match value {
        Some("short") => Some(NameForm::Short),
        Some("count") => Some(NameForm::Count),
        Some("long") => Some(NameForm::Long),
        _ => None,
    }
}

fn sort_order(value: Option<&str>) -> Option<NameAsSortOrder> {
    match value {
        Some("all") => Some(NameAsSortOrder::All),
        Some("first") => Some(NameAsSortOrder::First),
        _ => None,
    }
}

/// The name options that `style`, `citation`, `bibliography` and a sort key
/// pass down (`getInheritableNameFormat`).
fn inheritable_name_format<'a>(attr: &dyn Fn(&str) -> Option<&'a str>) -> NameFormat {
    NameFormat {
        given_formatting: None,
        family_formatting: None,
        and_style: and_style(attr("and")),
        delimiter: attr("name-delimiter")
            .or_else(|| attr("names-delimiter"))
            .map(str::to_owned),
        delimiter_precedes_et_al: precedes(attr("delimiter-precedes-et-al")),
        delimiter_precedes_last: precedes(attr("delimiter-precedes-last")),
        et_al_min: attr("et-al-min").and_then(read_as_int),
        et_al_use_first: attr("et-al-use-first").and_then(read_as_int),
        et_al_subsequent_use_first: attr("et-al-subsequent-use-first").and_then(read_as_int),
        et_al_subsequent_min: attr("et-al-subsequent-min").and_then(read_as_int),
        et_al_use_last: true_false(attr("et-al-use-last")),
        form: name_form(attr("name-form")),
        initialize: true_false(attr("initialize")),
        initialize_with: attr("initialize-with").map(str::to_owned),
        as_sort_order: sort_order(attr("name-as-sort-order")),
        sort_separator: attr("sort-separator").map(str::to_owned),
    }
}

/// The options of a `name` element (`pName`).
fn name_format(node: &XmlElement) -> NameFormat {
    let attr = |name| node.attr(name);
    let part = |which: &str| {
        node.children_named("name-part")
            .find(|p| p.attr("name") == Some(which))
            .map(formatting)
    };
    NameFormat {
        given_formatting: part("given"),
        family_formatting: part("family"),
        and_style: and_style(attr("and")),
        delimiter: attr("delimiter").map(str::to_owned),
        delimiter_precedes_et_al: precedes(attr("delimiter-precedes-et-al")),
        delimiter_precedes_last: precedes(attr("delimiter-precedes-last")),
        et_al_min: attr("et-al-min").and_then(read_as_int),
        et_al_use_first: attr("et-al-use-first").and_then(read_as_int),
        et_al_subsequent_use_first: attr("et-al-subsequent-use-first").and_then(read_as_int),
        et_al_subsequent_min: attr("et-al-subsequent-min").and_then(read_as_int),
        et_al_use_last: true_false(attr("et-al-use-last")),
        form: name_form(attr("form")),
        initialize: true_false(attr("initialize")),
        initialize_with: attr("initialize-with").map(str::to_owned),
        as_sort_order: sort_order(attr("name-as-sort-order")),
        sort_separator: attr("sort-separator").map(str::to_owned),
    }
}

fn sort_key(node: &XmlElement) -> SortKey {
    let direction = if node.attr("sort") == Some("descending") {
        SortDirection::Descending
    } else {
        SortDirection::Ascending
    };
    // `names-*` replaces `et-al-*`; `mapKeys` keeps the greater original key, which is `names-*`.
    let renamed = |name: &str| -> Option<&'_ str> {
        let alias = match name {
            "et-al-min" => Some("names-min"),
            "et-al-use-first" => Some("names-use-first"),
            "et-al-use-last" => Some("names-use-last"),
            "et-al-subsequent-min" => Some("names-subsequent-min"),
            "et-al-subsequent-use-first" => Some("names-subsequent-use-first"),
            _ => None,
        };
        alias
            .and_then(|alias| node.attr(alias))
            .or_else(|| node.attr(name))
    };
    let name_format = inheritable_name_format(&renamed);
    match node.attr("macro") {
        Some(name) => SortKey::Macro(direction, Box::new(name_format), name.to_owned()),
        None => SortKey::Variable(
            direction,
            Variable::new(node.attr("variable").unwrap_or("")),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        DatePartForm, DatePartName, DateType, DisplayStyle, FontStyle, FontVariant, FontWeight,
        ShowDateParts, TextCase, TextDecoration, VerticalAlign,
    };

    const NS: &str = "xmlns=\"http://purl.org/net/xbiblio/csl\"";

    fn style(body: &str, attrs: &str) -> Style {
        let text = format!("<style {NS}{attrs}>{body}</style>");
        let mut no_parent = |_: &str| -> Result<String, ()> { Err(()) };
        parse_style(&text, 128, &mut no_parent).unwrap().unwrap()
    }

    fn layout(inner: &str) -> Vec<Element> {
        style(
            &format!("<citation><layout>{inner}</layout></citation>"),
            "",
        )
        .citation
        .elements
    }

    fn one(inner: &str) -> Element {
        layout(inner).remove(0)
    }

    #[test]
    fn formatting_attributes_are_read() {
        let element = one(
            "<text value=\"v\" font-style=\"italic\" font-variant=\"small-caps\" font-weight=\"bold\" \
             text-decoration=\"underline\" vertical-align=\"sup\" prefix=\"(\" suffix=\")\" display=\"block\" \
             text-case=\"title\" delimiter=\", \" strip-periods=\"true\" quotes=\"true\"/>",
        );
        let f = element.formatting;
        assert_eq!(f.font_style, Some(FontStyle::Italic));
        assert_eq!(f.font_variant, Some(FontVariant::SmallCaps));
        assert_eq!(f.font_weight, Some(FontWeight::Bold));
        assert_eq!(f.text_decoration, Some(TextDecoration::Underline));
        assert_eq!(f.vertical_align, Some(VerticalAlign::Sup));
        assert_eq!(
            (f.prefix.as_deref(), f.suffix.as_deref()),
            (Some("("), Some(")"))
        );
        assert_eq!(f.display, Some(DisplayStyle::Block));
        assert_eq!(f.text_case, Some(TextCase::TitleCase));
        assert_eq!(f.delimiter.as_deref(), Some(", "));
        assert!(f.strip_periods && f.quotes && !f.affixes_inside);
        let values = [
            ("font-style", "oblique"),
            ("font-style", "normal"),
            ("font-variant", "normal"),
            ("font-weight", "light"),
            ("font-weight", "normal"),
            ("text-decoration", "none"),
            ("vertical-align", "sub"),
            ("vertical-align", "baseline"),
            ("display", "left-margin"),
            ("display", "right-inline"),
            ("display", "indent"),
            ("text-case", "lowercase"),
            ("text-case", "uppercase"),
            ("text-case", "capitalize-first"),
            ("text-case", "capitalize-all"),
            ("text-case", "sentence"),
        ];
        let got: Vec<String> = values
            .iter()
            .map(|(k, v)| {
                format!(
                    "{:?}",
                    one(&format!("<text value=\"v\" {k}=\"{v}\"/>")).formatting
                )
            })
            .collect();
        for (index, (k, v)) in values.iter().enumerate() {
            let expected = match (*k, *v) {
                (_, "oblique") => "font_style: Some(Oblique)",
                ("font-style", "normal") => "font_style: Some(Normal)",
                ("font-variant", "normal") => "font_variant: Some(Normal)",
                (_, "light") => "font_weight: Some(Light)",
                ("font-weight", "normal") => "font_weight: Some(Normal)",
                ("text-decoration", "none") => "text_decoration: Some(None)",
                (_, "sub") => "vertical_align: Some(Sub)",
                (_, "baseline") => "vertical_align: Some(Baseline)",
                (_, "left-margin") => "display: Some(LeftMargin)",
                (_, "right-inline") => "display: Some(RightInline)",
                (_, "indent") => "display: Some(Indent)",
                (_, "lowercase") => "text_case: Some(Lowercase)",
                (_, "uppercase") => "text_case: Some(Uppercase)",
                (_, "capitalize-first") => "text_case: Some(CapitalizeFirst)",
                (_, "capitalize-all") => "text_case: Some(CapitalizeAll)",
                _ => "text_case: Some(SentenceCase)",
            };
            assert!(got[index].contains(expected), "{k}={v}: {}", got[index]);
        }
        let plain =
            one("<text value=\"v\" font-style=\"x\" strip-periods=\"yes\" quotes=\"false\"/>")
                .formatting;
        assert_eq!(plain, Formatting::default());
    }

    #[test]
    fn text_prefers_variable_then_macro_then_term_then_value() {
        let kind = |attrs: &str| one(&format!("<text {attrs}/>")).kind;
        assert_eq!(
            kind("variable=\"title\" macro=\"m\" form=\"short\""),
            ElementType::Text(TextType::Variable(
                VariableForm::Short,
                Variable::new("title")
            ))
        );
        assert_eq!(
            kind("macro=\"m\" term=\"t\""),
            ElementType::Text(TextType::Macro("m".into()))
        );
        assert_eq!(
            kind("term=\"page\" form=\"verb-short\" plural=\"true\" value=\"v\""),
            ElementType::Text(TextType::Term(Term {
                form: TermForm::VerbShort,
                number: Some(TermNumber::Plural),
                ..Term::named("page")
            }))
        );
        assert_eq!(
            kind("term=\"page\" form=\"symbol\" plural=\"false\""),
            ElementType::Text(TextType::Term(Term {
                form: TermForm::Symbol,
                number: Some(TermNumber::Singular),
                ..Term::named("page")
            }))
        );
        assert_eq!(
            kind("term=\"page\" form=\"verb\""),
            ElementType::Text(TextType::Term(Term {
                form: TermForm::Verb,
                ..Term::named("page")
            }))
        );
        assert_eq!(
            kind("term=\"page\" form=\"short\""),
            ElementType::Text(TextType::Term(Term {
                form: TermForm::Short,
                ..Term::named("page")
            }))
        );
        assert_eq!(
            kind("value=\"v\" form=\"long\""),
            ElementType::Text(TextType::Value("v".into()))
        );
        assert_eq!(
            kind("variable=\"title\" form=\"long\""),
            ElementType::Text(TextType::Variable(
                VariableForm::Long,
                Variable::new("title")
            ))
        );
    }

    #[test]
    fn number_label_and_date_attributes_are_read() {
        let number =
            |form: &str| one(&format!("<number variable=\"edition\" form=\"{form}\"/>")).kind;
        assert_eq!(
            number("ordinal"),
            ElementType::Number(Variable::new("edition"), NumberForm::Ordinal)
        );
        assert_eq!(
            number("long-ordinal"),
            ElementType::Number(Variable::new("edition"), NumberForm::LongOrdinal)
        );
        assert_eq!(
            number("roman"),
            ElementType::Number(Variable::new("edition"), NumberForm::Roman)
        );
        assert_eq!(
            number("x"),
            ElementType::Number(Variable::new("edition"), NumberForm::Numeric)
        );
        assert_eq!(
            one("<label variable=\"page\" form=\"short\" plural=\"always\"/>").kind,
            ElementType::Label(Variable::new("page"), TermForm::Short, Pluralize::Always)
        );
        assert_eq!(
            one("<label plural=\"never\"/>").kind,
            ElementType::Label(Variable::new(""), TermForm::Long, Pluralize::Never)
        );
        let date = one(
            "<date variable=\"issued\" form=\"text\" date-parts=\"year-month\" prefix=\"[\">\
             <date-part name=\"month\" form=\"short\" range-delimiter=\"/\"/><date-part name=\"day\"/>\
             <date-part name=\"year\"/><date-part name=\"day\" form=\"ordinal\"/>\
             <date-part name=\"day\" form=\"numeric-leading-zeros\"/><date-part name=\"month\" form=\"numeric\"/>\
             <date-part name=\"month\" form=\"long\"/></date>",
        );
        assert_eq!(date.formatting.prefix.as_deref(), Some("["));
        let ElementType::Date {
            variable,
            date_type,
            show_date_parts,
            parts,
        } = date.kind
        else {
            panic!("not a date")
        };
        assert_eq!(variable, Variable::new("issued"));
        assert_eq!(date_type, DateType::LocalizedText);
        assert_eq!(show_date_parts, Some(ShowDateParts::YearMonth));
        let summary: Vec<(DatePartName, DatePartForm, &str)> = parts
            .iter()
            .map(|p| (p.name, p.form, p.range_delimiter.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                (DatePartName::Month, DatePartForm::Short, "/"),
                (DatePartName::Day, DatePartForm::Numeric, "\u{2013}"),
                (DatePartName::Year, DatePartForm::Long, "\u{2013}"),
                (DatePartName::Day, DatePartForm::Ordinal, "\u{2013}"),
                (
                    DatePartName::Day,
                    DatePartForm::NumericLeadingZeros,
                    "\u{2013}"
                ),
                (DatePartName::Month, DatePartForm::Numeric, "\u{2013}"),
                (DatePartName::Month, DatePartForm::Long, "\u{2013}"),
            ]
        );
        let kinds = |form: &str, parts: &str| match one(&format!(
            "<date form=\"{form}\" date-parts=\"{parts}\"/>"
        ))
        .kind
        {
            ElementType::Date {
                date_type,
                show_date_parts,
                ..
            } => (date_type, show_date_parts),
            _ => panic!("not a date"),
        };
        assert_eq!(
            kinds("numeric", "year-month-day"),
            (
                DateType::LocalizedNumeric,
                Some(ShowDateParts::YearMonthDay)
            )
        );
        assert_eq!(
            kinds("x", "year"),
            (DateType::NonLocalized, Some(ShowDateParts::Year))
        );
        assert_eq!(kinds("x", "x"), (DateType::NonLocalized, None));
    }

    #[test]
    fn choose_puts_if_then_else_if_then_else_and_orders_the_conditions() {
        let element = one("<choose><else><text value=\"e\"/></else>\
             <else-if match=\"none\" position=\"subsequent near-note bogus\"><text value=\"ei\"/></else-if>\
             <if match=\"any\" variable=\"title  URL\" type=\" book chapter \" position=\"first ibid ibid-with-locator\" \
             locator=\"page\" is-uncertain-date=\"issued\" is-numeric=\"volume\" disambiguate=\"true\"/></choose>");
        let ElementType::Choose(branches) = element.kind else {
            panic!("not a choose")
        };
        assert_eq!(
            branches.iter().map(|b| b.matching).collect::<Vec<_>>(),
            [Match::Any, Match::None, Match::All]
        );
        assert_eq!(
            branches[0].conditions,
            [
                Condition::WouldDisambiguate,
                Condition::IsNumeric(Variable::new("volume")),
                Condition::IsUncertainDate(Variable::new("issued")),
                Condition::HasLocatorType(Variable::new("page")),
                Condition::HasPosition(Position::First),
                Condition::HasPosition(Position::Ibid),
                Condition::HasPosition(Position::IbidWithLocator),
                Condition::HasType(vec!["book".into(), "chapter".into()]),
                Condition::HasVariable(Variable::new("title")),
                Condition::HasVariable(Variable::new("URL")),
            ]
        );
        assert_eq!(
            branches[1].conditions,
            [
                Condition::HasPosition(Position::Subsequent),
                Condition::HasPosition(Position::NearNote)
            ]
        );
        assert!(branches[2].conditions.is_empty());
        assert_eq!(branches[2].elements.len(), 1);
    }

    #[test]
    fn position_values_ignore_letter_case_and_words_split_on_haskell_spaces() {
        // Review finding citeproc-R1-04: upstream compares each word as a `Variable`.
        let element = one(
            "<choose><if position=\"SUBSEQUENT First bogus\" variable=\"a\u{2028}b c\u{A0}d\"/></choose>",
        );
        let ElementType::Choose(branches) = element.kind else {
            panic!("not a choose")
        };
        assert_eq!(
            branches[0].conditions,
            [
                Condition::HasPosition(Position::Subsequent),
                Condition::HasPosition(Position::First),
                Condition::HasVariable(Variable::new("a\u{2028}b")),
                Condition::HasVariable(Variable::new("c")),
                Condition::HasVariable(Variable::new("d")),
            ]
        );
    }

    #[test]
    fn names_read_name_label_et_al_and_substitute() {
        let element = one(
            "<names variable=\"author editor\" delimiter=\"; \"><label form=\"short\" plural=\"never\" prefix=\" \"/>\
             <name and=\"symbol\" delimiter=\"/\" delimiter-precedes-et-al=\"always\" delimiter-precedes-last=\"never\" \
             et-al-min=\"4\" et-al-use-first=\"1\" et-al-subsequent-use-first=\"2\" et-al-subsequent-min=\"3\" \
             et-al-use-last=\"true\" form=\"count\" initialize=\"false\" initialize-with=\". \" \
             name-as-sort-order=\"first\" sort-separator=\" \" font-weight=\"bold\">\
             <name-part name=\"family\" text-case=\"uppercase\"/><name-part name=\"given\" font-style=\"italic\"/>\
             <name-part name=\"given\" font-style=\"normal\"/></name>\
             <et-al term=\"and others\" font-style=\"italic\"/><substitute><names variable=\"editor\"/><text value=\"x\"/></substitute></names>",
        );
        let ElementType::Names(variables, names, substitute) = element.kind else {
            panic!("not names")
        };
        assert_eq!(
            variables,
            [Variable::new("author"), Variable::new("editor")]
        );
        assert_eq!(element.formatting.delimiter.as_deref(), Some("; "));
        assert_eq!(substitute.len(), 2);
        assert!(names.label_before_name);
        let (form, plural, label_format) = names.label.clone().unwrap();
        assert_eq!(
            (form, plural, label_format.prefix.as_deref()),
            (TermForm::Short, Pluralize::Never, Some(" "))
        );
        let (term, et_al_format) = names.et_al.clone().unwrap();
        assert_eq!(
            (term.as_str(), et_al_format.font_style),
            ("and others", Some(FontStyle::Italic))
        );
        let (name, name_format) = names.name.clone().unwrap();
        assert_eq!(name_format.font_weight, Some(FontWeight::Bold));
        assert_eq!(
            name.family_formatting.and_then(|f| f.text_case),
            Some(TextCase::Uppercase)
        );
        assert_eq!(
            name.given_formatting.and_then(|f| f.font_style),
            Some(FontStyle::Italic)
        );
        assert_eq!(name.and_style, Some(TermForm::Symbol));
        assert_eq!(name.delimiter.as_deref(), Some("/"));
        assert_eq!(
            name.delimiter_precedes_et_al,
            Some(DelimiterPrecedes::Always)
        );
        assert_eq!(name.delimiter_precedes_last, Some(DelimiterPrecedes::Never));
        assert_eq!(
            (
                name.et_al_min,
                name.et_al_use_first,
                name.et_al_subsequent_use_first,
                name.et_al_subsequent_min
            ),
            (Some(4), Some(1), Some(2), Some(3))
        );
        assert_eq!(name.et_al_use_last, Some(true));
        assert_eq!(name.form, Some(NameForm::Count));
        assert_eq!(name.initialize, Some(false));
        assert_eq!(name.initialize_with.as_deref(), Some(". "));
        assert_eq!(name.as_sort_order, Some(NameAsSortOrder::First));
        assert_eq!(name.sort_separator.as_deref(), Some(" "));
        let after = one(
            "<names variable=\"author\"><name and=\"text\" form=\"short\"/><label/><et-al/></names>",
        );
        let ElementType::Names(_, names, _) = after.kind else {
            panic!("not names")
        };
        assert!(!names.label_before_name);
        assert_eq!(names.et_al.map(|e| e.0).as_deref(), Some("et-al"));
        let name = names.name.unwrap().0;
        assert_eq!(
            (name.and_style, name.form),
            (Some(TermForm::Long), Some(NameForm::Short))
        );
        let values = |attr: &str, value: &str| {
            let element = one(&format!(
                "<names variable=\"author\"><name {attr}=\"{value}\"/></names>"
            ));
            let ElementType::Names(_, names, _) = element.kind else {
                panic!("not names")
            };
            names.name.unwrap().0
        };
        assert_eq!(
            values("delimiter-precedes-last", "after-inverted-name").delimiter_precedes_last,
            Some(DelimiterPrecedes::AfterInvertedName)
        );
        assert_eq!(
            values("delimiter-precedes-last", "contextual").delimiter_precedes_last,
            Some(DelimiterPrecedes::Contextual)
        );
        assert_eq!(
            values("delimiter-precedes-last", "x").delimiter_precedes_last,
            None
        );
        assert_eq!(
            values("et-al-use-last", "false").et_al_use_last,
            Some(false)
        );
        assert_eq!(values("et-al-use-last", "x").et_al_use_last, None);
        assert_eq!(values("form", "long").form, Some(NameForm::Long));
        assert_eq!(values("initialize", "true").initialize, Some(true));
        assert_eq!(
            values("name-as-sort-order", "all").as_sort_order,
            Some(NameAsSortOrder::All)
        );
        assert_eq!(values("et-al-min", "x").et_al_min, None);
        assert_eq!(values("and", "x").and_style, None);
    }

    #[test]
    fn style_options_come_from_style_citation_and_bibliography() {
        let s = style(
            "<citation disambiguate-add-names=\"true\" disambiguate-add-givenname=\"true\" \
             givenname-disambiguation-rule=\"primary-name\" disambiguate-add-year-suffix=\"true\" collapse=\"year\" \
             year-suffix-delimiter=\";\" after-collapse-delimiter=\"|\" name-form=\"short\">\
             <sort><key variable=\"issued\" sort=\"descending\"/><key macro=\"m\" names-min=\"3\" et-al-min=\"9\" \
             names-use-first=\"1\" names-use-last=\"true\" et-al-subsequent-min=\"5\"/></sort>\
             <layout delimiter=\"; \" prefix=\"(\"><text variable=\"year-suffix\"/></layout></citation>\
             <bibliography line-spacing=\"2\" entry-spacing=\"0\" hanging-indent=\"true\" second-field-align=\"flush\" \
             subsequent-author-substitute=\"---\" subsequent-author-substitute-rule=\"partial-first\" et-al-min=\"7\">\
             <layout><text value=\"b\"/></layout></bibliography><macro name=\"m\"><text value=\"1\"/></macro>",
            " class=\"in-text\" version=\"1.0.2\" default-locale=\"de-AT\" demote-non-dropping-particle=\"sort-only\" \
             initialize-with-hyphen=\"false\" page-range-format=\"chicago\" page-range-delimiter=\"-\" \
             near-note-distance=\"3\" and=\"symbol\" names-delimiter=\";\" name-as-sort-order=\"all\"",
        );
        let o = &s.options;
        assert_eq!(s.csl_version, (1, 0, 2));
        assert!(!o.is_note_style);
        assert_eq!(
            o.default_locale
                .as_ref()
                .map(crate::lang::render_lang)
                .as_deref(),
            Some("de-AT")
        );
        assert_eq!(
            o.demote_non_dropping_particle,
            DemoteNonDroppingParticle::SortOnly
        );
        assert!(!o.initialize_with_hyphen);
        assert_eq!(o.page_range_format, Some(PageRangeFormat::Chicago15));
        assert_eq!(o.page_range_delimiter.as_deref(), Some("-"));
        assert_eq!(o.near_note_distance, Some(3));
        assert_eq!(
            o.disambiguation,
            DisambiguationStrategy {
                add_names: true,
                add_given_names: Some(GivenNameDisambiguationRule::PrimaryName),
                add_year_suffix: true
            }
        );
        assert_eq!(o.cite_group_delimiter.as_deref(), Some(", "));
        assert_eq!((o.line_spacing, o.entry_spacing), (Some(2), Some(0)));
        assert!(o.hanging_indent);
        assert_eq!(o.second_field_align, Some(SecondFieldAlign::Flush));
        assert_eq!(
            o.subsequent_author_substitute,
            Some(("---".into(), SubsequentAuthorSubstituteRule::PartialFirst))
        );
        assert!(o.uses_year_suffix_variable);
        assert_eq!(o.name_format.and_style, Some(TermForm::Symbol));
        assert_eq!(o.name_format.delimiter.as_deref(), Some(";"));
        assert_eq!(o.name_format.as_sort_order, Some(NameAsSortOrder::All));
        let c = &s.citation;
        assert_eq!(c.options.collapse, Some(Collapsing::Year));
        assert_eq!(c.options.year_suffix_delimiter.as_deref(), Some(";"));
        assert_eq!(c.options.after_collapse_delimiter.as_deref(), Some("|"));
        assert_eq!(c.options.name_format.form, Some(NameForm::Short));
        assert!(c.formatting.affixes_inside);
        assert_eq!(c.formatting.prefix.as_deref(), Some("("));
        assert_eq!(
            c.sort_keys[0],
            SortKey::Variable(SortDirection::Descending, Variable::new("issued"))
        );
        let SortKey::Macro(direction, format, name) = &c.sort_keys[1] else {
            panic!("not a macro key")
        };
        assert_eq!((direction, name.as_str()), (&SortDirection::Ascending, "m"));
        assert_eq!(
            (
                format.et_al_min,
                format.et_al_use_first,
                format.et_al_use_last
            ),
            (Some(3), Some(1), Some(true))
        );
        assert_eq!(format.et_al_subsequent_min, Some(5));
        let b = s.bibliography.as_ref().unwrap();
        assert_eq!(b.options.name_format.et_al_min, Some(7));
        assert_eq!(b.options.year_suffix_delimiter, None);
        assert_eq!(s.macros.len(), 1);
    }

    #[test]
    fn style_option_defaults_and_alternatives() {
        let base = "<citation><layout delimiter=\"; \"><text value=\"x\"/></layout></citation>";
        let s = style(base, "");
        let o = &s.options;
        assert!(o.is_note_style);
        assert_eq!(s.csl_version, (0, 0, 0));
        assert_eq!(o.default_locale, None);
        assert_eq!(
            o.demote_non_dropping_particle,
            DemoteNonDroppingParticle::DisplayAndSort
        );
        assert!(o.initialize_with_hyphen && !o.hanging_indent && !o.uses_year_suffix_variable);
        assert_eq!(o.cite_group_delimiter, None);
        assert_eq!(o.disambiguation, DisambiguationStrategy::default());
        assert_eq!(
            s.citation.options.year_suffix_delimiter.as_deref(),
            Some("; ")
        );
        assert_eq!(
            s.citation.options.after_collapse_delimiter.as_deref(),
            Some("; ")
        );
        assert!(s.bibliography.is_none());
        let attr = |attrs: &str| style(base, attrs);
        assert!(attr(" class=\"note\"").options.is_note_style);
        assert_eq!(attr(" version=\"1.1\"").csl_version, (1, 1, 0));
        assert_eq!(attr(" version=\"2.x.1\"").csl_version, (2, 0, 0));
        assert_eq!(attr(" version=\"x\"").csl_version, (0, 0, 0));
        assert_eq!(
            attr(" demote-non-dropping-particle=\"never\"")
                .options
                .demote_non_dropping_particle,
            DemoteNonDroppingParticle::Never
        );
        assert!(
            attr(" initialize-with-hyphen=\"true\"")
                .options
                .initialize_with_hyphen
        );
        for (value, expected) in [
            ("chicago-15", Some(PageRangeFormat::Chicago15)),
            ("chicago-16", Some(PageRangeFormat::Chicago16)),
            ("expanded", Some(PageRangeFormat::Expanded)),
            ("minimal", Some(PageRangeFormat::Minimal)),
            ("minimal-two", Some(PageRangeFormat::MinimalTwo)),
            ("x", None),
        ] {
            assert_eq!(
                attr(&format!(" page-range-format=\"{value}\""))
                    .options
                    .page_range_format,
                expected
            );
        }
        assert_eq!(
            attr(" name-delimiter=\"a\" names-delimiter=\"b\"")
                .options
                .name_format
                .delimiter
                .as_deref(),
            Some("a")
        );
        let cite = |attrs: &str| {
            style(
                &format!("<citation {attrs}><layout><text value=\"x\"/></layout></citation>"),
                "",
            )
        };
        for (value, expected) in [
            ("all-names", GivenNameDisambiguationRule::AllNames),
            (
                "all-names-with-initials",
                GivenNameDisambiguationRule::AllNamesWithInitials,
            ),
            (
                "primary-name-with-initials",
                GivenNameDisambiguationRule::PrimaryNameWithInitials,
            ),
            ("x", GivenNameDisambiguationRule::ByCite),
        ] {
            let s = cite(&format!(
                "disambiguate-add-givenname=\"true\" givenname-disambiguation-rule=\"{value}\""
            ));
            assert_eq!(s.options.disambiguation.add_given_names, Some(expected));
        }
        assert_eq!(
            cite("disambiguate-add-givenname=\"false\"")
                .options
                .disambiguation
                .add_given_names,
            None
        );
        assert_eq!(
            cite("cite-group-delimiter=\"; \" collapse=\"year\"")
                .options
                .cite_group_delimiter
                .as_deref(),
            Some("; ")
        );
        assert_eq!(
            cite("cite-group-delimiter=\"; \"")
                .citation
                .options
                .year_suffix_delimiter
                .as_deref(),
            Some("; ")
        );
        for (value, expected) in [
            ("citation-number", Some(Collapsing::CitationNumber)),
            ("year-suffix", Some(Collapsing::YearSuffix)),
            ("year-suffix-ranged", Some(Collapsing::YearSuffixRanged)),
            ("x", None),
        ] {
            assert_eq!(
                cite(&format!("collapse=\"{value}\""))
                    .citation
                    .options
                    .collapse,
                expected
            );
        }
        let bib = |attrs: &str| {
            style(
                &format!(
                    "{base}<bibliography {attrs}><layout><text value=\"x\"/></layout></bibliography>"
                ),
                "",
            )
        };
        assert_eq!(
            bib("second-field-align=\"margin\"")
                .options
                .second_field_align,
            Some(SecondFieldAlign::Margin)
        );
        assert_eq!(
            bib("second-field-align=\"x\"").options.second_field_align,
            None
        );
        for (value, expected) in [
            (
                "complete-each",
                SubsequentAuthorSubstituteRule::CompleteEach,
            ),
            ("partial-each", SubsequentAuthorSubstituteRule::PartialEach),
            ("x", SubsequentAuthorSubstituteRule::CompleteAll),
        ] {
            let s = bib(&format!(
                "subsequent-author-substitute=\"-\" subsequent-author-substitute-rule=\"{value}\""
            ));
            assert_eq!(
                s.options.subsequent_author_substitute.map(|x| x.1),
                Some(expected)
            );
        }
    }

    #[test]
    fn the_year_suffix_search_follows_macros_groups_and_choose() {
        let uses = |body: &str| style(body, "").options.uses_year_suffix_variable;
        let cite = |inner: &str| format!("<citation><layout>{inner}</layout></citation>");
        assert!(uses(&format!(
            "<macro name=\"m\"><group><text variable=\"YEAR-SUFFIX\"/></group></macro>{}",
            cite("<text macro=\"m\"/>")
        )));
        assert!(uses(&cite(
            "<choose><if><text value=\"x\"/></if><else><text variable=\"year-suffix\"/></else></choose>"
        )));
        assert!(!uses(&cite(
            "<text macro=\"missing\"/><names variable=\"author\"><substitute><text variable=\"year-suffix\"/></substitute></names>"
        )));
        assert!(uses(&format!(
            "{}<bibliography><layout><text variable=\"year-suffix\"/></layout></bibliography>",
            cite("<text value=\"x\"/>")
        )));
        assert!(uses(&format!(
            "<macro name=\"a\"><text macro=\"a\"/><text macro=\"b\"/></macro><macro name=\"b\"><text variable=\"year-suffix\"/></macro>{}",
            cite("<text macro=\"a\"/>")
        )));
        assert!(!uses(&format!(
            "<macro name=\"m\"><text value=\"1\"/></macro>{}",
            cite("<text macro=\"m\"/><text macro=\"m\"/>")
        )));
    }

    #[test]
    fn the_last_macro_of_a_name_wins() {
        let s = style(
            "<macro name=\"m\"><text value=\"1\"/></macro><macro name=\"m\"><text value=\"2\"/><text value=\"3\"/></macro>\
             <citation><layout><text macro=\"m\"/></layout></citation>",
            "",
        );
        assert_eq!(s.macros.get("m").map(Vec::len), Some(2));
    }

    #[test]
    fn a_dependent_style_uses_its_parent_with_its_own_default_locale() {
        let child = format!(
            "<style {NS} default-locale=\"fr\"><info><link rel=\"self\" href=\"x\"/><link rel=\"independent-parent\" href=\"p.csl\"/></info></style>"
        );
        let parent = format!(
            "<style {NS} default-locale=\"de\" class=\"in-text\"><citation><layout><text value=\"p\"/></layout></citation></style>"
        );
        let mut asked = Vec::new();
        let mut fetch = |href: &str| -> Result<String, ()> {
            asked.push(href.to_owned());
            Ok(parent.clone())
        };
        let s = parse_style(&child, 128, &mut fetch).unwrap().unwrap();
        assert_eq!(asked, ["p.csl"]);
        assert!(!s.options.is_note_style);
        assert_eq!(
            s.options.default_locale.map(|l| l.language).as_deref(),
            Some("fr")
        );
        let mut broken = |_: &str| -> Result<String, ()> { Ok("<a>".into()) };
        assert!(matches!(
            parse_style(&child, 128, &mut broken),
            Ok(Err(CiteprocError::Xml(_)))
        ));
        let mut failing = |_: &str| -> Result<String, &'static str> { Err("no access") };
        assert_eq!(
            parse_style(&child, 128, &mut failing).err(),
            Some("no access")
        );
    }
}
