//! ODF style table: paragraph styles, text styles, and list styles.
//!
//! Resolves `style:parent-style-name` chains with explicit cycle detection and
//! depth bounds so hostile style graphs cannot loop or overflow the stack.

use std::collections::{BTreeMap, HashMap, HashSet};

use randoc_archive::xml::XmlElement;
use randoc_ast::{ListNumberDelim, ListNumberStyle};

use crate::ns::{FO, OFFICE, STYLE, TEXT, is_el};

/// Vertical script position from `style:text-position`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalPos {
    /// Normal baseline.
    Normal,
    /// Superscript.
    Super,
    /// Subscript.
    Sub,
}

/// Text formatting properties parsed from `<style:text-properties>`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextProps {
    /// Bold (`fo:font-weight="bold"` or numeric >= 600).
    pub bold: bool,
    /// Italic (`fo:font-style="italic"` or `"oblique"`).
    pub italic: bool,
    /// Underline (`style:text-underline-style != "none"`).
    pub underline: bool,
    /// Strikeout (`style:text-line-through-style != "none"`).
    pub strikeout: bool,
    /// Superscript or subscript (`style:text-position`).
    pub vertical: Option<VerticalPos>,
}

/// One `<style:style>` definition.
#[derive(Debug, Clone, Default)]
pub struct StyleDef {
    /// Parent style name (`style:parent-style-name`).
    pub parent: Option<String>,
    /// The style names a list style. Its margin gives the list indentation.
    pub is_list_paragraph: bool,
    /// Direct text properties declared on this style.
    pub text: TextProps,
    /// Left margin in millimeters, if explicitly specified on this style.
    pub margin_left_mm: Option<f64>,
    /// Whether `fo:margin-left` was a percentage value (treated as blockquote when > 0%).
    pub margin_left_pct: Option<f64>,
}

/// One level of a `<text:list-style>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListLevelKind {
    /// Bullet list (`<text:list-level-style-bullet>` or image).
    Bullet,
    /// Numbered list (`<text:list-level-style-number>`).
    Number {
        /// Starting value (`text:start-value`, default 1).
        start: i64,
        /// Numbering style (`style:num-format`).
        style: ListNumberStyle,
        /// Delimiter derived from `style:num-prefix` and `style:num-suffix`.
        delim: ListNumberDelim,
    },
}

/// A `<text:list-style>` mapping 1-based `text:level` to its level kind.
#[derive(Debug, Clone, Default)]
pub struct ListStyle {
    /// Level definitions keyed by 1-based level.
    pub levels: BTreeMap<usize, ListLevelKind>,
}

/// Collected named and automatic styles for an ODF document.
#[derive(Debug, Clone, Default)]
pub struct StyleTable {
    /// Paragraph and text styles keyed by `style:name`.
    styles: HashMap<String, StyleDef>,
    /// List styles keyed by `style:name`.
    list_styles: HashMap<String, ListStyle>,
    paragraph_flags: HashMap<String, ParagraphFlags>,
}

/// Left margin threshold in millimeters for BlockQuote detection (oracle-verified: 5.5 mm).
const BLOCKQUOTE_MARGIN_MM: f64 = 5.5;

#[derive(Debug, Clone, Copy, Default)]
pub struct ParagraphFlags {
    pub preformatted: bool,
    pub blockquote: bool,
}

