use std::collections::HashSet;
use std::fmt::Write;

use randoc_ast::Pandoc;

use crate::package::{internal_target, rel_kind};

use super::super::reference::{Reference, relationship};
use super::drawing::escape_content;
use super::meta_value;

const CORE: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";

pub(super) fn add(reference: &mut Reference, doc: &Pandoc, stamp: i64, count: usize) {
    const APP: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";
    let mut core = format!(
        r#"<cp:coreProperties xmlns:cp="{CORE}" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">"#
    );
    for (key, tag, sep) in [
        ("title", "dc:title", " "),
        ("subject", "dc:subject", " "),
        ("author", "dc:creator", "; "),
        ("keywords", "cp:keywords", ", "),
        ("description", "dc:description", " "),
        ("category", "cp:category", " "),
    ] {
        let value = meta_value(doc, key, sep);
        if !value.is_empty() {
            let _ = write!(core, "<{tag}>");
            escape_content(&value, &mut core);
            let _ = write!(core, "</{tag}>");
        }
    }
    let date = iso_time(stamp);
    let _ = write!(
        core,
        r#"<dcterms:created xsi:type="dcterms:W3CDTF">{date}</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">{date}</dcterms:modified></cp:coreProperties>"#
    );
    let app = format!(
        r#"<Properties xmlns="{APP}"><Application>Randoc</Application><Slides>{count}</Slides></Properties>"#
    );
    let rels = &mut reference.root_rels;
    let mut used: HashSet<_> = rels.iter().map(|rel| rel.id.clone()).collect();
    let mut number = 1;
    for (kind, mime, default_path, bytes) in [
        (
            "core-properties",
            "application/vnd.openxmlformats-package.core-properties+xml",
            "docProps/core.xml",
            core.into_bytes(),
        ),
        (
            "extended-properties",
            "application/vnd.openxmlformats-officedocument.extended-properties+xml",
            "docProps/app.xml",
            app.into_bytes(),
        ),
    ] {
        let path = if let Some(path) = rels
            .iter()
            .find(|rel| rel_kind(rel, kind))
            .and_then(|rel| internal_target("", rel))
        {
            path
        } else {
            let mut rel = loop {
                let rel = relationship(number, kind, default_path, false);
                number += 1;
                if used.insert(rel.id.clone()) {
                    break rel;
                }
            };
            if kind == "core-properties" {
                rel.kind = format!(
                    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/{kind}"
                );
            }
            rels.push(rel);
            default_path.into()
        };
        reference.overrides.insert(path.clone(), mime.into());
        reference.parts.insert(path, bytes);
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
