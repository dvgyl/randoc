use randoc_ast::{Inline, MetaValue};
use randoc_citeproc::case_transform::add_text_case;
use randoc_citeproc::csljson::reference_to_meta;
use randoc_citeproc::reference::{Date, DateParts, Reference, Val, Variable, raw_date_edtf};
use randoc_citeproc::types::TextCase;
use randoc_core::{Limits, Result};
use randoc_latex_reader::FieldOptions;

use crate::{Fields, Locations, charge, depth_error, names, parse_field, string, syntax};

pub(crate) fn read(
    text: &str,
    limits: &Limits,
    latex: bool,
    locations: &Locations<'_>,
) -> Result<Vec<MetaValue>> {
    let (entries, mut budget) = syntax::parse(text, limits, latex, locations)?;
    entries
        .iter()
        .filter(|entry| entry.kind != "xdata")
        .map(|entry| {
            // Count each use of an inherited field before conversion.
            for value in entry.fields.values() {
                charge(&mut budget, value.len())?;
            }
            convert(entry, latex, limits)
        })
        .collect()
}

struct Record<'a> {
    entry: &'a syntax::Entry,
    latex: bool,
    english: bool,
    limits: &'a Limits,
}

impl<'a> Record<'a> {
    fn get(&self, key: &str) -> Option<&'a str> {
        self.entry
            .fields
            .get(key)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
    }

    fn first(&self, keys: &[&str]) -> Option<&'a str> {
        keys.iter().find_map(|key| self.get(key))
    }

    fn field(&self, text: &str, keep_nocase: bool) -> Result<Vec<Inline>> {
        let mut parts = self.parsed_field(text)?.unwrap_or_default();
        if !keep_nocase {
            clear_nocase(&mut parts);
        }
        Ok(parts)
    }

    fn parsed_field(&self, text: &str) -> Result<Option<Vec<Inline>>> {
        let text = field_tex(&names::normalized(text));
        let Some(mut parts) = parse_field(
            &text,
            FieldOptions {
                keep_brace_spans: true,
            },
            self.limits,
        )?
        else {
            return Ok(None);
        };
        if text.contains("\\bibstring") {
            randoc_ast::walk::walk_inlines_mut(&mut parts, |item| {
                if let Inline::RawInline(_, raw) = item
                    && let Some(key) = raw
                        .strip_prefix("\\bibstring")
                        .and_then(|arg| arg.trim_start().strip_prefix('{'))
                        .and_then(|arg| arg.strip_suffix('}'))
                {
                    *item = Inline::Str(bibstring(key).into());
                }
            });
        }
        Ok(Some(parts))
    }

    fn paragraphs(&self, text: &str) -> Result<Vec<Inline>> {
        let mut parts = Vec::new();
        let mut paragraph = String::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                let Some(next) = self.parsed_field(&paragraph)? else {
                    return Ok(Vec::new());
                };
                parts.extend(next);
                paragraph.clear();
            } else {
                paragraph.push_str(line);
                paragraph.push(' ');
            }
        }
        let Some(next) = self.parsed_field(&paragraph)? else {
            return Ok(Vec::new());
        };
        parts.extend(next);
        Ok(parts)
    }

    fn title(&self, text: &str, sentence_case: bool) -> Result<Vec<Inline>> {
        Ok(self.parsed_title(text, sentence_case)?.unwrap_or_default())
    }

    fn parsed_title(&self, text: &str, sentence_case: bool) -> Result<Option<Vec<Inline>>> {
        let Some(mut parts) = self.parsed_field(text)? else {
            return Ok(None);
        };
        if sentence_case && !self.english {
            clear_nocase(&mut parts);
        }
        if sentence_case && self.english {
            randoc_ast::walk::walk_inlines_mut(&mut parts, |item| {
                if let Inline::Span(attr, body) = item
                    && attr.classes.is_empty()
                    && body.iter().any(|item| matches!(item, Inline::Str(text)
                        if text.split(|c: char| !c.is_alphanumeric()).any(|word| word.starts_with(char::is_lowercase))))
                {
                    attr.classes.push("nocase".into());
                }
            });
            protect_spans(&mut parts, true);
            parts = add_text_case(
                None,
                TextCase::SentenceCase,
                parts,
                self.limits.max_nesting_depth,
            )
            .map_err(|_| depth_error())?;
            protect_spans(&mut parts, false);
        }
        Ok(Some(parts))
    }

    fn compound(&self, keys: [&str; 3], sentence_case: bool) -> Result<Vec<Inline>> {
        let [main, subtitle, addon] = keys;
        let Some(text) = self.get(main) else {
            return Ok(Vec::new());
        };
        let Some(mut parts) = self.parsed_title(text, sentence_case)? else {
            return Ok(Vec::new());
        };
        for (key, separator) in [(subtitle, ":"), (addon, ".")] {
            if let Some(text) = self.get(key) {
                join(&mut parts, self.title(text, sentence_case)?, separator);
            }
        }
        Ok(parts)
    }

    fn list(&self, text: &str, localize: bool) -> Result<Vec<Inline>> {
        let text = names::normalized(text);
        let values = if self.latex {
            names::split_and(&text)
        } else {
            vec![text.as_str()]
        };
        let mut out = Vec::new();
        for value in values {
            let text = if localize { bibstring(value) } else { value };
            let next = if text != value {
                vec![Inline::Str(text.into())]
            } else {
                let Some(parts) = self.parsed_field(text)? else {
                    return Ok(Vec::new());
                };
                parts
            };
            if !out.is_empty() {
                if localize && text != value {
                    join(&mut out, next, ";");
                    continue;
                }
                out.extend([Inline::Str(";".into()), Inline::Space]);
            }
            out.extend(next);
        }
        clear_nocase(&mut out);
        Ok(out)
    }
}

