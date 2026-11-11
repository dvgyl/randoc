//! The markup inside CSL JSON strings: a port of `pCslJson` (`Citeproc.Types`)
//! and `renderCslJson` (`Citeproc.CslJson`) of `citeproc` 0.14.1.1.
//!
//! The markup is a small subset of HTML (`<i>`, `<b>`, `<sc>`, some `span`
//! forms) and quotation marks. Text that is not valid markup stays text.
//!
//! The upstream parser backtracks. Its result at a position does not depend
//! on the enclosing markup, so this parser fills tables from the end of the
//! text to the start: the end of the item at each position, and the end of
//! each closing tag search. Time and memory are linear in the text length.

use crate::unicode_class::is_alpha_num;

/// A style that markup gives to its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Quotation marks.
    Quoted,
    /// `<i>`.
    Italic,
    /// Normal font style (only from a style, not from the parser).
    Normal,
    /// `<b>`.
    Bold,
    /// `<u>`.
    Underline,
    /// `<span class="nodecor">`.
    NoDecoration,
    /// `<sc>` or `<span style="font-variant:small-caps;">`.
    SmallCaps,
    /// `<span class="baseline">`.
    Baseline,
    /// `<sup>`.
    Superscript,
    /// `<sub>`.
    Subscript,
    /// `<span class="nocase">`: no case change.
    NoCase,
}

/// A part of formatted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Markup {
    /// Plain text.
    Text(String),
    /// Content with a style.
    Styled(Style, Vec<Markup>),
    /// A display block (`csl-` and the class).
    Div(String, Vec<Markup>),
    /// A link.
    Link(String, Vec<Markup>),
}

/// The markup is nested deeper than the depth limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooDeep;

/// The closing strings of the parser. The index is the closer of a [`Group`].
const CLOSERS: [&str; 11] = [
    "</i>", "</b>", "</u>", "</span>", "</sup>", "</sub>", "</sc>", "\"", "'", "\u{201D}",
    "\u{2019}",
];
const QUOTE_CLOSERS: [(char, usize); 4] = [('"', 7), ('\'', 8), ('\u{201C}', 9), ('\u{2018}', 10)];

const NONE: u32 = u32::MAX;

/// Matches the rest of an opening `span` tag and returns the position after it.
type SpanOpen = fn(&Parser, usize) -> Option<usize>;

#[derive(Debug, Clone, Copy)]
enum Item {
    /// Plain text from `start` to `end`. `apostrophe` is the position of an
    /// apostrophe that becomes `’`.
    Text {
        end: usize,
        apostrophe: Option<usize>,
    },
    /// Styled content from `start` to `content_end`, then the closer to `end`.
    Group {
        style: Style,
        start: usize,
        content_end: usize,
        end: usize,
    },
    /// One special character.
    Symbol { end: usize },
}

impl Item {
    fn end(self) -> usize {
        match self {
            Item::Text { end, .. } | Item::Group { end, .. } | Item::Symbol { end } => end,
        }
    }
}

struct Parser {
    chars: Vec<char>,
    /// For each position: the end of the run of letters and digits that are not special.
    word_end: Vec<u32>,
    /// For each position: the end of the run of other characters that are not special.
    other_end: Vec<u32>,
    /// For each position: the end of the run of letters and digits.
    alnum_end: Vec<u32>,
    /// For each position: the end of the item there.
    item_end: Vec<u32>,
    /// For each closer and position: the end of the closer that `manyTill` finds.
    till: Vec<Vec<u32>>,
}

/// Parses the markup of a CSL JSON string with the default quotation marks.
///
/// Groups deeper than `max_depth` give [`TooDeep`].
pub fn parse(text: &str, max_depth: usize) -> Result<Vec<Markup>, TooDeep> {
    let parser = Parser::new(text);
    parser.build(0, parser.chars.len(), max_depth)
}

fn is_special(c: char) -> bool {
    matches!(c, '<' | '>' | '\'' | '"' | '\u{2019}')
        || (!c.is_ascii() && (is_quote_char(c) || superscript(c).is_some()))
}

fn is_quote_char(c: char) -> bool {
    matches!(c, '\u{201C}' | '\u{201D}' | '\u{2018}' | '\u{2019}')
}

fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '\u{2019}'
}

fn is_space_char(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(NONE)
}

fn from_u32(value: u32) -> Option<usize> {
    (value != NONE).then_some(value as usize)
}

