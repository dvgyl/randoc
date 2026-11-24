//! Converts the shared TeX expression tree to StarMath.

use std::fmt::Write as _;

use crate::render::{
    Unsupported, column_alignment, has_limits, prime_count, variant_char, variant_chars,
};
use crate::starmath_symbols::{delimiter, greek, named};
use crate::tree::{Alphabet, Class, Exp, Wrap};

pub(crate) fn render(items: &[Exp], display: bool) -> Result<String, Unsupported> {
    list(items, display)
}

fn list(items: &[Exp], display: bool) -> Result<String, Unsupported> {
    let fences = paired_fences(items);
    let mut out = String::new();
    let mut after = false;
    for (index, item) in items.iter().enumerate() {
        let (value, before, next) = if let Some((left, text)) = fences[index] {
            let direction = if left { "left" } else { "right" };
            (format!("{direction} {}", delimiter(text, left)), true, true)
        } else {
            let (before, next) = spacing(item);
            (expression(item, display)?, before, next)
        };
        if value.is_empty() {
            if matches!(item, Exp::Space(space) if !matches!(space.width, "-0.167em" | "0.333em"))
                && !out.is_empty()
            {
                after = true;
            }
            continue;
        }
        if (before || after)
            && !out.is_empty()
            && !out.ends_with(['(', '['])
            && !value.starts_with(['(', '[', ')', ']', '}', ',', ';'])
        {
            out.push(' ');
        }
        if let Exp::Sym(class, text) = item
            && infix(*class, text)
            && fences[index].is_none()
            && !(items.len() == 1 && text == "*")
        {
            if out.is_empty() && !matches!(text.as_str(), "+" | "−") {
                out.push_str("{} ");
            }
            out.push_str(&value);
            if index + 1 == items.len() {
                out.push_str(" {}");
            }
        } else {
            out.push_str(&value);
        }
        after = next;
    }
    Ok(out)
}

fn paired_fences(items: &[Exp]) -> Vec<Option<(bool, &str)>> {
    let mut matches = vec![None; items.len()];
    let mut braces = Vec::new();
    let mut bar = None;
    for (index, item) in items.iter().enumerate() {
        let Exp::Sym(_, symbol) = item else { continue };
        let opening = match symbol.as_str() {
            "{" => {
                braces.push(index);
                continue;
            }
            "}" => braces.pop().map(|start| (start, "{")),
            "∥" => match bar.take() {
                Some(start) => Some((start, "∥")),
                None => {
                    bar = Some(index);
                    continue;
                }
            },
            _ => None,
        };
        if let Some((start, left)) = opening {
            matches[start] = Some((true, left));
            matches[index] = Some((false, symbol.as_str()));
        }
    }
    matches
}

fn infix(class: Class, text: &str) -> bool {
    class == Class::Rel && text != "∣"
        || class == Class::Bin && (named(text).is_some() || matches!(text, "+" | "−" | "*"))
}

fn spacing(item: &Exp) -> (bool, bool) {
    match item {
        Exp::Sym(class, text) if infix(*class, text) || text == "/" => (true, true),
        Exp::Sym(Class::Punct, _) => (false, true),
        Exp::Sym(Class::Func | Class::FuncLimits, _) => (true, true),
        Exp::Sym(_, text) if greek(text).is_some() => (false, true),
        Exp::Sym(_, text) if named(text).is_some() => (true, true),
        Exp::Scripts { .. } => (false, true),
        Exp::Fenced { .. } => (true, true),
        Exp::Sqrt(_) | Exp::Root(_, _) | Exp::Accent { .. } | Exp::Phantom(_) => (true, false),
        Exp::Space(_) => (true, true),
        _ => (false, false),
    }
}