fn convert(entry: &syntax::Entry, latex: bool, limits: &Limits) -> Result<MetaValue> {
    let mut record = Record {
        entry,
        latex,
        english: true,
        limits,
    };
    let lang = record.first(&["langid", "hyphenation"]).map(|lang| {
        if lang == "english" {
            let options = record.get("langidopts").unwrap_or_default();
            let variant = options
                .split(',')
                .filter_map(|item| item.split_once('='))
                .find_map(|(key, value)| (key.trim() == "variant").then(|| value.trim()));
            if let Some(variant) = variant {
                return language(variant);
            }
        }
        language(lang)
    });
    record.english = lang.is_none_or(|lang| lang == "en" || lang.starts_with("en-"));
    let kind = kind(&entry.kind);
    let mut map = Fields::new();
    string(&mut map, "id", &entry.id);
    string(&mut map, "type", kind);
    if let Some(lang) = lang {
        string(&mut map, "language", lang);
    }
    titles(&record, &mut map, kind)?;
    let option = |key: &str, value: &str| {
        record.get("options").is_some_and(|options| {
            options
                .split(',')
                .filter_map(|item| item.split_once('='))
                .any(|(k, v)| k.trim() == key && v.trim() == value)
        })
    };
    let use_prefix = option("useprefix", "true")
        || record
            .get("options")
            .is_some_and(|options| options.split(',').any(|part| part.trim() == "useprefix"));
    for (key, field) in [
        ("author", "author"),
        ("editor", "editor"),
        ("bookauthor", "container-author"),
        ("translator", "translator"),
        ("origauthor", "original-author"),
    ] {
        if let Some(value) = record.get(key) {
            let target = if field == "editor" && record.get("editortype") == Some("director") {
                "director"
            } else {
                field
            };
            if let Some(name) = names::bib(value, use_prefix, limits)? {
                map.insert(target.into(), name);
            }
        }
    }
    for (key, field) in [
        ("abstract", "abstract"),
        ("edition", "edition"),
        ("chapter", "chapter-number"),
        ("volumes", "number-of-volumes"),
        ("pagetotal", "number-of-pages"),
        ("version", "version"),
        ("origpublisher", "original-publisher"),
        ("origlocation", "original-publisher-place"),
        ("venue", "event-place"),
        ("keywords", "keyword"),
    ] {
        if let Some(value) = record.get(key) {
            let mut parts = if field == "abstract" {
                record.paragraphs(value)?
            } else {
                record.field(value, true)?
            };
            clear_nocase(&mut parts);
            insert(&mut map, field, parts);
        }
    }
    if let Some(value) = record.first(&["annotation", "annote"]) {
        insert(&mut map, "annote", record.field(value, false)?);
    }
    if let Some(value) = record.get("eventtitle") {
        insert(&mut map, "event", record.title(value, true)?);
    }
    if let Some(value) = record.get("series") {
        let localized = bibstring(value);
        let parts = if localized != value {
            vec![Inline::Str(localized.into())]
        } else if kind == "article-journal"
            && let Ok(number) = value.parse::<u64>()
        {
            let suffix = match (number % 100, number % 10) {
                (11..=13, _) => "th",
                (_, 1) => "st",
                (_, 2) => "nd",
                (_, 3) => "rd",
                _ => "th",
            };
            vec![
                Inline::Str(format!("{number}{suffix}")),
                Inline::Space,
                Inline::Str("series".into()),
            ]
        } else {
            record.title(value, true)?
        };
        insert(&mut map, "collection-title", parts);
    }
    let genre = record.get("type").or(match entry.kind.as_str() {
        "phdthesis" => Some("phdthesis"),
        "mastersthesis" => Some("mathesis"),
        _ => None,
    });
    if let Some(genre) = genre {
        string(&mut map, "genre", bibstring(genre));
    }
    if let Some(volume) = record.get("volume") {
        let volume = record
            .get("part")
            .map_or_else(|| volume.to_owned(), |part| format!("{volume}.{part}"));
        insert(&mut map, "volume", record.field(&volume, false)?);
    }
    if let Some(pages) = record.get("pages") {
        let mut parts = record.field(pages, false)?;
        randoc_ast::walk::walk_inlines_mut(&mut parts, |item| {
            if let Inline::Str(text) = item {
                *text = text.replace('–', "-");
            }
        });
        insert(&mut map, "page", parts);
    }
    let mut issue = Vec::new();
    if let Some(number) = record.get("number") {
        match kind {
            "article-journal" => issue = record.field(number, false)?,
            "report" | "patent" | "legislation" => {
                insert(&mut map, "number", record.field(number, false)?)
            }
            _ if record.get("series").is_some() => {
                insert(&mut map, "collection-number", record.field(number, false)?)
            }
            _ => insert(&mut map, "number", record.field(number, false)?),
        }
    }
    if let Some(value) = record.get("issue") {
        join(&mut issue, record.field(value, false)?, ",");
    }
    insert(&mut map, "issue", issue);
    let mut publisher = Vec::new();
    for field in [
        "school",
        "institution",
        "organization",
        "howpublished",
        "publisher",
    ] {
        if let Some(value) = record.get(field) {
            let parts = if field == "howpublished" {
                let mut parts = record.field(value, true)?;
                clear_nocase(&mut parts);
                parts
            } else {
                record.list(value, false)?
            };
            join(&mut publisher, parts, ";");
        }
    }
    insert(&mut map, "publisher", publisher);
    if let Some(value) = record
        .get("address")
        .or_else(|| latex.then(|| record.get("location")).flatten())
    {
        let target = if kind == "patent" {
            "jurisdiction"
        } else {
            "publisher-place"
        };
        insert(&mut map, target, record.list(value, true)?);
    }
    let mut note = Vec::new();
    if let Some(value) = record.get("note") {
        note = record.field(value, false)?;
    }
    if latex && let Some(value) = record.get("addendum") {
        join(&mut note, record.field(value, false)?, ".");
    }
    insert(&mut map, "note", note);
    if record.get("url").is_none()
        && (!option("url", "false") || kind == "webpage")
        && let Some(id) = record.get("eprint")
        && let Some(provider) = record.first(&["eprinttype", "archiveprefix"])
    {
        let base = match provider.to_ascii_lowercase().as_str() {
            "arxiv" => Some("https://arxiv.org/abs/"),
            "googlebooks" => Some("https://books.google.com?id="),
            "jstor" => Some("https://www.jstor.org/stable/"),
            _ => None,
        };
        if let Some(base) = base {
            string(&mut map, "url", format!("{base}{id}"));
        }
    }
    for key in ["doi", "isbn", "issn", "url", "pmid", "pmcid"] {
        let show = !option(key, "false") || key == "url" && kind == "webpage";
        if show && let Some(value) = record.get(key) {
            string(&mut map, key, value);
        }
    }
    if let Some(date) = record
        .get("date")
        .and_then(date_string)
        .or_else(|| year_month_day(&record))
    {
        string(&mut map, "issued", date);
    } else if let Some(year) = record.get("year") {
        string(&mut map, "issued", year);
        if year == "forthcoming" {
            insert(&mut map, "status", vec![Inline::Str(year.into())]);
        }
    }
    for (key, target) in [
        ("origdate", "original-date"),
        ("urldate", "accessed"),
        ("eventdate", "event-date"),
    ] {
        if let Some(date) = record.get(key).and_then(date_string) {
            string(&mut map, target, date);
        }
    }
    if !map.contains_key("original-date")
        && let Some(date) = record.get("origyear").and_then(date_string)
    {
        string(&mut map, "original-date", date);
    }
    if let Some(value) = record.get("pubstate") {
        insert(
            &mut map,
            "status",
            vec![Inline::Str(bibstring(value).into())],
        );
    }
    Ok(MetaValue::MetaMap(map))
}

