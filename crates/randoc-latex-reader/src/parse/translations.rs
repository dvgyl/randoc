//! Caption commands and the active translation table.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};

use randoc_ast::Text;
use randoc_ast::json::{JsonKind, parse_value, value_from_json};
use randoc_core::{Context, Term};
use randoc_data::terms::{babel_language_tag, term_table};

type Table = BTreeMap<Text, Text>;

/// Returns the term for a supported LaTeX caption command.
pub(crate) fn caption_term(name: &str) -> Option<Term> {
    Some(match name {
        "figurename" => Term::Figure,
        "tablename" => Term::Table,
        "contentsname" => Term::Contents,
        "refname" => Term::References,
        "abstractname" => Term::Abstract,
        "proofname" => Term::Proof,
        "lstlistingname" => Term::Listing,
        "bibname" => Term::Bibliography,
        "chaptername" => Term::Chapter,
        "partname" => Term::Part,
        "listfigurename" => Term::ListOfFigures,
        "listtablename" => Term::ListOfTables,
        "indexname" => Term::Index,
        "prefacename" => Term::Preface,
        "pagename" => Term::Page,
        "seename" => Term::See,
        "glossaryname" => Term::Glossary,
        "enclname" => Term::Encl,
        "ccname" => Term::Cc,
        "headtoname" => Term::To,
        _ => return None,
    })
}

/// The translation tables that one reader run loaded, by language tag.
pub(super) type Tables = HashMap<&'static str, Table>;

/// Sets the caption labels for a Polyglossia language name. Pandoc
/// ignores names that it does not know, such as BCP 47 tags (`de`), names
/// with capitals (`German`) and names with spaces (` german `).
pub(super) fn switch(context: &mut Context, tables: &mut Tables, language: &str) {
    if language.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return;
    }
    let Some(tag) = babel_language_tag(language) else {
        return;
    };
    let table = match tables.entry(tag) {
        Entry::Occupied(entry) => entry.get().clone(),
        Entry::Vacant(entry) => {
            let Some(table) = load(context, tag) else {
                return;
            };
            entry.insert(table).clone()
        }
    };
    context.set_translations(Some(table));
}

/// Reads the table for the tag, else for its base language. For each tag,
/// the user file comes before the bundled table.
fn load(context: &mut Context, tag: &str) -> Option<Table> {
    let base = tag.split('-').next().unwrap_or(tag);
    [Some(tag), (base != tag).then_some(base)]
        .into_iter()
        .flatten()
        .find_map(|tag| {
            user_table(context, tag).or_else(|| {
                let table = term_table(tag)?;
                Some(
                    table
                        .iter()
                        .map(|&(key, value)| (key.into(), value.into()))
                        .collect(),
                )
            })
        })
}

fn user_table(context: &mut Context, tag: &str) -> Option<Table> {
    let dir = context.state.user_data_dir.as_deref()?;
    let path = if dir.is_empty() {
        format!("translations/{tag}.yaml")
    } else {
        format!("{}/translations/{tag}.yaml", dir.trim_end_matches('/'))
    };
    let limit = usize::try_from(context.limits.max_resource_size).unwrap_or(usize::MAX);
    let bytes = context.host().read_file_limited(&path, limit).ok()?;
    parse_table(std::str::from_utf8(&bytes).ok()?)
}

/// Reads a flat map of single-line string labels.
fn parse_table(text: &str) -> Option<Table> {
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.starts_with('{') {
        let json = parse_value(text, 2).ok()?;
        return Some(
            json.root()
                .entries()
                .filter_map(|(key, value)| match value.kind() {
                    JsonKind::String(value) => Some((key.into(), value.into())),
                    _ => None,
                })
                .collect(),
        );
    }
    let mut table = Table::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || matches!(line, "---" | "...") {
            continue;
        }
        let (key, value) = line.split_once(':')?;
        if let (Some(key), Some(value)) = (label_scalar(key), label_scalar(value)) {
            table.insert(key, value);
        }
    }
    Some(table)
}

