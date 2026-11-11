//! Locale files, the primary dialects, the locale fallback and the merge of
//! the style locales: a port of `Citeproc.Locale` and of `mergeLocales` of
//! `Citeproc.Style` (citeproc 0.14.1.1, BSD-2-Clause, see `NOTICE.md`).

use crate::element::locale;
use crate::error::CiteprocError;
use crate::lang::Lang;
use crate::types::{Locale, Style};
use crate::xml::parse_document;

/// Parses a CSL locale file.
pub fn parse_locale(text: &str, max_depth: usize) -> Result<Locale, CiteprocError> {
    let root = parse_document(text, max_depth).map_err(|e| CiteprocError::Xml(e.0))?;
    locale(&root)
}

/// The primary dialect of each language that has one. `None` means the
/// locale file has no region (for example `ar`).
const PRIMARY_DIALECTS: [(&str, Option<&str>); 45] = [
    ("af", Some("ZA")),
    ("ar", None),
    ("bg", Some("BG")),
    ("ca", Some("AD")),
    ("cs", Some("CZ")),
    ("cy", Some("GB")),
    ("da", Some("DK")),
    ("de", Some("DE")),
    ("el", Some("GR")),
    ("en", Some("US")),
    ("es", Some("ES")),
    ("et", Some("EE")),
    ("eu", None),
    ("fa", Some("IR")),
    ("fi", Some("FI")),
    ("fr", Some("FR")),
    ("he", Some("IL")),
    ("hr", Some("HR")),
    ("hu", Some("HU")),
    ("id", Some("ID")),
    ("is", Some("IS")),
    ("it", Some("IT")),
    ("ja", Some("JP")),
    ("km", Some("KH")),
    ("ko", Some("KR")),
    ("la", None),
    ("lt", Some("LT")),
    ("lv", Some("LV")),
    ("mn", Some("MN")),
    ("nb", Some("NO")),
    ("nl", Some("NL")),
    ("nn", Some("NO")),
    ("pl", Some("PL")),
    ("pt", Some("PT")),
    ("ro", Some("RO")),
    ("ru", Some("RU")),
    ("sk", Some("SK")),
    ("sl", Some("SI")),
    ("sr", Some("RS")),
    ("sv", Some("SE")),
    ("th", Some("TH")),
    ("tr", Some("TR")),
    ("uk", Some("UA")),
    ("vi", Some("VN")),
    ("zh", Some("CN")),
];

/// The primary dialect of a language, for example `lt-LT` for `lt`. The
/// other subtags of `lang` stay.
pub fn primary_dialect(lang: &Lang) -> Option<Lang> {
    PRIMARY_DIALECTS
        .binary_search_by(|(language, _)| (*language).cmp(lang.language.as_str()))
        .ok()
        .and_then(|index| PRIMARY_DIALECTS.get(index))
        .map(|(_, region)| Lang {
            region: region.map(str::to_owned),
            ..lang.clone()
        })
}

/// The name of the locale file of a tag: the language and the region.
pub fn locale_code(lang: &Lang) -> String {
    match &lang.region {
        Some(region) => format!("{}-{region}", lang.language),
        None => lang.language.clone(),
    }
}

/// The result of a locale lookup: `None` when no file has the code, else
/// the parse result of the file.
pub type FoundLocale = Option<Result<Locale, CiteprocError>>;

/// Finds the locale of a language with the CSL fallback: the exact code,
/// then the primary dialect. `lookup` returns the locale of one code, or
/// `None` when no file has that code.
pub fn get_locale<E>(
    lang: &Lang,
    lookup: &mut dyn FnMut(&str) -> Result<FoundLocale, E>,
) -> Result<FoundLocale, E> {
    if let Some(found) = lookup(&locale_code(lang))? {
        return Ok(Some(found));
    }
    match primary_dialect(lang) {
        Some(dialect) => lookup(&locale_code(&dialect)),
        None => Ok(None),
    }
}

/// Merges the locales of the style with the locale of the language: first
/// the style locales of the exact tag, then of the primary dialect, then of
/// the two-letter language, then the style locales with no language, then
/// (as upstream, which also matches the locales with no language in the
/// dialect step when the language has no primary dialect)
/// the locale of the language (or of `en-US` when it has none).
///
/// `language` comes from [`effective_language`]. `base` is the locale that
/// [`get_locale`] finds for it, or the locale of `en-US` when it finds none.
pub fn merge_locales(style: &Style, language: &Lang, base: &Locale) -> Locale {
    let primary = primary_dialect(language);
    let exact = style
        .locales
        .iter()
        .filter(|l| l.language.as_ref() == Some(language));
    let dialect = style
        .locales
        .iter()
        .filter(|l| primary.as_ref() != Some(language) && l.language == primary);
    let two_letter = style.locales.iter().filter(|l| {
        l.language
            .as_ref()
            .is_some_and(|tag| tag.region.is_none() && tag.language == language.language)
    });
    let no_language = style.locales.iter().filter(|l| l.language.is_none());
    let mut merged = Locale::default();
    for locale in exact.chain(dialect).chain(two_letter).chain(no_language) {
        merged = merged.merge(locale);
    }
    merged.merge(base)
}

