use std::fmt::Write as _;

use randoc_ast::MetaValue;
use randoc_core::shared::{is_haskell_alphanum, stringify_meta};
use randoc_templates::{Context, Val};

pub(super) fn xml_id(id: &str) -> String {
    let mut output = String::new();
    for (index, ch) in id.chars().enumerate() {
        let letter = is_haskell_alphanum(ch) && !ch.is_numeric();
        let continuation = ch.is_ascii_digit()
            || matches!(ch, '.' | '-' | '\u{00B7}' | '\u{0300}'..='\u{036F}' | '\u{203F}'..='\u{2040}');
        let valid = letter || ch == '_' || index > 0 && continuation;
        if valid {
            output.push(ch);
        } else {
            let _ = write!(output, "U{:04X}", u32::from(ch));
        }
    }
    output
}

pub(super) fn credit_roles(vars: &mut Context) {
    let Some(authors) = vars.0.get_mut("author") else {
        return;
    };
    for author in items(authors) {
        let Val::Map(author) = author else { continue };
        let Some(roles) = author.0.get_mut("roles") else {
            continue;
        };
        for role in items(roles) {
            let Val::Map(role) = role else { continue };
            if role.get("credit-name").is_some() {
                continue;
            }
            let Some(Val::Simple(credit)) = role.get("credit") else {
                continue;
            };
            let name = credit_name(&credit.render(None));
            role.insert("credit-name", Val::text(name));
        }
    }
}

fn items(value: &mut Val) -> &mut [Val] {
    match value {
        Val::List(items) => items,
        other => std::slice::from_mut(other),
    }
}

fn credit_name(slug: &str) -> &'static str {
    match slug {
        "conceptualization" => "Conceptualization",
        "data-curation" => "Data curation",
        "formal-analysis" => "Formal analysis",
        "funding-acquisition" => "Funding acquisition",
        "investigation" => "Investigation",
        "methodology" => "Methodology",
        "project-administration" => "Project administration",
        "resources" => "Resources",
        "software" => "Software",
        "supervision" => "Supervision",
        "validation" => "Validation",
        "visualization" => "Visualization",
        "writing-original-draft" => "Writing – original draft",
        "writing-review-editing" => "Writing – review &amp; editing",
        _ => "",
    }
}

pub(super) fn date_parts(value: &MetaValue) -> Option<Vec<String>> {
    let parts = match value {
        MetaValue::MetaMap(date) => {
            if let Some(MetaValue::MetaList(dates)) = date.get("date-parts") {
                let MetaValue::MetaList(parts) = dates.first()? else {
                    return None;
                };
                parts.iter().take(3).map(stringify_meta).collect()
            } else {
                return date_parts(date.get("raw").or_else(|| date.get("literal"))?);
            }
        }
        other => stringify_meta(other)
            .trim()
            .split('-')
            .map(str::to_owned)
            .collect::<Vec<_>>(),
    };
    let (year, rest) = parts.split_first()?;
    if rest.len() > 2 || year.is_empty() || !year.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let month = rest
        .first()
        .map(|month| month.parse::<u8>().ok().filter(|n| (1..=12).contains(n)));
    let day = rest
        .get(1)
        .map(|day| day.parse::<u8>().ok().filter(|n| (1..=31).contains(n)));
    if matches!(month, Some(None)) || matches!(day, Some(None)) {
        return None;
    }
    Some(
        std::iter::once(year.clone())
            .chain(
                month
                    .into_iter()
                    .chain(day)
                    .flatten()
                    .map(|n| format!("{n:02}")),
            )
            .collect(),
    )
}