fn label_scalar(text: &str) -> Option<Text> {
    let text = text.trim();
    if text.starts_with(['\'', '"']) {
        let quote = text.chars().next()?;
        let mut chars = text.char_indices().skip(1).peekable();
        while let Some((index, c)) = chars.next() {
            if quote == '"' && c == '\\'
                || quote == '\''
                    && c == quote
                    && chars.peek().is_some_and(|&(_, next)| next == quote)
            {
                chars.next();
            } else if c == quote {
                let suffix = text.get(index + 1..)?;
                if !suffix.trim().is_empty()
                    && !(suffix.starts_with(char::is_whitespace)
                        && suffix.trim_start().starts_with('#'))
                {
                    return None;
                }
                return if quote == '"' {
                    value_from_json(text.get(..=index)?).ok()
                } else {
                    Some(text.get(1..index)?.replace("''", "'"))
                };
            }
        }
        return None;
    }
    let end = text
        .match_indices('#')
        .find(|&(index, _)| index == 0 || text[..index].ends_with(char::is_whitespace))
        .map_or(text.len(), |(index, _)| index);
    let value = text[..end].trim_end();
    if value.is_empty()
        || value.starts_with(['[', '{', '&', '*', '!', '|', '>'])
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "null" | "~" | "true" | "false"
        )
        || value.parse::<f64>().is_ok()
    {
        return None;
    }
    Some(value.into())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use randoc_ast::{Block, Inline};
    use randoc_core::{Extension, Extensions, Host, LogMessage, PureHost, ReaderOptions, Sources};

    use super::*;

    fn blocks(input: &str, context: &mut Context) -> Vec<Block> {
        crate::read(
            &Sources::from_text(input),
            &ReaderOptions::default(),
            context,
        )
        .unwrap()
        .blocks
    }

    fn str_inline(value: &str) -> Inline {
        Inline::Str(value.into())
    }

    fn paragraph(value: &str) -> Block {
        Block::Para(vec![str_inline(value)])
    }

    fn user_context(files: &[(&str, &str)]) -> Context {
        let mut host = PureHost::default();
        for &(path, text) in files {
            host.files.insert(path.into(), text.as_bytes().to_vec());
        }
        let mut context = Context::new(Box::new(host));
        context.state.user_data_dir = Some("user".into());
        context
    }

    #[test]
    fn caption_fixtures_preserve_spacing_and_punctuation() {
        let cases = [
            ("en", r"\figurename\ 2", vec![paragraph("Figure\u{a0}2")]),
            (
                "de-DE",
                r"\figurename\ 2",
                vec![paragraph("Abbildung\u{a0}2")],
            ),
            (
                "en",
                "\\setmainlanguage{german}\n\\figurename 2",
                vec![paragraph("Abbildung2")],
            ),
            (
                "sr-Latn",
                "\\figurename~2\n\\figurename.",
                vec![Block::Para(vec![
                    str_inline("Slika\u{a0}2"),
                    Inline::SoftBreak,
                    str_inline("Slika."),
                ])],
            ),
        ];
        for (tag, input, expected) in cases {
            let mut context = Context::pure();
            let table = load(&mut context, tag);
            context.set_translations(table);
            assert_eq!(blocks(input, &mut context), expected, "{tag}");
            assert!(context.log().is_empty(), "{tag}");
        }
    }

    #[test]
    fn each_caption_command_uses_its_term() {
        let text = concat!(
            r"\figurename/\tablename/\contentsname/\refname/\abstractname/\proofname/",
            r"\lstlistingname/\bibname/\chaptername/\partname/\listfigurename/\listtablename/",
            r"\indexname/\prefacename/\pagename/\seename/\glossaryname/\enclname/\ccname/\headtoname",
            r"/\appendixname.\alsoname.",
        );
        let mut context = Context::pure();
        let terms = term_table("en").unwrap();
        context.set_translations(Some(
            terms
                .iter()
                .map(|&(key, _)| (key.into(), key.into()))
                .collect(),
        ));
        assert_eq!(
            blocks(text, &mut context),
            [paragraph(concat!(
                "Figure/Table/Contents/References/Abstract/Proof/Listing/Bibliography/Chapter/Part/",
                "ListOfFigures/ListOfTables/Index/Preface/Page/See/Glossary/Encl/Cc/To/..",
            ))]
        );
    }

    #[test]
    fn raw_tex_keeps_unsupported_commands_and_translates_captions() {
        let mut options = ReaderOptions::default();
        options.extensions = Extensions::from_list(&[Extension::RawTex]);
        let input = Sources::from_text("\\selectlanguage{french}\n\\tablename*\n\n\\appendixname");
        let doc = crate::read(&input, &options, &mut Context::pure()).unwrap();
        let latex = |text: &str| Block::RawBlock(randoc_ast::Format::new("latex"), text.into());
        assert_eq!(
            doc.blocks,
            [
                latex(r"\selectlanguage{french}"),
                paragraph("Table"),
                latex(r"\appendixname"),
            ]
        );
    }

    #[test]
    fn select_language_keeps_captions() {
        for (input, command, expected) in [
            (
                r"\selectlanguage{german}\figurename.",
                r"\selectlanguage{german}",
                "Figure.",
            ),
            (
                r"\selectlanguage{french} \tablename.",
                r"\selectlanguage{french}",
                "Table.",
            ),
        ] {
            let mut context = Context::pure();
            assert_eq!(
                blocks(input, &mut context),
                [paragraph(expected)],
                "{input}"
            );
            assert!(context.state.translations.is_none(), "{input}");
            assert_eq!(
                context.log(),
                [LogMessage::SkippedContent(
                    command.into(),
                    randoc_core::SourcePos::new("", 1, 24)
                )],
                "{input}"
            );
        }
    }

    #[test]
    fn main_and_default_commands_change_captions() {
        let mut context = Context::pure();
        let input = concat!(
            r"\documentclass{article}\setdefaultlanguage{german}\begin{document}",
            r"\tablename/\chaptername \setmainlanguage{serbianc}\figurename.",
            r" \selectlanguage{french}\tablename\end{document}",
        );
        assert_eq!(
            blocks(input, &mut context),
            [
                paragraph("Tabelle/Kapitel"),
                Block::Para(vec![
                    str_inline("Слика."),
                    Inline::Space,
                    str_inline("Табела"),
                ]),
            ]
        );
        assert_eq!(context.term(Term::Table), "Табела");
    }

    #[test]
    fn switching_languages_clears_pending_translation_warning() {
        let mut context = Context::pure();
        context.set_translation_missing("qq".into(), "translations/qq.yaml".into());
        assert_eq!(
            blocks(r"\setmainlanguage{german}\figurename.", &mut context),
            [paragraph("Abbildung.")]
        );
        assert!(context.log().is_empty());
    }

    #[test]
    fn only_polyglossia_names_switch_captions() {
        for (argument, figure, table) in [
            ("{de}", "Figure.", "Table."),
            ("{fr}", "Figure.", "Table."),
            ("{es-MX}", "Figure.", "Table."),
            ("{qq}", "Figure.", "Table."),
            ("{klingon}", "Figure.", "Table."),
            ("{xx-YY}", "Figure.", "Table."),
            ("{German}", "Figure.", "Table."),
            ("{ german }", "Figure.", "Table."),
            ("{german}", "Abbildung.", "Tabelle."),
            ("[variant=swiss]{german}", "Abbildung.", "Tabelle."),
        ] {
            let mut context = Context::pure();
            let input = format!(r"\setmainlanguage{argument}\figurename. \tablename.");
            assert_eq!(
                blocks(&input, &mut context),
                [Block::Para(vec![
                    str_inline(figure),
                    Inline::Space,
                    str_inline(table),
                ])],
                "{argument}"
            );
            assert!(context.log().is_empty(), "{argument}");
        }
    }

    #[test]
    fn unknown_name_keeps_the_current_captions() {
        let mut context = Context::pure();
        assert_eq!(
            blocks(
                r"\setmainlanguage{german}\figurename. \setmainlanguage{klingon}\figurename.",
                &mut context
            ),
            [paragraph("Abbildung."), paragraph("Abbildung.")]
        );
        assert!(context.log().is_empty());
    }

    #[test]
    fn translation_files_take_precedence_as_whole_tables() {
        let mut context = user_context(&[
            (
                "user/translations/sr-Cyrl.yaml",
                "Figure: 'Cyrillic ''figure''' # Comment\n",
            ),
            (
                "user/translations/sr.yaml",
                "Figure: Base figure\nTable: \"Table # base\"\n",
            ),
        ]);
        assert_eq!(
            blocks(
                r"\setmainlanguage{serbianc}\figurename/\tablename/\tablename.",
                &mut context
            ),
            [paragraph("Cyrillic 'figure'//.")]
        );
        assert_eq!(
            context.log(),
            [
                LogMessage::NoTranslation("Table".into()),
                LogMessage::NoTranslation("Table".into()),
            ]
        );
        assert_eq!(
            blocks(
                r"\setmainlanguage{serbian}\figurename/\tablename.",
                &mut context
            ),
            [paragraph("Base figure/Table # base.")]
        );
    }

    #[test]
    fn bundled_tag_table_comes_before_user_base_file() {
        let mut context = user_context(&[
            ("user/translations/SR-cyrl.yaml", "Figure: Wrong case\n"),
            ("user/translations/sr.yaml", "Figure: Base figure\n"),
        ]);
        assert_eq!(
            blocks(r"\setmainlanguage{serbianc}\figurename.", &mut context),
            [paragraph("Слика.")]
        );
        assert!(context.log().is_empty());
    }

    struct CountingHost {
        inner: PureHost,
        reads: Arc<AtomicUsize>,
    }

    impl Host for CountingHost {
        fn read_file(&mut self, path: &str) -> std::io::Result<Vec<u8>> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            self.inner.read_file(path)
        }
    }

    #[test]
    fn repeated_switches_read_each_file_once() {
        let reads = Arc::new(AtomicUsize::new(0));
        let mut inner = PureHost::default();
        inner.files.insert(
            "user/translations/de.yaml".into(),
            b"Figure: User\n".to_vec(),
        );
        let mut context = Context::new(Box::new(CountingHost {
            inner,
            reads: Arc::clone(&reads),
        }));
        context.state.user_data_dir = Some("user".into());
        let input = r"\setmainlanguage{german}\figurename. \setmainlanguage{serbian}\figurename. "
            .repeat(3);
        assert_eq!(
            blocks(&input, &mut context),
            [paragraph("User."), paragraph("Слика.")]
                .into_iter()
                .cycle()
                .take(6)
                .collect::<Vec<_>>()
        );
        assert_eq!(reads.load(Ordering::Relaxed), 2);
    }
}
