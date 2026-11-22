//! Office Math Markup Language (OMML) to TeX conversion.
//!
//! Converts `m:oMath` elements into TeX strings matching Pandoc / texmath's
//! TeX formatting conventions.

use randoc_archive::xml::XmlElement;

use crate::ns::DocxNs;

/// Converts an `m:oMath` element into a TeX string.
pub fn omml_to_tex(el: &XmlElement, ns: &DocxNs, max_depth: usize) -> String {
    let ctx = OmmlCtx { ns, max_depth };
    let tokens = ctx.parse_children(el, 0);
    render_tokens(&tokens)
}

#[derive(Debug, Clone)]
enum Tok {
    /// Identifier / variable (italic).
    Ident(String),
    /// Number.
    Num(String),
    /// Binary operator (`+`, `-`, `\times`, ...).
    Bin(String),
    /// Relation operator (`=`, `<`, `\leq`, ...).
    Rel(String),
    /// Punctuation (`,`, `;`, `:`).
    Punct(String),
    /// Large operator (`\sum`, `\int`, ...).
    Op(String),
    /// Opening delimiter (`(`, `[`, `\{`).
    Open(String),
    /// Closing delimiter (`)`, `]`, `\}`).
    Close(String),
    /// Math function (`\sin`, `\cos`, `\lim`, ...).
    Func(String),
    /// Already formatted raw TeX expression (e.g., `\frac{a}{b}`, `x^{2}`, `\text{...}`).
    Raw(String),
}

struct OmmlCtx<'a> {
    ns: &'a DocxNs,
    max_depth: usize,
}

impl<'a> OmmlCtx<'a> {
    fn parse_children(&self, parent: &XmlElement, depth: usize) -> Vec<Tok> {
        if depth > self.max_depth {
            return Vec::new();
        }
        let mut out = Vec::new();
        for child in parent.elements() {
            self.parse_node(child, depth + 1, &mut out);
        }
        out
    }

    fn parse_node(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        if depth > self.max_depth {
            return;
        }
        if !self.is_math_el(el) {
            // Skip `w:rPr`, `m:oMathParaPr`, etc.; unwrap transparent wrappers like `w:ins`.
            if self.ns.is_w(el, "ins")
                || self.ns.is_w(el, "sdt")
                || self.ns.is_w(el, "sdtContent")
                || self.ns.is_w(el, "smartTag")
            {
                for c in el.elements() {
                    self.parse_node(c, depth + 1, out);
                }
            }
            return;
        }

        match el.name.local.as_str() {
            "r" => self.parse_run(el, out),
            "f" => self.parse_frac(el, depth, out),
            "sSup" => self.parse_sup(el, depth, out),
            "sSub" => self.parse_sub(el, depth, out),
            "sSubSup" => self.parse_subsup(el, depth, out),
            "sPre" => self.parse_pre(el, depth, out),
            "rad" => self.parse_rad(el, depth, out),
            "nary" => self.parse_nary(el, depth, out),
            "d" => self.parse_delim(el, depth, out),
            "func" => self.parse_func(el, depth, out),
            "acc" => self.parse_acc(el, depth, out),
            "bar" => self.parse_bar(el, depth, out),
            "groupChr" => self.parse_group_chr(el, depth, out),
            "limLow" => self.parse_lim_low(el, depth, out),
            "limUpp" => self.parse_lim_upp(el, depth, out),
            "m" => self.parse_matrix(el, depth, out),
            "eqArr" => self.parse_eq_arr(el, depth, out),
            "box" => {
                if let Some(e) = self.ns.m_child(el, "e") {
                    out.extend(self.parse_children(e, depth + 1));
                }
            }
            "borderBox" => {
                if let Some(e) = self.ns.m_child(el, "e") {
                    let inner = render_tokens(&self.parse_children(e, depth + 1));
                    out.push(Tok::Raw(format!("\\boxed{{{inner}}}")));
                }
            }
            "phant" => {
                if let Some(e) = self.ns.m_child(el, "e") {
                    let inner = render_tokens(&self.parse_children(e, depth + 1));
                    out.push(Tok::Raw(format!("\\phantom{{{inner}}}")));
                }
            }
            "e" | "oMath" => {
                out.extend(self.parse_children(el, depth + 1));
            }
            _ => {}
        }
    }

