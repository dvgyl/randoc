//! OOXML `numbering.xml` parsing for bullet, task, and ordered lists.

use std::collections::HashMap;

use randoc_archive::xml::{XmlDoc, XmlElement};
use randoc_ast::{ListNumberDelim, ListNumberStyle, Text};

use crate::ns::DocxNs;
use crate::symbol::decode_symbol;

/// Definition of a single numbering level (`w:lvl`).
#[derive(Debug, Clone)]
pub struct LevelDef {
    /// Starting number (`w:start/@w:val`, default `1`).
    pub start: i64,
    /// Number format (`w:numFmt/@w:val`, e.g. `bullet`, `decimal`, `lowerRoman`).
    pub num_fmt: Text,
    /// Level format text (`w:lvlText/@w:val`, e.g. `%1.`, `(%1)`, `%1)`).
    pub lvl_text: Text,
    /// Optional font on the level marker (`w:rPr/w:rFonts`).
    pub font: Option<Text>,
    /// Optional paragraph style linked to this level (`w:pStyle/@w:val`).
    pub p_style: Option<Text>,
}

/// Concrete list marker kind resolved from `numbering.xml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListKind {
    /// Unordered bullet list (with optional task-list checkbox prefix `☐` / `☒`).
    Bullet {
        /// Optional task-list checkbox string (`☐` or `☒`).
        checkbox: Option<&'static str>,
    },
    /// Ordered list with `(start, style, delim)`.
    Ordered {
        /// Starting number.
        start: i64,
        /// Numbering style.
        style: ListNumberStyle,
        /// Delimiter style.
        delim: ListNumberDelim,
    },
}

#[derive(Debug, Clone)]
struct NumOverride {
    start_override: Option<i64>,
    lvl_override: Option<LevelDef>,
}

#[derive(Debug, Clone)]
struct NumDef {
    abstract_num_id: Text,
    overrides: HashMap<i64, NumOverride>,
}

/// Parsed `word/numbering.xml` definitions.
#[derive(Debug, Clone, Default)]
pub struct Numbering {
    abstract_nums: HashMap<Text, HashMap<i64, LevelDef>>,
    nums: HashMap<Text, NumDef>,
    /// Reverse lookup from `w:pStyle` to `(numId, ilvl)` when a style is linked in numbering.
    style_to_num: HashMap<Text, (Text, i64)>,
}

impl Numbering {
    /// Parses `numbering.xml`.
    pub fn parse(doc: &XmlDoc) -> Self {
        let ns = DocxNs::from_root(&doc.root);
        let mut out = Self::default();

        for abs in ns.w_children(&doc.root, "abstractNum") {
            let Some(abs_id) = ns.w_attr(abs, "abstractNumId") else {
                continue;
            };
            let mut levels = HashMap::new();
            for lvl in ns.w_children(abs, "lvl") {
                if let Some((ilvl, def)) = parse_lvl(lvl, &ns) {
                    levels.insert(ilvl, def);
                }
            }
            out.abstract_nums.insert(Text::from(abs_id), levels);
        }

        for num in ns.w_children(&doc.root, "num") {
            let Some(num_id) = ns.w_attr(num, "numId") else {
                continue;
            };
            let Some(abs_id) = ns
                .w_child(num, "abstractNumId")
                .and_then(|c| ns.w_attr(c, "val"))
            else {
                continue;
            };
            let mut overrides = HashMap::new();
            for ov in ns.w_children(num, "lvlOverride") {
                let Some(ilvl) = ns.w_attr(ov, "ilvl").and_then(|s| s.parse::<i64>().ok()) else {
                    continue;
                };
                let start_override = ns
                    .w_child(ov, "startOverride")
                    .and_then(|c| ns.w_attr(c, "val"))
                    .and_then(|s| s.parse::<i64>().ok());
                let lvl_override = ns
                    .w_child(ov, "lvl")
                    .and_then(|c| parse_lvl(c, &ns))
                    .map(|(_, d)| d);
                overrides.insert(
                    ilvl,
                    NumOverride {
                        start_override,
                        lvl_override,
                    },
                );
            }
            if let Some(levels) = out.abstract_nums.get(abs_id) {
                for (&ilvl, lvl) in levels {
                    if let Some(ps) = &lvl.p_style {
                        out.style_to_num
                            .entry(ps.clone())
                            .or_insert_with(|| (Text::from(num_id), ilvl));
                    }
                }
            }
            out.nums.insert(
                Text::from(num_id),
                NumDef {
                    abstract_num_id: Text::from(abs_id),
                    overrides,
                },
            );
        }
        out
    }

    /// Looks up a `(numId, ilvl)` linked to a paragraph style via `w:lvl/w:pStyle`.
    pub fn by_style_id(&self, style_id: &str) -> Option<(&str, i64)> {
        self.style_to_num
            .get(style_id)
            .map(|(n, i)| (n.as_str(), *i))
    }

