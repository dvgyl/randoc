use std::fmt::Write;

use randoc_ast::{MetaValue, Pandoc};

use super::super::reference::{DECLARATION, Reference, relationship, with_declaration};
use super::meta_value;

const CORE: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";

const CUSTOM: &str = "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties";

/// Adds the document properties as Pandoc does: new `core.xml` and
/// `custom.xml` parts, the `app.xml` part of the reference when it has one,
/// and new package relationships.
pub(super) fn add(reference: &mut Reference, doc: &Pandoc, stamp: i64, count: usize) {
    const APP: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";
    const PACKAGE: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata";
    if !reference.parts.contains_key("docProps/app.xml") {
        let app = format!(
            r#"<Properties xmlns="{APP}"><Application>Randoc</Application><Slides>{count}</Slides></Properties>"#
        );
        reference.parts.insert(
            "docProps/app.xml".into(),
            with_declaration(app).into_bytes(),
        );
    }
    for (path, mime, xml) in [
        (
            "docProps/core.xml",
            "application/vnd.openxmlformats-package.core-properties+xml",
            core(doc, stamp),
        ),
        (
            "docProps/custom.xml",
            "application/vnd.openxmlformats-officedocument.custom-properties+xml",
            custom(doc),
        ),
    ] {
        reference.parts.insert(path.into(), xml.into_bytes());
        reference.overrides.insert(path.into(), mime.into());
    }
    reference.overrides.insert(
        "docProps/app.xml".into(),
        "application/vnd.openxmlformats-officedocument.extended-properties+xml".into(),
    );
    let main = reference.main_path.clone();
    let mut rels = vec![
        relationship(1, "officeDocument", &main, false),
        relationship(2, "core-properties", "docProps/core.xml", false),
        relationship(3, "extended-properties", "docProps/app.xml", false),
        relationship(4, "custom-properties", "docProps/custom.xml", false),
    ];
    // Pandoc gives both property parts the package metadata type.
    for rel in &mut rels[1..3] {
        if let Some((_, kind)) = rel.kind.rsplit_once('/') {
            rel.kind = format!("{PACKAGE}/{kind}");
        }
    }
    reference.root_rels = rels;
}

fn core(doc: &Pandoc, stamp: i64) -> String {
    let mut out = format!(
        r#"{DECLARATION}<cp:coreProperties xmlns:cp="{CORE}" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">"#
    );
    for (key, tag, optional, separator) in [
        ("title", "dc:title", false, " "),
        ("author", "dc:creator", false, "; "),
        ("description", "dc:description", true, " "),
        ("lang", "dc:language", true, " "),
        ("subject", "dc:subject", true, " "),
        ("keywords", "cp:keywords", false, ", "),
    ] {
        if optional && doc.meta.get(key).is_none() {
            continue;
        }
        let _ = write!(out, "<{tag}>");
        escape(&meta_value(doc, key, separator), &mut out);
        let _ = write!(out, "</{tag}>");
    }
    let date = iso_time(stamp);
    let _ = write!(
        out,
        r#"<dcterms:created xsi:type="dcterms:W3CDTF">{date}</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">{date}</dcterms:modified></cp:coreProperties>"#
    );
    out
}

/// Writes each metadata field that `core.xml` does not hold as a custom
/// property, as Pandoc does.
fn custom(doc: &Pandoc) -> String {
    let mut out = format!(
        r#"{DECLARATION}<Properties xmlns="{CUSTOM}" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes""#
    );
    let fields: Vec<_> = doc
        .meta
        .0
        .keys()
        .filter(|name| {
            !matches!(
                name.as_str(),
                "title" | "author" | "description" | "lang" | "subject" | "keywords"
            )
        })
        .collect();
    if fields.is_empty() {
        out.push_str(" />");
        return out;
    }
    out.push('>');
    for (index, name) in fields.into_iter().enumerate() {
        let _ = write!(
            out,
            r#"<property fmtid="{{D5CDD505-2E9C-101B-9397-08002B2CF9AE}}" pid="{}" name=""#,
            index + 2
        );
        escape(name, &mut out);
        out.push_str(r#""><vt:lpwstr>"#);
        let value = match doc.meta.get(name) {
            Some(MetaValue::MetaList(_) | MetaValue::MetaMap(_)) => String::new(),
            Some(MetaValue::MetaBool(true)) => "True".into(),
            Some(MetaValue::MetaBool(false)) => "False".into(),
            _ => meta_value(doc, name, " "),
        };
        escape(&value, &mut out);
        out.push_str("</vt:lpwstr></property>");
    }
    out.push_str("</Properties>");
    out
}

/// Escapes text as Pandoc does in these parts, `'` included.
fn escape(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\0'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}' => {}
            _ => out.push(ch),
        }
    }
}

fn iso_time(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = (month_index + 2) % 12 + 1;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let time = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        time / 60 % 60,
        time % 60
    )
}
