//! The CSL style tree and the locale: a port of the style and locale types
//! of `Citeproc.Types` (citeproc 0.14.1.1, BSD-2-Clause, see `NOTICE.md`).
//! The field names and the defaults follow the upstream types.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::lang::Lang;

/// A CSL variable name. Names compare without regard to case, as the
/// upstream `CI Text` does.
#[derive(Debug, Clone, Default)]
pub struct Variable {
    original: String,
    folded: String,
}

impl Variable {
    /// Makes a variable from its name.
    pub fn new(name: &str) -> Variable {
        Variable {
            original: name.to_owned(),
            folded: fold_case(name),
        }
    }

    /// The name as the style writes it.
    pub fn as_str(&self) -> &str {
        &self.original
    }
}

/// Case folding for the comparison of variable names. Lower case plus the
/// folds of full case folding that differ from lower case for common letters.
fn fold_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            'ß' | 'ẞ' => out.push_str("ss"),
            'ς' => out.push('σ'),
            _ => out.extend(c.to_lowercase()),
        }
    }
    out
}

impl PartialEq for Variable {
    fn eq(&self, other: &Self) -> bool {
        self.folded == other.folded
    }
}

impl Eq for Variable {}

impl PartialOrd for Variable {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Variable {
    fn cmp(&self, other: &Self) -> Ordering {
        self.folded.cmp(&other.folded)
    }
}

/// `font-style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

/// `font-variant`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum FontVariant {
    Normal,
    SmallCaps,
}

/// `font-weight`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum FontWeight {
    Normal,
    Bold,
    Light,
}

/// `text-decoration`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum TextDecoration {
    None,
    Underline,
}

/// `vertical-align`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum VerticalAlign {
    Baseline,
    Sup,
    Sub,
}

/// `display`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum DisplayStyle {
    Block,
    LeftMargin,
    RightInline,
    Indent,
}

/// `text-case`. `PreserveCase` has no attribute value; evaluation uses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum TextCase {
    Lowercase,
    Uppercase,
    CapitalizeFirst,
    CapitalizeAll,
    SentenceCase,
    TitleCase,
    PreserveCase,
}

/// The formatting attributes of an element.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[allow(missing_docs)]
pub struct Formatting {
    pub lang: Option<Lang>,
    pub font_style: Option<FontStyle>,
    pub font_variant: Option<FontVariant>,
    pub font_weight: Option<FontWeight>,
    pub text_decoration: Option<TextDecoration>,
    pub vertical_align: Option<VerticalAlign>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub display: Option<DisplayStyle>,
    pub text_case: Option<TextCase>,
    pub delimiter: Option<String>,
    pub strip_periods: bool,
    pub quotes: bool,
    /// `true` puts the affixes inside the other formatting. Only a layout sets it.
    pub affixes_inside: bool,
}

impl Formatting {
    /// Combines two formattings: a field of `self` wins over the field of `other`.
    pub fn combine(&self, other: &Formatting) -> Formatting {
        Formatting {
            lang: self.lang.clone().or_else(|| other.lang.clone()),
            font_style: self.font_style.or(other.font_style),
            font_variant: self.font_variant.or(other.font_variant),
            font_weight: self.font_weight.or(other.font_weight),
            text_decoration: self.text_decoration.or(other.text_decoration),
            vertical_align: self.vertical_align.or(other.vertical_align),
            prefix: self.prefix.clone().or_else(|| other.prefix.clone()),
            suffix: self.suffix.clone().or_else(|| other.suffix.clone()),
            display: self.display.or(other.display),
            text_case: self.text_case.or(other.text_case),
            delimiter: self.delimiter.clone().or_else(|| other.delimiter.clone()),
            strip_periods: self.strip_periods || other.strip_periods,
            quotes: self.quotes || other.quotes,
            affixes_inside: self.affixes_inside || other.affixes_inside,
        }
    }
}

/// `match` of a condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Match {
    All,
    Any,
    None,
}

/// The cite position of a `position` condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs)]
pub enum Position {
    First,
    IbidWithLocator,
    Ibid,
    NearNote,
    Subsequent,
}

/// One test of `if` or `else-if`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// `variable`.
    HasVariable(Variable),
    /// `type`: the type names of one attribute.
    HasType(Vec<String>),
    /// `is-uncertain-date`.
    IsUncertainDate(Variable),
    /// `is-numeric`.
    IsNumeric(Variable),
    /// `locator`.
    HasLocatorType(Variable),
    /// `position`.
    HasPosition(Position),
    /// `disambiguate="true"`.
    WouldDisambiguate,
}

