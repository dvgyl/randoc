// These Org entity mappings have no identical HTML entity.
const ENTITIES: &[(&str, &str)] = &[
    ("DotDot", " \u{20dc}"),
    ("DownBreve", " \u{311}"),
    ("TripleDot", " \u{20db}"),
    ("backslash", "\u{2216}"),
    ("cdots", "\u{22ef}"),
    ("dag", "\u{2020}"),
    ("ddag", "\u{2021}"),
    ("dots", "\u{2026}"),
    ("dotsb", "\u{22ef}"),
    ("dotsc", "\u{2026}"),
    ("dotso", "\u{2026}"),
    ("epsilon", "\u{3f5}"),
    ("exists", "\u{2203}"),
    ("hdots", "\u{2026}"),
    ("iint", "\u{222c}"),
    ("infty", "\u{221e}"),
    ("land", "\u{2227}"),
    ("ldots", "\u{2026}"),
    ("lnot", "\u{ac}"),
    ("lor", "\u{2228}"),
    ("neg", "\u{ac}"),
    ("oe", "\u{153}"),
    ("partial", "\u{2202}"),
    ("slash", "/"),
    ("smileface", "\u{263a}"),
    ("smiley", "\u{263a}"),
    ("sphericalangle", "\u{2222}"),
    ("tdot", " \u{20db}"),
    ("textasciicircum", "^"),
    ("textasciitilde", "~"),
    ("textbackslash", "\u{5c}"),
    ("varepsilon", "\u{3b5}"),
    ("vdots", "\u{22ee}"),
];

pub(super) fn get(name: &str) -> Option<&'static str> {
    let index = ENTITIES.binary_search_by_key(&name, |&(key, _)| key);
    match index {
        Ok(index) => Some(ENTITIES[index].1),
        Err(_) if name.len() <= 32 => randoc_core::shared::named_entity(&format!("{name};")),
        Err(_) => None,
    }
}

pub(super) fn latex_function(name: &str) -> bool {
    matches!(
        name,
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

pub(super) fn latex_relation(name: &str) -> Option<&'static str> {
    Some(match name {
        "iff" => "⇔",
        "implies" => "⟹",
        "impliedby" => "⟸",
        "in" => "∈",
        "le" | "leq" => "≤",
        "ge" | "geq" => "≥",
        "neq" | "ne" => "≠",
        "to" | "rightarrow" => "→",
        "leftarrow" => "←",
        "longrightarrow" => "⟶",
        "longleftarrow" => "⟵",
        _ => return None,
    })
}

pub(super) fn latex_digit_entity(name: &str) -> Option<(&'static str, bool)> {
    let value = get(name)?;
    match name {
        "aleph" | "ell" => Some((value, true)),
        "times" | "pm" | "mp" | "infty" | "nabla" | "Re" | "Im" | "partial" | "exists"
        | "forall" | "neg" | "cdot" | "div" | "cup" | "cap" => Some((value, false)),
        "ldots" | "dots" | "cdots" => None,
        _ if value.chars().next().is_some_and(|c| matches!(c, 'Α'..='Ͽ')) => Some((value, true)),
        _ => None,
    }
}
