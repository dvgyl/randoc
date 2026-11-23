//! Converts presentation MathML to TeX.

use randoc_core::Limits;

use crate::xml::{self, XmlElement, XmlLimits, XmlOptions};

/// Parses a MathML element and converts it to TeX within the XML limits.
pub fn mathml_str_to_tex(src: &str, limits: &XmlLimits, html_entities: bool) -> Option<String> {
    let options = XmlOptions {
        html_entities,
        ..XmlOptions::default()
    };
    let doc = xml::parse(src, limits, options).ok()?;
    mathml_to_tex(&doc.root, limits.max_depth)
}

/// Converts a MathML element into a TeX string.
/// Returns `None` if the element has no convertible MathML content (e.g. StarMath-only `<semantics>`).
pub fn mathml_to_tex(el: &XmlElement, max_depth: usize) -> Option<String> {
    let mut ctx = MmlCtx {
        max_depth,
        unsupported: false,
    };
    let tokens = ctx.parse_children(el, 0);
    if ctx.unsupported || tokens.is_empty() {
        return None;
    }
    let out = strip_outer_braces(render_tokens(&tokens));
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
    /// Explicit MathML spacing.
    Space(String),
    /// Pre-formatted TeX expression (`\frac{a}{b}`, `c^{2}`, `{...}`).
    Raw(String),
}

impl Tok {
    fn text(&self) -> &str {
        match self {
            Self::Ident(s)
            | Self::Num(s)
            | Self::Bin(s)
            | Self::Rel(s)
            | Self::Punct(s)
            | Self::Op(s)
            | Self::Open(s)
            | Self::Close(s)
            | Self::Func(s)
            | Self::Space(s)
            | Self::Raw(s) => s,
        }
    }
}

struct MmlCtx {
    max_depth: usize,
    unsupported: bool,
}