/// `form` of a date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs)]
pub enum DateType {
    LocalizedNumeric,
    LocalizedText,
    NonLocalized,
}

/// `date-parts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum ShowDateParts {
    YearMonthDay,
    YearMonth,
    Year,
}

/// `name` of a date part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs)]
pub enum DatePartName {
    Year,
    Month,
    Day,
}

/// `form` of a date part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum DatePartForm {
    Numeric,
    NumericLeadingZeros,
    Ordinal,
    Long,
    Short,
}

/// A `date-part` element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatePart {
    /// `name`.
    pub name: DatePartName,
    /// `form`.
    pub form: DatePartForm,
    /// `range-delimiter`, an en dash by default.
    pub range_delimiter: String,
    /// The formatting attributes.
    pub formatting: Formatting,
}

/// `form` of a variable in `text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum VariableForm {
    Short,
    Long,
}

/// What a `text` element renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextType {
    /// `variable`, with `form`.
    Variable(VariableForm, Variable),
    /// `macro`.
    Macro(String),
    /// `term`.
    Term(Term),
    /// `value`.
    Value(String),
}

/// `form` of a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum NumberForm {
    Numeric,
    Ordinal,
    LongOrdinal,
    Roman,
}

/// `plural` of a label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Pluralize {
    Contextual,
    Always,
    Never,
}

/// The children of a `names` element other than `substitute`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NamesFormat {
    /// `label`: its form, its plural rule and its formatting.
    pub label: Option<(TermForm, Pluralize, Formatting)>,
    /// `et-al`: its term and its formatting.
    pub et_al: Option<(String, Formatting)>,
    /// `name`: its options and its formatting.
    pub name: Option<(NameFormat, Formatting)>,
    /// `true` when the label comes before the `name` element.
    pub label_before_name: bool,
}

/// `delimiter-precedes-et-al` and `delimiter-precedes-last`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum DelimiterPrecedes {
    Contextual,
    AfterInvertedName,
    Always,
    Never,
}

/// `form` of a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum NameForm {
    Long,
    Short,
    Count,
}

/// `name-as-sort-order`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum NameAsSortOrder {
    First,
    All,
}

/// The options of a `name` element, and the inheritable name options of
/// `style`, `citation`, `bibliography` and a sort `key`.
///
/// `None` means that the element does not set the option. [`NameFormat::combine`]
/// takes the value of the nearest level that sets it. Where two attribute
/// names are given, the first is the attribute of `name` and the second is the
/// inheritable attribute of the other elements.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NameFormat {
    /// The formatting of the first `name-part name="given"` (only on `name`).
    pub given_formatting: Option<Formatting>,
    /// The formatting of the first `name-part name="family"` (only on `name`).
    pub family_formatting: Option<Formatting>,
    /// `and`: `text` gives [`TermForm::Long`], `symbol` gives [`TermForm::Symbol`].
    pub and_style: Option<TermForm>,
    /// The delimiter between names: `delimiter`, or `name-delimiter` and
    /// then `names-delimiter`.
    pub delimiter: Option<String>,
    /// `delimiter-precedes-et-al`.
    pub delimiter_precedes_et_al: Option<DelimiterPrecedes>,
    /// `delimiter-precedes-last`.
    pub delimiter_precedes_last: Option<DelimiterPrecedes>,
    /// `et-al-min`, an integer.
    pub et_al_min: Option<i64>,
    /// `et-al-use-first`, an integer.
    pub et_al_use_first: Option<i64>,
    /// `et-al-subsequent-use-first`, an integer.
    pub et_al_subsequent_use_first: Option<i64>,
    /// `et-al-subsequent-min`, an integer.
    pub et_al_subsequent_min: Option<i64>,
    /// `et-al-use-last`: `true` or `false`.
    pub et_al_use_last: Option<bool>,
    /// The name form: `form`, or `name-form`.
    pub form: Option<NameForm>,
    /// `initialize`: `true` or `false`.
    pub initialize: Option<bool>,
    /// `initialize-with`.
    pub initialize_with: Option<String>,
    /// `name-as-sort-order`: `first` or `all`.
    pub as_sort_order: Option<NameAsSortOrder>,
    /// `sort-separator`.
    pub sort_separator: Option<String>,
}

