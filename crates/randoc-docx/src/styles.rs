//! OOXML `styles.xml` parsing and cycle-safe `basedOn` style inheritance.

use std::collections::{HashMap, HashSet};

use randoc_archive::xml::{XmlDoc, XmlElement};
use randoc_ast::Text;

use crate::ns::DocxNs;

const MAX_STYLE_CHAIN: usize = 64;

/// Vertical alignment of a run (`w:vertAlign`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertAlign {
    /// Normal baseline.
    Baseline,
    /// Superscript.
    Superscript,
    /// Subscript.
    Subscript,
}

/// Run-level formatting properties (`w:rPr`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunProps {
    /// Bold (`w:b`).
    pub bold: Option<bool>,
    /// Italic (`w:i`).
    pub italic: Option<bool>,
    /// Single strikeout (`w:strike`).
    pub strike: Option<bool>,
    /// Double strikeout (`w:dstrike`).
    pub dstrike: Option<bool>,
    /// Underline (`w:u`).
    pub underline: Option<bool>,
    /// Small caps (`w:smallCaps`).
    pub small_caps: Option<bool>,
    /// All caps (`w:caps`).
    pub all_caps: Option<bool>,
    /// Superscript / subscript (`w:vertAlign`).
    pub vert_align: Option<VertAlign>,
    /// Highlight (`w:highlight`).
    pub highlight: Option<bool>,
    /// Font name (`w:rFonts/@w:ascii` or `@w:hAnsi`).
    pub font: Option<Text>,
    /// Character style ID (`w:rStyle/@w:val`).
    pub style_id: Option<Text>,
    /// Resolved character style name (if `style_id` resolves).
    pub style_name: Option<Text>,
    /// Full style-name ancestry for the character style (most specific first).
    pub style_names: Vec<Text>,
}

impl RunProps {
    /// Parses a `w:rPr` element.
    pub fn parse(el: &XmlElement, ns: &DocxNs) -> Self {
        let mut p = Self::default();
        for child in el.elements() {
            if !ns.in_w(child) {
                continue;
            }
            match child.name.local.as_str() {
                "b" => p.bold = Some(ns.toggle_val(child)),
                "i" => p.italic = Some(ns.toggle_val(child)),
                "strike" => p.strike = Some(ns.toggle_val(child)),
                "dstrike" => p.dstrike = Some(ns.toggle_val(child)),
                "u" => {
                    let on = match ns.w_attr(child, "val") {
                        None => true,
                        Some(v) => !matches!(
                            v.to_ascii_lowercase().as_str(),
                            "none" | "false" | "0" | "off"
                        ),
                    };
                    p.underline = Some(on);
                }
                "smallCaps" => p.small_caps = Some(ns.toggle_val(child)),
                "caps" => p.all_caps = Some(ns.toggle_val(child)),
                "vertAlign" => {
                    if let Some(v) = ns.w_attr(child, "val") {
                        p.vert_align = match v {
                            "superscript" => Some(VertAlign::Superscript),
                            "subscript" => Some(VertAlign::Subscript),
                            "baseline" => Some(VertAlign::Baseline),
                            _ => None,
                        };
                    }
                }
                "highlight" => {
                    if let Some(v) = ns.w_attr(child, "val") {
                        p.highlight = Some(!v.eq_ignore_ascii_case("none"));
                    }
                }
                "rFonts" => {
                    if let Some(f) = ns
                        .w_attr(child, "ascii")
                        .or_else(|| ns.w_attr(child, "hAnsi"))
                        .or_else(|| ns.w_attr(child, "cs"))
                    {
                        p.font = Some(Text::from(f));
                    }
                }
                "rStyle" => {
                    if let Some(v) = ns.w_attr(child, "val") {
                        p.style_id = Some(Text::from(v));
                    }
                }
                _ => {}
            }
        }
        p
    }

    /// Returns `true` if either `strike` or `dstrike` is active.
    pub fn is_strikeout(&self) -> bool {
        self.strike == Some(true) || self.dstrike == Some(true)
    }