fn expression(item: &Exp, display: bool) -> Result<String, Unsupported> {
    Ok(match item {
        Exp::Num(value) => value.clone(),
        Exp::Sym(Class::Func | Class::FuncLimits, text) => function(text),
        Exp::Sym(_, text) => symbol(text),
        Exp::Group(items) => {
            if let [Exp::Space(_), Exp::Sym(Class::Open, open), tail @ ..] = items.as_slice()
                && open == "("
            {
                list(tail, display).map(|body| format!("({body}"))?
            } else if matches!(items.as_slice(), [Exp::Space(_), Exp::Sym(Class::Func, name), ..] if name == "mod")
            {
                format!("{{ {}}}", list(items, display)?)
            } else {
                braced(list(items, display)?)
            }
        }
        Exp::PaddedText(items) => {
            let text: Vec<_> = items
                .iter()
                .filter(|item| !matches!(item, Exp::Space(_)))
                .cloned()
                .collect();
            list(&text, display)?
        }
        Exp::Scripts {
            base,
            sub,
            sup,
            limits,
        } => scripts(base, sub.as_deref(), sup.as_deref(), *limits, display)?,
        Exp::Frac { num, den, line } => {
            if *line {
                let num = expression(num, display)?;
                let den = expression(den, display)?;
                let size = if display { "" } else { "size*0.7 " };
                format!("{size}{{{num} over {den}}}")
            } else {
                format!("binom{{{}}}{{{}}}", tex_fragment(num)?, tex_fragment(den)?)
            }
        }
        Exp::Sqrt(body) => format!("sqrt {}", braced(expression(body, display)?)),
        Exp::Root(body, index) => format!(
            "nroot {} {}",
            braced(expression(index, display)?),
            braced(expression(body, display)?)
        ),
        Exp::Fenced { open, body, close } => format!(
            "left {} {} right {}",
            delimiter(open.as_deref().unwrap_or_default(), true),
            list(body, display)?,
            delimiter(close.as_deref().unwrap_or_default(), false),
        ),
        Exp::Middle(text) => format!("mline {}", delimiter(text, true)),
        Exp::Accent { body, mark } => {
            let name = accent(mark.mathml).ok_or(Unsupported)?;
            let body_text = argument(body, display)?;
            if matches!(name, "underbrace" | "overbrace") {
                format!("{body_text} {name}")
            } else {
                format!("{name} {body_text}")
            }
        }
        Exp::Stack { base, under, over } => {
            if let Some(over) = over {
                let over = tex_fragment(over)?;
                let base = tex_fragment(base)?;
                let above = format!("\\overset{{{over}}}{{{base}}}");
                if let Some(under) = under {
                    format!("\\underset{{{}}}{{{above}}}", tex_fragment(under)?)
                } else {
                    above
                }
            } else {
                scripts(base, under.as_deref(), None, Some(false), display)?
            }
        }
        Exp::Space(space) => match space.width {
            "1.0em" => "~",
            "2.0em" => "~~",
            _ => "",
        }
        .into(),
        Exp::Text(style, text) => match style.map(|style| style.wrap) {
            Some(Wrap::Strong) => format!("bold {}", braced(quote(text))),
            Some(Wrap::Emph) if text.chars().count() > 1 => format!("ital {{{text}}}"),
            Some(Wrap::Emph) => format!("ital {text}"),
            _ => quote(text),
        },
        Exp::Styled(style, items) => styled(style, items, display)?,
        Exp::Limits(body) | Exp::Sized(_, body) => expression(body, display)?,
        Exp::Display(on, body) => expression(body, *on)?,
        Exp::Enclose(notation, body) => {
            let command = match *notation {
                "box" => "boxed",
                "updiagonalstrike" => "cancel",
                "downdiagonalstrike" => "bcancel",
                "updiagonalstrike downdiagonalstrike" => "xcancel",
                _ => return Err(Unsupported),
            };
            format!("\\{command}{{{}}}", tex_fragment(body)?)
        }
        Exp::Phantom(body) => format!("phantom {}", argument(body, display)?),
        Exp::Table { aligns, rows, .. } => {
            let mut rendered = Vec::with_capacity(rows.len());
            for row in rows {
                let mut cells = Vec::with_capacity(row.len());
                for (column, cell) in row.iter().enumerate() {
                    let align = match column_alignment(aligns, column, rows) {
                        "left" => "alignl ",
                        "right" => "alignr ",
                        _ => "",
                    };
                    cells.push(format!("{align}{}", list(cell, display)?));
                }
                rendered.push(cells.join(" # "));
            }
            format!("matrix {{ {} }}", rendered.join(" ## "))
        }
    })
}

