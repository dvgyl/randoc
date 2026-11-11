//! Formatting attributes, the locale element, terms and dates: a port of
//! `Citeproc.Element` (citeproc 0.14.1.1, BSD-2-Clause, see `NOTICE.md`).

use std::collections::BTreeMap;

use crate::error::CiteprocError;
use crate::lang::parse_lang;
use crate::types::{
    DatePart, DatePartForm, DatePartName, DateType, DisplayStyle, Element, ElementType, FontStyle,
    FontVariant, FontWeight, Formatting, Locale, ShowDateParts, Term, TermForm, TermGender,
    TermMatch, TermNumber, TextCase, TextDecoration, Variable, VerticalAlign,
};
use crate::xml::XmlElement;

/// The formatting attributes of an element (`getFormatting`).
pub(crate) fn formatting(node: &XmlElement) -> Formatting {
    let attr = |name| node.attr(name);
    Formatting {
        lang: None,
        font_style: match attr("font-style") {
            Some("italic") => Some(FontStyle::Italic),
            Some("oblique") => Some(FontStyle::Oblique),
            Some("normal") => Some(FontStyle::Normal),
            _ => None,
        },
        font_variant: match attr("font-variant") {
            Some("small-caps") => Some(FontVariant::SmallCaps),
            Some("normal") => Some(FontVariant::Normal),
            _ => None,
        },
        font_weight: match attr("font-weight") {
            Some("bold") => Some(FontWeight::Bold),
            Some("light") => Some(FontWeight::Light),
            Some("normal") => Some(FontWeight::Normal),
            _ => None,
        },
        text_decoration: match attr("text-decoration") {
            Some("underline") => Some(TextDecoration::Underline),
            Some("none") => Some(TextDecoration::None),
            _ => None,
        },
        vertical_align: match attr("vertical-align") {
            Some("sup") => Some(VerticalAlign::Sup),
            Some("sub") => Some(VerticalAlign::Sub),
            Some("baseline") => Some(VerticalAlign::Baseline),
            _ => None,
        },
        prefix: attr("prefix").map(str::to_owned),
        suffix: attr("suffix").map(str::to_owned),
        display: match attr("display") {
            Some("block") => Some(DisplayStyle::Block),
            Some("left-margin") => Some(DisplayStyle::LeftMargin),
            Some("right-inline") => Some(DisplayStyle::RightInline),
            Some("indent") => Some(DisplayStyle::Indent),
            _ => None,
        },
        text_case: match attr("text-case") {
            Some("lowercase") => Some(TextCase::Lowercase),
            Some("uppercase") => Some(TextCase::Uppercase),
            Some("capitalize-first") => Some(TextCase::CapitalizeFirst),
            Some("capitalize-all") => Some(TextCase::CapitalizeAll),
            Some("sentence") => Some(TextCase::SentenceCase),
            Some("title") => Some(TextCase::TitleCase),
            _ => None,
        },
        delimiter: attr("delimiter").map(str::to_owned),
        strip_periods: attr("strip-periods") == Some("true"),
        quotes: attr("quotes") == Some("true"),
        affixes_inside: false,
    }
}

/// The `form` of a term, a label or `and` (`Long` by default).
pub(crate) fn term_form(value: Option<&str>) -> TermForm {
    match value {
        Some("short") => TermForm::Short,
        Some("verb") => TermForm::Verb,
        Some("verb-short") => TermForm::VerbShort,
        Some("symbol") => TermForm::Symbol,
        _ => TermForm::Long,
    }
}

/// A `locale` element (`pLocale`).
pub(crate) fn locale(node: &XmlElement) -> Result<Locale, CiteprocError> {
    let language = match node.attr("lang") {
        None => None,
        Some(tag) => Some(parse_lang(tag).map_err(CiteprocError::Parse)?),
    };
    // `mconcat` of attribute maps keeps the first value of each key.
    let mut style_options: BTreeMap<&str, &str> = BTreeMap::new();
    for options in node.children_named("style-options") {
        for (key, value) in &options.attributes {
            style_options.entry(key.as_str()).or_insert(value.as_str());
        }
    }
    let mut dates = BTreeMap::new();
    for date_node in node.children_named("date") {
        let element = date(date_node);
        if let ElementType::Date { date_type, .. } = element.kind {
            dates.entry(date_type).or_insert(element);
        }
    }
    let mut terms: BTreeMap<String, Vec<(Term, String)>> = BTreeMap::new();
    for term_node in node
        .children_named("terms")
        .flat_map(|t| t.children_named("term"))
    {
        parse_term(&mut terms, term_node)?;
    }
    // The upstream code prepends each term, so the last term of the file comes first.
    for list in terms.values_mut() {
        list.reverse();
    }
    Ok(Locale {
        language,
        punctuation_in_quote: style_options
            .get("punctuation-in-quote")
            .map(|v| *v == "true"),
        limit_day_ordinals_to_day_1: style_options
            .get("limit-day-ordinals-to-day-1")
            .map(|v| *v == "true"),
        dates,
        terms,
    })
}