impl MmlCtx {
    fn parse_node(&mut self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
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
                if matches!(el.name.local.as_str(), "mrow" | "mstyle") && inner.len() > 1 {
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
                let text = el.all_text();
                let text = text.trim_matches(|ch: char| ch.is_ascii_whitespace());
                if text == "\u{a0}" {
                    out.push(Tok::Space("\\ ".to_owned()));
                } else if !text.is_empty() {
                    out.push(Tok::Raw(format!("\\text{{{}}}", escape_tex_text(text))));
                }
            }
            "ms" => {
                let t = el.all_text();
                out.push(Tok::Raw(format!("\\text{{\"{}\"}}", escape_tex_text(&t))));
            }
            "mspace" => {
                out.push(Tok::Space(math_space(
                    el.attr_local("width").unwrap_or_default(),
                )));
            }
            "msup" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 2 {
                    let base = self.parse_single_as_toks(elems[0], depth + 1);
                    let sup = self.render_argument(elems[1], depth + 1);
                    let base_str = wrap_script_base(&base);
                    out.push(Tok::Raw(format!("{base_str}^{{{sup}}}")));
                }
            }
            "msub" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 2 {
                    let base = self.parse_single_as_toks(elems[0], depth + 1);
                    let sub = self.render_argument(elems[1], depth + 1);
                    let base_str = wrap_script_base(&base);
                    out.push(Tok::Raw(format!("{base_str}_{{{sub}}}")));
                }
            }
            "msubsup" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 3 {
                    let base = self.parse_single_as_toks(elems[0], depth + 1);
                    let sub = self.render_argument(elems[1], depth + 1);
                    let sup = self.render_argument(elems[2], depth + 1);
                    let base_str = wrap_script_base(&base);
                    out.push(Tok::Raw(format!("{base_str}_{{{sub}}}^{{{sup}}}")));
                }
            }
            "mfrac" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 2 {
                    let num = self.render_argument(elems[0], depth + 1);
                    let den = self.render_argument(elems[1], depth + 1);
                    if el.attr_local("linethickness") == Some("0") {
                        out.push(Tok::Raw(format!("\\binom{{{num}}}{{{den}}}")));
                    } else {
                        out.push(Tok::Raw(format!("\\frac{{{num}}}{{{den}}}")));
                    }
                }
            }
            "msqrt" => {
                let body = strip_outer_braces(render_tokens(&self.parse_children(el, depth + 1)));
                out.push(Tok::Raw(format!("\\sqrt{{{body}}}")));
            }
            "mroot" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 2 {
                    let body = self.render_argument(elems[0], depth + 1);
                    let deg = self.render_argument(elems[1], depth + 1);
                    out.push(Tok::Raw(format!("\\sqrt[{deg}]{{{body}}}")));
                }
            }
            "mover" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 2 {
                    let base_toks = self.parse_single_as_toks(elems[0], depth + 1);
                    let over_text = elems[1].all_text();
                    if let Some(cmd) = over_accent_cmd(over_text.trim()) {
                        let base = strip_outer_braces(render_tokens(&base_toks));
                        out.push(Tok::Raw(format!("{cmd}{{{base}}}")));
                    } else {
                        let over = self.render_argument(elems[1], depth + 1);
                        let tex = if matches!(base_toks.as_slice(), [Tok::Op(_) | Tok::Func(_)]) {
                            format!("{}\\limits^{{{over}}}", render_tokens(&base_toks))
                        } else {
                            let base = strip_outer_braces(render_tokens(&base_toks));
                            format!("\\overset{{{over}}}{{{base}}}")
                        };
                        out.push(Tok::Raw(tex));
                    }
                }
            }
            "munder" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 2 {
                    let base_toks = self.parse_single_as_toks(elems[0], depth + 1);
                    let base = strip_outer_braces(render_tokens(&base_toks));
                    if elems[1].name.local == "mo" && elems[1].all_text().trim() == "⏟" {
                        out.push(Tok::Raw(format!("\\underbrace{{{base}}}")));
                    } else {
                        let under = self.render_argument(elems[1], depth + 1);
                        let tex = if matches!(base_toks.as_slice(), [Tok::Op(_) | Tok::Func(_)]) {
                            format!("{base}\\limits_{{{under}}}")
                        } else {
                            format!("\\underset{{{under}}}{{{base}}}")
                        };
                        out.push(Tok::Raw(tex));
                    }
                }
            }
            "munderover" => {
                let elems: Vec<&XmlElement> = el.elements().collect();
                if elems.len() >= 3 {
                    let base_toks = self.parse_single_as_toks(elems[0], depth + 1);
                    let base = strip_outer_braces(render_tokens(&base_toks));
                    let under = self.render_argument(elems[1], depth + 1);
                    let over = self.render_argument(elems[2], depth + 1);
                    if matches!(base_toks.as_slice(), [Tok::Op(_) | Tok::Func(_)]) {
                        out.push(Tok::Raw(format!("{base}\\limits_{{{under}}}^{{{over}}}")));
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
                let separators = el.attr_local("separators").unwrap_or(",");
                let mut separators = separators.chars().filter(|ch| !ch.is_whitespace());
                let mut separator = separators.next();
                let mut parts = Vec::new();
                for (index, child) in el.elements().enumerate() {
                    if index != 0
                        && let Some(ch) = separator
                    {
                        parts.push(Tok::Punct(ch.to_string()));
                        separator = separators.next().or(separator);
                    }
                    self.parse_node(child, depth + 1, &mut parts);
                }
                let left = tex_delim(open);
                let right = tex_delim(close);
                let tex = format_fence(left, right, &parts, parts.len() > 1);
                out.push(Tok::Raw(tex));
            }
            "mtable" => {
                out.push(Tok::Raw(self.render_table(el, depth)));
            }
            "mphantom" => {
                let body = strip_outer_braces(render_tokens(&self.parse_children(el, depth + 1)));
                out.push(Tok::Raw(format!("\\phantom{{{body}}}")));
            }
            "menclose" => {
                let body = strip_outer_braces(render_tokens(&self.parse_children(el, depth + 1)));
                let notation = el.attr_local("notation").unwrap_or("");
                if notation.contains("box") {
                    out.push(Tok::Raw(format!("\\boxed{{{body}}}")));
                } else {
                    out.push(Tok::Raw(body));
                }
            }
            "mlongdiv" | "mstack" => self.unsupported = true,
            _ => {
                // Unknown MathML element: descend into children.
                out.extend(self.parse_children(el, depth + 1));
            }
        }
    }

    fn render_table(&mut self, table: &XmlElement, depth: usize) -> String {
        let rows: Vec<Vec<String>> = table
            .elements()
            .filter(|row| row.name.local == "mtr")
            .map(|row| {
                row.elements()
                    .filter(|cell| cell.name.local == "mtd")
                    .map(|cell| {
                        let tokens = self.parse_children(cell, depth + 1);
                        let tex = render_tokens(&tokens);
                        if tokens.len() > 1 && !tex.is_empty() {
                            format!("{{{tex}}}")
                        } else {
                            tex
                        }
                    })
                    .collect()
            })
            .collect();
        let columns = rows.iter().map(Vec::len).max().unwrap_or_default();
        // Limit the number of cells that padding can add to a sparse table.
        let pad = rows.len().saturating_mul(columns) <= Limits::default().max_table_cells;
        let mut tex = String::from("\\begin{matrix}\n");
        for (row_index, row) in rows.iter().enumerate() {
            if row_index != 0 {
                tex.push_str(" \\\\\n");
            }
            let width = if pad { columns } else { row.len() };
            for column in 0..width {
                let cell = row.get(column).map_or("", String::as_str);
                if column != 0 {
                    tex.push_str(" &");
                    if !cell.is_empty() || (column + 1 == width && row_index + 1 == rows.len()) {
                        tex.push(' ');
                    }
                }
                tex.push_str(cell);
            }
        }
        if !rows.is_empty() {
            tex.push('\n');
        }
        tex.push_str("\\end{matrix}");
        tex
    }

    fn render_argument(&mut self, el: &XmlElement, depth: usize) -> String {
        strip_outer_braces(render_tokens(&self.parse_single_as_toks(el, depth)))
    }

    fn parse_children(&mut self, parent: &XmlElement, depth: usize) -> Vec<Tok> {
        if depth > self.max_depth {
            return Vec::new();
        }
        let mut out = Vec::new();
        for child in parent.elements() {
            self.parse_node(child, depth + 1, &mut out);
        }
        resolve_row_fence(&mut out);
        out
    }

    fn parse_single_as_toks(&mut self, el: &XmlElement, depth: usize) -> Vec<Tok> {
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
            out.push(Tok::Ident(format!("{cmd}{{{}}}", map_ident_str(trimmed))));
            return;
        }
        if let Some(styled) = styled_unicode_ident(trimmed) {
            out.push(Tok::Ident(styled));
            return;
        }
        let mapped = map_ident_str(trimmed);
        if trimmed.chars().count() > 1 && !mapped.starts_with('\\') {
            out.push(Tok::Ident(format!("{{{mapped}}}")));
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
            "≃" => out.push(Tok::Rel("\\simeq".to_owned())),
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
            "⟶" => out.push(Tok::Rel("\\longrightarrow".to_owned())),
            "↑" => out.push(Tok::Ident("\\uparrow".to_owned())),
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
            "(" | "[" | "{" | "⟨" | "〈" | ")" | "]" | "}" | "⟩" | "〉" => {
                let tex = tex_delim(trimmed).to_owned();
                if el.attr_local("stretchy") == Some("false") {
                    out.push(Tok::Raw(tex));
                } else if matches!(trimmed, "(" | "[" | "{" | "⟨" | "〈") {
                    out.push(Tok::Open(tex));
                } else {
                    out.push(Tok::Close(tex));
                }
            }
            "|" => out.push(Tok::Raw("|".to_owned())),
            "‖" | "∥" => out.push(Tok::Raw("\\|".to_owned())),
            "!" => out.push(Tok::Raw("!".to_owned())),
            "'" | "′" => out.push(Tok::Raw("'".to_owned())),
            "\u{2061}" | "\u{2062}" | "\u{2063}" | "\u{2064}" => {
                // Invisible FunctionApplication / InvisibleTimes / InvisibleSeparator / InvisiblePlus.
            }
            other => {
                if is_known_func(other) {
                    out.push(Tok::Func(format!("\\{other}")));
                } else if other.chars().count() > 1 && other.chars().any(char::is_alphabetic) {
                    let name = escape_tex_text(other).replace(' ', "\\ ");
                    out.push(Tok::Func(format!("\\operatorname{{{name}}}")));
                } else {
                    out.push(Tok::Ident(map_ident_str(other)));
                }
            }
        }
    }
}

