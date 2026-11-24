//! Names for symbols that StarMath writes as commands.

pub(crate) fn named(text: &str) -> Option<&'static str> {
    Some(match text {
        "·" | "⋅" => "cdot",
        "×" => "times",
        "÷" => "div",
        "∘" => "circ",
        "±" => "plusminus",
        "∓" => "minusplus",
        "≤" => "<=",
        "≥" => ">=",
        "≠" => "<>",
        "≈" => "approx",
        "≡" => "equiv",
        "∈" => "in",
        "∉" => "notin",
        "⊂" => "subset",
        "⊆" => "subseteq",
        "⊃" => "supset",
        "⊇" => "supseteq",
        "∪" => "union",
        "∩" => "intersection",
        "∧" => "and",
        "∨" => "or",
        "→" => "toward",
        "⇒" => "drarrow",
        "⇐" => "dlarrow",
        "⇔" => "dlrarrow",
        "↔" => "leftrightarrow",
        "∥" => "parallel",
        "∝" => "prop",
        "≪" => "ll",
        "≫" => "gg",
        "∞" => "infinity",
        "∂" => "partial",
        "∇" => "nabla",
        "∀" => "forall",
        "∃" => "exists",
        "¬" => "neg",
        "∅" => "emptyset",
        "…" => "dotslow",
        "⋯" => "dotsaxis",
        "⋮" => "dotsvert",
        "⋱" => "dotsdown",
        "∑" => "sum",
        "∫" => "int",
        _ => return None,
    })
}

pub(crate) fn greek(text: &str) -> Option<String> {
    const NAMES: [&str; 24] = [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
        "lambda", "mu", "nu", "xi", "omicron", "pi", "rho", "sigma", "tau", "upsilon", "phi",
        "chi", "psi", "omega",
    ];
    let mut letters = text.chars();
    let letter = letters.next()?;
    if letters.next().is_some() {
        return None;
    }
    match letter {
        'ε' => Some("%ivarepsilon".into()),
        'φ' => Some("%ivarphi".into()),
        'α'..='ω' if letter != 'ς' => {
            let index = (letter as usize - 'α' as usize) - usize::from(letter > 'ς');
            Some(format!("%i{}", NAMES[index]))
        }
        'Α'..='Ω' if letter != '\u{03a2}' => {
            let index = (letter as usize - 'Α' as usize) - usize::from(letter > '\u{03a2}');
            Some(format!("%{}", NAMES[index].to_ascii_uppercase()))
        }
        _ => Some(
            match letter {
                'ϵ' => "%iepsilon",
                'ϑ' => "%ivartheta",
                'ϕ' => "%iphi",
                'ϖ' => "%ivarpi",
                'ϱ' => "%ivarrho",
                'ς' => "%ivarsigma",
                _ => return None,
            }
            .into(),
        ),
    }
}

pub(crate) fn delimiter(text: &str, left: bool) -> &str {
    match text {
        "" => "none",
        "{" => "lbrace",
        "}" => "rbrace",
        "⟨" => "langle",
        "⟩" => "rangle",
        "|" | "∣" if left => "lline",
        "|" | "∣" => "rline",
        "∥" if left => "ldline",
        "∥" => "rdline",
        _ => text,
    }
}