    /// Overlays `other` (more specific) onto `self` (base).
    pub fn overlay(&mut self, other: &RunProps) {
        if other.bold.is_some() {
            self.bold = other.bold;
        }
        if other.italic.is_some() {
            self.italic = other.italic;
        }
        if other.strike.is_some() {
            self.strike = other.strike;
        }
        if other.dstrike.is_some() {
            self.dstrike = other.dstrike;
        }
        if other.underline.is_some() {
            self.underline = other.underline;
        }
        if other.small_caps.is_some() {
            self.small_caps = other.small_caps;
        }
        if other.all_caps.is_some() {
            self.all_caps = other.all_caps;
        }
        if other.vert_align.is_some() {
            self.vert_align = other.vert_align;
        }
        if other.highlight.is_some() {
            self.highlight = other.highlight;
        }
        if other.font.is_some() {
            self.font = other.font.clone();
        }
    }
}

/// Numbering reference (`w:numPr`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumPr {
    /// `w:numId/@w:val`.
    pub num_id: Text,
    /// `w:ilvl/@w:val` (defaults to `0`).
    pub ilvl: i64,
}

/// Paragraph-level properties (`w:pPr`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParaProps {
    /// Paragraph style ID (`w:pStyle/@w:val`).
    pub style_id: Option<Text>,
    /// Whether `w:pStyle` was explicitly specified on the paragraph itself.
    pub has_explicit_style: bool,
    /// Numbering properties (`w:numPr`).
    pub num_pr: Option<NumPr>,
    /// Left / start indentation in twips (`w:ind/@w:left` or `@w:start`).
    pub indent_left: Option<i64>,
    /// First-line indentation in twips (`w:ind/@w:firstLine`).
    pub indent_first_line: Option<i64>,
    /// Hanging indentation in twips (`w:ind/@w:hanging`).
    pub indent_hanging: Option<i64>,
    /// Direct left indent set on the paragraph itself (not inherited from style).
    pub direct_indent_left: Option<i64>,
    /// Direct hanging indent set on the paragraph itself.
    pub direct_indent_hanging: Option<i64>,
    /// Effective left indent of the paragraph's resolved style (before direct `w:ind`).
    pub style_indent_left: i64,
    /// Horizontal justification (`w:jc/@w:val`).
    pub jc: Option<Text>,
    /// Direct horizontal justification set on the paragraph itself (not inherited from style).
    pub direct_jc: Option<Text>,
    /// Outline level (`w:outlineLvl/@w:val`).
    pub outline_lvl: Option<i64>,
    /// Whether the paragraph has a drop-cap frame (`w:framePr/@w:dropCap`).
    pub drop_cap: bool,
}

impl ParaProps {
    /// Parses a `w:pPr` element.
    pub fn parse(el: &XmlElement, ns: &DocxNs) -> Self {
        let mut p = Self::default();
        for child in el.elements() {
            if !ns.in_w(child) {
                continue;
            }
            match child.name.local.as_str() {
                "pStyle" => {
                    if let Some(v) = ns.w_attr(child, "val") {
                        p.style_id = Some(Text::from(v));
                        p.has_explicit_style = true;
                    }
                }
                "numPr" => {
                    let num_id = ns
                        .w_child(child, "numId")
                        .and_then(|c| ns.w_attr(c, "val"))
                        .map(Text::from);
                    let ilvl = ns
                        .w_child(child, "ilvl")
                        .and_then(|c| ns.w_attr(c, "val"))
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(0);
                    if let Some(num_id) = num_id {
                        p.num_pr = Some(NumPr { num_id, ilvl });
                    }
                }
                "ind" => {
                    if let Some(v) = ns
                        .w_attr(child, "left")
                        .or_else(|| ns.w_attr(child, "start"))
                        .and_then(|s| s.parse::<i64>().ok())
                    {
                        p.indent_left = Some(v);
                        p.direct_indent_left = Some(v);
                    }
                    if let Some(v) = ns
                        .w_attr(child, "firstLine")
                        .and_then(|s| s.parse::<i64>().ok())
                    {
                        p.indent_first_line = Some(v);
                    }
                    if let Some(v) = ns
                        .w_attr(child, "hanging")
                        .and_then(|s| s.parse::<i64>().ok())
                    {
                        p.indent_hanging = Some(v);
                        p.direct_indent_hanging = Some(v);
                    }
                }
                "jc" => {
                    if let Some(v) = ns.w_attr(child, "val") {
                        p.jc = Some(Text::from(v));
                        p.direct_jc = Some(Text::from(v));
                    }
                }
                "outlineLvl" => {
                    if let Some(v) = ns.w_attr(child, "val").and_then(|s| s.parse::<i64>().ok()) {
                        p.outline_lvl = Some(v);
                    }
                }
                "framePr" => {
                    if let Some(dc) = ns.w_attr(child, "dropCap")
                        && (dc.eq_ignore_ascii_case("drop") || dc.eq_ignore_ascii_case("margin"))
                    {
                        p.drop_cap = true;
                    }
                }
                _ => {}
            }
        }
        p
    }

