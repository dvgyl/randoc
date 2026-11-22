//! Presentation MathML XML to TeX conversion for ODT/FODT embedded formulas.
//!
//! Converts `<math:math>` elements into TeX strings matching Pandoc / texmath's
//! TeX formatting conventions, bounded by `max_depth`.

use randoc_archive::xml::{XmlElement, XmlNode};

/// Converts a `<math:math>` element into a TeX string.
/// Returns `None` if the element has no convertible MathML content (e.g. StarMath-only `<semantics>`).
pub fn mathml_to_tex(el: &XmlElement, max_depth: usize) -> Option<String> {
    let ctx = MmlCtx { max_depth };
    let tokens = ctx.parse_math(el, 0);
    if tokens.is_empty() {
        return None;
    }
    let out = render_tokens(&tokens);
    if out.is_empty() { None } else { Some(out) }
}

#[derive(Debug, Clone)]
enum Tok {
    /// Identifier / variable (`x`, `\alpha`).
    Ident(String),
    /// Number (`2`, `12.5`).
    Num(String),
    /// Binary operator (`+`, `-`, `\cdot`, `\times`).
    Bin(String),
    /// Relation operator (`=`, `<`, `\leq`).
    Rel(String),
    /// Punctuation (`,`, `;`, `:`).
    Punct(String),
    /// Large operator (`\sum`, `\int`).
    Op(String),
    /// Opening delimiter (`(`, `[`, `\{`).
    Open(String),
    /// Closing delimiter (`)`, `]`, `\}`).
    Close(String),
    /// Function (`\sin`, `\cos`, `\lim`).
    Func(String),
    /// Pre-formatted TeX expression (`\frac{a}{b}`, `c^{2}`, `{...}`).
    Raw(String),
}

struct MmlCtx {
    max_depth: usize,
}

impl MmlCtx {
    fn parse_math(&self, el: &XmlElement, depth: usize) -> Vec<Tok> {
        if depth > self.max_depth {
            return Vec::new();
        }
        let mut out = Vec::new();
        for child in el.elements() {
            self.parse_node(child, depth + 1, &mut out);
        }
        out
    }

