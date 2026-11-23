use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use randoc_archive::xml::{XmlDoc, XmlNode};
use randoc_ast::{ListAttributes, ListNumberDelim, ListNumberStyle};
use randoc_core::Result;

use super::package::reference_children;
use super::xml::{Budget, DECL, W, Xml};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Marker {
    None,
    Bullet,
    Task(bool),
    Ordered(ListAttributes),
}

pub(super) struct Numbering {
    used_ids: HashSet<i64>,
    used_abstract: HashSet<i64>,
    definitions: Vec<(i64, Marker)>,
    index: HashMap<Marker, i64>,
    instances: Vec<(i64, i64, Option<i64>)>,
    next: i64,
}

impl Numbering {
    pub(super) fn new(reference: Option<&XmlDoc>) -> Self {
        let ids = |tag, attr| {
            reference
                .into_iter()
                .flat_map(|doc| doc.root.elements())
                .filter(|el| el.name.local == tag)
                .filter_map(|el| el.attr(W, attr)?.parse().ok())
                .collect()
        };
        let mut numbering = Self {
            used_ids: ids("num", "numId"),
            used_abstract: ids("abstractNum", "abstractNumId"),
            definitions: Vec::new(),
            index: HashMap::new(),
            instances: Vec::new(),
            next: 1000,
        };
        numbering.insert(Marker::None);
        numbering
    }

    pub(super) fn list(&mut self, marker: Marker, budget: &Budget) -> Result<i64> {
        budget.charge(128)?;
        Ok(self.insert(marker))
    }

    fn insert(&mut self, marker: Marker) -> i64 {
        let abstract_id = *self.index.entry(marker).or_insert_with(|| {
            let mut id = match marker {
                Marker::None => 990,
                Marker::Bullet => 991,
                Marker::Task(false) => 992,
                Marker::Task(true) => 993,
                Marker::Ordered(attr) => ordered_id(attr),
            };
            if !self.used_abstract.insert(id) {
                id = 100_000;
                while !self.used_abstract.insert(id) {
                    id += 1;
                }
            }
            self.definitions.push((id, marker));
            id
        });
        while !self.used_ids.insert(self.next) {
            self.next += 1;
        }
        let id = self.next;
        self.next += 1;
        let start = match marker {
            Marker::Ordered(attr) => Some(attr.start),
            _ => None,
        };
        self.instances.push((id, abstract_id, start));
        id
    }

    pub(super) fn finish(self, reference: Option<XmlDoc>, budget: &Budget) -> Result<String> {
        let mut out = budget.xml();
        out.raw(DECL);
        let _ = write!(out, "<w:numbering xmlns:w=\"{W}\">");
        let mut numbering_cleanup = Vec::new();
        let mut instances = Vec::new();
        let mut namespaces = Vec::new();
        if let Some(reference) = reference {
            let mut root = reference.root;
            for node in std::mem::take(&mut root.children) {
                match &node {
                    XmlNode::Element(el) if el.name.local == "num" => instances.push(node),
                    XmlNode::Element(el) if el.name.local == "numIdMacAtCleanup" => {
                        numbering_cleanup.push(node)
                    }
                    _ => root.children.push(node),
                }
            }
            namespaces.clone_from(&root.ns_decls);
            reference_children(&mut out, root);
        }
        for (id, marker) in self.definitions {
            definition(&mut out, id, marker);
        }
        super::package::write_children(&mut out, instances, &namespaces);
        for (id, abstract_id, start) in self.instances {
            let _ = write!(
                out,
                "<w:num w:numId=\"{id}\"><w:abstractNumId w:val=\"{abstract_id}\"/>"
            );
            if let Some(start) = start {
                for level in 0..9 {
                    let _ = write!(
                        out,
                        "<w:lvlOverride w:ilvl=\"{level}\"><w:startOverride w:val=\"{start}\"/></w:lvlOverride>"
                    );
                }
            }
            out.raw("</w:num>");
        }
        super::package::write_children(&mut out, numbering_cleanup, &namespaces);
        out.raw("</w:numbering>");
        out.into_string()
    }
}

fn ordered_id(attr: ListAttributes) -> i64 {
    let style = match attr.style {
        ListNumberStyle::DefaultStyle => 2,
        ListNumberStyle::Example => 3,
        ListNumberStyle::Decimal => 4,
        ListNumberStyle::LowerRoman => 5,
        ListNumberStyle::UpperRoman => 6,
        ListNumberStyle::LowerAlpha => 7,
        ListNumberStyle::UpperAlpha => 8,
    };
    let delim = match attr.delim {
        ListNumberDelim::DefaultDelim | ListNumberDelim::Period => 1,
        ListNumberDelim::OneParen => 2,
        ListNumberDelim::TwoParens => 3,
    };
    format!("99{style}{delim}{}", attr.start)
        .parse()
        .unwrap_or(100_000)
}

fn definition(out: &mut Xml, id: i64, marker: Marker) {
    let nsid = format!("A{id}");
    let _ = write!(
        out,
        "<w:abstractNum w:abstractNumId=\"{id}\"><w:nsid w:val=\"{nsid:0>8}\"/><w:multiLevelType w:val=\"multilevel\"/>"
    );
    for level in 0..9 {
        let _ = write!(out, "<w:lvl w:ilvl=\"{level}\">");
        match marker {
            Marker::None => out.raw(r#"<w:numFmt w:val="bullet"/><w:lvlText w:val=" "/>"#),
            Marker::Bullet => {
                let marker = ["\u{f0b7}", "o", "\u{f0a7}"][level % 3];
                let _ = write!(
                    out,
                    "<w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"{marker}\"/>"
                );
            }
            Marker::Task(checked) => {
                let marker = if checked { "☒" } else { "☐" };
                let _ = write!(
                    out,
                    "<w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"{marker}\"/>"
                );
            }
            Marker::Ordered(attr) => {
                let format = match attr.style {
                    ListNumberStyle::LowerRoman => "lowerRoman",
                    ListNumberStyle::UpperRoman => "upperRoman",
                    ListNumberStyle::LowerAlpha => "lowerLetter",
                    ListNumberStyle::UpperAlpha => "upperLetter",
                    _ => "decimal",
                };
                let number = level + 1;
                let label = match attr.delim {
                    ListNumberDelim::OneParen => format!("%{number})"),
                    ListNumberDelim::TwoParens => format!("(%{number})"),
                    _ => format!("%{number}."),
                };
                let _ = write!(
                    out,
                    "<w:start w:val=\"{}\"/><w:numFmt w:val=\"{format}\"/><w:lvlText w:val=\"{label}\"/>",
                    attr.start
                );
            }
        }
        let _ = write!(
            out,
            "<w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr>",
            720 * (level + 1)
        );
        if marker == Marker::Bullet {
            let font = ["Symbol", "Courier New", "Wingdings"][level % 3];
            let _ = write!(
                out,
                "<w:rPr><w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\" w:cs=\"{font}\" w:hint=\"default\"/></w:rPr>"
            );
        }
        out.raw("</w:lvl>");
    }
    out.raw("</w:abstractNum>");
}
