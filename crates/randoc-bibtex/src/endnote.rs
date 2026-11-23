use randoc_archive::xml::{self, XmlElement, XmlError, XmlLimits, XmlNode, XmlOptions};
use randoc_ast::{Inline, MetaValue, Pandoc};
use randoc_core::{Context, Error, ReaderOptions, Result, Sources};

use crate::{Fields, charge, document, names, string};

pub(super) fn read(sources: &Sources, _: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    let mut remaining = context.limits.max_input_size;
    for (index, source) in sources.0.iter().enumerate() {
        charge(&mut remaining, source.text.len())?;
        charge(&mut remaining, usize::from(index > 0))?;
        charge(
            &mut remaining,
            usize::from(!source.text.is_empty() && !source.text.ends_with('\n')),
        )?;
    }

    // Check the root after parsing all top-level nodes. This also detects text
    // before or after the root with the same error as a second root.
    let input = sources.concat();
    let fragment = xml::parse_fragment(
        &input,
        &XmlLimits::from_limits(&context.limits),
        XmlOptions::default(),
    )
    .map_err(|error| invalid_xml(xml_message(&error)))?;
    let mut roots = fragment
        .nodes
        .into_iter()
        .filter(|node| !matches!(node, XmlNode::Text(text) if text.trim().is_empty()));
    let mut root = None;
    let mut has_extra_content = false;
    for node in &mut roots {
        match node {
            XmlNode::Element(element) if root.is_none() => root = Some(element),
            XmlNode::Element(element) => {
                has_extra_content = true;
                drop_nodes(element.children);
            }
            XmlNode::Text(_) => has_extra_content = true,
        }
    }
    let root = root.ok_or_else(|| invalid_xml("XML document has no root element".into()))?;
    // Consume the DOM with an explicit stack.
    let mut pending = vec![XmlNode::Element(root)];
    let mut references = Vec::new();
    while let Some(element) = pop_element(&mut pending) {
        if has_name(&element, "record") {
            if !has_extra_content {
                references.push(reference(&element));
            }
            drop_nodes(element.children);
        } else {
            pending.extend(element.children.into_iter().rev());
        }
    }
    if has_extra_content {
        return Err(invalid_xml(
            "unexpected content after the root XML element".into(),
        ));
    }
    Ok(document(references))
}

fn pop_element(nodes: &mut Vec<XmlNode>) -> Option<XmlElement> {
    while let Some(node) = nodes.pop() {
        if let XmlNode::Element(element) = node {
            return Some(element);
        }
    }
    None
}

fn drop_nodes(mut nodes: Vec<XmlNode>) {
    while let Some(element) = pop_element(&mut nodes) {
        nodes.extend(element.children);
    }
}

fn invalid_xml(message: String) -> Error {
    Error::Xml {
        file: "'EndNote references'".into(),
        message,
    }
}

fn xml_message(error: &XmlError) -> String {
    if let XmlError::Syntax { detail, .. } = error
        && let Some(name) = detail
            .strip_prefix("unclosed element <")
            .and_then(|rest| rest.strip_suffix('>'))
    {
        return format!("Missing close tag for {name}");
    }
    error.format_pandoc()
}

fn has_name(element: &XmlElement, name: &str) -> bool {
    element.name.local == name
}

fn children<'a>(element: &'a XmlElement, name: &'a str) -> impl Iterator<Item = &'a XmlElement> {
    element.children.iter().filter_map(move |node| match node {
        XmlNode::Element(child) if has_name(child, name) => Some(child),
        _ => None,
    })
}

fn text(element: &XmlElement) -> String {
    let mut value = String::new();
    let mut pending = vec![element.children.iter()];
    while let Some(children) = pending.last_mut() {
        match children.next() {
            Some(XmlNode::Text(text)) => value.push_str(text),
            Some(XmlNode::Element(child)) => pending.push(child.children.iter()),
            None => {
                pending.pop();
            }
        }
    }
    value
}

fn reference(record: &XmlElement) -> MetaValue {
    let mut fields = Fields::new();
    let id = children(record, "rec-number")
        .next()
        .map(text)
        .unwrap_or_default();
    string(&mut fields, "id", id.trim());
    let kind = children(record, "ref-type")
        .next()
        .and_then(|element| element.attrs.iter().find(|(key, _)| key.local == "name"))
        .map_or("", |(_, value)| value.as_str());
    string(&mut fields, "type", reference_type(kind));

    let authors: Vec<_> = children(record, "contributors")
        .flat_map(|group| children(group, "authors"))
        .flat_map(|authors| children(authors, "author"))
        .map(|author| names::ris(&text(author)))
        .collect();
    if !authors.is_empty() {
        fields.insert("author".into(), MetaValue::MetaList(authors));
    }
    for dates in children(record, "dates") {
        for year in children(dates, "year") {
            string(&mut fields, "issued", issued(text(year).trim()));
        }
    }
    for titles in children(record, "titles") {
        for (tag, key) in [("title", "title"), ("secondary-title", "container-title")] {
            inline_fields(&mut fields, titles, tag, key);
        }
    }
    for (tag, key) in [
        ("abstract", "abstract"),
        ("isbn", "isbn"),
        ("number", "number"),
        ("orig-pub", "original-publisher"),
        ("pages", "pages"),
        ("pub-location", "publisher-place"),
        ("publisher", "publisher"),
        ("volume", "volume"),
    ] {
        inline_fields(&mut fields, record, tag, key);
    }
    MetaValue::MetaMap(fields)
}