    fn parse_node(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        if depth > self.max_depth {
            return;
        }
        match el.name.local.as_str() {
            "semantics" => {
                // Only convert the first presentation child; ignore `<annotation>` / `<annotation-xml>`.
                if let Some(first) = el.elements().next()
                    && !matches!(first.name.local.as_str(), "annotation" | "annotation-xml")
                {
                    self.parse_node(first, depth + 1, out);
                }
            }
            "annotation" | "annotation-xml" | "none" | "maligngroup" | "malignmark" => {}
            "mrow" | "mstyle" | "mpadded" | "math" => {
                let inner = self.parse_children(el, depth + 1);
                if el.name.local == "mrow" && should_group_mrow(&inner) {
                    out.push(Tok::Raw(format!("{{{}}}", render_tokens(&inner))));
                } else {
                    out.extend(inner);
                }
            }
            "mi" => self.parse_mi(el, out),
            "mn" => {
                let t = el.all_text();
                let trimmed = t.trim();
                if !trimmed.is_empty() {
                    out.push(Tok::Num(trimmed.to_owned()));
                }
            }
            "mo" => self.parse_mo(el, out),
            "mtext" => {
                let t = el.all_text();
                if !t.is_empty() {
                    out.push(Tok::Raw(format!("\\text{{{}}}", escape_tex_text(&t))));
                }
            }
            "ms" => {
                let t = el.all_text();
                out.push(Tok::Raw(format!("\\text{{\"{}\"}}", escape_tex_text(&t))));
            }
            "mspace" => {
                out.push(Tok::Raw("\\ ".to_owned()));
            }
            "msup" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 2 {
                    let base = self.parse_single_as_toks(elems[0], depth + 1);
                    let sup = render_tokens(&self.parse_single_as_toks(elems[1], depth + 1));
                    let base_str = wrap_script_base(&base);
                    out.push(Tok::Raw(format!("{base_str}^{{{sup}}}")));
                }
            }
            "msub" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 2 {
                    let base = self.parse_single_as_toks(elems[0], depth + 1);
                    let sub = render_tokens(&self.parse_single_as_toks(elems[1], depth + 1));
                    let base_str = wrap_script_base(&base);
                    out.push(Tok::Raw(format!("{base_str}_{{{sub}}}")));
                }
            }
            "msubsup" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 3 {
                    let base = self.parse_single_as_toks(elems[0], depth + 1);
                    let sub = render_tokens(&self.parse_single_as_toks(elems[1], depth + 1));
                    let sup = render_tokens(&self.parse_single_as_toks(elems[2], depth + 1));
                    let base_str = wrap_script_base(&base);
                    out.push(Tok::Raw(format!("{base_str}_{{{sub}}}^{{{sup}}}")));
                }
            }
            "mfrac" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 2 {
                    let num = render_tokens(&self.parse_single_as_toks(elems[0], depth + 1));
                    let den = render_tokens(&self.parse_single_as_toks(elems[1], depth + 1));
                    if el.attr_local("linethickness") == Some("0") {
                        out.push(Tok::Raw(format!("\\binom{{{num}}}{{{den}}}")));
                    } else {
                        out.push(Tok::Raw(format!("\\frac{{{num}}}{{{den}}}")));
                    }
                }
            }
            "msqrt" => {
                let body = render_tokens(&self.parse_children(el, depth + 1));
                out.push(Tok::Raw(format!("\\sqrt{{{body}}}")));
            }
            "mroot" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 2 {
                    let body = render_tokens(&self.parse_single_as_toks(elems[0], depth + 1));
                    let deg = render_tokens(&self.parse_single_as_toks(elems[1], depth + 1));
                    out.push(Tok::Raw(format!("\\sqrt[{deg}]{{{body}}}")));
                }
            }
            "mover" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 2 {
                    let base = render_tokens(&self.parse_single_as_toks(elems[0], depth + 1));
                    let over_text = elems[1].all_text();
                    let over_trimmed = over_text.trim();
                    if let Some(cmd) = over_accent_cmd(over_trimmed) {
                        out.push(Tok::Raw(format!("{cmd}{{{base}}}")));
                    } else {
                        let over = render_tokens(&self.parse_single_as_toks(elems[1], depth + 1));
                        out.push(Tok::Raw(format!("\\overset{{{over}}}{{{base}}}")));
                    }
                }
            }
            "munder" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 2 {
                    let base_toks = self.parse_single_as_toks(elems[0], depth + 1);
                    let base = render_tokens(&base_toks);
                    let under = render_tokens(&self.parse_single_as_toks(elems[1], depth + 1));
                    if matches!(base_toks.first(), Some(Tok::Op(_) | Tok::Func(_)))
                        && base_toks.len() == 1
                    {
                        out.push(Tok::Raw(format!("{base}_{{{under}}}")));
                    } else {
                        out.push(Tok::Raw(format!("\\underset{{{under}}}{{{base}}}")));
                    }
                }
            }
            "munderover" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 3 {
                    let base_toks = self.parse_single_as_toks(elems[0], depth + 1);
                    let base = render_tokens(&base_toks);
                    let under = render_tokens(&self.parse_single_as_toks(elems[1], depth + 1));
                    let over = render_tokens(&self.parse_single_as_toks(elems[2], depth + 1));
                    if matches!(base_toks.first(), Some(Tok::Op(_) | Tok::Func(_)))
                        && base_toks.len() == 1
                    {
                        out.push(Tok::Raw(format!("{base}_{{{under}}}^{{{over}}}")));
                    } else {
                        out.push(Tok::Raw(format!(
                            "\\underset{{{under}}}{{\\overset{{{over}}}{{{base}}}}}"
                        )));
                    }
                }
            }
            "mfenced" => {
                let open = el.attr_local("open").unwrap_or("(");
                let close = el.attr_local("close").unwrap_or(")");
                let seps = el.attr_local("separators").unwrap_or(",");
                let mut parts = Vec::new();
                for child in el.elements() {
                    parts.push(render_tokens(&self.parse_single_as_toks(child, depth + 1)));
                }
                let sep_char = seps.chars().next().unwrap_or(',');
                let body = parts.join(&format!("{sep_char} "));
                let left = tex_delim(open);
                let right = tex_delim(close);
                out.push(Tok::Raw(format!("\\left{left}{body}\\right{right}")));
            }
            "mtable" => {
                let mut rows = Vec::new();
                for tr in el.elements().filter(|c| c.name.local == "mtr") {
                    let cells: Vec<String> = tr
                        .elements()
                        .filter(|c| c.name.local == "mtd")
                        .map(|td| render_tokens(&self.parse_children(td, depth + 1)))
                        .collect();
                    rows.push(cells.join(" & "));
                }
                out.push(Tok::Raw(format!(
                    "\\begin{{matrix}}\n{}\n\\end{{matrix}}",
                    rows.join(" \\\\\n")
                )));
            }
            "mphantom" => {
                let body = render_tokens(&self.parse_children(el, depth + 1));
                out.push(Tok::Raw(format!("\\phantom{{{body}}}")));
            }
            "menclose" => {
                let body = render_tokens(&self.parse_children(el, depth + 1));
                let notation = el.attr_local("notation").unwrap_or("");
                if notation.contains("box") {
                    out.push(Tok::Raw(format!("\\boxed{{{body}}}")));
                } else {
                    out.push(Tok::Raw(body));
                }
            }
            _ => {
                // Unknown MathML element: descend into children.
                out.extend(self.parse_children(el, depth + 1));
            }
        }
    }

    fn parse_children(&self, parent: &XmlElement, depth: usize) -> Vec<Tok> {
        if depth > self.max_depth {
            return Vec::new();
        }
        let mut out = Vec::new();
        for node in &parent.children {
            if let XmlNode::Element(child) = node {
                self.parse_node(child, depth + 1, &mut out);
            }
        }
        out
    }

    fn parse_single_as_toks(&self, el: &XmlElement, depth: usize) -> Vec<Tok> {
        // When an element is `<mrow>`, unwrap its children directly for script/frac arguments.
        if el.name.local == "mrow" {
            self.parse_children(el, depth)
        } else {
            let mut out = Vec::new();
            self.parse_node(el, depth, &mut out);
            out
        }
    }

    fn parse_mi(&self, el: &XmlElement, out: &mut Vec<Tok>) {
        let raw = el.all_text();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        let variant = el.attr_local("mathvariant");
        if is_known_func(trimmed) {
            out.push(Tok::Func(format!("\\{trimmed}")));
            return;
        }
        if let Some(cmd) = mathvariant_cmd(variant) {
            out.push(Tok::Raw(format!("{cmd}{{{}}}", map_ident_str(trimmed))));
            return;
        }
        let mapped = map_ident_str(trimmed);
        if trimmed.chars().count() > 1 && !mapped.starts_with('\\') {
            out.push(Tok::Raw(format!("{{{mapped}}}")));
        } else {
            out.push(Tok::Ident(mapped));
        }
    }

    fn parse_mo(&self, el: &XmlElement, out: &mut Vec<Tok>) {
        let raw = el.all_text();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        match trimmed {
            "+" | "-" | "\u{2212}" => {
                let s = if trimmed == "\u{2212}" { "-" } else { trimmed };
                out.push(Tok::Bin(s.to_owned()));
            }
            "×" => out.push(Tok::Bin("\\times".to_owned())),
            "÷" => out.push(Tok::Bin("\\div".to_owned())),
            "±" => out.push(Tok::Bin("\\pm".to_owned())),
            "∓" => out.push(Tok::Bin("\\mp".to_owned())),
            "·" | "⋅" => out.push(Tok::Bin("\\cdot".to_owned())),
            "∗" | "*" => out.push(Tok::Bin("\\ast".to_owned())),
            "∘" => out.push(Tok::Bin("\\circ".to_owned())),
            "⊕" => out.push(Tok::Bin("\\oplus".to_owned())),
            "⊗" => out.push(Tok::Bin("\\otimes".to_owned())),
            "∩" => out.push(Tok::Bin("\\cap".to_owned())),
            "∪" => out.push(Tok::Bin("\\cup".to_owned())),
            "∧" => out.push(Tok::Bin("\\land".to_owned())),
            "∨" => out.push(Tok::Bin("\\lor".to_owned())),
            "/" => out.push(Tok::Bin("/".to_owned())),
            "=" | "<" | ">" => out.push(Tok::Rel(trimmed.to_owned())),
            "≤" => out.push(Tok::Rel("\\leq".to_owned())),
            "≥" => out.push(Tok::Rel("\\geq".to_owned())),
            "≠" => out.push(Tok::Rel("\\neq".to_owned())),
            "≈" => out.push(Tok::Rel("\\approx".to_owned())),
            "≡" => out.push(Tok::Rel("\\equiv".to_owned())),
            "≅" => out.push(Tok::Rel("\\cong".to_owned())),
            "∼" => out.push(Tok::Rel("\\sim".to_owned())),
            "∝" => out.push(Tok::Rel("\\propto".to_owned())),
            "∈" => out.push(Tok::Rel("\\in".to_owned())),
            "∉" => out.push(Tok::Rel("\\notin".to_owned())),
            "⊂" => out.push(Tok::Rel("\\subset".to_owned())),
            "⊃" => out.push(Tok::Rel("\\supset".to_owned())),
            "⊆" => out.push(Tok::Rel("\\subseteq".to_owned())),
            "⊇" => out.push(Tok::Rel("\\supseteq".to_owned())),
            "→" => out.push(Tok::Rel("\\rightarrow".to_owned())),
            "←" => out.push(Tok::Rel("\\leftarrow".to_owned())),
            "↔" => out.push(Tok::Rel("\\leftrightarrow".to_owned())),
            "⇒" => out.push(Tok::Rel("\\Rightarrow".to_owned())),
            "⇐" => out.push(Tok::Rel("\\Leftarrow".to_owned())),
            "⇔" => out.push(Tok::Rel("\\Leftrightarrow".to_owned())),
            "↦" => out.push(Tok::Rel("\\mapsto".to_owned())),
            "∑" => out.push(Tok::Op("\\sum".to_owned())),
            "∏" => out.push(Tok::Op("\\prod".to_owned())),
            "∐" => out.push(Tok::Op("\\coprod".to_owned())),
            "∫" => out.push(Tok::Op("\\int".to_owned())),
            "∬" => out.push(Tok::Op("\\iint".to_owned())),
            "∭" => out.push(Tok::Op("\\iiint".to_owned())),
            "∮" => out.push(Tok::Op("\\oint".to_owned())),
            "⋃" => out.push(Tok::Op("\\bigcup".to_owned())),
            "⋂" => out.push(Tok::Op("\\bigcap".to_owned())),
            "," | ";" => out.push(Tok::Punct(trimmed.to_owned())),
            ":" => out.push(Tok::Punct(":".to_owned())),
            "(" | "[" => out.push(Tok::Open(trimmed.to_owned())),
            "{" => out.push(Tok::Open("\\{".to_owned())),
            ")" | "]" => out.push(Tok::Close(trimmed.to_owned())),
            "}" => out.push(Tok::Close("\\}".to_owned())),
            "|" => out.push(Tok::Raw("|".to_owned())),
            "‖" | "∥" => out.push(Tok::Raw("\\|".to_owned())),
            "⟨" | "〈" => out.push(Tok::Open("\\langle".to_owned())),
            "⟩" | "〉" => out.push(Tok::Close("\\rangle".to_owned())),
            "!" => out.push(Tok::Close("!".to_owned())),
            "'" | "′" => out.push(Tok::Raw("'".to_owned())),
            "\u{2061}" | "\u{2062}" | "\u{2063}" | "\u{2064}" => {
                // Invisible FunctionApplication / InvisibleTimes / InvisibleSeparator / InvisiblePlus.
            }
            other => {
                if let Some(ch) = other.chars().next()
                    && other.chars().count() == 1
                    && let Some(sym) = unicode_to_tex_symbol(ch)
                {
                    out.push(Tok::Ident(sym.to_owned()));
                } else {
                    out.push(Tok::Ident(other.to_owned()));
                }
            }
        }
    }
}

