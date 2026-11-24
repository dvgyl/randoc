//! Citation Style Language (CSL) data that Randoc embeds.

use std::sync::OnceLock;

/// The Randoc Author–Date style in CSL XML.
pub const DEFAULT_STYLE: &str = include_str!("../data/csl/randoc-author-date.csl");

const TERMS: &str = include_str!("../data/csl/terms.tsv");
const LOCALES: &str = include_str!("../data/csl/locales.tsv");

fn rows<const N: usize>(table: &'static str) -> impl Iterator<Item = [&'static str; N]> {
    table.lines().skip(1).filter_map(|line| {
        let mut cells = line.split('\t');
        let mut row = [""; N];
        for cell in &mut row {
            *cell = cells.next()?;
        }
        cells.next().is_none().then_some(row)
    })
}

/// Returns the tags of all embedded CSL locales, in sorted order.
pub fn locale_tags() -> &'static [&'static str] {
    static TAGS: OnceLock<Vec<&'static str>> = OnceLock::new();
    TAGS.get_or_init(|| rows::<6>(LOCALES).map(|[tag, ..]| tag).collect())
}

/// Returns the singular and plural page labels, in long then short form.
///
/// Identical labels can occur more than once. Returns `None` if the exact tag
/// has no page terms.
pub fn page_labels(tag: &str) -> Option<Vec<&'static str>> {
    let labels: Vec<_> = rows::<9>(TERMS)
        .filter(|[row_tag, name, ..]| *row_tag == tag && *name == "page")
        .flat_map(|[_, _, _, single, multiple, ..]| {
            [
                single,
                if multiple.is_empty() {
                    single
                } else {
                    multiple
                },
            ]
        })
        .collect();
    (!labels.is_empty()).then_some(labels)
}

/// Builds CSL locale XML for the exact tag. Returns `None` for an unknown tag.
pub fn locale_xml(tag: &str) -> Option<String> {
    let [tag, quote, ordinal, text, numeric, _] =
        rows::<6>(LOCALES).find(|[row_tag, ..]| *row_tag == tag)?;
    let mut xml = String::from("<locale xmlns=\"http://purl.org/net/xbiblio/csl\" version=\"1.0\"");
    attribute(&mut xml, "xml:lang", tag);
    xml.push_str("><style-options");
    optional_attribute(&mut xml, "punctuation-in-quote", quote);
    optional_attribute(&mut xml, "limit-day-ordinals-to-day-1", ordinal);
    xml.push_str("/>");
    date(&mut xml, "text", text)?;
    // CSL locale files require a date format. Page-only locales use the
    // numeric format of the fallback locale.
    let numeric = if numeric.is_empty() {
        rows::<6>(LOCALES).find(|[tag, ..]| *tag == "en-US")?[4]
    } else {
        numeric
    };
    date(&mut xml, "numeric", numeric)?;
    xml.push_str("<terms>");
    for [
        _,
        name,
        form,
        single,
        multiple,
        gender,
        gender_form,
        matching,
        _,
    ] in rows::<9>(TERMS).filter(|[row_tag, ..]| *row_tag == tag)
    {
        xml.push_str("<term");
        attribute(&mut xml, "name", name);
        attribute(&mut xml, "form", form);
        optional_attribute(&mut xml, "gender", gender);
        optional_attribute(&mut xml, "gender-form", gender_form);
        optional_attribute(&mut xml, "match", matching);
        xml.push('>');
        if multiple.is_empty() {
            escape(&mut xml, single);
        } else {
            xml.push_str("<single>");
            escape(&mut xml, single);
            xml.push_str("</single><multiple>");
            escape(&mut xml, multiple);
            xml.push_str("</multiple>");
        }
        xml.push_str("</term>");
    }
    xml.push_str("</terms></locale>");
    Some(xml)
}

fn date(xml: &mut String, form: &str, pattern: &str) -> Option<()> {
    if pattern.is_empty() {
        return Some(());
    }
    let (prefix, parts) = pattern.split_once('{')?;
    xml.push_str("<date");
    attribute(xml, "form", form);
    xml.push('>');
    for (index, part) in parts.split('{').enumerate() {
        let (definition, suffix) = part.split_once('}')?;
        let (name, part_form) = definition.split_once(':').unwrap_or((definition, ""));
        xml.push_str("<date-part");
        attribute(xml, "name", name);
        optional_attribute(xml, "form", part_form);
        if index == 0 {
            optional_attribute(xml, "prefix", prefix);
        }
        optional_attribute(xml, "suffix", suffix);
        xml.push_str("/>");
    }
    xml.push_str("</date>");
    Some(())
}

fn optional_attribute(xml: &mut String, name: &str, value: &str) {
    if !value.is_empty() {
        attribute(xml, name, value);
    }
}

