use crate::writer::escape;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ordinary,
    Variable,
    Open,
    Close,
    Binary,
    Relation,
    Punctuation,
    Function,
    Limits,
    Integral,
    Space,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Default,
    Bold,
    Italic,
    Roman,
}

pub(crate) fn render(out: &mut String, tex: &str, depth: usize) -> bool {
    let start = out.len();
    let mut input = tex;
    if sequence(out, &mut input, depth, false, Style::Default).is_some() && input.is_empty() {
        true
    } else {
        out.truncate(start);
        false
    }
}

fn sequence(
    out: &mut String,
    input: &mut &str,
    depth: usize,
    grouped: bool,
    style: Style,
) -> Option<()> {
    if depth == 0 {
        return None;
    }
    let mut previous = None;
    let mut pending_space = "";
    while let Some(c) = input.trim_start().chars().next() {
        *input = input.trim_start();
        if c == '}' {
            *input = &input[1..];
            return grouped.then_some(());
        }
        let start = out.len();
        let mut kind = atom(out, input, depth, style)?;
        let has_scripts = scripts(out, input, depth, kind, style)?;
        let space = match kind {
            Kind::Binary
                if matches!(
                    previous,
                    Some(Kind::Ordinary | Kind::Variable | Kind::Close | Kind::Function)
                ) && !input.is_empty()
                    && !input.starts_with(['}', ')', ']', ',', ';', '=', '<', '>', ':']) =>
            {
                r"\u8197 ?"
            }
            Kind::Binary => "",
            Kind::Relation if !input.is_empty() && !input.starts_with(['}', ')', ']']) => {
                r"\u8196 ?"
            }
            _ => "",
        };
        if !space.is_empty() {
            out.insert_str(start, space);
        }
        if kind == Kind::Binary && space.is_empty() {
            kind = Kind::Open;
        }
        if !pending_space.is_empty() && !matches!(kind, Kind::Close | Kind::Punctuation) {
            out.insert_str(start, pending_space);
        }
        pending_space = if !space.is_empty() {
            space
        } else if matches!(kind, Kind::Punctuation | Kind::Function) && !has_scripts {
            r"\u8198 ?"
        } else {
            ""
        };
        previous = Some(kind);
    }
    (!grouped).then_some(())
}

fn scripts(
    out: &mut String,
    input: &mut &str,
    depth: usize,
    kind: Kind,
    style: Style,
) -> Option<bool> {
    let mut sub = None;
    let mut sup = None;
    *input = input.trim_start();
    while let Some(script) = input.chars().next().filter(|c| matches!(c, '^' | '_')) {
        if kind == Kind::Limits {
            return None;
        }
        let target = if script == '^' { &mut sup } else { &mut sub };
        if target.is_some() {
            return None;
        }
        *input = input[1..].trim_start();
        let mut text = String::new();
        atom(&mut text, input, depth - 1, style)?;
        *target = Some(text);
        *input = input.trim_start();
    }
    for (control, value) in [("\\sub ", &sub), ("\\super ", &sup)] {
        if let Some(text) = value {
            out.push('{');
            out.push_str(control);
            out.push_str(text);
            out.push('}');
        }
    }
    Some(sub.is_some() || sup.is_some())
}

