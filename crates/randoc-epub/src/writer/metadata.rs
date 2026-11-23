use std::collections::BTreeMap;
use std::fmt::Write as _;

use randoc_archive::xml::{self, XmlElement, XmlLimits, XmlNode, XmlOptions};
use randoc_ast::{Inline, Meta, MetaValue};
use randoc_core::{
    Context, Error, Result, WriterOptions,
    shared::{meta_context::normalize_date, stringify_meta},
};

use super::escape_package as escape;

pub(super) const DC: &str = "http://purl.org/dc/elements/1.1/";
pub(super) const OPF: &str = "http://www.idpf.org/2007/opf";

pub(super) struct Metadata {
    pub title: String,
    pub document_title: Option<String>,
    pub title_inlines: Vec<Inline>,
    pub language: String,
    pub identifier: String,
    pub timestamp: i64,
    pub xml: String,
}

impl Metadata {
    pub fn new(
        meta: &Meta,
        options: &WriterOptions,
        context: &mut Context,
        modern: bool,
    ) -> Result<Self> {
        let timestamp = context
            .host()
            .environment_variable("SOURCE_DATE_EPOCH")
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| context.host().current_time().unix_seconds);
        let extra = extra_metadata(options, context)?;
        let extra = |key: &str| extra.get(key).into_iter().flatten();
        let entries = |key, fields: &[&'static str]| {
            let mut entries: Vec<_> = extra(key)
                .map(|element| Entry::from_xml(element, fields))
                .collect();
            entries.extend(
                meta.get(key)
                    .into_iter()
                    .flat_map(values)
                    .map(|value| Entry::from_meta(value, fields)),
            );
            entries
        };
        let title_inlines = match meta
            .get("title")
            .into_iter()
            .flat_map(values)
            .next()
            .map(plain_value)
        {
            Some(MetaValue::MetaInlines(inlines)) => inlines.clone(),
            Some(value) => vec![Inline::Str(stringify_meta(value))],
            None => Vec::new(),
        };
        let titles = entries("title", &["file-as", "title-type"]);
        let document_title = meta.get("title").map(title_text);
        let title = document_title
            .clone()
            .or_else(|| extra("title").next().map(element_text))
            .unwrap_or_else(|| "UNTITLED".into());
        let language = extra("language")
            .next()
            .map(element_text)
            .or_else(|| {
                meta.get("lang")
                    .or_else(|| meta.get("language"))
                    .map(stringify_meta)
            })
            .unwrap_or_else(|| {
                context.host().environment_variable("LANG").map_or_else(
                    || "en-US".into(),
                    |lang| lang.split('.').next().unwrap_or(&lang).replace('_', "-"),
                )
            });
        let mut ids = entries("identifier", &["identifier-type"]);
        if ids.is_empty() {
            ids.push(Entry {
                text: uuid(context),
                fields: Vec::new(),
            });
        }
        let identifier = ids
            .first()
            .map_or_else(String::new, |entry| entry.text.clone());

        let mut xml = String::new();
        write_entries(&mut xml, "identifier", "epub-id", ids, modern);
        write_entries(&mut xml, "title", "epub-title", titles, modern);

        let mut dates = entries("date", &["event"]);
        if dates.is_empty() {
            dates.push(Entry {
                text: iso_time(timestamp),
                fields: Vec::new(),
            });
        } else {
            for date in &mut dates {
                date.text = epub_date(&date.text);
            }
        }
        if modern {
            if let Some(date) = dates.first() {
                dc(&mut xml, "date", "epub-date", &date.text, &[], true);
            }
        } else {
            write_entries(&mut xml, "date", "epub-date", dates, false);
        }
        dc(&mut xml, "language", "", &language, &[], modern);

        let people_fields = ["file-as", "role"];
        let creators = meta
            .get("author")
            .into_iter()
            .flat_map(values)
            .map(|value| {
                let mut entry = Entry::from_meta(value, &people_fields);
                if field(value, "role").is_none() {
                    entry.fields.push(("role", "aut".into()));
                }
                entry
            })
            .chain(entries("creator", &people_fields));
        write_entries(&mut xml, "creator", "epub-creator", creators, modern);
        write_entries(
            &mut xml,
            "contributor",
            "epub-contributor",
            entries("contributor", &people_fields),
            modern,
        );
        write_entries(
            &mut xml,
            "subject",
            "subject",
            entries("subject", &[]),
            modern,
        );
        for key in [
            "description",
            "type",
            "format",
            "publisher",
            "source",
            "relation",
            "coverage",
            "rights",
        ] {
            let value = extra(key).next().map(element_text).or_else(|| {
                meta.get(key).map(|value| {
                    if matches!(value, MetaValue::MetaList(_)) {
                        String::new()
                    } else {
                        stringify_meta(plain_value(value))
                    }
                })
            });
            if let Some(value) = value {
                dc(&mut xml, key, "", &value, &[], modern);
            }
        }