impl Parser {
    fn new(text: &str) -> Parser {
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len();
        let mut parser = Parser {
            word_end: vec![to_u32(n); n + 1],
            other_end: vec![to_u32(n); n + 1],
            alnum_end: vec![to_u32(n); n + 1],
            item_end: vec![NONE; n + 1],
            till: vec![vec![NONE; n + 1]; CLOSERS.len()],
            chars,
        };
        for p in (0..n).rev() {
            let c = parser.chars[p];
            let alnum = is_alpha_num(c);
            let special = is_special(c);
            parser.word_end[p] = if alnum && !special {
                parser.word_end[p + 1]
            } else {
                to_u32(p)
            };
            parser.other_end[p] = if !alnum && !special {
                parser.other_end[p + 1]
            } else {
                to_u32(p)
            };
            parser.alnum_end[p] = if alnum {
                parser.alnum_end[p + 1]
            } else {
                to_u32(p)
            };
            let item_end = parser.item(p).end();
            parser.item_end[p] = to_u32(item_end);
            for (closer, text) in CLOSERS.iter().enumerate() {
                parser.till[closer][p] = if parser.starts_with(p, text) {
                    to_u32(p + text.chars().count())
                } else {
                    parser.till[closer][item_end]
                };
            }
        }
        parser
    }

    fn starts_with(&self, p: usize, text: &str) -> bool {
        (p..)
            .zip(text.chars())
            .all(|(q, c)| self.chars.get(q) == Some(&c))
    }

    /// Matches a literal and returns the position after it.
    fn literal(&self, p: usize, text: &str) -> Option<usize> {
        self.starts_with(p, text).then(|| p + text.chars().count())
    }

    fn skip_space(&self, mut p: usize) -> usize {
        while self.chars.get(p).copied().is_some_and(is_space_char) {
            p += 1;
        }
        p
    }

    /// The content between an opening string that ends at `start` and a closer.
    fn group(&self, style: Style, start: usize, closer: usize) -> Option<Item> {
        let end = from_u32(self.till[closer][start])?;
        Some(Item::Group {
            style,
            start,
            content_end: end - CLOSERS[closer].chars().count(),
            end,
        })
    }

    /// The item at position `p < len`, in the order of `P.choice` of `pCslJson`.
    fn item(&self, p: usize) -> Item {
        if let Some(item) = self.text(p) {
            return item;
        }
        if let Some(item) = self.quoted(p) {
            return item;
        }
        if let Some(item) = self.tag(p) {
            return item;
        }
        Item::Symbol { end: p + 1 }
    }

    fn text(&self, p: usize) -> Option<Item> {
        let word_end = self.word_end[p] as usize;
        if word_end > p {
            let apostrophe = self
                .chars
                .get(word_end)
                .copied()
                .filter(|c| is_apostrophe(*c))
                .map(|_| word_end)
                .filter(|q| self.alnum_end[q + 1] as usize > q + 1);
            return Some(match apostrophe {
                Some(q) => Item::Text {
                    end: self.alnum_end[q + 1] as usize,
                    apostrophe: Some(q),
                },
                None => Item::Text {
                    end: word_end,
                    apostrophe: None,
                },
            });
        }
        let other_end = self.other_end[p] as usize;
        (other_end > p).then_some(Item::Text {
            end: other_end,
            apostrophe: None,
        })
    }

    fn quoted(&self, p: usize) -> Option<Item> {
        let c = self.chars[p];
        let (_, closer) = QUOTE_CLOSERS.iter().find(|(open, _)| *open == c)?;
        let next = *self.chars.get(p + 1)?;
        if is_space_char(next) || CLOSERS[*closer].starts_with(next) {
            return None;
        }
        self.group(Style::Quoted, p + 1, *closer)
    }

    fn tag(&self, p: usize) -> Option<Item> {
        const SIMPLE: [(&str, Style, usize); 6] = [
            ("<i>", Style::Italic, 0),
            ("<b>", Style::Bold, 1),
            ("<u>", Style::Underline, 2),
            ("<sup>", Style::Superscript, 4),
            ("<sub>", Style::Subscript, 5),
            ("<sc>", Style::SmallCaps, 6),
        ];
        if self.chars[p] != '<' {
            return None;
        }
        for (open, style, closer) in SIMPLE {
            if let Some(item) = self
                .literal(p, open)
                .and_then(|start| self.group(style, start, closer))
            {
                return Some(item);
            }
        }
        let after_span = self.skip_space(self.literal(p, "<span")?);
        let spans: [(Style, SpanOpen); 4] = [
            (Style::NoDecoration, |s, q| {
                s.class_span(q, "class=\"nodecor\"")
            }),
            (Style::SmallCaps, Parser::small_caps_span),
            (Style::Baseline, |s, q| s.literal(q, "class=\"baseline\">")),
            (Style::NoCase, |s, q| s.class_span(q, "class=\"nocase\"")),
        ];
        spans.iter().find_map(|(style, open)| {
            open(self, after_span).and_then(|start| self.group(*style, start, 3))
        })
    }