fn styled(style: &Alphabet, items: &[Exp], display: bool) -> Result<String, Unsupported> {
    let body = list(items, display)?;
    if style.ident == "normal" && items.len() > 1 {
        if items
            .iter()
            .all(|item| matches!(item, Exp::Num(_) | Exp::Sym(..)))
        {
            let raw: String = items
                .iter()
                .map(|item| match item {
                    Exp::Num(text) | Exp::Sym(_, text) => text.as_str(),
                    _ => "",
                })
                .collect();
            return Ok(format!("\"{raw}\""));
        }
        return Ok(format!("nitalic{{{body}}}"));
    }
    if style.inline_chars {
        let pairs = variant_chars(style.ident);
        let alphabet: Option<String> = items
            .iter()
            .map(|item| {
                let value = match item {
                    Exp::Sym(Class::Ident, value) | Exp::Num(value) => value,
                    _ => return None,
                };
                value
                    .chars()
                    .map(|plain| {
                        let styled = variant_char(pairs, plain);
                        (plain != styled).then_some(styled)
                    })
                    .collect::<Option<String>>()
            })
            .collect();
        if let Some(alphabet) = alphabet {
            let alphabet = if alphabet.chars().count() > 1 {
                braced(alphabet)
            } else {
                alphabet
            };
            return Ok(format!("nitalic {alphabet}"));
        }
        let prefix = match style.ident {
            "script" => "ital",
            "double-struck" => "bold nitalic",
            _ => "bold",
        };
        return Ok(format!("{prefix} {{{body}}}"));
    }
    let greek = body.starts_with('%');
    let prefix = match style.ident {
        "italic" => "ital",
        "bold-italic" if !style.upright_bold && greek => "bold",
        "bold-italic" | "bold" => "bold nitalic",
        "sans-serif" => "font sans nitalic",
        "monospace" => "font fixed nitalic",
        _ => "nitalic",
    };
    let group = items.len() > 1 || greek || matches!(items, [Exp::Scripts { .. }]);
    let body = if group { braced(body) } else { body };
    Ok(format!("{prefix} {body}"))
}

fn scripts(
    base: &Exp,
    sub: Option<&Exp>,
    sup: Option<&Exp>,
    limits: Option<bool>,
    display: bool,
) -> Result<String, Unsupported> {
    if let Exp::Accent { body, mark } = base
        && matches!(mark.mathml, "⏞" | "⏟")
    {
        let base = expression(body, display)?;
        let (name, label) = if mark.under {
            ("underbrace", sub)
        } else {
            ("overbrace", sup)
        };
        if let Some(label) = label {
            return Ok(format!("{base} {name} {}", argument(label, display)?));
        }
    }
    let limits = limits.unwrap_or_else(|| display && has_limits(base));
    let mut out = if matches!(base, Exp::Group(_)) {
        expression(base, display)?
    } else {
        argument(base, display)?
    };
    if limits {
        let (name, centered) = match base {
            Exp::Sym(_, text) => match text.as_str() {
                "∑" => ("sum", false),
                "∏" => ("prod", false),
                "∐" => ("coprod", false),
                "⋃" => ("oper ∪", false),
                "⋂" => ("oper ∩", false),
                "∫" => ("int", false),
                "lim" => ("lim", false),
                _ => ("", true),
            },
            _ => ("", true),
        };
        if centered {
            if out.contains(' ') {
                out = braced(out);
            }
        } else {
            out = name.into();
        }
        for (script, place, center) in [(sub, "from", "csub"), (sup, "to", "csup")] {
            if let Some(script) = script {
                if centered {
                    let _ = write!(out, " {center} {}", braced(unbraced(script, display)?));
                } else {
                    let _ = write!(out, " {place} {}", unbraced(script, display)?);
                }
            }
        }
    } else {
        if let Some(sub) = sub {
            let _ = write!(out, "_{}", argument(sub, display)?);
        }
        if let Some(sup) = sup {
            if let Exp::Sym(_, text) = sup
                && prime_count(text) != 0
            {
                out.push_str(&"'".repeat(prime_count(text)));
            } else {
                let _ = write!(out, "^{}", argument(sup, display)?);
            }
        }
    }
    Ok(out)
}

fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\\\""))
}

fn braced(body: String) -> String {
    format!("{{{body}}}")
}

fn unbraced(item: &Exp, display: bool) -> Result<String, Unsupported> {
    if let Exp::Group(items) = item {
        list(items, display)
    } else {
        expression(item, display)
    }
}

fn argument(item: &Exp, display: bool) -> Result<String, Unsupported> {
    let body = expression(item, display)?;
    if matches!(item, Exp::Group(_) | Exp::Scripts { .. }) {
        Ok(braced(body))
    } else {
        Ok(body)
    }
}

fn function(text: &str) -> String {
    if matches!(
        text,
        "sin"
            | "cos"
            | "tan"
            | "cot"
            | "sinh"
            | "cosh"
            | "tanh"
            | "coth"
            | "arcsin"
            | "arccos"
            | "arctan"
            | "exp"
            | "ln"
            | "log"
    ) {
        text.into()
    } else {
        format!("func {text}")
    }
}

fn symbol(text: &str) -> String {
    named(text)
        .map(str::to_owned)
        .or_else(|| greek(text))
        .unwrap_or_else(|| {
            match text {
                "−" => "-",
                "∗" => "*",
                "∖" => "\\",
                other => other,
            }
            .into()
        })
}

fn accent(mark: &str) -> Option<&'static str> {
    Some(match mark {
        "\u{0302}" => "hat",
        "\u{203e}" => "bar",
        "\u{2192}" => "vec",
        "\u{0303}" => "tilde",
        "\u{0307}" => "dot",
        "\u{0308}" => "ddot",
        "\u{0301}" => "acute",
        "\u{0300}" => "grave",
        "\u{030c}" => "check",
        "\u{0306}" => "breve",
        "\u{00af}" => "overline",
        "_" => "underline",
        "⏞" => "overbrace",
        "⏟" => "underbrace",
        _ => return None,
    })
}

fn tex_fragment(item: &Exp) -> Result<String, Unsupported> {
    match item {
        Exp::Num(value) => Ok(value.clone()),
        Exp::Sym(_, value) => match value.as_str() {
            "−" => Ok("-".into()),
            "#" | "$" | "%" | "&" | "_" | "{" | "}" => Ok(format!("\\{value}")),
            _ if value.is_ascii() => Ok(value.clone()),
            _ => crate::symbols::SYMBOLS
                .iter()
                .find(|(_, _, mathml, _)| mathml == value)
                .map(|(name, ..)| format!("\\{name}"))
                .ok_or(Unsupported),
        },
        Exp::Group(items) => items.iter().map(tex_fragment).collect(),
        Exp::Frac { num, den, line } => {
            let command = if *line { "frac" } else { "binom" };
            Ok(format!(
                "\\{command}{{{}}}{{{}}}",
                tex_fragment(num)?,
                tex_fragment(den)?
            ))
        }
        Exp::Scripts { base, sub, sup, .. } => {
            let mut out = tex_fragment(base)?;
            for (argument, marker) in [(sub, '_'), (sup, '^')] {
                if let Some(argument) = argument {
                    let _ = write!(out, "{marker}{{{}}}", tex_fragment(argument)?);
                }
            }
            Ok(out)
        }
        _ => Err(Unsupported),
    }
}