fn resolve_row_fence(tokens: &mut Vec<Tok>) {
    let [Tok::Open(left), rest @ ..] = tokens.as_slice() else {
        return;
    };
    let mut depth = 1usize;
    for (index, token) in rest.iter().enumerate() {
        match token {
            Tok::Open(_) => depth += 1,
            Tok::Close(right) => {
                depth -= 1;
                if depth == 0 {
                    if index + 1 == rest.len() {
                        let tex = format_fence(left, right, &rest[..index], false);
                        *tokens = vec![Tok::Raw(tex)];
                    }
                    return;
                }
            }
            _ => {}
        }
    }
    let tex = format_fence(left, ".", rest, false);
    *tokens = vec![Tok::Raw(tex)];
}

fn format_fence(left: &str, right: &str, body: &[Tok], group: bool) -> String {
    let stretch = group
        || matches!(left, "." | "\\{")
        || matches!(right, "." | "\\}")
        || body.iter().any(|token| matches!(token, Tok::Raw(_)));
    let tex = render_tokens(body);
    if stretch {
        if group {
            format!("\\left{left} {{{tex}}} \\right{right}")
        } else {
            format!("\\left{left} {tex} \\right{right}")
        }
    } else if command_needs_space(left, &tex) {
        format!("{left} {tex}{right}")
    } else {
        format!("{left}{tex}{right}")
    }
}

