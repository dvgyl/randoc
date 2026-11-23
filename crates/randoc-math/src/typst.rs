//! Typst math output.

use crate::{
    render::{Unsupported, accent_char, prime_count},
    tree::{Aligns, Class, Exp, Row},
};

pub(crate) fn render(items: &[Exp], _display: bool) -> Result<String, Unsupported> {
    Ok(list(items))
}

fn list(items: &[Exp]) -> String {
    let mut output = String::new();
    let mut previous = None;
    let mut previous_is_tight = false;
    for item in items {
        if matches!(item, Exp::Space(space) if space.width == "0.333em") {
            continue;
        }
        let override_base = match (item, previous) {
            (Exp::Scripts { base, .. }, Some(previous)) if matches!(base.as_ref(), Exp::Sym(_, text) if text.is_empty()) => {
                Some(if ends_in_prime(previous) { "\"\"" } else { "" })
            }
            _ => None,
        };
        let value = match (item, override_base) {
            (Exp::Scripts { base, sub, sup, .. }, Some(base_text)) => {
                scripts(base, sub.as_deref(), sup.as_deref(), Some(base_text))
            }
            _ => expression(item),
        };
        if previous.is_some()
            && override_base != Some("")
            && !starts_tight(&value)
            && !previous_is_tight
        {
            output.push(' ');
        }
        output.push_str(&value);
        previous_is_tight = ends_tight(&value)
            && !(matches!(item, Exp::Fenced { .. }) && value.ends_with([')', ']']));
        previous = Some(item);
    }
    output
}

fn starts_tight(value: &str) -> bool {
    value.starts_with('\'') || value.strip_prefix('\\').is_some_and(escaped_tight)
}

fn ends_tight(value: &str) -> bool {
    let mut chars = value.chars().rev();
    matches!(chars.next(), Some(c) if "()[]|,;/$#_^".contains(c)) && chars.next() == Some('\\')
}

fn escaped_tight(value: &str) -> bool {
    value.starts_with(|c| "()[]|,;/$#_^".contains(c))
}

fn ends_in_prime(item: &Exp) -> bool {
    match item {
        Exp::Scripts {
            sup: Some(prime),
            sub: None,
            ..
        } => ends_in_prime(prime),
        Exp::Sym(_, text) => prime_count(text) > 0,
        _ => false,
    }
}

fn expression(exp: &Exp) -> String {
    match exp {
        Exp::Num(text) => text.clone(),
        Exp::Sym(Class::Text, text) => format!("upright({})", quote(text)),
        Exp::Sym(Class::Func | Class::FuncLimits, name) if !builtin_function(name) => quote(name),
        Exp::Sym(_, text) => symbol(text),
        Exp::Group(items) | Exp::PaddedText(items) => list(items),
        Exp::Scripts { base, sub, sup, .. } => scripts(base, sub.as_deref(), sup.as_deref(), None),
        Exp::Frac {
            num,
            den,
            line: true,
        } => {
            let short = !compound(num) && !compound(den);
            let (num, den) = (expression(num), expression(den));
            if short {
                format!("{num} / {den}")
            } else {
                format!("frac({num}, {den})")
            }
        }
        Exp::Frac {
            num,
            den,
            line: false,
        } => format!(
            "mat(delim: #none, {}; {})",
            expression(num),
            expression(den),
        ),
        Exp::Sqrt(body) => format!("sqrt({})", expression(body)),
        Exp::Root(body, index) => format!("root({}, {})", expression(index), expression(body)),
        Exp::Fenced { open, body, close } => fenced(open.as_deref(), body, close.as_deref()),
        Exp::Middle(text) => {
            let delimiter = if matches!(text.as_str(), "|" | "∣") {
                "bar.v".into()
            } else {
                symbol(text)
            };
            format!("mid({delimiter})")
        }
        Exp::Accent { body, mark } => accent(body, mark.mathml),
        Exp::Stack { base, under, over } => scripts(base, under.as_deref(), over.as_deref(), None),
        Exp::Space(space) => match space.width {
            "0.167em" => "thin".into(),
            "0.222em" => "med".into(),
            "0.278em" => "thick".into(),
            "0.333em" => String::new(),
            "0.5em" => "#h(0.500em)".into(),
            "1.0em" => "quad".into(),
            "2.0em" => "#h(2em)".into(),
            width => format!("#h({width})"),
        },
        Exp::Text(style, text) => {
            let text = quote(text);
            style.map_or_else(
                || format!("upright({text})"),
                |style| styled(style.other, &text),
            )
        }
        Exp::Styled(style, items) => {
            let body = list(items);
            if style.upright_bold {
                format!("upright(bold({body}))")
            } else {
                styled(style.other, &body)
            }
        }
        Exp::Limits(body) | Exp::Display(_, body) => expression(body),
        Exp::Sized(size, body) => format!("#scale(x: {size}, y: {size})[${}$]", expression(body)),
        Exp::Enclose(notation, body) => {
            let body = expression(body);
            match *notation {
                "box" => format!("#box(stroke: black, inset: 3pt, [$ {body} $])"),
                "updiagonalstrike" => format!("cancel({body})"),
                "downdiagonalstrike" => format!("cancel({body}, inverted: #true)"),
                _ => format!("cancel({body}, cross: #true)"),
            }
        }
        Exp::Phantom(body) => format!("#hide[${}$]", expression(body)),
        Exp::Table { rows, .. } => table_rows(rows, " & ", "\\\n"),
    }
}