    fn is_math_el(&self, el: &XmlElement) -> bool {
        self.ns.is_m(el, &el.name.local)
    }

    fn parse_run(&self, el: &XmlElement, out: &mut Vec<Tok>) {
        let mut text = String::new();
        for c in el.elements() {
            if self.ns.is_m(c, "t") || self.ns.is_w(c, "t") {
                text.push_str(&c.text());
            }
        }
        if text.is_empty() {
            return;
        }
        let rpr = self.ns.m_child(el, "rPr");
        let is_nor = rpr
            .and_then(|p| self.ns.m_child(p, "nor"))
            .is_some_and(|n| {
                self.ns
                    .m_attr(n, "val")
                    .is_none_or(|v| v != "0" && v != "off")
            });
        let sty = rpr
            .and_then(|p| self.ns.m_child(p, "sty"))
            .and_then(|s| self.ns.m_attr(s, "val"));
        let scr = rpr
            .and_then(|p| self.ns.m_child(p, "scr"))
            .and_then(|s| self.ns.m_attr(s, "val"));

        if is_nor {
            out.push(Tok::Raw(format!("\\text{{{}}}", escape_tex_text(&text))));
            return;
        }

        // Check for math styled alphabet (`scr` / `sty`).
        if let Some(cmd) = math_style_command(scr, sty) {
            // Plain (`p`) with a known math operator name -> \sin, \cos, etc.
            if cmd == "\\mathrm" && is_known_func(text.trim()) {
                out.push(Tok::Func(format!("\\{}", text.trim())));
                return;
            }
            if cmd == "\\mathrm" && text.chars().all(|c| c.is_ascii_digit() || c == '.') {
                out.push(Tok::Num(text));
                return;
            }
            // Single plain operator or punctuation stays unstyled.
            let trimmed = text.trim();
            if cmd == "\\mathrm"
                && trimmed.chars().count() == 1
                && let Some(ch) = trimmed.chars().next()
                && !ch.is_alphabetic()
            {
                tokenize_math_text(&text, out);
                return;
            }
            let inner = render_tokens(&tokenize_to_vec(&text));
            out.push(Tok::Raw(format!("{cmd}{{{inner}}}")));
            return;
        }

        if sty == Some("p") && is_known_func(text.trim()) {
            out.push(Tok::Func(format!("\\{}", text.trim())));
            return;
        }

        tokenize_math_text(&text, out);
    }

    fn parse_frac(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let num = self
            .ns
            .m_child(el, "num")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        let den = self
            .ns
            .m_child(el, "den")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        let ftype = self
            .ns
            .m_child(el, "fPr")
            .and_then(|p| self.ns.m_child(p, "type"))
            .and_then(|t| self.ns.m_attr(t, "val"))
            .unwrap_or("bar");
        let tex = match ftype {
            "noBar" => format!("\\binom{{{num}}}{{{den}}}"),
            "lin" | "skw" => format!("{num}/{den}"),
            _ => format!("\\frac{{{num}}}{{{den}}}"),
        };
        out.push(Tok::Raw(tex));
    }

    fn parse_sup(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let base = self
            .ns
            .m_child(el, "e")
            .map(|c| self.parse_children(c, depth + 1))
            .unwrap_or_default();
        let sup = self
            .ns
            .m_child(el, "sup")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        let base_str = wrap_script_base(&base);
        out.push(Tok::Raw(format!("{base_str}^{{{sup}}}")));
    }

    fn parse_sub(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let base = self
            .ns
            .m_child(el, "e")
            .map(|c| self.parse_children(c, depth + 1))
            .unwrap_or_default();
        let sub = self
            .ns
            .m_child(el, "sub")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        let base_str = wrap_script_base(&base);
        out.push(Tok::Raw(format!("{base_str}_{{{sub}}}")));
    }