impl NameFormat {
    /// Combines two name formats: a field of `self` wins over the field of `other`.
    pub fn combine(&self, other: &NameFormat) -> NameFormat {
        fn pick<T: Clone>(a: &Option<T>, b: &Option<T>) -> Option<T> {
            a.clone().or_else(|| b.clone())
        }
        NameFormat {
            given_formatting: pick(&self.given_formatting, &other.given_formatting),
            family_formatting: pick(&self.family_formatting, &other.family_formatting),
            and_style: pick(&self.and_style, &other.and_style),
            delimiter: pick(&self.delimiter, &other.delimiter),
            delimiter_precedes_et_al: pick(
                &self.delimiter_precedes_et_al,
                &other.delimiter_precedes_et_al,
            ),
            delimiter_precedes_last: pick(
                &self.delimiter_precedes_last,
                &other.delimiter_precedes_last,
            ),
            et_al_min: pick(&self.et_al_min, &other.et_al_min),
            et_al_use_first: pick(&self.et_al_use_first, &other.et_al_use_first),
            et_al_subsequent_use_first: pick(
                &self.et_al_subsequent_use_first,
                &other.et_al_subsequent_use_first,
            ),
            et_al_subsequent_min: pick(&self.et_al_subsequent_min, &other.et_al_subsequent_min),
            et_al_use_last: pick(&self.et_al_use_last, &other.et_al_use_last),
            form: pick(&self.form, &other.form),
            initialize: pick(&self.initialize, &other.initialize),
            initialize_with: pick(&self.initialize_with, &other.initialize_with),
            as_sort_order: pick(&self.as_sort_order, &other.as_sort_order),
            sort_separator: pick(&self.sort_separator, &other.sort_separator),
        }
    }
}

/// One branch of `choose`: `if`, `else-if` or `else`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChooseBranch {
    /// `match`.
    pub matching: Match,
    /// The conditions. `else` has none.
    pub conditions: Vec<Condition>,
    /// The rendering elements of the branch.
    pub elements: Vec<Element>,
}

/// The kind of a rendering element and its own attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementType {
    /// `text`.
    Text(TextType),
    /// `date`.
    Date {
        /// `variable`.
        variable: Variable,
        /// `form`.
        date_type: DateType,
        /// `date-parts`.
        show_date_parts: Option<ShowDateParts>,
        /// The `date-part` children.
        parts: Vec<DatePart>,
    },
    /// `number`.
    Number(Variable, NumberForm),
    /// `names`: the variables, the name options and the `substitute` elements.
    /// The options are boxed to keep the element small on the stack.
    Names(Vec<Variable>, Box<NamesFormat>, Vec<Element>),
    /// `label`.
    Label(Variable, TermForm, Pluralize),
    /// `group`. The flag is `true` for an expanded macro.
    Group(bool, Vec<Element>),
    /// `choose`.
    Choose(Vec<ChooseBranch>),
}

/// A rendering element with its formatting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// The kind of the element.
    pub kind: ElementType,
    /// The formatting attributes.
    pub formatting: Formatting,
}

/// `sort` of a sort key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// A `key` of `sort`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortKey {
    /// A key with `variable`.
    Variable(SortDirection, Variable),
    /// A key with `macro`, and its name options (`names-min` and others).
    Macro(SortDirection, Box<NameFormat>, String),
}

/// `collapse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Collapsing {
    CitationNumber,
    Year,
    YearSuffix,
    YearSuffixRanged,
}

/// The options of `citation` or `bibliography` that the layout uses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[allow(missing_docs)]
pub struct LayoutOptions {
    pub collapse: Option<Collapsing>,
    pub year_suffix_delimiter: Option<String>,
    pub after_collapse_delimiter: Option<String>,
    pub name_format: NameFormat,
}

/// The content of `citation` or `bibliography`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The options.
    pub options: LayoutOptions,
    /// The formatting of the `layout` element, with `affixes_inside`.
    pub formatting: Formatting,
    /// The elements of `layout`.
    pub elements: Vec<Element>,
    /// The keys of `sort`.
    pub sort_keys: Vec<SortKey>,
}