    /// Resolves `(num_id, ilvl)` to a [`ListKind`]. Returns `None` if `num_id`
    /// is `"0"` (explicitly no list), unknown, or has `numFmt="none"`.
    pub fn resolve(&self, num_id: &str, ilvl: i64) -> Option<ListKind> {
        if num_id == "0" {
            return None;
        }
        let num = self.nums.get(num_id)?;
        let abs = self.abstract_nums.get(&num.abstract_num_id)?;
        let ov = num.overrides.get(&ilvl);
        let lvl = ov
            .and_then(|o| o.lvl_override.as_ref())
            .or_else(|| abs.get(&ilvl))
            .or_else(|| (0..=8).contains(&ilvl).then(|| abs.get(&0)).flatten())?;

        let start = ov.and_then(|o| o.start_override).unwrap_or(lvl.start);
        match lvl.num_fmt.as_str() {
            "none" => None,
            "bullet" => {
                // Whitespace-only bullet markers (e.g. `lvlText=" "`) represent
                // dummy/continuation items inside a list rather than new bullets.
                if lvl.lvl_text.trim().is_empty() {
                    return None;
                }
                let checkbox = detect_checkbox(&lvl.lvl_text, lvl.font.as_deref());
                Some(ListKind::Bullet { checkbox })
            }
            fmt => {
                let style = match fmt {
                    "decimal" | "decimalZero" => ListNumberStyle::Decimal,
                    "lowerLetter" => ListNumberStyle::LowerAlpha,
                    "upperLetter" => ListNumberStyle::UpperAlpha,
                    "lowerRoman" => ListNumberStyle::LowerRoman,
                    "upperRoman" => ListNumberStyle::UpperRoman,
                    _ => ListNumberStyle::DefaultStyle,
                };
                let delim = parse_delimiter(&lvl.lvl_text);
                Some(ListKind::Ordered {
                    start,
                    style,
                    delim,
                })
            }
        }
    }

    /// Returns `true` if `(num_id, ilvl)` has an explicit `startOverride`.
    pub fn has_start_override(&self, num_id: &str, ilvl: i64) -> bool {
        self.nums
            .get(num_id)
            .and_then(|n| n.overrides.get(&ilvl))
            .is_some_and(|o| o.start_override.is_some())
    }
}

fn parse_lvl(el: &XmlElement, ns: &DocxNs) -> Option<(i64, LevelDef)> {
    let ilvl = ns.w_attr(el, "ilvl")?.parse::<i64>().ok()?;
    let start = ns
        .w_child(el, "start")
        .and_then(|c| ns.w_attr(c, "val"))
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(1);
    let num_fmt = ns
        .w_child(el, "numFmt")
        .and_then(|c| ns.w_attr(c, "val"))
        .unwrap_or("decimal");
    let lvl_text = ns
        .w_child(el, "lvlText")
        .and_then(|c| ns.w_attr(c, "val"))
        .unwrap_or("");
    let font = ns
        .w_child(el, "rPr")
        .and_then(|r| ns.w_child(r, "rFonts"))
        .and_then(|f| {
            ns.w_attr(f, "ascii")
                .or_else(|| ns.w_attr(f, "hAnsi"))
                .or_else(|| ns.w_attr(f, "cs"))
        })
        .map(Text::from);
    let p_style = ns
        .w_child(el, "pStyle")
        .and_then(|c| ns.w_attr(c, "val"))
        .map(Text::from);
    Some((
        ilvl,
        LevelDef {
            start,
            num_fmt: Text::from(num_fmt),
            lvl_text: Text::from(lvl_text),
            font,
            p_style,
        },
    ))
}

fn detect_checkbox(lvl_text: &str, font: Option<&str>) -> Option<&'static str> {
    let ch = lvl_text.chars().next()?;
    let decoded = if (0xF000..=0xF0FF).contains(&u32::from(ch)) {
        decode_symbol(font, &format!("{:X}", u32::from(ch)))
            .and_then(|s| s.chars().next())
            .unwrap_or(ch)
    } else {
        ch
    };
    match decoded {
        '\u{2610}' | '\u{25A1}' => Some("\u{2610}"), // ☐
        '\u{2611}' | '\u{2612}' | '\u{2713}' | '\u{2714}' => Some("\u{2612}"), // ☒
        _ => None,
    }
}

fn parse_delimiter(lvl_text: &str) -> ListNumberDelim {
    let trimmed = lvl_text.trim();
    if trimmed.matches('%').count() > 1 {
        return ListNumberDelim::DefaultDelim;
    }
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        ListNumberDelim::TwoParens
    } else if trimmed.ends_with(')') {
        ListNumberDelim::OneParen
    } else if trimmed.ends_with('.') {
        ListNumberDelim::Period
    } else {
        ListNumberDelim::DefaultDelim
    }
}