fn attribute(xml: &mut String, name: &str, value: &str) {
    xml.push(' ');
    xml.push_str(name);
    xml.push_str("=\"");
    escape(xml, value);
    xml.push('"');
}

fn escape(xml: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => xml.push_str("&amp;"),
            '<' => xml.push_str("&lt;"),
            '>' => xml.push_str("&gt;"),
            '"' => xml.push_str("&quot;"),
            '\'' => xml.push_str("&apos;"),
            _ => xml.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const FULL: [&str; 7] = [
        "de-DE", "en-GB", "en-US", "es-ES", "fr-FR", "it-IT", "zh-CN",
    ];

    #[test]
    fn tables_join_on_exact_tags() {
        let tags = locale_tags();
        assert_eq!(tags.len(), 60);
        assert!(tags.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(locale_xml("EN-us").is_none());
        assert!(page_labels("en").is_none());
        assert_eq!(
            page_labels("en-US"),
            Some(vec!["page", "pages", "p.", "pp."])
        );
        assert_eq!(
            page_labels("de-DE"),
            Some(vec!["Seite", "Seiten", "S.", "S."])
        );
        assert_eq!(page_labels("zh-TW"), Some(vec!["頁"; 4]));
        for &tag in tags {
            assert_eq!(
                page_labels(tag).map(|labels| labels.len()),
                Some(4),
                "{tag}"
            );
            let xml = locale_xml(tag).unwrap_or_default();
            assert!(xml.contains(&format!("xml:lang=\"{tag}\"")));
            assert!(xml.ends_with("</terms></locale>"));
            assert_eq!(xml.contains("<date form=\"text\""), FULL.contains(&tag));
        }
    }

    #[test]
    fn term_rows_have_unique_keys_and_known_options() {
        let mut keys = BTreeSet::new();
        for row in TERMS.lines().skip(1) {
            assert_eq!(row.split('\t').count(), 9, "{row}");
        }
        for [
            tag,
            name,
            form,
            single,
            multiple,
            gender,
            gender_form,
            matching,
            source,
        ] in rows::<9>(TERMS)
        {
            assert!(locale_tags().contains(&tag), "{tag}");
            assert!(!name.is_empty(), "{tag}");
            assert!(
                !single.is_empty() || (matches!(name, "author" | "ordinal") && multiple.is_empty()),
                "{tag} {name}"
            );
            assert!(keys.insert((tag, name, form, gender, gender_form, matching)));
            assert!(["long", "short", "verb", "verb-short", "symbol"].contains(&form));
            assert!(["", "masculine", "feminine"].contains(&gender));
            assert!(["", "masculine", "feminine"].contains(&gender_form));
            assert!(["", "last-digit", "last-two-digits", "whole-number"].contains(&matching));
            assert!(["linguistic", "style-manual", "observed"].contains(&source));
            assert!(FULL.contains(&tag) || name == "page", "{tag} {name}");
        }
        for row in LOCALES.lines().skip(1) {
            assert_eq!(row.split('\t').count(), 6, "{row}");
        }
        for [tag, quote, ordinal, text, numeric, source] in rows::<6>(LOCALES) {
            assert!(["linguistic", "style-manual", "observed"].contains(&source));
            let full = FULL.contains(&tag);
            assert_eq!(["true", "false"].contains(&quote), full, "{tag}");
            assert_eq!(["true", "false"].contains(&ordinal), full, "{tag}");
            assert_eq!(!text.is_empty() && !numeric.is_empty(), full, "{tag}");
        }
    }

    #[test]
    fn escapes_markup_in_terms_and_date_affixes() {
        let mut xml = String::new();
        escape(&mut xml, "<&>\"'");
        assert_eq!(xml, "&lt;&amp;&gt;&quot;&apos;");
        xml.clear();
        assert_eq!(
            date(&mut xml, "text", "< {day:ordinal} & {month:long}"),
            Some(())
        );
        assert_eq!(
            xml,
            "<date form=\"text\"><date-part name=\"day\" form=\"ordinal\" prefix=\"&lt; \" suffix=\" &amp; \"/><date-part name=\"month\" form=\"long\"/></date>"
        );
    }

    #[test]
    fn style_has_randoc_identity_and_rights() {
        assert!(DEFAULT_STYLE.contains("<id>urn:randoc:style:author-date</id>"));
        assert!(DEFAULT_STYLE.contains("<title>Randoc Author–Date</title>"));
        assert!(DEFAULT_STYLE.contains(">MIT OR Apache-2.0</rights>"));
        assert!(!DEFAULT_STYLE.to_ascii_lowercase().contains("by-sa"));
    }
}
