//! Clean-room R, from the R language definition and black-box runs of the
//! oracle. `R documentation` includes the context `level0`. A `)` or `}`
//! without its opening bracket is an error.

use super::{Ctx, Lang, M, Switch::*};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    Lang::new("R Script", "r", &["*.r", "*.R"])
        .weak_delims(".")
        .list("constants", CONSTANTS)
        .list("control flow", CONTROL_FLOW)
        .context("level0", Normal, code)
        // Inside the parentheses of a call, `name =` names an argument.
        .context("parenthesis", Normal, |c| {
            c.rule(M::Char(')'), Normal, Pop(1))
                .rule(
                    M::Re(r"(?:[A-Za-z]|\.(?![0-9]))[\w.]*\s*=(?!=)"),
                    Attribute,
                    Stay,
                )
                .include("level0")
        })
        .context("braces", Normal, |c| {
            c.rule(M::Char('}'), Normal, Pop(1)).include("level0")
        })
        .context("suffix", Normal, |c| {
            c.rule(M::AnyOf("Li"), DataType, Pop(1)).fallthrough(Pop(1))
        })
        .context("comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .include("##Comments")
                .line_end(Pop(1))
        })
        .context("string", String, |c| {
            c.rule(M::Re(ESCAPE), SpecialChar, Stay)
                .rule(M::Char('"'), String, Pop(1))
        })
        .context("string2", String, |c| {
            c.rule(M::Re(ESCAPE), SpecialChar, Stay)
                .rule(M::Char('\''), String, Pop(1))
        })
        .context("backquoted", Attribute, |c| {
            c.rule(M::Char('`'), String, Pop(1))
        })
        .build()
}

fn code(c: Ctx) -> Ctx {
    c.rule(M::Char('#'), Comment, Push("comment"))
        .rule(M::Char('"'), String, Push("string"))
        .rule(M::Char('\''), String, Push("string2"))
        .rule(M::Char('`'), String, Push("backquoted"))
        .rule(M::Char('('), Normal, Push("parenthesis"))
        .rule(M::Char('{'), Normal, Push("braces"))
        .rule(M::AnyOf(")}"), Error, Stay)
        .rule(M::Kw("control flow"), ControlFlow, Stay)
        .rule(M::Kw("constants"), Constant, Stay)
        .rule(
            M::Re(r"(?:[A-Za-z]|\.(?![0-9]))[\w.]*(?=\s*\()"),
            Function,
            Stay,
        )
        .rule(M::Re(r"(?:[A-Za-z]|\.(?![0-9]))[\w.]*"), Normal, Stay)
        .rule(M::Float, Float, Stay)
        .rule(M::Int, DecVal, Push("suffix"))
        .rule(M::Re(r"<<-|->>|<-|->|=(?!=)"), Other, Stay)
        .rule(
            M::Re(r"%[^%\n]*%|\|>|:::?|&&?|\|\|?|[=!<>]=|[-+*/^~!$@<>]"),
            SpecialChar,
            Stay,
        )
}

const ESCAPE: &str = concat!(
    r#"\\(?:x[0-9a-fA-F]{1,2}|[0-7]{1,3}|u\{?[0-9a-fA-F]{1,4}\}?"#,
    r#"|U\{?[0-9a-fA-F]{1,8}\}?|[abfnrtv'"`\\])"#
);

const CONSTANTS: &[&str] = &[
    "FALSE",
    "Inf",
    "NA",
    "NA_character_",
    "NA_integer_",
    "NA_real_",
    "NULL",
    "NaN",
    "TRUE",
];

const CONTROL_FLOW: &[&str] = &[
    "break", "else", "for", "function", "if", "in", "next", "repeat", "while",
];