fn compound(exp: &Exp) -> bool {
    match exp {
        Exp::Group(items) => match items.as_slice() {
            [item] => compound(item),
            _ => true,
        },
        Exp::Limits(body) | Exp::Display(_, body) => compound(body),
        Exp::Table { .. } => true,
        _ => false,
    }
}

fn scripts(
    base: &Exp,
    mut sub: Option<&Exp>,
    mut sup: Option<&Exp>,
    base_text: Option<&str>,
) -> String {
    if let Exp::Accent { body, mark } = base {
        let script = match mark.mathml {
            "⏞" if sub.is_none() => sup.take().map(|exp| ("overbrace", exp)),
            "⏟" if sup.is_none() => sub.take().map(|exp| ("underbrace", exp)),
            _ => None,
        };
        if let Some((name, label)) = script {
            return format!("{name}({}, {})", expression(body), expression(label));
        }
    }
    let mut output = base_text.map_or_else(|| expression(base), str::to_owned);
    if base_text.is_none() && output.is_empty() {
        output.push_str("zws");
    }
    if let Some(Exp::Sym(_, text)) = sup {
        let count = prime_count(text);
        if count > 0 {
            output.push_str(&"'".repeat(count));
            sup = None;
        }
    }
    for (operator, script) in [('_', sub), ('^', sup)] {
        if let Some(script) = script {
            let value = expression(script);
            output.push(operator);
            if matches!(script, Exp::Num(..))
                || matches!(script, Exp::Sym(..) if value.chars().all(|c| c.is_alphanumeric() || c == '.') && value != ".")
            {
                output.push_str(&value);
            } else {
                output.push('(');
                output.push_str(&value);
                output.push(')');
            }
        }
    }
    output
}

fn accent(body: &Exp, mark: &str) -> String {
    let name = match mark {
        "\u{0300}" => "grave",
        "\u{0301}" => "acute",
        "\u{0302}" => "hat",
        "\u{0303}" => "tilde",
        "\u{0306}" => "breve",
        "\u{0307}" => "dot",
        "\u{0308}" => "dot.double",
        "\u{030a}" => "circle",
        "\u{030c}" => "caron",
        "‾" => "macron",
        "→" => "arrow",
        "¯" => "overline",
        "_" => "underline",
        "⏞" => "overbrace",
        "⏟" => "underbrace",
        "⎴" => "overbracket",
        "⎵" => return format!("{}_bracket.b", expression(body)),
        "⏜" => return format!("{}^paren.t", expression(body)),
        "⏝" => return format!("{}_paren.b", expression(body)),
        _ => "",
    };
    let value = expression(body);
    if name.is_empty()
        || (compound(body)
            && !matches!(
                name,
                "overline" | "underline" | "overbrace" | "underbrace" | "overbracket"
            ))
    {
        format!("accent({value}, {})", accent_char(mark))
    } else {
        format!("{name}({value})")
    }
}