fn gender(value: Option<&str>) -> Option<TermGender> {
    match value {
        Some("masculine") => Some(TermGender::Masculine),
        Some("feminine") => Some(TermGender::Feminine),
        _ => None,
    }
}

fn parse_term(
    terms: &mut BTreeMap<String, Vec<(Term, String)>>,
    node: &XmlElement,
) -> Result<(), CiteprocError> {
    let Some(name) = node.attr("name") else {
        return Err(CiteprocError::Parse(
            "Text node has no name attribute".to_owned(),
        ));
    };
    // Whitespace alone defines an empty term; other whitespace is kept.
    let unless_blank = |text: String| {
        if text.trim().is_empty() {
            String::new()
        } else {
            text
        }
    };
    let joined =
        |child: &str| unless_blank(node.children_named(child).map(XmlElement::text).collect());
    let single = joined("single");
    let multiple = joined("multiple");
    let text = unless_blank(node.text());
    let term = Term {
        name: name.to_owned(),
        form: term_form(node.attr("form")),
        number: None,
        gender: gender(node.attr("gender")),
        gender_form: gender(node.attr("gender-form")),
        matching: match node.attr("match") {
            Some("last-digit") => Some(TermMatch::LastDigit),
            Some("last-two-digits") => Some(TermMatch::LastTwoDigits),
            Some("whole-number") => Some(TermMatch::WholeNumber),
            _ => None,
        },
    };
    let list = terms.entry(term.name.clone()).or_default();
    if single.is_empty() {
        list.push((term, text));
    } else {
        let plural = Term {
            number: Some(TermNumber::Plural),
            ..term.clone()
        };
        let singular = Term {
            number: Some(TermNumber::Singular),
            ..term
        };
        list.push((plural, multiple));
        list.push((singular, single));
    }
    Ok(())
}

/// A `date` element (`pDate`).
pub(crate) fn date(node: &XmlElement) -> Element {
    let show_date_parts = match node.attr("date-parts") {
        Some("year-month-day") => Some(ShowDateParts::YearMonthDay),
        Some("year-month") => Some(ShowDateParts::YearMonth),
        Some("year") => Some(ShowDateParts::Year),
        _ => None,
    };
    let date_type = match node.attr("form") {
        Some("numeric") => DateType::LocalizedNumeric,
        Some("text") => DateType::LocalizedText,
        _ => DateType::NonLocalized,
    };
    let parts = node.children_named("date-part").map(date_part).collect();
    Element {
        kind: ElementType::Date {
            variable: Variable::new(node.attr("variable").unwrap_or("")),
            date_type,
            show_date_parts,
            parts,
        },
        formatting: formatting(node),
    }
}

fn date_part(node: &XmlElement) -> DatePart {
    let name = match node.attr("name") {
        Some("day") => DatePartName::Day,
        Some("month") => DatePartName::Month,
        _ => DatePartName::Year,
    };
    let form = match node.attr("form") {
        Some("numeric") => DatePartForm::Numeric,
        Some("numeric-leading-zeros") => DatePartForm::NumericLeadingZeros,
        Some("ordinal") => DatePartForm::Ordinal,
        Some("long") => DatePartForm::Long,
        Some("short") => DatePartForm::Short,
        _ if name == DatePartName::Day => DatePartForm::Numeric,
        _ => DatePartForm::Long,
    };
    DatePart {
        name,
        form,
        range_delimiter: node
            .attr("range-delimiter")
            .unwrap_or("\u{2013}")
            .to_owned(),
        formatting: formatting(node),
    }
}

#[cfg(test)]
mod tests {
    use crate::locale::parse_locale;
    use crate::types::{DateType, ElementType, Term, TermForm, TermGender, TermMatch, TermNumber};