        Ok(Self {
            title,
            document_title,
            title_inlines,
            language,
            identifier,
            timestamp,
            xml,
        })
    }

    pub fn append_package_metadata(&mut self, meta: &Meta, cover_id: Option<&str>, modern: bool) {
        let xml = &mut self.xml;
        if let Some(id) = cover_id {
            let _ = writeln!(xml, "    <meta name=\"cover\" content=\"{id}\" />");
        }
        if modern {
            let modified = iso_time(self.timestamp);
            let _ = writeln!(
                xml,
                "    <meta property=\"dcterms:modified\">{modified}</meta>"
            );
            for (key, value) in [
                ("accessMode", "textual"),
                ("accessModeSufficient", "textual"),
                ("accessibilityFeature", "alternativeText"),
                ("accessibilityFeature", "readingOrder"),
                ("accessibilityFeature", "structuralNavigation"),
                ("accessibilityFeature", "tableOfContents"),
                ("accessibilityHazard", "none"),
            ] {
                let _ = writeln!(xml, "    <meta property=\"schema:{key}\">{value}</meta>");
            }
            if let Some(collection) = meta.get("belongs-to-collection") {
                for (index, value) in values(collection).enumerate() {
                    let id = format!("epub-collection-{}", index + 1);
                    let _ = writeln!(
                        xml,
                        "    <meta property=\"belongs-to-collection\" id=\"{id}\">{}</meta>",
                        escape(&stringify_meta(plain_value(value)))
                    );
                    for key in ["collection-type", "group-position"] {
                        if let Some(value) = field(value, key).or_else(|| text(meta, key)) {
                            let _ = writeln!(
                                xml,
                                "    <meta refines=\"#{id}\" property=\"{key}\">{}</meta>",
                                escape(&value)
                            );
                        }
                    }
                }
            }
            if let Some(MetaValue::MetaMap(settings)) = meta.get("ibooks") {
                for (key, value) in settings {
                    let _ = writeln!(
                        xml,
                        "    <meta property=\"ibooks:{}\">{}</meta>",
                        escape(key),
                        escape(&stringify_meta(value))
                    );
                }
            }
        }
    }
}

pub(super) fn text(meta: &Meta, key: &str) -> Option<String> {
    meta.get(key)
        .map(stringify_meta)
        .filter(|value| !value.is_empty())
}

fn values(value: &MetaValue) -> impl Iterator<Item = &MetaValue> {
    match value {
        MetaValue::MetaList(values) => values.as_slice(),
        other => std::slice::from_ref(other),
    }
    .iter()
}

fn field(value: &MetaValue, key: &str) -> Option<String> {
    match value {
        MetaValue::MetaMap(map) => map.get(key).map(stringify_meta),
        _ => None,
    }
}

fn plain_value(value: &MetaValue) -> &MetaValue {
    match value {
        MetaValue::MetaMap(map) => map.get("text").unwrap_or(value),
        _ => value,
    }
}