fn math_space(width: &str) -> String {
    let (name, sign) = width
        .strip_prefix("negative")
        .map_or((width, 1.0), |name| (name, -1.0));
    let mu = match name {
        "veryverythinmathspace" => sign,
        "verythinmathspace" => sign * 2.0,
        "thinmathspace" => sign * 3.0,
        "mediummathspace" => sign * 4.0,
        "thickmathspace" => sign * 5.0,
        "verythickmathspace" => sign * 6.0,
        "veryverythickmathspace" => sign * 7.0,
        _ => numeric_math_space(width),
    };
    match mu {
        -3.5..=-2.5 => "\\!".to_owned(),
        2.5..=3.5 if mu > 2.5 => "\\,".to_owned(),
        3.5..=4.5 if mu > 3.5 => "\\ ".to_owned(),
        4.5..=5.5 if mu > 4.5 => "\\;".to_owned(),
        18.0 => "\\quad".to_owned(),
        36.0 => "\\qquad".to_owned(),
        _ if !mu.is_finite() || mu.abs() < 0.5 => String::new(),
        _ => format!("\\mspace{{{mu:.0}mu}}"),
    }
}

fn numeric_math_space(width: &str) -> f64 {
    let unit_start = width
        .find(|ch: char| !matches!(ch, '-' | '.' | '0'..='9'))
        .unwrap_or(width.len());
    let (number, unit) = width.split_at(unit_start);
    if !number
        .trim_start_matches('-')
        .starts_with(|ch: char| ch.is_ascii_digit())
    {
        return 0.0;
    }
    let mu_per_unit = match unit {
        "mu" => 1.0,
        "em" => 18.0,
        "ex" => 7.2,
        "pt" => 1.8,
        "pc" => 21.6,
        "in" => 129.6,
        "cm" => 129.6 / 2.54,
        "mm" => 129.6 / 25.4,
        _ => return 0.0,
    };
    number.parse::<f64>().unwrap_or_default() * mu_per_unit
}

fn wrap_script_base(tokens: &[Tok]) -> String {
    let rendered = strip_outer_braces(render_tokens(tokens));
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
        let text = tok.text();
        if let Some(p) = prev {
            let need_space = match (p, tok) {
                (Tok::Bin(_) | Tok::Rel(_), _) => true,
                (_, Tok::Bin(_) | Tok::Rel(_)) => true,
                (Tok::Space(_), _) => !out.is_empty() && text.starts_with(char::is_alphanumeric),
                _ => command_needs_space(p.text(), text),
            };
            if need_space && !out.ends_with(' ') {
                out.push(' ');
            }
        }
        out.push_str(text);
    }
    out
}

