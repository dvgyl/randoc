use std::collections::HashMap;
use std::fmt::Write;

use randoc_archive::xml::XmlDoc;
use randoc_ast::{ListAttributes, ListNumberDelim, ListNumberStyle};
use randoc_core::Result;

use super::package::{end_tag, start_tag, write_elements};
use super::xml::{Budget, DECL, W, Xml};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Marker {
    None,
    Bullet,
    Task(bool),
    Ordered(ListAttributes),
}

pub(super) struct Numbering {
    definitions: Vec<(i64, Marker)>,
    index: HashMap<Marker, i64>,
    instances: Vec<(i64, i64, Option<i64>)>,
}

impl Numbering {
    pub(super) fn new() -> Self {
        let mut numbering = Self {
            definitions: Vec::new(),
            index: HashMap::new(),
            instances: Vec::new(),
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
            let id = match marker {
                Marker::None => 990,
                Marker::Bullet => 991,
                Marker::Task(false) => 992,
                Marker::Task(true) => 993,
                Marker::Ordered(attr) => ordered_id(attr),
            };
            self.definitions.push((id, marker));
            id
        });
        let id = 1000 + self.instances.len() as i64;
        let start = match marker {
            Marker::Ordered(attr) => Some(attr.start),
            _ => None,
        };
        self.instances.push((id, abstract_id, start));
        id
    }

    /// Writes the numbering part. Pandoc keeps the `abstractNum` elements of
    /// the reference below 990 and the `num` elements below 1000, the ranges
    /// that it does not use, and no other child of the reference root.
    pub(super) fn finish(self, reference: Option<XmlDoc>, budget: &Budget) -> Result<String> {
        let kept = |local: &'static str, attr: &'static str, limit: i64| {
            reference.iter().flat_map(move |reference| {
                reference.root.children_ns(W, local).filter(move |el| {
                    el.attr(W, attr)
                        .and_then(|id| id.parse::<i64>().ok())
                        .is_some_and(|id| id < limit)
                })
            })
        };
        let abstracts = kept("abstractNum", "abstractNumId", 990);
        let nums = kept("num", "numId", 1000);
        let mut out = budget.xml();
        out.raw(DECL);
        match &reference {
            Some(reference) => start_tag(&mut out, &reference.root),
            None => {
                let _ = write!(out, "<w:numbering xmlns:w=\"{W}\"");
            }
        }
        out.raw(">");
        write_elements(&mut out, abstracts);
        for (id, marker) in self.definitions {
            definition(&mut out, id, marker);
        }
        write_elements(&mut out, nums);
        for (id, abstract_id, start) in self.instances {
            let _ = write!(
                out,
                "<w:num w:numId=\"{id}\"><w:abstractNumId w:val=\"{abstract_id}\" />"
            );
            if let Some(start) = start {
                for level in 0..9 {
                    let _ = write!(
                        out,
                        "<w:lvlOverride w:ilvl=\"{level}\"><w:startOverride w:val=\"{start}\" /></w:lvlOverride>"
                    );
                }
            }
            out.raw("</w:num>");
        }
        match &reference {
            Some(reference) => end_tag(&mut out, &reference.root),
            None => out.raw("</w:numbering>"),
        }
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
        "<w:abstractNum w:abstractNumId=\"{id}\"><w:nsid w:val=\"{nsid:0>8}\" /><w:multiLevelType w:val=\"multilevel\" />"
    );
    for level in 0..9 {
        let _ = write!(out, "<w:lvl w:ilvl=\"{level}\">");
        match marker {
            Marker::None => out.raw(r#"<w:numFmt w:val="bullet" /><w:lvlText w:val=" " />"#),
            Marker::Bullet => {
                let marker = ["\u{f0b7}", "o", "\u{f0a7}"][level % 3];
                let _ = write!(
                    out,
                    "<w:numFmt w:val=\"bullet\" /><w:lvlText w:val=\"{marker}\" />"
                );
            }
            Marker::Task(checked) => {
                let marker = if checked { "☒" } else { "☐" };
                let _ = write!(
                    out,
                    "<w:numFmt w:val=\"bullet\" /><w:lvlText w:val=\"{marker}\" />"
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
                    "<w:start w:val=\"{}\" /><w:numFmt w:val=\"{format}\" /><w:lvlText w:val=\"{label}\" />",
                    attr.start
                );
            }
        }
        let _ = write!(
            out,
            "<w:lvlJc w:val=\"left\" /><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\" /></w:pPr>",
            720 * (level + 1)
        );
        if marker == Marker::Bullet {
            let font = ["Symbol", "Courier New", "Wingdings"][level % 3];
            let _ = write!(
                out,
                "<w:rPr><w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\" w:cs=\"{font}\" w:hint=\"default\" /></w:rPr>"
            );
        }
        out.raw("</w:lvl>");
    }
    out.raw("</w:abstractNum>");
}