struct Entry {
    text: String,
    fields: Vec<(&'static str, String)>,
}

impl Entry {
    fn from_meta(value: &MetaValue, fields: &[&'static str]) -> Self {
        Self {
            text: stringify_meta(plain_value(value)),
            fields: fields
                .iter()
                .filter_map(|&name| {
                    field(value, input_field(name)).map(|value| {
                        let value = if name == "role" {
                            match value.as_str() {
                                "author" => "aut".into(),
                                "editor" => "edt".into(),
                                "translator" => "trl".into(),
                                "illustrator" => "ill".into(),
                                "contributor" => "ctb".into(),
                                "compiler" => "com".into(),
                                _ => value,
                            }
                        } else {
                            value
                        };
                        (name, value)
                    })
                })
                .collect(),
        }
    }

    fn from_xml(element: &XmlElement, fields: &[&'static str]) -> Self {
        Self {
            text: element_text(element),
            fields: fields
                .iter()
                .filter_map(|&name| {
                    element.attrs.iter().find_map(|(attr, value)| {
                        (attr.prefix.as_deref() == Some("opf") && attr.local == input_field(name))
                            .then(|| (name, value.clone()))
                    })
                })
                .collect(),
        }
    }
}

fn input_field(name: &str) -> &str {
    match name {
        "identifier-type" => "scheme",
        "title-type" => "type",
        _ => name,
    }
}

fn element_text(element: &XmlElement) -> String {
    element
        .children
        .iter()
        .filter_map(|node| match node {
            XmlNode::Text(text) => Some(text.as_str()),
            XmlNode::Element(_) => None,
        })
        .collect()
}

fn title_text(value: &MetaValue) -> String {
    values(value)
        .map(|value| stringify_meta(plain_value(value)))
        .collect()
}

fn epub_date(value: &str) -> String {
    let value = value.trim();
    let mut parts = value.split('-');
    let year = parts.next().unwrap_or_default();
    if year.len() == 4 && year.bytes().all(|byte| byte.is_ascii_digit()) {
        match (parts.next(), parts.next()) {
            (None, _) => return value.to_owned(),
            (Some(month), None)
                if month.len() == 2 && month.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                return value.to_owned();
            }
            _ => {}
        }
    }
    normalize_date(value).unwrap_or_default()
}

fn write_entries(
    xml: &mut String,
    key: &str,
    id: &str,
    entries: impl IntoIterator<Item = Entry>,
    modern: bool,
) {
    for (index, entry) in entries.into_iter().enumerate() {
        dc(
            xml,
            key,
            &format!("{id}-{}", index + 1),
            &entry.text,
            &entry.fields,
            modern,
        );
    }
}

fn dc(
    output: &mut String,
    key: &str,
    id: &str,
    value: &str,
    fields: &[(&str, String)],
    modern: bool,
) {
    let _ = write!(output, "    <dc:{key}");
    if !id.is_empty() {
        let _ = write!(output, " id=\"{id}\"");
    }
    if !modern {
        for (field, value) in fields.iter().filter(|(_, value)| !value.is_empty()) {
            let field = if *field == "identifier-type" {
                "scheme"
            } else {
                field
            };
            if field != "title-type" {
                let _ = write!(output, " opf:{field}=\"{}\"", escape(value));
            }
        }
    }
    let _ = writeln!(output, ">{}</dc:{key}>", escape(value));
    if modern {
        for (field, value) in fields.iter().filter(|(_, value)| !value.is_empty()) {
            let (value, scheme) = match (*field, value.as_str()) {
                ("role", role) => (role, " scheme=\"marc:relators\""),
                ("identifier-type", kind) => match kind {
                    "ISBN-13" => ("15", " scheme=\"onix:codelist5\""),
                    "ISBN-10" => ("02", " scheme=\"onix:codelist5\""),
                    "DOI" => ("06", " scheme=\"onix:codelist5\""),
                    "URN" => ("22", " scheme=\"onix:codelist5\""),
                    other => (other, ""),
                },
                (_, text) => (text, ""),
            };
            let _ = writeln!(
                output,
                "    <meta refines=\"#{id}\" property=\"{field}\"{scheme}>{}</meta>",
                escape(value)
            );
        }
    }
}

fn extra_metadata(
    options: &WriterOptions,
    context: &Context,
) -> Result<BTreeMap<String, Vec<XmlElement>>> {
    let mut entries = BTreeMap::<_, Vec<_>>::new();
    let Some(text) = &options.epub_metadata else {
        return Ok(entries);
    };
    if text.len() as u64 > context.limits.max_input_size {
        return Err(Error::App(
            "The EPUB metadata exceeds the input size limit.".into(),
        ));
    }
    let mut limits = XmlLimits::from_limits(&context.limits);
    limits.max_depth = limits.max_depth.min(crate::tree::STACK_DEPTH);
    let fragment =
        xml::parse_fragment(text, &limits, XmlOptions::default()).map_err(|err| Error::Xml {
            file: "'epub metadata'".into(),
            message: err.format_pandoc(),
        })?;
    for node in fragment.nodes {
        if let XmlNode::Element(element) = node
            && element.name.prefix.as_deref() == Some("dc")
        {
            entries
                .entry(element.name.local.clone())
                .or_default()
                .push(element);
        }
    }
    Ok(entries)
}

fn uuid(context: &mut Context) -> String {
    let mut bytes = [0u8; 16];
    for (out, value) in bytes.iter_mut().zip(context.host().random_bytes(16)) {
        *out = value;
    }
    bytes[6] = 0x40 | (bytes[6] & 0x0f);
    bytes[8] = 0x80 | (bytes[8] & 0x3f);
    let mut id = String::from("urn:uuid:");
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            id.push('-');
        }
        let _ = write!(id, "{byte:02x}");
    }
    id
}

fn iso_time(unix_seconds: i64) -> String {
    // Convert days since 1970 to a date in the Gregorian calendar.
    let days = unix_seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day = days.rem_euclid(146_097);
    let year = (day - day / 1460 + day / 36_524 - day / 146_096) / 365;
    let day = day - (365 * year + year / 4 - year / 100);
    let month = (5 * day + 2) / 153;
    let day = day - (153 * month + 2) / 5 + 1;
    let month = if month < 10 { month + 3 } else { month - 9 };
    let year = year + era * 400 + i64::from(month <= 2);
    let seconds = unix_seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}