    #[test]
    fn a_locale_reads_terms_dates_and_style_options() {
        let text = "<locale xml:lang=\"xx-YY\"><style-options punctuation-in-quote=\"true\"/>\
            <style-options punctuation-in-quote=\"false\" limit-day-ordinals-to-day-1=\"yes\"/>\
            <date form=\"text\" prefix=\"1\"/><date form=\"text\" prefix=\"2\"/><date form=\"numeric\"/>\
            <terms><term name=\"and\">and</term><term name=\"and\" form=\"symbol\">&amp;</term>\
            <term name=\"page\" form=\"short\"><single>p.</single><multiple>pp.</multiple></term>\
            <term name=\"blank\">  </term><term name=\"ad\"> AD</term>\
            <term name=\"ordinal-01\" gender-form=\"feminine\" match=\"last-two-digits\">e</term>\
            <term name=\"month-01\" gender=\"masculine\">m</term>\
            <term name=\"o\" match=\"last-digit\" form=\"verb\">a</term><term name=\"o\" match=\"whole-number\" form=\"verb-short\">b</term>\
            <term name=\"o\" match=\"x\" gender=\"x\">c</term></terms><terms><term name=\"and\">und</term></terms></locale>";
        let locale = parse_locale(text, 128).unwrap();
        assert_eq!(
            locale.language.map(|l| (l.language, l.region)),
            Some(("xx".into(), Some("YY".into())))
        );
        assert_eq!(locale.punctuation_in_quote, Some(true));
        assert_eq!(locale.limit_day_ordinals_to_day_1, Some(false));
        assert_eq!(locale.dates.len(), 2);
        assert_eq!(
            locale.dates[&DateType::LocalizedText]
                .formatting
                .prefix
                .as_deref(),
            Some("1")
        );
        assert!(matches!(
            locale.dates[&DateType::LocalizedNumeric].kind,
            ElementType::Date { .. }
        ));
        let and: Vec<(TermForm, &str)> = locale.terms["and"]
            .iter()
            .map(|(t, s)| (t.form, s.as_str()))
            .collect();
        assert_eq!(
            and,
            [
                (TermForm::Long, "und"),
                (TermForm::Symbol, "&"),
                (TermForm::Long, "and")
            ]
        );
        let page: Vec<(Option<TermNumber>, &str)> = locale.terms["page"]
            .iter()
            .map(|(t, s)| (t.number, s.as_str()))
            .collect();
        assert_eq!(
            page,
            [
                (Some(TermNumber::Singular), "p."),
                (Some(TermNumber::Plural), "pp.")
            ]
        );
        assert_eq!(locale.terms["page"][0].0.form, TermForm::Short);
        assert_eq!(locale.terms["blank"][0].1, "");
        assert_eq!(locale.terms["ad"][0].1, " AD");
        let ordinal = &locale.terms["ordinal-01"][0].0;
        assert_eq!(
            (ordinal.gender_form, ordinal.matching),
            (Some(TermGender::Feminine), Some(TermMatch::LastTwoDigits))
        );
        assert_eq!(
            locale.terms["month-01"][0].0.gender,
            Some(TermGender::Masculine)
        );
        let o: Vec<&Term> = locale.terms["o"].iter().map(|(t, _)| t).collect();
        assert_eq!(
            (o[0].matching, o[0].gender, o[0].form),
            (None, None, TermForm::Long)
        );
        assert_eq!(
            (o[1].matching, o[1].form),
            (Some(TermMatch::WholeNumber), TermForm::VerbShort)
        );
        assert_eq!(
            (o[2].matching, o[2].form),
            (Some(TermMatch::LastDigit), TermForm::Verb)
        );
    }

    #[test]
    fn a_locale_without_language_or_options_has_none() {
        let locale = parse_locale("<locale/>", 128).unwrap();
        assert_eq!(locale, crate::types::Locale::default());
        let single_blank = parse_locale("<locale><terms><term name=\"t\"><single> </single><multiple>x</multiple>y</term></terms></locale>", 128).unwrap();
        assert_eq!(
            single_blank.terms["t"],
            [(Term::named("t"), "y".to_owned())]
        );
    }

    #[test]
    fn locale_errors_have_the_upstream_texts() {
        assert_eq!(
            parse_locale("<locale><terms><term>x</term></terms></locale>", 128)
                .unwrap_err()
                .to_string(),
            "CiteprocParseError: Text node has no name attribute"
        );
        assert_eq!(
            parse_locale("<locale xml:lang=\"!\"/>", 128)
                .unwrap_err()
                .to_string(),
            "CiteprocParseError: \"lang\" (line 1, column 1):\nunexpected !\nexpecting language"
        );
        assert_eq!(
            parse_locale("", 128).unwrap_err().to_string(),
            "CiteprocXMLError: Missing root element"
        );
    }
}
