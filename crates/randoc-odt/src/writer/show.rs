//! Writes parsed XML on one line, as Pandoc writes the parts that it reads
//! from a reference document: empty elements end with ` />`, and text and
//! attribute values escape `&`, `<`, `>`, `"` and `'`.

use randoc_archive::xml::{QName, XmlElement, XmlNode};

pub(super) const DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>"#;

enum Step<'a> {
    Node(&'a XmlNode),
    Close(&'a XmlElement),
}

pub(super) fn element(out: &mut String, element: &XmlElement) {
    start_tag(out, element);
    if element.children.is_empty() {
        out.push_str(" />");
        return;
    }
    out.push('>');
    let mut stack = vec![Step::Close(element)];
    stack.extend(element.children.iter().rev().map(Step::Node));
    while let Some(step) = stack.pop() {
        match step {
            Step::Node(XmlNode::Text(text)) => escape(out, text),
            Step::Node(XmlNode::Element(child)) => {
                start_tag(out, child);
                if child.children.is_empty() {
                    out.push_str(" />");
                } else {
                    out.push('>');
                    stack.push(Step::Close(child));
                    stack.extend(child.children.iter().rev().map(Step::Node));
                }
            }
            Step::Close(element) => {
                out.push_str("</");
                name(out, &element.name);
                out.push('>');
            }
        }
    }
}

/// Writes the declarations of prefixes, the attributes, and then the
/// declaration of the default namespace. Pandoc keeps the order of the
/// source, and its MathML has `display` before `xmlns`.
fn start_tag(out: &mut String, element: &XmlElement) {
    out.push('<');
    name(out, &element.name);
    let declaration = |out: &mut String, prefix: Option<&str>, uri: &str| {
        out.push_str(" xmlns");
        if let Some(prefix) = prefix {
            out.push(':');
            out.push_str(prefix);
        }
        out.push_str("=\"");
        escape(out, uri);
        out.push('"');
    };
    for (prefix, uri) in &element.ns_decls {
        if let Some(prefix) = prefix {
            declaration(out, Some(prefix), uri);
        }
    }
    for (attribute, value) in &element.attrs {
        out.push(' ');
        name(out, attribute);
        out.push_str("=\"");
        escape(out, value);
        out.push('"');
    }
    for (prefix, uri) in &element.ns_decls {
        if prefix.is_none() {
            declaration(out, None, uri);
        }
    }
}

fn name(out: &mut String, name: &QName) {
    if let Some(prefix) = &name.prefix {
        out.push_str(prefix);
        out.push(':');
    }
    out.push_str(&name.local);
}

pub(super) fn escape(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
}
