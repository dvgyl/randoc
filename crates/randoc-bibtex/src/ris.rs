use std::collections::{HashMap, HashSet};

use randoc_ast::MetaValue;
use randoc_core::Result;

use crate::{EndOfInput, Fields, Locations, diagnostic, names, string, words};

pub(crate) fn read(text: &str, locations: &Locations<'_>) -> Result<Vec<MetaValue>> {
    let mut out = Vec::new();
    let mut fields = Vec::new();
    let mut used = HashSet::new();
    let mut counters = HashMap::new();
    let mut offset = text.len() - text.trim_start_matches('\u{feff}').len();
    let body = &text[offset..];
    if body.starts_with(char::is_whitespace) {
        let start = text.len() - body.trim_start().len();
        if start == text.len() {
            return Ok(out);
        }
        return Err(diagnostic(
            text,
            start,
            Some("end of input"),
            locations,
            EndOfInput::Actual,
        ));
    }
    for line in body.split_inclusive('\n') {
        if !(fields.is_empty() && line.trim().is_empty()) {
            let (tag, value) = tag_value(text, offset, line, fields.is_empty(), locations)?;
            if tag == "ER" {
                out.push(reference(&fields, &mut used, &mut counters));
                fields.clear();
            } else {
                fields.push((tag, value));
            }
        }
        offset += line.len();
    }
    if !fields.is_empty() {
        return Err(diagnostic(
            text,
            text.len(),
            None,
            locations,
            EndOfInput::Ris,
        ));
    }
    Ok(out)
}

fn tag_value<'a>(
    text: &str,
    offset: usize,
    line: &'a str,
    outside: bool,
    locations: &Locations<'_>,
) -> Result<(&'a str, &'a str)> {
    let error = |index, expected| {
        diagnostic(
            text,
            offset + index,
            expected,
            locations,
            EndOfInput::Actual,
        )
    };
    let mut chars = line.char_indices();
    for _ in 0..2 {
        match chars.next() {
            Some((_, c)) if c.is_ascii_uppercase() || c.is_ascii_digit() => {}
            Some((index, _)) => {
                let expected = (outside && index == 0).then_some("white space or end of input");
                return Err(error(index, expected));
            }
            None => return Err(error(line.len(), None)),
        }
    }
    let tag = &line[..2];
    let rest = &line[2..];
    let spaces = rest.trim_start_matches([' ', '\t']);
    if rest.len() == spaces.len() {
        return Err(error(2, None));
    }
    let Some(value) = spaces.strip_prefix('-') else {
        return Err(error(line.len() - spaces.len(), None));
    };
    Ok((tag, value.trim()))
}

fn reference(
    fields: &[(&str, &str)],
    used: &mut HashSet<String>,
    counters: &mut HashMap<String, usize>,
) -> MetaValue {
    let first = |tags: &[&str]| {
        fields
            .iter()
            .find(|(tag, _)| tags.contains(tag))
            .map(|(_, value)| *value)
    };
    let authors: Vec<_> = fields
        .iter()
        .filter(|(tag, _)| matches!(*tag, "AU" | "A1"))
        .map(|(_, value)| *value)
        .collect();
    let mut map = Fields::new();
    let date = first(&["PY", "Y1"]);
    let base = first(&["ID"]).map(str::to_owned).unwrap_or_else(|| {
        let mut id = authors
            .iter()
            .filter_map(|author| author.split([',', ' ']).next())
            .collect::<Vec<_>>()
            .join("_");
        if let Some(date) = date {
            id.push('_');
            id.push_str(date);
        }
        id
    });
    let mut id = base.clone();
    if !used.insert(id.clone()) {
        let count = counters.entry(base.clone()).or_insert(0);
        loop {
            id = format!("{base}{}", suffix(*count));
            *count += 1;
            if used.insert(id.clone()) {
                break;
            }
        }
    }
    string(&mut map, "id", id);
    string(&mut map, "type", first(&["TY"]).map_or("", kind));
    if let Some(date) = date {
        string(&mut map, "issued", date);
    }
    for (key, values) in [
        (
            "author",
            authors.into_iter().map(names::ris).collect::<Vec<_>>(),
        ),
        (
            "editor",
            fields
                .iter()
                .filter(|(tag, _)| matches!(*tag, "A2" | "ED"))
                .map(|(_, value)| names::ris(value))
                .collect(),
        ),
    ] {
        if !values.is_empty() {
            map.insert(key.into(), MetaValue::MetaList(values));
        }
    }
    for (tags, key) in [
        (&["TI", "T1"][..], "title"),
        (&["T2", "JO", "JF"], "container-title"),
        (&["VL"], "volume"),
        (&["IS"], "issue"),
        (&["PB"], "publisher"),
        (&["PP"], "publisher-place"),
        (&["DO"], "DOI"),
        (&["SN"], "ISSN"),
        (&["UR"], "url"),
        (&["AB"], "abstract"),
        (&["ET"], "edition"),
        (&["LA"], "language"),
    ] {
        if let Some(value) = first(tags) {
            map.insert(key.into(), MetaValue::MetaInlines(words(value)));
        }
    }
    let start = first(&["SP"]);
    let end = first(&["EP"]);
    if start.is_some() || end.is_some() {
        let start = start.unwrap_or("");
        let pages = end.map_or_else(|| start.to_owned(), |end| format!("{start}-{end}"));
        map.insert("page".into(), MetaValue::MetaInlines(words(&pages)));
    }
    let keywords: Vec<_> = fields
        .iter()
        .rev()
        .filter(|(tag, _)| *tag == "KW")
        .map(|(_, value)| *value)
        .collect();
    if !keywords.is_empty() {
        string(&mut map, "keyword", keywords.join(", "));
    }
    MetaValue::MetaMap(map)
}

fn suffix(index: usize) -> String {
    let code = index.saturating_add('a' as usize);
    u32::try_from(code)
        .ok()
        .and_then(char::from_u32)
        .map_or_else(|| index.to_string(), |c| c.to_string())
}

fn kind(code: &str) -> &'static str {
    match code {
        "JOUR" | "JFULL" | "INPR" => "article-journal",
        "EJOUR" | "ABST" | "SER" => "article",
        "BOOK" | "EBOOK" | "DICT" | "EDBOOK" | "CLSWK" => "book",
        "CHAP" | "ECHAP" => "chapter",
        "CONF" | "CPAPER" => "paper-conference",
        "ENCYC" => "entry-encyclopedia",
        "THES" => "thesis",
        "PAT" => "patent",
        "WEB" | "MULTI" => "webpage",
        "BLOG" => "post-weblog",
        "NEWS" => "article-newspaper",
        "MGZN" => "article-magazine",
        "COMP" => "program",
        "ADVS" | "MPCT" => "motion-picture",
        "VIDEO" | "ART" | "CHART" | "SLIDE" => "graphic",
        "EQUA" | "FIGURE" => "figure",
        "UNPB" => "unpublished",
        "RPRT" | "GOVDOC" | "GRANT" | "HEAR" | "STAND" => "report",
        "BILL" | "UNBILL" => "bill",
        "CASE" | "LEGAL" => "legal_case",
        "STAT" => "legislation",
        "DATA" | "DBASE" | "AGGR" => "dataset",
        "SOUND" | "MUSIC" => "musical_score",
        "ICOMM" | "PCOMM" => "personal_communication",
        "MAP" => "map",
        "PAMP" => "pamphlet",
        "CTLG" => "catalog",
        "GEN" => "entry",
        _ => "misc",
    }
}