    fn parse_subsup(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let base = self
            .ns
            .m_child(el, "e")
            .map(|c| self.parse_children(c, depth + 1))
            .unwrap_or_default();
        let sub = self
            .ns
            .m_child(el, "sub")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        let sup = self
            .ns
            .m_child(el, "sup")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        let base_str = wrap_script_base(&base);
        out.push(Tok::Raw(format!("{base_str}_{{{sub}}}^{{{sup}}}")));
    }

    fn parse_pre(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let base = self
            .ns
            .m_child(el, "e")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        let sub = self
            .ns
            .m_child(el, "sub")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        let sup = self
            .ns
            .m_child(el, "sup")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        let mut s = String::from("{}");
        if !sub.is_empty() {
            s.push_str(&format!("_{{{sub}}}"));
        }
        if !sup.is_empty() {
            s.push_str(&format!("^{{{sup}}}"));
        }
        s.push_str(&base);
        out.push(Tok::Raw(s));
    }

    fn parse_rad(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let deg_hide = self
            .ns
            .m_child(el, "radPr")
            .and_then(|p| self.ns.m_child(p, "degHide"))
            .is_some_and(|d| {
                self.ns
                    .m_attr(d, "val")
                    .is_none_or(|v| v != "0" && v != "off")
            });
        let deg = if deg_hide {
            String::new()
        } else {
            self.ns
                .m_child(el, "deg")
                .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
                .unwrap_or_default()
        };
        let body = self
            .ns
            .m_child(el, "e")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        if deg.is_empty() {
            out.push(Tok::Raw(format!("\\sqrt{{{body}}}")));
        } else {
            out.push(Tok::Raw(format!("\\sqrt[{deg}]{{{body}}}")));
        }
    }

    fn parse_nary(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let pr = self.ns.m_child(el, "naryPr");
        let chr = pr
            .and_then(|p| self.ns.m_child(p, "chr"))
            .and_then(|c| self.ns.m_attr(c, "val"))
            .unwrap_or("∫");
        let sub_hide = pr
            .and_then(|p| self.ns.m_child(p, "subHide"))
            .is_some_and(|d| {
                self.ns
                    .m_attr(d, "val")
                    .is_none_or(|v| v != "0" && v != "off")
            });
        let sup_hide = pr
            .and_then(|p| self.ns.m_child(p, "supHide"))
            .is_some_and(|d| {
                self.ns
                    .m_attr(d, "val")
                    .is_none_or(|v| v != "0" && v != "off")
            });
        let op = match chr {
            "∑" => "\\sum",
            "∏" => "\\prod",
            "∐" => "\\coprod",
            "∫" => "\\int",
            "∬" => "\\iint",
            "∭" => "\\iiint",
            "∮" => "\\oint",
            "⋃" => "\\bigcup",
            "⋂" => "\\bigcap",
            "⋁" => "\\bigvee",
            "⋀" => "\\bigwedge",
            "⨁" => "\\bigoplus",
            "⨂" => "\\bigotimes",
            other => other,
        };
        let sub = if sub_hide {
            String::new()
        } else {
            self.ns
                .m_child(el, "sub")
                .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
                .unwrap_or_default()
        };
        let sup = if sup_hide {
            String::new()
        } else {
            self.ns
                .m_child(el, "sup")
                .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
                .unwrap_or_default()
        };
        let mut s = op.to_owned();
        if !sub.is_empty() {
            s.push_str(&format!("_{{{sub}}}"));
        }
        if !sup.is_empty() {
            s.push_str(&format!("^{{{sup}}}"));
        }
        out.push(Tok::Op(s));
        if let Some(e) = self.ns.m_child(el, "e") {
            out.extend(self.parse_children(e, depth + 1));
        }
    }