fn inline_fields(fields: &mut Fields, parent: &XmlElement, tag: &str, key: &str) {
    if let Some(element) = children(parent, tag).last() {
        let value = text(element);
        let value = value.trim_matches([' ', '\t', '\n', '\r']);
        let mut parts = Vec::new();
        let mut start = 0;
        let mut separator = None;
        for (pos, ch) in value.char_indices() {
            if matches!(ch, ' ' | '\t' | '\n' | '\r') {
                if separator.is_none() && start < pos {
                    parts.push(Inline::Str(value[start..pos].into()));
                }
                if ch == '\n' || separator.is_none() {
                    separator = Some(if ch == '\n' {
                        Inline::SoftBreak
                    } else {
                        Inline::Space
                    });
                }
                start = pos + 1;
            } else if let Some(separator) = separator.take() {
                parts.push(separator);
            }
        }
        if start < value.len() {
            parts.push(Inline::Str(value[start..].into()));
        }
        fields.insert(key.into(), MetaValue::MetaInlines(parts));
    }
}

fn issued(year: &str) -> String {
    if year.starts_with('+') {
        return String::new();
    }
    match year.parse::<i64>() {
        Ok(1..=9999) => format!("{:0>4}", year.trim_start_matches('0')),
        Ok(-1) => "-001".into(),
        Ok(year @ -10000..=-2) => format!("-{:04}", -(year + 1)),
        Ok(year) if year > 9999 => format!("y{year}"),
        Ok(year) if year < -10000 => format!("y{}", year + 1),
        _ => String::new(),
    }
}

fn reference_type(name: &str) -> &'static str {
    match name {
        "Journal Article" => "article-journal",
        "Book" | "Edited Book" | "Electronic Book" | "Dictionary" | "Encyclopedia" => "book",
        "Book Section" => "chapter",
        "Conference Proceedings" => "periodical",
        "Conference Paper" | "Electronic Article" => "article",
        "Magazine Article" => "article-magazine",
        "Newspaper Article" => "article-newspaper",
        "Report" => "report",
        "Thesis" => "thesis",
        "Patent" => "patent",
        "Manuscript" => "manuscript",
        "Map" => "map",
        "Dataset" => "dataset",
        "Unpublished Work" => "unpublished",
        "Web Page" => "webpage",
        "Personal Communication" => "personal_communication",
        "Film or Broadcast" => "motion_picture",
        "Artwork" => "graphic",
        "Blog" => "post-weblog",
        _ => "document",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn read_xml(input: &str, context: &mut Context) -> Result<Pandoc> {
        read(
            &Sources::from_text(input),
            &ReaderOptions::default(),
            context,
        )
    }

    #[test]
    fn requires_one_root_element() {
        let mut context = Context::pure();
        for (input, message) in [
            ("", "XML document has no root element"),
            ("words", "XML document has no root element"),
            (
                "words<record/>",
                "unexpected content after the root XML element",
            ),
            (
                "<record/><record/>",
                "unexpected content after the root XML element",
            ),
            ("<record>", "Missing close tag for record"),
        ] {
            let error = read_xml(input, &mut context).unwrap_err();
            assert_eq!(error.exit_code(), 44);
            assert_eq!(
                error.to_string(),
                format!("Invalid XML in 'EndNote references':\n{message}")
            );
        }
    }

    #[test]
    fn rejects_external_xml_entities() {
        let input = r#"<!DOCTYPE record [<!ENTITY x SYSTEM "file:///unused.xml">]><record><volume>&x;</volume></record>"#;
        assert!(matches!(
            read_xml(input, &mut Context::pure()),
            Err(Error::Xml { .. })
        ));
    }

    #[test]
    fn nested_xml_fits_on_a_two_mebibyte_stack() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let mut context = Context::pure();
                let depth = context.limits.max_nesting_depth;
                let input = format!(
                    "{}<record/>{}",
                    "<x>".repeat(depth - 1),
                    "</x>".repeat(depth - 1)
                );
                assert!(read_xml(&input, &mut context).is_ok());
                context.limits.max_nesting_depth -= 1;
                assert!(matches!(
                    read_xml(&input, &mut context),
                    Err(Error::Xml { .. })
                ));
                context.limits.max_input_size = 8;
                assert!(matches!(read_xml(&input, &mut context), Err(Error::App(_))));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