fn atom(out: &mut String, input: &mut &str, depth: usize, style: Style) -> Option<Kind> {
    use Kind::*;
    if depth == 0 {
        return None;
    }
    let source = *input;
    let c = input.chars().next()?;
    *input = &input[c.len_utf8()..];
    let mut buf = [0; 4];
    let (kind, text) = match c {
        '{' => {
            sequence(out, input, depth - 1, true, style)?;
            return Some(Ordinary);
        }
        '\\' => {
            let end = input
                .find(|c: char| !c.is_ascii_alphabetic())
                .unwrap_or(input.len());
            let word = &input[..end];
            *input = &input[end..];
            match word {
                "" => {
                    let symbol = input.chars().next()?;
                    *input = &input[symbol.len_utf8()..];
                    match symbol {
                        ',' => (Space, "\u{2006}"),
                        ';' => (Space, "\u{2005}"),
                        ':' | ' ' => (Space, "\u{a0}"),
                        '!' => (Space, "\u{200a}"),
                        '{' => (Open, "{"),
                        '}' => (Close, "}"),
                        _ => return None,
                    }
                }
                "left" | "right" => {
                    *input = input.trim_start();
                    let delimiter = [
                        "(", ")", "[", "]", "|", ".", "\\{", "\\}", "\\langle", "\\rangle",
                    ]
                    .iter()
                    .find(|&&delimiter| input.starts_with(delimiter))?;
                    *input = &input[delimiter.len()..];
                    let text = match *delimiter {
                        "." => "",
                        "\\{" => "{",
                        "\\}" => "}",
                        "\\langle" => "⟨",
                        "\\rangle" => "⟩",
                        other => other,
                    };
                    (if word == "left" { Open } else { Close }, text)
                }
                "mathbf" | "mathit" | "mathrm" => {
                    let style = match word {
                        "mathbf" => Style::Bold,
                        "mathit" => Style::Italic,
                        _ => Style::Roman,
                    };
                    *input = input.trim_start();
                    return atom(out, input, depth - 1, style);
                }
                _ => command(word)?,
            }
        }
        'a'..='z' | 'A'..='Z' => (Variable, &*c.encode_utf8(&mut buf)),
        '-' | '−' => (Binary, "−"),
        '+' | '*' => (Binary, &*c.encode_utf8(&mut buf)),
        '=' | '<' | '>' | ':' => (Relation, &*c.encode_utf8(&mut buf)),
        ',' | ';' => (Punctuation, &*c.encode_utf8(&mut buf)),
        '(' | '[' => (Open, &*c.encode_utf8(&mut buf)),
        ')' | ']' | '!' => (Close, &*c.encode_utf8(&mut buf)),
        '0'..='9' if style != Style::Default => {
            let digits = |text: &str| text.bytes().take_while(u8::is_ascii_digit).count();
            let mut end = digits(source);
            if let Some(fraction) = source[end..].strip_prefix('.') {
                let count = digits(fraction);
                if count > 0 {
                    end += 1 + count;
                }
            }
            *input = &source[end..];
            (Ordinary, &source[..end])
        }
        '0'..='9' | '.' | '/' | '|' => (Ordinary, &*c.encode_utf8(&mut buf)),
        _ => return None,
    };
    let control = match style {
        Style::Bold if kind != Space => Some("{\\b "),
        Style::Italic if kind != Space => Some("{\\i "),
        Style::Default if kind == Variable => Some("{\\i "),
        _ => None,
    };
    if let Some(control) = control {
        out.push_str(control);
        escape(out, text);
        out.push('}');
    } else {
        escape(out, text);
    }
    Some(kind)
}