    fn parse_delim(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let pr = self.ns.m_child(el, "dPr");
        let beg = pr
            .and_then(|p| self.ns.m_child(p, "begChr"))
            .and_then(|c| self.ns.m_attr(c, "val"))
            .unwrap_or("(");
        let end = pr
            .and_then(|p| self.ns.m_child(p, "endChr"))
            .and_then(|c| self.ns.m_attr(c, "val"))
            .unwrap_or(")");
        let sep = pr
            .and_then(|p| self.ns.m_child(p, "sepChr"))
            .and_then(|c| self.ns.m_attr(c, "val"))
            .unwrap_or("|");

        let parts: Vec<String> = el
            .elements()
            .filter(|c| self.ns.is_m(c, "e"))
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .collect();
        let body = parts.join(&format!(" {sep} "));
        let left = tex_delim(beg);
        let right = tex_delim(end);
        let left_sep = if left.starts_with('\\') && left.len() > 2 {
            " "
        } else {
            ""
        };
        let right_sep = if right.starts_with('\\') && right.len() > 2 {
            " "
        } else {
            ""
        };
        out.push(Tok::Raw(format!(
            "\\left{left}{left_sep}{body}{right_sep}\\right{right}"
        )));
    }

    fn parse_func(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        if let Some(fname) = self.ns.m_child(el, "fName") {
            let name_toks = self.parse_children(fname, depth + 1);
            let raw = render_tokens(&name_toks);
            let trimmed = raw.trim();
            if is_known_func(trimmed) {
                out.push(Tok::Func(format!("\\{trimmed}")));
            } else {
                out.push(Tok::Func(raw));
            }
        }
        if let Some(e) = self.ns.m_child(el, "e") {
            out.extend(self.parse_children(e, depth + 1));
        }
    }

    fn parse_acc(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let chr = self
            .ns
            .m_child(el, "accPr")
            .and_then(|p| self.ns.m_child(p, "chr"))
            .and_then(|c| self.ns.m_attr(c, "val"))
            .unwrap_or("\u{0302}");
        let body = self
            .ns
            .m_child(el, "e")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        let cmd = match chr {
            "\u{0302}" | "^" => "\\hat",
            "\u{0303}" | "~" => "\\tilde",
            "\u{0304}" | "\u{00AF}" => "\\bar",
            "\u{0305}" => "\\overline",
            "\u{0307}" => "\\dot",
            "\u{0308}" => "\\ddot",
            "\u{030C}" => "\\check",
            "\u{0301}" => "\\acute",
            "\u{0300}" => "\\grave",
            "\u{0306}" => "\\breve",
            "\u{20D7}" | "\u{2192}" => "\\vec",
            _ => "\\hat",
        };
        out.push(Tok::Raw(format!("{cmd}{{{body}}}")));
    }

    fn parse_bar(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let pos = self
            .ns
            .m_child(el, "barPr")
            .and_then(|p| self.ns.m_child(p, "pos"))
            .and_then(|c| self.ns.m_attr(c, "val"))
            .unwrap_or("top");
        let body = self
            .ns
            .m_child(el, "e")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        let cmd = if pos == "bot" {
            "\\underline"
        } else {
            "\\overline"
        };
        out.push(Tok::Raw(format!("{cmd}{{{body}}}")));
    }

    fn parse_group_chr(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let pr = self.ns.m_child(el, "groupChrPr");
        let pos = pr
            .and_then(|p| self.ns.m_child(p, "pos"))
            .and_then(|c| self.ns.m_attr(c, "val"))
            .unwrap_or("bot");
        let body = self
            .ns
            .m_child(el, "e")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        let cmd = if pos == "top" {
            "\\overbrace"
        } else {
            "\\underbrace"
        };
        out.push(Tok::Raw(format!("{cmd}{{{body}}}")));
    }

    fn parse_lim_low(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let base = self
            .ns
            .m_child(el, "e")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        let lim = self
            .ns
            .m_child(el, "lim")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        if base.starts_with("\\lim") || base.starts_with("\\min") || base.starts_with("\\max") {
            out.push(Tok::Raw(format!("{base}_{{{lim}}}")));
        } else {
            out.push(Tok::Raw(format!("\\underset{{{lim}}}{{{base}}}")));
        }
    }