impl StyleTable {
    /// Loads named and automatic styles, then resolves paragraph properties.
    pub fn from_roots<'a>(
        roots: impl IntoIterator<Item = &'a XmlElement>,
        max_depth: usize,
    ) -> Self {
        let mut table = Self::default();
        for root in roots {
            for child in root.elements() {
                if is_el(child, OFFICE, "styles") || is_el(child, OFFICE, "automatic-styles") {
                    table.load_style_container(child);
                }
            }
        }
        table.paragraph_flags = table
            .styles
            .keys()
            .map(|name| {
                (
                    name.clone(),
                    table.resolve_paragraph_flags(name, max_depth.max(1)),
                )
            })
            .collect();
        table
    }

    fn load_style_container(&mut self, container: &XmlElement) {
        for child in container.elements() {
            if is_el(child, STYLE, "style") {
                if let Some(name) = child.attr(STYLE, "name") {
                    let def = parse_style_def(child);
                    self.styles.insert(name.to_owned(), def);
                }
            } else if is_el(child, TEXT, "list-style")
                && let Some(name) = child.attr(STYLE, "name")
            {
                let ls = parse_list_style(child);
                self.list_styles.insert(name.to_owned(), ls);
            }
        }
    }

    /// Returns the direct text formatting properties of `name` (Pandoc's ODT
    /// reader does not inherit `style:text-properties` across parent styles,
    /// except when checking paragraph/span direct properties).
    pub fn direct_text_props(&self, name: &str) -> TextProps {
        self.styles.get(name).map(|s| s.text).unwrap_or_default()
    }

    /// Returns `true` if `name` is an inline code style (`Source_20_Text` or `Source_Text`).
    pub fn is_inline_code(&self, name: &str) -> bool {
        matches!(name, "Source_20_Text" | "Source_Text")
    }

    /// Returns the cached properties of a paragraph style.
    pub fn paragraph_flags(&self, name: &str) -> ParagraphFlags {
        self.paragraph_flags
            .get(name)
            .copied()
            .unwrap_or_else(|| ParagraphFlags {
                preformatted: is_preformatted(name),
                blockquote: false,
            })
    }

    fn resolve_paragraph_flags(&self, name: &str, max_depth: usize) -> ParagraphFlags {
        let mut flags = ParagraphFlags::default();
        let mut blockquote = None;
        let mut seen = HashSet::new();
        let mut current = name;
        for _ in 0..=max_depth {
            flags.preformatted |= is_preformatted(current);
            if !seen.insert(current) {
                break;
            }
            let Some(style) = self.styles.get(current) else {
                break;
            };
            if blockquote.is_none() {
                blockquote = if style.is_list_paragraph {
                    Some(false)
                } else if let Some(mm) = style.margin_left_mm {
                    Some(mm >= BLOCKQUOTE_MARGIN_MM)
                } else {
                    style.margin_left_pct.map(|pct| pct > 0.0)
                };
            }
            let Some(parent) = style.parent.as_deref() else {
                break;
            };
            current = parent;
        }
        flags.blockquote = blockquote.unwrap_or(false);
        flags
    }

    /// Looks up the list level definition for `list_style_name` at 1-based `level`.
    pub fn list_level(&self, list_style_name: &str, level: usize) -> Option<ListLevelKind> {
        let ls = self.list_styles.get(list_style_name)?;
        if let Some(kind) = ls.levels.get(&level) {
            return Some(*kind);
        }
        // Fallback: use the nearest defined level <= level, or level 1.
        ls.levels
            .range(..=level)
            .next_back()
            .or_else(|| ls.levels.iter().next())
            .map(|(_, k)| *k)
    }
}

fn is_preformatted(name: &str) -> bool {
    matches!(name, "Preformatted_20_Text" | "Preformatted_Text")
}

fn parse_style_def(el: &XmlElement) -> StyleDef {
    let mut def = StyleDef {
        parent: el.attr(STYLE, "parent-style-name").map(str::to_owned),
        is_list_paragraph: el
            .attr(STYLE, "list-style-name")
            .is_some_and(|name| !name.is_empty()),
        text: TextProps::default(),
        margin_left_mm: None,
        margin_left_pct: None,
    };

    for child in el.elements() {
        if is_el(child, STYLE, "text-properties") {
            parse_text_props(child, &mut def.text);
        } else if is_el(child, STYLE, "paragraph-properties")
            && let Some(ml) = child.attr(FO, "margin-left")
        {
            if let Some(pct) = ml.trim().strip_suffix('%') {
                if let Ok(v) = pct.trim().parse::<f64>()
                    && v.is_finite()
                {
                    def.margin_left_pct = Some(v);
                }
            } else if let Some(mm) = parse_length_mm(ml) {
                def.margin_left_mm = Some(mm);
            }
        }
    }
    def
}