    /// Overlays `other` (more specific) onto `self` (base).
    pub fn overlay(&mut self, other: &ParaProps) {
        if other.style_id.is_some() {
            self.style_id = other.style_id.clone();
        }
        if other.num_pr.is_some() {
            self.num_pr = other.num_pr.clone();
        }
        if other.indent_left.is_some() {
            self.indent_left = other.indent_left;
        }
        if other.indent_first_line.is_some() {
            self.indent_first_line = other.indent_first_line;
        }
        if other.indent_hanging.is_some() {
            self.indent_hanging = other.indent_hanging;
        }
        if other.jc.is_some() {
            self.jc = other.jc.clone();
        }
        if other.outline_lvl.is_some() {
            self.outline_lvl = other.outline_lvl;
        }
        if other.drop_cap {
            self.drop_cap = true;
        }
    }
}

/// Raw style entry from `styles.xml`.
#[derive(Debug, Clone)]
pub struct StyleDef {
    /// Human-readable style name (`w:name/@w:val`), falling back to `styleId`.
    pub name: Text,
    /// Parent style ID (`w:basedOn/@w:val`).
    pub based_on: Option<Text>,
    /// Paragraph properties defined directly on this style.
    pub p_pr: ParaProps,
    /// Run properties defined directly on this style.
    pub r_pr: RunProps,
}

/// Parsed style sheet from `word/styles.xml`.
#[derive(Debug, Clone, Default)]
pub struct StyleSheet {
    styles: HashMap<Text, StyleDef>,
    default_para_style: Option<Text>,
}

impl StyleSheet {
    /// Parses `styles.xml`. Returns an empty stylesheet on unexpected structure.
    pub fn parse(doc: &XmlDoc) -> Self {
        let ns = DocxNs::from_root(&doc.root);
        let mut sheet = Self::default();
        for el in ns.w_children(&doc.root, "style") {
            let Some(id) = ns.w_attr(el, "styleId") else {
                continue;
            };
            let kind = ns.w_attr(el, "type").unwrap_or("paragraph");
            let name = ns
                .w_child(el, "name")
                .and_then(|c| ns.w_attr(c, "val"))
                .unwrap_or(id);
            let based_on = ns
                .w_child(el, "basedOn")
                .and_then(|c| ns.w_attr(c, "val"))
                .map(Text::from);
            let p_pr = ns
                .w_child(el, "pPr")
                .map(|c| ParaProps::parse(c, &ns))
                .unwrap_or_default();
            let r_pr = ns
                .w_child(el, "rPr")
                .map(|c| RunProps::parse(c, &ns))
                .unwrap_or_default();
            let is_default = ns
                .w_attr(el, "default")
                .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "on"));
            if is_default && kind == "paragraph" && sheet.default_para_style.is_none() {
                sheet.default_para_style = Some(Text::from(id));
            }
            sheet.styles.insert(
                Text::from(id),
                StyleDef {
                    name: Text::from(name),
                    based_on,
                    p_pr,
                    r_pr,
                },
            );
        }
        sheet
    }

    /// Returns the inheritance chain for `style_id` (most specific first, cycle-safe).
    pub fn chain(&self, style_id: &str) -> Vec<&StyleDef> {
        let mut chain = Vec::new();
        let mut seen = HashSet::new();
        let mut cur = Some(style_id);
        while let Some(id) = cur {
            if chain.len() >= MAX_STYLE_CHAIN || !seen.insert(id) {
                break;
            }
            let Some(def) = self.styles.get(id) else {
                break;
            };
            chain.push(def);
            cur = def.based_on.as_deref();
        }
        chain
    }

    /// Resolves paragraph properties (base styles first, then direct `p_pr`)
    /// and the style-name ancestry (most specific first).
    ///
    /// Note: per Pandoc 3.13 behavior, paragraph-style `<w:rPr>` is NOT
    /// inherited onto runs inside the paragraph.
    pub fn resolve_para(&self, direct: &ParaProps) -> (ParaProps, Vec<Text>) {
        let style_id = direct
            .style_id
            .as_deref()
            .or(self.default_para_style.as_deref());
        let mut resolved_p = ParaProps::default();
        let mut names = Vec::new();

        if let Some(id) = style_id {
            let chain = self.chain(id);
            for def in &chain {
                names.push(def.name.clone());
            }
            for def in chain.iter().rev() {
                resolved_p.overlay(&def.p_pr);
            }
            if names.is_empty() && direct.style_id.is_some() {
                names.push(Text::from(id));
            }
        }
        let style_left =
            resolved_p.indent_left.unwrap_or(0) - resolved_p.indent_hanging.unwrap_or(0);
        resolved_p.overlay(direct);
        resolved_p.has_explicit_style = direct.has_explicit_style;
        resolved_p.direct_indent_left = direct.direct_indent_left;
        resolved_p.direct_indent_hanging = direct.direct_indent_hanging;
        resolved_p.direct_jc = direct.direct_jc.clone();
        resolved_p.style_indent_left = style_left;
        (resolved_p, names)
    }

    /// Resolves run properties from character style (`w:rStyle`) and direct
    /// `<w:rPr>`. Under `+styles`, a custom character style's own `rPr` is NOT
    /// applied. `Hyperlink` character style's `rPr` is ignored inside links.
    pub fn resolve_run(&self, direct: &RunProps, keep_styles: bool, in_link: bool) -> RunProps {
        let mut out = RunProps::default();
        if let Some(style_id) = direct.style_id.as_deref() {
            let chain = self.chain(style_id);
            let mut names: Vec<Text> = chain.iter().map(|d| d.name.clone()).collect();
            if names.is_empty() {
                names.push(Text::from(style_id));
            }
            let primary_name = names.first().cloned();
            let is_verbatim = names.iter().any(|n| is_code_char_style(n));
            let is_hyperlink_style = names.iter().any(|n| {
                n.eq_ignore_ascii_case("Hyperlink") || n.eq_ignore_ascii_case("FollowedHyperlink")
            });
            if (!keep_styles || is_verbatim) && !(in_link && is_hyperlink_style) {
                for def in chain.iter().rev() {
                    out.overlay(&def.r_pr);
                }
            }
            out.style_id = Some(Text::from(style_id));
            out.style_name = primary_name;
            out.style_names = names;
        }
        out.overlay(direct);
        out
    }
}