    fn parse_lim_upp(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let base = self
            .ns
            .m_child(el, "e")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        let lim = self
            .ns
            .m_child(el, "lim")
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .unwrap_or_default();
        out.push(Tok::Raw(format!("\\overset{{{lim}}}{{{base}}}")));
    }

    fn parse_matrix(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let mut rows = Vec::new();
        for mr in el.elements().filter(|c| self.ns.is_m(c, "mr")) {
            let cells: Vec<String> = mr
                .elements()
                .filter(|c| self.ns.is_m(c, "e"))
                .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
                .collect();
            rows.push(cells.join(" & "));
        }
        out.push(Tok::Raw(format!(
            "\\begin{{matrix}}\n{}\n\\end{{matrix}}",
            rows.join(" \\\\\n")
        )));
    }

    fn parse_eq_arr(&self, el: &XmlElement, depth: usize, out: &mut Vec<Tok>) {
        let rows: Vec<String> = el
            .elements()
            .filter(|c| self.ns.is_m(c, "e"))
            .map(|c| render_tokens(&self.parse_children(c, depth + 1)))
            .collect();
        out.push(Tok::Raw(format!(
            "\\begin{{aligned}}\n{}\n\\end{{aligned}}",
            rows.join(" \\\\\n")
        )));
    }
}