fn titles(record: &Record<'_>, map: &mut Fields, kind: &str) -> Result<()> {
    let mut title = record.compound(["title", "subtitle", "titleaddon"], true)?;
    let main = record.compound(["maintitle", "mainsubtitle", "maintitleaddon"], true)?;
    let book = record.compound(["booktitle", "booksubtitle", "booktitleaddon"], true)?;
    let mut container = if record.get("journaltitle").is_some() {
        record.compound(
            ["journaltitle", "journalsubtitle", "journaltitleaddon"],
            false,
        )?
    } else {
        record.compound(["journal", "journalsubtitle", "journaltitleaddon"], false)?
    };
    if matches!(record.entry.kind.as_str(), "periodical" | "suppperiodical") {
        container = record.compound(["title", "subtitle", "titleaddon"], false)?;
        title = record.compound(["issuetitle", "issuesubtitle", "issuetitleaddon"], true)?;
    }
    if !main.is_empty() {
        if kind == "book" && record.entry.kind != "bookinbook" {
            insert(map, "volume-title", std::mem::replace(&mut title, main));
        } else {
            container = main;
            insert(map, "volume-title", book);
        }
    } else if !book.is_empty() && kind != "book" || record.entry.kind == "bookinbook" {
        container = book;
    }
    let short =
        if kind == "book" && record.get("maintitle").is_some() && record.entry.kind != "bookinbook"
        {
            match map.get("volume-title") {
                Some(MetaValue::MetaInlines(parts)) if record.english => before_colon(parts),
                _ => Vec::new(),
            }
        } else if let Some(value) = record.get("shorttitle") {
            record.title(value, true)?
        } else if record.english || record.get("maintitle").is_none() {
            before_colon(&title)
        } else {
            Vec::new()
        };
    insert(map, "title", title);
    insert(map, "title-short", short);
    insert(map, "container-title", container);
    if let Some(value) = record.get("shortjournal") {
        insert(map, "container-title-short", record.title(value, false)?);
    }
    Ok(())
}