fn table_rows(rows: &[Row], cell_sep: &str, row_sep: &str) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|cell| list(cell))
                .collect::<Vec<_>>()
                .join(cell_sep)
        })
        .collect::<Vec<_>>()
        .join(row_sep)
}

fn fenced(open: Option<&str>, body: &[Exp], close: Option<&str>) -> String {
    let pair = (open.unwrap_or_default(), close.unwrap_or_default());
    let matrix_delim = match pair {
        ("(", ")") => Some("("),
        ("[", "]") => Some("["),
        ("{", "}") => Some("{"),
        ("|", "|") => Some("|"),
        ("∣", "∣") | ("∥", "∥") | ("‖", "‖") => Some("||"),
        _ => None,
    };
    if let [Exp::Table { rows, aligns, .. }] = body {
        if pair == ("{", "")
            && matches!(aligns, Aligns::FirstRow(_))
            && rows.first().is_some_and(|row| row.len() > 1)
        {
            return format!("cases(delim: \"{{\", {})", table_rows(rows, " & ", ", "));
        }
        if let Some(delim) = matrix_delim {
            return format!("mat(delim: \"{delim}\", {})", table_rows(rows, ", ", "; "));
        }
    }
    if let [
        Exp::Frac {
            num,
            den,
            line: false,
        },
    ] = body
        && pair == ("(", ")")
    {
        return format!("binom({}, {})", expression(num), expression(den));
    }
    let value = list(body);
    match pair {
        ("(", ")")
        | ("[", "]")
        | ("{", "}")
        | ("⟨", "⟩")
        | ("⌊", "⌋")
        | ("⌈", "⌉")
        | ("∥", "∥") => format!("{}{value}{}", pair.0, pair.1),
        ("|", "|") | ("∣", "∣") => format!("lr(|{value}|)"),
        ("‖", "‖") => format!("lr(bar.v.double {value} bar.v.double)"),
        ("(", "]") | ("[", ")") => format!("lr({}{value}{})", pair.0, pair.1),
        (open, close) => format!(
            "{}{value}{}",
            unpaired_delimiter(open),
            unpaired_delimiter(close)
        ),
    }
}

fn unpaired_delimiter(text: &str) -> String {
    match text {
        "(" | ")" | "[" | "]" | "|" => format!("\\{text}"),
        _ => text.into(),
    }
}

fn styled(style: &str, body: &str) -> String {
    match style {
        "normal" => format!("upright({body})"),
        "bold" => format!("bold({body})"),
        "italic" => format!("italic({body})"),
        "bold-italic" => format!("bold(italic({body}))"),
        "double-struck" => format!("bb({body})"),
        "script" => format!("cal({body})"),
        "bold-script" => format!("bold(cal({body}))"),
        "fraktur" => format!("frak({body})"),
        "bold-fraktur" => format!("bold(frak({body}))"),
        "sans-serif" => format!("sans({body})"),
        "sans-serif-italic" => format!("italic(sans({body}))"),
        "monospace" => format!("mono({body})"),
        _ => body.into(),
    }
}

fn quote(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

fn symbol(text: &str) -> String {
    let primes = prime_count(text);
    if primes > 0 {
        return "'".repeat(primes);
    }
    match text {
        "(" | ")" | "[" | "]" | "|" | "," | ";" | "/" | "$" | "#" | "_" | "^" => {
            format!("\\{text}")
        }
        _ => crate::typst_symbols::name(text).unwrap_or(text).into(),
    }
}

fn builtin_function(name: &str) -> bool {
    name == "mod"
        || crate::symbols::SYMBOLS
            .iter()
            .any(|(_, class, text, _)| *class == Class::Func && *text == name)
}
