//! Symbol spellings observed in Pandoc 3.13 output.

pub(crate) fn name(text: &str) -> Option<&'static str> {
    Some(match text {
        "\\\\" => "\\",
        "{" => "\\[lC]",
        "}" => "\\[rC]",
        "±" => "+-",
        "·" => "cdot",
        "×" => "times",
        "Γ" => "GAMMA",
        "Δ" => "DELTA",
        "Θ" => "THETA",
        "Λ" => "LAMBDA",
        "Ξ" => "XI",
        "Π" => "PI",
        "Σ" => "SIGMA",
        "Υ" => "UPSILON",
        "Φ" => "PHI",
        "Ψ" => "PSI",
        "Ω" => "OMEGA",
        "α" => "alpha",
        "β" => "beta",
        "γ" => "gamma",
        "δ" => "delta",
        "ζ" => "zeta",
        "η" => "eta",
        "θ" => "theta",
        "ι" => "iota",
        "κ" => "kappa",
        "λ" => "lambda",
        "μ" => "mu",
        "ν" => "nu",
        "ξ" => "xi",
        "π" => "pi",
        "ρ" => "rho",
        "σ" => "sigma",
        "τ" => "tau",
        "υ" => "upsilon",
        "φ" => "varphi",
        "χ" => "chi",
        "ψ" => "psi",
        "ω" => "omega",
        "ϕ" => "phi",
        "ϵ" => "epsilon",
        "…" => "...",
        "′" => "prime",
        "←" => "<-",
        "→" => "->",
        "∂" => "partial",
        "∇" => "grad",
        "∏" => "prod",
        "∑" => "sum",
        "−" => "-",
        "∞" => "inf",
        "∫" => "int",
        "≈" => "approx",
        "≠" => "!=",
        "≡" => "==",
        "≤" => "<=",
        "≥" => ">=",
        "≪" => "<<",
        "≫" => ">>",
        "⋂" => "union",
        "⋃" => "inter",
        "⋯" => "cdots",
        _ => return None,
    })
}

pub(crate) fn builtin_function(text: &str) -> bool {
    matches!(
        text,
        "cos"
            | "cosh"
            | "exp"
            | "lim"
            | "ln"
            | "log"
            | "max"
            | "min"
            | "sin"
            | "sinh"
            | "tan"
            | "tanh"
    )
}