fn command(word: &str) -> Option<(Kind, &str)> {
    use Kind::*;
    Some(match word {
        "alpha" => (Variable, "α"),
        "beta" => (Variable, "β"),
        "gamma" => (Variable, "γ"),
        "delta" => (Variable, "δ"),
        "epsilon" => (Variable, "ϵ"),
        "varepsilon" => (Variable, "ε"),
        "zeta" => (Variable, "ζ"),
        "eta" => (Variable, "η"),
        "theta" => (Variable, "θ"),
        "vartheta" => (Variable, "ϑ"),
        "iota" => (Variable, "ι"),
        "kappa" => (Variable, "κ"),
        "lambda" => (Variable, "λ"),
        "mu" => (Variable, "μ"),
        "nu" => (Variable, "ν"),
        "xi" => (Variable, "ξ"),
        "pi" => (Variable, "π"),
        "varpi" => (Variable, "ϖ"),
        "rho" => (Variable, "ρ"),
        "varrho" => (Variable, "\u{1d71a}"),
        "sigma" => (Variable, "σ"),
        "varsigma" => (Variable, "\u{1d70d}"),
        "tau" => (Variable, "τ"),
        "upsilon" => (Variable, "υ"),
        "phi" => (Variable, "ϕ"),
        "varphi" => (Variable, "φ"),
        "chi" => (Variable, "χ"),
        "psi" => (Variable, "ψ"),
        "omega" => (Variable, "ω"),
        "Gamma" => (Variable, "Γ"),
        "Delta" => (Variable, "Δ"),
        "Theta" => (Variable, "Θ"),
        "Lambda" => (Variable, "Λ"),
        "Xi" => (Variable, "Ξ"),
        "Pi" => (Variable, "Π"),
        "Sigma" => (Variable, "Σ"),
        "Upsilon" => (Variable, "Υ"),
        "Phi" => (Variable, "Φ"),
        "Psi" => (Variable, "Ψ"),
        "Omega" => (Variable, "Ω"),
        "ell" => (Variable, "ℓ"),
        "aleph" => (Variable, "ℵ"),
        "imath" => (Variable, "ı"),
        "jmath" => (Variable, "ȷ"),
        "wedge" | "land" => (Binary, "∧"),
        "vee" | "lor" => (Binary, "∨"),
        "times" => (Binary, "×"),
        "div" => (Binary, "÷"),
        "cdot" => (Binary, "⋅"),
        "pm" => (Binary, "±"),
        "mp" => (Binary, "∓"),
        "ast" => (Binary, "*"),
        "star" => (Binary, "⋆"),
        "circ" => (Binary, "∘"),
        "bullet" => (Binary, "•"),
        "cap" => (Binary, "∩"),
        "cup" => (Binary, "∪"),
        "mid" => (Binary, "∣"),
        "oplus" => (Binary, "⊕"),
        "otimes" => (Binary, "⊗"),
        "setminus" => (Binary, "\\"),
        "le" | "leq" | "leqslant" => (Relation, "≤"),
        "ge" | "geq" | "geqslant" => (Relation, "≥"),
        "ne" | "neq" => (Relation, "≠"),
        "equiv" => (Relation, "≡"),
        "approx" => (Relation, "≈"),
        "sim" => (Relation, "∼"),
        "simeq" => (Relation, "≃"),
        "cong" => (Relation, "≅"),
        "propto" => (Relation, "∝"),
        "in" => (Relation, "∈"),
        "ni" => (Relation, "∋"),
        "notin" => (Relation, "∉"),
        "subset" => (Relation, "⊂"),
        "subseteq" => (Relation, "⊆"),
        "supset" => (Relation, "⊃"),
        "supseteq" => (Relation, "⊇"),
        "parallel" => (Relation, "∥"),
        "perp" => (Relation, "⟂"),
        "models" => (Relation, "⊨"),
        "vdash" => (Relation, "⊢"),
        "to" | "rightarrow" => (Relation, "→"),
        "gets" | "leftarrow" => (Relation, "←"),
        "leftrightarrow" => (Relation, "↔"),
        "Rightarrow" => (Relation, "⇒"),
        "Leftarrow" => (Relation, "⇐"),
        "Leftrightarrow" => (Relation, "⇔"),
        "mapsto" => (Relation, "↦"),
        "langle" => (Open, "⟨"),
        "rangle" => (Close, "⟩"),
        "infty" => (Ordinary, "∞"),
        "nabla" => (Ordinary, "∇"),
        "partial" => (Ordinary, "∂"),
        "hbar" => (Ordinary, "ℏ"),
        "Re" => (Ordinary, "ℜ"),
        "Im" => (Ordinary, "ℑ"),
        "emptyset" => (Ordinary, "∅"),
        "varnothing" => (Ordinary, "⌀"),
        "forall" => (Ordinary, "∀"),
        "exists" => (Ordinary, "∃"),
        "neg" | "lnot" => (Ordinary, "¬"),
        "top" => (Ordinary, "⊤"),
        "bot" => (Ordinary, "⊥"),
        "angle" => (Ordinary, "∠"),
        "prime" => (Ordinary, "′"),
        "dots" | "ldots" => (Ordinary, "…"),
        "cdots" => (Ordinary, "⋯"),
        "vdots" => (Ordinary, "⋮"),
        "ddots" => (Relation, "⋱"),
        "int" => (Integral, "∫"),
        "iint" => (Integral, "∬"),
        "oint" => (Integral, "∮"),
        "quad" => (Space, "\u{200a}\u{2001}"),
        "qquad" => (Space, "\u{200a}\u{2001}\u{2001}"),
        "sum" => (Limits, "∑"),
        "prod" => (Limits, "∏"),
        "bigcap" => (Limits, "⋂"),
        "bigcup" => (Limits, "⋃"),
        "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "ln" | "log" | "exp" | "lim" | "min"
        | "max" | "det" | "gcd" | "arcsin" | "arccos" | "arctan" => (Function, word),
        // Fractions and roots cannot render as Pandoc's inline math.
        _ => return None,
    })
}