    fn class_span(&self, p: usize, class: &str) -> Option<usize> {
        let q = self.skip_space(self.literal(p, class)?);
        self.literal(q, ">")
    }

    fn small_caps_span(&self, p: usize) -> Option<usize> {
        let q = self.skip_space(self.literal(p, "style=\"font-variant:")?);
        let q = self.skip_space(self.literal(q, "small-caps;")?);
        let q = self.skip_space(self.literal(q, "\"")?);
        self.literal(q, ">")
    }

    fn build(&self, mut p: usize, end: usize, depth: usize) -> Result<Vec<Markup>, TooDeep> {
        let mut out = Vec::new();
        while p < end {
            let item = self.item(p);
            match item {
                Item::Text { end, apostrophe } => {
                    let mut text: String = self.chars[p..end]
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            if Some(p + i) == apostrophe {
                                '\u{2019}'
                            } else {
                                *c
                            }
                        })
                        .collect();
                    add_narrow_space(&mut text);
                    out.push(Markup::Text(text));
                }
                Item::Symbol { .. } => {
                    let c = self.chars[p];
                    let c = if is_apostrophe(c) { '\u{2019}' } else { c };
                    out.push(Markup::Text(c.to_string()));
                }
                Item::Group {
                    style,
                    start,
                    content_end,
                    ..
                } => {
                    let inner = depth.checked_sub(1).ok_or(TooDeep)?;
                    out.push(Markup::Styled(
                        style,
                        self.build(start, content_end, inner)?,
                    ));
                }
            }
            p = item.end();
        }
        Ok(out)
    }
}

fn add_narrow_space(text: &mut String) {
    if !text.contains([' ']) {
        return;
    }
    let replaced = text
        .replace("\u{AB} ", "\u{AB}\u{202F}")
        .replace(" \u{BB}", "\u{202F}\u{BB}")
        .replace(" !", "\u{202F}!")
        .replace(" ?", "\u{202F}?")
        .replace(" ;", "\u{202F};");
    *text = replaced;
}

/// Renders markup as a CSL JSON string (`renderCslJson False` with the
/// default locale).
pub fn render(markup: &[Markup]) -> String {
    let mut out = String::new();
    let context = Context {
        outer_quotes: true,
        italics: true,
        bold: true,
        small_caps: true,
    };
    for part in markup {
        render_one(part, context, &mut out);
    }
    out
}

#[derive(Debug, Clone, Copy)]
struct Context {
    outer_quotes: bool,
    italics: bool,
    bold: bool,
    small_caps: bool,
}

fn render_all(parts: &[Markup], context: Context, out: &mut String) {
    for part in parts {
        render_one(part, context, out);
    }
}

fn wrap(out: &mut String, open: &str, parts: &[Markup], context: Context, close: &str) {
    out.push_str(open);
    render_all(parts, context, out);
    out.push_str(close);
}