fn before_colon(title: &[Inline]) -> Vec<Inline> {
    let mut prefix = Vec::new();
    for item in title {
        if let Inline::Str(s) = item
            && let Some((left, _)) = s.split_once(':')
        {
            if !left.is_empty() {
                prefix.push(Inline::Str(left.to_owned()));
            }
            return prefix;
        }
        prefix.push(item.clone());
    }
    Vec::new()
}

fn clear_nocase(parts: &mut Vec<Inline>) {
    randoc_ast::walk::walk_inlines_mut(parts, |item| {
        if let Inline::Span(attr, _) = item {
            attr.classes.retain(|class| class != "nocase");
        }
    });
}

fn protect_spans(parts: &mut [Inline], protect: bool) {
    for part in parts {
        match part {
            Inline::Span(attr, _) => {
                if protect && !attr.classes.iter().any(|c| c == "nocase") {
                    attr.classes.push("bibtex-protected".into());
                    attr.classes.push("nocase".into());
                } else if !protect && attr.classes.iter().any(|c| c == "bibtex-protected") {
                    attr.classes.pop();
                    attr.classes.pop();
                }
            }
            Inline::Emph(parts)
            | Inline::Strong(parts)
            | Inline::Underline(parts)
            | Inline::Strikeout(parts)
            | Inline::Quoted(_, parts)
            | Inline::Link(_, parts, _)
            | Inline::Image(_, parts, _) => protect_spans(parts, protect),
            _ => {}
        }
    }
}