fn parse_text_props(el: &XmlElement, props: &mut TextProps) {
    if let Some(weight) = el.attr(FO, "font-weight") {
        let w = weight.trim();
        if w.eq_ignore_ascii_case("bold") {
            props.bold = true;
        } else if let Ok(num) = w.parse::<u32>()
            && num >= 600
        {
            props.bold = true;
        }
    }
    if let Some(style) = el.attr(FO, "font-style") {
        let s = style.trim();
        if s.eq_ignore_ascii_case("italic") || s.eq_ignore_ascii_case("oblique") {
            props.italic = true;
        }
    }
    if let Some(ul) = el.attr(STYLE, "text-underline-style")
        && !ul.trim().eq_ignore_ascii_case("none")
        && !ul.trim().is_empty()
    {
        props.underline = true;
    }
    if let Some(lt) = el.attr(STYLE, "text-line-through-style")
        && !lt.trim().eq_ignore_ascii_case("none")
        && !lt.trim().is_empty()
    {
        props.strikeout = true;
    }
    if let Some(pos) = el.attr(STYLE, "text-position") {
        let first = pos.split_whitespace().next().unwrap_or("");
        if first.eq_ignore_ascii_case("super") {
            props.vertical = Some(VerticalPos::Super);
        } else if first.eq_ignore_ascii_case("sub") {
            props.vertical = Some(VerticalPos::Sub);
        } else if let Some(num_str) = first.strip_suffix('%')
            && let Ok(v) = num_str.parse::<f64>()
            && v.is_finite()
        {
            if v > 0.0 {
                props.vertical = Some(VerticalPos::Super);
            } else if v < 0.0 {
                props.vertical = Some(VerticalPos::Sub);
            } else {
                props.vertical = Some(VerticalPos::Normal);
            }
        }
    }
}

fn parse_list_style(el: &XmlElement) -> ListStyle {
    let mut ls = ListStyle::default();
    for child in el.elements() {
        let Some(level) = child
            .attr(TEXT, "level")
            .and_then(|s| s.trim().parse::<usize>().ok())
        else {
            continue;
        };
        if is_el(child, TEXT, "list-level-style-bullet")
            || is_el(child, TEXT, "list-level-style-image")
        {
            ls.levels.insert(level, ListLevelKind::Bullet);
        } else if is_el(child, TEXT, "list-level-style-number") {
            let start = child
                .attr(TEXT, "start-value")
                .and_then(|s| s.trim().parse::<i64>().ok())
                .unwrap_or(1);
            let style = match child.attr(STYLE, "num-format").map(str::trim) {
                Some("1") => ListNumberStyle::Decimal,
                Some("a") => ListNumberStyle::LowerAlpha,
                Some("A") => ListNumberStyle::UpperAlpha,
                Some("i") => ListNumberStyle::LowerRoman,
                Some("I") => ListNumberStyle::UpperRoman,
                _ => ListNumberStyle::DefaultStyle,
            };
            let prefix = child.attr(STYLE, "num-prefix").unwrap_or("");
            let suffix = child.attr(STYLE, "num-suffix").unwrap_or("");
            let delim = match (prefix, suffix) {
                ("", ".") => ListNumberDelim::Period,
                ("", ")") => ListNumberDelim::OneParen,
                ("(", ")") => ListNumberDelim::TwoParens,
                _ => ListNumberDelim::DefaultDelim,
            };
            ls.levels.insert(
                level,
                ListLevelKind::Number {
                    start,
                    style,
                    delim,
                },
            );
        }
    }
    ls
}

/// Parses an ODF/CSS length string (e.g. `"1cm"`, `"0.5in"`, `"15.9pt"`, `"5.5mm"`)
/// into millimeters.
pub fn parse_length_mm(raw: &str) -> Option<f64> {
    let s = raw.trim();
    let (num_str, factor) = if let Some(n) = s.strip_suffix("mm") {
        (n, 1.0)
    } else if let Some(n) = s.strip_suffix("cm") {
        (n, 10.0)
    } else if let Some(n) = s.strip_suffix("in") {
        (n, 25.4)
    } else if let Some(n) = s.strip_suffix("pt") {
        (n, 25.4 / 72.0)
    } else if let Some(n) = s.strip_suffix("pc") {
        (n, 25.4 / 6.0)
    } else {
        (s.strip_suffix("px")?, 25.4 / 96.0)
    };
    let val = num_str.trim().parse::<f64>().ok()?;
    if val.is_finite() {
        Some(val * factor)
    } else {
        None
    }
}