/// Returns `true` if a character style name denotes inline code (`Verbatim Char`).
pub fn is_code_char_style(name: &str) -> bool {
    name.eq_ignore_ascii_case("Verbatim Char") || name.eq_ignore_ascii_case("VerbatimChar")
}

/// Extracts heading level `1..=6` and optional custom class name from a
/// paragraph's style ancestry.
///
/// Pandoc treats `heading 1`..`heading 6` (and styles derived from them) as
/// `Header 1..=6`. `heading 0` clamps to level 1 with class `Heading-0`.
/// Levels 7+ remain normal paragraphs.
pub fn heading_info(names: &[Text], _outline_lvl: Option<i64>) -> Option<(i64, Option<Text>)> {
    let mut level: Option<i64> = None;
    let mut was_zero = false;
    for name in names {
        let lower = name.to_ascii_lowercase();
        let num_str = lower
            .strip_prefix("heading ")
            .or_else(|| lower.strip_prefix("heading"))
            .or_else(|| lower.strip_prefix("título "))
            .or_else(|| lower.strip_prefix("überschrift "))
            .or_else(|| lower.strip_prefix("titre "));
        if let Some(rest) = num_str
            && !rest.is_empty()
            && let Ok(n) = rest.trim().parse::<i64>()
        {
            if n == 0 {
                level = Some(1);
                was_zero = true;
                break;
            } else if (1..=6).contains(&n) {
                level = Some(n);
                break;
            } else {
                // Heading 7..9 is not a Pandoc Header.
                return None;
            }
        }
    }
    let level = level?;

    // If the primary style is a custom style (not standard `heading 1..6`), attach
    // its normalized class name to the Header.
    let class = names.first().and_then(|primary| {
        let lower = primary.to_ascii_lowercase();
        let is_standard = !was_zero
            && lower
                .strip_prefix("heading ")
                .or_else(|| lower.strip_prefix("heading"))
                .or_else(|| lower.strip_prefix("título "))
                .or_else(|| lower.strip_prefix("überschrift "))
                .or_else(|| lower.strip_prefix("titre "))
                .is_some_and(|r| r.trim().parse::<i64>().is_ok_and(|n| (1..=6).contains(&n)));
        if is_standard {
            None
        } else {
            let cls = primary.replace(' ', "-");
            if cls.is_empty() { None } else { Some(cls) }
        }
    });

    Some((level, class))
}