/// Heuristic matching texmath's nested `<mrow>` grouping:
/// When an inner `<mrow>` is nested inside another `<mrow>` alongside operators
/// (e.g., `<mrow><mi>E</mi><mo>=</mo><mrow><mi>m</mi><mo>⋅</mo><msup>...</msup></mrow></mrow>`),
/// texmath wraps the nested expression in `{...}` when it contains binary/relation ops
/// or multiple tokens.
fn should_group_mrow(inner: &[Tok]) -> bool {
    if inner.len() <= 1 {
        return false;
    }
    // Check if inner contains a binary or relation op or is a compound sub-expression.
    inner.iter().any(|t| matches!(t, Tok::Bin(_) | Tok::Rel(_)))
}

fn wrap_script_base(tokens: &[Tok]) -> String {
    let rendered = render_tokens(tokens);
    if tokens.len() == 1 {
        rendered
    } else {
        format!("{{{rendered}}}")
    }
}

fn render_tokens(tokens: &[Tok]) -> String {
    let mut out = String::new();
    for (i, tok) in tokens.iter().enumerate() {
        let prev = if i > 0 { tokens.get(i - 1) } else { None };
        let text = match tok {
            Tok::Ident(s)
            | Tok::Num(s)
            | Tok::Bin(s)
            | Tok::Rel(s)
            | Tok::Punct(s)
            | Tok::Op(s)
            | Tok::Open(s)
            | Tok::Close(s)
            | Tok::Func(s)
            | Tok::Raw(s) => s.as_str(),
        };
        if let Some(p) = prev {
            let need_space = match (p, tok) {
                (Tok::Bin(_) | Tok::Rel(_), _) => true,
                (_, Tok::Bin(_) | Tok::Rel(_)) => true,
                (Tok::Punct(s), _) if s == "," || s == ";" => true,
                (
                    Tok::Op(_) | Tok::Func(_),
                    Tok::Ident(_) | Tok::Num(_) | Tok::Func(_) | Tok::Op(_),
                ) => true,
                (Tok::Ident(ps), Tok::Ident(_) | Tok::Num(_)) if ps.starts_with('\\') => true,
                _ => {
                    let prev_ends_cmd = prev_ends_with_command(p);
                    prev_ends_cmd && text.starts_with(|c: char| c.is_ascii_alphabetic())
                }
            };
            if need_space && !out.ends_with(' ') {
                out.push(' ');
            }
        }
        out.push_str(text);
    }
    // Strip outer redundant braces if the entire formula is wrapped in `{...}`
    // from a top-level `<mrow>` (e.g. `<semantics><mrow><mrow>...</mrow></mrow>`).
    strip_outer_braces(out)
}