fn insert(map: &mut Fields, key: &str, value: Vec<Inline>) {
    if !value.is_empty() {
        map.insert(key.into(), MetaValue::MetaInlines(value));
    }
}

fn join(out: &mut Vec<Inline>, next: Vec<Inline>, separator: &str) {
    if next.is_empty() {
        return;
    }
    if !out.is_empty() {
        match out.last_mut() {
            Some(Inline::Str(text)) => text.push_str(separator),
            _ => out.push(Inline::Str(separator.to_owned())),
        }
        out.push(Inline::Space);
    }
    out.extend(next);
}

fn date_string(value: &str) -> Option<String> {
    date_metadata(raw_date_edtf(value)?)
}

fn date_metadata(date: Date) -> Option<String> {
    let key = Variable::new("issued");
    let mut reference = Reference::default();
    reference.variables.insert(key, Val::Date(date));
    let MetaValue::MetaMap(mut map) = reference_to_meta(&reference) else {
        return None;
    };
    match map.remove("issued") {
        Some(MetaValue::MetaString(value)) => Some(value),
        _ => None,
    }
}

pub(crate) const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

fn year_month_day(record: &Record<'_>) -> Option<String> {
    let year = record.get("year")?;
    if let Ok(number) = year.parse::<i64>() {
        let mut parts = vec![number];
        if let Some(month) = record.get("month").and_then(|month| {
            month.parse::<i64>().ok().or_else(|| {
                MONTHS
                    .iter()
                    .zip(1..)
                    .find_map(|(name, number)| name.eq_ignore_ascii_case(month).then_some(number))
            })
        }) {
            parts.push(month);
            if let Some(day) = record.get("day").and_then(|day| day.parse::<i64>().ok()) {
                parts.push(day);
            }
        }
        date_metadata(Date {
            parts: vec![DateParts(parts)],
            ..Date::default()
        })
    } else {
        date_string(year)
    }
}

fn field_tex(text: &str) -> String {
    let text = text
        .replace("\\hyphen ", "-")
        .replace("\\hyphen{}", "-")
        .replace('“', "``")
        .replace('”', "''")
        .replace('‘', "`")
        .replace('’', "'");
    if text.contains("\\mkbibquote{") {
        protect_quotes(&text)
    } else {
        text
    }
}