/// `givenname-disambiguation-rule`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs)]
pub enum GivenNameDisambiguationRule {
    AllNames,
    AllNamesWithInitials,
    PrimaryName,
    PrimaryNameWithInitials,
    ByCite,
}

/// The disambiguation attributes of `citation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(missing_docs)]
pub struct DisambiguationStrategy {
    pub add_names: bool,
    pub add_given_names: Option<GivenNameDisambiguationRule>,
    pub add_year_suffix: bool,
}

/// `demote-non-dropping-particle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum DemoteNonDroppingParticle {
    DisplayAndSort,
    SortOnly,
    Never,
}

/// `subsequent-author-substitute-rule`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum SubsequentAuthorSubstituteRule {
    CompleteAll,
    CompleteEach,
    PartialEach,
    PartialFirst,
}

/// `second-field-align`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum SecondFieldAlign {
    Flush,
    Margin,
}

/// `page-range-format`. `chicago` is `Chicago15`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs)]
pub enum PageRangeFormat {
    Chicago15,
    Chicago16,
    Expanded,
    Minimal,
    MinimalTwo,
}

/// The global options of a style.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct StyleOptions {
    /// `true` for `class="note"` and for a style without `class`.
    pub is_note_style: bool,
    pub default_locale: Option<Lang>,
    pub demote_non_dropping_particle: DemoteNonDroppingParticle,
    pub initialize_with_hyphen: bool,
    pub page_range_format: Option<PageRangeFormat>,
    pub page_range_delimiter: Option<String>,
    pub disambiguation: DisambiguationStrategy,
    pub near_note_distance: Option<i64>,
    pub cite_group_delimiter: Option<String>,
    pub line_spacing: Option<i64>,
    pub entry_spacing: Option<i64>,
    pub hanging_indent: bool,
    pub second_field_align: Option<SecondFieldAlign>,
    pub subsequent_author_substitute: Option<(String, SubsequentAuthorSubstituteRule)>,
    /// `true` when a layout uses the variable `year-suffix`, also through a macro.
    pub uses_year_suffix_variable: bool,
    pub name_format: NameFormat,
}

/// A parsed CSL style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    /// `version`, as three numbers.
    pub csl_version: (i64, i64, i64),
    /// The global options.
    pub options: StyleOptions,
    /// `citation`.
    pub citation: Layout,
    /// `bibliography`.
    pub bibliography: Option<Layout>,
    /// The `locale` elements of the style, in style order.
    pub locales: Vec<Locale>,
    /// The macros by name. Of two macros with one name, the last one wins, as upstream.
    pub macros: BTreeMap<String, Vec<Element>>,
}

/// `form` of a term.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs)]
pub enum TermForm {
    Long,
    Short,
    Verb,
    VerbShort,
    Symbol,
}

/// Singular or plural.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs)]
pub enum TermNumber {
    Singular,
    Plural,
}

/// `gender` and `gender-form`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs)]
pub enum TermGender {
    Masculine,
    Feminine,
}

/// `match` of an ordinal term.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs)]
pub enum TermMatch {
    LastDigit,
    LastTwoDigits,
    WholeNumber,
}

/// The key of a term.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct Term {
    pub name: String,
    pub form: TermForm,
    pub number: Option<TermNumber>,
    pub gender: Option<TermGender>,
    pub gender_form: Option<TermGender>,
    pub matching: Option<TermMatch>,
}

impl Term {
    /// A term with a name, the long form, and no other attribute.
    pub fn named(name: &str) -> Term {
        Term {
            name: name.to_owned(),
            form: TermForm::Long,
            number: None,
            gender: None,
            gender_form: None,
            matching: None,
        }
    }

    /// `true` when `self` matches `other`: the name and the form are equal,
    /// and `None` on either side matches each value of the other fields.
    pub fn matches(&self, other: &Term) -> bool {
        fn wild<T: PartialEq>(a: &Option<T>, b: &Option<T>) -> bool {
            a.is_none() || b.is_none() || a == b
        }
        self.name == other.name
            && self.form == other.form
            && wild(&self.number, &other.number)
            && wild(&self.gender, &other.gender)
            && wild(&self.gender_form, &other.gender_form)
            && wild(&self.matching, &other.matching)
    }
}

