//! Eqn math output for the ms writer.

use std::fmt::Write as _;

use crate::{
    eqn_symbols::{builtin_function, name},
    render::{Unsupported, accent_char, has_limits, prime_count},
    tree::{Aligns, Class, Exp, Row},
};

pub(crate) fn render(items: &[Exp], display: bool) -> Result<String, Unsupported> {
    Ok(Eqn { display }.list(items))
}

struct Eqn {
    display: bool,
}

impl Eqn {
    fn list(&self, items: &[Exp]) -> String {
        let mut output = String::new();
        for (index, item) in items.iter().enumerate() {
            if matches!(item, Exp::Space(space) if space.width == "0.333em") {
                continue;
            }
            if index > 0 && !output.is_empty() {
                output.push(' ');
            }
            output.push_str(&self.expression(item));
        }
        output
    }

    fn expression(&self, exp: &Exp) -> String {
        match exp {
            Exp::Num(text) => symbol(text),
            Exp::Sym(Class::Func | Class::FuncLimits, text) if !builtin_function(text) => {
                quote(text)
            }
            Exp::Sym(Class::Text, text) => format!("roman {}", quote(text)),
            Exp::Sym(Class::Op, text) if operator_text(text) => {
                format!("roman{{{}}}", quote(&format!("{text} ")))
            }
            Exp::Sym(_, text) => symbol(text),
            Exp::Group(items) => group(&self.list(items)),
            Exp::PaddedText(items) => self.list(items),
            Exp::Scripts {
                base,
                sub,
                sup,
                limits,
            } => {
                let stacked = limits.unwrap_or_else(|| {
                    matches!(base.as_ref(), Exp::Accent { mark, .. } if matches!(mark.mathml, "⏞" | "⏟"))
                        || (self.display && has_limits(base))
                });
                self.scripts(base, sub.as_deref(), sup.as_deref(), stacked)
            }
            Exp::Stack { base, under, over } => {
                self.scripts(base, under.as_deref(), over.as_deref(), true)
            }
            Exp::Frac { num, den, line } => {
                let operator = if *line { "over" } else { "/" };
                format!("{} {operator} {}", self.argument(num), self.argument(den))
            }
            Exp::Sqrt(body) => format!("sqrt {}", self.argument(body)),
            Exp::Root(body, index) => format!(
                "\"\" sup {} sqrt {}",
                self.argument(index),
                self.argument(body)
            ),
            Exp::Fenced { open, body, close } => {
                let open = open.as_deref().unwrap_or("\"\"");
                let close = close.as_deref().unwrap_or("\"\"");
                format!("left {open} {} right {close}", self.list(body))
            }
            Exp::Middle(text) => quote(text),
            Exp::Accent { body, mark } => {
                let operator = if mark.under { "from" } else { "to" };
                format!(
                    "{} {operator} {}",
                    self.argument(body),
                    symbol(accent_char(mark.mathml))
                )
            }
            Exp::Space(space) => match space.width {
                "0.167em" => "^".into(),
                "0.222em" | "0.278em" => "~".into(),
                "-0.167em" => "back 16".into(),
                "0.333em" => String::new(),
                width => {
                    let em = width
                        .trim_end_matches("em")
                        .parse::<f64>()
                        .unwrap_or_default();
                    format!("fwd {}", (em * 100.0).trunc())
                }
            },
            Exp::Text(style, text) => {
                let text = quote(text);
                match style.map(|style| style.other) {
                    None | Some("normal") => format!("roman {text}"),
                    Some("bold") => format!("bold {text}"),
                    _ => text,
                }
            }
            Exp::Styled(style, items) => {
                let body = group(&self.list(items));
                if style.upright_bold {
                    format!("roman {{bold {body}}}")
                } else {
                    match style.other {
                        "bold" => format!("bold {body}"),
                        "italic" => format!("italic {body}"),
                        "bold-italic" => format!("bold italic {body}"),
                        "normal" => format!("roman {body}"),
                        _ => body,
                    }
                }
            }
            Exp::Limits(body)
            | Exp::Sized(_, body)
            | Exp::Display(_, body)
            | Exp::Enclose(_, body) => self.expression(body),
            Exp::Phantom(body) => format!("hphantom {}", self.argument(body)),
            Exp::Table { aligns, rows, .. } => self.table(aligns, rows),
        }
    }

    fn argument(&self, exp: &Exp) -> String {
        if let Exp::Limits(body) | Exp::Sized(_, body) | Exp::Display(_, body) = exp {
            return self.argument(body);
        }
        let value = self.expression(exp);
        match exp {
            Exp::Sym(Class::Op, text) if operator_text(text) => group(&value),
            Exp::Sym(..) | Exp::Num(..) | Exp::Space(..) if !value.is_empty() => value,
            Exp::Group(..) => value,
            Exp::Styled(style, _)
                if !style.upright_bold
                    && !matches!(style.other, "normal" | "bold" | "italic" | "bold-italic") =>
            {
                value
            }
            _ => group(&value),
        }
    }

    fn scripts(
        &self,
        base: &Exp,
        sub: Option<&Exp>,
        mut sup: Option<&Exp>,
        stacked: bool,
    ) -> String {
        let mut output = if matches!(base, Exp::Sym(_, text) if text.is_empty()) {
            String::new()
        } else {
            self.argument(base)
        };
        let primes = match sup {
            Some(Exp::Sym(_, text)) => prime_count(text),
            _ => 0,
        };
        if primes > 0 {
            sup = None;
        }
        let (lower, upper) = if stacked {
            ("from", "to")
        } else {
            ("sub", "sup")
        };
        for (operator, value) in [(lower, sub), (upper, sup)] {
            if let Some(exp) = value {
                let _ = write!(output, " {operator} {}", self.argument(exp));
            }
        }
        output.push_str(&"'".repeat(primes));
        output
    }

    fn table(&self, aligns: &Aligns, rows: &[Row]) -> String {
        let count = match aligns {
            Aligns::Columns(columns) => columns
                .len()
                .min(rows.iter().map(Vec::len).max().unwrap_or_default()),
            Aligns::Pairs | Aligns::Cycle(_) => rows.iter().map(Vec::len).max().unwrap_or_default(),
            _ => rows.first().map_or(0, Vec::len),
        };
        let mut output = String::from("matrix{\n");
        for column in 0..count {
            let align = match super::render::column_alignment(aligns, column, rows) {
                "left" => 'l',
                "right" => 'r',
                _ => 'c',
            };
            let _ = write!(output, "{align}col{{ ");
            for (index, cell) in rows.iter().filter_map(|row| row.get(column)).enumerate() {
                if index > 0 {
                    output.push_str(" above ");
                }
                match cell.as_slice() {
                    [item] => output.push_str(&self.argument(item)),
                    items => output.push_str(&group(&self.list(items))),
                }
            }
            output.push_str(" }\n");
        }
        output.push('}');
        output
    }
}

fn group(text: &str) -> String {
    if text.is_empty() {
        "{\"\"}".into()
    } else {
        format!("{{{text}}}")
    }
}

fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\[rs]").replace('"', "\\\""))
}

fn symbol(text: &str) -> String {
    if let Some(spelling) = name(text) {
        return spelling.into();
    }
    let mut output = String::new();
    for c in text.chars() {
        if c.is_ascii() {
            output.push(c);
        } else {
            let _ = write!(output, "\\[u{:04X}]", u32::from(c));
        }
    }
    output
}

fn operator_text(text: &str) -> bool {
    text.len() > 1 && text.is_ascii() && text.chars().any(|c| c.is_ascii_alphanumeric())
}