fn render_one(markup: &Markup, c: Context, out: &mut String) {
    const NORMAL_STYLE: &str = "<span style=\"font-style:normal;\">";
    match markup {
        Markup::Text(text) => {
            let mut chars = text.chars();
            match (chars.next().and_then(superscript), chars.next()) {
                (Some(base), None) => {
                    out.push_str("<sup>");
                    out.push_str(base);
                    out.push_str("</sup>");
                }
                _ => out.push_str(text),
            }
        }
        Markup::Styled(style, parts) => match style {
            Style::Quoted if c.outer_quotes => {
                let inner = Context {
                    outer_quotes: false,
                    ..c
                };
                wrap(out, "\u{201C}", parts, inner, "\u{201D}");
            }
            Style::Quoted => {
                let inner = Context {
                    outer_quotes: true,
                    ..c
                };
                wrap(out, "\u{2018}", parts, inner, "\u{2019}");
            }
            Style::Normal if c.italics => render_all(parts, c, out),
            Style::Normal => wrap(out, NORMAL_STYLE, parts, c, "</span>"),
            Style::Italic if c.italics => {
                wrap(
                    out,
                    "<i>",
                    parts,
                    Context {
                        italics: false,
                        ..c
                    },
                    "</i>",
                );
            }
            Style::Italic => {
                wrap(
                    out,
                    NORMAL_STYLE,
                    parts,
                    Context { italics: true, ..c },
                    "</span>",
                );
            }
            Style::Bold if c.bold => {
                wrap(out, "<b>", parts, Context { bold: false, ..c }, "</b>");
            }
            Style::Bold => wrap(
                out,
                "<span style=\"font-weight:normal;\">",
                parts,
                Context { bold: true, ..c },
                "</span>",
            ),
            Style::Underline => wrap(out, "<u>", parts, c, "</u>"),
            Style::NoDecoration => {
                out.push_str("<span style=\"");
                if !c.small_caps {
                    out.push_str("font-variant:normal;");
                }
                if !c.bold {
                    out.push_str("font-weight:normal;");
                }
                if !c.italics {
                    out.push_str("font-style:normal;");
                }
                wrap(out, "\">", parts, c, "</span>");
            }
            Style::SmallCaps if c.small_caps => wrap(
                out,
                "<span style=\"font-variant:small-caps;\">",
                parts,
                Context {
                    small_caps: false,
                    ..c
                },
                "</span>",
            ),
            Style::SmallCaps => wrap(
                out,
                "<span style=\"font-variant:normal;\">",
                parts,
                Context {
                    small_caps: true,
                    ..c
                },
                "</span>",
            ),
            Style::Superscript => wrap(out, "<sup>", parts, c, "</sup>"),
            Style::Subscript => wrap(out, "<sub>", parts, c, "</sub>"),
            Style::Baseline => wrap(out, "<span class=\"baseline\">", parts, c, "</span>"),
            Style::NoCase => render_all(parts, c, out),
        },
        Markup::Div(class, parts) => {
            wrap(
                out,
                &format!("<div class=\"csl-{class}\">"),
                parts,
                c,
                "</div>",
            );
        }
        Markup::Link(url, parts) => wrap(out, &format!("<a href=\"{url}\">"), parts, c, "</a>"),
    }
}

/// The plain character of a superscript character (`superscriptChars`).
fn superscript(c: char) -> Option<&'static str> {
    SUPERSCRIPTS
        .binary_search_by_key(&c, |(key, _)| *key)
        .ok()
        .map(|index| SUPERSCRIPTS[index].1)
}