/// A locale: terms, date formats and the style options of a language.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Locale {
    /// `xml:lang`.
    pub language: Option<Lang>,
    /// `punctuation-in-quote` of `style-options`.
    pub punctuation_in_quote: Option<bool>,
    /// `limit-day-ordinals-to-day-1` of `style-options`.
    pub limit_day_ordinals_to_day_1: Option<bool>,
    /// The `date` elements by form. The first date of a form wins.
    pub dates: BTreeMap<DateType, Element>,
    /// The terms by name. The term that comes last in the file is first in its list.
    pub terms: BTreeMap<String, Vec<(Term, String)>>,
}

impl Locale {
    /// Merges two locales: a value of `self` wins, and the term lists are
    /// joined with the terms of `self` first.
    pub fn merge(mut self, other: &Locale) -> Locale {
        if self.language.is_none() {
            self.language.clone_from(&other.language);
        }
        self.punctuation_in_quote = self.punctuation_in_quote.or(other.punctuation_in_quote);
        self.limit_day_ordinals_to_day_1 = self
            .limit_day_ordinals_to_day_1
            .or(other.limit_day_ordinals_to_day_1);
        for (date_type, element) in &other.dates {
            self.dates
                .entry(*date_type)
                .or_insert_with(|| element.clone());
        }
        for (name, terms) in &other.terms {
            self.terms
                .entry(name.clone())
                .or_default()
                .extend(terms.iter().cloned());
        }
        self
    }
}

/// Reads an integer as `readAsInt` of the upstream code: an optional `-`,
/// then ASCII digits only. The value wraps at 64 bits as a Haskell `Int`.
pub fn read_as_int(text: &str) -> Option<i64> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value = digits.bytes().fold(0i64, |acc, b| {
        acc.wrapping_mul(10).wrapping_add(i64::from(b - b'0'))
    });
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_compare_without_case() {
        assert_eq!(Variable::new("Title"), Variable::new("title"));
        assert_ne!(Variable::new("title"), Variable::new("titles"));
        assert_eq!(Variable::new("Title").as_str(), "Title");
        assert_eq!(Variable::new("STRASSE"), Variable::new("straße"));
        assert!(Variable::new("a") < Variable::new("B"));
    }

    #[test]
    fn read_as_int_follows_the_upstream_rules() {
        assert_eq!(read_as_int("12"), Some(12));
        assert_eq!(read_as_int("-3"), Some(-3));
        assert_eq!(read_as_int("+3"), None);
        assert_eq!(read_as_int(""), None);
        assert_eq!(read_as_int("-"), None);
        assert_eq!(read_as_int("1.5"), None);
        assert_eq!(read_as_int(" 1"), None);
        assert_eq!(read_as_int("٣"), None);
        assert_eq!(read_as_int("18446744073709551617"), Some(1));
    }

    #[test]
    fn formatting_combine_prefers_the_first_value() {
        let a = Formatting {
            prefix: Some("a".into()),
            quotes: false,
            ..Formatting::default()
        };
        let b = Formatting {
            prefix: Some("b".into()),
            suffix: Some("s".into()),
            quotes: true,
            ..Formatting::default()
        };
        let c = a.combine(&b);
        assert_eq!(c.prefix.as_deref(), Some("a"));
        assert_eq!(c.suffix.as_deref(), Some("s"));
        assert!(c.quotes);
    }

    #[test]
    fn term_matching_treats_none_as_a_wildcard() {
        let base = Term::named("page");
        let plural = Term {
            number: Some(TermNumber::Plural),
            ..base.clone()
        };
        let singular = Term {
            number: Some(TermNumber::Singular),
            ..base.clone()
        };
        assert!(base.matches(&plural));
        assert!(!plural.matches(&singular));
        assert!(!base.matches(&Term {
            form: TermForm::Short,
            ..base.clone()
        }));
        assert!(!base.matches(&Term::named("pages")));
        let fem = Term {
            gender: Some(TermGender::Feminine),
            ..base.clone()
        };
        assert!(!fem.matches(&Term {
            gender: Some(TermGender::Masculine),
            ..base.clone()
        }));
        let m = Term {
            matching: Some(TermMatch::LastDigit),
            ..base.clone()
        };
        assert!(!m.matches(&Term {
            matching: Some(TermMatch::WholeNumber),
            ..base.clone()
        }));
        let gf = Term {
            gender_form: Some(TermGender::Feminine),
            ..base.clone()
        };
        assert!(!gf.matches(&Term {
            gender_form: Some(TermGender::Masculine),
            ..base
        }));
    }
}