fn protect_quotes(text: &str) -> String {
    let mut out = String::new();
    let mut quoted = Vec::new();
    let mut depth = 0usize;
    let mut offset = 0;
    while let Some(c) = text[offset..].chars().next() {
        let rest = &text[offset..];
        if rest.starts_with("\\mkbibquote{") {
            out.push_str("{\\enquote{");
            quoted.push(depth);
            depth += 1;
            offset += "\\mkbibquote{".len();
            continue;
        }
        out.push(c);
        offset += c.len_utf8();
        match c {
            '\\' if text[offset..].starts_with(['{', '}', '\\']) => {
                out.push_str(&text[offset..offset + 1]);
                offset += 1;
            }
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if quoted.last() == Some(&depth) {
                    out.push('}');
                    quoted.pop();
                }
            }
            _ => {}
        }
    }
    out
}

fn bibstring(key: &str) -> &str {
    match key {
        "newseries" => "New series",
        "oldseries" => "Old series",
        "inpress" => "in press",
        "inpreparation" => "in preparation",
        "phdthesis" => "PhD thesis",
        "mathesis" => "Master’s thesis",
        "resreport" => "research report",
        "techreport" => "technical report",
        "patreqfr" => "French patent request",
        "patenteu" => "European patent",
        "patentus" => "U.S. patent",
        "patentde" => "German patent",
        "patentfr" => "French patent",
        "patentuk" => "British patent",
        "countryfr" => "France",
        "countryuk" => "United Kingdom",
        "countryde" => "Germany",
        "countryus" => "United States",
        "countryeu" => "European Union",
        _ => key,
    }
}

fn language(value: &str) -> &str {
    match value {
        "english" | "american" | "USenglish" => "en-US",
        "british" | "UKenglish" => "en-GB",
        "australian" => "en-AU",
        "canadian" => "en-CA",
        "newzealand" => "en-NZ",
        "german" | "ngerman" => "de-DE",
        "austrian" | "naustrian" => "de-AT",
        "french" | "francais" => "fr-FR",
        "canadien" => "fr-CA",
        "spanish" => "es-ES",
        "italian" => "it-IT",
        "latin" => "la",
        "dutch" => "nl-NL",
        "portuguese" => "pt-PT",
        "brazilian" | "brazil" => "pt-BR",
        "russian" => "ru-RU",
        "greek" => "el-GR",
        "polish" => "pl-PL",
        "swedish" => "sv-SE",
        "danish" => "da-DK",
        "finnish" => "fi-FI",
        "norwegian" | "norsk" => "nb-NO",
        "czech" => "cs-CZ",
        "turkish" => "tr-TR",
        _ => value,
    }
}

fn kind(kind: &str) -> &'static str {
    match kind {
        "article" | "periodical" | "suppperiodical" => "article-journal",
        "book" | "manual" | "proceedings" | "mvbook" | "collection" | "mvcollection"
        | "mvproceedings" | "reference" | "mvreference" | "bookinbook" => "book",
        "inbook" | "incollection" | "suppbook" | "suppcollection" => "chapter",
        "inproceedings" => "paper-conference",
        "inreference" => "entry-encyclopedia",
        "phdthesis" | "mastersthesis" | "thesis" => "thesis",
        "techreport" | "report" => "report",
        "online" | "www" | "electronic" => "webpage",
        "unpublished" => "manuscript",
        "booklet" => "pamphlet",
        "patent" => "patent",
        "dataset" => "dataset",
        "software" => "software",
        "review" => "review",
        "standard" | "legislation" => "legislation",
        "jurisdiction" => "legal_case",
        "letter" => "personal_communication",
        "audio" => "song",
        "movie" | "video" => "motion_picture",
        "artwork" => "graphic",
        "performance" => "speech",
        _ => "",
    }
}
