//! Document labels and LaTeX language aliases.

use std::collections::BTreeMap;
use std::sync::OnceLock;

type Pair = (&'static str, &'static str);
type Tables = BTreeMap<String, Vec<Pair>>;

const TERMS: &str = include_str!("../data/terms.tsv");
const BABEL_ALIASES: &str = include_str!("../data/babel_aliases.tsv");

static TABLES: OnceLock<Tables> = OnceLock::new();
static ALIASES: OnceLock<BTreeMap<&'static str, &'static str>> = OnceLock::new();

/// Returns the label table for an exact BCP 47 tag. ASCII case does not matter.
pub fn term_table(tag: &str) -> Option<&'static [Pair]> {
    let tables = TABLES.get_or_init(|| {
        let mut tables = Tables::new();
        for row in TERMS.lines().skip(1) {
            if let Some((tag, label)) = row.split_once('\t')
                && let Some(pair) = label.split_once('\t')
            {
                tables
                    .entry(tag.to_ascii_lowercase())
                    .or_default()
                    .push(pair);
            }
        }
        tables
    });
    tables.get(&tag.to_ascii_lowercase()).map(Vec::as_slice)
}

/// Returns the BCP 47 tag for an exact Babel or Polyglossia language name.
/// ASCII case does not matter.
pub fn babel_language_tag(name: &str) -> Option<&'static str> {
    let aliases = ALIASES.get_or_init(|| {
        BABEL_ALIASES
            .lines()
            .skip(1)
            .filter_map(|row| row.split_once('\t'))
            .collect()
    });
    aliases.get(name.to_ascii_lowercase().as_str()).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn term_tables_match_exact_tags_ignoring_ascii_case() {
        for (tag, figure) in [("en", "Figure"), ("de", "Abbildung"), ("sr-Latn", "Slika")] {
            let table = term_table(tag).unwrap();
            assert_eq!(table.len(), 26);
            assert!(table.contains(&("Figure", figure)));
            assert_eq!(term_table(&tag.to_ascii_uppercase()), Some(table));
        }
        assert!(term_table("de-DE").is_none());
        assert!(term_table("unknown").is_none());
    }

    #[test]
    fn term_tables_cover_the_pandoc_tags_with_all_labels() {
        let tags: std::collections::BTreeSet<_> = TERMS
            .lines()
            .skip(1)
            .filter_map(|row| row.split('\t').next())
            .collect();
        assert_eq!(tags.len(), 107);
        for tag in ["zh", "de-AT", "de-CH"] {
            assert!(!tags.contains(tag), "{tag}");
            assert!(term_table(tag).is_none(), "{tag}");
        }
        let keys: Vec<_> = term_table("en").unwrap().iter().map(|(k, _)| *k).collect();
        assert_eq!(keys.len(), 26);
        for tag in tags {
            let table = term_table(tag).unwrap();
            let names: Vec<_> = table.iter().map(|(k, _)| *k).collect();
            assert_eq!(names, keys, "{tag}");
            assert!(table.iter().all(|(_, value)| !value.is_empty()), "{tag}");
        }
    }

    #[test]
    fn babel_aliases_match_exact_names_ignoring_ascii_case() {
        for (alias, tag) in [
            ("german", "de"),
            ("ngerman", "de"),
            ("french", "fr"),
            ("serbian", "sr"),
            ("swissgerman", "de-CH"),
        ] {
            assert_eq!(babel_language_tag(alias), Some(tag));
            assert_eq!(babel_language_tag(&alias.to_ascii_uppercase()), Some(tag));
        }
        assert!(babel_language_tag("unknown").is_none());
    }
}