fn strip_outer_braces(mut s: String) -> String {
    while s.starts_with('{') && s.ends_with('}') && s.len() >= 2 {
        let mut depth = 0i32;
        let mut wraps_all = true;
        let mut chars = s.char_indices();
        while let Some((idx, ch)) = chars.next() {
            match ch {
                '\\' => {
                    chars.next();
                }
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

fn command_needs_space(previous: &str, next: &str) -> bool {
    next.starts_with(char::is_alphanumeric)
        && previous.rsplit_once('\\').is_some_and(|(_, suffix)| {
            !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_alphabetic())
        })
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
        "⏞" => Some("\\overbrace"),
        _ => None,
    }
}

fn tex_delim(d: &str) -> &str {
    match d {
        "" => ".",
        "[" => "\\lbrack",
        "]" => "\\rbrack",
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

fn styled_unicode_ident(s: &str) -> Option<String> {
    let mut chars = s.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    // Some capital script letters have code points outside the main alphabet.
    const SCRIPT_CAPITALS: &str = "𝒜ℬ𝒞𝒟ℰℱ𝒢ℋℐ𝒥𝒦ℒℳ𝒩𝒪𝒫𝒬ℛ𝒮𝒯𝒰𝒱𝒲𝒳𝒴𝒵";
    if let Some(index) = SCRIPT_CAPITALS.chars().position(|letter| letter == ch) {
        let letter = char::from(u8::try_from(index).ok()? + b'A');
        return Some(format!("\\mathcal{{{letter}}}"));
    }
    let (style, letter) = match ch {
        '𝔞'..='𝔷' => (
            "mathfrak",
            char::from_u32(u32::from(ch) - u32::from('𝔞') + u32::from('a'))?,
        ),
        'ⅅ' => ("mathbb", 'D'),
        'ⅆ' => ("mathbb", 'd'),
        'ⅇ' => ("mathbb", 'e'),
        'ⅈ' => ("mathbb", 'i'),
        'ⅉ' => ("mathbb", 'j'),
        _ => return None,
    };
    Some(format!("{{\\{style}{{{letter}}}}}"))
}

fn escape_tex_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '\u{a0}' => out.push('~'),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mathml_string_accepts_local_names_and_optional_html_entities() {
        let limits = XmlLimits::default();
        for source in [
            "<math><msup><mi>x</mi><mn>2</mn></msup></math>",
            r#"<mml:math xmlns:mml="http://www.w3.org/1998/Math/MathML"><mml:msup><mml:mi>x</mml:mi><mml:mn>2</mml:mn></mml:msup></mml:math>"#,
        ] {
            assert_eq!(
                mathml_str_to_tex(source, &limits, false).as_deref(),
                Some("x^{2}")
            );
            assert_eq!(
                mathml_str_to_tex(
                    source,
                    &XmlLimits {
                        max_depth: 2,
                        ..limits
                    },
                    false
                ),
                None
            );
        }
        let source = "<math><mi>&alpha;</mi></math>";
        assert_eq!(
            mathml_str_to_tex(source, &limits, true).as_deref(),
            Some(r"\alpha")
        );
        assert_eq!(mathml_str_to_tex(source, &limits, false), None);
        assert_eq!(mathml_str_to_tex("<math><mi></math>", &limits, false), None);
    }
}

#[cfg(test)]
mod features_tests {
    use super::*;

    fn assert_tex(source: &str, expected: &str) {
        let limits = XmlLimits::default();
        let math = format!("<math>{source}</math>");
        assert_eq!(
            mathml_str_to_tex(&math, &limits, true).as_deref(),
            Some(expected)
        );
    }

    #[test]
    fn integral_with_thin_space() {
        assert_tex(
            r#"<msubsup><mo>∫</mo><mrow><mo>−</mo><mi>∞</mi></mrow><mi>∞</mi></msubsup>
               <msup><mi>e</mi><mrow><mo>−</mo><msup><mi>x</mi><mn>2</mn></msup></mrow></msup>
               <mspace width="thinmathspace"/><mi>d</mi><mi>x</mi><mo>=</mo>
               <msqrt><mi>π</mi></msqrt>"#,
            r"\int_{- \infty}^{\infty}e^{- x^{2}}\, dx = \sqrt{\pi}",
        );
    }

    #[test]
    fn sum_with_limits() {
        assert_tex(
            "<munderover><mo>∑</mo><mrow><mi>n</mi><mo>=</mo><mn>1</mn></mrow><mi>∞</mi></munderover>
             <mfrac><mn>1</mn><msup><mi>n</mi><mn>2</mn></msup></mfrac><mo>=</mo>
             <mfrac><msup><mi>π</mi><mn>2</mn></msup><mn>6</mn></mfrac>",
            r"\sum\limits_{n = 1}^{\infty}\frac{1}{n^{2}} = \frac{\pi^{2}}{6}",
        );
    }

    #[test]
    fn quadratic_formula_with_leading_minus() {
        assert_tex(
            "<mi>x</mi><mo>=</mo><mfrac>
               <mrow><mo>−</mo><mi>b</mi><mo>±</mo>
                 <msqrt><msup><mi>b</mi><mn>2</mn></msup><mo>−</mo><mn>4</mn><mi>a</mi><mi>c</mi></msqrt>
               </mrow>
               <mrow><mn>2</mn><mi>a</mi></mrow>
             </mfrac>",
            r"x = \frac{- b \pm \sqrt{b^{2} - 4ac}}{2a}",
        );
    }

    fn assert_basic_rows(number_attr: &str, ident_attr: &str, operator_attr: &str) {
        let source = format!(
            "<mrow><mn{number_attr}>2</mn><mo{operator_attr}> &#x2061;</mo><mi{ident_attr}>x</mi></mrow>
             <mrow><mo{operator_attr}>+</mo><mi{ident_attr}>y</mi><mo{operator_attr}>-</mo><mi{ident_attr}>z</mi></mrow>"
        );
        assert_tex(&source, "{2x}{+ y - z}");
    }

    #[test]
    fn groups_sibling_rows() {
        assert_basic_rows("", "", "");
    }

    #[test]
    fn groups_rows_with_operator_classes() {
        assert_basic_rows("", "", r#" class="test""#);
    }

    #[test]
    fn groups_rows_with_identifier_classes() {
        assert_basic_rows("", r#" class="test""#, "");
    }

    #[test]
    fn groups_rows_with_number_classes() {
        assert_basic_rows(r#" class="test""#, "", "");
    }

    #[test]
    fn complex_number_with_labeled_braces() {
        assert_tex(
            r#"<mrow><mi>c</mi><mo>=</mo><mover><mover><mrow>
                 <munder><munder>
                   <mrow><mspace width="1.1em"/><mi>a</mi><mspace width="1.1em"/></mrow>
                   <mo>⏟</mo>
                 </munder><mtext>real</mtext></munder>
                 <mo>+</mo>
                 <munder><munder>
                   <mrow><mspace width="1em"/><mi>b</mi><mo>&#x2062;</mo><mi>ⅈ</mi><mspace width="1em"/></mrow>
                   <mo>⏟</mo>
                 </munder><mtext>imaginary</mtext></munder>
               </mrow><mo>⏞</mo></mover><mtext>complex number</mtext></mover></mrow>"#,
            concat!(
                r"c = \overset{\text{complex number}}{\overbrace{",
                r"\underset{\text{real}}{\underbrace{\mspace{20mu} a\mspace{20mu}}}",
                r" + \underset{\text{imaginary}}{\underbrace{\quad b{\mathbb{i}}\quad}}}}",
            ),
        );
    }

    #[test]
    fn matrix_with_spans_and_named_operators() {
        let arrow = "<mtd><mo>⟶</mo></mtd>";
        let up = "<mtd><mo>↑</mo></mtd>";
        let operator = |name, argument| {
            format!("<mtd><mo>{name}</mo><mfenced><mi>{argument}</mi></mfenced></mtd>")
        };
        let source = format!(
            r#"<mtable>
             <mtr><mtd columnspan="2"/>{cov_l}{arrow}{non_k}{arrow}{cof_k}{arrow}{cof_l}{arrow}
               <mtd><msup><mn>2</mn><msub><mi>ℵ</mi><mn>0</mn></msub></msup></mtd>
             </mtr>
             <mtr><mtd columnspan="2"/><mtd rowspan="3"><mo>↑</mo></mtd><mtd/>{up}<mtd/>{up}
               <mtd/><mtd rowspan="3"><mo>↑</mo></mtd><mtd columnspan="2"/>
             </mtr>
             <mtr><mtd columnspan="4"/><mtd><mi>𝔟</mi></mtd>{arrow}<mtd><mi>𝔡</mi></mtd><mtd columnspan="5"/></mtr>
             <mtr><mtd columnspan="4"/>{up}<mtd/>{up}<mtd columnspan="5"/></mtr>
             <mtr><mtd><msub><mi>ℵ</mi><mn>1</mn></msub></mtd>
               {arrow}{add_l}{arrow}{add_k}{arrow}{cov_k}{arrow}{non_l}<mtd columnspan="2"/>
             </mtr>
             </mtable>"#,
            cov_l = operator("cov", "ℒ"),
            non_k = operator("non", "𝒦"),
            cof_k = operator("cof", "𝒦"),
            cof_l = operator("cof", "ℒ"),
            add_l = operator("add", "ℒ"),
            add_k = operator("add", "𝒦"),
            cov_k = operator("cov", "𝒦"),
            non_l = operator("non", "ℒ"),
        );
        assert_tex(
            &source,
            concat!(
                "\\begin{matrix}\n",
                r" & {\operatorname{cov}(\mathcal{L})} & \longrightarrow & {\operatorname{non}(\mathcal{K})}",
                r" & \longrightarrow & {\operatorname{cof}(\mathcal{K})} & \longrightarrow & {\operatorname{cof}(\mathcal{L})}",
                " & \\longrightarrow & 2^{\\aleph_{0}} \\\\\n",
                " & \\uparrow & & \\uparrow & & \\uparrow & & \\uparrow & & \\\\\n",
                " & {\\mathfrak{b}} & \\longrightarrow & {\\mathfrak{d}} & & & & & & \\\\\n",
                " & \\uparrow & & \\uparrow & & & & & & \\\\\n",
                r"\aleph_{1} & \longrightarrow & {\operatorname{add}(\mathcal{L})} & \longrightarrow & {\operatorname{add}(\mathcal{K})}",
                " & \\longrightarrow & {\\operatorname{cov}(\\mathcal{K})} & \\longrightarrow & {\\operatorname{non}(\\mathcal{L})} & \n",
                r"\end{matrix}",
            ),
        );
    }

    #[test]
    fn arabic_piecewise_function() {
        assert_tex(
            r#"<mrow>
             <mrow><mi>د</mi><mo>&#x2061;</mo><mrow><mo>(</mo><mi>س</mi><mo>)</mo></mrow></mrow>
             <mo>=</mo><mrow><mo>{</mo><mtable>
               <mtr>
                 <mtd><mrow><munderover><mo movablelimits="false">∑</mo>
                   <mrow><mi>ٮ</mi><mo>=</mo><mn>1</mn></mrow><mi>ص</mi></munderover>
                   <mo>&#x2061;</mo><msup><mi>س</mi><mi>ٮ</mi></msup></mrow></mtd>
                 <mtd><mrow><mtext>إذاكان </mtext><mi>س</mi><mo>&gt;</mo><mn>0</mn></mrow></mtd>
               </mtr>
               <mtr>
                 <mtd><mrow><msubsup><mo>∫</mo><mn>1</mn><mi>ص</mi></msubsup>
                   <mo>&#x2061;</mo><mrow><msup><mi>س</mi><mi>ٮ</mi></msup>
                     <mo>&#x2062;</mo><mi>ء</mi><mo>&#x2061;</mo><mi>س</mi></mrow></mrow></mtd>
                 <mtd><mrow><mtext>إذاكان </mtext><mi>س</mi><mo>∈</mo><mi>م</mi></mrow></mtd>
               </mtr>
               <mtr>
                 <mtd><mrow><mi>طا</mi><mo>&#x2061;</mo><mi>π</mi></mrow></mtd>
                 <mtd><mrow><mtext>غيرذلك </mtext><mrow>
                   <mo>(</mo><mtext>مع</mtext><mi>π</mi><mo>≃</mo><mn>3,141</mn><mo>)</mo>
                 </mrow></mrow></mtd>
               </mtr>
             </mtable></mrow></mrow>"#,
            concat!(
                "{د(س)} = \\left\\{ \\begin{matrix}\n",
                "{\\sum\\limits_{ٮ = 1}^{ص}س^{ٮ}} & {\\text{إذاكان}س > 0} \\\\\n",
                "{\\int_{1}^{ص}{س^{ٮ}ءس}} & {\\text{إذاكان}س \\in م} \\\\\n",
                "{{طا}\\pi} & {\\text{غيرذلك}\\left( \\text{مع}\\pi \\simeq 3,141 \\right)}\n",
                r"\end{matrix} \right.",
            ),
        );
    }

    #[test]
    fn long_division_requests_reader_fallback() {
        let math = r#"<math><mlongdiv longdivstyle="lefttop">
            <mn>3</mn><mn>435.3</mn><mn>1306</mn>
            <msgroup position="2" shift="-1">
              <msgroup><mn>12</mn><msline length="2"/></msgroup>
              <msgroup><mn>10</mn><mn>9</mn><msline length="2"/></msgroup>
              <msgroup><mn>16</mn><mn>15</mn><msline length="2"/><mn>1.0</mn></msgroup>
              <msgroup position="-1"><mn>9</mn><msline length="3"/><mn>1</mn></msgroup>
            </msgroup></mlongdiv></math>"#;
        assert_eq!(mathml_str_to_tex(math, &XmlLimits::default(), true), None);
    }
}