fn wrap_script_base(tokens: &[Tok]) -> String {
    let rendered = render_tokens(tokens);
    if tokens.len() == 1 {
        rendered
    } else {
        format!("{{{rendered}}}")
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

fn math_style_command(scr: Option<&str>, sty: Option<&str>) -> Option<&'static str> {
    match (scr, sty) {
        (Some("double-struck"), _) => Some("\\mathbb"),
        (Some("fraktur"), _) => Some("\\mathfrak"),
        (Some("script"), _) => Some("\\mathcal"),
        (Some("sans-serif"), Some("b")) => Some("\\mathsfbf"),
        (Some("sans-serif"), _) => Some("\\mathsf"),
        (Some("monospace"), _) => Some("\\mathtt"),
        (_, Some("b")) => Some("\\mathbf"),
        (_, Some("bi")) => Some("\\mathbfit"),
        (_, Some("p")) => Some("\\mathrm"),
        _ => None,
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

fn tokenize_to_vec(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    tokenize_math_text(text, &mut out);
    out
}

fn tokenize_math_text(text: &str, out: &mut Vec<Tok>) {
    let mut chars = text.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_whitespace() {
            chars.next();
            continue;
        }
        if ch.is_ascii_digit() {
            let mut num = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() || c == '.' {
                    num.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push(Tok::Num(num));
            continue;
        }
        chars.next();
        match ch {
            '+' | '-' | '\u{2212}' => {
                let s = if ch == '\u{2212}' {
                    "-"
                } else {
                    &ch.to_string()
                };
                out.push(Tok::Bin(s.to_owned()));
            }
            '×' => out.push(Tok::Bin("\\times".to_owned())),
            '÷' => out.push(Tok::Bin("\\div".to_owned())),
            '±' => out.push(Tok::Bin("\\pm".to_owned())),
            '∓' => out.push(Tok::Bin("\\mp".to_owned())),
            '·' | '⋅' => out.push(Tok::Bin("\\cdot".to_owned())),
            '∗' => out.push(Tok::Bin("\\ast".to_owned())),
            '∘' => out.push(Tok::Bin("\\circ".to_owned())),
            '⊕' => out.push(Tok::Bin("\\oplus".to_owned())),
            '⊗' => out.push(Tok::Bin("\\otimes".to_owned())),
            '∩' => out.push(Tok::Bin("\\cap".to_owned())),
            '∪' => out.push(Tok::Bin("\\cup".to_owned())),
            '∧' => out.push(Tok::Bin("\\land".to_owned())),
            '∨' => out.push(Tok::Bin("\\lor".to_owned())),
            '=' | '<' | '>' => out.push(Tok::Rel(ch.to_string())),
            '≤' => out.push(Tok::Rel("\\leq".to_owned())),
            '≥' => out.push(Tok::Rel("\\geq".to_owned())),
            '≠' => out.push(Tok::Rel("\\neq".to_owned())),
            '≈' => out.push(Tok::Rel("\\approx".to_owned())),
            '≡' => out.push(Tok::Rel("\\equiv".to_owned())),
            '≅' => out.push(Tok::Rel("\\cong".to_owned())),
            '∼' => out.push(Tok::Rel("\\sim".to_owned())),
            '∝' => out.push(Tok::Rel("\\propto".to_owned())),
            '∈' => out.push(Tok::Rel("\\in".to_owned())),
            '∉' => out.push(Tok::Rel("\\notin".to_owned())),
            '⊂' => out.push(Tok::Rel("\\subset".to_owned())),
            '⊃' => out.push(Tok::Rel("\\supset".to_owned())),
            '⊆' => out.push(Tok::Rel("\\subseteq".to_owned())),
            '⊇' => out.push(Tok::Rel("\\supseteq".to_owned())),
            '→' => out.push(Tok::Rel("\\rightarrow".to_owned())),
            '←' => out.push(Tok::Rel("\\leftarrow".to_owned())),
            '↔' => out.push(Tok::Rel("\\leftrightarrow".to_owned())),
            '⇒' => out.push(Tok::Rel("\\Rightarrow".to_owned())),
            '⇐' => out.push(Tok::Rel("\\Leftarrow".to_owned())),
            '⇔' => out.push(Tok::Rel("\\Leftrightarrow".to_owned())),
            '↦' => out.push(Tok::Rel("\\mapsto".to_owned())),
            ',' | ';' | ':' => out.push(Tok::Punct(ch.to_string())),
            '(' | '[' => out.push(Tok::Open(ch.to_string())),
            '{' => out.push(Tok::Open("\\{".to_owned())),
            ')' | ']' => out.push(Tok::Close(ch.to_string())),
            '}' => out.push(Tok::Close("\\}".to_owned())),
            other => {
                if let Some(cmd) = unicode_to_tex_symbol(other) {
                    out.push(Tok::Ident(cmd.to_owned()));
                } else {
                    out.push(Tok::Ident(other.to_string()));
                }
            }
        }
    }
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
        'ℜ' => Some("\\Re"),
        'ℑ' => Some("\\Im"),
        '℘' => Some("\\wp"),
        '…' => Some("\\ldots"),
        '⋯' => Some("\\cdots"),
        '⋮' => Some("\\vdots"),
        '⋱' => Some("\\ddots"),
        '′' => Some("'"),
        '″' => Some("''"),
        '°' => Some("^{\\circ}"),
        _ => None,
    }
}

fn render_tokens(tokens: &[Tok]) -> String {
    let mut out = String::new();
    for (idx, tok) in tokens.iter().enumerate() {
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
        if idx > 0 {
            let prev = &tokens[idx - 1];
            let need_space = match (prev, tok) {
                (_, Tok::Bin(_) | Tok::Rel(_)) => true,
                (Tok::Bin(_) | Tok::Rel(_), _) => true,
                (Tok::Punct(_), _) => true,
                // Avoid merging two backslash commands or command + letter (e.g. `\sin x`)
                (_, _) => {
                    let prev_ends_cmd = ends_with_cs_name(&out);
                    let next_starts_alpha =
                        s.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
                    prev_ends_cmd && next_starts_alpha
                }
            };
            if need_space && !out.ends_with(' ') {
                out.push(' ');
            }
        }
        out.push_str(s);
    }
    out
}

fn ends_with_cs_name(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = bytes.len();
    while i > 0 && bytes[i - 1].is_ascii_alphabetic() {
        i -= 1;
    }
    i < bytes.len() && i > 0 && bytes[i - 1] == b'\\'
}