/// `superscriptChars` of `Citeproc.Types`, sorted by the first column.
const SUPERSCRIPTS: [(char, &str); 104] = [
    ('\u{00AA}', "\u{0061}"),
    ('\u{00B2}', "\u{0032}"),
    ('\u{00B3}', "\u{0033}"),
    ('\u{00B9}', "\u{0031}"),
    ('\u{00BA}', "\u{006F}"),
    ('\u{02B0}', "\u{0068}"),
    ('\u{02B1}', "\u{0266}"),
    ('\u{02B2}', "\u{006A}"),
    ('\u{02B3}', "\u{0072}"),
    ('\u{02B4}', "\u{0279}"),
    ('\u{02B5}', "\u{027B}"),
    ('\u{02B6}', "\u{0281}"),
    ('\u{02B7}', "\u{0077}"),
    ('\u{02B8}', "\u{0079}"),
    ('\u{02C0}', "\u{0294}"),
    ('\u{02C1}', "\u{0295}"),
    ('\u{02E0}', "\u{0263}"),
    ('\u{02E1}', "\u{006C}"),
    ('\u{02E2}', "\u{0073}"),
    ('\u{02E3}', "\u{0078}"),
    ('\u{02E4}', "\u{0295}"),
    ('\u{06E5}', "\u{0648}"),
    ('\u{06E6}', "\u{064A}"),
    ('\u{1D2C}', "\u{0041}"),
    ('\u{1D2D}', "\u{00C6}"),
    ('\u{1D2E}', "\u{0042}"),
    ('\u{1D30}', "\u{0044}"),
    ('\u{1D31}', "\u{0045}"),
    ('\u{1D32}', "\u{018E}"),
    ('\u{1D33}', "\u{0047}"),
    ('\u{1D34}', "\u{0048}"),
    ('\u{1D35}', "\u{0049}"),
    ('\u{1D36}', "\u{004A}"),
    ('\u{1D37}', "\u{004B}"),
    ('\u{1D38}', "\u{004C}"),
    ('\u{1D39}', "\u{004D}"),
    ('\u{1D3A}', "\u{004E}"),
    ('\u{1D3C}', "\u{004F}"),
    ('\u{1D3D}', "\u{0222}"),
    ('\u{1D3E}', "\u{0050}"),
    ('\u{1D3F}', "\u{0052}"),
    ('\u{1D40}', "\u{0054}"),
    ('\u{1D41}', "\u{0055}"),
    ('\u{1D42}', "\u{0057}"),
    ('\u{1D43}', "\u{0061}"),
    ('\u{1D44}', "\u{0250}"),
    ('\u{1D45}', "\u{0251}"),
    ('\u{1D46}', "\u{1D02}"),
    ('\u{1D47}', "\u{0062}"),
    ('\u{1D48}', "\u{0064}"),
    ('\u{1D49}', "\u{0065}"),
    ('\u{1D4A}', "\u{0259}"),
    ('\u{1D4B}', "\u{025B}"),
    ('\u{1D4C}', "\u{025C}"),
    ('\u{1D4D}', "\u{0067}"),
    ('\u{1D4F}', "\u{006B}"),
    ('\u{1D50}', "\u{006D}"),
    ('\u{1D51}', "\u{014B}"),
    ('\u{1D52}', "\u{006F}"),
    ('\u{1D53}', "\u{0254}"),
    ('\u{1D54}', "\u{1D16}"),
    ('\u{1D55}', "\u{1D17}"),
    ('\u{1D56}', "\u{0070}"),
    ('\u{1D57}', "\u{0074}"),
    ('\u{1D58}', "\u{0075}"),
    ('\u{1D59}', "\u{1D1D}"),
    ('\u{1D5A}', "\u{026F}"),
    ('\u{1D5B}', "\u{0076}"),
    ('\u{1D5C}', "\u{1D25}"),
    ('\u{1D5D}', "\u{03B2}"),
    ('\u{1D5E}', "\u{03B3}"),
    ('\u{1D5F}', "\u{03B4}"),
    ('\u{1D60}', "\u{03C6}"),
    ('\u{1D61}', "\u{03C7}"),
    ('\u{2070}', "\u{0030}"),
    ('\u{2071}', "\u{0069}"),
    ('\u{2074}', "\u{0034}"),
    ('\u{2075}', "\u{0035}"),
    ('\u{2076}', "\u{0036}"),
    ('\u{2077}', "\u{0037}"),
    ('\u{2078}', "\u{0038}"),
    ('\u{2079}', "\u{0039}"),
    ('\u{207A}', "\u{002B}"),
    ('\u{207B}', "\u{2212}"),
    ('\u{207C}', "\u{003D}"),
    ('\u{207D}', "\u{0028}"),
    ('\u{207E}', "\u{0029}"),
    ('\u{207F}', "\u{006E}"),
    ('\u{2120}', "\u{0053}\u{004D}"),
    ('\u{2122}', "\u{0054}\u{004D}"),
    ('\u{3192}', "\u{4E00}"),
    ('\u{3193}', "\u{4E8C}"),
    ('\u{3194}', "\u{4E09}"),
    ('\u{3195}', "\u{56DB}"),
    ('\u{3196}', "\u{4E0A}"),
    ('\u{3197}', "\u{4E2D}"),
    ('\u{3198}', "\u{4E0B}"),
    ('\u{3199}', "\u{7532}"),
    ('\u{319A}', "\u{4E59}"),
    ('\u{319B}', "\u{4E19}"),
    ('\u{319C}', "\u{4E01}"),
    ('\u{319D}', "\u{5929}"),
    ('\u{319E}', "\u{5730}"),
    ('\u{319F}', "\u{4EBA}"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str) -> Markup {
        Markup::Text(t.into())
    }

    fn styled(style: Style, parts: Vec<Markup>) -> Markup {
        Markup::Styled(style, parts)
    }

    fn round_trip(input: &str) -> String {
        render(&parse(input, 128).unwrap())
    }

    #[test]
    fn the_superscript_table_is_sorted() {
        assert!(SUPERSCRIPTS.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn tags_give_styles() {
        assert_eq!(
            parse("<i>a</i><b>b</b>", 128).unwrap(),
            vec![
                styled(Style::Italic, vec![text("a")]),
                styled(Style::Bold, vec![text("b")])
            ]
        );
        assert_eq!(
            parse("<span  class=\"nocase\" >n</span>", 128).unwrap(),
            vec![styled(Style::NoCase, vec![text("n")])]
        );
        assert_eq!(
            parse("<span class=\"baseline\">x</span>", 128).unwrap(),
            vec![styled(Style::Baseline, vec![text("x")])]
        );
    }

    #[test]
    fn quotes_and_apostrophes() {
        assert_eq!(parse("don't", 128).unwrap(), vec![text("don\u{2019}t")]);
        assert_eq!(
            parse("\" x\"", 128).unwrap(),
            vec![text("\""), text(" "), text("x"), text("\"")]
        );
        assert_eq!(parse("\"\"", 128).unwrap(), vec![text("\""), text("\"")]);
        assert_eq!(
            parse("'a'", 128).unwrap(),
            vec![styled(Style::Quoted, vec![text("a")])]
        );
    }

    #[test]
    fn unclosed_tags_are_text() {
        assert_eq!(
            parse("<i>open", 128).unwrap(),
            vec![text("<"), text("i"), text(">"), text("open")]
        );
    }

    #[test]
    fn rendering_matches_the_oracle() {
        // Oracle: `pandoc -f csljson -t csljson` of each title.
        assert_eq!(
            round_trip("<sc>c</sc>"),
            "<span style=\"font-variant:small-caps;\">c</span>"
        );
        assert_eq!(
            round_trip("\"q 'in' q\""),
            "\u{201C}q \u{2018}in\u{2019} q\u{201D}"
        );
        assert_eq!(
            round_trip("x ; y ? z ! \u{AB} w \u{BB}"),
            "x\u{202F}; y\u{202F}? z\u{202F}! \u{AB}\u{202F}w\u{202F}\u{BB}"
        );
        assert_eq!(round_trip("\u{B2}"), "<sup>2</sup>");
        assert_eq!(round_trip("a\u{B2}"), "a<sup>2</sup>");
        assert_eq!(
            round_trip("<i><i>nest</i></i>"),
            "<i><span style=\"font-style:normal;\">nest</span></i>"
        );
        assert_eq!(round_trip("<span class=\"nocase\">n</span>"), "n");
    }

    #[test]
    fn rendering_of_each_style() {
        let all = |style| render(&[styled(style, vec![styled(style, vec![text("x")])])]);
        assert_eq!(all(Style::Normal), "x");
        assert_eq!(
            render(&[styled(
                Style::Italic,
                vec![styled(Style::Normal, vec![text("x")])]
            )]),
            "<i><span style=\"font-style:normal;\">x</span></i>"
        );
        assert_eq!(
            all(Style::Bold),
            "<b><span style=\"font-weight:normal;\">x</span></b>"
        );
        assert_eq!(
            all(Style::SmallCaps),
            "<span style=\"font-variant:small-caps;\"><span style=\"font-variant:normal;\">x</span></span>"
        );
        assert_eq!(all(Style::Underline), "<u><u>x</u></u>");
        assert_eq!(all(Style::Superscript), "<sup><sup>x</sup></sup>");
        assert_eq!(all(Style::Subscript), "<sub><sub>x</sub></sub>");
        assert_eq!(
            all(Style::Baseline),
            "<span class=\"baseline\"><span class=\"baseline\">x</span></span>"
        );
        assert_eq!(
            all(Style::NoDecoration),
            "<span style=\"\"><span style=\"\">x</span></span>"
        );
        assert_eq!(
            render(&[styled(
                Style::Italic,
                vec![styled(
                    Style::Bold,
                    vec![styled(
                        Style::SmallCaps,
                        vec![styled(Style::NoDecoration, vec![text("x")])]
                    )]
                )]
            )]),
            "<i><b><span style=\"font-variant:small-caps;\"><span style=\"font-variant:normal;font-weight:normal;font-style:normal;\">x</span></span></b></i>"
        );
        assert_eq!(
            render(&[
                Markup::Div("block".into(), vec![text("d")]),
                Markup::Link("u".into(), vec![text("l")])
            ]),
            "<div class=\"csl-block\">d</div><a href=\"u\">l</a>"
        );
    }

    #[test]
    fn depth_beyond_the_limit_is_an_error() {
        let deep = format!("{}x{}", "<b>".repeat(5), "</b>".repeat(5));
        assert!(parse(&deep, 5).is_ok());
        assert_eq!(parse(&deep, 4), Err(TooDeep));
    }
}