fn strip_outer_braces(mut s: String) -> String {
    while s.starts_with('{') && s.ends_with('}') && s.len() >= 2 {
        let mut depth = 0i32;
        let mut wraps_all = true;
        for (idx, ch) in s.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 && idx + ch.len_utf8() < s.len() {
                        wraps_all = false;
                        break;
                    }
                }
                _ => {}
            }
        }
        if wraps_all && depth == 0 {
            s = s[1..s.len() - 1].to_owned();
        } else {
            break;
        }
    }
    s
}

fn prev_ends_with_command(tok: &Tok) -> bool {
    let s = match tok {
        Tok::Ident(s)
        | Tok::Num(s)
        | Tok::Bin(s)
        | Tok::Rel(s)
        | Tok::Punct(s)
        | Tok::Op(s)
        | Tok::Open(s)
        | Tok::Close(s)
        | Tok::Func(s)
        | Tok::Raw(s) => s.as_str(),
    };
    if let Some(bs) = s.rfind('\\') {
        let after = &s[bs + 1..];
        !after.is_empty() && after.chars().all(|c| c.is_ascii_alphabetic())
    } else {
        false
    }
}

fn mathvariant_cmd(variant: Option<&str>) -> Option<&'static str> {
    match variant? {
        "bold" => Some("\\mathbf"),
        "bold-italic" => Some("\\mathbfit"),
        "double-struck" => Some("\\mathbb"),
        "fraktur" => Some("\\mathfrak"),
        "script" => Some("\\mathcal"),
        "sans-serif" => Some("\\mathsf"),
        "monospace" => Some("\\mathtt"),
        "normal" => None,
        _ => None,
    }
}