/// The language whose locale applies: the requested one, else the default
/// locale of the style, else `en-US`.
pub fn effective_language(requested: Option<&Lang>, style: &Style) -> Lang {
    requested
        .or(style.options.default_locale.as_ref())
        .cloned()
        .unwrap_or_else(Lang::en_us)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::parse_lang;

    #[test]
    fn the_primary_dialect_table_is_sorted() {
        assert!(PRIMARY_DIALECTS.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn primary_dialects_replace_the_region() {
        let tag = |t: &str| parse_lang(t).unwrap();
        assert_eq!(primary_dialect(&tag("lt")), Some(tag("lt-LT")));
        assert_eq!(primary_dialect(&tag("de-AT")), Some(tag("de-DE")));
        assert_eq!(primary_dialect(&tag("ar-SA")), Some(tag("ar")));
        assert_eq!(primary_dialect(&tag("af")), Some(tag("af-ZA")));
        assert_eq!(primary_dialect(&tag("zh")), Some(tag("zh-CN")));
        assert_eq!(primary_dialect(&tag("xx")), None);
        assert_eq!(locale_code(&tag("sr-Latn-RS")), "sr-RS");
    }

    #[test]
    fn get_locale_tries_the_exact_code_then_the_primary_dialect() {
        let tag = |t: &str| parse_lang(t).unwrap();
        let mut asked = Vec::new();
        let mut lookup = |code: &str| -> Result<FoundLocale, ()> {
            asked.push(code.to_owned());
            Ok((code == "de-DE").then(|| Ok(Locale::default())))
        };
        assert!(matches!(
            get_locale(&tag("de-AT"), &mut lookup),
            Ok(Some(Ok(_)))
        ));
        assert!(matches!(get_locale(&tag("xx-YY"), &mut lookup), Ok(None)));
        assert!(matches!(get_locale(&tag("fr"), &mut lookup), Ok(None)));
        assert_eq!(asked, ["de-AT", "de-DE", "xx-YY", "fr", "fr-FR"]);
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;
    use crate::loader::parse_style;
    use randoc_core::Context;

    fn term(locale: &Locale, name: &str) -> Vec<String> {
        locale
            .terms
            .get(name)
            .map(|list| list.iter().map(|(_, text)| text.clone()).collect())
            .unwrap_or_default()
    }

    fn style_with_locales() -> Style {
        let locales = ["", "de", "de-DE", "de-AT", "fr"]
            .iter()
            .map(|tag| {
                let lang = if tag.is_empty() {
                    String::new()
                } else {
                    format!(" xml:lang=\"{tag}\"")
                };
                let label = if tag.is_empty() { "none" } else { tag };
                format!("<locale{lang}><terms><term name=\"t\">{label}</term></terms></locale>")
            })
            .collect::<String>();
        let text = format!(
            "<style xmlns=\"http://purl.org/net/xbiblio/csl\">{locales}<citation><layout><text value=\"x\"/></layout></citation></style>"
        );
        parse_style(&mut Context::pure(), &text).unwrap()
    }

    #[test]
    fn style_locales_merge_in_the_upstream_order() {
        let style = style_with_locales();
        let file = parse_locale(
            "<locale xml:lang=\"de-AT\"><terms><term name=\"t\">file</term></terms></locale>",
            128,
        )
        .unwrap();
        let tag = |t: &str| crate::lang::parse_lang(t).unwrap();
        let merged = merge_locales(&style, &tag("de-AT"), &file);
        assert_eq!(term(&merged, "t"), ["de-AT", "de-DE", "de", "none", "file"]);
        assert_eq!(merged.language, Some(tag("de-AT")));
        let merged = merge_locales(&style, &tag("de-DE"), &file);
        assert_eq!(term(&merged, "t"), ["de-DE", "de", "none", "file"]);
        // No primary dialect: the upstream dialect step matches the locales with no language.
        let merged = merge_locales(&style, &tag("xx"), &file);
        assert_eq!(term(&merged, "t"), ["none", "none", "file"]);
        assert_eq!(merged.language, Some(tag("de-AT")));
    }

    #[test]
    fn the_effective_language_prefers_the_request_then_the_style() {
        let mut style = style_with_locales();
        let tag = |t: &str| crate::lang::parse_lang(t).unwrap();
        assert_eq!(effective_language(None, &style), Lang::en_us());
        style.options.default_locale = Some(tag("fr-CA"));
        assert_eq!(effective_language(None, &style), tag("fr-CA"));
        assert_eq!(effective_language(Some(&tag("it")), &style), tag("it"));
    }
}
