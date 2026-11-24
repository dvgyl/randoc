use std::fmt::Write as _;

use randoc_core::shared::is_haskell_alphanum;
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