fn over_accent_cmd(s: &str) -> Option<&'static str> {
    match s {
        "^" | "\u{0302}" | "\u{02C6}" => Some("\\hat"),
        "~" | "\u{0303}" | "\u{02DC}" => Some("\\tilde"),
        "\u{0304}" | "\u{00AF}" | "\u{203E}" => Some("\\overline"),
        "\u{0307}" | "\u{02D9}" => Some("\\dot"),
        "\u{0308}" | "\u{00A8}" => Some("\\ddot"),
        "\u{030C}" | "\u{02C7}" => Some("\\check"),
        "\u{0301}" | "\u{00B4}" => Some("\\acute"),
        "\u{0300}" | "`" => Some("\\grave"),
        "\u{0306}" | "\u{02D8}" => Some("\\breve"),
        "\u{20D7}" | "\u{2192}" => Some("\\vec"),
        _ => None,
    }
}

fn tex_delim(d: &str) -> &str {
    match d {
        "" => ".",
        "[" => "[",
        "]" => "]",
        "{" => "\\{",
        "}" => "\\}",
        "|" => "|",
        "‖" | "∥" => "\\|",
        "⟨" | "〈" => "\\langle",
        "⟩" | "〉" => "\\rangle",
        "⌊" => "\\lfloor",
        "⌋" => "\\rfloor",
        "⌈" => "\\lceil",
        "⌉" => "\\rceil",
        other => other,
    }
}

fn is_known_func(s: &str) -> bool {
    matches!(
        s,
        "arccos"
            | "arcsin"
            | "arctan"
            | "arg"
            | "cos"
            | "cosh"
            | "cot"
            | "coth"
            | "csc"
            | "deg"
            | "det"
            | "dim"
            | "exp"
            | "gcd"
            | "hom"
            | "inf"
            | "ker"
            | "lg"
            | "lim"
            | "liminf"
            | "limsup"
            | "ln"
            | "log"
            | "max"
            | "min"
            | "Pr"
            | "sec"
            | "sin"
            | "sinh"
            | "sup"
            | "tan"
            | "tanh"
    )
}

fn map_ident_str(s: &str) -> String {
    if s.chars().count() == 1
        && let Some(ch) = s.chars().next()
        && let Some(sym) = unicode_to_tex_symbol(ch)
    {
        return sym.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if let Some(sym) = unicode_to_tex_symbol(ch) {
            out.push_str(sym);
        } else {
            out.push(ch);
        }
    }
    out
}

fn escape_tex_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' | '}' | '#' | '$' | '%' | '&' | '_' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

fn unicode_to_tex_symbol(ch: char) -> Option<&'static str> {
    match ch {
        'α' => Some("\\alpha"),
        'β' => Some("\\beta"),
        'γ' => Some("\\gamma"),
        'δ' => Some("\\delta"),
        'ε' => Some("\\varepsilon"),
        'ϵ' => Some("\\epsilon"),
        'ζ' => Some("\\zeta"),
        'η' => Some("\\eta"),
        'θ' => Some("\\theta"),
        'ϑ' => Some("\\vartheta"),
        'ι' => Some("\\iota"),
        'κ' => Some("\\kappa"),
        'λ' => Some("\\lambda"),
        'μ' => Some("\\mu"),
        'ν' => Some("\\nu"),
        'ξ' => Some("\\xi"),
        'π' => Some("\\pi"),
        'ϖ' => Some("\\varpi"),
        'ρ' => Some("\\rho"),
        'ϱ' => Some("\\varrho"),
        'σ' => Some("\\sigma"),
        'ς' => Some("\\varsigma"),
        'τ' => Some("\\tau"),
        'υ' => Some("\\upsilon"),
        'φ' => Some("\\varphi"),
        'ϕ' => Some("\\phi"),
        'χ' => Some("\\chi"),
        'ψ' => Some("\\psi"),
        'ω' => Some("\\omega"),
        'Γ' => Some("\\Gamma"),
        'Δ' => Some("\\Delta"),
        'Θ' => Some("\\Theta"),
        'Λ' => Some("\\Lambda"),
        'Ξ' => Some("\\Xi"),
        'Π' => Some("\\Pi"),
        'Σ' => Some("\\Sigma"),
        'Υ' => Some("\\Upsilon"),
        'Φ' => Some("\\Phi"),
        'Ψ' => Some("\\Psi"),
        'Ω' => Some("\\Omega"),
        '∞' => Some("\\infty"),
        '∂' => Some("\\partial"),
        '∇' => Some("\\nabla"),
        '∀' => Some("\\forall"),
        '∃' => Some("\\exists"),
        '∅' => Some("\\emptyset"),
        '¬' => Some("\\neg"),
        '⊥' => Some("\\bot"),
        '⊤' => Some("\\top"),
        'ℵ' => Some("\\aleph"),
        'ℏ' => Some("\\hbar"),
        'ℓ' => Some("\\ell"),
        'ℜ' => Some("\\Re"),
        'ℑ' => Some("\\Im"),
        '℘' => Some("\\wp"),
        '…' => Some("\\ldots"),
        '⋯' => Some("\\cdots"),
        '⋮' => Some("\\vdots"),
        '⋱' => Some("\\ddots"),
        _ => None,
    }
}
